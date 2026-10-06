//! NIP-98 HTTP Auth verification (kind:27235).
//!
//! NIP-98 is the standard Nostr HTTP Auth pattern used by Nostr.build, Blossom, and
//! other Nostr HTTP services. It is **stateless** — no WebSocket session required.
//!
//! The client signs a short-lived kind:27235 event containing the target URL, HTTP method,
//! and an optional SHA-256 hash of the request body, then sends it as:
//!
//! ```text
//! Authorization: Nostr <base64(JSON-serialized-event)>
//! ```
//!
//! ## Verification steps
//!
//! 1. Parse JSON into a `nostr::Event`
//! 2. Verify `kind == 27235` (`Kind::HttpAuth`)
//! 3. Verify Schnorr signature via `beekeeper_core::verify_event`
//! 4. Verify `created_at` within ±60 seconds of server time
//! 5. Verify `["u", <url>]` tag matches `expected_url` (normalised: case-insensitive
//!    scheme/host, trailing slash stripped)
//! 6. Verify `["method", <method>]` tag matches `expected_method` (case-insensitive)
//! 7. If `["payload", <hash>]` tag is present **and** `body` is `Some`: verify
//!    `SHA-256(body) == hex(payload_tag)`. This prevents body-substitution attacks.
//! 8. Return `event.pubkey` on success.

use nostr::{Alphabet, Event, Kind, SingleLetterTag, TagKind, Timestamp};
use sha2::{Digest, Sha256};
use url::Url;

use crate::error::AuthError;

/// Default freshness window: a token must be signed within ±60 s of server time.
///
/// The right default for a request/response API, where signing and sending are
/// milliseconds apart.
pub const TIMESTAMP_TOLERANCE_SECS: u64 = 60;

/// Verify a NIP-98 HTTP Auth event (kind:27235).
///
/// # Parameters
///
/// - `event_json` — the raw JSON string of the Nostr event (decoded from base64 by the caller).
/// - `expected_url` — the canonical URL of the request being authenticated.
///   For reverse-proxy deployments, reconstruct from `X-Forwarded-Proto` / `X-Forwarded-Host`
///   before passing here.
/// - `expected_method` — the HTTP method (e.g. `"POST"`). Compared case-insensitively.
/// - `body` — raw request body bytes. If `Some` and a `payload` tag is present in the event,
///   the SHA-256 hash of `body` must match the tag value. If `None`, the `payload` tag is
///   ignored (clients SHOULD include it for POST requests, but it is not required).
///
/// # Returns
///
/// The authenticated `nostr::PublicKey` on success.
///
/// # Errors
///
/// Returns [`AuthError::Nip98Invalid`] with a descriptive message for any verification failure.
/// The message is safe for server logs but should not be forwarded verbatim to clients.
pub fn verify_nip98_event(
    event_json: &str,
    expected_url: &str,
    expected_method: &str,
    body: Option<&[u8]>,
) -> Result<nostr::PublicKey, AuthError> {
    verify_nip98_event_within(
        event_json,
        expected_url,
        expected_method,
        body,
        TIMESTAMP_TOLERANCE_SECS,
    )
}

/// [`verify_nip98_event`] with an explicit freshness window.
///
/// Exists for the **git transport**, for a structural reason rather than a
/// preference. Git invokes a credential helper once per authentication
/// challenge and reuses the resulting `Authorization` header for the whole
/// operation: it fetches the ref advertisement first (`GET info/refs` — that is
/// what mints the token), then does everything else — runs `pre-push`,
/// enumerates and compresses the pack — and only then sends
/// `POST git-receive-pack` carrying that same, now-aged header.
///
/// Under ±60 s a slow push therefore fails *deterministically*, not flakily.
/// Observed on 2026-08-24: a `pre-push` hook running the desktop test suite took
/// 99.9 s and every push to the relay died with `RPC failed; HTTP 401`. Pack
/// building for a large repository does the same with no hook involved.
///
/// The cost is real and worth stating: the window is exactly how long an
/// observed token stays replayable. Because the git transport deliberately does
/// not bind the HTTP method (`docs/git-nip98-method-binding.md` — the sibling of
/// this debt, from the same credential-helper contract), a captured read token
/// is a write token for that long, and widening the window widens that. What
/// still holds: the `u` tag binds the token to one repository on one community
/// host, and the token only ever crosses TLS.
pub fn verify_nip98_event_within(
    event_json: &str,
    expected_url: &str,
    expected_method: &str,
    body: Option<&[u8]>,
    tolerance_secs: u64,
) -> Result<nostr::PublicKey, AuthError> {
    // 1. Parse JSON.
    let event: Event = serde_json::from_str(event_json)
        .map_err(|e| AuthError::Nip98Invalid(format!("event JSON parse error: {e}")))?;

    // 2. Verify kind == 27235.
    if event.kind != Kind::HttpAuth {
        return Err(AuthError::Nip98Invalid(format!(
            "expected kind 27235, got {}",
            event.kind.as_u16()
        )));
    }

    // 3. Verify Schnorr signature (also verifies event ID hash).
    beekeeper_core::verify_event(&event)
        .map_err(|_| AuthError::Nip98Invalid("invalid Schnorr signature".to_string()))?;

    // 4. Verify created_at within ±tolerance_secs of now.
    let now = Timestamp::now().as_secs();
    let event_ts = event.created_at.as_secs();
    let delta = now.abs_diff(event_ts);
    if delta > tolerance_secs {
        return Err(AuthError::Nip98Invalid(format!(
            "event timestamp outside ±{tolerance_secs}s window (delta: {delta}s)"
        )));
    }

    // 5. Verify `u` tag matches expected_url (normalised).
    // NIP-98 uses the single-letter "u" tag, not the multi-letter "url" tag.
    let u_tag = event
        .tags
        .find(TagKind::SingleLetter(SingleLetterTag::lowercase(
            Alphabet::U,
        )))
        .and_then(|t| t.content())
        .ok_or_else(|| AuthError::Nip98Invalid("missing `u` tag".to_string()))?;

    if normalize_url(u_tag) != normalize_url(expected_url) {
        return Err(AuthError::Nip98Invalid(format!(
            "URL mismatch: event has `{u_tag}`, expected `{expected_url}`"
        )));
    }

    // 6. Verify `method` tag matches expected_method (case-insensitive).
    let method_tag = event
        .tags
        .find(TagKind::Method)
        .and_then(|t| t.content())
        .ok_or_else(|| AuthError::Nip98Invalid("missing `method` tag".to_string()))?;

    if !method_tag.eq_ignore_ascii_case(expected_method) {
        return Err(AuthError::Nip98Invalid(format!(
            "method mismatch: event has `{method_tag}`, expected `{expected_method}`"
        )));
    }

    // 7. If `payload` tag present AND body is Some: verify SHA-256(body) == payload hex.
    let payload_tag = event.tags.find(TagKind::Payload).and_then(|t| t.content());

    if let (Some(payload_hex), Some(body_bytes)) = (payload_tag, body) {
        let computed: [u8; 32] = Sha256::digest(body_bytes).into();
        let computed_hex = hex::encode(computed);
        if computed_hex != payload_hex {
            return Err(AuthError::Nip98Invalid(
                "payload tag SHA-256 mismatch: request body does not match signed hash".to_string(),
            ));
        }
    }

    // 8. Return the authenticated pubkey.
    Ok(event.pubkey)
}

/// Normalize a URL for comparison.
///
/// - Lowercases scheme and host (already done by the `url` crate).
/// - Strips trailing slash from path.
///
/// **No loopback aliasing.** `localhost`, `::1`, and `127.0.0.1` are three
/// distinct hosts here. Under multi-tenant the `u`-tag host is the row-zero
/// community binding (`docs/multi-tenant-conformance.md`, NIP-98 row): if
/// `verify_nip98_event` collapses them, an event signed for `localhost`
/// would pass against a `127.0.0.1`-resolved community (or vice versa) —
/// a host-binding side door. Tests reconstruct `expected_url` from their
/// own bound host, the same shape production does.
fn normalize_url(raw: &str) -> String {
    let mut parsed = match Url::parse(raw) {
        Ok(u) => u,
        Err(_) => return raw.to_lowercase(),
    };
    let path = parsed.path().trim_end_matches('/').to_string();
    parsed.set_path(&path);
    parsed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Timestamp};

    const TEST_URL: &str = "https://relay.example.com/api/tokens";
    const TEST_METHOD: &str = "POST";

    fn make_nip98_event(
        keys: &Keys,
        url: &str,
        method: &str,
        payload_hex: Option<&str>,
        created_at: Option<Timestamp>,
    ) -> String {
        use nostr::Tag;

        let mut tags = vec![
            Tag::parse(["u", url]).unwrap(),
            Tag::parse(["method", method]).unwrap(),
        ];
        if let Some(hex) = payload_hex {
            tags.push(Tag::parse(["payload", hex]).unwrap());
        }

        let mut builder = EventBuilder::new(Kind::HttpAuth, "").tags(tags);
        if let Some(ts) = created_at {
            builder = builder.custom_created_at(ts);
        }
        let event = builder.sign_with_keys(keys).expect("sign");
        serde_json::to_string(&event).expect("serialize")
    }

    #[test]
    fn valid_event_returns_pubkey() {
        let keys = Keys::generate();
        let json = make_nip98_event(&keys, TEST_URL, TEST_METHOD, None, None);
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, None);
        assert!(result.is_ok(), "verify failed: {:?}", result.err());
        assert_eq!(result.unwrap(), keys.public_key());
    }

    #[test]
    fn wrong_kind_rejected() {
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::TextNote, "")
            .tags([])
            .sign_with_keys(&keys)
            .expect("sign");
        let json = serde_json::to_string(&event).unwrap();
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, None);
        assert!(matches!(result, Err(AuthError::Nip98Invalid(_))));
    }

    #[test]
    fn expired_timestamp_rejected() {
        let keys = Keys::generate();
        let old_ts = Timestamp::from(Timestamp::now().as_secs().saturating_sub(120));
        let json = make_nip98_event(&keys, TEST_URL, TEST_METHOD, None, Some(old_ts));
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, None);
        assert!(matches!(result, Err(AuthError::Nip98Invalid(_))));
    }

    /// The regression this window exists for.
    ///
    /// Git mints its token at `GET info/refs`, then runs pre-push hooks and
    /// builds the pack before sending `POST git-receive-pack` with that same
    /// header. On 2026-08-24 a 99.9s hook put the token 40s past the ±60s
    /// window and every push to the relay failed with `RPC failed; HTTP 401`.
    /// A push is not flaky under this — it is impossible.
    #[test]
    fn a_token_older_than_a_slow_pre_push_hook_still_authenticates_git() {
        let keys = Keys::generate();
        let minted = Timestamp::from(Timestamp::now().as_secs().saturating_sub(100));
        let json = make_nip98_event(&keys, TEST_URL, TEST_METHOD, None, Some(minted));

        assert!(
            verify_nip98_event(&json, TEST_URL, TEST_METHOD, None).is_err(),
            "the default API window must stay tight — widening it everywhere is not the fix"
        );
        assert_eq!(
            verify_nip98_event_within(&json, TEST_URL, TEST_METHOD, None, 600).unwrap(),
            keys.public_key()
        );
    }

    /// Widened is not unbounded. The window is exactly how long an observed
    /// token stays replayable, and the git transport does not bind the method,
    /// so a stale token must still eventually stop working.
    #[test]
    fn the_widened_window_still_expires() {
        let keys = Keys::generate();
        let ancient = Timestamp::from(Timestamp::now().as_secs().saturating_sub(601));
        let json = make_nip98_event(&keys, TEST_URL, TEST_METHOD, None, Some(ancient));
        assert!(verify_nip98_event_within(&json, TEST_URL, TEST_METHOD, None, 600).is_err());
    }

    /// Clock skew cuts both ways: a client running fast must not be able to
    /// mint a token that outlives the window.
    #[test]
    fn a_future_dated_token_is_rejected_by_the_same_bound() {
        let keys = Keys::generate();
        let ahead = Timestamp::from(Timestamp::now().as_secs().saturating_add(601));
        let json = make_nip98_event(&keys, TEST_URL, TEST_METHOD, None, Some(ahead));
        assert!(verify_nip98_event_within(&json, TEST_URL, TEST_METHOD, None, 600).is_err());
    }

    #[test]
    fn url_mismatch_rejected() {
        let keys = Keys::generate();
        let json = make_nip98_event(
            &keys,
            "https://other.example.com/api/tokens",
            TEST_METHOD,
            None,
            None,
        );
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, None);
        assert!(matches!(result, Err(AuthError::Nip98Invalid(_))));
    }

    #[test]
    fn method_mismatch_rejected() {
        let keys = Keys::generate();
        let json = make_nip98_event(&keys, TEST_URL, "GET", None, None);
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, None);
        assert!(matches!(result, Err(AuthError::Nip98Invalid(_))));
    }

    #[test]
    fn method_case_insensitive() {
        let keys = Keys::generate();
        let json = make_nip98_event(&keys, TEST_URL, "post", None, None);
        let result = verify_nip98_event(&json, TEST_URL, "POST", None);
        assert!(result.is_ok());
    }

    #[test]
    fn payload_tag_correct_hash_passes() {
        let keys = Keys::generate();
        let body = b"hello world";
        let hash: [u8; 32] = Sha256::digest(body).into();
        let hash_hex = hex::encode(hash);
        let json = make_nip98_event(&keys, TEST_URL, TEST_METHOD, Some(&hash_hex), None);
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, Some(body));
        assert!(result.is_ok());
    }

    #[test]
    fn payload_tag_wrong_hash_rejected() {
        let keys = Keys::generate();
        let body = b"hello world";
        let wrong_hex = "deadbeef".repeat(8); // 64 hex chars but wrong hash
        let json = make_nip98_event(&keys, TEST_URL, TEST_METHOD, Some(&wrong_hex), None);
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, Some(body));
        assert!(matches!(result, Err(AuthError::Nip98Invalid(_))));
    }

    #[test]
    fn payload_tag_absent_with_body_passes() {
        // payload tag is optional per spec; clients SHOULD include it but it's not required
        let keys = Keys::generate();
        let json = make_nip98_event(&keys, TEST_URL, TEST_METHOD, None, None);
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, Some(b"some body"));
        assert!(result.is_ok());
    }

    #[test]
    fn trailing_slash_normalized() {
        let keys = Keys::generate();
        let url_with_slash = "https://relay.example.com/api/tokens/";
        let json = make_nip98_event(&keys, url_with_slash, TEST_METHOD, None, None);
        // expected_url without trailing slash — should still match
        let result = verify_nip98_event(&json, TEST_URL, TEST_METHOD, None);
        assert!(result.is_ok());
    }

    #[test]
    fn loopback_aliases_are_distinct_hosts() {
        // Under multi-tenant, the `u`-tag host is the row-zero community
        // binding. An event signed for `localhost` MUST NOT pass against an
        // expected URL on `127.0.0.1` (or `::1`) — collapsing the three would
        // be a host-check side door. Production reconstructs `expected_url`
        // from the community-bound host; tests do the same.
        let keys = Keys::generate();
        let localhost_url = "http://localhost:3000/api/tokens";
        let loopback_url = "http://127.0.0.1:3000/api/tokens";
        let json = make_nip98_event(&keys, localhost_url, TEST_METHOD, None, None);
        let result = verify_nip98_event(&json, loopback_url, TEST_METHOD, None);
        assert!(
            matches!(result, Err(AuthError::Nip98Invalid(_))),
            "localhost u-tag must NOT match a 127.0.0.1 expected_url; got {result:?}"
        );

        // Symmetric: signed-for-127.0.0.1 against expected localhost — same answer.
        let json2 = make_nip98_event(&keys, loopback_url, TEST_METHOD, None, None);
        let result2 = verify_nip98_event(&json2, localhost_url, TEST_METHOD, None);
        assert!(
            matches!(result2, Err(AuthError::Nip98Invalid(_))),
            "127.0.0.1 u-tag must NOT match a localhost expected_url; got {result2:?}"
        );

        // And identity still holds — same host on both sides verifies.
        let json3 = make_nip98_event(&keys, loopback_url, TEST_METHOD, None, None);
        assert!(verify_nip98_event(&json3, loopback_url, TEST_METHOD, None).is_ok());
    }
}
