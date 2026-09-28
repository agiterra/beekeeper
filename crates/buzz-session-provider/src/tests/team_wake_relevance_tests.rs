//! Ledger 266, control run 8: a wake must deliver new responsibility, not
//! announce a record. After `mission.completed` the lead took five do-nothing
//! turns: its own two dispositions woke it, and a report wake and a
//! verify-result wake arrived 73–93 s late, after the facts were used.
//!
//! These tests drive the real wake path: the enqueue side through
//! `on_team_transaction` → `process_team_wake_for` against a recording relay,
//! and the dequeue side through the exact `SessionEvent` an actor sends
//! immediately before prompting (`WakeTurnAdmissionRequested`), answered by
//! `handle_session_event` from fresh relay facts.

use super::team_wake_driver_tests::{
    driver_channel_fixture, execution_events, expire_backoffs, published_wakes,
    DriverChannelFixture, LEAD_ROLE,
};
use super::*;

use nostr::Timestamp;
use tokio::sync::watch;

use buzz_core::coding_session_team_transaction::{
    CodingSessionTeamDispositionDecision, CodingSessionTeamMissionBlocked,
    CodingSessionTeamRefutationDecision, CodingSessionTeamTransactionBody,
    CodingSessionTeamVerdict,
};
use buzz_core::project_work::{
    ProjectWorkEvidenceBound, ProjectWorkEvidenceKind, ProjectWorkEvidenceRef,
};
use buzz_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};
use buzz_sdk::project_work::{build_project_work_evidence_bound, ProjectWorkEnvelope};

use crate::wake_relevance::{WakeAdmission, WakeDropReason};

/// A one-report umbrella whose **lead runs on this provider**, so a wake to
/// the lead is a turn this provider would spend.
fn local_lead_fixture(provider: &mut Provider, relay: &Keys, cwd: &Path) -> DriverChannelFixture {
    let channel = Uuid::new_v4();
    let founder = provider.config.keys.clone();
    let mut fixture = driver_channel_fixture(provider, relay, cwd, channel, false, 1, |_| 10);
    let lead_target = CodingSessionTarget {
        driver: fixture.builder_target.driver.clone(),
        instance_id: provider.config.instance_id.clone(),
        session_id: Uuid::new_v4().to_string(),
        generation: fixture.builder_target.generation,
    };
    fixture.events.extend(execution_events(
        &founder,
        &founder,
        channel,
        &fixture.scope.session_ref,
        &fixture.scope.genesis_ref,
        "create-lead",
        "fixture-provider",
        &fixture.lead,
        LEAD_ROLE,
        &lead_target,
    ));
    let mut lead_record = fixture.builder_record.clone();
    lead_record.session_id.clone_from(&lead_target.session_id);
    lead_record.actor = Some(fixture.lead.public_key().to_hex());
    lead_record.role = Some(LEAD_ROLE.into());
    lead_record.command_id = "create-lead".into();
    lead_record.generation_command_id = None;
    provider
        .state
        .insert_session(fixture.builder_record.clone())
        .expect("insert builder");
    provider
        .state
        .insert_session(lead_record)
        .expect("insert lead");
    fixture.lead_target = lead_target;
    fixture
}

fn team_tx(
    fixture: &DriverChannelFixture,
    body: CodingSessionTeamTransactionBody,
    signer: &Keys,
    created_at: u64,
) -> Event {
    build_coding_session_team_transaction(
        &fixture.channel.to_string(),
        coding_session_team_transaction_payload(
            fixture.scope.session_ref.clone(),
            fixture.scope.genesis_ref.clone(),
            None,
            None,
            body,
        ),
    )
    .expect("team transaction builds")
    .custom_created_at(Timestamp::from(created_at))
    .sign_with_keys(signer)
    .expect("team transaction signs")
}

fn blocked_terminal(fixture: &DriverChannelFixture, created_at: u64) -> Event {
    team_tx(
        fixture,
        CodingSessionTeamTransactionBody::MissionBlocked(CodingSessionTeamMissionBlocked {
            assignment_refs: vec![fixture.assignments[0].id.to_hex()],
            summary: "Stopped".into(),
            blockers: vec!["Held for the founder".into()],
            held_on: None,
            required_action: "Decide".into(),
        }),
        &fixture.lead,
        created_at,
    )
}

fn report_pointer(fixture: &DriverChannelFixture) -> String {
    team_wake::wake_text(&team_wake::WakeSource::Report {
        operation_id: fixture.reports[0].id.to_hex(),
        operation_type: "report".into(),
        author_pubkey: fixture.builder.public_key().to_hex(),
        created_at: fixture.reports[0].created_at.as_secs(),
    })
    .expect("report pointer")
}

/// Ask the provider, exactly as the actor does at dequeue, and wait for the
/// answer.
async fn admission(provider: &mut Provider, session_id: &str, text: &str) -> WakeAdmission {
    let (decision, mut answer) = watch::channel(WakeAdmission::Pending);
    provider
        .handle_session_event(session::SessionEvent::WakeTurnAdmissionRequested {
            session_id: session_id.to_owned(),
            command_id: "wake-under-test".into(),
            text: text.to_owned(),
            decision,
        })
        .expect("admission request handled");
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let current = answer.borrow().clone();
            if current != WakeAdmission::Pending {
                return current;
            }
            if answer.changed().await.is_err() {
                return WakeAdmission::Admit;
            }
        }
    })
    .await
    .expect("the provider answers within the bound")
}

struct Rig {
    _dir: tempfile::TempDir,
    provider: Provider,
    fixture: DriverChannelFixture,
    relay: HarnessRelay,
    control: RecordingTestRelay,
    server: tokio::task::JoinHandle<()>,
}

async fn rig(extra: impl FnOnce(&DriverChannelFixture) -> Vec<Event>) -> Rig {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("checkout");
    let founder = test_operator_keys().clone();
    let relay_keys = Keys::generate();
    let mut provider = Provider::new(config_of(
        founder.clone(),
        &dir.path().join("state"),
        None,
        "missing-agent".into(),
    ))
    .expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    let fixture = local_lead_fixture(&mut provider, &relay_keys, &cwd);
    let mut events = fixture.events.clone();
    events.extend(extra(&fixture));
    let (relay, control, server) = spawn_recording_test_relay(&founder, events).await;
    provider.set_rest_client(relay.rest_client());
    provider.subscribed.insert(fixture.channel);
    Rig {
        _dir: dir,
        provider,
        fixture,
        relay,
        control,
        server,
    }
}

impl Rig {
    async fn shutdown(self) {
        self.relay.shutdown().await;
        self.server.abort();
    }

    fn lead_session(&self) -> String {
        self.fixture.lead_target.session_id.clone()
    }
}

/// (1) Control run 8: the lead's own disposition woke the lead. It must
/// wake nobody — retired at enqueue, `self_authored`, no 44220 minted.
#[tokio::test]
async fn a_self_authored_disposition_wakes_nobody() {
    let mut disposition = None;
    let mut rig = rig(|fixture| {
        let event = team_tx(
            fixture,
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
                assignment_ref: fixture.assignments[0].id.to_hex(),
                report_ref: fixture.reports[0].id.to_hex(),
                refutation_ref: None,
                decision: CodingSessionTeamDispositionDecision::Approve,
                summary: "Approved".into(),
                findings: Vec::new(),
                required_action: None,
            }),
            &fixture.lead,
            20,
        );
        disposition = Some(event.clone());
        vec![event]
    })
    .await;
    let disposition = disposition.expect("disposition");
    let channel = rig.fixture.channel;
    rig.provider
        .on_team_transaction(channel, &disposition)
        .expect("capture");
    let publisher = rig.relay.event_publisher();
    for _ in 0..4 {
        expire_backoffs(&mut rig.provider);
        rig.provider
            .process_team_wake_for(channel, &publisher)
            .await
            .expect("process");
    }
    assert!(
        published_wakes(&rig.control).is_empty(),
        "the lead's own disposition must not mint a wake to the lead"
    );
    assert_eq!(
        rig.provider.team_wakes.channel_counts(channel),
        (1, 0, 0, 0),
        "the intent is retired into the durable resolved ledger, so a replay is a duplicate"
    );
    rig.provider
        .on_team_transaction(channel, &disposition)
        .expect("replay");
    assert_eq!(
        rig.provider.team_wakes.channel_counts(channel),
        (1, 0, 0, 0)
    );
    rig.shutdown().await;
}

/// (2) A report wake owed while the umbrella was open is dropped
/// `post_terminal` when it reaches the head of the queue after the terminal.
#[tokio::test]
async fn a_report_wake_dequeued_after_the_terminal_is_dropped_post_terminal() {
    let mut rig = rig(|_| Vec::new()).await;
    let pointer = report_pointer(&rig.fixture);
    let lead = rig.lead_session();
    assert_eq!(
        admission(&mut rig.provider, &lead, &pointer).await,
        WakeAdmission::Admit,
        "while the umbrella is open the builder's report is owed"
    );

    let terminal = blocked_terminal(&rig.fixture, now_secs());
    rig.control.events.lock().expect("events").push(terminal);
    assert_eq!(
        admission(&mut rig.provider, &lead, &pointer).await,
        WakeAdmission::Drop {
            reason: WakeDropReason::PostTerminal,
            fact_id: rig.fixture.reports[0].id.to_hex(),
        }
    );
    rig.shutdown().await;
}

/// (3) A host-result wake whose result the lead already bound as evidence is
/// dropped `already_consumed`.
#[tokio::test]
async fn a_host_result_already_cited_by_the_leads_evidence_is_already_consumed() {
    let mut rig = rig(|_| Vec::new()).await;
    let result_id = "5e".repeat(32);
    let pointer = serde_json::json!({
        "schema": crate::host_result_wake::HOST_RESULT_WAKE_SCHEMA,
        "type": crate::host_result_wake::HOST_RESULT_WAKE_TYPE,
        "runId": "run-1",
        "stepId": "verify",
        "disposition": "exited",
        "resultEventId": result_id,
        "exitCode": 0,
    })
    .to_string();
    assert!(team_wake::is_team_wake_pointer(&pointer));
    let lead = rig.lead_session();
    assert_eq!(
        admission(&mut rig.provider, &lead, &pointer).await,
        WakeAdmission::Admit
    );

    let evidence = build_project_work_evidence_bound(
        &ProjectWorkEnvelope {
            channel_ref: rig.fixture.channel.to_string(),
            session_ref: rig.fixture.scope.session_ref.clone(),
            genesis_ref: rig.fixture.scope.genesis_ref.clone(),
            project_ref: format!("30621:{}:kettle", "1e".repeat(32)),
        },
        ProjectWorkEvidenceBound {
            declaration_ref: "de".repeat(32),
            criterion_ids: vec!["verify".into()],
            artifact_commit: "e7".repeat(20),
            evidence_refs: vec![ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::ActionResult,
                event_id: result_id.clone(),
            }],
            completion_ref: None,
        },
    )
    .expect("evidence builds")
    .sign_with_keys(&rig.fixture.lead)
    .expect("evidence signs");
    rig.control.events.lock().expect("events").push(evidence);
    assert_eq!(
        admission(&mut rig.provider, &lead, &pointer).await,
        WakeAdmission::Drop {
            reason: WakeDropReason::AlreadyConsumed,
            fact_id: result_id,
        }
    );
    rig.shutdown().await;
}

/// (5) After the terminal, genuinely new evidence that contests it — a
/// verifier's confirmed refutation signed later, delivered by the CLI's
/// `--wake-to` pointer — still wakes the lead.
#[tokio::test]
async fn a_newer_contesting_fact_after_the_terminal_still_wakes() {
    let terminal_at = now_secs();
    let verifier = Keys::generate();
    let mut refutation_id = String::new();
    let mut rig = rig(|fixture| {
        let refutation = team_tx(
            fixture,
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
                assignment_ref: fixture.assignments[0].id.to_hex(),
                report_ref: fixture.reports[0].id.to_hex(),
                decision: CodingSessionTeamRefutationDecision::Confirmed,
                summary: "The landed commit breaks the build".into(),
                findings: vec!["cargo test fails".into()],
                required_action: Some("Reopen".into()),
            }),
            &verifier,
            terminal_at + 5,
        );
        refutation_id = refutation.id.to_hex();
        vec![blocked_terminal(fixture, terminal_at), refutation]
    })
    .await;
    let cli_pointer =
        serde_json::json!({"operationId": refutation_id, "type": "verdict"}).to_string();
    let lead = rig.lead_session();
    assert_eq!(
        admission(&mut rig.provider, &lead, &cli_pointer).await,
        WakeAdmission::Admit,
        "evidence that calls the terminal into question is new responsibility"
    );
    // The same umbrella still drops the stale report: the rule is about the
    // fact, not about the umbrella being terminal.
    let pointer = report_pointer(&rig.fixture);
    assert!(matches!(
        admission(&mut rig.provider, &lead, &pointer).await,
        WakeAdmission::Drop {
            reason: WakeDropReason::PostTerminal,
            ..
        }
    ));
    rig.shutdown().await;
}

/// A drop at dequeue is answered in public: a `turn_dropped` receipt coded
/// `WAKE_NOT_OWED` that names the reason and the fact, and a durable refusal
/// so a redelivery cannot spend the turn after all.
#[tokio::test]
async fn a_dequeue_drop_is_published_as_a_turn_dropped_receipt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("checkout");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&dir.path().join("state"), Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("create");
    let session_id = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .session_id
        .clone();
    provider
        .handle_session_event(session::SessionEvent::TurnDropped {
            session_id,
            command_id: "wake-dropped".into(),
            reason: session::TurnDropReason::NotOwed {
                reason: WakeDropReason::PostTerminal,
                fact_id: "ab".repeat(32),
            },
        })
        .expect("drop handled");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let receipt = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "wake-dropped")
        .expect("the drop is answered");
    assert_eq!(receipt["status"], "turn_dropped");
    assert_eq!(
        receipt["error"]["code"],
        crate::wake_relevance::WAKE_NOT_OWED
    );
    let message = receipt["error"]["message"].as_str().expect("message");
    assert!(message.contains("post_terminal") && message.contains(&"ab".repeat(32)));
    assert!(provider.state.is_command_refused("wake-dropped"));
}

/// Ledger 272(d), run 11 lead seq 88–104, through the real dequeue path.
///
/// The verifier's refutation of a builder report woke the lead (a
/// `cli-wake-v1:<verdict>` command whose `turn_started` receipt and
/// `result` are on the wire). The verifier's settlement report on its own
/// assignment — base = the refuted head — was signed while that turn ran.
/// Ledger 275 A3: its signing time proves neither delivery nor consumption,
/// and the lead's ruling on it is owed, so its wake is admitted.
#[tokio::test]
async fn a_settlement_report_signed_inside_the_verdicts_turn_keeps_its_wake() {
    use buzz_core::coding_session_command::{
        CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery,
        CODING_SESSION_COMMAND_SCHEMA,
    };
    use buzz_core::coding_session_payload::{ReceiptStatus, TranscriptEnvelope};
    use buzz_core::coding_session_team_transaction::{
        CodingSessionTeamAssignment, CodingSessionTeamReport,
    };

    const HEAD: &str = "7c54cd1ff680aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let base = now_secs() - 600;
    let founder = test_operator_keys().clone();
    let verifier = Keys::generate();
    let mut verifier_report_id = String::new();
    let mut rig = rig(|fixture| {
        let report = |assignment_ref: String| {
            CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
                assignment_ref,
                summary: "Done".into(),
                branch: None,
                base_sha: Some(HEAD.into()),
                head_sha: Some(HEAD.into()),
                files: Vec::new(),
                tests: Vec::new(),
                red_before_green: None,
                deviations: Vec::new(),
                residuals: Vec::new(),
                anomalies: Vec::new(),
            })
        };
        let builder_report = team_tx(
            fixture,
            report(fixture.assignments[0].id.to_hex()),
            &fixture.builder,
            base + 10,
        );
        let verifier_assignment = team_tx(
            fixture,
            CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
                assignee_actor: verifier.public_key().to_hex(),
                assignee_role: "verifier".into(),
                objective: "Refute the report".into(),
                brief: "Try to break it.".into(),
                branch: None,
                base_sha: Some(HEAD.into()),
                file_ownership: Vec::new(),
                acceptance_steps: vec!["publish a verdict".into()],
            }),
            &fixture.lead,
            base + 20,
        );
        let verdict = team_tx(
            fixture,
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
                assignment_ref: fixture.assignments[0].id.to_hex(),
                report_ref: builder_report.id.to_hex(),
                decision: CodingSessionTeamRefutationDecision::Confirmed,
                summary: "Line numbers are wrong".into(),
                findings: vec!["bad UTF-8 on line 3 reports line 2".into()],
                required_action: Some("Fix the count".into()),
            }),
            &verifier,
            base + 100,
        );
        let verifier_report = team_tx(
            fixture,
            report(verifier_assignment.id.to_hex()),
            &verifier,
            base + 110,
        );
        verifier_report_id = verifier_report.id.to_hex();

        // The verdict's wake, as `bee` minted it, and the lead's turn on it.
        let verdict_id = verdict.id.to_hex();
        let command_id = format!("cli-wake-v1:{verdict_id}:f1e8076482ec");
        let pointer = serde_json::json!({"operationId": verdict_id, "type": "verdict"}).to_string();
        let target = fixture.lead_target.clone();
        let command = buzz_sdk::builders::build_coding_session_command(
            fixture.channel,
            &CodingSessionCommandPayload {
                schema: CODING_SESSION_COMMAND_SCHEMA.into(),
                command_id: command_id.clone(),
                target: target.clone(),
                action: CodingSessionAction::ThreadTurnStart {
                    text: pointer.clone(),
                    attachments: Vec::new(),
                    deliver: CodingSessionDelivery::Boundary,
                },
            },
        )
        .expect("command builds")
        .custom_created_at(Timestamp::from(base + 101))
        .sign_with_keys(&founder)
        .expect("command signs");
        let started = buzz_sdk::builders::build_coding_session_turn_receipt(
            fixture.channel,
            &command_id,
            ReceiptStatus::TurnStarted,
            &serde_json::to_string(&LifecycleReceipt::turn_started(&command_id, &target, "t-v"))
                .expect("receipt json"),
        )
        .expect("receipt builds")
        .custom_created_at(Timestamp::from(base + 102))
        .sign_with_keys(&founder)
        .expect("receipt signs");
        let transcript = |seq: u64, at: u64, item: serde_json::Value| {
            let envelope =
                TranscriptEnvelope::new(&target, seq, (at * 1_000) as i64, Some("t-v"), item);
            buzz_sdk::builders::build_coding_session_transcript_item(
                fixture.channel,
                &target,
                seq,
                &serde_json::to_string(&envelope).expect("envelope json"),
            )
            .expect("transcript builds")
            .custom_created_at(Timestamp::from(at))
            .sign_with_keys(&founder)
            .expect("transcript signs")
        };
        vec![
            builder_report,
            verifier_assignment,
            verdict,
            verifier_report,
            command,
            started,
            transcript(
                1,
                base + 102,
                serde_json::json!({
                    "kind": "user_prompt",
                    "commandId": command_id,
                    "content": pointer,
                }),
            ),
            transcript(
                2,
                base + 130,
                serde_json::json!({
                    "kind": "result",
                    "subtype": "success",
                    "isError": false,
                    "durationMs": 28_000,
                    "result": "completed",
                    "costUsd": null,
                    "costReason": "no_usage_reported",
                }),
            ),
        ]
    })
    .await;
    let lead = rig.lead_session();
    let pointer =
        serde_json::json!({"operationId": verifier_report_id, "type": "report"}).to_string();
    assert_eq!(
        admission(&mut rig.provider, &lead, &pointer).await,
        WakeAdmission::Admit
    );
    rig.shutdown().await;
}
