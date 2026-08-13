//! NIP-ST shared-terminal ephemeral kinds: watch (24310) and frame (24311).
//!
//! Modeled on the kind:24200 agent-observer branch in `event.rs`: signature
//! verified off-thread, a ±5-minute freshness window, strict tag validation,
//! a publisher-side project-membership gate, and per-pubkey rate limits —
//! then Global-topic pub/sub fan-out, never storage. Delivery-side gating for
//! private projects lives in `filter_fanout_by_access` (the shared fan-out
//! chokepoint), keyed off the same `a`-tag project coordinate validated here.
//!
//! The relay stays stateless about who is watching: watcher bookkeeping is
//! the session owner's concern (it must see 24310 events to stream at all).

use std::sync::Arc;
use std::time::Instant;

use buzz_core::event::StoredEvent;
use buzz_core::kind::{event_kind_u32, KIND_SHELL_WATCH};
use buzz_core::verification::verify_event;
use buzz_core::CommunityId;
use buzz_pubsub::EventTopic;
use nostr::Event;
use tracing::warn;

use crate::connection::ConnectionState;
use crate::protocol::RelayMessage;
use crate::state::{AppState, ScopedRateLimiter};

use super::event::fan_out_event_to_local_subscribers;
use super::ingest::validate_project_ref_tag;

/// Max base64 content bytes of one frame event (raw ≈ 72 KiB): a 64 KiB
/// scrollback tail is chunked below this, and rendered-screen snapshots/diffs
/// are far smaller.
const MAX_FRAME_CONTENT_BYTES: usize = 96 * 1024;
/// Max content bytes of a watch event (a tiny JSON action).
const MAX_WATCH_CONTENT_BYTES: usize = 1024;
/// Frame events allowed per second per (community, publisher).
const FRAME_RATE_PER_SEC: u32 = 60;
/// Watch events allowed per second per (community, observer).
const WATCH_RATE_PER_SEC: u32 = 10;
/// The frame types the relay accepts; unknown types are rejected (fail
/// closed — an observer would ignore them anyway, so nothing is lost).
const FRAME_TYPES: &[&str] = &["tail", "snap", "diff", "resize", "end"];

/// Handle a shared-terminal ephemeral event (kind 24310/24311) end to end.
/// Sends the `OK` itself in every arm.
pub(crate) async fn handle_shell_observe_event(
    event: Event,
    conn_id: uuid::Uuid,
    event_id_hex: &str,
    conn: Arc<ConnectionState>,
    state: Arc<AppState>,
) {
    let event_clone = event.clone();
    let verify_result = tokio::task::spawn_blocking(move || verify_event(&event_clone)).await;
    match verify_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            conn.send(RelayMessage::ok(
                event_id_hex,
                false,
                &format!("invalid: {e}"),
            ));
            return;
        }
        Err(_) => {
            conn.send(RelayMessage::ok(
                event_id_hex,
                false,
                "error: internal error",
            ));
            return;
        }
    }

    // Freshness: a replayed frame/watch outside the window is useless and
    // only costs fan-out.
    let now = chrono::Utc::now().timestamp();
    let event_ts = event.created_at.as_secs() as i64;
    if (event_ts - now).unsigned_abs() > 300 {
        conn.send(RelayMessage::ok(
            event_id_hex,
            false,
            "invalid: shared-terminal event timestamp outside ±5 minute freshness window",
        ));
        return;
    }

    let coordinate = match validate_shell_observe_tags(&event) {
        Ok(coordinate) => coordinate,
        Err(message) => {
            conn.send(RelayMessage::ok(event_id_hex, false, &message));
            return;
        }
    };

    // Publisher gate: events scoped to a *private* project may only be
    // published by identities the project admits. Public/unknown coordinates
    // are open to any authenticated member, matching
    // `can_access_project_contents` semantics for other project content.
    let gate = match state
        .project_coordinate_gate_cached(conn.tenant.community(), &coordinate)
        .await
    {
        Ok(gate) => gate,
        Err(e) => {
            // Fail closed on lookup errors.
            warn!(conn_id = %conn_id, event_id = %event_id_hex, "shared-terminal project gate lookup failed: {e}");
            conn.send(RelayMessage::ok(
                event_id_hex,
                false,
                "error: internal server error",
            ));
            return;
        }
    };
    if let Some(gate) = gate {
        if !gate.admits_read(&event.pubkey.to_bytes()) {
            conn.send(RelayMessage::ok(
                event_id_hex,
                false,
                "restricted: project is private",
            ));
            return;
        }
    }

    let kind_u32 = event_kind_u32(&event);
    let (limiter, limit) = if kind_u32 == KIND_SHELL_WATCH {
        (&state.shell_watch_rate_limiter, WATCH_RATE_PER_SEC)
    } else {
        (&state.shell_frame_rate_limiter, FRAME_RATE_PER_SEC)
    };
    let author_key: [u8; 32] = event.pubkey.to_bytes();
    if rate_limited(limiter, conn.tenant.community(), author_key, limit) {
        conn.send(RelayMessage::ok(
            event_id_hex,
            false,
            "rate-limited: shared-terminal event rate exceeded",
        ));
        return;
    }

    state.mark_local_event(conn.tenant.community(), &event.id);
    if let Err(e) = state
        .pubsub
        .publish_event(&conn.tenant, EventTopic::Global, &event)
        .await
    {
        state
            .local_event_ids
            .invalidate(&(conn.tenant.community(), event.id.to_bytes()));
        warn!(conn_id = %conn_id, event_id = %event_id_hex, "shared-terminal publish failed: {e}");
    }

    let stored_event = StoredEvent::new(event, None);
    fan_out_event_to_local_subscribers(&state, conn.tenant.community(), &stored_event).await;

    conn.send(RelayMessage::ok(event_id_hex, true, ""));
}

/// Validate the tag/content envelope of a 24310/24311 and return its project
/// coordinate. Rejections are complete `OK` message strings.
fn validate_shell_observe_tags(event: &Event) -> Result<String, String> {
    let kind_u32 = event_kind_u32(event);

    let max_content = if kind_u32 == KIND_SHELL_WATCH {
        MAX_WATCH_CONTENT_BYTES
    } else {
        MAX_FRAME_CONTENT_BYTES
    };
    if event.content.len() > max_content {
        return Err(format!(
            "invalid: content exceeds {max_content} bytes for shared-terminal events"
        ));
    }

    // Exactly one session id, in a shape a session id can take (uuid-ish:
    // bounded, no whitespace/control bytes).
    let session_ids = tag_values(event, "d");
    let [session_id] = session_ids.as_slice() else {
        return Err("invalid: shared-terminal events require exactly one d tag".into());
    };
    if session_id.is_empty()
        || session_id.len() > 64
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("invalid: malformed shared-terminal session id".into());
    }

    // Exactly one project coordinate, validated with the same rule as every
    // other `30621:<owner>:<dtag>` reference.
    let coordinates = tag_values(event, "a");
    let [coordinate] = coordinates.as_slice() else {
        return Err("invalid: shared-terminal events require exactly one a tag".into());
    };
    validate_project_ref_tag(coordinate).map_err(|e| format!("invalid: {e}"))?;

    if kind_u32 == KIND_SHELL_WATCH {
        // Routing target: exactly one owner pubkey.
        let owners = tag_values(event, "p");
        let [owner] = owners.as_slice() else {
            return Err("invalid: shared-terminal watch requires exactly one p tag".into());
        };
        if nostr::PublicKey::from_hex(owner).is_err() {
            return Err("invalid: malformed shared-terminal watch p tag".into());
        }
    } else {
        // Frames declare a known type.
        let types = tag_values(event, "t");
        let [frame_type] = types.as_slice() else {
            return Err("invalid: shared-terminal frames require exactly one t tag".into());
        };
        if !FRAME_TYPES.contains(&frame_type.as_str()) {
            return Err("invalid: unknown shared-terminal frame type".into());
        }
    }

    Ok(coordinate.clone())
}

/// The values of every tag named `name` (single-letter tags only).
fn tag_values(event: &Event, name: &str) -> Vec<String> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            let parts = tag.as_slice();
            (parts.len() >= 2 && parts[0].as_str() == name).then(|| parts[1].as_str().to_string())
        })
        .collect()
}

/// Check + bump a per-(community, pubkey) fixed-window limit. Same shape as
/// `observer_frame_rate_limited` in `event.rs`, with the limit parameterized.
fn rate_limited(
    limiter: &ScopedRateLimiter,
    community_id: CommunityId,
    pubkey: [u8; 32],
    limit: u32,
) -> bool {
    let now = Instant::now();
    let mut entry = limiter.entry((community_id, pubkey)).or_insert((0, now));
    let (count, window_start) = entry.value_mut();
    if now.duration_since(*window_start).as_secs() >= 1 {
        *count = 1;
        *window_start = now;
        false
    } else {
        *count += 1;
        *count > limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn signed(kind: u32, tags: Vec<Tag>, content: &str) -> Event {
        let keys = Keys::generate();
        EventBuilder::new(Kind::Custom(kind as u16), content)
            .tags(tags)
            .sign_with_keys(&keys)
            .expect("sign")
    }

    fn tag(parts: &[&str]) -> Tag {
        Tag::parse(parts.iter().map(|s| s.to_string()).collect::<Vec<_>>()).expect("tag")
    }

    fn owner_hex() -> String {
        Keys::generate().public_key().to_hex()
    }

    fn valid_frame_tags(coord: &str) -> Vec<Tag> {
        vec![
            tag(&["d", "a4f6c8e0-1111-2222-3333-444455556666"]),
            tag(&["a", coord]),
            tag(&["t", "snap"]),
            tag(&["seq", "1"]),
            tag(&["epoch", "e1"]),
        ]
    }

    #[test]
    fn valid_frame_passes_and_returns_coordinate() {
        let coord = format!("30621:{}:myproj", owner_hex());
        let ev = signed(24311, valid_frame_tags(&coord), "aGVsbG8=");
        assert_eq!(validate_shell_observe_tags(&ev).expect("valid"), coord);
    }

    #[test]
    fn frame_requires_known_type_and_singleton_tags() {
        let coord = format!("30621:{}:myproj", owner_hex());

        let mut tags = valid_frame_tags(&coord);
        tags.retain(|t| t.as_slice()[0].as_str() != "t");
        tags.push(tag(&["t", "mystery"]));
        let ev = signed(24311, tags, "");
        assert!(validate_shell_observe_tags(&ev)
            .unwrap_err()
            .contains("unknown shared-terminal frame type"));

        let mut tags = valid_frame_tags(&coord);
        tags.push(tag(&["a", &format!("30621:{}:other", owner_hex())]));
        let ev = signed(24311, tags, "");
        assert!(validate_shell_observe_tags(&ev)
            .unwrap_err()
            .contains("exactly one a tag"));

        let mut tags = valid_frame_tags(&coord);
        tags.retain(|t| t.as_slice()[0].as_str() != "d");
        let ev = signed(24311, tags, "");
        assert!(validate_shell_observe_tags(&ev)
            .unwrap_err()
            .contains("exactly one d tag"));
    }

    #[test]
    fn malformed_session_id_and_coordinate_are_rejected() {
        let coord = format!("30621:{}:myproj", owner_hex());

        let ev = signed(
            24311,
            vec![
                tag(&["d", "../../etc/passwd"]),
                tag(&["a", &coord]),
                tag(&["t", "snap"]),
            ],
            "",
        );
        assert!(validate_shell_observe_tags(&ev)
            .unwrap_err()
            .contains("malformed shared-terminal session id"));

        let ev = signed(
            24311,
            vec![
                tag(&["d", "abc-123"]),
                tag(&["a", "30617:deadbeef:not-a-project"]),
                tag(&["t", "snap"]),
            ],
            "",
        );
        assert!(validate_shell_observe_tags(&ev).is_err());
    }

    #[test]
    fn watch_requires_single_valid_owner_p_tag() {
        let coord = format!("30621:{}:myproj", owner_hex());
        let ev = signed(
            24310,
            vec![tag(&["d", "abc-123"]), tag(&["a", &coord])],
            "{\"action\":\"watch\"}",
        );
        assert!(validate_shell_observe_tags(&ev)
            .unwrap_err()
            .contains("exactly one p tag"));

        let ev = signed(
            24310,
            vec![
                tag(&["d", "abc-123"]),
                tag(&["a", &coord]),
                tag(&["p", "zznothex"]),
            ],
            "{\"action\":\"watch\"}",
        );
        assert!(validate_shell_observe_tags(&ev)
            .unwrap_err()
            .contains("malformed shared-terminal watch p tag"));

        let ev = signed(
            24310,
            vec![
                tag(&["d", "abc-123"]),
                tag(&["a", &coord]),
                tag(&["p", &owner_hex()]),
            ],
            "{\"action\":\"watch\"}",
        );
        assert!(validate_shell_observe_tags(&ev).is_ok());
    }

    #[test]
    fn oversized_content_is_rejected_per_kind() {
        let coord = format!("30621:{}:myproj", owner_hex());
        let big_watch = "x".repeat(MAX_WATCH_CONTENT_BYTES + 1);
        let ev = signed(
            24310,
            vec![
                tag(&["d", "abc-123"]),
                tag(&["a", &coord]),
                tag(&["p", &owner_hex()]),
            ],
            &big_watch,
        );
        assert!(validate_shell_observe_tags(&ev)
            .unwrap_err()
            .contains("content exceeds"));

        // The same size is fine for a frame (its cap is much larger).
        let ev = signed(24311, valid_frame_tags(&coord), &big_watch);
        assert!(validate_shell_observe_tags(&ev).is_ok());
    }

    #[test]
    fn rate_limiter_scopes_by_community_and_resets_windows() {
        let limiter: ScopedRateLimiter = DashMap::new();
        let community_a = CommunityId::from_uuid(uuid::Uuid::new_v4());
        let community_b = CommunityId::from_uuid(uuid::Uuid::new_v4());
        let key = [7u8; 32];

        for _ in 0..10 {
            assert!(!rate_limited(&limiter, community_a, key, 10));
        }
        assert!(rate_limited(&limiter, community_a, key, 10));
        // Another community has its own budget.
        assert!(!rate_limited(&limiter, community_b, key, 10));
    }
}
