//! What `bee sessions complete` does when the mission's prerequisites are not
//! all on the wire yet.
//!
//! # The finding this file exists for
//!
//! Ledger 179(a). A local pre-publication check refused `mission.completed`
//! until every named active assignment carried a typed acknowledgement. The
//! lead therefore woke six seats for the sole purpose of collecting receipts;
//! all of them arrived within 70 seconds, the lead's turn ended 15 seconds
//! after the last one without re-evaluating, and the mission's fold sat at
//! 8/8 settled with no terminal at all. Six "housekeeping, no new work" turns
//! cost $9.33, and 14 of that run's 32 turns were disposition or
//! acknowledgement bookkeeping.
//!
//! # What replaces it
//!
//! A completion whose prerequisites are missing is **early, not wrong**. It is
//! published, exits 0, and reports `outcome: pending` with the exact list of
//! facts it is waiting for. The fold re-derives those prerequisites from the
//! supplied set on every read
//! ([`buzz_core::coding_session_team_transaction::CodingSessionTeamPendingCompletion`]),
//! so the record becomes the session's terminal the moment the last one
//! arrives — no second lead turn, no re-publication, nothing to remember.
//! `VISION_COLLABORATION.md` § "Gates must earn their delay": this gate could
//! not name a failure it prevented, only work it created.
//!
//! **Three exclusion classes, and only those three, are waits.** Everything
//! else a completion can be refused for — an unauthorized signer, a dangling
//! or wrong-type pointer, an invalid correction, a newer canonical terminal —
//! stays true however long anyone waits, and is still refused before the event
//! reaches the relay.

use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, CodingSessionTeamAssignmentSettlement,
    CodingSessionTeamFold,
};
use nostr::Event;
use serde_json::{json, Value};

use super::PrecheckedOperation;
use crate::error::CliError;

/// What the fold says about a signed completion that has not been submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CompletionOutcome {
    /// The fold makes this completion the session's canonical terminal.
    Terminal,
    /// The completion is durable and waiting. The value is the `pending`
    /// object the CLI prints, naming every unmet prerequisite.
    Pending(Value),
}

/// The sentence a pending completion prints, so nobody reads "pending" as
/// "rejected" or as "retry this later by hand".
pub(super) const COMPLETION_PENDING_MESSAGE: &str =
    "this completion is published and durable: the fold makes it this session's terminal as soon \
     as the facts listed under `awaiting` are on the wire, with no further turn from anybody. \
     Do not wake seats to collect them and do not publish it again.";

/// Classify a signed completion against the fold, without submitting it.
///
/// Re-folds the session **with** the signed candidate, reusing the event set
/// and authority context the pre-publish check already fetched rather than
/// querying the relay a second time for the same two answers.
pub(super) fn classify_completion_before_submit(
    prechecked: &PrecheckedOperation,
    candidate: &Event,
) -> Result<CompletionOutcome, CliError> {
    // A completion always points at assignments, so the pre-publish check
    // never short-circuits for one and this read is always present. Saying so
    // as a refusal rather than an `expect` keeps the production path free of
    // panics if that ever stops being true.
    let session = prechecked.session.as_ref().ok_or_else(|| {
        CliError::Other(
            "completion verification needs this session's operations and nothing read them".into(),
        )
    })?;
    let mut events = session.events.clone();
    events.push(candidate.clone());
    let fold = fold_coding_session_team_transactions(&events, &session.context)
        .map_err(|error| CliError::Usage(format!("completion verification failed: {error}")))?;
    let candidate_id = candidate.id.to_hex();
    if fold
        .canonical_terminal
        .as_ref()
        .is_some_and(|terminal| terminal.event_id == candidate_id)
    {
        return Ok(CompletionOutcome::Terminal);
    }
    if let Some(pending) = fold
        .pending_completion
        .as_ref()
        .filter(|pending| pending.event_id == candidate_id)
    {
        return Ok(CompletionOutcome::Pending(pending_json(&fold, pending)));
    }
    // Not terminal and not waiting: the fold's own reason when it gave one —
    // a completion can be refused for its signer, its pointers or a newer
    // terminal, and telling the author about the approval chain when what is
    // wrong is the signature would send it to fix the wrong thing.
    let reason = fold
        .excluded
        .iter()
        .find(|item| item.event_id == candidate_id)
        .map(|item| item.reason.clone())
        .unwrap_or_else(|| super::COMPLETION_REFUSED_FALLBACK.to_owned());
    Err(CliError::Usage(format!("completion refused: {reason}")))
}

/// Render the unmet prerequisites of a pending completion.
///
/// Every key is present whether or not it has a value: an absent `awaiting`
/// entry and an empty list are different answers, and an open ruling is a
/// different wait from an incomplete approval chain.
fn pending_json(
    fold: &CodingSessionTeamFold,
    pending: &buzz_core::coding_session_team_transaction::CodingSessionTeamPendingCompletion,
) -> Value {
    let awaiting: Vec<Value> = pending
        .unsettled_assignment_event_ids
        .iter()
        .map(|assignment| {
            let settlement = fold
                .assignments
                .iter()
                .find(|state| &state.assignment_event_id == assignment);
            awaiting_json(assignment, settlement)
        })
        .collect();
    json!({
        "code": buzz_core::team_vocabulary::fold_exclusion_wire_code(pending.code),
        "reason": pending.reason,
        "awaiting": awaiting,
        "waitingOnDecision": fold.waiting_on_decision.as_ref().map(|item| json!({
            "requestId": item.request_event_id,
            "heldOn": item.held_on,
        })),
        "message": COMPLETION_PENDING_MESSAGE,
    })
}

/// One unsettled assignment and the single link it is waiting for.
fn awaiting_json(
    assignment_event_id: &str,
    settlement: Option<&CodingSessionTeamAssignmentSettlement>,
) -> Value {
    let awaiting = settlement.and_then(|state| state.awaiting.as_ref());
    json!({
        "assignmentEventId": assignment_event_id,
        // Present and null only in the shape the fold cannot reach — an
        // unsettled assignment always has exactly one missing link — so a
        // reader never has to tell "no link" from "link not disclosed".
        "link": awaiting.map(|item| item.link.as_str()),
        "owedByRole": awaiting.map(|item| item.owed_by_role.clone()),
        "owedByActor": awaiting.and_then(|item| item.owed_by_actor.clone()),
    })
}

/// Attach the completion's outcome to the answer `bee sessions complete`
/// prints.
///
/// `outcome` and `pending` are present on every completion answer and on no
/// other verb's: `pending` is `null` for a completion that folded terminal, so
/// "nothing is waiting" and "this verb does not have waits" stay distinct.
pub(super) fn disclose_completion_outcome(output: &mut Value, outcome: &CompletionOutcome) {
    let Some(object) = output.as_object_mut() else {
        return;
    };
    match outcome {
        CompletionOutcome::Terminal => {
            object.insert("outcome".into(), Value::String("terminal".into()));
            object.insert("pending".into(), Value::Null);
        }
        CompletionOutcome::Pending(pending) => {
            object.insert("outcome".into(), Value::String("pending".into()));
            object.insert("pending".into(), pending.clone());
        }
    }
}

// ── Coverage before a terminal (lane 201, NIP-PW) ──────────────────────────

/// Refuse a `mission.completed` while the session's adopted plan still has
/// criteria that are open, stale or unknown.
///
/// **Two questions, two answers.** A completion says the mission is done; the
/// NIP-PW coverage fold says whether the adopted plan's criteria are met at
/// one delivered revision. Nothing here merges them — the record is still the
/// same signed 44244 terminal, and `bee sessions work status` still prints
/// coverage and mission as two rows. What this adds is the one check the
/// contract asks for: the CLI checks coverage *before* publishing
/// (`conformance/project-work/README.md` § (c), last paragraph).
///
/// **A session with no declaration is unchanged.** No declaration, no
/// contract to measure against, and this returns `Ok(())` having published
/// and refused nothing — every session that predates NIP-PW completes exactly
/// as it did.
///
/// `--without-coverage "<reason>"` publishes anyway. The reason is **not**
/// written into the 44244 body: that body is `deny_unknown_fields` and its
/// closed shape is what lets every reader decode it, so this prints the
/// disclosure and tells the operator to record it in Pulse rather than
/// quietly editing prose the operator signed.
///
/// # Errors
/// [`CliError::Usage`] listing every criterion that is not covered, with its
/// reason; or the error naming a read that failed.
pub(super) async fn refuse_incomplete_coverage(
    client: &crate::client::BuzzClient,
    transaction_type: buzz_core::coding_session_team_transaction::CodingSessionTeamTransactionType,
    channel: &str,
    session_ref: &str,
    agents_repo: Option<&str>,
    without_coverage: Option<&str>,
) -> Result<(), CliError> {
    use buzz_core::coding_session_team_transaction::CodingSessionTeamTransactionType;
    if transaction_type != CodingSessionTeamTransactionType::MissionCompleted {
        return Ok(());
    }
    let session =
        crate::commands::sessions::work::session_context(client, channel, session_ref).await?;
    let (coverage, _reads) =
        crate::commands::sessions::work::coverage_for_session(client, &session, agents_repo)
            .await?;
    let Some(open) = incomplete_head(&coverage) else {
        return Ok(());
    };
    if let Some(reason) = without_coverage {
        eprintln!(
            "{}",
            json!({
                "coverage": "incomplete",
                "publishedAnyway": true,
                "reason": reason,
                "criteria": open.criteria,
                "why": open.message,
                "recordedIn": "nowhere on the wire",
                "message": format!(
                    "published without coverage: {reason}. This reason is NOT on the wire: the                      kind:44244 completion body is a closed shape and this command will not add                      a key to it. Record it in Pulse (`bee pulse note`) so the next reader finds                      it beside the terminal."
                ),
            })
        );
        return Ok(());
    }
    Err(CliError::Usage(format!(
        "coverage-incomplete: work {} ({}) is not covered — {}{}. Bind the evidence that answers          them (`bee sessions work bind evidence`), read the whole picture with `bee sessions          work status --channel {channel} --session-ref {session_ref}`, or publish anyway with          --without-coverage \"<reason>\" and record the reason in Pulse.",
        &open.work_id,
        open.coverage_reason,
        open.message,
        if open.criteria.is_empty() {
            String::new()
        } else {
            format!(" ({})", open.criteria.join("; "))
        }
    )))
}

/// One current declaration that is not fully covered, rendered for a refusal.
struct IncompleteCoverage {
    work_id: String,
    coverage_reason: String,
    /// Every criterion row that is not `covered`. **May be empty**, and an
    /// empty list is never permission: a plan that could not be read and a
    /// conflicted fork both render no rows at all (ledger 213, finding 5).
    criteria: Vec<String>,
    /// The declaration-level sentence: which clause of coverage failed, in
    /// words, whether or not any criterion row exists to list.
    message: String,
}

/// The first current declaration whose coverage is incomplete, if any.
///
/// **This gate fails closed** (ledger 213, finding 5). It reads the
/// declaration's own facts — its state, whether its plan resolved, whether
/// its work is forked, and `coverageComplete` — and never the *availability
/// of rendered criterion rows*. Those rows are absent in exactly the two
/// cases that most need refusing: a plan the reader could not open renders
/// none (`project_work_fold_project.rs`), and a conflicted declaration
/// deliberately carries none because it is not a current contract.
///
/// `superseded` is the one state that never blocks: it is not a current
/// contract, and its successor is judged on its own.
fn incomplete_head(
    coverage: &buzz_core::project_work_fold::WorkProjection,
) -> Option<IncompleteCoverage> {
    use buzz_core::project_work_fold::WorkDeclarationState;
    coverage
        .declarations
        .iter()
        .find(|declaration| {
            matches!(
                declaration.state,
                WorkDeclarationState::Head
                    | WorkDeclarationState::Stale
                    | WorkDeclarationState::Conflict
            ) && !declaration.coverage_complete
        })
        .map(|declaration| IncompleteCoverage {
            work_id: declaration.work_id.clone(),
            coverage_reason: declaration
                .coverage_reason
                .clone()
                .unwrap_or_else(|| "coverage is incomplete".to_owned()),
            message: incomplete_sentence(declaration, coverage),
            criteria: declaration
                .criteria
                .iter()
                .filter(|criterion| {
                    criterion.status != buzz_core::project_work_fold::WorkCriterionStatus::Covered
                })
                .map(|criterion| {
                    format!(
                        "{} is {}{}",
                        criterion.criterion_id,
                        criterion.status.as_str(),
                        criterion
                            .reason_code
                            .map(|code| format!(" ({})", code.as_str()))
                            .unwrap_or_default()
                    )
                })
                .collect(),
        })
}

/// Why this declaration blocks a terminal, in one sentence a person can act
/// on — including the two cases that render no criterion rows at all.
fn incomplete_sentence(
    declaration: &buzz_core::project_work_fold::WorkDeclarationProjection,
    coverage: &buzz_core::project_work_fold::WorkProjection,
) -> String {
    use buzz_core::project_work_fold::WorkDeclarationState;
    if declaration.state == WorkDeclarationState::Conflict {
        let heads = coverage
            .conflicts
            .iter()
            .find(|conflict| conflict.work_id == declaration.work_id)
            .map(|conflict| conflict.heads.join(", "))
            .unwrap_or_default();
        return format!(
            "this work is in conflict: more than one declaration is a head ({heads}), so \
             nothing here is a single current contract. Resolve it by adopting one declaration \
             that supersedes every head"
        );
    }
    if !declaration.plan_resolved {
        return format!(
            "this declaration's plan could not be read at the commit it pins ({}@{}), so its \
             criteria are unknown rather than met. Pass --agents-repo <dir> so the plan can be \
             read at that commit",
            declaration.plan_ref.path,
            &declaration.plan_ref.commit[..declaration.plan_ref.commit.len().min(12)]
        );
    }
    declaration
        .coverage_reason
        .clone()
        .unwrap_or_else(|| "this declaration's coverage is incomplete".to_owned())
}

#[cfg(test)]
#[path = "operations_completion_tests.rs"]
mod tests;
