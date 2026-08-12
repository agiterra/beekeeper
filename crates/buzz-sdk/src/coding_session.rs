//! Coding-session tag versions and semantic-key derivation.
//!
//! Every coding-session event carries a *semantic key* tag — a deterministic,
//! collision-free encoding of the identity the event claims. Consumers use it to
//! deduplicate and to fence a stale writer; producers use it as the idempotency
//! key of an outbox row. Because both sides derive the key independently, the
//! encoding has to agree byte-for-byte across the Rust producer and the
//! TypeScript consumer.
//!
//! The encoding is length-prefixed, which is the whole point: joining fields
//! with a separator lets a field containing that separator impersonate a
//! different tuple, and session ids and driver slugs are attacker-adjacent
//! strings. `10:instance-1` cannot be confused with anything else.
//!
//! Mirrors the donor's `encodeStructuredKey`
//! (`desktop/src/features/agents/ui/buzzSessionProjectionKeys.ts`).

use sha2::{Digest, Sha256};

use buzz_core::coding_session_command::CodingSessionTarget;

/// Version tag value on every coding-session provider catalog (kind 44222).
pub const CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION: &str = "cspc1-1";
/// Version tag value on every coding-session metadata event (kind 44223).
pub const CODING_SESSION_METADATA_TAG_VERSION: &str = "csm1-1";
/// Version tag value on every coding-session lifecycle receipt (kind 44224).
pub const CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION: &str = "cslr1-1";
/// Version tag value on every coding-session transcript item (kind 44225).
pub const CODING_SESSION_TRANSCRIPT_TAG_VERSION: &str = "cst1-1";

/// Maximum signed content bytes for a provider catalog (kind 44222).
pub const MAX_PROVIDER_CATALOG_CONTENT_BYTES: usize = 256 * 1024;
/// Maximum signed content bytes for a session metadata event (kind 44223).
pub const MAX_METADATA_CONTENT_BYTES: usize = 32 * 1024;
/// Maximum signed content bytes for a lifecycle receipt (kind 44224).
pub const MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES: usize = 16 * 1024;
/// Maximum signed content bytes for a transcript item (kind 44225).
pub const MAX_TRANSCRIPT_CONTENT_BYTES: usize = 32 * 1024;

/// Encode a deterministic, unambiguous structured key.
///
/// Each field is prefixed with its UTF-8 byte length and a colon, then all
/// fields are concatenated after `<domain>|`. There is no separator between
/// fields — the length prefix is the separator, which is what makes the
/// encoding injective over arbitrary field content.
///
/// ```
/// use buzz_sdk::coding_session::encode_structured_key;
/// assert_eq!(
///     encode_structured_key("coding-session/v1", &["provider-a", "instance-1"]),
///     "coding-session/v1|10:provider-a10:instance-1"
/// );
/// ```
pub fn encode_structured_key(domain: &str, fields: &[&str]) -> String {
    let mut key = String::with_capacity(domain.len() + 1);
    key.push_str(domain);
    key.push('|');
    for field in fields {
        key.push_str(&field.len().to_string());
        key.push(':');
        key.push_str(field);
    }
    key
}

/// The `cs-target` tag value naming one exact session generation.
///
/// Identical to [`buzz_core::coding_session_command::coding_session_target_key`];
/// re-derived here through [`encode_structured_key`] so the shared encoding has
/// a single tested definition. A test asserts the two agree.
pub fn coding_session_target_key(target: &CodingSessionTarget) -> String {
    encode_structured_key(
        "coding-session/v1",
        &[
            &target.driver,
            &target.instance_id,
            &target.session_id,
            &target.generation.to_string(),
        ],
    )
}

/// The `csm-key` tag value: the immutable identity of one generation's metadata.
pub fn coding_session_metadata_semantic_key(target: &CodingSessionTarget) -> String {
    encode_structured_key(
        "coding-session-metadata/v1",
        &[
            &target.driver,
            &target.instance_id,
            &target.session_id,
            &target.generation.to_string(),
        ],
    )
}

/// The `csl-key` tag value: the immutable identity of one lifecycle receipt.
///
/// Keyed by `commandId` alone. One lifecycle command has exactly one outcome,
/// so a second receipt for the same command is a duplicate to be dropped, not a
/// revision to be applied.
pub fn coding_session_lifecycle_receipt_semantic_key(command_id: &str) -> String {
    encode_structured_key("coding-session-lifecycle-receipt/v1", &[command_id])
}

/// The `cst-key` tag value: the immutable identity of one transcript item.
pub fn coding_session_transcript_semantic_key(
    target: &CodingSessionTarget,
    event_seq: u64,
) -> String {
    encode_structured_key(
        "coding-session-transcript/v1",
        &[
            &target.driver,
            &target.instance_id,
            &target.session_id,
            &target.generation.to_string(),
            &event_seq.to_string(),
        ],
    )
}

/// The `cspc-key` tag value: the identity of one catalog advertisement.
///
/// Includes a SHA-256 of the exact signed content, not just the revision. A
/// producer that bumped its revision without changing anything, or reused a
/// revision with different content, is then visibly distinguishable to a
/// consumer rather than silently collapsing into one entry.
pub fn coding_session_provider_catalog_semantic_key(
    channel_id: &str,
    revision: u64,
    content: &str,
) -> String {
    let digest = hex::encode(Sha256::digest(content.as_bytes()));
    encode_structured_key(
        "coding-session-provider-catalog/v1",
        &[channel_id, &revision.to_string(), &digest],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "provider-a".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation: 1,
        }
    }

    /// Golden vector. This exact string is the cross-implementation contract
    /// between the Rust producer and the TypeScript consumer; if it changes,
    /// every stored 442xx event's routing tag changes with it.
    #[test]
    fn cs_target_key_matches_the_cross_implementation_golden_vector() {
        assert_eq!(
            coding_session_target_key(&target()),
            "coding-session/v1|10:provider-a10:instance-19:session-11:1"
        );
    }

    #[test]
    fn cs_target_key_agrees_with_the_buzz_core_definition() {
        let target = target();
        assert_eq!(
            coding_session_target_key(&target),
            buzz_core::coding_session_command::coding_session_target_key(&target)
        );
    }

    #[test]
    fn semantic_keys_are_domain_separated() {
        let target = target();
        let keys = [
            coding_session_target_key(&target),
            coding_session_metadata_semantic_key(&target),
            coding_session_transcript_semantic_key(&target, 1),
            coding_session_lifecycle_receipt_semantic_key("create-1"),
            coding_session_provider_catalog_semantic_key("channel", 1, "{}"),
        ];
        for (i, a) in keys.iter().enumerate() {
            for b in keys.iter().skip(i + 1) {
                assert_ne!(a, b, "semantic keys must not collide across domains");
            }
        }
        assert!(coding_session_metadata_semantic_key(&target)
            .starts_with("coding-session-metadata/v1|"));
    }

    /// The reason the encoding is length-prefixed rather than delimiter-joined:
    /// a session id containing the delimiter must not be able to forge the key
    /// of a different session.
    #[test]
    fn length_prefixing_prevents_field_boundary_forgery() {
        let honest = CodingSessionTarget {
            driver: "a".into(),
            instance_id: "b".into(),
            session_id: "c".into(),
            generation: 1,
        };
        let forger = CodingSessionTarget {
            driver: "a".into(),
            instance_id: "b:c".into(),
            session_id: String::new(),
            generation: 1,
        };
        assert_ne!(
            coding_session_target_key(&honest),
            coding_session_target_key(&forger)
        );
    }

    #[test]
    fn transcript_keys_are_unique_per_sequence_and_generation() {
        let mut target = target();
        let seq_1 = coding_session_transcript_semantic_key(&target, 1);
        let seq_2 = coding_session_transcript_semantic_key(&target, 2);
        assert_ne!(seq_1, seq_2);
        target.generation = 2;
        assert_ne!(seq_1, coding_session_transcript_semantic_key(&target, 1));
    }

    /// Byte length, not character count — a multi-byte driver slug must produce
    /// the same prefix the TextEncoder-based consumer produces.
    #[test]
    fn field_lengths_are_utf8_bytes() {
        assert_eq!(encode_structured_key("d", &["é"]), "d|2:é");
        assert_eq!(encode_structured_key("d", &["🐝"]), "d|4:🐝");
        assert_eq!(encode_structured_key("d", &[]), "d|");
        assert_eq!(encode_structured_key("d", &[""]), "d|0:");
    }

    #[test]
    fn catalog_key_changes_with_content_at_the_same_revision() {
        let a = coding_session_provider_catalog_semantic_key("channel", 3, r#"{"a":1}"#);
        let b = coding_session_provider_catalog_semantic_key("channel", 3, r#"{"a":2}"#);
        assert_ne!(a, b);
        assert_eq!(
            a,
            coding_session_provider_catalog_semantic_key("channel", 3, r#"{"a":1}"#)
        );
        // Empty-string SHA-256, pinned so a digest-encoding change is caught.
        assert_eq!(
            coding_session_provider_catalog_semantic_key("c", 1, ""),
            "coding-session-provider-catalog/v1|1:c1:164:\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
