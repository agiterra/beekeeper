//! Tests for `bee sessions model`: the receipt classifier, the metadata read
//! that is the only source of `model`, the report fold, and — against a
//! recording relay — the exact 44220 it publishes.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::response::Json;
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};

use beekeeper_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionTarget, CODING_SESSION_COMMAND_TAG_VERSION,
};
use beekeeper_core::coding_session_payload::{
    Capabilities, LifecycleReceipt, ReceiptError, ReceiptStatus, SessionMetadata, SessionStatus,
    LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA, MODEL_NOT_OFFERED,
};
use beekeeper_core::kind::KIND_CODING_SESSION_COMMAND;
use beekeeper_sdk::builders::{
    build_coding_session_lifecycle_receipt, build_coding_session_metadata,
    build_coding_session_turn_receipt,
};

use super::crew_cmds::ReceiptWait;
use super::model::{
    classify_model_receipts, cmd_model, fold_model_report, merge_model_report,
    metadata_after_receipt, MetadataModel, ModelAnswer, MODEL_WAIT_SECONDS,
};
use crate::client::BeekeeperClient;
use crate::error::CliError;

const CHANNEL: &str = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const SESSION: &str = "5f0c2a8e-1b7d-4c3e-9f60-2d4a8b1c7e90";
const COMMAND: &str = "model-1";

fn channel() -> uuid::Uuid {
    uuid::Uuid::parse_str(CHANNEL).expect("channel")
}

fn target(generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "instance-1".into(),
        session_id: SESSION.into(),
        generation,
    }
}

fn metadata_payload(target: &CodingSessionTarget, model: Option<&str>) -> SessionMetadata {
    SessionMetadata {
        schema: METADATA_SCHEMA.to_owned(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: None,
        role: None,
        provider: Some("claude-primary".try_into().expect("alias")),
        runtime: Some("claude".try_into().expect("runtime")),
        model: model.map(str::to_owned),
        status: SessionStatus::Idle,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: None,
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        handover: None,
        compose_ref: None,
    }
}

/// A provider-signed 44223 at `at`, built by the SDK builder the provider uses.
fn signed_metadata(
    keys: &nostr::Keys,
    target: &CodingSessionTarget,
    model: Option<&str>,
    at: u64,
) -> Value {
    let content = serde_json::to_string(&metadata_payload(target, model)).expect("metadata");
    let event = build_coding_session_metadata(channel(), target, &content)
        .expect("metadata builder")
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(keys)
        .expect("sign metadata");
    serde_json::to_value(event).expect("event json")
}

fn receipt_content(
    status: ReceiptStatus,
    session: Option<CodingSessionTarget>,
    error: Option<ReceiptError>,
) -> String {
    serde_json::to_string(&LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
        command_id: COMMAND.to_owned(),
        status,
        session,
        error,
        turn_id: None,
        rewind: None,
    })
    .expect("receipt")
}

/// A provider-signed stage receipt, built by the turn-receipt builder.
fn signed_stage(
    keys: &nostr::Keys,
    command_id: &str,
    status: ReceiptStatus,
    session: Option<CodingSessionTarget>,
    error: Option<ReceiptError>,
    at: u64,
) -> Value {
    let mut content = receipt_content(status, session, error);
    if command_id != COMMAND {
        content = content.replace(COMMAND, command_id);
    }
    let event = build_coding_session_turn_receipt(channel(), command_id, status, &content)
        .expect("turn receipt builder")
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(keys)
        .expect("sign receipt");
    serde_json::to_value(event).expect("event json")
}

fn refusal(code: &str) -> ReceiptError {
    ReceiptError {
        code: code.to_owned(),
        message: "not in this instance's allowedModels".to_owned(),
    }
}

fn provider_hex(keys: &nostr::Keys) -> String {
    keys.public_key().to_hex()
}

// ── Receipt classification ──────────────────────────────────────────────────

#[test]
fn model_applied_for_the_exact_target_is_the_answer() {
    let keys = nostr::Keys::generate();
    let events = vec![signed_stage(
        &keys,
        COMMAND,
        ReceiptStatus::ModelApplied,
        Some(target(3)),
        None,
        1_000,
    )];
    let answer =
        classify_model_receipts(&events, CHANNEL, COMMAND, &provider_hex(&keys), &target(3))
            .expect("no contradiction");
    assert!(matches!(
        answer,
        Some(ModelAnswer::Applied {
            created_at: 1_000,
            ..
        })
    ));
}

#[test]
fn refusal_and_drop_are_not_applied_and_keep_their_code() {
    let keys = nostr::Keys::generate();
    for status in [ReceiptStatus::TurnRefused, ReceiptStatus::TurnDropped] {
        let events = vec![signed_stage(
            &keys,
            COMMAND,
            status,
            Some(target(3)),
            Some(refusal(MODEL_NOT_OFFERED)),
            1_000,
        )];
        let answer =
            classify_model_receipts(&events, CHANNEL, COMMAND, &provider_hex(&keys), &target(3))
                .expect("no contradiction");
        let Some(ModelAnswer::NotApplied {
            status: got, error, ..
        }) = answer
        else {
            panic!("expected a not-applied answer for {status:?}");
        };
        assert_eq!(got, status);
        assert_eq!(
            error.map(|error| error.code).as_deref(),
            Some(MODEL_NOT_OFFERED)
        );
    }
}

#[test]
fn receipts_from_another_signer_another_command_or_a_lifecycle_key_are_ignored() {
    let provider = nostr::Keys::generate();
    let stranger = nostr::Keys::generate();
    // A lifecycle-keyed receipt (csl-key over commandId alone) is not the
    // stage-keyed envelope a turn command's answer carries.
    let lifecycle_keyed = serde_json::to_value(
        build_coding_session_lifecycle_receipt(
            channel(),
            COMMAND,
            &receipt_content(ReceiptStatus::ModelApplied, Some(target(3)), None),
        )
        .expect("builder")
        .sign_with_keys(&provider)
        .expect("sign"),
    )
    .expect("json");
    let events = vec![
        signed_stage(
            &stranger,
            COMMAND,
            ReceiptStatus::ModelApplied,
            Some(target(3)),
            None,
            1_000,
        ),
        signed_stage(
            &provider,
            "other-command",
            ReceiptStatus::ModelApplied,
            Some(target(3)),
            None,
            1_000,
        ),
        lifecycle_keyed,
        // A stage a switch never produces is not progress.
        signed_stage(
            &provider,
            COMMAND,
            ReceiptStatus::TurnQueued,
            Some(target(3)),
            None,
            1_000,
        ),
    ];
    let answer = classify_model_receipts(
        &events,
        CHANNEL,
        COMMAND,
        &provider_hex(&provider),
        &target(3),
    )
    .expect("no contradiction");
    assert_eq!(answer, None);
}

#[test]
fn applied_for_another_generation_or_disagreeing_answers_are_contradictions() {
    let keys = nostr::Keys::generate();
    let wrong_target = vec![signed_stage(
        &keys,
        COMMAND,
        ReceiptStatus::ModelApplied,
        Some(target(2)),
        None,
        1_000,
    )];
    assert!(classify_model_receipts(
        &wrong_target,
        CHANNEL,
        COMMAND,
        &provider_hex(&keys),
        &target(3)
    )
    .is_err());

    let disagreeing = vec![
        signed_stage(
            &keys,
            COMMAND,
            ReceiptStatus::ModelApplied,
            Some(target(3)),
            None,
            1_000,
        ),
        signed_stage(
            &keys,
            COMMAND,
            ReceiptStatus::TurnRefused,
            Some(target(3)),
            Some(refusal(MODEL_NOT_OFFERED)),
            1_001,
        ),
    ];
    let error = classify_model_receipts(
        &disagreeing,
        CHANNEL,
        COMMAND,
        &provider_hex(&keys),
        &target(3),
    )
    .expect_err("two answers that disagree");
    assert!(error.contains("conflicting"), "{error}");
}

// ── Metadata: the only source of `model` ─────────────────────────────────────

#[test]
fn metadata_republished_after_the_receipt_confirms_the_model() {
    let keys = nostr::Keys::generate();
    let before = signed_metadata(&keys, &target(3), Some("sonnet"), 900);
    let before_id = before["id"].as_str().expect("id").to_owned();
    let after = signed_metadata(&keys, &target(3), Some("opus[1m][high]"), 1_000);
    let after_id = after["id"].as_str().expect("id").to_owned();
    let seen: HashSet<String> = [before_id].into_iter().collect();
    let read = metadata_after_receipt(
        &[before, after],
        &provider_hex(&keys),
        &target(3),
        &seen,
        1_000,
    );
    assert_eq!(
        read,
        MetadataModel::Confirmed {
            event_id: after_id,
            model: Some("opus[1m][high]".to_owned()),
        }
    );
}

#[test]
fn metadata_that_cannot_be_the_republish_is_never_read_as_the_model() {
    let keys = nostr::Keys::generate();
    let stranger = nostr::Keys::generate();
    let pre_existing = signed_metadata(&keys, &target(3), Some("opus"), 1_000);
    let seen: HashSet<String> = [pre_existing["id"].as_str().expect("id").to_owned()]
        .into_iter()
        .collect();
    let mut forged = signed_metadata(&keys, &target(3), Some("opus"), 1_000);
    forged["sig"] = json!("0".repeat(128));
    let events = vec![
        pre_existing,
        // Older than the receipt.
        signed_metadata(&keys, &target(3), Some("opus"), 999),
        // Another generation of the same execution.
        signed_metadata(&keys, &target(4), Some("opus"), 1_000),
        // Another signer.
        signed_metadata(&stranger, &target(3), Some("opus"), 1_000),
        forged,
    ];
    assert_eq!(
        metadata_after_receipt(&events, &provider_hex(&keys), &target(3), &seen, 1_000),
        MetadataModel::Absent
    );
}

#[test]
fn two_same_second_republishes_that_disagree_are_ambiguous() {
    let keys = nostr::Keys::generate();
    let events = vec![
        signed_metadata(&keys, &target(3), Some("opus"), 1_000),
        signed_metadata(&keys, &target(3), Some("sonnet"), 1_000),
    ];
    assert!(matches!(
        metadata_after_receipt(
            &events,
            &provider_hex(&keys),
            &target(3),
            &HashSet::new(),
            1_000
        ),
        MetadataModel::Ambiguous(_)
    ));
}

// ── The printed report ──────────────────────────────────────────────────────

fn applied() -> ReceiptWait<ModelAnswer> {
    ReceiptWait::Answered(ModelAnswer::Applied {
        receipt_event_id: "aa".repeat(32),
        created_at: 1_000,
    })
}

#[test]
fn applied_and_confirmed_reports_the_metadata_model() {
    let report = fold_model_report(
        "opus[high]",
        Some(&applied()),
        &MetadataModel::Confirmed {
            event_id: "bb".repeat(32),
            model: Some("opus[high]".to_owned()),
        },
        10,
    );
    assert_eq!(report.delivery_status, "model_applied");
    assert_eq!(report.model.as_deref(), Some("opus[high]"));
    assert_eq!(report.model_status, "confirmed");
    assert_eq!(report.metadata_event_id, Some("bb".repeat(32)));
}

#[test]
fn applied_with_a_different_metadata_model_says_asked_and_running() {
    let report = fold_model_report(
        "opus[xhigh]",
        Some(&applied()),
        &MetadataModel::Confirmed {
            event_id: "bb".repeat(32),
            model: Some("opus[high]".to_owned()),
        },
        10,
    );
    assert_eq!(report.model.as_deref(), Some("opus[high]"));
    assert!(report.delivery.contains("asked for opus[xhigh]"));
    assert!(report.delivery.contains("reads opus[high]"));
}

#[test]
fn applied_without_a_republish_never_claims_the_requested_model() {
    let report = fold_model_report("opus[high]", Some(&applied()), &MetadataModel::Absent, 10);
    assert_eq!(report.delivery_status, "model_applied");
    assert_eq!(report.model, None);
    assert_eq!(report.model_status, "unconfirmed");
    let ambiguous = fold_model_report(
        "opus[high]",
        Some(&applied()),
        &MetadataModel::Ambiguous("two records".to_owned()),
        10,
    );
    assert_eq!(ambiguous.model, None);
    assert_eq!(ambiguous.model_status, "unconfirmed");
}

#[test]
fn a_refusal_reports_its_code_and_that_nothing_switched() {
    let wait = ReceiptWait::Answered(ModelAnswer::NotApplied {
        receipt_event_id: "cc".repeat(32),
        status: ReceiptStatus::TurnRefused,
        error: Some(refusal(MODEL_NOT_OFFERED)),
    });
    let report = fold_model_report("gpt-9", Some(&wait), &MetadataModel::Absent, 10);
    assert_eq!(report.delivery_status, "turn_refused");
    assert_eq!(report.model, None);
    assert_eq!(report.model_status, "not_switched");
    assert!(report.delivery.contains(MODEL_NOT_OFFERED));
    assert_eq!(report.receipt["error"]["code"], json!(MODEL_NOT_OFFERED));
}

/// A refusal exits 0 like `send`, but the JSON's first fields say it: the
/// relay's acceptance moves to `relayAccepted`, `accepted` is `false`, and
/// `message` is the "not switched" sentence.
#[test]
fn a_refusal_is_stated_in_accepted_and_message_not_only_delivery_status() {
    let wait = ReceiptWait::Answered(ModelAnswer::NotApplied {
        receipt_event_id: "cc".repeat(32),
        status: ReceiptStatus::TurnRefused,
        error: Some(refusal(MODEL_NOT_OFFERED)),
    });
    let report = fold_model_report("gpt-9", Some(&wait), &MetadataModel::Absent, 10);
    let mut merged = json!({"event_id": "ee", "accepted": true, "message": ""});
    merge_model_report(&mut merged, report, true);
    assert_eq!(merged["accepted"], json!(false));
    assert_eq!(merged["relayAccepted"], json!(true));
    let message = merged["message"].as_str().expect("message");
    assert!(message.starts_with("not switched:"), "{message}");
    assert!(message.contains(MODEL_NOT_OFFERED), "{message}");
    assert_eq!(merged["modelStatus"], json!("not_switched"));

    let pending = fold_model_report(
        "opus",
        Some(&ReceiptWait::Unconfirmed {
            query_failed: false,
        }),
        &MetadataModel::Absent,
        MODEL_WAIT_SECONDS,
    );
    let mut merged = json!({"event_id": "ee", "accepted": true, "message": ""});
    merge_model_report(&mut merged, pending, true);
    assert_eq!(merged["accepted"], json!(true), "pending is not a refusal");
    assert!(merged.get("relayAccepted").is_none());
    assert_eq!(MODEL_WAIT_SECONDS, 30);
}

#[test]
fn no_receipt_and_no_wait_are_unconfirmed_and_say_which() {
    let silent = fold_model_report(
        "opus",
        Some(&ReceiptWait::Unconfirmed {
            query_failed: false,
        }),
        &MetadataModel::Absent,
        10,
    );
    assert_eq!(silent.delivery_status, "unconfirmed");
    assert_eq!(silent.model_status, "unconfirmed");
    assert!(silent.delivery.contains("within 10s"));
    assert!(silent.delivery.contains("may still apply"));
    let skipped = fold_model_report("opus", None, &MetadataModel::Absent, 10);
    assert_eq!(skipped.delivery_status, "unconfirmed");
    assert!(skipped.delivery.contains("--no-wait"));
}

// ── Against a recording relay ───────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Answer {
    Applied,
    Refused,
}

#[derive(Clone)]
struct Relay {
    provider: nostr::Keys,
    channel: Arc<Vec<Value>>,
    answer: Answer,
    published: Arc<Mutex<Vec<nostr::Event>>>,
}

async fn spawn_relay(
    provider: nostr::Keys,
    channel: Vec<Value>,
    answer: Answer,
) -> (String, Relay, tokio::task::JoinHandle<()>) {
    let relay = Relay {
        provider,
        channel: Arc::new(channel),
        answer,
        published: Arc::new(Mutex::new(Vec::new())),
    };
    let app = Router::new()
        .route(
            "/events",
            post(|State(relay): State<Relay>, body: Bytes| async move {
                let event: nostr::Event = serde_json::from_slice(&body).expect("event JSON");
                let event_id = event.id.to_hex();
                relay.published.lock().expect("published").push(event);
                Json(json!({ "event_id": event_id, "accepted": true, "message": "" }))
            }),
        )
        .route(
            "/query",
            post(|State(relay): State<Relay>, body: Bytes| async move {
                let filters: Vec<Value> = serde_json::from_slice(&body).expect("filter JSON");
                let filter = filters.first().expect("one filter").clone();
                assert_eq!(filter["#h"], json!([CHANNEL]), "every read is h-scoped");
                let published = relay.published.lock().expect("published").last().cloned();
                let command = published.map(|event| {
                    serde_json::from_str::<CodingSessionCommandPayload>(&event.content)
                        .expect("44220")
                });
                if let Some(command_id) = filter["#csl-command"][0].as_str() {
                    let command = command.expect("a receipt query follows its publish");
                    assert_eq!(command.command_id, command_id);
                    let (status, error) = match relay.answer {
                        Answer::Applied => (ReceiptStatus::ModelApplied, None),
                        Answer::Refused => {
                            (ReceiptStatus::TurnRefused, Some(refusal(MODEL_NOT_OFFERED)))
                        }
                    };
                    return Json(json!([signed_stage(
                        &relay.provider,
                        command_id,
                        status,
                        Some(command.target),
                        error,
                        2_000,
                    )]));
                }
                if filter.get("#cs-target").is_some() {
                    // The republish after the receipt, plus the record that
                    // already existed, which must not be read as the answer.
                    let mut events: Vec<Value> = relay.channel.iter().cloned().collect();
                    if let Some(command) = command {
                        let CodingSessionAction::ThreadModelSet { selection } = command.action
                        else {
                            panic!("published something other than thread.model.set");
                        };
                        events.push(signed_metadata(
                            &relay.provider,
                            &command.target,
                            Some(&selection),
                            2_000,
                        ));
                    }
                    return Json(json!(events));
                }
                let kinds: Vec<u64> = filter["kinds"]
                    .as_array()
                    .expect("explicit kinds")
                    .iter()
                    .filter_map(Value::as_u64)
                    .collect();
                let events: Vec<Value> = relay
                    .channel
                    .iter()
                    .filter(|event| {
                        event["kind"]
                            .as_u64()
                            .is_some_and(|kind| kinds.contains(&kind))
                    })
                    .cloned()
                    .collect();
                Json(json!(events))
            }),
        )
        .with_state(relay.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("recording relay");
    });
    (url, relay, server)
}

fn client(url: String) -> BeekeeperClient {
    BeekeeperClient::new(url, nostr::Keys::generate(), None, None).expect("client")
}

fn live_channel(provider: &nostr::Keys) -> Vec<Value> {
    vec![
        signed_metadata(provider, &target(1), Some("sonnet"), 1_000),
        signed_metadata(provider, &target(2), Some("sonnet"), 1_500),
    ]
}

#[tokio::test]
async fn publishes_one_thread_model_set_to_the_current_generation() {
    for answer in [Answer::Applied, Answer::Refused] {
        let provider = nostr::Keys::generate();
        let (url, relay, server) =
            spawn_relay(provider.clone(), live_channel(&provider), answer).await;
        cmd_model(
            &client(url),
            CHANNEL,
            SESSION,
            None,
            "opus[1m][high]",
            false,
            Some(5),
        )
        .await
        .expect("a refusal exits like a refused send: 0, with the answer in the JSON");
        server.abort();

        let published = relay.published.lock().expect("published").clone();
        assert_eq!(published.len(), 1, "exactly one command, never a retry");
        let event = &published[0];
        assert_eq!(u32::from(event.kind.as_u16()), KIND_CODING_SESSION_COMMAND);
        let tags: Vec<Vec<String>> = event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect();
        assert_eq!(
            tags,
            vec![
                vec!["h".to_owned(), CHANNEL.to_owned()],
                vec![
                    "cs-v".to_owned(),
                    CODING_SESSION_COMMAND_TAG_VERSION.to_owned()
                ],
                vec![
                    "cs-target".to_owned(),
                    coding_session_target_key(&target(2))
                ],
            ]
        );
        let content: Value = serde_json::from_str(&event.content).expect("content");
        assert_eq!(
            content["action"],
            json!({"type": "thread.model.set", "selection": "opus[1m][high]"}),
            "the action carries exactly type and selection"
        );
        assert_eq!(content["target"]["generation"], json!(2));
    }
}

#[tokio::test]
async fn an_unknown_addressee_or_an_invalid_selection_publishes_nothing() {
    let provider = nostr::Keys::generate();
    let (url, relay, server) =
        spawn_relay(provider.clone(), live_channel(&provider), Answer::Applied).await;
    let client = client(url);
    let unknown = cmd_model(&client, CHANNEL, "nobody", None, "opus", false, None).await;
    assert!(matches!(unknown, Err(CliError::NotFound(_))), "{unknown:?}");
    for selection in ["   ", "opus\u{7}", &"x".repeat(2_049)] {
        let invalid = cmd_model(&client, CHANNEL, SESSION, None, selection, false, None).await;
        assert!(invalid.is_err(), "selection {selection:?} must be refused");
    }
    let bad_timeout = cmd_model(&client, CHANNEL, SESSION, None, "opus", false, Some(0)).await;
    assert!(matches!(bad_timeout, Err(CliError::Usage(_))));
    server.abort();
    assert!(relay.published.lock().expect("published").is_empty());
}

#[test]
fn inbox_and_send_fold_read_model_applied_as_delivered_without_naming_a_model() {
    use beekeeper_core::coding_session_command::CodingSessionDelivery;

    use super::crew::{fold_delivery, TurnStage};

    let stage = TurnStage {
        status: ReceiptStatus::ModelApplied,
        error_code: None,
        error_message: None,
        turn_id: None,
        at: 1_000,
    };
    let report = fold_delivery(CodingSessionDelivery::Boundary, Some(&stage), true);
    assert_eq!(report.delivered, Some(true));
    assert_eq!(report.status, "model_applied");
    assert!(report.detail.contains("metadata"), "{}", report.detail);
}
