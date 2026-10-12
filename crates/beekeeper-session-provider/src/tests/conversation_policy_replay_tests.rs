use super::*;

fn policy_definition(actor: &Keys, mode: &str, allowlist: Vec<String>) -> Event {
    nostr::EventBuilder::new(Kind::Custom(30177),serde_json::json!({"name":"Conversation worker","system_prompt":"Answer ordinary requests plainly.","parallelism":1,"respond_to":mode,"respond_to_allowlist":allowlist}).to_string())
        .tags([Tag::parse(["d",&actor.public_key().to_hex()]).unwrap()]).sign_with_keys(test_operator_keys()).unwrap()
}
fn public_channel(channel: Uuid) -> Event {
    nostr::EventBuilder::new(Kind::Custom(39000), "{\"name\":\"test\",\"private\":false}")
        .tags([
            Tag::parse(["d", &channel.to_string()]).unwrap(),
            Tag::parse(["t", "stream"]).unwrap(),
        ])
        .sign_with_keys(test_operator_keys())
        .unwrap()
}
#[tokio::test]
async fn ordinary_anyone_policy_does_not_grant_explicit_session_steering() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    let (relay, _, _server) = spawn_test_relay_with_events(
        &host.config.keys,
        vec![
            policy_definition(&actor, "anyone", vec![]),
            public_channel(channel),
        ],
    )
    .await;
    host.set_rest_client(relay.rest_client());
    let event = message(channel, &actor, None, &stranger);
    host.handle_command_event(channel, &event).await.unwrap();
    pump_until_turn_finished(&mut host).await;
    assert!(host.state.is_command_consumed(&event.id.to_hex()));
    let explicit = turn_event(channel, "unauthorized-explicit-turn", &target);
    let explicit = nostr::EventBuilder::new(explicit.kind, explicit.content)
        .tags(explicit.tags)
        .sign_with_keys(&stranger)
        .unwrap();
    host.handle_command_event(channel, &explicit).await.unwrap();
    assert!(host.state.is_command_refused("unauthorized-explicit-turn"));
}
#[tokio::test]
async fn ordinary_allowlist_and_private_owner_clamp_are_revalidated() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let allowed = Keys::generate();
    let outsider = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let definition = policy_definition(&actor, "allowlist", vec![allowed.public_key().to_hex()]);
    let (relay, _, _server) = spawn_test_relay_with_events(
        &host.config.keys,
        vec![definition.clone(), public_channel(channel)],
    )
    .await;
    host.set_rest_client(relay.rest_client());
    assert!(host
        .conversation_author_allowed(
            &actor.public_key().to_hex(),
            channel,
            &allowed.public_key().to_hex()
        )
        .await
        .unwrap());
    assert!(!host
        .conversation_author_allowed(
            &actor.public_key().to_hex(),
            channel,
            &outsider.public_key().to_hex()
        )
        .await
        .unwrap());
    state::atomic_write(&host.config.state_dir.join("actors").join(actor.public_key().to_hex()).join("configuration.json"),&serde_json::to_vec(&serde_json::json!({"schema":"beekeeper-private-agent-config/v1","actorPubkey":actor.public_key().to_hex(),"relayUrl":host.config.relay_url,"definition":definition,"record":{"enforced_owner_only":true}})).unwrap()).unwrap();
    assert!(!host
        .conversation_author_allowed(
            &actor.public_key().to_hex(),
            channel,
            &allowed.public_key().to_hex()
        )
        .await
        .unwrap());
    assert!(host
        .conversation_author_allowed(
            &actor.public_key().to_hex(),
            channel,
            &test_operator_keys().public_key().to_hex()
        )
        .await
        .unwrap());
}
#[tokio::test]
async fn newly_installed_migration_route_backfills_below_advanced_watermark_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let floor = now_secs().saturating_sub(3);
    let make = |text: &str, time: u64| {
        nostr::EventBuilder::new(Kind::Custom(9), text)
            .tags([
                Tag::parse(["h", &channel.to_string()]).unwrap(),
                Tag::parse(["p", &actor.public_key().to_hex()]).unwrap(),
            ])
            .custom_created_at(nostr::Timestamp::from(time))
            .sign_with_keys(test_operator_keys())
            .unwrap()
    };
    let uncertain = make("older uncertain", floor - 1);
    let same = make("same second pending", floor);
    let after = make("after drain pending", floor + 1);
    let answered = make("already answered", floor);
    let reply = nostr::EventBuilder::new(Kind::Custom(9), "old actor answer")
        .tags([
            Tag::parse(["h", &channel.to_string()]).unwrap(),
            Tag::parse(["e", &answered.id.to_hex(), "", "reply"]).unwrap(),
        ])
        .sign_with_keys(&actor)
        .unwrap();
    let (relay, queries, _server) = spawn_test_relay_with_events(
        &host.config.keys,
        vec![
            policy_definition(&actor, "owner-only", vec![]),
            reply,
            after.clone(),
            answered.clone(),
            uncertain.clone(),
            same.clone(),
        ],
    )
    .await;
    host.set_rest_client(relay.rest_client());
    host.state
        .record_watermark(channel, now_secs() + 100)
        .unwrap();
    let path = host.config.state_dir.join("conversation-routes.json");
    let mut routes: ConversationRoutes =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    routes.routes[0].replay_floor_unix = Some(floor);
    state::atomic_write(&path, &serde_json::to_vec(&routes).unwrap()).unwrap();
    assert_eq!(
        host.subscription_replay_since(channel, None),
        Some(floor.saturating_sub(REPLAY_GRACE_SECS))
    );
    let mut receiver = host.conversation_backfill.slot.take_receiver();
    host.start_conversation_backfill().unwrap();
    let fetched = tokio::time::timeout(Duration::from_secs(5), off_loop::next(&mut receiver))
        .await
        .unwrap();
    host.finish_conversation_backfill(fetched).await.unwrap();
    assert_eq!(host.replay.held.len(), 2);
    assert!(host.state.is_command_consumed(&answered.id.to_hex()));
    assert!(host.state.is_command_consumed(&uncertain.id.to_hex()));
    host.flush_replays_now().await.unwrap();
    pump_until_turn_finished(&mut host).await;
    pump_until_turn_finished(&mut host).await;
    for event in [&same, &after] {
        assert!(host.state.is_command_consumed(&event.id.to_hex()));
        host.handle_command_event(channel, event).await.unwrap();
    }
    assert!(host.replay.held.is_empty());
    assert!(queries
        .lock()
        .unwrap()
        .iter()
        .any(|filter| filter.to_string().contains("#p")));
    let sink = CollectingSink::new();
    host.flush(&sink).await.unwrap();
    assert!(sink
        .all()
        .iter()
        .any(|event| event.content.contains("earlier outcome is uncertain")));
}

#[tokio::test]
async fn ordinary_owner_sibling_is_verified_but_unknown_channel_never_widens() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let sibling = Keys::generate();
    let stranger = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let auth =
        beekeeper_sdk::nip_oa::compute_auth_tag(test_operator_keys(), &sibling.public_key(), "")
            .unwrap();
    let profile = nostr::EventBuilder::new(Kind::Metadata, "{}")
        .tags([beekeeper_sdk::nip_oa::parse_auth_tag(&auth).unwrap()])
        .sign_with_keys(&sibling)
        .unwrap();
    let (relay, _, _server) = spawn_test_relay_with_events(
        &host.config.keys,
        vec![policy_definition(&actor, "anyone", vec![]), profile],
    )
    .await;
    host.set_rest_client(relay.rest_client());
    assert!(host
        .conversation_author_allowed(
            &actor.public_key().to_hex(),
            channel,
            &sibling.public_key().to_hex()
        )
        .await
        .unwrap());
    assert!(!host
        .conversation_author_allowed(
            &actor.public_key().to_hex(),
            channel,
            &stranger.public_key().to_hex()
        )
        .await
        .unwrap());
}
#[tokio::test]
async fn unauthorized_ordinary_author_gets_no_ambiguous_responsibility_disclosure() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    let mut second = target.clone();
    second.session_id = Uuid::new_v4().to_string();
    install_routes(&host, channel, &actor, vec![target, second]);
    let event = message(channel, &actor, None, &stranger);
    host.handle_command_event(channel, &event).await.unwrap();
    assert!(host.state.is_command_consumed(&event.id.to_hex()));
    let sink = CollectingSink::new();
    host.flush(&sink).await.unwrap();
    assert!(!sink.all().iter().any(|reply| reply.kind == Kind::Custom(9)));
}

#[tokio::test]
async fn long_stopped_downtime_is_inspected_from_proven_source_without_claiming_consumption() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let capture = now_secs();
    let stopped_time = capture - 1500;
    let source = capture - 2000;
    let make = |text: &str| {
        nostr::EventBuilder::new(Kind::Custom(9), text)
            .tags([
                Tag::parse(["h", &channel.to_string()]).unwrap(),
                Tag::parse(["p", &actor.public_key().to_hex()]).unwrap(),
            ])
            .custom_created_at(nostr::Timestamp::from(stopped_time))
            .sign_with_keys(test_operator_keys())
            .unwrap()
    };
    let during = make("old during drain pending");
    let after = make("old after stop pending");
    let handled = make("old handled");
    let cold = nostr::EventBuilder::new(Kind::Custom(9), "trigger before native session creation")
        .tags([
            Tag::parse(["h", &channel.to_string()]).unwrap(),
            Tag::parse(["p", &actor.public_key().to_hex()]).unwrap(),
        ])
        .custom_created_at(nostr::Timestamp::from(source - 1))
        .sign_with_keys(test_operator_keys())
        .unwrap();
    let reply = nostr::EventBuilder::new(Kind::Custom(9), "proven old result")
        .tags([
            Tag::parse(["h", &channel.to_string()]).unwrap(),
            Tag::parse(["e", &handled.id.to_hex(), "", "reply"]).unwrap(),
        ])
        .sign_with_keys(&actor)
        .unwrap();
    let definition = policy_definition(&actor, "owner-only", vec![]);
    let (relay, queries, _server) = spawn_test_relay_with_events(
        &host.config.keys,
        vec![
            definition.clone(),
            cold.clone(),
            during.clone(),
            after.clone(),
            handled.clone(),
            reply,
        ],
    )
    .await;
    host.set_rest_client(relay.rest_client());
    let path = host.config.state_dir.join("conversation-routes.json");
    let mut routes: ConversationRoutes =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    routes.routes[0].replay_floor_unix = Some(capture);
    state::atomic_write(&path, &serde_json::to_vec(&routes).unwrap()).unwrap();
    host.state.record_watermark(channel, capture + 100).unwrap();
    let mut receiver = host.conversation_backfill.slot.take_receiver();
    // The old grace-only snapshot misses the long-stopped inputs; it must not
    // prevent a newly verified immutable source bound from triggering backfill.
    host.start_conversation_backfill().unwrap();
    let fetched = tokio::time::timeout(Duration::from_secs(5), off_loop::next(&mut receiver))
        .await
        .unwrap();
    host.finish_conversation_backfill(fetched).await.unwrap();
    assert!(!host.state.is_command_consumed(&during.id.to_hex()));
    let private_path = host
        .config
        .state_dir
        .join("actors")
        .join(actor.public_key().to_hex())
        .join("configuration.json");
    state::atomic_write(&private_path,&serde_json::to_vec(&serde_json::json!({"schema":"beekeeper-private-agent-config/v1","actorPubkey":actor.public_key().to_hex(),"relayUrl":host.config.relay_url,"definition":definition,"record":{"migration_backfill_since_unix":source}})).unwrap()).unwrap();
    assert_eq!(
        host.subscription_replay_since(channel, None),
        Some(source.saturating_sub(REPLAY_GRACE_SECS))
    );
    host.start_conversation_backfill().unwrap();
    let fetched = tokio::time::timeout(Duration::from_secs(5), off_loop::next(&mut receiver))
        .await
        .unwrap();
    host.finish_conversation_backfill(fetched).await.unwrap();
    for event in [&during, &after, &handled, &cold] {
        assert!(host.state.is_command_consumed(&event.id.to_hex()));
    }
    assert!(host.in_flight.is_empty());
    assert!(host.replay.held.is_empty());
    let sink = CollectingSink::new();
    host.flush(&sink).await.unwrap();
    let replies: Vec<_> = sink
        .all()
        .into_iter()
        .filter(|event| event.kind == Kind::Custom(9))
        .collect();
    assert_eq!(replies.len(), 3);
    assert!(replies
        .iter()
        .all(|event| event.content.contains("earlier outcome is uncertain")));
    assert!(queries
        .lock()
        .unwrap()
        .iter()
        .any(|query| query
            .as_array()
            .is_some_and(|filters| filters.iter().any(|filter| filter
                .get("since")
                .and_then(serde_json::Value::as_u64)
                == Some(source.saturating_sub(REPLAY_GRACE_SECS))))));
    let mut invalid: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&private_path).unwrap()).unwrap();
    invalid["record"]["migration_backfill_since_unix"] = serde_json::json!(capture + 1);
    state::atomic_write(&private_path, &serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(host
        .start_conversation_backfill()
        .unwrap_err()
        .to_string()
        .contains("MIGRATION_REPLAY_PROVENANCE_INVALID"));
}
