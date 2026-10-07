//! `bee sessions checkpoints` and `bee sessions diff` — reading turn
//! checkpoints (kind 44231, NIP-CSCK) from the command line (SV-28, SV-30).
//!
//! A checkpoint is a provider-signed fact: which files one turn changed,
//! measured from git, and the two tree ids the change runs between. This
//! module reads them, and nothing here writes to any repository.
//!
//! Three rules shape every answer:
//!
//! * **A refusal is disclosed, never dropped.** A checkpoint that does not
//!   decode, whose signature does not verify, or whose signer is not the key
//!   that signs the same generation's 44225 transcript is listed under
//!   `refused` with the reason. A reader that silently dropped it would report
//!   a clean total over a partly unreadable channel.
//! * **One turn, one checkpoint.** `turn` checkpoints fold per
//!   `(cs-target, turnId)`, highest `throughSeq` winning; a second event with
//!   the same `csck-key` is a duplicate, not a revision. `pre_rewind`
//!   checkpoints are kept apart (NIP-CSCK § Reader guidance).
//! * **Never an empty diff for "could not read".** `diff` reads the trees from
//!   a checkout this computer records for the session. When there is none, or
//!   it does not hold the objects, the answer is the checkpoint's own file list
//!   plus a `diffUnavailable` reason — the full diff lives on the computer that
//!   ran the turn.
//!
//! Git runs read-only: `cat-file -e` and `diff` between two tree ids, with
//! hooks pointed at `/dev/null`, no external diff, no textconv, no fsmonitor
//! and no optional locks. The real index, `HEAD` and branches are never
//! touched.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use nostr::Event;
use serde_json::{json, Value};

use beekeeper_core::coding_session_checkpoint::{
    coding_session_checkpoint_semantic_key, decode_coding_session_checkpoint_event,
    CodingSessionCheckpointPayload, CodingSessionCheckpointReason,
};
use beekeeper_core::coding_session_command::coding_session_target_key;
use beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT;
use beekeeper_sdk::kind::KIND_CODING_SESSION_TRANSCRIPT;

use super::worktree::default_store_path;
use super::{decode_transcripts, fetch_channel_events, resolve_target, tag_value};
use crate::client::BeekeeperClient;
use crate::error::CliError;
use crate::validate::validate_uuid;

/// The sentence every remote-objects answer carries.
pub const REMOTE_OBJECTS_SENTENCE: &str =
    "The full diff lives on the computer that ran this turn; this computer has the file list only.";

// ── Decoding and admission ──────────────────────────────────────────────────

/// One kind 44231 event that decoded, verified, and was signed by its
/// generation's transcript signer.
#[derive(Debug, Clone)]
pub struct CheckpointRecord {
    /// The event id.
    pub event_id: String,
    /// The signing key, lowercase hex.
    pub signer: String,
    /// The event's `created_at`, unix seconds.
    pub created_at: u64,
    /// The structured `cs-target` key of the generation it describes.
    pub target_key: String,
    /// The `csck-key` identity; equal keys are duplicates.
    pub semantic_key: String,
    /// The decoded payload.
    pub payload: CodingSessionCheckpointPayload,
}

/// A 44231 event this reader would not use, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRefusal {
    /// The event id, when the event carried one.
    pub event_id: Option<String>,
    /// The signer, when the event carried one.
    pub signer: Option<String>,
    /// The generation it claims to describe, from its content or, failing
    /// that, its `cs-target` tag.
    pub target_key: Option<String>,
    /// Why it was refused.
    pub reason: String,
}

impl CheckpointRefusal {
    fn to_json(&self) -> Value {
        json!({
            "eventId": self.event_id,
            "signer": self.signer,
            "target": self.target_key,
            "reason": self.reason,
        })
    }
}

/// Decode every 44231 in `events`: strict core decode plus signature check.
/// The signer is not checked here; see [`admit_checkpoints`].
pub fn decode_checkpoint_events(
    events: &[Value],
) -> (Vec<CheckpointRecord>, Vec<CheckpointRefusal>) {
    let mut records = Vec::new();
    let mut refused = Vec::new();
    for value in events {
        if value.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_CHECKPOINT))
        {
            continue;
        }
        let refuse = |reason: String| CheckpointRefusal {
            event_id: value.get("id").and_then(Value::as_str).map(str::to_owned),
            signer: value
                .get("pubkey")
                .and_then(Value::as_str)
                .map(str::to_owned),
            target_key: tag_value(value, "cs-target"),
            reason,
        };
        let event = match serde_json::from_value::<Event>(value.clone()) {
            Ok(event) => event,
            Err(error) => {
                refused.push(refuse(format!("not a well-formed signed event: {error}")));
                continue;
            }
        };
        if event.verify().is_err() {
            refused.push(refuse("the event's signature does not verify".to_owned()));
            continue;
        }
        match decode_coding_session_checkpoint_event(&event) {
            Ok(payload) => records.push(CheckpointRecord {
                event_id: event.id.to_hex(),
                signer: event.pubkey.to_hex(),
                created_at: event.created_at.as_secs(),
                target_key: coding_session_target_key(&payload.session),
                semantic_key: coding_session_checkpoint_semantic_key(
                    &payload.session,
                    payload.reason,
                    payload.coverage.through_seq,
                ),
                payload,
            }),
            Err(reason) => refused.push(refuse(reason)),
        }
    }
    (records, refused)
}

/// The keys that signed each generation's 44225 transcript items.
pub fn transcript_signers(events: &[Value]) -> BTreeMap<String, BTreeSet<String>> {
    let (records, _) = decode_transcripts(events);
    let mut signers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for record in records {
        signers
            .entry(record.target_key)
            .or_default()
            .insert(record.signer);
    }
    signers
}

/// Keep the checkpoints signed by their generation's one transcript signer
/// (NIP-CSCK § Signer); refuse the rest with the reason.
///
/// A generation whose transcript has no items, or items from more than one
/// key, has no single signer to compare against, so its checkpoints are
/// refused rather than trusted on a guess.
pub fn admit_checkpoints(
    records: Vec<CheckpointRecord>,
    signers: &BTreeMap<String, BTreeSet<String>>,
) -> (Vec<CheckpointRecord>, Vec<CheckpointRefusal>) {
    let mut admitted = Vec::new();
    let mut refused = Vec::new();
    for record in records {
        let reason = match signers.get(&record.target_key) {
            None => Some(
                "no transcript item names this generation, so its provider signer is unknown"
                    .to_owned(),
            ),
            Some(set) if set.len() > 1 => Some(format!(
                "this generation's transcript has {} signers, so its provider signer is ambiguous",
                set.len()
            )),
            Some(set) if !set.contains(&record.signer) => Some(format!(
                "signed by {}, not by the key that signs this generation's transcript ({})",
                record.signer,
                set.iter().next().map_or("", String::as_str)
            )),
            Some(_) => None,
        };
        match reason {
            None => admitted.push(record),
            Some(reason) => refused.push(CheckpointRefusal {
                event_id: Some(record.event_id),
                signer: Some(record.signer),
                target_key: Some(record.target_key),
                reason,
            }),
        }
    }
    (admitted, refused)
}

/// The fold of one channel's admitted checkpoints.
#[derive(Debug, Clone, Default)]
pub struct CheckpointFold {
    /// The winning checkpoints — one `turn` checkpoint per
    /// `(cs-target, turnId)` plus every distinct `pre_rewind` — ordered by
    /// target, then `throughSeq`, then reason.
    pub checkpoints: Vec<CheckpointRecord>,
    /// Events whose `csck-key` an earlier event already held.
    pub duplicates: usize,
    /// `turn` checkpoints replaced by a later one for the same turn.
    pub superseded: usize,
}

/// Fold admitted checkpoints (NIP-CSCK § Reader guidance).
pub fn fold_checkpoints(mut records: Vec<CheckpointRecord>) -> CheckpointFold {
    // Earliest first, so the first holder of a csck-key is the one kept.
    records.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then(a.event_id.cmp(&b.event_id))
    });
    let mut fold = CheckpointFold::default();
    let mut seen_keys = BTreeSet::new();
    let mut turns: BTreeMap<(String, String), CheckpointRecord> = BTreeMap::new();
    let mut rewinds = Vec::new();
    for record in records {
        if !seen_keys.insert(record.semantic_key.clone()) {
            fold.duplicates += 1;
            continue;
        }
        match (record.payload.reason, record.payload.turn_id.clone()) {
            (CodingSessionCheckpointReason::Turn, Some(turn_id)) => {
                let key = (record.target_key.clone(), turn_id);
                match turns.get(&key) {
                    Some(held)
                        if held.payload.coverage.through_seq
                            >= record.payload.coverage.through_seq =>
                    {
                        fold.superseded += 1;
                    }
                    Some(_) => {
                        fold.superseded += 1;
                        turns.insert(key, record);
                    }
                    None => {
                        turns.insert(key, record);
                    }
                }
            }
            // The decoder refuses a turn checkpoint without a turnId, so
            // anything else here is a pre_rewind.
            _ => rewinds.push(record),
        }
    }
    fold.checkpoints = turns.into_values().chain(rewinds).collect();
    fold.checkpoints.sort_by(|a, b| {
        a.target_key
            .cmp(&b.target_key)
            .then(
                a.payload
                    .coverage
                    .through_seq
                    .cmp(&b.payload.coverage.through_seq),
            )
            .then(a.payload.reason.as_str().cmp(b.payload.reason.as_str()))
    });
    fold
}

/// Decode, admit and fold one channel's events in one step. Refusals from
/// decoding come first, then signer refusals.
pub fn read_checkpoints(events: &[Value]) -> (CheckpointFold, Vec<CheckpointRefusal>) {
    let (decoded, mut refused) = decode_checkpoint_events(events);
    let (admitted, signer_refused) = admit_checkpoints(decoded, &transcript_signers(events));
    refused.extend(signer_refused);
    (fold_checkpoints(admitted), refused)
}

// ── Rows ────────────────────────────────────────────────────────────────────

/// `omitted.length + omittedNotListed`: the true count of paths left out of
/// the captured tree, or 0 when no tree was captured.
pub fn files_not_captured(payload: &CodingSessionCheckpointPayload) -> u64 {
    payload.git.as_ref().map_or(0, |git| {
        (git.omitted.len() as u64).saturating_add(git.omitted_not_listed)
    })
}

/// One checkpoint as `bee sessions checkpoints` prints it.
pub fn checkpoint_row(record: &CheckpointRecord) -> Value {
    let payload = &record.payload;
    let git = payload.git.as_ref();
    json!({
        "eventId": record.event_id,
        "signer": record.signer,
        "createdAt": record.created_at,
        "target": record.target_key,
        "sessionId": payload.session.session_id,
        "generation": payload.session.generation,
        "turnId": payload.turn_id,
        "reason": payload.reason.as_str(),
        "fromSeq": payload.coverage.from_seq,
        "throughSeq": payload.coverage.through_seq,
        "head": git.and_then(|git| git.head.clone()),
        "branch": git.and_then(|git| git.branch.clone()),
        "baseTree": git.and_then(|git| git.base_tree.clone()),
        "tree": git.map(|git| git.tree.clone()),
        "commit": git.map(|git| git.commit.clone()),
        "files": serde_json::to_value(&payload.files).unwrap_or(Value::Null),
        "filesNotListed": payload.files_not_listed,
        "complete": git.map(|git| git.complete),
        "omitted": git.map(|git| serde_json::to_value(&git.omitted).unwrap_or(Value::Null)),
        "filesNotCaptured": files_not_captured(payload),
        "outsideTurn": git.and_then(|git| git.outside_turn),
        "restorable": payload.restorable,
        "unavailable": serde_json::to_value(&payload.unavailable).unwrap_or(Value::Null),
    })
}

/// Normalize a `--commit` value: 7 to 64 hex digits, lowercased.
pub fn normalize_commit_prefix(value: &str) -> Result<String, CliError> {
    let value = value.trim();
    if (7..=64).contains(&value.len()) && value.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(CliError::Usage(format!(
            "--commit must be 7 to 64 hexadecimal digits: {value}"
        )))
    }
}

/// Whether a checkpoint's `git.head` starts with `prefix`.
fn head_matches(record: &CheckpointRecord, prefix: &str) -> bool {
    record
        .payload
        .git
        .as_ref()
        .and_then(|git| git.head.as_deref())
        .is_some_and(|head| head.starts_with(prefix))
}

/// What `bee sessions checkpoints` prints, before formatting.
#[derive(Debug, Clone, Default)]
pub struct CheckpointsReport {
    /// One row per kept checkpoint, already filtered.
    pub rows: Vec<Value>,
    /// Refusals relevant to the request.
    pub refused: Vec<CheckpointRefusal>,
    /// From [`CheckpointFold::duplicates`].
    pub duplicates: usize,
    /// From [`CheckpointFold::superseded`].
    pub superseded: usize,
}

/// Build the report for one target (or every target, when `target` is
/// `None`), optionally keeping only checkpoints whose `git.head` matches
/// `commit`.
///
/// A refusal whose target is known and differs from `target` is left out; one
/// whose target could not be read at all is kept, since it may be this one.
pub fn checkpoints_report(
    events: &[Value],
    target: Option<&str>,
    commit: Option<&str>,
) -> CheckpointsReport {
    let (fold, refused) = read_checkpoints(events);
    let rows = fold
        .checkpoints
        .iter()
        .filter(|record| target.is_none_or(|target| record.target_key == target))
        .filter(|record| commit.is_none_or(|prefix| head_matches(record, prefix)))
        .map(checkpoint_row)
        .collect();
    let refused = refused
        .into_iter()
        .filter(|refusal| match (target, refusal.target_key.as_deref()) {
            (Some(target), Some(key)) => key == target,
            _ => true,
        })
        .collect();
    CheckpointsReport {
        rows,
        refused,
        duplicates: fold.duplicates,
        superseded: fold.superseded,
    }
}

/// `bee sessions checkpoints`.
pub async fn cmd_checkpoints(
    client: &BeekeeperClient,
    channel: &str,
    target: Option<&str>,
    session: Option<&str>,
    commit: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel)?;
    let commit = commit.map(normalize_commit_prefix).transpose()?;
    let target_key = if target.is_some() || session.is_some() {
        Some(resolve_target(client, channel, target, session).await?.0)
    } else if commit.is_some() {
        None
    } else {
        return Err(CliError::Usage(
            "one of --target, --session or --commit is required".to_owned(),
        ));
    };
    let events = fetch_channel_events(
        client,
        channel,
        &[
            KIND_CODING_SESSION_CHECKPOINT,
            KIND_CODING_SESSION_TRANSCRIPT,
        ],
    )
    .await?;
    let report = checkpoints_report(&events, target_key.as_deref(), commit.as_deref());
    match format {
        crate::OutputFormat::Compact => {
            println!("{}", Value::Array(report.rows));
            for refusal in &report.refused {
                eprintln!(
                    "refused checkpoint {}: {}",
                    refusal.event_id.as_deref().unwrap_or("<no id>"),
                    refusal.reason
                );
            }
        }
        crate::OutputFormat::Json => println!(
            "{}",
            json!({
                "channel": channel,
                "target": target_key,
                "commit": commit,
                "checkpoints": report.rows,
                "refused": report.refused.iter().map(CheckpointRefusal::to_json).collect::<Vec<_>>(),
                "duplicates": report.duplicates,
                "superseded": report.superseded,
            })
        ),
    }
    Ok(())
}

// ── Diff: choosing the range ────────────────────────────────────────────────

/// Which range `bee sessions diff` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum DiffScope {
    /// One turn: its `baseTree` to its `tree`.
    Turn,
    /// The whole generation: the first captured baseline to the latest tree.
    Session,
}

impl DiffScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Session => "session",
        }
    }
}

/// Why no patch is printed, in the answer's own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffUnavailable {
    /// A stable token: `CHECKPOINT_UNAVAILABLE`, `BASELINE_MISSING`,
    /// `NO_CHECKOUT`, `RECORD_UNREADABLE`, `OBJECTS_MISSING`, `NOT_A_TREE`
    /// or `GIT_FAILED`.
    pub code: &'static str,
    /// One sentence a person can read.
    pub sentence: String,
    /// For `OBJECTS_MISSING`, the tree ids this checkout does not hold.
    pub missing: Vec<String>,
}

impl DiffUnavailable {
    fn new(code: &'static str, sentence: impl Into<String>) -> Self {
        Self {
            code,
            sentence: sentence.into(),
            missing: Vec::new(),
        }
    }

    fn to_json(&self) -> Value {
        json!({ "code": self.code, "sentence": self.sentence, "missing": self.missing })
    }
}

/// The range one diff request resolved to, before git runs.
#[derive(Debug, Clone)]
pub struct DiffPlan {
    /// The scope asked for.
    pub scope: DiffScope,
    /// The checkpoints the range spans, in `throughSeq` order.
    pub checkpoints: Vec<CheckpointRecord>,
    /// Start of the range.
    pub from_tree: Option<String>,
    /// End of the range.
    pub to_tree: Option<String>,
    /// Set when the range does not start at the checkpoint's own baseline.
    pub baseline: Option<Value>,
    /// Set when nothing can be diffed regardless of checkout.
    pub unavailable: Option<DiffUnavailable>,
}

/// Choose the checkpoint(s) and tree ids for one request.
///
/// `turns` are one target's folded `turn` checkpoints in `throughSeq` order;
/// `all` additionally holds its `pre_rewind` ones, which `--checkpoint` may
/// name.
///
/// # Errors
/// `--turn`/`--checkpoint` with `--scope session`, or a turn or checkpoint
/// this target has no admitted checkpoint for.
pub fn plan_diff(
    all: &[CheckpointRecord],
    turn: Option<&str>,
    checkpoint: Option<&str>,
    scope: DiffScope,
) -> Result<DiffPlan, CliError> {
    let turns: Vec<&CheckpointRecord> = all
        .iter()
        .filter(|record| record.payload.reason == CodingSessionCheckpointReason::Turn)
        .collect();
    match scope {
        DiffScope::Session => {
            if turn.is_some() || checkpoint.is_some() {
                return Err(CliError::Usage(
                    "--turn and --checkpoint pick one turn; they do not combine with --scope session"
                        .to_owned(),
                ));
            }
            Ok(plan_session(&turns))
        }
        DiffScope::Turn => {
            let chosen = match (turn, checkpoint) {
                (_, Some(id)) => all.iter().find(|record| record.event_id == id),
                (Some(turn), None) => turns
                    .iter()
                    .copied()
                    .find(|record| record.payload.turn_id.as_deref() == Some(turn)),
                (None, None) => turns.last().copied(),
            };
            let chosen = chosen.ok_or_else(|| {
                CliError::NotFound(match (turn, checkpoint) {
                    (_, Some(id)) => format!("no admitted checkpoint {id} for this target"),
                    (Some(turn), None) => format!("no admitted checkpoint for turn {turn}"),
                    (None, None) => "this target has no admitted turn checkpoint".to_owned(),
                })
            })?;
            Ok(plan_turn(chosen, &turns))
        }
    }
}

fn plan_turn(chosen: &CheckpointRecord, turns: &[&CheckpointRecord]) -> DiffPlan {
    let mut plan = DiffPlan {
        scope: DiffScope::Turn,
        checkpoints: vec![chosen.clone()],
        from_tree: None,
        to_tree: None,
        baseline: None,
        unavailable: None,
    };
    let Some(git) = chosen.payload.git.as_ref() else {
        plan.unavailable = Some(checkpoint_unavailable(chosen));
        return plan;
    };
    plan.to_tree = Some(git.tree.clone());
    if let Some(base) = &git.base_tree {
        plan.from_tree = Some(base.clone());
        return plan;
    }
    // Baseline not captured: start at the previous checkpoint's tree and say
    // so (NIP-CSCK `baseTree: null` ⇒ `files: []`).
    let previous = turns
        .iter()
        .rev()
        .find(|record| {
            record.payload.coverage.through_seq < chosen.payload.coverage.from_seq
                && record.payload.git.is_some()
        })
        .and_then(|record| record.payload.git.as_ref().map(|git| (record, git)));
    match previous {
        Some((record, previous_git)) => {
            plan.from_tree = Some(previous_git.tree.clone());
            plan.baseline = Some(json!({
                "source": "previous_checkpoint",
                "eventId": record.event_id,
                "sentence": "This turn's baseline was not captured, so the diff starts at the previous checkpoint's tree and also shows anything changed between the two turns; the checkpoint itself lists no files.",
            }));
        }
        None => {
            plan.unavailable = Some(DiffUnavailable::new(
                "BASELINE_MISSING",
                "This turn's baseline was not captured and no earlier checkpoint names a tree to start from.",
            ));
        }
    }
    plan
}

fn plan_session(turns: &[&CheckpointRecord]) -> DiffPlan {
    let measured: Vec<&CheckpointRecord> = turns
        .iter()
        .copied()
        .filter(|record| record.payload.git.is_some())
        .collect();
    let mut plan = DiffPlan {
        scope: DiffScope::Session,
        checkpoints: turns.iter().map(|record| (*record).clone()).collect(),
        from_tree: None,
        to_tree: None,
        baseline: None,
        unavailable: None,
    };
    if turns.is_empty() {
        plan.unavailable = Some(DiffUnavailable::new(
            "CHECKPOINT_UNAVAILABLE",
            "This session has no admitted turn checkpoint.",
        ));
        return plan;
    }
    let start = measured.iter().position(|record| {
        record
            .payload
            .git
            .as_ref()
            .is_some_and(|git| git.base_tree.is_some())
    });
    let (Some(start), Some(last)) = (start, measured.last()) else {
        plan.unavailable = Some(DiffUnavailable::new(
            "BASELINE_MISSING",
            "No turn of this session captured both a baseline and a tree.",
        ));
        return plan;
    };
    let first = measured[start];
    plan.from_tree = first
        .payload
        .git
        .as_ref()
        .and_then(|git| git.base_tree.clone());
    plan.to_tree = last.payload.git.as_ref().map(|git| git.tree.clone());
    let uncovered: Vec<Value> = turns
        .iter()
        .filter(|record| {
            record.payload.git.is_none()
                || record.payload.coverage.through_seq < first.payload.coverage.from_seq
        })
        .map(|record| json!(record.payload.turn_id))
        .collect();
    if !uncovered.is_empty() {
        plan.baseline = Some(json!({
            "source": "first_captured_baseline",
            "eventId": first.event_id,
            "uncoveredTurns": uncovered,
            "sentence": "Some turns have no measured tree or end before the first captured baseline; the diff runs from that baseline to the latest captured tree.",
        }));
    }
    plan
}

fn checkpoint_unavailable(record: &CheckpointRecord) -> DiffUnavailable {
    let (code, sentence) = record.payload.unavailable.as_ref().map_or(
        ("", "the checkpoint carries no tree".to_owned()),
        |unavailable| (unavailable.code.as_str(), unavailable.sentence.clone()),
    );
    DiffUnavailable::new(
        "CHECKPOINT_UNAVAILABLE",
        format!("No checkpoint tree was captured ({code}): {sentence}"),
    )
}

/// The file facts the answer carries for the plan's range, from the signed
/// checkpoints rather than from git.
fn plan_files(plan: &DiffPlan) -> Value {
    match plan.scope {
        DiffScope::Turn => {
            let Some(record) = plan.checkpoints.first() else {
                return json!({});
            };
            let payload = &record.payload;
            json!({
                "files": serde_json::to_value(&payload.files).unwrap_or(Value::Null),
                "filesNotListed": payload.files_not_listed,
                "complete": payload.git.as_ref().map(|git| git.complete),
                "filesNotCaptured": files_not_captured(payload),
                "outsideTurn": payload.git.as_ref().and_then(|git| git.outside_turn),
            })
        }
        DiffScope::Session => {
            let mut touched = BTreeSet::new();
            let mut not_listed = 0u64;
            let mut not_captured = 0u64;
            let mut complete = true;
            // Any measured "yes" is a yes; "no" only when every checkpoint
            // measured it; otherwise unknown (null), never a guessed `false`.
            let mut outside_any = false;
            let mut outside_all_known = true;
            for record in &plan.checkpoints {
                let payload = &record.payload;
                touched.extend(payload.files.iter().map(|file| file.path.clone()));
                not_listed = not_listed.saturating_add(payload.files_not_listed);
                not_captured = not_captured.saturating_add(files_not_captured(payload));
                complete &= payload.git.as_ref().is_some_and(|git| git.complete);
                match payload.git.as_ref().and_then(|git| git.outside_turn) {
                    Some(true) => outside_any = true,
                    Some(false) => {}
                    None => outside_all_known = false,
                }
            }
            json!({
                "filesTouched": touched.into_iter().collect::<Vec<_>>(),
                "filesNotListed": not_listed,
                "complete": complete,
                "filesNotCaptured": not_captured,
                "outsideTurn": if outside_any {
                    Some(true)
                } else if outside_all_known {
                    Some(false)
                } else {
                    None
                },
            })
        }
    }
}

/// Run the plan against a checkout (or none) and build the answer.
pub fn diff_answer(
    channel: &str,
    target: &str,
    plan: &DiffPlan,
    checkout: Result<Option<Checkout>, String>,
) -> Value {
    let mut patch: Option<DiffPatch> = None;
    let mut source: Option<&'static str> = None;
    let unavailable = match (&plan.unavailable, &plan.from_tree, &plan.to_tree) {
        (Some(unavailable), _, _) => Some(unavailable.clone()),
        (None, Some(from), Some(to)) => match (clean_oid(from), clean_oid(to), checkout) {
            (Some(from), Some(to), Ok(Some(checkout))) => {
                source = Some(checkout.source);
                match diff_trees_in(&checkout.dir, from, to) {
                    Ok(found) => {
                        patch = Some(found);
                        None
                    }
                    Err(unavailable) => Some(unavailable),
                }
            }
            (Some(_), Some(_), Ok(None)) => Some(DiffUnavailable::new(
                "NO_CHECKOUT",
                format!(
                    "This computer records no checkout for this session. {REMOTE_OBJECTS_SENTENCE}"
                ),
            )),
            (Some(_), Some(_), Err(reason)) => Some(DiffUnavailable::new(
                "RECORD_UNREADABLE",
                format!("The host's checkout record could not be read: {reason}"),
            )),
            _ => Some(DiffUnavailable::new(
                "GIT_FAILED",
                "a checkpoint tree id is not 40 or 64 lowercase hex digits",
            )),
        },
        (None, _, _) => Some(DiffUnavailable::new(
            "BASELINE_MISSING",
            "the range has no start or end tree",
        )),
    };
    let first = plan.checkpoints.first();
    let last = plan.checkpoints.last();
    let mut answer = json!({
        "channel": channel,
        "target": target,
        "scope": plan.scope.as_str(),
        "checkpoints": plan.checkpoints.iter().map(|record| record.event_id.clone()).collect::<Vec<_>>(),
        "turnId": match plan.scope {
            DiffScope::Turn => first.and_then(|record| record.payload.turn_id.clone()),
            DiffScope::Session => None,
        },
        "fromSeq": first.map(|record| record.payload.coverage.from_seq),
        "throughSeq": last.map(|record| record.payload.coverage.through_seq),
        "fromTree": plan.from_tree,
        "toTree": plan.to_tree,
        "baseline": plan.baseline,
        "checkout": source,
        "patch": patch.as_ref().map(|found| found.patch.clone()),
        "patchTruncatedBytes": patch.as_ref().map(|found| found.truncated_bytes),
        "diffUnavailable": unavailable.as_ref().map(DiffUnavailable::to_json),
    });
    if let (Value::Object(answer), Value::Object(files)) = (&mut answer, plan_files(plan)) {
        answer.extend(files);
    }
    answer
}

/// `bee sessions diff`.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_diff(
    client: &BeekeeperClient,
    channel: &str,
    target: Option<&str>,
    session: Option<&str>,
    turn: Option<&str>,
    checkpoint: Option<&str>,
    scope: DiffScope,
    checkout: Option<&Path>,
    store: Option<&Path>,
) -> Result<(), CliError> {
    validate_uuid(channel)?;
    let (target_key, row) = resolve_target(client, channel, target, session).await?;
    let events = fetch_channel_events(
        client,
        channel,
        &[
            KIND_CODING_SESSION_CHECKPOINT,
            KIND_CODING_SESSION_TRANSCRIPT,
        ],
    )
    .await?;
    let (fold, refused) = read_checkpoints(&events);
    let mine: Vec<CheckpointRecord> = fold
        .checkpoints
        .into_iter()
        .filter(|record| record.target_key == target_key)
        .collect();
    let plan = plan_diff(&mine, turn, checkpoint, scope)?;
    let resolved = match checkout {
        Some(dir) if !dir.is_dir() => {
            return Err(CliError::Usage(format!(
                "--checkout {} is not a directory",
                dir.display()
            )))
        }
        Some(dir) => Ok(Some(Checkout {
            dir: dir.to_path_buf(),
            source: "explicit",
        })),
        None => {
            let session_id = plan
                .checkpoints
                .first()
                .map(|record| record.payload.session.session_id.clone())
                .unwrap_or_default();
            let session_ref = row.as_ref().and_then(|row| row.session_ref.as_deref());
            match store {
                Some(store) => resolve_checkout(store, &session_id, session_ref, channel),
                None => match default_store_path() {
                    Ok(store) => resolve_checkout(&store, &session_id, session_ref, channel),
                    Err(CliError::NotFound(_)) => Ok(None),
                    Err(error) => Err(error.to_string()),
                },
            }
        }
    };
    let mut answer = diff_answer(channel, &target_key, &plan, resolved);
    if let Value::Object(object) = &mut answer {
        let refused: Vec<Value> = refused
            .iter()
            .filter(|refusal| {
                refusal
                    .target_key
                    .as_deref()
                    .is_none_or(|key| key == target_key)
            })
            .map(CheckpointRefusal::to_json)
            .collect();
        object.insert("refused".to_owned(), Value::Array(refused));
    }
    println!("{answer}");
    Ok(())
}

#[path = "checkpoints_git.rs"]
mod git;
use git::clean_oid;
pub use git::{diff_trees_in, resolve_checkout, Checkout, DiffPatch};

#[cfg(test)]
#[path = "checkpoints_tests.rs"]
mod tests;
