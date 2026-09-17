//! Autorun grants for project actions (spec § 5.4).
//!
//! Every `run_on_host` step is gated on the operator's approval. A grant
//! (kind:46030) whose content says `scope: action` also records an **autorun
//! grant** bound to the workflow's current definition hash, and later runs of
//! that exact definition skip the gate. Editing the action changes the hash
//! and re-arms approval; a kind:46032 revokes it by hand. The relay records
//! both changes as a relay-signed kind:46015 so every reader sees the same
//! fact.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::kind::{event_kind_u32, KIND_WORKFLOW_AUTORUN_CHANGED, KIND_WORKFLOW_AUTORUN_REVOKE};

/// Exact schema named by the 46015 and 46032 payloads.
pub const AUTORUN_SCHEMA: &str = "buzz-workflow-autorun/v1";
/// Maximum UTF-8 byte length of a grant note.
pub const MAX_APPROVAL_NOTE_BYTES: usize = 2048;

/// What an approval grant releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// This run only — the default, and what every pre-C4 client meant.
    #[default]
    Run,
    /// This run and every later run of the same definition hash.
    Action,
}

impl ApprovalScope {
    /// The wire word.
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalScope::Run => "run",
            ApprovalScope::Action => "action",
        }
    }
}

/// The content of a kind:46030 grant, once decoded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApprovalGrantContent {
    /// The approver's note, if any.
    pub note: Option<String>,
    /// What the grant releases.
    pub scope: ApprovalScope,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantContentWire {
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    scope: ApprovalScope,
}

/// Decode a kind:46030's content.
///
/// A JSON object `{"note": …, "scope": "run" | "action"}` is the C4 form.
/// Anything else — the empty string, or plain text — is the pre-C4 form: the
/// text is the note and the scope is `run`, so an older desktop or CLI keeps
/// working and can never grant more than it asked for. A JSON object with a
/// key this decoder does not know is refused rather than read as a note.
pub fn decode_approval_grant_content(content: &str) -> Result<ApprovalGrantContent, String> {
    let trimmed = content.trim();
    if trimmed.starts_with('{') {
        let wire: GrantContentWire = serde_json::from_str(trimmed)
            .map_err(|error| format!("malformed approval grant content: {error}"))?;
        let note = wire
            .note
            .map(|note| note.trim().to_owned())
            .filter(|note| !note.is_empty());
        if let Some(note) = &note {
            if note.len() > MAX_APPROVAL_NOTE_BYTES {
                return Err(format!(
                    "approval note exceeds {MAX_APPROVAL_NOTE_BYTES} bytes"
                ));
            }
        }
        return Ok(ApprovalGrantContent {
            note,
            scope: wire.scope,
        });
    }
    if trimmed.len() > MAX_APPROVAL_NOTE_BYTES {
        return Err(format!(
            "approval note exceeds {MAX_APPROVAL_NOTE_BYTES} bytes"
        ));
    }
    Ok(ApprovalGrantContent {
        note: (!trimmed.is_empty()).then(|| trimmed.to_owned()),
        scope: ApprovalScope::Run,
    })
}

/// Encode a kind:46030's content in the C4 form.
pub fn encode_approval_grant_content(note: Option<&str>, scope: ApprovalScope) -> String {
    serde_json::json!({
        "note": note.map(str::trim).filter(|note| !note.is_empty()),
        "scope": scope,
    })
    .to_string()
}

/// Whether a 46015 records a grant or a revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutorunChange {
    /// An autorun grant now stands for the definition hash.
    Granted,
    /// Every autorun grant for the workflow was revoked.
    Revoked,
}

/// Content of a relay-signed kind:46015.
///
/// Tags: `d` = workflow id, `h` = channel, `schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutorunChanged {
    /// Must equal [`AUTORUN_SCHEMA`].
    pub schema: String,
    /// Canonical lowercase UUID of the workflow.
    pub workflow_id: String,
    /// Lowercase hex SHA-256 of the definition the grant is bound to.
    pub definition_hash: String,
    /// Granted or revoked.
    pub change: AutorunChange,
    /// Lowercase hex pubkey of the operator whose event caused it.
    pub by: String,
    /// Event id (64 lowercase hex) of that kind:46030 or kind:46032.
    pub source_event_id: String,
    /// Canonical lowercase UUID of the workflow's channel.
    pub channel_id: String,
}

/// Content of an operator-signed kind:46032.
///
/// Tags: `d` = workflow id, `h` = channel, `schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutorunRevoke {
    /// Must equal [`AUTORUN_SCHEMA`].
    pub schema: String,
    /// Canonical lowercase UUID of the workflow whose grants are revoked.
    pub workflow_id: String,
    /// Canonical lowercase UUID of the workflow's channel.
    pub channel_id: String,
}

/// Build the exact tags and content of a kind:46015.
pub fn build_autorun_changed(
    changed: &AutorunChanged,
) -> Result<(Vec<Vec<String>>, String), String> {
    require_schema(&changed.schema)?;
    canonical_uuid("workflow id", &changed.workflow_id)?;
    canonical_uuid("channel id", &changed.channel_id)?;
    hex64("autorun definition hash", &changed.definition_hash)?;
    hex64("autorun by", &changed.by)?;
    hex64("autorun source event id", &changed.source_event_id)?;
    let tags = vec![
        vec!["d".into(), changed.workflow_id.clone()],
        vec!["h".into(), changed.channel_id.clone()],
        vec!["schema".into(), AUTORUN_SCHEMA.into()],
    ];
    let content = serde_json::to_string(changed)
        .map_err(|error| format!("autorun change could not be encoded: {error}"))?;
    Ok((tags, content))
}

/// Decode and validate a kind:46015, including exact tag agreement.
pub fn decode_autorun_changed(event: &nostr::Event) -> Result<AutorunChanged, String> {
    if event_kind_u32(event) != KIND_WORKFLOW_AUTORUN_CHANGED {
        return Err(format!(
            "autorun change must be kind {KIND_WORKFLOW_AUTORUN_CHANGED}"
        ));
    }
    let changed: AutorunChanged = serde_json::from_str(&event.content)
        .map_err(|error| format!("malformed autorun change content: {error}"))?;
    let (expected, _) = build_autorun_changed(&changed)?;
    require_exact_tags(event, &expected, "autorun change")?;
    Ok(changed)
}

/// Build the exact tags and content of a kind:46032.
pub fn build_autorun_revoke(revoke: &AutorunRevoke) -> Result<(Vec<Vec<String>>, String), String> {
    require_schema(&revoke.schema)?;
    canonical_uuid("workflow id", &revoke.workflow_id)?;
    canonical_uuid("channel id", &revoke.channel_id)?;
    let tags = vec![
        vec!["d".into(), revoke.workflow_id.clone()],
        vec!["h".into(), revoke.channel_id.clone()],
        vec!["schema".into(), AUTORUN_SCHEMA.into()],
    ];
    let content = serde_json::to_string(revoke)
        .map_err(|error| format!("autorun revoke could not be encoded: {error}"))?;
    Ok((tags, content))
}

/// Decode and validate a kind:46032, including exact tag agreement.
pub fn decode_autorun_revoke(event: &nostr::Event) -> Result<AutorunRevoke, String> {
    if event_kind_u32(event) != KIND_WORKFLOW_AUTORUN_REVOKE {
        return Err(format!(
            "autorun revoke must be kind {KIND_WORKFLOW_AUTORUN_REVOKE}"
        ));
    }
    let revoke: AutorunRevoke = serde_json::from_str(&event.content)
        .map_err(|error| format!("malformed autorun revoke content: {error}"))?;
    let (expected, _) = build_autorun_revoke(&revoke)?;
    require_exact_tags(event, &expected, "autorun revoke")?;
    Ok(revoke)
}

fn require_schema(schema: &str) -> Result<(), String> {
    if schema != AUTORUN_SCHEMA {
        return Err(format!("autorun schema must be {AUTORUN_SCHEMA:?}"));
    }
    Ok(())
}

fn canonical_uuid(label: &str, value: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(value).map_err(|_| format!("autorun {label} must be a UUID"))?;
    if parsed.to_string() != value {
        return Err(format!(
            "autorun {label} must be a lowercase canonical UUID"
        ));
    }
    Ok(())
}

fn hex64(label: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} must be lowercase 64-hex"));
    }
    Ok(())
}

fn require_exact_tags(
    event: &nostr::Event,
    expected: &[Vec<String>],
    label: &str,
) -> Result<(), String> {
    if event.tags.len() != expected.len() {
        return Err(format!("{label} requires exactly {} tags", expected.len()));
    }
    for tag in expected {
        let matches = event
            .tags
            .iter()
            .filter(|candidate| candidate.as_slice() == tag.as_slice())
            .count();
        if matches != 1 {
            return Err(format!(
                "{label} tags must exactly match its validated content"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn signed(kind: u32, tags: Vec<Vec<String>>, content: String) -> nostr::Event {
        let tags: Vec<Tag> = tags
            .into_iter()
            .map(|tag| Tag::parse(tag).expect("tag"))
            .collect();
        EventBuilder::new(Kind::from(kind as u16), content)
            .tags(tags)
            .sign_with_keys(&Keys::generate())
            .expect("sign")
    }

    #[test]
    fn grant_content_reads_both_forms_and_never_widens_a_plain_note() {
        let plain = decode_approval_grant_content("looks fine").expect("plain");
        assert_eq!(plain.note.as_deref(), Some("looks fine"));
        assert_eq!(plain.scope, ApprovalScope::Run);
        let empty = decode_approval_grant_content("").expect("empty");
        assert_eq!(empty, ApprovalGrantContent::default());
        let action =
            decode_approval_grant_content(r#"{"note":"ship it","scope":"action"}"#).expect("json");
        assert_eq!(action.scope, ApprovalScope::Action);
        assert_eq!(action.note.as_deref(), Some("ship it"));
        let bare = decode_approval_grant_content(r#"{"scope":"run"}"#).expect("json");
        assert_eq!(bare.note, None);
        assert!(decode_approval_grant_content(r#"{"scope":"forever"}"#).is_err());
        assert!(decode_approval_grant_content(r#"{"scope":"action","allow":true}"#).is_err());
        let encoded = encode_approval_grant_content(Some("  ok "), ApprovalScope::Action);
        assert_eq!(
            decode_approval_grant_content(&encoded).expect("round trip"),
            ApprovalGrantContent {
                note: Some("ok".into()),
                scope: ApprovalScope::Action
            }
        );
    }

    #[test]
    fn autorun_change_and_revoke_round_trip_with_exact_tags() {
        let changed = AutorunChanged {
            schema: AUTORUN_SCHEMA.into(),
            workflow_id: Uuid::from_u128(7).to_string(),
            definition_hash: "a".repeat(64),
            change: AutorunChange::Granted,
            by: "b".repeat(64),
            source_event_id: "c".repeat(64),
            channel_id: Uuid::from_u128(9).to_string(),
        };
        let (tags, content) = build_autorun_changed(&changed).expect("build");
        let event = signed(KIND_WORKFLOW_AUTORUN_CHANGED, tags.clone(), content.clone());
        assert_eq!(decode_autorun_changed(&event).expect("decode"), changed);
        let mut extra = tags;
        extra.push(vec!["p".into(), "b".repeat(64)]);
        assert!(
            decode_autorun_changed(&signed(KIND_WORKFLOW_AUTORUN_CHANGED, extra, content)).is_err()
        );

        let revoke = AutorunRevoke {
            schema: AUTORUN_SCHEMA.into(),
            workflow_id: Uuid::from_u128(7).to_string(),
            channel_id: Uuid::from_u128(9).to_string(),
        };
        let (tags, content) = build_autorun_revoke(&revoke).expect("build");
        let event = signed(KIND_WORKFLOW_AUTORUN_REVOKE, tags, content);
        assert_eq!(decode_autorun_revoke(&event).expect("decode"), revoke);
    }
}
