use super::*;
fn fixture() -> (
    tempfile::TempDir,
    nostr::Keys,
    CodingSessionProviderRecord,
    HostConfig,
) {
    let home = tempfile::tempdir().unwrap();
    let owner = nostr::Keys::generate();
    let record = crate::provision::mint_provider_record(&owner, "wss://relay.example").unwrap();
    let config=HostConfig {version:crate::config::HOST_CONFIG_VERSION,instance:Instance::Dev,
        relay_url:record.relay_url.clone(),provider_pubkey:record.provider_pubkey.clone(),
        session_provider_base_dir:home.path().join("operator/records"),provider_state_dir:home.path().join("operator/state"),
        runtimes:serde_json::from_value(serde_json::json!([{"instanceRef":"codex-primary","driver":"codex-acp","runtime":"codex",
            "agentCommand":"/operator/not-on-path/node","agentArgs":["/operator/codex/adapter.js"],"defaultModel":"chosen-model",
            "allowedModels":["chosen-model","other"],"discoverModels":false,"titleModel":null}])).unwrap(),
        max_sessions:Some(7),turn_idle_timeout_secs:Some(123),turn_budget:Some(91),provider_command:Some(home.path().join("bin/provider")),
        written_at:"2026-10-12T00:00:00Z".into()};
    crate::provision::write_commissioning(home.path(), &config, &record).unwrap();
    (home, owner, record, config)
}
#[test]
fn explicit_single_limit_patch_preserves_cli_commissioning_and_next_start_readback() {
    let (home, owner, record, original) = fixture();
    let key = std::fs::read(layout::host_key_file_path(home.path(), Instance::Dev)).unwrap();
    let patches = [
        LimitPatch::MaxSessions(Some(4)),
        LimitPatch::TurnIdleTimeout(Some(321)),
        LimitPatch::TurnBudget(None),
    ];
    let mut expected = original;
    for patch in patches {
        match patch {
            LimitPatch::MaxSessions(v) => expected.max_sessions = v,
            LimitPatch::TurnIdleTimeout(v) => expected.turn_idle_timeout_secs = v,
            LimitPatch::TurnBudget(v) => expected.turn_budget = v,
        }
        let patched = patch_host_limits(
            home.path(),
            Instance::Dev,
            &record,
            &owner.public_key(),
            patch,
        )
        .unwrap()
        .unwrap();
        expected.written_at = patched.written_at.clone();
        assert_eq!(patched, expected);
        // Host startup uses this same existing config loader, not desktop defaults.
        assert_eq!(
            HostConfig::load(&layout::host_config_path(home.path(), Instance::Dev)).unwrap(),
            Some(expected.clone())
        );
        assert_eq!(
            scoped_host_config(home.path(), Instance::Dev, &record, &owner.public_key()).unwrap(),
            Some(expected.clone())
        );
        assert_eq!(
            std::fs::read(layout::host_key_file_path(home.path(), Instance::Dev)).unwrap(),
            key
        );
    }
}
#[test]
fn foreign_scope_owner_and_invalid_limit_refuse_without_mutation() {
    let (home, owner, record, _) = fixture();
    let path = layout::host_config_path(home.path(), Instance::Dev);
    let before = std::fs::read(&path).unwrap();
    let patch = LimitPatch::MaxSessions(Some(1));
    let mut foreign = record.clone();
    foreign.relay_url = "wss://foreign.example".into();
    assert!(patch_host_limits(
        home.path(),
        Instance::Dev,
        &foreign,
        &owner.public_key(),
        patch
    )
    .is_err());
    let other = crate::provision::mint_provider_record(&owner, &record.relay_url).unwrap();
    assert!(patch_host_limits(
        home.path(),
        Instance::Dev,
        &other,
        &owner.public_key(),
        patch
    )
    .is_err());
    assert!(patch_host_limits(
        home.path(),
        Instance::Dev,
        &record,
        &nostr::Keys::generate().public_key(),
        patch
    )
    .is_err());
    assert!(patch_host_limits(
        home.path(),
        Instance::Dev,
        &record,
        &owner.public_key(),
        LimitPatch::TurnIdleTimeout(Some(0))
    )
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut wrong_instance = HostConfig::load(&path).unwrap().unwrap();
    wrong_instance.instance = Instance::Production;
    atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec(&wrong_instance).unwrap(),
    )
    .unwrap();
    let malformed_scope = std::fs::read(&path).unwrap();
    assert!(patch_host_limits(
        home.path(),
        Instance::Dev,
        &record,
        &owner.public_key(),
        patch
    )
    .is_err());
    assert_eq!(std::fs::read(path).unwrap(), malformed_scope);
}
#[test]
fn uncommissioned_limits_are_defaults_only_and_malformed_config_is_not_replaced() {
    let home = tempfile::tempdir().unwrap();
    let owner = nostr::Keys::generate();
    let record = crate::provision::mint_provider_record(&owner, "wss://relay.example").unwrap();
    assert!(patch_host_limits(
        home.path(),
        Instance::Dev,
        &record,
        &owner.public_key(),
        LimitPatch::MaxSessions(Some(0))
    )
    .unwrap()
    .is_none());
    let path = layout::host_config_path(home.path(), Instance::Dev);
    assert!(!path.exists());
    assert!(!layout::host_key_file_path(home.path(), Instance::Dev).exists());
    atomic_write::atomic_write_json_restricted(&path, b"broken").unwrap();
    assert!(patch_host_limits(
        home.path(),
        Instance::Dev,
        &record,
        &owner.public_key(),
        LimitPatch::MaxSessions(None)
    )
    .is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"broken");
}
