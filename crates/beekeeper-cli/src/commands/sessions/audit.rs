//! `bee sessions audit` — what a team's turns actually cost, from the signed
//! transcript and nothing else.
//!
//! Per-turn accounting has been on the wire since the `usage` block joined the
//! terminal `result` item (`beekeeper_core::coding_session_payload::TurnUsageReport`),
//! and until this command existed the only way to see it was to read kind 44225
//! by hand. The 2026-09-01 live run did exactly that, one turn at a time
//! (`plans/SESSION_STATE.md` item 103, finding 11).
//!
//! Three rules govern every number here:
//!
//! - **Absent is `null`, never `0`.** "The driver reported no output tokens"
//!   and "the driver reported zero output tokens" are different facts, and the
//!   archive keeps them apart; so does this command.
//! - **Nothing is inferred from a price list.** `costUsd` is the producer's
//!   own number, taken off the `result` item that closes the turn, or `null`.
//!   A cost computed here against a table this binary happens to carry would be
//!   an invention wearing a number's clothes.
//! - **Bounds are disclosed.** At most
//!   [`MAX_AUDIT_ITEMS_PER_EXECUTION`] items are folded per execution, and the
//!   report says when it stopped and how much it did not read.
//!
//! The row shape is frozen with the Mission Audit tab
//! (`review-2026-09-01/batch2/00-BATCH2.md`, "the audit row shape") so the
//! ledger, the CLI and the screen say one thing.

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use beekeeper_core::coding_session_payload::context_window_usage;
use beekeeper_core::kind::{KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA};
use beekeeper_sdk::kind::KIND_CODING_SESSION_TRANSCRIPT;

use super::crew::{build_executions, CrewExecution};
use super::{
    decode_metadata, decode_receipts, decode_transcripts, fetch_channel_events,
    filter_transcripts_by_target, rfc3339, sort_transcripts, TranscriptRecord,
};
use crate::client::BeekeeperClient;
use crate::error::CliError;
use crate::validate::validate_uuid;

/// Transcript items folded per execution before the audit stops reading.
///
/// A session's transcript is unbounded; a report is not. The cap is disclosed
/// per execution rather than raised, so a long session produces a short honest
/// answer instead of a long slow one.
pub const MAX_AUDIT_ITEMS_PER_EXECUTION: usize = 4_096;

/// Calls of one tool against one key before it counts as handed twice.
const HANDED_TWICE_MIN: u64 = 2;

/// Consecutive identical commands with identical results before it counts as a
/// retry loop.
const RETRY_LOOP_MIN: usize = 3;

/// The `bee sessions` reads a seat can make that pull the room back down into
/// its own context. Every one of them is a download the seat paid for.
const ROOM_DOWNLOAD_VERBS: &[&str] = &["status", "inbox", "send", "operation"];

/// The marker `beekeeper_session_provider::transcript::bound_text` leaves behind
/// when it clips a tool result at its 8 KiB ceiling.
const ELISION_MARKER: &str = "…[elided ";

// ── Command ─────────────────────────────────────────────────────────────────

/// `bee sessions audit` — per-turn usage and waste for one channel.
///
/// `session_ref` narrows the report to one umbrella's executions; without it
/// every execution in the channel is folded.
pub async fn cmd_audit(
    client: &BeekeeperClient,
    channel_id: &str,
    session_ref: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    if let Some(session_ref) = session_ref {
        validate_uuid(session_ref)?;
    }
    let events = fetch_channel_events(
        client,
        channel_id,
        &[
            KIND_CODING_SESSION_METADATA,
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            KIND_CODING_SESSION_TRANSCRIPT,
        ],
    )
    .await?;
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let (transcripts, _) = decode_transcripts(&events);
    // Leases are ephemeral liveness, which an audit of finished turns has no
    // use for; an empty snapshot keeps the read to one query.
    let leases = HashMap::new();
    let now = chrono::Utc::now().timestamp();
    let executions: Vec<CrewExecution> =
        build_executions(&metadata, &receipts, &transcripts, &leases, now)
            .into_iter()
            .filter(|execution| match session_ref {
                Some(wanted) => execution.session_ref.as_deref() == Some(wanted),
                None => true,
            })
            .collect();

    let report = audit_report(&executions, &transcripts);
    match format {
        crate::OutputFormat::Compact => {
            for line in compact_lines(&report) {
                println!("{line}");
            }
        }
        crate::OutputFormat::Json => println!("{report}"),
    }
    Ok(())
}

/// The lines `--format compact` prints, one JSON object each.
///
/// One row per turn — the shape a spreadsheet, an awk one-liner, or a lead
/// skimming a night's work actually wants — followed by one row per execution
/// bound. The bound rows are printed here too because a disclosure only the
/// JSON form carries is not a disclosure to the reader who uses this one; they
/// are told apart by their own keys (`itemsBound`, which no turn row has).
pub fn compact_lines(report: &Value) -> Vec<String> {
    let table = |name: &str| -> Vec<String> {
        report
            .get(name)
            .and_then(Value::as_array)
            .map(|rows| rows.iter().map(Value::to_string).collect())
            .unwrap_or_default()
    };
    let mut lines = table("turns");
    lines.extend(table("bounds"));
    lines
}

// ── The fold ────────────────────────────────────────────────────────────────

/// Fold one channel's executions and transcript into the frozen audit shape.
///
/// Pure: everything it reports comes from the records handed to it, so the
/// whole report is testable against a fixture built from real wire shapes.
pub fn audit_report(executions: &[CrewExecution], transcripts: &[TranscriptRecord]) -> Value {
    let mut turns: Vec<Value> = Vec::new();
    let mut handed: BTreeMap<(String, &'static str, String), Handled> = BTreeMap::new();
    let mut downloads: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut loops: BTreeMap<(String, String), IdenticalRun> = BTreeMap::new();
    let mut bounds: Vec<Value> = Vec::new();

    for execution in executions {
        let seat = execution.seat_label();
        let mut items = filter_transcripts_by_target(transcripts, &execution.target_key);
        sort_transcripts(&mut items);
        let read = items.len();
        let clipped = read > MAX_AUDIT_ITEMS_PER_EXECUTION;
        if clipped {
            items.truncate(MAX_AUDIT_ITEMS_PER_EXECUTION);
        }
        bounds.push(json!({
            "seat": seat,
            "executionKey": execution.target_key,
            "itemsRead": items.len(),
            "itemsPublished": read,
            "itemsBound": MAX_AUDIT_ITEMS_PER_EXECUTION,
            "itemsTruncated": clipped,
        }));

        turns.extend(execution_turns(execution, &seat, &items, clipped));
        fold_tool_traffic(&seat, &items, &mut handed, &mut downloads, &mut loops);
    }

    let handed_twice: Vec<Value> = handed
        .into_iter()
        .filter(|(_, handled)| handled.count >= HANDED_TWICE_MIN)
        .map(|((seat, what, key), handled)| {
            json!({
                "seat": seat,
                "what": what,
                "key": key,
                "count": handled.count,
                // Null rather than 0 when nothing came back: an unanswered
                // call has no byte count, and 0 would claim it returned
                // nothing.
                "bytes": (handled.results > 0).then_some(handled.bytes),
                // How much of `count` the byte total actually covers.
                "resultsSeen": handled.results,
                // The provider clips a tool result at 8 KiB, so `bytes` is what
                // was published, not what the tool produced. Said out loud
                // rather than left for a reader to discover.
                "bytesClipped": handled.clipped,
            })
        })
        .collect();
    let room_downloads: Vec<Value> = downloads
        .into_iter()
        .map(|((seat, command), count)| json!({ "seat": seat, "command": command, "count": count }))
        .collect();
    let retry_loops: Vec<Value> = loops
        .into_iter()
        .filter(|(_, run)| run.length >= RETRY_LOOP_MIN)
        .map(|((seat, command), run)| {
            json!({
                "seat": seat,
                "command": command,
                "count": run.length,
                // `true` only when every run in the loop published a result to
                // compare. Runs whose results never reached the transcript are
                // consecutive repeats of one command and nothing more — and
                // "unknown" is not "they agreed".
                "identicalResults": run.results_seen.then_some(true),
            })
        })
        .collect();

    json!({
        "turns": turns,
        "handedTwice": handed_twice,
        "roomDownloads": room_downloads,
        "retryLoops": retry_loops,
        "totals": totals(&turns),
        "bounds": bounds,
    })
}

/// One tool key's traffic for one seat.
#[derive(Default)]
struct Handled {
    count: u64,
    bytes: u64,
    /// How many of those calls published a result. `bytes` covers only these,
    /// so a row where it is smaller than `count` is a partial byte count and
    /// says so rather than reading as the whole traffic.
    results: u64,
    clipped: bool,
}

/// Fold one execution's items into its per-turn rows.
fn execution_turns(
    execution: &CrewExecution,
    seat: &str,
    items: &[TranscriptRecord],
    clipped: bool,
) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<&TranscriptRecord>> = HashMap::new();
    for item in items {
        // Items outside any turn — session init, lifecycle status rows — are
        // real, but they are not a turn and inventing one for them would put a
        // row in the table that nothing spent.
        let Some(turn_id) = item.envelope.turn_id.clone() else {
            continue;
        };
        if !grouped.contains_key(&turn_id) {
            order.push(turn_id.clone());
        }
        grouped.entry(turn_id).or_default().push(item);
    }

    order
        .into_iter()
        .filter_map(|turn_id| {
            let group = grouped.get(&turn_id)?;
            Some(turn_row(execution, seat, &turn_id, group, clipped))
        })
        .collect()
}

/// One turn's row in the frozen shape.
fn turn_row(
    execution: &CrewExecution,
    seat: &str,
    turn_id: &str,
    items: &[&TranscriptRecord],
    clipped: bool,
) -> Value {
    let started_at_ms = items.first().map(|item| item.envelope.timestamp);
    let counted_tool_calls = items
        .iter()
        .filter(|item| item_kind(item) == Some("tool_call"))
        .count() as u64;
    let result = items
        .iter()
        .find(|item| item_kind(item) == Some("result"))
        .map(|item| &item.envelope.item);
    let usage = result.and_then(|item| item.get("usage"));
    // The driver's own occupancy statement, when it made one; it is occupancy
    // rather than consumption, so it is the better source for the window.
    let driver_window = items
        .iter()
        .rev()
        .find_map(|item| context_window_usage(&item.envelope.item))
        .and_then(|occupancy| occupancy.context_window);

    json!({
        "seat": seat,
        "executionKey": execution.target_key,
        "turnId": turn_id,
        "startedAt": started_at_ms.map(|ms| rfc3339(ms / 1_000)),
        "durationMs": result.and_then(|item| number(item, "durationMs")),
        // The driver's own count when it made one, otherwise the calls this
        // turn actually published. Both are measurements of the same turn;
        // neither is a guess — but the second one is only complete when the
        // fold read the whole execution, and truncation removes exactly the
        // terminal `result` item that would have carried the driver's.
        "toolCalls": usage
            .and_then(|usage| number(usage, "toolCalls"))
            .unwrap_or(counted_tool_calls),
        // True when this row's count was taken from a stream the fold stopped
        // reading: the number is a floor, not the turn's.
        "toolCallsTruncated": clipped && usage.and_then(|usage| number(usage, "toolCalls")).is_none(),
        "inputTokens": usage.and_then(|usage| number(usage, "inputTokens")),
        "outputTokens": usage.and_then(|usage| number(usage, "outputTokens")),
        "cacheReadTokens": usage.and_then(|usage| number(usage, "cacheReadTokens")),
        "cacheWriteTokens": usage.and_then(|usage| number(usage, "cacheWriteTokens")),
        "contextWindow": usage
            .and_then(|usage| number(usage, "contextWindow"))
            .or(driver_window),
        "costUsd": published_cost(result),
        "costBasis": result.and_then(|item| item.get("costBasis")).cloned().unwrap_or(Value::Null),
        "costReason": result.and_then(|item| item.get("costReason")).cloned().unwrap_or(Value::Null),
    })
}

/// The turn's cost, exactly as the producer published it.
///
/// The `result` item's own `costUsd`
/// (`beekeeper_core::coding_session_payload::result_item`) and nothing else: this
/// command never multiplies tokens by a price list it carries. An earlier
/// version gated the number on a `pricingIdentity` key inside `usage`; that
/// key is a `beekeeper-acp` internal (`crates/beekeeper-acp/src/usage.rs`) that no
/// producer serializes into a 44225, so the gate reported `null` for every
/// turn on the wire while the item beside it carried a number (REVIEW-A1 F2).
fn published_cost(result: Option<&Value>) -> Option<f64> {
    result?.get("costUsd")?.as_f64()
}

/// Fold one execution's tool traffic into the three waste tables.
fn fold_tool_traffic(
    seat: &str,
    items: &[TranscriptRecord],
    handed: &mut BTreeMap<(String, &'static str, String), Handled>,
    downloads: &mut BTreeMap<(String, String), u64>,
    loops: &mut BTreeMap<(String, String), IdenticalRun>,
) {
    // What each open call was for, so its result can be charged to the same
    // key. A `tool_result` names its tool only sometimes; `toolId` always.
    let mut open: HashMap<String, Vec<(&'static str, String)>> = HashMap::new();
    let mut commands: HashMap<String, String> = HashMap::new();
    // The seat's ordered (command, result) stream, for the retry-loop scan.
    let mut ran: Vec<(String, Option<String>)> = Vec::new();

    for item in items {
        match item_kind(item) {
            Some("tool_call") => {
                let tool = item.envelope.item.get("tool");
                let id = tool
                    .and_then(|tool| tool.get("toolId"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let keys = handled_keys(tool);
                for (what, key) in &keys {
                    handed
                        .entry((seat.to_owned(), what, key.clone()))
                        .or_default()
                        .count += 1;
                }
                if let Some(command) = tool
                    .and_then(|tool| tool.get("input"))
                    .and_then(|input| input.get("command"))
                    .and_then(Value::as_str)
                {
                    let command = command.trim().to_owned();
                    if let Some(verb) = room_download_verb(&command) {
                        *downloads
                            .entry((seat.to_owned(), format!("sessions {verb}")))
                            .or_default() += 1;
                    }
                    ran.push((command.clone(), None));
                    commands.insert(id.clone(), command);
                }
                if !id.is_empty() {
                    open.insert(id, keys);
                }
            }
            Some("tool_result") => {
                let item = &item.envelope.item;
                let id = item
                    .get("toolId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let content = item.get("content").and_then(Value::as_str).unwrap_or("");
                let bytes = content.len() as u64;
                let clipped = content.contains(ELISION_MARKER);
                if let Some(keys) = open.remove(&id) {
                    for (what, key) in keys {
                        let entry = handed.entry((seat.to_owned(), what, key)).or_default();
                        entry.bytes = entry.bytes.saturating_add(bytes);
                        entry.results = entry.results.saturating_add(1);
                        entry.clipped |= clipped;
                    }
                }
                if let Some(command) = commands.remove(&id) {
                    // Charge the result to the newest run of that exact
                    // command that is still waiting for one.
                    if let Some(slot) = ran
                        .iter_mut()
                        .rev()
                        .find(|(ran, result)| ran == &command && result.is_none())
                    {
                        slot.1 = Some(content.to_owned());
                    }
                }
            }
            _ => {}
        }
    }

    for (command, run) in longest_identical_runs(&ran) {
        let entry = loops.entry((seat.to_owned(), command)).or_default();
        if run.length > entry.length {
            *entry = run;
        }
    }
}

/// The longest run of one command, and whether its results were ever seen.
#[derive(Clone, Copy, Default)]
struct IdenticalRun {
    /// How many consecutive times the command ran with the same result.
    length: usize,
    /// Whether that result was published at all. A run of calls whose results
    /// never reached the transcript agrees about nothing.
    results_seen: bool,
}

/// The longest run of consecutive identical commands with identical results,
/// per command.
///
/// Consecutive is over the seat's own ordered tool stream, so a command
/// separated by other runs of itself with a different result is measured as
/// the longest single run, not the total.
///
/// A run's members share one result by construction, so `results_seen` is a
/// property of the run: `false` means the transcript carries no result for any
/// of them, and the caller reports agreement as unknown rather than as `true`.
fn longest_identical_runs(ran: &[(String, Option<String>)]) -> BTreeMap<String, IdenticalRun> {
    let mut longest: BTreeMap<String, IdenticalRun> = BTreeMap::new();
    let mut index = 0;
    while index < ran.len() {
        let (command, result) = &ran[index];
        let mut end = index + 1;
        while end < ran.len() && &ran[end].0 == command && &ran[end].1 == result {
            end += 1;
        }
        let run = IdenticalRun {
            length: end - index,
            results_seen: result.is_some(),
        };
        let entry = longest.entry(command.clone()).or_default();
        if run.length > entry.length {
            *entry = run;
        }
        index = end;
    }
    longest
}

/// Every `(what, key)` one open tool call handed the agent.
///
/// Edit-shaped tools publish the canonical `edit.paths` list, which is the
/// truth about which files a call touched; everything else falls back to the
/// argument names the adapters actually send.
fn handled_keys(tool: Option<&Value>) -> Vec<(&'static str, String)> {
    let mut keys: Vec<(&'static str, String)> = Vec::new();
    let mut push_path = |path: &str| {
        let path = path.trim();
        if !path.is_empty() && !keys.iter().any(|(_, kept)| kept == path) {
            keys.push(("path", path.to_owned()));
        }
    };
    if let Some(paths) = tool
        .and_then(|tool| tool.get("edit"))
        .and_then(|edit| edit.get("paths"))
        .and_then(Value::as_array)
    {
        for path in paths.iter().filter_map(Value::as_str) {
            push_path(path);
        }
    }
    let input = tool.and_then(|tool| tool.get("input"));
    if let Some(input) = input {
        for key in ["path", "file_path", "filePath", "abs_path", "absPath"] {
            if let Some(path) = input.get(key).and_then(Value::as_str) {
                push_path(path);
            }
        }
        if let Some(command) = input.get("command").and_then(Value::as_str) {
            let command = command.trim();
            if !command.is_empty() {
                keys.push(("command", command.to_owned()));
            }
        }
    }
    keys
}

/// The `bee sessions <verb>` a shell command runs, when it runs one of the
/// four that pull the room into the seat's context.
///
/// Modelled on the desktop classifier's own scan
/// (`desktop/src/features/agents/ui/agentSessionToolClassifier.ts`,
/// `findBeekeeperCommand`): find the executable, skip its flags — and the values
/// they take — then read the group and its verb.
///
/// The executable is only recognized in **command position**: the start of the
/// line, after a `&&`/`||`/`|`/`;`, or after the environment assignments that
/// precede a command. `echo bee sessions status` mentions the CLI; it does not
/// run it, and counting it made a measured number partly a word count
/// (REVIEW-A1 F9).
fn room_download_verb(command: &str) -> Option<&'static str> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let mut command_position = true;
    for (index, token) in tokens.iter().enumerate() {
        if is_separator(token) {
            command_position = true;
            continue;
        }
        if !command_position {
            continue;
        }
        // `FOO=bar bee …` still runs bee; the assignments come first and the
        // command position survives them.
        if is_env_assignment(token) {
            continue;
        }
        command_position = false;
        let executable = token.rsplit('/').next().unwrap_or(token);
        if executable != "bee" && executable != "buzz" {
            continue;
        }
        let mut cursor = index + 1;
        while cursor < tokens.len() {
            let token = tokens[cursor];
            if is_separator(token) {
                break;
            }
            if token.starts_with('-') {
                // A long flag with a separate value consumes the next token,
                // so `--format compact sessions status` is still `sessions`.
                if !token.contains('=')
                    && tokens
                        .get(cursor + 1)
                        .is_some_and(|next| !next.starts_with('-'))
                {
                    cursor += 1;
                }
                cursor += 1;
                continue;
            }
            if token == "sessions" {
                let verb = tokens.get(cursor + 1)?;
                return ROOM_DOWNLOAD_VERBS
                    .iter()
                    .find(|known| *known == verb)
                    .copied();
            }
            break;
        }
    }
    None
}

/// Whether a token ends one command and starts the next.
fn is_separator(token: &str) -> bool {
    matches!(token, "&&" | "||" | "|" | ";" | "(" | "{")
}

/// Whether a token is a `NAME=value` prefix rather than the command itself.
fn is_env_assignment(token: &str) -> bool {
    token
        .split_once('=')
        .is_some_and(|(name, _)| !name.is_empty() && !name.contains('/'))
}

/// Per-seat and whole-session totals over the turn rows.
///
/// A total is `null` when no turn reported the number at all — summing absent
/// measurements into `0` would claim the team spent nothing.
fn totals(turns: &[Value]) -> Value {
    const SUMMED: &[&str] = &[
        "toolCalls",
        "inputTokens",
        "outputTokens",
        "cacheReadTokens",
        "cacheWriteTokens",
        "durationMs",
    ];
    let mut by_seat: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for turn in turns {
        let seat = turn
            .get("seat")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        by_seat.entry(seat).or_default().push(turn);
    }
    let sum = |rows: &[&Value]| -> Value {
        let mut object = serde_json::Map::new();
        object.insert("turns".into(), json!(rows.len()));
        for field in SUMMED {
            object.insert(
                (*field).to_owned(),
                sum_reported(rows, field).map_or(Value::Null, |total| json!(total)),
            );
        }
        object.insert(
            "costUsd".into(),
            sum_cost(rows).map_or(Value::Null, |total| json!(total)),
        );
        // A total that summed even one partial count is itself a floor. The
        // flag travels with the number rather than being left in a sibling
        // table the reader may not have open.
        object.insert(
            "toolCallsTruncated".into(),
            json!(rows
                .iter()
                .any(|row| row.get("toolCallsTruncated") == Some(&Value::Bool(true)))),
        );
        Value::Object(object)
    };
    let seats: Vec<Value> = by_seat
        .iter()
        .map(|(seat, rows)| {
            let mut row = sum(rows);
            if let Some(object) = row.as_object_mut() {
                object.insert("seat".into(), json!(seat));
            }
            row
        })
        .collect();
    let all: Vec<&Value> = turns.iter().collect();
    json!({ "bySeat": seats, "session": sum(&all) })
}

/// Sum one field across rows, or `None` when no row reported it.
fn sum_reported(rows: &[&Value], field: &str) -> Option<u64> {
    let mut total: Option<u64> = None;
    for row in rows {
        if let Some(value) = row.get(field).and_then(Value::as_u64) {
            total = Some(total.unwrap_or(0).saturating_add(value));
        }
    }
    total
}

/// Sum the published costs, or `None` when no turn published one.
///
/// There is no pricing identity on the wire — `TurnUsageReport`
/// (`beekeeper-core/src/coding_session_payload.rs`) is `deny_unknown_fields` over
/// six token fields — so the only honest source is the item's own
/// `costUsd`/`cost_usd`. A turn that published none contributes nothing, and
/// a session where none did reports `None`, never `0.0`.
fn sum_cost(rows: &[&Value]) -> Option<f64> {
    let mut total: Option<f64> = None;
    for row in rows {
        if let Some(value) = row.get("costUsd").and_then(Value::as_f64) {
            total = Some(total.unwrap_or(0.0) + value);
        }
    }
    total
}

fn item_kind(item: &TranscriptRecord) -> Option<&str> {
    item.envelope.item.get("kind").and_then(Value::as_str)
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}
