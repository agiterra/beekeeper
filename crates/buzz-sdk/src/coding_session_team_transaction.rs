//! Typed builders and parsers for NIP-CSTX team transactions (kind 44244).

use buzz_core::coding_session_team_transaction::{
    decode_coding_session_team_transaction, validate_coding_session_team_transaction_envelope,
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use buzz_core::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{Event, EventBuilder, Kind, Tag};

use crate::SdkError;

/// Build a structurally validated kind 44244 team transaction.
///
/// The builder emits the exact five-tag envelope required by NIP-CSTX. The
/// caller remains responsible for signing and publishing the event.
pub fn build_coding_session_team_transaction(
    channel_ref: &str,
    payload: CodingSessionTeamTransactionPayload,
) -> Result<EventBuilder, SdkError> {
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(&payload).map_err(|error| {
        SdkError::InvalidInput(format!("payload serialization failed: {error}"))
    })?;
    // Exercise the strict decoder before returning bytes to a signer. This
    // catches exact-key regressions that serde's typed serialization alone
    // would otherwise conceal.
    decode_coding_session_team_transaction(&content).map_err(SdkError::InvalidInput)?;

    let tags = [
        ["h", channel_ref],
        ["d", payload.session_ref.as_str()],
        ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
        ["cstx-genesis", payload.genesis_ref.as_str()],
        ["cstx-type", payload.transaction_type.as_str()],
    ]
    .into_iter()
    .map(|parts| Tag::parse(parts).map_err(|error| SdkError::InvalidTag(error.to_string())))
    .collect::<Result<Vec<_>, _>>()?;

    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        content,
    )
    .tags(tags))
}

/// Construct the common transaction payload around one typed operation body.
pub fn coding_session_team_transaction_payload(
    session_ref: impl Into<String>,
    genesis_ref: impl Into<String>,
    supersedes: Option<String>,
    delivery_command_id: Option<String>,
    body: CodingSessionTeamTransactionBody,
) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.to_owned(),
        session_ref: session_ref.into(),
        genesis_ref: genesis_ref.into(),
        transaction_type: body.transaction_type(),
        supersedes,
        delivery_command_id,
        body,
    }
}

/// Parse and validate a signed kind 44244 event's exact envelope.
pub fn parse_coding_session_team_transaction(
    event: &Event,
) -> Result<CodingSessionTeamTransactionPayload, SdkError> {
    validate_coding_session_team_transaction_envelope(event).map_err(SdkError::InvalidInput)
}

#[cfg(test)]
mod tests {
    use nostr::Keys;
    use serde::Deserialize;
    use serde_json::Value;

    use super::*;

    #[derive(Deserialize)]
    struct Fixture {
        vectors: Vec<Vector>,
    }

    #[derive(Deserialize)]
    struct Vector {
        name: String,
        valid: bool,
        content: Value,
    }

    #[test]
    fn shared_schema_vectors_match_the_sdk_parser() {
        let fixture: Fixture = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../conformance/coding-session-team-transaction/fixtures/schema-vectors.json"
        )))
        .expect("shared fixture must decode");

        for vector in fixture.vectors {
            let content = serde_json::to_string(&vector.content).expect("fixture JSON");
            assert_eq!(
                decode_coding_session_team_transaction(&content).is_ok(),
                vector.valid,
                "shared vector {}",
                vector.name
            );
        }
    }

    #[test]
    fn builder_emits_exact_envelope_and_round_trips() {
        let content = serde_json::json!({
            "schema": CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
            "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
            "genesisRef": "ab".repeat(32),
            "type": "assignment",
            "supersedes": null,
            "deliveryCommandId": "wake-builder-1",
            "body": {
                "assigneeActor": "cd".repeat(32),
                "assigneeRole": "builder",
                "objective": "Implement transport",
                "brief": "Build and test the transport slice.",
                "branch": null,
                "baseSha": null,
                "fileOwnership": ["crates/buzz-sdk"],
                "acceptanceSteps": ["cargo test -p buzz-sdk"]
            }
        });
        let payload: CodingSessionTeamTransactionPayload =
            serde_json::from_value(content).expect("typed fixture");
        let event = build_coding_session_team_transaction(
            "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86",
            payload.clone(),
        )
        .expect("builder")
        .sign_with_keys(&Keys::generate())
        .expect("sign");

        assert_eq!(
            parse_coding_session_team_transaction(&event).expect("parse"),
            payload
        );
        let tags: Vec<Vec<String>> = event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect();
        assert_eq!(tags.len(), 5);
        assert_eq!(tags[0], ["h", "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86"]);
        assert_eq!(tags[1], ["d", "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"]);
    }

    #[test]
    fn builder_signs_the_two_state_free_verbs_and_tags_their_exact_types() {
        for (expected_type, body) in [
            (
                "note",
                serde_json::json!({
                    "text": "Lane B is rebasing; nothing is blocked.",
                    "refs": ["11".repeat(32)],
                }),
            ),
            (
                "decision.request",
                serde_json::json!({
                    "question": "Ship the CLI fix now, or after the app rebuild?",
                    "options": ["now", "after the rebuild"],
                    "heldOn": "founder",
                    "blocks": ["11".repeat(32)],
                    "recommendation": null,
                }),
            ),
            (
                "decision.answer",
                serde_json::json!({
                    "requestRef": "22".repeat(32),
                    "choice": 1,
                    "note": null,
                }),
            ),
        ] {
            let content = serde_json::json!({
                "schema": CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
                "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
                "genesisRef": "ab".repeat(32),
                "type": expected_type,
                "supersedes": null,
                "deliveryCommandId": null,
                "body": body,
            });
            let payload: CodingSessionTeamTransactionPayload =
                serde_json::from_value(content).expect("typed fixture");
            let event = build_coding_session_team_transaction(
                "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86",
                payload.clone(),
            )
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");

            assert_eq!(
                parse_coding_session_team_transaction(&event).expect("parse"),
                payload
            );
            let tags: Vec<Vec<String>> = event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect();
            assert_eq!(tags.len(), 5);
            assert_eq!(tags[4], ["cstx-type", expected_type]);
        }
    }

    #[test]
    fn builder_refuses_a_note_that_supersedes_and_a_terminal_that_clears_itself() {
        let refuse = |transaction_type: &str, supersedes: Value, body: Value| {
            let content = serde_json::json!({
                "schema": CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
                "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
                "genesisRef": "ab".repeat(32),
                "type": transaction_type,
                "supersedes": supersedes,
                "deliveryCommandId": null,
                "body": body,
            });
            let payload: CodingSessionTeamTransactionPayload =
                serde_json::from_value(content).expect("typed fixture");
            build_coding_session_team_transaction("e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86", payload)
                .expect_err("builder must refuse")
                .to_string()
        };

        assert!(refuse(
            "note",
            Value::String("33".repeat(32)),
            serde_json::json!({"text": "said", "refs": []}),
        )
        .contains("a note never supersedes another record"));
        assert!(refuse(
            "mission.blocked",
            Value::String("44".repeat(32)),
            serde_json::json!({
                "assignmentRefs": [],
                "summary": "Nothing is blocked; work resumed.",
                "blockers": [],
                "heldOn": null,
                "requiredAction": "None.",
            }),
        )
        .contains("a terminal cannot clear itself"));
    }
}
