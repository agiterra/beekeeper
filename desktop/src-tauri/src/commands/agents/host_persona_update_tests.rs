use super::*;
fn fixture() -> (
    ManagedAgentRecord,
    AgentDefinition,
    OwnedPersonaUpdateRequest,
) {
    let persona: AgentDefinition = serde_json::from_value(json!({"id":"owned","display_name":"Owned","avatar_url":null,"system_prompt":"original","runtime":"codex","is_builtin":false,"is_active":true,"shared":false,"created_at":"created","updated_at":"observed"})).unwrap();
    let mut record = persona.clone().into_agent_record();
    record.pubkey = "actor".into();
    record.persona_id = Some(persona.id.clone());
    record.persona_source_version =
        Some(crate::managed_agents::persona_events::persona_content_hash(
            &crate::managed_agents::persona_events::persona_event_content(&persona),
        ));
    let request=serde_json::from_value(json!({"expectedUpdatedAt":"observed","definition":{"id":"owned","displayName":"Owned","systemPrompt":"explicit new instructions","runtime":"codex"}})).unwrap();
    (record, persona, request)
}
#[test]
fn explicit_edit_stages_without_mutation_and_commits_exact_definition_with_same_actor() {
    let (record, persona, input) = fixture();
    let original = record.clone();
    let staged = StagedPersonaUpdate::prepare(
        &record,
        &input,
        std::slice::from_ref(&persona),
        std::slice::from_ref(&record),
    )
    .unwrap();
    assert_eq!(record, original);
    assert_eq!(persona.system_prompt, "original");
    let mut desired = record.clone();
    staged.apply_record(&mut desired);
    assert_eq!(desired.pubkey, original.pubkey);
    assert_eq!(desired.persona_id, original.persona_id);
    assert_ne!(
        desired.persona_source_version,
        original.persona_source_version
    );
    let mut personas = vec![persona];
    staged.commit_personas(&mut personas).unwrap();
    assert_eq!(personas[0].system_prompt, desired.system_prompt.unwrap());
}
#[test]
fn exact_after_commit_lost_reply_reuses_old_timestamp_but_changed_request_refuses() {
    let (record, persona, mut input) = fixture();
    let staged =
        StagedPersonaUpdate::prepare(&record, &input, &[persona], std::slice::from_ref(&record))
            .unwrap();
    let mut desired = record;
    staged.apply_record(&mut desired);
    let mut committed = staged.desired.clone();
    committed.updated_at = "after-commit".into();
    assert!(StagedPersonaUpdate::prepare(
        &desired,
        &input,
        std::slice::from_ref(&committed),
        std::slice::from_ref(&desired)
    )
    .is_ok());
    input.definition.system_prompt = "unseen changed request".into();
    assert!(StagedPersonaUpdate::prepare(
        &desired,
        &input,
        &[committed],
        std::slice::from_ref(&desired)
    )
    .is_err());
}
#[test]
fn original_partial_definition_edit_can_be_staged_explicitly_without_replacing_old_pin() {
    let (record, mut persona, mut input) = fixture();
    let old = record.persona_source_version.clone();
    persona.system_prompt = input.definition.system_prompt.clone();
    persona.updated_at = "retained-partial-save".into();
    input.expected_updated_at = persona.updated_at.clone();
    let staged =
        StagedPersonaUpdate::prepare(&record, &input, &[persona], std::slice::from_ref(&record))
            .unwrap();
    assert_eq!(record.persona_source_version, old);
    let mut desired = record;
    staged.apply_record(&mut desired);
    assert_ne!(desired.persona_source_version, old);
}
#[test]
fn foreign_shared_builtin_team_ambiguous_multi_actor_and_stale_sources_refuse() {
    let (record, persona, mut input) = fixture();
    let mut shared = persona.clone();
    shared.shared = true;
    assert!(StagedPersonaUpdate::prepare(
        &record,
        &input,
        &[shared],
        std::slice::from_ref(&record)
    )
    .is_err());
    let mut builtin = persona.clone();
    builtin.is_builtin = true;
    assert!(StagedPersonaUpdate::prepare(
        &record,
        &input,
        &[builtin],
        std::slice::from_ref(&record)
    )
    .is_err());
    let mut team = persona.clone();
    team.source_team = Some("team".into());
    assert!(
        StagedPersonaUpdate::prepare(&record, &input, &[team], std::slice::from_ref(&record))
            .is_err()
    );
    assert!(StagedPersonaUpdate::prepare(
        &record,
        &input,
        &[persona.clone(), persona.clone()],
        std::slice::from_ref(&record)
    )
    .is_err());
    let mut other = record.clone();
    other.pubkey = "other".into();
    assert!(StagedPersonaUpdate::prepare(
        &record,
        &input,
        std::slice::from_ref(&persona),
        &[record.clone(), other]
    )
    .is_err());
    input.expected_updated_at = "unseen".into();
    assert!(StagedPersonaUpdate::prepare(
        &record,
        &input,
        std::slice::from_ref(&persona),
        std::slice::from_ref(&record)
    )
    .is_err());
    input.expected_updated_at = persona.updated_at.clone();
    input.definition.id = "foreign".into();
    assert!(StagedPersonaUpdate::prepare(
        &record,
        &input,
        &[persona],
        std::slice::from_ref(&record)
    )
    .is_err());
}
#[test]
fn accepted_update_never_overwrites_a_changed_local_definition() {
    let (record, persona, input) = fixture();
    let staged = StagedPersonaUpdate::prepare(
        &record,
        &input,
        std::slice::from_ref(&persona),
        std::slice::from_ref(&record),
    )
    .unwrap();
    let mut changed = persona;
    changed
        .env_vars
        .insert("NEWER_PRIVATE_SETTING".into(), "retained".into());
    let mut current = vec![changed.clone()];
    assert!(staged
        .commit_personas(&mut current)
        .unwrap_err()
        .starts_with("Configuration accepted"));
    assert_eq!(
        serde_json::to_value(&current[0]).unwrap(),
        serde_json::to_value(changed).unwrap()
    );
}
