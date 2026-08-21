//! Ephemeral provider-signed liveness leases for exact coding-session generations.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionTarget, CODING_SESSION_COMMAND_SCHEMA, MAX_IDENTIFIER_BYTES, MAX_SAFE_GENERATION,
};
use crate::kind::{event_kind_u32, KIND_CODING_SESSION_LEASE};

/// The only supported coding-session lease payload schema.
pub const CODING_SESSION_LEASE_SCHEMA: &str = "buzz-coding-session-lease/v1";
/// The required `cslease-v` tag value for kind-24223 events.
pub const CODING_SESSION_LEASE_TAG_VERSION: &str = "cslease1-1";
/// Maximum UTF-8 byte length of signed lease content.
pub const MAX_CODING_SESSION_LEASE_CONTENT_BYTES: usize = 2 * 1024;
/// Maximum age accepted for a newly observed lease event.
pub const CODING_SESSION_LEASE_REPLAY_WINDOW_SECS: u64 = 180;
/// Maximum provider-clock lead accepted relative to relay time.
pub const CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS: u64 = 30;

const LEASE_FIELDS: [&str; 4] = ["schema", "target", "state", "leaseSequence"];
const TARGET_FIELDS: [&str; 4] = ["driver", "instanceId", "sessionId", "generation"];

/// Provider-asserted state for one exact session-generation lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingSessionLeaseState {
    /// The provider currently owns a reachable live actor for this generation.
    Live,
    /// The provider intentionally relinquished liveness for this generation.
    Released,
}

/// Strict signed content of an ephemeral coding-session lease event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionLease {
    /// Must equal [`CODING_SESSION_LEASE_SCHEMA`].
    pub schema: String,
    /// The exact provider session generation whose liveness is asserted.
    pub target: CodingSessionTarget,
    /// Whether the lease asserts liveness or explicitly releases it.
    pub state: CodingSessionLeaseState,
    /// Positive JavaScript-safe sequence, monotonically reserved by the provider.
    pub lease_sequence: u64,
}

impl CodingSessionLease {
    /// Construct and validate a lease payload suitable for signing.
    pub fn new(
        target: CodingSessionTarget,
        state: CodingSessionLeaseState,
        lease_sequence: u64,
    ) -> Result<Self, String> {
        let lease = Self {
            schema: CODING_SESSION_LEASE_SCHEMA.to_owned(),
            target,
            state,
            lease_sequence,
        };
        lease.validate()?;
        Ok(lease)
    }

    /// Validate the schema, target, and sequence bounds of this payload.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_LEASE_SCHEMA {
            return Err("unsupported coding-session lease schema".into());
        }
        validate_target(&self.target)?;
        if self.lease_sequence == 0 || self.lease_sequence > MAX_SAFE_GENERATION {
            return Err("leaseSequence must be a positive safe integer".into());
        }
        Ok(())
    }
}

/// Strictly validated lease plus the filterable authority inputs from its tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedCodingSessionLease {
    /// Decoded and validated signed lease content.
    pub payload: CodingSessionLease,
    /// Canonical channel UUID from the first `h` tag.
    pub channel_id: Uuid,
    /// Lifecycle command identifier that minted the exact generation.
    pub command_id: String,
}

/// Strictly decode and validate signed coding-session lease content.
///
/// The byte cap is enforced before JSON parsing. The first decode checks the
/// exact closed field sets; the second decode preserves serde's duplicate-key
/// rejection and enforces the field types.
pub fn decode_coding_session_lease(content: &str) -> Result<CodingSessionLease, String> {
    if content.len() > MAX_CODING_SESSION_LEASE_CONTENT_BYTES {
        return Err(format!(
            "coding-session lease content exceeds {MAX_CODING_SESSION_LEASE_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session lease payload".to_owned())?;
    require_exact_fields(&value, &LEASE_FIELDS, "payload")?;
    let target = value
        .get("target")
        .ok_or_else(|| "coding-session lease payload missing target".to_owned())?;
    require_exact_fields(target, &TARGET_FIELDS, "target")?;

    let lease: CodingSessionLease = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session lease payload: {error}"))?;
    lease.validate()?;
    Ok(lease)
}

/// Validate a lease event's kind, replay window, exact ordered tags, and content.
///
/// `relay_now` is an explicit relay clock reading in Unix seconds. It is used
/// only for first-acceptance replay and future-skew checks; Redis expiry is
/// derived separately from relay acceptance time and must never use the signed
/// provider timestamp.
pub fn validate_coding_session_lease_envelope(
    event: &nostr::Event,
    relay_now: u64,
) -> Result<ValidatedCodingSessionLease, String> {
    if event_kind_u32(event) != KIND_CODING_SESSION_LEASE {
        return Err("event is not a coding-session lease (kind 24223)".into());
    }
    let issued_at = event.created_at.as_secs();
    if issued_at > relay_now.saturating_add(CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS) {
        return Err("coding-session lease timestamp exceeds allowed future skew".into());
    }
    if issued_at <= relay_now && relay_now - issued_at > CODING_SESSION_LEASE_REPLAY_WINDOW_SECS {
        return Err("coding-session lease is outside the first-acceptance replay window".into());
    }

    let lease = decode_coding_session_lease(&event.content)?;
    if event.tags.len() != 5 {
        return Err("coding-session lease requires exactly five ordered tags".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session lease tags must have exactly two fields".into());
    }
    for (index, expected) in ["h", "cslease-v", "cs-target", "csl-command", "cslease-seq"]
        .iter()
        .enumerate()
    {
        if tags[index][0] != *expected {
            return Err("coding-session lease tags are missing or out of order".into());
        }
    }
    if tags[1][1] != CODING_SESSION_LEASE_TAG_VERSION {
        return Err("unsupported coding-session lease tag version".into());
    }

    let channel_id = Uuid::parse_str(&tags[0][1])
        .map_err(|_| "coding-session lease h tag must be a canonical channel UUID".to_owned())?;
    if channel_id.to_string() != tags[0][1] {
        return Err("coding-session lease h tag must be a canonical channel UUID".into());
    }
    let expected_target = coding_session_target_key(&lease.target);
    if tags[2][1] != expected_target {
        return Err("coding-session lease cs-target tag does not match content target".into());
    }
    validate_identifier(&tags[3][1], "csl-command")?;
    if tags[4][1] != lease.lease_sequence.to_string() {
        return Err("coding-session lease cslease-seq tag does not match content sequence".into());
    }

    Ok(ValidatedCodingSessionLease {
        payload: lease,
        channel_id,
        command_id: tags[3][1].clone(),
    })
}

fn require_exact_fields(value: &Value, expected: &[&str], label: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("coding-session lease {label} must be an object"))?;
    if expected.iter().any(|key| !object.contains_key(*key))
        || object.keys().any(|key| !expected.contains(&key.as_str()))
    {
        return Err(format!(
            "coding-session lease {label} has missing or unsupported fields"
        ));
    }
    Ok(())
}

fn validate_target(target: &CodingSessionTarget) -> Result<(), String> {
    CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: "lease-validation".to_owned(),
        target: target.clone(),
        action: CodingSessionAction::ThreadTurnInterrupt,
    }
    .validate()
}

fn validate_identifier(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(format!("{field} exceeds {MAX_IDENTIFIER_BYTES} bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_session_command::{CodingSessionTarget, MAX_SAFE_GENERATION};
    use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};
    use uuid::Uuid;

    const RELAY_NOW: u64 = 1_785_600_000;

    fn target() -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "provider-instance".into(),
            session_id: "provider-session-id".into(),
            generation: 1,
        }
    }

    fn payload(state: CodingSessionLeaseState, lease_sequence: u64) -> CodingSessionLease {
        CodingSessionLease {
            schema: CODING_SESSION_LEASE_SCHEMA.into(),
            target: target(),
            state,
            lease_sequence,
        }
    }

    fn tag(parts: &[&str]) -> Tag {
        Tag::parse(parts.iter().copied()).expect("valid test tag")
    }

    fn tags(
        channel: Uuid,
        target: &CodingSessionTarget,
        command_id: &str,
        sequence: &str,
    ) -> Vec<Tag> {
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["cslease-v", CODING_SESSION_LEASE_TAG_VERSION]),
            tag(&[
                "cs-target",
                &crate::coding_session_command::coding_session_target_key(target),
            ]),
            tag(&["csl-command", command_id]),
            tag(&["cslease-seq", sequence]),
        ]
    }

    fn event(content: String, tags: Vec<Tag>, created_at: u64) -> Event {
        EventBuilder::new(Kind::Custom(24_223), content)
            .tags(tags)
            .custom_created_at(Timestamp::from_secs(created_at))
            .sign_with_keys(&Keys::generate())
            .expect("sign test event")
    }

    fn valid_event(state: CodingSessionLeaseState, sequence: u64, created_at: u64) -> Event {
        let payload = payload(state, sequence);
        let channel =
            Uuid::parse_str("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10").expect("valid channel");
        event(
            serde_json::to_string(&payload).expect("serialize lease"),
            tags(channel, &payload.target, "create-1", &sequence.to_string()),
            created_at,
        )
    }

    #[test]
    fn strictly_decodes_both_lease_states() {
        for state in [
            CodingSessionLeaseState::Live,
            CodingSessionLeaseState::Released,
        ] {
            let expected = payload(state, 42);
            let content = serde_json::to_string(&expected).expect("serialize lease");
            assert_eq!(decode_coding_session_lease(&content).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_unknown_missing_duplicate_and_nested_unknown_content_fields() {
        let valid = serde_json::json!({
            "schema": CODING_SESSION_LEASE_SCHEMA,
            "target": {
                "driver": "codex-acp",
                "instanceId": "provider-instance",
                "sessionId": "provider-session-id",
                "generation": 1,
            },
            "state": "live",
            "leaseSequence": 42,
        });

        let mut unknown = valid.clone();
        unknown["trusted"] = serde_json::json!(true);
        assert!(decode_coding_session_lease(&unknown.to_string()).is_err());

        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove("state");
        assert!(decode_coding_session_lease(&missing.to_string()).is_err());

        let duplicate = format!(
            r#"{{"schema":"{schema}","schema":"{schema}","target":{{"driver":"codex-acp","instanceId":"provider-instance","sessionId":"provider-session-id","generation":1}},"state":"live","leaseSequence":42}}"#,
            schema = CODING_SESSION_LEASE_SCHEMA,
        );
        assert!(decode_coding_session_lease(&duplicate).is_err());

        let mut nested_unknown = valid;
        nested_unknown["target"]["authority"] = serde_json::json!("self");
        assert!(decode_coding_session_lease(&nested_unknown.to_string()).is_err());
    }

    #[test]
    fn rejects_wrong_schema_state_target_and_sequence_bounds() {
        let mut value = serde_json::to_value(payload(CodingSessionLeaseState::Live, 1)).unwrap();
        value["schema"] = serde_json::json!("buzz-coding-session-lease/v2");
        assert!(decode_coding_session_lease(&value.to_string()).is_err());

        value = serde_json::to_value(payload(CodingSessionLeaseState::Live, 1)).unwrap();
        value["state"] = serde_json::json!("idle");
        assert!(decode_coding_session_lease(&value.to_string()).is_err());

        value = serde_json::to_value(payload(CodingSessionLeaseState::Live, 1)).unwrap();
        value["target"]["sessionId"] = serde_json::json!("  ");
        assert!(decode_coding_session_lease(&value.to_string()).is_err());

        for invalid in [0, MAX_SAFE_GENERATION + 1] {
            value = serde_json::to_value(payload(CodingSessionLeaseState::Live, invalid)).unwrap();
            assert!(decode_coding_session_lease(&value.to_string()).is_err());
        }
        assert!(decode_coding_session_lease(
            &serde_json::to_string(&payload(CodingSessionLeaseState::Live, MAX_SAFE_GENERATION,))
                .unwrap()
        )
        .is_ok());
    }

    #[test]
    fn enforces_content_cap_before_decode() {
        let at_cap = " ".repeat(MAX_CODING_SESSION_LEASE_CONTENT_BYTES);
        assert!(!decode_coding_session_lease(&at_cap)
            .unwrap_err()
            .contains("exceeds"));
        let over_cap = " ".repeat(MAX_CODING_SESSION_LEASE_CONTENT_BYTES + 1);
        assert!(decode_coding_session_lease(&over_cap)
            .unwrap_err()
            .contains("exceeds"));
    }

    #[test]
    fn validates_the_exact_ordered_envelope_and_returns_authority_inputs() {
        let event = valid_event(CodingSessionLeaseState::Live, 42, RELAY_NOW);
        let lease = validate_coding_session_lease_envelope(&event, RELAY_NOW).unwrap();
        assert_eq!(lease.payload.lease_sequence, 42);
        assert_eq!(lease.payload.target, target());
        assert_eq!(lease.command_id, "create-1");
        assert_eq!(
            lease.channel_id.to_string(),
            "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"
        );
    }

    #[test]
    fn rejects_wrong_order_shape_kind_version_and_unknown_tags() {
        let base = valid_event(CodingSessionLeaseState::Live, 42, RELAY_NOW);
        let mut wrong_order: Vec<Tag> = base.tags.iter().cloned().collect();
        wrong_order.swap(1, 2);
        let candidate = event(base.content.clone(), wrong_order, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());

        let mut unknown: Vec<Tag> = base.tags.iter().cloned().collect();
        unknown.push(tag(&["nonce", "1"]));
        let candidate = event(base.content.clone(), unknown, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());

        let mut long_tag: Vec<Tag> = base.tags.iter().cloned().collect();
        long_tag[4] = tag(&["cslease-seq", "42", "extra"]);
        let candidate = event(base.content.clone(), long_tag, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());

        let wrong_kind = EventBuilder::new(Kind::Custom(44_223), base.content.clone())
            .tags(base.tags.iter().cloned())
            .custom_created_at(Timestamp::from_secs(RELAY_NOW))
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert!(validate_coding_session_lease_envelope(&wrong_kind, RELAY_NOW).is_err());

        let mut wrong_version: Vec<Tag> = base.tags.iter().cloned().collect();
        wrong_version[1] = tag(&["cslease-v", "cslease1-2"]);
        let candidate = event(base.content, wrong_version, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());
    }

    #[test]
    fn rejects_tag_content_mismatch_and_noncanonical_values() {
        let base = valid_event(CodingSessionLeaseState::Live, 42, RELAY_NOW);

        let mut target_mismatch: Vec<Tag> = base.tags.iter().cloned().collect();
        target_mismatch[2] = tag(&["cs-target", "coding-session/v1|wrong"]);
        let candidate = event(base.content.clone(), target_mismatch, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());

        let mut sequence_mismatch: Vec<Tag> = base.tags.iter().cloned().collect();
        sequence_mismatch[4] = tag(&["cslease-seq", "41"]);
        let candidate = event(base.content.clone(), sequence_mismatch, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());

        let mut leading_zero: Vec<Tag> = base.tags.iter().cloned().collect();
        leading_zero[4] = tag(&["cslease-seq", "042"]);
        let candidate = event(base.content.clone(), leading_zero, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());

        let mut bad_channel: Vec<Tag> = base.tags.iter().cloned().collect();
        bad_channel[0] = tag(&["h", "5B7E1C2A-90D4-4B0E-A1F3-7C2D8E6F4A10"]);
        let candidate = event(base.content.clone(), bad_channel, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());

        let mut empty_command: Vec<Tag> = base.tags.iter().cloned().collect();
        empty_command[3] = tag(&["csl-command", "  "]);
        let candidate = event(base.content.clone(), empty_command, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());

        let mut long_command: Vec<Tag> = base.tags.iter().cloned().collect();
        long_command[3] = tag(&["csl-command", &"x".repeat(MAX_IDENTIFIER_BYTES + 1)]);
        let candidate = event(base.content, long_command, RELAY_NOW);
        assert!(validate_coding_session_lease_envelope(&candidate, RELAY_NOW).is_err());
    }

    #[test]
    fn replay_window_and_future_skew_use_explicit_relay_time() {
        assert!(validate_coding_session_lease_envelope(
            &valid_event(
                CodingSessionLeaseState::Live,
                1,
                RELAY_NOW - CODING_SESSION_LEASE_REPLAY_WINDOW_SECS,
            ),
            RELAY_NOW,
        )
        .is_ok());
        assert!(validate_coding_session_lease_envelope(
            &valid_event(
                CodingSessionLeaseState::Live,
                1,
                RELAY_NOW - CODING_SESSION_LEASE_REPLAY_WINDOW_SECS - 1,
            ),
            RELAY_NOW,
        )
        .is_err());
        assert!(validate_coding_session_lease_envelope(
            &valid_event(
                CodingSessionLeaseState::Live,
                1,
                RELAY_NOW + CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS,
            ),
            RELAY_NOW,
        )
        .is_ok());
        assert!(validate_coding_session_lease_envelope(
            &valid_event(
                CodingSessionLeaseState::Live,
                1,
                RELAY_NOW + CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS + 1,
            ),
            RELAY_NOW,
        )
        .is_err());
    }
}
