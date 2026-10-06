//! The desktop's adapter onto the shared assignment-input records.
//!
//! # What this file is now
//!
//! Establishing a seat's exact input — checking out the commit its assignment
//! names, in that seat's own worktree — used to live here in full: the git
//! ladder, the refusal codes, the durable record, and (after lane 185) the
//! queue that drained them. All of it now lives in
//! [`beekeeper_session_provider_pkg::assignment_inputs`], because the party that must
//! own the order `accept → establish → verify → start` is the one holding both
//! the governed kind-44244 subscription and the turn gate, and that party is
//! the provider, not this app (ledger 185's "owed, live" paragraph, and 202).
//!
//! What is left here is an **adapter and a display**: the seat's tree is
//! resolved from the record this host wrote when it cut it, the shared core is
//! asked to do the work, and the same rows are read back for the mission
//! inspector. The types are re-exported rather than redefined — one record
//! shape, in one crate, written by both processes under one lock.
//!
//! # What it still will not do
//!
//! Every rule the original stated still holds, and holds in one place now: it
//! never discards work (a dirty tree is refused, nothing written), it never
//! adopts a tree this host did not cut (the worktree comes from
//! [`CodingSessionWorkdirStore::worktrees`], never from a search of the disk),
//! it never names a remote (the ladder resolves one at run time), and a commit
//! already in the seat's repository is established without a fetch at all.

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::{
    load_workdir_store, load_workdir_store_readonly, lock_workdir_store, save_workdir_store,
    seat_worktree_key, CodingSessionWorkdirStore, WORKDIR_STORE_VERSION,
};

use beekeeper_session_provider_pkg::assignment_inputs::{refuse, SeatCheckout};
pub(crate) use beekeeper_session_provider_pkg::assignment_inputs::{
    AssignmentInputRecord as CodingSessionAssignmentInputRecord, EstablishAssignmentInputRequest,
    MAX_ASSIGNMENT_INPUT_RECORDS,
};
pub use beekeeper_session_provider_pkg::assignment_inputs::{
    EstablishAssignmentInputCode, EstablishAssignmentInputError, EstablishedAssignmentInput,
};

/// Where this host recorded cutting that seat's tree, if it did.
///
/// The record, never the disk: a directory this host did not create is a
/// directory it must not move, and the whole point of the `worktrees` map is
/// that the host can name what it made.
pub(crate) fn seat_checkout(
    store: &CodingSessionWorkdirStore,
    session_ref: &str,
    seat_label: &str,
) -> Option<SeatCheckout> {
    store
        .worktrees
        .get(&seat_worktree_key(session_ref, seat_label))
        .map(|recorded| SeatCheckout {
            path: recorded.path.clone(),
            branch: recorded.branch.clone(),
        })
}

impl CodingSessionWorkdirStore {
    /// The last attempt recorded for one assignment, if any.
    pub(crate) fn assignment_input(
        &self,
        assignment_id: &str,
    ) -> Option<&CodingSessionAssignmentInputRecord> {
        beekeeper_session_provider_pkg::assignment_inputs::assignment_input(
            &self.assignment_inputs,
            assignment_id,
        )
    }
}

/// The seat label a worktree was recorded under, keyed by the seat's own
/// actor pubkey — never by a profile name, which a hired seat may not have.
///
/// `unattributed` is true when this session holds at least one worktree
/// record with no `actorPubkey` at all: a tree cut before this field existed.
/// A caller that finds no entry for an actor it is asking about, while this is
/// true, cannot tell "this host never cut that seat's tree" from "this host
/// cut it before it could say whose it was" — those are different facts and
/// must not both read as `off_host`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionSeatWorktreeActors {
    /// Actor pubkey (lowercase hex) → the seat label this host cut the tree
    /// under, for every worktree in this session that named one.
    pub labels: std::collections::BTreeMap<String, String>,
    /// Whether this session holds a worktree recorded before actor pubkeys
    /// were tracked, so a miss above is not necessarily off-host.
    pub unattributed: bool,
}

/// Read-only: every seat label this host cut a tree under for `session_ref`,
/// keyed by the seat's actor pubkey where one was recorded.
pub(crate) fn seat_worktree_actors(
    store: &CodingSessionWorkdirStore,
    session_ref: &str,
) -> CodingSessionSeatWorktreeActors {
    let session_ref = session_ref.trim();
    let mut labels = std::collections::BTreeMap::new();
    let mut unattributed = false;
    for (key, entry) in &store.worktrees {
        let Some((entry_session, seat_label)) = key.split_once('/') else {
            continue;
        };
        if entry_session != session_ref {
            continue;
        }
        match &entry.actor_pubkey {
            Some(pubkey) => {
                labels.insert(pubkey.clone(), seat_label.to_string());
            }
            None => unattributed = true,
        }
    }
    CodingSessionSeatWorktreeActors {
        labels,
        unattributed,
    }
}

/// Read-only: every seat label this host cut a tree under for one session,
/// keyed by the seat's own actor pubkey.
///
/// Used to resolve which seat a signed assignment's `assigneeActor` belongs
/// to when establishing its input — a hired seat has no relay profile name in
/// general, so a resolver built from profile lookups (`useCodingSessionActorNames`)
/// cannot answer this; the store, keyed by the actor the hire itself signed
/// as, can.
#[tauri::command]
pub async fn coding_session_seat_worktree_actors(
    app: AppHandle,
    session_ref: String,
) -> Result<CodingSessionSeatWorktreeActors, String> {
    let store = load_workdir_store_readonly(&app)?;
    Ok(seat_worktree_actors(&store, &session_ref))
}

/// Run one attempt against this host's store, resolving the tree from it.
///
/// Split from the command so the behaviour is testable without a Tauri app
/// handle; the command is this plus the store's lock, load and save.
pub(crate) fn establish(
    store: &mut CodingSessionWorkdirStore,
    request: &EstablishAssignmentInputRequest,
    state_dir: &std::path::Path,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let session_ref = request.session_ref.clone().or_else(|| {
        beekeeper_session_provider_pkg::assignment_inputs::assignment_input(
            &store.assignment_inputs,
            &request.assignment_id,
        )
        .and_then(|record| record.session_ref.clone())
    });
    let seat_label = request.seat_label.clone().or_else(|| {
        beekeeper_session_provider_pkg::assignment_inputs::assignment_input(
            &store.assignment_inputs,
            &request.assignment_id,
        )
        .and_then(|record| record.seat_label.clone())
    });
    let checkout = match (session_ref.as_deref(), seat_label.as_deref()) {
        (Some(session_ref), Some(seat_label)) => seat_checkout(store, session_ref, seat_label),
        _ => None,
    };
    beekeeper_session_provider_pkg::assignment_inputs::establish(
        &mut store.assignment_inputs,
        request,
        checkout.as_ref(),
        state_dir,
    )
}

/// Put the commit an assignment names into that seat's own worktree.
///
/// `commit`, `branch`, `sessionRef` and `seatLabel` may be omitted on a retry:
/// the last recorded attempt for `assignmentId` supplies them, and anything
/// supplied wins over what was recorded. Idempotent — a second call with the
/// same inputs answers `alreadyCurrent: true` and writes nothing to the tree.
///
/// Kept registered as the single-assignment entry point a person can reach.
/// The provider establishes inputs on its own, before the seat's turn opens;
/// this is the door for an operator who wants one done now.
#[tauri::command]
pub async fn coding_session_establish_assignment_input(
    app: AppHandle,
    state: State<'_, AppState>,
    assignment_id: String,
    session_ref: Option<String>,
    seat_label: Option<String>,
    commit: Option<String>,
    branch: Option<String>,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let request = EstablishAssignmentInputRequest {
        assignment_id,
        session_ref,
        seat_label,
        commit,
        branch,
    };
    // A store this host cannot read is reported as a missing record rather
    // than as some other failure: what the caller needs to know is that the
    // seat's tree could not be resolved from the record, which is true.
    let unreadable = |error: String| {
        refuse(
            EstablishAssignmentInputCode::UnrecordedTree,
            "this host could not read the record it writes when it cuts a worktree",
            Some(error),
        )
    };
    let _lock = lock_workdir_store(&app).map_err(unreadable)?;
    let mut store = load_workdir_store(&app).map_err(unreadable)?;
    // Pointed at this host's worktree record, so the seat's branch authority
    // is the one every other preparation of the tree reads (ledger 277).
    let state_dir = crate::session_provider::host_git_state_dir(&app)
        .and_then(|dir| {
            crate::coding_sessions::workdir_store::materialize_host_store_pointer(&app, &dir)
                .map(|()| dir)
        })
        .map_err(|error| {
            refuse(
                EstablishAssignmentInputCode::CheckoutFailed,
                "this host could not prepare the boundary the seat's Git runs inside",
                Some(error),
            )
        })?;
    let outcome = establish(&mut store, &request, &state_dir);
    store.version = WORKDIR_STORE_VERSION;
    if let Err(error) = save_workdir_store(&app, &state, &store) {
        // The disk is already in the state the outcome describes, so the
        // outcome stands; what is lost is the durable record of it, and that
        // is said out loud rather than folded into the result.
        eprintln!("coding session: assignment input record could not be saved: {error}");
    }
    outcome
}

/// What this host last established for one assignment, without touching git.
///
/// The read the mission inspector uses: it answers from the record alone, so
/// it costs nothing and cannot move a tree. The record it reads is the same
/// row the provider writes, in the same file.
#[tauri::command]
pub async fn coding_session_assignment_input_record(
    app: AppHandle,
    assignment_id: String,
) -> Result<Option<CodingSessionAssignmentInputRecord>, String> {
    let store = load_workdir_store_readonly(&app)?;
    Ok(
        beekeeper_session_provider_pkg::assignment_inputs::assignment_input(
            &store.assignment_inputs,
            &assignment_id,
        )
        .cloned(),
    )
}

#[cfg(test)]
#[path = "assignment_input_tests.rs"]
mod tests;
