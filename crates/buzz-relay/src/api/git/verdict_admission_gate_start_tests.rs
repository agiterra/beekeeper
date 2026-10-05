//! **SV-41** against Postgres: a gate start on the page changes no push.
//!
//! The provider signs an observed `gate:<gate>` **phase** when a gate command
//! has been running a few seconds. This module folds the same page with the
//! same fold, and reads only `.gates`; these cases pin that a start reaches
//! neither side of that read — it lands nothing a missing gate would refuse,
//! and refuses nothing green gates would land.

use super::*;
use super::{default_green, watched, Watched, HEAD_SHA};

use buzz_core::coding_session_observation::{
    CodingSessionObservationBody, CodingSessionObservationPayload,
    CodingSessionObservationPhaseTiming, CodingSessionObservationSource,
    CodingSessionObservationType, CODING_SESSION_OBSERVATION_SCHEMA,
};
use buzz_core::coding_session_verdict_admission::DEFAULT_REQUIRED_GATES;
use buzz_core::kind::KIND_CODING_SESSION_OBSERVATION;
use nostr::{EventBuilder, Kind, Tag};

/// Store one open provider-signed start for `gate` on the mission's channel.
async fn start(w: &Watched, gate: &str) {
    let payload = CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.into(),
        session_ref: w.session_ref.clone(),
        genesis_ref: w.genesis_ref.clone(),
        observation_type: CodingSessionObservationType::Phase,
        source: CodingSessionObservationSource::Observed,
        assignment_ref: None,
        body: CodingSessionObservationBody::Phase(CodingSessionObservationPhaseTiming {
            phase: format!("gate:{gate}"),
            started_at_ms: 1_759_572_120_000,
            ended_at_ms: None,
            duration_ms: None,
        }),
    };
    let event = EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        serde_json::to_string(&payload).expect("observation json"),
    )
    .tags([
        Tag::parse(["h", &w.channel_id.to_string()]).expect("h"),
        Tag::parse(["d", &w.session_ref]).expect("d"),
        Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("v"),
        Tag::parse(["csob-genesis", &w.genesis_ref]).expect("genesis"),
        Tag::parse(["csob-type", "phase"]).expect("type"),
    ])
    .sign_with_keys(&w.provider)
    .expect("sign start");
    w.state
        .db
        .insert_event(w.community, &event, Some(w.channel_id))
        .await
        .expect("insert start");
}

/// Every required gate only *started*: a push is refused, and the refusal
/// says no observed gate row names the commit — a start is no row at all.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_started_gate_is_not_an_observed_gate_at_the_push() {
    let w = watched().await;
    for gate in DEFAULT_REQUIRED_GATES {
        start(&w, gate).await;
    }
    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(
        body.contains(&format!("No observed gate row names {HEAD_SHA}")),
        "a start is not a gate row: {body}"
    );
    assert!(
        !body.contains("running"),
        "a start is never an outcome: {body}"
    );
}

/// Green on the head plus an open start beside them still lands.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_open_start_beside_green_rows_does_not_refuse_the_push() {
    let w = watched().await;
    w.observe(
        &w.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )
    .await;
    start(&w, DEFAULT_REQUIRED_GATES[0]).await;
    let (status, body) = w.seat_push(HEAD_SHA).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}
