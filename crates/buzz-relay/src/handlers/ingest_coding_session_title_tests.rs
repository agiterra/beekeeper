//! The relay's half of kind 44252 (NIP-CSG § Generated title): scope,
//! membership, and structure only. Mirrors the 44229 name tests in
//! `ingest.rs` (`coding_session_name_requires_exact_regular_revision_envelope`).

use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::coding_session_title::{
    validate_coding_session_title_envelope, CodingSessionTitleBasis, CodingSessionTitlePayload,
    CODING_SESSION_TITLE_SCHEMA,
};
use buzz_sdk::builders::build_coding_session_generated_title;

use super::*;

const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "1958c6c448e05eed".into(),
        session_id: "sess-a".into(),
        generation: 1,
    }
}

fn content(title: &str) -> String {
    serde_json::json!({
        "schema": CODING_SESSION_TITLE_SCHEMA,
        "title": title,
        "model": "claude-haiku-4-5",
        "basis": "first-message",
        "sourceCommand": null,
        "createEventId": "ca".repeat(32),
    })
    .to_string()
}

fn sign(content: &str, tags: &[&[&str]]) -> Event {
    let tags: Vec<nostr::Tag> = tags
        .iter()
        .map(|parts| nostr::Tag::parse(parts.iter().copied()).expect("tag"))
        .collect();
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CODING_SESSION_GENERATED_TITLE as u16),
        content,
    )
    .tags(tags)
    .sign_with_keys(&nostr::Keys::generate())
    .expect("sign title")
}

fn title_event(content: &str, channel: &str, version: &str) -> Event {
    let key = coding_session_target_key(&target());
    sign(
        content,
        &[
            &["h", channel],
            &["d", SESSION_REF],
            &["cstl-v", version],
            &["cs-target", &key],
        ],
    )
}

/// Kind 44252 is a channel-scoped message write under the strict
/// coding-session membership gate, like every other session kind.
///
/// It deliberately does **not** decide standing: the signer below is a fresh
/// key with no execution anywhere, and the structure still passes. Whether
/// the signer is the provider authority of the execution its `cs-target`
/// names is the reader's fold (`resolve_session_display_name`).
#[test]
fn generated_title_uses_core_envelope_and_strict_membership_admission() {
    let channel = Uuid::new_v4();
    let valid = build_coding_session_generated_title(
        channel,
        SESSION_REF,
        &target(),
        &CodingSessionTitlePayload {
            schema: CODING_SESSION_TITLE_SCHEMA.into(),
            title: "Login redirect fix".into(),
            model: "claude-haiku-4-5".into(),
            basis: CodingSessionTitleBasis::FirstMessage,
            source_command: None,
            create_event_id: "ca".repeat(32),
        },
    )
    .expect("title builder")
    .sign_with_keys(&nostr::Keys::generate())
    .expect("sign title");
    assert!(validate_coding_session_title_envelope(&valid).is_ok());

    assert_eq!(
        required_scope_for_kind(KIND_CODING_SESSION_GENERATED_TITLE, &valid).unwrap(),
        Scope::MessagesWrite
    );
    assert!(requires_h_channel_scope(
        KIND_CODING_SESSION_GENERATED_TITLE
    ));
    assert!(!is_global_only_kind(KIND_CODING_SESSION_GENERATED_TITLE));
    assert!(is_coding_session_kind(KIND_CODING_SESSION_GENERATED_TITLE));
    assert!(requires_strict_coding_session_membership(
        KIND_CODING_SESSION_GENERATED_TITLE
    ));
}

#[test]
fn generated_title_requires_the_exact_envelope() {
    let channel = Uuid::new_v4().to_string();
    let key = coding_session_target_key(&target());
    assert!(validate_coding_session_title_envelope(&title_event(
        &content("Login redirect fix"),
        &channel,
        "cstl1-1"
    ))
    .is_ok());

    let refused = [
        // An extra tag.
        sign(
            &content("Extra tag"),
            &[
                &["h", &channel],
                &["d", SESSION_REF],
                &["cstl-v", "cstl1-1"],
                &["cs-target", &key],
                &["p", &"ab".repeat(32)],
            ],
        ),
        // Reordered tags.
        sign(
            &content("Reordered"),
            &[
                &["d", SESSION_REF],
                &["h", &channel],
                &["cstl-v", "cstl1-1"],
                &["cs-target", &key],
            ],
        ),
        // No cs-target.
        sign(
            &content("No target"),
            &[
                &["h", &channel],
                &["d", SESSION_REF],
                &["cstl-v", "cstl1-1"],
            ],
        ),
        // A malformed cs-target.
        sign(
            &content("Bad target"),
            &[
                &["h", &channel],
                &["d", SESSION_REF],
                &["cstl-v", "cstl1-1"],
                &["cs-target", "coding-session/v1|wrong"],
            ],
        ),
        // An unknown cstl-v.
        title_event(&content("Unknown version"), &channel, "cstl1-2"),
        // A multiline title.
        title_event(&content("first\nsecond"), &channel, "cstl1-1"),
        // A title over 256 bytes.
        title_event(&content(&"x".repeat(257)), &channel, "cstl1-1"),
        // A blank title.
        title_event(&content("   "), &channel, "cstl1-1"),
        // Bad JSON.
        title_event("{\"schema\":", &channel, "cstl1-1"),
        // Prose, as a 44229 would carry.
        title_event("Login redirect fix", &channel, "cstl1-1"),
        // A non-UUID channel.
        title_event(&content("Bad channel"), "general", "cstl1-1"),
    ];
    for event in refused {
        assert!(
            validate_coding_session_title_envelope(&event).is_err(),
            "accepted tags {:?} content {:?}",
            event.tags,
            event.content
        );
    }
}

/// An unknown content key is refused, so nothing rides along with a title —
/// no prompt text, no workdir.
#[test]
fn generated_title_refuses_unknown_content_keys() {
    let channel = Uuid::new_v4().to_string();
    let mut value: serde_json::Value = serde_json::from_str(&content("Title")).expect("json");
    value["prompt"] = serde_json::Value::String("the first message".into());
    assert!(validate_coding_session_title_envelope(&title_event(
        &value.to_string(),
        &channel,
        "cstl1-1"
    ))
    .is_err());
}
