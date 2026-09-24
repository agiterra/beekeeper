//! Tests for the desktop's *adapter* onto the shared assignment-input records.
//!
//! The git ladder, the refusal codes and the durable queue moved to
//! `buzz-session-provider`, and their cases moved with them
//! (`crates/buzz-session-provider/src/assignment_inputs_tests.rs`: remote
//! ladder, fetch, dirty tree, interruption bound, two-process lock). What is
//! left to prove here is what only this crate can get wrong — that the seat's
//! tree is resolved from the record this host wrote when it cut it, that a
//! seat it never recorded is refused rather than searched for, and that the
//! shared record round-trips through the desktop's own store file.
//!
//! Every case still builds an actual repository and an actual linked seat
//! worktree, and asserts on what git says afterwards.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::coding_sessions::workdir_store::CodingSessionSeatWorktree;
use crate::commands::project_git_exec::GIT_REPO_SELECTION_VARS;
use crate::util::now_iso;

const ASSIGNMENT: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const SESSION: &str = "session-verify-1";
const SEAT: &str = "verifier";
const SEAT_BRANCH: &str = "lane/verifier";

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
                    commit_identity: None,
                    actor_pubkey: None,
                },
            )
            .expect("recorded seat worktree");
        Self {
            home,
            main,
            seat,
            store,
        }
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
}

/// Run one attempt for this fixture's assignment.
fn run(
    fixture: &mut Fixture,
    commit: &str,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let request = fixture.request(commit);
    establish(&mut fixture.store, &request)
}

#[test]
fn the_seat_tree_the_host_recorded_is_the_tree_that_moves() {
    let mut fixture = Fixture::new("recorded-tree");
    let local = fixture.commit_in_main("local only\n");
    let established = run(&mut fixture, &local).expect("established");

    assert_eq!(established.commit, local);
    assert_eq!(established.branch, SEAT_BRANCH);
    assert_eq!(
        established.remote, None,
        "a commit already in the repository needs no remote"
    );
    assert_eq!(fixture.seat_head(), local, "git, not the return value");
    assert_eq!(fixture.seat_branch(), SEAT_BRANCH);
    assert_eq!(
        fixture
            .store
            .assignment_input(ASSIGNMENT)
            .expect("record")
            .path
            .as_deref(),
        Some(fixture.seat.as_path()),
        "the record names the tree this host cut, not one it found"
    );
}

#[test]
fn a_seat_this_host_never_recorded_is_refused_rather_than_searched_for() {
    let mut fixture = Fixture::new("unrecorded");
    let local = fixture.commit_in_main("local only\n");
    let request = EstablishAssignmentInputRequest {
        assignment_id: ASSIGNMENT.to_string(),
        session_ref: Some(SESSION.to_string()),
        seat_label: Some("someone-elses-seat".to_string()),
        commit: Some(local.clone()),
        branch: None,
    };
    let refusal = establish(&mut fixture.store, &request).expect_err("refused");
    assert_eq!(refusal.code, EstablishAssignmentInputCode::UnrecordedTree);
    assert_ne!(
        fixture.seat_head(),
        local,
        "the recorded seat's tree must not move for another seat's assignment"
    );
}

#[test]
fn a_dirty_tree_is_refused_and_nothing_on_disk_moves() {
    let mut fixture = Fixture::new("dirty");
    let before = fixture.seat_head();
    let local = fixture.commit_in_main("local only\n");
    std::fs::write(fixture.seat.join("wip.txt"), "unpublished work\n").expect("wip file");

    let refusal = run(&mut fixture, &local).expect_err("refused");
    assert_eq!(refusal.code, EstablishAssignmentInputCode::DirtyTree);
    assert_eq!(refusal.changes, Some(1));
    assert_eq!(fixture.seat_head(), before);
    assert_eq!(
        std::fs::read_to_string(fixture.seat.join("wip.txt")).expect("wip survives"),
        "unpublished work\n"
    );
}

#[test]
fn an_assignment_id_that_is_not_an_event_id_is_refused_and_records_nothing() {
    let mut fixture = Fixture::new("bad-id");
    let local = fixture.commit_in_main("local only\n");
    let request = EstablishAssignmentInputRequest {
        assignment_id: "not-an-event-id".to_string(),
        session_ref: Some(SESSION.to_string()),
        seat_label: Some(SEAT.to_string()),
        commit: Some(local),
        branch: None,
    };
    let refusal = establish(&mut fixture.store, &request).expect_err("refused");
    assert_eq!(refusal.code, EstablishAssignmentInputCode::InvalidInput);
    assert!(
        fixture.store.assignment_inputs.is_empty(),
        "a record has to be filed under an assignment id"
    );
}

#[test]
fn the_shared_record_round_trips_through_this_stores_json() {
    let mut fixture = Fixture::new("round-trip");
    let local = fixture.commit_in_main("local only\n");
    run(&mut fixture, &local).expect("established");

    let encoded = serde_json::to_string(&fixture.store).expect("encode");
    let decoded: CodingSessionWorkdirStore = serde_json::from_str(&encoded).expect("decode");
    assert_eq!(
        decoded.assignment_input(ASSIGNMENT),
        fixture.store.assignment_input(ASSIGNMENT),
        "the record the provider writes and the record this store holds are one shape"
    );
    let raw: serde_json::Value = serde_json::from_str(&encoded).expect("json");
    assert_eq!(
        raw["assignmentInputs"][ASSIGNMENT]["outcome"], "established",
        "the outcome word travels as the provider spells it"
    );
    assert!(
        raw["assignmentInputs"][ASSIGNMENT]["recordedAt"].is_string(),
        "recordedAt is a string, as the host has always written it"
    );
}

#[test]
fn a_cut_worktrees_actor_pubkey_is_recorded_and_the_read_returns_it() {
    let actor = "a".repeat(64);
    let mut fixture = Fixture::new("actor-pubkey");
    // Overwrite the fixture's record with one naming the seat's actor, the
    // way `create_coding_session_worktree` does when a hire cuts the tree —
    // the fixture itself pins `actor_pubkey: None` to prove the pre-existing
    // (legacy) shape still loads.
    fixture
        .store
        .record_seat_worktree(
            SESSION,
            SEAT,
            CodingSessionSeatWorktree {
                path: fixture.seat.clone(),
                branch: SEAT_BRANCH.to_string(),
                repo_root: fixture.main.clone(),
                created_at: now_iso(),
                session_id: None,
                agents_clone: None,
                commit_identity: None,
                actor_pubkey: Some(actor.clone()),
            },
        )
        .expect("recorded seat worktree with an actor");

    let actors = seat_worktree_actors(&fixture.store, SESSION);
    assert_eq!(
        actors.labels.get(&actor).map(String::as_str),
        Some(SEAT),
        "the read answers the seat label this host cut the tree under, keyed by actor"
    );
    assert!(
        !actors.unattributed,
        "every worktree in this session named an actor, so nothing is ambiguous"
    );

    // A record cut before this host tracked actor pubkeys is unattributed,
    // not absent, and a miss for a genuine stranger to this session stays a
    // miss.
    let mut legacy_present = fixture.store.clone();
    legacy_present
        .record_seat_worktree(
            SESSION,
            "builder",
            CodingSessionSeatWorktree {
                path: fixture
                    .seat
                    .parent()
                    .expect("seat has a parent holder")
                    .join("builder"),
                branch: "lane/builder".to_string(),
                repo_root: fixture.main.clone(),
                created_at: now_iso(),
                session_id: None,
                agents_clone: None,
                commit_identity: None,
                actor_pubkey: None,
            },
        )
        .expect("recorded a legacy seat worktree");
    let with_legacy = seat_worktree_actors(&legacy_present, SESSION);
    assert!(
        with_legacy.unattributed,
        "a worktree with no actor pubkey makes this session's attribution ambiguous"
    );
    assert_eq!(
        with_legacy.labels.get(&"b".repeat(64)),
        None,
        "an actor this session never cut a tree for is still a genuine miss"
    );
}
