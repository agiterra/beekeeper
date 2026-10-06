//! The relay's half of kind 44247: scope, membership, and structure only.

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_handover::{
    validate_coding_session_handover_envelope, CodingSessionHandoverArtifact,
    CodingSessionHandoverArtifactKind, CodingSessionHandoverBody, CodingSessionHandoverCheckpoint,
    CodingSessionHandoverContinuation, CodingSessionHandoverMode, CodingSessionHandoverPayload,
    CodingSessionHandoverPreserved, CodingSessionHandoverRevision, CodingSessionHandoverType,
    CODING_SESSION_HANDOVER_SCHEMA,
};
use beekeeper_core::kind::KIND_SYSTEM_MESSAGE;
use beekeeper_sdk::coding_session_handover::build_coding_session_handover;
use nostr::EventBuilder;

use super::*;

fn checkpoint(session: &str, genesis: &str) -> CodingSessionHandoverPayload {
    CodingSessionHandoverPayload {
        schema: CODING_SESSION_HANDOVER_SCHEMA.to_owned(),
        session_ref: session.to_owned(),
        genesis_ref: genesis.to_owned(),
        handover_type: CodingSessionHandoverType::Checkpoint,
        body: CodingSessionHandoverBody::Checkpoint(CodingSessionHandoverCheckpoint {
            // The relay never reads this — supersession is the fold's rule —
            // but the key is part of the envelope it validates.
            prev_checkpoint_ref: None,
            task: "Land the handover envelope".to_owned(),
            assignment_refs: Vec::new(),
            decisions: Vec::new(),
            revision: CodingSessionHandoverRevision {
                repo_ref: Some("30617:aa/beekeeper".to_owned()),
                base_sha: None,
                head_sha: Some("2b".repeat(20)),
                branch: Some("work/handover".to_owned()),
                dirty: true,
                preserved: CodingSessionHandoverPreserved::Partial,
            },
            artifacts: vec![CodingSessionHandoverArtifact {
                kind: CodingSessionHandoverArtifactKind::WipRef,
                repo_ref: "30617:aa/beekeeper".to_owned(),
                r#ref: Some("refs/heads/wip/builder/1f2e3d4c".to_owned()),
                sha: Some("2b".repeat(20)),
                event_id: None,
                hash: None,
                base_sha: None,
                bytes: None,
            }],
            tests: Vec::new(),
            unresolved: Vec::new(),
            next_action: "Fetch the wip ref and continue".to_owned(),
            missing: vec!["uncommitted changes above the patch bound".to_owned()],
        }),
    }
}

fn continuation(session: &str, genesis: &str) -> CodingSessionHandoverPayload {
    CodingSessionHandoverPayload {
        schema: CODING_SESSION_HANDOVER_SCHEMA.to_owned(),
        session_ref: session.to_owned(),
        genesis_ref: genesis.to_owned(),
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
            recovered: Vec::new(),
            missing: Vec::new(),
            note: None,
        }),
    }
}

/// The relay knows kind 44247, admits it under the same strict coding-session
/// membership gate every other session kind uses, and checks its structure
/// through `buzz-core`'s own envelope validator.
///
/// It deliberately does **not** decide standing. The signature below is a
/// generated key that was never granted anything on this umbrella, and the
/// structure still passes: whether that author could checkpoint — or continue
/// — is the consuming fold's question against the accepted NIP-CSAT chain,
/// exactly as it is for kinds 44244, 44245 and 44246.
#[test]
fn handover_uses_core_envelope_and_strict_membership_admission() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "cd".repeat(32);
    let valid = build_coding_session_handover(&channel, checkpoint(&session, &genesis))
        .expect("handover builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign handover");
    assert!(validate_coding_session_handover_envelope(&valid).is_ok());

    assert_eq!(
        required_scope_for_kind(KIND_CODING_SESSION_HANDOVER, &valid).unwrap(),
        Scope::MessagesWrite
    );
    assert!(requires_h_channel_scope(KIND_CODING_SESSION_HANDOVER));
    assert!(is_coding_session_kind(KIND_CODING_SESSION_HANDOVER));
    assert!(requires_strict_coding_session_membership(
        KIND_CODING_SESSION_HANDOVER
    ));
}

/// Membership is answered before the body is ever parsed.
#[test]
fn a_non_member_is_refused_before_the_handover_is_parsed() {
    assert!(requires_strict_coding_session_membership(
        KIND_CODING_SESSION_HANDOVER
    ));
    assert!(coding_session_membership_verdict(true).is_ok());
    let message =
        coding_session_membership_verdict(false).expect_err("a non-member handover is refused");
    assert!(
        message.starts_with("restricted:") && !message.contains("invalid:"),
        "a non-member refusal must read as a standing refusal, not a structural one: {message}"
    );
}

/// A malformed record is refused by the relay's gate, with `buzz-core`'s own
/// sentence rather than a second implementation's paraphrase.
#[test]
fn a_malformed_handover_is_refused_by_the_relay_gate() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "cd".repeat(32);
    let valid = build_coding_session_handover(&channel, checkpoint(&session, &genesis))
        .expect("handover builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign handover");

    let mut content: serde_json::Value =
        serde_json::from_str(&valid.content).expect("handover content");
    content
        .as_object_mut()
        .expect("handover object")
        .insert("cadence".to_owned(), serde_json::json!("hourly"));
    let unknown_key = EventBuilder::new(valid.kind, content.to_string())
        .tags(valid.tags.iter().cloned())
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign handover with an unknown key");
    let error = validate_coding_session_handover_envelope(&unknown_key)
        .expect_err("an unknown key is refused");
    assert!(error.contains("cadence"), "{error}");

    let mut crossed: Vec<nostr::Tag> = valid.tags.iter().cloned().collect();
    crossed[4] = nostr::Tag::parse(["csh-type", "continuation"]).expect("crossed type tag");
    let crossed_type = EventBuilder::new(valid.kind, valid.content.clone())
        .tags(crossed)
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign handover with a crossed type tag");
    let error = validate_coding_session_handover_envelope(&crossed_type)
        .expect_err("a csh-type tag that disagrees with the content is refused");
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
        .expect("sign handover with a sixth tag");
    let error =
        validate_coding_session_handover_envelope(&sixth_tag).expect_err("a sixth tag is refused");
    assert!(error.contains("exactly five two-field tags"), "{error}");
}

/// Both record types travel the same envelope, and a continuation naming a
/// claim this relay knows nothing about is still structurally fine — standing
/// is not the relay's question.
#[test]
fn a_continuation_passes_structure_without_the_relay_resolving_its_claim() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "cd".repeat(32);
    let event = build_coding_session_handover(&channel, continuation(&session, &genesis))
        .expect("handover builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign handover");
    let payload = validate_coding_session_handover_envelope(&event).expect("valid envelope");
    assert_eq!(payload.handover_type.as_str(), "continuation");
}

/// The two new authority refusals reach the wire as their own sentences, each
/// naming what the publisher should do instead. A refusal a client cannot act
/// on is barely a refusal.
#[test]
fn the_claim_refusals_have_their_own_wire_sentences() {
    let claimant_not_signer = coding_session_authority_transition_refusal_result(
        "ab".repeat(32),
        &beekeeper_db::AuthorityTransitionRefusal::ClaimantNotSigner,
    );
    assert!(!claimant_not_signer.accepted);
    assert!(
        claimant_not_signer.message.contains("self-claim")
            && claimant_not_signer.message.contains("transfer"),
        "{}",
        claimant_not_signer.message
    );

    let no_claim = coding_session_authority_transition_refusal_result(
        "ab".repeat(32),
        &beekeeper_db::AuthorityTransitionRefusal::NoActiveClaim,
    );
    assert!(!no_claim.accepted);
    assert!(
        no_claim.message.contains("no claim to transfer") && no_claim.message.contains("takeover"),
        "{}",
        no_claim.message
    );

    // The legacy refusals are untouched: every one still says exactly what it
    // said before claims existed.
    let not_owner = coding_session_authority_transition_refusal_result(
        "ab".repeat(32),
        &beekeeper_db::AuthorityTransitionRefusal::SignerNotOwner {
            owner_pubkey: vec![0x11; 32],
        },
    );
    assert!(not_owner
        .message
        .starts_with("invalid: signer is not the session's current owner"));
}

/// Review finding N8: a client may not publish kind 40099.
///
/// The two receipts this work added — `coding_session_authority_transition_accepted`
/// and `coding_session_deletion_accepted` — are the relay's own word, and the
/// second one tells every provider to stop working on a session. Every reader
/// in this tree already checks the signer against the witnessed relay identity,
/// so a forgery was never *believed*; it was, until now, *storable* and
/// renderable as a system row.
#[test]
fn a_client_submitted_system_message_is_refused_as_a_relay_only_kind() {
    let channel = Uuid::new_v4().to_string();
    let forged = nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        serde_json::json!({
            "type": "coding_session_deletion_accepted",
            "genesisRef": "ab".repeat(32),
            "sessionRef": Uuid::new_v4().to_string(),
            "deletionEventId": "cd".repeat(32),
            "channelId": channel,
        })
        .to_string(),
    )
    .tags(vec![nostr::Tag::parse(["h", &channel]).expect("h tag")])
    .sign_with_keys(&nostr::Keys::generate())
    .expect("sign a forged receipt");

    let error = refuse_relay_only_kind(event_kind_u32(&forged)).expect_err("must be refused");
    match error {
        IngestError::Rejected(message) => {
            assert_eq!(message, "restricted: relay-only kind");
            assert!(
                message.starts_with("restricted:"),
                "the refusal is about who may author the kind, not about its JSON"
            );
        }
        other => panic!("a relay-only kind must be a plain rejection, got {other:?}"),
    }
}

/// The gate refuses the **kind**, not a signer — including the relay's own
/// key — and that is safe because the relay never submits its receipts here.
///
/// `side_effects::emit_system_message` signs with the relay keypair and writes
/// straight through `db.insert_event` plus a pubsub fan-out, so nothing this
/// relay publishes passes through `refuse_relay_only_kind` at all. The kinds a
/// client legitimately writes into the same channel — including 44228 claims
/// and 44247 handover records — stay submittable.
#[test]
fn the_relay_only_gate_narrows_to_40099_and_admits_every_client_kind_beside_it() {
    assert!(refuse_relay_only_kind(KIND_SYSTEM_MESSAGE).is_err());
    for admitted in [
        KIND_CODING_SESSION_HANDOVER,
        KIND_CODING_SESSION_AUTHORITY_TRANSITION,
        KIND_CODING_SESSION_OBSERVATION,
        KIND_CODING_SESSION_GENESIS,
        beekeeper_core::kind::KIND_DELETION,
    ] {
        assert!(
            refuse_relay_only_kind(admitted).is_ok(),
            "kind {admitted} is client-authored and must stay submittable"
        );
    }
}
