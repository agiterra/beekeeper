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

// ------------------- one custody identity per seat (lane 227, third look R2)

/// The compressed dequeue wait is process-global, so the two cases that use
/// it take turns. Without this they can unset each other's bound and one of
/// them waits the production five minutes — a flake that would look like a
/// custody bug.
static WAIT_BOUND: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Hold the compressed wait for the length of one case.
fn compressed_wait(bound: Duration) -> impl Drop {
    struct Restore(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::assignment_custody::set_wait_bound_for_tests(None);
        }
    }
    let guard = match WAIT_BOUND.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    crate::assignment_custody::set_wait_bound_for_tests(Some(bound));
    Restore(guard)
}

/// Astra's probe, as a test: with one custody identity per seat, no public
/// path produces a second concurrent guard while the first is alive.
///
/// Before this lane `supersede_seat` replaced the seat's mutex, so the probe
/// printed `busy=false` with the old guard still held and then acquired a
/// second guard. Replacing a locked mutex is not a handoff.
#[tokio::test]
async fn no_second_custody_exists_while_the_first_is_held() {
    let seat = format!("seat-{}", Uuid::new_v4());
    let first = crate::assignment_custody::hold(&seat).await;
    assert!(crate::assignment_custody::is_busy(&seat));
    // Every way in: the plain wait, and the bounded one the dequeue uses.
    assert!(
        crate::assignment_custody::hold_for_turn(&seat, Duration::from_millis(50))
            .await
            .is_none(),
        "a second guard must not be obtainable while the first is alive"
    );
    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            crate::assignment_custody::hold(&seat)
        )
        .await
        .is_err(),
        "and waiting does not conjure one either"
    );
    assert!(crate::assignment_custody::is_busy(&seat));
    drop(first);
    assert!(!crate::assignment_custody::is_busy(&seat));
}

/// A turn that cannot obtain custody is **discharged, not run**: the actor
/// reports it, the provider publishes `VERIFICATION_INPUT_SEAT_BUSY`, and the
/// agent is never prompted.
///
/// This is the shape the old fix hid: with a replaced lock the follow-up ran
/// happily while somebody else still held the tree.
#[tokio::test]
async fn a_turn_that_cannot_get_custody_is_discharged_and_never_prompted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    seat_repo(&seat);
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &seat);
    let agent = fake_agent(dir.path(), "stalling-agent", STALLING_AGENT);
    let mut provider = Provider::new(config_of(Keys::generate(), &state, Some(&projects), agent))
        .expect("provider");
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    pump_available(&mut provider).await;
    let record = provider.state().sessions().next().expect("session").clone();
    let target = provider.target_for(&record);
    let session_id = record.session_id.clone();

    // Somebody else holds this seat's checkout — an establishment that has
    // not finished. Compress the dequeue's wait so the answer arrives in test
    // time; production waits `CUSTODY_WAIT_BOUND`.
    let squatter = crate::assignment_custody::hold(&session_id).await;
    let _compressed = compressed_wait(Duration::from_millis(100));

    provider
        .handle_command_event(channel_id, &turn_event(channel_id, "wake-1", &target))
        .await
        .expect("handle");
    let dropped = tokio::time::timeout(Duration::from_secs(10), provider.next_session_event())
        .await
        .expect("the actor reports rather than running")
        .expect("event");
    assert!(
        matches!(
            &dropped,
            SessionEvent::TurnDropped {
                reason: crate::session::TurnDropReason::SeatBusy,
                ..
            }
        ),
        "{dropped:?}"
    );
    provider.handle_session_event(dropped).expect("fold");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let answer = sink
        .contents_of(buzz_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "wake-1" && receipt["status"] == "turn_dropped")
        .expect("a receipt a person can see");
    assert_eq!(
        answer["error"]["code"],
        crate::verification_input::VERIFICATION_INPUT_SEAT_BUSY
    );
    assert!(
        !provider.in_flight.contains_key("wake-1"),
        "the delivery is discharged"
    );
    drop(squatter);
}

/// Restart during a cancellation grace: the predecessor is **ended**, not
/// out-voted. `shutdown_and_join` reports which proof it got, the seat's
/// custody is free afterwards, and no second lock was ever minted.
#[tokio::test]
async fn a_restart_ends_the_predecessor_and_frees_its_seat() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    seat_repo(&seat);
    let (mut provider, session_id, _channel) =
        provider_with_a_running_turn(dir.path(), &state, &seat).await;
    // The running turn holds the seat, as it must.
    assert!(crate::assignment_custody::is_busy(&session_id));

    // The grace is compressed: what is under test is that the predecessor is
    // ended and says how, not how long a real agent gets to wind down.
    let quiescence = provider
        .sessions
        .shutdown_and_join(&session_id, Duration::from_millis(250))
        .await;
    assert!(
        matches!(
            quiescence,
            crate::session::ActorQuiescence::Joined | crate::session::ActorQuiescence::Aborted
        ),
        "a successor learns what happened to its predecessor: {quiescence:?}"
    );
    // The custody the predecessor held is released because the predecessor is
    // gone — not because anybody replaced its lock.
    for _ in 0..50 {
        if !crate::assignment_custody::is_busy(&session_id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !crate::assignment_custody::is_busy(&session_id),
        "ending the holder is what frees the seat"
    );
}

/// An establishment that outlives the dequeue's wait keeps the tree: the
/// follow-up turn does not run, and the checkout it was racing lands on the
/// seat afterwards rather than underneath it.
#[tokio::test]
async fn an_establishment_that_outlives_the_wait_keeps_the_tree() {
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
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    pump_available(&mut provider).await;
    let record = provider.state().sessions().next().expect("session").clone();
    let target = provider.target_for(&record);
    let session_id = record.session_id.clone();

    // B's establishment, holding the seat across its git work.
    let assignment = "56".repeat(32);
    let store = queued_store(&state, dir.path(), &seat, &assignment, &second);
    let establishing = crate::assignment_custody::hold(&session_id).await;
    let _compressed = compressed_wait(Duration::from_millis(100));

    // The ordinary follow-up arrives while that establishment still runs.
    provider
        .handle_command_event(channel_id, &turn_event(channel_id, "follow-up", &target))
        .await
        .expect("handle");
    let dropped = tokio::time::timeout(Duration::from_secs(10), provider.next_session_event())
        .await
        .expect("the follow-up is answered, not run")
        .expect("event");
    assert!(
        matches!(
            &dropped,
            SessionEvent::TurnDropped {
                reason: crate::session::TurnDropReason::SeatBusy,
                ..
            }
        ),
        "{dropped:?}"
    );
    assert_eq!(
        head_of(&seat),
        first,
        "and B's checkout has not landed under it"
    );

    // Only once the establishment ends does the tree move — and then it is
    // B's own turn that would use it.
    drop(establishing);
    let settled = crate::assignment_custody::establish_for_turn(
        &session_id,
        &crate::assignment_custody::TurnRequirement {
            assignment_ref: assignment.clone(),
            base_sha: second.clone(),
            cwd: seat.clone(),
            store,
            branch: "seat/verifier".to_owned(),
        },
        Duration::from_secs(30),
    )
    .await;
    assert!(
        matches!(
            settled,
            crate::assignment_custody::Establishment::Settled(_)
        ),
        "{settled:?}"
    );
    assert_eq!(head_of(&seat), second);
}

/// A dropped provider does not leave an execution holding a seat: the manager
/// ends its actors, so the next provider in this process can take custody.
/// This is the in-process restart case, cured without replacing any lock.
#[tokio::test]
async fn a_dropped_provider_releases_its_seats() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    seat_repo(&seat);
    let session_id = {
        let (provider, session_id, _channel) =
            provider_with_a_running_turn(dir.path(), &state, &seat).await;
        assert!(crate::assignment_custody::is_busy(&session_id));
        drop(provider);
        session_id
    };
    for _ in 0..100 {
        if !crate::assignment_custody::is_busy(&session_id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !crate::assignment_custody::is_busy(&session_id),
        "a provider that is gone holds nothing"
    );
    assert!(
        crate::assignment_custody::hold_for_turn(&session_id, Duration::from_millis(100))
            .await
            .is_some(),
        "and a successor takes the same custody identity, not a new one"
    );
}

/// The A8.1 amendment: an **aborted** actor is not quiescent because
/// `kill_on_drop` exists. That signals the direct child, waits for nothing,
/// and does not touch the process group — so a grandchild the agent spawned
/// can still be writing the seat's tree when a successor takes custody.
///
/// The agent here spawns exactly that grandchild and ignores the cancel. The
/// abort path must kill the whole group, watch it go, and only then release
/// custody; nothing may land in the tree afterwards.
#[cfg(unix)]
#[tokio::test]
async fn an_aborted_actor_leaves_no_grandchild_writing_the_seats_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    seat_repo(&seat);
    let marker = seat.join("grandchild-writes.txt");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &seat);
    let agent = fake_agent(
        dir.path(),
        "grandchild-agent",
        &crate::session::testing::grandchild_writing_agent(&marker.to_string_lossy()),
    );
    let mut provider = Provider::new(config_of(Keys::generate(), &state, Some(&projects), agent))
        .expect("provider");
    provider
        .handle_command_event(
            channel_id,
            &create_event_with_initial_turn(&provider, channel_id, "create-1", "go"),
        )
        .await
        .expect("handle");
    let started = tokio::time::timeout(Duration::from_secs(10), provider.next_session_event())
        .await
        .expect("the prompt is sent")
        .expect("event");
    provider.handle_session_event(started).expect("record");
    let session_id = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .session_id
        .clone();
    let child_pid = provider
        .sessions
        .handle(&session_id)
        .expect("live handle")
        .child_pid_for_tests()
        .expect("a spawned agent has a pid");

    // The grandchild is alive and writing.
    let mut alive = false;
    for _ in 0..200 {
        if marker.metadata().is_ok_and(|meta| meta.len() > 0) {
            alive = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(alive, "the grandchild never started writing");

    // Abort: the grace is short on purpose — this agent answers no cancel, so
    // the join always times out, which is the path under test.
    let quiescence = provider
        .sessions
        .shutdown_and_join(&session_id, Duration::from_millis(150))
        .await;
    assert!(
        matches!(
            quiescence,
            crate::session::ActorQuiescence::Aborted | crate::session::ActorQuiescence::Unproven
        ),
        "{quiescence:?}"
    );

    if matches!(quiescence, crate::session::ActorQuiescence::Unproven) {
        // The honest other outcome: nothing could be proven dead, so the seat
        // is fenced and no successor may run on it at all.
        assert!(
            crate::assignment_custody::is_fenced(&session_id),
            "an unproven predecessor leaves its seat fenced"
        );
        assert!(
            crate::assignment_custody::hold_for_turn(&session_id, Duration::from_millis(50))
                .await
                .is_none(),
            "and a successor is refused custody"
        );
        return;
    }

    // Proven dead. Custody is released only after that proof, so at the
    // instant a successor holds the seat the whole group is already gone —
    // this is the assertion the pre-amendment shape could not make, because
    // it released custody with the kill merely scheduled.
    let successor = crate::assignment_custody::hold_for_turn(&session_id, Duration::from_secs(5))
        .await
        .expect("custody is free once the predecessor is proven gone");
    assert!(
        group_is_gone(child_pid),
        "custody was handed over while the predecessor's process group was still alive"
    );
    let at_handover = marker.metadata().map(|meta| meta.len()).unwrap_or_default();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let later = marker.metadata().map(|meta| meta.len()).unwrap_or_default();
    assert_eq!(
        later, at_handover,
        "something the predecessor spawned is still writing the seat's tree after the successor \
         took custody"
    );
    drop(successor);
}

/// Whether nothing of `pid`'s process group is left.
#[cfg(unix)]
fn group_is_gone(pid: u32) -> bool {
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    matches!(
        killpg(Pid::from_raw(i32::try_from(pid).unwrap_or(i32::MAX)), None),
        Err(nix::errno::Errno::ESRCH)
    )
}

/// A fenced seat is refused to **everything**, which is the honest answer
/// when a predecessor could not be shown to be dead: a stuck seat that says
/// so beats two executions sharing one tree (ledger 227, A8.1 amendment).
#[tokio::test]
async fn a_fenced_seat_refuses_every_turn_and_every_establishment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let seat = dir.path().join("checkout");
    std::fs::create_dir_all(&seat).expect("mkdir");
    let (first, second) = seat_repo(&seat);
    let session_id = format!("seat-{}", Uuid::new_v4());
    let assignment = "78".repeat(32);
    let store = queued_store(&state, dir.path(), &seat, &assignment, &second);

    crate::assignment_custody::fence_seat(&session_id);
    assert!(crate::assignment_custody::is_fenced(&session_id));
    assert!(
        crate::assignment_custody::hold_for_turn(&session_id, Duration::from_millis(50))
            .await
            .is_none(),
        "no turn takes a fenced seat"
    );
    let outcome = crate::assignment_custody::establish_for_turn(
        &session_id,
        &crate::assignment_custody::TurnRequirement {
            assignment_ref: assignment.clone(),
            base_sha: second.clone(),
            cwd: seat.clone(),
            store,
            branch: "seat/verifier".to_owned(),
        },
        Duration::from_millis(50),
    )
    .await;
    assert!(
        matches!(outcome, crate::assignment_custody::Establishment::SeatBusy),
        "and no establishment either: {outcome:?}"
    );
    assert_eq!(head_of(&seat), first, "nothing moved");

    // The fence lifts only when somebody proves the predecessor is gone.
    crate::assignment_custody::clear_seat_fence(&session_id);
    assert!(
        crate::assignment_custody::hold_for_turn(&session_id, Duration::from_millis(50))
            .await
            .is_some()
    );
}
