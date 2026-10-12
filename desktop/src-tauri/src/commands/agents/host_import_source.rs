//! Exact source CAS with a bounded proof of the existing B5 representation fold.
use super::*;
use crate::managed_agents::{
    persona_events::{persona_content_hash, persona_event_content},
    AgentDefinition,
};

pub(super) fn unchanged(
    source: &ManagedAgentRecord,
    current: &ManagedAgentRecord,
    protected: &PrivateConfiguration,
    observed: &PrivateConfiguration,
    personas: &[AgentDefinition],
) -> bool {
    if current.private_key_nsec != source.private_key_nsec || current.auth_tag != source.auth_tag {
        return false;
    }
    let source_public = crate::managed_agents::agent_events::agent_event_content(source);
    let current_public = crate::managed_agents::agent_events::agent_event_content(current);
    if source_public == current_public && protected.record == observed.record {
        return true;
    }
    // c34 migration/backfill.rs102–124: only standalone -> self-slug link,
    // with the definition manufactured from precisely these source settings.
    if source.persona_id.is_some()
        || source.persona_source_version.is_some()
        || current.persona_id.as_deref() != Some(source.pubkey.as_str())
    {
        return false;
    }
    let mut view_source = source.clone();
    view_source.slug = Some(source.pubkey.clone());
    view_source.definition_respond_to = Some(source.respond_to.as_str().to_string());
    view_source.definition_respond_to_allowlist = source.respond_to_allowlist.clone();
    view_source.definition_parallelism = Some(source.parallelism);
    let Some(expected) = view_source.to_definition_view() else {
        return false;
    };
    let expected_content = persona_event_content(&expected);
    let expected_hash = persona_content_hash(&expected_content);
    if current.persona_source_version.as_deref() != Some(expected_hash.as_str()) {
        return false;
    }
    let mut matching = personas
        .iter()
        .filter(|definition| definition.id == source.pubkey);
    let Some(actual) = matching.next() else {
        return false;
    };
    if matching.next().is_some() || persona_event_content(actual) != expected_content {
        return false;
    }
    // c34 migration.rs665–719 replaces only these obsolete stored MCP values.
    // The protected effective selection must already have the same command.
    if current.mcp_command != source.mcp_command {
        if !matches!(
            source.mcp_command.as_str(),
            "" | "buzz-mcp-server" | "buzz-dev-mcp"
        ) {
            return false;
        }
        let Some(command) = protected
            .record
            .get("effective_command")
            .and_then(Value::as_str)
        else {
            return false;
        };
        let Some(runtime) = crate::managed_agents::known_acp_runtime(command) else {
            return false;
        };
        let canonical = runtime.mcp_command.unwrap_or("");
        if current.mcp_command != canonical
            || protected.record.get("effective_mcp_command")
                != observed.record.get("effective_mcp_command")
            || protected
                .record
                .get("effective_mcp_command")
                .and_then(Value::as_str)
                != Some(canonical)
        {
            return false;
        }
    }
    let mut normalized = current.clone();
    normalized.persona_id = source.persona_id.clone();
    normalized.persona_source_version = source.persona_source_version.clone();
    normalized.mcp_command = source.mcp_command.clone();
    if crate::managed_agents::agent_events::agent_event_content(&normalized) != source_public {
        return false;
    }
    let mut private = observed.record.clone();
    let Some(object) = private.as_object_mut() else {
        return false;
    };
    for key in ["persona_id", "persona_source_version", "mcp_command"] {
        match protected.record.get(key) {
            Some(value) => {
                object.insert(key.into(), value.clone());
            }
            None => {
                object.remove(key);
            }
        }
    }
    private == protected.record
}

// A revision repair must not be authorized by the exact-unchanged fastpath.
pub(super) fn proven_backfill(
    source: &ManagedAgentRecord,
    current: &ManagedAgentRecord,
    protected: &PrivateConfiguration,
    observed: &PrivateConfiguration,
    personas: &[AgentDefinition],
) -> bool {
    !(crate::managed_agents::agent_events::agent_event_content(source)
        == crate::managed_agents::agent_events::agent_event_content(current)
        && protected.record == observed.record)
        && unchanged(source, current, protected, observed, personas)
}

pub(super) fn verified_equivalent_heads(
    source: &ManagedAgentRecord,
    current: &ManagedAgentRecord,
    old: &Event,
    new: &Event,
    owner: &nostr::PublicKey,
) -> bool {
    [(source, old), (current, new)].into_iter().all(|(record, event)| {
        let known_content = serde_json::from_str::<Value>(&event.content).is_ok_and(|value| {
            value.as_object().is_some_and(|object| object.keys().all(|key| matches!(key.as_str(),
                "name" | "persona_id" | "system_prompt" | "model" | "provider" | "persona_source_version"
                | "parallelism" | "respond_to" | "respond_to_allowlist" | "home_role" | "project_digest" | "project_withdrawn")))
        });
        known_content && event.pubkey == *owner && event.kind.as_u16() == 30177
            && event.verify().is_ok()
            && event.tags.iter().filter(|tag| tag.as_slice().first().map(String::as_str) == Some("d")).count() == 1
            && event.tags.iter().any(|tag| tag.as_slice() == ["d", record.pubkey.as_str()])
            && serde_json::from_str::<beekeeper_core_pkg::managed_agent::ManagedAgentEventContent>(&event.content)
                .is_ok_and(|content| content == crate::managed_agents::agent_events::agent_event_content(record))
    })
}

#[cfg(test)]
#[path = "host_import_source_tests.rs"]
mod tests;
