//! Admission and durable refusal recovery; no real adapter or relay required.
use super::team_wake_driver_tests::{driver_channel_fixture, DriverChannelFixture};
use super::*;
use crate::work_brief::{WorkBriefRefusal, WORK_BRIEF_BUDGET_BYTES};
use beekeeper_core::coding_session_team_transaction::*;
use beekeeper_sdk::coding_session_team_transaction::*;

#[tokio::test]
async fn work_brief_admission_preserves_ordinary_prose_and_refuses_missing_assignment_facts() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(&dir.path().join("state"), None);
    let channel = Uuid::new_v4();
    let record = governed_record(channel, dir.path(), &"ab".repeat(32));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).unwrap();
    assert_eq!(
        provider
            .assignment_work_brief(&target, "ordinary", "Hello")
            .await,
        Ok(None)
    );
    let pointer =
        serde_json::json!({"type":"assignment", "operationId":"cd".repeat(32)}).to_string();
    assert_eq!(
        provider
            .assignment_work_brief(&target, "assigned", &pointer)
            .await,
        Err(WorkBriefRefusal::Unavailable)
    );
    assert!(provider
        .admit_work_brief(channel, "ordinary", &target, Ok(None))
        .unwrap()
        .unwrap()
        .is_none());
    assert!(!provider.state.is_command_refused("ordinary"));
}

#[tokio::test]
async fn work_brief_boundary_rejects_oversized_payload_even_if_renderer_misbehaves() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(&dir.path().join("state"), None);
    let channel = Uuid::new_v4();
    let record = governed_record(channel, dir.path(), &"ab".repeat(32));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).unwrap();
    for size in [WORK_BRIEF_BUDGET_BYTES - 1, WORK_BRIEF_BUDGET_BYTES] {
        let text = "x".repeat(size);
        assert_eq!(
            provider
                .admit_work_brief(channel, "fits", &target, Ok(Some(text.clone())))
                .unwrap()
                .unwrap(),
            Some(text)
        );
    }
    let refused = provider
        .admit_work_brief(
            channel,
            "too-large",
            &target,
            Ok(Some("x".repeat(WORK_BRIEF_BUDGET_BYTES + 1))),
        )
        .unwrap();
    assert!(
        matches!(refused, Err(TurnDisposition::Answered(ref code)) if code == "WORK_BRIEF_TOO_LARGE")
    );
    assert!(provider.state.is_command_refused("too-large"));
    assert!(!provider.state.is_command_consumed("too-large"));
    assert!(provider
        .state
        .session(&target.session_id)
        .unwrap()
        .open_turn
        .is_none());
    let sink = CollectingSink::new();
    provider.flush(&sink).await.unwrap();
    let receipts = sink.contents_of(beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["status"], "turn_refused");
    assert_eq!(receipts[0]["error"]["code"], "WORK_BRIEF_TOO_LARGE");
}

#[tokio::test]
async fn work_brief_refusal_replays_original_receipt_after_write_failure_and_restart() {
    for ledger_failure in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let mut running = provider(&state, None);
        let channel = Uuid::new_v4();
        let record = governed_record(channel, dir.path(), &"ab".repeat(32));
        let target = running.target_for(&record);
        running.state.insert_session(record).unwrap();
        if ledger_failure {
            running.state.fault_plan().fail_next_refusal_append = true;
        } else {
            running.outbox.fail_next_enqueue();
        }
        assert!(running
            .admit_work_brief(channel, "refused", &target, Err(WorkBriefRefusal::TooLarge))
            .is_err());
        assert!(running.state.is_command_refused("refused"));
        assert!(!running.state.is_command_consumed("refused"));
        let staged = running.state.terminal_dispositions()[0].1.event.clone();
        // Re-open the real durable stores with the same signing identity.
        let config = running.config.clone();
        drop(running);
        let mut resumed = Provider::new(config).unwrap();
        let replay = resumed
            .apply_turn_decision(
                channel,
                now_secs(),
                &test_operator_keys().public_key().to_hex(),
                "event",
                TurnDecision::Ignore(commands::Ignored::AlreadyRefused {
                    command_id: "refused".into(),
                }),
            )
            .await
            .unwrap();
        assert!(
            matches!(replay, TurnDisposition::Answered(ref code) if code == "STAGED_ANSWER_REPUBLISHED")
        );
        let sink = CollectingSink::new();
        resumed.flush(&sink).await.unwrap();
        let receipts =
            sink.contents_of(beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
        assert_eq!(receipts.len(), 1);
        assert_eq!(
            receipts[0],
            serde_json::from_str::<serde_json::Value>(&staged.content).unwrap()
        );
        assert_eq!(receipts[0]["status"], "turn_refused");
        assert!(resumed
            .state
            .session(&target.session_id)
            .unwrap()
            .open_turn
            .is_none());
    }
}

#[tokio::test]
async fn work_brief_missing_data_stops_the_real_turn_path_before_mailbox_submission() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(&dir.path().join("state"), None);
    let channel = Uuid::new_v4();
    let record = governed_record(channel, dir.path(), &"ab".repeat(32));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).unwrap();
    let disposition = provider
        .apply_turn_decision(
            channel,
            now_secs(),
            &test_operator_keys().public_key().to_hex(),
            "event",
            TurnDecision::Start {
                command_id: "missing-brief".into(),
                target: target.clone(),
                text: serde_json::json!({"type":"assignment", "operationId":"cd".repeat(32)})
                    .to_string(),
                attachments: vec![],
                deliver: CodingSessionDelivery::Boundary,
                operation_key: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        disposition,
        TurnDisposition::Answered("WORK_BRIEF_UNAVAILABLE".into())
    );
    assert!(!provider.state.is_command_consumed("missing-brief"));
    assert!(provider
        .state
        .session(&target.session_id)
        .unwrap()
        .open_turn
        .is_none());
    let sink = CollectingSink::new();
    provider.flush(&sink).await.unwrap();
    let receipts = sink.contents_of(beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["status"], "turn_refused");
}

#[tokio::test]
async fn work_brief_lost_during_steer_reconstruction_cannot_fall_back_to_a_naked_assignment() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(&dir.path().join("state"), None);
    let channel = Uuid::new_v4();
    let record = governed_record(channel, dir.path(), &"ab".repeat(32));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).unwrap();
    let (tx, mut rx) = mpsc::channel(2);
    let _shutdown = provider.sessions.attach_test_handle(&target.session_id, tx);
    let pointer =
        serde_json::json!({"type":"assignment", "operationId":"cd".repeat(32)}).to_string();
    let attempt = SteerAttemptRecord {
        attempt_id: "lost-brief#1".into(),
        command_id: "lost-brief".into(),
        session_id: target.session_id.clone(),
        generation: target.generation,
        channel_id: channel,
        target: target.clone(),
        created_at: now_secs(),
        operation_key: None,
        operator_pubkey: test_operator_keys().public_key().to_hex(),
        sender_role: None,
        text: pointer,
        disposition: SteerDisposition::Intent,
        at: now_secs(),
        turn_id: None,
    };
    provider.state.stage_steer_intent(attempt.clone()).unwrap();
    // Only the durable attempt remains, as after reconstruction. No in-flight
    // frame holds the collected brief. The real fallback must refuse before send.
    provider
        .fallback_steer_to_boundary(&attempt, payload::STEER_TURN_ENDED)
        .unwrap();
    assert!(rx.try_recv().is_err());
    assert!(provider.state.is_command_refused("lost-brief"));
    assert!(!provider.state.is_command_consumed("lost-brief"));
    assert_eq!(
        provider
            .state
            .steer_attempt(&attempt.attempt_id)
            .unwrap()
            .disposition,
        SteerDisposition::Prevented
    );
    let sink = CollectingSink::new();
    provider.flush(&sink).await.unwrap();
    let receipts = sink.contents_of(beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["error"]["code"], "WORK_BRIEF_UNAVAILABLE");
}

fn brief_transaction(
    f: &DriverChannelFixture,
    signer: &Keys,
    body: CodingSessionTeamTransactionBody,
    command: Option<&str>,
    supersedes: Option<String>,
    at: u64,
) -> Event {
    build_coding_session_team_transaction(
        &f.channel.to_string(),
        coding_session_team_transaction_payload(
            f.scope.session_ref.clone(),
            f.scope.genesis_ref.clone(),
            supersedes,
            command.map(str::to_owned),
            body,
        ),
    )
    .unwrap()
    .custom_created_at(nostr::Timestamp::from(at))
    .sign_with_keys(signer)
    .unwrap()
}

fn brief_assignment(f: &DriverChannelFixture, command: &str) -> Event {
    brief_transaction(
        f,
        &f.lead,
        CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
            assignee_actor: f.builder.public_key().to_hex(),
            assignee_role: "builder".into(),
            objective: "Complete the bounded slice".into(),
            brief: "Keep all constraints.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["fixture/".into()],
            acceptance_steps: vec!["test".into()],
        }),
        Some(command),
        None,
        100,
    )
}

async fn start_brief(
    provider: &mut Provider,
    f: &DriverChannelFixture,
    command: &str,
    assignment: &Event,
) -> TurnDisposition {
    provider
        .apply_turn_decision(
            f.channel,
            now_secs(),
            &provider.config.keys.public_key().to_hex(),
            "event",
            TurnDecision::Start {
                command_id: command.into(),
                target: f.builder_target.clone(),
                text:
                    serde_json::json!({"type":"assignment", "operationId":assignment.id.to_hex()})
                        .to_string(),
                attachments: vec![],
                deliver: CodingSessionDelivery::Boundary,
                operation_key: None,
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn work_brief_authority_current_decisions_control_mailbox_delivery() {
    for case in ["unauthorized", "superseded", "current"] {
        let dir = tempfile::tempdir().unwrap();
        let mut provider = provider(&dir.path().join("state"), None);
        let relay_keys = Keys::generate();
        let f = driver_channel_fixture(
            &provider,
            &relay_keys,
            dir.path(),
            Uuid::new_v4(),
            false,
            0,
            |_| 0,
        );
        provider.set_relay_self(relay_keys.public_key().to_hex());
        provider
            .state
            .insert_session(f.builder_record.clone())
            .unwrap();
        let assignment = brief_assignment(&f, "C1");
        let mut events = f.events.clone();
        events.push(assignment.clone());
        {
            let request = brief_transaction(
                &f,
                &f.lead,
                CodingSessionTeamTransactionBody::DecisionRequest(
                    CodingSessionTeamDecisionRequest {
                        question: "May this assignment touch production?".into(),
                        options: vec!["local only".into()],
                        held_on: "founder".into(),
                        blocks: vec![assignment.id.to_hex()],
                        recommendation: None,
                    },
                ),
                None,
                None,
                101,
            );
            let answer = brief_transaction(
                &f,
                if case == "unauthorized" {
                    &f.builder
                } else {
                    &provider.config.keys
                },
                CodingSessionTeamTransactionBody::DecisionAnswer(CodingSessionTeamDecisionAnswer {
                    request_ref: request.id.to_hex(),
                    choice: CodingSessionTeamDecisionChoice::Index(if case == "unauthorized" {
                        1
                    } else {
                        0
                    }),
                    note: Some("x".repeat(WORK_BRIEF_BUDGET_BYTES)),
                    condition: None,
                }),
                None,
                None,
                102,
            );
            validate_coding_session_team_transaction_envelope(&answer).unwrap();
            if case == "superseded" {
                events.push(brief_transaction(
                    &f,
                    &provider.config.keys,
                    CodingSessionTeamTransactionBody::DecisionAnswer(
                        CodingSessionTeamDecisionAnswer {
                            request_ref: request.id.to_hex(),
                            choice: CodingSessionTeamDecisionChoice::Index(0),
                            note: Some("Current binding ruling".into()),
                            condition: None,
                        },
                    ),
                    None,
                    Some(answer.id.to_hex()),
                    103,
                ));
            }
            events.extend([request, answer]);
        }
        let (relay, _control, server) =
            spawn_recording_test_relay(&provider.config.keys, events).await;
        provider.set_rest_client(relay.rest_client());
        let snapshot = team_wake::fetch_verified_snapshot_with(
            &relay.rest_client(),
            &relay_keys.public_key().to_hex(),
            &f.scope,
            Default::default(),
        )
        .await
        .unwrap();
        let fold = fold_coding_session_team_transactions(
            &snapshot.team_events,
            &team_wake::fold_context(
                &f.scope,
                &snapshot.founder_pubkey,
                &snapshot.authority,
                &snapshot.vocabulary,
            ),
        )
        .unwrap();
        assert_eq!(fold.decisions.len(), 1);
        assert_eq!(
            fold.decisions[0].answer_event_id.is_none(),
            case == "unauthorized"
        );
        let (tx, mut rx) = mpsc::channel(2);
        let _shutdown = provider
            .sessions
            .attach_test_handle(&f.builder_target.session_id, tx);
        let result = start_brief(&mut provider, &f, "C1", &assignment).await;
        if case == "current" {
            assert_eq!(
                result,
                TurnDisposition::Answered("WORK_BRIEF_TOO_LARGE".into())
            );
            assert!(rx.try_recv().is_err());
            assert!(provider.state.is_command_refused("C1"));
        } else {
            assert!(
                !provider.state.is_command_refused("C1"),
                "{case}: {result:?}"
            );
            let delivered = rx.try_recv().expect("valid brief reaches mailbox");
            let session::SessionCommand::Turn { framing, .. } = delivered else {
                panic!("expected prompt")
            };
            let brief = framing.unwrap().work_brief.unwrap();
            assert!(brief.len() <= WORK_BRIEF_BUDGET_BYTES);
            assert!(!brief.contains(&"x".repeat(WORK_BRIEF_BUDGET_BYTES)));
            assert_eq!(
                brief.contains("Current binding ruling"),
                case == "superseded"
            );
        }
        relay.shutdown().await;
        server.abort();
    }
}

#[tokio::test]
async fn work_brief_recovery_requires_reissued_assignment_not_only_new_command() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(&dir.path().join("state"), None);
    let relay_keys = Keys::generate();
    let f = driver_channel_fixture(
        &provider,
        &relay_keys,
        dir.path(),
        Uuid::new_v4(),
        false,
        0,
        |_| 0,
    );
    provider.set_relay_self(relay_keys.public_key().to_hex());
    provider
        .state
        .insert_session(f.builder_record.clone())
        .unwrap();
    let old = brief_assignment(&f, "C1");
    let (relay, control, server) =
        spawn_recording_test_relay(&provider.config.keys, f.events.clone()).await;
    provider.set_rest_client(relay.rest_client());
    let (tx, mut rx) = mpsc::channel(2);
    let _shutdown = provider
        .sessions
        .attach_test_handle(&f.builder_target.session_id, tx);
    assert_eq!(
        start_brief(&mut provider, &f, "C1", &old).await,
        TurnDisposition::Answered("WORK_BRIEF_UNAVAILABLE".into())
    );
    assert!(rx.try_recv().is_err());
    let original_refusal = provider
        .outbox
        .pending_events()
        .find(|event| event.kind.as_u16() as u32 == KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .unwrap()
        .clone();
    // Restore missing canonical data. The original command remains terminal.
    control.events.lock().unwrap().push(old.clone());
    let pointer =
        serde_json::json!({"type":"assignment", "operationId":old.id.to_hex()}).to_string();
    assert!(provider
        .assignment_work_brief(&f.builder_target, "C1", &pointer)
        .await
        .unwrap()
        .is_some());
    assert!(provider.state.is_command_refused("C1"));
    provider
        .apply_turn_decision(
            f.channel,
            now_secs(),
            &provider.config.keys.public_key().to_hex(),
            "replay",
            TurnDecision::Ignore(commands::Ignored::AlreadyRefused {
                command_id: "C1".into(),
            }),
        )
        .await
        .unwrap();
    assert!(provider.state.is_command_refused("C1"));
    assert!(rx.try_recv().is_err());
    assert!(provider
        .outbox
        .pending_events()
        .any(|event| event.id == original_refusal.id));
    let sink = CollectingSink::new();
    provider.flush(&sink).await.unwrap();
    let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
    assert_eq!(
        receipts,
        vec![serde_json::from_str::<serde_json::Value>(&original_refusal.content).unwrap()]
    );
    assert_eq!(receipts[0]["status"], "turn_refused");
    assert_eq!(
        start_brief(&mut provider, &f, "C2", &old).await,
        TurnDisposition::Answered("WORK_BRIEF_UNAVAILABLE".into())
    );
    assert!(rx.try_recv().is_err());
    let CodingSessionTeamTransactionBody::Assignment(body) =
        validate_coding_session_team_transaction_envelope(&old)
            .unwrap()
            .body
    else {
        unreachable!()
    };
    let reissued = brief_transaction(
        &f,
        &f.lead,
        CodingSessionTeamTransactionBody::Assignment(body),
        Some("C3"),
        Some(old.id.to_hex()),
        104,
    );
    control.events.lock().unwrap().push(reissued.clone());
    let result = start_brief(&mut provider, &f, "C3", &reissued).await;
    assert!(!provider.state.is_command_refused("C3"), "{result:?}");
    assert!(matches!(
        rx.try_recv().unwrap(),
        session::SessionCommand::Turn { .. }
    ));
    for refusal in [WorkBriefRefusal::Unavailable, WorkBriefRefusal::TooLarge] {
        assert!(
            refusal.to_string().contains("reissue the assignment"),
            "{refusal}"
        );
        assert!(refusal.to_string().contains("bindings"), "{refusal}");
    }
    relay.shutdown().await;
    server.abort();
}
