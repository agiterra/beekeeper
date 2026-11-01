use super::*;

async fn stopped_unprompted(
    dir: &std::path::Path,
    channel: Uuid,
    actor: &Keys,
    stub: &str,
    edit: impl FnOnce(&mut crate::config::Config),
) -> (Provider, CodingSessionTarget) {
    let (mut host, target) = routed_provider(dir, channel, actor).await;
    host.state
        .update_session(&target.session_id, |record| {
            record.native_history_started = false;
        })
        .expect("an unprompted generation");
    let rest = host.rest_client.clone();
    let mut config = host.config.clone();
    let live: Vec<_> = host
        .state
        .sessions()
        .map(|r| r.session_id.clone())
        .collect();
    for session_id in live {
        host.sessions
            .shutdown_and_join(&session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    drop(host);
    config.runtimes[0].agent_command = fake_agent(dir, "review-stub", stub);
    edit(&mut config);
    let mut host = Provider::new(config).expect("restart");
    host.rest_client = rest;
    host.set_relay_self(host.config.keys.public_key().to_hex());
    host.recover().await.expect("crash reconciliation");
    (host, target)
}

/// one operator turn at a stopped, never-prompted generation.
/// It is answered NO_LIVE_EXECUTION, so no prompt exists. Control: the existing
/// `unprompted_conversation_after_worker_restart_...` test, which differs only
/// by not sending this turn, goes fresh.
#[tokio::test]
async fn an_undelivered_turn_does_not_strand_the_generation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let log = dir.path().join("checkout").join("p1-methods.log");
    let stub = crate::session::testing::load_rejecting_agent(&log.to_string_lossy());
    let (mut host, target) = stopped_unprompted(dir.path(), channel, &actor, &stub, |_| {}).await;
    let turn = turn_event(channel, "p1-operator-turn", &target);
    host.handle_command_event(channel, &turn)
        .await
        .expect("turn");
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("flush");
    let dropped = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "p1-operator-turn")
        .expect("answered");
    assert_eq!(dropped["status"], "turn_dropped", "{dropped}");
    assert_eq!(dropped["error"]["code"], "NO_LIVE_EXECUTION", "{dropped}");
    let fact = host
        .state
        .session(&target.session_id)
        .unwrap()
        .native_history_started;
    assert_eq!(
        std::fs::read_to_string(&log).unwrap_or_default(),
        "",
        "the undelivered turn never reached an adapter"
    );
    let request = message(channel, &actor, None, test_operator_keys());
    host.handle_command_event(channel, &request)
        .await
        .expect("mention");
    pump_until_turn_finished(&mut host).await;
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("flush");
    let answer: Vec<_> = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .filter(|receipt| receipt["commandId"] == request.id.to_hex())
        .map(|receipt| receipt["status"].clone())
        .collect();
    assert!(
        answer.iter().any(|status| status == "turn_started"),
        "{answer:?}"
    );
    let methods = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !fact,
        "no prompt was delivered, yet the fact now says one may have been"
    );
    assert!(methods.lines().any(|m| m == "session/new"), "{methods}");
}

/// Desktop's Claude descriptor declares PromptRequired, so a
/// mention takes dispatch_native_steer -> idle -> the boundary fallback, not
/// the ordinary turn path the shipped test exercises.
#[tokio::test]
async fn the_native_steer_fallback_marks_before_the_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let log = dir.path().join("checkout").join("p2-methods.log");
    let stub = format!(
        r#"
while IFS= read -r line; do
  printf '%s\n' "$line" | sed -n 's/.*"method":"\([a-z/_]*\)".*/\1/p' >> "{0}"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2,"agentCapabilities":{{"loadSession":true}},"_meta":{{"steering":{{"supported":true}}}}}}}}\n' "$id" ;;
    *'"method":"session/load"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"error":{{"code":-32002,"message":"Resource not found"}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"a-brand-new-conversation"}}}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id" ;;
  esac
done
"#,
        log.to_string_lossy()
    );
    let (mut host, target) = stopped_unprompted(dir.path(), channel, &actor, &stub, |c| {
        c.runtimes[0].steer_idle_guard = Some(SteerIdleGuard::PromptRequired);
    })
    .await;
    let request = message(channel, &actor, None, test_operator_keys());
    host.handle_command_event(channel, &request)
        .await
        .expect("mention");
    assert!(
        host.native_steer_deliverable(&target.session_id),
        "the guarded path"
    );
    assert!(
        !host
            .state
            .session(&target.session_id)
            .unwrap()
            .native_history_started,
        "only the fallback's delivery sets the fact on this path"
    );
    pump_until_turn_finished(&mut host).await;
    let record = host.state.session(&target.session_id).unwrap();
    let methods = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        methods.lines().filter(|m| *m == "session/prompt").count(),
        1,
        "{methods}"
    );
    assert!(!methods.lines().any(|m| m == "session/load"), "{methods}");
    assert!(
        record.native_history_started,
        "a prompt ran with the fact unset"
    );
}

/// the real idle-stop arm, not a shutdown_and_join simulation.
/// An actual actor runs out its idle timer, reports Exited(Idle), and the next
/// mention goes fresh in the same generation.
#[tokio::test]
async fn a_real_idle_exit_reaches_the_fresh_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let log = dir.path().join("checkout").join("p4-methods.log");
    let stub = crate::session::testing::load_rejecting_agent(&log.to_string_lossy());
    let (mut host, target) = stopped_unprompted(dir.path(), channel, &actor, &stub, |c| {
        c.session_idle_shutdown = std::time::Duration::from_millis(300);
    })
    .await;
    host.restore_generation(&target.session_id, None)
        .await
        .expect("open")
        .expect("fresh open");
    pump_until(&mut host, |event| {
        matches!(
            event,
            session::SessionEvent::Exited {
                reason: session::ExitReason::Idle,
                ..
            }
        )
    })
    .await;
    assert!(!host
        .sessions
        .handle(&target.session_id)
        .is_some_and(session::SessionHandle::is_live));
    let request = message(channel, &actor, None, test_operator_keys());
    host.handle_command_event(channel, &request)
        .await
        .expect("mention");
    pump_until_turn_finished(&mut host).await;
    let record = host.state.session(&target.session_id).unwrap();
    let methods = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(record.generation, target.generation);
    assert_eq!(
        methods.lines().filter(|m| *m == "session/new").count(),
        2,
        "{methods}"
    );
    assert_eq!(
        methods.lines().filter(|m| *m == "session/load").count(),
        0,
        "{methods}"
    );
    assert_eq!(
        methods.lines().filter(|m| *m == "session/prompt").count(),
        1,
        "{methods}"
    );
    assert!(record.native_history_started);
}

/// R2 at the call site: the fact is written before delivery, so a mark that
/// cannot be written delivers nothing. The retry is the control that this path
/// does deliver a prompt once the write can succeed.
#[tokio::test]
async fn a_durable_mark_that_fails_delivers_no_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let log = dir.path().join("checkout").join("mark-methods.log");
    let stub = crate::session::testing::load_rejecting_agent(&log.to_string_lossy());
    let (mut host, target) = stopped_unprompted(dir.path(), channel, &actor, &stub, |_| {}).await;
    host.restore_generation(&target.session_id, None)
        .await
        .expect("open")
        .expect("fresh open");
    host.state.fault_plan().fail_next_history_mark = true;
    let turn = turn_event(channel, "mark-turn", &target);
    host.handle_command_event(channel, &turn)
        .await
        .expect_err("the failed write stops the delivery");
    assert!(
        !host.state.fault_plan().fail_next_history_mark,
        "the mark was reached"
    );
    pump_available(&mut host).await;
    let prompts = |log: &std::path::Path| {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .filter(|method| *method == "session/prompt")
            .count()
    };
    assert_eq!(prompts(&log), 0, "no prompt without the durable fact");
    assert!(
        !host
            .state
            .session(&target.session_id)
            .unwrap()
            .native_history_started
    );
    let retry = turn_event(channel, "mark-turn-retry", &target);
    host.handle_command_event(channel, &retry)
        .await
        .expect("retry");
    pump_until_turn_finished(&mut host).await;
    assert_eq!(prompts(&log), 1);
    assert!(
        host.state
            .session(&target.session_id)
            .unwrap()
            .native_history_started
    );
}
