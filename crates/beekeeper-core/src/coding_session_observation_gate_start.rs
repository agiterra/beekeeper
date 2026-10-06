//! Gate start as an observed fact (SV-41): the reserved `gate:` phase.
//!
//! A child of `coding_session_observation`, beside the fold, for the same
//! reason the fold is: no file here passes 1,000 lines. `use super::*` gives it
//! the parent's types.
//!
//! # What a gate start is on the wire
//!
//! A kind 44246 **phase** row, `source: "observed"`, whose `phase` is `gate:`
//! followed by the provider's table name for the gate (`gate:cargo test`). The
//! provider signs one when a recognised gate command has been running for a
//! few seconds, with `endedAtMs: null`, and signs a second row with the same
//! author, the same `phase` and the same `startedAtMs` — `endedAtMs` set — when
//! the call ends, whatever ended it.
//!
//! It is a phase and not a gate outcome on purpose (spec Decision 1). A gate
//! row is the one object every verdict and push path reads, and the fold keeps
//! the newest gate row per `(author, source, gate)`: a `running` outcome would
//! displace the `failed` row before it. A phase reaches none of those paths,
//! and a reader that predates this module already treats `endedAtMs: null` as
//! not final.
//!
//! # Pairing, and the one clock reading it allows
//!
//! A start is closed by its own closing phase row, matched on
//! `(author, gate, startedAtMs)`, in **either** supplied order — a gate that
//! finishes within the second it started may page ahead of its start. The
//! finished gate row does **not** close a start: it carries no reference to
//! one, and two seats on one provider routinely run the same gate at once
//! (spec Decision 2). NIP-CSOB forbids using an author's times for ordering,
//! discovery or dedupe; pairing an author's close with that same author's
//! start is narrower — it can only ever touch the author's own rows — and the
//! NIP's "Gate start" amendment says so.
//!
//! Staleness is not folded, because it depends on "now". A reader asks
//! [`gate_start_is_stale`]; a start with no close after
//! [`GATE_START_STALE_AFTER_MS`] reads "no result observed", never "running".

use std::collections::BTreeMap;

use super::*;

/// The reserved prefix that makes an observed phase a gate start.
pub const GATE_START_PHASE_PREFIX: &str = "gate:";

/// How long an unclosed gate start may read as running.
///
/// Thirty minutes: three times the longest call any harness here allows
/// (Claude Code's Bash tool and `buzz-dev-mcp`'s shell both cap one at ten
/// minutes, and a backgrounded command returns at once). Measured against the
/// provider's own `startedAtMs`, and displayed as the provider's clock.
pub const GATE_START_STALE_AFTER_MS: u64 = 30 * 60 * 1_000;

/// The most gate starts one fold lists. The **newest** are kept: the running
/// ones are what a reader opened the screen for.
pub const MAX_OBSERVATION_GATE_STARTS: usize = 64;

/// The `phase` a gate start for `gate` carries: `gate:<gate>`.
///
/// `gate` is the provider's table name (`cargo test`), never a command line.
/// Refused when blank, padded, carrying a control character, or long enough to
/// take the phase past [`MAX_OBSERVATION_NAME_BYTES`].
pub fn gate_start_phase_name(gate: &str) -> Result<String, String> {
    validate_gate_start_gate(gate)?;
    let phase = format!("{GATE_START_PHASE_PREFIX}{gate}");
    if phase.len() > MAX_OBSERVATION_NAME_BYTES {
        return Err(format!(
            "gate start phase exceeds {MAX_OBSERVATION_NAME_BYTES} bytes (got {})",
            phase.len()
        ));
    }
    Ok(phase)
}

/// The gate a reserved `gate:` phase names, or `None` when `phase` is not one.
///
/// Only a non-empty remainder counts. This reads the name; whether the row is
/// a gate start at all also needs the effective source to be `observed`, which
/// is the fold's question, not this one's.
pub fn gate_of_start_phase(phase: &str) -> Option<&str> {
    phase
        .strip_prefix(GATE_START_PHASE_PREFIX)
        .filter(|gate| !gate.is_empty())
}

/// Whether a start with no close has gone past the point of reading as running.
///
/// True once `now_ms` is at or after `started_at_ms + GATE_START_STALE_AFTER_MS`.
/// A start dated in the viewer's future is not stale: the provider's clock is
/// ahead, and that is a disclosure, not an expiry.
pub fn gate_start_is_stale(started_at_ms: u64, now_ms: u64) -> bool {
    now_ms >= started_at_ms.saturating_add(GATE_START_STALE_AFTER_MS)
}

/// The structural rule a reserved `gate:` phase is held to, on top of every
/// phase's own.
///
/// Called from the phase validator for a row that claims `source: observed`
/// only, so the relay's ingest check, the strict decoder and every builder
/// refuse the same malformed start: an empty or padded gate name, and a
/// `durationMs` on a row that has not ended (a span cannot be measured before
/// it closes). A declared `gate:` phase is its author's own words and keeps
/// the ordinary phase rules — tightening it would turn rows a seat could
/// legally sign before SV-41 into `ignored` in every fold.
pub(super) fn validate_gate_start_phase(
    body: &CodingSessionObservationPhaseTiming,
) -> Result<(), String> {
    let Some(gate) = body.phase.strip_prefix(GATE_START_PHASE_PREFIX) else {
        return Ok(());
    };
    validate_gate_start_gate(gate)?;
    if body.ended_at_ms.is_none() && body.duration_ms.is_some() {
        return Err("gate start durationMs must be null while endedAtMs is null".into());
    }
    Ok(())
}

fn validate_gate_start_gate(gate: &str) -> Result<(), String> {
    if gate.trim().is_empty() {
        return Err("gate start phase must name a gate after `gate:`".into());
    }
    if gate.trim() != gate {
        return Err("gate start gate name must not begin or end with whitespace".into());
    }
    if gate.chars().any(char::is_control) {
        return Err("gate start gate name must not contain control characters".into());
    }
    Ok(())
}

/// The closing row a gate start was paired with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationGateStartClose {
    /// Event id of the closing phase row.
    pub event_id: String,
    /// When the provider says the call ended. Its own clock.
    pub ended_at_ms: u64,
    /// The provider's measured span; `None` when it measured none (a clock
    /// that ran backwards, a close signed for a call it never saw end).
    pub duration_ms: Option<u64>,
}

/// One gate start, open or closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationGateStartEntry {
    /// Event id of the start row.
    pub event_id: String,
    /// The provider instance that watched the call.
    pub author_pubkey: String,
    /// The provider's table name for the gate, e.g. `cargo test`.
    pub gate: String,
    /// When the provider says the call began. Its own clock.
    pub started_at_ms: u64,
    /// As written; observed starts carry `null`.
    pub assignment_ref: Option<String>,
    /// The paired close, when the page holds one.
    pub close: Option<CodingSessionObservationGateStartClose>,
}

/// The working set the fold routes observed `gate:` phases into.
#[derive(Debug, Default)]
pub(super) struct GateStartCollector {
    starts: Vec<CodingSessionObservationGateStartEntry>,
    start_keys: BTreeMap<(String, String, u64), usize>,
    closes: BTreeMap<(String, String, u64), CodingSessionObservationGateStartClose>,
}

impl GateStartCollector {
    /// Take one observed phase row. Returns the body back when it is not a gate
    /// start, so the caller lists it as an ordinary phase.
    pub(super) fn take(
        &mut self,
        event_id: String,
        author: &str,
        assignment_ref: Option<String>,
        body: CodingSessionObservationPhaseTiming,
    ) -> Option<(String, Option<String>, CodingSessionObservationPhaseTiming)> {
        let Some(gate) = gate_of_start_phase(&body.phase) else {
            return Some((event_id, assignment_ref, body));
        };
        let key = (author.to_owned(), gate.to_owned(), body.started_at_ms);
        match body.ended_at_ms {
            // A repeat of the same start is the same statement: the first is kept.
            None => {
                if !self.start_keys.contains_key(&key) {
                    self.start_keys.insert(key, self.starts.len());
                    self.starts.push(CodingSessionObservationGateStartEntry {
                        event_id,
                        author_pubkey: author.to_owned(),
                        gate: gate.to_owned(),
                        started_at_ms: body.started_at_ms,
                        assignment_ref,
                        close: None,
                    });
                }
            }
            Some(ended_at_ms) => {
                self.closes
                    .entry(key)
                    .or_insert(CodingSessionObservationGateStartClose {
                        event_id,
                        ended_at_ms,
                        duration_ms: body.duration_ms,
                    });
            }
        }
        None
    }

    /// Pair, bound and hand the collection to the fold.
    pub(super) fn finish(mut self, fold: &mut CodingSessionObservationFold) {
        for entry in &mut self.starts {
            let key = (
                entry.author_pubkey.clone(),
                entry.gate.clone(),
                entry.started_at_ms,
            );
            entry.close = self.closes.remove(&key);
        }
        // Whatever is left closed a start this page does not hold. A
        // disclosure, not a drop: the start fell off the page, or was never
        // published.
        fold.truncated.gate_start_closes_unmatched += self.closes.len();
        let excess = self
            .starts
            .len()
            .saturating_sub(MAX_OBSERVATION_GATE_STARTS);
        if excess > 0 {
            self.starts.drain(..excess);
            fold.truncated.gate_starts += excess;
        }
        fold.gate_starts = self.starts;
    }
}

/// A provider-signed gate start (or close, with `ended_at_ms`), for the
/// consumer tests that pin "a start is never an outcome" from their own side.
#[cfg(test)]
pub(crate) fn test_gate_start_event(
    keys: &nostr::Keys,
    channel: &str,
    session_ref: &str,
    genesis_ref: &str,
    gate: &str,
    started_at_ms: u64,
    ended_at_ms: Option<u64>,
) -> Event {
    let payload = CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.into(),
        session_ref: session_ref.into(),
        genesis_ref: genesis_ref.into(),
        observation_type: CodingSessionObservationType::Phase,
        source: CodingSessionObservationSource::Observed,
        assignment_ref: None,
        body: CodingSessionObservationBody::Phase(CodingSessionObservationPhaseTiming {
            phase: format!("{GATE_START_PHASE_PREFIX}{gate}"),
            started_at_ms,
            ended_at_ms,
            duration_ms: ended_at_ms.map(|end| end.saturating_sub(started_at_ms)),
        }),
    };
    nostr::EventBuilder::new(
        nostr::Kind::Custom(crate::kind::KIND_CODING_SESSION_OBSERVATION as u16),
        serde_json::to_string(&payload).expect("payload serializes"),
    )
    .tags([
        nostr::Tag::parse(["h", channel]).expect("h tag"),
        nostr::Tag::parse(["d", session_ref]).expect("d tag"),
        nostr::Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("version tag"),
        nostr::Tag::parse(["csob-genesis", genesis_ref]).expect("genesis tag"),
        nostr::Tag::parse(["csob-type", "phase"]).expect("type tag"),
    ])
    .sign_with_keys(keys)
    .expect("event signs")
}

#[cfg(test)]
#[path = "coding_session_observation_gate_start_tests.rs"]
mod tests;
