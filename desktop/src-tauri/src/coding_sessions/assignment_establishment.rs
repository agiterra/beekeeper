//! Recording what a surface saw, for the party that acts on it.
//!
//! # Who establishes an input now, and why it is not this process
//!
//! Lane 185 moved "accept assignment → establish input → record result" out of
//! a React effect and into a durable host queue, which closed the half of the
//! defect that a *closed panel* could stop the work. It left the other half
//! open, and said so: this app holds no relay subscription of its own for kind
//! 44244 — every team transaction arrives as an argument to
//! `fold_coding_session_team_transactions` — so the first **sighting** still
//! came from a mounted surface; and nothing ordered establishment before the
//! seat's wake, so a wake that raced ahead was refused by the provider's fence
//! and cost a re-issued assignment and a model turn (ledger 133, 178(d), 185).
//!
//! Both halves belong to the party that holds the governed subscription *and*
//! the turn gate. That is the provider. It now records the intent from its own
//! verified complete-discovery pass, drains it, and defers a wake that arrives
//! while an establishment is pending
//! ([`beekeeper_session_provider_pkg::assignment_inputs`], and the fence in
//! `crates/beekeeper-session-provider/src/verification_input.rs`).
//!
//! # What this module is for
//!
//! Two things, and deliberately not a third:
//!
//! * a surface that folded an assignment can still **hand over what it saw**,
//!   which costs nothing and can only ever add a row the provider would have
//!   recorded anyway;
//! * a person can ask for one settled assignment to be **tried again**.
//!
//! It no longer drains. A drain from here would be a second scheduler over one
//! record set, racing the party that decides whether the turn may open, and
//! the two could disagree about whether an attempt was in flight. The records,
//! the file and the lock are shared; the *order* has one owner.

use beekeeper_core_pkg::coding_session_team_transaction::role_requires_verification_input;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::coding_sessions::assignment_input::{seat_checkout, CodingSessionAssignmentInputRecord};
use crate::coding_sessions::workdir_store::{
    load_workdir_store, lock_workdir_store, save_workdir_store, CodingSessionWorkdirStore,
    WORKDIR_STORE_VERSION,
};

use beekeeper_session_provider_pkg::assignment_inputs::{
    queue_intent, requeue_assignment_input as requeue_assignment_input_core,
    AssignmentInputDisposition, AssignmentIntent,
};

/// What this host has to say about one observed assignment's input.
///
/// The shared disposition, re-exported: the words a surface reads are the
/// words the provider writes.
pub use beekeeper_session_provider_pkg::assignment_inputs::AssignmentInputDisposition as CodingSessionAssignmentInputDisposition;

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
/// replaced by a fresh intent with the attempt count back at zero. The rule
/// itself lives in the shared core; what is decided here is only which
/// observations are this host's business at all.
pub(crate) fn queue_observed_assignments(
    store: &mut CodingSessionWorkdirStore,
    observed: &[ObservedCodingSessionAssignment],
) -> Vec<AssignmentInputDisposition> {
    observed
        .iter()
        .map(|assignment| queue_one(store, assignment))
        .collect()
}

fn queue_one(
    store: &mut CodingSessionWorkdirStore,
    assignment: &ObservedCodingSessionAssignment,
) -> AssignmentInputDisposition {
    let role = assignment.assignee_role.as_deref().unwrap_or("").trim();
    if !role_requires_verification_input(role) {
        return AssignmentInputDisposition::NotRequired;
    }
    let assignment_id = assignment.assignment_id.trim();
    let commit = assignment.base_sha.as_deref().unwrap_or("").trim();
    // Read-only fast path: a record already naming this exact commit answers
    // the question, so a mere observation — which can arrive on every status
    // read, whether or not this host holds a checkout for the seat — must not
    // reach the queue at all. `queue_intent` would answer the same way, but
    // going by way of it still costs a mutation attempt and a save; this
    // keeps "observing" from being able to write in the first place.
    if !commit.is_empty()
        && store
            .assignment_input(assignment_id)
            .is_some_and(|record| record.commit.as_deref() == Some(commit))
    {
        return AssignmentInputDisposition::Recorded;
    }
    let session_ref = assignment.session_ref.trim().to_string();
    let seat_label = assignment
        .seat_label
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    // The tree this host recorded cutting, or nothing. No search of the disk:
    // adopting a tree this host did not cut is how a seat's uncommitted work
    // would get moved out from under somebody.
    let checkout = (!session_ref.is_empty() && !seat_label.is_empty())
        .then(|| seat_checkout(store, &session_ref, &seat_label))
        .flatten();
    queue_intent(
        &mut store.assignment_inputs,
        &AssignmentIntent {
            assignment_id: assignment.assignment_id.trim().to_string(),
            session_ref,
            seat_label: (!seat_label.is_empty()).then_some(seat_label.clone()),
            base_sha: assignment.base_sha.clone().unwrap_or_default(),
            branch: assignment.branch.clone(),
        },
        checkout.as_ref(),
    )
}

/// Every assignment this host still owes an input for, in key order.
#[must_use]
pub(crate) fn pending_assignment_inputs(store: &CodingSessionWorkdirStore) -> Vec<String> {
    beekeeper_session_provider_pkg::assignment_inputs::pending_assignment_inputs(
        &store.assignment_inputs,
    )
}

/// Put a settled assignment back in the queue, from a person's own click.
///
/// The attempt count goes back to zero: a person asking again is a new
/// question, and the bound exists to stop *the host* repeating itself.
pub(crate) fn requeue_assignment_input(
    store: &mut CodingSessionWorkdirStore,
    assignment_id: &str,
) -> bool {
    requeue_assignment_input_core(&mut store.assignment_inputs, assignment_id)
}

fn statuses(
    store: &CodingSessionWorkdirStore,
    observed: &[ObservedCodingSessionAssignment],
    dispositions: &[AssignmentInputDisposition],
) -> Vec<CodingSessionAssignmentInputStatus> {
    observed
        .iter()
        .zip(dispositions)
        .map(|(assignment, disposition)| {
            let assignment_id = assignment.assignment_id.trim().to_string();
            CodingSessionAssignmentInputStatus {
                record: (*disposition == AssignmentInputDisposition::Recorded)
                    .then(|| store.assignment_input(&assignment_id).cloned())
                    .flatten(),
                assignment_id,
                disposition: *disposition,
            }
        })
        .collect()
}

/// Every record the host holds for these assignments, as statuses.
fn recorded_statuses(
    store: &CodingSessionWorkdirStore,
    assignment_ids: &[String],
) -> Vec<CodingSessionAssignmentInputStatus> {
    assignment_ids
        .iter()
        .map(|assignment_id| CodingSessionAssignmentInputStatus {
            assignment_id: assignment_id.clone(),
            disposition: AssignmentInputDisposition::Recorded,
            record: store.assignment_input(assignment_id).cloned(),
        })
        .collect()
}

/// Run one mutation under the store's lock: load, act, save.
fn with_store<T>(
    app: &AppHandle,
    state: &AppState,
    act: impl FnOnce(&mut CodingSessionWorkdirStore) -> T,
) -> Result<T, String> {
    let _lock = lock_workdir_store(app)?;
    let mut store = load_workdir_store(app)?;
    store.version = WORKDIR_STORE_VERSION;
    let answer = act(&mut store);
    if let Err(error) = save_workdir_store(app, state, &store) {
        eprintln!("coding session: assignment-input records could not be saved: {error}");
    }
    Ok(answer)
}

/// Record the inputs these assignments need, and report what is known.
///
/// The surface's only job: hand over what it folded. It checks out nothing,
/// and it decides nothing about *when* — the provider establishes an input
/// before it opens the turn that needs it, whether or not this panel was ever
/// mounted.
#[tauri::command]
pub async fn coding_session_observe_assignment_inputs(
    app: AppHandle,
    state: State<'_, AppState>,
    assignments: Vec<ObservedCodingSessionAssignment>,
) -> Result<Vec<CodingSessionAssignmentInputStatus>, String> {
    with_store(&app, &state, |store| {
        let dispositions = queue_observed_assignments(store, &assignments);
        statuses(store, &assignments, &dispositions)
    })
}

/// Read back whatever this host still owes an input for.
///
/// Kept registered under its old name because the hook and the launch path
/// call it, but it no longer drains anything: it answers with the rows, so a
/// surface and an operator can see that work is outstanding and who is doing
/// it. The establishment itself belongs to the provider.
#[tauri::command]
pub async fn coding_session_resume_assignment_inputs(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<CodingSessionAssignmentInputStatus>, String> {
    with_store(&app, &state, |store| {
        let pending = pending_assignment_inputs(store);
        recorded_statuses(store, &pending)
    })
}

/// Ask for one assignment's input to be established again.
///
/// A person's request, not a timer: the record is put back to `intended` with
/// its attempt count zeroed, and the provider's next pass over that channel
/// drains it. Answers `None` for an assignment this host holds no record of,
/// which is a truer answer than an empty success.
#[tauri::command]
pub async fn coding_session_requeue_assignment_input(
    app: AppHandle,
    state: State<'_, AppState>,
    assignment_id: String,
) -> Result<Option<CodingSessionAssignmentInputStatus>, String> {
    let assignment_id = assignment_id.trim().to_string();
    with_store(&app, &state, |store| {
        if !requeue_assignment_input(store, &assignment_id) {
            return None;
        }
        Some(CodingSessionAssignmentInputStatus {
            assignment_id: assignment_id.clone(),
            disposition: AssignmentInputDisposition::Recorded,
            record: store.assignment_input(&assignment_id).cloned(),
        })
    })
}

/// Say at launch what is still owed, without doing it.
///
/// Runs with nobody watching, so errors are printed rather than surfaced. It
/// deliberately establishes nothing: an app that drained here would be racing
/// the provider, which is the party that knows whether a turn is about to open
/// against the tree in question.
pub fn resume_assignment_inputs_at_launch(app: &AppHandle) {
    use tauri::Manager;

    let state = app.state::<AppState>();
    match with_store(app, state.inner(), |store| pending_assignment_inputs(store)) {
        Ok(pending) if !pending.is_empty() => {
            eprintln!(
                "coding session: {} assignment input(s) are still owed; the provider establishes \
                 them before the turns that need them",
                pending.len()
            );
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("coding session: pending assignment inputs could not be read: {error}");
        }
    }
}

#[cfg(test)]
#[path = "assignment_establishment_tests.rs"]
mod tests;
