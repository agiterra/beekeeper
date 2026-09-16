use crate::managed_agents::runtime::test_fixtures::fixture;
use crate::managed_agents::types::RespondTo;
use crate::managed_agents::AgentDefinition;

fn persona(model: Option<&str>, provider: Option<&str>) -> AgentDefinition {
    AgentDefinition {
        id: "p".to_string(),
        display_name: "Persona".to_string(),
        avatar_url: None,
        system_prompt: "prompt".to_string(),
        runtime: None,
        model: model.map(str::to_string),
        provider: provider.map(str::to_string),
        name_pool: Vec::new(),
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        env_vars: Default::default(),
        respond_to: None,
        respond_to_allowlist: Vec::new(),
        parallelism: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// The summary the Agents card reads must report the model this host pinned on
/// the record — and say so in `model_source` — even though the identity is
/// linked to a definition that names its own model (item 90).
#[test]
fn summary_reports_host_set_model_and_instance_source_for_linked_record() {
    let definition = persona(Some("claude-opus-4-6"), Some("anthropic"));
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    record.persona_id = Some(definition.id.clone());
    // What the picker writes: a host pick made after the last snapshot apply.
    record.model = Some("gpt-5.6-sol".to_string());
    record.provider = Some("databricks_v2".to_string());

    let (model, provider, prompt, model_source) = super::summary_effective_fields(
        &record,
        std::slice::from_ref(&definition),
        &Default::default(),
    );

    assert_eq!(model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(provider.as_deref(), Some("databricks_v2"));
    assert_eq!(
        model_source,
        Some(crate::managed_agents::effective_config::ConfigSource::Instance),
        "the card must be told the host set this model, not the pack"
    );
    // The pack still owns the prompt.
    assert_eq!(prompt.as_deref(), Some("prompt"));
}

/// Same shape with no host pick: the definition's model still wins and the
/// source still says `definition`, so the card renders the pack's model.
#[test]
fn summary_reports_definition_model_when_record_has_no_host_pick() {
    let definition = persona(Some("claude-opus-4-6"), Some("anthropic"));
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    record.persona_id = Some(definition.id.clone());
    record.model = None;
    record.provider = None;

    let (model, _provider, _prompt, model_source) = super::summary_effective_fields(
        &record,
        std::slice::from_ref(&definition),
        &Default::default(),
    );

    assert_eq!(model.as_deref(), Some("claude-opus-4-6"));
    assert_eq!(
        model_source,
        Some(crate::managed_agents::effective_config::ConfigSource::Definition)
    );
}

/// An agent whose record pins no runtime at all inherits the harness from its
/// linked persona. `summary_effective_runtime` must report that inherited
/// value — the raw `record.runtime` stays `None` and is a different field
/// (`ManagedAgentSummary.runtime`); the summary's `effective_runtime` is the
/// one a hire and the project Agents tab must read (ledger 135(b), 136(b),
/// 139).
#[test]
fn summary_reports_inherited_runtime_and_definition_source() {
    let mut definition = persona(Some("claude-opus-4-6"), Some("anthropic"));
    definition.runtime = Some("codex".to_string());
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    record.persona_id = Some(definition.id.clone());
    record.runtime = None;

    let (runtime, source) = super::summary_effective_runtime(
        &record,
        std::slice::from_ref(&definition),
        &Default::default(),
    );

    assert_eq!(
        runtime.as_deref(),
        Some("codex"),
        "the effective runtime must be the inherited harness, not absent"
    );
    assert_eq!(
        source,
        Some(crate::managed_agents::effective_config::ConfigSource::Definition),
        "the source must name the tier the runtime actually came from"
    );
}

/// A record that pins its own runtime wins over the persona's, same as
/// model/provider (item 90) — and the source says `instance`.
#[test]
fn summary_reports_host_pinned_runtime_and_instance_source() {
    let mut definition = persona(Some("claude-opus-4-6"), Some("anthropic"));
    definition.runtime = Some("codex".to_string());
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    record.persona_id = Some(definition.id.clone());
    record.runtime = Some("claude".to_string());

    let (runtime, source) = super::summary_effective_runtime(
        &record,
        std::slice::from_ref(&definition),
        &Default::default(),
    );

    assert_eq!(runtime.as_deref(), Some("claude"));
    assert_eq!(
        source,
        Some(crate::managed_agents::effective_config::ConfigSource::Instance)
    );
}
