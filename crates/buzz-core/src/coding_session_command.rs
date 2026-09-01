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
/// Maximum number of image attachments a single turn may carry.
pub const MAX_TURN_ATTACHMENTS: usize = 4;
/// Maximum declared byte size of a single turn attachment.
pub const MAX_TURN_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
/// The image MIME types a turn attachment may declare.
///
/// Deliberately the same set `buzz-cli` will upload (`ALLOWED_MIMES`), minus
/// video: a still image is what an ACP `image` content block can carry.
pub const ALLOWED_ATTACHMENT_MIMES: [&str; 4] =
    ["image/jpeg", "image/png", "image/gif", "image/webp"];
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

/// How the sender asked the provider to deliver a turn.
///
/// The three classes are the wire form of the delivery contract: the sender
/// chooses, the provider executes, and a provider that cannot honour the
/// requested class says so in a receipt rather than silently doing something
/// else.
///
/// - `boundary` — hold the turn and start it when the current one settles.
///   This is the default, and the only behaviour that existed before the field
///   did, so an absent `deliver` key means exactly this.
/// - `steer` — inject into the running turn where the execution's runtime
///   advertised native steering. Where it did not, the provider publishes a
///   `turn_degraded` receipt and delivers at the next boundary instead. It
///   never cancels a running turn to merge the two prompts.
/// - `interrupt` — cancel the running turn first, then deliver at the boundary
///   that cancel creates. Reserved to the session founder.
///
/// The wire form is exactly these three lowercase strings. Anything else fails
/// to decode — [`CodingSessionCommandPayload`] carries `deny_unknown_fields`
/// and this enum has no catch-all variant — so a provider can never guess at a
/// class it does not implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodingSessionDelivery {
    /// Hold until the current turn settles. The default.
    #[default]
    Boundary,
    /// Inject mid-turn where the runtime supports it; otherwise downgrade to
    /// [`CodingSessionDelivery::Boundary`] and say so.
    Steer,
    /// Cancel the running turn, then deliver. Founder-only.
    Interrupt,
}

impl CodingSessionDelivery {
    /// The exact wire string this class serializes as.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boundary => "boundary",
            Self::Steer => "steer",
            Self::Interrupt => "interrupt",
        }
    }

    /// Decode one wire string, or `None` when it names no known class.
    ///
    /// The strict decoder rejects an unknown class outright; this exists for
    /// callers that hold the raw string (a relay envelope check, a CLI flag)
    /// and want the same answer without a serde round trip.
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "boundary" => Some(Self::Boundary),
            "steer" => Some(Self::Steer),
            "interrupt" => Some(Self::Interrupt),
            _ => None,
        }
    }
}

/// A Blossom-hosted image attached to a turn.
///
/// Deliberately carries **no URL**. The blob is addressed by hash and the
/// consuming provider derives `{relay}/media/{sha256}.{ext}` from the relay it
/// is already connected to, so an operator-supplied string can never steer a
/// provider's fetch at an arbitrary host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnAttachment {
    /// Lowercase hex sha256 of the blob — its Blossom identity.
    pub sha256: String,
    /// Declared MIME type; must be one of [`ALLOWED_ATTACHMENT_MIMES`].
    pub mime: String,
    /// Declared byte size of the blob.
    pub size: u64,
    /// Pixel dimensions as `WxH`, when the uploader reported them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim: Option<String>,
    /// Original filename, for display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

impl TurnAttachment {
    /// The file extension implied by [`Self::mime`].
    pub fn extension(&self) -> &'static str {
        match self.mime.as_str() {
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "png",
        }
    }

    /// Validate one attachment's fields.
    fn validate(&self, index: usize) -> Result<(), String> {
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(format!(
                "action.attachments[{index}].sha256 must be 64 lowercase hex characters"
            ));
        }
        if !ALLOWED_ATTACHMENT_MIMES.contains(&self.mime.as_str()) {
            return Err(format!(
                "action.attachments[{index}].mime must be one of {}",
                ALLOWED_ATTACHMENT_MIMES.join(", ")
            ));
        }
        if self.size == 0 || self.size > MAX_TURN_ATTACHMENT_BYTES {
            return Err(format!(
                "action.attachments[{index}].size must be between 1 and {MAX_TURN_ATTACHMENT_BYTES} bytes"
            ));
        }
        Ok(())
    }
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
        /// Images the operator attached to this turn.
        ///
        /// Omitted from the wire when empty, so a turn without attachments
        /// serializes exactly as it did before this field existed — the same
        /// forward-compatibility contract `deliver` keeps.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<TurnAttachment>,
        /// Requested delivery class. Absent on the wire means
        /// [`CodingSessionDelivery::Boundary`], which is what every command
        /// published before this field existed meant.
        #[serde(default)]
        deliver: CodingSessionDelivery,
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
            // `deliver` needs no check here: it is a closed enum, so a class
            // this build does not know never survives decoding to reach
            // validation.
            CodingSessionAction::ThreadTurnStart {
                text,
                attachments,
                deliver: _,
            } => {
                // An attachment adds to a turn; it never stands in for one. A
                // bare image with no instruction gives the agent nothing to do.
                if text.trim().is_empty() {
                    return Err("action.text must not be empty".into());
                }
                if text.len() > MAX_TURN_TEXT_BYTES {
                    return Err(format!("action.text exceeds {MAX_TURN_TEXT_BYTES} bytes"));
                }
                if attachments.len() > MAX_TURN_ATTACHMENTS {
                    return Err(format!(
                        "action.attachments exceeds {MAX_TURN_ATTACHMENTS} entries"
                    ));
                }
                for (index, attachment) in attachments.iter().enumerate() {
                    attachment.validate(index)?;
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
                attachments: Vec::new(),
                deliver: CodingSessionDelivery::Boundary,
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
        payload.action = CodingSessionAction::ThreadTurnStart {
            text: "   ".into(),
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        };
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
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        };
        assert_eq!(
            match &payload.action {
                CodingSessionAction::ThreadTurnStart { text, .. } => text.len(),
                CodingSessionAction::ThreadTurnInterrupt => unreachable!(),
            },
            MAX_TURN_TEXT_BYTES
        );
        assert!(payload.validate().is_ok());

        payload.action = CodingSessionAction::ThreadTurnStart {
            text: format!("{}a", "🐝".repeat(MAX_TURN_TEXT_BYTES / 4)),
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        };
        assert!(payload.validate().is_err());

        payload.action = CodingSessionAction::ThreadTurnStart {
            text: "ok".into(),
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        };
        payload.target.session_id = "é".repeat(MAX_IDENTIFIER_BYTES / 2);
        assert_eq!(payload.target.session_id.len(), MAX_IDENTIFIER_BYTES);
        assert!(payload.validate().is_ok());
        payload.target.session_id.push('a');
        assert!(payload.validate().is_err());
    }

    /// The delivery class is optional on the wire and defaults to the one
    /// behaviour that existed before it did. A command published by an older
    /// client — no `deliver` key at all — must keep meaning "hold until the
    /// current turn settles", or every pre-existing queued turn silently
    /// changes class the day this field ships.
    #[test]
    fn an_absent_deliver_class_means_boundary() {
        let decoded: CodingSessionCommandPayload = serde_json::from_str(
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"cmd-3","target":{"driver":"provider-a","instanceId":"instance-1","sessionId":"session-1","generation":1},"action":{"type":"thread.turn.start","text":"go"}}"#,
        )
        .expect("decode a command with no deliver key");
        assert_eq!(
            decoded.action,
            CodingSessionAction::ThreadTurnStart {
                text: "go".into(),
                attachments: Vec::new(),
                deliver: CodingSessionDelivery::Boundary,
            }
        );
        assert!(decoded.validate().is_ok());
    }

    fn attachment(sha: &str) -> TurnAttachment {
        TurnAttachment {
            sha256: sha.into(),
            mime: "image/png".into(),
            size: 1024,
            dim: Some("800x600".into()),
            filename: Some("shot.png".into()),
        }
    }

    fn start_payload(action: CodingSessionAction) -> CodingSessionCommandPayload {
        CodingSessionCommandPayload {
            schema: CODING_SESSION_COMMAND_SCHEMA.into(),
            command_id: "cmd-att".into(),
            target: CodingSessionTarget {
                driver: "provider-a".into(),
                instance_id: "instance-1".into(),
                session_id: "session-1".into(),
                generation: 1,
            },
            action,
        }
    }

    const SHA_A: &str = "aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd";

    /// A turn with no attachments must serialize to *exactly* the bytes it did
    /// before the field existed. This is the whole forward-compatibility
    /// contract: every existing client, relay and provider keeps working, and
    /// only a turn that actually carries an image takes the new shape.
    #[test]
    fn an_empty_attachment_list_is_absent_from_the_wire() {
        let payload = start_payload(CodingSessionAction::ThreadTurnStart {
            text: "go".into(),
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        });
        let wire = serde_json::to_string(&payload).expect("serialize");
        assert!(
            !wire.contains("attachments"),
            "an empty list must not reach the wire: {wire}"
        );
    }

    /// A payload written before this field existed still decodes, as no
    /// attachments — the same absent-means-default rule `deliver` follows.
    #[test]
    fn an_absent_attachment_list_decodes_as_empty() {
        let decoded: CodingSessionCommandPayload = serde_json::from_str(
            r#"{"schema":"buzz-coding-session-command/v1","commandId":"cmd-3","target":{"driver":"provider-a","instanceId":"instance-1","sessionId":"session-1","generation":1},"action":{"type":"thread.turn.start","text":"go"}}"#,
        )
        .expect("decode a command with no attachments key");
        assert!(matches!(
            decoded.action,
            CodingSessionAction::ThreadTurnStart { ref attachments, .. } if attachments.is_empty()
        ));
    }

    /// Attachments round-trip, and the optional display fields stay optional.
    #[test]
    fn attachments_round_trip() {
        let payload = start_payload(CodingSessionAction::ThreadTurnStart {
            text: "why is this wrong?".into(),
            attachments: vec![attachment(SHA_A)],
            deliver: CodingSessionDelivery::Boundary,
        });
        payload.validate().expect("valid");
        let wire = serde_json::to_string(&payload).expect("serialize");
        let back: CodingSessionCommandPayload = serde_json::from_str(&wire).expect("decode");
        assert_eq!(back, payload);

        let bare = TurnAttachment {
            dim: None,
            filename: None,
            ..attachment(SHA_A)
        };
        let wire = serde_json::to_string(&bare).expect("serialize");
        assert!(
            !wire.contains("dim") && !wire.contains("filename"),
            "{wire}"
        );
    }

    /// Every field an attachment declares is checked before signing. A
    /// provider acts on these values — it derives a fetch URL from the hash —
    /// so a malformed one must never reach it.
    #[test]
    fn malformed_attachments_are_refused() {
        let cases: [(TurnAttachment, &str); 5] = [
            (
                TurnAttachment {
                    sha256: "abc".into(),
                    ..attachment(SHA_A)
                },
                "short hash",
            ),
            (
                TurnAttachment {
                    sha256: SHA_A.to_uppercase(),
                    ..attachment(SHA_A)
                },
                "uppercase hash",
            ),
            (
                TurnAttachment {
                    mime: "application/pdf".into(),
                    ..attachment(SHA_A)
                },
                "non-image mime",
            ),
            (
                TurnAttachment {
                    size: 0,
                    ..attachment(SHA_A)
                },
                "zero size",
            ),
            (
                TurnAttachment {
                    size: MAX_TURN_ATTACHMENT_BYTES + 1,
                    ..attachment(SHA_A)
                },
                "oversize",
            ),
        ];
        for (bad, label) in cases {
            let payload = start_payload(CodingSessionAction::ThreadTurnStart {
                text: "go".into(),
                attachments: vec![bad],
                deliver: CodingSessionDelivery::Boundary,
            });
            assert!(payload.validate().is_err(), "accepted {label}");
        }
    }

    /// The count is capped, and an image never substitutes for an instruction.
    #[test]
    fn attachment_count_is_capped_and_text_is_still_required() {
        let too_many = vec![attachment(SHA_A); MAX_TURN_ATTACHMENTS + 1];
        let payload = start_payload(CodingSessionAction::ThreadTurnStart {
            text: "go".into(),
            attachments: too_many,
            deliver: CodingSessionDelivery::Boundary,
        });
        assert!(payload.validate().is_err(), "accepted too many attachments");

        let payload = start_payload(CodingSessionAction::ThreadTurnStart {
            text: "   ".into(),
            attachments: vec![attachment(SHA_A)],
            deliver: CodingSessionDelivery::Boundary,
        });
        assert!(
            payload.validate().is_err(),
            "an attachment must not stand in for turn text"
        );
    }

    /// All three classes round-trip as their exact lowercase wire strings, and
    /// nothing else decodes: a provider must never have to guess what an
    /// unrecognized class was supposed to do.
    #[test]
    fn deliver_classes_round_trip_and_reject_anything_else() {
        for (wire, class) in [
            ("boundary", CodingSessionDelivery::Boundary),
            ("steer", CodingSessionDelivery::Steer),
            ("interrupt", CodingSessionDelivery::Interrupt),
        ] {
            let mut payload = valid_payload();
            payload.action = CodingSessionAction::ThreadTurnStart {
                text: "go".into(),
                attachments: Vec::new(),
                deliver: class,
            };
            assert!(payload.validate().is_ok());
            let encoded = serde_json::to_string(&payload.action).unwrap_or_default();
            assert_eq!(
                encoded,
                format!(r#"{{"type":"thread.turn.start","text":"go","deliver":"{wire}"}}"#)
            );
            let decoded: CodingSessionCommandPayload =
                serde_json::from_str(&serde_json::to_string(&payload).unwrap_or_default())
                    .expect("round trip");
            assert_eq!(decoded.action, payload.action);
            assert_eq!(class.as_str(), wire);
            assert_eq!(CodingSessionDelivery::from_wire(wire), Some(class));
        }

        for rejected in ["Boundary", "cancel", "", "steer "] {
            let content = format!(
                r#"{{"schema":"buzz-coding-session-command/v1","commandId":"cmd-4","target":{{"driver":"provider-a","instanceId":"instance-1","sessionId":"session-1","generation":1}},"action":{{"type":"thread.turn.start","text":"go","deliver":"{rejected}"}}}}"#
            );
            assert!(
                serde_json::from_str::<CodingSessionCommandPayload>(&content).is_err(),
                "accepted deliver={rejected:?}"
            );
            assert_eq!(CodingSessionDelivery::from_wire(rejected), None);
        }
    }
}
