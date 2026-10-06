//! The scratch index is reset to `HEAD` before staging, so the person's own
//! index — staged edits, an unfinished merge — never reaches a checkpoint
//! tree for a path outside the session's `cwd`. Every repository here is a
//! throwaway created by the test in its own temporary directory.

use super::*;
use crate::execution_scope_host::UNBOUNDED_FOR_TESTS;

const SESSION: &str = "sess-index";

/// `git` in `cwd`, hermetic; returns success and stdout.
fn git_status(cwd: &Path, args: &[&str]) -> (bool, String) {
    let mut command = std::process::Command::new("git");
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    let output = command
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    )
}

/// `git` in `cwd`, asserting success.
fn git(cwd: &Path, args: &[&str]) -> String {
    let (ok, out) = git_status(cwd, args);
    assert!(ok, "git {args:?} failed");
    out
}

fn write(root: &Path, name: &str, body: &str) {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("dirs");
    }
    std::fs::write(path, body).expect("write");
}

/// A repository on `main` with `top.txt` and `sub/inner.txt`, one commit.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main", "."]);
    write(root, "top.txt", "top\n");
    write(root, "sub/inner.txt", "inner\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "--no-gpg-sign", "-m", "init"]);
    dir
}

async fn capture_from(cwd: &Path, leaf: RefLeaf<'_>) -> Result<CapturedTree, CaptureFailure> {
    capture_tree(
        cwd,
        Some(&UNBOUNDED_FOR_TESTS),
        SESSION,
        1,
        leaf,
        None,
        CapturePhase::TurnEnd,
    )
    .await
}

fn blob(root: &Path, tree: &str, path: &str) -> String {
    git(root, &["cat-file", "-p", &format!("{tree}:{path}")])
}

#[tokio::test]
async fn an_unmerged_path_outside_a_subdirectory_cwd_still_captures() {
    let dir = repo();
    let root = dir.path();
    git(root, &["checkout", "-q", "-b", "side"]);
    write(root, "top.txt", "side\n");
    git(root, &["commit", "-q", "--no-gpg-sign", "-am", "side"]);
    git(root, &["checkout", "-q", "main"]);
    write(root, "top.txt", "main\n");
    git(root, &["commit", "-q", "--no-gpg-sign", "-am", "main"]);
    let (merged, _) = git_status(root, &["merge", "-q", "--no-edit", "side"]);
    assert!(!merged, "the fixture needs a conflict");
    assert!(
        !git(root, &["ls-files", "-u"]).is_empty(),
        "top.txt is unmerged in the person's index"
    );
    write(root, "sub/inner.txt", "edited in the turn\n");

    let captured = capture_from(&root.join("sub"), RefLeaf::Through(1))
        .await
        .expect("an unmerged path outside cwd does not stop the capture");
    assert_eq!(blob(root, &captured.tree, "top.txt"), "main");
    assert_eq!(
        blob(root, &captured.tree, "sub/inner.txt"),
        "edited in the turn"
    );
    assert!(
        !git(root, &["ls-files", "-u"]).is_empty(),
        "the person's index is untouched"
    );
}

#[tokio::test]
async fn content_staged_outside_cwd_is_not_in_the_tree_or_the_turn() {
    let dir = repo();
    let root = dir.path();
    let sub = root.join("sub");
    let base = capture_from(&sub, RefLeaf::Base(1))
        .await
        .expect("baseline");

    // The person stages an edit outside the session's cwd mid-turn; the
    // session edits inside it.
    write(root, "top.txt", "staged by the person\n");
    git(root, &["add", "top.txt"]);
    write(root, "sub/inner.txt", "edited in the turn\n");

    let end = capture_from(&sub, RefLeaf::Through(1))
        .await
        .expect("end capture");
    assert_eq!(blob(root, &end.tree, "top.txt"), "top");
    let changed = diff_tree_files(root, Some(&UNBOUNDED_FOR_TESTS), &base.tree, &end.tree)
        .await
        .expect("diff");
    let paths: Vec<&str> = changed
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, vec!["sub/inner.txt"]);
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only"]),
        "top.txt",
        "the person's staged edit is still staged"
    );
}

/// A repository like [`repo`] with `far/away.txt` committed, then made a
/// sparse checkout of `sub` alone (with a sparse index when asked).
fn sparse_repo(sparse_index: bool) -> tempfile::TempDir {
    let dir = repo();
    let root = dir.path();
    write(root, "far/away.txt", "away\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "--no-gpg-sign", "-m", "far"]);
    let index_flag = if sparse_index {
        "--sparse-index"
    } else {
        "--no-sparse-index"
    };
    git(
        root,
        &["sparse-checkout", "set", "--cone", index_flag, "sub"],
    );
    assert!(
        !root.join("far/away.txt").exists(),
        "the fixture leaves far/ out of the checkout"
    );
    dir
}

/// SV-51: without `--sparse`, `add -A` exits 1 on an untracked file outside
/// the sparse-checkout definition and nothing is captured. With it, the file
/// is captured, and a file the checkout leaves out keeps its `HEAD` content
/// rather than reading as deleted.
#[tokio::test]
async fn a_sparse_checkout_with_a_file_outside_the_cone_still_captures() {
    for sparse_index in [false, true] {
        let dir = sparse_repo(sparse_index);
        let root = dir.path();
        write(root, "far/stray.txt", "stray\n");
        write(root, "sub/inner.txt", "edited in the turn\n");

        let captured = capture_from(root, RefLeaf::Through(1))
            .await
            .unwrap_or_else(|failure| {
                panic!(
                    "sparse index {sparse_index}: no capture: {}",
                    failure.sentence
                )
            });
        assert_eq!(blob(root, &captured.tree, "far/away.txt"), "away");
        assert_eq!(blob(root, &captured.tree, "far/stray.txt"), "stray");
        assert_eq!(
            blob(root, &captured.tree, "sub/inner.txt"),
            "edited in the turn"
        );
        assert_eq!(blob(root, &captured.tree, "top.txt"), "top");
        assert!(captured.complete);
        assert_eq!(
            git(root, &["status", "--porcelain", "--untracked-files=no"]),
            "M sub/inner.txt",
            "sparse index {sparse_index}: the person's index is untouched"
        );
    }
}

/// A sparse checkout whose index cannot be copied would be seeded by a fresh
/// `read-tree HEAD` with no skip-worktree marks, recording every file outside
/// the cone as deleted; it is refused instead.
#[cfg(unix)]
#[tokio::test]
async fn a_sparse_checkout_whose_index_cannot_be_copied_is_refused_not_captured_wrong() {
    use std::os::unix::fs::PermissionsExt;
    let dir = sparse_repo(false);
    let root = dir.path();
    let index = root.join(".git/index");
    std::fs::set_permissions(&index, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::File::open(&index).is_ok() {
        // Running as a user that reads anything (root): no fixture to make.
        return;
    }
    let result = capture_from(root, RefLeaf::Through(1)).await;
    std::fs::set_permissions(&index, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let failure = result.expect_err("refused");
    assert_eq!(failure.code, UnavailableCode::GitFailed);
    assert!(
        failure.sentence.contains("sparse checkout"),
        "{}",
        failure.sentence
    );
    assert!(git(root, &["for-each-ref", CHECKPOINT_REF_ROOT]).is_empty());
}
