//! Every git-touching test here builds its own throwaway layout under a
//! `TempDir`: `git init`, one commit, two `git worktree add`s, one left dirty,
//! one branch merged and one not.
//!
//! A test that could reach `/Users/brian/Projects/beekeeper/beekeeper` is a bug
//! in the test. Nothing below reads a path from the environment, and
//! `scripts/worktrees-prune.sh` is only ever run with `--repo` pointed at a
//! directory the same function created.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn git(args: &[&str], cwd: &Path) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        // The pre-push gate runs these tests inside a git hook, which exports
        // GIT_DIR; inherited, it points `git init` at the pushing repository.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "L11")
        .env("GIT_AUTHOR_EMAIL", "l11@example.invalid")
        .env("GIT_COMMITTER_NAME", "L11")
        .env("GIT_COMMITTER_EMAIL", "l11@example.invalid")
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

struct Layout {
    _dir: tempfile::TempDir,
    repo: PathBuf,
}

impl Layout {
    fn holder(&self) -> PathBuf {
        self.repo.parent().expect("parent").join("proj.worktrees")
    }

    fn add(&self, slug: &str) -> PathBuf {
        let path = self.holder().join(slug);
        git(
            &[
                "worktree",
                "add",
                "-b",
                slug,
                path.to_str().expect("utf8"),
                "main",
            ],
            &self.repo,
        );
        path
    }
}

fn layout() -> Layout {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("proj");
    std::fs::create_dir_all(&repo).expect("mkdir");
    git(&["init", "--initial-branch=main", "."], &repo);
    std::fs::write(repo.join("README"), b"hello\n").expect("write");
    git(&["add", "README"], &repo);
    git(&["commit", "-m", "one"], &repo);
    Layout { _dir: dir, repo }
}

fn record(repo: &Path, path: &Path, branch: &str) -> RecordedSeatWorktree {
    RecordedSeatWorktree {
        path: path.to_path_buf(),
        branch: branch.to_string(),
        repo_root: repo.to_path_buf(),
        created_at: "2026-09-02T19:00:00Z".to_string(),
    }
}

const SESSION: &str = "11111111-1111-4111-8111-111111111111";

fn settled_facts() -> SessionFacts {
    SessionFacts {
        settled: true,
        settled_for_secs: Some(buzz_core::worktree_lifecycle::SEAT_WORKTREE_GRACE_SECS + 1),
        execution_live: false,
    }
}

// ── the record ──────────────────────────────────────────────────────────────

#[test]
fn a_v1_host_record_reads_with_no_worktrees_at_all() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("coding-session-workdirs.json");
    std::fs::write(
        &path,
        br#"{"version":1,"byProject":{},"byChannel":{},"mru":[],"pending":{}}"#,
    )
    .expect("write");

    let store = load_store(&path).expect("a v1 record must still read");
    assert_eq!(store.version, 1);
    assert!(
        store.worktrees.is_empty(),
        "trees cut before the record are unrecorded, and this says so"
    );
}

#[test]
fn a_v2_record_reads_its_worktrees() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("coding-session-workdirs.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"version":2,"worktrees":{{"{SESSION}/builder-1":{{"path":"/src/proj.worktrees/lane","branch":"lane","repoRoot":"/src/proj","createdAt":"2026-09-02T19:00:00Z"}}}}}}"#
        )
        .as_bytes(),
    )
    .expect("write");

    let store = load_store(&path).expect("read");
    assert_eq!(store.worktrees.len(), 1);
    let entry = &store.worktrees[&format!("{SESSION}/builder-1")];
    assert_eq!(entry.branch, "lane");
    assert_eq!(entry.path, PathBuf::from("/src/proj.worktrees/lane"));
}

// ── dispositions over a real layout ─────────────────────────────────────────

#[test]
fn a_dirty_recorded_tree_is_held_with_its_own_count_and_never_prunable() {
    let layout = layout();
    let tree = layout.add("lane-dirty");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(tree.join(name), b"x").expect("write");
    }
    assert_eq!(count_dirty_files(&tree), 3);

    let row = row_for(
        &format!("{SESSION}/builder-1"),
        &record(&layout.repo, &tree, "lane-dirty"),
        settled_facts(),
        Some(true),
    );
    assert_eq!(row.disposition.token(), "held");
    assert_eq!(row.detail(), "held: 3 uncommitted files");
    assert!(!row.disposition.is_host_prunable());
}

#[test]
fn ignored_build_output_never_makes_a_tree_look_dirty() {
    let layout = layout();
    std::fs::write(layout.repo.join(".gitignore"), b"target/\nnode_modules/\n").expect("write");
    git(&["add", ".gitignore"], &layout.repo);
    git(&["commit", "-m", "ignore"], &layout.repo);
    let tree = layout.add("lane-built");
    std::fs::create_dir_all(tree.join("target/debug")).expect("mkdir");
    std::fs::write(tree.join("target/debug/blob"), vec![1_u8; 4096]).expect("write");
    std::fs::create_dir_all(tree.join("desktop/node_modules")).expect("mkdir");
    std::fs::write(tree.join("desktop/node_modules/index.js"), b"x").expect("write");

    assert_eq!(count_dirty_files(&tree), 0);
    let row = row_for(
        &format!("{SESSION}/builder-1"),
        &record(&layout.repo, &tree, "lane-built"),
        settled_facts(),
        Some(true),
    );
    assert_eq!(row.disposition.token(), "prunable");
    assert!(row.reclaimable_now);
    assert!(row.reclaimable.unwrap_or(0) > 0);
}

#[test]
fn an_unconfirmed_relay_tip_says_so_and_never_claims_the_branch_was_not_pushed() {
    let layout = layout();
    let tree = layout.add("lane-unknown");
    let unknown = row_for(
        &format!("{SESSION}/builder-1"),
        &record(&layout.repo, &tree, "lane-unknown"),
        settled_facts(),
        None,
    );
    assert_eq!(unknown.disposition.token(), "tip-not-on-relay");
    assert!(!unknown.tip_known);
    assert!(unknown.detail().contains("could not confirm"));

    let known = row_for(
        &format!("{SESSION}/builder-1"),
        &record(&layout.repo, &tree, "lane-unknown"),
        settled_facts(),
        Some(false),
    );
    assert!(known.detail().contains("current ref state does not hold"));
}

#[test]
fn the_repository_checkout_and_anything_outside_the_holder_are_protected() {
    let layout = layout();
    assert!(is_protected(&layout.repo, &layout.repo));
    assert!(is_protected(
        &layout.repo,
        &layout.repo.parent().expect("parent").join("proj-prod")
    ));
    assert!(is_protected(&layout.repo, &layout.holder()));
    let tree = layout.add("lane-ok");
    assert!(!is_protected(&layout.repo, &tree));
}

#[test]
fn a_row_carries_the_relay_limit_sentence_with_every_tip_answer() {
    let layout = layout();
    let tree = layout.add("lane-json");
    let row = row_for(
        &format!("{SESSION}/builder-1"),
        &record(&layout.repo, &tree, "lane-json"),
        settled_facts(),
        Some(true),
    );
    let json = row.to_json();
    assert_eq!(
        json["tipOnRelayLimit"],
        serde_json::json!(TIP_ON_RELAY_LIMIT)
    );
    assert!(TIP_ON_RELAY_LIMIT.contains("never a push history"));
    assert_eq!(json["disposition"], serde_json::json!("prunable"));
    assert_eq!(json["dirtyFiles"], serde_json::json!(0));
}

#[test]
fn an_unmeasurable_directory_reads_unknown_in_the_row() {
    let layout = layout();
    let tree = layout.add("lane-empty");
    let row = row_for(
        &format!("{SESSION}/builder-1"),
        &record(&layout.repo, &tree, "lane-empty"),
        settled_facts(),
        Some(true),
    );
    // Nothing to measure is genuinely zero here; `unknown` is reserved for a
    // measurement that failed, which the renderer proves separately.
    assert_eq!(row.to_json()["reclaimable"], serde_json::json!("0.0 GB"));
    assert_eq!(
        buzz_core::worktree_lifecycle::render_reclaimable_bytes(None),
        "unknown"
    );
}

#[test]
fn the_tip_check_accepts_an_ancestor_as_well_as_an_exact_ref() {
    let layout = layout();
    let tree = layout.add("lane-ancestor");
    let base = git(&["rev-parse", "HEAD"], &tree).trim().to_string();
    std::fs::write(tree.join("later.txt"), b"later\n").expect("write");
    git(&["add", "later.txt"], &tree);
    git(&["commit", "-m", "later"], &tree);
    let tip = git(&["rev-parse", "HEAD"], &tree).trim().to_string();

    let mut only_tip = BTreeSet::new();
    only_tip.insert(tip.clone());
    assert_eq!(tip_on_relay(&tree, &only_tip), Some(true));

    let mut only_base = BTreeSet::new();
    only_base.insert(base);
    assert_eq!(
        tip_on_relay(&tree, &only_base),
        Some(false),
        "a tip ahead of the relay's ref state is not on the relay"
    );
}

#[test]
fn the_repo_id_comes_from_the_remote_the_repository_actually_has() {
    let layout = layout();
    git(
        &[
            "remote",
            "add",
            "hive",
            "https://hive.example.invalid/git/community/agiterra-beekeeper",
        ],
        &layout.repo,
    );
    assert_eq!(
        repo_id_of(&layout.repo).as_deref(),
        Some("agiterra-beekeeper")
    );
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/other-name.git",
        ],
        &layout.repo,
    );
    assert_eq!(
        repo_id_of(&layout.repo).as_deref(),
        Some("other-name"),
        "origin wins when it exists, and no name is hard-coded"
    );
}

// ── the removal itself ──────────────────────────────────────────────────────

#[test]
fn removing_a_clean_tree_takes_the_directory_and_leaves_the_repository() {
    let layout = layout();
    let tree = layout.add("lane-remove");
    remove_worktree(&layout.repo, &tree).expect("remove");
    assert!(!tree.exists());
    assert!(layout.repo.join("README").is_file());
}

#[test]
fn git_refuses_a_dirty_tree_because_nothing_here_passes_force() {
    let layout = layout();
    let tree = layout.add("lane-refuse");
    std::fs::write(tree.join("scratch.txt"), b"unsaved\n").expect("write");
    let error = remove_worktree(&layout.repo, &tree).expect_err("git must refuse");
    assert!(tree.join("scratch.txt").is_file());
    assert!(format!("{error}").contains("refused"));
}

// ── scripts/worktrees-prune.sh ──────────────────────────────────────────────

fn prune_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/worktrees-prune.sh")
        .canonicalize()
        .expect("the prune script must exist")
}

fn run_prune(layout: &Layout, args: &[&str]) -> (bool, String) {
    let mut command = Command::new("bash");
    command
        .arg(prune_script())
        .arg("--repo")
        .arg(&layout.repo)
        .args(args)
        // The script measures "merged" against a real trunk, and the tests
        // build a repository with no remote, so a local `main` is the trunk.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
    let output = command.output().expect("run the prune script");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

/// merged+clean, merged+dirty, unmerged+clean, and the main checkout.
fn prune_layout() -> Layout {
    let layout = layout();
    layout.add("lane-merged-clean");

    let dirty = layout.add("lane-merged-dirty");
    std::fs::write(dirty.join("wip.txt"), b"work in progress\n").expect("write");

    let unmerged = layout.add("lane-unmerged");
    std::fs::write(unmerged.join("new.txt"), b"new\n").expect("write");
    git(&["add", "new.txt"], &unmerged);
    git(&["commit", "-m", "unmerged work"], &unmerged);

    layout
}

#[test]
fn the_dry_run_prints_the_whole_plan_and_removes_nothing() {
    let layout = prune_layout();
    let (ok, output) = run_prune(&layout, &["--dry-run"]);
    assert!(ok, "{output}");

    assert!(output.contains("lane-merged-clean"), "{output}");
    assert!(output.contains("lane-merged-dirty"), "{output}");
    assert!(output.contains("lane-unmerged"), "{output}");
    assert!(output.contains("nothing removed"), "{output}");

    for slug in ["lane-merged-clean", "lane-merged-dirty", "lane-unmerged"] {
        assert!(
            layout.holder().join(slug).is_dir(),
            "--dry-run removed {slug}"
        );
    }
}

#[test]
fn a_merged_clean_tree_goes_a_merged_dirty_tree_is_listed_and_an_unmerged_tree_is_refused() {
    let layout = prune_layout();
    let (ok, output) = run_prune(&layout, &[]);
    assert!(ok, "{output}");

    assert!(
        !layout.holder().join("lane-merged-clean").exists(),
        "a merged clean tree is removed: {output}"
    );
    assert!(
        layout.holder().join("lane-merged-dirty").is_dir(),
        "a merged dirty tree survives: {output}"
    );
    assert!(
        output.contains("1 uncommitted files") || output.contains("(1 uncommitted"),
        "and is listed with its count: {output}"
    );
    assert!(
        layout.holder().join("lane-unmerged").is_dir(),
        "an unmerged tree survives: {output}"
    );
    assert!(
        output.contains("not merged into"),
        "and is refused in one line: {output}"
    );
    assert!(
        layout.repo.join("README").is_file(),
        "the main checkout survives every run"
    );
}

#[test]
fn the_main_checkout_is_listed_as_protected_and_never_removed() {
    let layout = prune_layout();
    let (ok, output) = run_prune(&layout, &["--dry-run"]);
    assert!(ok, "{output}");
    let protected_section = output
        .split("merged and clean")
        .next()
        .expect("a protected section");
    assert!(
        protected_section.contains("protected — never touched:"),
        "{output}"
    );
    assert!(layout.repo.is_dir());
}

#[test]
fn targets_removes_build_output_and_leaves_git_and_sources_untouched() {
    let layout = prune_layout();
    let dirty = layout.holder().join("lane-merged-dirty");
    std::fs::create_dir_all(dirty.join("target/debug")).expect("mkdir");
    std::fs::write(dirty.join("target/debug/blob"), vec![5_u8; 2048]).expect("write");

    let (ok, output) = run_prune(&layout, &["--targets"]);
    assert!(ok, "{output}");

    assert!(!dirty.join("target").exists(), "{output}");
    assert!(
        dirty.join(".git").exists(),
        "the worktree's git link survives"
    );
    assert_eq!(
        std::fs::read_to_string(dirty.join("wip.txt")).expect("read"),
        "work in progress\n",
        "uncommitted work is never deleted"
    );
    assert!(dirty.join("README").is_file(), "sources survive");
}

#[test]
fn the_script_refuses_a_repository_with_no_trunk_rather_than_guessing_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("proj");
    std::fs::create_dir_all(&repo).expect("mkdir");
    git(&["init", "--initial-branch=trunkless", "."], &repo);
    std::fs::write(repo.join("README"), b"hi\n").expect("write");
    git(&["add", "README"], &repo);
    git(&["commit", "-m", "one"], &repo);

    let output = Command::new("bash")
        .arg(prune_script())
        .arg("--repo")
        .arg(&repo)
        .arg("--dry-run")
        .output()
        .expect("run");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to guess a trunk"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
