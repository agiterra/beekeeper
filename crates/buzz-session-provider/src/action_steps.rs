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
//!    in `ProjectsFile.projects`, `beekeeper/actions.yml` must compile, the
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

use buzz_acp::relay::RelayEventPublisher;
use buzz_core::host_step::{
    build_host_step_claim, build_host_step_result, HostIdentity, HostStepClaim,
    HostStepDisposition, HostStepRefusal, HostStepRequested, HostStepResult, HOST_STEP_SCHEMA,
};
use buzz_core::host_step::{HOST_STEP_KIND_RUN_ON_HOST, HOST_STEP_KIND_WAKE_AGENT};
use buzz_core::kind::{KIND_HOST_STEP_CLAIM, KIND_HOST_STEP_RESULT};

/// What a verified, prepared request will do once claimed.
enum StepPlan {
    /// Run a command in the checkout.
    Command {
        checkout: PathBuf,
        prepared: PreparedCommand,
    },
    /// Deliver a brief to an open execution on this computer.
    Wake {
        agent: String,
        role: String,
        session_id: String,
        session_ref: Option<String>,
        channel_id: uuid::Uuid,
        target: buzz_core::coding_session_command::CodingSessionTarget,
        text: String,
    },
}

/// The wake half of a [`StepPlan`], handed to `route_wake` after the claim.
struct RoutePlan {
    agent: String,
    role: String,
    session_id: String,
    session_ref: Option<String>,
    channel_id: uuid::Uuid,
    target: buzz_core::coding_session_command::CodingSessionTarget,
    text: String,
}

/// How many times a claim is published before it is given up as unconfirmed.
const CLAIM_ATTEMPTS: u32 = 3;
/// Base delay between claim attempts; multiplied by the attempt number.
const CLAIM_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
/// Refusal code for a claim the relay never acknowledged.
pub const CLAIM_UNCONFIRMED: &str = "CLAIM_UNCONFIRMED";
use buzz_workflow::schema::{resolve_run_on_host, ActionDef, ResolvedRunOnHost};
use buzz_workflow::{parse_actions_yml, ACTIONS_YML};
use nostr::{EventBuilder, Kind, Tag};
use tokio::sync::mpsc;

use crate::action_step_listener::{ActionStepEvent, ActionStepListener, ListenerConfig};
use crate::action_step_store::{ActionStepRecord, StepState};
use crate::commands::ProjectsFile;
use crate::host_command::{self, HostCommandOutcome, PreparedCommand};
use crate::publish::Priority;
use crate::state::now_secs;
use crate::Provider;

/// The project's recorded repository folder is not in the projects file.
pub const ACTION_CHECKOUT_NOT_RECORDED: &str = "ACTION_CHECKOUT_NOT_RECORDED";
/// The checkout has no `beekeeper/actions.yml`.
pub const ACTION_FILE_MISSING: &str = "ACTION_FILE_MISSING";
/// The checkout's `beekeeper/actions.yml` does not compile.
pub const ACTION_FILE_INVALID: &str = "ACTION_FILE_INVALID";
/// The file has no entry with the request's `workflowName`.
pub const ACTION_UNKNOWN: &str = "ACTION_UNKNOWN";
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

/// Verify a request against this host's checkout, as § 5.6 requires.
///
/// Pure over the file system: `projects` is the loaded projects map. Every
/// `Err` is the exact refusal the host publishes.
pub fn verify_request(
    request: &HostStepRequested,
    projects: &BTreeMap<String, PathBuf>,
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
    let path = checkout.join(ACTIONS_YML);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(refusal(
                ACTION_FILE_MISSING,
                format!("{} has no {ACTIONS_YML}", checkout.display()),
            ));
        }
        Err(error) => {
            return Err(refusal(
                ACTION_FILE_INVALID,
                format!("{} could not be read: {error}", path.display()),
            ));
        }
    };
    let entries = parse_actions_yml(&text, &request.project)
        .map_err(|error| refusal(ACTION_FILE_INVALID, format!("{ACTIONS_YML}: {error}")))?;
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.name == request.workflow_name)
    else {
        return Err(refusal(
            ACTION_UNKNOWN,
            format!(
                "{ACTIONS_YML} in this checkout has no action named {:?}",
                request.workflow_name
            ),
        ));
    };
    if entry.hash != request.definition_hash {
        return Err(refusal(
            ACTION_DEFINITION_DRIFT,
            format!(
                "this checkout's actions.yml compiles to {}…, the relay's definition is {}…; \
                 publish the file or check out the branch that matches",
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
        (ActionDef::RunOnHost { .. }, _) => return Err(mismatch("run_on_host")),
        (ActionDef::WakeAgent { .. }, _) => return Err(mismatch("wake_agent")),
        _ => return Err(mismatch("not a host step")),
    };
    Ok(VerifiedStep {
        checkout: checkout.clone(),
        action,
    })
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
        dirty: None,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        truncated: false,
        artifact_path: None,
        routed: None,
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
        dirty: None,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        truncated: false,
        artifact_path: None,
        routed: None,
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
        dirty,
        stdout_tail: outcome.stdout_tail.clone(),
        stderr_tail: outcome.stderr_tail.clone(),
        truncated: outcome.truncated,
        artifact_path: Some(outcome.artifact_path.display().to_string()),
        routed: None,
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
            } => self.handle_host_step_finished(&requested_event_id, &outcome, head_sha, dirty),
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
        let plan = match verify_request(&request, &projects.projects).and_then(|verified| {
            match verified.action {
                VerifiedAction::Command(spec) => {
                    host_command::prepare(&spec, &verified.checkout, &host_env)
                        .map(|prepared| StepPlan::Command {
                            checkout: verified.checkout,
                            prepared,
                        })
                        .map_err(|refusal| HostStepRefusal {
                            code: refusal.code,
                            message: refusal.message,
                        })
                }
                VerifiedAction::Wake { agent, brief } => {
                    let role = crate::action_route::resolve_wake_role(&verified.checkout, &agent)?;
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
            StepPlan::Command { checkout, prepared } => {
                self.spawn_host_step(requested_event_id, checkout, prepared)
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
        let routed = buzz_core::host_step::HostStepRouted {
            agent: plan.agent,
            role: plan.role,
            session_id: plan.session_id,
            session_ref: plan.session_ref,
            command_id,
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
        checkout: PathBuf,
        prepared: PreparedCommand,
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
        let command = host_command::spawn(prepared, &artifact_dir).await?;
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
        tokio::spawn(async move {
            let outcome = command.wait().await;
            let (head_sha, dirty) = host_command::git_head_and_dirty(&checkout).await;
            let _ = reporter
                .send(ActionStepEvent::Finished {
                    requested_event_id,
                    outcome: Box::new(outcome),
                    head_sha,
                    dirty,
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
        let result = exited_result(&record, claim_event_id, outcome, head_sha, dirty);
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
        result: HostStepResult,
    ) -> anyhow::Result<()> {
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

    fn action_artifact_dir(&self, run_id: &str, step_id: &str) -> PathBuf {
        artifact_dir(&self.config.state_dir, run_id, step_id)
    }
}

/// `<state_dir>/actions/<run_id>/<step_id>/`.
pub fn artifact_dir(state_dir: &Path, run_id: &str, step_id: &str) -> PathBuf {
    state_dir.join(ARTIFACTS_DIR).join(run_id).join(step_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::host_step::{build_host_step_result, HOST_STEP_KIND_RUN_ON_HOST};

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse";

    const ACTIONS: &str = "schema: buzz-project-actions/v1\nactions:\n  - name: nightly\n    trigger: { on: manual }\n    steps:\n      - id: say\n        action: send_message\n        text: hi\n      - id: build\n        action: run_on_host\n        command: [\"true\"]\n";

    fn checkout_with(actions: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        if let Some(actions) = actions {
            let path = dir.path().join(ACTIONS_YML);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, actions).expect("write");
        }
        dir
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

    #[test]
    fn a_matching_checkout_verifies_and_resolves_the_step() {
        let checkout = checkout_with(Some(ACTIONS));
        let projects = BTreeMap::from([(PROJECT.to_owned(), checkout.path().to_path_buf())]);
        let verified = verify_request(&request(&real_hash()), &projects).expect("verified");
        assert_eq!(verified.checkout, checkout.path());
        match verified.action {
            VerifiedAction::Command(spec) => assert_eq!(spec.command, vec!["true".to_owned()]),
            other => panic!("expected a command, got {other:?}"),
        }
    }

    #[test]
    fn an_unrecorded_checkout_is_refused_by_name() {
        let refusal = verify_request(&request(&real_hash()), &BTreeMap::new()).unwrap_err();
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
        let refusal = verify_request(&request(&"ab".repeat(32)), &projects).unwrap_err();
        assert_eq!(refusal.code, ACTION_DEFINITION_DRIFT);
        assert!(refusal.message.contains(&real_hash()[..12]));
        assert!(refusal.message.contains("abababababab"));
    }

    #[test]
    fn a_missing_file_an_unknown_action_and_a_mismatched_step_are_refused() {
        let missing = checkout_with(None);
        let projects = BTreeMap::from([(PROJECT.to_owned(), missing.path().to_path_buf())]);
        assert_eq!(
            verify_request(&request(&real_hash()), &projects)
                .unwrap_err()
                .code,
            ACTION_FILE_MISSING
        );

        let checkout = checkout_with(Some(ACTIONS));
        let projects = BTreeMap::from([(PROJECT.to_owned(), checkout.path().to_path_buf())]);
        let mut unknown = request(&real_hash());
        unknown.workflow_name = "nope".into();
        assert_eq!(
            verify_request(&unknown, &projects).unwrap_err().code,
            ACTION_UNKNOWN
        );

        let mut wrong_index = request(&real_hash());
        wrong_index.step_index = 0;
        assert_eq!(
            verify_request(&wrong_index, &projects).unwrap_err().code,
            ACTION_STEP_MISMATCH
        );
        let mut wrong_id = request(&real_hash());
        wrong_id.step_id = "say".into();
        assert_eq!(
            verify_request(&wrong_id, &projects).unwrap_err().code,
            ACTION_STEP_MISMATCH
        );

        let invalid = checkout_with(Some("schema: nope\n"));
        let projects = BTreeMap::from([(PROJECT.to_owned(), invalid.path().to_path_buf())]);
        assert_eq!(
            verify_request(&request(&real_hash()), &projects)
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
