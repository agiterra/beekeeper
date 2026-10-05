//! Session terminals (SV-25): refused without a tree, never `$HOME`, never a
//! path in what the renderer gets, and the announce names the session.

use std::path::Path;

use tempfile::TempDir;

use super::{
    announce_session_tag, choose_resume_cwd, renderer_view, resolve_session_cwd_in_store,
    validate_reference, ShellCodingSessionRef, MAX_SESSION_TAG_CHARS,
};
use crate::coding_sessions::workdir_store::{CodingSessionSeatWorktree, CodingSessionWorkdirStore};
use crate::shell_sessions::manager::ShellSessionInfo;

const SESSION_REF: &str =
    "44226:abababababababababababababababababababababababababababababababab:sess";

fn reference(is_local_provider: bool) -> ShellCodingSessionRef {
    ShellCodingSessionRef {
        session_ref: SESSION_REF.to_string(),
        session_id: Some("sess-1".to_string()),
        channel_id: Some("8f4d0a8e-0000-4000-8000-000000000001".to_string()),
        project_ref: Some("30621:abc:proj".to_string()),
        is_local_provider,
        is_hired_seat: false,
    }
}

fn info(coding_session: Option<ShellCodingSessionRef>, cwd: &str) -> ShellSessionInfo {
    ShellSessionInfo {
        session_id: "shell-1".to_string(),
        title: "Terminal".to_string(),
        current_directory: cwd.to_string(),
        shell: "/bin/zsh".to_string(),
        created_at: 1,
        rows: 24,
        cols: 80,
        running: true,
        restorable: false,
        project_ref: Some(format!("30621:{}:proj", "ab".repeat(32))),
        shared: true,
        roster: Vec::new(),
        coding_session,
    }
}

#[test]
fn a_session_with_no_tree_here_is_refused_with_the_trees_reason() {
    let store = CodingSessionWorkdirStore::default();
    let local = resolve_session_cwd_in_store(&store, &reference(true))
        .expect_err("no recorded tree must refuse");
    assert_eq!(
        local,
        "No working tree for this session is recorded on this computer."
    );
    let remote = resolve_session_cwd_in_store(&store, &reference(false))
        .expect_err("another computer's session must refuse");
    assert_eq!(remote, "The working tree is on another computer.");
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(!home.is_empty() && !local.contains(&home) && !remote.contains(&home));
}

#[test]
fn a_recorded_session_worktree_is_where_the_shell_opens() {
    let temp = TempDir::new().expect("tempdir");
    let tree = temp.path().join("seat-tree");
    std::fs::create_dir_all(&tree).expect("mkdir");
    let tree = std::fs::canonicalize(&tree).expect("canonicalize");
    let mut store = CodingSessionWorkdirStore::default();
    store.worktrees.insert(
        "ref/seat".to_string(),
        CodingSessionSeatWorktree {
            path: tree.clone(),
            branch: "seat".to_string(),
            repo_root: tree.clone(),
            created_at: "2026-10-04T00:00:00Z".to_string(),
            session_id: Some("sess-1".to_string()),
            agents_clone: None,
            commit_identity: None,
            actor_pubkey: None,
            seeding: None,
        },
    );
    // Even when another provider runs it: this host cut the tree.
    let cwd = resolve_session_cwd_in_store(&store, &reference(false)).expect("resolves");
    assert_eq!(Path::new(&cwd), tree.as_path());
}

#[test]
fn a_reference_that_names_no_session_is_refused() {
    let mut empty = reference(true);
    empty.session_ref = "  ".to_string();
    assert!(validate_reference(&empty).is_err());
    let mut long = reference(true);
    long.session_ref = "x".repeat(MAX_SESSION_TAG_CHARS + 1);
    assert!(validate_reference(&long).is_err());
    let mut control = reference(true);
    control.session_ref = "a\nb".to_string();
    assert!(validate_reference(&control).is_err());
    assert!(resolve_session_cwd_in_store(&CodingSessionWorkdirStore::default(), &empty).is_err());
}

#[test]
fn the_path_never_appears_in_what_the_renderer_gets() {
    let secret = "/Users/someone/worktrees/secret-seat";
    let shown = renderer_view(info(Some(reference(true)), secret));
    assert_eq!(shown.current_directory, "");
    let json = serde_json::to_string(&shown).expect("encode");
    assert!(!json.contains(secret), "json: {json}");
    assert!(!json.contains("secret-seat"), "json: {json}");
    assert!(
        json.contains("\"codingSession\":{\"sessionRef\""),
        "json: {json}"
    );
    // A plain shell keeps its directory: only session shells hide it.
    let plain = renderer_view(info(None, "/tmp/plain"));
    assert_eq!(plain.current_directory, "/tmp/plain");
}

#[test]
fn the_announce_names_the_session_and_only_a_session_shell() {
    assert_eq!(
        announce_session_tag(&info(Some(reference(true)), "/x")),
        Some(SESSION_REF.to_string())
    );
    assert_eq!(announce_session_tag(&info(None, "/x")), None);
    let mut bad = reference(true);
    bad.session_ref = String::new();
    assert_eq!(announce_session_tag(&info(Some(bad), "/x")), None);
}

#[test]
fn a_resume_stays_inside_the_tree() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("tree");
    let inner = root.join("crates");
    let outside = temp.path().join("elsewhere");
    for dir in [&inner, &outside] {
        std::fs::create_dir_all(dir).expect("mkdir");
    }
    let inner = inner.to_string_lossy().into_owned();
    assert_eq!(choose_resume_cwd(&root, &inner), inner);
    let root_text = root.to_string_lossy().into_owned();
    assert_eq!(
        choose_resume_cwd(&root, &outside.to_string_lossy()),
        root_text
    );
    assert_eq!(choose_resume_cwd(&root, "/nonexistent/dir"), root_text);
}

#[test]
fn a_reference_round_trips_through_the_sidecar_shape() {
    let json = serde_json::to_string(&reference(true)).expect("encode");
    let back: ShellCodingSessionRef = serde_json::from_str(&json).expect("decode");
    assert_eq!(back, reference(true));
    // The renderer sends only what it knows; the rest defaults.
    let minimal: ShellCodingSessionRef =
        serde_json::from_str(r#"{"sessionRef":"s","channelId":"c"}"#).expect("decode");
    assert_eq!(minimal.session_id, None);
    assert!(!minimal.is_local_provider);
}

#[test]
fn the_announce_carries_one_session_tag_and_no_path() {
    let coordinate = format!("30621:{}:proj", "ab".repeat(32));
    let secret = "/Users/someone/worktrees/secret-seat";
    let tags = crate::shell_sessions::broadcast::announce_tags(
        &info(Some(reference(true)), secret),
        &coordinate,
        "open",
    );
    let rows: Vec<Vec<String>> = tags.iter().map(|tag| tag.as_slice().to_vec()).collect();
    let session: Vec<&Vec<String>> = rows.iter().filter(|row| row[0] == "session").collect();
    assert_eq!(session.len(), 1, "rows: {rows:?}");
    assert_eq!(
        session[0],
        &vec!["session".to_string(), SESSION_REF.to_string()]
    );
    // The envelope the relay checks is unchanged: one d, one a, one status.
    for name in ["d", "a", "status"] {
        assert_eq!(
            rows.iter().filter(|row| row[0] == name).count(),
            1,
            "{name}"
        );
    }
    assert!(rows
        .iter()
        .flatten()
        .all(|value| !value.contains("secret-seat")));
    // A plain shell's announce has no session tag.
    let plain =
        crate::shell_sessions::broadcast::announce_tags(&info(None, "/tmp"), &coordinate, "open");
    assert!(plain.iter().all(|tag| tag.as_slice()[0] != "session"));
}
