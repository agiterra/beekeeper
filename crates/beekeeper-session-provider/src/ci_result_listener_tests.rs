//! Unit tests for the single bounded CI result listener.
//!
//! Each test runs the real task against a minimal in-process relay that speaks
//! the three frames the listener depends on — the NIP-42 handshake, a stored
//! REQ answered to `EOSE`, and live `EVENT`s — so the fold, the verification
//! order and the reconnect are exercised rather than described.

use super::*;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::routing::get;
use axum::Router;
use beekeeper_core::ci_result::{
    build_ci_result, correlation_id, CiConclusion, CiPhase, CiResult, CiResultIdentity,
    CI_RESULT_SCHEMA,
};
use nostr::Keys;
use tokio::sync::broadcast;

/// A relay that answers one REQ from a stored set and then streams whatever a
/// test publishes into it.
#[derive(Clone)]
struct FakeRelay {
    stored: Arc<Mutex<Vec<nostr::Event>>>,
    live: broadcast::Sender<nostr::Event>,
    /// Requests observed, as the raw filter values, so a test can assert the
    /// subscription was re-issued with a new `#d` set.
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
    /// When set, the relay hangs up immediately after the first `EOSE`.
    drop_after_eose: Arc<Mutex<bool>>,
}

async fn relay_socket(
    State(relay): State<FakeRelay>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| async move { serve(socket, relay).await })
}

async fn serve(mut socket: WebSocket, relay: FakeRelay) {
    if socket
        .send(AxumWsMessage::Text(
            json!(["AUTH", "ci-listener-test"]).to_string().into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let Some(Ok(AxumWsMessage::Text(auth))) = socket.recv().await else {
        return;
    };
    let Ok(auth) = serde_json::from_str::<serde_json::Value>(auth.as_str()) else {
        return;
    };
    let Some(event_id) = auth.pointer("/1/id").and_then(serde_json::Value::as_str) else {
        return;
    };
    if socket
        .send(AxumWsMessage::Text(
            json!(["OK", event_id, true, "authenticated"])
                .to_string()
                .into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    let mut live = relay.live.subscribe();
    let mut subscription: Option<(String, Vec<String>)> = None;
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(AxumWsMessage::Text(text))) = incoming else { return };
                let Ok(value) = serde_json::from_str::<serde_json::Value>(text.as_str()) else {
                    continue;
                };
                if value.pointer("/0").and_then(serde_json::Value::as_str) != Some("REQ") {
                    continue;
                }
                let Some(subscription_id) =
                    value.pointer("/1").and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                let Some(filter) = value.pointer("/2").cloned() else { continue };
                relay.requests.lock().expect("lock").push(filter.clone());
                let digests: Vec<String> = filter
                    .get("#d")
                    .and_then(serde_json::Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let matching: Vec<nostr::Event> = relay
                    .stored
                    .lock()
                    .expect("lock")
                    .iter()
                    .filter(|event| {
                        correlation_tag(event).is_some_and(|tag| digests.contains(&tag))
                    })
                    .cloned()
                    .collect();
                for event in matching {
                    if socket
                        .send(AxumWsMessage::Text(
                            json!(["EVENT", subscription_id, event]).to_string().into(),
                        ))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                if socket
                    .send(AxumWsMessage::Text(
                        json!(["EOSE", subscription_id]).to_string().into(),
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
                if *relay.drop_after_eose.lock().expect("lock") {
                    return;
                }
                subscription = Some((subscription_id.to_owned(), digests));
            }
            broadcast = live.recv() => {
                let Ok(event) = broadcast else { continue };
                let Some((subscription_id, digests)) = subscription.as_ref() else { continue };
                if !correlation_tag(&event).is_some_and(|tag| digests.contains(&tag)) {
                    continue;
                }
                if socket
                    .send(AxumWsMessage::Text(
                        json!(["EVENT", subscription_id, event]).to_string().into(),
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

async fn spawn_relay(stored: Vec<nostr::Event>) -> (String, FakeRelay) {
    let (live, _) = broadcast::channel(16);
    let relay = FakeRelay {
        stored: Arc::new(Mutex::new(stored)),
        live,
        requests: Arc::new(Mutex::new(Vec::new())),
        drop_after_eose: Arc::new(Mutex::new(false)),
    };
    let app: Router = Router::new()
        .route("/", get(relay_socket))
        .with_state(relay.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("ws://{address}"), relay)
}

fn identity(run: &str) -> CiResultIdentity {
    CiResultIdentity {
        project: format!("30621:{}:beekeeper", "11".repeat(32)),
        repository: format!("30617:{}:beekeeper", "11".repeat(32)),
        commit: "ab".repeat(20),
        check: "main-validation".into(),
        run: run.into(),
        attempt: 1,
        workflow: "6f1a2f1e-0b3c-4a5d-8e9f-0a1b2c3d4e5f".into(),
        phase: CiPhase::Build,
    }
}

fn result_event(
    keys: &Keys,
    identity: &CiResultIdentity,
    conclusion: CiConclusion,
) -> nostr::Event {
    let result = CiResult {
        schema: CI_RESULT_SCHEMA.into(),
        identity: identity.clone(),
        conclusion,
        evidence_url: Some("https://ci.agiterra.org/runs/136".into()),
        summary: Some("all gates green".into()),
    };
    let (tags, content) = build_ci_result(&result).expect("build result");
    nostr::EventBuilder::new(nostr::Kind::Custom(KIND_CI_RESULT as u16), content)
        .tags(
            tags.into_iter()
                .map(|tag| nostr::Tag::parse(tag).expect("tag"))
                .collect::<Vec<_>>(),
        )
        .sign_with_keys(keys)
        .expect("sign result")
}

fn listener_config(relay_url: &str, relay_self: &str) -> ListenerConfig {
    ListenerConfig {
        relay_url: relay_url.to_owned(),
        keys: Keys::generate(),
        auth_tag: None,
        relay_self: relay_self.to_owned(),
        idle: Duration::from_secs(5),
        first_backoff: Duration::from_millis(20),
        max_backoff: Duration::from_millis(50),
    }
}

fn watching(identities: &[CiResultIdentity]) -> BTreeMap<String, CiResultIdentity> {
    identities
        .iter()
        .map(|identity| (correlation_id(identity).expect("digest"), identity.clone()))
        .collect()
}

async fn next_listener_event(
    events: &mut mpsc::Receiver<CiListenerEvent>,
) -> Option<CiListenerEvent> {
    tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .expect("a listener event within the timeout")
}

/// Skip the bookkeeping frames a test is not asserting on.
async fn next_verdict(events: &mut mpsc::Receiver<CiListenerEvent>) -> CiListenerEvent {
    loop {
        match next_listener_event(events).await.expect("channel open") {
            CiListenerEvent::CiResultsAnswered { .. } => continue,
            verdict => return verdict,
        }
    }
}

#[tokio::test]
async fn a_result_stored_before_the_registration_is_found_by_the_first_request() {
    let relay_keys = Keys::generate();
    let identity = identity("136");
    let digest = correlation_id(&identity).expect("digest");
    let (url, _relay) = spawn_relay(vec![result_event(
        &relay_keys,
        &identity,
        CiConclusion::Success,
    )])
    .await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[identity]));

    match next_verdict(&mut events).await {
        CiListenerEvent::CiResultReady {
            digest: seen,
            signer,
            canonical_json,
            ..
        } => {
            assert_eq!(seen, digest);
            assert_eq!(signer, relay_keys.public_key().to_hex());
            assert!(canonical_json.contains("\"conclusion\":\"success\""));
        }
        other => panic!("expected ready, got {other:?}"),
    }
    listener.shutdown();
}

#[tokio::test]
async fn an_empty_answer_is_reported_as_answered_and_never_as_a_result() {
    let relay_keys = Keys::generate();
    let identity = identity("137");
    let digest = correlation_id(&identity).expect("digest");
    let (url, _relay) = spawn_relay(Vec::new()).await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[identity]));

    assert_eq!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered {
            digests: vec![digest],
        })
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.recv())
            .await
            .is_err(),
        "an empty answer must not produce a verdict"
    );
    listener.shutdown();
}

#[tokio::test]
async fn a_result_published_after_the_registration_arrives_live() {
    let relay_keys = Keys::generate();
    let identity = identity("138");
    let digest = correlation_id(&identity).expect("digest");
    let (url, relay) = spawn_relay(Vec::new()).await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(std::slice::from_ref(&identity)));
    // Wait for the subscription to be live before publishing, or the relay
    // has nowhere to send it.
    assert!(matches!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered { .. })
    ));

    let _ = relay
        .live
        .send(result_event(&relay_keys, &identity, CiConclusion::Failure));

    match next_verdict(&mut events).await {
        CiListenerEvent::CiResultReady {
            digest: seen,
            canonical_json,
            ..
        } => {
            assert_eq!(seen, digest);
            assert!(canonical_json.contains("\"conclusion\":\"failure\""));
        }
        other => panic!("expected ready, got {other:?}"),
    }
    listener.shutdown();
}

#[tokio::test]
async fn a_cancelled_conclusion_is_a_result_like_any_other() {
    let relay_keys = Keys::generate();
    let identity = identity("139");
    let (url, _relay) = spawn_relay(vec![result_event(
        &relay_keys,
        &identity,
        CiConclusion::Cancelled,
    )])
    .await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[identity]));

    match next_verdict(&mut events).await {
        CiListenerEvent::CiResultReady { canonical_json, .. } => {
            assert!(canonical_json.contains("\"conclusion\":\"cancelled\""));
        }
        other => panic!("expected ready, got {other:?}"),
    }
    listener.shutdown();
}

#[tokio::test]
async fn a_duplicate_of_one_result_is_reported_exactly_once() {
    let relay_keys = Keys::generate();
    let identity = identity("140");
    let stored = result_event(&relay_keys, &identity, CiConclusion::Success);
    let (url, relay) = spawn_relay(vec![stored.clone(), stored.clone()]).await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[identity]));

    assert!(matches!(
        next_verdict(&mut events).await,
        CiListenerEvent::CiResultReady { .. }
    ));
    // The very same event again, live. Same canonical bytes, so it is the
    // same fact and not a second one.
    let _ = relay.live.send(stored);
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.recv())
            .await
            .is_err(),
        "a duplicate result must not be reported a second time"
    );
    listener.shutdown();
}

#[tokio::test]
async fn two_different_results_for_one_digest_are_reported_as_a_conflict() {
    let relay_keys = Keys::generate();
    let identity = identity("141");
    let digest = correlation_id(&identity).expect("digest");
    let (url, _relay) = spawn_relay(vec![
        result_event(&relay_keys, &identity, CiConclusion::Success),
        result_event(&relay_keys, &identity, CiConclusion::Failure),
    ])
    .await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[identity]));

    assert_eq!(
        next_verdict(&mut events).await,
        CiListenerEvent::CiResultConflict { digest }
    );
    listener.shutdown();
}

#[tokio::test]
async fn a_conflict_arriving_after_a_ready_supersedes_it() {
    let relay_keys = Keys::generate();
    let identity = identity("142");
    let digest = correlation_id(&identity).expect("digest");
    let (url, relay) = spawn_relay(vec![result_event(
        &relay_keys,
        &identity,
        CiConclusion::Success,
    )])
    .await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(std::slice::from_ref(&identity)));
    assert!(matches!(
        next_verdict(&mut events).await,
        CiListenerEvent::CiResultReady { .. }
    ));

    let _ = relay
        .live
        .send(result_event(&relay_keys, &identity, CiConclusion::Failure));
    assert_eq!(
        next_verdict(&mut events).await,
        CiListenerEvent::CiResultConflict { digest }
    );
    listener.shutdown();
}

#[tokio::test]
async fn a_result_signed_by_anyone_but_the_relay_is_never_a_result() {
    let relay_keys = Keys::generate();
    let impostor = Keys::generate();
    let identity = identity("143");
    let (url, _relay) = spawn_relay(vec![result_event(
        &impostor,
        &identity,
        CiConclusion::Success,
    )])
    .await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[identity]));

    assert!(matches!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered { .. })
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.recv())
            .await
            .is_err(),
        "a result the relay did not sign must wake nothing"
    );
    listener.shutdown();
}

#[tokio::test]
async fn a_result_whose_signature_does_not_verify_is_never_a_result() {
    let relay_keys = Keys::generate();
    let watched = identity("144");
    let genuine = result_event(&relay_keys, &watched, CiConclusion::Success);
    let mut raw = serde_json::to_value(&genuine).expect("encode");
    // A syntactically valid Schnorr signature over something else.
    let other = result_event(&relay_keys, &identity("145"), CiConclusion::Success);
    raw["sig"] = serde_json::to_value(other.sig.to_string()).expect("sig");
    let forged: nostr::Event = serde_json::from_value(raw).expect("decode forged event");
    let (url, _relay) = spawn_relay(vec![forged]).await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[watched]));

    assert!(matches!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered { .. })
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.recv())
            .await
            .is_err(),
        "a broken signature must wake nothing"
    );
    listener.shutdown();
}

#[tokio::test]
async fn a_malformed_result_body_is_never_a_result() {
    let relay_keys = Keys::generate();
    let identity = identity("146");
    let digest = correlation_id(&identity).expect("digest");
    // Correctly tagged and correctly signed, but the content is not a result.
    let malformed = nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CI_RESULT as u16),
        "{\"schema\":\"buzz-ci-result/v1\"}",
    )
    .tags(vec![nostr::Tag::parse(["d", digest.as_str()]).expect("tag")])
    .sign_with_keys(&relay_keys)
    .expect("sign");
    let (url, _relay) = spawn_relay(vec![malformed]).await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[identity]));

    assert!(matches!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered { .. })
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.recv())
            .await
            .is_err(),
        "an undecodable body must wake nothing"
    );
    listener.shutdown();
}

#[tokio::test]
async fn a_result_relabelled_with_another_digest_is_never_a_result() {
    let relay_keys = Keys::generate();
    let wanted = identity("147");
    let wanted_digest = correlation_id(&wanted).expect("digest");
    let other = identity("148");
    // The body of a *different* run, wearing the watched digest's `d` tag.
    let genuine = result_event(&relay_keys, &other, CiConclusion::Success);
    let mut raw = serde_json::to_value(&genuine).expect("encode");
    raw["tags"] = json!([
        ["d", wanted_digest],
        ["a", other.repository],
        ["project", other.project],
        ["workflow", other.workflow],
        ["schema", CI_RESULT_SCHEMA],
    ]);
    let relabelled: nostr::Event = serde_json::from_value(raw).expect("decode relabelled");
    let (url, _relay) = spawn_relay(vec![relabelled]).await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(&[wanted]));

    assert!(matches!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered { .. })
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.recv())
            .await
            .is_err(),
        "a body that disagrees with its own d tag must wake nothing"
    );
    listener.shutdown();
}

/// Each of the eight identity fields is part of the correlation digest, so a
/// result that differs in any one of them is a result for a different run and
/// never satisfies this registration.
#[tokio::test]
async fn a_result_differing_in_any_identity_field_is_a_different_run() {
    let base = identity("149");
    let base_digest = correlation_id(&base).expect("digest");
    let mutations: Vec<CiResultIdentity> = vec![
        CiResultIdentity {
            project: format!("30621:{}:other", "11".repeat(32)),
            ..base.clone()
        },
        CiResultIdentity {
            repository: format!("30617:{}:other", "11".repeat(32)),
            ..base.clone()
        },
        CiResultIdentity {
            commit: "cd".repeat(20),
            ..base.clone()
        },
        CiResultIdentity {
            check: "other-validation".into(),
            ..base.clone()
        },
        CiResultIdentity {
            run: "150".into(),
            ..base.clone()
        },
        CiResultIdentity {
            attempt: 2,
            ..base.clone()
        },
        CiResultIdentity {
            workflow: "7f1a2f1e-0b3c-4a5d-8e9f-0a1b2c3d4e5f".into(),
            ..base.clone()
        },
        CiResultIdentity {
            phase: CiPhase::Deploy,
            ..base.clone()
        },
    ];
    for mutated in mutations {
        assert_ne!(
            correlation_id(&mutated).expect("digest"),
            base_digest,
            "every identity field must change the correlation digest"
        );
    }
}

#[tokio::test]
async fn changing_the_watched_set_reissues_the_subscription() {
    let relay_keys = Keys::generate();
    let first = identity("151");
    let second = identity("152");
    let (url, relay) = spawn_relay(Vec::new()).await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(std::slice::from_ref(&first)));
    assert!(matches!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered { .. })
    ));
    listener.watch(watching(&[first.clone(), second.clone()]));
    let answered = next_listener_event(&mut events).await.expect("answered");
    let CiListenerEvent::CiResultsAnswered { digests } = answered else {
        panic!("expected an answer, got {answered:?}");
    };
    assert_eq!(digests.len(), 2, "the new filter covers both digests");

    let requests = relay.requests.lock().expect("lock").clone();
    assert!(
        requests.len() >= 2,
        "the subscription must be re-issued when the set changes; saw {requests:?}"
    );
    listener.shutdown();
}

#[tokio::test]
async fn an_unchanged_watched_set_does_not_churn_the_subscription() {
    let relay_keys = Keys::generate();
    let identity = identity("153");
    let (url, relay) = spawn_relay(Vec::new()).await;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(std::slice::from_ref(&identity)));
    assert!(matches!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered { .. })
    ));
    for _ in 0..5 {
        listener.watch(watching(std::slice::from_ref(&identity)));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(400), events.recv())
            .await
            .is_err(),
        "re-declaring the same set must not re-issue the REQ"
    );
    assert_eq!(relay.requests.lock().expect("lock").len(), 1);
    listener.shutdown();
}

#[tokio::test]
async fn a_dropped_connection_is_retried_and_replays_the_stored_result() {
    let relay_keys = Keys::generate();
    let identity = identity("154");
    let digest = correlation_id(&identity).expect("digest");
    let (url, relay) = spawn_relay(Vec::new()).await;
    *relay.drop_after_eose.lock().expect("lock") = true;

    let (listener, mut events) =
        CiResultListener::spawn(listener_config(&url, &relay_keys.public_key().to_hex()));
    listener.watch(watching(std::slice::from_ref(&identity)));
    // The first connection answers empty and hangs up.
    assert!(matches!(
        next_listener_event(&mut events).await,
        Some(CiListenerEvent::CiResultsAnswered { .. })
    ));
    // The result lands while the listener is between connections.
    relay.stored.lock().expect("lock").push(result_event(
        &relay_keys,
        &identity,
        CiConclusion::Success,
    ));
    *relay.drop_after_eose.lock().expect("lock") = false;

    match next_verdict(&mut events).await {
        CiListenerEvent::CiResultReady { digest: seen, .. } => assert_eq!(seen, digest),
        other => panic!("expected ready after reconnect, got {other:?}"),
    }
    assert!(
        relay.requests.lock().expect("lock").len() >= 2,
        "the listener must reconnect and re-request"
    );
    listener.shutdown();
}
