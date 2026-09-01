//! Tests for the two 44220/44221 envelope rules `bee sessions` must honour:
//! the relay's exactly-three-tags validator, and the delivery class that is
//! omitted at its default because a relay predating the field refuses it.

use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery, CodingSessionTarget,
    CODING_SESSION_COMMAND_SCHEMA,
};

use super::crew_cmds::{
    boundary_free_content, build_turn_command, refuse_unsupported_create_flags,
};
use crate::error::CliError;

const CHANNEL: &str = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const ALICE: &str = "11";

fn target(session_id: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "instance-1".into(),
        session_id: session_id.into(),
        generation,
    }
}

fn pk(seed: &str) -> String {
    seed.repeat(32)
}

fn usage_message(error: CliError) -> String {
    match error {
        CliError::Usage(message) | CliError::NotFound(message) => message,
        other => panic!("expected a usage/not-found error, got {other:?}"),
    }
}

// ── wire shape ───────────────────────────────────────────────────────────────
fn payload(text: &str, deliver: CodingSessionDelivery) -> CodingSessionCommandPayload {
    CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: "cmd-1".to_owned(),
        target: target("s-1", 2),
        action: CodingSessionAction::ThreadTurnStart {
            text: text.to_owned(),
            attachments: Vec::new(),
            deliver,
        },
    }
}

/// A relay built before `deliver` existed refuses any payload carrying it —
/// the payload is `deny_unknown_fields` — so the default class is omitted,
/// exactly as the desktop sender does.
#[test]
fn a_boundary_turn_omits_the_deliver_key() {
    let content =
        boundary_free_content(&payload("go", CodingSessionDelivery::Boundary)).expect("content");
    assert!(!content.contains("deliver"), "got {content}");
    let decoded: CodingSessionCommandPayload =
        serde_json::from_str(&content).expect("round-trips through the relay's own decoder");
    assert_eq!(decoded, payload("go", CodingSessionDelivery::Boundary));
}

/// The same omission rule covers attachments, and `boundary_free_content`'s
/// string surgery must survive them: it strips the one `deliver` key by
/// literal match, so a payload that now carries another optional key has to
/// still come out decodable.
#[test]
fn a_turn_without_images_carries_no_attachments_key() {
    let content =
        boundary_free_content(&payload("go", CodingSessionDelivery::Boundary)).expect("content");
    assert!(!content.contains("attachments"), "got {content}");
}

#[test]
fn a_turn_with_images_keeps_them_through_the_boundary_rewrite() {
    let mut with_images = payload("look at this", CodingSessionDelivery::Boundary);
    with_images.action = CodingSessionAction::ThreadTurnStart {
        text: "look at this".to_owned(),
        attachments: vec![buzz_core::coding_session_command::TurnAttachment {
            sha256: "aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd".to_owned(),
            mime: "image/png".to_owned(),
            size: 2048,
            dim: Some("800x600".to_owned()),
            filename: Some("shot.png".to_owned()),
        }],
        deliver: CodingSessionDelivery::Boundary,
    };
    let content = boundary_free_content(&with_images).expect("content");
    assert!(!content.contains("deliver"), "got {content}");
    let decoded: CodingSessionCommandPayload =
        serde_json::from_str(&content).expect("round-trips through the relay's own decoder");
    assert_eq!(decoded, with_images);
}

#[test]
fn a_steer_turn_keeps_the_deliver_key() {
    let content =
        boundary_free_content(&payload("go", CodingSessionDelivery::Steer)).expect("content");
    assert!(content.contains(r#""deliver":"steer""#), "got {content}");
}

/// The removal is a string edit, so prove it cannot be fooled by turn text
/// that spells the needle: JSON escapes every quote inside a string value.
#[test]
fn turn_text_that_spells_the_deliver_key_is_left_intact() {
    let hostile = r#"look for ,"deliver":"boundary" in the payload"#;
    let content =
        boundary_free_content(&payload(hostile, CodingSessionDelivery::Boundary)).expect("content");
    let decoded: CodingSessionCommandPayload = serde_json::from_str(&content).expect("decodes");
    match decoded.action {
        CodingSessionAction::ThreadTurnStart { text, deliver, .. } => {
            assert_eq!(text, hostile);
            assert_eq!(deliver, CodingSessionDelivery::Boundary);
        }
        other => panic!("expected a turn start, got {other:?}"),
    }
}

/// The relay's 44220 envelope validator accepts exactly one `h`, `cs-v`, and
/// `cs-target` tag; the boundary path rebuilds them, so pin that it rebuilds
/// them identically to the SDK.
#[test]
fn the_boundary_envelope_carries_the_same_tags_as_the_sdk_builder() {
    let channel = uuid::Uuid::parse_str(CHANNEL).expect("uuid");
    let keys = nostr::Keys::generate();
    let ours = build_turn_command(channel, &payload("go", CodingSessionDelivery::Boundary))
        .expect("builder")
        .sign_with_keys(&keys)
        .expect("sign");
    let sdk = buzz_sdk::builders::build_coding_session_command(
        channel,
        &payload("go", CodingSessionDelivery::Boundary),
    )
    .expect("sdk builder")
    .sign_with_keys(&keys)
    .expect("sign");

    let tags = |event: &nostr::Event| -> Vec<Vec<String>> {
        event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect()
    };
    assert_eq!(tags(&ours), tags(&sdk));
    assert_eq!(ours.kind, sdk.kind);
    // The one intended difference, and nothing else.
    assert_eq!(
        ours.content,
        sdk.content.replace(r#","deliver":"boundary""#, "")
    );
}

#[test]
fn a_steer_turn_is_built_by_the_sdk_unchanged() {
    let channel = uuid::Uuid::parse_str(CHANNEL).expect("uuid");
    let keys = nostr::Keys::generate();
    let ours = build_turn_command(channel, &payload("go", CodingSessionDelivery::Steer))
        .expect("builder")
        .sign_with_keys(&keys)
        .expect("sign");
    let sdk = buzz_sdk::builders::build_coding_session_command(
        channel,
        &payload("go", CodingSessionDelivery::Steer),
    )
    .expect("sdk builder")
    .sign_with_keys(&keys)
    .expect("sign");
    assert_eq!(ours.content, sdk.content);
    assert_eq!(ours.id, sdk.id);
}

// ── create refusals ──────────────────────────────────────────────────────────

#[test]
fn create_refuses_an_actor_and_says_where_seats_come_from() {
    let message =
        usage_message(refuse_unsupported_create_flags(Some(&pk(ALICE)), None, None).unwrap_err());
    assert!(message.contains("ACTOR_UNAVAILABLE"), "got {message}");
    assert!(message.contains("desktop"), "got {message}");
}

#[test]
fn create_refuses_a_role_because_a_role_is_half_of_a_pair() {
    let message =
        usage_message(refuse_unsupported_create_flags(None, Some("builder"), None).unwrap_err());
    assert!(message.contains("ACTOR_ROLE_PAIR"), "got {message}");
}

#[test]
fn create_refuses_a_driver_because_the_provider_mints_it() {
    let message =
        usage_message(refuse_unsupported_create_flags(None, None, Some("codex-acp")).unwrap_err());
    assert!(message.contains("--provider-instance"), "got {message}");
}

#[test]
fn create_accepts_the_unseated_flag_set() {
    assert!(refuse_unsupported_create_flags(None, None, None).is_ok());
}
