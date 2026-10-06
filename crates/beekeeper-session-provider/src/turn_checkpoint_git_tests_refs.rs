//! Ref leaves that cannot collide, a branch read that never guesses
//! "detached", and omissions named once. Every repository here is a
//! throwaway created by the test in its own temporary directory.

use super::*;
use crate::execution_scope_host::UNBOUNDED_FOR_TESTS;

const SESSION: &str = "sess-refs";
const COMMAND: &str = "4f0c9e3b2a1d8e7f6a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f";

/// `git` in `cwd`, hermetic, asserting success.
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

/// A repository on `main` with one committed file.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-q", "-b", "main", "."]);
    std::fs::write(dir.path().join("a.txt"), "one\n").expect("write");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "--no-gpg-sign", "-m", "init"]);
    dir
}

async fn capture(cwd: &Path, leaf: RefLeaf<'_>) -> Result<CapturedTree, CaptureFailure> {
    capture_tree(
        cwd,
        Some(&UNBOUNDED_FOR_TESTS),
        SESSION,
        2,
        leaf,
        None,
        CapturePhase::TurnEnd,
    )
    .await
}

#[test]
fn a_pre_rewind_leaf_is_spelled_apart_from_the_turn_at_the_same_seq() {
    assert_eq!(
        checkpoint_ref(
            "s",
            2,
            RefLeaf::PreRewind {
                through_seq: 58,
                command: &COMMAND.to_ascii_uppercase(),
            }
        )
        .as_deref(),
        Some(format!("refs/beekeeper/checkpoints/s/2/pre-rewind-58-{COMMAND}").as_str())
    );
    assert_ne!(
        checkpoint_ref("s", 2, RefLeaf::Through(58)),
        checkpoint_ref(
            "s",
            2,
            RefLeaf::PreRewind {
                through_seq: 58,
                command: COMMAND,
            }
        )
    );
    for bad in ["", "abc", "../heads/main", &"g".repeat(64), &"a".repeat(65)] {
        assert_eq!(
            checkpoint_ref(
                "s",
                2,
                RefLeaf::PreRewind {
                    through_seq: 1,
                    command: bad,
                }
            ),
            None,
            "{bad}"
        );
    }
}

#[tokio::test]
async fn a_pre_rewind_capture_keeps_the_last_turns_checkpoint() {
    let dir = repo();
    let turn = capture(dir.path(), RefLeaf::Through(58))
        .await
        .expect("turn capture");
    std::fs::write(dir.path().join("a.txt"), "edited after the turn\n").expect("edit");
    let leaf = RefLeaf::PreRewind {
        through_seq: 58,
        command: COMMAND,
    };
    let pre = capture(dir.path(), leaf).await.expect("pre-rewind capture");
    assert_ne!(turn.commit, pre.commit);
    let turn_ref = checkpoint_ref(SESSION, 2, RefLeaf::Through(58)).expect("turn ref");
    let pre_ref = checkpoint_ref(SESSION, 2, leaf).expect("pre-rewind ref");
    assert_eq!(git(dir.path(), &["rev-parse", &turn_ref]), turn.commit);
    assert_eq!(git(dir.path(), &["rev-parse", &pre_ref]), pre.commit);
}

#[tokio::test]
async fn an_invalid_rewind_command_refuses_before_running_git() {
    let dir = repo();
    let failure = capture(
        dir.path(),
        RefLeaf::PreRewind {
            through_seq: 3,
            command: "not-an-event-id",
        },
    )
    .await
    .expect_err("refused");
    assert_eq!(failure.code, UnavailableCode::GitFailed);
    assert!(git(dir.path(), &["for-each-ref", "refs/beekeeper"]).is_empty());
}

#[tokio::test]
async fn a_detached_head_is_none_and_only_then() {
    let dir = repo();
    git(dir.path(), &["checkout", "-q", "--detach"]);
    let captured = capture(dir.path(), RefLeaf::Through(1))
        .await
        .expect("capture");
    assert_eq!(captured.branch, None);
}

#[tokio::test]
async fn a_branch_name_up_to_the_wire_limit_is_carried_whole() {
    let dir = repo();
    // Components stay under a file name's 255 bytes; the whole is 401.
    let name = format!("{}/{}", "a".repeat(200), "b".repeat(200));
    git(dir.path(), &["checkout", "-q", "-b", &name]);
    let captured = capture(dir.path(), RefLeaf::Through(1))
        .await
        .expect("capture");
    assert_eq!(captured.branch.as_deref(), Some(name.as_str()));
}

#[tokio::test]
async fn a_branch_name_over_the_wire_limit_fails_rather_than_reading_detached() {
    let dir = repo();
    let name = format!("{0}/{0}/{0}", "c".repeat(200));
    assert!(name.len() > MAX_BRANCH_BYTES);
    git(dir.path(), &["checkout", "-q", "-b", &name]);
    let failure = capture(dir.path(), RefLeaf::Through(1))
        .await
        .expect_err("refused");
    assert_eq!(failure.code, UnavailableCode::GitFailed);
    assert!(
        failure.sentence.contains("512 bytes"),
        "{}",
        failure.sentence
    );
    assert!(git(dir.path(), &["for-each-ref", "refs/beekeeper"]).is_empty());
}

#[tokio::test]
async fn a_head_that_is_not_a_branch_fails_rather_than_reading_detached() {
    let dir = repo();
    git(dir.path(), &["tag", "v1"]);
    git(dir.path(), &["symbolic-ref", "HEAD", "refs/tags/v1"]);
    let failure = capture(dir.path(), RefLeaf::Through(1))
        .await
        .expect_err("refused");
    assert_eq!(failure.code, UnavailableCode::GitFailed);
    assert!(git(dir.path(), &["for-each-ref", "refs/beekeeper"]).is_empty());
}

#[test]
fn omissions_are_prefixed_sorted_and_named_once() {
    let exclusion = |raw: &[u8], reason| Exclusion {
        raw: raw.to_vec(),
        reason,
    };
    let reported = normalize_omitted(
        vec![
            exclusion(b"z.bin", OmitReason::TooLarge),
            exclusion(b"a.key", OmitReason::Unreadable),
            exclusion(b"z.bin", OmitReason::Unreadable),
        ],
        "pkg/",
    );
    assert_eq!(
        reported.named,
        vec![
            OmittedPath {
                path: "pkg/a.key".into(),
                reason: OmitReason::Unreadable,
            },
            OmittedPath {
                path: "pkg/z.bin".into(),
                reason: OmitReason::TooLarge,
            },
        ]
    );
    assert_eq!(reported.not_listed, 0);
}

/// A Latin-1 `é` (0xE9) alone is not UTF-8.
const LATIN1_NAME: &[u8] = b"caf\xe9.bin";

#[test]
fn a_name_that_is_not_utf8_is_excluded_by_its_bytes_and_counted_not_named() {
    let excluded = vec![
        Exclusion {
            raw: LATIN1_NAME.to_vec(),
            reason: OmitReason::TooLarge,
        },
        Exclusion {
            raw: LATIN1_NAME.to_vec(),
            reason: OmitReason::Unreadable,
        },
    ];
    let mut expected = b".\0:(exclude,literal)".to_vec();
    expected.extend_from_slice(LATIN1_NAME);
    expected.push(0);
    assert!(exclusion_pathspecs(&excluded[..1]) == expected);
    let reported = normalize_omitted(excluded, "pkg/");
    assert!(reported.named.is_empty(), "{:?}", reported.named);
    assert_eq!(reported.not_listed, 1, "named once, counted once");
}

/// Unix only: the file system must accept a name that is not UTF-8. APFS
/// refuses one (`EILSEQ`), so on macOS the test has nothing to create and
/// says so; ext4 and the other Linux file systems take any bytes.
#[cfg(unix)]
#[tokio::test]
async fn an_oversized_untracked_file_with_a_non_utf8_name_stays_out_of_the_tree() {
    use std::os::unix::ffi::OsStrExt;
    let dir = repo();
    let name = std::ffi::OsStr::from_bytes(LATIN1_NAME);
    let big = vec![b'x'; 17 * 1024 * 1024];
    if let Err(error) = std::fs::write(dir.path().join(name), &big) {
        eprintln!("skipped: this file system refuses a non-UTF-8 name ({error})");
        return;
    }
    std::fs::write(dir.path().join("small.txt"), "small\n").expect("small");
    let captured = capture(dir.path(), RefLeaf::Through(9))
        .await
        .expect("capture");
    assert!(!captured.complete);
    assert!(captured.omitted.is_empty(), "{:?}", captured.omitted);
    assert_eq!(captured.omitted_not_listed, 1);
    let listing = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["ls-tree", "-r", "-z", "--name-only", &captured.tree])
        .output()
        .expect("ls-tree");
    let paths: Vec<&[u8]> = listing.stdout.split(|byte| *byte == 0).collect();
    assert!(
        !paths.contains(&LATIN1_NAME),
        "the oversized blob is in the tree"
    );
    assert!(paths.contains(&&b"small.txt"[..]));
}

#[tokio::test]
async fn a_second_capture_to_the_same_leaf_leaves_the_first_commit_pinned() {
    let dir = repo();
    let first = capture(dir.path(), RefLeaf::Through(7))
        .await
        .expect("first capture");
    // The person keeps editing; the provider restarts and re-runs the end
    // capture for the same leaf.
    std::fs::write(dir.path().join("a.txt"), "kept editing\n").expect("edit");
    let failure = capture(dir.path(), RefLeaf::Through(7))
        .await
        .expect_err("an already-pinned leaf is not re-captured");
    assert_eq!(failure.code, UnavailableCode::GitFailed);
    assert_eq!(
        failure.already_pinned,
        Some(PinnedCheckpoint {
            commit: first.commit.clone(),
            tree: first.tree.clone(),
        })
    );
    let leaf_ref = checkpoint_ref(SESSION, 2, RefLeaf::Through(7)).expect("ref");
    assert_eq!(git(dir.path(), &["rev-parse", &leaf_ref]), first.commit);
    // The baseline leaf has the same rule.
    let base = capture(dir.path(), RefLeaf::Base(3)).await.expect("base");
    let again = capture(dir.path(), RefLeaf::Base(3))
        .await
        .expect_err("base kept");
    assert_eq!(
        again.already_pinned.map(|pinned| pinned.commit),
        Some(base.commit)
    );
}

#[tokio::test]
async fn the_ref_write_itself_refuses_to_move_an_existing_ref() {
    // The race the up-front check cannot close: the ref appears between the
    // check and the write.
    let dir = repo();
    let first = capture(dir.path(), RefLeaf::Through(11))
        .await
        .expect("first capture");
    let leaf_ref = checkpoint_ref(SESSION, 2, RefLeaf::Through(11)).expect("ref");
    std::fs::write(dir.path().join("a.txt"), "later\n").expect("edit");
    git(dir.path(), &["add", "a.txt"]);
    git(
        dir.path(),
        &["commit", "-q", "--no-gpg-sign", "-m", "later"],
    );
    let other = git(dir.path(), &["rev-parse", "HEAD"]);
    let failure = pin::pin(dir.path(), &leaf_ref, &other)
        .await
        .expect_err("create-only");
    assert_eq!(
        failure.already_pinned.map(|pinned| pinned.commit),
        Some(first.commit.clone())
    );
    assert_eq!(git(dir.path(), &["rev-parse", &leaf_ref]), first.commit);
    // Re-pinning the same commit is not a conflict.
    pin::pin(dir.path(), &leaf_ref, &first.commit)
        .await
        .expect("same commit");
    // And a fresh leaf is created.
    let fresh = checkpoint_ref(SESSION, 2, RefLeaf::Through(12)).expect("ref");
    pin::pin(dir.path(), &fresh, &other).await.expect("created");
    assert_eq!(git(dir.path(), &["rev-parse", &fresh]), other);
}
