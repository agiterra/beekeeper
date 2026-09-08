//! Provider-neutral coding-session command contract.
//!
//! Events use [`crate::kind::KIND_CODING_SESSION_COMMAND`] and public JSON so
//! an installed provider adapter can consume signed operator intent. Event
//! authorship is the authority; content carries no claimed actor identity.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ci_result::{validate_identity, CiResultIdentity};

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
/// Maximum UTF-8 byte length of a CI-continuation prompt.
///
/// The same ceiling as [`MAX_TURN_TEXT_BYTES`], and for the same reason: the
/// continuation is delivered as part of a turn, so it cannot be permitted to
/// be larger than a turn.
pub const MAX_CI_CONTINUATION_BYTES: usize = MAX_TURN_TEXT_BYTES;
/// The `type` field of a CI-continuation operation pointer.
pub const CI_CONTINUATION_POINTER_TYPE: &str = "ci_result";
/// Domain separator for the derived CI-continuation `commandId`.
const CI_CONTINUATION_COMMAND_ID_DOMAIN: &str = "buzz-ci-continuation/v1";
/// Prefix of every derived CI-continuation `commandId`.
const CI_CONTINUATION_COMMAND_ID_PREFIX: &str = "cic-";

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
    /// Register a turn to be delivered when one exact CI result is recorded.
    ///
    /// This is not a turn. Nothing enters the mailbox, no budget is spent, and
    /// the provider answers it with a `continuation_registered` receipt rather
    /// than `turn_queued`. A turn is started later — under this same
    /// `commandId` and signer — only if the named run attempt records a result
    /// before `expiresAt` and the signer may still steer the target *then*.
    /// Registering is therefore a claim about a future turn, never a promise
    /// of one, and every refusal it can earn is published as a receipt.
    #[serde(rename = "thread.turn.continue_on_ci", rename_all = "camelCase")]
    ThreadTurnContinueOnCi {
        /// Exact CI run attempt whose recorded result unblocks the turn.
        ///
        /// Correlation is by the canonical digest of these eight fields, so a
        /// result for a different attempt, phase, or commit never satisfies
        /// this registration.
        identity: CiResultIdentity,
        /// Text delivered alongside the verified result when the turn starts.
        continuation: String,
        /// Unix seconds after which the registration is refused rather than
        /// delivered.
        ///
        /// Required and positive: a registration with no horizon would wait
        /// forever on a run that may never finish, and zero is not a shorthand
        /// for "never expires".
        expires_at: u64,
    },
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
            CodingSessionAction::ThreadTurnContinueOnCi {
                identity,
                continuation,
                expires_at,
            } => {
                validate_identity(identity)?;
                // A registration with no words is a wake with nothing to say:
                // the result alone is already on the wire for anyone watching.
                if continuation.trim().is_empty() {
                    return Err("action.continuation must not be empty".into());
                }
                if continuation.len() > MAX_CI_CONTINUATION_BYTES {
                    return Err(format!(
                        "action.continuation exceeds {MAX_CI_CONTINUATION_BYTES} bytes"
                    ));
                }
                if *expires_at == 0 {
                    return Err("action.expiresAt must be a positive Unix timestamp".into());
                }
            }
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

/// The exact operation-fence text for one CI correlation digest.
///
/// This compact pointer — not the materialized prompt the provider eventually
/// delivers — is what the operation ledger fences on, so two registrations of
/// the same result under different command ids converge on a single turn
/// instead of spending the agent's context twice on one fact. Callers must
/// build it here: the byte shape is the contract, and re-serializing an
/// equivalent map is not guaranteed to reproduce it.
pub fn ci_continuation_pointer(correlation_id: &str) -> String {
    format!(r#"{{"operationId":"{correlation_id}","type":"{CI_CONTINUATION_POINTER_TYPE}"}}"#)
}

/// Derive the deterministic `commandId` of a CI-continuation registration.
///
/// Every input that changes what would be delivered is in the digest, so an
/// exact retry names the same registration — the provider answers it
/// idempotently rather than minting a second pending turn — while a changed
/// continuation, expiry, target, or channel is a different command the
/// provider can refuse as a conflict instead of silently overwriting.
///
/// The pre-image is the domain separator, channel id, correlation digest,
/// [`coding_session_target_key`], `expires_at`, and the SHA-256 of the
/// continuation, each separated by one NUL byte.
pub fn ci_continuation_command_id(
    channel_id: &str,
    correlation_id: &str,
    target_key: &str,
    expires_at: u64,
    continuation: &str,
) -> String {
    let continuation_digest = hex::encode(Sha256::digest(continuation.as_bytes()));
    let expires_at = expires_at.to_string();
    let fields = [
        CI_CONTINUATION_COMMAND_ID_DOMAIN,
        channel_id,
        correlation_id,
        target_key,
        expires_at.as_str(),
        continuation_digest.as_str(),
    ];
    let mut hasher = Sha256::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            hasher.update([0_u8]);
        }
        hasher.update(field.as_bytes());
    }
    format!(
        "{CI_CONTINUATION_COMMAND_ID_PREFIX}{}",
        hex::encode(hasher.finalize())
    )
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
                CodingSessionAction::ThreadTurnInterrupt
                | CodingSessionAction::ThreadTurnContinueOnCi { .. } => unreachable!(),
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
    const CI_OWNER: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const CI_WORKFLOW: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";

    fn ci_identity() -> CiResultIdentity {
        CiResultIdentity {
            project: format!("30621:{CI_OWNER}:beekeeper"),
            repository: format!("30617:{CI_OWNER}:beekeeper"),
            commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            check: "main-validation".into(),
            run: "136".into(),
            attempt: 1,
            workflow: CI_WORKFLOW.into(),
            phase: crate::ci_result::CiPhase::Build,
        }
    }

    fn ci_continuation_payload() -> CodingSessionCommandPayload {
        let mut payload = valid_payload();
        payload.command_id = "cic-".to_owned() + &"a".repeat(64);
        payload.action = CodingSessionAction::ThreadTurnContinueOnCi {
            identity: ci_identity(),
            continuation: "Report the failing test".into(),
            expires_at: 1_788_800_000,
        };
        payload
    }

    #[test]
    fn ci_continuation_round_trips_the_closed_wire_shape() {
        let payload = ci_continuation_payload();
        assert!(payload.validate().is_ok());
        let encoded = serde_json::to_string(&payload.action).unwrap_or_default();
        assert_eq!(
            encoded,
            format!(
                concat!(
                    r#"{{"type":"thread.turn.continue_on_ci","identity":{{"project":"30621:{owner}:beekeeper","#,
                    r#""repository":"30617:{owner}:beekeeper","#,
                    r#""commit":"abcdef0123456789abcdef0123456789abcdef01","check":"main-validation","#,
                    r#""run":"136","attempt":1,"workflow":"{workflow}","phase":"build"}},"#,
                    r#""continuation":"Report the failing test","expiresAt":1788800000}}"#
                ),
                owner = CI_OWNER,
                workflow = CI_WORKFLOW,
            )
        );
        let decoded: CodingSessionCommandPayload =
            serde_json::from_str(&serde_json::to_string(&payload).unwrap_or_default())
                .expect("round trip");
        assert_eq!(decoded.action, payload.action);
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn ci_continuation_rejects_missing_and_unknown_action_keys() {
        // Unlike the unit-variant interrupt, this action is a struct variant
        // under `deny_unknown_fields`: a key this build does not know is a
        // hard rejection, because a consumer that ignored it would deliver a
        // turn under terms it never read.
        let encoded_action =
            serde_json::to_string(&ci_continuation_payload().action).expect("action serializes");
        let envelope = |action: &str| {
            format!(
                concat!(
                    r#"{{"schema":"buzz-coding-session-command/v1","commandId":"cic-1","#,
                    r#""target":{{"driver":"provider-a","instanceId":"instance-1","#,
                    r#""sessionId":"session-1","generation":1}},"action":{}}}"#
                ),
                action
            )
        };
        assert!(
            serde_json::from_str::<CodingSessionCommandPayload>(&envelope(&encoded_action)).is_ok()
        );

        let missing_expires_at = encoded_action.replace(r#","expiresAt":1788800000"#, "");
        assert!(
            serde_json::from_str::<CodingSessionCommandPayload>(&envelope(&missing_expires_at))
                .is_err(),
            "accepted an action with no expiresAt"
        );

        let snake_case = encoded_action.replace(r#""expiresAt""#, r#""expires_at""#);
        assert!(
            serde_json::from_str::<CodingSessionCommandPayload>(&envelope(&snake_case)).is_err(),
            "accepted a snake_case expiresAt"
        );

        let unknown_key = encoded_action.replace(
            r#","expiresAt":1788800000"#,
            r#","expiresAt":1788800000,"deliver":"steer""#,
        );
        assert!(
            serde_json::from_str::<CodingSessionCommandPayload>(&envelope(&unknown_key)).is_err(),
            "accepted an unknown action key"
        );

        let unknown_identity_key =
            encoded_action.replace(r#""phase":"build""#, r#""phase":"build","branch":"main""#);
        assert!(
            serde_json::from_str::<CodingSessionCommandPayload>(&envelope(&unknown_identity_key))
                .is_err(),
            "accepted an unknown identity key"
        );
    }

    #[test]
    fn ci_continuation_validates_identity_and_bounds() {
        let mut payload = ci_continuation_payload();
        if let CodingSessionAction::ThreadTurnContinueOnCi { continuation, .. } =
            &mut payload.action
        {
            *continuation = "   ".into();
        }
        assert_eq!(
            payload.validate(),
            Err("action.continuation must not be empty".into())
        );

        let mut payload = ci_continuation_payload();
        if let CodingSessionAction::ThreadTurnContinueOnCi { continuation, .. } =
            &mut payload.action
        {
            *continuation = "c".repeat(MAX_CI_CONTINUATION_BYTES + 1);
        }
        assert!(payload
            .validate()
            .is_err_and(|error| error.contains("action.continuation exceeds")));

        // The largest continuation the contract admits is still admitted.
        let mut payload = ci_continuation_payload();
        if let CodingSessionAction::ThreadTurnContinueOnCi { continuation, .. } =
            &mut payload.action
        {
            *continuation = "c".repeat(MAX_CI_CONTINUATION_BYTES);
        }
        assert!(payload.validate().is_ok());

        let mut payload = ci_continuation_payload();
        if let CodingSessionAction::ThreadTurnContinueOnCi { expires_at, .. } = &mut payload.action
        {
            *expires_at = 0;
        }
        assert_eq!(
            payload.validate(),
            Err("action.expiresAt must be a positive Unix timestamp".into())
        );

        // Identity is validated by the same rules the recorded result uses, so
        // a registration can never name a run a result could not be filed
        // under.
        let mut payload = ci_continuation_payload();
        if let CodingSessionAction::ThreadTurnContinueOnCi { identity, .. } = &mut payload.action {
            identity.commit = "A".repeat(40);
        }
        assert!(payload
            .validate()
            .is_err_and(|error| error.contains("commit must be lowercase 40-hex")));
    }

    #[test]
    fn ci_continuation_pointer_and_command_id_are_deterministic() {
        let identity = ci_identity();
        let correlation = crate::ci_result::correlation_id(&identity).expect("correlation");
        let pointer = ci_continuation_pointer(&correlation);
        assert_eq!(
            pointer,
            format!(r#"{{"operationId":"{correlation}","type":"ci_result"}}"#)
        );
        assert_eq!(pointer, ci_continuation_pointer(&correlation));
        assert_eq!(CI_CONTINUATION_POINTER_TYPE, "ci_result");

        let channel = "0eb1cd0f-7f4a-4a7f-9a0e-9f38a0d5b6b1";
        let target_key = coding_session_target_key(&ci_continuation_payload().target);
        let command_id =
            ci_continuation_command_id(channel, &correlation, &target_key, 1_788_800_000, "go");
        assert_eq!(
            command_id,
            ci_continuation_command_id(channel, &correlation, &target_key, 1_788_800_000, "go"),
            "the same registration must derive the same id"
        );
        assert!(command_id.starts_with("cic-"));
        assert_eq!(command_id.len(), 4 + 64);
        assert!(command_id[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert!(validate_identifier(&command_id, "commandId").is_ok());

        // Every input that changes what would be delivered changes the id, so
        // a different intent can never be mistaken for an exact retry.
        let variants = [
            ci_continuation_command_id(
                "0eb1cd0f-7f4a-4a7f-9a0e-9f38a0d5b6b2",
                &correlation,
                &target_key,
                1_788_800_000,
                "go",
            ),
            ci_continuation_command_id(channel, &"f".repeat(64), &target_key, 1_788_800_000, "go"),
            ci_continuation_command_id(channel, &correlation, "other", 1_788_800_000, "go"),
            ci_continuation_command_id(channel, &correlation, &target_key, 1_788_800_001, "go"),
            ci_continuation_command_id(channel, &correlation, &target_key, 1_788_800_000, "go2"),
        ];
        for variant in &variants {
            assert_ne!(&command_id, variant);
        }
        // NUL separation, not concatenation: moving a byte across a field
        // boundary must not collide.
        assert_ne!(
            ci_continuation_command_id("a", "bc", &target_key, 1, "x"),
            ci_continuation_command_id("ab", "c", &target_key, 1, "x"),
        );
    }
}
