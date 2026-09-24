//! Liveness tests for the single bounded host-step listener.
//!
//! Each test runs the real task against a minimal in-process relay that
//! speaks the frames the listener depends on — the NIP-42 handshake, a REQ
//! answered from a stored set and closed with `EOSE`, live `EVENT`s, and the
//! WebSocket `Ping` heartbeat a real relay sends every 30 s — so the
//! behaviour that failed on 2026-09-19 and 2026-09-20 (a connection the
//! heartbeat kept alive while the subscription delivered nothing) is
//! exercised rather than described.

use super::*;

use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::routing::get;
use axum::Router;
use buzz_core::host_step::{
    build_host_step_requested, HostStepRequested, HOST_STEP_KIND_RUN_ON_HOST, HOST_STEP_SCHEMA,
};
use nostr::{EventBuilder, Keys, Kind};
use tokio::sync::broadcast;

/// How the stub relay behaves on a given connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Behaviour {
    /// Answer every REQ from the stored set, then `EOSE`, then stream live.
    Normal,
    /// Answer the first REQ normally and then go deaf: ignore every later
    /// REQ (no `EOSE`, no events) while keeping the socket warm with pings.
    /// This is the shape of the defect.
    SilentAfterFirstEose,
}

#[derive(Clone)]
struct FakeRelay {
    stored: Arc<Mutex<Vec<nostr::Event>>>,
    live: broadcast::Sender<nostr::Event>,
    /// `(connection index, subscription id)` for every REQ observed.
    requests: Arc<Mutex<Vec<(usize, String)>>>,
    /// Per-connection behaviour, indexed from the first connection; the last
    /// entry applies to every later connection.
    behaviour: Arc<Mutex<Vec<Behaviour>>>,
    connections: Arc<Mutex<usize>>,
}

impl FakeRelay {
    fn behaviour_for(&self, connection: usize) -> Behaviour {
        let behaviour = self.behaviour.lock().expect("lock");
        *behaviour
            .get(connection)
            .or_else(|| behaviour.last())
            .unwrap_or(&Behaviour::Normal)
    }

    fn requests_on(&self, connection: usize) -> usize {
        self.requests
            .lock()
            .expect("lock")
            .iter()
            .filter(|(index, _)| *index == connection)
            .count()
    }

    fn connection_count(&self) -> usize {
        *self.connections.lock().expect("lock")
    }
}

async fn relay_socket(
    State(relay): State<FakeRelay>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    let connection = {
        let mut connections = relay.connections.lock().expect("lock");
        let index = *connections;
        *connections += 1;
        index
    };
    ws.on_upgrade(move |socket| async move { serve(socket, relay, connection).await })
}

async fn serve(mut socket: WebSocket, relay: FakeRelay, connection: usize) {
    if socket
        .send(AxumWsMessage::Text(
            json!(["AUTH", "action-step-listener-test"])
                .to_string()
                .into(),
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

    let behaviour = relay.behaviour_for(connection);
    let mut live = relay.live.subscribe();
    let mut subscription: Option<String> = None;
    let mut answered = 0usize;
    // A real relay heartbeats every 30 s; the test compresses that so a
    // deaf connection still looks busy at the socket.
    let mut heartbeat = tokio::time::interval(Duration::from_millis(20));
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if socket.send(AxumWsMessage::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { return };
                let AxumWsMessage::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<serde_json::Value>(text.as_str()) else {
                    continue;
                };
                let frame = value.pointer("/0").and_then(serde_json::Value::as_str);
                if frame == Some("CLOSE") {
                    continue;
                }
                if frame != Some("REQ") {
                    continue;
                }
                let Some(subscription_id) =
                    value.pointer("/1").and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                relay
                    .requests
                    .lock()
                    .expect("lock")
                    .push((connection, subscription_id.to_owned()));
                answered += 1;
                if behaviour == Behaviour::SilentAfterFirstEose && answered > 1 {
                    // Deaf: no EOSE, no events, heartbeat only.
                    continue;
                }
                let stored = relay.stored.lock().expect("lock").clone();
                for event in stored {
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
                subscription = Some(subscription_id.to_owned());
            }
            broadcast = live.recv() => {
                let Ok(event) = broadcast else { continue };
                let Some(subscription_id) = subscription.as_ref() else { continue };
                if behaviour == Behaviour::SilentAfterFirstEose && answered > 1 {
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

async fn spawn_relay(stored: Vec<nostr::Event>, behaviour: Vec<Behaviour>) -> (String, FakeRelay) {
    let (live, _) = broadcast::channel(256);
    let relay = FakeRelay {
        stored: Arc::new(Mutex::new(stored)),
        live,
        requests: Arc::new(Mutex::new(Vec::new())),
        behaviour: Arc::new(Mutex::new(behaviour)),
        connections: Arc::new(Mutex::new(0)),
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

fn project() -> String {
    format!("30621:{}:pivot-test", "11".repeat(32))
}

fn a_request(step: &str, expires_at: u64) -> HostStepRequested {
    HostStepRequested {
        schema: HOST_STEP_SCHEMA.into(),
        run_id: "00000000-0000-0000-0000-000000000001".into(),
        workflow_id: "00000000-0000-0000-0000-000000000002".into(),
        workflow_name: "verify".into(),
        step_id: step.into(),
        step_index: 0,
        definition_hash: "ab".repeat(32),
        step_kind: HOST_STEP_KIND_RUN_ON_HOST.into(),
        channel_id: "00000000-0000-0000-0000-000000000009".into(),
        project: project(),
        approval: None,
        trigger_context: serde_json::json!({}),
        inputs: serde_json::json!({}),
        expires_at,
    }
}

fn far_future() -> u64 {
    now_secs() + 3_600
}

fn signed(keys: &Keys, request: &HostStepRequested) -> nostr::Event {
    let (tags, content) = build_host_step_requested(request).expect("build");
    EventBuilder::new(
        Kind::Custom(KIND_WORKFLOW_HOST_STEP_REQUESTED as u16),
        content,
    )
    .tags(
        tags.into_iter()
            .map(|tag| Tag::parse(tag).expect("tag"))
            .collect::<Vec<_>>(),
    )
    .sign_with_keys(keys)
    .expect("sign")
}

/// Fast timings: the probe cadence and its EOSE deadline compressed so a
/// test observes in milliseconds what production does in minutes.
fn listener_config(relay_url: &str, relay_self: &str) -> ListenerConfig {
    ListenerConfig {
        relay_url: relay_url.to_owned(),
        keys: Keys::generate(),
        auth_tag: None,
        relay_self: relay_self.to_owned(),
        idle: Duration::from_secs(30),
        first_backoff: Duration::from_millis(20),
        max_backoff: Duration::from_millis(50),
        probe_interval: Duration::from_millis(300),
        probe_timeout: Duration::from_millis(200),
    }
}

async fn next_event(events: &mut mpsc::Receiver<ActionStepEvent>) -> ActionStepEvent {
    tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .expect("a listener event within the timeout")
        .expect("the queue is open")
}

/// The defect of 178(j): a relay that heartbeats while its subscription
/// delivers nothing. The idle window is 30 s here and the test finishes in
/// well under a second, so nothing but the probe can save it.
#[tokio::test]
async fn a_silent_subscription_behind_a_live_heartbeat_is_recycled_and_recovers_the_request() {
    let relay_keys = Keys::generate();
    let request = a_request("verify", far_future());
    let stored = signed(&relay_keys, &request);
    let stored_id = stored.id.to_hex();
    let (url, relay) = spawn_relay(
        vec![stored],
        vec![Behaviour::SilentAfterFirstEose, Behaviour::Normal],
    )
    .await;

    let config = listener_config(&url, &relay_keys.public_key().to_hex());
    let (listener, mut events) = ActionStepListener::spawn(config);
    listener.watch(BTreeSet::from([project()]));

    // The first connection answers its opening REQ, so the stored request
    // arrives once immediately.
    match next_event(&mut events).await {
        ActionStepEvent::Requested { event_id, .. } => assert_eq!(event_id, stored_id),
        other => panic!("expected a request, got {other:?}"),
    }

    // Now the relay is deaf but pinging. The listener must probe, miss the
    // EOSE, end the connection, reconnect, and find the stored request
    // again — all without an idle timeout and without a relaunch.
    match next_event(&mut events).await {
        ActionStepEvent::Requested { event_id, .. } => assert_eq!(event_id, stored_id),
        other => panic!("expected the replayed request, got {other:?}"),
    }
    assert!(
        relay.connection_count() >= 2,
        "the listener must have opened a second connection, saw {}",
        relay.connection_count()
    );
    listener.shutdown();
}

/// The read loop is never blocked on the provider's run loop: while nothing
/// drains the queue the listener keeps answering the relay, and the events
/// are still there when the consumer resumes.
#[tokio::test]
async fn a_slow_consumer_does_not_stop_the_read_loop_and_loses_nothing() {
    let relay_keys = Keys::generate();
    let live_request = a_request("late", far_future());
    let live_event = signed(&relay_keys, &live_request);
    let live_id = live_event.id.to_hex();
    let (url, relay) = spawn_relay(Vec::new(), vec![Behaviour::Normal]).await;

    let config = listener_config(&url, &relay_keys.public_key().to_hex());
    let (listener, mut events) = ActionStepListener::spawn(config);
    listener.watch(BTreeSet::from([project()]));

    // Wait until the subscription exists, then publish without draining.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while relay.requests_on(0) == 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(relay.requests_on(0), 1, "the opening REQ was never seen");
    let _ = relay.live.send(live_event);

    // The consumer is asleep for longer than two probe cadences.
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        relay.requests_on(0) >= 2,
        "a blocked read loop would never have re-issued the REQ; saw {}",
        relay.requests_on(0)
    );
    assert_eq!(
        relay.connection_count(),
        1,
        "a healthy connection must not be recycled"
    );

    // Resuming finds the event that arrived while it slept.
    match next_event(&mut events).await {
        ActionStepEvent::Requested { event_id, .. } => assert_eq!(event_id, live_id),
        other => panic!("expected the live request, got {other:?}"),
    }
    listener.shutdown();
}

/// A replay of a request this host already reported is skipped by the
/// durable store, not by the listener: the listener delivers it again, and
/// the record already present is what keeps the script from running twice.
#[test]
fn a_replayed_request_this_host_already_reported_is_skipped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = crate::action_step_store::ActionStepStore::open(dir.path()).expect("open");
    let record = crate::action_step_store::ActionStepRecord {
        run_id: "00000000-0000-0000-0000-000000000001".into(),
        step_id: "verify".into(),
        requested_event_id: "cd".repeat(32),
        project: project(),
        workflow_name: "verify".into(),
        channel_id: "00000000-0000-0000-0000-000000000009".into(),
        created_at: now_secs(),
        state: crate::action_step_store::StepState::Requested,
        agents_commit: None,
    };
    assert!(store.insert(record.clone()).expect("insert"));
    store
        .mark_reported(&record.requested_event_id, &"ef".repeat(32))
        .expect("report");

    // The same request, replayed by the next probe.
    assert!(
        !store.insert(record.clone()).expect("replay"),
        "a replayed request must not produce a second record"
    );
    assert_eq!(
        store
            .record(&record.requested_event_id)
            .map(|found| found.state.clone()),
        Some(crate::action_step_store::StepState::Reported {
            result_event_id: "ef".repeat(32),
        }),
        "the replay must leave the reported record exactly as it was"
    );
}

/// An expired request replays on every probe and must not shout: it is a
/// debug line, not the WARN that once repeated on every reconnect.
#[test]
fn an_expired_replayed_request_is_rejected_as_expired_not_as_invalid() {
    let relay_keys = Keys::generate();
    let expired = signed(&relay_keys, &a_request("verify", 1_000));
    let rejection = verify_request_event(&expired, &relay_keys.public_key().to_hex(), 2_000)
        .expect_err("expired");
    assert!(matches!(rejection, RequestRejection::Expired(_)));
}

/// Control run 5's 57 s: the relay's live frame reached the provider's channel
/// subscription but never this listener's `#a` one, so the request waited for
/// the next probe. Offered from the channel, it reaches the run loop at once —
/// here with the probe a minute away and the relay sending nothing live.
#[tokio::test]
async fn a_request_offered_from_the_channel_is_queued_without_waiting_for_a_probe() {
    let relay_keys = Keys::generate();
    let relay_self = relay_keys.public_key().to_hex();
    let (url, _relay) = spawn_relay(Vec::new(), vec![Behaviour::Normal]).await;
    let mut config = listener_config(&url, &relay_self);
    config.probe_interval = Duration::from_secs(60);
    let (listener, mut events) = ActionStepListener::spawn(config);
    listener.watch(BTreeSet::from([project()]));

    let live = signed(&relay_keys, &a_request("verify", far_future()));
    let live_id = live.id.to_hex();
    assert_eq!(listener.offer(&live, &relay_self), RequestOffer::Queued);
    let delivered = tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .expect("queued at once, not at the next probe")
        .expect("the queue is open");
    match delivered {
        ActionStepEvent::Requested { event_id, .. } => assert_eq!(event_id, live_id),
        other => panic!("expected the request, got {other:?}"),
    }

    // The channel path verifies exactly as the listener does.
    let forged = signed(&Keys::generate(), &a_request("verify", far_future()));
    assert!(matches!(
        listener.offer(&forged, &relay_self),
        RequestOffer::Rejected(_)
    ));
    let mut foreign = a_request("verify", far_future());
    foreign.project = format!("30621:{}:someone-else", "22".repeat(32));
    assert!(matches!(
        listener.offer(&signed(&relay_keys, &foreign), &relay_self),
        RequestOffer::NotServed(_)
    ));
    assert!(matches!(
        listener.offer(
            &signed(&relay_keys, &a_request("verify", 1_000)),
            &relay_self
        ),
        RequestOffer::Expired(_)
    ));
    listener.shutdown();
}

/// The channel offer and the listener's probe replay both queue the same
/// request, and the replay can land while the first is still claimed and
/// running. The durable store refuses the second record, which is the run
/// loop's first act, so it is never claimed or run twice.
#[test]
fn a_request_delivered_by_both_the_channel_and_the_probe_is_claimed_once() {
    let relay_keys = Keys::generate();
    let relay_self = relay_keys.public_key().to_hex();
    let event = signed(&relay_keys, &a_request("verify", far_future()));
    let served = BTreeSet::from([project()]);
    let (tx, mut rx) = mpsc::channel(4);
    assert_eq!(
        offer_request(&event, &relay_self, &served, &tx, now_secs()),
        RequestOffer::Queued
    );
    assert_eq!(
        offer_request(&event, &relay_self, &served, &tx, now_secs()),
        RequestOffer::Queued
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = crate::action_step_store::ActionStepStore::open(dir.path()).expect("open");
    let mut inserted = 0;
    while let Ok(ActionStepEvent::Requested { event_id, request }) = rx.try_recv() {
        let record = crate::action_step_store::ActionStepRecord {
            run_id: request.run_id.clone(),
            step_id: request.step_id.clone(),
            requested_event_id: event_id.clone(),
            project: request.project.clone(),
            workflow_name: request.workflow_name.clone(),
            channel_id: request.channel_id.clone(),
            created_at: now_secs(),
            state: crate::action_step_store::StepState::Requested,
            agents_commit: None,
        };
        if store.insert(record).expect("insert") {
            inserted += 1;
            // The first delivery claims before the second is handled.
            store
                .mark_claimed(&event_id, &"ef".repeat(32))
                .expect("claim");
        }
    }
    assert_eq!(
        inserted, 1,
        "two deliveries must make one record, one claim"
    );
    assert_eq!(
        store
            .record(&event.id.to_hex())
            .map(|found| found.state.clone()),
        Some(crate::action_step_store::StepState::Claimed {
            claim_event_id: "ef".repeat(32),
        }),
    );
}
