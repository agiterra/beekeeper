//! The session tree's lookup order and its refusals, against real directories
//! in a `TempDir`. Every directory here is created by the test that uses it.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::{
    list_tree_entries, resolve_relative_dir, resolve_tree_root_from_loaded,
    resolve_tree_root_in_store, tree_resolution, CodingSessionTreeEntryKind,
    CodingSessionTreeQuery, CodingSessionTreeRefusal, CodingSessionTreeSource, MAX_TREE_ENTRIES,
};
use crate::coding_sessions::workdir_store::{
    load_workdir_store_readonly_from, CodingSessionSeatWorktree, CodingSessionWorkdirEntry,
    CodingSessionWorkdirStore,
};

const SESSION_ID: &str = "sess-1";
const CHANNEL_ID: &str = "8f4d0a8e-0000-4000-8000-000000000001";
const PROJECT_REF: &str = "30621:abc:proj";

fn mkdir(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).expect("create dir");
    std::fs::canonicalize(&path).expect("canonicalize")
}

fn entry(path: &Path) -> CodingSessionWorkdirEntry {
    CodingSessionWorkdirEntry {
        path: path.to_path_buf(),
        updated_at: "2026-10-04T00:00:00Z".to_string(),
    }
}

fn worktree(path: &Path, session_id: &str) -> CodingSessionSeatWorktree {
    CodingSessionSeatWorktree {
        path: path.to_path_buf(),
        branch: "seat".to_string(),
        repo_root: path.to_path_buf(),
        created_at: "2026-10-04T00:00:00Z".to_string(),
        session_id: Some(session_id.to_string()),
        agents_clone: None,
        commit_identity: None,
        actor_pubkey: None,
        seeding: None,
    }
}

struct Fixture {
    _temp: TempDir,
    session: PathBuf,
    project: PathBuf,
    channel: PathBuf,
    store: CodingSessionWorkdirStore,
}

fn fixture() -> Fixture {
    let temp = TempDir::new().expect("tempdir");
    let session = mkdir(temp.path(), "session-tree");
    let project = mkdir(temp.path(), "project-checkout");
    let channel = mkdir(temp.path(), "channel-folder");
    let mut store = CodingSessionWorkdirStore::default();
    store
        .worktrees
        .insert("ref/seat".to_string(), worktree(&session, SESSION_ID));
    store
        .by_project
        .insert(PROJECT_REF.to_string(), entry(&project));
    store
        .by_channel
        .insert(CHANNEL_ID.to_string(), entry(&channel));
    Fixture {
        _temp: temp,
        session,
        project,
        channel,
        store,
    }
}

fn query(is_local_provider: bool) -> CodingSessionTreeQuery {
    CodingSessionTreeQuery {
        session_id: Some(SESSION_ID.to_string()),
        channel_id: Some(CHANNEL_ID.to_string()),
        project_ref: Some(PROJECT_REF.to_string()),
        is_local_provider,
        is_hired_seat: false,
    }
}

#[test]
fn the_session_worktree_wins_over_every_default() {
    let fx = fixture();
    let (root, source) = resolve_tree_root_in_store(&fx.store, &query(true)).expect("resolves");
    assert_eq!(root, fx.session);
    assert_eq!(source, CodingSessionTreeSource::Session);
}

#[test]
fn a_session_worktree_on_this_host_resolves_even_when_another_provider_runs_it() {
    let fx = fixture();
    let (root, source) = resolve_tree_root_in_store(&fx.store, &query(false)).expect("resolves");
    assert_eq!(root, fx.session);
    assert_eq!(source, CodingSessionTreeSource::Session);
}

/// Pins the provider's own rule (`ProjectsFile::resolve_default`): project
/// checkout first, channel folder second — the order DB9 states since its
/// 2026-10-04 amendment, because that is where the agent actually works.
#[test]
fn defaults_follow_the_providers_resolve_default_rule_project_before_channel() {
    let mut fx = fixture();
    fx.store.worktrees.clear();
    // Both defaults exist: the project's checkout must answer.
    assert!(fx.store.by_project.contains_key(PROJECT_REF));
    assert!(fx.store.by_channel.contains_key(CHANNEL_ID));
    let (root, source) = resolve_tree_root_in_store(&fx.store, &query(true)).expect("resolves");
    assert_eq!(source, CodingSessionTreeSource::Project);
    assert_eq!(root, fx.project);
    assert_ne!(root, fx.channel);
}

#[test]
fn a_hired_seat_never_falls_back_to_the_project_or_channel_checkout() {
    let mut fx = fixture();
    let hired = CodingSessionTreeQuery {
        is_hired_seat: true,
        ..query(true)
    };
    // Its own worktree still answers.
    let (root, source) = resolve_tree_root_in_store(&fx.store, &hired).expect("resolves");
    assert_eq!(root, fx.session);
    assert_eq!(source, CodingSessionTreeSource::Session);
    // Without it, the defaults are refused, not offered as its tree.
    fx.store.worktrees.clear();
    let error = resolve_tree_root_in_store(&fx.store, &hired).expect_err("refused");
    assert_eq!(error.refusal, CodingSessionTreeRefusal::NotRecorded);
    assert!(error.message.contains("hired seat"));
}

#[test]
fn without_a_session_tree_the_project_checkout_comes_before_the_channel_folder() {
    let mut fx = fixture();
    fx.store.worktrees.clear();
    let (root, source) = resolve_tree_root_in_store(&fx.store, &query(true)).expect("resolves");
    assert_eq!(root, fx.project);
    assert_eq!(source, CodingSessionTreeSource::Project);

    fx.store.by_project.clear();
    let (root, source) = resolve_tree_root_in_store(&fx.store, &query(true)).expect("resolves");
    assert_eq!(root, fx.channel);
    assert_eq!(source, CodingSessionTreeSource::Channel);
}

#[test]
fn a_missing_session_directory_falls_through_to_the_defaults() {
    let mut fx = fixture();
    std::fs::remove_dir_all(&fx.session).expect("remove session tree");
    let (root, source) = resolve_tree_root_in_store(&fx.store, &query(true)).expect("resolves");
    assert_eq!(root, fx.project);
    assert_eq!(source, CodingSessionTreeSource::Project);
    fx.store.by_project.clear();
    fx.store.by_channel.clear();
    let error = resolve_tree_root_in_store(&fx.store, &query(true)).expect_err("refuses");
    assert_eq!(error.refusal, CodingSessionTreeRefusal::NotRecorded);
}

#[test]
fn a_teammates_own_checkout_is_never_this_sessions_tree() {
    let mut fx = fixture();
    fx.store.worktrees.clear();
    let error = resolve_tree_root_in_store(&fx.store, &query(false)).expect_err("refuses");
    assert_eq!(error.refusal, CodingSessionTreeRefusal::NotLocal);
    assert_eq!(error.message, "The working tree is on another computer.");
}

#[test]
fn the_renderer_resolution_never_carries_the_path() {
    let fx = fixture();
    let resolution = tree_resolution(resolve_tree_root_in_store(&fx.store, &query(true)));
    assert!(resolution.available);
    assert_eq!(resolution.label, "this session's worktree");
    let json = serde_json::to_string(&resolution).expect("serialize");
    let root = fx.session.to_string_lossy().to_string();
    assert!(!json.contains(&root), "{json} leaked {root}");
    assert!(
        !json.contains("session-tree"),
        "{json} leaked the folder name"
    );

    let refused = tree_resolution(resolve_tree_root_in_store(
        &CodingSessionWorkdirStore::default(),
        &query(true),
    ));
    assert!(!refused.available);
    assert_eq!(
        refused.reason.as_deref(),
        Some("No working tree for this session is recorded on this computer.")
    );
}

#[test]
fn a_listing_refuses_parent_absolute_and_git_paths() {
    let fx = fixture();
    mkdir(&fx.session, "src");
    assert!(resolve_relative_dir(&fx.session, "..").is_err());
    assert!(resolve_relative_dir(&fx.session, "src/../..").is_err());
    assert!(resolve_relative_dir(&fx.session, "src/..").is_err());
    let absolute = fx.project.to_string_lossy().to_string();
    assert!(resolve_relative_dir(&fx.session, &absolute).is_err());
    assert!(resolve_relative_dir(&fx.session, "/etc").is_err());
    mkdir(&fx.session, ".git");
    assert!(resolve_relative_dir(&fx.session, ".git").is_err());
    assert!(resolve_relative_dir(&fx.session, "").is_ok());
    assert!(resolve_relative_dir(&fx.session, "src").is_ok());
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_tree_is_refused_and_listed_but_not_followed() {
    let fx = fixture();
    std::os::unix::fs::symlink(&fx.project, fx.session.join("escape")).expect("symlink");
    let error = resolve_relative_dir(&fx.session, "escape").expect_err("refuses");
    assert_eq!(error.message, "That path leads outside the session's tree.");
    let listing = list_tree_entries(&fx.session, "").expect("lists");
    let escape = listing
        .entries
        .iter()
        .find(|entry| entry.name == "escape")
        .expect("symlink listed");
    assert_eq!(escape.kind, CodingSessionTreeEntryKind::Symlink);
}

#[cfg(unix)]
#[test]
fn a_symlink_into_git_is_refused_after_canonicalizing() {
    let fx = fixture();
    let git = mkdir(&fx.session, ".git");
    mkdir(&git, "refs");
    let sub = mkdir(&fx.session, "sub");
    mkdir(&sub, ".git");
    std::os::unix::fs::symlink(".git", fx.session.join("gitdir")).expect("symlink");
    std::os::unix::fs::symlink("sub/.git", fx.session.join("x")).expect("symlink");
    std::os::unix::fs::symlink("../.git/refs", sub.join("refs-link")).expect("symlink");
    for rel in ["gitdir", "gitdir/refs", "x", "sub/refs-link"] {
        let error = resolve_relative_dir(&fx.session, rel).expect_err(rel);
        assert_eq!(
            error.message, "The repository's .git directory is not listed.",
            "{rel}"
        );
        assert!(list_tree_entries(&fx.session, rel).is_err(), "{rel}");
    }
    // The links themselves are still listed, never followed.
    let listing = list_tree_entries(&fx.session, "").expect("lists");
    assert!(listing
        .entries
        .iter()
        .any(|entry| entry.name == "gitdir" && entry.kind == CodingSessionTreeEntryKind::Symlink));
}

#[cfg(target_os = "linux")]
#[test]
fn a_name_that_is_not_utf8_is_counted_not_silently_dropped() {
    use std::os::unix::ffi::OsStrExt;
    let fx = fixture();
    let dir = mkdir(&fx.session, "odd");
    std::fs::write(dir.join("plain.txt"), "").expect("write");
    let name = std::ffi::OsStr::from_bytes(b"bad\xffname");
    std::fs::write(dir.join(name), "").expect("write");
    let listing = list_tree_entries(&fx.session, "odd").expect("lists");
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.omitted, 1);
}

#[test]
fn entries_are_relative_with_git_hidden_and_folders_first() {
    let fx = fixture();
    mkdir(&fx.session, ".git");
    let src = mkdir(&fx.session, "src");
    std::fs::write(fx.session.join("README.md"), "hi").expect("write");
    std::fs::write(src.join("main.rs"), "fn main() {}").expect("write");
    let root = list_tree_entries(&fx.session, "").expect("lists");
    let names: Vec<&str> = root
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["src", "README.md"]);
    assert!(!root.truncated);
    // `.git` is hidden by contract, not an omission.
    assert_eq!(root.omitted, 0);
    let nested = list_tree_entries(&fx.session, "src").expect("lists");
    assert_eq!(nested.entries.len(), 1);
    assert_eq!(nested.entries[0].rel_path, "src/main.rs");
    let root_text = fx.session.to_string_lossy().to_string();
    for entry in root.entries.iter().chain(nested.entries.iter()) {
        assert!(!entry.rel_path.starts_with('/'));
        assert!(!entry.rel_path.contains(&root_text));
    }
}

#[test]
fn a_listing_stops_at_the_cap_and_says_so() {
    let fx = fixture();
    let big = mkdir(&fx.session, "big");
    for index in 0..(MAX_TREE_ENTRIES + 5) {
        std::fs::write(big.join(format!("f{index:05}")), "").expect("write");
    }
    let listing = list_tree_entries(&fx.session, "big").expect("lists");
    assert_eq!(listing.entries.len(), MAX_TREE_ENTRIES);
    assert!(listing.truncated);
}

/// A corrupt or old-shaped store whose parse error quotes a path must not
/// carry that path into the renderer's reason.
#[test]
fn an_unreadable_store_never_puts_its_parse_error_in_the_reason() {
    let temp = TempDir::new().expect("tempdir");
    let store_path = temp.path().join("coding-session-workdirs.json");
    std::fs::write(
        &store_path,
        r#"{"byProject": "/Users/someone/secret-checkout"}"#,
    )
    .expect("write store");
    let loaded = load_workdir_store_readonly_from(&store_path);
    let raw = loaded
        .clone()
        .expect_err("a string where a map belongs fails to parse");
    assert!(
        raw.contains('/'),
        "the fixture must exercise a path-quoting error: {raw}"
    );

    let resolution = tree_resolution(resolve_tree_root_from_loaded(loaded, &query(true)));
    assert!(!resolution.available);
    assert_eq!(
        resolution.refusal,
        Some(CodingSessionTreeRefusal::StoreUnreadable)
    );
    let reason = resolution
        .reason
        .expect("an unavailable tree explains itself");
    assert!(!reason.contains('/'), "reason leaked a path: {reason}");
}
