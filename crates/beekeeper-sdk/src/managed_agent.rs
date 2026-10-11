//! Managed-agent public configuration builders shared by desktop and CLI.

use beekeeper_core::kind::KIND_MANAGED_AGENT;
use beekeeper_core::managed_agent::{
    validate_managed_agent_definition_text, ManagedAgentEventContent,
};
use nostr::{EventBuilder, Kind, PublicKey, Tag};

use crate::SdkError;

/// Build the existing owner-authored kind30177 public instance projection.
///
/// The caller signs and publishes; this builder holds no credentials. Private
/// custody/deployment fields are excluded by the input type, and definition
/// snapshots are slimmed through the canonical public projection.
pub fn build_managed_agent_event(
    agent_pubkey: &str,
    content: &ManagedAgentEventContent,
) -> Result<EventBuilder, SdkError> {
    PublicKey::from_hex(agent_pubkey).map_err(|_| {
        SdkError::InvalidInput("managed agent pubkey must be 64 hex chars".to_string())
    })?;
    validate_managed_agent_definition_text(
        &content.name,
        content.persona_id.as_deref(),
        content.system_prompt.as_deref(),
    )
    .map_err(|error| {
        SdkError::InvalidInput(format!(
            "Managed agent definition is unsafe to publish: {error}"
        ))
    })?;
    let content = serde_json::to_string(&content.public_projection()).map_err(|error| {
        SdkError::InvalidInput(format!(
            "failed to serialize managed-agent content: {error}"
        ))
    })?;
    let tag = Tag::parse(["d", agent_pubkey])
        .map_err(|error| SdkError::InvalidInput(format!("invalid d-tag: {error}")))?;
    Ok(EventBuilder::new(Kind::Custom(KIND_MANAGED_AGENT as u16), content).tags([tag]))
}

/// Build the existing kind9010 project membership operation.
///
/// The relay admits it only from the project's creator or roster owner. This
/// modifies one membership without rewriting the project's visibility or roster.
pub fn build_project_member_admission(
    project: &str,
    participant: &str,
    role: &str,
) -> Result<EventBuilder, SdkError> {
    let project = beekeeper_core::kind::normalize_project_coordinate(project)
        .ok_or_else(|| SdkError::InvalidInput("invalid project coordinate".into()))?;
    PublicKey::from_hex(participant)
        .map_err(|_| SdkError::InvalidInput("participant must be a hex public key".into()))?;
    if !beekeeper_core::kind::is_valid_project_role(role) {
        return Err(SdkError::InvalidInput(
            "invalid project membership role".into(),
        ));
    }
    let tags = [
        Tag::parse(["a", project.as_str()]),
        Tag::parse(["p", participant, "", role]),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| SdkError::InvalidInput(format!("invalid membership tag: {error}")))?;
    Ok(EventBuilder::new(
        Kind::Custom(beekeeper_core::kind::KIND_PROJECT_PUT_MEMBER as u16),
        "",
    )
    .tags(tags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_admission_keeps_existing_single_member_protocol() {
        let keys = nostr::Keys::generate();
        let participant = nostr::Keys::generate().public_key().to_hex();
        let project = format!("30621:{}:private", keys.public_key().to_hex());
        let event = build_project_member_admission(&project, &participant, "collaborator")
            .expect("membership")
            .sign_with_keys(&keys)
            .expect("signed");
        assert_eq!(event.kind.as_u16(), 9010);
        assert_eq!(
            event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect::<Vec<_>>(),
            vec![
                vec!["a".to_string(), project],
                vec![
                    "p".to_string(),
                    participant,
                    String::new(),
                    "collaborator".to_string()
                ]
            ]
        );
        assert!(build_project_member_admission(
            "invalid",
            &keys.public_key().to_hex(),
            "collaborator"
        )
        .is_err());
        assert!(build_project_member_admission(
            &format!("30621:{}:private", keys.public_key().to_hex()),
            &keys.public_key().to_hex(),
            "admin"
        )
        .is_err());
    }

    #[test]
    fn unsafe_instructions_refuse_publication_and_valid_content_keeps_existing_tags() {
        let keys = nostr::Keys::generate();
        let mut content: ManagedAgentEventContent = serde_json::from_value(serde_json::json!({
            "name":"Worker", "parallelism":1, "respond_to":"owner-only",
            "system_prompt":"unsafe\u{200b} instructions"
        }))
        .expect("content");
        assert!(build_managed_agent_event(&keys.public_key().to_hex(), &content).is_err());
        content.system_prompt = Some("Review changes.".to_string());
        let event = build_managed_agent_event(&keys.public_key().to_hex(), &content)
            .expect("builder")
            .sign_with_keys(&keys)
            .expect("sign");
        assert_eq!(event.kind.as_u16() as u32, KIND_MANAGED_AGENT);
        assert_eq!(event.tags.len(), 1);
        assert_eq!(
            event.tags.iter().next().expect("tag").as_slice(),
            &["d".to_string(), keys.public_key().to_hex()]
        );
    }
}
