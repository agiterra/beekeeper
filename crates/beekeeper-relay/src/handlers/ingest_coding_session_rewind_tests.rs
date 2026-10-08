//! The relay's half of SV-29 `session.rewind` (NIP-CSL): a 44221 rewind is
//! stored only in its exact five-key action shape under the restart's tags.
//! The provider authorizes it; the relay only refuses what is malformed.

use super::*;

const CHECKPOINT: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

fn rewind_action() -> serde_json::Value {
    serde_json::json!({
        "type": "session.rewind",
        "session": {
            "driver": "claude-agent-acp",
            "instanceId": "1958c6c448e05eed",
            "sessionId": "sess-a",
            "generation": 3,
        },
        "providerAuthorityPubkey": "ab".repeat(32),
        "checkpoint": CHECKPOINT,
        "files": "restore",
    })
}

fn content(action: serde_json::Value) -> String {
    serde_json::json!({
        "schema": "buzz-coding-session-lifecycle-command/v1",
        "commandId": "rewind-1",
        "action": action,
    })
    .to_string()
}

fn event(content: &str, channel: &str) -> Event {
    let tags: Vec<nostr::Tag> = [
        ["h", channel],
        ["csl-v", "csl1-1"],
        ["csl-command", "rewind-1"],
    ]
    .iter()
    .map(|parts| nostr::Tag::parse(parts.iter().copied()).expect("tag"))
    .collect();
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16),
        content,
    )
    .tags(tags)
    .sign_with_keys(&nostr::Keys::generate())
    .expect("sign rewind")
}

#[test]
fn ingest_accepts_an_exact_session_rewind() {
    let channel = Uuid::new_v4().to_string();
    for files in ["keep", "restore"] {
        let mut action = rewind_action();
        action["files"] = serde_json::Value::from(files);
        assert!(
            validate_coding_session_lifecycle_command_envelope(&event(&content(action), &channel))
                .is_ok(),
            "{files}"
        );
    }
}

#[test]
fn ingest_refuses_a_malformed_session_rewind() {
    let channel = Uuid::new_v4().to_string();
    let mut cases = Vec::new();
    for key in ["checkpoint", "files", "session", "providerAuthorityPubkey"] {
        let mut action = rewind_action();
        action.as_object_mut().expect("object").remove(key);
        cases.push((format!("missing {key}"), action));
    }
    let mut extra = rewind_action();
    extra["through"] = serde_json::Value::from(40);
    cases.push(("extra key".into(), extra));
    for (label, field, value) in [
        ("short checkpoint", "checkpoint", serde_json::json!("cd")),
        (
            "uppercase checkpoint",
            "checkpoint",
            serde_json::json!(CHECKPOINT.to_uppercase()),
        ),
        ("null checkpoint", "checkpoint", serde_json::Value::Null),
        ("unknown files", "files", serde_json::json!("everything")),
        ("null files", "files", serde_json::Value::Null),
    ] {
        let mut action = rewind_action();
        action[field] = value;
        cases.push((label.into(), action));
    }
    for (label, action) in cases {
        assert!(
            validate_coding_session_lifecycle_command_envelope(&event(&content(action), &channel))
                .is_err(),
            "accepted {label}"
        );
    }
}
