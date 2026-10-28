//! The automatic output of a generation that received private continuity.
//!
//! Once a continuity block is admitted for an execution, everything the
//! provider publishes *automatically* from that execution's adapter — prose,
//! reasoning, plans, tool calls with their inputs, outputs and edits, turn
//! results and observed gate rows — goes in full to one owner-private
//! channel, and the shared channel receives only a deterministic twin built
//! here: kind, tool category, ids the adapter minted, status flags, timing and
//! usage counts. An observed gate row is repeated in the shared channel, where
//! the push gate and teammates read it, only when its command is one of a
//! fixed set and with its summary withheld ([`shared_gate_row`]). Nothing in the twin is copied from model or tool text, so a
//! value the model derived from private knowledge (a substring, a paraphrase,
//! a file name) has no field to travel in. Turn checkpoints stay in the shared
//! channel, where rewind looks them up, with their file names, branch and
//! summary withheld ([`withhold_checkpoint_detail`]). Registered-fragment
//! redaction ([`crate::transcript`]) still runs before the split, so it masks
//! whole registered fragments (an evidence id, the claim sentence) in the
//! owner's copy too; it is defence in depth, not the boundary.
//!
//! The owner-private channel is a NIP-29 DM (kind 41010) the provider opens
//! with the actor and its verified NIP-OA owner: the relay fixes the
//! participant set at open, idempotently for the same set, and enforces
//! membership on every read. The provider verifies the relay-signed 39000 for
//! that channel names exactly those three participants before it binds, at
//! every continuity admission. The owner reads it with the ordinary
//! `bee sessions transcript --channel <dm> --target <target>`; the desktop's
//! session view does not yet show it.
//!
//! This governs automatic projection only. What an agent deliberately sends
//! (messages, files, commits) is the agent's act, steered by instructions, and
//! is not filtered: this is not a general non-interference guarantee.
use serde_json::{json, Map, Value};
use uuid::Uuid;

/// Shown in the shared channel in place of a private generation's prose.
pub const PRIVATE_PROSE_NOTICE: &str = "[This execution received private continuity: its replies \
     and tool detail go only to its owner. Teammates see progress markers here; the agent shares \
     anything else deliberately.]";

/// A split generation's turn result, in place of the agent's final text.
pub const PRIVATE_RESULT_NOTICE: &str = "Result detail is private to this agent's owner.";

/// Where a session's automatic output goes from `from_generation` on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateOutput {
    /// The owner-private DM channel.
    pub channel: Uuid,
    /// The verified owner, lowercase hex.
    pub owner: String,
    /// The first generation whose automatic output is split. Later
    /// generations of the session inherit the split; it is never lifted.
    pub from_generation: u64,
}

impl PrivateOutput {
    /// Whether `generation`'s automatic output is split.
    pub fn covers(&self, generation: u64) -> bool {
        generation >= self.from_generation
    }
}

/// ACP's tool discriminants, the only tool vocabulary the twin repeats.
const TOOL_KINDS: &[&str] = &[
    "read",
    "edit",
    "delete",
    "move",
    "search",
    "execute",
    "think",
    "fetch",
    "switch_mode",
    "other",
];

/// The provider's own `errorCode`s a turn result may carry; the twin repeats
/// nothing else under that key.
const RESULT_ERROR_CODES: &[&str] = &[
    beekeeper_core::coding_session_payload::MODEL_AUTH_FAILED,
    beekeeper_core::coding_session_payload::CANCEL_NOT_CONFIRMED,
    beekeeper_core::coding_session_payload::TURN_INTERRUPTED_BY_RESTART,
];

/// `costReason` values ([`beekeeper_core::coding_session_payload::CostReason`]).
const COST_REASONS: &[&str] = &[
    "no_baseline",
    "unknown_model_price",
    "no_usage_reported",
    "adapter_reported_none",
];

/// A display name derived from the discriminant alone, never from the
/// adapter's `toolName` (claude-agent-acp puts the command line there).
fn tool_category(kind: Option<&str>) -> (&'static str, &'static str) {
    match kind.filter(|k| TOOL_KINDS.contains(k)) {
        Some("read") => ("read", "Read"),
        Some("edit") => ("edit", "Edit"),
        Some("delete") => ("delete", "Delete"),
        Some("move") => ("move", "Move"),
        Some("search") => ("search", "Search"),
        Some("execute") => ("execute", "Terminal"),
        Some("think") => ("think", "Think"),
        Some("fetch") => ("fetch", "Fetch"),
        Some("switch_mode") => ("switch_mode", "Mode"),
        _ => ("other", "Tool"),
    }
}

/// An adapter-minted id is repeated only when it is a short token; anything
/// longer or with whitespace could be prose.
fn safe_id(value: Option<&Value>) -> Option<Value> {
    let id = value?.as_str()?;
    (!id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b':'))
    .then(|| Value::String(id.to_owned()))
}

fn copy_numbers(from: &Value, keys: &[&str], to: &mut Map<String, Value>) {
    for key in keys {
        if let Some(n) = from.get(*key).filter(|v| v.is_number()) {
            to.insert((*key).into(), n.clone());
        }
    }
}

fn copy_bools(from: &Value, keys: &[&str], to: &mut Map<String, Value>) {
    for key in keys {
        if let Some(b) = from.get(*key).filter(|v| v.is_boolean()) {
            to.insert((*key).into(), b.clone());
        }
    }
}

/// A usage object with only numeric leaves, one level deep.
fn numeric_object(value: Option<&Value>) -> Option<Value> {
    let object = value?.as_object()?;
    let kept: Map<String, Value> = object
        .iter()
        .filter(|(k, v)| v.is_number() && k.len() <= 64)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    (!kept.is_empty()).then_some(Value::Object(kept))
}

/// What one turn of a private generation shows teammates.
#[derive(Default)]
pub struct TwinState {
    prose_noticed: bool,
}

impl TwinState {
    /// Start a turn: the next prose item announces itself once.
    pub fn begin_turn(&mut self) {
        self.prose_noticed = false;
    }

    /// The deterministic shared twin of `item`, or `None` when the shared
    /// channel gets nothing for it (the sequence number is then a gap, which
    /// NIP-CST permits). An allowlist: an item kind not named here has no twin.
    pub fn twin(&mut self, item: &Value) -> Option<Value> {
        let kind = item.get("kind").and_then(Value::as_str)?;
        let mut out = Map::new();
        out.insert("kind".into(), Value::String(kind.to_owned()));
        if let Some(parent) = safe_id(item.get("parentToolId")) {
            out.insert("parentToolId".into(), parent);
        }
        match kind {
            // Signed operator input: its source command is already readable
            // in this channel, so the echo keeps its existing visibility.
            "user_prompt" => {
                self.begin_turn();
                return Some(item.clone());
            }
            "assistant_text" => {
                if self.prose_noticed {
                    return None;
                }
                self.prose_noticed = true;
                out.insert("text".into(), Value::String(PRIVATE_PROSE_NOTICE.into()));
            }
            "tool_call" => {
                let tool = item.get("tool");
                let (category, name) = tool_category(
                    tool.and_then(|t| t.get("toolKind"))
                        .or_else(|| tool.and_then(|t| t.get("kind")))
                        .and_then(Value::as_str),
                );
                let mut twin = Map::new();
                twin.insert("toolKind".into(), Value::String(category.into()));
                twin.insert("toolName".into(), Value::String(name.into()));
                if let Some(id) = safe_id(tool.and_then(|t| t.get("toolId"))) {
                    twin.insert("toolId".into(), id);
                }
                out.insert("tool".into(), Value::Object(twin));
            }
            "tool_result" => {
                let (category, name) = tool_category(item.get("toolKind").and_then(Value::as_str));
                out.insert("toolKind".into(), Value::String(category.into()));
                out.insert("toolName".into(), Value::String(name.into()));
                if let Some(id) = safe_id(item.get("toolId")) {
                    out.insert("toolId".into(), id);
                }
                copy_bools(item, &["isError"], &mut out);
            }
            "result" => {
                if let Some(subtype) = item
                    .get("subtype")
                    .and_then(Value::as_str)
                    .filter(|s| matches!(*s, "success" | "error" | "cancelled"))
                {
                    out.insert("subtype".into(), Value::String(subtype.into()));
                }
                copy_bools(item, &["isError"], &mut out);
                // The adapter's words (`result`, a failure's `detail`) stay
                // with the owner; teammates get the outcome class and a
                // provider-defined code, never the text behind it.
                out.insert("result".into(), Value::String(PRIVATE_RESULT_NOTICE.into()));
                if let Some(code) = item
                    .get("errorCode")
                    .and_then(Value::as_str)
                    .filter(|c| RESULT_ERROR_CODES.contains(c))
                {
                    out.insert("errorCode".into(), Value::String(code.into()));
                }
                if let Some(reason) = item
                    .get("costReason")
                    .and_then(Value::as_str)
                    .filter(|r| COST_REASONS.contains(r))
                {
                    out.insert("costReason".into(), Value::String(reason.into()));
                }
                copy_numbers(
                    item,
                    &[
                        "durationMs",
                        "costUsd",
                        "inputTokens",
                        "outputTokens",
                        "totalTokens",
                    ],
                    &mut out,
                );
                // `costUsd: null` is the cost block's "no number" (ledger 266).
                if item.get("costUsd").is_some_and(Value::is_null) {
                    out.insert("costUsd".into(), Value::Null);
                }
                if let Some(basis) = item.get("costBasis").and_then(Value::as_str).filter(|b| {
                    matches!(
                        *b,
                        "adapter_estimate" | "table_estimate" | "billed" | "estimated"
                    )
                }) {
                    out.insert("costBasis".into(), Value::String(basis.into()));
                }
                if let Some(usage) = numeric_object(item.get("usage")) {
                    out.insert("usage".into(), usage);
                }
            }
            "context_window_updated" => {
                out.insert("usage".into(), numeric_object(item.get("usage"))?);
            }
            "plan" => {
                let entries = item.get("entries").and_then(Value::as_array)?;
                let statuses: Vec<Value> = entries
                    .iter()
                    .map(|e| {
                        let status = e
                            .get("status")
                            .and_then(Value::as_str)
                            .filter(|s| matches!(*s, "pending" | "in_progress" | "completed"))
                            .unwrap_or("pending");
                        json!({ "status": status, "content": "(private step)" })
                    })
                    .collect();
                let done = statuses
                    .iter()
                    .filter(|e| e["status"] == "completed")
                    .count();
                out.insert(
                    "text".into(),
                    Value::String(format!(
                        "Plan: {done} of {} steps completed (steps are private).",
                        statuses.len()
                    )),
                );
                out.insert("entries".into(), Value::Array(statuses));
            }
            "elided" => {
                out.insert("reason".into(), Value::String("private".into()));
            }
            _ => return None,
        }
        Some(Value::Object(out))
    }
}

/// Strip what a split generation's checkpoint copies from tool output: the
/// file list, omission paths, branch name and summary. Object ids stay,
/// because rewind restores from them; counts say how much was withheld.
pub fn withhold_checkpoint_detail(
    payload: &mut beekeeper_core::coding_session_checkpoint::CodingSessionCheckpointPayload,
) {
    payload.files_not_listed = payload
        .files_not_listed
        .saturating_add(payload.files.len() as u64);
    payload.files.clear();
    payload.summary = None;
    if let Some(git) = payload.git.as_mut() {
        git.branch = None;
        git.omitted_not_listed = git
            .omitted_not_listed
            .saturating_add(git.omitted.len() as u64);
        git.omitted.clear();
    }
}

/// Gate commands a split generation's shared gate row may repeat: exact
/// strings fixed here, so nothing the model chose can ride one. Any other
/// command line — a test filter, a path, an argument — keeps its row
/// owner-private.
const SHAREABLE_GATE_COMMANDS: &[&str] = &[
    "cargo fmt",
    "cargo fmt --check",
    "cargo fmt --all --check",
    "cargo fmt --all -- --check",
    "cargo clippy",
    "cargo clippy --all-targets -- -D warnings",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo test",
    "cargo test --workspace",
    "pnpm test",
    "pnpm typecheck",
    "pnpm lint",
    "just check",
    "just test",
    "just ci",
];

/// The shared copy of a split generation's observed gate row, or `None` when
/// it stays with the owner alone.
///
/// The facts a push gate and a teammate read — gate name, outcome, the
/// provider's own duration, `headSha`/`dirty` and the observing signature —
/// are the provider's, not the model's, and stay. The command is repeated only
/// when it is one of [`SHAREABLE_GATE_COMMANDS`] exactly, so it is still
/// *the exact command*, as NIP-CSOB requires; the summary (tool output) never
/// is. A row that cannot be shared truthfully is not shared with a substitute
/// command: an altered command would claim a run that did not happen.
pub fn shared_gate_row(
    row: &beekeeper_core::coding_session_observation::CodingSessionObservationGateRow,
) -> Option<beekeeper_core::coding_session_observation::CodingSessionObservationGateRow> {
    let command = row.command.split_whitespace().collect::<Vec<_>>().join(" ");
    SHAREABLE_GATE_COMMANDS
        .contains(&command.as_str())
        .then(|| {
            let mut shared = row.clone();
            shared.command = command;
            shared.summary = None;
            shared
        })
}

/// Why binding refused. Public codes only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindFailure {
    NoRelayClient,
    OpenRefused,
    MembershipUnverified,
}

/// Open (or reopen, idempotently) the provider/actor/owner DM and verify the
/// relay's own record of its participants before anything is published there.
pub async fn bind_channel(
    rest: &beekeeper_acp::relay::RestClient,
    relay_self: Option<&str>,
    provider: &str,
    actor: &str,
    owner: &str,
) -> Result<Uuid, BindFailure> {
    let relay = relay_self
        .and_then(|r| nostr::PublicKey::from_hex(r).ok())
        .ok_or(BindFailure::NoRelayClient)?;
    let open = beekeeper_sdk::builders::build_dm_open(&[actor, owner])
        .map_err(|_| BindFailure::OpenRefused)?
        .sign_with_keys(&rest.keys)
        .map_err(|_| BindFailure::OpenRefused)?;
    let response = rest
        .submit_event(&open)
        .await
        .map_err(|_| BindFailure::OpenRefused)?;
    let channel = response
        .get("message")
        .and_then(Value::as_str)
        .and_then(|m| m.strip_prefix("response:"))
        .and_then(|m| serde_json::from_str::<Value>(m).ok())
        .and_then(|v| v.get("channel_id")?.as_str().map(str::to_owned))
        .and_then(|id| Uuid::parse_str(&id).ok())
        .ok_or(BindFailure::OpenRefused)?;
    let mut expected = vec![
        provider.to_ascii_lowercase(),
        actor.to_ascii_lowercase(),
        owner.to_ascii_lowercase(),
    ];
    expected.sort();
    expected.dedup();
    // The discovery record is written after the open commits; allow it a
    // short, bounded moment to appear.
    for attempt in 0..10u64 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(100 * attempt)).await;
        }
        match verify_participants(rest, &relay, channel, &expected).await {
            Some(true) => return Ok(channel),
            Some(false) => return Err(BindFailure::MembershipUnverified),
            None => continue,
        }
    }
    Err(BindFailure::MembershipUnverified)
}

/// `Some(true)` when the newest relay-signed 39000 for `channel` is a DM whose
/// participants are exactly `expected`; `Some(false)` when it names others;
/// `None` when no such record could be read yet.
async fn verify_participants(
    rest: &beekeeper_acp::relay::RestClient,
    relay: &nostr::PublicKey,
    channel: Uuid,
    expected: &[String],
) -> Option<bool> {
    let filter = nostr::Filter::new()
        .kind(nostr::Kind::Custom(39000))
        .author(*relay)
        .identifier(channel.to_string())
        .limit(4);
    let rows = rest.query(&[filter]).await.ok()?;
    let events: Vec<nostr::Event> = rows
        .as_array()?
        .iter()
        .filter_map(|row| serde_json::from_value(row.clone()).ok())
        .collect();
    participants_verdict(&events, relay, channel, expected)
}

/// The verdict over candidate 39000 records: only a verified relay-signed one
/// counts, and the newest decides.
pub(crate) fn participants_verdict(
    events: &[nostr::Event],
    relay: &nostr::PublicKey,
    channel: Uuid,
    expected: &[String],
) -> Option<bool> {
    let newest = events
        .iter()
        .filter(|event| {
            event.verify().is_ok()
                && event.pubkey == *relay
                && event.kind == nostr::Kind::Custom(39000)
        })
        .max_by_key(|event| (event.created_at, std::cmp::Reverse(event.id)))?;
    let tag = |name: &str| -> Vec<String> {
        newest
            .tags
            .iter()
            .filter(|t| t.as_slice().first().map(String::as_str) == Some(name))
            .filter_map(|t| t.as_slice().get(1).cloned())
            .collect()
    };
    if tag("d").first().map(String::as_str) != Some(channel.to_string().as_str()) {
        return None;
    }
    if !tag("t").iter().any(|t| t == "dm") {
        return Some(false);
    }
    let mut participants: Vec<String> = tag("p").iter().map(|p| p.to_ascii_lowercase()).collect();
    participants.sort();
    participants.dedup();
    Some(participants == expected)
}

#[cfg(test)]
#[path = "private_output_tests.rs"]
mod tests;
