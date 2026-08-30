//! Deterministic, storage-independent fold for NIP-CSTX event sets.
//!
//! The caller supplies a verified session/authority projection. This module
//! performs no I/O and never infers authority or terminal state from silence.

use std::collections::{HashMap, HashSet};

use nostr::Event;

use super::{
    validate_coding_session_team_transaction_envelope,
    validate_coding_session_team_transaction_supersession, validate_event_id, validate_role,
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CodingSessionTeamTransactionType, CodingSessionTeamVerdict,
};

/// One currently active role seat supplied by the authority/session reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamActiveSeat {
    /// Canonical lowercase-hex actor pubkey.
    pub actor_pubkey: String,
    /// Canonical `[a-z0-9-]+` role slug.
    pub role: String,
}

/// One active, signed authority grant supplied by the authority-chain reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamActiveGrant {
    /// Canonical lowercase-hex grantee pubkey.
    pub actor_pubkey: String,
    /// Event id of the active signed grant from which this projection came.
    pub grant_event_ref: String,
    /// Whether the active grant gives the operator steering standing.
    pub may_steer: bool,
}

/// Complete context required to authorize and fold one session's transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldContext {
    /// Canonical channel UUID expected in every event's `h` tag.
    pub channel_ref: String,
    /// Canonical umbrella UUID expected in every event and `d` tag.
    pub session_ref: String,
    /// Immutable genesis event id expected in every event.
    pub genesis_ref: String,
    /// Pubkey of the genesis signer.
    pub founder_pubkey: String,
    /// Current signed seat projection; only exact `lead` and `verifier` roles
    /// affect the v1 authority matrix.
    pub active_seats: Vec<CodingSessionTeamActiveSeat>,
    /// Current signed grant projection. Only grants with `may_steer=true`
    /// qualify an operator for founder/lead operations.
    pub active_grants: Vec<CodingSessionTeamActiveGrant>,
}

impl CodingSessionTeamFoldContext {
    fn validate(&self) -> Result<(), String> {
        super::validate_canonical_uuid("context.channelRef", &self.channel_ref)?;
        super::validate_canonical_uuid("context.sessionRef", &self.session_ref)?;
        validate_event_id("context.genesisRef", &self.genesis_ref)?;
        validate_event_id("context.founderPubkey", &self.founder_pubkey)?;
        for seat in &self.active_seats {
            validate_event_id("context.activeSeats.actorPubkey", &seat.actor_pubkey)?;
            validate_role(&seat.role)?;
        }
        for grant in &self.active_grants {
            validate_event_id("context.activeGrants.actorPubkey", &grant.actor_pubkey)?;
            validate_event_id("context.activeGrants.grantEventRef", &grant.grant_event_ref)?;
        }
        Ok(())
    }

    fn is_active_role(&self, author: &str, role: &str) -> bool {
        self.active_seats
            .iter()
            .any(|seat| seat.actor_pubkey == author && seat.role == role)
    }

    fn may_lead(&self, author: &str) -> bool {
        author == self.founder_pubkey
            || self.is_active_role(author, "lead")
            || self
                .active_grants
                .iter()
                .any(|grant| grant.actor_pubkey == author && grant.may_steer)
    }
}

/// Why a structurally valid event is absent from the canonical projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingSessionTeamFoldExclusionCode {
    /// The signer lacks the operation's required standing.
    Unauthorized,
    /// A required canonical parent was individually unauthorized.
    DependentOnUnauthorized,
    /// A required parent lost correction projection or was superseded.
    DependentOnSuperseded,
    /// A required parent was excluded for another stable reason.
    DependentOnExcluded,
    /// A valid correction replaces this event.
    Superseded,
    /// Another head of the same correction fork won deterministic ordering.
    CorrectionConflict,
    /// A completion did not prove every referenced assignment's approval chain.
    CompletionNotApproved,
    /// Another authorized terminal event is newer.
    TerminalConflict,
}

/// One excluded event and the stable reason for exclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldExclusion {
    /// Excluded event id.
    pub event_id: String,
    /// Machine-readable exclusion class.
    pub code: CodingSessionTeamFoldExclusionCode,
    /// Bounded diagnostic suitable for logs and conformance tests.
    pub reason: String,
}

/// A deterministic choice among competing semantic heads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldConflict {
    /// Stable logical subject of the conflict.
    pub subject: String,
    /// Winner selected by `(created_at, event id)`.
    pub winner_event_id: String,
    /// All competing heads, ordered by `(created_at, event id)`.
    pub contender_event_ids: Vec<String>,
}

/// Canonical approval state for one active assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamAssignmentSettlement {
    /// Active assignment event id.
    pub assignment_event_id: String,
    /// Explicitly governed report, when an approval chain is complete.
    pub governed_report_event_id: Option<String>,
    /// Approving disposition acknowledged by the assigned actor.
    pub disposition_event_id: Option<String>,
    /// Assigned actor's acknowledgement of that disposition.
    pub acknowledgement_event_id: Option<String>,
    /// True only for disposition `approve` or `approve-with-notes` plus the
    /// assigned actor's explicit acknowledgement.
    pub settled: bool,
}

/// Canonical newest authorized terminal transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamCanonicalTerminal {
    /// Terminal event id.
    pub event_id: String,
    /// Exactly `mission.completed` or `mission.blocked`.
    pub transaction_type: CodingSessionTeamTransactionType,
}

/// Deterministic projection of a supplied NIP-CSTX event set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFold {
    /// Canonical active facts after authority and correction projection.
    pub included_event_ids: Vec<String>,
    /// Rejected or displaced events with explicit reasons.
    pub excluded: Vec<CodingSessionTeamFoldExclusion>,
    /// Correction, governance, and terminal conflicts with deterministic winners.
    pub conflicts: Vec<CodingSessionTeamFoldConflict>,
    /// Approval state for every active assignment.
    pub assignments: Vec<CodingSessionTeamAssignmentSettlement>,
    /// Newest authorized valid terminal event, never inferred from silence.
    pub canonical_terminal: Option<CodingSessionTeamCanonicalTerminal>,
}

struct Record<'a> {
    event: &'a Event,
    id: String,
    author: String,
    payload: CodingSessionTeamTransactionPayload,
}

#[derive(Clone, Copy)]
enum ProjectionStage {
    Assignment,
    Report,
    Refutation,
    Disposition,
    Acknowledgement,
    MissionCompleted,
    MissionBlocked,
}

/// Validate and fold signed transactions against a supplied authority context.
///
/// Structural graph failures are hard errors: dangling, cross-context, wrong
/// type, invalid correction, and cycle inputs have no safe partial meaning.
/// Valid but unauthorized or deterministically displaced events are disclosed
/// in [`CodingSessionTeamFold::excluded`].
pub fn fold_coding_session_team_transactions(
    events: &[Event],
    context: &CodingSessionTeamFoldContext,
) -> Result<CodingSessionTeamFold, String> {
    context.validate()?;
    let mut records = Vec::with_capacity(events.len());
    let mut by_id = HashMap::with_capacity(events.len());
    for event in events {
        crate::verify_event(event)
            .map_err(|error| format!("invalid team-transaction signature: {error}"))?;
        let payload = validate_coding_session_team_transaction_envelope(event)?;
        let id = event.id.to_hex();
        if by_id.insert(id.clone(), records.len()).is_some() {
            return Err(format!("duplicate supplied team transaction {id}"));
        }
        if payload.session_ref != context.session_ref {
            return Err(format!(
                "team transaction {id} crosses the supplied session"
            ));
        }
        if payload.genesis_ref != context.genesis_ref {
            return Err(format!(
                "team transaction {id} crosses the supplied genesis"
            ));
        }
        let channel = event.tags.as_slice()[0].as_slice()[1].as_str();
        if channel != context.channel_ref {
            return Err(format!(
                "team transaction {id} crosses the supplied channel"
            ));
        }
        records.push(Record {
            event,
            id,
            author: event.pubkey.to_hex(),
            payload,
        });
    }

    validate_references(&records, &by_id)?;
    reject_cycles(&records, &by_id)?;

    let mut excluded = Vec::new();
    let mut authorized = HashSet::new();
    for (index, record) in records.iter().enumerate() {
        if is_authorized(record, &records, &by_id, context)? {
            authorized.insert(index);
        } else {
            excluded.push(CodingSessionTeamFoldExclusion {
                event_id: record.id.clone(),
                code: CodingSessionTeamFoldExclusionCode::Unauthorized,
                reason: "signer lacks the operation's required active authority".into(),
            });
        }
    }

    let mut active = Vec::new();
    let mut conflicts = Vec::new();
    for stage in [
        ProjectionStage::Assignment,
        ProjectionStage::Report,
        ProjectionStage::Refutation,
        ProjectionStage::Disposition,
        ProjectionStage::Acknowledgement,
    ] {
        project_stage(
            &records,
            &by_id,
            &authorized,
            &mut active,
            &mut excluded,
            &mut conflicts,
            stage,
        );
    }

    let mut assignments = settle_assignments(&records, &active, &mut conflicts);
    let settled: HashMap<&str, bool> = assignments
        .iter()
        .map(|state| (state.assignment_event_id.as_str(), state.settled))
        .collect();

    let mut terminal_authorized = authorized.clone();
    for &index in &authorized {
        let CodingSessionTeamTransactionBody::MissionCompleted(body) = &records[index].payload.body
        else {
            continue;
        };
        let all_assignments_active = body.assignment_refs.iter().all(|reference| {
            by_id
                .get(reference)
                .is_some_and(|assignment| active.contains(assignment))
        });
        if all_assignments_active
            && !body
                .assignment_refs
                .iter()
                .all(|reference| settled.get(reference.as_str()) == Some(&true))
        {
            terminal_authorized.remove(&index);
            excluded.push(CodingSessionTeamFoldExclusion {
                event_id: records[index].id.clone(),
                code: CodingSessionTeamFoldExclusionCode::CompletionNotApproved,
                reason: "mission.completed requires every named active assignment to have an acknowledged approving disposition".into(),
            });
        }
    }
    for stage in [
        ProjectionStage::MissionCompleted,
        ProjectionStage::MissionBlocked,
    ] {
        project_stage(
            &records,
            &by_id,
            &terminal_authorized,
            &mut active,
            &mut excluded,
            &mut conflicts,
            stage,
        );
    }

    let canonical_terminal = project_terminal(&records, &mut active, &mut excluded, &mut conflicts);
    sort_indices(&mut active, &records);
    assignments.sort_by(|left, right| left.assignment_event_id.cmp(&right.assignment_event_id));
    excluded.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    conflicts.sort_by(|left, right| {
        left.subject
            .cmp(&right.subject)
            .then_with(|| left.winner_event_id.cmp(&right.winner_event_id))
    });

    Ok(CodingSessionTeamFold {
        included_event_ids: active
            .into_iter()
            .map(|index| records[index].id.clone())
            .collect(),
        excluded,
        conflicts,
        assignments,
        canonical_terminal,
    })
}

fn validate_references(
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
) -> Result<(), String> {
    for record in records {
        for reference in record.payload.causal_references() {
            if !by_id.contains_key(reference) {
                return Err(format!(
                    "team transaction {} has dangling reference {reference}",
                    record.id
                ));
            }
        }
        if let Some(reference) = &record.payload.supersedes {
            let previous = by_id.get(reference).ok_or_else(|| {
                format!(
                    "team transaction {} has dangling supersedes {reference}",
                    record.id
                )
            })?;
            validate_coding_session_team_transaction_supersession(
                record.event,
                records[*previous].event,
            )?;
            if verdict_subtype(&record.payload) != verdict_subtype(&records[*previous].payload) {
                return Err("a verdict correction must preserve its subtype".into());
            }
            if logical_subject(&record.payload) != logical_subject(&records[*previous].payload) {
                return Err("a correction must preserve its logical subject".into());
            }
        }
        validate_causal_types(record, records, by_id)?;
    }
    Ok(())
}

fn validate_causal_types(
    record: &Record<'_>,
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
) -> Result<(), String> {
    let get = |reference: &str| -> &Record<'_> { &records[by_id[reference]] };
    match &record.payload.body {
        CodingSessionTeamTransactionBody::Assignment(_) => {}
        CodingSessionTeamTransactionBody::Report(body) => {
            require_type(
                get(&body.assignment_ref),
                CodingSessionTeamTransactionType::Assignment,
            )?;
        }
        CodingSessionTeamTransactionBody::Verdict(verdict) => {
            require_type(
                get(verdict.assignment_ref()),
                CodingSessionTeamTransactionType::Assignment,
            )?;
            let report = get(verdict.report_ref());
            require_type(report, CodingSessionTeamTransactionType::Report)?;
            let CodingSessionTeamTransactionBody::Report(report_body) = &report.payload.body else {
                return Err("verdict reportRef must name a report body".into());
            };
            if report_body.assignment_ref != verdict.assignment_ref() {
                return Err("verdict assignmentRef must match its report's assignmentRef".into());
            }
            if let CodingSessionTeamVerdict::Disposition {
                refutation_ref: Some(reference),
                ..
            } = verdict
            {
                let refutation = get(reference);
                let CodingSessionTeamTransactionBody::Verdict(
                    CodingSessionTeamVerdict::Refutation {
                        assignment_ref,
                        report_ref,
                        ..
                    },
                ) = &refutation.payload.body
                else {
                    return Err("disposition refutationRef must name a refutation verdict".into());
                };
                if assignment_ref != verdict.assignment_ref() || report_ref != verdict.report_ref()
                {
                    return Err(
                        "disposition refutationRef must govern the same assignment/report pair"
                            .into(),
                    );
                }
            }
        }
        CodingSessionTeamTransactionBody::Acknowledgement(body) => {
            let acknowledged = get(&body.acknowledged_event_ref);
            if !matches!(
                acknowledged.payload.body,
                CodingSessionTeamTransactionBody::Verdict(
                    CodingSessionTeamVerdict::Disposition { .. }
                )
            ) {
                return Err("acknowledgement must name a disposition verdict".into());
            }
        }
        CodingSessionTeamTransactionBody::MissionCompleted(body) => {
            for reference in &body.assignment_refs {
                require_type(get(reference), CodingSessionTeamTransactionType::Assignment)?;
            }
        }
        CodingSessionTeamTransactionBody::MissionBlocked(body) => {
            for reference in &body.assignment_refs {
                require_type(get(reference), CodingSessionTeamTransactionType::Assignment)?;
            }
        }
    }
    Ok(())
}

fn require_type(
    record: &Record<'_>,
    expected: CodingSessionTeamTransactionType,
) -> Result<(), String> {
    if record.payload.transaction_type != expected {
        return Err(format!(
            "team transaction {} has a wrong-type reference",
            record.id
        ));
    }
    Ok(())
}

fn reject_cycles(records: &[Record<'_>], by_id: &HashMap<String, usize>) -> Result<(), String> {
    fn visit(
        index: usize,
        records: &[Record<'_>],
        by_id: &HashMap<String, usize>,
        state: &mut [u8],
    ) -> Result<(), String> {
        if state[index] == 1 {
            return Err("team transaction reference graph contains a cycle".into());
        }
        if state[index] == 2 {
            return Ok(());
        }
        state[index] = 1;
        let mut references = records[index].payload.causal_references();
        if let Some(reference) = &records[index].payload.supersedes {
            references.push(reference);
        }
        for reference in references {
            visit(by_id[reference], records, by_id, state)?;
        }
        state[index] = 2;
        Ok(())
    }

    let mut state = vec![0; records.len()];
    for index in 0..records.len() {
        visit(index, records, by_id, &mut state)?;
    }
    Ok(())
}

fn is_authorized(
    record: &Record<'_>,
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
    context: &CodingSessionTeamFoldContext,
) -> Result<bool, String> {
    Ok(match &record.payload.body {
        CodingSessionTeamTransactionBody::Assignment(_)
        | CodingSessionTeamTransactionBody::MissionCompleted(_)
        | CodingSessionTeamTransactionBody::MissionBlocked(_) => context.may_lead(&record.author),
        CodingSessionTeamTransactionBody::Report(body) => {
            assignment_actor(&records[by_id[&body.assignment_ref]])? == record.author
        }
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            ..
        }) => context.is_active_role(&record.author, "verifier"),
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
            ..
        }) => context.may_lead(&record.author),
        CodingSessionTeamTransactionBody::Acknowledgement(body) => {
            let disposition = &records[by_id[&body.acknowledged_event_ref]];
            let CodingSessionTeamTransactionBody::Verdict(verdict) = &disposition.payload.body
            else {
                return Err("acknowledgement reference must name a verdict body".into());
            };
            assignment_actor(&records[by_id[verdict.assignment_ref()]])? == record.author
        }
    })
}

fn assignment_actor<'a>(record: &'a Record<'_>) -> Result<&'a str, String> {
    let CodingSessionTeamTransactionBody::Assignment(body) = &record.payload.body else {
        return Err("expected assignment record".into());
    };
    Ok(&body.assignee_actor)
}

fn verdict_subtype(payload: &CodingSessionTeamTransactionPayload) -> Option<&'static str> {
    match &payload.body {
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            ..
        }) => Some("refutation"),
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
            ..
        }) => Some("disposition"),
        _ => None,
    }
}

fn logical_subject(payload: &CodingSessionTeamTransactionPayload) -> String {
    match &payload.body {
        CodingSessionTeamTransactionBody::Assignment(body) => {
            format!("assignment:{}:{}", body.assignee_actor, body.assignee_role)
        }
        CodingSessionTeamTransactionBody::Report(body) => {
            format!("report:{}", body.assignment_ref)
        }
        CodingSessionTeamTransactionBody::Verdict(verdict) => format!(
            "verdict:{}:{}:{}",
            verdict_subtype(payload).unwrap_or("unknown"),
            verdict.assignment_ref(),
            verdict.report_ref()
        ),
        CodingSessionTeamTransactionBody::Acknowledgement(body) => {
            format!("acknowledgement:{}", body.acknowledged_event_ref)
        }
        CodingSessionTeamTransactionBody::MissionCompleted(_) => "mission.completed".into(),
        CodingSessionTeamTransactionBody::MissionBlocked(_) => "mission.blocked".into(),
    }
}

fn project_stage(
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
    authorized: &HashSet<usize>,
    active: &mut Vec<usize>,
    excluded: &mut Vec<CodingSessionTeamFoldExclusion>,
    conflicts: &mut Vec<CodingSessionTeamFoldConflict>,
    stage: ProjectionStage,
) {
    let active_set: HashSet<usize> = active.iter().copied().collect();
    let mut candidates = HashSet::new();
    for &index in authorized {
        if !matches_stage(&records[index].payload, stage) {
            continue;
        }
        let inactive_parent = records[index]
            .payload
            .causal_references()
            .into_iter()
            .map(|reference| by_id[reference])
            .find(|parent| !active_set.contains(parent));
        if let Some(parent) = inactive_parent {
            exclude_dependency(index, parent, records, excluded, "causal");
        } else {
            candidates.insert(index);
        }
    }

    loop {
        let invalid_children: Vec<(usize, usize)> = candidates
            .iter()
            .filter_map(|index| {
                records[*index]
                    .payload
                    .supersedes
                    .as_ref()
                    .map(|reference| (*index, by_id[reference]))
                    .filter(|(_, parent)| !candidates.contains(parent))
            })
            .collect();
        if invalid_children.is_empty() {
            break;
        }
        for (child, parent) in invalid_children {
            if candidates.remove(&child) {
                exclude_dependency(child, parent, records, excluded, "correction");
            }
        }
    }

    let (winners, mut stage_excluded, mut stage_conflicts) =
        project_corrections(records, by_id, &candidates);
    active.extend(winners);
    excluded.append(&mut stage_excluded);
    conflicts.append(&mut stage_conflicts);
}

fn matches_stage(payload: &CodingSessionTeamTransactionPayload, stage: ProjectionStage) -> bool {
    matches!(
        (&payload.body, stage),
        (
            CodingSessionTeamTransactionBody::Assignment(_),
            ProjectionStage::Assignment
        ) | (
            CodingSessionTeamTransactionBody::Report(_),
            ProjectionStage::Report
        ) | (
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation { .. }),
            ProjectionStage::Refutation
        ) | (
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition { .. }),
            ProjectionStage::Disposition
        ) | (
            CodingSessionTeamTransactionBody::Acknowledgement(_),
            ProjectionStage::Acknowledgement
        ) | (
            CodingSessionTeamTransactionBody::MissionCompleted(_),
            ProjectionStage::MissionCompleted
        ) | (
            CodingSessionTeamTransactionBody::MissionBlocked(_),
            ProjectionStage::MissionBlocked
        )
    )
}

fn exclude_dependency(
    child: usize,
    parent: usize,
    records: &[Record<'_>],
    excluded: &mut Vec<CodingSessionTeamFoldExclusion>,
    edge: &str,
) {
    let parent_code = excluded
        .iter()
        .find(|item| item.event_id == records[parent].id)
        .map(|item| item.code);
    let code = match parent_code {
        Some(CodingSessionTeamFoldExclusionCode::Unauthorized) => {
            CodingSessionTeamFoldExclusionCode::DependentOnUnauthorized
        }
        Some(
            CodingSessionTeamFoldExclusionCode::Superseded
            | CodingSessionTeamFoldExclusionCode::CorrectionConflict,
        ) => CodingSessionTeamFoldExclusionCode::DependentOnSuperseded,
        _ => CodingSessionTeamFoldExclusionCode::DependentOnExcluded,
    };
    excluded.push(CodingSessionTeamFoldExclusion {
        event_id: records[child].id.clone(),
        code,
        reason: format!(
            "{edge} parent {} is not active in the canonical transaction graph",
            records[parent].id
        ),
    });
}

fn project_corrections(
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
    authorized: &HashSet<usize>,
) -> (
    Vec<usize>,
    Vec<CodingSessionTeamFoldExclusion>,
    Vec<CodingSessionTeamFoldConflict>,
) {
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for &index in authorized {
        let mut root = index;
        while let Some(reference) = &records[root].payload.supersedes {
            let previous = by_id[reference];
            if !authorized.contains(&previous) {
                break;
            }
            root = previous;
        }
        groups.entry(root).or_default().push(index);
    }

    let mut active = Vec::new();
    let mut excluded = Vec::new();
    let mut conflicts = Vec::new();
    for mut group in groups.into_values() {
        sort_indices(&mut group, records);
        let superseded: HashSet<usize> = group
            .iter()
            .filter_map(|index| {
                records[*index]
                    .payload
                    .supersedes
                    .as_ref()
                    .map(|reference| by_id[reference])
                    .filter(|previous| authorized.contains(previous))
            })
            .collect();
        let mut heads: Vec<usize> = group
            .iter()
            .copied()
            .filter(|index| !superseded.contains(index))
            .collect();
        sort_indices(&mut heads, records);
        let Some(&winner) = heads.last() else {
            continue;
        };
        active.push(winner);
        for index in group {
            if index == winner {
                continue;
            }
            let (code, reason) = if heads.contains(&index) {
                (
                    CodingSessionTeamFoldExclusionCode::CorrectionConflict,
                    "lost deterministic correction-fork ordering",
                )
            } else {
                (
                    CodingSessionTeamFoldExclusionCode::Superseded,
                    "replaced by a valid same-subject correction",
                )
            };
            excluded.push(CodingSessionTeamFoldExclusion {
                event_id: records[index].id.clone(),
                code,
                reason: reason.into(),
            });
        }
        if heads.len() > 1 {
            conflicts.push(CodingSessionTeamFoldConflict {
                subject: format!("correction:{}", logical_subject(&records[winner].payload)),
                winner_event_id: records[winner].id.clone(),
                contender_event_ids: heads
                    .into_iter()
                    .map(|index| records[index].id.clone())
                    .collect(),
            });
        }
    }
    (active, excluded, conflicts)
}

fn settle_assignments(
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
            sort_indices(&mut acknowledgements, records);
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
                chains.push((disposition_index, report_index, *acknowledgement));
            }
        }
        chains.sort_by(|left, right| compare_records(left.0, right.0, records));
        let winner = chains.last().copied();
        if chains.len() > 1 {
            if let Some((winner_index, _, _)) = winner {
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
            acknowledgement_event_id: winner.map(|chain| records[chain.2].id.clone()),
            settled: winner.is_some(),
        });
    }
    result
}

fn project_terminal(
    records: &[Record<'_>],
    active: &mut Vec<usize>,
    excluded: &mut Vec<CodingSessionTeamFoldExclusion>,
    conflicts: &mut Vec<CodingSessionTeamFoldConflict>,
) -> Option<CodingSessionTeamCanonicalTerminal> {
    let mut terminals: Vec<usize> = active
        .iter()
        .copied()
        .filter(|index| {
            matches!(
                records[*index].payload.transaction_type,
                CodingSessionTeamTransactionType::MissionCompleted
                    | CodingSessionTeamTransactionType::MissionBlocked
            )
        })
        .collect();
    sort_indices(&mut terminals, records);
    let winner = *terminals.last()?;
    if terminals.len() > 1 {
        conflicts.push(CodingSessionTeamFoldConflict {
            subject: "terminal".into(),
            winner_event_id: records[winner].id.clone(),
            contender_event_ids: terminals
                .iter()
                .map(|index| records[*index].id.clone())
                .collect(),
        });
    }
    for loser in terminals.into_iter().filter(|index| *index != winner) {
        active.retain(|index| *index != loser);
        excluded.push(CodingSessionTeamFoldExclusion {
            event_id: records[loser].id.clone(),
            code: CodingSessionTeamFoldExclusionCode::TerminalConflict,
            reason: "a newer authorized terminal transaction is canonical".into(),
        });
    }
    Some(CodingSessionTeamCanonicalTerminal {
        event_id: records[winner].id.clone(),
        transaction_type: records[winner].payload.transaction_type,
    })
}

fn compare_records(left: usize, right: usize, records: &[Record<'_>]) -> std::cmp::Ordering {
    records[left]
        .event
        .created_at
        .as_secs()
        .cmp(&records[right].event.created_at.as_secs())
        .then_with(|| records[left].id.cmp(&records[right].id))
}

fn sort_indices(indices: &mut [usize], records: &[Record<'_>]) {
    indices.sort_by(|left, right| compare_records(*left, *right, records));
}

#[cfg(test)]
#[path = "coding_session_team_transaction_fold_tests.rs"]
mod tests;
