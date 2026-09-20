//! The durable, host-owned sequence that puts a seat on its exact input.
//!
//! # The defect this closes
//!
//! Establishing a verifier's or a runner's input — checking out the commit its
//! assignment names, in that seat's own worktree — used to run from a React
//! effect in the mission surface
//! (`desktop/src/features/coding-sessions/hooks/useCodingSessionAssignmentInputs.ts`).
//! That made a *panel* the thing which decided whether work became executable:
//! it ran only while Mission was mounted, stopped when the panel was closed,
//! and kept its "already attempted" set in a React ref, so a relaunch knew
//! nothing. In the 2026-09-20 kettle run the verifier's tree stayed on the
//! README commit and the seat ended up verifying a scratch archive by hand
//! (ledger 178(d)).
//!
//! The work is now queued **durably** the first time this host sees the
//! assignment, and the queue is drained by the host: on the observation that
//! queued it, and again at launch for anything a quit or a crash left pending.
//! Closing the panel no longer stops an establishment, and no longer loses one.
//!
//! # What still needs a surface, said plainly
//!
//! This host has **no relay subscription of its own** for kind 44244: every
//! team transaction reaches `desktop/src-tauri` as an argument to
//! `fold_coding_session_team_transactions`, handed in by the frontend that
//! holds the subscription. So the *first sighting* of an assignment still
//! comes from a surface. That is not only a limitation, it is also the safe
//! boundary: an assignment is only worth acting on once the governed fold has
//! admitted it under a session's authority context, and a background poller
//! that decoded 44244 without that context could move a seat's tree on an
//! excluded — possibly forged — assignment. What this module owns is
//! everything after the sighting, which is where the panel used to be
//! load-bearing.
//!
//! # Ordering against the wake
//!
//! Nothing here can be ordered *before* the assignee's wake: the only path
//! that wakes an assignee seat is the lead's CLI
//! (`crates/buzz-cli/src/commands/sessions/crew_cmds.rs`,
//! `send_team_operation_wake`). The order is made irrelevant instead of
//! claimed: the provider refuses a verifier or runner turn whose tree does not
//! hold the assignment's `baseSha`
//! (`crates/buzz-session-provider/src/verification_input.rs`), and it decides
//! by reading `HEAD` and `git status --porcelain` in the seat's own working
//! directory — never by being told. So the fence needs **no new signal** from
//! this module and no new event kind: a commit this host has established is a
//! fact the provider reads for itself, and the established commit already
//! reaches the wire through the provider's existing observed-commit refresh.
//!
//! # Retry discipline
//!
//! A terminal outcome is never retried automatically. A refusal is a fact
//! about this computer now — a dirty tree, a commit no remote has — and
//! re-running it on a timer would turn one honest sentence into a loop. Only
//! [`coding_session_requeue_assignment_input`], a person's own click, puts a
//! settled assignment back in the queue.
//!
//! An *interrupted* attempt is the one case that replays, and it is bounded:
//! [`CodingSessionAssignmentInputRecord::attempts`] is incremented and
//! persisted **before** the git work, so a launch that finds a started attempt
//! knows it was started. The second such find records
//! [`ASSIGNMENT_INPUT_ABANDONED`] and stops, because an attempt that takes the
//! app down with it twice will take it down a third time.

use buzz_core_pkg::coding_session_team_transaction::role_requires_verification_input;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::coding_sessions::assignment_input::{
    is_object_id, CodingSessionAssignmentInputRecord, EstablishAssignmentInputRequest,
};
use crate::coding_sessions::workdir_store::{
    load_workdir_store, lock_workdir_store, save_workdir_store, seat_worktree_key,
    CodingSessionWorkdirStore, WORKDIR_STORE_VERSION,
};
use crate::util::now_iso;

/// Queued by an observation, with no attempt started yet.
pub(crate) const ASSIGNMENT_INPUT_INTENDED: &str = "intended";

/// An attempt has been started; this is written before git runs.
pub(crate) const ASSIGNMENT_INPUT_ESTABLISHING: &str = "establishing";

/// Two attempts were started and neither finished; the host stopped trying.
pub(crate) const ASSIGNMENT_INPUT_ABANDONED: &str = "establish_abandoned";

/// How many attempts may be *started* for one assignment before the host stops
/// replaying it at launch.
///
/// Two, not one: the ordinary interruption is a person quitting the app
/// mid-checkout, and refusing to finish that would be worse than finishing it.
/// Three would be a loop with extra steps.
pub(crate) const MAX_ESTABLISH_ATTEMPTS: u32 = 2;

/// True for an outcome the host still owes an answer for.
#[must_use]
pub(crate) fn assignment_input_is_pending(outcome: &str) -> bool {
    outcome == ASSIGNMENT_INPUT_INTENDED || outcome == ASSIGNMENT_INPUT_ESTABLISHING
}

/// One folded assignment, as a surface observed it.
///
/// Deliberately not the signed event: the caller has already re-verified and
/// folded it (`fold_coding_session_team_transactions`), and this host acts on
/// the assignment's *fields*. `seatLabel` is the name this host cut the tree
/// under, which only the caller can resolve from the actor.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedCodingSessionAssignment {
    /// The signed assignment event id, 64 lowercase hex.
    pub assignment_id: String,
    /// The umbrella session the seat belongs to.
    pub session_ref: String,
    /// The seat label this host recorded the worktree under, when the caller
    /// could resolve one.
    #[serde(default)]
    pub seat_label: Option<String>,
    /// The signed `assigneeRole`, exactly as it is on the wire.
    #[serde(default)]
    pub assignee_role: Option<String>,
    /// The signed `baseSha`, or `None` when the assignment named no commit.
    #[serde(default)]
    pub base_sha: Option<String>,
    /// The signed branch, when the assignment carried one.
    #[serde(default)]
    pub branch: Option<String>,
}

/// What this host has to say about one observed assignment's input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingSessionAssignmentInputDisposition {
    /// The role does not start from a named revision — a builder is told what
    /// to change, not asked a question about a commit.
    NotRequired,
    /// An input-bound role whose assignment names no commit. Nothing was
    /// queued and nothing could be; inventing a commit would be a guess.
    Unnamed,
    /// The observation could not be read as one: an id or a commit that is not
    /// an object id. Said out loud rather than dropped.
    Invalid,
    /// This host cut no worktree for that seat, so it has no tree to move.
    /// The ordinary case for anyone watching a mission they did not hire.
    OffHost,
    /// This host has a durable record for the assignment; read it.
    Recorded,
}

/// One row: what the host decided, and its durable record when it has one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionAssignmentInputStatus {
    /// The assignment this row is about.
    pub assignment_id: String,
    /// Why there is, or is not, a record.
    pub disposition: CodingSessionAssignmentInputDisposition,
    /// The host's own record of the attempt, when one exists.
    pub record: Option<CodingSessionAssignmentInputRecord>,
}

/// Queue every observed assignment this host owes an input for.
///
/// Returns one disposition per observation, in the order they were given. A
/// record that already names the **same** commit is dispositive and is left
/// exactly as it is — including a refusal, which is why nothing loops. A
/// record for a *different* commit is no answer about this one, so it is
/// replaced by a fresh intent with the attempt count back at zero.
pub(crate) fn queue_observed_assignments(
    store: &mut CodingSessionWorkdirStore,
    observed: &[ObservedCodingSessionAssignment],
) -> Vec<CodingSessionAssignmentInputDisposition> {
    observed
        .iter()
        .map(|assignment| queue_one(store, assignment))
        .collect()
}

fn queue_one(
    store: &mut CodingSessionWorkdirStore,
    assignment: &ObservedCodingSessionAssignment,
) -> CodingSessionAssignmentInputDisposition {
    use CodingSessionAssignmentInputDisposition as Disposition;

    let role = assignment.assignee_role.as_deref().unwrap_or("").trim();
    if !role_requires_verification_input(role) {
        return Disposition::NotRequired;
    }
    let assignment_id = assignment.assignment_id.trim().to_string();
    if assignment_id.len() != 64 || !is_object_id(&assignment_id) {
        return Disposition::Invalid;
    }
    let commit = assignment
        .base_sha
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if commit.is_empty() {
        return Disposition::Unnamed;
    }
    if !is_object_id(&commit) {
        return Disposition::Invalid;
    }
    let session_ref = assignment.session_ref.trim().to_string();
    let seat_label = assignment
        .seat_label
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if session_ref.is_empty() || seat_label.is_empty() {
        return Disposition::OffHost;
    }
    // The tree this host recorded cutting, or nothing. No search of the disk:
    // adopting a tree this host did not cut is how a seat's uncommitted work
    // would get moved out from under somebody.
    if !store
        .worktrees
        .contains_key(&seat_worktree_key(&session_ref, &seat_label))
    {
        return Disposition::OffHost;
    }
    let already = store
        .assignment_input(&assignment_id)
        .is_some_and(|record| record.commit.as_deref() == Some(commit.as_str()));
    if already {
        return Disposition::Recorded;
    }
    let branch = assignment
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_string);
    store.record_assignment_input(CodingSessionAssignmentInputRecord {
        assignment_id,
        session_ref: Some(session_ref),
        seat_label: Some(seat_label),
        commit: Some(commit),
        branch,
        path: None,
        remote: None,
        outcome: ASSIGNMENT_INPUT_INTENDED.to_string(),
        message: None,
        changes: None,
        attempts: 0,
        recorded_at: now_iso(),
    });
    Disposition::Recorded
}

/// Every assignment this host still owes an input for, in key order.
///
/// Key order, not assignment order: the queue is drained one at a time and
/// each item is independent, so the only property that matters is that all of
/// them are reached. The signed order is not recoverable from the store and
/// pretending otherwise would be a fiction.
#[must_use]
pub(crate) fn pending_assignment_inputs(store: &CodingSessionWorkdirStore) -> Vec<String> {
    store
        .assignment_inputs
        .iter()
        .filter(|(_, record)| assignment_input_is_pending(&record.outcome))
        .map(|(key, _)| key.clone())
        .collect()
}

/// Drain the queue, one assignment at a time, persisting as it goes.
///
/// `persist` is called after the attempt count is written and again after each
/// terminal outcome, because the count is only a bound if it reaches the disk
/// before the git work it is counting. A `persist` that fails says so where it
/// is wired; the tree is already in the state the record describes, so the
/// outcome stands and what is lost is the record of it.
///
/// Returns the assignment ids it touched, in the order it touched them.
pub(crate) fn drain_pending_assignment_inputs(
    store: &mut CodingSessionWorkdirStore,
    mut persist: impl FnMut(&CodingSessionWorkdirStore),
) -> Vec<String> {
    let mut drained = Vec::new();
    for assignment_id in pending_assignment_inputs(store) {
        let Some(record) = store.assignment_input(&assignment_id).cloned() else {
            continue;
        };
        drained.push(assignment_id.clone());
        if record.attempts >= MAX_ESTABLISH_ATTEMPTS {
            let mut abandoned = record;
            abandoned.outcome = ASSIGNMENT_INPUT_ABANDONED.to_string();
            abandoned.message = Some(format!(
                "{} attempts to establish this input were started on this computer and none \
                 finished, so it will not be tried again on its own",
                abandoned.attempts
            ));
            abandoned.recorded_at = now_iso();
            store.record_assignment_input(abandoned);
            persist(store);
            continue;
        }
        let mut started = record;
        started.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_string();
        started.attempts += 1;
        started.message = None;
        started.recorded_at = now_iso();
        store.record_assignment_input(started);
        persist(store);
        // The attempt files its own terminal record, carrying the count
        // forward. Its refusal is the record; nothing here re-reads it, and
        // nothing here tries again.
        let _ = crate::coding_sessions::assignment_input::establish(
            store,
            &EstablishAssignmentInputRequest {
                assignment_id: assignment_id.clone(),
                ..EstablishAssignmentInputRequest::default()
            },
        );
        persist(store);
    }
    drained
}

/// Put a settled assignment back in the queue, from a person's own click.
///
/// The attempt count goes back to zero: a person asking again is a new
/// question, and the bound exists to stop *the host* repeating itself.
pub(crate) fn requeue_assignment_input(
    store: &mut CodingSessionWorkdirStore,
    assignment_id: &str,
) -> bool {
    let Some(record) = store.assignment_input(assignment_id).cloned() else {
        return false;
    };
    let mut requeued = record;
    requeued.outcome = ASSIGNMENT_INPUT_INTENDED.to_string();
    requeued.message = None;
    requeued.changes = None;
    requeued.attempts = 0;
    requeued.recorded_at = now_iso();
    store.record_assignment_input(requeued);
    true
}

fn statuses(
    store: &CodingSessionWorkdirStore,
    observed: &[ObservedCodingSessionAssignment],
    dispositions: &[CodingSessionAssignmentInputDisposition],
) -> Vec<CodingSessionAssignmentInputStatus> {
    observed
        .iter()
        .zip(dispositions)
        .map(|(assignment, disposition)| {
            let assignment_id = assignment.assignment_id.trim().to_string();
            CodingSessionAssignmentInputStatus {
                record: (*disposition == CodingSessionAssignmentInputDisposition::Recorded)
                    .then(|| store.assignment_input(&assignment_id).cloned())
                    .flatten(),
                assignment_id,
                disposition: *disposition,
            }
        })
        .collect()
}

/// Every record the host holds, as statuses, newest state included.
fn recorded_statuses(
    store: &CodingSessionWorkdirStore,
    assignment_ids: &[String],
) -> Vec<CodingSessionAssignmentInputStatus> {
    assignment_ids
        .iter()
        .map(|assignment_id| CodingSessionAssignmentInputStatus {
            assignment_id: assignment_id.clone(),
            disposition: CodingSessionAssignmentInputDisposition::Recorded,
            record: store.assignment_input(assignment_id).cloned(),
        })
        .collect()
}

/// Run the whole sequence under the store's lock: load, act, drain, save.
fn with_store<T>(
    app: &AppHandle,
    state: &AppState,
    act: impl FnOnce(&mut CodingSessionWorkdirStore, &mut dyn FnMut(&CodingSessionWorkdirStore)) -> T,
) -> Result<T, String> {
    let _lock = lock_workdir_store(app)?;
    let mut store = load_workdir_store(app)?;
    store.version = WORKDIR_STORE_VERSION;
    let mut persist = |store: &CodingSessionWorkdirStore| {
        if let Err(error) = save_workdir_store(app, state, store) {
            eprintln!("coding session: assignment-input queue could not be saved: {error}");
        }
    };
    let answer = act(&mut store, &mut persist);
    persist(&store);
    Ok(answer)
}

/// Queue the inputs these assignments need, establish them, and report.
///
/// The surface's only job: hand over what it folded. It does not check out
/// anything, decide what is a candidate, or hold the "already attempted" set —
/// all three now live in this host's durable record, so closing the surface
/// cannot stop an establishment and relaunching cannot lose one.
#[tauri::command]
pub async fn coding_session_observe_assignment_inputs(
    app: AppHandle,
    state: State<'_, AppState>,
    assignments: Vec<ObservedCodingSessionAssignment>,
) -> Result<Vec<CodingSessionAssignmentInputStatus>, String> {
    with_store(&app, &state, |store, persist| {
        let dispositions = queue_observed_assignments(store, &assignments);
        drain_pending_assignment_inputs(store, |snapshot| persist(snapshot));
        statuses(store, &assignments, &dispositions)
    })
}

/// Finish what an earlier run started: drain the queue with no new sighting.
///
/// Called at launch, with no arguments and no surface, which is the whole
/// point — an assignment queued yesterday is established today whether or not
/// anybody opens the mission that queued it.
#[tauri::command]
pub async fn coding_session_resume_assignment_inputs(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<CodingSessionAssignmentInputStatus>, String> {
    with_store(&app, &state, |store, persist| {
        let drained = drain_pending_assignment_inputs(store, |snapshot| persist(snapshot));
        recorded_statuses(store, &drained)
    })
}

/// Ask this host to establish one assignment's input again.
///
/// A person's click, not a timer. Answers `None` for an assignment this host
/// holds no record of, which is a truer answer than an empty success.
#[tauri::command]
pub async fn coding_session_requeue_assignment_input(
    app: AppHandle,
    state: State<'_, AppState>,
    assignment_id: String,
) -> Result<Option<CodingSessionAssignmentInputStatus>, String> {
    let assignment_id = assignment_id.trim().to_string();
    with_store(&app, &state, |store, persist| {
        if !requeue_assignment_input(store, &assignment_id) {
            return None;
        }
        drain_pending_assignment_inputs(store, |snapshot| persist(snapshot));
        Some(CodingSessionAssignmentInputStatus {
            assignment_id: assignment_id.clone(),
            disposition: CodingSessionAssignmentInputDisposition::Recorded,
            record: store.assignment_input(&assignment_id).cloned(),
        })
    })
}

/// Drain the queue once at launch, off the async executor.
///
/// Errors are printed, never surfaced: this runs with nobody watching, and a
/// store it could not read leaves every pending intent exactly where it was
/// for the next launch or the next observation.
pub fn resume_assignment_inputs_at_launch(app: &AppHandle) {
    use tauri::Manager;

    let state = app.state::<AppState>();
    match with_store(app, state.inner(), |store, persist| {
        drain_pending_assignment_inputs(store, |snapshot| persist(snapshot))
    }) {
        Ok(drained) if !drained.is_empty() => {
            eprintln!(
                "coding session: resumed {} pending assignment input(s) at launch",
                drained.len()
            );
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("coding session: pending assignment inputs could not be resumed: {error}");
        }
    }
}

#[cfg(test)]
#[path = "assignment_establishment_tests.rs"]
mod tests;
