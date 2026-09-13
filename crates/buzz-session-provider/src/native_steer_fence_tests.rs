/// A pending ACK must not turn the transport's own queue into authority
/// to write the next input after its sender loses the execution.
#[tokio::test]
async fn native_steer_queued_behind_ack_is_fenced_before_runtime_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let first_written = dir.path().join("first-written");
    let release_ack = dir.path().join("release-ack");
    let second_written = dir.path().join("second-written");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let answer = format!(
            "if [ \"$STEERS\" -eq 1 ]; then\n  touch '{}'\n  while [ ! -f '{}' ]; do sleep 0.01; done\nelse\n  touch '{}'\nfi\nprintf '{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{\"outcome\":\"injected\"}}}}\\n' \"$id\"",
            first_written.display(), release_ack.display(), second_written.display(),
        );
    let agent = crate::session::testing::steer_agent(&answer, 1_000_000);
    let mut provider = steering_provider(
        &dir.path().join("state"),
        Some(&projects),
        &agent,
        Some(SteerIdleGuard::PromptRequired),
    );
    let target = create_steer_session(&mut provider, channel_id).await;
    let genesis = "cd".repeat(32);
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.genesis_ref = Some(genesis.clone());
            record.session_ref = Some(genesis.clone());
            record.granted_operators.insert(claimant_hex.clone());
        })
        .expect("grant claimant");
    provider
        .handle_command_event(
            channel_id,
            &command_event_by(
                channel_id,
                "turn-1",
                &target,
                serde_json::json!({"type":"thread.turn.start", "text":"original claimant turn"}),
                &claimant,
            ),
        )
        .await
        .expect("original turn");
    pump_until_turn_started(&mut provider).await;
    let original_turn = provider
        .state
        .session(&target.session_id)
        .expect("session")
        .open_turn
        .as_ref()
        .expect("open turn")
        .clone();
    let original_spend = provider.state.turns_used(&genesis);
    assert_eq!(original_spend, 1, "the original turn owns the spend");
    provider
        .handle_command_event(
            channel_id,
            &steer_event(channel_id, "steer-first", &target, "first steer"),
        )
        .await
        .expect("first steer");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !first_written.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("first request reached runtime and is awaiting ACK");
    provider
        .handle_command_event(
            channel_id,
            &steer_event(channel_id, "steer-second", &target, "revoked queued steer"),
        )
        .await
        .expect("second steer");
    tokio::time::timeout(Duration::from_secs(5), async {
        while provider
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
    .expect("second input moved from actor mailbox into native queue");
    assert!(!second_written.exists());

    let body = provider.config.pubkey_hex();
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.handover = ClaimState::Active(CurrentClaim {
                claimant: claimant_hex.clone(),
                body_pubkey: body,
                accepted_event_id: "12".repeat(32),
                seq: 1,
            });
        })
        .expect("claim execution");
    provider
        .enforce_claim_for_genesis(&genesis)
        .expect("fence former operator");
    assert_eq!(
        dispositions(&provider, "steer-second"),
        vec![SteerDisposition::Prevented],
        "native queue admission must not count as a runtime dispatch"
    );
    std::fs::write(&release_ack, b"release").expect("release first ACK");
    let mut second_resolved = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !second_resolved || !provider.state.is_command_consumed("steer-first") {
            let event = provider.next_session_event().await.expect("actor report");
            second_resolved |= matches!(&event,
                    SessionEvent::SteerResolved { command_id, .. } if command_id == "steer-second");
            provider.handle_session_event(event).expect("fold report");
        }
    })
    .await
    .expect("settle both native inputs");
    assert!(
        !second_written.exists(),
        "revoked queued input must never reach runtime"
    );
    assert!(provider.state.is_command_refused("steer-second"));
    assert!(!provider.state.is_command_consumed("steer-second"));
    assert_eq!(
        provider
            .state
            .session(&target.session_id)
            .expect("session")
            .open_turn
            .as_ref()
            .expect("original turn still open"),
        &original_turn
    );
    assert_eq!(provider.state.turns_used(&genesis), original_spend);
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush receipts");
    assert_eq!(
        receipt_codes(&sink, "steer-second"),
        vec![(
            "turn_refused".to_owned(),
            payload::HANDOVER_FENCED.to_owned()
        )]
    );
    provider
        .handle_command_event(
            channel_id,
            &command_event_by(
                channel_id,
                "end-original",
                &target,
                serde_json::json!({"type":"thread.turn.interrupt"}),
                &claimant,
            ),
        )
        .await
        .expect("end original turn");
    pump_until_turn_finished(&mut provider).await;
}

/// The other half of ADV-A: the fence lands while the steer is written
/// and **unanswered**. The fence cannot prevent the write, so it marks
/// the entry `fenced_after_dispatch` and refuses nothing; when the
/// runtime then answers `promptRequired`, the fallback is refused
/// `HANDOVER_FENCED` instead of running under revoked authority. The
/// running turn belongs to the new claimant, so it is left alone.
#[tokio::test]
async fn a_steer_fenced_while_unanswered_is_refused_when_it_comes_back_undelivered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    // Answers `promptRequired` a second after reading the steer, and
    // keeps the prompt open past that.
    let agent = crate::session::testing::steer_agent(
        r#"( sleep 1; printf '{"jsonrpc":"2.0","id":%s,"result":{"outcome":"promptRequired"}}\n' "$id" ) &"#,
        1_000_000,
    );
    let mut provider = steering_provider(
        &dir.path().join("state"),
        Some(&projects),
        &agent,
        Some(SteerIdleGuard::PromptRequired),
    );
    let target = create_steer_session(&mut provider, channel_id).await;
    let genesis = "cd".repeat(32);
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.genesis_ref = Some(genesis.clone());
            record.granted_operators.insert(claimant_hex.clone());
        })
        .expect("genesis");
    // The running turn is the future claimant's; the steer is the
    // founder's.
    provider
        .handle_command_event(
            channel_id,
            &command_event_by(
                channel_id,
                "turn-1",
                &target,
                serde_json::json!({ "type": "thread.turn.start", "text": "do the thing" }),
                &claimant,
            ),
        )
        .await
        .expect("turn");
    pump_until_turn_started(&mut provider).await;
    provider
        .handle_command_event(
            channel_id,
            &steer_event(channel_id, "steer-x", &target, "steer me"),
        )
        .await
        .expect("steer");
    // Written, unanswered.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let body = provider.config.pubkey_hex();
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.handover = ClaimState::Active(CurrentClaim {
                claimant: claimant_hex.clone(),
                body_pubkey: body.clone(),
                accepted_event_id: "12".repeat(32),
                seq: 1,
            });
        })
        .expect("claim");
    provider.enforce_claim_for_genesis(&genesis).expect("fence");
    assert!(
        provider
            .in_flight
            .get("steer-x")
            .is_some_and(|turn| turn.fenced_after_dispatch),
        "the fence that could not prevent the write is remembered on the entry"
    );
    assert_eq!(
        dispositions(&provider, "steer-x"),
        vec![SteerDisposition::Intent],
        "nothing is refused while the runtime may still inject it"
    );
    pump_until_steer_resolved(&mut provider).await;
    assert_eq!(
        dispositions(&provider, "steer-x"),
        vec![SteerDisposition::Prevented]
    );
    assert!(provider.state().is_command_refused("steer-x"));
    assert!(!provider.state().is_command_consumed("steer-x"));
    assert!(!provider.in_flight.contains_key("steer-x"));
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_codes(&sink, "steer-x"),
        vec![(
            "turn_refused".to_owned(),
            payload::HANDOVER_FENCED.to_owned()
        )]
    );
    // The claimant's turn was left alone; end it and make sure the
    // founder's steer never started.
    provider
        .handle_command_event(
            channel_id,
            &command_event_by(
                channel_id,
                "int-1",
                &target,
                serde_json::json!({ "type": "thread.turn.interrupt" }),
                &claimant,
            ),
        )
        .await
        .expect("interrupt");
    let events = pump_collecting_until(&mut provider, |event| {
        matches!(event, SessionEvent::TurnFinished { .. })
    })
    .await;
    pump_available(&mut provider).await;
    assert!(!events.iter().any(|event| matches!(
        event,
        SessionEvent::TurnStarted { command_id, .. } if command_id == "steer-x"
    )));
    assert!(!provider.state().is_command_consumed("steer-x"));
}
