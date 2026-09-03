//! Every git-touching test builds its own throwaway layout under a `TempDir`.
//!
//! A test that could reach `/Users/brian/Projects/beekeeper/beekeeper` is a bug
//! in the test, so nothing here reads an environment variable for a path, and
//! nothing here removes a directory it did not create in the same function.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::coding_sessions::workdir_store::{
    is_inside_worktree_parent, load_workdir_store_readonly_from, seat_worktree_key,
    CodingSessionWorkdirStore, WORKDIR_STORE_VERSION,
};

fn git(args: &[&str], cwd: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "L11")
        .env("GIT_AUTHOR_EMAIL", "l11@example.invalid")
        .env("GIT_COMMITTER_NAME", "L11")
        .env("GIT_COMMITTER_EMAIL", "l11@example.invalid")
        .output()
        .expect("run git");
    assert!(
        status.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

/// A repository with one commit, and its sibling `.worktrees` folder.
struct Layout {
    _dir: tempfile::TempDir,
    repo: PathBuf,
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

fn seat_record(repo: &Path, path: &Path, branch: &str) -> CodingSessionSeatWorktree {
    CodingSessionSeatWorktree {
        path: path.to_path_buf(),
        branch: branch.to_string(),
        repo_root: repo.to_path_buf(),
        created_at: "2026-09-02T19:00:00Z".to_string(),
    }
}

fn settled(session_ref: &str) -> CodingSessionWorktreeSessionFacts {
    CodingSessionWorktreeSessionFacts {
        session_ref: session_ref.to_string(),
        session_settled: true,
        execution_live: false,
        tip_on_relay: Some(true),
        settled_for_secs: Some(buzz_core_pkg::worktree_lifecycle::SEAT_WORKTREE_GRACE_SECS + 1),
    }
}

const SESSION: &str = "11111111-1111-4111-8111-111111111111";

// ── L11.1 the record ────────────────────────────────────────────────────────

#[test]
fn a_create_writes_exactly_one_record_under_its_seat_key() {
    let mut store = CodingSessionWorkdirStore::default();
    store
        .record_seat_worktree(
            SESSION,
            "builder-1",
            seat_record(
                Path::new("/src/proj"),
                Path::new("/src/proj.worktrees/lane"),
                "lane",
            ),
        )
        .expect("record");

    assert_eq!(store.worktrees.len(), 1);
    let key = seat_worktree_key(SESSION, "builder-1");
    assert_eq!(key, format!("{SESSION}/builder-1"));
    assert_eq!(
        store.worktrees[&key].path,
        PathBuf::from("/src/proj.worktrees/lane")
    );
}

#[test]
fn a_path_outside_the_worktrees_folder_is_refused_at_write() {
    let mut store = CodingSessionWorkdirStore::default();
    for outside in [
        "/src/proj",
        "/src/proj.worktrees",
        "/src/somewhere-else/lane",
        "/src/proj.worktrees-not",
        "proj.worktrees/lane",
    ] {
        let error = store
            .record_seat_worktree(
                SESSION,
                "builder-1",
                seat_record(Path::new("/src/proj"), Path::new(outside), "lane"),
            )
            .expect_err("must refuse");
        assert!(
            error.contains("outside the repository's worktrees folder"),
            "unexpected error for {outside}: {error}"
        );
    }
    assert!(store.worktrees.is_empty());
}

#[test]
fn a_v1_file_loads_with_the_map_empty_and_re_saves_as_v2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("coding-session-workdirs.json");
    std::fs::write(
        &path,
        br#"{"version":1,"byProject":{"30621:aa:general":{"path":"/src/proj","updatedAt":"2026-08-01T00:00:00Z"}},"byChannel":{},"mru":[],"pending":{}}"#,
    )
    .expect("write");

    let loaded = load_workdir_store_readonly_from(&path).expect("a v1 file must still load");
    assert_eq!(loaded.version, 1);
    assert!(
        loaded.worktrees.is_empty(),
        "the trees that predate the record are unrecorded, and that is the truth"
    );
    assert_eq!(loaded.by_project.len(), 1, "every v1 record survives");

    let mut migrated = loaded.clone();
    migrated.version = WORKDIR_STORE_VERSION;
    let re_saved = serde_json::to_vec_pretty(&migrated).expect("serialize");
    std::fs::write(&path, &re_saved).expect("write");
    let reloaded = load_workdir_store_readonly_from(&path).expect("reload");
    assert_eq!(reloaded.version, 2);
    assert_eq!(reloaded.by_project, loaded.by_project);
    assert!(reloaded.worktrees.is_empty());
}

#[test]
fn the_projects_view_is_byte_identical_to_what_it_was_before_the_record() {
    let mut store = CodingSessionWorkdirStore::default();
    store.set(
        crate::coding_sessions::workdir_store::CodingSessionWorkdirScope::Project,
        "30621:aa:general",
        PathBuf::from("/src/proj"),
    );
    let without = serde_json::to_string(&store.projects_view()).expect("serialize");
    store
        .record_seat_worktree(
            SESSION,
            "builder-1",
            seat_record(
                Path::new("/src/proj"),
                Path::new("/src/proj.worktrees/lane"),
                "lane",
            ),
        )
        .expect("record");
    let with = serde_json::to_string(&store.projects_view()).expect("serialize");

    assert_eq!(
        without, with,
        "the provider resolves a cwd; it does not reap, so it never sees this map"
    );
    let parsed: serde_json::Value = serde_json::from_str(&with).expect("json");
    let mut keys: Vec<String> = parsed
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, vec!["channels", "pending", "projects", "version"]);
}

#[test]
fn the_worktrees_folder_test_accepts_only_children_of_the_holder() {
    let repo = Path::new("/src/proj");
    assert!(is_inside_worktree_parent(
        repo,
        Path::new("/src/proj.worktrees/lane")
    ));
    assert!(is_inside_worktree_parent(
        repo,
        Path::new("/src/proj.worktrees/lane/deeper")
    ));
    assert!(!is_inside_worktree_parent(
        repo,
        Path::new("/src/proj.worktrees")
    ));
    assert!(!is_inside_worktree_parent(repo, Path::new("/src/proj")));
}

// ── L11.2/L11.3 dispositions over a real layout ─────────────────────────────

#[test]
fn a_dirty_tree_reads_held_with_its_own_count() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-dirty");
    std::fs::write(tree.join("a.txt"), b"1").expect("write");
    std::fs::write(tree.join("b.txt"), b"2").expect("write");
    std::fs::write(tree.join("c.txt"), b"3").expect("write");

    assert_eq!(count_dirty_files(&tree), 3);

    let row = build_row(
        &format!("{SESSION}/builder-1"),
        &seat_record(&layout.repo, &tree, "lane-dirty"),
        &settled(SESSION),
    );
    assert_eq!(row.disposition, "held");
    assert_eq!(row.dirty_files, 3);
    assert_eq!(row.detail, "held: 3 uncommitted files");
    assert_eq!(row.seat_label, "builder-1");
    assert_eq!(row.session_ref, SESSION);
}

#[test]
fn ignored_build_output_never_makes_a_tree_look_dirty() {
    let layout = layout();
    std::fs::write(layout.repo.join(".gitignore"), b"target/\nnode_modules/\n").expect("write");
    git(&["add", ".gitignore"], &layout.repo);
    git(&["commit", "-m", "ignore"], &layout.repo);
    let tree = add_worktree(&layout, "lane-built");
    std::fs::create_dir_all(tree.join("target/debug")).expect("mkdir");
    std::fs::write(tree.join("target/debug/blob"), vec![7_u8; 4096]).expect("write");
    std::fs::create_dir_all(tree.join("desktop/node_modules/pkg")).expect("mkdir");
    std::fs::write(tree.join("desktop/node_modules/pkg/index.js"), b"x").expect("write");

    assert_eq!(
        count_dirty_files(&tree),
        0,
        "git status --porcelain excludes ignored paths"
    );
    let row = build_row(
        &format!("{SESSION}/builder-1"),
        &seat_record(&layout.repo, &tree, "lane-built"),
        &settled(SESSION),
    );
    assert_eq!(row.disposition, "prunable");
    assert_eq!(row.detail, format!("{}: clean, will be removed", row.path));
    assert!(row.reclaimable_now);
    assert!(
        row.reclaimable_bytes.unwrap_or(0) > 0,
        "the build output is measured, not guessed"
    );
}

#[test]
fn an_unconfirmed_relay_tip_says_it_could_not_confirm_never_that_it_is_missing() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-unknown");
    let mut facts = settled(SESSION);
    facts.tip_on_relay = None;

    let row = build_row(
        &format!("{SESSION}/builder-1"),
        &seat_record(&layout.repo, &tree, "lane-unknown"),
        &facts,
    );
    assert_eq!(row.disposition, "tip-not-on-relay");
    assert!(!row.tip_on_relay_known);
    assert!(
        row.detail.contains("could not confirm"),
        "unknown is not false: {}",
        row.detail
    );

    facts.tip_on_relay = Some(false);
    let known = build_row(
        &format!("{SESSION}/builder-1"),
        &seat_record(&layout.repo, &tree, "lane-unknown"),
        &facts,
    );
    assert!(known.tip_on_relay_known);
    assert!(known.detail.contains("current ref state does not hold"));
}

#[test]
fn the_repositorys_own_checkout_is_protected_and_so_is_anything_outside_the_holder() {
    let layout = layout();
    assert!(is_protected_worktree(&layout.repo, &layout.repo));
    assert!(is_protected_worktree(
        &layout.repo,
        &layout.repo.parent().expect("parent").join("proj-prod")
    ));
    let tree = add_worktree(&layout, "lane-ok");
    assert!(!is_protected_worktree(&layout.repo, &tree));

    let row = build_row(
        &format!("{SESSION}/builder-1"),
        &seat_record(&layout.repo, &layout.repo, "main"),
        &settled(SESSION),
    );
    assert_eq!(row.disposition, "protected");
    assert_eq!(row.dirty_files, 0);
}

// ── L11.4 build output ──────────────────────────────────────────────────────

#[test]
fn reclaim_removes_both_build_directories_and_leaves_every_source_file_identical() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-reclaim");
    std::fs::write(tree.join("src.txt"), b"source of truth\n").expect("write");
    std::fs::create_dir_all(tree.join("target/debug")).expect("mkdir");
    std::fs::write(tree.join("target/debug/blob"), vec![9_u8; 8192]).expect("write");
    std::fs::create_dir_all(tree.join("desktop/node_modules/pkg")).expect("mkdir");
    std::fs::write(tree.join("desktop/node_modules/pkg/index.js"), b"module\n").expect("write");

    let before_readme = std::fs::read(tree.join("README")).expect("read");
    let before_src = std::fs::read(tree.join("src.txt")).expect("read");

    let reclaimed = reclaim_build_output(&tree).expect("reclaim");
    assert_eq!(reclaimed.removed.len(), 2, "{:?}", reclaimed.removed);
    assert!(!tree.join("target").exists());
    assert!(!tree.join("desktop/node_modules").exists());
    assert!(
        tree.join("desktop").exists(),
        "only node_modules goes, not the directory holding it"
    );
    assert!(reclaimed.freed_label.ends_with(" GB"));

    assert_eq!(
        std::fs::read(tree.join("README")).expect("read"),
        before_readme
    );
    assert_eq!(
        std::fs::read(tree.join("src.txt")).expect("read"),
        before_src
    );
    assert!(
        tree.join(".git").exists(),
        "the worktree's git link survives"
    );
}

#[test]
fn reclaim_on_a_held_tree_still_removes_build_output_and_keeps_the_edits() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-held-reclaim");
    std::fs::write(tree.join("uncommitted.txt"), b"work in progress\n").expect("write");
    std::fs::create_dir_all(tree.join("target")).expect("mkdir");
    std::fs::write(tree.join("target/blob"), vec![3_u8; 2048]).expect("write");

    reclaim_build_output(&tree).expect("reclaim");

    assert!(!tree.join("target").exists());
    assert_eq!(
        std::fs::read_to_string(tree.join("uncommitted.txt")).expect("read"),
        "work in progress\n",
        "uncommitted work is never deleted"
    );
    assert_eq!(count_dirty_files(&tree), 1);
}

#[test]
fn an_unmeasurable_size_reads_unknown_in_a_row() {
    assert_eq!(render_reclaimable_bytes(None), "unknown");
}

// ── the removal itself ──────────────────────────────────────────────────────

#[test]
fn removing_a_clean_tree_takes_the_directory_and_leaves_the_repository() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-remove");
    assert!(tree.is_dir());

    crate::coding_sessions::worktree::remove_worktree(&layout.repo, &tree).expect("remove");

    assert!(!tree.exists());
    assert!(layout.repo.join("README").is_file());
}

#[test]
fn git_itself_refuses_to_remove_a_dirty_tree_because_nothing_passes_force() {
    let layout = layout();
    let tree = add_worktree(&layout, "lane-refuse");
    std::fs::write(tree.join("scratch.txt"), b"unsaved\n").expect("write");

    let error = crate::coding_sessions::worktree::remove_worktree(&layout.repo, &tree)
        .expect_err("git must refuse a dirty tree");
    assert!(
        tree.join("scratch.txt").is_file(),
        "the file survives the refusal: {error}"
    );
}
