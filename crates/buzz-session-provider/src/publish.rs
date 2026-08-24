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

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::state::atomic_write;

const OUTBOX_FILE: &str = "outbox.jsonl";
/// Rewrite the ledger once it exceeds this size and nothing is pending.
const COMPACT_THRESHOLD_BYTES: u64 = 256 * 1024;
/// First retry delay after a failed publish.
pub const RETRY_BASE: Duration = Duration::from_secs(1);
/// Ceiling on the exponential retry delay.
pub const RETRY_MAX: Duration = Duration::from_secs(60);

/// Drain order for pending rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Priority {
    /// Lifecycle receipts and terminal turn items — a consumer waits on these.
    High,
    /// Everything else.
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
}

impl EventSink for buzz_acp::relay::RelayEventPublisher {
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
}

impl Outbox {
    /// Load the ledger, dropping rows this identity can no longer publish.
    pub fn open(dir: &Path, signer: &str) -> io::Result<Self> {
        let path = dir.join(OUTBOX_FILE);
        let mut pending: Vec<Pending> = Vec::new();
        let mut acked: HashSet<String> = HashSet::new();
        let mut fenced_by_signer = 0usize;
        let mut fenced_by_key = 0usize;

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
                Ok(LedgerRow::Ack { id }) => {
                    acked.insert(id);
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
        for entry in rows {
            if acked.contains(&entry.id) {
                continue;
            }
            if entry.signer != signer {
                fenced_by_signer += 1;
                continue;
            }
            if !seen_keys.insert((entry.kind, entry.semantic_key.clone())) {
                fenced_by_key += 1;
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

        let outbox = Self {
            path,
            signer: signer.to_owned(),
            pending,
        };
        outbox.compact_if_idle()?;
        Ok(outbox)
    }

    /// Number of rows still awaiting delivery.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Fact identities still awaiting a positive relay OK. Latest-value
    /// replacement preserves this identity even though its row id changes.
    pub(crate) fn pending_keys(&self) -> HashSet<(u32, String)> {
        self.pending
            .iter()
            .map(|row| (row.entry.kind, row.entry.semantic_key.clone()))
            .collect()
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
            .take(max_attempts)
            .map(|index| self.pending[index].entry.id.clone())
            .collect();
        let mut delivered = 0usize;
        for id in ordered_ids {
            let Some(index) = self.pending.iter().position(|row| row.entry.id == id) else {
                continue;
            };
            let entry = self.pending[index].entry.clone();
            match sink.publish(entry.event.clone()).await {
                Ok(()) => {
                    self.ack(&entry.id)?;
                    delivered = delivered.saturating_add(1);
                }
                Err(error) => {
                    let row = &mut self.pending[index];
                    row.attempts = row.attempts.saturating_add(1);
                    row.next_attempt_at = Some(Instant::now() + backoff(row.attempts));
                    tracing::warn!(
                        target: "csp::outbox",
                        kind = entry.kind,
                        attempts = row.attempts,
                        "publish failed, will retry: {error}"
                    );
                    // A failed publish almost always means the socket is gone;
                    // hammering the rest of the queue would just burn attempts.
                    break;
                }
            }
        }

        if delivered > 0 {
            self.compact_if_idle()?;
        }
        Ok(delivered)
    }

    /// How long until the next row becomes eligible, if anything is waiting.
    pub fn next_retry_delay(&self) -> Option<Duration> {
        let now = Instant::now();
        self.pending
            .iter()
            .map(|row| match row.next_attempt_at {
                None => Duration::ZERO,
                Some(at) => at.saturating_duration_since(now),
            })
            .min()
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
            Ok(meta) if meta.len() > COMPACT_THRESHOLD_BYTES => atomic_write(&self.path, b""),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
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
    }

    impl EventSink for RecordingSink {
        async fn publish(&self, event: nostr::Event) -> Result<(), String> {
            if self.fail {
                return Err("relay is down".into());
            }
            self.published.lock().expect("lock").push(event);
            Ok(())
        }
    }

    fn signed(keys: &Keys, content: &str) -> nostr::Event {
        EventBuilder::new(Kind::Custom(44225), content)
            .sign_with_keys(keys)
            .expect("sign")
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

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff(1), RETRY_BASE);
        assert_eq!(backoff(2), RETRY_BASE * 2);
        assert_eq!(backoff(3), RETRY_BASE * 4);
        assert_eq!(backoff(50), RETRY_MAX);
    }
}
