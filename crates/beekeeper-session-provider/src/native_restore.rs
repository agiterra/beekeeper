//! Reopen a generation this provider already owns, in place, or refuse.
//!
//! # The problem
//!
//! [`Provider::recover`](crate::Provider::recover) detaches every open
//! execution: a restarted provider has no process behind any session it was
//! running, so each open generation is published `disconnected` and its
//! in-flight turn is closed. The durable record survives, but nothing reopens
//! it — the only re-open path is the explicit `session.resume` lifecycle
//! command, and that deliberately mints `generation + 1`, resets the sequence
//! and lease counters, and needs its own one-shot seat entry keyed by the
//! resume command.
//!
//! That is the right answer for an operator asking to reconnect. It is the
//! wrong answer for a promise the provider already made. A CI continuation
//! registered before the restart is owed **one** turn against the **exact**
//! target it named; delivering it into generation N+1 would answer a different
//! target than the one the sender is watching, and delivering it into a fresh
//! `session/new` conversation would hand the agent a prompt about work it has
//! no memory of while the metadata still says the generation continued.
//!
//! # What this does instead
//!
//! [`Provider::restore_generation`] reopens the **current** generation:
//! same generation number, same `generation_command_id`, same seated identity,
//! same transcript sequence, same lease sequence, same open-turn state. The
//! ACP session is reattached natively (`session/resume`, else `session/load`)
//! and the open is *strict* — [`crate::session::CreateRequest::strict_native`]
//! — so an adapter that cannot reattach produces a named refusal rather than a
//! new conversation.
//!
//! # Preconditions, in order, each a named obstacle
//!
//! 1. This provider holds an open (not closed) record for the session.
//! 2. It has no live handle for it ([`RestoreObstacle::AlreadyLive`]).
//! 3. The record holds a native session cursor
//!    ([`RestoreObstacle::NoResumeCursor`]).
//! 4. The generation's runtime is still installed
//!    ([`RestoreObstacle::ProviderUnavailable`]).
//! 5. For a **seated** record, the actor-seats file holds an entry under the
//!    generation's command id whose pubkey is the record's actor
//!    ([`RestoreObstacle::ActorUnavailable`]).
//! 6. The adapter offers a native reattachment and accepts this cursor
//!    ([`RestoreObstacle::Unsupported`], [`RestoreObstacle::Rejected`]).
//!
//! Five and six are in the opposite order from a naive reading of the spec's
//! list, and necessarily so: an adapter's capabilities are only knowable by
//! spawning it, and a seated generation's adapter cannot be spawned without
//! the seat's key material. So a seated generation with no custody defers on
//! `ACTOR_UNAVAILABLE` without ever asking the adapter anything — which is
//! also the better answer, because that obstacle is the retryable one.
//!
//! # What a failure must not leave behind
//!
//! Nothing. No session record is written, no generation is advanced, no
//! metadata or receipt claims an execution that does not exist, and the
//! adapter child (if one was spawned at all) is shut down by
//! [`crate::session::SessionManager::start`]'s own error path. The caller
//! turns the obstacle into either a bounded retry (`ACTOR_UNAVAILABLE`, whose
//! custody may still be re-staged before the registration expires) or a
//! durable, visible refusal.

use std::time::Duration;

use beekeeper_acp::relay::{HarnessRelay, RestClient};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_runtime::RuntimeDescriptor;
use tokio::sync::mpsc;

use crate::actor_seats::ActorSeatsFile;
use crate::attachments::MediaFetcher;
use crate::config::Config;
use crate::execution_scope::BoundaryState;
use crate::payload::{self, ACTOR_UNAVAILABLE, PROVIDER_UNAVAILABLE, SESSION_ALREADY_ATTACHED};
use crate::project_deletion::ProjectFact;
use crate::session::{self, CreateRequest, SessionManager, StartedSession};
use crate::state::SessionRecord;
use crate::{seat_skills, Priority, Provider, SessionStatus};

/// The record names no native session cursor, so there is nothing to reattach
/// to and the only way to obtain a process would be a new conversation.
///
/// Provider-local, like [`crate::ci_continuation::LOST_AFTER_CLAIM`]: it
/// describes a fact about *this* host's stored state, not a rule the wire
/// protocol has an opinion about, so `buzz-core` is left alone.
pub const NO_RESUME_CURSOR: &str = "NO_RESUME_CURSOR";

/// The adapter behind this generation advertises neither `session/resume` nor
/// `session/load`, so no build of it can reopen the conversation.
///
/// Distinct from [`NATIVE_RESTORE_REJECTED`] on purpose: "this runtime has no
/// such feature" and "this runtime has the feature and refused this session"
/// lead to different remedies, and collapsing them would tell an operator to
/// go looking for a session the adapter never claimed to have lost.
pub const NATIVE_RESTORE_UNSUPPORTED: &str = "NATIVE_RESTORE_UNSUPPORTED";

/// The adapter offered a native reattachment and refused this cursor.
///
/// The strict open's whole purpose: without it this case silently became
/// `session/new`, and the generation carried on with a conversation that had
/// none of its history behind it.
pub const NATIVE_RESTORE_REJECTED: &str = "NATIVE_RESTORE_REJECTED";

/// Why a generation could not be reopened in place.
///
/// Named rather than stringly-typed because the caller's decision turns on
/// exactly one distinction: [`Self::ActorUnavailable`] is the obstacle whose
/// remedy can still arrive (the desktop re-stages custody after a restart, and
/// that may land after the first attempt), so it defers; every other obstacle
/// is settled and refuses durably.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestoreObstacle {
    /// No stored ACP cursor, so no conversation can be named.
    NoResumeCursor,
    /// No open record, or the generation's runtime is not installed here.
    ProviderUnavailable,
    /// The adapter advertises no native reattachment at all.
    Unsupported,
    /// The adapter rejected the reattachment, with its own account of why.
    Rejected(String),
    /// A seated generation whose seat has not been re-staged on this host.
    ActorUnavailable,
    /// The generation already has a live process; there is nothing to restore.
    AlreadyLive,
    /// The umbrella has been handed over or deleted, so nothing under it may
    /// be reopened on this body (`docs/HANDOVER_IMPL.md` §3).
    ///
    /// Carries the fence's own answer rather than re-deriving one: the code
    /// and the sentence a restore refuses with are exactly the ones a turn
    /// would have refused with, so a person chasing "why did nothing happen"
    /// reads one story and not two.
    Fenced(crate::commands::FenceRefusal),
}

impl RestoreObstacle {
    /// The receipt error code this obstacle publishes as.
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::NoResumeCursor => NO_RESUME_CURSOR,
            Self::ProviderUnavailable => PROVIDER_UNAVAILABLE,
            Self::Unsupported => NATIVE_RESTORE_UNSUPPORTED,
            Self::Rejected(_) => NATIVE_RESTORE_REJECTED,
            Self::ActorUnavailable => ACTOR_UNAVAILABLE,
            Self::AlreadyLive => SESSION_ALREADY_ATTACHED,
            Self::Fenced(refusal) => refusal.code,
        }
    }

    /// The operator-facing sentence this obstacle publishes with.
    ///
    /// Every one of these says what was refused *and* states that nothing was
    /// created in its place, because the failure this whole module exists to
    /// prevent is a silent new conversation.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::NoResumeCursor => "this execution has no saved native session cursor, so its \
                                     generation could not be reopened; no new conversation was \
                                     started"
                .to_owned(),
            Self::ProviderUnavailable => "this execution's runtime is no longer installed on the \
                                          host that owns it, so its generation could not be \
                                          reopened"
                .to_owned(),
            Self::Unsupported => "this execution's runtime advertises neither session resume nor \
                                  session load, so its generation could not be reopened; no new \
                                  conversation was started"
                .to_owned(),
            Self::Rejected(detail) => format!(
                "this execution's runtime refused to reopen its generation ({detail}); no new \
                 conversation was started"
            ),
            Self::ActorUnavailable => "no key material was staged for this execution's agent seat \
                                       before the registration expired, so its generation could \
                                       not be reopened as that agent"
                .to_owned(),
            Self::AlreadyLive => {
                "this execution already has a live process on this provider".to_owned()
            }
            Self::Fenced(refusal) => refusal.message.clone(),
        }
    }
}

impl Provider {
    /// Reopen the current generation of `session_id` in place, or name the
    /// obstacle that stopped it.
    ///
    /// `Ok(Ok(()))` means the generation now has a live adapter attached to
    /// the same ACP conversation it had before, and a
    /// `session_restored_native` transcript row says so. `Ok(Err(obstacle))`
    /// means nothing was created and nothing was changed. `Err` is reserved
    /// for a durable write that failed after the adapter was already
    /// attached — the caller cannot proceed, and the actor is shut down before
    /// the error is returned so the provider does not hold a process it has no
    /// record of.
    ///
    /// Deliberately not called at boot: reopening every idle session would
    /// spawn one adapter per record for conversations nobody is waiting on.
    /// The trigger is a promise coming due — today, a CI continuation whose
    /// target has no live handle (`crate::ci_continuation`).
    pub(crate) async fn restore_generation(
        &mut self,
        session_id: &str,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<Result<(), RestoreObstacle>> {
        let job = match self.prepare_restore(session_id) {
            Ok(job) => job,
            Err(obstacle) => return Ok(Err(obstacle)),
        };
        let run = self
            .await_with_lease_maintenance(
                run_restore_job(job),
                relay.map(HarnessRelay::event_publisher),
            )
            .await;
        self.finish_restore(run)
    }

    /// Everything a restore decides before it waits on anything: steps 1–5,
    /// each a named obstacle, and the inputs the slow half needs, owned.
    ///
    /// Split from [`Self::finish_restore`] so the slow half — the deletion
    /// read, the agents clone, the scope, and the adapter's own startup — can
    /// run off the run loop (SV-76) as [`run_restore_job`].
    pub(crate) fn prepare_restore(
        &mut self,
        session_id: &str,
    ) -> Result<RestoreJob, RestoreObstacle> {
        // 1. An open record. The delivery path checks this too, so reaching
        //    here without one means the record vanished between the two; the
        //    honest reading is that this host can no longer serve the target.
        let Some(record) = self.state.session(session_id).cloned() else {
            return Err(RestoreObstacle::ProviderUnavailable);
        };
        if record.closed {
            return Err(RestoreObstacle::ProviderUnavailable);
        }
        // 1a. The handover fence, before anything is asked of an adapter
        //     (§3). A restore spawns a process and reattaches a conversation;
        //     doing that for a session another machine now holds is precisely
        //     the second live execution of one task that the fence exists to
        //     prevent, and doing it for a deleted one would reopen work the
        //     relay no longer has. Asked with this provider as both operator
        //     and body, because a restore is this provider acting on its own
        //     initiative: on the claimed body it proceeds, anywhere else it
        //     refuses by name.
        if let Some(refusal) =
            crate::commands::handover_fence(&record, &self.pubkey_hex, &self.pubkey_hex)
        {
            return Err(RestoreObstacle::Fenced(refusal));
        }
        // 2. No live process.
        if self
            .sessions
            .handle(session_id)
            .is_some_and(session::SessionHandle::is_live)
        {
            return Err(RestoreObstacle::AlreadyLive);
        }
        // 3. A cursor to reattach to.
        let Some(resume_cursor) = record.resume_cursor.clone() else {
            return Err(RestoreObstacle::NoResumeCursor);
        };
        // 4. A runtime to reattach with.
        let Some(descriptor) = self.config.runtime(&record.provider_instance_ref).cloned() else {
            return Err(RestoreObstacle::ProviderUnavailable);
        };

        // The *current* target: this reopens a generation, it does not mint
        // one. Everything a consumer already has bound to this session — the
        // 44223 it is rendering, the lease semantic key, the operation fence —
        // keeps naming the same thing.
        let target = self.target_for(&record);
        // 5. Custody, under the command id that minted *this* generation. A
        //    record from before generation commands were persisted had its
        //    generation minted by the create.
        let seat_command_id = record
            .generation_command_id
            .clone()
            .unwrap_or_else(|| record.command_id.clone());
        let (seat_identity, post_fence_env, seat_skills) = match record.actor.as_deref() {
            // A human execution holds no seat and needs none: it restores with
            // the cursor alone, exactly as it ran.
            None => (None, Vec::new(), None),
            Some(actor) => {
                let seats = ActorSeatsFile::load(self.config.actor_seats_file.as_deref());
                match seats.seat(&seat_command_id) {
                    // An entry naming a different pubkey is not this seat's
                    // custody. Refusing rather than substituting is the same
                    // rule the create and resume paths hold.
                    Some(seat)
                        if seat.pubkey == actor
                            && seat.pack_ref == record.pack_ref
                            && (record.pack_ref.is_some() || seat.pack_dir.is_none())
                            && seat.relay_url == self.config.relay_url =>
                    {
                        (
                            Some(session::SeatIdentity {
                                actor_pubkey: seat.pubkey.clone(),
                                role: record.role.clone().unwrap_or_default(),
                                relay_url: seat.relay_url.clone(),
                            }),
                            seat.post_fence_env_with_bee(
                                record.role.as_deref(),
                                record.project_ref.as_deref(),
                                crate::seat_bee::host_seat_bee().map(|(bee, _)| bee),
                                std::env::var_os("PATH").as_ref(),
                            ),
                            seat_skills(seat, &self.config.state_dir, &target.session_id),
                        )
                    }
                    _ => {
                        tracing::info!(
                            target: "csp::restore",
                            %session_id,
                            command_id = %seat_command_id,
                            "no seat custody is staged for this generation yet; the restore \
                             defers rather than reopening it without the agent's identity"
                        );
                        return Err(RestoreObstacle::ActorUnavailable);
                    }
                }
            }
        };

        let agent_env: Vec<(String, String)> = descriptor
            .cli_env
            .iter()
            .map(|env| (env.name.clone(), env.value.clone()))
            .collect();
        // The deletion re-check every start and resume makes, read in the slow
        // half and applied in `finish_restore`.
        let deletion_check = match (record.project_ref.clone(), self.rest_client.clone()) {
            (Some(project), Some(rest)) => {
                let seen = self.seen_project_head(&project);
                Some((project, rest, seen))
            }
            _ => None,
        };
        Ok(RestoreJob {
            record,
            target,
            seat_command_id,
            descriptor,
            resume_cursor,
            seat_identity,
            post_fence_env,
            seat_skills,
            agent_env,
            deletion_check,
            config: self.config.clone(),
            media: self.media.clone(),
            events: self.sessions.event_sender(),
        })
    }

    /// Apply a restore's slow half on the loop: the deletion fact it read,
    /// the seat entry it spent, and — when an adapter opened — the attach and
    /// the one write a restore may make.
    ///
    /// `Err` is reserved, as on [`Self::restore_generation`], for a durable
    /// write that failed after the adapter was attached.
    pub(crate) fn finish_restore(
        &mut self,
        run: RestoreRun,
    ) -> anyhow::Result<Result<(), RestoreObstacle>> {
        let RestoreRun {
            record,
            target,
            seat_command_id,
            project_fact,
            outcome,
        } = run;
        let session_id = record.session_id.clone();
        if let Some((project, fact)) = project_fact {
            self.apply_project_fact(&project, &fact);
            if fact == crate::project_deletion::ProjectFact::Deleted {
                self.stop_project_executions(&project);
            }
        }
        // One-shot, exactly as the create and resume paths treat it: the entry
        // has been read and handed to the slow half, so it is spent whether or
        // not that opened anything. Leaving it would keep a usable key on disk
        // for a generation that will be answered terminally.
        if record.actor.is_some() {
            self.forget_actor_seat(&seat_command_id);
        }
        let (started, execution_state) = match outcome {
            RestoreOutcome::Unplanned(failure) => {
                tracing::info!(
                    target: "csp::restore",
                    %session_id,
                    code = failure.code,
                    "a generation could not be reopened: {}", failure.message
                );
                return Ok(Err(RestoreObstacle::ProviderUnavailable));
            }
            RestoreOutcome::NativeRefused(reason) => {
                return Ok(Err(RestoreObstacle::Rejected(reason)));
            }
            RestoreOutcome::Started {
                started,
                execution_state,
            } => (started, execution_state),
        };
        // 6. What the adapter answered.
        let started = match *started {
            Ok(started) => started,
            Err(failure) => {
                let obstacle = match failure.code {
                    NATIVE_RESTORE_UNSUPPORTED => RestoreObstacle::Unsupported,
                    NATIVE_RESTORE_REJECTED => RestoreObstacle::Rejected(failure.message),
                    NO_RESUME_CURSOR => RestoreObstacle::NoResumeCursor,
                    // Spawn, `initialize` and authentication failures all mean
                    // the same thing here: no process, nothing created.
                    _ => RestoreObstacle::ProviderUnavailable,
                };
                tracing::info!(
                    target: "csp::restore",
                    %session_id,
                    code = %obstacle.code(),
                    "a generation could not be reopened in place"
                );
                return Ok(Err(obstacle));
            }
        };
        // 6a. Still the state the restore was prepared for. When the slow half
        //     ran off the loop (SV-76) other work ran beside it: the record may
        //     have closed, been fenced, or moved to a new generation, another
        //     path may have attached a process, and the adapter may already
        //     have died. Attaching over any of those would advertise a process
        //     that is not this generation's, or none at all.
        let current = self.state.session(&session_id);
        let still_current = current.is_some_and(|current| {
            !current.closed
                && current.generation == record.generation
                && crate::commands::handover_fence(current, &self.pubkey_hex, &self.pubkey_hex)
                    .is_none()
        });
        let already_attached = self.sessions.handle(&session_id).is_some();
        if !still_current || already_attached || !started.is_live() {
            started.discard();
            tracing::info!(
                target: "csp::restore",
                %session_id,
                still_current,
                already_attached,
                "a reopened adapter was discarded: the generation changed while it started"
            );
            return Ok(Err(if already_attached {
                RestoreObstacle::AlreadyLive
            } else {
                RestoreObstacle::ProviderUnavailable
            }));
        }
        let outbox_before = self.outbox.pending_keys();
        let startup = self.sessions.attach(started);
        // Per-process capabilities, re-witnessed: this is a different adapter
        // process than the one that opened the generation, and two builds of
        // one adapter can legitimately disagree.
        self.steering
            .insert(record.session_id.clone(), startup.steering_supported);
        self.prompt_image
            .insert(record.session_id.clone(), startup.prompt_image_supported);
        self.model_switch
            .witness(&record.session_id, startup.model_switch_supported);

        // The one write a restore is allowed to make, and the two fields it
        // may touch. `generation`, `generation_command_id`, `next_seq`,
        // `next_lease_sequence` and `open_turn` are deliberately absent: they
        // describe the generation, and this reopened it rather than replacing
        // it. `pack_ref` is absent too — no pack was re-resolved here, and
        // restating one would claim a staging decision nobody made.
        if let Err(error) = self.state.update_session(&record.session_id, |stored| {
            if stored.resume_cursor.as_deref() != Some(startup.acp_session_id.as_str()) {
                stored.resume_cursor = Some(startup.acp_session_id.clone());
            }
            if startup.model.is_some() {
                stored.model = startup.model.clone();
            }
            stored.execution_boundary = crate::execution_scope::recorded_state(&execution_state);
        }) {
            // Never hold a live adapter the durable record does not describe.
            self.sessions.shutdown(&record.session_id);
            return Err(error.into());
        }

        // No lifecycle receipt: no command was issued. A transcript row is the
        // right surface — somebody watching this session's timeline needs to
        // know a gap in it was a restart, not the agent going quiet.
        self.enqueue_transcript(
            record.channel_id,
            &target,
            None,
            payload::status_item(SESSION_RESTORED_NATIVE),
            Priority::High,
        )?;
        self.enqueue_transcript(
            record.channel_id,
            &target,
            None,
            crate::execution_scope::boundary_status_item(&execution_state),
            Priority::High,
        )?;
        // What the provider's isolation settings withheld, when any did.
        for item in crate::session_isolation::isolation_status_items(&execution_state) {
            self.enqueue_transcript(record.channel_id, &target, None, item, Priority::High)?;
        }
        self.publish_metadata(record.channel_id, &target, SessionStatus::Idle)?;
        // Same handshake the resume path uses: the live lease is queued by the
        // runtime tick once the facts this open enqueued have been published,
        // so a consumer never sees `live` before the row that explains it.
        self.record_first_lease_prerequisites(&target, &outbox_before);
        tracing::info!(
            target: "csp::restore",
            session_id = %record.session_id,
            generation = record.generation,
            seated = record.actor.is_some(),
            "generation reopened in place"
        );
        Ok(Ok(()))
    }
}

/// A restore's inputs, decided on the loop by [`Provider::prepare_restore`]
/// and owned, so [`run_restore_job`] can run anywhere.
pub(crate) struct RestoreJob {
    record: SessionRecord,
    target: CodingSessionTarget,
    seat_command_id: String,
    descriptor: RuntimeDescriptor,
    resume_cursor: String,
    seat_identity: Option<session::SeatIdentity>,
    post_fence_env: Vec<(String, String)>,
    seat_skills: Option<session::SeatSkills>,
    agent_env: Vec<(String, String)>,
    /// `(project, reader, head already seen)` when a deletion re-check runs.
    deletion_check: Option<(String, RestClient, Option<u64>)>,
    config: Config,
    media: Option<MediaFetcher>,
    events: mpsc::Sender<session::SessionEvent>,
}

/// Who a restore was for, kept beside an off-loop run so a run that never
/// returns can still be answered ([`RestoreRun::timed_out`]).
pub(crate) struct RestoreMeta {
    record: SessionRecord,
    target: CodingSessionTarget,
    seat_command_id: String,
}

impl RestoreJob {
    /// A copy of who this restore is for.
    pub(crate) fn meta(&self) -> RestoreMeta {
        RestoreMeta {
            record: self.record.clone(),
            target: self.target.clone(),
            seat_command_id: self.seat_command_id.clone(),
        }
    }
}

/// What the slow half of a restore produced, for [`Provider::finish_restore`].
pub(crate) struct RestoreRun {
    record: SessionRecord,
    target: CodingSessionTarget,
    seat_command_id: String,
    project_fact: Option<(String, ProjectFact)>,
    outcome: RestoreOutcome,
}

impl RestoreRun {
    /// The slow half did not finish in time. Nothing it might have opened
    /// survives: dropping the startup future ends the adapter it spawned.
    pub(crate) fn timed_out(meta: RestoreMeta, limit: Duration) -> Self {
        RestoreRun {
            record: meta.record,
            target: meta.target,
            seat_command_id: meta.seat_command_id,
            project_fact: None,
            outcome: RestoreOutcome::Unplanned(crate::session::CreateFailure {
                code: PROVIDER_UNAVAILABLE,
                message: format!(
                    "the generation could not be reopened within {}s",
                    limit.as_secs()
                ),
            }),
        }
    }
}

enum RestoreOutcome {
    /// Refused before anything was spawned.
    Unplanned(crate::session::CreateFailure),
    /// The scope refuses native reattachment.
    NativeRefused(String),
    /// An adapter was asked to reattach; this is its answer.
    Started {
        started: Box<Result<StartedSession, crate::session::CreateFailure>>,
        execution_state: BoundaryState,
    },
}

/// The slow half of a restore: the deletion re-check, the read-only agents
/// clone, the execution scope, and the adapter's startup. Touches no provider
/// state, so it may run off the run loop (SV-76).
pub(crate) async fn run_restore_job(job: RestoreJob) -> RestoreRun {
    let RestoreJob {
        record,
        target,
        seat_command_id,
        descriptor,
        resume_cursor,
        seat_identity,
        post_fence_env,
        seat_skills,
        agent_env,
        deletion_check,
        config,
        media,
        events,
    } = job;
    let project_fact = match deletion_check {
        Some((project, rest, seen)) => {
            let fact = crate::project_deletion::revalidate(&rest, &project, seen).await;
            Some((project, fact))
        }
        None => None,
    };
    let finish = |outcome| RestoreRun {
        record: record.clone(),
        target: target.clone(),
        seat_command_id: seat_command_id.clone(),
        project_fact: project_fact.clone(),
        outcome,
    };
    let deleted = project_fact
        .as_ref()
        .is_some_and(|(_, fact)| *fact == ProjectFact::Deleted);
    // Strict: the recorded binding must match the scope prepared now, or
    // the restore refuses rather than reopen another scope's history.
    let unseated_agents = if deleted {
        Err(crate::execution_scope::refuse(
            crate::execution_scope::EXECUTION_SCOPE_INVALID,
            "this project was deleted, so no execution of it starts",
        ))
    } else if record.actor.is_none() {
        crate::execution_scope::stage_unseated_agents_for(
            &config,
            record.project_ref.as_deref(),
            &record.session_id,
        )
        .await
    } else {
        Ok(None)
    };
    let execution = match unseated_agents
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|_| {
            crate::execution_scope::execution_plan_for(
                &config,
                &crate::execution_scope::LaunchFacts {
                    session_id: &record.session_id,
                    project_ref: record.project_ref.as_deref(),
                    association: crate::execution_scope::recorded_association(&record),
                    unseated_agents: unseated_agents.as_ref().ok().and_then(Option::as_deref),
                    actor: record.actor.as_deref(),
                    driver: &record.driver,
                    cwd: &record.cwd,
                    agent_command: &descriptor.agent_command,
                    agent_args: &descriptor.agent_args,
                    agent_env: &agent_env,
                    identity_env: &post_fence_env,
                    seat_skills: seat_skills.as_ref(),
                    role: record.role.as_deref(),
                    rehydration: None,
                    prior: Some((
                        record.execution_binding.as_ref(),
                        record.resume_cursor.as_deref(),
                    )),
                },
            )
        }) {
        Ok(execution) => execution,
        Err(failure) => return finish(RestoreOutcome::Unplanned(failure)),
    };
    if let Some(reason) = execution.native_refusal() {
        return finish(RestoreOutcome::NativeRefused(reason.to_owned()));
    }
    let execution_state = execution.state();
    let request = CreateRequest {
        execution,
        media,
        target: target.clone(),
        channel_id: record.channel_id,
        cwd: record.cwd.clone(),
        title: record.title.clone(),
        model: record.model.clone(),
        resume_cursor: Some(resume_cursor),
        // A native reattachment carries its own history. Building a
        // verified-context package for it would spend the work and then
        // brief the agent about a past it already remembers.
        rehydration_mcp: None,
        strict_native: true,
        agent_command: descriptor.agent_command.clone(),
        agent_args: descriptor.agent_args.clone(),
        agent_env,
        seat: seat_identity,
        post_fence_env,
        seat_skills,
        idle_timeout: config.idle_timeout,
        answer_stall_timeout: config.answer_stall_timeout,
        emit_raw_sdk_frames: config.emit_raw_sdk_frames,
        max_turn_duration: config.max_turn_duration,
        idle_shutdown: config.session_idle_shutdown,
        include_thoughts: config.include_thoughts,
        transcript_paragraph_flush: config.transcript_paragraph_flush,
    };
    let started = Box::new(SessionManager::start(request, events).await);
    finish(RestoreOutcome::Started {
        started,
        execution_state,
    })
}

/// Transcript status published when a generation is reopened in place.
///
/// Deliberately its own word rather than reusing `session_resumed`: that one
/// answers a `session.resume` command and comes with a new generation, and a
/// reader who saw it here would look for a lifecycle receipt and a generation
/// bump that never happened.
pub(crate) const SESSION_RESTORED_NATIVE: &str = "session_restored_native";
