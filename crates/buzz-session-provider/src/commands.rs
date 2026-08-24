//! Decoding, addressing, fencing, and freshness for the two operator-authored
//! command kinds (44220 turn, 44221 lifecycle).
//!
//! Everything in this module is a pure decision over already-fetched state: it
//! returns *what to do*, never does it. That is what makes the fencing rules —
//! the parts that must never regress — cheap to test exhaustively.
//!
//! Both 44220 actions (`thread.turn.start`, `thread.turn.interrupt`) decode
//! through the shared `buzz_core::coding_session_command` type, so the
//! provider and the relay can never disagree about what is valid.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use uuid::Uuid;

use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionTarget,
};
use buzz_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
};
use buzz_core::coding_session_runtime::RuntimeDescriptor;

use crate::payload::{
    PROJECT_CWD_UNRESOLVED, PROVIDER_UNAVAILABLE, SESSION_CLOSED, SESSION_LIMIT, STALE_GENERATION,
    UNAUTHORIZED_OPERATOR, UNKNOWN_TARGET,
};
use crate::state::StateStore;

/// Why a command produced no side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ignored {
    /// The command names a different provider authority or instance.
    NotAddressed,
    /// This `commandId` was already consumed.
    AlreadyConsumed,
    /// Older than the freshness horizon and never seen — history, not intent.
    PastHorizon,
    /// Content did not decode against the contract.
    Malformed(String),
    /// No live session matches the addressed target.
    UnknownTarget,
    /// The addressed generation is not the one this provider is running.
    StaleGeneration,
    /// The addressed session has been retired.
    SessionClosed,
}

/// What to do about one 44221 lifecycle command.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleDecision {
    /// Do nothing.
    Ignore(Ignored),
    /// Publish a `failed` receipt with this code.
    Fail {
        /// The command being answered.
        command_id: String,
        /// Stable receipt error code.
        code: &'static str,
        /// Operator-facing detail.
        message: String,
    },
    /// Create a session.
    Create(Box<CreatePlan>),
    /// Reattach a disconnected session as a new generation.
    Resume(ResumePlan),
    /// Durably stop a session.
    Stop(StopPlan),
}

/// A validated, resolved create request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePlan {
    /// The create command being answered.
    pub command_id: String,
    /// The matched runtime's `instance_ref` — which descriptor serves this
    /// create.
    pub runtime_instance_ref: String,
    /// Channel every event for the new session is published into.
    pub channel_id: Uuid,
    /// Host-local working directory. Never appears in signed content.
    pub cwd: PathBuf,
    /// NIP-MP project coordinate, or `None` for a standalone session.
    pub project_ref: Option<String>,
    /// Repository coordinate, or `None`.
    pub repo_ref: Option<String>,
    /// Umbrella session reference from the create, or `None` when unclaimed.
    pub session_ref: Option<String>,
    /// Explicit genesis event id to resolve before execution starts.
    pub genesis_ref: Option<String>,
    /// Locally witnessed create signer, replaced by the explicitly referenced
    /// genesis signer for authority-aware creates.
    pub founder_pubkey: String,
    /// Requested model, or `None`.
    pub model: Option<String>,
    /// Operator-facing title, or `None`.
    pub title: Option<String>,
    /// First turn to deliver after creation, or `None`.
    pub initial_turn: Option<String>,
}

/// A validated request to reattach one exact prior generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePlan {
    /// Lifecycle command being answered.
    pub command_id: String,
    /// Channel the execution belongs to.
    pub channel_id: Uuid,
    /// Exact disconnected generation the operator observed.
    pub target: CodingSessionTarget,
}

/// A validated request to durably stop one exact current generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopPlan {
    /// Lifecycle command being answered.
    pub command_id: String,
    /// Channel the execution belongs to.
    pub channel_id: Uuid,
    /// Exact current generation the operator observed.
    pub target: CodingSessionTarget,
}

/// What to do about one 44220 turn command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnDecision {
    /// Do nothing.
    Ignore(Ignored),
    /// Refuse visibly because the signer lacks session authority.
    Fail {
        /// The command being answered.
        command_id: String,
        /// Operator-facing detail.
        message: String,
    },
    /// Deliver a prompt to a live session.
    Start {
        /// The command being answered.
        command_id: String,
        /// The fenced target.
        target: CodingSessionTarget,
        /// Operator-entered turn text.
        text: String,
    },
    /// Cancel the in-flight turn of a live session.
    Interrupt {
        /// The command being answered.
        command_id: String,
        /// The fenced target.
        target: CodingSessionTarget,
    },
}

/// Read-only view of everything a decision depends on.
pub struct CommandContext<'a> {
    /// This provider's signing pubkey, lowercase hex.
    pub provider_pubkey: &'a str,
    /// Pubkey that signed the command currently being decided.
    pub operator_pubkey: &'a str,
    /// Every runtime this provider offers.
    pub runtimes: &'a [RuntimeDescriptor],
    /// Instance id in every `cs-target` this provider mints.
    pub instance_id: &'a str,
    /// Current wall clock, seconds since the Unix epoch.
    pub now_secs: u64,
    /// Freshness horizon in seconds.
    pub horizon_secs: u64,
    /// Ceiling on concurrently live sessions.
    pub max_sessions: usize,
    /// Number of adapter actors currently attached in this process.
    pub active_session_count: usize,
    /// Durable state: dedupe ledger and session records.
    pub state: &'a StateStore,
    /// Host-local working-directory map, freshly read.
    pub projects: &'a ProjectsFile,
}

impl CommandContext<'_> {
    fn past_horizon(&self, created_at: u64) -> bool {
        created_at + self.horizon_secs < self.now_secs
    }
}

/// Decide what a 44221 lifecycle command means for this provider.
pub fn decide_lifecycle(
    context: &CommandContext<'_>,
    channel_id: Uuid,
    created_at: u64,
    content: &str,
) -> LifecycleDecision {
    let payload = match decode_coding_session_lifecycle_command(content) {
        Ok(payload) => payload,
        Err(error) => return LifecycleDecision::Ignore(Ignored::Malformed(error)),
    };
    let provider_authority_pubkey = match &payload.action {
        CodingSessionLifecycleAction::SessionCreate {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionResume {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionStop {
            provider_authority_pubkey,
            ..
        } => provider_authority_pubkey,
    };

    // Addressing before dedupe: a command for another adapter must not consume
    // an id in *this* adapter's ledger, or a later legitimate reuse would be
    // silently dropped.
    if !provider_authority_pubkey.eq_ignore_ascii_case(context.provider_pubkey) {
        return LifecycleDecision::Ignore(Ignored::NotAddressed);
    }
    if context.state.is_command_consumed(&payload.command_id) {
        return LifecycleDecision::Ignore(Ignored::AlreadyConsumed);
    }
    if context.past_horizon(created_at) {
        return LifecycleDecision::Ignore(Ignored::PastHorizon);
    }

    if let CodingSessionLifecycleAction::SessionResume { session, .. }
    | CodingSessionLifecycleAction::SessionStop { session, .. } = &payload.action
    {
        if session.instance_id != context.instance_id {
            return LifecycleDecision::Ignore(Ignored::NotAddressed);
        }
        let Some(record) = context.state.session(&session.session_id) else {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: UNKNOWN_TARGET,
                message: "this provider has no record of the addressed execution".into(),
            };
        };
        if record.channel_id != channel_id || record.driver != session.driver {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: UNKNOWN_TARGET,
                message:
                    "the addressed execution does not belong to this channel and provider runtime"
                        .into(),
            };
        }
        if record.generation != session.generation {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: STALE_GENERATION,
                message: format!(
                    "the addressed execution generation {} is stale; the current generation is {}",
                    session.generation, record.generation
                ),
            };
        }
        if !operator_owns_session(record, context.operator_pubkey) {
            return LifecycleDecision::Fail {
                command_id: payload.command_id,
                code: UNAUTHORIZED_OPERATOR,
                message: "only the session founder may stop or resume this execution".into(),
            };
        }

        return match &payload.action {
            CodingSessionLifecycleAction::SessionResume { .. } if record.closed => {
                LifecycleDecision::Fail {
                    command_id: payload.command_id,
                    code: SESSION_CLOSED,
                    message: "the addressed execution was already durably stopped".into(),
                }
            }
            CodingSessionLifecycleAction::SessionResume { .. } => {
                LifecycleDecision::Resume(ResumePlan {
                    command_id: payload.command_id,
                    channel_id,
                    target: session.clone(),
                })
            }
            CodingSessionLifecycleAction::SessionStop { .. } => LifecycleDecision::Stop(StopPlan {
                command_id: payload.command_id,
                channel_id,
                target: session.clone(),
            }),
            CodingSessionLifecycleAction::SessionCreate { .. } => unreachable!(),
        };
    }

    let CodingSessionLifecycleAction::SessionCreate {
        project_ref,
        repo_ref,
        session_ref,
        genesis_ref,
        provider_instance_ref,
        provider_authority_pubkey: _,
        model,
        title,
        initial_turn,
    } = &payload.action
    else {
        unreachable!()
    };

    // The command is addressed to *this* signer, so no other process will ever
    // answer it. A ref naming no descriptor therefore fails loudly — silence
    // would strand the consumer's durable create forever.
    if !context
        .runtimes
        .iter()
        .any(|descriptor| &descriptor.instance_ref == provider_instance_ref)
    {
        let mut offered: Vec<&str> = context
            .runtimes
            .iter()
            .map(|descriptor| descriptor.instance_ref.as_str())
            .collect();
        offered.sort_unstable();
        return LifecycleDecision::Fail {
            command_id: payload.command_id.clone(),
            code: PROVIDER_UNAVAILABLE,
            message: format!(
                "unknown providerInstanceRef {provider_instance_ref:?}; this provider offers: {}",
                offered.join(", ")
            ),
        };
    }

    if context.max_sessions != crate::config::UNLIMITED_MAX_SESSIONS
        && context.active_session_count >= context.max_sessions
    {
        // The count is live adapter *processes* on this one provider, not
        // durable sessions and nothing to do with the model vendor's own
        // limits — a distinction the old sentence left to the reader, who
        // reasonably read "maximum of 4 session(s)" as an account limit
        // (reported 2026-08-24). Say whose cap it is and what clears it.
        return LifecycleDecision::Fail {
            command_id: payload.command_id.clone(),
            code: SESSION_LIMIT,
            message: format!(
                "this provider already holds its maximum of {} running agent process(es); stop an execution you are finished with to free a slot, or set BUZZ_CSP_MAX_SESSIONS to raise the cap",
                context.max_sessions
            ),
        };
    }

    let Some(cwd) =
        context
            .projects
            .resolve(&payload.command_id, project_ref.as_deref(), channel_id)
    else {
        return LifecycleDecision::Fail {
            command_id: payload.command_id.clone(),
            code: PROJECT_CWD_UNRESOLVED,
            message: match project_ref {
                Some(project_ref) => format!(
                    "no working directory is configured for project {project_ref} or channel {channel_id}"
                ),
                None => format!("no working directory is configured for channel {channel_id}"),
            },
        };
    };

    LifecycleDecision::Create(Box::new(CreatePlan {
        command_id: payload.command_id.clone(),
        runtime_instance_ref: provider_instance_ref.clone(),
        channel_id,
        cwd,
        project_ref: project_ref.clone(),
        repo_ref: repo_ref.clone(),
        session_ref: session_ref.clone(),
        genesis_ref: genesis_ref.clone(),
        founder_pubkey: context.operator_pubkey.to_owned(),
        model: model.clone(),
        title: title.clone(),
        initial_turn: initial_turn.clone(),
    }))
}

/// Decide what a 44220 turn command means for this provider.
pub fn decide_turn(context: &CommandContext<'_>, created_at: u64, content: &str) -> TurnDecision {
    let command = match decode_turn_command(content) {
        Ok(command) => command,
        Err(error) => return TurnDecision::Ignore(Ignored::Malformed(error)),
    };

    // Any driver this provider's runtimes mint is acceptable; the session id
    // (a UUID) plus generation fence everything downstream, so two runtimes
    // sharing a driver slug cannot misroute a turn.
    let known_driver = context
        .runtimes
        .iter()
        .any(|descriptor| descriptor.driver == command.target.driver);
    if !known_driver || command.target.instance_id != context.instance_id {
        return TurnDecision::Ignore(Ignored::NotAddressed);
    }
    if context.state.is_command_consumed(&command.command_id) {
        return TurnDecision::Ignore(Ignored::AlreadyConsumed);
    }
    if context.past_horizon(created_at) {
        return TurnDecision::Ignore(Ignored::PastHorizon);
    }

    let Some(record) = context.state.session(&command.target.session_id) else {
        return TurnDecision::Ignore(Ignored::UnknownTarget);
    };
    if record.generation != command.target.generation {
        return TurnDecision::Ignore(Ignored::StaleGeneration);
    }
    if !operator_may_steer(record, context.operator_pubkey) {
        return TurnDecision::Fail {
            command_id: command.command_id,
            message: "only the session founder or a granted operator may steer this execution"
                .into(),
        };
    }
    if record.closed {
        return TurnDecision::Ignore(Ignored::SessionClosed);
    }

    match command.action {
        TurnAction::Start { text } => TurnDecision::Start {
            command_id: command.command_id,
            target: command.target,
            text,
        },
        TurnAction::Interrupt => TurnDecision::Interrupt {
            command_id: command.command_id,
            target: command.target,
        },
    }
}

/// Owner-only authority: stop/resume/end. Checks only authority facts
/// persisted when this provider witnessed the create. Old no-genesis records
/// predate that field and remain ungoverned; genesis-bearing records can
/// never fall open when their founder is absent. `grant-operator` never moves
/// ownership, so the granted-operator set is deliberately not consulted here.
fn operator_owns_session(record: &crate::state::SessionRecord, operator_pubkey: &str) -> bool {
    match record.founder_pubkey.as_deref() {
        Some(founder) => founder == operator_pubkey,
        None => record.genesis_ref.is_none(),
    }
}

/// Steering authority: turn start/interrupt. The owner always may; beyond
/// that, only a genesis-bearing session consults its verified
/// granted-operator cache (each entry applied from a relay-signed acceptance
/// receipt plus the resolved accepted transition — see [`crate::authority`]).
/// Legacy no-genesis sessions never gain operators this way (R20): umbrella
/// authority for them arrives by adoption, not provider inference.
fn operator_may_steer(record: &crate::state::SessionRecord, operator_pubkey: &str) -> bool {
    if operator_owns_session(record, operator_pubkey) {
        return true;
    }
    record.genesis_ref.is_some() && record.granted_operators.contains(operator_pubkey)
}

/// A decoded 44220 payload, covering both donor actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCommand {
    /// Client-generated idempotency key.
    pub command_id: String,
    /// The exact generation addressed.
    pub target: CodingSessionTarget,
    /// The requested action.
    pub action: TurnAction,
}

/// The two turn actions the donor contract defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAction {
    /// Start a turn with operator-entered text.
    Start {
        /// The prompt.
        text: String,
    },
    /// Cancel the in-flight turn.
    Interrupt,
}

/// Strictly decode a 44220 payload of either action.
pub fn decode_turn_command(content: &str) -> Result<TurnCommand, String> {
    let payload: CodingSessionCommandPayload = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session command payload: {error}"))?;
    payload.validate()?;
    let action = match payload.action {
        CodingSessionAction::ThreadTurnStart { text } => TurnAction::Start { text },
        CodingSessionAction::ThreadTurnInterrupt => TurnAction::Interrupt,
    };
    Ok(TurnCommand {
        command_id: payload.command_id,
        target: payload.target,
        action,
    })
}

/// Host-local map from session coordinates to working directories.
///
/// This file is the entire seam between signed intent and the machine a session
/// actually runs on. It is written by the desktop host, re-read on every
/// lifecycle command (so an operator can fix a missing entry and republish
/// without restarting the provider), and never travels anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectsFile {
    /// Format version. Reserved; unknown versions are still read best-effort.
    pub version: u32,
    /// One-shot hint keyed by `commandId`, for a directory chosen at create time.
    pub pending: BTreeMap<String, PathBuf>,
    /// Directory per NIP-MP project coordinate.
    pub projects: BTreeMap<String, PathBuf>,
    /// Fallback directory per channel.
    pub channels: BTreeMap<Uuid, PathBuf>,
}

impl ProjectsFile {
    /// Read the file, treating every failure as "no entries".
    ///
    /// A missing or malformed projects file must not take the provider down: it
    /// degrades every affected create into a `PROJECT_CWD_UNRESOLVED` receipt,
    /// which is a message the operator can act on, unlike a dead process.
    pub fn load(path: Option<&Path>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let body = match std::fs::read_to_string(path) {
            Ok(body) => body,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                tracing::warn!(target: "csp::projects", "cannot read {}: {error}", path.display());
                return Self::default();
            }
        };
        match serde_json::from_str(&body) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(target: "csp::projects", "cannot parse {}: {error}", path.display());
                Self::default()
            }
        }
    }

    /// Resolve a working directory: pending hint, then project, then channel.
    ///
    /// A configured path that is not an existing absolute directory resolves to
    /// `None` — the same outcome as no entry at all — because handing a relative
    /// or missing path to `session/new` fails later and less legibly.
    pub fn resolve(
        &self,
        command_id: &str,
        project_ref: Option<&str>,
        channel_id: Uuid,
    ) -> Option<PathBuf> {
        let candidates = [
            self.pending.get(command_id),
            project_ref.and_then(|project_ref| self.projects.get(project_ref)),
            self.channels.get(&channel_id),
        ];
        candidates
            .into_iter()
            .flatten()
            .find(|path| usable_directory(path))
            .cloned()
    }

    /// Project coordinates advertised in the catalog's optional `projects[]`.
    pub fn project_refs(&self) -> impl Iterator<Item = &str> {
        self.projects.keys().map(String::as_str)
    }
}

fn usable_directory(path: &Path) -> bool {
    if !path.is_absolute() {
        tracing::warn!(target: "csp::projects", "ignoring relative working directory {}", path.display());
        return false;
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => true,
        Ok(_) => {
            tracing::warn!(target: "csp::projects", "ignoring non-directory working directory {}", path.display());
            false
        }
        Err(_) => {
            tracing::warn!(target: "csp::projects", "ignoring missing working directory {}", path.display());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{SessionRecord, StateStore};

    const AUTHORITY: &str = "ab00000000000000000000000000000000000000000000000000000000000000";

    fn store(dir: &Path) -> StateStore {
        StateStore::open(dir, 86_400).expect("state")
    }

    fn runtime(instance_ref: &str, driver: &str, runtime: &str) -> RuntimeDescriptor {
        RuntimeDescriptor {
            instance_ref: instance_ref.to_owned(),
            driver: driver.to_owned(),
            runtime: runtime.to_owned(),
            agent_command: driver.to_owned(),
            agent_args: Vec::new(),
            cli_env: None,
            default_model: "default".into(),
            allowed_models: vec!["default".into()],
            discover_models: false,
            capabilities: None,
        }
    }

    fn runtimes() -> &'static [RuntimeDescriptor] {
        static RUNTIMES: std::sync::OnceLock<Vec<RuntimeDescriptor>> = std::sync::OnceLock::new();
        RUNTIMES.get_or_init(|| {
            vec![
                runtime("claude-primary", "claude-agent-acp", "claude"),
                runtime("codex-primary", "codex-acp", "codex"),
            ]
        })
    }

    fn ctx<'a>(
        state: &'a StateStore,
        projects: &'a ProjectsFile,
        now_secs: u64,
    ) -> CommandContext<'a> {
        ctx_as(state, projects, now_secs, AUTHORITY)
    }

    fn ctx_as<'a>(
        state: &'a StateStore,
        projects: &'a ProjectsFile,
        now_secs: u64,
        operator_pubkey: &'a str,
    ) -> CommandContext<'a> {
        CommandContext {
            provider_pubkey: AUTHORITY,
            operator_pubkey,
            runtimes: runtimes(),
            instance_id: "instance-1",
            now_secs,
            horizon_secs: 86_400,
            max_sessions: 4,
            active_session_count: state.live_session_count(),
            state,
            projects,
        }
    }

    fn create_content(command_id: &str, project_ref: &str, authority: &str) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"{command_id}","action":{{"type":"session.create","projectRef":{project_ref},"repoRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{authority}","model":"claude-sonnet-4-6","title":"Ship it","initialTurn":"go"}}}}"#
        )
    }

    fn turn_content(command_id: &str, session_id: &str, generation: u64) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-command/v1","commandId":"{command_id}","target":{{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"{session_id}","generation":{generation}}},"action":{{"type":"thread.turn.start","text":"do the thing"}}}}"#
        )
    }

    fn interrupt_content(command_id: &str, session_id: &str, generation: u64) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-command/v1","commandId":"{command_id}","target":{{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"{session_id}","generation":{generation}}},"action":{{"type":"thread.turn.interrupt"}}}}"#
        )
    }

    fn lifecycle_target_content(
        action: &str,
        command_id: &str,
        session_id: &str,
        generation: u64,
    ) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"{command_id}","action":{{"type":"{action}","session":{{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"{session_id}","generation":{generation}}},"providerAuthorityPubkey":"{AUTHORITY}"}}}}"#
        )
    }

    fn session(session_id: &str, cwd: &Path) -> SessionRecord {
        SessionRecord {
            session_id: session_id.to_owned(),
            generation: 1,
            channel_id: Uuid::nil(),
            command_id: "create-1".into(),
            generation_command_id: None,
            provider_instance_ref: "claude-primary".into(),
            runtime: "claude".into(),
            driver: "claude-agent-acp".into(),
            cwd: cwd.to_path_buf(),
            project_ref: None,
            repo_ref: None,
            session_ref: None,
            genesis_ref: None,
            founder_pubkey: Some(AUTHORITY.into()),
            granted_operators: std::collections::BTreeSet::new(),
            granted_viewers: std::collections::BTreeSet::new(),
            authority_seq: 0,
            model: None,
            resume_cursor: None,
            title: None,
            created_at_ms: 0,
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
        }
    }

    fn projects_with_channel(channel_id: Uuid, cwd: &Path) -> ProjectsFile {
        ProjectsFile {
            version: 1,
            channels: [(channel_id, cwd.to_path_buf())].into_iter().collect(),
            ..ProjectsFile::default()
        }
    }

    #[test]
    fn a_create_for_another_authority_is_ignored_without_consuming_the_command_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let context = ctx(&state, &projects, 1_000);
        let other = "cd".repeat(32);
        assert_eq!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", &other)
            ),
            LifecycleDecision::Ignore(Ignored::NotAddressed)
        );
    }

    /// Authority matched but the ref names no descriptor: nobody else will ever
    /// answer this command, so it fails loudly instead of being ignored.
    #[test]
    fn a_create_for_an_unknown_instance_ref_fails_with_provider_unavailable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let context = ctx(&state, &projects, 1_000);
        let content = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"providerInstanceRef":"ghost-primary","providerAuthorityPubkey":"{AUTHORITY}","model":null,"title":null,"initialTurn":null}}}}"#
        );
        match decide_lifecycle(&context, Uuid::nil(), 1_000, &content) {
            LifecycleDecision::Fail {
                command_id,
                code,
                message,
            } => {
                assert_eq!(command_id, "create-1");
                assert_eq!(code, PROVIDER_UNAVAILABLE);
                assert_eq!(
                    message,
                    "unknown providerInstanceRef \"ghost-primary\"; this provider offers: \
                     claude-primary, codex-primary"
                );
            }
            other => panic!("expected a failure receipt, got {other:?}"),
        }
    }

    /// Any driver in the runtime set is addressable; the session UUID does the
    /// rest of the routing.
    #[test]
    fn turns_for_any_offered_driver_are_addressed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        let codex_turn = r#"{"schema":"buzz-coding-session-command/v1","commandId":"turn-1","target":{"driver":"codex-acp","instanceId":"instance-1","sessionId":"s1","generation":1},"action":{"type":"thread.turn.start","text":"go"}}"#;
        assert!(matches!(
            decide_turn(&context, 1_000, codex_turn),
            TurnDecision::Start { .. }
        ));

        let alien_turn = r#"{"schema":"buzz-coding-session-command/v1","commandId":"turn-2","target":{"driver":"someone-elses-acp","instanceId":"instance-1","sessionId":"s1","generation":1},"action":{"type":"thread.turn.start","text":"go"}}"#;
        assert_eq!(
            decide_turn(&context, 1_000, alien_turn),
            TurnDecision::Ignore(Ignored::NotAddressed)
        );
    }

    #[test]
    fn a_create_resolves_its_working_directory_in_pending_project_channel_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pending_dir = dir.path().join("pending");
        let project_dir = dir.path().join("project");
        let channel_dir = dir.path().join("channel");
        for path in [&pending_dir, &project_dir, &channel_dir] {
            std::fs::create_dir_all(path).expect("mkdir");
        }
        let channel = Uuid::new_v4();
        let project_ref = format!("30621:{}:demo", "cd".repeat(32));
        let state = store(dir.path());

        let all = ProjectsFile {
            version: 1,
            pending: [("create-1".to_owned(), pending_dir.clone())]
                .into_iter()
                .collect(),
            projects: [(project_ref.clone(), project_dir.clone())]
                .into_iter()
                .collect(),
            channels: [(channel, channel_dir.clone())].into_iter().collect(),
        };
        let content = create_content("create-1", &format!("\"{project_ref}\""), AUTHORITY);

        let resolved = |projects: &ProjectsFile| match decide_lifecycle(
            &ctx(&state, projects, 1_000),
            channel,
            1_000,
            &content,
        ) {
            LifecycleDecision::Create(plan) => plan.cwd,
            other => panic!("expected a create plan, got {other:?}"),
        };
        assert_eq!(resolved(&all), pending_dir);

        let without_pending = ProjectsFile {
            pending: BTreeMap::new(),
            ..all.clone()
        };
        assert_eq!(resolved(&without_pending), project_dir);

        let channel_only = ProjectsFile {
            pending: BTreeMap::new(),
            projects: BTreeMap::new(),
            ..all.clone()
        };
        assert_eq!(resolved(&channel_only), channel_dir);
    }

    /// Fork amendment: a create may claim an umbrella via `sessionRef`. The
    /// plan carries it verbatim; the historical 8-key form plans `None` — the
    /// exact behavior of every create before the field existed.
    #[test]
    fn a_create_plans_the_session_ref_it_claimed_and_none_otherwise() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let context = ctx(&state, &projects, 1_000);

        let umbrella = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let claiming = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":"{umbrella}","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{AUTHORITY}","model":null,"title":null,"initialTurn":null}}}}"#
        );
        match decide_lifecycle(&context, Uuid::nil(), 1_000, &claiming) {
            LifecycleDecision::Create(plan) => {
                assert_eq!(plan.session_ref.as_deref(), Some(umbrella));
            }
            other => panic!("expected a create plan, got {other:?}"),
        }

        match decide_lifecycle(
            &context,
            Uuid::nil(),
            1_000,
            &create_content("create-2", "null", AUTHORITY),
        ) {
            LifecycleDecision::Create(plan) => assert!(plan.session_ref.is_none()),
            other => panic!("expected a create plan, got {other:?}"),
        }
    }

    #[test]
    fn an_unresolvable_working_directory_fails_the_create_rather_than_guessing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);
        match decide_lifecycle(
            &context,
            Uuid::new_v4(),
            1_000,
            &create_content("create-1", "null", AUTHORITY),
        ) {
            LifecycleDecision::Fail { code, .. } => assert_eq!(code, PROJECT_CWD_UNRESOLVED),
            other => panic!("expected a failure receipt, got {other:?}"),
        }
    }

    /// A relative path, a missing directory, or a file where a directory was
    /// expected are all indistinguishable from "unconfigured" as far as the
    /// operator's next action goes — the create must fail legibly, not later.
    #[test]
    fn unusable_configured_paths_are_treated_as_unconfigured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").expect("write");
        let channel = Uuid::new_v4();

        for path in [
            PathBuf::from("relative/path"),
            dir.path().join("does-not-exist"),
            file,
        ] {
            let projects = projects_with_channel(channel, &path);
            assert!(projects.resolve("create-1", None, channel).is_none());
        }
    }

    #[test]
    fn a_replayed_create_produces_no_second_side_effect() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state.consume_command("create-1", 1_000).expect("consume");
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        assert_eq!(
            decide_lifecycle(
                &ctx(&state, &projects, 1_000),
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", AUTHORITY)
            ),
            LifecycleDecision::Ignore(Ignored::AlreadyConsumed)
        );
    }

    #[test]
    fn resume_and_stop_are_fenced_to_the_exact_persisted_generation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        assert!(matches!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.resume", "resume-1", "s1", 1),
            ),
            LifecycleDecision::Resume(ResumePlan { target, .. }) if target.generation == 1
        ));
        assert!(matches!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-1", "s1", 1),
            ),
            LifecycleDecision::Stop(StopPlan { target, .. }) if target.generation == 1
        ));
        for (action, command_id) in [
            ("session.resume", "resume-stale"),
            ("session.stop", "stop-stale"),
        ] {
            match decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content(action, command_id, "s1", 2),
            ) {
                LifecycleDecision::Fail {
                    command_id: actual_command_id,
                    code,
                    message,
                } => {
                    assert_eq!(actual_command_id, command_id);
                    assert_eq!(code, STALE_GENERATION);
                    assert_eq!(
                        message,
                        "the addressed execution generation 2 is stale; the current generation is 1"
                    );
                }
                other => panic!("expected a stale-generation receipt, got {other:?}"),
            }
        }
    }

    #[test]
    fn addressed_unknown_lifecycle_targets_fail_loudly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        for (action, command_id) in [
            ("session.resume", "resume-unknown"),
            ("session.stop", "stop-unknown"),
        ] {
            match decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content(action, command_id, "missing", 1),
            ) {
                LifecycleDecision::Fail {
                    command_id: actual_command_id,
                    code,
                    message,
                } => {
                    assert_eq!(actual_command_id, command_id);
                    assert_eq!(code, UNKNOWN_TARGET);
                    assert_eq!(
                        message,
                        "this provider has no record of the addressed execution"
                    );
                }
                other => panic!("expected an unknown-target receipt, got {other:?}"),
            }
        }
    }

    #[test]
    fn target_refusals_do_not_override_addressing_dedupe_or_horizon_silence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .consume_command("stop-replayed", 900)
            .expect("consume command");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        let other_instance =
            lifecycle_target_content("session.stop", "stop-other-instance", "missing", 1)
                .replace("\"instance-1\"", "\"instance-2\"");
        assert_eq!(
            decide_lifecycle(&context, Uuid::nil(), 1_000, &other_instance),
            LifecycleDecision::Ignore(Ignored::NotAddressed)
        );
        assert_eq!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-replayed", "missing", 1),
            ),
            LifecycleDecision::Ignore(Ignored::AlreadyConsumed)
        );

        let old_context = ctx(&state, &projects, 100_000);
        assert_eq!(
            decide_lifecycle(
                &old_context,
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-old", "missing", 1),
            ),
            LifecycleDecision::Ignore(Ignored::PastHorizon)
        );
    }

    #[test]
    fn a_target_with_the_wrong_channel_or_driver_fails_as_unknown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let wrong_channel = Uuid::new_v4();
        match decide_lifecycle(
            &ctx(&state, &projects, 1_000),
            wrong_channel,
            1_000,
            &lifecycle_target_content("session.stop", "stop-wrong-channel", "s1", 1),
        ) {
            LifecycleDecision::Fail { code, .. } => assert_eq!(code, UNKNOWN_TARGET),
            other => panic!("expected an unknown-target receipt, got {other:?}"),
        }

        let mut wrong_driver = session("s2", dir.path());
        wrong_driver.driver = "codex-acp".into();
        state.insert_session(wrong_driver).expect("insert");
        match decide_lifecycle(
            &ctx(&state, &projects, 1_000),
            Uuid::nil(),
            1_000,
            &lifecycle_target_content("session.resume", "resume-wrong-driver", "s2", 1),
        ) {
            LifecycleDecision::Fail { code, .. } => assert_eq!(code, UNKNOWN_TARGET),
            other => panic!("expected an unknown-target receipt, got {other:?}"),
        }
    }

    #[test]
    fn pre_authority_legacy_records_stay_ungoverned_but_genesis_never_falls_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        let projects = ProjectsFile::default();

        let mut legacy_state = store(&dir.path().join("legacy"));
        let mut legacy = session("legacy", dir.path());
        legacy.founder_pubkey = None;
        legacy_state.insert_session(legacy).expect("insert legacy");
        assert!(matches!(
            decide_turn(
                &ctx(&legacy_state, &projects, 1_000),
                1_000,
                &turn_content("turn-legacy", "legacy", 1),
            ),
            TurnDecision::Start { .. }
        ));
        assert!(matches!(
            decide_lifecycle(
                &ctx(&legacy_state, &projects, 1_000),
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-legacy", "legacy", 1),
            ),
            LifecycleDecision::Stop(_)
        ));

        let mut genesis_state = store(&dir.path().join("genesis"));
        let mut unresolved = session("genesis", dir.path());
        unresolved.genesis_ref = Some("12".repeat(32));
        unresolved.founder_pubkey = None;
        genesis_state
            .insert_session(unresolved)
            .expect("insert genesis");
        assert!(matches!(
            decide_turn(
                &ctx(&genesis_state, &projects, 1_000),
                1_000,
                &turn_content("turn-genesis", "genesis", 1),
            ),
            TurnDecision::Fail { .. }
        ));
        assert!(matches!(
            decide_lifecycle(
                &ctx(&genesis_state, &projects, 1_000),
                Uuid::nil(),
                1_000,
                &lifecycle_target_content("session.stop", "stop-genesis", "genesis", 1),
            ),
            LifecycleDecision::Fail {
                code: UNAUTHORIZED_OPERATOR,
                ..
            }
        ));
    }

    /// The A5 authority split: a granted operator may steer (turn start and
    /// interrupt) a genesis-bearing session, but stop/resume stay owner-only —
    /// `grant-operator` never moves ownership.
    #[test]
    fn a_granted_operator_may_steer_but_never_stop_or_resume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let grantee = "ef".repeat(32);
        let mut governed = session("s1", dir.path());
        governed.genesis_ref = Some("12".repeat(32));
        governed.granted_operators = [grantee.clone()].into_iter().collect();
        governed.authority_seq = 1;
        state.insert_session(governed).expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx_as(&state, &projects, 1_000, &grantee);

        assert!(matches!(
            decide_turn(&context, 1_000, &turn_content("turn-grantee", "s1", 1)),
            TurnDecision::Start { .. }
        ));
        assert!(matches!(
            decide_turn(
                &context,
                1_000,
                &interrupt_content("interrupt-grantee", "s1", 1)
            ),
            TurnDecision::Interrupt { .. }
        ));
        for (action, command_id) in [
            ("session.stop", "stop-grantee"),
            ("session.resume", "resume-grantee"),
        ] {
            assert!(
                matches!(
                    decide_lifecycle(
                        &context,
                        Uuid::nil(),
                        1_000,
                        &lifecycle_target_content(action, command_id, "s1", 1),
                    ),
                    LifecycleDecision::Fail {
                        code: UNAUTHORIZED_OPERATOR,
                        ..
                    }
                ),
                "{action} from a granted operator must stay owner-only"
            );
        }
    }

    /// A channel member who is neither founder nor granted operator is
    /// refused visibly, and a granted-operator set can never open a legacy
    /// no-genesis record (R20: legacy authority is the witnessed creator).
    #[test]
    fn non_granted_members_are_refused_and_grants_never_apply_without_a_genesis() {
        let dir = tempfile::tempdir().expect("tempdir");
        let projects = ProjectsFile::default();
        let stranger = "99".repeat(32);
        let grantee = "ef".repeat(32);

        let mut governed_state = store(&dir.path().join("governed"));
        let mut governed = session("s1", dir.path());
        governed.genesis_ref = Some("12".repeat(32));
        governed.granted_operators = [grantee.clone()].into_iter().collect();
        governed.authority_seq = 1;
        governed_state.insert_session(governed).expect("insert");
        assert!(matches!(
            decide_turn(
                &ctx_as(&governed_state, &projects, 1_000, &stranger),
                1_000,
                &turn_content("turn-stranger", "s1", 1)
            ),
            TurnDecision::Fail { .. }
        ));

        // Defensive: a grant entry on a no-genesis record is inert — legacy
        // sessions acquire umbrella authority by adoption, never inference.
        let mut legacy_state = store(&dir.path().join("legacy"));
        let mut legacy = session("s2", dir.path());
        legacy.granted_operators = [grantee.clone()].into_iter().collect();
        legacy_state.insert_session(legacy).expect("insert");
        assert!(matches!(
            decide_turn(
                &ctx_as(&legacy_state, &projects, 1_000, &grantee),
                1_000,
                &turn_content("turn-inert-grant", "s2", 1)
            ),
            TurnDecision::Fail { .. }
        ));
    }

    #[test]
    fn a_durably_stopped_session_cannot_resume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let mut stopped = session("s1", dir.path());
        stopped.closed = true;
        state.insert_session(stopped).expect("insert");
        let projects = ProjectsFile::default();
        match decide_lifecycle(
            &ctx(&state, &projects, 1_000),
            Uuid::nil(),
            1_000,
            &lifecycle_target_content("session.resume", "resume-1", "s1", 1),
        ) {
            LifecycleDecision::Fail {
                command_id,
                code,
                message,
            } => {
                assert_eq!(command_id, "resume-1");
                assert_eq!(code, SESSION_CLOSED);
                assert_eq!(
                    message,
                    "the addressed execution was already durably stopped"
                );
            }
            other => panic!("expected a session-closed receipt, got {other:?}"),
        }
    }

    #[test]
    fn a_command_older_than_the_horizon_is_history_not_intent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = store(dir.path());
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let context = ctx(&state, &projects, 1_000_000);
        assert_eq!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", AUTHORITY)
            ),
            LifecycleDecision::Ignore(Ignored::PastHorizon)
        );
        // Exactly at the horizon is still fresh.
        let at_horizon = ctx(&state, &projects, 1_000 + 86_400);
        assert!(matches!(
            decide_lifecycle(
                &at_horizon,
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", AUTHORITY)
            ),
            LifecycleDecision::Create(_)
        ));
    }

    #[test]
    fn the_session_cap_fails_the_create_with_session_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        for index in 0..4 {
            state
                .insert_session(session(&format!("s{index}"), dir.path()))
                .expect("insert");
        }
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        match decide_lifecycle(
            &ctx(&state, &projects, 1_000),
            Uuid::nil(),
            1_000,
            &create_content("create-1", "null", AUTHORITY),
        ) {
            LifecycleDecision::Fail { code, .. } => assert_eq!(code, SESSION_LIMIT),
            other => panic!("expected a failure receipt, got {other:?}"),
        }
    }

    /// Zero is unlimited: the very state that refuses at a ceiling of four
    /// admits when the ceiling is none (asked for 2026-08-24).
    #[test]
    fn an_unlimited_ceiling_never_refuses_for_capacity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        for index in 0..64 {
            state
                .insert_session(session(&format!("s{index}"), dir.path()))
                .expect("insert");
        }
        let projects = projects_with_channel(Uuid::nil(), dir.path());
        let mut context = ctx(&state, &projects, 1_000);
        context.max_sessions = crate::config::UNLIMITED_MAX_SESSIONS;
        assert!(matches!(
            decide_lifecycle(
                &context,
                Uuid::nil(),
                1_000,
                &create_content("create-1", "null", AUTHORITY),
            ),
            LifecycleDecision::Create(_)
        ));
    }

    /// Generation fencing is the whole safety story for turns: a command that
    /// names a generation this provider is not running must never reach an
    /// agent, because the operator was looking at a session that no longer
    /// exists in this form.
    #[test]
    fn turns_are_fenced_to_the_exact_live_generation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);

        assert!(matches!(
            decide_turn(&context, 1_000, &turn_content("turn-1", "s1", 1)),
            TurnDecision::Start { .. }
        ));
        assert_eq!(
            decide_turn(&context, 1_000, &turn_content("turn-2", "s1", 2)),
            TurnDecision::Ignore(Ignored::StaleGeneration)
        );
        assert_eq!(
            decide_turn(&context, 1_000, &turn_content("turn-3", "ghost", 1)),
            TurnDecision::Ignore(Ignored::UnknownTarget)
        );
    }

    #[test]
    fn turns_addressed_to_another_instance_are_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = CommandContext {
            instance_id: "instance-2",
            ..ctx(&state, &projects, 1_000)
        };
        assert_eq!(
            decide_turn(&context, 1_000, &turn_content("turn-1", "s1", 1)),
            TurnDecision::Ignore(Ignored::NotAddressed)
        );
    }

    #[test]
    fn a_closed_session_accepts_no_further_turns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        let mut record = session("s1", dir.path());
        record.closed = true;
        state.insert_session(record).expect("insert");
        let projects = ProjectsFile::default();
        assert_eq!(
            decide_turn(
                &ctx(&state, &projects, 1_000),
                1_000,
                &turn_content("turn-1", "s1", 1)
            ),
            TurnDecision::Ignore(Ignored::SessionClosed)
        );
    }

    #[test]
    fn both_donor_turn_actions_decode() {
        let start = decode_turn_command(&turn_content("turn-1", "s1", 1)).expect("decode start");
        assert_eq!(
            start.action,
            TurnAction::Start {
                text: "do the thing".into()
            }
        );
        let interrupt =
            decode_turn_command(&interrupt_content("turn-2", "s1", 1)).expect("decode interrupt");
        assert_eq!(interrupt.action, TurnAction::Interrupt);
        assert_eq!(interrupt.target.session_id, "s1");
    }

    #[test]
    fn malformed_turn_payloads_are_rejected() {
        for content in [
            "{}",
            r#"{"schema":"wrong","commandId":"c","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},"action":{"type":"thread.turn.interrupt"}}"#,
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},"action":{"type":"thread.turn.interrupt"}}"#,
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"c","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":0},"action":{"type":"thread.turn.interrupt"}}"#,
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"c","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},"action":{"type":"thread.turn.steer"}}"#,
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"c","target":{"driver":"d","instanceId":"i","sessionId":"s","generation":1},"action":{"type":"thread.turn.interrupt"},"cwd":"/tmp"}"#,
        ] {
            assert!(
                decode_turn_command(content).is_err(),
                "should reject {content}"
            );
        }
    }

    #[test]
    fn interrupt_decisions_are_fenced_the_same_way_starts_are() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = store(dir.path());
        state
            .insert_session(session("s1", dir.path()))
            .expect("insert");
        let projects = ProjectsFile::default();
        let context = ctx(&state, &projects, 1_000);
        assert!(matches!(
            decide_turn(&context, 1_000, &interrupt_content("turn-1", "s1", 1)),
            TurnDecision::Interrupt { .. }
        ));
        assert_eq!(
            decide_turn(&context, 1_000, &interrupt_content("turn-2", "s1", 9)),
            TurnDecision::Ignore(Ignored::StaleGeneration)
        );
    }

    #[test]
    fn a_missing_or_unreadable_projects_file_degrades_to_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(ProjectsFile::load(None), ProjectsFile::default());
        assert_eq!(
            ProjectsFile::load(Some(&dir.path().join("absent.json"))),
            ProjectsFile::default()
        );
        let broken = dir.path().join("broken.json");
        std::fs::write(&broken, b"{ not json").expect("write");
        assert_eq!(ProjectsFile::load(Some(&broken)), ProjectsFile::default());
    }

    #[test]
    fn the_projects_file_schema_round_trips_from_the_host_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel = Uuid::new_v4();
        let path = dir.path().join("projects.json");
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"pending":{{"create-1":"{cwd}"}},"projects":{{"30621:{owner}:demo":"{cwd}"}},"channels":{{"{channel}":"{cwd}"}}}}"#,
                cwd = dir.path().display(),
                owner = "cd".repeat(32),
            ),
        )
        .expect("write");
        let file = ProjectsFile::load(Some(&path));
        assert_eq!(file.version, 1);
        assert_eq!(
            file.resolve("create-1", None, Uuid::new_v4()).as_deref(),
            Some(dir.path())
        );
        assert_eq!(file.project_refs().count(), 1);
        assert_eq!(
            file.resolve("other", None, channel).as_deref(),
            Some(dir.path())
        );
    }
}
