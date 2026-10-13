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
        json!({"name":"Actor","persona_id":"private","parallelism":1,"respond_to":"owner-only"})
            .to_string(),
    )
    .tags([Tag::parse(["d", &actor]).unwrap()])
    .sign_with_keys(&owner)
    .unwrap();
    let snapshot = Snapshot {
        schema: "beekeeper-private-persona/v1".into(),
        actor_pubkey: actor.clone(),
        relay_url: "wss://fixture.example".into(),
        persona_id: "private".into(),
        persona_coordinate: None,
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
fn relay_only_owner_rename_preserves_pin_but_other_definition_changes_refuse() {
    let (owner, actor, bound, snapshot) = fixture();
    let value = envelope(&actor, &bound, &snapshot);
    let original = serde_json::to_vec(&value).unwrap();
    let sign = |content: serde_json::Value, keys: &Keys| {
        EventBuilder::new(bound.kind, content.to_string())
            .tags(bound.tags.clone())
            .custom_created_at(nostr::Timestamp::from(bound.created_at.as_secs() + 1))
            .sign_with_keys(keys)
            .unwrap()
    };
    let mut renamed: serde_json::Value = serde_json::from_str(&bound.content).unwrap();
    renamed["name"] = json!("Owner's remote rename");
    let current = sign(renamed.clone(), &owner);
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
    assert_eq!(serde_json::to_vec(&value).unwrap(), original);
    for (field, changed) in [
        ("persona_id", json!("another")),
        ("persona_source_version", json!("new hash")),
        ("model", json!("other model")),
        ("provider", json!("other provider")),
        ("runtime", json!("other runtime")),
        ("respond_to", json!("anyone")),
        ("project_ref", json!("foreign project")),
        ("parallelism", json!(2)),
        ("unknown_future_policy", json!(true)),
    ] {
        let mut changed_content = renamed.clone();
        changed_content[field] = changed;
        assert!(
            from_envelope(
                &value,
                &actor,
                "wss://fixture.example",
                &sign(changed_content, &owner),
                owner.public_key(),
                "private"
            )
            .is_err(),
            "{field}"
        );
    }
    assert!(from_envelope(
        &value,
        &actor,
        "wss://fixture.example",
        &sign(renamed, &Keys::generate()),
        owner.public_key(),
        "private"
    )
    .is_err());
    let mut tampered = current;
    tampered.content.push(' ');
    assert!(from_envelope(
        &value,
        &actor,
        "wss://fixture.example",
        &tampered,
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
        "private",
        None
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
        "private",
        None
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
        "private",
        None
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

#[test]
fn current_declared_persona_version_is_required_when_present_without_broadening_public_rules() {
    let (owner, actor, _, snapshot) = fixture();
    let head = |persona: &str, version: Option<&str>| {
        EventBuilder::new(
            Kind::Custom(30177),
            json!({"name":"Actor","persona_id":persona,"persona_source_version":version,"parallelism":1,"respond_to":"owner-only"})
                .to_string(),
        )
        .tags([Tag::parse(["d", &actor]).unwrap()])
        .sign_with_keys(&owner)
        .unwrap()
    };
    let matching = head("private", Some(&snapshot.source_hash));
    assert!(from_envelope(
        &envelope(&actor, &matching, &snapshot),
        &actor,
        "wss://fixture.example",
        &matching,
        owner.public_key(),
        "private"
    )
    .unwrap()
    .is_some());
    let mismatching = head("private", Some("unreviewed-other-source"));
    assert!(from_envelope(
        &envelope(&actor, &mismatching, &snapshot),
        &actor,
        "wss://fixture.example",
        &mismatching,
        owner.public_key(),
        "private"
    )
    .is_err());
    let wrong_persona = head("other", None);
    assert!(from_envelope(
        &envelope(&actor, &wrong_persona, &snapshot),
        &actor,
        "wss://fixture.example",
        &wrong_persona,
        owner.public_key(),
        "private"
    )
    .is_err());
    // Existing public linked projection omits this field. Absence is supported,
    // not a new universal persona/workflow version prerequisite.
    let old = head("private", None);
    assert!(from_envelope(
        &envelope(&actor, &old, &snapshot),
        &actor,
        "wss://fixture.example",
        &old,
        owner.public_key(),
        "private"
    )
    .unwrap()
    .is_some());
}

#[test]
fn duplicate_signed_coordinates_are_ambiguous_even_when_one_matches() {
    let (owner, actor, current, mut snapshot) = fixture();
    let good = snapshot.clone();
    snapshot.event = EventBuilder::new(Kind::Custom(30175), snapshot.event.content.clone())
        .tags([
            Tag::parse(["d", "private"]).unwrap(),
            Tag::parse(["d", "other"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    assert!(validate(
        &snapshot,
        &actor,
        "wss://fixture.example",
        owner.public_key(),
        "private",
        None
    )
    .is_err());
    let bad_head = EventBuilder::new(current.kind, current.content.clone())
        .tags([
            Tag::parse(["d", &actor]).unwrap(),
            Tag::parse(["d", "other-actor"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    assert!(from_envelope(
        &envelope(&actor, &bad_head, &good),
        &actor,
        "wss://fixture.example",
        &bad_head,
        owner.public_key(),
        "private"
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn protected_private_pin_file_refuses_world_readable_or_symlink_custody() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("configuration.json");
    std::fs::write(&path, b"{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(super::super::protected(&path).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(super::super::protected(&path).unwrap().is_some());
    let link = dir.path().join("linked.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(super::super::protected(&link).is_err());
}

fn mapped_fixture(raw: &str, coordinate: &str) -> (Keys, String, Event, Snapshot) {
    let (owner, actor, _, mut snapshot) = fixture();
    snapshot.persona_id = raw.into();
    snapshot.persona_coordinate = Some(coordinate.into());
    snapshot.event = EventBuilder::new(Kind::Custom(30175), snapshot.event.content.clone())
        .tags([Tag::parse(["d", coordinate]).unwrap()])
        .sign_with_keys(&owner)
        .unwrap();
    let current = EventBuilder::new(
        Kind::Custom(30177),
        json!({
            "name":"Actor", "persona_id":raw, "persona_source_version":snapshot.source_hash,
            "parallelism":1, "respond_to":"owner-only"
        })
        .to_string(),
    )
    .tags([Tag::parse(["d", &actor]).unwrap()])
    .sign_with_keys(&owner)
    .unwrap();
    (owner, actor, current, snapshot)
}
#[test]
fn internal_builtin_and_team_ids_keep_exact_signed_coordinate_and_pinned_replay() {
    for (raw, coordinate) in [
        ("builtin:fizz", "builtin-fizz"),
        ("definition-uuid", "reviewer"),
    ] {
        let (owner, actor, current, snapshot) = mapped_fixture(raw, coordinate);
        let value = envelope(&actor, &current, &snapshot);
        let before = serde_json::to_vec(&value).unwrap();
        for _ in 0..2 {
            let (event, content) = from_envelope(
                &value,
                &actor,
                "wss://fixture.example",
                &current,
                owner.public_key(),
                raw,
            )
            .unwrap()
            .unwrap();
            assert_eq!(event, snapshot.event);
            assert_eq!(
                content.system_prompt.as_deref(),
                Some("Private protected instructions")
            );
            assert_eq!(serde_json::to_vec(&value).unwrap(), before);
        }
        assert!(validate(
            &snapshot,
            &actor,
            "wss://fixture.example",
            owner.public_key(),
            raw,
            None
        )
        .is_err());
    }
}
#[test]
fn mapped_coordinate_requires_affirmative_owner_hash_and_exact_signed_coordinate() {
    let raw = "builtin:fizz";
    let (owner, actor, current, snapshot) = mapped_fixture(raw, "builtin-fizz");
    let content: serde_json::Value = serde_json::from_str(&current.content).unwrap();
    for hash in [serde_json::Value::Null, json!("different-hash")] {
        let mut changed = content.clone();
        changed["persona_source_version"] = hash;
        let current = EventBuilder::new(current.kind, changed.to_string())
            .tags(current.tags.clone())
            .sign_with_keys(&owner)
            .unwrap();
        assert!(from_envelope(
            &envelope(&actor, &current, &snapshot),
            &actor,
            "wss://fixture.example",
            &current,
            owner.public_key(),
            raw
        )
        .is_err());
    }
    for coordinate in [
        None,
        Some("forged-coordinate".to_string()),
        Some("".to_string()),
    ] {
        let mut changed = snapshot.clone();
        changed.persona_coordinate = coordinate;
        assert!(from_envelope(
            &envelope(&actor, &current, &changed),
            &actor,
            "wss://fixture.example",
            &current,
            owner.public_key(),
            raw
        )
        .is_err());
    }
    for coordinate in ["-invalid", "UPPER", "not:a-slug"] {
        let mut changed = snapshot.clone();
        changed.persona_coordinate = Some(coordinate.into());
        changed.event = EventBuilder::new(Kind::Custom(30175), snapshot.event.content.clone())
            .tags([Tag::parse(["d", coordinate]).unwrap()])
            .sign_with_keys(&owner)
            .unwrap();
        assert!(from_envelope(
            &envelope(&actor, &current, &changed),
            &actor,
            "wss://fixture.example",
            &current,
            owner.public_key(),
            raw
        )
        .is_err());
    }
}
