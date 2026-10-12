use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::json;
fn fixture() -> (Keys, String, Event, Snapshot) {
    let owner = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let content: PersonaEventContent=serde_json::from_value(json!({"display_name":"Private","system_prompt":"Private protected instructions","runtime":"codex","model":"model-a"})).unwrap();
    let bytes = serde_json::to_vec(&content).unwrap();
    let pin = EventBuilder::new(
        Kind::Custom(30175),
        String::from_utf8(bytes.clone()).unwrap(),
    )
    .tags([Tag::parse(["d", "private"]).unwrap()])
    .sign_with_keys(&owner)
    .unwrap();
    let current = EventBuilder::new(
        Kind::Custom(30177),
        json!({"name":"Actor","persona_id":"private"}).to_string(),
    )
    .tags([Tag::parse(["d", &actor]).unwrap()])
    .sign_with_keys(&owner)
    .unwrap();
    let snapshot = Snapshot {
        schema: "beekeeper-private-persona/v1".into(),
        actor_pubkey: actor.clone(),
        relay_url: "wss://fixture.example".into(),
        persona_id: "private".into(),
        source_hash: hex::encode(Sha256::digest(bytes)),
        event: pin,
    };
    (owner, actor, current, snapshot)
}
fn envelope(actor: &str, current: &Event, snapshot: &Snapshot) -> serde_json::Value {
    json!({"schema":"beekeeper-private-agent-config/v1","actorPubkey":actor,"relayUrl":"wss://fixture.example/","definition":current,"record":{"private_persona":snapshot}})
}
#[test]
fn signed_private_pin_is_exactly_bound_to_current_revision_and_replay_bytes() {
    let (owner, actor, current, snapshot) = fixture();
    let value = envelope(&actor, &current, &snapshot);
    let selected = from_envelope(
        &value,
        &actor,
        "wss://fixture.example",
        &current,
        owner.public_key(),
        "private",
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.0, snapshot.event);
    assert_eq!(
        selected.1.system_prompt.as_deref(),
        Some("Private protected instructions")
    );
    let repeated = from_envelope(
        &value,
        &actor,
        "wss://fixture.example/",
        &current,
        owner.public_key(),
        "private",
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.0, repeated.0);
    let changed = EventBuilder::new(current.kind, "new revision")
        .tags(current.tags.clone())
        .sign_with_keys(&owner)
        .unwrap();
    assert!(from_envelope(
        &value,
        &actor,
        "wss://fixture.example",
        &changed,
        owner.public_key(),
        "private"
    )
    .is_err());
}
#[test]
fn private_pin_rejects_other_actor_community_persona_owner_or_source_hash() {
    let (owner, actor, current, snapshot) = fixture();
    let value = envelope(&actor, &current, &snapshot);
    for (a, r, id) in [
        ("00", "wss://fixture.example", "private"),
        (actor.as_str(), "wss://other.example", "private"),
        (actor.as_str(), "wss://fixture.example", "other"),
    ] {
        assert!(from_envelope(&value, a, r, &current, owner.public_key(), id).is_err());
    }
    assert!(from_envelope(
        &value,
        &actor,
        "wss://fixture.example",
        &current,
        Keys::generate().public_key(),
        "private"
    )
    .is_err());
    let mut bad = snapshot.clone();
    bad.source_hash = "00".repeat(32);
    assert!(validate(
        &bad,
        &actor,
        "wss://fixture.example",
        owner.public_key(),
        "private"
    )
    .is_err());
}
#[test]
fn private_pin_cannot_import_shared_event_or_tampered_signed_content() {
    let (owner, actor, _, snapshot) = fixture();
    let mut bad = snapshot.clone();
    bad.event.content.push_str("tampered");
    assert!(validate(
        &bad,
        &actor,
        "wss://fixture.example",
        owner.public_key(),
        "private"
    )
    .is_err());
    bad = snapshot.clone();
    bad.event = EventBuilder::new(Kind::Custom(30175), snapshot.event.content)
        .tags([
            Tag::parse(["d", "private"]).unwrap(),
            Tag::parse(["shared", "true"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    assert!(validate(
        &bad,
        &actor,
        "wss://fixture.example",
        owner.public_key(),
        "private"
    )
    .is_err());
}
#[test]
fn absent_private_pin_preserves_existing_public_resolution_and_unknown_schema_refuses() {
    let (owner, actor, current, snapshot) = fixture();
    let mut value = envelope(&actor, &current, &snapshot);
    value["record"] = json!({});
    assert!(from_envelope(
        &value,
        &actor,
        "wss://fixture.example",
        &current,
        owner.public_key(),
        "private"
    )
    .unwrap()
    .is_none());
    value = envelope(&actor, &current, &snapshot);
    value["schema"] = json!("unknown");
    assert!(from_envelope(
        &value,
        &actor,
        "wss://fixture.example",
        &current,
        owner.public_key(),
        "private"
    )
    .is_err());
}
