//! ACP `session/update` → projected transcript item translation.
//!
//! The translator is a pure state machine: notifications in, items out, no I/O
//! and no clock. That is what lets the item sequence for a scripted stream be
//! asserted exactly, which matters because these items are the durable record
//! the whole feature exists to produce — a mistranslation is not a rendering bug,
//! it is a corrupted archive.
//!
//! # Coalescing
//!
//! Agents stream prose one token at a time. Publishing an event per token would
//! be unreadable, unaffordable, and would bury the tool calls that carry the
//! actual work. So text accumulates and flushes at three boundaries:
//!
//! - **size** — [`COALESCE_FLUSH_BYTES`], well under the 32 KiB event cap so a
//!   flushed block never needs truncating;
//! - **narrative** — any tool call or tool result, because prose written *before*
//!   the agent reached for a tool belongs before it in the record;
//! - **turn end** — nothing is left buffered when the turn's `result` lands.
//!
//! # Redaction
//!
//! NIP-CST requires the published `item` to be *deeply redacted*: no host
//! paths, no credentials, no raw provider objects. That is not something the
//! per-kind constructors can be trusted to remember — ACP's `title` is prose
//! the agent wrote and `rawInput` is the literal arguments, so host-private
//! strings arrive through ordinary fields. [`fit_item`] therefore redacts every
//! item on its way to the envelope, using the same redactor as the private
//! rehydration package.
//!
//! # Truncation
//!
//! Two independent caps. Tool inputs and outputs are bounded on the way in
//! ([`MAX_TOOL_INPUT_BYTES`] / [`MAX_TOOL_CONTENT_BYTES`]) so one enormous file
//! read cannot dominate an item. The envelope is then bounded on the way out by
//! [`fit_item`], which shrinks the largest string it can find and, if even that
//! is not enough, replaces the item with an `elided` marker. Every elision
//! carries a byte count and a SHA-256 of what was dropped, so a reader can tell
//! "the provider had this and chose not to publish it" apart from "nothing was
//! there".

use std::collections::HashMap;
use std::path::Path;

use buzz_core::coding_session_context::{
    sanitize_coding_session_context_content, sanitize_coding_session_context_content_for_workspace,
    sanitize_coding_session_context_content_recording,
    sanitize_coding_session_context_content_recording_for_workspace, Redaction,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// Flush the text buffer once it reaches this size.
pub const COALESCE_FLUSH_BYTES: usize = 24 * 1024;
/// Cap on a serialized tool input before it is replaced by a digest.
pub const MAX_TOOL_INPUT_BYTES: usize = 8 * 1024;
/// Cap on a tool result's textual content.
pub const MAX_TOOL_CONTENT_BYTES: usize = 8 * 1024;
/// Shortest string worth truncating; below this, shrinking buys nothing.
const MIN_TRUNCATABLE_BYTES: usize = 64;

/// `sessionUpdate` discriminators that carry context-window or usage numbers.
///
/// Adapters disagree on the name and the ACP schema does not pin one, so all
/// four are accepted. Which one `claude-agent-acp` actually emits is unverified
/// — see the crate docs.
const USAGE_UPDATE_VARIANTS: [&str; 4] = [
    "usage_update",
    "context_window_update",
    "context_window_updated",
    "token_usage",
];

/// Turns a session's ACP notification stream into projected transcript items.
#[derive(Debug)]
pub struct TranscriptTranslator {
    include_thoughts: bool,
    text: String,
    thoughts: String,
    usage: Option<Value>,
    tool_calls: u64,
    tool_names: HashMap<String, String>,
    tool_kinds: HashMap<String, String>,
}

impl TranscriptTranslator {
    /// A translator for one session generation.
    pub fn new(include_thoughts: bool) -> Self {
        Self {
            include_thoughts,
            text: String::new(),
            thoughts: String::new(),
            usage: None,
            tool_calls: 0,
            tool_names: HashMap::new(),
            tool_kinds: HashMap::new(),
        }
    }

    /// Open a turn with the operator's prompt.
    ///
    /// `operator_pubkey` is the verified signer of the command that requested
    /// the turn, stamped onto the `user_prompt` item so the durable record
    /// names who drove it. `None` leaves the item unattributed rather than
    /// guessing a founder.
    ///
    /// `command_id` is that same command's `commandId`, stamped so a consumer
    /// joins this echo to the turn's receipts by id. Without it the only join
    /// available is the prompt text, which cannot tell two identical prompts
    /// apart. `None` only when the caller had no command to name.
    ///
    /// `sender_role` is the crew role the signer held on its own seat when the
    /// turn was delivered, or `None` for the founder and for any operator with
    /// no seat in this umbrella.
    ///
    /// `prompt` is the **signed** text: the original words, never the
    /// `[Context]`-framed rendering the adapter is given. The frame is
    /// addressing metadata for the model; the record keeps what was sent.
    pub fn begin_turn(
        &mut self,
        prompt: &str,
        operator_pubkey: Option<&str>,
        command_id: Option<&str>,
        sender_role: Option<&str>,
    ) -> Vec<Value> {
        self.text.clear();
        self.thoughts.clear();
        self.usage = None;
        self.tool_calls = 0;
        vec![crate::payload::user_prompt_item(
            prompt,
            false,
            operator_pubkey,
            command_id,
            sender_role,
        )]
    }

    /// Translate one `params.update` object.
    pub fn on_update(&mut self, update: &Value) -> Vec<Value> {
        let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
            return Vec::new();
        };
        if is_subagent_attributed(update) {
            // Buzz declares the `subagent-transcript` client capability, so
            // claude-agent-acp stops stripping a subagent's prose and thinking
            // and sends those frames stamped with the Task call that owns them.
            // We want them *on the wire* — every parsed line resets the read
            // loop's idle deadline, which is what keeps a long, quiet subagent
            // from being killed as a stalled turn — but they must not reach the
            // published record through this path. Folding them into the
            // top-level buffers would interleave a subagent's narration with
            // the agent's own with no way to tell them apart afterwards, and
            // this record is an archive: that is corruption, not a rendering
            // bug. Attributing them properly needs an item kind the clients can
            // render (today an unknown `kind` degrades to "no payload content
            // is surfaced"), so until that contract exists the honest handling
            // is to carry the liveness and publish nothing.
            return Vec::new();
        }
        if USAGE_UPDATE_VARIANTS.contains(&kind) {
            // Only the last one per turn is published: a running counter emitted
            // per token is noise, and the final value is the one that is true.
            self.usage = Some(usage_object(update));
            return Vec::new();
        }
        match kind {
            "agent_message_chunk" => {
                self.text.push_str(&content_text(update.get("content")));
                if self.text.len() >= COALESCE_FLUSH_BYTES {
                    return self.flush_text();
                }
                Vec::new()
            }
            "agent_thought_chunk" => {
                if !self.include_thoughts {
                    return Vec::new();
                }
                self.thoughts.push_str(&content_text(update.get("content")));
                if self.thoughts.len() >= COALESCE_FLUSH_BYTES {
                    return self.flush_thoughts();
                }
                Vec::new()
            }
            "tool_call" => {
                let mut items = self.flush_all();
                // Counted here rather than at the result, because a call that
                // never returns still consumed a call's worth of context.
                self.tool_calls = self.tool_calls.saturating_add(1);
                items.push(self.tool_call_item(update));
                items
            }
            "tool_call_update" => match terminal_status(update) {
                None => Vec::new(),
                Some(status) => {
                    let mut items = self.flush_all();
                    items.push(self.tool_result_item(update, status));
                    items
                }
            },
            "plan" => {
                let mut items = self.flush_all();
                items.push(plan_item(update));
                items
            }
            _ => Vec::new(),
        }
    }

    /// Close a turn: flush anything buffered and publish its final usage
    /// snapshot. The terminal `result` item is appended by the caller, which is
    /// the only party that knows how the turn actually ended.
    /// Tool calls opened during the turn that is open now.
    ///
    /// Reset by [`begin_turn`](Self::begin_turn), so the caller must read it
    /// before opening the next turn. Counts *calls*, not results: a call the
    /// agent abandoned still spent the context its arguments occupy.
    pub fn tool_calls(&self) -> u64 {
        self.tool_calls
    }

    pub fn close_turn(&mut self) -> Vec<Value> {
        let mut items = self.flush_all();
        if let Some(usage) = self.usage.take() {
            items.push(json!({ "kind": "context_window_updated", "usage": usage }));
        }
        self.tool_names.clear();
        self.tool_kinds.clear();
        items
    }

    /// [`close_turn`](Self::close_turn) followed by the terminal item.
    pub fn end_turn(&mut self, result: Value) -> Vec<Value> {
        let mut items = self.close_turn();
        items.push(result);
        items
    }

    /// Emit anything buffered without closing the turn.
    pub fn flush_all(&mut self) -> Vec<Value> {
        let mut items = self.flush_text();
        items.extend(self.flush_thoughts());
        items
    }

    fn flush_text(&mut self) -> Vec<Value> {
        if self.text.trim().is_empty() {
            self.text.clear();
            return Vec::new();
        }
        let text = std::mem::take(&mut self.text);
        vec![json!({ "kind": "assistant_text", "text": text })]
    }

    fn flush_thoughts(&mut self) -> Vec<Value> {
        if self.thoughts.trim().is_empty() {
            self.thoughts.clear();
            return Vec::new();
        }
        let text = std::mem::take(&mut self.thoughts);
        vec![json!({ "kind": "reasoning", "text": text })]
    }

    fn tool_call_item(&mut self, update: &Value) -> Value {
        let tool_id = string_field(update, "toolCallId").unwrap_or_default();
        let tool_name = tool_name(update);
        let tool_kind = string_field(update, "kind");
        if !tool_id.is_empty() {
            self.tool_names.insert(tool_id.clone(), tool_name.clone());
            if let Some(kind) = tool_kind.clone() {
                self.tool_kinds.insert(tool_id.clone(), kind);
            }
        }
        let mut tool = Map::new();
        tool.insert("toolName".into(), json!(tool_name));
        tool.insert("toolId".into(), json!(tool_id));
        tool.insert("input".into(), bounded_input(tool_input(update)));
        // ACP's `kind` is a *discriminant* ("read", "execute", "think"), not a
        // name, and it is optional in the spec. It gets its own key so a tool
        // that also sent a `title` cannot suppress it — and it is written only
        // when the adapter actually sent one, because an invented discriminant
        // is worse than an absent one.
        if let Some(kind) = tool_kind {
            tool.insert("toolKind".into(), Value::String(kind));
        }
        json!({ "kind": "tool_call", "tool": Value::Object(tool) })
    }

    fn tool_result_item(&mut self, update: &Value, status: &str) -> Value {
        let tool_id = string_field(update, "toolCallId").unwrap_or_default();
        let tool_name = self
            .tool_names
            .remove(&tool_id)
            .unwrap_or_else(|| tool_name(update));
        // The pairing is what carries the discriminant onto the result: a
        // `tool_call_update` rarely repeats `kind`, so it is recalled from the
        // opening call and only read off the update as a fallback.
        let tool_kind = self
            .tool_kinds
            .remove(&tool_id)
            .or_else(|| string_field(update, "kind"));
        let content = content_text(update.get("content"));
        let content = if content.is_empty() {
            raw_output_text(update)
        } else {
            content
        };
        let mut item = Map::new();
        item.insert("kind".into(), json!("tool_result"));
        item.insert("toolId".into(), json!(tool_id));
        item.insert("toolName".into(), json!(tool_name));
        if let Some(kind) = tool_kind {
            item.insert("toolKind".into(), Value::String(kind));
        }
        item.insert(
            "content".into(),
            json!(bound_text(&content, MAX_TOOL_CONTENT_BYTES)),
        );
        item.insert("isError".into(), json!(status == "failed"));
        Value::Object(item)
    }
}

/// Redact `item`, then shrink it until the whole envelope fits
/// `max_envelope_bytes`.
///
/// This is the last transformation an item receives before it is wrapped in a
/// CST envelope and signed, which is why redaction lives here rather than in
/// the individual item constructors. NIP-CST requires *every* published item to
/// be deeply redacted, and items reach the envelope from several producers —
/// this translator, the payload builders, and the provider's own lifecycle rows
/// — so one seam at the end covers all of them and cannot be forgotten by a
/// future producer. The redactor is
/// [`buzz_core::coding_session_context::sanitize_coding_session_context_content`],
/// the same one the private rehydration package uses: one implementation, so
/// the public transcript and the private handoff cannot drift apart on what
/// counts as host-private.
///
/// Redaction runs **first** because it can *grow* a value — an elided host path
/// is replaced by a longer marker — so fitting afterwards is what keeps the
/// 32 KiB cap honest.
///
/// `overhead` is the envelope's own serialized size around the item. The
/// dominant string is truncated first; when even an empty item would not fit —
/// or the item is pathological, all structure and no text — the item is replaced
/// by an `elided` marker that still records how much was dropped and its digest.
pub fn fit_item(item: Value, overhead: usize, max_envelope_bytes: usize) -> Value {
    shrink_item(
        sanitize_coding_session_context_content(&item),
        overhead,
        max_envelope_bytes,
    )
}

/// [`fit_item`] with the execution checkout available for safe path
/// relativization. The root itself is never copied into the signed item.
pub fn fit_item_for_workspace(
    item: Value,
    overhead: usize,
    max_envelope_bytes: usize,
    workspace_root: &Path,
) -> Value {
    shrink_item(
        sanitize_coding_session_context_content_for_workspace(&item, workspace_root),
        overhead,
        max_envelope_bytes,
    )
}

/// [`fit_item`], reporting the recoverable redactions it made.
///
/// The publish path uses this so the host can keep a private note of what it
/// removed from its own transcripts (see [`crate::redaction_vault`]). Only
/// recoverable classes are ever reported — a credential is redacted identically
/// and never named, and that gate lives in `buzz-core` where the redaction is
/// decided, not here.
///
/// The returned redactions describe the item *before* size-fitting, which is
/// correct: fitting can truncate a marker's surrounding text but never changes
/// a marker, so every digest reported still names something a reader can meet.
pub fn fit_item_recording(
    item: Value,
    overhead: usize,
    max_envelope_bytes: usize,
) -> (Value, Vec<Redaction>) {
    let (sanitized, redactions) = sanitize_coding_session_context_content_recording(&item);
    (
        shrink_item(sanitized, overhead, max_envelope_bytes),
        redactions,
    )
}

/// [`fit_item_recording`] with the execution checkout available for safe path
/// relativization, exactly as [`fit_item_for_workspace`] applies it.
pub fn fit_item_recording_for_workspace(
    item: Value,
    overhead: usize,
    max_envelope_bytes: usize,
    workspace_root: &Path,
) -> (Value, Vec<Redaction>) {
    let (sanitized, redactions) =
        sanitize_coding_session_context_content_recording_for_workspace(&item, workspace_root);
    (
        shrink_item(sanitized, overhead, max_envelope_bytes),
        redactions,
    )
}

fn shrink_item(item: Value, overhead: usize, max_envelope_bytes: usize) -> Value {
    /// Slack per shrink attempt, absorbing the elision marker's own bytes and
    /// the JSON escaping of whatever replaced them.
    const SHRINK_MARGIN: usize = 64;
    /// Enough passes to converge; a third means the item is not text-dominated.
    const MAX_ATTEMPTS: usize = 3;

    let serialized_len = |value: &Value| serde_json::to_string(value).map(|s| s.len()).unwrap_or(0);
    let budget = max_envelope_bytes.saturating_sub(overhead);
    if serialized_len(&item) <= budget {
        return item;
    }

    let mut shrunk = item.clone();
    if let Some(object) = shrunk.as_object_mut() {
        object.insert("truncated".into(), json!(true));
    }
    for _ in 0..MAX_ATTEMPTS {
        let current = serialized_len(&shrunk);
        if current <= budget {
            return shrunk;
        }
        let excess = current - budget + SHRINK_MARGIN;
        if !truncate_longest_string(&mut shrunk, excess) {
            break;
        }
    }

    let body = serde_json::to_string(&item).unwrap_or_default();
    json!({
        "kind": "elided",
        "reason": "oversize",
        "byteCount": body.len(),
        "contentDigest": digest(&body),
    })
}

/// Build the ACP `plan` item.
///
/// Emitted as its own `plan` kind rather than smuggled through the donor's
/// `exit_plan_mode` tool-call special case: this provider *has* a first-class
/// plan signal and flattening it into a fake tool call would lose the per-entry
/// status. `text` carries a rendered checklist so a consumer that has not yet
/// learned the kind still has something to show.
fn plan_item(update: &Value) -> Value {
    let entries: Vec<Value> = update
        .get("entries")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    json!({
                        "content": string_field(entry, "content").unwrap_or_default(),
                        "priority": string_field(entry, "priority").unwrap_or_default(),
                        "status": string_field(entry, "status").unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let text = entries
        .iter()
        .filter_map(|entry| {
            let content = entry.get("content")?.as_str()?;
            if content.is_empty() {
                return None;
            }
            let checkbox = if entry.get("status")?.as_str() == Some("completed") {
                "[x]"
            } else {
                "[ ]"
            };
            let suffix = if entry.get("status")?.as_str() == Some("in_progress") {
                " (in progress)"
            } else {
                ""
            };
            Some(format!("- {checkbox} {content}{suffix}"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    json!({ "kind": "plan", "entries": entries, "text": bound_text(&text, MAX_TOOL_CONTENT_BYTES) })
}

/// Whether this update describes a subagent's work rather than the agent's own.
///
/// claude-agent-acp stamps `_meta.claudeCode.parentToolUseId` onto every frame
/// produced inside a Task subagent, naming the tool call that spawned it, and
/// uses the same field to decide what counts as the turn's own answer. `_meta`
/// rides on the `update` object itself, not on the notification's `params`.
fn is_subagent_attributed(update: &Value) -> bool {
    update
        .get("_meta")
        .and_then(|meta| meta.get("claudeCode"))
        .and_then(|claude| claude.get("parentToolUseId"))
        .and_then(Value::as_str)
        .is_some_and(|parent| !parent.is_empty())
}

/// Whether a `tool_call_update` reports a terminal status.
fn terminal_status(update: &Value) -> Option<&str> {
    match update.get("status").and_then(Value::as_str) {
        Some(status @ ("completed" | "failed")) => Some(status),
        _ => None,
    }
}

/// Collect numeric and boolean fields into a flat usage object.
///
/// Only numbers and booleans survive: the consumer's `context_window_updated`
/// renderer discards everything else, so nesting or stringly-typed extras would
/// be dead weight in a signed, size-capped event.
fn usage_object(update: &Value) -> Value {
    let mut usage = Map::new();
    for source in [update.get("usage"), Some(update)].into_iter().flatten() {
        let Some(object) = source.as_object() else {
            continue;
        };
        for (key, value) in object {
            if key == "sessionUpdate" || key == "usage" {
                continue;
            }
            if value.is_number() || value.is_boolean() {
                usage.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
    }
    Value::Object(usage)
}

/// Pull a tool's arguments out of whichever field the adapter used.
fn tool_input(update: &Value) -> Value {
    for key in ["rawInput", "input", "arguments", "args"] {
        if let Some(value) = update.get(key) {
            if value.is_object() {
                return value.clone();
            }
        }
    }
    json!({})
}

/// Bound a tool input to [`MAX_TOOL_INPUT_BYTES`], keeping it an object.
///
/// The replacement stays an object because the consumer feeds `input` straight
/// into its tool classifier as `args`; a string there would silently degrade
/// every oversized call to an unclassified one.
fn bounded_input(input: Value) -> Value {
    let serialized = serde_json::to_string(&input).unwrap_or_default();
    if serialized.len() <= MAX_TOOL_INPUT_BYTES {
        return input;
    }
    json!({
        "truncated": true,
        "byteCount": serialized.len(),
        "contentDigest": digest(&serialized),
        "preview": bound_text(&serialized, MAX_TOOL_INPUT_BYTES / 2),
    })
}

/// The tool's *display* name.
///
/// `kind` stays last in the chain purely as a last-resort label for an adapter
/// that sent neither a name nor a title — it is no longer load-bearing, because
/// the discriminant is now published separately as `toolKind`, where a present
/// `title` cannot suppress it.
fn tool_name(update: &Value) -> String {
    for key in ["toolName", "title", "kind"] {
        if let Some(value) = string_field(update, key) {
            if !value.is_empty() {
                return value;
            }
        }
    }
    "unknown_tool".to_owned()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Flatten an ACP content block, block array, or bare string into text.
fn content_text(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .map(|block| content_text(Some(block)))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => match object.get("text") {
            Some(Value::String(text)) => text.clone(),
            _ => content_text(object.get("content")),
        },
        _ => String::new(),
    }
}

fn raw_output_text(update: &Value) -> String {
    match update.get("rawOutput") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// Truncate `text` to `max_bytes`, recording what was dropped.
pub fn bound_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let dropped = &text[floor_boundary(text, max_bytes)..];
    let marker = elision_marker(dropped);
    let keep = floor_boundary(text, max_bytes.saturating_sub(marker.len()));
    format!("{}{marker}", &text[..keep])
}

fn elision_marker(dropped: &str) -> String {
    format!(
        "…[elided {} bytes, sha256:{}]",
        dropped.len(),
        digest(dropped)
    )
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn digest(body: &str) -> String {
    hex::encode(Sha256::digest(body.as_bytes()))
}

/// Truncate the single longest string in `value` by at least `excess` bytes.
///
/// Returns `false` when no string is long enough to be worth cutting, which is
/// the signal that the item is structurally oversized and must be elided whole.
fn truncate_longest_string(value: &mut Value, excess: usize) -> bool {
    let Some(longest) = longest_string_len(value) else {
        return false;
    };
    if longest < MIN_TRUNCATABLE_BYTES || longest <= excess {
        return false;
    }
    let target = longest - excess;
    truncate_first_string_of_len(value, longest, target)
}

fn longest_string_len(value: &Value) -> Option<usize> {
    match value {
        Value::String(text) => Some(text.len()),
        Value::Array(items) => items.iter().filter_map(longest_string_len).max(),
        Value::Object(object) => object.values().filter_map(longest_string_len).max(),
        _ => None,
    }
}

fn truncate_first_string_of_len(value: &mut Value, len: usize, target: usize) -> bool {
    match value {
        Value::String(text) if text.len() == len => {
            *text = bound_text(text, target);
            true
        }
        Value::Array(items) => items
            .iter_mut()
            .any(|item| truncate_first_string_of_len(item, len, target)),
        Value::Object(object) => object
            .values_mut()
            .any(|item| truncate_first_string_of_len(item, len, target)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(value: Value) -> Value {
        value
    }

    /// A prose chunk produced *inside* a Task subagent, stamped the way
    /// claude-agent-acp stamps it.
    fn subagent_chunk(parent_tool_use_id: &str, text: &str) -> Value {
        update(json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": text },
            "_meta": { "claudeCode": { "parentToolUseId": parent_tool_use_id } },
        }))
    }

    fn kinds(items: &[Value]) -> Vec<&str> {
        items
            .iter()
            .filter_map(|item| item.get("kind").and_then(Value::as_str))
            .collect()
    }

    fn chunk(text: &str) -> Value {
        update(json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": text },
        }))
    }

    fn thought(text: &str) -> Value {
        update(json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": { "type": "text", "text": text },
        }))
    }

    fn tool_call(id: &str, name: &str, input: Value) -> Value {
        update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": name,
            "status": "in_progress",
            "rawInput": input,
        }))
    }

    fn tool_done(id: &str, status: &str, content: &str) -> Value {
        update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": status,
            "content": [{ "type": "content", "content": { "type": "text", "text": content } }],
        }))
    }

    fn result() -> Value {
        crate::payload::result_item(
            crate::payload::ResultSubtype::Success,
            1234,
            "completed",
            crate::payload::TurnCost::default(),
            crate::payload::TurnUsageReport::default(),
        )
    }

    /// The translator counts the turn's tool calls, because it is the only
    /// party that sees every one of them, and the count belongs in the turn's
    /// usage block.
    #[test]
    fn the_translator_counts_this_turns_tool_calls() {
        let mut translator = TranscriptTranslator::new(false);
        let _ = translator.begin_turn("do the thing", None, None, None);
        assert_eq!(translator.tool_calls(), 0);
        let _ = translator.on_update(&tool_call("t1", "Read", json!({})));
        let _ = translator.on_update(&tool_done("t1", "completed", "ok"));
        let _ = translator.on_update(&tool_call("t2", "Bash", json!({})));
        assert_eq!(
            translator.tool_calls(),
            2,
            "two calls opened; a result is not a second call"
        );
    }

    /// The count is per turn: the next turn starts at zero rather than
    /// inheriting the last one's total.
    #[test]
    fn the_tool_call_count_resets_on_the_next_turn() {
        let mut translator = TranscriptTranslator::new(false);
        let _ = translator.begin_turn("first", None, None, None);
        let _ = translator.on_update(&tool_call("t1", "Read", json!({})));
        let _ = translator.close_turn();
        let _ = translator.begin_turn("second", None, None, None);
        assert_eq!(translator.tool_calls(), 0);
    }

    /// The golden case: one prompt, some prose, a tool call and its result,
    /// more prose, then the terminal item — in that exact order.
    #[test]
    fn a_whole_turn_translates_to_the_expected_item_sequence() {
        let mut translator = TranscriptTranslator::new(true);
        let mut items = translator.begin_turn("do the thing", None, None, None);
        items.extend(translator.on_update(&thought("let me look")));
        items.extend(translator.on_update(&chunk("I will ")));
        items.extend(translator.on_update(&chunk("read the file.")));
        items.extend(translator.on_update(&tool_call("t1", "read_file", json!({"path": "/x"}))));
        items.extend(translator.on_update(&tool_done("t1", "completed", "file body")));
        items.extend(translator.on_update(&chunk("Done.")));
        items.extend(translator.end_turn(result()));

        assert_eq!(
            kinds(&items),
            vec![
                "user_prompt",
                "assistant_text",
                "reasoning",
                "tool_call",
                "tool_result",
                "assistant_text",
                "result",
            ]
        );
        assert_eq!(items[0]["content"], "do the thing");
        assert_eq!(items[0]["steered"], false);
        assert_eq!(items[1]["text"], "I will read the file.");
        assert_eq!(items[2]["text"], "let me look");
        assert_eq!(items[3]["tool"]["toolName"], "read_file");
        assert_eq!(items[3]["tool"]["toolId"], "t1");
        assert_eq!(items[3]["tool"]["input"]["path"], "/x");
        assert_eq!(items[4]["toolId"], "t1");
        assert_eq!(items[4]["toolName"], "read_file");
        assert_eq!(items[4]["content"], "file body");
        assert_eq!(items[4]["isError"], false);
        assert_eq!(items[5]["text"], "Done.");
        assert_eq!(items[6]["subtype"], "success");
    }

    #[test]
    fn a_failed_tool_call_is_marked_as_an_error() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&tool_call("t1", "bash", json!({"command": "false"})));
        let items = translator.on_update(&tool_done("t1", "failed", "exit 1"));
        assert_eq!(items[0]["isError"], true);
        assert_eq!(items[0]["toolName"], "bash");
    }

    /// Non-terminal progress updates are noise: the consumer pairs a call with
    /// its result, and an `in_progress` row between them says nothing new.
    #[test]
    fn non_terminal_tool_updates_produce_nothing() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&tool_call("t1", "bash", json!({})));
        assert!(translator
            .on_update(&update(json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "t1",
                "status": "in_progress",
            })))
            .is_empty());
    }

    #[test]
    fn text_flushes_at_the_size_boundary_without_waiting_for_the_turn() {
        let mut translator = TranscriptTranslator::new(true);
        assert!(translator.on_update(&chunk(&"a".repeat(1024))).is_empty());
        let items = translator.on_update(&chunk(&"b".repeat(COALESCE_FLUSH_BYTES)));
        assert_eq!(kinds(&items), vec!["assistant_text"]);
        assert_eq!(
            items[0]["text"].as_str().expect("text").len(),
            1024 + COALESCE_FLUSH_BYTES
        );
        // The buffer is empty afterwards, so the turn end adds only the result.
        assert_eq!(kinds(&translator.end_turn(result())), vec!["result"]);
    }

    /// Prose written before the agent reached for a tool belongs *before* the
    /// tool call in the record, so a tool boundary forces a flush.
    #[test]
    fn a_tool_call_flushes_buffered_prose_first() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&chunk("thinking out loud"));
        let items = translator.on_update(&tool_call("t1", "bash", json!({})));
        assert_eq!(kinds(&items), vec!["assistant_text", "tool_call"]);
    }

    #[test]
    fn thoughts_are_gated_by_configuration() {
        let mut off = TranscriptTranslator::new(false);
        assert!(off.on_update(&thought("secret")).is_empty());
        assert_eq!(kinds(&off.end_turn(result())), vec!["result"]);

        let mut on = TranscriptTranslator::new(true);
        on.on_update(&thought("secret"));
        assert_eq!(kinds(&on.end_turn(result())), vec!["reasoning", "result"]);
    }

    #[test]
    fn whitespace_only_buffers_never_become_items() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&chunk("   \n "));
        assert_eq!(kinds(&translator.end_turn(result())), vec!["result"]);
    }

    /// A running token counter emitted per chunk would be pure noise; only the
    /// last snapshot of a turn is true when the turn ends.
    #[test]
    fn only_the_last_usage_update_of_a_turn_is_published() {
        for variant in USAGE_UPDATE_VARIANTS {
            let mut translator = TranscriptTranslator::new(true);
            assert!(translator
                .on_update(&update(json!({
                    "sessionUpdate": variant,
                    "usage": { "inputTokens": 10, "outputTokens": 1 },
                })))
                .is_empty());
            assert!(translator
                .on_update(&update(json!({
                    "sessionUpdate": variant,
                    "usage": { "inputTokens": 10, "outputTokens": 42, "cached": true },
                })))
                .is_empty());

            let items = translator.end_turn(result());
            assert_eq!(kinds(&items), vec!["context_window_updated", "result"]);
            assert_eq!(items[0]["usage"]["outputTokens"], 42);
            assert_eq!(items[0]["usage"]["cached"], true);
        }
    }

    #[test]
    fn usage_numbers_at_the_top_level_are_collected_too() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&update(json!({
            "sessionUpdate": "usage_update",
            "contextWindow": 200_000,
            "label": "ignored",
        })));
        let items = translator.end_turn(result());
        assert_eq!(items[0]["usage"]["contextWindow"], 200_000);
        assert!(items[0]["usage"].get("label").is_none());
    }

    #[test]
    fn a_plan_update_carries_entries_and_a_rendered_checklist() {
        let mut translator = TranscriptTranslator::new(true);
        let items = translator.on_update(&update(json!({
            "sessionUpdate": "plan",
            "entries": [
                { "content": "read", "priority": "high", "status": "completed" },
                { "content": "write", "priority": "high", "status": "in_progress" },
            ],
        })));
        assert_eq!(kinds(&items), vec!["plan"]);
        assert_eq!(items[0]["entries"].as_array().expect("entries").len(), 2);
        assert_eq!(items[0]["text"], "- [x] read\n- [ ] write (in progress)");
    }

    #[test]
    fn unknown_update_kinds_are_ignored_rather_than_guessed_at() {
        let mut translator = TranscriptTranslator::new(true);
        for kind in [
            "session_info_update",
            "available_commands_update",
            "keepalive",
        ] {
            assert!(translator
                .on_update(&update(json!({ "sessionUpdate": kind })))
                .is_empty());
        }
        assert!(translator
            .on_update(&json!({ "no": "discriminator" }))
            .is_empty());
    }

    /// The regression that declaring `subagent-transcript` would otherwise
    /// introduce.
    ///
    /// Before the capability, the adapter filtered a subagent's prose out
    /// itself and this case could not arise. Now the frames arrive, and if they
    /// fell through to the normal `agent_message_chunk` arm the subagent's
    /// narration would be concatenated into the same buffer as the agent's own
    /// and flushed as one indistinguishable block. The assertion is on the
    /// *seam*: the two top-level chunks must still meet exactly, with nothing
    /// of the subagent's between them.
    #[test]
    fn a_subagents_prose_never_lands_in_the_agents_own_narrative() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&chunk("I will delegate this. "));
        assert!(
            translator
                .on_update(&subagent_chunk("t-parent", "Reading forty files…"))
                .is_empty(),
            "a subagent chunk must publish nothing on its own"
        );
        translator.on_update(&subagent_chunk("t-parent", " and forty more."));
        let items = translator.close_turn();

        assert_eq!(kinds(&items), vec!["assistant_text"]);
        assert_eq!(
            items[0]["text"], "I will delegate this. ",
            "the subagent's narration leaked into the agent's own text"
        );
    }

    /// Every frame kind a subagent can produce is covered, not just prose.
    ///
    /// A subagent's tool calls are the noisiest of these — an unguarded
    /// `tool_call` arm would also register the child's `toolCallId` in the
    /// pairing maps, so a later top-level result could resolve against a
    /// subagent's call.
    #[test]
    fn subagent_attribution_is_honoured_for_every_frame_kind() {
        let mut translator = TranscriptTranslator::new(true);
        for kind in [
            "agent_message_chunk",
            "agent_thought_chunk",
            "tool_call",
            "tool_call_update",
            "plan",
        ] {
            let mut frame = json!({
                "sessionUpdate": kind,
                "toolCallId": "child-1",
                "title": "child work",
                "status": "completed",
                "content": { "type": "text", "text": "child prose" },
                "entries": [],
            });
            frame["_meta"]["claudeCode"]["parentToolUseId"] = json!("t-parent");
            assert!(
                translator.on_update(&update(frame)).is_empty(),
                "subagent-attributed `{kind}` must not be published"
            );
        }
        assert!(
            translator.close_turn().is_empty(),
            "nothing from a subagent may survive to the turn's close"
        );
    }

    /// The guard keys on a *present, non-empty* parent id and nothing else, so
    /// an adapter that sends `_meta` without one cannot silently blank the
    /// agent's own transcript.
    #[test]
    fn an_empty_or_absent_parent_id_leaves_a_frame_top_level() {
        let mut translator = TranscriptTranslator::new(true);
        let mut empty_parent = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "still mine" },
        });
        empty_parent["_meta"]["claudeCode"]["parentToolUseId"] = json!("");
        translator.on_update(&update(empty_parent));

        let mut unrelated_meta = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": " and mine" },
        });
        unrelated_meta["_meta"]["claudeCode"]["toolName"] = json!("Task");
        translator.on_update(&update(unrelated_meta));

        let items = translator.close_turn();
        assert_eq!(kinds(&items), vec!["assistant_text"]);
        assert_eq!(items[0]["text"], "still mine and mine");
    }

    /// An oversized tool input must stay an *object*: the consumer feeds it
    /// straight into its tool classifier, and a string there would silently
    /// degrade every large call to an unclassified one.
    #[test]
    fn an_oversized_tool_input_becomes_a_bounded_object_with_a_digest() {
        let mut translator = TranscriptTranslator::new(true);
        let huge = "x".repeat(MAX_TOOL_INPUT_BYTES * 2);
        let items = translator.on_update(&tool_call("t1", "write", json!({ "body": huge })));
        let input = &items[0]["tool"]["input"];
        assert!(input.is_object());
        assert_eq!(input["truncated"], true);
        assert!(input["byteCount"].as_u64().expect("count") > MAX_TOOL_INPUT_BYTES as u64);
        assert!(input["contentDigest"]
            .as_str()
            .expect("digest")
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn an_oversized_tool_result_is_truncated_with_an_elision_marker() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&tool_call("t1", "read", json!({})));
        let items = translator.on_update(&tool_done(
            "t1",
            "completed",
            &"y".repeat(MAX_TOOL_CONTENT_BYTES * 2),
        ));
        let content = items[0]["content"].as_str().expect("content");
        assert!(content.len() <= MAX_TOOL_CONTENT_BYTES);
        assert!(content.contains("…[elided "));
        assert!(content.contains("sha256:"));
    }

    #[test]
    fn an_item_that_fits_is_returned_untouched() {
        let item = json!({ "kind": "assistant_text", "text": "short" });
        assert_eq!(fit_item(item.clone(), 100, 32 * 1024), item);
    }

    #[test]
    fn an_oversized_item_is_shrunk_by_its_dominant_string() {
        let item = json!({ "kind": "assistant_text", "text": "z".repeat(40 * 1024) });
        let fitted = fit_item(item, 512, 32 * 1024);
        assert_eq!(fitted["kind"], "assistant_text");
        assert_eq!(fitted["truncated"], true);
        let text = fitted["text"].as_str().expect("text");
        assert!(text.contains("…[elided "));
        assert!(serde_json::to_string(&fitted).expect("json").len() <= 32 * 1024 - 512);
    }

    /// All structure and no text: nothing can be usefully truncated, so the
    /// whole item is replaced by a marker that still records its size and hash.
    #[test]
    fn a_pathological_item_is_replaced_by_an_elided_marker() {
        let entries: Vec<Value> = (0..4000).map(|index| json!({ "n": index })).collect();
        let item = json!({ "kind": "plan", "entries": entries });
        let fitted = fit_item(item, 512, 4 * 1024);
        assert_eq!(fitted["kind"], "elided");
        assert_eq!(fitted["reason"], "oversize");
        assert!(fitted["byteCount"].as_u64().expect("count") > 4 * 1024);
        assert_eq!(
            fitted["contentDigest"].as_str().expect("digest").len(),
            64,
            "a sha-256 in lowercase hex"
        );
    }

    #[test]
    fn elision_never_splits_a_multibyte_character() {
        let text = "🐝".repeat(400);
        let bounded = bound_text(&text, 300);
        assert!(bounded.len() <= 300);
        assert!(std::str::from_utf8(bounded.as_bytes()).is_ok());
    }

    #[test]
    fn tool_results_fall_back_to_raw_output_when_no_content_block_is_sent() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&tool_call("t1", "bash", json!({})));
        let items = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "status": "failed",
            "rawOutput": { "error": "boom" },
        })));
        assert!(items[0]["content"]
            .as_str()
            .expect("content")
            .contains("boom"));
        assert_eq!(items[0]["isError"], true);
    }

    #[test]
    fn a_new_turn_discards_anything_left_from_the_previous_one() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&chunk("stale"));
        let items = translator.begin_turn("fresh", None, None, None);
        assert_eq!(kinds(&items), vec!["user_prompt"]);
        assert_eq!(kinds(&translator.end_turn(result())), vec!["result"]);
    }

    /// Sessions are multi-operator, so the opening item has to name the
    /// operator the provider verified — not the founder, and not nobody.
    #[test]
    fn begin_turn_stamps_the_commanding_operator_on_the_prompt() {
        let operator = "b".repeat(64);
        let mut translator = TranscriptTranslator::new(false);
        let items = translator.begin_turn("go", Some(&operator), Some("turn-1"), None);
        assert_eq!(items[0]["kind"], "user_prompt");
        assert_eq!(items[0]["operatorPubkey"], operator);
    }

    /// Without a witnessed operator the key is absent rather than `null`: an
    /// explicit null would claim the provider observed "no operator".
    #[test]
    fn begin_turn_omits_attribution_when_no_operator_is_known() {
        let mut translator = TranscriptTranslator::new(false);
        let items = translator.begin_turn("go", None, None, None);
        assert!(items[0].get("operatorPubkey").is_none());
        assert!(items[0].get("commandId").is_none());
    }

    /// NIP-CST :43 — the signed `item` is "deeply redacted"; host paths are
    /// forbidden. The tool-call path is where they actually arrive: ACP's
    /// `title` is prose the agent wrote, and `rawInput` carries the literal
    /// arguments, so an absolute checkout path reaches the envelope verbatim
    /// unless the item is redacted before it is signed.
    #[test]
    fn an_absolute_host_path_in_a_tool_call_never_reaches_the_signed_item() {
        let mut translator = TranscriptTranslator::new(true);
        let call = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Read /Users/brian/Projects/buzz/secret.rs",
            "kind": "read",
            "status": "in_progress",
            "rawInput": { "file_path": "/Users/brian/Projects/buzz/secret.rs" },
        })));
        let done = translator.on_update(&tool_done(
            "t1",
            "completed",
            "opened /Users/brian/Projects/buzz/secret.rs",
        ));

        for item in call.iter().chain(done.iter()) {
            let signed = fit_item(item.clone(), 512, 32 * 1024);
            let serialized = serde_json::to_string(&signed).expect("json");
            assert!(
                !serialized.contains("/Users/brian"),
                "a host path survived into a signed transcript item: {serialized}"
            );
            assert!(
                serialized.contains("[elided private context: "),
                "the path was dropped without a visible elision: {serialized}"
            );
        }
    }

    #[test]
    fn a_workspace_citation_is_signed_as_a_relative_path() {
        let item = json!({
            "kind": "assistant_text",
            "text": "See `/Users/brian/Projects/beekeeper/desktop/src/App.tsx:42`; not /Users/brian/.ssh/id_ed25519"
        });
        let signed = fit_item_for_workspace(
            item,
            512,
            32 * 1024,
            Path::new("/Users/brian/Projects/beekeeper"),
        );
        let text = signed["text"].as_str().expect("text");
        assert!(text.contains("`desktop/src/App.tsx:42`"), "{text}");
        assert!(!text.contains("/Users/brian/Projects/beekeeper"));
        assert!(!text.contains("/Users/brian/.ssh"));
        assert!(text.contains("[elided private context: "));
    }

    /// ACP's `kind` is a discriminant, not a name. Folding it into the name
    /// chain meant any agent that also sent a `title` silently erased it.
    #[test]
    fn a_present_acp_kind_survives_beside_a_present_title() {
        let mut translator = TranscriptTranslator::new(true);
        let call = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Run the test suite",
            "kind": "execute",
            "status": "in_progress",
            "rawInput": {},
        })));
        assert_eq!(call[0]["tool"]["toolName"], "Run the test suite");
        assert_eq!(call[0]["tool"]["toolKind"], "execute");
        let signed = fit_item(call[0].clone(), 512, 32 * 1024);
        assert_eq!(signed["tool"]["toolKind"], "execute");

        // The pairing carries the discriminant onto the result too.
        let done = translator.on_update(&tool_done("t1", "completed", "ok"));
        assert_eq!(done[0]["toolName"], "Run the test suite");
        assert_eq!(done[0]["toolKind"], "execute");
    }

    /// ACP marks `kind` optional, so an absent one stays absent — the provider
    /// never guesses a discriminant it was not given.
    #[test]
    fn an_absent_acp_kind_is_never_invented() {
        let mut translator = TranscriptTranslator::new(true);
        let call = translator.on_update(&tool_call("t1", "read_file", json!({})));
        assert_eq!(call[0]["tool"]["toolName"], "read_file");
        assert!(call[0]["tool"].get("toolKind").is_none());
        let done = translator.on_update(&tool_done("t1", "completed", "ok"));
        assert!(done[0].get("toolKind").is_none());
    }
}
