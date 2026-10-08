//! SV-29 `files: restore` against throwaway repositories, each created by the
//! test in its own temporary directory and deleted with it — never this
//! checkout.

use super::super::{capture_tree, CapturePhase, RefLeaf, MAX_UNTRACKED_FILE_BYTES};
use super::*;
use crate::execution_scope_host::UNBOUNDED_FOR_TESTS;

const COMMAND: &str = "abababababababababababababababababababababababababababababababab";

fn git(cwd: &Path, args: &[&str]) -> String {
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
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write(root: &Path, name: &str, body: &str) {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("dirs");
    }
    std::fs::write(path, body).expect("write");
}

/// A repository with `a.txt`, `keep/b.txt`, `gone.txt`, a `.gitignore`
/// for `*.log`, and one commit.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main", "."]);
    write(root, "a.txt", "one\n");
    write(root, "keep/b.txt", "bee\n");
    write(root, "gone.txt", "will be deleted by a turn\n");
    write(root, ".gitignore", "*.log\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "--no-gpg-sign", "-m", "init"]);
    dir
}

async fn capture(root: &Path, leaf: RefLeaf<'_>) -> super::super::CapturedTree {
    capture_tree(
        root,
        Some(&UNBOUNDED_FOR_TESTS),
        "sess-1",
        1,
        leaf,
        None,
        CapturePhase::PreRewind,
    )
    .await
    .expect("capture")
}

#[tokio::test]
async fn rewind_restore_returns_the_base_tree_and_touches_nothing_else() {
    let dir = repo();
    let root = dir.path();
    let base = capture(root, RefLeaf::Base(1)).await;

    // What the rewound turns did: modify, delete, add (nested), add an
    // ignored file, stage a new file, and write an untracked file too large
    // to capture.
    write(root, "a.txt", "changed by turn 3\n");
    std::fs::remove_file(root.join("gone.txt")).expect("rm");
    write(root, "new.txt", "added\n");
    write(root, "deep/er/x.txt", "added deep\n");
    write(root, "keep/added.txt", "added beside a kept file\n");
    write(root, "build.log", "ignored, survives\n");
    write(root, "staged.txt", "staged\n");
    git(root, &["add", "staged.txt"]);
    let big = root.join("huge.bin");
    let file = std::fs::File::create(&big).expect("big");
    file.set_len(MAX_UNTRACKED_FILE_BYTES + 1).expect("size");
    drop(file);

    let index_before = std::fs::read(root.join(".git/index")).expect("index");
    let staged_before = git(root, &["ls-files", "-s"]);
    let head_before = git(root, &["rev-parse", "HEAD"]);
    let refs_before = git(root, &["for-each-ref", "refs/heads"]);

    let pre = capture(
        root,
        RefLeaf::PreRewind {
            through_seq: 9,
            command: COMMAND,
        },
    )
    .await;
    let omitted: Vec<String> = pre.omitted.iter().map(|path| path.path.clone()).collect();
    assert_eq!(omitted, vec!["huge.bin".to_owned()]);

    let report = restore_tree(RestoreRequest {
        cwd: root,
        scope: Some(&UNBOUNDED_FOR_TESTS),
        base_tree: &base.tree,
        pre_tree: &pre.tree,
        omitted: &omitted,
    })
    .await
    .expect("restore");

    assert_eq!(report.removed, 4, "{report:?}");
    assert!(report.left.is_empty(), "{report:?}");
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    assert!(root.join("gone.txt").exists(), "a deleted file comes back");
    assert!(!root.join("new.txt").exists());
    assert!(
        !root.join("deep").exists(),
        "empty parents the turns made go"
    );
    assert!(!root.join("keep/added.txt").exists());
    assert!(root.join("keep/b.txt").exists(), "a kept directory stays");
    assert!(!root.join("staged.txt").exists(), "added since the base");
    assert!(
        root.join("build.log").exists(),
        "ignored paths are untouched"
    );
    assert!(big.exists(), "omitted paths are untouched");

    assert_eq!(
        std::fs::read(root.join(".git/index")).expect("index"),
        index_before,
        "the real index is byte-identical"
    );
    assert_eq!(git(root, &["ls-files", "-s"]), staged_before);
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(root, &["for-each-ref", "refs/heads"]), refs_before);
    let leftovers: Vec<_> = std::fs::read_dir(root.join(".git"))
        .expect("git dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("rewind-index"))
        .collect();
    assert!(leftovers.is_empty(), "no scratch index is left behind");
}

#[tokio::test]
async fn rewind_restore_never_deletes_through_a_symlinked_directory() {
    let dir = repo();
    let root = dir.path();
    let outside = tempfile::tempdir().expect("outside");
    let base = capture(root, RefLeaf::Base(1)).await;
    write(root, "d/x.txt", "added by a turn\n");
    let pre = capture(
        root,
        RefLeaf::PreRewind {
            through_seq: 9,
            command: COMMAND,
        },
    )
    .await;
    // After the capture, `d` is swapped for a link to a directory outside
    // the tree holding a file of the same name.
    std::fs::remove_dir_all(root.join("d")).expect("rm d");
    write(outside.path(), "x.txt", "not the session's\n");
    std::os::unix::fs::symlink(outside.path(), root.join("d")).expect("link");

    let report = restore_tree(RestoreRequest {
        cwd: root,
        scope: Some(&UNBOUNDED_FOR_TESTS),
        base_tree: &base.tree,
        pre_tree: &pre.tree,
        omitted: &[],
    })
    .await
    .expect("restore");
    assert_eq!(report.left, vec!["d/x.txt".to_owned()]);
    assert!(
        outside.path().join("x.txt").exists(),
        "nothing outside cwd goes"
    );
}

#[tokio::test]
async fn rewind_objects_present_answers_for_both_objects() {
    let dir = repo();
    let root = dir.path();
    let base = capture(root, RefLeaf::Base(1)).await;
    let scope = Some(&UNBOUNDED_FOR_TESTS);
    assert!(objects_present(root, scope, &base.tree, &base.commit)
        .await
        .unwrap());
    // A tree is not a commit, and an absent id is absent.
    assert!(!objects_present(root, scope, &base.tree, &base.tree)
        .await
        .unwrap());
    assert!(!objects_present(root, scope, &"1".repeat(40), &base.commit)
        .await
        .unwrap());
    assert!(objects_present(root, None, &base.tree, &base.commit)
        .await
        .is_err());
}

#[test]
fn rewind_paths_outside_cwd_are_never_joined() {
    let cwd = Path::new("/tmp/x");
    assert!(inside(cwd, "a/b").is_some());
    for bad in ["", "../a", "/etc/passwd", "a/../../b", "./a"] {
        assert!(inside(cwd, bad).is_none(), "{bad}");
    }
}

/// The restore re-makes the obstruction check itself, just before its first
/// write: a base file that is now a directory refuses, untouched.
#[tokio::test]
async fn rewind_restore_refuses_before_writing_over_a_directory() {
    let dir = repo();
    let root = dir.path();
    let base = capture(root, RefLeaf::Base(1)).await;
    std::fs::remove_file(root.join("a.txt")).expect("rm");
    write(root, "a.txt/x.log", "ignored, in no capture\n");
    write(root, "gone.txt", "changed\n");
    let pre = capture(
        root,
        RefLeaf::PreRewind {
            through_seq: 9,
            command: COMMAND,
        },
    )
    .await;
    let outcome = restore_tree(RestoreRequest {
        cwd: root,
        scope: Some(&UNBOUNDED_FOR_TESTS),
        base_tree: &base.tree,
        pre_tree: &pre.tree,
        omitted: &[],
    })
    .await;
    let Err(RestoreError::Refused(sentence)) = outcome else {
        panic!("refused: {outcome:?}");
    };
    assert!(sentence.contains("\"a.txt\""), "{sentence}");
    assert!(
        root.join("a.txt/x.log").exists(),
        "the ignored file survives"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("gone.txt")).unwrap(),
        "changed\n",
        "nothing was written"
    );
}
