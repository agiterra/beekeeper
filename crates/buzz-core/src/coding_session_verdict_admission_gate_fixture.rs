//! The kind 44246 half of a verdict-admission fixture, in one place.
//!
//! Added 2026-09-03 (L27), when arm (C) started requiring arm (B)'s evidence
//! on top of a verifier's clearance. Before that, every arm-(C) case could be
//! written with `observed_gates: Vec::new()`; now each of them needs a folded
//! set of provider-signed rows, and four test files would otherwise each grow
//! their own copy of the builder — four chances for one of them to drift into
//! constructing entries by hand and quietly skipping the provenance fold that
//! decides what `observed` means.
//!
//! Everything here goes through
//! [`crate::coding_session_observation::fold_coding_session_observations`]
//! with the provider set supplied, exactly as the relay folds. A row a seat
//! signed is downgraded to `declared` here for the same reason it is
//! downgraded there.

use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};

use crate::coding_session_observation::{
    fold_coding_session_observations, CodingSessionObservationBody,
    CodingSessionObservationFoldContext, CodingSessionObservationGate,
    CodingSessionObservationGateEntry, CodingSessionObservationGateOutcome,
    CodingSessionObservationGateRow, CodingSessionObservationPayload,
    CodingSessionObservationSource, CodingSessionObservationType,
    CODING_SESSION_OBSERVATION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_OBSERVATION;

use super::{VerdictAdmissionGatePolicy, DEFAULT_REQUIRED_GATES};

/// One gate row as a provider signs it: green on `head_sha`, clean tree.
pub(super) fn green_row(gate: &str, head_sha: &str) -> CodingSessionObservationGateRow {
    CodingSessionObservationGateRow {
        gate: gate.to_owned(),
        outcome: CodingSessionObservationGateOutcome::Passed,
        command: format!("{gate} --locked"),
        summary: None,
        duration_ms: Some(1_000),
        head_sha: Some(head_sha.to_owned()),
        dirty: Some(false),
    }
}

/// Every gate [`DEFAULT_REQUIRED_GATES`] names, green on `head_sha`.
pub(super) fn default_green(head_sha: &str) -> Vec<CodingSessionObservationGateRow> {
    DEFAULT_REQUIRED_GATES
        .iter()
        .map(|gate| green_row(gate, head_sha))
        .collect()
}

/// Sign one kind 44246 gate observation carrying `rows`.
pub(super) fn signed_observation(
    keys: &Keys,
    channel: &str,
    session_ref: &str,
    genesis_ref: &str,
    source: CodingSessionObservationSource,
    rows: Vec<CodingSessionObservationGateRow>,
) -> Event {
    let payload = CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.into(),
        session_ref: session_ref.to_owned(),
        genesis_ref: genesis_ref.to_owned(),
        observation_type: CodingSessionObservationType::Gate,
        source,
        assignment_ref: None,
        body: CodingSessionObservationBody::Gate(CodingSessionObservationGate { rows }),
    };
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        serde_json::to_string(&payload).expect("payload serializes"),
    )
    .tags([
        Tag::parse(["h", channel]).expect("h tag"),
        Tag::parse(["d", session_ref]).expect("d tag"),
        Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("version tag"),
        Tag::parse(["csob-genesis", genesis_ref]).expect("genesis tag"),
        Tag::parse(["csob-type", "gate"]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(400))
    .sign_with_keys(keys)
    .expect("event signs")
}

/// Fold `events` the way the relay does, honouring exactly one provider.
pub(super) fn folded(
    provider: &Keys,
    session_ref: &str,
    genesis_ref: &str,
    events: &[Event],
) -> Vec<CodingSessionObservationGateEntry> {
    fold_coding_session_observations(
        events,
        &CodingSessionObservationFoldContext {
            session_ref: session_ref.to_owned(),
            genesis_ref: genesis_ref.to_owned(),
            known_assignment_refs: Vec::new(),
            provider_pubkeys: Some(vec![provider.public_key().to_hex()]),
        },
    )
    .gates
}

/// The rows an arm-(C) case needs to still be an arm-(C) case: every default
/// gate observed green on `head_sha` by a provider nobody else holds.
///
/// The provider key is generated and discarded — no case that calls this cares
/// *who* watched the gates, only that the fold kept the rows `observed`.
pub(super) fn observed_green_gates(
    channel: &str,
    session_ref: &str,
    genesis_ref: &str,
    head_sha: &str,
) -> Vec<CodingSessionObservationGateEntry> {
    let provider = Keys::generate();
    let event = signed_observation(
        &provider,
        channel,
        session_ref,
        genesis_ref,
        CodingSessionObservationSource::Observed,
        default_green(head_sha),
    );
    folded(&provider, session_ref, genesis_ref, &[event])
}

/// `gates.verifierRequired: true` — the policy that closes arm (B) and leaves
/// arm (C) the only route, so a case about arm (C) is about arm (C).
///
/// Every arm-(C) fixture carries it since L27: without it a mission holding
/// both a clearance and green rows is admitted by arm (B) first (it is
/// evaluated first, being the cheaper arm), and the evidence would name the
/// wrong arm for reasons that have nothing to do with the case.
pub(super) fn verifier_required_policy() -> Option<VerdictAdmissionGatePolicy> {
    Some(VerdictAdmissionGatePolicy {
        verifier_required: Some(true),
        required_gates: None,
    })
}
