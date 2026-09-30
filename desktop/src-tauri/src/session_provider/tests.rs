//! Unit tests for the desktop's half of the coding-session provider host.
//!
//! Everything here is `AppHandle`-free. The record-file and environment
//! assertions moved to `beekeeper_host_core::contract_tests` along with the
//! code they cover — that contract is now shared with `beekeeper-host`, and the
//! properties belong to the contract rather than to one launcher.
//!
//! What remains is what is genuinely the desktop's: minting an identity (which
//! is what commissioning means, and only the desktop does it), the supervisor's
//! restart policy, trust seeding, and the model/runtime probes behind the
//! picker. The minting tests deliberately run their record through the shared
//! env builder — that is a claim about this app's wiring, not about the
//! builder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nostr::ToBech32;

use crate::managed_agents::{
    validate_global_config, AgentModelInfo, AgentModelsResponse, GlobalAgentConfig,
};
use crate::session_provider::commands::{
    coding_session_provider_models_from_response, mint_provider_record,
    provider_command_relay_for_active,
};
// Imported from the shared crate directly: the app no longer builds this map
// for real, and these tests assert its *minting* against the one builder both
// launchers use.
use crate::session_provider::store::CodingSessionProviderRecord;
use crate::session_provider::trust::{append_allowed_bridge_pubkey, LOCAL_PROVIDER_LABEL};
use beekeeper_host_core::env::{build_provider_env, ProviderEnvInputs};

pub(super) const RELAY: &str = "wss://relay.example/";

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

// ── record store ─────────────────────────────────────────────────────────────

// ── env assembly ─────────────────────────────────────────────────────────────

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

/// The minted record, through the shared env builder.
///
/// A local copy rather than an import: the version in
/// `beekeeper_host_core::contract_tests` belongs to that crate's own tests,
/// and a test helper shared across a crate boundary is a dependency the
/// contract should not carry.
fn env_for(record: &CodingSessionProviderRecord) -> BTreeMap<String, String> {
    build_provider_env(&ProviderEnvInputs {
        record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: Some(PathBuf::from("/opt/buzz/bin/claude-agent-acp")),
        context_mcp_command: Some(PathBuf::from("/opt/buzz/bin/buzz-dev-mcp")),
        claude_code_executable: Some(PathBuf::from("/usr/local/bin/claude")),
        runtimes: Vec::new(),
        augmented_path: Some("/opt/buzz/bin:/usr/bin".into()),
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    })
}

// The restart policy and the lock-owner parsing moved to `beekeeper-host` with
// the supervisor: `restart_policy.rs` (which now also refuses to revive a
// clean `exit(0)`, per `docs/remote-agents.md` § I5) and `takeover.rs`.
// The app has no restart ladder to test — it does not start the provider.

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
