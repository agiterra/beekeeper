//! Assignment settlement, the missing link that explains it, and the pending
//! completion that is waiting for that link to arrive.
//!
//! A child of [`super`] for the same reason its siblings are: it reads the
//! parent's private `Record`, and no file in this crate may pass 1,000 lines.
//!
//! # The finding this file exists for
//!
//! Ledger 178(e). With a canonical approving disposition and a canonical
//! report on the wire, `settled` was `false` and every disposition field was
//! `null`, because the *acknowledgement* was missing — and nothing in the
//! projection said so. A lead read those nulls, concluded that a refutation
//! was missing, and recalled the verifier for a second turn it did not owe.
//! Three nulls are not a diagnosis: [`CodingSessionTeamSettlementAwaiting`]
//! names the one link that is absent and the party who owes it.
//!
//! Ledger 179(a) is the other half. A completion whose prerequisites are not
//! yet on the wire is not wrong, it is *early*: the lead has said the mission
//! is finished and the transport has not caught up. The fold has always
//! re-derived settlement from the supplied set, so such a completion becomes
//! this session's terminal the moment the last acknowledgement arrives, with
//! no further turn from anybody. What was missing was a name for that state
//! and a surface that shows it, so nobody has to wake six seats to watch it
//! happen: [`CodingSessionTeamPendingCompletion`].
//!
//! # The settlement rule changed here (lane 210, ledger 210)
//!
//! An approving disposition that asks the assignee for nothing settles the
//! assignment **without** an acknowledgement. Measured cost of the old rule:
//! Andy's eight-seat run spent 14 of 32 turns on disposition/acknowledgement
//! handling and $9.33 on six "housekeeping, no new work" wakes (ledger
//! 179(a)); a kettle-sized session spent 11 turns and 8.2M cache-inclusive
//! input closing the protocol against 5 turns and 8.5M doing the work, one of
//! them an acknowledgement and nothing else. Every wake re-reads the seat's
//! whole context, so a receipt that says "received" costs what a turn that
//! writes code costs, and carries no information the disposition did not
//! already carry. The provider cannot sign the receipt for the seat: the
//! seat's key is a one-shot host-local file deleted at spawn (ledger 183(f)).
//! `VISION_COLLABORATION.md` § "Gates must earn their delay" — this gate
//! could name no failure it prevented.
//!
//! **Scope.** This changes the fold's settlement rule and the projection. It
//! adds no key to the closed 44244 envelope or to any body, and it does not
//! touch `docs/design/portable-team-loop/PLAN.md:123`'s separation of
//! provider delivery from semantic acknowledgement: no kind-44220 receipt
//! became an acknowledgement, and an assignee's explicit acknowledgement is
//! still the only thing that can *be* one. What changed is that the fold no
//! longer **requires** one to settle an approval that asks nothing.
//! [`CodingSessionTeamSettledBy`] discloses which rule settled each
//! assignment, so no reader has to guess.

use std::collections::HashSet;

use super::{
    CodingSessionTeamFoldConflict, CodingSessionTeamFoldExclusionCode,
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionType, CodingSessionTeamVerdict,
    Record,
};
use crate::coding_session_team_transaction::CodingSessionTeamDispositionDecision;

/// Whether an approving disposition asks the assignee for nothing.
///
/// **The exact definition, over the disposition body's own fields**
/// (`crates/beekeeper-core/src/coding_session_team_transaction.rs:341-357`, the
/// `Disposition` variant of [`CodingSessionTeamVerdict`]):
///
/// | field | read as |
/// | --- | --- |
/// | `decision` | an ask unless it is `approve` or `approve-with-notes` — [`CodingSessionTeamDispositionDecision::is_approval`]. `changes-requested`, `reject` and `blocked` all require the assignee to do something next, by definition. |
/// | `requiredAction` | *"Required next action, or null when none remains."* Present and non-blank **is** the ask, whatever the prose says. A ruling whose `requiredAction` reads "nothing is required of the builder" still owes an answer: this fold reads fields, never prose (finding 26). |
/// | `refutationRef` | a pointer to a prior verdict, not a request. |
/// | `assignmentRef`, `reportRef` | what the ruling is about. |
/// | `summary`, `findings` | the ruling's own reasoning and evidence, addressed to every reader of the mission, not a request of the assignee. Nothing in the vocabulary distinguishes a finding from a note, and on the one measured eight-seat run **every** approving disposition carried four supporting findings with `requiredAction: null` — reading findings as an ask would settle nothing at all and leave the measurement unchanged. Where a lead does want something, the field for it exists and is `requiredAction`. |
///
/// Whitespace-only `requiredAction` is no ask: it is the absent value spelled
/// badly, and a surface would render it as nothing.
///
/// Conservative in the direction that matters: presence of the ask field, not
/// its meaning, decides. A lead who wants an explicit answer writes one word
/// in `requiredAction` and gets the old behaviour exactly.
#[must_use]
pub fn approving_disposition_asks_nothing(
    decision: CodingSessionTeamDispositionDecision,
    required_action: Option<&str>,
) -> bool {
    decision.is_approval()
        && required_action
            .map(|action| action.trim().is_empty())
            .unwrap_or(true)
}

/// Which rule settled an assignment.
///
/// `None` on [`CodingSessionTeamAssignmentSettlement::settled_by`] exactly
/// when the assignment is not settled — the same invariant `awaiting` holds
/// in reverse — so a reader never has to tell "not settled" from "settled for
/// a reason this build does not disclose".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingSessionTeamSettledBy {
    /// The assignee's explicit acknowledgement of the approving disposition.
    ///
    /// Still accepted, still recorded, and still what settles an approval
    /// that carries an ask. When an acknowledgement is on the wire it wins
    /// this label even if the disposition asked nothing: the receipt is a
    /// signed fact about a named party, and reporting the weaker rule when
    /// the stronger one is satisfied would hide it.
    Acknowledgement,
    /// An approving disposition that asks the assignee for nothing
    /// ([`approving_disposition_asks_nothing`]), with no acknowledgement on
    /// the wire. Lane 210.
    ApprovingDispositionWithoutAsk,
}

impl CodingSessionTeamSettledBy {
    /// The stable lowercase wire word for this rule.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Acknowledgement => "acknowledgement",
            Self::ApprovingDispositionWithoutAsk => "approving_disposition_without_ask",
        }
    }
}

/// Which link of an assignment's approval chain is not on the wire.
///
/// Exactly one is reported — the *first* one missing, walking the chain in the
/// order the protocol requires it — because a reader asking "what is this
/// assignment waiting for?" needs one next action, not a checklist of
/// consequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingSessionTeamSettlementLink {
    /// No canonical report answers this assignment yet.
    Report,
    /// A canonical report exists and no canonical **approving** disposition
    /// governs it.
    ///
    /// A `changes-requested`, `reject` or `blocked` disposition leaves the
    /// assignment here: the chain needs an approval, and a ruling that is not
    /// one has not supplied it. The distinction is visible in the records
    /// themselves; this field says only what settlement is still waiting for.
    Disposition,
    /// An approving disposition governs a canonical report, that disposition
    /// **asks the assignee for something** (`requiredAction`), and the
    /// assigned actor has not answered it.
    ///
    /// Lane 210: this link is never reported for an approving disposition
    /// that asks nothing — such an assignment is settled, and saying it awaits
    /// a receipt would be the false statement that cost six wakes in ledger
    /// 179(a).
    Acknowledgement,
}

impl CodingSessionTeamSettlementLink {
    /// The lowercase wire word for this link.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::Disposition => "disposition",
            Self::Acknowledgement => "acknowledgement",
        }
    }
}

/// The one missing link that keeps an assignment unsettled, and who owes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamSettlementAwaiting {
    /// The absent link.
    pub link: CodingSessionTeamSettlementLink,
    /// Role slug of the party that owes it: the assignment's `assigneeRole`
    /// for a report or an acknowledgement, and `lead` for a disposition.
    pub owed_by_role: String,
    /// Lowercase-hex pubkey of the party that owes it, when exactly one party
    /// can supply it.
    ///
    /// `None` for a disposition, and deliberately: the founder, any active
    /// `lead` seat and any actor holding a steer grant may all rule, so naming
    /// one of them would be a guess. Absent means "more than one party may
    /// supply this", never "unknown".
    pub owed_by_actor: Option<String>,
}

/// Canonical approval state for one active assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamAssignmentSettlement {
    /// Active assignment event id.
    pub assignment_event_id: String,
    /// Explicitly governed report, when an approval chain is complete.
    pub governed_report_event_id: Option<String>,
    /// The approving disposition that settles this assignment, when one does.
    pub disposition_event_id: Option<String>,
    /// Assigned actor's acknowledgement of that disposition, when one is on
    /// the wire. `None` on a settled assignment means it settled under
    /// [`CodingSessionTeamSettledBy::ApprovingDispositionWithoutAsk`].
    pub acknowledgement_event_id: Option<String>,
    /// True for disposition `approve` or `approve-with-notes` over a
    /// canonical report, plus **either** the assigned actor's explicit
    /// acknowledgement **or** a disposition that asks the assignee for
    /// nothing (lane 210). [`Self::settled_by`] says which.
    pub settled: bool,
    /// Which rule settled it, and `None` exactly when [`Self::settled`] is
    /// false.
    ///
    /// Lane 210. A reader that shows an assignment as settled must be able to
    /// say why without inferring it from a null acknowledgement id — the
    /// inference a lead drew from three nulls in ledger 178(e) is the whole
    /// reason this file exists.
    pub settled_by: Option<CodingSessionTeamSettledBy>,
    /// The one missing link while [`Self::settled`] is false, and `None`
    /// exactly when it is true.
    ///
    /// Ledger 178(e): the three nulls above say *that* the chain is
    /// incomplete and never *where*. This says where, so a surface can print
    /// "awaiting acknowledgement by builder" instead of leaving a reader to
    /// infer a cause — and the inference the live run actually drew was wrong.
    pub awaiting: Option<CodingSessionTeamSettlementAwaiting>,
}

/// A published `mission.completed` that is not this session's terminal yet
/// because at least one of its prerequisites is not on the wire.
///
/// Not a refusal. The completion is a durable, signed statement that the lead
/// considers the mission finished; the fold re-derives its prerequisites from
/// the supplied set on every read, so the record becomes terminal as soon as
/// the last one arrives — no further lead turn, no re-publication, nothing to
/// remember (ledger 179(a)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamPendingCompletion {
    /// Event id of the published completion.
    pub event_id: String,
    /// Why it is not terminal yet: one of `CompletionNotApproved`,
    /// `CompletionBlockedByOpenDecision` or `CompletionNotVerified`. Every
    /// other exclusion class is a defect of the record rather than a wait, and
    /// never appears here.
    pub code: CodingSessionTeamFoldExclusionCode,
    /// The fold's own sentence for that code, verbatim.
    pub reason: String,
    /// Assignments this completion names that are not settled, in the fold's
    /// assignment order. Each one's missing link is in
    /// [`CodingSessionTeamAssignmentSettlement::awaiting`].
    pub unsettled_assignment_event_ids: Vec<String>,
}

/// The three exclusion classes that mean "not yet", rather than "not this
/// record".
///
/// A completion excluded for any of these becomes terminal unchanged once the
/// missing fact is published. Every other class — an unauthorized signer, a
/// dangling pointer, an invalid correction, a newer terminal — stays true
/// however long anybody waits, and a writer must be refused rather than told
/// to wait.
pub const PENDING_COMPLETION_CODES: &[CodingSessionTeamFoldExclusionCode] = &[
    CodingSessionTeamFoldExclusionCode::CompletionNotApproved,
    CodingSessionTeamFoldExclusionCode::CompletionBlockedByOpenDecision,
    CodingSessionTeamFoldExclusionCode::CompletionNotVerified,
];

/// Whether a completion excluded with `code` is waiting rather than rejected.
pub fn completion_exclusion_is_pending(code: CodingSessionTeamFoldExclusionCode) -> bool {
    PENDING_COMPLETION_CODES.contains(&code)
}

/// One completion the terminal pass held back, in supplied order.
pub(super) struct HeldCompletion {
    pub(super) index: usize,
    pub(super) code: CodingSessionTeamFoldExclusionCode,
    pub(super) reason: String,
}

/// Settle every active assignment and name the link each unsettled one waits
/// on.
pub(super) fn settle_assignments(
    records: &[Record<'_>],
    active: &[usize],
    conflicts: &mut Vec<CodingSessionTeamFoldConflict>,
) -> Vec<CodingSessionTeamAssignmentSettlement> {
    let active_set: HashSet<usize> = active.iter().copied().collect();
    let mut result = Vec::new();
    for &assignment_index in active {
        let CodingSessionTeamTransactionBody::Assignment(_) =
            records[assignment_index].payload.body
        else {
            continue;
        };
        let assignment_id = &records[assignment_index].id;
        let mut chains = Vec::new();
        for &disposition_index in active {
            let CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
                assignment_ref,
                report_ref,
                decision,
                required_action,
                ..
            }) = &records[disposition_index].payload.body
            else {
                continue;
            };
            if assignment_ref != assignment_id || !decision.is_approval() {
                continue;
            }
            let Some(report_index) = records.iter().position(|record| &record.id == report_ref)
            else {
                continue;
            };
            if !active_set.contains(&report_index) {
                continue;
            }
            let mut acknowledgements: Vec<usize> = active
                .iter()
                .copied()
                .filter(|index| {
                    matches!(
                        &records[*index].payload.body,
                        CodingSessionTeamTransactionBody::Acknowledgement(body)
                            if body.acknowledged_event_ref == records[disposition_index].id
                    )
                })
                .collect();
            super::sort_indices(&mut acknowledgements, records);
            if let Some(acknowledgement) = acknowledgements.last() {
                if acknowledgements.len() > 1 {
                    conflicts.push(CodingSessionTeamFoldConflict {
                        subject: format!("acknowledgement:{}", records[disposition_index].id),
                        winner_event_id: records[*acknowledgement].id.clone(),
                        contender_event_ids: acknowledgements
                            .iter()
                            .map(|index| records[*index].id.clone())
                            .collect(),
                    });
                }
                chains.push((
                    disposition_index,
                    report_index,
                    Some(*acknowledgement),
                    CodingSessionTeamSettledBy::Acknowledgement,
                ));
            } else if approving_disposition_asks_nothing(*decision, required_action.as_deref()) {
                // Lane 210. No receipt is owed, so the chain is complete
                // here. A receipt that arrives later still folds — it is an
                // acknowledgement of this same disposition, so the branch
                // above claims this chain on the next read and `settledBy`
                // becomes `acknowledgement`. Order independence is preserved
                // because neither branch depends on arrival order: both read
                // the same supplied set.
                chains.push((
                    disposition_index,
                    report_index,
                    None,
                    CodingSessionTeamSettledBy::ApprovingDispositionWithoutAsk,
                ));
            }
        }
        chains.sort_by(|left, right| super::compare_records(left.0, right.0, records));
        // **Selection order, pinned (lane 210).** An acknowledged chain wins
        // over every unacknowledged one, whatever their order on the wire;
        // among chains of the same kind, the newest disposition wins as it
        // always has.
        //
        // Without this, a *newer* approving disposition that asks nothing —
        // published over a corrected report, say — would outrank an older
        // chain the assignee actually acknowledged, and an assignment that
        // was already settled would silently change which report governs it.
        // "Nothing already settled changes" is a statement about the evidence
        // chain, not only about the boolean: an assignment settled by an
        // acknowledged chain keeps that chain, and lane 210's rule only
        // settles assignments that have no acknowledged chain at all.
        let winner = chains
            .iter()
            .rev()
            .find(|chain| chain.2.is_some())
            .or_else(|| chains.last())
            .copied();
        if chains.len() > 1 {
            if let Some((winner_index, _, _, _)) = winner {
                conflicts.push(CodingSessionTeamFoldConflict {
                    subject: format!("governance:{assignment_id}"),
                    winner_event_id: records[winner_index].id.clone(),
                    contender_event_ids: chains
                        .iter()
                        .map(|chain| records[chain.0].id.clone())
                        .collect(),
                });
            }
        }
        result.push(CodingSessionTeamAssignmentSettlement {
            assignment_event_id: assignment_id.clone(),
            governed_report_event_id: winner.map(|chain| records[chain.1].id.clone()),
            disposition_event_id: winner.map(|chain| records[chain.0].id.clone()),
            acknowledgement_event_id: winner
                .and_then(|chain| chain.2)
                .map(|index| records[index].id.clone()),
            settled: winner.is_some(),
            settled_by: winner.map(|chain| chain.3),
            awaiting: match winner {
                Some(_) => None,
                None => awaiting_link(records, active, assignment_index),
            },
        });
    }
    result
}

/// Walk one assignment's approval chain and name the first missing link.
///
/// Reads only `active`, exactly as [`settle_assignments`] does: a report or a
/// disposition the projection excluded is not on the chain, so an assignment
/// whose only report was excluded honestly reports that it is still awaiting a
/// report.
fn awaiting_link(
    records: &[Record<'_>],
    active: &[usize],
    assignment_index: usize,
) -> Option<CodingSessionTeamSettlementAwaiting> {
    let CodingSessionTeamTransactionBody::Assignment(assignment) =
        &records[assignment_index].payload.body
    else {
        return None;
    };
    let assignment_id = &records[assignment_index].id;
    let assignee = || CodingSessionTeamSettlementAwaiting {
        link: CodingSessionTeamSettlementLink::Report,
        owed_by_role: assignment.assignee_role.clone(),
        owed_by_actor: Some(assignment.assignee_actor.clone()),
    };
    let reports: Vec<&str> = active
        .iter()
        .filter_map(|index| match &records[*index].payload.body {
            CodingSessionTeamTransactionBody::Report(body)
                if &body.assignment_ref == assignment_id =>
            {
                Some(records[*index].id.as_str())
            }
            _ => None,
        })
        .collect();
    if reports.is_empty() {
        return Some(assignee());
    }
    let approved = active.iter().any(|index| {
        matches!(
            &records[*index].payload.body,
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
                assignment_ref,
                report_ref,
                decision,
                ..
            }) if assignment_ref == assignment_id
                && decision.is_approval()
                && reports.contains(&report_ref.as_str())
        )
    });
    if !approved {
        return Some(CodingSessionTeamSettlementAwaiting {
            link: CodingSessionTeamSettlementLink::Disposition,
            owed_by_role: "lead".into(),
            owed_by_actor: None,
        });
    }
    Some(CodingSessionTeamSettlementAwaiting {
        link: CodingSessionTeamSettlementLink::Acknowledgement,
        ..assignee()
    })
}

/// Disclose the newest held-back completion that is waiting rather than wrong.
///
/// `None` when nothing was held back, when everything held back was held for a
/// reason that will never clear on its own, or when a completion **did** fold:
/// a finished mission is not also a pending one.
pub(super) fn pending_completion(
    records: &[Record<'_>],
    held: &[HeldCompletion],
    canonical_terminal_type: Option<CodingSessionTeamTransactionType>,
    assignments: &[CodingSessionTeamAssignmentSettlement],
) -> Option<CodingSessionTeamPendingCompletion> {
    if canonical_terminal_type == Some(CodingSessionTeamTransactionType::MissionCompleted) {
        return None;
    }
    let newest = held
        .iter()
        .filter(|candidate| completion_exclusion_is_pending(candidate.code))
        .max_by(|left, right| super::compare_records(left.index, right.index, records))?;
    let CodingSessionTeamTransactionBody::MissionCompleted(body) =
        &records[newest.index].payload.body
    else {
        return None;
    };
    let unsettled_assignment_event_ids = assignments
        .iter()
        .filter(|state| !state.settled && body.assignment_refs.contains(&state.assignment_event_id))
        .map(|state| state.assignment_event_id.clone())
        .collect();
    Some(CodingSessionTeamPendingCompletion {
        event_id: records[newest.index].id.clone(),
        code: newest.code,
        reason: newest.reason.clone(),
        unsettled_assignment_event_ids,
    })
}
