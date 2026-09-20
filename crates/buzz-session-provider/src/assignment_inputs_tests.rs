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

fn drain(records: &mut AssignmentInputRecords, checkout: &SeatCheckout) -> Vec<String> {
    let checkout = checkout.clone();
    drain_pending_assignment_inputs(records, move |_| Some(checkout.clone()), |_| {})
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
