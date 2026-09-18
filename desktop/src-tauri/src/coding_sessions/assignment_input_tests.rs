//! Tests for establishing an assignment's input, against real repositories.
//!
//! Every case here builds an actual git repository, an actual linked seat
//! worktree and an actual remote, and then asserts on what git says
//! afterwards. Nothing asserts on a string this module would have produced:
//! the failures worth catching — a tree that did not move, a remote name that
//! was really a constant, work discarded by a refusal — are all invisible to a
//! test that only reads its own plan back.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::coding_sessions::workdir_store::CodingSessionSeatWorktree;
use crate::commands::project_git_exec::GIT_REPO_SELECTION_VARS;

const ASSIGNMENT: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const OTHER_ASSIGNMENT: &str = "1122334455667788990011223344556677889900112233445566778899001122";
const SESSION: &str = "session-verify-1";
const SEAT: &str = "verifier";
const SEAT_BRANCH: &str = "lane/verifier";
/// Well-formed, and no repository in these tests has ever contained it.
const ABSENT_COMMIT: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

static FIXTURE_SEQ: AtomicUsize = AtomicUsize::new(0);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fixture_dir(name: &str) -> PathBuf {
    let seq = FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root()
        .join("target/assignment-input-fixtures")
        .join(format!("{name}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    dir
}

/// Hermetic git for the fixtures, with a throwaway `HOME` so no test can read
/// or write the person's own git configuration.
fn fixture_git(cwd: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    command.output().expect("git ran")
}

fn git_ok(cwd: &Path, home: &Path, args: &[&str]) -> String {
    let output = fixture_git(cwd, home, args);
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A repository, a linked seat worktree inside the managed holder, and the
/// store record the hire host writes when it cuts one.
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    main: PathBuf,
    seat: PathBuf,
    store: CodingSessionWorkdirStore,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = fixture_dir(name);
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(home.join("gitconfig"), "# untouched\n").expect("global config");
        let main = root.join("main");
        std::fs::create_dir_all(&main).expect("main");
        git_ok(&main, &home, &["init", "-q", "-b", "main"]);
        std::fs::write(main.join("README.md"), "hello\n").expect("seed file");
        git_ok(&main, &home, &["add", "README.md"]);
        git_ok(
            &main,
            &home,
            &["commit", "-q", "--no-gpg-sign", "-m", "seed"],
        );
        // `<repo>.worktrees/<slug>`: the holder the store will accept a record
        // for. A path outside it is refused by `record_seat_worktree`, which is
        // the guard this fixture must not route around.
        let holder = root.join("main.worktrees");
        std::fs::create_dir_all(&holder).expect("holder");
        let seat = holder.join("verifier");
        git_ok(
            &main,
            &home,
            &[
                "worktree",
                "add",
                "-q",
                seat.to_str().expect("seat path"),
                "-b",
                SEAT_BRANCH,
            ],
        );
        let mut store = CodingSessionWorkdirStore::default();
        store
            .record_seat_worktree(
                SESSION,
                SEAT,
                CodingSessionSeatWorktree {
                    path: seat.clone(),
                    branch: SEAT_BRANCH.to_string(),
                    repo_root: main.clone(),
                    created_at: now_iso(),
                    session_id: None,
                    agents_clone: None,
                },
            )
            .expect("recorded seat worktree");
        Self {
            root,
            home,
            main,
            seat,
            store,
        }
    }

    /// Add a bare repository as a remote under a caller-chosen name.
    fn add_remote(&self, remote: &str) -> PathBuf {
        let bare = self.root.join(format!("{remote}.git"));
        std::fs::create_dir_all(&bare).expect("bare dir");
        git_ok(&bare, &self.home, &["init", "--bare", "-q", "-b", "main"]);
        let url = bare.to_string_lossy().to_string();
        git_ok(&self.main, &self.home, &["remote", "add", remote, &url]);
        git_ok(&self.main, &self.home, &["push", "-q", remote, "main"]);
        bare
    }

    /// A remote name that exists in config and points nowhere.
    fn add_broken_remote(&self, remote: &str) {
        let url = self.root.join("nowhere.git").to_string_lossy().to_string();
        git_ok(&self.main, &self.home, &["remote", "add", remote, &url]);
    }

    /// Make a commit in a *separate* clone and push it, so the object exists on
    /// the remote and nowhere in the seat's repository.
    fn commit_on_remote_only(&self, bare: &Path, branch: &str, body: &str) -> String {
        let producer = self
            .root
            .join(format!("producer-{branch}").replace('/', "-"));
        git_ok(
            &self.root,
            &self.home,
            &[
                "clone",
                "-q",
                bare.to_str().expect("bare path"),
                producer.to_str().expect("producer path"),
            ],
        );
        std::fs::write(producer.join("work.txt"), body).expect("work file");
        git_ok(&producer, &self.home, &["add", "work.txt"]);
        git_ok(
            &producer,
            &self.home,
            &["commit", "-q", "--no-gpg-sign", "-m", "work"],
        );
        git_ok(
            &producer,
            &self.home,
            &["push", "-q", "origin", &format!("HEAD:refs/heads/{branch}")],
        );
        git_ok(&producer, &self.home, &["rev-parse", "HEAD"])
    }

    /// A commit made in the main worktree: local to the seat's repository the
    /// moment it exists, and on no remote at all.
    fn commit_in_main(&self, body: &str) -> String {
        std::fs::write(self.main.join("local.txt"), body).expect("local file");
        git_ok(&self.main, &self.home, &["add", "local.txt"]);
        git_ok(
            &self.main,
            &self.home,
            &["commit", "-q", "--no-gpg-sign", "-m", "local"],
        );
        git_ok(&self.main, &self.home, &["rev-parse", "HEAD"])
    }

    fn set_config(&self, key: &str, value: &str) {
        git_ok(&self.main, &self.home, &["config", key, value]);
    }

    fn seat_head(&self) -> String {
        git_ok(&self.seat, &self.home, &["rev-parse", "HEAD"])
    }

    fn seat_branch(&self) -> String {
        git_ok(&self.seat, &self.home, &["symbolic-ref", "--short", "HEAD"])
    }

    fn request(&self, commit: &str) -> EstablishAssignmentInputRequest {
        EstablishAssignmentInputRequest {
            assignment_id: ASSIGNMENT.to_string(),
            session_ref: Some(SESSION.to_string()),
            seat_label: Some(SEAT.to_string()),
            commit: Some(commit.to_string()),
            branch: None,
        }
    }

    fn record(&self) -> &CodingSessionAssignmentInputRecord {
        self.store
            .assignment_input(ASSIGNMENT)
            .expect("an attempt was recorded")
    }
}

/// Run one attempt for this fixture's assignment.
///
/// A free function rather than a method: the request has to be built before
/// the store is borrowed mutably, and doing that inline borrows the fixture
/// twice.
fn run(
    fixture: &mut Fixture,
    commit: &str,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let request = fixture.request(commit);
    establish(&mut fixture.store, &request)
}

/// Run one attempt with a caller-built request.
fn run_request(
    fixture: &mut Fixture,
    request: EstablishAssignmentInputRequest,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    establish(&mut fixture.store, &request)
}

// --------------------------------------------------------------- establishing

#[test]
fn a_commit_already_checked_out_is_reported_already_current() {
    let mut fixture = Fixture::new("already");
    fixture.add_remote("hive");
    let head = fixture.seat_head();

    let established = run(&mut fixture, &head).expect("established");

    assert!(established.already_current, "{established:?}");
    assert_eq!(established.commit, head);
    assert_eq!(established.branch, SEAT_BRANCH);
    assert_eq!(
        established.remote, None,
        "nothing was fetched, so no remote was used"
    );
    assert_eq!(established.path, fixture.seat.to_string_lossy());
}

#[test]
fn a_commit_present_only_on_the_remote_is_fetched_and_checked_out() {
    let mut fixture = Fixture::new("fetched");
    let bare = fixture.add_remote("hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/elsewhere", "from elsewhere\n");
    assert_ne!(fixture.seat_head(), commit);

    let established = run(&mut fixture, &commit).expect("established");

    assert!(!established.already_current);
    assert_eq!(established.commit, commit);
    assert_eq!(fixture.seat_head(), commit, "HEAD is the requested commit");
    assert_eq!(fixture.seat_branch(), SEAT_BRANCH);
    assert!(
        fixture.seat.join("work.txt").exists(),
        "the fetched tree is on disk"
    );
}

#[test]
fn a_commit_no_remote_has_is_refused_as_unknown_after_the_fetch() {
    let mut fixture = Fixture::new("unknown");
    fixture.add_remote("hive");
    let before = fixture.seat_head();

    let error = run(&mut fixture, ABSENT_COMMIT).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::UnknownCommit);
    assert_eq!(fixture.seat_head(), before, "the tree did not move");
}

#[test]
fn a_remote_that_cannot_be_reached_is_refused_as_fetch_failed() {
    let mut fixture = Fixture::new("fetchfail");
    fixture.add_remote("hive");
    fixture.add_broken_remote("gone");
    fixture.set_config("remote.pushDefault", "gone");

    let error = run(&mut fixture, ABSENT_COMMIT).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::FetchFailed);
    assert!(error.detail.is_some(), "git's own stderr is carried");
}

#[test]
fn establishing_the_same_input_twice_is_idempotent() {
    let mut fixture = Fixture::new("idempotent");
    let bare = fixture.add_remote("hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/twice", "twice\n");

    let first = run(&mut fixture, &commit).expect("first");
    let second = run(&mut fixture, &commit).expect("second");

    assert!(!first.already_current);
    assert!(second.already_current, "{second:?}");
    assert_eq!(fixture.seat_head(), commit);
}

// -------------------------------------------------------------- the refusals

#[test]
fn a_dirty_tree_is_refused_and_nothing_on_disk_moves() {
    let mut fixture = Fixture::new("dirty");
    let bare = fixture.add_remote("hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/dirty", "dirty\n");
    let before = fixture.seat_head();
    std::fs::write(fixture.seat.join("scratch.txt"), "unsaved\n").expect("dirty file");
    std::fs::write(fixture.seat.join("README.md"), "edited\n").expect("edit");

    let error = run(&mut fixture, &commit).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::DirtyTree);
    assert!(
        error.message.contains("2 uncommitted change(s)"),
        "{}",
        error.message
    );
    assert_eq!(
        error.changes,
        Some(2),
        "the count is a field, not something to parse out of the sentence"
    );
    assert!(error.detail.expect("paths").contains("scratch.txt"));
    assert_eq!(fixture.seat_head(), before, "HEAD did not move");
    assert!(
        fixture.seat.join("scratch.txt").exists(),
        "the uncommitted file survives"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.seat.join("README.md")).expect("read"),
        "edited\n",
        "the uncommitted edit survives"
    );
}

#[test]
fn no_refusal_but_a_dirty_tree_carries_a_change_count() {
    // One case per code that a fixture can reach without a second repository:
    // every one of them must leave `changes` absent, because the number means
    // uncommitted work and nothing else.
    let mut fixture = Fixture::new("nochanges");
    let head = fixture.seat_head();
    let no_remote = run(&mut fixture, ABSENT_COMMIT).expect_err("no remote");
    assert_eq!(no_remote.code, EstablishAssignmentInputCode::NoRemote);
    assert_eq!(no_remote.changes, None);

    fixture.add_remote("hive");
    fixture.add_remote("bridge");
    let ambiguous = run(&mut fixture, ABSENT_COMMIT).expect_err("ambiguous");
    assert_eq!(
        ambiguous.code,
        EstablishAssignmentInputCode::AmbiguousRemote
    );
    assert_eq!(ambiguous.changes, None);

    fixture.set_config("remote.pushDefault", "hive");
    let unknown = run(&mut fixture, ABSENT_COMMIT).expect_err("unknown");
    assert_eq!(unknown.code, EstablishAssignmentInputCode::UnknownCommit);
    assert_eq!(unknown.changes, None);

    let bad = run(&mut fixture, "not-a-commit").expect_err("invalid");
    assert_eq!(bad.code, EstablishAssignmentInputCode::InvalidInput);
    assert_eq!(bad.changes, None);

    let established = run(&mut fixture, &head).expect("established");
    assert!(established.already_current);
    assert_eq!(
        fixture.record().changes,
        None,
        "a success records no change count"
    );
}

#[test]
fn a_seat_this_host_never_recorded_is_refused() {
    let mut store = CodingSessionWorkdirStore::default();
    let request = EstablishAssignmentInputRequest {
        assignment_id: ASSIGNMENT.to_string(),
        session_ref: Some(SESSION.to_string()),
        seat_label: Some("nobody".to_string()),
        commit: Some(ABSENT_COMMIT.to_string()),
        branch: None,
    };

    let error = establish(&mut store, &request).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::UnrecordedTree);
    assert_eq!(error.detail.as_deref(), Some("session-verify-1/nobody"));
}

#[test]
fn a_recorded_path_that_is_gone_is_refused() {
    let mut fixture = Fixture::new("missing");
    fixture.add_remote("hive");
    let head = fixture.seat_head();
    std::fs::remove_dir_all(&fixture.seat).expect("remove the seat tree");

    let error = run(&mut fixture, &head).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::MissingTree);
}

#[test]
fn no_remote_configured_is_refused_when_the_object_is_missing() {
    let mut fixture = Fixture::new("noremote");

    // Absent from this repository, so a remote is genuinely the only way to
    // get it — which is what makes `no_remote` a true statement here.
    let error = run(&mut fixture, ABSENT_COMMIT).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::NoRemote);
}

#[test]
fn a_commit_already_present_is_established_with_no_remote_configured() {
    let mut fixture = Fixture::new("localonly");
    // No `add_remote`: this repository has none, and needs none.
    let commit = fixture.commit_in_main("local work\n");
    assert_ne!(fixture.seat_head(), commit);

    let established = run(&mut fixture, &commit).expect("established");

    assert!(!established.already_current, "{established:?}");
    assert_eq!(established.remote, None);
    assert_eq!(established.commit, commit);
    assert_eq!(fixture.seat_head(), commit, "the tree really moved");
    assert_eq!(fixture.seat_branch(), SEAT_BRANCH);
    let record = fixture.record();
    assert_eq!(record.outcome, "established");
    assert_eq!(record.commit.as_deref(), Some(commit.as_str()));
    assert_eq!(
        record.remote, None,
        "no remote was used, so none is recorded"
    );
}

#[test]
fn several_remotes_with_no_config_answer_are_ambiguous() {
    let mut fixture = Fixture::new("ambiguous");
    fixture.add_remote("hive");
    fixture.add_remote("bridge");

    let error = run(&mut fixture, ABSENT_COMMIT).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::AmbiguousRemote);
    let detail = error.detail.expect("the candidates are named");
    assert!(
        detail.contains("hive") && detail.contains("bridge"),
        "{detail}"
    );
}

#[test]
fn a_malformed_commit_is_refused_as_invalid_input() {
    let mut fixture = Fixture::new("badcommit");
    fixture.add_remote("hive");

    for candidate in [
        "",
        "HEAD",
        "DEADBEEFDEADBEEFDEADBEEFDEADBEEFDEADBEEF",
        "abc123",
    ] {
        let error = run(&mut fixture, candidate).expect_err("refused");
        assert_eq!(
            error.code,
            EstablishAssignmentInputCode::InvalidInput,
            "{candidate:?}"
        );
    }
}

#[test]
fn an_unsafe_branch_name_is_refused_as_invalid_input() {
    let mut fixture = Fixture::new("badbranch");
    fixture.add_remote("hive");
    let head = fixture.seat_head();

    for candidate in [
        "-force",
        "lane/../main",
        "lane verifier",
        "lane:refs/heads/main",
        "lane^",
        "refs/heads/main.lock",
    ] {
        let mut request = fixture.request(&head);
        request.branch = Some(candidate.to_string());
        let error = run_request(&mut fixture, request).expect_err("refused");
        assert_eq!(
            error.code,
            EstablishAssignmentInputCode::InvalidInput,
            "{candidate:?}"
        );
    }
    assert_eq!(
        fixture.seat_branch(),
        SEAT_BRANCH,
        "the branch did not move"
    );
}

#[test]
fn an_assignment_id_that_is_not_an_event_id_is_refused() {
    let mut store = CodingSessionWorkdirStore::default();
    let request = EstablishAssignmentInputRequest {
        assignment_id: "not-an-id".to_string(),
        ..EstablishAssignmentInputRequest::default()
    };

    let error = establish(&mut store, &request).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::InvalidInput);
    assert!(
        store.assignment_inputs.is_empty(),
        "nothing is filed under an id that is not one"
    );
}

#[test]
fn a_request_with_nothing_to_replay_names_what_is_missing() {
    let mut store = CodingSessionWorkdirStore::default();
    let request = EstablishAssignmentInputRequest {
        assignment_id: ASSIGNMENT.to_string(),
        ..EstablishAssignmentInputRequest::default()
    };

    let error = establish(&mut store, &request).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::InvalidInput);
    for field in ["sessionRef", "seatLabel", "commit"] {
        assert!(error.message.contains(field), "{}", error.message);
    }
}

// ------------------------------------------------------------- the remote name

#[test]
fn the_remote_comes_from_push_default_under_a_name_that_is_not_origin() {
    let mut fixture = Fixture::new("pushdefault");
    fixture.add_remote("hive");
    let bare = fixture.add_remote("bridge");
    fixture.set_config("remote.pushDefault", "bridge");
    let commit = fixture.commit_on_remote_only(&bare, "work/bridge", "bridge\n");

    let established = run(&mut fixture, &commit).expect("established");

    assert_eq!(established.remote.as_deref(), Some("bridge"));
    assert_eq!(fixture.seat_head(), commit);
}

#[test]
fn the_branchs_push_remote_wins_over_push_default() {
    let mut fixture = Fixture::new("branchpush");
    let bare = fixture.add_remote("hive");
    fixture.add_remote("bridge");
    fixture.set_config("remote.pushDefault", "bridge");
    fixture.set_config(&format!("branch.{SEAT_BRANCH}.pushRemote"), "hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/branchpush", "branch push\n");

    let established = run(&mut fixture, &commit).expect("established");

    assert_eq!(established.remote.as_deref(), Some("hive"));
}

#[test]
fn buzz_wip_remote_is_the_first_rung_of_the_ladder() {
    let mut fixture = Fixture::new("wipremote");
    let bare = fixture.add_remote("hive");
    fixture.add_remote("bridge");
    fixture.set_config("remote.pushDefault", "bridge");
    fixture.set_config(&format!("branch.{SEAT_BRANCH}.pushRemote"), "bridge");
    fixture.set_config("buzz.wipRemote", "hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/wip", "wip remote\n");

    let established = run(&mut fixture, &commit).expect("established");

    assert_eq!(established.remote.as_deref(), Some("hive"));
}

#[test]
fn a_configured_remote_the_repository_does_not_have_is_no_remote() {
    let mut fixture = Fixture::new("phantom");
    fixture.add_remote("hive");
    fixture.set_config("buzz.wipRemote", "origin");

    let error = run(&mut fixture, ABSENT_COMMIT).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::NoRemote);
    assert!(error.message.contains("origin"), "{}", error.message);
}

// ------------------------------------------------------------- the record

#[test]
fn a_successful_attempt_is_recorded_with_commit_path_remote_and_time() {
    let mut fixture = Fixture::new("record-ok");
    let bare = fixture.add_remote("hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/record", "record\n");

    run(&mut fixture, &commit).expect("established");

    let record = fixture.record();
    assert_eq!(record.assignment_id, ASSIGNMENT);
    assert_eq!(record.session_ref.as_deref(), Some(SESSION));
    assert_eq!(record.seat_label.as_deref(), Some(SEAT));
    assert_eq!(record.commit.as_deref(), Some(commit.as_str()));
    assert_eq!(record.path.as_deref(), Some(fixture.seat.as_path()));
    assert_eq!(
        record.remote.as_deref(),
        Some("hive"),
        "this commit had to be fetched, so the remote is recorded"
    );
    assert_eq!(record.outcome, "established");
    assert_eq!(record.message, None);
    assert!(record.recorded_at.contains('T'), "{}", record.recorded_at);
}

#[test]
fn a_second_establish_records_already_current() {
    let mut fixture = Fixture::new("record-current");
    fixture.add_remote("hive");
    let head = fixture.seat_head();

    run(&mut fixture, &head).expect("established");

    assert_eq!(fixture.record().outcome, "already_current");
}

#[test]
fn a_refusal_is_recorded_with_its_code_and_message() {
    let mut fixture = Fixture::new("record-refusal");
    fixture.add_remote("hive");
    let head = fixture.seat_head();
    std::fs::write(fixture.seat.join("scratch.txt"), "unsaved\n").expect("dirty file");

    run(&mut fixture, &head).expect_err("refused");

    let record = fixture.record();
    assert_eq!(record.outcome, "dirty_tree");
    assert_eq!(record.changes, Some(1), "the count is durable too");
    assert!(
        record
            .message
            .as_deref()
            .expect("a refusal carries its sentence")
            .contains("1 uncommitted change(s)"),
        "{record:?}"
    );
    assert_eq!(record.path.as_deref(), Some(fixture.seat.as_path()));
}

#[test]
fn a_retry_with_only_the_assignment_id_replays_the_recorded_inputs() {
    let mut fixture = Fixture::new("retry-replay");
    let bare = fixture.add_remote("hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/replay", "replay\n");
    run(&mut fixture, &commit).expect("established");

    let retry = EstablishAssignmentInputRequest {
        assignment_id: ASSIGNMENT.to_string(),
        ..EstablishAssignmentInputRequest::default()
    };
    let established = run_request(&mut fixture, retry).expect("replayed");

    assert!(established.already_current, "{established:?}");
    assert_eq!(established.commit, commit);
    assert_eq!(fixture.record().commit.as_deref(), Some(commit.as_str()));
}

#[test]
fn a_retry_with_a_different_commit_replaces_the_record() {
    let mut fixture = Fixture::new("retry-replace");
    let bare = fixture.add_remote("hive");
    let first = fixture.commit_on_remote_only(&bare, "work/first", "first\n");
    let second = fixture.commit_on_remote_only(&bare, "work/second", "second\n");
    run(&mut fixture, &first).expect("first");

    run(&mut fixture, &second).expect("second");

    assert_eq!(
        fixture.store.assignment_inputs.len(),
        1,
        "one per assignment"
    );
    assert_eq!(fixture.record().commit.as_deref(), Some(second.as_str()));
    assert_eq!(fixture.seat_head(), second);
}

#[test]
fn the_record_is_readable_by_assignment_id_without_touching_git() {
    let mut fixture = Fixture::new("record-read");
    fixture.add_remote("hive");
    let head = fixture.seat_head();
    run(&mut fixture, &head).expect("established");

    // The body of `coding_session_assignment_input_record`, minus the store
    // load the Tauri handle does.
    let found = fixture.store.assignment_input(ASSIGNMENT).cloned();
    let absent = fixture.store.assignment_input(OTHER_ASSIGNMENT);

    assert_eq!(
        found.expect("the record").commit.as_deref(),
        Some(head.as_str())
    );
    assert!(
        absent.is_none(),
        "an assignment with no attempt has no record"
    );
}

#[test]
fn the_record_survives_a_round_trip_through_the_store_file() {
    let mut fixture = Fixture::new("record-json");
    fixture.add_remote("hive");
    let head = fixture.seat_head();
    run(&mut fixture, &head).expect("established");

    let json = serde_json::to_string(&fixture.store).expect("serialize");
    let reloaded: CodingSessionWorkdirStore = serde_json::from_str(&json).expect("deserialize");

    assert!(
        json.contains("\"assignmentInputs\""),
        "camelCase on the wire"
    );
    assert!(json.contains("\"alreadyCurrent\"") || json.contains("\"recordedAt\""));
    assert_eq!(
        reloaded.assignment_input(ASSIGNMENT),
        fixture.store.assignment_input(ASSIGNMENT)
    );
}

#[test]
fn records_are_bounded_and_the_oldest_attempt_is_evicted_first() {
    let mut store = CodingSessionWorkdirStore::default();
    for index in 0..(MAX_ASSIGNMENT_INPUT_RECORDS + 10) {
        store.record_assignment_input(CodingSessionAssignmentInputRecord {
            assignment_id: format!("{index:064x}"),
            session_ref: Some(SESSION.to_string()),
            seat_label: Some(SEAT.to_string()),
            commit: None,
            branch: None,
            path: None,
            remote: None,
            outcome: "unrecorded_tree".to_string(),
            message: None,
            changes: None,
            // Ordered by construction, so "oldest" is not a guess about clocks.
            recorded_at: format!("2026-09-15T00:00:{:02}Z", index % 60),
        });
    }

    assert_eq!(store.assignment_inputs.len(), MAX_ASSIGNMENT_INPUT_RECORDS);
    let newest = format!("{:064x}", MAX_ASSIGNMENT_INPUT_RECORDS + 9);
    assert!(
        store.assignment_input(&newest).is_some(),
        "the newest attempt is kept"
    );
    assert!(
        store.assignment_input(&format!("{:064x}", 0)).is_none(),
        "the oldest attempt was evicted"
    );
}

#[test]
fn every_refusal_code_serializes_to_its_documented_word() {
    for code in [
        EstablishAssignmentInputCode::UnrecordedTree,
        EstablishAssignmentInputCode::MissingTree,
        EstablishAssignmentInputCode::DirtyTree,
        EstablishAssignmentInputCode::NoRemote,
        EstablishAssignmentInputCode::AmbiguousRemote,
        EstablishAssignmentInputCode::FetchFailed,
        EstablishAssignmentInputCode::UnknownCommit,
        EstablishAssignmentInputCode::CheckoutFailed,
        EstablishAssignmentInputCode::InvalidInput,
    ] {
        assert_eq!(
            serde_json::to_string(&code).expect("serialize"),
            format!("\"{}\"", code.as_str()),
            "the wire word and the recorded word must be the same word"
        );
    }
}

#[test]
fn a_branch_belonging_to_someone_else_is_never_reset() {
    let mut fixture = Fixture::new("otherbranch");
    let bare = fixture.add_remote("hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/other", "other\n");
    git_ok(
        &fixture.main,
        &fixture.home,
        &["branch", "lane/somebody-else"],
    );
    let before = git_ok(
        &fixture.main,
        &fixture.home,
        &["rev-parse", "lane/somebody-else"],
    );
    let mut request = fixture.request(&commit);
    request.branch = Some("lane/somebody-else".to_string());

    let error = run_request(&mut fixture, request).expect_err("refused");

    assert_eq!(error.code, EstablishAssignmentInputCode::CheckoutFailed);
    assert_eq!(
        git_ok(
            &fixture.main,
            &fixture.home,
            &["rev-parse", "lane/somebody-else"]
        ),
        before,
        "the other branch did not move"
    );
}

#[test]
fn a_named_branch_that_does_not_exist_yet_is_created_on_the_commit() {
    let mut fixture = Fixture::new("newbranch");
    let bare = fixture.add_remote("hive");
    let commit = fixture.commit_on_remote_only(&bare, "work/new", "new\n");
    let mut request = fixture.request(&commit);
    request.branch = Some("lane/verifier-2".to_string());

    let established = run_request(&mut fixture, request).expect("established");

    assert_eq!(established.branch, "lane/verifier-2");
    assert_eq!(fixture.seat_branch(), "lane/verifier-2");
    assert_eq!(fixture.seat_head(), commit);
}
