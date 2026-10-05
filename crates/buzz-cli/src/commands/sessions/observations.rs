//! `bee sessions observe` and `bee sessions observations` — the writer and
//! reader for NIP-CSOB observations (kind 44246).
//!
//! Four facts a person watching an agent team needs and cannot get from the
//! governance record: where a seat is in its own loop, what its gates said,
//! what it found and did about it, and how long a phase took.
//!
//! # What these commands may and may not claim
//!
//! An observation **settles nothing**. It grants nothing, blocks nothing,
//! excludes nothing and corrects nothing; a mission's state is decided entirely
//! by kind 44244 and is not readable here. Every duration and `startedAtMs` is
//! the author's own measurement, printed as the author's claim, and it is never
//! used for ordering, discovery or dedupe — "newest" here is the relay's own
//! storage order. The relay hands a page over newest-first, and the reader
//! reverses it before folding (`fold_coding_session_observation_page`,
//! finding 79): folded as read, a seat's first row per gate printed as its
//! current one.
//!
//! # Standing
//!
//! **Any active seat or the founder may observe, and the relay validates
//! structure only** — NIP-CSOB's own boundary, the same one NIP-CSP draws. This
//! command therefore does not refuse before signing the way `policy set` does:
//! there is nothing an unseated observation could bind that a reader would then
//! have to un-believe, because an observation binds nothing in the first place.
//! It is the reader that says who wrote what.

use buzz_core::coding_session_observation::{
    fold_coding_session_observation_page, CodingSessionObservationBody,
    CodingSessionObservationCheckpoint, CodingSessionObservationFinding,
    CodingSessionObservationFold, CodingSessionObservationFoldContext,
    CodingSessionObservationGate, CodingSessionObservationGateRow,
    CodingSessionObservationGateStartEntry, CodingSessionObservationPayload,
    CodingSessionObservationPhaseTiming, CodingSessionObservationSource,
    CODING_SESSION_OBSERVATION_SCHEMA, GATE_START_PHASE_PREFIX, GATE_START_STALE_AFTER_MS,
};
use buzz_core::kind::{KIND_CODING_SESSION_OBSERVATION, KIND_CODING_SESSION_TEAM_TRANSACTION};
use buzz_sdk::coding_session_observation::build_coding_session_observation;
use nostr::Event;
use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_uuid};
use crate::{SessionObserveCmd, SessionObserveGateArgs};

/// The one sentence every surface that renders an observation owes its reader.
pub const OBSERVATION_DISCLOSURE: &str =
    "an observation is something its author saw, not a decision: it settles nothing, authorizes \
     nothing and excludes nothing, and every duration in it is the author's own measurement";

/// Dispatch `bee sessions observe`.
pub async fn cmd_observe(client: &BuzzClient, cmd: SessionObserveCmd) -> Result<(), CliError> {
    cmd_observe_as(client, cmd, CodingSessionObservationSource::Declared).await
}

/// `bee sessions observe`, with the caller naming the record's `source`.
///
/// Crate-internal and deliberately narrow. The only caller that may name
/// anything but `Declared` is the registry bench (`registry_measure`), whose
/// rows are produced by a mechanical scorer running a fixed task set — not by
/// a subject describing its own work. `observed` is never reachable from here:
/// a row nobody watched cannot become one by asking.
pub(super) async fn cmd_observe_as(
    client: &BuzzClient,
    cmd: SessionObserveCmd,
    source: CodingSessionObservationSource,
) -> Result<(), CliError> {
    if source == CodingSessionObservationSource::Observed {
        return Err(CliError::Usage(
            "`observed` is published by the mechanism that watched, never by this command".into(),
        ));
    }
    cmd_observe_inner(client, cmd, source).await
}

async fn cmd_observe_inner(
    client: &BuzzClient,
    cmd: SessionObserveCmd,
    source: CodingSessionObservationSource,
) -> Result<(), CliError> {
    match cmd {
        SessionObserveCmd::Checkpoint(args) => {
            let body =
                CodingSessionObservationBody::Checkpoint(CodingSessionObservationCheckpoint {
                    phase: closed_word(
                        "--phase",
                        &args.phase,
                        &["planning", "red", "green", "gates", "reporting"],
                    )?,
                    tests_written: args.tests_written,
                    tests_red: args.tests_red,
                    tests_green: args.tests_green,
                    last_command: args.last_command.clone(),
                    last_summary: args.last_summary.clone(),
                    note: args.note.clone(),
                });
            publish(
                client,
                &args.channel,
                &args.session_ref,
                args.genesis.as_deref(),
                args.assignment.as_deref(),
                body,
                source,
            )
            .await
        }
        SessionObserveCmd::Gate(args) => {
            let body = CodingSessionObservationBody::Gate(CodingSessionObservationGate {
                rows: gate_rows(&args)?,
            });
            publish(
                client,
                &args.channel,
                &args.session_ref,
                args.genesis.as_deref(),
                args.assignment.as_deref(),
                body,
                source,
            )
            .await
        }
        SessionObserveCmd::Finding(args) => {
            for reference in &args.reference {
                validate_lower_hex64("--ref", reference)?;
            }
            if let Some(decision) = &args.decision {
                validate_lower_hex64("--decision", decision)?;
            }
            let body = CodingSessionObservationBody::Finding(CodingSessionObservationFinding {
                finding_id: args.finding_id.clone(),
                title: args.title.clone(),
                disposition: closed_word(
                    "--disposition",
                    &args.disposition,
                    &["found", "fixed", "cross-lane", "needs-ruling", "wont-fix"],
                )?,
                detail: args.detail.clone(),
                refs: args.reference.clone(),
                decision_ref: args.decision.clone(),
            });
            publish(
                client,
                &args.channel,
                &args.session_ref,
                args.genesis.as_deref(),
                args.assignment.as_deref(),
                body,
                source,
            )
            .await
        }
        SessionObserveCmd::Phase(args) => {
            refuse_reserved_gate_phase(&args.phase)?;
            let body = CodingSessionObservationBody::Phase(CodingSessionObservationPhaseTiming {
                phase: args.phase.clone(),
                started_at_ms: args.started_at_ms,
                ended_at_ms: args.ended_at_ms,
                duration_ms: args.duration_ms,
            });
            publish(
                client,
                &args.channel,
                &args.session_ref,
                args.genesis.as_deref(),
                args.assignment.as_deref(),
                body,
                source,
            )
            .await
        }
    }
}

/// SV-41: `gate:` phases are the provider's gate starts.
///
/// A seat's own `gate:` phase would fold as an ordinary declared phase and
/// never as a start, so writing one could only confuse a reader comparing the
/// two; a seat states a gate's result with `observe gate`.
pub(super) fn refuse_reserved_gate_phase(phase: &str) -> Result<(), CliError> {
    if phase.starts_with(GATE_START_PHASE_PREFIX) {
        return Err(CliError::Usage(
            "the gate: prefix is the provider's; a seat states a gate with `observe gate`".into(),
        ));
    }
    Ok(())
}

/// Build the gate rows from the repeated `--gate name:outcome:command` flag.
///
/// One flag per row, parsed on the two first colons so a command may contain
/// colons of its own. The row's optional `summary` and `durationMs` are set
/// through their own repeated flags, positionally paired with the rows.
pub(super) fn gate_rows(
    args: &SessionObserveGateArgs,
) -> Result<Vec<CodingSessionObservationGateRow>, CliError> {
    if args.gate.is_empty() {
        return Err(CliError::Usage(
            "`observe gate` needs at least one --gate NAME:OUTCOME:COMMAND — an observation with \
             no rows claims to state something and states nothing"
                .into(),
        ));
    }
    let mut rows = Vec::with_capacity(args.gate.len());
    for (index, raw) in args.gate.iter().enumerate() {
        let (gate, rest) = raw.split_once(':').ok_or_else(|| {
            CliError::Usage(format!(
                "--gate {raw:?} must be NAME:OUTCOME:COMMAND (outcome is passed, failed or \
                 not-run)"
            ))
        })?;
        let (outcome, command) = rest.split_once(':').ok_or_else(|| {
            CliError::Usage(format!(
                "--gate {raw:?} must be NAME:OUTCOME:COMMAND (outcome is passed, failed or \
                 not-run)"
            ))
        })?;
        let (head_sha, dirty) = match args.head_sha.get(index) {
            Some(raw) => {
                let (head_sha, dirty) = head_sha_of(raw)?;
                (Some(head_sha), Some(dirty))
            }
            None => (None, None),
        };
        rows.push(CodingSessionObservationGateRow {
            gate: gate.to_owned(),
            outcome: closed_word("--gate outcome", outcome, &["passed", "failed", "not-run"])?,
            command: command.to_owned(),
            summary: args.summary.get(index).cloned(),
            duration_ms: args.duration_ms.get(index).copied(),
            head_sha,
            dirty,
        });
    }
    Ok(rows)
}

/// Parse one `--head-sha SHA:clean|dirty` value.
///
/// The cleanliness word is mandatory rather than defaulted. A default of
/// `clean` would let a seat name a commit its worktree did not match by
/// leaving a word out, and a default of `dirty` would make the flag useless;
/// the honest shape is to make the author say which, in a value the wire
/// refuses to carry half of.
fn head_sha_of(raw: &str) -> Result<(String, bool), CliError> {
    let (head_sha, word) = raw.split_once(':').ok_or_else(|| {
        CliError::Usage(format!(
            "--head-sha {raw:?} must be SHA:clean or SHA:dirty — a commit named without saying \
             whether the tree matched it is not evidence about that commit"
        ))
    })?;
    let dirty = match word {
        "clean" => false,
        "dirty" => true,
        _ => {
            return Err(CliError::Usage(format!(
                "--head-sha {raw:?} must end in :clean or :dirty"
            )))
        }
    };
    if !matches!(head_sha.len(), 40 | 64)
        || !head_sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(CliError::Usage(format!(
            "--head-sha {raw:?} must name a lowercase 40- or 64-hex git object id"
        )));
    }
    Ok((head_sha.to_owned(), dirty))
}

/// Parse one word of a closed vocabulary through the record's own serde
/// definition, so the CLI can never accept a word the wire refuses.
fn closed_word<T: serde::de::DeserializeOwned>(
    flag: &str,
    value: &str,
    vocabulary: &[&str],
) -> Result<T, CliError> {
    serde_json::from_value::<T>(Value::String(value.to_owned()))
        .map_err(|_| CliError::Usage(format!("{flag} must be one of: {}", vocabulary.join(", "))))
}

/// Resolve the umbrella's genesis, from the flag or from the relay.
async fn genesis_for(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: Option<&str>,
) -> Result<String, CliError> {
    match genesis {
        Some(genesis) => {
            validate_lower_hex64("--genesis", genesis)?;
            Ok(genesis.to_owned())
        }
        None => {
            let events = super::fetch_channel_events(
                client,
                channel,
                &[buzz_core::kind::KIND_CODING_SESSION_GENESIS],
            )
            .await?;
            super::crew::resolve_umbrella_genesis(&events, session_ref)
        }
    }
}

async fn publish(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: Option<&str>,
    assignment: Option<&str>,
    body: CodingSessionObservationBody,
    source: CodingSessionObservationSource,
) -> Result<(), CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;
    if let Some(assignment) = assignment {
        validate_lower_hex64("--assignment", assignment)?;
    }
    let genesis_ref = genesis_for(client, channel, session_ref, genesis).await?;
    let payload = CodingSessionObservationPayload {
        schema: CODING_SESSION_OBSERVATION_SCHEMA.to_owned(),
        session_ref: session_ref.to_owned(),
        genesis_ref,
        observation_type: body.observation_type(),
        // `bee sessions observe` is a seat speaking about its own work, so
        // every row it publishes is a **claim** — `cmd_observe` passes
        // `Declared` and there is deliberately no flag to say otherwise: a
        // subject that could label itself `observed` would erase the only
        // distinction the field carries. Observed rows are published by the
        // mechanism that watched — today the session provider, from the seat's
        // own tool calls. The registry bench is the one caller that names
        // anything else, and it names `measured`.
        source,
        assignment_ref: assignment.map(str::to_owned),
        body,
    };
    let observation_type = payload.observation_type.as_str();
    let builder = build_coding_session_observation(channel, payload)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let event = client.sign_event_unchecked(builder)?;
    let event_id = event.id.to_hex();
    client.submit_event(event).await?;
    println!(
        "{}",
        json!({
            "eventId": event_id,
            "accepted": true,
            "type": observation_type,
            "source": source.as_str(),
            "disclosure": OBSERVATION_DISCLOSURE,
        })
    );
    Ok(())
}

/// `bee sessions observations` — print the bounded observation fold.
pub async fn cmd_observations(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;
    let genesis_ref = genesis_for(client, channel, session_ref, genesis).await?;

    // The assignments an `assignmentRef` may resolve against are governance
    // records on a different kind, read here only so a dangling pointer can be
    // *disclosed*. Nothing about them changes what this fold includes.
    let assignments = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
            "#h": [channel],
            "#d": [session_ref],
            "#cstx-type": ["assignment"],
        }))
        .await?;
    let known_assignment_refs: Vec<String> = assignments
        .iter()
        .filter_map(|value| value.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();

    let values = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_OBSERVATION],
            "#h": [channel],
            "#d": [session_ref],
            "#csob-genesis": [genesis_ref],
        }))
        .await?;
    let events: Vec<Event> = values
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .collect();
    // `query_all` concatenates relay pages in the relay's own order — newest
    // first — so this is a page fold, which reverses before folding. Folding
    // it as read rendered a seat's *first* row per gate as the current one
    // (finding 79).
    let fold = fold_coding_session_observation_page(
        &events,
        &CodingSessionObservationFoldContext {
            session_ref: session_ref.to_owned(),
            genesis_ref,
            known_assignment_refs,
            // REVIEW-L5 F2. This reader does not resolve the umbrella's
            // provider instances, so it verifies no `observed` claim and says
            // so (`provenanceChecked: false`) rather than either trusting a
            // claim or downgrading an honest one. Resolving them here is one
            // more query against the 44221 creates — a named follow-on, not a
            // silence.
            provider_pubkeys: None,
        },
    );
    match format {
        crate::OutputFormat::Compact => {
            for line in compact_rows(&fold) {
                println!("{line}");
            }
        }
        _ => println!("{}", fold_json(&fold)),
    }
    Ok(())
}

/// One line per fact, for a reader that wants the shape at a glance.
pub(super) fn compact_rows(fold: &CodingSessionObservationFold) -> Vec<String> {
    compact_rows_at(fold, now_ms())
}

/// The reader's own clock, for the gate-start stale rule only.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// What a gate start reads as at `now_ms`: `running`, `ended <ms>`, or — past
/// the stale rule with no close — `no result observed`, never `running`.
///
/// Only a measured close (`durationMs` set) is the gate ending. A close with
/// `durationMs: null` and `endedAtMs > startedAtMs` is the provider no longer
/// watching (evicted, turn end, session exit; NIP-CSOB § Gate start) — the
/// command may still be running — so it reads `stopped watching <ms> (end not
/// observed)`. A null-duration close at `startedAtMs` (a clock that ran
/// backwards) cannot say which it was and claims neither.
pub(super) fn gate_start_state(
    entry: &CodingSessionObservationGateStartEntry,
    now_ms: u64,
) -> String {
    match &entry.close {
        Some(close) if close.duration_ms.is_some() => format!("ended {}", close.ended_at_ms),
        Some(close) if close.ended_at_ms > entry.started_at_ms => {
            format!("stopped watching {} (end not observed)", close.ended_at_ms)
        }
        Some(close) => format!("closed {} (no measured span)", close.ended_at_ms),
        None if buzz_core::coding_session_observation::gate_start_is_stale(
            entry.started_at_ms,
            now_ms,
        ) =>
        {
            "no result observed".to_owned()
        }
        None => "running".to_owned(),
    }
}

/// [`compact_rows`] with the clock supplied.
pub(super) fn compact_rows_at(fold: &CodingSessionObservationFold, now_ms: u64) -> Vec<String> {
    let mut rows = Vec::new();
    for entry in &fold.checkpoints {
        rows.push(format!(
            // Each count labelled. The row used to print `tests_written`
            // twice, so `5/5 red 3 green 2` read as a ratio that was always
            // 100% (REVIEW-L1 F5).
            "checkpoint {} {} phase {} written {} red {} green {}",
            short(&entry.author_pubkey),
            entry.source.as_str(),
            phase_word(entry.body.phase),
            entry.body.tests_written,
            entry.body.tests_red,
            entry.body.tests_green
        ));
    }
    for entry in &fold.gates {
        rows.push(format!(
            "gate {} {} {} {}{}",
            short(&entry.author_pubkey),
            entry.source.as_str(),
            entry.row.gate,
            outcome_word(entry.row.outcome),
            dropped_suffix(entry.dropped_event_ids)
        ));
    }
    for entry in &fold.findings {
        rows.push(format!(
            "finding {} {} {} {} {}{}",
            short(&entry.author_pubkey),
            entry.source.as_str(),
            entry.body.finding_id,
            disposition_word(entry.body.disposition),
            entry.body.title,
            dropped_suffix(entry.dropped_event_ids)
        ));
    }
    for entry in &fold.phases {
        rows.push(format!(
            "phase {} {} {} started {} (author's own measurement)",
            short(&entry.author_pubkey),
            entry.source.as_str(),
            entry.body.phase,
            entry.body.started_at_ms
        ));
    }
    for entry in &fold.gate_starts {
        rows.push(format!(
            "gate-start {} observed {} started {} (author's own measurement) {}",
            short(&entry.author_pubkey),
            entry.gate,
            entry.started_at_ms,
            gate_start_state(entry, now_ms)
        ));
    }
    for entry in &fold.unresolved {
        rows.push(format!(
            "unresolved {} assignmentRef {}",
            short(&entry.event_id),
            short(&entry.assignment_ref)
        ));
    }
    for entry in &fold.ignored {
        rows.push(format!(
            "ignored {} {}",
            short(&entry.event_id),
            entry.reason
        ));
    }
    if fold.truncated.any() {
        rows.push(format!(
            "truncated checkpoints {} gates {} findings {} phases {} unresolved {} ignored {} \
             entryEventIds {} displacedGates {} displacedFindings {} misclaimedObserved {} \
             gateStarts {}",
            fold.truncated.checkpoints,
            fold.truncated.gates,
            fold.truncated.findings,
            fold.truncated.phases,
            fold.truncated.unresolved,
            fold.truncated.ignored,
            fold.truncated.entry_event_ids,
            fold.truncated.displaced_gates,
            fold.truncated.displaced_findings,
            fold.truncated.misclaimed_observed,
            fold.truncated.gate_starts
        ));
    }
    if fold.truncated.gate_start_closes_unmatched > 0 {
        rows.push(format!(
            "gate-start closes with no start on this page {}",
            fold.truncated.gate_start_closes_unmatched
        ));
    }
    rows
}

/// The " (+N older ids not listed)" an entry that dropped some carries.
///
/// Empty when nothing was dropped: a reader must be able to tell "all of them"
/// from "the newest 16 of many" without counting.
fn dropped_suffix(dropped: usize) -> String {
    if dropped == 0 {
        return String::new();
    }
    format!(" (+{dropped} older ids not listed)")
}

fn short(value: &str) -> String {
    value.chars().take(8).collect()
}

fn phase_word(
    phase: buzz_core::coding_session_observation::CodingSessionObservationPhase,
) -> &'static str {
    use buzz_core::coding_session_observation::CodingSessionObservationPhase as Phase;
    match phase {
        Phase::Planning => "planning",
        Phase::Red => "red",
        Phase::Green => "green",
        Phase::Gates => "gates",
        Phase::Reporting => "reporting",
    }
}

fn outcome_word(
    outcome: buzz_core::coding_session_observation::CodingSessionObservationGateOutcome,
) -> &'static str {
    use buzz_core::coding_session_observation::CodingSessionObservationGateOutcome as Outcome;
    match outcome {
        Outcome::Passed => "passed",
        Outcome::Failed => "failed",
        Outcome::NotRun => "not-run",
    }
}

fn disposition_word(
    disposition: buzz_core::coding_session_observation::CodingSessionObservationDisposition,
) -> &'static str {
    use buzz_core::coding_session_observation::CodingSessionObservationDisposition as Disposition;
    match disposition {
        Disposition::Found => "found",
        Disposition::Fixed => "fixed",
        Disposition::CrossLane => "cross-lane",
        Disposition::NeedsRuling => "needs-ruling",
        Disposition::WontFix => "wont-fix",
    }
}

/// The wire shape `bee sessions observations` prints.
///
/// Every collection is present even when empty, and truncation is a number
/// rather than a silence: unknown is not empty, and empty is not zero-dropped.
pub(super) fn fold_json(fold: &CodingSessionObservationFold) -> Value {
    fold_json_at(fold, now_ms())
}

/// [`fold_json`] with the clock supplied.
pub(super) fn fold_json_at(fold: &CodingSessionObservationFold, now_ms: u64) -> Value {
    json!({
        "checkpoints": fold.checkpoints.iter().map(|entry| json!({
            "eventId": entry.event_id,
            "author": entry.author_pubkey,
            "source": entry.source.as_str(),
            "assignmentRef": entry.assignment_ref,
            "phase": phase_word(entry.body.phase),
            "testsWritten": entry.body.tests_written,
            "testsRed": entry.body.tests_red,
            "testsGreen": entry.body.tests_green,
            "lastCommand": entry.body.last_command,
            "lastSummary": entry.body.last_summary,
            "note": entry.body.note,
        })).collect::<Vec<_>>(),
        "gates": fold.gates.iter().map(|entry| json!({
            "author": entry.author_pubkey,
            "eventIds": entry.event_ids,
            "source": entry.source.as_str(),
            "droppedEventIds": entry.dropped_event_ids,
            "assignmentRef": entry.assignment_ref,
            "gate": entry.row.gate,
            "outcome": outcome_word(entry.row.outcome),
            "command": entry.row.command,
            "summary": entry.row.summary,
            "durationMs": entry.row.duration_ms,
            // The commit the row ran at and whether the tree was clean: the
            // two facts the push gate rules on, and the two this reader did
            // not print while finding 79 was being diagnosed. `null` when the
            // row predates the keys or a declared row carried none.
            "headSha": entry.row.head_sha,
            "dirty": entry.row.dirty,
        })).collect::<Vec<_>>(),
        "findings": fold.findings.iter().map(|entry| json!({
            "author": entry.author_pubkey,
            "eventIds": entry.event_ids,
            "source": entry.source.as_str(),
            "droppedEventIds": entry.dropped_event_ids,
            "assignmentRef": entry.assignment_ref,
            "findingId": entry.body.finding_id,
            "title": entry.body.title,
            "disposition": disposition_word(entry.body.disposition),
            "detail": entry.body.detail,
            "refs": entry.body.refs,
            "decisionRef": entry.body.decision_ref,
        })).collect::<Vec<_>>(),
        "phases": fold.phases.iter().map(|entry| json!({
            "eventId": entry.event_id,
            "author": entry.author_pubkey,
            "source": entry.source.as_str(),
            "assignmentRef": entry.assignment_ref,
            "phase": entry.body.phase,
            "startedAtMs": entry.body.started_at_ms,
            "endedAtMs": entry.body.ended_at_ms,
            "durationMs": entry.body.duration_ms,
        })).collect::<Vec<_>>(),
        "gateStarts": fold.gate_starts.iter().map(|entry| json!({
            "eventId": entry.event_id,
            "author": entry.author_pubkey,
            "source": "observed",
            "assignmentRef": entry.assignment_ref,
            "gate": entry.gate,
            "startedAtMs": entry.started_at_ms,
            "closeEventId": entry.close.as_ref().map(|close| close.event_id.clone()),
            "endedAtMs": entry.close.as_ref().map(|close| close.ended_at_ms),
            "durationMs": entry.close.as_ref().and_then(|close| close.duration_ms),
            "state": gate_start_state(entry, now_ms),
        })).collect::<Vec<_>>(),
        "gateStartStaleAfterMs": GATE_START_STALE_AFTER_MS,
        "unresolved": fold.unresolved.iter().map(|entry| json!({
            "eventId": entry.event_id,
            "assignmentRef": entry.assignment_ref,
        })).collect::<Vec<_>>(),
        "ignored": fold.ignored.iter().map(|entry| json!({
            "eventId": entry.event_id,
            "reason": entry.reason,
        })).collect::<Vec<_>>(),
        "truncated": {
            "displacedGates": fold.truncated.displaced_gates,
            "displacedFindings": fold.truncated.displaced_findings,
            "misclaimedObserved": fold.truncated.misclaimed_observed,
            "checkpoints": fold.truncated.checkpoints,
            "gates": fold.truncated.gates,
            "findings": fold.truncated.findings,
            "phases": fold.truncated.phases,
            "unresolved": fold.truncated.unresolved,
            "ignored": fold.truncated.ignored,
            "entryEventIds": fold.truncated.entry_event_ids,
            "gateStarts": fold.truncated.gate_starts,
            "gateStartClosesUnmatched": fold.truncated.gate_start_closes_unmatched,
        },
        "misclaimedObserved": fold.misclaimed_observed.iter().map(|entry| json!({
            "eventId": entry.event_id,
            "author": entry.author_pubkey,
        })).collect::<Vec<_>>(),
        "provenanceChecked": fold.provenance_checked,
        "disclosure": OBSERVATION_DISCLOSURE,
    })
}

#[cfg(test)]
#[path = "observations_tests.rs"]
mod tests;
