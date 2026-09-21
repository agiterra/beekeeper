//! Who may move a seat's checkout, and when.
//!
//! # The defect this closes
//!
//! Lane 202 established a seat's input in `apply_turn_decision`, before the
//! turn reached the actor's mailbox, and nothing there waited for the seat's
//! previous turn to end (Astra's Wave 2 review, finding 2). A verifier testing
//! A in a clean tree could have that tree changed under it by preparation for
//! B; and a later assignment C could move it again, so B — which had passed
//! its check — would start on C's tree.
//!
//! Two facts close it, and they are the same fact in two places:
//!
//! * **`git checkout -B` runs only while the seat is idle.** An establishment
//!   takes the seat's custody token before it runs git, and a turn holds that
//!   same token from the moment the actor dequeues it until the turn ends
//!   (`crates/buzz-session-provider/src/session.rs`, the dequeue boundary). A
//!   turn in flight therefore blocks every checkout for that seat, and an
//!   establishment in flight delays the next turn rather than racing it.
//! * **The tree is verified where the prompt is sent, not where the turn is
//!   decided.** The provider records what a turn requires
//!   ([`remember_requirement`]); the actor re-reads `HEAD` under custody
//!   before prompting ([`verify_for_turn`]). Anything that moved in between is
//!   caught at the only boundary where catching it still means something.
//!
//! # Live attempts
//!
//! An attempt carries an identity (`owner`), written with its started-attempt
//! record. A waiter that gives up after its bound does **not** make that
//! record replayable: [`establish_for_turn`] refuses to start a second attempt
//! while the first is live here, terminal writes compare the owner so a
//! detached task can never overwrite a newer outcome, and a process that
//! restarts knows no owner at all — which is exactly what makes every
//! `establishing` record it finds the interrupted case the one-replay rule
//! exists for (finding 3).
//!
//! # Why a registry rather than a field
//!
//! The two parties that need this are the provider's decision path and the
//! session actor's dequeue, and they are joined by nothing else: the actor is
//! constructed long before an assignment exists and takes its commands through
//! an `mpsc` whose message shape is shared with every other turn. Keying a
//! process-wide registry by session id and command id adds no plumbing to
//! either, and a provider is one process per identity, so there is nothing for
//! the keys to collide with.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::future::{FutureExt, Shared};
use tokio::sync::oneshot;

use crate::assignment_inputs::{
    assignment_input, commit_terminal_record, establish_recorded_assignment, AssignmentInputRecord,
    AssignmentInputStore, EstablishmentError, SeatCheckout,
};

/// Custody of one seat's checkout, held by whichever party may move or read
/// it: an establishment, or a turn.
pub type CustodyGuard = tokio::sync::OwnedMutexGuard<()>;

/// What one turn needs to be true of its seat's tree before its prompt is
/// sent.
#[derive(Debug, Clone)]
pub struct TurnRequirement {
    /// The canonical assignment the turn points at.
    pub assignment_ref: String,
    /// The commit that assignment names, lowercase hex.
    pub base_sha: String,
    /// The seat's working directory, as the provider resolved it.
    pub cwd: PathBuf,
    /// The shared host store the record lives in.
    pub store: AssignmentInputStore,
    /// The seat's branch, read from the tree when the requirement was made.
    pub branch: String,
}

/// The outcome of asking for one seat's input to be established.
#[derive(Debug, Clone)]
pub enum Establishment {
    /// The record reached a terminal outcome; read it.
    Settled(Box<AssignmentInputRecord>),
    /// The seat has a turn in flight, so nothing may move its tree. An
    /// attempt is queued behind that turn.
    SeatBusy,
    /// An attempt is running and did not finish inside the caller's bound.
    InFlight(String),
    /// Nothing ran and nothing will until this is fixed.
    Unstarted(String),
}

/// One attempt, and the handle every waiter shares.
type AttemptFuture = Shared<oneshot::Receiver<Arc<Establishment>>>;

#[derive(Default)]
struct Registry {
    seats: HashMap<String, Arc<tokio::sync::Mutex<()>>>,
    requirements: HashMap<(String, String), TurnRequirement>,
    /// Attempts started by this process and not yet finished, by owner.
    live: HashMap<String, String>,
    /// One attempt per seat, joined rather than duplicated.
    attempts: HashMap<String, (String, AttemptFuture)>,
    /// Terminal records whose write failed, kept so the next pass can write
    /// them instead of doing the git work again (finding 12).
    unrecorded: HashMap<String, (String, AssignmentInputRecord)>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

/// The lock that serializes everything allowed to touch one seat's tree.
fn seat_lock(session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut registry = match registry().lock() {
        Ok(registry) => registry,
        Err(poisoned) => poisoned.into_inner(),
    };
    Arc::clone(
        registry
            .seats
            .entry(session_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

/// Take custody of a seat's checkout, waiting for whoever holds it.
///
/// Held by a turn from the actor's dequeue until the turn ends, which is what
/// stops a checkout running underneath a running verifier.
pub async fn hold(session_id: &str) -> CustodyGuard {
    seat_lock(session_id).lock_owned().await
}

/// Whether some party is holding this seat's checkout right now.
#[must_use]
pub fn is_busy(session_id: &str) -> bool {
    seat_lock(session_id).try_lock_owned().is_err()
}

/// Record what a turn will need when the actor dequeues it.
pub fn remember_requirement(session_id: &str, command_id: &str, requirement: TurnRequirement) {
    let mut registry = match registry().lock() {
        Ok(registry) => registry,
        Err(poisoned) => poisoned.into_inner(),
    };
    registry
        .requirements
        .insert((session_id.to_owned(), command_id.to_owned()), requirement);
}

/// Take back what a turn needs, if this provider recorded anything.
#[must_use]
pub fn take_requirement(session_id: &str, command_id: &str) -> Option<TurnRequirement> {
    let mut registry = match registry().lock() {
        Ok(registry) => registry,
        Err(poisoned) => poisoned.into_inner(),
    };
    registry
        .requirements
        .remove(&(session_id.to_owned(), command_id.to_owned()))
}

/// Forget a requirement without consuming it, for a command that was answered
/// some other way.
pub fn forget_requirement(session_id: &str, command_id: &str) {
    let _ = take_requirement(session_id, command_id);
}

/// Whether `owner` names an attempt this process still has running.
#[must_use]
pub fn attempt_is_live(owner: &str) -> bool {
    let registry = match registry().lock() {
        Ok(registry) => registry,
        Err(poisoned) => poisoned.into_inner(),
    };
    registry.live.contains_key(owner)
}

/// The identity of one attempt: this process's run, and this attempt within
/// it.
fn mint_owner() -> String {
    static PROCESS: OnceLock<String> = OnceLock::new();
    let process = PROCESS.get_or_init(|| uuid::Uuid::new_v4().to_string());
    format!("{process}:{}", uuid::Uuid::new_v4())
}

/// Establish one assignment's input, or say what is in the way.
///
/// Never starts a second attempt for a seat that already has one, never runs
/// git while the seat has a turn in flight, and never waits longer than
/// `bound` — past which the attempt carries on and the caller is told
/// [`Establishment::InFlight`], because a slow fetch is not an interrupted
/// one.
pub async fn establish_for_turn(
    session_id: &str,
    requirement: &TurnRequirement,
    bound: Duration,
) -> Establishment {
    // A terminal record whose write failed is owed to the disk before
    // anything else: writing it is not the git work, and doing the git work
    // again would be.
    if let Some(settled) = retry_unrecorded(&requirement.store, &requirement.assignment_ref).await {
        return settled;
    }

    let existing = {
        let registry = match registry().lock() {
            Ok(registry) => registry,
            Err(poisoned) => poisoned.into_inner(),
        };
        registry
            .attempts
            .get(session_id)
            .filter(|(assignment, _)| assignment == &requirement.assignment_ref)
            .map(|(_, future)| future.clone())
    };
    if let Some(existing) = existing {
        return join_attempt(existing, bound).await;
    }

    if is_busy(session_id) {
        // The seat has a turn in flight. Nothing is spawned: an attempt
        // queued behind that turn would take the tree the moment the turn
        // released it, which is how a later assignment moved the tree out
        // from under an earlier one that had already passed its check
        // (review finding 2). The wake waits durably and is prepared again
        // when the seat is idle, oldest first.
        return Establishment::SeatBusy;
    }
    let owner = mint_owner();
    let (sender, receiver) = oneshot::channel();
    let shared: AttemptFuture = receiver.shared();
    {
        let mut registry = match registry().lock() {
            Ok(registry) => registry,
            Err(poisoned) => poisoned.into_inner(),
        };
        registry
            .live
            .insert(owner.clone(), requirement.assignment_ref.clone());
        registry.attempts.insert(
            session_id.to_owned(),
            (requirement.assignment_ref.clone(), shared.clone()),
        );
    }

    let task_session = session_id.to_owned();
    let task_owner = owner.clone();
    let store = requirement.store.clone();
    let assignment_ref = requirement.assignment_ref.clone();
    let checkout = SeatCheckout {
        path: requirement.cwd.clone(),
        branch: requirement.branch.clone(),
    };
    tokio::spawn(async move {
        // Custody first, always: the tree does not move while a turn is using
        // it, however long that turn takes.
        let guard = hold(&task_session).await;
        let attempt_owner = task_owner.clone();
        let attempt_store = store.clone();
        let attempt_id = assignment_ref.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            establish_recorded_assignment(
                &attempt_store,
                &attempt_id,
                &checkout,
                &attempt_owner,
                &attempt_is_live,
            )
        })
        .await;
        drop(guard);

        let answer = match outcome {
            Ok(Ok(record)) => Establishment::Settled(Box::new(record)),
            Ok(Err(EstablishmentError::Unrecorded { record, error })) => {
                tracing::error!(
                    target: "csp::assignment_custody",
                    assignment_ref = %assignment_ref,
                    %error,
                    "the tree was established and its record could not be saved; it will be \
                     written on the next pass rather than established again"
                );
                remember_unrecorded(&assignment_ref, &task_owner, *record);
                Establishment::Unstarted(format!("the record could not be saved: {error}"))
            }
            Ok(Err(error)) => Establishment::Unstarted(error.to_string()),
            Err(error) => Establishment::Unstarted(format!("the attempt did not finish: {error}")),
        };
        {
            let mut registry = match registry().lock() {
                Ok(registry) => registry,
                Err(poisoned) => poisoned.into_inner(),
            };
            registry.live.remove(&task_owner);
            if registry
                .attempts
                .get(&task_session)
                .is_some_and(|(assignment, _)| assignment == &assignment_ref)
            {
                registry.attempts.remove(&task_session);
            }
        }
        let _ = sender.send(Arc::new(answer));
    });

    join_attempt(shared, bound).await
}

/// Wait for an attempt, up to `bound`.
async fn join_attempt(attempt: AttemptFuture, bound: Duration) -> Establishment {
    match tokio::time::timeout(bound, attempt).await {
        Ok(Ok(outcome)) => (*outcome).clone(),
        Ok(Err(_)) => Establishment::Unstarted("the attempt ended without an answer".to_owned()),
        Err(_) => Establishment::InFlight("bound_exceeded".to_owned()),
    }
}

/// Whether any attempt is still running for this seat.
#[must_use]
pub fn attempt_running_for(session_id: &str) -> Option<String> {
    let registry = match registry().lock() {
        Ok(registry) => registry,
        Err(poisoned) => poisoned.into_inner(),
    };
    registry
        .attempts
        .get(session_id)
        .map(|(assignment, _)| assignment.clone())
}

fn remember_unrecorded(assignment_ref: &str, owner: &str, record: AssignmentInputRecord) {
    let mut registry = match registry().lock() {
        Ok(registry) => registry,
        Err(poisoned) => poisoned.into_inner(),
    };
    registry
        .unrecorded
        .insert(assignment_ref.to_owned(), (owner.to_owned(), record));
}

/// Write a terminal record whose earlier write failed, without redoing the
/// checkout it describes.
async fn retry_unrecorded(
    store: &AssignmentInputStore,
    assignment_ref: &str,
) -> Option<Establishment> {
    let owed = {
        let registry = match registry().lock() {
            Ok(registry) => registry,
            Err(poisoned) => poisoned.into_inner(),
        };
        registry.unrecorded.get(assignment_ref).cloned()
    };
    let (owner, record) = owed?;
    let store = store.clone();
    let written =
        tokio::task::spawn_blocking(move || commit_terminal_record(&store, &owner, record)).await;
    match written {
        Ok(Ok(record)) => {
            let mut registry = match registry().lock() {
                Ok(registry) => registry,
                Err(poisoned) => poisoned.into_inner(),
            };
            registry.unrecorded.remove(assignment_ref);
            Some(Establishment::Settled(Box::new(record)))
        }
        Ok(Err(error)) => Some(Establishment::Unstarted(format!(
            "an established tree's record is still unsaved: {error}"
        ))),
        Err(error) => Some(Establishment::Unstarted(format!(
            "an established tree's record could not be retried: {error}"
        ))),
    }
}

/// What the actor found when it checked its seat's tree at the last moment it
/// could — and, in every case but a refusal, the custody it now holds.
///
/// There is deliberately no "not required" variant that hands back nothing.
/// Custody belongs to the **turn**, not to the assignment (Astra's Wave 2
/// re-check, R2): an ordinary follow-up prompt to a verifier examining commit
/// A is using that tree just as much as an assignment turn is, and before
/// this the unguarded path let preparation for B move the tree underneath it.
#[derive(Debug)]
pub enum TurnCustody {
    /// Custody of the seat, with nothing to verify: this turn carries no
    /// assignment requirement. The guard is still held, because the turn is
    /// still using the tree.
    Unmanaged(Box<CustodyGuard>),
    /// Custody of the seat, and the tree holds the commit the assignment
    /// names. Hold this until the turn ends.
    Held(Box<CustodyGuard>),
    /// The tree is not what the assignment named. The prompt must not be
    /// sent, and the delivery must be discharged rather than left accepted
    /// (R2's second hole).
    Refused {
        /// The assignment the turn pointed at.
        assignment_ref: String,
        /// What the tree actually holds, when it could be read.
        observed: Option<String>,
        /// The commit that was required.
        required: String,
    },
}

impl TurnCustody {
    /// The guard to keep for the length of the turn, when there is one.
    #[must_use]
    pub fn into_guard(self) -> Option<Box<CustodyGuard>> {
        match self {
            Self::Unmanaged(guard) | Self::Held(guard) => Some(guard),
            Self::Refused { .. } => None,
        }
    }
}

/// Take custody of the seat and re-verify its tree, at the dequeue boundary.
///
/// The provider decided this turn against a tree it read earlier; this reads
/// the tree again at the only moment that matters — after the previous turn
/// has ended, before this prompt is sent — and holds custody for as long as
/// the caller keeps the guard, so nothing can move it while the turn runs.
pub async fn verify_for_turn(session_id: &str, command_id: &str) -> TurnCustody {
    // The lock comes first, unconditionally. Deciding whether to take custody
    // by asking whether this particular prompt names an assignment was R2:
    // the tree is the seat's, and whichever turn is running is using it.
    let guard = hold(session_id).await;
    let Some(requirement) = take_requirement(session_id, command_id) else {
        return TurnCustody::Unmanaged(Box::new(guard));
    };
    // Read the row *now*, under custody, so the observation this boundary
    // writes can be conditioned on the exact attempt it saw (R3).
    let store = requirement.store.clone();
    let assignment_ref = requirement.assignment_ref.clone();
    let observed_record = {
        let store = store.clone();
        let assignment_ref = assignment_ref.clone();
        tokio::task::spawn_blocking(move || {
            store.with_records(|records, _| assignment_input(records, &assignment_ref).cloned())
        })
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
    };
    let observed = crate::git_probe::probe_verification_input(&requirement.cwd)
        .await
        .ok();
    let head = observed.as_ref().and_then(|tree| tree.head.clone());
    let dirty = observed.as_ref().and_then(|tree| tree.dirty_lines);
    let established = head
        .as_deref()
        .is_some_and(|head| head.eq_ignore_ascii_case(&requirement.base_sha))
        && dirty == Some(0);
    if established {
        // The record is the provider's, but the fact is this boundary's: say
        // what was true where the prompt is sent.
        tracing::info!(
            target: "csp::assignment_custody",
            %session_id,
            %command_id,
            assignment_ref = %requirement.assignment_ref,
            commit = %requirement.base_sha,
            code = "turn_input_verified_at_dequeue",
            "the seat's tree holds its assignment's commit; custody is held for this turn"
        );
        return TurnCustody::Held(Box::new(guard));
    }

    // The record keeps the reason, so the party that publishes blockers can
    // find it without this boundary having to reach the relay. Written while
    // custody is still held, and **only** if the row is still the one this
    // boundary observed: between a dropped guard and this write, an operator
    // requeue could have minted a new attempt, and overwriting its
    // `attemptOwner` made that attempt's own terminal write fail its
    // ownership comparison (R3).
    let observed_head = head.clone();
    let required = requirement.base_sha.clone();
    let recorded = tokio::task::spawn_blocking(move || {
        store.with_records(|records, persist| {
            let Some(current) = assignment_input(records, &assignment_ref).cloned() else {
                return Ok(false);
            };
            if Some(&current) != observed_record.as_ref() {
                // The row changed under this boundary. Its observation is
                // already stale, and writing it would speak over whatever
                // replaced it.
                return Ok(false);
            }
            if current.attempt_owner.is_some() {
                // An attempt owns this row. This boundary is not an attempt:
                // it observed a tree, it did not try to move one, and the
                // owner's own terminal write is the answer for this row. R3's
                // interleaving is exactly this case — a requeue mints a new
                // attempt between the mismatch and this write, and clearing
                // or overwriting its row made that attempt's terminal write
                // fail its ownership comparison.
                return Ok(false);
            }
            let mut moved = current;
            moved.outcome = crate::assignment_inputs::ASSIGNMENT_INPUT_MOVED.to_owned();
            moved.message = Some(match observed_head.as_deref() {
                Some(head) => format!(
                    "the seat's tree held {head} when its turn was about to start, and the \
                     assignment names {required}"
                ),
                None => format!(
                    "the seat's tree could not be read when its turn was about to start, so \
                     whether it holds {required} is unknown"
                ),
            });
            // Untouched deliberately: this row's `attemptOwner` was `None` in
            // the snapshot this write is conditioned on, so there is nothing
            // to clear and no newer owner to lose.
            crate::assignment_inputs::record_assignment_input(records, moved);
            persist(records).map(|()| true)
        })
    })
    .await;
    match recorded {
        Ok(Ok(Ok(true))) => {}
        Ok(Ok(Ok(false))) => tracing::info!(
            target: "csp::assignment_custody",
            %command_id,
            assignment_ref = %requirement.assignment_ref,
            "leaving a newer attempt's record alone; this boundary's observation is stale"
        ),
        Ok(Ok(Err(error))) | Ok(Err(error)) => tracing::warn!(
            target: "csp::assignment_custody",
            %error,
            "the moved-tree observation could not be saved"
        ),
        Err(error) => tracing::warn!(
            target: "csp::assignment_custody",
            %error,
            "the moved-tree observation panicked"
        ),
    }
    drop(guard);
    TurnCustody::Refused {
        assignment_ref: requirement.assignment_ref,
        observed: head,
        required: requirement.base_sha,
    }
}

#[cfg(test)]
#[path = "assignment_custody_tests.rs"]
mod tests;
