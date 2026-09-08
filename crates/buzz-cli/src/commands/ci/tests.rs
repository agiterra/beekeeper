use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::routing::get;
use axum::Router;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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
