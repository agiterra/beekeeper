use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::routing::{get, post};
use axum::Router;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::continuation::{
    find_registration_ack, parse_target_key, read_continuation, resolve_continuation_status,
    resolve_target, stage_str,
};
use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_payload::{LifecycleReceipt, ReceiptStatus};
use buzz_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;
use serde_json::Value;

fn cli_args() -> Vec<String> {
    vec![
        "bee".into(),
        "ci".into(),
        "wait".into(),
        "--project".into(),
        format!("30621:{}:beekeeper", "11".repeat(32)),
        "--repo".into(),
        format!("30617:{}:beekeeper", "22".repeat(32)),
        "--commit".into(),
        "33".repeat(20),
        "--check".into(),
        "server".into(),
        "--run".into(),
        "run-17".into(),
        "--attempt".into(),
        "2".into(),
        "--workflow".into(),
        "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50".into(),
        "--phase".into(),
        "build".into(),
    ]
}

#[test]
fn cli_requires_positive_attempt_and_closed_phase_vocabulary() {
    use clap::Parser;

    assert!(crate::Cli::try_parse_from(cli_args()).is_ok());
    let mut zero = cli_args();
    *zero.last_mut().unwrap() = "build".into();
    let attempt = zero.iter().position(|arg| arg == "--attempt").unwrap() + 1;
    zero[attempt] = "0".into();
    assert!(crate::Cli::try_parse_from(zero).is_err());

    let mut phase = cli_args();
    *phase.last_mut().unwrap() = "release".into();
    assert!(crate::Cli::try_parse_from(phase).is_err());
}

#[test]
fn nip11_self_is_strict_and_normalized() {
    let keys = nostr::Keys::generate();
    let upper = keys.public_key().to_hex().to_ascii_uppercase();
    assert_eq!(
        relay_self_from_nip11(&json!({"self": upper}).to_string()).unwrap(),
        keys.public_key().to_hex()
    );
    assert!(relay_self_from_nip11("{}").is_err());
    assert!(relay_self_from_nip11(&json!({"self": "abcd"}).to_string()).is_err());
}

#[test]
fn duplicate_replay_collapses_and_different_canonical_results_conflict() {
    let keys = nostr::Keys::generate();
    let result = sample_result(CiConclusion::Success);
    let first = accepted(&keys, &result);
    let second = accepted(&keys, &result);
    let mut duplicates = BTreeMap::new();
    duplicates.insert(first.canonical.clone(), first);
    duplicates.insert(second.canonical.clone(), second);
    assert!(fold_replay(duplicates).unwrap().is_some());

    let failed = sample_result(CiConclusion::Failure);
    let mut conflict = BTreeMap::new();
    let success = accepted(&keys, &result);
    let failure = accepted(&keys, &failed);
    conflict.insert(success.canonical.clone(), success);
    conflict.insert(failure.canonical.clone(), failure);
    assert!(matches!(fold_replay(conflict), Err(CliError::Conflict(_))));
}

#[test]
fn command_output_is_structured_and_terminal_exit_conventions_are_truthful() {
    let keys = nostr::Keys::generate();
    let success = accepted(&keys, &sample_result(CiConclusion::Success));
    let output: serde_json::Value =
        serde_json::from_str(&encode_output(&success).unwrap()).unwrap();
    assert_eq!(output["event_id"], success.event_id);
    assert_eq!(output["result"]["identity"]["run"], "run-17");
    assert!(terminal_disposition(&success.result).is_ok());

    for conclusion in [CiConclusion::Failure, CiConclusion::Cancelled] {
        let result = sample_result(conclusion);
        let error = terminal_disposition(&result).unwrap_err();
        assert_eq!(crate::error::exit_code(&error), 1);
        assert!(matches!(error, CliError::Refused(_)));
    }
    let conflict = CliError::Conflict("conflicting exact result".into());
    assert_eq!(crate::error::exit_code(&conflict), 5);
}

fn sample_identity() -> CiResultIdentity {
    CiResultIdentity {
        project: format!("30621:{}:beekeeper", "11".repeat(32)),
        repository: format!("30617:{}:beekeeper", "22".repeat(32)),
        commit: "33".repeat(20),
        check: "server".into(),
        run: "run-17".into(),
        attempt: 2,
        workflow: "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50".into(),
        phase: buzz_core::ci_result::CiPhase::Build,
    }
}

fn sample_result(conclusion: CiConclusion) -> CiResult {
    CiResult {
        schema: "buzz-ci-result/v1".into(),
        identity: sample_identity(),
        conclusion,
        evidence_url: Some("https://ci.example/runs/17".into()),
        summary: None,
    }
}

fn event(keys: &nostr::Keys, result: &CiResult) -> nostr::Event {
    let (tags, content) = buzz_core::ci_result::build_ci_result(result).unwrap();
    let tags = tags
        .into_iter()
        .map(nostr::Tag::parse)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    nostr::EventBuilder::new(nostr::Kind::Custom(KIND_CI_RESULT as u16), content)
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap()
}

fn accepted(keys: &nostr::Keys, result: &CiResult) -> AcceptedResult {
    verify_result_event(
        &event(keys, result),
        &keys.public_key().to_hex(),
        &result.identity,
    )
    .unwrap()
}

#[test]
fn exact_identity_and_relay_signer_are_mandatory() {
    let relay = nostr::Keys::generate();
    let foreign = nostr::Keys::generate();
    let result = sample_result(CiConclusion::Success);
    assert!(verify_result_event(
        &event(&foreign, &result),
        &relay.public_key().to_hex(),
        &result.identity
    )
    .is_err());

    let mut wrong = result.identity.clone();
    wrong.phase = buzz_core::ci_result::CiPhase::Deploy;
    assert!(verify_result_event(
        &event(&relay, &result),
        &relay.public_key().to_hex(),
        &wrong
    )
    .is_err());

    let valid = event(&relay, &result);
    let mut corrupted = serde_json::to_value(&valid).unwrap();
    corrupted["content"] = json!("{}");
    let corrupted: nostr::Event = serde_json::from_value(corrupted).unwrap();
    assert!(
        verify_result_event(&corrupted, &relay.public_key().to_hex(), &result.identity).is_err()
    );
}

async fn authenticate_and_read_req(
    socket: &mut WebSocket,
    challenge: &str,
) -> (serde_json::Value, nostr::Event) {
    socket
        .send(Message::Text(json!(["AUTH", challenge]).to_string().into()))
        .await
        .unwrap();
    let Message::Text(raw) = socket.recv().await.unwrap().unwrap() else {
        panic!("expected AUTH text frame");
    };
    let auth: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(auth[0], "AUTH");
    let auth_event: nostr::Event = serde_json::from_value(auth[1].clone()).unwrap();
    auth_event.verify().unwrap();
    socket
        .send(Message::Text(
            json!(["OK", auth_event.id.to_hex(), true, ""])
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let Message::Text(raw) = socket.recv().await.unwrap().unwrap() else {
        panic!("expected REQ text frame");
    };
    (serde_json::from_str(&raw).unwrap(), auth_event)
}

fn fast_timing() -> WaitTiming {
    WaitTiming {
        idle: Duration::from_millis(100),
        first_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(20),
    }
}

#[tokio::test]
async fn disconnect_reauthenticates_resubscribes_and_consumes_stored_replay_through_eose() {
    let relay = nostr::Keys::generate();
    let caller = nostr::Keys::generate();
    let result = sample_result(CiConclusion::Success);
    let result_event = event(&relay, &result);
    let result_event_id = result_event.id.to_hex();
    let digest = correlation_id(&result.identity).unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));

    let app = Router::new().route(
        "/",
        get({
            let connections = connections.clone();
            let requests = requests.clone();
            move |ws: WebSocketUpgrade| {
                let connection = connections.fetch_add(1, Ordering::SeqCst);
                let requests = requests.clone();
                let result_event = result_event.clone();
                async move {
                    ws.on_upgrade(move |mut socket| async move {
                        let (req, auth_event) = authenticate_and_read_req(
                            &mut socket,
                            &format!("challenge-{connection}"),
                        )
                        .await;
                        assert_eq!(
                            auth_event
                                .tags
                                .iter()
                                .filter(|tag| tag.as_slice().first().map(String::as_str)
                                    == Some("auth"))
                                .count(),
                            1,
                            "every reconnect must carry the caller's NIP-OA tag"
                        );
                        requests.lock().unwrap().push(req);
                        if connection == 0 {
                            socket.send(Message::Close(None)).await.unwrap();
                            return;
                        }
                        let frame = json!(["EVENT", SUBSCRIPTION_ID, result_event]);
                        socket
                            .send(Message::Text(frame.to_string().into()))
                            .await
                            .unwrap();
                        // A duplicate replay is harmless, but the result must not
                        // return until the stored window is fenced by EOSE.
                        socket
                            .send(Message::Text(frame.to_string().into()))
                            .await
                            .unwrap();
                        socket
                            .send(Message::Text(
                                json!(["EOSE", SUBSCRIPTION_ID]).to_string().into(),
                            ))
                            .await
                            .unwrap();
                    })
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let owner = nostr::Keys::generate();
    let auth_json = buzz_sdk::nip_oa::compute_auth_tag(&owner, &caller.public_key(), "").unwrap();
    let auth_tag = buzz_sdk::nip_oa::parse_auth_tag(&auth_json).unwrap();
    let client = BuzzClient::new(url, caller, Some(auth_tag), Some(auth_json)).unwrap();

    let accepted = wait_for_result(
        &client,
        &relay.public_key().to_hex(),
        &result.identity,
        wait_deadline(Some(Duration::from_secs(2))).unwrap(),
        fast_timing(),
    )
    .await
    .unwrap();
    assert_eq!(accepted.event_id, result_event_id);
    assert_eq!(connections.load(Ordering::SeqCst), 2);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for req in requests.iter() {
        assert_eq!(req[0], "REQ");
        assert_eq!(req[1], SUBSCRIPTION_ID);
        assert_eq!(req[2]["kinds"], json!([KIND_CI_RESULT]));
        assert_eq!(req[2]["#d"], json!([digest]));
    }
    server.abort();
}

#[tokio::test]
async fn empty_eose_keeps_waiting_for_live_terminal() {
    let relay = nostr::Keys::generate();
    let caller = nostr::Keys::generate();
    let result = sample_result(CiConclusion::Success);
    let result_event = event(&relay, &result);
    let app = Router::new().route(
        "/",
        get(move |ws: WebSocketUpgrade| {
            let result_event = result_event.clone();
            async move {
                ws.on_upgrade(move |mut socket| async move {
                    let _ = authenticate_and_read_req(&mut socket, "live-challenge").await;
                    socket
                        .send(Message::Text(
                            json!(["EOSE", SUBSCRIPTION_ID]).to_string().into(),
                        ))
                        .await
                        .unwrap();
                    socket
                        .send(Message::Text(
                            json!(["EVENT", SUBSCRIPTION_ID, result_event])
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap();
                })
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = BuzzClient::new(url, caller, None, None).unwrap();
    let accepted = wait_for_result(
        &client,
        &relay.public_key().to_hex(),
        &result.identity,
        wait_deadline(Some(Duration::from_secs(2))).unwrap(),
        fast_timing(),
    )
    .await
    .unwrap();
    assert_eq!(accepted.result.identity.commit, result.identity.commit);
    server.abort();
}

#[tokio::test]
async fn overall_timeout_survives_reconnect_backoff() {
    let relay = nostr::Keys::generate();
    let caller = nostr::Keys::generate();
    let result = sample_result(CiConclusion::Success);
    let app = Router::new().route(
        "/",
        get(move |ws: WebSocketUpgrade| async move {
            ws.on_upgrade(move |mut socket| async move {
                let _ = authenticate_and_read_req(&mut socket, "timeout-challenge").await;
                socket.send(Message::Close(None)).await.unwrap();
            })
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = BuzzClient::new(url, caller, None, None).unwrap();
    let error = wait_for_result(
        &client,
        &relay.public_key().to_hex(),
        &result.identity,
        wait_deadline(Some(Duration::from_millis(75))).unwrap(),
        fast_timing(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, CliError::Unconfirmed(_)));
    server.abort();
}

#[test]
fn oversized_timeout_is_a_usage_error_instead_of_panicking() {
    let error = wait_deadline(Some(Duration::MAX)).unwrap_err();
    assert!(matches!(error, CliError::Usage(message) if message == "--timeout is too large"));
}

#[tokio::test]
async fn overall_deadline_includes_stalled_relay_metadata_fetch() {
    let app = Router::new().route(
        "/",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            axum::Json(json!({"self": "11".repeat(32)}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = BuzzClient::new(url, nostr::Keys::generate(), None, None).unwrap();
    let digest = correlation_id(&sample_identity()).unwrap();

    let error = trusted_relay_self(
        &client,
        wait_deadline(Some(Duration::from_millis(30))).unwrap(),
        &digest,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, CliError::Unconfirmed(message) if message.contains(&digest)));
    server.abort();
}

#[tokio::test]
async fn authentication_refusal_is_permanent() {
    let relay = nostr::Keys::generate();
    let caller = nostr::Keys::generate();
    let result = sample_result(CiConclusion::Success);
    let connections = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/",
        get({
            let connections = connections.clone();
            move |ws: WebSocketUpgrade| {
                connections.fetch_add(1, Ordering::SeqCst);
                async move {
                    ws.on_upgrade(move |mut socket| async move {
                        socket
                            .send(Message::Text(
                                json!(["AUTH", "refused-challenge"]).to_string().into(),
                            ))
                            .await
                            .unwrap();
                        let Message::Text(raw) = socket.recv().await.unwrap().unwrap() else {
                            panic!("expected AUTH text frame");
                        };
                        let auth: serde_json::Value = serde_json::from_str(&raw).unwrap();
                        let auth_event: nostr::Event =
                            serde_json::from_value(auth[1].clone()).unwrap();
                        socket
                            .send(Message::Text(
                                json!(["OK", auth_event.id.to_hex(), false, "revoked"])
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                    })
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = BuzzClient::new(url, caller, None, None).unwrap();
    let error = wait_for_result(
        &client,
        &relay.public_key().to_hex(),
        &result.identity,
        wait_deadline(Some(Duration::from_secs(1))).unwrap(),
        fast_timing(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, CliError::Auth(message) if message == "revoked"));
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    server.abort();
}

// ── `bee ci continue` / `bee ci continuation status` ────────────────────────

fn ci_target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-code".into(),
        instance_id: "inst-1".into(),
        session_id: "sess-1".into(),
        generation: 1,
    }
}

fn receipt_event(receipt: &LifecycleReceipt, created_at: i64, event_id: &str) -> Value {
    json!({
        "id": event_id,
        "pubkey": "aa".repeat(32),
        "created_at": created_at,
        "kind": KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        "tags": [],
        "content": serde_json::to_string(receipt).expect("receipt serializes"),
        "sig": "bb".repeat(64),
    })
}

// ── `parse_target_key` / `resolve_target` ───────────────────────────────────

#[test]
fn parse_target_key_round_trips_coding_session_target_key() {
    let target = ci_target();
    let key = buzz_core::coding_session_command::coding_session_target_key(&target);
    assert_eq!(parse_target_key(&key).unwrap(), target);
}

#[test]
fn parse_target_key_rejects_malformed_keys() {
    for bad in [
        "not-a-target-key",
        "coding-session/v1|",
        "coding-session/v1|3:abc",
        "coding-session/v1|3:abc3:def3:ghi1:1trailing",
        // A length that overruns the remaining bytes.
        "coding-session/v1|99:short3:def3:ghi1:1",
    ] {
        assert!(
            parse_target_key(bad).is_err(),
            "accepted malformed target key: {bad}"
        );
    }
}

#[test]
fn resolve_target_accepts_target_key_or_all_four_parts_and_refuses_a_partial_set() {
    let target = ci_target();
    let key = buzz_core::coding_session_command::coding_session_target_key(&target);
    assert_eq!(
        resolve_target(Some(&key), None, None, None, None).unwrap(),
        target
    );
    assert_eq!(
        resolve_target(
            None,
            Some("claude-code"),
            Some("inst-1"),
            Some("sess-1"),
            Some(1),
        )
        .unwrap(),
        target
    );
    assert!(resolve_target(None, Some("claude-code"), None, None, None).is_err());
    assert!(resolve_target(None, None, None, None, None).is_err());
}

// ── `read_continuation` ──────────────────────────────────────────────────────

#[test]
fn read_continuation_reads_literal_text_or_an_at_prefixed_file() {
    assert_eq!(read_continuation("ship it").unwrap(), "ship it");

    let mut path = std::env::temp_dir();
    path.push(format!("bee-ci-continuation-test-{}", uuid::Uuid::new_v4()));
    std::fs::write(&path, "from a file\n").expect("write temp file");
    let arg = format!("@{}", path.display());
    assert_eq!(read_continuation(&arg).unwrap(), "from a file\n");
    std::fs::remove_file(&path).ok();
}

// ── `find_registration_ack` ──────────────────────────────────────────────────

#[test]
fn find_registration_ack_matches_registered_and_refused_but_ignores_wrong_target_and_wrong_command()
{
    let target = ci_target();
    let mut other_target = target.clone();
    other_target.generation = 2;

    let registered = receipt_event(
        &LifecycleReceipt::continuation_registered("cic-a", &target),
        100,
        "receipt-registered",
    );
    let wrong_target = receipt_event(
        &LifecycleReceipt::continuation_registered("cic-a", &other_target),
        50,
        "receipt-wrong-target",
    );
    let wrong_command = receipt_event(
        &LifecycleReceipt::continuation_registered("cic-other", &target),
        10,
        "receipt-wrong-command",
    );

    // The wrong-target and wrong-commandId receipts are both present, but
    // neither is this registration's answer.
    let events = vec![wrong_target.clone(), wrong_command, registered.clone()];
    let ack = find_registration_ack(&events, "cic-a", &target).expect("a match exists");
    assert_eq!(ack.receipt_event_id, "receipt-registered");
    assert_eq!(ack.status, ReceiptStatus::ContinuationRegistered);

    // With only the wrong-target receipt for this commandId, there is no
    // match at all — it must not be mistaken for an answer.
    let only_wrong_target = vec![wrong_target];
    assert!(find_registration_ack(&only_wrong_target, "cic-a", &target).is_none());

    let refused = receipt_event(
        &LifecycleReceipt::turn_refused(
            "cic-b",
            &target,
            "COMMAND_ID_CONFLICT",
            "already registered",
        ),
        200,
        "receipt-refused",
    );
    let ack = find_registration_ack(&[refused], "cic-b", &target).expect("a refusal matches");
    assert_eq!(ack.status, ReceiptStatus::TurnRefused);
    assert_eq!(ack.refusal_code.as_deref(), Some("COMMAND_ID_CONFLICT"));
    assert_eq!(ack.refusal_message.as_deref(), Some("already registered"));
}

#[test]
fn find_registration_ack_ignores_later_turn_stages_and_malformed_events() {
    let target = ci_target();
    // `turn_queued`/`turn_started` are delivery-time stages, not the initial
    // registration ack this function answers.
    let queued = receipt_event(
        &LifecycleReceipt::turn_queued("cic-a", &target),
        100,
        "receipt-queued",
    );
    assert!(find_registration_ack(&[queued], "cic-a", &target).is_none());

    let malformed = json!({"kind": KIND_CODING_SESSION_LIFECYCLE_RECEIPT, "content": "not json"});
    let wrong_kind = json!({"kind": 1, "content": "{}"});
    assert!(find_registration_ack(&[malformed, wrong_kind], "cic-a", &target).is_none());
}

// ── `resolve_continuation_status` ────────────────────────────────────────────

#[test]
fn continuation_status_resolves_the_latest_stage_with_receipt_ids() {
    let target = ci_target();
    let events = vec![
        receipt_event(
            &LifecycleReceipt::continuation_registered("cic-s", &target),
            100,
            "r1",
        ),
        receipt_event(&LifecycleReceipt::turn_queued("cic-s", &target), 200, "r2"),
        receipt_event(
            &LifecycleReceipt::turn_started("cic-s", &target, "turn-1"),
            300,
            "r3",
        ),
        // A receipt for a different commandId must not contribute.
        receipt_event(
            &LifecycleReceipt::continuation_registered("cic-other", &target),
            400,
            "r-other",
        ),
    ];
    let status = resolve_continuation_status(&events, "cic-s");
    assert_eq!(status.stage, "started");
    assert_eq!(status.receipt_event_ids, vec!["r1", "r2", "r3"]);
    assert_eq!(status.refusal_code, None);
}

#[test]
fn continuation_status_is_none_with_no_matching_receipts() {
    let target = ci_target();
    let events = vec![receipt_event(
        &LifecycleReceipt::continuation_registered("cic-other", &target),
        100,
        "r1",
    )];
    let status = resolve_continuation_status(&events, "cic-s");
    assert_eq!(status.stage, "none");
    assert!(status.receipt_event_ids.is_empty());
    assert_eq!(status.refusal_code, None);
}

#[test]
fn continuation_status_reports_dropped_and_refused_with_their_codes() {
    let target = ci_target();
    for (build, expected_stage) in [
        (
            LifecycleReceipt::turn_dropped("cic-d", &target, "QUEUE_FULL", "mailbox full"),
            "dropped",
        ),
        (
            LifecycleReceipt::turn_refused("cic-d", &target, "STALE_GENERATION", "moved on"),
            "refused",
        ),
    ] {
        let events = vec![receipt_event(&build, 100, "r1")];
        let status = resolve_continuation_status(&events, "cic-d");
        assert_eq!(status.stage, expected_stage);
        assert!(status.refusal_code.is_some());
    }
}

#[test]
fn stage_str_covers_every_continuation_receipt_status() {
    assert_eq!(
        stage_str(ReceiptStatus::ContinuationRegistered),
        "registered"
    );
    assert_eq!(stage_str(ReceiptStatus::TurnQueued), "queued");
    assert_eq!(stage_str(ReceiptStatus::TurnStarted), "started");
    assert_eq!(stage_str(ReceiptStatus::TurnRefused), "refused");
    assert_eq!(stage_str(ReceiptStatus::TurnDropped), "dropped");
}

// ── End-to-end `cmd_continue` / `cmd_continuation_status` against a mock relay ──

/// A minimal `/events` + `/query` relay: `/events` records the exact
/// commandId/target the CLI registered (decoded from the posted 44220's
/// content), and `/query` answers with whatever `build_receipts` computes
/// from that recorded commandId/target. A plain `fn` pointer (not a general
/// closure) keeps this Clone/Send/Sync for axum's handler bound without any
/// `Arc<dyn Fn>` indirection.
async fn serve_continuation_relay(
    build_receipts: fn(&str, &CodingSessionTarget) -> Vec<Value>,
) -> String {
    let registered: Arc<Mutex<Option<(String, CodingSessionTarget)>>> = Arc::new(Mutex::new(None));

    let app = Router::new()
        .route(
            "/events",
            post({
                let registered = registered.clone();
                move |body: String| {
                    let registered = registered.clone();
                    async move {
                        let event: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                        let content = event.get("content").and_then(Value::as_str).unwrap_or("");
                        let payload: Value = serde_json::from_str(content).unwrap_or(Value::Null);
                        let command_id = payload
                            .get("commandId")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        if let Some(target) = payload.get("target").cloned().and_then(|value| {
                            serde_json::from_value::<CodingSessionTarget>(value).ok()
                        }) {
                            *registered.lock().unwrap() = Some((command_id, target));
                        }
                        let event_id = event
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("evt")
                            .to_owned();
                        axum::Json(json!({"accepted": true, "id": event_id, "message": ""}))
                    }
                }
            }),
        )
        .route(
            "/query",
            post({
                let registered = registered.clone();
                move |_body: String| {
                    let registered = registered.clone();
                    async move {
                        let events = match registered.lock().unwrap().clone() {
                            Some((command_id, target)) => build_receipts(&command_id, &target),
                            None => Vec::new(),
                        };
                        axum::Json(events)
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn build_registered_receipt(command_id: &str, target: &CodingSessionTarget) -> Vec<Value> {
    vec![receipt_event(
        &LifecycleReceipt::continuation_registered(command_id, target),
        100,
        "receipt-registered",
    )]
}

fn build_refused_receipt(command_id: &str, target: &CodingSessionTarget) -> Vec<Value> {
    vec![receipt_event(
        &LifecycleReceipt::turn_refused(
            command_id,
            target,
            "COMMAND_ID_CONFLICT",
            "a different registration already holds this commandId",
        ),
        100,
        "receipt-refused",
    )]
}

fn build_no_receipt(_command_id: &str, _target: &CodingSessionTarget) -> Vec<Value> {
    Vec::new()
}

fn build_wrong_target_receipt(command_id: &str, target: &CodingSessionTarget) -> Vec<Value> {
    let mut other = target.clone();
    other.generation += 1;
    vec![receipt_event(
        &LifecycleReceipt::continuation_registered(command_id, &other),
        100,
        "receipt-wrong-target",
    )]
}

#[allow(clippy::too_many_arguments)]
async fn run_continue(url: String, ack_timeout: u64) -> Result<(), CliError> {
    let client = BuzzClient::new(url, nostr::Keys::generate(), None, None).expect("client");
    continuation::cmd_continue(
        &client,
        &crate::OutputFormat::Json,
        "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50",
        None,
        Some("claude-code"),
        Some("inst-1"),
        Some("sess-1"),
        Some(1),
        format!("30621:{}:beekeeper", "11".repeat(32)),
        format!("30617:{}:beekeeper", "22".repeat(32)),
        "33".repeat(20),
        "server".to_owned(),
        "run-17".to_owned(),
        2,
        "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50".to_owned(),
        crate::CiPhaseArg::Build,
        "ship it",
        86400,
        ack_timeout,
    )
    .await
}

#[tokio::test]
async fn continue_reports_registered_and_exits_zero() {
    let url = serve_continuation_relay(build_registered_receipt).await;
    let result = run_continue(url, 5).await;
    assert!(result.is_ok(), "expected success, got {result:?}");
}

#[tokio::test]
async fn continue_reports_refused_and_exits_one_with_the_code() {
    let url = serve_continuation_relay(build_refused_receipt).await;
    let error = run_continue(url, 5).await.unwrap_err();
    assert_eq!(crate::error::exit_code(&error), 1);
    assert!(matches!(error, CliError::Refused(_)));
    assert!(error.to_string().contains("COMMAND_ID_CONFLICT"));
}

#[tokio::test]
async fn continue_with_no_receipt_is_unconfirmed_and_names_the_derived_command_id() {
    let url = serve_continuation_relay(build_no_receipt).await;
    let error = run_continue(url, 1).await.unwrap_err();
    assert_eq!(crate::error::exit_code(&error), 5);
    assert!(matches!(error, CliError::Unconfirmed(_)));
    assert!(
        error.to_string().contains("cic-"),
        "unconfirmed error should name the derived commandId so a retry is recognizable: {error}"
    );
}

#[tokio::test]
async fn continue_ignores_a_receipt_naming_the_same_command_id_but_the_wrong_target() {
    let url = serve_continuation_relay(build_wrong_target_receipt).await;
    let error = run_continue(url, 1).await.unwrap_err();
    assert_eq!(
        crate::error::exit_code(&error),
        5,
        "a wrong-target receipt must not be accepted as this registration's answer"
    );
    assert!(matches!(error, CliError::Unconfirmed(_)));
}

/// Two independently derived registrations for the same channel, CI identity,
/// target, continuation text, and `--expires-in` must produce the same
/// `commandId` — the mechanism behind "an exact retry names the same
/// registration rather than minting a second one" (§0). This pins the
/// derivation the CLI actually calls (`ci_continuation_command_id` with the
/// operation digest, the `cs-target` key, and the continuation text), rather
/// than re-deriving it a different way.
#[test]
fn identical_registration_inputs_derive_the_same_command_id() {
    use buzz_core::ci_result::correlation_id;
    use buzz_core::coding_session_command::{
        ci_continuation_command_id, coding_session_target_key,
    };

    let identity = sample_identity();
    let digest = correlation_id(&identity).unwrap();
    let target_key = coding_session_target_key(&ci_target());
    let channel = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
    let expires_at = 1_800_000_000u64;

    let first = ci_continuation_command_id(channel, &digest, &target_key, expires_at, "ship it");
    let second = ci_continuation_command_id(channel, &digest, &target_key, expires_at, "ship it");
    assert_eq!(first, second);
    assert!(first.starts_with("cic-"));

    // A different continuation text is a different registration.
    let different =
        ci_continuation_command_id(channel, &digest, &target_key, expires_at, "ship it now");
    assert_ne!(first, different);
}

#[tokio::test]
async fn continuation_status_status_reads_are_read_only_and_exit_zero() {
    async fn serve_query_only(events: Vec<Value>) -> String {
        let app = Router::new().route(
            "/query",
            post(move |_body: String| {
                let events = events.clone();
                async move { axum::Json(events) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    let target = ci_target();
    let events = vec![receipt_event(
        &LifecycleReceipt::continuation_registered("cic-s", &target),
        100,
        "r1",
    )];
    let url = serve_query_only(events).await;
    let client = BuzzClient::new(url, nostr::Keys::generate(), None, None).expect("client");
    let result = continuation::cmd_continuation_status(
        &client,
        &crate::OutputFormat::Json,
        "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50",
        "cic-s",
    )
    .await;
    assert!(
        result.is_ok(),
        "expected a read-only success, got {result:?}"
    );
}
