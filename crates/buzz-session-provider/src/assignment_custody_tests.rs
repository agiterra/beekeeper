//! Custody tests: who may move a seat's tree, and when.
//!
//! Every case here builds real repositories in a temporary directory and
//! asserts on what git says afterwards. Timing is controlled by holding the
//! custody token — the same token a running turn holds — rather than by
//! sleeping and hoping.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::assignment_inputs::{
    assignment_input, queue_intent, record_assignment_input, AssignmentInputRecord,
    AssignmentIntent, ASSIGNMENT_INPUT_ESTABLISHED, ASSIGNMENT_INPUT_ESTABLISHING,
};

const BOUND: Duration = Duration::from_secs(30);

fn run_git(dir: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(dir);
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    command.env("GIT_AUTHOR_NAME", "Test");
    command.env("GIT_AUTHOR_EMAIL", "test@example.com");
    command.env("GIT_COMMITTER_NAME", "Test");
    command.env("GIT_COMMITTER_EMAIL", "test@example.com");
    let output = command.output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn head_of(dir: &Path) -> String {
    run_git(dir, &["rev-parse", "HEAD"])
}

/// A seat repository with three commits on one branch, oldest first.
fn seat_with_three_commits(seat: &Path) -> (String, String, String) {
    std::fs::create_dir_all(seat).expect("dir");
    run_git(seat, &["init", "--quiet", "--initial-branch=main", "."]);
    std::fs::write(seat.join("a.txt"), "a\n").expect("write");
    run_git(seat, &["add", "a.txt"]);
    run_git(seat, &["commit", "--quiet", "-m", "a"]);
    let first = head_of(seat);
    std::fs::write(seat.join("b.txt"), "b\n").expect("write");
    run_git(seat, &["add", "b.txt"]);
    run_git(seat, &["commit", "--quiet", "-m", "b"]);
    let second = head_of(seat);
    std::fs::write(seat.join("c.txt"), "c\n").expect("write");
    run_git(seat, &["add", "c.txt"]);
    run_git(seat, &["commit", "--quiet", "-m", "c"]);
    let third = head_of(seat);
    run_git(
        seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );
    (first, second, third)
}

fn store_at(root: &Path) -> AssignmentInputStore {
    let path = root.join("coding-session-workdirs.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 2,
            "worktrees": {},
            "assignmentInputs": {}
        }))
        .expect("serialize"),
    )
    .expect("write");
    AssignmentInputStore::new(path)
}

fn requirement(
    store: &AssignmentInputStore,
    seat: &Path,
    assignment_ref: &str,
    base_sha: &str,
) -> TurnRequirement {
    TurnRequirement {
        assignment_ref: assignment_ref.to_owned(),
        base_sha: base_sha.to_owned(),
        cwd: seat.to_path_buf(),
        store: store.clone(),
        branch: "seat/verifier".to_owned(),
    }
}

/// Queue the intent the provider would have recorded before asking for an
/// establishment.
fn queue(store: &AssignmentInputStore, assignment_ref: &str, base_sha: &str, seat: &Path) {
    store
        .with_records(|records, persist| {
            queue_intent(
                records,
                &AssignmentIntent {
                    assignment_id: assignment_ref.to_owned(),
                    session_ref: "session-1".to_owned(),
                    seat_label: None,
                    base_sha: base_sha.to_owned(),
                    branch: None,
                },
                Some(&SeatCheckout {
                    path: seat.to_path_buf(),
                    branch: "seat/verifier".to_owned(),
                }),
            );
            persist(records)
        })
        .expect("lock and load")
        .expect("save");
}

fn assignment(byte: &str) -> String {
    byte.repeat(32)
}

/// Review finding 2, the whole of it: a verifier is running on A; two more
/// assignments arrive. The running turn's tree never changes, B runs on B, and
/// C runs on C.
#[tokio::test]
async fn two_assignments_behind_a_running_verifier_each_run_on_their_own_commit() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    let (first, second, third) = seat_with_three_commits(&seat);
    let store = store_at(root.path());
    let session = format!("seat-{}", uuid::Uuid::new_v4());
    let (b, c) = (assignment("b1"), assignment("c1"));
    queue(&store, &b, &second, &seat);
    queue(&store, &c, &third, &seat);

    // A's turn is running: the actor holds custody for the length of the turn.
    let running = hold(&session).await;
    assert_eq!(head_of(&seat), first);

    // Both later assignments are told the seat is busy, and neither touches
    // the tree. Before this lane both ran `git checkout -B` immediately.
    for (assignment_ref, commit) in [(&b, &second), (&c, &third)] {
        let outcome = establish_for_turn(
            &session,
            &requirement(&store, &seat, assignment_ref, commit),
            BOUND,
        )
        .await;
        assert!(
            matches!(outcome, Establishment::SeatBusy),
            "a running turn's tree is nobody else's to move: {outcome:?}"
        );
    }
    assert_eq!(
        head_of(&seat),
        first,
        "the running verifier's tree must not have changed"
    );

    // A's turn ends. The oldest deferred assignment is prepared first.
    drop(running);
    let settled =
        establish_for_turn(&session, &requirement(&store, &seat, &b, &second), BOUND).await;
    assert!(matches!(settled, Establishment::Settled(_)), "{settled:?}");
    assert_eq!(head_of(&seat), second);

    // B's turn dequeues: it re-verifies under custody and keeps it.
    remember_requirement(&session, "wake-b", requirement(&store, &seat, &b, &second));
    let custody = verify_for_turn(&session, "wake-b").await;
    let TurnCustody::Held(guard) = custody else {
        panic!("B must run on B: {custody:?}");
    };

    // C is prepared while B runs: refused, and B's tree is untouched.
    let while_b_runs =
        establish_for_turn(&session, &requirement(&store, &seat, &c, &third), BOUND).await;
    assert!(
        matches!(while_b_runs, Establishment::SeatBusy),
        "{while_b_runs:?}"
    );
    assert_eq!(head_of(&seat), second, "B runs on B for the whole turn");

    // B's turn ends; C is established and C's turn verifies against C.
    drop(guard);
    let settled =
        establish_for_turn(&session, &requirement(&store, &seat, &c, &third), BOUND).await;
    assert!(matches!(settled, Establishment::Settled(_)), "{settled:?}");
    remember_requirement(&session, "wake-c", requirement(&store, &seat, &c, &third));
    assert!(
        matches!(
            verify_for_turn(&session, "wake-c").await,
            TurnCustody::Held(_)
        ),
        "C must run on C"
    );
    assert_eq!(head_of(&seat), third);
}

/// The last line: if something did move the tree between the provider's
/// decision and the prompt, the dequeue refuses rather than prompting a
/// verifier about a commit it is not on.
#[tokio::test]
async fn a_tree_that_moved_after_the_decision_refuses_at_the_dequeue() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    let (first, second, _third) = seat_with_three_commits(&seat);
    let store = store_at(root.path());
    let session = format!("seat-{}", uuid::Uuid::new_v4());
    let b = assignment("b2");
    queue(&store, &b, &second, &seat);
    establish_for_turn(&session, &requirement(&store, &seat, &b, &second), BOUND).await;
    assert_eq!(head_of(&seat), second);

    // Something outside this provider moves the tree back.
    run_git(
        &seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );
    remember_requirement(&session, "wake-b", requirement(&store, &seat, &b, &second));
    let custody = verify_for_turn(&session, "wake-b").await;
    let TurnCustody::Refused {
        observed, required, ..
    } = custody
    else {
        panic!("a moved tree must not be prompted: {custody:?}");
    };
    assert_eq!(observed.as_deref(), Some(first.as_str()));
    assert_eq!(required, second);
    let record = store
        .with_records(|records, _| assignment_input(records, &b).cloned())
        .expect("read");
    assert_eq!(
        record.expect("record").outcome,
        crate::assignment_inputs::ASSIGNMENT_INPUT_MOVED,
        "the reason is durable, so whoever publishes blockers can find it"
    );
}

/// A turn that was never an assignment turn is untouched by any of this.
#[tokio::test]
async fn a_turn_with_no_recorded_requirement_needs_no_custody() {
    let session = format!("seat-{}", uuid::Uuid::new_v4());
    assert!(matches!(
        verify_for_turn(&session, "ordinary-prose").await,
        TurnCustody::NotRequired
    ));
}

/// Review finding 3: a waiter that gives up does not make the record
/// replayable. The attempt is barrier-controlled — it cannot finish until the
/// test lets go of the seat — so the timeout is real rather than timed.
#[tokio::test]
async fn a_timed_out_waiter_starts_no_second_attempt_and_one_git_task_runs() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    let (_first, second, _third) = seat_with_three_commits(&seat);
    let store = store_at(root.path());
    let session = format!("seat-{}", uuid::Uuid::new_v4());
    let b = assignment("b3");
    queue(&store, &b, &second, &seat);

    // The barrier: a turn holds custody, so the attempt spawned below cannot
    // reach git until it is released.
    let running = hold(&session).await;
    // Start an attempt anyway, the way a seat that went idle a moment later
    // would, and give the waiter no time at all.
    let started = tokio::spawn({
        let store = store.clone();
        let seat = seat.clone();
        let session = session.clone();
        let b = b.clone();
        let second = second.clone();
        async move {
            // A deliberately tiny bound: the attempt will still be waiting for
            // custody when this returns.
            establish_for_turn(
                &session,
                &requirement(&store, &seat, &b, &second),
                Duration::from_millis(1),
            )
            .await
        }
    });
    // Let the seat go idle so the attempt begins, then give up waiting.
    drop(running);
    let first_answer = started.await.expect("join");
    assert!(
        matches!(
            first_answer,
            Establishment::InFlight(_) | Establishment::Settled(_) | Establishment::SeatBusy
        ),
        "{first_answer:?}"
    );

    // Redelivery while the attempt may still be live: it joins, never starts a
    // second. The attempt count proves it.
    let second_answer =
        establish_for_turn(&session, &requirement(&store, &seat, &b, &second), BOUND).await;
    assert!(
        matches!(second_answer, Establishment::Settled(_)),
        "{second_answer:?}"
    );
    let record = store
        .with_records(|records, _| assignment_input(records, &b).cloned())
        .expect("read")
        .expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ESTABLISHED);
    assert_eq!(
        record.attempts, 1,
        "a redelivery joins the live attempt; it does not start another"
    );
    assert_eq!(head_of(&seat), second);
}

/// Two assignments for one seat serialize: whichever runs second finds the
/// tree the first left, and neither interleaves with the other's git.
#[tokio::test]
async fn overlapping_assignments_for_one_seat_are_serialised() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    let (_first, second, third) = seat_with_three_commits(&seat);
    let store = store_at(root.path());
    let session = format!("seat-{}", uuid::Uuid::new_v4());
    let (b, c) = (assignment("b4"), assignment("c4"));
    queue(&store, &b, &second, &seat);
    queue(&store, &c, &third, &seat);

    let one = tokio::spawn({
        let (store, seat, session, b, second) = (
            store.clone(),
            seat.clone(),
            session.clone(),
            b.clone(),
            second.clone(),
        );
        async move {
            establish_for_turn(&session, &requirement(&store, &seat, &b, &second), BOUND).await
        }
    });
    let two = tokio::spawn({
        let (store, seat, session, c, third) = (
            store.clone(),
            seat.clone(),
            session.clone(),
            c.clone(),
            third.clone(),
        );
        async move { establish_for_turn(&session, &requirement(&store, &seat, &c, &third), BOUND).await }
    });
    let (one, two) = (one.await.expect("join"), two.await.expect("join"));
    for outcome in [&one, &two] {
        assert!(
            matches!(
                outcome,
                Establishment::Settled(_) | Establishment::SeatBusy | Establishment::InFlight(_)
            ),
            "{outcome:?}"
        );
    }
    // Whatever order they took, no record is left mid-attempt and the tree
    // holds one of the two commits — never a half-applied checkout.
    let head = head_of(&seat);
    assert!(head == second || head == third, "{head}");
    for assignment_ref in [&b, &c] {
        if let Some(record) = store
            .with_records(|records, _| assignment_input(records, assignment_ref).cloned())
            .expect("read")
        {
            assert_ne!(
                record.outcome, ASSIGNMENT_INPUT_ESTABLISHING,
                "no attempt is left running"
            );
            assert!(record.attempts <= 1, "one attempt each at most");
        }
    }
}

/// A terminal record whose write failed is written on the next pass, and the
/// git work is not repeated.
#[tokio::test]
async fn an_unrecorded_terminal_is_written_by_the_next_pass() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    let (_first, second, _third) = seat_with_three_commits(&seat);
    let path = root.path().join("coding-session-workdirs.json");
    let store = store_at(root.path());
    let session = format!("seat-{}", uuid::Uuid::new_v4());
    let b = assignment("b5");
    queue(&store, &b, &second, &seat);

    let failing = AssignmentInputStore::new(&path).with_writer(Arc::new(
        move |path: &Path, payload: &[u8]| {
            if String::from_utf8_lossy(payload).contains(ASSIGNMENT_INPUT_ESTABLISHED) {
                return Err("the disk is full".to_owned());
            }
            std::fs::write(path, payload).map_err(|error| error.to_string())
        },
    ));
    let mut failing_requirement = requirement(&store, &seat, &b, &second);
    failing_requirement.store = failing;
    let outcome = establish_for_turn(&session, &failing_requirement, BOUND).await;
    assert!(
        matches!(outcome, Establishment::Unstarted(_)),
        "an unsaved record is reported, never reported as success: {outcome:?}"
    );
    assert_eq!(head_of(&seat), second, "the effect happened");

    // The next pass, with a working writer: the record is written and the
    // attempt count shows no second checkout.
    let settled =
        establish_for_turn(&session, &requirement(&store, &seat, &b, &second), BOUND).await;
    assert!(matches!(settled, Establishment::Settled(_)), "{settled:?}");
    let record = store
        .with_records(|records, _| assignment_input(records, &b).cloned())
        .expect("read")
        .expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ESTABLISHED);
    assert_eq!(record.attempts, 1, "the retry redid no git work");
}

/// A record left `establishing` by a process that is gone is the interrupted
/// case: this process knows no such owner, so the one-replay rule applies.
#[tokio::test]
async fn an_establishing_record_from_a_dead_process_is_replayed_once() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    let (_first, second, _third) = seat_with_three_commits(&seat);
    let store = store_at(root.path());
    let session = format!("seat-{}", uuid::Uuid::new_v4());
    let b = assignment("b6");
    queue(&store, &b, &second, &seat);
    store
        .with_records(|records, persist| {
            let mut interrupted = assignment_input(records, &b).cloned().expect("record");
            interrupted.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_owned();
            interrupted.attempts = 1;
            interrupted.attempt_owner = Some("a-process-that-is-gone:1".to_owned());
            record_assignment_input(records, interrupted);
            persist(records)
        })
        .expect("lock")
        .expect("save");

    assert!(
        !attempt_is_live("a-process-that-is-gone:1"),
        "nothing from a previous run is live here"
    );
    let settled =
        establish_for_turn(&session, &requirement(&store, &seat, &b, &second), BOUND).await;
    assert!(matches!(settled, Establishment::Settled(_)), "{settled:?}");
    let record = store
        .with_records(|records, _| assignment_input(records, &b).cloned())
        .expect("read")
        .expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ESTABLISHED);
    assert_eq!(record.attempts, 2, "the interrupted attempt replayed once");
    assert_eq!(record.attempt_owner, None, "a settled row owns nothing");
}

/// A record whose intent never mentioned a tree is not this module's to move.
#[tokio::test]
async fn an_assignment_with_no_record_is_unstarted_rather_than_guessed_at() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    let (_first, second, _third) = seat_with_three_commits(&seat);
    let store = store_at(root.path());
    let session = format!("seat-{}", uuid::Uuid::new_v4());
    let outcome = establish_for_turn(
        &session,
        &requirement(&store, &seat, &assignment("b7"), &second),
        BOUND,
    )
    .await;
    assert!(
        matches!(outcome, Establishment::Unstarted(_)),
        "{outcome:?}"
    );
    let _ = AssignmentInputRecord::intended(
        &AssignmentIntent {
            assignment_id: assignment("b7"),
            session_ref: "session-1".to_owned(),
            seat_label: None,
            base_sha: second,
            branch: None,
        },
        None,
    );
}
