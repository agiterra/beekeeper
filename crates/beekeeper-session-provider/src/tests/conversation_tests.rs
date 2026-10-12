use super::*;
use crate::conversation::{ConversationRoute, ConversationRoutes};
use nostr::Tag;

async fn routed_provider(
    dir: &Path,
    channel: Uuid,
    actor: &Keys,
) -> (Provider, CodingSessionTarget) {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("checkout");
    let projects = write_projects(dir, channel, &cwd);
    let mut host = provider(&dir.join("state"), Some(&projects));
    // Actor startup now verifies current relay-signed retirement before opening.
    // Establish the mock transport/authority before create, just as run_with does.
    let (initial_relay, _, _initial_server) = spawn_test_relay(&host.config.keys, None).await;
    host.set_rest_client(initial_relay.rest_client());
    host.set_relay_self(host.config.keys.public_key().to_hex());
    let seats = dir.join("conversation-actor-seats.json");
    state::atomic_write(&seats, &serde_json::to_vec(&serde_json::json!({"version":1,"pending":{"conversation-create":{"pubkey":actor.public_key().to_hex(),"nsec":actor.secret_key().to_secret_hex(),"authTag":null,"relayUrl":host.config.relay_url}}})).expect("custody json")).expect("actor seat");
    host.config.actor_seats_file = Some(seats);
    let create = create_event(&host, channel, "conversation-create");
    let mut content: serde_json::Value =
        serde_json::from_str(&create.content).expect("create payload");
    content["action"]["actor"] = serde_json::json!(actor.public_key().to_hex());
    let create = nostr::EventBuilder::new(create.kind, content.to_string())
        .tags(create.tags)
        .sign_with_keys(test_operator_keys())
        .expect("attributed Solo create");
    host.handle_command_event(channel, &create)
        .await
        .expect("accepted create");
    let record = host
        .state
        .sessions()
        .next()
        .expect("accepted session")
        .clone();
    let target = host.target_for(host.state.session(&record.session_id).expect("session"));
    install_routes(&host, channel, actor, vec![target.clone()]);
    let tag =
        beekeeper_sdk::nip_oa::compute_auth_tag(test_operator_keys(), &actor.public_key(), "")
            .expect("owner attestation");
    state::atomic_write(
        &host
            .config
            .state_dir
            .join("actors")
            .join(actor.public_key().to_hex())
            .join("auth-tag.json"),
        tag.as_bytes(),
    )
    .expect("scoped attestation");
    let definition = nostr::EventBuilder::new(nostr::Kind::Custom(beekeeper_core::kind::KIND_MANAGED_AGENT as u16),
        serde_json::json!({"name":"Conversation worker", "system_prompt":"Answer ordinary requests plainly.", "parallelism":1, "respond_to":"owner-only"}).to_string())
        .tags([Tag::parse(["d", &actor.public_key().to_hex()]).expect("actor coordinate")])
        .sign_with_keys(test_operator_keys()).expect("owner definition");
    let (relay, _, _server) = spawn_test_relay(&host.config.keys, Some(definition)).await;
    host.set_rest_client(relay.rest_client());
    (host, target)
}

fn install_routes(host: &Provider, channel: Uuid, actor: &Keys, targets: Vec<CodingSessionTarget>) {
    let dir = host
        .config
        .state_dir
        .join("actors")
        .join(actor.public_key().to_hex());
    std::fs::create_dir_all(&dir).expect("actor dir");
    state::atomic_write(
        &dir.join("key"),
        actor.secret_key().to_secret_hex().as_bytes(),
    )
    .expect("scoped custody");
    let routes = ConversationRoutes {
        routes: targets
            .into_iter()
            .map(|target| ConversationRoute {
                actor_pubkey: actor.public_key().to_hex(),
                channel_id: channel,
                target,
                replay_floor_unix: None,
            })
            .collect(),
    };
    state::atomic_write(
        &host.config.state_dir.join("conversation-routes.json"),
        &serde_json::to_vec(&routes).expect("routes"),
    )
    .expect("route materialization");
}

fn message(channel: Uuid, actor: &Keys, root: Option<&str>, keys: &Keys) -> Event {
    let mut tags = vec![Tag::parse(["h", &channel.to_string()]).expect("channel")];
    if let Some(root) = root {
        tags.push(Tag::parse(["e", root, "", "root"]).expect("root"));
    } else {
        tags.push(Tag::parse(["p", &actor.public_key().to_hex()]).expect("mention"));
    }
    nostr::EventBuilder::new(nostr::Kind::Custom(9), "ordinary conversation")
        .tags(tags)
        .sign_with_keys(keys)
        .expect("signed input")
}

#[tokio::test]
async fn conversation_runs_once_through_existing_replay_and_returns_actor_thread_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    let event = message(channel, &actor, None, test_operator_keys());
    host.open_replay_window(channel);
    host.handle_command_event(channel, &event)
        .await
        .expect("hold");
    assert!(!host.state.is_command_consumed(&event.id.to_hex()));
    host.flush_replays_now()
        .await
        .expect("existing replay admission");
    pump_until_turn_finished(&mut host).await;
    assert!(host.state.is_command_consumed(&event.id.to_hex()));
    host.handle_command_event(channel, &event)
        .await
        .expect("duplicate");
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("outbox");
    let replies: Vec<_> = sink
        .all()
        .into_iter()
        .filter(|e| e.kind == nostr::Kind::Custom(9))
        .collect();
    assert!(replies.iter().all(|e| e.pubkey == actor.public_key()));
    assert!(replies.iter().all(|e| e
        .tags
        .iter()
        .any(|tag| tag.as_slice().get(1) == Some(&event.id.to_hex()))));
    assert!(
        replies.iter().any(|e| !e.content.starts_with("Request ")),
        "assistant answer returned to origin"
    );
    let config = host.config.clone();
    let live_sessions: Vec<_> = host
        .state
        .sessions()
        .map(|record| record.session_id.clone())
        .collect();
    for session_id in live_sessions {
        host.sessions
            .shutdown_and_join(&session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    drop(host);
    let mut restarted = Provider::new(config).expect("restart");
    restarted
        .handle_command_event(channel, &event)
        .await
        .expect("consumed input replay");
    assert!(restarted.in_flight.is_empty());
    assert_eq!(restarted.state.sessions().count(), 1);
    assert_eq!(
        restarted
            .state
            .session(&target.session_id)
            .expect("identity survives")
            .actor,
        Some(actor.public_key().to_hex())
    );
}

#[tokio::test]
async fn conversation_bound_thread_selects_its_responsibility_and_new_thread_is_ambiguous() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, first) = routed_provider(dir.path(), channel, &actor).await;
    let original = message(channel, &actor, None, test_operator_keys());
    let payload = host
        .conversation_command(channel, &original)
        .expect("bind")
        .expect("turn");
    assert_eq!(
        commands::decode_turn_command(&payload)
            .expect("command")
            .target,
        first
    );
    let mut second = host
        .state
        .session(&first.session_id)
        .expect("record")
        .clone();
    second.session_id = Uuid::new_v4().to_string();
    let second_target = host.target_for(&second);
    host.state
        .insert_session(second)
        .expect("second accepted responsibility");
    install_routes(&host, channel, &actor, vec![first.clone(), second_target]);
    let followup = message(
        channel,
        &actor,
        Some(&original.id.to_hex()),
        test_operator_keys(),
    );
    let payload = host
        .conversation_command(channel, &followup)
        .expect("continue")
        .expect("turn");
    assert_eq!(
        commands::decode_turn_command(&payload)
            .expect("command")
            .target,
        first
    );
    let new_request = message(channel, &actor, None, test_operator_keys());
    // Same timestamp and content produces the original id; use a changed signed body.
    let new_request = nostr::EventBuilder::new(nostr::Kind::Custom(9), "new work")
        .tags(new_request.tags)
        .sign_with_keys(test_operator_keys())
        .expect("new input");
    assert!(host
        .conversation_command(channel, &new_request)
        .expect("ambiguity")
        .is_none());
    assert!(host.in_flight.is_empty());
    let stranger = message(
        channel,
        &actor,
        Some(&original.id.to_hex()),
        &Keys::generate(),
    );
    assert!(host
        .conversation_command(channel, &stranger)
        .expect("ungranted")
        .is_none());
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("flush");
    assert!(sink
        .all()
        .iter()
        .any(|e| e.content.contains("multiple responsibilities")));
}

#[tokio::test]
async fn conversation_actor_outbox_survives_restart_but_key_withdrawal_fences_publication() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let event = message(channel, &actor, None, test_operator_keys());
    let payload = host
        .conversation_command(channel, &event)
        .expect("translate")
        .expect("command");
    let target = commands::decode_turn_command(&payload)
        .expect("command")
        .target;
    host.conversation_receipt(
        &event.id.to_hex(),
        &LifecycleReceipt::turn_refused(
            &event.id.to_hex(),
            &target,
            "PROVIDER_UNAVAILABLE",
            "private/path",
        ),
    )
    .expect("blocked reply");
    let config = host.config.clone();
    let live_sessions: Vec<_> = host
        .state
        .sessions()
        .map(|record| record.session_id.clone())
        .collect();
    for session_id in live_sessions {
        host.sessions
            .shutdown_and_join(&session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    drop(host);
    let mut host = Provider::new(config.clone()).expect("reload scoped reply");
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("recover outbox");
    let replies: Vec<_> = sink
        .all()
        .into_iter()
        .filter(|e| e.kind == nostr::Kind::Custom(9))
        .collect();
    assert_eq!(replies.len(), 1);
    assert!(replies[0].content.contains("PROVIDER_UNAVAILABLE"));
    assert!(!replies[0].content.contains("private/path"));
    assert!(crate::conversation::delegated_outbox_event(
        &config.state_dir,
        &replies[0]
    ));
    std::fs::remove_file(
        config
            .state_dir
            .join("actors")
            .join(actor.public_key().to_hex())
            .join("key"),
    )
    .expect("withdraw custody");
    assert!(!crate::conversation::delegated_outbox_event(
        &config.state_dir,
        &replies[0]
    ));
}

#[tokio::test]
async fn conversation_definition_unavailable_is_visible_and_never_opens_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    host.rest_client = None;
    let request = message(channel, &actor, None, test_operator_keys());
    host.handle_command_event(channel, &request)
        .await
        .expect("truthful refusal");
    assert!(host.in_flight.is_empty());
    assert!(!host.state.is_command_consumed(&request.id.to_hex()));
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("refusal output");
    assert!(sink
        .all()
        .iter()
        .any(|event| event.kind == nostr::Kind::Custom(9)
            && event.content.contains("AGENT_DEFINITION_UNAVAILABLE")));
}

#[tokio::test]
async fn conversation_native_restore_uses_persistent_scoped_custody_and_withdrawal_blocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    host.state
        .update_session(&target.session_id, |record| {
            record.resume_cursor = Some("existing-native-cursor".into());
        })
        .expect("saved native cursor");
    let config = host.config.clone();
    let live_sessions: Vec<_> = host
        .state
        .sessions()
        .map(|record| record.session_id.clone())
        .collect();
    for session_id in live_sessions {
        host.sessions
            .shutdown_and_join(&session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    drop(host);
    let mut host = Provider::new(config.clone()).expect("worker restart");
    assert!(
        host.prepare_restore(&target.session_id).is_ok(),
        "accepted actor-only context retains scoped native restore custody without a restaged seat"
    );
    // Preparing a reopen is durable intent, even if this inspection drops its
    // job. Use an independent joined fixture for the withdrawn-custody check.
    let missing_dir = tempfile::tempdir().expect("independent custody fixture");
    let (mut missing, missing_target) = routed_provider(missing_dir.path(), channel, &actor).await;
    missing
        .sessions
        .shutdown_and_join(
            &missing_target.session_id,
            crate::session::ACTOR_RETIRE_GRACE,
        )
        .await;
    std::fs::remove_file(
        missing
            .config
            .state_dir
            .join("actors")
            .join(actor.public_key().to_hex())
            .join("key"),
    )
    .expect("withdraw custody");
    assert!(matches!(
        missing.prepare_restore(&missing_target.session_id),
        Err(crate::native_restore::RestoreObstacle::ActorUnavailable)
    ));
}

#[tokio::test]
async fn conversation_after_worker_restart_loads_native_cursor_once_without_new_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    let cursor = host
        .state
        .session(&target.session_id)
        .expect("record")
        .resume_cursor
        .clone();
    let rest = host.rest_client.clone();
    let mut config = host.config.clone();
    let live_sessions: Vec<_> = host
        .state
        .sessions()
        .map(|record| record.session_id.clone())
        .collect();
    for session_id in live_sessions {
        host.sessions
            .shutdown_and_join(&session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    drop(host);
    let log = dir.path().join("checkout").join("native-methods.log");
    config.runtimes[0].agent_command = fake_agent(
        dir.path(),
        "native-loader",
        &crate::session::testing::restorable_agent(&log.to_string_lossy()),
    );
    let mut host = Provider::new(config).expect("restart");
    host.rest_client = rest;
    host.set_relay_self(host.config.keys.public_key().to_hex());
    host.recover().await.expect("existing crash reconciliation");
    let request = message(channel, &actor, None, test_operator_keys());
    host.handle_command_event(channel, &request)
        .await
        .expect("ordinary request lazy restore");
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("diagnostic disposition");
    assert!(
        host.sessions
            .handle(&target.session_id)
            .is_some_and(session::SessionHandle::is_live),
        "restore disposition {:?}; methods {:?}",
        sink.all()
            .iter()
            .map(|event| &event.content)
            .collect::<Vec<_>>(),
        std::fs::read_to_string(&log)
    );
    pump_until_turn_finished(&mut host).await;
    assert!(host.state.is_command_consumed(&request.id.to_hex()));
    assert_eq!(host.state.sessions().count(), 1);
    let record = host
        .state
        .session(&target.session_id)
        .expect("same responsibility");
    assert_eq!(record.generation, target.generation);
    assert_eq!(record.resume_cursor, cursor);
    let methods = std::fs::read_to_string(&log).expect("native methods");
    assert_eq!(
        methods
            .lines()
            .filter(|method| *method == "session/load")
            .count(),
        1
    );
    assert!(!methods.lines().any(|method| method == "session/new"));
    host.handle_command_event(channel, &request)
        .await
        .expect("replay");
    assert_eq!(
        std::fs::read_to_string(log).expect("methods after replay"),
        methods
    );
}

#[tokio::test]
async fn conversation_definition_uses_existing_low_event_id_rule_for_same_second_heads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let definition = |prompt: &str| {
        nostr::EventBuilder::new(nostr::Kind::Custom(30177), serde_json::json!({"name":"Participant","system_prompt":prompt,"parallelism":1,"respond_to":"owner-only"}).to_string())
        .custom_created_at(nostr::Timestamp::from(12345))
        .tags([Tag::parse(["d", &actor.public_key().to_hex()]).expect("coordinate")])
        .sign_with_keys(test_operator_keys()).expect("definition")
    };
    let left = definition("first behavior");
    let right = definition("second behavior");
    let expected = if left.id < right.id {
        "first behavior"
    } else {
        "second behavior"
    };
    let (relay, _, _server) =
        spawn_test_relay_with_events(&host.config.keys, vec![right, left]).await;
    host.set_rest_client(relay.rest_client());
    let request = message(channel, &actor, None, test_operator_keys());
    host.conversation_command(channel, &request)
        .expect("route")
        .expect("command");
    let instructions = host
        .conversation_instructions(
            &request.id.to_hex(),
            &test_operator_keys().public_key().to_hex(),
        )
        .await
        .expect("same owner head")
        .expect("instructions");
    assert!(instructions.contains(expected));
}

fn linked_instance(actor: &Keys) -> Event {
    nostr::EventBuilder::new(nostr::Kind::Custom(30177), serde_json::json!({"name":"Linked participant","persona_id":"linked-persona","parallelism":1,"respond_to":"owner-only"}).to_string())
        .tags([Tag::parse(["d", &actor.public_key().to_hex()]).expect("coordinate")])
        .sign_with_keys(test_operator_keys()).expect("instance")
}
fn linked_persona(keys: &Keys, shared: bool, prompt: &str, timestamp: u64) -> Event {
    let mut tags = vec![Tag::parse(["d", "linked-persona"]).expect("coordinate")];
    if shared {
        tags.push(Tag::parse(["shared", "true"]).expect("shared"));
    }
    nostr::EventBuilder::new(
        nostr::Kind::Custom(30175),
        serde_json::json!({"display_name":"Linked definition","system_prompt":prompt}).to_string(),
    )
    .custom_created_at(nostr::Timestamp::from(timestamp))
    .tags(tags)
    .sign_with_keys(keys)
    .expect("persona")
}
#[tokio::test]
async fn linked_shared_definition_retains_both_revisions_and_fences_running_revision() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    let instance = linked_instance(&actor);
    let persona = linked_persona(test_operator_keys(), true, "original linked behavior", 100);
    let (relay, control, _server) =
        spawn_recording_test_relay(&host.config.keys, vec![instance.clone(), persona.clone()])
            .await;
    host.set_rest_client(relay.rest_client());
    let request = message(channel, &actor, None, test_operator_keys());
    host.conversation_command(channel, &request)
        .expect("route")
        .expect("command");
    let id = request.id.to_hex();
    let operator = test_operator_keys().public_key().to_hex();
    let instructions = host
        .conversation_instructions(&id, &operator)
        .await
        .expect("shared authorized head")
        .expect("instructions");
    assert!(instructions.contains("original linked behavior"));
    {
        let queries = control.queries.lock().expect("queries");
        assert!(queries
            .iter()
            .any(
                |query| query
                    .as_array()
                    .is_some_and(|filters| filters
                        .iter()
                        .any(|filter| filter["kinds"] == serde_json::json!([30175])
                            && filter["authors"] == serde_json::json!([operator.clone()])
                            && filter["#d"] == serde_json::json!(["linked-persona"])))
            ));
    }

    let origins: serde_json::Value = serde_json::from_slice(
        &std::fs::read(host.config.state_dir.join("conversation-origins.json"))
            .expect("origin file"),
    )
    .expect("origins");
    assert_eq!(
        origins["requests"][&id]["definitionEventId"],
        serde_json::json!(instance.id.to_hex())
    );
    assert_eq!(
        origins["requests"][&id]["linkedDefinitionEventId"],
        serde_json::json!(persona.id.to_hex())
    );
    // Persist the existing open-turn boundary with its accepted revision. A newer pack cannot enter
    // via steering while the original turn is still running.
    host.state
        .consume_command(&id, now_secs())
        .expect("accepted ledger");
    host.state
        .update_session(&target.session_id, |record| {
            record.open_turn = Some(state::OpenTurn {
                turn_id: "linked-turn".into(),
                command_id: Some(id.clone()),
                team_wake_eligible: true,
                started_at_ms: now_ms(),
                operator_pubkey: Some(operator.clone()),
            })
        })
        .expect("persist existing open-turn boundary");
    control.events.lock().expect("events").push(linked_persona(
        test_operator_keys(),
        true,
        "updated linked behavior",
        101,
    ));
    let second = nostr::EventBuilder::new(nostr::Kind::Custom(9), "second request")
        .tags([
            Tag::parse(["h", &channel.to_string()]).expect("channel"),
            Tag::parse(["e", &id, "", "root"]).expect("root"),
        ])
        .sign_with_keys(test_operator_keys())
        .expect("second");
    host.conversation_command(channel, &second)
        .expect("route")
        .expect("command");
    let failure = host
        .conversation_instructions(&second.id.to_hex(), &operator)
        .await
        .expect_err("changed linked revision blocks active turn");
    assert_eq!(failure.code, "AGENT_DEFINITION_UPDATE_PENDING");
    assert_eq!(
        host.conversation_instructions(&id, &operator)
            .await
            .expect("consumed replay"),
        Some(instructions)
    );
    host.state
        .update_session(&target.session_id, |record| record.open_turn = None)
        .expect("safe boundary");
    assert!(host
        .conversation_instructions(&second.id.to_hex(), &operator)
        .await
        .expect("safe boundary")
        .expect("instructions")
        .contains("updated linked behavior"));
    assert_eq!(
        host.state
            .session(&target.session_id)
            .expect("same context")
            .generation,
        1
    );
}
#[tokio::test]
async fn linked_definition_missing_protected_or_foreign_never_admits_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let instance = linked_instance(&actor);
    let outsider = Keys::generate();
    let (relay, control, _server) =
        spawn_recording_test_relay(&host.config.keys, vec![instance.clone()]).await;
    host.set_rest_client(relay.rest_client());
    let request = message(channel, &actor, None, test_operator_keys());
    host.conversation_command(channel, &request)
        .expect("route")
        .expect("command");
    for pack in [
        None,
        Some(linked_persona(&outsider, true, "foreign", 100)),
        Some(linked_persona(
            test_operator_keys(),
            false,
            "protected",
            100,
        )),
    ] {
        *control.events.lock().expect("events") = vec![instance.clone()];
        if let Some(pack) = pack {
            control.events.lock().expect("events").push(pack);
        }
        let failure = host
            .conversation_instructions(
                &request.id.to_hex(),
                &test_operator_keys().public_key().to_hex(),
            )
            .await
            .expect_err("unavailable");
        assert_eq!(failure.code, "AGENT_DEFINITION_PACK_UNAVAILABLE");
        assert!(!host.state.is_command_consumed(&request.id.to_hex()));
        assert!(host.in_flight.is_empty());
    }
}

#[tokio::test]
async fn linked_definition_cannot_switch_runtime_model_or_provider_custody() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let instance = linked_instance(&actor);
    let (relay, control, _server) =
        spawn_recording_test_relay(&host.config.keys, vec![instance.clone()]).await;
    host.set_rest_client(relay.rest_client());
    let request = message(channel, &actor, None, test_operator_keys());
    host.conversation_command(channel, &request)
        .expect("route")
        .expect("command");
    for (field, value, code) in [
        ("runtime", "uninstalled", "AGENT_RUNTIME_UPDATE_PENDING"),
        ("model", "unavailable-model", "AGENT_MODEL_UPDATE_PENDING"),
        (
            "provider",
            "unestablished-account",
            "AGENT_PROVIDER_UPDATE_PENDING",
        ),
    ] {
        let mut body =
            serde_json::json!({"display_name":"Linked definition", "system_prompt":"Behavior"});
        body[field] = serde_json::json!(value);
        let pack = nostr::EventBuilder::new(nostr::Kind::Custom(30175), body.to_string())
            .tags([
                Tag::parse(["d", "linked-persona"]).expect("coordinate"),
                Tag::parse(["shared", "true"]).expect("shared"),
            ])
            .sign_with_keys(test_operator_keys())
            .expect("signed pack");
        *control.events.lock().expect("events") = vec![instance.clone(), pack];
        let failure = host
            .conversation_instructions(
                &request.id.to_hex(),
                &test_operator_keys().public_key().to_hex(),
            )
            .await
            .expect_err("binding change pending");
        assert_eq!(failure.code, code);
        assert!(!host.state.is_command_consumed(&request.id.to_hex()));
        assert!(host.in_flight.is_empty());
    }
}

#[tokio::test]
async fn inline_definition_cannot_claim_an_unestablished_provider_update() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let definition = nostr::EventBuilder::new(nostr::Kind::Custom(30177), serde_json::json!({"name":"Inline participant", "system_prompt":"Behavior", "provider":"unestablished-account", "parallelism":1,"respond_to":"owner-only"}).to_string())
        .tags([Tag::parse(["d", &actor.public_key().to_hex()]).expect("coordinate")])
        .sign_with_keys(test_operator_keys()).expect("definition");
    let (relay, _, _server) = spawn_test_relay(&host.config.keys, Some(definition)).await;
    host.set_rest_client(relay.rest_client());
    let request = message(channel, &actor, None, test_operator_keys());
    host.conversation_command(channel, &request)
        .expect("route")
        .expect("command");
    let failure = host
        .conversation_instructions(
            &request.id.to_hex(),
            &test_operator_keys().public_key().to_hex(),
        )
        .await
        .expect_err("account change pending");
    assert_eq!(failure.code, "AGENT_PROVIDER_UPDATE_PENDING");
    assert!(!host.state.is_command_consumed(&request.id.to_hex()));
    assert!(host.in_flight.is_empty());
}

#[tokio::test]
async fn stopped_responsibility_does_not_make_successor_ambiguous_or_redirect_old_thread() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, old) = routed_provider(dir.path(), channel, &actor).await;
    let original = message(channel, &actor, None, test_operator_keys());
    host.conversation_command(channel, &original)
        .expect("bind old")
        .expect("command");
    let mut successor = host
        .state
        .session(&old.session_id)
        .expect("old record")
        .clone();
    successor.session_id = Uuid::new_v4().to_string();
    let current = host.target_for(&successor);
    host.state
        .insert_session(successor)
        .expect("accepted successor fixture");
    host.state
        .update_session(&old.session_id, |record| record.closed = true)
        .expect("terminal predecessor");
    // A stale route for the same live session must not count as another choice.
    let mut stale = current.clone();
    stale.generation += 1;
    install_routes(
        &host,
        channel,
        &actor,
        vec![old.clone(), current.clone(), stale],
    );
    let new_request = nostr::EventBuilder::new(nostr::Kind::Custom(9), "new successor work")
        .tags(original.tags.clone())
        .sign_with_keys(test_operator_keys())
        .expect("new request");
    let routed = host
        .conversation_command(channel, &new_request)
        .expect("new route")
        .expect("one active target");
    assert_eq!(
        commands::decode_turn_command(&routed)
            .expect("command")
            .target,
        current
    );
    let old_followup = message(
        channel,
        &actor,
        Some(&original.id.to_hex()),
        test_operator_keys(),
    );
    let routed = host
        .conversation_command(channel, &old_followup)
        .expect("bound route")
        .expect("historical target");
    assert_eq!(
        commands::decode_turn_command(&routed)
            .expect("command")
            .target,
        old
    );
    host.handle_command_event(channel, &old_followup)
        .await
        .expect("terminal refusal");
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("receipts");
    assert!(sink
        .all()
        .iter()
        .any(|event| event.kind == nostr::Kind::Custom(9)
            && event.content.contains("SESSION_CLOSED")));
    assert!(host.in_flight.is_empty());
}

#[tokio::test]
async fn declined_ambiguous_request_stays_declined_after_stop_and_worker_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, first) = routed_provider(dir.path(), channel, &actor).await;
    let mut second = host
        .state
        .session(&first.session_id)
        .expect("record")
        .clone();
    second.session_id = Uuid::new_v4().to_string();
    let other = host.target_for(&second);
    host.state
        .insert_session(second)
        .expect("second responsibility fixture");
    install_routes(&host, channel, &actor, vec![first.clone(), other.clone()]);
    let declined = message(channel, &actor, None, test_operator_keys());
    host.handle_command_event(channel, &declined)
        .await
        .expect("ambiguity disposition");
    assert!(host.state.is_command_consumed(&declined.id.to_hex()));
    assert!(host.in_flight.is_empty());
    host.state
        .update_session(&other.session_id, |record| record.closed = true)
        .expect("one responsibility stopped");
    let rest = host.rest_client.clone();
    let mut config = host.config.clone();
    let live_sessions: Vec<_> = host
        .state
        .sessions()
        .map(|record| record.session_id.clone())
        .collect();
    for session_id in live_sessions {
        host.sessions
            .shutdown_and_join(&session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    drop(host);
    let log = dir
        .path()
        .join("checkout")
        .join("declined-native-methods.log");
    config.runtimes[0].agent_command = fake_agent(
        dir.path(),
        "declined-native-loader",
        &crate::session::testing::restorable_agent(&log.to_string_lossy()),
    );
    let mut restarted = Provider::new(config).expect("worker restart");
    restarted.rest_client = rest;
    restarted.set_relay_self(restarted.config.keys.public_key().to_hex());
    restarted
        .handle_command_event(channel, &declined)
        .await
        .expect("declined replay");
    assert!(restarted.in_flight.is_empty());
    let fresh = nostr::EventBuilder::new(nostr::Kind::Custom(9), "explicit new selection")
        .tags(declined.tags.clone())
        .sign_with_keys(test_operator_keys())
        .expect("fresh event");
    let routed = restarted
        .conversation_command(channel, &fresh)
        .expect("new selection")
        .expect("active responsibility");
    assert_eq!(
        commands::decode_turn_command(&routed)
            .expect("command")
            .target,
        first
    );
    restarted
        .handle_command_event(channel, &fresh)
        .await
        .expect("new event executes");
    pump_until_turn_finished(&mut restarted).await;
    assert!(restarted.state.is_command_consumed(&fresh.id.to_hex()));
    let methods = std::fs::read_to_string(&log).expect("executed native methods");
    assert_eq!(
        methods
            .lines()
            .filter(|method| *method == "session/prompt")
            .count(),
        1
    );
    assert!(!methods.lines().any(|method| method == "session/new"));
    // The original signed denial remains publishable after restart.
    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("durable reply");
    assert_eq!(
        sink.all()
            .iter()
            .filter(|event| event.kind == nostr::Kind::Custom(9)
                && event.content.contains("multiple responsibilities"))
            .count(),
        1
    );
}

#[tokio::test]
async fn busy_ownership_terminal_answer_survives_later_watermark_and_outer_recovery() {
    use beekeeper_host_core::{layout::Instance, ownership};
    let dir = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("home");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    ownership::record_host_enrollment(
        home.path(),
        Instance::Dev,
        &actor.public_key().to_hex(),
        &host.config.relay_url,
        &Uuid::new_v4().to_string(),
        &"ab".repeat(32),
        Some(target),
    )
    .expect("modern enrollment");
    host.conversation_ownership_context = Some((home.path().to_path_buf(), Instance::Dev));
    let busy = message(channel, &actor, None, test_operator_keys());
    let lock = ownership::admission_lock(
        home.path(),
        Instance::Dev,
        &actor.public_key().to_hex(),
        &host.config.relay_url,
    )
    .expect("actual held admission barrier");
    host.handle_command_event(channel, &busy)
        .await
        .expect("outer busy handling");
    assert!(host.state.is_command_consumed(&busy.id.to_hex()));
    assert!(host.in_flight.is_empty());
    drop(lock);
    let later = nostr::EventBuilder::new(nostr::Kind::Custom(9), "later available request")
        .custom_created_at(nostr::Timestamp::from(busy.created_at.as_secs() + 1))
        .tags(busy.tags.clone())
        .sign_with_keys(test_operator_keys())
        .expect("later event");
    host.handle_command_event(channel, &later)
        .await
        .expect("outer later handling");
    pump_until_turn_finished(&mut host).await;
    assert!(host.state.is_command_consumed(&later.id.to_hex()));
    assert!(host
        .state
        .watermark(channel)
        .is_some_and(|mark| mark > busy.created_at.as_secs()));
    let config = host.config.clone();
    let live_sessions: Vec<_> = host
        .state
        .sessions()
        .map(|record| record.session_id.clone())
        .collect();
    for session_id in live_sessions {
        host.sessions
            .shutdown_and_join(&session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    drop(host);
    let mut restarted = Provider::new(config).expect("outer recovery worker restart");
    restarted.conversation_ownership_context = Some((home.path().to_path_buf(), Instance::Dev));
    restarted
        .handle_command_event(channel, &busy)
        .await
        .expect("terminal busy replay");
    assert!(restarted.in_flight.is_empty());
    let sink = CollectingSink::new();
    restarted
        .flush(&sink)
        .await
        .expect("durable correlated dispositions");
    let replies = sink.all();
    let busy_replies: Vec<_> = replies
        .iter()
        .filter(|event| {
            event.kind == nostr::Kind::Custom(9)
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice().get(1) == Some(&busy.id.to_hex()))
        })
        .collect();
    assert_eq!(busy_replies.len(), 1);
    assert!(busy_replies[0]
        .content
        .contains("This host has not executed"));
    assert!(replies
        .iter()
        .any(|event| event.kind == nostr::Kind::Custom(9)
            && event
                .tags
                .iter()
                .any(|tag| tag.as_slice().get(1) == Some(&later.id.to_hex()))
            && !event.content.starts_with("Request ")));
}

#[path = "conversation_policy_replay_tests.rs"]
mod policy_replay;
