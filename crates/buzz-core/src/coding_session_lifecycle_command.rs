//! Provider-neutral coding-session lifecycle command contract.
//!
//! Events use [`crate::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND`] and public
//! JSON so an installed provider adapter can create, resume, or stop a session.
//! Event authorship
//! is the authority; content carries no claimed actor identity or host-specific
//! execution state — notably, never a filesystem path.
//!
//! # Fork amendment: `projectRef` is optional
//!
//! The donor contract required every session to name a Buzz project. Here a
//! session may stand alone: `projectRef` is `Option<String>` and, when present,
//! must be a NIP-MP project coordinate (`30621:<owner>:<d>`). The field is still
//! *structurally required* in signed JSON — `null` must be written explicitly —
//! so a truncated or partially-serialized payload can never be mistaken for a
//! deliberate standalone session. See `docs/nips/NIP-CSL.md`.
//!
//! # Fork amendment: `sessionRef` umbrella reference
//!
//! `session.create` also carries a nullable `sessionRef` — a client-minted
//! lowercase UUID grouping several provider executions into one user-facing
//! umbrella session. Unlike `projectRef`, this field was added *after* the v1
//! schema shipped, so signed events without the key exist and must stay valid
//! forever. The decoder therefore accepts exactly one of three forms: the
//! historical 8-key action, the 9-key action including `sessionRef`, or the
//! 10-key action including both `sessionRef` and `genesisRef`. `genesisRef`
//! can never appear without `sessionRef`; nothing between or beyond those
//! forms is accepted. See `docs/nips/NIP-CSL.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coding_session_command::{
    CodingSessionTarget, MAX_IDENTIFIER_BYTES, MAX_SAFE_GENERATION,
};
use crate::kind::KIND_PROJECT;

/// The currently supported coding-session lifecycle command envelope schema.
pub const CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA: &str =
    "buzz-coding-session-lifecycle-command/v1";
/// The version tag placed on each coding-session lifecycle command event.
pub const CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION: &str = "csl1-1";
/// Maximum UTF-8 byte length for a lifecycle command identifier.
pub const MAX_LIFECYCLE_COMMAND_ID_BYTES: usize = 256;
/// Maximum UTF-8 byte length for project, repository, provider, model, or title references.
pub const MAX_LIFECYCLE_REFERENCE_BYTES: usize = 2 * 1024;
/// Maximum UTF-8 byte length for an initial turn.
pub const MAX_LIFECYCLE_INITIAL_TURN_BYTES: usize = 12 * 1024;
/// Maximum UTF-8 byte length for the complete signed event content.
pub const MAX_LIFECYCLE_CONTENT_BYTES: usize = 16 * 1024;

/// The kind segment every `projectRef` coordinate must carry.
///
/// Sessions bind to NIP-MP projects (kind 30621) and nothing else. The donor's
/// era of `30178:` team-catalog coordinates is not accepted here.
pub const PROJECT_REF_KIND_SEGMENT: &str = "30621";
const _: () = assert!(KIND_PROJECT == 30621);

/// Supported coding-session lifecycle actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CodingSessionLifecycleAction {
    /// Create a new provider session, optionally bound to a Buzz project.
    #[serde(rename = "session.create")]
    SessionCreate {
        /// Optional Buzz project reference (`30621:<owner>:<d>`).
        ///
        /// `None` creates a standalone session, owned by the channel it is
        /// published into rather than by a project.
        project_ref: Option<String>,
        /// Optional repository reference within the project.
        repo_ref: Option<String>,
        /// Optional umbrella session reference (canonical lowercase UUID).
        ///
        /// A later create carrying the same `sessionRef` joins the same
        /// umbrella as a new execution. `None` claims no umbrella — the
        /// pre-amendment semantics, an implicit umbrella of one. Decodes to
        /// `None` both from an explicit `null` and from the historical 8-key
        /// form that predates the field.
        session_ref: Option<String>,
        /// Optional event id of the immutable genesis founding this umbrella.
        ///
        /// Emitted only for the 10-key authority-aware form. A present
        /// reference requires a present `sessionRef`; consumers resolve it by
        /// event id rather than querying by the umbrella label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        genesis_ref: Option<String>,
        /// Required capability-advertised provider instance reference.
        provider_instance_ref: String,
        /// Required signing pubkey of the selected provider catalog authority.
        provider_authority_pubkey: String,
        /// Optional provider-neutral model identifier.
        model: Option<String>,
        /// Optional operator-facing session title.
        title: Option<String>,
        /// Optional first turn to deliver after session creation.
        initial_turn: Option<String>,
    },
    /// Reattach a disconnected, non-stopped execution as a new generation.
    #[serde(rename = "session.resume")]
    SessionResume {
        /// Exact previous generation being resumed.
        session: CodingSessionTarget,
        /// Signing pubkey of the provider authority that owns the target.
        provider_authority_pubkey: String,
    },
    /// Durably stop an execution so a provider restart cannot revive it.
    #[serde(rename = "session.stop")]
    SessionStop {
        /// Exact current generation being stopped.
        session: CodingSessionTarget,
        /// Signing pubkey of the provider authority that owns the target.
        provider_authority_pubkey: String,
    },
}

/// Durable coding-session lifecycle command JSON payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionLifecycleCommandPayload {
    /// Must equal [`CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA`].
    pub schema: String,
    /// Client-generated id used by provider adapters for idempotency.
    pub command_id: String,
    /// Requested lifecycle action.
    pub action: CodingSessionLifecycleAction,
}

impl CodingSessionLifecycleCommandPayload {
    /// Validate all payload fields before signing a lifecycle command.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA {
            return Err("unsupported coding-session lifecycle command schema".into());
        }
        validate_required(
            &self.command_id,
            "commandId",
            MAX_LIFECYCLE_COMMAND_ID_BYTES,
        )?;
        match &self.action {
            CodingSessionLifecycleAction::SessionCreate {
                project_ref,
                repo_ref,
                session_ref,
                genesis_ref,
                provider_instance_ref,
                provider_authority_pubkey,
                model,
                title,
                initial_turn,
            } => {
                if let Some(project_ref) = project_ref {
                    validate_required(
                        project_ref,
                        "action.projectRef",
                        MAX_LIFECYCLE_REFERENCE_BYTES,
                    )?;
                    validate_project_ref(project_ref)?;
                }
                if let Some(session_ref) = session_ref {
                    validate_session_ref(session_ref)?;
                }
                if let Some(genesis_ref) = genesis_ref {
                    if session_ref.is_none() {
                        return Err(
                            "action.genesisRef requires a non-null action.sessionRef".into()
                        );
                    }
                    validate_event_id_hex("action.genesisRef", genesis_ref)?;
                }
                validate_optional(repo_ref, "action.repoRef", MAX_LIFECYCLE_REFERENCE_BYTES)?;
                validate_required(
                    provider_instance_ref,
                    "action.providerInstanceRef",
                    MAX_LIFECYCLE_REFERENCE_BYTES,
                )?;
                validate_provider_authority_pubkey(provider_authority_pubkey)?;
                validate_optional(model, "action.model", MAX_LIFECYCLE_REFERENCE_BYTES)?;
                validate_optional(title, "action.title", MAX_LIFECYCLE_REFERENCE_BYTES)?;
                validate_optional(
                    initial_turn,
                    "action.initialTurn",
                    MAX_LIFECYCLE_INITIAL_TURN_BYTES,
                )?;
            }
            CodingSessionLifecycleAction::SessionResume {
                session,
                provider_authority_pubkey,
            }
            | CodingSessionLifecycleAction::SessionStop {
                session,
                provider_authority_pubkey,
            } => {
                validate_target(session)?;
                validate_provider_authority_pubkey(provider_authority_pubkey)?;
            }
        }
        Ok(())
    }
}

/// Strictly decode and validate signed lifecycle-command content.
///
/// Nullable action fields must be present explicitly, even when their value is
/// `null`. Unknown, duplicate, or missing fields are rejected. The one
/// exceptions are the additive authority fields: create carries exactly the
/// historical 8-key set, the 9-key set including `sessionRef`, or the 10-key
/// set including both `sessionRef` and `genesisRef`.
pub fn decode_coding_session_lifecycle_command(
    content: &str,
) -> Result<CodingSessionLifecycleCommandPayload, String> {
    if content.len() > MAX_LIFECYCLE_CONTENT_BYTES {
        return Err(format!(
            "coding-session lifecycle command content exceeds {MAX_LIFECYCLE_CONTENT_BYTES} bytes"
        ));
    }

    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session lifecycle command payload".to_string())?;
    require_exact_fields(&value, &["schema", "commandId", "action"], "payload")?;
    let action = value
        .get("action")
        .ok_or_else(|| "coding-session lifecycle command payload missing action".to_string())?;
    match action.get("type").and_then(Value::as_str) {
        Some("session.create") => {
            require_exact_field_forms(
                action,
                &[
                    &[
                        "type",
                        "projectRef",
                        "repoRef",
                        "providerInstanceRef",
                        "providerAuthorityPubkey",
                        "model",
                        "title",
                        "initialTurn",
                    ],
                    &[
                        "type",
                        "projectRef",
                        "repoRef",
                        "sessionRef",
                        "providerInstanceRef",
                        "providerAuthorityPubkey",
                        "model",
                        "title",
                        "initialTurn",
                    ],
                    &[
                        "type",
                        "projectRef",
                        "repoRef",
                        "sessionRef",
                        "genesisRef",
                        "providerInstanceRef",
                        "providerAuthorityPubkey",
                        "model",
                        "title",
                        "initialTurn",
                    ],
                ],
                "action",
            )?;
            if action.get("genesisRef").is_some()
                && (action.get("sessionRef").and_then(Value::as_str).is_none()
                    || action.get("genesisRef").and_then(Value::as_str).is_none())
            {
                return Err(
                    "coding-session lifecycle command action.genesisRef requires non-null string sessionRef and genesisRef"
                        .into(),
                );
            }
        }
        Some("session.resume" | "session.stop") => require_exact_fields(
            action,
            &["type", "session", "providerAuthorityPubkey"],
            "action",
        )?,
        _ => return Err("coding-session lifecycle command action type is unsupported".into()),
    }

    // Decode a second time into the strict serde type. This preserves serde's
    // duplicate-field detection, which a Value alone cannot represent.
    let payload: CodingSessionLifecycleCommandPayload = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session lifecycle command payload".to_string())?;
    payload.validate()?;
    Ok(payload)
}

/// Check that `project_ref` is a canonical NIP-MP project coordinate.
///
/// Splits on the first two colons only, matching how NIP-09 deletion handling
/// and NIP-MP member parsing read coordinates, so a project whose `d` tag
/// contains a colon stays addressable. Owner hex must be lowercase: `#a` filter
/// matching is byte-exact, so an uppercase-owner coordinate would be invisible
/// to the queries readers actually issue.
pub fn validate_project_ref(project_ref: &str) -> Result<(), String> {
    let malformed = || {
        format!(
            "action.projectRef must be \
             `{PROJECT_REF_KIND_SEGMENT}:<lowercase-64-hex-owner>:<project-d>` \
             (got {project_ref:?})"
        )
    };
    let mut segments = project_ref.splitn(3, ':');
    let (Some(kind), Some(owner), Some(project_d)) =
        (segments.next(), segments.next(), segments.next())
    else {
        return Err(malformed());
    };
    if kind != PROJECT_REF_KIND_SEGMENT {
        return Err(malformed());
    }
    if owner.len() != 64
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(malformed());
    }
    if project_d.is_empty() {
        return Err(malformed());
    }
    Ok(())
}

/// Check that `session_ref` is a canonical lowercase hyphenated UUID.
///
/// The umbrella reference never rides in a tag, so nothing downstream
/// normalizes it — two clients only agree on membership if the bytes are
/// byte-exact. Canonical form is therefore the contract: 36 characters,
/// `8-4-4-4-12`, lowercase hex. Uppercase, braces, URNs, and truncations are
/// all rejected rather than coerced.
pub fn validate_session_ref(session_ref: &str) -> Result<(), String> {
    let malformed = || {
        format!(
            "action.sessionRef must be a canonical lowercase hyphenated UUID \
             (got {session_ref:?})"
        )
    };
    let bytes = session_ref.as_bytes();
    if bytes.len() != 36 {
        return Err(malformed());
    }
    for (index, byte) in bytes.iter().enumerate() {
        let valid = match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
        };
        if !valid {
            return Err(malformed());
        }
    }
    Ok(())
}

/// Check that a Nostr event reference is canonical lowercase 64-hex.
pub fn validate_event_id_hex(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be a lowercase 64-hex event id"));
    }
    Ok(())
}

fn require_exact_fields(value: &Value, expected: &[&str], field: &str) -> Result<(), String> {
    require_exact_fields_with_optional(value, expected, &[], field)
}

/// Require every `expected` key and allow — without requiring — the `optional`
/// ones. Any key outside both sets is still a hard rejection, so "optional"
/// here means exactly "a later schema revision's additive key", never "extra
/// data tolerated".
fn require_exact_fields_with_optional(
    value: &Value,
    expected: &[&str],
    optional: &[&str],
    field: &str,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("coding-session lifecycle command {field} must be an object"))?;
    let complete = expected.iter().all(|key| object.contains_key(*key));
    let recognized = object
        .keys()
        .all(|key| expected.contains(&key.as_str()) || optional.contains(&key.as_str()));
    if !complete || !recognized {
        return Err(format!(
            "coding-session lifecycle command {field} has missing or unsupported fields"
        ));
    }
    Ok(())
}

fn require_exact_field_forms(
    value: &Value,
    accepted: &[&[&str]],
    field: &str,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("coding-session lifecycle command {field} must be an object"))?;
    if accepted
        .iter()
        .any(|keys| object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)))
    {
        return Ok(());
    }
    Err(format!(
        "coding-session lifecycle command {field} has missing or unsupported fields"
    ))
}

fn validate_required(value: &str, field: &str, max_bytes: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > max_bytes {
        return Err(format!("{field} exceeds {max_bytes} bytes"));
    }
    Ok(())
}

fn validate_optional(value: &Option<String>, field: &str, max_bytes: usize) -> Result<(), String> {
    if let Some(value) = value {
        validate_required(value, field, max_bytes)?;
    }
    Ok(())
}

fn validate_provider_authority_pubkey(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("action.providerAuthorityPubkey must be a lowercase 64-hex public key".into());
    }
    Ok(())
}

fn validate_target(target: &CodingSessionTarget) -> Result<(), String> {
    for (field, value) in [
        ("action.session.driver", &target.driver),
        ("action.session.instanceId", &target.instance_id),
        ("action.session.sessionId", &target.session_id),
    ] {
        validate_required(value, field, MAX_IDENTIFIER_BYTES)?;
    }
    if target.generation == 0 || target.generation > MAX_SAFE_GENERATION {
        return Err("action.session.generation must be a positive safe integer".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A syntactically valid project coordinate: 64 lowercase hex, then a `d` tag.
    fn project_coordinate() -> String {
        format!("30621:{}:amas-redux", "cd".repeat(32))
    }

    /// A canonical lowercase umbrella session reference.
    fn session_reference() -> String {
        "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned()
    }

    fn valid_payload() -> CodingSessionLifecycleCommandPayload {
        CodingSessionLifecycleCommandPayload {
            schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: "create-1".into(),
            action: CodingSessionLifecycleAction::SessionCreate {
                project_ref: Some(project_coordinate()),
                repo_ref: Some("30617:owner:amas-redux".into()),
                session_ref: Some(session_reference()),
                genesis_ref: Some("12".repeat(32)),
                provider_instance_ref: "claude-primary".into(),
                provider_authority_pubkey: "ab".repeat(32),
                model: Some("claude-sonnet-4-6".into()),
                title: Some("Advance Buzz live sessions".into()),
                initial_turn: Some("Start with the highest priority task.".into()),
            },
        }
    }

    /// The historical 8-key v1 action, exactly as pre-amendment signers wrote it.
    fn lifecycle_content(project_ref_json: &str) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":{project_ref_json},"repoRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        )
    }

    /// The 9-key action a post-amendment signer writes: `sessionRef` always
    /// present, explicit `null` allowed.
    fn lifecycle_content_with_session_ref(session_ref_json: &str) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":{session_ref_json},"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        )
    }

    /// The authority-aware 10-key action carries both references.
    fn lifecycle_content_with_genesis_ref(
        session_ref_json: &str,
        genesis_ref_json: &str,
    ) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":{session_ref_json},"genesisRef":{genesis_ref_json},"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        )
    }

    #[test]
    fn validates_and_strictly_decodes_the_exact_contract() {
        let payload = valid_payload();
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            decode_coding_session_lifecycle_command(&content).unwrap(),
            payload
        );

        let nulls = lifecycle_content("null");
        assert!(decode_coding_session_lifecycle_command(&nulls).is_ok());
    }

    #[test]
    fn resume_and_stop_round_trip_exact_generation_targets() {
        let target = CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation: 7,
        };
        for action in [
            CodingSessionLifecycleAction::SessionResume {
                session: target.clone(),
                provider_authority_pubkey: "ab".repeat(32),
            },
            CodingSessionLifecycleAction::SessionStop {
                session: target.clone(),
                provider_authority_pubkey: "ab".repeat(32),
            },
        ] {
            let payload = CodingSessionLifecycleCommandPayload {
                schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
                command_id: "lifecycle-1".into(),
                action,
            };
            let content = serde_json::to_string(&payload).unwrap();
            assert_eq!(
                decode_coding_session_lifecycle_command(&content).unwrap(),
                payload
            );
        }

        let smuggled = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"lifecycle-1","action":{{"type":"session.resume","session":{{"driver":"codex-acp","instanceId":"instance-1","sessionId":"session-1","generation":7}},"providerAuthorityPubkey":"{}","cwd":"/tmp"}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&smuggled).is_err());
    }

    /// Fork amendment: a session need not belong to a project.
    #[test]
    fn accepts_a_standalone_session_with_no_project_ref() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *project_ref = None;
        assert!(payload.validate().is_ok());

        let content = serde_json::to_string(&payload).unwrap();
        let decoded = decode_coding_session_lifecycle_command(&content).unwrap();
        assert_eq!(decoded, payload);
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert!(project_ref.is_none());
    }

    /// Optional does not mean unvalidated: a present `projectRef` must be a
    /// real NIP-MP coordinate, so a project-bound session can never be
    /// silently downgraded to a standalone one by a malformed reference.
    #[test]
    fn accepts_only_project_kind_30621_coordinates() {
        assert!(validate_project_ref(&project_coordinate()).is_ok());
        // `d` tags may contain colons — split on the first two only.
        assert!(validate_project_ref(&format!("30621:{}:a:b", "cd".repeat(32))).is_ok());

        let owner = "cd".repeat(32);
        for rejected in [
            format!("30178:{owner}:amas-redux"), // donor-era team catalog
            format!("30617:{owner}:amas-redux"), // repository announcement
            format!("30621:{}:amas-redux", "CD".repeat(32)), // uppercase owner
            format!("30621:{}:amas-redux", "cd".repeat(31)), // short owner
            format!("30621:{owner}:"),           // empty d tag
            format!("30621:{owner}"),            // no d tag at all
            "amas-redux".to_string(),            // bare slug
            String::new(),
        ] {
            assert!(
                validate_project_ref(&rejected).is_err(),
                "should reject {rejected:?}"
            );
        }
    }

    /// Fork amendment: the action is exactly the historical 8-key form or
    /// exactly the 9-key form with `sessionRef`, or exactly the 10-key form
    /// with both references. The 8-key form reads as "no umbrella claimed".
    #[test]
    fn accepts_exactly_the_8_key_9_key_and_10_key_action_forms() {
        let historical = lifecycle_content("null");
        let decoded = decode_coding_session_lifecycle_command(&historical).unwrap();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert!(
            session_ref.is_none(),
            "the pre-amendment form claims no umbrella"
        );

        let explicit_null = lifecycle_content_with_session_ref("null");
        let decoded = decode_coding_session_lifecycle_command(&explicit_null).unwrap();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert!(session_ref.is_none());

        let claimed = lifecycle_content_with_session_ref(&format!("\"{}\"", session_reference()));
        let decoded = decode_coding_session_lifecycle_command(&claimed).unwrap();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert_eq!(session_ref.as_deref(), Some(session_reference().as_str()));

        let genesis_ref = "12".repeat(32);
        let linked = lifecycle_content_with_genesis_ref(
            &format!("\"{}\"", session_reference()),
            &format!("\"{genesis_ref}\""),
        );
        let decoded = decode_coding_session_lifecycle_command(&linked).unwrap();
        let CodingSessionLifecycleAction::SessionCreate {
            session_ref,
            genesis_ref: decoded_genesis_ref,
            ..
        } = &decoded.action
        else {
            panic!("expected create action")
        };
        assert_eq!(session_ref.as_deref(), Some(session_reference().as_str()));
        assert_eq!(decoded_genesis_ref.as_deref(), Some(genesis_ref.as_str()));
    }

    #[test]
    fn genesis_ref_is_non_null_canonical_and_requires_session_ref() {
        let session_ref = format!("\"{}\"", session_reference());
        for rejected_genesis in ["null".to_owned(), "\"short\"".to_owned()] {
            assert!(
                decode_coding_session_lifecycle_command(&lifecycle_content_with_genesis_ref(
                    &session_ref,
                    &rejected_genesis
                ))
                .is_err()
            );
        }
        assert!(
            decode_coding_session_lifecycle_command(&lifecycle_content_with_genesis_ref(
                "null",
                &format!("\"{}\"", "12".repeat(32))
            ))
            .is_err()
        );

        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *session_ref = None;
        assert!(payload.validate().is_err());
    }

    /// Authority-aware producers write both keys; legacy serializers keep
    /// emitting the 9-key `sessionRef` form when no genesis is named.
    #[test]
    fn new_producers_always_write_the_session_ref_key() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *session_ref = None;
        let CodingSessionLifecycleAction::SessionCreate { genesis_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *genesis_ref = None;
        let content = serde_json::to_string(&payload).unwrap();
        assert!(content.contains("\"sessionRef\":null"));
        assert_eq!(
            decode_coding_session_lifecycle_command(&content).unwrap(),
            payload
        );
    }

    /// Optional does not mean unvalidated, and canonical form is the whole
    /// contract: the reference travels in no tag, so nothing downstream ever
    /// normalizes it — a non-canonical spelling would silently split an
    /// umbrella in two.
    #[test]
    fn rejects_a_present_but_malformed_session_ref() {
        assert!(validate_session_ref(&session_reference()).is_ok());

        for rejected in [
            session_reference().to_uppercase(),                // uppercase hex
            session_reference().replace('-', ""),              // no hyphens
            format!("{{{}}}", session_reference()),            // braced form
            format!("urn:uuid:{}", session_reference()),       // URN form
            session_reference()[..35].to_owned(),              // truncated
            format!("{}0", session_reference()),               // too long
            "5b7e1c2a-90d4x4b0e-a1f3-7c2d8e6f4a10".to_owned(), // hyphen misplaced
            "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a1g".to_owned(), // non-hex digit
            " ".repeat(36),                                    // whitespace
            String::new(),
        ] {
            assert!(
                validate_session_ref(&rejected).is_err(),
                "should reject {rejected:?}"
            );
            let content = lifecycle_content_with_session_ref(
                &serde_json::Value::String(rejected.clone()).to_string(),
            );
            assert!(
                decode_coding_session_lifecycle_command(&content).is_err(),
                "decode should reject sessionRef {rejected:?}"
            );
        }

        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { session_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *session_ref = Some("umbrella".into());
        assert!(payload.validate().is_err());
    }

    /// The three exact forms admit nothing between and nothing beyond.
    #[test]
    fn rejects_action_shapes_between_and_beyond_the_three_forms() {
        // 8 keys, but sessionRef standing in for the required repoRef.
        let swapped = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"sessionRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&swapped).is_err());

        // 10 keys, but not the accepted 10-key form: smuggled host path.
        let beyond = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null,"cwd":"/tmp"}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&beyond).is_err());

        // A 9-key form with genesisRef but no sessionRef is never authority-
        // bearing: it is rejected structurally before semantic validation.
        let genesis_without_session = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"genesisRef":"{}","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "12".repeat(32),
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&genesis_without_session).is_err());

        // 11 keys: the accepted 10-key form plus smuggled content.
        let beyond_authority = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"sessionRef":"{}","genesisRef":"{}","providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null,"cwd":"/tmp"}}}}"#,
            session_reference(),
            "12".repeat(32),
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&beyond_authority).is_err());
    }

    #[test]
    fn rejects_a_present_but_malformed_project_ref() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *project_ref = Some("project".into());
        assert!(payload.validate().is_err());

        let content = lifecycle_content("\"project\"");
        assert!(decode_coding_session_lifecycle_command(&content).is_err());
    }

    /// `projectRef` stays structurally required even though it is nullable: a
    /// payload that simply omits the key is a truncation, not a standalone
    /// session, and must not be accepted as one.
    #[test]
    fn rejects_missing_unknown_and_duplicate_fields() {
        let missing_nullable_project_ref = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","repoRef":null,"providerInstanceRef":"provider","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        );
        assert!(
            decode_coding_session_lifecycle_command(&missing_nullable_project_ref).is_err(),
            "an omitted projectRef key is a truncated payload, not a standalone session"
        );

        let missing_nullable = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"providerInstanceRef":"provider","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&missing_nullable).is_err());

        let unknown = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":null,"repoRef":null,"providerInstanceRef":"provider","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null,"cwd":"/tmp"}}}}"#,
            "ab".repeat(32)
        );
        assert!(
            decode_coding_session_lifecycle_command(&unknown).is_err(),
            "a host filesystem path must never ride along in signed content"
        );

        let duplicate = format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","commandId":"create-2","action":{{"type":"session.create","projectRef":null,"repoRef":null,"providerInstanceRef":"provider","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
            "ab".repeat(32)
        );
        assert!(decode_coding_session_lifecycle_command(&duplicate).is_err());
    }

    #[test]
    fn enforces_required_and_optional_string_semantics() {
        let mut payload = valid_payload();
        payload.command_id = " ".into();
        assert!(payload.validate().is_err());

        payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate {
            provider_authority_pubkey,
            ..
        } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *provider_authority_pubkey = "AB".repeat(32);
        assert!(payload.validate().is_err());

        payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { repo_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *repo_ref = Some("\n".into());
        assert!(payload.validate().is_err());

        payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        *project_ref = Some(" ".into());
        assert!(payload.validate().is_err());
    }

    #[test]
    fn utf8_byte_limits_match_the_interoperability_contract() {
        let mut payload = valid_payload();
        match &mut payload.action {
            CodingSessionLifecycleAction::SessionCreate {
                project_ref,
                initial_turn,
                ..
            } => {
                // Pad the `d` segment out to the byte ceiling exactly.
                let prefix = format!("30621:{}:", "cd".repeat(32));
                let padding = MAX_LIFECYCLE_REFERENCE_BYTES - prefix.len();
                *project_ref = Some(format!(
                    "{prefix}{}{}",
                    "é".repeat(padding / 2),
                    "a".repeat(padding % 2)
                ));
                *initial_turn = Some("🐝".repeat(MAX_LIFECYCLE_INITIAL_TURN_BYTES / 4));
            }
            _ => panic!("expected create action"),
        }
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &payload.action
        else {
            panic!("expected create action")
        };
        assert_eq!(
            project_ref.as_deref().map(str::len),
            Some(MAX_LIFECYCLE_REFERENCE_BYTES)
        );
        assert!(payload.validate().is_ok());

        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action
        else {
            panic!("expected create action")
        };
        project_ref.as_mut().unwrap().push('a');
        assert!(payload.validate().is_err());

        let CodingSessionLifecycleAction::SessionCreate {
            project_ref,
            initial_turn,
            ..
        } = &mut payload.action
        else {
            panic!("expected create action")
        };
        project_ref.as_mut().unwrap().pop();
        initial_turn.as_mut().unwrap().push('a');
        assert!(payload.validate().is_err());
    }

    #[test]
    fn rejects_signed_content_over_16_kib() {
        let content = " ".repeat(MAX_LIFECYCLE_CONTENT_BYTES + 1);
        assert!(decode_coding_session_lifecycle_command(&content).is_err());
    }
}
