//! The `fold` object `bee sessions operation list|get` prints.
//!
//! A child of [`super::operations`], split out only so that file stays under
//! the repository's 1,000-line ceiling. No behaviour change.

use buzz_core::coding_session_team_transaction::CodingSessionTeamFold;
use serde_json::{json, Value};

/// Render one fold as the CLI's `fold` object.
///
/// Every collection is present even when empty, and every nullable field is
/// present as `null`: "not disclosed" and "empty" are different answers and
/// this surface must never merge them.
///
/// `excluded[].code` is the **snake_case wire spelling**
/// ([`buzz_core::team_vocabulary::fold_exclusion_wire_code`]), the same word
/// the Tauri adapter emits and the same word `bee sessions explain <word>`
/// answers to. It used to be the Rust `Debug` spelling here and snake_case
/// there — one code, two names, depending on which surface a seat happened to
/// read (REVIEW-L13 F5). `explain` still accepts both spellings, so a seat that
/// copied the old one still gets an answer.
pub(super) fn fold_json(fold: &CodingSessionTeamFold) -> Value {
    json!({
        "includedEventIds": fold.included_event_ids,
        "excluded": fold.excluded.iter().map(|item| json!({
            "eventId": item.event_id,
            "code": buzz_core::team_vocabulary::fold_exclusion_wire_code(item.code),
            "reason": item.reason,
        })).collect::<Vec<_>>(),
        "conflicts": fold.conflicts.iter().map(|item| json!({
            "subject": item.subject,
            "winnerEventId": item.winner_event_id,
            "contenderEventIds": item.contender_event_ids,
        })).collect::<Vec<_>>(),
        "assignments": fold.assignments.iter().map(|item| json!({
            "assignmentEventId": item.assignment_event_id,
            "governedReportEventId": item.governed_report_event_id,
            "dispositionEventId": item.disposition_event_id,
            "acknowledgementEventId": item.acknowledgement_event_id,
            "settled": item.settled,
            // Ledger 178(e): the four values above say *that* a chain is
            // incomplete and never *where*, and a lead read those nulls as a
            // missing refutation and recalled a verifier that owed nothing.
            // This names the one absent link and the party who owes it, and is
            // null exactly when `settled` is true.
            "awaiting": item.awaiting.as_ref().map(|awaiting| json!({
                "link": awaiting.link.as_str(),
                "owedByRole": awaiting.owed_by_role,
                "owedByActor": awaiting.owed_by_actor,
            })),
        })).collect::<Vec<_>>(),
        // Disclosure, not exclusion: these reports ARE canonical. The seat is
        // the separate fact — see `bee sessions seat-repair`.
        "unseatedReports": fold.unseated_reports.iter().map(|item| json!({
            "eventId": item.event_id,
            "authorPubkey": item.author_pubkey,
            "assignmentRef": item.assignment_ref,
            "assigneeRole": item.assignee_role,
        })).collect::<Vec<_>>(),
        // Listed, never folded into state: a note changes nothing, and the
        // rail must be able to show what was said without reading it as a
        // phase change.
        "notes": fold.notes.iter().map(|item| json!({
            "eventId": item.event_id,
            "authorPubkey": item.author_pubkey,
            "refs": item.refs,
        })).collect::<Vec<_>>(),
        "decisions": fold.decisions.iter().map(|item| json!({
            "requestId": item.request_event_id,
            "heldOn": item.held_on,
            "blocks": item.blocks,
            // Present and null while the question stands open — never absent,
            // so "unanswered" and "not disclosed" stay different answers.
            "answeredBy": item.answered_by,
            "answerId": item.answer_event_id,
        })).collect::<Vec<_>>(),
        // The mission is waiting on a person. That is not a terminal, and it
        // is exactly the state `mission.blocked` was being used to fake.
        "waitingOnDecision": fold.waiting_on_decision.as_ref().map(|item| json!({
            "requestId": item.request_event_id,
            "heldOn": item.held_on,
        })),
        "canonicalTerminal": fold.canonical_terminal.as_ref().map(|item| json!({
            "eventId": item.event_id,
            "type": item.transaction_type.as_str(),
        })),
        // A completion that is early rather than wrong (ledger 179(a)): it is
        // published and durable, and it becomes `canonicalTerminal` the moment
        // the facts it names arrive, with no further turn from anybody.
        "pendingCompletion": fold.pending_completion.as_ref().map(|item| json!({
            "eventId": item.event_id,
            "code": buzz_core::team_vocabulary::fold_exclusion_wire_code(item.code),
            "reason": item.reason,
            "unsettledAssignmentEventIds": item.unsettled_assignment_event_ids,
        })),
    })
}
