use super::*;
use std::cell::Cell;
use std::fs;
use std::process::Command;

use crate::coding_sessions::workdir_store::load_workdir_store_readonly_from;
use crate::coding_sessions::workdir_store::CodingSessionWorkdirStore;
use crate::managed_agents::crew_roles::DiscoveredRolePack;
use crate::managed_agents::storage_readiness::load_managed_agent_readiness_metadata_from;
use crate::session_provider::runtimes::{runtime_readiness_metadata, StrictRuntimeAuthState};
use crate::session_provider::store::load_provider_readiness_store_from;
use crate::session_provider::store::CodingSessionProviderReadinessRecord;
use crate::session_provider::store::CodingSessionProviderReadinessStore;

const PROJECT_REF: &str =
    "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:beekeeper";

fn signed_auth_tag(conditions: &str) -> (String, String, String) {
    let owner = nostr::Keys::generate();
    let subject = nostr::Keys::generate();
    let tag = buzz_sdk_pkg::nip_oa::compute_auth_tag(&owner, &subject.public_key(), conditions)
        .expect("mint valid auth tag");
    (
        owner.public_key().to_hex(),
        subject.public_key().to_hex(),
        tag,
    )
}

#[derive(Default)]
struct CountingHost {
    owner: Cell<usize>,
    workdirs: Cell<usize>,
    agents: Cell<usize>,
    live: Cell<usize>,
    runtimes: Cell<usize>,
    provider_store: Cell<usize>,
    relay: Cell<usize>,
    process: Cell<usize>,
}

impl ReadinessHost for CountingHost {
    fn owner_pubkey(&self) -> Result<String, String> {
        self.owner.set(self.owner.get() + 1);
        Err(KEYCHAIN_UNAVAILABLE.into())
    }

    fn workdirs(&self) -> Result<CodingSessionWorkdirStore, String> {
        self.workdirs.set(self.workdirs.get() + 1);
        Ok(CodingSessionWorkdirStore::default())
    }

    fn agents(
        &self,
        _expected_owner: Option<&str>,
    ) -> Result<Vec<ManagedAgentReadinessMetadata>, String> {
        self.agents.set(self.agents.get() + 1);
        Ok(Vec::new())
    }

    fn live_agent_pubkeys(&self) -> Result<HashSet<String>, String> {
        self.live.set(self.live.get() + 1);
        Ok(HashSet::new())
    }

    fn runtimes(&self) -> Vec<StrictRuntimeDiagnostic> {
        self.runtimes.set(self.runtimes.get() + 1);
        runtime_readiness_metadata()
    }

    fn provider_store(
        &self,
        _expected_owner: Option<&str>,
    ) -> Result<CodingSessionProviderReadinessStore, String> {
        self.provider_store.set(self.provider_store.get() + 1);
        Ok(CodingSessionProviderReadinessStore::default())
    }

    fn relay_url(&self) -> String {
        self.relay.set(self.relay.get() + 1);
        "wss://relay.example".into()
    }

    fn provider_process(&self, _pubkey: &str) -> CodingSessionProviderProcessState {
        self.process.set(self.process.get() + 1);
        CodingSessionProviderProcessState::Unknown
    }
}

#[test]
fn real_gather_graph_uses_only_injected_metadata_inventory() {
    let host = CountingHost::default();
    let response = gather(&host, PROJECT_REF.into(), None, Some(true));
    assert_eq!(response.host_class, TeamReadinessHostClass::Cold);
    assert_eq!(host.owner.get(), 1);
    assert_eq!(host.workdirs.get(), 1);
    assert_eq!(host.agents.get(), 1);
    assert_eq!(host.live.get(), 1);
    assert_eq!(host.runtimes.get(), 1);
    assert_eq!(host.provider_store.get(), 1);
    assert_eq!(host.relay.get(), 1);
    assert_eq!(host.process.get(), 0, "no process lookup without a record");
}

fn local_ready_gathered() -> Gathered {
    Gathered {
        source: TeamReadinessSource {
            app_commit: Some("a".repeat(40)),
            app_source_dirty: Some(false),
            checkout_path: Some("/tmp/project".into()),
            checkout_commit: Some("b".repeat(40)),
            checkout_dirty: Some(false),
        },
        owner_pubkey: Some("c".repeat(64)),
        key_store_safe: true,
        team: TeamReadinessTeam {
            packs: vec![TeamReadinessRolePack {
                role: "builder".into(),
                persona_name: "builder".into(),
                path: "/tmp/project/personas/roles/builder".into(),
                installed_pubkey: None,
            }],
            available_roles: vec!["builder".into()],
            ..Default::default()
        },
        registry: TeamReadinessRegistryCoverage {
            provider_targets: vec!["codex-primary".into()],
            covered_targets: Vec::new(),
            uncovered_targets: Vec::new(),
            pending_targets: vec!["codex-primary:gpt-5.6-luna".into()],
        },
        runtimes: runtime_readiness_metadata(),
        provider: TeamReadinessProvider {
            provisioned: true,
            process: "live".into(),
            ..Default::default()
        },
        policy: TeamReadinessPolicy {
            hiring_policy: "enabled".into(),
            ..Default::default()
        },
        facts: vec![TeamReadinessFact::local(
            "host",
            "LOCAL_PLANES_READY",
            TeamReadinessFactState::Ready,
            "local planes ready",
        )],
    }
}

#[test]
fn project_coordinate_validation_is_strict() {
    assert!(valid_project_ref(PROJECT_REF));
    assert!(!valid_project_ref(""));
    assert!(!valid_project_ref("30621:short:beekeeper"));
    assert!(!valid_project_ref(&format!(
        "30617:{}:beekeeper",
        "a".repeat(64)
    )));
    assert!(!valid_project_ref(&format!("30621:{}:", "a".repeat(64))));
    assert!(!valid_project_ref(&format!(
        "30621:{}:beekeeper",
        "A".repeat(64)
    )));
}

#[test]
fn readiness_auth_requires_canonical_signed_owner_attestation() {
    let (owner, subject, tag) = signed_auth_tag("");
    let valid = crate::readiness_auth::inspect_auth_tag(Some(&tag), &subject, Some(&owner));
    assert!(valid.present);
    assert_eq!(valid.verified_owner.as_deref(), Some(owner.as_str()));

    let (_, conditioned_subject, conditioned) = signed_auth_tag("kind=1");
    assert!(
        crate::readiness_auth::inspect_auth_tag(Some(&conditioned), &conditioned_subject, None)
            .invalid
    );

    let mut random_signature: Vec<String> = serde_json::from_str(&tag).expect("tag array");
    random_signature[3] = "00".repeat(64);
    let random_signature = serde_json::to_string(&random_signature).expect("tag json");
    assert!(
        crate::readiness_auth::inspect_auth_tag(Some(&random_signature), &subject, None).invalid
    );

    let wrong_subject = nostr::Keys::generate().public_key().to_hex();
    assert!(crate::readiness_auth::inspect_auth_tag(Some(&tag), &wrong_subject, None).invalid);

    let wrong_owner = nostr::Keys::generate().public_key().to_hex();
    let mismatch =
        crate::readiness_auth::inspect_auth_tag(Some(&tag), &subject, Some(&wrong_owner));
    assert!(mismatch.owner_mismatch);
    assert!(!mismatch.present);

    assert!(
        crate::readiness_auth::inspect_auth_tag(Some(r#"["auth","bad"]"#), &subject, None).invalid
    );
    let self_tag = serde_json::json!(["auth", subject, "", "00".repeat(64)]).to_string();
    assert!(crate::readiness_auth::inspect_auth_tag(Some(&self_tag), &subject, None).invalid);

    let mut gathered = Gathered::default();
    gathered.team.selected_roles = vec!["builder".into()];
    append_agent_auth_facts(
        &[ManagedAgentReadinessMetadata {
            pubkey: "a".repeat(64),
            name: "Bob".into(),
            home_role: Some("builder".into()),
            persona_team_dir: None,
            persona_name_in_team: None,
            persona_source_version: None,
            auth_tag_present: false,
            auth_tag_owner: Some(owner),
            auth_tag_invalid: false,
            auth_tag_owner_mismatch: true,
        }],
        &mut gathered,
    );
    assert_eq!(gathered.facts[0].code, "AGENT_AUTH_OWNER_MISMATCH");
    assert_eq!(gathered.facts[0].state, TeamReadinessFactState::Unknown);

    let mut gathered = Gathered::default();
    append_provider_auth_fact(
        &CodingSessionProviderReadinessRecord {
            provider_pubkey: "a".repeat(64),
            instance_id: "host-a".into(),
            auth_tag_present: false,
            auth_tag_owner: None,
            auth_tag_invalid: true,
            auth_tag_owner_mismatch: false,
            created_at: "2026-08-30T00:00:00Z".into(),
            relay_url: "wss://relay.example".into(),
        },
        &mut gathered,
    );
    assert_eq!(gathered.facts[0].code, "PROVIDER_AUTH_TAG_INVALID");
    assert_eq!(gathered.facts[0].state, TeamReadinessFactState::Unknown);
}

#[test]
fn embedded_source_preserves_explicit_unknowns() {
    let missing = parse_embedded_source(None, None);
    assert_eq!(missing.app_commit, None);
    assert_eq!(missing.app_source_dirty, None);

    let invalid = parse_embedded_source(Some("short"), Some("maybe"));
    assert_eq!(invalid.app_commit, None);
    assert_eq!(invalid.app_source_dirty, None);

    let known = parse_embedded_source(Some(&"A".repeat(40)), Some("1"));
    assert_eq!(known.app_commit, Some("a".repeat(40)));
    assert_eq!(known.app_source_dirty, Some(true));
    assert_eq!(
        parse_embedded_source(Some(&"a".repeat(40)), Some("0")).app_source_dirty,
        None,
        "a false-clean build claim is intentionally not trusted"
    );
}

#[test]
fn registry_targets_keep_provider_instance_and_model_together() {
    let valid = include_str!("../../../../team/model-registry.yaml");
    let targets = registry_targets(valid).expect("the router's registry parser accepts it");
    assert!(!targets.is_empty());
    assert!(targets
        .iter()
        .all(|(provider, target)| target.starts_with(&format!("{provider}:"))));
    assert!(registry_targets(&valid.replace("version: 1", "version: 0")).is_err());
    assert!(registry_targets("version: 1\ntargets: []").is_err());
}

#[test]
fn prepared_host_can_launch_first_session_while_wire_is_awaiting() {
    let response = finish(PROJECT_REF.into(), local_ready_gathered());
    assert_eq!(response.status, TeamReadinessStatus::AwaitingFirstSession);
    assert_eq!(
        response.host_class,
        TeamReadinessHostClass::PreparedForFirstSession
    );
    assert!(response.ready_for_first_session);
    assert!(!response.ready);
    assert_eq!(
        response.awaiting_codes,
        vec!["CATALOG_AWAITING_FIRST_SESSION", "RELAY_UNOBSERVED"]
    );
}

#[test]
fn cold_locked_and_unknown_hosts_never_report_launch_ready() {
    let mut cold = Gathered {
        source: TeamReadinessSource {
            app_commit: Some("a".repeat(40)),
            app_source_dirty: Some(false),
            ..Default::default()
        },
        ..Default::default()
    };
    cold.facts.push(TeamReadinessFact::blocked(
        "project",
        "CHECKOUT_NOT_RECORDED",
        "missing",
        "Choose a checkout.",
    ));
    let response = finish(PROJECT_REF.into(), cold);
    assert_eq!(response.host_class, TeamReadinessHostClass::Cold);
    assert_eq!(response.status, TeamReadinessStatus::Blocked);
    assert!(!response.ready_for_first_session);

    let mut locked = local_ready_gathered();
    locked.key_store_safe = false;
    locked.owner_pubkey = None;
    locked.facts.push(TeamReadinessFact::unknown(
        "identity",
        KEYCHAIN_UNAVAILABLE,
        "locked",
        "Unlock and relaunch.",
    ));
    let response = finish(PROJECT_REF.into(), locked);
    assert_eq!(response.status, TeamReadinessStatus::Unknown);
    assert!(!response.ready_for_first_session);
    assert_eq!(response.unknown_codes, vec![KEYCHAIN_UNAVAILABLE]);
}

#[test]
fn one_provider_is_nonblocking_but_zero_is_not_ready() {
    let mut one = local_ready_gathered();
    one.facts.push(TeamReadinessFact::local(
        "runtime",
        "PROVIDER_DIVERSITY_LIMITED",
        TeamReadinessFactState::Limited,
        "one provider",
    ));
    let response = finish(PROJECT_REF.into(), one);
    assert!(response.ready_for_first_session);
    assert_eq!(response.limited_codes, vec!["PROVIDER_DIVERSITY_LIMITED"]);

    let mut zero = local_ready_gathered();
    zero.facts.push(TeamReadinessFact::blocked(
        "runtime",
        "RUNTIME_UNAVAILABLE",
        "zero providers",
        "Install one runtime.",
    ));
    let response = finish(PROJECT_REF.into(), zero);
    assert!(!response.ready_for_first_session);
}

#[test]
fn only_selected_roles_gate_first_launch() {
    let mut unselected = local_ready_gathered();
    unselected.facts.push(TeamReadinessFact::local(
        "team",
        "ROLE_PACKS_MISSING",
        TeamReadinessFactState::Limited,
        "no role selected",
    ));
    assert!(finish(PROJECT_REF.into(), unselected).ready_for_first_session);

    let mut selected = local_ready_gathered();
    selected.team.selected_roles = vec!["verifier".into()];
    selected.facts.push(TeamReadinessFact::blocked(
        "team",
        "SELECTED_ROLE_UNAVAILABLE",
        "selected role missing",
        "Choose an available role.",
    ));
    assert!(!finish(PROJECT_REF.into(), selected).ready_for_first_session);
}

#[test]
fn selected_role_pack_state_distinguishes_dirty_and_wrong_project() {
    let pack = DiscoveredRolePack {
        dir: PathBuf::from("/project/personas/roles/builder"),
        persona_name: "builder".into(),
        display_name: "Builder".into(),
        role: "builder".into(),
        system_prompt: "Build carefully".into(),
        runtime: Some("codex".into()),
        model: None,
        provider: None,
        avatar_url: None,
    };
    let mut row = ManagedAgentReadinessMetadata {
        pubkey: "a".repeat(64),
        name: "Bob".into(),
        home_role: Some("builder".into()),
        persona_team_dir: Some(pack.dir.clone()),
        persona_name_in_team: Some(pack.persona_name.clone()),
        persona_source_version: None,
        auth_tag_present: true,
        auth_tag_owner: None,
        auth_tag_invalid: false,
        auth_tag_owner_mismatch: false,
    };
    assert_eq!(
        selected_role_pack_state(&[row.clone()], &pack),
        SelectedRolePackState::SourceUnknown
    );
    row.persona_source_version = Some("b".repeat(64));
    assert_eq!(
        selected_role_pack_state(&[row.clone()], &pack),
        SelectedRolePackState::Dirty
    );
    row.persona_source_version = Some(role_pack_source_version(&pack, &row.name));
    assert_eq!(
        selected_role_pack_state(&[row.clone()], &pack),
        SelectedRolePackState::Current
    );
    row.persona_team_dir = Some(PathBuf::from("/other/personas/roles/builder"));
    assert_eq!(
        selected_role_pack_state(&[row], &pack),
        SelectedRolePackState::WrongProject
    );
}

#[test]
fn trusted_wire_fold_requires_reachability_and_a_covered_target() {
    let local = finish(PROJECT_REF.into(), local_ready_gathered());
    let ready = fold_trusted_team_wire(
        local.clone(),
        Some(TrustedTeamCatalogSnapshot {
            revision: 8,
            targets: vec!["codex-primary:gpt-5.6-luna".into()],
        }),
        Some(true),
    );
    assert_eq!(ready.status, TeamReadinessStatus::Ready);
    assert!(ready.ready);
    assert_eq!(ready.catalog.revision, Some(8));
    assert!(ready.runtimes.iter().any(|runtime| {
        runtime.instance_ref == "codex-primary"
            && runtime.auth == StrictRuntimeAuthState::Ready
            && runtime.model_probe == "trusted_provider_catalog"
    }));

    let uncovered = fold_trusted_team_wire(
        local.clone(),
        Some(TrustedTeamCatalogSnapshot {
            revision: 8,
            targets: vec!["claude-primary:claude-sonnet-5".into()],
        }),
        Some(true),
    );
    assert_eq!(uncovered.status, TeamReadinessStatus::Unknown);
    assert!(!uncovered.ready);
    assert_eq!(uncovered.unknown_codes, vec!["CATALOG_TARGETS_UNCOVERED"]);

    let unreachable = fold_trusted_team_wire(
        local.clone(),
        Some(TrustedTeamCatalogSnapshot {
            revision: 8,
            targets: vec!["codex-primary:gpt-5.6-luna".into()],
        }),
        Some(false),
    );
    assert_eq!(unreachable.status, TeamReadinessStatus::Blocked);
    assert!(!unreachable.ready);

    let untrusted = fold_trusted_team_wire(local, None, Some(true));
    assert_eq!(untrusted.status, TeamReadinessStatus::Unknown);
    assert_eq!(untrusted.catalog.revision, None);
}

#[test]
fn contract_serializes_local_and_wire_planes_without_hidden_defaults() {
    let response = finish(PROJECT_REF.into(), local_ready_gathered());
    let json = serde_json::to_value(response).expect("serialize readiness");
    assert_eq!(json["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(json["readyForFirstSession"], true);
    assert_eq!(json["hostClass"], "prepared_for_first_session");
    assert_eq!(json["catalog"]["source"], "wire");
    assert_eq!(json["relay"]["reachable"], serde_json::Value::Null);
    assert_eq!(json["team"]["selectedRoles"], serde_json::json!([]));
    assert!(json.get("project_ref").is_none());
}

#[test]
fn readonly_stores_do_not_create_missing_directories() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workdir = temp.path().join("config/nested/workdirs.json");
    let agents = temp.path().join("data/agents/managed-agents.json");
    let provider = temp.path().join("data/provider/provider.json");

    assert!(load_workdir_store_readonly_from(&workdir).is_ok());
    assert!(load_managed_agent_readiness_metadata_from(&agents, None).is_ok());
    assert!(load_provider_readiness_store_from(&provider, None).is_ok());
    assert_eq!(fs::read_dir(temp.path()).expect("root exists").count(), 0);
}

#[test]
fn readonly_workdir_inventory_rejects_malformed_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("workdirs.json");
    for payload in [
        r#"{"version":99,"byProject":{},"byChannel":{},"mru":[],"pending":{}}"#,
        r#"{"version":1,"byProject":{"project":{"path":"relative","updatedAt":"now"}},"byChannel":{},"mru":[],"pending":{}}"#,
    ] {
        fs::write(&path, payload).expect("seed malformed workdir store");
        assert!(load_workdir_store_readonly_from(&path).is_err());
    }
}

#[test]
fn readonly_agent_inventory_neither_backs_up_nor_hydrates_secrets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("managed-agents.json");
    fs::write(&path, "not json").expect("seed invalid store");
    let before = fs::read(&path).expect("read seed");
    assert!(load_managed_agent_readiness_metadata_from(&path, None).is_err());
    assert_eq!(fs::read(&path).expect("read after"), before);
    assert_eq!(fs::read_dir(temp.path()).expect("list").count(), 1);

    let (owner, subject, auth_tag) = signed_auth_tag("");
    fs::write(
        &path,
        serde_json::json!([{
            "pubkey": subject,
            "name": "Bob",
            "home_role": "builder",
            "private_key_nsec": "nsec-secret",
            "auth_tag": auth_tag,
            "persona_source_version": "a".repeat(64),
        }])
        .to_string(),
    )
    .expect("seed public plus secret fields");
    let rows =
        load_managed_agent_readiness_metadata_from(&path, Some(&owner)).expect("metadata only");
    assert_eq!(rows.len(), 1);
    assert!(rows[0].auth_tag_present);
    assert_eq!(rows[0].persona_source_version, Some("a".repeat(64)));
    let wrong_owner = nostr::Keys::generate().public_key().to_hex();
    let mismatch = load_managed_agent_readiness_metadata_from(&path, Some(&wrong_owner))
        .expect("valid signature, wrong owner");
    assert!(!mismatch[0].auth_tag_present);
    assert!(mismatch[0].auth_tag_owner_mismatch);
    let projection = format!("{rows:?}");
    assert!(!projection.contains("nsec-secret"));
    assert!(!projection.contains("auth-secret"));
    assert_eq!(fs::read_dir(temp.path()).expect("list").count(), 1);

    fs::write(
        &path,
        format!(
            r#"[{{"pubkey":"{}","name":"Bob","auth_tag":null}}]"#,
            "d".repeat(64)
        ),
    )
    .expect("seed null auth tag");
    let rows = load_managed_agent_readiness_metadata_from(&path, None).expect("metadata only");
    assert!(
        !rows[0].auth_tag_present,
        "null is absence, not a credential"
    );
}

#[test]
fn readonly_agent_inventory_rejects_malformed_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("managed-agents.json");
    for payload in [
        r#"[{"pubkey":"","name":"Bob"}]"#.to_string(),
        format!(
            r#"[{{"pubkey":"{}","name":"Bob","home_role":"Lead"}}]"#,
            "a".repeat(64)
        ),
        format!(
            r#"[{{"pubkey":"{}","name":"Bob","persona_source_version":"short"}}]"#,
            "a".repeat(64)
        ),
    ] {
        fs::write(&path, payload).expect("seed malformed metadata");
        assert!(load_managed_agent_readiness_metadata_from(&path, None).is_err());
    }
}

#[test]
fn readonly_provider_inventory_neither_mutates_nor_exposes_secrets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("provider.json");
    let relay = "wss://relay.example";
    let (owner, subject, auth_tag) = signed_auth_tag("");
    fs::write(
        &path,
        serde_json::json!({
            "version": 1,
            "providers": {
                (relay): {
                    "providerPubkey": subject,
                    "instanceId": "host-a",
                    "authTag": auth_tag,
                    "createdAt": "2026-08-30T00:00:00Z",
                    "relayUrl": relay,
                    "privateKeyNsec": "nsec-secret",
                }
            },
            "maxSessions": 4,
        })
        .to_string(),
    )
    .expect("seed provider store");
    let before = fs::read(&path).expect("read seed");
    let store = load_provider_readiness_store_from(&path, Some(&owner)).expect("metadata only");
    let record = store.providers.values().next().expect("provider row");
    assert!(record.auth_tag_present);
    assert_eq!(store.max_sessions, Some(4));
    let wrong_owner = nostr::Keys::generate().public_key().to_hex();
    let mismatch = load_provider_readiness_store_from(&path, Some(&wrong_owner))
        .expect("valid signature, wrong owner");
    let mismatch = mismatch.providers.values().next().expect("provider row");
    assert!(!mismatch.auth_tag_present);
    assert!(mismatch.auth_tag_owner_mismatch);
    let projection = format!("{store:?}");
    assert!(!projection.contains("nsec-secret"));
    assert!(!projection.contains("auth-secret"));
    assert_eq!(fs::read(&path).expect("read after"), before);
    assert_eq!(fs::read_dir(temp.path()).expect("list").count(), 1);

    fs::write(
        &path,
        format!(
            r#"{{"version":1,"providers":{{"{relay}":{{"providerPubkey":"{}","instanceId":"host-a","authTag":null,"createdAt":"2026-08-30T00:00:00Z","relayUrl":"{relay}"}}}}}}"#,
            "e".repeat(64)
        ),
    )
    .expect("seed null auth tag");
    let store = load_provider_readiness_store_from(&path, None).expect("metadata only");
    assert!(
        !store.providers.values().next().unwrap().auth_tag_present,
        "null is absence, not a credential"
    );
}

#[test]
fn readonly_provider_inventory_rejects_malformed_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("provider.json");
    let relay = "wss://relay.example";
    for payload in [
        r#"{"version":99,"providers":{}}"#.to_string(),
        format!(
            r#"{{"version":1,"providers":{{"{relay}":{{"providerPubkey":"","instanceId":"host-a","authTag":null,"createdAt":"2026-08-30T00:00:00Z","relayUrl":"{relay}"}}}}}}"#
        ),
        format!(
            r#"{{"version":1,"providers":{{"{relay}":{{"providerPubkey":"{}","instanceId":"","authTag":null,"createdAt":"2026-08-30T00:00:00Z","relayUrl":"{relay}"}}}}}}"#,
            "a".repeat(64)
        ),
    ] {
        fs::write(&path, payload).expect("seed malformed metadata");
        assert!(load_provider_readiness_store_from(&path, None).is_err());
    }
}

#[test]
fn checkout_probe_detects_untracked_files_without_git_locks() {
    let temp = tempfile::tempdir().expect("tempdir");
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(args)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    };
    run(&["init", "-q"]);
    fs::write(temp.path().join("tracked"), "one").expect("tracked");
    run(&["add", "tracked"]);
    run(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-q",
        "--no-gpg-sign",
        "-m",
        "seed",
    ]);
    let (_, dirty) = checkout_source(temp.path()).expect("clean source");
    assert!(!dirty);

    fs::write(temp.path().join(".untracked-hidden"), "two").expect("untracked");
    let (_, dirty) = checkout_source(temp.path()).expect("dirty source");
    assert!(dirty);
    assert!(!temp.path().join(".git/index.lock").exists());
}

#[test]
fn checkout_probe_rejects_unexpected_git_exit_codes() {
    assert_eq!(validate_dirty(0, 1), Ok(false));
    assert_eq!(validate_dirty(1, 1), Ok(true));
    assert!(validate_dirty(128, 1).is_err());
    assert!(validate_dirty(0, 128).is_err());
}
