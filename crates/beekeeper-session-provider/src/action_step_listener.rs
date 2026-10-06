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
//! Stored requests replay on every (re)connect **and on every cadence probe**
//! (see [`LISTENER_PROBE_INTERVAL`]), so a request published while the
//! provider was down — or one whose live frame never arrived over a
//! connection the relay still heartbeats — is found by the next REQ. The
//! provider's durable [`crate::action_step_store`] is what keeps a replay
//! from running anything twice; this task only decides whether an event is a *verifiable* request.
//!
//! The provider's channel subscription offers the same kind:46013 the moment
//! it arrives ([`ActionStepListener::offer`]), because on 2026-09-24 the relay
//! fanned requests out live to that `#h` subscription and not to this `#a`
//! one; the probe is then the backstop, not the pickup.
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
//!
//! # Liveness
//!
//! A relay heartbeat is not evidence that a subscription still delivers. The
//! relay pings every 30 s (`buzz-relay/src/connection.rs` `heartbeat_loop`),
//! so a connection whose subscription has silently stopped feeding this task
//! still looks busy at the socket. This task therefore measures liveness at
//! the application layer: every [`LISTENER_PROBE_INTERVAL`] it closes its
//! subscription and re-issues the REQ under a fresh id, and requires the
//! relay's `EOSE` within [`LISTENER_PROBE_TIMEOUT`]. A missing `EOSE` ends
//! the connection and reconnects. Because the filter carries no `since`, the
//! re-issued REQ replays every stored request, so a missed live frame is
//! recovered at the next probe rather than waiting for a relaunch.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use beekeeper_core::host_step::{decode_host_step_requested, HostStepRequested};
use beekeeper_core::kind::KIND_WORKFLOW_HOST_STEP_REQUESTED;
use beekeeper_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
use nostr::{Event, Keys, Tag};
use serde_json::json;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::host_command::HostCommandOutcome;

/// Prefix of the subscription id the listener's REQ uses. Each re-issue adds
/// a sequence suffix, so the relay's `EOSE` names exactly one probe.
const SUBSCRIPTION_PREFIX: &str = "csp-action-steps";
/// How long one connection waits for any *relay message* before it is
/// recycled. WebSocket pings do not count: see [`crate::action_step_listener`]
/// § Liveness.
pub const LISTENER_IDLE: Duration = Duration::from_secs(900);
/// How often the subscription is closed and re-issued as a liveness probe.
pub const LISTENER_PROBE_INTERVAL: Duration = Duration::from_secs(90);
/// How long a probe waits for the relay's `EOSE` before the connection is
/// declared dead.
pub const LISTENER_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
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
        /// How the tree the command ran in was established, and what it held
        /// before it ran. `None` when no tree was established.
        checkout: Option<Box<beekeeper_core::host_step::HostStepCheckout>>,
        /// The scrubbed logs uploaded after the run, when the step asked.
        artifacts: Vec<beekeeper_core::host_step::HostStepArtifact>,
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
    /// How often the subscription is re-issued as a liveness probe.
    pub probe_interval: Duration,
    /// How long a probe waits for `EOSE` before the connection is dead.
    pub probe_timeout: Duration,
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
            probe_interval: LISTENER_PROBE_INTERVAL,
            probe_timeout: LISTENER_PROBE_TIMEOUT,
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

    /// Offer a kind:46013 that arrived on the provider's **channel**
    /// subscription, so it is claimed now rather than at the next probe.
    ///
    /// Control run 5 (2026-09-24) measured why this exists: the relay fanned
    /// both of its requests out live to the provider's `#h` channel
    /// subscription (the host-result wake indexed each within a second), but
    /// never to this listener's `#a` subscription, which found each one only
    /// on the next [`LISTENER_PROBE_INTERVAL`] replay — 57 s after the
    /// trigger for the verify that closed the run. The probe stays as the
    /// backstop for a request whose channel this provider does not hold.
    pub(crate) fn offer(&self, event: &Event, relay_self: &str) -> RequestOffer {
        let served = self.served.borrow().clone();
        offer_request(event, relay_self, &served, &self.events, now_secs())
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

/// Why a candidate 46013 was not delivered.
///
/// Expiry is separated from every other rejection because it is *routine*:
/// the filter carries no `since`, so every probe replays every stored
/// request, including ones whose claim window closed hours ago. Logging that
/// at WARN once per replay is noise that hides real refusals.
#[derive(Debug)]
pub(crate) enum RequestRejection {
    /// The claim window had already closed when the request was seen.
    Expired(String),
    /// A forgery, a bad signature, a foreign signer, or a malformed body.
    Invalid(String),
}

impl std::fmt::Display for RequestRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expired(reason) | Self::Invalid(reason) => formatter.write_str(reason),
        }
    }
}

/// Verify one candidate 46013: signature, signer, wire shape, window.
///
/// `pub(crate)` so the provider's tests can exercise the exact checks the
/// task applies without a socket.
pub(crate) fn verify_request_event(
    event: &Event,
    relay_self: &str,
    now: u64,
) -> Result<HostStepRequested, RequestRejection> {
    event.verify().map_err(|error| {
        RequestRejection::Invalid(format!(
            "host step request failed cryptographic verification: {error}"
        ))
    })?;
    let signer = event.pubkey.to_hex();
    if signer != relay_self {
        return Err(RequestRejection::Invalid(format!(
            "host step request signer {signer} does not match the witnessed relay self {relay_self}"
        )));
    }
    let request = decode_host_step_requested(event).map_err(|error| {
        RequestRejection::Invalid(format!("invalid host step request: {error}"))
    })?;
    if request.expires_at <= now {
        return Err(RequestRejection::Expired(format!(
            "host step request expired at {} (now {now})",
            request.expires_at
        )));
    }
    Ok(request)
}

/// What became of one kind:46013 offered to the host-step queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RequestOffer {
    /// Verified, for a served project, and queued for the run loop.
    Queued,
    /// Its claim window has closed; nothing an honest host can do with it.
    Expired(String),
    /// Failed signature, signer or schema verification.
    Rejected(String),
    /// Verified, but for a project this host does not serve (the project).
    NotServed(String),
    /// The queue is full; this delivery is dropped and the next probe replays it.
    QueueFull,
    /// The run loop dropped its receiver.
    Closed,
}

/// Verify one candidate kind:46013 and, when it is a live request for a
/// project this host serves, queue it for the run loop.
///
/// Both deliveries use this: the listener's own `#a` subscription, and the
/// provider's channel subscription (see [`ActionStepListener::offer`]).
/// Queuing the same request twice is safe by construction: the run loop's
/// first act is [`crate::action_step_store::ActionStepStore::insert`], which
/// refuses a request id it already holds, so nothing is claimed or run twice.
/// Never `send().await`: the run loop can be inside a long turn, and a
/// blocked caller stops answering the relay altogether.
pub(crate) fn offer_request(
    event: &Event,
    relay_self: &str,
    served: &BTreeSet<String>,
    events: &mpsc::Sender<ActionStepEvent>,
    now: u64,
) -> RequestOffer {
    let request = match verify_request_event(event, relay_self, now) {
        Ok(request) => request,
        Err(RequestRejection::Expired(reason)) => return RequestOffer::Expired(reason),
        Err(rejection) => return RequestOffer::Rejected(rejection.to_string()),
    };
    if !served.contains(&request.project) {
        return RequestOffer::NotServed(request.project);
    }
    match events.try_send(ActionStepEvent::Requested {
        event_id: event.id.to_hex(),
        request: Box::new(request),
    }) {
        Ok(()) => RequestOffer::Queued,
        Err(mpsc::error::TrySendError::Full(_)) => RequestOffer::QueueFull,
        Err(mpsc::error::TrySendError::Closed(_)) => RequestOffer::Closed,
    }
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

/// Compose the subscription id one probe uses.
fn subscription_id(sequence: u64) -> String {
    format!("{SUBSCRIPTION_PREFIX}-{sequence}")
}

/// Whether a subscription id in a relay frame is one of ours.
///
/// A frame from the *previous* probe's id can still be in flight when the
/// next REQ goes out; those events are as good as any other, so they are
/// accepted rather than dropped.
fn ours(subscription_id: &str) -> bool {
    subscription_id.starts_with(SUBSCRIPTION_PREFIX)
}

/// Send `CLOSE` for the previous subscription (when there was one) and `REQ`
/// the new one, logging the served set the filter names.
async fn issue_subscription(
    conn: &mut NostrWsConnection,
    wanted: &BTreeSet<String>,
    previous: Option<&str>,
    sequence: u64,
) -> Result<String, String> {
    if let Some(previous) = previous {
        conn.send_raw(&json!(["CLOSE", previous]))
            .await
            .map_err(|error| format!("close previous subscription: {error}"))?;
    }
    let id = subscription_id(sequence);
    let projects: Vec<&String> = wanted.iter().collect();
    let filter = json!({"kinds": [KIND_WORKFLOW_HOST_STEP_REQUESTED], "#a": projects});
    conn.send_raw(&json!(["REQ", &id, filter]))
        .await
        .map_err(|error| format!("subscribe: {error}"))?;
    tracing::info!(
        target: "csp::actions",
        subscription_id = %id,
        served = wanted.len(),
        projects = %wanted.iter().cloned().collect::<Vec<_>>().join(","),
        "action step listener subscribed"
    );
    Ok(id)
}

/// The `d` tag of a candidate, read before anything is verified, so the log
/// names the run even when verification then refuses the event.
fn requested_d_tag(event: &Event) -> String {
    event
        .tags
        .iter()
        .map(nostr::Tag::as_slice)
        .find(|values| values.first().map(String::as_str) == Some("d"))
        .and_then(|values| values.get(1).cloned())
        .unwrap_or_else(|| "<none>".to_owned())
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

    let mut sequence: u64 = 0;
    let mut current = issue_subscription(&mut conn, wanted, None, sequence).await?;
    let mut last_message = Instant::now();
    let mut probe_at = last_message + config.probe_interval;
    // The opening REQ is itself a probe: its `EOSE` is the first proof that
    // this subscription is alive.
    let mut eose_by = Some(last_message + config.probe_timeout);

    loop {
        // Deadlines are settled at the top of every pass, not only when the
        // read times out: a connection that keeps *delivering* while it owes
        // an EOSE is still a connection whose subscription is unproven, and
        // one that keeps timing out must still probe on the cadence.
        let now = Instant::now();
        if eose_by.is_some_and(|deadline| now >= deadline) {
            return Err(format!(
                "the relay did not answer subscription {current} with EOSE within {} s",
                config.probe_timeout.as_secs()
            ));
        }
        if now.saturating_duration_since(last_message) >= config.idle {
            return Err("no relay message within the idle window".into());
        }
        if now >= probe_at {
            sequence += 1;
            current = issue_subscription(&mut conn, wanted, Some(&current), sequence).await?;
            probe_at = now + config.probe_interval;
            eose_by = Some(now + config.probe_timeout);
            continue;
        }

        let mut wait = config
            .idle
            .saturating_sub(now.saturating_duration_since(last_message))
            .min(probe_at.saturating_duration_since(now));
        if let Some(deadline) = eose_by {
            wait = wait.min(deadline.saturating_duration_since(now));
        }
        // Never a zero wait: a deadline already passed is handled below, and
        // a zero timeout would spin.
        let wait = wait.max(Duration::from_millis(1));

        let message = tokio::select! {
            changed = served.changed() => {
                return match changed {
                    Ok(()) => Ok(ConnectionEnd::ServedChanged),
                    Err(_) => Ok(ConnectionEnd::Shutdown),
                };
            }
            message = conn.next_event(wait) => message,
        };
        if message.is_ok() {
            last_message = Instant::now();
        }
        match message {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if ours(&subscription_id) => {
                let event_id = event.id.to_hex();
                tracing::info!(
                    target: "csp::actions",
                    %event_id,
                    run = %requested_d_tag(&event),
                    "host step request received"
                );
                match offer_request(&event, &config.relay_self, wanted, events, now_secs()) {
                    RequestOffer::Queued => {}
                    // Routine on every replay: the stored request outlives its
                    // own claim window and there is nothing to do.
                    RequestOffer::Expired(reason) => tracing::debug!(
                        target: "csp::actions",
                        %event_id,
                        "skipped a replayed host step request: {reason}"
                    ),
                    RequestOffer::Rejected(reason) => tracing::warn!(
                        target: "csp::actions",
                        %event_id,
                        "skipped a candidate host step request: {reason}"
                    ),
                    RequestOffer::NotServed(project) => tracing::debug!(
                        target: "csp::actions",
                        %event_id,
                        %project,
                        "host step request for a project this host does not serve ignored"
                    ),
                    RequestOffer::QueueFull => tracing::warn!(
                        target: "csp::actions",
                        %event_id,
                        capacity = LISTENER_EVENT_CAPACITY,
                        "the host step queue is full; this delivery is dropped and will be \
                         replayed by the next subscription probe"
                    ),
                    RequestOffer::Closed => return Ok(ConnectionEnd::Shutdown),
                }
            }
            Ok(RelayMessage::Eose { subscription_id }) if subscription_id == current => {
                eose_by = None;
            }
            Ok(RelayMessage::Closed {
                subscription_id,
                message,
            }) if subscription_id == current => {
                return Err(format!(
                    "relay closed the action step subscription: {message}"
                ));
            }
            Ok(RelayMessage::Notice { message }) => {
                tracing::debug!(target: "csp::actions", "relay notice: {message}");
            }
            Ok(_) => {}
            // Nothing arrived inside the shortest deadline; the top of the
            // loop decides what that means.
            Err(WsClientError::Timeout) => {}
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
#[path = "action_step_listener_tests.rs"]
mod liveness_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core::host_step::{
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
            inputs: serde_json::json!({}),
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
            .to_string()
            .contains("does not match the witnessed relay self"));

        // Expiry is its own rejection: the log demotes it to debug because
        // every probe replays stored requests whose window has closed.
        let stale = signed(&relay, &request(2_000));
        let rejection = verify_request_event(&stale, &relay_self, 2_000).unwrap_err();
        assert!(matches!(rejection, RequestRejection::Expired(_)));
        assert!(rejection.to_string().contains("expired"));
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
