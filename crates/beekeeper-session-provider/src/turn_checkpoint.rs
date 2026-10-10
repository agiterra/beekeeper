//! The provider half of a turn checkpoint (SV-28, NIP-CSCK `kind:44231`):
//! when to capture, and what to publish.
//!
//! A turn has two captures, both run by [`crate::turn_checkpoint_git`] and
//! both **off the provider loop** (the SV-72/SV-76 rule — host Git, its
//! boundary preparation and the relay are never awaited on the loop):
//!
//! - **Baseline.** Once the turn's opening items are queued — so the prompt's
//!   `eventSeq` is known — and before the prompt is sent, the actor asks for
//!   one ([`await_baseline`]) and waits at most [`BASELINE_WAIT`]
//!   ([`FIRST_TURN_BASELINE_WAIT`] for a generation's first prompted turn,
//!   whose cold index is the slowest to capture — ledger 371). The
//!   provider spawns the capture at `base-<firstSeq>` and answers on the
//!   request's channel; the actor forwards what it received as
//!   [`SessionEvent::BaselineCaptured`], so the baseline is folded before any
//!   later item of the turn. A capture that is late, or fails, is no
//!   baseline: the turn goes ahead and its checkpoint says `baseTree: null`.
//! - **End.** When the terminal result has its `eventSeq`, the provider spawns
//!   the capture at `<throughSeq>`, lists the files from the baseline's tree
//!   to it, and sends the result back as [`SessionEvent::CheckpointCaptured`].
//!   The loop arm maps it onto the wire and queues it at normal priority:
//!   the terminal 44225 and the 44223 status were queued before the capture
//!   even started, so a slow or stuck capture cannot hold them.
//! - **Overlap.** The next turn's baseline first waits (inside the same
//!   [`BASELINE_WAIT`]) for the session's end captures still running. If the
//!   actor gives up first, its prompt goes out while a capture may still be
//!   reading the tree, so that capture can hold the next turn's edits: the
//!   provider marks it when the actor reports [`SessionEvent::BaselineSettled`]
//!   (or an autonomous turn starts), and it is published `complete: false`.
//!   Both reports ride the one ordered inbox and a capture reports only after
//!   it measured, so a capture measured before the prompt went out is never
//!   marked and one still measuring when it went out always is.
//!
//! Only object ids, repo-relative paths and a short branch name reach the
//! wire; the working directory and the ref names stay on this host.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use beekeeper_core::coding_session_checkpoint::{
    coding_session_checkpoint_semantic_key, is_publishable_checkpoint_path,
    CodingSessionCheckpointCoverage, CodingSessionCheckpointFile,
    CodingSessionCheckpointFileStatus, CodingSessionCheckpointGit, CodingSessionCheckpointOmission,
    CodingSessionCheckpointOmissionReason, CodingSessionCheckpointPayload,
    CodingSessionCheckpointReason, CodingSessionCheckpointUnavailable,
    CodingSessionCheckpointUnavailableCode, CODING_SESSION_CHECKPOINT_SCHEMA, MAX_CHECKPOINT_FILES,
    MAX_CHECKPOINT_OMITTED, MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES,
};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT;
use beekeeper_sdk::coding_session_checkpoint::build_coding_session_checkpoint;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::execution_scope_host::{HostGitRequest, HostLaunchPlan};
use crate::publish::Priority;
use crate::session::SessionEvent;
use crate::state;
use crate::turn_checkpoint_git::{
    capture_tree, capture_tree_within, diff_tree_files, CaptureFailure, CapturePhase, CapturedTree,
    ChangedFile, DiffResult, FileChange, OmittedPath, RefLeaf, UnavailableCode, DIFF_TIMEOUT,
};

/// Whether this build implements `session.rewind` (SV-29). Every `turn`
/// checkpoint it publishes says `restorable: true` when this holds: the
/// conversation can be rewound to that turn whatever the git facts say
/// (ledger 371). Restoring the *files* additionally needs `git.baseTree`,
/// which the rewind checks on its own (`files: restore` without one is
/// `NOT_RESTORABLE`).
pub(crate) const BUILD_IMPLEMENTS_REWIND: bool = true;

/// How long an actor holds a turn's prompt for its baseline, for every turn
/// but a generation's first. Above the capture's own 3 s ceiling, so a
/// capture that makes its ceiling is used; a cold boundary preparation in
/// front of it can still miss, and then the turn simply has no baseline.
pub const BASELINE_WAIT: Duration = Duration::from_millis(3_500);

/// How long an actor holds the prompt of a generation's **first** prompted
/// turn (no end capture started in the generation yet) for its baseline.
///
/// Ledger 371: in a large fresh worktree every index entry is racy, so the
/// first capture rehashes the whole tree, and the 3.5 s bound published the
/// first turn of a Tank Loop run with `baseTree: null` — no files, and no
/// "Chat and files" rewind for it. Measured 2026-10-10 with
/// `bench_baseline_capture` on fresh `git clone --local`s of this repository
/// (7,454 tracked files, test build): the first capture took 2.2–3.6 s, so
/// the 3 s capture ceiling already loses it on this tree, and a larger one
/// is further out. T3 Code gives git 30 s per command; a generation's first
/// turn gets the same, once. Later turns keep [`BASELINE_WAIT`]. An
/// interrupt, a steer or a shutdown still ends the wait at once.
pub const FIRST_TURN_BASELINE_WAIT: Duration = Duration::from_secs(30);

/// How long the actor waits for the provider to take the request up at all.
/// A loop that has not reached it by then is busy, and the prompt is not
/// held behind it.
pub const BASELINE_PICKUP_WAIT: Duration = Duration::from_millis(750);

/// How often the actor looks for a command waiting in its mailbox.
const MAILBOX_POLL: Duration = Duration::from_millis(10);

/// How long a clean shutdown waits for end captures already running.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(5);

/// Where a baseline request stands, as the provider reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineAnswer {
    /// Not taken up yet.
    Requested,
    /// The provider started the capture; the actor waits up to
    /// [`BASELINE_WAIT`] in all.
    Capturing,
    /// The provider started the capture of its generation's first prompted
    /// turn; the actor waits up to [`FIRST_TURN_BASELINE_WAIT`] in all.
    CapturingFirstTurn,
    /// The baseline tree's object id.
    Captured(String),
}

/// The turn a baseline is asked for.
pub(crate) struct BaselineRequest<'a> {
    /// The session's working directory.
    pub(crate) cwd: &'a Path,
    /// Which session.
    pub(crate) session_id: &'a str,
    /// The turn about to be prompted.
    pub(crate) turn_id: &'a str,
}

/// Whether `cwd` is inside a git working tree, by a `.git` entry in it or an
/// ancestor — a few `stat`s, no git. A tree outside one has no baseline to
/// take, and its prompt is not held at all; its end capture still says
/// `NOT_A_REPOSITORY`.
fn inside_work_tree(cwd: &Path) -> bool {
    cwd.ancestors()
        .any(|dir| std::fs::symlink_metadata(dir.join(".git")).is_ok())
}

/// How long the actor holds a prompt for its baseline in all, once the
/// provider has taken the request up as `answer`.
#[must_use]
pub fn baseline_wait(answer: &BaselineAnswer) -> Duration {
    match answer {
        BaselineAnswer::CapturingFirstTurn => FIRST_TURN_BASELINE_WAIT,
        BaselineAnswer::Requested | BaselineAnswer::Capturing | BaselineAnswer::Captured(_) => {
            BASELINE_WAIT
        }
    }
}

/// Ask the provider for this turn's baseline and wait for it, bounded.
///
/// Asks nothing outside a git working tree. Never fails the turn and never
/// holds it past [`baseline_wait`] (counted from the request): a closed
/// inbox, a request not taken up within [`BASELINE_PICKUP_WAIT`], a refused
/// capture, a command arriving in the actor's mailbox meanwhile (an interrupt,
/// a steer — the prompt loop answers it) or a shutdown all return at
/// once with no baseline. Shutdown is watched on a clone and the mailbox is
/// only looked at, so the prompt loop still sees both.
pub(crate) async fn await_baseline<C>(
    events: &mpsc::Sender<SessionEvent>,
    shutdown: &watch::Receiver<bool>,
    mailbox: &mpsc::Receiver<C>,
    turn: BaselineRequest<'_>,
) {
    let BaselineRequest {
        cwd,
        session_id,
        turn_id,
    } = turn;
    if !inside_work_tree(cwd) {
        return;
    }
    let (reply, mut answer) = watch::channel(BaselineAnswer::Requested);
    let request = SessionEvent::TurnBaselineRequested {
        session_id: session_id.to_owned(),
        turn_id: turn_id.to_owned(),
        reply,
    };
    if events.send(request).await.is_err() {
        return;
    }
    let mut stop = shutdown.clone();
    let stopped = async move {
        // A dropped shutdown sender is not a shutdown.
        if stop.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    // Only a command that arrives now: turns already queued behind this one
    // are no reason to skip its baseline.
    let queued = mailbox.len();
    let commanded = async {
        while mailbox.len() <= queued {
            tokio::time::sleep(MAILBOX_POLL).await;
        }
    };
    let answered = async {
        let asked = tokio::time::Instant::now();
        // Until the provider takes the request up, only the pickup bound;
        // then the whole wait its answer names, counted from the request.
        let mut deadline = asked + BASELINE_PICKUP_WAIT;
        loop {
            tokio::select! {
                changed = answer.changed() => {
                    if changed.is_err() {
                        return None;
                    }
                    let now = answer.borrow_and_update().clone();
                    match now {
                        BaselineAnswer::Captured(tree) => return Some(tree),
                        BaselineAnswer::Requested => {}
                        taken_up => deadline = asked + baseline_wait(&taken_up),
                    }
                }
                () = tokio::time::sleep_until(deadline) => return None,
            }
        }
    };
    let base_tree = tokio::select! {
        biased;
        () = stopped => None,
        () = commanded => None,
        base_tree = answered => base_tree,
    };
    // Dropped before the prompt is sent, so a capture finishing after this
    // point finds nobody listening and is not used as the baseline.
    drop(answer);
    // Sent with or without a baseline, and before the prompt: the provider
    // reads it as "this turn's edits can begin now".
    let _ = events
        .send(SessionEvent::BaselineSettled {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            base_tree,
        })
        .await;
}

/// Whether the turn a baseline is asked for is its generation's first: no
/// end capture has started in `chain` yet (ledger 371). Its capture then
/// gets [`FIRST_TURN_BASELINE_WAIT`].
fn is_first_prompted_turn(chain: Option<&state::CheckpointChain>) -> bool {
    chain.is_none_or(|chain| chain.last_started.is_none())
}

/// Whether a transcript item can open a turn's checkpoint coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoverageItem {
    /// The turn's own, non-steered `user_prompt`.
    Prompt,
    /// Anything else (the first item of an autonomous turn opens it).
    Other,
}

impl CoverageItem {
    /// Classify `item` before it is queued.
    pub(crate) fn of(item: &serde_json::Value) -> Self {
        let prompt = item["kind"] == "user_prompt"
            && item.get("steered") != Some(&serde_json::Value::Bool(true));
        if prompt {
            Self::Prompt
        } else {
            Self::Other
        }
    }
}

/// The turn-checkpoint captures in flight.
#[derive(Debug, Default)]
pub(crate) struct TurnCheckpoints {
    tasks: tokio::task::JoinSet<()>,
    /// Each session's end captures not yet published.
    end_in_flight: HashMap<String, Vec<PendingEnd>>,
    /// Test-only override of the baseline capture's ceiling
    /// ([`CapturePhase::Baseline`]'s otherwise).
    #[cfg(test)]
    pub(crate) baseline_ceiling: Option<Duration>,
    /// Test-only: end captures wait for a permit before they start.
    #[cfg(test)]
    pub(crate) hold: Option<std::sync::Arc<tokio::sync::Semaphore>>,
}

impl TurnCheckpoints {
    /// Reap captures that have finished.
    pub(crate) fn reap(&mut self) {
        while self.tasks.try_join_next().is_some() {}
        self.end_in_flight.retain(|_, pending| {
            pending.retain(PendingEnd::unpublished);
            !pending.is_empty()
        });
    }

    /// `session_id`'s next turn can start editing now (its prompt goes out
    /// next, or it is autonomous): every end capture of the session not yet
    /// reported may measure that turn's edits too, so none of them is
    /// published as a complete measurement. No await: called on the loop.
    pub(crate) fn next_turn_started(&mut self, session_id: &str) {
        let Some(pending) = self.end_in_flight.get_mut(session_id) else {
            return;
        };
        pending.retain(PendingEnd::unpublished);
        for end in pending.iter() {
            end.overlapped.store(true, Ordering::SeqCst);
        }
        if pending.is_empty() {
            self.end_in_flight.remove(session_id);
        }
    }

    /// Register a new end capture for `session_id`: the flag rides the
    /// capture to its publication, and the sender is dropped once the
    /// capture has reported.
    fn end_capture_started(&mut self, session_id: &str) -> (Arc<AtomicBool>, watch::Sender<()>) {
        let overlapped = Arc::new(AtomicBool::new(false));
        let (reported, done) = watch::channel(());
        let pending = self.end_in_flight.entry(session_id.to_owned()).or_default();
        pending.retain(PendingEnd::unpublished);
        pending.push(PendingEnd {
            overlapped: overlapped.clone(),
            done,
        });
        (overlapped, reported)
    }

    /// One receiver per end capture of `session_id` that has not reported
    /// yet; each errors once its capture has reported.
    fn ends_running(&self, session_id: &str) -> Vec<watch::Receiver<()>> {
        self.end_in_flight
            .get(session_id)
            .into_iter()
            .flatten()
            .filter(|end| end.done.has_changed().is_ok())
            .map(|end| end.done.clone())
            .collect()
    }

    /// Await every capture in flight. Test-only.
    #[cfg(test)]
    pub(crate) async fn settle(&mut self) {
        let settled = tokio::time::timeout(Duration::from_secs(60), async {
            while self.tasks.join_next().await.is_some() {}
        })
        .await;
        assert!(settled.is_ok(), "a checkpoint capture did not finish");
    }
}

/// An end capture not yet published.
#[derive(Debug)]
struct PendingEnd {
    /// Shared with the capture; set when the next turn may have begun.
    overlapped: Arc<AtomicBool>,
    /// Errors on receive once the capture has reported.
    done: watch::Receiver<()>,
}

impl PendingEnd {
    /// The capture (or its report, still in the inbox) holds the flag.
    fn unpublished(&self) -> bool {
        Arc::strong_count(&self.overlapped) > 1
    }
}

/// What an end capture measured, with what the provider knew when it
/// started, ready to be mapped onto a 44231. Opaque outside this module.
#[derive(Debug, Clone)]
pub struct TurnCheckpointCapture {
    channel_id: Uuid,
    target: CodingSessionTarget,
    turn_id: String,
    from_seq: u64,
    through_seq: u64,
    base_tree: Option<String>,
    previous: Option<u64>,
    measured: Measured,
    /// Set when the session's next turn could start editing before this
    /// capture reported, so its tree (and the files listed against it) may
    /// include that turn's edits.
    overlapped: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
enum Measured {
    Captured {
        tree: CapturedTree,
        files: Option<DiffResult>,
    },
    Failed(CaptureFailure),
}

/// What an end capture is started from.
struct EndJob {
    host_git: HostGitRequest,
    cwd: PathBuf,
    capture: TurnCheckpointCapture,
}

/// One step of the shutdown drain.
enum DrainStep {
    Joined(bool),
    Event(Option<Box<SessionEvent>>),
    Elapsed,
}

impl crate::Provider {
    /// On a clean shutdown: publish the end captures that finish within
    /// [`SHUTDOWN_DRAIN`], then stop the rest. Other session reports arriving
    /// meanwhile are not handled, as they would not be once the loop exits.
    pub(crate) async fn drain_turn_checkpoints(&mut self) {
        let deadline = tokio::time::Instant::now() + SHUTDOWN_DRAIN;
        loop {
            let step = tokio::select! {
                joined = self.turn_checkpoints.tasks.join_next() => DrainStep::Joined(joined.is_some()),
                event = self.session_events.recv() => DrainStep::Event(event.map(Box::new)),
                () = tokio::time::sleep_until(deadline) => DrainStep::Elapsed,
            };
            match step {
                DrainStep::Joined(true) => {}
                DrainStep::Joined(false) | DrainStep::Event(None) | DrainStep::Elapsed => break,
                DrainStep::Event(Some(event)) => self.publish_drained(*event),
            }
        }
        self.turn_checkpoints.tasks.shutdown().await;
        while let Ok(event) = self.session_events.try_recv() {
            self.publish_drained(event);
        }
    }

    fn publish_drained(&mut self, event: SessionEvent) {
        if let SessionEvent::CheckpointCaptured {
            session_id,
            capture,
        } = event
        {
            if let Err(error) = self.publish_turn_checkpoint(&session_id, *capture) {
                tracing::warn!(target: "csp::checkpoint", %session_id, "checkpoint not queued at shutdown: {error}");
            }
        }
    }

    /// Note the first item of the open turn's checkpoint coverage.
    pub(crate) fn note_checkpoint_coverage(
        &mut self,
        session_id: &str,
        turn_id: &str,
        item: CoverageItem,
        seq: Option<u64>,
    ) {
        let Some(seq) = seq else { return };
        let Some(record) = self.state.session(session_id) else {
            return;
        };
        let Some(open) = record
            .open_turn
            .as_ref()
            .filter(|open| open.turn_id == turn_id)
        else {
            return;
        };
        let autonomous = open.command_id.is_none();
        let generation = record.generation;
        let noted = self
            .state
            .checkpoint_chain(session_id, generation)
            .and_then(|chain| chain.open.as_ref())
            .is_some_and(|turn| turn.turn_id == turn_id);
        if noted || (!autonomous && item != CoverageItem::Prompt) {
            return;
        }
        let opened = state::CheckpointTurn {
            turn_id: turn_id.to_owned(),
            first_seq: seq,
            base_tree: None,
        };
        if let Err(error) = self
            .state
            .update_checkpoint_chain(session_id, generation, |chain| chain.open = Some(opened))
        {
            tracing::warn!(target: "csp::checkpoint", %session_id, "could not record the turn's first seq: {error}");
        }
    }

    /// The open turn's checkpoint facts, when they belong to `turn_id`.
    fn checkpoint_turn(&self, session_id: &str, turn_id: &str) -> Option<&state::CheckpointTurn> {
        let generation = self.state.session(session_id)?.generation;
        self.state
            .checkpoint_chain(session_id, generation)?
            .open
            .as_ref()
            .filter(|turn| turn.turn_id == turn_id)
    }

    /// Start the open turn's baseline capture off the loop. Dropping `reply`
    /// unanswered is "no baseline".
    pub(crate) fn start_turn_baseline(
        &mut self,
        session_id: &str,
        turn_id: &str,
        reply: watch::Sender<BaselineAnswer>,
    ) {
        let Some(first_seq) = self
            .checkpoint_turn(session_id, turn_id)
            .map(|turn| turn.first_seq)
        else {
            return;
        };
        let Some(record) = self.state.session(session_id) else {
            return;
        };
        let cwd = record.cwd.clone();
        let host_git = self.host_git_request(record, &cwd);
        let generation = record.generation;
        // Ledger 371: no end capture started in this generation yet, so this
        // is its first prompted turn, and its capture gets the long bound.
        let first_turn =
            is_first_prompted_turn(self.state.checkpoint_chain(session_id, generation));
        let (taken_up, phase) = if first_turn {
            (
                BaselineAnswer::CapturingFirstTurn,
                CapturePhase::FirstBaseline,
            )
        } else {
            (BaselineAnswer::Capturing, CapturePhase::Baseline)
        };
        let session_id = session_id.to_owned();
        #[cfg(test)]
        let ceiling = self.turn_checkpoints.baseline_ceiling;
        #[cfg(not(test))]
        let ceiling = None;
        // The previous turn's end captures still running: the baseline is
        // taken after they report, so its prompt does not overlap them.
        let ends = self.turn_checkpoints.ends_running(&session_id);
        // An error is the actor having stopped waiting already.
        if reply.send(taken_up).is_err() {
            return;
        }
        self.turn_checkpoints.tasks.spawn(async move {
            let ends_reported = async move {
                for mut end in ends {
                    while end.changed().await.is_ok() {}
                }
            };
            // The actor giving up first means no baseline (and its report
            // marks the captures still running as overlapped).
            let waited = async {
                tokio::select! {
                    () = ends_reported => true,
                    () = reply.closed() => false,
                }
            };
            let (scope, waited) = tokio::join!(host_git.prepare(), waited);
            if !waited {
                return;
            }
            let leaf = RefLeaf::Base(first_seq);
            let captured = match ceiling {
                Some(ceiling) => {
                    capture_tree_within(&cwd, scope.as_ref(), &session_id, generation, leaf, None, ceiling)
                        .await
                }
                None => {
                    capture_tree(
                        &cwd,
                        scope.as_ref(),
                        &session_id,
                        generation,
                        leaf,
                        None,
                        phase,
                    )
                    .await
                }
            };
            let tree = match captured {
                Ok(captured) => captured.tree,
                Err(CaptureFailure {
                    already_pinned: Some(pinned),
                    ..
                }) => pinned.tree,
                Err(failure) => {
                    tracing::debug!(target: "csp::checkpoint", %session_id, code = failure.code.as_str(), "no baseline: {}", failure.sentence);
                    return;
                }
            };
            // An error is the actor having stopped waiting: not a baseline.
            let _ = reply.send(BaselineAnswer::Captured(tree));
        });
    }

    /// Fold the baseline the actor accepted into the open turn's facts.
    pub(crate) fn note_turn_baseline(
        &mut self,
        session_id: &str,
        turn_id: &str,
        base_tree: String,
    ) {
        let Some(generation) = self
            .state
            .session(session_id)
            .map(|record| record.generation)
        else {
            return;
        };
        if let Err(error) = self
            .state
            .update_checkpoint_chain(session_id, generation, |chain| {
                if let Some(turn) = chain.open.as_mut().filter(|turn| turn.turn_id == turn_id) {
                    turn.base_tree = Some(base_tree);
                }
            })
        {
            tracing::warn!(target: "csp::checkpoint", %session_id, "could not record the turn's baseline: {error}");
        }
    }

    /// Start the turn's end capture off the loop. Call while the turn is
    /// still open, once its terminal item has `through_seq`. Never fails the
    /// caller: what this cannot start, it logs.
    pub(crate) fn start_turn_checkpoint(
        &mut self,
        session_id: &str,
        channel_id: Uuid,
        target: &CodingSessionTarget,
        turn_id: &str,
        through_seq: u64,
    ) {
        let Some(record) = self.state.session(session_id) else {
            return;
        };
        // An autonomous turn had no prompt to take a baseline before.
        let prompted = record
            .open_turn
            .as_ref()
            .filter(|open| open.turn_id == turn_id)
            .is_some_and(|open| open.command_id.is_some());
        let cwd = record.cwd.clone();
        let host_git = self.host_git_request(record, &cwd);
        let turn = self.checkpoint_turn(session_id, turn_id).cloned();
        let from_seq = turn
            .as_ref()
            .map_or(through_seq, |turn| turn.first_seq)
            .min(through_seq);
        let base_tree = turn.and_then(|turn| turn.base_tree).filter(|_| prompted);
        let previous = self
            .state
            .update_checkpoint_chain(session_id, target.generation, |chain| {
                chain.open = None;
                chain.last_started.replace(through_seq)
            })
            .unwrap_or_else(|error| {
                tracing::warn!(target: "csp::checkpoint", %session_id, "could not record the checkpoint chain: {error}");
                None
            });
        let (overlapped, reported) = self.turn_checkpoints.end_capture_started(session_id);
        let job = EndJob {
            host_git,
            cwd,
            capture: TurnCheckpointCapture {
                channel_id,
                target: target.clone(),
                turn_id: turn_id.to_owned(),
                from_seq,
                through_seq,
                base_tree,
                previous,
                measured: Measured::Failed(not_measured()),
                overlapped,
            },
        };
        let events = self.session_events_tx.clone();
        #[cfg(test)]
        let hold = self.turn_checkpoints.hold.clone();
        self.turn_checkpoints.tasks.spawn(async move {
            let EndJob {
                host_git,
                cwd,
                mut capture,
            } = job;
            let scope = host_git.prepare().await;
            #[cfg(test)]
            if let Some(hold) = hold {
                let _ = hold.acquire().await;
            }
            capture.measured = measure_end(&cwd, scope.as_ref(), &capture).await;
            let session_id = capture.target.session_id.clone();
            let _ = events
                .send(SessionEvent::CheckpointCaptured {
                    session_id,
                    capture: Box::new(capture),
                })
                .await;
            // Reported: a next turn's baseline waiting on this may go ahead.
            drop(reported);
        });
    }

    /// Map a finished end capture onto a 44231, sign it with the key that
    /// signs this generation's 44225 items, and queue it.
    pub(crate) fn publish_turn_checkpoint(
        &mut self,
        session_id: &str,
        capture: TurnCheckpointCapture,
    ) -> anyhow::Result<()> {
        let live = self.state.session(session_id).is_some_and(|record| {
            record.generation == capture.target.generation && !record.is_retired()
        });
        if !live {
            return Ok(());
        }
        let previous_tree = capture.previous.and_then(|seq| {
            self.state
                .checkpoint_chain(session_id, capture.target.generation)
                .and_then(|chain| chain.recorded.get(&seq).cloned().flatten())
        });
        let payload = checkpoint_payload(&capture, previous_tree.as_deref());
        let (event, published_tree) = match fit_checkpoint(capture.channel_id, payload) {
            Ok(fitted) => fitted,
            Err(error) => {
                tracing::warn!(target: "csp::checkpoint", %session_id, "checkpoint not encodable, publishing it as unavailable: {error}");
                let mut fallback = checkpoint_payload(&capture, None);
                fallback.git = None;
                fallback.files.clear();
                fallback.files_not_listed = 0;
                fallback.unavailable = Some(CodingSessionCheckpointUnavailable {
                    code: CodingSessionCheckpointUnavailableCode::GitFailed,
                    sentence: "The capture could not be encoded as a checkpoint, so nothing \
                               measured is published."
                        .to_owned(),
                });
                fit_checkpoint(capture.channel_id, fallback)?
            }
        };
        let event = event.sign_with_keys(&self.config.keys)?;
        let key = coding_session_checkpoint_semantic_key(
            &capture.target,
            CodingSessionCheckpointReason::Turn,
            capture.through_seq,
        );
        self.outbox.enqueue(
            KIND_CODING_SESSION_CHECKPOINT,
            &key,
            Priority::Normal,
            event,
        )?;
        self.state.record_checkpoint(
            session_id,
            capture.target.generation,
            capture.through_seq,
            published_tree,
        )?;
        Ok(())
    }
}

/// What a rewind measured before it touched anything, for its `pre_rewind`
/// 44231 (NIP-CSCK § `pre_rewind`).
pub(crate) struct PreRewindCapture {
    /// Channel the generation publishes into.
    pub(crate) channel_id: Uuid,
    /// The generation being rewound.
    pub(crate) target: CodingSessionTarget,
    /// The rewound checkpoint's `fromSeq`: the first item the rewind forgets.
    pub(crate) from_seq: u64,
    /// The last seq the rewound generation wrote.
    pub(crate) through_seq: u64,
    /// The rewound checkpoint's `baseTree`, so `files` lists what a restore
    /// would undo.
    pub(crate) base_tree: Option<String>,
    /// The capture and its files from `base_tree`, or why there is none.
    pub(crate) measured: Result<(CapturedTree, Option<DiffResult>), CaptureFailure>,
}

impl crate::Provider {
    /// Sign and durably queue a rewind's `pre_rewind` checkpoint at high
    /// priority, before any file is written. Returns its event id.
    pub(crate) fn publish_pre_rewind_checkpoint(
        &mut self,
        capture: PreRewindCapture,
    ) -> anyhow::Result<String> {
        let PreRewindCapture {
            channel_id,
            target,
            from_seq,
            through_seq,
            base_tree,
            measured,
        } = capture;
        let measured = match measured {
            Ok((tree, files)) => Measured::Captured { tree, files },
            Err(failure) => Measured::Failed(failure),
        };
        let shaped = TurnCheckpointCapture {
            channel_id,
            target: target.clone(),
            turn_id: String::new(),
            from_seq: from_seq.min(through_seq),
            through_seq,
            base_tree,
            previous: None,
            measured,
            overlapped: Arc::default(),
        };
        let mut payload = checkpoint_payload(&shaped, None);
        payload.reason = CodingSessionCheckpointReason::PreRewind;
        payload.turn_id = None;
        // A pre_rewind is the undo point, not a turn a rewind can name.
        payload.restorable = false;
        let (event, _) = fit_checkpoint(channel_id, payload)?;
        let event = event.sign_with_keys(&self.config.keys)?;
        let id = event.id.to_hex();
        let key = coding_session_checkpoint_semantic_key(
            &target,
            CodingSessionCheckpointReason::PreRewind,
            through_seq,
        );
        // NIP-CSCK § `pre_rewind`: a second capture at the same `throughSeq`
        // would share the first's `csck-key` and be dropped by every reader,
        // so it is not published; the first is the undo point that matters.
        if self
            .outbox
            .accepted_content(KIND_CODING_SESSION_CHECKPOINT, &key)
            .is_some()
        {
            anyhow::bail!(
                "a pre_rewind checkpoint at this point of the record was already published"
            );
        }
        let queued =
            self.outbox
                .enqueue(KIND_CODING_SESSION_CHECKPOINT, &key, Priority::High, event)?;
        if !queued {
            anyhow::bail!("the pre_rewind checkpoint was not queued");
        }
        Ok(id)
    }
}

fn not_measured() -> CaptureFailure {
    CaptureFailure {
        code: UnavailableCode::GitFailed,
        sentence: "The turn's end was not captured.".to_owned(),
        already_pinned: None,
    }
}

/// Capture the turn's end and list its files from the baseline. Off the loop.
async fn measure_end(
    cwd: &Path,
    scope: Option<&HostLaunchPlan>,
    capture: &TurnCheckpointCapture,
) -> Measured {
    let target = &capture.target;
    let captured = capture_tree(
        cwd,
        scope,
        &target.session_id,
        target.generation,
        RefLeaf::Through(capture.through_seq),
        None,
        CapturePhase::TurnEnd,
    )
    .await;
    let tree = match captured {
        Ok(tree) => tree,
        Err(CaptureFailure {
            already_pinned: Some(pinned),
            ..
        }) => match pinned_context(cwd, &pinned.commit).await {
            Ok((head, branch)) => CapturedTree {
                tree: pinned.tree,
                commit: pinned.commit,
                head,
                branch,
                // The pinned capture's omissions are not recorded anywhere
                // this can read, so it does not claim to be complete.
                complete: false,
                omitted: Vec::new(),
                omitted_not_listed: 0,
                boundary_enforced: matches!(scope, Some(HostLaunchPlan::Bounded(_))),
            },
            Err(failure) => return Measured::Failed(failure),
        },
        Err(failure) => return Measured::Failed(failure),
    };
    let Some(base_tree) = capture.base_tree.as_deref() else {
        return Measured::Captured { tree, files: None };
    };
    match diff_tree_files(cwd, scope, base_tree, &tree.tree).await {
        Ok(files) => Measured::Captured {
            tree,
            files: Some(files),
        },
        Err(failure) => Measured::Failed(failure),
    }
}

/// `HEAD` and branch for a checkpoint that was already pinned: its commit's
/// parent (what `HEAD` was when it was written) and the current branch.
async fn pinned_context(
    cwd: &Path,
    commit: &str,
) -> Result<(Option<String>, Option<String>), CaptureFailure> {
    let failed = || CaptureFailure {
        code: UnavailableCode::GitFailed,
        sentence: "The checkpoint already pinned here could not be read back.".to_owned(),
        already_pinned: None,
    };
    let run = |args: Vec<String>| {
        let mut command =
            tokio::process::Command::from(crate::host_command::metadata_git_command(cwd));
        command.args(args).kill_on_drop(true);
        async move { tokio::time::timeout(DIFF_TIMEOUT, command.output()).await }
    };
    let parent = run(vec![
        "rev-parse".into(),
        "--verify".into(),
        "--quiet".into(),
        format!("{commit}^1"),
    ])
    .await
    .map_err(|_| failed())?
    .map_err(|_| failed())?;
    let head = match parent.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&parent.stdout).trim().to_owned()),
        Some(1) if parent.stdout.is_empty() => None,
        _ => return Err(failed()),
    };
    let branch = run(vec![
        "symbolic-ref".into(),
        "--short".into(),
        "--quiet".into(),
        "HEAD".into(),
    ])
    .await
    .map_err(|_| failed())?
    .map_err(|_| failed())?;
    let branch = match branch.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&branch.stdout).trim().to_owned()),
        Some(1) => None,
        _ => return Err(failed()),
    };
    Ok((head, branch))
}

/// The wire payload for a capture (NIP-CSCK § Rules). `previous_tree` is the
/// tree of the generation's previous checkpoint, when it published one.
fn checkpoint_payload(
    capture: &TurnCheckpointCapture,
    previous_tree: Option<&str>,
) -> CodingSessionCheckpointPayload {
    let mut payload = CodingSessionCheckpointPayload {
        schema: CODING_SESSION_CHECKPOINT_SCHEMA.to_owned(),
        session: capture.target.clone(),
        turn_id: Some(capture.turn_id.clone()),
        reason: CodingSessionCheckpointReason::Turn,
        coverage: CodingSessionCheckpointCoverage {
            from_seq: capture.from_seq,
            through_seq: capture.through_seq,
        },
        git: None,
        files: Vec::new(),
        files_not_listed: 0,
        // SV-29 / ledger 371: a turn can be rewound to by this build whatever
        // was measured; restoring its files is the rewind's own check on
        // `git.baseTree`. A `pre_rewind` caller clears this.
        restorable: BUILD_IMPLEMENTS_REWIND,
        unavailable: None,
        summary: None,
    };
    match &capture.measured {
        Measured::Failed(failure) => {
            payload.unavailable = Some(CodingSessionCheckpointUnavailable {
                code: wire_code(failure.code),
                sentence: failure.sentence.clone(),
            });
        }
        Measured::Captured { tree, files } => {
            let base_tree = capture.base_tree.clone();
            let outside_turn = match (previous_tree, base_tree.as_deref()) {
                (Some(previous), Some(base)) => Some(previous != base),
                _ => None,
            };
            let mut omitted = Vec::new();
            let mut omitted_not_listed = tree.omitted_not_listed;
            for path in &tree.omitted {
                match wire_omission(path).filter(|_| omitted.len() < MAX_CHECKPOINT_OMITTED) {
                    Some(omission) => omitted.push(omission),
                    None => omitted_not_listed = omitted_not_listed.saturating_add(1),
                }
            }
            // An overlapped capture is not a measurement of this turn alone
            // (the next turn's edits may be in it): never `complete`.
            let complete = tree.complete
                && omitted.is_empty()
                && omitted_not_listed == 0
                && !capture.overlapped.load(Ordering::SeqCst);
            // No baseline, no range: `files` stays empty and the reader says
            // the baseline was not captured, never "0 files".
            if let (Some(diff), Some(_)) = (files, base_tree.as_ref()) {
                payload.files_not_listed = diff.not_listed;
                for file in &diff.files {
                    match wire_file(file).filter(|_| payload.files.len() < MAX_CHECKPOINT_FILES) {
                        Some(file) => payload.files.push(file),
                        None => {
                            payload.files_not_listed = payload.files_not_listed.saturating_add(1)
                        }
                    }
                }
            }
            payload.git = Some(CodingSessionCheckpointGit {
                head: tree.head.clone(),
                branch: tree.branch.clone(),
                base_tree,
                tree: tree.tree.clone(),
                commit: tree.commit.clone(),
                outside_turn,
                complete,
                omitted,
                omitted_not_listed,
            });
        }
    }
    payload
}

/// Build `payload`, moving listed files (then omissions) into the
/// not-listed counts until the content fits the kind's 32 KiB. Returns the
/// unsigned event and the tree it publishes, if any.
fn fit_checkpoint(
    channel_id: Uuid,
    mut payload: CodingSessionCheckpointPayload,
) -> anyhow::Result<(nostr::EventBuilder, Option<String>)> {
    loop {
        let size = serde_json::to_string(&payload)?.len();
        if size <= MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES {
            break;
        }
        if !payload.files.is_empty() {
            let keep = payload.files.len() / 2;
            let dropped = (payload.files.len() - keep) as u64;
            payload.files.truncate(keep);
            payload.files_not_listed = payload.files_not_listed.saturating_add(dropped);
        } else if let Some(git) = payload.git.as_mut().filter(|git| !git.omitted.is_empty()) {
            let keep = git.omitted.len() / 2;
            let dropped = (git.omitted.len() - keep) as u64;
            git.omitted.truncate(keep);
            git.omitted_not_listed = git.omitted_not_listed.saturating_add(dropped);
            git.complete = false;
        } else {
            anyhow::bail!("checkpoint content is {size} bytes with nothing left to move");
        }
    }
    let tree = payload.git.as_ref().map(|git| git.tree.clone());
    Ok((build_coding_session_checkpoint(channel_id, &payload)?, tree))
}

fn wire_code(code: UnavailableCode) -> CodingSessionCheckpointUnavailableCode {
    match code {
        UnavailableCode::NotARepository => CodingSessionCheckpointUnavailableCode::NotARepository,
        UnavailableCode::BoundaryUnprepared => {
            CodingSessionCheckpointUnavailableCode::BoundaryUnprepared
        }
        UnavailableCode::TimedOut => CodingSessionCheckpointUnavailableCode::TimedOut,
        UnavailableCode::GitFailed => CodingSessionCheckpointUnavailableCode::GitFailed,
    }
}

/// One changed file as the wire names it, or `None` when its path (or a
/// rename's old path) is one NIP-CSCK refuses: it is then counted instead.
fn wire_file(file: &ChangedFile) -> Option<CodingSessionCheckpointFile> {
    let publishable = is_publishable_checkpoint_path(&file.path)
        && file
            .from
            .as_deref()
            .is_none_or(is_publishable_checkpoint_path);
    let status = wire_status(file.status)?;
    publishable.then(|| CodingSessionCheckpointFile {
        path: file.path.clone(),
        status,
        from: file.from.clone(),
        additions: file.additions,
        deletions: file.deletions,
    })
}

/// A change's wire token, parsed by the wire's own type.
fn wire_status(status: FileChange) -> Option<CodingSessionCheckpointFileStatus> {
    serde_json::from_value(serde_json::Value::from(status.as_str())).ok()
}

/// One omitted path as the wire names it, or `None` when it is refused.
fn wire_omission(path: &OmittedPath) -> Option<CodingSessionCheckpointOmission> {
    let reason: CodingSessionCheckpointOmissionReason =
        serde_json::from_value(serde_json::Value::from(path.reason.as_str())).ok()?;
    is_publishable_checkpoint_path(&path.path).then(|| CodingSessionCheckpointOmission {
        path: path.path.clone(),
        reason,
    })
}

#[cfg(test)]
#[path = "turn_checkpoint_tests.rs"]
mod tests;
