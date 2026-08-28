//! Working-directory store behavior, and the containment law it exists for.

use std::path::PathBuf;

use super::workdir_store::{
    validate_workdir, CodingSessionWorkdirScope, CodingSessionWorkdirStore, MAX_MRU_ENTRIES,
    MAX_PENDING_HINTS, PROJECTS_VIEW_VERSION, WORKDIR_STORE_VERSION,
};

const PROJECT_REF: &str =
    "30621:aa00000000000000000000000000000000000000000000000000000000000000:buzz";
const CHANNEL_ID: &str = "11111111-2222-3333-4444-555555555555";

fn store_with_choices() -> CodingSessionWorkdirStore {
    let mut store = CodingSessionWorkdirStore::default();
    store.set(
        CodingSessionWorkdirScope::Project,
        PROJECT_REF,
        PathBuf::from("/src/buzz"),
    );
    store.set(
        CodingSessionWorkdirScope::Channel,
        CHANNEL_ID,
        PathBuf::from("/src/side-quest"),
    );
    store.record_use(PathBuf::from("/src/buzz"));
    store.stage_hint("create-1", PathBuf::from("/src/one-shot"));
    store
}

#[test]
fn a_store_round_trips_through_json_unchanged() {
    let store = store_with_choices();
    let encoded = serde_json::to_string(&store).expect("serialize");
    let decoded: CodingSessionWorkdirStore = serde_json::from_str(&encoded).expect("deserialize");
    assert_eq!(decoded, store);
    assert_eq!(decoded.version, WORKDIR_STORE_VERSION);
}

#[test]
fn an_absent_file_decodes_as_the_empty_steady_state() {
    let decoded: CodingSessionWorkdirStore =
        serde_json::from_str(r#"{"version":1}"#).expect("deserialize");
    assert_eq!(decoded, CodingSessionWorkdirStore::default());
}

#[test]
fn clearing_a_scope_forgets_only_that_key() {
    let mut store = store_with_choices();
    store.clear(CodingSessionWorkdirScope::Project, PROJECT_REF);

    assert!(store.by_project.is_empty());
    assert_eq!(
        store.by_channel[CHANNEL_ID].path,
        PathBuf::from("/src/side-quest"),
        "clearing a project key must not touch channel choices"
    );
    // The provider view drops the entry with it.
    assert!(store.projects_view().projects.is_empty());
    // Clearing an unknown key is a no-op, not an error.
    store.clear(CodingSessionWorkdirScope::Project, "30621:absent:none");
}

#[test]
fn setting_a_scope_replaces_rather_than_accumulates() {
    let mut store = CodingSessionWorkdirStore::default();
    store.set(
        CodingSessionWorkdirScope::Channel,
        CHANNEL_ID,
        PathBuf::from("/src/first"),
    );
    store.set(
        CodingSessionWorkdirScope::Channel,
        CHANNEL_ID,
        PathBuf::from("/src/second"),
    );

    assert_eq!(store.by_channel.len(), 1);
    assert_eq!(
        store.by_channel[CHANNEL_ID].path,
        PathBuf::from("/src/second")
    );
    assert!(
        store.by_project.is_empty(),
        "scopes must not bleed together"
    );
}

#[test]
fn reusing_a_directory_promotes_it_instead_of_duplicating_it() {
    let mut store = CodingSessionWorkdirStore::default();
    store.record_use(PathBuf::from("/src/a"));
    store.record_use(PathBuf::from("/src/b"));
    store.record_use(PathBuf::from("/src/a"));

    assert_eq!(
        store
            .mru
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        vec![PathBuf::from("/src/a"), PathBuf::from("/src/b")],
    );
}

#[test]
fn the_mru_evicts_the_least_recently_used_entry() {
    let mut store = CodingSessionWorkdirStore::default();
    for index in 0..MAX_MRU_ENTRIES + 3 {
        store.record_use(PathBuf::from(format!("/src/{index}")));
    }

    assert_eq!(store.mru.len(), MAX_MRU_ENTRIES);
    assert_eq!(
        store.mru[0].path,
        PathBuf::from(format!("/src/{}", MAX_MRU_ENTRIES + 2)),
    );
    assert!(
        !store
            .mru
            .iter()
            .any(|entry| entry.path == std::path::Path::new("/src/0")),
        "the oldest directory must have aged out",
    );
}

#[test]
fn a_hint_lives_from_staging_until_its_receipt_clears_it() {
    let mut store = CodingSessionWorkdirStore::default();
    store.stage_hint("create-1", PathBuf::from("/src/one-shot"));
    assert_eq!(
        store.pending.get("create-1"),
        Some(&PathBuf::from("/src/one-shot")),
    );

    store.stage_hint("create-1", PathBuf::from("/src/corrected"));
    assert_eq!(store.pending.len(), 1, "a retry replaces its own hint");
    assert_eq!(
        store.pending.get("create-1"),
        Some(&PathBuf::from("/src/corrected")),
    );

    store.clear_hint("create-1");
    assert!(store.pending.is_empty());
    store.clear_hint("create-1");
    assert!(store.pending.is_empty(), "clearing twice is not an error");
}

#[test]
fn pending_hints_are_bounded_when_receipts_never_arrive() {
    let mut store = CodingSessionWorkdirStore::default();
    for index in 0..MAX_PENDING_HINTS + 5 {
        store.stage_hint(&format!("create-{index:04}"), PathBuf::from("/src/buzz"));
    }
    assert_eq!(store.pending.len(), MAX_PENDING_HINTS);
}

#[test]
fn the_provider_view_carries_paths_and_nothing_else() {
    let view = store_with_choices().projects_view();

    assert_eq!(view.version, PROJECTS_VIEW_VERSION);
    assert_eq!(
        view.projects.get(PROJECT_REF),
        Some(&PathBuf::from("/src/buzz")),
    );
    assert_eq!(
        view.channels.get(CHANNEL_ID),
        Some(&PathBuf::from("/src/side-quest")),
    );
    assert_eq!(
        view.pending.get("create-1"),
        Some(&PathBuf::from("/src/one-shot")),
    );

    // The desktop's memory of *when* a choice was made is UI state. The
    // provider resolves a cwd; it has no business knowing the rest.
    let encoded = serde_json::to_string(&view).expect("serialize");
    assert!(!encoded.contains("updatedAt"), "{encoded}");
    assert!(!encoded.contains("lastUsedAt"), "{encoded}");
    assert!(!encoded.contains("mru"), "{encoded}");
}

#[test]
fn the_provider_view_matches_the_key_names_the_provider_parses() {
    let encoded = serde_json::to_string(&store_with_choices().projects_view()).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_str(&encoded).expect("json");
    let object = parsed.as_object().expect("object");

    // Exactly the four fields of `buzz_session_provider::commands::ProjectsFile`.
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys, vec!["channels", "pending", "projects", "version"]);
}

#[test]
fn validation_reports_absolute_existing_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("not-a-dir");
    std::fs::write(&file, b"x").expect("write");

    let on_dir = validate_workdir(dir.path());
    assert!(on_dir.exists && on_dir.is_dir && on_dir.is_absolute);

    let on_file = validate_workdir(&file);
    assert!(on_file.exists && !on_file.is_dir);

    let missing = validate_workdir(&dir.path().join("nope"));
    assert!(!missing.exists && !missing.is_dir);

    let relative = validate_workdir(std::path::Path::new("src/buzz"));
    assert!(
        !relative.is_absolute,
        "the provider treats a relative path as unconfigured, so the picker must say so",
    );
}

/// The law this whole module exists to keep.
///
/// A working directory names a person's disk. If one ever reached a relay-event
/// builder it would be published to every member of the channel, forever, for a
/// value only this machine can use. The check is textual on purpose: it fails
/// on the *shape* of the mistake — a store or view type reaching event-building
/// code — rather than on one known call path, so a new builder cannot quietly
/// opt out of it.
#[test]
fn no_workdir_type_is_reachable_from_relay_event_construction() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "CodingSessionWorkdirStore",
        "CodingSessionProjectsView",
        "load_workdir_store",
        "coding-session-workdirs.json",
    ];

    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            // The store's own module and its tests are where these names are
            // supposed to appear.
            if path.starts_with(root.join("coding_sessions")) {
                continue;
            }
            let body = std::fs::read_to_string(&path).expect("read rs");
            let builds_events = body.contains("EventBuilder")
                || body.contains("nostr::Event")
                || body.contains("sign_event")
                || body.contains("publish_event");
            if !builds_events {
                continue;
            }
            scanned += 1;
            for needle in forbidden {
                if body.contains(needle) {
                    offenders.push(format!("{} mentions {needle}", path.display()));
                }
            }
        }
    }

    assert!(
        scanned > 5,
        "the scan found only {scanned} event-building files — the heuristic has drifted and \
         this assertion is no longer checking anything",
    );
    assert!(
        offenders.is_empty(),
        "working-directory state must never reach relay-event construction: {offenders:?}",
    );
}
