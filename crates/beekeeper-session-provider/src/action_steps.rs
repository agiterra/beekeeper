//! Host verification and execution of `run_on_host` action steps.
//!
//! The provider executes, not the desktop: it holds a relay identity, a state
//! directory, the operation ledger and the restart patterns
//! (`docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md` § 5.6). One kind:46013 request
//! goes through, in order:
//!
//! 1. **Record** it in [`crate::action_step_store`] — a redelivery of a
//!    request already recorded is answered from the record, never re-run.
//! 2. **Verify** it against this host's own checkout: the project must be
//!    in `ProjectsFile.projects`, `actions.yml` (the agents repository's root) must compile, the
//!    named entry's hash must equal the request's `definitionHash`, and the
//!    indexed step must be the named `run_on_host` step. The request carries
//!    no command text; only a checkout that agrees with the relay runs
//!    anything. A failure is a kind:46023 `refused` *without* a claim, so
//!    another host whose checkout does agree may still take the step.
//! 3. **Fence** with `consume_operation("action-step:<run>:<step>")` before
//!    the claim is published.
//! 4. **Claim** with a kind:46022, published synchronously so the relay's
//!    "claimed by <hex>" rejection is seen and recorded as a lost claim.
//! 5. **Run** under [`crate::host_command`], record the pid, and on completion
//!    publish the kind:46023 result through the durable outbox.
//!
//! On restart, a `running` record whose pid is gone is reported as
//! `lost_on_restart`: the host cannot say how the command ended, and says so.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use beekeeper_acp::relay::RelayEventPublisher;
use beekeeper_core::host_step::{
    build_host_step_claim, build_host_step_result, HostIdentity, HostStepClaim,
    HostStepDisposition, HostStepRefusal, HostStepRequested, HostStepResult, HOST_STEP_SCHEMA,
};
use beekeeper_core::host_step::{
    HOST_STEP_KIND_HIRE_AGENT, HOST_STEP_KIND_RUN_ON_HOST, HOST_STEP_KIND_WAKE_AGENT,
};
use beekeeper_core::kind::{KIND_HOST_STEP_CLAIM, KIND_HOST_STEP_RESULT};

/// What a verified, prepared request will do once claimed.
enum StepPlan {
    /// Run a command in the checkout — or, when the step asks, in a fresh
    /// worktree at the triggering commit.
    Command {
        checkout: PathBuf,
        prepared: PreparedCommand,
        spec: ResolvedRunOnHost,
        /// The commit to cut a worktree at: the one the trigger names, or
        /// the one a manual trigger bound with `--checkout`. `None` runs in
        /// the recorded project directory as found.
        bound_commit: Option<String>,
    },
    /// Deliver a brief to an open execution on this computer.
    Wake {
        agent: String,
        role: String,
        session_id: String,
        session_ref: Option<String>,
        channel_id: uuid::Uuid,
        target: beekeeper_core::coding_session_command::CodingSessionTarget,
        text: String,
    },
    /// Hire a seat into an open execution's umbrella.
    Hire {
        agent: String,
        role: String,
        session_id: String,
        session_ref: String,
        genesis_ref: String,
        channel_id: uuid::Uuid,
        text: String,
    },
}

/// The hire half of a [`StepPlan`], handed to `route_hire` after the claim.
struct HirePlan {
    agent: String,
    role: String,
    session_id: String,
    session_ref: String,
    genesis_ref: String,
    channel_id: uuid::Uuid,
    text: String,
}

/// The wake half of a [`StepPlan`], handed to `route_wake` after the claim.
struct RoutePlan {
    agent: String,
    role: String,
    session_id: String,
    session_ref: Option<String>,
    channel_id: uuid::Uuid,
    target: beekeeper_core::coding_session_command::CodingSessionTarget,
    text: String,
}

/// How many times a claim is published before it is given up as unconfirmed.
const CLAIM_ATTEMPTS: u32 = 3;
/// Base delay between claim attempts; multiplied by the attempt number.
const CLAIM_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
/// Refusal code for a claim the relay never acknowledged.
pub const CLAIM_UNCONFIRMED: &str = "CLAIM_UNCONFIRMED";
use beekeeper_workflow::schema::{resolve_run_on_host, ActionDef, ResolvedRunOnHost};
use beekeeper_workflow::{parse_actions_yml, ACTIONS_YML};
use nostr::{EventBuilder, Kind, Tag};
use tokio::sync::mpsc;

use crate::action_step_listener::{
    ActionStepEvent, ActionStepListener, ListenerConfig, RequestOffer,
};
use crate::action_step_store::{ActionStepRecord, StepState};
use crate::commands::ProjectsFile;
use crate::host_command::{self, HostCommandOutcome, PreparedCommand};
use crate::publish::Priority;
use crate::state::now_secs;
use crate::Provider;

/// The project's recorded repository folder is not in the projects file.
pub const ACTION_CHECKOUT_NOT_RECORDED: &str = "ACTION_CHECKOUT_NOT_RECORDED";
/// This host has no clone of the project's agents repository recorded.
pub const ACTION_AGENTS_REPO_NOT_RECORDED: &str = "ACTION_AGENTS_REPO_NOT_RECORDED";
/// The agents repository could not be read (no tip, git failure).
pub const ACTION_AGENTS_REPO_UNREADABLE: &str = "ACTION_AGENTS_REPO_UNREADABLE";
/// `checkout: triggering_commit`, but the trigger named no commit.
pub const ACTION_NO_TRIGGER_COMMIT: &str = "ACTION_NO_TRIGGER_COMMIT";
/// The triggering commit is not in the checkout even after a fetch.
pub const ACTION_COMMIT_UNAVAILABLE: &str = "ACTION_COMMIT_UNAVAILABLE";
/// The agents repository's tip has no `actions.yml`.
pub const ACTION_FILE_MISSING: &str = "ACTION_FILE_MISSING";
/// The agents repository's `actions.yml` does not compile.
pub const ACTION_FILE_INVALID: &str = "ACTION_FILE_INVALID";
/// The file has no entry with the request's `workflowName`.
pub const ACTION_UNKNOWN: &str = "ACTION_UNKNOWN";
/// The agents repository could not be fetched just now, and the last tip
/// this host had does not hold the requested definition — so the host cannot
/// say whether the relay's main does (ledger 250).
pub const ACTION_AGENTS_FETCH_FAILED: &str = "ACTION_AGENTS_FETCH_FAILED";
/// The entry compiles to a different hash than the relay's definition.
pub const ACTION_DEFINITION_DRIFT: &str = "ACTION_DEFINITION_DRIFT";
/// The indexed step is not the named `run_on_host` step.
pub const ACTION_STEP_MISMATCH: &str = "ACTION_STEP_MISMATCH";
/// The step fails `resolve_run_on_host`, or its directory escapes the checkout.
pub const ACTION_STEP_INVALID: &str = host_command::ACTION_STEP_INVALID;

/// Directory under the state directory that holds per-step artifacts.
pub const ARTIFACTS_DIR: &str = "actions";

/// What verification found: the checkout and the step, ready to prepare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedStep {
    /// The project's recorded repository folder.
    pub checkout: PathBuf,
    /// What the step asks this host to do.
    pub action: VerifiedAction,
}

/// The two things a host step can be, recompiled from this host's own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedAction {
    /// `run_on_host`, with defaults applied.
    Command(ResolvedRunOnHost),
    /// `wake_agent`: deliver `brief` to the agent `team.yml` calls `agent`.
    Wake {
        /// The agent's name in `team.yml`.
        agent: String,
        /// The brief template from `actions.yml`.
        brief: String,
    },
    /// `hire_agent`: seat `role` in the umbrella of the agent `team.yml`
    /// calls `agent`, with `brief` as the seat's first turn.
    Hire {
        /// The role slug to seat.
        role: String,
        /// The agent whose umbrella the seat joins.
        agent: String,
        /// The brief template from `actions.yml`.
        brief: String,
    },
}

/// The operation-ledger key one run's step is fenced under.
pub fn operation_key(run_id: &str, step_id: &str) -> String {
    format!("action-step:{run_id}:{step_id}")
}

/// Outbox semantic key for the kind:46023 answering one request.
fn result_semantic_key(requested_event_id: &str) -> String {
    format!("host-step-result:{requested_event_id}")
}

fn refusal(code: &str, message: impl Into<String>) -> HostStepRefusal {
    HostStepRefusal {
        code: code.to_owned(),
        message: message.into(),
    }
}

/// The project's `actions.yml` and `team.yml` as this host read them from
/// the agents repository's fetched tip (spec § 4.11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsFiles {
    /// `actions.yml`, verbatim.
    pub actions_yml: String,
    /// `team.yml`, when the tip has one.
    pub team_yml: Option<String>,
    /// Where the bytes came from, for messages: the short sha, and "as
    /// last fetched at …; fetching just now failed: …" when it did.
    pub provenance: String,
    /// The full commit `actions.yml` was read at — what the kind:46023 names
    /// as `agentsCommit`.
    pub commit: String,
    /// `true` when the fetch made for this request failed and `commit` is
    /// the last tip this host had.
    pub stale: bool,
}

impl AgentsFiles {
    /// The refusal code for a definition this host's file does not hold:
    /// `code` when the file is the relay's current tip, and
    /// [`ACTION_AGENTS_FETCH_FAILED`] when it is only the last tip this host
    /// managed to fetch — the absence is then this host's, not the relay's.
    fn absence_code<'a>(&self, code: &'a str) -> &'a str {
        if self.stale {
            ACTION_AGENTS_FETCH_FAILED
        } else {
            code
        }
    }
}

/// Read the project's agents files for a request, or the refusal to publish.
pub async fn load_agents_files(
    projects: &ProjectsFile,
    project: &str,
) -> Result<AgentsFiles, HostStepRefusal> {
    use crate::agents_checkout::{read_agents_file, AgentsReadError};
    let Some(record) = projects.agents_repos.get(project) else {
        return Err(refusal(
            ACTION_AGENTS_REPO_NOT_RECORDED,
            format!(
                "this computer has no clone of the agents repository recorded for project \
                 {project}; open the project's Actions tab or Finish repository setup under \
                 Project settings → Packs on this computer"
            ),
        ));
    };
    let actions = match read_agents_file(record, ACTIONS_YML).await {
        Ok(file) => file,
        Err(AgentsReadError::FileMissing { sha, .. }) => {
            return Err(refusal(
                ACTION_FILE_MISSING,
                format!(
                    "the agents repository at {} has no {ACTIONS_YML}",
                    short(&sha)
                ),
            ))
        }
        Err(error) => return Err(refusal(ACTION_AGENTS_REPO_UNREADABLE, error.to_string())),
    };
    let team_yml = match read_agents_file(record, beekeeper_persona::team::TEAM_YML).await {
        Ok(file) => Some(file.text),
        Err(AgentsReadError::FileMissing { .. }) => None,
        Err(error) => return Err(refusal(ACTION_AGENTS_REPO_UNREADABLE, error.to_string())),
    };
    Ok(AgentsFiles {
        provenance: actions.provenance(),
        commit: actions.sha.clone(),
        stale: actions.stale,
        actions_yml: actions.text,
        team_yml,
    })
}

/// Verify a request against this host's own copy of the definition, as
/// § 5.6 requires.
///
/// Pure: `projects` is the loaded projects map (where `run_on_host` runs),
/// `files` the agents repository's files as [`load_agents_files`] read them.
/// Every `Err` is the exact refusal the host publishes.
pub fn verify_request(
    request: &HostStepRequested,
    projects: &BTreeMap<String, PathBuf>,
    files: &AgentsFiles,
) -> Result<VerifiedStep, HostStepRefusal> {
    let Some(checkout) = projects.get(&request.project) else {
        return Err(refusal(
            ACTION_CHECKOUT_NOT_RECORDED,
            format!(
                "this computer has no repository folder recorded for project {}; set it under \
                 Project settings → This computer → Repository folder",
                request.project
            ),
        ));
    };
    let entries = parse_actions_yml(&files.actions_yml, &request.project).map_err(|error| {
        refusal(
            ACTION_FILE_INVALID,
            format!("{ACTIONS_YML} ({}): {error}", files.provenance),
        )
    })?;
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.name == request.workflow_name)
    else {
        return Err(refusal(
            files.absence_code(ACTION_UNKNOWN),
            format!(
                "{ACTIONS_YML} in the agents repository ({}) has no action named {:?}",
                files.provenance, request.workflow_name
            ),
        ));
    };
    if entry.hash != request.definition_hash {
        return Err(refusal(
            files.absence_code(ACTION_DEFINITION_DRIFT),
            format!(
                "the agents repository's actions.yml ({}) compiles to {}…, the relay's definition \
                 is {}…; publish the file or push the commit that matches",
                files.provenance,
                short(&entry.hash),
                short(&request.definition_hash)
            ),
        ));
    }
    let index = usize::try_from(request.step_index).unwrap_or(usize::MAX);
    let step = entry.def.steps.get(index).ok_or_else(|| {
        refusal(
            ACTION_STEP_MISMATCH,
            format!(
                "action {:?} has no step at index {}",
                request.workflow_name, request.step_index
            ),
        )
    })?;
    let mismatch = |what: &str| {
        refusal(
            ACTION_STEP_MISMATCH,
            format!(
                "step {} of action {:?} is {:?} ({what}), not {} step {:?}",
                request.step_index,
                request.workflow_name,
                step.id,
                request.step_kind,
                request.step_id
            ),
        )
    };
    if step.id != request.step_id {
        return Err(mismatch("a different id"));
    }
    let action = match (&step.action, request.step_kind.as_str()) {
        (ActionDef::RunOnHost { .. }, HOST_STEP_KIND_RUN_ON_HOST) => VerifiedAction::Command(
            resolve_run_on_host(&step.action)
                .map_err(|error| refusal(ACTION_STEP_INVALID, error.to_string()))?,
        ),
        (ActionDef::WakeAgent { to, brief }, HOST_STEP_KIND_WAKE_AGENT) => VerifiedAction::Wake {
            agent: to.agent.clone(),
            brief: brief.clone(),
        },
        (
            ActionDef::HireAgent {
                role,
                session,
                brief,
            },
            HOST_STEP_KIND_HIRE_AGENT,
        ) => VerifiedAction::Hire {
            role: role.clone(),
            agent: session.agent.clone(),
            brief: brief.clone(),
        },
        (ActionDef::RunOnHost { .. }, _) => return Err(mismatch("run_on_host")),
        (ActionDef::WakeAgent { .. }, _) => return Err(mismatch("wake_agent")),
        (ActionDef::HireAgent { .. }, _) => return Err(mismatch("hire_agent")),
        _ => return Err(mismatch("not a host step")),
    };
    Ok(VerifiedStep {
        checkout: checkout.clone(),
        action,
    })
}

/// The commit a `run_on_host` step must run at, given its `checkout` mode and
/// the run's trigger context.
///
/// - `current` runs in the recorded project directory as found — unless the
///   run bound a commit anyway, which is then honoured rather than silently
///   ignored.
/// - `triggering_commit` needs `ref_updated`'s `after` or `ci_result`'s
///   `commit` (spec § 7 C6).
/// - `required` takes either, and refuses a run that names neither — the
///   manual case, where the remedy is `--checkout <sha>` (ledger 178(g)).
pub fn bound_commit(
    spec: &ResolvedRunOnHost,
    trigger_context: &serde_json::Value,
) -> Result<Option<String>, HostStepRefusal> {
    use beekeeper_workflow::schema::HostCheckout;
    match spec.checkout {
        HostCheckout::Current => Ok(host_command::bound_checkout(trigger_context)),
        HostCheckout::TriggeringCommit => host_command::triggering_commit(trigger_context)
            .map(Some)
            .ok_or_else(|| HostStepRefusal {
                code: ACTION_NO_TRIGGER_COMMIT.to_owned(),
                message: "checkout: triggering_commit, but this run's trigger names no commit (a \
                          deleted ref, or a trigger without one)"
                    .into(),
            }),
        HostCheckout::Required => host_command::bound_checkout(trigger_context)
            .or_else(|| host_command::triggering_commit(trigger_context))
            .map(Some)
            .ok_or_else(|| HostStepRefusal {
                code: ACTION_NO_TRIGGER_COMMIT.to_owned(),
                message: "checkout: required, but this run names no commit; start it with `bee \
                          workflows trigger --checkout <sha>`"
                    .into(),
            }),
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// The kind:46023 a restarted host publishes for a run it cannot observe.
pub fn lost_on_restart_result(record: &ActionStepRecord, claim_event_id: &str) -> HostStepResult {
    HostStepResult {
        schema: HOST_STEP_SCHEMA.into(),
        run_id: record.run_id.clone(),
        step_id: record.step_id.clone(),
        requested_event_id: record.requested_event_id.clone(),
        claim_event_id: Some(claim_event_id.to_owned()),
        channel_id: record.channel_id.clone(),
        disposition: HostStepDisposition::LostOnRestart,
        exit_code: None,
        refusal: None,
        timed_out: false,
        duration_ms: None,
        head_sha: None,
        agents_commit: None,
        dirty: None,
        checkout: None,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        truncated: false,
        artifact_path: None,
        routed: None,
        artifacts: Vec::new(),
    }
}

/// The kind:46023 for a refusal made before any claim.
pub fn refused_result(
    request: &HostStepRequested,
    requested_event_id: &str,
    refusal: &HostStepRefusal,
) -> HostStepResult {
    HostStepResult {
        schema: HOST_STEP_SCHEMA.into(),
        run_id: request.run_id.clone(),
        step_id: request.step_id.clone(),
        requested_event_id: requested_event_id.to_owned(),
        claim_event_id: None,
        channel_id: request.channel_id.clone(),
        disposition: HostStepDisposition::Refused,
        exit_code: None,
        refusal: Some(refusal.clone()),
        timed_out: false,
        duration_ms: None,
        head_sha: None,
        agents_commit: None,
        dirty: None,
        checkout: None,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        truncated: false,
        artifact_path: None,
        routed: None,
        artifacts: Vec::new(),
    }
}

/// The kind:46023 a host publishes when its claim was never acknowledged and
/// it therefore did not run the command.
pub fn unconfirmed_claim_result(
    request: &HostStepRequested,
    requested_event_id: &str,
    claim_event_id: &str,
    reason: &str,
) -> HostStepResult {
    let mut result = refused_result(
        request,
        requested_event_id,
        &HostStepRefusal {
            code: CLAIM_UNCONFIRMED.into(),
            message: format!(
                "this host's claim was never acknowledged, so it did not run: {reason}"
            ),
        },
    );
    result.claim_event_id = Some(claim_event_id.to_owned());
    result
}

/// The kind:46023 for a command that ran.
pub fn exited_result(
    record: &ActionStepRecord,
    claim_event_id: &str,
    outcome: &HostCommandOutcome,
    head_sha: Option<String>,
    dirty: Option<bool>,
    checkout: Option<beekeeper_core::host_step::HostStepCheckout>,
) -> HostStepResult {
    HostStepResult {
        schema: HOST_STEP_SCHEMA.into(),
        run_id: record.run_id.clone(),
        step_id: record.step_id.clone(),
        requested_event_id: record.requested_event_id.clone(),
        claim_event_id: Some(claim_event_id.to_owned()),
        channel_id: record.channel_id.clone(),
        disposition: if outcome.timed_out {
            HostStepDisposition::TimedOut
        } else {
            HostStepDisposition::Exited
        },
        exit_code: outcome.exit_code,
        refusal: None,
        timed_out: outcome.timed_out,
        duration_ms: Some(outcome.duration_ms),
        head_sha,
        agents_commit: None,
        dirty,
        checkout,
        stdout_tail: outcome.stdout_tail.clone(),
        stderr_tail: outcome.stderr_tail.clone(),
        truncated: outcome.truncated,
        artifact_path: Some(outcome.artifact_path.display().to_string()),
        routed: None,
        artifacts: Vec::new(),
    }
}

/// Records whose command this host spawned and can no longer see.
///
/// `alive` answers whether a pid still exists; separated so the report shape
/// is testable without a real dead process.
pub fn lost_on_restart<'a>(
    running: impl Iterator<Item = &'a ActionStepRecord>,
    alive: impl Fn(u32) -> bool,
) -> Vec<(&'a ActionStepRecord, String)> {
    running
        .filter_map(|record| match &record.state {
            StepState::Running {
                claim_event_id,
                pid,
                ..
            } if !pid.is_some_and(&alive) => Some((record, claim_event_id.clone())),
            _ => None,
        })
        .collect()
}

/// The winner named by a relay "claimed by <hex>" rejection, if that is what
/// the publish error was.
pub fn claim_winner(error: &str) -> Option<String> {
    let (_, rest) = error.split_once("claimed by ")?;
    let winner: String = rest
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();
    (winner.len() == 64).then_some(winner)
}

fn signed_event(
    keys: &nostr::Keys,
    kind: u32,
    tags: Vec<Vec<String>>,
    content: String,
) -> anyhow::Result<nostr::Event> {
    let tags = tags
        .into_iter()
        .map(Tag::parse)
        .collect::<Result<Vec<Tag>, _>>()?;
    let kind = u16::try_from(kind).map_err(|_| anyhow::anyhow!("kind {kind} does not fit"))?;
    Ok(EventBuilder::new(Kind::Custom(kind), content)
        .tags(tags)
        .sign_with_keys(keys)?)
}

impl Provider {
    /// Start the single host-step listener, if a relay identity is known.
    ///
    /// Without the witnessed relay `self` no request signer can be verified,
    /// so — exactly as for CI results — nothing is listened for at all.
    pub fn start_action_step_listener(&mut self) {
        let Some(relay_self) = self.relay_self.clone() else {
            tracing::warn!(
                target: "csp::actions",
                "no relay identity was witnessed, so host step requests cannot be verified; \
                 this host will serve no action steps until the provider restarts against a \
                 relay that advertises NIP-11 self"
            );
            return;
        };
        let (listener, events) = ActionStepListener::spawn(ListenerConfig::new(
            self.config.relay_url.clone(),
            self.config.keys.clone(),
            self.config.auth_tag.clone(),
            relay_self,
        ));
        self.action_step_listener = Some(listener);
        self.action_step_events = Some(events);
        self.sync_action_step_listener();
    }

    /// Hand a kind:46013 from the channel subscription to the host-step
    /// queue (see [`ActionStepListener::offer`]). The listener's own probe
    /// still replays it; the durable store keeps the two deliveries to one
    /// claim and one run.
    pub(crate) fn offer_channel_host_step_request(&self, event: &nostr::Event) {
        let (Some(listener), Some(relay_self)) =
            (&self.action_step_listener, self.relay_self.as_deref())
        else {
            return;
        };
        let event_id = event.id.to_hex();
        match listener.offer(event, relay_self) {
            RequestOffer::Queued => tracing::info!(
                target: "csp::actions",
                %event_id,
                "host step request received on the channel subscription"
            ),
            RequestOffer::QueueFull => tracing::warn!(
                target: "csp::actions",
                %event_id,
                "the host step queue is full; the listener's next probe replays this request"
            ),
            RequestOffer::Rejected(reason) => tracing::warn!(
                target: "csp::actions",
                %event_id,
                "skipped a candidate host step request from the channel: {reason}"
            ),
            RequestOffer::Expired(_) | RequestOffer::NotServed(_) | RequestOffer::Closed => {}
        }
    }

    /// Hand the listener's queue to the run loop.
    pub fn take_action_step_events(&mut self) -> Option<mpsc::Receiver<ActionStepEvent>> {
        self.action_step_events.take()
    }

    /// Tell the listener exactly which projects this host serves now.
    pub(crate) fn sync_action_step_listener(&self) {
        if let Some(listener) = &self.action_step_listener {
            let projects = ProjectsFile::load(self.config.projects_file.as_deref());
            let served: BTreeSet<String> = projects.projects.keys().cloned().collect();
            listener.watch(served);
        }
    }

    /// Report every run this host lost to its last exit.
    ///
    /// A `running` record whose pid is gone becomes a kind:46023
    /// `lost_on_restart` through the durable outbox; a record whose pid is
    /// still alive is left alone (the command outlived the provider and its
    /// end cannot be collected — it will be reported lost at the next start
    /// once the pid is gone).
    pub fn recover_action_steps_on_start(&mut self) -> anyhow::Result<()> {
        let mut lost: Vec<(ActionStepRecord, String)> =
            lost_on_restart(self.action_steps.running(), host_command::pid_alive)
                .into_iter()
                .map(|(record, claim)| (record.clone(), claim))
                .collect();
        // A claim that was acknowledged but whose command never spawned
        // (crash between the two writes) is lost too: the relay's row is
        // `claimed` and nothing else will ever close it.
        let mut unreported: Vec<(String, HostStepResult)> = Vec::new();
        for record in self.action_steps.records() {
            match &record.state {
                StepState::Claimed { claim_event_id } => {
                    lost.push((record.clone(), claim_event_id.clone()));
                }
                // The result is known but the crash came before it was
                // queued: queue it now, nothing re-runs.
                StepState::Exited { result } => {
                    unreported.push((record.requested_event_id.clone(), (**result).clone()));
                }
                _ => {}
            }
        }
        for (record, claim_event_id) in lost {
            let result = lost_on_restart_result(&record, &claim_event_id);
            tracing::warn!(
                target: "csp::actions",
                run_id = %record.run_id,
                step_id = %record.step_id,
                "a host step was claimed or running when this provider last exited; reporting it lost"
            );
            self.report_host_step_result(&record.requested_event_id, result)?;
        }
        for (requested_event_id, result) in unreported {
            tracing::warn!(
                target: "csp::actions",
                %requested_event_id,
                "a host step result was never queued before this provider last exited; queuing it"
            );
            self.report_host_step_result(&requested_event_id, result)?;
        }
        Ok(())
    }

    /// Apply one listener event.
    pub(crate) async fn handle_action_step_event(
        &mut self,
        event: ActionStepEvent,
        publisher: &RelayEventPublisher,
    ) -> anyhow::Result<()> {
        match event {
            ActionStepEvent::Requested { event_id, request } => {
                self.handle_host_step_requested(&event_id, *request, publisher)
                    .await
            }
            ActionStepEvent::Finished {
                requested_event_id,
                outcome,
                head_sha,
                dirty,
                checkout,
                artifacts,
            } => self.handle_host_step_finished(
                &requested_event_id,
                &outcome,
                head_sha,
                dirty,
                checkout.map(|checkout| *checkout),
                artifacts,
            ),
        }
    }

    async fn handle_host_step_requested(
        &mut self,
        requested_event_id: &str,
        request: HostStepRequested,
        publisher: &RelayEventPublisher,
    ) -> anyhow::Result<()> {
        let now = now_secs();
        let record = ActionStepRecord {
            run_id: request.run_id.clone(),
            step_id: request.step_id.clone(),
            requested_event_id: requested_event_id.to_owned(),
            project: request.project.clone(),
            workflow_name: request.workflow_name.clone(),
            channel_id: request.channel_id.clone(),
            created_at: now,
            state: StepState::Requested,
            agents_commit: None,
        };
        if !self.action_steps.insert(record)? {
            // A redelivery. Whatever the record says already happened, and a
            // script is not assumed idempotent: nothing runs twice.
            tracing::debug!(
                target: "csp::actions",
                %requested_event_id,
                "host step request already recorded; ignoring the redelivery"
            );
            return Ok(());
        }

        // Verify against this host's own checkout before anything is claimed.
        // For a command that includes preparing it (directory, env); for a
        // routed brief it includes resolving the agent to a role and to an
        // open execution here, so a brief nobody can receive is never claimed.
        let projects = ProjectsFile::load(self.config.projects_file.as_deref());
        let host_env: HashMap<String, String> = std::env::vars().collect();
        // The definition comes from the agents repository's fetched tip
        // (spec § 4.11), read before the sync verification below.
        let files = load_agents_files(&projects, &request.project).await;
        if let Ok(files) = &files {
            // Named on whatever 46023 this request produces (ledger 250).
            self.action_steps
                .set_agents_commit(requested_event_id, &files.commit)?;
        }
        let plan = match files.and_then(|files| verify_request(&request, &projects.projects, &files).map(|verified| (verified, files))).and_then(|(verified, files)| {
            match verified.action {
                VerifiedAction::Command(spec) => {
                    let bound_commit = bound_commit(&spec, &request.trigger_context)?;
                    host_command::prepare(&spec, &verified.checkout, &host_env)
                        .map(|prepared| StepPlan::Command {
                            checkout: verified.checkout,
                            prepared,
                            spec,
                            bound_commit,
                        })
                        .map_err(|refusal| HostStepRefusal {
                            code: refusal.code,
                            message: refusal.message,
                        })
                }
                VerifiedAction::Hire { role, agent, brief } => {
                    let agent_role =
                        crate::action_route::resolve_wake_role(files.team_yml.as_deref(), &agent)?;
                    let execution = crate::action_route::pick_open_execution(
                        self.state.sessions(),
                        &request.project,
                        &agent_role,
                    )
                    .ok_or_else(|| HostStepRefusal {
                        code: crate::action_route::ROUTE_NO_SESSION.to_owned(),
                        message: format!(
                            "no open execution of {agent} ({agent_role}) for this project runs on \
                             this computer to hire into"
                        ),
                    })?;
                    let (Some(session_ref), Some(genesis_ref)) =
                        (execution.session_ref.clone(), execution.genesis_ref.clone())
                    else {
                        return Err(HostStepRefusal {
                            code: crate::action_route::HIRE_NO_UMBRELLA.to_owned(),
                            message: format!(
                                "{agent}'s execution names no umbrella and genesis a seat could join"
                            ),
                        });
                    };
                    let text = crate::action_route::wake_brief_text(
                        &request,
                        requested_event_id,
                        &brief,
                    )
                    .map_err(|error| HostStepRefusal {
                        code: crate::action_route::ROUTE_BRIEF_INVALID.to_owned(),
                        message: error,
                    })?;
                    Ok(StepPlan::Hire {
                        agent,
                        role,
                        session_id: execution.session_id.clone(),
                        session_ref,
                        genesis_ref,
                        channel_id: execution.channel_id,
                        text,
                    })
                }
                VerifiedAction::Wake { agent, brief } => {
                    let role =
                        crate::action_route::resolve_wake_role(files.team_yml.as_deref(), &agent)?;
                    let execution = crate::action_route::pick_open_execution(
                        self.state.sessions(),
                        &request.project,
                        &role,
                    )
                    .ok_or_else(|| HostStepRefusal {
                        code: crate::action_route::ROUTE_NO_SESSION.to_owned(),
                        message: format!(
                            "no open execution of {agent} ({role}) for this project runs on this \
                             computer; hire one first"
                        ),
                    })?;
                    let text =
                        crate::action_route::wake_brief_text(&request, requested_event_id, &brief)
                            .map_err(|error| HostStepRefusal {
                                code: crate::action_route::ROUTE_BRIEF_INVALID.to_owned(),
                                message: error,
                            })?;
                    Ok(StepPlan::Wake {
                        agent,
                        role,
                        session_id: execution.session_id.clone(),
                        session_ref: execution.session_ref.clone(),
                        channel_id: execution.channel_id,
                        target: self.target_for(execution),
                        text,
                    })
                }
            }
        }) {
            Ok(plan) => plan,
            Err(refusal) => {
                tracing::warn!(
                    target: "csp::actions",
                    %requested_event_id,
                    code = %refusal.code,
                    "refusing a host step: {}",
                    refusal.message
                );
                let result = refused_result(&request, requested_event_id, &refusal);
                self.report_host_step_result(requested_event_id, result)?;
                self.action_steps
                    .mark_refused(requested_event_id, &refusal.code)?;
                return Ok(());
            }
        };
        // The durable fence, written before the claim is published.
        let key = operation_key(&request.run_id, &request.step_id);
        self.state
            .consume_operation(&key, requested_event_id, now)?;
        if self.state.operation_owner(&key) != Some(requested_event_id) {
            tracing::info!(
                target: "csp::actions",
                %requested_event_id,
                "this run's step is already owned by another request on this host; ignoring"
            );
            self.action_steps.remove(requested_event_id)?;
            return Ok(());
        }

        // Claim, synchronously, so a lost race is seen rather than retried.
        let claim = HostStepClaim {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: request.run_id.clone(),
            step_id: request.step_id.clone(),
            requested_event_id: requested_event_id.to_owned(),
            channel_id: request.channel_id.clone(),
            host: HostIdentity {
                name: host_command::hostname(),
            },
        };
        let (tags, content) =
            build_host_step_claim(&claim).map_err(|error| anyhow::anyhow!(error))?;
        let claim_event = signed_event(&self.config.keys, KIND_HOST_STEP_CLAIM, tags, content)?;
        let claim_event_id = claim_event.id.to_hex();
        let mut attempt = 0u32;
        loop {
            match publisher
                .publish_event_acknowledged(claim_event.clone())
                .await
            {
                Ok(()) => break,
                Err(error) => {
                    let message = error.to_string();
                    if let Some(winner) = claim_winner(&message) {
                        tracing::info!(
                            target: "csp::actions",
                            %requested_event_id,
                            %winner,
                            "another host claimed this step first"
                        );
                        self.action_steps
                            .mark_lost_claim(requested_event_id, &winner)?;
                        return Ok(());
                    }
                    if message.contains("relay rejected durable event") {
                        // A definitive relay refusal that is not a lost race
                        // (expired window, membership): the step is not ours.
                        tracing::warn!(
                            target: "csp::actions",
                            %requested_event_id,
                            "the relay refused this host's claim: {message}"
                        );
                        self.action_steps
                            .mark_refused(requested_event_id, "CLAIM_REJECTED")?;
                        return Ok(());
                    }
                    attempt += 1;
                    if attempt < CLAIM_ATTEMPTS {
                        tracing::warn!(
                            target: "csp::actions",
                            %requested_event_id,
                            attempt,
                            "claim acknowledgement did not arrive ({message}); retrying"
                        );
                        tokio::time::sleep(CLAIM_RETRY_DELAY * attempt).await;
                        continue;
                    }
                    // Still unconfirmed: the relay may or may not have
                    // recorded the claim. Running now could be a second
                    // execution if another host won and only the verdict was
                    // lost, so nothing runs. The durable refusal names this
                    // claim: if the relay did record it, the refusal closes
                    // the row honestly; if it did not, the relay stores the
                    // refusal and another host may still claim.
                    tracing::warn!(
                        target: "csp::actions",
                        %requested_event_id,
                        "claim never acknowledged ({message}); refusing without running"
                    );
                    let result = unconfirmed_claim_result(
                        &request,
                        requested_event_id,
                        &claim_event_id,
                        &message,
                    );
                    self.report_host_step_result(requested_event_id, result)?;
                    self.action_steps
                        .mark_refused(requested_event_id, CLAIM_UNCONFIRMED)?;
                    return Ok(());
                }
            }
        }
        self.action_steps
            .mark_claimed(requested_event_id, &claim_event_id)?;

        match plan {
            StepPlan::Command {
                checkout,
                prepared,
                spec,
                bound_commit,
            } => {
                self.spawn_host_step(
                    requested_event_id,
                    &claim_event_id,
                    checkout,
                    prepared,
                    spec,
                    bound_commit,
                )
                .await
            }
            StepPlan::Hire {
                agent,
                role,
                session_id,
                session_ref,
                genesis_ref,
                channel_id,
                text,
            } => {
                self.route_hire(
                    requested_event_id,
                    &request,
                    &claim_event_id,
                    publisher,
                    HirePlan {
                        agent,
                        role,
                        session_id,
                        session_ref,
                        genesis_ref,
                        channel_id,
                        text,
                    },
                )
                .await
            }
            StepPlan::Wake {
                agent,
                role,
                session_id,
                session_ref,
                channel_id,
                target,
                text,
            } => {
                self.route_wake(
                    requested_event_id,
                    &request,
                    &claim_event_id,
                    publisher,
                    RoutePlan {
                        agent,
                        role,
                        session_id,
                        session_ref,
                        channel_id,
                        target,
                        text,
                    },
                )
                .await
            }
        }
    }

    /// Deliver a claimed `hire_agent` step: publish the `session.hire` into
    /// the umbrella and report where it went. The desktop answers the hire
    /// (stages the seat's custody and creates it) exactly as it does for a
    /// lead's hire; this host only asks.
    async fn route_hire(
        &mut self,
        requested_event_id: &str,
        request: &HostStepRequested,
        claim_event_id: &str,
        publisher: &RelayEventPublisher,
        plan: HirePlan,
    ) -> anyhow::Result<()> {
        let Some(record) = self.action_steps.record(requested_event_id).cloned() else {
            return Ok(());
        };
        let command_id = crate::action_route::hire_command_id(&request.run_id, &request.step_id);
        let event = match crate::action_route::build_hire_event(
            &self.config.keys,
            plan.channel_id,
            command_id.clone(),
            plan.session_ref.clone(),
            plan.genesis_ref.clone(),
            plan.role.clone(),
            plan.text.clone(),
        ) {
            Ok(event) => event,
            Err(error) => {
                let refusal = HostStepRefusal {
                    code: crate::action_route::HIRE_BRIEF_TOO_LONG.to_owned(),
                    message: error,
                };
                let result =
                    crate::action_route::route_refused_result(&record, claim_event_id, &refusal);
                self.report_host_step_result(requested_event_id, result)?;
                self.action_steps
                    .mark_refused(requested_event_id, &refusal.code)?;
                return Ok(());
            }
        };
        if let Err(error) = publisher.publish_event_acknowledged(event).await {
            let refusal = HostStepRefusal {
                code: crate::action_route::HIRE_REJECTED.to_owned(),
                message: format!("the relay did not accept the session.hire: {error}"),
            };
            tracing::warn!(
                target: "csp::actions",
                %requested_event_id,
                "a hire was not accepted: {}",
                refusal.message
            );
            let result =
                crate::action_route::route_refused_result(&record, claim_event_id, &refusal);
            self.report_host_step_result(requested_event_id, result)?;
            self.action_steps
                .mark_refused(requested_event_id, &refusal.code)?;
            return Ok(());
        }
        let routed = beekeeper_core::host_step::HostStepRouted {
            agent: plan.agent,
            role: plan.role.clone(),
            session_id: plan.session_id,
            session_ref: Some(plan.session_ref),
            command_id,
            hired_role: Some(plan.role),
        };
        tracing::info!(
            target: "csp::actions",
            %requested_event_id,
            role = %routed.role,
            command_id = %routed.command_id,
            "published a hire for a project action"
        );
        let result = crate::action_route::hired_result(&record, claim_event_id, routed);
        self.action_steps
            .mark_exited(requested_event_id, result.clone())?;
        self.report_host_step_result(requested_event_id, result)
    }

    /// Deliver a claimed `wake_agent` step: publish the turn, leave the
    /// observed gate row in the woken session, and report the result.
    async fn route_wake(
        &mut self,
        requested_event_id: &str,
        request: &HostStepRequested,
        claim_event_id: &str,
        publisher: &RelayEventPublisher,
        plan: RoutePlan,
    ) -> anyhow::Result<()> {
        let Some(record) = self.action_steps.record(requested_event_id).cloned() else {
            return Ok(());
        };
        let command_id = crate::action_route::route_command_id(&request.run_id, &request.step_id);
        let event = crate::action_route::build_route_event(
            &self.config.keys,
            plan.channel_id,
            plan.target.clone(),
            command_id.clone(),
            plan.text.clone(),
        )
        .map_err(|error| anyhow::anyhow!(error))?;
        match publisher.publish_event_acknowledged(event).await {
            Ok(()) => {}
            Err(error) => {
                let refusal = HostStepRefusal {
                    code: crate::action_route::ROUTE_REJECTED.to_owned(),
                    message: format!("the relay did not accept the turn: {error}"),
                };
                tracing::warn!(
                    target: "csp::actions",
                    %requested_event_id,
                    "a routed brief was not accepted: {}",
                    refusal.message
                );
                let result =
                    crate::action_route::route_refused_result(&record, claim_event_id, &refusal);
                self.report_host_step_result(requested_event_id, result)?;
                self.action_steps
                    .mark_refused(requested_event_id, &refusal.code)?;
                return Ok(());
            }
        }
        // The evidence trail in the woken session: a gate row for the host
        // step whose result the brief carries (spec § 5.5). Best effort, like
        // every observed row — a disclosure, not a condition of delivery.
        if let Err(error) = self.publish_observed_gate_row(
            &plan.session_id,
            plan.channel_id,
            crate::action_route::observed_gate_row(crate::action_route::gate_row_for(request)),
        ) {
            tracing::warn!(
                target: "csp::actions",
                %requested_event_id,
                "the routed result's gate row could not be published: {error}"
            );
        }
        let routed = beekeeper_core::host_step::HostStepRouted {
            agent: plan.agent,
            role: plan.role,
            session_id: plan.session_id,
            session_ref: plan.session_ref,
            command_id,
            hired_role: None,
        };
        tracing::info!(
            target: "csp::actions",
            %requested_event_id,
            agent = %routed.agent,
            command_id = %routed.command_id,
            "delivered a routed brief"
        );
        let result = crate::action_route::routed_result(&record, claim_event_id, routed);
        self.action_steps
            .mark_exited(requested_event_id, result.clone())?;
        self.report_host_step_result(requested_event_id, result)
    }

    async fn spawn_host_step(
        &mut self,
        requested_event_id: &str,
        claim_event_id: &str,
        checkout: PathBuf,
        prepared: PreparedCommand,
        spec: ResolvedRunOnHost,
        bound_commit: Option<String>,
    ) -> anyhow::Result<()> {
        let Some(record) = self.action_steps.record(requested_event_id).cloned() else {
            return Ok(());
        };
        let Some(reporter) = self
            .action_step_listener
            .as_ref()
            .map(ActionStepListener::reporter)
        else {
            return Ok(());
        };
        let artifact_dir = self.action_artifact_dir(&record.run_id, &record.step_id);
        let refused = |code: &str, message: String| HostStepRefusal {
            code: code.to_owned(),
            message,
        };
        let hermit_state = crate::execution_scope_host::host_hermit_state();
        let (declared_env, from_host) =
            crate::execution_scope_host::split_declared_env(&prepared.env, &spec.env_from_host);
        let step_name = format!("{}\n{}", record.run_id, record.step_id);
        // Spec § 7 C6: run in a fresh detached worktree at the triggering
        // commit when asked. Cut after the claim (it costs a fetch). Nothing
        // of the project's runs on the host: the fetch runs inside a boundary
        // around the checkout, the cut checks nothing out, and the tree is
        // materialized inside the step's own boundary, where any smudge
        // filter is the project's own code running with the project's rights.
        let worktree = bound_commit.as_ref().map(|_| artifact_dir.join("worktree"));
        if let (Some(commit), Some(worktree)) = (&bound_commit, &worktree) {
            let fetch_name = format!("{step_name}\nfetch");
            let checkout_plan = crate::execution_scope_host::prepare_host_command(
                &crate::execution_scope_host::HostCommandScope::git(
                    &self.config.state_dir,
                    Some(&record.project),
                    Some(&checkout),
                    &checkout,
                    &fetch_name,
                )
                .fetching(),
            );
            let cut = match checkout_plan {
                Ok(checkout_plan) => host_command::cut_worktree_unmaterialized(
                    &checkout_plan,
                    &checkout,
                    commit,
                    worktree,
                )
                .await
                .map_err(|message| refused(ACTION_COMMIT_UNAVAILABLE, message)),
                Err(refusal) => Err(refused(&refusal.code, refusal.message)),
            };
            if let Err(refusal) = cut {
                return self.refuse_claimed_step(
                    requested_event_id,
                    &record,
                    claim_event_id,
                    refusal,
                );
            }
        }
        let run_dir = worktree.clone().unwrap_or_else(|| checkout.clone());
        // The project boundary around the step (`crate::execution_scope_host`).
        // A step that cannot be bounded where a backend exists is refused,
        // never run unbounded.
        let plan = crate::execution_scope_host::prepare_host_command(
            &crate::execution_scope_host::HostCommandScope {
                state_dir: &self.config.state_dir,
                project_ref: Some(&record.project),
                checkout: Some(&checkout),
                run_dir: &run_dir,
                cwd: if worktree.is_some() {
                    &run_dir
                } else {
                    &prepared.cwd
                },
                name: &step_name,
                // The host's own worktree under its state is bound to the
                // project by the host that cut it; an as-found run must be
                // the recorded checkout itself.
                association: if worktree.is_some() {
                    crate::execution_scope::WorkspaceAssociation::HostBound
                } else {
                    crate::execution_scope::WorkspaceAssociation::Unbound
                },
                declared_env: &declared_env,
                from_host: &from_host,
                hermit_state: hermit_state.as_deref(),
                host_branch: None,
                branch_authority: None,
                host_read: &[],
                git_transport: false,
            },
        );
        let plan = match plan {
            Ok(plan) => plan,
            Err(refusal) => {
                if let Some(worktree) = &worktree {
                    host_command::remove_worktree(&checkout, worktree).await;
                }
                return self.refuse_claimed_step(
                    requested_event_id,
                    &record,
                    claim_event_id,
                    refused(&refusal.code, refusal.message),
                );
            }
        };
        let prepared = match (&bound_commit, &worktree) {
            (Some(commit), Some(worktree)) => {
                let materialized =
                    host_command::materialize_worktree(&plan, worktree, commit).await;
                let host_env: HashMap<String, String> = std::env::vars().collect();
                let prepared = materialized.and_then(|()| {
                    // Seed the tree's build state before the step runs in it.
                    // Never fatal: a step in an unseeded tree builds from cold,
                    // which is slow, not wrong — and `sandbox.yml` is a tracked
                    // file, so letting it fail a step would hand whoever can
                    // edit it a lever to fail every step.
                    seed_step_worktree(
                        &plan,
                        &checkout,
                        worktree,
                        Some(&record.project),
                        &self.config.state_dir,
                        &artifact_dir,
                    );
                    host_command::prepare(&spec, worktree, &host_env)
                        .map_err(|refusal| refusal.message)
                });
                match prepared {
                    Ok(prepared) => prepared,
                    Err(message) => {
                        host_command::remove_worktree(&checkout, worktree).await;
                        return self.refuse_claimed_step(
                            requested_event_id,
                            &record,
                            claim_event_id,
                            refused(ACTION_COMMIT_UNAVAILABLE, message),
                        );
                    }
                }
            }
            _ => prepared,
        };
        let uploader = if spec.upload {
            crate::artifact_upload::ArtifactUploader::new(
                &self.config.relay_url,
                self.config.keys.clone(),
                self.config.auth_tag.as_ref(),
            )
        } else {
            None
        };
        // Sample the tree the command is about to run in, before it runs:
        // a post-execution sample alone cannot say what was tested (ledger
        // 178(g)). For a bound run this is the commit itself on a clean tree.
        let (head_before, dirty_before) = host_command::git_head_and_dirty(&plan, &run_dir).await;
        let checkout_record = beekeeper_core::host_step::HostStepCheckout {
            mode: match &bound_commit {
                Some(commit) => beekeeper_core::host_step::host_step_checkout_commit(commit),
                None => beekeeper_core::host_step::HOST_STEP_CHECKOUT_AS_FOUND.to_owned(),
            },
            sha: bound_commit.clone(),
            head_sha_before: head_before,
            dirty_before,
        };
        let secrets = prepared.secrets.clone();
        let command = host_command::spawn(prepared, &artifact_dir, plan.launch()).await?;
        let status_plan = plan.clone();
        self.action_steps
            .mark_running(requested_event_id, command.pid, now_secs())?;
        tracing::info!(
            target: "csp::actions",
            %requested_event_id,
            run_id = %record.run_id,
            step_id = %record.step_id,
            pid = ?command.pid,
            "running a host step"
        );
        let requested_event_id = requested_event_id.to_owned();
        let upload_wanted = spec.upload;
        tokio::spawn(async move {
            let outcome = command.wait().await;
            let (head_sha, dirty) = host_command::git_head_and_dirty(&status_plan, &run_dir).await;
            if let Some(worktree) = &worktree {
                host_command::remove_worktree(&checkout, worktree).await;
            }
            let mut artifacts = Vec::new();
            if upload_wanted {
                match &uploader {
                    Some(uploader) => {
                        for name in [host_command::STDOUT_LOG, host_command::STDERR_LOG] {
                            let path = outcome.artifact_path.join(name);
                            let bytes = match tokio::fs::read(&path).await {
                                Ok(bytes) => bytes,
                                Err(error) => {
                                    tracing::warn!(
                                        target: "csp::actions",
                                        %requested_event_id,
                                        log = name,
                                        "log not uploaded, could not be read: {error}"
                                    );
                                    continue;
                                }
                            };
                            let scrubbed = host_command::scrub_bytes(&bytes, &secrets);
                            match uploader.upload_log(name, scrubbed).await {
                                Ok(artifact) => artifacts.push(artifact),
                                Err(error) => tracing::warn!(
                                    target: "csp::actions",
                                    %requested_event_id,
                                    log = name,
                                    "log not uploaded: {error}"
                                ),
                            }
                        }
                    }
                    None => tracing::warn!(
                        target: "csp::actions",
                        %requested_event_id,
                        "logs not uploaded: the relay URL has no authority to upload to"
                    ),
                }
            }
            let _ = reporter
                .send(ActionStepEvent::Finished {
                    requested_event_id,
                    outcome: Box::new(outcome),
                    head_sha,
                    dirty,
                    checkout: Some(Box::new(checkout_record)),
                    artifacts,
                })
                .await;
        });
        Ok(())
    }

    fn handle_host_step_finished(
        &mut self,
        requested_event_id: &str,
        outcome: &HostCommandOutcome,
        head_sha: Option<String>,
        dirty: Option<bool>,
        checkout: Option<beekeeper_core::host_step::HostStepCheckout>,
        artifacts: Vec<beekeeper_core::host_step::HostStepArtifact>,
    ) -> anyhow::Result<()> {
        let Some(record) = self.action_steps.record(requested_event_id).cloned() else {
            return Ok(());
        };
        let StepState::Running { claim_event_id, .. } = &record.state else {
            tracing::debug!(
                target: "csp::actions",
                %requested_event_id,
                "a host step finished but its record is not running; ignoring"
            );
            return Ok(());
        };
        let mut result = exited_result(&record, claim_event_id, outcome, head_sha, dirty, checkout);
        result.artifacts = artifacts;
        self.action_steps
            .mark_exited(requested_event_id, result.clone())?;
        tracing::info!(
            target: "csp::actions",
            %requested_event_id,
            exit_code = ?outcome.exit_code,
            timed_out = outcome.timed_out,
            "a host step finished"
        );
        self.report_host_step_result(requested_event_id, result)
    }

    /// Sign one kind:46023, queue it durably, and record it as reported.
    fn report_host_step_result(
        &mut self,
        requested_event_id: &str,
        mut result: HostStepResult,
    ) -> anyhow::Result<()> {
        if result.agents_commit.is_none() {
            result.agents_commit = self
                .action_steps
                .record(requested_event_id)
                .and_then(|record| record.agents_commit.clone());
        }
        let (tags, content) =
            build_host_step_result(&result).map_err(|error| anyhow::anyhow!(error))?;
        let event = signed_event(&self.config.keys, KIND_HOST_STEP_RESULT, tags, content)?;
        let result_event_id = event.id.to_hex();
        self.outbox.enqueue(
            KIND_HOST_STEP_RESULT,
            &result_semantic_key(requested_event_id),
            Priority::High,
            event,
        )?;
        if result.disposition != HostStepDisposition::Refused {
            self.action_steps
                .mark_reported(requested_event_id, &result_event_id)?;
        }
        Ok(())
    }

    /// Answer a claimed step with a refusal: report it and record it.
    fn refuse_claimed_step(
        &mut self,
        requested_event_id: &str,
        record: &ActionStepRecord,
        claim_event_id: &str,
        refusal: HostStepRefusal,
    ) -> anyhow::Result<()> {
        tracing::warn!(
            target: "csp::actions",
            %requested_event_id,
            "refusing a host step after claiming it: {}",
            refusal.message
        );
        let result = crate::action_route::route_refused_result(record, claim_event_id, &refusal);
        self.report_host_step_result(requested_event_id, result)?;
        self.action_steps
            .mark_refused(requested_event_id, &refusal.code)?;
        Ok(())
    }

    fn action_artifact_dir(&self, run_id: &str, step_id: &str) -> PathBuf {
        artifact_dir(&self.config.state_dir, run_id, step_id)
    }
}

/// Seed a step's detached worktree, and leave the receipt beside its logs.
///
/// Never fails the step. What it does instead is *say*: the receipt lands at
/// `<artifact dir>/seed.json`, beside the step's stdout and stderr, and a line
/// a lead can read goes to the log — so a step that took twenty minutes because
/// its tree started cold is explicable rather than mysterious.
fn seed_step_worktree(
    plan: &crate::execution_scope_host::HostLaunchPlan,
    checkout: &Path,
    worktree: &Path,
    project_ref: Option<&str>,
    state_dir: &Path,
    artifact_dir: &Path,
) {
    // Inside the directory this project's executions are already granted, so a
    // link into a pool resolves for the step without widening anything.
    let pool_root =
        crate::execution_scope::project_scope_pool_dir(state_dir, project_ref, worktree);
    let receipt =
        match crate::sandbox_seed_host::seed_tree(plan, checkout, worktree, Some(&pool_root)) {
            // The project declares nothing to seed.
            Ok(None) => return,
            Ok(Some(receipt)) => receipt,
            Err(refusal) => {
                tracing::warn!(
                    target: "csp::seed",
                    code = refusal.code,
                    message = %refusal.message,
                    "the project's sandbox.yml was refused; this step's tree starts cold"
                );
                return;
            }
        };
    let summary = crate::sandbox_seed_host::summarize(&receipt);
    if receipt.complete {
        tracing::info!(target: "csp::seed", summary = %summary, "seeded a step worktree");
    } else {
        tracing::warn!(target: "csp::seed", summary = %summary, "a step worktree is cold");
    }
    if let Ok(body) = serde_json::to_vec_pretty(&receipt) {
        let _ = std::fs::create_dir_all(artifact_dir);
        let _ = std::fs::write(artifact_dir.join("seed.json"), body);
    }
}

/// `<state_dir>/actions/<run_id>/<step_id>/`.
pub fn artifact_dir(state_dir: &Path, run_id: &str, step_id: &str) -> PathBuf {
    state_dir.join(ARTIFACTS_DIR).join(run_id).join(step_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core::host_step::{build_host_step_result, HOST_STEP_KIND_RUN_ON_HOST};

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse";

    const ACTIONS: &str = "schema: buzz-project-actions/v1\nactions:\n  - name: nightly\n    trigger: { on: manual }\n    steps:\n      - id: say\n        action: send_message\n        text: hi\n      - id: build\n        action: run_on_host\n        command: [\"true\"]\n";

    fn checkout_with(_actions: Option<&str>) -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn files(actions: &str) -> AgentsFiles {
        AgentsFiles {
            actions_yml: actions.to_owned(),
            team_yml: None,
            provenance: "abcd1234".to_owned(),
            commit: "abcd1234".repeat(5),
            stale: false,
        }
    }

    fn request(hash: &str) -> HostStepRequested {
        HostStepRequested {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            workflow_id: "00000000-0000-0000-0000-000000000002".into(),
            workflow_name: "nightly".into(),
            step_id: "build".into(),
            step_index: 1,
            definition_hash: hash.into(),
            step_kind: HOST_STEP_KIND_RUN_ON_HOST.into(),
            channel_id: "00000000-0000-0000-0000-000000000009".into(),
            project: PROJECT.into(),
            approval: None,
            trigger_context: serde_json::json!({}),
            inputs: serde_json::json!({}),
            expires_at: u64::MAX,
        }
    }

    fn real_hash() -> String {
        parse_actions_yml(ACTIONS, PROJECT).expect("parse")[0]
            .hash
            .clone()
    }

    fn command_spec(
        command: &[&str],
        checkout: beekeeper_workflow::schema::HostCheckout,
    ) -> ResolvedRunOnHost {
        ResolvedRunOnHost {
            command: command.iter().map(|part| (*part).to_owned()).collect(),
            working_directory: ".".into(),
            timeout_secs: 60,
            env: BTreeMap::new(),
            env_from_host: Vec::new(),
            tail_bytes: 4096,
            artifact_max_bytes: 8192,
            upload: false,
            checkout,
        }
    }

    async fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = tokio::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example")
            .output()
            .await
            .expect("git");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A repository whose first commit holds `f: one` and whose tip holds
    /// `f: two`, plus an uncommitted `f: dirty` on top. Returns the first
    /// commit's sha.
    async fn stub_repo(dir: &std::path::Path) -> String {
        git(dir, &["init", "-q", "-b", "main"]).await;
        tokio::fs::write(dir.join("f"), "one").await.expect("write");
        git(dir, &["add", "f"]).await;
        git(dir, &["commit", "-q", "-m", "one"]).await;
        let first = git(dir, &["rev-parse", "HEAD"]).await;
        tokio::fs::write(dir.join("f"), "two").await.expect("write");
        git(dir, &["commit", "-q", "-am", "two"]).await;
        tokio::fs::write(dir.join("f"), "dirty")
            .await
            .expect("write");
        first
    }

    #[test]
    fn a_required_checkout_refuses_a_run_that_names_no_commit() {
        let spec = command_spec(
            &["true"],
            beekeeper_workflow::schema::HostCheckout::Required,
        );
        let refusal = bound_commit(&spec, &serde_json::json!({}))
            .expect_err("a required checkout cannot run against whatever is checked out");
        assert_eq!(refusal.code, ACTION_NO_TRIGGER_COMMIT);
        assert!(
            refusal.message.contains("--checkout"),
            "the refusal names the flag that fixes it: {}",
            refusal.message
        );
    }

    #[test]
    fn a_bound_manual_trigger_satisfies_every_checkout_mode() {
        let sha = "ab".repeat(20);
        let context = serde_json::json!({ "checkout": sha });
        for mode in [
            beekeeper_workflow::schema::HostCheckout::Required,
            beekeeper_workflow::schema::HostCheckout::Current,
        ] {
            assert_eq!(
                bound_commit(&command_spec(&["true"], mode), &context).expect("bound"),
                Some(sha.clone()),
                "a bound commit is honoured, never silently ignored: {mode:?}"
            );
        }
        // An unbound legacy manual run still runs in the directory as found.
        assert_eq!(
            bound_commit(
                &command_spec(&["true"], beekeeper_workflow::schema::HostCheckout::Current),
                &serde_json::json!({})
            )
            .expect("unbound"),
            None
        );
        // `required` also accepts the commit a push or a CI result names.
        assert_eq!(
            bound_commit(
                &command_spec(
                    &["true"],
                    beekeeper_workflow::schema::HostCheckout::Required
                ),
                &serde_json::json!({ "after": sha })
            )
            .expect("bound"),
            Some(sha)
        );
    }

    #[tokio::test]
    async fn a_bound_run_executes_at_the_commit_and_the_record_says_so() {
        let repo = tempfile::tempdir().expect("tempdir");
        let first = stub_repo(repo.path()).await;
        let scratch = tempfile::tempdir().expect("tempdir");
        let worktree = scratch.path().join("worktree");
        // Record-keeping only: the boundary around these commands is
        // exercised in `execution_scope_host_tests`.
        let plan = crate::execution_scope_host::HostLaunchPlan::Unenforced {
            reason: "unit-test",
        };
        host_command::cut_worktree_unmaterialized(&plan, repo.path(), &first, &worktree)
            .await
            .expect("cut the worktree the bound run executes in");
        host_command::materialize_worktree(&plan, &worktree, &first)
            .await
            .expect("materialize it");

        let (head_before, dirty_before) = host_command::git_head_and_dirty(&plan, &worktree).await;
        assert_eq!(head_before.as_deref(), Some(first.as_str()));
        assert_eq!(dirty_before, Some(false));

        let spec = command_spec(
            &["cat", "f"],
            beekeeper_workflow::schema::HostCheckout::Required,
        );
        let outcome = host_command::run(
            &spec,
            &worktree,
            &scratch.path().join("artifacts"),
            &HashMap::new(),
            host_command::HostLaunch::Unenforced {
                reason: "unit-test",
            },
        )
        .await
        .expect("run");
        assert_eq!(
            outcome.stdout_tail.trim(),
            "one",
            "the command saw the bound commit's tree, not the checkout's dirty tip"
        );

        let (head_after, dirty_after) = host_command::git_head_and_dirty(&plan, &worktree).await;
        let record = ActionStepRecord {
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            step_id: "verify".into(),
            requested_event_id: "cd".repeat(32),
            project: PROJECT.into(),
            workflow_name: "verify".into(),
            channel_id: "00000000-0000-0000-0000-000000000009".into(),
            created_at: 0,
            state: StepState::Requested,
            agents_commit: None,
        };
        let result = exited_result(
            &record,
            &"ef".repeat(32),
            &outcome,
            head_after,
            dirty_after,
            Some(beekeeper_core::host_step::HostStepCheckout {
                mode: beekeeper_core::host_step::host_step_checkout_commit(&first),
                sha: Some(first.clone()),
                head_sha_before: head_before,
                dirty_before,
            }),
        );
        let checkout = result.checkout.clone().expect("the record names the tree");
        assert_eq!(checkout.sha.as_deref(), Some(first.as_str()));
        assert_eq!(checkout.head_sha_before.as_deref(), Some(first.as_str()));
        assert_eq!(checkout.dirty_before, Some(false), "clean before");
        assert_eq!(result.head_sha.as_deref(), Some(first.as_str()));
        assert_eq!(result.dirty, Some(false), "clean after");
        assert_eq!(checkout.mode, format!("commit {first}"));
        build_host_step_result(&result).expect("the result is a valid kind:46023");

        host_command::remove_worktree(repo.path(), &worktree).await;
    }

    #[tokio::test]
    async fn an_unbound_run_says_it_used_the_directory_as_found() {
        let repo = tempfile::tempdir().expect("tempdir");
        stub_repo(repo.path()).await;
        let scratch = tempfile::tempdir().expect("tempdir");
        let plan = crate::execution_scope_host::HostLaunchPlan::Unenforced {
            reason: "unit-test",
        };
        let (head_before, dirty_before) =
            host_command::git_head_and_dirty(&plan, repo.path()).await;
        assert_eq!(
            dirty_before,
            Some(true),
            "the stub's working tree is deliberately dirty"
        );
        let outcome = host_command::run(
            &command_spec(
                &["cat", "f"],
                beekeeper_workflow::schema::HostCheckout::Current,
            ),
            repo.path(),
            &scratch.path().join("artifacts"),
            &HashMap::new(),
            host_command::HostLaunch::Unenforced {
                reason: "unit-test",
            },
        )
        .await
        .expect("run");
        assert_eq!(outcome.stdout_tail.trim(), "dirty");
        let record = ActionStepRecord {
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            step_id: "verify".into(),
            requested_event_id: "cd".repeat(32),
            project: PROJECT.into(),
            workflow_name: "verify".into(),
            channel_id: "00000000-0000-0000-0000-000000000009".into(),
            created_at: 0,
            state: StepState::Requested,
            agents_commit: None,
        };
        let (head_after, dirty_after) = host_command::git_head_and_dirty(&plan, repo.path()).await;
        let result = exited_result(
            &record,
            &"ef".repeat(32),
            &outcome,
            head_after,
            dirty_after,
            Some(beekeeper_core::host_step::HostStepCheckout {
                mode: beekeeper_core::host_step::HOST_STEP_CHECKOUT_AS_FOUND.to_owned(),
                sha: None,
                head_sha_before: head_before,
                dirty_before,
            }),
        );
        let checkout = result.checkout.clone().expect("the record names the tree");
        assert_eq!(
            checkout.mode,
            beekeeper_core::host_step::HOST_STEP_CHECKOUT_AS_FOUND,
            "an unbound run says so explicitly rather than implying a commit"
        );
        assert!(checkout.sha.is_none());
        assert_eq!(checkout.dirty_before, Some(true));
        assert_eq!(result.dirty, Some(true));
        build_host_step_result(&result).expect("the result is a valid kind:46023");
    }

    #[test]
    fn a_matching_checkout_verifies_and_resolves_the_step() {
        let checkout = checkout_with(Some(ACTIONS));
        let projects = BTreeMap::from([(PROJECT.to_owned(), checkout.path().to_path_buf())]);
        let verified =
            verify_request(&request(&real_hash()), &projects, &files(ACTIONS)).expect("verified");
        assert_eq!(verified.checkout, checkout.path());
        match verified.action {
            VerifiedAction::Command(spec) => assert_eq!(spec.command, vec!["true".to_owned()]),
            other => panic!("expected a command, got {other:?}"),
        }
    }

    #[test]
    fn an_unrecorded_checkout_is_refused_by_name() {
        let refusal =
            verify_request(&request(&real_hash()), &BTreeMap::new(), &files(ACTIONS)).unwrap_err();
        assert_eq!(refusal.code, ACTION_CHECKOUT_NOT_RECORDED);
        assert!(refusal.message.contains("Repository folder"));
        let result = refused_result(&request(&real_hash()), &"ab".repeat(32), &refusal);
        assert!(result.claim_event_id.is_none());
        build_host_step_result(&result).expect("a refusal without a claim is valid");
    }

    #[test]
    fn definition_drift_names_both_hashes() {
        let checkout = checkout_with(Some(ACTIONS));
        let projects = BTreeMap::from([(PROJECT.to_owned(), checkout.path().to_path_buf())]);
        let refusal =
            verify_request(&request(&"ab".repeat(32)), &projects, &files(ACTIONS)).unwrap_err();
        assert_eq!(refusal.code, ACTION_DEFINITION_DRIFT);
        assert!(refusal.message.contains(&real_hash()[..12]));
        assert!(refusal.message.contains("abababababab"));
        assert!(
            refusal.message.contains("abcd1234"),
            "names where the bytes came from: {}",
            refusal.message
        );
    }

    #[tokio::test]
    async fn an_unrecorded_agents_repository_is_refused_by_name() {
        let projects = ProjectsFile::default();
        let refusal = load_agents_files(&projects, PROJECT).await.unwrap_err();
        assert_eq!(refusal.code, ACTION_AGENTS_REPO_NOT_RECORDED);
        assert!(refusal.message.contains("Finish repository setup"));
        let mut projects = ProjectsFile::default();
        projects.agents_repos.insert(
            PROJECT.to_owned(),
            crate::agents_checkout::AgentsRepoRecord {
                path: std::env::temp_dir().join("no-such-agents-clone"),
                ref_name: "refs/heads/main".into(),
                url: None,
            },
        );
        assert_eq!(
            load_agents_files(&projects, PROJECT)
                .await
                .unwrap_err()
                .code,
            ACTION_AGENTS_REPO_UNREADABLE
        );
    }

    /// Ledger 250 (kettle-control-2 run b60be720, kind:46023 `dd359da3…`):
    /// the lead committed `verify` to the agents repository's main (71098e4b)
    /// and triggered it; this host's pack copy — cloned by URL, no `origin`
    /// remote — was one commit behind and refused `ACTION_UNKNOWN` "as
    /// fetched at" the moment its fetch failed. Now the host fetches main
    /// from the recorded URL before resolving, the action resolves, and the
    /// commit it resolved at is what the 46023 names.
    #[tokio::test]
    async fn an_action_only_on_main_resolves_after_a_fetch_and_names_the_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "--quiet", "--initial-branch", "main"]).await;
        std::fs::write(
            origin.join("actions.yml"),
            "schema: buzz-project-actions/v1\nactions: []\n",
        )
        .unwrap();
        git(&origin, &["add", "--all"]).await;
        git(&origin, &["commit", "--quiet", "-m", "seed"]).await;
        let copy = tmp.path().join("copy");
        std::fs::create_dir_all(&copy).unwrap();
        git(&copy, &["init", "--quiet"]).await;
        let origin_url = origin.to_str().unwrap().to_owned();
        git(
            &copy,
            &[
                "fetch",
                "--quiet",
                "--",
                &origin_url,
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        )
        .await;
        std::fs::write(origin.join("actions.yml"), ACTIONS).unwrap();
        git(&origin, &["commit", "--quiet", "-am", "add nightly"]).await;
        let main = git(&origin, &["rev-parse", "HEAD"]).await;

        let checkout = checkout_with(None);
        let mut projects = ProjectsFile::default();
        projects
            .projects
            .insert(PROJECT.to_owned(), checkout.path().to_path_buf());
        projects.agents_repos.insert(
            PROJECT.to_owned(),
            crate::agents_checkout::AgentsRepoRecord {
                path: copy.clone(),
                ref_name: "refs/heads/main".into(),
                url: Some(origin_url),
            },
        );
        let files = load_agents_files(&projects, PROJECT).await.expect("read");
        assert_eq!(files.commit, main.trim(), "resolved at the relay's main");
        assert!(!files.stale);
        verify_request(&request(&real_hash()), &projects.projects, &files)
            .expect("the action on main resolves");
        let mut result = refused_result(
            &request(&real_hash()),
            &"e".repeat(64),
            &refusal(ACTION_STEP_INVALID, "x"),
        );
        result.agents_commit = Some(files.commit.clone());
        let (_, content) = build_host_step_result(&result).expect("valid");
        assert!(content.contains(&format!("\"agentsCommit\":\"{}\"", main.trim())));

        // The same copy with nowhere to fetch from: the absence is this
        // host's, and the refusal says so rather than ACTION_UNKNOWN.
        let mut unknown = request(&real_hash());
        unknown.workflow_name = "verify".into();
        if let Some(record) = projects.agents_repos.get_mut(PROJECT) {
            record.url = Some(tmp.path().join("gone").to_str().unwrap().to_owned());
        }
        let stale = load_agents_files(&projects, PROJECT).await.expect("read");
        assert!(stale.stale);
        let refused = verify_request(&unknown, &projects.projects, &stale).unwrap_err();
        assert_eq!(refused.code, ACTION_AGENTS_FETCH_FAILED);
        assert!(
            refused.message.contains("fetching just now failed"),
            "{}",
            refused.message
        );
    }

    #[test]
    fn an_unknown_action_a_mismatched_step_and_an_invalid_file_are_refused() {
        let checkout = checkout_with(Some(ACTIONS));
        let projects = BTreeMap::from([(PROJECT.to_owned(), checkout.path().to_path_buf())]);
        let mut unknown = request(&real_hash());
        unknown.workflow_name = "nope".into();
        assert_eq!(
            verify_request(&unknown, &projects, &files(ACTIONS))
                .unwrap_err()
                .code,
            ACTION_UNKNOWN
        );

        let mut wrong_index = request(&real_hash());
        wrong_index.step_index = 0;
        assert_eq!(
            verify_request(&wrong_index, &projects, &files(ACTIONS))
                .unwrap_err()
                .code,
            ACTION_STEP_MISMATCH
        );
        let mut wrong_id = request(&real_hash());
        wrong_id.step_id = "say".into();
        assert_eq!(
            verify_request(&wrong_id, &projects, &files(ACTIONS))
                .unwrap_err()
                .code,
            ACTION_STEP_MISMATCH
        );

        assert_eq!(
            verify_request(&request(&real_hash()), &projects, &files("schema: nope\n"))
                .unwrap_err()
                .code,
            ACTION_FILE_INVALID
        );
    }

    #[test]
    fn a_dead_pid_is_reported_lost_on_restart_and_a_live_one_is_not() {
        let mut dead = ActionStepRecord {
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            step_id: "build".into(),
            requested_event_id: "aa".repeat(32),
            project: PROJECT.into(),
            workflow_name: "nightly".into(),
            channel_id: "00000000-0000-0000-0000-000000000009".into(),
            created_at: 1,
            state: StepState::Running {
                claim_event_id: "bb".repeat(32),
                pid: Some(7),
                started_at: 2,
            },
            agents_commit: None,
        };
        let mut alive = dead.clone();
        alive.requested_event_id = "cc".repeat(32);
        alive.state = StepState::Running {
            claim_event_id: "dd".repeat(32),
            pid: Some(8),
            started_at: 2,
        };
        let mut no_pid = dead.clone();
        no_pid.requested_event_id = "ee".repeat(32);
        no_pid.state = StepState::Running {
            claim_event_id: "ff".repeat(32),
            pid: None,
            started_at: 2,
        };
        dead.created_at = 1;
        let records = [dead.clone(), alive, no_pid.clone()];
        let lost = lost_on_restart(records.iter(), |pid| pid == 8);
        let ids: Vec<&str> = lost
            .iter()
            .map(|(record, _)| record.requested_event_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                dead.requested_event_id.as_str(),
                no_pid.requested_event_id.as_str()
            ]
        );

        let result = lost_on_restart_result(&dead, &"bb".repeat(32));
        assert_eq!(result.disposition, HostStepDisposition::LostOnRestart);
        assert_eq!(result.exit_code, None);
        assert_eq!(
            result.claim_event_id.as_deref(),
            Some("bb".repeat(32).as_str())
        );
        build_host_step_result(&result).expect("a lost result is valid on the wire");
    }

    #[test]
    fn the_relay_rejection_names_the_winner() {
        let winner = "ab".repeat(32);
        assert_eq!(
            claim_winner(&format!(
                "Unexpected message: relay rejected durable event: claimed by {winner}"
            )),
            Some(winner)
        );
        assert_eq!(claim_winner("Timeout"), None);
        assert_eq!(claim_winner("claimed by nobody"), None);
    }

    #[test]
    fn an_exited_result_carries_the_outcome_and_a_timeout_is_its_own_disposition() {
        let record = ActionStepRecord {
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            step_id: "build".into(),
            requested_event_id: "aa".repeat(32),
            project: PROJECT.into(),
            workflow_name: "nightly".into(),
            channel_id: "00000000-0000-0000-0000-000000000009".into(),
            created_at: 1,
            state: StepState::Requested,
            agents_commit: None,
        };
        let outcome = HostCommandOutcome {
            exit_code: Some(124),
            timed_out: true,
            duration_ms: 1_000,
            stdout_tail: "out".into(),
            stderr_tail: String::new(),
            truncated: false,
            artifact_path: PathBuf::from("/x/actions/run/build"),
        };
        let result = exited_result(
            &record,
            &"bb".repeat(32),
            &outcome,
            Some("a".repeat(40)),
            Some(true),
            None,
        );
        assert_eq!(result.disposition, HostStepDisposition::TimedOut);
        assert_eq!(
            result.artifact_path.as_deref(),
            Some("/x/actions/run/build")
        );
        build_host_step_result(&result).expect("valid");
        assert_eq!(
            artifact_dir(Path::new("/s"), "run", "build"),
            PathBuf::from("/s/actions/run/build")
        );
    }
}
