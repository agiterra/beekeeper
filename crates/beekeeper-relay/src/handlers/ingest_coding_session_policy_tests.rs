use beekeeper_core::coding_session_policy::{
    validate_coding_session_policy_envelope, CodingSessionAttention, CodingSessionPolicyGates,
    CodingSessionPolicyPayload, CodingSessionPosture,
};
use beekeeper_sdk::coding_session_policy::build_coding_session_policy;
use nostr::EventBuilder;

use super::*;

fn policy(session: &str, genesis: &str) -> CodingSessionPolicyPayload {
    CodingSessionPolicyPayload {
        posture: Some(CodingSessionPosture::Ship),
        attention: Some(CodingSessionAttention::Decisions),
        gates: Some(CodingSessionPolicyGates {
            red_first: Some(true),
            review_every_lane: None,
            required_gates: Some(vec!["just ci".to_owned()]),
            verifier_required: None,
        }),
        ..CodingSessionPolicyPayload::empty(session, genesis)
    }
}

/// The relay validates the policy record's structure and admits it under the
/// same strict coding-session membership gate every other session kind uses.
/// It deliberately does **not** decide authority: whether the signer held the
/// standing to set policy is the consuming fold's question against the
/// accepted NIP-CSAT chain, exactly as it is for kind 44244.
#[test]
fn policy_uses_core_envelope_and_strict_membership_admission() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "ab".repeat(32);
    let valid = build_coding_session_policy(&channel, policy(&session, &genesis))
        .expect("policy builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign policy");
    assert!(validate_coding_session_policy_envelope(&valid).is_ok());

    let malformed = EventBuilder::new(valid.kind, valid.content.clone())
        .tags(
            valid
                .tags
                .iter()
                .cloned()
                .chain([nostr::Tag::parse(["unexpected", "tag"]).expect("extra tag")]),
        )
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign malformed policy");
    assert!(validate_coding_session_policy_envelope(&malformed).is_err());

    assert_eq!(
        required_scope_for_kind(KIND_CODING_SESSION_POLICY, &valid).unwrap(),
        Scope::MessagesWrite
    );
    assert!(requires_h_channel_scope(KIND_CODING_SESSION_POLICY));
    assert!(is_coding_session_kind(KIND_CODING_SESSION_POLICY));
    assert!(requires_strict_coding_session_membership(
        KIND_CODING_SESSION_POLICY
    ));
    assert!(coding_session_membership_verdict(true).is_ok());
    assert!(coding_session_membership_verdict(false).is_err());
}

/// The relay's structural gate is the same one `buzz-core` exposes, so a
/// payload the core decoder refuses cannot be stored by the relay either.
/// Unknown fields are the case that matters: v1 rejects rather than ignores.
#[test]
fn a_policy_carrying_an_unknown_field_is_refused_by_the_relay_gate() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "ab".repeat(32);
    let valid = build_coding_session_policy(&channel, policy(&session, &genesis))
        .expect("policy builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign policy");

    let mut content: serde_json::Value =
        serde_json::from_str(&valid.content).expect("policy content");
    content
        .as_object_mut()
        .expect("policy object")
        .insert("cadence".to_owned(), serde_json::json!("hourly"));
    let unknown_field = EventBuilder::new(valid.kind, content.to_string())
        .tags(valid.tags.iter().cloned())
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign policy with an unknown field");
    let error = validate_coding_session_policy_envelope(&unknown_field)
        .expect_err("an unknown field is refused");
    assert!(error.contains("cadence"), "{error}");
}
