/// Terminal projection failures must leave a recoverable intent or a durable
/// signed refusal, never a closed attempt whose caller can receive no answer.
#[tokio::test]
async fn native_steer_terminal_projection_failures_are_answerable_at_restart() {
    for (outcome, failure) in [
        ("injected", "operation"),
        ("injected", "command"),
        ("injected", "outbox"),
        ("injected", "attempt"),
        ("new-turn", "operation"),
        ("new-turn", "command"),
        ("new-turn", "outbox"),
        ("new-turn", "attempt"),
        ("saturated", "outbox"),
        ("saturated", "refusal"),
        ("saturated", "attempt"),
        ("mailbox-full", "outbox"),
        ("mailbox-lost", "outbox"),
        ("fallback-full", "outbox"),
        ("fallback-gone", "outbox"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_dir = dir.path().join("state");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let mut provider = steering_provider(
            &state_dir,
            Some(&projects),
            &steer_agent_answering("{}"),
            Some(SteerIdleGuard::PromptRequired),
        );
        let target = create_steer_session(&mut provider, channel_id).await;
        let (tx, mut rx) = mpsc::channel(1);
        let _shutdown = provider.sessions.attach_test_handle(&target.session_id, tx);
        if outcome == "mailbox-full" {
            provider
                .sessions
                .handle(&target.session_id)
                .expect("handle")
                .deliver(SessionCommand::Interrupt {
                    command_id: "fill".into(),
                })
                .expect("fill mailbox");
        } else {
            provider
                .dispatch_native_steer(
                    channel_id,
                    now_secs(),
                    &test_operator_keys().public_key().to_hex(),
                    "steer-projection".into(),
                    target.clone(),
                    "input".into(),
                    None,
                    Some("operation-projection".into()),
                )
                .expect("durable admission");
            assert!(matches!(rx.try_recv(), Ok(SessionCommand::Steer { .. })));
        }
        if outcome == "fallback-full" {
            provider
                .sessions
                .handle(&target.session_id)
                .expect("handle")
                .deliver(SessionCommand::Interrupt {
                    command_id: "fill".into(),
                })
                .expect("fill fallback mailbox");
        } else if outcome == "fallback-gone" {
            rx.close();
        }
        let outbox = state_dir.join("outbox.jsonl");
        match failure {
            "outbox" => {
                std::fs::rename(&outbox, outbox.with_extension("saved")).expect("save outbox");
                std::fs::create_dir(&outbox).expect("block outbox");
            }
            "operation" => provider.state.fault_plan().fail_next_operation_append = true,
            "command" => provider.state.fault_plan().fail_next_command_append = true,
            "refusal" => provider.state.fault_plan().fail_next_refusal_append = true,
            "attempt" => provider.state.fault_plan().fail_next_steer_append = true,
            _ => unreachable!(),
        }
        let result = match outcome {
            "mailbox-full" => provider
                .dispatch_native_steer(
                    channel_id,
                    now_secs(),
                    &test_operator_keys().public_key().to_hex(),
                    "steer-projection".into(),
                    target.clone(),
                    "input".into(),
                    None,
                    None,
                )
                .map(|_| ()),
            "mailbox-lost" => provider.report_lost_mailbox(&target.session_id),
            _ => {
                let resolution = match outcome {
                    "injected" => SteerDispatch::Transport(SteerResolution::Injected {
                        wire: buzz_acp::steer::SteerWire::AcpExtension,
                        native_run_id: None,
                    }),
                    "new-turn" => SteerDispatch::Transport(SteerResolution::StartedNewTurn {
                        wire: buzz_acp::steer::SteerWire::AcpExtension,
                    }),
                    "saturated" => SteerDispatch::Saturated,
                    "fallback-full" | "fallback-gone" => {
                        SteerDispatch::Transport(SteerResolution::NotDelivered {
                            reason: NotDeliveredReason::PromptRequired,
                        })
                    }
                    _ => unreachable!(),
                };
                provider.fold_steer_resolution(
                    &target.session_id,
                    Some("original-turn".into()),
                    "steer-projection",
                    "steer-projection#1",
                    resolution,
                )
            }
        };
        result.expect_err("injected projection failure");
        assert_eq!(
            dispositions(&provider, "steer-projection"),
            vec![SteerDisposition::Intent],
            "{outcome}/{failure}: a projection failure cannot close the attempt"
        );
        let terminal = provider.state.is_command_refused("steer-projection");
        if matches!(outcome, "injected" | "new-turn") && failure != "operation" {
            assert_eq!(
                provider.state.operation_owner("operation-projection"),
                Some("steer-projection")
            );
        }
        if failure == "outbox" {
            std::fs::remove_dir(&outbox).expect("unblock");
            std::fs::rename(outbox.with_extension("saved"), &outbox).expect("restore outbox");
        }
        let config = provider.config.clone();
        drop(provider);
        let mut restarted = Provider::new(config).expect("restart");
        restarted.recover().await.expect("recover");
        let sink = CollectingSink::new();
        restarted
            .flush(&sink)
            .await
            .expect("flush recovered answer");
        let stages = receipt_stages(&sink, "steer-projection");
        assert!(
            stages.contains(&if terminal {
                "turn_dropped".into()
            } else {
                "turn_delivery_unknown".into()
            }),
            "{outcome}/{failure}: {stages:?}"
        );
        assert!(restarted.state.is_command_refused("steer-projection"));
        restarted
            .handle_command_event(
                channel_id,
                &steer_event(channel_id, "steer-projection", &target, "input"),
            )
            .await
            .expect("redelivery");
        assert_eq!(dispositions(&restarted, "steer-projection").len(), 1);
    }
}

/// A queued input can be dropped by prompt completion before its guard runs.
/// Even after a failed refusal snapshot and a later regrant, that undelivered
/// input must not turn into a fresh boundary prompt.
#[tokio::test]
async fn native_steer_queued_refusal_survives_failed_snapshot_and_regrant() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = steering_provider(
        &state_dir,
        Some(&projects),
        &steer_agent_answering("{}"),
        Some(SteerIdleGuard::PromptRequired),
    );
    let target = create_steer_session(&mut provider, channel_id).await;
    let operator = test_operator_keys().public_key().to_hex();
    let genesis = "cd".repeat(32);
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.genesis_ref = Some(genesis.clone());
            record.founder_pubkey = Some("ab".repeat(32));
            record.granted_operators.insert(operator.clone());
        })
        .expect("grant delegate");
    let (tx, mut rx) = mpsc::channel(1);
    let _shutdown = provider.sessions.attach_test_handle(&target.session_id, tx);
    provider
        .handle_command_event(
            channel_id,
            &steer_event(channel_id, "steer-dropped", &target, "queued"),
        )
        .await
        .expect("admit");
    assert!(matches!(rx.try_recv(), Ok(SessionCommand::Steer { .. })));
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.granted_operators.remove(&operator);
        })
        .expect("verified revoke");
    let snapshot = state_dir.join("state.json");
    std::fs::rename(&snapshot, snapshot.with_extension("saved")).expect("save snapshot");
    std::fs::create_dir(&snapshot).expect("block snapshot");
    provider
        .enforce_claim_for_genesis(&genesis)
        .expect_err("refusal snapshot failed");
    std::fs::remove_dir(&snapshot).expect("unblock");
    std::fs::rename(snapshot.with_extension("saved"), &snapshot).expect("restore snapshot");
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.granted_operators.insert(operator.clone());
        })
        .expect("regrant");
    provider
        .fold_steer_resolution(
            &target.session_id,
            None,
            "steer-dropped",
            "steer-dropped#1",
            SteerDispatch::Transport(SteerResolution::NotDelivered {
                reason: NotDeliveredReason::PromptEndedBeforeWrite,
            }),
        )
        .expect("prompt completion dropped input before write guard");
    assert_eq!(
        dispositions(&provider, "steer-dropped"),
        vec![SteerDisposition::Prevented]
    );
    assert!(rx.try_recv().is_err(), "no boundary delivery after regrant");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_codes(&sink, "steer-dropped"),
        vec![("turn_refused".into(), payload::UNAUTHORIZED_OPERATOR.into())]
    );
}
