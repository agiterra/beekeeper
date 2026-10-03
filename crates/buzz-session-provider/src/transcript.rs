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
//! actual work. So text accumulates and flushes at four boundaries:
//!
//! - **size** — [`COALESCE_FLUSH_BYTES`], well under the 32 KiB event cap so a
//!   flushed block never needs truncating;
//! - **paragraph** — only when switched on
//!   ([`with_paragraph_flush`](TranscriptTranslator::with_paragraph_flush);
//!   off by default, because every reader older than the change shows each
//!   item as its own message), and for the agent's own prose only: a blank
//!   line outside an open
//!   code fence, once at least [`MIN_PARAGRAPH_FLUSH_BYTES`] has built up, so a
//!   long answer reaches a reader paragraph by paragraph instead of all at
//!   once. The boundary is a property of the text, not of time, so the
//!   translator keeps no clock. A blank line between items of one list, or
//!   before an indented continuation, is not a boundary: a client rendering
//!   each item alone would show two lists. The split moves no byte — the items
//!   concatenate to exactly what the agent wrote — so a client can join a
//!   turn's consecutive `assistant_text` items back into one message. See
//!   `ParagraphScanner`;
//! - **narrative** — any tool call or tool result, because prose written *before*
//!   the agent reached for a tool belongs before it in the record;
//! - **turn end** — nothing is left buffered when the turn's `result` lands.
//!
//! # Subagents
//!
//! A frame claude-agent-acp stamps with `_meta.claudeCode.parentToolUseId` was
//! produced inside a Task/Agent subagent. It is published as the item it would
//! be anyway, plus one top-level `parentToolId` naming the Task/Agent call that
//! owns it, so a client can nest a subagent's work under its spawn and an old
//! client — which ignores unknown fields — still renders every item (ledger
//! 308). Subagent prose coalesces in its **own** buffer per owning call: a
//! subagent's narration must never merge with the agent's or another
//! subagent's, because once flushed as one block nothing could split it again.
//! Those buffers flush on that subagent's next non-text item, on the owning
//! call's terminal result, at turn end and at the size boundary — never at a
//! paragraph, which is for the narrative a person reads live. Thoughts, which
//! readers fold away, do not split at paragraphs either.
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
//! # Command output completeness
//!
//! A `tool_result` whose content came from the adapter's final frame (its
//! `content` blocks or `rawOutput`) carries no completeness keys: the adapter
//! handed over the output as a whole. One assembled only from streamed output
//! chunks (codex-acp's `terminal_output_delta`) cannot be taken as whole —
//! Codex starts streaming a command only after it has spawned it, so output
//! printed in between is never streamed, and codex-acp does not resend it at
//! completion once any chunk streamed. Such a result carries three keys,
//! always together:
//!
//! - `contentSource`: `"streamed_deltas"` — the content is the streamed bytes;
//!   or `"native_rollout"` — the provider recovered the whole output from the
//!   agent's own record of the command and the streamed bytes were its tail;
//! - `outputComplete`: `true` only when the provider verified the content
//!   against that record; `false` when it could not;
//! - `outputGap` (only when `outputComplete` is `false`):
//!   `{ "streamedBytes", "aggregatedBytes"? }` — what was streamed, and what
//!   the agent's record says the command printed, when that record was read.
//!
//! The translator emits the unverified form; the provider's reconciliation
//! (the crate's `native_output` module) upgrades it or leaves it, never
//! guesses.
//!
//! # Truncation
//!
//! Two independent caps. Tool inputs, outputs and edit payloads are bounded on
//! the way in ([`MAX_TOOL_INPUT_BYTES`] / [`MAX_TOOL_CONTENT_BYTES`] /
//! [`buzz_core::coding_session_payload::MAX_TOOL_EDIT_PAYLOAD_BYTES`]) so one
//! enormous file read cannot dominate an item. The envelope is then bounded on the way out by
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
use buzz_core::coding_session_payload::{tool_edit_payload, ToolEditChange};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// Flush the text buffer once it reaches this size.
pub const COALESCE_FLUSH_BYTES: usize = 24 * 1024;
/// The shortest prose a paragraph boundary may flush on its own.
///
/// Every flush is a signed, durably queued kind 44225 event, so the boundary
/// that makes an answer arrive paragraph by paragraph is also a multiplier on
/// relay volume. Below this size a paragraph rides along with the next one
/// instead of being published alone. 512 bytes bounds a turn's prose events at
/// one per half-kilobyte of prose (at most 48 per 24 KiB buffer, against one
/// today), keeps each event's id, signature, tags and envelope well under the
/// text it carries, and — at the few hundred bytes a second a model streams —
/// holds a session to roughly one prose event a second without a clock. Short
/// openers ("Sure.", "Done:") and terse bullet lists coalesce, which is where
/// an event per paragraph would have been mostly envelope.
pub const MIN_PARAGRAPH_FLUSH_BYTES: usize = 512;
/// Cap on a serialized tool input before it is replaced by a digest.
pub const MAX_TOOL_INPUT_BYTES: usize = 8 * 1024;
/// Cap on a tool result's textual content.
pub const MAX_TOOL_CONTENT_BYTES: usize = 8 * 1024;
/// `contentSource` of a result assembled only from streamed output chunks.
pub const CONTENT_SOURCE_STREAMED: &str = "streamed_deltas";
/// `contentSource` of a result the provider recovered whole from the agent's
/// own record of the command.
pub const CONTENT_SOURCE_NATIVE_ROLLOUT: &str = "native_rollout";
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
    /// Whether `text` flushes at paragraph boundaries.
    paragraph_flush: bool,
    /// Where `text` may be split at a paragraph boundary. Fed only while
    /// `paragraph_flush` is on.
    paragraphs: ParagraphScanner,
    thoughts: String,
    usage: Option<Value>,
    tool_calls: u64,
    tools: HashMap<String, ToolMemo>,
    /// Memo for a frame that carried no `toolCallId`, rebuilt per frame.
    anonymous: ToolMemo,
    /// Calls opened with no arguments yet, oldest first, each with its opening
    /// frame. See [`hold_or_publish`](Self::hold_or_publish).
    held: Vec<HeldCall>,
    /// Prose buffers for subagents, keyed by the Task/Agent call that owns
    /// them, in the order each was opened so a turn-end flush is deterministic.
    subagents: Vec<(String, ProseBuffers)>,
    /// Stream-assembled results produced since the last
    /// [`take_stream_checks`](Self::take_stream_checks).
    stream_checks: Vec<StreamCheck>,
}

/// A call held back until its arguments stream in.
///
/// `scope` is the owning Task/Agent call for a subagent's call and `None` for
/// the agent's own. Holding is per scope because subagents run alongside the
/// agent and each other: if a sibling's next item released every held call,
/// a lead's `Agent` call still streaming its arguments would be published as
/// an empty "Task" the moment some other subagent ran a tool — exactly the
/// argument-less row the hold exists to prevent.
#[derive(Debug)]
struct HeldCall {
    id: String,
    scope: Option<String>,
    frame: Value,
}

/// One subagent's not-yet-flushed prose.
#[derive(Debug, Default)]
struct ProseBuffers {
    text: String,
    thoughts: String,
}

/// The longest head of a line the paragraph scanner keeps. Enough to read a
/// list marker or a fence; a longer line is ordinary content whatever its tail.
const LINE_HEAD_BYTES: usize = 64;

/// Finds paragraph boundaries in the agent's streamed prose.
///
/// Fed each chunk as it arrives, with the buffer offset it lands at, and kept
/// incremental so a long buffer is never rescanned per token. A boundary is
/// the end of a blank line (whitespace only) that closes a block with content
/// and is outside an open code fence. The offset is just past the blank line's
/// newline, so the item ends with the paragraph break and the next begins
/// with the next paragraph's first character.
///
/// After a block that held a list item the boundary waits for the first
/// characters of the next line: a list marker or an indented line continues
/// the list, and the blank line is then not a boundary.
#[derive(Debug, Default)]
struct ParagraphScanner {
    /// The open fence's character and run length.
    fence: Option<(char, usize)>,
    /// The head of the line being streamed, up to [`LINE_HEAD_BYTES`].
    line: String,
    /// The line being streamed is longer than `line` holds.
    line_overflowed: bool,
    /// The block since the last blank line has a non-blank line.
    block_has_content: bool,
    /// The block since the last blank line has a list item.
    block_has_list: bool,
    /// A boundary after a list block, waiting on the next line.
    pending: Option<usize>,
    /// The latest boundary found and not yet taken.
    boundary: Option<usize>,
}

impl ParagraphScanner {
    /// Read `chunk`, which starts at byte `base` of the buffer.
    fn feed(&mut self, chunk: &str, base: usize) {
        for (index, ch) in chunk.char_indices() {
            if ch == '\n' {
                self.end_line(base + index + 1);
                continue;
            }
            if self.line.len() + ch.len_utf8() <= LINE_HEAD_BYTES {
                self.line.push(ch);
            } else {
                self.line_overflowed = true;
            }
            if self.pending.is_some() {
                self.decide_pending(false);
            }
        }
    }

    fn end_line(&mut self, offset: usize) {
        let blank = self.line.trim().is_empty() && !self.line_overflowed;
        if self.pending.is_some() {
            if blank {
                // Another blank line: still undecided, and the split moves
                // past it.
                self.pending = Some(offset);
            } else {
                self.decide_pending(true);
            }
        }
        let line = std::mem::take(&mut self.line);
        let overflowed = std::mem::take(&mut self.line_overflowed);
        if let Some((fence, run)) = self.fence {
            if !overflowed && closes_fence(&line, fence, run) {
                self.fence = None;
            }
            return;
        }
        if blank {
            if std::mem::take(&mut self.block_has_content) {
                if std::mem::take(&mut self.block_has_list) {
                    self.pending = Some(offset);
                } else {
                    self.boundary = Some(offset);
                }
            }
            return;
        }
        self.block_has_content = true;
        if list_marker(&line, true) == Some(true) {
            self.block_has_list = true;
        }
        if let Some(fence) = opens_fence(&line) {
            self.fence = Some(fence);
        }
    }

    /// Settle a pending boundary once the next line says whether it continues
    /// the list. `complete` is whether that line has ended.
    fn decide_pending(&mut self, complete: bool) {
        let Some(at) = self.pending else {
            return;
        };
        let continues = if self.line.starts_with([' ', '\t']) {
            Some(true)
        } else {
            list_marker(&self.line, complete)
        };
        match continues {
            Some(true) => {
                self.pending = None;
                self.block_has_list = true;
            }
            Some(false) => {
                self.pending = None;
                self.boundary = Some(at);
            }
            None => {}
        }
    }

    /// The latest boundary at least `min` bytes in, rebasing what remains to
    /// the buffer that will be left once the prefix is flushed.
    fn take_boundary(&mut self, min: usize) -> Option<usize> {
        let at = self.boundary.filter(|at| *at >= min)?;
        self.boundary = None;
        self.pending = self.pending.map(|pending| pending.saturating_sub(at));
        Some(at)
    }

    /// The whole buffer was flushed: no offset means anything now, but the
    /// fence, the block and the line being streamed carry on.
    fn forget_offsets(&mut self) {
        self.boundary = None;
        self.pending = None;
    }
}

/// Whether `line` starts with a list marker (`-`, `*`, `+`, or up to nine
/// digits and `.` or `)`, then whitespace or the line's end). `None` while
/// the streamed head could still go either way.
fn list_marker(line: &str, complete: bool) -> Option<bool> {
    let line = line.trim_start_matches([' ', '\t']);
    let mut chars = line.chars();
    let after_marker = match chars.next() {
        None => return if complete { Some(false) } else { None },
        Some('-' | '*' | '+') => chars.next(),
        Some(first) if first.is_ascii_digit() => {
            let mut digits = 1;
            loop {
                match chars.next() {
                    Some(ch) if ch.is_ascii_digit() && digits < 9 => digits += 1,
                    Some('.' | ')') => break chars.next(),
                    None => return if complete { Some(false) } else { None },
                    Some(_) => return Some(false),
                }
            }
        }
        Some(_) => return Some(false),
    };
    match after_marker {
        None => {
            if complete {
                Some(true)
            } else {
                None
            }
        }
        Some(ch) => Some(ch == ' ' || ch == '\t'),
    }
}

/// The fence a line opens: three or more backticks or tildes after any
/// indentation (a fence inside a list item is indented).
fn opens_fence(line: &str) -> Option<(char, usize)> {
    let line = line.trim_start();
    let fence = line.chars().next().filter(|ch| matches!(ch, '`' | '~'))?;
    let run = line.chars().take_while(|ch| *ch == fence).count();
    (run >= 3).then_some((fence, run))
}

/// Whether `line` closes a fence of `run` `fence` characters: at least as
/// many of the same character and nothing else.
fn closes_fence(line: &str, fence: char, run: usize) -> bool {
    let line = line.trim();
    line.len() >= run && line.chars().all(|ch| ch == fence)
}

/// What the owning call's frames have said about the subagent it spawned.
///
/// Kept apart from the payload half of [`ToolMemo`] because it is read at the
/// terminal result, after [`ToolMemo::shed_payload`] would have dropped the
/// input it came from.
#[derive(Debug, Default)]
struct SpawnFacts {
    /// The call is a Task/Agent spawn: the adapter named the tool `Agent` or
    /// `Task`, or the input carried a `subagent_type`.
    is_spawn: bool,
    subagent_type: Option<String>,
    /// The `model` the call's input asked for. The Agent tool's `model`
    /// argument overrides the subagent definition's own, so when present it
    /// is the model the subagent ran on; when absent nothing on the wire says
    /// which model that was, and none is published.
    model: Option<String>,
}

impl SpawnFacts {
    fn absorb(&mut self, update: &Value) {
        let named_spawn = [
            update.pointer("/_meta/claudeCode/toolName"),
            update.get("name"),
            update.get("toolName"),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|name| matches!(name, "Agent" | "Task"));
        let input = tool_input(update);
        let subagent_type = string_field(&input, "subagent_type");
        if named_spawn || subagent_type.is_some() {
            self.is_spawn = true;
        }
        if subagent_type.is_some() {
            self.subagent_type = subagent_type;
        }
        if self.is_spawn {
            if let Some(model) = string_field(&input, "model") {
                self.model = Some(model);
            }
        }
    }

    /// The `subagent` object for the owning call's `tool_result`, from what
    /// was observed and nothing else; `None` when nothing was.
    ///
    /// The run totals are read off the terminal frame's `rawOutput` when the
    /// adapter puts the structured Agent output there. claude-agent-acp
    /// 0.84.0 does not for a non-AIR client (`tool-calls/renderer.js:244-252`
    /// sends `rawOutput` only for ExitPlanMode, and the Agent reporter
    /// renders the report text without its `<usage>` trailer,
    /// `tool-calls/reporters/agent.js:12-31`), so today these are absent and
    /// stay absent rather than being estimated.
    fn object(&self, terminal: &Value) -> Option<Value> {
        if !self.is_spawn {
            return None;
        }
        let mut object = Map::new();
        if let Some(kind) = &self.subagent_type {
            object.insert("type".into(), json!(kind));
        }
        if let Some(model) = &self.model {
            object.insert("model".into(), json!(model));
        }
        if let Some(output) = terminal.get("rawOutput").filter(|v| v.is_object()) {
            for (key, aliases) in [
                ("totalTokens", &["totalTokens", "total_tokens"][..]),
                (
                    "durationMs",
                    &["totalDurationMs", "durationMs", "duration_ms"][..],
                ),
                (
                    "toolUseCount",
                    &["totalToolUseCount", "toolUseCount", "tool_uses"][..],
                ),
            ] {
                if let Some(value) = aliases
                    .iter()
                    .find_map(|alias| output.get(*alias).and_then(Value::as_u64))
                {
                    object.insert(key.into(), json!(value));
                }
            }
        }
        (!object.is_empty()).then_some(Value::Object(object))
    }
}

/// An append-only terminal stream with a bounded prefix and a digest of every
/// omitted byte. Reserving marker space once avoids retaining the full output
/// just to compute an honest truncation digest at completion.
#[derive(Debug, Default)]
struct TerminalOutput {
    prefix: String,
    omitted: Option<(usize, Sha256)>,
    /// Every byte the stream carried, counted and hashed whole, so the
    /// provider can later check it against the agent's own record of the
    /// command without having retained it.
    total: usize,
    whole: Sha256,
}

impl TerminalOutput {
    fn append(&mut self, data: &str) {
        self.total = self.total.saturating_add(data.len());
        self.whole.update(data.as_bytes());
        if let Some((bytes, hash)) = &mut self.omitted {
            *bytes = bytes.saturating_add(data.len());
            hash.update(data.as_bytes());
            return;
        }
        if data.len() <= MAX_TOOL_CONTENT_BYTES.saturating_sub(self.prefix.len()) {
            self.prefix.push_str(data);
            return;
        }
        // 128 exceeds the longest marker (usize decimal count + SHA-256).
        let retained_limit = MAX_TOOL_CONTENT_BYTES - 128;
        let mut hash = Sha256::new();
        let mut omitted = 0;
        if self.prefix.len() > retained_limit {
            let keep = floor_boundary(&self.prefix, retained_limit);
            let tail = self.prefix.split_off(keep);
            hash.update(tail.as_bytes());
            omitted += tail.len();
        } else {
            let keep = floor_boundary(data, retained_limit - self.prefix.len());
            self.prefix.push_str(&data[..keep]);
            let tail = &data[keep..];
            hash.update(tail.as_bytes());
            self.omitted = Some((tail.len(), hash));
            return;
        }
        hash.update(data.as_bytes());
        omitted += data.len();
        self.omitted = Some((omitted, hash));
    }

    fn finish(self) -> (String, StreamedOutput) {
        let streamed = StreamedOutput {
            bytes: self.total,
            sha256: self.whole.finalize().into(),
        };
        let text = match self.omitted {
            Some((bytes, hash)) => format!(
                "{}…[elided {bytes} bytes, sha256:{}]",
                self.prefix,
                hex::encode(hash.finalize())
            ),
            None => self.prefix,
        };
        (text, streamed)
    }
}

/// The size and digest of everything one call's output stream carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamedOutput {
    /// Bytes the stream carried, before any elision.
    pub bytes: usize,
    /// SHA-256 of those bytes, in order.
    pub sha256: [u8; 32],
}

/// A `tool_result` whose content was assembled only from streamed output
/// chunks, which the provider may verify against the agent's own record of
/// the command (see the module docs, "Command output completeness").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamCheck {
    /// The call the result belongs to.
    pub tool_id: String,
    /// What the stream carried.
    pub streamed: StreamedOutput,
}

/// What the adapter has told us about one open tool call so far.
///
/// ACP delivers a tool call in pieces: the opening `tool_call` frame often
/// carries only a placeholder title and an empty `rawInput`, and the arguments,
/// the `locations` and the diff blocks arrive on later `tool_call_update`s —
/// including non-terminal ones, which publish nothing of their own. Without a
/// memo those frames were simply discarded, which is how every edit in the
/// 2026-08-29 walk reached the wire as `"input": {}`.
#[derive(Debug, Default)]
struct ToolMemo {
    name: Option<String>,
    kind: Option<String>,
    input: Option<Value>,
    paths: Vec<String>,
    changes: Vec<ToolEditChange>,
    spawn: SpawnFacts,
    terminal_id: Option<String>,
    terminal_output: Option<TerminalOutput>,
    exit_code: Option<i64>,
    output_closed: bool,
}

impl ToolMemo {
    /// Fold one frame's tool fields in. Later frames win on the scalar fields
    /// — the adapter is correcting itself — while paths and changes accumulate,
    /// because a call can touch more than one file.
    fn absorb(&mut self, update: &Value) {
        if update.get("sessionUpdate").and_then(Value::as_str) == Some("tool_call") {
            // Only the opening frame declares a terminal association. Later
            // chunks cannot rebind a call to a different terminal.
            self.terminal_id = update
                .pointer("/_meta/terminal_info/terminal_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            self.terminal_output = None;
            self.exit_code = None;
            self.output_closed = false;
        }
        self.spawn.absorb(update);
        self.absorb_terminal(update);
        if let Some(name) =
            string_field(update, "toolName").or_else(|| string_field(update, "title"))
        {
            self.name = Some(name);
        }
        if let Some(kind) = string_field(update, "kind") {
            self.kind = Some(kind);
        }
        let input = tool_input(update);
        if input.as_object().is_some_and(|object| !object.is_empty()) {
            self.input = Some(input);
        }
        for path in update
            .get("locations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|location| string_field(location, "path"))
        {
            self.paths.push(path);
        }
        for block in update
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if block.get("type").and_then(Value::as_str) != Some("diff") {
                continue;
            }
            let path = string_field(block, "path");
            if let Some(path) = path.clone() {
                self.paths.push(path);
            }
            self.changes.push(ToolEditChange {
                path,
                old_text: block
                    .get("oldText")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                new_text: block
                    .get("newText")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
    }

    /// codex-acp 2.0.1 sends output as metadata deltas, then deliberately
    /// omits terminal content. Both its delta and Zed-compatible chunk keys
    /// append, not replace. Bind their terminal id to this exact call; never
    /// let malformed metadata attach another terminal's output or exit.
    fn absorb_terminal(&mut self, update: &Value) {
        if self.output_closed {
            return;
        }
        let Some(id) = update
            .get("toolCallId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        // Some adapters declare a terminal id distinct from the tool id.
        // Without an opening declaration, accept only the tool id itself.
        let terminal_id = self.terminal_id.as_deref().unwrap_or(id);
        let matching =
            |value: &Value| value.get("terminal_id").and_then(Value::as_str) == Some(terminal_id);
        if let Some(output) = update
            .pointer("/_meta/terminal_output_delta")
            .or_else(|| update.pointer("/_meta/terminal_output"))
            .filter(|value| matching(value))
        {
            if let Some(data) = output.get("data").and_then(Value::as_str) {
                self.terminal_output
                    .get_or_insert_with(TerminalOutput::default)
                    .append(data);
            }
        }
        if let Some(exit) = update
            .pointer("/_meta/terminal_exit")
            .filter(|value| matching(value))
        {
            self.exit_code = exit.get("exit_code").and_then(Value::as_i64);
        } else if let Some(output) = update.get("rawOutput").and_then(Value::as_object) {
            // Non-terminal command completion in the same adapter. A random
            // MCP result containing an exit_code business field is not this
            // single-field command envelope.
            if output.len() == 1
                && matches!(self.kind.as_deref(), Some("execute" | "read" | "search"))
                && update
                    .pointer("/_meta/is_mcp_tool_call")
                    .and_then(Value::as_bool)
                    != Some(true)
            {
                self.exit_code = output.get("exit_code").and_then(Value::as_i64);
            }
        }
    }

    /// Whether the adapter has said what this call acts on: its arguments, or
    /// the files and diffs an edit carries in place of them.
    fn has_arguments(&self) -> bool {
        self.input
            .as_ref()
            .is_some_and(|input| input.as_object().is_some_and(|o| !o.is_empty()))
            || !self.paths.is_empty()
            || !self.changes.is_empty()
    }

    /// The display name, falling back to whatever the frame in hand offers.
    fn display_name(&self, update: &Value) -> String {
        self.name.clone().unwrap_or_else(|| tool_name(update))
    }

    /// Forget the bulky half once the call is closed.
    ///
    /// The name and the discriminant stay for the rest of the turn, so a second
    /// terminal frame for the same call cannot publish an unlabelled result;
    /// the input and the diff texts go, so a long turn's memo cannot grow with
    /// every file the agent touched.
    fn shed_payload(&mut self) {
        self.input = None;
        self.paths = Vec::new();
        self.changes = Vec::new();
        self.terminal_id = None;
        self.terminal_output = None;
        self.exit_code = None;
        self.output_closed = true;
    }
}

impl TranscriptTranslator {
    /// A translator for one session generation.
    pub fn new(include_thoughts: bool) -> Self {
        Self {
            include_thoughts,
            text: String::new(),
            paragraph_flush: false,
            paragraphs: ParagraphScanner::default(),
            thoughts: String::new(),
            usage: None,
            tool_calls: 0,
            tools: HashMap::new(),
            anonymous: ToolMemo::default(),
            held: Vec::new(),
            subagents: Vec::new(),
            stream_checks: Vec::new(),
        }
    }

    /// Publish the agent's prose a paragraph at a time as well as at the size,
    /// narrative and turn boundaries. Off unless asked for: with it off the
    /// item sequence is exactly what it was before the paragraph boundary
    /// existed. See `Config::transcript_paragraph_flush` for who it is unsafe
    /// for.
    #[must_use]
    pub fn with_paragraph_flush(mut self, paragraph_flush: bool) -> Self {
        self.paragraph_flush = paragraph_flush;
        self
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
    ///
    /// `attachment_count` is how many images were actually **delivered** with
    /// this turn, not how many the operator attached: a turn whose images were
    /// dropped for an execution that cannot take them must not leave a record
    /// claiming the agent saw them.
    pub fn begin_turn(
        &mut self,
        prompt: &str,
        operator_pubkey: Option<&str>,
        command_id: Option<&str>,
        sender_role: Option<&str>,
        attachment_count: usize,
    ) -> Vec<Value> {
        self.text.clear();
        self.paragraphs = ParagraphScanner::default();
        self.thoughts.clear();
        self.subagents.clear();
        self.usage = None;
        self.tool_calls = 0;
        vec![crate::payload::user_prompt_item(
            prompt,
            false,
            operator_pubkey,
            command_id,
            sender_role,
            attachment_count,
        )]
    }

    /// Translate one `params.update` object.
    pub fn on_update(&mut self, update: &Value) -> Vec<Value> {
        let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
            return Vec::new();
        };
        if let Some(parent) = subagent_parent(update) {
            // Buzz declares the `subagent-transcript` client capability, so
            // claude-agent-acp stops stripping a subagent's prose and thinking
            // and sends those frames stamped with the Task/Agent call that owns
            // them. They are published, attributed, rather than dropped
            // (ledger 308): a seat may now use subagents, and work the record
            // cannot show is work nobody can read, cite or replay.
            return self.on_subagent_update(kind, &parent, update);
        }
        if USAGE_UPDATE_VARIANTS.contains(&kind) {
            // Only the last one per turn is published: a running counter emitted
            // per token is noise, and the final value is the one that is true.
            self.usage = Some(usage_object(update));
            return Vec::new();
        }
        match kind {
            "agent_message_chunk" => {
                let chunk = content_text(update.get("content"));
                if self.paragraph_flush {
                    self.paragraphs.feed(&chunk, self.text.len());
                }
                self.text.push_str(&chunk);
                if self.text.len() >= COALESCE_FLUSH_BYTES {
                    let mut items = self.release_held_in(None);
                    // The cut lands wherever the size does, possibly inside
                    // a fence or mid-line; the scanner keeps reading the text
                    // that continues it.
                    self.paragraphs.forget_offsets();
                    items.extend(self.take_text());
                    return items;
                }
                // A call of the agent's still waiting for its arguments
                // defers the paragraph: publishing prose releases held calls
                // first, and a paragraph is far more frequent than the size
                // cut, so flushing here would put argument-less "Terminal"
                // rows back on the wire — the very row the hold exists to
                // prevent. The boundary is kept; the first chunk after the
                // arguments land (or the next narrative boundary) publishes it.
                if self.held.iter().any(|call| call.scope.is_none()) {
                    return Vec::new();
                }
                match self.paragraphs.take_boundary(MIN_PARAGRAPH_FLUSH_BYTES) {
                    Some(at) => self.flush_paragraphs(at),
                    None => Vec::new(),
                }
            }
            "agent_thought_chunk" => {
                if !self.include_thoughts {
                    return Vec::new();
                }
                self.thoughts.push_str(&content_text(update.get("content")));
                if self.thoughts.len() >= COALESCE_FLUSH_BYTES {
                    let mut items = self.release_held_in(None);
                    items.extend(self.flush_thoughts());
                    return items;
                }
                Vec::new()
            }
            "tool_call" => {
                let mut items = self.flush_all();
                // Counted here rather than at the result, because a call that
                // never returns still consumed a call's worth of context.
                self.tool_calls = self.tool_calls.saturating_add(1);
                items.extend(self.hold_or_publish(update, None));
                items
            }
            "tool_call_update" => match terminal_status(update) {
                None => {
                    // Where claude-agent-acp fills in the arguments, the
                    // `locations` and the diff blocks it had not finished
                    // streaming when the call opened. Dropping the frame whole
                    // is what stripped every edit payload. A call held for its
                    // arguments is published now that it has them.
                    let tool_id = string_field(update, "toolCallId").unwrap_or_default();
                    if self.absorb(update).has_arguments() {
                        self.release_through(&tool_id)
                    } else {
                        Vec::new()
                    }
                }
                Some(status) => {
                    let mut items = self.flush_all();
                    items.extend(self.close_owned_subagent(update));
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

    /// Translate one frame a subagent produced, attributed to `parent`.
    ///
    /// The same item kinds the agent's own frames become, each stamped with
    /// `parentToolId`. Two things differ, both on purpose:
    ///
    /// - prose coalesces in the subagent's own buffer, and a subagent's tool
    ///   call or result flushes only that buffer — the agent's prose and a
    ///   sibling subagent's are not at a narrative boundary just because this
    ///   subagent reached for a tool;
    /// - its calls are not counted in [`tool_calls`](Self::tool_calls), and
    ///   its usage frames are not the turn's usage: both describe the
    ///   subagent's own context window, not the agent's.
    fn on_subagent_update(&mut self, kind: &str, parent: &str, update: &Value) -> Vec<Value> {
        match kind {
            "agent_message_chunk" => {
                let full = {
                    let buffers = self.subagent_buffers(parent);
                    buffers.text.push_str(&content_text(update.get("content")));
                    buffers.text.len() >= COALESCE_FLUSH_BYTES
                };
                if full {
                    let mut items = self.release_held_in(Some(parent));
                    items.extend(self.take_subagent_prose(parent, true, false));
                    return items;
                }
                Vec::new()
            }
            "agent_thought_chunk" => {
                if !self.include_thoughts {
                    return Vec::new();
                }
                let full = {
                    let buffers = self.subagent_buffers(parent);
                    buffers
                        .thoughts
                        .push_str(&content_text(update.get("content")));
                    buffers.thoughts.len() >= COALESCE_FLUSH_BYTES
                };
                if full {
                    let mut items = self.release_held_in(Some(parent));
                    items.extend(self.take_subagent_prose(parent, false, true));
                    return items;
                }
                Vec::new()
            }
            "tool_call" => {
                let mut items = self.flush_subagent(parent);
                items.extend(self.hold_or_publish(update, Some(parent)));
                items
            }
            "tool_call_update" => match terminal_status(update) {
                None => {
                    let tool_id = string_field(update, "toolCallId").unwrap_or_default();
                    if self.absorb(update).has_arguments() {
                        self.release_through(&tool_id)
                    } else {
                        Vec::new()
                    }
                }
                Some(status) => {
                    let mut items = self.flush_subagent(parent);
                    items.extend(self.close_owned_subagent(update));
                    items.push(self.tool_result_item(update, status));
                    items
                }
            },
            "plan" => {
                let mut items = self.flush_subagent(parent);
                let mut item = plan_item(update);
                stamp_parent(&mut item, Some(parent));
                items.push(item);
                items
            }
            _ => Vec::new(),
        }
    }

    /// The stream-assembled results produced since the last call, for the
    /// provider to verify against the agent's own record. Each names a
    /// `tool_result` item already returned, marked `outputComplete: false`.
    pub(crate) fn take_stream_checks(&mut self) -> Vec<StreamCheck> {
        std::mem::take(&mut self.stream_checks)
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
        // Every subagent's buffer, oldest first, and any of its calls still
        // held for arguments that never came: nothing a subagent said may be
        // left unpublished when the turn closes.
        let parents: Vec<String> = self.subagents.iter().map(|(id, _)| id.clone()).collect();
        for parent in parents {
            items.extend(self.flush_subagent(&parent));
        }
        let held = std::mem::take(&mut self.held);
        items.extend(held.iter().map(|call| self.render_tool_call(&call.frame)));
        if let Some(usage) = self.usage.take() {
            items.push(json!({ "kind": "context_window_updated", "usage": usage }));
        }
        self.tools.clear();
        self.anonymous = ToolMemo::default();
        items
    }

    /// [`close_turn`](Self::close_turn) followed by the terminal item.
    pub fn end_turn(&mut self, result: Value) -> Vec<Value> {
        let mut items = self.close_turn();
        items.push(result);
        items
    }

    /// Emit the agent's own buffered prose and held calls without closing the
    /// turn.
    ///
    /// Subagent buffers are deliberately not included: the agent reaching a
    /// narrative boundary says nothing about where a subagent running beside
    /// it is. They flush on their own boundaries ([`on_update`](Self::on_update))
    /// and at [`close_turn`](Self::close_turn).
    pub fn flush_all(&mut self) -> Vec<Value> {
        let mut items = self.release_held_in(None);
        items.extend(self.flush_text());
        items.extend(self.flush_thoughts());
        items
    }

    /// Publish the agent's prose up to `at`, a paragraph boundary, keeping
    /// the rest buffered.
    ///
    /// Never reached with one of the agent's calls held (the caller defers),
    /// so the release below is a no-op kept for the hold's contract. Buffered
    /// thinking goes first:
    /// the agent thinks before it writes, so what is buffered was written
    /// before this paragraph, and left for the next narrative boundary it
    /// would surface several paragraphs after the answer it led to.
    fn flush_paragraphs(&mut self, at: usize) -> Vec<Value> {
        let mut items = self.release_held_in(None);
        items.extend(self.flush_thoughts());
        let rest = self.text.split_off(at);
        let text = std::mem::replace(&mut self.text, rest);
        if !text.trim().is_empty() {
            items.push(json!({ "kind": "assistant_text", "text": text }));
        }
        items
    }

    /// Publish all of the agent's prose at a narrative or turn boundary.
    ///
    /// What follows a tool call or a plan is a new block to every reader, so
    /// the paragraph scanner starts over: a fence the prose before it left
    /// open does not swallow the paragraph breaks after it.
    fn flush_text(&mut self) -> Vec<Value> {
        self.paragraphs = ParagraphScanner::default();
        self.take_text()
    }

    fn take_text(&mut self) -> Vec<Value> {
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

    /// Fold one frame into the memo for the call it names, and return it.
    ///
    /// A frame with no `toolCallId` — which ACP does not allow, but a sloppy
    /// adapter can still send — has nothing to pair across frames, so it gets a
    /// memo rebuilt from that frame alone. Sharing one bucket between every
    /// anonymous call would let one call's path be published on another's.
    fn absorb(&mut self, update: &Value) -> &mut ToolMemo {
        let tool_id = string_field(update, "toolCallId").unwrap_or_default();
        if tool_id.is_empty() {
            self.anonymous = ToolMemo::default();
            self.anonymous.absorb(update);
            return &mut self.anonymous;
        }
        let memo = self.tools.entry(tool_id).or_default();
        memo.absorb(update);
        memo
    }

    /// Publish an opening `tool_call`, or hold it until its arguments arrive.
    ///
    /// claude-agent-acp opens every call at the start of the model's tool-use
    /// block, before the arguments have streamed, so the frame says only
    /// `"Terminal"` with an empty input; the complete input follows on a
    /// non-terminal `tool_call_update` once the block ends, before the tool
    /// runs. Published as it came, the row a person watches reads "Run
    /// Terminal" for as long as the command runs, and names the command only
    /// once it is over. So a call with no arguments is held — for the length
    /// of the streaming, not the run — and published, once, with them.
    ///
    /// Held calls never go missing: anything else this translator publishes in
    /// the same scope (text, a later call, a result) releases them first, in
    /// order, as they are, and the end of the turn releases every scope.
    ///
    /// `scope` is the owning Task/Agent call for a subagent's call, `None` for
    /// the agent's own; see [`HeldCall`] for why holding is per scope.
    fn hold_or_publish(&mut self, update: &Value, scope: Option<&str>) -> Vec<Value> {
        let tool_id = string_field(update, "toolCallId").unwrap_or_default();
        let has_arguments = self.absorb(update).has_arguments();
        if tool_id.is_empty() || has_arguments {
            return vec![self.render_tool_call(update)];
        }
        self.held.push(HeldCall {
            id: tool_id,
            scope: scope.map(str::to_owned),
            frame: update.clone(),
        });
        Vec::new()
    }

    /// Publish every call held in `scope`, oldest first, leaving other scopes'
    /// held calls where they are.
    fn release_held_in(&mut self, scope: Option<&str>) -> Vec<Value> {
        let (released, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|call| call.scope.as_deref() == scope);
        self.held = kept;
        released
            .iter()
            .map(|call| self.render_tool_call(&call.frame))
            .collect()
    }

    /// Publish held calls of `tool_id`'s scope up to and including it, keeping
    /// their order.
    fn release_through(&mut self, tool_id: &str) -> Vec<Value> {
        let Some(position) = self.held.iter().position(|call| call.id == tool_id) else {
            return Vec::new();
        };
        let scope = self.held[position].scope.clone();
        let mut released = Vec::new();
        let mut kept = Vec::new();
        for (index, call) in std::mem::take(&mut self.held).into_iter().enumerate() {
            if index <= position && call.scope == scope {
                released.push(call);
            } else {
                kept.push(call);
            }
        }
        self.held = kept;
        released
            .iter()
            .map(|call| self.render_tool_call(&call.frame))
            .collect()
    }

    /// The prose buffers for the subagent owned by `parent`, opened on first
    /// use.
    fn subagent_buffers(&mut self, parent: &str) -> &mut ProseBuffers {
        let index = match self.subagents.iter().position(|(id, _)| id == parent) {
            Some(index) => index,
            None => {
                self.subagents
                    .push((parent.to_owned(), ProseBuffers::default()));
                self.subagents.len() - 1
            }
        };
        &mut self.subagents[index].1
    }

    /// Take a subagent's buffered prose as attributed items.
    fn take_subagent_prose(&mut self, parent: &str, text: bool, thoughts: bool) -> Vec<Value> {
        let Some(index) = self.subagents.iter().position(|(id, _)| id == parent) else {
            return Vec::new();
        };
        let buffers = &mut self.subagents[index].1;
        let mut items = Vec::new();
        if text {
            let taken = std::mem::take(&mut buffers.text);
            if !taken.trim().is_empty() {
                items.push(
                    json!({ "kind": "assistant_text", "text": taken, "parentToolId": parent }),
                );
            }
        }
        if thoughts {
            let taken = std::mem::take(&mut buffers.thoughts);
            if !taken.trim().is_empty() {
                items.push(json!({ "kind": "reasoning", "text": taken, "parentToolId": parent }));
            }
        }
        if buffers.text.is_empty() && buffers.thoughts.is_empty() {
            self.subagents.remove(index);
        }
        items
    }

    /// Everything a subagent has pending: its held calls, then its prose.
    fn flush_subagent(&mut self, parent: &str) -> Vec<Value> {
        let mut items = self.release_held_in(Some(parent));
        items.extend(self.take_subagent_prose(parent, true, true));
        items
    }

    /// Flush the subagent `update` owns, ahead of the owning call's result.
    ///
    /// A no-op unless the terminal frame is for a call that spawned a
    /// subagent with something still buffered.
    fn close_owned_subagent(&mut self, update: &Value) -> Vec<Value> {
        let tool_id = string_field(update, "toolCallId").unwrap_or_default();
        if tool_id.is_empty() {
            return Vec::new();
        }
        self.flush_subagent(&tool_id)
    }

    /// The `tool_call` item for a call whose frames are already absorbed.
    fn render_tool_call(&mut self, update: &Value) -> Value {
        let tool_id = string_field(update, "toolCallId").unwrap_or_default();
        let memo = if tool_id.is_empty() {
            &mut self.anonymous
        } else {
            self.tools.entry(tool_id.clone()).or_default()
        };
        let tool_name = memo.display_name(update);
        let tool_kind = memo.kind.clone();
        let input = memo.input.clone().unwrap_or_else(|| json!({}));
        let edit = tool_edit_payload(&memo.paths, &memo.changes);

        let mut tool = Map::new();
        tool.insert("toolName".into(), json!(tool_name));
        tool.insert("toolId".into(), json!(tool_id));
        tool.insert("input".into(), bounded_input(input));
        // ACP's `kind` is a *discriminant* ("read", "execute", "think"), not a
        // name, and it is optional in the spec. It gets its own key so a tool
        // that also sent a `title` cannot suppress it — and it is written only
        // when the adapter actually sent one, because an invented discriminant
        // is worse than an absent one.
        if let Some(kind) = tool_kind {
            tool.insert("toolKind".into(), Value::String(kind));
        }
        if let Some(edit) = edit {
            tool.insert("edit".into(), edit);
        }
        let mut item = json!({ "kind": "tool_call", "tool": Value::Object(tool) });
        // Beside `tool`, not inside it: the attribution is a fact about where
        // the call sits in the record, not about the call.
        stamp_parent(&mut item, subagent_parent(update).as_deref());
        item
    }

    fn tool_result_item(&mut self, update: &Value, status: &str) -> Value {
        let tool_id = string_field(update, "toolCallId").unwrap_or_default();
        let memo = self.absorb(update);
        let tool_name = memo.display_name(update);
        // The pairing is what carries the discriminant onto the result: a
        // `tool_call_update` rarely repeats `kind`, so it is recalled from the
        // opening call and only read off the update as a fallback.
        let tool_kind = memo.kind.clone();
        let input = memo.input.clone();
        let edit = tool_edit_payload(&memo.paths, &memo.changes);
        let subagent = memo.spawn.object(update);
        let streamed = memo.terminal_output.take().map(TerminalOutput::finish);
        let exit_code = memo.exit_code;
        memo.shed_payload();

        // A final text snapshot replaces, never appends to, streamed bytes.
        // Commands with only a numeric rawOutput still use their stream.
        let content = content_text(update.get("content"));
        let mut stream_only = None;
        let content = if !content.is_empty() {
            content
        } else if let Some((text, streamed)) = streamed {
            stream_only = Some(streamed);
            text
        } else {
            raw_output_text(update)
        };
        let mut item = Map::new();
        item.insert("kind".into(), json!("tool_result"));
        item.insert("toolId".into(), json!(tool_id));
        item.insert("toolName".into(), json!(tool_name));
        if let Some(kind) = tool_kind {
            item.insert("toolKind".into(), Value::String(kind));
        }
        // The arguments the adapter finished streaming after the opening frame.
        // The consumer already reads `input` off a result; the provider simply
        // never sent one, so an edit arrived with nothing to name its file.
        if let Some(input) = input {
            item.insert("input".into(), bounded_input(input));
        }
        if let Some(edit) = edit {
            item.insert("edit".into(), edit);
        }
        item.insert(
            "content".into(),
            json!(bound_text(&content, MAX_TOOL_CONTENT_BYTES)),
        );
        item.insert("isError".into(), json!(status == "failed"));
        if let Some(exit_code) = exit_code {
            item.insert("exitCode".into(), json!(exit_code));
        }
        if let Some(subagent) = subagent {
            item.insert("subagent".into(), subagent);
        }
        if let Some(streamed) = stream_only {
            // Unverified until the provider checks it: an adapter can stream
            // only part of a command's output and say nothing at completion.
            item.insert("contentSource".into(), json!(CONTENT_SOURCE_STREAMED));
            item.insert("outputComplete".into(), json!(false));
            item.insert(
                "outputGap".into(),
                json!({ "streamedBytes": streamed.bytes }),
            );
            self.stream_checks.push(StreamCheck {
                tool_id: tool_id.clone(),
                streamed,
            });
        }
        let mut item = Value::Object(item);
        stamp_parent(&mut item, subagent_parent(update).as_deref());
        item
    }
}

/// Attribute `item` to the Task/Agent call that owns it, when one does.
fn stamp_parent(item: &mut Value, parent: Option<&str>) {
    if let (Some(parent), Some(object)) = (parent, item.as_object_mut()) {
        object.insert("parentToolId".into(), json!(parent));
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

/// The Task/Agent call that owns this update, when it describes a subagent's
/// work rather than the agent's own.
///
/// claude-agent-acp stamps `_meta.claudeCode.parentToolUseId` onto every frame
/// produced inside a Task/Agent subagent, naming the tool call that spawned it
/// (`stampParentToolUseId`, `dist/acp-agent.js:483` in 0.84.0), and uses the
/// same field to decide what counts as the turn's own answer. `_meta` rides on
/// the `update` object itself, not on the notification's `params`. Only a
/// present, non-empty id counts, so an adapter that sends `_meta` without one
/// cannot silently move the agent's own work under a spawn.
fn subagent_parent(update: &Value) -> Option<String> {
    update
        .pointer("/_meta/claudeCode/parentToolUseId")
        .and_then(Value::as_str)
        .filter(|parent| !parent.is_empty())
        .map(str::to_owned)
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
///
/// When the budget cannot hold a complete elision marker, return an empty
/// string: a partial marker would imply digest evidence that is unavailable,
/// and exceeding the budget would break the enclosing item's size limit.
pub fn bound_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut keep = floor_boundary(text, max_bytes);
    loop {
        // The marker itself displaces source bytes. Hash the actual suffix
        // after making room, not the suffix at the original byte ceiling.
        let marker = elision_marker(&text[keep..]);
        if keep + marker.len() <= max_bytes {
            return format!("{}{marker}", &text[..keep]);
        }
        if keep == 0 {
            return String::new();
        }
        keep = floor_boundary(text, max_bytes.saturating_sub(marker.len()));
    }
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
#[path = "transcript_paragraph_tests.rs"]
mod paragraph_tests;

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

    /// The `tool_result` among `items`; a call held for its arguments is
    /// released just ahead of it.
    fn result_of(items: &[Value]) -> &Value {
        items
            .iter()
            .find(|item| item["kind"] == "tool_result")
            .expect("a tool_result")
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
        let _ = translator.begin_turn("do the thing", None, None, None, 0);
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
        let _ = translator.begin_turn("first", None, None, None, 0);
        let _ = translator.on_update(&tool_call("t1", "Read", json!({})));
        let _ = translator.close_turn();
        let _ = translator.begin_turn("second", None, None, None, 0);
        assert_eq!(translator.tool_calls(), 0);
    }

    /// The golden case: one prompt, some prose, a tool call and its result,
    /// more prose, then the terminal item — in that exact order.
    #[test]
    fn a_whole_turn_translates_to_the_expected_item_sequence() {
        let mut translator = TranscriptTranslator::new(true);
        let mut items = translator.begin_turn("do the thing", None, None, None, 0);
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
        let items = translator.on_update(&tool_call("t1", "bash", json!({ "command": "ls" })));
        assert_eq!(kinds(&items), vec!["assistant_text", "tool_call"]);
    }

    /// claude-agent-acp opens a shell call as `"Terminal"` with no command and
    /// sends the command a moment later, before it runs. Published as it came,
    /// the running row said "Run Terminal" for the whole run (2026-10-01). The
    /// call is held for that moment and published once, with its command.
    #[test]
    fn a_call_opened_without_arguments_is_published_once_they_arrive() {
        let mut translator = TranscriptTranslator::new(true);
        let opened = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Terminal",
            "kind": "execute",
            "status": "pending",
            "rawInput": {},
        })));
        assert!(opened.is_empty(), "{opened:?}");

        let filled = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "title": "docker compose up -d",
            "rawInput": { "command": "docker compose up -d" },
        })));
        assert_eq!(kinds(&filled), vec!["tool_call"]);
        assert_eq!(filled[0]["tool"]["toolId"], "t1");
        assert_eq!(filled[0]["tool"]["toolName"], "docker compose up -d");
        assert_eq!(
            filled[0]["tool"]["input"]["command"],
            "docker compose up -d"
        );
        assert_eq!(filled[0]["tool"]["toolKind"], "execute");

        // Published once: a later refinement adds nothing to the record.
        let again = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "rawInput": { "command": "docker compose up -d" },
        })));
        assert!(again.is_empty(), "{again:?}");
        let done = translator.on_update(&tool_done("t1", "completed", "ok"));
        assert_eq!(kinds(&done), vec!["tool_result"]);
    }

    /// A held call is never lost or reordered: whatever is published next — a
    /// result, prose, another call, the end of the turn — releases it first.
    #[test]
    fn a_held_call_is_released_before_anything_published_after_it() {
        let empty = || tool_call("t1", "Terminal", json!({}));

        let mut by_result = TranscriptTranslator::new(true);
        by_result.on_update(&empty());
        let items = by_result.on_update(&tool_done("t1", "completed", "ok"));
        assert_eq!(kinds(&items), vec!["tool_call", "tool_result"]);

        let mut by_call = TranscriptTranslator::new(true);
        by_call.on_update(&empty());
        let items = by_call.on_update(&tool_call("t2", "read", json!({ "path": "a" })));
        assert_eq!(kinds(&items), vec!["tool_call", "tool_call"]);
        assert_eq!(items[0]["tool"]["toolId"], "t1");

        let mut by_turn_end = TranscriptTranslator::new(true);
        by_turn_end.on_update(&empty());
        by_turn_end.on_update(&chunk("after"));
        assert_eq!(
            kinds(&by_turn_end.end_turn(result())),
            vec!["tool_call", "assistant_text", "result"]
        );
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

    /// A subagent-attributed `tool_call` frame, shaped as claude-agent-acp
    /// sends it for a non-AIR client.
    fn subagent_tool_call(parent: &str, id: &str, name: &str, input: Value) -> Value {
        let mut frame = tool_call(id, name, input);
        frame["_meta"]["claudeCode"]["parentToolUseId"] = json!(parent);
        frame
    }

    fn subagent_tool_done(parent: &str, id: &str, status: &str, content: &str) -> Value {
        let mut frame = tool_done(id, status, content);
        frame["_meta"]["claudeCode"]["parentToolUseId"] = json!(parent);
        frame
    }

    /// The opening frame of an `Agent` spawn, as the adapter's renderer
    /// builds it (`tool-calls/renderer.js:52-77`, title from the Agent
    /// reporter's `description`, kind `think`).
    fn agent_spawn(id: &str, input: Value) -> Value {
        update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "name": "Agent",
            "title": input.get("description").and_then(Value::as_str).unwrap_or("Task"),
            "kind": "think",
            "status": "pending",
            "rawInput": input,
            "_meta": { "claudeCode": { "toolName": "Agent" } },
        }))
    }

    fn agent_done(id: &str, status: &str, report: &str) -> Value {
        update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": status,
            "content": [{ "type": "content", "content": { "type": "text", "text": report } }],
            "_meta": { "claudeCode": { "toolName": "Agent" } },
        }))
    }

    /// Ledger 308: a subagent's prose is published, attributed to its spawn,
    /// and never concatenated into the agent's own narrative. The seam
    /// assertion still holds — the agent's two chunks meet exactly with
    /// nothing of the subagent's between them — and the subagent's words come
    /// out as their own item carrying `parentToolId`.
    #[test]
    fn a_subagents_prose_never_lands_in_the_agents_own_narrative() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&chunk("I will delegate this. "));
        assert!(
            translator
                .on_update(&subagent_chunk("t-parent", "Reading forty files…"))
                .is_empty(),
            "a subagent chunk coalesces like any other; it publishes nothing alone"
        );
        translator.on_update(&chunk("Waiting."));
        translator.on_update(&subagent_chunk("t-parent", " and forty more."));
        let items = translator.close_turn();

        assert_eq!(kinds(&items), vec!["assistant_text", "assistant_text"]);
        assert_eq!(items[0]["text"], "I will delegate this. Waiting.");
        assert!(items[0].get("parentToolId").is_none());
        assert_eq!(items[1]["text"], "Reading forty files… and forty more.");
        assert_eq!(items[1]["parentToolId"], "t-parent");
    }

    /// Two subagents running side by side keep separate buffers: one's prose
    /// never merges with the other's.
    #[test]
    fn sibling_subagents_coalesce_separately() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&subagent_chunk("spawn-a", "alpha one "));
        translator.on_update(&subagent_chunk("spawn-b", "beta one "));
        translator.on_update(&subagent_chunk("spawn-a", "alpha two"));
        translator.on_update(&subagent_chunk("spawn-b", "beta two"));
        let items = translator.close_turn();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["parentToolId"], "spawn-a");
        assert_eq!(items[0]["text"], "alpha one alpha two");
        assert_eq!(items[1]["parentToolId"], "spawn-b");
        assert_eq!(items[1]["text"], "beta one beta two");
    }

    /// Every frame kind a subagent can produce is published as the item it
    /// would be at the top level, each stamped with the owning call — never
    /// unattributed, so nothing a subagent did can pass for the agent's own.
    #[test]
    fn every_subagent_frame_kind_is_published_attributed() {
        let mut translator = TranscriptTranslator::new(true);
        let mut published = Vec::new();
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
                "rawInput": { "path": "src/lib.rs" },
                "content": { "type": "text", "text": format!("child {kind}") },
                "entries": [],
            });
            frame["_meta"]["claudeCode"]["parentToolUseId"] = json!("t-parent");
            published.extend(translator.on_update(&update(frame)));
        }
        published.extend(translator.close_turn());

        assert_eq!(
            kinds(&published),
            vec![
                "assistant_text",
                "reasoning",
                "tool_call",
                "tool_result",
                "plan"
            ]
        );
        for item in &published {
            assert_eq!(
                item["parentToolId"], "t-parent",
                "a subagent item went out unattributed: {item}"
            );
        }
        let call = &published[2];
        assert!(
            call["tool"].get("parentToolId").is_none(),
            "the attribution sits beside `tool`, not inside it"
        );
    }

    /// A subagent's buffer flushes at its own boundaries: its next tool call,
    /// and the owning call's terminal result — ahead of that result, so the
    /// record reads in the order the work happened.
    #[test]
    fn a_subagents_prose_flushes_at_its_own_call_and_at_the_spawns_result() {
        let mut translator = TranscriptTranslator::new(false);
        let spawn = translator.on_update(&agent_spawn(
            "spawn-1",
            json!({ "description": "Survey", "prompt": "look", "subagent_type": "Explore" }),
        ));
        assert_eq!(kinds(&spawn), vec!["tool_call"]);

        translator.on_update(&subagent_chunk("spawn-1", "Looking first."));
        let call = translator.on_update(&subagent_tool_call(
            "spawn-1",
            "child-1",
            "Read",
            json!({ "file_path": "src/lib.rs" }),
        ));
        assert_eq!(kinds(&call), vec!["assistant_text", "tool_call"]);
        assert_eq!(call[0]["text"], "Looking first.");
        assert_eq!(call[1]["parentToolId"], "spawn-1");

        translator.on_update(&subagent_tool_done("spawn-1", "child-1", "completed", "ok"));
        translator.on_update(&subagent_chunk("spawn-1", "Found it."));
        // The agent's own prose is not a boundary for the subagent.
        translator.on_update(&chunk("Meanwhile."));
        let lead_call = translator.on_update(&tool_call("t2", "Bash", json!({ "command": "ls" })));
        assert_eq!(kinds(&lead_call), vec!["assistant_text", "tool_call"]);
        assert!(lead_call[0].get("parentToolId").is_none());

        let done = translator.on_update(&agent_done("spawn-1", "completed", "report"));
        assert_eq!(kinds(&done), vec!["assistant_text", "tool_result"]);
        assert_eq!(done[0]["text"], "Found it.");
        assert_eq!(done[0]["parentToolId"], "spawn-1");
        assert!(
            done[1].get("parentToolId").is_none(),
            "the spawn is the agent's own call"
        );
        assert!(translator.close_turn().is_empty());
    }

    /// A subagent's thinking obeys `include_thoughts` exactly as the agent's
    /// does.
    #[test]
    fn a_subagents_thoughts_obey_include_thoughts() {
        let mut thought_frame = thought("pondering");
        thought_frame["_meta"]["claudeCode"]["parentToolUseId"] = json!("spawn-1");

        let mut without = TranscriptTranslator::new(false);
        without.on_update(&thought_frame);
        assert!(without.close_turn().is_empty());

        let mut with = TranscriptTranslator::new(true);
        with.on_update(&thought_frame);
        let items = with.close_turn();
        assert_eq!(kinds(&items), vec!["reasoning"]);
        assert_eq!(items[0]["parentToolId"], "spawn-1");
    }

    /// A subagent's prose flushes at the size boundary without merging with
    /// the agent's buffer.
    #[test]
    fn a_subagents_prose_flushes_at_the_size_boundary() {
        let mut translator = TranscriptTranslator::new(false);
        translator.on_update(&chunk("mine"));
        let big = "x".repeat(COALESCE_FLUSH_BYTES);
        let items = translator.on_update(&subagent_chunk("spawn-1", &big));
        assert_eq!(kinds(&items), vec!["assistant_text"]);
        assert_eq!(items[0]["parentToolId"], "spawn-1");
        let rest = translator.close_turn();
        assert_eq!(rest[0]["text"], "mine");
    }

    /// The hold for an argument-less opening frame (48124e695) works per call
    /// and per scope: a subagent's activity does not release the agent's own
    /// held call, which would publish an empty spawn row, and a subagent's
    /// held call is published, attributed, once its arguments arrive.
    #[test]
    fn held_calls_are_released_per_scope() {
        let mut translator = TranscriptTranslator::new(false);
        // The agent opens a second spawn whose arguments are still streaming.
        assert!(translator
            .on_update(&agent_spawn("spawn-2", json!({})))
            .is_empty());
        // A running subagent of an earlier spawn opens a call, also without
        // arguments yet, then does some talking.
        assert!(translator
            .on_update(&subagent_tool_call(
                "spawn-1",
                "child-1",
                "Terminal",
                json!({})
            ))
            .is_empty());
        let other = translator.on_update(&subagent_tool_call(
            "spawn-1",
            "child-2",
            "Read",
            json!({ "file_path": "a.rs" }),
        ));
        // The subagent's own held call goes first; the agent's stays held.
        assert_eq!(kinds(&other), vec!["tool_call", "tool_call"]);
        assert_eq!(other[0]["tool"]["toolId"], "child-1");
        assert_eq!(other[0]["parentToolId"], "spawn-1");
        assert_eq!(other[1]["tool"]["toolId"], "child-2");

        // A subagent call held, then given its arguments, publishes with them.
        assert!(translator
            .on_update(&subagent_tool_call(
                "spawn-1",
                "child-3",
                "Terminal",
                json!({})
            ))
            .is_empty());
        let mut args = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "child-3",
            "rawInput": { "command": "cargo test" },
        });
        args["_meta"]["claudeCode"]["parentToolUseId"] = json!("spawn-1");
        let released = translator.on_update(&args);
        assert_eq!(kinds(&released), vec!["tool_call"]);
        assert_eq!(released[0]["tool"]["input"]["command"], "cargo test");
        assert_eq!(released[0]["parentToolId"], "spawn-1");

        // The agent's spawn finally gets its arguments and is published whole.
        let spawn = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "spawn-2",
            "rawInput": { "description": "Second", "prompt": "go", "subagent_type": "general-purpose" },
            "_meta": { "claudeCode": { "toolName": "Agent" } },
        })));
        assert_eq!(kinds(&spawn), vec!["tool_call"]);
        assert_eq!(
            spawn[0]["tool"]["input"]["subagent_type"],
            "general-purpose"
        );
        assert!(spawn[0].get("parentToolId").is_none());
    }

    /// A held subagent call whose arguments never come is still published, at
    /// turn end, attributed.
    #[test]
    fn a_held_subagent_call_is_released_at_turn_end() {
        let mut translator = TranscriptTranslator::new(false);
        translator.on_update(&subagent_tool_call(
            "spawn-1",
            "child-1",
            "Terminal",
            json!({}),
        ));
        let items = translator.close_turn();
        assert_eq!(kinds(&items), vec!["tool_call"]);
        assert_eq!(items[0]["parentToolId"], "spawn-1");
    }

    /// The owning call's result carries a `subagent` object with exactly what
    /// was observed: the type and model from the call's input, the totals
    /// from a structured `rawOutput` when the adapter sends one.
    #[test]
    fn the_spawns_result_carries_only_observed_subagent_facts() {
        let mut translator = TranscriptTranslator::new(false);
        translator.on_update(&agent_spawn(
            "spawn-1",
            json!({ "description": "Survey", "prompt": "p", "subagent_type": "Explore", "model": "haiku" }),
        ));
        let mut done = agent_done("spawn-1", "completed", "report");
        done["rawOutput"] = json!({
            "status": "completed",
            "totalTokens": 51234,
            "totalDurationMs": 385000,
            "totalToolUseCount": 17,
        });
        let result = translator.on_update(&done);
        assert_eq!(
            result_of(&result)["subagent"],
            json!({
                "type": "Explore",
                "model": "haiku",
                "totalTokens": 51234,
                "durationMs": 385000,
                "toolUseCount": 17,
            })
        );

        // What claude-agent-acp 0.84.0 actually sends a non-AIR client: no
        // rawOutput, no model argument. Only the type survives.
        translator.on_update(&agent_spawn(
            "spawn-2",
            json!({ "description": "Again", "prompt": "p", "subagent_type": "general-purpose" }),
        ));
        let result = translator.on_update(&agent_done("spawn-2", "failed", "boom"));
        let result = result_of(&result);
        assert_eq!(result["subagent"], json!({ "type": "general-purpose" }));
        assert_eq!(result["isError"], true);

        // A spawn with nothing observed about its subagent carries no object
        // rather than an invented one, and an ordinary call never does.
        translator.on_update(&agent_spawn(
            "spawn-3",
            json!({ "description": "Bare", "prompt": "p" }),
        ));
        let bare = translator.on_update(&agent_done("spawn-3", "completed", "ok"));
        assert!(result_of(&bare).get("subagent").is_none());
        translator.on_update(&tool_call("t9", "Read", json!({ "file_path": "a" })));
        let plain = translator.on_update(&tool_done("t9", "completed", "ok"));
        assert!(result_of(&plain).get("subagent").is_none());
    }

    /// A subagent's calls and usage describe the subagent's context window,
    /// not the agent's: they neither count toward the turn's tool calls nor
    /// replace the turn's usage snapshot.
    #[test]
    fn a_subagents_calls_and_usage_are_not_the_turns() {
        let mut translator = TranscriptTranslator::new(false);
        let _ = translator.begin_turn("go", None, None, None, 0);
        translator.on_update(&agent_spawn(
            "spawn-1",
            json!({ "description": "d", "prompt": "p", "subagent_type": "Explore" }),
        ));
        translator.on_update(&subagent_tool_call(
            "spawn-1",
            "c1",
            "Read",
            json!({ "file_path": "a" }),
        ));
        translator.on_update(&subagent_tool_call(
            "spawn-1",
            "c2",
            "Read",
            json!({ "file_path": "b" }),
        ));
        assert_eq!(translator.tool_calls(), 1);

        translator.on_update(&update(
            json!({ "sessionUpdate": "usage_update", "used": 100 }),
        ));
        let mut sub_usage = json!({ "sessionUpdate": "usage_update", "used": 999 });
        sub_usage["_meta"]["claudeCode"]["parentToolUseId"] = json!("spawn-1");
        translator.on_update(&sub_usage);
        let items = translator.close_turn();
        let usage = items
            .iter()
            .find(|item| item["kind"] == "context_window_updated")
            .expect("usage");
        assert_eq!(usage["usage"]["used"], 100);
    }

    /// The attribution survives the publish seam: redaction and fitting keep
    /// `parentToolId` and `subagent` on the item.
    #[test]
    fn attribution_survives_fit_item() {
        let item = json!({
            "kind": "tool_result",
            "toolId": "spawn-1",
            "toolName": "Survey",
            "content": "report",
            "isError": false,
            "subagent": { "type": "Explore" },
            "parentToolId": "toolu_01ABC",
        });
        let fitted = fit_item(item, 512, 32 * 1024);
        assert_eq!(fitted["parentToolId"], "toolu_01ABC");
        assert_eq!(fitted["subagent"]["type"], "Explore");
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
        let content = result_of(&items)["content"].as_str().expect("content");
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
        assert!(result_of(&items)["content"]
            .as_str()
            .expect("content")
            .contains("boom"));
        assert_eq!(result_of(&items)["isError"], true);
    }

    #[test]
    fn a_new_turn_discards_anything_left_from_the_previous_one() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&chunk("stale"));
        let items = translator.begin_turn("fresh", None, None, None, 0);
        assert_eq!(kinds(&items), vec!["user_prompt"]);
        assert_eq!(kinds(&translator.end_turn(result())), vec!["result"]);
    }

    /// Sessions are multi-operator, so the opening item has to name the
    /// operator the provider verified — not the founder, and not nobody.
    #[test]
    fn begin_turn_stamps_the_commanding_operator_on_the_prompt() {
        let operator = "b".repeat(64);
        let mut translator = TranscriptTranslator::new(false);
        let items = translator.begin_turn("go", Some(&operator), Some("turn-1"), None, 0);
        assert_eq!(items[0]["kind"], "user_prompt");
        assert_eq!(items[0]["operatorPubkey"], operator);
    }

    /// Without a witnessed operator the key is absent rather than `null`: an
    /// explicit null would claim the provider observed "no operator".
    #[test]
    fn begin_turn_omits_attribution_when_no_operator_is_known() {
        let mut translator = TranscriptTranslator::new(false);
        let items = translator.begin_turn("go", None, None, None, 0);
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
            "rawInput": { "command": "just test" },
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

    /// claude-agent-acp opens an edit with an empty `rawInput` and a
    /// placeholder title, then fills both in on later `tool_call_update`s. The
    /// provider used to read only the opening frame, so 19 of 19 edits in the
    /// 2026-08-29 walk reached the wire as `"input": {}` with no path and no
    /// diff — presence published as absence.
    #[test]
    fn an_edits_path_and_diff_reach_the_wire_not_an_empty_input() {
        let mut translator = TranscriptTranslator::new(true);
        let call = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Preparing file…",
            "kind": "edit",
            "status": "pending",
            "rawInput": {},
        })));
        assert!(
            call.is_empty(),
            "held until it says what it edits: {call:?}"
        );

        // The arguments, the locations and the diff all arrive later, on a
        // non-terminal update, which publishes the held call with them.
        let opened = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "title": "Edit",
            "status": "in_progress",
            "rawInput": { "file_path": "desktop/src/App.tsx" },
            "locations": [{ "path": "desktop/src/App.tsx", "line": 42 }],
            "content": [{
                "type": "diff",
                "path": "desktop/src/App.tsx",
                "oldText": "const a = 1;",
                "newText": "const a = 2;",
            }],
        })));
        assert_eq!(kinds(&opened), vec!["tool_call"]);
        assert_eq!(opened[0]["tool"]["toolKind"], "edit");
        assert_eq!(
            opened[0]["tool"]["input"]["file_path"],
            "desktop/src/App.tsx"
        );
        assert_eq!(opened[0]["tool"]["edit"]["paths"][0], "desktop/src/App.tsx");

        let done = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "status": "completed",
        })));
        assert_eq!(done[0]["toolKind"], "edit");
        assert_eq!(done[0]["toolName"], "Edit");
        assert_eq!(done[0]["input"]["file_path"], "desktop/src/App.tsx");
        assert_eq!(done[0]["edit"]["paths"][0], "desktop/src/App.tsx");
        assert_eq!(done[0]["edit"]["changes"][0]["path"], "desktop/src/App.tsx");
        assert_eq!(done[0]["edit"]["changes"][0]["oldText"], "const a = 1;");
        assert_eq!(done[0]["edit"]["changes"][0]["newText"], "const a = 2;");
        assert!(done[0]["edit"].get("truncated").is_none());
    }

    /// The opening frame publishes whatever the adapter already had, so an
    /// adapter that sends the edit up front is not made to wait for the result.
    #[test]
    fn an_edit_that_arrives_complete_is_published_on_the_call_itself() {
        let mut translator = TranscriptTranslator::new(true);
        let call = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Write",
            "kind": "edit",
            "status": "pending",
            "locations": [{ "path": "docs/NOTES.md" }],
            "content": [{ "type": "diff", "path": "docs/NOTES.md", "newText": "hello" }],
        })));
        assert_eq!(call[0]["tool"]["edit"]["paths"][0], "docs/NOTES.md");
        assert_eq!(call[0]["tool"]["edit"]["changes"][0]["newText"], "hello");
        assert!(call[0]["tool"]["edit"]["changes"][0]
            .get("oldText")
            .is_none());
    }

    /// A tool with no locations and no diff blocks publishes no `edit` key at
    /// all — an empty object would claim an observation nobody made.
    #[test]
    fn a_tool_with_nothing_to_report_publishes_no_edit_payload() {
        let mut translator = TranscriptTranslator::new(true);
        let call = translator.on_update(&tool_call("t1", "read_file", json!({ "path": "a.rs" })));
        assert!(call[0]["tool"].get("edit").is_none());
        let done = translator.on_update(&tool_done("t1", "completed", "ok"));
        assert!(done[0].get("edit").is_none());
    }

    /// An enormous edit is bounded, and the reader is told it was bounded.
    #[test]
    fn an_oversized_edit_payload_is_truncated_out_loud() {
        let mut translator = TranscriptTranslator::new(true);
        let huge = "x".repeat(64 * 1024);
        translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Edit",
            "kind": "edit",
            "status": "pending",
        })));
        let done = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "status": "completed",
            "locations": [{ "path": "big.txt" }],
            "content": [{ "type": "diff", "path": "big.txt", "oldText": huge, "newText": huge }],
        })));
        let payload = &result_of(&done)["edit"];
        assert_eq!(payload["paths"][0], "big.txt");
        let serialized = payload.to_string();
        assert!(
            serialized.len() <= buzz_core::coding_session_payload::MAX_TOOL_EDIT_PAYLOAD_BYTES,
            "{} bytes",
            serialized.len()
        );
        assert_eq!(payload["changes"][0]["truncated"], true);
    }

    /// A second terminal update for the same call still names the kind: the
    /// memo is kept for the whole turn rather than consumed by the first
    /// result, so a repeated `completed` frame cannot publish an unlabelled
    /// result.
    #[test]
    fn a_repeated_terminal_update_still_carries_the_discriminant() {
        let mut translator = TranscriptTranslator::new(true);
        translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Edit",
            "kind": "edit",
            "status": "pending",
        })));
        let first = translator.on_update(&tool_done("t1", "completed", "ok"));
        assert_eq!(result_of(&first)["toolKind"], "edit");
        let second = translator.on_update(&tool_done("t1", "completed", "ok"));
        assert_eq!(second[0]["toolKind"], "edit");
        assert_eq!(second[0]["toolName"], "Edit");
    }

    /// Two calls that both arrived without a `toolCallId` cannot be paired, so
    /// neither may borrow the other's file.
    #[test]
    fn an_anonymous_call_never_borrows_another_anonymous_calls_path() {
        let mut translator = TranscriptTranslator::new(true);
        let first = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "title": "Edit",
            "kind": "edit",
            "status": "pending",
            "locations": [{ "path": "first.rs" }],
        })));
        assert_eq!(first[0]["tool"]["edit"]["paths"][0], "first.rs");
        let second = translator.on_update(&update(json!({
            "sessionUpdate": "tool_call",
            "title": "Bash",
            "status": "pending",
        })));
        assert!(second[0]["tool"].get("edit").is_none());
        assert_eq!(second[0]["tool"]["toolName"], "Bash");
    }

    /// ACP marks `kind` optional, so an absent one stays absent — the provider
    /// never guesses a discriminant it was not given.
    #[test]
    fn an_absent_acp_kind_is_never_invented() {
        let mut translator = TranscriptTranslator::new(true);
        let call = translator.on_update(&tool_call("t1", "read_file", json!({ "path": "a" })));
        assert_eq!(call[0]["tool"]["toolName"], "read_file");
        assert!(call[0]["tool"].get("toolKind").is_none());
        let done = translator.on_update(&tool_done("t1", "completed", "ok"));
        assert!(done[0].get("toolKind").is_none());
    }
}

#[cfg(test)]
#[path = "transcript_terminal_tests.rs"]
mod terminal_tests;
