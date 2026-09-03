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

/// The relay's 44244 arm delegates every structural rule to the shared core
/// validator, so a new operation type is admitted or refused there and nowhere
/// else. This pins that the two verbs added on 2026-09-01 actually reach it:
/// a relay that stored an unvalidated `note` or `decision.*` would be storing a
/// shape no reader in the fleet can trust.
#[test]
fn the_state_free_verbs_are_structurally_validated_at_ingest() {
    let channel = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let genesis = "ab".repeat(32);
    let keys = nostr::Keys::generate();

    let sign = |transaction_type: &str, supersedes: serde_json::Value, body: serde_json::Value| {
        let content = serde_json::json!({
            "schema": buzz_core::coding_session_team_transaction::CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
            "sessionRef": session,
            "genesisRef": genesis,
            "type": transaction_type,
            "supersedes": supersedes,
            "deliveryCommandId": null,
            "body": body,
        })
        .to_string();
        EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
            content,
        )
        .tags([
            nostr::Tag::parse(["h", channel.as_str()]).expect("h"),
            nostr::Tag::parse(["d", session.as_str()]).expect("d"),
            nostr::Tag::parse([
                "cstx-v",
                buzz_core::coding_session_team_transaction::CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
            ])
            .expect("version"),
            nostr::Tag::parse(["cstx-genesis", genesis.as_str()]).expect("genesis"),
            nostr::Tag::parse(["cstx-type", transaction_type]).expect("type"),
        ])
        .sign_with_keys(&keys)
        .expect("sign")
    };

    let accepted = [
        (
            "note",
            serde_json::json!({"text": "nothing is blocked", "refs": []}),
        ),
        (
            "decision.request",
            serde_json::json!({
                "question": "ship now or after the rebuild?",
                "options": ["now", "after"],
                "heldOn": "founder",
                "blocks": [],
                "recommendation": null,
            }),
        ),
        (
            "decision.answer",
            // The shape signed before `condition` existed. Untouched by this
            // lane: it decodes exactly as it always did, which is the point of
            // "optional on read" (NIP-CSTX, item I).
            serde_json::json!({"requestRef": "22".repeat(32), "choice": 1, "note": null}),
        ),
        (
            // And a writer's shape, which every client emits from 2026-09-02.
            // Ingest must admit both, or the relay refuses either the records
            // it already stores or the ones it is about to be sent.
            "decision.answer",
            serde_json::json!({
                "requestRef": "22".repeat(32),
                "choice": 1,
                "note": null,
                "condition": "any SHA whose buzz-acp diff against origin/main is empty",
            }),
        ),
    ];
    for (transaction_type, body) in &accepted {
        let event = sign(transaction_type, serde_json::Value::Null, body.clone());
        assert!(
            validate_coding_session_team_transaction_envelope(&event).is_ok(),
            "{transaction_type} must be admitted"
        );
        assert_eq!(
            required_scope_for_kind(KIND_CODING_SESSION_TEAM_TRANSACTION, &event).unwrap(),
            Scope::MessagesWrite
        );
    }

    let refused = [
        // Unknown key in a note body.
        (
            "note",
            serde_json::Value::Null,
            serde_json::json!({"text": "said", "refs": [], "extra": 1}),
        ),
        // A note that claims to correct something.
        (
            "note",
            serde_json::Value::String("33".repeat(32)),
            serde_json::json!({"text": "said", "refs": []}),
        ),
        // heldOn is neither `founder` nor a 64-hex actor.
        (
            "decision.request",
            serde_json::Value::Null,
            serde_json::json!({
                "question": "ship?",
                "options": [],
                "heldOn": "lead",
                "blocks": [],
                "recommendation": null,
            }),
        ),
        // An option index no request could ever carry.
        (
            "decision.answer",
            serde_json::Value::Null,
            serde_json::json!({
                "requestRef": "22".repeat(32),
                "choice": 99,
                "note": null,
                "condition": null,
            }),
        ),
        // Keystone's shape: a terminal correcting itself to empty.
        (
            "mission.blocked",
            serde_json::Value::String("44".repeat(32)),
            serde_json::json!({
                "assignmentRefs": [],
                "summary": "Nothing is blocked; work resumed.",
                "blockers": [],
                "heldOn": null,
                "requiredAction": "None.",
            }),
        ),
    ];
    for (transaction_type, supersedes, body) in refused {
        let event = sign(transaction_type, supersedes, body.clone());
        assert!(
            validate_coding_session_team_transaction_envelope(&event).is_err(),
            "{transaction_type} must be refused: {body}"
        );
    }
}
