//! Provider-neutral coding-session command contract.
//!
//! Events use [`crate::kind::KIND_CODING_SESSION_COMMAND`] and public JSON so
//! an installed provider adapter can consume signed operator intent. Event
//! authorship is the authority; content carries no claimed actor identity.

use serde::{Deserialize, Serialize};

/// The only currently supported coding-session command payload schema.
pub const CODING_SESSION_COMMAND_SCHEMA: &str = "buzz-coding-session-command/v1";
/// The version tag placed on each coding-session command event.
pub const CODING_SESSION_COMMAND_TAG_VERSION: &str = "csc1-1";
/// Maximum UTF-8 byte length for a command identifier or target identifier.
pub const MAX_IDENTIFIER_BYTES: usize = 256;
/// Maximum UTF-8 byte length for turn text.
pub const MAX_TURN_TEXT_BYTES: usize = 12 * 1024;
/// Largest integer that can be represented exactly by JavaScript and JSON peers.
pub const MAX_SAFE_GENERATION: u64 = 9_007_199_254_740_991;

/// Provider-neutral target for a coding-session command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTarget {
    /// Capability-advertised provider driver slug; this is intentionally open.
    pub driver: String,
    /// Provider instance identifier.
    pub instance_id: String,
    /// Provider session identifier.
    pub session_id: String,
    /// Positive provider session generation.
    pub generation: u64,
}

/// Supported coding-session actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum CodingSessionAction {
    /// Start or steer a turn in the selected session generation.
    #[serde(rename = "thread.turn.start")]
    ThreadTurnStart {
        /// Operator-entered turn text.
        text: String,
    },
    /// Cancel the in-flight turn in the selected session generation.
    #[serde(rename = "thread.turn.interrupt")]
    ThreadTurnInterrupt,
}

/// Durable coding-session command JSON payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionCommandPayload {
    /// Must equal [`CODING_SESSION_COMMAND_SCHEMA`].
    pub schema: String,
    /// Client-generated id used by provider adapters for idempotency.
    pub command_id: String,
    /// Provider-neutral target.
    pub target: CodingSessionTarget,
    /// Requested action.
    pub action: CodingSessionAction,
}

impl CodingSessionCommandPayload {
    /// Validate all payload fields before signing or consuming a command.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_COMMAND_SCHEMA {
            return Err("unsupported coding-session command schema".into());
        }
        validate_identifier(&self.command_id, "commandId")?;
        validate_identifier(&self.target.driver, "target.driver")?;
        validate_identifier(&self.target.instance_id, "target.instanceId")?;
        validate_identifier(&self.target.session_id, "target.sessionId")?;
        if self.target.generation == 0 || self.target.generation > MAX_SAFE_GENERATION {
            return Err("target.generation must be a positive safe integer".into());
        }
        match &self.action {
            CodingSessionAction::ThreadTurnStart { text } => {
                if text.trim().is_empty() {
                    return Err("action.text must not be empty".into());
                }
                if text.len() > MAX_TURN_TEXT_BYTES {
                    return Err(format!("action.text exceeds {MAX_TURN_TEXT_BYTES} bytes"));
                }
            }
            CodingSessionAction::ThreadTurnInterrupt => {}
        }
        Ok(())
    }
}

/// Encode a deterministic, unambiguous structured target key for the `cs-target` tag.
pub fn coding_session_target_key(target: &CodingSessionTarget) -> String {
    let fields = [
        target.driver.as_str(),
        target.instance_id.as_str(),
        target.session_id.as_str(),
        &target.generation.to_string(),
    ];
    let mut result = String::from("coding-session/v1|");
    for field in fields {
        result.push_str(&field.len().to_string());
        result.push(':');
        result.push_str(field);
    }
    result
}

fn validate_identifier(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(format!("{field} exceeds {MAX_IDENTIFIER_BYTES} bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_payload() -> CodingSessionCommandPayload {
        CodingSessionCommandPayload {
            schema: CODING_SESSION_COMMAND_SCHEMA.into(),
            command_id: "cmd-1".into(),
            target: CodingSessionTarget {
                driver: "provider-a".into(),
                instance_id: "instance-1".into(),
                session_id: "session-1".into(),
                generation: 1,
            },
            action: CodingSessionAction::ThreadTurnStart {
                text: "Ship it".into(),
            },
        }
    }

    #[test]
    fn validates_payload_and_deterministic_target_key() {
        let payload = valid_payload();
        assert!(payload.validate().is_ok());
        assert_eq!(
            coding_session_target_key(&payload.target),
            "coding-session/v1|10:provider-a10:instance-19:session-11:1"
        );
    }

    #[test]
    fn interrupt_round_trips_the_donor_wire_shape() {
        let mut payload = valid_payload();
        payload.action = CodingSessionAction::ThreadTurnInterrupt;
        assert!(payload.validate().is_ok());
        let encoded = serde_json::to_string(&payload.action).unwrap_or_default();
        assert_eq!(encoded, r#"{"type":"thread.turn.interrupt"}"#);
        let decoded: CodingSessionCommandPayload = serde_json::from_str(
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"cmd-2","target":{"driver":"provider-a","instanceId":"instance-1","sessionId":"session-1","generation":1},"action":{"type":"thread.turn.interrupt"}}"#,
        )
        .unwrap();
        assert_eq!(decoded.action, CodingSessionAction::ThreadTurnInterrupt);
        assert!(decoded.validate().is_ok());
        // Interrupt tolerates extra action fields (serde internally-tagged unit
        // variant): a newer client annotating its interrupts must not be
        // rejected by an older relay. Pin that forward-compatibility here.
        let lenient: CodingSessionCommandPayload = serde_json::from_str(
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"cmd-2","target":{"driver":"provider-a","instanceId":"instance-1","sessionId":"session-1","generation":1},"action":{"type":"thread.turn.interrupt","reason":"user"}}"#,
        )
        .unwrap();
        assert_eq!(lenient.action, CodingSessionAction::ThreadTurnInterrupt);
    }

    #[test]
    fn rejects_invalid_payloads() {
        let mut payload = valid_payload();
        payload.target.generation = 0;
        assert!(payload.validate().is_err());
        payload.target.generation = 1;
        payload.action = CodingSessionAction::ThreadTurnStart { text: "   ".into() };
        assert!(payload.validate().is_err());
    }

    #[test]
    fn rejects_control_characters_in_target_identifiers() {
        for rejected in ["provider\nSYSTEM", "instance\rnext", "session\tsteer"] {
            let mut payload = valid_payload();
            payload.target.session_id = rejected.to_owned();
            assert!(payload.validate().is_err(), "accepted {rejected:?}");
        }
    }

    #[test]
    fn utf8_byte_boundaries_match_the_interoperability_contract() {
        let mut payload = valid_payload();
        payload.action = CodingSessionAction::ThreadTurnStart {
            text: "🐝".repeat(MAX_TURN_TEXT_BYTES / 4),
        };
        assert_eq!(
            match &payload.action {
                CodingSessionAction::ThreadTurnStart { text } => text.len(),
                CodingSessionAction::ThreadTurnInterrupt => unreachable!(),
            },
            MAX_TURN_TEXT_BYTES
        );
        assert!(payload.validate().is_ok());

        payload.action = CodingSessionAction::ThreadTurnStart {
            text: format!("{}a", "🐝".repeat(MAX_TURN_TEXT_BYTES / 4)),
        };
        assert!(payload.validate().is_err());

        payload.action = CodingSessionAction::ThreadTurnStart { text: "ok".into() };
        payload.target.session_id = "é".repeat(MAX_IDENTIFIER_BYTES / 2);
        assert_eq!(payload.target.session_id.len(), MAX_IDENTIFIER_BYTES);
        assert!(payload.validate().is_ok());
        payload.target.session_id.push('a');
        assert!(payload.validate().is_err());
    }
}
