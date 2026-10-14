use super::*;

fn fixture() -> (tempfile::TempDir, Uuid, Provider) {
    let dir = tempfile::tempdir().expect("fixture");
    let cwd = dir.path().join("ordinary-work");
    std::fs::create_dir(&cwd).expect("workspace");
    let channel = Uuid::new_v4();
    let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
    let provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
    (dir, channel, provider)
}

#[tokio::test]
async fn consumed_before_effects_create_recovers_same_reserved_target_once() {
    let (_dir, channel, mut provider) = fixture();
    let command = "crash-before-create-effects";
    let event = crate::tests::create_event(&provider, channel, command);
    provider
        .state
        .fault_plan()
        .fail_next_lifecycle_effects_write = true;
    assert!(provider
        .handle_command_event(channel, &event)
        .await
        .is_err());
    assert!(provider.state.is_command_consumed(command));
    assert_eq!(provider.sessions.live_count(), 0);
    assert_eq!(provider.state.sessions().count(), 0);
    let intent = provider
        .state
        .lifecycle_start(command)
        .expect("durable reservation");
    assert_eq!(intent.phase, state::LifecycleStartPhase::BeforeEffects);
    let config = provider.config.clone();
    drop(provider);
    let mut provider = Provider::new(config).expect("restart");
    provider.recover().await.expect("actual recovery");
    assert!(!provider.outbox.pending_events().any(|event| {
        payload::decode_coding_session_lifecycle_receipt(&event.content)
            .is_ok_and(|receipt| receipt.command_id == command)
    }));
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("same signed replay");
    let record = provider.state.sessions().next().expect("accepted record");
    assert_eq!(record.target(&provider.config.instance_id), intent.target);
    let pid = provider
        .sessions
        .handle(&intent.target.session_id)
        .expect("child")
        .child_pid_for_tests();
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("duplicate replay");
    assert_eq!(provider.state.sessions().count(), 1);
    assert_eq!(provider.sessions.live_count(), 1);
    assert_eq!(
        provider
            .sessions
            .handle(&intent.target.session_id)
            .expect("same child")
            .child_pid_for_tests(),
        pid
    );
    assert!(provider.outbox.pending_events().any(|event| {
        payload::decode_coding_session_lifecycle_receipt(&event.content).is_ok_and(|receipt| {
            receipt.command_id == command
                && receipt.status == payload::ReceiptStatus::Created
                && receipt.session == Some(intent.target.clone())
        })
    }));
    provider
        .sessions
        .shutdown_and_join(&intent.target.session_id, session::ACTOR_RETIRE_GRACE)
        .await;
}

#[cfg(unix)]
#[tokio::test]
async fn verified_stop_crash_before_projection_recovers_exact_signed_terminal_without_child() {
    let (_dir, channel, mut provider) = fixture();
    let create = crate::tests::create_event(&provider, channel, "create-stop-crash");
    provider
        .handle_command_event(channel, &create)
        .await
        .expect("actual ACP create");
    let target = provider
        .state
        .sessions()
        .next()
        .expect("record")
        .target(&provider.config.instance_id);
    let pid = provider
        .sessions
        .handle(&target.session_id)
        .expect("child")
        .child_pid_for_tests()
        .expect("pid");
    let command = "stop-projection-crash";
    let event =
        crate::tests::lifecycle_target_event(&provider, channel, command, "session.stop", &target);
    provider
        .state
        .fault_plan()
        .fail_next_verified_stop_projection = true;
    assert!(provider
        .handle_command_event(channel, &event)
        .await
        .is_err());
    assert!(nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid as i32), None).is_err());
    let proof = provider
        .state
        .terminal_dispositions()
        .into_iter()
        .find(|(id, _)| id == command)
        .expect("durable completion")
        .1
        .event;
    let marker = provider
        .config
        .state_dir
        .join(format!("stop-quiescence-{}.json", target.session_id));
    assert!(marker.exists());
    assert!(!provider
        .outbox
        .pending_events()
        .any(|event| event.id == proof.id));
    let config = provider.config.clone();
    drop(provider);
    let mut provider = Provider::new(config).expect("restart");
    provider.recover().await.expect("actual recovery");
    assert!(!marker.exists());
    assert_eq!(provider.sessions.live_count(), 0);
    assert!(
        provider
            .state
            .session(&target.session_id)
            .expect("retained record")
            .closed
    );
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("exact same signed stop replay");
    assert_eq!(
        provider
            .outbox
            .pending_events()
            .filter(|event| event.id == proof.id)
            .count(),
        1
    );
    assert_eq!(provider.sessions.live_count(), 0);
    let receipt =
        payload::decode_coding_session_lifecycle_receipt(&proof.content).expect("receipt");
    assert_eq!(receipt.status, payload::ReceiptStatus::Stopped);
    assert_eq!(receipt.session, Some(target));
}

#[tokio::test]
async fn crash_after_real_acp_start_without_record_recovers_target_bearing_uncertainty_and_no_reexecution(
) {
    let (_dir, channel, mut provider) = fixture();
    let command = "crash-after-real-create-start";
    let event = crate::tests::create_event(&provider, channel, command);
    provider.state.fault_plan().fail_next_create_record_write = true;
    assert!(provider
        .handle_command_event(channel, &event)
        .await
        .is_err());
    let intent = provider
        .state
        .lifecycle_start(command)
        .expect("reserved target");
    assert_eq!(intent.phase, state::LifecycleStartPhase::RuntimeStarting);
    assert_eq!(provider.state.sessions().count(), 0);
    assert_eq!(
        provider.sessions.live_count(),
        1,
        "actual ACP launch occurred"
    );
    // Simulate the old worker exiting; startup provenance still cannot certify
    // whether a first turn or custody side effect happened before that exit.
    provider
        .sessions
        .shutdown_and_join(&intent.target.session_id, session::ACTOR_RETIRE_GRACE)
        .await;
    let config = provider.config.clone();
    drop(provider);
    let mut provider = Provider::new(config).expect("restart");
    provider.recover().await.expect("actual recovery");
    let answer = provider
        .outbox
        .pending_events()
        .find_map(|event| {
            payload::decode_coding_session_lifecycle_receipt(&event.content)
                .ok()
                .filter(|r| r.command_id == command)
        })
        .expect("correlated outcome");
    assert_eq!(answer.status, payload::ReceiptStatus::Failed);
    assert_eq!(answer.session, Some(intent.target.clone()));
    assert_eq!(
        answer.error.expect("uncertain").code,
        CREATE_START_OUTCOME_UNCONFIRMED
    );
    assert!(provider
        .outbox
        .pending_events()
        .any(
            |event| payload::decode_coding_session_metadata(&event.content)
                .is_ok_and(|metadata| metadata.session == intent.target
                    && metadata.status == SessionStatus::Disconnected)
        ));
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("same signed replay abstains");
    let next =
        crate::tests::create_event(&provider, channel, "different-create-after-uncertain-start");
    provider
        .handle_command_event(channel, &next)
        .await
        .expect("different create fenced");
    assert_eq!(provider.sessions.live_count(), 0);
    assert_eq!(provider.state.sessions().count(), 0);
    assert!(provider.state.unresolved_start_target(&intent.target));
}

struct StopSink {
    park: bool,
    ids: std::sync::Mutex<Vec<nostr::EventId>>,
}
impl publish::EventSink for StopSink {
    async fn publish(&self, event: Event) -> Result<(), String> {
        self.ids.lock().expect("fixture sink").push(event.id);
        if self.park {
            Err("relay rejected durable event: invalid: fixture refusal".into())
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn delivered_or_parked_stop_recovery_never_reexecutes_or_invents_receipt_acceptance() {
    for park in [false, true] {
        let (_dir, channel, mut provider) = fixture();
        let create = crate::tests::create_event(&provider, channel, "create-stop-drained-outbox");
        provider
            .handle_command_event(channel, &create)
            .await
            .expect("real create");
        let target = provider
            .state
            .sessions()
            .next()
            .expect("record")
            .target(&provider.config.instance_id);
        let command = "stop-drained-outbox";
        let stop = crate::tests::lifecycle_target_event(
            &provider,
            channel,
            command,
            "session.stop",
            &target,
        );
        provider
            .handle_command_event(channel, &stop)
            .await
            .expect("verified stop");
        let exact = provider
            .outbox
            .pending_events()
            .find(|event| {
                payload::decode_coding_session_lifecycle_receipt(&event.content).is_ok_and(|r| {
                    r.command_id == command && r.status == payload::ReceiptStatus::Stopped
                })
            })
            .expect("signed proof")
            .id;
        let sink = StopSink {
            park,
            ids: std::sync::Mutex::new(Vec::new()),
        };
        provider
            .outbox
            .flush(&sink)
            .await
            .expect("durable delivered or parked drain");
        assert!(sink.ids.lock().expect("sink").contains(&exact));
        if park {
            assert!(provider.outbox.parked_len() > 0);
        }
        let config = provider.config.clone();
        drop(provider);
        let mut provider = Provider::new(config).expect("restart");
        provider.recover().await.expect("actual recovery");
        provider
            .handle_command_event(channel, &stop)
            .await
            .expect("same signed stop replay");
        assert_eq!(provider.sessions.live_count(), 0);
        assert!(
            provider
                .state
                .session(&target.session_id)
                .expect("record")
                .closed
        );
        assert!(
            !provider
                .outbox
                .pending_events()
                .any(|event| event.id == exact),
            "delivered does not duplicate; parked preserves existing explicit no-retry policy"
        );
        if park {
            assert!(provider.outbox.parked_len() > 0);
        }
    }
}

#[tokio::test]
async fn accepted_record_with_uncertain_initial_delivery_fences_explicit_resume_and_lazy_restore() {
    let (_dir, channel, mut provider) = fixture();
    let original = crate::tests::create_event(&provider, channel, "create-seeded-crash");
    let mut content: serde_json::Value = serde_json::from_str(&original.content).expect("content");
    content["action"]["initialTurn"] = serde_json::json!("ordinary request");
    let event = nostr::EventBuilder::new(
        original.kind,
        serde_json::to_string(&content).expect("body"),
    )
    .tags(original.tags.iter().cloned())
    .sign_with_keys(crate::tests::test_operator_keys())
    .expect("signed command");
    provider
        .state
        .fault_plan()
        .fail_next_create_initial_delivery = true;
    assert!(provider
        .handle_command_event(channel, &event)
        .await
        .is_err());
    let target = provider
        .state
        .sessions()
        .next()
        .expect("accepted record")
        .target(&provider.config.instance_id);
    assert!(provider.state.unresolved_start_target(&target));
    provider
        .sessions
        .shutdown_and_join(&target.session_id, session::ACTOR_RETIRE_GRACE)
        .await;
    let config = provider.config.clone();
    drop(provider);
    let mut provider = Provider::new(config).expect("restart");
    provider.recover().await.expect("actual recovery");
    assert!(
        matches!(provider.restore_generation(&target.session_id, None).await.expect("lazy restore refusal"),Err(native_restore::RestoreObstacle::Prerequisite(failure)) if failure.code==CREATE_START_OUTCOME_UNCONFIRMED)
    );
    let resume = crate::tests::lifecycle_target_event(
        &provider,
        channel,
        "resume-uncertain-create",
        "session.resume",
        &target,
    );
    provider
        .handle_command_event(channel, &resume)
        .await
        .expect("explicit refused resume");
    assert!(provider.outbox.pending_events().any(|event| {
        payload::decode_coding_session_lifecycle_receipt(&event.content).is_ok_and(|r| {
            r.command_id == "resume-uncertain-create"
                && r.error
                    .is_some_and(|error| error.code == CREATE_START_OUTCOME_UNCONFIRMED)
        })
    }));
    assert_eq!(provider.sessions.live_count(), 0);
    assert_eq!(
        provider
            .state
            .session(&target.session_id)
            .expect("record")
            .generation,
        1
    );
    assert!(provider.state.unresolved_start_target(&target));
}

#[tokio::test]
async fn actual_startup_refusal_retains_cause_and_accepted_uncertainty_does_not_republish() {
    let (dir, channel, mut provider) = fixture();
    provider.config.runtimes[0].agent_command =
        dir.path().join("absent-adapter").to_string_lossy().into();
    let command = "actual-startup-refusal-diagnostic";
    let event = crate::tests::create_event(&provider, channel, command);
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("correlated refusal");
    let intent = provider
        .state
        .lifecycle_start(command)
        .expect("uncertain fence retained");
    let answer = provider
        .outbox
        .pending_events()
        .find(|event| {
            payload::decode_coding_session_lifecycle_receipt(&event.content)
                .is_ok_and(|r| r.command_id == command)
        })
        .expect("answer")
        .clone();
    let receipt =
        payload::decode_coding_session_lifecycle_receipt(&answer.content).expect("decode");
    assert_eq!(receipt.session, Some(intent.target.clone()));
    let error = receipt.error.expect("diagnostic");
    assert_eq!(error.code, CREATE_START_OUTCOME_UNCONFIRMED);
    assert!(
        error.message.contains("PROVIDER_UNAVAILABLE"),
        "original code: {}",
        error.message
    );
    // The adapter does not exist. Where a sandbox backend exists (macOS) the
    // wrapper spawns and the absence surfaces at initialize; on a platform
    // with no backend the Legacy plan execs the absent path and the absence
    // surfaces at spawn. Either is the same fact, stated where it was found.
    assert!(
        error.message.contains("could not initialize the agent")
            || error.message.contains("could not spawn the agent"),
        "original cause: {}",
        error.message
    );
    assert!(provider.state.unresolved_start_target(&intent.target));
    assert_eq!(provider.sessions.live_count(), 0);
    let sink = StopSink {
        park: false,
        ids: std::sync::Mutex::new(Vec::new()),
    };
    provider
        .outbox
        .flush(&sink)
        .await
        .expect("accepted projection");
    assert_eq!(
        sink.ids
            .lock()
            .expect("sink")
            .iter()
            .filter(|id| **id == answer.id)
            .count(),
        1
    );
    for _ in 0..4 {
        provider
            .flush_terminal_dispositions()
            .expect("repeat projection");
        assert!(!provider
            .outbox
            .pending_events()
            .any(|event| event.id == answer.id || event.kind.as_u16() == 44223));
    }
    let config = provider.config.clone();
    drop(provider);
    let mut provider = Provider::new(config).expect("reopen accepted ledger");
    provider.recover().await.expect("actual recovery");
    assert!(provider.state.unresolved_start_target(&intent.target));
    for _ in 0..4 {
        provider
            .flush_terminal_dispositions()
            .expect("repeat recovered projection");
    }
    assert!(!provider
        .outbox
        .pending_events()
        .any(|event| event.id == answer.id || event.kind.as_u16() == 44223));
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("same signed replay refused");
    assert_eq!(provider.sessions.live_count(), 0);
    assert!(provider.state.unresolved_start_target(&intent.target));
    assert!(!provider
        .outbox
        .pending_events()
        .any(|event| event.id == answer.id));
}

#[tokio::test]
async fn parked_uncertain_start_keeps_fence_without_requeue_on_flush_or_recovery() {
    let (dir, channel, mut provider) = fixture();
    provider.config.runtimes[0].agent_command =
        dir.path().join("absent-adapter").to_string_lossy().into();
    let command = "parked-startup-refusal";
    let event = crate::tests::create_event(&provider, channel, command);
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("correlated failure");
    let intent = provider.state.lifecycle_start(command).expect("intent");
    let answer = provider
        .outbox
        .pending_events()
        .find(|event| {
            payload::decode_coding_session_lifecycle_receipt(&event.content)
                .is_ok_and(|r| r.command_id == command)
        })
        .expect("answer")
        .clone();
    let sink = StopSink {
        park: true,
        ids: std::sync::Mutex::new(Vec::new()),
    };
    provider
        .outbox
        .flush(&sink)
        .await
        .expect("permanent refusal recorded");
    assert!(provider.outbox.is_parked_event(&answer));
    assert!(provider
        .outbox
        .accepted_event(
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            &coding_session_lifecycle_receipt_semantic_key(command)
        )
        .is_none());
    for _ in 0..4 {
        provider
            .flush_terminal_dispositions()
            .expect("repeat flush");
    }
    assert!(!provider
        .outbox
        .pending_events()
        .any(|event| event.id == answer.id));
    let parked = provider.outbox.parked_len();
    let config = provider.config.clone();
    drop(provider);
    let mut provider = Provider::new(config).expect("restart");
    provider.recover().await.expect("actual recovery");
    for _ in 0..4 {
        provider
            .flush_terminal_dispositions()
            .expect("recovered flush");
    }
    assert!(provider.outbox.is_parked_event(&answer));
    assert_eq!(provider.outbox.parked_len(), parked);
    assert!(!provider
        .outbox
        .pending_events()
        .any(|event| event.id == answer.id || event.kind.as_u16() == 44223));
    assert!(provider.state.unresolved_start_target(&intent.target));
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("same operation abstains");
    assert_eq!(provider.sessions.live_count(), 0);
    assert!(provider.state.unresolved_start_target(&intent.target));
}
