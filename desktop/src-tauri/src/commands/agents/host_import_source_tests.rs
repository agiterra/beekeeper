use super::*;
fn fixture() -> (
    ManagedAgentRecord,
    ManagedAgentRecord,
    PrivateConfiguration,
    PrivateConfiguration,
    Vec<AgentDefinition>,
) {
    let definition: AgentDefinition = serde_json::from_value(json!({
        "id":"source", "display_name":"Source", "system_prompt":"exact instructions",
        "name_pool":[], "created_at":"2026-10-11T00:00:00Z", "updated_at":"2026-10-11T00:00:00Z"
    }))
    .unwrap();
    let mut source = definition.into_agent_record();
    source.pubkey = "ab".repeat(32);
    source.slug = None;
    source.private_key_nsec = "protected-source-custody".into();
    source.auth_tag = Some("protected-auth".into());
    source.agent_command = "codex-acp".into();
    source.mcp_command = String::new();
    let mut view = source.clone();
    view.slug = Some(source.pubkey.clone());
    view.definition_respond_to = Some(source.respond_to.as_str().into());
    view.definition_respond_to_allowlist = source.respond_to_allowlist.clone();
    view.definition_parallelism = Some(source.parallelism);
    let manufactured = view.to_definition_view().unwrap();
    let mut current = source.clone();
    current.persona_id = Some(source.pubkey.clone());
    current.persona_source_version =
        Some(persona_content_hash(&persona_event_content(&manufactured)));
    current.mcp_command = "beekeeper-dev-mcp".into();
    let private = |record: &ManagedAgentRecord| {
        let mut value = serde_json::to_value(record).unwrap();
        value["effective_command"] = json!("codex-acp");
        value["effective_mcp_command"] = json!("beekeeper-dev-mcp");
        PrivateConfiguration { record: value }
    };
    let before = private(&source);
    let after = private(&current);
    (source, current, before, after, vec![manufactured])
}
#[test]
fn positive_backfill_and_catalog_preserves_protected_effective_configuration() {
    let (source, current, before, after, personas) = fixture();
    assert!(unchanged(&source, &current, &before, &after, &personas));
    let bytes = serde_json::to_vec(&before.record).unwrap();
    assert!(unchanged(&source, &current, &before, &after, &personas));
    assert_eq!(serde_json::to_vec(&before.record).unwrap(), bytes);
}
#[test]
fn negative_hash_prompt_env_access_runtime_mcp_association_custody() {
    let (source, current, before, after, personas) = fixture();
    let mut bad = current.clone();
    bad.persona_source_version = Some("foreign-hash".into());
    assert!(!unchanged(&source, &bad, &before, &after, &personas));
    let mut edited = personas.clone();
    edited[0].system_prompt = "changed definition".into();
    assert!(!unchanged(&source, &current, &before, &after, &edited));
    let mut bad = current.clone();
    bad.private_key_nsec = "different custody".into();
    assert!(!unchanged(&source, &bad, &before, &after, &personas));
    bad = current.clone();
    bad.auth_tag = Some("different grant".into());
    assert!(!unchanged(&source, &bad, &before, &after, &personas));
    bad = current.clone();
    bad.mcp_command = "custom-command".into();
    assert!(!unchanged(&source, &bad, &before, &after, &personas));
    for (key, value) in [
        ("system_prompt", json!("changed instructions")),
        ("env_vars", json!({"TOKEN":"different"})),
        ("respond_to", json!("anyone")),
        ("effective_command", json!("claude-agent-acp")),
        ("effective_mcp_command", json!("other-server")),
        ("project_ref", json!("foreign-project")),
    ] {
        let mut changed = PrivateConfiguration {
            record: after.record.clone(),
        };
        changed.record[key] = value;
        assert!(
            !unchanged(&source, &current, &before, &changed, &personas),
            "{key}"
        );
    }
    assert!(!unchanged(&source, &current, &before, &after, &[]));
    let mut duplicate = personas.clone();
    duplicate.extend(personas.clone());
    assert!(!unchanged(&source, &current, &before, &after, &duplicate));
}

#[test]
fn revision_repair_requires_proven_backfill_and_both_exact_owner_signed_heads() {
    let (source, current, before, after, personas) = fixture();
    assert!(proven_backfill(
        &source, &current, &before, &after, &personas
    ));
    assert!(!proven_backfill(&source, &source, &before, &before, &[]));
    let owner = Keys::generate();
    let sign = |record: &ManagedAgentRecord, keys: &Keys| {
        crate::managed_agents::agent_events::build_agent_event(record)
            .unwrap()
            .sign_with_keys(keys)
            .unwrap()
    };
    let old = sign(&source, &owner);
    let new = sign(&current, &owner);
    assert!(verified_equivalent_heads(
        &source,
        &current,
        &old,
        &new,
        &owner.public_key()
    ));
    let foreign = sign(&current, &Keys::generate());
    assert!(!verified_equivalent_heads(
        &source,
        &current,
        &old,
        &foreign,
        &owner.public_key()
    ));
    let mut arbitrary = current.clone();
    arbitrary.respond_to = beekeeper_core_pkg::managed_agent::RespondTo::Anyone;
    let changed = sign(&arbitrary, &owner);
    assert!(!verified_equivalent_heads(
        &source,
        &current,
        &old,
        &changed,
        &owner.public_key()
    ));
    let mut changed_source = source.clone();
    changed_source.system_prompt = Some("other instructions".into());
    let changed = sign(&changed_source, &owner);
    assert!(!verified_equivalent_heads(
        &source,
        &current,
        &changed,
        &new,
        &owner.public_key()
    ));
    let mut tampered = old.clone();
    tampered.content.push(' ');
    assert!(!verified_equivalent_heads(
        &source,
        &current,
        &tampered,
        &new,
        &owner.public_key()
    ));
    let unknown = nostr::EventBuilder::new(nostr::Kind::Custom(30177),
        serde_json::json!({"name":current.name,"persona_id":current.persona_id,"parallelism":current.parallelism,"respond_to":"owner-only","unrecognized":"behavior"}).to_string())
        .tags([nostr::Tag::identifier(current.pubkey.clone())]).sign_with_keys(&owner).unwrap();
    assert!(!verified_equivalent_heads(
        &source,
        &current,
        &old,
        &unknown,
        &owner.public_key()
    ));
}
