//! A role's definition save keeps execution settings off the shared role (ledger 401).

/// The save the definition editor made in the fixture, through the real edit
/// rule: Codex and Sol 6.1 on the shared role. Refused, and an instructions
/// edit on the same role still saves.
#[test]
fn the_definition_save_refuses_codex_on_a_role_and_keeps_instruction_edits() {
    use super::prospective_persona_edit;
    use crate::managed_agents::role_source::tests::installed;
    let request = |runtime: Option<&str>, model: Option<&str>, prompt: &str| {
        serde_json::from_value::<crate::managed_agents::UpdatePersonaRequest>(serde_json::json!({
            "id": "crew-role:builder",
            "displayName": "builder",
            "avatarUrl": null,
            "systemPrompt": prompt,
            "runtime": runtime,
            "model": model,
        }))
        .expect("request")
    };
    let refusal = prospective_persona_edit(
        &installed(),
        &request(Some("codex"), Some("gpt-6.1-sol"), "You are builder."),
    )
    .unwrap_err();
    assert!(refusal.contains("Provider & settings"), "{refusal}");
    let saved = prospective_persona_edit(
        &installed(),
        &request(None, None, "You are a careful builder."),
    )
    .expect("instructions still edit");
    assert_eq!(saved.system_prompt, "You are a careful builder.");
}
