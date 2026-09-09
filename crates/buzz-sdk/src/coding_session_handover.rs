//! Typed builders and parser for NIP-CSH handover records (kind 44247) and
//! for the two claim links they are read against (kind 44228
//! `takeover`/`transfer`).
//!
//! Everything the parser refuses, the builder refuses first, so no author ever
//! signs bytes the relay and the reader will both reject.

use buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload;
use buzz_core::coding_session_handover::{
    decode_coding_session_handover, validate_coding_session_handover_envelope,
    CodingSessionHandoverPayload, CODING_SESSION_HANDOVER_TAG_VERSION,
};
use buzz_core::kind::KIND_CODING_SESSION_HANDOVER;
use nostr::{Event, EventBuilder, Kind, Tag};
use uuid::Uuid;

use crate::builders::build_coding_session_authority_transition;
use crate::SdkError;

/// Build a structurally validated kind 44247 handover record.
///
/// Emits the exact five-tag envelope NIP-CSH requires — `h`, `d`, `csh-v`,
/// `csh-genesis`, `csh-type`, in that order — and leaves signing and
/// publishing to the caller. It does **not** check standing: the signature is
/// the author, and whether that author was the founder, an operator, a seat or
/// the claimant is the consuming fold's question against the accepted NIP-CSAT
/// chain, exactly as for kinds 44244, 44245 and 44246.
///
/// # Errors
/// [`SdkError::InvalidInput`] naming the field the payload got wrong, or the
/// channel reference when it is not a canonical lowercase UUID.
pub fn build_coding_session_handover(
    channel_ref: &str,
    payload: CodingSessionHandoverPayload,
) -> Result<EventBuilder, SdkError> {
    validate_channel_ref(channel_ref)?;
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(&payload).map_err(|error| {
        SdkError::InvalidInput(format!("payload serialization failed: {error}"))
    })?;
    // Exercise the strict decoder before returning bytes to a signer: a typed
    // serialization alone would conceal an exact-key regression until the
    // event was already signed.
    decode_coding_session_handover(&content).map_err(SdkError::InvalidInput)?;

    let tags = [
        ["h", channel_ref],
        ["d", payload.session_ref.as_str()],
        ["csh-v", CODING_SESSION_HANDOVER_TAG_VERSION],
        ["csh-genesis", payload.genesis_ref.as_str()],
        ["csh-type", payload.handover_type.as_str()],
    ]
    .into_iter()
    .map(|parts| Tag::parse(parts).map_err(|error| SdkError::InvalidTag(error.to_string())))
    .collect::<Result<Vec<_>, _>>()?;

    Ok(EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_HANDOVER as u16), content).tags(tags))
}

/// Build a kind 44228 `takeover`: a self-claim of one session, on one body.
///
/// `claimant` must be the pubkey that will sign — the relay refuses a takeover
/// whose grantee is anyone else, so a builder that let one through would mint
/// bytes guaranteed to be refused.
///
/// # Errors
/// [`SdkError::InvalidInput`] naming the malformed field.
pub fn build_coding_session_takeover(
    channel_id: Uuid,
    genesis_ref: &str,
    prev_accepted: Option<String>,
    seq: u32,
    claimant: &str,
    body_pubkey: &str,
) -> Result<EventBuilder, SdkError> {
    build_coding_session_authority_transition(
        channel_id,
        &CodingSessionAuthorityTransitionPayload::new_takeover(
            genesis_ref.to_owned(),
            prev_accepted,
            seq,
            claimant.to_owned(),
            body_pubkey.to_owned(),
        ),
    )
}

/// Build a kind 44228 `transfer`: hand the claim to `claimant`, on `body_pubkey`.
///
/// The signer must be the current claimant or the founder; that is the relay's
/// check against the accepted chain, which no builder can make locally.
///
/// # Errors
/// [`SdkError::InvalidInput`] naming the malformed field.
pub fn build_coding_session_transfer(
    channel_id: Uuid,
    genesis_ref: &str,
    prev_accepted: Option<String>,
    seq: u32,
    claimant: &str,
    body_pubkey: &str,
) -> Result<EventBuilder, SdkError> {
    build_coding_session_authority_transition(
        channel_id,
        &CodingSessionAuthorityTransitionPayload::new_transfer(
            genesis_ref.to_owned(),
            prev_accepted,
            seq,
            claimant.to_owned(),
            body_pubkey.to_owned(),
        ),
    )
}

/// Parse and validate a signed kind 44247 event's exact envelope.
///
/// # Errors
/// [`SdkError::InvalidInput`] naming the tag or field that disagrees.
pub fn parse_coding_session_handover(
    event: &Event,
) -> Result<CodingSessionHandoverPayload, SdkError> {
    validate_coding_session_handover_envelope(event).map_err(SdkError::InvalidInput)
}

/// Check `channel_ref` against the exact rule the envelope validator applies
/// to the `h` tag: a canonical lowercase hyphenated UUID.
fn validate_channel_ref(channel_ref: &str) -> Result<(), SdkError> {
    let parsed = Uuid::parse_str(channel_ref)
        .map_err(|_| SdkError::InvalidInput("h must be a UUID".to_owned()))?;
    if parsed.to_string() != channel_ref {
        return Err(SdkError::InvalidInput(
            "h must be a lowercase canonical UUID".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use buzz_core::coding_session_authority_transition::{
        decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
    };
    use buzz_core::coding_session_command::CodingSessionTarget;
    use buzz_core::coding_session_handover::{
        CodingSessionHandoverBody, CodingSessionHandoverCheckpoint,
        CodingSessionHandoverContinuation, CodingSessionHandoverMode,
        CodingSessionHandoverPreserved, CodingSessionHandoverRevision, CodingSessionHandoverType,
        CODING_SESSION_HANDOVER_SCHEMA,
    };
    use nostr::Keys;

    use super::*;

    const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
    const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";

    fn checkpoint(prev_checkpoint_ref: Option<String>) -> CodingSessionHandoverPayload {
        CodingSessionHandoverPayload {
            schema: CODING_SESSION_HANDOVER_SCHEMA.to_owned(),
            session_ref: SESSION.to_owned(),
            genesis_ref: "12".repeat(32),
            handover_type: CodingSessionHandoverType::Checkpoint,
            body: CodingSessionHandoverBody::Checkpoint(CodingSessionHandoverCheckpoint {
                prev_checkpoint_ref,
                task: "Land the supersession rule".to_owned(),
                assignment_refs: Vec::new(),
                decisions: Vec::new(),
                revision: CodingSessionHandoverRevision {
                    repo_ref: None,
                    base_sha: None,
                    head_sha: None,
                    branch: None,
                    dirty: false,
                    preserved: CodingSessionHandoverPreserved::All,
                },
                artifacts: Vec::new(),
                tests: Vec::new(),
                unresolved: Vec::new(),
                next_action: "Pin the fixture".to_owned(),
                missing: Vec::new(),
            }),
        }
    }

    fn continuation() -> CodingSessionHandoverPayload {
        CodingSessionHandoverPayload {
            schema: CODING_SESSION_HANDOVER_SCHEMA.to_owned(),
            session_ref: SESSION.to_owned(),
            genesis_ref: "12".repeat(32),
            handover_type: CodingSessionHandoverType::Continuation,
            body: CodingSessionHandoverBody::Continuation(CodingSessionHandoverContinuation {
                claim_ref: "aa".repeat(32),
                mode: CodingSessionHandoverMode::Reconstructed,
                checkpoint_ref: None,
                target: CodingSessionTarget {
                    driver: "claude-agent-acp".to_owned(),
                    instance_id: "provider-b".to_owned(),
                    session_id: "sess-b-1".to_owned(),
                    generation: 1,
                },
                recovered: vec!["wip-ref refs/heads/wip/builder/1f2e at 2b2b".to_owned()],
                missing: Vec::new(),
                note: None,
            }),
        }
    }

    #[test]
    fn the_handover_builder_emits_the_exact_envelope_and_round_trips() {
        let payload = continuation();
        let event = build_coding_session_handover(CHANNEL, payload.clone())
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");

        assert_eq!(
            parse_coding_session_handover(&event).expect("parse"),
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
        assert_eq!(tags[2], ["csh-v", "csh1"]);
        assert_eq!(tags[3], ["csh-genesis", &"12".repeat(32)]);
        assert_eq!(tags[4], ["csh-type", "continuation"]);
        // Absent is not omitted: an unset optional is on the wire as null.
        assert!(
            event.content.contains("\"checkpointRef\":null"),
            "{}",
            event.content
        );
    }

    #[test]
    fn the_handover_builder_refuses_what_its_own_parser_would_reject() {
        for bad_channel in [
            "not-a-uuid",
            "",
            "D3E440EA-89F8-4AEE-8A02-17EDC3E7272E",
            "d3e440ea89f84aee8a0217edc3e7272e",
        ] {
            assert!(
                build_coding_session_handover(bad_channel, continuation()).is_err(),
                "{bad_channel:?} must never reach a signer"
            );
        }

        let mut wrong_type = continuation();
        wrong_type.handover_type = CodingSessionHandoverType::Checkpoint;
        assert!(build_coding_session_handover(CHANNEL, wrong_type).is_err());

        let mut bad_claim = continuation();
        if let CodingSessionHandoverBody::Continuation(body) = &mut bad_claim.body {
            body.claim_ref = "not-hex".to_owned();
        }
        assert!(build_coding_session_handover(CHANNEL, bad_claim).is_err());
    }

    /// The supersession reference survives the builder untouched, and is on
    /// the wire as an explicit `null` when an author has nothing to replace —
    /// the key is always present, exactly like `prevAccepted` on a 44228 link.
    #[test]
    fn a_checkpoint_carries_its_prev_checkpoint_ref_and_writes_null_for_the_first() {
        let first = checkpoint(None);
        let event = build_coding_session_handover(CHANNEL, first.clone())
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");
        assert_eq!(parse_coding_session_handover(&event).expect("parse"), first);
        assert!(
            event.content.contains("\"prevCheckpointRef\":null"),
            "an author's first checkpoint says so with a null, never by omission: {}",
            event.content
        );

        let next = checkpoint(Some("7a".repeat(32)));
        let event = build_coding_session_handover(CHANNEL, next.clone())
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");
        assert_eq!(parse_coding_session_handover(&event).expect("parse"), next);
        assert!(event
            .content
            .contains(&format!("\"prevCheckpointRef\":\"{}\"", "7a".repeat(32))));

        // A malformed reference never reaches a signer.
        assert!(
            build_coding_session_handover(CHANNEL, checkpoint(Some("not-hex".to_owned()))).is_err()
        );
    }

    #[test]
    fn the_claim_builders_emit_the_44228_envelope_and_carry_the_body() {
        let channel = Uuid::parse_str(CHANNEL).expect("uuid");
        let genesis = "ab".repeat(32);
        let claimant = "cd".repeat(32);
        let body = "ef".repeat(32);

        for (event, expected) in [
            (
                build_coding_session_takeover(channel, &genesis, None, 1, &claimant, &body)
                    .expect("takeover")
                    .sign_with_keys(&Keys::generate())
                    .expect("sign"),
                CodingSessionAuthorityTransitionType::Takeover,
            ),
            (
                build_coding_session_transfer(
                    channel,
                    &genesis,
                    Some("11".repeat(32)),
                    2,
                    &claimant,
                    &body,
                )
                .expect("transfer")
                .sign_with_keys(&Keys::generate())
                .expect("sign"),
                CodingSessionAuthorityTransitionType::Transfer,
            ),
        ] {
            let tags: Vec<Vec<String>> = event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect();
            assert_eq!(tags.len(), 3);
            assert_eq!(tags[0], ["h", CHANNEL]);
            assert_eq!(tags[1], ["csat-v", "csat1-1"]);
            assert_eq!(tags[2], ["csat-genesis", &genesis]);
            let payload =
                decode_coding_session_authority_transition(&event.content).expect("decode");
            assert_eq!(payload.transition_type, expected);
            assert_eq!(payload.grantee_pubkey, claimant);
            assert_eq!(payload.body_pubkey.as_deref(), Some(body.as_str()));
            assert!(payload.role.is_none());
        }
    }

    #[test]
    fn a_claim_with_a_malformed_body_never_reaches_a_signer() {
        let channel = Uuid::parse_str(CHANNEL).expect("uuid");
        let genesis = "ab".repeat(32);
        let claimant = "cd".repeat(32);
        for bad_body in ["", "EF".repeat(32).as_str(), "ef".repeat(31).as_str()] {
            assert!(
                build_coding_session_takeover(channel, &genesis, None, 1, &claimant, bad_body)
                    .is_err(),
                "body {bad_body:?} must be refused"
            );
        }
        // seq/prevAccepted disagreement is refused by the shared payload rule.
        assert!(build_coding_session_takeover(
            channel,
            &genesis,
            Some("11".repeat(32)),
            1,
            &claimant,
            &"ef".repeat(32)
        )
        .is_err());
    }
}
