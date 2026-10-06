//! Crash-safe storage for pending CI-managed continuation registrations.
//!
//! A `thread.turn.continue_on_ci` command is not a turn. It is a promise that
//! *one* turn will be started later, when the exact CI run it names reaches a
//! terminal result — and the whole value of the promise is that it survives the
//! registering agent's turn ending, the provider restarting, and the relay
//! replaying the registration a second time. So it is written here, atomically,
//! **before** the provider acknowledges it, and it is removed only once the
//! turn it promised is durably somebody's (the command ledger) or once its
//! refusal is durably the operator's (the refusal ledger).
//!
//! Between the durable claim and the adapter actually starting the turn the
//! record sits in [`RecordState::Claimed`] rather than vanishing, so a crash
//! in that window leaves evidence of a delivery that was lost and can never be
//! re-admitted, instead of leaving nothing at all.
//!
//! The file is `ci-continuations.json` beside `state.json`, written through
//! [`crate::state::atomic_write`], so a crash leaves either the whole previous
//! generation or the whole next one.
//!
//! # Why the caps are refusals rather than evictions
//!
//! Evicting a *pending* registration to make room for a new one would silently
//! drop a promise somebody is waiting on. So the caps
//! ([`MAX_REGISTRATIONS`], [`MAX_REGISTRATIONS_PER_CHANNEL`]) are checked at
//! admission and answered with a visible `CI_CONTINUATION_STORE_FULL` refusal.
//! Only *terminal* records — records whose disposition has already been
//! published — are evicted, oldest first, at [`MAX_RETAINED_TERMINALS`]. They
//! are kept at all so a relay redelivery of a registration this provider
//! already refused cannot resurrect it.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use beekeeper_core::ci_result::CiResultIdentity;
use beekeeper_core::coding_session_command::CodingSessionTarget;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::atomic_write;

/// Name of the store file inside the provider state directory.
pub const STORE_FILE: &str = "ci-continuations.json";
/// Schema version this build reads and writes.
pub const STORE_VERSION: u32 = 1;
/// Ceiling on pending (waiting or ready) registrations across every channel.
pub const MAX_REGISTRATIONS: usize = 256;
/// Ceiling on pending (waiting or ready) registrations for one channel.
pub const MAX_REGISTRATIONS_PER_CHANNEL: usize = 32;
/// Ceiling on retained terminal records, evicted oldest-first.
pub const MAX_RETAINED_TERMINALS: usize = 256;

/// What the provider observed of the relay while a registration was pending.
///
/// The distinction the expiry disposition turns on. The provider genuinely
/// cannot tell "this project is private to my key" from "CI never reported" —
/// both are an empty answer — so it publishes the fact it *did* witness rather
/// than a guess about which of the two happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayObservation {
    /// The relay answered a subscription covering this digest (EOSE) and the
    /// answer contained no result this provider's identity could read.
    AnsweredEmpty,
    /// No relay answer covering this digest was observed before expiry.
    Unanswered,
}

/// The verified result a registration is waiting on, once it has been seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyResult {
    /// Event id of the relay-signed kind:46008 that carried it.
    pub result_event_id: String,
    /// Lowercase hex pubkey that signed it — always the witnessed relay self.
    pub result_signer: String,
    /// The decoded result re-encoded canonically, so the delivered turn text
    /// does not depend on the byte layout the relay happened to store.
    pub result_canonical_json: String,
    /// Epoch seconds at which this provider verified it.
    pub observed_at: u64,
}

/// Where one registration is in its life.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RecordState {
    /// No verified result yet.
    Waiting,
    /// A verified result is held and the continuation turn has not been
    /// admitted yet.
    Ready(ReadyResult),
    /// Start permission was durably claimed — the operation and command
    /// ledgers both name this `commandId` — and the adapter has not yet
    /// reported the turn beginning.
    ///
    /// The window this state exists to make visible is short and unavoidable:
    /// between the durable claim and the actor's prompt there is a crash that
    /// loses the delivery and *cannot* re-admit it, because the ledgers are
    /// first-writer-wins. Removing the record at claim time (what this store
    /// used to do) made that loss indistinguishable from a delivered turn. A
    /// record left here instead is found at the next boot and answered with
    /// `LOST_AFTER_CLAIM` — see
    /// [`crate::ci_continuation::LOST_AFTER_CLAIM`].
    Claimed {
        /// Epoch seconds at which start permission was granted.
        at: u64,
    },
    /// The registration was answered with a durable refusal.
    Terminal {
        /// The receipt error code that was published.
        code: String,
        /// Epoch seconds at which the disposition was recorded.
        at: u64,
        /// What the provider had witnessed of the relay, for the expiry codes
        /// where that is the whole difference. `None` for dispositions that do
        /// not turn on it (authority, generation, closure, conflict).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observation: Option<RelayObservation>,
    },
}

impl RecordState {
    /// Whether this state still owes the operator a turn or a refusal.
    ///
    /// `Claimed` is deliberately **not** pending: its turn is already
    /// somebody's, so it must not be re-delivered, re-watched, expired or
    /// counted against the admission caps. What it still owes is a *drop*
    /// receipt if the turn never started, and that is reconciled at boot
    /// rather than on the delivery path.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Waiting | Self::Ready(_))
    }

    /// Whether start permission has been durably claimed for this record.
    pub fn is_claimed(&self) -> bool {
        matches!(self, Self::Claimed { .. })
    }
}

/// One durably registered CI continuation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiContinuationRecord {
    /// The registering 44220's `commandId`. The delivered turn runs under it.
    pub command_id: String,
    /// Event id of the signed registration, so a consumer can find it.
    pub registration_event_id: String,
    /// Lowercase hex SHA-256 of the registration event's content.
    ///
    /// The idempotence fence: the same `commandId` carrying the same bytes is
    /// the same registration; the same `commandId` carrying different bytes is
    /// a `COMMAND_ID_CONFLICT` and the first durable record wins.
    pub payload_digest: String,
    /// Channel the registration arrived on; its receipts go back there.
    pub channel_id: Uuid,
    /// Pubkey that signed the registration. Re-checked for steering authority
    /// at delivery, never trusted from this file alone.
    pub signer: String,
    /// The exact generation the delivered turn will address.
    pub target: CodingSessionTarget,
    /// The exact CI run identity awaited.
    pub identity: CiResultIdentity,
    /// `correlation_id(identity)` — the `#d` value results are tagged with.
    pub correlation_id: String,
    /// Text the registering agent asked to be delivered with the result.
    pub continuation: String,
    /// Epoch seconds after which the registration is refused rather than kept.
    pub expires_at: u64,
    /// Epoch seconds at which this record became durable.
    pub registered_at: u64,
    /// Whether the relay has answered a subscription covering this digest at
    /// least once since the registration. Persisted because the window it
    /// describes spans restarts.
    pub relay_answered: bool,
    /// Delivery attempts made for a `ready` record.
    pub attempts: u32,
    /// Epoch seconds before which the provider will not retry delivery.
    pub next_check_at: u64,
    /// The receipt code of the last thing that stopped a delivery, when a
    /// delivery was deferred rather than answered.
    ///
    /// Written only by the deferral path (see
    /// [`Self::note_attempt_with_obstacle`](CiContinuationStore::note_attempt_with_obstacle)),
    /// and read only at expiry. It exists so the terminal answer names the
    /// obstacle that actually held the turn up instead of the generic "the
    /// window closed": a registration whose target could never be reopened
    /// because its agent seat was never re-staged is a custody problem with a
    /// custody remedy, and `CI_CONTINUATION_EXPIRED` would send the operator
    /// looking at CI instead.
    ///
    /// Defaults to `None` for records written before this field existed, and
    /// for every record that has never been deferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_obstacle: Option<String>,
    /// Lifecycle position.
    pub state: RecordState,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    version: u32,
    #[serde(default)]
    registrations: Vec<CiContinuationRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotRef<'a> {
    version: u32,
    registrations: &'a [CiContinuationRecord],
}

/// Why a registration could not be admitted into the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmitRefusal {
    /// A record with this `commandId` already holds different bytes.
    CommandIdConflict,
    /// Every pending slot this store offers is taken.
    StoreFull {
        /// Human-facing detail naming which cap was reached.
        detail: String,
    },
}

/// What admitting a registration did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admitted {
    /// A new durable record was written.
    Stored,
    /// A byte-identical record was already durable; nothing changed.
    AlreadyStored,
}

/// Atomic, crash-safe store of pending CI continuation registrations.
#[derive(Debug)]
pub struct CiContinuationStore {
    path: PathBuf,
    registrations: Vec<CiContinuationRecord>,
    /// One-shot injected failure for the next durable write.
    ///
    /// Test-only. The ordering guarantees this store participates in are only
    /// meaningful if the failing half can actually be made to fail, and a real
    /// disk that refuses one write on demand is not something a unit test can
    /// arrange.
    #[cfg(test)]
    fail_next_write: bool,
}

impl CiContinuationStore {
    /// Open (or create) the store in `dir`.
    ///
    /// A file that exists but does not parse, or names a version this build
    /// does not read, is an error rather than an empty start: every record in
    /// it is a promise somebody is waiting on, and silently starting empty
    /// would drop them all while reporting success.
    pub fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(STORE_FILE);
        let registrations = match fs::read(&path) {
            Ok(body) => {
                let snapshot: Snapshot = serde_json::from_slice(&body).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{STORE_FILE} is unreadable: {error}"),
                    )
                })?;
                if snapshot.version != STORE_VERSION {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{STORE_FILE} version {} is not supported (expected {STORE_VERSION})",
                            snapshot.version
                        ),
                    ));
                }
                snapshot.registrations
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            registrations,
            #[cfg(test)]
            fail_next_write: false,
        })
    }

    /// Make the next durable write fail once.
    #[cfg(test)]
    pub(crate) fn fail_next_write(&mut self) {
        self.fail_next_write = true;
    }

    /// Every record, in insertion order.
    pub fn records(&self) -> impl Iterator<Item = &CiContinuationRecord> {
        self.registrations.iter()
    }

    /// Every record still owed a turn or a refusal.
    pub fn pending(&self) -> impl Iterator<Item = &CiContinuationRecord> {
        self.registrations
            .iter()
            .filter(|record| record.state.is_pending())
    }

    /// Every record whose start permission was claimed and whose turn has not
    /// been observed starting.
    ///
    /// Empty in steady state: `TurnStarted` removes each record as it arrives.
    /// A non-empty answer at boot is exactly the set of deliveries this
    /// provider lost to a crash inside the claim window.
    pub fn claimed(&self) -> impl Iterator<Item = &CiContinuationRecord> {
        self.registrations
            .iter()
            .filter(|record| record.state.is_claimed())
    }

    /// The record for `command_id`, whatever state it is in.
    pub fn record(&self, command_id: &str) -> Option<&CiContinuationRecord> {
        self.registrations
            .iter()
            .find(|record| record.command_id == command_id)
    }

    /// The exact identity every pending registration is waiting on, keyed by
    /// its correlation digest.
    ///
    /// A `BTreeMap` because it is what the listener subscribes with: two
    /// registrations for one identity must produce one `#d` value and one
    /// deterministic filter, so a set change is a real change rather than a
    /// reordering.
    ///
    /// A `ready` record is still watched. It has a result, but it has not
    /// started a turn yet, and a *second* canonical result arriving in that
    /// window has to be able to turn the delivery into a `CI_RESULT_CONFLICT`
    /// refusal rather than start a turn on a fact the relay contradicts.
    pub fn pending_identities(&self) -> BTreeMap<String, CiResultIdentity> {
        self.registrations
            .iter()
            .filter(|record| record.state.is_pending())
            .map(|record| (record.correlation_id.clone(), record.identity.clone()))
            .collect()
    }

    /// Command ids of every `ready` record eligible for a delivery attempt at
    /// `now`, oldest registration first.
    pub fn ready_for_delivery(&self, now: u64) -> Vec<String> {
        let mut ready: Vec<&CiContinuationRecord> = self
            .registrations
            .iter()
            .filter(|record| {
                matches!(record.state, RecordState::Ready(_)) && record.next_check_at <= now
            })
            .collect();
        ready.sort_by_key(|record| record.registered_at);
        ready
            .into_iter()
            .map(|record| record.command_id.clone())
            .collect()
    }

    /// Command ids of every pending record whose expiry has passed at `now`.
    pub fn expired(&self, now: u64) -> Vec<String> {
        self.registrations
            .iter()
            .filter(|record| record.state.is_pending() && record.expires_at <= now)
            .map(|record| record.command_id.clone())
            .collect()
    }

    /// Number of pending records, across every channel.
    pub fn pending_count(&self) -> usize {
        self.pending().count()
    }

    /// Number of pending records for one channel.
    pub fn pending_count_for_channel(&self, channel_id: Uuid) -> usize {
        self.pending()
            .filter(|record| record.channel_id == channel_id)
            .count()
    }

    /// Decide whether `record` may be admitted, without writing anything.
    ///
    /// Separate from [`Self::insert`] so the admission decision — which must
    /// run inside `decide_turn` alongside every other check, in the existing
    /// order — can be made from a `&self` view.
    pub fn admission(
        &self,
        command_id: &str,
        payload_digest: &str,
        channel_id: Uuid,
    ) -> Result<Option<Admitted>, AdmitRefusal> {
        if let Some(existing) = self.record(command_id) {
            if existing.payload_digest != payload_digest {
                return Err(AdmitRefusal::CommandIdConflict);
            }
            return Ok(Some(Admitted::AlreadyStored));
        }
        if self.pending_count() >= MAX_REGISTRATIONS {
            return Err(AdmitRefusal::StoreFull {
                detail: format!(
                    "this provider already holds {MAX_REGISTRATIONS} pending CI continuations"
                ),
            });
        }
        if self.pending_count_for_channel(channel_id) >= MAX_REGISTRATIONS_PER_CHANNEL {
            return Err(AdmitRefusal::StoreFull {
                detail: format!(
                    "this channel already holds {MAX_REGISTRATIONS_PER_CHANNEL} pending CI \
                     continuations"
                ),
            });
        }
        Ok(None)
    }

    /// Write one new registration durably.
    ///
    /// Idempotent for a byte-identical redelivery, and refuses rather than
    /// overwrites when the same `commandId` arrives carrying different bytes:
    /// the first durable record wins, so two processes reading this file can
    /// never disagree about what a command id promised.
    pub fn insert(&mut self, record: CiContinuationRecord) -> io::Result<Admitted> {
        if let Some(existing) = self.record(&record.command_id) {
            if existing.payload_digest == record.payload_digest {
                return Ok(Admitted::AlreadyStored);
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "a different CI continuation is already registered under command {}",
                    record.command_id
                ),
            ));
        }
        self.mutate(|registrations| registrations.push(record))?;
        Ok(Admitted::Stored)
    }

    /// Attach a verified result to every waiting record for `digest`.
    ///
    /// Returns the command ids that moved to `ready`. A record already `ready`
    /// or `terminal` is left alone: the first verified result wins, and a
    /// duplicate must not restart a delivery that is already under way.
    pub fn mark_ready(&mut self, digest: &str, result: &ReadyResult) -> io::Result<Vec<String>> {
        let moved: Vec<String> = self
            .registrations
            .iter()
            .filter(|record| {
                record.correlation_id == digest && matches!(record.state, RecordState::Waiting)
            })
            .map(|record| record.command_id.clone())
            .collect();
        if moved.is_empty() {
            return Ok(moved);
        }
        self.mutate(|registrations| {
            for record in registrations.iter_mut() {
                if record.correlation_id == digest && matches!(record.state, RecordState::Waiting) {
                    record.state = RecordState::Ready(result.clone());
                    record.next_check_at = 0;
                }
            }
        })?;
        Ok(moved)
    }

    /// Record that the relay answered a subscription covering these digests.
    ///
    /// The fact the expiry disposition reads. Idempotent, and only ever moves
    /// from "not answered" to "answered".
    pub fn mark_relay_answered(&mut self, digests: &[String]) -> io::Result<()> {
        let changed = self
            .registrations
            .iter()
            .any(|record| !record.relay_answered && digests.contains(&record.correlation_id));
        if !changed {
            return Ok(());
        }
        self.mutate(|registrations| {
            for record in registrations.iter_mut() {
                if digests.contains(&record.correlation_id) {
                    record.relay_answered = true;
                }
            }
        })
    }

    /// Move one pending record into [`RecordState::Claimed`].
    ///
    /// Called once the operation and command ledgers both name this
    /// `commandId`, and **only** then: the state asserts that start permission
    /// is already durable elsewhere.
    ///
    /// This is the one mutation that does not roll its in-memory half back
    /// when the write fails, and the asymmetry is deliberate. The claim itself
    /// is durable in the two ledgers whatever this file says; all this record
    /// still decides is whether *this process* would treat the registration as
    /// deliverable again. Rolling back to `ready` would do exactly that — hand
    /// the same registration to another delivery pass for a turn that is
    /// already permitted. Keeping the memory state and returning the error
    /// lets the caller log the lost durability without re-arming the promise.
    pub fn mark_claimed(&mut self, command_id: &str, at: u64) -> io::Result<()> {
        if !self
            .registrations
            .iter()
            .any(|record| record.command_id == command_id && record.state.is_pending())
        {
            return Ok(());
        }
        for record in self.registrations.iter_mut() {
            if record.command_id == command_id {
                record.state = RecordState::Claimed { at };
            }
        }
        self.persist()
    }

    /// Record a durable disposition for one registration.
    ///
    /// Written after the receipt is in the crash-safe outbox and after the
    /// refusal ledger, so a record that says `terminal` is one whose answer is
    /// both queued for the operator and fenced against a second answer. The
    /// converse is not guaranteed and does not need to be: a queued receipt
    /// with no terminal record is re-derived and re-fenced on replay.
    pub fn mark_terminal(
        &mut self,
        command_id: &str,
        code: &str,
        at: u64,
        observation: Option<RelayObservation>,
    ) -> io::Result<()> {
        if !self
            .registrations
            .iter()
            .any(|record| record.command_id == command_id && record.state.is_pending())
        {
            return Ok(());
        }
        self.mutate(|registrations| {
            for record in registrations.iter_mut() {
                if record.command_id == command_id {
                    record.state = RecordState::Terminal {
                        code: code.to_owned(),
                        at,
                        observation,
                    };
                }
            }
        })
    }

    /// Note a delivery attempt and hold the next one until `next_check_at`.
    pub fn note_attempt(&mut self, command_id: &str, next_check_at: u64) -> io::Result<()> {
        if !self
            .registrations
            .iter()
            .any(|record| record.command_id == command_id)
        {
            return Ok(());
        }
        self.mutate(|registrations| {
            for record in registrations.iter_mut() {
                if record.command_id == command_id {
                    record.attempts = record.attempts.saturating_add(1);
                    record.next_check_at = next_check_at;
                }
            }
        })
    }

    /// Note a delivery attempt that was held up by a *named* obstacle.
    ///
    /// Identical to [`Self::note_attempt`] except that it also records the
    /// obstacle on the record, so the eventual expiry can answer with it. A
    /// separate method rather than a wider `note_attempt` signature: the
    /// callers that defer for an unnamed transient reason genuinely have
    /// nothing to record, and passing them `None` would invite recording
    /// "unknown" over an obstacle a previous pass did name.
    pub fn note_attempt_with_obstacle(
        &mut self,
        command_id: &str,
        next_check_at: u64,
        obstacle: &str,
    ) -> io::Result<()> {
        if !self
            .registrations
            .iter()
            .any(|record| record.command_id == command_id)
        {
            return Ok(());
        }
        self.mutate(|registrations| {
            for record in registrations.iter_mut() {
                if record.command_id == command_id {
                    record.attempts = record.attempts.saturating_add(1);
                    record.next_check_at = next_check_at;
                    record.last_obstacle = Some(obstacle.to_owned());
                }
            }
        })
    }

    /// Bring one registration's deadline forward to now.
    ///
    /// Test-only. Expiry is a wall-clock fact and this crate's `now_secs`
    /// reads the system clock, so a test that wanted to observe what happens
    /// *after* a deferral would otherwise have to sleep out the registration's
    /// whole window. Nothing in production ever shortens a deadline.
    #[cfg(test)]
    pub fn expire_now(&mut self, command_id: &str) -> io::Result<()> {
        self.mutate(|registrations| {
            for record in registrations.iter_mut() {
                if record.command_id == command_id {
                    record.expires_at = 0;
                }
            }
        })
    }

    /// Remove a registration entirely.
    ///
    /// Used for exactly two cases, both of which have a durable fence of their
    /// own that a retained record could only duplicate: the promised turn's
    /// `commandId` reached the command ledger, or startup reconciliation found
    /// it already consumed or refused.
    pub fn remove(&mut self, command_id: &str) -> io::Result<()> {
        if !self
            .registrations
            .iter()
            .any(|record| record.command_id == command_id)
        {
            return Ok(());
        }
        self.mutate(|registrations| registrations.retain(|record| record.command_id != command_id))
    }

    /// Move one record's expiry.
    ///
    /// Test-only: reaching the expiry path otherwise means waiting out a real
    /// window, and a test that sleeps is a test that eventually flakes.
    #[cfg(test)]
    pub(crate) fn set_expiry_for_test(
        &mut self,
        command_id: &str,
        expires_at: u64,
    ) -> io::Result<()> {
        self.mutate(|registrations| {
            for record in registrations.iter_mut() {
                if record.command_id == command_id {
                    record.expires_at = expires_at;
                }
            }
        })
    }

    /// Apply `change`, persist, and roll the in-memory half back if the write
    /// failed — an undurable change is not a change.
    fn mutate(&mut self, change: impl FnOnce(&mut Vec<CiContinuationRecord>)) -> io::Result<()> {
        let previous = self.registrations.clone();
        change(&mut self.registrations);
        prune_terminals(&mut self.registrations);
        if let Err(error) = self.persist() {
            self.registrations = previous;
            return Err(error);
        }
        Ok(())
    }

    fn persist(&mut self) -> io::Result<()> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_write) {
            return Err(io::Error::other(
                "injected CI continuation store write failure",
            ));
        }
        let body = serde_json::to_vec_pretty(&SnapshotRef {
            version: STORE_VERSION,
            registrations: &self.registrations,
        })?;
        atomic_write(&self.path, &body)
    }
}

/// Drop the oldest terminal records beyond [`MAX_RETAINED_TERMINALS`].
///
/// Only terminal records are ever evicted, and the oldest disposition is the
/// one whose redelivery window closed longest ago.
fn prune_terminals(registrations: &mut Vec<CiContinuationRecord>) {
    let mut terminal_ats: Vec<u64> = registrations
        .iter()
        .filter_map(|record| match &record.state {
            RecordState::Terminal { at, .. } => Some(*at),
            _ => None,
        })
        .collect();
    if terminal_ats.len() <= MAX_RETAINED_TERMINALS {
        return;
    }
    terminal_ats.sort_unstable();
    let excess = terminal_ats.len() - MAX_RETAINED_TERMINALS;
    let cutoff = terminal_ats[excess.saturating_sub(1)];
    let mut still_to_drop = excess;
    registrations.retain(|record| match &record.state {
        RecordState::Terminal { at, .. } if *at <= cutoff && still_to_drop > 0 => {
            still_to_drop -= 1;
            false
        }
        _ => true,
    });
}

#[cfg(test)]
#[path = "ci_continuation_store_tests.rs"]
mod tests;
