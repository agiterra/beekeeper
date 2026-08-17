//! Private, provider-neutral context package for rehydrating a coding session.
//!
//! This is not a Nostr event contract. A provider projects already-published,
//! signature-verified relay facts into this bounded serializable package and
//! stores or serves it privately on the destination machine. The package never
//! contains a Nostr signing key, host path, ACP cursor, or fabricated native
//! Claude/Codex session record.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::coding_session_command::{
    coding_session_target_key, CodingSessionTarget, MAX_IDENTIFIER_BYTES, MAX_SAFE_GENERATION,
};
use crate::coding_session_lifecycle_command::validate_session_ref;

/// Current private context-package schema version.
pub const CODING_SESSION_CONTEXT_PACKAGE_VERSION: u64 = 1;
/// Maximum verified history items carried in one package.
pub const MAX_CONTEXT_HISTORY_ITEMS: usize = 4_096;
/// Maximum serialized size of one complete package.
pub const MAX_CONTEXT_PACKAGE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum provenance notes carried in one package.
pub const MAX_CONTEXT_PROVENANCE_NOTES: usize = 32;
/// Maximum UTF-8 byte length of one provenance note.
pub const MAX_CONTEXT_PROVENANCE_NOTE_BYTES: usize = 512;
/// Maximum UTF-8 byte length of optional name text.
pub const MAX_CONTEXT_NAME_BYTES: usize = 256;
/// Maximum UTF-8 byte length of optional goal text.
pub const MAX_CONTEXT_GOAL_BYTES: usize = 4_096;
/// Maximum UTF-8 byte length of an item discriminator.
pub const MAX_CONTEXT_ITEM_KIND_BYTES: usize = 128;
/// Maximum serialized bytes of one structured transcript item.
///
/// The signed kind-44225 envelope is capped at 32 KiB, so a valid projected
/// item is always below this ceiling. Keeping the bound in the shared package
/// contract ensures a history page can return an accepted item in full.
pub const MAX_CONTEXT_HISTORY_CONTENT_BYTES: usize = 32 * 1024;

/// A private, bounded reconstruction of one durable Buzz session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionContextPackage {
    /// Package schema version; currently exactly `1`.
    pub v: u64,
    /// Durable umbrella identity and human-authored context.
    pub session: CodingSessionContextIdentity,
    /// Coverage, truncation, and construction provenance.
    pub provenance: CodingSessionContextProvenance,
    /// Ordered, signature-verified provider transcript facts.
    pub history: Vec<CodingSessionContextHistoryItem>,
}

/// Durable identity fields shared by every execution in the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionContextIdentity {
    /// Canonical umbrella UUID.
    pub session_ref: String,
    /// Exact lowercase event id of the canonical genesis.
    pub genesis_ref: String,
    /// Channel containing the genesis, creates, and provider facts.
    pub channel_id: Uuid,
    /// Latest verified founder-authored navigation name, when supplied.
    pub name: Option<String>,
    /// Latest verified founder-authored goal, when supplied.
    pub goal: Option<String>,
    /// Project coordinate from the verified create chain, or standalone.
    pub project_ref: Option<String>,
}

/// Honest statement of what the projector saw and what it retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionContextProvenance {
    /// Epoch milliseconds when this local package was generated.
    pub generated_at: i64,
    /// Whether the source query proved it had the complete relevant history.
    pub complete: bool,
    /// Whether package bounds omitted otherwise-valid history items.
    pub truncated: bool,
    /// Number of signed source facts retained in the verified proof graph.
    pub source_event_count: u64,
    /// Number of history items retained below.
    pub included_history_items: u64,
    /// Number of verified history items omitted due package bounds.
    pub omitted_history_items: u64,
    /// Total relevant history items, when the source established that number.
    pub total_history_items: Option<u64>,
    /// Bounded human-readable coverage or truncation explanations.
    pub notes: Vec<String>,
}

/// Semantic role of a verified provider transcript item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingSessionContextRole {
    /// Operator prompt recorded by the provider.
    User,
    /// Provider-projected assistant output.
    Assistant,
    /// Provider-projected tool call or result.
    Tool,
    /// Provider-projected reasoning/thought text.
    Reasoning,
    /// Turn, plan, or execution lifecycle state.
    Lifecycle,
    /// Other recognized provider bookkeeping that may matter to continuity.
    System,
}

/// One retained, signature-verified kind-44225 transcript fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionContextHistoryItem {
    /// Signed source event id.
    pub event_id: String,
    /// Signed source event creation time in Unix seconds.
    pub created_at: u64,
    /// Provider-authority pubkey that signed the source event.
    pub author: String,
    /// Source Nostr kind; v1 requires exactly `44225`.
    pub source_kind: u32,
    /// Exact provider execution generation this item belongs to.
    pub target: CodingSessionTarget,
    /// Monotonic transcript sequence within `target`.
    pub event_seq: u64,
    /// Provider turn id, when the source carried one.
    pub turn_id: Option<String>,
    /// Stable semantic role derived from `item_kind`.
    pub role: CodingSessionContextRole,
    /// Exact recognized transcript item discriminator.
    pub item_kind: String,
    /// Deeply redacted structured transcript item from the signed envelope.
    pub content: Value,
}

/// Map a recognized transcript item discriminator to its package role.
pub fn coding_session_context_role_for_item_kind(
    item_kind: &str,
) -> Option<CodingSessionContextRole> {
    match item_kind {
        "user_prompt" => Some(CodingSessionContextRole::User),
        "assistant_text" => Some(CodingSessionContextRole::Assistant),
        "tool_call" | "tool_result" => Some(CodingSessionContextRole::Tool),
        "reasoning" => Some(CodingSessionContextRole::Reasoning),
        "result" | "status" | "interrupted" | "plan" => Some(CodingSessionContextRole::Lifecycle),
        "system_init"
        | "account_info"
        | "context_window_updated"
        | "compact_boundary"
        | "compact_summary"
        | "context_cleared"
        | "elided" => Some(CodingSessionContextRole::System),
        _ => None,
    }
}

impl CodingSessionContextPackage {
    /// Validate the complete private package before storing or serving it.
    pub fn validate(&self) -> Result<(), String> {
        if self.v != CODING_SESSION_CONTEXT_PACKAGE_VERSION {
            return Err(format!(
                "unsupported coding-session context package version {}",
                self.v
            ));
        }
        self.session.validate()?;
        self.provenance.validate(self.history.len())?;
        if self.history.len() > MAX_CONTEXT_HISTORY_ITEMS {
            return Err(format!(
                "context history exceeds {MAX_CONTEXT_HISTORY_ITEMS} items"
            ));
        }

        let mut event_ids = HashSet::with_capacity(self.history.len());
        let mut target_sequences = HashSet::with_capacity(self.history.len());
        for item in &self.history {
            item.validate()?;
            if !event_ids.insert(item.event_id.as_str()) {
                return Err(format!(
                    "context history repeats event id {}",
                    item.event_id
                ));
            }
            let target_key = coding_session_target_key(&item.target);
            if !target_sequences.insert((target_key.clone(), item.event_seq)) {
                return Err(format!(
                    "context history conflicts at target {target_key} sequence {}",
                    item.event_seq
                ));
            }
        }

        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("context package serialization failed: {error}"))?;
        if encoded.len() > MAX_CONTEXT_PACKAGE_BYTES {
            return Err(format!(
                "context package exceeds {MAX_CONTEXT_PACKAGE_BYTES} serialized bytes"
            ));
        }
        Ok(())
    }
}

impl CodingSessionContextIdentity {
    fn validate(&self) -> Result<(), String> {
        validate_session_ref(&self.session_ref)
            .map_err(|_| "context sessionRef is not a canonical lowercase UUID".to_owned())?;
        validate_lower_hex("context genesisRef", &self.genesis_ref, 64)?;
        validate_optional_text("context name", self.name.as_deref(), MAX_CONTEXT_NAME_BYTES)?;
        validate_optional_text("context goal", self.goal.as_deref(), MAX_CONTEXT_GOAL_BYTES)?;
        if let Some(project_ref) = &self.project_ref {
            validate_project_ref(project_ref)?;
        }
        Ok(())
    }
}

impl CodingSessionContextProvenance {
    fn validate(&self, actual_history_len: usize) -> Result<(), String> {
        if self.generated_at < 0 {
            return Err("context provenance generatedAt must be non-negative".into());
        }
        if self.included_history_items != actual_history_len as u64 {
            return Err("context provenance includedHistoryItems disagrees with history".into());
        }
        if self.truncated != (self.omitted_history_items > 0) {
            return Err("context provenance truncated must match omittedHistoryItems".into());
        }
        if let Some(total) = self.total_history_items {
            let accounted = self
                .included_history_items
                .checked_add(self.omitted_history_items)
                .ok_or_else(|| "context provenance history counts overflow".to_owned())?;
            if total != accounted {
                return Err(
                    "context provenance totalHistoryItems disagrees with included + omitted".into(),
                );
            }
        }
        if self.complete && self.total_history_items.is_none() {
            return Err("complete context provenance requires totalHistoryItems".into());
        }
        if self.notes.len() > MAX_CONTEXT_PROVENANCE_NOTES {
            return Err(format!(
                "context provenance exceeds {MAX_CONTEXT_PROVENANCE_NOTES} notes"
            ));
        }
        for note in &self.notes {
            if note.trim().is_empty() || note.len() > MAX_CONTEXT_PROVENANCE_NOTE_BYTES {
                return Err(format!(
                    "context provenance notes must contain text and be at most {MAX_CONTEXT_PROVENANCE_NOTE_BYTES} bytes"
                ));
            }
        }
        Ok(())
    }
}

impl CodingSessionContextHistoryItem {
    fn validate(&self) -> Result<(), String> {
        validate_lower_hex("context history eventId", &self.event_id, 64)?;
        validate_lower_hex("context history author", &self.author, 64)?;
        if self.source_kind != crate::kind::KIND_CODING_SESSION_TRANSCRIPT {
            return Err("context history sourceKind must be 44225".into());
        }
        validate_target(&self.target)?;
        if self.event_seq == 0 {
            return Err("context history eventSeq must be positive".into());
        }
        if let Some(turn_id) = &self.turn_id {
            validate_nonempty_bounded("context history turnId", turn_id, MAX_IDENTIFIER_BYTES)?;
        }
        validate_nonempty_bounded(
            "context history itemKind",
            &self.item_kind,
            MAX_CONTEXT_ITEM_KIND_BYTES,
        )?;
        let content_kind = self
            .content
            .as_object()
            .and_then(|object| object.get("kind"))
            .and_then(Value::as_str)
            .ok_or_else(|| "context history content must carry a string kind".to_owned())?;
        if content_kind != self.item_kind {
            return Err("context history itemKind disagrees with content.kind".into());
        }
        let expected_role =
            coding_session_context_role_for_item_kind(&self.item_kind).ok_or_else(|| {
                format!(
                    "unrecognized context history item kind {:?}",
                    self.item_kind
                )
            })?;
        if self.role != expected_role {
            return Err("context history role disagrees with itemKind".into());
        }
        let content_bytes = serde_json::to_vec(&self.content)
            .map_err(|error| format!("context history content serialization failed: {error}"))?;
        if content_bytes.len() > MAX_CONTEXT_HISTORY_CONTENT_BYTES {
            return Err(format!(
                "context history content exceeds {MAX_CONTEXT_HISTORY_CONTENT_BYTES} bytes"
            ));
        }
        Ok(())
    }
}

fn validate_target(target: &CodingSessionTarget) -> Result<(), String> {
    validate_nonempty_bounded(
        "context target.driver",
        &target.driver,
        MAX_IDENTIFIER_BYTES,
    )?;
    validate_nonempty_bounded(
        "context target.instanceId",
        &target.instance_id,
        MAX_IDENTIFIER_BYTES,
    )?;
    validate_nonempty_bounded(
        "context target.sessionId",
        &target.session_id,
        MAX_IDENTIFIER_BYTES,
    )?;
    if target.generation == 0 || target.generation > MAX_SAFE_GENERATION {
        return Err("context target generation must be a positive safe integer".into());
    }
    Ok(())
}

fn validate_project_ref(value: &str) -> Result<(), String> {
    let mut parts = value.splitn(3, ':');
    if parts.next() != Some("30621") {
        return Err("context projectRef must use kind 30621".into());
    }
    let owner = parts
        .next()
        .ok_or_else(|| "context projectRef is missing its owner".to_owned())?;
    let d_tag = parts
        .next()
        .ok_or_else(|| "context projectRef is missing its d tag".to_owned())?;
    validate_lower_hex("context projectRef owner", owner, 64)?;
    if d_tag.is_empty() {
        return Err("context projectRef d tag must not be empty".into());
    }
    Ok(())
}

fn validate_optional_text(field: &str, value: Option<&str>, max: usize) -> Result<(), String> {
    if let Some(value) = value {
        validate_nonempty_bounded(field, value, max)?;
    }
    Ok(())
}

fn validate_nonempty_bounded(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must contain text"));
    }
    if value.len() > max {
        return Err(format!("{field} exceeds {max} bytes"));
    }
    Ok(())
}

fn validate_lower_hex(field: &str, value: &str, length: usize) -> Result<(), String> {
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be {length} lowercase hex characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "codex-primary".into(),
            session_id: "session-1".into(),
            generation: 1,
        }
    }

    fn history(seq: u64, kind: &str) -> CodingSessionContextHistoryItem {
        CodingSessionContextHistoryItem {
            event_id: format!("{seq:064x}"),
            created_at: seq,
            author: "ab".repeat(32),
            source_kind: crate::kind::KIND_CODING_SESSION_TRANSCRIPT,
            target: target(),
            event_seq: seq,
            turn_id: Some("turn-1".into()),
            role: coding_session_context_role_for_item_kind(kind).unwrap(),
            item_kind: kind.into(),
            content: serde_json::json!({"kind": kind, "text": "verified relay fact"}),
        }
    }

    fn package() -> CodingSessionContextPackage {
        CodingSessionContextPackage {
            v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
            session: CodingSessionContextIdentity {
                session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
                genesis_ref: "cd".repeat(32),
                channel_id: Uuid::nil(),
                name: Some("Rehydrated context".into()),
                goal: Some("Continue from verified durable facts".into()),
                project_ref: Some(format!("30621:{}:buzz", "ef".repeat(32))),
            },
            provenance: CodingSessionContextProvenance {
                generated_at: 1,
                complete: true,
                truncated: false,
                source_event_count: 4,
                included_history_items: 2,
                omitted_history_items: 0,
                total_history_items: Some(2),
                notes: vec!["Relay query reached EOSE without a local package truncation".into()],
            },
            history: vec![history(1, "user_prompt"), history(2, "assistant_text")],
        }
    }

    #[test]
    fn valid_package_round_trips_as_strict_json() {
        let package = package();
        package.validate().unwrap();
        let encoded = serde_json::to_string(&package).unwrap();
        let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, package);
        decoded.validate().unwrap();
    }

    #[test]
    fn completeness_and_truncation_are_independent_but_accounted() {
        let mut package = package();
        package.provenance.complete = false;
        package.provenance.total_history_items = None;
        package.validate().unwrap();

        package.provenance.truncated = true;
        package.provenance.omitted_history_items = 3;
        package.validate().unwrap();
        package.provenance.truncated = false;
        assert!(package.validate().is_err());
    }

    #[test]
    fn rejects_conflicts_and_semantic_role_substitution() {
        {
            let mut package = package();
            package.history[1].event_seq = 1;
            assert!(package.validate().is_err());
        }

        {
            let mut package = package();
            package.history[0].role = CodingSessionContextRole::Assistant;
            assert!(package.validate().is_err());
        }

        let mut package = package();
        package.history[0].content["kind"] = Value::String("assistant_text".into());
        assert!(package.validate().is_err());
    }

    #[test]
    fn rejects_unknown_fields_and_unrecognized_item_kinds() {
        let encoded = serde_json::to_value(package()).unwrap();
        let mut smuggled = encoded.clone();
        smuggled
            .as_object_mut()
            .unwrap()
            .insert("privateKey".into(), Value::String("secret".into()));
        assert!(serde_json::from_value::<CodingSessionContextPackage>(smuggled).is_err());

        let mut package = package();
        package.history[0].item_kind = "native_provider_state".into();
        package.history[0].content["kind"] = Value::String("native_provider_state".into());
        assert!(package.validate().is_err());
    }

    #[test]
    fn rejects_an_item_too_large_to_return_in_full() {
        let mut package = package();
        package.history[0].content["text"] =
            Value::String("x".repeat(MAX_CONTEXT_HISTORY_CONTENT_BYTES + 1));
        assert!(package.validate().is_err());
    }
}
