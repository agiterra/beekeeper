//! Custody through the **real** actor path.
//!
//! Astra's Wave 2 re-check was right about the old proof: a test that takes
//! the custody token by hand shows that the token works, not that the code
//! which must take it does
//! (`assignment_custody_tests.rs`, the two-assignment case). Every case here
//! drives a live session — a real subprocess, a real `session/prompt`, a real
//! dequeue — and then asks the registry what the running turn is holding.
//!
//! The agent is [`STALLING_AGENT`]: it answers `initialize` and `session/new`
//! and then never answers the prompt. That is the barrier. While it sits
//! there, the turn is genuinely running, which is exactly the window in which
//! preparation used to be able to move the tree.

use std::time::Duration;

use super::*;
use crate::assignment_inputs::{
    assignment_input, host_store_from_pointer, queue_intent, record_assignment_input,
    AssignmentInputStore, AssignmentIntent, SeatCheckout, ASSIGNMENT_INPUT_ESTABLISHING,
    ASSIGNMENT_INPUT_INTENDED, ASSIGNMENT_INPUT_MOVED, HOST_STORE_POINTER_FILE,
    HOST_STORE_POINTER_VERSION,
};
use crate::verification_input::VERIFICATION_INPUT_MOVED_AT_START;

/// `git` in `cwd` with the repository-selection variables cleared, so a test
/// running under a hook cannot target the developer's own repository.
fn git(cwd: &Path, args: &[&str]) {
    let mut command = std::process::Command::new("git");
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    let status = command
        .arg("-C")
        .arg(cwd)
        .args([
            "-c",
            "user.email=custody@example.invalid",
            "-c",
            "user.name=custody",
        ])
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?} failed");
}

fn head_of(cwd: &Path) -> String {
    String::from_utf8_lossy(
        &std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git")
            .stdout,
    )
    .trim()
    .to_owned()
}

/// A repository with one commit, and that commit.
fn repo_with_commit(cwd: &Path) -> String {
    git(cwd, &["init", "-q", "-b", "main"]);
    std::fs::write(cwd.join("README.md"), "first\n").expect("write");
    git(cwd, &["add", "README.md"]);
    git(cwd, &["commit", "-q", "--no-gpg-sign", "-m", "first"]);
    head_of(cwd)
}

/// The two-key pointer both producers mint for an assignment wake.
fn assignment_pointer(operation_id: &str) -> String {
    serde_json::json!({"operationId": operation_id, "type": "assignment"}).to_string()
}

/// A desktop-shaped store beside the provider's state directory, plus the
/// pointer that tells this provider where it is.
fn host_store(state: &Path, root: &Path) {
    std::fs::create_dir_all(state).expect("state dir");
    let store = root.join("coding-session-workdirs.json");
    std::fs::write(
        &store,
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 2,
            "worktrees": {},
            "assignmentInputs": {}
        }))
        .expect("serialize"),
    )
    .expect("write the store");
    std::fs::write(
        state.join(HOST_STORE_POINTER_FILE),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": HOST_STORE_POINTER_VERSION,
            "path": store,
        }))
        .expect("serialize"),
    )
    .expect("write the pointer");
}

/// A repository with two commits, on a seat branch at the first.
fn seat_repo(cwd: &Path) -> (String, String) {
    let first = repo_with_commit(cwd);
    std::fs::write(cwd.join("second.txt"), "next\n").expect("write");
    git(cwd, &["add", "second.txt"]);
    git(cwd, &["commit", "-q", "--no-gpg-sign", "-m", "second"]);
    let second = head_of(cwd);
    git(cwd, &["checkout", "-q", "-B", "seat/verifier", &first]);
    (first, second)
}

/// The store, its pointer, and one queued intent for `assignment` at `commit`.
fn queued_store(
    state: &Path,
    root: &Path,
    seat: &Path,
    assignment: &str,
    commit: &str,
) -> AssignmentInputStore {
    host_store(state, root);
    let store = host_store_from_pointer(state).expect("pointer");
    store
        .with_records(|records, persist| {
            queue_intent(
                records,
                &AssignmentIntent {
                    assignment_id: assignment.to_owned(),
                    session_ref: "session-1".to_owned(),
                    seat_label: None,
                    base_sha: commit.to_owned(),
                    branch: None,
                },
                Some(&SeatCheckout {
                    path: seat.to_path_buf(),
                    branch: "seat/verifier".to_owned(),
                }),
            );
            persist(records)
        })
        .expect("lock")
        .expect("save");
    store
}

/// Start a live session whose initial turn stalls, and return the provider and
/// the session id once that turn is genuinely open.
async fn provider_with_a_running_turn(
    dir: &Path,
    state: &Path,
    cwd: &Path,
) -> (Provider, String, Uuid) {
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir, channel_id, cwd);
    let agent = fake_agent(dir, "stalling-agent", STALLING_AGENT);
    let mut provider = Provider::new(config_of(Keys::generate(), state, Some(&projects), agent))
        .expect("provider");
    let create = create_event_with_initial_turn(&provider, channel_id, "create-1", "go");
    provider
        .handle_command_event(channel_id, &create)
        .await
        .expect("handle");
    let started = tokio::time::timeout(Duration::from_secs(10), provider.next_session_event())
        .await
        .expect("the actor starts its initial turn")
        .expect("event");
    provider.handle_session_event(started).expect("record");
    pump_available(&mut provider).await;
    let session_id = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .session_id
        .clone();
    assert!(
        provider
            .state()
            .session(&session_id)
            .expect("session")
            .open_turn
            .is_some(),
        "the barrier only works if the turn really is open"
    );
    (provider, session_id, channel_id)
}

/// R2, through the real path: an **ordinary** turn — no assignment pointer, no
/// requirement registered, nothing for the fence to check — still holds its
/// seat's custody while it runs, so preparation for the next assignment
/// cannot move the tree underneath it.
///
/// Before this lane the dequeue returned `NotRequired` without taking the
/// lock, `is_busy` was false for the whole turn, and `establish_for_turn`
/// walked in and ran `git checkout -B`.
#[tokio::test]
async fn an_ordinary_running_turn_holds_its_seats_custody() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    let (first, second) = seat_repo(&seat);
    let (_provider, session_id, _channel) =
        provider_with_a_running_turn(dir.path(), &state, &seat).await;

    assert!(
        crate::assignment_custody::is_busy(&session_id),
        "a running turn must hold its seat's checkout, requirement or not"
    );

    let assignment = "ab".repeat(32);
    let store = queued_store(&state, dir.path(), &seat, &assignment, &second);
    let outcome = crate::assignment_custody::establish_for_turn(
        &session_id,
        &crate::assignment_custody::TurnRequirement {
            assignment_ref: assignment.clone(),
            base_sha: second.clone(),
            cwd: seat.clone(),
            store,
            branch: "seat/verifier".to_owned(),
        },
        Duration::from_secs(5),
    )
    .await;
    assert!(
        matches!(outcome, crate::assignment_custody::Establishment::SeatBusy),
        "the seat is in use: {outcome:?}"
    );
    assert_eq!(
        head_of(&seat),
        first,
        "the running turn's tree must not have moved"
    );
}

/// The same guarantee for a turn the provider *did* prepare: the guard the
/// dequeue took is the same lock preparation asks for, so the next assignment
/// waits for the turn rather than racing it.
#[tokio::test]
async fn a_prepared_running_turn_also_blocks_the_next_assignment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    let (first, second) = seat_repo(&seat);
    let (_provider, session_id, _channel) =
        provider_with_a_running_turn(dir.path(), &state, &seat).await;

    // A requirement registered *after* the turn started is never consumed by
    // it; what matters here is that the lock the turn holds is the one
    // preparation needs.
    let assignment = "cd".repeat(32);
    let store = queued_store(&state, dir.path(), &seat, &assignment, &second);
    let requirement = crate::assignment_custody::TurnRequirement {
        assignment_ref: assignment.clone(),
        base_sha: second.clone(),
        cwd: seat.clone(),
        store: store.clone(),
        branch: "seat/verifier".to_owned(),
    };
    for _ in 0..3 {
        let outcome = crate::assignment_custody::establish_for_turn(
            &session_id,
            &requirement,
            Duration::from_millis(50),
        )
        .await;
        assert!(
            matches!(outcome, crate::assignment_custody::Establishment::SeatBusy),
            "{outcome:?}"
        );
    }
    assert_eq!(head_of(&seat), first, "three passes, and nothing moved");
    let record = store
        .with_records(|records, _| assignment_input(records, &assignment).cloned())
        .expect("read")
        .expect("record");
    assert_eq!(
        record.outcome, ASSIGNMENT_INPUT_INTENDED,
        "the work is still owed; no attempt was started behind the turn"
    );
    assert_eq!(record.attempts, 0);
}

/// R2's second hole, through the real path: a prepared turn whose tree moved
/// before its prompt is **discharged**, not left accepted. The provider
/// publishes a terminal `turn_dropped` receipt naming the moved input, drops
/// the command from `in_flight`, and records the refusal durably.
#[tokio::test]
async fn a_turn_refused_at_the_dequeue_is_discharged_with_a_visible_answer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    let (first, second) = seat_repo(&seat);
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &seat);
    let agent = fake_agent(dir.path(), "stalling-agent", STALLING_AGENT);
    let mut provider = Provider::new(config_of(Keys::generate(), &state, Some(&projects), agent))
        .expect("provider");
    let create = create_event(&provider, channel_id, "create-1");
    provider
        .handle_command_event(channel_id, &create)
        .await
        .expect("handle");
    pump_available(&mut provider).await;
    let record = provider.state().sessions().next().expect("session").clone();
    let target = provider.target_for(&record);
    let session_id = record.session_id.clone();

    // A requirement for a commit the tree does **not** hold: this is the
    // state left behind when another assignment moved the tree between the
    // provider's decision and this prompt.
    let assignment = "ef".repeat(32);
    let store = queued_store(&state, dir.path(), &seat, &assignment, &second);
    crate::assignment_custody::remember_requirement(
        &session_id,
        "wake-1",
        crate::assignment_custody::TurnRequirement {
            assignment_ref: assignment.clone(),
            base_sha: second.clone(),
            cwd: seat.clone(),
            store: store.clone(),
            branch: "seat/verifier".to_owned(),
        },
    );

    let wake = command_event(
        channel_id,
        "wake-1",
        &target,
        serde_json::json!({
            "type": "thread.turn.start",
            "text": assignment_pointer(&assignment),
        }),
    );
    provider
        .handle_command_event(channel_id, &wake)
        .await
        .expect("handle");
    // The actor dequeues, refuses, and tells the provider.
    let dropped = tokio::time::timeout(Duration::from_secs(10), provider.next_session_event())
        .await
        .expect("the actor reports the refusal")
        .expect("event");
    assert!(
        matches!(
            &dropped,
            SessionEvent::TurnDropped {
                reason: crate::session::TurnDropReason::InputMoved { .. },
                ..
            }
        ),
        "the actor must name why, not let the provider guess: {dropped:?}"
    );
    provider.handle_session_event(dropped).expect("fold");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let receipts = sink.contents_of(buzz_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
    let dropped_receipt = receipts
        .iter()
        .find(|receipt| receipt["commandId"] == "wake-1" && receipt["status"] == "turn_dropped")
        .unwrap_or_else(|| panic!("a terminal receipt a person can see; got {receipts:?}"));
    assert_eq!(
        dropped_receipt["error"]["code"], VERIFICATION_INPUT_MOVED_AT_START,
        "the answer names the moved input, not a full queue: {dropped_receipt}"
    );
    assert!(
        dropped_receipt["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains(&second) && message.contains(&first)),
        "and names both commits: {dropped_receipt}"
    );
    assert!(
        !provider.in_flight.contains_key("wake-1"),
        "the delivery is discharged, not accepted-but-never-started"
    );
    assert!(
        provider.state.record_refusal("wake-1", now_secs()).is_ok(),
        "the durable answer is idempotent under the same command id"
    );
    assert_eq!(head_of(&seat), first, "nothing moved");
}

/// R3, the interleaving Astra gives: the dequeue observes a mismatch, an
/// operator requeue then mints a new attempt, and the dequeue's observation
/// must not overwrite the new attempt's ownership.
#[tokio::test]
async fn a_dequeue_observation_never_clears_a_newer_attempts_owner() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    let (_first, second) = seat_repo(&seat);
    let assignment = "12".repeat(32);
    let store = queued_store(&state, dir.path(), &seat, &assignment, &second);
    let session_id = format!("seat-{}", Uuid::new_v4());

    // The row as a fresh attempt from another party would leave it: owned,
    // mid-flight, one attempt started.
    store
        .with_records(|records, persist| {
            let mut running = assignment_input(records, &assignment)
                .cloned()
                .expect("record");
            running.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_owned();
            running.attempts = 1;
            running.attempt_owner = Some("owner-of-the-new-attempt".to_owned());
            record_assignment_input(records, running);
            persist(records)
        })
        .expect("lock")
        .expect("save");

    // The dequeue boundary observes the row, finds the tree wrong, and would
    // have written `tree_moved` over it. Its snapshot is taken under custody
    // *before* the probe, so a row that changed since is left alone — and
    // this row has: the seeded owner is not what a stale observer saw.
    crate::assignment_custody::remember_requirement(
        &session_id,
        "wake-1",
        crate::assignment_custody::TurnRequirement {
            assignment_ref: assignment.clone(),
            base_sha: "ab".repeat(20),
            cwd: seat.clone(),
            store: store.clone(),
            branch: "seat/verifier".to_owned(),
        },
    );
    let custody = crate::assignment_custody::verify_for_turn(&session_id, "wake-1").await;
    assert!(
        matches!(
            custody,
            crate::assignment_custody::TurnCustody::Refused { .. }
        ),
        "the tree does not hold that commit: {custody:?}"
    );

    let row = store
        .with_records(|records, _| assignment_input(records, &assignment).cloned())
        .expect("read")
        .expect("record");
    assert_eq!(
        row.attempt_owner.as_deref(),
        Some("owner-of-the-new-attempt"),
        "a dequeue observation must not destroy an attempt's ownership"
    );
    assert_eq!(row.attempts, 1);
    assert_eq!(
        row.outcome, ASSIGNMENT_INPUT_ESTABLISHING,
        "nor overwrite the outcome the owner is about to write"
    );
}

/// The observation *is* written when the row is untouched — otherwise the
/// conditional write would be a silent no-op and the reason would be lost.
#[tokio::test]
async fn a_dequeue_observation_is_written_when_the_row_is_the_one_it_saw() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    let (_first, second) = seat_repo(&seat);
    let assignment = "34".repeat(32);
    let store = queued_store(&state, dir.path(), &seat, &assignment, &second);
    let session_id = format!("seat-{}", Uuid::new_v4());
    crate::assignment_custody::remember_requirement(
        &session_id,
        "wake-1",
        crate::assignment_custody::TurnRequirement {
            assignment_ref: assignment.clone(),
            base_sha: "ab".repeat(20),
            cwd: seat.clone(),
            store: store.clone(),
            branch: "seat/verifier".to_owned(),
        },
    );
    let custody = crate::assignment_custody::verify_for_turn(&session_id, "wake-1").await;
    assert!(matches!(
        custody,
        crate::assignment_custody::TurnCustody::Refused { .. }
    ));
    let row = store
        .with_records(|records, _| assignment_input(records, &assignment).cloned())
        .expect("read")
        .expect("record");
    assert_eq!(row.outcome, ASSIGNMENT_INPUT_MOVED);
    assert!(
        row.message
            .as_deref()
            .is_some_and(|message| message.contains("about to start")),
        "{:?}",
        row.message
    );
}
