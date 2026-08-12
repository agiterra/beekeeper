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

use crate::payload::{PROJECT_CWD_UNRESOLVED, SESSION_LIMIT};
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
}

/// A validated, resolved create request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePlan {
    /// The create command being answered.
    pub command_id: String,
    /// Channel every event for the new session is published into.
    pub channel_id: Uuid,
    /// Host-local working directory. Never appears in signed content.
    pub cwd: PathBuf,
    /// NIP-MP project coordinate, or `None` for a standalone session.
    pub project_ref: Option<String>,
    /// Repository coordinate, or `None`.
    pub repo_ref: Option<String>,
    /// Requested model, or `None`.
    pub model: Option<String>,
    /// Operator-facing title, or `None`.
    pub title: Option<String>,
    /// First turn to deliver after creation, or `None`.
    pub initial_turn: Option<String>,
}

/// What to do about one 44220 turn command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnDecision {
    /// Do nothing.
    Ignore(Ignored),
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
    /// The `providerInstanceRef` this provider answers to.
    pub provider_instance_ref: &'a str,
    /// Driver slug in every `cs-target` this provider mints.
    pub driver: &'a str,
    /// Instance id in every `cs-target` this provider mints.
    pub instance_id: &'a str,
    /// Current wall clock, seconds since the Unix epoch.
    pub now_secs: u64,
    /// Freshness horizon in seconds.
    pub horizon_secs: u64,
    /// Ceiling on concurrently live sessions.
    pub max_sessions: usize,
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
    let CodingSessionLifecycleAction::SessionCreate {
        project_ref,
        repo_ref,
        provider_instance_ref,
        provider_authority_pubkey,
        model,
        title,
        initial_turn,
    } = &payload.action;

    // Addressing before dedupe: a command for another adapter must not consume
    // an id in *this* adapter's ledger, or a later legitimate reuse would be
    // silently dropped.
    if !provider_authority_pubkey.eq_ignore_ascii_case(context.provider_pubkey)
        || provider_instance_ref != context.provider_instance_ref
    {
        return LifecycleDecision::Ignore(Ignored::NotAddressed);
    }
    if context.state.is_command_consumed(&payload.command_id) {
        return LifecycleDecision::Ignore(Ignored::AlreadyConsumed);
    }
    if context.past_horizon(created_at) {
        return LifecycleDecision::Ignore(Ignored::PastHorizon);
    }

    if context.state.live_session_count() >= context.max_sessions {
        return LifecycleDecision::Fail {
            command_id: payload.command_id.clone(),
            code: SESSION_LIMIT,
            message: format!(
                "provider is already running its maximum of {} session(s)",
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
        channel_id,
        cwd,
        project_ref: project_ref.clone(),
        repo_ref: repo_ref.clone(),
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

    if command.target.driver != context.driver || command.target.instance_id != context.instance_id
    {
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

    fn ctx<'a>(
        state: &'a StateStore,
        projects: &'a ProjectsFile,
        now_secs: u64,
    ) -> CommandContext<'a> {
        CommandContext {
            provider_pubkey: AUTHORITY,
            provider_instance_ref: "claude-primary",
            driver: "claude-agent-acp",
            instance_id: "instance-1",
            now_secs,
            horizon_secs: 86_400,
            max_sessions: 4,
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

    fn session(session_id: &str, cwd: &Path) -> SessionRecord {
        SessionRecord {
            session_id: session_id.to_owned(),
            generation: 1,
            channel_id: Uuid::nil(),
            command_id: "create-1".into(),
            cwd: cwd.to_path_buf(),
            project_ref: None,
            repo_ref: None,
            model: None,
            title: None,
            created_at_ms: 0,
            next_seq: 1,
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
