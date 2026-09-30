//! ACP client module — manages communication with an AI agent subprocess over stdio
//! using JSON-RPC 2.0 (newline-delimited / NDJSON).
//!
//! # Lifecycle
//! 1. [`AcpClient::spawn`] — launch agent binary as subprocess
//! 2. [`AcpClient::initialize`] — protocol version negotiation
//! 3. [`AcpClient::session_new`] — create session with MCP server config
//! 4. [`AcpClient::session_prompt_with_idle_timeout`] — send prompt with idle/hard deadline, return stop reason
//! 5. [`AcpClient::session_cancel`] / [`AcpClient::cancel_with_cleanup`] — cancel in-flight turn

use std::collections::HashMap;

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};

use crate::observer::{ObserverContext, ObserverHandle};
use crate::steer::{
    IdleGuard, LateSteerAck, NotDeliveredReason, SteerInput, SteerResolution, SteerWire,
    UnknownReason, STEER_ACK_DRAIN,
};
use crate::usage::{
    PromptResponseUsage, StandardAdapterKind, StandardUsageTracker, TurnUsage, UsageTracker,
};

/// The clock the turn deadlines are measured against. See
/// [`idle_clock::TurnClock`].
#[path = "acp_idle_clock.rs"]
mod idle_clock;
#[path = "acp_steer_write.rs"]
mod steer_write;

use idle_clock::{SystemTurnClock, TurnClock};

/// Maximum allowed size of a single NDJSON line from the agent's stdout.
/// Lines exceeding this limit are rejected to prevent OOM from rogue agents.
const MAX_LINE_SIZE: usize = 10_000_000; // 10 MB

/// Lines of adapter stderr retained for the failure report.
const STDERR_TAIL_LINES: usize = 200;
/// Longest single stderr line retained; longer ones keep their head.
const STDERR_TAIL_LINE_BYTES: usize = 2_048;

/// The tail of an agent's stderr, bounded in both directions.
///
/// An adapter's stderr is unbounded output from a process this host does not
/// control, so it is never accumulated whole: the buffer keeps the most recent
/// [`STDERR_TAIL_LINES`] lines and truncates any line past
/// [`STDERR_TAIL_LINE_BYTES`]. The tail is the useful part — a crash explains
/// itself in its last lines, not its first.
#[derive(Debug, Default)]
pub struct StderrTail {
    lines: std::sync::Mutex<std::collections::VecDeque<String>>,
}

impl StderrTail {
    fn push(&self, line: &str) {
        let mut kept = line.trim_end().to_owned();
        if kept.len() > STDERR_TAIL_LINE_BYTES {
            // Cut on a char boundary so the retained head stays valid UTF-8.
            let mut cut = STDERR_TAIL_LINE_BYTES;
            while cut > 0 && !kept.is_char_boundary(cut) {
                cut -= 1;
            }
            kept.truncate(cut);
            kept.push_str("…[truncated]");
        }
        let Ok(mut lines) = self.lines.lock() else {
            // A poisoned lock means a reader panicked mid-tail. Diagnostics are
            // not worth propagating a panic into the session's control path.
            return;
        };
        if lines.len() == STDERR_TAIL_LINES {
            lines.pop_front();
        }
        lines.push_back(kept);
    }

    /// The retained lines, oldest first. Empty when the adapter said nothing.
    pub fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The retained tail as one block, or `None` when nothing was captured.
    ///
    /// `None` rather than an empty string so a caller can tell "the adapter
    /// printed nothing" apart from "there is a report here", and omit the
    /// section entirely rather than showing an empty heading.
    pub fn joined(&self) -> Option<String> {
        let lines = self.lines();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

/// Which of the read loop's deadlines is nearest, decided before sleeping so
/// the classification cannot be changed by scheduler jitter after the fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeadlineKind {
    Idle,
    AnswerStall,
    Hard,
}

impl DeadlineKind {
    /// The error this expiry means, with the warning that explains it.
    ///
    /// Both expiry sites — the pre-select check and the sleep arm — go through
    /// here. They were duplicated blocks before there was a third deadline to
    /// get wrong in two places.
    fn into_error(
        self,
        idle_timeout: std::time::Duration,
        stall_watch: &AnswerStallWatch,
        last_activity_at: tokio::time::Instant,
        wire: &TurnWire,
        now: tokio::time::Instant,
    ) -> AcpError {
        // One snapshot for every arm: the diagnosis is the same set of facts
        // whichever budget expired, and taking it once means the three cannot
        // disagree about what the wire had done. `now` comes from the clock
        // that classified the expiry, so the summary can never disagree with
        // the deadline that fired.
        let summary = Box::new(wire.summarize(now, stall_watch));
        match self {
            Self::Idle => {
                tracing::warn!(
                    target: "acp::stall",
                    "idle timeout ({idle_timeout:?}) — {}",
                    summary.one_line()
                );
                AcpError::IdleTimeout {
                    timeout: idle_timeout,
                    wire: summary,
                }
            }
            Self::AnswerStall => {
                let quiet = stall_watch.timeout.unwrap_or_default();
                tracing::warn!(
                    target: "acp::stall",
                    "answer stall ({quiet:?}) — the agent finished answering with no tool \
                     in flight and never resolved the prompt — {}",
                    summary.one_line()
                );
                AcpError::AnswerStall {
                    quiet,
                    wire: summary,
                }
            }
            Self::Hard => {
                let silence = now.saturating_duration_since(last_activity_at);
                tracing::warn!(
                    target: "acp::stall",
                    "hard turn timeout exceeded (silence {silence:?}) — {}",
                    summary.one_line()
                );
                AcpError::HardTimeout {
                    silence,
                    wire: summary,
                }
            }
        }
    }
}

/// Watches a turn for the signature of an adapter that has stopped answering.
///
/// The idle deadline is a *silence* budget, and silence is normal: a build, a
/// test suite, a subagent thinking can all legitimately say nothing for
/// minutes. So it is sized for the worst legitimate case and cannot be
/// tightened without killing healthy turns.
///
/// This watches for something else — the shape of a turn that is already over.
/// Once the agent has streamed prose at the top level and every tool call it
/// opened has reported a terminal status, there is nothing left for it to be
/// doing: the SDK result follows within milliseconds. Silence *there* is not
/// slow work, it is a prompt that will never be answered, and it can be given a
/// far shorter budget than the idle timer without endangering anything.
///
/// Subagent-attributed frames are excluded from every part of this. A
/// subagent's prose is not the turn's answer — claude-agent-acp makes the same
/// distinction on the same field when it decides what counts as answer
/// delivery — and a subagent's tool calls belong to the Task call that is
/// already counted at the top level.
#[derive(Debug)]
struct AnswerStallWatch {
    /// `None` disables the watch entirely.
    timeout: Option<std::time::Duration>,
    /// Whether top-level prose has been seen since the last top-level tool
    /// call opened. Prose *before* a tool call is narration ("I'll orient
    /// first…"), not the answer: the agent still owes a response to the tool's
    /// result, and silence there is the agent working or stuck mid-turn, not an
    /// answer whose prompt response never came (ledger 166).
    answer_streamed: bool,
    /// Tool calls opened at the top level and not yet terminal.
    tools_in_flight: std::collections::HashSet<String>,
    /// Armed deadline, or `None` when the signature does not currently hold.
    deadline: Option<tokio::time::Instant>,
}

impl AnswerStallWatch {
    fn new(timeout: Option<std::time::Duration>) -> Self {
        Self {
            timeout,
            answer_streamed: false,
            tools_in_flight: std::collections::HashSet::new(),
            deadline: None,
        }
    }

    /// Fold one inbound message in, then re-evaluate the deadline.
    ///
    /// Called for every line, not just `session/update`s: any line at all is
    /// activity, and while the signature holds it pushes the deadline back. The
    /// watch therefore measures silence *after* the answer, which is the thing
    /// that is actually anomalous.
    fn observe(&mut self, msg: &serde_json::Value, now: tokio::time::Instant) {
        if self.timeout.is_none() {
            return;
        }
        if msg.get("method").and_then(|m| m.as_str()) == Some("session/update") {
            let update = &msg["params"]["update"];
            if !update_is_subagent_attributed(update) {
                match update.get("sessionUpdate").and_then(|k| k.as_str()) {
                    Some("agent_message_chunk") => self.answer_streamed = true,
                    Some("tool_call") => {
                        // Whatever streamed before this was not the answer.
                        self.answer_streamed = false;
                        if let Some(id) = update.get("toolCallId").and_then(|v| v.as_str()) {
                            self.tools_in_flight.insert(id.to_owned());
                        }
                    }
                    Some("tool_call_update") => {
                        let terminal = matches!(
                            update.get("status").and_then(|v| v.as_str()),
                            Some("completed" | "failed")
                        );
                        if terminal {
                            if let Some(id) = update.get("toolCallId").and_then(|v| v.as_str()) {
                                self.tools_in_flight.remove(id);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.rearm(now);
    }

    fn rearm(&mut self, now: tokio::time::Instant) {
        let armed = self.answer_streamed && self.tools_in_flight.is_empty();
        self.deadline = match (armed, self.timeout) {
            (true, Some(timeout)) => Some(now + timeout),
            _ => None,
        };
    }

    fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }
}

/// Whether a `session/update`'s payload describes a subagent's work.
///
/// claude-agent-acp stamps `_meta.claudeCode.parentToolUseId` — on the `update`
/// object, not the notification's `params` — onto every frame produced inside a
/// Task subagent.
fn update_is_subagent_attributed(update: &serde_json::Value) -> bool {
    update
        .pointer("/_meta/claudeCode/parentToolUseId")
        .and_then(|v| v.as_str())
        .is_some_and(|parent| !parent.is_empty())
}

/// What crossed the wire during one turn, accumulated as it happens.
///
/// This exists because a published transcript records when Buzz *flushed* an
/// item, not when the frame arrived. Agent prose is buffered to a size, tool,
/// or turn-end boundary, so a turn that answered at +52s and then went silent
/// is indistinguishable, in the archive, from one that answered at +952s. The
/// only way anyone had to tell them apart was to subtract the idle budget from
/// the turn span by hand — which is how the 2026-08-24 stall was eventually
/// read, an hour later than it needed to be.
///
/// Recorded per turn and reported on abnormal endings only. A turn that
/// resolved normally has no unanswered question for this to answer, and an
/// extra item on every turn of every transcript is a cost paid forever for a
/// diagnosis wanted rarely.
#[derive(Debug, Clone)]
struct TurnWire {
    started_at: tokio::time::Instant,
    frames: u64,
    bytes: u64,
    first_frame_at: Option<tokio::time::Instant>,
    last_frame_at: Option<tokio::time::Instant>,
    last_frame_kind: Option<String>,
    subagent_frames: u64,
    kinds: std::collections::BTreeMap<String, u64>,
}

impl TurnWire {
    fn new(started_at: tokio::time::Instant) -> Self {
        Self {
            started_at,
            frames: 0,
            bytes: 0,
            first_frame_at: None,
            last_frame_at: None,
            last_frame_kind: None,
            subagent_frames: 0,
            kinds: std::collections::BTreeMap::new(),
        }
    }

    /// Classify one inbound message into the bucket a reader would look for.
    ///
    /// `session/update` frames are named by their `sessionUpdate` kind, which
    /// is the distinction that matters when reading a stall; everything else
    /// keeps its JSON-RPC identity so an unanswered agent-initiated request is
    /// visible as itself rather than as "other".
    fn classify(msg: &serde_json::Value) -> String {
        match msg.get("method").and_then(|m| m.as_str()) {
            Some("session/update") => msg["params"]["update"]
                .get("sessionUpdate")
                .and_then(|k| k.as_str())
                .unwrap_or("session/update")
                .to_owned(),
            Some(other) => other.to_owned(),
            None if msg.get("id").is_some() => "response".to_owned(),
            None => "unknown".to_owned(),
        }
    }

    fn record(&mut self, msg: &serde_json::Value, bytes: usize, now: tokio::time::Instant) {
        let kind = Self::classify(msg);
        self.frames += 1;
        self.bytes += bytes as u64;
        self.first_frame_at.get_or_insert(now);
        self.last_frame_at = Some(now);
        if msg.get("method").and_then(|m| m.as_str()) == Some("session/update")
            && update_is_subagent_attributed(&msg["params"]["update"])
        {
            self.subagent_frames += 1;
        }
        *self.kinds.entry(kind.clone()).or_default() += 1;
        self.last_frame_kind = Some(kind);
    }

    /// Freeze the running tally into the reportable shape.
    ///
    /// `quiet_for` and the offsets are resolved against `now` here rather than
    /// stored as instants, so the summary can outlive the turn and cross into
    /// the provider without carrying a clock with it.
    fn summarize(
        &self,
        now: tokio::time::Instant,
        stall_watch: &AnswerStallWatch,
    ) -> TurnWireSummary {
        TurnWireSummary {
            frames: self.frames,
            bytes: self.bytes,
            subagent_frames: self.subagent_frames,
            turn_elapsed: now.saturating_duration_since(self.started_at),
            first_frame_offset: self
                .first_frame_at
                .map(|at| at.saturating_duration_since(self.started_at)),
            last_frame_offset: self
                .last_frame_at
                .map(|at| at.saturating_duration_since(self.started_at)),
            last_frame_kind: self.last_frame_kind.clone(),
            quiet_for: self
                .last_frame_at
                .map(|at| now.saturating_duration_since(at))
                .unwrap_or_else(|| now.saturating_duration_since(self.started_at)),
            tools_in_flight: stall_watch.tools_in_flight.len(),
            answer_streamed: stall_watch.answer_streamed,
            kinds: self.kinds.clone(),
        }
    }
}

/// A turn's wire activity, frozen at the moment something went wrong.
///
/// Carried inside the timeout errors so the failure explains itself, and handed
/// to the provider so an abnormal turn can publish the same facts to a reader
/// who was never near the machine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnWireSummary {
    /// Frames parsed from the agent's stdout this turn.
    pub frames: u64,
    /// Bytes of those frames, before parsing.
    pub bytes: u64,
    /// How many of `frames` were attributed to a subagent.
    pub subagent_frames: u64,
    /// Wall time from the prompt write to this snapshot.
    pub turn_elapsed: std::time::Duration,
    /// When the first frame arrived, relative to the prompt write.
    pub first_frame_offset: Option<std::time::Duration>,
    /// When the last frame arrived, relative to the prompt write.
    pub last_frame_offset: Option<std::time::Duration>,
    /// `sessionUpdate` kind — or JSON-RPC method — of that last frame.
    pub last_frame_kind: Option<String>,
    /// How long the wire had been quiet when this was taken.
    pub quiet_for: std::time::Duration,
    /// Top-level tool calls opened and never reported terminal.
    pub tools_in_flight: usize,
    /// Whether top-level prose had streamed before the silence.
    pub answer_streamed: bool,
    /// Frame counts by kind.
    pub kinds: std::collections::BTreeMap<String, u64>,
}

impl TurnWireSummary {
    /// The two or three kinds worth naming, most frequent first.
    ///
    /// Bounded because the one consumer that renders this has a 200-character
    /// row and a long tail of one-off kinds would push the load-bearing facts
    /// out of it.
    pub fn top_kinds(&self, limit: usize) -> Vec<(String, u64)> {
        let mut pairs: Vec<(String, u64)> =
            self.kinds.iter().map(|(k, v)| (k.clone(), *v)).collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        pairs.truncate(limit);
        pairs
    }

    /// A one-line rendering: what arrived, when it stopped, and what was owed.
    pub fn one_line(&self) -> String {
        let last = match (&self.last_frame_kind, self.last_frame_offset) {
            (Some(kind), Some(offset)) => format!("last {kind} at +{:.1}s", offset.as_secs_f64()),
            _ => "no frames arrived".to_owned(),
        };
        let kinds = self
            .top_kinds(3)
            .into_iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{} frames/{} KiB over {:.0}s; {last}; quiet {:.1}s; {} tools in flight; answer {}{}{}",
            self.frames,
            self.bytes / 1024,
            self.turn_elapsed.as_secs_f64(),
            self.quiet_for.as_secs_f64(),
            self.tools_in_flight,
            if self.answer_streamed {
                "streamed"
            } else {
                "not streamed"
            },
            if self.subagent_frames > 0 {
                format!("; {} subagent frames", self.subagent_frames)
            } else {
                String::new()
            },
            if kinds.is_empty() {
                String::new()
            } else {
                format!("; {kinds}")
            },
        )
    }
}

/// Ext-notification claude-agent-acp uses to forward raw SDK messages.
///
/// Only arrives when the session asked for it via
/// `_meta.claudeCode.emitRawSDKMessages`.
const RAW_SDK_FRAME_METHOD: &str = "_claude/sdkMessage";

/// Result-message origin kinds claude-agent-acp treats as an autonomous cycle
/// rather than the user's own turn — copied from the adapter's
/// `AUTONOMOUS_RESULT_ORIGINS` (0.70.0, `dist/acp-agent.js:114`). The adapter
/// is deliberately fail-open: any other kind, including `human` (which it
/// stamps on every ACP prompt) and future kinds, is the user's lane.
const AUTONOMOUS_RESULT_ORIGINS: &[&str] = &[
    "task-notification",
    "peer",
    "coordinator",
    "observer",
    "observer-activity",
];

/// Drain a child's stderr into `tail`, re-emitting each line through `tracing`.
///
/// Both halves matter: the re-emission is what keeps the harness terminal's
/// behaviour after the switch from `Stdio::inherit`, and the tail is what makes
/// the same output available to a session that failed hours ago on a machine
/// nobody was watching.
fn drain_stderr(stderr: ChildStderr, tail: std::sync::Arc<StderrTail>) {
    tokio::spawn(async move {
        let mut reader = FramedRead::new(stderr, LinesCodec::new_with_max_length(MAX_LINE_SIZE));
        while let Some(next) = reader.next().await {
            match next {
                Ok(line) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    tracing::info!(target: "acp::stderr", "{line}");
                    tail.push(&line);
                }
                // A line past the cap or invalid UTF-8 is the adapter
                // misbehaving on a channel we only observe. Note it and keep
                // reading: abandoning the drain would block the child once the
                // pipe filled, turning a cosmetic fault into a hang.
                Err(error) => {
                    tracing::debug!(target: "acp::stderr", "unreadable stderr line: {error}");
                }
            }
        }
    });
}

/// An MCP server configuration passed to `session/new`.
///
/// Corresponds to the `McpServerStdio` variant in the ACP schema.
/// All four fields are **required** by the schema (`args` and `env` may be empty arrays).
#[derive(Debug, Clone, serde::Serialize)]
pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<EnvVar>,
}

/// A single environment variable for an MCP server.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// Stop reason returned by `session/prompt` when the agent finishes a turn.
///
/// Maps to the `stopReason` field in the `SessionPromptResponse`.
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// Agent completed the turn normally (`"end_turn"`).
    EndTurn,
    /// Turn was cancelled via `session/cancel` (`"cancelled"`).
    Cancelled,
    /// Agent hit its token limit (`"max_tokens"`).
    MaxTokens,
    /// Agent hit its per-turn request limit (`"max_turn_requests"`).
    MaxTurnRequests,
    /// Agent refused the prompt (`"refusal"`).
    /// Note: refused turns are dropped from history by the agent.
    Refusal,
}

impl StopReason {
    /// Parse a `stopReason` string from the ACP wire format.
    ///
    /// Matching is case-insensitive so agents that send `"END_TURN"` or
    /// `"Cancelled"` are handled correctly without a protocol error.
    // Deliberately not `FromStr`: an unrecognized stop reason is a normal wire
    // condition to be tolerated, not an error to be constructed and reported,
    // so `Option` is the honest return type. (The lint only became visible when
    // this module went public; the signature predates it and callers rely on it.)
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "end_turn" => Some(Self::EndTurn),
            "cancelled" => Some(Self::Cancelled),
            "max_tokens" => Some(Self::MaxTokens),
            "max_turn_requests" => Some(Self::MaxTurnRequests),
            "refusal" => Some(Self::Refusal),
            _ => None,
        }
    }
}

/// Errors that can occur in the ACP client.
#[derive(Debug, thiserror::Error)]
pub enum AcpError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Agent process exited unexpectedly")]
    AgentExited,

    #[error("Idle timeout — no agent activity for {timeout:?} [{}]", wire.one_line())]
    IdleTimeout {
        timeout: std::time::Duration,
        /// What the wire had actually done when the budget ran out. Boxed to
        /// keep `AcpError` small — it is returned by value on every hot path.
        wire: Box<TurnWireSummary>,
    },

    #[error("Hard turn timeout exceeded (silence {silence:?}) [{}]", wire.one_line())]
    HardTimeout {
        silence: std::time::Duration,
        wire: Box<TurnWireSummary>,
    },

    /// The agent answered and then never resolved the prompt.
    ///
    /// Distinct from [`IdleTimeout`](Self::IdleTimeout) on purpose: an idle
    /// timeout means "we do not know what it is doing", and this means "we know
    /// it is finished and the response never came". They call for different
    /// words to the operator and a different presumption about the work, so
    /// they must not collapse into one error.
    #[error(
        "Agent finished answering but never resolved the prompt ({quiet:?} after the answer) [{}]",
        wire.one_line()
    )]
    AnswerStall {
        quiet: std::time::Duration,
        wire: Box<TurnWireSummary>,
    },

    #[error("Agent did not stop within {0:?} after cancellation")]
    CancelDrainTimeout(std::time::Duration),

    #[error("Request timeout — agent did not respond within {0:?}")]
    Timeout(std::time::Duration),

    #[error("Write timeout — agent stopped reading stdin (blocked for {0:?})")]
    WriteTimeout(std::time::Duration),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Agent reported error (code {code}): {message}")]
    AgentError { code: i64, message: String },
}

/// Build an [`AcpError::AgentError`] from a JSON-RPC error object,
/// preserving the numeric code. When the `message` field is missing or
/// non-string, fall back to the full JSON object so provider-specific
/// detail (e.g. a `data` field) is not lost.
fn agent_error_from_json(error: &serde_json::Value) -> AcpError {
    let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-32000);
    let message = match error.get("message").and_then(|m| m.as_str()) {
        Some(m) => m.to_string(),
        None => error.to_string(),
    };
    AcpError::AgentError { code, message }
}

fn build_initialize_params() -> serde_json::Value {
    serde_json::json!({
        "protocolVersion": 2,
        "clientCapabilities": build_client_capabilities(),
        "clientInfo": {
            "name": "buzz-acp",
            "version": env!("CARGO_PKG_VERSION")
        },
    })
}

/// ACP client that owns an agent subprocess and communicates over its stdio.
///
/// One `AcpClient` per agent process. Multiple sessions can be created on the
/// same client via repeated calls to [`session_new`](AcpClient::session_new).
pub struct AcpClient {
    /// The agent child process (kept alive to prevent zombie).
    child: Child,
    /// Write end of the agent's stdin pipe.
    stdin: ChildStdin,
    /// Framed reader over the agent's stdout pipe (line-oriented, bounded).
    /// Uses `LinesCodec::new_with_max_length` to enforce MAX_LINE_SIZE at the
    /// read level — prevents OOM from rogue agents writing infinite non-newline bytes.
    reader: FramedRead<ChildStdout, LinesCodec>,
    /// Monotonically increasing JSON-RPC request id counter.
    /// Harness-generated IDs are always numeric.
    next_id: u64,
    /// The id of a `session/request_permission` request that has been received
    /// but not yet responded to. Stored as `serde_json::Value` because JSON-RPC 2.0
    /// permits both numeric and string IDs from the agent.
    /// Used by [`cancel_with_cleanup`](AcpClient::cancel_with_cleanup) to send
    /// a `cancelled` outcome before the agent returns from `session/prompt`.
    pending_permission_id: Option<serde_json::Value>,
    /// Whether we have already sent a response to the pending permission request.
    /// Guards against double-response if a timeout fires after the allow_once
    /// response was written but before `pending_permission_id` was cleared.
    permission_responded: bool,
    /// The JSON-RPC id of the most recently sent `session/prompt` request.
    /// Used by [`cancel_with_cleanup`] to drain the correct response.
    /// Set in [`session_prompt_with_idle_timeout`]; consumed in [`cancel_with_cleanup`].
    last_prompt_id: Option<u64>,
    /// Hard deadline for the current turn, set by `session_prompt_with_idle_timeout`.
    /// Inherited by `cancel_with_cleanup` so the drain loop shares the same budget
    /// rather than starting a fresh timer (prevents double-jeopardy).
    current_hard_deadline: Option<tokio::time::Instant>,
    /// Optional local observer feed used by the desktop app.
    observer: Option<ObserverHandle>,
    /// Pool slot index for this agent process.
    observer_agent_index: Option<usize>,
    /// Best-effort context attached to raw ACP wire events.
    observer_context: ObserverContext,
    /// Most recently observed `_meta.goose.activeRunId` from a
    /// `session/update` notification of kind `session_info_update`.
    ///
    /// Both goose and buzz-agent emit `session_info_update` with this field;
    /// goose emits it whenever it starts or clears an active prompt run
    /// (`crates/goose/src/acp/server.rs:2277` `send_active_run_update`).
    /// Required as `expectedRunId` when calling the non-standard
    /// `_goose/unstable/session/steer` method to inject a message into an
    /// in-flight turn without cancelling it.
    ///
    /// `None` until the first `session_info_update` arrives, or after the
    /// run clears (goose/buzz-agent emit `activeRunId: null` at end of turn).
    /// Other agents may leave this unset — readers must treat `None` as
    /// "no active run to steer into" and fall back to cancel+merge.
    active_run_id: Option<String>,
    /// Whether the agent advertised `_meta.steering.supported: true` in its
    /// `initialize` response, meaning it implements the cross-adapter
    /// [`ACP_STEER_METHOD`] extension.
    ///
    /// Set once by [`initialize`](Self::initialize); `false` for agents that
    /// omit the key. This is the **only** gate on writing an
    /// [`ACP_STEER_METHOD`] request. It must never be replaced by error-code
    /// probing: codex-acp answers unrecognized extension methods with `{}` —
    /// a JSON-RPC *success*, not `-32601` — which the main loop would read as
    /// a delivered steer and drop the user's message from the queue.
    steering_supported: bool,
    /// Whether the agent advertised
    /// `agentCapabilities.promptCapabilities.image: true` at `initialize`.
    prompt_image_supported: bool,
    /// Whether the agent advertised the stable top-level `loadSession`
    /// capability during initialization.
    session_load_supported: bool,
    /// Whether the agent advertised `sessionCapabilities.resume` during
    /// initialization.
    session_resume_supported: bool,
    /// Per-turn source of mid-turn steer inputs for the read loop's steer
    /// arm. Installed by [`install_steer_input`](Self::install_steer_input)
    /// (public transport) or [`install_steer_rx`](Self::install_steer_rx)
    /// (legacy pool harness) before the prompt, and consumed (via `take()`)
    /// by `session_prompt_with_idle_timeout` so it is dropped at scope exit
    /// alongside the turn it served. `None` outside a turn — the steer arm
    /// is disabled in that case.
    steer_rx: Option<SteerSource>,
    /// Where a steer acknowledgement goes when it arrives after its attempt
    /// was already resolved [`SteerResolution::Unknown`]. Unset → logged and
    /// dropped.
    late_steer_sink: Option<tokio::sync::mpsc::UnboundedSender<LateSteerAck>>,
    /// Written steer requests whose acknowledgement never arrived, keyed by
    /// JSON-RPC id so every read loop can still correlate a late answer.
    /// Shared with the in-flight [`PendingSteer`] so a cancelled prompt
    /// future registers its attempt on drop.
    unresolved_steers: UnresolvedSteers,
    /// Usage tracker for goose/buzz-agent's cumulative notification format.
    goose_usage: UsageTracker,
    /// Per-turn prompt-response usage and Claude's optional cumulative cost.
    standard_usage: StandardUsageTracker,
    /// Known adapter identity for prompt-response usage mapping.
    standard_adapter: Option<StandardAdapterKind>,
    /// Normalized adapter identity from `initialize` (`agentInfo.name`, else
    /// `serverInfo.name`). `"unknown"` until `initialize` answers.
    agent_name: String,
    /// Bounded tail of the adapter's stderr, filled by a background reader.
    stderr_tail: std::sync::Arc<StderrTail>,
    /// How long a turn may stay silent *after* it has finished answering before
    /// the host stops waiting. `None` disables the watch. See
    /// [`AnswerStallWatch`].
    answer_stall_timeout: Option<std::time::Duration>,
    /// Ask claude-agent-acp to forward every raw SDK message it sees.
    ///
    /// Off by default and deliberately not a published fact: these frames are
    /// the adapter's own internals, unredacted, and they are wanted for one
    /// debugging session at a time rather than for a session's life.
    emit_raw_sdk_frames: bool,
    /// Tool names claude-agent-acp is asked to remove from this session.
    ///
    /// Empty by default, and empty means the `_meta` key is omitted entirely
    /// rather than sent as `[]`, so a session that denies nothing is
    /// byte-identical to one from before the option existed.
    /// Host-owned `_meta.claudeCode.options` entries written on every
    /// session open. Empty by default, which omits nothing and adds nothing.
    claude_options: serde_json::Map<String, serde_json::Value>,
    disallowed_tools: Vec<String>,
    /// Adapter build from `initialize` (`agentInfo.version`, else
    /// `serverInfo.version`), verbatim. `None` when the adapter reported none.
    ///
    /// Kept because the adapter is the moving part in this stack — its turn
    /// lifecycle changed repeatedly across 0.4x → 0.7x — so "which build was
    /// this?" is the first question any stalled-turn report has to answer, and
    /// the name alone cannot.
    agent_version: Option<String>,
    /// ACP protocol version reported by the adapter at `initialize`. `1` until
    /// `initialize` answers, matching the pool's own default for adapters that
    /// omit the field.
    protocol_version: u32,
    /// The clock every turn deadline in this client is measured against.
    ///
    /// Always [`SystemTurnClock`] in production — real monotonic time is the
    /// only correct answer for a silent-agent guard. Replaceable in tests
    /// (`set_turn_clock`) so a test asserting wire bookkeeping is not also
    /// asserting how fast the operating system schedules a subprocess.
    turn_clock: std::sync::Arc<dyn TurnClock>,
}

/// Recursively merge `overlay` into `base`, with `overlay` winning on scalar/shape
/// collisions.  When both sides have an object for the same key, the merge recurses so
/// unrelated nested keys from `base` are preserved.
fn deep_merge(
    base: &mut serde_json::Map<String, serde_json::Value>,
    overlay: serde_json::Map<String, serde_json::Value>,
) {
    for (k, overlay_val) in overlay {
        match base.get_mut(&k) {
            Some(serde_json::Value::Object(base_obj))
                if matches!(overlay_val, serde_json::Value::Object(_)) =>
            {
                // Both sides are objects — recurse to preserve unrelated nested keys.
                if let serde_json::Value::Object(overlay_obj) = overlay_val {
                    deep_merge(base_obj, overlay_obj);
                }
            }
            _ => {
                // Scalar, array, type mismatch, or new key — overlay wins.
                base.insert(k, overlay_val);
            }
        }
    }
}

/// Build the merged `CODEX_CONFIG` environment-variable value for a Codex agent spawn.
///
/// Returns `Some(json_string)` when `has_generated_codex_config` is true (Buzz injected a
/// `CODEX_CONFIG` entry via `codex_network_env()`), `None` otherwise.
///
/// # Merge contract (when `has_generated_codex_config` is true)
///
/// 1. **Persona base** — the first `CODEX_CONFIG` value in `extra_env` is taken as
///    the base object (all keys preserved, recursively).  When there is no persona entry,
///    the generated entry serves as the base.
/// 2. **Generated overlay** — all subsequent `CODEX_CONFIG` entries are deep-merged into
///    the base so unrelated nested persona keys survive.
/// 3. **Parent-env precedence** — if `parent_codex_config` is `Some`, its keys are
///    deep-merged into the result (parent wins on colliding keys at every nesting level;
///    unrelated keys from either side survive).
/// 4. **Forced overlay** — `sandbox_workspace_write.network_access = true` is applied
///    last so relay access is guaranteed regardless of operator / persona config.
///
/// When `has_generated_codex_config` is false, the function returns `None` and the
/// caller handles any persona-supplied `CODEX_CONFIG` with ordinary operator-wins
/// semantics (no merging, no sandbox widening).
///
/// # Errors
///
/// Returns `Err(AcpError::Protocol)` when `has_generated_codex_config` is true and any
/// `CODEX_CONFIG` value is not valid JSON or is not a JSON object, or when
/// `sandbox_workspace_write` is present but not an object after all merges.
pub(crate) fn build_codex_config_env(
    extra_env: &[(String, String)],
    parent_codex_config: Option<&str>,
    has_generated_codex_config: bool,
) -> Result<Option<String>, AcpError> {
    // Without an explicit Buzz-generated overlay signal, skip the merge entirely.
    // Any persona CODEX_CONFIG is handled by the caller with operator-wins semantics.
    if !has_generated_codex_config {
        return Ok(None);
    }

    // Collect all CODEX_CONFIG entries from extra_env in order.
    let codex_entries: Vec<&str> = extra_env
        .iter()
        .filter(|(k, _)| k == "CODEX_CONFIG")
        .map(|(_, v)| v.as_str())
        .collect();

    if codex_entries.is_empty() {
        // has_generated_codex_config is true but no entry in extra_env — shouldn't
        // happen in practice, but treat as no-op rather than panic.
        return Ok(None);
    }

    // Parse all entries; first one is the persona base (or the generated entry if no
    // persona CODEX_CONFIG was set), rest are additional generated entries.
    let mut parsed_entries: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    for (i, raw) in codex_entries.iter().enumerate() {
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(serde_json::Value::Object(obj)) => parsed_entries.push(obj),
            Ok(_) => {
                let source = if i == 0 { "persona" } else { "generated" };
                return Err(AcpError::Protocol(format!(
                    "CODEX_CONFIG {source} value is valid JSON but not an object"
                )));
            }
            Err(e) => {
                let source = if i == 0 { "persona" } else { "generated" };
                return Err(AcpError::Protocol(format!(
                    "CODEX_CONFIG {source} value is not valid JSON: {e}"
                )));
            }
        }
    }

    // Start from first entry, deep-merge remaining entries.
    let mut base = parsed_entries.remove(0);
    for overlay in parsed_entries {
        deep_merge(&mut base, overlay);
    }

    // Deep-merge parent env (parent wins on colliding keys at every nesting level).
    if let Some(parent_raw) = parent_codex_config {
        match serde_json::from_str::<serde_json::Value>(parent_raw) {
            Ok(serde_json::Value::Object(parent_obj)) => {
                deep_merge(&mut base, parent_obj);
            }
            Ok(_) => {
                return Err(AcpError::Protocol(
                    "CODEX_CONFIG in parent environment is valid JSON but not an object".into(),
                ));
            }
            Err(e) => {
                return Err(AcpError::Protocol(format!(
                    "CODEX_CONFIG in parent environment is not valid JSON: {e}"
                )));
            }
        }
    }

    // Force sandbox_workspace_write.network_access = true (our invariant, always wins).
    let sws_entry = base
        .entry("sandbox_workspace_write")
        .or_insert_with(|| serde_json::json!({}));
    match sws_entry {
        serde_json::Value::Object(sws_obj) => {
            sws_obj.insert("network_access".to_string(), serde_json::Value::Bool(true));
        }
        other => {
            return Err(AcpError::Protocol(format!(
                "CODEX_CONFIG sandbox_workspace_write is not an object (got {}); \
                 cannot set network_access=true",
                other
            )));
        }
    }

    Ok(Some(serde_json::Value::Object(base).to_string()))
}

/// goose's non-standard mid-turn steer method. Requires `expectedRunId`, so it
/// is only usable once a `session_info_update` has supplied
/// `_meta.goose.activeRunId`. Emitted by goose and buzz-agent only.
const GOOSE_STEER_METHOD: &str = "_goose/unstable/session/steer";

/// The cross-adapter mid-turn steer method, shipped by claude-agent-acp
/// (`src/acp-agent.ts:200`) and codex-acp (`src/AcpExtensions.ts:11`).
/// Params are `{sessionId, prompt}` — no run id — and the result is
/// `{outcome}`. Gated on [`AcpClient::steering_supported`].
const ACP_STEER_METHOD: &str = "_session/steering";

/// `outcome` value meaning the steer was applied to the turn Buzz is waiting
/// on, which therefore keeps running.
const STEER_OUTCOME_INJECTED: &str = "injected";

/// `outcome` value meaning the turn Buzz was steering had already finished, so
/// the adapter began a fresh turn carrying the message. Still a delivery
/// success, but the awaited turn is over — see the steer-response arm for why
/// this must not renew the hard deadline.
const STEER_OUTCOME_STARTED_NEW_TURN: &str = "startedNewTurn";

/// `outcome` value meaning the adapter found no running turn and, because the
/// request asked for it (`_meta.steering.idleBehavior: "promptRequired"`),
/// started nothing.
const STEER_OUTCOME_PROMPT_REQUIRED: &str = "promptRequired";

/// `outcome` value the adapter uses for its own catch-all failure.
const STEER_OUTCOME_FAILED: &str = "failed";

/// JSON-RPC `method_not_found`.
const JSON_RPC_METHOD_NOT_FOUND: i64 = -32601;

/// Where the read loop's steer arm takes its inputs from for one turn.
///
/// Both sources feed the same arm; the only difference is how an outcome is
/// answered (see [`SteerOutcomeSink`]). Wrapping at `recv` time rather than
/// through a forwarding task keeps the legacy channel's capacity and drop
/// semantics exactly as they were.
enum SteerSource {
    /// The public transport (`buzz_acp::steer`).
    Native(tokio::sync::mpsc::Receiver<SteerInput>),
    /// The legacy channel harness's private request type, adapted per
    /// `docs/NATIVE_STEERING_IMPL.md` §3.1 item 8.
    Legacy(tokio::sync::mpsc::Receiver<crate::pool::SteerRequest>),
}

impl SteerSource {
    /// Take the next input, in order. `None` once the sender side is gone.
    /// Cancel-safe: neither receiver loses a message when this future is
    /// dropped mid-poll.
    async fn recv(&mut self) -> Option<TakenSteer> {
        match self {
            Self::Native(rx) => rx.recv().await.map(|input| TakenSteer {
                attempt_id: Some(input.attempt_id),
                prompt_blocks: input.prompt_blocks,
                idle_guard: input.idle_guard,
                write_guard: input.write_guard,
                sink: SteerOutcomeSink::Native(input.outcome_tx),
            }),
            Self::Legacy(rx) => rx.recv().await.map(|request| TakenSteer {
                attempt_id: None,
                prompt_blocks: request.prompt_blocks,
                idle_guard: IdleGuard::AdapterDefault,
                write_guard: None,
                sink: SteerOutcomeSink::Legacy(request.ack_tx),
            }),
        }
    }
}

/// One input the steer arm has taken from its source and not yet written.
struct TakenSteer {
    /// Caller-minted attempt id; `None` for the legacy harness, whose
    /// requests have no identity beyond their oneshot.
    attempt_id: Option<String>,
    prompt_blocks: Vec<String>,
    idle_guard: IdleGuard,
    write_guard: Option<std::sync::Arc<dyn crate::steer::SteerWriteGuard>>,
    sink: SteerOutcomeSink,
}

/// How one input's outcome is answered.
enum SteerOutcomeSink {
    /// Answered with the public [`SteerResolution`].
    Native(tokio::sync::oneshot::Sender<SteerResolution>),
    /// Answered with the legacy pool's `SteerAck`, mapped from the same
    /// resolution so the two surfaces cannot disagree about what happened.
    Legacy(tokio::sync::oneshot::Sender<crate::pool::SteerAck>),
}

impl SteerOutcomeSink {
    fn is_native(&self) -> bool {
        matches!(self, Self::Native(_))
    }

    /// Answer exactly once. A closed receiver is the caller's business.
    fn resolve(self, resolution: SteerResolution, session_id: &str) {
        match self {
            Self::Native(tx) => {
                let _ = tx.send(resolution);
            }
            Self::Legacy(tx) => {
                let _ = tx.send(crate::pool::SteerAck::from_resolution(
                    resolution, session_id,
                ));
            }
        }
    }
}

/// A written steer request that has not been answered, remembered so a late
/// answer can still be decoded and correlated to its attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UnresolvedSteer {
    /// The attempt the request belonged to.
    attempt_id: String,
    /// The method that carried it, so the late answer is decoded the way
    /// that method answers.
    wire: SteerWire,
    /// The run id the request named, for a late goose answer.
    native_run_id: Option<String>,
}

/// Written-but-unanswered steer requests keyed by JSON-RPC id.
///
/// Shared between the client and the in-flight [`PendingSteer`] so that a
/// prompt future dropped mid-flight (cancel, shutdown) can still record its
/// attempt from `Drop`, where `&mut AcpClient` is not reachable.
#[derive(Debug, Default, Clone)]
struct UnresolvedSteers(std::sync::Arc<std::sync::Mutex<HashMap<u64, UnresolvedSteer>>>);

impl UnresolvedSteers {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, UnresolvedSteer>> {
        // A poisoned map is still the right map: a panic elsewhere must not
        // turn every later late ACK into a stray.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn insert(&self, request_id: u64, entry: UnresolvedSteer) {
        self.lock().insert(request_id, entry);
    }

    fn remove(&self, request_id: u64) -> Option<UnresolvedSteer> {
        self.lock().remove(&request_id)
    }

    /// Attempt ids in wire-request order.
    fn attempt_ids(&self) -> Vec<String> {
        let map = self.lock();
        let mut entries: Vec<(&u64, &UnresolvedSteer)> = map.iter().collect();
        entries.sort_by_key(|(id, _)| **id);
        entries
            .into_iter()
            .map(|(_, entry)| entry.attempt_id.clone())
            .collect()
    }
}

/// The one steer request the read loop has written and is awaiting.
///
/// Holds the sink until an answer, a bounded drain, or the loop's own end
/// settles it. If the whole prompt future is dropped first (cancel,
/// shutdown), `Drop` answers [`UnknownReason::PromptEndedBeforeAck`] and
/// registers the request as unresolved, so the caller is never left with a
/// silently closed channel for bytes that did go out.
struct PendingSteer {
    request_id: u64,
    wire: SteerWire,
    native_run_id: Option<String>,
    attempt_id: Option<String>,
    session_id: String,
    sink: Option<SteerOutcomeSink>,
    unresolved: UnresolvedSteers,
}

impl PendingSteer {
    fn is_native(&self) -> bool {
        self.sink.as_ref().is_some_and(SteerOutcomeSink::is_native)
    }

    /// Answer with a decoded acknowledgement.
    fn resolve(&mut self, resolution: SteerResolution) {
        if let Some(sink) = self.sink.take() {
            sink.resolve(resolution, &self.session_id);
        }
    }

    /// Answer [`SteerResolution::Unknown`] and remember the request so a late
    /// acknowledgement can still be correlated. Only native attempts have an
    /// identity to correlate; a legacy request is answered and forgotten.
    fn abandon(&mut self, reason: UnknownReason) {
        if let (Some(attempt_id), true) = (self.attempt_id.clone(), self.is_native()) {
            self.unresolved.insert(
                self.request_id,
                UnresolvedSteer {
                    attempt_id,
                    wire: self.wire,
                    native_run_id: self.native_run_id.clone(),
                },
            );
        }
        self.resolve(SteerResolution::Unknown {
            reason,
            wire_request_id: Some(self.request_id),
        });
    }
}

impl Drop for PendingSteer {
    fn drop(&mut self) {
        if self.sink.is_some() {
            tracing::warn!(
                request_id = self.request_id,
                attempt_id = ?self.attempt_id,
                "prompt loop dropped with a steer request awaiting its acknowledgement"
            );
            self.abandon(UnknownReason::PromptEndedBeforeAck);
        }
    }
}

/// Decode a JSON-RPC response to a steer request into what it establishes.
///
/// `docs/NATIVE_STEERING_IMPL.md` §3.1 item 3. The goose wire answers with no
/// `outcome` — any success is a delivery into the run the request named.
/// The ACP extension's outcome must be positively recognized: codex-acp
/// answers unknown extension methods with a bare `{}` success, and reading
/// that as delivery would drop the input.
fn decode_steer_ack(
    msg: &serde_json::Value,
    wire: SteerWire,
    native_run_id: Option<String>,
    request_id: u64,
) -> SteerResolution {
    if let Some(error) = msg.get("error") {
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string());
        let reason = if code == JSON_RPC_METHOD_NOT_FOUND {
            NotDeliveredReason::MethodNotFound { message }
        } else {
            NotDeliveredReason::Rejected { code, message }
        };
        return SteerResolution::NotDelivered { reason };
    }
    match wire {
        SteerWire::Goose => SteerResolution::Injected {
            wire,
            native_run_id,
        },
        SteerWire::AcpExtension => {
            let outcome = msg.pointer("/result/outcome");
            match outcome.and_then(|v| v.as_str()) {
                Some(STEER_OUTCOME_INJECTED) => SteerResolution::Injected {
                    wire,
                    native_run_id: None,
                },
                Some(STEER_OUTCOME_STARTED_NEW_TURN) => SteerResolution::StartedNewTurn { wire },
                Some(STEER_OUTCOME_PROMPT_REQUIRED) => SteerResolution::NotDelivered {
                    reason: NotDeliveredReason::PromptRequired,
                },
                Some(STEER_OUTCOME_FAILED) => SteerResolution::Unknown {
                    reason: UnknownReason::AdapterReportedFailure {
                        outcome: STEER_OUTCOME_FAILED.to_owned(),
                    },
                    wire_request_id: Some(request_id),
                },
                _ => {
                    // Report the raw string when there is one, so logs read
                    // `weird` not `"weird"`; fall back to the JSON for a
                    // non-string value.
                    let reported = match outcome {
                        None => "<absent>".to_owned(),
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(other) => other.to_string(),
                    };
                    SteerResolution::Unknown {
                        reason: UnknownReason::UnrecognizedAck { outcome: reported },
                        wire_request_id: Some(request_id),
                    }
                }
            }
        }
    }
}

/// How the prompt read loop is leaving while a steer is still unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptExit {
    /// The prompt's own response or error arrived; the reader is healthy.
    Answered,
    /// A turn deadline fired; the reader is healthy.
    Deadline,
    /// The runtime's stdout closed.
    Eof,
    /// The reader itself failed; nothing more can be read.
    ReadError,
}

fn build_client_capabilities() -> serde_json::Value {
    serde_json::json!({
        // Signal to ACP adapters that Buzz can hand users to terminal-native
        // auth flows. Adapters decide which auth methods to expose; Buzz does
        // not hardcode vendor login commands from this capability.
        "auth": {
            "terminal": true
        },
        // Signal to goose that we handle `_goose/unstable/session/update`
        // notifications. Without this the custom notification is suppressed
        // on goose's side and usage data is never emitted.
        "_meta": {
            "goose": {
                "customNotifications": true
            },
            // Non-standard extension used by claude-agent-acp to advertise the
            // exact terminal login argv for subscription auth. Unknown `_meta`
            // keys are ignored by other adapters.
            "terminal-auth": true,
            // claude-agent-acp strips `text` and `thinking` blocks out of a
            // subagent's `session/update` frames unless the client declares
            // this capability (its `supportsSubagentTranscript`, which tests
            // `_meta["subagent-transcript"] === true` exactly). Undeclared, a
            // subagent that reasons for minutes without calling a tool puts
            // *nothing* on the wire, so the turn's only activity is the
            // `tool_call` that launched it and the idle deadline counts down
            // through work that is progressing normally. Declaring it makes the
            // subagent's work visible to the operator and legible to the timer.
            "subagent-transcript": true
        }
    })
}

/// Environment variables an agent subprocess must not receive.
///
/// A spawn inherits the parent environment wholesale — that is correct for the
/// managed-agent harness, where the agent is a Buzz participant meant to act as
/// itself, and wrong for a host that merely supervises an agent it does not
/// want speaking in its name. This type is how such a host says so.
///
/// Policy lives with the caller: the fence carries no defaults, and
/// [`OPEN`](Self::OPEN) — the fence that stops nothing — is what every existing
/// call site gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvFence {
    /// Exact variable names, removed whether or not this process has them set.
    /// Removing an absent key is a no-op, so an enumerated fence does not
    /// depend on how the host happened to be launched.
    pub keys: &'static [&'static str],
    /// Name prefixes. Every variable under one is removed, *including names
    /// nobody has invented yet* — the property an enumerated list cannot
    /// offer, and the reason a fence over a namespace the caller owns is worth
    /// more than a list of the credentials it currently holds.
    pub prefixes: &'static [&'static str],
    /// Names that survive a matching prefix. Adding one should be a
    /// one-line, reviewable exception, not a reason to weaken a prefix.
    pub exempt: &'static [&'static str],
}

/// Everything [`AcpClient::spawn_bounded`] needs to start a project
/// execution: a verified boundary, the resolved environment and the working
/// directory.
///
/// The boundary is not optional: a launch that holds a `BoundedLaunch` runs
/// inside the boundary it names, and [`crate::exec_boundary::prepare`] is the
/// only way to obtain one. Where no backend exists, callers keep their
/// existing unbounded spawn and disclose it as not enforced; they cannot
/// build a `BoundedLaunch` that silently runs unconfined.
#[derive(Debug, Clone)]
pub struct BoundedLaunch {
    boundary: crate::exec_boundary::PreparedBoundary,
    env: crate::exec_env::ResolvedEnv,
    cwd: std::path::PathBuf,
}

impl BoundedLaunch {
    /// Bind a verified boundary, the child's complete environment and its
    /// working directory.
    #[must_use]
    pub fn new(
        boundary: crate::exec_boundary::PreparedBoundary,
        env: crate::exec_env::ResolvedEnv,
        cwd: std::path::PathBuf,
    ) -> Self {
        Self { boundary, env, cwd }
    }

    /// The boundary this launch runs inside.
    #[must_use]
    pub fn boundary(&self) -> &crate::exec_boundary::PreparedBoundary {
        &self.boundary
    }

    /// The child's complete environment.
    #[must_use]
    pub fn env(&self) -> &crate::exec_env::ResolvedEnv {
        &self.env
    }

    /// The child's working directory.
    #[must_use]
    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }
}

impl EnvFence {
    /// The fence that stops nothing — full environment inheritance.
    pub const OPEN: Self = Self {
        keys: &[],
        prefixes: &[],
        exempt: &[],
    };

    /// Whether `key` is fenced.
    pub fn covers(&self, key: &str) -> bool {
        if self.exempt.contains(&key) {
            return false;
        }
        self.keys.contains(&key) || self.prefixes.iter().any(|prefix| key.starts_with(*prefix))
    }

    /// Whether this fence stops anything at all.
    pub fn is_open(&self) -> bool {
        self.keys.is_empty() && self.prefixes.is_empty()
    }

    /// Remove every fenced variable from `cmd`'s child environment.
    ///
    /// The enumerated keys go unconditionally; the prefix rules are resolved
    /// against this process's own environment, because those are the values
    /// the child would otherwise inherit.
    fn apply(&self, cmd: &mut tokio::process::Command) {
        if self.is_open() {
            return;
        }
        for key in self.keys {
            if !self.exempt.contains(key) {
                cmd.env_remove(key);
            }
        }
        if self.prefixes.is_empty() {
            return;
        }
        for (key, _) in std::env::vars_os() {
            if self.covers(&key.to_string_lossy()) {
                cmd.env_remove(&key);
            }
        }
    }
}

impl AcpClient {
    /// Kill the agent subprocess and wait for it to exit (no zombies).
    ///
    /// `Drop` only calls `start_kill()` (sends SIGKILL but doesn't reap).
    /// Call this when you need guaranteed cleanup — e.g., in `run_models`
    /// before process exit.
    /// The agent child's pid, while it is still running.
    ///
    /// The only thing a *caller* needs to end that child itself: a provider
    /// aborting a wedged actor cannot await the actor's own `shutdown`, and
    /// must still kill the child's process group and watch it go before it
    /// lets anything else touch that seat's checkout (Beekeeper ledger 227,
    /// the A8.1 amendment). `None` once the child has been reaped.
    #[must_use]
    pub fn child_pid(&self) -> Option<u32> {
        self.child.id()
    }

    pub async fn shutdown(&mut self) {
        // Kill the entire process group when possible. The child was spawned
        // with process_group(0), so its PID == its PGID. Killing the group
        // ensures subprocesses (MCP servers, tool processes) are cleaned up
        // rather than orphaned to init.
        //
        // Falls back to start_kill() (direct child only) on non-Unix or if
        // the child has been polled to completion (id() returns None).
        match self.child.id() {
            Some(pid) if kill_process_group(pid) => {}
            _ => {
                let _ = self.child.start_kill();
            }
        }
        // Bounded wait: if the child doesn't exit within 5s after SIGKILL,
        // give up and let Drop/OS handle it. An unbounded wait here would
        // wedge the harness during respawn or shutdown if a child is stuck.
        match tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::debug!("child wait error after kill: {e}"),
            Err(_) => tracing::warn!("child did not exit within 5s after SIGKILL — abandoning"),
        }
    }

    /// Spawn the agent binary as a subprocess and connect to its stdio pipes.
    ///
    /// `has_generated_codex_config` must be true when `codex_network_env()` successfully
    /// injected a `CODEX_CONFIG` entry into `extra_env`.  The spawn path uses it to
    /// trigger the recursive merge + forced `network_access=true` in
    /// `build_codex_config_env`.  Pass `false` for test spawns and non-Codex agents.
    ///
    /// The child inherits this process's entire environment; callers that hold
    /// credentials the agent must not see want
    /// [`spawn_with_env_fence`](Self::spawn_with_env_fence) instead.
    ///
    /// After spawning, call [`initialize`](Self::initialize) before any other method.
    pub async fn spawn(
        command: &str,
        args: &[String],
        extra_env: &[(String, String)],
        has_generated_codex_config: bool,
    ) -> Result<Self, AcpError> {
        // The open fence: the managed-agent harness *wants* its agent to
        // inherit `BUZZ_PRIVATE_KEY` and friends, because a managed agent is a
        // Buzz participant acting as itself.
        Self::spawn_with_env_fence(
            command,
            args,
            extra_env,
            has_generated_codex_config,
            &EnvFence::OPEN,
        )
        .await
    }

    /// Like [`spawn`](Self::spawn), but with `fence` applied to the child's
    /// environment.
    ///
    /// Opt-in by design. Inheriting the parent environment is right for the
    /// managed-agent harness and wrong for hosts that merely *supervise* an
    /// agent — a coding-session sidecar signs provider-authoritative events
    /// with a key its agent has no business holding. Rather than guess which
    /// caller is which, the fence is a parameter and the default is unchanged.
    pub async fn spawn_with_env_fence(
        command: &str,
        args: &[String],
        extra_env: &[(String, String)],
        has_generated_codex_config: bool,
        fence: &EnvFence,
    ) -> Result<Self, AcpError> {
        Self::spawn_with_env_fence_and_overrides(
            command,
            args,
            extra_env,
            has_generated_codex_config,
            fence,
            &[],
        )
        .await
    }

    /// Like [`spawn_with_env_fence`](Self::spawn_with_env_fence), but with
    /// `post_fence_env` applied **after** the fence has run.
    ///
    /// The fence is deliberately last in the ordinary path: skipping injection
    /// is not enough on its own, because inheritance is the leak. That makes
    /// the fence unconditional, which is right for every variable a
    /// *supervised* agent must not hold — and wrong for the one case where the
    /// host is deliberately handing the child a different identity than its
    /// own. A coding-session seat is that case: the sidecar's `BUZZ_*` must
    /// still be stripped, and then the seat's own credentials must be put back,
    /// which no fence configuration can express.
    ///
    /// So it is a separate argument rather than an [`EnvFence::exempt`] entry:
    /// exempting `BUZZ_PRIVATE_KEY` would let the *parent's* value be inherited
    /// (the injection loop yields to an already-set parent variable), which is
    /// exactly the forgery the fence exists to prevent. Overrides are set
    /// explicitly, so the child gets the value the caller named or nothing.
    ///
    /// `post_fence_env` bypasses every rule above it — the fence, the
    /// operator-precedence check, and the per-runtime defaults. Callers own
    /// that: pass only values the child is *meant* to hold.
    pub async fn spawn_with_env_fence_and_overrides(
        command: &str,
        args: &[String],
        extra_env: &[(String, String)],
        has_generated_codex_config: bool,
        fence: &EnvFence,
        post_fence_env: &[(String, String)],
    ) -> Result<Self, AcpError> {
        let cmd = Self::build_agent_command(
            command,
            args,
            extra_env,
            has_generated_codex_config,
            fence,
            post_fence_env,
        )?;
        Self::from_command(cmd, command)
    }

    /// Spawn `command args…` inside a host-prepared execution boundary, with
    /// exactly the environment the host resolved.
    ///
    /// Nothing is inherited: the launch's environment is the child's whole
    /// environment ([`crate::exec_env::ResolvedEnv`]), the process starts in
    /// the launch's working directory, and the argv is wrapped so the adapter
    /// and every process it starts run under the launch's boundary.
    pub async fn spawn_bounded(
        command: &str,
        args: &[String],
        launch: &BoundedLaunch,
    ) -> Result<Self, AcpError> {
        let cmd = Self::build_bounded_command(command, args, launch);
        Self::from_command(cmd, command)
    }

    /// Assemble a bounded child `Command` without spawning it.
    fn build_bounded_command(
        command: &str,
        args: &[String],
        launch: &BoundedLaunch,
    ) -> tokio::process::Command {
        use std::process::Stdio;

        let (program, argv) = launch.boundary.wrap(command, args);
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(argv)
            .current_dir(&launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        launch.env.apply_to(&mut cmd);
        #[cfg(unix)]
        cmd.process_group(0);
        configure_no_window(&mut cmd);
        cmd
    }

    /// Spawn an assembled command and wire up its stdio.
    fn from_command(mut cmd: tokio::process::Command, command: &str) -> Result<Self, AcpError> {
        let standard_adapter =
            match crate::config::normalize_agent_command_identity(command).as_str() {
                "claude-agent-acp" | "claude-code-acp" | "claude-code" | "claudecode" => {
                    Some(StandardAdapterKind::Claude)
                }
                "codex" | "codex-acp" => Some(StandardAdapterKind::Codex),
                _ => None,
            };
        let mut child = cmd.spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AcpError::Protocol("failed to open agent stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AcpError::Protocol("failed to open agent stdout".into()))?;

        // Absent stderr is not fatal: the adapter is still usable, we just have
        // no diagnostics from it. Failing the spawn over a missing diagnostic
        // channel would trade a working session for a better error message.
        let stderr_tail = std::sync::Arc::new(StderrTail::default());
        match child.stderr.take() {
            Some(stderr) => drain_stderr(stderr, std::sync::Arc::clone(&stderr_tail)),
            None => tracing::warn!(
                target: "acp::stderr",
                "agent stderr was not captured; its diagnostics are unavailable"
            ),
        }

        Ok(Self {
            child,
            stdin,
            reader: FramedRead::new(stdout, LinesCodec::new_with_max_length(MAX_LINE_SIZE)),
            next_id: 0,
            pending_permission_id: None,
            permission_responded: false,
            last_prompt_id: None,
            current_hard_deadline: None,
            observer: None,
            observer_agent_index: None,
            observer_context: ObserverContext::default(),
            active_run_id: None,
            steering_supported: false,
            prompt_image_supported: false,
            session_load_supported: false,
            session_resume_supported: false,
            steer_rx: None,
            late_steer_sink: None,
            unresolved_steers: UnresolvedSteers::default(),
            goose_usage: UsageTracker::default(),
            standard_usage: StandardUsageTracker::default(),
            standard_adapter,
            stderr_tail,
            answer_stall_timeout: None,
            emit_raw_sdk_frames: false,
            disallowed_tools: Vec::new(),
            claude_options: serde_json::Map::new(),
            agent_name: "unknown".to_owned(),
            agent_version: None,
            protocol_version: 1,
            turn_clock: std::sync::Arc::new(SystemTurnClock),
        })
    }

    /// Replace the clock this client's turn deadlines are measured against.
    ///
    /// Test-only: production always runs on [`SystemTurnClock`].
    #[cfg(test)]
    fn set_turn_clock(&mut self, clock: std::sync::Arc<dyn TurnClock>) {
        self.turn_clock = clock;
    }

    /// Assemble the child `Command` without spawning it.
    ///
    /// Split out from the spawn so the environment it hands the child is
    /// inspectable by tests: `Command::get_envs` reports both the keys we set
    /// and the keys we removed, which is the only way to assert the fence
    /// without running an adapter.
    fn build_agent_command(
        command: &str,
        args: &[String],
        extra_env: &[(String, String)],
        has_generated_codex_config: bool,
        fence: &EnvFence,
        post_fence_env: &[(String, String)],
    ) -> Result<tokio::process::Command, AcpError> {
        use std::process::Stdio;

        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Piped, not inherited. Inheriting put the adapter's diagnostics on
            // the harness's own stderr, which is fine when a person is watching
            // a terminal and useless everywhere else — the desktop app writes
            // no log file, so the one artifact that explains a failed session
            // went nowhere. The reader task below re-emits every line through
            // `tracing`, so a terminal still shows them, and keeps the tail for
            // the failure report.
            .stderr(Stdio::piped())
            // Ensure the child is killed when the AcpClient is dropped (best-effort).
            // Callers MUST still call shutdown().await for guaranteed cleanup.
            .kill_on_drop(true);

        // Per-persona env vars (e.g., GOOSE_PROVIDER, BUZZ_AGENT_PROVIDER).
        // For most keys, operator precedence wins: skip injection if already set
        // in the parent environment.
        //
        // CODEX_CONFIG is handled specially via build_codex_config_env:
        //   • has_generated_codex_config=true: merge all CODEX_CONFIG entries + parent
        //     recursively and force network_access=true.
        //   • has_generated_codex_config=false: return None; any persona-supplied
        //     CODEX_CONFIG falls through to the normal operator-wins loop below.
        let has_codex_config = extra_env.iter().any(|(k, _)| k == "CODEX_CONFIG");
        let parent_codex_config = if has_generated_codex_config && has_codex_config {
            std::env::var("CODEX_CONFIG").ok()
        } else {
            None
        };
        let codex_config_value = build_codex_config_env(
            extra_env,
            parent_codex_config.as_deref(),
            has_generated_codex_config,
        )?;
        // When the merge path was not taken (None returned), any persona CODEX_CONFIG
        // entry falls through to the standard operator-wins treatment below.
        let codex_merge_active = codex_config_value.is_some();

        // Per-runtime environment defaults (e.g. Hermes MCP-startup isolation).
        // Applied first so both persona `extra_env` (below, via `Command::env`
        // key replacement) and inherited parent env (via the parent-presence
        // check) override them.
        for &(key, value) in crate::config::default_agent_env(command) {
            if fence.covers(key) {
                continue;
            }
            if std::env::var_os(key).is_none() {
                cmd.env(key, value);
            }
        }

        for (key, value) in extra_env {
            if key == "CODEX_CONFIG" && codex_merge_active {
                // Handled by build_codex_config_env; skip here to avoid double-setting.
                continue;
            }
            // The fence outranks an explicit value. A fenced key appearing in
            // `extra_env` is a configuration mistake, and failing closed is the
            // only reading of it that cannot leak.
            if fence.covers(key) {
                continue;
            }
            if std::env::var_os(key).is_none() {
                cmd.env(key, value);
            }
        }
        if let Some(merged) = codex_config_value {
            if !fence.covers("CODEX_CONFIG") {
                cmd.env("CODEX_CONFIG", merged);
            }
        }

        // Spawn the agent in its own process group so SIGKILL doesn't propagate
        // to the harness's own process group on Unix.
        // tokio::process::Command::process_group is a stable tokio API (no extra imports needed).
        #[cfg(unix)]
        cmd.process_group(0);

        // Suppress the console window that Windows otherwise allocates for every
        // console-subsystem child process spawned from a GUI/non-console parent.
        configure_no_window(&mut cmd);

        // The fence, last. Skipping injection above is not enough on its own:
        // the parent-presence checks deliberately leave *inherited* values
        // alone, and inheritance is the whole leak. `env_remove` is what
        // reaches those.
        fence.apply(&mut cmd);

        // After the fence, and only what the caller named. An empty slice —
        // every existing call site — leaves the command byte-for-byte what it
        // was before this parameter existed.
        for (key, value) in post_fence_env {
            cmd.env(key, value);
        }

        Ok(cmd)
    }

    /// Attach a local observer feed to this ACP client.
    pub fn set_observer(&mut self, observer: Option<ObserverHandle>, agent_index: usize) {
        self.observer = observer;
        self.observer_agent_index = Some(agent_index);
    }

    /// Update metadata that will be attached to subsequent raw wire events.
    pub fn set_observer_context(&mut self, context: ObserverContext) {
        self.observer_context = context;
    }

    /// Return a clone of the observer handle, if attached.
    pub(crate) fn observer_handle(&self) -> Option<ObserverHandle> {
        self.observer.clone()
    }

    /// Return the pool slot index for this agent process.
    pub(crate) fn observer_agent_index(&self) -> Option<usize> {
        self.observer_agent_index
    }

    /// Emit a semantic event to the local observer feed, if enabled.
    pub fn observe(&self, kind: impl Into<String>, payload: serde_json::Value) {
        if let Some(observer) = &self.observer {
            observer.emit(
                kind,
                self.observer_agent_index,
                &self.observer_context,
                payload,
            );
        }
    }

    /// Send the `initialize` request and return the agent's response result value.
    ///
    /// Must be called exactly once, before any other ACP method.
    /// The caller may inspect `agentCapabilities` in the returned value.
    ///
    /// Records `_meta.steering.supported` into
    /// [`steering_supported`](Self::steering_supported) so the read loop's steer
    /// arm can choose [`ACP_STEER_METHOD`] for adapters that implement it.
    /// Parsed here rather than at each call site so no caller can forget it.
    ///
    /// The adapter's identity and protocol version are recorded the same way,
    /// into [`agent_name`](Self::agent_name) and
    /// [`protocol_version`](Self::protocol_version), so any caller can reach the
    /// system-prompt capability gates without re-parsing the response.
    pub async fn initialize(&mut self) -> Result<serde_json::Value, AcpError> {
        // Requesting version 2 is an intentional temporary pin — we are squatting
        // on ACP v2 ahead of the upstream ACP RFD. Revisit when that RFD merges.
        let params = build_initialize_params();
        let result = self.send_request("initialize", params).await?;
        self.steering_supported = result
            .pointer("/_meta/steering/supported")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // Absent means false: an agent that does not say it takes images is one
        // we must not send them to. `buzz-agent` fails the whole turn on a
        // block it did not advertise, so guessing here is not a soft failure.
        self.prompt_image_supported = result
            .pointer("/agentCapabilities/promptCapabilities/image")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        self.session_load_supported = result
            .pointer("/agentCapabilities/loadSession")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        self.session_resume_supported = result
            .pointer("/agentCapabilities/sessionCapabilities/resume")
            .is_some_and(|value| !value.is_null() && value != &serde_json::Value::Bool(false));
        self.agent_name = normalized_agent_name(&result);
        self.agent_version = reported_agent_version(&result);
        self.protocol_version = result["protocolVersion"].as_u64().unwrap_or(1) as u32;
        tracing::debug!(target: "acp::init", "initialize response: {result}");
        Ok(result)
    }

    /// Send the ACP `authenticate` request for an adapter-advertised method.
    pub async fn authenticate(&mut self, method_id: &str) -> Result<serde_json::Value, AcpError> {
        let params = serde_json::json!({
            "methodId": method_id,
        });
        self.send_request("authenticate", params).await
    }

    /// Send `session/new` and return the full response alongside the session ID.
    ///
    /// `cwd` must be an absolute path. `mcp_servers` may be empty.
    ///
    /// `system_prompt` controls how the prompt text is delivered:
    ///
    /// - `None` — no system-prompt field in the request (legacy framing).
    /// - `Some(SystemPromptTransport::Field(text))` — bare `systemPrompt` field
    ///   (ACP protocol v2, buzz-agent, goose unused).
    /// - `Some(SystemPromptTransport::ClaudeMeta(text))` — `_meta.systemPrompt`
    ///   as `{"append": text}`, keeping claude-agent-acp's native preset intact.
    ///
    /// `session_title` rides in `_meta.sessionTitle` when `Some`; `_meta` is
    /// omitted entirely otherwise, since adapters may distinguish an absent
    /// member from a null one. When both `ClaudeMeta` and `session_title` are
    /// present the two `_meta` members are merged into a single object.
    ///
    /// Callers use [`extract_model_config_options`] and [`extract_model_state`]
    /// to pull model info from the raw result.
    pub async fn session_new_full(
        &mut self,
        cwd: &str,
        mcp_servers: Vec<McpServer>,
        system_prompt: Option<SystemPromptTransport<'_>>,
        session_title: Option<&str>,
    ) -> Result<SessionNewResponse, AcpError> {
        let mut params = serde_json::json!({
            "cwd": cwd,
            "mcpServers": mcp_servers,
        });
        match system_prompt {
            Some(SystemPromptTransport::Field(sp)) => {
                params["systemPrompt"] = serde_json::Value::String(sp.to_owned());
            }
            Some(SystemPromptTransport::ClaudeMeta(sp)) => {
                // Merge into _meta so sessionTitle (set below) is not clobbered.
                params["_meta"]["systemPrompt"] = serde_json::json!({ "append": sp });
            }
            None => {}
        }
        if let Some(title) = session_title {
            // Merge — _meta may already carry systemPrompt from ClaudeMeta above.
            params["_meta"]["sessionTitle"] = serde_json::Value::String(title.to_owned());
        }
        self.apply_raw_sdk_frames_meta(&mut params);
        self.apply_disallowed_tools_meta(&mut params);
        self.apply_claude_options_meta(&mut params);
        let result = self.send_request("session/new", params).await?;
        let session_id = result["sessionId"]
            .as_str()
            .ok_or_else(|| AcpError::Protocol("session/new response missing sessionId".into()))?
            .to_owned();
        // A session this client just created has spent nothing: its first
        // turn's cumulative cost is measured from zero. Seeded here, for every
        // caller, because the session provider opens sessions through this
        // method and never reaches the pool's `notify_session_spawned`
        // (ledger 272(d)); the pool's later call is then a no-op. A resume or
        // load is never seeded — that conversation may already have spent.
        self.notify_session_spawned(&session_id);
        tracing::info!(target: "acp::session", "session created");
        Ok(SessionNewResponse {
            session_id,
            raw: result,
        })
    }

    /// Resume an existing ACP session without requesting transcript replay.
    ///
    /// Callers must gate this on [`session_resume_supported`](Self::session_resume_supported).
    /// The returned session id is the opaque cursor supplied by the caller;
    /// ACP resume responses do not repeat it.
    pub async fn session_resume_full(
        &mut self,
        session_id: &str,
        cwd: &str,
        mcp_servers: Vec<McpServer>,
    ) -> Result<SessionNewResponse, AcpError> {
        // A reattachment re-enters the conversation on a fresh adapter
        // process, and the adapter rebuilds the SDK query from this request's
        // `_meta` — so the denial has to be restated or the fence lapses at
        // the first resume.
        let mut params = serde_json::json!({
            "sessionId": session_id,
            "cwd": cwd,
            "mcpServers": mcp_servers,
        });
        self.apply_raw_sdk_frames_meta(&mut params);
        self.apply_disallowed_tools_meta(&mut params);
        self.apply_claude_options_meta(&mut params);
        let result = self.send_request("session/resume", params).await?;
        tracing::info!(target: "acp::session", "session resumed");
        Ok(SessionNewResponse {
            session_id: session_id.to_owned(),
            raw: result,
        })
    }

    /// Load an existing ACP session, allowing the adapter to replay its history.
    ///
    /// Callers must gate this on [`session_load_supported`](Self::session_load_supported).
    /// Observer consumers should subscribe only after this call if replayed
    /// updates are already represented in their own durable transcript.
    pub async fn session_load_full(
        &mut self,
        session_id: &str,
        cwd: &str,
        mcp_servers: Vec<McpServer>,
    ) -> Result<SessionNewResponse, AcpError> {
        // See `session_resume_full`: the adapter rebuilds the session from
        // this request's `_meta`, so the denial is restated here too.
        let mut params = serde_json::json!({
            "sessionId": session_id,
            "cwd": cwd,
            "mcpServers": mcp_servers,
        });
        self.apply_raw_sdk_frames_meta(&mut params);
        self.apply_disallowed_tools_meta(&mut params);
        self.apply_claude_options_meta(&mut params);
        let result = self.send_request("session/load", params).await?;
        tracing::info!(target: "acp::session", "session loaded");
        Ok(SessionNewResponse {
            session_id: session_id.to_owned(),
            raw: result,
        })
    }

    /// Send `session/new` and return only the `sessionId` string.
    ///
    /// Convenience wrapper around [`session_new_full`].
    #[allow(dead_code)] // Public API — callers outside the harness may use this.
    pub async fn session_new(
        &mut self,
        cwd: &str,
        mcp_servers: Vec<McpServer>,
        system_prompt: Option<SystemPromptTransport<'_>>,
        session_title: Option<&str>,
    ) -> Result<String, AcpError> {
        Ok(self
            .session_new_full(cwd, mcp_servers, system_prompt, session_title)
            .await?
            .session_id)
    }

    /// Replace Goose's native system prompt after `session/new`.
    pub async fn session_set_goose_system_prompt(
        &mut self,
        session_id: &str,
        text: &str,
    ) -> Result<serde_json::Value, AcpError> {
        self.send_request(
            "_goose/unstable/session/system-prompt/set",
            serde_json::json!({
                "sessionId": session_id,
                "mode": "set",
                "key": "buzz",
                "text": text,
            }),
        )
        .await
    }

    /// Send `session/set_config_option` (stable ACP path).
    pub async fn session_set_config_option(
        &mut self,
        session_id: &str,
        config_id: &str,
        value: &str,
    ) -> Result<serde_json::Value, AcpError> {
        self.session_set_config_value(session_id, config_id, serde_json::Value::from(value))
            .await
    }

    /// Send `session/set_config_option` with any JSON value — a native
    /// boolean for a `type: "boolean"` option, a string for a select.
    pub async fn session_set_config_value(
        &mut self,
        session_id: &str,
        config_id: &str,
        value: serde_json::Value,
    ) -> Result<serde_json::Value, AcpError> {
        let params = serde_json::json!({
            "sessionId": session_id,
            "configId": config_id,
            "value": value,
        });
        self.send_request("session/set_config_option", params).await
    }

    /// Send `session/set_model` (unstable ACP path).
    pub async fn session_set_model(
        &mut self,
        session_id: &str,
        model_id: &str,
    ) -> Result<serde_json::Value, AcpError> {
        let params = serde_json::json!({
            "sessionId": session_id,
            "modelId": model_id,
        });
        self.send_request("session/set_model", params).await
    }

    /// Send `session/set_mode` (ACP session modes).
    pub async fn session_set_mode(
        &mut self,
        session_id: &str,
        mode_id: &str,
    ) -> Result<serde_json::Value, AcpError> {
        let params = serde_json::json!({
            "sessionId": session_id,
            "modeId": mode_id,
        });
        self.send_request("session/set_mode", params).await
    }

    /// Send `session/prompt` with idle-based timeout instead of wall-clock.
    ///
    /// The idle deadline resets on any stdout activity from the agent. The hard
    /// deadline is an absolute wall-clock cap (safety valve).
    pub async fn session_prompt_with_idle_timeout(
        &mut self,
        session_id: &str,
        prompt_text: &str,
        idle_timeout: std::time::Duration,
        max_duration: std::time::Duration,
    ) -> Result<StopReason, AcpError> {
        self.session_prompt_blocks_with_idle_timeout(
            session_id,
            std::slice::from_ref(&prompt_text),
            idle_timeout,
            max_duration,
        )
        .await
    }

    /// Like [`session_prompt_with_idle_timeout`](Self::session_prompt_with_idle_timeout),
    /// but sends each entry in `prompt_blocks` as a separate text content block.
    ///
    /// Used for slash-command pass-through: ACP connectors detect commands via
    /// the **first** block's text starting with `/`, so the harness sends
    /// `["/cmd args", "<buzz context>"]` instead of one wrapped block.
    pub async fn session_prompt_blocks_with_idle_timeout(
        &mut self,
        session_id: &str,
        prompt_blocks: &[&str],
        idle_timeout: std::time::Duration,
        max_duration: std::time::Duration,
    ) -> Result<StopReason, AcpError> {
        let blocks: Vec<PromptBlock> = prompt_blocks
            .iter()
            .map(|text| PromptBlock::Text((*text).to_owned()))
            .collect();
        self.session_prompt_content_with_idle_timeout(
            session_id,
            &blocks,
            idle_timeout,
            max_duration,
        )
        .await
    }

    /// Like [`session_prompt_blocks_with_idle_timeout`](Self::session_prompt_blocks_with_idle_timeout),
    /// but sends typed content blocks so a caller can attach images.
    ///
    /// Callers must gate image blocks on
    /// [`prompt_image_supported`](Self::prompt_image_supported); this method
    /// sends what it is given.
    pub async fn session_prompt_content_with_idle_timeout(
        &mut self,
        session_id: &str,
        prompt_blocks: &[PromptBlock],
        idle_timeout: std::time::Duration,
        max_duration: std::time::Duration,
    ) -> Result<StopReason, AcpError> {
        let params = build_prompt_params(session_id, prompt_blocks);
        let hard_deadline = self.turn_clock.now() + max_duration;
        self.current_hard_deadline = Some(hard_deadline);

        // Mark the usage tracker as in-flight for this turn BEFORE sending the
        // prompt so that any setup notifications recorded earlier are not
        // misattributed to this turn.
        self.goose_usage.begin_turn(session_id);
        self.standard_usage.begin_turn(session_id);

        self.last_prompt_id = Some(self.next_id);
        let id = self.next_id;
        self.next_id += 1;

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/prompt",
            "params": params,
        });

        tracing::debug!(target: "acp::wire", "→ {}", &serde_json::to_string(&msg).unwrap_or_default());
        if let Err(e) = self.write_ndjson(&msg).await {
            self.last_prompt_id = None;
            self.current_hard_deadline = None;
            return Err(e);
        }

        let result = self
            .read_until_response_with_idle_timeout(
                session_id,
                id,
                idle_timeout,
                hard_deadline,
                max_duration,
            )
            .await;

        // On timeout errors, leave current_hard_deadline set so cancel_with_cleanup
        // can inherit the remaining budget. Clear it on all other outcomes.
        match &result {
            Ok(_) => {
                self.last_prompt_id = None;
                self.current_hard_deadline = None;
            }
            Err(
                AcpError::IdleTimeout { .. }
                | AcpError::HardTimeout { .. }
                | AcpError::AnswerStall { .. },
            ) => {
                // Leave last_prompt_id and current_hard_deadline set —
                // caller will invoke cancel_with_cleanup. An answer stall needs
                // this most of all: the cancel is not cleanup there, it is the
                // adapter's own way out of the hold, and it cannot be sent
                // against a prompt id we have already forgotten.
            }
            Err(_) => {
                self.last_prompt_id = None;
                self.current_hard_deadline = None;
            }
        }
        self.parse_prompt_response(session_id, &result?)
    }

    /// Send a `session/cancel` **notification** (no `id` field, no response expected).
    ///
    /// After calling this, the agent will eventually respond to the in-flight
    /// `session/prompt` with `stopReason: "cancelled"`. Use
    /// [`cancel_with_cleanup`](Self::cancel_with_cleanup) if you need to drain
    /// that response.
    ///
    /// Note: async because writing to stdin requires async I/O.
    pub async fn session_cancel(&mut self, session_id: &str) -> Result<(), AcpError> {
        let params = serde_json::json!({
            "sessionId": session_id,
        });
        self.send_notification("session/cancel", params).await
    }

    /// Returns `true` if a `session/prompt` request is currently in flight.
    pub fn has_in_flight_prompt(&self) -> bool {
        self.last_prompt_id.is_some()
    }

    /// Most recently observed goose `_meta.goose.activeRunId` from a
    /// `session_info_update`, if any.
    ///
    /// Both goose and buzz-agent emit `session_info_update`; other agents
    /// leave this `None` for the lifetime of the client. Read directly by
    /// `read_until_response_with_idle_timeout`'s
    /// steer arm at write time (see [`crate::pool::SteerRequest`] for
    /// why the read loop owns this); production callers do not need this
    /// accessor. Kept as `pub` so tests can introspect the field.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn active_run_id(&self) -> Option<&str> {
        self.active_run_id.as_deref()
    }

    /// Whether the agent advertised `promptCapabilities.image` at `initialize`.
    ///
    /// A caller must consult this before putting a [`PromptBlock::Image`] on
    /// the wire, and must tell the operator when it drops one — an image that
    /// silently never reached the agent looks exactly like an agent that
    /// ignored it.
    pub fn prompt_image_supported(&self) -> bool {
        self.prompt_image_supported
    }

    /// Whether the agent advertised the [`ACP_STEER_METHOD`] extension at
    /// `initialize` time (`_meta.steering.supported`).
    ///
    /// The read loop's steer arm reads the field directly; this accessor exists
    /// for the supervisor's post-initialize log line.
    pub fn steering_supported(&self) -> bool {
        self.steering_supported
    }

    /// Whether initialization advertised `loadSession: true`.
    pub fn session_load_supported(&self) -> bool {
        self.session_load_supported
    }

    /// Whether initialization advertised `sessionCapabilities.resume`.
    pub fn session_resume_supported(&self) -> bool {
        self.session_resume_supported
    }

    /// Normalized adapter identity recorded at `initialize`.
    ///
    /// `"unknown"` before `initialize` answers, or when the adapter reported no
    /// name. Feed this to [`session_new_system_prompt`] rather than a display
    /// name — the capability gates key on the package identity.
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// Set how long a turn may stay silent after answering before the host
    /// gives up on the adapter resolving it. `None` disables the watch.
    ///
    /// Opt-in rather than defaulted so the managed-agent pool, whose turns have
    /// different shapes and its own supervision, is unaffected until someone
    /// decides it should be.
    /// Ask the adapter to forward raw SDK messages for this session.
    ///
    /// Must be set before `session/new`: the adapter reads the flag once, off
    /// that request's `_meta`, and never re-reads it.
    /// Record one raw SDK frame to the local log, and nowhere else.
    ///
    /// These frames carry `origin.kind` and the whole task lifecycle — the
    /// evidence that would have named the cause of the 2026-08-24 stall. They
    /// also carry the adapter's unredacted internals, so they go to `tracing`
    /// and are deliberately **not** handed to the observer, whose frames become
    /// signed transcript items.
    fn log_raw_sdk_frame(&self, msg: &serde_json::Value) {
        let message = &msg["params"]["message"];
        tracing::info!(
            target: "acp::sdk_frame",
            kind = message.get("type").and_then(|v| v.as_str()).unwrap_or("?"),
            subtype = message.get("subtype").and_then(|v| v.as_str()).unwrap_or(""),
            origin = message.pointer("/origin/kind").and_then(|v| v.as_str()).unwrap_or(""),
            "{message}"
        );
    }

    pub fn set_emit_raw_sdk_frames(&mut self, emit: bool) {
        self.emit_raw_sdk_frames = emit;
    }

    /// Ask claude-agent-acp to deny these tool names for this session.
    ///
    /// The adapter merges the list into the SDK query's `disallowedTools`
    /// (`dist/acp-agent.js:4913` in 0.70.0), so the tools are gone from the
    /// model's toolset rather than merely discouraged. It reads the list off
    /// the `_meta` of the request that opens the session and never re-reads
    /// it, so this must be set **before** `session/new`, `session/resume` or
    /// `session/load`; a later call is inert for the session already open.
    ///
    /// Other adapters ignore the key: it lives under `_meta.claudeCode`, and
    /// codex-acp 1.6.2 offers no per-session tool denial of any kind. Setting
    /// it there is harmless but buys nothing.
    ///
    /// An empty list omits the key.
    pub fn set_disallowed_tools(&mut self, tools: &[&str]) {
        self.disallowed_tools = tools.iter().map(|tool| (*tool).to_owned()).collect();
    }

    /// Ask claude-agent-acp for the raw SDK frames this client reads.
    ///
    /// With the diagnostic flag on, every frame (`true`, unfiltered — the
    /// reason to turn it on is not knowing yet which frames matter). Otherwise
    /// a Claude adapter is asked for its `result` frames only: their
    /// `modelUsage` map is the one place the adapter reports the resolved
    /// model id that actually answered a turn, rather than the picker label
    /// (`default`) the session was opened on (ledger 268(e)). The adapter
    /// reads the key once, off the request that opens the session, so it is
    /// written on `session/new`, `session/resume` and `session/load` alike.
    /// Any other adapter's request is unchanged.
    fn apply_raw_sdk_frames_meta(&self, params: &mut serde_json::Value) {
        if self.emit_raw_sdk_frames {
            params["_meta"]["claudeCode"]["emitRawSDKMessages"] = serde_json::Value::Bool(true);
        } else if self.standard_adapter == Some(StandardAdapterKind::Claude) {
            params["_meta"]["claudeCode"]["emitRawSDKMessages"] =
                serde_json::json!([{ "type": "result" }]);
        }
    }

    /// Handle one `_claude/sdkMessage` frame: record the model a `result`
    /// reports, and log the frame only when the diagnostic flag asked for it.
    ///
    /// Only user-turn results name the turn's model: an autonomous result (a
    /// task-notification followup, a peer/coordinator/observer cycle) did not
    /// answer the turn in flight. "Autonomous" is exactly the adapter's own
    /// set, [`AUTONOMOUS_RESULT_ORIGINS`]; every other origin — `human`,
    /// which claude-agent-acp stamps on every ACP prompt, `channel`, an
    /// absent one, an unknown future kind — is the user's lane, as the
    /// adapter itself routes it (ledger 272(d)). An autonomous result still
    /// advances the session's cumulative `modelUsage` baseline, so its usage
    /// is never attributed to the next user turn.
    fn handle_raw_sdk_frame(&mut self, msg: &serde_json::Value) {
        if self.emit_raw_sdk_frames {
            self.log_raw_sdk_frame(msg);
        }
        if self.standard_adapter != Some(StandardAdapterKind::Claude) {
            return;
        }
        let message = &msg["params"]["message"];
        if message.get("type").and_then(serde_json::Value::as_str) != Some("result") {
            return;
        }
        let autonomous = message
            .pointer("/origin/kind")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|kind| AUTONOMOUS_RESULT_ORIGINS.contains(&kind));
        let Some(session_id) = msg
            .pointer("/params/sessionId")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        self.standard_usage
            .record_sdk_result(session_id, message, !autonomous);
    }

    /// Write the denied-tool list into a request's `_meta`, if there is one.
    ///
    /// Merges rather than assigns: `_meta` may already carry `systemPrompt`,
    /// `sessionTitle` or `claudeCode.emitRawSDKMessages`.
    fn apply_disallowed_tools_meta(&self, params: &mut serde_json::Value) {
        if self.disallowed_tools.is_empty() {
            return;
        }
        params["_meta"]["claudeCode"]["options"]["disallowedTools"] = serde_json::Value::Array(
            self.disallowed_tools
                .iter()
                .map(|tool| serde_json::Value::String(tool.clone()))
                .collect(),
        );
    }

    /// Set host-owned claude-agent-acp session options.
    ///
    /// Written into `_meta.claudeCode.options` on `session/new`,
    /// `session/resume` and `session/load`; the adapter spreads that object
    /// over its own defaults (`dist/acp-agent.js:4868-4870` in 0.70.0), so a
    /// key set here — `settingSources`, `settings` — replaces the adapter's
    /// default for this session. Other adapters ignore the key. Must be set
    /// before the session is opened.
    pub fn set_claude_options(&mut self, options: serde_json::Map<String, serde_json::Value>) {
        self.claude_options = options;
    }

    fn apply_claude_options_meta(&self, params: &mut serde_json::Value) {
        for (key, value) in &self.claude_options {
            params["_meta"]["claudeCode"]["options"][key] = value.clone();
        }
    }

    pub fn set_answer_stall_timeout(&mut self, timeout: Option<std::time::Duration>) {
        self.answer_stall_timeout = timeout;
    }

    /// The tail of the adapter's stderr captured so far.
    pub fn stderr_tail(&self) -> &StderrTail {
        &self.stderr_tail
    }

    /// Adapter build recorded at `initialize`, or `None` if it reported none.
    pub fn agent_version(&self) -> Option<&str> {
        self.agent_version.as_deref()
    }

    /// ACP protocol version recorded at `initialize` (`1` when unreported).
    pub fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    /// Consume per-turn usage for NIP-AM publishing. Goose/buzz-agent is an
    /// exclusive cumulative path; standard ACP prompt usage is used only when
    /// goose emitted nothing for this turn.
    pub fn take_turn_usage(&mut self) -> Option<TurnUsage> {
        let goose_usage = self.goose_usage.take();
        let standard_usage = self.standard_usage.take();
        goose_usage.or(standard_usage)
    }

    /// Notify the usage tracker that buzz-acp just spawned a new session.
    ///
    /// Seeds a zero baseline so the first usage notification for `session_id`
    /// produces `delta_reliable: true` (turn delta == cumulative from zero).
    /// Must be called only when buzz-acp created the session via `session/new`;
    /// never when attaching to a pre-existing session. [`Self::session_new_full`]
    /// calls it itself; an explicit later call is a no-op.
    pub(crate) fn notify_session_spawned(&mut self, session_id: &str) {
        self.goose_usage.seed_zero_baseline(session_id);
        self.standard_usage.seed_zero_baseline(session_id);
    }

    /// Install a per-turn steer request channel for goose-native
    /// non-cancelling mid-turn delivery.
    ///
    /// Called by the dispatch path immediately before
    /// [`session_prompt_with_idle_timeout`] for all prompt tasks.
    /// The matching `Sender` is stored in `TaskMeta.steer_tx` for the
    /// main loop's mode-gate fork to drive.
    ///
    /// Panics if a receiver is already installed — there is exactly one
    /// turn per `AcpClient` at a time, and stacking receivers would
    /// silently misroute steer requests across turns. The previous
    /// turn's receiver must have been consumed by the read loop and
    /// dropped at scope exit before the next turn dispatches.
    pub fn install_steer_rx(&mut self, rx: tokio::sync::mpsc::Receiver<crate::pool::SteerRequest>) {
        assert!(
            self.steer_rx.is_none(),
            "install_steer_rx: previous turn's receiver was not consumed — \
             stacking receivers would misroute steer requests across turns"
        );
        self.steer_rx = Some(SteerSource::Legacy(rx));
    }

    /// Clear any installed steer receiver without consuming it.
    ///
    /// Called by `send_prompt_result` on every exit path of `run_prompt_task`
    /// so that `install_steer_rx`'s `is_none()` invariant holds for the next
    /// dispatch even when the turn ended before the read loop ran `take()`.
    /// Idempotent — safe to call when `steer_rx` is already `None`.
    pub fn clear_steer_rx(&mut self) {
        self.clear_steer_input();
    }

    /// Install the per-turn source of [`SteerInput`]s for the next prompt.
    ///
    /// The read loop of `session_prompt_*` takes inputs from `rx` one at a
    /// time, in order, writes each into the running turn and answers its
    /// `outcome_tx` with what the wire established
    /// (`docs/NATIVE_STEERING_IMPL.md` §3.1). Inputs still in the channel when
    /// the prompt ends are dropped unanswered — the caller reads the closed
    /// oneshot as [`NotDeliveredReason::PromptEndedBeforeWrite`].
    ///
    /// Panics if a source is already installed (from either this or
    /// [`install_steer_rx`](Self::install_steer_rx)): there is exactly one
    /// turn per client at a time, and stacking sources would misroute inputs
    /// across turns. Call [`clear_steer_input`](Self::clear_steer_input) on
    /// every path that ends a turn before the read loop consumed the source.
    pub fn install_steer_input(&mut self, rx: tokio::sync::mpsc::Receiver<SteerInput>) {
        assert!(
            self.steer_rx.is_none(),
            "install_steer_input: previous turn's steer source was not consumed — \
             stacking sources would misroute steer inputs across turns"
        );
        self.steer_rx = Some(SteerSource::Native(rx));
    }

    /// Drop any installed steer source without consuming it. Idempotent.
    ///
    /// Inputs still queued in the dropped channel are never answered; their
    /// closed oneshots are the callers'
    /// [`NotDeliveredReason::PromptEndedBeforeWrite`] signal.
    pub fn clear_steer_input(&mut self) {
        self.steer_rx = None;
    }

    /// Route acknowledgements that arrive after their attempt was resolved
    /// [`SteerResolution::Unknown`] to `tx` as [`LateSteerAck`]s.
    ///
    /// Without a sink such an answer is decoded, logged and dropped; the
    /// attempt is removed from [`unresolved_steer_attempts`] either way.
    ///
    /// [`unresolved_steer_attempts`]: Self::unresolved_steer_attempts
    pub fn set_late_steer_sink(&mut self, tx: tokio::sync::mpsc::UnboundedSender<LateSteerAck>) {
        self.late_steer_sink = Some(tx);
    }

    /// Attempt ids of steer requests that were written and never answered,
    /// oldest first. Each leaves the list when its late acknowledgement is
    /// read by any of this client's read loops.
    pub fn unresolved_steer_attempts(&self) -> Vec<String> {
        self.unresolved_steers.attempt_ids()
    }

    /// If `msg` is the late answer to an unresolved steer request, decode it,
    /// hand it to the late sink and forget the request. Returns whether the
    /// message was consumed.
    ///
    /// Every read loop calls this before treating an unexpected response id
    /// as stray (`docs/NATIVE_STEERING_IMPL.md` §3.1 item 6).
    fn route_late_steer_ack(&mut self, msg: &serde_json::Value) -> bool {
        if msg.get("method").is_some() {
            return false;
        }
        let Some(request_id) = msg.get("id").and_then(|id| id.as_u64()) else {
            return false;
        };
        let Some(entry) = self.unresolved_steers.remove(request_id) else {
            return false;
        };
        let resolution = decode_steer_ack(msg, entry.wire, entry.native_run_id, request_id);
        match &self.late_steer_sink {
            Some(sink) => {
                if sink
                    .send(LateSteerAck {
                        attempt_id: entry.attempt_id.clone(),
                        resolution: resolution.clone(),
                    })
                    .is_err()
                {
                    tracing::warn!(
                        request_id,
                        attempt_id = %entry.attempt_id,
                        ?resolution,
                        "late steer acknowledgement dropped: the late sink is closed"
                    );
                }
            }
            None => tracing::warn!(
                request_id,
                attempt_id = %entry.attempt_id,
                ?resolution,
                "late steer acknowledgement dropped: no late sink installed"
            ),
        }
        true
    }

    /// Returns `true` if no steer receiver is currently installed.
    ///
    /// Test-only: used by `pool` tests to assert the post-return invariant
    /// without exposing the private field directly.
    #[cfg(test)]
    pub fn steer_rx_is_none(&self) -> bool {
        self.steer_rx.is_none()
    }

    /// Cancel a turn cleanly, handling any pending permission request first.
    ///
    /// Steps:
    /// 1. If there is a pending `session/request_permission` that hasn't been
    ///    responded to yet, respond with `outcome: "cancelled"`.
    /// 2. Send `session/cancel` notification (no id).
    /// 3. Continue reading until the `session/prompt` response arrives with `stopReason: "cancelled"`.
    ///
    /// Returns the final [`StopReason`] (almost always [`StopReason::Cancelled`]).
    pub async fn cancel_with_cleanup(
        &mut self,
        session_id: &str,
        _idle_timeout: std::time::Duration,
    ) -> Result<StopReason, AcpError> {
        // Inherit the hard deadline from the timed-out turn so the drain loop
        // doesn't start a fresh timer (prevents double-jeopardy). If the original
        // deadline is already expired or near-expired, grant a 30s floor so the
        // cancel notification has time to propagate and the agent can respond.
        let stored_deadline = self.current_hard_deadline.take();
        let min_cleanup_deadline = self.turn_clock.now() + std::time::Duration::from_secs(30);
        let hard_deadline = match stored_deadline {
            Some(d) if d > min_cleanup_deadline => d,
            Some(_) => {
                tracing::debug!(
                    "original hard deadline expired or near-expired — using 30s cleanup grace"
                );
                min_cleanup_deadline
            }
            None => {
                tracing::warn!(
                    "cancel_with_cleanup called without current_hard_deadline — using 30s fallback"
                );
                min_cleanup_deadline
            }
        };

        self.cancel_with_cleanup_until(session_id, hard_deadline)
            .await
    }

    /// Cancel a user-interrupted turn with a bounded grace window.
    ///
    /// Some ACP servers currently keep streaming after `session/cancel`. For an
    /// explicit Stop button, waiting until the original turn deadline can make
    /// cancellation look broken. This variant gives the agent a short chance to
    /// acknowledge cancellation, then returns a timeout so the caller can respawn
    /// the agent process and actually stop the work.
    ///
    /// The `grace` window is a cleanup deadline, not the turn's real max-turn
    /// wall clock — a bounded drain that expires maps to
    /// [`AcpError::CancelDrainTimeout`], never [`AcpError::HardTimeout`], so
    /// callers can distinguish "agent didn't stop in time" from a genuine
    /// configured hard-cap breach.
    pub async fn cancel_with_cleanup_grace(
        &mut self,
        session_id: &str,
        grace: std::time::Duration,
    ) -> Result<StopReason, AcpError> {
        let _ = self.current_hard_deadline.take();
        let hard_deadline = self.turn_clock.now() + grace;
        match self
            .cancel_with_cleanup_until(session_id, hard_deadline)
            .await
        {
            Err(AcpError::HardTimeout { .. }) => Err(AcpError::CancelDrainTimeout(grace)),
            other => other,
        }
    }

    async fn cancel_with_cleanup_until(
        &mut self,
        session_id: &str,
        hard_deadline: tokio::time::Instant,
    ) -> Result<StopReason, AcpError> {
        // The turn is being cancelled: nothing admitted for it may reach the
        // wire after `session/cancel`. The drain below reuses the prompt read
        // loop, which would otherwise take a still-installed steer source and
        // write its inputs into a turn that is ending. Normally the prompt
        // loop already consumed the source; the case this closes is a prompt
        // future dropped before its loop ran `take()`. Dropping the source
        // closes every queued input's oneshot, which the caller reads as
        // `NotDelivered{PromptEndedBeforeWrite}` — and, for the legacy pool,
        // as the neutral release it already applies to a closed ack.
        if self.steer_rx.take().is_some() {
            tracing::info!(
                target: "acp::cancel",
                "dropped the turn's steer source before cancel: queued inputs stay unwritten"
            );
        }

        // Validate precondition before any side effects — fail fast if there's
        // no in-flight prompt (prevents writing permission responses or cancel
        // notifications to the agent when no prompt is active).
        let prompt_id = self.last_prompt_id.take().ok_or_else(|| {
            AcpError::Protocol("cancel_with_cleanup called with no in-flight prompt".into())
        })?;

        // Step 1: respond to any pending permission request with "cancelled",
        // but only if we haven't already responded (guards against double-response race).
        if let Some(perm_id) = self.pending_permission_id.clone() {
            if !self.permission_responded {
                let response = permission_response_cancelled(&perm_id);
                self.write_ndjson(&response).await?;
                tracing::debug!(
                    target: "acp::cancel",
                    "responded cancelled to pending permission id={perm_id}"
                );
            }
            self.pending_permission_id = None;
            self.permission_responded = false;
        }

        // Step 2: send session/cancel notification (no id)
        self.session_cancel(session_id).await?;
        tracing::info!(target: "acp::cancel", "sent session/cancel for {session_id}");
        // Use a fixed 30s idle timeout during cleanup — the cancel notification
        // needs time to propagate and the agent may go silent while winding down.
        // The separate hard_deadline bounds agents that keep producing output
        // but ignore cancellation.
        let cleanup_idle = std::time::Duration::from_secs(30);
        let remaining = hard_deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or_default();
        let result = self
            .read_until_response_with_idle_timeout(
                session_id,
                prompt_id,
                cleanup_idle,
                hard_deadline,
                remaining,
            )
            .await?;
        self.parse_prompt_response(session_id, &result)
    }

    /// Serialize `value` as a single NDJSON line and flush to the agent's stdin.
    ///
    /// Bounded by a 30-second write timeout. If the agent stops reading stdin
    /// (e.g., it's stuck or dead), the write would otherwise block forever.
    async fn write_ndjson(&mut self, value: &serde_json::Value) -> Result<(), AcpError> {
        const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        let line = serde_json::to_string(value)?;
        tokio::time::timeout(WRITE_TIMEOUT, async {
            self.stdin.write_all(line.as_bytes()).await?;
            self.stdin.write_all(b"\n").await?;
            self.stdin.flush().await?;
            Ok::<(), std::io::Error>(())
        })
        .await
        .map_err(|_| AcpError::WriteTimeout(WRITE_TIMEOUT))?
        .map_err(AcpError::Io)?;
        self.observe("acp_write", value.clone());
        Ok(())
    }

    /// Default timeout for non-prompt RPCs (initialize, session/new, etc.).
    const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    /// Send a JSON-RPC request and wait for the matching response.
    ///
    /// Assigns the next available id, writes the NDJSON line to stdin,
    /// then calls [`read_until_response`](Self::read_until_response).
    ///
    /// The write phase is bounded by `WRITE_TIMEOUT` (30s) and the read phase
    /// by `REQUEST_TIMEOUT` (60s), so worst-case wall clock is ~90s. Non-prompt
    /// RPCs like `initialize` and `session/new` should complete in seconds;
    /// if they don't, the agent is likely stuck and we must not block forever.
    async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AcpError> {
        let id = self.next_id;
        self.next_id += 1;

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        if let Some(payload) = acp_request_log_payload(method, &msg) {
            tracing::debug!(target: "acp::wire", "→ {payload}");
        } else {
            tracing::debug!(target: "acp::wire", id, method, "→ ACP request (session details redacted)");
        }

        // Wrap write + read in a single timeout so a hung agent can't block forever.
        // We cannot use an async block that borrows `self` mutably across two awaits
        // inside timeout(), so we sequence them with early-return on timeout.
        let timeout = Self::REQUEST_TIMEOUT;
        match tokio::time::timeout(timeout, self.write_ndjson(&msg)).await {
            Ok(result) => result?,
            Err(_) => return Err(AcpError::Timeout(timeout)),
        }

        match tokio::time::timeout(timeout, self.read_until_response(id)).await {
            Ok(result) => result,
            Err(_) => Err(AcpError::Timeout(timeout)),
        }
    }

    /// Drain any buffered lines from the agent's stdout without blocking.
    ///
    /// After a [`AcpError::Timeout`] from [`send_request`], the agent may
    /// eventually send the late response. That stale message will sit in the
    /// `BufReader` buffer and be silently skipped by the next `read_until_response`
    /// call (ID mismatch). However, if the caller wants a clean slate — e.g.
    /// before retrying the same method — they can call this to consume any
    /// buffered data with a short deadline.
    ///
    /// This is a best-effort drain: it reads until the buffer is empty or
    /// `drain_timeout` elapses, whichever comes first. Errors are ignored.
    #[allow(dead_code)] // Scaffolding for future model-switch timeout cleanup; not yet wired.
    pub async fn drain_stale_responses(&mut self, drain_timeout: std::time::Duration) {
        let deadline = tokio::time::Instant::now() + drain_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let read_result = tokio::time::timeout(remaining, self.reader.next()).await;
            match read_result {
                // Timeout or stream ended — buffer is empty or agent exited.
                Err(_) | Ok(None) => break,
                Ok(Some(Ok(_))) => {
                    // Consumed one buffered line; loop to drain more.
                    tracing::debug!(target: "acp::wire", "drained stale buffered line");
                }
                Ok(Some(Err(_))) => break,
            }
        }
    }

    /// Send a JSON-RPC **notification** — no `id` field, no response expected.
    ///
    /// Used for `session/cancel`. The absence of `id` is the JSON-RPC 2.0
    /// distinguisher between requests and notifications.
    async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), AcpError> {
        // Notifications deliberately have NO "id" field.
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        tracing::debug!(target: "acp::wire", "→ (notification) {}", &serde_json::to_string(&msg).unwrap_or_default());
        self.write_ndjson(&msg).await?;
        Ok(())
    }

    /// Core message loop: read NDJSON lines until we get a response matching `expected_id`.
    ///
    /// While waiting, handles:
    /// - `session/update` notifications → logged via tracing
    /// - `session/request_permission` requests → auto-approved with `allow_once`
    /// - Any other messages → debug-logged and ignored; if they carry an `id`
    ///   (i.e. they are requests, not notifications), a JSON-RPC -32601 error is sent.
    ///
    /// Compares the incoming `id` field as a `serde_json::Value` against
    /// `json!(expected_id)` so that both numeric and string IDs work correctly.
    async fn read_until_response(
        &mut self,
        expected_id: u64,
    ) -> Result<serde_json::Value, AcpError> {
        loop {
            // LinesCodec::new_with_max_length enforces MAX_LINE_SIZE at the
            // read level — the buffer never grows beyond the limit, preventing
            // OOM from rogue agents writing infinite non-newline bytes.
            let line = match self.reader.next().await {
                None => return Err(AcpError::AgentExited),
                Some(Err(LinesCodecError::MaxLineLengthExceeded)) => {
                    return Err(AcpError::Protocol(
                        "agent stdout line exceeded 10MB limit".into(),
                    ));
                }
                Some(Err(e)) => {
                    return Err(AcpError::Io(std::io::Error::other(e)));
                }
                Some(Ok(line)) => line,
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Only log and reset idle after we have a valid non-empty line.
            tracing::debug!(target: "acp::wire", "← {trimmed}");

            let msg: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    self.observe(
                        "acp_parse_error",
                        serde_json::json!({
                            "line": trimmed,
                            "error": e.to_string(),
                        }),
                    );
                    tracing::warn!(
                        target: "acp::wire",
                        "failed to parse line as JSON: {e} — skipping"
                    );
                    continue;
                }
            };
            self.observe("acp_read", msg.clone());

            // Check if this is a response to our expected request (has matching id
            // AND no `method` field — a `method` field means it's an agent-initiated
            // request, not a response, even if the id happens to match).
            if let Some(id) = msg.get("id") {
                if *id == serde_json::json!(expected_id) && msg.get("method").is_none() {
                    if let Some(error) = msg.get("error") {
                        return Err(agent_error_from_json(error));
                    }
                    return Ok(msg["result"].clone());
                }
                // A response to some other id may be the late answer to a
                // steer request whose prompt already ended; correlate it
                // before it is skipped as stray.
                if self.route_late_steer_ack(&msg) {
                    continue;
                }
            }

            // Dispatch by method name (notifications and agent-initiated requests).
            if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
                match method {
                    "session/update" => {
                        let _ = self.handle_session_update(&msg);
                    }
                    "_goose/unstable/session/update" => {
                        self.handle_goose_usage_update(&msg);
                    }
                    RAW_SDK_FRAME_METHOD => self.handle_raw_sdk_frame(&msg),
                    "session/request_permission" => {
                        self.handle_permission_request(&msg).await?;
                    }
                    other => {
                        // If the unknown message has an id, it's a request expecting a reply.
                        // Silence would cause the agent to hang waiting for a response.
                        // Send a JSON-RPC -32601 "Method not found" error.
                        if msg.get("id").is_some() {
                            let err_resp = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": msg["id"],
                                "error": {"code": -32601, "message": format!("Method not found: {other}")}
                            });
                            // Surface write failures — a broken pipe means the
                            // agent process is dead and continuing would hang.
                            self.write_ndjson(&err_resp).await?;
                        }
                        tracing::debug!(target: "acp::wire", "ignoring unknown method: {other}");
                    }
                }
            }
        }
    }

    /// Idle-aware message loop: like [`read_until_response`] but resets an idle
    /// deadline on every stdout line. Fires [`AcpError::IdleTimeout`] on silence
    /// or [`AcpError::HardTimeout`] on absolute wall-clock cap.
    ///
    /// `hard_deadline` is an absolute `Instant` (pre-computed by the caller) so
    /// that `cancel_with_cleanup` can inherit the remaining budget from the
    /// original turn rather than starting a fresh timer.
    /// Read agent messages until the response with `expected_id` arrives, or
    /// either of two timeouts fires. Returns `Result<value, IdleTimeout |
    /// HardTimeout | other>`.
    ///
    /// - `idle_timeout`: silent-agent guard, **reset on every line of valid
    ///   JSON** (and explicitly on `session/update` notifications).
    /// - `hard_deadline`: absolute wall-clock cap on the whole call, passed
    ///   in so that `cancel_with_cleanup` can inherit the remaining budget
    ///   from the original turn rather than starting a fresh timer.
    ///
    /// While reading, the loop interleaves goose-native non-cancelling steer
    /// requests via `tokio::select!`. The select uses `biased` for
    /// reader-first throughput, with a pre-select deadline check at the top
    /// of every loop iteration so a continuously-ready reader arm cannot
    /// starve the hard deadline (Max's review gate). The steer arm is
    /// guarded by `pending_steer.is_none()` so at most one steer is in
    /// flight at a time; a successful steer response is routed to the
    /// caller's oneshot ack instead of being returned as the prompt result.
    ///
    /// `session_id` is threaded in lexically by callers so the goose-native
    /// steer arm can complete `sessionId` in the steer JSON-RPC params at
    /// write time without needing access to outer state. See
    /// [`crate::pool::SteerRequest`] for why params are built here and not
    /// in the main loop.
    async fn read_until_response_with_idle_timeout(
        &mut self,
        session_id: &str,
        expected_id: u64,
        idle_timeout: std::time::Duration,
        hard_deadline: tokio::time::Instant,
        max_duration: std::time::Duration,
    ) -> Result<serde_json::Value, AcpError> {
        // Take the per-turn steer source into a local so it can be borrowed
        // independently of `self.reader` inside `select!`. Dropped at scope
        // exit: inputs still queued in it are never answered, which the
        // caller reads as `PromptEndedBeforeWrite` (§3.1 item 7).
        let mut steer_rx = self.steer_rx.take();

        // The one written steer request awaiting its answer. While `Some`,
        // the steer arm is gated off so we don't stack writes, and a response
        // matching its id is routed to its sink instead of being treated as
        // the prompt result. Settled on every return path by
        // `leave_prompt_loop`, so a caller is never left hanging.
        let mut pending_steer: Option<PendingSteer> = None;

        // Cloned out of `self` before the loop: the select arms borrow
        // `self.reader` mutably, so the clock cannot be read through `self`
        // from inside them.
        let clock = self.turn_clock.clone();

        let now = clock.now();
        let mut idle_deadline = now + idle_timeout;
        let mut hard_deadline = hard_deadline;
        let mut last_activity_at = now;
        let mut stall_watch = AnswerStallWatch::new(self.answer_stall_timeout);
        let mut wire = TurnWire::new(now);

        loop {
            // Determine which deadline fires first BEFORE sleeping — this is
            // the classification we'll use on timeout, immune to scheduler jitter.
            // The stall deadline is only present while the turn actually looks
            // finished, so most iterations still choose between two.
            let mut next_deadline = idle_deadline.min(hard_deadline);
            let mut expiry = if idle_deadline < hard_deadline {
                DeadlineKind::Idle
            } else {
                DeadlineKind::Hard
            };
            if let Some(stall_deadline) = stall_watch.deadline() {
                if stall_deadline < next_deadline {
                    next_deadline = stall_deadline;
                    expiry = DeadlineKind::AnswerStall;
                }
            }

            // Pre-select deadline check — required by Max's review. Under
            // `biased`, a continuously-ready reader arm wins every poll and
            // `sleep_until(next_deadline)` is never reached, silently
            // defeating the hard-deadline guarantee for agents that keep
            // producing output (see `acp.rs:608` for why the hard deadline
            // exists). Check the classified deadline here so a steady-
            // stream agent is still bounded.
            if clock.now() >= next_deadline {
                let error = expiry.into_error(
                    idle_timeout,
                    &stall_watch,
                    last_activity_at,
                    &wire,
                    clock.now(),
                );
                return self
                    .leave_prompt_loop(
                        Err(error),
                        pending_steer.take(),
                        PromptExit::Deadline,
                        &clock,
                    )
                    .await;
            }

            // LinesCodec::new_with_max_length enforces MAX_LINE_SIZE at the
            // read level — the buffer never grows beyond the limit.
            let read_result = tokio::select! {
                biased;
                read_result = self.reader.next() => Some(read_result),
                // Steer arm: gated off whenever a steer write is already in
                // flight so we don't stack two writes against the same
                // process. The `async { steer_rx.as_mut()?.recv().await }`
                // wrapper produces `None` when no source is installed,
                // which mismatches the `Some(taken)` pattern and disables the
                // branch for that iteration (no busy loop). Cancel-safe:
                // `mpsc::Receiver::recv` does not lose messages on drop.
                Some(taken) = async {
                    match steer_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => None,
                    }
                }, if pending_steer.is_none() => {
                    pending_steer = self.write_steer(session_id, taken).await;
                    // Loop back to the next iteration without consuming a
                    // reader line; we'll wait for either the prompt
                    // response or the steer response next.
                    None
                }
                _ = clock.sleep_until(next_deadline) => {
                    // The pre-select check at the top of the next iteration
                    // would catch this anyway, but firing the deadline arm
                    // here makes the wakeup immediate (no extra reader poll
                    // round-trip when stdout is idle).
                    let error = expiry.into_error(
                        idle_timeout,
                        &stall_watch,
                        last_activity_at,
                        &wire,
                        clock.now(),
                    );
                    return self
                        .leave_prompt_loop(
                            Err(error),
                            pending_steer.take(),
                            PromptExit::Deadline,
                            &clock,
                        )
                        .await;
                }
            };

            // Steer arm fired (or the select selected nothing read-side this
            // iteration): no reader frame to process, loop to re-evaluate
            // deadlines and arm the next select.
            let read_result = match read_result {
                Some(r) => r,
                None => continue,
            };

            match read_result {
                None => {
                    return self
                        .leave_prompt_loop(
                            Err(AcpError::AgentExited),
                            pending_steer.take(),
                            PromptExit::Eof,
                            &clock,
                        )
                        .await;
                }
                Some(Err(LinesCodecError::MaxLineLengthExceeded)) => {
                    return self
                        .leave_prompt_loop(
                            Err(AcpError::Protocol(
                                "agent stdout line exceeded 10MB limit".into(),
                            )),
                            pending_steer.take(),
                            PromptExit::ReadError,
                            &clock,
                        )
                        .await;
                }
                Some(Err(e)) => {
                    return self
                        .leave_prompt_loop(
                            Err(AcpError::Io(std::io::Error::other(e))),
                            pending_steer.take(),
                            PromptExit::ReadError,
                            &clock,
                        )
                        .await;
                }
                Some(Ok(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    tracing::debug!(target: "acp::wire", "← {trimmed}");

                    let msg: serde_json::Value = match serde_json::from_str(trimmed) {
                        Ok(v) => v,
                        Err(e) => {
                            self.observe(
                                "acp_parse_error",
                                serde_json::json!({
                                    "line": trimmed,
                                    "error": e.to_string(),
                                }),
                            );
                            tracing::warn!(
                                target: "acp::wire",
                                "failed to parse line as JSON: {e} — skipping"
                            );
                            continue;
                        }
                    };
                    self.observe("acp_read", msg.clone());

                    let activity_now = clock.now();
                    idle_deadline = activity_now + idle_timeout;
                    last_activity_at = activity_now;
                    stall_watch.observe(&msg, activity_now);
                    wire.record(&msg, trimmed.len(), activity_now);

                    // Steer response routing must come BEFORE the prompt
                    // response check: a steer response is a regular
                    // JSON-RPC response (id + result/error, no method),
                    // so the matcher must disambiguate by id. All checks
                    // share the `no method` guard.
                    if let Some(id) = msg.get("id") {
                        if msg.get("method").is_none() {
                            if let Some(pending) = pending_steer.as_mut() {
                                if *id == serde_json::json!(pending.request_id) {
                                    // Route the answer to its sink. We do not
                                    // return — keep reading until the prompt
                                    // response arrives.
                                    let resolution = decode_steer_ack(
                                        &msg,
                                        pending.wire,
                                        pending.native_run_id.clone(),
                                        pending.request_id,
                                    );
                                    match &resolution {
                                        SteerResolution::Injected { .. } => {
                                            // The awaited turn keeps running
                                            // with more work in it: give it
                                            // a fresh budget.
                                            let renew_now = clock.now();
                                            let new_deadline = renew_now + max_duration;
                                            if new_deadline > hard_deadline {
                                                hard_deadline = new_deadline;
                                                self.current_hard_deadline = Some(new_deadline);
                                                tracing::info!(
                                                    "steer injected: renewed hard deadline ({max_duration:?} from now)"
                                                );
                                            }
                                        }
                                        SteerResolution::StartedNewTurn { .. } => {
                                            // Delivered, but into a NEW turn:
                                            // the one this read loop awaits
                                            // had already finished. Renewing
                                            // the hard deadline would extend
                                            // the clock on a settled turn, so
                                            // leave it alone.
                                            tracing::info!(
                                                "steer accepted as {STEER_OUTCOME_STARTED_NEW_TURN}: \
                                                 awaited turn had ended — hard deadline not renewed"
                                            );
                                        }
                                        other => {
                                            tracing::warn!(
                                                request_id = pending.request_id,
                                                resolution = ?other,
                                                "steer request was not delivered as injected"
                                            );
                                        }
                                    }
                                    pending.resolve(resolution);
                                    pending_steer = None;
                                    continue;
                                }
                            }
                            if *id == serde_json::json!(expected_id) {
                                let outcome = match msg.get("error") {
                                    Some(error) => Err(agent_error_from_json(error)),
                                    None => Ok(msg["result"].clone()),
                                };
                                return self
                                    .leave_prompt_loop(
                                        outcome,
                                        pending_steer.take(),
                                        PromptExit::Answered,
                                        &clock,
                                    )
                                    .await;
                            }
                            if self.route_late_steer_ack(&msg) {
                                continue;
                            }
                        }
                    }

                    // Dispatch notifications and agent-initiated requests.
                    if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
                        match method {
                            "session/update" => {
                                if self.handle_session_update(&msg) {
                                    let activity_now = clock.now();
                                    idle_deadline = activity_now + idle_timeout;
                                    last_activity_at = activity_now;
                                    tracing::debug!("idle clock reset: tool call started");
                                }
                            }
                            "_goose/unstable/session/update" => {
                                self.handle_goose_usage_update(&msg);
                            }
                            RAW_SDK_FRAME_METHOD => self.handle_raw_sdk_frame(&msg),
                            "session/request_permission" => {
                                if let Err(error) = self.handle_permission_request(&msg).await {
                                    return self
                                        .leave_prompt_loop(
                                            Err(error),
                                            pending_steer.take(),
                                            PromptExit::ReadError,
                                            &clock,
                                        )
                                        .await;
                                }
                            }
                            other => {
                                // If the unknown message has an id, it's a request expecting a reply.
                                // Silence would cause the agent to hang waiting for a response.
                                // Send a JSON-RPC -32601 "Method not found" error.
                                if msg.get("id").is_some() {
                                    let err_resp = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": msg["id"],
                                        "error": {"code": -32601, "message": format!("Method not found: {other}")}
                                    });
                                    // Surface write failures — a broken pipe means the
                                    // agent process is dead and continuing would hang.
                                    if let Err(error) = self.write_ndjson(&err_resp).await {
                                        return self
                                            .leave_prompt_loop(
                                                Err(error),
                                                pending_steer.take(),
                                                PromptExit::ReadError,
                                                &clock,
                                            )
                                            .await;
                                    }
                                }
                                tracing::debug!(target: "acp::wire", "ignoring unknown method: {other}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// Leave the prompt read loop with `outcome`, settling any steer request
    /// still awaiting its answer first. The settlement never changes
    /// `outcome`: the prompt's own result is fixed before the drain starts.
    async fn leave_prompt_loop(
        &mut self,
        outcome: Result<serde_json::Value, AcpError>,
        pending: Option<PendingSteer>,
        exit: PromptExit,
        clock: &std::sync::Arc<dyn TurnClock>,
    ) -> Result<serde_json::Value, AcpError> {
        if let Some(mut pending) = pending {
            self.settle_pending_steer(&mut pending, exit, clock).await;
        }
        outcome
    }

    /// Settle a written steer request the prompt loop is leaving behind
    /// (`docs/NATIVE_STEERING_IMPL.md` §3.1 item 5).
    ///
    /// With a healthy reader, a native attempt gets a bounded drain of up to
    /// [`STEER_ACK_DRAIN`] for its answer. A legacy request is answered
    /// `PromptCompletedNeutral` at once, as the pool harness always was —
    /// its main loop reads the prompt result and the ack together, and a
    /// delayed result would change its timing. EOF and reader failures
    /// cannot be drained and are named for what they are.
    async fn settle_pending_steer(
        &mut self,
        pending: &mut PendingSteer,
        exit: PromptExit,
        clock: &std::sync::Arc<dyn TurnClock>,
    ) {
        match exit {
            PromptExit::Eof => pending.abandon(UnknownReason::RuntimeExited),
            PromptExit::ReadError => pending.abandon(UnknownReason::PromptEndedBeforeAck),
            PromptExit::Answered | PromptExit::Deadline => {
                if pending.is_native() {
                    self.drain_steer_ack(pending, clock).await;
                } else {
                    pending.abandon(UnknownReason::PromptEndedBeforeAck);
                }
            }
        }
    }

    /// Keep reading for up to [`STEER_ACK_DRAIN`] for `pending`'s answer.
    ///
    /// Notifications seen meanwhile are handled as in the prompt loop;
    /// responses to other ids are checked against the unresolved map and
    /// otherwise ignored. Expiry answers [`UnknownReason::AckTimeout`], EOF
    /// [`UnknownReason::RuntimeExited`], and a broken reader
    /// [`UnknownReason::PromptEndedBeforeAck`]; each of those also records
    /// the request as unresolved so a later loop can still correlate it.
    async fn drain_steer_ack(
        &mut self,
        pending: &mut PendingSteer,
        clock: &std::sync::Arc<dyn TurnClock>,
    ) {
        let deadline = clock.now() + STEER_ACK_DRAIN;
        tracing::info!(
            request_id = pending.request_id,
            attempt_id = ?pending.attempt_id,
            "prompt ended with a steer awaiting its acknowledgement — draining up to {STEER_ACK_DRAIN:?}"
        );
        loop {
            // Pre-select check, for the same reason the prompt loop has one:
            // a continuously-ready reader must not starve the drain bound.
            if clock.now() >= deadline {
                pending.abandon(UnknownReason::AckTimeout);
                return;
            }
            let read_result = tokio::select! {
                biased;
                read_result = self.reader.next() => read_result,
                _ = clock.sleep_until(deadline) => {
                    pending.abandon(UnknownReason::AckTimeout);
                    return;
                }
            };
            let line = match read_result {
                None => {
                    pending.abandon(UnknownReason::RuntimeExited);
                    return;
                }
                Some(Err(_)) => {
                    pending.abandon(UnknownReason::PromptEndedBeforeAck);
                    return;
                }
                Some(Ok(line)) => line,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            tracing::debug!(target: "acp::wire", "← {trimmed}");
            let msg: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    self.observe(
                        "acp_parse_error",
                        serde_json::json!({
                            "line": trimmed,
                            "error": e.to_string(),
                        }),
                    );
                    tracing::warn!(
                        target: "acp::wire",
                        "failed to parse line as JSON: {e} — skipping"
                    );
                    continue;
                }
            };
            self.observe("acp_read", msg.clone());

            if msg.get("method").is_none() {
                if let Some(id) = msg.get("id") {
                    if *id == serde_json::json!(pending.request_id) {
                        let resolution = decode_steer_ack(
                            &msg,
                            pending.wire,
                            pending.native_run_id.clone(),
                            pending.request_id,
                        );
                        tracing::info!(
                            request_id = pending.request_id,
                            ?resolution,
                            "steer acknowledged inside the post-prompt drain"
                        );
                        pending.resolve(resolution);
                        return;
                    }
                    if !self.route_late_steer_ack(&msg) {
                        tracing::debug!(
                            target: "acp::wire",
                            "ignoring stray response id {id} during steer drain"
                        );
                    }
                    continue;
                }
            }

            if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
                match method {
                    "session/update" => {
                        let _ = self.handle_session_update(&msg);
                    }
                    "_goose/unstable/session/update" => {
                        self.handle_goose_usage_update(&msg);
                    }
                    RAW_SDK_FRAME_METHOD => self.handle_raw_sdk_frame(&msg),
                    "session/request_permission" => {
                        if let Err(e) = self.handle_permission_request(&msg).await {
                            tracing::warn!("permission reply failed during steer drain: {e}");
                            pending.abandon(UnknownReason::PromptEndedBeforeAck);
                            return;
                        }
                    }
                    other => {
                        if msg.get("id").is_some() {
                            let err_resp = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": msg["id"],
                                "error": {"code": -32601, "message": format!("Method not found: {other}")}
                            });
                            if let Err(e) = self.write_ndjson(&err_resp).await {
                                tracing::warn!("reply failed during steer drain: {e}");
                                pending.abandon(UnknownReason::PromptEndedBeforeAck);
                                return;
                            }
                        }
                        tracing::debug!(target: "acp::wire", "ignoring unknown method: {other}");
                    }
                }
            }
        }
    }

    /// Log a `session/update` notification via tracing.
    ///
    /// The discriminator field is `sessionUpdate` (not `type`) per the ACP schema.
    /// Returns `true` if the update indicates a tool call started, signaling that
    /// the idle clock should be explicitly reset (the agent will be silent while
    /// the tool executes).
    ///
    /// Takes `&mut self` (not `&self`) because some updates carry agent state
    /// the client must observe — notably goose's `session_info_update` with
    /// `_meta.goose.activeRunId`, which seeds [`active_run_id`](Self::active_run_id)
    /// so the steer arm can target `_goose/unstable/session/steer` at the
    /// correct run. Agents that never emit it (claude-agent-acp, codex-acp)
    /// leave it `None` and are steered via `_session/steering` instead, which
    /// needs no run id.
    fn handle_session_update(&mut self, msg: &serde_json::Value) -> bool {
        let update = &msg["params"]["update"];
        let update_type = update
            .get("sessionUpdate")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        match update_type {
            "agent_message_chunk" => {
                if let Some(text) = update["content"]["text"].as_str() {
                    tracing::info!(target: "acp::stream", "{text}");
                }
                false
            }
            "tool_call" => {
                let title = update
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let kind = update
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                tracing::info!(target: "acp::tool", "tool_call: {title} ({kind})");
                true
            }
            "tool_call_update" => {
                let tool_id = update
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let status = update.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                tracing::info!(target: "acp::tool", "tool_call_update: {tool_id} → {status}");
                false
            }
            "plan" => {
                tracing::info!(target: "acp::plan", "plan update received");
                false
            }
            "agent_thought_chunk" => {
                if let Some(text) = update["content"]["text"].as_str() {
                    tracing::debug!(target: "acp::thought", "{text}");
                }
                false
            }
            "available_commands_update" => {
                // Advertised slash commands (ACP slash-commands extension).
                // Logged for observability; UI surfacing is a follow-up.
                let names: Vec<&str> = update["availableCommands"]
                    .as_array()
                    .map(|cmds| cmds.iter().filter_map(|c| c["name"].as_str()).collect())
                    .unwrap_or_default();
                tracing::info!(
                    target: "acp::update",
                    "available_commands_update: {} commands [{}]",
                    names.len(),
                    names.join(", ")
                );
                false
            }
            "session_info_update" => {
                // Both goose and buzz-agent emit `session_info_update` with
                // `_meta.goose.activeRunId`: the id of the currently-active
                // prompt run, or `null` when the run has cleared. Other agents
                // don't emit this field; for them `active_run_id` stays `None`
                // and steer callers will fall back to cancel+merge.
                //
                // Per the ACP `SessionInfoUpdate` schema, `_meta` is a field
                // on the update object itself — nested inside `update`, not
                // alongside it at the params level. Goose and buzz-agent both
                // emit it at `params.update._meta.goose.activeRunId`.
                let meta = msg["params"]["update"]
                    .get("_meta")
                    .and_then(|m| m.get("goose"));
                if let Some(goose_meta) = meta {
                    match goose_meta.get("activeRunId") {
                        Some(serde_json::Value::String(run_id)) => {
                            tracing::debug!(
                                target: "acp::update",
                                "session_info_update: activeRunId={run_id}"
                            );
                            self.active_run_id = Some(run_id.clone());
                        }
                        Some(serde_json::Value::Null) => {
                            tracing::debug!(
                                target: "acp::update",
                                "session_info_update: activeRunId cleared"
                            );
                            self.active_run_id = None;
                        }
                        // Missing or non-string/null — leave state untouched.
                        _ => {}
                    }
                }
                false
            }
            "usage_update" => {
                self.handle_standard_usage_update(msg);
                false
            }
            "keepalive" => false,
            other => {
                tracing::debug!(target: "acp::update", "session/update: {other}");
                false
            }
        }
    }

    /// Record the standard ACP cumulative cost notification when emitted by
    /// Claude. Unlike Goose's payload, `used`/`size` are context occupancy and
    /// are intentionally not mapped to token accounting.
    fn handle_standard_usage_update(&mut self, msg: &serde_json::Value) {
        if self.standard_adapter != Some(StandardAdapterKind::Claude) {
            return;
        }
        let session_id = match msg
            .pointer("/params/sessionId")
            .and_then(serde_json::Value::as_str)
        {
            Some(session_id) => session_id,
            None => return,
        };
        let cost = match msg
            .pointer("/params/update/cost/amount")
            .and_then(serde_json::Value::as_f64)
        {
            Some(cost) => cost,
            None => return,
        };
        self.standard_usage.record_cost(session_id, cost);
    }

    /// Parse a `_goose/unstable/session/update` notification and record the
    /// usage snapshot in the per-session tracker.
    ///
    /// Silently ignores malformed or non-`usage_update` variants — the
    /// notification is best-effort observability data, not a protocol
    /// requirement. Failures are logged at debug level.
    fn handle_goose_usage_update(&mut self, msg: &serde_json::Value) {
        use crate::usage::{GooseSessionUpdateNotification, GooseSessionUpdateVariant};
        let params = match msg.get("params") {
            Some(p) => p,
            None => {
                tracing::debug!(
                    target: "acp::usage",
                    "_goose/unstable/session/update: missing params"
                );
                return;
            }
        };
        match serde_json::from_value::<GooseSessionUpdateNotification>(params.clone()) {
            Ok(notif) => {
                if let GooseSessionUpdateVariant::UsageUpdate(payload) = &notif.update {
                    tracing::debug!(
                        target: "acp::usage",
                        session_id = %notif.session_id,
                        input = ?payload.accumulated_input_tokens,
                        output = ?payload.accumulated_output_tokens,
                        // A subset of `input`, logged so downstream accounting can
                        // price it at the provider's cached rate. Always emitted,
                        // including as 0, so a parser can tell "no cache hits"
                        // apart from "this build predates the field".
                        cached = payload.accumulated_cached_input_tokens,
                        "goose usage update"
                    );
                    self.goose_usage.record(&notif.session_id, payload);
                }
            }
            Err(e) => {
                tracing::debug!(
                    target: "acp::usage",
                    "_goose/unstable/session/update: deserialization error: {e}"
                );
            }
        }
    }

    /// Auto-approve a `session/request_permission` request from the agent.
    ///
    /// Finds the option with `kind == "allow_once"` and responds with its `optionId`.
    /// If no `allow_once` option exists, falls back to `reject_once`.
    ///
    /// **Critical:** Never hardcode `optionId` — always find it dynamically by `kind`.
    ///
    /// The request `id` is stored as `serde_json::Value` to support both numeric
    /// and string IDs per JSON-RPC 2.0.
    async fn handle_permission_request(&mut self, msg: &serde_json::Value) -> Result<(), AcpError> {
        // Extract id as a Value — JSON-RPC 2.0 allows both numeric and string IDs.
        let id = msg
            .get("id")
            .cloned()
            .ok_or_else(|| AcpError::Protocol("permission request missing id".into()))?;

        // Store pending permission id so cancel_with_cleanup can respond to it.
        self.pending_permission_id = Some(id.clone());
        // Mark as not yet responded — guards against double-response race.
        self.permission_responded = false;

        let options = msg["params"]["options"]
            .as_array()
            .ok_or_else(|| AcpError::Protocol("permission request missing options".into()))?;

        tracing::debug!(
            target: "acp::permission",
            "session/request_permission id={id}, {} options",
            options.len()
        );

        // Find allow_once by kind — NEVER hardcode optionId.
        let allow_once = options
            .iter()
            .find(|opt| opt.get("kind").and_then(|k| k.as_str()) == Some("allow_once"));

        let response = if let Some(opt) = allow_once {
            let option_id = opt["optionId"]
                .as_str()
                .ok_or_else(|| AcpError::Protocol("allow_once option missing optionId".into()))?;
            tracing::info!(
                target: "acp::permission",
                "auto-approving permission id={id} with allow_once optionId={option_id:?}"
            );
            permission_response_selected(&id, option_id)
        } else {
            // No allow_once — fall back to reject_once.
            tracing::warn!(
                target: "acp::permission",
                "no allow_once option found in permission request id={id}, falling back to reject_once"
            );
            let reject = options
                .iter()
                .find(|opt| opt.get("kind").and_then(|k| k.as_str()) == Some("reject_once"));

            if let Some(opt) = reject {
                let option_id = opt["optionId"].as_str().unwrap_or("reject");
                permission_response_selected(&id, option_id)
            } else {
                return Err(AcpError::Protocol(
                    "no suitable permission option found (neither allow_once nor reject_once)"
                        .into(),
                ));
            }
        };

        // Write the response first, then mark as responded.
        //
        // Previous ordering (flag-before-write) was intended to guard against a
        // double-response if a timeout fires between write and flag-set. However,
        // the deadlock risk is worse: if write_ndjson fails (e.g. WriteTimeout),
        // the flag would be true but no response was actually sent. Then
        // cancel_with_cleanup would see permission_responded=true, skip sending
        // the cancelled outcome, and the agent would hang waiting for a reply
        // that never arrives — a guaranteed deadlock.
        //
        // The correct fix: set the flag AFTER a successful write. The double-
        // response window (between write completion and flag-set) is negligibly
        // small and bounded by a single memory store; the deadlock window was
        // unbounded.
        self.write_ndjson(&response).await?;
        self.permission_responded = true;
        self.pending_permission_id = None;
        Ok(())
    }

    /// Parse a completed prompt response and retain its optional per-turn usage.
    fn parse_prompt_response(
        &mut self,
        session_id: &str,
        result: &serde_json::Value,
    ) -> Result<StopReason, AcpError> {
        let stop_reason = self.parse_stop_reason(result)?;
        if let Some(adapter) = self.standard_adapter {
            match serde_json::from_value::<PromptResponseUsage>(result["usage"].clone()) {
                Ok(usage) => self
                    .standard_usage
                    .record_prompt_usage(session_id, usage, adapter),
                Err(_) if result.get("usage").is_some() => tracing::debug!(
                    target: "acp::usage",
                    "session/prompt response contained malformed standard usage"
                ),
                Err(_) => {}
            }
        }
        Ok(stop_reason)
    }

    /// Parse `stopReason` from a `session/prompt` result value.
    fn parse_stop_reason(&self, result: &serde_json::Value) -> Result<StopReason, AcpError> {
        let raw = result["stopReason"].as_str().ok_or_else(|| {
            AcpError::Protocol("session/prompt response missing stopReason".into())
        })?;
        StopReason::from_str(raw)
            .ok_or_else(|| AcpError::Protocol(format!("unknown stopReason: {raw:?}")))
    }
}

/// One ACP `prompt` content block.
///
/// The wire is hand-rolled here (there is no ACP crate in the workspace), so
/// this is the single place a block shape is defined. Only the two variants the
/// harness actually sends exist: an agent that receives a block it did not
/// advertise support for is entitled to fail the turn, and one of ours
/// (`buzz-agent`) does exactly that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptBlock {
    /// A `{"type":"text"}` block.
    Text(String),
    /// A `{"type":"image"}` block carrying base64 bytes inline.
    ///
    /// Only sent when the agent advertised
    /// `agentCapabilities.promptCapabilities.image` at `initialize` — see
    /// [`AcpClient::prompt_image_supported`].
    Image {
        /// The image's MIME type, e.g. `image/png`.
        mime: String,
        /// Standard-alphabet base64 of the image bytes, unpadded or padded.
        data_base64: String,
    },
}

impl PromptBlock {
    /// Render this block as its ACP wire object.
    fn to_wire(&self) -> serde_json::Value {
        match self {
            Self::Text(text) => serde_json::json!({ "type": "text", "text": text }),
            Self::Image { mime, data_base64 } => serde_json::json!({
                "type": "image",
                "mimeType": mime,
                "data": data_base64,
            }),
        }
    }
}

/// Build `session/prompt` params from one or more content blocks.
fn build_prompt_params(session_id: &str, prompt_blocks: &[PromptBlock]) -> serde_json::Value {
    let blocks: Vec<serde_json::Value> = prompt_blocks.iter().map(PromptBlock::to_wire).collect();
    serde_json::json!({
        "sessionId": session_id,
        "prompt": blocks,
    })
}

/// Build `_goose/unstable/session/steer` params from one or more text
/// content blocks plus the freshest `expectedRunId`.
///
/// Wire shape:
/// ```json
/// { "sessionId": "...", "expectedRunId": "...", "prompt": [{"type":"text","text":"..."}, ...] }
/// ```
///
/// Called from the read-loop steer arm at write time so `expectedRunId`
/// matches goose's *current* run (it advances on each `session/update`).
/// See [`crate::pool::SteerRequest`] for why this is the read loop's job
/// and not the main loop's.
fn build_goose_steer_params(
    session_id: &str,
    expected_run_id: &str,
    prompt_blocks: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "sessionId": session_id,
        "expectedRunId": expected_run_id,
        "prompt": steer_prompt_blocks(prompt_blocks),
    })
}

/// Build the params for an [`ACP_STEER_METHOD`] request.
///
/// Wire shape:
/// ```json
/// { "sessionId": "...", "prompt": [{"type":"text","text":"..."}, ...] }
/// ```
///
/// Deliberately carries **no** `expectedRunId`: the cross-adapter method
/// steers whatever turn is currently running and neither claude-agent-acp nor
/// codex-acp emits a run id to target.
///
/// With [`IdleGuard::PromptRequired`] the params also carry
/// `"_meta": {"steering": {"idleBehavior": "promptRequired"}}`, which
/// claude-agent-acp 0.70 honours by answering `promptRequired` instead of
/// starting a detached turn when nothing is running
/// (`docs/NATIVE_STEERING_IMPL.md` §1). [`IdleGuard::AdapterDefault`] sends
/// no `_meta` at all.
fn build_acp_steer_params(
    session_id: &str,
    prompt_blocks: &[&str],
    idle_guard: IdleGuard,
) -> serde_json::Value {
    let mut params = serde_json::json!({
        "sessionId": session_id,
        "prompt": steer_prompt_blocks(prompt_blocks),
    });
    if idle_guard == IdleGuard::PromptRequired {
        params["_meta"] = serde_json::json!({
            "steering": { "idleBehavior": STEER_OUTCOME_PROMPT_REQUIRED }
        });
    }
    params
}

/// Render steer body strings as ACP `text` content blocks. Shared by both
/// steer transports so the prompt shape cannot drift between them.
fn steer_prompt_blocks(prompt_blocks: &[&str]) -> Vec<serde_json::Value> {
    prompt_blocks
        .iter()
        .map(|text| serde_json::json!({ "type": "text", "text": text }))
        .collect()
}

/// Serialize a request for debug logging unless it opens a session.
///
/// Session-open parameters carry host-private cursors, working directories,
/// and MCP environment such as `BUZZ_SESSION_CONTEXT_PACKAGE`. The wire still
/// receives the full request; debug logs receive only the method and id at the
/// call site.
fn acp_request_log_payload(method: &str, message: &serde_json::Value) -> Option<String> {
    if matches!(method, "session/new" | "session/resume" | "session/load") {
        None
    } else {
        Some(serde_json::to_string(message).unwrap_or_default())
    }
}

/// Build a JSON-RPC permission response with `outcome: "selected"`.
fn permission_response_selected(id: &serde_json::Value, option_id: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "outcome": { "outcome": "selected", "optionId": option_id } }
    })
}

/// Build a JSON-RPC permission response with `outcome: "cancelled"`.
fn permission_response_cancelled(id: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "outcome": { "outcome": "cancelled" } }
    })
}

/// Full `session/new` response — session ID plus the raw JSON result.
///
/// Callers use the extractor helpers to pull model info from `raw`.
pub struct SessionNewResponse {
    pub session_id: String,
    /// The full `result` value from the JSON-RPC response.
    pub raw: serde_json::Value,
}

/// How to deliver a system prompt on `session/new`.
///
/// The two variants match the two mechanisms supported by current adapters:
///
/// - **`Field`** — bare `systemPrompt` field (ACP protocol v2, buzz-agent).
/// - **`ClaudeMeta`** — `_meta.systemPrompt: {"append": text}`, used by
///   `claude-agent-acp` to append to the adapter's own native system prompt
///   while keeping its tool-use preset intact.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemPromptTransport<'a> {
    /// Deliver as a bare top-level `systemPrompt` field.
    Field(&'a str),
    /// Deliver as `_meta.systemPrompt: {"append": text}`.
    ClaudeMeta(&'a str),
}

/// Package name reported by `claude-agent-acp` in its `initialize` response.
///
/// Any adapter reporting this name supports `_meta.systemPrompt: {append: ...}`
/// on `session/new` — the feature landed in v0.6.0 (Oct 2025), before the
/// `@zed-industries/claude-code-acp` → `@agentclientprotocol/claude-agent-acp`
/// rename, so the new name is a reliable capability gate.
pub const CLAUDE_AGENT_ACP_NAME: &str = "@agentclientprotocol/claude-agent-acp";

/// Whether an adapter can receive a system prompt through *any* supported
/// transport (`session/new` for standard adapters, the custom post-`session/new`
/// request for goose).
///
/// `agent_name` is the normalized identity from `initialize` — see
/// [`normalized_agent_name`]. `goose_system_prompt_supported` is goose's probe
/// result (`None` before the first probe, i.e. "not known to work").
///
/// Callers that cannot use goose's custom request — anything that only speaks
/// `session/new` — must gate on [`session_new_system_prompt`] instead, which
/// reports `None` for goose.
pub fn has_system_prompt_support(
    protocol_version: u32,
    agent_name: &str,
    goose_system_prompt_supported: Option<bool>,
) -> bool {
    if agent_name == "goose" {
        goose_system_prompt_supported == Some(true)
    } else if agent_name == CLAUDE_AGENT_ACP_NAME {
        true
    } else {
        protocol_version >= 2
    }
}

/// Pick the `session/new` system-prompt transport for an adapter, if it has one.
///
/// `None` means the adapter has no supported `session/new` transport and the
/// caller must fall back to its own framing (a user-message section for the
/// harness, a first-turn preamble for the session provider). Goose is always
/// `None` here: it takes its system prompt through
/// [`AcpClient::session_set_goose_system_prompt`] after the session exists.
pub fn session_new_system_prompt<'a>(
    is_goose: bool,
    protocol_version: u32,
    agent_name: &str,
    prompt: Option<&'a str>,
) -> Option<SystemPromptTransport<'a>> {
    if is_goose || (protocol_version < 2 && agent_name != CLAUDE_AGENT_ACP_NAME) {
        None
    } else if agent_name == CLAUDE_AGENT_ACP_NAME {
        prompt.map(SystemPromptTransport::ClaudeMeta)
    } else {
        prompt.map(SystemPromptTransport::Field)
    }
}

/// Normalized adapter identity from an `initialize` response.
///
/// Reads `agentInfo.name`, falling back to `serverInfo.name`, and lowercases it
/// so capability gates like [`CLAUDE_AGENT_ACP_NAME`] compare reliably.
/// `"unknown"` when the adapter reported no name at all.
pub fn normalized_agent_name(init_result: &serde_json::Value) -> String {
    init_result
        .get("agentInfo")
        .or_else(|| init_result.get("serverInfo"))
        .and_then(|info| info.get("name"))
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .trim()
        .to_ascii_lowercase()
}

/// Adapter build from an `initialize` response, verbatim.
///
/// Reads `agentInfo.version`, falling back to `serverInfo.version`, mirroring
/// [`normalized_agent_name`]'s fallback. Unlike the name this is **not**
/// normalized: a version is an opaque token to be reported back exactly as the
/// adapter stated it, and lowercasing or trimming it would only invent a
/// difference between what we display and what the adapter said. Blank strings
/// are reported as absent, since an adapter that sends `""` has told us
/// nothing.
pub fn reported_agent_version(init_result: &serde_json::Value) -> Option<String> {
    init_result
        .get("agentInfo")
        .or_else(|| init_result.get("serverInfo"))
        .and_then(|info| info.get("version"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// How to switch to a particular model on a session.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type")]
pub enum ModelSwitchMethod {
    /// Stable: use `session/set_config_option` with these exact values.
    ConfigOption {
        config_id: String,
        option_value: String,
    },
    /// Unstable: use `session/set_model` with this model_id.
    SetModel { model_id: String },
}

/// Extract `configOptions` entries with `category == "model"` from a `session/new` result.
///
/// Returns the raw JSON array entries. Per the ACP schema each entry has `id` and
/// `name`, with `options: [{ value, name, description? }]` — what claude-agent-acp,
/// codex-acp and `grok agent stdio` send. Older producers spell them `configId` /
/// `displayName`; consumers should read `name` first and fall back.
pub fn extract_model_config_options(result: &serde_json::Value) -> Vec<serde_json::Value> {
    result["configOptions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|opt| opt.get("category").and_then(|c| c.as_str()) == Some("model"))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Extract `SessionModelState` (unstable path) from a `session/new` result.
///
/// Returns the `models` object if present: `{ currentModelId, availableModels: [...] }`.
pub fn extract_model_state(result: &serde_json::Value) -> Option<serde_json::Value> {
    result.get("models").cloned()
}

/// The model the adapter says this session is actually on.
///
/// Stable `configOptions[category=model].currentValue` first, then the
/// unstable `models.currentModelId` — the same precedence
/// [`resolve_model_switch_method`] uses, so a reader and a writer never
/// disagree about which surface is authoritative.
///
/// This exists because a *requested* model is not an applied one. When a
/// create asks for a model the adapter does not offer, the switch is skipped
/// and the session runs on the adapter's own default; publishing the request
/// as the model is how `Codex · default` came to label executions that were
/// really running `gpt-5.6-terra` (§2 item 39).
pub fn reported_model(session_new_result: &serde_json::Value) -> Option<String> {
    for config_option in extract_model_config_options(session_new_result) {
        if let Some(current) = config_option
            .get("currentValue")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        {
            return Some(current.to_owned());
        }
    }
    extract_model_state(session_new_result)
        .as_ref()
        .and_then(|models| models.get("currentModelId"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Match a desired model ID against a fresh `session/new` response.
///
/// Returns the correct ACP method to call, or `None` if no match.
///
/// **Precedence**: stable `configOptions` first (spec-blessed), then unstable
/// `availableModels`. The fresh `session/new` response is always authoritative.
pub fn resolve_model_switch_method(
    session_new_result: &serde_json::Value,
    desired_model: &str,
) -> Option<ModelSwitchMethod> {
    // 1. Search stable configOptions for a "model"-category entry whose
    //    options contain a value matching desired_model.
    for config_opt in extract_model_config_options(session_new_result) {
        // Adapters disagree on the key: the ACP spec says `configId`, but
        // claude-agent-acp emits `id`. Accept both; the set request always
        // uses `configId` on the wire.
        let config_id = match config_opt
            .get("configId")
            .or_else(|| config_opt.get("id"))
            .and_then(|v| v.as_str())
        {
            Some(id) => id,
            None => continue,
        };
        if let Some(options) = config_opt.get("options").and_then(|v| v.as_array()) {
            for opt in options {
                if opt.get("value").and_then(|v| v.as_str()) == Some(desired_model) {
                    return Some(ModelSwitchMethod::ConfigOption {
                        config_id: config_id.to_string(),
                        option_value: desired_model.to_string(),
                    });
                }
            }
        }
    }

    // 2. Search unstable availableModels for a matching modelId.
    if let Some(models) = extract_model_state(session_new_result) {
        if let Some(available) = models.get("availableModels").and_then(|v| v.as_array()) {
            for model in available {
                if model.get("modelId").and_then(|v| v.as_str()) == Some(desired_model) {
                    return Some(ModelSwitchMethod::SetModel {
                        model_id: desired_model.to_string(),
                    });
                }
            }
        }
    }

    // 3. No match.
    None
}

/// Whether `desired_model` appears in pre-extracted catalog halves.
///
/// Mirrors [`resolve_model_switch_method`]'s match, but operates on the
/// already-extracted `configOptions` (model category) and `models` state that
/// [`AgentModelCapabilities`](crate::pool::AgentModelCapabilities) caches — the
/// idle-path pre-cancel guard has those halves, not the full `session/new` JSON.
pub fn model_in_catalog(
    config_options: &[serde_json::Value],
    available_models: Option<&serde_json::Value>,
    desired_model: &str,
) -> bool {
    let in_config_options = config_options.iter().any(|config_opt| {
        config_opt
            .get("options")
            .and_then(|v| v.as_array())
            .is_some_and(|options| {
                options
                    .iter()
                    .any(|opt| opt.get("value").and_then(|v| v.as_str()) == Some(desired_model))
            })
    });
    if in_config_options {
        return true;
    }

    available_models
        .and_then(|models| models.get("availableModels"))
        .and_then(|v| v.as_array())
        .is_some_and(|available| {
            available
                .iter()
                .any(|model| model.get("modelId").and_then(|v| v.as_str()) == Some(desired_model))
        })
}

// ─── Drop: kill child process ─────────────────────────────────────────────────

impl Drop for AcpClient {
    fn drop(&mut self) {
        // Best-effort SIGKILL + reap. We cannot `await` in Drop (sync context).
        // Kill the process group when possible so subprocesses don't leak.
        // Callers SHOULD still call `shutdown().await` for guaranteed reaping.
        match self.child.id() {
            Some(pid) if kill_process_group(pid) => {}
            _ => {
                let _ = self.child.start_kill();
            }
        }
        // Non-blocking reap attempt — prevents zombie accumulation in the
        // common case where SIGKILL takes effect before Drop returns.
        let _ = self.child.try_wait();
    }
}

/// Send SIGKILL to an entire process group. Returns `true` if the signal was sent.
///
/// The child is spawned with `process_group(0)`, so its PID equals its PGID.
/// Killing the group ensures subprocesses (MCP servers, tool processes) are
/// cleaned up rather than orphaned to init on repeated crash-recovery cycles.
///
/// Uses `nix::sys::signal::killpg` — a safe wrapper around the POSIX `killpg`
/// syscall — so the crate's `#![deny(unsafe_code)]` policy is preserved.
#[cfg(unix)]
fn kill_process_group(pid: u32) -> bool {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::Pid;

    // pid == pgid because the child was spawned with process_group(0).
    killpg(Pid::from_raw(pid as i32), Signal::SIGKILL).is_ok()
}

/// Fallback for non-Unix: process-group kill not available.
/// Returns `false` so the caller falls back to `child.start_kill()`.
#[cfg(not(unix))]
fn kill_process_group(_pid: u32) -> bool {
    false
}

/// Suppress the console window that Windows otherwise allocates for every
/// console-subsystem child process spawned from a GUI (non-console) parent.
/// No-op on non-Windows platforms.
fn configure_no_window(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {

    /// §2 item 39 — a requested model is not an applied one.
    #[test]
    fn the_reported_model_prefers_the_stable_config_option() {
        let response = serde_json::json!({
            "configOptions": [{
                "category": "model",
                "id": "model",
                "currentValue": "gpt-5.6-terra",
                "options": [{"value": "gpt-5.6-terra"}, {"value": "gpt-5.6-sol"}]
            }],
            "models": {"currentModelId": "gpt-5.6-sol", "availableModels": []}
        });
        assert_eq!(
            reported_model(&response).as_deref(),
            Some("gpt-5.6-terra"),
            "the stable surface wins, exactly as the switch resolver decides"
        );
    }

    #[test]
    fn the_reported_model_falls_back_to_the_unstable_model_state() {
        let response = serde_json::json!({
            "models": {"currentModelId": "claude-sonnet-5", "availableModels": []}
        });
        assert_eq!(
            reported_model(&response).as_deref(),
            Some("claude-sonnet-5")
        );
    }

    #[test]
    fn an_adapter_that_reports_nothing_is_not_invented_for() {
        assert_eq!(reported_model(&serde_json::json!({})), None);
        assert_eq!(
            reported_model(&serde_json::json!({
                "configOptions": [{"category": "model", "id": "model", "currentValue": ""}]
            })),
            None,
            "an empty string is not a model name"
        );
    }
    use super::*;

    /// The child's env plan as `(key, Some(value) | None)`, where `None` is an
    /// explicit removal from the inherited environment.
    fn env_plan(cmd: &tokio::process::Command) -> Vec<(String, Option<String>)> {
        cmd.as_std()
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    /// A fence shaped like the coding-session provider's, without depending on
    /// that crate.
    const TEST_FENCE: EnvFence = EnvFence {
        keys: &["TYPESENSE_API_KEY", "NOSTR_PRIVATE_KEY"],
        prefixes: &["BUZZ_TEST_FENCED_"],
        exempt: &["BUZZ_TEST_FENCED_BUT_EXEMPT"],
    };

    /// The managed-agent path. A managed agent is a Buzz participant and is
    /// supposed to inherit `BUZZ_PRIVATE_KEY` from the harness, so the open
    /// fence must remove nothing at all — this is the assertion that the
    /// coding-session fix left the harness alone.
    #[test]
    fn the_default_spawn_removes_nothing_from_the_inherited_environment() {
        let extra = vec![("GOOSE_PROVIDER".to_string(), "anthropic".to_string())];
        let cmd = AcpClient::build_agent_command("true", &[], &extra, false, &EnvFence::OPEN, &[])
            .expect("build command");
        let plan = env_plan(&cmd);

        assert!(
            plan.iter().all(|(_, value)| value.is_some()),
            "the open fence removed a key: {plan:?}"
        );
        assert!(
            plan.iter()
                .any(|(key, _)| key == "GOOSE_PROVIDER" || std::env::var_os(key).is_some()),
            "per-persona env did not reach the child: {plan:?}"
        );
    }

    /// An enumerated key is removed whether or not this process has it set:
    /// the parent-presence checks in the injection loops deliberately leave
    /// inherited values alone, so `env_remove` is the only thing standing
    /// between a host's secrets and its adapter.
    #[test]
    fn an_enumerated_key_is_removed_from_the_child() {
        let extra = vec![(
            "CLAUDE_CODE_EXECUTABLE".to_string(),
            "/opt/claude".to_string(),
        )];
        let cmd = AcpClient::build_agent_command("true", &[], &extra, false, &TEST_FENCE, &[])
            .expect("build command");
        let plan = env_plan(&cmd);

        for key in TEST_FENCE.keys {
            assert_eq!(
                plan.iter()
                    .find(|(planned, _)| planned == key)
                    .map(|(_, value)| value.clone()),
                Some(None),
                "{key} was not removed from the child environment: {plan:?}"
            );
        }
        // The `|| var_os(...)` is the same isolation the sibling test above
        // uses, and it is here for the same reason: the injection loops
        // deliberately leave an inherited value alone, so on a machine whose
        // own environment already carries `CLAUDE_CODE_EXECUTABLE` the key
        // never enters the plan and this assertion failed on a tree with no
        // defect in it (finding 20, live run 2). What is being asserted is
        // that the fence did not *drop* the variable — inherited counts.
        assert!(
            plan.iter()
                .any(|(key, value)| key == "CLAUDE_CODE_EXECUTABLE" && value.is_some())
                || std::env::var_os("CLAUDE_CODE_EXECUTABLE").is_some(),
            "the fence dropped a per-runtime variable it should have kept: {plan:?}"
        );
    }

    /// The fence outranks an explicit `extra_env` entry. Anything else would
    /// let a runtime descriptor re-open the hole from the far side of a wire
    /// format.
    #[test]
    fn the_fence_outranks_an_explicit_value_for_the_same_key() {
        let extra = vec![
            (
                "BUZZ_TEST_FENCED_SECRET".to_string(),
                "nsec1leak".to_string(),
            ),
            ("NOSTR_PRIVATE_KEY".to_string(), "nsec1leak".to_string()),
            (
                "BUZZ_TEST_FENCED_BUT_EXEMPT".to_string(),
                "kept".to_string(),
            ),
        ];
        let cmd = AcpClient::build_agent_command("true", &[], &extra, false, &TEST_FENCE, &[])
            .expect("build command");
        let plan = env_plan(&cmd);

        for key in ["BUZZ_TEST_FENCED_SECRET", "NOSTR_PRIVATE_KEY"] {
            assert!(
                !plan
                    .iter()
                    .any(|(planned, value)| planned == key && value.is_some()),
                "{key} was injected despite the fence: {plan:?}"
            );
        }
        assert!(
            plan.iter()
                .any(|(key, value)| key == "BUZZ_TEST_FENCED_BUT_EXEMPT"
                    && value.as_deref() == Some("kept")),
            "the exemption did not survive its own prefix: {plan:?}"
        );
    }

    #[test]
    fn the_open_fence_covers_nothing() {
        assert!(EnvFence::OPEN.is_open());
        assert!(!EnvFence::OPEN.covers("BUZZ_PRIVATE_KEY"));
    }

    #[test]
    fn stop_reason_parses_all_known_values() {
        assert_eq!(StopReason::from_str("end_turn"), Some(StopReason::EndTurn));
        assert_eq!(
            StopReason::from_str("cancelled"),
            Some(StopReason::Cancelled)
        );
        assert_eq!(
            StopReason::from_str("max_tokens"),
            Some(StopReason::MaxTokens)
        );
        assert_eq!(
            StopReason::from_str("max_turn_requests"),
            Some(StopReason::MaxTurnRequests)
        );
        assert_eq!(StopReason::from_str("refusal"), Some(StopReason::Refusal));
    }

    #[test]
    fn stop_reason_returns_none_for_unknown() {
        assert_eq!(StopReason::from_str("unknown_value"), None);
        assert_eq!(StopReason::from_str(""), None);
        assert_eq!(StopReason::from_str("endturn"), None); // no camelCase — still unknown
    }

    #[test]
    fn stop_reason_is_case_insensitive() {
        // Agents may send uppercase or mixed-case variants — all should parse correctly.
        assert_eq!(StopReason::from_str("END_TURN"), Some(StopReason::EndTurn));
        assert_eq!(
            StopReason::from_str("CANCELLED"),
            Some(StopReason::Cancelled)
        );
        assert_eq!(
            StopReason::from_str("Max_Tokens"),
            Some(StopReason::MaxTokens)
        );
        assert_eq!(
            StopReason::from_str("MAX_TURN_REQUESTS"),
            Some(StopReason::MaxTurnRequests)
        );
        assert_eq!(StopReason::from_str("Refusal"), Some(StopReason::Refusal));
    }

    #[test]
    fn find_allow_once_by_kind_not_by_option_id() {
        // optionId values are intentionally non-obvious to prove we don't hardcode them.
        let options: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
            {"optionId": "opt-reject-42",  "name": "Reject",       "kind": "reject_once"},
            {"optionId": "opt-allow-99",   "name": "Allow once",   "kind": "allow_once"},
            {"optionId": "opt-always-7",   "name": "Always allow", "kind": "allow_always"}
        ]"#,
        )
        .unwrap();

        let allow_once = options
            .iter()
            .find(|opt| opt.get("kind").and_then(|k| k.as_str()) == Some("allow_once"));

        assert!(allow_once.is_some(), "should find allow_once option");
        let opt = allow_once.unwrap();
        // Found by kind, not by hardcoded optionId
        assert_eq!(opt["kind"].as_str(), Some("allow_once"));
        assert_eq!(opt["optionId"].as_str(), Some("opt-allow-99"));
    }

    #[test]
    fn find_allow_once_returns_none_when_absent() {
        let options: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
            {"optionId": "reject-1",      "name": "Reject",        "kind": "reject_once"},
            {"optionId": "reject-always", "name": "Always reject", "kind": "reject_always"}
        ]"#,
        )
        .unwrap();

        let allow_once = options
            .iter()
            .find(|opt| opt.get("kind").and_then(|k| k.as_str()) == Some("allow_once"));

        assert!(allow_once.is_none());
    }

    #[test]
    fn find_reject_once_fallback_when_no_allow_once() {
        let options: Vec<serde_json::Value> = serde_json::from_str(
            r#"[{"optionId": "rej-x", "name": "Reject", "kind": "reject_once"}]"#,
        )
        .unwrap();

        let allow_once = options
            .iter()
            .find(|opt| opt.get("kind").and_then(|k| k.as_str()) == Some("allow_once"));
        assert!(allow_once.is_none());

        let reject_once = options
            .iter()
            .find(|opt| opt.get("kind").and_then(|k| k.as_str()) == Some("reject_once"));
        assert!(reject_once.is_some());
        assert_eq!(reject_once.unwrap()["optionId"].as_str(), Some("rej-x"));
    }

    #[test]
    fn request_has_id_field() {
        let id: u64 = 42;
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {}
        });
        assert!(msg.get("id").is_some(), "request must have id field");
        assert_eq!(msg["id"].as_u64(), Some(42));
        assert_eq!(msg["jsonrpc"].as_str(), Some("2.0"));
        assert_eq!(msg["method"].as_str(), Some("initialize"));
    }

    #[test]
    fn notification_has_no_id_field() {
        // session/cancel is a notification — must NOT have an id field.
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {
                "sessionId": "sess_abc123"
            }
        });
        assert!(
            msg.get("id").is_none(),
            "notification must NOT have id field"
        );
        assert_eq!(msg["jsonrpc"].as_str(), Some("2.0"));
        assert_eq!(msg["method"].as_str(), Some("session/cancel"));
    }

    #[test]
    fn initialize_request_format() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0u64,
            "method": "initialize",
            "params": {
                "protocolVersion": 2,
                "clientCapabilities": build_client_capabilities(),
                "clientInfo": {
                    "name": "buzz-acp",
                    "version": "0.1.0"
                }
            }
        });
        assert_eq!(msg["params"]["protocolVersion"].as_u64(), Some(2));
        assert_eq!(
            msg["params"]["clientInfo"]["name"].as_str(),
            Some("buzz-acp")
        );
        assert!(msg["params"]["clientCapabilities"].is_object());
        assert_eq!(
            msg["params"]["clientCapabilities"]["auth"]["terminal"].as_bool(),
            Some(true),
            "terminal auth capability must be advertised so adapters can expose terminal login methods"
        );
        assert_eq!(
            msg["params"]["clientCapabilities"]["_meta"]["goose"]["customNotifications"].as_bool(),
            Some(true),
            "goose customNotifications capability must be advertised"
        );
    }

    /// The key and the type, both pinned.
    ///
    /// claude-agent-acp reads this as
    /// `capabilities?._meta?.["subagent-transcript"] === true` — a strict
    /// identity check, so the hyphenated spelling and a real JSON boolean are
    /// both load-bearing. `"true"`, `1`, or a camelCased key all silently fail
    /// the check and take the subagent's prose off the wire again, which is a
    /// regression no other assertion in this file would catch.
    #[test]
    fn the_subagent_transcript_capability_is_advertised_as_a_strict_boolean() {
        let caps = build_client_capabilities();
        let declared = &caps["_meta"]["subagent-transcript"];
        assert_eq!(
            declared.as_bool(),
            Some(true),
            "subagent transcript capability must be advertised, or a thinking \
             subagent puts nothing on the wire and the idle deadline counts \
             down through healthy work"
        );
        assert!(
            declared.is_boolean(),
            "the adapter compares with `=== true`, so a stringified or numeric \
             truthy value is not accepted"
        );
    }

    /// The version is reported back, not normalized.
    ///
    /// `normalized_agent_name` lowercases and trims because it feeds equality
    /// gates. A version feeds a human reading a stall report, so the only
    /// correct transformation is none: `"0.70.0-rc.1+Build"` must survive
    /// intact, and an adapter that volunteers `""` has told us nothing rather
    /// than told us its version is empty.
    #[test]
    fn the_adapter_version_is_reported_verbatim_or_not_at_all() {
        assert_eq!(
            reported_agent_version(&serde_json::json!({
                "agentInfo": { "name": "claude-agent-acp", "version": "0.70.0-rc.1+Build" }
            }))
            .as_deref(),
            Some("0.70.0-rc.1+Build"),
        );
        // Same fallback chain as the name, so the two never disagree about
        // which object they described.
        assert_eq!(
            reported_agent_version(&serde_json::json!({
                "serverInfo": { "version": "1.6.2" }
            }))
            .as_deref(),
            Some("1.6.2"),
        );
        for silent in [
            serde_json::json!({ "agentInfo": { "name": "x" } }),
            serde_json::json!({ "agentInfo": { "version": "" } }),
            serde_json::json!({ "agentInfo": { "version": 70 } }),
            serde_json::json!({}),
        ] {
            assert_eq!(
                reported_agent_version(&silent),
                None,
                "an adapter that reported no usable version must read as absent: {silent}"
            );
        }
    }

    /// The tail is bounded in both directions, and keeps the *end*.
    ///
    /// An adapter in a crash loop can print without limit, so neither the line
    /// count nor any single line may grow unbounded. Which end survives is the
    /// substantive half: a process explains its death in its last lines, so
    /// dropping the oldest is what makes the buffer worth keeping at all.
    #[test]
    fn the_stderr_tail_keeps_the_most_recent_lines_within_a_fixed_bound() {
        let tail = StderrTail::default();
        for i in 0..(STDERR_TAIL_LINES * 3) {
            tail.push(&format!("line {i}"));
        }
        let lines = tail.lines();
        assert_eq!(lines.len(), STDERR_TAIL_LINES, "the tail must stay bounded");
        assert_eq!(
            lines.last().map(String::as_str),
            Some(format!("line {}", STDERR_TAIL_LINES * 3 - 1).as_str()),
            "the newest line must survive"
        );
        assert!(
            !lines.iter().any(|line| line == "line 0"),
            "the oldest line must have been dropped"
        );
    }

    /// A single enormous line is truncated on a char boundary.
    ///
    /// The naive `truncate` panics mid-codepoint, which would take down the
    /// reader task and silently stop draining the pipe — the failure mode is a
    /// blocked child, not a lost log line, so it is worth a test of its own.
    #[test]
    fn an_enormous_stderr_line_is_truncated_without_splitting_a_character() {
        let tail = StderrTail::default();
        tail.push(&"é".repeat(STDERR_TAIL_LINE_BYTES));
        let lines = tail.lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("…[truncated]"));
        assert!(lines[0].len() <= STDERR_TAIL_LINE_BYTES + "…[truncated]".len());
    }

    /// Nothing captured reads as absent, not as an empty report.
    #[test]
    fn an_empty_stderr_tail_is_reported_as_nothing_rather_than_a_blank_block() {
        let tail = StderrTail::default();
        assert_eq!(tail.joined(), None);
        tail.push("boom");
        assert_eq!(tail.joined().as_deref(), Some("boom"));
    }

    #[test]
    fn session_new_mcp_server_has_required_fields() {
        // Schema requires name, command, args, env — all present, args/env may be empty.
        let server = McpServer {
            name: "test-mcp".into(),
            command: "/usr/local/bin/test-mcp-server".into(),
            args: vec![],
            env: vec![
                EnvVar {
                    name: "BUZZ_RELAY_URL".into(),
                    value: "ws://localhost:3000".into(),
                },
                EnvVar {
                    name: "BUZZ_PRIVATE_KEY".into(),
                    value: "nsec1abc".into(),
                },
            ],
        };
        let serialized = serde_json::to_value(&server).unwrap();
        assert_eq!(serialized["name"].as_str(), Some("test-mcp"));
        assert_eq!(
            serialized["command"].as_str(),
            Some("/usr/local/bin/test-mcp-server")
        );
        assert!(serialized["args"].is_array());
        assert_eq!(serialized["args"].as_array().unwrap().len(), 0);
        assert!(serialized["env"].is_array());
        assert_eq!(serialized["env"].as_array().unwrap().len(), 2);
        assert_eq!(
            serialized["env"][0]["name"].as_str(),
            Some("BUZZ_RELAY_URL")
        );
    }

    #[test]
    fn session_open_debug_logging_never_serializes_private_parameters() {
        let message = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "session/new",
            "params": {
                "cwd": "/private/checkout",
                "mcpServers": [{
                    "name": "buzz-session-context",
                    "command": "/private/buzz-session-context",
                    "args": [],
                    "env": [{
                        "name": "BUZZ_SESSION_CONTEXT_PACKAGE",
                        "value": "/private/verified-package.json",
                    }],
                }],
            },
        });

        for method in ["session/new", "session/resume", "session/load"] {
            assert!(
                acp_request_log_payload(method, &message).is_none(),
                "{method} must redact its full parameter object"
            );
        }
        let visible = acp_request_log_payload("initialize", &message).expect("ordinary request");
        assert!(visible.contains("verified-package.json"));
    }

    /// Text blocks keep the exact wire shape they have always had, and an
    /// image block renders as ACP's `{type,mimeType,data}` beside them.
    #[test]
    fn build_prompt_params_renders_text_and_image_blocks() {
        let params = build_prompt_params(
            "sess_abc123",
            &[
                PromptBlock::Text("why is this chart wrong?".into()),
                PromptBlock::Image {
                    mime: "image/png".into(),
                    data_base64: "iVBORw0KGgo=".into(),
                },
            ],
        );
        assert_eq!(params["sessionId"].as_str(), Some("sess_abc123"));
        let prompt = params["prompt"].as_array().expect("prompt array");
        assert_eq!(prompt.len(), 2);
        assert_eq!(prompt[0]["type"].as_str(), Some("text"));
        assert_eq!(prompt[0]["text"].as_str(), Some("why is this chart wrong?"));
        assert_eq!(prompt[1]["type"].as_str(), Some("image"));
        assert_eq!(prompt[1]["mimeType"].as_str(), Some("image/png"));
        assert_eq!(prompt[1]["data"].as_str(), Some("iVBORw0KGgo="));
        // An image block carries no `text` key — a client that reads one would
        // otherwise silently render an empty message.
        assert!(prompt[1].get("text").is_none());
    }

    /// A text-only prompt is byte-identical to what shipped before typed
    /// blocks existed, so widening the builder cannot have moved the wire.
    #[test]
    fn text_only_prompt_params_are_unchanged() {
        let params = build_prompt_params("s", &[PromptBlock::Text("hello".into())]);
        assert_eq!(
            serde_json::to_string(&params).expect("serialize"),
            r#"{"prompt":[{"text":"hello","type":"text"}],"sessionId":"s"}"#
        );
    }

    #[test]
    fn session_prompt_request_format() {
        let prompt_text = "[Buzz @mention]\nChannel: test\nFrom: npub1...\nMessage: hello";
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2u64,
            "method": "session/prompt",
            "params": {
                "sessionId": "sess_abc123",
                "prompt": [
                    { "type": "text", "text": prompt_text }
                ]
            }
        });
        assert_eq!(msg["method"].as_str(), Some("session/prompt"));
        let prompt = msg["params"]["prompt"].as_array().unwrap();
        assert_eq!(prompt.len(), 1);
        assert_eq!(prompt[0]["type"].as_str(), Some("text"));
        assert_eq!(prompt[0]["text"].as_str(), Some(prompt_text));
    }

    #[test]
    fn session_prompt_slash_command_two_block_format() {
        // Slash-command pass-through: bare command first, wrapped context second.
        let params = build_prompt_params(
            "sess_abc123",
            &[
                PromptBlock::Text("/goal ship it".into()),
                PromptBlock::Text("[Buzz event: @mention]\nContent: @Eva /goal ship it".into()),
            ],
        );
        let prompt = params["prompt"].as_array().unwrap();
        assert_eq!(prompt.len(), 2);
        assert_eq!(prompt[0]["type"].as_str(), Some("text"));
        assert_eq!(prompt[0]["text"].as_str(), Some("/goal ship it"));
        assert!(prompt[0]["text"].as_str().unwrap().starts_with('/'));
        assert_eq!(prompt[1]["type"].as_str(), Some("text"));
    }

    #[test]
    fn permission_response_selected_format() {
        let id: u64 = 5;
        let option_id = "opt-allow-99";
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "outcome": {
                    "outcome": "selected",
                    "optionId": option_id
                }
            }
        });
        assert_eq!(response["id"].as_u64(), Some(5));
        assert_eq!(
            response["result"]["outcome"]["outcome"].as_str(),
            Some("selected")
        );
        assert_eq!(
            response["result"]["outcome"]["optionId"].as_str(),
            Some("opt-allow-99")
        );
    }

    #[test]
    fn permission_response_cancelled_format() {
        let id: u64 = 5;
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "outcome": {
                    "outcome": "cancelled"
                }
            }
        });
        assert_eq!(
            response["result"]["outcome"]["outcome"].as_str(),
            Some("cancelled")
        );
        // cancelled outcome has no optionId
        assert!(response["result"]["outcome"].get("optionId").is_none());
    }

    #[test]
    fn session_cancel_notification_has_session_id_in_params() {
        let session_id = "sess_xyz789";
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {
                "sessionId": session_id
            }
        });
        // Must have no id (notification)
        assert!(msg.get("id").is_none());
        // Must have sessionId in params
        assert_eq!(msg["params"]["sessionId"].as_str(), Some("sess_xyz789"));
    }

    #[test]
    fn permission_request_with_string_id() {
        // Verify that permission response uses the same ID type as the request.
        // JSON-RPC 2.0 permits string IDs from the agent.
        let string_id = serde_json::json!("perm-req-001");
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": string_id,
            "result": {
                "outcome": { "outcome": "selected", "optionId": "allow-once" }
            }
        });
        assert_eq!(response["id"], "perm-req-001");
        assert!(response["id"].is_string());
    }

    #[test]
    fn id_comparison_works_for_numeric_and_string() {
        // Verify json!(expected_id) comparison logic used in read_until_response.
        let expected_id: u64 = 3;
        let numeric_response_id = serde_json::json!(3u64);
        let string_response_id = serde_json::json!("3");

        // Numeric matches
        assert_eq!(numeric_response_id, serde_json::json!(expected_id));
        // String does NOT match numeric (correct — different types)
        assert_ne!(string_response_id, serde_json::json!(expected_id));
    }

    #[test]
    fn permission_cancelled_response_preserves_id_type() {
        // String ID from agent should be echoed back as string in cancelled response.
        let string_id = serde_json::json!("req-abc");
        let cancelled = serde_json::json!({
            "jsonrpc": "2.0",
            "id": string_id.clone(),
            "result": { "outcome": { "outcome": "cancelled" } }
        });
        assert_eq!(cancelled["id"], string_id);
        assert!(cancelled["id"].is_string());

        // Numeric ID from agent should be echoed back as numeric.
        let numeric_id = serde_json::json!(42u64);
        let cancelled_numeric = serde_json::json!({
            "jsonrpc": "2.0",
            "id": numeric_id.clone(),
            "result": { "outcome": { "outcome": "cancelled" } }
        });
        assert_eq!(cancelled_numeric["id"], numeric_id);
        assert!(cancelled_numeric["id"].is_number());
    }

    #[test]
    fn extract_model_config_options_finds_model_category() {
        let result = serde_json::json!({
            "sessionId": "sess-1",
            "configOptions": [
                {
                    "configId": "model",
                    "category": "model",
                    "displayName": "Model",
                    "options": [
                        { "value": "claude-sonnet-4-20250514", "displayName": "Claude Sonnet 4" },
                        { "value": "claude-opus-4-20250514", "displayName": "Claude Opus 4" }
                    ]
                },
                {
                    "configId": "theme",
                    "category": "appearance",
                    "displayName": "Theme",
                    "options": [{ "value": "dark", "displayName": "Dark" }]
                }
            ]
        });
        let opts = super::extract_model_config_options(&result);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0]["configId"].as_str(), Some("model"));
    }

    #[test]
    fn extract_model_config_options_empty_when_no_config_options() {
        let result = serde_json::json!({ "sessionId": "sess-1" });
        assert!(super::extract_model_config_options(&result).is_empty());
    }

    #[test]
    fn extract_model_config_options_empty_when_no_model_category() {
        let result = serde_json::json!({
            "configOptions": [
                { "configId": "theme", "category": "appearance" }
            ]
        });
        assert!(super::extract_model_config_options(&result).is_empty());
    }

    #[test]
    fn extract_model_state_returns_models_object() {
        let result = serde_json::json!({
            "sessionId": "sess-1",
            "models": {
                "currentModelId": "gpt-5",
                "availableModels": [
                    { "modelId": "gpt-5", "name": "GPT-5" },
                    { "modelId": "o3-pro", "name": "o3 Pro" }
                ]
            }
        });
        let ms = super::extract_model_state(&result).expect("should have models");
        assert_eq!(ms["currentModelId"].as_str(), Some("gpt-5"));
        assert_eq!(ms["availableModels"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn extract_model_state_none_when_absent() {
        let result = serde_json::json!({ "sessionId": "sess-1" });
        assert!(super::extract_model_state(&result).is_none());
    }

    #[test]
    fn resolve_prefers_stable_over_unstable() {
        let result = serde_json::json!({
            "configOptions": [{
                "configId": "model",
                "category": "model",
                "options": [
                    { "value": "claude-sonnet-4-20250514", "displayName": "Sonnet 4" }
                ]
            }],
            "models": {
                "currentModelId": "claude-sonnet-4-20250514",
                "availableModels": [
                    { "modelId": "claude-sonnet-4-20250514", "name": "Sonnet 4" }
                ]
            }
        });
        let method = super::resolve_model_switch_method(&result, "claude-sonnet-4-20250514");
        assert_eq!(
            method,
            Some(super::ModelSwitchMethod::ConfigOption {
                config_id: "model".to_string(),
                option_value: "claude-sonnet-4-20250514".to_string(),
            })
        );
    }

    #[test]
    fn resolve_accepts_id_keyed_config_options() {
        // claude-agent-acp (observed on v0.61.0) keys config options with
        // `id` instead of the spec's `configId`. Payload mirrors its real
        // `session/new` response.
        let result = serde_json::json!({
            "configOptions": [{
                "id": "model",
                "name": "Model",
                "category": "model",
                "type": "select",
                "currentValue": "default",
                "options": [
                    { "value": "default", "name": "Default" },
                    { "value": "opus[1m]", "name": "Opus" },
                    { "value": "sonnet", "name": "Sonnet" }
                ]
            }],
            "models": null
        });
        let method = super::resolve_model_switch_method(&result, "opus[1m]");
        assert_eq!(
            method,
            Some(super::ModelSwitchMethod::ConfigOption {
                config_id: "model".to_string(),
                option_value: "opus[1m]".to_string(),
            })
        );
    }

    #[test]
    fn resolve_falls_back_to_unstable() {
        let result = serde_json::json!({
            "models": {
                "currentModelId": "gpt-5",
                "availableModels": [
                    { "modelId": "gpt-5", "name": "GPT-5" },
                    { "modelId": "o3-pro", "name": "o3 Pro" }
                ]
            }
        });
        let method = super::resolve_model_switch_method(&result, "o3-pro");
        assert_eq!(
            method,
            Some(super::ModelSwitchMethod::SetModel {
                model_id: "o3-pro".to_string(),
            })
        );
    }

    #[test]
    fn resolve_returns_none_when_no_match() {
        let result = serde_json::json!({
            "configOptions": [{
                "configId": "model",
                "category": "model",
                "options": [{ "value": "claude-sonnet-4-20250514" }]
            }],
            "models": {
                "availableModels": [{ "modelId": "gpt-5" }]
            }
        });
        assert!(super::resolve_model_switch_method(&result, "nonexistent-model").is_none());
    }

    #[test]
    fn resolve_returns_none_when_no_model_info() {
        let result = serde_json::json!({ "sessionId": "sess-1" });
        assert!(super::resolve_model_switch_method(&result, "anything").is_none());
    }

    #[test]
    fn resolve_handles_multiple_config_options() {
        // Agent could have multiple configOptions with category "model"
        // (unlikely but defensive).
        let result = serde_json::json!({
            "configOptions": [
                {
                    "configId": "primary-model",
                    "category": "model",
                    "options": [{ "value": "model-a" }]
                },
                {
                    "configId": "fallback-model",
                    "category": "model",
                    "options": [{ "value": "model-b" }]
                }
            ]
        });
        let method = super::resolve_model_switch_method(&result, "model-b");
        assert_eq!(
            method,
            Some(super::ModelSwitchMethod::ConfigOption {
                config_id: "fallback-model".to_string(),
                option_value: "model-b".to_string(),
            })
        );
    }

    // ── model_in_catalog tests ────────────────────────────────────────────

    #[test]
    fn model_in_catalog_true_when_in_config_options() {
        let config_options = vec![serde_json::json!({
            "configId": "model",
            "category": "model",
            "options": [
                { "value": "claude-sonnet-4-20250514" },
                { "value": "claude-opus-4-20250514" }
            ]
        })];
        assert!(super::model_in_catalog(
            &config_options,
            None,
            "claude-opus-4-20250514"
        ));
    }

    #[test]
    fn model_in_catalog_true_when_in_available_models() {
        let available = serde_json::json!({
            "currentModelId": "gpt-5",
            "availableModels": [
                { "modelId": "gpt-5" },
                { "modelId": "o3-pro" }
            ]
        });
        assert!(super::model_in_catalog(&[], Some(&available), "o3-pro"));
    }

    #[test]
    fn model_in_catalog_false_when_absent_from_both_halves() {
        let config_options = vec![serde_json::json!({
            "configId": "model",
            "options": [{ "value": "claude-sonnet-4-20250514" }]
        })];
        let available = serde_json::json!({
            "availableModels": [{ "modelId": "gpt-5" }]
        });
        assert!(!super::model_in_catalog(
            &config_options,
            Some(&available),
            "nonexistent-model"
        ));
    }

    #[test]
    fn model_in_catalog_false_when_both_halves_empty() {
        assert!(!super::model_in_catalog(&[], None, "anything"));
    }

    // ── Error variant display ─────────────────────────────────────────────

    #[test]
    fn idle_timeout_error_includes_duration() {
        let err = AcpError::IdleTimeout {
            timeout: std::time::Duration::from_secs(320),
            wire: Box::new(TurnWireSummary::default()),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("320"),
            "IdleTimeout display should include duration: {msg}"
        );
    }

    #[test]
    fn hard_timeout_error_display() {
        let err = AcpError::HardTimeout {
            silence: std::time::Duration::from_secs(120),
            wire: Box::new(TurnWireSummary::default()),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("Hard turn timeout"),
            "HardTimeout display: {msg}"
        );
    }

    /// The whole point of carrying the summary is that nobody should have to
    /// subtract the budget from the span by hand to learn when the wire went
    /// quiet. If the numbers stop reaching the message, that is back.
    #[test]
    fn a_timeout_says_when_the_wire_actually_went_quiet() {
        let err = AcpError::IdleTimeout {
            timeout: std::time::Duration::from_secs(870),
            wire: Box::new(TurnWireSummary {
                frames: 47,
                bytes: 18 * 1024,
                subagent_frames: 0,
                turn_elapsed: std::time::Duration::from_secs(952),
                first_frame_offset: Some(std::time::Duration::from_millis(300)),
                last_frame_offset: Some(std::time::Duration::from_millis(52_100)),
                last_frame_kind: Some("agent_message_chunk".into()),
                quiet_for: std::time::Duration::from_secs(900),
                tools_in_flight: 0,
                answer_streamed: true,
                kinds: std::collections::BTreeMap::new(),
            }),
        };
        let msg = err.to_string();
        for expected in [
            "last agent_message_chunk at +52.1s",
            "0 tools in flight",
            "answer streamed",
            "47 frames",
        ] {
            assert!(
                msg.contains(expected),
                "a stalled turn must report {expected:?} without arithmetic: {msg}"
            );
        }
    }

    /// The three deadlines report the same shape. A reader should not have to
    /// learn which words a given budget happens to use.
    #[test]
    fn every_deadline_reports_the_same_diagnostics() {
        let wire = || {
            Box::new(TurnWireSummary {
                frames: 3,
                last_frame_kind: Some("tool_call_update".into()),
                last_frame_offset: Some(std::time::Duration::from_secs(9)),
                ..TurnWireSummary::default()
            })
        };
        for err in [
            AcpError::IdleTimeout {
                timeout: std::time::Duration::from_secs(870),
                wire: wire(),
            },
            AcpError::HardTimeout {
                silence: std::time::Duration::from_secs(30),
                wire: wire(),
            },
            AcpError::AnswerStall {
                quiet: std::time::Duration::from_secs(120),
                wire: wire(),
            },
        ] {
            let msg = err.to_string();
            assert!(
                msg.contains("last tool_call_update at +9.0s"),
                "every deadline should name the last frame: {msg}"
            );
        }
    }

    /// A long tail of one-off frame kinds must not push the load-bearing facts
    /// past the renderer's row limit.
    #[test]
    fn the_kind_census_is_bounded_to_the_frequent_few() {
        let mut kinds = std::collections::BTreeMap::new();
        for (i, n) in [90u64, 80, 70, 60, 50, 40].into_iter().enumerate() {
            kinds.insert(format!("kind_{i}"), n);
        }
        let summary = TurnWireSummary {
            kinds,
            ..TurnWireSummary::default()
        };
        let top = summary.top_kinds(3);
        assert_eq!(top.len(), 3);
        assert_eq!(top[0], ("kind_0".to_owned(), 90));
        assert_eq!(top[2], ("kind_2".to_owned(), 70));
        let line = summary.one_line();
        assert!(
            !line.contains("kind_3"),
            "census must stop at the limit: {line}"
        );
    }

    /// `classify` is what makes a stall readable at a glance, so the mapping
    /// from wire shape to bucket name is pinned rather than incidental.
    #[test]
    fn frames_are_classified_by_what_a_reader_would_look_for() {
        let cases = [
            (
                serde_json::json!({
                    "method": "session/update",
                    "params": {"update": {"sessionUpdate": "agent_message_chunk"}}
                }),
                "agent_message_chunk",
            ),
            (
                serde_json::json!({"method": "session/request_permission", "id": 4}),
                "session/request_permission",
            ),
            (serde_json::json!({"id": 7, "result": {}}), "response"),
            (serde_json::json!({"jsonrpc": "2.0"}), "unknown"),
        ];
        for (msg, expected) in cases {
            assert_eq!(TurnWire::classify(&msg), expected, "for {msg}");
        }
    }

    /// A subagent's frames count as activity but are not the turn's answer.
    /// Reporting them separately is what stops "47 frames arrived" from
    /// reading as "the agent was working" when all 47 came from a subagent.
    #[test]
    fn subagent_frames_are_counted_apart() {
        let now = tokio::time::Instant::now();
        let mut wire = TurnWire::new(now);
        wire.record(
            &serde_json::json!({
                "method": "session/update",
                "params": {"update": {
                    "sessionUpdate": "agent_message_chunk",
                    "_meta": {"claudeCode": {"parentToolUseId": "toolu_parent"}}
                }}
            }),
            120,
            now,
        );
        wire.record(
            &serde_json::json!({
                "method": "session/update",
                "params": {"update": {"sessionUpdate": "agent_message_chunk"}}
            }),
            80,
            now,
        );
        let summary = wire.summarize(now, &AnswerStallWatch::new(None));
        assert_eq!(summary.frames, 2);
        assert_eq!(summary.subagent_frames, 1);
        assert_eq!(summary.bytes, 200);
        assert!(summary.one_line().contains("1 subagent frames"));
    }

    async fn spawn_script(script: &str) -> AcpClient {
        AcpClient::spawn("bash", &["-c".into(), script.into()], &[], false)
            .await
            .expect("failed to spawn test script")
    }

    #[cfg(unix)]
    async fn spawn_named_script(name: &str, script: &str) -> (AcpClient, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "buzz-acp-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp adapter dir");
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/usr/bin/env bash\n{script}\n"))
            .expect("write fake adapter");
        let mut permissions = std::fs::metadata(&path)
            .expect("adapter metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("chmod fake adapter");
        let client = AcpClient::spawn(path.to_str().expect("utf8 path"), &[], &[], false)
            .await
            .expect("spawn named fake adapter");
        (client, dir)
    }

    /// Spawn a probe script whose file name carries a runtime identity (e.g.
    /// `hermes-acp`) and return the value of `var` as the child observed it.
    /// `<unset>` means the child did not receive the var.
    #[cfg(unix)]
    async fn spawn_named_and_read_child_env(
        file_name: &str,
        var: &str,
        extra_env: &[(String, String)],
    ) -> String {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("buzz-acp-env-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create env probe dir");
        let path = dir.join(file_name);
        std::fs::write(
            &path,
            format!("#!/bin/sh\nprintf '%s\\n' \"${{{var}:-<unset>}}\"\n"),
        )
        .expect("write env probe script");
        let mut permissions = std::fs::metadata(&path).expect("stat probe").permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).expect("chmod probe");

        let mut client = AcpClient::spawn(
            path.to_str().expect("probe path is UTF-8"),
            &[],
            extra_env,
            false,
        )
        .await
        .expect("spawn env probe script");
        let observed = client
            .reader
            .next()
            .await
            .unwrap_or_else(|| panic!("child produced no output for {var}"))
            .expect("child stdout was not readable");
        client.shutdown().await;
        std::fs::remove_dir_all(&dir).expect("remove env probe dir");
        observed
    }

    /// Buzz-owned Hermes processes get the configured-MCP isolation default,
    /// and an explicit persona entry still overrides it (defaults are applied
    /// before `extra_env`, so the later `Command::env` write wins).
    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_applies_runtime_env_defaults_with_extra_env_precedence() {
        const VAR: &str = "HERMES_ACP_SKIP_CONFIGURED_MCP";
        if std::env::var_os(VAR).is_some() {
            // Inherited parent values win over both layers; the default and
            // override behavior below is unobservable in such an environment.
            return;
        }

        assert_eq!(
            spawn_named_and_read_child_env("hermes-acp", VAR, &[]).await,
            "1",
            "Hermes spawns must default {VAR}=1"
        );
        assert_eq!(
            spawn_named_and_read_child_env("hermes-acp", VAR, &[(VAR.into(), "0".into())]).await,
            "0",
            "an explicit extra_env entry must override the runtime default"
        );
        assert_eq!(
            spawn_named_and_read_child_env("other-agent", VAR, &[]).await,
            "<unset>",
            "non-Hermes spawns must not receive Hermes defaults"
        );
    }

    #[tokio::test]
    async fn idle_timeout_fires_on_silent_process() {
        let mut client = spawn_script("sleep 10").await;
        let max_dur = std::time::Duration::from_secs(30);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_millis(100),
                hard_deadline,
                max_dur,
            )
            .await;
        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "expected IdleTimeout, got {result:?}"
        );
    }

    #[tokio::test]
    async fn hard_timeout_fires_when_deadline_is_immediate() {
        let mut client = spawn_script("while true; do echo 'noise'; sleep 0.01; done").await;
        let max_dur = std::time::Duration::from_millis(1);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_secs(60),
                hard_deadline,
                max_dur,
            )
            .await;
        assert!(
            matches!(result, Err(AcpError::HardTimeout { .. })),
            "expected HardTimeout, got {result:?}"
        );
    }

    /// `cancel_with_cleanup_grace`'s bounded drain deadline must map to
    /// [`AcpError::CancelDrainTimeout`], never [`AcpError::HardTimeout`] —
    /// the two share an underlying deadline mechanism but must not share
    /// classification, since callers dead-letter a real `HardTimeout` and
    /// must not dead-letter a drain that simply ran past its grace window.
    #[tokio::test]
    async fn cancel_with_cleanup_grace_maps_expiry_to_cancel_drain_timeout() {
        // Agent ignores `session/cancel` on stdin and keeps producing noise
        // forever — never drains within the grace window.
        let mut client = spawn_script("while true; do echo 'noise'; sleep 0.01; done").await;
        client.last_prompt_id = Some(999);
        let grace = std::time::Duration::from_millis(200);
        let result = client
            .cancel_with_cleanup_grace("test-session", grace)
            .await;
        assert!(
            matches!(result, Err(AcpError::CancelDrainTimeout(g)) if g == grace),
            "expected CancelDrainTimeout({grace:?}), got {result:?}"
        );
    }

    #[tokio::test]
    async fn idle_resets_on_stdout_activity() {
        // Send valid JSON (session/update notifications) to reset the idle timer.
        // Non-JSON lines no longer reset idle — only valid JSON notifications do.
        let mut client = spawn_script(
            r#"for i in $(seq 1 10); do echo '{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_thought_chunk","content":{"text":"thinking"}}}}'; sleep 0.05; done; sleep 10"#,
        )
        .await;
        let max_dur = std::time::Duration::from_secs(10);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let start = std::time::Instant::now();
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_millis(200),
                hard_deadline,
                max_dur,
            )
            .await;
        let elapsed = start.elapsed();
        // 10 messages × 50ms = ~500ms of activity, then idle timeout fires after 200ms more
        assert!(elapsed >= std::time::Duration::from_millis(400));
        assert!(elapsed < std::time::Duration::from_secs(3));
        assert!(matches!(result, Err(AcpError::IdleTimeout { .. })));
    }

    #[tokio::test]
    async fn response_returned_when_matching_id_arrives() {
        let mut client =
            spawn_script(r#"echo '{"jsonrpc":"2.0","id":42,"result":{"stopReason":"end_turn"}}'"#)
                .await;
        let max_dur = std::time::Duration::from_secs(5);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                42,
                std::time::Duration::from_secs(2),
                hard_deadline,
                max_dur,
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["stopReason"].as_str(), Some("end_turn"));
    }

    #[tokio::test]
    async fn agent_exit_detected_as_eof() {
        let mut client = spawn_script("exit 0").await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let max_dur = std::time::Duration::from_secs(5);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_secs(2),
                hard_deadline,
                max_dur,
            )
            .await;
        assert!(matches!(result, Err(AcpError::AgentExited)));
    }

    /// A message with both `id` and `method` is an agent-initiated request,
    /// not a response. The response matcher must not consume it even if the
    /// id happens to match the expected value.
    #[tokio::test]
    async fn agent_request_with_matching_id_not_consumed_as_response() {
        // The script sends an agent-initiated request (has both id and method)
        // whose id matches what we're waiting for (0), then sends the real
        // response. The request should be dispatched (triggering -32601 since
        // "test/method" is unknown), and the real response should be returned.
        let script = r#"
            echo '{"jsonrpc":"2.0","id":0,"method":"test/method","params":{}}'
            read -t 2 _reply
            echo '{"jsonrpc":"2.0","id":0,"result":{"ok":true}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        let max_dur = std::time::Duration::from_secs(5);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                0,
                std::time::Duration::from_secs(3),
                hard_deadline,
                max_dur,
            )
            .await;
        assert!(result.is_ok(), "expected Ok response, got {result:?}");
        assert_eq!(result.unwrap()["ok"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn idle_fires_before_hard_when_idle_is_shorter() {
        let mut client = spawn_script("sleep 10").await;
        let idle = std::time::Duration::from_millis(100);
        let max_dur = std::time::Duration::from_secs(10);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let result = client
            .read_until_response_with_idle_timeout("test", 999, idle, hard_deadline, max_dur)
            .await;
        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "idle should fire before hard when idle << hard, got {result:?}"
        );
    }

    /// Hard-deadline starvation regression (Max's review gate, Eva's required test).
    ///
    /// When the read-loop became a `tokio::select!` with `biased; reader →
    /// steer → sleep_until`, a continuously-ready reader arm could win every
    /// poll and starve the timer arm — silently defeating the hard-deadline
    /// guarantee. The fix is a pre-select deadline check at the top of every
    /// loop iteration; this test pins that behavior.
    ///
    /// Setup: agent emits a **gapless** stream of valid JSON `session/update`
    /// notifications (no `sleep` between lines) so the reader arm is
    /// continuously ready. Each line is valid JSON, so it resets the idle
    /// clock — and we set idle ≫ hard so idle cannot fire first. With
    /// `biased; reader → steer → sleep_until`, the reader arm would win
    /// every poll and `sleep_until` would never be reached. Only the
    /// pre-select deadline check at the top of the loop can stop us.
    ///
    /// Without the pre-select check, this test hangs against the infinite
    /// bash subprocess until the test harness's own outer timeout, and the
    /// returned error would never be `HardTimeout`.
    #[tokio::test]
    async fn hard_deadline_fires_under_continuous_valid_json_stream() {
        // Truly infinite, gapless stream of valid JSON. No `sleep` between
        // echoes — the reader arm is continuously ready, which is the
        // exact starvation scenario the pre-select check guards against.
        // `while :; do echo ...; done` (not a fixed-count `for`) so the
        // subprocess never naturally exits before the hard deadline,
        // regardless of how fast the host drains bash output. Without
        // this, fast hardware drains a bounded loop in < hard_deadline
        // and the reader hits EOF (`AgentExited`) before the timer fires,
        // masking whether the pre-select check actually works.
        let mut client = spawn_script(
            r#"while :; do echo '{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"x"}}}}'; done"#,
        )
        .await;
        let hard = std::time::Duration::from_millis(300);
        let hard_deadline = tokio::time::Instant::now() + hard;
        let idle = std::time::Duration::from_secs(60); // idle ≫ hard
        let start = std::time::Instant::now();
        let result = client
            .read_until_response_with_idle_timeout("test", 999, idle, hard_deadline, hard)
            .await;
        let elapsed = start.elapsed();
        assert!(
            matches!(result, Err(AcpError::HardTimeout { .. })),
            "expected HardTimeout under gapless valid-JSON stream, got {result:?} (elapsed {elapsed:?})"
        );
        // Must fire close to the hard deadline, not late. Without the
        // pre-select check the reader arm starves sleep_until and elapsed
        // tracks the bash subprocess lifetime instead.
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "HardTimeout fired late ({elapsed:?}); reader arm may be starving sleep_until"
        );
    }

    /// Same as `agent_request_with_matching_id_not_consumed_as_response` but
    /// exercises the non-idle `read_until_response` path (via `send_request`).
    #[tokio::test]
    async fn agent_request_not_consumed_via_send_request() {
        // Script: wait for the initialize request, reply, then send an
        // agent-initiated request with id=1 (matching the next send_request id),
        // wait for the -32601 error reply, then send the real response.
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 _req
            echo '{"jsonrpc":"2.0","id":1,"method":"test/unknown","params":{}}'
            read -t 2 _err_reply
            echo '{"jsonrpc":"2.0","id":1,"result":{"worked":true}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        // initialize consumes id=0
        let _init = client
            .initialize()
            .await
            .expect("initialize should succeed");
        // send_request uses id=1 — the agent's request with id=1 and method
        // must not be consumed as the response.
        let result = client
            .send_request("test/echo", serde_json::json!({}))
            .await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(result.unwrap()["worked"], serde_json::json!(true));
    }

    /// Keepalive `session/update` lines push the idle deadline past its
    /// original expiry, and once they stop the idle timeout fires.
    ///
    /// Measured on a [`ManualTurnClock`], not on wall time. The wall-clock
    /// version of this test asserted `elapsed >= 500ms` while a `sleep 0.05`
    /// loop in a spawned `sh` supplied the keepalives — so it was really
    /// asserting that the operating system would schedule that subprocess 20
    /// times inside half a second. Under load average 173 it did not, the
    /// window expired between keepalives, and the test failed with "elapsed
    /// only 372ms": a starved subprocess, reported as a broken reset rule.
    ///
    /// Here each keepalive is released by a gate file, and the gap before it is
    /// an advance of the clock. Ten gaps of 90 ms pass inside a 100 ms window —
    /// nine times the deadline — and the turn survives, because every frame
    /// resets the window (`acp.rs`, the `activity_now` reset in the read loop).
    /// The eleventh advance carries the clock past the window with the agent
    /// silent, and the timeout fires. No `assert` in this test reads wall time.
    #[cfg(unix)]
    #[tokio::test]
    async fn keepalive_resets_idle_past_deadline() {
        const KEEPALIVES: usize = 10;
        const WINDOW: std::time::Duration = std::time::Duration::from_millis(100);
        // Under the window, so no single gap may expire it; ten of them are
        // nine deadlines' worth of clock time.
        const GAP: std::time::Duration = std::time::Duration::from_millis(90);

        let dir = std::env::temp_dir().join(format!("buzz-acp-keepalive-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create gate dir");
        let gates: Vec<std::path::PathBuf> = (0..KEEPALIVES)
            .map(|i| dir.join(format!("keepalive-{i}")))
            .collect();

        let frame = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"keepalive"}}}"#;
        let mut script = String::new();
        for gate in &gates {
            script.push_str(&format!("echo '{frame}'\n"));
            // A gate file, not a duration: the script waits for the test, so
            // the frame's arrival is ordered against the clock advance rather
            // than racing it. The poll interval is wall time but bounds
            // nothing — no deadline can expire while the clock is still.
            script.push_str(&format!(
                "while [ ! -e '{}' ]; do sleep 0.01; done\n",
                gate.display()
            ));
        }
        // Then silence, so the idle timeout has something to fire on.
        script.push_str("sleep 30\n");

        let mut client = spawn_script(&script).await;
        let observer = crate::observer::ObserverHandle::in_process();
        let mut reads = observer.subscribe();
        client.set_observer(Some(observer), 0);

        let clock = idle_clock::ManualTurnClock::frozen();
        client.set_turn_clock(clock.clone());
        let started_at = clock.now();
        let hard_deadline = clock.now() + std::time::Duration::from_secs(3600);

        let driver = async {
            for gate in &gates {
                // Wait for the read loop to take this keepalive in, so the
                // advance below is measured from *its* reset and not from the
                // previous frame's.
                loop {
                    let event = reads.recv().await.expect("observer feed closed");
                    if event.kind == "acp_read" {
                        break;
                    }
                }
                clock.advance(GAP);
                std::fs::write(gate, b"").expect("open gate");
            }
            // The agent is silent from here. One advance past the window ends
            // the turn.
            clock.advance(WINDOW + std::time::Duration::from_millis(1));
        };

        let (result, ()) = tokio::join!(
            client.read_until_response_with_idle_timeout(
                "test",
                999,
                WINDOW,
                hard_deadline,
                std::time::Duration::from_secs(3600),
            ),
            driver,
        );

        let survived = clock.now().saturating_duration_since(started_at);
        assert!(
            survived >= GAP * KEEPALIVES as u32,
            "keepalives must reset the idle window past its deadline: only \
             {survived:?} of clock time passed inside a {WINDOW:?} window"
        );
        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "the turn must end on the idle window once the keepalives stop, \
             got {result:?}"
        );
        drop(client);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// One `session/update` line for the scripted-stream stall tests.
    ///
    /// Built rather than written out so the four tests below cannot drift into
    /// disagreeing about the frame shape they are asserting on.
    fn stall_frame(update: serde_json::Value) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "update": update },
        })
        .to_string()
    }

    fn top_level_prose() -> String {
        stall_frame(serde_json::json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "done" },
        }))
    }

    fn task_call_opened() -> String {
        stall_frame(serde_json::json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Task",
        }))
    }

    fn task_call_completed() -> String {
        stall_frame(serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "status": "completed",
        }))
    }

    fn subagent_prose() -> String {
        stall_frame(serde_json::json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "child" },
            "_meta": { "claudeCode": { "parentToolUseId": "t1" } },
        }))
    }

    /// The reported failure, reproduced: the answer streams, the tool finishes,
    /// and the prompt response never comes.
    ///
    /// The turn must end on the *stall* budget, not the idle one, and must say
    /// which — the point is that the operator learns the adapter dropped the
    /// prompt rather than that "the agent went quiet".
    #[tokio::test]
    async fn a_turn_that_answers_and_then_never_resolves_ends_on_the_stall_budget() {
        let script = format!(
            "echo '{}'; echo '{}'; echo '{}'; sleep 10",
            task_call_opened(),
            task_call_completed(),
            top_level_prose(),
        );
        let mut client = spawn_script(&script).await;
        client.set_answer_stall_timeout(Some(std::time::Duration::from_millis(150)));

        let max_dur = std::time::Duration::from_secs(30);
        let started = std::time::Instant::now();
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                // An idle budget far larger than the stall budget: without the
                // stall watch this would sit here for the full ten seconds and
                // then fail on the wrong error.
                std::time::Duration::from_secs(10),
                tokio::time::Instant::now() + max_dur,
                max_dur,
            )
            .await;

        assert!(
            matches!(result, Err(AcpError::AnswerStall { .. })),
            "expected an answer stall, got {result:?}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the stall budget must fire long before the idle budget"
        );
    }

    /// The guard must not arm while the agent is still working.
    ///
    /// This is the consequential direction: a tool call that runs longer than
    /// the stall budget — a build, a test suite, a subagent — must be left to
    /// the idle budget alone.
    #[tokio::test]
    async fn an_unfinished_tool_call_is_never_treated_as_a_stall() {
        let script = format!(
            "echo '{}'; echo '{}'; sleep 10",
            top_level_prose(),
            task_call_opened(),
        );
        let mut client = spawn_script(&script).await;
        client.set_answer_stall_timeout(Some(std::time::Duration::from_millis(100)));

        let max_dur = std::time::Duration::from_secs(30);
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_millis(400),
                tokio::time::Instant::now() + max_dur,
                max_dur,
            )
            .await;

        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "a tool still in flight must fall to the idle budget, got {result:?}"
        );
    }

    /// Narration before a tool call is not the turn's answer either.
    ///
    /// Claude narrates ("I'll orient first…"), calls tools, and answers after
    /// the results. A lead that went quiet right after its last tool result had
    /// not answered anything, yet the watch — armed by the narration — closed
    /// the turn at 120s and published "the answer above is complete" over a
    /// turn that decided nothing (ledger 166). Silence there belongs to the
    /// idle budget, and the disclosure it earns says the agent went quiet.
    #[tokio::test]
    async fn narration_before_a_tool_call_does_not_arm_the_stall_watch() {
        let script = format!(
            "echo '{}'; echo '{}'; echo '{}'; sleep 10",
            top_level_prose(),
            task_call_opened(),
            task_call_completed(),
        );
        let mut client = spawn_script(&script).await;
        client.set_answer_stall_timeout(Some(std::time::Duration::from_millis(100)));

        let max_dur = std::time::Duration::from_secs(30);
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_millis(400),
                tokio::time::Instant::now() + max_dur,
                max_dur,
            )
            .await;

        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "prose before the last tool call must not arm the stall watch, got {result:?}"
        );
    }

    /// A subagent's prose is not the turn's answer.
    ///
    /// Were it counted, a subagent narrating its work would arm the watch while
    /// its Task call is still legitimately running, and the guard would kill
    /// exactly the turns it exists to protect.
    #[tokio::test]
    async fn a_subagents_prose_does_not_arm_the_stall_watch() {
        let script = format!(
            "echo '{}'; for i in $(seq 1 10); do echo '{}'; sleep 0.05; done; sleep 10",
            task_call_opened(),
            subagent_prose(),
        );
        let mut client = spawn_script(&script).await;
        client.set_answer_stall_timeout(Some(std::time::Duration::from_millis(100)));

        let max_dur = std::time::Duration::from_secs(30);
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_millis(400),
                tokio::time::Instant::now() + max_dur,
                max_dur,
            )
            .await;

        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "subagent prose must not count as the turn's answer, got {result:?}"
        );
    }

    /// The watch is opt-in, and off by default.
    #[tokio::test]
    async fn the_stall_watch_does_nothing_until_it_is_configured() {
        let script = format!("echo '{}'; sleep 10", top_level_prose());
        let mut client = spawn_script(&script).await;

        let max_dur = std::time::Duration::from_secs(30);
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_millis(200),
                tokio::time::Instant::now() + max_dur,
                max_dur,
            )
            .await;

        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "an unconfigured client must behave exactly as before, got {result:?}"
        );
    }

    #[tokio::test]
    async fn tool_call_resets_idle_then_silence_times_out() {
        // A tool_call session/update resets the idle timer (belt-and-suspenders path),
        // then silence causes idle timeout. This proves the reset works for tool_call
        // specifically — not just via the general valid-JSON reset at line 839.
        //
        // The script emits a tool_call, waits 80ms (under the 200ms idle), then goes
        // silent. If the tool_call reset didn't fire, idle would fire at 200ms from
        // start. With the reset, idle fires at 80ms + 200ms = ~280ms from start.
        let mut client = spawn_script(
            r#"echo '{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call","title":"long_running","kind":"shell"}}}'; sleep 0.08; sleep 10"#,
        )
        .await;
        let max_dur = std::time::Duration::from_secs(10);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let start = std::time::Instant::now();
        let result = client
            .read_until_response_with_idle_timeout(
                "test",
                999,
                std::time::Duration::from_millis(200),
                hard_deadline,
                max_dur,
            )
            .await;
        let elapsed = start.elapsed();
        // The tool_call arrives near-instantly and resets idle.
        // Then 80ms of silence, then idle fires at ~280ms from start.
        // Must be > 200ms (proves the reset happened after the tool_call).
        assert!(
            elapsed >= std::time::Duration::from_millis(200),
            "tool_call should reset idle; elapsed only {elapsed:?}"
        );
        assert!(elapsed < std::time::Duration::from_secs(2));
        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "expected IdleTimeout after silence, got {result:?}"
        );
    }

    /// The flag has to reach `session/new`, because that is the only place the
    /// adapter ever reads it. A later toggle is silently inert.
    #[tokio::test]
    async fn raw_sdk_frames_are_requested_on_the_session_that_opts_in() {
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client.initialize().await.expect("initialize");
        client.set_emit_raw_sdk_frames(true);
        let response = client
            .session_new_full("/tmp", vec![], None, None)
            .await
            .expect("session/new");
        let sent = &response.raw["_receivedRequest"]["params"];
        assert_eq!(
            sent.pointer("/_meta/claudeCode/emitRawSDKMessages"),
            Some(&serde_json::json!(true)),
            "the adapter only reads this on session/new: {sent}"
        );
    }

    /// Default off, and off means the key is absent rather than `false`.
    /// A session that never asked should be byte-identical to one from before
    /// the flag existed.
    #[tokio::test]
    async fn a_session_that_did_not_opt_in_sends_no_raw_frame_key() {
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client.initialize().await.expect("initialize");
        let response = client
            .session_new_full("/tmp", vec![], None, None)
            .await
            .expect("session/new");
        let sent = &response.raw["_receivedRequest"]["params"];
        assert_eq!(
            sent.pointer("/_meta/claudeCode"),
            None,
            "off must mean absent, not false: {sent}"
        );
    }

    /// The seat fence is only real if the denial reaches the adapter. It is
    /// read off `session/new`'s `_meta`, merged into the SDK query there and
    /// nowhere else, so a list set after the open is inert.
    #[tokio::test]
    async fn denied_tools_reach_session_new_meta() {
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client.initialize().await.expect("initialize");
        client.set_disallowed_tools(&["Task", "Agent", "SendMessage"]);
        let response = client
            .session_new_full("/tmp", vec![], None, None)
            .await
            .expect("session/new");
        let sent = &response.raw["_receivedRequest"]["params"];
        assert_eq!(
            sent.pointer("/_meta/claudeCode/options/disallowedTools"),
            Some(&serde_json::json!(["Task", "Agent", "SendMessage"])),
            "the adapter merges this key into the SDK query: {sent}"
        );
    }

    /// A reattachment re-enters the same conversation on a fresh adapter
    /// process, so the denial has to be restated or the fence lapses at the
    /// first resume.
    #[tokio::test]
    async fn denied_tools_reach_session_resume_meta() {
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client.initialize().await.expect("initialize");
        client.set_disallowed_tools(&["Task"]);
        let response = client
            .session_resume_full("ses_prior", "/tmp", vec![])
            .await
            .expect("session/resume");
        let sent = &response.raw["_receivedRequest"]["params"];
        assert_eq!(
            sent.pointer("/_meta/claudeCode/options/disallowedTools"),
            Some(&serde_json::json!(["Task"])),
            "resume forwards _meta to the same session builder: {sent}"
        );
    }

    /// Default off, and off means the key is absent rather than an empty list:
    /// a session that denied nothing must be byte-identical to one from before
    /// the option existed.
    #[tokio::test]
    async fn a_session_that_denied_nothing_sends_no_disallowed_tools_key() {
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client.initialize().await.expect("initialize");
        let response = client
            .session_new_full("/tmp", vec![], None, None)
            .await
            .expect("session/new");
        let sent = &response.raw["_receivedRequest"]["params"];
        assert_eq!(
            sent.pointer("/_meta/claudeCode"),
            None,
            "denying nothing must leave the key absent: {sent}"
        );
    }

    #[tokio::test]
    async fn session_new_full_includes_system_prompt_when_some() {
        // Script: respond to initialize, then echo back the session/new request.
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client
            .initialize()
            .await
            .expect("initialize should succeed");

        let resp = client
            .session_new_full(
                "/tmp",
                vec![],
                Some(SystemPromptTransport::Field("Custom system prompt")),
                None,
            )
            .await
            .expect("session_new_full should succeed");

        assert_eq!(resp.session_id, "ses_test");
        let received = &resp.raw["_receivedRequest"];
        assert_eq!(
            received["params"]["systemPrompt"].as_str(),
            Some("Custom system prompt"),
            "systemPrompt should be included in params when Some"
        );
    }

    #[tokio::test]
    async fn goose_system_prompt_request_uses_set_contract() {
        let script = r#"
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":0,"result":{"_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        let result = client
            .session_set_goose_system_prompt("ses_goose", "Be terse")
            .await
            .expect("custom request succeeds");
        let received = &result["_receivedRequest"];
        assert_eq!(
            received["method"],
            "_goose/unstable/session/system-prompt/set"
        );
        assert_eq!(received["params"]["sessionId"], "ses_goose");
        assert_eq!(received["params"]["mode"], "set");
        assert_eq!(received["params"]["key"], "buzz");
        assert_eq!(received["params"]["text"], "Be terse");
    }

    #[tokio::test]
    async fn goose_system_prompt_preserves_method_not_found_for_fallback() {
        let script = r#"
            read -t 2 _REQ
            echo '{"jsonrpc":"2.0","id":0,"error":{"code":-32601,"message":"Method not found"}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        assert!(matches!(
            client
                .session_set_goose_system_prompt("ses_goose", "Be terse")
                .await,
            Err(AcpError::AgentError { code: -32601, .. })
        ));
    }

    #[tokio::test]
    async fn goose_system_prompt_preserves_invalid_params_as_error() {
        let script = r#"
            read -t 2 _REQ
            echo '{"jsonrpc":"2.0","id":0,"error":{"code":-32602,"message":"Invalid params"}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        assert!(matches!(
            client
                .session_set_goose_system_prompt("ses_goose", "Be terse")
                .await,
            Err(AcpError::AgentError { code: -32602, .. })
        ));
    }

    #[tokio::test]
    async fn session_new_full_omits_system_prompt_when_none() {
        // When system_prompt is None, the field should not appear in params.
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client
            .initialize()
            .await
            .expect("initialize should succeed");

        let resp = client
            .session_new_full("/tmp", vec![], None, None)
            .await
            .expect("session_new_full should succeed");

        assert_eq!(resp.session_id, "ses_test");
        let received = &resp.raw["_receivedRequest"];
        assert!(
            received["params"]["systemPrompt"].is_null(),
            "systemPrompt should NOT be in params when value is None"
        );
    }

    #[tokio::test]
    async fn session_new_full_sends_session_title_in_meta_when_some() {
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client
            .initialize()
            .await
            .expect("initialize should succeed");

        let resp = client
            .session_new_full("/tmp", vec![], None, Some("Fizz · #buzz-dev"))
            .await
            .expect("session_new_full should succeed");

        let received = &resp.raw["_receivedRequest"];
        assert_eq!(
            received["params"]["_meta"]["sessionTitle"].as_str(),
            Some("Fizz · #buzz-dev"),
            "title should ride in _meta.sessionTitle, out of band from the prompt"
        );
    }

    #[tokio::test]
    async fn session_new_full_omits_meta_when_session_title_none() {
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client
            .initialize()
            .await
            .expect("initialize should succeed");

        let resp = client
            .session_new_full("/tmp", vec![], None, None)
            .await
            .expect("session_new_full should succeed");

        let received = &resp.raw["_receivedRequest"];
        assert!(
            received["params"].get("_meta").is_none(),
            "_meta should be absent entirely, not an empty object or null"
        );
    }

    // ── claude-agent-acp _meta.systemPrompt transport ─────────────────────

    #[tokio::test]
    async fn session_new_full_sends_claude_meta_system_prompt_when_claude_meta_transport() {
        // When ClaudeMeta transport is requested, the prompt must appear as
        // _meta.systemPrompt: {"append": text} — never as a bare systemPrompt field.
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_claude","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client
            .initialize()
            .await
            .expect("initialize should succeed");

        let resp = client
            .session_new_full(
                "/tmp",
                vec![],
                Some(SystemPromptTransport::ClaudeMeta("Be concise")),
                None,
            )
            .await
            .expect("session_new_full should succeed");

        let received = &resp.raw["_receivedRequest"];
        assert!(
            received["params"].get("systemPrompt").is_none(),
            "bare systemPrompt must not be present for ClaudeMeta transport"
        );
        assert_eq!(
            received["params"]["_meta"]["systemPrompt"]["append"].as_str(),
            Some("Be concise"),
            "_meta.systemPrompt.append must carry the prompt text"
        );
    }

    #[tokio::test]
    async fn session_new_full_merges_claude_meta_and_session_title_into_single_meta_object() {
        // Both ClaudeMeta prompt and session_title must coexist under _meta —
        // the prompt must not clobber sessionTitle or vice versa.
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_merged","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let mut client = spawn_script(script).await;
        client
            .initialize()
            .await
            .expect("initialize should succeed");

        let resp = client
            .session_new_full(
                "/tmp",
                vec![],
                Some(SystemPromptTransport::ClaudeMeta("Be concise")),
                Some("Fizz · #buzz-dev"),
            )
            .await
            .expect("session_new_full should succeed");

        let received = &resp.raw["_receivedRequest"];
        assert_eq!(
            received["params"]["_meta"]["systemPrompt"]["append"].as_str(),
            Some("Be concise"),
            "_meta.systemPrompt.append must be present"
        );
        assert_eq!(
            received["params"]["_meta"]["sessionTitle"].as_str(),
            Some("Fizz · #buzz-dev"),
            "_meta.sessionTitle must be present alongside systemPrompt"
        );
    }

    // ── Goose-native steer scaffold (PR follow-up to #1160) ──────────────

    /// Helper: spawn an inert `cat` subprocess so we have a real AcpClient
    /// to drive `handle_session_update` against. `cat` never writes back,
    /// which is fine — these tests don't read from the agent, they just
    /// feed JSON into the parser.
    async fn spawn_inert_client() -> AcpClient {
        AcpClient::spawn("cat", &[], &[], false)
            .await
            .expect("spawn cat as inert client")
    }

    /// Build a `session/update` JSON-RPC notification carrying a
    /// `session_info_update` with the given `_meta.goose.activeRunId` value.
    /// Pass `None` to omit the `activeRunId` field entirely.
    ///
    /// `_meta` is nested inside the `update` object (per the ACP
    /// `SessionInfoUpdate` schema), matching what goose and buzz-agent
    /// emit on the wire.
    fn session_info_update_msg(active_run_id: Option<serde_json::Value>) -> serde_json::Value {
        let mut goose = serde_json::Map::new();
        if let Some(v) = active_run_id {
            goose.insert("activeRunId".to_string(), v);
        }
        let mut meta = serde_json::Map::new();
        meta.insert("goose".to_string(), serde_json::Value::Object(goose));
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "test-session",
                "update": {
                    "sessionUpdate": "session_info_update",
                    "_meta": serde_json::Value::Object(meta),
                },
            }
        })
    }

    #[tokio::test]
    async fn active_run_id_sets_on_string() {
        let mut client = spawn_inert_client().await;
        assert!(client.active_run_id().is_none(), "starts as None");

        let msg = session_info_update_msg(Some(serde_json::json!("run-abc-123")));
        let _ = client.handle_session_update(&msg);

        assert_eq!(client.active_run_id(), Some("run-abc-123"));
    }

    #[tokio::test]
    async fn active_run_id_clears_on_null() {
        let mut client = spawn_inert_client().await;
        // Set it first
        let set_msg = session_info_update_msg(Some(serde_json::json!("run-xyz")));
        let _ = client.handle_session_update(&set_msg);
        assert_eq!(client.active_run_id(), Some("run-xyz"));

        // Then clear with explicit null
        let clear_msg = session_info_update_msg(Some(serde_json::Value::Null));
        let _ = client.handle_session_update(&clear_msg);
        assert!(
            client.active_run_id().is_none(),
            "explicit null must clear active_run_id"
        );
    }

    #[tokio::test]
    async fn active_run_id_untouched_when_missing() {
        // Field absent entirely — must NOT clear existing state (only an
        // explicit null clears; missing means "no new info this update").
        let mut client = spawn_inert_client().await;
        let set_msg = session_info_update_msg(Some(serde_json::json!("run-stable")));
        let _ = client.handle_session_update(&set_msg);
        assert_eq!(client.active_run_id(), Some("run-stable"));

        // session_info_update with no activeRunId field — leave state alone.
        let missing_msg = session_info_update_msg(None);
        let _ = client.handle_session_update(&missing_msg);
        assert_eq!(
            client.active_run_id(),
            Some("run-stable"),
            "missing activeRunId must leave state untouched"
        );
    }

    #[tokio::test]
    async fn active_run_id_untouched_on_wrong_type() {
        // A number or object in activeRunId is malformed — neither set nor clear.
        let mut client = spawn_inert_client().await;
        let set_msg = session_info_update_msg(Some(serde_json::json!("run-stable")));
        let _ = client.handle_session_update(&set_msg);
        assert_eq!(client.active_run_id(), Some("run-stable"));

        let wrong_type_msg = session_info_update_msg(Some(serde_json::json!(42)));
        let _ = client.handle_session_update(&wrong_type_msg);
        assert_eq!(
            client.active_run_id(),
            Some("run-stable"),
            "non-string/non-null activeRunId must leave state untouched"
        );
    }

    // ── Goose-native steer arm tests ──────────────────────────────────────
    //
    // These exercise the seam between `install_steer_rx` and the read
    // loop's steer arm, isolated from `AgentPool` / `EventQueue` /
    // dispatch. They prove the locked Option-X contract at the read-loop
    // boundary:
    //   1. With `active_run_id == None`, the steer arm acks
    //      `Err(ExpectedRunIdMissing)` and writes nothing — the main
    //      loop's "Err-before-pending" fallback path is reachable.
    //   2. With `active_run_id` set, the steer arm writes the JSON-RPC
    //      request with the matching `expectedRunId` and routes the
    //      response to the ack oneshot as `Success`.
    //
    // We don't test the full mode-gate fork here — that lives in lib.rs
    // and is covered by goose e2e (Eva's lane).

    /// Steer with no `active_run_id` set acks `ExpectedRunIdMissing`
    /// without writing anything. The read loop continues normally and
    /// eventually hits the idle timeout (which is fine — we just need to
    /// observe the ack).
    #[tokio::test]
    async fn native_steer_with_no_active_run_id_acks_expected_run_id_missing() {
        // Quiet process: never emits anything, so the read loop has only
        // the steer arm and the idle timeout to consider.
        let mut client = spawn_script("sleep 10").await;
        assert!(
            client.active_run_id().is_none(),
            "precondition: active_run_id starts as None"
        );

        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel::<crate::pool::SteerRequest>(1);
        client.install_steer_rx(steer_rx);

        // Fire-and-forget: send a SteerRequest from a separate task so
        // the read loop picks it up via the select! arm.
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<crate::pool::SteerAck>();
        let send_task = tokio::spawn(async move {
            steer_tx
                .send(crate::pool::SteerRequest {
                    prompt_blocks: vec!["test steer body".into()],
                    ack_tx,
                })
                .await
                .expect("steer_tx send should succeed");
        });

        // Drive the read loop with short idle timeout so the test
        // doesn't hang. The expected_id is intentionally never going to
        // be matched (the script writes nothing); the read loop will
        // exit via IdleTimeout shortly after the steer arm fires.
        let idle = std::time::Duration::from_millis(500);
        let max_dur = std::time::Duration::from_secs(5);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let read_result = client
            .read_until_response_with_idle_timeout("sess-test", 999, idle, hard_deadline, max_dur)
            .await;
        send_task.await.expect("send_task should complete");

        // Read loop exit shape: IdleTimeout (no agent activity).
        assert!(
            matches!(read_result, Err(AcpError::IdleTimeout { .. })),
            "expected IdleTimeout once steer was acked + script stayed silent, got {read_result:?}"
        );

        // Ack must be ExpectedRunIdMissing — the steer arm bailed out
        // without writing because active_run_id was None at write time.
        let ack = ack_rx
            .await
            .expect("ack oneshot must have received a SteerAck");
        match ack {
            crate::pool::SteerAck::Err(crate::pool::SteerError::ExpectedRunIdMissing) => {}
            other => panic!("expected SteerAck::Err(ExpectedRunIdMissing), got {other:?}"),
        }
    }

    /// Steer with `active_run_id` set writes the JSON-RPC request and
    /// routes the matching response to the ack oneshot as `Success`.
    /// Verifies the wire shape (`sessionId` + `expectedRunId` + `prompt`)
    /// indirectly: the bash script emits a response keyed by the steer
    /// id (0), and `Success` only fires if the read loop matched that
    /// id to its `pending_steer` entry.
    #[tokio::test]
    async fn native_steer_with_active_run_id_routes_response_to_ack() {
        // Script: pause briefly so the test task can install the steer
        // and we can be sure the response doesn't race ahead of the
        // write — then emit the steer response (id=0 because next_id
        // starts at 0 and the steer is the first request the read loop
        // writes), then idle. This is a JSON-RPC success response with
        // a `stopReason` payload (matching the shape goose uses for
        // steer responses in fake_llm.rs).
        let script = "sleep 0.5; \
                      echo '{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{\"stopReason\":\"end_turn\"}}'; \
                      sleep 10";
        let mut client = spawn_script(script).await;

        // Set active_run_id via a synthesized session_info_update so the
        // steer arm has a non-None value to read at write time.
        let update = session_info_update_msg(Some(serde_json::json!("run-42")));
        let _ = client.handle_session_update(&update);
        assert_eq!(client.active_run_id(), Some("run-42"));

        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel::<crate::pool::SteerRequest>(1);
        client.install_steer_rx(steer_rx);

        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<crate::pool::SteerAck>();
        let send_task = tokio::spawn(async move {
            steer_tx
                .send(crate::pool::SteerRequest {
                    prompt_blocks: vec!["test steer body".into()],
                    ack_tx,
                })
                .await
                .expect("steer_tx send should succeed");
        });

        // Drive the read loop. Expected_id 999 will never be emitted by
        // the script so the read loop exits via idle timeout after the
        // steer response is routed to ack.
        let idle = std::time::Duration::from_secs(2);
        let max_dur = std::time::Duration::from_secs(10);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let read_result = client
            .read_until_response_with_idle_timeout("sess-test", 999, idle, hard_deadline, max_dur)
            .await;
        send_task.await.expect("send_task should complete");

        // Read loop exit: IdleTimeout (no further activity after the
        // routed steer response). AgentExited would also be a valid
        // exit if the bash script terminated early; either is fine —
        // what matters is the ack.
        assert!(
            matches!(
                read_result,
                Err(AcpError::IdleTimeout { .. }) | Err(AcpError::AgentExited)
            ),
            "expected IdleTimeout or AgentExited after steer ack, got {read_result:?}"
        );

        // Ack must be Success: the steer response (id=0) was routed to
        // pending_steer.ack_tx.
        let ack = ack_rx
            .await
            .expect("ack oneshot must have received a SteerAck");
        match ack {
            crate::pool::SteerAck::Success { .. } => {}
            other => panic!("expected SteerAck::Success, got {other:?}"),
        }
    }

    /// Steer-success renewal keeps the turn alive past the original hard
    /// deadline. This is the red-on-old/green-on-new test for the core bug
    /// fix (acp.rs:1440-1444): without renewal, the read loop returns
    /// `HardTimeout` before the prompt response arrives.
    ///
    /// Timeline:
    ///   t≈0:    read loop starts, `hard_deadline = now + 1s`
    ///   t≈0.5s: script emits steer response (id=0) → Success renewal
    ///           moves `hard_deadline` to `now + 3s` (≈3.5s from start)
    ///   t≈1.5s: script emits prompt response (id=999) → `Ok`
    ///
    /// Old code: `HardTimeout` at t≈1s (before prompt response).
    /// New code: deadline renewed at t≈0.5s → prompt response at t≈1.5s → `Ok`.
    #[tokio::test]
    async fn steer_success_renews_hard_deadline_and_survives_past_original() {
        let script = "sleep 0.5; \
                      echo '{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{\"stopReason\":\"end_turn\"}}'; \
                      sleep 1; \
                      echo '{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{\"done\":true}}'";
        let mut client = spawn_script(script).await;

        let update = session_info_update_msg(Some(serde_json::json!("run-99")));
        let _ = client.handle_session_update(&update);

        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel::<crate::pool::SteerRequest>(1);
        client.install_steer_rx(steer_rx);

        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<crate::pool::SteerAck>();
        let send_task = tokio::spawn(async move {
            steer_tx
                .send(crate::pool::SteerRequest {
                    prompt_blocks: vec!["steer body".into()],
                    ack_tx,
                })
                .await
                .expect("steer_tx send should succeed");
        });

        let idle = std::time::Duration::from_secs(10);
        let max_dur = std::time::Duration::from_secs(3);
        let hard_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let result = client
            .read_until_response_with_idle_timeout("sess-test", 999, idle, hard_deadline, max_dur)
            .await;
        send_task.await.expect("send_task should complete");

        assert!(
            result.is_ok(),
            "expected Ok (prompt response after renewed deadline), got {result:?}"
        );
        assert_eq!(result.unwrap()["done"], serde_json::json!(true));

        let ack = ack_rx
            .await
            .expect("ack oneshot must have received a SteerAck");
        match ack {
            crate::pool::SteerAck::Success { .. } => {}
            other => panic!("expected SteerAck::Success, got {other:?}"),
        }
    }

    // ── Cross-harness steer transport tests ───────────────────────────────
    //
    // These cover the `_session/steering` transport added alongside the
    // goose-native method: capability capture at `initialize`, write-time
    // transport selection, and outcome decoding. Wire-shape assertions read
    // the actual serialized request bytes via `capture_steer_request` rather
    // than inferring the shape from response-id routing.

    /// Spawn a client whose script captures the first line written to its
    /// stdin into `capture_path`, then emits `response` (already-serialized
    /// JSON-RPC) and idles.
    ///
    /// The steer request is the first thing this read loop writes, so the
    /// captured line IS the steer request bytes.
    async fn spawn_steer_capture_script(
        capture_path: &std::path::Path,
        response: &str,
    ) -> AcpClient {
        let script = format!(
            "read -r line; printf '%s' \"$line\" > {capture}; \
             printf '%s\\n' '{response}'; sleep 10",
            capture = capture_path.display(),
            response = response,
        );
        spawn_script(&script).await
    }

    /// Drive one steer through the read loop and return
    /// `(captured_request_bytes, ack)`.
    ///
    /// `capture_path` may be absent afterwards when the arm wrote nothing —
    /// callers assert on that. The read loop is expected to exit via a
    /// timeout or EOF; the ack is what these tests care about.
    async fn run_one_steer(
        client: &mut AcpClient,
        capture_path: &std::path::Path,
    ) -> (Option<String>, crate::pool::SteerAck) {
        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel::<crate::pool::SteerRequest>(1);
        client.install_steer_rx(steer_rx);

        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<crate::pool::SteerAck>();
        let send_task = tokio::spawn(async move {
            steer_tx
                .send(crate::pool::SteerRequest {
                    prompt_blocks: vec!["steer body".into()],
                    ack_tx,
                })
                .await
                .expect("steer_tx send should succeed");
        });

        let idle = std::time::Duration::from_millis(800);
        let max_dur = std::time::Duration::from_secs(10);
        let hard_deadline = tokio::time::Instant::now() + max_dur;
        let _ = client
            .read_until_response_with_idle_timeout("sess-test", 999, idle, hard_deadline, max_dur)
            .await;
        send_task.await.expect("send_task should complete");

        let ack = ack_rx
            .await
            .expect("ack oneshot must have received a SteerAck");
        (std::fs::read_to_string(capture_path).ok(), ack)
    }

    /// Unique temp path for one test's captured request bytes.
    fn capture_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("buzz-acp-steer-capture");
        std::fs::create_dir_all(&dir).expect("create capture dir");
        let path = dir.join(format!("{name}.json"));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// Mark a client as having advertised `_meta.steering.supported` without
    /// running a real `initialize` handshake. The capability-parsing tests
    /// cover the handshake itself.
    fn set_steering_supported(client: &mut AcpClient) {
        client.steering_supported = true;
    }

    /// Run `initialize` against a script that replies with `init_result` as
    /// the JSON-RPC result, and return the resulting `steering_supported`.
    async fn steering_supported_after_initialize(init_result: &str) -> bool {
        let script = format!(
            "read -r _init; printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{result}}}'; \
             sleep 5",
            result = init_result,
        );
        let mut client = spawn_script(&script).await;
        client
            .initialize()
            .await
            .expect("initialize should succeed");
        client.steering_supported()
    }

    /// Test 1a: an adapter advertising `_meta.steering.supported: true`
    /// (claude-agent-acp `src/acp-agent.ts:1444`, codex-acp
    /// `src/CodexAcpServer.ts:247`) is recorded as steering-capable.
    #[tokio::test]
    async fn initialize_records_steering_supported_when_advertised() {
        let supported = steering_supported_after_initialize(
            r#"{"protocolVersion":2,"agentCapabilities":{},"_meta":{"steering":{"supported":true}}}"#,
        )
        .await;
        assert!(
            supported,
            "_meta.steering.supported: true must set steering_supported"
        );
    }

    /// Test 1b: no `_meta` at all (goose, buzz-agent, any older adapter) must
    /// leave the capability off — this is what keeps a steer off the wire for
    /// agents that never implemented it.
    #[tokio::test]
    async fn initialize_leaves_steering_unsupported_when_meta_absent() {
        let supported =
            steering_supported_after_initialize(r#"{"protocolVersion":2,"agentCapabilities":{}}"#)
                .await;
        assert!(
            !supported,
            "absent _meta must leave steering_supported false"
        );
    }

    /// Test 1c: an explicit `supported: false` is respected, not treated as
    /// "the key exists so it must work".
    #[tokio::test]
    async fn initialize_leaves_steering_unsupported_when_explicitly_false() {
        let supported = steering_supported_after_initialize(
            r#"{"protocolVersion":2,"_meta":{"steering":{"supported":false}}}"#,
        )
        .await;
        assert!(
            !supported,
            "_meta.steering.supported: false must leave steering_supported false"
        );
    }

    #[tokio::test]
    async fn initialize_records_load_and_resume_capabilities() {
        let script = r#"
read -r _init
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":2,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}}'
sleep 5
"#;
        let mut client = spawn_script(script).await;
        client.initialize().await.expect("initialize");
        assert!(client.session_load_supported());
        assert!(client.session_resume_supported());

        let script = r#"
read -r _init
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":2,"agentCapabilities":{"loadSession":false,"sessionCapabilities":{"resume":null}}}}'
sleep 5
"#;
        let mut client = spawn_script(script).await;
        client.initialize().await.expect("initialize");
        assert!(!client.session_load_supported());
        assert!(!client.session_resume_supported());
    }

    /// Test 2: no `active_run_id` + capability advertised → the bytes on the
    /// wire are an `_session/steering` request carrying `sessionId` and
    /// `prompt`, and carrying **no** `expectedRunId` (the adapters reject
    /// unknown required fields, and there is no run id to report anyway).
    #[tokio::test]
    async fn acp_steer_request_omits_expected_run_id_and_carries_session_and_prompt() {
        let capture = capture_path("acp_shape");
        let mut client = spawn_steer_capture_script(
            &capture,
            r#"{"jsonrpc":"2.0","id":0,"result":{"outcome":"injected"}}"#,
        )
        .await;
        set_steering_supported(&mut client);
        assert!(
            client.active_run_id().is_none(),
            "precondition: no active_run_id"
        );

        let (written, ack) = run_one_steer(&mut client, &capture).await;

        let written = written.expect("steer request must have been written");
        let msg: serde_json::Value =
            serde_json::from_str(&written).expect("written line must be valid JSON");
        assert_eq!(
            msg["method"].as_str(),
            Some(ACP_STEER_METHOD),
            "must use the cross-adapter steer method; wrote: {written}"
        );
        assert_eq!(msg["params"]["sessionId"].as_str(), Some("sess-test"));
        assert_eq!(
            msg["params"]["prompt"][0]["text"].as_str(),
            Some("steer body"),
            "prompt must carry the steer body as a text block"
        );
        assert!(
            msg["params"].get("expectedRunId").is_none(),
            "_session/steering must not carry expectedRunId; wrote: {written}"
        );
        assert!(
            matches!(ack, crate::pool::SteerAck::Success { .. }),
            "injected outcome must ack Success, got {ack:?}"
        );
    }

    /// Test 3: goose keeps priority. With both an `active_run_id` and the
    /// advertised capability, the goose method wins — `expectedRunId` is
    /// strictly more precise about which run is being steered.
    #[tokio::test]
    async fn goose_transport_wins_when_both_run_id_and_capability_present() {
        let capture = capture_path("goose_priority");
        let mut client =
            spawn_steer_capture_script(&capture, r#"{"jsonrpc":"2.0","id":0,"result":{}}"#).await;
        set_steering_supported(&mut client);
        let update = session_info_update_msg(Some(serde_json::json!("run-77")));
        let _ = client.handle_session_update(&update);

        let (written, ack) = run_one_steer(&mut client, &capture).await;

        let written = written.expect("steer request must have been written");
        let msg: serde_json::Value =
            serde_json::from_str(&written).expect("written line must be valid JSON");
        assert_eq!(
            msg["method"].as_str(),
            Some(GOOSE_STEER_METHOD),
            "goose method must win when a run id exists; wrote: {written}"
        );
        assert_eq!(msg["params"]["expectedRunId"].as_str(), Some("run-77"));
        // A bare `{}` result is a success on the goose transport (goose sends
        // no `outcome`) — the OutcomeRejected guard applies only to
        // `_session/steering`.
        assert!(
            matches!(ack, crate::pool::SteerAck::Success { .. }),
            "goose success result must ack Success, got {ack:?}"
        );
    }

    /// Test 7: codex-acp's third outcome, `failed`
    /// (`src/AcpExtensions.ts:92`), is a delivery rejection despite being a
    /// JSON-RPC success — release the event and fall back.
    #[tokio::test]
    async fn acp_steer_failed_outcome_acks_outcome_rejected() {
        let capture = capture_path("outcome_failed");
        let mut client = spawn_steer_capture_script(
            &capture,
            r#"{"jsonrpc":"2.0","id":0,"result":{"outcome":"failed"}}"#,
        )
        .await;
        set_steering_supported(&mut client);

        let (_written, ack) = run_one_steer(&mut client, &capture).await;

        match ack {
            crate::pool::SteerAck::Err(crate::pool::SteerError::OutcomeRejected { outcome }) => {
                assert_eq!(
                    outcome, "failed",
                    "rejected outcome must report what the agent said, unquoted"
                );
            }
            other => panic!("expected Err(OutcomeRejected), got {other:?}"),
        }
    }

    /// Test 8: **codex `extMethod` silent-loss regression guard.** codex-acp's
    /// ext dispatcher answers unrecognized methods with a bare `{}` — a
    /// JSON-RPC *success*, not `-32601` (`src/CodexAcpServer.ts:255-258`).
    /// Buzz maps `SteerAck::Success` to `queue.remove_event`, so decoding
    /// `{}` as success would delete the user's message with no error, no
    /// fallback, and no log. An absent `outcome` must therefore be a
    /// rejection, which releases the event and fires cancel+merge.
    #[tokio::test]
    async fn acp_steer_missing_outcome_acks_outcome_rejected_and_never_drops_event() {
        let capture = capture_path("outcome_absent");
        let mut client =
            spawn_steer_capture_script(&capture, r#"{"jsonrpc":"2.0","id":0,"result":{}}"#).await;
        set_steering_supported(&mut client);

        let (_written, ack) = run_one_steer(&mut client, &capture).await;

        match ack {
            crate::pool::SteerAck::Err(crate::pool::SteerError::OutcomeRejected { outcome }) => {
                assert_eq!(
                    outcome, "<absent>",
                    "a result with no outcome field must be reported as absent"
                );
            }
            other => panic!(
                "expected Err(OutcomeRejected) for a bare {{}} success — \
                 anything else risks dropping the event, got {other:?}"
            ),
        }
    }

    /// Test 5: `injected` renews the hard deadline, so the turn survives past
    /// its original one. Mirrors
    /// `steer_success_renews_hard_deadline_and_survives_past_original` for
    /// the `_session/steering` transport.
    ///
    /// Timeline: original hard deadline at t≈1s; steer response at t≈0.5s
    /// renews it to t≈3.5s; prompt response at t≈1.5s lands inside it.
    #[tokio::test]
    async fn acp_steer_injected_renews_hard_deadline_and_survives_past_original() {
        let script = "sleep 0.5; \
                      echo '{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{\"outcome\":\"injected\"}}'; \
                      sleep 1; \
                      echo '{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{\"done\":true}}'";
        let mut client = spawn_script(script).await;
        set_steering_supported(&mut client);

        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel::<crate::pool::SteerRequest>(1);
        client.install_steer_rx(steer_rx);
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<crate::pool::SteerAck>();
        let send_task = tokio::spawn(async move {
            steer_tx
                .send(crate::pool::SteerRequest {
                    prompt_blocks: vec!["steer body".into()],
                    ack_tx,
                })
                .await
                .expect("steer_tx send should succeed");
        });

        let idle = std::time::Duration::from_secs(10);
        let max_dur = std::time::Duration::from_secs(3);
        let hard_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let result = client
            .read_until_response_with_idle_timeout("sess-test", 999, idle, hard_deadline, max_dur)
            .await;
        send_task.await.expect("send_task should complete");

        assert!(
            result.is_ok(),
            "injected must renew the deadline so the prompt response still lands, got {result:?}"
        );
        assert_eq!(result.unwrap()["done"], serde_json::json!(true));
        let ack = ack_rx.await.expect("ack must be received");
        assert!(
            matches!(ack, crate::pool::SteerAck::Success { .. }),
            "injected must ack Success, got {ack:?}"
        );
    }

    /// Test 6: **red/green for the no-renewal rule.** `startedNewTurn` means
    /// the turn Buzz was steering had already ended and the adapter began a
    /// fresh, detached one. It acks `Success` (the message WAS delivered, so
    /// the event must not be redelivered) but must NOT renew the hard
    /// deadline — that clock belongs to a turn which is already settled.
    ///
    /// Same timeline as the `injected` test, so the only difference is the
    /// outcome string: original hard deadline at t≈1s, steer response at
    /// t≈0.5s, prompt response at t≈1.5s. With renewal the prompt response
    /// would land and this returns `Ok`; without renewal the original
    /// deadline fires first and we get `HardTimeout`.
    #[tokio::test]
    async fn acp_steer_started_new_turn_acks_success_without_renewing_hard_deadline() {
        let script = "sleep 0.5; \
             echo '{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{\"outcome\":\"startedNewTurn\"}}'; \
             sleep 1; \
             echo '{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{\"done\":true}}'";
        let mut client = spawn_script(script).await;
        set_steering_supported(&mut client);

        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel::<crate::pool::SteerRequest>(1);
        client.install_steer_rx(steer_rx);
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<crate::pool::SteerAck>();
        let send_task = tokio::spawn(async move {
            steer_tx
                .send(crate::pool::SteerRequest {
                    prompt_blocks: vec!["steer body".into()],
                    ack_tx,
                })
                .await
                .expect("steer_tx send should succeed");
        });

        let idle = std::time::Duration::from_secs(10);
        let max_dur = std::time::Duration::from_secs(3);
        let hard_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let result = client
            .read_until_response_with_idle_timeout("sess-test", 999, idle, hard_deadline, max_dur)
            .await;
        send_task.await.expect("send_task should complete");

        // The original deadline must still fire — renewal here would extend
        // the clock on a turn the adapter has already finished.
        assert!(
            matches!(result, Err(AcpError::HardTimeout { .. })),
            "startedNewTurn must NOT renew the hard deadline, so the original \
             one must still fire; got {result:?}"
        );
        // Delivery still succeeded, so the withheld event must be dropped
        // rather than released — hence Success, not an Err.
        let ack = ack_rx.await.expect("ack must be received");
        assert!(
            matches!(ack, crate::pool::SteerAck::Success { .. }),
            "startedNewTurn is a delivery success, got {ack:?}"
        );
    }

    /// Test 4 (companion to the existing
    /// `native_steer_with_no_active_run_id_acks_expected_run_id_missing`):
    /// no run id AND no advertised capability means nothing is written at
    /// all. This is the gate that keeps a steer off the wire for adapters
    /// that never implemented either method.
    #[tokio::test]
    async fn steer_writes_nothing_when_no_run_id_and_capability_absent() {
        let capture = capture_path("no_transport");
        let mut client =
            spawn_steer_capture_script(&capture, r#"{"jsonrpc":"2.0","id":0,"result":{}}"#).await;
        assert!(!client.steering_supported(), "precondition: not advertised");
        assert!(
            client.active_run_id().is_none(),
            "precondition: no active_run_id"
        );

        let (written, ack) = run_one_steer(&mut client, &capture).await;

        assert!(
            written.is_none(),
            "no transport available must write nothing; wrote: {written:?}"
        );
        match ack {
            crate::pool::SteerAck::Err(crate::pool::SteerError::ExpectedRunIdMissing) => {}
            other => panic!("expected Err(ExpectedRunIdMissing), got {other:?}"),
        }
    }

    // ── Standard ACP prompt-response usage ─────────────────────────────────

    fn prompt_response_usage(
        input: u64,
        output: u64,
        total: u64,
        cached_read: Option<u64>,
        cached_write: Option<u64>,
    ) -> serde_json::Value {
        let mut usage = serde_json::json!({
            "inputTokens": input,
            "outputTokens": output,
            "totalTokens": total,
        });
        if let Some(cached_read) = cached_read {
            usage["cachedReadTokens"] = serde_json::json!(cached_read);
        }
        if let Some(cached_write) = cached_write {
            usage["cachedWriteTokens"] = serde_json::json!(cached_write);
        }
        serde_json::json!({"stopReason": "end_turn", "usage": usage})
    }

    fn standard_cost_update(session_id: &str, cost: f64) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "usage_update",
                    "cost": {"amount": cost, "currency": "USD"}
                }
            }
        })
    }

    fn sdk_result_frame(
        session_id: &str,
        origin: Option<&str>,
        usage: serde_json::Value,
    ) -> serde_json::Value {
        let mut message = serde_json::json!({ "type": "result", "modelUsage": usage });
        if let Some(kind) = origin {
            message["origin"] = serde_json::json!({ "kind": kind });
        }
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": RAW_SDK_FRAME_METHOD,
            "params": { "sessionId": session_id, "message": message }
        })
    }

    /// Ledger 272(d), RED-first: claude-agent-acp 0.70.0 stamps every ACP
    /// prompt `origin: {kind: "human"}` (`acp-agent.js:6180`), and its result
    /// carries that origin. Only the adapter's own autonomous set
    /// (`task-notification`, `peer`, `coordinator`, `observer`,
    /// `observer-activity`, `acp-agent.js:114`) is someone else's cycle; a
    /// `human` result answered the user's turn and names its model.
    #[tokio::test]
    async fn a_human_origin_result_names_the_model_that_answered_the_turn() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Claude);
        client.notify_session_spawned("human-session");
        client.standard_usage.begin_turn("human-session");
        client.handle_raw_sdk_frame(&sdk_result_frame(
            "human-session",
            Some("human"),
            serde_json::json!({ "claude-opus-5-5": { "costUSD": 0.2, "outputTokens": 400 } }),
        ));
        client.handle_session_update(&standard_cost_update("human-session", 0.2));
        let usage = client.take_turn_usage().expect("usage");
        assert_eq!(usage.model.as_deref(), Some("claude-opus-5-5"));
    }

    /// The adapter's autonomous origins still never name the turn's model,
    /// and an origin the adapter does not list falls to the user lane, as the
    /// adapter itself routes it (fail-open, `acp-agent.js:105-113`).
    #[tokio::test]
    async fn only_the_adapters_autonomous_origins_are_someone_elses_cycle() {
        for (origin, expected) in [
            ("task-notification", None),
            ("peer", None),
            ("coordinator", None),
            ("observer", None),
            ("observer-activity", None),
            ("channel", Some("claude-sonnet-5")),
            ("unclassified", Some("claude-sonnet-5")),
        ] {
            let mut client = spawn_inert_client().await;
            client.standard_adapter = Some(StandardAdapterKind::Claude);
            client.notify_session_spawned("origin-session");
            client.standard_usage.begin_turn("origin-session");
            client.handle_raw_sdk_frame(&sdk_result_frame(
                "origin-session",
                Some(origin),
                serde_json::json!({ "claude-sonnet-5": { "costUSD": 0.1, "outputTokens": 10 } }),
            ));
            client.handle_session_update(&standard_cost_update("origin-session", 0.1));
            let usage = client.take_turn_usage().expect("usage");
            assert_eq!(usage.model.as_deref(), expected, "origin {origin}");
        }
    }

    /// Ledger 272(d), RED-first: `modelUsage` on a Claude SDK result is the
    /// session's cumulative map, so the model that answered *this* turn is
    /// the one whose usage grew since the previous result — not the one with
    /// the most usage over the whole session.
    #[tokio::test]
    async fn the_turns_model_is_the_one_whose_cumulative_usage_grew() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Claude);
        client.notify_session_spawned("delta-session");
        client.standard_usage.begin_turn("delta-session");
        client.handle_raw_sdk_frame(&sdk_result_frame(
            "delta-session",
            Some("human"),
            serde_json::json!({ "claude-opus-5-5": { "costUSD": 1.0, "outputTokens": 900 } }),
        ));
        client.handle_session_update(&standard_cost_update("delta-session", 1.0));
        assert_eq!(
            client.take_turn_usage().expect("usage").model.as_deref(),
            Some("claude-opus-5-5")
        );
        client.standard_usage.begin_turn("delta-session");
        client.handle_raw_sdk_frame(&sdk_result_frame(
            "delta-session",
            Some("human"),
            serde_json::json!({
                "claude-opus-5-5": { "costUSD": 1.0, "outputTokens": 900 },
                "claude-sonnet-5": { "costUSD": 0.1, "outputTokens": 50 }
            }),
        ));
        client.handle_session_update(&standard_cost_update("delta-session", 1.1));
        assert_eq!(
            client.take_turn_usage().expect("usage").model.as_deref(),
            Some("claude-sonnet-5"),
            "opus did nothing this turn; the cumulative map still ranks it first"
        );
    }

    /// Ledger 272(d), RED-first: every `session/new` seeds the zero cost
    /// baseline, whoever called it — the session provider opens sessions
    /// itself and never reaches `pool.rs`'s `notify_session_spawned`, so its
    /// first turn's cumulative cost had nothing to subtract from.
    #[cfg(unix)]
    #[tokio::test]
    async fn session_new_seeds_the_first_turns_cost_baseline() {
        let script = r#"
            read -r REQ
            ID=$(printf '%s' "$REQ" | sed -E 's/.*"id":([0-9]+).*/\1/')
            echo '{"jsonrpc":"2.0","id":'"$ID"',"result":{"sessionId":"fresh-session"}}'
            read -r REQ
            ID=$(printf '%s' "$REQ" | sed -E 's/.*"id":([0-9]+).*/\1/')
            echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fresh-session","update":{"sessionUpdate":"usage_update","cost":{"amount":0.25,"currency":"USD"}}}}'
            echo '{"jsonrpc":"2.0","id":'"$ID"',"result":{"stopReason":"end_turn","usage":{"inputTokens":7,"outputTokens":3,"totalTokens":10}}}'
            sleep 1
        "#;
        let (mut client, dir) = spawn_named_script("claude-code", script).await;
        client.set_turn_clock(idle_clock::ManualTurnClock::frozen());
        let opened = client
            .session_new_full("/tmp", vec![], None, None)
            .await
            .expect("session/new");
        assert_eq!(opened.session_id, "fresh-session");
        client
            .session_prompt_with_idle_timeout(
                "fresh-session",
                "hello",
                std::time::Duration::from_secs(2),
                std::time::Duration::from_secs(5),
            )
            .await
            .expect("prompt");
        let usage = client.take_turn_usage().expect("usage");
        assert_eq!(usage.turn_cost_usd, Some(0.25));
        drop(client);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn claude_prompt_response_usage_merges_with_cumulative_cost() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Claude);
        client.notify_session_spawned("claude-session");
        client.standard_usage.begin_turn("claude-session");
        client.handle_session_update(&standard_cost_update("claude-session", 0.042));
        assert_eq!(
            client
                .parse_prompt_response(
                    "claude-session",
                    &prompt_response_usage(100, 20, 175, Some(30), Some(25)),
                )
                .unwrap(),
            StopReason::EndTurn
        );

        let usage = client.take_turn_usage().expect("prompt usage");
        assert!(usage.delta_reliable, "response tokens need no baseline");
        assert_eq!(usage.turn_input_tokens, Some(155));
        assert_eq!(usage.turn_output_tokens, Some(20));
        assert_eq!(
            usage.turn_total_tokens, None,
            "Claude total is adapter-derived"
        );
        assert_eq!(usage.turn_cache_read_tokens, Some(30));
        assert_eq!(usage.turn_cache_write_tokens, Some(25));
        assert_eq!(usage.turn_cost_usd, Some(0.042));
        assert_eq!(usage.cumulative_cost_usd, Some(0.042));
        assert_eq!(usage.cumulative_input_tokens, None);
        assert_eq!(usage.cumulative_output_tokens, None);
    }

    #[tokio::test]
    async fn codex_prompt_response_usage_preserves_provider_total_without_cost() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Codex);
        client.standard_usage.begin_turn("codex-session");
        client.handle_session_update(&standard_cost_update("codex-session", 0.042));
        client
            .parse_prompt_response(
                "codex-session",
                &prompt_response_usage(90, 10, 140, Some(40), None),
            )
            .unwrap();

        let usage = client.take_turn_usage().expect("prompt usage");
        assert!(usage.delta_reliable);
        assert_eq!(usage.turn_input_tokens, Some(130));
        assert_eq!(usage.turn_output_tokens, Some(10));
        assert_eq!(usage.turn_total_tokens, Some(140));
        assert_eq!(usage.turn_cache_read_tokens, Some(40));
        assert_eq!(usage.turn_cache_write_tokens, None);
        assert_eq!(
            usage.cumulative_cost_usd, None,
            "Codex cost update is ignored"
        );
        assert_eq!(usage.cumulative_input_tokens, None);
        assert_eq!(usage.cumulative_output_tokens, None);
    }

    #[tokio::test]
    async fn standard_prompt_input_overflow_fails_closed() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Claude);
        client.standard_usage.begin_turn("overflow-session");
        client
            .parse_prompt_response(
                "overflow-session",
                &prompt_response_usage(u64::MAX, 10, u64::MAX, Some(1), None),
            )
            .unwrap();

        assert!(
            client.take_turn_usage().is_none(),
            "overflow without another valid signal must not emit all-null usage"
        );
    }

    /// Ledger 268(e): a Claude adapter's `result` frame names the model that
    /// actually answered; the turn's usage carries it, not the picker label.
    #[cfg(unix)]
    #[tokio::test]
    async fn claude_named_adapter_records_the_model_its_result_frame_reports() {
        let script = r#"
            read -r REQ
            ID=$(printf '%s' "$REQ" | sed -E 's/.*"id":([0-9]+).*/\1/')
            echo '{"jsonrpc":"2.0","method":"_claude/sdkMessage","params":{"sessionId":"model-session","message":{"type":"result","origin":{"kind":"task-notification"},"modelUsage":{"claude-haiku-4-5":{"costUSD":9.0}}}}}'
            echo '{"jsonrpc":"2.0","method":"_claude/sdkMessage","params":{"sessionId":"model-session","message":{"type":"result","modelUsage":{"claude-haiku-4-5":{"costUSD":0.001},"claude-opus-4-6":{"costUSD":0.4}}}}}'
            echo '{"jsonrpc":"2.0","id":'"$ID"',"result":{"stopReason":"end_turn","usage":{"inputTokens":7,"outputTokens":3,"totalTokens":10}}}'
            sleep 1
        "#;
        let (mut client, dir) = spawn_named_script("claude-code", script).await;
        client.set_turn_clock(idle_clock::ManualTurnClock::frozen());
        client.notify_session_spawned("model-session");
        client
            .session_prompt_with_idle_timeout(
                "model-session",
                "hello",
                std::time::Duration::from_secs(2),
                std::time::Duration::from_secs(5),
            )
            .await
            .expect("prompt");
        let usage = client.take_turn_usage().expect("usage");
        assert_eq!(usage.model.as_deref(), Some("claude-opus-4-6"));
        drop(client);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A Claude adapter is asked for its `result` frames on `session/new`
    /// even with the diagnostic flag off — that is where the model id lives.
    #[cfg(unix)]
    #[tokio::test]
    async fn claude_named_adapter_requests_result_frames_on_session_new() {
        let script = r#"
            read -t 2 _init
            echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
            read -t 2 REQ
            echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"ses_test","_receivedRequest":'"$REQ"'}}'
            sleep 1
        "#;
        let (mut client, dir) = spawn_named_script("claude-code", script).await;
        client.initialize().await.expect("initialize");
        let response = client
            .session_new_full("/tmp", vec![], None, None)
            .await
            .expect("session/new");
        let sent = &response.raw["_receivedRequest"]["params"];
        assert_eq!(
            sent.pointer("/_meta/claudeCode/emitRawSDKMessages"),
            Some(&serde_json::json!([{ "type": "result" }])),
            "{sent}"
        );
        drop(client);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The Claude adapter's wire lifecycle: one `usage_update` before the
    /// prompt response, and both folded into one turn's usage record.
    ///
    /// Runs on a [`ManualTurnClock`] that is never advanced. The assertions
    /// here are about *bookkeeping*, and on the real clock they were also,
    /// silently, about how quickly the operating system schedules a spawned
    /// `sh`: under a saturating build the 2 s idle window expired with
    /// `frames: 0, bytes: 0, quiet_for: 2.003s` — the child had not written
    /// its first line yet. Nothing in the turn's deadlines can now expire
    /// because of load; `manual_clock_still_fires_the_idle_timeout_when_advanced`
    /// and `keepalive_resets_idle_past_deadline` keep that from being vacuous
    /// by proving the same clock does fire — and does reset — when it moves.
    #[cfg(unix)]
    #[tokio::test]
    async fn claude_named_adapter_wire_lifecycle_records_prompt_and_cost() {
        // The `sleep 3` is the flake's own condition, made permanent: a first
        // frame that arrives *after* the 2 s idle window. On the real clock
        // that is exactly what a loaded box produced by accident
        // (`IdleTimeout { frames: 0, bytes: 0, quiet_for: 2.003s }`) and what
        // this test now survives on purpose. Delete the injected clock below
        // and this test fails every time instead of one run in fifty.
        let script = r#"
            read -r REQ
            ID=$(printf '%s' "$REQ" | sed -E 's/.*"id":([0-9]+).*/\1/')
            sleep 3
            echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"wire-session","update":{"sessionUpdate":"usage_update","cost":{"amount":0.5,"currency":"USD"}}}}'
            echo '{"jsonrpc":"2.0","id":'"$ID"',"result":{"stopReason":"end_turn","usage":{"inputTokens":7,"outputTokens":3,"totalTokens":10,"cachedReadTokens":2}}}'
            sleep 1
        "#;
        let (mut client, dir) = spawn_named_script("claude-code", script).await;
        assert_eq!(client.standard_adapter, Some(StandardAdapterKind::Claude));
        client.set_turn_clock(idle_clock::ManualTurnClock::frozen());
        client.notify_session_spawned("wire-session");

        let stop = client
            .session_prompt_with_idle_timeout(
                "wire-session",
                "hello",
                std::time::Duration::from_secs(2),
                std::time::Duration::from_secs(5),
            )
            .await
            .expect("wire prompt");
        assert_eq!(stop, StopReason::EndTurn);

        let usage = client.take_turn_usage().expect("wire usage");
        assert_eq!(usage.turn_seq, 1);
        assert_eq!(usage.turn_input_tokens, Some(9));
        assert_eq!(usage.turn_output_tokens, Some(3));
        assert_eq!(usage.turn_cost_usd, Some(0.5));
        assert_eq!(usage.cumulative_cost_usd, Some(0.5));
        drop(client);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The injected clock is a real clock, not a way to switch the deadlines
    /// off: advanced past the idle window with a silent agent, the loop still
    /// returns [`AcpError::IdleTimeout`]. Without this,
    /// `claude_named_adapter_wire_lifecycle_records_prompt_and_cost` could
    /// pass because nothing was being measured at all.
    #[tokio::test]
    async fn manual_clock_still_fires_the_idle_timeout_when_advanced() {
        let mut client = spawn_script("sleep 30").await;
        let clock = idle_clock::ManualTurnClock::frozen();
        client.set_turn_clock(clock.clone());

        let window = std::time::Duration::from_secs(10);
        let hard_deadline = clock.now() + std::time::Duration::from_secs(3600);
        let driver = async {
            // Wall time never moves this clock, so the loop is parked on
            // `sleep_until` until the advance below wakes it.
            tokio::task::yield_now().await;
            clock.advance(window + std::time::Duration::from_secs(1));
        };
        let (result, ()) = tokio::join!(
            client.read_until_response_with_idle_timeout(
                "idle-session",
                999,
                window,
                hard_deadline,
                std::time::Duration::from_secs(3600),
            ),
            driver,
        );

        assert!(
            matches!(result, Err(AcpError::IdleTimeout { .. })),
            "expected IdleTimeout on an advanced manual clock, got {result:?}"
        );
    }

    #[tokio::test]
    async fn claude_cost_only_record_survives_missing_prompt_usage() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Claude);
        client.notify_session_spawned("cost-only-session");
        client.standard_usage.begin_turn("cost-only-session");
        client.handle_session_update(&standard_cost_update("cost-only-session", 0.125));

        let usage = client.take_turn_usage().expect("cost-only usage");
        assert_eq!(usage.turn_seq, 1);
        assert!(usage.delta_reliable);
        assert_eq!(usage.turn_input_tokens, None);
        assert_eq!(usage.turn_cost_usd, Some(0.125));
        assert_eq!(usage.cumulative_cost_usd, Some(0.125));
    }

    #[tokio::test]
    async fn attached_claude_session_does_not_invent_first_cost_delta() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Claude);
        client.standard_usage.begin_turn("attached-session");
        client.handle_session_update(&standard_cost_update("attached-session", 1.25));
        client
            .parse_prompt_response(
                "attached-session",
                &prompt_response_usage(10, 2, 12, None, None),
            )
            .unwrap();

        let usage = client.take_turn_usage().expect("attached usage");
        assert_eq!(usage.turn_cost_usd, None);
        assert_eq!(usage.cumulative_cost_usd, Some(1.25));
    }

    #[tokio::test]
    async fn standard_usage_two_prompts_preserve_both_monotonic_sequences() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Claude);
        client.notify_session_spawned("two-prompt-session");

        client.standard_usage.begin_turn("two-prompt-session");
        client.handle_session_update(&standard_cost_update("two-prompt-session", 0.1));
        client
            .parse_prompt_response(
                "two-prompt-session",
                &prompt_response_usage(10, 2, 12, None, None),
            )
            .unwrap();
        let initial = client.take_turn_usage().expect("initial prompt usage");

        client.standard_usage.begin_turn("two-prompt-session");
        client.handle_session_update(&standard_cost_update("two-prompt-session", 0.25));
        client
            .parse_prompt_response(
                "two-prompt-session",
                &prompt_response_usage(20, 3, 23, None, None),
            )
            .unwrap();
        let user = client.take_turn_usage().expect("user prompt usage");

        assert_eq!((initial.turn_seq, user.turn_seq), (1, 2));
        assert_eq!(
            (initial.turn_input_tokens, user.turn_input_tokens),
            (Some(10), Some(20))
        );
        assert_eq!(
            (initial.turn_cost_usd, user.turn_cost_usd),
            (Some(0.1), Some(0.15))
        );
    }

    #[tokio::test]
    async fn goose_usage_stays_exclusive_and_drains_standard_usage() {
        let mut client = spawn_inert_client().await;
        client.standard_adapter = Some(StandardAdapterKind::Claude);
        client.goose_usage.begin_turn("goose-session");
        client.standard_usage.begin_turn("goose-session");
        client.handle_goose_usage_update(&goose_usage_update_msg("goose-session", 1000, 200, None));
        client
            .parse_prompt_response(
                "goose-session",
                &prompt_response_usage(100, 20, 120, None, None),
            )
            .unwrap();

        let usage = client.take_turn_usage().expect("goose usage");
        assert_eq!(usage.cumulative_input_tokens, Some(1000));
        assert_eq!(
            usage.turn_input_tokens, None,
            "goose first delta remains exclusive"
        );
        assert!(
            client.take_turn_usage().is_none(),
            "standard usage was drained"
        );
    }

    // ── Goose usage notification integration ──────────────────────────────

    /// Build a `_goose/unstable/session/update` JSON-RPC notification.
    fn goose_usage_update_msg(
        session_id: &str,
        input: u64,
        output: u64,
        cost: Option<f64>,
    ) -> serde_json::Value {
        let mut update = serde_json::json!({
            "sessionUpdate": "usage_update",
            "used": input + output,
            "contextLimit": 200000u64,
            "accumulatedInputTokens": input,
            "accumulatedOutputTokens": output,
        });
        if let Some(c) = cost {
            update["accumulatedCost"] = serde_json::json!(c);
        }
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "_goose/unstable/session/update",
            "params": {
                "sessionId": session_id,
                "update": update
            }
        })
    }

    #[tokio::test]
    async fn goose_usage_notification_recorded_and_take_returns_usage() {
        let mut client = spawn_inert_client().await;
        assert!(client.take_turn_usage().is_none(), "starts empty");

        // begin_turn before sending the prompt — mirrors the real call flow.
        client.goose_usage.begin_turn("s1");
        let msg = goose_usage_update_msg("s1", 1000, 200, Some(0.01));
        client.handle_goose_usage_update(&msg);

        let usage = client
            .take_turn_usage()
            .expect("usage should be present after notification");
        assert_eq!(usage.session_id, "s1");
        assert_eq!(usage.turn_seq, 1);
        assert!(!usage.delta_reliable, "first turn must be unreliable");
        assert_eq!(usage.cumulative_input_tokens, Some(1000));
        assert_eq!(usage.cumulative_output_tokens, Some(200));
        assert_eq!(usage.cumulative_cost_usd, Some(0.01));

        // Second take must be None.
        assert!(
            client.take_turn_usage().is_none(),
            "take after drain is None"
        );
    }

    #[tokio::test]
    async fn goose_usage_second_turn_delta_reliable() {
        let mut client = spawn_inert_client().await;
        // Turn 1.
        client.goose_usage.begin_turn("s2");
        client.handle_goose_usage_update(&goose_usage_update_msg("s2", 1000, 200, None));
        let _ = client.take_turn_usage();
        // Turn 2.
        client.goose_usage.begin_turn("s2");
        client.handle_goose_usage_update(&goose_usage_update_msg("s2", 1800, 450, None));
        let usage = client.take_turn_usage().expect("turn 2 usage");
        assert!(usage.delta_reliable);
        assert_eq!(usage.turn_input_tokens, Some(800));
        assert_eq!(usage.turn_output_tokens, Some(250));
    }

    #[tokio::test]
    async fn goose_usage_malformed_notification_does_not_panic() {
        let mut client = spawn_inert_client().await;
        // Missing params entirely.
        let bad = serde_json::json!({"jsonrpc":"2.0","method":"_goose/unstable/session/update"});
        client.handle_goose_usage_update(&bad);
        assert!(client.take_turn_usage().is_none());

        // params present but wrong shape.
        let bad2 = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "_goose/unstable/session/update",
            "params": { "oops": true }
        });
        client.handle_goose_usage_update(&bad2);
        assert!(client.take_turn_usage().is_none());
    }

    #[test]
    fn agent_error_from_json_falls_back_to_full_json_when_message_missing() {
        // Errors without a string `message` field (e.g. only a `data` field) must
        // not be silently truncated to "unknown error" — the full JSON is preserved.
        let error = serde_json::json!({"code": -32000, "data": "quota exceeded"});
        match super::agent_error_from_json(&error) {
            AcpError::AgentError { code, message } => {
                assert_eq!(code, -32000);
                assert!(
                    message.contains("quota exceeded"),
                    "expected full JSON in message, got: {message}"
                );
            }
            other => panic!("expected AgentError, got {other:?}"),
        }
    }

    #[test]
    fn agent_error_from_json_uses_message_field_when_present() {
        let error = serde_json::json!({"code": -32001, "message": "auth denied"});
        match super::agent_error_from_json(&error) {
            AcpError::AgentError { code, message } => {
                assert_eq!(code, -32001);
                assert_eq!(message, "auth denied");
            }
            other => panic!("expected AgentError, got {other:?}"),
        }
    }

    // ── build_codex_config_env ────────────────────────────────────────────────

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    const GENERATED: &str = r#"{"sandbox_workspace_write":{"network_access":true}}"#;

    #[test]
    fn build_codex_config_env_returns_none_when_no_codex_config_in_extra_env() {
        // Non-Codex agents: extra_env has no CODEX_CONFIG → None regardless of signal.
        let extra = env(&[("GOOSE_PROVIDER", "openai")]);
        let result = build_codex_config_env(&extra, None, false).unwrap();
        assert_eq!(
            result, None,
            "no CODEX_CONFIG in extra_env must return None"
        );
    }

    #[test]
    fn build_codex_config_env_generated_only_single_entry_with_signal_true_merges_with_parent() {
        // No persona: Buzz injects one CODEX_CONFIG; signal=true.
        // Parent may have its own CODEX_CONFIG — deep_merge applies, network_access forced.
        let extra = env(&[("CODEX_CONFIG", GENERATED)]);
        let parent =
            r#"{"some_operator_key":"val","sandbox_workspace_write":{"operator_key":"keep"}}"#;
        let merged = build_codex_config_env(&extra, Some(parent), true)
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        // network_access forced true even though only one entry in extra_env.
        assert_eq!(
            v["sandbox_workspace_write"]["network_access"], true,
            "network_access must be forced true with signal=true"
        );
        // Operator key preserved via deep_merge.
        assert_eq!(
            v["sandbox_workspace_write"]["operator_key"], "keep",
            "operator nested key must survive"
        );
        assert_eq!(
            v["some_operator_key"], "val",
            "operator top-level key must survive"
        );
    }

    #[test]
    fn build_codex_config_env_persona_only_signal_false_returns_none() {
        // Persona set CODEX_CONFIG; Buzz did not inject a generated overlay (signal=false).
        // Must return None — no merging, no sandbox widening.
        let persona = r#"{"some_feature":"on"}"#;
        let extra = env(&[("CODEX_CONFIG", persona)]);
        let result = build_codex_config_env(&extra, None, false).unwrap();
        assert_eq!(
            result, None,
            "persona-only CODEX_CONFIG with signal=false must return None"
        );
    }

    #[test]
    fn build_codex_config_env_returns_none_for_persona_only_no_generated_overlay() {
        // Alias: same scenario as above, confirms the old count-based path no longer exists.
        let persona = r#"{"some_feature":"on"}"#;
        let extra = env(&[("CODEX_CONFIG", persona)]);
        let result = build_codex_config_env(&extra, None, false).unwrap();
        assert_eq!(
            result, None,
            "persona-only CODEX_CONFIG with signal=false must return None"
        );
    }

    #[test]
    fn build_codex_config_env_sets_network_access_from_scratch() {
        // Persona + generated overlay, signal=true: network_access is forced true.
        let persona = r#"{}"#;
        let extra = env(&[("CODEX_CONFIG", persona), ("CODEX_CONFIG", GENERATED)]);
        let merged = build_codex_config_env(&extra, None, true).unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["sandbox_workspace_write"]["network_access"], true);
    }

    #[test]
    fn build_codex_config_env_persona_keys_survive_merge() {
        // Persona has CODEX_CONFIG with unrelated keys; generated overlay must
        // force network_access=true without erasing persona keys.
        let persona_cfg = r#"{"some_feature":{"enabled":true}}"#;
        // Config::from_args appends generated AFTER persona env vars.
        let extra = env(&[("CODEX_CONFIG", persona_cfg), ("CODEX_CONFIG", GENERATED)]);
        let merged = build_codex_config_env(&extra, None, true).unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(
            v["some_feature"]["enabled"], true,
            "persona key must survive merge"
        );
        assert_eq!(
            v["sandbox_workspace_write"]["network_access"], true,
            "network_access must be forced true"
        );
    }

    #[test]
    fn build_codex_config_env_nested_persona_keys_survive_when_parent_has_same_top_level_key() {
        // Persona has sandbox_workspace_write.persona_only; parent has
        // sandbox_workspace_write.parent_only.  A flat top-level spread would drop
        // persona_only.  deep_merge must preserve both nested keys, and
        // network_access must be forced true last.
        let persona_cfg = r#"{"sandbox_workspace_write":{"persona_only":"keep_me"}}"#;
        let extra = env(&[("CODEX_CONFIG", persona_cfg), ("CODEX_CONFIG", GENERATED)]);
        let parent = r#"{"sandbox_workspace_write":{"parent_only":"also_here"}}"#;
        let merged = build_codex_config_env(&extra, Some(parent), true)
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        // Both nested keys survive — no flat-spread drop.
        assert_eq!(
            v["sandbox_workspace_write"]["persona_only"], "keep_me",
            "nested persona key must survive when parent has the same top-level key"
        );
        assert_eq!(
            v["sandbox_workspace_write"]["parent_only"], "also_here",
            "nested parent key must be present"
        );
        // Forced last.
        assert_eq!(
            v["sandbox_workspace_write"]["network_access"], true,
            "network_access must be forced true"
        );
    }

    #[test]
    fn build_codex_config_env_parent_env_wins_on_collisions_persona_keys_survive() {
        // Parent env has CODEX_CONFIG with some keys; persona has different keys.
        // Parent wins on collision; unrelated persona keys survive.
        // network_access is always forced true.
        let persona_cfg = r#"{"persona_key":"persona_val","shared_key":"persona_version"}"#;
        // Config::from_args appends generated AFTER persona env vars.
        let extra = env(&[("CODEX_CONFIG", persona_cfg), ("CODEX_CONFIG", GENERATED)]);
        let parent = r#"{"parent_key":"parent_val","shared_key":"parent_version"}"#;
        let merged = build_codex_config_env(&extra, Some(parent), true)
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        // Parent-only key present
        assert_eq!(
            v["parent_key"], "parent_val",
            "parent-only key must be present"
        );
        // Unrelated persona key survives (no collision with parent)
        assert_eq!(
            v["persona_key"], "persona_val",
            "unrelated persona key must survive"
        );
        // Collision: parent wins
        assert_eq!(
            v["shared_key"], "parent_version",
            "parent must win on colliding key"
        );
        // network_access always true (forced last)
        assert_eq!(v["sandbox_workspace_write"]["network_access"], true);
    }

    #[test]
    fn build_codex_config_env_parent_has_existing_sandbox_other_keys_survive() {
        // Parent env has sandbox_workspace_write with extra keys; after merge
        // those extra keys survive alongside network_access=true.
        let persona = r#"{}"#;
        let extra = env(&[("CODEX_CONFIG", persona), ("CODEX_CONFIG", GENERATED)]);
        let parent =
            r#"{"sandbox_workspace_write":{"network_access":false,"other_sandbox_key":"val"}}"#;
        let merged = build_codex_config_env(&extra, Some(parent), true)
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        // network_access forced true even though parent set false
        assert_eq!(v["sandbox_workspace_write"]["network_access"], true);
        // other_sandbox_key survives (parent's sws merged, then network_access forced)
        assert_eq!(v["sandbox_workspace_write"]["other_sandbox_key"], "val");
    }

    #[test]
    fn build_codex_config_env_errors_on_invalid_persona_json() {
        // Bad persona JSON + generated overlay, signal=true → parse error before merging.
        let extra = env(&[("CODEX_CONFIG", "not-json"), ("CODEX_CONFIG", GENERATED)]);
        let result = build_codex_config_env(&extra, None, true);
        assert!(result.is_err(), "invalid persona JSON must return Err");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("CODEX_CONFIG"),
            "error must mention CODEX_CONFIG"
        );
    }

    #[test]
    fn build_codex_config_env_errors_on_non_object_persona_json() {
        // Non-object persona JSON + generated overlay, signal=true → parse error.
        let extra = env(&[("CODEX_CONFIG", "[1,2,3]"), ("CODEX_CONFIG", GENERATED)]);
        let result = build_codex_config_env(&extra, None, true);
        assert!(result.is_err(), "non-object persona JSON must return Err");
    }

    #[test]
    fn build_codex_config_env_errors_on_invalid_parent_json() {
        let persona = r#"{}"#;
        let extra = env(&[("CODEX_CONFIG", persona), ("CODEX_CONFIG", GENERATED)]);
        let result = build_codex_config_env(&extra, Some("bad-json"), true);
        assert!(result.is_err(), "invalid parent env JSON must return Err");
    }

    #[test]
    fn build_codex_config_env_errors_on_non_object_sandbox_workspace_write() {
        // sandbox_workspace_write must be an object for network_access forcing.
        // If the parent env sets it to a non-object scalar, deep_merge replaces
        // our object with the scalar, and the force step must fail clearly.
        let persona = r#"{}"#;
        let extra = env(&[("CODEX_CONFIG", persona), ("CODEX_CONFIG", GENERATED)]);
        // Parent replaces the object with a scalar — deep_merge: scalar overlay wins.
        let parent = r#"{"sandbox_workspace_write": 42}"#;
        let result = build_codex_config_env(&extra, Some(parent), true);
        assert!(
            result.is_err(),
            "non-object sandbox_workspace_write must return Err"
        );
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("sandbox_workspace_write"),
            "error must mention sandbox_workspace_write"
        );
    }

    // ── Native steering transport (docs/NATIVE_STEERING_IMPL.md §3.1) ─────
    //
    // Every scenario below runs on a frozen `ManualTurnClock` against a
    // scripted bash agent, so nothing here depends on wall time: the agent
    // answers when the script says, deadlines move only when a test moves
    // them, and the wire assertions read the exact JSON the client wrote
    // (the observer's `acp_write` payload) rather than inferring the shape
    // from response routing.

    /// Everything one native-steer scenario needs.
    struct SteerHarness {
        client: AcpClient,
        clock: std::sync::Arc<idle_clock::ManualTurnClock>,
        reads: tokio::sync::broadcast::Receiver<crate::observer::ObserverEvent>,
        steer_tx: tokio::sync::mpsc::Sender<SteerInput>,
        late_rx: tokio::sync::mpsc::UnboundedReceiver<LateSteerAck>,
    }

    /// A turn budget nothing in these tests ever reaches unless a test
    /// advances the clock past it on purpose.
    const FAR: std::time::Duration = std::time::Duration::from_secs(3600);

    /// Shell prelude for the scripted agents: `steer_id "$line"` prints the
    /// JSON-RPC id of a request the client wrote, `prompt_done` answers the
    /// prompt (id 999) with `end_turn`.
    const STEER_SCRIPT_PRELUDE: &str = "steer_id() { printf '%s' \"$1\" | sed -E 's/.*\"id\":([0-9]+).*/\\1/'; }\n\
         prompt_done() { printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{\"stopReason\":\"end_turn\"}}'; }\n";

    /// One shell line that answers the request in `$line` with `answer`
    /// (a `"result":…` or `"error":…` fragment) under that request's id.
    fn answer_line(answer: &str) -> String {
        format!(
            "printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":'\"$(steer_id \"$line\")\"',{answer}}}'\n"
        )
    }

    /// Agent that reads one steer, answers it with `answer`, then finishes
    /// the prompt and idles.
    fn answer_then_finish_script(answer: &str) -> String {
        format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\n{}prompt_done\nsleep 30\n",
            answer_line(answer)
        )
    }

    async fn steer_harness(script: &str) -> SteerHarness {
        let mut client = spawn_script(script).await;
        set_steering_supported(&mut client);
        let observer = crate::observer::ObserverHandle::in_process();
        let reads = observer.subscribe();
        client.set_observer(Some(observer), 0);
        let clock = idle_clock::ManualTurnClock::frozen();
        client.set_turn_clock(clock.clone());
        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel::<SteerInput>(2);
        client.install_steer_input(steer_rx);
        let (late_tx, late_rx) = tokio::sync::mpsc::unbounded_channel::<LateSteerAck>();
        client.set_late_steer_sink(late_tx);
        SteerHarness {
            client,
            clock,
            reads,
            steer_tx,
            late_rx,
        }
    }

    fn steer_input(
        attempt_id: &str,
        text: &str,
        idle_guard: IdleGuard,
    ) -> (SteerInput, tokio::sync::oneshot::Receiver<SteerResolution>) {
        let (outcome_tx, outcome_rx) = tokio::sync::oneshot::channel();
        (
            SteerInput {
                attempt_id: attempt_id.to_owned(),
                prompt_blocks: vec![text.to_owned()],
                idle_guard,
                write_guard: None,
                outcome_tx,
            },
            outcome_rx,
        )
    }

    /// The next line the client wrote to the agent, as it went out.
    async fn next_write(
        reads: &mut tokio::sync::broadcast::Receiver<crate::observer::ObserverEvent>,
    ) -> serde_json::Value {
        loop {
            let event = reads.recv().await.expect("observer feed closed");
            if event.kind == "acp_write" {
                return event.payload;
            }
        }
    }

    /// Block until the client has read a response carrying `id`.
    async fn wait_for_read_of_id(
        reads: &mut tokio::sync::broadcast::Receiver<crate::observer::ObserverEvent>,
        id: u64,
    ) {
        loop {
            let event = reads.recv().await.expect("observer feed closed");
            if event.kind == "acp_read" && event.payload.get("id") == Some(&serde_json::json!(id)) {
                return;
            }
        }
    }

    async fn run_prompt_loop(client: &mut AcpClient) -> Result<serde_json::Value, AcpError> {
        let hard_deadline = client.turn_clock.now() + FAR;
        client
            .read_until_response_with_idle_timeout("sess-test", 999, FAR, hard_deadline, FAR)
            .await
    }

    /// Queue one input, run the prompt loop to its end, and return what was
    /// written, how the input resolved, and the prompt's own result.
    async fn run_single_steer(
        h: &mut SteerHarness,
        idle_guard: IdleGuard,
    ) -> (
        serde_json::Value,
        SteerResolution,
        Result<serde_json::Value, AcpError>,
    ) {
        let (input, outcome_rx) = steer_input("a1", "steer body", idle_guard);
        h.steer_tx.send(input).await.expect("queue steer input");
        let result = run_prompt_loop(&mut h.client).await;
        let written = next_write(&mut h.reads).await;
        let resolution = outcome_rx.await.expect("input must be resolved");
        (written, resolution, result)
    }

    /// A gate file a script polls for, so a test orders the agent's next line
    /// against something the client has already done.
    fn gate_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("buzz-acp-steer-gates");
        std::fs::create_dir_all(&dir).expect("create gate dir");
        let path = dir.join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn wait_for_gate(gate: &std::path::Path) -> String {
        format!("while [ ! -e '{}' ]; do sleep 0.01; done\n", gate.display())
    }

    fn assert_end_turn(result: &Result<serde_json::Value, AcpError>) {
        match result {
            Ok(value) => assert_eq!(value["stopReason"], serde_json::json!("end_turn")),
            Err(e) => panic!("prompt must have answered end_turn, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn native_steer_injected_ack_resolves_injected_on_the_written_id() {
        let mut h = steer_harness(&answer_then_finish_script(
            r#""result":{"outcome":"injected"}"#,
        ))
        .await;

        let (written, resolution, result) =
            run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert_eq!(written["id"], serde_json::json!(0));
        assert_eq!(written["method"], serde_json::json!(ACP_STEER_METHOD));
        assert_eq!(
            written["params"]["sessionId"],
            serde_json::json!("sess-test")
        );
        assert_eq!(
            written["params"]["prompt"],
            serde_json::json!([{"type": "text", "text": "steer body"}])
        );
        assert!(
            written["params"].get("_meta").is_none(),
            "AdapterDefault must send no _meta; wrote {written}"
        );
        assert!(
            written["params"].get("expectedRunId").is_none(),
            "_session/steering carries no run id; wrote {written}"
        );
        assert_eq!(
            resolution,
            SteerResolution::Injected {
                wire: SteerWire::AcpExtension,
                native_run_id: None,
            }
        );
        assert_end_turn(&result);
        assert!(h.client.unresolved_steer_attempts().is_empty());
        assert!(h.late_rx.try_recv().is_err(), "nothing was late");
    }

    #[tokio::test]
    async fn native_steer_started_new_turn_ack_resolves_started_new_turn() {
        let mut h = steer_harness(&answer_then_finish_script(
            r#""result":{"outcome":"startedNewTurn"}"#,
        ))
        .await;

        let (_, resolution, result) = run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert_eq!(
            resolution,
            SteerResolution::StartedNewTurn {
                wire: SteerWire::AcpExtension
            }
        );
        assert_end_turn(&result);
    }

    #[tokio::test]
    async fn native_steer_prompt_required_guard_is_on_the_wire_and_resolves_not_delivered() {
        let mut h = steer_harness(&answer_then_finish_script(
            r#""result":{"outcome":"promptRequired","reason":"noRunningTurn"}"#,
        ))
        .await;

        let (written, resolution, result) =
            run_single_steer(&mut h, IdleGuard::PromptRequired).await;

        assert_eq!(
            written["params"]["_meta"],
            serde_json::json!({"steering": {"idleBehavior": "promptRequired"}}),
            "PromptRequired must ride in _meta.steering.idleBehavior; wrote {written}"
        );
        assert_eq!(
            resolution,
            SteerResolution::NotDelivered {
                reason: NotDeliveredReason::PromptRequired
            }
        );
        assert_end_turn(&result);
        assert!(h.client.unresolved_steer_attempts().is_empty());
    }

    #[tokio::test]
    async fn native_steer_bare_success_resolves_unknown_unrecognized_ack() {
        let mut h = steer_harness(&answer_then_finish_script(r#""result":{}"#)).await;

        let (_, resolution, result) = run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert_eq!(
            resolution,
            SteerResolution::Unknown {
                reason: UnknownReason::UnrecognizedAck {
                    outcome: "<absent>".to_owned()
                },
                wire_request_id: Some(0),
            }
        );
        assert_end_turn(&result);
    }

    #[tokio::test]
    async fn native_steer_failed_outcome_resolves_unknown_adapter_reported_failure() {
        let mut h = steer_harness(&answer_then_finish_script(
            r#""result":{"outcome":"failed"}"#,
        ))
        .await;

        let (_, resolution, result) = run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert_eq!(
            resolution,
            SteerResolution::Unknown {
                reason: UnknownReason::AdapterReportedFailure {
                    outcome: "failed".to_owned()
                },
                wire_request_id: Some(0),
            }
        );
        assert_end_turn(&result);
    }

    #[tokio::test]
    async fn native_steer_method_not_found_resolves_not_delivered_method_not_found() {
        let mut h = steer_harness(&answer_then_finish_script(
            r#""error":{"code":-32601,"message":"Method not found"}"#,
        ))
        .await;

        let (_, resolution, result) = run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert_eq!(
            resolution,
            SteerResolution::NotDelivered {
                reason: NotDeliveredReason::MethodNotFound {
                    message: "Method not found".to_owned()
                }
            }
        );
        assert_end_turn(&result);
    }

    #[tokio::test]
    async fn native_steer_other_json_rpc_error_resolves_not_delivered_rejected() {
        let mut h = steer_harness(&answer_then_finish_script(
            r#""error":{"code":-32000,"message":"no steerable turn"}"#,
        ))
        .await;

        let (_, resolution, result) = run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert_eq!(
            resolution,
            SteerResolution::NotDelivered {
                reason: NotDeliveredReason::Rejected {
                    code: -32000,
                    message: "no steerable turn".to_owned()
                }
            }
        );
        assert_end_turn(&result);
    }

    /// The goose wire keeps its run-id form: `expectedRunId`, no `_meta`
    /// even when the input asks for the idle guard, and any success is
    /// `Injected` naming that run.
    #[tokio::test]
    async fn native_steer_goose_wire_names_the_run_and_carries_no_idle_guard() {
        let mut h = steer_harness(&answer_then_finish_script(r#""result":{}"#)).await;
        let update = session_info_update_msg(Some(serde_json::json!("run-7")));
        let _ = h.client.handle_session_update(&update);

        let (written, resolution, result) =
            run_single_steer(&mut h, IdleGuard::PromptRequired).await;

        assert_eq!(written["method"], serde_json::json!(GOOSE_STEER_METHOD));
        assert_eq!(
            written["params"]["expectedRunId"],
            serde_json::json!("run-7")
        );
        assert!(
            written["params"].get("_meta").is_none(),
            "goose wire is unchanged by the idle guard; wrote {written}"
        );
        assert_eq!(
            resolution,
            SteerResolution::Injected {
                wire: SteerWire::Goose,
                native_run_id: Some("run-7".to_owned()),
            }
        );
        assert_end_turn(&result);
    }

    /// No run id and no advertised capability: nothing is written, and the
    /// input is answered `Unsupported` — never probed.
    #[tokio::test]
    async fn native_steer_without_a_wire_writes_nothing_and_resolves_unsupported() {
        let mut h = steer_harness(&format!("{STEER_SCRIPT_PRELUDE}sleep 30\n")).await;
        h.client.steering_supported = false;

        let (input, outcome_rx) = steer_input("a1", "steer body", IdleGuard::PromptRequired);
        h.steer_tx.send(input).await.expect("queue steer input");
        let driver = async {
            let resolution = outcome_rx.await.expect("input must be resolved");
            // Nothing else will end the turn: the agent is silent.
            h.clock.advance(FAR + std::time::Duration::from_secs(1));
            resolution
        };
        let (result, resolution) = tokio::join!(run_prompt_loop(&mut h.client), driver);

        assert_eq!(
            resolution,
            SteerResolution::NotDelivered {
                reason: NotDeliveredReason::Unsupported
            }
        );
        assert!(
            matches!(
                result,
                Err(AcpError::IdleTimeout { .. }) | Err(AcpError::HardTimeout { .. })
            ),
            "expected the deadline the test forced, got {result:?}"
        );
        assert!(
            !matches!(h.reads.try_recv(), Ok(e) if e.kind == "acp_write"),
            "no request may reach the wire without a transport"
        );
        assert!(h.client.unresolved_steer_attempts().is_empty());
    }

    /// The agent closes its stdin before the input is queued, so the write
    /// fails: the outcome is `Unknown{WriteFailed}` carrying the id the
    /// request would have gone out under, and the attempt is remembered.
    #[tokio::test]
    async fn native_steer_write_failure_resolves_unknown_write_failed_with_the_request_id() {
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}exec 0<&-\n\
             printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{{\"sessionId\":\"sess-test\",\"update\":{{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{{\"type\":\"text\",\"text\":\"stdin closed\"}}}}}}}}'\n\
             sleep 30\n"
        );
        let mut h = steer_harness(&script).await;
        let (input, outcome_rx) = steer_input("a1", "steer body", IdleGuard::AdapterDefault);
        let steer_tx = h.steer_tx.clone();
        let reads = &mut h.reads;
        let clock = h.clock.clone();
        let driver = async move {
            // Only queue the input once the agent has told us stdin is gone.
            loop {
                let event = reads.recv().await.expect("observer feed closed");
                if event.kind == "acp_read" && event.payload["method"] == "session/update" {
                    break;
                }
            }
            steer_tx.send(input).await.expect("queue steer input");
            let resolution = outcome_rx.await.expect("input must be resolved");
            clock.advance(FAR + std::time::Duration::from_secs(1));
            resolution
        };
        let (result, resolution) = tokio::join!(run_prompt_loop(&mut h.client), driver);

        match resolution {
            SteerResolution::Unknown {
                reason: UnknownReason::WriteFailed { .. },
                wire_request_id: Some(0),
            } => {}
            other => panic!("expected Unknown{{WriteFailed}} on id 0, got {other:?}"),
        }
        assert!(
            matches!(
                result,
                Err(AcpError::IdleTimeout { .. }) | Err(AcpError::HardTimeout { .. })
            ),
            "expected the deadline the test forced, got {result:?}"
        );
        assert_eq!(h.client.unresolved_steer_attempts(), vec!["a1".to_owned()]);
    }

    /// The prompt answers first; the ACK lands inside the bounded drain and
    /// resolves the attempt, while the prompt's own result is untouched.
    #[tokio::test]
    async fn native_steer_ack_inside_post_prompt_drain_resolves_injected() {
        let gate = gate_path("ack-inside-drain");
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\nprompt_done\n{}{}sleep 30\n",
            wait_for_gate(&gate),
            answer_line(r#""result":{"outcome":"injected"}"#),
        );
        let mut h = steer_harness(&script).await;
        let (input, outcome_rx) = steer_input("a1", "steer body", IdleGuard::PromptRequired);
        h.steer_tx.send(input).await.expect("queue steer input");
        let reads = &mut h.reads;
        let driver = async move {
            // Let the agent answer only after the client has read the prompt
            // response, so the ACK can only be met by the drain.
            wait_for_read_of_id(reads, 999).await;
            std::fs::write(&gate, b"").expect("open gate");
        };
        let (result, ()) = tokio::join!(run_prompt_loop(&mut h.client), driver);

        assert_end_turn(&result);
        assert_eq!(
            outcome_rx.await.expect("input must be resolved"),
            SteerResolution::Injected {
                wire: SteerWire::AcpExtension,
                native_run_id: None,
            }
        );
        assert!(h.client.unresolved_steer_attempts().is_empty());
        assert!(
            h.late_rx.try_recv().is_err(),
            "resolved in time, nothing late"
        );
    }

    /// The drain never changes what the prompt returned: a prompt that
    /// errored stays errored even though the steer was acknowledged after it.
    #[tokio::test]
    async fn native_steer_drain_keeps_the_prompts_own_error() {
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\n\
             printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":999,\"error\":{{\"code\":-32603,\"message\":\"turn exploded\"}}}}'\n\
             {}sleep 30\n",
            answer_line(r#""result":{"outcome":"injected"}"#),
        );
        let mut h = steer_harness(&script).await;

        let (_, resolution, result) = run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert!(
            matches!(&result, Err(AcpError::AgentError { code: -32603, message }) if message.contains("turn exploded")),
            "prompt error must survive the drain, got {result:?}"
        );
        assert_eq!(
            resolution,
            SteerResolution::Injected {
                wire: SteerWire::AcpExtension,
                native_run_id: None,
            }
        );
    }

    /// The drain expires: the attempt resolves `Unknown{AckTimeout}` and is
    /// remembered; when the ACK finally arrives during a later
    /// `send_request` wait it is correlated and delivered to the late sink.
    #[tokio::test]
    async fn native_steer_ack_after_drain_times_out_then_arrives_as_late_ack() {
        let gate = gate_path("ack-after-drain");
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\nprompt_done\n{}{}\
             read -r line\n{}sleep 30\n",
            wait_for_gate(&gate),
            answer_line(r#""result":{"outcome":"injected"}"#),
            answer_line(r#""result":{"pong":true}"#),
        );
        let mut h = steer_harness(&script).await;
        let (input, outcome_rx) = steer_input("a1", "steer body", IdleGuard::PromptRequired);
        h.steer_tx.send(input).await.expect("queue steer input");
        let reads = &mut h.reads;
        let clock = h.clock.clone();
        let driver = async move {
            wait_for_read_of_id(reads, 999).await;
            clock.advance(STEER_ACK_DRAIN + std::time::Duration::from_millis(1));
        };
        let (result, ()) = tokio::join!(run_prompt_loop(&mut h.client), driver);

        assert_end_turn(&result);
        assert_eq!(
            outcome_rx.await.expect("input must be resolved"),
            SteerResolution::Unknown {
                reason: UnknownReason::AckTimeout,
                wire_request_id: Some(0),
            }
        );
        assert_eq!(h.client.unresolved_steer_attempts(), vec!["a1".to_owned()]);

        // Now the agent emits the late ACK, followed by the answer to the
        // ping `send_request` writes next.
        std::fs::write(&gate, b"").expect("open gate");
        let pong = h
            .client
            .send_request("ping", serde_json::json!({}))
            .await
            .expect("ping must still be answered");
        assert_eq!(pong, serde_json::json!({"pong": true}));

        let late = h.late_rx.try_recv().expect("late ACK must reach the sink");
        assert_eq!(
            late,
            LateSteerAck {
                attempt_id: "a1".to_owned(),
                resolution: SteerResolution::Injected {
                    wire: SteerWire::AcpExtension,
                    native_run_id: None,
                },
            }
        );
        assert!(h.client.unresolved_steer_attempts().is_empty());
    }

    /// A late ACK is also correlated by the *next prompt's* read loop.
    #[tokio::test]
    async fn native_steer_late_ack_is_correlated_by_the_next_prompt_loop() {
        let gate = gate_path("late-ack-next-prompt");
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\nprompt_done\n{}{}prompt_done\nsleep 30\n",
            wait_for_gate(&gate),
            answer_line(r#""result":{"outcome":"startedNewTurn"}"#),
        );
        let mut h = steer_harness(&script).await;
        let (input, outcome_rx) = steer_input("a1", "steer body", IdleGuard::AdapterDefault);
        h.steer_tx.send(input).await.expect("queue steer input");
        let reads = &mut h.reads;
        let clock = h.clock.clone();
        let driver = async move {
            wait_for_read_of_id(reads, 999).await;
            clock.advance(STEER_ACK_DRAIN + std::time::Duration::from_millis(1));
        };
        let (first, ()) = tokio::join!(run_prompt_loop(&mut h.client), driver);
        assert_end_turn(&first);
        assert!(matches!(
            outcome_rx.await.expect("resolved"),
            SteerResolution::Unknown {
                reason: UnknownReason::AckTimeout,
                wire_request_id: Some(0)
            }
        ));

        // Second turn: the late ACK precedes this prompt's response.
        let (_tx2, rx2) = tokio::sync::mpsc::channel::<SteerInput>(1);
        h.client.install_steer_input(rx2);
        std::fs::write(&gate, b"").expect("open gate");
        let second = run_prompt_loop(&mut h.client).await;
        assert_end_turn(&second);

        let late = h.late_rx.try_recv().expect("late ACK must reach the sink");
        assert_eq!(late.attempt_id, "a1");
        assert_eq!(
            late.resolution,
            SteerResolution::StartedNewTurn {
                wire: SteerWire::AcpExtension
            }
        );
        assert!(h.client.unresolved_steer_attempts().is_empty());
    }

    /// stdout closes right after the request went out: `RuntimeExited`, id
    /// attached, attempt remembered; the prompt reports `AgentExited`.
    #[tokio::test]
    async fn native_steer_eof_after_write_resolves_unknown_runtime_exited() {
        let mut h = steer_harness(&format!("{STEER_SCRIPT_PRELUDE}read -r line\nexit 0\n")).await;

        let (written, resolution, result) =
            run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert_eq!(written["id"], serde_json::json!(0));
        assert_eq!(
            resolution,
            SteerResolution::Unknown {
                reason: UnknownReason::RuntimeExited,
                wire_request_id: Some(0),
            }
        );
        assert!(
            matches!(result, Err(AcpError::AgentExited)),
            "expected AgentExited, got {result:?}"
        );
        assert_eq!(h.client.unresolved_steer_attempts(), vec!["a1".to_owned()]);
    }

    /// Two inputs go out once each, in order, one at a time, and each
    /// carries its own idle guard.
    #[tokio::test]
    async fn native_steer_two_inputs_are_written_once_each_in_order() {
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\n{}read -r line\n{}prompt_done\nsleep 30\n",
            answer_line(r#""result":{"outcome":"injected"}"#),
            answer_line(r#""result":{"outcome":"injected"}"#),
        );
        let mut h = steer_harness(&script).await;
        let (first, first_rx) = steer_input("a1", "first", IdleGuard::AdapterDefault);
        let (second, second_rx) = steer_input("a2", "second", IdleGuard::PromptRequired);
        h.steer_tx.send(first).await.expect("queue first");
        h.steer_tx.send(second).await.expect("queue second");

        let result = run_prompt_loop(&mut h.client).await;
        assert_end_turn(&result);

        let w1 = next_write(&mut h.reads).await;
        let w2 = next_write(&mut h.reads).await;
        assert_eq!(w1["id"], serde_json::json!(0));
        assert_eq!(
            w1["params"]["prompt"][0]["text"],
            serde_json::json!("first")
        );
        assert!(w1["params"].get("_meta").is_none(), "wrote {w1}");
        assert_eq!(w2["id"], serde_json::json!(1));
        assert_eq!(
            w2["params"]["prompt"][0]["text"],
            serde_json::json!("second")
        );
        assert_eq!(
            w2["params"]["_meta"]["steering"]["idleBehavior"],
            serde_json::json!("promptRequired"),
            "wrote {w2}"
        );
        // Exactly two requests went out.
        assert!(
            !matches!(h.reads.try_recv(), Ok(e) if e.kind == "acp_write"),
            "a third write is a duplicate delivery"
        );
        let injected = SteerResolution::Injected {
            wire: SteerWire::AcpExtension,
            native_run_id: None,
        };
        assert_eq!(first_rx.await.expect("first resolved"), injected);
        assert_eq!(second_rx.await.expect("second resolved"), injected);
        assert!(h.client.unresolved_steer_attempts().is_empty());
    }

    /// A response under some other id never resolves the pending attempt:
    /// the `failed` on id 777 is ignored and the real answer still lands.
    #[tokio::test]
    async fn native_steer_response_with_wrong_id_does_not_resolve_the_attempt() {
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\n\
             printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":777,\"result\":{{\"outcome\":\"failed\"}}}}'\n\
             {}prompt_done\nsleep 30\n",
            answer_line(r#""result":{"outcome":"injected"}"#),
        );
        let mut h = steer_harness(&script).await;

        let (_, resolution, result) = run_single_steer(&mut h, IdleGuard::AdapterDefault).await;

        assert_eq!(
            resolution,
            SteerResolution::Injected {
                wire: SteerWire::AcpExtension,
                native_run_id: None,
            }
        );
        assert_end_turn(&result);
        assert!(h.late_rx.try_recv().is_err(), "id 777 belongs to nobody");
    }

    /// §4's mis-correlation fixture: the agent answers the second steer
    /// under the first steer's id. The first is already resolved, so that
    /// answer is stray; the second must not borrow it and ends
    /// `Unknown{AckTimeout}` under its own id.
    #[tokio::test]
    async fn native_steer_ack_reusing_an_earlier_id_is_refused_for_the_later_attempt() {
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\nfirst_id=$(steer_id \"$line\")\n\
             printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":'\"$first_id\"',\"result\":{{\"outcome\":\"injected\"}}}}'\n\
             read -r line\n\
             printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":'\"$first_id\"',\"result\":{{\"outcome\":\"injected\"}}}}'\n\
             prompt_done\nsleep 30\n"
        );
        let mut h = steer_harness(&script).await;
        let (first, first_rx) = steer_input("a1", "first", IdleGuard::AdapterDefault);
        let (second, second_rx) = steer_input("a2", "second", IdleGuard::AdapterDefault);
        h.steer_tx.send(first).await.expect("queue first");
        h.steer_tx.send(second).await.expect("queue second");
        let reads = &mut h.reads;
        let clock = h.clock.clone();
        let driver = async move {
            wait_for_read_of_id(reads, 999).await;
            clock.advance(STEER_ACK_DRAIN + std::time::Duration::from_millis(1));
        };
        let (result, ()) = tokio::join!(run_prompt_loop(&mut h.client), driver);

        assert_end_turn(&result);
        assert_eq!(
            first_rx.await.expect("first resolved"),
            SteerResolution::Injected {
                wire: SteerWire::AcpExtension,
                native_run_id: None,
            }
        );
        assert_eq!(
            second_rx.await.expect("second resolved"),
            SteerResolution::Unknown {
                reason: UnknownReason::AckTimeout,
                wire_request_id: Some(1),
            }
        );
        assert_eq!(h.client.unresolved_steer_attempts(), vec!["a2".to_owned()]);
        assert!(
            h.late_rx.try_recv().is_err(),
            "a stray answer is not a late ACK"
        );
    }

    /// The prompt future is dropped (cancel / shutdown) with the request
    /// written and unanswered: the caller still gets an answer —
    /// `Unknown{PromptEndedBeforeAck}` under the written id — and the
    /// attempt is remembered for a late ACK.
    #[tokio::test]
    async fn native_steer_cancel_while_ack_pending_resolves_unknown_and_remembers_the_attempt() {
        let mut h = steer_harness(&format!("{STEER_SCRIPT_PRELUDE}read -r line\nsleep 30\n")).await;
        let (input, outcome_rx) = steer_input("a1", "steer body", IdleGuard::PromptRequired);
        h.steer_tx.send(input).await.expect("queue steer input");
        let SteerHarness { client, reads, .. } = &mut h;
        tokio::select! {
            biased;
            _ = async {
                // The request is on the wire; now abandon the prompt.
                let written = next_write(reads).await;
                assert_eq!(written["id"], serde_json::json!(0));
            } => {}
            result = run_prompt_loop(client) => {
                panic!("the prompt loop must still be running, returned {result:?}");
            }
        }

        assert_eq!(
            outcome_rx.await.expect("dropped loop must still answer"),
            SteerResolution::Unknown {
                reason: UnknownReason::PromptEndedBeforeAck,
                wire_request_id: Some(0),
            }
        );
        assert_eq!(h.client.unresolved_steer_attempts(), vec!["a1".to_owned()]);
        assert!(
            h.client.steer_rx_is_none(),
            "the loop consumed the source; nothing is left to clear"
        );
        h.client.shutdown().await;
    }

    /// An input still in the channel when the prompt ends is never written:
    /// its oneshot closes, which is the caller's `PromptEndedBeforeWrite`.
    /// The input that did go out is still answered.
    #[tokio::test]
    async fn native_steer_input_left_in_channel_at_exit_is_dropped_unanswered() {
        let mut h = steer_harness(&format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\nprompt_done\nsleep 30\n"
        ))
        .await;
        let (first, first_rx) = steer_input("a1", "first", IdleGuard::AdapterDefault);
        let (second, second_rx) = steer_input("a2", "second", IdleGuard::AdapterDefault);
        h.steer_tx.send(first).await.expect("queue first");
        h.steer_tx.send(second).await.expect("queue second");
        let reads = &mut h.reads;
        let clock = h.clock.clone();
        let driver = async move {
            // The first request is out and unanswered when the prompt ends;
            // expire its drain so the loop can leave. The write is captured
            // here because this driver is what drains the observer feed.
            let w1 = next_write(reads).await;
            wait_for_read_of_id(reads, 999).await;
            clock.advance(STEER_ACK_DRAIN + std::time::Duration::from_millis(1));
            w1
        };
        let (result, w1) = tokio::join!(run_prompt_loop(&mut h.client), driver);
        assert_end_turn(&result);

        assert_eq!(
            first_rx
                .await
                .expect("first was written and must be answered"),
            SteerResolution::Unknown {
                reason: UnknownReason::AckTimeout,
                wire_request_id: Some(0),
            }
        );
        assert!(
            second_rx.await.is_err(),
            "the second input never left the channel; its oneshot must simply close"
        );
        assert_eq!(
            w1["params"]["prompt"][0]["text"],
            serde_json::json!("first")
        );
        assert!(
            !matches!(h.reads.try_recv(), Ok(e) if e.kind == "acp_write"),
            "the second input must never reach the wire"
        );
        assert!(h.client.steer_rx_is_none());
    }

    #[tokio::test]
    #[should_panic(expected = "install_steer_input")]
    async fn install_steer_input_panics_when_a_source_is_already_installed() {
        let mut client = spawn_inert_client().await;
        let (_tx1, rx1) = tokio::sync::mpsc::channel::<SteerInput>(1);
        let (_tx2, rx2) = tokio::sync::mpsc::channel::<SteerInput>(1);
        client.install_steer_input(rx1);
        client.install_steer_input(rx2);
    }

    #[tokio::test]
    async fn clear_steer_input_is_idempotent_and_shared_with_the_legacy_alias() {
        let mut client = spawn_inert_client().await;
        assert!(client.steer_rx_is_none());
        client.clear_steer_input();
        assert!(client.steer_rx_is_none(), "clearing nothing is a no-op");

        let (_tx, rx) = tokio::sync::mpsc::channel::<SteerInput>(1);
        client.install_steer_input(rx);
        assert!(!client.steer_rx_is_none());
        client.clear_steer_rx();
        assert!(
            client.steer_rx_is_none(),
            "the legacy alias clears the same slot"
        );

        let (_ltx, lrx) = tokio::sync::mpsc::channel::<crate::pool::SteerRequest>(1);
        client.install_steer_rx(lrx);
        client.clear_steer_input();
        client.clear_steer_input();
        assert!(client.steer_rx_is_none());
        assert!(client.unresolved_steer_attempts().is_empty());
    }

    #[test]
    fn decode_steer_ack_covers_every_locked_outcome() {
        let ok = |result: serde_json::Value| serde_json::json!({"id": 4, "result": result});
        let acp = SteerWire::AcpExtension;
        assert_eq!(
            decode_steer_ack(
                &ok(serde_json::json!({"outcome": "injected"})),
                acp,
                None,
                4
            ),
            SteerResolution::Injected {
                wire: acp,
                native_run_id: None
            }
        );
        assert_eq!(
            decode_steer_ack(
                &ok(serde_json::json!({"outcome": "startedNewTurn"})),
                acp,
                None,
                4
            ),
            SteerResolution::StartedNewTurn { wire: acp }
        );
        assert_eq!(
            decode_steer_ack(
                &ok(serde_json::json!({"outcome": "promptRequired"})),
                acp,
                None,
                4
            ),
            SteerResolution::NotDelivered {
                reason: NotDeliveredReason::PromptRequired
            }
        );
        assert_eq!(
            decode_steer_ack(&ok(serde_json::json!({"outcome": "failed"})), acp, None, 4),
            SteerResolution::Unknown {
                reason: UnknownReason::AdapterReportedFailure {
                    outcome: "failed".into()
                },
                wire_request_id: Some(4)
            }
        );
        assert_eq!(
            decode_steer_ack(&ok(serde_json::json!({"outcome": 12})), acp, None, 4),
            SteerResolution::Unknown {
                reason: UnknownReason::UnrecognizedAck {
                    outcome: "12".into()
                },
                wire_request_id: Some(4)
            }
        );
        assert_eq!(
            decode_steer_ack(&ok(serde_json::json!({})), acp, None, 4),
            SteerResolution::Unknown {
                reason: UnknownReason::UnrecognizedAck {
                    outcome: "<absent>".into()
                },
                wire_request_id: Some(4)
            }
        );
        // Goose: any success is a delivery into the named run, outcome or not.
        assert_eq!(
            decode_steer_ack(
                &ok(serde_json::json!({})),
                SteerWire::Goose,
                Some("r1".into()),
                4
            ),
            SteerResolution::Injected {
                wire: SteerWire::Goose,
                native_run_id: Some("r1".into())
            }
        );
        // Errors are the same on both wires.
        let err = serde_json::json!({"id": 4, "error": {"code": -32601, "message": "nope"}});
        assert_eq!(
            decode_steer_ack(&err, SteerWire::Goose, None, 4),
            SteerResolution::NotDelivered {
                reason: NotDeliveredReason::MethodNotFound {
                    message: "nope".into()
                }
            }
        );
        let err = serde_json::json!({"id": 4, "error": {"code": 7}});
        assert_eq!(
            decode_steer_ack(&err, acp, None, 4),
            SteerResolution::NotDelivered {
                reason: NotDeliveredReason::Rejected {
                    code: 7,
                    message: r#"{"code":7}"#.into()
                }
            }
        );
    }

    /// A prompt future dropped before its read loop ran `take()` leaves the
    /// steer source installed with admitted inputs in it. The cancel path
    /// must drop that source, not hand it to the cancel drain: nothing may
    /// be written after `session/cancel`, and every unwritten input's
    /// oneshot simply closes (`PromptEndedBeforeWrite` for the caller).
    #[tokio::test]
    async fn cancel_drops_admitted_steer_inputs_instead_of_writing_them_after_session_cancel() {
        // Agent: reads the `session/cancel` notification, answers the prompt
        // (id 1) as cancelled, then idles.
        let script = format!(
            "{STEER_SCRIPT_PRELUDE}read -r line\n\
             printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"stopReason\":\"cancelled\"}}}}'\n\
             sleep 30\n"
        );
        let mut h = steer_harness(&script).await;
        let (first, first_rx) = steer_input("a1", "first", IdleGuard::PromptRequired);
        let (second, second_rx) = steer_input("a2", "second", IdleGuard::PromptRequired);
        h.steer_tx.send(first).await.expect("queue first");
        h.steer_tx.send(second).await.expect("queue second");
        // The prompt went out as id 1 and its future was dropped before the
        // read loop took the source: exactly the state the cancel path sees.
        h.client.last_prompt_id = Some(1);
        h.client.next_id = 2;
        h.client.current_hard_deadline = Some(h.clock.now() + FAR);
        assert!(
            !h.client.steer_rx_is_none(),
            "precondition: the source is still installed"
        );

        let stop = h
            .client
            .cancel_with_cleanup("sess-test", FAR)
            .await
            .expect("cancel drain must complete");
        assert_eq!(stop, StopReason::Cancelled);

        assert!(h.client.steer_rx_is_none(), "cancel must drop the source");
        assert!(
            first_rx.await.is_err(),
            "an unwritten input's oneshot must close, never be answered"
        );
        assert!(second_rx.await.is_err());

        // The wire saw exactly one write: the cancel notification.
        let written = next_write(&mut h.reads).await;
        assert_eq!(written["method"], serde_json::json!("session/cancel"));
        while let Ok(event) = h.reads.try_recv() {
            assert_ne!(
                event.kind, "acp_write",
                "nothing may be written after session/cancel; wrote {}",
                event.payload
            );
        }
        assert!(h.client.unresolved_steer_attempts().is_empty());
    }

    // ── Live adapter run (ignored; evidence, not a gate) ──────────────────

    /// The node binary Beekeeper pins for its node tools, else whatever
    /// `node` is on PATH.
    fn beekeeper_node_binary(home: &std::path::Path) -> String {
        let root = home.join("Library/Application Support/Beekeeper/runtimes/node");
        let mut found = Vec::new();
        if let Ok(versions) = std::fs::read_dir(&root) {
            for version in versions.flatten() {
                if let Ok(platforms) = std::fs::read_dir(version.path()) {
                    for platform in platforms.flatten() {
                        let node = platform.path().join("bin/node");
                        if node.is_file() {
                            found.push(node);
                        }
                    }
                }
            }
        }
        found.sort();
        found
            .pop()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "node".to_owned())
    }

    /// Drives the REAL installed claude-agent-acp through `AcpClient` and the
    /// public `buzz_acp::steer` API. Prints every wire frame so the run log
    /// is the evidence. Skips (prints why, returns) when the adapter is not
    /// installed on this machine.
    ///
    /// Run: `cargo test -p buzz-acp native_steer_against_installed_claude_agent_acp -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "drives the installed claude-agent-acp and a real model; run with --ignored --nocapture"]
    async fn native_steer_against_installed_claude_agent_acp() {
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let adapter =
            home.join("Library/Application Support/Beekeeper/node-tools/bin/claude-agent-acp");
        if !adapter.exists() {
            println!("SKIP: adapter not installed at {}", adapter.display());
            return;
        }
        let adapter = std::fs::canonicalize(&adapter).unwrap_or(adapter);
        let node = beekeeper_node_binary(&home);
        let claude = home.join(".local/bin/claude");
        let path = format!(
            "{}:{}",
            home.join(".local/bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        println!(
            "LIVE node={node} adapter={} claude={}",
            adapter.display(),
            claude.display()
        );

        let extra_env = vec![
            (
                "CLAUDE_CODE_EXECUTABLE".to_owned(),
                claude.display().to_string(),
            ),
            ("PATH".to_owned(), path),
        ];
        let mut client =
            AcpClient::spawn(&node, &[adapter.display().to_string()], &extra_env, false)
                .await
                .expect("spawn claude-agent-acp");

        // Every frame the client writes or reads goes to stdout, and the
        // agent's streamed text is accumulated for the assertions below.
        let observer = crate::observer::ObserverHandle::in_process();
        let mut feed = observer.subscribe();
        client.set_observer(Some(observer), 0);
        let agent_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (text_len_tx, mut text_len_rx) = tokio::sync::watch::channel(0usize);
        let printer = {
            let agent_text = agent_text.clone();
            tokio::spawn(async move {
                loop {
                    let event = match feed.recv().await {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            println!("WIRE ?? observer lagged by {n} frames");
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    match event.kind.as_str() {
                        "acp_write" => println!("WIRE -> {}", event.payload),
                        "acp_read" => {
                            println!("WIRE <- {}", event.payload);
                            let update = &event.payload["params"]["update"];
                            if update["sessionUpdate"] == "agent_message_chunk" {
                                if let Some(text) = update["content"]["text"].as_str() {
                                    let len = {
                                        let mut buf =
                                            agent_text.lock().unwrap_or_else(|e| e.into_inner());
                                        buf.push_str(text);
                                        buf.len()
                                    };
                                    let _ = text_len_tx.send(len);
                                }
                            }
                        }
                        other => println!("OBS {other} {}", event.payload),
                    }
                }
            })
        };
        let snapshot = |buf: &std::sync::Arc<std::sync::Mutex<String>>| -> String {
            buf.lock().unwrap_or_else(|e| e.into_inner()).clone()
        };

        client.initialize().await.expect("initialize");
        assert!(
            client.steering_supported(),
            "claude-agent-acp must advertise _meta.steering.supported"
        );
        println!(
            "LIVE initialize ok: agent={} steering_supported={}",
            client.agent_name(),
            client.steering_supported()
        );

        let cwd = std::env::temp_dir().join(format!("buzz-acp-live-steer-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).expect("create session cwd");
        let session_id = client
            .session_new(&cwd.display().to_string(), Vec::new(), None, None)
            .await
            .expect("session/new");
        println!("LIVE session {session_id} cwd={}", cwd.display());

        // ── Turn 1: steer into a running turn ──────────────────────────
        let marker = uuid::Uuid::new_v4().simple().to_string();
        let ack_word = format!("ACK-{marker}");
        let (steer_tx, steer_rx) = tokio::sync::mpsc::channel::<SteerInput>(1);
        client.install_steer_input(steer_rx);
        let (late_tx, mut late_rx) = tokio::sync::mpsc::unbounded_channel::<LateSteerAck>();
        client.set_late_steer_sink(late_tx);

        let prompt = "Count slowly from 1 to 40, one number per line, one line at a time. \
                      Use no tools. Write nothing but the numbers.";
        let (outcome_tx, outcome_rx) = tokio::sync::oneshot::channel();
        let driver = async {
            // Wait until ~40 chars of the agent's answer have streamed, so
            // the turn is demonstrably in flight when the steer is written.
            loop {
                if *text_len_rx.borrow() >= 40 {
                    break;
                }
                text_len_rx.changed().await.expect("text feed closed");
            }
            let streamed = snapshot(&agent_text);
            println!(
                "LIVE {} chars streamed before steer: {streamed:?}",
                streamed.len()
            );
            steer_tx
                .send(SteerInput {
                    attempt_id: "live#1".to_owned(),
                    prompt_blocks: vec![format!(
                        "MARKER-{marker}: stop counting and reply with exactly the single word {ack_word}"
                    )],
                    idle_guard: IdleGuard::PromptRequired,
                    write_guard: None,
                    outcome_tx,
                })
                .await
                .expect("queue steer input");
            let resolution = outcome_rx.await.expect("live#1 must be resolved");
            println!("LIVE live#1 resolution: {resolution:?}");
            resolution
        };
        let (stop, resolution) = tokio::join!(
            client.session_prompt_with_idle_timeout(
                &session_id,
                prompt,
                std::time::Duration::from_secs(120),
                std::time::Duration::from_secs(300),
            ),
            driver,
        );
        let stop = stop.expect("turn 1 must complete");
        let text_after_turn_1 = snapshot(&agent_text);
        println!("LIVE turn 1 stop={stop:?}");
        println!("LIVE turn 1 agent text:\n{text_after_turn_1}");

        assert!(
            matches!(
                resolution,
                SteerResolution::Injected {
                    wire: SteerWire::AcpExtension,
                    ..
                }
            ),
            "live#1 must be Injected over _session/steering, got {resolution:?}"
        );
        assert_eq!(stop, StopReason::EndTurn, "turn 1 must end with end_turn");
        assert!(
            text_after_turn_1.contains(&ack_word),
            "the ACK word {ack_word} must appear in the SAME prompt's streamed text"
        );
        assert!(
            client.unresolved_steer_attempts().is_empty(),
            "nothing unresolved after an injected ACK"
        );
        assert!(late_rx.try_recv().is_err(), "nothing was late");
        println!("LIVE ACK word {ack_word} found in turn 1 text: yes");

        // ── Idle: the guard must keep the adapter from starting a turn ──
        let (idle_tx, idle_rx) = tokio::sync::mpsc::channel::<SteerInput>(1);
        client.install_steer_input(idle_rx);
        let (idle_outcome_tx, idle_outcome_rx) = tokio::sync::oneshot::channel();
        idle_tx
            .send(SteerInput {
                attempt_id: "live#2".to_owned(),
                prompt_blocks: vec![format!("Reply with exactly the word IDLE-{marker}.")],
                idle_guard: IdleGuard::PromptRequired,
                write_guard: None,
                outcome_tx: idle_outcome_tx,
            })
            .await
            .expect("queue idle steer");
        let text_before_idle = snapshot(&agent_text);
        // No prompt is in flight, so drive the read loop directly with an id
        // nobody will answer: the steer arm writes the request, the adapter
        // answers it, and the loop ends on the 12 s idle timeout, which is
        // the "no agent text within 10 s" window.
        let idle = std::time::Duration::from_secs(12);
        let hard = client.turn_clock.now() + std::time::Duration::from_secs(60);
        let idle_result = client
            .read_until_response_with_idle_timeout(
                &session_id,
                999_999,
                idle,
                hard,
                std::time::Duration::from_secs(60),
            )
            .await;
        let idle_resolution = idle_outcome_rx.await.expect("live#2 must be resolved");
        let text_after_idle = snapshot(&agent_text);
        println!("LIVE idle loop exit: {idle_result:?}");
        println!("LIVE live#2 resolution: {idle_resolution:?}");

        assert_eq!(
            idle_resolution,
            SteerResolution::NotDelivered {
                reason: NotDeliveredReason::PromptRequired
            },
            "an idle session with the guard must answer promptRequired"
        );
        assert!(
            matches!(idle_result, Err(AcpError::IdleTimeout { .. })),
            "the idle read must end on silence, got {idle_result:?}"
        );
        assert_eq!(
            text_after_idle, text_before_idle,
            "no agent text may follow an idle steer within the 12 s window"
        );
        println!("LIVE no agent text after idle steer within {idle:?}: yes");

        client.shutdown().await;
        drop(client);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), printer).await;
        let _ = std::fs::remove_dir_all(&cwd);
        println!("LIVE PASS marker={marker}");
    }
}
