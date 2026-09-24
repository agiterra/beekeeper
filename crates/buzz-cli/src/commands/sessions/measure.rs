//! `bee sessions measure` — the scorecard a model used to produce by hand.
//!
//! On 2026-09-20 two audits of two team sessions were written by a model
//! reading relay events one at a time
//! (`plans/archive/2026-09-20-astra-kettle-audit.md` and
//! `plans/archive/2026-09-20-astra-andy-rpg-audit.md`). Both cost hours and
//! millions of tokens, and neither is repeatable. This command reads the same
//! events and prints the same tables.
//!
//! Every number here is derived from signed relay records, and the derivation
//! is named beside it:
//!
//! - **Tokens, cost, duration and tool calls come only from terminal `result`
//!   rows of kind 44225.** An open turn's consumption is not on the wire, so
//!   it is counted as an open turn and nothing else. Top-level `inputTokens`
//!   is cache-inclusive; `usage.inputTokens` / `cacheReadTokens` /
//!   `cacheWriteTokens` are the disjoint split and are never added to it.
//! - **A partial sum is never presented as a total.** `costUsd` is omitted by
//!   some drivers on some turns, so every cost carries its coverage — how many
//!   of the seat's results were priced. No price table is consulted; a cost
//!   this binary computed would be an invention wearing a number's clothes.
//! - **Absence is disclosed, not rounded to zero.** A metric the wire cannot
//!   support is printed as `unknown` with the reason, in the `honesty` block.
//! - **A read that fails is an error.** The fetch layer propagates a p-gated or
//!   refused query rather than folding an empty result into a clean-looking
//!   report.
//!
//! The arithmetic is pure functions over a vector of events
//! ([`measure_report`]); the relay is touched only by [`cmd_measure`]. The
//! oracle tests in `measure_tests.rs` run the pure path against frozen
//! captures of both audited sessions.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use buzz_core::kind::{KIND_APPROVAL_DENY, KIND_APPROVAL_GRANT};
use buzz_core::kind::{
    KIND_CODING_SESSION_CLOSURE, KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_GOAL, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_NAME,
    KIND_CODING_SESSION_TEAM_TRANSACTION, KIND_CODING_SESSION_TRANSCRIPT, KIND_GIT_REPO_STATE,
    KIND_HOST_STEP_CLAIM, KIND_HOST_STEP_RESULT, KIND_WORKFLOW_APPROVAL_REQUESTED,
    KIND_WORKFLOW_HOST_STEP_REQUESTED, KIND_WORKFLOW_TRIGGER,
};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::validate_uuid;

// ── Kind sets ───────────────────────────────────────────────────────────────

/// Kinds read from the session's channel inside the requested window.
///
/// Each is an event the measured session writes while it runs: its commands,
/// its lifecycle receipts, its per-execution metadata, its transcript, and its
/// typed team transactions.
pub const MEASURE_SESSION_KINDS: &[u32] = &[
    KIND_CODING_SESSION_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_TRANSCRIPT,
    KIND_CODING_SESSION_TEAM_TRANSACTION,
];

/// Kinds that identify the umbrella itself, read **without** a lower bound.
///
/// The goal, the genesis and the name are published *before* the first turn —
/// the kettle run's goal is three minutes older than the window its audit used
/// — so applying `--since` to them would make a tool report `unknown` for the
/// very timestamp it exists to print.
pub const MEASURE_IDENTITY_KINDS: &[u32] = &[
    KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_GOAL,
    KIND_CODING_SESSION_NAME,
    KIND_CODING_SESSION_CLOSURE,
];

/// The action chain: trigger, approval request, grant or denial, host request,
/// claim, result.
pub const MEASURE_ACTION_KINDS: &[u32] = &[
    KIND_WORKFLOW_TRIGGER,
    KIND_WORKFLOW_APPROVAL_REQUESTED,
    KIND_APPROVAL_GRANT,
    KIND_APPROVAL_DENY,
    KIND_WORKFLOW_HOST_STEP_REQUESTED,
    KIND_HOST_STEP_CLAIM,
    KIND_HOST_STEP_RESULT,
];

/// Substrings whose presence in a tool call or its result marks the call as
/// orientation — the seat learning its own interface rather than doing work.
///
/// Held as data, not as judgement: the wire carries no orientation label, so
/// the report prints the detector alongside the count.
const ORIENTATION_MARKERS: &[&str] = &[
    "--help",
    "Usage:",
    "--example",
    "unknown word",
    "missing field",
];

/// Path fragments that mark a read of a role or skill file as orientation.
const ORIENTATION_PATHS: &[&str] = &["roles/", "skills/", "SKILL.md", "AGENTS.md", "personas/"];

/// The CLI's own machine-readable class for "you called me wrong".
const USER_ERROR_TOKEN: &str = "\"user_error\"";

/// The relay's own machine-readable class for a refused write or read.
const RELAY_ERROR_TOKEN: &str = "\"relay_error\"";

/// Prefix the CLI puts on a command id when one seat wakes another by naming
/// the exact record that caused the wake: `cli-wake-v1:<event id>:<suffix>`.
///
/// This is the only link on the wire from a delivered turn back to the
/// transaction that provoked it — 44244's own `deliveryCommandId` was null on
/// every record of both audited sessions.
const WAKE_COMMAND_PREFIX: &str = "cli-wake-v1:";

// ── Command ─────────────────────────────────────────────────────────────────

/// `bee sessions measure` — read one team session and print its scorecard.
///
/// Read-only: four queries, no writes, no provider contact. `session_ref`
/// narrows every per-seat and coordination number to one umbrella; without it
/// the whole channel is folded and the report says so.
pub async fn cmd_measure(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    if let Some(session_ref) = session_ref {
        validate_uuid(session_ref)?;
    }
    let since = since.map(|raw| parse_time("--since", raw)).transpose()?;
    let until = until.map(|raw| parse_time("--until", raw)).transpose()?;
    if let (Some(since), Some(until)) = (since, until) {
        if until < since {
            return Err(CliError::Usage(format!(
                "--until ({until}) is before --since ({since})"
            )));
        }
    }

    let mut events = fetch_window(client, channel_id, MEASURE_SESSION_KINDS, since, until).await?;
    events.extend(fetch_window(client, channel_id, MEASURE_IDENTITY_KINDS, None, until).await?);
    events.extend(fetch_window(client, channel_id, MEASURE_ACTION_KINDS, since, until).await?);

    // An approval the person signed by hand carries no `h` tag (ledger 197),
    // so the channel-scoped reads above can never return one. It is reachable
    // only by author and kind over the same window. A refusal here is not
    // fatal: the rest of the report stands, and `measure_report` discloses
    // the absence rather than reporting zero approvals as a fact.
    if let Some(owner) = founder_key(&events) {
        let mut filter = json!({
            "kinds": [KIND_APPROVAL_GRANT, KIND_APPROVAL_DENY],
            "authors": [owner],
        });
        if let Some(since) = since {
            filter["since"] = json!(since);
        }
        if let Some(until) = until {
            filter["until"] = json!(until);
        }
        match client.query_all(filter).await {
            Ok(approvals) => events.extend(approvals),
            Err(error) => eprintln!(
                "note: the founder's approvals (46030/46031) could not be read by author and \
                 kind, so none is counted: {error}"
            ),
        }
    }

    // Git ref state is repo-scoped by its own `d` tag, never `h`-scoped, so it
    // needs a second filter keyed on the repository names the session's own
    // metadata records. Without one the landed sha has no observed delivery.
    for repo in repo_names(&events) {
        let mut filter = json!({ "kinds": [KIND_GIT_REPO_STATE], "#d": [repo] });
        if let Some(until) = until {
            filter["until"] = json!(until);
        }
        events.extend(client.query_all(filter).await?);
    }

    // The author-scoped approval read overlaps the channel-scoped action read
    // for any grant that *does* carry an `h` tag, so the same event can arrive
    // twice and would be folded twice.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    events.retain(|event| match event.get("id").and_then(Value::as_str) {
        Some(id) => seen.insert(id.to_owned()),
        None => true,
    });

    if let Some(wanted) = session_ref {
        check_session_ref_known(&events, channel_id, wanted)?;
    }
    let report = measure_report(&events, session_ref, since, until);
    match format {
        crate::OutputFormat::Compact => {
            for line in table_lines(&report) {
                println!("{line}");
            }
        }
        crate::OutputFormat::Json => println!("{report}"),
    }
    Ok(())
}

/// Parse `--since`/`--until`: RFC 3339 first, then Unix seconds.
///
/// The same two spellings `bee events query` takes, so a window copied from
/// one command into the other means the same thing in both.
fn parse_time(flag: &str, raw: &str) -> Result<i64, CliError> {
    if let Ok(unix) = raw.trim().parse::<i64>() {
        return Ok(unix);
    }
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map(|time| time.timestamp())
        .map_err(|error| {
            CliError::Usage(format!(
                "{flag} must be an RFC 3339 timestamp or Unix seconds: {raw} ({error})"
            ))
        })
}

/// One bounded, fully paged query. A refusal is returned, never swallowed.
async fn fetch_window(
    client: &BuzzClient,
    channel_id: &str,
    kinds: &[u32],
    since: Option<i64>,
    until: Option<i64>,
) -> Result<Vec<Value>, CliError> {
    let mut filter = json!({ "kinds": kinds, "#h": [channel_id] });
    if let Some(since) = since {
        filter["since"] = json!(since);
    }
    if let Some(until) = until {
        filter["until"] = json!(until);
    }
    client.query_all(filter).await
}

/// The founder key a `projectRef` names, read off raw events.
///
/// The same derivation [`project_owner`] makes over parsed events, needed
/// once before parsing so the fetch layer can ask for that key's approvals.
fn founder_key(events: &[Value]) -> Option<String> {
    events.iter().find_map(|event| {
        if event.get("kind").and_then(Value::as_u64)? != u64::from(KIND_CODING_SESSION_METADATA) {
            return None;
        }
        let content = event.get("content").and_then(Value::as_str)?;
        let parsed: Value = serde_json::from_str(content).ok()?;
        parsed
            .get("projectRef")?
            .as_str()?
            .split(':')
            .nth(1)
            .map(str::to_owned)
    })
}

/// Repository names named by any 44223 `repoRef` or `projectRef`.
fn repo_names(events: &[Value]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_METADATA))
        {
            continue;
        }
        let Some(content) = event.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        for key in ["repoRef", "projectRef"] {
            if let Some(addr) = parsed.get(key).and_then(Value::as_str) {
                if let Some(name) = addr.rsplit(':').next() {
                    if !name.is_empty() {
                        names.insert(name.to_owned());
                    }
                }
            }
        }
    }
    names
}

// ── Parsed events ───────────────────────────────────────────────────────────

/// One relay event with its content already decoded, or `Value::Null` when it
/// is not JSON (30618 and 30620 carry no JSON body).
struct Ev {
    id: String,
    pubkey: String,
    kind: u32,
    created_at: i64,
    tags: Vec<Vec<String>>,
    content: Value,
    /// The raw content string, kept for the kinds whose body is not JSON.
    raw_content: String,
}

impl Ev {
    fn tag(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|tag| tag.first().map(String::as_str) == Some(name))
            .and_then(|tag| tag.get(1))
            .map(String::as_str)
    }
}

fn parse_events(events: &[Value]) -> (Vec<Ev>, usize) {
    let mut parsed = Vec::with_capacity(events.len());
    let mut malformed = 0usize;
    for event in events {
        let (Some(id), Some(kind), Some(created_at)) = (
            event.get("id").and_then(Value::as_str),
            event.get("kind").and_then(Value::as_u64),
            event.get("created_at").and_then(Value::as_i64),
        ) else {
            malformed += 1;
            continue;
        };
        let raw_content = event
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let tags = event
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(Value::as_array)
                    .map(|tag| {
                        tag.iter()
                            .map(|part| part.as_str().unwrap_or_default().to_owned())
                            .collect()
                    })
                    .collect()
            })
            .unwrap_or_default();
        parsed.push(Ev {
            id: id.to_owned(),
            pubkey: event
                .get("pubkey")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            kind: kind as u32,
            created_at,
            tags,
            content: serde_json::from_str(&raw_content).unwrap_or(Value::Null),
            raw_content,
        });
    }
    parsed.sort_by_key(|event| event.created_at);
    parsed.dedup_by(|a, b| a.id == b.id);
    (parsed, malformed)
}

// ── Seat model ──────────────────────────────────────────────────────────────

/// One transcript item, flattened to the fields the arithmetic reads.
struct Item<'a> {
    session_id: &'a str,
    turn_id: Option<&'a str>,
    /// Millisecond timestamp from the transcript envelope.
    at_ms: i64,
    kind: &'a str,
    item: &'a Value,
}

/// A seat's per-turn span: when its first item landed and when its terminal
/// `result` landed (or the cutoff, for a turn still open).
struct Turn {
    start_ms: i64,
    end_ms: i64,
    completed: bool,
    /// Type of the 44244 record whose `cli-wake-v1` command opened this turn.
    woken_by: Option<String>,
    /// Types of the 44244 records this seat wrote while the turn was open.
    writes: Vec<String>,
}

#[derive(Default)]
struct SeatAgg {
    role: Option<String>,
    actor: Option<String>,
    runtime: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    generation: Option<i64>,
    session_ref: Option<String>,
    metadata_at: i64,
}

/// Refuse a `--session-ref` no 44223 in the fetched window claims.
///
/// `measure_report` selects seats by the umbrella `sessionRef` their metadata
/// records, so a ref nothing records yields a report with no seats and zero
/// turns — indistinguishable from an idle session. Control run 4 (ledger
/// 255(d)) lost time to exactly that, passing the `sessionId` embedded in a
/// `sessions list` target. The refusal names the id kind accepted and, when
/// the value is an execution's `sessionId`, the umbrella that execution
/// actually belongs to.
pub fn check_session_ref_known(
    events: &[Value],
    channel_id: &str,
    wanted: &str,
) -> Result<(), CliError> {
    let metadata = || {
        events.iter().filter(|event| {
            event.get("kind").and_then(Value::as_u64)
                == Some(u64::from(KIND_CODING_SESSION_METADATA))
        })
    };
    let content = |event: &Value| -> Option<Value> {
        event
            .get("content")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str(raw).ok())
    };
    if metadata()
        .filter_map(content)
        .any(|payload| str_at(&payload, "sessionRef").as_deref() == Some(wanted))
    {
        return Ok(());
    }
    let owner = metadata().filter_map(content).find_map(|payload| {
        let session_id = payload.pointer("/session/sessionId")?.as_str()?;
        (session_id == wanted).then(|| str_at(&payload, "sessionRef"))
    });
    let hint = match owner {
        Some(Some(umbrella)) => format!(
            " {wanted} is an execution's sessionId (the uuid inside a `sessions list` target); \
             its umbrella sessionRef is {umbrella}."
        ),
        Some(None) => format!(
            " {wanted} is an execution's sessionId whose metadata names no umbrella \
             sessionRef; omit --session-ref and scope by --since/--until instead."
        ),
        None => String::new(),
    };
    Err(CliError::Usage(format!(
        "no coding session in channel {channel_id} records sessionRef {wanted} in the measured \
         window, so the report would be empty. --session-ref takes an umbrella sessionRef — \
         the `sessionRef` field `bee sessions list` prints, the id `sessions work status`, \
         `sessions operation list` and `sessions decide answer` take.{hint}"
    )))
}

/// Build the report. Pure: every number below comes from `events` alone.
///
/// `session_ref` selects one umbrella by the `sessionRef` its 44223 metadata
/// records. `since` and `until` are reported verbatim so a reader can tell a
/// window from a fact; `until` also fixes the cutoff every open-turn and
/// waiting number is measured against, and when it is absent the cutoff is the
/// last terminal `result` on the wire.
pub fn measure_report(
    events: &[Value],
    session_ref: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Value {
    let (mut evs, malformed) = parse_events(events);
    // Mirror the fetch layer's bounds exactly, so the pure path folds the same
    // set the live path would have been handed. `since` is not applied to the
    // umbrella's identity kinds or to git ref state, because neither is queried
    // with a lower bound: the goal precedes the first turn, and a ref state is
    // repo-scoped rather than window-scoped.
    evs.retain(|ev| {
        let above = since.is_none_or(|since| {
            ev.created_at >= since
                || MEASURE_IDENTITY_KINDS.contains(&ev.kind)
                || ev.kind == KIND_GIT_REPO_STATE
        });
        let below = until.is_none_or(|until| ev.created_at <= until);
        above && below
    });
    let mut honesty: Vec<Value> = Vec::new();

    // ── seats, from 44223 ───────────────────────────────────────────────────
    let mut seats: BTreeMap<String, SeatAgg> = BTreeMap::new();
    for ev in evs
        .iter()
        .filter(|e| e.kind == KIND_CODING_SESSION_METADATA)
    {
        let Some(session_id) = ev
            .content
            .pointer("/session/sessionId")
            .and_then(Value::as_str)
        else {
            continue;
        };
        let entry = seats.entry(session_id.to_owned()).or_default();
        if ev.created_at < entry.metadata_at {
            continue;
        }
        entry.metadata_at = ev.created_at;
        entry.role = str_at(&ev.content, "role");
        entry.actor = str_at(&ev.content, "agentRef");
        entry.runtime = str_at(&ev.content, "runtime");
        entry.model = str_at(&ev.content, "model");
        entry.provider = str_at(&ev.content, "provider");
        entry.session_ref = str_at(&ev.content, "sessionRef");
        entry.generation = ev
            .content
            .pointer("/session/generation")
            .and_then(Value::as_i64);
    }

    let selected: BTreeSet<String> = seats
        .iter()
        .filter(|(_, seat)| match session_ref {
            Some(wanted) => seat.session_ref.as_deref() == Some(wanted),
            None => true,
        })
        .map(|(id, _)| id.clone())
        .collect();

    // ── transcript items ────────────────────────────────────────────────────
    let mut items: Vec<Item> = Vec::new();
    let mut unresolved: BTreeSet<&str> = BTreeSet::new();
    for ev in evs
        .iter()
        .filter(|e| e.kind == KIND_CODING_SESSION_TRANSCRIPT)
    {
        let Some(session_id) = ev
            .content
            .pointer("/session/sessionId")
            .and_then(Value::as_str)
        else {
            continue;
        };
        if !selected.contains(session_id) {
            if !seats.contains_key(session_id) {
                unresolved.insert(session_id);
            }
            continue;
        }
        let Some(item) = ev.content.get("item") else {
            continue;
        };
        let Some(kind) = item.get("kind").and_then(Value::as_str) else {
            continue;
        };
        items.push(Item {
            session_id,
            turn_id: ev.content.get("turnId").and_then(Value::as_str),
            at_ms: ev
                .content
                .get("timestamp")
                .and_then(Value::as_i64)
                .unwrap_or(ev.created_at * 1_000),
            kind,
            item,
        });
    }
    items.sort_by_key(|item| item.at_ms);
    if !unresolved.is_empty() {
        honesty.push(json!({
            "metric": "seat identity for some executions",
            "value": "unknown",
            "reason": format!(
                "{} execution(s) have transcript rows in this window but no kind 44223 metadata, \
                 so their role, runtime, model and umbrella cannot be resolved; they are excluded",
                unresolved.len()
            ),
        }));
    }

    // The cutoff every open-turn and waiting figure is measured against: the
    // last instant this session was observed inside the window. `--until` is a
    // second, and a transcript item is a millisecond, so taking the later of
    // the two is what stops an open turn from appearing to run past the end of
    // the window it was read in — the 167 ms that would otherwise turn the
    // kettle verifier's 51.915 s of waiting into 51.7.
    let last_item_ms = items.iter().map(|item| item.at_ms).max();
    let cutoff_ms = match (until.map(|until| until * 1_000), last_item_ms) {
        (Some(until), Some(last)) => Some(until.max(last)),
        (Some(until), None) => Some(until),
        (None, last) => last,
    };

    // ── 44244 transactions ──────────────────────────────────────────────────
    let mut tx_by_id: BTreeMap<&str, &str> = BTreeMap::new();
    let mut tx_rows: Vec<(&Ev, &str)> = Vec::new();
    for ev in evs
        .iter()
        .filter(|e| e.kind == KIND_CODING_SESSION_TEAM_TRANSACTION)
    {
        if let (Some(wanted), Some(actual)) = (
            session_ref,
            ev.content.get("sessionRef").and_then(Value::as_str),
        ) {
            if wanted != actual {
                continue;
            }
        }
        let ty = ev
            .content
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| ev.tag("cstx-type"))
            .unwrap_or("unknown");
        tx_by_id.insert(ev.id.as_str(), ty);
        tx_rows.push((ev, ty));
    }

    // ── turns ───────────────────────────────────────────────────────────────
    let mut turns: BTreeMap<(&str, &str), Turn> = BTreeMap::new();
    for item in &items {
        let Some(turn_id) = item.turn_id else {
            continue;
        };
        let turn = turns.entry((item.session_id, turn_id)).or_insert(Turn {
            start_ms: item.at_ms,
            end_ms: item.at_ms,
            completed: false,
            woken_by: None,
            writes: Vec::new(),
        });
        turn.start_ms = turn.start_ms.min(item.at_ms);
        if item.kind == "result" {
            turn.completed = true;
            turn.end_ms = turn.end_ms.max(item.at_ms);
        } else if !turn.completed {
            turn.end_ms = turn.end_ms.max(item.at_ms);
        }
        if item.kind == "user_prompt" {
            if let Some(command_id) = item.item.get("commandId").and_then(Value::as_str) {
                if let Some(source) = command_id.strip_prefix(WAKE_COMMAND_PREFIX) {
                    let event_id = source.split(':').next().unwrap_or_default();
                    turn.woken_by = tx_by_id.get(event_id).map(|ty| (*ty).to_owned());
                }
            }
        }
    }
    // An open turn runs to the cutoff; that is what makes the waiting figure a
    // measurement of the seat rather than of the transcript's last line.
    if let Some(cutoff_ms) = cutoff_ms {
        for turn in turns.values_mut() {
            if !turn.completed {
                turn.end_ms = turn.end_ms.max(cutoff_ms);
            }
        }
    }
    // Attribute each seat's own 44244 writes to the turn they landed inside.
    for (ev, ty) in &tx_rows {
        for ((session_id, _), turn) in turns.iter_mut() {
            let actor = seats.get(*session_id).and_then(|s| s.actor.as_deref());
            if actor != Some(ev.pubkey.as_str()) {
                continue;
            }
            let at_ms = ev.created_at * 1_000;
            if at_ms >= turn.start_ms - 999 && at_ms <= turn.end_ms + 999 {
                turn.writes.push((*ty).to_owned());
            }
        }
    }

    // ── per-seat fold ───────────────────────────────────────────────────────
    let mut seat_rows: Vec<Value> = Vec::new();
    let mut totals = Totals::default();
    for session_id in &selected {
        let seat = seats.get(session_id);
        let row = seat_row(
            session_id,
            seat,
            &items,
            &turns,
            &evs,
            cutoff_ms,
            &mut totals,
        );
        seat_rows.push(row);
    }

    // ── coordination ────────────────────────────────────────────────────────
    let mut ack_only = 0usize;
    let mut verdict_woken = 0usize;
    let mut disposition_or_ack = 0usize;
    for turn in turns.values() {
        let is_ack_only = !turn.writes.is_empty()
            && turn
                .writes
                .iter()
                .all(|ty| ty == "acknowledgement" || ty == "acknowledgment");
        let is_verdict_woken = turn.woken_by.as_deref() == Some("verdict");
        if is_ack_only {
            ack_only += 1;
        }
        if is_verdict_woken {
            verdict_woken += 1;
        }
        if is_ack_only || is_verdict_woken {
            disposition_or_ack += 1;
        }
    }

    let owner = project_owner(&evs);
    let mut attribution = FounderActions::default();
    if let Some(owner) = owner.as_deref() {
        attribution = attribute_founder_actions(&evs, owner, session_ref, &selected);
        if evs
            .iter()
            .all(|ev| ev.kind != KIND_APPROVAL_GRANT && ev.kind != KIND_APPROVAL_DENY)
        {
            honesty.push(json!({
                "metric": "approvals the person signed",
                "value": "unknown",
                "reason": "no kind 46030/46031 is in the input. A hand-signed grant carries no \
                           `h` tag (ledger 197), so it is reachable only by an author+kind read \
                           over the window; none was returned or none was made, and the wire \
                           cannot tell those apart",
            }));
        }
    } else {
        honesty.push(json!({
            "metric": "person and host actions under the founder key",
            "value": "unknown",
            "reason": "no 44223 metadata names a projectRef, so the founder key this session \
                       answers to cannot be derived from the wire",
        }));
    }
    let rulings_answered = tx_rows
        .iter()
        .filter(|(_, ty)| *ty == "decision.answer")
        .count();
    let rulings_opened = tx_rows
        .iter()
        .filter(|(_, ty)| *ty == "decision.request")
        .count();

    // ── timeline ────────────────────────────────────────────────────────────
    let timeline = build_timeline(&evs, &items, &tx_rows, session_ref);
    let gross = gross_duration(&timeline);

    // ── honesty ─────────────────────────────────────────────────────────────
    if malformed > 0 {
        honesty.push(json!({
            "metric": "event decoding",
            "value": "partial",
            "reason": format!("{malformed} event(s) had no id, kind or created_at and were dropped"),
        }));
    }
    if totals.open_turns > 0 {
        honesty.push(json!({
            "metric": "tokens and cost of open turns",
            "value": "unknown",
            "reason": format!(
                "{} turn(s) have no terminal result row; the wire carries no partial usage, \
                 so their tokens, duration, tool calls and cost are not counted anywhere",
                totals.open_turns
            ),
        }));
    }
    if totals.results_priced < totals.results_total {
        honesty.push(json!({
            "metric": "dollar cost",
            "value": "partial",
            "reason": format!(
                "{} of {} terminal results carry costUsd; the reported sum is the sum of the \
                 available fields, not this run's spend. No price table was consulted",
                totals.results_priced, totals.results_total
            ),
        }));
    }
    if evs.iter().all(|e| e.kind != KIND_HOST_STEP_RESULT) {
        honesty.push(json!({
            "metric": "action trigger → result chain",
            "value": "unknown",
            "reason": "no kind 46023 host-step result is in the window; either no action ran or \
                       its result is outside the bounds given",
        }));
    }
    if evs.iter().all(|e| e.kind != KIND_GIT_REPO_STATE) {
        honesty.push(json!({
            "metric": "observed delivery of a landed sha",
            "value": "unknown",
            "reason": "no relay-signed kind 30618 ref state was returned; 30618 is repo-scoped by \
                       its `d` tag, so a session whose metadata names no repository has none",
        }));
    }
    if items.iter().all(|item| {
        item.item
            .get("hostAnswer")
            .and_then(Value::as_bool)
            .is_none()
    }) {
        honesty.push(json!({
            "metric": "prompts that are host notices",
            "value": "unknown",
            "reason": "no transcript row carries `hostAnswer`; on a build older than ledger 178(b) \
                       this computer's own notices are indistinguishable on the wire from a \
                       person typing, and are counted as person prompts",
        }));
    }
    honesty.push(json!({
        "metric": "orientation, polling and waiting classification",
        "value": "detector",
        "reason": "the wire carries no orientation, polling or waiting label. Orientation is a \
                   text match on the tool call and its result; polling is repeated identical \
                   calls inside one turn with a sleep between; waiting is observed no-open-turn \
                   wall time. Each is a detector, not a fact the producer asserted",
    }));
    honesty.push(json!({
        "metric": "publication refusals",
        "value": "detector",
        "reason": "a refusal is not typed on the wire as `publication`; relay refusals are \
                   counted as the `relay_error` class and hire refusals by their HIRE_* code",
    }));

    json!({
        "schema": "buzz-sessions-measure/v1",
        "channel_scope": {
            "session_ref": session_ref,
            "since": since.map(rfc3339),
            "until": until.map(rfc3339),
            "cutoff": cutoff_ms.map(|ms| rfc3339(ms / 1_000)),
            "events_read": evs.len(),
            "executions_selected": selected.len(),
            "truncated": false,
        },
        "timeline": timeline,
        "gross": gross,
        "seats": seat_rows,
        "totals": {
            "completed_turns": totals.completed_turns,
            "open_turns": totals.open_turns,
            "input_tokens": totals.input_tokens,
            "output_tokens": totals.output_tokens,
            "cache_read_tokens": totals.cache_read,
            "cache_write_tokens": totals.cache_write,
            "fresh_input_tokens": totals.fresh_input,
            "tool_calls": totals.tool_calls,
            "active_minutes": round2(totals.active_ms as f64 / 60_000.0),
            "cost": {
                "reported_usd": round6(totals.cost_usd),
                "results_priced": totals.results_priced,
                "results_total": totals.results_total,
            },
        },
        "coordination": {
            "turns_total": turns.len(),
            "acknowledgement_only_turns": ack_only,
            "verdict_woken_turns": verdict_woken,
            "disposition_or_acknowledgement_turns": disposition_or_ack,
            "disposition_or_acknowledgement_share": share(disposition_or_ack, turns.len()),
            // Founder-signed acts, split by who performed them. `person_*`
            // is the number to compare across runs; the host's own acts under
            // the founder's key are not human interventions and were counted
            // as such until ledger 206 C.
            "person_actions": attribution.person,
            "person_action_count": attribution.person.len(),
            "host_actions_under_founder_key": attribution.host,
            "host_action_under_founder_key_count": attribution.host.len(),
            "unattributed_founder_actions": attribution.unattributed,
            "unattributed_founder_action_count": attribution.unattributed.len(),
            "rulings_opened": rulings_opened,
            "rulings_answered": rulings_answered,
            "founder_key": owner,
        },
        "honesty": honesty,
    })
}

#[derive(Default)]
struct Totals {
    completed_turns: usize,
    open_turns: usize,
    input_tokens: i64,
    output_tokens: i64,
    cache_read: i64,
    cache_write: i64,
    fresh_input: i64,
    tool_calls: i64,
    active_ms: i64,
    cost_usd: f64,
    results_priced: usize,
    results_total: usize,
}

#[allow(clippy::too_many_arguments)]
fn seat_row(
    session_id: &str,
    seat: Option<&SeatAgg>,
    items: &[Item],
    turns: &BTreeMap<(&str, &str), Turn>,
    evs: &[Ev],
    cutoff_ms: Option<i64>,
    totals: &mut Totals,
) -> Value {
    let mine: Vec<&Item> = items
        .iter()
        .filter(|item| item.session_id == session_id)
        .collect();

    let mut input = 0i64;
    let mut output = 0i64;
    let mut fresh = 0i64;
    let mut cache_read = 0i64;
    let mut cache_write = 0i64;
    let mut tool_calls = 0i64;
    let mut active_ms = 0i64;
    let mut cost = 0f64;
    let mut priced = 0usize;
    let mut results = 0usize;
    for item in mine.iter().filter(|item| item.kind == "result") {
        results += 1;
        input += num(item.item, "inputTokens");
        output += num(item.item, "outputTokens");
        active_ms += num(item.item, "durationMs");
        fresh += item
            .item
            .pointer("/usage/inputTokens")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        cache_read += item
            .item
            .pointer("/usage/cacheReadTokens")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        cache_write += item
            .item
            .pointer("/usage/cacheWriteTokens")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        tool_calls += item
            .item
            .pointer("/usage/toolCalls")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if let Some(usd) = item.item.get("costUsd").and_then(Value::as_f64) {
            cost += usd;
            priced += 1;
        }
    }

    let my_turns: Vec<&Turn> = turns
        .iter()
        .filter(|((sid, _), _)| *sid == session_id)
        .map(|(_, turn)| turn)
        .collect();
    let completed = my_turns.iter().filter(|turn| turn.completed).count();
    let open = my_turns.len() - completed;

    // Waiting: wall time from this seat's first prompt to the cutoff with no
    // open turn. Reproduces both 2026-09-20 audits exactly.
    let busy_ms: i64 = my_turns
        .iter()
        .map(|turn| (turn.end_ms - turn.start_ms).max(0))
        .sum();
    let first_ms = my_turns.iter().map(|turn| turn.start_ms).min();
    let idle_minutes = match (cutoff_ms, first_ms) {
        (Some(cutoff), Some(first)) => json!(round2((cutoff - first - busy_ms) as f64 / 60_000.0)),
        _ => json!(null),
    };

    // Latency: 44224 turn_queued → turn_started paired by (session, commandId).
    let mut queued: BTreeMap<&str, i64> = BTreeMap::new();
    let mut started: BTreeMap<&str, i64> = BTreeMap::new();
    for ev in evs
        .iter()
        .filter(|e| e.kind == KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
    {
        if ev
            .content
            .pointer("/session/sessionId")
            .and_then(Value::as_str)
            != Some(session_id)
        {
            continue;
        }
        let Some(command_id) = ev.content.get("commandId").and_then(Value::as_str) else {
            continue;
        };
        match ev.content.get("status").and_then(Value::as_str) {
            Some("turn_queued") => {
                queued.entry(command_id).or_insert(ev.created_at);
            }
            Some("turn_started") => {
                started.entry(command_id).or_insert(ev.created_at);
            }
            _ => {}
        }
    }
    let mut samples: Vec<i64> = queued
        .iter()
        .filter_map(|(command_id, at)| started.get(command_id).map(|start| start - at))
        .collect();
    samples.sort_unstable();
    let unpaired = started.len().saturating_sub(samples.len());

    // Refusals and schema discovery, counted as occurrences in tool results.
    let mut hire_codes: BTreeMap<String, usize> = BTreeMap::new();
    let mut user_errors = 0usize;
    let mut user_error_rows = 0usize;
    let mut relay_errors = 0usize;
    let mut orientation_calls = 0usize;
    let mut call_total = 0usize;
    let mut polling_turns: BTreeSet<&str> = BTreeSet::new();
    let mut calls_by_turn: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for item in &mine {
        match item.kind {
            "tool_call" => {
                call_total += 1;
                let input = text_of(item.item, "input");
                if is_orientation(&input) {
                    orientation_calls += 1;
                }
                if let Some(turn_id) = item.turn_id {
                    calls_by_turn.entry(turn_id).or_default().push(input);
                }
            }
            "tool_result" => {
                let body = text_of(item.item, "content");
                let count = occurrences(&body, USER_ERROR_TOKEN);
                if count > 0 {
                    user_errors += count;
                    user_error_rows += 1;
                }
                relay_errors += occurrences(&body, RELAY_ERROR_TOKEN);
                for code in hire_refusal_codes(&body) {
                    *hire_codes.entry(code).or_default() += 1;
                }
                if ORIENTATION_MARKERS.iter().any(|m| body.contains(m)) {
                    orientation_calls += 1;
                }
            }
            _ => {}
        }
    }
    for (turn_id, calls) in &calls_by_turn {
        let sleeps = calls.iter().filter(|call| call.contains("sleep")).count();
        if sleeps == 0 {
            continue;
        }
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for call in calls {
            *seen.entry(call.as_str()).or_default() += 1;
        }
        if seen.values().any(|count| *count >= 3) {
            polling_turns.insert(turn_id);
        }
    }

    // Prompt provenance.
    let mut from_person = 0usize;
    let mut from_host_notice = 0usize;
    let mut from_team = 0usize;
    for item in mine.iter().filter(|item| item.kind == "user_prompt") {
        let host = item
            .item
            .get("hostAnswer")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let sender_role = item.item.get("senderRole").and_then(Value::as_str);
        let wake = item
            .item
            .get("commandId")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with(WAKE_COMMAND_PREFIX));
        if host {
            from_host_notice += 1;
        } else if sender_role.is_some() || wake {
            from_team += 1;
        } else {
            from_person += 1;
        }
    }

    totals.completed_turns += completed;
    totals.open_turns += open;
    totals.input_tokens += input;
    totals.output_tokens += output;
    totals.cache_read += cache_read;
    totals.cache_write += cache_write;
    totals.fresh_input += fresh;
    totals.tool_calls += tool_calls;
    totals.active_ms += active_ms;
    totals.cost_usd += cost;
    totals.results_priced += priced;
    totals.results_total += results;

    json!({
        "session_id": session_id,
        "role": seat.and_then(|s| s.role.clone()),
        "actor": seat.and_then(|s| s.actor.clone()),
        "runtime": seat.and_then(|s| s.runtime.clone()),
        "model": seat.and_then(|s| s.model.clone()),
        "provider": seat.and_then(|s| s.provider.clone()),
        "generation": seat.and_then(|s| s.generation),
        "turns": { "completed": completed, "open": open },
        "tokens": {
            "input_total_cache_inclusive": input,
            "fresh_input": fresh,
            "cache_read": cache_read,
            "cache_write": cache_write,
            "output": output,
        },
        "tool_calls_reported": tool_calls,
        "cost": {
            "reported_usd": round6(cost),
            "results_priced": priced,
            "results_total": results,
            "coverage": share(priced, results),
        },
        "active_minutes": round2(active_ms as f64 / 60_000.0),
        "no_open_turn_wall_minutes": idle_minutes,
        "queue_to_start_seconds": {
            "samples": samples,
            "p50": percentile50(&samples),
            "max": samples.last().copied(),
            "resolution_limited_zeros": samples.iter().filter(|s| **s == 0).count(),
            "starts_without_a_queue_receipt": unpaired,
        },
        "refusals": {
            "hire_by_code": hire_codes,
            "relay_error_records": relay_errors,
            "schema_discovery_user_error_records": user_errors,
            "schema_discovery_tool_results": user_error_rows,
        },
        "orientation": {
            "tool_calls": orientation_calls,
            "tool_calls_total": call_total,
            "share_of_tool_calls": share(orientation_calls, call_total),
        },
        "polling_turns": polling_turns.len(),
        "prompts": {
            "from_person": from_person,
            "from_host_notice": from_host_notice,
            "from_team_wake": from_team,
        },
    })
}

// ── Timeline ────────────────────────────────────────────────────────────────

fn build_timeline(
    evs: &[Ev],
    items: &[Item],
    tx_rows: &[(&Ev, &str)],
    session_ref: Option<&str>,
) -> Vec<Value> {
    let mut rows: Vec<(i64, Value)> = Vec::new();
    let mut push = |at: i64, event: &str, detail: Value, id: Option<&str>| {
        rows.push((
            at,
            json!({ "at": rfc3339(at), "unix": at, "event": event, "detail": detail, "event_id": id }),
        ));
    };

    let genesis_at = evs
        .iter()
        .filter(|ev| ev.kind == KIND_CODING_SESSION_GENESIS)
        .filter(|ev| match session_ref {
            None => true,
            Some(wanted) => {
                ev.tag("csg-session") == Some(wanted)
                    || ev.content.get("sessionRef").and_then(Value::as_str) == Some(wanted)
            }
        })
        .map(|ev| ev.created_at)
        .min();

    let matches_umbrella = |ev: &Ev| -> bool {
        match session_ref {
            None => true,
            Some(wanted) => {
                ev.tag("d") == Some(wanted)
                    || ev.tag("csg-session") == Some(wanted)
                    || ev.content.get("sessionRef").and_then(Value::as_str) == Some(wanted)
            }
        }
    };

    for ev in evs {
        match ev.kind {
            k if k == KIND_CODING_SESSION_GENESIS && matches_umbrella(ev) => {
                push(ev.created_at, "genesis", Value::Null, Some(&ev.id));
            }
            k if k == KIND_CODING_SESSION_GOAL && matches_umbrella(ev) => {
                push(
                    ev.created_at,
                    "goal",
                    json!(clip(&ev.raw_content, 200)),
                    Some(&ev.id),
                );
            }
            k if k == KIND_CODING_SESSION_CLOSURE && matches_umbrella(ev) => {
                push(ev.created_at, "closure", Value::Null, Some(&ev.id));
            }
            k if k == KIND_CODING_SESSION_LIFECYCLE_COMMAND => {
                // A create or a hire names the umbrella it is for in its own
                // action body. Without this a channel that has held several
                // team sessions puts every one of their hires on one timeline.
                if let Some(wanted) = session_ref {
                    if ev
                        .content
                        .pointer("/action/sessionRef")
                        .and_then(Value::as_str)
                        != Some(wanted)
                    {
                        continue;
                    }
                }
                let action = ev
                    .content
                    .pointer("/action/type")
                    .and_then(Value::as_str)
                    .unwrap_or("lifecycle");
                push(ev.created_at, action, Value::Null, Some(&ev.id));
            }
            k if k == KIND_WORKFLOW_TRIGGER && genesis_at.is_none_or(|at| ev.created_at >= at) => {
                push(
                    ev.created_at,
                    "action.trigger",
                    json!(ev.tag("d")),
                    Some(&ev.id),
                );
            }
            k if k == KIND_WORKFLOW_APPROVAL_REQUESTED
                && genesis_at.is_none_or(|at| ev.created_at >= at) =>
            {
                push(
                    ev.created_at,
                    "action.approval_requested",
                    ev.content.get("message").cloned().unwrap_or(Value::Null),
                    Some(&ev.id),
                );
            }
            k if k == KIND_APPROVAL_GRANT && genesis_at.is_none_or(|at| ev.created_at >= at) => {
                push(
                    ev.created_at,
                    "action.approval_granted",
                    json!(ev.tag("d")),
                    Some(&ev.id),
                );
            }
            k if k == KIND_APPROVAL_DENY && genesis_at.is_none_or(|at| ev.created_at >= at) => {
                push(
                    ev.created_at,
                    "action.approval_denied",
                    json!(ev.tag("d")),
                    Some(&ev.id),
                );
            }
            k if k == KIND_WORKFLOW_HOST_STEP_REQUESTED
                && genesis_at.is_none_or(|at| ev.created_at >= at) =>
            {
                push(
                    ev.created_at,
                    "action.host_requested",
                    json!(ev.tag("d")),
                    Some(&ev.id),
                );
            }
            k if k == KIND_HOST_STEP_CLAIM && genesis_at.is_none_or(|at| ev.created_at >= at) => {
                push(
                    ev.created_at,
                    "action.claimed",
                    json!(ev.tag("d")),
                    Some(&ev.id),
                );
            }
            k if k == KIND_HOST_STEP_RESULT && genesis_at.is_none_or(|at| ev.created_at >= at) => {
                push(
                    ev.created_at,
                    "action.result",
                    json!({
                        "exitCode": ev.content.get("exitCode"),
                        "headSha": ev.content.get("headSha"),
                        "dirty": ev.content.get("dirty"),
                    }),
                    Some(&ev.id),
                );
            }
            // A repository's ref history is older than any one session, so a
            // ref state published before this umbrella's genesis is not this
            // session's delivery and is not put on its timeline.
            k if k == KIND_GIT_REPO_STATE && genesis_at.is_none_or(|at| ev.created_at >= at) => {
                let refs: Map<String, Value> = ev
                    .tags
                    .iter()
                    .filter(|tag| tag.first().is_some_and(|name| name.starts_with("refs/")))
                    .filter_map(|tag| Some((tag.first()?.clone(), json!(tag.get(1)?.clone()))))
                    .collect();
                push(
                    ev.created_at,
                    "observed_delivery",
                    json!({ "repo": ev.tag("d"), "refs": refs }),
                    Some(&ev.id),
                );
            }
            _ => {}
        }
    }

    for (ev, ty) in tx_rows {
        push(ev.created_at, ty, Value::Null, Some(&ev.id));
    }

    // First turn start per execution, from the transcript itself.
    let mut first_seen: BTreeSet<&str> = BTreeSet::new();
    for item in items {
        if first_seen.insert(item.session_id) {
            push(
                item.at_ms / 1_000,
                "first_turn_start",
                json!({ "session": item.session_id }),
                None,
            );
        }
    }

    rows.sort_by_key(|(at, _)| *at);
    rows.into_iter().map(|(_, row)| row).collect()
}

/// Goal (or the earliest recorded event) to the terminal record.
///
/// Both endpoints are named, because a gross duration whose ends are implicit
/// is the number that made the kettle run read as 5 h 34 m in one document and
/// 5 h 31 m in another: one started at the goal, the other at the first
/// recorded intent.
fn gross_duration(timeline: &[Value]) -> Value {
    let at = |name: &str| -> Option<i64> {
        timeline
            .iter()
            .find(|row| row.get("event").and_then(Value::as_str) == Some(name))
            .and_then(|row| row.get("unix").and_then(Value::as_i64))
    };
    let goal = at("goal");
    let first_turn = at("first_turn_start");
    let terminal = timeline
        .iter()
        .rev()
        .find(|row| {
            matches!(
                row.get("event").and_then(Value::as_str),
                Some("mission.completed") | Some("closure")
            )
        })
        .and_then(|row| row.get("unix").and_then(Value::as_i64));
    json!({
        "goal_at": goal.map(rfc3339),
        "first_turn_start_at": first_turn.map(rfc3339),
        "terminal_at": terminal.map(rfc3339),
        "goal_to_terminal_seconds": match (goal, terminal) {
            (Some(goal), Some(terminal)) => json!(terminal - goal),
            _ => Value::Null,
        },
        "first_turn_to_terminal_seconds": match (first_turn, terminal) {
            (Some(start), Some(terminal)) => json!(terminal - start),
            _ => Value::Null,
        },
    })
}

// ── Rendering ───────────────────────────────────────────────────────────────

/// The `--format compact` rendering: one fixed-width table per section.
pub fn table_lines(report: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push("TIMELINE".to_owned());
    for row in report
        .get("timeline")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        lines.push(format!(
            "  {}  {}",
            row.get("at").and_then(Value::as_str).unwrap_or("?"),
            row.get("event").and_then(Value::as_str).unwrap_or("?"),
        ));
    }
    lines.push(String::new());
    lines.push("SEATS".to_owned());
    lines.push(format!(
        "  {:<10} {:>4} {:>4} {:>13} {:>9} {:>7} {:>9} {:>8}",
        "role", "done", "open", "input", "output", "tools", "cost", "idle m"
    ));
    for seat in report
        .get("seats")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        lines.push(format!(
            "  {:<10} {:>4} {:>4} {:>13} {:>9} {:>7} {:>9} {:>8}",
            seat.get("role").and_then(Value::as_str).unwrap_or("?"),
            json_num(seat.pointer("/turns/completed")),
            json_num(seat.pointer("/turns/open")),
            json_num(seat.pointer("/tokens/input_total_cache_inclusive")),
            json_num(seat.pointer("/tokens/output")),
            json_num(seat.pointer("/tool_calls_reported")),
            json_num(seat.pointer("/cost/reported_usd")),
            json_num(seat.pointer("/no_open_turn_wall_minutes")),
        ));
    }
    lines.push(String::new());
    lines.push("COORDINATION".to_owned());
    if let Some(coordination) = report.get("coordination") {
        lines.push(format!("  {coordination}"));
    }
    lines.push(String::new());
    lines.push("HONESTY".to_owned());
    for row in report
        .get("honesty")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        lines.push(format!(
            "  {} = {}: {}",
            row.get("metric").and_then(Value::as_str).unwrap_or("?"),
            row.get("value").and_then(Value::as_str).unwrap_or("?"),
            row.get("reason").and_then(Value::as_str).unwrap_or("?"),
        ));
    }
    lines
}

// ── Small helpers ───────────────────────────────────────────────────────────

fn str_at(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn num(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or_default()
}

fn json_num(value: Option<&Value>) -> String {
    value.map(Value::to_string).unwrap_or_else(|| "-".into())
}

fn text_of(item: &Value, key: &str) -> String {
    match item.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars().take(max).collect::<String>() + "…"
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn is_orientation(call: &str) -> bool {
    ORIENTATION_MARKERS.iter().any(|m| call.contains(m))
        || ORIENTATION_PATHS.iter().any(|p| call.contains(p))
}

/// Every `HIRE_*` refusal code named anywhere in a tool result.
fn hire_refusal_codes(body: &str) -> Vec<String> {
    let mut codes = Vec::new();
    let bytes = body.as_bytes();
    let mut index = 0usize;
    while let Some(found) = body[index..].find("HIRE_") {
        let start = index + found;
        let mut end = start + "HIRE_".len();
        while end < bytes.len() && (bytes[end].is_ascii_uppercase() || bytes[end] == b'_') {
            end += 1;
        }
        codes.push(body[start..end].to_owned());
        index = end;
    }
    codes
}

/// Whether a 44220 or 44221 command belongs to the umbrella under measurement.
///
/// A channel outlives its sessions: the kettle channel holds an earlier
/// hiring-verification umbrella whose founder-signed commands would otherwise
/// be counted as unrequested human actions in a run they predate by a day. A
/// turn command names its execution in `cs-target`; a lifecycle command names
/// the umbrella in its own action body.
fn command_is_this_umbrellas(
    ev: &Ev,
    session_ref: Option<&str>,
    selected: &BTreeSet<String>,
) -> bool {
    let Some(wanted) = session_ref else {
        return true;
    };
    if ev.kind == KIND_CODING_SESSION_LIFECYCLE_COMMAND {
        return ev
            .content
            .pointer("/action/sessionRef")
            .and_then(Value::as_str)
            == Some(wanted);
    }
    ev.tag("cs-target")
        .is_some_and(|target| selected.iter().any(|session| target.contains(session)))
}

/// Founder-signed acts, split by who actually performed them.
#[derive(Default)]
struct FounderActions {
    /// A goal or turn the person typed, a ruling they answered, an approval
    /// they signed, a trigger they started by hand.
    person: Vec<Value>,
    /// This computer, signing with the founder's key: a seat created to
    /// answer a lead's hire request, a host answer, a launch-time act.
    host: Vec<Value>,
    /// Founder-signed, and the wire does not say which. Never guessed into
    /// one of the other two.
    unattributed: Vec<Value>,
}

/// Split every founder-signed act into person, host and unattributed.
///
/// **Why this exists.** The count this replaced was every founder-signed
/// 44220/44221 that was not a tagged host answer, reported as
/// `human_actions`. On a team session that is wrong in one specific,
/// repeatable way: when a lead hires, the desktop answers the hire by
/// signing `session.create` **with the founder's key**, so a seat this
/// computer created to satisfy an agent's own request was counted as an
/// unrequested human intervention. The 2026-09-20 kettle run reported 7 such
/// actions; 2 of the 7 are host-made seat creations, and the RPG run's 9 are
/// 2 and 7 (ledger 206 C).
///
/// **The rules, each named on the row it decides.**
///
/// - A `session.create` is the host's when some earlier `session.hire` in the
///   window names the same umbrella and role and the create's `initialTurn`
///   *contains* that hire's `brief`. Containment, not equality: the host
///   prepends its own attribution line (`[From the lead] `) and trims the
///   brief's trailing newline, so equality would miss every real pair.
/// - A `session.create` with no `initialTurn` and no matching hire is the
///   person's: that is a seat opened from the app with nothing to say to it.
/// - A `session.create` carrying an `initialTurn` that matches no hire is
///   **unattributed**. It could be a person typing a first turn, or a hire
///   whose request fell outside the window; the wire does not say.
/// - A `thread.turn.start` is the person's unless its command id carries the
///   `cli-wake-v1:` prefix, which is one seat waking another — under the
///   founder's key that is a CLI acting, not a person typing, and it is
///   unattributed rather than assigned.
/// - A tagged host answer is the host's, by its own tag.
/// - A founder-signed approval (46030/46031), manual trigger (46020) or
///   `decision.answer` is the person's. These are channel- or author-scoped
///   rather than umbrella-scoped, and each row says so.
fn attribute_founder_actions(
    evs: &[Ev],
    owner: &str,
    session_ref: Option<&str>,
    selected: &BTreeSet<String>,
) -> FounderActions {
    let mut out = FounderActions::default();
    let hires: Vec<&Ev> = evs
        .iter()
        .filter(|ev| {
            ev.kind == KIND_CODING_SESSION_LIFECYCLE_COMMAND
                && ev.content.pointer("/action/type").and_then(Value::as_str)
                    == Some("session.hire")
        })
        .collect();

    for ev in evs.iter().filter(|ev| ev.pubkey == owner) {
        let action = ev.content.pointer("/action/type").and_then(Value::as_str);
        let row = |rule: &str, answers: Option<&str>| {
            json!({
                "at": rfc3339(ev.created_at),
                "kind": ev.kind,
                "event_id": ev.id,
                "action": action,
                "rule": rule,
                "answers_hire_request": answers,
            })
        };
        match ev.kind {
            KIND_CODING_SESSION_COMMAND | KIND_CODING_SESSION_LIFECYCLE_COMMAND => {
                if !command_is_this_umbrellas(ev, session_ref, selected) {
                    continue;
                }
                if ev.tag("buzz-host-answer").is_some() {
                    out.host
                        .push(row("carries the `buzz-host-answer` tag", None));
                    continue;
                }
                match action {
                    Some("session.create") => {
                        let initial_turn = ev
                            .content
                            .pointer("/action/initialTurn")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match answering_hire(ev, initial_turn, &hires) {
                            Some(hire) => out.host.push(row(
                                "a seat created to answer a lead's `session.hire`: same umbrella \
                                 and role, and the hire's brief is the create's initial turn",
                                Some(&hire),
                            )),
                            None if initial_turn.is_empty() => out.person.push(row(
                                "a seat created with no initial turn and no hire request naming \
                                 it",
                                None,
                            )),
                            None => out.unattributed.push(row(
                                "a seat created with an initial turn that matches no \
                                 `session.hire` in this window; a person's first turn and a hire \
                                 requested outside the window look the same here",
                                None,
                            )),
                        }
                    }
                    Some("thread.turn.start") => {
                        let command_id = ev
                            .content
                            .get("commandId")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if command_id.starts_with(WAKE_COMMAND_PREFIX) {
                            out.unattributed.push(row(
                                "a turn opened by a `cli-wake-v1:` command id — a CLI waking a \
                                 seat, signed with the founder's key",
                                None,
                            ));
                        } else {
                            out.person
                                .push(row("a turn opened with no wake command id", None));
                        }
                    }
                    _ => out.person.push(row(
                        "a founder-signed lifecycle command that is neither a create nor a turn",
                        None,
                    )),
                }
            }
            KIND_APPROVAL_GRANT | KIND_APPROVAL_DENY => out.person.push(row(
                "an approval signed by the founder key (46030/46031 carry no `h` tag, so these \
                 are reached by an author+kind read over the window, not by channel scope)",
                None,
            )),
            KIND_WORKFLOW_TRIGGER => out.person.push(row(
                "a manual trigger signed by the founder key; scoped to this channel, not to this \
                 umbrella",
                None,
            )),
            KIND_CODING_SESSION_TEAM_TRANSACTION
                if ev.content.get("type").and_then(Value::as_str) == Some("decision.answer") =>
            {
                out.person
                    .push(row("a ruling answered under the founder key", None));
            }
            _ => {}
        }
    }
    out
}

/// The id of the `session.hire` a founder-signed `session.create` answers.
fn answering_hire(create: &Ev, initial_turn: &str, hires: &[&Ev]) -> Option<String> {
    if initial_turn.is_empty() {
        return None;
    }
    let field = |ev: &Ev, name: &str| {
        ev.content
            .pointer(&format!("/action/{name}"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let session = field(create, "sessionRef");
    let role = field(create, "role");
    hires
        .iter()
        .filter(|hire| hire.created_at <= create.created_at)
        .filter(|hire| field(hire, "sessionRef") == session && field(hire, "role") == role)
        .filter(|hire| {
            field(hire, "brief")
                .map(|brief| brief.trim().to_owned())
                .is_some_and(|brief| !brief.is_empty() && initial_turn.contains(&brief))
        })
        // The latest such hire: a refused hire is retried, and it is the
        // request this create actually answered that the row should name.
        .max_by_key(|hire| hire.created_at)
        .map(|hire| hire.id.to_owned())
}

/// The founder key this session answers to, from the `projectRef` address.
fn project_owner(evs: &[Ev]) -> Option<String> {
    evs.iter()
        .filter(|ev| ev.kind == KIND_CODING_SESSION_METADATA)
        .find_map(|ev| {
            let addr = ev.content.get("projectRef")?.as_str()?;
            addr.split(':').nth(1).map(str::to_owned)
        })
}

fn percentile50(sorted: &[i64]) -> Value {
    if sorted.is_empty() {
        return Value::Null;
    }
    json!(sorted[sorted.len() / 2])
}

fn share(part: usize, whole: usize) -> Value {
    if whole == 0 {
        return Value::Null;
    }
    json!(round2(part as f64 * 100.0 / whole as f64))
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn rfc3339(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|time| time.to_rfc3339())
        .unwrap_or_else(|| seconds.to_string())
}

#[cfg(test)]
#[path = "measure_tests.rs"]
mod measure_tests;
