//! Durable coding-session closure revisions (kind 44230).
//!
//! A closure revision changes the shared organizational state of one umbrella
//! session without addressing, stopping, or reviving any provider execution.
//! Revisions are regular append-only events. Consumers retain the history and
//! fold it by `(created_at, event id)`: `closed` moves the umbrella to its
//! settled shelf, while `open` makes it available for continuation again.
//!
//! Every revision names the session's genesis by event id. That explicit link
//! is the authority root the relay verifies; neither a `sessionRef` scan nor a
//! provider-authored execution fact is allowed to establish ownership.

use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coding_session_lifecycle_command::validate_session_ref;

/// The currently supported coding-session closure schema version.
pub const CODING_SESSION_CLOSURE_SCHEMA_VERSION: u64 = 1;
/// Exact version carried by the `cscl-v` tag.
pub const CODING_SESSION_CLOSURE_TAG_VERSION: &str = "cscl1-1";
/// Maximum UTF-8 byte length of a complete closure payload.
pub const MAX_CODING_SESSION_CLOSURE_CONTENT_BYTES: usize = 512;

/// The shared organizational state a closure revision establishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodingSessionClosureAction {
    /// The umbrella is finished and belongs on its settled shelf.
    Closed,
    /// The umbrella is available for continuation again.
    Open,
}

/// Strict public JSON carried by a coding-session closure revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionClosurePayload {
    /// The state established by this revision.
    pub action: CodingSessionClosureAction,
    /// Event id of the session's canonical genesis authority root.
    pub genesis_ref: String,
    /// Canonical lowercase UUID of the umbrella session.
    pub session_ref: String,
    /// Schema version; currently exactly `1`.
    pub v: u64,
}

impl CodingSessionClosurePayload {
    /// Construct a closure revision at the current schema version.
    pub fn new(
        action: CodingSessionClosureAction,
        genesis_ref: impl Into<String>,
        session_ref: impl Into<String>,
    ) -> Self {
        Self {
            action,
            genesis_ref: genesis_ref.into(),
            session_ref: session_ref.into(),
            v: CODING_SESSION_CLOSURE_SCHEMA_VERSION,
        }
    }

    /// Validate every field before an event is signed or accepted.
    pub fn validate(&self) -> Result<(), String> {
        if self.v != CODING_SESSION_CLOSURE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported coding-session closure schema version {}",
                self.v
            ));
        }
        validate_coding_session_closure_event_id(&self.genesis_ref)?;
        validate_session_ref(&self.session_ref).map_err(|_| {
            format!(
                "sessionRef must be a canonical lowercase hyphenated UUID (got {:?})",
                self.session_ref
            )
        })?;
        Ok(())
    }
}

/// Validate a lowercase 64-hex event id used as a closure authority root.
pub fn validate_coding_session_closure_event_id(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "genesisRef must be a lowercase 64-hex event id (got {value:?})"
        ));
    }
    Ok(())
}

/// Strictly decode and validate a closure payload.
pub fn decode_coding_session_closure(content: &str) -> Result<CodingSessionClosurePayload, String> {
    if content.len() > MAX_CODING_SESSION_CLOSURE_CONTENT_BYTES {
        return Err(format!(
            "coding-session closure content exceeds {MAX_CODING_SESSION_CLOSURE_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session closure payload".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "coding-session closure payload must be an object".to_owned())?;
    let expected = ["action", "genesisRef", "sessionRef", "v"];
    if !expected.iter().all(|key| object.contains_key(*key))
        || !object.keys().all(|key| expected.contains(&key.as_str()))
    {
        return Err("coding-session closure payload has missing or unsupported fields".into());
    }
    let payload: CodingSessionClosurePayload = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session closure payload".to_owned())?;
    payload.validate()?;
    Ok(payload)
}

/// Validate the exact ordered closure envelope: `h`, `d`, `cscl-v`, then
/// `cscl-genesis`.
pub fn validate_coding_session_closure_envelope(
    event: &Event,
) -> Result<CodingSessionClosurePayload, String> {
    let payload = decode_coding_session_closure(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 4 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session closure requires exactly four two-field tags".into());
    }
    if tags[0][0] != "h" || uuid::Uuid::parse_str(&tags[0][1]).is_err() {
        return Err("coding-session closure first tag must be a channel UUID h tag".into());
    }
    if tags[1][0] != "d" || tags[1][1] != payload.session_ref {
        return Err("coding-session closure d tag does not match payload sessionRef".into());
    }
    if tags[2][0] != "cscl-v" || tags[2][1] != CODING_SESSION_CLOSURE_TAG_VERSION {
        return Err("unsupported coding-session closure tag version".into());
    }
    if tags[3][0] != "cscl-genesis" || tags[3][1] != payload.genesis_ref {
        return Err("coding-session closure cscl-genesis does not match payload genesisRef".into());
    }
    Ok(payload)
}

/// Select the latest already-validated closure revision deterministically.
pub fn latest_coding_session_closure<'a>(
    events: impl IntoIterator<Item = &'a Event>,
) -> Option<&'a Event> {
    events.into_iter().max_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.to_hex().cmp(&right.id.to_hex()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

    const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
    const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    fn payload(action: CodingSessionClosureAction) -> CodingSessionClosurePayload {
        CodingSessionClosurePayload::new(action, "ab".repeat(32), SESSION_REF)
    }

    fn event(action: CodingSessionClosureAction, created_at: u64) -> Event {
        let payload = payload(action);
        EventBuilder::new(
            Kind::Custom(44230),
            serde_json::to_string(&payload).unwrap(),
        )
        .tags([
            Tag::parse(["h", CHANNEL]).unwrap(),
            Tag::parse(["d", SESSION_REF]).unwrap(),
            Tag::parse(["cscl-v", CODING_SESSION_CLOSURE_TAG_VERSION]).unwrap(),
            Tag::parse(["cscl-genesis", &payload.genesis_ref]).unwrap(),
        ])
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(&Keys::generate())
        .unwrap()
    }

    #[test]
    fn accepts_exact_closed_and_open_envelopes() {
        assert!(validate_coding_session_closure_envelope(&event(
            CodingSessionClosureAction::Closed,
            1
        ))
        .is_ok());
        assert!(validate_coding_session_closure_envelope(&event(
            CodingSessionClosureAction::Open,
            2
        ))
        .is_ok());
    }

    #[test]
    fn strict_decode_rejects_unknown_actions_fields_and_references() {
        for content in [
            format!(
                r#"{{"action":"settled","genesisRef":"{}","sessionRef":"{SESSION_REF}","v":1}}"#,
                "ab".repeat(32)
            ),
            format!(
                r#"{{"action":"closed","genesisRef":"{}","sessionRef":"{SESSION_REF}","v":1,"note":"smuggled"}}"#,
                "ab".repeat(32)
            ),
            format!(
                r#"{{"action":"closed","genesisRef":"{}","sessionRef":"{SESSION_REF}","v":1}}"#,
                "AB".repeat(32)
            ),
        ] {
            assert!(
                decode_coding_session_closure(&content).is_err(),
                "{content}"
            );
        }
    }

    #[test]
    fn rejects_reordered_extra_and_disagreeing_tags() {
        let payload = payload(CodingSessionClosureAction::Closed);
        let content = serde_json::to_string(&payload).unwrap();
        for tags in [
            vec![
                Tag::parse(["d", SESSION_REF]).unwrap(),
                Tag::parse(["h", CHANNEL]).unwrap(),
                Tag::parse(["cscl-v", CODING_SESSION_CLOSURE_TAG_VERSION]).unwrap(),
                Tag::parse(["cscl-genesis", &payload.genesis_ref]).unwrap(),
            ],
            vec![
                Tag::parse(["h", CHANNEL]).unwrap(),
                Tag::parse(["d", SESSION_REF]).unwrap(),
                Tag::parse(["cscl-v", CODING_SESSION_CLOSURE_TAG_VERSION]).unwrap(),
                Tag::parse(["cscl-genesis", &"cd".repeat(32)]).unwrap(),
            ],
            vec![
                Tag::parse(["h", CHANNEL]).unwrap(),
                Tag::parse(["d", SESSION_REF]).unwrap(),
                Tag::parse(["cscl-v", CODING_SESSION_CLOSURE_TAG_VERSION]).unwrap(),
                Tag::parse(["cscl-genesis", &payload.genesis_ref]).unwrap(),
                Tag::parse(["p", &"ef".repeat(32)]).unwrap(),
            ],
        ] {
            let event = EventBuilder::new(Kind::Custom(44230), &content)
                .tags(tags)
                .sign_with_keys(&Keys::generate())
                .unwrap();
            assert!(validate_coding_session_closure_envelope(&event).is_err());
        }
    }

    #[test]
    fn latest_fold_uses_timestamp_then_event_id() {
        let old = event(CodingSessionClosureAction::Closed, 1);
        let latest_a = event(CodingSessionClosureAction::Open, 2);
        let latest_b = event(CodingSessionClosureAction::Closed, 2);
        let expected = if latest_a.id.to_hex() > latest_b.id.to_hex() {
            &latest_a
        } else {
            &latest_b
        };
        assert_eq!(
            latest_coding_session_closure([&old, &latest_a, &latest_b]).map(|value| value.id),
            Some(expected.id)
        );
    }
}
