//! NIP-CSN: append-only session-name revisions (kind 44229).
//!
//! A name revision is human-signed text scoped to one channel and one
//! umbrella `sessionRef`. Revisions are regular events so relay retention does
//! not erase history. Consumers fold them deterministically by `(created_at,
//! event id)` and may expose revision history later without changing the wire
//! representation.

use nostr::Event;
use uuid::Uuid;

/// Exact version carried by the `csnm-v` tag.
pub const CODING_SESSION_NAME_TAG_VERSION: &str = "csnm1-1";
/// Maximum UTF-8 byte length of a short navigation name.
pub const MAX_CODING_SESSION_NAME_CONTENT_BYTES: usize = 256;

/// Validate a session reference used by a name `d` tag.
pub fn validate_coding_session_name_session_ref(session_ref: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(session_ref)
        .map_err(|_| "coding-session name d tag must be a session UUID".to_owned())?;
    if parsed.to_string() != session_ref {
        return Err("coding-session name d tag must be a lowercase canonical UUID".into());
    }
    Ok(())
}

/// Validate a short session navigation name.
pub fn validate_coding_session_name_content(content: &str) -> Result<(), String> {
    if content.len() > MAX_CODING_SESSION_NAME_CONTENT_BYTES {
        return Err(format!(
            "coding-session name content exceeds {MAX_CODING_SESSION_NAME_CONTENT_BYTES} bytes"
        ));
    }
    if content.trim().is_empty() {
        return Err("coding-session name content must contain text".into());
    }
    if content.contains('\r') || content.contains('\n') {
        return Err("coding-session name content must be a single line".into());
    }
    Ok(())
}

/// Validate the exact ordered name envelope: `h`, `d`, then `csnm-v`.
pub fn validate_coding_session_name_envelope(event: &Event) -> Result<(), String> {
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    validate_coding_session_name_parts(&tags, &event.content)
}

/// [`validate_coding_session_name_envelope`] over an event's parts rather
/// than a signed [`Event`].
///
/// Exists so a reader that holds events in its own shape — the display-name
/// resolver in [`crate::coding_session_title`] and the conformance vectors it
/// binds to, whose ids and signers are synthetic labels — applies exactly the
/// rule the relay applies at ingest, not a copy of it.
pub fn validate_coding_session_name_parts(tags: &[&[String]], content: &str) -> Result<(), String> {
    validate_coding_session_name_content(content)?;
    if tags.len() != 3 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session name requires exactly three two-field tags".into());
    }
    if tags[0][0] != "h" || Uuid::parse_str(&tags[0][1]).is_err() {
        return Err("coding-session name first tag must be a channel UUID h tag".into());
    }
    if tags[1][0] != "d" {
        return Err("coding-session name second tag must be d=sessionRef".into());
    }
    validate_coding_session_name_session_ref(&tags[1][1])?;
    if tags[2][0] != "csnm-v" || tags[2][1] != CODING_SESSION_NAME_TAG_VERSION {
        return Err("unsupported coding-session name tag version".into());
    }
    Ok(())
}

/// Select the latest already-validated name revision deterministically.
///
/// Nostr timestamps have one-second precision, so event-id ordering is the
/// stable tie-breaker. Callers must group by `(h, d)` before invoking this
/// helper and must discard events that fail
/// [`validate_coding_session_name_envelope`].
pub fn latest_coding_session_name<'a>(
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

    const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    fn event(content: &str, created_at: u64) -> Event {
        EventBuilder::new(Kind::Custom(44229), content)
            .tags([
                Tag::parse(["h", "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2"]).unwrap(),
                Tag::parse(["d", SESSION_REF]).unwrap(),
                Tag::parse(["csnm-v", CODING_SESSION_NAME_TAG_VERSION]).unwrap(),
            ])
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(&Keys::generate())
            .unwrap()
    }

    #[test]
    fn accepts_exact_name_envelope() {
        assert!(validate_coding_session_name_envelope(&event("Authority phase", 1)).is_ok());
    }

    #[test]
    fn rejects_blank_multiline_oversized_and_smuggled_forms() {
        assert!(validate_coding_session_name_envelope(&event(" \n ", 1)).is_err());
        assert!(validate_coding_session_name_envelope(&event("first\nsecond", 1)).is_err());
        assert!(validate_coding_session_name_envelope(&event(
            &"x".repeat(MAX_CODING_SESSION_NAME_CONTENT_BYTES + 1),
            1,
        ))
        .is_err());
        let smuggled = EventBuilder::new(Kind::Custom(44229), "name")
            .tags([
                Tag::parse(["h", "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2"]).unwrap(),
                Tag::parse(["d", SESSION_REF]).unwrap(),
                Tag::parse(["csnm-v", CODING_SESSION_NAME_TAG_VERSION]).unwrap(),
                Tag::parse(["p", &"ab".repeat(32)]).unwrap(),
            ])
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert!(validate_coding_session_name_envelope(&smuggled).is_err());
    }

    #[test]
    fn latest_fold_uses_timestamp_then_event_id() {
        let old = event("old", 1);
        let latest_a = event("latest a", 2);
        let latest_b = event("latest b", 2);
        let expected = if latest_a.id.to_hex() > latest_b.id.to_hex() {
            &latest_a
        } else {
            &latest_b
        };
        assert_eq!(
            latest_coding_session_name([&old, &latest_a, &latest_b]).map(|value| value.id),
            Some(expected.id)
        );
    }
}
