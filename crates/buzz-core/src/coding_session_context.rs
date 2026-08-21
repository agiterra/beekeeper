//! Private, provider-neutral context package for rehydrating a coding session.
//!
//! This is not a Nostr event contract. A provider projects already-published,
//! signature-verified relay facts into this bounded serializable package and
//! stores or serves it privately on the destination machine. The package never
//! contains a Nostr signing key, host path, ACP cursor, or fabricated native
//! Claude/Codex session record.

use std::collections::{BTreeMap, HashMap, HashSet};

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
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, nested)| {
                    let value = if sensitive_context_key(key) {
                        if nested.as_str().is_some_and(is_context_elision_marker) {
                            nested.clone()
                        } else {
                            Value::String(context_elision_marker(nested))
                        }
                    } else {
                        sanitize_coding_session_context_content(nested)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(sanitize_coding_session_context_content)
                .collect(),
        ),
        Value::String(text) => Value::String(sanitize_coding_session_context_text(text)),
        _ => value.clone(),
    }
}

/// Redact an unsafe host-private or credential-bearing prose field while
/// retaining safe text byte-for-byte.
pub fn sanitize_coding_session_context_text(value: &str) -> String {
    if contains_credential_material(value) {
        return context_elision_marker(&Value::String(value.to_owned()));
    }
    let mut sanitized = String::with_capacity(value.len());
    for segment in value.split_inclusive(char::is_whitespace) {
        let word = segment.trim_end_matches(char::is_whitespace);
        let trailing = &segment[word.len()..];
        if contains_host_path(word) {
            sanitized.push_str(&context_elision_marker(&Value::String(word.to_owned())));
        } else {
            sanitized.push_str(word);
        }
        sanitized.push_str(trailing);
    }
    sanitized
}

fn contains_host_path(word: &str) -> bool {
    let token = word.trim_matches(|character: char| {
        matches!(
            character,
            '\'' | '"' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    let candidate = token.rsplit_once('=').map_or(token, |(_, value)| value);
    let lowered = candidate.to_ascii_lowercase();
    let unix_absolute = candidate.char_indices().any(|(index, character)| {
        character == '/'
            && (index == 0
                || candidate[..index]
                    .chars()
                    .next_back()
                    .is_some_and(|previous| {
                        matches!(
                            previous,
                            '=' | '(' | '[' | '{' | ',' | ';' | '\'' | '"' | '`'
                        )
                    }))
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

fn sensitive_context_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "privatekey"
            | "secretkey"
            | "signingkey"
            | "nostrprivatekey"
            | "buzzprivatekey"
            | "buzzauthtag"
            | "relayauthtoken"
            | "password"
            | "passwd"
            | "authorization"
            | "cookie"
            | "credential"
            | "credentials"
            | "apikey"
            | "accesstoken"
            | "authtoken"
            | "token"
            | "resumecursor"
            | "acpsessionid"
    )
}

fn context_elision_marker(value: &Value) -> String {
    let encoded = serde_json::to_vec(value).unwrap_or_default();
    format!(
        "[elided private context: {} bytes, sha256:{}]",
        encoded.len(),
        hex::encode(Sha256::digest(&encoded))
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
