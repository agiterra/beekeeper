//! A private signed NIP-AP projection, bound by the existing protected30177 envelope.
use crate::{config::Config, session::CreateFailure};
use beekeeper_core::persona::PersonaEventContent;
use nostr::{Event, PublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Snapshot {
    pub schema: String,
    pub actor_pubkey: String,
    pub relay_url: String,
    pub persona_id: String,
    pub source_hash: String,
    pub event: Event,
}
fn refused() -> CreateFailure {
    crate::execution_scope::refuse("AGENT_DEFINITION_PACK_UNAVAILABLE", "protected private persona pin is unavailable or no longer matches this actor/community/definition; no turn was admitted")
}
pub(crate) fn validate(
    snapshot: &Snapshot,
    actor: &str,
    relay: &str,
    owner: PublicKey,
    persona_id: &str,
) -> Result<PersonaEventContent, CreateFailure> {
    let event = &snapshot.event;
    if snapshot.schema != "beekeeper-private-persona/v1"
        || snapshot.actor_pubkey != actor
        || beekeeper_host_core::record::canonical_relay_key(&snapshot.relay_url)
            != beekeeper_host_core::record::canonical_relay_key(relay)
        || snapshot.persona_id != persona_id
        || event.pubkey != owner
        || event.kind.as_u16() != beekeeper_core::kind::KIND_PERSONA as u16
        || event.verify().is_err()
        || !event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["d", persona_id])
        || event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["shared", "true"])
    {
        return Err(refused());
    }
    let content: PersonaEventContent =
        serde_json::from_str(&event.content).map_err(|_| refused())?;
    let bytes = serde_json::to_vec(&content).map_err(|_| refused())?;
    if hex::encode(Sha256::digest(bytes)) != snapshot.source_hash {
        return Err(refused());
    }
    Ok(content)
}
/// Read only the executor-private pin; never obtain an owner's relay credentials.
pub(crate) fn load(
    config: &Config,
    actor: &str,
    current: &Event,
    owner: PublicKey,
    persona_id: &str,
) -> Result<Option<(Event, PersonaEventContent)>, CreateFailure> {
    let path = config
        .state_dir
        .join("actors")
        .join(actor)
        .join("configuration.json");
    let Some(value) = super::protected(&path)? else {
        return Ok(None);
    };
    from_envelope(&value, actor, &config.relay_url, current, owner, persona_id)
}
fn from_envelope(
    value: &serde_json::Value,
    actor: &str,
    relay: &str,
    current: &Event,
    owner: PublicKey,
    persona_id: &str,
) -> Result<Option<(Event, PersonaEventContent)>, CreateFailure> {
    let Some(pin) = value
        .get("record")
        .and_then(|record| record.get("private_persona"))
    else {
        return Ok(None);
    };
    let bound: Event =
        serde_json::from_value(value.get("definition").cloned().ok_or_else(refused)?)
            .map_err(|_| refused())?;
    if value.get("schema").and_then(serde_json::Value::as_str)
        != Some("beekeeper-private-agent-config/v1")
        || value.get("actorPubkey").and_then(serde_json::Value::as_str) != Some(actor)
        || value
            .get("relayUrl")
            .and_then(serde_json::Value::as_str)
            .map(beekeeper_host_core::record::canonical_relay_key)
            != Some(beekeeper_host_core::record::canonical_relay_key(relay))
        || bound.id != current.id
        || bound != *current
        || bound.pubkey != owner
        || bound.verify().is_err()
    {
        return Err(refused());
    }
    let snapshot: Snapshot = serde_json::from_value(pin.clone()).map_err(|_| refused())?;
    let content = validate(&snapshot, actor, relay, owner, persona_id)?;
    Ok(Some((snapshot.event, content)))
}

#[cfg(test)]
#[path = "private_persona_tests.rs"]
mod tests;
