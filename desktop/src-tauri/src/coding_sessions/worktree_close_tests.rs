//! The close-time disposition, against real repositories in a `TempDir`.
//!
//! Every repository, worktree and remote here is created inside a temporary
//! directory by the test that uses it. Nothing reads an environment variable
//! for a path, nothing runs `git` in a directory it did not create, and the
//! "relay" is a bare clone on the local filesystem — so `ls-remote` is a real
//! transport round trip with no network and no credential helper (memory rule:
//! git-tooling tests use throwaway clones, never the real repo).

use std::path::{Path, PathBuf};
use std::process::Command;

use buzz_core_pkg::worktree_lifecycle::{SeatWorktreeDisposition, SEAT_WORKTREE_GRACE_SECS};

use super::{close_disposition, tip_on_remote, CloseDecision};
use crate::commands::project_git_exec::{build_test_git_auth_config, GitAuthConfig};

/// The git config these tests run under.
///
/// `build_test_git_auth_config` is the one that allows the `file` transport,
/// which is what makes a bare clone in a `TempDir` usable as a remote —
/// production's config forbids it, deliberately, and a real relay is https.
fn auth() -> GitAuthConfig {
    build_test_git_auth_config().expect("git auth config")
}

/// `git` with the ambient environment's repository selection cleared.
///
/// `GIT_DIR` and friends are exported into every hook git runs, and the
/// pre-push gate runs these tests inside one. Without this, a `git init` in a
/// temp directory targets the *pushing* repository — which is how a previous
/// lane wrote a commit and a branch into the real checkout.
fn git(args: &[&str], cwd: &Path) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "P3")
        .env("GIT_AUTHOR_EMAIL", "p3@example.invalid")
        .env("GIT_COMMITTER_NAME", "P3")
        .env("GIT_COMMITTER_EMAIL", "p3@example.invalid")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository with a bare "relay" clone as its `origin`.
struct Layout {
    _dir: tempfile::TempDir,
    repo: PathBuf,
}

fn layout() -> Layout {
    let dir = tempfile::tempdir().expect("tempdir");
    let origin = dir.path().join("relay.git");
    git(
        &[
            "init",
            "--bare",
            "--initial-branch=main",
            origin.to_str().expect("utf8"),
        ],
        dir.path(),
    );
    let repo = dir.path().join("proj");
    std::fs::create_dir_all(&repo).expect("mkdir");
    git(&["init", "--initial-branch=main", "."], &repo);
    std::fs::write(repo.join("README"), b"hello\n").expect("write");
    git(&["add", "README"], &repo);
    git(&["commit", "-m", "one"], &repo);
    git(
        &["remote", "add", "origin", origin.to_str().expect("utf8")],
        &repo,
    );
    git(&["push", "origin", "main"], &repo);
    Layout { _dir: dir, repo }
}

/// A worktree of `layout` on its own branch, in the legacy sibling holder.
fn add_worktree(layout: &Layout, slug: &str) -> PathBuf {
    let path = layout
        .repo
        .parent()
        .expect("parent")
        .join("proj.worktrees")
        .join(slug);
    git(
        &[
            "worktree",
            "add",
            "-b",
            slug,
            path.to_str().expect("utf8"),
            "main",
        ],
        &layout.repo,
    );
    path
}

/// Long enough ago that the grace window has run out.
const PAST_GRACE: Option<u64> = Some(SEAT_WORKTREE_GRACE_SECS + 1);

// ── the tip question, asked of the transport ────────────────────────────────

#[test]
fn a_branch_the_remote_holds_reads_as_on_the_relay() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-pushed");
    std::fs::write(tree.join("work.txt"), b"work\n").expect("write");
    git(&["add", "work.txt"], &tree);
    git(&["commit", "-m", "seat work"], &tree);
    git(&["push", "origin", "lane-pushed"], &tree);

    assert_eq!(tip_on_remote(&tree, &auth()), Some(true));
}

/// The other admissible shape: the seat's own branch was never pushed, but its
/// commits are an ancestor of something the remote holds — a landed lane.
#[test]
fn work_merged_into_a_branch_the_remote_holds_reads_as_on_the_relay() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-merged");
    std::fs::write(tree.join("work.txt"), b"work\n").expect("write");
    git(&["add", "work.txt"], &tree);
    git(&["commit", "-m", "seat work"], &tree);
    git(&["merge", "--ff-only", "lane-merged"], &layout.repo);
    git(&["push", "origin", "main"], &layout.repo);

    assert_eq!(tip_on_remote(&tree, &auth()), Some(true));
}

#[test]
fn a_branch_the_remote_has_never_seen_reads_as_not_on_the_relay() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-unpushed");
    std::fs::write(tree.join("work.txt"), b"work\n").expect("write");
    git(&["add", "work.txt"], &tree);
    git(&["commit", "-m", "seat work"], &tree);

    assert_eq!(tip_on_remote(&tree, &auth()), Some(false));
}

// ── the close-time disposition ──────────────────────────────────────────────

#[test]
fn a_clean_pushed_tree_past_its_grace_window_is_prunable() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-done");
    std::fs::write(tree.join("work.txt"), b"work\n").expect("write");
    git(&["add", "work.txt"], &tree);
    git(&["commit", "-m", "seat work"], &tree);
    git(&["push", "origin", "lane-done"], &tree);

    let CloseDecision {
        disposition,
        tip,
        detail,
        ..
    } = close_disposition(&layout.repo, &tree, false, PAST_GRACE, false, &auth());
    assert_eq!(disposition, SeatWorktreeDisposition::Prunable);
    assert_eq!(tip, Some(true));
    assert!(detail.ends_with("clean, will be removed"), "{detail}");
}

/// Uncommitted work is never removed by anything that runs on its own, and the
/// refusal names the count rather than a token.
#[test]
fn a_dirty_tree_is_refused_with_the_count_of_what_it_holds() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-dirty");
    std::fs::write(tree.join("work.txt"), b"work\n").expect("write");
    git(&["add", "work.txt"], &tree);
    git(&["commit", "-m", "seat work"], &tree);
    git(&["push", "origin", "lane-dirty"], &tree);
    std::fs::write(tree.join("unsaved.txt"), b"not committed\n").expect("write");

    let CloseDecision {
        disposition,
        tip,
        detail,
        ..
    } = close_disposition(&layout.repo, &tree, false, PAST_GRACE, false, &auth());
    assert_eq!(
        disposition,
        SeatWorktreeDisposition::Held { dirty_files: 1 }
    );
    // Not even asked: a dirty tree's disposition is decided without a round
    // trip, so the close path never pays for one.
    assert_eq!(tip, None);
    assert_eq!(detail, "held: 1 uncommitted files");
}

#[test]
fn an_unpushed_tree_is_refused_with_the_relay_sentence() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-unpushed");
    std::fs::write(tree.join("work.txt"), b"work\n").expect("write");
    git(&["add", "work.txt"], &tree);
    git(&["commit", "-m", "seat work"], &tree);

    let CloseDecision {
        disposition,
        tip,
        detail,
        ..
    } = close_disposition(&layout.repo, &tree, false, PAST_GRACE, false, &auth());
    assert_eq!(disposition, SeatWorktreeDisposition::TipNotOnRelay);
    assert_eq!(tip, Some(false));
    assert!(
        detail.ends_with(
            "the relay's current ref state does not hold this branch, so nothing is removed"
        ),
        "{detail}"
    );
}

/// The grace window is policy, and a close is not an override of it. A tree
/// closed a moment ago is kept and says for how long — which is why the build
/// output is reclaimed separately and needs no grace at all.
#[test]
fn a_tree_closed_just_now_is_kept_for_its_grace_window() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-fresh");
    std::fs::write(tree.join("work.txt"), b"work\n").expect("write");
    git(&["add", "work.txt"], &tree);
    git(&["commit", "-m", "seat work"], &tree);
    git(&["push", "origin", "lane-fresh"], &tree);

    let CloseDecision {
        disposition,
        detail,
        ..
    } = close_disposition(&layout.repo, &tree, false, Some(0), false, &auth());
    assert!(
        matches!(disposition, SeatWorktreeDisposition::WithinGrace { .. }),
        "{disposition:?}"
    );
    assert!(
        detail.ends_with("clean and pushed, kept 7 more days"),
        "{detail}"
    );
}

#[test]
fn a_live_execution_outranks_every_other_reason() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-live");
    git(&["push", "origin", "lane-live"], &tree);

    let CloseDecision {
        disposition,
        detail,
        ..
    } = close_disposition(&layout.repo, &tree, true, PAST_GRACE, false, &auth());
    assert_eq!(disposition, SeatWorktreeDisposition::ExecutionLive);
    assert!(
        detail.ends_with("an execution is still running here"),
        "{detail}"
    );
}

/// The repository's own checkout is never a candidate, however clean and
/// pushed it is — and it is never even asked about, so the close path cannot
/// spend a round trip deciding not to delete somebody's working copy.
#[test]
fn the_repositorys_own_checkout_is_protected() {
    let layout = layout();
    let CloseDecision {
        disposition,
        tip,
        detail,
        ..
    } = close_disposition(
        &layout.repo,
        &layout.repo,
        false,
        PAST_GRACE,
        false,
        &auth(),
    );
    assert_eq!(disposition, SeatWorktreeDisposition::Protected);
    assert_eq!(tip, None);
    assert!(detail.ends_with("protected, never removed"), "{detail}");
}
