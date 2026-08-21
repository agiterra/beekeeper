//! NIP-CSG: append-only session-goal revisions (kind 44227).
//!
//! A goal revision is human-signed prose scoped to one channel and one
//! umbrella `sessionRef`. Revisions are regular events so relay retention does
//! not erase history. Consumers fold them deterministically by `(created_at,
//! event id)` and may expose revision history later without changing the wire
//! representation.

use nostr::Event;
use uuid::Uuid;

/// Exact version carried by the `csgl-v` tag.
pub const CODING_SESSION_GOAL_TAG_VERSION: &str = "csgl1-1";
/// Maximum UTF-8 byte length of goal prose.
pub const MAX_CODING_SESSION_GOAL_CONTENT_BYTES: usize = 4 * 1024;

/// Validate a session reference used by a goal `d` tag.
pub fn validate_coding_session_goal_session_ref(session_ref: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(session_ref)
        .map_err(|_| "coding-session goal d tag must be a session UUID".to_owned())?;
    if parsed.to_string() != session_ref {
        return Err("coding-session goal d tag must be a lowercase canonical UUID".into());
    }
    Ok(())
}

/// Validate raw goal prose.
pub fn validate_coding_session_goal_content(content: &str) -> Result<(), String> {
    if content.len() > MAX_CODING_SESSION_GOAL_CONTENT_BYTES {
        return Err(format!(
            "coding-session goal content exceeds {MAX_CODING_SESSION_GOAL_CONTENT_BYTES} bytes"
        ));
    }
    if content.trim().is_empty() {
        return Err("coding-session goal content must contain prose".into());
    }
    Ok(())
}

/// Validate the exact ordered goal envelope: `h`, `d`, then `csgl-v`.
pub fn validate_coding_session_goal_envelope(event: &Event) -> Result<(), String> {
    validate_coding_session_goal_content(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 3 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session goal requires exactly three two-field tags".into());
    }
    if tags[0][0] != "h" || Uuid::parse_str(&tags[0][1]).is_err() {
        return Err("coding-session goal first tag must be a channel UUID h tag".into());
    }
    if tags[1][0] != "d" {
        return Err("coding-session goal second tag must be d=sessionRef".into());
    }
    validate_coding_session_goal_session_ref(&tags[1][1])?;
    if tags[2][0] != "csgl-v" || tags[2][1] != CODING_SESSION_GOAL_TAG_VERSION {
        return Err("unsupported coding-session goal tag version".into());
    }
    Ok(())
}

/// Select the latest already-validated goal revision deterministically.
///
/// Nostr timestamps have one-second precision, so event-id ordering is the
/// stable tie-breaker. Callers must group by `(h, d)` before invoking this
/// helper and must discard events that fail
/// [`validate_coding_session_goal_envelope`].
pub fn latest_coding_session_goal<'a>(
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
        EventBuilder::new(Kind::Custom(44227), content)
            .tags([
                Tag::parse(["h", "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2"]).unwrap(),
                Tag::parse(["d", SESSION_REF]).unwrap(),
                Tag::parse(["csgl-v", CODING_SESSION_GOAL_TAG_VERSION]).unwrap(),
            ])
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(&Keys::generate())
            .unwrap()
    }

    #[test]
    fn accepts_exact_goal_envelope() {
        assert!(validate_coding_session_goal_envelope(&event("Ship authority pixels", 1)).is_ok());
    }

    #[test]
    fn rejects_blank_oversized_and_smuggled_forms() {
        assert!(validate_coding_session_goal_envelope(&event(" \n ", 1)).is_err());
        assert!(validate_coding_session_goal_envelope(&event(
            &"x".repeat(MAX_CODING_SESSION_GOAL_CONTENT_BYTES + 1),
            1,
        ))
        .is_err());
        let smuggled = EventBuilder::new(Kind::Custom(44227), "goal")
            .tags([
                Tag::parse(["h", "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2"]).unwrap(),
                Tag::parse(["d", SESSION_REF]).unwrap(),
                Tag::parse(["csgl-v", CODING_SESSION_GOAL_TAG_VERSION]).unwrap(),
                Tag::parse(["p", &"ab".repeat(32)]).unwrap(),
            ])
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert!(validate_coding_session_goal_envelope(&smuggled).is_err());
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
            latest_coding_session_goal([&old, &latest_a, &latest_b]).map(|value| value.id),
            Some(expected.id)
        );
    }
}
