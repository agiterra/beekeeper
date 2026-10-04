//! Every test builds its own throwaway repository under a `TempDir` and diffs
//! trees it wrote there. Nothing here reads a path from the environment or
//! touches the checkout the suite runs from.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::coding_sessions::workdir_store::{
    CodingSessionSeatWorktree, CodingSessionWorkdirEntry, CodingSessionWorkdirStore,
};
use crate::commands::project_git_exec::GIT_REPO_SELECTION_VARS;

fn git(args: &[&str], cwd: &Path) -> String {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "S2")
        .env("GIT_AUTHOR_EMAIL", "s2@example.invalid")
        .env("GIT_COMMITTER_NAME", "S2")
        .env("GIT_COMMITTER_EMAIL", "s2@example.invalid");
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    let output = command.output().expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

fn repo() -> Repo {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical").join("proj");
    std::fs::create_dir_all(&root).expect("mkdir");
    git(&["init", "-q", "--initial-branch=main", "."], &root);
    std::fs::write(root.join("README"), "hello\n").expect("write");
    git(&["add", "README"], &root);
    git(&["commit", "-q", "-m", "one"], &root);
    Repo { _dir: dir, root }
}

/// The tree of the working directory as it stands, written as objects.
fn snapshot(root: &Path) -> String {
    git(&["add", "-A"], root);
    git(&["write-tree"], root)
}

fn checkout_at(root: &Path) -> ResolvedCheckout {
    ResolvedCheckout {
        tree: root.to_path_buf(),
        repo_root: None,
        source: CheckpointDiffCheckout::ProjectCheckout,
    }
}

fn expect_local(
    answer: CodingSessionCheckpointDiff,
) -> (CheckpointDiffCheckout, ProjectRepoDiffInfo) {
    match answer {
        CodingSessionCheckpointDiff::Local {
            checkout,
            diff,
            files_not_listed,
        } => {
            assert_eq!(files_not_listed, 0, "every name here is UTF-8");
            (checkout, diff)
        }
        other => panic!(
            "expected a local diff, got {}",
            serde_json::to_string(&other).expect("json")
        ),
    }
}

#[test]
fn a_two_tree_patch_is_read_from_the_checkout() {
    let repo = repo();
    std::fs::write(repo.root.join("notes.txt"), "one\ntwo\n").expect("write");
    let base = snapshot(&repo.root);
    std::fs::write(repo.root.join("notes.txt"), "one\nTWO\nthree\n").expect("write");
    std::fs::write(repo.root.join("added.txt"), "new\n").expect("write");
    std::fs::remove_file(repo.root.join("README")).expect("rm");
    let tree = snapshot(&repo.root);

    let (source, diff) = expect_local(
        checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&base), &tree).expect("diff"),
    );
    assert_eq!(source, CheckpointDiffCheckout::ProjectCheckout);
    let paths: Vec<&str> = diff.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["README", "added.txt", "notes.txt"]);
    let notes = &diff.files[2];
    assert_eq!((notes.additions, notes.deletions), (2, 1));
    assert!(notes.patch.contains("-two") && notes.patch.contains("+TWO"));
    assert!(!notes.truncated);
    assert_eq!((diff.additions, diff.deletions), (3, 2));
    assert!(diff.commit_body.is_none());

    let wire = serde_json::to_value(
        checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&base), &tree).expect("diff"),
    )
    .expect("json");
    assert_eq!(wire["state"], "local");
    assert_eq!(wire["checkout"], "project_checkout");
    assert_eq!(wire["diff"]["files"].as_array().map(Vec::len), Some(3));
}

#[test]
fn a_long_patch_is_capped_at_two_thousand_lines_and_says_so() {
    let repo = repo();
    let base = snapshot(&repo.root);
    let body: String = (0..3_000).map(|n| format!("line {n}\n")).collect();
    std::fs::write(repo.root.join("big.txt"), body).expect("write");
    let tree = snapshot(&repo.root);

    let (_, diff) = expect_local(
        checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&base), &tree).expect("diff"),
    );
    let big = &diff.files[0];
    assert_eq!(big.additions, 3_000, "the count is the real one");
    assert!(big.truncated);
    assert_eq!(big.patch.lines().count(), 2_000);
}

#[test]
fn trees_this_checkout_does_not_hold_are_objects_missing_never_an_empty_diff() {
    let repo = repo();
    let here = snapshot(&repo.root);
    let elsewhere = "0123456789abcdef0123456789abcdef01234567";
    let elsewhere_too = "89abcdef0123456789abcdef0123456789abcdef";

    let answer =
        checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&here), elsewhere).expect("answer");
    let wire = serde_json::to_value(&answer).expect("json");
    assert_eq!(wire["state"], "objects_missing");
    assert_eq!(wire["missing"], serde_json::json!([elsewhere]));

    let both = checkpoint_diff_in(
        Some(&checkout_at(&repo.root)),
        Some(elsewhere),
        elsewhere_too,
    )
    .expect("answer");
    match both {
        CodingSessionCheckpointDiff::ObjectsMissing { missing } => {
            assert_eq!(missing, [elsewhere, elsewhere_too]);
        }
        _ => panic!("both endpoints absent must name both"),
    }
}

#[test]
fn an_id_naming_a_non_tree_object_is_an_error_not_a_missing_object() {
    let repo = repo();
    let tree = snapshot(&repo.root);
    let blob = git(&["rev-parse", "HEAD:README"], &repo.root);
    let error = checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&blob), &tree)
        .err()
        .expect("a blob is not a checkpoint tree");
    assert!(error.contains("not a tree"), "{error}");
}

#[test]
fn a_malformed_id_is_refused_before_git_runs() {
    let repo = repo();
    let tree = snapshot(&repo.root);
    for bad in [
        "",
        "abc123",
        "--output=/tmp/owned",
        "HEAD",
        "0123456789abcdef0123456789abcdef0123456g",
        "0123456789abcdef0123456789abcdef01234567^{tree}",
    ] {
        assert!(
            checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(bad), &tree).is_err(),
            "fromTree {bad:?} must be refused"
        );
        assert!(
            checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&tree), bad).is_err(),
            "toTree {bad:?} must be refused"
        );
        // Refused even when the answer would otherwise need no checkout.
        assert!(checkpoint_diff_in(None, None, bad).is_err());
    }
    assert_eq!(
        clean_tree_oid(&"AB".repeat(32)).expect("sha-256 length"),
        "ab".repeat(32)
    );
}

#[test]
fn no_recorded_checkout_and_no_baseline_are_their_own_states() {
    let tree = "0123456789abcdef0123456789abcdef01234567";
    let none = serde_json::to_value(checkpoint_diff_in(None, Some(tree), tree).expect("answer"))
        .expect("json");
    assert_eq!(none, serde_json::json!({ "state": "no_checkout" }));
    let baseline =
        serde_json::to_value(checkpoint_diff_in(None, None, tree).expect("answer")).expect("json");
    assert_eq!(baseline, serde_json::json!({ "state": "baseline_missing" }));
}

fn seat(repo_root: &Path, path: &Path, session_id: Option<&str>) -> CodingSessionSeatWorktree {
    CodingSessionSeatWorktree {
        path: path.to_path_buf(),
        branch: "seat".to_string(),
        repo_root: repo_root.to_path_buf(),
        created_at: "2026-10-04T00:00:00Z".to_string(),
        session_id: session_id.map(str::to_string),
        agents_clone: None,
        commit_identity: None,
        actor_pubkey: None,
        seeding: None,
    }
}

#[test]
fn the_seat_worktree_whose_session_id_is_the_target_is_the_checkout() {
    let repo = repo();
    let tree_dir = repo
        .root
        .parent()
        .expect("parent")
        .join("proj.worktrees/builder");
    git(
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "builder",
            tree_dir.to_str().expect("utf8"),
            "main",
        ],
        &repo.root,
    );
    let other_dir = repo
        .root
        .parent()
        .expect("parent")
        .join("proj.worktrees/other");
    std::fs::create_dir_all(&other_dir).expect("mkdir");
    let mut store = CodingSessionWorkdirStore::default();
    store.worktrees.insert(
        "umbrella/Builder".to_string(),
        seat(&repo.root, &tree_dir, Some("exec-1")),
    );
    store.worktrees.insert(
        "umbrella/Other".to_string(),
        seat(&repo.root, &other_dir, Some("exec-2")),
    );
    store.worktrees.insert(
        "another/Builder".to_string(),
        seat(&repo.root, &other_dir, Some("exec-1")),
    );
    store.by_project.insert(
        "project-a".to_string(),
        CodingSessionWorkdirEntry {
            path: repo.root.clone(),
            updated_at: "2026-10-04T00:00:00Z".to_string(),
        },
    );

    let resolved =
        resolve_checkout(&store, "umbrella", "exec-1", Some("project-a")).expect("the seat's tree");
    assert_eq!(resolved.tree, tree_dir);
    assert_eq!(resolved.repo_root.as_deref(), Some(repo.root.as_path()));
    assert_eq!(resolved.source, CheckpointDiffCheckout::SeatWorktree);

    // No seat answers: the project's own checkout stands in, and only with a
    // project named.
    let fallback =
        resolve_checkout(&store, "umbrella", "exec-9", Some("project-a")).expect("project");
    assert_eq!(fallback.tree, repo.root);
    assert_eq!(fallback.source, CheckpointDiffCheckout::ProjectCheckout);
    assert!(resolve_checkout(&store, "umbrella", "exec-9", None).is_none());
    assert!(resolve_checkout(&store, "umbrella", "exec-9", Some("project-b")).is_none());

    // A record whose directory is gone is no checkout.
    std::fs::remove_dir_all(&other_dir).expect("rm");
    assert!(resolve_checkout(&store, "umbrella", "exec-2", None).is_none());

    // The checkpoint objects the provider writes live in the shared store, so
    // the linked worktree reads them inside its boundary.
    std::fs::write(tree_dir.join("README"), "changed in the seat\n").expect("write");
    let base = git(&["rev-parse", "HEAD^{tree}"], &tree_dir);
    let tree = snapshot(&tree_dir);
    let (source, diff) =
        expect_local(checkpoint_diff_in(Some(&resolved), Some(&base), &tree).expect("diff"));
    assert_eq!(source, CheckpointDiffCheckout::SeatWorktree);
    assert_eq!(diff.files.len(), 1);
    assert!(diff.files[0].patch.contains("+changed in the seat"));
}

/// Runs git with raw `stdin`, answering stdout trimmed.
fn git_stdin(args: &[&str], cwd: &Path, stdin: &[u8]) -> String {
    use std::io::Write;
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    let mut child = command.spawn().expect("spawn git");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait git");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A tree written straight into the object store, so names the filesystem
/// here would refuse (macOS will not create a non-UTF-8 name) can be tested.
fn mktree(root: &Path, entries: &[(&[u8], &str)]) -> String {
    let mut listing = Vec::new();
    for (name, body) in entries {
        let blob = git_stdin(&["hash-object", "-w", "--stdin"], root, body.as_bytes());
        listing.extend_from_slice(format!("100644 blob {blob}\t").as_bytes());
        listing.extend_from_slice(name);
        listing.push(b'\n');
    }
    git_stdin(&["mktree"], root, &listing)
}

fn local_with_count(answer: CodingSessionCheckpointDiff) -> (ProjectRepoDiffInfo, u64) {
    match answer {
        CodingSessionCheckpointDiff::Local {
            diff,
            files_not_listed,
            ..
        } => (diff, files_not_listed),
        _ => panic!("expected a local diff"),
    }
}

#[test]
fn a_non_utf8_name_is_counted_never_named_and_two_never_collapse() {
    let repo = repo();
    let empty = mktree(&repo.root, &[]);
    let tree = mktree(
        &repo.root,
        &[
            (b"bad\xfe.txt", "one\n"),
            (b"bad\xff.txt", "two\n"),
            (b"ok.txt", "three\n"),
        ],
    );
    let answer =
        checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&empty), &tree).expect("diff");
    let wire = serde_json::to_value(&answer).expect("json");
    assert_eq!(wire["filesNotListed"], 2);
    let (diff, not_listed) = local_with_count(answer);
    assert_eq!(not_listed, 2);
    let paths: Vec<&str> = diff.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["ok.txt"], "no U+FFFD name, no collapsed pair");
    assert!(diff.files[0].patch.contains("+three"));
    assert_eq!(
        diff.additions, 3,
        "the totals still count the unnamed files"
    );
}

#[test]
fn a_non_ascii_utf8_name_is_named_as_it_is_with_its_patch() {
    let repo = repo();
    let base = snapshot(&repo.root);
    std::fs::write(repo.root.join("café.txt"), "crème\n").expect("write");
    let tree = snapshot(&repo.root);
    let (diff, not_listed) = local_with_count(
        checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&base), &tree).expect("diff"),
    );
    assert_eq!(not_listed, 0);
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "café.txt", "not C-quoted");
    assert!(
        diff.files[0].patch.contains("+crème"),
        "never an empty patch"
    );
}

#[test]
fn a_rename_is_one_row_and_a_glob_name_matches_only_itself() {
    let repo = repo();
    let body: String = (0..20).map(|n| format!("line {n}\n")).collect();
    let base = mktree(
        &repo.root,
        &[
            (b"old.txt", body.as_str()),
            (b"ab.txt", "x\n"),
            (b"a*.txt", "y\n"),
        ],
    );
    let tree = mktree(
        &repo.root,
        &[
            (b"new.txt", body.as_str()),
            (b"ab.txt", "x2\n"),
            (b"a*.txt", "y2\n"),
        ],
    );
    let (diff, not_listed) = local_with_count(
        checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&base), &tree).expect("diff"),
    );
    assert_eq!(not_listed, 0);
    let paths: Vec<&str> = diff.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["a*.txt", "ab.txt", "new.txt"]);
    let glob = &diff.files[0];
    assert!(glob.patch.contains("+y2") && !glob.patch.contains("+x2"));
    let renamed = &diff.files[2];
    assert!(
        renamed.patch.contains("rename from old.txt"),
        "{}",
        renamed.patch
    );
    assert_eq!((renamed.additions, renamed.deletions), (0, 0));
}

#[test]
fn files_past_the_list_cap_are_counted_not_dropped() {
    let repo = repo();
    let empty = mktree(&repo.root, &[]);
    let names: Vec<String> = (0..=files::MAX_LISTED_FILES)
        .map(|n| format!("f{n:04}.txt"))
        .collect();
    let entries: Vec<(&[u8], &str)> = names.iter().map(|name| (name.as_bytes(), "x\n")).collect();
    let tree = mktree(&repo.root, &entries);
    let (diff, not_listed) = local_with_count(
        checkpoint_diff_in(Some(&checkout_at(&repo.root)), Some(&empty), &tree).expect("diff"),
    );
    assert_eq!(diff.files.len(), files::MAX_LISTED_FILES);
    assert_eq!(not_listed, 1);
    assert_eq!(diff.additions, files::MAX_LISTED_FILES + 1);
}

#[test]
fn numstat_records_keep_raw_bytes_and_read_a_binary_dash_as_zero() {
    let raw = b"1\t2\tplain.txt\0-\t-\tbin\xff\x003\t0\t\0from.txt\0to.txt\0";
    let records = files::parse_numstat_z(raw);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].path, b"plain.txt");
    assert_eq!((records[0].additions, records[0].deletions), (1, 2));
    assert_eq!(records[1].path, b"bin\xff");
    assert_eq!((records[1].additions, records[1].deletions), (0, 0));
    assert_eq!(records[2].path, b"to.txt");
    assert_eq!(records[2].from.as_deref(), Some(&b"from.txt"[..]));
}
