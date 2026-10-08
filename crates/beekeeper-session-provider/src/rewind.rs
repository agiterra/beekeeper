//! SV-29 `session.rewind`: **Edit from here**, chat only or files too.
//!
//! A rewind of generation N to before turn *k* detaches N and opens N+1 as a
//! new conversation (`session/new`) seeded from the signed record cut at turn
//! *k*'s `fromSeq − 1`; with `files: restore` it first returns the working
//! tree to turn *k*'s `baseTree`. N is detached, never truncated: its native
//! cursor still remembers the rewound turns, which is why it is not resumed.
//!
//! # Order (each refusal consumes the command and the seat staged for it)
//!
//! 1. Authority — the restart's, decided in [`crate::commands`].
//! 2. `SESSION_BUSY` while N has an open turn (or another rewind runs here).
//!    A redelivery of the rewind that is running is dropped unanswered.
//! 3. `TREE_BUSY` while another execution of **this provider** in the same
//!    canonical working tree has an open turn; the sentence names it.
//! 4. The checkpoint verifies (off the loop): fetched by id, strictly decoded,
//!    signed by this provider, of exactly generation N (iteration 1: only
//!    this run's turns), a `turn` checkpoint with `restorable: true`; a
//!    restore also needs its `baseTree` and commit in the object store, and
//!    the tree must match what this provider recorded publishing.
//! 5. A `pre_rewind` capture (off the loop), durably queued at high priority
//!    **before any file is written**. With `restore`, a failed capture
//!    refuses and touches nothing, and so does a working tree that has put a
//!    directory where the base has a file, or a file or symlink where it has
//!    a directory (`NOT_RESTORABLE`, naming the path): restoring over it would
//!    delete what no capture holds.
//! 6. `restore` only: [`crate::turn_checkpoint_git::restore`] (off the loop).
//! 7. Open N+1 through the resume path with the cut.
//! 8. N+1 does not open, or the restore failed: N is reopened in place when it
//!    can be ([`crate::Provider::restore_generation`]) — it then still
//!    remembers the rewound turns — and the answer is `failed`
//!    `REWIND_NOT_RESTARTED` with `rewind.files` saying what became of them.
//!
//! The git and relay work runs off the provider loop (SV-72/SV-76) in one
//! [`RewindSlot`]; the steps between are applied on the loop, and the open
//! turn and the sibling check are re-made there before anything is written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use beekeeper_acp::relay::{HarnessRelay, RestClient};
use beekeeper_core::coding_session_checkpoint::{
    decode_coding_session_checkpoint_event, CodingSessionCheckpointPayload,
    CodingSessionCheckpointReason,
};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_lifecycle_command::RewindFiles;
use beekeeper_core::coding_session_payload::{
    LifecycleReceipt, CHECKPOINT_NOT_THIS_EXECUTION, CHECKPOINT_UNAVAILABLE, NOT_RESTORABLE,
    REWIND_NOT_RESTARTED, SESSION_BUSY, SESSION_CLOSED, STALE_GENERATION, TREE_BUSY,
};
use beekeeper_core::coding_session_rewind::{
    session_rewound_item, ReceiptRewind, RewindFilesOutcome, RewindMemory,
};
use beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT;
use nostr::Event;

use crate::commands::{ResumePlan, RewindPlan};
use crate::context_projector::ContextCut;
use crate::execution_scope_host::HostGitRequest;
use crate::off_loop::OffLoopSlot;
use crate::session::{self, SessionContinuity};
use crate::turn_checkpoint::PreRewindCapture;
use crate::turn_checkpoint_git::restore::{
    objects_present, restore_obstruction, restore_tree, RestoreError, RestoreRequest,
};
use crate::turn_checkpoint_git::{
    capture_tree, diff_tree_files, CaptureFailure, CapturePhase, CapturedTree, DiffResult, RefLeaf,
};
use crate::{now_secs, Provider, SessionStatus};

/// Bound on fetching and verifying the checkpoint and taking the capture.
const PREPARE_TIMEOUT: Duration = Duration::from_secs(90);
/// Bound on the restore step, beyond its own git ceiling.
const RESTORE_STEP_TIMEOUT: Duration = Duration::from_secs(150);

/// One finished off-loop step of a rewind.
pub(crate) enum RewindDone {
    /// Steps 4–5 measured; applied by [`Provider::finish_rewind_prepared`].
    Prepared(Box<Prepared>),
    /// Step 6 finished; applied by [`Provider::finish_rewind_restored`].
    Restored(Box<Restored>),
}

/// The single slot a rewind's slow work occupies: one rewind at a time.
pub(crate) type RewindSlot = OffLoopSlot<RewindDone>;

pub(crate) fn new_slot() -> RewindSlot {
    OffLoopSlot::new(PREPARE_TIMEOUT)
}

/// What the generation a rewind opens is cut at, and what its receipt says.
#[derive(Debug, Clone)]
pub(crate) struct RewindOpen {
    /// The cut its context package applies.
    pub(crate) cut: ContextCut,
    /// The receipt's `rewind` object.
    pub(crate) rewind: ReceiptRewind,
}

/// A named refusal from steps 2–5.
#[derive(Debug, Clone)]
struct Refusal {
    code: &'static str,
    message: String,
}

fn refusal(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
    }
}

/// Where the checkpoint is read from.
enum CheckpointSource {
    Rest(Box<RestClient>),
    #[cfg(test)]
    Fixed(Option<Box<Event>>),
    Unavailable,
}

/// Steps 4–5, owned, for the off-loop task.
struct PrepareJob {
    plan: RewindPlan,
    event_id: String,
    provider_pubkey: String,
    source: CheckpointSource,
    host_git: HostGitRequest,
    cwd: PathBuf,
    through_seq: u64,
    recorded: BTreeMap<u64, Option<String>>,
}

/// What steps 4–5 found.
pub(crate) struct Prepared {
    plan: RewindPlan,
    through_seq: u64,
    host_git: HostGitRequest,
    cwd: PathBuf,
    outcome: Result<Verified, Refusal>,
}

/// A verified checkpoint and the pre-rewind capture.
struct Verified {
    checkpoint: CodingSessionCheckpointPayload,
    /// `None`: not a repository, so there is no capture (chat-only only).
    pre: Option<Result<(CapturedTree, Option<DiffResult>), CaptureFailure>>,
}

/// What step 6 did.
pub(crate) struct Restored {
    plan: RewindPlan,
    open: RewindOpen,
    result: Result<(), RestoreFailure>,
}

/// Why step 6 did not leave the base tree in place.
enum RestoreFailure {
    /// Refused before any write: the files are as they were (`kept`).
    Untouched(String),
    /// The tree may be partly restored (`restore_failed`).
    Failed(String),
}

fn inside_work_tree(cwd: &Path) -> bool {
    cwd.ancestors()
        .any(|dir| std::fs::symlink_metadata(dir.join(".git")).is_ok())
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

async fn fetch(source: CheckpointSource, id: &str) -> Result<Event, Refusal> {
    let unavailable = |why: &str| {
        refusal(
            CHECKPOINT_UNAVAILABLE,
            format!("the checkpoint could not be read: {why}"),
        )
    };
    match source {
        CheckpointSource::Rest(rest) => match rest
            .query_event_by_id(
                id,
                nostr::Kind::Custom(KIND_CODING_SESSION_CHECKPOINT as u16),
            )
            .await
        {
            Ok(Some(event)) => Ok(event),
            Ok(None) => Err(unavailable("it is not on the relay")),
            Err(_) => Err(unavailable("the relay did not answer")),
        },
        #[cfg(test)]
        CheckpointSource::Fixed(event) => event
            .map(|event| *event)
            .ok_or_else(|| unavailable("it is not on the relay")),
        CheckpointSource::Unavailable => {
            Err(unavailable("this provider has no relay to read it from"))
        }
    }
}

/// Step 4: the checkpoint is this run's and can be rewound to.
fn verify(job: &PrepareJob, event: &Event) -> Result<CodingSessionCheckpointPayload, Refusal> {
    if event.id.to_hex() != job.plan.checkpoint {
        return Err(refusal(
            CHECKPOINT_UNAVAILABLE,
            "the relay answered with another event",
        ));
    }
    let payload = decode_coding_session_checkpoint_event(event).map_err(|_| {
        refusal(
            CHECKPOINT_UNAVAILABLE,
            "the checkpoint does not decode as a checkpoint",
        )
    })?;
    let this_run = "only this run's turns can be rewound: the checkpoint";
    if !event
        .pubkey
        .to_hex()
        .eq_ignore_ascii_case(&job.provider_pubkey)
    {
        return Err(refusal(
            CHECKPOINT_NOT_THIS_EXECUTION,
            format!("{this_run} was not signed by this provider"),
        ));
    }
    let target = &job.plan.target;
    let session = &payload.session;
    if session.driver != target.driver
        || session.instance_id != target.instance_id
        || session.session_id != target.session_id
    {
        return Err(refusal(
            CHECKPOINT_NOT_THIS_EXECUTION,
            format!("{this_run} belongs to another execution"),
        ));
    }
    if session.generation != target.generation {
        return Err(refusal(
            CHECKPOINT_NOT_THIS_EXECUTION,
            format!(
                "{this_run} is from generation {}, before the session last restarted",
                session.generation
            ),
        ));
    }
    if payload.reason != CodingSessionCheckpointReason::Turn || !payload.restorable {
        return Err(refusal(
            NOT_RESTORABLE,
            "this checkpoint cannot be rewound to: the provider build that captured it cannot \
             rewind, or it is not a turn's checkpoint",
        ));
    }
    if payload.coverage.from_seq == 0 || payload.coverage.from_seq > job.through_seq + 1 {
        return Err(refusal(
            CHECKPOINT_UNAVAILABLE,
            "the checkpoint's coverage is not in this generation's record",
        ));
    }
    if job.plan.files == RewindFiles::Restore
        && payload
            .git
            .as_ref()
            .and_then(|git| git.base_tree.as_ref())
            .is_none()
    {
        return Err(refusal(
            NOT_RESTORABLE,
            "the files cannot be restored: this turn's checkpoint has no tree from before it",
        ));
    }
    let published = job.recorded.get(&payload.coverage.through_seq);
    let tree = payload.git.as_ref().map(|git| git.tree.clone());
    if published.is_some_and(|recorded| recorded != &tree) {
        return Err(refusal(
            CHECKPOINT_UNAVAILABLE,
            "the checkpoint does not match what this provider recorded publishing",
        ));
    }
    Ok(payload)
}

async fn run_prepare(job: PrepareJob) -> Prepared {
    let outcome = prepare_outcome(&job).await;
    let PrepareJob {
        plan,
        host_git,
        cwd,
        through_seq,
        ..
    } = job;
    Prepared {
        plan,
        through_seq,
        host_git,
        cwd,
        outcome,
    }
}

async fn prepare_outcome(job: &PrepareJob) -> Result<Verified, Refusal> {
    let source = match &job.source {
        CheckpointSource::Rest(rest) => CheckpointSource::Rest(rest.clone()),
        #[cfg(test)]
        CheckpointSource::Fixed(event) => CheckpointSource::Fixed(event.clone()),
        CheckpointSource::Unavailable => CheckpointSource::Unavailable,
    };
    let event = fetch(source, &job.plan.checkpoint).await?;
    let checkpoint = verify(job, &event)?;
    let restore = job.plan.files == RewindFiles::Restore;
    if !inside_work_tree(&job.cwd) {
        if restore {
            return Err(refusal(
                CHECKPOINT_UNAVAILABLE,
                "the working directory is no longer a git repository, so nothing was restored",
            ));
        }
        return Ok(Verified {
            checkpoint,
            pre: None,
        });
    }
    let scope = job.host_git.clone().prepare().await;
    let base_tree = checkpoint
        .git
        .as_ref()
        .and_then(|git| git.base_tree.clone());
    if restore {
        let (Some(git), Some(base)) = (checkpoint.git.as_ref(), base_tree.as_deref()) else {
            return Err(refusal(
                NOT_RESTORABLE,
                "the checkpoint has no tree to restore",
            ));
        };
        match objects_present(&job.cwd, scope.as_ref(), base, &git.commit).await {
            Ok(true) => {}
            Ok(false) => {
                return Err(refusal(
                    CHECKPOINT_UNAVAILABLE,
                    "the checkpoint's tree or commit is not in this repository any more",
                ))
            }
            Err(failure) => {
                return Err(refusal(
                    CHECKPOINT_UNAVAILABLE,
                    format!(
                        "the checkpoint's objects could not be checked: {}",
                        failure.sentence
                    ),
                ))
            }
        }
    }
    let captured = capture_tree(
        &job.cwd,
        scope.as_ref(),
        &job.plan.target.session_id,
        job.plan.target.generation,
        RefLeaf::PreRewind {
            through_seq: job.through_seq,
            command: &job.event_id,
        },
        None,
        CapturePhase::PreRewind,
    )
    .await;
    if let (true, Ok(tree), Some(base)) = (restore, captured.as_ref(), base_tree.as_deref()) {
        let omitted: Vec<String> = tree.omitted.iter().map(|path| path.path.clone()).collect();
        match restore_obstruction(&job.cwd, scope.as_ref(), base, &omitted).await {
            Ok(None) => {}
            Ok(Some(sentence)) => return Err(refusal(NOT_RESTORABLE, sentence)),
            Err(failure) => {
                return Err(refusal(
                    CHECKPOINT_UNAVAILABLE,
                    format!(
                        "the working tree could not be checked before the restore, so nothing \
                         was touched: {}",
                        failure.sentence
                    ),
                ))
            }
        }
    }
    let pre = match captured {
        Ok(tree) => {
            let files = match base_tree.as_deref() {
                Some(base) => diff_tree_files(&job.cwd, scope.as_ref(), base, &tree.tree)
                    .await
                    .ok(),
                None => None,
            };
            Ok((tree, files))
        }
        Err(failure) if restore => {
            return Err(refusal(
                CHECKPOINT_UNAVAILABLE,
                format!(
                    "the tree could not be captured before the restore, so nothing was touched: {}",
                    failure.sentence
                ),
            ))
        }
        Err(failure) => Err(failure),
    };
    Ok(Verified {
        checkpoint,
        pre: Some(pre),
    })
}

/// The receipt and the opening row of the generation a rewind opened.
pub(crate) fn rewound_answer(
    command_id: &str,
    target: &CodingSessionTarget,
    open: RewindOpen,
    continuity: &SessionContinuity,
    unavailable_reason: Option<&'static str>,
) -> (LifecycleReceipt, serde_json::Value) {
    let (mut receipt, memory) = match continuity {
        // The cursor was withheld, so these two cannot happen; were an
        // adapter to reattach anyway, it is still the record that seeded it.
        SessionContinuity::Rehydrated | SessionContinuity::Resumed | SessionContinuity::Loaded => (
            LifecycleReceipt::resumed(command_id, target),
            RewindMemory::Seeded,
        ),
        SessionContinuity::Fresh | SessionContinuity::RestartedWithoutContext { .. } => (
            LifecycleReceipt::resumed_without_context(
                command_id,
                target,
                "restarted with no memory: no record of the session could be attached to the new \
                 conversation",
            ),
            RewindMemory::None,
        ),
    };
    let item = session_rewound_item(command_id, &open.rewind, memory, unavailable_reason);
    receipt.rewind = Some(open.rewind);
    (receipt, item)
}

impl Provider {
    /// Whether a rewind of `session_id` is in flight off the loop, so no
    /// other lifecycle command may act on it until it settles.
    pub(crate) fn rewind_in_flight(&self, session_id: &str) -> bool {
        self.rewind.busy() && self.rewind_session.as_deref() == Some(session_id)
    }

    /// Refuse a resume, restart or stop of a session whose rewind is still
    /// running off the loop: it would race the rewind's own reopen.
    pub(crate) fn refuse_during_rewind(
        &mut self,
        command_id: &str,
        channel_id: uuid::Uuid,
    ) -> anyhow::Result<()> {
        self.state.consume_command(command_id, now_secs())?;
        self.forget_actor_seat(command_id);
        let receipt = LifecycleReceipt::failed(
            command_id,
            SESSION_BUSY,
            "a rewind of this execution is in progress; try again when it ends",
        );
        self.enqueue_receipt(channel_id, command_id, &receipt)
    }

    /// The run loop's queue of finished rewind steps.
    pub(crate) fn take_rewind_answers(
        &mut self,
    ) -> Option<tokio::sync::mpsc::Receiver<RewindDone>> {
        self.rewind.take_receiver()
    }

    /// Refuse the rewind: consume it and the seat staged for it, answer it.
    fn refuse_rewind(&mut self, plan: &RewindPlan, refusal: Refusal) -> anyhow::Result<()> {
        self.state.consume_command(&plan.command_id, now_secs())?;
        if self
            .state
            .session(&plan.target.session_id)
            .is_some_and(|record| record.actor.is_some())
        {
            self.forget_actor_seat(&plan.command_id);
        }
        tracing::info!(target: "csp::rewind", command_id = %plan.command_id, code = refusal.code, "rewind refused: {}", refusal.message);
        let receipt = LifecycleReceipt::failed(&plan.command_id, refusal.code, &refusal.message);
        self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt)
    }

    /// Steps 2–3, on the loop: the target's open turn and its siblings'.
    fn rewind_busy(&self, plan: &RewindPlan) -> Option<Refusal> {
        let record = self.state.session(&plan.target.session_id)?;
        if record.open_turn.is_some() {
            return Some(refusal(
                SESSION_BUSY,
                "a turn is open on this execution; wait for it to end or interrupt it, then rewind",
            ));
        }
        let tree = canonical(&record.cwd);
        let sibling = self.state.sessions().find(|other| {
            other.session_id != record.session_id
                && !other.closed
                && other.open_turn.is_some()
                && canonical(&other.cwd) == tree
        })?;
        let name = sibling
            .title
            .clone()
            .or_else(|| sibling.role.clone())
            .unwrap_or_else(|| sibling.session_id.clone());
        Some(refusal(
            TREE_BUSY,
            format!(
                "\"{name}\" ({}) has a turn open in the same working tree; rewinding now could \
                 undo its work. This check covers sessions of this provider only",
                sibling.session_id
            ),
        ))
    }

    /// A `session.rewind` that passed its authority check (step 1).
    pub(crate) async fn start_rewind(
        &mut self,
        plan: RewindPlan,
        event_id: &str,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        // A redelivery (reconnect replay) of the rewind that is running: it
        // is that rewind, so it gets no answer of its own and its seat stays.
        if self.rewind_command.as_deref() == Some(plan.command_id.as_str()) {
            tracing::info!(target: "csp::rewind", command_id = %plan.command_id, "redelivered rewind dropped: it is in flight");
            return Ok(());
        }
        if self.rewind.busy() {
            return self.refuse_rewind(
                &plan,
                refusal(
                    SESSION_BUSY,
                    "another rewind is in progress on this provider; try again when it ends",
                ),
            );
        }
        if let Some(busy) = self.rewind_busy(&plan) {
            return self.refuse_rewind(&plan, busy);
        }
        let Some(record) = self.state.session(&plan.target.session_id).cloned() else {
            return Ok(());
        };
        let source = match self.rest_client.clone() {
            Some(rest) => CheckpointSource::Rest(Box::new(rest)),
            None => CheckpointSource::Unavailable,
        };
        #[cfg(test)]
        let source = match self.rewind_checkpoints.get(&plan.checkpoint) {
            Some(event) => CheckpointSource::Fixed(Some(Box::new(event.clone()))),
            None if matches!(source, CheckpointSource::Unavailable) => {
                CheckpointSource::Fixed(None)
            }
            None => source,
        };
        let job = PrepareJob {
            event_id: event_id.to_owned(),
            provider_pubkey: self.pubkey_hex.clone(),
            source,
            host_git: self.host_git_request(&record, &record.cwd),
            cwd: record.cwd.clone(),
            through_seq: record.next_seq.saturating_sub(1),
            recorded: self
                .state
                .checkpoint_chain(&record.session_id, record.generation)
                .map(|chain| chain.recorded.clone())
                .unwrap_or_default(),
            plan,
        };
        if self.rewind_off_loop {
            self.rewind_session = Some(record.session_id.clone());
            self.rewind_command = Some(job.plan.command_id.clone());
            let limit = self.rewind.timeout;
            self.rewind.spawn(async move {
                let plan = job.plan.clone();
                let through_seq = job.through_seq;
                let host_git = job.host_git.clone();
                let cwd = job.cwd.clone();
                let prepared = match tokio::time::timeout(limit, run_prepare(job)).await {
                    Ok(prepared) => prepared,
                    Err(_) => Prepared {
                        plan,
                        through_seq,
                        host_git,
                        cwd,
                        outcome: Err(refusal(
                            CHECKPOINT_UNAVAILABLE,
                            "verifying the checkpoint took too long, so nothing was touched",
                        )),
                    },
                };
                RewindDone::Prepared(Box::new(prepared))
            });
            return Ok(());
        }
        // Boxed: each step's future is large, and the inline (test) path
        // nests them all on one stack.
        let prepared = Box::pin(run_prepare(job)).await;
        Box::pin(self.finish_rewind_prepared(prepared, relay)).await
    }

    /// Apply a finished off-loop step (the run loop's arm).
    pub(crate) async fn finish_rewind_step(
        &mut self,
        done: RewindDone,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        self.rewind.settle();
        // Applied on the loop, where no redelivery can arrive in between; a
        // step that spawns the next one names the command again.
        self.rewind_command = None;
        match done {
            RewindDone::Prepared(prepared) => {
                Box::pin(self.finish_rewind_prepared(*prepared, relay)).await
            }
            RewindDone::Restored(restored) => {
                Box::pin(self.finish_rewind_restored(*restored, relay)).await
            }
        }
    }

    /// Steps 5–6 on the loop: re-check, queue the `pre_rewind`, detach N,
    /// then restore (off the loop) or open N+1.
    async fn finish_rewind_prepared(
        &mut self,
        prepared: Prepared,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        let Prepared {
            plan,
            through_seq,
            host_git,
            cwd,
            outcome,
            ..
        } = prepared;
        let Some(record) = self.state.session(&plan.target.session_id).cloned() else {
            return Ok(());
        };
        if record.closed {
            return self.refuse_rewind(
                &plan,
                refusal(
                    SESSION_CLOSED,
                    "the execution was stopped while the rewind was prepared",
                ),
            );
        }
        if record.generation != plan.target.generation {
            return self.refuse_rewind(
                &plan,
                refusal(
                    STALE_GENERATION,
                    "the execution moved to another generation while the rewind was prepared",
                ),
            );
        }
        let verified = match outcome {
            Ok(verified) => verified,
            Err(refused) => return self.refuse_rewind(&plan, refused),
        };
        // Re-made here, where nothing can start in between: what ran while
        // the checkpoint was verified is not in the capture.
        if let Some(busy) = self.rewind_busy(&plan) {
            return self.refuse_rewind(&plan, busy);
        }
        if record.next_seq.saturating_sub(1) != through_seq {
            return self.refuse_rewind(
                &plan,
                refusal(SESSION_BUSY, "the session moved on while the rewind was prepared; nothing was touched, rewind again"),
            );
        }
        let restore = plan.files == RewindFiles::Restore;
        let from_seq = verified.checkpoint.coverage.from_seq;
        let base_tree = verified
            .checkpoint
            .git
            .as_ref()
            .and_then(|git| git.base_tree.clone());
        let (pre_tree, omitted, head) = match &verified.pre {
            Some(Ok((tree, _))) => (
                Some(tree.tree.clone()),
                tree.omitted.iter().map(|path| path.path.clone()).collect(),
                tree.head.clone(),
            ),
            _ => (None, Vec::new(), None),
        };
        let pre_rewind_checkpoint = match verified.pre {
            Some(measured) => {
                let queued = self.publish_pre_rewind_checkpoint(PreRewindCapture {
                    channel_id: plan.channel_id,
                    target: plan.target.clone(),
                    from_seq,
                    through_seq,
                    base_tree: base_tree.clone(),
                    measured,
                });
                match queued {
                    Ok(id) => Some(id),
                    Err(error) if restore => {
                        tracing::warn!(target: "csp::rewind", "pre_rewind checkpoint not queued: {error}");
                        return self.refuse_rewind(
                            &plan,
                            refusal(CHECKPOINT_UNAVAILABLE, "the pre-rewind checkpoint could not be recorded, so nothing was touched"),
                        );
                    }
                    Err(error) => {
                        tracing::warn!(target: "csp::rewind", "pre_rewind checkpoint not queued; chat-only rewind continues: {error}");
                        None
                    }
                }
            }
            None => None,
        };
        let open = RewindOpen {
            cut: ContextCut {
                target: plan.target.clone(),
                after_seq: from_seq.saturating_sub(1),
            },
            rewind: ReceiptRewind {
                checkpoint: plan.checkpoint.clone(),
                cut_generation: plan.target.generation,
                cut_after_seq: from_seq.saturating_sub(1),
                previous_generation: plan.target.generation,
                files: if restore {
                    RewindFilesOutcome::Restored
                } else {
                    RewindFilesOutcome::Kept
                },
                pre_rewind_checkpoint,
                head,
            },
        };
        self.detach_for_new_generation(&plan.command_id, &plan.target, plan.channel_id)
            .await?;
        if !restore {
            return self.open_rewound(plan, open, relay).await;
        }
        let (Some(base_tree), Some(pre_tree)) = (base_tree, pre_tree) else {
            let mut open = open;
            open.rewind.files = RewindFilesOutcome::RestoreFailed;
            return self
                .rewind_not_restarted(
                    &plan,
                    open,
                    "the trees a restore needs were not captured",
                    relay,
                )
                .await;
        };
        let job = RestoreJob {
            plan,
            open,
            host_git,
            cwd,
            base_tree,
            pre_tree,
            omitted,
        };
        if self.rewind_off_loop {
            self.rewind_session = Some(job.plan.target.session_id.clone());
            self.rewind_command = Some(job.plan.command_id.clone());
            self.rewind.spawn(async move {
                let restored = run_restore(job).await;
                RewindDone::Restored(Box::new(restored))
            });
            return Ok(());
        }
        let restored = Box::pin(run_restore(job)).await;
        Box::pin(self.finish_rewind_restored(restored, relay)).await
    }

    /// Step 6's answer, on the loop.
    async fn finish_rewind_restored(
        &mut self,
        restored: Restored,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        let Restored {
            plan,
            mut open,
            result,
        } = restored;
        let why = match result {
            Ok(()) => return self.open_rewound(plan, open, relay).await,
            Err(RestoreFailure::Untouched(why)) => {
                open.rewind.files = RewindFilesOutcome::Kept;
                why
            }
            Err(RestoreFailure::Failed(why)) => {
                open.rewind.files = RewindFilesOutcome::RestoreFailed;
                why
            }
        };
        Box::pin(self.rewind_not_restarted(&plan, open, &why, relay)).await
    }

    /// The restart's detach: the child ends (joined), its packages go, the
    /// record stays resumable, and N reads `disconnected`.
    async fn detach_for_new_generation(
        &mut self,
        command_id: &str,
        target: &CodingSessionTarget,
        channel_id: uuid::Uuid,
    ) -> anyhow::Result<()> {
        if self.sessions.handle(&target.session_id).is_none() {
            return Ok(());
        }
        let quiescence = self
            .sessions
            .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
            .await;
        self.discard_context_packages(&target.session_id);
        self.publish_metadata(channel_id, target, SessionStatus::Disconnected)?;
        tracing::info!(target: "csp::rewind", %command_id, session_id = %target.session_id, quiescence = ?quiescence, "session detached for rewind");
        Ok(())
    }

    /// Step 7: open N+1 through the resume path, cut at the rewind.
    async fn open_rewound(
        &mut self,
        plan: RewindPlan,
        open: RewindOpen,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        Box::pin(self.resume_generation(
            ResumePlan {
                command_id: plan.command_id,
                channel_id: plan.channel_id,
                target: plan.target,
            },
            relay,
            Some(open),
        ))
        .await
    }

    /// A failure inside the resume path: a plain refusal, or for a rewind,
    /// step 8.
    pub(crate) async fn answer_resume_failure(
        &mut self,
        plan: &ResumePlan,
        rewind: Option<RewindOpen>,
        receipt: LifecycleReceipt,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        let Some(open) = rewind else {
            return self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt);
        };
        let why = receipt
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| "the new generation did not open".to_owned());
        let plan = RewindPlan {
            command_id: plan.command_id.clone(),
            channel_id: plan.channel_id,
            target: plan.target.clone(),
            checkpoint: open.rewind.checkpoint.clone(),
            files: RewindFiles::Keep,
        };
        Box::pin(self.rewind_not_restarted(&plan, open, &why, relay)).await
    }

    /// Step 8: reopen N in place when it can be, and answer
    /// `REWIND_NOT_RESTARTED` with what became of the files.
    async fn rewind_not_restarted(
        &mut self,
        plan: &RewindPlan,
        open: RewindOpen,
        why: &str,
        relay: Option<&HarnessRelay>,
    ) -> anyhow::Result<()> {
        if !self.state.is_command_consumed(&plan.command_id) {
            self.state.consume_command(&plan.command_id, now_secs())?;
        }
        if self
            .state
            .session(&plan.target.session_id)
            .is_some_and(|record| record.actor.is_some())
        {
            self.forget_actor_seat(&plan.command_id);
        }
        let files = match open.rewind.files {
            RewindFilesOutcome::Kept => "the files were kept",
            RewindFilesOutcome::Restored => "the files were restored",
            RewindFilesOutcome::RestoreFailed => {
                "restoring the files failed part way; the pre-rewind checkpoint holds the tree as it was"
            }
        };
        let generation = plan.target.generation;
        let after = open.rewind.cut_after_seq;
        let reopened = Box::pin(self.restore_generation(&plan.target.session_id, relay)).await;
        let message = match reopened {
            Ok(Ok(())) => format!(
                "{files}, but the agent was not restarted ({why}); generation {generation} was \
                 reopened and still remembers the turns after item {after}"
            ),
            Ok(Err(obstacle)) => format!(
                "{files}, but the agent was not restarted ({why}) and generation {generation} \
                 could not be reopened ({}); it is detached and needs a Restart",
                obstacle.message()
            ),
            Err(error) => {
                tracing::warn!(target: "csp::rewind", "reopening the rewound generation failed: {error}");
                format!(
                    "{files}, but the agent was not restarted ({why}) and generation {generation} \
                     is detached; it needs a Restart"
                )
            }
        };
        let mut receipt =
            LifecycleReceipt::failed(&plan.command_id, REWIND_NOT_RESTARTED, &message);
        receipt.rewind = Some(open.rewind);
        tracing::warn!(target: "csp::rewind", command_id = %plan.command_id, "rewind not restarted: {message}");
        self.enqueue_receipt(plan.channel_id, &plan.command_id, &receipt)
    }
}

/// Step 6, owned, for the off-loop task.
struct RestoreJob {
    plan: RewindPlan,
    open: RewindOpen,
    host_git: HostGitRequest,
    cwd: PathBuf,
    base_tree: String,
    pre_tree: String,
    omitted: Vec<String>,
}

async fn run_restore(job: RestoreJob) -> Restored {
    let RestoreJob {
        plan,
        open,
        host_git,
        cwd,
        base_tree,
        pre_tree,
        omitted,
    } = job;
    let work = async {
        let scope = host_git.prepare().await;
        let report = restore_tree(RestoreRequest {
            cwd: &cwd,
            scope: scope.as_ref(),
            base_tree: &base_tree,
            pre_tree: &pre_tree,
            omitted: &omitted,
        })
        .await
        .map_err(|error| match error {
            RestoreError::Refused(sentence) => RestoreFailure::Untouched(sentence),
            RestoreError::Failed(failure) => RestoreFailure::Failed(failure.sentence),
        })?;
        if let Some(first) = report.missing.first() {
            return Err(RestoreFailure::Failed(format!(
                "{} file(s) of the restored tree are missing after the restore, \"{first}\" first",
                report.missing.len()
            )));
        }
        match report.left.first() {
            None => Ok(()),
            Some(first) => Err(RestoreFailure::Failed(format!(
                "{} path(s) the rewound turns added were left, \"{first}\" first, because \
                 removing them would follow a link, remove a directory, or could take a restored \
                 file with them",
                report.left.len()
            ))),
        }
    };
    let result = match tokio::time::timeout(RESTORE_STEP_TIMEOUT, work).await {
        Ok(result) => result,
        Err(_) => Err(RestoreFailure::Failed(
            "restoring the files took too long; the tree may be partly restored".into(),
        )),
    };
    Restored { plan, open, result }
}
