//! Assignment-input tests against real git repositories in temporary
//! directories.
//!
//! Never a worktree of this repository: every fixture here is a repository
//! this test created, so a failing case can only ever move a tree the test
//! owns.

use std::path::{Path, PathBuf};

use super::*;

/// The environment variable that turns [`two_process_lock_child`] from a
/// no-op into the second process of the lock test.
const LOCK_CHILD_STORE: &str = "BUZZ_TEST_ASSIGNMENT_INPUT_LOCK_CHILD_STORE";

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

/// A repository with one commit on `main`, and the commit's id.
fn repo_with_one_commit(path: &Path) -> String {
    std::fs::create_dir_all(path).expect("dir");
    run_git(path, &["init", "--quiet", "--initial-branch=main", "."]);
    std::fs::write(path.join("README.md"), "first\n").expect("write");
    run_git(path, &["add", "README.md"]);
    run_git(path, &["commit", "--quiet", "-m", "first"]);
    run_git(path, &["rev-parse", "HEAD"])
}

/// A second commit on the same branch, and its id.
fn add_commit(path: &Path, name: &str) -> String {
    std::fs::write(path.join(name), "next\n").expect("write");
    run_git(path, &["add", name]);
    run_git(path, &["commit", "--quiet", "-m", name]);
    run_git(path, &["rev-parse", "HEAD"])
}

/// A seat tree cloned from `origin`, on its own branch.
fn seat_clone(origin: &Path, seat: &Path, branch: &str) -> SeatCheckout {
    run_git(
        origin.parent().expect("parent"),
        &[
            "clone",
            "--quiet",
            &origin.to_string_lossy(),
            &seat.to_string_lossy(),
        ],
    );
    run_git(seat, &["checkout", "--quiet", "-B", branch]);
    SeatCheckout {
        path: seat.to_path_buf(),
        branch: branch.to_owned(),
    }
}

fn intent(assignment_id: &str, base_sha: &str) -> AssignmentIntent {
    AssignmentIntent {
        assignment_id: assignment_id.to_owned(),
        session_ref: "session-1".to_owned(),
        seat_label: Some("verifier".to_owned()),
        base_sha: base_sha.to_owned(),
        branch: None,
    }
}

fn assignment_id(byte: &str) -> String {
    byte.repeat(32)
}

fn head_of(path: &Path) -> String {
    run_git(path, &["rev-parse", "HEAD"])
}

/// The `assignmentInputs` object as it stands on disk.
fn records_of(store: &Path) -> serde_json::Value {
    serde_json::from_slice::<serde_json::Value>(&std::fs::read(store).expect("read")).expect("json")
        ["assignmentInputs"]
        .clone()
}

fn drain(records: &mut AssignmentInputRecords, checkout: &SeatCheckout) -> Vec<String> {
    let checkout = checkout.clone();
    drain_pending_assignment_inputs(records, move |_| Some(checkout.clone()), |_| Ok(()))
}

#[test]
fn a_queued_intent_is_established_and_the_tree_is_measured_by_git() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let second = add_commit(&origin, "second.txt");
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");
    run_git(
        &seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );
    assert_eq!(head_of(&seat), first);

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("a1");
    assert_eq!(
        queue_intent(&mut records, &intent(&id, &second), Some(&checkout)),
        AssignmentInputDisposition::Recorded
    );
    assert_eq!(drain(&mut records, &checkout), vec![id.clone()]);

    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ESTABLISHED);
    assert_eq!(record.attempts, 1);
    assert_eq!(
        head_of(&seat),
        second,
        "the tree must hold the named commit"
    );
}

#[test]
fn a_terminal_outcome_is_never_drained_a_second_time() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let second = add_commit(&origin, "second.txt");
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");
    run_git(
        &seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("b2");
    queue_intent(&mut records, &intent(&id, &second), Some(&checkout));
    drain(&mut records, &checkout);
    // A second drain finds nothing pending, so nothing is attempted again.
    assert!(drain(&mut records, &checkout).is_empty());
    assert_eq!(
        assignment_input(&records, &id).expect("record").attempts,
        1,
        "a settled assignment must not start another attempt"
    );
}

#[test]
fn a_dirty_tree_is_preserved_refused_and_left_alone() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let second = add_commit(&origin, "second.txt");
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");
    run_git(
        &seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );
    std::fs::write(seat.join("uncommitted.txt"), "work nobody else has\n").expect("write");

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("c3");
    queue_intent(&mut records, &intent(&id, &second), Some(&checkout));
    drain(&mut records, &checkout);

    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(
        record.outcome,
        EstablishAssignmentInputCode::DirtyTree.as_str()
    );
    assert_eq!(record.changes, Some(1));
    assert_eq!(head_of(&seat), first, "the tree must not have moved");
    assert_eq!(
        std::fs::read_to_string(seat.join("uncommitted.txt")).expect("read"),
        "work nobody else has\n",
        "uncommitted work is the one thing nobody else has a copy of"
    );
    // And it is never retried on its own.
    assert!(drain(&mut records, &checkout).is_empty());
}

#[test]
fn an_interrupted_attempt_replays_once_and_is_then_abandoned() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let second = add_commit(&origin, "second.txt");
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");
    run_git(
        &seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("d4");
    queue_intent(&mut records, &intent(&id, &second), Some(&checkout));
    // What a quit mid-checkout leaves behind: the count is on the disk, the
    // outcome is still `establishing`.
    let mut interrupted = assignment_input(&records, &id).expect("record").clone();
    interrupted.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_owned();
    interrupted.attempts = 1;
    record_assignment_input(&mut records, interrupted);

    // The replay finishes it.
    drain(&mut records, &checkout);
    assert_eq!(
        assignment_input(&records, &id).expect("record").outcome,
        ASSIGNMENT_INPUT_ESTABLISHED
    );

    // A second interruption at the bound is abandoned rather than replayed.
    let mut twice = assignment_input(&records, &id).expect("record").clone();
    twice.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_owned();
    twice.attempts = MAX_ESTABLISH_ATTEMPTS;
    record_assignment_input(&mut records, twice);
    drain(&mut records, &checkout);
    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ABANDONED);
    assert!(
        record
            .message
            .as_deref()
            .is_some_and(|message| message.contains("none finished")),
        "the abandonment says why in words: {:?}",
        record.message
    );
}

#[test]
fn a_commit_absent_from_the_clone_is_fetched_from_the_resolved_remote() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    repo_with_one_commit(&origin);
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/runner");
    // Published after the clone, so the seat's repository has never seen it.
    let later = add_commit(&origin, "later.txt");

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("e5");
    queue_intent(&mut records, &intent(&id, &later), Some(&checkout));
    drain(&mut records, &checkout);

    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ESTABLISHED);
    assert_eq!(
        record.remote.as_deref(),
        Some("origin"),
        "the remote is the one git config resolved, never a constant"
    );
    assert_eq!(head_of(&seat), later);
}

#[test]
fn a_commit_no_remote_has_is_refused_by_name_after_the_fetch() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/runner");
    let absent = "0".repeat(39) + "1";

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("f6");
    queue_intent(&mut records, &intent(&id, &absent), Some(&checkout));
    drain(&mut records, &checkout);

    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(
        record.outcome,
        EstablishAssignmentInputCode::UnknownCommit.as_str(),
        "a commit the remote does not have is `unknown_commit`, not `fetch_failed`"
    );
    assert_eq!(head_of(&seat), first, "nothing moved");
}

#[test]
fn an_assignment_with_no_tree_is_off_host_and_queues_nothing() {
    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("07");
    assert_eq!(
        queue_intent(&mut records, &intent(&id, &"a".repeat(40)), None),
        AssignmentInputDisposition::OffHost
    );
    assert!(records.is_empty());
}

#[test]
fn an_unreadable_assignment_or_commit_queues_nothing_and_says_so() {
    let mut records = AssignmentInputRecords::new();
    assert_eq!(
        queue_intent(&mut records, &intent("not-an-id", &"a".repeat(40)), None),
        AssignmentInputDisposition::Invalid
    );
    let id = assignment_id("18");
    assert_eq!(
        queue_intent(&mut records, &intent(&id, "HEAD~1"), None),
        AssignmentInputDisposition::Invalid
    );
    assert_eq!(
        queue_intent(&mut records, &intent(&id, "   "), None),
        AssignmentInputDisposition::Unnamed
    );
    assert!(records.is_empty(), "nothing was queued: {records:?}");
}

#[test]
fn a_record_for_another_commit_is_replaced_and_the_same_one_is_dispositive() {
    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("29");
    let first = "a".repeat(40);
    let second = "b".repeat(40);
    let checkout = SeatCheckout {
        path: PathBuf::from("/nowhere"),
        branch: "seat/verifier".to_owned(),
    };
    queue_intent(&mut records, &intent(&id, &first), Some(&checkout));
    let mut settled = assignment_input(&records, &id).expect("record").clone();
    settled.outcome = EstablishAssignmentInputCode::DirtyTree.as_str().to_owned();
    settled.attempts = 1;
    record_assignment_input(&mut records, settled);

    // The same commit is an answer about this assignment; nothing is requeued.
    assert_eq!(
        queue_intent(&mut records, &intent(&id, &first), Some(&checkout)),
        AssignmentInputDisposition::Recorded
    );
    assert_eq!(
        assignment_input(&records, &id).expect("record").outcome,
        EstablishAssignmentInputCode::DirtyTree.as_str()
    );

    // A different commit is no answer about this one.
    queue_intent(&mut records, &intent(&id, &second), Some(&checkout));
    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_INTENDED);
    assert_eq!(record.attempts, 0);
    assert_eq!(record.commit.as_deref(), Some(second.as_str()));
}

#[test]
fn a_dispositive_record_answers_even_without_a_tree_on_this_host() {
    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("5c");
    let commit = "c".repeat(40);
    let checkout = SeatCheckout {
        path: PathBuf::from("/nowhere"),
        branch: "seat/verifier".to_owned(),
    };
    queue_intent(&mut records, &intent(&id, &commit), Some(&checkout));
    let mut settled = assignment_input(&records, &id).expect("record").clone();
    settled.outcome = ASSIGNMENT_INPUT_ESTABLISHED.to_owned();
    record_assignment_input(&mut records, settled);
    let before = records.clone();

    // This host did not cut the tree (checkout: None), but a record already
    // names this exact commit — that record is dispositive and answers the
    // question; it is not `OffHost`, and nothing is requeued or touched.
    assert_eq!(
        queue_intent(&mut records, &intent(&id, &commit), None),
        AssignmentInputDisposition::Recorded
    );
    assert_eq!(records, before, "a dispositive record is left untouched");
}

#[test]
fn a_requeue_zeroes_the_count_and_forgets_the_published_blocker() {
    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("3a");
    let checkout = SeatCheckout {
        path: PathBuf::from("/nowhere"),
        branch: "seat/verifier".to_owned(),
    };
    queue_intent(&mut records, &intent(&id, &"a".repeat(40)), Some(&checkout));
    let mut settled = assignment_input(&records, &id).expect("record").clone();
    settled.outcome = ASSIGNMENT_INPUT_ABANDONED.to_owned();
    settled.attempts = MAX_ESTABLISH_ATTEMPTS;
    settled.blocker_published = true;
    record_assignment_input(&mut records, settled);

    assert!(requeue_assignment_input(&mut records, &id));
    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_INTENDED);
    assert_eq!(record.attempts, 0);
    assert!(!record.blocker_published);
    assert!(
        !requeue_assignment_input(&mut records, &assignment_id("4b")),
        "an assignment with no record answers no, not an empty success"
    );
}

#[test]
fn the_outcome_words_are_pinned_against_serde() {
    for (code, word) in [
        (
            EstablishAssignmentInputCode::UnrecordedTree,
            "unrecorded_tree",
        ),
        (EstablishAssignmentInputCode::MissingTree, "missing_tree"),
        (EstablishAssignmentInputCode::DirtyTree, "dirty_tree"),
        (EstablishAssignmentInputCode::NoRemote, "no_remote"),
        (
            EstablishAssignmentInputCode::AmbiguousRemote,
            "ambiguous_remote",
        ),
        (EstablishAssignmentInputCode::FetchFailed, "fetch_failed"),
        (
            EstablishAssignmentInputCode::UnknownCommit,
            "unknown_commit",
        ),
        (
            EstablishAssignmentInputCode::CheckoutFailed,
            "checkout_failed",
        ),
        (EstablishAssignmentInputCode::InvalidInput, "invalid_input"),
    ] {
        assert_eq!(code.as_str(), word);
        assert_eq!(
            serde_json::to_value(code).expect("json"),
            serde_json::Value::String(word.to_owned()),
            "the record's word and the response's word are one word"
        );
    }
}

/// A store document shaped like the desktop's own file, with keys this module
/// deliberately does not model.
fn desktop_shaped_store(path: &Path) {
    let document = serde_json::json!({
        "version": 2,
        "byProject": {},
        "byChannel": {},
        "mru": [],
        "pending": {},
        "worktrees": {
            "session-1/verifier": {
                "path": "/seats/verifier",
                "branch": "seat/verifier",
                "repoRoot": "/repo",
                "createdAt": "2026-09-20T00:00:00Z"
            }
        },
        "worktreeParents": {},
        "prunes": [],
        "assignmentInputs": {}
    });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&document).expect("serialize"),
    )
    .expect("write");
}

#[test]
fn a_missing_store_is_refused_rather_than_invented() {
    let root = tempfile::tempdir().expect("temp");
    let store = AssignmentInputStore::new(root.path().join("coding-session-workdirs.json"));
    let error = store
        .load()
        .expect_err("a missing store is not an empty one");
    assert!(
        error.contains("failed to read the host workdir store"),
        "{error}"
    );
    assert!(
        !root.path().join("coding-session-workdirs.json").exists(),
        "reading must not create the desktop's file"
    );
}

#[test]
fn a_write_from_this_module_carries_every_key_it_does_not_model() {
    let root = tempfile::tempdir().expect("temp");
    let path = root.path().join("coding-session-workdirs.json");
    desktop_shaped_store(&path);
    let store = AssignmentInputStore::new(&path);
    store
        .with_records(|records, _| {
            record_assignment_input(
                records,
                AssignmentInputRecord::intended(
                    &intent(&assignment_id("5c"), &"a".repeat(40)),
                    None,
                ),
            );
        })
        .expect("mutation");

    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    assert_eq!(saved["version"], 2);
    assert_eq!(
        saved["worktrees"]["session-1/verifier"]["branch"],
        "seat/verifier"
    );
    assert!(saved["assignmentInputs"]
        .as_object()
        .is_some_and(|inputs| inputs.contains_key(&assignment_id("5c"))));
}

#[test]
fn two_writers_serialise_and_neither_clobbers_the_others_keys() {
    let root = tempfile::tempdir().expect("temp");
    let path = root.path().join("coding-session-workdirs.json");
    desktop_shaped_store(&path);

    // One writer is this module; the other writes the file the way the
    // desktop does — the whole document, including keys this module models as
    // `rest`. Each takes the same lock on its own open file description,
    // which is what two processes get.
    let workers: Vec<_> = (0..8)
        .map(|index| {
            let path = path.clone();
            std::thread::spawn(move || {
                if index % 2 == 0 {
                    let store = AssignmentInputStore::new(&path);
                    store
                        .with_records(|records, _| {
                            record_assignment_input(
                                records,
                                AssignmentInputRecord::intended(
                                    &intent(
                                        &assignment_id(&format!("{index}{index}")),
                                        &"a".repeat(40),
                                    ),
                                    None,
                                ),
                            );
                        })
                        .expect("provider mutation");
                } else {
                    let _lock = lock_store_file(&path).expect("lock");
                    let mut document: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
                    std::thread::yield_now();
                    document["pending"][format!("command-{index}")] =
                        serde_json::json!(format!("/drafts/{index}"));
                    std::fs::write(
                        &path,
                        serde_json::to_vec_pretty(&document).expect("serialize"),
                    )
                    .expect("write");
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker");
    }

    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    assert_eq!(
        saved["assignmentInputs"].as_object().expect("object").len(),
        4,
        "every provider write survived"
    );
    assert_eq!(
        saved["pending"].as_object().expect("object").len(),
        4,
        "every desktop write survived"
    );
    assert_eq!(saved["version"], 2);
}

/// The second process of [`a_second_process_waits_for_the_lock`].
///
/// An ordinary test that does nothing at all unless the parent set
/// [`LOCK_CHILD_STORE`], which is how it can be spawned by name from the same
/// test binary without needing a second executable.
#[test]
fn two_process_lock_child() {
    let Ok(path) = std::env::var(LOCK_CHILD_STORE) else {
        return;
    };
    let path = PathBuf::from(path);
    let _lock = lock_store_file(&path).expect("lock");
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    let parent_was_first = document["assignmentInputs"]
        .as_object()
        .is_some_and(|inputs| inputs.contains_key(&assignment_id("aa")));
    document["childSawParent"] = serde_json::json!(parent_was_first);
    document["pending"]["child"] = serde_json::json!("/drafts/child");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&document).expect("serialize"),
    )
    .expect("write");
}

#[test]
fn a_second_process_waits_for_the_lock_and_keeps_what_it_finds() {
    if std::env::var(LOCK_CHILD_STORE).is_ok() {
        // This process *is* a child; it must not spawn another.
        return;
    }
    let root = tempfile::tempdir().expect("temp");
    let path = root.path().join("coding-session-workdirs.json");
    desktop_shaped_store(&path);

    let executable = std::env::current_exe().expect("test binary");
    let lock = lock_store_file(&path).expect("lock");
    let mut child = std::process::Command::new(executable)
        .args([
            "--exact",
            "assignment_inputs::tests::two_process_lock_child",
            "--nocapture",
        ])
        .env(LOCK_CHILD_STORE, &path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the second process");

    // The child is blocked on the lock this process holds. Write under it,
    // then release.
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(
        child.try_wait().expect("child status").is_none(),
        "the second process must still be waiting on the lock"
    );
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    document["assignmentInputs"][assignment_id("aa")] = serde_json::json!({
        "assignmentId": assignment_id("aa"),
        "sessionRef": "session-1",
        "seatLabel": "verifier",
        "commit": "a".repeat(40),
        "branch": null,
        "path": null,
        "remote": null,
        "outcome": ASSIGNMENT_INPUT_INTENDED,
        "message": null,
        "recordedAt": "2026-09-20T00:00:00Z"
    });
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&document).expect("serialize"),
    )
    .expect("write");
    drop(lock);

    let status = child.wait().expect("child finishes");
    assert!(status.success(), "the second process failed: {status}");

    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    assert_eq!(
        saved["childSawParent"],
        serde_json::json!(true),
        "the second process read the first one's write, so the lock serialised them"
    );
    assert_eq!(saved["pending"]["child"], "/drafts/child");
    assert_eq!(saved["version"], 2);
}

#[test]
fn a_commit_already_checked_out_is_reported_already_current() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("6d");
    queue_intent(&mut records, &intent(&id, &first), Some(&checkout));
    drain(&mut records, &checkout);
    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ALREADY_CURRENT);
    assert_eq!(
        record.remote, None,
        "a commit already in the repository names no means that was never used"
    );
    assert_eq!(head_of(&seat), first);
}

#[test]
fn a_tree_the_caller_cannot_place_is_refused_as_unrecorded() {
    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("7e");
    let commit = "a".repeat(40);
    record_assignment_input(
        &mut records,
        AssignmentInputRecord::intended(&intent(&id, &commit), None),
    );
    let refusal = establish(
        &mut records,
        &EstablishAssignmentInputRequest {
            assignment_id: id.clone(),
            ..EstablishAssignmentInputRequest::default()
        },
        None,
    )
    .expect_err("no tree, no establishment");
    assert_eq!(refusal.code, EstablishAssignmentInputCode::UnrecordedTree);
    assert_eq!(
        assignment_input(&records, &id).expect("record").outcome,
        EstablishAssignmentInputCode::UnrecordedTree.as_str()
    );
}

#[test]
fn a_recorded_path_that_is_gone_is_refused_as_missing_tree() {
    let root = tempfile::tempdir().expect("temp");
    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("8f");
    let checkout = SeatCheckout {
        path: root.path().join("never-cut"),
        branch: "seat/verifier".to_owned(),
    };
    queue_intent(&mut records, &intent(&id, &"a".repeat(40)), Some(&checkout));
    drain(&mut records, &checkout);
    assert_eq!(
        assignment_input(&records, &id).expect("record").outcome,
        EstablishAssignmentInputCode::MissingTree.as_str()
    );
}

#[test]
fn a_repository_with_no_remote_refuses_by_name_when_the_object_is_missing() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    repo_with_one_commit(&seat);
    let checkout = SeatCheckout {
        path: seat.clone(),
        branch: "main".to_owned(),
    };

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("90");
    queue_intent(&mut records, &intent(&id, &"a".repeat(40)), Some(&checkout));
    drain(&mut records, &checkout);
    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(
        record.outcome,
        EstablishAssignmentInputCode::NoRemote.as_str()
    );
}

#[test]
fn several_remotes_with_no_config_answer_are_ambiguous() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    repo_with_one_commit(&origin);
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/runner");
    run_git(&seat, &["remote", "add", "elsewhere", "../origin"]);

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("a2");
    queue_intent(&mut records, &intent(&id, &"a".repeat(40)), Some(&checkout));
    drain(&mut records, &checkout);
    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(
        record.outcome,
        EstablishAssignmentInputCode::AmbiguousRemote.as_str(),
        "two remotes and no config key is a question, not a coin toss"
    );
}

#[test]
fn buzz_wip_remote_is_the_first_rung_of_the_ladder() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    repo_with_one_commit(&origin);
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/runner");
    // A second remote makes the ladder load-bearing: without the config key
    // this would be `ambiguous_remote`.
    run_git(&seat, &["remote", "rename", "origin", "hive"]);
    run_git(&seat, &["remote", "add", "elsewhere", "../origin"]);
    run_git(&seat, &["config", "buzz.wipRemote", "hive"]);
    let later = add_commit(&origin, "later.txt");

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("b3");
    queue_intent(&mut records, &intent(&id, &later), Some(&checkout));
    drain(&mut records, &checkout);
    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ESTABLISHED);
    assert_eq!(
        record.remote.as_deref(),
        Some("hive"),
        "the remote is whatever config named, never the literal 'origin'"
    );
}

#[test]
fn an_unreachable_remote_is_refused_as_fetch_failed() {
    let root = tempfile::tempdir().expect("temp");
    let seat = root.path().join("seat");
    repo_with_one_commit(&seat);
    run_git(
        &seat,
        &[
            "remote",
            "add",
            "gone",
            &root.path().join("no-such-repository").to_string_lossy(),
        ],
    );
    let checkout = SeatCheckout {
        path: seat.clone(),
        branch: "main".to_owned(),
    };

    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("c4");
    queue_intent(&mut records, &intent(&id, &"a".repeat(40)), Some(&checkout));
    drain(&mut records, &checkout);
    let record = assignment_input(&records, &id).expect("record");
    assert_eq!(
        record.outcome,
        EstablishAssignmentInputCode::FetchFailed.as_str()
    );
    assert!(record.message.is_some(), "a refusal says what happened");
}

#[test]
fn an_unsafe_branch_name_is_refused_before_git_sees_it() {
    for name in ["--upload-pack=x", "seat/verifier\nmain", "a..b", "@", ""] {
        assert!(
            !is_safe_branch_name(name),
            "{name:?} must not be handed to git as a single branch"
        );
    }
    assert!(is_safe_branch_name("seat/verifier"));

    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");
    let mut records = AssignmentInputRecords::new();
    let id = assignment_id("d5");
    let mut unsafe_intent = intent(&id, &first);
    unsafe_intent.branch = Some("--upload-pack=touch".to_owned());
    queue_intent(&mut records, &unsafe_intent, Some(&checkout));
    drain(&mut records, &checkout);
    assert_eq!(
        assignment_input(&records, &id).expect("record").outcome,
        EstablishAssignmentInputCode::InvalidInput.as_str()
    );
}

/// Finding 12, first half: a started-attempt record that never reached the
/// disk bounds nothing, so the checkout it was supposed to bound must not
/// happen. Before the fix this test failed with the tree already moved and
/// the outcome reported as `established`.
///
/// # Why the failure is injected and not chmod-ed
///
/// The first version of this test made the write fail by taking the write bit
/// off the store's directory. That is not a failure — it is a *permission*,
/// and root has `CAP_DAC_OVERRIDE`, so in CI pipeline 202 (which runs as root
/// in a container) the rename succeeded, the checkout ran, and the record
/// came back `established` with `attempts: 1`. The test passed on a
/// developer's machine and lied in the only place it mattered. A failing
/// [`StoreWriter`] fails by construction, for every uid there is.
#[test]
fn a_started_attempt_that_cannot_be_saved_stops_before_git() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let second = add_commit(&origin, "second.txt");
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");
    run_git(
        &seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );

    let path = root.path().join("coding-session-workdirs.json");
    desktop_shaped_store(&path);
    let id = assignment_id("f1");
    // Seeded through a working writer: what is under test is the *second*
    // write, the started-attempt record that has to reach the disk before git
    // runs.
    AssignmentInputStore::new(&path)
        .with_records(|records, _| {
            record_assignment_input(
                records,
                AssignmentInputRecord::intended(&intent(&id, &second), Some(&checkout)),
            );
        })
        .expect("seed");

    let attempted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let writes = std::sync::Arc::clone(&attempted);
    let store = AssignmentInputStore::new(&path).with_writer(std::sync::Arc::new(
        move |_path: &Path, _payload: &[u8]| {
            writes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("the disk is full".to_owned())
        },
    ));
    let outcome = establish_recorded_assignment(&store, &id, &checkout, "test-owner", &|_| false);

    assert!(
        attempted.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "the started-attempt record was never even offered to the disk"
    );
    assert_eq!(
        head_of(&seat),
        first,
        "a started-attempt record that never reached the disk must not be followed by a checkout; \
         got {outcome:?}"
    );
    assert!(
        matches!(outcome, Err(EstablishmentError::Unstarted(_))),
        "the caller is told nothing ran: {outcome:?}"
    );
    assert_eq!(
        records_of(&path)[&id]["outcome"],
        ASSIGNMENT_INPUT_INTENDED,
        "and the record on disk still says the work is owed, not started"
    );
}

/// The same guarantee through the **real** writer, failing for a reason no
/// privilege overrides: the store's parent is a regular file, so every
/// `open`/`rename` under it is `ENOTDIR`.
///
/// This is the companion to the injected-writer case above. One proves the
/// caller handles a write failure; this one proves the write path really can
/// fail and really is handled — and `ENOTDIR` is a shape error, not an access
/// check, so root gets it too.
#[test]
fn a_store_path_that_cannot_exist_stops_before_git_for_any_user() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let second = add_commit(&origin, "second.txt");
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");
    run_git(
        &seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );

    let not_a_directory = root.path().join("not-a-directory");
    std::fs::write(&not_a_directory, "a regular file\n").expect("write");
    let store = AssignmentInputStore::new(not_a_directory.join("coding-session-workdirs.json"));
    let outcome = establish_recorded_assignment(
        &store,
        &assignment_id("f5"),
        &checkout,
        "test-owner",
        &|_| false,
    );
    assert!(
        matches!(outcome, Err(EstablishmentError::Unstarted(_))),
        "a store this provider cannot even open establishes nothing: {outcome:?}"
    );
    assert_eq!(head_of(&seat), first, "nothing moved");
    assert_ne!(
        head_of(&seat),
        second,
        "and certainly not to the commit nothing was recorded about"
    );
}

/// Finding 12, second half: a terminal record that cannot be saved *after* a
/// completed checkout is a different fact — the effect is real — so it is
/// reported distinctly and written on the next pass without redoing the git
/// work.
#[test]
fn a_terminal_record_that_cannot_be_saved_is_retried_without_redoing_the_checkout() {
    let root = tempfile::tempdir().expect("temp");
    let origin = root.path().join("origin");
    let first = repo_with_one_commit(&origin);
    let second = add_commit(&origin, "second.txt");
    let seat = root.path().join("seat");
    let checkout = seat_clone(&origin, &seat, "seat/verifier");
    run_git(
        &seat,
        &["checkout", "--quiet", "-B", "seat/verifier", &first],
    );

    let path = root.path().join("coding-session-workdirs.json");
    desktop_shaped_store(&path);
    let id = assignment_id("f2");
    AssignmentInputStore::new(&path)
        .with_records(|records, _| {
            record_assignment_input(
                records,
                AssignmentInputRecord::intended(&intent(&id, &second), Some(&checkout)),
            );
        })
        .expect("seed");

    // Fail only the write that follows the git work — the one carrying the
    // terminal outcome. The started-attempt save has to succeed, or nothing
    // would run at all and this would be the other half of the finding.
    let store = AssignmentInputStore::new(&path).with_writer(std::sync::Arc::new(
        move |path: &Path, payload: &[u8]| {
            if String::from_utf8_lossy(payload).contains(ASSIGNMENT_INPUT_ESTABLISHED) {
                return Err("the disk is full".to_owned());
            }
            std::fs::write(path, payload).map_err(|error| error.to_string())
        },
    ));
    let outcome = establish_recorded_assignment(&store, &id, &checkout, "owner-1", &|_| false);
    let Err(EstablishmentError::Unrecorded { record, error }) = outcome else {
        panic!("a completed checkout with an unsaved record is its own answer: {outcome:?}");
    };
    assert!(error.contains("the disk is full"), "{error}");
    assert_eq!(record.outcome, ASSIGNMENT_INPUT_ESTABLISHED);
    assert_eq!(
        head_of(&seat),
        second,
        "the effect is real; only its record is owed"
    );

    // The retry writes the record. It does not run git again — the proof is
    // that the working store still says `establishing` before the retry, and
    // that no third attempt is started by it.
    let stored_before = records_of(&path)[&id]["outcome"].clone();
    assert_eq!(stored_before, ASSIGNMENT_INPUT_ESTABLISHING);
    let written = commit_terminal_record(
        &AssignmentInputStore::new(&path),
        "owner-1",
        (*record).clone(),
    )
    .expect("the retry writes it");
    assert_eq!(written.outcome, ASSIGNMENT_INPUT_ESTABLISHED);
    assert_eq!(written.attempts, 1, "the retry started no further attempt");
    assert_eq!(
        records_of(&path)[&id]["outcome"],
        ASSIGNMENT_INPUT_ESTABLISHED
    );
}

/// A detached task whose waiter gave up must never speak over a newer
/// outcome: the terminal write is conditional on still owning the row.
#[test]
fn a_terminal_write_from_a_superseded_attempt_is_dropped() {
    let root = tempfile::tempdir().expect("temp");
    let path = root.path().join("coding-session-workdirs.json");
    desktop_shaped_store(&path);
    let store = AssignmentInputStore::new(&path);
    let id = assignment_id("f3");
    let commit = "a".repeat(40);
    store
        .with_records(|records, _| {
            let mut newer = AssignmentInputRecord::intended(&intent(&id, &commit), None);
            newer.outcome = ASSIGNMENT_INPUT_ESTABLISHED.to_owned();
            newer.attempts = 2;
            record_assignment_input(records, newer);
        })
        .expect("seed a newer outcome");

    let mut stale = AssignmentInputRecord::intended(&intent(&id, &commit), None);
    stale.outcome = EstablishAssignmentInputCode::DirtyTree.as_str().to_owned();
    stale.attempts = 1;
    let kept = commit_terminal_record(&store, "owner-that-has-moved-on", stale).expect("answered");
    assert_eq!(
        kept.outcome, ASSIGNMENT_INPUT_ESTABLISHED,
        "the newer outcome stands"
    );
    assert_eq!(records_of(&path)[&id]["attempts"], 2);
}

/// An `establishing` record whose owner is still running here is a live
/// attempt, not an interrupted one: nothing starts a second.
#[test]
fn a_live_attempt_is_not_replayed_as_an_interrupted_one() {
    let root = tempfile::tempdir().expect("temp");
    let path = root.path().join("coding-session-workdirs.json");
    desktop_shaped_store(&path);
    let store = AssignmentInputStore::new(&path);
    let id = assignment_id("f4");
    let commit = "a".repeat(40);
    store
        .with_records(|records, _| {
            let mut running = AssignmentInputRecord::intended(&intent(&id, &commit), None);
            running.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_owned();
            running.attempts = 1;
            running.attempt_owner = Some("owner-alive".to_owned());
            record_assignment_input(records, running);
        })
        .expect("seed a live attempt");

    let checkout = SeatCheckout {
        path: root.path().join("seat"),
        branch: "seat/verifier".to_owned(),
    };
    let outcome = establish_recorded_assignment(&store, &id, &checkout, "owner-2", &|owner| {
        owner == "owner-alive"
    });
    assert!(
        matches!(outcome, Err(EstablishmentError::AlreadyRunning { ref owner }) if owner == "owner-alive"),
        "a live attempt is joined, never duplicated: {outcome:?}"
    );
    assert_eq!(
        records_of(&path)[&id]["attempts"],
        1,
        "no second attempt was started"
    );

    // The same record after a restart: nobody alive owns it, so it is the
    // interrupted case and the one-replay rule applies.
    let replayed = establish_recorded_assignment(&store, &id, &checkout, "owner-3", &|_| false);
    assert!(replayed.is_ok(), "{replayed:?}");
    assert_eq!(records_of(&path)[&id]["attempts"], 2);
}
