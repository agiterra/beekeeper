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
async fn newly_installed_responsibility_route_backfills_below_advanced_watermark_in_order() {
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
    let (relay, queries, _server) = spawn_test_relay_with_events(
        &host.config.keys,
        vec![
            policy_definition(&actor, "owner-only", vec![]),
            after.clone(),
            answered.clone(),
            uncertain.clone(),
            same.clone(),
        ],
    )
    .await;
    host.set_rest_client(relay.rest_client());
    // Modern reply deduplication uses this host’s durable consumption ledger.
    host.state
        .consume_command(&answered.id.to_hex(), now_secs())
        .unwrap();
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
async fn configured_before_activation_backfills_original_same_second_message_once() {
    configured_activation_replays_original_once(false).await;
}

#[tokio::test]
async fn fresh_mint_floor_backfills_message_before_first_definition_once() {
    configured_activation_replays_original_once(true).await;
}

async fn configured_activation_replays_original_once(pre_enrollment: bool) {
    let dir = tempfile::tempdir().expect("fixture");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let owner = test_operator_keys();
    let cwd = dir.path().join("checkout");
    std::fs::create_dir(&cwd).expect("workspace");
    let projects = write_projects(dir.path(), channel, &cwd);
    let initial = provider(&dir.path().join("state"), Some(&projects));
    let mut config = initial.config.clone();
    drop(initial);
    let log = cwd.join("activation-methods.log");
    config.runtimes[0].agent_command = fake_agent(
        dir.path(),
        "activation-agent",
        &crate::session::testing::restorable_agent(&log.to_string_lossy()),
    );
    let seats = dir.path().join("activation-seats.json");
    state::atomic_write(&seats, br#"{"version":1,"pending":{}}"#).expect("seat store");
    config.actor_seats_file = Some(seats);
    let mut host = Provider::new(config).expect("unactivated provider");
    let enrolled_at = now_secs().saturating_sub(3);
    let definition_at = |text: &str, at: u64| {
        nostr::EventBuilder::new(
            Kind::Custom(30177),
            serde_json::json!({"name":"Candidate worker","system_prompt":text,"parallelism":1,"respond_to":"owner-only"}).to_string(),
        )
        .tags([Tag::parse(["d", &actor.public_key().to_hex()]).expect("coordinate")])
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(owner)
        .expect("signed definition")
    };
    // First-time commissioning may persist its signed definition only after the
    // original message is sent. In that case the minted identity's durable floor
    // must survive both first enrollment and a subsequent definition revision.
    let definition_time = enrolled_at + u64::from(pre_enrollment);
    let original = definition_at("Configured original", definition_time);
    let latest = definition_at("Updated before activation", definition_time + 1);
    let addressed = |text: &str| {
        nostr::EventBuilder::new(Kind::Custom(9), text)
            .tags([
                Tag::parse(["h", &channel.to_string()]).expect("channel"),
                Tag::parse(["p", &actor.public_key().to_hex()]).expect("actor"),
            ])
            .custom_created_at(nostr::Timestamp::from(enrolled_at))
            .sign_with_keys(owner)
            .expect("signed addressed input")
    };
    let queued = addressed("Reply CANDIDATE-OK, original input before first activation");
    if pre_enrollment {
        assert!(queued.created_at < original.created_at);
    } else {
        assert_eq!(queued.created_at, original.created_at);
    }
    let (relay, queries, server) =
        spawn_test_relay_with_events(&host.config.keys, vec![latest.clone(), queued.clone()]).await;
    host.set_rest_client(relay.rest_client());
    host.set_relay_self(host.config.keys.public_key().to_hex());
    let actor_dir = host
        .config
        .state_dir
        .join("actors")
        .join(actor.public_key().to_hex());
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&actor_dir)
        .expect("protected actor directory");
    state::atomic_write(
        &actor_dir.join("key"),
        actor.secret_key().to_secret_hex().as_bytes(),
    )
    .expect("actor custody");
    let auth = beekeeper_sdk::nip_oa::compute_auth_tag(owner, &actor.public_key(), "")
        .expect("owner attestation");
    state::atomic_write(&actor_dir.join("auth-tag.json"), auth.as_bytes()).expect("attestation");
    // This is a configured identity, not a route grafted onto an already-running
    // actor. Its immutable signed source predates the latest definition revision.
    let enrollment = actor_dir.join("enrollment.json");
    state::atomic_write(
        &enrollment,
        &serde_json::to_vec(&serde_json::json!({
            "actorPubkey":actor.public_key().to_hex(), "channelId":channel,
            "providerInstance":host.config.runtimes[0].instance_ref, "definition":original,
            "cwd":cwd, "projectRef":null,
            "replayFloorUnix":pre_enrollment.then_some(enrolled_at),
        }))
        .expect("enrollment encoding"),
    )
    .expect("protected enrollment");
    assert_eq!(host.state.sessions().count(), 0);
    host.handle_command_event(channel, &queued)
        .await
        .expect("pre-activation delivery");
    assert!(!host.state.is_command_consumed(&queued.id.to_hex()));
    assert!(host.in_flight.is_empty());
    assert!(!log.exists(), "no runtime exists before activation");
    host.state
        .record_watermark(channel, now_secs() + 100)
        .expect("other channel traffic");
    let create = create_event(&host, channel, "candidate-first-activation");
    let mut content: serde_json::Value = serde_json::from_str(&create.content).expect("create");
    content["action"]["actor"] = serde_json::json!(actor.public_key().to_hex());
    let create = nostr::EventBuilder::new(create.kind, content.to_string())
        .tags(create.tags)
        .sign_with_keys(owner)
        .expect("signed activation");
    host.handle_command_event(channel, &create)
        .await
        .expect("actual managed activation");
    assert_eq!(
        host.state.sessions().count(),
        1,
        "Create must open exactly one real fake-ACP runtime"
    );
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&enrollment).expect("enrollment"))
            .expect("enrollment JSON");
    assert_eq!(persisted["definition"]["id"], latest.id.to_hex());
    assert_eq!(
        persisted["replayFloorUnix"], enrolled_at,
        "latest settings revision and activation time cannot move past queued work"
    );
    assert_eq!(
        host.subscription_replay_since(channel, None),
        Some(enrolled_at.saturating_sub(REPLAY_GRACE_SECS))
    );
    let mut receiver = host.conversation_backfill.slot.take_receiver();
    host.start_conversation_backfill()
        .expect("actual bounded HTTP read");
    let fetched = tokio::time::timeout(Duration::from_secs(5), off_loop::next(&mut receiver))
        .await
        .expect("backfill completes");
    host.finish_conversation_backfill(fetched)
        .await
        .expect("existing replay admission");
    host.flush_replays_now()
        .await
        .expect("same-second original admission");
    pump_until_turn_finished(&mut host).await;
    assert!(host.state.is_command_consumed(&queued.id.to_hex()));
    let methods = std::fs::read_to_string(&log).expect("actual runtime methods");
    assert_eq!(
        methods
            .lines()
            .filter(|method| *method == "session/new")
            .count(),
        1
    );
    assert_eq!(
        methods
            .lines()
            .filter(|method| *method == "session/prompt")
            .count(),
        1
    );
    host.handle_command_event(channel, &create)
        .await
        .expect("exact activation retry");
    host.handle_command_event(channel, &queued)
        .await
        .expect("original input retry");
    assert_eq!(host.state.sessions().count(), 1);
    assert_eq!(
        std::fs::read_to_string(&log).expect("retry methods"),
        methods
    );
    assert!(host.in_flight.is_empty());
    assert!(queries.lock().expect("queries").iter().any(|filter| {
        filter.as_array().is_some_and(|filters| {
            filters.iter().any(|filter| {
                filter.get("#p").is_some()
                    && filter["since"] == enrolled_at.saturating_sub(REPLAY_GRACE_SECS)
            })
        })
    }));
    let session = host
        .state
        .sessions()
        .next()
        .expect("session")
        .session_id
        .clone();
    assert_eq!(
        host.sessions
            .shutdown_and_join(&session, crate::session::ACTOR_RETIRE_GRACE)
            .await,
        crate::session::ActorQuiescence::Joined
    );
    relay.shutdown().await;
    server.abort();
}
