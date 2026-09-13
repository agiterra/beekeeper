//! Tests for the seat hook installer, against real git repositories.
//!
//! Nothing here asserts about strings the code would have written. Every case
//! creates an actual repository under `target/l9-hook-fixtures/`, installs into
//! it, and then makes real commits and real pushes — because the failures this
//! lane is guarding against (config landing in the wrong file, a hook that
//! fails somebody's commit, a trailer invented out of nothing) are all
//! invisible to a test that only reads the planner's output.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::commands::project_git_exec::GIT_REPO_SELECTION_VARS;

const SEAT: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const ASSIGNMENT: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";

static FIXTURE_SEQ: AtomicUsize = AtomicUsize::new(0);

/// The repo root, from this crate's manifest directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// A fresh, gitignored directory for one test's repositories.
fn fixture_dir(name: &str) -> PathBuf {
    let seq = FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root()
        .join("target/l9-hook-fixtures")
        .join(format!("{name}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    dir
}

/// A git invocation for the fixtures: hermetic, and pointed at a throwaway
/// `HOME`/global config so a test can prove the real one is never touched.
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

/// Run git and fail loudly, so a broken fixture never looks like a finding.
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

/// A `config --get` that answers `None` when the key is unset.
fn config_get(cwd: &Path, home: &Path, args: &[&str]) -> Option<String> {
    let output = fixture_git(cwd, home, args);
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// One main repository with a commit, a linked seat worktree, and a throwaway
/// home directory — the shape the hire host actually produces.
struct Fixture {
    home: PathBuf,
    main: PathBuf,
    seat: PathBuf,
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
        let seat = root.join("seat");
        git_ok(
            &main,
            &home,
            &[
                "worktree",
                "add",
                "-q",
                seat.to_str().expect("seat path"),
                "-b",
                "lane/refuter",
            ],
        );
        Self { home, main, seat }
    }

    /// The install request the hire host would send for this seat.
    fn request(&self) -> InstallCodingSessionSeatHooksRequest {
        InstallCodingSessionSeatHooksRequest {
            worktree_path: self.seat.to_string_lossy().to_string(),
            seat_role: "Refuter".to_string(),
            seat_pubkey: SEAT.to_string(),
            keyfile_path: Some(self.home.join("seat.key").to_string_lossy().to_string()),
            signer_program: "git-sign-nostr".to_string(),
            assignment_id: Some(ASSIGNMENT.to_string()),
            session_ref: Some("session-1".to_string()),
            genesis_ref: Some("genesis-1".to_string()),
            channel_id: Some("c0ffee".to_string()),
            branch: Some("lane/refuter".to_string()),
        }
    }

    /// The bytes of the throwaway global config, to prove they never change.
    fn global_config(&self) -> String {
        std::fs::read_to_string(self.home.join("gitconfig")).expect("global config")
    }
}

/// Run one of the repo's hook scripts directly, the way git would.
fn run_hook(script: &str, cwd: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("scripts").join(script))
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
    command.output().expect("hook ran")
}

// ------------------------------------------------------------------ install

#[test]
fn install_writes_both_hooks_and_the_config_into_the_worktrees_own_scope() {
    let fixture = Fixture::new("scope");
    let report = install(&fixture.request()).expect("install");

    assert_eq!(report.wip_ref, "refs/heads/wip/refuter/0f1e2d3c");
    assert_eq!(
        report.hooks_written,
        vec!["post-commit", "prepare-commit-msg"]
    );
    assert_eq!(report.config_scope, "worktree");
    assert_eq!(report.signing, "keyfile");
    assert!(report.changed);

    let hooks_dir = PathBuf::from(&report.hooks_dir);
    for name in ["post-commit", "prepare-commit-msg"] {
        let path = hooks_dir.join(name);
        assert!(path.is_file(), "{name} was not written");
        assert!(is_executable(&path), "{name} is not executable");
    }

    // The seat reads its own arming...
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "buzz.wipShare"]
        )
        .as_deref(),
        Some("true")
    );
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "buzz.wipRef"]
        )
        .as_deref(),
        Some("refs/heads/wip/refuter/0f1e2d3c")
    );
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "user.signingkey"]
        )
        .as_deref(),
        Some(SEAT)
    );
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "nostr.keyfile"]
        )
        .as_deref(),
        Some(
            fixture
                .home
                .join("seat.key")
                .to_string_lossy()
                .to_string()
                .as_str()
        )
    );
    // ...and the person's own checkout reads none of it. `--local` in a linked
    // worktree writes the *shared* file; a seat armed that way would silently
    // arm the human's checkout too.
    assert_eq!(
        config_get(
            &fixture.main,
            &fixture.home,
            &["config", "--get", "buzz.wipShare"]
        ),
        None,
        "the seat's arming leaked into the shared repository config"
    );
    assert_eq!(
        config_get(
            &fixture.main,
            &fixture.home,
            &["config", "--get", "commit.gpgsign"]
        ),
        None,
        "the seat's signing config leaked into the shared repository config"
    );
}

#[test]
fn install_never_touches_the_global_config() {
    let fixture = Fixture::new("global");
    let before = fixture.global_config();
    install(&fixture.request()).expect("install");
    assert_eq!(
        fixture.global_config(),
        before,
        "the global config was written to"
    );
    // And structurally: every git process the installer spawns runs with the
    // global file switched off entirely.
    assert!(!fixture.global_config().contains("wipShare"));
}

#[test]
fn a_second_install_is_a_no_op() {
    let fixture = Fixture::new("idempotent");
    let first = install(&fixture.request()).expect("first install");
    assert!(first.changed);
    let second = install(&fixture.request()).expect("second install");
    assert!(!second.changed, "a second install reported a change");
    assert_eq!(first.wip_ref, second.wip_ref);
    assert_eq!(first.hooks_dir, second.hooks_dir);
    assert_eq!(first.config_scope, second.config_scope);
    assert_eq!(first.dispatch, second.dispatch);
}

#[test]
fn install_refuses_a_path_that_is_not_a_git_worktree() {
    let dir = fixture_dir("not-a-repo");
    let mut request = Fixture::new("not-a-repo-seed").request();
    request.worktree_path = dir.to_string_lossy().to_string();
    let error = install(&request).expect_err("must refuse");
    // The fixture directory sits inside this repository, so `rev-parse` finds a
    // git dir by walking up. Arming *that* checkout because a caller named a
    // subdirectory is the accident being refused here.
    assert!(
        error.contains("is not a git worktree") || error.contains("is not its root"),
        "unexpected refusal: {error}"
    );
    assert!(
        !dir.join("hooks").exists(),
        "hooks were written into a directory that is not a worktree"
    );
}

#[test]
fn install_refuses_a_seat_pubkey_that_is_not_hex_before_writing_anything() {
    let fixture = Fixture::new("bad-pubkey");
    let mut request = fixture.request();
    request.seat_pubkey = "nope".to_string();
    assert_eq!(
        install(&request).unwrap_err(),
        "seat pubkey must be 64 lowercase hex characters"
    );
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "buzz.wipShare"]
        ),
        None
    );
}

#[test]
fn install_points_git_at_the_seats_own_hooks() {
    let fixture = Fixture::new("dispatch");
    let report = install(&fixture.request()).expect("install");
    assert_eq!(report.dispatch, "worktree");
    // The seat's hooks path is the seat's own directory...
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "core.hooksPath"]
        )
        .as_deref(),
        Some(report.hooks_dir.as_str())
    );
    // ...and the person's checkout still resolves hooks the way it always did.
    assert_eq!(
        config_get(
            &fixture.main,
            &fixture.home,
            &["config", "--get", "core.hooksPath"]
        ),
        None
    );
}

// -------------------------------------------------------------- commit path

#[test]
fn a_real_commit_in_the_seat_gets_the_assignment_trailer_once() {
    let fixture = Fixture::new("trailer");
    install(&fixture.request()).expect("install");
    std::fs::write(fixture.seat.join("a.txt"), "a\n").expect("write");
    git_ok(&fixture.seat, &fixture.home, &["add", "a.txt"]);
    git_ok(
        &fixture.seat,
        &fixture.home,
        &["commit", "-q", "--no-gpg-sign", "-m", "first"],
    );
    let message = git_ok(&fixture.seat, &fixture.home, &["log", "-1", "--pretty=%B"]);
    assert_eq!(
        message
            .matches(&format!("Assignment: {ASSIGNMENT}"))
            .count(),
        1,
        "expected exactly one assignment trailer in:\n{message}"
    );
}

#[test]
fn a_commit_with_no_assignment_id_gets_no_trailer() {
    let fixture = Fixture::new("no-trailer");
    let mut request = fixture.request();
    request.assignment_id = None;
    install(&request).expect("install");
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "buzz.assignmentId"]
        ),
        None,
        "an assignment id was configured for a seat that has none"
    );
    std::fs::write(fixture.seat.join("b.txt"), "b\n").expect("write");
    git_ok(&fixture.seat, &fixture.home, &["add", "b.txt"]);
    git_ok(
        &fixture.seat,
        &fixture.home,
        &["commit", "-q", "--no-gpg-sign", "-m", "second"],
    );
    let message = git_ok(&fixture.seat, &fixture.home, &["log", "-1", "--pretty=%B"]);
    assert!(
        !message.contains("Assignment:"),
        "a trailer was invented for a commit nobody assigned:\n{message}"
    );
}

// --------------------------------------------------------------- push path

#[test]
fn the_post_commit_hook_does_nothing_when_sharing_is_off() {
    let fixture = Fixture::new("off");
    // No install: this is the human's checkout with the hook present and
    // `buzz.wipShare` unset, which is what `just hooks` alone produces.
    let output = run_hook("wip-post-commit.sh", &fixture.main, &fixture.home, &[]);
    assert!(output.status.success(), "the hook must always exit 0");
    let log = fixture.main.join(".git/buzz-wip-push.log");
    assert!(!log.exists(), "an inert hook still wrote a log line");
}

#[test]
fn the_post_commit_hook_pushes_head_to_the_wip_ref() {
    let fixture = Fixture::new("push");
    install(&fixture.request()).expect("install");
    let bare = fixture.seat.parent().expect("root").join("remote.git");
    git_ok(
        &fixture.main,
        &fixture.home,
        &["init", "-q", "--bare", bare.to_str().expect("bare path")],
    );
    git_ok(
        &fixture.seat,
        &fixture.home,
        &[
            "remote",
            "add",
            "anything",
            bare.to_str().expect("bare path"),
        ],
    );

    std::fs::write(fixture.seat.join("c.txt"), "c\n").expect("write");
    git_ok(&fixture.seat, &fixture.home, &["add", "c.txt"]);
    git_ok(
        &fixture.seat,
        &fixture.home,
        &["commit", "-q", "--no-gpg-sign", "-m", "shared"],
    );
    let head = git_ok(&fixture.seat, &fixture.home, &["rev-parse", "HEAD"]);

    let output = run_hook("wip-post-commit.sh", &fixture.seat, &fixture.home, &[]);
    assert!(output.status.success(), "the hook must always exit 0");

    // The commit is on the remote, under the derived ref and nowhere else.
    let refs = git_ok(
        &fixture.main,
        &fixture.home,
        &["ls-remote", bare.to_str().expect("bare")],
    );
    assert!(
        refs.contains(&format!("{head}\trefs/heads/wip/refuter/0f1e2d3c")),
        "expected the wip ref on the remote, got:\n{refs}"
    );
    assert!(
        !refs.contains("refs/heads/lane/refuter"),
        "the hook pushed the working branch as well:\n{refs}"
    );
}

#[test]
fn the_post_commit_hook_never_publishes_uncommitted_work() {
    let fixture = Fixture::new("uncommitted");
    install(&fixture.request()).expect("install");
    let bare = fixture.seat.parent().expect("root").join("remote.git");
    git_ok(
        &fixture.main,
        &fixture.home,
        &["init", "-q", "--bare", bare.to_str().expect("bare path")],
    );
    git_ok(
        &fixture.seat,
        &fixture.home,
        &[
            "remote",
            "add",
            "anything",
            bare.to_str().expect("bare path"),
        ],
    );
    // A secret sitting in the working tree, never committed.
    std::fs::write(fixture.seat.join("secret.txt"), "do not publish\n").expect("write");

    run_hook("wip-post-commit.sh", &fixture.seat, &fixture.home, &[]);

    // Read the ref from the remote it was pushed to: a push creates the ref
    // there, not in the pushing repository.
    let pushed = git_ok(
        &bare,
        &fixture.home,
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "refs/heads/wip/refuter/0f1e2d3c",
        ],
    );
    assert!(
        !pushed.contains("secret.txt"),
        "uncommitted work reached the wip ref:\n{pushed}"
    );
    // And the file is still sitting there, unstaged and untouched.
    assert!(fixture.seat.join("secret.txt").is_file());
    let status = git_ok(&fixture.seat, &fixture.home, &["status", "--porcelain"]);
    assert!(
        status.contains("?? secret.txt"),
        "the hook staged or stashed the working tree: {status}"
    );
}

#[test]
fn a_push_to_an_unreachable_remote_exits_zero_and_leaves_the_commit_alone() {
    let fixture = Fixture::new("unreachable");
    install(&fixture.request()).expect("install");
    let missing = fixture.seat.parent().expect("root").join("nowhere.git");
    git_ok(
        &fixture.seat,
        &fixture.home,
        &["remote", "add", "anything", missing.to_str().expect("path")],
    );
    std::fs::write(fixture.seat.join("d.txt"), "d\n").expect("write");
    git_ok(&fixture.seat, &fixture.home, &["add", "d.txt"]);
    git_ok(
        &fixture.seat,
        &fixture.home,
        &["commit", "-q", "--no-gpg-sign", "-m", "unreachable"],
    );
    let head = git_ok(&fixture.seat, &fixture.home, &["rev-parse", "HEAD"]);

    let output = run_hook("wip-post-commit.sh", &fixture.seat, &fixture.home, &[]);
    assert!(
        output.status.success(),
        "a failed push must never fail the commit; exit was {:?}",
        output.status.code()
    );
    assert_eq!(
        git_ok(&fixture.seat, &fixture.home, &["rev-parse", "HEAD"]),
        head,
        "the commit moved"
    );
    let log = std::fs::read_to_string(
        PathBuf::from(git_ok(
            &fixture.seat,
            &fixture.home,
            &["rev-parse", "--git-dir"],
        ))
        .join("buzz-wip-push.log"),
    )
    .expect("log");
    assert!(
        log.contains("the commit is local and untouched"),
        "the failure was not disclosed in the log:\n{log}"
    );
}

#[test]
fn the_post_commit_hook_refuses_a_ref_outside_the_wip_namespace() {
    let fixture = Fixture::new("namespace");
    install(&fixture.request()).expect("install");
    let bare = fixture.seat.parent().expect("root").join("remote.git");
    git_ok(
        &fixture.main,
        &fixture.home,
        &["init", "-q", "--bare", bare.to_str().expect("bare path")],
    );
    git_ok(
        &fixture.seat,
        &fixture.home,
        &[
            "remote",
            "add",
            "anything",
            bare.to_str().expect("bare path"),
        ],
    );
    // Somebody edits the config by hand and aims it at the trunk.
    git_ok(
        &fixture.seat,
        &fixture.home,
        &["config", "--worktree", "buzz.wipRef", "refs/heads/main"],
    );

    let output = run_hook("wip-post-commit.sh", &fixture.seat, &fixture.home, &[]);
    assert!(output.status.success());
    let refs = git_ok(
        &fixture.main,
        &fixture.home,
        &["ls-remote", bare.to_str().expect("bare")],
    );
    assert!(
        !refs.contains("refs/heads/main"),
        "the hook force-pushed outside the wip namespace:\n{refs}"
    );
    // The configured ref is not in the namespace, so the hook falls back to the
    // derived one — which is the only thing it may ever force-push.
    assert!(
        refs.is_empty() || refs.contains("refs/heads/wip/"),
        "unexpected refs on the remote:\n{refs}"
    );
}

#[test]
fn the_post_commit_hook_needs_no_remote_name_of_its_own() {
    // The single configured remote is deliberately not called `origin`,
    // `upstream`, or `vanilla`: CLAUDE.md forbids a hard-coded remote name and
    // two guards in this repo broke the day the names moved.
    let fixture = Fixture::new("remote-name");
    install(&fixture.request()).expect("install");
    let bare = fixture.seat.parent().expect("root").join("remote.git");
    git_ok(
        &fixture.main,
        &fixture.home,
        &["init", "-q", "--bare", bare.to_str().expect("bare path")],
    );
    git_ok(
        &fixture.seat,
        &fixture.home,
        &[
            "remote",
            "add",
            "a-name-nobody-hard-coded",
            bare.to_str().expect("bare path"),
        ],
    );
    let head = git_ok(&fixture.seat, &fixture.home, &["rev-parse", "HEAD"]);
    run_hook("wip-post-commit.sh", &fixture.seat, &fixture.home, &[]);
    let refs = git_ok(
        &fixture.main,
        &fixture.home,
        &["ls-remote", bare.to_str().expect("bare")],
    );
    assert!(refs.contains(&head), "nothing reached the remote:\n{refs}");
}

// ------------------------------------------------- prepare-commit-msg alone

#[test]
fn prepare_commit_msg_writes_nothing_without_an_assignment_id() {
    let fixture = Fixture::new("prepare-none");
    let message = fixture.seat.join("MSG");
    std::fs::write(&message, "just a message\n").expect("write");
    let output = run_hook(
        "wip-prepare-commit-msg.sh",
        &fixture.seat,
        &fixture.home,
        &[message.to_str().expect("msg path")],
    );
    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(&message).expect("read"),
        "just a message\n"
    );
}

#[test]
fn prepare_commit_msg_ignores_a_malformed_assignment_id() {
    let fixture = Fixture::new("prepare-malformed");
    git_ok(
        &fixture.seat,
        &fixture.home,
        &["config", "--local", "buzz.assignmentId", "not-an-event-id"],
    );
    let message = fixture.seat.join("MSG");
    std::fs::write(&message, "just a message\n").expect("write");
    run_hook(
        "wip-prepare-commit-msg.sh",
        &fixture.seat,
        &fixture.home,
        &[message.to_str().expect("msg path")],
    );
    assert_eq!(
        std::fs::read_to_string(&message).expect("read"),
        "just a message\n"
    );
}

#[test]
fn prepare_commit_msg_is_idempotent() {
    let fixture = Fixture::new("prepare-twice");
    install(&fixture.request()).expect("install");
    let message = fixture.seat.join("MSG");
    std::fs::write(&message, "subject line\n").expect("write");
    for _ in 0..2 {
        run_hook(
            "wip-prepare-commit-msg.sh",
            &fixture.seat,
            &fixture.home,
            &[message.to_str().expect("msg path")],
        );
    }
    let written = std::fs::read_to_string(&message).expect("read");
    assert_eq!(
        written
            .matches(&format!("Assignment: {ASSIGNMENT}"))
            .count(),
        1,
        "the trailer was written twice:\n{written}"
    );
}

// ------------------------------------------------- a seat with no key file

#[test]
fn the_only_lines_an_unsigned_seat_loses_are_the_signing_ones() {
    // The unsigned install keeps `buzz.*` and drops the rest. This is the
    // assertion that says what "the rest" is today: a line added to
    // `plan_seat_git_hooks` that is neither `buzz.*` nor one of these fails
    // here rather than quietly disappearing from every unsigned seat.
    let plan = plan_seat_git_hooks(&SeatGitHookRequest {
        seat_role: "Refuter".to_string(),
        seat_pubkey: SEAT.to_string(),
        keyfile_path: Some("/somewhere/seat.key".to_string()),
        signer_program: "git-sign-nostr".to_string(),
        assignment_id: Some(ASSIGNMENT.to_string()),
        session_ref: Some("session-1".to_string()),
        genesis_ref: Some("genesis-1".to_string()),
        channel_id: Some("c0ffee".to_string()),
        branch: Some("lane/refuter".to_string()),
    })
    .expect("plan");
    let dropped: Vec<&str> = plan
        .config
        .iter()
        .map(|line| line.key.as_str())
        .filter(|key| !key.starts_with(SHARING_CONFIG_PREFIX))
        .collect();
    assert_eq!(dropped, SIGNING_CONFIG_KEYS.to_vec());
}

#[test]
fn a_seat_whose_key_file_this_host_cannot_name_is_still_armed_to_share() {
    let fixture = Fixture::new("unsigned");
    let mut request = fixture.request();
    request.keyfile_path = None;
    let report = install(&request).expect("install");

    // The push half is installed in full.
    assert_eq!(report.signing, "unsigned");
    assert_eq!(report.wip_ref, "refs/heads/wip/refuter/0f1e2d3c");
    assert_eq!(
        report.hooks_written,
        vec!["post-commit", "prepare-commit-msg"]
    );
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "buzz.wipShare"]
        )
        .as_deref(),
        Some("true")
    );
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "buzz.wipRef"]
        )
        .as_deref(),
        Some("refs/heads/wip/refuter/0f1e2d3c")
    );

    // The signing half is absent entirely — not a placeholder, not the
    // operator's own key file.
    for key in SIGNING_CONFIG_KEYS {
        assert_eq!(
            config_get(&fixture.seat, &fixture.home, &["config", "--get", key]),
            None,
            "{key} was configured for a seat whose key file this host cannot name"
        );
    }
    let listed = git_ok(&fixture.seat, &fixture.home, &["config", "--list"]);
    assert!(
        !listed.contains("cannot name"),
        "a placeholder key file path reached the config:\n{listed}"
    );
}

#[test]
fn an_unsigned_seat_can_still_commit_and_its_commit_still_reaches_the_wip_ref() {
    // The reason the signing lines are dropped rather than pointed at a path
    // that does not exist: `commit.gpgsign = true` with no key and no
    // `git-sign-nostr` on PATH fails *every* commit the seat makes, which is
    // strictly worse than a seat whose commits are unsigned.
    let fixture = Fixture::new("unsigned-commit");
    let mut request = fixture.request();
    request.keyfile_path = None;
    install(&request).expect("install");

    let bare = fixture.seat.parent().expect("root").join("remote.git");
    git_ok(
        &fixture.main,
        &fixture.home,
        &["init", "-q", "--bare", bare.to_str().expect("bare path")],
    );
    git_ok(
        &fixture.seat,
        &fixture.home,
        &[
            "remote",
            "add",
            "anything",
            bare.to_str().expect("bare path"),
        ],
    );

    std::fs::write(fixture.seat.join("e.txt"), "e\n").expect("write");
    git_ok(&fixture.seat, &fixture.home, &["add", "e.txt"]);
    // Deliberately no `--no-gpg-sign`: this is the seat's own `git commit`.
    git_ok(
        &fixture.seat,
        &fixture.home,
        &["commit", "-q", "-m", "plain"],
    );
    let head = git_ok(&fixture.seat, &fixture.home, &["rev-parse", "HEAD"]);

    let output = run_hook("wip-post-commit.sh", &fixture.seat, &fixture.home, &[]);
    assert!(output.status.success(), "the hook must always exit 0");
    let refs = git_ok(
        &fixture.main,
        &fixture.home,
        &["ls-remote", bare.to_str().expect("bare")],
    );
    assert!(
        refs.contains(&format!("{head}\trefs/heads/wip/refuter/0f1e2d3c")),
        "an unsigned seat shared nothing:\n{refs}"
    );
}

#[test]
fn a_blank_key_file_path_is_treated_as_no_key_file_rather_than_a_refusal() {
    let fixture = Fixture::new("blank-key");
    let mut request = fixture.request();
    request.keyfile_path = Some("   ".to_string());
    let report = install(&request).expect("install");
    assert_eq!(report.signing, "unsigned");
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "commit.gpgsign"]
        ),
        None
    );
}

/// Serialises this file's poisoned-environment test against any other test
/// in the same binary that also mutates process environment.
static GIT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A hermetic `git init`, independent of `Fixture` — this test's two
/// repositories are OS-temp throwaways, never anything under this worktree's
/// `target/`, so a bug here can only corrupt its own fixtures.
fn init_bare_local_repo(dir: &Path) {
    let mut command = Command::new("git");
    command
        .args(["init", "-q", "--initial-branch=main"])
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    let status = command.status().expect("git init");
    assert!(status.success(), "git init failed in {}", dir.display());
}

fn read_local_config(dir: &Path, key: &str) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["config", "--local", "--get", key])
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    let output = command.output().expect("git config --get");
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Finding 46's class of bug, reproduced against the seat-hook installer's
/// own `git()` — the same one `install()` uses to write every
/// `buzz.*`/signing config line into a seat's worktree.
///
/// `git()` clears every repository-selection variable through
/// `GIT_REPO_SELECTION_VARS`. This poisons only `GIT_COMMON_DIR` to preserve
/// the regression boundary: that variable alone, with no `GIT_DIR`, is enough
/// for git to resolve `--local` config against a repository other than the one
/// named by `cwd`. The write must still land in the given worktree and nowhere
/// else.
#[test]
fn a_config_write_addresses_the_worktree_it_was_given_not_a_poisoned_git_common_dir() {
    let _guard = GIT_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());

    let poison = tempfile::tempdir().expect("poison tempdir");
    let target = tempfile::tempdir().expect("target tempdir");
    init_bare_local_repo(poison.path());
    init_bare_local_repo(target.path());

    let original = std::env::var_os("GIT_COMMON_DIR");
    // SAFETY-EQUIVALENT: single-threaded section guarded by `GIT_ENV_LOCK`.
    std::env::set_var("GIT_COMMON_DIR", poison.path().join(".git"));

    let result = git(
        target.path(),
        &["config", "--local", "buzz.l25marker", "target-value"],
    );

    match original {
        Some(value) => std::env::set_var("GIT_COMMON_DIR", value),
        None => std::env::remove_var("GIT_COMMON_DIR"),
    }

    result.expect("git config write must succeed");
    assert_eq!(
        read_local_config(target.path(), "buzz.l25marker").as_deref(),
        Some("target-value"),
        "the worktree named by cwd must receive the write"
    );
    assert_eq!(
        read_local_config(poison.path(), "buzz.l25marker"),
        None,
        "a poisoned GIT_COMMON_DIR must not receive a write meant for the target worktree"
    );
}
