//! Coding-session tag versions and semantic-key derivation.
//!
//! Durable provider-fact coding-session events carry a *semantic key* tag — a
//! deterministic, collision-free encoding of the identity the event claims.
//! Consumers use it to deduplicate and fence stale writers; producers use it as
//! the idempotency key of an outbox row. Ephemeral kind-24223 leases instead use
//! an exact target plus a monotonic `cslease-seq` register. Because both sides
//! derive target and semantic keys independently, the encoding has to agree
//! byte-for-byte across Rust producers and TypeScript consumers.
//!
//! The encoding is length-prefixed, which is the whole point: joining fields
//! with a separator lets a field containing that separator impersonate a
//! different tuple, and session ids and driver slugs are attacker-adjacent
//! strings. `10:instance-1` cannot be confused with anything else.
//!
//! Mirrors the donor's `encodeStructuredKey`
//! (`desktop/src/features/agents/ui/buzzSessionProjectionKeys.ts`).

use nostr::{EventBuilder, Kind, Tag};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::kind::{
    event_kind_u32, KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_PROVIDER_PRESENCE,
};

use crate::SdkError;

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

// ── Provider presence (kind 24224, NIP-CSL § Provider presence) ─────────────

/// The only supported provider-presence payload schema.
pub const CODING_SESSION_PROVIDER_PRESENCE_SCHEMA: &str =
    "buzz-coding-session-provider-presence/v1";
/// Version tag value (`cspp-v`) on every provider-presence event (kind 24224).
pub const CODING_SESSION_PROVIDER_PRESENCE_TAG_VERSION: &str = "cspp1-1";
/// Maximum signed content bytes for a provider-presence event.
pub const MAX_PROVIDER_PRESENCE_CONTENT_BYTES: usize = 8 * 1024;
/// Default presence time-to-live, in seconds. The provider publishes every
/// `ttlSecs / 3`, so two missed heartbeats still read as live.
pub const DEFAULT_PROVIDER_PRESENCE_TTL_SECS: u64 = 90;
/// Ceiling on a `ttlSecs` a reader accepts; a larger value cannot make an
/// absent provider look present for longer than this.
pub const MAX_PROVIDER_PRESENCE_TTL_SECS: u64 = 3600;
/// Ceiling on `liveTargets` entries in one presence event.
pub const MAX_PROVIDER_PRESENCE_LIVE_TARGETS: usize = 256;

/// What the provider is doing at the moment it heartbeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPresenceState {
    /// Running, no turn in flight on any execution in this channel.
    Idle,
    /// At least one turn is in flight in this channel.
    Busy,
}

impl ProviderPresenceState {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
        }
    }
}

/// Strict signed content of a provider-presence event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPresence {
    /// Must equal [`CODING_SESSION_PROVIDER_PRESENCE_SCHEMA`].
    pub schema: String,
    /// The provider instance heartbeating (the `instanceId` in its targets).
    pub instance_id: String,
    /// Monotonic per-process heartbeat counter; a reader keeps the highest.
    pub seq: u64,
    /// Seconds after `created_at` during which this heartbeat counts as live.
    pub ttl_secs: u64,
    /// Idle or busy at the moment of the heartbeat.
    pub state: ProviderPresenceState,
    /// Exact generations with a live actor in this channel at the heartbeat.
    pub live_targets: Vec<CodingSessionTarget>,
}

impl ProviderPresence {
    /// Construct and validate a presence payload suitable for signing.
    pub fn new(
        instance_id: &str,
        seq: u64,
        ttl_secs: u64,
        state: ProviderPresenceState,
        live_targets: Vec<CodingSessionTarget>,
    ) -> Result<Self, String> {
        let presence = Self {
            schema: CODING_SESSION_PROVIDER_PRESENCE_SCHEMA.to_owned(),
            instance_id: instance_id.to_owned(),
            seq,
            ttl_secs,
            state,
            live_targets,
        };
        presence.validate()?;
        Ok(presence)
    }

    /// Validate the schema, identifiers and bounds of this payload.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_PROVIDER_PRESENCE_SCHEMA {
            return Err("unsupported provider-presence schema".into());
        }
        if self.instance_id.trim().is_empty() || self.instance_id.len() > 256 {
            return Err(
                "provider-presence instanceId must be a nonempty bounded identifier".into(),
            );
        }
        if self.seq == 0 {
            return Err("provider-presence seq must be positive".into());
        }
        if self.ttl_secs == 0 || self.ttl_secs > MAX_PROVIDER_PRESENCE_TTL_SECS {
            return Err(format!(
                "provider-presence ttlSecs must be 1..={MAX_PROVIDER_PRESENCE_TTL_SECS}"
            ));
        }
        if self.live_targets.len() > MAX_PROVIDER_PRESENCE_LIVE_TARGETS {
            return Err("provider-presence liveTargets exceeds the ceiling".into());
        }
        if self
            .live_targets
            .iter()
            .any(|target| target.instance_id != self.instance_id)
        {
            return Err("provider-presence liveTargets must belong to instanceId".into());
        }
        Ok(())
    }
}

/// Strictly decode signed provider-presence content.
pub fn decode_provider_presence(content: &str) -> Result<ProviderPresence, String> {
    if content.len() > MAX_PROVIDER_PRESENCE_CONTENT_BYTES {
        return Err(format!(
            "provider-presence content exceeds {MAX_PROVIDER_PRESENCE_CONTENT_BYTES} bytes"
        ));
    }
    let presence: ProviderPresence = serde_json::from_str(content)
        .map_err(|error| format!("malformed provider-presence payload: {error}"))?;
    presence.validate()?;
    Ok(presence)
}

/// A presence event read back from the relay, with its freshness judged
/// against the reader's clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedProviderPresence {
    /// Decoded and validated signed content.
    pub payload: ProviderPresence,
    /// Canonical channel UUID from the `h` tag.
    pub channel_id: Uuid,
    /// Whether `created_at + ttlSecs` is still ahead of the reader's clock.
    pub live: bool,
}

/// Validate a kind-24224 event's kind, ordered tags and content, and judge
/// its freshness at `now` (Unix seconds). Signature verification is the
/// caller's: this checks shape, not authorship.
pub fn validate_provider_presence_envelope(
    event: &nostr::Event,
    now: u64,
) -> Result<ValidatedProviderPresence, String> {
    if event_kind_u32(event) != KIND_CODING_SESSION_PROVIDER_PRESENCE {
        return Err("event is not a provider-presence heartbeat (kind 24224)".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    let [h, version, instance, seq] = tags.as_slice() else {
        return Err("provider-presence event must carry exactly four tags".into());
    };
    let channel_id = match h {
        [name, value] if name == "h" => Uuid::parse_str(value)
            .ok()
            .filter(|parsed| parsed.to_string() == *value)
            .ok_or_else(|| "provider-presence h tag must be a canonical UUID".to_owned())?,
        _ => return Err("provider-presence first tag must be h".into()),
    };
    if *version != ["cspp-v", CODING_SESSION_PROVIDER_PRESENCE_TAG_VERSION] {
        return Err("provider-presence cspp-v tag is not cspp1-1".into());
    }
    let payload = decode_provider_presence(&event.content)?;
    if *instance != ["cspp-instance", payload.instance_id.as_str()] {
        return Err("provider-presence cspp-instance tag does not match content".into());
    }
    if *seq != ["cspp-seq", payload.seq.to_string().as_str()] {
        return Err("provider-presence cspp-seq tag does not match content".into());
    }
    let created_at = event.created_at.as_secs();
    let live = created_at.saturating_add(payload.ttl_secs) > now;
    Ok(ValidatedProviderPresence {
        payload,
        channel_id,
        live,
    })
}

/// Build one provider-presence heartbeat (kind 24224) for `channel_id`.
pub fn build_coding_session_provider_presence(
    channel_id: Uuid,
    presence: &ProviderPresence,
) -> Result<EventBuilder, SdkError> {
    presence.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(presence).map_err(|error| {
        SdkError::InvalidInput(format!("provider-presence serialization: {error}"))
    })?;
    if content.len() > MAX_PROVIDER_PRESENCE_CONTENT_BYTES {
        return Err(SdkError::ContentTooLarge {
            max: MAX_PROVIDER_PRESENCE_CONTENT_BYTES,
            got: content.len(),
        });
    }
    let tags = [
        vec!["h".to_owned(), channel_id.to_string()],
        vec![
            "cspp-v".to_owned(),
            CODING_SESSION_PROVIDER_PRESENCE_TAG_VERSION.to_owned(),
        ],
        vec!["cspp-instance".to_owned(), presence.instance_id.clone()],
        vec!["cspp-seq".to_owned(), presence.seq.to_string()],
    ]
    .into_iter()
    .map(|parts| Tag::parse(parts).map_err(|error| SdkError::InvalidInput(error.to_string())))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_PROVIDER_PRESENCE as u16),
        content,
    )
    .tags(tags))
}

/// The latest-ephemeral key one provider instance's heartbeat replaces under
/// in one channel: a newer heartbeat supersedes an older unsent one.
pub fn coding_session_provider_presence_semantic_key(
    channel_id: Uuid,
    instance_id: &str,
) -> String {
    encode_structured_key(
        "coding-session-provider-presence/v1",
        &[&channel_id.to_string(), instance_id],
    )
}

// ── Receipt vocabulary (NIP-CSL § turn_interrupted, NIP-CSE) ────────────────
//
// Defined beside the other codes in `beekeeper-core/src/coding_session_payload.rs`
// and re-exported here so the provider's existing imports keep resolving.
pub use beekeeper_core::coding_session_payload::{
    CREDENTIAL_SOURCE_AMBIENT_LOGIN, MODEL_AUTH_FAILED, QUIESCENCE_UNCONFIRMED,
    RECEIPT_QUIESCENCE_FIELD, TURN_INTERRUPTED_BY_RESTART, TURN_INTERRUPTED_STATUS,
};

/// The `csl-key` for a turn-stage receipt whose status is given as a wire
/// string. Same encoding as `coding_session_turn_receipt_semantic_key`, for
/// statuses the closed `ReceiptStatus` enum does not yet carry.
pub fn coding_session_turn_receipt_semantic_key_str(command_id: &str, status: &str) -> String {
    encode_structured_key("coding-session-lifecycle-receipt/v1", &[command_id, status])
}

/// Build a `turn_interrupted` receipt (kind 44224): the five receipt keys
/// plus `quiescence: "unconfirmed"`, under the turn-stage `csl-key`.
///
/// The content is `LifecycleReceipt::turn_interrupted` and round-trips through
/// the strict core decoder. Documented in NIP-CSL § turn_interrupted.
pub fn build_coding_session_turn_interrupted_receipt(
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
) -> Result<EventBuilder, SdkError> {
    if command_id.trim().is_empty() || command_id.len() > 256 {
        return Err(SdkError::InvalidInput(
            "commandId must be a nonempty bounded identifier".into(),
        ));
    }
    let content = serde_json::to_string(
        &beekeeper_core::coding_session_payload::LifecycleReceipt::turn_interrupted(
            command_id, target,
        ),
    )
    .map_err(|error| SdkError::InvalidInput(format!("receipt serialization: {error}")))?;
    let tags = [
        vec!["h".to_owned(), channel_id.to_string()],
        vec![
            "cslr-v".to_owned(),
            CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION.to_owned(),
        ],
        vec!["csl-command".to_owned(), command_id.to_owned()],
        vec![
            "csl-key".to_owned(),
            coding_session_turn_receipt_semantic_key_str(command_id, TURN_INTERRUPTED_STATUS),
        ],
    ]
    .into_iter()
    .map(|parts| Tag::parse(parts).map_err(|error| SdkError::InvalidInput(error.to_string())))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_LIFECYCLE_RECEIPT as u16),
        content,
    )
    .tags(tags))
}

/// Encode a deterministic, unambiguous structured key.
///
/// Each field is prefixed with its UTF-8 byte length and a colon, then all
/// fields are concatenated after `<domain>|`. There is no separator between
/// fields — the length prefix is the separator, which is what makes the
/// encoding injective over arbitrary field content.
///
/// ```
/// use beekeeper_sdk::coding_session::encode_structured_key;
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
/// Identical to [`beekeeper_core::coding_session_command::coding_session_target_key`];
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

/// The `csm-key` tag value: the grouping identity for one generation's
/// metadata observations.
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
    fn cs_target_key_agrees_with_the_beekeeper_core_definition() {
        let target = target();
        assert_eq!(
            coding_session_target_key(&target),
            beekeeper_core::coding_session_command::coding_session_target_key(&target)
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
    fn presence_round_trips_and_judges_freshness_from_created_at_plus_ttl() {
        let keys = nostr::Keys::generate();
        let channel = Uuid::new_v4();
        let mut target = target();
        target.instance_id = "instance-1".into();
        let presence = ProviderPresence::new(
            "instance-1",
            7,
            90,
            ProviderPresenceState::Busy,
            vec![target.clone()],
        )
        .expect("presence");
        let event = build_coding_session_provider_presence(channel, &presence)
            .expect("builder")
            .sign_with_keys(&keys)
            .expect("signed");
        assert_eq!(
            u32::from(event.kind.as_u16()),
            KIND_CODING_SESSION_PROVIDER_PRESENCE
        );
        let tags: Vec<Vec<String>> = event.tags.iter().map(|t| t.as_slice().to_vec()).collect();
        assert_eq!(tags[0], ["h", &channel.to_string()]);
        assert_eq!(tags[1], ["cspp-v", "cspp1-1"]);
        assert_eq!(tags[2], ["cspp-instance", "instance-1"]);
        assert_eq!(tags[3], ["cspp-seq", "7"]);
        let content: serde_json::Value = serde_json::from_str(&event.content).expect("json");
        assert_eq!(content["schema"], CODING_SESSION_PROVIDER_PRESENCE_SCHEMA);
        assert_eq!(content["state"], "busy");
        assert_eq!(content["ttlSecs"], 90);
        assert_eq!(content["liveTargets"][0]["sessionId"], "session-1");

        let at = event.created_at.as_secs();
        let fresh = validate_provider_presence_envelope(&event, at + 89).expect("fresh");
        assert!(fresh.live);
        assert_eq!(fresh.channel_id, channel);
        assert_eq!(fresh.payload, presence);
        let stale = validate_provider_presence_envelope(&event, at + 90).expect("stale");
        assert!(!stale.live, "ttl is exclusive at the boundary");
    }

    #[test]
    fn presence_refuses_foreign_targets_zero_seq_and_wrong_tags() {
        let foreign = target();
        assert!(
            ProviderPresence::new("other", 1, 90, ProviderPresenceState::Idle, vec![foreign])
                .is_err()
        );
        assert!(ProviderPresence::new("i", 0, 90, ProviderPresenceState::Idle, vec![]).is_err());
        assert!(ProviderPresence::new("i", 1, 0, ProviderPresenceState::Idle, vec![]).is_err());
        assert!(ProviderPresence::new("i", 1, 3601, ProviderPresenceState::Idle, vec![]).is_err());
        let keys = nostr::Keys::generate();
        let presence =
            ProviderPresence::new("i", 2, 90, ProviderPresenceState::Idle, vec![]).expect("ok");
        let forged = EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_PROVIDER_PRESENCE as u16),
            serde_json::to_string(&presence).expect("json"),
        )
        .tags([
            Tag::parse(["h", &Uuid::new_v4().to_string()]).expect("h"),
            Tag::parse(["cspp-v", "cspp1-1"]).expect("v"),
            Tag::parse(["cspp-instance", "i"]).expect("i"),
            Tag::parse(["cspp-seq", "3"]).expect("seq"),
        ])
        .sign_with_keys(&keys)
        .expect("signed");
        assert!(validate_provider_presence_envelope(&forged, 0)
            .expect_err("seq tag disagrees with content")
            .contains("cspp-seq"));
    }

    #[test]
    fn turn_interrupted_receipt_carries_the_sixth_key_under_a_turn_stage_csl_key() {
        let keys = nostr::Keys::generate();
        let channel = Uuid::new_v4();
        let event = build_coding_session_turn_interrupted_receipt(channel, "turn-1", &target())
            .expect("builder")
            .sign_with_keys(&keys)
            .expect("signed");
        let content: serde_json::Value = serde_json::from_str(&event.content).expect("json");
        assert_eq!(content["status"], TURN_INTERRUPTED_STATUS);
        assert_eq!(content["quiescence"], QUIESCENCE_UNCONFIRMED);
        assert_eq!(content["commandId"], "turn-1");
        assert!(content["error"].is_null());
        assert_eq!(content["session"]["sessionId"], "session-1");
        assert_eq!(
            content.as_object().map(|o| o.len()),
            Some(6),
            "five receipt keys plus quiescence"
        );
        let key = event
            .tags
            .iter()
            .find(|t| t.as_slice().first().is_some_and(|n| n == "csl-key"))
            .expect("csl-key")
            .as_slice()[1]
            .clone();
        assert_eq!(
            key,
            encode_structured_key(
                "coding-session-lifecycle-receipt/v1",
                &["turn-1", "turn_interrupted"]
            )
        );
        // The closed core decoder accepts the status and its sixth key.
        let decoded =
            beekeeper_core::coding_session_payload::decode_coding_session_lifecycle_receipt(
                &event.content,
            )
            .expect("the strict decoder accepts turn_interrupted");
        assert_eq!(
            decoded.status,
            beekeeper_core::coding_session_payload::ReceiptStatus::TurnInterrupted
        );
        assert_eq!(decoded.quiescence.as_deref(), Some(QUIESCENCE_UNCONFIRMED));
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
