use super::*;
use beekeeper_host_core::{layout::Instance, provision::mint_provider_record};
use std::path::PathBuf;
fn config(record: &CodingSessionProviderRecord) -> HostConfig {
    HostConfig {
        version: HOST_CONFIG_VERSION, instance: Instance::Dev, relay_url: record.relay_url.clone(),
        provider_pubkey: record.provider_pubkey.clone(), session_provider_base_dir: PathBuf::from("/operator/records"),
        provider_state_dir: PathBuf::from("/operator/provider-state"),
        runtimes: serde_json::from_value(serde_json::json!([
            {"instanceRef":"codex-primary", "driver":"codex-acp", "runtime":"codex",
                "agentCommand":"/operator/not-on-desktop-path/node", "agentArgs":["/operator/codex/dist/index.js"],
                "defaultModel":"operator-model", "allowedModels":["operator-model","other-model"],
                "discoverModels":false, "titleModel":null, "steerIdleGuard":"promptRequired",
                "cliEnv":{"name":"CODEX_HOME","value":"/operator/codex-account"}},
            {"instanceRef":"custom-remote-compatible", "driver":"custom-acp", "runtime":"custom",
                "agentCommand":"custom-adapter-not-discoverable", "agentArgs":["--endpoint","https://configured.example"],
                "defaultModel":"custom-model", "allowedModels":["custom-model"], "discoverModels":false,
                "capabilities":beekeeper_core_pkg::coding_session_payload::Capabilities::v1_for_runtime("codex")}
        ])).unwrap(),
        max_sessions:Some(7), turn_idle_timeout_secs:Some(123), turn_budget:Some(91),
        provider_command:Some(PathBuf::from("/operator/bin/provider")), written_at:"2026-10-12T00:00:00Z".into(),
    }
}
#[test]
fn commissioned_codex_and_custom_flags_survive_boot_without_path_discovery() {
    let owner = nostr::Keys::generate();
    let record = mint_provider_record(&owner, "wss://relay.example").unwrap();
    let existing = config(&record);
    let mut proposed = existing.clone();
    proposed.session_provider_base_dir = PathBuf::from("/desktop/default-records");
    proposed.provider_state_dir = PathBuf::from("/desktop/default-state");
    proposed.runtimes.clear();
    proposed.provider_command = None;
    proposed.max_sessions = None;
    proposed.turn_budget = None;
    proposed.turn_idle_timeout_secs = None;
    proposed.written_at = "later".into();
    let selected = select(
        Some(existing.clone()),
        proposed,
        &record,
        &CodingSessionProviderStore::default(),
        Commissioning::FirstIdentity,
        || panic!("existing config must not discover PATH runtimes"),
    )
    .unwrap();
    assert_eq!(selected, existing);
}
#[test]
fn explicit_same_owner_rebind_changes_only_new_scope_and_retains_operator_runtimes() {
    let owner = nostr::Keys::generate();
    let old = mint_provider_record(&owner, "wss://old.example").unwrap();
    let new = mint_provider_record(&owner, "wss://new.example").unwrap();
    let existing = config(&old);
    let mut target = config(&new);
    target.provider_state_dir = PathBuf::from("/selected-new-provider/state");
    target.session_provider_base_dir = PathBuf::from("/selected-new-provider/records");
    target.runtimes.clear();
    target.provider_command = None;
    target.max_sessions = None;
    let mut store = CodingSessionProviderStore::default();
    store
        .providers
        .insert(canonical_relay_key(&old.relay_url), old);
    let chosen = select(
        Some(existing.clone()),
        target.clone(),
        &new,
        &store,
        Commissioning::RelayChanged,
        || panic!("explicit rebind must not rediscover commissioned runtimes"),
    )
    .unwrap();
    assert_eq!(chosen.runtimes, existing.runtimes);
    assert_eq!(chosen.provider_command, existing.provider_command);
    assert_eq!(chosen.max_sessions, existing.max_sessions);
    assert_eq!(chosen.turn_budget, existing.turn_budget);
    assert_eq!(
        chosen.turn_idle_timeout_secs,
        existing.turn_idle_timeout_secs
    );
    assert_eq!(chosen.provider_state_dir, target.provider_state_dir);
    assert_eq!(
        chosen.session_provider_base_dir,
        target.session_provider_base_dir
    );
    assert_eq!(chosen.provider_pubkey, new.provider_pubkey);
    assert_eq!(chosen.relay_url, new.relay_url);
}
#[test]
fn foreign_provider_owner_instance_or_malformed_config_refuses_without_discovery() {
    let owner = nostr::Keys::generate();
    let old = mint_provider_record(&owner, "wss://old.example").unwrap();
    let foreign = mint_provider_record(&nostr::Keys::generate(), "wss://new.example").unwrap();
    let existing = config(&old);
    let target = config(&foreign);
    let mut store = CodingSessionProviderStore::default();
    store
        .providers
        .insert(canonical_relay_key(&old.relay_url), old.clone());
    let no_discovery = || panic!("invalid existing config cannot trigger fallback");
    assert!(select(
        Some(existing.clone()),
        target.clone(),
        &foreign,
        &store,
        Commissioning::RelayChanged,
        no_discovery
    )
    .is_err());
    assert!(select(
        Some(existing.clone()),
        target,
        &foreign,
        &store,
        Commissioning::FirstIdentity,
        no_discovery
    )
    .is_err());
    let mut bad = existing.clone();
    bad.instance = Instance::Production;
    assert!(select(
        Some(bad),
        existing.clone(),
        &old,
        &store,
        Commissioning::FirstIdentity,
        no_discovery
    )
    .is_err());
    let mut bad = existing.clone();
    bad.provider_state_dir = PathBuf::from("relative");
    assert!(select(
        Some(bad),
        existing.clone(),
        &old,
        &store,
        Commissioning::FirstIdentity,
        no_discovery
    )
    .is_err());
    let mut bad = existing.clone();
    bad.runtimes[0].instance_ref.clear();
    assert!(select(
        Some(bad),
        existing,
        &old,
        &store,
        Commissioning::FirstIdentity,
        no_discovery
    )
    .is_err());
}
#[test]
fn missing_config_discovers_once_but_corrupt_or_public_file_never_falls_back() {
    let owner = nostr::Keys::generate();
    let record = mint_provider_record(&owner, "wss://relay.example").unwrap();
    let target = config(&record);
    let runtimes = target.runtimes.clone();
    let called = std::cell::Cell::new(0);
    let selected = select(
        None,
        target.clone(),
        &record,
        &CodingSessionProviderStore::default(),
        Commissioning::FirstIdentity,
        || {
            called.set(called.get() + 1);
            runtimes
        },
    )
    .unwrap();
    assert_eq!(called.get(), 1);
    assert_eq!(selected, target);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("host.json");
    assert!(load_existing(&path).unwrap().is_none());
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(&path, b"broken").unwrap();
    assert!(load_existing(&path).is_err());
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec(&selected).unwrap(),
    )
    .unwrap();
    assert_eq!(load_existing(&path).unwrap(), Some(selected));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_existing(&path).is_err());
    }
}
