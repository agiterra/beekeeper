//! The branch authority's storage contract (ledger 275 A1): it holds, or it
//! refuses — never a silent fall back to the child-writable `HEAD`.

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

/// A test-owned root: provider state, a repository and a seat worktree on
/// `seat-branch`.
struct Fx {
    _dir: tempfile::TempDir,
    state: PathBuf,
    seat: PathBuf,
}

fn fx() -> Fx {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    let state = root.join("state");
    std::fs::create_dir_all(&state).expect("state");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let seat = root.join("seat");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "seat-branch",
            seat.to_str().expect("utf8"),
        ],
    );
    Fx {
        _dir: dir,
        state,
        seat,
    }
}

#[test]
fn the_first_preparation_pins_head_and_later_ones_keep_the_pin() {
    let fx = fx();
    assert_eq!(
        branch_authority(&fx.state, &fx.seat, None),
        Ok(Some("seat-branch".to_owned()))
    );
    // A child re-points HEAD; the pin decides.
    git(&fx.seat, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    assert_eq!(
        branch_authority(&fx.state, &fx.seat, None),
        Ok(Some("seat-branch".to_owned()))
    );
}

/// Astra's A1 reproduction: a file where the pins directory must be. The
/// preparation refuses instead of granting from HEAD.
#[test]
fn a_pin_that_cannot_be_stored_refuses() {
    let fx = fx();
    std::fs::write(fx.state.join(BRANCH_PINS_DIR), "blocks the pin directory").expect("block");
    let error = branch_authority(&fx.state, &fx.seat, None).expect_err("refused");
    assert!(error.0.contains("branch pin"), "{error:?}");
}

#[test]
fn an_empty_or_corrupt_pin_refuses() {
    for content in ["", "\n", "../main", "a b", "main.lock"] {
        let fx = fx();
        let pin = pin_path(&fx.state, &fx.seat);
        std::fs::create_dir_all(pin.parent().expect("dir")).expect("dir");
        std::fs::write(&pin, content).expect("pin");
        git(&fx.seat, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        let error = branch_authority(&fx.state, &fx.seat, None).expect_err(content);
        assert!(error.0.contains("branch pin"), "{content:?}: {error:?}");
    }
}

#[test]
fn an_unreadable_pin_refuses() {
    let fx = fx();
    let pin = pin_path(&fx.state, &fx.seat);
    // A directory where the pin file must be: reading it fails, not "absent".
    std::fs::create_dir_all(&pin).expect("dir");
    assert!(branch_authority(&fx.state, &fx.seat, None).is_err());
}

#[test]
fn concurrent_first_preparations_agree_on_one_pin() {
    let fx = fx();
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| branch_authority(&fx.state, &fx.seat, None)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });
    for result in results {
        assert_eq!(result, Ok(Some("seat-branch".to_owned())));
    }
    let leftovers: Vec<_> = std::fs::read_dir(fx.state.join(BRANCH_PINS_DIR))
        .expect("pins")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temporary files remain: {leftovers:?}"
    );
}

/// The host's own worktree record decides over HEAD and over any pin.
#[test]
fn the_hosts_worktree_record_is_the_authority() {
    let fx = fx();
    let store = fx.state.join("coding-session-workdirs.json");
    std::fs::write(
        &store,
        serde_json::json!({
            "version": 2,
            "worktrees": { "session/builder": {
                "path": fx.seat,
                "branch": "seat-branch",
                "repoRoot": fx.state,
                "createdAt": "2026-09-28T00:00:00Z",
            }},
        })
        .to_string(),
    )
    .expect("store");
    std::fs::write(
        fx.state
            .join(crate::assignment_inputs::HOST_STORE_POINTER_FILE),
        serde_json::json!({
            "version": crate::assignment_inputs::HOST_STORE_POINTER_VERSION,
            "path": store,
        })
        .to_string(),
    )
    .expect("pointer");
    git(&fx.seat, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    assert_eq!(
        branch_authority(&fx.state, &fx.seat, Some("main")),
        Ok(Some("seat-branch".to_owned()))
    );
    // An unreadable host record refuses rather than falling through.
    std::fs::write(&store, "{ not json").expect("corrupt");
    assert!(branch_authority(&fx.state, &fx.seat, None).is_err());
}

#[test]
fn a_detached_tree_nobody_records_gets_no_branch() {
    let fx = fx();
    git(&fx.seat, &["checkout", "-q", "--detach"]);
    assert_eq!(branch_authority(&fx.state, &fx.seat, None), Ok(None));
}
