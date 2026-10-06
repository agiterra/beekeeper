//! Encode/decode cases for the two project-action transition types
//! (ledger 186). Split from the module's own `tests` because that block is
//! about the session links and this one is about the delegation's exact wire
//! shape.

use super::*;

fn hex(byte: &str) -> String {
    byte.repeat(32)
}

const PROJECT: &str =
    "30621:1111111111111111111111111111111111111111111111111111111111111111:kettle";

#[test]
fn a_grant_round_trips_with_exactly_six_keys() {
    let payload = CodingSessionAuthorityTransitionPayload::new_grant_project_actions(
        hex("ab"),
        Some(hex("11")),
        2,
        hex("cd"),
        PROJECT,
    );
    payload.validate().expect("valid");
    let content = serde_json::to_string(&payload).expect("serialize");
    assert_eq!(
        content,
        format!(
            r#"{{"genesisRef":"{}","prevAccepted":"{}","seq":2,"type":"grant-project-actions","granteePubkey":"{}","projectRef":"{PROJECT}"}}"#,
            hex("ab"),
            hex("11"),
            hex("cd"),
        )
    );
    assert_eq!(
        decode_coding_session_authority_transition(&content).expect("decode"),
        payload
    );
}

#[test]
fn a_revoke_round_trips_and_names_its_project() {
    let payload = CodingSessionAuthorityTransitionPayload::new_revoke_project_actions(
        hex("ab"),
        None,
        1,
        hex("cd"),
        PROJECT,
    );
    payload.validate().expect("valid");
    let content = serde_json::to_string(&payload).expect("serialize");
    let decoded = decode_coding_session_authority_transition(&content).expect("decode");
    assert_eq!(
        decoded.transition_type,
        CodingSessionAuthorityTransitionType::RevokeProjectActions
    );
    assert_eq!(decoded.project_ref.as_deref(), Some(PROJECT));
    assert_eq!(decoded.transition_type.as_str(), "revoke-project-actions");
    assert!(decoded.transition_type.is_project_actions());
}

#[test]
fn a_delegation_without_a_project_is_refused_on_write_and_on_read() {
    let mut payload = CodingSessionAuthorityTransitionPayload::new_grant_project_actions(
        hex("ab"),
        None,
        1,
        hex("cd"),
        PROJECT,
    );
    payload.project_ref = None;
    let error = payload.validate().expect_err("must refuse");
    assert!(error.contains("require projectRef"), "{error}");

    let content = format!(
        r#"{{"genesisRef":"{}","prevAccepted":null,"seq":1,"type":"grant-project-actions","granteePubkey":"{}"}}"#,
        hex("ab"),
        hex("cd"),
    );
    assert!(decode_coding_session_authority_transition(&content).is_err());
}

#[test]
fn no_other_transition_type_may_carry_a_project() {
    let mut payload =
        CodingSessionAuthorityTransitionPayload::new_grant_operator(hex("ab"), None, 1, hex("cd"));
    payload.project_ref = Some(PROJECT.to_owned());
    let error = payload.validate().expect_err("must refuse");
    assert!(error.contains("may carry projectRef"), "{error}");

    let content = format!(
        r#"{{"genesisRef":"{}","prevAccepted":null,"seq":1,"type":"grant-operator","granteePubkey":"{}","projectRef":"{PROJECT}"}}"#,
        hex("ab"),
        hex("cd"),
    );
    assert!(decode_coding_session_authority_transition(&content).is_err());
}

#[test]
fn a_delegation_may_not_carry_a_role_or_a_body() {
    let mut payload = CodingSessionAuthorityTransitionPayload::new_grant_project_actions(
        hex("ab"),
        None,
        1,
        hex("cd"),
        PROJECT,
    );
    payload.role = Some("lead".into());
    assert!(payload.validate().is_err());

    let mut payload = CodingSessionAuthorityTransitionPayload::new_grant_project_actions(
        hex("ab"),
        None,
        1,
        hex("cd"),
        PROJECT,
    );
    payload.body_pubkey = Some(hex("ee"));
    assert!(payload.validate().is_err());
}

#[test]
fn a_project_ref_that_is_not_a_project_coordinate_is_refused() {
    for bad in [
        "30617:1111111111111111111111111111111111111111111111111111111111111111:repo",
        "30621:NOTHEX:kettle",
        "30621:1111111111111111111111111111111111111111111111111111111111111111:",
        "kettle",
    ] {
        let payload = CodingSessionAuthorityTransitionPayload::new_grant_project_actions(
            hex("ab"),
            None,
            1,
            hex("cd"),
            bad,
        );
        assert!(payload.validate().is_err(), "accepted {bad:?}");
    }
}

#[test]
fn the_longest_allowed_delegation_still_fits_the_content_ceiling() {
    let project = format!(
        "30621:{}:{}",
        "1".repeat(64),
        "d".repeat(MAX_PROJECT_REF_BYTES - 71)
    );
    assert_eq!(project.len(), MAX_PROJECT_REF_BYTES);
    let payload = CodingSessionAuthorityTransitionPayload::new_grant_project_actions(
        hex("ab"),
        Some(hex("11")),
        u32::MAX,
        hex("cd"),
        project,
    );
    payload.validate().expect("valid");
    let content = serde_json::to_string(&payload).expect("serialize");
    assert!(
        content.len() <= MAX_AUTHORITY_TRANSITION_CONTENT_BYTES,
        "{} bytes",
        content.len()
    );
    assert!(decode_coding_session_authority_transition(&content).is_ok());
}

/// The shared cross-reader vectors, run against the canonical decoder every
/// other Rust reader ultimately calls.
///
/// One fixture, four readers (`conformance/authority-chain/README.md`): a
/// lane that adds a key to this wire adds a vector, and every reader's copy
/// of this test fails until that reader has been taught the key. Ledger 204
/// is what happens without it.
#[test]
fn shared_authority_chain_vectors_match_the_transition_decoder() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../conformance/authority-chain/fixtures/chain-vectors.json"
    ))
    .expect("fixture parses");
    assert_eq!(
        fixture["schema"],
        "buzz-coding-session-authority-chain-conformance/v1"
    );
    let vectors = fixture["vectors"].as_array().expect("vectors");
    assert!(!vectors.is_empty());
    let mut saw_project_actions = false;
    for vector in vectors {
        let name = vector["name"].as_str().expect("name");
        let expected = vector["transitionValid"]
            .as_bool()
            .expect("transitionValid");
        let content = serde_json::to_string(&vector["transition"]).expect("transition");
        assert_eq!(
            decode_coding_session_authority_transition(&content).is_ok(),
            expected,
            "shared authority-chain vector {name}"
        );
        if vector["transition"]["type"]
            .as_str()
            .is_some_and(|value| value.ends_with("-project-actions"))
        {
            saw_project_actions = true;
        }
    }
    assert!(
        saw_project_actions,
        "the shared set must cover the project-action types"
    );
}
