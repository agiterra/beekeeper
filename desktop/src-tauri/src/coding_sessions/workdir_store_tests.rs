//! Working-directory store behavior, and the containment law it exists for.

use std::path::PathBuf;

use super::workdir_store::{
    migrate_pending_worktrees, projects_view_provider_for_relay, validate_workdir,
    CodingSessionSeatWorktree, CodingSessionWorkdirScope, CodingSessionWorkdirStore,
    MAX_MRU_ENTRIES, MAX_PENDING_HINTS, MIGRATED_HINT_PREFIX, PROJECTS_VIEW_VERSION,
    WORKDIR_STORE_VERSION,
};
use crate::session_provider::store::{CodingSessionProviderRecord, CodingSessionProviderStore};

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

fn provider_record(relay_url: &str, provider_pubkey: &str) -> CodingSessionProviderRecord {
    CodingSessionProviderRecord {
        provider_pubkey: provider_pubkey.into(),
        instance_id: format!("desktop-{}", &provider_pubkey[..8]),
        auth_tag: None,
        created_at: "2026-08-30T00:00:00Z".into(),
        relay_url: relay_url.into(),
        private_key_nsec: String::new(),
    }
}

#[test]
fn provisioning_rematerializes_only_the_relay_captured_under_its_lock() {
    const RELAY_A: &str = "wss://relay-a.example";
    const RELAY_B: &str = "wss://relay-b.example";
    let pubkey_a = "a".repeat(64);
    let pubkey_b = "b".repeat(64);
    let mut providers = CodingSessionProviderStore::default();
    providers.upsert(RELAY_A, provider_record(RELAY_A, &pubkey_a));
    providers.upsert(RELAY_B, provider_record(RELAY_B, &pubkey_b));

    // Community B may become active after provisioning captured A. The
    // materialization boundary receives A explicitly and cannot re-resolve B.
    let relay_captured_under_lock = RELAY_A;
    let active_after_lock = RELAY_B;
    assert_ne!(relay_captured_under_lock, active_after_lock);
    let selected = projects_view_provider_for_relay(&providers, relay_captured_under_lock)
        .expect("the pinned provider exists");
    assert_eq!(selected.provider_pubkey, pubkey_a);
    assert_eq!(
        projects_view_provider_for_relay(&providers, active_after_lock)
            .expect("the newly active provider exists")
            .provider_pubkey,
        pubkey_b
    );
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
        serde_json::from_str(r#"{"version":2}"#).expect("deserialize");
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

    // The agents repository record reaches the provider as path and ref,
    // under the key names `buzz_session_provider::agents_checkout` parses.
    let mut store = store_with_choices();
    store.set_agents_repo(
        PROJECT_REF,
        PathBuf::from("/packs/aa-demo-beekeeper-agents"),
        "refs/heads/main",
    );
    let view = store.projects_view();
    let agents = view.agents_repos.get(PROJECT_REF).expect("recorded");
    assert_eq!(
        agents.path,
        PathBuf::from("/packs/aa-demo-beekeeper-agents")
    );
    assert_eq!(agents.ref_name, "refs/heads/main");
    let encoded = serde_json::to_string(&view).expect("serialize");
    assert!(encoded.contains(r#""agentsRepos""#), "{encoded}");
    assert!(encoded.contains(r#""ref":"refs/heads/main""#), "{encoded}");

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

    // The four fields of `buzz_session_provider::commands::ProjectsFile` — it
    // ignores anything else it is handed — plus `sessions`, read by that
    // crate's `gate_cwd` module and by nothing else. Both sides are additive:
    // a provider that predates the key ignores it, and a host that predates it
    // writes none, which reads as "no override" rather than as an error.
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "agentsRepos",
            "channels",
            "pending",
            "projects",
            "sessions",
            "version"
        ]
    );
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

// ── P3: the pending hints finding 60 stranded ──────────────────────────────

/// One store with a hint per shape, to migrate.
fn store_with_hints() -> CodingSessionWorkdirStore {
    let mut store = CodingSessionWorkdirStore::default();
    // The legacy sibling container, which is where every tree on the machine
    // that produced finding 60 actually lives.
    store.stage_hint("csl-1", PathBuf::from("/src/proj.worktrees/lane-a"));
    // The in-repo holder.
    store.stage_hint("csl-2", PathBuf::from("/src/proj/.worktrees/lane-b"));
    // A person's own checkout — staged as a hint on every ordinary create.
    store.stage_hint("csl-3", PathBuf::from("/src/proj"));
    // A per-worktree sibling: real, but not invertible from the path alone.
    store.stage_hint("csl-4", PathBuf::from("/src/proj-wt-lane-c"));
    store
}

/// Finding 60: 27 seat worktrees sat in `pending` while `worktrees` was empty,
/// so `bee sessions worktree status` and Pulse's disk row had nothing to show.
#[test]
fn hints_that_name_a_managed_worktree_become_records_the_reclaim_tool_can_see() {
    let mut store = store_with_hints();
    let migrated = migrate_pending_worktrees(&mut store);

    assert_eq!(migrated, 2);
    let mut paths: Vec<String> = store
        .worktrees
        .values()
        .map(|entry| entry.path.display().to_string())
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        vec!["/src/proj.worktrees/lane-a", "/src/proj/.worktrees/lane-b"]
    );
    for entry in store.worktrees.values() {
        assert_eq!(entry.repo_root, PathBuf::from("/src/proj"));
        // A branch is not knowable from a path, and inventing one would put a
        // name into the record whose whole purpose is naming what may go.
        assert_eq!(entry.branch, "");
    }
}

/// A migrated hint is *listable* and never *removable*: its key can never be
/// the session half of a real `<sessionRef>/<seatLabel>`, so no settlement fact
/// attaches to it and every predicate answers `not-settled`.
#[test]
fn a_migrated_hint_is_keyed_so_no_session_can_ever_claim_it() {
    let mut store = store_with_hints();
    migrate_pending_worktrees(&mut store);

    for key in store.worktrees.keys() {
        assert!(key.starts_with(MIGRATED_HINT_PREFIX), "{key}");
        let (session, seat) = key.split_once('/').expect("a two-part key");
        assert!(!seat.is_empty());
        // A session ref is a UUID; this can never be parsed as one.
        assert!(uuid::Uuid::parse_str(session).is_err(), "{session}");
    }
}

/// A person's own checkout is staged as a hint on every ordinary create. If
/// migration adopted it, the host would hold a record giving the prune path a
/// licence over somebody's working copy.
#[test]
fn a_plain_checkout_hint_is_never_adopted_as_a_seat_worktree() {
    let mut store = CodingSessionWorkdirStore::default();
    store.stage_hint("csl-3", PathBuf::from("/src/proj"));
    store.stage_hint("csl-5", PathBuf::from("/Users/someone/src/private-thing"));

    assert_eq!(migrate_pending_worktrees(&mut store), 0);
    assert!(store.worktrees.is_empty());
}

/// Run on every load, so it must converge rather than accumulate — and it must
/// not disturb a record a real create already wrote.
#[test]
fn migration_is_idempotent_and_never_overwrites_a_real_record() {
    let mut store = store_with_hints();
    assert_eq!(migrate_pending_worktrees(&mut store), 2);
    let after_first = store.worktrees.clone();
    assert_eq!(migrate_pending_worktrees(&mut store), 0);
    assert_eq!(store.worktrees, after_first);

    // The hint stays where it is: it may still be steering an in-flight
    // create, and a record is not a reason to break one.
    assert_eq!(store.pending.len(), 4);

    let real_key = format!("{}/builder-1", "11111111-2222-3333-4444-555555555555");
    store.worktrees.insert(
        real_key.clone(),
        CodingSessionSeatWorktree {
            path: PathBuf::from("/src/proj.worktrees/lane-a"),
            branch: "lane-a".into(),
            repo_root: PathBuf::from("/src/proj"),
            created_at: "2026-09-05T00:00:00Z".into(),
            session_id: Some("s-1".into()),
            agents_clone: None,
            commit_identity: None,
        },
    );
    assert_eq!(migrate_pending_worktrees(&mut store), 0);
    assert_eq!(store.worktrees[&real_key].branch, "lane-a");
}

/// The store schema stays at 2 whatever this adds, because `bee` hard-errors
/// on a version above its own maximum and would stop reading the record on
/// every machine that had opened the app once.
#[test]
fn migration_never_moves_the_schema_version() {
    let mut store = store_with_hints();
    migrate_pending_worktrees(&mut store);
    assert_eq!(store.version, WORKDIR_STORE_VERSION);
    assert_eq!(WORKDIR_STORE_VERSION, 2);
}

/// A record written before `sessionId` existed must round-trip byte-identically
/// — `skip_serializing_if` is what keeps a v2 file from growing a `null` the
/// older reader would have to be taught about.
#[test]
fn a_record_without_a_session_id_serializes_exactly_as_it_did_before() {
    let entry = CodingSessionSeatWorktree {
        path: PathBuf::from("/src/proj.worktrees/lane"),
        branch: "lane".into(),
        repo_root: PathBuf::from("/src/proj"),
        created_at: "2026-09-05T00:00:00Z".into(),
        session_id: None,
        agents_clone: None,
        commit_identity: None,
    };
    let encoded = serde_json::to_string(&entry).expect("serialize");
    assert!(!encoded.contains("sessionId"), "{encoded}");
    assert_eq!(
        serde_json::from_str::<CodingSessionSeatWorktree>(&encoded).expect("decode"),
        entry
    );
}

/// A prune is remembered after the directory it names is gone.
#[test]
fn a_prune_is_recorded_with_the_sentence_that_admitted_it() {
    let mut store = CodingSessionWorkdirStore::default();
    let entry = CodingSessionSeatWorktree {
        path: PathBuf::from("/src/proj.worktrees/lane"),
        branch: "lane".into(),
        repo_root: PathBuf::from("/src/proj"),
        created_at: "2026-09-05T00:00:00Z".into(),
        session_id: None,
        agents_clone: None,
        commit_identity: None,
    };
    store.record_prune(
        "s/builder-1",
        &entry,
        "/src/proj.worktrees/lane: clean, will be removed",
    );

    let recorded = &store.pruned["s/builder-1"];
    assert_eq!(recorded.path, entry.path);
    assert_eq!(recorded.branch, "lane");
    assert_eq!(
        recorded.reason,
        "/src/proj.worktrees/lane: clean, will be removed"
    );
    assert!(!recorded.pruned_at.is_empty());
}

#[test]
fn a_project_scoped_hint_becomes_the_project_default_when_it_has_none() {
    let mut store = CodingSessionWorkdirStore::default();
    let project = "30621:11868153aa:test-proj";
    store.stage_hint_for_project(
        "csl-1",
        Some(project),
        PathBuf::from("/src/test-proj"),
        None,
    );
    assert_eq!(store.pending["csl-1"], PathBuf::from("/src/test-proj"));
    assert_eq!(
        store.by_project[project].path,
        PathBuf::from("/src/test-proj")
    );

    // A later create in another tree keeps the recorded default: settings win.
    store.stage_hint_for_project(
        "csl-2",
        Some(project),
        PathBuf::from("/src/elsewhere"),
        None,
    );
    assert_eq!(store.pending["csl-2"], PathBuf::from("/src/elsewhere"));
    assert_eq!(
        store.by_project[project].path,
        PathBuf::from("/src/test-proj")
    );

    // No project, or a blank one: a one-shot hint and nothing more.
    store.stage_hint_for_project("csl-3", None, PathBuf::from("/src/standalone"), None);
    store.stage_hint_for_project("csl-4", Some("  "), PathBuf::from("/src/standalone"), None);
    assert_eq!(store.by_project.len(), 1);
    assert_eq!(store.pending.len(), 4);
}

#[test]
fn a_generated_worktree_hint_remembers_the_checkout_for_later_project_creates() {
    let mut store = CodingSessionWorkdirStore::default();
    let checkout = PathBuf::from("/src/beekeeper");
    let worktree = PathBuf::from("/src/beekeeper-wt-trashme");
    store.stage_hint_for_project(
        "worktree-create",
        Some(PROJECT_REF),
        worktree.clone(),
        Some(checkout.clone()),
    );

    // The exact command runs in its worktree, while a later phone/project
    // create with no command hint resolves to the canonical checkout.
    let view = store.projects_view();
    assert_eq!(view.pending["worktree-create"], worktree);
    assert_eq!(view.projects[PROJECT_REF], checkout);
    store.clear_hint("worktree-create");
    let reloaded: CodingSessionWorkdirStore =
        serde_json::from_str(&serde_json::to_string(&store).expect("serialize")).expect("reload");
    assert!(reloaded.projects_view().pending.is_empty());
    assert_eq!(reloaded.projects_view().projects[PROJECT_REF], checkout);
}

#[test]
fn a_canonical_hint_does_not_replace_an_existing_project_choice() {
    let mut store = store_with_choices();
    let original = store.by_project[PROJECT_REF].clone();
    store.stage_hint_for_project(
        "new-create",
        Some(PROJECT_REF),
        PathBuf::from("/src/another-worktree"),
        Some(PathBuf::from("/src/another-checkout")),
    );
    assert_eq!(store.by_project[PROJECT_REF], original);
    assert_eq!(
        store.projects_view().pending["new-create"],
        PathBuf::from("/src/another-worktree")
    );
}

#[test]
fn an_explicit_workspace_without_project_ref_never_sets_a_project_default() {
    let mut store = CodingSessionWorkdirStore::default();
    for (command, project_ref) in [("reuse", None), ("blank", Some("  "))] {
        store.stage_hint_for_project(
            command,
            project_ref,
            PathBuf::from("/src/shared-worktree"),
            Some(PathBuf::from("/src/checkout")),
        );
    }
    let view = store.projects_view();
    assert!(view.projects.is_empty());
    assert!(view.channels.is_empty());
    assert_eq!(view.pending.len(), 2);
    assert_eq!(view.pending["reuse"], PathBuf::from("/src/shared-worktree"));
    assert!(
        store.mru.is_empty(),
        "staging must not implicitly populate recents"
    );
}
