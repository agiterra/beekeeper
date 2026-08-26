//! Private, provider-neutral context package for rehydrating a coding session.
//!
//! This is not a Nostr event contract. A provider projects already-published,
//! signature-verified relay facts into this bounded serializable package and
//! stores or serves it privately on the destination machine. The package never
//! contains a Nostr signing key, host path, ACP cursor, or fabricated native
//! Claude/Codex session record.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::coding_session_command::{
    coding_session_target_key, CodingSessionTarget, MAX_IDENTIFIER_BYTES, MAX_SAFE_GENERATION,
};
use crate::coding_session_lifecycle_command::validate_session_ref;

/// Current private context-package schema version.
pub const CODING_SESSION_CONTEXT_PACKAGE_VERSION: u64 = 2;
/// Oldest private context-package schema version a reader still accepts.
///
/// Version 2 only adds the optional `sourceEventBreakdown` reconciliation, so
/// a version-1 package still validates unchanged. The bump exists so that an
/// *older* reader — whose provenance struct is `deny_unknown_fields` — fails
/// with "unsupported … package version 2" instead of an opaque unknown-field
/// error.
pub const MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION: u64 = 1;
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
/// Maximum completed or active turns included in the first-turn brief.
pub const MAX_CONTEXT_BRIEF_TURNS: usize = 8;
/// Maximum tool attempts retained for one turn in the first-turn brief.
pub const MAX_CONTEXT_BRIEF_TOOL_ATTEMPTS: usize = 8;
/// Maximum source-event references retained for one brief turn.
pub const MAX_CONTEXT_BRIEF_EVIDENCE_IDS: usize = 16;
/// Maximum safe prose bytes copied into one brief field.
pub const MAX_CONTEXT_BRIEF_TEXT_BYTES: usize = 1_024;
/// Maximum serialized bytes pushed before the first model token.
pub const MAX_CONTEXT_FIRST_TURN_BRIEF_BYTES: usize = 12 * 1024;

/// A private, bounded reconstruction of one durable Buzz session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionContextPackage {
    /// Package schema version; readers accept
    /// [`MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION`] through
    /// [`CODING_SESSION_CONTEXT_PACKAGE_VERSION`].
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
    /// Snapshot watermark for a source query that reported complete coverage.
    ///
    /// This bounds `complete`: it never means "current forever." `None` is
    /// accepted only for packages produced before this additive field existed
    /// or for a source query that could not prove completeness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete_as_of: Option<i64>,
    /// Whether the source query proved it had the complete relevant history.
    pub complete: bool,
    /// Whether package bounds omitted otherwise-valid history items.
    pub truncated: bool,
    /// Number of signed source facts retained in the verified proof graph.
    pub source_event_count: u64,
    /// Per-category accounting that reconciles `source_event_count`.
    ///
    /// Additive and optional: absent on packages produced before this field
    /// existed. When present, [`CodingSessionContextProvenance::validate`]
    /// requires it to sum exactly to `source_event_count`, so a reader never
    /// has to trust a breakdown that does not add up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_breakdown: Option<CodingSessionContextSourceBreakdown>,
    /// Number of history items retained below.
    pub included_history_items: u64,
    /// Number of verified history items omitted due package bounds.
    pub omitted_history_items: u64,
    /// Total relevant history items, when the source established that number.
    pub total_history_items: Option<u64>,
    /// Bounded human-readable coverage or truncation explanations.
    pub notes: Vec<String>,
}

/// Per-category accounting of the signed proof events behind one package.
///
/// The six terms sum to `CodingSessionContextProvenance::source_event_count`
/// by construction; only `transcript_events` can become history items, which
/// is why `sourceEventCount` exceeds `totalHistoryItems`. Without this
/// breakdown the two numbers sit side by side in a response with no way for a
/// reader to account for the difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionContextSourceBreakdown {
    /// Canonical genesis facts; always exactly `1`.
    pub genesis_events: u64,
    /// Authority link facts, counted at two events per link.
    pub authority_link_events: u64,
    /// Verified founder-authored name revisions.
    pub name_revision_events: u64,
    /// Verified founder-authored goal revisions.
    pub goal_revision_events: u64,
    /// Per-generation bookkeeping facts, counted at three events per
    /// execution generation.
    pub generation_bookkeeping_events: u64,
    /// Kind-44225 transcript facts — the only category that can become a
    /// history item.
    pub transcript_events: u64,
}

impl CodingSessionContextSourceBreakdown {
    /// Sum of the six terms.
    ///
    /// Saturates instead of wrapping. A saturated total can never be mistaken
    /// for a reconciliation: [`CodingSessionContextProvenance::validate`] sums
    /// with checked arithmetic and rejects a breakdown that overflows.
    pub fn total(&self) -> u64 {
        self.checked_total().unwrap_or(u64::MAX)
    }

    fn checked_total(&self) -> Option<u64> {
        [
            self.genesis_events,
            self.authority_link_events,
            self.name_revision_events,
            self.goal_revision_events,
            self.generation_bookkeeping_events,
            self.transcript_events,
        ]
        .into_iter()
        .try_fold(0u64, |total, term| total.checked_add(term))
    }
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

/// Build the bounded, deterministic orientation delivered before a new
/// execution's first token.
///
/// The brief is an index over verified relay facts, not a model summary. It
/// carries the durable objective when it is safe to repeat, recent turn and
/// tool outcomes, evidence event ids, and an explicit snapshot watermark. It
/// deliberately excludes tool arguments/results, reasoning, host paths,
/// credentials, and provider-native cursors. A normal ACP turn ending is
/// labelled `ended_normally`, never `task_finished`: transport completion is
/// evidence about execution, not proof that the user's objective is done.
pub fn coding_session_first_turn_brief(package: &CodingSessionContextPackage) -> Value {
    let mut turns = BTreeMap::<(String, String), BriefTurn>::new();
    for (history_offset, item) in package.history.iter().enumerate() {
        let Some(turn_id) = item.turn_id.as_ref() else {
            continue;
        };
        let target_key = coding_session_target_key(&item.target);
        let key = (target_key, turn_id.clone());
        let turn = turns.entry(key).or_insert_with(|| {
            BriefTurn::new(item.target.clone(), turn_id.clone(), history_offset)
        });
        turn.observe(item);
    }
    let mut turns = turns.into_values().collect::<Vec<_>>();
    turns.sort_by(|left, right| {
        left.first_history_offset
            .cmp(&right.first_history_offset)
            .then_with(|| {
                coding_session_target_key(&left.target)
                    .cmp(&coding_session_target_key(&right.target))
            })
            .then_with(|| left.turn_id.cmp(&right.turn_id))
    });
    let mut recent = turns.into_iter().map(BriefTurn::finish).collect::<Vec<_>>();
    let indexed_turn_count = recent.len();
    let retain_from = recent.len().saturating_sub(MAX_CONTEXT_BRIEF_TURNS);
    recent.drain(..retain_from);

    let safe_name = package.session.name.as_deref().and_then(safe_brief_text);
    let safe_goal = package.session.goal.as_deref().and_then(safe_brief_text);
    let omitted_identity_text = package.session.name.is_some() && safe_name.is_none()
        || package.session.goal.is_some() && safe_goal.is_none();

    let safe_project_ref = package
        .session
        .project_ref
        .as_deref()
        .filter(|project_ref| {
            project_ref.len() <= MAX_IDENTIFIER_BYTES && !contains_host_path(project_ref)
        });
    let mut brief = serde_json::json!({
        "schema": "coding-session-first-turn-brief/v1",
        "session": {
            "sessionRef": package.session.session_ref,
            "genesisRef": package.session.genesis_ref,
            "name": safe_name,
            "goal": safe_goal,
            "projectRef": safe_project_ref,
        },
        "snapshot": {
            "projectedAt": package.provenance.generated_at,
            "completeAsOf": package.provenance.complete_as_of,
            "complete": package.provenance.complete,
            "truncated": package.provenance.truncated,
            "includedHistoryItems": package.provenance.included_history_items,
            "omittedHistoryItems": package.provenance.omitted_history_items,
            "totalHistoryItems": package.provenance.total_history_items,
            // The brief is also injected standalone as the ACP bootstrap
            // prompt, where no other rendering of the provenance travels with
            // it. Without this the brief would print the six breakdown terms
            // and a rule saying they reconcile `sourceEventCount` against a
            // number the agent cannot see.
            "sourceEventCount": package.provenance.source_event_count,
            "sourceEventBreakdown": package.provenance.source_event_breakdown,
            "indexedTurnCount": indexed_turn_count,
            "includedTurnCount": recent.len(),
            "omittedTurnCount": indexed_turn_count.saturating_sub(recent.len()),
        },
        "recentTurns": recent,
        "identityTextOmittedForSafety": omitted_identity_text,
        "rules": [
            "This is an evidence index, not a claim that prior statements are true",
            "ended_normally describes ACP transport only; semantic task completion remains unknown",
            "Later concurrent activity may exist after snapshot.completeAsOf",
            "sourceEventCount counts signed proof events, not content; sourceEventBreakdown reconciles it against totalHistoryItems",
            "Use session_history or search_session for cited evidence before relying on details"
        ]
    });
    while serde_json::to_vec(&brief)
        .map(|encoded| encoded.len() > MAX_CONTEXT_FIRST_TURN_BRIEF_BYTES)
        .unwrap_or(true)
    {
        let Some(turns) = brief.get_mut("recentTurns").and_then(Value::as_array_mut) else {
            break;
        };
        if turns.is_empty() {
            break;
        }
        turns.remove(0);
    }
    let included_turn_count = brief
        .get("recentTurns")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    brief["snapshot"]["includedTurnCount"] = Value::from(included_turn_count);
    brief["snapshot"]["omittedTurnCount"] =
        Value::from(indexed_turn_count.saturating_sub(included_turn_count));
    brief
}

/// Validate the encoded brief at the final ACP transport boundary.
///
/// This is a defense behind the typed projector: an oversized, substituted,
/// host-path-bearing, or credential-bearing descriptor is rejected instead of
/// being copied into a model's system prompt.
pub fn validate_coding_session_first_turn_brief_json(encoded: &str) -> Result<(), String> {
    if encoded.len() > MAX_CONTEXT_FIRST_TURN_BRIEF_BYTES {
        return Err(format!(
            "first-turn brief exceeds {MAX_CONTEXT_FIRST_TURN_BRIEF_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(encoded)
        .map_err(|error| format!("first-turn brief is not valid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "first-turn brief must be an object".to_owned())?;
    let expected = [
        "schema",
        "session",
        "snapshot",
        "recentTurns",
        "identityTextOmittedForSafety",
        "rules",
    ];
    if object.len() != expected.len()
        || !expected.iter().all(|key| object.contains_key(*key))
        || !object.keys().all(|key| expected.contains(&key.as_str()))
        || !object.get("session").is_some_and(Value::is_object)
        || !object.get("snapshot").is_some_and(Value::is_object)
        || !object.get("recentTurns").is_some_and(Value::is_array)
        || !object
            .get("identityTextOmittedForSafety")
            .is_some_and(Value::is_boolean)
        || !object.get("rules").is_some_and(Value::is_array)
    {
        return Err("first-turn brief has an invalid top-level shape".into());
    }
    if value.get("schema").and_then(Value::as_str) != Some("coding-session-first-turn-brief/v1") {
        return Err("first-turn brief has an unsupported schema".into());
    }
    if sanitize_coding_session_context_content(&value) != value {
        return Err("first-turn brief contains host-private or credential material".into());
    }
    Ok(())
}

struct BriefTurn {
    first_history_offset: usize,
    target: CodingSessionTarget,
    turn_id: String,
    operator_pubkey: Option<String>,
    request: Option<String>,
    request_omitted: bool,
    request_event_id: Option<String>,
    latest_assistant_event_id: Option<String>,
    latest_plan_event_id: Option<String>,
    terminal_event_id: Option<String>,
    outcome: &'static str,
    plan_counts: BTreeMap<String, u64>,
    tool_attempts: Vec<Value>,
    open_tools: HashMap<String, usize>,
    evidence_event_ids: Vec<String>,
}

impl BriefTurn {
    fn new(target: CodingSessionTarget, turn_id: String, first_history_offset: usize) -> Self {
        Self {
            first_history_offset,
            target,
            turn_id,
            operator_pubkey: None,
            request: None,
            request_omitted: false,
            request_event_id: None,
            latest_assistant_event_id: None,
            latest_plan_event_id: None,
            terminal_event_id: None,
            outcome: "in_progress_or_unclosed",
            plan_counts: BTreeMap::new(),
            tool_attempts: Vec::new(),
            open_tools: HashMap::new(),
            evidence_event_ids: Vec::new(),
        }
    }

    fn observe(&mut self, item: &CodingSessionContextHistoryItem) {
        if self.evidence_event_ids.len() < MAX_CONTEXT_BRIEF_EVIDENCE_IDS {
            self.evidence_event_ids.push(item.event_id.clone());
        }
        match item.item_kind.as_str() {
            "user_prompt" => {
                self.request_event_id = Some(item.event_id.clone());
                self.operator_pubkey = item
                    .content
                    .get("operatorPubkey")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let content = item.content.get("content").and_then(Value::as_str);
                self.request = content.and_then(safe_brief_text);
                self.request_omitted = content.is_some() && self.request.is_none();
            }
            "assistant_text" => {
                self.latest_assistant_event_id = Some(item.event_id.clone());
            }
            "tool_call" if self.tool_attempts.len() < MAX_CONTEXT_BRIEF_TOOL_ATTEMPTS => {
                let tool = item.content.get("tool").and_then(Value::as_object);
                let tool_id = tool
                    .and_then(|tool| tool.get("toolId"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let tool_name = tool
                    .and_then(|tool| tool.get("toolName"))
                    .and_then(Value::as_str)
                    .filter(|name| safe_brief_identifier(name))
                    .unwrap_or("redacted_tool");
                let index = self.tool_attempts.len();
                self.tool_attempts.push(serde_json::json!({
                    "tool": tool_name,
                    "outcome": "result_not_observed",
                    "callEventId": item.event_id,
                    "resultEventId": null,
                }));
                if !tool_id.is_empty() {
                    self.open_tools.insert(tool_id.to_owned(), index);
                }
            }
            "tool_result" => {
                let tool_id = item
                    .content
                    .get("toolId")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if let Some(index) = self.open_tools.remove(tool_id) {
                    if let Some(attempt) = self
                        .tool_attempts
                        .get_mut(index)
                        .and_then(Value::as_object_mut)
                    {
                        let outcome = if item
                            .content
                            .get("isError")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            "failed"
                        } else {
                            "succeeded"
                        };
                        attempt.insert("outcome".into(), Value::String(outcome.into()));
                        attempt
                            .insert("resultEventId".into(), Value::String(item.event_id.clone()));
                    }
                }
            }
            "plan" => {
                self.latest_plan_event_id = Some(item.event_id.clone());
                self.plan_counts.clear();
                if let Some(entries) = item.content.get("entries").and_then(Value::as_array) {
                    for entry in entries {
                        let status = entry
                            .get("status")
                            .and_then(Value::as_str)
                            .filter(|status| safe_brief_identifier(status))
                            .unwrap_or("unknown");
                        *self.plan_counts.entry(status.to_owned()).or_default() += 1;
                    }
                }
            }
            "result" => {
                self.terminal_event_id = Some(item.event_id.clone());
                self.outcome = match item.content.get("subtype").and_then(Value::as_str) {
                    Some("success") => "ended_normally",
                    Some("cancelled") => "cancelled",
                    Some("error") => classify_result_error(&item.content),
                    _ => "unknown_terminal_result",
                };
            }
            _ => {}
        }
    }

    fn finish(self) -> Value {
        serde_json::json!({
            "target": self.target,
            "turnId": self.turn_id,
            "operatorPubkey": self.operator_pubkey,
            "request": self.request,
            "requestOmittedForSafety": self.request_omitted,
            "requestEventId": self.request_event_id,
            "latestAssistantEventId": self.latest_assistant_event_id,
            "latestPlanEventId": self.latest_plan_event_id,
            "terminalEventId": self.terminal_event_id,
            "outcome": self.outcome,
            "latestPlanCounts": self.plan_counts,
            "toolAttempts": self.tool_attempts,
            "evidenceEventIds": self.evidence_event_ids,
        })
    }
}

fn classify_result_error(content: &Value) -> &'static str {
    let result = content
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if result.contains("refused") {
        "refused"
    } else if result.contains("token limit") {
        "token_limit"
    } else if result.contains("request limit") {
        "request_limit"
    } else {
        "failed"
    }
}

fn safe_brief_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CONTEXT_ITEM_KIND_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn safe_brief_text(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty()
        || text.len() > MAX_CONTEXT_BRIEF_TEXT_BYTES
        || contains_credential_material(text)
    {
        return None;
    }
    let sanitized = sanitize_coding_session_context_text(text);
    (sanitized.len() <= MAX_CONTEXT_BRIEF_TEXT_BYTES).then_some(sanitized)
}

fn contains_credential_material(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "password",
        "api key",
        "api_key",
        "secret",
        "credential",
        "authorization",
        "bearer ",
        "private key",
        "private_key",
        "resume cursor",
        "resume_cursor",
        "access token",
        "api token",
        "auth_token",
        "token=",
        "token:",
        "nsec1",
        "sk-",
        "ghp_",
        "github_pat_",
        "xoxb-",
        "akia",
        "-----begin",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

impl CodingSessionContextPackage {
    /// Validate the complete private package before storing or serving it.
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION..=CODING_SESSION_CONTEXT_PACKAGE_VERSION)
            .contains(&self.v)
        {
            return Err(format!(
                "unsupported coding-session context package version {} (supported {}..={})",
                self.v,
                MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION,
                CODING_SESSION_CONTEXT_PACKAGE_VERSION
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
        for (field, value) in [
            ("context name", self.name.as_deref()),
            ("context goal", self.goal.as_deref()),
        ] {
            if value.is_some_and(|value| sanitize_coding_session_context_text(value) != value) {
                return Err(format!(
                    "{field} contains host-private or credential material"
                ));
            }
        }
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
        if self.complete_as_of.is_some_and(|value| value < 0) {
            return Err("context provenance completeAsOf must be non-negative".into());
        }
        if !self.complete && self.complete_as_of.is_some() {
            return Err("incomplete context provenance must not claim completeAsOf".into());
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
        if let Some(breakdown) = &self.source_event_breakdown {
            let total = breakdown
                .checked_total()
                .ok_or_else(|| "context provenance sourceEventBreakdown overflows".to_owned())?;
            if total != self.source_event_count {
                return Err(format!(
                    "context provenance sourceEventBreakdown sums to {total}, not sourceEventCount {}",
                    self.source_event_count
                ));
            }
            let accounted = self
                .included_history_items
                .checked_add(self.omitted_history_items)
                .ok_or_else(|| "context provenance history counts overflow".to_owned())?;
            if breakdown.transcript_events < accounted {
                return Err(format!(
                    "context provenance transcriptEvents {} is fewer than included + omitted {accounted}",
                    breakdown.transcript_events
                ));
            }
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
            if sanitize_coding_session_context_text(note) != *note {
                return Err(
                    "context provenance notes contain host-private or credential material".into(),
                );
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
        if sanitize_coding_session_context_content(&self.content) != self.content {
            return Err(
                "context history content contains host-private or credential material".into(),
            );
        }
        Ok(())
    }
}

/// Redact host paths, credentials, and provider-native cursor fields from one
/// structured transcript item before it enters a private handoff package.
///
/// Redaction is fail-closed and content-addressed: the unsafe value is replaced
/// in full by a marker carrying its serialized byte count and SHA-256 digest.
/// Repo-relative paths are left intact. The signed source event remains named
/// by the surrounding history item, so an authorized reader can distinguish a
/// deliberate handoff elision from absent source data.
pub fn sanitize_coding_session_context_content(value: &Value) -> Value {
    sanitize_content(value, None, &mut RedactionLog::off())
}

/// Redact a transcript item after making paths inside `workspace_root`
/// repository-relative.
///
/// The root itself is never retained. `/private/checkout/desktop/src/App.tsx`
/// becomes `desktop/src/App.tsx`, while `/private/other/secret` still reaches
/// the ordinary host-path guard and is elided. This keeps code citations useful
/// without publishing the machine-specific checkout location.
pub fn sanitize_coding_session_context_content_for_workspace(
    value: &Value,
    workspace_root: &Path,
) -> Value {
    sanitize_content(value, Some(workspace_root), &mut RedactionLog::off())
}

/// Redact one item **and report what was redacted**.
///
/// Same redaction, byte for byte — this is the recording twin of
/// [`sanitize_coding_session_context_content`], not a second implementation, so
/// the published transcript and the host's own record cannot drift apart on
/// what counts as private.
///
/// Only *recoverable* classes appear in the returned log (see
/// [`RedactionClass::is_recoverable`]). A secret is redacted exactly as before
/// and is never reported, because a caller that persists this log must not be
/// able to persist a credential by accident: the guarantee is structural, not a
/// filtering step every caller has to remember.
pub fn sanitize_coding_session_context_content_recording(value: &Value) -> (Value, Vec<Redaction>) {
    sanitize_content_recording(value, None)
}

/// [`sanitize_coding_session_context_content_recording`] with workspace paths
/// made repository-relative first, exactly as
/// [`sanitize_coding_session_context_content_for_workspace`] does.
pub fn sanitize_coding_session_context_content_recording_for_workspace(
    value: &Value,
    workspace_root: &Path,
) -> (Value, Vec<Redaction>) {
    sanitize_content_recording(value, Some(workspace_root))
}

fn sanitize_content_recording(
    value: &Value,
    workspace_root: Option<&Path>,
) -> (Value, Vec<Redaction>) {
    let mut log = RedactionLog::recording();
    let sanitized = sanitize_content(value, workspace_root, &mut log);
    (sanitized, log.into_entries())
}

fn sanitize_content(value: &Value, workspace_root: Option<&Path>, log: &mut RedactionLog) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, nested)| {
                    let value = if let Some(class) = sensitive_context_key(key) {
                        if nested.as_str().is_some_and(is_context_elision_marker) {
                            nested.clone()
                        } else {
                            Value::String(elide(nested, class, log))
                        }
                    } else {
                        sanitize_content(nested, workspace_root, log)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|nested| sanitize_content(nested, workspace_root, log))
                .collect(),
        ),
        Value::String(text) => Value::String(match workspace_root {
            Some(root) => sanitize_text(&relativize_workspace_paths(text, root), log),
            None => sanitize_text(text, log),
        }),
        _ => value.clone(),
    }
}

/// Make workspace-contained paths relative before applying the ordinary
/// fail-closed text sanitizer.
pub fn sanitize_coding_session_context_text_for_workspace(
    value: &str,
    workspace_root: &Path,
) -> String {
    sanitize_coding_session_context_text(&relativize_workspace_paths(value, workspace_root))
}

fn relativize_workspace_paths(value: &str, workspace_root: &Path) -> String {
    let Some(root) = workspace_root.to_str() else {
        return value.to_owned();
    };
    let root = root.trim_end_matches(['/', '\\']);
    if root.is_empty() || root == "/" {
        return value.to_owned();
    }

    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find(root) {
        output.push_str(&rest[..index]);
        let after = &rest[index + root.len()..];
        let boundary_before = index == 0
            || rest[..index]
                .chars()
                .next_back()
                .is_some_and(|character| !character.is_ascii_alphanumeric());
        let boundary_after = after.is_empty() || after.starts_with(['/', '\\']);
        if boundary_before && boundary_after {
            if let Some(relative) = after.strip_prefix('/').or_else(|| after.strip_prefix('\\')) {
                rest = relative;
            } else {
                output.push('.');
                rest = after;
            }
        } else {
            output.push_str(root);
            rest = after;
        }
    }
    output.push_str(rest);
    output
}

/// Redact an unsafe host-private or credential-bearing prose field while
/// retaining safe text byte-for-byte.
///
/// **Secrets are redacted; sentences are not.** This used to replace the
/// *entire* text with an elision marker whenever it contained any of a list of
/// words — "secret", "credential", "authorization", "private key", "token:".
/// Those are the ordinary vocabulary of the work: this repo ships a binary
/// called `git-credential-nostr`, and an agent asked about a git ACL branch
/// answers in exactly those words. The result was that a coding session
/// working on authorization could not show its work at all — an operator asked
/// "what was the result?" and received a 1,934-byte hash (observed
/// 2026-08-24).
///
/// So a hit now has to be a *value*, not a topic:
///
/// 1. a key block (`-----BEGIN … -----END …`), redacted whole, because the
///    body carries no other information anyway;
/// 2. a token with a recognisable shape — `nsec1…`, `sk-…`, `ghp_…`,
///    `github_pat_…`, `xoxb-…`, `AKIA…` — redacted where it stands;
/// 3. the value side of a credential assignment (`token=…`, `password: …`,
///    `api key is …`), redacted while the sentence survives.
///
/// A bare mention of a credential word, with no value beside it, is prose and
/// is left alone. Every shape the old rule caught is still caught; what
/// changed is that catching one no longer costs the whole message.
pub fn sanitize_coding_session_context_text(value: &str) -> String {
    sanitize_text(value, &mut RedactionLog::off())
}

fn sanitize_text(value: &str, log: &mut RedactionLog) -> String {
    let without_blocks = redact_key_blocks(value, log);
    let mut sanitized = String::with_capacity(without_blocks.len());
    for segment in without_blocks.split_inclusive(char::is_whitespace) {
        let word = segment.trim_end_matches(char::is_whitespace);
        let trailing = &segment[word.len()..];
        if is_context_elision_marker(word) || is_credential_mask(word) {
            // Already redacted upstream: never redact a redaction.
            sanitized.push_str(word);
        } else if contains_shaped_secret(word) {
            // A credential is masked, not hashed: see `mask_credential_value`.
            sanitized.push_str(&mask_credential_value(word));
        } else if contains_host_path(word) {
            sanitized.push_str(&elide_str(word, RedactionClass::HostPath, log));
        } else {
            sanitized.push_str(word);
        }
        sanitized.push_str(trailing);
    }
    redact_credential_assignments(&sanitized)
}

/// PEM-style blocks, redacted whole.
fn redact_key_blocks(value: &str, log: &mut RedactionLog) -> String {
    const BEGIN: &str = "-----BEGIN";
    const END_MARK: &str = "-----END";
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    loop {
        let Some(start) = rest.find(BEGIN) else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        // An unterminated block is still a key: redact to the end rather than
        // emitting the half that was written.
        let block_end = match tail.find(END_MARK) {
            Some(at) => tail[at..]
                .find("-----\n")
                .map(|nl| at + nl + "-----\n".len())
                .or_else(|| tail[at..].rfind("-----").map(|last| at + last + 5))
                .unwrap_or(tail.len()),
            None => tail.len(),
        };
        let block = &tail[..block_end];
        out.push_str(&elide_str(block, RedactionClass::KeyBlock, log));
        rest = &tail[block_end..];
    }
}

/// Prefixes that identify a credential by shape rather than by topic.
const SHAPED_SECRET_PREFIXES: &[&str] = &[
    "nsec1",
    "sk-",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "xapp-",
    "akia",
    "asia",
];

/// Does this word carry a recognisable secret shape?
fn contains_shaped_secret(word: &str) -> bool {
    let token = word.trim_matches(|character: char| {
        matches!(
            character,
            '\'' | '"' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    // The value side of `KEY=value` carries the shape, not the whole pair.
    let candidate = token.rsplit_once('=').map_or(token, |(_, value)| value);
    let lowered = candidate.to_ascii_lowercase();
    SHAPED_SECRET_PREFIXES
        .iter()
        .any(|prefix| lowered.starts_with(prefix) && candidate.len() > prefix.len() + 8)
}

/// Words that name a credential when a value follows them.
const CREDENTIAL_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api key",
    "api_key",
    "apikey",
    "api token",
    "access token",
    "auth token",
    "auth_token",
    "authtoken",
    "token",
    "credential",
    "credentials",
    "authorization",
    "bearer",
    "private key",
    "private_key",
    "privatekey",
    "signing key",
    "resume cursor",
    "resume_cursor",
];

/// Redact the value side of `<credential word><separator><value>`.
///
/// Line-scoped, because a value ends at the end of its line — redacting to the
/// end of a paragraph would take the explanation with it, which is the failure
/// this whole function exists to undo.
fn redact_credential_assignments(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for line in value.split_inclusive('\n') {
        out.push_str(&redact_credential_assignment_line(line));
    }
    out
}

/// Does this token look like a value rather than the next word of a sentence?
///
/// The gate exists for the space-separated form (`password hunter2`), where
/// the "separator" carries no signal at all. A digit, mixed case, or real
/// length is what distinguishes a secret from the next word of prose — so
/// "the private key never leaves the keychain" keeps its sentence while
/// "password hunter2" does not keep its password.
fn looks_like_credential_value(token: &str) -> bool {
    let trimmed = token.trim_matches(|character: char| {
        matches!(
            character,
            '\'' | '"' | '`' | ',' | ';' | '.' | ')' | ']' | '}'
        )
    });
    if trimmed.len() < 6 {
        return false;
    }
    trimmed.chars().any(|character| character.is_ascii_digit())
        || trimmed.len() >= 16
        || trimmed.contains('_')
        || (trimmed.chars().any(char::is_uppercase)
            && trimmed.chars().any(char::is_lowercase)
            && trimmed.chars().filter(|c| c.is_uppercase()).count() > 1)
}

/// Where a credential value starts on this line, and how it ends.
enum CredentialValue {
    /// `key=value` / `key: value` — everything after the separator is value.
    RestOfLine { at: usize },
    /// `key is value` / `key value` — only the next token is value.
    NextToken { at: usize },
}

fn find_credential_value(line: &str) -> Option<CredentialValue> {
    let lowered = line.to_ascii_lowercase();
    CREDENTIAL_KEYS
        .iter()
        .filter_map(|key| {
            let at = lowered.find(key)?;
            let after = at + key.len();
            let tail = &lowered[after..];
            for separator in ["=", ":"] {
                if let Some(rest) = tail.strip_prefix(separator) {
                    let padding = rest.len() - rest.trim_start().len();
                    return Some(CredentialValue::RestOfLine {
                        at: after + separator.len() + padding,
                    });
                }
            }
            // `is`/`was`/a bare space: only the next token can be the value,
            // and only if it looks like one — and never a parenthetical, which
            // is how prose qualifies a noun rather than how anyone writes a
            // secret. "private key (secp256k1)" lost its curve name to the
            // digit test on 2026-08-24; asides like that are the common case
            // after exactly these words.
            for separator in [" is ", " was ", " "] {
                let Some(rest) = tail.strip_prefix(separator) else {
                    continue;
                };
                let padding = rest.len() - rest.trim_start().len();
                let start = after + separator.len() + padding;
                let token = line[start..].split_whitespace().next().unwrap_or_default();
                let parenthetical = token.starts_with(['(', '[', '{']);
                if !parenthetical && looks_like_credential_value(token) {
                    return Some(CredentialValue::NextToken { at: start });
                }
                break;
            }
            None
        })
        .min_by_key(|found| match found {
            CredentialValue::RestOfLine { at } | CredentialValue::NextToken { at } => *at,
        })
}

fn redact_credential_assignment_line(line: &str) -> String {
    let Some(found) = find_credential_value(line) else {
        return line.to_owned();
    };
    let (start, payload_len) = match found {
        CredentialValue::RestOfLine { at } => {
            let payload = line[at..].trim_end();
            (at, payload.len())
        }
        CredentialValue::NextToken { at } => {
            let token = line[at..].split_whitespace().next().unwrap_or_default();
            (at, token.len())
        }
    };
    let payload = &line[start..start + payload_len];
    if payload.is_empty() || is_context_elision_marker(payload) || is_credential_mask(payload) {
        return line.to_owned();
    }
    format!(
        "{}{}{}",
        &line[..start],
        mask_credential_value(payload),
        &line[start + payload_len..]
    )
}

/// Absolute paths that describe every POSIX host identically.
///
/// The host-path rule protects *this* machine's layout — home directories,
/// project roots, mount points. A stock system interpreter is none of those:
/// `/bin/zsh` is the same string on every macOS install and discloses nothing.
///
/// It has to be exempt because of how `codex-acp` names tool calls. It leaves
/// `tool.input` empty and puts the whole command in `tool.toolName` as prose
/// (§2 item 2), so a Codex shell row is the argv itself — and the redactor,
/// working word by word, elided argv[0] and produced `Ran [elided private
/// context: 10 bytes, sha256:…] -lc "sed -n …"` (§2 item 43). Ten bytes of
/// public knowledge, in exchange for a row nobody can read.
///
/// Deliberately exact matches only, and deliberately short: a path under
/// `/usr/local`, `/opt`, or anywhere a person installs things is host layout
/// again and stays redacted.
const SYSTEM_COMMAND_PATHS: &[&str] = &[
    "/bin/sh",
    "/bin/bash",
    "/bin/zsh",
    "/bin/dash",
    "/bin/ksh",
    "/bin/csh",
    "/bin/tcsh",
    "/usr/bin/sh",
    "/usr/bin/bash",
    "/usr/bin/zsh",
    "/usr/bin/dash",
    "/usr/bin/env",
];

fn contains_host_path(word: &str) -> bool {
    let token = word.trim_matches(|character: char| {
        matches!(
            character,
            '\'' | '"' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    let candidate = token.rsplit_once('=').map_or(token, |(_, value)| value);
    if SYSTEM_COMMAND_PATHS.contains(&candidate) {
        return false;
    }
    let lowered = candidate.to_ascii_lowercase();
    // A leading `/`, or one that starts a *quoted* absolute path (`="/etc/x"`,
    // `(/var/log)`). The delimiter alone is not enough: an agent explaining a
    // command writes `` `get`/`store`/`erase` ``, where the slash sits between
    // two code spans and names no path at all. That elided a clause out of the
    // middle of a live answer on 2026-08-24, so the character *after* the
    // slash has to look like a path segment too.
    let unix_absolute = candidate.char_indices().any(|(index, character)| {
        if character != '/' {
            return false;
        }
        let starts_segment = candidate[index + 1..].chars().next().is_some_and(|next| {
            next.is_ascii_alphanumeric() || matches!(next, '.' | '_' | '-' | '~')
        });
        if !starts_segment {
            return false;
        }
        index == 0
            || candidate[..index]
                .chars()
                .next_back()
                .is_some_and(|previous| {
                    matches!(
                        previous,
                        '=' | '(' | '[' | '{' | ',' | ';' | '\'' | '"' | '`'
                    )
                })
    });
    unix_absolute
        || candidate.starts_with("~/")
        || candidate.starts_with("\\\\")
        || lowered.contains("file://")
        || (!lowered.starts_with("http://")
            && !lowered.starts_with("https://")
            && (candidate.contains(":/") || candidate.contains(":\\")))
        || (candidate.len() >= 3
            && candidate.as_bytes()[0].is_ascii_alphabetic()
            && candidate.as_bytes()[1] == b':'
            && matches!(candidate.as_bytes()[2], b'\\' | b'/'))
}

/// Which rule, if any, claims a value by the name of the key holding it — and
/// therefore whether that value could ever be shown back to its own operator.
///
/// `resumecursor` and `acpsessionid` sit apart from the rest deliberately: they
/// are opaque provider bookkeeping, private to the host but not *secret*, and
/// they are exactly what an operator needs when a session stalls. Everything
/// else on this list names a credential.
fn sensitive_context_key(key: &str) -> Option<RedactionClass> {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match normalized.as_str() {
        "resumecursor" | "acpsessionid" => Some(RedactionClass::Structural),
        "privatekey" | "secretkey" | "signingkey" | "nostrprivatekey" | "buzzprivatekey"
        | "buzzauthtag" | "relayauthtoken" | "password" | "passwd" | "authorization" | "cookie"
        | "credential" | "credentials" | "apikey" | "accesstoken" | "authtoken" | "token" => {
            Some(RedactionClass::SecretKey)
        }
        _ => None,
    }
}

/// Fixed-width mask, so the redaction never states a length.
///
/// A byte count is a hint about the secret. For a machine-generated token it
/// is a harmless one; for `password: hunter2` it says the password is seven
/// characters, which is a real gift. The mask is therefore always the same
/// width regardless of what it replaced.
const CREDENTIAL_MASK: &str = "••••••••";

/// Characters of a machine-shaped token shown after the mask.
///
/// Four, the convention every card receipt and cloud console uses, and only
/// for a token whose *shape* proves it is machine-generated: `ghp_` plus 36
/// base62 characters leaves 62^32 possibilities after revealing four, which is
/// not a search anyone runs. A human password has no such floor — its search
/// space is a dictionary — so nothing is revealed for a value whose shape says
/// nothing (see [`mask_credential_value`]).
const CREDENTIAL_TAIL_CHARS: usize = 4;
/// Shortest token whose tail may be shown; below this the tail is too much of it.
const CREDENTIAL_TAIL_MIN_LEN: usize = 24;

/// Replace a secret with a mask that says what it was, not what it is.
///
/// The old marker was `[elided private context: N bytes, sha256:…]` for every
/// redaction. Two problems, both worse for credentials than for paths: it
/// published a **crackable digest** — sha256 of a human password is a
/// dictionary attack, not a secret — and it stated the length. It also drowned
/// the sentence it sat in, which is how a mid-sentence redaction became hard
/// to even notice (2026-08-24).
///
/// A known prefix is kept because it is a public format tag, not secret
/// material: `ghp_`, `nsec1`, `AKIA` say *what kind of credential leaked*,
/// which is exactly what a person reading the transcript needs in order to go
/// rotate the right thing.
fn mask_credential_value(value: &str) -> String {
    // `contains_shaped_secret` looks past a `KEY=` to find the shape, so the
    // mask has to as well — otherwise `token=ghp_…` masks the variable name
    // along with its value and the reader loses which one leaked.
    if let Some((key, secret)) = value.split_once('=') {
        if !key.is_empty() && !secret.is_empty() {
            return format!("{key}={}", mask_credential_value(secret));
        }
    }
    let trimmed = value.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let prefix = SHAPED_SECRET_PREFIXES
        .iter()
        .find(|prefix| lowered.starts_with(*prefix))
        .map(|prefix| &trimmed[..prefix.len()]);
    match prefix {
        Some(prefix) if trimmed.len() >= CREDENTIAL_TAIL_MIN_LEN => {
            let tail_start = trimmed.len() - CREDENTIAL_TAIL_CHARS;
            format!("{prefix}{CREDENTIAL_MASK}{}", &trimmed[tail_start..])
        }
        Some(prefix) => format!("{prefix}{CREDENTIAL_MASK}"),
        // Shape says nothing, so nothing is said: this may be a password.
        None => CREDENTIAL_MASK.to_string(),
    }
}

/// Is this text already a credential mask? Masks are never re-masked.
fn is_credential_mask(value: &str) -> bool {
    value.contains(CREDENTIAL_MASK)
}

/// Why a value was redacted — and, structurally, whether it may be recorded.
///
/// The class is decided **by the rule that caught the value**, not by
/// re-inspecting the plaintext afterwards. A later reader would have to guess;
/// the redactor knows for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RedactionClass {
    /// An absolute path, `~/…`, or a Windows/UNC path — this machine's layout.
    HostPath,
    /// Opaque provider bookkeeping named by its key (`resumeCursor`,
    /// `acpSessionId`). Private to the host; not a credential.
    Structural,
    /// A value held under a key that names a credential.
    SecretKey,
    /// A PEM-style `-----BEGIN … -----END …` block.
    KeyBlock,
    /// A token with a recognisable secret shape (`nsec1…`, `ghp_…`, `AKIA…`).
    ShapedSecret,
    /// The value side of `token=…` / `password: …` / `api key is …`.
    CredentialAssignment,
}

impl RedactionClass {
    /// May this class's plaintext be written to the host's own vault?
    ///
    /// **Only host layout and opaque bookkeeping.** Showing an operator their
    /// own home directory discloses nothing they do not already know; writing a
    /// redacted credential to disk in plaintext would create a liability
    /// strictly worse than the readability problem the vault exists to solve.
    ///
    /// This is the single gate. It is enforced where redactions are produced
    /// rather than where they are persisted, so no caller can opt out of it by
    /// forgetting to filter.
    pub fn is_recoverable(self) -> bool {
        matches!(self, Self::HostPath | Self::Structural)
    }
}

/// One recoverable redaction, as the host may record it for its own operator.
///
/// Never published, never placed in an agent environment. The `digest` is the
/// same one the published marker carries, which is what lets a reader on this
/// machine join the two without any new wire field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Redaction {
    /// SHA-256 of the value's JSON encoding, lowercase hex, no `sha256:` prefix.
    pub digest: String,
    /// Serialized JSON byte count — the number the marker reports.
    pub bytes: usize,
    /// The rule that caught it. Always a recoverable class.
    pub class: RedactionClass,
    /// The value as it was before redaction. A string value is recorded as
    /// itself; anything else as its compact JSON encoding.
    pub plaintext: String,
}

/// Collects recoverable redactions, or discards everything.
///
/// `off()` is the validating path — [`sanitize_coding_session_context_content`]
/// is called as a sanitize-and-compare check in half a dozen places, and it
/// must not materialize a plaintext copy of anything it redacts just to drop
/// it. Nothing is cloned unless someone asked to record.
struct RedactionLog {
    entries: Option<Vec<Redaction>>,
}

impl RedactionLog {
    fn off() -> Self {
        Self { entries: None }
    }

    fn recording() -> Self {
        Self {
            entries: Some(Vec::new()),
        }
    }

    fn into_entries(self) -> Vec<Redaction> {
        self.entries.unwrap_or_default()
    }

    fn record(&mut self, class: RedactionClass, digest: &str, bytes: usize, plaintext: &Value) {
        let Some(entries) = self.entries.as_mut() else {
            return;
        };
        if !class.is_recoverable() {
            return;
        }
        entries.push(Redaction {
            digest: digest.to_owned(),
            bytes,
            class,
            plaintext: match plaintext {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            },
        });
    }
}

/// Redact one value, recording it when its class allows.
fn elide(value: &Value, class: RedactionClass, log: &mut RedactionLog) -> String {
    let (marker, digest, bytes) = context_elision_marker_parts(value);
    log.record(class, &digest, bytes, value);
    marker
}

/// [`elide`] for a value that is known to be a string.
fn elide_str(text: &str, class: RedactionClass, log: &mut RedactionLog) -> String {
    elide(&Value::String(text.to_owned()), class, log)
}

fn context_elision_marker_parts(value: &Value) -> (String, String, usize) {
    let encoded = serde_json::to_vec(value).unwrap_or_default();
    let digest = hex::encode(Sha256::digest(&encoded));
    (
        format!(
            "[elided private context: {} bytes, sha256:{}]",
            encoded.len(),
            digest
        ),
        digest,
        encoded.len(),
    )
}

fn is_context_elision_marker(value: &str) -> bool {
    value.starts_with("[elided private context: ")
        && value.ends_with(']')
        && value.contains(" bytes, sha256:")
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
    if sanitize_coding_session_context_text(d_tag) != d_tag {
        return Err("context projectRef must not contain host-private material".into());
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
#[path = "coding_session_context_tests.rs"]
mod tests;
