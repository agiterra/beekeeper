//! `bee sessions` — read-side analysis over the stored coding-session record.
//!
//! Coding sessions are already an analysis database: every turn, tool call, and
//! result is a signed, h-scoped event sitting in the relay's `events` table
//! (kinds 44223 metadata, 44224 lifecycle receipts, 44225 transcript items).
//! This module is the agent-facing way to read it without writing SQL —
//! `docs/coding-session-analysis.md` covers the SQL when you want it.
//!
//! Two rules shape everything here:
//!
//! 1. **Every filter carries explicit `kinds`.** The relay's p-gate answers 403
//!    to an open-ended query, so there is no such thing as "just fetch the
//!    channel" — see the query builders below.
//! 2. **Resolution matches the desktop consumer exactly.** A *lifecycle*
//!    receipt (`created`, `resumed`, `stopped`, ...) confirms a generation
//!    exists; the newest metadata wins, and a same-second burst is broken on
//!    event id rather than treated as a conflict (a provider legitimately
//!    moves `starting -> idle -> running` inside one second, and
//!    second-granularity `created_at` cannot order that). A *turn* receipt
//!    (`turn_queued`/`turn_started`/`turn_dropped`/`turn_refused`, NIP-CSL
//!    fork amendment 7) is the deliberate exception: it never creates,
//!    confirms, or ends a generation, so `resolve_sessions` ignores it for
//!    that purpose. Diverging from the desktop here would mean the CLI and
//!    the app disagree about what happened, which is worse than either being
//!    wrong alone.
//!
//! Signature verification is NOT performed here: `/query` results come from the
//! relay the caller authenticated to, and every row carries its `signer` so a
//! caller doing trust work can filter on it. The desktop's fail-closed
//! allowlist is a rendering decision, not a storage one.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use buzz_core::coding_session_authority_transition::{
    CodingSessionAuthorityTransitionPayload, CodingSessionAuthorityTransitionType,
    CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::coding_session_payload::{
    LifecycleReceipt, ReceiptStatus, SessionMetadata, TranscriptEnvelope,
};
use buzz_core::kind::{KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_SYSTEM_MESSAGE};
use buzz_sdk::kind::{
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_TRANSCRIPT,
};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::validate_uuid;

/// Item kinds the 44225 contract recognizes. Anything else is counted as
/// `other` rather than dropped — a provider that learns a new item kind must
/// not make this command lie about how much it did not understand.
const KNOWN_ITEM_KINDS: &[&str] = &[
    "user_prompt",
    "assistant_text",
    "reasoning",
    "tool_call",
    "tool_result",
    "result",
    "status",
    "system_init",
    "account_info",
    "context_window_updated",
    "compact_boundary",
    "compact_summary",
    "context_cleared",
    "interrupted",
    "plan",
    "elided",
];

// ── Decoded records ──────────────────────────────────────────────────────────

/// A 44223 metadata event whose content decoded cleanly.
#[derive(Debug, Clone)]
pub struct MetadataRecord {
    id: String,
    signer: String,
    created_at: i64,
    target_key: String,
    canonical: String,
    metadata: SessionMetadata,
    raw: Value,
}

/// A 44224 lifecycle receipt whose content decoded cleanly.
#[derive(Debug, Clone)]
pub struct ReceiptRecord {
    signer: String,
    created_at: i64,
    /// `None` for a receipt that reports a create which never produced a
    /// session — it confirms nothing exists, so it names no target.
    target: Option<CodingSessionTarget>,
    target_key: Option<String>,
    /// Whether this receipt reports one of the D4 turn stages (`turn_queued`,
    /// `turn_started`, `turn_dropped`, `turn_refused`) rather than a lifecycle
    /// outcome. Per NIP-CSL, a turn receipt names the generation its command
    /// targeted but never creates, confirms, or ends it — `resolve_sessions`
    /// must ignore it for those purposes.
    is_turn_status: bool,
    raw: Value,
}

/// A 44225 transcript item whose envelope decoded cleanly.
#[derive(Debug, Clone)]
pub struct TranscriptRecord {
    id: String,
    signer: String,
    created_at: i64,
    target_key: String,
    seq: u64,
    envelope: TranscriptEnvelope,
    raw: Value,
}

/// What decoding a batch of events threw away, so no command reports a clean
/// total over a partly unreadable channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeStats {
    /// Events whose content or tags did not decode against the contract.
    pub malformed: usize,
}

// ── Event field access ───────────────────────────────────────────────────────

fn event_str(event: &Value, key: &str) -> Option<String> {
    event.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn event_created_at(event: &Value) -> Option<i64> {
    event.get("created_at").and_then(Value::as_i64)
}

/// First value of the named tag, in the `["name", "value"]` shape all six
/// coding-session kinds use.
fn tag_value(event: &Value, name: &str) -> Option<String> {
    event
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(Value::as_array)
        .find(|tag| tag.first().and_then(Value::as_str) == Some(name))
        .and_then(|tag| tag.get(1))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn content_of(event: &Value) -> Option<&str> {
    event.get("content").and_then(Value::as_str)
}

// ── Decoding ─────────────────────────────────────────────────────────────────

/// Decode every 44223 event in `events`, dropping and counting the unreadable.
pub fn decode_metadata(events: &[Value]) -> (Vec<MetadataRecord>, DecodeStats) {
    let mut records = Vec::new();
    let mut stats = DecodeStats::default();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_METADATA))
        {
            continue;
        }
        let decoded = content_of(event)
            .and_then(|content| serde_json::from_str::<SessionMetadata>(content).ok());
        let (Some(metadata), Some(id), Some(signer), Some(created_at)) = (
            decoded,
            event_str(event, "id"),
            event_str(event, "pubkey"),
            event_created_at(event),
        ) else {
            stats.malformed += 1;
            continue;
        };
        // The tag is what the relay indexed; the payload is what the signer
        // meant. The desktop's ingress requires them to agree, so an event
        // where they disagree is not a fact this command may quietly average
        // into a total — it is malformed.
        let target_key = coding_session_target_key(&metadata.session);
        if tag_value(event, "cs-target").is_some_and(|tagged| tagged != target_key) {
            stats.malformed += 1;
            continue;
        }
        records.push(MetadataRecord {
            id,
            signer,
            created_at,
            target_key,
            canonical: content_of(event).unwrap_or_default().to_owned(),
            metadata,
            raw: event.clone(),
        });
    }
    (records, stats)
}

/// Whether a decoded `ReceiptStatus` is a turn stage.
///
/// Asked of `buzz-core`'s own enum, never of a list kept here. The list this
/// replaced named four statuses and fell behind the day `turn_degraded` and
/// `interrupt_delivered` were added: both decoded as *lifecycle* outcomes, and
/// a turn receipt that reaches `resolve_sessions` confirms a generation, which
/// NIP-CSL makes a MUST NOT. Nothing failed to compile, and no test noticed.
fn is_turn_receipt_status(status: ReceiptStatus) -> bool {
    status.is_turn_stage()
}

/// Whether a raw receipt JSON value's `status` field names a turn stage.
///
/// Same source of truth as [`is_turn_receipt_status`], reached by parsing the
/// wire string back into the enum: this path exists for a receipt whose typed
/// decode failed for some *other* reason (an unexpected key, a shape this
/// build does not know), not for a status vocabulary this build has never
/// heard of.
fn is_turn_receipt_value(value: &Value) -> bool {
    value
        .get("status")
        .cloned()
        .and_then(|status| serde_json::from_value::<ReceiptStatus>(status).ok())
        .is_some_and(is_turn_receipt_status)
}

/// Decode every 44224 event in `events`, dropping and counting the unreadable.
///
/// Lifecycle statuses decode through `buzz-core`'s typed `LifecycleReceipt`.
/// Turn statuses (every [`ReceiptStatus::is_turn_stage`] variant) take the same
/// typed path first and fall back to reading `status`/`session` straight off
/// the JSON when that fails, so a well-formed turn receipt whose shape this
/// build cannot type is never miscounted as malformed. Either way the result is
/// marked
/// [`ReceiptRecord::is_turn_status`] so callers (`resolve_sessions`) never let
/// it create, confirm, or end a generation.
pub fn decode_receipts(events: &[Value]) -> (Vec<ReceiptRecord>, DecodeStats) {
    let mut records = Vec::new();
    let mut stats = DecodeStats::default();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_LIFECYCLE_RECEIPT))
        {
            continue;
        }
        let (Some(content), Some(signer), Some(created_at)) = (
            content_of(event),
            event_str(event, "pubkey"),
            event_created_at(event),
        ) else {
            stats.malformed += 1;
            continue;
        };

        if let Ok(receipt) = serde_json::from_str::<LifecycleReceipt>(content) {
            records.push(ReceiptRecord {
                signer,
                created_at,
                is_turn_status: is_turn_receipt_status(receipt.status),
                target_key: receipt.session.as_ref().map(coding_session_target_key),
                target: receipt.session,
                raw: event.clone(),
            });
            continue;
        }

        let Some(value) = serde_json::from_str::<Value>(content)
            .ok()
            .filter(is_turn_receipt_value)
        else {
            stats.malformed += 1;
            continue;
        };
        let target = value
            .get("session")
            .filter(|session| !session.is_null())
            .and_then(|session| {
                serde_json::from_value::<CodingSessionTarget>(session.clone()).ok()
            });
        records.push(ReceiptRecord {
            signer,
            created_at,
            target_key: target.as_ref().map(coding_session_target_key),
            target,
            is_turn_status: true,
            raw: event.clone(),
        });
    }
    (records, stats)
}

/// Decode every 44225 event in `events`, dropping and counting the unreadable.
///
/// `cst-seq` is read from the envelope, not the tag: the tag is a decimal
/// string and a lexicographic reader would order seq 10 before seq 9.
pub fn decode_transcripts(events: &[Value]) -> (Vec<TranscriptRecord>, DecodeStats) {
    let mut records = Vec::new();
    let mut stats = DecodeStats::default();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_TRANSCRIPT))
        {
            continue;
        }
        let decoded = content_of(event)
            .and_then(|content| serde_json::from_str::<TranscriptEnvelope>(content).ok());
        let (Some(envelope), Some(id), Some(signer), Some(created_at)) = (
            decoded,
            event_str(event, "id"),
            event_str(event, "pubkey"),
            event_created_at(event),
        ) else {
            stats.malformed += 1;
            continue;
        };
        let target_key = coding_session_target_key(&envelope.session);
        // Both indexed tags must agree with the payload. `cst-seq` is compared
        // after parsing it as a number, not as text: the tag is decimal, so a
        // string comparison would call seq 10 older than seq 9.
        let tag_target_disagrees =
            tag_value(event, "cs-target").is_some_and(|tagged| tagged != target_key);
        let tag_seq_disagrees = tag_value(event, "cst-seq")
            .is_some_and(|tagged| tagged.parse::<u64>().ok() != Some(envelope.event_seq));
        if tag_target_disagrees || tag_seq_disagrees {
            stats.malformed += 1;
            continue;
        }
        records.push(TranscriptRecord {
            id,
            signer,
            created_at,
            target_key,
            seq: envelope.event_seq,
            envelope,
            raw: event.clone(),
        });
    }
    (records, stats)
}

/// Order transcript items the way the producer reserved them: by numeric
/// sequence, then by arrival, then by id so the order is total.
pub fn sort_transcripts(records: &mut [TranscriptRecord]) {
    records.sort_by(|left, right| {
        left.seq
            .cmp(&right.seq)
            .then(left.created_at.cmp(&right.created_at))
            .then(left.id.cmp(&right.id))
    });
}

/// Keep only the items belonging to one exact generation.
pub fn filter_transcripts_by_target(
    records: &[TranscriptRecord],
    target_key: &str,
) -> Vec<TranscriptRecord> {
    records
        .iter()
        .filter(|record| record.target_key == target_key)
        .cloned()
        .collect()
}

// ── Session resolution ───────────────────────────────────────────────────────

/// One resolved generation, as `sessions list` reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    /// The structured `cs-target` key — the value every other command takes.
    pub target_key: String,
    pub target: CodingSessionTarget,
    pub signer: String,
    pub title: Option<String>,
    pub status: String,
    pub model: Option<String>,
    /// Earliest event seen for this generation.
    pub created_at: i64,
    /// Latest event seen for this generation.
    pub last_event_at: i64,
    pub transcript_items: usize,
    /// A lifecycle receipt from this signer names this generation.
    pub confirmed: bool,
    /// Distinct metadata payloads that shared the newest second.
    pub metadata_conflicts: usize,
}

fn status_string(metadata: &SessionMetadata) -> String {
    serde_json::to_value(metadata.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Infer a status from the transcript when no metadata has arrived.
///
/// Only a terminal item says anything definite; anything else means the
/// provider was still emitting, which is what `running` reports.
fn infer_status(records: &[TranscriptRecord]) -> String {
    let Some(latest) = records.iter().max_by_key(|record| record.seq) else {
        return "unknown".to_owned();
    };
    let item = &latest.envelope.item;
    match item.get("kind").and_then(Value::as_str) {
        Some("interrupted") => "interrupted".to_owned(),
        Some("result") => match item.get("subtype").and_then(Value::as_str) {
            Some("cancelled") => "interrupted".to_owned(),
            Some("error") => "failed".to_owned(),
            Some("success") => "completed".to_owned(),
            _ if item.get("isError").and_then(Value::as_bool) == Some(true) => "failed".to_owned(),
            _ => "running".to_owned(),
        },
        _ => "running".to_owned(),
    }
}

/// Build one row per (signer, generation) the channel has events for.
///
/// A generation is discoverable from any of its three streams: a receipt that
/// created it, metadata that describes it, or transcript items that came out of
/// it. None is required — a session must never become invisible because one
/// stream is late.
pub fn resolve_sessions(
    metadata: &[MetadataRecord],
    receipts: &[ReceiptRecord],
    transcripts: &[TranscriptRecord],
) -> Vec<SessionRow> {
    // A turn receipt names the generation its command targeted, but per
    // NIP-CSL it never creates, confirms, or ends one — only the six lifecycle
    // statuses do that. Which statuses are turn stages is `buzz-core`'s
    // question, asked in `is_turn_receipt_status`. Filtering here, once, keeps
    // every rule below exactly as it read before turn receipts existed.
    let receipts: Vec<&ReceiptRecord> = receipts
        .iter()
        .filter(|record| !record.is_turn_status)
        .collect();

    let mut identities: BTreeMap<(String, String), CodingSessionTarget> = BTreeMap::new();
    for record in metadata {
        identities.insert(
            (record.signer.clone(), record.target_key.clone()),
            record.metadata.session.clone(),
        );
    }
    for record in transcripts {
        identities.insert(
            (record.signer.clone(), record.target_key.clone()),
            record.envelope.session.clone(),
        );
    }

    let confirmed: HashSet<(String, String)> = receipts
        .iter()
        .filter_map(|record| {
            record
                .target_key
                .as_ref()
                .map(|key| (record.signer.clone(), key.clone()))
        })
        .collect();
    // A receipt alone is enough to know a generation exists, even before its
    // first metadata or transcript event lands.
    for record in &receipts {
        if let (Some(key), Some(target)) = (record.target_key.clone(), record.target.clone()) {
            identities
                .entry((record.signer.clone(), key))
                .or_insert(target);
        }
    }

    let mut rows: Vec<SessionRow> = identities
        .into_iter()
        .map(|((signer, target_key), target)| {
            let own_metadata: Vec<&MetadataRecord> = metadata
                .iter()
                .filter(|record| record.signer == signer && record.target_key == target_key)
                .collect();
            let (newest, conflicts) = newest_metadata(&own_metadata);
            let own_transcripts: Vec<&TranscriptRecord> = transcripts
                .iter()
                .filter(|record| record.signer == signer && record.target_key == target_key)
                .collect();

            let mut timestamps: Vec<i64> = own_metadata
                .iter()
                .map(|record| record.created_at)
                .chain(own_transcripts.iter().map(|record| record.created_at))
                .chain(
                    receipts
                        .iter()
                        .filter(|record| {
                            record.signer == signer
                                && record.target_key.as_deref() == Some(target_key.as_str())
                        })
                        .map(|record| record.created_at),
                )
                .collect();
            timestamps.sort_unstable();

            let owned: Vec<TranscriptRecord> = own_transcripts
                .iter()
                .map(|record| (*record).clone())
                .collect();
            let status = newest
                .map(|record| status_string(&record.metadata))
                .unwrap_or_else(|| infer_status(&owned));

            SessionRow {
                target_key: target_key.clone(),
                target,
                signer: signer.clone(),
                title: newest.and_then(|record| record.metadata.title.clone()),
                status,
                model: newest.and_then(|record| record.metadata.model.clone()),
                created_at: timestamps.first().copied().unwrap_or_default(),
                last_event_at: timestamps.last().copied().unwrap_or_default(),
                transcript_items: own_transcripts.len(),
                confirmed: confirmed.contains(&(signer, target_key)),
                metadata_conflicts: conflicts,
            }
        })
        .collect();

    rows.sort_by(|left, right| {
        right
            .last_event_at
            .cmp(&left.last_event_at)
            .then(left.target_key.cmp(&right.target_key))
            .then(left.signer.cmp(&right.signer))
    });
    rows
}

/// Pick the metadata a reader should believe, and report the ambiguity.
///
/// Metadata is last-writer-wins over a second-granularity clock, and the writer
/// legitimately publishes several states inside one second. Two payloads
/// sharing the newest second are resolved on event id rather than left as a
/// conflict — a burst must never wedge a session on a disagreement it will
/// never resolve — but the count of distinct payloads is still reported so the
/// ambiguity stays visible.
fn newest_metadata<'a>(records: &[&'a MetadataRecord]) -> (Option<&'a MetadataRecord>, usize) {
    let Some(newest_at) = records.iter().map(|record| record.created_at).max() else {
        return (None, 0);
    };
    let mut newest: Vec<&&MetadataRecord> = records
        .iter()
        .filter(|record| record.created_at == newest_at)
        .collect();
    newest.sort_by(|left, right| left.id.cmp(&right.id));
    let distinct: HashSet<&str> = newest
        .iter()
        .map(|record| record.canonical.as_str())
        .collect();
    (
        newest.first().map(|record| **record),
        distinct.len().saturating_sub(1),
    )
}

// ── Tool aggregation ─────────────────────────────────────────────────────────

/// Per-tool call and failure counts.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolStat {
    pub tool_name: String,
    pub calls: usize,
    pub errors: usize,
}

impl ToolStat {
    fn error_rate(&self) -> f64 {
        if self.calls == 0 {
            0.0
        } else {
            self.errors as f64 / self.calls as f64
        }
    }
}

/// The whole `sessions tools` answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolReport {
    pub tools: Vec<ToolStat>,
    /// Every item kind seen, with unrecognized kinds folded into `other`.
    pub item_kinds: BTreeMap<String, usize>,
    pub transcript_items: usize,
}

/// Aggregate tool usage across transcript items.
///
/// A `tool_result` names its tool only sometimes, so results are joined back to
/// their call by `toolId`. A result whose call was never seen (a transcript
/// fetched from the middle of a session) still counts, under `(unknown)` —
/// dropping it would quietly deflate the error rate, which is the one number
/// this command exists to report.
pub fn aggregate_tools(records: &[TranscriptRecord]) -> ToolReport {
    let mut sorted: Vec<TranscriptRecord> = records.to_vec();
    sort_transcripts(&mut sorted);

    let mut names_by_id: HashMap<String, String> = HashMap::new();
    let mut calls: BTreeMap<String, usize> = BTreeMap::new();
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    let mut item_kinds: BTreeMap<String, usize> = BTreeMap::new();

    for record in &sorted {
        let item = &record.envelope.item;
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("other");
        let bucket = if KNOWN_ITEM_KINDS.contains(&kind) {
            kind
        } else {
            "other"
        };
        *item_kinds.entry(bucket.to_owned()).or_default() += 1;

        match kind {
            "tool_call" => {
                let tool = item.get("tool");
                let name = tool
                    .and_then(|tool| tool.get("toolName"))
                    .and_then(Value::as_str)
                    .unwrap_or("(unknown)")
                    .to_owned();
                if let Some(id) = tool
                    .and_then(|tool| tool.get("toolId"))
                    .and_then(Value::as_str)
                {
                    names_by_id.insert(id.to_owned(), name.clone());
                }
                *calls.entry(name).or_default() += 1;
            }
            "tool_result" => {
                let name = item
                    .get("toolName")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        item.get("toolId")
                            .and_then(Value::as_str)
                            .and_then(|id| names_by_id.get(id).cloned())
                    })
                    .unwrap_or_else(|| "(unknown)".to_owned());
                calls.entry(name.clone()).or_default();
                if item.get("isError").and_then(Value::as_bool) == Some(true) {
                    *errors.entry(name).or_default() += 1;
                }
            }
            _ => {}
        }
    }

    let mut tools: Vec<ToolStat> = calls
        .into_iter()
        .map(|(tool_name, count)| {
            let failures = errors.get(&tool_name).copied().unwrap_or_default();
            ToolStat {
                tool_name,
                calls: count,
                errors: failures,
            }
        })
        .collect();
    tools.sort_by(|left, right| {
        right
            .calls
            .cmp(&left.calls)
            .then(left.tool_name.cmp(&right.tool_name))
    });

    ToolReport {
        tools,
        item_kinds,
        transcript_items: sorted.len(),
    }
}

// ── Markdown rendering ───────────────────────────────────────────────────────

fn item_text(item: &Value, key: &str) -> Option<String> {
    item.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Render one generation's transcript as prose.
///
/// Turns are the unit a reader thinks in, so items are grouped by `turnId` in
/// sequence order; items outside any turn (session init, status) open the
/// document instead of inventing a turn for themselves.
pub fn render_markdown(row: Option<&SessionRow>, records: &[TranscriptRecord]) -> String {
    let mut sorted: Vec<TranscriptRecord> = records.to_vec();
    sort_transcripts(&mut sorted);

    // Results are joined back to their calls so each call line can say how it
    // ended without the reader scrolling to find out.
    let mut result_by_id: HashMap<String, bool> = HashMap::new();
    for record in &sorted {
        let item = &record.envelope.item;
        if item.get("kind").and_then(Value::as_str) == Some("tool_result") {
            if let Some(id) = item.get("toolId").and_then(Value::as_str) {
                result_by_id.insert(
                    id.to_owned(),
                    item.get("isError").and_then(Value::as_bool) == Some(true),
                );
            }
        }
    }

    let mut out = String::new();
    let target = row
        .map(|row| row.target.clone())
        .or_else(|| sorted.first().map(|record| record.envelope.session.clone()));

    let title = row
        .and_then(|row| row.title.clone())
        .or_else(|| target.as_ref().map(|target| target.session_id.clone()))
        .unwrap_or_else(|| "Coding session".to_owned());
    out.push_str(&format!("# {title}\n\n"));
    if let Some(target) = &target {
        out.push_str(&format!(
            "`{}` · session `{}` · generation {}\n\n",
            target.driver, target.session_id, target.generation
        ));
    }
    if let Some(row) = row {
        out.push_str(&format!(
            "status **{}** · {} transcript items · signer `{}`\n\n",
            row.status, row.transcript_items, row.signer
        ));
    }

    let mut current_turn: Option<Option<String>> = None;
    let mut turn_index = 0usize;
    for record in &sorted {
        let turn = record.envelope.turn_id.clone();
        if current_turn.as_ref() != Some(&turn) {
            current_turn = Some(turn.clone());
            match &turn {
                Some(turn_id) => {
                    turn_index += 1;
                    out.push_str(&format!("## Turn {turn_index} (`{turn_id}`)\n\n"));
                }
                None => out.push_str("## Session\n\n"),
            }
        }
        out.push_str(&render_item(&record.envelope.item, &result_by_id));
    }

    condense_redaction_markers(&out)
}

/// Replace privacy redaction markers with a short label and a footnote.
///
/// The provider replaces each host-private or credential-bearing value with
/// `[elided private context: N bytes, sha256:<64 hex>]` before signing (see
/// `buzz_core::coding_session_context`). That is 90 characters of hash dropped
/// mid-sentence, and it is usually hiding a path.
///
/// The digest is not thrown away — it is what distinguishes "the provider had
/// this and chose not to publish it" from "nothing was there", and two readers
/// comparing two transcripts need it. A terminal has no hover, so it moves to
/// a numbered footnote instead: the body reads, the digest stays reachable,
/// and identical values share one footnote number so a repeated redaction is
/// visibly the same value.
///
/// Fenced code blocks are left byte-for-byte. Inside a fence the reader is
/// looking at the bytes, and a substitution would claim they say something
/// they do not.
fn condense_redaction_markers(markdown: &str) -> String {
    let mut digests: Vec<(String, u64)> = Vec::new();
    let mut out = String::with_capacity(markdown.len());
    let mut in_fence = false;

    for line in markdown.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        out.push_str(&condense_redaction_markers_in_line(line, &mut digests));
    }

    if digests.is_empty() {
        return out;
    }

    out.push_str("\n## Redactions\n\n");
    out.push_str(
        "Values this session's provider removed before signing. Each digest is \
         SHA-256 over the value's JSON encoding, so the byte count includes its \
         quoting and escaping.\n\n",
    );
    for (index, (digest, bytes)) in digests.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} bytes — `sha256:{}`\n",
            index + 1,
            bytes,
            digest
        ));
    }
    out.push('\n');
    out
}

fn condense_redaction_markers_in_line(line: &str, digests: &mut Vec<(String, u64)>) -> String {
    const PREFIX: &str = "[elided private context: ";
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    loop {
        let Some(start) = rest.find(PREFIX) else {
            out.push_str(rest);
            return out;
        };
        let tail = &rest[start..];
        let Some(close) = tail.find(']') else {
            // No closing bracket on this line: not a marker this reader can
            // prove, so it stays text rather than eating the rest of the line.
            out.push_str(rest);
            return out;
        };
        let Some((bytes, digest)) = parse_redaction_marker(&tail[..=close]) else {
            out.push_str(&rest[..start + PREFIX.len()]);
            rest = &rest[start + PREFIX.len()..];
            continue;
        };
        out.push_str(&rest[..start]);
        let index = match digests.iter().position(|(seen, _)| *seen == digest) {
            Some(index) => index,
            None => {
                digests.push((digest, bytes));
                digests.len() - 1
            }
        };
        out.push_str(&format!(
            "[redacted {} · #{}]",
            format_redacted_bytes(bytes),
            index + 1
        ));
        rest = &tail[close + 1..];
    }
}

/// Read `[elided private context: N bytes, sha256:<64 lowercase hex>]`.
///
/// Deliberately strict: anything that does not match exactly is not a marker,
/// and is left as the text it is.
fn parse_redaction_marker(candidate: &str) -> Option<(u64, String)> {
    let body = candidate
        .strip_prefix("[elided private context: ")?
        .strip_suffix(']')?;
    let (bytes, digest) = body.split_once(" bytes, sha256:")?;
    let digest = digest.to_owned();
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if digest.bytes().any(|b| b.is_ascii_uppercase()) {
        return None;
    }
    Some((bytes.parse().ok()?, digest))
}

/// Byte counts for humans, in decimal units — the same ramp the desktop pill
/// uses, so one transcript reads the same in both places.
fn format_redacted_bytes(bytes: u64) -> String {
    if bytes < 1_000 {
        return format!("{bytes} B");
    }
    if bytes < 1_000_000 {
        let kilobytes = bytes as f64 / 1_000.0;
        return if kilobytes < 10.0 {
            format!("{kilobytes:.1} KB")
        } else {
            format!("{} KB", kilobytes.round() as u64)
        };
    }
    let megabytes = bytes as f64 / 1_000_000.0;
    if megabytes < 10.0 {
        format!("{megabytes:.1} MB")
    } else {
        format!("{} MB", megabytes.round() as u64)
    }
}

fn render_item(item: &Value, result_by_id: &HashMap<String, bool>) -> String {
    let kind = item.get("kind").and_then(Value::as_str).unwrap_or("other");
    match kind {
        "user_prompt" => {
            let content = item_text(item, "content").unwrap_or_default();
            // `commandId` is present whenever an operator command started the
            // turn: the 44220 `thread.turn.start` for an ordinary turn, or the
            // 44221 create's own commandId for the initial turn embedded in a
            // create — so the id must be resolved against both kinds, and
            // absence marks an older provider rather than a create-embedded
            // turn (see NIP-CST). Rendered as a short footnote so a reader can
            // join this line back to its command without the prose growing.
            let command_note = item_text(item, "commandId")
                .map(|command_id| format!(" _(cmd `{command_id}`)_"))
                .unwrap_or_default();
            format!("**User**{command_note}\n\n{content}\n\n")
        }
        "assistant_text" => {
            let text = item_text(item, "text").unwrap_or_default();
            format!("**Assistant**\n\n{text}\n\n")
        }
        "reasoning" => {
            let text = item_text(item, "text").unwrap_or_default();
            format!("**Reasoning**\n\n{text}\n\n")
        }
        "tool_call" => {
            let tool = item.get("tool");
            let name = tool
                .and_then(|tool| tool.get("toolName"))
                .and_then(Value::as_str)
                .unwrap_or("(unknown)");
            let id = tool
                .and_then(|tool| tool.get("toolId"))
                .and_then(Value::as_str);
            let status = match id.and_then(|id| result_by_id.get(id)) {
                Some(true) => "error",
                Some(false) => "ok",
                None => "no result",
            };
            match id {
                Some(id) => format!("- tool `{name}` (`{id}`) — {status}\n"),
                None => format!("- tool `{name}` — {status}\n"),
            }
        }
        "tool_result" => String::new(),
        "result" => {
            let subtype = item_text(item, "subtype").unwrap_or_else(|| "unknown".to_owned());
            let duration = item
                .get("durationMs")
                .and_then(Value::as_u64)
                .map(|ms| format!(" · {ms} ms"))
                .unwrap_or_default();
            let cost = item
                .get("costUsd")
                .and_then(Value::as_f64)
                .map(|usd| format!(" · ${usd:.4}"))
                .unwrap_or_default();
            format!("\n_result: {subtype}{duration}{cost}_\n\n")
        }
        "interrupted" => "\n_interrupted_\n\n".to_owned(),
        "status" => {
            let status = item_text(item, "status").unwrap_or_default();
            format!("_status: {status}_\n\n")
        }
        "elided" => {
            // The *cap*, not a privacy redaction — a different cause, and the
            // reader is owed the difference. The digest rides along so a
            // dropped item stays distinguishable from an absent one.
            let size = item
                .get("byteCount")
                .and_then(Value::as_u64)
                .map(format_redacted_bytes)
                .unwrap_or_else(|| "an unknown amount".to_owned());
            let digest = item
                .get("contentDigest")
                .and_then(Value::as_str)
                .filter(|digest| !digest.is_empty())
                .map(|digest| format!(" · `sha256:{}`", digest.trim_start_matches("sha256:")))
                .unwrap_or_default();
            format!("_[dropped {size} — did not fit the event cap]_{digest}\n\n")
        }
        other => format!("_[{other}]_\n\n"),
    }
}

// ── Export ───────────────────────────────────────────────────────────────────

/// One generation's slice of an export.
#[derive(Debug, Clone)]
pub struct ExportGroup {
    pub file_name: String,
    pub row: SessionRow,
    pub events: Vec<Value>,
}

/// Group every event by generation and assign each group its output file name.
///
/// `<sessionId>-g<generation>.jsonl` is the name; when two signers published
/// under the same session id and generation, every colliding group takes the
/// signer suffix instead. Silently letting one overwrite the other would lose a
/// whole session to a name clash.
pub fn plan_export(
    rows: &[SessionRow],
    metadata: &[MetadataRecord],
    receipts: &[ReceiptRecord],
    transcripts: &[TranscriptRecord],
) -> Vec<ExportGroup> {
    let mut base_counts: BTreeMap<String, usize> = BTreeMap::new();
    for row in rows {
        *base_counts
            .entry(export_base_name(&row.target))
            .or_default() += 1;
    }

    rows.iter()
        .map(|row| {
            let base = export_base_name(&row.target);
            let file_name = if base_counts.get(&base).copied().unwrap_or_default() > 1 {
                format!("{base}-{}.jsonl", row.signer)
            } else {
                format!("{base}.jsonl")
            };

            let mut events: Vec<(i64, u64, String, Value)> = Vec::new();
            for record in metadata {
                if record.signer == row.signer && record.target_key == row.target_key {
                    events.push((record.created_at, 0, record.id.clone(), record.raw.clone()));
                }
            }
            for record in receipts {
                if record.signer == row.signer
                    && record.target_key.as_deref() == Some(row.target_key.as_str())
                {
                    let id = event_str(&record.raw, "id").unwrap_or_default();
                    events.push((record.created_at, 0, id, record.raw.clone()));
                }
            }
            for record in transcripts {
                if record.signer == row.signer && record.target_key == row.target_key {
                    events.push((
                        record.created_at,
                        record.seq,
                        record.id.clone(),
                        record.raw.clone(),
                    ));
                }
            }
            events.sort_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then(left.1.cmp(&right.1))
                    .then(left.2.cmp(&right.2))
            });

            ExportGroup {
                file_name,
                row: row.clone(),
                events: events.into_iter().map(|(_, _, _, raw)| raw).collect(),
            }
        })
        .collect()
}

fn export_base_name(target: &CodingSessionTarget) -> String {
    let session = sanitize_segment(&target.session_id);
    format!("{session}-g{}", target.generation)
}

/// Reduce an identifier to something safe to name a file with.
///
/// Session ids are producer-minted UUIDs today, but the wire contract only
/// bounds them to 512 bytes — a path separator in one must not become a
/// directory traversal in an export.
fn sanitize_segment(value: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' || character == '.' {
            cleaned.push(character);
        } else if !cleaned.ends_with('-') {
            cleaned.push('-');
        }
    }
    // Leading dots go too, so a segment can never come out as `.` or `..`.
    let trimmed = cleaned.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "session".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Build the manifest that makes an export directory self-describing.
pub fn build_manifest(channel_id: &str, groups: &[ExportGroup], exported_at: &str) -> Value {
    let targets: Vec<Value> = groups
        .iter()
        .map(|group| {
            json!({
                "file": group.file_name,
                "target": group.row.target_key,
                "driver": group.row.target.driver,
                "instanceId": group.row.target.instance_id,
                "sessionId": group.row.target.session_id,
                "generation": group.row.target.generation,
                "signer": group.row.signer,
                "title": group.row.title,
                "status": group.row.status,
                "confirmed": group.row.confirmed,
                "events": group.events.len(),
                "transcriptItems": group.row.transcript_items,
                "firstEventAt": rfc3339(group.row.created_at),
                "lastEventAt": rfc3339(group.row.last_event_at),
            })
        })
        .collect();

    let first = groups.iter().map(|group| group.row.created_at).min();
    let last = groups.iter().map(|group| group.row.last_event_at).max();

    json!({
        "version": 1,
        "channel": channel_id,
        "exportedAt": exported_at,
        "counts": {
            "generations": groups.len(),
            "events": groups.iter().map(|group| group.events.len()).sum::<usize>(),
        },
        "timeRange": {
            "firstEventAt": first.map(rfc3339),
            "lastEventAt": last.map(rfc3339),
        },
        "targets": targets,
    })
}

fn rfc3339(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|time| time.to_rfc3339())
        .unwrap_or_else(|| seconds.to_string())
}

// ── Relay access ─────────────────────────────────────────────────────────────

/// Fetch every event of `kinds` in one channel.
///
/// `kinds` is never omitted and never empty: an open-ended filter trips the
/// relay's p-gate and comes back 403.
async fn fetch_channel_events(
    client: &BuzzClient,
    channel_id: &str,
    kinds: &[u32],
) -> Result<Vec<Value>, CliError> {
    let filter = json!({ "kinds": kinds, "#h": [channel_id] });
    client.query_all(filter).await
}

// ── Commands ─────────────────────────────────────────────────────────────────

async fn cmd_list(
    client: &BuzzClient,
    channel_id: &str,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let events = fetch_channel_events(
        client,
        channel_id,
        &[
            KIND_CODING_SESSION_METADATA,
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        ],
    )
    .await?;
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let rows = resolve_sessions(&metadata, &receipts, &[]);

    let output: Vec<Value> = rows
        .iter()
        .map(|row| match format {
            crate::OutputFormat::Compact => json!({
                "target": row.target_key,
                "title": row.title,
                "status": row.status,
                "model": row.model,
                "createdAt": rfc3339(row.created_at),
            }),
            crate::OutputFormat::Json => json!({
                "target": row.target_key,
                "driver": row.target.driver,
                "instanceId": row.target.instance_id,
                "sessionId": row.target.session_id,
                "generation": row.target.generation,
                "signer": row.signer,
                "title": row.title,
                "status": row.status,
                "model": row.model,
                "createdAt": rfc3339(row.created_at),
                "lastEventAt": rfc3339(row.last_event_at),
                "confirmed": row.confirmed,
                "metadataConflicts": row.metadata_conflicts,
            }),
        })
        .collect();

    println!("{}", Value::Array(output));
    Ok(())
}

/// Turn `--target` or `--session` into one exact generation.
async fn resolve_target(
    client: &BuzzClient,
    channel_id: &str,
    target: Option<&str>,
    session: Option<&str>,
) -> Result<(String, Option<SessionRow>), CliError> {
    let events = fetch_channel_events(
        client,
        channel_id,
        &[
            KIND_CODING_SESSION_METADATA,
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        ],
    )
    .await?;
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let rows = resolve_sessions(&metadata, &receipts, &[]);

    if let Some(target) = target {
        let row = rows.iter().find(|row| row.target_key == target).cloned();
        return Ok((target.to_owned(), row));
    }

    let session = session
        .ok_or_else(|| CliError::Usage("one of --target or --session is required".to_owned()))?;
    let matches: Vec<&SessionRow> = rows
        .iter()
        .filter(|row| row.target.session_id == session)
        .collect();
    match matches.as_slice() {
        [] => Err(CliError::NotFound(format!(
            "no coding session with id '{session}' in channel {channel_id}"
        ))),
        [row] => Ok((row.target_key.clone(), Some((*row).clone()))),
        many => Err(CliError::Usage(format!(
            "session id '{session}' matches {} generations — pass --target with one of: {}",
            many.len(),
            many.iter()
                .map(|row| row.target_key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

async fn cmd_transcript(
    client: &BuzzClient,
    channel_id: &str,
    target: Option<&str>,
    session: Option<&str>,
    format: &crate::TranscriptFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let (target_key, row) = resolve_target(client, channel_id, target, session).await?;

    let events =
        fetch_channel_events(client, channel_id, &[KIND_CODING_SESSION_TRANSCRIPT]).await?;
    let (all, _) = decode_transcripts(&events);
    let mut records = filter_transcripts_by_target(&all, &target_key);
    sort_transcripts(&mut records);

    match format {
        crate::TranscriptFormat::Jsonl => {
            for record in &records {
                println!("{}", record.raw);
            }
        }
        crate::TranscriptFormat::Md => {
            print!("{}", render_markdown(row.as_ref(), &records));
        }
    }
    Ok(())
}

async fn cmd_tools(
    client: &BuzzClient,
    channel_id: &str,
    target: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let events =
        fetch_channel_events(client, channel_id, &[KIND_CODING_SESSION_TRANSCRIPT]).await?;
    let (all, stats) = decode_transcripts(&events);
    let records = match target {
        Some(target) => filter_transcripts_by_target(&all, target),
        None => all,
    };
    let report = aggregate_tools(&records);

    let tools: Vec<Value> = report
        .tools
        .iter()
        .map(|stat| {
            json!({
                "toolName": stat.tool_name,
                "calls": stat.calls,
                "errors": stat.errors,
                "errorRate": stat.error_rate(),
            })
        })
        .collect();

    match format {
        crate::OutputFormat::Compact => println!("{}", Value::Array(tools)),
        crate::OutputFormat::Json => println!(
            "{}",
            json!({
                "channel": channel_id,
                "target": target,
                "transcriptItems": report.transcript_items,
                "malformedEvents": stats.malformed,
                "itemKinds": report.item_kinds,
                "tools": tools,
            })
        ),
    }
    Ok(())
}

// ── Turn diagnosis ───────────────────────────────────────────────────────────

/// What one turn's transcript says about how it ended.
///
/// This is the triage that was done by hand, in Python, against the 2026-08-24
/// "project repo access" session: group by turn, find the ones whose span is
/// suspiciously close to a timeout budget, check whether any tool call was left
/// unterminated, and read the result's token counts. Doing it by hand took an
/// hour and the interesting fact — that no SDK result had ever arrived — was
/// nearly missed. It is a command now.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnDiagnosis {
    pub turn_id: Option<String>,
    /// Wall time from the turn's first item to its last, in seconds.
    pub span_secs: f64,
    /// Unix seconds of the first item, for correlating with anything else.
    pub started_at: i64,
    pub items: usize,
    /// Tool calls with no matching `tool_result`, by name.
    pub unterminated_tools: Vec<String>,
    /// Offset of the last tool frame from the turn's start, in seconds.
    pub last_tool_offset_secs: Option<f64>,
    /// Whether the terminal `result` reported an error.
    pub is_error: Option<bool>,
    /// The terminal `result`'s own words.
    pub result: Option<String>,
    /// Token counts as the `result` reported them.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// The `turn_wire:` row, when the producer published one.
    pub wire: Option<String>,
    /// Everything this turn tripped, in the words a reader needs.
    pub findings: Vec<String>,
}

/// Whether a failed turn's own result proves the agent never answered it.
///
/// A provider-synthesized timeout result carries no usage, because there was no
/// SDK result to take usage from. A turn that genuinely errored downstream
/// still has real counts. That difference is the single clearest signal that a
/// prompt was dropped rather than a turn going wrong, and it is invisible
/// unless someone thinks to compare against a healthy turn.
fn result_has_no_usage(input: Option<u64>, output: Option<u64>) -> bool {
    matches!((input, output), (None, None) | (Some(0), Some(0)))
}

/// Group a generation's items by turn and report on each.
pub fn diagnose_turns(records: &[TranscriptRecord]) -> Vec<TurnDiagnosis> {
    let mut order: Vec<Option<String>> = Vec::new();
    let mut by_turn: HashMap<Option<String>, Vec<&TranscriptRecord>> = HashMap::new();
    for record in records {
        let key = record.envelope.turn_id.clone();
        if !by_turn.contains_key(&key) {
            order.push(key.clone());
        }
        by_turn.entry(key).or_default().push(record);
    }

    order
        .into_iter()
        .filter_map(|key| {
            let mut items = by_turn.remove(&key)?;
            items.sort_by_key(|record| record.seq);
            let first = items.first()?;
            let last = items.last()?;
            let start_ms = first.envelope.timestamp;
            let span_secs = (last.envelope.timestamp - start_ms) as f64 / 1000.0;

            let mut called: Vec<(String, String)> = Vec::new();
            let mut resulted: HashSet<String> = HashSet::new();
            let mut last_tool_ms: Option<i64> = None;
            let mut wire: Option<String> = None;
            let (mut is_error, mut result, mut input_tokens, mut output_tokens) =
                (None, None, None, None);

            for record in &items {
                let item = &record.envelope.item;
                match item.get("kind").and_then(Value::as_str) {
                    Some("tool_call") => {
                        if let Some(id) = item.pointer("/tool/toolId").and_then(Value::as_str) {
                            let name = item
                                .pointer("/tool/toolName")
                                .and_then(Value::as_str)
                                .unwrap_or("(unnamed)");
                            called.push((id.to_owned(), name.to_owned()));
                        }
                        last_tool_ms = Some(record.envelope.timestamp);
                    }
                    Some("tool_result") => {
                        if let Some(id) = item.get("toolId").and_then(Value::as_str) {
                            resulted.insert(id.to_owned());
                        }
                        last_tool_ms = Some(record.envelope.timestamp);
                    }
                    Some("status") => {
                        if let Some(status) = item.get("status").and_then(Value::as_str) {
                            if status.starts_with("turn_wire: ") {
                                wire = Some(status.to_owned());
                            }
                        }
                    }
                    Some("result") => {
                        is_error = item.get("isError").and_then(Value::as_bool);
                        result = item
                            .get("result")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        input_tokens = item.get("inputTokens").and_then(Value::as_u64);
                        output_tokens = item.get("outputTokens").and_then(Value::as_u64);
                    }
                    _ => {}
                }
            }

            let unterminated: Vec<String> = called
                .iter()
                .filter(|(id, _)| !resulted.contains(id))
                .map(|(id, name)| format!("{name} ({id})"))
                .collect();
            let last_tool_offset_secs = last_tool_ms.map(|at| (at - start_ms) as f64 / 1000.0);

            let mut findings = Vec::new();
            if is_error == Some(true) {
                if result_has_no_usage(input_tokens, output_tokens) {
                    findings.push(
                        "no usage on the result — the provider synthesized it, so the agent \
                         never resolved this prompt"
                            .to_owned(),
                    );
                }
                if unterminated.is_empty() {
                    findings.push(
                        "every tool call terminated — nothing was outstanding when it died"
                            .to_owned(),
                    );
                } else {
                    findings.push(format!(
                        "{} tool call(s) never terminated: {}",
                        unterminated.len(),
                        unterminated.join(", ")
                    ));
                }
                if let Some(offset) = last_tool_offset_secs {
                    let quiet = span_secs - offset;
                    if quiet > 60.0 {
                        findings.push(format!(
                            "{quiet:.0}s between the last tool frame and the end of the turn"
                        ));
                    }
                }
                if wire.is_none() {
                    findings.push(
                        "no turn_wire row — this generation predates it, so the quiet-onset \
                         above is inferred from item timestamps, not measured"
                            .to_owned(),
                    );
                }
            }

            Some(TurnDiagnosis {
                turn_id: key,
                span_secs,
                started_at: start_ms / 1000,
                items: items.len(),
                unterminated_tools: unterminated,
                last_tool_offset_secs,
                is_error,
                result,
                input_tokens,
                output_tokens,
                wire,
                findings,
            })
        })
        .collect()
}

async fn cmd_doctor(
    client: &BuzzClient,
    channel_id: &str,
    target: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let events =
        fetch_channel_events(client, channel_id, &[KIND_CODING_SESSION_TRANSCRIPT]).await?;
    let (all, stats) = decode_transcripts(&events);
    let mut records = match target {
        Some(target) => filter_transcripts_by_target(&all, target),
        None => all,
    };
    sort_transcripts(&mut records);
    let turns = diagnose_turns(&records);

    let rows: Vec<Value> = turns
        .iter()
        .map(|turn| {
            json!({
                "turnId": turn.turn_id,
                "startedAt": rfc3339(turn.started_at),
                "spanSecs": turn.span_secs,
                "items": turn.items,
                "isError": turn.is_error,
                "result": turn.result,
                "inputTokens": turn.input_tokens,
                "outputTokens": turn.output_tokens,
                "unterminatedTools": turn.unterminated_tools,
                "lastToolOffsetSecs": turn.last_tool_offset_secs,
                "wire": turn.wire,
                "findings": turn.findings,
            })
        })
        .collect();

    match format {
        crate::OutputFormat::Compact => {
            for turn in &turns {
                // Items with no turn id are session-level facts — the opening
                // `session_fresh` status, say. Calling that group "unfinished"
                // reports a turn that never existed as one that failed to end,
                // which is the same class of lie this command exists to catch.
                let id = turn
                    .turn_id
                    .as_deref()
                    .unwrap_or("(session-level, outside any turn)");
                let verdict = match (turn.turn_id.is_some(), turn.is_error) {
                    (false, _) => "-",
                    (true, Some(true)) => "FAILED",
                    (true, Some(false)) => "ok",
                    (true, None) => "unfinished",
                };
                println!(
                    "{id}  {verdict}  {:.1}s  {} items{}",
                    turn.span_secs,
                    turn.items,
                    if turn.unterminated_tools.is_empty() {
                        String::new()
                    } else {
                        format!("  {} unterminated", turn.unterminated_tools.len())
                    }
                );
                for finding in &turn.findings {
                    println!("    - {finding}");
                }
            }
        }
        crate::OutputFormat::Json => println!(
            "{}",
            json!({
                "channel": channel_id,
                "target": target,
                "malformedEvents": stats.malformed,
                "turns": rows,
            })
        ),
    }
    Ok(())
}

async fn cmd_export(client: &BuzzClient, channel_id: &str, out: &str) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let directory = Path::new(out);
    prepare_export_dir(directory)?;

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
    let rows = resolve_sessions(&metadata, &receipts, &transcripts);
    let groups = plan_export(&rows, &metadata, &receipts, &transcripts);

    for group in &groups {
        let mut body = String::new();
        for event in &group.events {
            body.push_str(&event.to_string());
            body.push('\n');
        }
        let path = directory.join(&group.file_name);
        fs::write(&path, body).map_err(|error| {
            CliError::Other(format!("failed to write {}: {error}", path.display()))
        })?;
    }

    let manifest = build_manifest(channel_id, &groups, &chrono::Utc::now().to_rfc3339());
    let manifest_path = directory.join("manifest.json");
    let rendered = serde_json::to_string_pretty(&manifest)
        .map_err(|error| CliError::Other(format!("failed to render manifest: {error}")))?;
    fs::write(&manifest_path, format!("{rendered}\n")).map_err(|error| {
        CliError::Other(format!(
            "failed to write {}: {error}",
            manifest_path.display()
        ))
    })?;

    println!("{manifest}");
    Ok(())
}

/// Refuse to write into a directory that already holds anything.
///
/// An export is a claim about a directory's whole contents; merging into an
/// occupied one produces a manifest that does not describe the files beside it.
fn prepare_export_dir(directory: &Path) -> Result<(), CliError> {
    if directory.exists() {
        if !directory.is_dir() {
            return Err(CliError::Usage(format!(
                "--out {} exists and is not a directory",
                directory.display()
            )));
        }
        let mut entries = fs::read_dir(directory).map_err(|error| {
            CliError::Other(format!("failed to read {}: {error}", directory.display()))
        })?;
        if entries.next().is_some() {
            return Err(CliError::Usage(format!(
                "--out {} is not empty; exports never overwrite an existing directory",
                directory.display()
            )));
        }
        return Ok(());
    }
    fs::create_dir_all(directory).map_err(|error| {
        CliError::Other(format!("failed to create {}: {error}", directory.display()))
    })
}

// ── NIP-CSAT authority chain (kind 44228 + kind 40099 receipts) ──────────────

/// The receipt `content.type` the relay stamps on an accepted transition.
const AUTHORITY_RECEIPT_TYPE: &str = "coding_session_authority_transition_accepted";

/// One relay-emitted kind:40099 acceptance receipt for a 44228 transition,
/// decoded from its system-message content.
///
/// CLI-grade trust: receipts are matched by `acceptedEventId` and
/// `content.type` from the relay the caller authenticated to. Full
/// relay-signature verification of each receipt is the session providers'
/// job, not this read surface's.
#[derive(Debug, Clone)]
pub struct AuthorityReceipt {
    /// Event id of the accepted 44228 transition.
    pub accepted_event_id: String,
    /// The chain's sequence number this receipt confirms.
    pub seq: u32,
    /// Transition type (`grant-operator` | `grant-viewer` | `revoke`).
    pub transition_type: String,
    /// The pubkey the transition targeted.
    pub grantee_pubkey: String,
}

/// Decode the acceptance receipts for one genesis out of a batch of raw
/// kind:40099 events. Non-receipt system messages and receipts for other
/// chains are skipped.
pub fn decode_authority_receipts(events: &[Value], genesis: &str) -> Vec<AuthorityReceipt> {
    events
        .iter()
        .filter_map(|event| {
            let content: Value = serde_json::from_str(content_of(event)?).ok()?;
            if content.get("type")?.as_str()? != AUTHORITY_RECEIPT_TYPE {
                return None;
            }
            if content.get("genesisRef")?.as_str()? != genesis {
                return None;
            }
            Some(AuthorityReceipt {
                accepted_event_id: content.get("acceptedEventId")?.as_str()?.to_owned(),
                seq: u32::try_from(content.get("seq")?.as_u64()?).ok()?,
                transition_type: content.get("transitionType")?.as_str()?.to_owned(),
                grantee_pubkey: content.get("granteePubkey")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

/// The accepted state of one session's authority chain, folded from its
/// receipts in sequence order.
#[derive(Debug, Clone, Default)]
pub struct AuthorityChainState {
    /// Event id of the accepted head transition, `None` for an empty chain.
    pub head_event_id: Option<String>,
    /// Sequence number of the accepted head (0 for an empty chain).
    pub head_seq: u32,
    /// Live grants: pubkey → CLI role (`collaborator` for `grant-operator`
    /// grants, `viewer` for `grant-viewer`). A `revoke` removes the entry.
    pub grants: BTreeMap<String, String>,
}

/// Fold acceptance receipts into the chain's live grant map and head.
pub fn fold_authority_receipts(mut receipts: Vec<AuthorityReceipt>) -> AuthorityChainState {
    receipts.sort_by(|a, b| {
        a.seq
            .cmp(&b.seq)
            .then_with(|| a.accepted_event_id.cmp(&b.accepted_event_id))
    });
    let mut state = AuthorityChainState::default();
    for receipt in receipts {
        match receipt.transition_type.as_str() {
            "grant-operator" => {
                state
                    .grants
                    .insert(receipt.grantee_pubkey.clone(), "collaborator".to_owned());
            }
            "grant-viewer" => {
                state
                    .grants
                    .insert(receipt.grantee_pubkey.clone(), "viewer".to_owned());
            }
            "revoke" => {
                state.grants.remove(&receipt.grantee_pubkey);
            }
            // Future transition types change the chain in ways this build
            // cannot interpret; they still advance the head below.
            _ => {}
        }
        state.head_seq = receipt.seq;
        state.head_event_id = Some(receipt.accepted_event_id);
    }
    state
}

/// Validate a 64-character lowercase-hex id (genesis event id or pubkey).
fn validate_lower_hex64(label: &str, value: &str) -> Result<(), CliError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(CliError::Usage(format!(
            "{label} must be a 64-character lowercase hex string: {value}"
        )));
    }
    Ok(())
}

/// Fetch the raw kind:40099 receipts for a channel and fold the chain state
/// for one genesis.
async fn fetch_authority_state(
    client: &BuzzClient,
    channel_id: &str,
    genesis: &str,
) -> Result<AuthorityChainState, CliError> {
    let receipts = fetch_channel_events(client, channel_id, &[KIND_SYSTEM_MESSAGE]).await?;
    Ok(fold_authority_receipts(decode_authority_receipts(
        &receipts, genesis,
    )))
}

/// Whether a submit failure looks like a lost chain-head race (the relay
/// names the `prevAccepted`/`seq` linkage it expected) — the one failure
/// worth a refetch-and-retry.
fn is_chain_head_conflict(error: &CliError) -> bool {
    let message = error.to_string();
    message.contains("prevAccepted") || message.contains("seq")
}

/// Build, sign, and submit one 44228 transition extending the chain's
/// current accepted head; on a head race, refetch and retry once.
///
/// The envelope is pinned to exactly three two-field tags (`h`, `csat-v`,
/// `csat-genesis`), so the event is signed without NIP-OA auth-tag
/// injection — the chain's authority model is the signature itself (the
/// relay checks the signer against the session owner).
async fn submit_authority_transition(
    client: &BuzzClient,
    channel_id: &str,
    genesis: &str,
    transition_type: CodingSessionAuthorityTransitionType,
    grantee: &str,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    validate_lower_hex64("--genesis", genesis)?;
    validate_lower_hex64("--pubkey", grantee)?;

    for attempt in 0..2 {
        let state = fetch_authority_state(client, channel_id, genesis).await?;
        if matches!(
            transition_type,
            CodingSessionAuthorityTransitionType::Revoke
        ) && !state.grants.contains_key(grantee)
        {
            return Err(CliError::NotFound(format!(
                "pubkey {grantee} holds no live grant on this session — nothing to revoke"
            )));
        }

        let payload = CodingSessionAuthorityTransitionPayload::new(
            transition_type,
            genesis,
            state.head_event_id.clone(),
            state
                .head_seq
                .checked_add(1)
                .ok_or_else(|| CliError::Other("authority chain seq overflow".into()))?,
            grantee,
        );
        payload.validate().map_err(CliError::Other)?;
        let content = serde_json::to_string(&payload)
            .map_err(|e| CliError::Other(format!("transition serialization failed: {e}")))?;

        let tags = [
            ["h", channel_id],
            ["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION],
            ["csat-genesis", genesis],
        ]
        .iter()
        .map(|parts| {
            nostr::Tag::parse(parts.iter().copied())
                .map_err(|e| CliError::Other(format!("tag construction failed: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
        let builder = nostr::EventBuilder::new(
            nostr::Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
            &content,
        )
        .tags(tags);
        let event = client.sign_event_unchecked(builder)?;

        let outcome = match client.submit_event(event).await {
            Ok(raw) => {
                crate::commands::parse_write_response(&raw, "authority transition already accepted")
            }
            Err(error) => Err(error),
        };
        match outcome {
            Ok(response) => {
                println!("{response}");
                return Ok(());
            }
            Err(error) if attempt == 0 && is_chain_head_conflict(&error) => {
                // Lost a head race: another transition landed between our
                // read and our write. Refetch the head and try once more.
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("loop returns on the second attempt")
}

/// `bee sessions grant` — role→type mapping: collaborator ⇒ grant-operator,
/// viewer ⇒ grant-viewer.
async fn cmd_grant(
    client: &BuzzClient,
    channel_id: &str,
    genesis: &str,
    pubkey: &str,
    role: crate::GrantRoleArg,
) -> Result<(), CliError> {
    let transition_type = match role {
        crate::GrantRoleArg::Collaborator => CodingSessionAuthorityTransitionType::GrantOperator,
        crate::GrantRoleArg::Viewer => CodingSessionAuthorityTransitionType::GrantViewer,
    };
    submit_authority_transition(client, channel_id, genesis, transition_type, pubkey).await
}

/// `bee sessions revoke`
async fn cmd_revoke(
    client: &BuzzClient,
    channel_id: &str,
    genesis: &str,
    pubkey: &str,
) -> Result<(), CliError> {
    submit_authority_transition(
        client,
        channel_id,
        genesis,
        CodingSessionAuthorityTransitionType::Revoke,
        pubkey,
    )
    .await
}

/// `bee sessions roster` — the folded grant map plus pending (un-receipted)
/// transitions.
async fn cmd_authority_roster(
    client: &BuzzClient,
    channel_id: &str,
    genesis: &str,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    validate_lower_hex64("--genesis", genesis)?;

    let transitions = fetch_channel_events(
        client,
        channel_id,
        &[KIND_CODING_SESSION_AUTHORITY_TRANSITION],
    )
    .await?;
    let receipts_raw = fetch_channel_events(client, channel_id, &[KIND_SYSTEM_MESSAGE]).await?;
    let receipts = decode_authority_receipts(&receipts_raw, genesis);
    let accepted_ids: HashSet<&str> = receipts
        .iter()
        .map(|receipt| receipt.accepted_event_id.as_str())
        .collect();
    let state = fold_authority_receipts(receipts.clone());

    // A transition with no matching receipt is pending: submitted but not
    // (or not yet) accepted as a chain link.
    let pending: Vec<Value> = transitions
        .iter()
        .filter_map(|event| {
            let id = event_str(event, "id")?;
            if accepted_ids.contains(id.as_str()) {
                return None;
            }
            let content: Value = serde_json::from_str(content_of(event)?).ok()?;
            if content.get("genesisRef")?.as_str()? != genesis {
                return None;
            }
            Some(json!({
                "eventId": id,
                "seq": content.get("seq"),
                "type": content.get("type"),
                "granteePubkey": content.get("granteePubkey"),
            }))
        })
        .collect();

    let grants: Vec<Value> = state
        .grants
        .iter()
        .map(|(pubkey, role)| json!({ "pubkey": pubkey, "role": role }))
        .collect();

    println!(
        "{}",
        json!({
            "genesisRef": genesis,
            "headEventId": state.head_event_id,
            "headSeq": state.head_seq,
            "grants": grants,
            "pending": pending,
        })
    );
    Ok(())
}

/// Route one `sessions` subcommand.
pub async fn dispatch(
    cmd: crate::SessionsCmd,
    client: &BuzzClient,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    use crate::SessionsCmd;
    match cmd {
        SessionsCmd::List { channel } => cmd_list(client, &channel, format).await,
        SessionsCmd::Transcript {
            channel,
            target,
            session,
            format: transcript_format,
        } => {
            cmd_transcript(
                client,
                &channel,
                target.as_deref(),
                session.as_deref(),
                &transcript_format,
            )
            .await
        }
        SessionsCmd::Tools { channel, target } => {
            cmd_tools(client, &channel, target.as_deref(), format).await
        }
        SessionsCmd::Doctor { channel, target } => {
            cmd_doctor(client, &channel, target.as_deref(), format).await
        }
        SessionsCmd::Export { channel, out } => cmd_export(client, &channel, &out).await,
        SessionsCmd::Grant {
            channel,
            genesis,
            pubkey,
            role,
        } => cmd_grant(client, &channel, &genesis, &pubkey, role).await,
        SessionsCmd::Revoke {
            channel,
            genesis,
            pubkey,
        } => cmd_revoke(client, &channel, &genesis, &pubkey).await,
        SessionsCmd::Roster { channel, genesis } => {
            cmd_authority_roster(client, &channel, &genesis).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::coding_session_payload::{
        Capabilities, LifecycleReceipt, SessionStatus, LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA,
    };

    fn target(session_id: &str, generation: u64) -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "instance-1".into(),
            session_id: session_id.into(),
            generation,
        }
    }

    fn metadata_payload(
        target: &CodingSessionTarget,
        status: SessionStatus,
        title: Option<&str>,
        model: Option<&str>,
    ) -> SessionMetadata {
        SessionMetadata {
            schema: METADATA_SCHEMA.to_owned(),
            session: target.clone(),
            project_ref: None,
            repo_ref: None,
            title: title.map(str::to_owned),
            agent_ref: None,
            provider: Some("claude-primary".into()),
            runtime: Some("claude".into()),
            model: model.map(str::to_owned),
            status,
            branch: None,
            capabilities: Capabilities::v1_claude(),
            session_ref: None,
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
        }
    }

    fn metadata_event(id: &str, signer: &str, created_at: i64, payload: &SessionMetadata) -> Value {
        json!({
            "id": id,
            "pubkey": signer,
            "kind": KIND_CODING_SESSION_METADATA,
            "created_at": created_at,
            "sig": "0".repeat(128),
            "tags": [
                ["h", "channel"],
                ["csm-v", "csm1-1"],
                ["cs-target", coding_session_target_key(&payload.session)],
            ],
            "content": serde_json::to_string(payload).expect("serialize"),
        })
    }

    fn receipt_event(
        id: &str,
        signer: &str,
        created_at: i64,
        target: Option<&CodingSessionTarget>,
    ) -> Value {
        let receipt = match target {
            Some(target) => LifecycleReceipt::created("create-1", target),
            None => LifecycleReceipt::failed("create-1", "SESSION_LIMIT", "at cap"),
        };
        json!({
            "id": id,
            "pubkey": signer,
            "kind": KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            "created_at": created_at,
            "sig": "0".repeat(128),
            "tags": [["h", "channel"], ["cslr-v", "cslr1-1"]],
            "content": serde_json::to_string(&receipt).expect("serialize"),
        })
    }

    fn transcript_event(
        id: &str,
        signer: &str,
        created_at: i64,
        target: &CodingSessionTarget,
        seq: u64,
        turn_id: Option<&str>,
        item: Value,
    ) -> Value {
        let envelope = TranscriptEnvelope::new(target, seq, created_at * 1000, turn_id, item);
        json!({
            "id": id,
            "pubkey": signer,
            "kind": KIND_CODING_SESSION_TRANSCRIPT,
            "created_at": created_at,
            "sig": "0".repeat(128),
            "tags": [
                ["h", "channel"],
                ["cst-v", "cst1-1"],
                ["cs-target", coding_session_target_key(target)],
                ["cst-seq", seq.to_string()],
            ],
            "content": serde_json::to_string(&envelope).expect("serialize"),
        })
    }

    fn tool_call(name: &str, id: &str) -> Value {
        json!({ "kind": "tool_call", "tool": { "toolName": name, "toolId": id } })
    }

    fn tool_result(id: &str, is_error: bool) -> Value {
        json!({ "kind": "tool_result", "toolId": id, "isError": is_error })
    }

    /// Rebuilt from the real 2026-08-24 stall: prompt, one terminated tool,
    /// then ~900s of nothing, then a synthesized error result carrying zero
    /// tokens. This is the shape `doctor` exists to name on sight.
    #[test]
    fn a_dropped_prompt_is_named_from_its_transcript_alone() {
        let session = target("s-1", 1);
        let base = 1_700_000_000;
        let events = vec![
            transcript_event(
                &format!("{:064}", 1),
                &"a".repeat(64),
                base,
                &session,
                1,
                Some("t-1"),
                json!({ "kind": "user_prompt", "content": "merge it" }),
            ),
            transcript_event(
                &format!("{:064}", 2),
                &"a".repeat(64),
                base,
                &session,
                2,
                Some("t-1"),
                tool_call("Terminal", "toolu_1"),
            ),
            transcript_event(
                &format!("{:064}", 3),
                &"a".repeat(64),
                base + 40,
                &session,
                3,
                Some("t-1"),
                tool_result("toolu_1", false),
            ),
            transcript_event(
                &format!("{:064}", 4),
                &"a".repeat(64),
                base + 952,
                &session,
                4,
                Some("t-1"),
                json!({
                    "kind": "result",
                    "isError": true,
                    "result": "Idle timeout — no agent activity for 870s",
                    "inputTokens": 0,
                    "outputTokens": 0,
                }),
            ),
        ];
        let (records, _) = decode_transcripts(&events);
        let turns = diagnose_turns(&records);
        assert_eq!(turns.len(), 1);
        let turn = &turns[0];

        assert_eq!(turn.is_error, Some(true));
        assert!(turn.unterminated_tools.is_empty());
        assert!((turn.span_secs - 952.0).abs() < 0.5);
        assert_eq!(turn.last_tool_offset_secs, Some(40.0));

        let findings = turn.findings.join(" | ");
        assert!(
            findings.contains("never resolved this prompt"),
            "the zero-token result is the clearest tell and must be called out: {findings}"
        );
        assert!(
            findings.contains("every tool call terminated"),
            "ruling the tools out is what separates this from a hung tool: {findings}"
        );
        assert!(
            findings.contains("912s between the last tool frame"),
            "the quiet window is the number a reader needs: {findings}"
        );
        assert!(
            findings.contains("inferred"),
            "without a turn_wire row the quiet onset is inferred, and saying so \
             is the difference between a measurement and a guess: {findings}"
        );
    }

    /// Items outside any turn are session-level facts, not a turn that failed
    /// to finish. Reporting them as `unfinished` is the same shape of lie the
    /// command exists to catch, so it is pinned.
    #[test]
    fn session_level_items_are_not_reported_as_an_unfinished_turn() {
        let session = target("s-1", 1);
        let events = vec![transcript_event(
            &format!("{:064}", 1),
            &"a".repeat(64),
            1_700_000_000,
            &session,
            1,
            None,
            json!({ "kind": "status", "status": "session_fresh" }),
        )];
        let (records, _) = decode_transcripts(&events);
        let turns = diagnose_turns(&records);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].turn_id, None);
        assert_eq!(turns[0].is_error, None);
        assert!(
            turns[0].findings.is_empty(),
            "a session-level group has nothing to diagnose: {:?}",
            turns[0].findings
        );
    }

    /// A turn that ended normally gets no findings — a triage command that
    /// flags healthy turns trains people to ignore it.
    #[test]
    fn a_healthy_turn_is_reported_without_findings() {
        let session = target("s-1", 1);
        let base = 1_700_000_000;
        let events = vec![
            transcript_event(
                &format!("{:064}", 1),
                &"a".repeat(64),
                base,
                &session,
                1,
                Some("t-1"),
                json!({ "kind": "user_prompt", "content": "go" }),
            ),
            transcript_event(
                &format!("{:064}", 2),
                &"a".repeat(64),
                base + 12,
                &session,
                2,
                Some("t-1"),
                json!({
                    "kind": "result",
                    "isError": false,
                    "result": "completed",
                    "inputTokens": 2_465_043,
                    "outputTokens": 4014,
                }),
            ),
        ];
        let (records, _) = decode_transcripts(&events);
        let turns = diagnose_turns(&records);
        assert_eq!(turns.len(), 1);
        assert!(
            turns[0].findings.is_empty(),
            "a clean turn must stay quiet: {:?}",
            turns[0].findings
        );
    }

    /// An unterminated call points at a hung tool, not a dropped prompt. The
    /// two need different words or the command sends people the wrong way.
    #[test]
    fn a_hung_tool_is_distinguished_from_a_dropped_prompt() {
        let session = target("s-1", 1);
        let base = 1_700_000_000;
        let events = vec![
            transcript_event(
                &format!("{:064}", 1),
                &"a".repeat(64),
                base,
                &session,
                1,
                Some("t-1"),
                tool_call("Terminal", "toolu_hung"),
            ),
            transcript_event(
                &format!("{:064}", 2),
                &"a".repeat(64),
                base + 900,
                &session,
                2,
                Some("t-1"),
                json!({
                    "kind": "result",
                    "isError": true,
                    "result": "Idle timeout — no agent activity for 870s",
                    "inputTokens": 1200,
                    "outputTokens": 30,
                }),
            ),
        ];
        let (records, _) = decode_transcripts(&events);
        let turns = diagnose_turns(&records);
        let findings = turns[0].findings.join(" | ");
        assert_eq!(turns[0].unterminated_tools.len(), 1);
        assert!(
            findings.contains("never terminated: Terminal (toolu_hung)"),
            "name the call that was still open: {findings}"
        );
        assert!(
            !findings.contains("never resolved this prompt"),
            "real usage means the agent did answer — this is a hung tool: {findings}"
        );
    }

    /// A published `turn_wire` row is a measurement and must win over the
    /// inference, including dropping the caveat that says it is inferred.
    #[test]
    fn a_published_wire_row_replaces_the_inference() {
        let session = target("s-1", 1);
        let base = 1_700_000_000;
        let events = vec![
            transcript_event(
                &format!("{:064}", 1),
                &"a".repeat(64),
                base,
                &session,
                1,
                Some("t-1"),
                tool_call("Terminal", "toolu_1"),
            ),
            transcript_event(
                &format!("{:064}", 2),
                &"a".repeat(64),
                base + 40,
                &session,
                2,
                Some("t-1"),
                tool_result("toolu_1", false),
            ),
            transcript_event(
                &format!("{:064}", 3),
                &"a".repeat(64),
                base + 952,
                &session,
                3,
                Some("t-1"),
                json!({
                    "kind": "status",
                    "status": "turn_wire: 47 frames/18 KiB over 952s; last \
                               agent_message_chunk at +52.1s; quiet 900.0s"
                }),
            ),
            transcript_event(
                &format!("{:064}", 4),
                &"a".repeat(64),
                base + 952,
                &session,
                4,
                Some("t-1"),
                json!({
                    "kind": "result",
                    "isError": true,
                    "result": "Idle timeout",
                    "inputTokens": 0,
                    "outputTokens": 0,
                }),
            ),
        ];
        let (records, _) = decode_transcripts(&events);
        let turns = diagnose_turns(&records);
        let turn = &turns[0];
        assert!(
            turn.wire
                .as_deref()
                .is_some_and(|wire| wire.contains("+52.1s")),
            "the measured row must be surfaced: {:?}",
            turn.wire
        );
        assert!(
            !turn.findings.iter().any(|f| f.contains("inferred")),
            "with a measurement present the inference caveat is wrong: {:?}",
            turn.findings
        );
    }

    /// The one ordering bug this command exists to not have: `cst-seq` is a
    /// decimal string on the wire, so a lexicographic sort puts item 10 before
    /// item 9 and silently reorders every transcript longer than nine items.
    #[test]
    fn transcripts_sort_by_numeric_sequence_not_by_text() {
        let session = target("s-1", 1);
        let events: Vec<Value> = [2u64, 10, 9, 1, 100, 11]
            .iter()
            .enumerate()
            .map(|(index, seq)| {
                transcript_event(
                    &format!("{index:064}"),
                    &"a".repeat(64),
                    1_700_000_000,
                    &session,
                    *seq,
                    None,
                    json!({ "kind": "assistant_text", "text": format!("item {seq}") }),
                )
            })
            .collect();

        let (mut records, stats) = decode_transcripts(&events);
        assert_eq!(stats.malformed, 0);
        sort_transcripts(&mut records);
        assert_eq!(
            records.iter().map(|record| record.seq).collect::<Vec<_>>(),
            vec![1, 2, 9, 10, 11, 100]
        );
    }

    /// Same second, same target: a total order still has to exist, and it comes
    /// from the event id — never from map iteration order.
    #[test]
    fn a_same_second_burst_still_has_a_total_order() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![
            transcript_event(
                &format!("{:064}", 9),
                &signer,
                1_700_000_000,
                &session,
                5,
                None,
                json!({ "kind": "status", "status": "idle" }),
            ),
            transcript_event(
                &format!("{:064}", 1),
                &signer,
                1_700_000_000,
                &session,
                5,
                None,
                json!({ "kind": "status", "status": "running" }),
            ),
        ];
        let (mut records, _) = decode_transcripts(&events);
        sort_transcripts(&mut records);
        assert_eq!(records[0].id, format!("{:064}", 1));
    }

    #[test]
    fn target_filtering_keeps_only_the_requested_generation() {
        let first = target("s-1", 1);
        let second = target("s-1", 2);
        let other = target("s-2", 1);
        let signer = "a".repeat(64);
        let events = vec![
            transcript_event(
                &format!("{:064}", 1),
                &signer,
                1,
                &first,
                1,
                None,
                json!({ "kind": "assistant_text", "text": "one" }),
            ),
            transcript_event(
                &format!("{:064}", 2),
                &signer,
                2,
                &second,
                1,
                None,
                json!({ "kind": "assistant_text", "text": "two" }),
            ),
            transcript_event(
                &format!("{:064}", 3),
                &signer,
                3,
                &other,
                1,
                None,
                json!({ "kind": "assistant_text", "text": "three" }),
            ),
        ];
        let (records, _) = decode_transcripts(&events);
        assert_eq!(records.len(), 3);

        // Generation is part of the identity: generation 2 of the same session
        // id is a different transcript, not more of the same one.
        let filtered = filter_transcripts_by_target(&records, &coding_session_target_key(&first));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].envelope.session.generation, 1);
    }

    #[test]
    fn tool_aggregation_counts_calls_errors_and_sorts_by_frequency() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let mut events = Vec::new();
        let mut push = |seq: u64, item: Value| {
            events.push(transcript_event(
                &format!("{seq:064}"),
                &signer,
                1_700_000_000 + seq as i64,
                &session,
                seq,
                Some("turn-1"),
                item,
            ));
        };
        push(1, tool_call("Read", "call-1"));
        push(2, tool_result("call-1", false));
        push(3, tool_call("Read", "call-2"));
        push(4, tool_result("call-2", true));
        push(5, tool_call("Read", "call-3"));
        push(6, tool_result("call-3", false));
        push(7, tool_call("Bash", "call-4"));
        push(8, tool_result("call-4", true));

        let (records, _) = decode_transcripts(&events);
        let report = aggregate_tools(&records);

        assert_eq!(report.tools.len(), 2);
        assert_eq!(report.tools[0].tool_name, "Read");
        assert_eq!(report.tools[0].calls, 3);
        assert_eq!(report.tools[0].errors, 1);
        assert!((report.tools[0].error_rate() - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(report.tools[1].tool_name, "Bash");
        assert_eq!(report.tools[1].errors, 1);
        assert!((report.tools[1].error_rate() - 1.0).abs() < 1e-9);
        assert_eq!(report.transcript_items, 8);
    }

    /// A result whose call was never fetched still counts. Dropping it would
    /// deflate the error rate, which is the number this command exists for.
    #[test]
    fn an_orphaned_tool_result_is_counted_rather_than_dropped() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![transcript_event(
            &format!("{:064}", 1),
            &signer,
            1,
            &session,
            1,
            None,
            tool_result("call-missing", true),
        )];
        let (records, _) = decode_transcripts(&events);
        let report = aggregate_tools(&records);
        assert_eq!(report.tools.len(), 1);
        assert_eq!(report.tools[0].tool_name, "(unknown)");
        assert_eq!(report.tools[0].errors, 1);
    }

    #[test]
    fn unknown_item_kinds_are_counted_as_other_rather_than_dropped() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![
            transcript_event(
                &format!("{:064}", 1),
                &signer,
                1,
                &session,
                1,
                None,
                json!({ "kind": "assistant_text", "text": "hi" }),
            ),
            transcript_event(
                &format!("{:064}", 2),
                &signer,
                2,
                &session,
                2,
                None,
                json!({ "kind": "telemetry_from_the_future", "value": 1 }),
            ),
        ];
        let (records, _) = decode_transcripts(&events);
        let report = aggregate_tools(&records);
        assert_eq!(report.item_kinds.get("assistant_text"), Some(&1));
        assert_eq!(report.item_kinds.get("other"), Some(&1));
        assert_eq!(report.transcript_items, 2);
    }

    #[test]
    fn newest_metadata_wins_and_a_same_second_tie_breaks_on_event_id() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![
            metadata_event(
                &format!("{:064}", 1),
                &signer,
                100,
                &metadata_payload(&session, SessionStatus::Starting, Some("first"), None),
            ),
            // Same second, different payload: resolved on id, and reported as
            // one conflict rather than wedging the session.
            metadata_event(
                &format!("{:064}", 3),
                &signer,
                200,
                &metadata_payload(&session, SessionStatus::Running, Some("late"), Some("m")),
            ),
            metadata_event(
                &format!("{:064}", 2),
                &signer,
                200,
                &metadata_payload(&session, SessionStatus::Idle, Some("early"), Some("m")),
            ),
        ];
        let (metadata, stats) = decode_metadata(&events);
        assert_eq!(stats.malformed, 0);
        let rows = resolve_sessions(&metadata, &[], &[]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title.as_deref(), Some("early"));
        assert_eq!(rows[0].status, "idle");
        assert_eq!(rows[0].model.as_deref(), Some("m"));
        assert_eq!(rows[0].metadata_conflicts, 1);
        assert_eq!(rows[0].created_at, 100);
        assert_eq!(rows[0].last_event_at, 200);
        assert!(!rows[0].confirmed);
    }

    #[test]
    fn a_receipt_alone_makes_a_generation_visible_and_confirmed() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![
            receipt_event(&format!("{:064}", 1), &signer, 50, Some(&session)),
            // A failed create names no session, so it confirms nothing.
            receipt_event(&format!("{:064}", 2), &signer, 60, None),
        ];
        let (receipts, stats) = decode_receipts(&events);
        assert_eq!(stats.malformed, 0);
        let rows = resolve_sessions(&[], &receipts, &[]);

        assert_eq!(rows.len(), 1);
        assert!(rows[0].confirmed);
        assert_eq!(rows[0].status, "unknown");
        assert_eq!(rows[0].target.session_id, "s-1");
    }

    /// Without metadata the transcript's own terminal item is the only thing
    /// that says how a session ended.
    #[test]
    fn status_falls_back_to_the_last_transcript_item() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![
            transcript_event(
                &format!("{:064}", 1),
                &signer,
                1,
                &session,
                1,
                Some("turn-1"),
                json!({ "kind": "assistant_text", "text": "working" }),
            ),
            transcript_event(
                &format!("{:064}", 2),
                &signer,
                2,
                &session,
                2,
                Some("turn-1"),
                json!({ "kind": "result", "subtype": "success", "isError": false }),
            ),
        ];
        let (transcripts, _) = decode_transcripts(&events);
        let rows = resolve_sessions(&[], &[], &transcripts);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "completed");
        assert_eq!(rows[0].transcript_items, 2);
    }

    #[test]
    fn an_indexed_tag_that_disagrees_with_its_payload_is_malformed() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let mut event = transcript_event(
            &format!("{:064}", 1),
            &signer,
            1,
            &session,
            7,
            None,
            json!({ "kind": "assistant_text", "text": "hi" }),
        );
        event["tags"] = json!([
            ["h", "channel"],
            ["cst-v", "cst1-1"],
            ["cs-target", coding_session_target_key(&session)],
            ["cst-seq", "8"],
        ]);
        let (records, stats) = decode_transcripts(&[event]);
        assert!(records.is_empty());
        assert_eq!(stats.malformed, 1);
    }

    #[test]
    fn the_manifest_describes_every_file_and_the_whole_time_range() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let metadata_events = vec![metadata_event(
            &format!("{:064}", 1),
            &signer,
            100,
            &metadata_payload(
                &session,
                SessionStatus::Completed,
                Some("Ship it"),
                Some("m"),
            ),
        )];
        let transcript_events = vec![transcript_event(
            &format!("{:064}", 2),
            &signer,
            300,
            &session,
            1,
            Some("turn-1"),
            json!({ "kind": "assistant_text", "text": "done" }),
        )];
        let (metadata, _) = decode_metadata(&metadata_events);
        let (transcripts, _) = decode_transcripts(&transcript_events);
        let rows = resolve_sessions(&metadata, &[], &transcripts);
        let groups = plan_export(&rows, &metadata, &[], &transcripts);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].file_name, "s-1-g1.jsonl");
        assert_eq!(groups[0].events.len(), 2);
        // Chronological within a file, so the jsonl reads as it happened.
        assert_eq!(groups[0].events[0]["created_at"], 100);

        let manifest = build_manifest("channel-uuid", &groups, "2026-08-12T00:00:00Z");
        assert_eq!(manifest["version"], 1);
        assert_eq!(manifest["channel"], "channel-uuid");
        assert_eq!(manifest["counts"]["generations"], 1);
        assert_eq!(manifest["counts"]["events"], 2);
        assert_eq!(manifest["targets"][0]["file"], "s-1-g1.jsonl");
        assert_eq!(manifest["targets"][0]["sessionId"], "s-1");
        assert_eq!(manifest["targets"][0]["generation"], 1);
        assert_eq!(manifest["targets"][0]["title"], "Ship it");
        assert_eq!(manifest["targets"][0]["status"], "completed");
        assert_eq!(manifest["targets"][0]["transcriptItems"], 1);
        assert!(manifest["timeRange"]["firstEventAt"]
            .as_str()
            .expect("first")
            .starts_with("1970-01-01"));
        assert_eq!(
            manifest["targets"][0]["target"],
            coding_session_target_key(&session)
        );
    }

    /// Two providers can mint the same session id. Letting the second file
    /// overwrite the first would lose a whole session to a name clash.
    #[test]
    fn colliding_session_ids_get_signer_suffixed_file_names() {
        let session = target("s-1", 1);
        let first_signer = "a".repeat(64);
        let second_signer = "b".repeat(64);
        let events = vec![
            metadata_event(
                &format!("{:064}", 1),
                &first_signer,
                100,
                &metadata_payload(&session, SessionStatus::Idle, None, None),
            ),
            metadata_event(
                &format!("{:064}", 2),
                &second_signer,
                200,
                &metadata_payload(&session, SessionStatus::Idle, None, None),
            ),
        ];
        let (metadata, _) = decode_metadata(&events);
        let rows = resolve_sessions(&metadata, &[], &[]);
        assert_eq!(rows.len(), 2);

        let groups = plan_export(&rows, &metadata, &[], &[]);
        let mut names: Vec<String> = groups.iter().map(|group| group.file_name.clone()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                format!("s-1-g1-{first_signer}.jsonl"),
                format!("s-1-g1-{second_signer}.jsonl"),
            ]
        );
    }

    /// An export is a claim about a whole directory. Merging into an occupied
    /// one would produce a manifest that does not describe the files beside it.
    #[test]
    fn an_export_refuses_a_non_empty_directory_but_accepts_an_empty_one() {
        let root = tempfile::tempdir().expect("tempdir");

        // Absent: created.
        let fresh = root.path().join("fresh");
        prepare_export_dir(&fresh).expect("absent directory is created");
        assert!(fresh.is_dir());

        // Present and empty: reused, and the caller's own first write is not
        // treated as a collision.
        prepare_export_dir(&fresh).expect("empty directory is reused");

        // Present and occupied: refused, with the occupant untouched.
        let occupant = fresh.join("keep.txt");
        fs::write(&occupant, "keep me").expect("seed");
        let error = prepare_export_dir(&fresh).expect_err("non-empty directory is refused");
        assert!(matches!(error, CliError::Usage(_)), "{error:?}");
        assert_eq!(fs::read_to_string(&occupant).expect("read"), "keep me");

        // Present but not a directory: refused as input, not as an IO error.
        let file = root.path().join("a-file");
        fs::write(&file, "x").expect("seed");
        let error = prepare_export_dir(&file).expect_err("a file is refused");
        assert!(matches!(error, CliError::Usage(_)), "{error:?}");
    }

    /// A session id is only bounded to 512 bytes by the wire contract; a path
    /// separator in one must not become a directory traversal on export.
    #[test]
    fn export_file_names_never_escape_the_output_directory() {
        let session = target("../../etc/passwd", 3);
        assert_eq!(export_base_name(&session), "etc-passwd-g3");
        assert_eq!(export_base_name(&target("   ", 1)), "session-g1");
    }

    #[test]
    fn markdown_groups_items_into_turns_and_closes_each_with_its_result() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![
            transcript_event(
                &format!("{:064}", 1),
                &signer,
                1,
                &session,
                1,
                None,
                json!({ "kind": "status", "status": "idle" }),
            ),
            transcript_event(
                &format!("{:064}", 2),
                &signer,
                2,
                &session,
                2,
                Some("turn-1"),
                json!({ "kind": "user_prompt", "content": "fix the test" }),
            ),
            transcript_event(
                &format!("{:064}", 3),
                &signer,
                3,
                &session,
                3,
                Some("turn-1"),
                tool_call("Read", "call-1"),
            ),
            transcript_event(
                &format!("{:064}", 4),
                &signer,
                4,
                &session,
                4,
                Some("turn-1"),
                tool_result("call-1", true),
            ),
            transcript_event(
                &format!("{:064}", 5),
                &signer,
                5,
                &session,
                5,
                Some("turn-1"),
                json!({ "kind": "assistant_text", "text": "fixed" }),
            ),
            transcript_event(
                &format!("{:064}", 6),
                &signer,
                6,
                &session,
                6,
                Some("turn-1"),
                json!({
                    "kind": "result",
                    "subtype": "success",
                    "isError": false,
                    "durationMs": 1234
                }),
            ),
        ];
        let (records, _) = decode_transcripts(&events);
        let rows = resolve_sessions(&[], &[], &records);
        let markdown = render_markdown(rows.first(), &records);

        assert!(markdown.contains("## Session"), "{markdown}");
        assert!(markdown.contains("## Turn 1 (`turn-1`)"), "{markdown}");
        assert!(markdown.contains("**User**"), "{markdown}");
        assert!(markdown.contains("fix the test"), "{markdown}");
        // The call line carries its outcome, so a reader never has to scroll
        // to find out whether a tool failed.
        assert!(
            markdown.contains("- tool `Read` (`call-1`) — error"),
            "{markdown}"
        );
        assert!(
            markdown.contains("_result: success · 1234 ms_"),
            "{markdown}"
        );
        // A raw tool_result never renders on its own — it is already folded
        // into the call line above it.
        assert!(!markdown.contains("tool_result"), "{markdown}");
    }

    // ── Redaction markers in rendered markdown ───────────────────────────────

    fn marker(bytes: u64, digest_seed: char) -> String {
        format!(
            "[elided private context: {bytes} bytes, sha256:{}]",
            String::from_iter(std::iter::repeat_n(digest_seed, 64))
        )
    }

    #[test]
    fn a_rendered_transcript_condenses_the_redactions_it_contains() {
        // End to end through `render_markdown`, because the condensing being
        // *reachable* is the whole feature — unit-testing the condenser alone
        // would still pass with it unhooked.
        let session = target("s-redacted", 1);
        let signer = "a".repeat(64);
        let events = vec![transcript_event(
            &format!("{:064}", 1),
            &signer,
            1,
            &session,
            1,
            Some("turn-1"),
            json!({
                "kind": "assistant_text",
                "text": format!("I read {} and stopped.", marker(148, 'a')),
            }),
        )];
        let (records, _) = decode_transcripts(&events);
        let rows = resolve_sessions(&[], &[], &records);
        let markdown = render_markdown(rows.first(), &records);

        assert!(markdown.contains("[redacted 148 B · #1]"), "{markdown}");
        assert!(!markdown.contains("elided private context"), "{markdown}");
        assert!(markdown.contains("## Redactions"), "{markdown}");
    }

    #[test]
    fn a_redaction_reads_as_a_label_and_the_digest_moves_to_a_footnote() {
        let rendered = condense_redaction_markers(&format!(
            "**Assistant**\n\nI read {} and stopped.\n",
            marker(148, 'a')
        ));

        assert!(rendered.contains("[redacted 148 B · #1]"), "{rendered}");
        assert!(!rendered.contains("elided private context"), "{rendered}");
        // The digest is condensed, never discarded: it is what tells two
        // readers whether they are looking at the same hidden value.
        assert!(rendered.contains("## Redactions"), "{rendered}");
        assert!(
            rendered.contains(&format!("1. 148 bytes — `sha256:{}`", "a".repeat(64))),
            "{rendered}"
        );
    }

    #[test]
    fn the_same_value_redacted_twice_shares_one_footnote_number() {
        let rendered = condense_redaction_markers(&format!(
            "first {} then {} then {}\n",
            marker(148, 'a'),
            marker(9, 'b'),
            marker(148, 'a')
        ));

        assert_eq!(rendered.matches("· #1]").count(), 2, "{rendered}");
        assert_eq!(rendered.matches("· #2]").count(), 1, "{rendered}");
        assert!(rendered.contains("2. 9 bytes"), "{rendered}");
        assert!(!rendered.contains("3. "), "{rendered}");
    }

    #[test]
    fn a_fenced_block_keeps_the_marker_byte_for_byte() {
        // Inside a fence the reader is looking at the bytes; a substitution
        // would claim they say something they do not.
        let source = format!("prose\n\n```\ngrep {}\n```\n", marker(148, 'a'));
        let rendered = condense_redaction_markers(&source);

        assert_eq!(rendered, source, "{rendered}");
        assert!(!rendered.contains("## Redactions"), "{rendered}");
    }

    #[test]
    fn text_that_is_not_a_marker_survives_unchanged() {
        for text in [
            "[elided private context: 12\n",
            "[elided private context: 148 bytes, sha256:beef]\n",
            &format!(
                "[elided private context: 148 bytes, sha256:{}]\n",
                "A".repeat(64)
            ),
            &format!(
                "[elided private context: many bytes, sha256:{}]\n",
                "a".repeat(64)
            ),
            "…[elided 41235 bytes]…\n",
        ] {
            assert_eq!(condense_redaction_markers(text), text, "{text}");
        }
    }

    #[test]
    fn a_document_with_no_redaction_gains_no_footnote_section() {
        let source = "**Assistant**\n\nran the tests and they passed\n";
        assert_eq!(condense_redaction_markers(source), source);
    }

    #[test]
    fn a_capped_item_says_dropped_and_names_its_cause() {
        // The cap is not a privacy redaction, and the reader is owed the
        // difference: different verb, and no entry in the Redactions footnote.
        let rendered = render_item(
            &json!({
                "kind": "elided",
                "reason": "oversize",
                "byteCount": 41_235,
                "contentDigest": "deadbeef",
            }),
            &HashMap::new(),
        );

        assert!(rendered.contains("dropped 41 KB"), "{rendered}");
        assert!(rendered.contains("did not fit the event cap"), "{rendered}");
        assert!(rendered.contains("`sha256:deadbeef`"), "{rendered}");
        assert!(!rendered.contains("redacted"), "{rendered}");
    }

    #[test]
    fn byte_counts_render_in_decimal_units() {
        // Same ramp as the desktop pill, so one transcript reads the same in
        // both places.
        assert_eq!(format_redacted_bytes(0), "0 B");
        assert_eq!(format_redacted_bytes(148), "148 B");
        assert_eq!(format_redacted_bytes(999), "999 B");
        assert_eq!(format_redacted_bytes(1_000), "1.0 KB");
        assert_eq!(format_redacted_bytes(2_140), "2.1 KB");
        assert_eq!(format_redacted_bytes(41_235), "41 KB");
        assert_eq!(format_redacted_bytes(3_000_000), "3.0 MB");
    }

    /// D4/NIP-CST: a `user_prompt` carries `commandId` whenever the turn was
    /// started by a command. The markdown footnote lets a reader join the
    /// line back to the `thread.turn.start` that produced it without
    /// bloating the prose line itself.
    #[test]
    fn markdown_shows_a_user_prompts_command_id_when_present() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![transcript_event(
            &format!("{:064}", 1),
            &signer,
            1,
            &session,
            1,
            Some("turn-1"),
            json!({ "kind": "user_prompt", "content": "fix the test", "commandId": "cmd-9" }),
        )];
        let (records, _) = decode_transcripts(&events);
        let rows = resolve_sessions(&[], &[], &records);
        let markdown = render_markdown(rows.first(), &records);

        assert!(markdown.contains("**User** _(cmd `cmd-9`)_"), "{markdown}");
        assert!(markdown.contains("fix the test"), "{markdown}");
    }

    /// Older providers never emit `commandId` — the footnote must not appear
    /// and rendering must not regress for them.
    #[test]
    fn markdown_omits_the_footnote_when_command_id_is_absent() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let events = vec![transcript_event(
            &format!("{:064}", 1),
            &signer,
            1,
            &session,
            1,
            Some("turn-1"),
            json!({ "kind": "user_prompt", "content": "fix the test" }),
        )];
        let (records, _) = decode_transcripts(&events);
        let rows = resolve_sessions(&[], &[], &records);
        let markdown = render_markdown(rows.first(), &records);

        assert!(markdown.contains("**User**\n\nfix the test"), "{markdown}");
        assert!(!markdown.contains("_(cmd"), "{markdown}");
    }

    // ── D4 turn receipts never create, confirm, or end a generation ─────────

    /// Builds a well-formed turn-stage 44224 event straight from JSON, since
    /// `buzz-core`'s `LifecycleReceipt` builders (Lane 1A) do not mint these
    /// statuses yet. Mirrors the exact-key shape D4 requires: 6 keys with
    /// `turnId` for `turn_started`, 5 keys otherwise.
    fn turn_receipt_event(
        id: &str,
        signer: &str,
        created_at: i64,
        command_id: &str,
        status: &str,
        target: Option<&CodingSessionTarget>,
        turn_id: Option<&str>,
    ) -> Value {
        let mut content = serde_json::json!({
            "schema": LIFECYCLE_RECEIPT_SCHEMA,
            "commandId": command_id,
            "status": status,
            "session": target,
            "error": Value::Null,
        });
        if let (Some(object), Some(turn_id)) = (content.as_object_mut(), turn_id) {
            object.insert("turnId".into(), json!(turn_id));
        }
        json!({
            "id": id,
            "pubkey": signer,
            "kind": KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            "created_at": created_at,
            "sig": "0".repeat(128),
            "tags": [["h", "channel"], ["cslr-v", "cslr1-1"]],
            "content": content.to_string(),
        })
    }

    /// Every turn stage, not two named ones: the list this file used to keep
    /// by hand fell behind the day `turn_degraded` and `interrupt_delivered`
    /// were added, and nothing here noticed.
    const EVERY_TURN_STAGE: &[ReceiptStatus] = &[
        ReceiptStatus::TurnQueued,
        ReceiptStatus::TurnStarted,
        ReceiptStatus::TurnDropped,
        ReceiptStatus::TurnRefused,
        ReceiptStatus::TurnDegraded,
        ReceiptStatus::InterruptDelivered,
    ];

    #[test]
    fn every_turn_stage_this_build_knows_is_covered_here() {
        for status in EVERY_TURN_STAGE {
            assert!(
                status.is_turn_stage(),
                "{} is not a turn stage",
                status.as_str()
            );
        }
        // A new turn stage in `buzz-core` has to be added to the list above,
        // or the two tests below stop covering it silently.
        assert_eq!(
            EVERY_TURN_STAGE.len(),
            6,
            "a turn stage was added or removed in buzz-core; extend EVERY_TURN_STAGE"
        );
    }

    #[test]
    fn a_turn_receipt_for_an_unknown_target_creates_no_row() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        for (index, status) in EVERY_TURN_STAGE.iter().enumerate() {
            let turn_id = (*status == ReceiptStatus::TurnStarted).then_some("provider-turn-9");
            let events = vec![turn_receipt_event(
                &format!("{:064}", index + 1),
                &signer,
                10,
                "turn-cmd-1",
                status.as_str(),
                Some(&session),
                turn_id,
            )];
            let (receipts, stats) = decode_receipts(&events);
            assert_eq!(
                stats.malformed,
                0,
                "a well-formed {} receipt is not garbage",
                status.as_str()
            );
            assert_eq!(receipts.len(), 1);
            assert!(
                receipts[0].is_turn_status,
                "{} is a turn stage and must be marked as one",
                status.as_str()
            );

            let rows = resolve_sessions(&[], &receipts, &[]);
            assert!(
                rows.is_empty(),
                "a {} receipt alone must never create a generation row: {rows:?}",
                status.as_str()
            );
        }
    }

    #[test]
    fn a_turn_receipt_does_not_confirm_or_change_the_status_of_a_known_target() {
        let session = target("s-1", 1);
        let signer = "a".repeat(64);
        let metadata_events = vec![metadata_event(
            &format!("{:064}", 1),
            &signer,
            100,
            &metadata_payload(&session, SessionStatus::Idle, Some("t"), None),
        )];
        let (metadata, _) = decode_metadata(&metadata_events);
        let baseline = resolve_sessions(&metadata, &[], &[]);
        assert_eq!(baseline.len(), 1);
        assert!(!baseline[0].confirmed);
        assert_eq!(baseline[0].status, "idle");

        for (index, status) in EVERY_TURN_STAGE.iter().enumerate() {
            let turn_id = (*status == ReceiptStatus::TurnStarted).then_some("provider-turn-9");
            let receipt_events = vec![turn_receipt_event(
                &format!("{:064}", index + 2),
                &signer,
                200,
                "turn-cmd-2",
                status.as_str(),
                Some(&session),
                turn_id,
            )];
            let (receipts, stats) = decode_receipts(&receipt_events);
            assert_eq!(stats.malformed, 0);

            let rows = resolve_sessions(&metadata, &receipts, &[]);
            assert_eq!(rows.len(), 1);
            assert!(
                !rows[0].confirmed,
                "a {} receipt must never confirm a generation",
                status.as_str()
            );
            assert_eq!(
                rows[0].status,
                "idle",
                "a {} receipt must never change a generation's status",
                status.as_str()
            );
        }
    }

    // ── NIP-CSAT receipt decode + fold ───────────────────────────────────────

    fn receipt_event_40099(
        genesis: &str,
        accepted: &str,
        seq: u32,
        ttype: &str,
        pk: &str,
    ) -> Value {
        json!({
            "id": format!("receipt-{seq}"),
            "kind": 40099,
            "content": json!({
                "type": "coding_session_authority_transition_accepted",
                "genesisRef": genesis,
                "acceptedEventId": accepted,
                "seq": seq,
                "transitionType": ttype,
                "granteePubkey": pk,
            }).to_string(),
        })
    }

    #[test]
    fn authority_receipts_filter_by_genesis_and_type() {
        let genesis = "a".repeat(64);
        let other = "b".repeat(64);
        let events = vec![
            receipt_event_40099(
                &genesis,
                &"1".repeat(64),
                1,
                "grant-operator",
                &"c".repeat(64),
            ),
            // A receipt for another chain must not be folded into this one.
            receipt_event_40099(&other, &"2".repeat(64), 1, "grant-viewer", &"d".repeat(64)),
            // A non-receipt system message is skipped.
            json!({ "id": "sys-1", "kind": 40099, "content": "{\"type\":\"member_added\"}" }),
            // Undecodable content is skipped, not fatal.
            json!({ "id": "sys-2", "kind": 40099, "content": "not json" }),
        ];
        let receipts = decode_authority_receipts(&events, &genesis);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].seq, 1);
        assert_eq!(receipts[0].transition_type, "grant-operator");
    }

    #[test]
    fn authority_fold_grants_regrades_and_revokes_in_seq_order() {
        let genesis = "a".repeat(64);
        let alice = "c".repeat(64);
        let bob = "d".repeat(64);
        // Deliberately out of order: the fold must sort by seq.
        let events = vec![
            receipt_event_40099(&genesis, &"3".repeat(64), 3, "revoke", &alice),
            receipt_event_40099(&genesis, &"1".repeat(64), 1, "grant-operator", &alice),
            receipt_event_40099(&genesis, &"2".repeat(64), 2, "grant-viewer", &bob),
            receipt_event_40099(&genesis, &"4".repeat(64), 4, "grant-operator", &bob),
        ];
        let state = fold_authority_receipts(decode_authority_receipts(&events, &genesis));
        assert_eq!(state.head_seq, 4);
        assert_eq!(
            state.head_event_id.as_deref(),
            Some("4".repeat(64).as_str())
        );
        // Alice was revoked; Bob was re-graded viewer -> collaborator.
        assert_eq!(state.grants.len(), 1);
        assert_eq!(
            state.grants.get(&bob).map(String::as_str),
            Some("collaborator")
        );
    }

    #[test]
    fn authority_fold_of_no_receipts_is_the_empty_chain() {
        let state = fold_authority_receipts(Vec::new());
        assert_eq!(state.head_seq, 0);
        assert!(state.head_event_id.is_none());
        assert!(state.grants.is_empty());
    }

    #[test]
    fn chain_head_conflict_matches_relay_linkage_refusals() {
        for msg in [
            "relay rejected event: invalid: prevAccepted does not match the chain's current head (expected aa)",
            "relay rejected event: invalid: seq does not extend the chain (expected 3)",
        ] {
            assert!(is_chain_head_conflict(&CliError::Other(msg.into())), "{msg}");
        }
        assert!(!is_chain_head_conflict(&CliError::Other(
            "relay rejected event: invalid: only the session owner may extend the chain".into()
        )));
    }
}
