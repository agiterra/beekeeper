//! Durable publish outbox.
//!
//! Every provider-authored event is written to `outbox.jsonl` *before* it is
//! handed to the relay, and removed only after the relay accepted it. A relay
//! outage, a dropped socket, or a crash therefore costs latency rather than
//! history: on restart the pending rows are replayed.
//!
//! Replay is safe because it is fenced three ways:
//!
//! - **semantic key** — one row per `(kind, semantic key)`. That tuple *is* the
//!   consumer's dedupe key, so re-enqueueing the same fact is a no-op rather
//!   than a second event a consumer would have to reconcile.
//! - **signer** — rows signed by a key this process no longer holds are dropped
//!   at load. After a key rotation the old rows would be published under an
//!   identity the consumer's trusted-signer set no longer recognizes, so they
//!   are unpublishable by construction; keeping them would only guarantee a
//!   permanently stuck queue.
//! - **priority** — receipts and terminal turn items drain before ordinary
//!   transcript chatter, because a consumer blocked on a receipt is blocked on
//!   the whole session, while a delayed mid-turn chunk is only a delayed chunk.
//!
//! Retry is for *transport* failures only. A relay that answers "this event is
//! invalid" has given a verdict on the bytes, and those bytes never change: the
//! row is parked — durably finished, with the reason and the time recorded —
//! rather than retried forever. Without that distinction one permanently
//! rejected row at high priority is enough to starve every fresh row behind it,
//! because the drain stops at the first error on every pass (ledger 170).

use beekeeper_acp::relay::AcknowledgedPublishError;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::Instant;

use serde::{Deserialize, Serialize};

use crate::state::atomic_write;

const OUTBOX_FILE: &str = "outbox.jsonl";
/// Rewrite the ledger once it exceeds this size and nothing is pending.
const COMPACT_THRESHOLD_BYTES: u64 = 256 * 1024;
/// First retry delay after a failed publish.
pub const RETRY_BASE: Duration = Duration::from_secs(1);
/// Ceiling on the exponential retry delay.
pub const RETRY_MAX: Duration = Duration::from_secs(60);

/// How far an event's `created_at` may sit from the relay's own clock.
///
/// The relay refuses anything further out with
/// `invalid: event timestamp too far from server time` — see
/// `MAX_TIMESTAMP_DRIFT_SECS` in `crates/beekeeper-relay/src/handlers/ingest.rs`
/// (line 3502 at the time of writing). A row that has aged past this window
/// can never be accepted as signed, so sending it is pure waste.
pub const RELAY_TIMESTAMP_WINDOW_SECS: i64 = 900;

/// The envelope `RelayEventPublisher` wraps a negative NIP-01 `OK` message in
/// (`resolve_acknowledged_publish` in `crates/beekeeper-acp/src/relay.rs`).
/// Everything after it is the relay's own machine-readable reason.
const RELAY_REJECTION_ENVELOPE: &str = "relay rejected durable event: ";

/// NIP-01 `OK` reason prefixes that are final for the event *as signed*.
///
/// `invalid:` is a verdict on the event's own bytes and `blocked:` is a verdict
/// on its signer; re-sending the identical, identically signed event can only
/// earn the identical answer. Deliberately excluded: `rate-limited:` and
/// `error:` (the relay is asking for later, not for never), `auth-required:`
/// and other `restricted:` reasons (an authenticated or re-admitted socket can change the
/// answer), and `duplicate:` (the relay already holds the event, which a retry
/// resolves harmlessly).
/// The exact membership refusal is also final: this host must observe membership
/// before it authors new status, rather than retry a departed channel forever.
const FINAL_REJECTION_PREFIXES: &[&str] = &["invalid:", "blocked:"];

/// Seconds since the Unix epoch, or 0 if the clock is before it.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether this event's `created_at` already sits outside the relay's window.
fn outside_relay_window(event: &nostr::Event, now: i64) -> bool {
    (event.created_at.as_secs() as i64 - now).abs() > RELAY_TIMESTAMP_WINDOW_SECS
}

/// The relay's reason, when an error is a final verdict rather than a
/// transport failure.
///
/// The sink surfaces errors as strings (see [`EventSink::publish`]), so the
/// classification is by NIP-01 reason prefix. The envelope is stripped when
/// present, and a sink that returns a bare reason is understood too.
fn final_rejection_reason(error: &str) -> Option<&str> {
    let reason = match error.find(RELAY_REJECTION_ENVELOPE) {
        Some(at) => &error[at + RELAY_REJECTION_ENVELOPE.len()..],
        None => error,
    };
    (reason == "restricted: not a channel member"
        || FINAL_REJECTION_PREFIXES
            .iter()
            .any(|prefix| reason.starts_with(prefix)))
    .then_some(reason)
}

/// Drain order for pending rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Priority {
    /// Live session receipts and current-turn transcript, ahead of recovery.
    Live,
    /// Lifecycle receipts and terminal turn items — a consumer waits on these.
    High,
    /// Paced background work, including catalogs and recovery status.
    Normal,
}

/// One durable publish intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEntry {
    /// Row id, unique per enqueue.
    pub id: String,
    /// Event kind, used with `semantic_key` as the fencing tuple.
    pub kind: u32,
    /// The event's semantic key (`csl-key`, `csm-key`, `cst-key`, `cspc-key`).
    pub semantic_key: String,
    /// Hex pubkey that signed `event`.
    pub signer: String,
    /// Drain order.
    pub priority: Priority,
    /// The fully signed Nostr event.
    pub event: nostr::Event,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum LedgerRow {
    Enqueue {
        entry: Box<OutboxEntry>,
    },
    Replace {
        entry: Box<OutboxEntry>,
        supersedes: Vec<String>,
    },
    Ack {
        id: String,
    },
    /// A relay-accepted latest value, retained even after ledger compaction.
    Accepted {
        entry: Box<OutboxEntry>,
    },
    /// This row will never be published. Finished, like [`LedgerRow::Ack`],
    /// but for a reason the consumer never saw — so the reason and the time
    /// are recorded with it rather than lost to a log line.
    Park {
        id: String,
        kind: u32,
        reason: String,
        /// Seconds since the Unix epoch, when the row was parked.
        at: i64,
    },
}

#[derive(Debug)]
struct Pending {
    entry: OutboxEntry,
    attempts: u32,
    next_attempt_at: Option<Instant>,
}

/// A sink that can accept a signed event for delivery.
///
/// Exists so the outbox's retry and fencing behavior is testable without a
/// relay, and so the provider is not coupled to one transport.
pub trait EventSink {
    /// Deliver one signed event, or report why it could not be delivered.
    fn publish(
        &self,
        event: nostr::Event,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;

    /// Preserve typed local-gate information when the transport provides it.
    fn publish_detailed(
        &self,
        event: nostr::Event,
    ) -> impl std::future::Future<Output = Result<(), AcknowledgedPublishError>> + Send {
        let result = self.publish(event);
        async move { result.await.map_err(AcknowledgedPublishError::from) }
    }
}

impl EventSink for beekeeper_acp::relay::RelayEventPublisher {
    async fn publish_detailed(&self, event: nostr::Event) -> Result<(), AcknowledgedPublishError> {
        self.publish_event_acknowledged_detailed(event).await
    }

    async fn publish(&self, event: nostr::Event) -> Result<(), String> {
        self.publish_event_acknowledged(event)
            .await
            .map_err(|error| error.to_string())
    }
}

/// Append-only, crash-safe publish queue.
#[derive(Debug)]
pub struct Outbox {
    path: PathBuf,
    signer: String,
    pending: Vec<Pending>,
    accepted: HashMap<(u32, String), OutboxEntry>,
    paused_until: Option<Instant>,
    background_interval: Duration,
    background_next_at: Option<Instant>,
    /// Rows this ledger has given up on: parked at load, parked during a
    /// drain, or refused at enqueue. Counted from the ledger's own `Park`
    /// rows at load and incremented as more are parked, so a restart does not
    /// silently reset the disclosure.
    parked: usize,
    /// One-shot injected failure for the next enqueue. Test-only.
    ///
    /// The dispositions that now enqueue before they write their ledger claim
    /// that a failing enqueue leaves the command *unanswered and unfenced*,
    /// which is only provable if the enqueue can be made to fail on demand.
    #[cfg(test)]
    fail_next_enqueue: bool,
}

impl Outbox {
    /// Load the ledger, dropping rows this identity can no longer publish.
    pub fn open(dir: &Path, signer: &str) -> io::Result<Self> {
        let path = dir.join(OUTBOX_FILE);
        let mut pending: Vec<Pending> = Vec::new();
        let mut acked: HashSet<String> = HashSet::new();
        let mut fenced_by_signer = 0usize;
        let mut fenced_by_key = 0usize;
        let mut superseded_ids = Vec::new();
        let mut parked = 0usize;
        let mut accepted = HashMap::new();

        let body = match std::fs::read_to_string(&path) {
            Ok(body) => body,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let mut rows: Vec<OutboxEntry> = Vec::new();
        for line in body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<LedgerRow>(line) {
                Ok(LedgerRow::Enqueue { entry }) => rows.push(*entry),
                Ok(LedgerRow::Replace { entry, supersedes }) => {
                    acked.extend(supersedes);
                    rows.push(*entry);
                }
                Ok(LedgerRow::Accepted { entry }) => {
                    acked.insert(entry.id.clone());
                    if entry.signer == signer {
                        accepted.insert((entry.kind, entry.semantic_key.clone()), *entry);
                    }
                }
                Ok(LedgerRow::Ack { id }) => {
                    acked.insert(id);
                }
                Ok(LedgerRow::Park { id, .. }) => {
                    // A parked row is finished exactly as an acked row is; it
                    // is counted separately only so status can disclose it.
                    acked.insert(id);
                    parked = parked.saturating_add(1);
                }
                Err(error) => {
                    tracing::warn!(target: "csp::outbox", "dropping unreadable outbox line: {error}");
                }
            }
        }

        let mut seen_keys: HashSet<(u32, String)> = HashSet::new();
        // Newest wins for a semantic key. This is both the recovery rule for
        // an atomic `Replace` row and the fail-safe rule for a process that
        // died after durably appending a newer value but before retiring a
        // legacy row written by an older binary.
        rows.reverse();
        for mut entry in rows {
            if entry.kind == 44222 {
                entry.priority = Priority::Normal;
                if let Some(channel) = entry.event.tags.iter().find_map(|tag| {
                    let parts = tag.as_slice();
                    (parts.first().map(String::as_str) == Some("h"))
                        .then(|| parts.get(1))
                        .flatten()
                        .and_then(|value| uuid::Uuid::parse_str(value).ok())
                }) {
                    entry.semantic_key = format!("catalog:{channel}");
                }
            }
            if entry.kind == 44223
                && serde_json::from_str::<serde_json::Value>(&entry.event.content)
                    .ok()
                    .is_some_and(|content| {
                        content.get("status").and_then(serde_json::Value::as_str)
                            == Some("disconnected")
                    })
            {
                entry.priority = Priority::Normal;
            }
            if entry.kind == 44225 && entry.priority == Priority::Live {
                // A previous process's transcript is replay, not the current turn.
                entry.priority = Priority::High;
            }
            if acked.contains(&entry.id) {
                continue;
            }
            if entry.signer != signer {
                fenced_by_signer += 1;
                continue;
            }
            if !seen_keys.insert((entry.kind, entry.semantic_key.clone())) {
                fenced_by_key += 1;
                superseded_ids.push(entry.id);
                continue;
            }
            pending.push(Pending {
                entry,
                attempts: 0,
                next_attempt_at: None,
            });
        }
        pending.reverse();
        if fenced_by_signer > 0 {
            tracing::warn!(
                target: "csp::outbox",
                "dropped {fenced_by_signer} outbox row(s) signed by a rotated key"
            );
        }
        if fenced_by_key > 0 {
            tracing::debug!(
                target: "csp::outbox",
                "collapsed {fenced_by_key} duplicate outbox row(s)"
            );
        }

        let mut outbox = Self {
            path,
            signer: signer.to_owned(),
            pending,
            accepted,
            paused_until: None,
            background_interval: Duration::ZERO,
            background_next_at: None,
            parked,
            #[cfg(test)]
            fail_next_enqueue: false,
        };
        // Replay coalescing must be durable: otherwise accepting the newest
        // row lets an older unsent version resurrect on the next restart.
        for id in superseded_ids {
            outbox.append(&LedgerRow::Ack { id })?;
        }
        // Same treatment as a rotated key: a row the relay can no longer accept
        // is not a backlog, it is a poison that starves everything behind it.
        // Clearing it on open means a restart is a real remedy.
        let stale = outbox.park_rows_outside_window(unix_now())?;
        if stale > 0 {
            tracing::warn!(
                target: "csp::outbox",
                "parked {stale} outbox row(s) whose created_at is already outside the relay's \u{00b1}{RELAY_TIMESTAMP_WINDOW_SECS}s window"
            );
        }
        outbox.compact_if_idle()?;
        Ok(outbox)
    }

    /// Park every pending row that has aged out of the relay's window.
    ///
    /// Returns how many were parked.
    fn park_rows_outside_window(&mut self, now: i64) -> io::Result<usize> {
        let doomed: Vec<(String, u32, i64)> = self
            .pending
            .iter()
            .filter(|row| outside_relay_window(&row.entry.event, now))
            .map(|row| {
                (
                    row.entry.id.clone(),
                    row.entry.kind,
                    row.entry.event.created_at.as_secs() as i64,
                )
            })
            .collect();
        let count = doomed.len();
        for (id, kind, created_at) in doomed {
            let age = now.saturating_sub(created_at);
            self.park(&id, kind, &stale_reason(age), now)?;
        }
        Ok(count)
    }

    /// Make the next enqueue fail once. Test-only.
    #[cfg(test)]
    pub(crate) fn fail_next_enqueue(&mut self) {
        self.fail_next_enqueue = true;
    }

    /// Pace background traffic while leaving live work immediately eligible.
    pub fn set_background_interval(&mut self, interval: Duration) {
        self.background_interval = interval;
    }

    /// Number of rows still awaiting delivery.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Number of facts this ledger has given up on.
    ///
    /// Sits beside [`Outbox::pending_len`] so status can say *why* a queue is
    /// short: a parked row is one the consumer will never see, and a provider
    /// that quietly dropped it would be lying about its own history. Counts
    /// the ledger's `Park` rows at load plus everything parked since; a
    /// compaction of a fully drained ledger is the only thing that resets it,
    /// and only for the next process.
    pub fn parked_len(&self) -> usize {
        self.parked
    }

    /// The signed events still awaiting delivery, oldest first.
    ///
    /// For tests only: a caller that wants to *send* one takes it through the
    /// ordinary drain, which is the only path that records the attempt.
    #[cfg(test)]
    pub(crate) fn pending_events(&self) -> impl Iterator<Item = &nostr::Event> {
        self.pending.iter().map(|row| &row.entry.event)
    }

    /// Fact identities still awaiting a positive relay OK. Latest-value
    /// replacement preserves this identity even though its row id changes.
    pub(crate) fn pending_keys(&self) -> HashSet<(u32, String)> {
        self.pending
            .iter()
            .map(|row| (row.entry.kind, row.entry.semantic_key.clone()))
            .collect()
    }

    /// Content of the latest relay-accepted status for this exact identity.
    pub fn accepted_content(&self, kind: u32, semantic_key: &str) -> Option<&str> {
        self.accepted
            .get(&(kind, semantic_key.to_owned()))
            .map(|entry| entry.event.content.as_str())
    }

    /// Content of the current queued value, which may supersede an accepted one.
    pub fn pending_content(&self, kind: u32, semantic_key: &str) -> Option<&str> {
        self.pending
            .iter()
            .rev()
            .find(|row| row.entry.kind == kind && row.entry.semantic_key == semantic_key)
            .map(|row| row.entry.event.content.as_str())
    }

    /// Whether a row for this `(kind, semantic key)` is already queued.
    pub fn contains(&self, kind: u32, semantic_key: &str) -> bool {
        self.pending
            .iter()
            .any(|row| row.entry.kind == kind && row.entry.semantic_key == semantic_key)
    }

    /// Durably queue an event for publication.
    ///
    /// Returns `false` when the row was fenced — an identical `(kind, semantic
    /// key)` is already queued, or the event was signed by another identity.
    pub fn enqueue(
        &mut self,
        kind: u32,
        semantic_key: &str,
        priority: Priority,
        event: nostr::Event,
    ) -> io::Result<bool> {
        self.enqueue_inner(kind, semantic_key, priority, event, false)
    }

    /// Queue an event that *supersedes* any unsent row with the same key.
    ///
    /// Only correct for facts whose latest value is the whole truth — session
    /// metadata, where the consumer keeps the newest event per target. Sending
    /// a stale `idle` and dropping the `running` that overtook it would leave a
    /// permanently wrong status; worse, publishing both inside the same second
    /// reads to the consumer as two conflicting claims rather than a sequence.
    /// Immutable facts (receipts, transcript items) must never use this.
    pub fn enqueue_latest(
        &mut self,
        kind: u32,
        semantic_key: &str,
        priority: Priority,
        event: nostr::Event,
    ) -> io::Result<bool> {
        self.enqueue_inner(kind, semantic_key, priority, event, true)
    }

    fn enqueue_inner(
        &mut self,
        kind: u32,
        semantic_key: &str,
        priority: Priority,
        event: nostr::Event,
        supersede: bool,
    ) -> io::Result<bool> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_enqueue) {
            return Err(io::Error::other("injected outbox enqueue failure"));
        }
        let now = unix_now();
        if outside_relay_window(&event, now) {
            // Queueing this would only add another row that can never drain.
            let age = now.saturating_sub(event.created_at.as_secs() as i64);
            tracing::warn!(
                target: "csp::outbox",
                kind,
                semantic_key,
                "refusing to queue an event the relay cannot accept: {}",
                stale_reason(age)
            );
            self.parked = self.parked.saturating_add(1);
            return Ok(false);
        }
        let signer = event.pubkey.to_hex();
        if signer != self.signer {
            tracing::warn!(
                target: "csp::outbox",
                "refusing to queue an event signed by {signer}, not this provider"
            );
            return Ok(false);
        }
        let superseded: Vec<String> = self
            .pending
            .iter()
            .filter(|row| row.entry.kind == kind && row.entry.semantic_key == semantic_key)
            .map(|row| row.entry.id.clone())
            .collect();
        if !superseded.is_empty() && !supersede {
            return Ok(false);
        }
        let entry = OutboxEntry {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            semantic_key: semantic_key.to_owned(),
            signer,
            priority,
            event,
        };
        if superseded.is_empty() {
            self.append(&LedgerRow::Enqueue {
                entry: Box::new(entry.clone()),
            })?;
        } else {
            // One fsynced row establishes both sides of the replacement. A
            // crash can expose the old value or the new value, never neither.
            self.append(&LedgerRow::Replace {
                entry: Box::new(entry.clone()),
                supersedes: superseded.clone(),
            })?;
            self.pending
                .retain(|row| !superseded.iter().any(|id| id == &row.entry.id));
        }
        self.pending.push(Pending {
            entry,
            attempts: 0,
            next_attempt_at: None,
        });
        Ok(true)
    }

    /// Attempt one delivery pass, oldest-first within each priority band.
    ///
    /// Rows whose backoff has not elapsed are skipped. Returns the number of
    /// rows successfully delivered.
    pub async fn flush<S: EventSink>(&mut self, sink: &S) -> io::Result<usize> {
        self.flush_limit(sink, usize::MAX).await
    }

    /// Attempt at most one eligible delivery.
    ///
    /// The provider runtime uses this bounded pass so one slow relay ACK cannot
    /// multiply across the durable backlog and starve lease renewal work.
    pub async fn flush_one<S: EventSink>(&mut self, sink: &S) -> io::Result<usize> {
        self.flush_limit(sink, 1).await
    }

    async fn flush_limit<S: EventSink>(
        &mut self,
        sink: &S,
        max_attempts: usize,
    ) -> io::Result<usize> {
        let now = Instant::now();
        if self.paused_until.is_some_and(|until| until > now) {
            return Ok(0);
        }
        self.paused_until = None;
        let mut order: Vec<usize> = (0..self.pending.len())
            .filter(|index| {
                self.pending[*index]
                    .next_attempt_at
                    .is_none_or(|at| at <= now)
            })
            .collect();
        order.sort_by_key(|index| (self.pending[*index].entry.priority, *index));

        let ordered_ids: Vec<String> = order
            .into_iter()
            .map(|index| self.pending[index].entry.id.clone())
            .collect();
        let mut delivered = 0usize;
        let mut parked_here = 0usize;
        // The budget counts *sends*. Parking costs no relay round trip, so
        // spending the bounded pass on a row that is never going to be sent
        // would reintroduce the starvation this pass exists to avoid.
        let mut sends = 0usize;
        for id in ordered_ids {
            if sends >= max_attempts {
                break;
            }
            let Some(index) = self.pending.iter().position(|row| row.entry.id == id) else {
                continue;
            };
            let entry = self.pending[index].entry.clone();
            if entry.priority == Priority::Normal
                && self
                    .background_next_at
                    .is_some_and(|at| at > Instant::now())
            {
                continue;
            }
            let now = unix_now();
            if outside_relay_window(&entry.event, now) {
                // Do not spend a send on it: the answer is already known.
                let age = now.saturating_sub(entry.event.created_at.as_secs() as i64);
                let reason = stale_reason(age);
                tracing::warn!(
                    target: "csp::outbox",
                    kind = entry.kind,
                    event_id = %entry.event.id.to_hex(),
                    "parking an outbox row the relay cannot accept: {reason}"
                );
                self.park(&entry.id, entry.kind, &reason, now)?;
                parked_here = parked_here.saturating_add(1);
                continue;
            }
            sends = sends.saturating_add(1);
            tracing::debug!(target: "csp::outbox", kind = entry.kind, event_id = %entry.event.id, priority = ?entry.priority, "attempting queued publication");
            let outcome = sink.publish_detailed(entry.event.clone()).await;
            if entry.priority == Priority::Normal
                && !matches!(
                    &outcome,
                    Err(AcknowledgedPublishError::RateLimited { sent: false, .. })
                )
            {
                self.background_next_at = Some(Instant::now() + self.background_interval);
            }
            match outcome {
                Ok(()) => {
                    if entry.kind == 44223 {
                        self.append(&LedgerRow::Accepted {
                            entry: Box::new(entry.clone()),
                        })?;
                        self.accepted
                            .insert((entry.kind, entry.semantic_key.clone()), entry.clone());
                        self.pending.retain(|row| row.entry.id != entry.id);
                    } else {
                        self.ack(&entry.id)?;
                    }
                    delivered = delivered.saturating_add(1);
                }
                Err(AcknowledgedPublishError::RateLimited { deadline, sent }) => {
                    self.paused_until = Some(deadline);
                    let row = &mut self.pending[index];
                    if sent {
                        row.attempts = row.attempts.saturating_add(1);
                    }
                    // The gate, not an exponential row backoff, determines eligibility.
                    row.next_attempt_at = None;
                    tracing::warn!(target: "csp::outbox", kind = entry.kind, attempts = row.attempts,
                        sent, retry_secs = deadline.saturating_duration_since(Instant::now()).as_secs_f64(),
                        "publication paused by relay rate gate");
                    break;
                }
                Err(error) => {
                    let error = error.to_string();
                    if let Some(reason) = final_rejection_reason(&error) {
                        // A verdict on the bytes, not on the socket. Retrying
                        // the identical event can only earn the identical
                        // answer, and one such row at the head of the drain
                        // order would otherwise stop every pass before the
                        // fresh rows behind it (ledger 170).
                        let reason = reason.to_owned();
                        tracing::warn!(
                            target: "csp::outbox",
                            kind = entry.kind,
                            event_id = %entry.event.id.to_hex(),
                            "relay refused this event as final; parking it: {reason}"
                        );
                        self.park(&entry.id, entry.kind, &reason, now)?;
                        parked_here = parked_here.saturating_add(1);
                        continue;
                    }
                    let row = &mut self.pending[index];
                    row.attempts = row.attempts.saturating_add(1);
                    row.next_attempt_at = Some(Instant::now() + backoff(row.attempts));
                    tracing::warn!(
                        target: "csp::outbox",
                        kind = entry.kind,
                        attempts = row.attempts,
                        "publish failed, will retry: {error}"
                    );
                    // A transport failure almost always means the socket is
                    // gone; hammering the rest of the queue would just burn
                    // attempts.
                    break;
                }
            }
        }

        if delivered > 0 || parked_here > 0 {
            self.compact_if_idle()?;
        }
        Ok(delivered)
    }

    /// How long until the next row becomes eligible, if anything is waiting.
    pub fn next_retry_delay(&self) -> Option<Duration> {
        let now = Instant::now();
        self.pending
            .iter()
            .map(|row| {
                let retry = row
                    .next_attempt_at
                    .map(|at| at.saturating_duration_since(now))
                    .unwrap_or_default();
                if row.entry.priority == Priority::Normal {
                    retry.max(
                        self.background_next_at
                            .map(|at| at.saturating_duration_since(now))
                            .unwrap_or_default(),
                    )
                } else {
                    retry
                }
            })
            .min()
            .map(|delay| {
                delay.max(
                    self.paused_until
                        .map(|at| at.saturating_duration_since(now))
                        .unwrap_or_default(),
                )
            })
    }

    /// Durably drop every queued row `discard` selects, without publishing it.
    ///
    /// Retirement's other half. Suppressing *future* metadata for a deleted
    /// session is not enough: a 44223 queued while the relay was unreachable
    /// is already signed and already durable, and it cannot have been named by
    /// a deletion that happened after it was written — so on the next
    /// successful flush it lands and recreates exactly the ghost session row
    /// retirement exists to remove (root's P1).
    ///
    /// A dropped row is acked rather than deleted, which is what makes this
    /// crash-safe: the ledger already means "this row is finished", the replay
    /// on open already honours it, and no new row type has to be understood by
    /// a reader written before this existed.
    ///
    /// Returns how many rows were dropped. The caller chooses the predicate,
    /// and is responsible for keeping the answers a command is still owed —
    /// see [`crate::Provider::purge_outbox_for_retired`].
    pub fn discard(&mut self, discard: impl Fn(&OutboxEntry) -> bool) -> io::Result<usize> {
        let doomed: Vec<String> = self
            .pending
            .iter()
            .filter(|row| discard(&row.entry))
            .map(|row| row.entry.id.clone())
            .collect();
        for id in &doomed {
            self.ack(id)?;
        }
        Ok(doomed.len())
    }

    /// Durably finish a row that can never be published, recording why.
    ///
    /// Parking is deliberately the *only* disposal for an unpublishable row:
    /// this outbox holds signed events, not the key that signed them, so
    /// re-stamping a rejected event with a fresh `created_at` is not something
    /// it could do even if it should. And it should not. Every kind that
    /// reaches here — lifecycle receipts, transcript items, turn claims — names
    /// a moment that has already happened; re-signing one with today's clock
    /// would publish a true-looking event with a false time, which is a worse
    /// outcome than a disclosed gap. If a kind is ever found whose whole truth
    /// is its latest value and whose timestamp carries no claim, re-signing it
    /// belongs at its own call site, where the signer lives.
    fn park(&mut self, id: &str, kind: u32, reason: &str, at: i64) -> io::Result<()> {
        self.append(&LedgerRow::Park {
            id: id.to_owned(),
            kind,
            reason: reason.to_owned(),
            at,
        })?;
        self.pending.retain(|row| row.entry.id != id);
        self.parked = self.parked.saturating_add(1);
        Ok(())
    }

    fn ack(&mut self, id: &str) -> io::Result<()> {
        self.append(&LedgerRow::Ack { id: id.to_owned() })?;
        self.pending.retain(|row| row.entry.id != id);
        Ok(())
    }

    fn append(&self, row: &LedgerRow) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        // One `write` call for the whole line (payload + newline) on an
        // O_APPEND handle: an interleaving writer can order lines, but it can
        // never tear one in half.
        let mut line = serde_json::to_string(row)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        file.sync_all()
    }

    /// Truncate a fully drained ledger once it has grown large.
    fn compact_if_idle(&self) -> io::Result<()> {
        if !self.pending.is_empty() {
            return Ok(());
        }
        match std::fs::metadata(&self.path) {
            Ok(meta) if meta.len() > COMPACT_THRESHOLD_BYTES => {
                let mut body = String::new();
                for entry in self.accepted.values() {
                    body.push_str(&serde_json::to_string(&LedgerRow::Accepted {
                        entry: Box::new(entry.clone()),
                    })?);
                    body.push('\n');
                }
                atomic_write(&self.path, body.as_bytes())
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Why a row aged out, in the words the ledger and the log both use.
fn stale_reason(age_secs: i64) -> String {
    format!(
        "created_at is {age_secs}s old, outside the relay's \u{00b1}{RELAY_TIMESTAMP_WINDOW_SECS}s window"
    )
}

fn backoff(attempts: u32) -> Duration {
    let shift = attempts.saturating_sub(1).min(6);
    RETRY_MAX.min(RETRY_BASE * 2u32.saturating_pow(shift))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use nostr::{EventBuilder, Keys, Kind};

    use super::*;

    #[derive(Default)]
    struct RecordingSink {
        fail: bool,
        published: Mutex<Vec<nostr::Event>>,
        /// Contents the sink refuses, and the relay reply it refuses them
        /// with. Everything else is accepted.
        refuse: Mutex<Vec<(String, String)>>,
        /// Every event handed to the sink, accepted or not.
        attempted: Mutex<Vec<nostr::Event>>,
    }

    impl EventSink for RecordingSink {
        async fn publish(&self, event: nostr::Event) -> Result<(), String> {
            self.attempted.lock().expect("lock").push(event.clone());
            if self.fail {
                return Err("relay is down".into());
            }
            let refusal = self
                .refuse
                .lock()
                .expect("lock")
                .iter()
                .find(|(content, _)| content == &event.content)
                .map(|(_, reply)| reply.clone());
            if let Some(reply) = refusal {
                return Err(reply);
            }
            self.published.lock().expect("lock").push(event);
            Ok(())
        }
    }

    impl RecordingSink {
        /// Refuse this content the way the live relay refuses a durable event.
        fn refusing(content: &str, reason: &str) -> Self {
            let sink = Self::default();
            sink.refuse.lock().expect("lock").push((
                content.to_owned(),
                format!("Unexpected message: {RELAY_REJECTION_ENVELOPE}{reason}"),
            ));
            sink
        }

        fn attempted_contents(&self) -> Vec<String> {
            self.attempted
                .lock()
                .expect("lock")
                .iter()
                .map(|event| event.content.clone())
                .collect()
        }

        fn published_contents(&self) -> Vec<String> {
            self.published
                .lock()
                .expect("lock")
                .iter()
                .map(|event| event.content.clone())
                .collect()
        }
    }

    fn signed(keys: &Keys, content: &str) -> nostr::Event {
        EventBuilder::new(Kind::Custom(44225), content)
            .sign_with_keys(keys)
            .expect("sign")
    }

    /// An event whose `created_at` is `age_secs` in the past. Signed honestly:
    /// this is what a row queued during a long relay outage looks like.
    fn signed_aged(keys: &Keys, content: &str, age_secs: u64) -> nostr::Event {
        EventBuilder::new(Kind::Custom(44225), content)
            .custom_created_at(nostr::Timestamp::from(
                (unix_now() as u64).saturating_sub(age_secs),
            ))
            .sign_with_keys(keys)
            .expect("sign")
    }

    /// Put a row in the ledger without the enqueue-time window guard, which is
    /// how the real poisoned ledgers were written: the rows were fresh when
    /// they were queued and aged out while the relay was unreachable.
    fn append_aged_row(outbox: &Outbox, keys: &Keys, key: &str, content: &str, age_secs: u64) {
        outbox
            .append(&LedgerRow::Enqueue {
                entry: Box::new(OutboxEntry {
                    id: format!("row-{key}"),
                    kind: 44223,
                    semantic_key: key.to_owned(),
                    signer: keys.public_key().to_hex(),
                    priority: Priority::High,
                    event: signed_aged(keys, content, age_secs),
                }),
            })
            .expect("durable row");
    }

    #[tokio::test]
    async fn delivers_pending_rows_and_forgets_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        assert!(outbox
            .enqueue(44225, "key-1", Priority::Normal, signed(&keys, "a"))
            .expect("enqueue"));
        assert_eq!(outbox.pending_len(), 1);

        let sink = RecordingSink::default();
        assert_eq!(outbox.flush(&sink).await.expect("flush"), 1);
        assert_eq!(outbox.pending_len(), 0);
        assert_eq!(sink.published.lock().expect("lock").len(), 1);

        // The ack is durable: a restart must not republish.
        let reopened = Outbox::open(dir.path(), &signer).expect("reopen");
        assert_eq!(reopened.pending_len(), 0);
    }

    #[tokio::test]
    async fn a_failed_publish_survives_restart_and_backs_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        outbox
            .enqueue(44225, "key-1", Priority::Normal, signed(&keys, "a"))
            .expect("enqueue");

        let down = RecordingSink {
            fail: true,
            ..RecordingSink::default()
        };
        assert_eq!(outbox.flush(&down).await.expect("flush"), 0);
        assert_eq!(outbox.pending_len(), 1);
        assert!(outbox.next_retry_delay().expect("delay") > Duration::ZERO);

        // A second immediate pass is skipped by the backoff rather than retried.
        assert_eq!(outbox.flush(&down).await.expect("flush"), 0);

        let mut reopened = Outbox::open(dir.path(), &signer).expect("reopen");
        assert_eq!(reopened.pending_len(), 1);
        let sink = RecordingSink::default();
        assert_eq!(reopened.flush(&sink).await.expect("flush"), 1);
    }

    #[tokio::test]
    async fn a_crash_between_latest_enqueue_and_retirement_replays_the_newest_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        outbox
            .enqueue_latest(44223, "session-1", Priority::High, signed(&keys, "idle"))
            .expect("initial metadata");

        // This is the only crash-safe ordering for a replacement: the new row
        // is durable before the old row is retired. Simulate a process dying in
        // that window and require replay to choose the later value.
        let replacement = OutboxEntry {
            id: "replacement-row".into(),
            kind: 44223,
            semantic_key: "session-1".into(),
            signer: signer.clone(),
            priority: Priority::High,
            event: signed(&keys, "running"),
        };
        outbox
            .append(&LedgerRow::Enqueue {
                entry: Box::new(replacement),
            })
            .expect("durable replacement");
        drop(outbox);

        let mut reopened = Outbox::open(dir.path(), &signer).expect("reopen");
        let sink = RecordingSink::default();
        assert_eq!(reopened.flush(&sink).await.expect("flush"), 1);
        assert_eq!(
            sink.published.lock().expect("lock")[0].content,
            "running",
            "replay must never resurrect the superseded metadata value"
        );
    }

    #[tokio::test]
    async fn a_failed_latest_replace_keeps_the_previous_durable_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        outbox
            .enqueue_latest(44223, "session-1", Priority::High, signed(&keys, "idle"))
            .expect("initial metadata");
        let ledger = outbox.path.clone();
        outbox.path = dir.path().to_path_buf();

        assert!(outbox
            .enqueue_latest(44223, "session-1", Priority::High, signed(&keys, "running"),)
            .is_err());
        assert_eq!(outbox.pending_len(), 1);
        drop(outbox);

        let mut reopened = Outbox::open(dir.path(), &signer).expect("reopen");
        assert_eq!(reopened.path, ledger);
        let sink = RecordingSink::default();
        assert_eq!(reopened.flush(&sink).await.expect("flush"), 1);
        assert_eq!(sink.published.lock().expect("lock")[0].content, "idle");
    }

    /// Enqueueing the same fact twice is the normal shape of a retry after a
    /// crash, and must not become two events for the consumer to reconcile.
    #[test]
    fn the_semantic_key_fences_duplicate_enqueues() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        assert!(outbox
            .enqueue(44225, "key-1", Priority::Normal, signed(&keys, "a"))
            .expect("enqueue"));
        assert!(!outbox
            .enqueue(44225, "key-1", Priority::Normal, signed(&keys, "b"))
            .expect("enqueue"));
        // A different kind with the same key string is a different fact.
        assert!(outbox
            .enqueue(44223, "key-1", Priority::Normal, signed(&keys, "c"))
            .expect("enqueue"));
        assert_eq!(outbox.pending_len(), 2);
    }

    /// After a key rotation the queued rows are unpublishable under the new
    /// identity — a consumer's trusted-signer set would reject them — so they
    /// are dropped rather than wedging the queue forever.
    #[test]
    fn rows_from_a_rotated_key_are_fenced_at_load_and_at_enqueue() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = Keys::generate();
        let new = Keys::generate();
        {
            let mut outbox = Outbox::open(dir.path(), &old.public_key().to_hex()).expect("open");
            outbox
                .enqueue(44225, "key-1", Priority::Normal, signed(&old, "a"))
                .expect("enqueue");
        }
        let mut rotated = Outbox::open(dir.path(), &new.public_key().to_hex()).expect("reopen");
        assert_eq!(rotated.pending_len(), 0);
        assert!(!rotated
            .enqueue(44225, "key-2", Priority::Normal, signed(&old, "b"))
            .expect("enqueue"));
    }

    #[tokio::test]
    async fn receipts_drain_before_transcript_chatter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        outbox
            .enqueue(44225, "chatter", Priority::Normal, signed(&keys, "chatter"))
            .expect("enqueue");
        outbox
            .enqueue(44224, "receipt", Priority::High, signed(&keys, "receipt"))
            .expect("enqueue");

        let sink = RecordingSink::default();
        assert_eq!(outbox.flush(&sink).await.expect("flush"), 2);
        let published = sink.published.lock().expect("lock");
        assert_eq!(published[0].content, "receipt");
        assert_eq!(published[1].content, "chatter");
    }

    #[tokio::test]
    async fn a_runtime_delivery_pass_attempts_only_one_eligible_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        for (key, content) in [("key-1", "a"), ("key-2", "b"), ("key-3", "c")] {
            outbox
                .enqueue(44225, key, Priority::Normal, signed(&keys, content))
                .expect("enqueue");
        }

        let sink = RecordingSink::default();
        assert_eq!(outbox.flush_one(&sink).await.expect("runtime pass"), 1);
        assert_eq!(outbox.pending_len(), 2);
        assert_eq!(sink.published.lock().expect("lock").len(), 1);
    }

    /// The runtime loop waits on this value instead of a fixed tick, so its
    /// three states are load-bearing: nothing queued must park the arm, a fresh
    /// row must be eligible *now* rather than at the next timer edge, and a
    /// failed row must hold the arm off for its backoff instead of spinning.
    #[tokio::test]
    async fn the_next_delay_distinguishes_empty_eligible_and_backing_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        assert_eq!(outbox.next_retry_delay(), None, "an empty outbox parks");

        outbox
            .enqueue(44225, "key-1", Priority::Normal, signed(&keys, "a"))
            .expect("enqueue");
        assert_eq!(
            outbox.next_retry_delay(),
            Some(Duration::ZERO),
            "a queued row is eligible immediately, not on the next tick"
        );

        let down = RecordingSink {
            fail: true,
            ..RecordingSink::default()
        };
        assert_eq!(outbox.flush_one(&down).await.expect("flush"), 0);
        assert!(outbox.next_retry_delay().expect("delay") > Duration::ZERO);

        let sink = RecordingSink::default();
        while outbox.pending_len() > 0 {
            tokio::time::sleep(outbox.next_retry_delay().expect("delay")).await;
            outbox.flush_one(&sink).await.expect("flush");
        }
        assert_eq!(outbox.next_retry_delay(), None);
    }

    /// The liveness bug (ledger 170): one permanently refused row at the head
    /// of the drain order stopped every pass, so fresh rows behind it never
    /// got a turn. A final refusal must finish that row and let the pass go on.
    #[tokio::test]
    async fn a_final_rejection_is_parked_and_the_next_row_still_lands() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        // The doomed row is queued first *and* at high priority, so it is
        // first in the drain order on every pass.
        outbox
            .enqueue(44223, "doomed", Priority::High, signed(&keys, "doomed"))
            .expect("enqueue");
        outbox
            .enqueue(46022, "fresh", Priority::High, signed(&keys, "fresh"))
            .expect("enqueue");

        let sink = RecordingSink::refusing(
            "doomed",
            "invalid: event timestamp too far from server time",
        );
        assert_eq!(outbox.flush(&sink).await.expect("flush"), 1);
        assert_eq!(
            sink.published_contents(),
            vec!["fresh".to_string()],
            "the row behind the refused one must be delivered in the same pass"
        );
        assert_eq!(outbox.pending_len(), 0);
        assert_eq!(outbox.parked_len(), 1);

        // Parking is durable: a restart must not resurrect the refused row.
        let reopened = Outbox::open(dir.path(), &signer).expect("reopen");
        assert_eq!(reopened.pending_len(), 0);
        assert_eq!(reopened.parked_len(), 1);
    }

    /// The other half of the same distinction: a transport error says nothing
    /// about the event, so the row is kept and the pass stops.
    #[tokio::test]
    async fn a_transport_error_still_backs_off_and_stops_the_pass() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        outbox
            .enqueue(44223, "first", Priority::High, signed(&keys, "first"))
            .expect("enqueue");
        outbox
            .enqueue(44225, "second", Priority::Normal, signed(&keys, "second"))
            .expect("enqueue");

        let down = RecordingSink {
            fail: true,
            ..RecordingSink::default()
        };
        assert_eq!(outbox.flush(&down).await.expect("flush"), 0);
        assert_eq!(outbox.pending_len(), 2, "nothing may be parked");
        assert_eq!(outbox.parked_len(), 0);
        assert_eq!(
            down.attempted_contents(),
            vec!["first".to_string()],
            "the pass must break rather than burn attempts on the whole queue"
        );
        let failed = outbox
            .pending
            .iter()
            .find(|row| row.entry.event.content == "first")
            .expect("the refused row is retained");
        assert_eq!(failed.attempts, 1);
        assert!(
            failed
                .next_attempt_at
                .expect("backoff")
                .saturating_duration_since(Instant::now())
                > Duration::ZERO,
            "a transport failure must back the row off rather than finish it"
        );
    }

    /// `rate-limited:` and `error:` are the relay asking for later, not never.
    #[tokio::test]
    async fn a_retryable_relay_reply_is_not_parked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        outbox
            .enqueue(44225, "key-1", Priority::Normal, signed(&keys, "a"))
            .expect("enqueue");

        let gated = RecordingSink::refusing("a", "rate-limited: slow down");
        assert_eq!(outbox.flush(&gated).await.expect("flush"), 0);
        assert_eq!(outbox.pending_len(), 1);
        assert_eq!(outbox.parked_len(), 0);
    }

    /// A row that has aged past the relay's window is refused before the send,
    /// not after: the answer is already known from `created_at` alone.
    #[tokio::test]
    async fn a_row_outside_the_relay_window_is_parked_without_a_send() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let ledger = {
            let outbox = Outbox::open(dir.path(), &signer).expect("open");
            append_aged_row(&outbox, &keys, "stale", "stale", 21_000 * 60);
            outbox.path.clone()
        };
        assert!(ledger.exists());

        // Load it past the open-time sweep so the drain-time guard is the one
        // under test.
        let mut outbox = Outbox::open(dir.path(), &signer).expect("reopen");
        assert_eq!(outbox.pending_len(), 0, "the open sweep parks it");
        assert_eq!(outbox.parked_len(), 1);

        // Now prove the drain-time guard directly, with the row put back into
        // memory as a running provider would have held it.
        outbox.pending.push(Pending {
            entry: OutboxEntry {
                id: "in-memory".into(),
                kind: 44223,
                semantic_key: "stale".into(),
                signer: signer.clone(),
                priority: Priority::High,
                event: signed_aged(&keys, "stale", RELAY_TIMESTAMP_WINDOW_SECS as u64 + 60),
            },
            attempts: 0,
            next_attempt_at: None,
        });
        let sink = RecordingSink::default();
        assert_eq!(outbox.flush(&sink).await.expect("flush"), 0);
        assert!(
            sink.attempted_contents().is_empty(),
            "an unacceptable row must not cost a relay round trip"
        );
        assert_eq!(outbox.pending_len(), 0);
        assert_eq!(outbox.parked_len(), 2);
    }

    /// A restart is the remedy for a poisoned ledger: the stale rows go, the
    /// rows the relay would still accept stay.
    #[tokio::test]
    async fn open_parks_stale_rows_and_keeps_fresh_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        {
            let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
            append_aged_row(&outbox, &keys, "old-a", "old-a", 15_000 * 60);
            append_aged_row(&outbox, &keys, "old-b", "old-b", 21_000 * 60);
            outbox
                .enqueue(46022, "fresh", Priority::High, signed(&keys, "fresh"))
                .expect("enqueue");
        }

        let mut reopened = Outbox::open(dir.path(), &signer).expect("reopen");
        assert_eq!(reopened.pending_len(), 1);
        assert_eq!(reopened.parked_len(), 2);
        let sink = RecordingSink::default();
        assert_eq!(reopened.flush(&sink).await.expect("flush"), 1);
        assert_eq!(sink.published_contents(), vec!["fresh".to_string()]);
    }

    /// An event that is already unacceptable never becomes a ledger row.
    #[test]
    fn enqueue_refuses_an_event_the_relay_cannot_accept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = Keys::generate();
        let signer = keys.public_key().to_hex();
        let mut outbox = Outbox::open(dir.path(), &signer).expect("open");
        assert!(!outbox
            .enqueue(
                44223,
                "stale",
                Priority::High,
                signed_aged(&keys, "stale", RELAY_TIMESTAMP_WINDOW_SECS as u64 + 1),
            )
            .expect("enqueue"));
        assert_eq!(outbox.pending_len(), 0);
        assert_eq!(outbox.parked_len(), 1);
        // A row at the edge of the window is still worth sending.
        assert!(outbox
            .enqueue(
                44223,
                "recent",
                Priority::High,
                signed_aged(&keys, "recent", 60),
            )
            .expect("enqueue"));
        assert_eq!(outbox.pending_len(), 1);
    }

    /// The reason and the time a row was parked are ledger facts, not log
    /// lines, so a reader written today can still explain the gap tomorrow.
    #[test]
    fn the_park_row_round_trips_through_the_ledger() {
        let row = LedgerRow::Park {
            id: "row-1".into(),
            kind: 44223,
            reason: "invalid: event timestamp too far from server time".into(),
            at: 1_758_000_000,
        };
        let line = serde_json::to_string(&row).expect("serialize");
        assert!(line.contains("\"op\":\"park\""), "{line}");
        match serde_json::from_str::<LedgerRow>(&line).expect("deserialize") {
            LedgerRow::Park {
                id,
                kind,
                reason,
                at,
            } => {
                assert_eq!(id, "row-1");
                assert_eq!(kind, 44223);
                assert_eq!(reason, "invalid: event timestamp too far from server time");
                assert_eq!(at, 1_758_000_000);
            }
            other => panic!("expected a park row, got {other:?}"),
        }
    }

    #[test]
    fn only_a_final_relay_verdict_is_recognised_as_final() {
        let envelope =
            |reason: &str| format!("Unexpected message: {RELAY_REJECTION_ENVELOPE}{reason}");
        assert_eq!(
            final_rejection_reason(&envelope(
                "invalid: event timestamp too far from server time"
            )),
            Some("invalid: event timestamp too far from server time")
        );
        assert_eq!(
            final_rejection_reason(&envelope("blocked: pubkey is not a member")),
            Some("blocked: pubkey is not a member")
        );
        for retryable in [
            "rate-limited: slow down",
            "error: internal verification error",
            "auth-required: we can't serve DMs to unauthenticated users",
            "restricted: not a member of this group",
            "duplicate: already have this event",
        ] {
            assert_eq!(
                final_rejection_reason(&envelope(retryable)),
                None,
                "{retryable} must not be treated as final"
            );
        }
        assert_eq!(final_rejection_reason("Connection closed"), None);
        assert_eq!(final_rejection_reason("Timeout"), None);
        // A sink that returns a bare NIP-01 reason is understood too.
        assert_eq!(
            final_rejection_reason("invalid: bad signature"),
            Some("invalid: bad signature")
        );
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff(1), RETRY_BASE);
        assert_eq!(backoff(2), RETRY_BASE * 2);
        assert_eq!(backoff(3), RETRY_BASE * 4);
        assert_eq!(backoff(50), RETRY_MAX);
    }
}

#[cfg(test)]
#[path = "publish/throttling_tests.rs"]
mod throttling_tests;
