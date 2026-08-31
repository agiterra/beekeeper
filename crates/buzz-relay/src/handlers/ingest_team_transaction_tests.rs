use buzz_core::coding_session_team_transaction::{
    validate_coding_session_team_transaction_envelope, CodingSessionTeamAssignment,
    CodingSessionTeamTransactionBody,
};
use buzz_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};
use nostr::EventBuilder;

use super::*;

#[test]
fn team_transaction_uses_core_envelope_and_strict_membership_admission() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "ab".repeat(32);
    let payload = coding_session_team_transaction_payload(
        &session,
        &genesis,
        None,
        None,
        CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
            assignee_actor: "cd".repeat(32),
            assignee_role: "builder".into(),
            objective: "Prove the relay stores the signed record".into(),
            brief: "Exercise the shared kind 44244 validator before persistence.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["crates/buzz-relay".into()],
            acceptance_steps: vec!["cargo test -p buzz-relay coding_session".into()],
        }),
    );
    let valid = build_coding_session_team_transaction(&channel, payload)
        .expect("team transaction builder")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign team transaction");
    assert!(validate_coding_session_team_transaction_envelope(&valid).is_ok());

    let malformed = EventBuilder::new(valid.kind, valid.content.clone())
        .tags(
            valid
                .tags
                .iter()
                .cloned()
                .chain([nostr::Tag::parse(["unexpected", "tag"]).expect("extra tag")]),
        )
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign malformed transaction");
    assert!(validate_coding_session_team_transaction_envelope(&malformed).is_err());
    assert_eq!(
        required_scope_for_kind(KIND_CODING_SESSION_TEAM_TRANSACTION, &valid).unwrap(),
        Scope::MessagesWrite
    );
    assert!(requires_h_channel_scope(
        KIND_CODING_SESSION_TEAM_TRANSACTION
    ));
    assert!(coding_session_membership_verdict(true).is_ok());
    assert!(coding_session_membership_verdict(false).is_err());
}
