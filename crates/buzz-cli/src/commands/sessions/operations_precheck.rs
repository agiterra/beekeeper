//! Pre-publish fold check for every `bee sessions` kind-44244 verb.
//!
//! The fold learned, over four rounds on 2026-09-01, to exclude a bad record
//! on its own account rather than failing the whole session closed. Nothing
//! stopped a seat *writing* one. Every record that caused those four rounds —
//! the runner's `c737be4c` (a report citing an assignment nobody published)
//! and `46b03d08` (a correction that moved its own subject) — would still be
//! signed and published today, and the session would still carry them forever
//! (REVIEW-B1b F2).
//!
//! This module is the writer's half of the same rule: **refuse before
//! signing**. The seat is told the id and the rule, in the same words the fold
//! would have used, at the moment it can still do something about it.
//!
//! # It reads the fold; it never re-implements one
//!
//! Inclusion comes from [`fold_coding_session_team_transactions`], the subject
//! from [`logical_subject`], and the correction rules from
//! `validate_coding_session_team_transaction_supersession`'s payload-level
//! twins. Where a rule could not be reached without a signed event — the
//! envelope checks, which need the tags — it is not duplicated here; those
//! shapes cannot be produced by this CLI in the first place, because it builds
//! the envelope itself.

use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, logical_subject, same_blocker_set,
    terminal_correction_is_allowed, CodingSessionTeamFold, CodingSessionTeamFoldContext,
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CodingSessionTeamTransactionType, TERMINAL_COMPLETION_IS_NOT_REOPENED,
    TERMINAL_PROSE_EDIT_NEEDS_A_NOTE,
};
use buzz_sdk::coding_session_team_transaction::parse_coding_session_team_transaction;
use nostr::Event;

use crate::client::BuzzClient;
use crate::error::CliError;

/// Everything the pre-publish check read, kept so the caller need not fetch the
/// same session twice.
pub(super) struct PrecheckedOperation {
    /// The `supersedes` this record should actually carry.
    ///
    /// The caller's value when it passed every rule, or — for a completion that
    /// named none while the lead's own `mission.blocked` stands as the
    /// canonical terminal — that blocked's event id (B2.7, live finding 14).
    pub(super) supersedes: Option<String>,
    /// The session as read, or `None` when this record pointed at nothing and
    /// the check therefore read nothing.
    pub(super) session: Option<FetchedSession>,
}

/// One read of a session's kind-44244 set and the authority it folds against.
pub(super) struct FetchedSession {
    /// Every kind-44244 event this session holds, as fetched.
    pub(super) events: Vec<Event>,
    /// The authority context those events were folded against.
    pub(super) context: CodingSessionTeamFoldContext,
}

/// The candidate record, before it is signed.
pub(super) struct PrecheckRequest<'a> {
    /// Channel uuid the record will be published into.
    pub(super) channel: &'a str,
    /// Umbrella session reference.
    pub(super) session_ref: &'a str,
    /// Immutable genesis event id of that umbrella.
    pub(super) genesis: &'a str,
    /// Lowercase 64-hex pubkey that is about to sign.
    pub(super) signer_pubkey: &'a str,
    /// The payload as the caller built it, carrying the caller's `supersedes`.
    pub(super) payload: &'a CodingSessionTeamTransactionPayload,
}

/// Refuse this record locally, before signing, when the fold would not accept
/// what it points at.
///
/// Three rules, in the order a reader can act on them:
///
/// 1. every **causal** reference (`assignmentRef`, `reportRef`,
///    `acknowledgedEventRef`, `assignmentRefs`, `requestRef`) names a record
///    this session holds **and** the fold includes;
/// 2. a `supersedes` target exists, was signed by this same key, keeps the
///    operation type (the one legal crossing is `mission.blocked` →
///    `mission.completed`), and keeps its logical subject;
/// 3. a `mission.completed` that named no `supersedes` adopts the lead's own
///    canonical `mission.blocked` as its correction target.
///
/// A `note`'s `refs` and a `decision.request`'s `blocks` are deliberately
/// **not** checked. The fold treats them as pointers rather than causal
/// references precisely so they survive a correction of what they point at, and
/// they may name a kind-44225 transcript item that is not in this set at all.
/// Refusing them here would contradict the reader.
///
/// # It reads nothing when there is nothing to read
///
/// A record that points at no other record — an `assignment`, a `note`, a
/// `decision.request` with no `--supersedes` — has nothing for the fold to
/// judge, so the check short-circuits and the relay is not queried at all.
/// That matters: `bee sessions inbox` already pages the entire channel
/// transcript (COMMS-MAP §2), and adding two unconditional full-session reads
/// to every write would have made a safety check into a bandwidth problem.
pub(super) async fn precheck_operation(
    client: &BuzzClient,
    request: PrecheckRequest<'_>,
) -> Result<PrecheckedOperation, CliError> {
    if request.payload.causal_references().is_empty()
        && request.payload.supersedes.is_none()
        && request.payload.transaction_type != CodingSessionTeamTransactionType::MissionCompleted
    {
        return Ok(PrecheckedOperation {
            supersedes: None,
            session: None,
        });
    }
    let events = super::operations_reads::fetch_transactions(
        client,
        request.channel,
        request.session_ref,
        request.genesis,
    )
    .await?;
    // The policy-gated context, not the bare one: a `mission.completed` this
    // CLI is about to sign is re-folded against it in
    // `verify_completion_before_submit`, and the writer must refuse exactly
    // what the reader would exclude — including `CompletionNotVerified`.
    let context = super::operations_verifier_gate::fetch_context_with_verifier_gate(
        client,
        request.channel,
        request.session_ref,
        request.genesis,
    )
    .await?;
    let fold = fold_coding_session_team_transactions(&events, &context).map_err(|error| {
        CliError::Other(format!(
            "refusing to publish: this session's existing operations do not fold ({error}); \
             read it with `bee sessions operation list` before writing to it"
        ))
    })?;

    let supersedes = check_against_fold(&events, &fold, &request)?;
    Ok(PrecheckedOperation {
        supersedes,
        session: Some(FetchedSession { events, context }),
    })
}

/// The pure half of [`precheck_operation`]: every rule, no I/O.
///
/// Returns the `supersedes` the record should carry.
pub(super) fn check_against_fold(
    events: &[Event],
    fold: &CodingSessionTeamFold,
    request: &PrecheckRequest<'_>,
) -> Result<Option<String>, CliError> {
    let payload = request.payload;
    let transaction_type = payload.transaction_type;

    for reference in payload.causal_references() {
        require_included(events, fold, reference, transaction_type, "reference")?;
    }

    if let Some(target) = payload.supersedes.as_deref() {
        check_supersedes(events, fold, request, target)?;
        return Ok(Some(target.to_owned()));
    }

    if transaction_type == CodingSessionTeamTransactionType::MissionCompleted {
        return Ok(corrected_blocked_terminal(
            events,
            fold,
            request.signer_pubkey,
        ));
    }
    Ok(None)
}

/// The lead's own `mission.blocked`, when that is what this session's canonical
/// terminal currently is.
///
/// Finding 14, live at 02:35: the lead published `mission.blocked` at 21:57,
/// worked for four and a half hours, and completed at 02:35. Neither record
/// could retract the other, so the fold recorded a `terminal` **conflict** and
/// the rail wore a conflict badge over a mission that had simply finished. A
/// completion that corrects the blocked it supersedes folds to **one corrected
/// terminal** instead.
///
/// `None` — leaving the completion uncorrected, exactly as before — when there
/// is no terminal, when the terminal is already a completion, or when the
/// blocked was signed by somebody else. Correcting another author's record is
/// not something a completion may do, and claiming it would be worse than the
/// conflict.
fn corrected_blocked_terminal(
    events: &[Event],
    fold: &CodingSessionTeamFold,
    signer_pubkey: &str,
) -> Option<String> {
    let terminal = fold.canonical_terminal.as_ref()?;
    if terminal.transaction_type != CodingSessionTeamTransactionType::MissionBlocked {
        return None;
    }
    let event = find_event(events, &terminal.event_id)?;
    (event.pubkey.to_hex() == signer_pubkey).then(|| terminal.event_id.clone())
}

/// Refuse a causal reference the fold does not include, naming the id and the
/// rule.
fn require_included(
    events: &[Event],
    fold: &CodingSessionTeamFold,
    reference: &str,
    transaction_type: CodingSessionTeamTransactionType,
    label: &str,
) -> Result<(), CliError> {
    if find_event(events, reference).is_none() {
        return Err(CliError::Usage(format!(
            "refusing to publish this {}: {label} {reference} is not a transaction of this \
             session. A record that cites an id nobody published is excluded by the fold as a \
             dangling reference and stays that way forever — check the id, or publish what it \
             names first.",
            transaction_type.as_str()
        )));
    }
    if let Some(exclusion) = fold.excluded.iter().find(|item| item.event_id == reference) {
        return Err(CliError::Usage(format!(
            "refusing to publish this {}: {reference} is not canonical in this session — the \
             fold excluded it ({:?}: {}). Cite the record that replaced it.",
            transaction_type.as_str(),
            exclusion.code,
            exclusion.reason
        )));
    }
    if !fold.included_event_ids.iter().any(|id| id == reference) {
        return Err(CliError::Usage(format!(
            "refusing to publish this {}: {reference} is present but not included by the fold, \
             and this session cannot say why. Read it with `bee sessions operation get --id \
             {reference}` before citing it.",
            transaction_type.as_str()
        )));
    }
    Ok(())
}

/// Every rule a `--supersedes` must satisfy, in the order the author can act on.
fn check_supersedes(
    events: &[Event],
    fold: &CodingSessionTeamFold,
    request: &PrecheckRequest<'_>,
    target: &str,
) -> Result<(), CliError> {
    let payload = request.payload;
    let transaction_type = payload.transaction_type;
    let Some(previous_event) = find_event(events, target) else {
        return Err(CliError::Usage(format!(
            "--supersedes {target} is refused: it is not a transaction of this session. A \
             correction names a record this session holds; publish a new {} instead.",
            transaction_type.as_str()
        )));
    };
    if previous_event.pubkey.to_hex() != request.signer_pubkey {
        return Err(CliError::Usage(format!(
            "--supersedes {target} is refused: it was signed by {}, and a correction must have \
             the same signed author. Publish a new {} instead.",
            previous_event.pubkey.to_hex(),
            transaction_type.as_str()
        )));
    }
    let previous = parse_coding_session_team_transaction(previous_event).map_err(|error| {
        CliError::Other(format!("--supersedes {target} is unreadable: {error}"))
    })?;

    // The one legal type crossing is the reader's rule, not a copy of it
    // (REVIEW-B2 F5): `terminal_correction_is_allowed` is the same `const fn`
    // `validate_coding_session_team_transaction_supersession` consults.
    if previous.transaction_type != transaction_type
        && !terminal_correction_is_allowed(previous.transaction_type, transaction_type)
    {
        if previous.transaction_type == CodingSessionTeamTransactionType::MissionCompleted
            && transaction_type == CodingSessionTeamTransactionType::MissionBlocked
        {
            return Err(CliError::Usage(format!(
                "--supersedes {target} is refused: {TERMINAL_COMPLETION_IS_NOT_REOPENED}"
            )));
        }
        return Err(CliError::Usage(format!(
            "--supersedes {target} is refused: it is a {}, and a correction must preserve the \
             operation type. Publish a new {} instead.",
            previous.transaction_type.as_str(),
            transaction_type.as_str()
        )));
    }

    if logical_subject(payload) != logical_subject(&previous) {
        return Err(CliError::Usage(format!(
            "--supersedes {target} is refused: a correction changes wording, never its subject; \
             publish a new {} instead. ({} names {}; {target} names {}.)",
            transaction_type.as_str(),
            transaction_type.as_str(),
            logical_subject(payload),
            logical_subject(&previous)
        )));
    }

    if let (
        CodingSessionTeamTransactionBody::MissionBlocked(current_body),
        CodingSessionTeamTransactionBody::MissionBlocked(previous_body),
    ) = (&payload.body, &previous.body)
    {
        if same_blocker_set(&current_body.blockers, &previous_body.blockers) {
            return Err(CliError::Usage(format!(
                "--supersedes {target} is refused: {TERMINAL_PROSE_EDIT_NEEDS_A_NOTE}"
            )));
        }
    }

    require_included(events, fold, target, transaction_type, "--supersedes")
}

/// The supplied event with this id, if the session holds it.
fn find_event<'a>(events: &'a [Event], event_id: &str) -> Option<&'a Event> {
    events.iter().find(|event| event.id.to_hex() == event_id)
}

#[cfg(test)]
#[path = "operations_precheck_tests.rs"]
mod tests;
