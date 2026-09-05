//! Native adapter for the NIP-CSOB observation fold (kind 44246).
//!
//! Desktop never folds 44246 itself. This boundary verifies the signatures,
//! hands the events — the relay's page, newest first — to
//! `buzz_core::fold_coding_session_observation_page` (finding 79), and
//! flattens **its** answer — every collection, every disclosure, every
//! truncation count — for a TypeScript decoder that only checks shape. There
//! is no second implementation of newest-wins, of the `(author, source, gate)`
//! dedupe key, or of the bounds, because two implementations of a wire
//! contract is the one thing the batch rules forbid outright (§0.5, I6).
//!
//! Every key of every row is **present**, with `null` where the record sets
//! nothing: a key that vanished when unset would make "this build has not
//! shipped the field" and "this observation does not set it" look identical,
//! and unknown ≠ empty is the distinction the whole kind exists to keep.

use buzz_core_pkg::coding_session_observation::{
    fold_coding_session_observation_page, CodingSessionObservationDisposition,
    CodingSessionObservationFold, CodingSessionObservationFoldContext,
    CodingSessionObservationGateOutcome, CodingSessionObservationPhase,
    CodingSessionObservationSource,
};
use nostr::Event;
use serde::{Deserialize, Serialize};

/// Closed wire-schema identifier accepted by this boundary.
pub const CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA: &str =
    "buzz-coding-session-observation-fold-request/v1";
/// Closed wire-schema identifier this native adapter answers with.
pub const CODING_SESSION_OBSERVATION_FOLD_ADAPTER_SCHEMA: &str =
    "buzz-coding-session-observation-fold-adapter/v1";

/// The one sentence every surface rendering an observation owes its reader.
///
/// Byte-identical to `bee sessions observations`' own `disclosure` field
/// (`crates/buzz-cli/src/commands/sessions/observations.rs`). Repeated here
/// rather than imported because Desktop does not depend on the CLI crate; the
/// test `the_disclosure_sentence_is_the_clis_own` holds the two together.
pub const OBSERVATION_DISCLOSURE: &str =
    "an observation is something its author saw, not a decision: it settles nothing, authorizes \
     nothing and excludes nothing, and every duration in it is the author's own measurement";

/// Every published kind-44246 for one umbrella, on its way to the fold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionObservationFoldRequest {
    /// Exact closed request-schema identifier.
    pub schema: String,
    /// Umbrella this fold is scoped to.
    pub session_ref: String,
    /// The umbrella's immutable authority anchor.
    pub genesis_ref: String,
    /// Assignment event ids an `assignmentRef` may resolve against.
    ///
    /// Empty means the caller supplied none, which makes every pointer
    /// unresolved — the honest answer, and never an exclusion.
    pub known_assignment_refs: Vec<String>,
    /// Pubkeys whose `observed` claim this session honours (REVIEW-L5 F2).
    ///
    /// `null` is not `[]`: `null` means the caller could not resolve the
    /// session's provider instances, so no claim is checked and the response
    /// says `provenanceChecked: false`. A list — even an empty one — is a real
    /// answer, and an `observed` row from a signer outside it is folded as
    /// `declared` and listed under `misclaimedObserved`.
    pub provider_pubkeys: Option<Vec<String>>,
    /// Raw signed kind-44246 events, verified here before any is read.
    ///
    /// **In the relay's own page order: newest first**, exactly as a `REQ` or
    /// `POST /query` returned them. The adapter folds them as a relay page,
    /// which reverses before folding; a caller that reordered them oldest-first
    /// would get the oldest statement per gate crowned instead (finding 79).
    pub events: Vec<serde_json::Value>,
}

/// One folded checkpoint.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldCheckpoint {
    /// The event this checkpoint was read from.
    pub event_id: String,
    /// Who signed it. The signature is the sole author.
    pub author_pubkey: String,
    /// `observed` or `declared`.
    pub source: String,
    /// The assignment it points at, or null.
    pub assignment_ref: Option<String>,
    /// `planning` / `red` / `green` / `gates` / `reporting`.
    pub phase: String,
    /// Tests written so far in this lane.
    pub tests_written: u32,
    /// Of those, how many the author observed failing first.
    pub tests_red: u32,
    /// Of those, how many now pass.
    pub tests_green: u32,
    /// The last command run, or null.
    pub last_command: Option<String>,
    /// A one-line summary of the last result, or null.
    pub last_summary: Option<String>,
    /// Free prose, or null.
    pub note: Option<String>,
}

/// One folded gate row: the newest statement one author made about one gate.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldGate {
    /// Who signed it.
    pub author_pubkey: String,
    /// `observed` (a mechanism watched it) or `declared` (its subject said so).
    pub source: String,
    /// The newest 16 observations naming this gate; the last is the one shown.
    pub event_ids: Vec<String>,
    /// How many older ids fell off the front of `event_ids`.
    pub dropped_event_ids: usize,
    /// The assignment the newest of them points at, or null.
    pub assignment_ref: Option<String>,
    /// The gate's name, as the brief names it.
    pub gate: String,
    /// `passed` / `failed` / `not-run` — 44244's own three words.
    pub outcome: String,
    /// The exact command that produced the outcome.
    pub command: String,
    /// The command's own summary line, or null.
    pub summary: Option<String>,
    /// The author's own measurement, in milliseconds, or null.
    pub duration_ms: Option<u64>,
    /// The commit the gate ran against, lowercase 40- or 64-hex, or null when
    /// the row names none.
    ///
    /// Null is what a row signed before 2026-09-03 carries, and what a workdir
    /// with no resolvable `HEAD` yields. It is rendered as "no commit", never
    /// as the commit currently checked out.
    pub head_sha: Option<String>,
    /// Whether the worktree carried uncommitted changes when the gate ran, or
    /// null. Present exactly when `head_sha` is.
    pub dirty: Option<bool>,
}

/// One folded finding: the newest disposition one author gave one id.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldFinding {
    /// Who signed it.
    pub author_pubkey: String,
    /// `observed` or `declared`.
    pub source: String,
    /// The newest 16 observations carrying this id.
    pub event_ids: Vec<String>,
    /// How many older ids fell off the front of `event_ids`.
    pub dropped_event_ids: usize,
    /// The assignment the newest of them points at, or null.
    pub assignment_ref: Option<String>,
    /// The author's own stable id for this finding.
    pub finding_id: String,
    /// One line naming the finding.
    pub title: String,
    /// `found` / `fixed` / `cross-lane` / `needs-ruling` / `wont-fix`.
    pub disposition: String,
    /// Free prose, or null.
    pub detail: Option<String>,
    /// Event ids a reader can follow. Pointers, never causal references.
    pub refs: Vec<String>,
    /// The `decision.request` this finding waits on, or null.
    pub decision_ref: Option<String>,
}

/// One folded phase timing. Every number in it is the author's claim.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldPhase {
    /// The event this timing was read from.
    pub event_id: String,
    /// Who signed it.
    pub author_pubkey: String,
    /// `observed` or `declared`.
    pub source: String,
    /// The assignment it points at, or null.
    pub assignment_ref: Option<String>,
    /// The phase's name, in the author's own words.
    pub phase: String,
    /// When the author says it started, in milliseconds since the epoch.
    pub started_at_ms: u64,
    /// When the author says it ended, or null while it runs.
    pub ended_at_ms: Option<u64>,
    /// The author's own measured duration, or null.
    pub duration_ms: Option<u64>,
}

/// One observation whose `assignmentRef` named nothing the caller supplied.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldUnresolved {
    /// The observation.
    pub event_id: String,
    /// The pointer that resolved to nothing, as its author wrote it.
    pub assignment_ref: String,
}

/// One row that claimed `observed` from a signer no provider instance backs.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldMisclaimed {
    /// The observation, folded as `declared`.
    pub event_id: String,
    /// The signer that claimed to be watching.
    pub author_pubkey: String,
}

/// One event the fold could not read, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldIgnored {
    /// The unreadable event.
    pub event_id: String,
    /// One sentence naming the rule it failed.
    pub reason: String,
}

/// How much each collection dropped at its bound.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldTruncation {
    /// Checkpoints not listed.
    pub checkpoints: usize,
    /// Gate rows not listed.
    pub gates: usize,
    /// Findings not listed.
    pub findings: usize,
    /// Phase timings not listed.
    pub phases: usize,
    /// Unresolved pointers not listed.
    pub unresolved: usize,
    /// Ignored events not listed.
    pub ignored: usize,
    /// Misclaimed-provenance rows not listed.
    pub misclaimed_observed: usize,
    /// Event ids dropped from the front of gate and finding entries, summed.
    pub entry_event_ids: usize,
    /// Gate rows a later statement by the same author and provenance replaced.
    pub displaced_gates: usize,
    /// Findings a later disposition by the same author and provenance replaced.
    pub displaced_findings: usize,
}

/// The four observation facts, plus what the fold could not resolve or read.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionObservationFoldResponse {
    /// Exact closed adapter-schema identifier.
    pub schema: String,
    /// Names the crate whose rules produced this, never "desktop".
    pub implementation: String,
    /// The event ids this fold was handed, in the caller's order.
    pub input_event_ids: Vec<String>,
    /// Umbrella this fold was scoped to; echoed so a caller can bind it.
    pub session_ref: String,
    /// Genesis this fold was scoped to; echoed so a caller can bind it.
    pub genesis_ref: String,
    /// Checkpoints, in supplied order. Never empty-by-omission.
    pub checkpoints: Vec<CodingSessionObservationFoldCheckpoint>,
    /// Gate rows, one per `(author, source, gate)`, in first-seen order.
    pub gates: Vec<CodingSessionObservationFoldGate>,
    /// Findings, one per `(author, source, findingId)`, in first-seen order.
    pub findings: Vec<CodingSessionObservationFoldFinding>,
    /// Phase timings, in supplied order.
    pub phases: Vec<CodingSessionObservationFoldPhase>,
    /// Pointers that resolved to nothing the caller supplied.
    pub unresolved: Vec<CodingSessionObservationFoldUnresolved>,
    /// Events this fold could not read at all.
    pub ignored: Vec<CodingSessionObservationFoldIgnored>,
    /// Rows that claimed `observed` without a provider instance behind them.
    pub misclaimed_observed: Vec<CodingSessionObservationFoldMisclaimed>,
    /// Whether the caller supplied the provider set at all. `false` means no
    /// claim in this fold has been verified — not that every one checked out.
    pub provenance_checked: bool,
    /// What each bounded collection dropped.
    pub truncated: CodingSessionObservationFoldTruncation,
    /// The disclosure sentence, byte-identical to the CLI's.
    pub disclosure: String,
}

/// Read a closed-vocabulary word back out as the string the wire carries.
///
/// Through serde rather than a hand-written match, so a sixth checkpoint phase
/// added in `buzz-core` reaches this adapter with no edit here and cannot
/// arrive spelled differently from the way it is signed.
fn vocabulary_word<T: Serialize>(label: &str, value: &T) -> Result<String, String> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(word)) => Ok(word),
        _ => Err(format!("{label} is not a closed-vocabulary word")),
    }
}

fn source_word(source: &CodingSessionObservationSource) -> Result<String, String> {
    vocabulary_word("source", source)
}

fn phase_word(phase: &CodingSessionObservationPhase) -> Result<String, String> {
    vocabulary_word("checkpoint phase", phase)
}

fn outcome_word(outcome: &CodingSessionObservationGateOutcome) -> Result<String, String> {
    vocabulary_word("gate outcome", outcome)
}

fn disposition_word(disposition: &CodingSessionObservationDisposition) -> Result<String, String> {
    vocabulary_word("finding disposition", disposition)
}

fn flatten(
    fold: CodingSessionObservationFold,
    input_event_ids: Vec<String>,
    session_ref: String,
    genesis_ref: String,
) -> Result<CodingSessionObservationFoldResponse, String> {
    Ok(CodingSessionObservationFoldResponse {
        schema: CODING_SESSION_OBSERVATION_FOLD_ADAPTER_SCHEMA.to_owned(),
        implementation: "buzz-core".to_owned(),
        input_event_ids,
        session_ref,
        genesis_ref,
        checkpoints: fold
            .checkpoints
            .into_iter()
            .map(|entry| {
                Ok(CodingSessionObservationFoldCheckpoint {
                    event_id: entry.event_id,
                    author_pubkey: entry.author_pubkey,
                    source: source_word(&entry.source)?,
                    assignment_ref: entry.assignment_ref,
                    phase: phase_word(&entry.body.phase)?,
                    tests_written: entry.body.tests_written,
                    tests_red: entry.body.tests_red,
                    tests_green: entry.body.tests_green,
                    last_command: entry.body.last_command,
                    last_summary: entry.body.last_summary,
                    note: entry.body.note,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        gates: fold
            .gates
            .into_iter()
            .map(|entry| {
                Ok(CodingSessionObservationFoldGate {
                    author_pubkey: entry.author_pubkey,
                    source: source_word(&entry.source)?,
                    event_ids: entry.event_ids,
                    dropped_event_ids: entry.dropped_event_ids,
                    assignment_ref: entry.assignment_ref,
                    gate: entry.row.gate,
                    outcome: outcome_word(&entry.row.outcome)?,
                    command: entry.row.command,
                    summary: entry.row.summary,
                    duration_ms: entry.row.duration_ms,
                    head_sha: entry.row.head_sha,
                    dirty: entry.row.dirty,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        findings: fold
            .findings
            .into_iter()
            .map(|entry| {
                Ok(CodingSessionObservationFoldFinding {
                    author_pubkey: entry.author_pubkey,
                    source: source_word(&entry.source)?,
                    event_ids: entry.event_ids,
                    dropped_event_ids: entry.dropped_event_ids,
                    assignment_ref: entry.assignment_ref,
                    finding_id: entry.body.finding_id,
                    title: entry.body.title,
                    disposition: disposition_word(&entry.body.disposition)?,
                    detail: entry.body.detail,
                    refs: entry.body.refs,
                    decision_ref: entry.body.decision_ref,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        phases: fold
            .phases
            .into_iter()
            .map(|entry| {
                Ok(CodingSessionObservationFoldPhase {
                    event_id: entry.event_id,
                    author_pubkey: entry.author_pubkey,
                    source: source_word(&entry.source)?,
                    assignment_ref: entry.assignment_ref,
                    phase: entry.body.phase,
                    started_at_ms: entry.body.started_at_ms,
                    ended_at_ms: entry.body.ended_at_ms,
                    duration_ms: entry.body.duration_ms,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        unresolved: fold
            .unresolved
            .into_iter()
            .map(|entry| CodingSessionObservationFoldUnresolved {
                event_id: entry.event_id,
                assignment_ref: entry.assignment_ref,
            })
            .collect(),
        ignored: fold
            .ignored
            .into_iter()
            .map(|entry| CodingSessionObservationFoldIgnored {
                event_id: entry.event_id,
                reason: entry.reason,
            })
            .collect(),
        misclaimed_observed: fold
            .misclaimed_observed
            .into_iter()
            .map(|entry| CodingSessionObservationFoldMisclaimed {
                event_id: entry.event_id,
                author_pubkey: entry.author_pubkey,
            })
            .collect(),
        provenance_checked: fold.provenance_checked,
        truncated: CodingSessionObservationFoldTruncation {
            checkpoints: fold.truncated.checkpoints,
            gates: fold.truncated.gates,
            findings: fold.truncated.findings,
            phases: fold.truncated.phases,
            unresolved: fold.truncated.unresolved,
            ignored: fold.truncated.ignored,
            misclaimed_observed: fold.truncated.misclaimed_observed,
            entry_event_ids: fold.truncated.entry_event_ids,
            displaced_gates: fold.truncated.displaced_gates,
            displaced_findings: fold.truncated.displaced_findings,
        },
        disclosure: OBSERVATION_DISCLOSURE.to_owned(),
    })
}

fn fold_adapter(
    request: CodingSessionObservationFoldRequest,
) -> Result<CodingSessionObservationFoldResponse, String> {
    if request.schema != CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA {
        return Err(format!(
            "request.schema must be {CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA}"
        ));
    }
    let mut events: Vec<Event> = Vec::with_capacity(request.events.len());
    for value in request.events {
        let event: Event = serde_json::from_value(value)
            .map_err(|error| format!("events[] is not a signed Nostr event: {error}"))?;
        events.push(event);
    }
    let input_event_ids = events.iter().map(|event| event.id.to_hex()).collect();
    // Signature checking lives inside the fold, which lists an event whose
    // signature does not check under `ignored` rather than failing the set —
    // the difference between this kind and the governance fold, and the whole
    // reason observations are not 44244 subtypes. Nothing here pre-filters,
    // because a filtered event would vanish instead of being disclosed.
    //
    // `request.events` is the relay page as the relay returned it — newest
    // first — so this is the page fold, which reverses before folding. Folded
    // as read, the newest statement per `(author, gate)` lost to the oldest
    // (finding 79); the same fold the relay's push gate runs, so Desktop and
    // the gate cannot crown different rows.
    let fold = fold_coding_session_observation_page(
        &events,
        &CodingSessionObservationFoldContext {
            session_ref: request.session_ref.clone(),
            genesis_ref: request.genesis_ref.clone(),
            known_assignment_refs: request.known_assignment_refs,
            provider_pubkeys: request.provider_pubkeys,
        },
    );
    flatten(
        fold,
        input_event_ids,
        request.session_ref,
        request.genesis_ref,
    )
}

/// Fold an umbrella's published kind-44246 observations.
///
/// The same `buzz-core` fold `bee sessions observations` prints, so Desktop
/// cannot give a second answer about what a session's own records say.
#[tauri::command]
pub async fn fold_coding_session_observations_command(
    request: CodingSessionObservationFoldRequest,
) -> Result<CodingSessionObservationFoldResponse, String> {
    tauri::async_runtime::spawn_blocking(move || fold_adapter(request))
        .await
        .map_err(|error| format!("session-observation fold task failed: {error}"))?
}

#[cfg(test)]
#[path = "coding_session_observation_fold_tests.rs"]
mod tests;
