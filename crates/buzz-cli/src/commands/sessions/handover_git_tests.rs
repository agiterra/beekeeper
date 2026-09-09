//! Every git-touching test here builds its own throwaway repository under a
//! `TempDir` and never reads a path from the environment.
//!
//! A test that could reach the repository this crate lives in is a bug in the
//! test: `git init` in a temp directory inherits `GIT_DIR` from a pre-push
//! hook otherwise, and one run of these tests would then write into the
//! pushing repository's index. [`git`] clears the whole selection list and
//! pins the config files to `/dev/null`, exactly as `worktree_tests.rs` does.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

/// Run git in `cwd` with a hermetic environment, asserting success.
fn git(args: &[&str], cwd: &Path) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "lane c")
        .env("GIT_AUTHOR_EMAIL", "lane-c@example.invalid")
        .env("GIT_COMMITTER_NAME", "lane c")
        .env("GIT_COMMITTER_EMAIL", "lane-c@example.invalid")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A throwaway repository with one commit on `main`.
struct Repo {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("repo");
        std::fs::create_dir_all(&path).expect("mkdir");
        git(&["init", "--quiet", "--initial-branch=main"], &path);
        std::fs::write(path.join("tracked.txt"), "one\n").expect("write");
        std::fs::write(path.join("staged.txt"), "s0\n").expect("write");
        git(&["add", "-A"], &path);
        git(&["commit", "--quiet", "-m", "init"], &path);
        Self { _dir: dir, path }
    }

    fn head(&self) -> String {
        git(&["rev-parse", "HEAD"], &self.path).trim().to_owned()
    }

    fn status(&self) -> String {
        git(&["status", "--porcelain"], &self.path)
    }

    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.path.join(name), contents).expect("write");
    }

    fn write_bytes(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.path.join(name), bytes).expect("write");
    }
}

/// Bytes no text diff can carry, built programmatically rather than written as
/// a source literal.
fn binary_blob(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}

/// Bytes that do not compress, so a binary patch of them is about as large as
/// the file. A cyclic pattern deflates to almost nothing, which quietly made
/// the whole-patch bound untestable.
fn incompressible(len: usize) -> Vec<u8> {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

#[test]
fn revision_reads_head_branch_and_dirtiness() {
    let repo = Repo::new();
    let clean = read_revision(&repo.path).expect("clean revision");
    assert_eq!(clean.head_sha, repo.head());
    assert_eq!(clean.branch.as_deref(), Some("main"));
    assert!(!clean.dirty, "a fresh checkout is not dirty");
    // The merge base of `main` with itself is `main`, so a repository whose
    // only branch is the integration branch still names a base.
    assert_eq!(clean.base_sha.as_deref(), Some(repo.head().as_str()));

    repo.write("tracked.txt", "one\ntwo\n");
    let dirty = read_revision(&repo.path).expect("dirty revision");
    assert!(dirty.dirty, "an edited file makes the worktree dirty");
}

#[test]
fn capture_carries_staged_unstaged_untracked_and_binary_in_one_patch() {
    let repo = Repo::new();
    // Four kinds of uncommitted change at once, which is the case the capture
    // exists for: a temp-index diff sees all of them, `git diff HEAD` alone
    // would miss the untracked file.
    repo.write("tracked.txt", "one\nedited\n");
    repo.write("staged.txt", "s1\n");
    git(&["add", "staged.txt"], &repo.path);
    repo.write("untracked.txt", "brand new\n");
    repo.write_bytes("blob.bin", &binary_blob(4096));

    let head = repo.head();
    let captured = capture_working_tree(
        &repo.path,
        &head,
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");

    for path in ["tracked.txt", "staged.txt", "untracked.txt", "blob.bin"] {
        assert!(
            captured.patch.contains(path),
            "the single patch must carry {path}; it read:\n{}",
            captured.patch
        );
        assert!(
            captured.changed_paths.iter().any(|held| held == path),
            "{path} must be enumerated as changed"
        );
    }
    assert!(
        captured.patch.contains("GIT binary patch"),
        "binary content must travel as a binary patch, not be dropped"
    );
    assert!(
        captured.omitted.is_empty(),
        "nothing was over a bound, so nothing may be omitted: {:?}",
        captured.omitted
    );
    assert_eq!(
        super::super::handover_checkpoint::decide_preserved(true, &captured, true),
        CodingSessionHandoverPreserved::All,
        "a dirty tree whose whole capture travelled is preserved: all"
    );
}

#[test]
fn capture_leaves_the_callers_own_index_untouched() {
    let repo = Repo::new();
    repo.write("staged.txt", "s1\n");
    git(&["add", "staged.txt"], &repo.path);
    repo.write("untracked.txt", "new\n");
    let before = repo.status();

    capture_working_tree(
        &repo.path,
        &repo.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");

    assert_eq!(
        repo.status(),
        before,
        "the capture uses a temporary index, so the caller's staging area is unchanged"
    );
}

#[test]
fn capture_omits_an_oversize_file_by_path_and_names_the_bound() {
    let repo = Repo::new();
    repo.write("small.txt", "still here\n");
    repo.write_bytes("huge.bin", &binary_blob(300 * 1024));

    let captured = capture_working_tree(
        &repo.path,
        &repo.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");

    assert!(
        captured.patch.contains("small.txt"),
        "one oversize file must not evict the rest"
    );
    assert!(
        !captured.patch.contains("huge.bin"),
        "the oversize file's bytes must not be in the patch"
    );
    let omitted: Vec<&str> = captured
        .omitted
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(omitted, vec!["huge.bin"], "it is omitted by path");
    let line = captured.omitted[0].line();
    assert!(
        line.contains("huge.bin") && line.contains("per-file capture bound"),
        "the missing line names the path and the bound: {line}"
    );
    assert!(
        captured.changed_paths.iter().any(|path| path == "huge.bin"),
        "an omitted path is still enumerated as changed — omitted is not forgotten"
    );
    assert_eq!(
        super::super::handover_checkpoint::decide_preserved(true, &captured, true),
        CodingSessionHandoverPreserved::Partial,
        "some bytes travelled and some did not: partial"
    );
}

#[test]
fn capture_drops_the_largest_paths_until_the_patch_bound_is_met() {
    let repo = Repo::new();
    repo.write_bytes("a.bin", &incompressible(60 * 1024));
    repo.write("b.txt", "small\n");

    // A per-file bound above both, so only the whole-patch rule can fire.
    let captured =
        capture_working_tree(&repo.path, &repo.head(), 200 * 1024, 4 * 1024).expect("capture");

    assert_eq!(
        captured.omitted.len(),
        1,
        "the largest path alone brings the patch under the bound: {:?}",
        captured.omitted
    );
    assert_eq!(captured.omitted[0].path, "a.bin");
    assert!(
        captured.omitted[0].line().contains("patch bound"),
        "the reason names the whole-patch bound, not the per-file one: {}",
        captured.omitted[0].line()
    );
    assert!(
        captured.patch.len() <= 4 * 1024,
        "the surviving patch is under the bound it was cut to: {} bytes",
        captured.patch.len()
    );
    assert!(
        captured.patch.contains("b.txt"),
        "the small path survives the eviction"
    );
}

#[test]
fn a_clean_tree_captures_nothing_and_is_preserved_all() {
    let repo = Repo::new();
    let captured = capture_working_tree(
        &repo.path,
        &repo.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");
    assert!(!captured.has_patch());
    assert!(captured.changed_paths.is_empty());
    assert_eq!(
        super::super::handover_checkpoint::decide_preserved(false, &captured, false),
        CodingSessionHandoverPreserved::All
    );
}

#[test]
fn capture_records_a_deletion_rather_than_dropping_it() {
    let repo = Repo::new();
    std::fs::remove_file(repo.path.join("tracked.txt")).expect("remove");
    let captured = capture_working_tree(
        &repo.path,
        &repo.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");
    assert!(
        captured.patch.contains("deleted file"),
        "a file removed from the worktree is a deletion in the patch: {}",
        captured.patch
    );
}

#[test]
fn apply_round_trips_a_capture_into_a_second_checkout() {
    let source = Repo::new();
    source.write("tracked.txt", "one\nedited\n");
    source.write("untracked.txt", "new\n");
    let captured = capture_working_tree(
        &source.path,
        &source.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");

    let target = Repo::new();
    apply_patch(&target.path, &captured.patch).expect("apply");
    assert_eq!(
        std::fs::read_to_string(target.path.join("tracked.txt")).expect("read"),
        "one\nedited\n"
    );
    assert!(target.path.join("untracked.txt").is_file());
}

#[test]
fn apply_that_would_fail_the_check_changes_nothing() {
    let source = Repo::new();
    source.write("tracked.txt", "one\nedited\n");
    let captured = capture_working_tree(
        &source.path,
        &source.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");

    // A target whose file has different content and whose object store does
    // not hold the patch's pre-image, so neither a straight apply nor a 3-way
    // merge can place it.
    let target = Repo::new();
    target.write("tracked.txt", "something else entirely\n");
    git(&["add", "-A"], &target.path);
    git(&["commit", "--quiet", "-m", "diverge"], &target.path);
    let before_status = target.status();
    let before_head = target.head();
    let before_file = std::fs::read_to_string(target.path.join("tracked.txt")).expect("read");

    let error = apply_patch(&target.path, &captured.patch).expect_err("must refuse");
    assert!(
        error.contains("does not apply") && error.contains("nothing was applied"),
        "the refusal must say the tree was left alone: {error}"
    );
    assert_eq!(target.status(), before_status, "the index is unchanged");
    assert_eq!(target.head(), before_head, "HEAD is unchanged");
    assert_eq!(
        std::fs::read_to_string(target.path.join("tracked.txt")).expect("read"),
        before_file,
        "the working tree is byte-identical: never half-applied"
    );
}

#[test]
fn apply_of_an_empty_patch_is_a_no_op() {
    let repo = Repo::new();
    let before = repo.status();
    apply_patch(&repo.path, "   \n").expect("empty patch applies trivially");
    assert_eq!(repo.status(), before);
}

#[test]
fn push_refuses_any_ref_outside_the_wip_namespace() {
    let repo = Repo::new();
    let error = push_wip_ref(&repo.path, "origin", "refs/heads/main").expect_err("must refuse");
    assert!(
        error.contains("only ever writes the wip namespace"),
        "the refusal names the rule: {error}"
    );
}

#[test]
fn push_remote_resolves_from_git_config_and_never_from_a_hard_coded_name() {
    let repo = Repo::new();
    assert_eq!(
        resolve_push_remote(&repo.path, Some("main")),
        None,
        "a repository with no remote resolves none rather than guessing 'origin'"
    );

    git(
        &[
            "remote",
            "add",
            "somewhere",
            "https://example.invalid/x.git",
        ],
        &repo.path,
    );
    assert_eq!(
        resolve_push_remote(&repo.path, Some("main")).as_deref(),
        Some("somewhere"),
        "a sole remote is the answer, whatever it is called"
    );

    git(
        &[
            "remote",
            "add",
            "elsewhere",
            "https://example.invalid/y.git",
        ],
        &repo.path,
    );
    assert_eq!(
        resolve_push_remote(&repo.path, Some("main")),
        None,
        "two remotes and no push configuration is ambiguous, not 'origin'"
    );

    git(&["config", "buzz.wipRemote", "elsewhere"], &repo.path);
    assert_eq!(
        resolve_push_remote(&repo.path, Some("main")).as_deref(),
        Some("elsewhere"),
        "an explicit buzz.wipRemote wins"
    );
}

#[test]
fn fetch_and_checkout_lands_the_named_sha_on_the_named_branch() {
    let origin = Repo::new();
    origin.write("tracked.txt", "one\nshared\n");
    git(&["add", "-A"], &origin.path);
    git(&["commit", "--quiet", "-m", "wip"], &origin.path);
    let sha = origin.head();
    git(
        &["update-ref", "refs/heads/wip/owner/abcd1234", &sha],
        &origin.path,
    );

    let target = Repo::new();
    git(
        &["remote", "add", "src", origin.path.to_str().expect("utf8")],
        &target.path,
    );
    fetch_and_checkout(
        &target.path,
        "src",
        "refs/heads/wip/owner/abcd1234",
        &sha,
        "handover/abcd1234",
    )
    .expect("fetch and checkout");

    assert_eq!(target.head(), sha);
    assert_eq!(
        git(&["rev-parse", "--abbrev-ref", "HEAD"], &target.path).trim(),
        "handover/abcd1234"
    );

    // Idempotent: a rerun lands on the same branch rather than failing on
    // "already exists", which is what the interrupted-rerun case needs.
    fetch_and_checkout(
        &target.path,
        "src",
        "refs/heads/wip/owner/abcd1234",
        &sha,
        "handover/abcd1234",
    )
    .expect("rerun");
    assert_eq!(target.head(), sha);
}

#[test]
fn fetch_and_checkout_refuses_a_sha_the_fetched_ref_does_not_carry() {
    let origin = Repo::new();
    let real = origin.head();
    git(
        &["update-ref", "refs/heads/wip/owner/abcd1234", &real],
        &origin.path,
    );

    let target = Repo::new();
    git(
        &["remote", "add", "src", origin.path.to_str().expect("utf8")],
        &target.path,
    );
    let absent = "0".repeat(40);
    let error = fetch_and_checkout(
        &target.path,
        "src",
        "refs/heads/wip/owner/abcd1234",
        &absent,
        "handover/zzzz",
    )
    .expect_err("must refuse");
    assert!(
        error.contains("not present after fetching"),
        "the refusal names what was missing: {error}"
    );
}

#[test]
fn ignored_files_are_named_and_stop_the_checkpoint_claiming_preserved_all() {
    // The case that made this a finding: `git add -A` honours `.gitignore` and
    // `git status --porcelain` does not list ignored files at all, so a
    // worktree whose real uncommitted work includes a modified `.env` would
    // have signed `preserved: "all"` over bytes nothing carries (REVIEW S3).
    let repo = Repo::new();
    repo.write(".gitignore", ".env\nbuild/\n");
    repo.write(".env", "SECRET=old\n");
    git(&["add", "-A"], &repo.path);
    git(&["commit", "--quiet", "-m", "ignore .env"], &repo.path);

    repo.write("tracked.txt", "one\nedited\n");
    repo.write(".env", "SECRET=new\n");
    std::fs::create_dir_all(repo.path.join("build")).expect("mkdir");
    std::fs::write(repo.path.join("build/out.o"), "obj").expect("write");

    let captured = capture_working_tree(
        &repo.path,
        &repo.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");

    assert!(
        captured.patch.contains("tracked.txt"),
        "the tracked edit still travels"
    );
    assert!(
        !captured.patch.contains("SECRET=new"),
        "an ignored file's bytes are not in the patch — git will not stage them"
    );
    assert!(captured.ignored.any(), "the ignored paths must be reported");
    assert!(
        captured.ignored.listed.iter().any(|path| path == ".env"),
        "named by path so a person can act on it: {:?}",
        captured.ignored.listed
    );
    assert!(
        captured
            .ignored
            .listed
            .iter()
            .any(|path| path.starts_with("build")),
        "an ignored directory is one entry, not one per file: {:?}",
        captured.ignored.listed
    );
    let line = captured.ignored.line();
    assert!(
        line.contains("ignored by .gitignore, not captured"),
        "the missing line says what it is: {line}"
    );

    assert_eq!(
        super::super::handover_checkpoint::decide_preserved(true, &captured, true),
        CodingSessionHandoverPreserved::Partial,
        "with ignored files present, a checkpoint never claims every byte travelled"
    );
    // And the cap applies to a clean worktree too: `all` is the one value a
    // reader acts on without checking `missing`.
    let clean = Repo::new();
    clean.write(".gitignore", ".env\n");
    clean.write(".env", "SECRET=x\n");
    git(&["add", ".gitignore"], &clean.path);
    git(&["commit", "--quiet", "-m", "ignore"], &clean.path);
    let clean_capture = capture_working_tree(
        &clean.path,
        &clean.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");
    assert!(clean_capture.ignored.any());
    assert_eq!(
        super::super::handover_checkpoint::decide_preserved(false, &clean_capture, false),
        CodingSessionHandoverPreserved::Partial
    );
}

#[test]
fn a_worktree_with_nothing_ignored_still_reports_preserved_all() {
    let repo = Repo::new();
    repo.write("tracked.txt", "one\nedited\n");
    let captured = capture_working_tree(
        &repo.path,
        &repo.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");
    assert!(!captured.ignored.any());
    assert_eq!(
        super::super::handover_checkpoint::decide_preserved(true, &captured, true),
        CodingSessionHandoverPreserved::All,
        "the cap must not fire when there is nothing to disclose"
    );
}

#[test]
fn a_conflicting_three_way_apply_restores_the_index_as_well_as_the_worktree() {
    // The `--check --3way` pair passes and the apply then lands conflict
    // markers, so the restore path runs. A first cut restored only the
    // worktree, with `read-tree --reset -u` against the real index — which
    // staged everything the caller had left unstaged (REVIEW S6). This asserts
    // `git status --porcelain` byte-for-byte, the one output that
    // distinguishes staged from unstaged.
    let source = Repo::new();
    source.write("tracked.txt", "one\nfrom the checkpoint\n");
    let captured = capture_working_tree(
        &source.path,
        &source.head(),
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )
    .expect("capture");

    // The target holds `tracked.txt` at diverged content, **committed and
    // clean**, so `git apply --check --3way` passes and the real apply then
    // conflicts. The staged/unstaged distinction sits on files the patch does
    // not touch, so what is measured is the restore and not the apply.
    let target = Repo::new();
    target.write("other.txt", "base\n");
    target.write("tracked.txt", "one\nsomething else entirely\n");
    git(&["add", "-A"], &target.path);
    git(&["commit", "--quiet", "-m", "diverge"], &target.path);

    target.write("staged.txt", "staged edit\n");
    git(&["add", "staged.txt"], &target.path);
    target.write("other.txt", "local work in progress\n");
    target.write("untracked.txt", "mine\n");

    let before_status = target.status();
    assert!(
        before_status.contains("M  staged.txt"),
        "the fixture must actually hold a staged change: {before_status:?}"
    );
    assert!(
        before_status.contains(" M other.txt"),
        "and an unstaged one: {before_status:?}"
    );
    assert!(
        before_status.contains("?? untracked.txt"),
        "and an untracked one: {before_status:?}"
    );
    let names = ["tracked.txt", "staged.txt", "other.txt", "untracked.txt"];
    let before_files: Vec<String> = names
        .iter()
        .map(|name| std::fs::read_to_string(target.path.join(name)).expect("read"))
        .collect();
    let before_head = target.head();

    let error = apply_patch(&target.path, &captured.patch).expect_err("must refuse");
    assert!(
        error.contains("restored to how it was"),
        "the error claims a restore, so the restore must be real: {error}"
    );

    assert_eq!(
        target.status(),
        before_status,
        "staged stays staged and unstaged stays unstaged: the index was restored, not rebuilt"
    );
    assert_eq!(target.head(), before_head, "HEAD is unchanged");
    for (name, before) in names.iter().zip(before_files) {
        assert_eq!(
            std::fs::read_to_string(target.path.join(name)).expect("read"),
            before,
            "{name} is byte-identical after the failed apply"
        );
    }
    assert!(
        !std::fs::read_to_string(target.path.join("tracked.txt"))
            .expect("read")
            .contains("<<<<<<<"),
        "no conflict markers survive the restore"
    );
}

#[test]
fn an_existing_handover_branch_is_reused_when_its_tip_is_contained() {
    // The ordinary rerun: nothing happened on the branch since, so placing it
    // at the same commit discards nothing.
    let origin = Repo::new();
    origin.write("tracked.txt", "one\nshared\n");
    git(&["add", "-A"], &origin.path);
    git(&["commit", "--quiet", "-m", "wip"], &origin.path);
    let sha = origin.head();
    git(
        &["update-ref", "refs/heads/wip/owner/1f2e3d4c", &sha],
        &origin.path,
    );

    let target = Repo::new();
    git(
        &["remote", "add", "src", origin.path.to_str().expect("utf8")],
        &target.path,
    );
    for _ in 0..2 {
        fetch_and_checkout(
            &target.path,
            "src",
            "refs/heads/wip/owner/1f2e3d4c",
            &sha,
            "handover/1f2e3d4c",
        )
        .expect("a rerun onto the same commit is safe");
    }
    assert_eq!(target.head(), sha);
    assert_eq!(
        git(&["rev-parse", "--abbrev-ref", "HEAD"], &target.path).trim(),
        "handover/1f2e3d4c",
        "one session, one branch: the name is the session's, not the head sha's"
    );
}

#[test]
fn a_handover_branch_that_moved_on_is_refused_rather_than_discarded() {
    // A previous reconstruction committed on the branch. `checkout -B` would
    // silently throw those commits away (composition run 5, finding 3).
    let origin = Repo::new();
    origin.write("tracked.txt", "one\nshared\n");
    git(&["add", "-A"], &origin.path);
    git(&["commit", "--quiet", "-m", "wip"], &origin.path);
    let sha = origin.head();
    git(
        &["update-ref", "refs/heads/wip/owner/1f2e3d4c", &sha],
        &origin.path,
    );

    let target = Repo::new();
    git(
        &["remote", "add", "src", origin.path.to_str().expect("utf8")],
        &target.path,
    );
    fetch_and_checkout(
        &target.path,
        "src",
        "refs/heads/wip/owner/1f2e3d4c",
        &sha,
        "handover/1f2e3d4c",
    )
    .expect("first reconstruction");

    // The work a second run must not discard.
    target.write("continued.txt", "the claimant's own work\n");
    git(&["add", "-A"], &target.path);
    git(&["commit", "--quiet", "-m", "continued"], &target.path);
    let moved_tip = target.head();
    git(&["checkout", "--quiet", "main"], &target.path);

    let error = fetch_and_checkout(
        &target.path,
        "src",
        "refs/heads/wip/owner/1f2e3d4c",
        &sha,
        "handover/1f2e3d4c",
    )
    .expect_err("must refuse to move a branch that has moved on");
    assert!(
        error.contains("handover/1f2e3d4c") && error.contains(&moved_tip),
        "the refusal names the branch and its tip so the work is recoverable: {error}"
    );
    assert!(
        error.contains("would discard those commits"),
        "and says what it is protecting: {error}"
    );
    assert_eq!(
        git(&["rev-parse", "handover/1f2e3d4c"], &target.path).trim(),
        moved_tip,
        "and the branch is exactly where it was"
    );
}

#[test]
fn a_hostile_artifact_ref_is_refused_before_git_is_ever_invoked() {
    // Kind 44247 validates an artifact's `ref` for text bounds only — the
    // relay adjudicates structure, not meaning — so a checkpoint published by
    // somebody else's machine can carry any string here. `git fetch <remote>
    // <src>:<dst>` **writes `<dst>` locally**, during the fetch, before any
    // sha check could refuse it.
    let repo = Repo::new();
    let before_main = git(&["rev-parse", "main"], &repo.path).trim().to_owned();
    // A remote that resolves, so nothing is refused for the wrong reason.
    git(
        &["remote", "add", "src", repo.path.to_str().expect("utf8")],
        &repo.path,
    );

    let hostile = [
        // The one that matters: a forced write onto the caller's own main.
        "+refs/heads/wip/x:refs/heads/main",
        // The same write without the force.
        "refs/heads/wip/x:refs/heads/main",
        // Revision syntax that resolves to somebody else's commit.
        "refs/heads/wip/x^{commit}",
        "refs/heads/wip/x~3",
        // Glob syntax, which would fetch more than the record names.
        "refs/heads/wip/*",
        // Not a branch ref at all.
        "--upload-pack=touch /tmp/pwned",
    ];
    for spelling in hostile {
        let error = fetch_and_checkout(
            &repo.path,
            "src",
            spelling,
            &repo.head(),
            "handover/1f2e3d4c",
        )
        .expect_err("must refuse {spelling}");
        assert!(
            error.contains("refusing to fetch") && error.contains("plain"),
            "{spelling:?} must be refused by name: {error}"
        );
        assert_eq!(
            git(&["rev-parse", "main"], &repo.path).trim(),
            before_main,
            "local main must be untouched after refusing {spelling:?}"
        );
    }

    // And the shape that is allowed still is.
    assert!(safe_wip_ref("refs/heads/wip/owner/1f2e3d4c").is_ok());
    assert!(safe_wip_ref("refs/heads/wip/a.b_c-d/9").is_ok());
    for bad in [
        "refs/heads/",
        "refs/tags/v1",
        "refs/heads/a//b",
        "refs/heads/a/",
        "refs/heads/../x",
    ] {
        assert!(safe_wip_ref(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn a_hostile_remote_name_is_refused_too() {
    // Resolved from git config rather than from a checkpoint, but it lands in
    // the same argv position, and a value beginning `-` reads as a flag.
    for bad in ["--upload-pack=touch /tmp/pwned", "-o", "orig in", "a:b", ""] {
        assert!(
            safe_remote_name(bad).is_err(),
            "{bad:?} must be refused as a remote name"
        );
    }
    assert!(safe_remote_name("origin").is_ok());
    assert!(safe_remote_name("hive.agiterra.org").is_ok());
}

#[test]
fn the_fetch_refspec_names_no_destination() {
    // The positive half of the guard: even a well-formed ref is fetched with
    // an empty destination, so the objects land and no local ref moves.
    let origin = Repo::new();
    origin.write("tracked.txt", "one\nshared\n");
    git(&["add", "-A"], &origin.path);
    git(&["commit", "--quiet", "-m", "wip"], &origin.path);
    let sha = origin.head();
    git(
        &["update-ref", "refs/heads/wip/owner/1f2e3d4c", &sha],
        &origin.path,
    );

    let target = Repo::new();
    let before_main = git(&["rev-parse", "main"], &target.path).trim().to_owned();
    git(
        &["remote", "add", "src", origin.path.to_str().expect("utf8")],
        &target.path,
    );
    fetch_and_checkout(
        &target.path,
        "src",
        "refs/heads/wip/owner/1f2e3d4c",
        &sha,
        "handover/1f2e3d4c",
    )
    .expect("fetch and checkout");

    assert_eq!(
        git(&["rev-parse", "main"], &target.path).trim(),
        before_main,
        "the fetch moved no local branch of its own"
    );
    // `rev-parse --verify` exits non-zero on a missing ref, so this asks
    // without the asserting helper.
    let local_copy = Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/heads/wip/owner/1f2e3d4c",
        ])
        .current_dir(&target.path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        !local_copy.status.success(),
        "and wrote no local copy of the source ref either"
    );
    assert_eq!(target.head(), sha, "only the handover branch was placed");
}
