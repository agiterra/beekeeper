//! Typed builder and parser for NIP-CSP session policy records (kind 44245).

use beekeeper_core::coding_session_policy::{
    decode_coding_session_policy, validate_coding_session_policy_envelope,
    CodingSessionPolicyPayload, CODING_SESSION_POLICY_SCHEMA,
};
use beekeeper_core::kind::KIND_CODING_SESSION_POLICY;
use nostr::{Event, EventBuilder, Kind, Tag};

use crate::SdkError;

/// Build a structurally validated kind 44245 session policy record.
///
/// The builder emits the exact four-tag envelope NIP-CSP requires — `h`, `d`,
/// `csp-v`, `csp-genesis`, in that order — and leaves signing and publishing
/// to the caller. It does **not** check that the caller holds the standing to
/// set policy: the signature is the author, and authority is the consuming
/// fold's question against the accepted NIP-CSAT chain, exactly as it is for
/// kind 44244.
pub fn build_coding_session_policy(
    channel_ref: &str,
    payload: CodingSessionPolicyPayload,
) -> Result<EventBuilder, SdkError> {
    // The builder must refuse everything its own parser refuses. It validated
    // the payload and re-ran the strict decoder over the content, but wrote
    // `channel_ref` into the `h` tag unchecked — so a non-UUID channel
    // produced a signed event that `parse_coding_session_policy` and the relay
    // both reject (REVIEW-B1 F5). A builder that signs bytes nothing will
    // accept has moved the failure past the point where the author can act
    // on it.
    validate_policy_channel_ref(channel_ref)?;
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(&payload).map_err(|error| {
        SdkError::InvalidInput(format!("payload serialization failed: {error}"))
    })?;
    // Exercise the strict decoder before returning bytes to a signer. A typed
    // serialization alone would conceal an exact-key regression — the producer
    // and the reader would disagree only once the event was signed.
    decode_coding_session_policy(&content).map_err(SdkError::InvalidInput)?;

    let tags = [
        ["h", channel_ref],
        ["d", payload.session_ref.as_str()],
        ["csp-v", CODING_SESSION_POLICY_SCHEMA],
        ["csp-genesis", payload.genesis_ref.as_str()],
    ]
    .into_iter()
    .map(|parts| Tag::parse(parts).map_err(|error| SdkError::InvalidTag(error.to_string())))
    .collect::<Result<Vec<_>, _>>()?;

    Ok(EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_POLICY as u16), content).tags(tags))
}

/// Check `channel_ref` against the exact rule the envelope validator applies
/// to the `h` tag: a canonical lowercase hyphenated UUID.
fn validate_policy_channel_ref(channel_ref: &str) -> Result<(), SdkError> {
    let parsed = uuid::Uuid::parse_str(channel_ref)
        .map_err(|_| SdkError::InvalidInput("h must be a UUID".to_owned()))?;
    if parsed.to_string() != channel_ref {
        return Err(SdkError::InvalidInput(
            "h must be a lowercase canonical UUID".to_owned(),
        ));
    }
    Ok(())
}

/// Parse and validate a signed kind 44245 event's exact envelope.
pub fn parse_coding_session_policy(event: &Event) -> Result<CodingSessionPolicyPayload, SdkError> {
    validate_coding_session_policy_envelope(event).map_err(SdkError::InvalidInput)
}

#[cfg(test)]
mod tests {
    use beekeeper_core::coding_session_policy::{
        CodingSessionAttention, CodingSessionIrreversibleAct, CodingSessionPolicyBudget,
        CodingSessionPolicyGates, CodingSessionPosture,
    };
    use nostr::Keys;

    use super::*;

    const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";

    fn policy() -> CodingSessionPolicyPayload {
        CodingSessionPolicyPayload {
            posture: Some(CodingSessionPosture::Ship),
            budget: Some(CodingSessionPolicyBudget {
                turns: Some(120),
                tokens_per_seat: None,
                tokens_per_session: None,
                cost_usd_per_session: Some(80.0),
                context_tier: None,
            }),
            attention: Some(CodingSessionAttention::DecisionsAndMilestones),
            gates: Some(CodingSessionPolicyGates {
                red_first: Some(true),
                review_every_lane: None,
                required_gates: Some(vec!["just ci".to_owned()]),
                verifier_required: None,
            }),
            irreversible: Some(vec![CodingSessionIrreversibleAct::Push]),
            ..CodingSessionPolicyPayload::empty(SESSION, "12".repeat(32))
        }
    }

    #[test]
    fn builder_emits_exact_envelope_and_round_trips() {
        let payload = policy();
        let event = build_coding_session_policy(CHANNEL, payload.clone())
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");

        assert_eq!(parse_coding_session_policy(&event).expect("parse"), payload);
        let tags: Vec<Vec<String>> = event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect();
        assert_eq!(tags.len(), 4);
        assert_eq!(tags[0], ["h", CHANNEL]);
        assert_eq!(tags[1], ["d", SESSION]);
        assert_eq!(tags[2], ["csp-v", CODING_SESSION_POLICY_SCHEMA]);
        assert_eq!(tags[3], ["csp-genesis", &"12".repeat(32)]);
        assert!(
            !event.content.contains("null"),
            "unset policy keys are omitted, never written as null: {}",
            event.content
        );
    }

    /// The withdrawal record builds too: a policy nobody can take back is a
    /// policy that outlives the decision it was made for.
    #[test]
    fn the_empty_withdrawal_record_builds() {
        let payload = CodingSessionPolicyPayload::empty(SESSION, "12".repeat(32));
        let event = build_coding_session_policy(CHANNEL, payload.clone())
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");
        assert_eq!(parse_coding_session_policy(&event).expect("parse"), payload);
        assert!(!payload.sets_any_policy());
    }

    /// **REVIEW-B1 F5.** The builder must refuse everything its own parser
    /// refuses. It used to write `channel_ref` into the `h` tag unchecked, so
    /// a non-UUID channel produced a signed event that
    /// `parse_coding_session_policy` — and the relay — both reject.
    #[test]
    fn the_builder_refuses_a_channel_its_own_parser_would_reject() {
        for bad_channel in [
            "not-a-uuid",
            "",
            "E0D3F1B8-8C66-4C62-9EF1-3FA933B32F86",
            "{e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86}",
            "e0d3f1b88c664c629ef13fa933b32f86",
        ] {
            assert!(
                build_coding_session_policy(bad_channel, policy()).is_err(),
                "{bad_channel:?} must never reach a signer"
            );
        }

        // And every channel the builder accepts round-trips through the
        // parser, so builder and parser agree by construction.
        let event = build_coding_session_policy(CHANNEL, policy())
            .expect("a canonical channel builds")
            .sign_with_keys(&Keys::generate())
            .expect("sign");
        assert!(parse_coding_session_policy(&event).is_ok());
    }

    /// The builder refuses invalid input before a signer ever sees bytes.
    #[test]
    fn an_invalid_policy_never_reaches_a_signer() {
        let mut payload = policy();
        payload.session_ref = "NOT-A-UUID".to_owned();
        assert!(build_coding_session_policy(CHANNEL, payload).is_err());

        let mut zero_budget = policy();
        zero_budget.budget = Some(CodingSessionPolicyBudget {
            turns: Some(0),
            tokens_per_seat: None,
            tokens_per_session: None,
            cost_usd_per_session: None,
            context_tier: None,
        });
        assert!(build_coding_session_policy(CHANNEL, zero_budget).is_err());
    }
}
