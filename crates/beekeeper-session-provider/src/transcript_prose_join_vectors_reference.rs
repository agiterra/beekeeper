//! The reference join — `conformance/transcript-prose-join/CONTRACT.md` in
//! executable form — and the byte-stable serializer that writes the vectors.
//! A sibling of `transcript_prose_join_vectors_tests.rs`, which owns the cases.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

/// Latest-metadata (kind:44223) statuses that end the session itself, so
/// nothing more of that generation's answer is being written (`SessionStatus`
/// in `beekeeper-core/src/coding_session_payload.rs`). Every other status, and no
/// metadata at all, leaves `arriving` to the lease and the turn state.
///
/// `interrupted` and `failed` are deliberately absent: the producer publishes
/// them at the end of an ordinary turn while the session goes on taking turns
/// (`beekeeper-session-provider/src/lib.rs`, the turn-outcome publish: a cancelled
/// turn is `Interrupted`, a failure whose agent is still alive is `Failed`,
/// and the next turn publishes `Running`). That turn's own `result` or
/// `interrupted` item already ends its trailing message, so counting them here
/// adds nothing — and would hide the next turn's answer whenever its `Running`
/// metadata ties or arrives late. A failure whose agent is gone publishes
/// `disconnected`, which is here.
pub(super) const TERMINAL_STATUSES: [&str; 3] = ["completed", "stopped", "disconnected"];

/// Statuses that end a turn but not the session: never an ended target.
pub(super) const TURN_ONLY_STATUSES: [&str; 2] = ["interrupted", "failed"];

pub(super) fn content(envelope: &Value) -> &Value {
    &envelope["content"]
}

pub(super) fn item_kind(envelope: &Value) -> &str {
    content(envelope)["item"]["kind"].as_str().unwrap_or("")
}

/// Signer, then the exact target, ordered as the vectors order messages.
pub(super) type TargetKey = (String, String, String, String, u64);

fn target_key(signer: &Value, session: &Value) -> TargetKey {
    let text = |value: &Value| value.as_str().unwrap_or("").to_owned();
    (
        text(signer),
        text(&session["driver"]),
        text(&session["instanceId"]),
        text(&session["sessionId"]),
        session["generation"].as_u64().unwrap_or(0),
    )
}

/// The exact target of one envelope, signer first.
pub(super) fn group_key(envelope: &Value) -> TargetKey {
    target_key(&envelope["signer"], &content(envelope)["session"])
}

/// Exact targets in which no answer can still be arriving, from wire facts
/// only: the same signer + driver + instanceId + sessionId has published any
/// item at a higher `generation` (the run was superseded — a crash, a host
/// restart, a resume), the target's latest metadata status is terminal, or
/// the reader holds no unexpired `live` kind:24223 lease for it (the provider
/// is not proven reachable — its machine slept, lost power or lost the
/// network, and nothing will ever publish a terminal status for it).
fn ended_targets(
    input: &[Value],
    session_status: &[Value],
    session_lease: &[Value],
) -> BTreeSet<TargetKey> {
    let keys: BTreeSet<TargetKey> = input.iter().map(group_key).collect();
    let mut ended: BTreeSet<TargetKey> = keys
        .iter()
        .filter(|(signer, driver, instance, session, generation)| {
            keys.iter().any(|(s, d, i, id, g)| {
                s == signer && d == driver && i == instance && id == session && g > generation
            })
        })
        .cloned()
        .collect();
    for status in session_status {
        if TERMINAL_STATUSES.contains(&status["status"].as_str().unwrap_or("")) {
            ended.insert(target_key(&status["signer"], &status["target"]));
        }
    }
    let live: BTreeSet<TargetKey> = session_lease
        .iter()
        .filter(|lease| lease["lease"].as_str() == Some("live"))
        .map(|lease| target_key(&lease["signer"], &lease["target"]))
        .collect();
    ended.extend(keys.into_iter().filter(|key| !live.contains(key)));
    ended
}

struct Joined {
    kind: String,
    signer: String,
    target: Value,
    turn_id: Value,
    parent_tool_id: Value,
    message_id: Option<String>,
    first: String,
    last: String,
    text: String,
}

fn joins(message: &Joined, envelope: &Value) -> bool {
    let item = &content(envelope)["item"];
    let message_id_agrees = match (&message.message_id, item["messageId"].as_str()) {
        (Some(ours), Some(theirs)) => ours == theirs,
        _ => true,
    };
    item_kind(envelope) == message.kind
        && content(envelope)["turnId"] == message.turn_id
        && item.get("parentToolId").cloned().unwrap_or(Value::Null) == message.parent_tool_id
        && message_id_agrees
}

/// Join a vector's input into its expected messages and brief.
/// `session_status` is the vector's `sessionStatus[]`: the latest metadata
/// status per exact target, where the case has one. `session_lease` is its
/// `sessionLease[]`: the reader's view of each exact target's winning
/// kind:24223 lease at its own `now` — `live`, `released` or `lapsed`; a
/// target with no entry has no lease the reader holds.
pub(super) fn reference_join(
    input: &[Value],
    session_status: &[Value],
    session_lease: &[Value],
) -> (Vec<Value>, Vec<Value>) {
    let ended = ended_targets(input, session_status, session_lease);
    let mut groups: BTreeMap<TargetKey, Vec<&Value>> = BTreeMap::new();
    // Rule 0: one event, one piece. A repeated delivery of the same event id
    // (backfill plus a live subscription) is dropped before anything sorts it.
    let mut seen: BTreeSet<(TargetKey, &str)> = BTreeSet::new();
    for envelope in input {
        let event_id = envelope["eventId"].as_str().unwrap_or("");
        if !seen.insert((group_key(envelope), event_id)) {
            continue;
        }
        groups
            .entry(group_key(envelope))
            .or_default()
            .push(envelope);
    }
    let mut messages = Vec::new();
    let mut brief = Vec::new();
    for (key, mut envelopes) in groups {
        let target_ended = ended.contains(&key);
        envelopes.sort_by_key(|envelope| content(envelope)["eventSeq"].as_u64());
        let mut joined: Vec<Joined> = Vec::new();
        let mut open = false;
        for envelope in &envelopes {
            let kind = item_kind(envelope);
            if kind != "assistant_text" && kind != "reasoning" {
                open = false;
                continue;
            }
            let item = &content(envelope)["item"];
            let text = item["text"].as_str().unwrap_or("");
            let event_id = envelope["eventId"].as_str().unwrap_or("").to_owned();
            if let Some(message) = joined.last_mut().filter(|m| open && joins(m, envelope)) {
                message.text.push_str(text);
                message.last = event_id;
                if message.message_id.is_none() {
                    message.message_id = item["messageId"].as_str().map(str::to_owned);
                }
                continue;
            }
            open = true;
            joined.push(Joined {
                kind: kind.to_owned(),
                signer: envelope["signer"].as_str().unwrap_or("").to_owned(),
                target: content(envelope)["session"].clone(),
                turn_id: content(envelope)["turnId"].clone(),
                parent_tool_id: item.get("parentToolId").cloned().unwrap_or(Value::Null),
                message_id: item["messageId"].as_str().map(str::to_owned),
                first: event_id.clone(),
                last: event_id,
                text: text.to_owned(),
            });
        }
        // Per turn: whether it ended, and which event is its last.
        let mut turns: Vec<(Value, bool, String)> = Vec::new();
        for envelope in &envelopes {
            let turn = content(envelope)["turnId"].clone();
            if turn.is_null() {
                continue;
            }
            let terminal = matches!(item_kind(envelope), "result" | "interrupted");
            let id = envelope["eventId"].as_str().unwrap_or("").to_owned();
            match turns.iter_mut().find(|(t, _, _)| *t == turn) {
                Some(entry) => {
                    entry.1 |= terminal;
                    entry.2 = id;
                }
                None => turns.push((turn, terminal, id)),
            }
        }
        for message in &joined {
            let arriving = !target_ended
                && turns.iter().any(|(turn, turn_ended, last)| {
                    *turn == message.turn_id && !turn_ended && *last == message.last
                });
            messages.push(json!({
                "kind": message.kind,
                "signer": message.signer,
                "target": message.target,
                "turnId": message.turn_id,
                "parentToolId": message.parent_tool_id,
                "firstEventId": message.first,
                "lastEventId": message.last,
                "text": message.text,
                "arriving": arriving,
            }));
        }
        let signer = envelopes
            .first()
            .and_then(|envelope| envelope["signer"].as_str())
            .unwrap_or("");
        let session = envelopes
            .first()
            .map(|envelope| content(envelope)["session"].clone())
            .unwrap_or(Value::Null);
        for (turn, _, _) in &turns {
            let own = joined.iter().rev().find(|message| {
                message.kind == "assistant_text"
                    && message.turn_id == *turn
                    && message.parent_tool_id.is_null()
            });
            brief.push(json!({
                "signer": signer,
                "target": session,
                "turnId": turn,
                "latestAssistantEventId": own.map(|message| message.last.clone()),
                "latestAssistantFirstEventId": own.map(|message| message.first.clone()),
            }));
        }
    }
    (messages, brief)
}

// ---------------------------------------------------------------------------
// Byte-stable serialization
// ---------------------------------------------------------------------------

/// Pretty JSON with sorted keys, independent of whether some crate in the
/// build turned on serde_json's `preserve_order`.
pub(super) fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, 0, &mut out);
    out.push('\n');
    out
}

fn write_canonical(value: &Value, depth: usize, out: &mut String) {
    let pad = |depth: usize| "  ".repeat(depth);
    match value {
        Value::Object(map) if !map.is_empty() => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push_str("{\n");
            for (index, key) in keys.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                out.push_str(&Value::String((*key).clone()).to_string());
                out.push_str(": ");
                write_canonical(&map[key.as_str()], depth + 1, out);
                if index + 1 < keys.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad(depth));
            out.push('}');
        }
        Value::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                write_canonical(item, depth + 1, out);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad(depth));
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}
