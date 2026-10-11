//! Public managed-agent configuration and shared input validation.
//!
//! Kind 30177 is world-readable. This opt-in wire type cannot carry keys,
//! attestations, private environment/backend values, local project coordinates,
//! or process metadata. Client custody records remain separate.

use serde::{Deserialize, Serialize};

pub use crate::managed_agent_text::{
    validate_agent_definition_text, validate_managed_agent_definition_text,
};

/// Who the agent should respond to. Defaults to `OwnerOnly`, which matches
/// the harness default → existing agents behave identically.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RespondTo {
    /// Only the attesting owner may give instructions.
    #[default]
    OwnerOnly,
    /// Only explicitly named authors may give instructions.
    Allowlist,
    /// Any permitted conversation author may give instructions.
    Anyone,
}

impl RespondTo {
    /// CLI/env wire string (matches `beekeeper-acp`'s `--respond-to`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OwnerOnly => "owner-only",
            Self::Allowlist => "allowlist",
            Self::Anyone => "anyone",
        }
    }

    /// Parse the NIP-AP wire string. Definitions carry `respond_to` as
    /// opaque data everywhere else; this is the single parse boundary
    /// (instance mint), and an unrecognized mode fails LOUDLY here rather
    /// than silently defaulting — a typo'd definition must not mint an
    /// agent with a different audience than its author intended.
    pub fn parse_wire(value: &str) -> Result<Self, String> {
        match value {
            "owner-only" => Ok(Self::OwnerOnly),
            "allowlist" => Ok(Self::Allowlist),
            "anyone" => Ok(Self::Anyone),
            other => Err(format!(
                "definition respond_to '{other}' is not a recognized mode (expected 'owner-only', 'allowlist', or 'anyone')"
            )),
        }
    }
}

/// Validate and normalize a respond-to allowlist.
///
/// Rules mirror `beekeeper-acp/src/config.rs::validate_allowlist`:
/// - Each entry is exactly 64 hex chars (any case in, lowercase out).
/// - Duplicates removed, insertion order preserved.
///
/// Empty input is allowed here — the boundary check (allowlist mode requires
/// at least one entry) is the caller's job, because an `UpdateManagedAgentRequest`
/// may want to validate a list without yet knowing the final mode.
pub fn validate_respond_to_allowlist(input: &[String]) -> Result<Vec<String>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(input.len());
    for entry in input {
        let trimmed = entry.trim();
        if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "invalid pubkey in respond-to allowlist: '{trimmed}' (must be 64 hex chars)"
            ));
        }
        let lower = trimmed.to_ascii_lowercase();
        if seen.insert(lower.clone()) {
            out.push(lower);
        }
    }
    Ok(out)
}

/// The JSON body stored in a managed-agent event's content field.
///
/// Explicit opt-IN allowlist of the agent's public identity + behavioral
/// config. See the module docs for the exclusion contract — secrets, the
/// provider backend blob, and all runtime/install fields are deliberately
/// absent and must stay that way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedAgentEventContent {
    /// Human-visible instance name.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Linked shared definition, when this instance inherits its behavior.
    pub persona_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Executable instructions for a definition-less instance only.
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Public model choice for a definition-less instance.
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Public model provider name for a definition-less instance.
    pub provider: Option<String>,
    /// `persona_content_hash` of the persona snapshot pinned at create time.
    /// Public drift indicator (not a secret) — lets other clients flag a stale
    /// snapshot without re-reading the source persona.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_source_version: Option<String>,
    /// Requested concurrency ceiling (runtime capabilities may constrain it).
    pub parallelism: u32,
    /// Inbound author gate mode (wire string).
    pub respond_to: RespondTo,
    /// Allowlisted author pubkeys when `respond_to == Allowlist`. These are
    /// public keys, not secrets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub respond_to_allowlist: Vec<String>,
    /// The agent's primary role slug, when the record has one. Public: the
    /// role is what a project's lead hires by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_role: Option<String>,
    /// `project_agent_digest` of the record's `project_ref`, never the
    /// coordinate itself: this event is readable by every relay member and a
    /// project may be private. A claim by the event's author — readers check
    /// the author's authority over the project before believing it. Which
    /// digest, if any, is published is
    /// the client's verified association projection: only a
    /// verified public project's own, otherwise whatever the record carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_digest: Option<String>,
    /// `true` when this agent's association is intentionally unpublished
    /// (`PROJECT_AGENT_WITHDRAWN_CONTENT_KEY`): its project was verified
    /// private here or on another computer of this owner. Carries no digest,
    /// coordinate or project identity, is never serialized with
    /// `project_digest`, and stops the owner's other computers republishing a
    /// digest they carried. See
    /// the client's verified withdrawal projection.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub project_withdrawn: bool,
}

impl ManagedAgentEventContent {
    /// Apply the existing slim-publication rules without consulting private state.
    ///
    /// Callers supply only a verified/carry-forward project digest or withdrawal.
    /// Linked definitions own the executable prompt/model/provider snapshot, so
    /// those values are omitted from an instance publication. Local custody and
    /// deployment state cannot enter this projection's type.
    pub fn public_projection(&self) -> Self {
        let mut projected = self.clone();
        if projected.persona_id.is_some() {
            projected.system_prompt = None;
            projected.model = None;
            projected.provider = None;
            projected.persona_source_version = None;
        }
        projected.home_role = projected
            .home_role
            .as_deref()
            .map(str::trim)
            .filter(|role| !role.is_empty())
            .map(str::to_owned);
        if projected.project_withdrawn {
            projected.project_digest = None;
        }
        projected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_private_fields_cannot_reach_public_serialization() {
        let incoming = serde_json::json!({
            "name":"Worker", "parallelism":1, "respond_to":"owner-only",
            "private_key_nsec":"SECRET", "auth_tag":"ATTESTATION",
            "env_vars":{"TOKEN":"SECRET"}, "backend":{"config":"SECRET"},
            "project_ref":"PRIVATE_PROJECT", "runtime_pid":123
        });
        let public: ManagedAgentEventContent =
            serde_json::from_value(incoming).expect("public wire");
        let outgoing = serde_json::to_value(public).expect("public JSON");
        for key in [
            "private_key_nsec",
            "auth_tag",
            "env_vars",
            "backend",
            "project_ref",
            "runtime_pid",
        ] {
            assert!(
                outgoing.get(key).is_none(),
                "{key} must not enter public wire"
            );
        }
    }

    #[test]
    fn linked_definition_and_withdrawal_keep_private_or_inert_fields_off_wire() {
        let incoming = serde_json::json!({
            "name":"Worker", "parallelism":1, "respond_to":"owner-only",
            "persona_id":"worker", "system_prompt":"inert", "model":"model",
            "provider":"provider", "persona_source_version":"hash", "home_role":"  worker  ",
            "project_digest":"old", "project_withdrawn":true
        });
        let public: ManagedAgentEventContent = serde_json::from_value(incoming).expect("wire");
        let outgoing = serde_json::to_value(public.public_projection()).expect("projection");
        for key in [
            "system_prompt",
            "model",
            "provider",
            "persona_source_version",
            "project_digest",
        ] {
            assert!(outgoing.get(key).is_none(), "{key} must be omitted");
        }
        assert_eq!(outgoing["home_role"], "worker");
        assert_eq!(outgoing["project_withdrawn"], true);
    }
}
