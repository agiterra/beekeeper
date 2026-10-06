//! Put a streamed answer back together before `bee sessions transcript --format
//! md` prints it.
//!
//! The producer cuts one answer into several signed kind:44225
//! `assistant_text` items — at 24 KiB, at every tool call, and (with
//! `BUZZ_CSP_TRANSCRIPT_PARAGRAPH_FLUSH`) at paragraph boundaries. A reader
//! that heads every piece `**Assistant**` shows one answer as three. This
//! module is NIP-CST amendment 3 **Join key**
//! (`conformance/transcript-prose-join/CONTRACT.md`) for the CLI:
//!
//! - within one exact target — signer + driver + instanceId + sessionId +
//!   generation — drop repeated deliveries of the same event id (rule 0), sort
//!   by `eventSeq`, and join adjacent pieces of the same kind with the same
//!   `turnId` and `parentToolId` (rules 1–2);
//! - by plain concatenation, nothing inserted or trimmed (rule 3);
//! - identity is the first piece's id, the end is the last piece's (rule 4);
//! - a present, differing `messageId` splits (rule 5);
//! - `reasoning` joins the same way, only with reasoning (rule 6);
//! - `arriving` only from wire facts: an open turn, a target not superseded,
//!   not ended by status, and a live unexpired kind:24223 lease (rule 7). A
//!   reader with no lease evidence shows nothing as arriving.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_lease::{
    decode_coding_session_lease, CodingSessionLeaseState, CODING_SESSION_LEASE_REPLAY_WINDOW_SECS,
};
use beekeeper_core::kind::KIND_CODING_SESSION_LEASE;

use super::{
    content_of, event_created_at, event_str, status_string, MetadataRecord, TranscriptRecord,
};

/// Latest-metadata statuses that end the session itself (contract rule 7).
/// `interrupted` and `failed` end a turn, not the session, and are absent on
/// purpose: the turn's own `result`/`interrupted` item already ends its
/// message.
const SESSION_ENDING_STATUSES: [&str; 3] = ["completed", "stopped", "disconnected"];

/// The muted line printed after a message that is still being written.
pub const STILL_WRITING_LINE: &str = "_(still writing — turn has no result yet)_\n\n";

/// One exact target, signer first: `(signer, cs-target key)`.
type ExactTarget = (String, String);

/// What the reader knows, from the wire, about which targets may still be
/// writing. Rule 7 needs all three facts; a reader without them says nothing
/// is arriving rather than guessing from turn state alone.
#[derive(Debug, Clone, Default)]
pub struct ArrivalEvidence {
    /// Targets the reader holds a `live`, unexpired kind:24223 lease for.
    live_leases: HashSet<ExactTarget>,
    /// Targets known to have ended: superseded by a higher generation, or a
    /// latest metadata status that ends the session.
    ended: HashSet<ExactTarget>,
}

impl ArrivalEvidence {
    /// No lease evidence: nothing renders as arriving.
    #[cfg(test)]
    pub fn none() -> Self {
        Self::default()
    }

    /// Build the evidence from wire facts the caller fetched.
    ///
    /// `transcripts` should be every 44225 the caller holds (not only the
    /// target being rendered) so a higher generation can supersede a lower
    /// one. `lease_events` are raw kind:24223 events; a `live` lease older than
    /// the relay's replay window at `now` has lapsed and counts as none.
    pub fn from_wire(
        transcripts: &[TranscriptRecord],
        metadata: &[MetadataRecord],
        lease_events: &[Value],
        now: i64,
    ) -> Self {
        let mut evidence = Self::default();

        // Superseded: the same signer + driver + instance + session has any
        // item at a higher generation.
        let mut newest_generation: HashMap<(String, String, String, String), u64> = HashMap::new();
        for record in transcripts {
            let session = &record.envelope.session;
            let lineage = (
                record.signer.clone(),
                session.driver.clone(),
                session.instance_id.clone(),
                session.session_id.clone(),
            );
            let entry = newest_generation.entry(lineage).or_insert(0);
            *entry = (*entry).max(session.generation);
        }
        for record in transcripts {
            let session = &record.envelope.session;
            let lineage = (
                record.signer.clone(),
                session.driver.clone(),
                session.instance_id.clone(),
                session.session_id.clone(),
            );
            if newest_generation
                .get(&lineage)
                .is_some_and(|newest| *newest > session.generation)
            {
                evidence
                    .ended
                    .insert((record.signer.clone(), record.target_key.clone()));
            }
        }

        // Ended by status: the latest metadata per exact target. When several
        // tie on `created_at`, any session-ending one ends it — a caret over an
        // answer that may be dead is the claim this module must not make.
        let mut newest_at: HashMap<ExactTarget, i64> = HashMap::new();
        for record in metadata {
            let key = (record.signer.clone(), record.target_key.clone());
            let entry = newest_at.entry(key).or_insert(i64::MIN);
            *entry = (*entry).max(record.created_at);
        }
        for record in metadata {
            let key = (record.signer.clone(), record.target_key.clone());
            if newest_at.get(&key) == Some(&record.created_at)
                && SESSION_ENDING_STATUSES.contains(&status_string(&record.metadata).as_str())
            {
                evidence.ended.insert(key);
            }
        }

        // Leases: the winning record per exact target is the highest
        // `leaseSequence`, then the newest `created_at`.
        let mut winning: HashMap<ExactTarget, (u64, i64, CodingSessionLeaseState)> = HashMap::new();
        for event in lease_events {
            if event.get("kind").and_then(Value::as_u64)
                != Some(u64::from(KIND_CODING_SESSION_LEASE))
            {
                continue;
            }
            let (Some(content), Some(signer), Some(created_at)) = (
                content_of(event),
                event_str(event, "pubkey"),
                event_created_at(event),
            ) else {
                continue;
            };
            let Ok(lease) = decode_coding_session_lease(content) else {
                continue;
            };
            let key = (signer, coding_session_target_key(&lease.target));
            let candidate = (lease.lease_sequence, created_at, lease.state);
            match winning.get(&key) {
                Some(held) if (held.0, held.1) >= (candidate.0, candidate.1) => {}
                _ => {
                    winning.insert(key, candidate);
                }
            }
        }
        let window = i64::try_from(CODING_SESSION_LEASE_REPLAY_WINDOW_SECS).unwrap_or(i64::MAX);
        for (key, (_, created_at, state)) in winning {
            let unexpired = created_at.saturating_add(window) >= now;
            if state == CodingSessionLeaseState::Live && unexpired {
                evidence.live_leases.insert(key);
            }
        }

        evidence
    }

    /// Record the reader's lease view for one exact target directly.
    #[cfg(test)]
    pub fn with_live_lease(mut self, signer: &str, target: &CodingSessionTarget) -> Self {
        self.live_leases
            .insert((signer.to_owned(), coding_session_target_key(target)));
        self
    }

    /// Record that one exact target's latest metadata status is `status`.
    /// Only a session-ending status changes anything.
    #[cfg(test)]
    pub fn with_status(mut self, signer: &str, target: &CodingSessionTarget, status: &str) -> Self {
        if SESSION_ENDING_STATUSES.contains(&status) {
            self.ended
                .insert((signer.to_owned(), coding_session_target_key(target)));
        }
        self
    }

    fn may_be_writing(&self, key: &ExactTarget) -> bool {
        self.live_leases.contains(key) && !self.ended.contains(key)
    }
}

/// One message, joined back together from its pieces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinedProse {
    /// `assistant_text` or `reasoning`.
    pub kind: String,
    /// The signer of every piece.
    pub signer: String,
    /// The exact target every piece belongs to.
    pub target: CodingSessionTarget,
    /// The turn every piece belongs to; `None` outside any turn.
    pub turn_id: Option<String>,
    /// The subagent's tool call id; `None` for the agent's own prose.
    pub parent_tool_id: Option<String>,
    /// The first piece's event id — the message's identity.
    pub first_event_id: String,
    /// The last piece's event id — where the message currently ends.
    pub last_event_id: String,
    /// Every piece's text, concatenated exactly.
    pub text: String,
    /// Still being written, by contract rule 7.
    pub arriving: bool,
}

fn item_kind(record: &TranscriptRecord) -> &str {
    record
        .envelope
        .item
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn item_str<'a>(record: &'a TranscriptRecord, key: &str) -> Option<&'a str> {
    record.envelope.item.get(key).and_then(Value::as_str)
}

/// Whether this item kind joins with its neighbours (rules 1 and 6).
pub fn is_joinable_kind(kind: &str) -> bool {
    matches!(kind, "assistant_text" | "reasoning")
}

struct Open {
    message: JoinedProse,
    message_id: Option<String>,
}

fn continues(open: &Open, record: &TranscriptRecord) -> bool {
    let message_id_agrees = match (&open.message_id, item_str(record, "messageId")) {
        (Some(ours), Some(theirs)) => ours == theirs,
        _ => true,
    };
    item_kind(record) == open.message.kind
        && record.envelope.turn_id == open.message.turn_id
        && item_str(record, "parentToolId").map(str::to_owned) == open.message.parent_tool_id
        && message_id_agrees
}

/// Join every prose run in `records`.
///
/// Messages are ordered by signer, then driver, instanceId, sessionId and
/// generation, then `eventSeq` — the contract's order. Supersession among the
/// given records is detected here too; [`ArrivalEvidence`] adds what the
/// records alone cannot say (status and lease).
pub fn join_prose(records: &[TranscriptRecord], evidence: &ArrivalEvidence) -> Vec<JoinedProse> {
    type Group = (String, String, String, String, u64);
    let mut groups: BTreeMap<Group, Vec<&TranscriptRecord>> = BTreeMap::new();
    let mut seen: HashSet<(String, String, &str)> = HashSet::new();
    for record in records {
        // Rule 0: one event, one piece.
        if !seen.insert((
            record.signer.clone(),
            record.target_key.clone(),
            record.id.as_str(),
        )) {
            continue;
        }
        let session = &record.envelope.session;
        groups
            .entry((
                record.signer.clone(),
                session.driver.clone(),
                session.instance_id.clone(),
                session.session_id.clone(),
                session.generation,
            ))
            .or_default()
            .push(record);
    }

    let superseded: HashSet<&Group> = groups
        .keys()
        .filter(|(signer, driver, instance, session, generation)| {
            groups.keys().any(|(s, d, i, id, g)| {
                s == signer && d == driver && i == instance && id == session && g > generation
            })
        })
        .collect();

    let mut messages = Vec::new();
    for (group, mut pieces) in groups.iter().map(|(key, pieces)| (key, pieces.clone())) {
        pieces.sort_by_key(|record| record.seq);
        let Some(first) = pieces.first() else {
            continue;
        };
        let exact: ExactTarget = (first.signer.clone(), first.target_key.clone());
        let may_be_writing = !superseded.contains(group) && evidence.may_be_writing(&exact);

        let mut joined: Vec<JoinedProse> = Vec::new();
        let mut open: Option<Open> = None;
        for record in &pieces {
            let kind = item_kind(record);
            if !is_joinable_kind(kind) {
                if let Some(done) = open.take() {
                    joined.push(done.message);
                }
                continue;
            }
            let text = item_str(record, "text").unwrap_or("");
            if let Some(current) = open.as_mut().filter(|current| continues(current, record)) {
                current.message.text.push_str(text);
                current.message.last_event_id = record.id.clone();
                if current.message_id.is_none() {
                    current.message_id = item_str(record, "messageId").map(str::to_owned);
                }
                continue;
            }
            if let Some(done) = open.take() {
                joined.push(done.message);
            }
            open = Some(Open {
                message: JoinedProse {
                    kind: kind.to_owned(),
                    signer: record.signer.clone(),
                    target: record.envelope.session.clone(),
                    turn_id: record.envelope.turn_id.clone(),
                    parent_tool_id: item_str(record, "parentToolId").map(str::to_owned),
                    first_event_id: record.id.clone(),
                    last_event_id: record.id.clone(),
                    text: text.to_owned(),
                    arriving: false,
                },
                message_id: item_str(record, "messageId").map(str::to_owned),
            });
        }
        if let Some(done) = open.take() {
            joined.push(done.message);
        }

        if may_be_writing {
            // Per turn: its last event, and whether it has ended.
            let mut turns: HashMap<&str, (&str, bool)> = HashMap::new();
            for record in &pieces {
                let Some(turn) = record.envelope.turn_id.as_deref() else {
                    continue;
                };
                let ends_turn = matches!(item_kind(record), "result" | "interrupted");
                let entry = turns.entry(turn).or_insert((record.id.as_str(), false));
                entry.0 = record.id.as_str();
                entry.1 |= ends_turn;
            }
            for message in &mut joined {
                message.arriving = message
                    .turn_id
                    .as_deref()
                    .and_then(|turn| turns.get(turn))
                    .is_some_and(|(last, ended)| !ended && *last == message.last_event_id);
            }
        }
        messages.extend(joined);
    }
    messages
}

/// The bold heading a prose item renders under. Subagent prose names its
/// parent tool call, so a reader never takes a subagent's words for the
/// agent's own.
pub fn prose_heading(kind: &str, parent_tool_id: Option<&str>) -> String {
    let label = if kind == "reasoning" {
        "Reasoning"
    } else {
        "Assistant"
    };
    match parent_tool_id {
        Some(parent) => format!("**{label} (subagent `{parent}`)**"),
        None => format!("**{label}**"),
    }
}

/// Render one joined message as the transcript Markdown prints it.
pub fn render_joined(message: &JoinedProse) -> String {
    let heading = prose_heading(&message.kind, message.parent_tool_id.as_deref());
    let mut out = format!("{heading}\n\n{}\n\n", message.text);
    if message.arriving {
        out.push_str(STILL_WRITING_LINE);
    }
    out
}

/// How `render_markdown` treats each record of a joined run.
pub enum ProsePlace<'a> {
    /// The first piece: render the whole joined message here.
    Start(&'a JoinedProse),
    /// A later piece, already rendered with its first.
    Continuation,
}

/// Index joined messages by event id so a renderer walking records in order
/// prints each message once, at its first piece.
pub fn index_by_event<'a>(
    messages: &'a [JoinedProse],
    records: &[TranscriptRecord],
) -> HashMap<String, ProsePlace<'a>> {
    let starts: HashMap<(&str, &str), &JoinedProse> = messages
        .iter()
        .map(|message| {
            (
                (message.signer.as_str(), message.first_event_id.as_str()),
                message,
            )
        })
        .collect();
    let mut places = HashMap::new();
    for record in records {
        if !is_joinable_kind(item_kind(record)) {
            continue;
        }
        let place = match starts.get(&(record.signer.as_str(), record.id.as_str())) {
            Some(message) => ProsePlace::Start(message),
            None => ProsePlace::Continuation,
        };
        places.insert(record.id.clone(), place);
    }
    places
}
