//! One bounded, authenticated relay listener for host-step requests.
//!
//! Mirrors [`crate::ci_result_listener`]: **one** task for the whole provider
//! holding **one** authenticated subscription, re-issued whenever the set of
//! served projects changes:
//!
//! ```text
//! {"kinds":[46013], "#a":[<every project coordinate this host serves>]}
//! ```
//!
//! Stored requests replay on every (re)connect, so a request published while
//! the provider was down is found by the first REQ. The provider's durable
//! [`crate::action_step_store`] is what keeps a replay from running anything
//! twice; this task only decides whether an event is a *verifiable* request.
//!
//! # What it verifies
//!
//! Schnorr signature, signer equals the witnessed relay `self` (the trust
//! root, exactly as for CI results — fail closed, no fallback), and
//! [`decode_host_step_requested`] (schema, exact tags). A request whose claim
//! window has already closed is dropped here too: there is nothing an honest
//! host can do with it. Anything that fails is logged and skipped, never
//! delivered.
//!
//! The same queue also carries the completion of every spawned command
//! ([`ActionStepEvent::Finished`]), so the run loop selects on one receiver
//! for the whole feature.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use buzz_core::host_step::{decode_host_step_requested, HostStepRequested};
use buzz_core::kind::KIND_WORKFLOW_HOST_STEP_REQUESTED;
use buzz_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
use nostr::{Event, Keys, Tag};
use serde_json::json;
use tokio::sync::{mpsc, watch};

use crate::host_command::HostCommandOutcome;

/// Subscription id the listener's single REQ uses.
const SUBSCRIPTION_ID: &str = "csp-action-steps";
/// How long one connection waits for any frame before it is recycled.
pub const LISTENER_IDLE: Duration = Duration::from_secs(900);
/// First reconnect delay after a failed or dropped connection.
pub const LISTENER_FIRST_BACKOFF: Duration = Duration::from_secs(1);
/// Ceiling on the exponential reconnect delay.
pub const LISTENER_MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Backlog of events the provider loop will buffer.
pub const LISTENER_EVENT_CAPACITY: usize = 64;

/// What reaches the provider's main loop.
#[derive(Debug)]
pub enum ActionStepEvent {
    /// A verified kind:46013 for a project this host serves.
    Requested {
        /// Event id of the request — the record key and idempotency token.
        event_id: String,
        /// The decoded request.
        request: Box<HostStepRequested>,
    },
    /// A spawned command finished (or timed out).
    Finished {
        /// Event id of the request the command answered.
        requested_event_id: String,
        /// What the command produced.
        outcome: Box<HostCommandOutcome>,
        /// `git rev-parse HEAD` of the checkout after the run.
        head_sha: Option<String>,
        /// Whether the checkout had uncommitted changes after the run.
        dirty: Option<bool>,
    },
}

/// Everything the listener task needs that does not change while it runs.
#[derive(Clone)]
pub struct ListenerConfig {
    /// Relay WebSocket URL.
    pub relay_url: String,
    /// This provider's signing keys — the only credential the listener uses.
    pub keys: Keys,
    /// NIP-OA authorization tag, when the deployment requires one.
    pub auth_tag: Option<Tag>,
    /// The relay's own pubkey, witnessed from NIP-11 `self` at startup.
    pub relay_self: String,
    /// Idle ceiling for one connection.
    pub idle: Duration,
    /// First reconnect delay.
    pub first_backoff: Duration,
    /// Ceiling on the reconnect delay.
    pub max_backoff: Duration,
}

impl ListenerConfig {
    /// Build a configuration with this crate's default timing.
    pub fn new(relay_url: String, keys: Keys, auth_tag: Option<Tag>, relay_self: String) -> Self {
        Self {
            relay_url,
            keys,
            auth_tag,
            relay_self,
            idle: LISTENER_IDLE,
            first_backoff: LISTENER_FIRST_BACKOFF,
            max_backoff: LISTENER_MAX_BACKOFF,
        }
    }
}

/// The provider's handle on the running listener task.
#[derive(Debug)]
pub struct ActionStepListener {
    served: watch::Sender<Arc<BTreeSet<String>>>,
    events: mpsc::Sender<ActionStepEvent>,
    task: tokio::task::JoinHandle<()>,
}

impl ActionStepListener {
    /// Start the listener, returning the handle and the receiver the main loop
    /// selects on.
    pub fn spawn(config: ListenerConfig) -> (Self, mpsc::Receiver<ActionStepEvent>) {
        let (events_tx, events_rx) = mpsc::channel(LISTENER_EVENT_CAPACITY);
        let (served, served_rx) = watch::channel(Arc::new(BTreeSet::new()));
        let task = tokio::spawn(run(config, served_rx, events_tx.clone()));
        (
            Self {
                served,
                events: events_tx,
                task,
            },
            events_rx,
        )
    }

    /// Replace the set of served project coordinates.
    ///
    /// A no-op when the set is unchanged, so the ordinary tick does not churn
    /// the subscription. The listener re-issues its REQ on a real change.
    pub fn watch(&self, projects: BTreeSet<String>) {
        if **self.served.borrow() == projects {
            return;
        }
        let _ = self.served.send(Arc::new(projects));
    }

    /// A sender a spawned command task reports its completion through.
    pub fn reporter(&self) -> mpsc::Sender<ActionStepEvent> {
        self.events.clone()
    }

    /// Stop the listener task.
    pub fn shutdown(self) {
        self.task.abort();
    }
}

/// Epoch seconds, saturating at zero before the epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Verify one candidate 46013: signature, signer, wire shape, window.
///
/// `pub(crate)` so the provider's tests can exercise the exact checks the
/// task applies without a socket.
pub(crate) fn verify_request_event(
    event: &Event,
    relay_self: &str,
    now: u64,
) -> Result<HostStepRequested, String> {
    event
        .verify()
        .map_err(|error| format!("host step request failed cryptographic verification: {error}"))?;
    let signer = event.pubkey.to_hex();
    if signer != relay_self {
        return Err(format!(
            "host step request signer {signer} does not match the witnessed relay self {relay_self}"
        ));
    }
    let request = decode_host_step_requested(event)
        .map_err(|error| format!("invalid host step request: {error}"))?;
    if request.expires_at <= now {
        return Err(format!(
            "host step request expired at {} (now {now})",
            request.expires_at
        ));
    }
    Ok(request)
}

/// Why one connection ended.
#[derive(Debug)]
enum ConnectionEnd {
    /// The served set changed; reconnect immediately with the new filter.
    ServedChanged,
    /// The provider dropped the handle or the receiver.
    Shutdown,
}

async fn run(
    config: ListenerConfig,
    mut served: watch::Receiver<Arc<BTreeSet<String>>>,
    events: mpsc::Sender<ActionStepEvent>,
) {
    let mut backoff = config.first_backoff;
    loop {
        let wanted = served.borrow_and_update().clone();
        if wanted.is_empty() {
            if served.changed().await.is_err() {
                return;
            }
            backoff = config.first_backoff;
            continue;
        }
        match serve_one_connection(&config, &wanted, &mut served, &events).await {
            Ok(ConnectionEnd::ServedChanged) => backoff = config.first_backoff,
            Ok(ConnectionEnd::Shutdown) => return,
            Err(reason) => {
                tracing::warn!(
                    target: "csp::actions",
                    backoff_secs = backoff.as_secs(),
                    "action step listener connection ended: {reason}"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(config.max_backoff);
            }
        }
    }
}

async fn serve_one_connection(
    config: &ListenerConfig,
    wanted: &BTreeSet<String>,
    served: &mut watch::Receiver<Arc<BTreeSet<String>>>,
    events: &mpsc::Sender<ActionStepEvent>,
) -> Result<ConnectionEnd, String> {
    let mut conn = NostrWsConnection::connect_authenticated(
        &config.relay_url,
        &config.keys,
        config.auth_tag.as_ref(),
    )
    .await
    .map_err(|error| format!("connect: {error}"))?;

    let projects: Vec<&String> = wanted.iter().collect();
    let filter = json!({"kinds": [KIND_WORKFLOW_HOST_STEP_REQUESTED], "#a": projects});
    conn.send_raw(&json!(["REQ", SUBSCRIPTION_ID, filter]))
        .await
        .map_err(|error| format!("subscribe: {error}"))?;

    loop {
        let message = tokio::select! {
            changed = served.changed() => {
                return match changed {
                    Ok(()) => Ok(ConnectionEnd::ServedChanged),
                    Err(_) => Ok(ConnectionEnd::Shutdown),
                };
            }
            message = conn.next_event(config.idle) => message,
        };
        match message {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == SUBSCRIPTION_ID => {
                let event_id = event.id.to_hex();
                let request = match verify_request_event(&event, &config.relay_self, now_secs()) {
                    Ok(request) => request,
                    Err(reason) => {
                        tracing::warn!(
                            target: "csp::actions",
                            %event_id,
                            "skipped a candidate host step request: {reason}"
                        );
                        continue;
                    }
                };
                if !wanted.contains(&request.project) {
                    tracing::debug!(
                        target: "csp::actions",
                        %event_id,
                        project = %request.project,
                        "host step request for a project this host does not serve ignored"
                    );
                    continue;
                }
                if events
                    .send(ActionStepEvent::Requested {
                        event_id,
                        request: Box::new(request),
                    })
                    .await
                    .is_err()
                {
                    return Ok(ConnectionEnd::Shutdown);
                }
            }
            Ok(RelayMessage::Closed {
                subscription_id,
                message,
            }) if subscription_id == SUBSCRIPTION_ID => {
                return Err(format!(
                    "relay closed the action step subscription: {message}"
                ));
            }
            Ok(RelayMessage::Notice { message }) => {
                tracing::debug!(target: "csp::actions", "relay notice: {message}");
            }
            Ok(_) => {}
            Err(WsClientError::Timeout) => {
                return Err("no relay frame within the idle window".into());
            }
            Err(error) => return Err(format!("receive: {error}")),
        }
    }
}

/// Wait for the next event, or park forever once there is no queue.
///
/// Held as an `Option` outside the provider for the same reason the CI queue
/// is: the run loop's `select!` already borrows the provider mutably.
pub async fn next_action_step_event(
    events: &mut Option<mpsc::Receiver<ActionStepEvent>>,
) -> ActionStepEvent {
    let closed = match events {
        Some(queue) => match queue.recv().await {
            Some(event) => return event,
            None => true,
        },
        None => false,
    };
    if closed {
        *events = None;
        tracing::warn!(
            target: "csp::actions",
            "the action step listener stopped; host step requests will not be served until \
             the provider restarts"
        );
    }
    std::future::pending().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::host_step::{
        build_host_step_requested, HOST_STEP_KIND_RUN_ON_HOST, HOST_STEP_SCHEMA,
    };
    use nostr::{EventBuilder, Kind};

    fn request(expires_at: u64) -> HostStepRequested {
        HostStepRequested {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            workflow_id: "00000000-0000-0000-0000-000000000002".into(),
            workflow_name: "nightly".into(),
            step_id: "build".into(),
            step_index: 0,
            definition_hash: "ab".repeat(32),
            step_kind: HOST_STEP_KIND_RUN_ON_HOST.into(),
            channel_id: "00000000-0000-0000-0000-000000000009".into(),
            project: format!("30621:{}:pulse", "11".repeat(32)),
            approval: None,
            trigger_context: serde_json::json!({}),
            expires_at,
        }
    }

    fn signed(keys: &Keys, request: &HostStepRequested) -> Event {
        let (tags, content) = build_host_step_requested(request).expect("build");
        let tags: Vec<Tag> = tags
            .into_iter()
            .map(|tag| Tag::parse(tag).expect("tag"))
            .collect();
        EventBuilder::new(
            Kind::Custom(KIND_WORKFLOW_HOST_STEP_REQUESTED as u16),
            content,
        )
        .tags(tags)
        .sign_with_keys(keys)
        .expect("sign")
    }

    #[test]
    fn only_the_witnessed_relay_can_request_and_only_before_expiry() {
        let relay = Keys::generate();
        let relay_self = relay.public_key().to_hex();
        let event = signed(&relay, &request(2_000));
        assert_eq!(
            verify_request_event(&event, &relay_self, 1_000).expect("verified"),
            request(2_000)
        );

        let other = Keys::generate();
        let forged = signed(&other, &request(2_000));
        assert!(verify_request_event(&forged, &relay_self, 1_000)
            .unwrap_err()
            .contains("does not match the witnessed relay self"));

        let stale = signed(&relay, &request(2_000));
        assert!(verify_request_event(&stale, &relay_self, 2_000)
            .unwrap_err()
            .contains("expired"));
    }

    #[tokio::test]
    async fn watching_the_same_set_twice_does_not_notify() {
        let (listener, _events) = ActionStepListener::spawn(ListenerConfig::new(
            "ws://127.0.0.1:1".into(),
            Keys::generate(),
            None,
            "00".repeat(32),
        ));
        let mut rx = listener.served.subscribe();
        rx.borrow_and_update();
        listener.watch(BTreeSet::new());
        assert!(!rx.has_changed().expect("open"));
        listener.watch(BTreeSet::from(["a".to_owned()]));
        assert!(rx.has_changed().expect("open"));
        listener.shutdown();
    }
}
