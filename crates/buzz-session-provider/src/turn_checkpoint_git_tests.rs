//! Every repository here is a throwaway created by the test in its own
//! temporary directory and deleted with it — never this checkout, whose
//! common dir is shared with the person's live one.

use super::*;
use crate::execution_scope_host::UNBOUNDED_FOR_TESTS;

const SESSION: &str = "sess-1";

/// `git` in `cwd`, hermetic: no inherited repository selection, no global or
/// system configuration, a fixed identity.
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

struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    /// A repository on `main` with `a.txt`, `b.txt`, a `.gitignore` for
    /// `ignored.log`, and one commit.
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Self { dir };
        git(repo.path(), &["init", "-q", "-b", "main", "."]);
        repo.write("a.txt", "one\ntwo\nthree\n");
        repo.write("b.txt", "bee\n");
        repo.write(".gitignore", "ignored.log\n");
        git(repo.path(), &["add", "."]);
        git(
            repo.path(),
            &["commit", "-q", "--no-gpg-sign", "-m", "init"],
        );
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, name: &str, body: &str) {
        let path = self.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("dirs");
        }
        std::fs::write(path, body).expect("write");
    }

    fn git(&self, args: &[&str]) -> String {
        git(self.path(), args)
    }

    /// Paths in `tree`, recursively.
    fn tree_paths(&self, tree: &str) -> Vec<String> {
        self.git(&["ls-tree", "-r", "--name-only", tree])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn scratch_files(&self) -> Vec<String> {
        let git_dir = self.path().join(".git");
        std::fs::read_dir(git_dir)
            .expect("git dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(SCRATCH_INDEX_PREFIX))
            .collect()
    }
}

async fn capture_at(
    cwd: &Path,
    leaf: RefLeaf<'_>,
    phase: CapturePhase,
) -> Result<CapturedTree, CaptureFailure> {
    capture_tree(
        cwd,
        Some(&UNBOUNDED_FOR_TESTS),
        SESSION,
        1,
        leaf,
        None,
        phase,
    )
    .await
}

async fn capture(repo: &Repo, leaf: RefLeaf<'_>) -> CapturedTree {
    capture_at(repo.path(), leaf, CapturePhase::TurnEnd)
        .await
        .expect("capture")
}

async fn diff(repo: &Repo, base: &str, tree: &str) -> Vec<ChangedFile> {
    let result = diff_tree_files(repo.path(), Some(&UNBOUNDED_FOR_TESTS), base, tree)
        .await
        .expect("diff");
    assert_eq!(
        result.not_listed, 0,
        "every path in these fixtures is UTF-8"
    );
    result.files
}

#[tokio::test]
async fn an_edit_between_baseline_and_end_changes_the_tree_and_lists_the_file() {
    let repo = Repo::new();
    let base = capture(&repo, RefLeaf::Base(41)).await;
    repo.write("a.txt", "one\nTWO\nthree\nfour\n");
    let end = capture(&repo, RefLeaf::Through(58)).await;

    assert_ne!(base.tree, end.tree);
    assert_eq!(base.head, end.head);
    assert_eq!(end.branch.as_deref(), Some("main"));
    assert!(end.complete && end.omitted.is_empty());
    assert_eq!(
        repo.git(&["rev-parse", "refs/beekeeper/checkpoints/sess-1/1/base-41"]),
        base.commit
    );
    assert_eq!(
        repo.git(&["rev-parse", "refs/beekeeper/checkpoints/sess-1/1/58"]),
        end.commit
    );
    assert_eq!(
        repo.git(&["rev-parse", &format!("{}^{{tree}}", end.commit)]),
        end.tree
    );
    assert_eq!(
        repo.git(&["rev-parse", &format!("{}^", end.commit)]),
        repo.git(&["rev-parse", "HEAD"]),
        "the checkpoint is parented on HEAD"
    );
    assert_eq!(
        diff(&repo, &base.tree, &end.tree).await,
        vec![ChangedFile {
            path: "a.txt".to_owned(),
            status: FileChange::Modified,
            from: None,
            additions: Some(2),
            deletions: Some(1),
        }]
    );
}

#[tokio::test]
async fn the_real_index_head_and_branches_are_untouched() {
    let repo = Repo::new();
    repo.write("a.txt", "changed\n");
    repo.write("staged.txt", "staged\n");
    repo.git(&["add", "staged.txt"]);
    repo.write("new.txt", "new\n");
    // Read the refs and status first: `git status` refreshes (rewrites) the
    // index itself, which would hide or fake a rewrite by the capture.
    let head_before = repo.git(&["symbolic-ref", "HEAD"]);
    let refs_before = repo.git(&["for-each-ref", "refs/heads", "refs/tags"]);
    let status_before = repo.git(&["status", "--porcelain"]);
    let index = repo.path().join(".git/index");
    // Age the index so an incidental rewrite would show in its mtime.
    let old = std::time::SystemTime::now() - Duration::from_secs(120);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&index)
        .and_then(|file| file.set_modified(old))
        .expect("age index");
    let bytes_before = std::fs::read(&index).expect("index");
    let mtime_before = std::fs::metadata(&index)
        .and_then(|meta| meta.modified())
        .expect("mtime");

    let captured = capture(&repo, RefLeaf::Through(3)).await;

    assert_eq!(std::fs::read(&index).expect("index"), bytes_before);
    assert_eq!(
        std::fs::metadata(&index)
            .and_then(|meta| meta.modified())
            .expect("mtime"),
        mtime_before
    );
    assert_eq!(repo.git(&["symbolic-ref", "HEAD"]), head_before);
    assert_eq!(
        repo.git(&["for-each-ref", "refs/heads", "refs/tags"]),
        refs_before
    );
    assert_eq!(repo.git(&["status", "--porcelain"]), status_before);
    assert!(repo.scratch_files().is_empty(), "scratch index removed");
    // And the capture holds the worktree, staged or not.
    let paths = repo.tree_paths(&captured.tree);
    for expected in ["a.txt", "new.txt", "staged.txt"] {
        assert!(paths.iter().any(|p| p == expected), "{expected}: {paths:?}");
    }
}

#[tokio::test]
async fn no_hook_runs_during_a_capture() {
    let repo = Repo::new();
    let marker_dir = tempfile::tempdir().expect("marker dir");
    let hooks = repo.path().join(".git/hooks");
    std::fs::create_dir_all(&hooks).expect("hooks");
    let names = [
        "pre-commit",
        "post-commit",
        "post-index-change",
        "reference-transaction",
        "post-checkout",
    ];
    for name in names {
        let hook = hooks.join(name);
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\ntouch '{}'\n",
                marker_dir.path().join(name).display()
            ),
        )
        .expect("hook");
        let mut perms = std::fs::metadata(&hook).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&hook, perms).expect("chmod");
    }
    // A positive control: the hooks are live for ordinary git.
    repo.git(&["update-ref", "refs/control/probe", "HEAD"]);
    assert!(marker_dir.path().join("reference-transaction").exists());
    std::fs::remove_file(marker_dir.path().join("reference-transaction")).expect("reset");

    repo.write("a.txt", "edited\n");
    capture(&repo, RefLeaf::Through(9)).await;

    for name in names {
        assert!(
            !marker_dir.path().join(name).exists(),
            "{name} ran during a capture"
        );
    }
}

#[tokio::test]
async fn untracked_files_are_captured_and_ignored_ones_are_not() {
    let repo = Repo::new();
    repo.write("fresh/new.rs", "fn main() {}\n");
    repo.write("ignored.log", "noise\n");
    let captured = capture(&repo, RefLeaf::Through(2)).await;
    let paths = repo.tree_paths(&captured.tree);
    assert!(paths.iter().any(|p| p == "fresh/new.rs"), "{paths:?}");
    assert!(!paths.iter().any(|p| p == "ignored.log"), "{paths:?}");
    assert!(captured.complete);
}

#[tokio::test]
async fn an_untracked_file_over_sixteen_mebibytes_is_omitted_by_name() {
    let repo = Repo::new();
    let big = vec![b'x'; 17 * 1024 * 1024];
    std::fs::write(repo.path().join("huge.bin"), big).expect("big");
    repo.write("small.txt", "small\n");
    let captured = capture(&repo, RefLeaf::Through(4)).await;
    assert!(!captured.complete);
    assert_eq!(
        captured.omitted,
        vec![OmittedPath {
            path: "huge.bin".to_owned(),
            reason: OmitReason::TooLarge,
        }]
    );
    let paths = repo.tree_paths(&captured.tree);
    assert!(!paths.iter().any(|p| p == "huge.bin"));
    assert!(paths.iter().any(|p| p == "small.txt"));
}

#[tokio::test]
async fn an_unreadable_untracked_file_is_omitted_by_name() {
    let repo = Repo::new();
    repo.write("secret.txt", "no\n");
    let path = repo.path().join("secret.txt");
    let mut perms = std::fs::metadata(&path).expect("meta").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o000);
    std::fs::set_permissions(&path, perms).expect("chmod");
    if std::fs::File::open(&path).is_ok() {
        // Running as root: nothing is unreadable, so there is nothing to test.
        return;
    }
    let captured = capture(&repo, RefLeaf::Through(5)).await;
    assert_eq!(
        captured.omitted,
        vec![OmittedPath {
            path: "secret.txt".to_owned(),
            reason: OmitReason::Unreadable,
        }]
    );
    assert!(!captured.complete);
}

#[tokio::test]
async fn an_unreadable_tracked_file_is_omitted_by_name_and_keeps_its_committed_content() {
    let repo = Repo::new();
    repo.write("b.txt", "edited but unreadable\n");
    let path = repo.path().join("b.txt");
    let mut perms = std::fs::metadata(&path).expect("meta").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o000);
    std::fs::set_permissions(&path, perms).expect("chmod");
    if std::fs::File::open(&path).is_ok() {
        return;
    }
    let captured = capture(&repo, RefLeaf::Through(6)).await;
    assert_eq!(
        captured.omitted,
        vec![OmittedPath {
            path: "b.txt".to_owned(),
            reason: OmitReason::Unreadable,
        }]
    );
    assert_eq!(
        repo.git(&["cat-file", "-p", &format!("{}:b.txt", captured.tree)]),
        "bee"
    );
}

#[tokio::test]
async fn a_signing_configuration_with_a_failing_program_does_not_stop_a_capture() {
    let repo = Repo::new();
    repo.git(&["config", "commit.gpgSign", "true"]);
    repo.git(&["config", "gpg.program", "/usr/bin/false"]);
    repo.git(&["config", "gpg.format", "openpgp"]);
    repo.write("a.txt", "signed?\n");
    let captured = capture(&repo, RefLeaf::Through(7)).await;
    let raw = repo.git(&["cat-file", "commit", &captured.commit]);
    assert!(!raw.contains("gpgsig"), "{raw}");
    assert!(
        raw.contains("Beekeeper checkpoint <checkpoint@beekeeper.invalid>"),
        "{raw}"
    );
}

#[tokio::test]
async fn a_host_with_no_identity_still_captures() {
    let repo = Repo::new();
    repo.git(&["config", "user.useConfigOnly", "true"]);
    repo.write("a.txt", "who?\n");
    capture(&repo, RefLeaf::Through(8)).await;
}

#[tokio::test]
async fn a_plain_directory_is_not_a_repository() {
    let dir = tempfile::tempdir().expect("tempdir");
    let failure = capture_at(dir.path(), RefLeaf::Base(1), CapturePhase::Baseline)
        .await
        .expect_err("not a repo");
    assert_eq!(failure.code, UnavailableCode::NotARepository);
    assert_eq!(failure.code.as_str(), "NOT_A_REPOSITORY");

    let gone = dir.path().join("gone");
    let failure = capture_at(&gone, RefLeaf::Base(1), CapturePhase::Baseline)
        .await
        .expect_err("missing");
    assert_eq!(failure.code, UnavailableCode::NotARepository);
}

#[tokio::test]
async fn no_boundary_means_no_capture() {
    let repo = Repo::new();
    let failure = capture_tree(
        repo.path(),
        None,
        SESSION,
        1,
        RefLeaf::Base(1),
        None,
        CapturePhase::Baseline,
    )
    .await
    .expect_err("refused");
    assert_eq!(failure.code, UnavailableCode::BoundaryUnprepared);
    assert_eq!(failure.code.as_str(), "BOUNDARY_UNPREPARED");
    assert!(repo.git(&["for-each-ref", CHECKPOINT_REF_ROOT]).is_empty());
}

#[tokio::test]
async fn a_capture_that_runs_out_of_time_says_so_and_leaves_no_scratch_index() {
    let repo = Repo::new();
    let failure = capture_tree_within(
        repo.path(),
        Some(&UNBOUNDED_FOR_TESTS),
        SESSION,
        1,
        RefLeaf::Base(1),
        None,
        Duration::ZERO,
    )
    .await
    .expect_err("timed out");
    assert_eq!(failure.code, UnavailableCode::TimedOut);
    assert_eq!(failure.code.as_str(), "TIMED_OUT");
    // The dropped child is killed; give it a moment before looking.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(repo.scratch_files().is_empty());
    assert!(repo.git(&["for-each-ref", CHECKPOINT_REF_ROOT]).is_empty());
    assert!(BASELINE_CAPTURE_TIMEOUT < END_CAPTURE_TIMEOUT);
    assert_eq!(CapturePhase::Baseline.timeout(), BASELINE_CAPTURE_TIMEOUT);
}

#[tokio::test]
async fn a_failing_step_is_git_failed_and_still_removes_the_scratch_index() {
    let repo = Repo::new();
    let failure = capture_tree(
        repo.path(),
        Some(&UNBOUNDED_FOR_TESTS),
        SESSION,
        1,
        RefLeaf::Through(2),
        // A well-formed id naming no commit: resetting the scratch index to
        // it fails, and so does the fresh `read-tree` fallback.
        Some("0123456789abcdef0123456789abcdef01234567"),
        CapturePhase::TurnEnd,
    )
    .await
    .expect_err("bad parent");
    assert_eq!(failure.code, UnavailableCode::GitFailed);
    assert_eq!(failure.code.as_str(), "GIT_FAILED");
    assert!(failure.sentence.contains("read-tree"), "{failure:?}");
    assert!(repo.scratch_files().is_empty());
}

#[tokio::test]
async fn renames_additions_deletions_and_binaries_are_classified() {
    let repo = Repo::new();
    repo.write("long.txt", &"same line\n".repeat(40));
    std::fs::write(repo.path().join("pic.bin"), [0u8, 1, 2, 0, 3]).expect("bin");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-q", "--no-gpg-sign", "-m", "more"]);
    let base = capture(&repo, RefLeaf::Base(10)).await;

    std::fs::rename(repo.path().join("long.txt"), repo.path().join("moved.txt")).expect("mv");
    std::fs::remove_file(repo.path().join("b.txt")).expect("rm");
    repo.write("added.txt", "hello\n");
    std::fs::write(repo.path().join("pic.bin"), [0u8, 9, 9, 0, 9, 9]).expect("bin");
    let end = capture(&repo, RefLeaf::Through(20)).await;

    let files = diff(&repo, &base.tree, &end.tree).await;
    let find = |path: &str| {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("{path} in {files:?}"))
            .clone()
    };
    assert_eq!(find("added.txt").status, FileChange::Added);
    assert_eq!(find("added.txt").additions, Some(1));
    assert_eq!(find("b.txt").status, FileChange::Deleted);
    assert_eq!(find("b.txt").deletions, Some(1));
    let moved = find("moved.txt");
    assert_eq!(moved.status, FileChange::Renamed);
    assert_eq!(moved.from.as_deref(), Some("long.txt"));
    assert_eq!((moved.additions, moved.deletions), (Some(0), Some(0)));
    let pic = find("pic.bin");
    assert_eq!(pic.status, FileChange::Modified);
    assert_eq!((pic.additions, pic.deletions), (None, None));
    assert_eq!(files.len(), 4, "{files:?}");
    assert_eq!(FileChange::Renamed.as_str(), "renamed");
}

#[tokio::test]
async fn an_assume_unchanged_edit_is_still_captured() {
    let repo = Repo::new();
    repo.git(&["update-index", "--assume-unchanged", "a.txt"]);
    repo.write("a.txt", "hidden from status\n");
    let captured = capture(&repo, RefLeaf::Through(11)).await;
    assert_eq!(
        repo.git(&["cat-file", "-p", &format!("{}:a.txt", captured.tree)]),
        "hidden from status"
    );
}

#[tokio::test]
async fn an_unborn_branch_captures_without_a_parent() {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-q", "-b", "fresh", "."]);
    std::fs::write(dir.path().join("first.txt"), "first\n").expect("write");
    let captured = capture_at(dir.path(), RefLeaf::Base(1), CapturePhase::Baseline)
        .await
        .expect("capture");
    assert_eq!(captured.head, None);
    assert_eq!(captured.branch.as_deref(), Some("fresh"));
    let raw = git(dir.path(), &["cat-file", "commit", &captured.commit]);
    assert!(!raw.contains("\nparent "), "{raw}");
    assert!(git(
        dir.path(),
        &["ls-tree", "-r", "--name-only", &captured.tree]
    )
    .contains("first.txt"));
}

#[tokio::test]
async fn a_subdirectory_cwd_reports_repository_relative_paths() {
    let repo = Repo::new();
    repo.write("pkg/lib.rs", "pub fn a() {}\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-q", "--no-gpg-sign", "-m", "pkg"]);
    let pkg = repo.path().join("pkg");
    std::fs::write(pkg.join("huge.dat"), vec![b'y'; 17 * 1024 * 1024]).expect("big");
    std::fs::write(pkg.join("lib.rs"), "pub fn b() {}\n").expect("edit");
    let captured = capture_at(&pkg, RefLeaf::Through(3), CapturePhase::TurnEnd)
        .await
        .expect("capture");
    assert_eq!(
        captured.omitted,
        vec![OmittedPath {
            path: "pkg/huge.dat".to_owned(),
            reason: OmitReason::TooLarge,
        }]
    );
    let head_tree = repo.git(&["rev-parse", "HEAD^{tree}"]);
    let files = diff(&repo, &head_tree, &captured.tree).await;
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].path, "pkg/lib.rs");
}

#[tokio::test]
async fn a_linked_worktree_captures_into_the_shared_repository() {
    let repo = Repo::new();
    let holder = tempfile::tempdir().expect("holder");
    let linked = holder.path().join("seat");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "seat",
        linked.to_str().expect("utf8"),
    ]);
    std::fs::write(linked.join("seat.txt"), "from the seat\n").expect("write");
    let captured = capture_at(&linked, RefLeaf::Through(5), CapturePhase::TurnEnd)
        .await
        .expect("capture");
    assert_eq!(captured.branch.as_deref(), Some("seat"));
    // The ref is shared, so the main checkout sees it and gc keeps the tree.
    assert_eq!(
        repo.git(&["rev-parse", "refs/beekeeper/checkpoints/sess-1/1/5"]),
        captured.commit
    );
    let admin = repo.path().join(".git/worktrees/seat");
    let leftovers: Vec<_> = std::fs::read_dir(admin)
        .expect("admin")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(SCRATCH_INDEX_PREFIX)
        })
        .collect();
    assert!(leftovers.is_empty());
    repo.git(&[
        "worktree",
        "remove",
        "--force",
        linked.to_str().expect("utf8"),
    ]);
}

#[tokio::test]
async fn no_returned_string_carries_the_host_path() {
    let repo = Repo::new();
    let root = repo.path().to_string_lossy().into_owned();
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    std::fs::write(repo.path().join("huge.bin"), vec![b'z'; 17 * 1024 * 1024]).expect("big");
    let base = capture(&repo, RefLeaf::Base(1)).await;
    repo.write("a.txt", "x\n");
    let end = capture(&repo, RefLeaf::Through(2)).await;
    let files = diff(&repo, &base.tree, &end.tree).await;
    let plain = tempfile::tempdir().expect("plain");
    let mut failures = vec![
        capture_at(plain.path(), RefLeaf::Base(1), CapturePhase::Baseline)
            .await
            .expect_err("plain"),
        capture_at(
            &plain.path().join("gone"),
            RefLeaf::Base(1),
            CapturePhase::Baseline,
        )
        .await
        .expect_err("gone"),
    ];
    failures.push(
        capture_tree(
            repo.path(),
            Some(&UNBOUNDED_FOR_TESTS),
            SESSION,
            1,
            RefLeaf::Base(2),
            Some("0123456789abcdef0123456789abcdef01234567"),
            CapturePhase::TurnEnd,
        )
        .await
        .expect_err("bad parent"),
    );
    let rendered = format!("{base:?}{end:?}{files:?}{failures:?}");
    for host in [
        root.as_str(),
        tmp.trim_end_matches('/'),
        "/private/",
        "/var/folders",
    ] {
        assert!(!rendered.contains(host), "{host} leaked: {rendered}");
    }
    for failure in &failures {
        assert!(failure.sentence.len() <= MAX_SENTENCE_BYTES);
    }
}

#[test]
fn a_session_id_cannot_escape_its_ref_namespace() {
    assert_eq!(
        checkpoint_ref("abc-1_2.x", 3, RefLeaf::Base(41)).as_deref(),
        Some("refs/beekeeper/checkpoints/abc-1_2.x/3/base-41")
    );
    assert_eq!(
        checkpoint_ref("abc", 3, RefLeaf::Through(58)).as_deref(),
        Some("refs/beekeeper/checkpoints/abc/3/58")
    );
    for bad in [
        "",
        "..",
        "a/../../heads/main",
        "a/b",
        ".hidden",
        "x.lock",
        "a..b",
        "sp ace",
        "tail.",
    ] {
        assert_eq!(checkpoint_ref(bad, 1, RefLeaf::Base(1)), None, "{bad}");
    }
    assert_eq!(checkpoint_ref(&"a".repeat(129), 1, RefLeaf::Base(1)), None);
}

#[tokio::test]
async fn an_unsafe_session_id_refuses_before_running_git() {
    let repo = Repo::new();
    let failure = capture_tree(
        repo.path(),
        Some(&UNBOUNDED_FOR_TESTS),
        "../../heads/main",
        1,
        RefLeaf::Base(1),
        None,
        CapturePhase::Baseline,
    )
    .await
    .expect_err("refused");
    assert_eq!(failure.code, UnavailableCode::GitFailed);
    assert_eq!(repo.git(&["for-each-ref", "refs/heads"]).lines().count(), 1);
}

#[test]
fn wire_spellings_match_the_checkpoint_schema() {
    let codes = [
        UnavailableCode::NotARepository,
        UnavailableCode::BoundaryUnprepared,
        UnavailableCode::TimedOut,
        UnavailableCode::GitFailed,
    ]
    .map(UnavailableCode::as_str);
    assert_eq!(
        codes,
        [
            "NOT_A_REPOSITORY",
            "BOUNDARY_UNPREPARED",
            "TIMED_OUT",
            "GIT_FAILED"
        ]
    );
    assert_eq!(
        [OmitReason::TooLarge, OmitReason::Unreadable].map(OmitReason::as_str),
        ["too_large", "unreadable"]
    );
    assert_eq!(
        [
            FileChange::Added,
            FileChange::Modified,
            FileChange::Deleted,
            FileChange::Renamed
        ]
        .map(FileChange::as_str),
        ["added", "modified", "deleted", "renamed"]
    );
}

#[test]
fn sentences_are_bounded_on_a_character_boundary() {
    let long = "é".repeat(400);
    let bounded = bounded_sentence(long);
    assert!(bounded.len() <= MAX_SENTENCE_BYTES);
    assert!(bounded.ends_with('…'));
}

#[test]
fn numstat_and_name_status_parse_renames_and_binaries() {
    let numstat = b"1\t0\tadded.txt\0-\t-\tpic.bin\x000\t0\t\0old.txt\0new.txt\0";
    let names = b"A\0added.txt\0M\0pic.bin\0R100\0old.txt\0new.txt\0D\0gone.txt\0T\0link\0";
    let result = merge_diff(&parse_name_status(names), &parse_numstat(numstat));
    assert_eq!(result.not_listed, 0);
    assert_eq!(
        result.files,
        vec![
            ChangedFile {
                path: "added.txt".to_owned(),
                status: FileChange::Added,
                from: None,
                additions: Some(1),
                deletions: Some(0),
            },
            ChangedFile {
                path: "gone.txt".to_owned(),
                status: FileChange::Deleted,
                from: None,
                additions: None,
                deletions: None,
            },
            ChangedFile {
                path: "link".to_owned(),
                status: FileChange::Modified,
                from: None,
                additions: None,
                deletions: None,
            },
            ChangedFile {
                path: "new.txt".to_owned(),
                status: FileChange::Renamed,
                from: Some("old.txt".to_owned()),
                additions: Some(0),
                deletions: Some(0),
            },
            ChangedFile {
                path: "pic.bin".to_owned(),
                status: FileChange::Modified,
                from: None,
                additions: None,
                deletions: None,
            },
        ]
    );
}

/// Measured cost of a warm baseline capture. Not part of the gate: point
/// `BK_CHECKPOINT_BENCH_REPO` at a throwaway `git clone --local` (never a
/// checkout that shares a common dir with someone's live repository) and run
/// `cargo test -p buzz-session-provider turn_checkpoint_git -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "benchmark: needs BK_CHECKPOINT_BENCH_REPO"]
async fn bench_baseline_capture() {
    let Some(repo) = std::env::var_os("BK_CHECKPOINT_BENCH_REPO") else {
        return;
    };
    let repo = PathBuf::from(repo);
    // Warm-up.
    capture_tree(
        &repo,
        Some(&UNBOUNDED_FOR_TESTS),
        "bench",
        1,
        RefLeaf::Base(0),
        None,
        CapturePhase::Baseline,
    )
    .await
    .expect("warm-up");
    let mut samples = Vec::new();
    for run in 1..=10u64 {
        let started = std::time::Instant::now();
        capture_tree(
            &repo,
            Some(&UNBOUNDED_FOR_TESTS),
            "bench",
            1,
            RefLeaf::Base(run),
            None,
            CapturePhase::Baseline,
        )
        .await
        .expect("capture");
        samples.push(started.elapsed().as_millis());
    }
    samples.sort_unstable();
    println!(
        "baseline capture ms: {samples:?} p50={}",
        (samples[4] + samples[5]) / 2
    );
}

/// The capture inside the real macOS boundary: an ordinary checkout and a
/// linked worktree both capture, and the boundary does refuse a shared ref to
/// the seat — the measured reason `update-ref` runs as the host.
#[cfg(target_os = "macos")]
mod bounded {
    use super::*;
    use crate::execution_scope_host::{prepare_host_command, HostCommandScope};

    fn plan(state: &Path, checkout: &Path, tree: &Path) -> HostLaunchPlan {
        match prepare_host_command(&HostCommandScope::git(
            state,
            Some("30621:aa:a"),
            Some(checkout),
            tree,
            "checkpoint",
        )) {
            Ok(plan @ HostLaunchPlan::Bounded(_)) => plan,
            other => panic!("bounded: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_bounded_capture_of_a_checkout_and_a_linked_worktree() {
        let root = tempfile::tempdir().expect("root");
        let root_path = root.path().canonicalize().expect("canonical");
        let state = root_path.join("state");
        std::fs::create_dir_all(&state).expect("state");
        let checkout = root_path.join("repo");
        std::fs::create_dir_all(&checkout).expect("checkout");
        git(&checkout, &["init", "-q", "-b", "main", "."]);
        std::fs::write(checkout.join("a.txt"), "a\n").expect("write");
        git(&checkout, &["add", "."]);
        git(&checkout, &["commit", "-q", "--no-gpg-sign", "-m", "init"]);

        std::fs::write(checkout.join("a.txt"), "edited\n").expect("edit");
        let own = plan(&state, &checkout, &checkout);
        let captured = capture_tree(
            &checkout,
            Some(&own),
            SESSION,
            1,
            RefLeaf::Through(2),
            None,
            CapturePhase::TurnEnd,
        )
        .await
        .expect("bounded checkout capture");
        assert!(captured.boundary_enforced);

        let linked = root_path.join("seat");
        git(
            &checkout,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "seat",
                linked.to_str().expect("utf8"),
            ],
        );
        std::fs::write(linked.join("seat.txt"), "seat\n").expect("write");
        let seat = plan(&state, &checkout, &linked);
        // The measured fact behind writing the ref as the host.
        let refused = seat
            .git_command(&linked, &["update-ref", "refs/beekeeper/probe", "HEAD"])
            .output()
            .expect("runs");
        assert!(
            !refused.status.success(),
            "the boundary let a seat write a shared ref"
        );
        let captured = capture_tree(
            &linked,
            Some(&seat),
            SESSION,
            1,
            RefLeaf::Through(3),
            None,
            CapturePhase::TurnEnd,
        )
        .await
        .expect("bounded linked capture");
        assert!(captured.boundary_enforced);
        assert_eq!(
            git(
                &checkout,
                &["rev-parse", "refs/beekeeper/checkpoints/sess-1/1/3"]
            ),
            captured.commit
        );
        assert!(
            git(&checkout, &["ls-tree", "-r", "--name-only", &captured.tree]).contains("seat.txt")
        );
        git(
            &checkout,
            &[
                "worktree",
                "remove",
                "--force",
                linked.to_str().expect("utf8"),
            ],
        );
    }
}
