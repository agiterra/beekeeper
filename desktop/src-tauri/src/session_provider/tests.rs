//! Unit tests for the coding-session provider host.
//!
//! Everything here is `AppHandle`-free: the store round-trips through
//! `serde_json`, the env map is built from data, and the restart policy is a
//! pure function. The properties worth defending are the ones a reader of the
//! spawn code cannot verify by eye — the exact `BUZZ_AUTH_TAG` shape, and the
//! absence of the owner's secret from the child environment.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nostr::ToBech32;

use crate::managed_agents::{
    validate_global_config, AgentModelInfo, AgentModelsResponse, GlobalAgentConfig,
};
use crate::session_provider::canonical_relay_key;
use crate::session_provider::commands::{
    coding_session_provider_models_from_response, mint_provider_record,
    provider_command_relay_for_active,
};
use crate::session_provider::env::{
    build_provider_env, resolve_app_checkout, ProviderEnvInputs, DEFAULT_RUST_LOG,
    PROJECTS_FILE_NAME, SHARED_WORKDIRS_VAR,
};
use crate::session_provider::store::{
    CodingSessionProviderRecord, CodingSessionProviderStore, STORE_VERSION,
};
use crate::session_provider::supervisor::{
    parse_lock_owner_pid, plan_restart, RestartDecision, BASE_RESTART_DELAY,
    MAX_RESTARTS_PER_WINDOW, MAX_RESTART_DELAY, RESTART_WINDOW,
};
use crate::session_provider::trust::{append_allowed_bridge_pubkey, LOCAL_PROVIDER_LABEL};

const RELAY: &str = "wss://relay.example/";

#[test]
fn provider_mutations_honor_an_explicit_active_relay_pin() {
    assert_eq!(
        provider_command_relay_for_active(
            "wss://relay-a.example/",
            Some(" WSS://RELAY-A.EXAMPLE ")
        ),
        Ok("WSS://RELAY-A.EXAMPLE".to_string())
    );
    assert!(provider_command_relay_for_active(
        "wss://relay-b.example",
        Some("wss://relay-a.example")
    )
    .is_err());
    assert_eq!(
        provider_command_relay_for_active("wss://relay-b.example", None),
        Ok("wss://relay-b.example".to_string())
    );
}

#[test]
fn live_claude_models_preserve_adapter_order_and_current_default() {
    let models = coding_session_provider_models_from_response(
        "claude-primary",
        AgentModelsResponse {
            agent_name: "claude-agent-acp".to_string(),
            agent_version: "1".to_string(),
            models: ["default", "opus[1m]", "sonnet", "haiku"]
                .into_iter()
                .map(|id| AgentModelInfo {
                    id: id.to_string(),
                    name: None,
                    description: None,
                })
                .collect(),
            agent_default_model: Some("sonnet".to_string()),
            selected_model: None,
            supports_switching: true,
        },
        "claude",
    )
    .expect("model response");

    assert_eq!(models.instance_ref, "claude-primary");
    assert_eq!(models.default_model, "sonnet");
    assert_eq!(
        models.allowed_models,
        vec!["default", "opus[1m]", "sonnet", "haiku"]
    );
}

#[test]
fn live_claude_models_fall_back_to_the_first_adapter_option() {
    let models = coding_session_provider_models_from_response(
        "claude-primary",
        AgentModelsResponse {
            agent_name: "claude-agent-acp".to_string(),
            agent_version: "1".to_string(),
            models: vec![AgentModelInfo {
                id: "default".to_string(),
                name: None,
                description: None,
            }],
            agent_default_model: Some("missing".to_string()),
            selected_model: None,
            supports_switching: true,
        },
        "claude",
    )
    .expect("model response");

    assert_eq!(models.default_model, "default");
}

fn sample_record() -> CodingSessionProviderRecord {
    CodingSessionProviderRecord {
        provider_pubkey: "a".repeat(64),
        instance_id: "a".repeat(16),
        auth_tag: Some(r#"["auth","0011","","beef"]"#.to_string()),
        created_at: "2026-08-11T00:00:00Z".to_string(),
        relay_url: RELAY.to_string(),
        private_key_nsec: "nsec1provider".to_string(),
    }
}

// ── record store ─────────────────────────────────────────────────────────────

#[test]
fn store_round_trips_through_json() {
    let mut store = CodingSessionProviderStore::default();
    store.upsert(RELAY, sample_record());

    let json = serde_json::to_string_pretty(&store).expect("serialize");
    let parsed: CodingSessionProviderStore = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed, store);
    assert_eq!(parsed.version, STORE_VERSION);
    let record = parsed.get(RELAY).expect("record for relay");
    assert_eq!(record.instance_id, "a".repeat(16));
}

/// The wire shape is camelCase and matches what the TS wrapper reads. A silent
/// rename here would leave an already-provisioned desktop looking
/// un-provisioned, which mints a second identity.
#[test]
fn store_serializes_camel_case_keys() {
    let mut store = CodingSessionProviderStore::default();
    store.upsert(RELAY, sample_record());
    let json = serde_json::to_string(&store).expect("serialize");

    for key in [
        "\"providerPubkey\"",
        "\"instanceId\"",
        "\"authTag\"",
        "\"createdAt\"",
        "\"relayUrl\"",
    ] {
        assert!(json.contains(key), "missing {key} in {json}");
    }
}

/// An empty nsec is the keyring-backed steady state and must not be written to
/// disk as an empty string — `skip_serializing_if` is what keeps the key out of
/// the JSON entirely.
#[test]
fn store_omits_a_blank_inline_key() {
    let mut record = sample_record();
    record.private_key_nsec.clear();
    let mut store = CodingSessionProviderStore::default();
    store.upsert(RELAY, record);

    let json = serde_json::to_string(&store).expect("serialize");
    assert!(!json.contains("privateKeyNsec"), "{json}");
}

/// Relay keys are normalized so `wss://R/` and `wss://r` are one identity, not
/// two.
#[test]
fn relay_keys_are_normalized() {
    assert_eq!(
        canonical_relay_key("  WSS://Relay.Example/  "),
        "wss://relay.example"
    );
    let mut store = CodingSessionProviderStore::default();
    store.upsert("wss://Relay.Example/", sample_record());
    assert!(store.get("wss://relay.example").is_some());
}

/// The three session settings round-trip through the record file, and an
/// unset one stays absent rather than being written as a null.
///
/// Absence is load-bearing: `None` means "the provider's own default", and a
/// stored `0` means "no limit". A serializer that wrote `null` for the first
/// would make the two indistinguishable to anything reading the file by hand,
/// and a change to the provider's default would silently not apply.
#[test]
fn store_round_trips_the_session_settings_and_omits_unset_ones() {
    let mut store = CodingSessionProviderStore::default();
    store.upsert(RELAY, sample_record());
    let json = serde_json::to_string(&store).expect("serialize");
    for key in ["maxSessions", "turnIdleTimeoutSecs", "turnBudget"] {
        assert!(!json.contains(key), "unset {key} must be absent: {json}");
    }

    store.max_sessions = Some(9);
    store.turn_idle_timeout_secs = Some(3_600);
    store.turn_budget = Some(50);
    let json = serde_json::to_string_pretty(&store).expect("serialize");
    assert!(json.contains("\"turnBudget\""), "{json}");
    let parsed: CodingSessionProviderStore = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, store);
    assert_eq!(parsed.turn_budget, Some(50));

    // Zero is "no budget", a choice, and survives as one.
    store.turn_budget = Some(0);
    let parsed: CodingSessionProviderStore =
        serde_json::from_str(&serde_json::to_string(&store).expect("serialize"))
            .expect("deserialize");
    assert_eq!(parsed.turn_budget, Some(0));
}

/// A config file written before this feature existed must still load, and must
/// not resurrect as a different provider.
#[test]
fn legacy_store_without_optional_fields_loads() {
    let json = r#"{
        "version": 1,
        "providers": {
            "wss://relay.example": {
                "providerPubkey": "bb",
                "instanceId": "bb",
                "createdAt": "2026-01-01T00:00:00Z"
            }
        }
    }"#;
    let store: CodingSessionProviderStore = serde_json::from_str(json).expect("deserialize");
    let record = store.get("wss://relay.example").expect("record");
    assert_eq!(record.auth_tag, None);
    assert!(record.private_key_nsec.is_empty());
    assert!(record.relay_url.is_empty());
    // Including the settings added since: absent means "the provider's own
    // default", never a locally invented number.
    assert_eq!(store.max_sessions, None);
    assert_eq!(store.turn_idle_timeout_secs, None);
    assert_eq!(store.turn_budget, None);
}

// ── env assembly ─────────────────────────────────────────────────────────────

fn sample_runtimes() -> Vec<buzz_core_pkg::coding_session_runtime::RuntimeDescriptor> {
    use buzz_core_pkg::coding_session_runtime::{CliEnvVar, RuntimeDescriptor};
    vec![
        RuntimeDescriptor {
            instance_ref: "claude-primary".into(),
            driver: "claude-agent-acp".into(),
            runtime: "claude".into(),
            agent_command: "/opt/buzz/bin/claude-agent-acp".into(),
            agent_args: Vec::new(),
            cli_env: Some(CliEnvVar {
                name: "CLAUDE_CODE_EXECUTABLE".into(),
                value: "/usr/local/bin/claude".into(),
            }),
            default_model: "default".into(),
            allowed_models: vec!["default".into()],
            discover_models: true,
            capabilities: None,
        },
        RuntimeDescriptor {
            instance_ref: "goose-primary".into(),
            driver: "goose-acp".into(),
            runtime: "goose".into(),
            agent_command: "/opt/homebrew/bin/goose".into(),
            agent_args: vec!["acp".into()],
            cli_env: None,
            default_model: "default".into(),
            allowed_models: vec!["default".into()],
            discover_models: false,
            capabilities: None,
        },
    ]
}

fn env_for(record: &CodingSessionProviderRecord) -> BTreeMap<String, String> {
    build_provider_env(&ProviderEnvInputs {
        record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: Some(PathBuf::from("/opt/buzz/bin/claude-agent-acp")),
        context_mcp_command: Some(PathBuf::from("/opt/buzz/bin/buzz-dev-mcp")),
        claude_code_executable: Some(PathBuf::from("/usr/local/bin/claude")),
        runtimes: sample_runtimes(),
        augmented_path: Some("/opt/buzz/bin:/usr/bin".into()),
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    })
}

#[test]
fn env_carries_the_required_provider_contract() {
    let record = sample_record();
    let env = env_for(&record);

    assert_eq!(
        env.get("BUZZ_PRIVATE_KEY").map(String::as_str),
        Some("nsec1provider")
    );
    assert_eq!(env.get("BUZZ_RELAY_URL").map(String::as_str), Some(RELAY));
    assert_eq!(
        env.get("BUZZ_CSP_STATE_DIR").map(String::as_str),
        Some("/tmp/session-provider/aaaa")
    );
    assert_eq!(
        env.get("BUZZ_CSP_PROJECTS_FILE").map(String::as_str),
        Some(
            Path::new("/tmp/session-provider/aaaa")
                .join(PROJECTS_FILE_NAME)
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        env.get("BUZZ_CSP_INSTANCE_ID").map(String::as_str),
        Some(record.instance_id.as_str())
    );
    assert_eq!(
        env.get("BUZZ_CSP_AGENT_COMMAND").map(String::as_str),
        Some("/opt/buzz/bin/claude-agent-acp")
    );
    assert_eq!(
        env.get("BUZZ_CSP_CONTEXT_MCP_COMMAND").map(String::as_str),
        Some("/opt/buzz/bin/buzz-dev-mcp")
    );
    assert_eq!(
        env.get("CLAUDE_CODE_EXECUTABLE").map(String::as_str),
        Some("/usr/local/bin/claude")
    );
    assert!(env.contains_key("RUST_LOG"));
    // The provider's adapters are `env node` shims; the augmented PATH is how
    // they find `node` when the desktop was launched with a bare GUI PATH.
    assert_eq!(
        env.get("PATH").map(String::as_str),
        Some("/opt/buzz/bin:/usr/bin")
    );
}

/// `BUZZ_CSP_RUNTIMES` must round-trip through the exact parser the sidecar
/// uses — the env map is the real interface, so this is the drift check.
#[test]
fn env_carries_a_parseable_runtime_list() {
    let env = env_for(&sample_record());
    let raw = env.get("BUZZ_CSP_RUNTIMES").expect("runtimes in env");
    let parsed = buzz_core_pkg::coding_session_runtime::parse_runtime_descriptors(raw)
        .expect("the sidecar parser must accept what the host writes");
    assert_eq!(parsed, sample_runtimes());
    // The legacy variables stay exported alongside the list, so an older
    // sidecar binary keeps working under a newer desktop.
    assert!(env.contains_key("BUZZ_CSP_AGENT_COMMAND"));
    assert!(env.contains_key("CLAUDE_CODE_EXECUTABLE"));
}

/// An empty runtime list writes no variable at all: the sidecar treats absence
/// as "synthesize the legacy claude default", while an empty JSON array would
/// be a startup error.
#[test]
fn env_omits_an_empty_runtime_list() {
    let record = sample_record();
    let env = build_provider_env(&ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    });
    assert!(!env.contains_key("BUZZ_CSP_RUNTIMES"));
    // Without an augmented PATH the child inherits the process PATH unchanged.
    assert!(!env.contains_key("PATH"));
}

/// `BUZZ_AUTH_TAG` is parsed by the provider as a JSON array of strings and
/// handed straight to `nostr::Tag::parse`. Any re-encoding (bare signature,
/// object form, comma-joined) makes the tag fail to verify at AUTH time, which
/// surfaces only as a relay rejection at runtime.
#[test]
fn auth_tag_is_a_json_array_of_strings() {
    let owner = nostr::Keys::generate();
    let record = mint_provider_record(&owner, RELAY).expect("mint");
    let env = env_for(&record);

    let raw = env.get("BUZZ_AUTH_TAG").expect("auth tag in env");
    let parts: Vec<String> = serde_json::from_str(raw).expect("auth tag must be a JSON array");
    assert_eq!(parts.len(), 4, "{parts:?}");
    assert_eq!(parts[0], "auth");
    assert_eq!(parts[1], owner.public_key().to_hex());
    assert_eq!(parts[2], "", "conditions must stay empty");
    nostr::Tag::parse(parts).expect("must parse as a nostr tag");
}

/// A minted tag must verify against the provider pubkey it was minted for —
/// this is the property the relay checks, and the one a hex/bech32 mix-up in
/// the record would break.
#[test]
fn minted_auth_tag_verifies_against_the_provider_pubkey() {
    let owner = nostr::Keys::generate();
    let record = mint_provider_record(&owner, RELAY).expect("mint");
    let auth_tag = record.auth_tag.clone().expect("auth tag");
    let provider_pubkey =
        nostr::PublicKey::from_hex(&record.provider_pubkey).expect("provider pubkey");

    let resolved = buzz_sdk_pkg::nip_oa::verify_auth_tag(&auth_tag, &provider_pubkey)
        .expect("auth tag must verify");
    assert_eq!(resolved, owner.public_key());
}

/// The owner's key authorizes the provider; it must never *be* the provider.
/// Nothing in the child environment may carry it in any encoding.
#[test]
fn child_env_never_carries_the_owner_secret() {
    let owner = nostr::Keys::generate();
    let record = mint_provider_record(&owner, RELAY).expect("mint");
    let env = env_for(&record);

    let owner_hex = owner.secret_key().to_secret_hex();
    let owner_nsec = owner.secret_key().to_bech32().expect("owner nsec");
    for (key, value) in &env {
        assert!(
            !value.contains(&owner_hex) && !value.contains(&owner_nsec),
            "owner secret key leaked into {key}"
        );
    }

    // Positive control: the provider's own key IS present, so the assertion
    // above is checking encoding, not an empty environment.
    let provider_nsec = env.get("BUZZ_PRIVATE_KEY").expect("provider key");
    assert!(provider_nsec.starts_with("nsec1"), "{provider_nsec}");
    assert_ne!(provider_nsec, &owner_nsec);
}

/// A record with no attestation must produce no variable at all, so the child's
/// config sees "absent" rather than an empty string it would reject.
#[test]
fn env_omits_the_auth_tag_when_unattested() {
    let mut record = sample_record();
    record.auth_tag = None;
    assert!(!env_for(&record).contains_key("BUZZ_AUTH_TAG"));
}

/// The provider derives its own instance id from the same pubkey prefix length.
/// A drift here splits one provider across two `cs-target` identities, which
/// consumers treat as two different producers.
#[test]
fn instance_id_matches_the_provider_derivation() {
    let owner = nostr::Keys::generate();
    let record = mint_provider_record(&owner, RELAY).expect("mint");
    assert_eq!(record.instance_id.len(), 16);
    assert!(record.provider_pubkey.starts_with(&record.instance_id));
}

// ── restart policy ───────────────────────────────────────────────────────────

fn delay_of(decision: RestartDecision) -> Duration {
    match decision {
        RestartDecision::Retry { delay, .. } => delay,
        RestartDecision::GiveUp => panic!("expected a retry"),
    }
}

#[test]
fn backoff_doubles_from_the_base_delay() {
    let fresh = Duration::from_secs(1);
    assert_eq!(delay_of(plan_restart(0, fresh)), BASE_RESTART_DELAY);
    assert_eq!(delay_of(plan_restart(1, fresh)), BASE_RESTART_DELAY * 2);
    assert_eq!(delay_of(plan_restart(2, fresh)), BASE_RESTART_DELAY * 4);
    assert_eq!(delay_of(plan_restart(3, fresh)), BASE_RESTART_DELAY * 8);
}

#[test]
fn backoff_is_capped() {
    let fresh = Duration::from_secs(1);
    for failures in 0..MAX_RESTARTS_PER_WINDOW {
        assert!(delay_of(plan_restart(failures, fresh)) <= MAX_RESTART_DELAY);
    }
}

#[test]
fn supervision_gives_up_after_the_window_budget() {
    let fresh = Duration::from_secs(1);
    assert_eq!(
        plan_restart(MAX_RESTARTS_PER_WINDOW, fresh),
        RestartDecision::GiveUp
    );
}

/// An aged-out window is what stops a provider that crashes once a day from
/// eventually being abandoned.
#[test]
fn an_expired_window_resets_the_failure_count() {
    let expired = RESTART_WINDOW + Duration::from_secs(1);
    assert_eq!(
        plan_restart(MAX_RESTARTS_PER_WINDOW, expired),
        RestartDecision::Retry {
            delay: BASE_RESTART_DELAY,
            failures: 1,
        }
    );
}

/// Lock-file contents that are not exactly one plausible pid must never
/// become a kill target: the takeover path signals whatever pid this returns.
#[test]
fn lock_owner_pid_parsing_rejects_garbage_and_system_pids() {
    assert_eq!(parse_lock_owner_pid("4242\n"), Some(4242));
    assert_eq!(parse_lock_owner_pid("  4242  "), Some(4242));
    assert_eq!(parse_lock_owner_pid(""), None);
    assert_eq!(parse_lock_owner_pid("not-a-pid"), None);
    assert_eq!(parse_lock_owner_pid("-7"), None);
    assert_eq!(parse_lock_owner_pid("0"), None, "never signal pid 0");
    assert_eq!(parse_lock_owner_pid("1"), None, "never signal launchd/init");
}

// ── trust seeding ────────────────────────────────────────────────────────────

#[test]
fn appending_a_bridge_pubkey_is_idempotent() {
    let pubkey = "b".repeat(64);
    let mut config = GlobalAgentConfig::default();

    assert!(append_allowed_bridge_pubkey(
        &mut config,
        &pubkey,
        LOCAL_PROVIDER_LABEL
    ));
    assert!(!append_allowed_bridge_pubkey(
        &mut config,
        &pubkey,
        LOCAL_PROVIDER_LABEL
    ));
    assert_eq!(config.allowed_bridge_pubkeys.len(), 1);
    assert_eq!(config.allowed_bridge_pubkeys[0].pubkey, pubkey);
    assert_eq!(config.allowed_bridge_pubkeys[0].label, LOCAL_PROVIDER_LABEL);
    assert!(validate_global_config(&config).is_ok());
}

/// A hand-edited uppercase entry already grants trust. Appending a lowercase
/// twin would produce a config `validate_global_config` refuses to accept,
/// making every later settings save fail.
#[test]
fn appending_matches_case_insensitively() {
    let mut config = GlobalAgentConfig::default();
    assert!(append_allowed_bridge_pubkey(
        &mut config,
        &"C".repeat(64),
        "manual"
    ));
    assert!(!append_allowed_bridge_pubkey(
        &mut config,
        &"c".repeat(64),
        LOCAL_PROVIDER_LABEL
    ));
    assert_eq!(config.allowed_bridge_pubkeys.len(), 1);
}

#[test]
fn appending_preserves_existing_entries() {
    let existing = "d".repeat(64);
    let mut config = GlobalAgentConfig {
        allowed_bridge_pubkeys: vec![crate::managed_agents::AllowedBridgePubkey {
            pubkey: existing.clone(),
            label: "someone else".to_string(),
        }],
        ..Default::default()
    };
    assert!(append_allowed_bridge_pubkey(
        &mut config,
        &"e".repeat(64),
        LOCAL_PROVIDER_LABEL
    ));
    assert_eq!(config.allowed_bridge_pubkeys.len(), 2);
    assert_eq!(config.allowed_bridge_pubkeys[0].pubkey, existing);
    assert!(validate_global_config(&config).is_ok());
}

#[test]
fn appending_rejects_a_blank_pubkey() {
    let mut config = GlobalAgentConfig::default();
    assert!(!append_allowed_bridge_pubkey(&mut config, "   ", "label"));
    assert!(config.allowed_bridge_pubkeys.is_empty());
}

/// The new field must not change how a pre-existing config file deserializes,
/// and must not appear as a surprise in a config that never set it.
#[test]
fn global_config_without_the_allowlist_still_loads() {
    let json = r#"{"env_vars":{"A":"b"},"provider":null,"model":null,"preferred_runtime":null}"#;
    let config: GlobalAgentConfig = serde_json::from_str(json).expect("deserialize");
    assert!(config.allowed_bridge_pubkeys.is_empty());
}

/// The persisted key is kebab-case, matching the donor wire shape.
#[test]
fn global_config_serializes_the_allowlist_under_the_donor_key() {
    let mut config = GlobalAgentConfig::default();
    append_allowed_bridge_pubkey(&mut config, &"f".repeat(64), LOCAL_PROVIDER_LABEL);
    let json = serde_json::to_string(&config).expect("serialize");
    assert!(json.contains("\"allowed-bridge-pubkeys\""), "{json}");
}

/// §2 item 39 — every Codex execution rendered as `Codex · default` because
/// this runtime was declared with `discover_models: false` and a placeholder
/// model. codex-acp answers the same probe claude-agent-acp does (verified
/// 2026-08-24 against codex-acp 1.6.2), so the placeholder was hiding a real
/// model list, which is the "default label hiding the real model" bug.
#[test]
fn codex_opts_into_live_model_discovery_and_goose_does_not() {
    use crate::session_provider::runtimes::known_instance_ref;
    assert_eq!(known_instance_ref("claude-primary"), Some(true));
    assert_eq!(known_instance_ref("codex-primary"), Some(true));
    // goose answers the probe with `-32603 Internal error`; opting it in would
    // spend the discovery timeout to learn nothing.
    assert_eq!(known_instance_ref("goose-primary"), Some(false));
    assert_eq!(known_instance_ref("ghost-primary"), None);
}

/// A probe that resolved `claude-agent-acp` whatever it was asked about would
/// report Claude's models under Codex's label.
#[test]
fn each_runtime_is_probed_through_its_own_adapter() {
    use crate::session_provider::runtimes::runtime_probe_target;
    let unknown = runtime_probe_target("ghost-primary").expect_err("unknown ref must fail");
    assert!(unknown.contains("ghost-primary"));

    match runtime_probe_target("codex-primary") {
        Ok(target) => {
            assert_eq!(target.label, "codex");
            assert!(!target.needs_claude_executable);
            assert!(
                target.agent_command.to_string_lossy().contains("codex-acp"),
                "codex must be probed through codex-acp, not through Claude's adapter"
            );
        }
        // Not installed on this host: the message must name the adapter that is
        // missing rather than Claude's.
        Err(message) => assert!(message.contains("codex")),
    }

    match runtime_probe_target("goose-primary") {
        Ok(target) => {
            assert_eq!(target.label, "goose");
            assert_eq!(target.agent_args, vec!["acp".to_string()]);
        }
        Err(message) => assert!(message.contains("goose")),
    }
}

/// The adapter's own label reaches the error, so an empty Codex list does not
/// blame Claude.
#[test]
fn an_empty_model_list_names_the_runtime_that_returned_it() {
    let error = coding_session_provider_models_from_response(
        "codex-primary",
        AgentModelsResponse {
            agent_name: "codex-acp".to_string(),
            agent_version: "1.6.2".to_string(),
            models: Vec::new(),
            agent_default_model: None,
            selected_model: None,
            supports_switching: true,
        },
        "codex",
    )
    .expect_err("no models is an error");
    assert!(error.contains("codex"), "{error}");
    assert!(!error.contains("Claude"), "{error}");
}

/// The real shape codex-acp returned on 2026-08-24, through the same decoder.
#[test]
fn codex_model_ids_survive_the_response_decoder() {
    let models = coding_session_provider_models_from_response(
        "codex-primary",
        AgentModelsResponse {
            agent_name: "@agentclientprotocol/codex-acp".to_string(),
            agent_version: "1.6.2".to_string(),
            models: ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]
                .into_iter()
                .map(|id| AgentModelInfo {
                    id: id.to_string(),
                    name: None,
                    description: None,
                })
                .collect(),
            agent_default_model: Some("gpt-5.6-terra".to_string()),
            selected_model: None,
            supports_switching: true,
        },
        "codex",
    )
    .expect("model response");

    assert_eq!(models.default_model, "gpt-5.6-terra");
    assert!(!models.allowed_models.contains(&"default".to_string()));
}

/// The person's ceiling has to reach the child, and an unset one must stay
/// unset — exporting a number equal to the provider's default would make a
/// later change to that default silently not apply (asked for 2026-08-24).
/// The debug switch has to survive the trip from the host's environment into
/// the child, and has to be absent — not `false` — when nobody asked. A
/// provider that never asked must send no `emitRawSDKMessages` key at all.
#[test]
fn env_exports_the_raw_frame_switch_only_when_it_is_asked_for() {
    let record = sample_record();
    let base = |emit_raw_sdk_frames| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames,
        turn_budget: None,
        app_checkout: None,
    };

    assert!(!build_provider_env(&base(false)).contains_key("BUZZ_CSP_EMIT_RAW_SDK_FRAMES"));
    assert_eq!(
        build_provider_env(&base(true))
            .get("BUZZ_CSP_EMIT_RAW_SDK_FRAMES")
            .map(String::as_str),
        Some("true")
    );
}

/// `RUST_LOG` used to be set unconditionally, which silently discarded a
/// filter chosen for a debugging session — the one moment anybody cares what
/// the child logs. It is a default now, and the host's choice wins.
#[test]
fn a_chosen_log_filter_beats_the_default() {
    let record = sample_record();
    let base = |rust_log: Option<String>| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    };

    assert_eq!(
        build_provider_env(&base(None))
            .get("RUST_LOG")
            .map(String::as_str),
        Some(DEFAULT_RUST_LOG),
        "an unset filter keeps the quiet default"
    );
    assert_eq!(
        build_provider_env(&base(Some("debug,acp::sdk_frame=trace".into())))
            .get("RUST_LOG")
            .map(String::as_str),
        Some("debug,acp::sdk_frame=trace"),
        "a filter chosen for a debugging session must reach the child"
    );
}

#[test]
fn env_exports_the_session_ceiling_only_when_one_is_chosen() {
    let record = sample_record();
    let base = |max_sessions| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    };

    assert!(!build_provider_env(&base(None)).contains_key("BUZZ_CSP_MAX_SESSIONS"));
    assert_eq!(
        build_provider_env(&base(Some(9)))
            .get("BUZZ_CSP_MAX_SESSIONS")
            .map(String::as_str),
        Some("9")
    );
    // Zero is unlimited, not "unset": it must be exported like any other choice.
    assert_eq!(
        build_provider_env(&base(Some(0)))
            .get("BUZZ_CSP_MAX_SESSIONS")
            .map(String::as_str),
        Some("0")
    );
}

/// Same contract for the per-turn silence budget: two of Andy's turns died as
/// "no agent activity for 900s" while a long command ran, so the number is the
/// person's — and an unset one must stay unset.
#[test]
fn env_exports_the_turn_idle_timeout_only_when_one_is_chosen() {
    let record = sample_record();
    let base = |turn_idle_timeout_secs| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    };

    assert!(!build_provider_env(&base(None)).contains_key("BUZZ_CSP_IDLE_TIMEOUT"));
    assert_eq!(
        build_provider_env(&base(Some(3_600)))
            .get("BUZZ_CSP_IDLE_TIMEOUT")
            .map(String::as_str),
        Some("3600")
    );
}

/// And the same contract again for the crew turn budget (D9). Zero is the
/// spelling of "no budget" and is a choice, so it is exported; unset is not.
#[test]
fn env_exports_the_crew_turn_budget_only_when_one_is_chosen() {
    let record = sample_record();
    let base = |turn_budget| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget,
        app_checkout: None,
    };

    assert!(!build_provider_env(&base(None)).contains_key("BUZZ_CSP_TURN_BUDGET"));
    assert_eq!(
        build_provider_env(&base(Some(50)))
            .get("BUZZ_CSP_TURN_BUDGET")
            .map(String::as_str),
        Some("50")
    );
    assert_eq!(
        build_provider_env(&base(Some(0)))
            .get("BUZZ_CSP_TURN_BUDGET")
            .map(String::as_str),
        Some("0")
    );
}

/// Item 87(d), found live 2026-08-28 21:2x. A Team launch seated its lead in
/// the checkout this app was running from — the operator's own hot tree — and
/// the provider had no way to know that directory was special. The provider
/// owns the refusal (`session::seated_workdir_refusal`); only the host knows
/// which directory it is, so the host hands it down.
#[test]
fn the_checkout_the_app_runs_from_is_handed_to_the_provider() {
    let record = sample_record();
    let base = |app_checkout: Option<PathBuf>| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout,
    };

    assert_eq!(
        build_provider_env(&base(Some(PathBuf::from(
            "/Users/b/Projects/beekeeper/beekeeper"
        ))))
        .get(SHARED_WORKDIRS_VAR)
        .map(String::as_str),
        Some("/Users/b/Projects/beekeeper/beekeeper")
    );
    // A bundled app launched from Finder resolves no checkout. Absent, not
    // empty: an empty list would read as "the host looked and found nothing
    // shared", which is a different claim.
    assert!(!build_provider_env(&base(None)).contains_key(SHARED_WORKDIRS_VAR));
}

/// The resolver walks up from the process's working directory to the
/// repository that contains it, and refuses to call a home directory a
/// checkout.
#[test]
fn an_app_checkout_is_the_repository_the_process_is_running_inside() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let repo = tmp.path().join("Projects/beekeeper/beekeeper");
    std::fs::create_dir_all(repo.join(".git")).expect("repo");
    let inner = repo.join("desktop/src-tauri");
    std::fs::create_dir_all(&inner).expect("inner");

    assert_eq!(
        resolve_app_checkout(&inner, Some(tmp.path())),
        Some(repo.clone())
    );
    assert_eq!(resolve_app_checkout(&repo, Some(tmp.path())), Some(repo));

    // Outside any repository — a bundled app's `/` — there is nothing to name.
    let bare = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&bare).expect("bare");
    assert_eq!(resolve_app_checkout(&bare, Some(tmp.path())), None);

    // A home directory that happens to be a git repository is still the
    // operator's home, and the provider already refuses that by name.
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".git")).expect("home repo");
    assert_eq!(resolve_app_checkout(&home, Some(&home)), None);
}
