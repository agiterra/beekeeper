//! Pre-spawn store writes: `resnapshot_linked_record` (item 90).

use std::collections::BTreeMap;

use super::resnapshot_linked_record;
use crate::managed_agents::{AgentDefinition, BackendKind, ManagedAgentRecord, RespondTo};

fn record(
    persona_id: Option<&str>,
    model: Option<&str>,
    provider: Option<&str>,
) -> ManagedAgentRecord {
    ManagedAgentRecord {
        reserves_name_globally: false,
        pubkey: "agent".to_string(),
        name: "Agent".to_string(),
        persona_id: persona_id.map(str::to_string),
        private_key_nsec: String::new(),
        auth_tag: None,
        relay_url: "ws://localhost:3000".to_string(),
        avatar_url: None,
        acp_command: "beekeeper-acp".to_string(),
        agent_command: "goose".to_string(),
        agent_command_override: None,
        agent_args: vec![],
        mcp_command: String::new(),
        turn_timeout_seconds: 300,
        idle_timeout_seconds: None,
        max_turn_duration_seconds: None,
        parallelism: 1,
        system_prompt: None,
        model: model.map(str::to_string),
        provider: provider.map(str::to_string),
        persona_source_version: None,
        env_vars: BTreeMap::new(),
        start_on_app_launch: false,
        auto_restart_on_config_change: true,
        runtime_pid: None,
        backend: BackendKind::Local,
        backend_agent_id: None,
        provider_policy_pending: false,
        provider_binary_path: None,
        team_id: None,
        persona_team_dir: None,
        persona_name_in_team: None,
        home_role: None,
        project_ref: None,
        project_public: None,
        carried_project_digest: None,
        project_publication_withdrawn: false,
        created_at: "now".to_string(),
        updated_at: "now".to_string(),
        last_started_at: None,
        last_stopped_at: None,
        last_exit_code: None,
        last_error: None,
        last_error_code: None,
        respond_to: RespondTo::OwnerOnly,
        respond_to_allowlist: vec![],
        display_name: None,
        slug: None,
        runtime: None,
        name_pool: Vec::new(),
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        definition_respond_to: None,
        definition_respond_to_allowlist: Vec::new(),
        definition_parallelism: None,
        relay_mesh: None,
    }
}

fn definition(id: &str, model: Option<&str>, provider: Option<&str>) -> AgentDefinition {
    AgentDefinition {
        id: id.to_string(),
        display_name: "Test Persona".to_string(),
        avatar_url: None,
        system_prompt: String::new(),
        runtime: None,
        model: model.map(str::to_string),
        provider: provider.map(str::to_string),
        name_pool: vec![],
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        env_vars: BTreeMap::new(),
        respond_to: None,
        respond_to_allowlist: vec![],
        parallelism: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

// ── Pre-spawn store writes (item 90) ────────────────────────────────────────

/// The pair-restart path writes the whole store BEFORE the spawn it is about
/// to attempt. On 2026-08-29 a dark-wake start rewrote every record even though
/// nothing about them had moved. A re-pin that changes nothing must report so,
/// and the caller must skip the write.
#[test]
fn resnapshot_reports_no_change_when_the_repin_is_a_noop() {
    let persona = definition("p1", Some("claude-opus-4"), Some("anthropic"));
    let mut record = record(Some("p1"), None, None);

    assert!(
        resnapshot_linked_record(&mut record, std::slice::from_ref(&persona)),
        "the first re-pin materializes the definition and must report a change"
    );
    assert!(
        !resnapshot_linked_record(&mut record, std::slice::from_ref(&persona)),
        "a second re-pin changes nothing and must not ask the caller to save"
    );
}

/// A record with no linked definition is never re-pinned, so it never asks for
/// a write either.
#[test]
fn resnapshot_reports_no_change_for_a_definition_less_record() {
    let mut record = record(None, Some("gpt-5.6-sol"), Some("openai"));
    assert!(!resnapshot_linked_record(&mut record, &[]));
    assert_eq!(record.model.as_deref(), Some("gpt-5.6-sol"));
}

/// An orphan (linked definition missing) is left exactly as it is — the spawn
/// path refuses it downstream; the store must not be rewritten on its way there.
#[test]
fn resnapshot_reports_no_change_for_an_orphaned_record() {
    let mut record = record(Some("gone"), Some("gpt-5.6-sol"), None);
    let before = record.clone();
    assert!(!resnapshot_linked_record(&mut record, &[]));
    assert_eq!(record, before);
}
