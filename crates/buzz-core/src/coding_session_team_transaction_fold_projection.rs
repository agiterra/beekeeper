//! Stage projection for the NIP-CSTX fold: which records enter a stage, which
//! are excluded for depending on something inactive, and which of a correction
//! group wins.
//!
//! A child of `coding_session_team_transaction_fold`, split out only to keep
//! every file under 1,000 lines (FINAL-B §7 asked B2 for this). No behaviour
//! change: these are the same four functions, and `use super::*` gives them the
//! parent's private `Record`, `ProjectionStage` and helpers exactly as before.

use std::collections::{HashMap, HashSet};

use super::*;

pub(super) fn project_stage(
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
            // Safe by the same invariant: a record with an unresolvable causal
            // reference never entered `authorized`.
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
                    // Safe by the same invariant, for `supersedes`: a correction
                    // naming an absent target is excluded before authorization.
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
            CodingSessionTeamTransactionBody::Note(_),
            ProjectionStage::Note
        ) | (
            CodingSessionTeamTransactionBody::DecisionRequest(_),
            ProjectionStage::DecisionRequest
        ) | (
            CodingSessionTeamTransactionBody::DecisionAnswer(_),
            ProjectionStage::DecisionAnswer
        ) | (
            CodingSessionTeamTransactionBody::MissionCompleted(_)
                | CodingSessionTeamTransactionBody::MissionBlocked(_),
            ProjectionStage::Terminal
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
            // Safe by the same invariant: every `supersedes` on an authorized
            // record resolves.
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
                    // Safe by the same invariant as the walk above.
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
