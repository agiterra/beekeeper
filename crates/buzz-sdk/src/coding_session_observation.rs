//! Typed builder and parser for NIP-CSOB observations (kind 44246).

use buzz_core::coding_session_observation::{
    decode_coding_session_observation, validate_coding_session_observation_envelope,
    CodingSessionObservationPayload, CODING_SESSION_OBSERVATION_SCHEMA,
};
use buzz_core::kind::KIND_CODING_SESSION_OBSERVATION;
use nostr::{Event, EventBuilder, Kind, Tag};

use crate::SdkError;

/// Build a structurally validated kind 44246 observation.
///
/// The builder emits the exact five-tag envelope NIP-CSOB requires — `h`, `d`,
/// `csob-v`, `csob-genesis`, `csob-type`, in that order — and leaves signing
/// and publishing to the caller. It does **not** check that the caller holds a
/// seat: the signature is the author, and whether that author was seated is the
/// consuming fold's question against the accepted NIP-CSAT chain, exactly as
/// for kinds 44244 and 44245.
///
/// Everything the parser refuses, the builder refuses first, so no author ever
/// signs bytes that the relay and the reader will both reject (the REVIEW-B1 F5
/// rule, applied here from the start).
pub fn build_coding_session_observation(
    channel_ref: &str,
    payload: CodingSessionObservationPayload,
) -> Result<EventBuilder, SdkError> {
    validate_observation_channel_ref(channel_ref)?;
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(&payload).map_err(|error| {
        SdkError::InvalidInput(format!("payload serialization failed: {error}"))
    })?;
    // Exercise the strict decoder before returning bytes to a signer: a typed
    // serialization alone would conceal an exact-key regression until the event
    // was already signed.
    decode_coding_session_observation(&content).map_err(SdkError::InvalidInput)?;

    let tags = [
        ["h", channel_ref],
        ["d", payload.session_ref.as_str()],
        ["csob-v", CODING_SESSION_OBSERVATION_SCHEMA],
        ["csob-genesis", payload.genesis_ref.as_str()],
        ["csob-type", payload.observation_type.as_str()],
    ]
    .into_iter()
    .map(|parts| Tag::parse(parts).map_err(|error| SdkError::InvalidTag(error.to_string())))
    .collect::<Result<Vec<_>, _>>()?;

    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        content,
    )
    .tags(tags))
}

/// Check `channel_ref` against the exact rule the envelope validator applies to
/// the `h` tag: a canonical lowercase hyphenated UUID.
fn validate_observation_channel_ref(channel_ref: &str) -> Result<(), SdkError> {
    let parsed = uuid::Uuid::parse_str(channel_ref)
        .map_err(|_| SdkError::InvalidInput("h must be a UUID".to_owned()))?;
    if parsed.to_string() != channel_ref {
        return Err(SdkError::InvalidInput(
            "h must be a lowercase canonical UUID".to_owned(),
        ));
    }
    Ok(())
}

/// Parse and validate a signed kind 44246 event's exact envelope.
pub fn parse_coding_session_observation(
    event: &Event,
) -> Result<CodingSessionObservationPayload, SdkError> {
    validate_coding_session_observation_envelope(event).map_err(SdkError::InvalidInput)
}

#[cfg(test)]
mod tests {
    use buzz_core::coding_session_observation::{
        CodingSessionObservationBody, CodingSessionObservationGate,
        CodingSessionObservationGateOutcome, CodingSessionObservationGateRow,
        CodingSessionObservationSource, CodingSessionObservationType,
    };
    use nostr::Keys;

    use super::*;

    const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
    const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";

    fn gate_observation() -> CodingSessionObservationPayload {
        CodingSessionObservationPayload {
            schema: CODING_SESSION_OBSERVATION_SCHEMA.to_owned(),
            session_ref: SESSION.to_owned(),
            genesis_ref: "12".repeat(32),
            observation_type: CodingSessionObservationType::Gate,
            source: CodingSessionObservationSource::Declared,
            assignment_ref: None,
            body: CodingSessionObservationBody::Gate(CodingSessionObservationGate {
                rows: vec![CodingSessionObservationGateRow {
                    gate: "just ci".to_owned(),
                    outcome: CodingSessionObservationGateOutcome::Passed,
                    command: "just ci".to_owned(),
                    summary: None,
                    duration_ms: Some(41_000),
                }],
            }),
        }
    }

    #[test]
    fn builder_emits_exact_envelope_and_round_trips() {
        let payload = gate_observation();
        let event = build_coding_session_observation(CHANNEL, payload.clone())
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");

        assert_eq!(
            parse_coding_session_observation(&event).expect("parse"),
            payload
        );
        let tags: Vec<Vec<String>> = event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect();
        assert_eq!(tags.len(), 5);
        assert_eq!(tags[0], ["h", CHANNEL]);
        assert_eq!(tags[1], ["d", SESSION]);
        assert_eq!(tags[2], ["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]);
        assert_eq!(tags[3], ["csob-genesis", &"12".repeat(32)]);
        assert_eq!(tags[4], ["csob-type", "gate"]);
        // Absent is not null: an unset optional is on the wire as JSON null.
        assert!(
            event.content.contains("\"assignmentRef\":null"),
            "{}",
            event.content
        );
        // Provenance is on the wire in words, never inferred from the signer.
        assert!(
            event.content.contains("\"source\":\"declared\""),
            "{}",
            event.content
        );
    }

    #[test]
    fn the_builder_refuses_a_channel_its_own_parser_would_reject() {
        for bad_channel in [
            "not-a-uuid",
            "",
            "D3E440EA-89F8-4AEE-8A02-17EDC3E7272E",
            "{d3e440ea-89f8-4aee-8a02-17edc3e7272e}",
            "d3e440ea89f84aee8a0217edc3e7272e",
        ] {
            assert!(
                build_coding_session_observation(bad_channel, gate_observation()).is_err(),
                "{bad_channel:?} must never reach a signer"
            );
        }
    }

    #[test]
    fn an_invalid_observation_never_reaches_a_signer() {
        let mut empty_rows = gate_observation();
        empty_rows.body =
            CodingSessionObservationBody::Gate(CodingSessionObservationGate { rows: Vec::new() });
        assert!(build_coding_session_observation(CHANNEL, empty_rows).is_err());

        let mut wrong_type = gate_observation();
        wrong_type.observation_type = CodingSessionObservationType::Finding;
        assert!(build_coding_session_observation(CHANNEL, wrong_type).is_err());

        let mut bad_session = gate_observation();
        bad_session.session_ref = "NOT-A-UUID".to_owned();
        assert!(build_coding_session_observation(CHANNEL, bad_session).is_err());
    }
}
