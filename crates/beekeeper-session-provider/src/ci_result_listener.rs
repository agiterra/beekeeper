//! One bounded, authenticated relay listener for every pending CI result.
//!
//! # Why one listener and not a poll
//!
//! A CI continuation is a promise to watch for one exact run attempt until it
//! finishes or the registration expires — hours, in the ordinary case. Polling
//! that would mean a timer per registration and a relay query per tick, and a
//! task per registration would mean an unbounded number of sockets whose
//! failure modes nobody watches. This is instead **one** task for the whole
//! provider holding **one** authenticated subscription, re-issued whenever the
//! pending set changes:
//!
//! ```text
//! {"kinds":[46008], "#d":[<every pending correlation digest>]}
//! ```
//!
//! Stored results replay on every (re)connect, so a result recorded while the
//! provider was down is found by the first REQ rather than by a special
//! catch-up path. Live results arrive on the same socket.
//!
//! # What it decides, and what it refuses to decide
//!
//! The listener answers exactly three things per digest and posts them to the
//! provider's main loop, which owns every durable consequence:
//!
//! - **ready** — one canonical verified result exists.
//! - **conflict** — more than one *distinct* canonical result exists for one
//!   digest, so there is no single fact to deliver.
//! - **answered** — the relay served this provider's subscription to `EOSE`.
//!   Not a result: it is the witnessed fact that separates "the run has not
//!   reported" from "this provider never got to look", which is the whole
//!   difference between the two expiry dispositions.
//!
//! Verification is the same four checks `bee ci wait` makes
//! (`crates/beekeeper-cli/src/commands/ci.rs:126-159`, reimplemented here rather
//! than shared because those helpers are private to the CLI): Schnorr
//! signature, signer equals the witnessed relay self, `decode_ci_result`, and
//! an exact byte-compare of the canonical identity against the registration's.
//! Anything that fails any of them is not a result — it is logged and dropped,
//! and the registration keeps waiting.
//!
//! # What it reads with
//!
//! The provider's own key, and nothing else. Results are private-project gated
//! for the reader, so a project this provider's identity may not read answers
//! empty — indistinguishable from "not finished", which is exactly why the
//! expiry disposition reports the observation rather than a cause.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use beekeeper_core::ci_result::{decode_ci_result, CiResultIdentity};
use beekeeper_core::kind::KIND_CI_RESULT;
use beekeeper_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
use nostr::{Event, Keys, Tag};
use serde_json::json;
use tokio::sync::{mpsc, watch};

/// Subscription id the listener's single REQ uses.
const SUBSCRIPTION_ID: &str = "csp-ci-continuations";
/// How long one connection waits for any frame before it is recycled.
pub const LISTENER_IDLE: Duration = Duration::from_secs(900);
/// First reconnect delay after a failed or dropped connection.
pub const LISTENER_FIRST_BACKOFF: Duration = Duration::from_secs(1);
/// Ceiling on the exponential reconnect delay.
pub const LISTENER_MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Backlog of listener reports the provider loop will buffer.
pub const LISTENER_EVENT_CAPACITY: usize = 64;

/// What the listener tells the provider's main loop.
///
/// Keyed by correlation digest rather than by `commandId`: two registrations
/// may name one identity, and which command ids a digest backs is durable
/// state the main loop owns. The loop resolves digest to command ids through
/// [`crate::ci_continuation_store::CiContinuationStore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiListenerEvent {
    /// Exactly one canonical verified result exists for this digest.
    CiResultReady {
        /// Correlation digest the result was tagged with.
        digest: String,
        /// Event id of the relay-signed result.
        event_id: String,
        /// Lowercase hex pubkey that signed it (the witnessed relay self).
        signer: String,
        /// The decoded result re-encoded canonically.
        canonical_json: String,
        /// Epoch seconds at which this provider verified it.
        observed_at: u64,
    },
    /// More than one distinct canonical result exists for this digest.
    CiResultConflict {
        /// Correlation digest with contradictory results.
        digest: String,
    },
    /// The relay served the subscription covering these digests to `EOSE`.
    CiResultsAnswered {
        /// Every digest the answered subscription covered.
        digests: Vec<String>,
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
    ///
    /// The trust root for every result. There is deliberately no fallback: a
    /// provider that never witnessed a relay identity cannot verify a result
    /// signer, so it verifies nothing (fail closed) and registrations expire
    /// with the honest `CI_RESULT_UNAVAILABLE_OR_HIDDEN` / expired
    /// disposition instead of delivering an unverified fact.
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
pub struct CiResultListener {
    pending: watch::Sender<Arc<BTreeMap<String, CiResultIdentity>>>,
    task: tokio::task::JoinHandle<()>,
}

impl CiResultListener {
    /// Start the listener, returning the handle and the receiver the main loop
    /// selects on.
    pub fn spawn(config: ListenerConfig) -> (Self, mpsc::Receiver<CiListenerEvent>) {
        let (events_tx, events_rx) = mpsc::channel(LISTENER_EVENT_CAPACITY);
        let (pending, pending_rx) = watch::channel(Arc::new(BTreeMap::new()));
        let task = tokio::spawn(run(config, pending_rx, events_tx));
        (Self { pending, task }, events_rx)
    }

    /// Replace the set of identities being watched.
    ///
    /// A no-op when the set is unchanged, so the ordinary tick does not churn
    /// the subscription. The listener re-issues its REQ on a real change.
    pub fn watch(&self, wanted: BTreeMap<String, CiResultIdentity>) {
        if **self.pending.borrow() == wanted {
            return;
        }
        // A closed receiver means the task is gone; the provider keeps every
        // durable promise either way and the expiry tick still answers them.
        let _ = self.pending.send(Arc::new(wanted));
    }

    /// Stop the listener task.
    pub fn shutdown(self) {
        self.task.abort();
    }
}

/// A result that passed every verification check.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AcceptedResult {
    event_id: String,
    /// Canonical re-encoding of the decoded result; the fold's identity.
    canonical: Vec<u8>,
}

/// What one canonical-set fold says about a digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DigestVerdict {
    Ready,
    Conflict,
}

/// Verify one candidate 46008 against the registration that is waiting on it.
///
/// Reimplements `bee ci wait`'s `verify_result_event`
/// (`crates/beekeeper-cli/src/commands/ci.rs:126`) because that function is private
/// to the CLI and returns `CliError`. The four checks and their order are the
/// same, deliberately: a result the CLI would refuse must not wake an agent.
fn verify_result_event(
    event: &Event,
    relay_self: &str,
    expected: &CiResultIdentity,
) -> Result<AcceptedResult, String> {
    event
        .verify()
        .map_err(|error| format!("CI result failed cryptographic verification: {error}"))?;
    let signer = event.pubkey.to_hex();
    if signer != relay_self {
        return Err(format!(
            "CI result signer {signer} does not match the witnessed relay self {relay_self}"
        ));
    }
    let result = decode_ci_result(event).map_err(|error| format!("invalid CI result: {error}"))?;
    let actual = serde_json::to_vec(&result.identity)
        .map_err(|error| format!("cannot encode CI identity: {error}"))?;
    let wanted = serde_json::to_vec(expected)
        .map_err(|error| format!("cannot encode expected CI identity: {error}"))?;
    if actual != wanted {
        return Err("CI result names a different exact identity".into());
    }
    let canonical =
        serde_json::to_vec(&result).map_err(|error| format!("cannot encode CI result: {error}"))?;
    Ok(AcceptedResult {
        event_id: event.id.to_hex(),
        canonical,
    })
}

/// The `d` tag one result is correlated by, if it has exactly one.
fn correlation_tag(event: &Event) -> Option<String> {
    let mut found = None;
    for tag in event.tags.iter() {
        let tag = tag.as_slice();
        if tag.len() == 2 && tag[0] == "d" {
            if found.is_some() {
                return None;
            }
            found = Some(tag[1].clone());
        }
    }
    found
}

/// Verified canonical results seen on one connection, per digest.
#[derive(Debug, Default)]
struct Fold {
    canonical: BTreeMap<String, BTreeMap<Vec<u8>, AcceptedResult>>,
}

impl Fold {
    fn insert(&mut self, digest: &str, accepted: AcceptedResult) {
        self.canonical
            .entry(digest.to_owned())
            .or_default()
            .entry(accepted.canonical.clone())
            .or_insert(accepted);
    }

    /// The verdict for one digest, or `None` when nothing has been seen.
    fn verdict(&self, digest: &str) -> Option<(DigestVerdict, &AcceptedResult)> {
        let seen = self.canonical.get(digest)?;
        let first = seen.values().next()?;
        Some(if seen.len() > 1 {
            (DigestVerdict::Conflict, first)
        } else {
            (DigestVerdict::Ready, first)
        })
    }
}

/// Epoch seconds, saturating at zero before the epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Why one connection ended.
#[derive(Debug)]
enum ConnectionEnd {
    /// The watched set changed; reconnect immediately with the new filter.
    PendingChanged,
    /// The provider dropped the handle or the receiver.
    Shutdown,
}

async fn run(
    config: ListenerConfig,
    mut pending: watch::Receiver<Arc<BTreeMap<String, CiResultIdentity>>>,
    events: mpsc::Sender<CiListenerEvent>,
) {
    let mut backoff = config.first_backoff;
    loop {
        let wanted = pending.borrow_and_update().clone();
        if wanted.is_empty() {
            if pending.changed().await.is_err() {
                return;
            }
            backoff = config.first_backoff;
            continue;
        }
        match serve_one_connection(&config, &wanted, &mut pending, &events).await {
            Ok(ConnectionEnd::PendingChanged) => backoff = config.first_backoff,
            Ok(ConnectionEnd::Shutdown) => return,
            Err(reason) => {
                tracing::warn!(
                    target: "csp::ci",
                    backoff_secs = backoff.as_secs(),
                    "CI result listener connection ended: {reason}"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(config.max_backoff);
            }
        }
    }
}

async fn serve_one_connection(
    config: &ListenerConfig,
    wanted: &BTreeMap<String, CiResultIdentity>,
    pending: &mut watch::Receiver<Arc<BTreeMap<String, CiResultIdentity>>>,
    events: &mpsc::Sender<CiListenerEvent>,
) -> Result<ConnectionEnd, String> {
    let mut conn = NostrWsConnection::connect_authenticated(
        &config.relay_url,
        &config.keys,
        config.auth_tag.as_ref(),
    )
    .await
    .map_err(|error| format!("connect: {error}"))?;

    let digests: Vec<String> = wanted.keys().cloned().collect();
    let filter = json!({"kinds": [KIND_CI_RESULT], "#d": digests});
    conn.send_raw(&json!(["REQ", SUBSCRIPTION_ID, filter]))
        .await
        .map_err(|error| format!("subscribe: {error}"))?;

    let mut fold = Fold::default();
    let mut emitted: HashMap<String, DigestVerdict> = HashMap::new();
    let mut replay_complete = false;

    loop {
        let message = tokio::select! {
            changed = pending.changed() => {
                return match changed {
                    Ok(()) => Ok(ConnectionEnd::PendingChanged),
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
                let Some(digest) = correlation_tag(&event) else {
                    tracing::debug!(target: "csp::ci", "CI result without one d tag ignored");
                    continue;
                };
                let Some(expected) = wanted.get(&digest) else {
                    tracing::debug!(
                        target: "csp::ci",
                        %digest,
                        "CI result for a digest this provider is not watching ignored"
                    );
                    continue;
                };
                match verify_result_event(&event, &config.relay_self, expected) {
                    Ok(accepted) => fold.insert(&digest, accepted),
                    Err(reason) => {
                        // Not a result. The registration keeps waiting, and
                        // nothing is published: an unverifiable event is not
                        // evidence of anything an agent should be woken for.
                        tracing::warn!(
                            target: "csp::ci",
                            %digest,
                            event_id = %event.id.to_hex(),
                            "rejected a candidate CI result: {reason}"
                        );
                        continue;
                    }
                }
                if replay_complete
                    && !publish_verdict(&fold, &digest, &config.relay_self, &mut emitted, events)
                        .await
                {
                    return Ok(ConnectionEnd::Shutdown);
                }
            }
            Ok(RelayMessage::Eose { subscription_id }) if subscription_id == SUBSCRIPTION_ID => {
                replay_complete = true;
                // The witnessed fact, sent before any verdict: the relay
                // served this provider's own identity the stored set for
                // every digest in the filter, empty or not.
                if events
                    .send(CiListenerEvent::CiResultsAnswered {
                        digests: digests.clone(),
                    })
                    .await
                    .is_err()
                {
                    return Ok(ConnectionEnd::Shutdown);
                }
                for digest in &digests {
                    if !publish_verdict(&fold, digest, &config.relay_self, &mut emitted, events)
                        .await
                    {
                        return Ok(ConnectionEnd::Shutdown);
                    }
                }
            }
            Ok(RelayMessage::Closed {
                subscription_id,
                message,
            }) if subscription_id == SUBSCRIPTION_ID => {
                return Err(format!("relay closed the CI subscription: {message}"));
            }
            Ok(RelayMessage::Notice { message }) => {
                tracing::debug!(target: "csp::ci", "relay notice: {message}");
            }
            Ok(_) => {}
            Err(WsClientError::Timeout) => {
                return Err("no relay frame within the idle window".into());
            }
            Err(error) => return Err(format!("receive: {error}")),
        }
    }
}

/// Send the verdict for `digest` if it is new or has hardened into a conflict.
///
/// Answers `false` when the provider's receiver is gone.
///
/// A repeat of a canonical result the fold already holds changes nothing, so
/// it produces no event: a duplicate result must not look like a second fact.
/// The one transition that *is* published twice is ready → conflict, because
/// a contradicting result is new information about a delivery that has not
/// happened yet.
async fn publish_verdict(
    fold: &Fold,
    digest: &str,
    relay_self: &str,
    emitted: &mut HashMap<String, DigestVerdict>,
    events: &mpsc::Sender<CiListenerEvent>,
) -> bool {
    let Some((verdict, accepted)) = fold.verdict(digest) else {
        return true;
    };
    let already = emitted.get(digest).copied();
    if already == Some(verdict) || already == Some(DigestVerdict::Conflict) {
        return true;
    }
    let event = match verdict {
        DigestVerdict::Conflict => CiListenerEvent::CiResultConflict {
            digest: digest.to_owned(),
        },
        DigestVerdict::Ready => CiListenerEvent::CiResultReady {
            digest: digest.to_owned(),
            event_id: accepted.event_id.clone(),
            signer: relay_self.to_owned(),
            canonical_json: String::from_utf8_lossy(&accepted.canonical).into_owned(),
            observed_at: now_secs(),
        },
    };
    if events.send(event).await.is_err() {
        return false;
    }
    emitted.insert(digest.to_owned(), verdict);
    true
}

#[cfg(test)]
#[path = "ci_result_listener_tests.rs"]
mod tests;
