//! Tests for what this app does with an assignment a surface folded.
//!
//! Since lane 202 that is exactly two things — record the intent, and let a
//! person ask for one to be tried again — so these cases pin the *dispositions*
//! and the fact that nothing here establishes anything. The establishment
//! itself, with real repositories, real interruptions and the two-process
//! lock, is proven where it now lives
//! (`crates/beekeeper-session-provider/src/assignment_inputs_tests.rs`).

use std::path::PathBuf;

use super::*;
use crate::coding_sessions::workdir_store::CodingSessionSeatWorktree;
use crate::util::now_iso;
use beekeeper_session_provider_pkg::assignment_inputs::{
    outcome_is_pending as assignment_input_is_pending, record_assignment_input,
    ASSIGNMENT_INPUT_ABANDONED, ASSIGNMENT_INPUT_ESTABLISHED, ASSIGNMENT_INPUT_ESTABLISHING,
    ASSIGNMENT_INPUT_INTENDED, MAX_ESTABLISH_ATTEMPTS,
};

const ASSIGNMENT: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const OTHER_ASSIGNMENT: &str = "1122334455667788990011223344556677889900112233445566778899001122";
const SESSION: &str = "session-establish-1";
const SEAT: &str = "verifier";
const SEAT_BRANCH: &str = "lane/verifier";
const COMMIT: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
const OTHER_COMMIT: &str = "abc0123456789abc0123456789abc0123456789a";

/// A store holding one seat worktree this host recorded cutting.
///
/// The path is inside the managed `<repo>.worktrees/<slug>` holder, because
/// `record_seat_worktree` refuses anything else and this fixture must not
/// route around that guard.
fn store_with_recorded_seat() -> CodingSessionWorkdirStore {
    let repo_root = PathBuf::from("/fixtures/main");
    let mut store = CodingSessionWorkdirStore::default();
    store
        .record_seat_worktree(
            SESSION,
            SEAT,
            CodingSessionSeatWorktree {
                path: PathBuf::from("/fixtures/main.worktrees/verifier"),
                branch: SEAT_BRANCH.to_string(),
                repo_root,
                created_at: now_iso(),
                session_id: None,
                agents_clone: None,
                commit_identity: None,
                actor_pubkey: None,
                seeding: None,
            },
        )
        .expect("recorded seat worktree");
    store
}

fn observed(
    assignment_id: &str,
    role: &str,
    seat_label: Option<&str>,
    base_sha: Option<&str>,
) -> ObservedCodingSessionAssignment {
    ObservedCodingSessionAssignment {
        assignment_id: assignment_id.to_string(),
        session_ref: SESSION.to_string(),
        seat_label: seat_label.map(str::to_string),
        assignee_role: Some(role.to_string()),
        base_sha: base_sha.map(str::to_string),
        branch: None,
    }
}

#[test]
fn an_observed_assignment_is_recorded_as_an_intent_and_nothing_is_established() {
    let mut store = store_with_recorded_seat();
    let dispositions = queue_observed_assignments(
        &mut store,
        &[observed(ASSIGNMENT, "verifier", Some(SEAT), Some(COMMIT))],
    );
    assert_eq!(
        dispositions,
        vec![CodingSessionAssignmentInputDisposition::Recorded]
    );
    let record = store.assignment_input(ASSIGNMENT).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_INTENDED);
    assert_eq!(
        record.attempts, 0,
        "this app records the intent; it does not start attempts"
    );
    assert_eq!(record.commit.as_deref(), Some(COMMIT));
    assert_eq!(pending_assignment_inputs(&store), vec![ASSIGNMENT]);
}

#[test]
fn a_role_that_does_not_start_from_a_commit_queues_nothing() {
    let mut store = store_with_recorded_seat();
    let dispositions = queue_observed_assignments(
        &mut store,
        &[
            observed(ASSIGNMENT, "builder", Some(SEAT), Some(COMMIT)),
            observed(OTHER_ASSIGNMENT, "verifier", Some(SEAT), None),
        ],
    );
    assert_eq!(
        dispositions,
        vec![
            CodingSessionAssignmentInputDisposition::NotRequired,
            CodingSessionAssignmentInputDisposition::Unnamed,
        ]
    );
    assert!(store.assignment_inputs.is_empty());
}

#[test]
fn a_seat_this_host_never_cut_is_off_host_and_writes_no_record() {
    let mut store = store_with_recorded_seat();
    let dispositions = queue_observed_assignments(
        &mut store,
        &[
            observed(
                ASSIGNMENT,
                "runner",
                Some("somebody-elses-seat"),
                Some(COMMIT),
            ),
            observed(OTHER_ASSIGNMENT, "runner", None, Some(COMMIT)),
        ],
    );
    assert_eq!(
        dispositions,
        vec![
            CodingSessionAssignmentInputDisposition::OffHost,
            CodingSessionAssignmentInputDisposition::OffHost,
        ]
    );
    assert!(store.assignment_inputs.is_empty());
}

#[test]
fn an_observation_this_host_cannot_read_is_invalid_rather_than_dropped() {
    let mut store = store_with_recorded_seat();
    let dispositions = queue_observed_assignments(
        &mut store,
        &[
            observed("not-an-event-id", "verifier", Some(SEAT), Some(COMMIT)),
            observed(ASSIGNMENT, "verifier", Some(SEAT), Some("HEAD~1")),
        ],
    );
    assert_eq!(
        dispositions,
        vec![
            CodingSessionAssignmentInputDisposition::Invalid,
            CodingSessionAssignmentInputDisposition::Invalid,
        ]
    );
    assert!(store.assignment_inputs.is_empty());
}

#[test]
fn a_settled_record_for_the_same_commit_is_dispositive_and_a_new_commit_is_not() {
    let mut store = store_with_recorded_seat();
    queue_observed_assignments(
        &mut store,
        &[observed(ASSIGNMENT, "verifier", Some(SEAT), Some(COMMIT))],
    );
    let mut settled = store.assignment_input(ASSIGNMENT).expect("record").clone();
    settled.outcome = ASSIGNMENT_INPUT_ABANDONED.to_string();
    settled.attempts = MAX_ESTABLISH_ATTEMPTS;
    record_assignment_input(&mut store.assignment_inputs, settled);

    // Seeing the same assignment again changes nothing, which is why nothing
    // loops.
    queue_observed_assignments(
        &mut store,
        &[observed(ASSIGNMENT, "verifier", Some(SEAT), Some(COMMIT))],
    );
    assert_eq!(
        store.assignment_input(ASSIGNMENT).expect("record").outcome,
        ASSIGNMENT_INPUT_ABANDONED
    );

    // A different commit is no answer about this one.
    queue_observed_assignments(
        &mut store,
        &[observed(
            ASSIGNMENT,
            "verifier",
            Some(SEAT),
            Some(OTHER_COMMIT),
        )],
    );
    let record = store.assignment_input(ASSIGNMENT).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_INTENDED);
    assert_eq!(record.attempts, 0);
    assert_eq!(record.commit.as_deref(), Some(OTHER_COMMIT));
}

#[test]
fn observing_an_established_assignment_twice_is_read_only() {
    let mut store = store_with_recorded_seat();
    queue_observed_assignments(
        &mut store,
        &[observed(ASSIGNMENT, "verifier", Some(SEAT), Some(COMMIT))],
    );
    let mut settled = store.assignment_input(ASSIGNMENT).expect("record").clone();
    settled.outcome = ASSIGNMENT_INPUT_ESTABLISHED.to_string();
    record_assignment_input(&mut store.assignment_inputs, settled);
    let before = store.clone();

    // No seat label this time — the real-world shape of the defect, where
    // the caller could not resolve a profile name for the actor. A record
    // that already answers this exact commit must be read-only: it neither
    // rewrites the record nor reports `off_host` just because this call
    // could not resolve a checkout.
    let dispositions = queue_observed_assignments(
        &mut store,
        &[observed(ASSIGNMENT, "verifier", None, Some(COMMIT))],
    );
    assert_eq!(
        dispositions,
        vec![CodingSessionAssignmentInputDisposition::Recorded]
    );
    assert_eq!(
        store, before,
        "observing an already-answered assignment must not write"
    );

    // Observing it again changes nothing further either.
    let dispositions = queue_observed_assignments(
        &mut store,
        &[observed(ASSIGNMENT, "verifier", None, Some(COMMIT))],
    );
    assert_eq!(
        dispositions,
        vec![CodingSessionAssignmentInputDisposition::Recorded]
    );
    assert_eq!(store, before);
}

#[test]
fn a_persons_requeue_clears_the_count_and_answers_no_for_an_unknown_assignment() {
    let mut store = store_with_recorded_seat();
    queue_observed_assignments(
        &mut store,
        &[observed(ASSIGNMENT, "verifier", Some(SEAT), Some(COMMIT))],
    );
    let mut settled = store.assignment_input(ASSIGNMENT).expect("record").clone();
    settled.outcome = "dirty_tree".to_string();
    settled.attempts = MAX_ESTABLISH_ATTEMPTS;
    settled.changes = Some(3);
    record_assignment_input(&mut store.assignment_inputs, settled);

    assert!(requeue_assignment_input(&mut store, ASSIGNMENT));
    let record = store.assignment_input(ASSIGNMENT).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_INTENDED);
    assert_eq!(record.attempts, 0);
    assert_eq!(record.changes, None);
    assert!(
        !requeue_assignment_input(&mut store, OTHER_ASSIGNMENT),
        "an assignment with no record answers no, not an empty success"
    );
}

#[test]
fn the_pending_words_are_the_ones_the_surface_reads() {
    assert!(assignment_input_is_pending(ASSIGNMENT_INPUT_INTENDED));
    assert!(assignment_input_is_pending(ASSIGNMENT_INPUT_ESTABLISHING));
    assert!(!assignment_input_is_pending(ASSIGNMENT_INPUT_ABANDONED));
    assert!(!assignment_input_is_pending("established"));
    assert_eq!(ASSIGNMENT_INPUT_INTENDED, "intended");
    assert_eq!(ASSIGNMENT_INPUT_ESTABLISHING, "establishing");
    assert_eq!(ASSIGNMENT_INPUT_ABANDONED, "establish_abandoned");
    assert_eq!(
        serde_json::to_value(CodingSessionAssignmentInputDisposition::OffHost).expect("json"),
        serde_json::Value::String("off_host".to_string()),
        "the disposition the surface branches on keeps its exact word"
    );
}
