//! The one rule that reads `gates.verifierRequired` (batch 3, item G).
//!
//! A child of `coding_session_team_transaction_fold`, in its own file for the
//! same reason every other piece of that fold is: no file here passes 1,000
//! lines. `use super::*` gives it the parent's private `Record` and helpers
//! exactly as if it were written inline.
//!
//! # What this decides, and what it refuses to decide
//!
//! It decides one thing: whether every assignment a `mission.completed` names
//! **and the fold settled** carries a verifier's ruling on the report that
//! settlement is about. It decides nothing about branches, commits or the
//! quality of anything — `landedShas` is not read here, and a report the
//! completion merely mentions in prose is not a report anybody ruled on.
//!
//! The report it looks at is
//! [`super::CodingSessionTeamAssignmentSettlement::governed_report_event_id`]:
//! the report the approving disposition actually governs. That is the only
//! report the fold can point at and say "this is what the approval was about".

use std::collections::HashSet;

use super::*;
use crate::coding_session_team_transaction::{
    CodingSessionTeamMissionCompleted, CodingSessionTeamRefutationDecision,
};

/// Why this completion cannot be canonical while the policy requires a
/// verifier, or `None` when every settled assignment it names carries one.
///
/// Returns the first failing assignment in the completion's own
/// `assignmentRefs` order, so two readers of the same event set name the same
/// assignment.
///
/// An assignment the fold did **not** settle is skipped here on purpose: an
/// unsettled assignment is already
/// [`super::CodingSessionTeamFoldExclusionCode::CompletionNotApproved`]'s
/// business, and naming it twice would say a verifier is missing when what is
/// missing is the approval itself. An assignment id this set does not hold at
/// all is likewise skipped — the completion is excluded
/// `DanglingReference` before this ever runs.
pub(super) fn completion_not_verified(
    body: &CodingSessionTeamMissionCompleted,
    assignments: &[CodingSessionTeamAssignmentSettlement],
    records: &[Record<'_>],
    active: &HashSet<usize>,
    context: &CodingSessionTeamFoldContext,
) -> Option<String> {
    for reference in &body.assignment_refs {
        let Some(settlement) = assignments
            .iter()
            .find(|state| &state.assignment_event_id == reference)
        else {
            continue;
        };
        if !settlement.settled {
            continue;
        }
        let Some(report_id) = settlement.governed_report_event_id.as_deref() else {
            continue;
        };
        if !completion_verifier_rulings_are_present(report_id, records, active, context) {
            return Some(format!(
                "mission.completed requires a verifier's ruling while the policy sets \
                 gates.verifierRequired: assignment {reference} settled on report {report_id}, \
                 and no active verifier seat has ruled on that report"
            ));
        }
    }
    None
}

/// Whether an active `verifier` seat has signed about exactly this report.
///
/// Two shapes count, and both are a verifier signing **about that report**:
///
/// (a) a canonical `refutation` verdict whose `reportRef` is this report,
///     whose `decision` is `not-refuted`, and whose author holds an active
///     `verifier` seat. `confirmed` and `blocked` are rulings too, and they
///     are rulings *against* the completion: a verifier who found the failure,
///     or who could not conclude, has not cleared the report.
///
/// (b) that report's **own author** holding an active `verifier` seat. Live
///     run 3's shape: the lead assigned the verification to Ira, Ira's report
///     *was* the verification, and no separate `refutation` was ever
///     published. Refusing (b) would have blocked run 3's completion with a
///     verb nobody was using. **Ruling, Brian's to tighten** (LANE-L7.md
///     §L7.1, addendum ruling 1): if (b) is later withdrawn, this function is
///     the single place it lives.
///
/// Neither shape is prose. A summary that says "verified" and a note that says
/// "Ira is happy" are not rulings — finding 26 is what happens when prose is
/// read as evidence.
///
/// `active` is the fold's own included set at the moment the terminal is
/// judged: a refutation that lost a correction conflict, or one whose author
/// was never authorized, has already left it and cannot clear anything.
pub(super) fn completion_verifier_rulings_are_present(
    report_id: &str,
    records: &[Record<'_>],
    active: &HashSet<usize>,
    context: &CodingSessionTeamFoldContext,
) -> bool {
    active.iter().any(|&index| {
        let record = &records[index];
        match &record.payload.body {
            // (a) a verifier's canonical `not-refuted` refutation of this
            // exact report.
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
                report_ref,
                decision,
                ..
            }) => {
                report_ref == report_id
                    && *decision == CodingSessionTeamRefutationDecision::NotRefuted
                    && context.is_active_role(&record.author, "verifier")
            }
            // (b) the report itself, signed by an active verifier seat.
            CodingSessionTeamTransactionBody::Report(_) => {
                record.id == report_id && context.is_active_role(&record.author, "verifier")
            }
            _ => false,
        }
    })
}

#[cfg(test)]
#[path = "coding_session_completion_verification_tests.rs"]
mod tests;
