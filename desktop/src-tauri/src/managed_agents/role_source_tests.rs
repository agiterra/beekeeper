//! The live C4 failure (ledger 400): Codex and Sol 6.1 were saved on the
//! shared `builder` role, the agent stayed on Claude, and nothing could bring
//! the pin back.

use std::path::PathBuf;

use super::*;
use crate::managed_agents::crew_roles::role_pack_source_version;

const NOW: &str = "2026-10-28T00:00:00Z";

fn pack() -> DiscoveredRolePack {
    DiscoveredRolePack {
        dir: PathBuf::from("/packs/builder"),
        persona_name: "builder".into(),
        display_name: "builder".into(),
        role: "builder".into(),
        system_prompt: "You are builder.".into(),
        runtime: None,
        model: None,
        provider: None,
        avatar_url: None,
    }
}

/// The definition exactly as the installer writes it for `pack()`.
pub(crate) fn installed() -> AgentDefinition {
    AgentDefinition {
        id: "crew-role:builder".into(),
        display_name: "builder".into(),
        avatar_url: None,
        system_prompt: "You are builder.".into(),
        runtime: None,
        model: None,
        provider: None,
        name_pool: Vec::new(),
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: Some("team".into()),
        source_team_persona_slug: Some("builder".into()),
        catalog_source: None,
        env_vars: Default::default(),
        respond_to: None,
        respond_to_allowlist: Vec::new(),
        parallelism: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

/// What the definition editor saved in the fixture.
fn edited_execution() -> AgentDefinition {
    let mut definition = installed();
    definition.runtime = Some("codex".into());
    definition.model = Some("gpt-6.1-sol".into());
    definition.respond_to = Some("owner-only".into());
    definition.parallelism = Some(10);
    definition
}

fn agent_pinned_to(definition: &AgentDefinition) -> ManagedAgentRecord {
    let mut record = definition.clone().into_agent_record();
    record.persona_id = Some(definition.id.clone());
    record.persona_source_version = Some(persona_content_hash(&persona_event_content(definition)));
    record
}

#[test]
fn a_role_definition_refuses_execution_and_behaviour_edits() {
    let source = installed();
    assert!(refuse_role_execution_edit(&source, &edited_execution()).is_err());
    for edit in [
        |d: &mut AgentDefinition| d.runtime = Some("codex".into()),
        |d: &mut AgentDefinition| d.model = Some("gpt-6.1-sol".into()),
        |d: &mut AgentDefinition| d.provider = Some("openai".into()),
        |d: &mut AgentDefinition| d.respond_to = Some("owner-only".into()),
        |d: &mut AgentDefinition| d.parallelism = Some(10),
    ] {
        let mut prospective = source.clone();
        edit(&mut prospective);
        let refusal = refuse_role_execution_edit(&source, &prospective).unwrap_err();
        assert!(refusal.contains("Provider & settings"));
    }
    let mut instructions = source.clone();
    instructions.system_prompt = "You are a careful builder.".into();
    assert!(refuse_role_execution_edit(&source, &instructions).is_ok());

    let mut standalone = source.clone();
    standalone.id = "my-agent".into();
    assert!(refuse_role_execution_edit(&standalone, &{
        let mut d = standalone.clone();
        d.runtime = Some("codex".into());
        d
    })
    .is_ok());
}

#[test]
fn source_state_tells_missing_current_and_diverged_apart() {
    let definition = installed();
    let mut agent = agent_pinned_to(&definition);
    assert_eq!(
        source_state(&agent, Some(&definition)),
        Some(SourceState::Current)
    );
    assert_eq!(
        source_state(&agent, Some(&edited_execution())),
        Some(SourceState::Diverged)
    );
    agent.persona_source_version = None;
    assert_eq!(
        source_state(&agent, Some(&definition)),
        Some(SourceState::Missing)
    );
    assert_eq!(source_state(&agent, None), None);
}

#[test]
fn reapply_restores_the_pack_moves_execution_to_the_agent_and_repins() {
    let plan =
        plan_reapply(&edited_execution(), &pack(), false, NOW).expect("no instruction edits");
    assert_eq!(plan.pin, role_pack_source_version(&pack(), "builder"));
    let restored_agent = agent_pinned_to(&plan.definition);
    assert_eq!(
        source_state(&restored_agent, Some(&plan.definition)),
        Some(SourceState::Current)
    );
    assert_eq!(
        plan.moved,
        MovedExecution {
            runtime: Some("codex".into()),
            model: Some("gpt-6.1-sol".into()),
            provider: None,
        }
    );
    assert_eq!(plan.cleared, vec!["who can instruct", "parallelism"]);
    assert!(plan.replaced.is_empty());
    assert_eq!(plan.definition.id, "crew-role:builder");
    assert_eq!(plan.definition.source_team.as_deref(), Some("team"));
    assert_eq!(plan.definition.created_at, NOW);
}

#[test]
fn reapply_never_discards_edited_instructions_without_the_explicit_confirm() {
    let mut edited = edited_execution();
    edited.system_prompt = "You are a careful builder.".into();
    let refusal = plan_reapply(&edited, &pack(), false, NOW).unwrap_err();
    assert!(refusal.starts_with(INSTRUCTIONS_EDITED));
    assert!(refusal.contains("instructions"));

    let plan = plan_reapply(&edited, &pack(), true, NOW).expect("confirmed");
    assert_eq!(plan.replaced, vec!["instructions"]);
    assert_eq!(plan.definition.system_prompt, "You are builder.");
}

#[test]
fn an_unchanged_or_unpinned_role_reapplies_to_the_same_pin_with_nothing_moved() {
    let plan = plan_reapply(&installed(), &pack(), false, NOW).expect("clean");
    assert!(plan.moved.is_empty());
    assert!(plan.cleared.is_empty());
    assert_eq!(plan.pin, role_pack_source_version(&pack(), "builder"));
}

#[test]
fn only_role_definitions_are_reapplied() {
    let mut other = installed();
    other.id = "my-agent".into();
    assert!(plan_reapply(&other, &pack(), true, NOW).is_err());
}
