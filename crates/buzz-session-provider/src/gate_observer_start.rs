//! The kind 44246 rows a gate start becomes (SV-41): pure builders, the clock
//! clamp, and the outbox semantic keys.
//!
//! A child of [`crate::gate_observer`], split out so the observer stays a
//! matcher and this stays a wire shape. The provider calls
//! [`gate_start_payload`] for both the start and its close, signs the result
//! with its own key, and enqueues it under [`gate_start_semantic_key`].
//!
//! What never reaches the wire from here: the session id, the tool id, the
//! workdir, the command line, and the publish delay. The row names the
//! table's gate (`gate:cargo test`) and two of the provider's own clock
//! readings, and nothing else.

use buzz_core::coding_session_observation::{
    gate_start_phase_name, CodingSessionObservationBody, CodingSessionObservationPayload,
    CodingSessionObservationPhaseTiming, CodingSessionObservationSource,
    CodingSessionObservationType, CODING_SESSION_OBSERVATION_SCHEMA,
};

use super::ObservedGateStart;

/// The observed phase row for one start (`ended_at_ms: None`) or its close.
///
/// `None` when nothing honest can be built: a start dated before the epoch, or
/// a gate name the shared validator refuses. The close is clamped rather than
/// refused when the clock ran backwards — `endedAtMs = startedAtMs` with
/// `durationMs: null` — because a smaller end is invalid and a zero duration
/// would claim a measurement nobody made; leaving the start open instead would
/// read as running until the stale rule. A close that is the provider no
/// longer watching the call (`measured: false`: eviction, turn end, exit)
/// carries `durationMs: null` too — the command was not seen to end.
pub fn gate_start_payload(
    session_ref: &str,
    genesis_ref: &str,
    start: &ObservedGateStart,
) -> Option<CodingSessionObservationPayload> {
    let started_at_ms = u64::try_from(start.started_at_ms).ok()?;
    let phase = gate_start_phase_name(start.gate).ok()?;
    let (ended_at_ms, duration_ms) = match start.ended_at_ms {
        None => (None, None),
        Some(ended) => match u64::try_from(ended) {
            Ok(ended) if ended >= started_at_ms => {
                (Some(ended), start.measured.then(|| ended - started_at_ms))
            }
            // The provider's clock went backwards between open and end.
            _ => (Some(started_at_ms), None),
        },
    };
    Some(CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.to_owned(),
        session_ref: session_ref.to_owned(),
        genesis_ref: genesis_ref.to_owned(),
        observation_type: CodingSessionObservationType::Phase,
        source: CodingSessionObservationSource::Observed,
        // Null for the same reason an observed gate row's is: the provider
        // watched a command run, and has no signed evidence of which
        // assignment the seat believed it was answering.
        assignment_ref: None,
        body: CodingSessionObservationBody::Phase(CodingSessionObservationPhaseTiming {
            phase,
            started_at_ms,
            ended_at_ms,
            duration_ms,
        }),
    })
}

/// The outbox key for one start or close: one row per fact, so a replay or a
/// re-enqueue is a no-op (`publish.rs`, "semantic key").
pub fn gate_start_semantic_key(session_id: &str, start: &ObservedGateStart) -> String {
    let stage = if start.ended_at_ms.is_some() {
        "coding-session-gate-start-close"
    } else {
        "coding-session-gate-start"
    };
    format!(
        "{stage}:{session_id}:{}:{}",
        start.gate, start.started_at_ms
    )
}
