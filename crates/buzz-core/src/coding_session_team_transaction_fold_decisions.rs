//! Projections for the two state-free verbs, `note` and `decision.*`.
//!
//! A child of `coding_session_team_transaction_fold`, split out only to keep
//! every file under 1,000 lines (REVIEW-B1b R6, the structure B1b round 4
//! established). No behaviour change: `use super::*` gives these functions the
//! parent's private `Record` and helpers exactly as before.

use std::collections::HashSet;

use super::*;
use crate::coding_session_team_transaction::CodingSessionTeamMissionCompleted;

impl CodingSessionTeamFoldContext {
    /// Whether this author holds any active seat at all, in any role.
    ///
    /// The two state-free verbs (`note`, `decision.request`) ask only for
    /// participation, not for standing over anyone else's work.
    fn is_seated(&self, author: &str) -> bool {
        self.active_seats
            .iter()
            .any(|seat| seat.actor_pubkey == author)
    }

    pub(super) fn may_speak(&self, author: &str) -> bool {
        author == self.founder_pubkey || self.is_seated(author)
    }
}

/// Why this completion cannot be canonical while a ruling it asked for stands
/// open, or `None` when nothing it names is blocked.
///
/// A mission cannot be complete and waiting on a person at the same time
/// (REVIEW-B1c F1). Excluding the completion is the honest choice over
/// suppressing the waiting state: the request is a signed fact nobody
/// answered, while the completion is a claim the fold can test and find
/// unproven — exactly the shape `CompletionNotApproved` already handles.
pub(super) fn completion_blocked_by_open_decision(
    body: &CodingSessionTeamMissionCompleted,
    open_blocks: &HashSet<&str>,
) -> Option<String> {
    let blocked = body
        .assignment_refs
        .iter()
        .find(|reference| open_blocks.contains(reference.as_str()))?;
    Some(format!(
        "assignment {blocked} is blocked by an unanswered decision.request"
    ))
}

/// One canonical `note`: something a participant said without changing state.
///
/// A note is listed, never folded into a phase. Its `refs` are reproduced as
/// the author wrote them and are **not** required to resolve inside the
/// supplied set — a pointer that does not resolve is a reader's dead end, not
/// grounds to drop the sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldNote {
    /// Event id of the canonical note.
    pub event_id: String,
    /// Canonical lowercase-hex pubkey that signed the note.
    pub author_pubkey: String,
    /// Event ids the note points at, in the author's own order.
    pub refs: Vec<String>,
}

/// One canonical `decision.request` and its answer, when one exists.
///
/// `answered_by` and `answer_event_id` are both `None` exactly while the
/// question stands open. They are never inferred: an answer counts only when it
/// is itself canonical and its signer held the standing the request named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldDecision {
    /// Event id of the canonical request.
    pub request_event_id: String,
    /// Exactly `founder`, or the lowercase 64-hex actor holding the decision.
    pub held_on: String,
    /// Assignment event ids the request declares itself blocking.
    pub blocks: Vec<String>,
    /// Canonical lowercase-hex pubkey that answered, when answered.
    pub answered_by: Option<String>,
    /// Event id of the canonical answer, when answered.
    pub answer_event_id: Option<String>,
}

/// The one open decision that is actually holding the mission up.
///
/// Present exactly when a canonical unanswered `decision.request` names at
/// least one **active** assignment in `blocks`. This is the mission's
/// waiting-on-a-person state, and it deliberately carries no terminal: nothing
/// is settled, work simply cannot finish until the named party rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldWaitingOnDecision {
    /// Event id of the open request a reader can go and answer.
    pub request_event_id: String,
    /// Exactly `founder`, or the lowercase 64-hex actor being waited on.
    pub held_on: String,
}

/// List every canonical note in the projection's own included order.
pub(super) fn list_notes(
    records: &[Record<'_>],
    active: &[usize],
) -> Vec<CodingSessionTeamFoldNote> {
    active
        .iter()
        .filter_map(|index| {
            let CodingSessionTeamTransactionBody::Note(body) = &records[*index].payload.body else {
                return None;
            };
            Some(CodingSessionTeamFoldNote {
                event_id: records[*index].id.clone(),
                author_pubkey: records[*index].author.clone(),
                refs: body.refs.clone(),
            })
        })
        .collect()
}

/// Pair every canonical decision request with its canonical answer, if any.
///
/// Two canonical answers to one request are both real signed facts, so the
/// newest by `(created_at, event id)` wins and the competition is disclosed as
/// a conflict rather than quietly dropped.
pub(super) fn project_decisions(
    records: &[Record<'_>],
    active: &[usize],
    conflicts: &mut Vec<CodingSessionTeamFoldConflict>,
) -> Vec<CodingSessionTeamFoldDecision> {
    let mut decisions = Vec::new();
    for &index in active {
        let CodingSessionTeamTransactionBody::DecisionRequest(body) = &records[index].payload.body
        else {
            continue;
        };
        let request_id = &records[index].id;
        let mut answers: Vec<usize> = active
            .iter()
            .copied()
            .filter(|candidate| {
                matches!(
                    &records[*candidate].payload.body,
                    CodingSessionTeamTransactionBody::DecisionAnswer(answer)
                        if &answer.request_ref == request_id
                )
            })
            .collect();
        sort_indices(&mut answers, records);
        if answers.len() > 1 {
            if let Some(&winner) = answers.last() {
                conflicts.push(CodingSessionTeamFoldConflict {
                    subject: format!("decision:{request_id}"),
                    winner_event_id: records[winner].id.clone(),
                    contender_event_ids: answers
                        .iter()
                        .map(|answer| records[*answer].id.clone())
                        .collect(),
                });
            }
        }
        let answer = answers.last().copied();
        decisions.push(CodingSessionTeamFoldDecision {
            request_event_id: request_id.clone(),
            held_on: body.held_on.clone(),
            blocks: body.blocks.clone(),
            answered_by: answer.map(|index| records[index].author.clone()),
            answer_event_id: answer.map(|index| records[index].id.clone()),
        });
    }
    decisions
}

/// Every assignment id named by a canonical request that nobody has answered.
///
/// Read from `active` only, so a request excluded for any other reason never
/// holds a completion up. `blocks` ids are reproduced as the author wrote them
/// and are resolved against the *current* active set by the caller.
pub(super) fn open_request_blocks<'a>(
    records: &'a [Record<'_>],
    active: &[usize],
) -> HashSet<&'a str> {
    let answered: HashSet<&str> = active
        .iter()
        .filter_map(|index| match &records[*index].payload.body {
            CodingSessionTeamTransactionBody::DecisionAnswer(body) => {
                Some(body.request_ref.as_str())
            }
            _ => None,
        })
        .collect();
    active
        .iter()
        .filter_map(|index| match &records[*index].payload.body {
            CodingSessionTeamTransactionBody::DecisionRequest(body)
                if !answered.contains(records[*index].id.as_str()) =>
            {
                Some(body.blocks.iter().map(String::as_str))
            }
            _ => None,
        })
        .flatten()
        .collect()
}

/// Return the oldest unanswered canonical request that blocks active work.
///
/// A request that names no assignment, or whose named assignments are not
/// active, is a real open question but is not holding this mission up, so it
/// never produces a waiting state.
pub(super) fn waiting_on_decision(
    records: &[Record<'_>],
    active: &[usize],
    decisions: &[CodingSessionTeamFoldDecision],
) -> Option<CodingSessionTeamFoldWaitingOnDecision> {
    let active_assignments: HashSet<&str> = active
        .iter()
        .filter(|index| {
            matches!(
                records[**index].payload.body,
                CodingSessionTeamTransactionBody::Assignment(_)
            )
        })
        .map(|index| records[*index].id.as_str())
        .collect();
    decisions
        .iter()
        .find(|decision| {
            decision.answer_event_id.is_none()
                && decision
                    .blocks
                    .iter()
                    .any(|reference| active_assignments.contains(reference.as_str()))
        })
        .map(|decision| CodingSessionTeamFoldWaitingOnDecision {
            request_event_id: decision.request_event_id.clone(),
            held_on: decision.held_on.clone(),
        })
}
