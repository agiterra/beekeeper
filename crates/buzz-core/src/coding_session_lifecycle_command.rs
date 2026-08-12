//! Provider-neutral coding-session lifecycle command contract.
//!
//! Events use [`crate::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND`] and public
//! JSON so an installed provider adapter can create a session. Event authorship
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

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kind::KIND_PROJECT;

/// The only currently supported coding-session lifecycle command schema.
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
        }
        Ok(())
    }
}

/// Strictly decode and validate signed lifecycle-command content.
///
/// Nullable action fields must be present explicitly, even when their value is
/// `null`. Unknown, duplicate, or missing fields are rejected.
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
    require_exact_fields(
        action,
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
        "action",
    )?;

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

fn require_exact_fields(value: &Value, expected: &[&str], field: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("coding-session lifecycle command {field} must be an object"))?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(format!(
            "coding-session lifecycle command {field} has missing or unsupported fields"
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A syntactically valid project coordinate: 64 lowercase hex, then a `d` tag.
    fn project_coordinate() -> String {
        format!("30621:{}:amas-redux", "cd".repeat(32))
    }

    fn valid_payload() -> CodingSessionLifecycleCommandPayload {
        CodingSessionLifecycleCommandPayload {
            schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: "create-1".into(),
            action: CodingSessionLifecycleAction::SessionCreate {
                project_ref: Some(project_coordinate()),
                repo_ref: Some("30617:owner:amas-redux".into()),
                provider_instance_ref: "claude-primary".into(),
                provider_authority_pubkey: "ab".repeat(32),
                model: Some("claude-sonnet-4-6".into()),
                title: Some("Advance Buzz live sessions".into()),
                initial_turn: Some("Start with the highest priority task.".into()),
            },
        }
    }

    fn lifecycle_content(project_ref_json: &str) -> String {
        format!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-1","action":{{"type":"session.create","projectRef":{project_ref_json},"repoRef":null,"providerInstanceRef":"claude-primary","providerAuthorityPubkey":"{}","model":null,"title":null,"initialTurn":null}}}}"#,
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

    /// Fork amendment: a session need not belong to a project.
    #[test]
    fn accepts_a_standalone_session_with_no_project_ref() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action;
        *project_ref = None;
        assert!(payload.validate().is_ok());

        let content = serde_json::to_string(&payload).unwrap();
        let decoded = decode_coding_session_lifecycle_command(&content).unwrap();
        assert_eq!(decoded, payload);
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &decoded.action;
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

    #[test]
    fn rejects_a_present_but_malformed_project_ref() {
        let mut payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action;
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
        } = &mut payload.action;
        *provider_authority_pubkey = "AB".repeat(32);
        assert!(payload.validate().is_err());

        payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { repo_ref, .. } = &mut payload.action;
        *repo_ref = Some("\n".into());
        assert!(payload.validate().is_err());

        payload = valid_payload();
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action;
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
        }
        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &payload.action;
        assert_eq!(
            project_ref.as_deref().map(str::len),
            Some(MAX_LIFECYCLE_REFERENCE_BYTES)
        );
        assert!(payload.validate().is_ok());

        let CodingSessionLifecycleAction::SessionCreate { project_ref, .. } = &mut payload.action;
        project_ref.as_mut().unwrap().push('a');
        assert!(payload.validate().is_err());

        let CodingSessionLifecycleAction::SessionCreate {
            project_ref,
            initial_turn,
            ..
        } = &mut payload.action;
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
