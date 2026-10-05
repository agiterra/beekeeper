//! Owner-side handling of kind:24312 remote keystrokes (shared-terminal
//! collaborators typing into this owner's PTY) — **defense in depth**.
//!
//! The relay already gates senders (owner or roster collaborator on a live
//! `open` announce, 20 events/s, ≤8 KiB content) and delivers only to the
//! owner's connections. None of that is trusted here: the event arrives via
//! the TS pump (`ShellBroadcastPump`), which is compromised-renderer-shaped
//! input, so this module independently re-verifies EVERYTHING before a
//! single byte reaches the PTY — signature, kind, freshness, addressing,
//! session state, sender authorization (roster collaborator or self), size,
//! and a local per-sender token bucket. Every check fails closed.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use base64::Engine;
use nostr::JsonUtil;
use tauri::Manager;

use super::manager::{self, ShellSessionInfo};

/// Max base64 content size, matching the relay's ingest cap for kind:24312.
const MAX_CONTENT_BYTES: usize = 8 * 1024;
/// Freshness window (± seconds) — a replayed old input event is refused.
const MAX_CLOCK_SKEW_SECS: u64 = 5 * 60;
/// Steady-state per-sender rate (matches the relay's 20/s cap).
const RATE_PER_SEC: f64 = 20.0;
/// Small burst allowance above the steady rate (typing is bursty).
const BURST_TOKENS: f64 = 30.0;
/// Bound the bucket map — a rotating cast of spoofed sender keys must not
/// grow memory unbounded.
const MAX_BUCKETS: usize = 256;

struct Bucket {
    tokens: f64,
    last: Instant,
}

static BUCKETS: OnceLock<Mutex<HashMap<String, Bucket>>> = OnceLock::new();

fn buckets() -> &'static Mutex<HashMap<String, Bucket>> {
    BUCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Take one token from `sender`'s bucket; `false` means over the rate cap.
fn take_token(sender: &str) -> bool {
    let Ok(mut map) = buckets().lock() else {
        // Fail closed: a poisoned lock never admits input.
        return false;
    };
    if map.len() >= MAX_BUCKETS && !map.contains_key(sender) {
        // Drop refilled (idle) buckets before admitting a new sender.
        map.retain(|_, b| b.last.elapsed().as_secs_f64() < BURST_TOKENS / RATE_PER_SEC);
        if map.len() >= MAX_BUCKETS {
            return false;
        }
    }
    let bucket = map.entry(sender.to_string()).or_insert(Bucket {
        tokens: BURST_TOKENS,
        last: Instant::now(),
    });
    let now = Instant::now();
    let refill = now.duration_since(bucket.last).as_secs_f64() * RATE_PER_SEC;
    bucket.tokens = (bucket.tokens + refill).min(BURST_TOKENS);
    bucket.last = now;
    if bucket.tokens >= 1.0 {
        bucket.tokens -= 1.0;
        true
    } else {
        false
    }
}

/// First value of the first tag named `name`.
fn tag_value<'a>(event: &'a nostr::Event, name: &str) -> Option<&'a str> {
    event.tags.iter().find_map(|tag| {
        let slice = tag.as_slice();
        (slice.first().map(String::as_str) == Some(name))
            .then(|| slice.get(1).map(String::as_str))?
    })
}

/// Validate one raw kind:24312 event against this identity and the resolved
/// session, returning the target session id + decoded input bytes. Pure of
/// IO (the session is resolved through `resolve`) so every refusal path is
/// unit-testable. Does NOT rate-limit — the caller does, after validation.
fn validate_input_event(
    event: &nostr::Event,
    my_pubkey: &nostr::PublicKey,
    now_unix: u64,
    resolve: impl Fn(&str) -> Option<ShellSessionInfo>,
) -> Result<(String, Vec<u8>), String> {
    // 1. The signature is the sender's identity — never trust a TS-claimed
    //    pubkey for writes.
    event
        .verify()
        .map_err(|e| format!("invalid event signature: {e}"))?;

    // 2. Exactly the shell-input kind.
    if event.kind.as_u16() as u32 != buzz_core_pkg::kind::KIND_SHELL_INPUT {
        return Err(format!("unexpected kind {}", event.kind.as_u16()));
    }

    // Freshness: refuse replays and far-future timestamps.
    let created_at = event.created_at.as_secs();
    if created_at.abs_diff(now_unix) > MAX_CLOCK_SKEW_SECS {
        return Err("event timestamp outside the freshness window".to_string());
    }

    // Addressed to this identity.
    let my_hex = my_pubkey.to_hex();
    let target = tag_value(event, "p").unwrap_or_default();
    if target.trim().to_ascii_lowercase() != my_hex {
        return Err("event is not addressed to this identity".to_string());
    }

    // The session must exist here, be live, and be actively broadcastable
    // (real project coordinate + shared-or-rostered) — input to a private or
    // ended session is refused even if the roster still lists the sender.
    let session_id = tag_value(event, "d")
        .filter(|d| !d.is_empty())
        .ok_or_else(|| "missing session (d) tag".to_string())?
        .to_string();
    let info = resolve(&session_id).ok_or_else(|| format!("unknown session {session_id}"))?;
    if !info.running {
        return Err(format!("session {session_id} is not running"));
    }
    if crate::shell_sessions::broadcast::may_broadcast(&info).is_none() {
        return Err(format!("session {session_id} is not broadcasting"));
    }

    // Sender authorization: the owner themself (another of their devices),
    // or a roster entry with the collaborator role. Viewers and strangers
    // are refused.
    let sender_hex = event.pubkey.to_hex();
    let authorized = sender_hex == my_hex
        || info.roster.iter().any(|entry| {
            entry.pubkey == sender_hex && entry.role == buzz_core_pkg::kind::SHELL_ROLE_COLLABORATOR
        });
    if !authorized {
        return Err("sender is not a collaborator on this session".to_string());
    }

    // Size cap, then decode.
    if event.content.len() > MAX_CONTENT_BYTES {
        return Err("input content exceeds the size cap".to_string());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(event.content.as_bytes())
        .map_err(|e| format!("invalid base64 content: {e}"))?;
    Ok((session_id, bytes))
}

/// Handle one raw kind:24312 event JSON forwarded by the TS pump: verify,
/// authorize, rate-limit, and write the decoded bytes into the session's
/// PTY. Any failure is an error to the caller and nothing is written.
pub fn handle_event_json(app: &tauri::AppHandle, event_json: &str) -> Result<(), String> {
    let event =
        nostr::Event::from_json(event_json).map_err(|e| format!("malformed input event: {e}"))?;
    let my_pubkey = app
        .state::<crate::app_state::AppState>()
        .signing_keys()?
        .public_key();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (session_id, bytes) = validate_input_event(&event, &my_pubkey, now, manager::info)?;
    // Rate-limit after signature verification so a spoofer can't burn a real
    // collaborator's budget with unsigned junk.
    if !take_token(&event.pubkey.to_hex()) {
        return Err("remote input rate limit exceeded".to_string());
    }
    manager::write(&session_id, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::JsonUtil;

    const SESSION_ID: &str = "sess-remote-input";

    fn owner_keys() -> nostr::Keys {
        nostr::Keys::generate()
    }

    fn info_with_roster(roster: Vec<(String, &str)>) -> ShellSessionInfo {
        ShellSessionInfo {
            session_id: SESSION_ID.to_string(),
            title: "shell".to_string(),
            current_directory: "/tmp".to_string(),
            shell: "/bin/zsh".to_string(),
            created_at: 1,
            rows: 24,
            cols: 80,
            running: true,
            restorable: false,
            project_ref: Some(format!("30621:{}:proj", "ab".repeat(32))),
            shared: false,
            roster: roster
                .into_iter()
                .map(|(pubkey, role)| manager::RosterEntry {
                    pubkey,
                    role: role.to_string(),
                })
                .collect(),
            coding_session: None,
        }
    }

    fn build_event(
        sender: &nostr::Keys,
        kind: u32,
        target_hex: &str,
        session_id: &str,
        content: &str,
    ) -> nostr::Event {
        nostr::EventBuilder::new(nostr::Kind::Custom(kind as u16), content)
            // Mirrors `build_shell_input_event`: the owner-as-sender case
            // p-tags the signer's own key, which the builder would strip.
            .allow_self_tagging()
            .tags([
                nostr::Tag::parse(["p", target_hex]).expect("p tag"),
                nostr::Tag::parse(["d", session_id]).expect("d tag"),
                nostr::Tag::parse(["a", &format!("30621:{}:proj", "ab".repeat(32))])
                    .expect("a tag"),
            ])
            .sign_with_keys(sender)
            .expect("sign")
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn collaborator_input_is_accepted_and_decoded() {
        let owner = owner_keys();
        let sender = nostr::Keys::generate();
        let info = info_with_roster(vec![(sender.public_key().to_hex(), "collaborator")]);
        let event = build_event(
            &sender,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(b"ls -la\r"),
        );
        let (session_id, bytes) =
            validate_input_event(&event, &owner.public_key(), now(), |_| Some(info.clone()))
                .expect("collaborator input accepted");
        assert_eq!(session_id, SESSION_ID);
        assert_eq!(bytes, b"ls -la\r");
    }

    #[test]
    fn owner_own_input_is_accepted_without_roster_entry() {
        let owner = owner_keys();
        let info = info_with_roster(vec![(
            nostr::Keys::generate().public_key().to_hex(),
            "collaborator",
        )]);
        let event = build_event(
            &owner,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(b"pwd\r"),
        );
        validate_input_event(&event, &owner.public_key(), now(), |_| Some(info.clone()))
            .expect("owner input accepted");
    }

    #[test]
    fn tampered_signature_is_refused() {
        let owner = owner_keys();
        let sender = nostr::Keys::generate();
        let info = info_with_roster(vec![(sender.public_key().to_hex(), "collaborator")]);
        let event = build_event(
            &sender,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(b"ls\r"),
        );
        // Tamper with the content after signing (JSON round-trip).
        let mut json: serde_json::Value = serde_json::from_str(&event.as_json()).unwrap();
        json["content"] = serde_json::Value::String(b64(b"rm -rf /\r"));
        let tampered = nostr::Event::from_json(json.to_string()).expect("parses");
        let err = validate_input_event(&tampered, &owner.public_key(), now(), |_| {
            Some(info.clone())
        })
        .unwrap_err();
        assert!(err.contains("signature"), "unexpected error: {err}");
    }

    #[test]
    fn wrong_kind_is_refused() {
        let owner = owner_keys();
        let sender = nostr::Keys::generate();
        let info = info_with_roster(vec![(sender.public_key().to_hex(), "collaborator")]);
        let event = build_event(
            &sender,
            buzz_core_pkg::kind::KIND_SHELL_WATCH,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(b"ls\r"),
        );
        let err = validate_input_event(&event, &owner.public_key(), now(), |_| Some(info.clone()))
            .unwrap_err();
        assert!(err.contains("kind"), "unexpected error: {err}");
    }

    #[test]
    fn wrong_p_target_is_refused() {
        let owner = owner_keys();
        let sender = nostr::Keys::generate();
        let info = info_with_roster(vec![(sender.public_key().to_hex(), "collaborator")]);
        // Addressed to some third identity, not this owner.
        let event = build_event(
            &sender,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &nostr::Keys::generate().public_key().to_hex(),
            SESSION_ID,
            &b64(b"ls\r"),
        );
        let err = validate_input_event(&event, &owner.public_key(), now(), |_| Some(info.clone()))
            .unwrap_err();
        assert!(err.contains("not addressed"), "unexpected error: {err}");
    }

    #[test]
    fn stale_event_is_refused() {
        let owner = owner_keys();
        let sender = nostr::Keys::generate();
        let info = info_with_roster(vec![(sender.public_key().to_hex(), "collaborator")]);
        let event = build_event(
            &sender,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(b"ls\r"),
        );
        // Pretend "now" is far past the event's created_at.
        let err = validate_input_event(
            &event,
            &owner.public_key(),
            event.created_at.as_secs() + MAX_CLOCK_SKEW_SECS + 1,
            |_| Some(info.clone()),
        )
        .unwrap_err();
        assert!(err.contains("freshness"), "unexpected error: {err}");
    }

    #[test]
    fn non_rostered_sender_is_refused() {
        let owner = owner_keys();
        let stranger = nostr::Keys::generate();
        let info = info_with_roster(vec![(
            nostr::Keys::generate().public_key().to_hex(),
            "collaborator",
        )]);
        let event = build_event(
            &stranger,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(b"ls\r"),
        );
        let err = validate_input_event(&event, &owner.public_key(), now(), |_| Some(info.clone()))
            .unwrap_err();
        assert!(
            err.contains("not a collaborator"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn viewer_role_sender_is_refused() {
        let owner = owner_keys();
        let viewer = nostr::Keys::generate();
        let info = info_with_roster(vec![(viewer.public_key().to_hex(), "viewer")]);
        let event = build_event(
            &viewer,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(b"ls\r"),
        );
        let err = validate_input_event(&event, &owner.public_key(), now(), |_| Some(info.clone()))
            .unwrap_err();
        assert!(
            err.contains("not a collaborator"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn oversized_content_is_refused() {
        let owner = owner_keys();
        let sender = nostr::Keys::generate();
        let info = info_with_roster(vec![(sender.public_key().to_hex(), "collaborator")]);
        // > 8 KiB of base64 (raw 7 KiB encodes to ~9.3 KiB).
        let event = build_event(
            &sender,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(&vec![b'x'; 7 * 1024]),
        );
        let err = validate_input_event(&event, &owner.public_key(), now(), |_| Some(info.clone()))
            .unwrap_err();
        assert!(err.contains("size cap"), "unexpected error: {err}");
    }

    #[test]
    fn unknown_ended_or_private_sessions_are_refused() {
        let owner = owner_keys();
        let sender = nostr::Keys::generate();
        let event = build_event(
            &sender,
            buzz_core_pkg::kind::KIND_SHELL_INPUT,
            &owner.public_key().to_hex(),
            SESSION_ID,
            &b64(b"ls\r"),
        );

        // Unknown session.
        let err = validate_input_event(&event, &owner.public_key(), now(), |_| None).unwrap_err();
        assert!(err.contains("unknown session"), "unexpected error: {err}");

        // Exited session.
        let mut ended = info_with_roster(vec![(sender.public_key().to_hex(), "collaborator")]);
        ended.running = false;
        let err = validate_input_event(&event, &owner.public_key(), now(), |_| Some(ended.clone()))
            .unwrap_err();
        assert!(err.contains("not running"), "unexpected error: {err}");

        // Not broadcasting: unshared and (after clearing) no roster.
        let mut private = info_with_roster(Vec::new());
        private.shared = false;
        let err = validate_input_event(&event, &owner.public_key(), now(), |_| {
            Some(private.clone())
        })
        .unwrap_err();
        assert!(err.contains("not broadcasting"), "unexpected error: {err}");
    }

    #[test]
    fn token_bucket_caps_sustained_rate() {
        let sender = "rate-test-sender";
        let mut admitted = 0;
        for _ in 0..100 {
            if take_token(sender) {
                admitted += 1;
            }
        }
        // The full burst drains, then the steady rate admits ~nothing within
        // this tight loop.
        assert!(
            (admitted as f64 - BURST_TOKENS).abs() <= 2.0,
            "admitted {admitted}, expected ≈{BURST_TOKENS}"
        );
    }
}
