//! Modern owner-authorized channel activation and exact replay integration.
use super::*;

#[tokio::test]
async fn joined_additional_channel_keeps_primary_and_replays_distinct_inputs_once() {
    use crate::conversation::ConversationRoutes;
    use crate::session::testing::{fake_agent, GOOD_AGENT};
    use crate::tests::{create_event, provider, test_operator_keys, write_projects};
    use beekeeper_host_core::layout::Instance;
    use nostr::{Kind, Tag, Timestamp};
    let dir = tempfile::tempdir().expect("fixture");
    let home = tempfile::tempdir().expect("modern host home");
    let primary = Uuid::new_v4();
    let stream = Uuid::new_v4();
    let denied = Uuid::new_v4();
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).expect("workspace");
    let projects = write_projects(dir.path(), primary, &cwd);
    let initial = provider(&dir.path().join("state"), Some(&projects));
    let mut config = initial.config.clone();
    drop(initial);
    let log = cwd.join("additional-methods.log");
    let body = GOOD_AGENT.replace("while IFS= read -r line; do", &format!(
        "while IFS= read -r line; do\n  printf '%s\\n' \"$line\" | sed -n 's/.*\"method\":\"\\([a-z/_]*\\)\".*/\\1/p' >> \"{}\"", log.display()));
    config.runtimes[0].agent_command = fake_agent(dir.path(), "additional-agent", &body);
    let seats = dir.path().join("actor-seats.json");
    crate::state::atomic_write(&seats, br#"{"version":1,"pending":{}}"#).expect("seats");
    config.actor_seats_file = Some(seats);
    let mut host = crate::Provider::new(config).expect("host");
    let actor = Keys::generate();
    let owner = test_operator_keys();
    let floor = crate::now_secs().saturating_sub(3);
    let definition = EventBuilder::new(Kind::Custom(30177),
        json!({"name":"two conversations","system_prompt":"ordinary work","parallelism":1,"respond_to":"owner-only"}).to_string())
        .tags([Tag::parse(["d", &actor.public_key().to_hex()]).expect("actor")])
        .custom_created_at(Timestamp::from(floor))
        .sign_with_keys(owner).expect("definition");
    let addressed = |text: &str, author: &Keys| {
        EventBuilder::new(Kind::Custom(9), text)
            .allow_self_tagging()
            .tags([
                Tag::parse(["h", &stream.to_string()]).expect("stream"),
                Tag::parse(["p", &actor.public_key().to_hex()]).expect("actor"),
            ])
            .custom_created_at(Timestamp::from(floor))
            .sign_with_keys(author)
            .expect("input")
    };
    let inputs = [
        addressed("first original", owner),
        addressed("second original", owner),
        addressed("third original", owner),
    ];
    let echo = addressed("actor self echo", &actor);
    let membership = |channel: Uuid, with_owner: bool| {
        let mut tags = vec![
            Tag::parse(["d", &channel.to_string()]).expect("channel"),
            Tag::parse(["p", &actor.public_key().to_hex()]).expect("actor"),
            Tag::parse(["p", &host.config.keys.public_key().to_hex()]).expect("provider"),
        ];
        if with_owner {
            tags.push(Tag::parse(["p", &owner.public_key().to_hex()]).expect("owner"));
        }
        EventBuilder::new(Kind::Custom(39002), "")
            .allow_self_tagging()
            .tags(tags)
            .sign_with_keys(&host.config.keys)
            .expect("relay membership")
    };
    let mut events = vec![
        definition.clone(),
        membership(stream, true),
        membership(denied, false),
        echo.clone(),
    ];
    events.extend(inputs.iter().cloned());
    let (relay, _, server) =
        crate::tests::spawn_test_relay_with_identity(&host.config.keys, events).await;
    host.set_rest_client(relay.rest_client());
    host.set_relay_self(host.config.keys.public_key().to_hex());
    let actor_dir = host
        .config
        .state_dir
        .join("actors")
        .join(actor.public_key().to_hex());
    atomic_write::create_dir_all_restricted(&actor_dir).expect("custody");
    crate::state::atomic_write(
        &actor_dir.join("key"),
        actor.secret_key().to_secret_hex().as_bytes(),
    )
    .expect("key");
    let auth =
        beekeeper_sdk::nip_oa::compute_auth_tag(owner, &actor.public_key(), "").expect("auth");
    crate::state::atomic_write(&actor_dir.join("auth-tag.json"), auth.as_bytes())
        .expect("auth custody");
    let enrollment = actor_dir.join("enrollment.json");
    write(&enrollment,&json!({"actorPubkey":actor.public_key().to_hex(),"channelId":primary,
        "providerInstance":host.config.runtimes[0].instance_ref,"definition":definition,"cwd":cwd,"projectRef":null,"replayFloorUnix":floor})).expect("enrollment");
    let create_for = |host: &crate::Provider, channel, command: &str| {
        let event = create_event(host, channel, command);
        let mut payload: Value = serde_json::from_str(&event.content).expect("payload");
        payload["action"]["actor"] = json!(actor.public_key().to_hex());
        EventBuilder::new(event.kind, payload.to_string())
            .tags(event.tags)
            .sign_with_keys(owner)
            .expect("create")
    };
    let primary_command = Uuid::new_v4().to_string();
    let first = create_for(&host, primary, &primary_command);
    host.handle_command_event(primary, &first)
        .await
        .expect("actual primary create");
    let primary_record = host
        .state
        .sessions()
        .next()
        .expect("primary accepted")
        .clone();
    let primary_target = host.target_for(&primary_record);
    ownership::record_host_enrollment(
        home.path(),
        Instance::Dev,
        &actor.public_key().to_hex(),
        &host.config.relay_url,
        &primary_command,
        &definition.id.to_hex(),
        Some(primary_target.clone()),
    )
    .expect("modern ownership");
    let journal_before = ownership::for_pair(
        home.path(),
        Instance::Dev,
        &actor.public_key().to_hex(),
        &host.config.relay_url,
    )
    .expect("journal")
    .expect("owner");
    for event in &inputs {
        host.handle_command_event(stream, event)
            .await
            .expect("pre-route arrival");
        assert!(!host.state.is_command_consumed(&event.id.to_hex()));
    }
    host.state
        .record_watermark(stream, crate::now_secs() + 100)
        .expect("advanced watermark");
    let second = create_for(&host, stream, &Uuid::new_v4().to_string());
    let context = Some((home.path().to_path_buf(), Instance::Dev));
    let bad = create_for(&host, denied, &Uuid::new_v4().to_string());
    assert!(stage_with_context(
        &host.config,
        &host.state,
        Some(&relay.rest_client()),
        denied,
        &owner.public_key().to_hex(),
        &bad.content,
        context.clone()
    )
    .await
    .is_err());
    let unknown = Uuid::new_v4();
    let bad = create_for(&host, unknown, &Uuid::new_v4().to_string());
    assert!(stage_with_context(
        &host.config,
        &host.state,
        Some(&relay.rest_client()),
        unknown,
        &owner.public_key().to_hex(),
        &bad.content,
        context.clone()
    )
    .await
    .is_err());
    host.state
        .update_session(&primary_record.session_id, |row| row.closed = true)
        .expect("unproved closed witness");
    assert!(stage_with_context(
        &host.config,
        &host.state,
        Some(&relay.rest_client()),
        stream,
        &owner.public_key().to_hex(),
        &second.content,
        context.clone()
    )
    .await
    .is_err());
    host.state
        .update_session(&primary_record.session_id, |row| row.closed = false)
        .expect("healthy primary");
    stage_with_context(
        &host.config,
        &host.state,
        Some(&relay.rest_client()),
        stream,
        &owner.public_key().to_hex(),
        &second.content,
        context,
    )
    .await
    .expect("authorized additional scope");
    // Exercise the normal decoder/authorization/workspace machinery and real
    // runtime create after the explicit-context enrollment fixture seam.
    let projects = crate::commands::ProjectsFile::load(host.config.projects_file.as_deref());
    let seats = crate::actor_seats::ActorSeatsFile::load(host.config.actor_seats_file.as_deref());
    let decision = crate::commands::decide_lifecycle(
        &host.context(stream, &projects, &seats, &owner.public_key().to_hex()),
        stream,
        second.created_at.as_secs(),
        &second.content,
    );
    let crate::commands::LifecycleDecision::Create(plan) = decision else {
        panic!("additional create refused: {decision:?}");
    };
    host.create_session(*plan, None)
        .await
        .expect("actual second runtime");
    assert_eq!(host.state.sessions().count(), 2);
    assert!(
        !host
            .state
            .session(&primary_record.session_id)
            .expect("primary untouched")
            .closed
    );
    assert_eq!(
        ownership::for_pair(
            home.path(),
            Instance::Dev,
            &actor.public_key().to_hex(),
            &host.config.relay_url
        )
        .expect("journal")
        .expect("owner")
        .operation_id,
        journal_before.operation_id
    );
    let routes: ConversationRoutes = serde_json::from_slice(
        &std::fs::read(host.config.state_dir.join("conversation-routes.json")).expect("routes"),
    )
    .expect("JSON");
    assert_eq!(routes.routes.len(), 2);
    assert!(routes
        .routes
        .iter()
        .any(|route| route.channel_id == primary && route.target == primary_target));
    assert!(routes
        .routes
        .iter()
        .any(|route| route.channel_id == stream && route.replay_floor_unix == Some(floor)));
    // Primary backfill settles first; then the requested stream snapshot.
    let mut receiver = host.conversation_backfill.slot.take_receiver();
    for _ in 0..2 {
        host.start_conversation_backfill().expect("backfill");
        let fetched = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            crate::off_loop::next(&mut receiver),
        )
        .await
        .expect("fetched");
        host.finish_conversation_backfill(fetched)
            .await
            .expect("admit");
    }
    host.flush_replays_now().await.expect("replay");
    for _ in 0..3 {
        loop {
            let event =
                tokio::time::timeout(std::time::Duration::from_secs(5), host.next_session_event())
                    .await
                    .expect("runtime response")
                    .expect("event");
            let finished = matches!(event, crate::session::SessionEvent::TurnFinished { .. });
            host.handle_session_event(event)
                .expect("record runtime output");
            if finished {
                break;
            }
        }
    }
    let methods = std::fs::read_to_string(&log).expect("methods");
    assert_eq!(
        methods
            .lines()
            .filter(|line| *line == "session/new")
            .count(),
        2
    );
    assert_eq!(
        methods
            .lines()
            .filter(|line| *line == "session/prompt")
            .count(),
        3
    );
    for event in &inputs {
        assert!(host.state.is_command_consumed(&event.id.to_hex()));
        host.handle_command_event(stream, event)
            .await
            .expect("exact retry");
    }
    host.handle_command_event(stream, &echo)
        .await
        .expect("self echo excluded");
    assert_eq!(std::fs::read_to_string(&log).expect("after retry"), methods);
    struct Replies(std::sync::Mutex<Vec<Event>>);
    impl crate::publish::EventSink for Replies {
        async fn publish(&self, event: Event) -> Result<(), String> {
            self.0.lock().expect("replies").push(event);
            Ok(())
        }
    }
    let sink = Replies(std::sync::Mutex::new(Vec::new()));
    host.flush(&sink).await.expect("replies");
    for input in &inputs {
        assert!(sink
            .0
            .lock()
            .expect("replies")
            .iter()
            .any(|event| event.kind == Kind::Custom(9)
                && event.content == "working"
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["h", &stream.to_string()])
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice().get(1) == Some(&input.id.to_hex()))));
    }
    let sessions: Vec<_> = host
        .state
        .sessions()
        .map(|row| row.session_id.clone())
        .collect();
    for session in sessions {
        host.sessions
            .shutdown_and_join(&session, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    relay.shutdown().await;
    server.abort();
}
