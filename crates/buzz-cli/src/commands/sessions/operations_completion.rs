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

#[cfg(test)]
#[path = "operations_completion_tests.rs"]
mod tests;
