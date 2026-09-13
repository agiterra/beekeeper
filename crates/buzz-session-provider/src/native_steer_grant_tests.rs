#[tokio::test]
async fn native_steer_queued_grant_removal_prevents_write_without_takeover() {
    for action in [
        CodingSessionAuthorityTransitionType::Revoke,
        CodingSessionAuthorityTransitionType::GrantViewer,
    ] {
        exercise_native_steer_grant_fence(action, false, true, false).await;
    }
}

#[tokio::test]
async fn native_steer_queued_unreadable_accepted_link_prevents_write() {
    exercise_native_steer_grant_fence(
        CodingSessionAuthorityTransitionType::Revoke,
        true,
        true,
        false,
    )
    .await;
}

#[tokio::test]
async fn native_steer_unreadable_chain_prevents_undelivered_fallback() {
    exercise_native_steer_grant_fence(
        CodingSessionAuthorityTransitionType::Revoke,
        true,
        false,
        false,
    )
    .await;
}

#[tokio::test]
async fn native_steer_revoked_dispatched_input_stays_fenced_after_regrant() {
    exercise_native_steer_grant_fence(
        CodingSessionAuthorityTransitionType::Revoke,
        false,
        false,
        true,
    )
    .await;
}

/// The command fence and its answer must survive together, even when any
/// projection after the snapshot fails. A snapshot that itself fails leaves
/// the original intent recoverable as unknown, never silently prevented.
#[tokio::test]
async fn native_steer_queued_fence_failures_preserve_a_restart_answer() {
    for failure in ["snapshot", "outbox", "refusal", "attempt"] {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("checkout");
        std::fs::create_dir_all(&cwd).expect("mkdir");
        let channel_id = Uuid::new_v4();
        let projects = write_projects(dir.path(), channel_id, &cwd);
        let state_dir = dir.path().join("state");
        let mut provider = steering_provider(
            &state_dir,
            Some(&projects),
            &steer_agent_answering(r#"{"outcome":"injected"}"#),
            Some(SteerIdleGuard::PromptRequired),
        );
        let target = create_steer_session(&mut provider, channel_id).await;
        let genesis = "cd".repeat(32);
        provider
            .state
            .update_session(&target.session_id, |record| {
                record.genesis_ref = Some(genesis.clone());
            })
            .expect("govern session");
        let mut sibling_record = provider
            .state
            .session(&target.session_id)
            .expect("session")
            .clone();
        sibling_record.session_id = "sibling".into();
        let sibling = sibling_record.target(&provider.config.instance_id);
        provider
            .state
            .insert_session(sibling_record)
            .expect("sibling");
        provider.steering.insert(sibling.session_id.clone(), true);
        let (tx, mut rx) = mpsc::channel(4);
        let (sibling_tx, mut sibling_rx) = mpsc::channel(4);
        let _shutdown = provider.sessions.attach_test_handle(&target.session_id, tx);
        let _sibling_shutdown = provider
            .sessions
            .attach_test_handle(&sibling.session_id, sibling_tx);
        let commands = [
            ("steer-fault", &target),
            ("steer-fault-2", &target),
            ("steer-sibling", &sibling),
        ];
        for (id, target) in &commands {
            provider
                .handle_command_event(
                    channel_id,
                    &steer_event(channel_id, id, target, "never written"),
                )
                .await
                .expect("admit");
        }
        assert!(matches!(rx.try_recv(), Ok(SessionCommand::Steer { .. })));
        assert!(matches!(rx.try_recv(), Ok(SessionCommand::Steer { .. })));
        assert!(matches!(
            sibling_rx.try_recv(),
            Ok(SessionCommand::Steer { .. })
        ));
        let body = provider.config.pubkey_hex();
        for target in [&target, &sibling] {
            provider
                .state
                .update_session(&target.session_id, |record| {
                    record.handover = ClaimState::Active(CurrentClaim {
                        claimant: "ab".repeat(32),
                        body_pubkey: body.clone(),
                        accepted_event_id: "12".repeat(32),
                        seq: 1,
                    });
                })
                .expect("accepted handover");
        }
        let blocked = match failure {
            "snapshot" => Some(state_dir.join("state.json")),
            "outbox" => Some(state_dir.join("outbox.jsonl")),
            "refusal" => {
                provider.state.fault_plan().fail_next_refusal_append = true;
                None
            }
            "attempt" => {
                provider.state.fault_plan().fail_next_steer_append = true;
                None
            }
            _ => unreachable!(),
        };
        if let Some(path) = &blocked {
            std::fs::rename(path, path.with_extension("saved")).expect("save durable file");
            std::fs::create_dir(path).expect("block append or rename");
        }
        provider
            .enforce_claim_for_genesis(&genesis)
            .expect_err("injected persistence failure");
        let mut answered = HashSet::new();
        for (id, target) in &commands {
            assert_eq!(
                dispositions(&provider, id),
                vec![SteerDisposition::Intent],
                "{failure}: {id}"
            );
            assert!(provider.in_flight.contains_key(*id), "{failure}: {id}");
            assert_eq!(provider.sessions.handle(&target.session_id).expect("handle")
                .begin_native_write_for_test(id), Err(SteerWriteRefusal::Fenced),
                "{failure}: every queued command and sibling must be fenced before the first persistence error");
            if provider.state.is_command_refused(id) {
                answered.insert(*id);
            }
        }
        assert_eq!(answered.len(), usize::from(failure != "snapshot"));
        if let Some(path) = &blocked {
            std::fs::remove_dir(path).expect("unblock");
            std::fs::rename(path.with_extension("saved"), path).expect("restore durable file");
        }
        let config = provider.config.clone();
        drop(provider);
        let mut restarted = Provider::new(config).expect("restart");
        restarted.recover().await.expect("recover durable answer");
        let sink = CollectingSink::new();
        restarted
            .flush(&sink)
            .await
            .expect("publish recoverable answer");
        for (id, target) in &commands {
            let expected = if answered.contains(id) {
                ("turn_refused".into(), payload::HANDOVER_FENCED.into())
            } else {
                (
                    "turn_delivery_unknown".into(),
                    payload::STEER_UNRESOLVED_AT_RESTART.into(),
                )
            };
            assert_eq!(receipt_codes(&sink, id), vec![expected], "{failure}: {id}");
            restarted
                .handle_command_event(
                    channel_id,
                    &steer_event(channel_id, id, target, "never written"),
                )
                .await
                .expect("redelivery");
            assert_eq!(dispositions(&restarted, id).len(), 1);
        }
        let again = CollectingSink::new();
        restarted.flush(&again).await.expect("flush duplicate");
        for (id, _) in &commands {
            assert!(receipt_stages(&again, id).is_empty(), "{failure}: {id}");
        }
    }
}

/// The first request is already written; the second is held behind its ACK.
/// Apply a signed, relay-accepted grant change through the real receipt path.
async fn exercise_native_steer_grant_fence(
    action: CodingSessionAuthorityTransitionType,
    missing_transition: bool,
    injected: bool,
    regrant: bool,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let first_written = dir.path().join("first-written");
    let release_ack = dir.path().join("release-ack");
    let second_written = dir.path().join("second-written");
    let answer = format!(
        "if [ \"$STEERS\" -eq 1 ]; then\n touch '{}'\n while [ ! -f '{}' ]; do sleep 0.01; done\nelse\n touch '{}'\nfi\nprintf '{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{\"outcome\":\"{}\"}}}}\\n' \"$id\"",
        first_written.display(), release_ack.display(), second_written.display(),
        if injected { "injected" } else { "promptRequired" },
    );
    let agent = crate::session::testing::steer_agent(&answer, 1_000_000);
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = steering_provider(
        &dir.path().join("state"),
        Some(&projects),
        &agent,
        Some(SteerIdleGuard::PromptRequired),
    );
    let target = create_steer_session(&mut provider, channel_id).await;
    let genesis = "cd".repeat(32);
    let grantee = Keys::generate();
    let grantee_hex = grantee.public_key().to_hex();
    let relay_keys = Keys::generate();
    provider.set_relay_self(relay_keys.public_key().to_hex());
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.genesis_ref = Some(genesis.clone());
            record.session_ref = Some(genesis.clone());
        })
        .expect("govern session");
    let grant = grant_transition_event(channel_id, &genesis, None, 1, &grantee_hex);
    let change = authority_transition_event(
        channel_id,
        &genesis,
        Some(grant.id.to_hex()),
        2,
        &grantee_hex,
        action,
    );
    let restored = grant_transition_event(
        channel_id,
        &genesis,
        Some(change.id.to_hex()),
        3,
        &grantee_hex,
    );
    let mut events = vec![grant.clone(), restored.clone()];
    if !missing_transition {
        events.push(change.clone());
    }
    let (mut relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, events).await;
    provider
        .handle_relay_event(
            &mut relay,
            channel_id,
            &acceptance_receipt_for_transition(&relay_keys, channel_id, &grant),
        )
        .await
        .expect("apply verified grant");
    provider
        .handle_command_event(
            channel_id,
            &turn_event(channel_id, "turn-original", &target),
        )
        .await
        .expect("founder's original turn");
    pump_until_turn_started(&mut provider).await;
    let original = provider
        .state
        .session(&target.session_id)
        .expect("session")
        .open_turn
        .clone()
        .expect("open turn");
    assert_eq!(provider.state.turns_used(&genesis), 1);
    for (id, text) in [
        ("steer-written", "already dispatched"),
        ("steer-queued", "still queued"),
    ] {
        provider
            .handle_command_event(
                channel_id,
                &command_event_by(
                    channel_id,
                    id,
                    &target,
                    serde_json::json!({"type":"thread.turn.start", "text":text, "deliver":"steer"}),
                    &grantee,
                ),
            )
            .await
            .expect("granted steer");
        tokio::time::timeout(Duration::from_secs(5), async {
            while !first_written.exists()
                || provider
                    .sessions
                    .handle(&target.session_id)
                    .expect("actor")
                    .free_slots()
                    != session::SESSION_MAILBOX_DEPTH
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("input reached native transport queue");
    }
    assert!(!second_written.exists());
    provider
        .handle_relay_event(
            &mut relay,
            channel_id,
            &acceptance_receipt_for_transition(&relay_keys, channel_id, &change),
        )
        .await
        .expect("accepted authority change");
    let record = provider.state.session(&target.session_id).expect("session");
    assert!(
        matches!(record.handover, ClaimState::NoClaim),
        "no takeover is involved"
    );
    assert_eq!(record.authority_seq, if missing_transition { 1 } else { 2 });
    assert_eq!(
        dispositions(&provider, "steer-written"),
        vec![SteerDisposition::Intent]
    );
    assert_eq!(
        dispositions(&provider, "steer-queued"),
        vec![SteerDisposition::Prevented]
    );
    if regrant {
        provider
            .handle_relay_event(
                &mut relay,
                channel_id,
                &acceptance_receipt_for_transition(&relay_keys, channel_id, &restored),
            )
            .await
            .expect("restore operator grant before first ACK");
        assert!(provider
            .state
            .session(&target.session_id)
            .expect("session")
            .granted_operators
            .contains(&grantee_hex));
    }
    std::fs::write(&release_ack, b"release").expect("release ACK");
    let mut queued_resolved = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !queued_resolved
            || !(provider.state.is_command_consumed("steer-written")
                || provider.state.is_command_refused("steer-written"))
        {
            let event = provider.next_session_event().await.expect("actor report");
            queued_resolved |= matches!(&event,
                SessionEvent::SteerResolved { command_id, .. } if command_id == "steer-queued");
            let written_resolved = matches!(&event,
                SessionEvent::SteerResolved { command_id, .. } if command_id == "steer-written");
            provider.handle_session_event(event).expect("fold report");
            if written_resolved && !injected {
                assert_eq!(
                    dispositions(&provider, "steer-written"),
                    vec![SteerDisposition::Prevented],
                    "authority loss must not revive an undelivered input, even after regrant"
                );
            }
        }
    })
    .await
    .expect("settle both native inputs");
    assert!(
        !second_written.exists(),
        "revoked input must never reach runtime"
    );
    assert!(provider.state.is_command_refused("steer-queued"));
    assert!(!provider.state.is_command_consumed("steer-queued"));
    assert_eq!(
        provider
            .state
            .session(&target.session_id)
            .expect("session")
            .open_turn
            .as_ref(),
        Some(&original)
    );
    assert_eq!(provider.state.turns_used(&genesis), 1);
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_stages(&sink, "steer-written"),
        vec![if injected {
            "turn_injected"
        } else {
            "turn_refused"
        }]
    );
    assert_eq!(
        receipt_codes(&sink, "steer-queued"),
        vec![(
            "turn_refused".into(),
            if missing_transition {
                commands::AUTHORITY_NOT_REVERIFIED
            } else {
                payload::UNAUTHORIZED_OPERATOR
            }
            .into()
        )]
    );
    // Cancellation remains the founder's existing action, not a side effect
    // of removing a delegate's grant or fencing an unverified chain.
    provider.interrupt_open_turn(&target.session_id, "turn-original");
    pump_until_turn_finished(&mut provider).await;
    relay.shutdown().await;
    server.abort();
}
