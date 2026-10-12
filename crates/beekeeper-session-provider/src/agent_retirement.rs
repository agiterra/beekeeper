//! Durable admission fence witnessed from existing relay-signed identity retirement.
use crate::{config::Config, session::CreateFailure, Provider};
use beekeeper_acp::relay::RestClient;
use nostr::{Event, Kind, PublicKey};
use serde_json::{json, Value};

fn unavailable(message: &str) -> CreateFailure {
    crate::execution_scope::refuse("AGENT_RETIREMENT_UNAVAILABLE", message)
}
fn refused() -> CreateFailure {
    crate::execution_scope::refuse(
        "AGENT_RETIRED",
        "this managed identity is retired; no runtime admission or replay was allowed",
    )
}
fn valid_event(event: &Event, relay: &PublicKey, actor: &str) -> bool {
    event.kind.as_u16() == 8002
        && event.pubkey == *relay
        && event.verify().is_ok()
        && event.tags.iter().any(|tag| tag.as_slice() == ["p", actor])
        && event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["reason", "retired"])
}
/// Inspect retirement before startup and after its await; no unavailable-read fallback.
pub(crate) async fn observe(
    config: &Config,
    rest: Option<&RestClient>,
    relay_self: Option<&str>,
    actor: Option<&str>,
) -> Result<(), CreateFailure> {
    let Some(actor) = actor else { return Ok(()) };
    let relay = relay_self
        .and_then(|key| PublicKey::from_hex(key).ok())
        .ok_or_else(|| unavailable("relay retirement authority is not witnessed"))?;
    let key =
        PublicKey::from_hex(actor).map_err(|_| unavailable("actor retirement identity invalid"))?;
    let path = config
        .state_dir
        .join("actors")
        .join(actor)
        .join("retirement.json");
    if let Some(proof) = crate::managed_configuration::protected(&path)? {
        let event: Event = serde_json::from_value(
            proof
                .get("event")
                .cloned()
                .ok_or_else(|| unavailable("retirement custody proof missing"))?,
        )
        .map_err(|_| unavailable("retirement custody proof malformed"))?;
        if proof.get("schema").and_then(Value::as_str) != Some("beekeeper-agent-retirement/v1")
            || proof.get("actorPubkey").and_then(Value::as_str) != Some(actor)
            || proof
                .get("relayUrl")
                .and_then(Value::as_str)
                .map(beekeeper_host_core::record::canonical_relay_key)
                != Some(beekeeper_host_core::record::canonical_relay_key(
                    &config.relay_url,
                ))
            || !valid_event(&event, &relay, actor)
        {
            return Err(unavailable(
                "retirement custody proof invalid; repair required",
            ));
        }
        return Err(refused());
    }
    let rest = rest
        .ok_or_else(|| unavailable("retirement transport unavailable; no actor work admitted"))?;
    let rows = rest
        .query(&[nostr::Filter::new()
            .kind(Kind::Custom(8002))
            .author(relay)
            .pubkey(key)])
        .await
        .map_err(|_| unavailable("retirement observation unavailable; no actor work admitted"))?;
    let events = rows
        .as_array()
        .ok_or_else(|| unavailable("retirement observation malformed"))?;
    for raw in events {
        let Ok(event) = serde_json::from_value::<Event>(raw.clone()) else {
            continue;
        };
        if !valid_event(&event, &relay, actor) {
            continue;
        }
        let parent = path.parent().ok_or_else(refused)?;
        beekeeper_host_core::atomic_write::create_dir_all_restricted(parent)
            .map_err(|_| unavailable("retirement custody directory unavailable"))?;
        let proof = json!({"schema":"beekeeper-agent-retirement/v1","actorPubkey":actor,"relayUrl":config.relay_url,"event":event});
        let bytes = serde_json::to_vec(&proof)
            .map_err(|_| unavailable("retirement proof cannot be encoded"))?;
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(&path, &bytes)
            .map_err(|_| unavailable("retirement proof cannot be retained"))?;
        return Err(refused());
    }
    Ok(())
}
impl Provider {
    pub(crate) async fn require_actor_not_retired(
        &mut self,
        actor: Option<&str>,
    ) -> Result<(), CreateFailure> {
        observe(
            &self.config,
            self.rest_client.as_ref(),
            self.relay_self.as_deref(),
            actor,
        )
        .await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Tag};
    #[test]
    fn retirement_requires_exact_relay_signature_actor_and_reason() {
        let relay = Keys::generate();
        let foreign = Keys::generate();
        let actor = Keys::generate().public_key().to_hex();
        let build = |signer: &Keys, target: &str, reason: &str| {
            EventBuilder::new(Kind::Custom(8002), "")
                .tags([
                    Tag::parse(["p", target]).expect("p"),
                    Tag::parse(["reason", reason]).expect("reason"),
                ])
                .sign_with_keys(signer)
                .expect("signed delta")
        };
        assert!(valid_event(
            &build(&relay, &actor, "retired"),
            &relay.public_key(),
            &actor
        ));
        assert!(!valid_event(
            &build(&foreign, &actor, "retired"),
            &relay.public_key(),
            &actor
        ));
        assert!(!valid_event(
            &build(&relay, &actor, "rotated"),
            &relay.public_key(),
            &actor
        ));
        assert!(!valid_event(
            &build(&relay, &foreign.public_key().to_hex(), "retired"),
            &relay.public_key(),
            &actor
        ));
    }
}
