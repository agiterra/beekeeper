use super::*;
use crate::conversation_project::{admitted_reply_origin, ProjectContext};

async fn project_host(
    dir: &Path,
    channel: Uuid,
    actor: &Keys,
) -> (Provider, CodingSessionTarget, ProjectContext) {
    let project = format!("30621:{}:fixture", test_operator_keys().public_key());
    let (host, target) = routed_provider_for_project(dir, channel, actor, Some(&project)).await;
    let genesis = beekeeper_sdk::build_coding_session_genesis(
        channel,
        &beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload::new(
            Uuid::new_v4().to_string(),
        ),
    )
    .unwrap()
    .sign_with_keys(test_operator_keys())
    .unwrap();
    let scope = ProjectContext {
        project,
        channel,
        actor: actor.public_key().to_hex(),
        genesis,
        hire_selection: None,
    };
    install_scope(&host, &scope);
    (host, target, scope)
}
fn install_scope(host: &Provider, scope: &ProjectContext) {
    let path = host
        .config
        .state_dir
        .join("conversation-projects")
        .join(format!("{}.json", scope.actor));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    state::atomic_write(&path, &serde_json::to_vec(scope).unwrap()).unwrap();
}
fn reply(
    host: &Provider,
    scope: &ProjectContext,
    source: &Event,
) -> anyhow::Result<beekeeper_sdk::ThreadRef> {
    admitted_reply_origin(
        &host.config.state_dir,
        &scope.actor,
        scope.channel,
        &source.id.to_hex(),
        &scope.project,
        &scope.genesis.id.to_hex(),
    )
}
async fn shutdown(host: &mut Provider) {
    let ids: Vec<_> = host
        .state
        .sessions()
        .map(|record| record.session_id.clone())
        .collect();
    for id in ids {
        host.sessions
            .shutdown_and_join(&id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
}

#[tokio::test]
async fn conversation_return_requires_actual_admission_not_error_origin_or_current_scope() {
    for branch in [
        "author_verification",
        "invalid_payload",
        "runtime_refusal",
        "historically_unbound",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let channel = Uuid::new_v4();
        let actor = Keys::generate();
        let (mut host, _, scope) = project_host(dir.path(), channel, &actor).await;
        let mut source = message(channel, &actor, None, test_operator_keys());
        if branch == "author_verification" {
            let (relay, _, _server) = spawn_test_relay_with_events(&host.config.keys, vec![]).await;
            host.set_rest_client(relay.rest_client());
        }
        if branch == "invalid_payload" {
            source = nostr::EventBuilder::new(Kind::Custom(9), "x".repeat(140_000))
                .tags(source.tags)
                .sign_with_keys(test_operator_keys())
                .unwrap();
        }
        if branch == "runtime_refusal" {
            let original = policy_replay::policy_definition(&actor, "owner-only", vec![]);
            let mut content: serde_json::Value = serde_json::from_str(&original.content).unwrap();
            content["model"] = serde_json::json!("different-model");
            let definition = nostr::EventBuilder::new(original.kind, content.to_string())
                .tags(original.tags)
                .sign_with_keys(test_operator_keys())
                .unwrap();
            let (relay, _, _server) =
                spawn_test_relay_with_events(&host.config.keys, vec![definition]).await;
            host.set_rest_client(relay.rest_client());
        }
        if branch == "historically_unbound" {
            std::fs::remove_file(
                host.config
                    .state_dir
                    .join("conversation-projects")
                    .join(format!("{}.json", scope.actor)),
            )
            .unwrap();
        }
        Box::pin(host.handle_command_event(channel, &source))
            .await
            .unwrap();
        if branch == "historically_unbound" {
            pump_until_turn_finished(&mut host).await;
        }
        install_scope(&host, &scope); // Today's valid scope is not historical admission.
        assert!(reply(&host, &scope, &source).is_err(), "{branch}");
        let state: serde_json::Value = serde_json::from_slice(
            &std::fs::read(host.config.state_dir.join("conversation-origins.json")).unwrap(),
        )
        .unwrap();
        assert!(
            state["requests"][source.id.to_hex()].is_object(),
            "actual negative branch retained its routing origin: {branch}"
        );
        let config = host.config.clone();
        shutdown(&mut host).await;
        drop(host);
        let restarted = Provider::new(config).unwrap();
        assert!(
            reply(&restarted, &scope, &source).is_err(),
            "restart: {branch}"
        );
    }
}

#[tokio::test]
async fn conversation_admission_pins_author_execution_project_and_genesis_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _, scope) = project_host(dir.path(), channel, &actor).await;
    let source = message(channel, &actor, None, test_operator_keys());
    Box::pin(host.handle_command_event(channel, &source))
        .await
        .unwrap();
    pump_until_turn_finished(&mut host).await;
    assert_eq!(
        reply(&host, &scope, &source).unwrap().parent_event_id,
        source.id
    );
    let mut other = scope.clone();
    other.project.push_str("-other");
    install_scope(&host, &other);
    assert!(reply(&host, &other, &source).is_err());
    other = scope.clone();
    other.genesis = beekeeper_sdk::build_coding_session_genesis(
        channel,
        &beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload::new(
            Uuid::new_v4().to_string(),
        ),
    )
    .unwrap()
    .sign_with_keys(test_operator_keys())
    .unwrap();
    install_scope(&host, &other);
    assert!(reply(&host, &other, &source).is_err());
    let path = host.config.state_dir.join("conversation-origins.json");
    let original = std::fs::read(&path).unwrap();
    for field in ["author", "target", "admission", "signedSource"] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        if field == "target" {
            value["requests"][source.id.to_hex()][field]["sessionId"] =
                serde_json::json!("wrong-execution");
        } else {
            value["requests"][source.id.to_hex()]
                .as_object_mut()
                .unwrap()
                .remove(field);
        }
        state::atomic_write(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            reply(&host, &scope, &source).is_err(),
            "missing/different {field}"
        );
    }
    state::atomic_write(&path, &original).unwrap();
    let config = host.config.clone();
    shutdown(&mut host).await;
    drop(host);
    let mut restarted = Provider::new(config.clone()).unwrap();
    assert!(reply(&restarted, &scope, &source).is_ok());
    let target = restarted.target_for(restarted.state.sessions().next().unwrap());
    for receipt in [
        crate::payload::LifecycleReceipt::turn_refused(
            &source.id.to_hex(),
            &target,
            "TEST_REFUSED",
            "refused",
        ),
        crate::payload::LifecycleReceipt::turn_dropped(
            &source.id.to_hex(),
            &target,
            "TEST_DROPPED",
            "dropped",
        ),
    ] {
        state::atomic_write(&path, &original).unwrap();
        restarted
            .conversation_receipt(&source.id.to_hex(), &receipt)
            .unwrap();
        restarted
            .conversation_receipt(
                &source.id.to_hex(),
                &crate::payload::LifecycleReceipt::turn_queued(&source.id.to_hex(), &target),
            )
            .unwrap();
        assert!(
            reply(&restarted, &scope, &source).is_err(),
            "later positive receipt cannot resurrect a refused source"
        );
        let recovered = Provider::new(config.clone()).unwrap();
        assert!(reply(&recovered, &scope, &source).is_err());
    }
}

#[tokio::test]
async fn conversation_host_refusal_preserves_typed_origin_through_replay_and_restart() {
    for (restart, fault) in [
        (false, None),
        (true, None),
        (true, Some(0)),
        (true, Some(1)),
        (true, Some(2)),
    ] {
        Box::pin(host_notice_recovery_case(restart, fault)).await;
    }
}
async fn drive_notice_maintenance(
    host: &mut Provider,
    receiver: &mut Option<tokio::sync::mpsc::Receiver<crate::conversation::backfill::Fetched>>,
    now: u64,
) {
    host.start_conversation_backfill_at(now).unwrap();
    let fetched = tokio::time::timeout(Duration::from_secs(5), off_loop::next(receiver))
        .await
        .unwrap();
    Box::pin(host.finish_conversation_backfill(fetched))
        .await
        .unwrap();
    host.flush_replays_now().await.unwrap();
}
async fn host_notice_recovery_case(restart: bool, fault: Option<u8>) {
    use beekeeper_core::coding_session_lifecycle_command::{
        CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
        CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
    };
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _, scope) = project_host(dir.path(), channel, &actor).await;
    let root = message(channel, &actor, None, test_operator_keys());
    let source = message(
        channel,
        &actor,
        Some(&root.id.to_hex()),
        test_operator_keys(),
    );
    let source = nostr::EventBuilder::new(source.kind, source.content)
        .tags(source.tags)
        .tags([Tag::parse(["p", &actor.public_key().to_hex()]).unwrap()])
        .sign_with_keys(test_operator_keys())
        .unwrap();
    Box::pin(host.handle_command_event(channel, &source))
        .await
        .unwrap();
    pump_until_turn_finished(&mut host).await;
    let thread = reply(&host, &scope, &source).unwrap();
    let hire = beekeeper_sdk::build_coding_session_lifecycle_command(
        channel,
        &CodingSessionLifecycleCommandPayload {
            schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: Uuid::new_v4().to_string(),
            action: CodingSessionLifecycleAction::SessionHire {
                session_ref: Uuid::new_v4().to_string(),
                genesis_ref: scope.genesis.id.to_hex(),
                role: "runner".into(),
                provider_instance_ref: None,
                model: Some("fixture-bare".into()),
                brief: "bounded work".into(),
                requested_by: Some(actor.public_key().to_hex()),
                routing: None,
                reply_to: Some(source.id.to_hex()),
            },
        },
    )
    .unwrap()
    .sign_with_keys(&actor)
    .unwrap();
    let definition = policy_replay::policy_definition(&actor, "owner-only", vec![]);
    let (relay, _, _server) =
        spawn_test_relay_with_events(&host.config.keys, vec![definition, hire.clone()]).await;
    host.set_rest_client(relay.rest_client());
    let build = |keys: &Keys, host_tag: bool| {
        let mut builder = beekeeper_sdk::build_message(
            channel,
            "hire refused: HIRE_MODEL_NOT_OFFERED — exact selection required",
            Some(&thread),
            &[&actor.public_key().to_hex()],
            false,
            &[],
        )
        .unwrap()
        .tags([Tag::parse(["hire-ref", &hire.id.to_hex()]).unwrap()]);
        if host_tag {
            builder = builder.tags([Tag::parse(["buzz-host-answer", "hire"]).unwrap()]);
        }
        builder.sign_with_keys(keys).unwrap()
    };
    let valid_shape = build(test_operator_keys(), true);
    let malformed = nostr::EventBuilder::new(valid_shape.kind, valid_shape.content)
        .tags(valid_shape.tags)
        .tags([Tag::parse(["hire-ref", &"b".repeat(64)]).unwrap()])
        .sign_with_keys(test_operator_keys())
        .unwrap();
    for forged in [
        build(&actor, true),
        build(&Keys::generate(), true),
        build(test_operator_keys(), false),
        malformed,
    ] {
        Box::pin(host.handle_command_event(channel, &forged))
            .await
            .unwrap();
        if forged.pubkey == test_operator_keys().public_key() {
            assert!(host.state.is_command_refused(&forged.id.to_hex()));
        }
        assert!(!host
            .conversation_is_host_answer(&forged.id.to_hex())
            .unwrap());
    }
    let before: serde_json::Value = serde_json::from_slice(
        &std::fs::read(host.config.state_dir.join("conversation-origins.json")).unwrap(),
    )
    .unwrap();
    assert!(
        before["pending_notices"].as_object().unwrap().is_empty(),
        "foreign/missing-Host inputs never enter retry work"
    );
    // Complete responsibility backfill before the live failure; its checked bit
    // must not be the owner of this subsequent notice.
    let route_path = host.config.state_dir.join("conversation-routes.json");
    let mut routes: ConversationRoutes =
        serde_json::from_slice(&std::fs::read(&route_path).unwrap()).unwrap();
    routes.routes[0].replay_floor_unix = Some(crate::now_secs().saturating_sub(1));
    state::atomic_write(&route_path, &serde_json::to_vec(&routes).unwrap()).unwrap();
    let mut receiver = host.conversation_backfill.slot.take_receiver();
    drive_notice_maintenance(&mut host, &mut receiver, crate::now_secs()).await;
    host.start_conversation_backfill().unwrap();
    assert!(
        !host.conversation_backfill.slot.busy(),
        "backfill completed"
    );
    let notice = build(test_operator_keys(), true);
    let (missing, _, _missing_server) = spawn_test_relay_with_events(
        &host.config.keys,
        vec![policy_replay::policy_definition(
            &actor,
            "owner-only",
            vec![],
        )],
    )
    .await;
    host.set_rest_client(missing.rest_client());
    let error = Box::pin(host.handle_command_event(channel, &notice))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("HOST_NOTICE_LOOKUP_PENDING"));
    assert!(!host.state.is_command_refused(&notice.id.to_hex()));
    assert!(!host.state.is_command_consumed(&notice.id.to_hex()));
    host.set_rest_client(relay.rest_client());
    let config = host.config.clone();
    shutdown(&mut host).await;
    if restart {
        drop(host);
        host = Provider::new(config.clone()).unwrap();
        host.set_rest_client(relay.rest_client());
        receiver = host.conversation_backfill.slot.take_receiver();
    }
    host.conversation_backfill.notice_write_fault = fault;
    let mut saved_transcript = None;
    drive_notice_maintenance(&mut host, &mut receiver, crate::now_secs() + 3).await;
    if fault.is_some() {
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(host.config.state_dir.join("conversation-origins.json")).unwrap(),
        )
        .unwrap();
        saved_transcript = serde_json::from_value::<Event>(
            saved["requests"][notice.id.to_hex()]["hostNotice"]["transcript"].clone(),
        )
        .ok();
        assert_eq!(saved_transcript.is_some(), fault != Some(0));
        assert!(!host.state.is_command_consumed(&notice.id.to_hex()));
        drop(host);
        host = Provider::new(config.clone()).unwrap();
        host.set_rest_client(relay.rest_client());
        receiver = host.conversation_backfill.slot.take_receiver();
        drive_notice_maintenance(&mut host, &mut receiver, crate::now_secs() + 310).await;
    }
    assert!(host.state.is_command_consumed(&notice.id.to_hex()));
    assert_eq!(
        host.sessions.live_count(),
        0,
        "maintenance notice opens no model actor"
    );
    let origins: serde_json::Value = serde_json::from_slice(
        &std::fs::read(host.config.state_dir.join("conversation-origins.json")).unwrap(),
    )
    .unwrap();
    let stored = &origins["requests"][notice.id.to_hex()]["hostNotice"];
    assert_eq!(stored["hire"], hire.id.to_hex());
    assert_eq!(stored["source"], source.id.to_hex());
    assert_eq!(
        stored["sourceAuthor"],
        test_operator_keys().public_key().to_hex()
    );
    let exact: Event = serde_json::from_value(stored["transcript"].clone()).unwrap();
    if let Some(saved) = saved_transcript {
        assert_eq!(
            saved, exact,
            "saved signed bytes must survive enqueue failure/restart"
        );
    }
    assert!(exact.content.contains("\"hostAnswer\":true"));
    assert!(exact.content.contains("host_hire_refusal"));
    assert!(!exact.content.contains("human_override"));
    drop(host); // Unacknowledged exact transcript remains in durable outbox.
    let mut host = Provider::new(config).unwrap();
    host.set_rest_client(relay.rest_client());
    let sink = CollectingSink::new();
    host.flush(&sink).await.unwrap();
    assert_eq!(
        sink.all()
            .iter()
            .filter(|event| event.content.contains("\"hostAnswer\":true"))
            .count(),
        1,
        "one Host transcript, not merely one matching ID"
    );
    assert_eq!(
        sink.all()
            .iter()
            .filter(|event| event.id == exact.id)
            .count(),
        1
    );
    Box::pin(host.handle_command_event(channel, &notice))
        .await
        .unwrap();
    host.flush(&sink).await.unwrap();
    assert_eq!(
        sink.all()
            .iter()
            .filter(|event| event.content.contains("\"hostAnswer\":true"))
            .count(),
        1,
        "one Host transcript, not merely one matching ID"
    );
    assert_eq!(
        sink.all()
            .iter()
            .filter(|event| event.id == exact.id)
            .count(),
        1
    );
}

#[tokio::test]
async fn conversation_notice_recovery_bounds_capacity_backoff_and_exhaustion() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _, scope) = project_host(dir.path(), channel, &actor).await;
    let source = message(channel, &actor, None, test_operator_keys());
    Box::pin(host.handle_command_event(channel, &source))
        .await
        .unwrap();
    pump_until_turn_finished(&mut host).await;
    let thread = reply(&host, &scope, &source).unwrap();
    shutdown(&mut host).await;
    let path = host.config.state_dir.join("conversation-origins.json");
    for i in 0..65 {
        let notice = beekeeper_sdk::build_message(
            channel,
            &format!("bounded refusal fixture {i}"),
            Some(&thread),
            &[&actor.public_key().to_hex()],
            false,
            &[],
        )
        .unwrap()
        .tags([
            Tag::parse(["hire-ref", &"a".repeat(64)]).unwrap(),
            Tag::parse(["buzz-host-answer", "hire"]).unwrap(),
        ])
        .sign_with_keys(test_operator_keys())
        .unwrap();
        let result = Box::pin(host.handle_command_event(channel, &notice)).await;
        if i == 64 {
            result.unwrap();
            assert!(
                host.state.is_command_refused(&notice.id.to_hex()),
                "capacity refuses durably instead of claiming a retry owner"
            );
        } else {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("HOST_NOTICE_LOOKUP_PENDING"));
        }
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let pending = value["pending_notices"].as_object_mut().unwrap();
    assert_eq!(pending.len(), 64);
    let now = crate::now_secs();
    let first = pending.keys().next().unwrap().clone();
    for row in pending.values_mut() {
        row["nextAttempt"] = serde_json::json!(now + 100_000);
    }
    pending.get_mut(&first).unwrap()["nextAttempt"] = serde_json::json!(0);
    // Advance the persisted retry budget without 31 real wall-clock waits.
    pending.get_mut(&first).unwrap()["attempts"] = serde_json::json!(31);
    state::atomic_write(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
    let mut receiver = host.conversation_backfill.slot.take_receiver();
    drive_notice_maintenance(&mut host, &mut receiver, now).await;
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let parked = &value["pending_notices"][&first];
    assert_eq!(parked["attempts"], 32);
    assert_eq!(parked["nextAttempt"], now + 300);
    assert!(parked["error"]
        .as_str()
        .unwrap()
        .contains("HOST_NOTICE_RETRY_EXHAUSTED"));
    assert!(!host.state.is_command_consumed(&first));
    let config = host.config.clone();
    drop(host);
    let mut host = Provider::new(config).unwrap();
    let (relay, _, _server) = spawn_test_relay(&host.config.keys, None).await;
    host.set_rest_client(relay.rest_client());
    host.start_conversation_backfill_at(now + 301).unwrap();
    assert!(
        !host.conversation_backfill.slot.busy(),
        "parked retries do not reset on restart"
    );
    assert_eq!(host.sessions.live_count(), 0);
}
