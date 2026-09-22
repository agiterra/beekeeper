//! The git identity a seat's worktree gets, against real git repositories.
//!
//! A child of the hook installer's own test module so it reuses that fixture:
//! a real main repository, a real **linked** worktree, and a throwaway `HOME`.
//! That shape is the defect verbatim — a tree cut by `git worktree add`
//! inherits no `user.name`/`user.email` at any scope, and the seat that found
//! it empty stopped and asked a founder (ledger 236(a), 239).
//!
//! These are not string tests over the planner: each case reads the value back
//! out of git, in the scope git would resolve it from.

use super::*;

/// The identity the fixture's seat should end up with: its own key, its role
/// and its project. Spelled out here rather than derived, so a change to the
/// derivation has to be a decision somebody makes.
const EXPECTED_NAME: &str = "refuter · kettle-control";
const EXPECTED_EMAIL: &str = "a1b2c3d4@beekeeper.local";

/// The state the control run found: a freshly cut linked worktree answers with
/// nothing for `user.email`, at every scope git would look in.
///
/// This is the premise of everything below. If it ever stops being true the
/// rest of these tests would pass for the wrong reason.
#[test]
fn a_freshly_cut_worktree_has_no_identity_of_its_own() {
    let fixture = Fixture::new("identity-absent");
    for scope in [
        vec!["config", "--get", "user.email"],
        vec!["config", "--local", "--get", "user.email"],
        vec!["config", "--get", "user.name"],
    ] {
        assert_eq!(
            config_get(&fixture.seat, &fixture.home, &scope),
            None,
            "a cut worktree already had {scope:?}"
        );
    }
}

/// The fix: the host writes the identity, at the linked worktree's own scope,
/// and git resolves it there.
#[test]
fn the_host_gives_a_seat_worktree_an_identity_at_worktree_scope() {
    let fixture = Fixture::new("identity-written");
    let (identity, scope) =
        ensure_seat_commit_identity(&fixture.seat, SEAT, "Refuter", Some("kettle-control"))
            .expect("the host configures the tree it cut");
    assert_eq!(identity.name, EXPECTED_NAME);
    assert_eq!(identity.email, EXPECTED_EMAIL);
    // A linked worktree: `--local` is the *shared* config, so the lines must
    // be in the per-worktree file or configuring one seat re-authors the
    // person's own checkout.
    assert_eq!(scope, "worktree");
    for (key, expected) in [("user.name", EXPECTED_NAME), ("user.email", EXPECTED_EMAIL)] {
        assert_eq!(
            config_get(
                &fixture.seat,
                &fixture.home,
                &["config", "--worktree", "--get", key]
            )
            .as_deref(),
            Some(expected),
            "{key} is not at worktree scope"
        );
        // And what git itself resolves, which is the only thing a commit uses.
        assert_eq!(
            config_get(&fixture.seat, &fixture.home, &["config", "--get", key]).as_deref(),
            Some(expected),
            "git does not resolve {key} in the seat's tree"
        );
    }
    // The person's own checkout is untouched: the whole point of the scope.
    for key in ["user.name", "user.email"] {
        assert_eq!(
            config_get(&fixture.main, &fixture.home, &["config", "--get", key]),
            None,
            "configuring a seat re-authored the operator's own checkout"
        );
    }
}

/// The identity has to survive into an actual commit — the thing the seat was
/// blocked on. A worktree with no `GIT_AUTHOR_*` in its environment and no
/// identity in its config cannot commit at all; with one it can.
#[test]
fn a_seat_can_commit_under_the_identity_the_host_configured() {
    let fixture = Fixture::new("identity-commits");
    ensure_seat_commit_identity(&fixture.seat, SEAT, "Refuter", Some("kettle-control"))
        .expect("configured");
    std::fs::write(fixture.seat.join("work.txt"), "done\n").expect("seat writes");
    // `commit -a` stages tracked changes only, so the new file is added first.
    git_ok(&fixture.seat, &fixture.home, &["add", "work.txt"]);

    // Deliberately without the fixture's `GIT_AUTHOR_*`/`GIT_COMMITTER_*`
    // overrides: those would supply an identity the config never had to.
    let mut command = std::process::Command::new("git");
    command
        .args(["commit", "-q", "--no-gpg-sign", "-a", "-m", "seat work"])
        .current_dir(&fixture.seat)
        .env("HOME", &fixture.home)
        .env("GIT_CONFIG_GLOBAL", fixture.home.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    let output = command.output().expect("git ran");
    assert!(
        output.status.success(),
        "the seat could not commit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git_ok(
            &fixture.seat,
            &fixture.home,
            &["log", "-1", "--format=%an <%ae>"]
        ),
        format!("{EXPECTED_NAME} <{EXPECTED_EMAIL}>")
    );
}

/// Idempotent: a second pass — the hook installer's, after the cut's — writes
/// nothing new and reports the same identity.
#[test]
fn configuring_the_same_seat_twice_is_the_same_identity() {
    let fixture = Fixture::new("identity-twice");
    let first = ensure_seat_commit_identity(&fixture.seat, SEAT, "Refuter", Some("kettle-control"))
        .expect("first");
    let second =
        ensure_seat_commit_identity(&fixture.seat, SEAT, "Refuter", Some("kettle-control"))
            .expect("second");
    assert_eq!(first, second);
}

/// A seat with no nameable project still gets a name. An empty `user.name` is
/// the state that produced the defect, so no input may yield one.
#[test]
fn a_seat_with_no_project_still_gets_a_name_and_an_address() {
    let fixture = Fixture::new("identity-no-project");
    let (identity, _) =
        ensure_seat_commit_identity(&fixture.seat, SEAT, "runner", None).expect("configured");
    assert_eq!(identity.name, "runner seat");
    assert_eq!(identity.email, EXPECTED_EMAIL);
    assert!(!identity.name.trim().is_empty());
}

/// The path handed in must be the worktree's own root. `rev-parse` walks up,
/// so a subdirectory would otherwise re-author the enclosing checkout.
#[test]
fn a_subdirectory_is_refused_rather_than_configuring_the_tree_above_it() {
    let fixture = Fixture::new("identity-subdir");
    let inside = fixture.seat.join("src");
    std::fs::create_dir_all(&inside).expect("subdir");
    let error = ensure_seat_commit_identity(&inside, SEAT, "Refuter", Some("kettle-control"))
        .expect_err("a subdirectory must be refused");
    assert!(error.contains("is not its root"), "{error}");
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "user.email"]
        ),
        None,
        "a refused call still wrote to the tree"
    );
}

/// An unsigned seat — the ordinary case on this machine, since a seat's key is
/// never on disk — keeps its identity. The installer drops every non-`buzz.`
/// line from the *plan* when there is no key file, and the identity must not
/// be one of them: a seat that cannot be named cannot commit at all.
#[test]
fn an_unsigned_seat_still_gets_its_commit_identity() {
    let fixture = Fixture::new("identity-unsigned");
    let mut request = fixture.request();
    request.keyfile_path = None;
    let installed = install(&request).expect("install");
    assert_eq!(installed.signing, "unsigned");
    assert_eq!(installed.commit_identity_name, EXPECTED_NAME);
    assert_eq!(installed.commit_identity_email, EXPECTED_EMAIL);
    // Signing really was dropped, so this is not passing because the filter
    // stopped running.
    assert_eq!(
        config_get(
            &fixture.seat,
            &fixture.home,
            &["config", "--get", "commit.gpgsign"]
        ),
        None
    );
    for (key, expected) in [("user.name", EXPECTED_NAME), ("user.email", EXPECTED_EMAIL)] {
        assert_eq!(
            config_get(&fixture.seat, &fixture.home, &["config", "--get", key]).as_deref(),
            Some(expected),
            "the unsigned install left {key} unset"
        );
    }
}

/// The installer reports the identity it wrote, for a signed seat too.
#[test]
fn the_installer_reports_the_identity_it_configured() {
    let fixture = Fixture::new("identity-reported");
    let installed = install(&fixture.request()).expect("install");
    assert_eq!(installed.commit_identity_name, EXPECTED_NAME);
    assert_eq!(installed.commit_identity_email, EXPECTED_EMAIL);
    assert_eq!(installed.config_scope, "worktree");
}

/// Never a person's address. The one key file on this machine holds the
/// operator's identity, and a seat committing under it is a forged
/// attribution — so the derivation can only ever produce the seat's own key.
#[test]
fn no_seat_is_ever_configured_with_a_persons_address() {
    let fixture = Fixture::new("identity-not-a-person");
    let (identity, _) =
        ensure_seat_commit_identity(&fixture.seat, SEAT, "Refuter", Some("kettle-control"))
            .expect("configured");
    assert!(identity.email.ends_with("@beekeeper.local"), "{identity:?}");
    assert!(identity.email.starts_with(&SEAT[..8]), "{identity:?}");
}
