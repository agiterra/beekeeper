use super::auth_facts::append_agent_auth_facts;
use super::*;
use std::cell::Cell;
use std::fs;

use crate::coding_sessions::workdir_store::load_workdir_store_readonly_from;
use crate::coding_sessions::workdir_store::CodingSessionWorkdirStore;
use crate::managed_agents::storage_readiness::load_managed_agent_readiness_metadata_from;
use crate::session_provider::runtimes::runtime_readiness_metadata;
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
pub(super) struct CountingHost {
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
    let response = gather(
        &host,
        PROJECT_REF.into(),
        None,
        Some(true),
        &ProjectPackSourceProbe::NotProbed,
    );
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
fn signed_catalog_query_and_validation_bind_signer_channel_and_project() {
    let keys = nostr::Keys::generate();
    let channel =
        uuid::Uuid::parse_str("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10").expect("channel uuid");
    let signer = keys.public_key().to_hex();
    let content = catalog_content(8, PROJECT_REF, "gpt-5.6-luna");
    let event = buzz_sdk_pkg::builders::build_coding_session_provider_catalog(channel, 8, &content)
        .expect("catalog builder")
        .sign_with_keys(&keys)
        .expect("signed catalog");
    let filter =
        wire::provider_catalog_filter(std::slice::from_ref(&signer), &[channel.to_string()]);
    assert_eq!(
        filter,
        serde_json::json!({
            "kinds": [buzz_core_pkg::kind::KIND_CODING_SESSION_PROVIDER_CATALOG],
            "authors": [signer],
            "#h": [channel.to_string()],
            "limit": 1001,
        })
    );

    let snapshot = wire::validate_catalog_events(
        std::slice::from_ref(&event),
        &[keys.public_key().to_hex()],
        &[channel.to_string()],
        PROJECT_REF,
    );
    assert_eq!(snapshot.invalid_event_count, 0);
    assert_eq!(snapshot.conflict_count, 0);
    assert_eq!(snapshot.targets, vec!["codex-primary:gpt-5.6-luna"]);
    assert_eq!(snapshot.provenance[0].event_id, event.id.to_hex());

    let wrong_signer = wire::validate_catalog_events(
        std::slice::from_ref(&event),
        &[nostr::Keys::generate().public_key().to_hex()],
        &[channel.to_string()],
        PROJECT_REF,
    );
    assert_eq!(wrong_signer.invalid_event_count, 1);
    assert!(wrong_signer.provenance.is_empty());

    let wrong_channel = wire::validate_catalog_events(
        std::slice::from_ref(&event),
        &[keys.public_key().to_hex()],
        &[uuid::Uuid::new_v4().to_string()],
        PROJECT_REF,
    );
    assert_eq!(wrong_channel.invalid_event_count, 1);
    assert!(wrong_channel.targets.is_empty());

    let wrong_project = wire::validate_catalog_events(
        &[event],
        &[keys.public_key().to_hex()],
        &[channel.to_string()],
        "30621:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:other",
    );
    assert_eq!(wrong_project.provenance.len(), 1);
    assert!(wrong_project.targets.is_empty());
}

#[test]
fn relay_scope_is_rechecked_at_every_async_boundary() {
    assert!(
        wire::ensure_active_relay_scope("wss://relay-a.example/", "WSS://RELAY-A.EXAMPLE").is_ok()
    );
    for active_after_await in ["wss://relay-b.example", "", "  "] {
        assert!(
            wire::ensure_active_relay_scope("wss://relay-a.example", active_after_await).is_err()
        );
    }
    assert!(wire::ensure_active_relay_scope("", "").is_err());
}

#[test]
fn failing_trust_read_is_rejected_if_the_community_changed_while_awaiting_it() {
    let stale_failure = wire::classify_trusted_signers(Err("malformed metadata".into()));
    let result = wire::finish_trust_observation_for_scope(
        "wss://relay-a.example",
        "wss://relay-b.example",
        stale_failure,
    );
    assert!(
        result.is_err(),
        "a stale A trust failure must not become B's readiness Unknown"
    );

    let current_failure = wire::classify_trusted_signers(Err("malformed metadata".into()));
    let observation = wire::finish_trust_observation_for_scope(
        "wss://relay-a.example",
        "wss://relay-a.example",
        current_failure,
    )
    .expect("unchanged community")
    .expect_err("malformed trust remains structured Unknown");
    assert!(matches!(
        observation,
        wire::TeamReadinessWireObservation::Unknown { ref code, .. }
            if code == "TRUST_CONFIG_INVALID"
    ));
}

#[test]
fn catalog_history_sentinel_and_cross_channel_catalog_fail_closed() {
    let keys = nostr::Keys::generate();
    let channel_a = uuid::Uuid::new_v4();
    let channel_b = uuid::Uuid::new_v4();
    let event = buzz_sdk_pkg::builders::build_coding_session_provider_catalog(
        channel_a,
        1,
        &catalog_content(1, PROJECT_REF, "gpt-5.6-luna"),
    )
    .expect("catalog builder")
    .sign_with_keys(&keys)
    .expect("signed catalog");
    let signer = keys.public_key().to_hex();
    let cross_channel = wire::classify_catalog_query(
        std::slice::from_ref(&event),
        std::slice::from_ref(&signer),
        &[channel_b.to_string()],
        PROJECT_REF,
    );
    let response = fold_trusted_team_wire(
        finish(PROJECT_REF.into(), local_ready_gathered()),
        cross_channel,
    );
    assert_eq!(response.status, TeamReadinessStatus::Unknown);
    assert!(response
        .unknown_codes
        .contains(&"CATALOG_EVENTS_INVALID".to_string()));
    assert!(response
        .unknown_codes
        .contains(&"CATALOG_TARGETS_UNCOVERED".to_string()));
    assert!(response.registry.covered_targets.is_empty());

    let overflow = wire::classify_catalog_query(
        &vec![event; 1001],
        &[signer],
        &[channel_a.to_string()],
        PROJECT_REF,
    );
    let response =
        fold_trusted_team_wire(finish(PROJECT_REF.into(), local_ready_gathered()), overflow);
    assert_eq!(response.status, TeamReadinessStatus::Unknown);
    assert_eq!(response.unknown_codes, vec!["CATALOG_HISTORY_OVERFLOW"]);
}

#[test]
fn equal_catalog_revision_with_different_signed_content_is_a_conflict() {
    let keys = nostr::Keys::generate();
    let channel = uuid::Uuid::new_v4();
    let first = buzz_sdk_pkg::builders::build_coding_session_provider_catalog(
        channel,
        4,
        &catalog_content(4, PROJECT_REF, "gpt-5.6-luna"),
    )
    .expect("catalog builder")
    .sign_with_keys(&keys)
    .expect("signed catalog");
    let second = buzz_sdk_pkg::builders::build_coding_session_provider_catalog(
        channel,
        4,
        &catalog_content(4, PROJECT_REF, "gpt-5.6-sol"),
    )
    .expect("catalog builder")
    .sign_with_keys(&keys)
    .expect("signed catalog");
    let snapshot = wire::validate_catalog_events(
        &[first, second],
        &[keys.public_key().to_hex()],
        &[channel.to_string()],
        PROJECT_REF,
    );
    assert_eq!(snapshot.conflict_count, 1);
    assert!(snapshot.provenance.is_empty());
    assert!(snapshot.targets.is_empty());
}

#[test]
fn empty_reverified_catalog_query_stays_awaiting_first_session() {
    let local = finish(PROJECT_REF.into(), local_ready_gathered());
    let response = fold_trusted_team_wire(
        local,
        TeamReadinessWireObservation::Reached(TrustedTeamCatalogSnapshot {
            provenance: Vec::new(),
            targets: Vec::new(),
            invalid_event_count: 0,
            conflict_count: 0,
        }),
    );
    assert!(response.ready_for_first_session);
    assert!(!response.ready);
    assert_eq!(response.status, TeamReadinessStatus::AwaitingFirstSession);
    assert_eq!(response.relay.reachable, Some(true));
    assert_eq!(
        response.awaiting_codes,
        vec!["CATALOG_AWAITING_FIRST_SESSION"]
    );
}

#[test]
fn missing_first_session_channel_uses_only_local_launch_truth() {
    let response = fold_trusted_team_wire(
        finish(PROJECT_REF.into(), local_ready_gathered()),
        TeamReadinessWireObservation::AwaitingChannel,
    );
    assert!(response.ready_for_first_session);
    assert!(!response.ready);
    assert_eq!(response.status, TeamReadinessStatus::AwaitingFirstSession);
    assert_eq!(response.relay.reachable, None);
    assert_eq!(
        response.awaiting_codes,
        vec!["CATALOG_AWAITING_FIRST_SESSION", "RELAY_UNOBSERVED"]
    );
}

#[test]
fn trusted_wire_fold_requires_reachability_and_a_covered_target() {
    let local = finish(PROJECT_REF.into(), local_ready_gathered());
    let ready = fold_trusted_team_wire(
        local.clone(),
        TeamReadinessWireObservation::Reached(TrustedTeamCatalogSnapshot {
            provenance: vec![wire::TeamReadinessCatalogProvenance {
                event_id: "d".repeat(64),
                channel_id: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
                signer_pubkey: "e".repeat(64),
                revision: 8,
            }],
            targets: vec!["codex-primary:gpt-5.6-luna".into()],
            invalid_event_count: 0,
            conflict_count: 0,
        }),
    );
    assert_eq!(ready.status, TeamReadinessStatus::Ready);
    assert!(ready.ready);
    assert_eq!(ready.catalog.revision, Some(8));
    assert_eq!(ready.catalog.provenance.len(), 1);
    assert_eq!(ready.relay.reachable, Some(true));

    let uncovered = fold_trusted_team_wire(
        local.clone(),
        TeamReadinessWireObservation::Reached(TrustedTeamCatalogSnapshot {
            provenance: vec![wire::TeamReadinessCatalogProvenance {
                event_id: "d".repeat(64),
                channel_id: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
                signer_pubkey: "e".repeat(64),
                revision: 8,
            }],
            targets: vec!["claude-primary:claude-sonnet-5".into()],
            invalid_event_count: 0,
            conflict_count: 0,
        }),
    );
    assert_eq!(uncovered.status, TeamReadinessStatus::Unknown);
    assert!(!uncovered.ready);
    assert_eq!(uncovered.unknown_codes, vec!["CATALOG_TARGETS_UNCOVERED"]);

    let unreachable =
        fold_trusted_team_wire(local.clone(), TeamReadinessWireObservation::Unreachable);
    assert_eq!(unreachable.status, TeamReadinessStatus::Blocked);
    assert!(!unreachable.ready);

    let untrusted = fold_trusted_team_wire(
        local,
        TeamReadinessWireObservation::Unknown {
            code: "TRUST_CONFIG_INVALID".into(),
            summary: "Trusted signer metadata is invalid".into(),
            remedy: Some("Repair metadata, then re-read readiness".into()),
        },
    );
    assert_eq!(untrusted.status, TeamReadinessStatus::Unknown);
    assert_eq!(untrusted.catalog.revision, None);
    assert_eq!(untrusted.unknown_codes, vec!["TRUST_CONFIG_INVALID"]);
    assert_eq!(
        untrusted
            .facts
            .iter()
            .find(|fact| fact.code == "TRUST_CONFIG_INVALID")
            .and_then(|fact| fact.remedy.as_deref()),
        Some("Repair metadata, then re-read readiness")
    );
}

#[test]
fn registry_unreadable_is_limited_when_packs_come_from_a_project_source() {
    // Ledger 137: a project whose roles are staged from a kind:30624 packs
    // repository never reads team/model-registry.yaml at all, so its
    // absence is a limit on what readiness can say (each pack names its own
    // runtime and model), not a reason to refuse the session.
    let temp = tempfile::tempdir().expect("tempdir");
    let host = CountingHost::default();
    let mut gathered = Gathered::default();
    collect_runtimes_and_registry(&host, Some(temp.path()), true, &mut gathered);
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "REGISTRY_UNREADABLE")
        .expect("registry fact is present for an absent registry file");
    assert_eq!(fact.state, TeamReadinessFactState::Limited);
    assert!(
        fact.summary
            .contains("each role pack names its own runtime and model instead"),
        "{}",
        fact.summary
    );
}

#[test]
fn registry_unreadable_still_blocks_without_a_project_pack_source() {
    // The counterpart: a project with no packs repository still routes
    // every session through team/model-registry.yaml, so its absence stays
    // a hard blocker, unchanged by ledger 140.
    let temp = tempfile::tempdir().expect("tempdir");
    let host = CountingHost::default();
    let mut gathered = Gathered::default();
    collect_runtimes_and_registry(&host, Some(temp.path()), false, &mut gathered);
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "REGISTRY_UNREADABLE")
        .expect("registry fact is present for an absent registry file");
    assert_eq!(fact.state, TeamReadinessFactState::Blocked);
}

#[test]
fn catalog_coverage_is_not_applicable_when_no_local_registry_pins_a_target() {
    // Ledger 137 left CATALOG_TARGETS_UNCOVERED (Unknown) firing beside
    // REGISTRY_UNREADABLE (Limited) for a project whose packs come from a
    // project source and therefore hold no team/model-registry.yaml at all:
    // the coverage check had no registry to cover, but it computed
    // `covered_targets.is_empty()` from an equally empty `configured` list
    // and reported that as "uncovered" anyway. Fixed in ledger 140: an empty
    // local registry gets Limited coverage-not-applicable instead of a false
    // Unknown, because coverage for a project-sourced pack is judged per
    // role pack at hire time, not against a registry that was never there.
    let mut gathered = local_ready_gathered();
    gathered.registry = TeamReadinessRegistryCoverage::default();
    let local = finish(PROJECT_REF.into(), gathered);
    let response = fold_trusted_team_wire(
        local,
        TeamReadinessWireObservation::Reached(TrustedTeamCatalogSnapshot {
            provenance: vec![wire::TeamReadinessCatalogProvenance {
                event_id: "d".repeat(64),
                channel_id: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
                signer_pubkey: "e".repeat(64),
                revision: 8,
            }],
            targets: vec!["codex-primary:gpt-5.6-luna".into()],
            invalid_event_count: 0,
            conflict_count: 0,
        }),
    );
    assert!(!response
        .unknown_codes
        .contains(&"CATALOG_TARGETS_UNCOVERED".to_string()));
    assert!(response
        .limited_codes
        .contains(&"CATALOG_COVERAGE_NOT_APPLICABLE".to_string()));
    assert_eq!(response.registry.covered_targets, Vec::<String>::new());
    assert_eq!(response.registry.uncovered_targets, Vec::<String>::new());
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
fn readonly_trust_projection_never_creates_or_repairs_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("app-data/agents/global-agent-config.json");
    assert_eq!(
        super::host::load_allowed_bridge_pubkeys_readonly_from(&path),
        Ok(Vec::new())
    );
    assert_eq!(fs::read_dir(temp.path()).expect("root exists").count(), 0);

    fs::create_dir_all(path.parent().expect("parent")).expect("test fixture directory");
    fs::write(&path, br#"{"allowed-bridge-pubkeys":[{"pubkey":"short"}]}"#)
        .expect("malformed fixture");
    let before = fs::read(&path).expect("fixture bytes");
    assert!(super::host::load_allowed_bridge_pubkeys_readonly_from(&path).is_err());
    assert_eq!(fs::read(&path).expect("unchanged fixture"), before);
    assert_eq!(
        fs::read_dir(path.parent().expect("parent"))
            .expect("list")
            .map(|entry| entry.expect("entry").file_name())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsString::from("global-agent-config.json")],
        "readiness must not create a backup, lock, or repaired config"
    );
}

#[test]
fn malformed_trust_projection_becomes_structured_unknown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("global-agent-config.json");
    fs::write(&path, b"not-json").expect("fixture");
    let before = fs::read(&path).expect("before");
    let observation = wire::classify_trusted_signers(
        super::host::load_allowed_bridge_pubkeys_readonly_from(&path),
    )
    .expect_err("invalid metadata cannot yield trusted signers");
    assert_eq!(fs::read(&path).expect("after"), before);
    let response = fold_trusted_team_wire(
        finish(PROJECT_REF.into(), local_ready_gathered()),
        observation,
    );
    assert_eq!(response.status, TeamReadinessStatus::Unknown);
    assert_eq!(response.unknown_codes, vec!["TRUST_CONFIG_INVALID"]);
    assert!(!response.ready);
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
        r#"[{"pubkey":"not-a-pubkey","name":"Bob"}]"#.to_string(),
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

fn catalog_content(revision: u64, project_ref: &str, model: &str) -> String {
    let catalog = buzz_core_pkg::coding_session_catalog::Catalog {
        schema: buzz_core_pkg::coding_session_catalog::CATALOG_SCHEMA.into(),
        revision,
        providers: vec![buzz_core_pkg::coding_session_catalog::CatalogProvider {
            provider_instance_ref: "codex-primary".into(),
            driver: "codex-agent-acp".into(),
            runtime: "codex".into(),
            default_model: model.into(),
            allowed_models: vec![model.into()],
            capabilities: buzz_core_pkg::coding_session_payload::Capabilities::v1_baseline(),
            models: Vec::new(),
        }],
        projects: vec![buzz_core_pkg::coding_session_catalog::CatalogProject {
            project_ref: project_ref.into(),
            repo_ref: None,
            providers: vec!["codex-primary".into()],
        }],
    };
    buzz_core_pkg::coding_session_catalog::to_canonical_json(&catalog).expect("canonical catalog")
}
