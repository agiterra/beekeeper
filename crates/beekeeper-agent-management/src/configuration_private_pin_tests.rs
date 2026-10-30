//! Real shared private Update materialization, without changing public projection.
use super::*;
fn private_pin(owner: &Keys, actor: &str, relay: &str) -> (Value, String) {
    use nostr::hashes::{sha256, Hash};
    let content: beekeeper_core::persona::PersonaEventContent = serde_json::from_value(json!({"display_name":"PrivateBuiltin", "system_prompt":"private native prompt", "runtime":"codex"})).expect("typed persona");
    let bytes = serde_json::to_vec(&content).expect("canonical content");
    let hash = sha256::Hash::hash(&bytes).to_string();
    let event = nostr::EventBuilder::new(
        nostr::Kind::Custom(30175),
        String::from_utf8(bytes).expect("UTF8"),
    )
    .tags([nostr::Tag::identifier("builtin-fizz")])
    .sign_with_keys(owner)
    .expect("private source");
    (
        json!({"schema":"beekeeper-private-persona/v1","actorPubkey":actor,"relayUrl":relay,"personaId":"builtin:fizz","personaCoordinate":"builtin-fizz","sourceHash":hash,"event":event}),
        hash,
    )
}
#[tokio::test]
async fn mapped_private_update_keeps_public_hash_slim_and_private_signed_pin_retry_stable() {
    let (_home, context, mut relay, actor, head, mut desired) = fixture();
    let (pin, hash) = private_pin(relay.keys(), &actor, &relay.url);
    desired.record["private_persona"] = pin;
    let mut args = request(&context, &actor, &head, desired.clone());
    let mut public: ManagedAgentEventContent =
        serde_json::from_str(&head.content).expect("definition");
    public.persona_id = Some("builtin:fizz".into());
    public.persona_source_version = Some(hash.clone());
    args.public_definition = Some(public);
    relay.url = "http://127.0.0.1:31234".into();
    update(&args, &relay, &context)
        .await
        .expect("canonical private Update");
    let path = layout::host_config_path(&context.home, context.instance);
    let config = HostConfig::load(&path)
        .expect("config")
        .expect("commission");
    let materialized = config
        .provider_state_dir
        .join("actors")
        .join(&actor)
        .join("configuration.json");
    let retained = std::fs::read(&materialized).expect("protected bytes");
    let envelope: Value = serde_json::from_slice(&retained).expect("envelope");
    let bound: Event =
        serde_json::from_value(envelope["definition"].clone()).expect("public bound");
    let private: Event = serde_json::from_value(envelope["privateDefinition"].clone())
        .expect("private signed bound");
    let mut private_body: Value = serde_json::from_str(&private.content).expect("private content");
    assert_eq!(
        private_body
            .as_object_mut()
            .expect("object")
            .remove("persona_source_version"),
        Some(json!(hash))
    );
    assert_eq!(
        private_body,
        serde_json::from_str::<Value>(&bound.content).expect("public content")
    );
    assert_eq!(private.pubkey, bound.pubkey);
    assert_eq!(private.tags, bound.tags);
    assert_eq!(private.created_at, bound.created_at);
    assert!(private.verify().is_ok());
    assert!(!bound.content.contains("persona_source_version"));
    assert!(!bound.content.contains("private native prompt"));
    assert!(
        relay
            .events
            .lock()
            .expect("events")
            .iter()
            .all(|event| event.id != private.id),
        "private signed definition never submitted"
    );
    update(&args, &relay, &context)
        .await
        .expect("same operation replay");
    assert_eq!(std::fs::read(&materialized).expect("retry bytes"), retained);
    let after =
        private_configuration_snapshot(&context, &relay, &actor).expect("authorized snapshot");
    for foreign_owner in [false, true] {
        let mut invalid = desired.clone();
        if foreign_owner {
            invalid.record["private_persona"] =
                private_pin(&Keys::generate(), &actor, "ws://127.0.0.1:31234").0;
        } else {
            invalid.record["private_persona"]["sourceHash"] = json!("ab".repeat(32));
        }
        let refused = request(&context, &actor, &after.definition, invalid);
        let writes = relay.writes.load(Ordering::SeqCst);
        assert!(matches!(
            update(&refused, &relay, &context).await,
            Err(CliError::Auth(_))
        ));
        assert_eq!(
            relay.writes.load(Ordering::SeqCst),
            writes,
            "invalid pin refused before publication"
        );
        assert_eq!(
            std::fs::read(&materialized).expect("unchanged bytes"),
            retained
        );
    }
}
#[test]
fn newer_definition_staging_preserves_captured_floor_and_refuses_explicit_drift() {
    let (_home, context, relay, actor, head, private) = fixture();
    let config = HostConfig::load(&layout::host_config_path(&context.home, context.instance))
        .expect("config")
        .expect("commission");
    let dir = config.provider_state_dir.join("actors").join(&actor);
    let path = dir.join("enrollment.json");
    let mut enrollment: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("enrollment")).expect("JSON");
    let original_floor = head.created_at.as_secs().saturating_sub(30);
    enrollment["replayFloorUnix"] = json!(original_floor);
    save(&path, &enrollment).expect("captured mint floor");
    let revised = nostr::EventBuilder::new(head.kind, head.content.clone())
        .tags(head.tags.iter().cloned())
        .custom_created_at(Timestamp::from(head.created_at.as_secs() + 20))
        .sign_with_keys(relay.keys())
        .expect("new revision");
    let operation = Operation {
        hire: None,
        hire_decision: None,
        request: json!({}),
        actor_pubkey: actor,
        events: vec![revised],
        target: None,
        receipt_event_id: None,
        predecessor: None,
        custody_handoff: None,
    };
    let mut args = CreateRequest {
        channel: Uuid::parse_str(enrollment["channelId"].as_str().expect("channel")).expect("UUID"),
        private_config: Some(private),
        ..Default::default()
    };
    let cwd = context.home.join("work");
    let floor = crate::management::private_config::stage_configuration(
        &context.home,
        &config,
        &operation,
        &args,
        &cwd,
        relay.keys(),
    )
    .expect("new revision staged");
    assert_eq!(floor, original_floor);
    let retained = std::fs::read(&path).expect("retained enrollment");
    assert_eq!(
        serde_json::from_slice::<Value>(&retained).expect("JSON")["replayFloorUnix"],
        original_floor
    );
    args.replay_floor_unix = Some(original_floor + 1);
    assert!(matches!(
        crate::management::private_config::stage_configuration(
            &context.home,
            &config,
            &operation,
            &args,
            &cwd,
            relay.keys()
        ),
        Err(CliError::Conflict(_))
    ));
    assert_eq!(
        std::fs::read(&path).expect("unchanged enrollment"),
        retained
    );
    args.channel = Uuid::new_v4();
    assert_eq!(
        crate::management::private_config::stage_configuration(
            &context.home,
            &config,
            &operation,
            &args,
            &cwd,
            relay.keys()
        )
        .expect("new channel may choose explicit responsibility floor"),
        original_floor + 1
    );
}
#[test]
fn completion_uses_exact_created_target_worker_floor_without_overwriting_routes() {
    let state = tempfile::tempdir().expect("state");
    let actor = "ab".repeat(32);
    let channel = Uuid::new_v4();
    let target = beekeeper_core::coding_session_command::CodingSessionTarget {
        driver: "codex-acp".into(),
        instance_id: "provider".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    install_route(state.path(), &actor, channel, &target, Some(100)).expect("worker route");
    let path = state.path().join("conversation-routes.json");
    let retained = std::fs::read(&path).expect("routes");
    assert_eq!(
        completed_route_floor(state.path(), &actor, channel, &target, None, 999)
            .expect("exact accepted route"),
        Some(100)
    );
    assert!(matches!(
        completed_route_floor(state.path(), &actor, channel, &target, Some(999), 999),
        Err(CliError::Conflict(_))
    ));
    let other = beekeeper_core::coding_session_command::CodingSessionTarget {
        generation: 2,
        ..target
    };
    assert_eq!(
        completed_route_floor(state.path(), &actor, channel, &other, None, 321)
            .expect("different target uses its own staged boundary"),
        Some(321)
    );
    assert_eq!(std::fs::read(path).expect("routes preserved"), retained);
}

#[test]
fn existing_unproved_floor_never_becomes_latest_definition_time_or_stage_write() {
    for invalid in [None, Some(Value::Null), Some(json!("123"))] {
        let (_home, context, relay, actor, head, private) = fixture();
        let config = HostConfig::load(&layout::host_config_path(&context.home, context.instance))
            .expect("config")
            .expect("commission");
        let dir = config.provider_state_dir.join("actors").join(&actor);
        let path = dir.join("enrollment.json");
        let mut enrollment: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("bytes")).expect("enrollment");
        enrollment
            .as_object_mut()
            .expect("object")
            .remove("replayFloorUnix");
        if let Some(invalid) = invalid {
            enrollment["replayFloorUnix"] = invalid;
        }
        save(&path, &enrollment).expect("uncertain existing floor fixture");
        let before = std::fs::read(&path).expect("retained enrollment");
        let config_before = std::fs::read(dir.join("configuration.json")).expect("snapshot");
        let operation = Operation {
            hire: None,
            hire_decision: None,
            request: json!({}),
            actor_pubkey: actor,
            events: vec![head],
            target: None,
            receipt_event_id: None,
            predecessor: None,
            custody_handoff: None,
        };
        let args = CreateRequest {
            channel: Uuid::parse_str(enrollment["channelId"].as_str().expect("channel"))
                .expect("UUID"),
            private_config: Some(private),
            ..Default::default()
        };
        assert!(matches!(
            crate::management::private_config::stage_configuration(
                &context.home,
                &config,
                &operation,
                &args,
                &context.home.join("work"),
                relay.keys()
            ),
            Err(CliError::Refused(_))
        ));
        assert_eq!(std::fs::read(path).expect("enrollment preserved"), before);
        assert_eq!(
            std::fs::read(dir.join("configuration.json")).expect("snapshot preserved"),
            config_before
        );
    }
}
