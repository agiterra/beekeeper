//! Tests for the durable establishment sequence, against real repositories.
//!
//! The properties worth pinning here are all about *when* the host acts, and
//! none of them can be read off a return value: an intent that reaches the
//! disk before the git work, a pending intent that a later run finishes, a
//! refusal that is never tried again, and an interrupted attempt that is
//! replayed once and then abandoned. So every case builds an actual repository
//! and an actual linked seat worktree, and asserts on what git says afterwards
//! and on what the store holds — never on a sentence this module wrote.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::coding_sessions::workdir_store::CodingSessionSeatWorktree;
use crate::commands::project_git_exec::GIT_REPO_SELECTION_VARS;

const ASSIGNMENT: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const SESSION: &str = "session-establish-1";
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
        .join("target/assignment-establishment-fixtures")
        .join(format!("{name}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    dir
}

/// Hermetic git with a throwaway `HOME`, so no test reads or writes the
/// person's own git configuration.
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

/// A repository with two commits, a linked seat worktree parked on the first,
/// and the store record the hire host writes when it cuts one.
struct Fixture {
    home: PathBuf,
    seat: PathBuf,
    /// The commit an assignment names: the tip, which the seat is *not* on.
    target: String,
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
        // The seat is cut from the seed commit, exactly as a hire does today.
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
        // …and then the work being verified lands on the trunk.
        std::fs::write(main.join("feature.txt"), "the work\n").expect("feature file");
        git_ok(&main, &home, &["add", "feature.txt"]);
        git_ok(
            &main,
            &home,
            &["commit", "-q", "--no-gpg-sign", "-m", "the work"],
        );
        let target = git_ok(&main, &home, &["rev-parse", "HEAD"]);

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
            home,
            seat,
            target,
            store,
        }
    }

    fn seat_head(&self) -> String {
        git_ok(&self.seat, &self.home, &["rev-parse", "HEAD"])
    }

    fn record(&self) -> &CodingSessionAssignmentInputRecord {
        self.store
            .assignment_input(ASSIGNMENT)
            .expect("a record for the assignment")
    }
}

/// A `persist` that counts, so "written before the git work" is measured.
fn counting_persist(saves: &Cell<usize>) -> impl FnMut(&CodingSessionWorkdirStore) + '_ {
    move |_| saves.set(saves.get() + 1)
}

fn observation(fixture: &Fixture) -> ObservedCodingSessionAssignment {
    ObservedCodingSessionAssignment {
        assignment_id: ASSIGNMENT.to_string(),
        session_ref: SESSION.to_string(),
        seat_label: Some(SEAT.to_string()),
        assignee_role: Some("verifier".to_string()),
        base_sha: Some(fixture.target.clone()),
        branch: None,
    }
}

#[test]
fn an_observed_assignment_is_queued_then_established_in_the_seats_tree() {
    let mut fixture = Fixture::new("queued-then-established");
    let seed = fixture.seat_head();
    assert_ne!(seed, fixture.target, "the seat starts on the wrong commit");

    let observed = vec![observation(&fixture)];
    let dispositions = queue_observed_assignments(&mut fixture.store, &observed);
    assert_eq!(
        dispositions,
        vec![CodingSessionAssignmentInputDisposition::Recorded]
    );
    // Queued, durably, before any git work: the intent is the thing that
    // survives the panel closing.
    assert_eq!(fixture.record().outcome, ASSIGNMENT_INPUT_INTENDED);
    assert_eq!(fixture.record().attempts, 0);

    let saves = Cell::new(0);
    let drained = drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves));
    assert_eq!(drained, vec![ASSIGNMENT.to_string()]);
    assert_eq!(fixture.seat_head(), fixture.target, "the tree moved");
    assert_eq!(fixture.record().outcome, "established");
    assert_eq!(fixture.record().attempts, 1);
    // Counted before and after the attempt: a crash between them leaves the
    // started count on disk, which is what bounds the replay.
    assert_eq!(saves.get(), 2);
}

#[test]
fn a_pending_intent_is_replayed_once_by_a_later_run() {
    let mut fixture = Fixture::new("replayed-after-restart");
    let observed = vec![observation(&fixture)];
    queue_observed_assignments(&mut fixture.store, &observed);

    // The restart: the same durable store, reloaded, with no observation and
    // no surface — which is the whole point of the queue.
    let serialized = serde_json::to_string(&fixture.store).expect("store serializes");
    fixture.store = serde_json::from_str(&serialized).expect("store reloads");
    assert_eq!(
        pending_assignment_inputs(&fixture.store),
        vec![ASSIGNMENT.to_string()]
    );

    let saves = Cell::new(0);
    drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves));
    assert_eq!(fixture.seat_head(), fixture.target);
    assert_eq!(fixture.record().outcome, "established");

    // Once, not on every pass: the settled record is not pending.
    assert!(pending_assignment_inputs(&fixture.store).is_empty());
    let drained = drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves));
    assert!(drained.is_empty());
}

#[test]
fn a_refusal_is_recorded_and_never_retried_on_its_own() {
    let mut fixture = Fixture::new("refusal-not-retried");
    // Uncommitted work in the seat's tree: the one thing nobody else has a
    // copy of, so the attempt refuses and writes nothing.
    std::fs::write(fixture.seat.join("scratch.txt"), "mine\n").expect("scratch file");
    let before = fixture.seat_head();

    let observed = observation(&fixture);
    queue_observed_assignments(&mut fixture.store, &[observed]);
    let saves = Cell::new(0);
    drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves));

    assert_eq!(fixture.record().outcome, "dirty_tree");
    assert_eq!(fixture.record().changes, Some(1));
    assert_eq!(fixture.seat_head(), before, "nothing was discarded");
    assert!(fixture.record().message.is_some(), "the reason is recorded");

    // A terminal outcome is not pending, so no later pass touches it, and a
    // fresh observation of the same commit is dispositive.
    assert!(pending_assignment_inputs(&fixture.store).is_empty());
    let again = observation(&fixture);
    let dispositions = queue_observed_assignments(&mut fixture.store, &[again]);
    assert_eq!(
        dispositions,
        vec![CodingSessionAssignmentInputDisposition::Recorded]
    );
    assert_eq!(fixture.record().outcome, "dirty_tree");
    assert!(
        drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves)).is_empty()
    );
}

#[test]
fn an_attempt_interrupted_twice_is_abandoned_rather_than_replayed_forever() {
    let mut fixture = Fixture::new("abandoned-after-two");
    let observed = observation(&fixture);
    queue_observed_assignments(&mut fixture.store, &[observed]);

    // What a quit mid-checkout leaves behind, twice over.
    let mut interrupted = fixture.record().clone();
    interrupted.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_string();
    interrupted.attempts = MAX_ESTABLISH_ATTEMPTS;
    fixture.store.record_assignment_input(interrupted);

    let seed = fixture.seat_head();
    let saves = Cell::new(0);
    drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves));

    assert_eq!(fixture.record().outcome, ASSIGNMENT_INPUT_ABANDONED);
    assert_eq!(
        fixture.seat_head(),
        seed,
        "no third attempt ran against the tree"
    );
    assert!(
        fixture
            .record()
            .message
            .as_deref()
            .is_some_and(|message| message.contains("will not be tried again")),
        "the abandonment says so: {:?}",
        fixture.record().message
    );
    assert!(pending_assignment_inputs(&fixture.store).is_empty());
}

#[test]
fn one_interruption_is_finished_rather_than_abandoned() {
    let mut fixture = Fixture::new("one-interruption-finishes");
    let observed = observation(&fixture);
    queue_observed_assignments(&mut fixture.store, &[observed]);
    let mut interrupted = fixture.record().clone();
    interrupted.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_string();
    interrupted.attempts = 1;
    fixture.store.record_assignment_input(interrupted);

    let saves = Cell::new(0);
    drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves));
    assert_eq!(fixture.seat_head(), fixture.target);
    assert_eq!(fixture.record().outcome, "established");
    assert_eq!(fixture.record().attempts, MAX_ESTABLISH_ATTEMPTS);
}

#[test]
fn a_persons_requeue_clears_the_count_and_reaches_the_tree_again() {
    let mut fixture = Fixture::new("requeue-after-refusal");
    std::fs::write(fixture.seat.join("scratch.txt"), "mine\n").expect("scratch file");
    let observed = observation(&fixture);
    queue_observed_assignments(&mut fixture.store, &[observed]);
    let saves = Cell::new(0);
    drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves));
    assert_eq!(fixture.record().outcome, "dirty_tree");

    // The person fixes the tree and asks again.
    std::fs::remove_file(fixture.seat.join("scratch.txt")).expect("scratch removed");
    assert!(requeue_assignment_input(&mut fixture.store, ASSIGNMENT));
    assert_eq!(fixture.record().attempts, 0);
    assert_eq!(fixture.record().outcome, ASSIGNMENT_INPUT_INTENDED);
    drain_pending_assignment_inputs(&mut fixture.store, counting_persist(&saves));
    assert_eq!(fixture.seat_head(), fixture.target);
    assert_eq!(fixture.record().outcome, "established");
}

#[test]
fn a_requeue_of_an_assignment_this_host_never_saw_answers_no() {
    let mut store = CodingSessionWorkdirStore::default();
    assert!(!requeue_assignment_input(&mut store, ASSIGNMENT));
    assert!(store.assignment_inputs.is_empty());
}

#[test]
fn a_new_commit_on_the_same_assignment_is_a_new_intent() {
    let mut fixture = Fixture::new("new-commit-new-intent");
    let observed = observation(&fixture);
    queue_observed_assignments(&mut fixture.store, &[observed]);
    drain_pending_assignment_inputs(&mut fixture.store, |_| {});
    assert_eq!(fixture.record().outcome, "established");

    let mut later = observation(&fixture);
    later.base_sha = Some("c".repeat(40));
    queue_observed_assignments(&mut fixture.store, &[later]);
    assert_eq!(fixture.record().outcome, ASSIGNMENT_INPUT_INTENDED);
    assert_eq!(fixture.record().attempts, 0);
    assert_eq!(
        fixture.record().commit.as_deref(),
        Some("c".repeat(40).as_str())
    );
}

#[test]
fn a_builder_queues_nothing_and_an_unnamed_commit_queues_nothing() {
    let mut fixture = Fixture::new("not-required-and-unnamed");
    let mut builder = observation(&fixture);
    builder.assignee_role = Some("builder".to_string());
    let mut unnamed = observation(&fixture);
    unnamed.base_sha = None;
    let mut blank = observation(&fixture);
    blank.base_sha = Some("   ".to_string());

    let dispositions = queue_observed_assignments(&mut fixture.store, &[builder, unnamed, blank]);
    assert_eq!(
        dispositions,
        vec![
            CodingSessionAssignmentInputDisposition::NotRequired,
            CodingSessionAssignmentInputDisposition::Unnamed,
            CodingSessionAssignmentInputDisposition::Unnamed,
        ]
    );
    assert!(fixture.store.assignment_inputs.is_empty());
    assert!(pending_assignment_inputs(&fixture.store).is_empty());
}

#[test]
fn a_seat_this_host_never_cut_is_off_host_and_writes_no_record() {
    let mut fixture = Fixture::new("off-host");
    let mut elsewhere = observation(&fixture);
    elsewhere.seat_label = Some("Somebody-Else".to_string());
    let mut nameless = observation(&fixture);
    nameless.seat_label = None;

    let dispositions = queue_observed_assignments(&mut fixture.store, &[elsewhere, nameless]);
    assert_eq!(
        dispositions,
        vec![
            CodingSessionAssignmentInputDisposition::OffHost,
            CodingSessionAssignmentInputDisposition::OffHost,
        ]
    );
    assert!(fixture.store.assignment_inputs.is_empty());
}

#[test]
fn an_observation_this_host_cannot_read_is_invalid_rather_than_dropped() {
    let mut fixture = Fixture::new("invalid-observation");
    let mut short_id = observation(&fixture);
    short_id.assignment_id = "abc".to_string();
    let mut bad_commit = observation(&fixture);
    bad_commit.base_sha = Some("not-a-commit".to_string());

    let dispositions = queue_observed_assignments(&mut fixture.store, &[short_id, bad_commit]);
    assert_eq!(
        dispositions,
        vec![
            CodingSessionAssignmentInputDisposition::Invalid,
            CodingSessionAssignmentInputDisposition::Invalid,
        ]
    );
    assert!(fixture.store.assignment_inputs.is_empty());
}

#[test]
fn the_pending_words_are_the_ones_the_surface_reads() {
    // Pinned against serde and against the copy module: a rename here without
    // one there would show a state the surface cannot name.
    assert!(assignment_input_is_pending(ASSIGNMENT_INPUT_INTENDED));
    assert!(assignment_input_is_pending(ASSIGNMENT_INPUT_ESTABLISHING));
    assert!(!assignment_input_is_pending(ASSIGNMENT_INPUT_ABANDONED));
    assert!(!assignment_input_is_pending("established"));
    assert!(!assignment_input_is_pending("dirty_tree"));
    assert_eq!(
        serde_json::to_string(&CodingSessionAssignmentInputDisposition::OffHost)
            .expect("disposition serializes"),
        "\"off_host\""
    );
    assert_eq!(
        serde_json::to_string(&CodingSessionAssignmentInputDisposition::NotRequired)
            .expect("disposition serializes"),
        "\"not_required\""
    );
}
