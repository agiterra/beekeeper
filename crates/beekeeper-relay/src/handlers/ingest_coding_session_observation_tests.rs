use beekeeper_core::coding_session_observation::{
    validate_coding_session_observation_envelope, CodingSessionObservationBody,
    CodingSessionObservationGate, CodingSessionObservationGateOutcome,
    CodingSessionObservationGateRow, CodingSessionObservationPayload,
    CodingSessionObservationSource, CodingSessionObservationType,
    CODING_SESSION_OBSERVATION_SCHEMA,
};
use beekeeper_sdk::coding_session_observation::build_coding_session_observation;
use nostr::EventBuilder;

use super::*;

fn gate_observation(session: &str, genesis: &str) -> CodingSessionObservationPayload {
    CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.to_owned(),
        session_ref: session.to_owned(),
        genesis_ref: genesis.to_owned(),
        observation_type: CodingSessionObservationType::Gate,
        // A fixture the relay signs itself: it watched nothing, so it says so.
        source: CodingSessionObservationSource::Declared,
        assignment_ref: None,
        body: CodingSessionObservationBody::Gate(CodingSessionObservationGate {
            rows: vec![CodingSessionObservationGateRow {
                gate: "just ci".to_owned(),
                outcome: CodingSessionObservationGateOutcome::Passed,
                command: "just ci".to_owned(),
                summary: Some("all stages green".to_owned()),
                duration_ms: Some(515_000),
                head_sha: None,
                dirty: None,
            }],
        }),
    }
}

/// The relay knows kind 44246, admits it under the same strict coding-session
/// membership gate every other session kind uses, and checks its structure
/// through `buzz-core`'s own envelope validator.
///
/// It deliberately does **not** decide authority. Whether the signer held an
/// active seat on this umbrella — or was its founder — is the consuming fold's
/// question against the accepted NIP-CSAT chain, exactly as it is for kinds
/// 44244 and 44245. The signature here is a generated key that was never
/// seated, and the relay still accepts the event: that is the division
/// NIP-CSP draws, asserted rather than assumed.
#[test]
fn observation_uses_core_envelope_and_strict_membership_admission() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "cd".repeat(32);
    let valid = build_coding_session_observation(&channel, gate_observation(&session, &genesis))
        .expect("observation builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign observation");
    assert!(validate_coding_session_observation_envelope(&valid).is_ok());

    assert_eq!(
        required_scope_for_kind(KIND_CODING_SESSION_OBSERVATION, &valid).unwrap(),
        Scope::MessagesWrite
    );
    assert!(requires_h_channel_scope(KIND_CODING_SESSION_OBSERVATION));
    assert!(is_coding_session_kind(KIND_CODING_SESSION_OBSERVATION));
    assert!(requires_strict_coding_session_membership(
        KIND_CODING_SESSION_OBSERVATION
    ));
}

/// Membership is answered before the body is ever parsed.
///
/// 44246 goes through `requires_strict_coding_session_membership`, so a signer
/// who is not a member of the channel is refused whatever its content says —
/// the refusal is about standing in the channel, not about JSON.
#[test]
fn a_non_member_is_refused_before_the_observation_is_parsed() {
    assert!(requires_strict_coding_session_membership(
        KIND_CODING_SESSION_OBSERVATION
    ));
    assert!(coding_session_membership_verdict(true).is_ok());
    let message =
        coding_session_membership_verdict(false).expect_err("a non-member observation is refused");
    assert!(
        message.starts_with("restricted:") && !message.contains("invalid:"),
        "a non-member refusal must read as a standing refusal, not a structural one: {message}"
    );
}

/// A malformed body is refused by the relay's gate, with `buzz-core`'s own
/// sentence rather than a second implementation's paraphrase.
///
/// Three shapes, one per thing the envelope validator owns: an unknown body
/// key (v1 rejects rather than ignores), a `csob-type` tag disagreeing with the
/// content `type`, and a sixth tag on an envelope that takes exactly five.
#[test]
fn a_malformed_observation_is_refused_by_the_relay_gate() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "cd".repeat(32);
    let valid = build_coding_session_observation(&channel, gate_observation(&session, &genesis))
        .expect("observation builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign observation");

    let mut content: serde_json::Value =
        serde_json::from_str(&valid.content).expect("observation content");
    content
        .as_object_mut()
        .expect("observation object")
        .insert("cadence".to_owned(), serde_json::json!("hourly"));
    let unknown_key = EventBuilder::new(valid.kind, content.to_string())
        .tags(valid.tags.iter().cloned())
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign observation with an unknown key");
    let error = validate_coding_session_observation_envelope(&unknown_key)
        .expect_err("an unknown key is refused");
    assert!(error.contains("cadence"), "{error}");

    let mut crossed: Vec<nostr::Tag> = valid.tags.iter().cloned().collect();
    crossed[4] = nostr::Tag::parse(["csob-type", "finding"]).expect("crossed type tag");
    let crossed_type = EventBuilder::new(valid.kind, valid.content.clone())
        .tags(crossed)
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign observation with a crossed type tag");
    let error = validate_coding_session_observation_envelope(&crossed_type)
        .expect_err("a csob-type tag that disagrees with the content is refused");
    assert!(
        error.contains("type tag does not match payload type"),
        "{error}"
    );

    let sixth_tag = EventBuilder::new(valid.kind, valid.content.clone())
        .tags(
            valid
                .tags
                .iter()
                .cloned()
                .chain([nostr::Tag::parse(["unexpected", "tag"]).expect("extra tag")]),
        )
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign observation with a sixth tag");
    let error = validate_coding_session_observation_envelope(&sixth_tag)
        .expect_err("a sixth tag is refused");
    assert!(error.contains("exactly five two-field tags"), "{error}");
}

fn gate_start(
    session: &str,
    genesis: &str,
    phase: &str,
    ended_at_ms: Option<u64>,
    duration_ms: Option<u64>,
) -> CodingSessionObservationPayload {
    CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.to_owned(),
        session_ref: session.to_owned(),
        genesis_ref: genesis.to_owned(),
        observation_type: CodingSessionObservationType::Phase,
        source: CodingSessionObservationSource::Observed,
        assignment_ref: None,
        body: CodingSessionObservationBody::Phase(
            beekeeper_core::coding_session_observation::CodingSessionObservationPhaseTiming {
                phase: phase.to_owned(),
                started_at_ms: 1_759_572_120_000,
                ended_at_ms,
                duration_ms,
            },
        ),
    }
}

/// SV-41. The provider's gate start and its close are ordinary phase rows on
/// the existing kind, so the relay's gate accepts both with no schema change.
#[test]
fn the_relay_gate_accepts_a_gate_start_and_its_close() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "cd".repeat(32);
    for (ended, duration) in [
        (None, None),
        (Some(1_759_572_300_000), Some(180_000)),
        (Some(1_759_572_120_000), None),
    ] {
        let event = build_coding_session_observation(
            &channel,
            gate_start(&session, &genesis, "gate:cargo test", ended, duration),
        )
        .expect("a well-formed start builds")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign start");
        validate_coding_session_observation_envelope(&event).expect("the relay accepts it");
    }
}

/// SV-41. A malformed start is refused with `buzz-core`'s sentence: no gate
/// after the prefix, a padded gate name, a duration on a row still open, and a
/// close that ends before it started.
#[test]
fn the_relay_gate_refuses_a_malformed_gate_start() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "cd".repeat(32);
    let valid = build_coding_session_observation(
        &channel,
        gate_start(&session, &genesis, "gate:cargo test", None, None),
    )
    .expect("a well-formed start builds")
    .sign_with_keys(&nostr::Keys::generate())
    .expect("sign start");
    let cases: [(serde_json::Value, &str); 4] = [
        (
            serde_json::json!({"phase": "gate:", "startedAtMs": 5, "endedAtMs": null, "durationMs": null}),
            "must name a gate",
        ),
        (
            serde_json::json!({"phase": "gate:cargo test ", "startedAtMs": 5, "endedAtMs": null, "durationMs": null}),
            "whitespace",
        ),
        (
            serde_json::json!({"phase": "gate:cargo test", "startedAtMs": 5, "endedAtMs": null, "durationMs": 9}),
            "durationMs must be null",
        ),
        (
            serde_json::json!({"phase": "gate:cargo test", "startedAtMs": 5, "endedAtMs": 4, "durationMs": null}),
            "must not precede",
        ),
    ];
    for (body, expected) in cases {
        let mut content: serde_json::Value =
            serde_json::from_str(&valid.content).expect("start content");
        content["body"] = body;
        let event = EventBuilder::new(valid.kind, content.to_string())
            .tags(valid.tags.iter().cloned())
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign malformed start");
        let error = validate_coding_session_observation_envelope(&event)
            .expect_err("a malformed start is refused");
        assert!(error.contains(expected), "{expected}: {error}");
    }
}
