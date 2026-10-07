use super::*;

fn provider_record(deployed: bool) -> ManagedAgentRecord {
    let mut record: ManagedAgentRecord = serde_json::from_value(serde_json::json!({
        "pubkey": "agent", "name": "Agent", "relay_url": "", "acp_command": "",
        "agent_command": "", "agent_args": [], "mcp_command": "",
        "turn_timeout_seconds": 0, "system_prompt": null, "created_at": "",
        "updated_at": "", "last_started_at": null, "last_stopped_at": null,
        "last_exit_code": null, "last_error": null
    }))
    .unwrap();
    record.backend = crate::managed_agents::BackendKind::Provider {
        id: "provider".into(),
        config: serde_json::json!({}),
    };
    record.backend_agent_id = deployed.then(|| "deployment".to_string());
    record
}

#[test]
fn deployed_provider_rejects_access_edits_that_cannot_be_revoked() {
    let error = ensure_access_policy_change_supported(&provider_record(true), true)
        .expect_err("deployed provider access edit must fail closed");
    assert!(error.contains("no explicit stop or revocation acknowledgement"));
}

#[test]
fn undeployed_provider_accepts_access_edits() {
    ensure_access_policy_change_supported(&provider_record(false), true)
        .expect("no running provider deployment can retain stale access");
}

// ---------------------------------------------------------------------------
// Host-owned model/provider on a linked instance (item 90)
// ---------------------------------------------------------------------------

/// Model and provider are HOST-owned identity facts, so the picker must reach
/// a persona-linked record too. `system_prompt` stays pack-owned and is still
/// refused. Before this fix the whole update was silently dropped for every
/// team identity: the control rendered, saved, and did nothing.
#[test]
fn linked_instance_accepts_model_provider_but_not_prompt_writes() {
    let mut record: crate::managed_agents::ManagedAgentRecord = serde_json::from_str(
        r#"{
            "pubkey": "linked1",
            "name": "linked-agent",
            "persona_id": "p1",
            "private_key_nsec": "nsec1fake",
            "relay_url": "wss://localhost:3000",
            "acp_command": "beekeeper-acp",
            "agent_command": "goose",
            "agent_args": [],
            "mcp_command": "",
            "turn_timeout_seconds": 320,
            "system_prompt": null,
            "model": null,
            "provider": null,
            "env_vars": {},
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "last_started_at": null,
            "last_stopped_at": null,
            "last_exit_code": null,
            "last_error": null
        }"#,
    )
    .expect("linked agent record");

    assert!(
        record.persona_id.is_some(),
        "test setup: record must be linked"
    );

    crate::commands::agent_models::apply_model_provider_prompt_update(
        &mut record,
        Some(Some("gpt-5.6-sol".to_string())),
        Some(Some("openai".to_string())),
        Some(Some("explicit-prompt".to_string())),
        Some(Some("codex".to_string())),
    )
    .unwrap();

    assert_eq!(
        record.model.as_deref(),
        Some("gpt-5.6-sol"),
        "a linked record's model is host-owned and must be written"
    );
    assert_eq!(record.provider.as_deref(), Some("openai"));
    assert_eq!(
        record.runtime.as_deref(),
        Some("codex"),
        "a linked record's runtime is host-owned and must be written"
    );
    assert!(
        record.system_prompt.is_none(),
        "system_prompt stays pack-owned and must not be written on a linked record"
    );
}

/// Clearing is host intent too: an explicit null returns the field to
/// definition/global inheritance instead of being ignored.
#[test]
fn linked_instance_accepts_explicit_model_provider_runtime_clear() {
    let mut record: crate::managed_agents::ManagedAgentRecord = serde_json::from_str(
        r#"{
            "pubkey": "linked2",
            "name": "linked-agent-2",
            "persona_id": "p1",
            "private_key_nsec": "nsec1fake",
            "relay_url": "wss://localhost:3000",
            "acp_command": "beekeeper-acp",
            "agent_command": "goose",
            "agent_args": [],
            "mcp_command": "",
            "turn_timeout_seconds": 320,
            "system_prompt": null,
            "model": "gpt-5.6-sol",
            "provider": "openai",
            "runtime": "codex",
            "env_vars": {},
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "last_started_at": null,
            "last_stopped_at": null,
            "last_exit_code": null,
            "last_error": null
        }"#,
    )
    .expect("linked agent record");

    crate::commands::agent_models::apply_model_provider_prompt_update(
        &mut record,
        Some(None),
        Some(None),
        None,
        Some(None),
    )
    .unwrap();

    assert!(record.model.is_none());
    assert!(record.provider.is_none());
    assert!(record.runtime.is_none());
}

/// An absent field is still "don't touch" for a linked record.
#[test]
fn linked_instance_absent_fields_leave_host_values_intact() {
    let mut record: crate::managed_agents::ManagedAgentRecord = serde_json::from_str(
        r#"{
            "pubkey": "linked3",
            "name": "linked-agent-3",
            "persona_id": "p1",
            "private_key_nsec": "nsec1fake",
            "relay_url": "wss://localhost:3000",
            "acp_command": "beekeeper-acp",
            "agent_command": "goose",
            "agent_args": [],
            "mcp_command": "",
            "turn_timeout_seconds": 320,
            "system_prompt": null,
            "model": "gpt-5.6-sol",
            "provider": "openai",
            "runtime": "codex",
            "env_vars": {},
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "last_started_at": null,
            "last_stopped_at": null,
            "last_exit_code": null,
            "last_error": null
        }"#,
    )
    .expect("linked agent record");

    crate::commands::agent_models::apply_model_provider_prompt_update(
        &mut record,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(record.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(record.provider.as_deref(), Some("openai"));
    assert_eq!(record.runtime.as_deref(), Some("codex"));
}
