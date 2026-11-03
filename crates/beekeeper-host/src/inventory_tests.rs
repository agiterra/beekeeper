//! The inventory's honesty rules, against files laid out the way the provider
//! and the management operations lay them out.
use super::*;
use beekeeper_core::coding_session_catalog::{Catalog, CatalogProvider, CATALOG_SCHEMA};
use beekeeper_core::coding_session_payload::Capabilities;
use beekeeper_host_core::agent_setup::{AgentSetupSpec, SetupAccess, SetupState};
use beekeeper_host_core::project::AgentsRepoRecord;
use uuid::Uuid;

struct Fixture {
    _dir: tempfile::TempDir,
    home: std::path::PathBuf,
    state: std::path::PathBuf,
    config: HostConfig,
}

fn descriptor(instance: &str, allowed: &[&str], discover: bool) -> serde_json::Value {
    serde_json::json!({
        "instanceRef": instance, "driver": "acme-acp", "runtime": "acme",
        "agentCommand": "acme-acp", "defaultModel": allowed[0],
        "allowedModels": allowed, "discoverModels": discover,
        "credentialEnv": ["ACME_TOKEN"], "credentialSource": "acme-vault"
    })
}

fn fixture(runtimes: Vec<serde_json::Value>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let config: HostConfig = serde_json::from_value(serde_json::json!({
        "version": 1, "instance": "production", "relayUrl": "ws://relay.example",
        "providerPubkey": "aa".repeat(32),
        "sessionProviderBaseDir": dir.path().join("base"),
        "providerStateDir": state, "runtimes": runtimes, "writtenAt": ""
    }))
    .unwrap();
    Fixture {
        _dir: dir,
        home,
        state,
        config,
    }
}

fn write_catalog(
    state: &std::path::Path,
    pid: u32,
    instance: &str,
    default: &str,
    allowed: &[&str],
    discovery: DiscoveryOutcome,
) {
    let file = EffectiveCatalogFile {
        version: 1,
        provider_pid: pid,
        written_at: "2026-11-02T00:00:00Z".into(),
        catalog: Catalog {
            schema: CATALOG_SCHEMA.into(),
            revision: 3,
            providers: vec![CatalogProvider {
                provider_instance_ref: instance.into(),
                driver: "acme-acp".into(),
                runtime: "acme".into(),
                default_model: default.into(),
                allowed_models: allowed.iter().map(|m| m.to_string()).collect(),
                capabilities: Capabilities::v1_baseline(),
                models: Vec::new(),
            }],
            projects: Vec::new(),
        },
        discovery: BTreeMap::from([(instance.to_string(), discovery)]),
    };
    std::fs::write(
        state.join(agent_setup::EFFECTIVE_CATALOG_FILE),
        serde_json::to_vec(&file).unwrap(),
    )
    .unwrap();
}

fn read_at(f: &Fixture, provider: ProviderChildState, sessions: Vec<SessionRow>) -> Inventory {
    let snapshot = SessionSnapshot {
        read_at: String::new(),
        unavailable: None,
        sessions,
    };
    read(InventoryInput {
        home: &f.home,
        instance: Instance::Production,
        config: &f.config,
        provider,
        host_pid: 1,
        host_version: "test".into(),
        sessions: &snapshot,
    })
}

fn live(pid: u32) -> ProviderChildState {
    ProviderChildState::Live {
        pid,
        started_at: String::new(),
    }
}

#[test]
fn the_effective_offer_wins_and_a_replaced_commissioned_model_stays_visible() {
    let f = fixture(vec![descriptor("acme-primary", &["sonnet[low]"], true)]);
    write_catalog(
        &f.state,
        42,
        "acme-primary",
        "claude-sonnet-5-5",
        &["claude-sonnet-5-5", "claude-opus-5-5"],
        DiscoveryOutcome::Discovered,
    );
    let row = read_at(&f, live(42), Vec::new()).runtimes.remove(0);
    assert_eq!(row.readiness, RuntimeReadiness::AdapterAnswered);
    let ids: Vec<_> = row
        .selectable_models
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(ids, ["claude-sonnet-5-5", "claude-opus-5-5"]);
    assert_eq!(row.commissioned_not_offered, ["sonnet[low]"]);
    assert!(row.selectable_models[0].is_default);
    assert!(row.readiness_detail.contains("unconfirmed"));
    assert_eq!(row.catalog_revision, Some(3));
}

#[test]
fn a_stopped_provider_offers_nothing_even_with_a_catalog_on_disk() {
    let f = fixture(vec![descriptor("acme-primary", &["m"], false)]);
    write_catalog(
        &f.state,
        42,
        "acme-primary",
        "m",
        &["m"],
        DiscoveryOutcome::NotRequested,
    );
    let row = read_at(&f, ProviderChildState::NotSupervised, Vec::new())
        .runtimes
        .remove(0);
    assert_eq!(row.readiness, RuntimeReadiness::ProviderNotLive);
    assert!(row.selectable_models.is_empty());
    assert_eq!(row.catalog_revision, None);
}

#[test]
fn a_catalog_from_another_process_is_not_current() {
    let f = fixture(vec![descriptor("acme-primary", &["m"], false)]);
    write_catalog(
        &f.state,
        41,
        "acme-primary",
        "m",
        &["m"],
        DiscoveryOutcome::NotRequested,
    );
    let row = read_at(&f, live(42), Vec::new()).runtimes.remove(0);
    assert_eq!(row.readiness, RuntimeReadiness::CatalogStale);
    assert!(row.selectable_models.is_empty());
}

#[test]
fn no_catalog_is_unavailable_rather_than_the_commissioned_list() {
    let f = fixture(vec![descriptor("acme-primary", &["m"], false)]);
    let row = read_at(&f, live(42), Vec::new()).runtimes.remove(0);
    assert_eq!(row.readiness, RuntimeReadiness::CatalogUnavailable);
    assert!(row.selectable_models.is_empty());
    assert_eq!(row.commissioned_models, ["m"]);
}

#[test]
fn a_failed_discovery_and_a_missing_credential_are_different_sentences() {
    let f = fixture(vec![descriptor("acme-primary", &["m"], true)]);
    write_catalog(
        &f.state,
        42,
        "acme-primary",
        "m",
        &["m"],
        DiscoveryOutcome::Failed("Agent process exited".into()),
    );
    let failed = read_at(&f, live(42), Vec::new()).runtimes.remove(0);
    assert_eq!(failed.readiness, RuntimeReadiness::DiscoveryFailed);
    assert!(failed.readiness_detail.contains("Agent process exited"));
    assert!(failed.readiness_detail.contains("fallback"));
    assert_eq!(
        failed.selectable_models.len(),
        1,
        "a concrete model stays, disclosed"
    );
    write_catalog(
        &f.state,
        42,
        "acme-primary",
        "m",
        &["m"],
        DiscoveryOutcome::CredentialsUnavailable("RUNTIME_CREDENTIAL_MISSING".into()),
    );
    let missing = read_at(&f, live(42), Vec::new()).runtimes.remove(0);
    assert_eq!(missing.readiness, RuntimeReadiness::CredentialsUnavailable);
    assert!(
        missing.selectable_models.is_empty(),
        "nothing can run on it"
    );
    assert!(missing
        .readiness_detail
        .contains("RUNTIME_CREDENTIAL_MISSING"));
}

#[test]
fn the_word_default_cannot_fall_through_a_failed_discovery() {
    let f = fixture(vec![descriptor("acme-primary", &["default"], true)]);
    write_catalog(
        &f.state,
        42,
        "acme-primary",
        "default",
        &["default"],
        DiscoveryOutcome::Failed("no answer".into()),
    );
    let row = read_at(&f, live(42), Vec::new()).runtimes.remove(0);
    assert_eq!(row.readiness, RuntimeReadiness::DiscoveryFailed);
    assert!(row.selectable_models.is_empty());
    assert!(row.readiness_detail.contains("`default` is not offered"));
}

#[test]
fn a_closed_record_is_not_a_held_execution() {
    let f = fixture(Vec::new());
    let actor = "cc".repeat(32);
    let spec = AgentSetupSpec {
        project: "30621:aa:slug".into(),
        channel: Uuid::from_u128(1),
        role: "lead".into(),
        display_name: "Agent".into(),
        provider_instance: "acme-primary".into(),
        model: "m".into(),
        effort: None,
        parallelism: 1,
        access: SetupAccess::OwnerOnly,
    };
    let setup = AgentSetup {
        version: 1,
        setup_id: Uuid::from_u128(9),
        spec_digest: spec.digest().unwrap(),
        spec,
        owner: "aa".into(),
        relay_url: "ws://relay.example".into(),
        created_at: String::new(),
        updated_at: String::new(),
        state: SetupState::Running,
        install_operation: Uuid::from_u128(10),
        start_operation: Uuid::from_u128(11),
        confirmed_start: None,
        context_operation: Uuid::from_u128(12),
        session_ref: Uuid::from_u128(13),
        genesis_event_id: None,
        project_context: None,
        pending_action: None,
        attempts: Vec::new(),
        agent_pubkey: Some(actor.clone()),
        revision: None,
        receipt_event_id: None,
        steps: Vec::new(),
        last_refusal: None,
        readback: None,
    };
    agent_setup::save_setup(&f.state, &setup).unwrap();
    let write = |closed: bool| {
        std::fs::write(
            f.state.join("state.json"),
            serde_json::json!({"sessions": {"s1": {"sessionId": "s1", "actor": actor, "closed": closed}}}).to_string(),
        )
        .unwrap();
    };
    write(true);
    let inventory = read_at(&f, live(1), Vec::new());
    assert_eq!(
        inventory.setups[0].effective_state,
        EffectiveSetupState::NotHeld
    );
    assert!(!inventory.setups[0].execution_held);
    write(false);
    let inventory = read_at(&f, live(1), Vec::new());
    assert_eq!(
        inventory.setups[0].effective_state,
        EffectiveSetupState::Running
    );
    assert!(inventory.setups[0].execution_held);
    assert_eq!(inventory.agents[0].open_sessions, 1);
}

#[test]
fn the_word_default_is_marked_as_a_label_not_a_model() {
    let f = fixture(vec![descriptor("goose-primary", &["default"], false)]);
    write_catalog(
        &f.state,
        42,
        "goose-primary",
        "default",
        &["default"],
        DiscoveryOutcome::NotRequested,
    );
    let row = read_at(&f, live(42), Vec::new()).runtimes.remove(0);
    assert_eq!(row.readiness, RuntimeReadiness::Unconfirmed);
    assert!(row.selectable_models[0].is_label);
}

fn seat(session: &str, project: Option<&str>, fenced: bool) -> serde_json::Value {
    serde_json::json!({"commandId": format!("csl-{session}"), "actor": "bb".repeat(32),
        "role": "builder", "projectRef": project, "sessionId": session, "generation": 2,
        "packRef": {"repo": "r", "sha": "abc", "role": "builder", "path": "roles/builder"},
        "fenced": fenced})
}

#[test]
fn every_recovery_row_names_its_own_reason_and_nothing_offers_clear_all() {
    let f = fixture(Vec::new());
    let trusted = "30621:aa:trusted";
    let projects = ProjectsFile {
        projects: BTreeMap::from([(trusted.to_string(), f.home.clone())]),
        ..Default::default()
    };
    std::fs::write(
        f.state.join(PROJECTS_FILE_NAME),
        serde_json::to_vec(&projects).unwrap(),
    )
    .unwrap();
    std::fs::write(
        f.state.join("seat-requests.json"),
        serde_json::to_vec(&serde_json::json!({"version": 1, "requests": [
            seat("s-fenced", Some(trusted), true),
            seat("s-gone", Some(trusted), false),
            seat("s-none", None, false),
            seat("s-other", Some("30621:aa:elsewhere"), false),
            seat("s-trusted", Some(trusted), false),
        ]}))
        .unwrap(),
    )
    .unwrap();
    let held = ["s-fenced", "s-none", "s-other", "s-trusted"]
        .into_iter()
        .map(|id| SessionRow {
            session_id: id.into(),
            generation: 2,
            channel_id: None,
            runtime: None,
            actor: None,
            role: None,
            turn_started_at_ms: None,
        })
        .collect();
    let recovery = read_at(&f, live(42), held).recovery;
    let codes: Vec<_> = recovery.rows.iter().map(|r| r.code.as_str()).collect();
    assert_eq!(
        codes,
        [
            "FENCED_BY_HANDOVER",
            "SESSION_NOT_HELD",
            "NO_PROJECT",
            "PROJECT_NOT_TRUSTED_HERE",
            "RESTAGE_NEEDS_DESKTOP"
        ]
    );
    assert_eq!(recovery.rows[4].pack_sha.as_deref(), Some("abc"));
    assert!(recovery.write_operation.contains("Read-only"));
    assert!(recovery.write_operation.contains("Keychain"));
}

#[test]
fn an_unreadable_seat_file_is_not_the_same_as_no_rows() {
    let f = fixture(Vec::new());
    assert!(read_at(&f, live(1), Vec::new())
        .recovery
        .unavailable
        .is_none());
    std::fs::write(f.state.join("seat-requests.json"), b"{").unwrap();
    let recovery = read_at(&f, live(1), Vec::new()).recovery;
    assert!(recovery.unavailable.unwrap().contains("malformed"));
}

#[test]
fn a_project_needs_both_recorded_paths_present_to_be_usable() {
    let f = fixture(Vec::new());
    let projects = ProjectsFile {
        projects: BTreeMap::from([
            ("30621:aa:both".to_string(), f.home.clone()),
            ("30621:aa:missing-dir".to_string(), f.home.join("gone")),
        ]),
        agents_repos: BTreeMap::from([
            (
                "30621:aa:both".to_string(),
                AgentsRepoRecord {
                    path: f.home.clone(),
                    ref_name: "refs/heads/main".into(),
                    url: None,
                },
            ),
            (
                "30621:aa:missing-dir".to_string(),
                AgentsRepoRecord {
                    path: f.home.clone(),
                    ref_name: "refs/heads/main".into(),
                    url: None,
                },
            ),
        ]),
        ..Default::default()
    };
    std::fs::write(
        f.state.join(PROJECTS_FILE_NAME),
        serde_json::to_vec(&projects).unwrap(),
    )
    .unwrap();
    let rows = read_at(&f, live(1), Vec::new()).projects;
    assert!(rows[0].usable);
    assert!(!rows[1].usable);
    assert!(!rows[1].checkout_present);
}

#[test]
fn agents_merge_sessions_enrollment_and_setups_without_opening_custody() {
    let f = fixture(Vec::new());
    let actor = "cc".repeat(32);
    let spec = AgentSetupSpec {
        project: "30621:aa:slug".into(),
        channel: Uuid::from_u128(1),
        role: "lead".into(),
        display_name: "Project agent".into(),
        provider_instance: "acme-primary".into(),
        model: "m".into(),
        effort: None,
        parallelism: 1,
        access: SetupAccess::OwnerOnly,
    };
    let setup = AgentSetup {
        version: 1,
        setup_id: Uuid::from_u128(9),
        spec_digest: spec.digest().unwrap(),
        spec,
        owner: "aa".into(),
        relay_url: "ws://relay.example".into(),
        created_at: String::new(),
        updated_at: String::new(),
        state: SetupState::Applying,
        install_operation: Uuid::from_u128(10),
        start_operation: Uuid::from_u128(11),
        confirmed_start: None,
        context_operation: Uuid::from_u128(12),
        session_ref: Uuid::from_u128(13),
        genesis_event_id: None,
        project_context: None,
        pending_action: None,
        attempts: Vec::new(),
        agent_pubkey: Some(actor.clone()),
        revision: None,
        receipt_event_id: None,
        steps: Vec::new(),
        last_refusal: None,
        readback: None,
    };
    agent_setup::save_setup(&f.state, &setup).unwrap();
    // Custody files that must never be opened: a secret here would otherwise
    // be one bad field away from the socket.
    std::fs::write(f.state.join("actor-seats.json"), b"nsec1secretseatvalue").unwrap();
    std::fs::write(f.state.join("provider-key"), b"nsec1secretprovider").unwrap();
    let session = SessionRow {
        session_id: "s1".into(),
        generation: 1,
        channel_id: None,
        runtime: Some("acme".into()),
        actor: Some(actor.clone()),
        role: Some("lead".into()),
        turn_started_at_ms: Some(5),
    };
    let inventory = read_at(&f, live(1), vec![session]);
    let row = &inventory.agents[0];
    assert_eq!(row.display_name.as_deref(), Some("Project agent"));
    assert_eq!(row.turns_in_flight, 1);
    assert_eq!(row.custody, "sessions_only");
    assert_eq!(
        row.setup_id.as_deref(),
        Some(setup.setup_id.to_string().as_str())
    );
    assert_eq!(
        inventory.setups[0].effective_state,
        EffectiveSetupState::Interrupted,
        "nothing holds the lock, so this apply is dead, not running"
    );
    let json = serde_json::to_string(&inventory).unwrap();
    assert!(!json.contains("nsec"), "{json}");
    assert!(!json.contains("secret"), "{json}");
}

/// Classify a copy of a real provider state directory without a Host, a key or a
/// relay. `HM_REAL_STATE_DIR` must hold `seat-requests.json`, `state.json` and
/// `projects.json` only. Prints counts, never rows.
#[test]
#[ignore = "reads a copy of real state named by HM_REAL_STATE_DIR"]
fn classify_a_copy_of_real_recovery_rows() {
    let dir = std::path::PathBuf::from(std::env::var("HM_REAL_STATE_DIR").unwrap());
    let projects: ProjectsFile =
        serde_json::from_slice(&std::fs::read(dir.join(PROJECTS_FILE_NAME)).unwrap()).unwrap();
    let sessions = crate::sessions::read_sessions(&dir);
    let found = recovery(&dir, &"aa".repeat(32), &projects, &sessions.sessions);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for row in &found.rows {
        *counts.entry(row.code.as_str()).or_default() += 1;
    }
    assert!(found.unavailable.is_none());
    println!(
        "rows={} stoppable={} {counts:?}",
        found.rows.len(),
        found
            .rows
            .iter()
            .filter(|row| row.stop_command.is_some())
            .count()
    );
}
