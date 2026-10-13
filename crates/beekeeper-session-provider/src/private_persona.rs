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
    /// Signed NIP-AP coordinate when it differs from the internal definition ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_coordinate: Option<String>,
    pub source_hash: String,
    pub event: Event,
}
fn exact_coordinate(event: &Event, id: &str) -> bool {
    let mut tags = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_some_and(|key| key == "d"));
    tags.next().is_some_and(|tag| tag.as_slice() == ["d", id]) && tags.next().is_none()
}
fn refused() -> CreateFailure {
    crate::execution_scope::refuse("AGENT_DEFINITION_PACK_UNAVAILABLE", "protected private persona pin is unavailable or no longer matches this actor/community/definition; no turn was admitted")
}
// A relay-only owner may rename an actor without executor-private access.
// Preserve the exact signed persona pin for that metadata-only revision. All
// tags and every other raw content field must remain identical; this does not
// admit policy/runtime changes or relax open-turn revision replay fences.
fn bound_revision_matches(bound: &Event, current: &Event, actor: &str, owner: PublicKey) -> bool {
    for event in [bound, current] {
        if event.pubkey != owner
            || event.kind.as_u16() != beekeeper_core::kind::KIND_MANAGED_AGENT as u16
            || !exact_coordinate(event, actor)
            || event.verify().is_err()
        {
            return false;
        }
    }
    if bound == current {
        return true;
    }
    if bound.tags != current.tags || current.created_at < bound.created_at {
        return false;
    }
    let (Ok(mut before), Ok(mut after)) = (
        serde_json::from_str::<serde_json::Value>(&bound.content),
        serde_json::from_str::<serde_json::Value>(&current.content),
    ) else {
        return false;
    };
    let (Some(before), Some(after)) = (before.as_object_mut(), after.as_object_mut()) else {
        return false;
    };
    if before
        .get("name")
        .and_then(serde_json::Value::as_str)
        .is_none()
        || after
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_none()
    {
        return false;
    }
    before.remove("name");
    after.remove("name");
    before == after
}
// The public projection omits a linked source hash. Its owner can bind that
// private hash with the same30177 definition, signed and retained only here.
fn private_bound_hash(
    value: &serde_json::Value,
    bound: &Event,
    actor: &str,
    owner: PublicKey,
) -> Result<Option<String>, CreateFailure> {
    let Some(raw) = value.get("privateDefinition") else {
        return Ok(None);
    };
    let private: Event = serde_json::from_value(raw.clone()).map_err(|_| refused())?;
    if private.pubkey != owner
        || private.kind != bound.kind
        || private.kind.as_u16() != beekeeper_core::kind::KIND_MANAGED_AGENT as u16
        || private.tags != bound.tags
        || private.created_at != bound.created_at
        || !exact_coordinate(&private, actor)
        || private.verify().is_err()
    {
        return Err(refused());
    }
    let public = beekeeper_core::private_managed_agent::parse_strict_json(bound.content.as_bytes())
        .map_err(|_| refused())?;
    let mut preimage =
        beekeeper_core::private_managed_agent::parse_strict_json(private.content.as_bytes())
            .map_err(|_| refused())?;
    let fields = preimage.as_object_mut().ok_or_else(refused)?;
    let hash = fields
        .remove("persona_source_version")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(refused)?;
    let mut projected = public.clone();
    if let Some(fields) = projected.as_object_mut() {
        // A public hash, when present, is checked independently below.
        fields.remove("persona_source_version");
    }
    if preimage != projected {
        return Err(refused());
    }
    Ok(Some(hash))
}

pub(crate) fn validate(
    snapshot: &Snapshot,
    actor: &str,
    relay: &str,
    owner: PublicKey,
    persona_id: &str,
    bound_source_hash: Option<&str>,
) -> Result<PersonaEventContent, CreateFailure> {
    let event = &snapshot.event;
    let coordinate = snapshot.persona_coordinate.as_deref().unwrap_or(persona_id);
    // A protected mapping is not authority by itself. The owner-signed30177
    // must affirmatively pin the exact source when its internal ID differs.
    if coordinate != persona_id
        && (bound_source_hash != Some(snapshot.source_hash.as_str())
            || coordinate.is_empty()
            || coordinate.len() > 64
            || !coordinate.as_bytes()[0].is_ascii_lowercase()
                && !coordinate.as_bytes()[0].is_ascii_digit()
            || !coordinate
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'))
    {
        return Err(refused());
    }
    if snapshot.schema != "beekeeper-private-persona/v1"
        || snapshot.actor_pubkey != actor
        || beekeeper_host_core::record::canonical_relay_key(&snapshot.relay_url)
            != beekeeper_host_core::record::canonical_relay_key(relay)
        || snapshot.persona_id != persona_id
        || event.pubkey != owner
        || event.kind.as_u16() != beekeeper_core::kind::KIND_PERSONA as u16
        || event.verify().is_err()
        || !exact_coordinate(event, coordinate)
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
        || !bound_revision_matches(&bound, current, actor, owner)
        || bound.pubkey != owner
        || bound.kind.as_u16() != beekeeper_core::kind::KIND_MANAGED_AGENT as u16
        || !exact_coordinate(&bound, actor)
        || bound.verify().is_err()
    {
        return Err(refused());
    }
    let snapshot: Snapshot = serde_json::from_value(pin.clone()).map_err(|_| refused())?;
    let definition: beekeeper_core::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&bound.content).map_err(|_| refused())?;
    if definition.persona_id.as_deref() != Some(persona_id)
        || definition
            .persona_source_version
            .as_deref()
            .is_some_and(|version| version != snapshot.source_hash)
    {
        return Err(refused());
    }
    let private_hash = private_bound_hash(value, &bound, actor, owner)?;
    if private_hash
        .as_deref()
        .is_some_and(|hash| hash != snapshot.source_hash)
        || definition
            .persona_source_version
            .as_deref()
            .zip(private_hash.as_deref())
            .is_some_and(|(public, private)| public != private)
    {
        return Err(refused());
    }
    let content = validate(
        &snapshot,
        actor,
        relay,
        owner,
        persona_id,
        definition
            .persona_source_version
            .as_deref()
            .or(private_hash.as_deref()),
    )?;
    Ok(Some((snapshot.event, content)))
}

#[cfg(test)]
#[path = "private_persona_tests.rs"]
mod tests;
