//! `coverageComplete`, and the sentence that says which clause failed.
//!
//! A child of `project_work_fold_project`. Coverage is a statement about
//! **one delivered revision**, not a per-criterion scoreboard: five criteria
//! individually green at three different commits is nothing verified at the
//! delivered commit, and this module is where that is said out loud.
//!
//! Every phrasing here is pinned by a sequence fixture under
//! `conformance/project-work/fixtures/sequences/`.

use super::*;

/// Decide `coverageComplete` and compose its reason.
pub(super) fn coverage(
    state: WorkDeclarationState,
    plan_ref: &ProjectWorkPlanRef,
    plan_resolved: bool,
    criteria: &[WorkCriterionProjection],
    competing_heads: usize,
    artifact_commits: &[String],
    candidate_artifact: Option<&str>,
) -> (bool, Option<WorkCoverageReasonCode>, Option<String>) {
    match state {
        WorkDeclarationState::Superseded => {
            return (
                false,
                Some(WorkCoverageReasonCode::Superseded),
                Some("superseded: coverage is computed for the head declaration only".to_owned()),
            )
        }
        WorkDeclarationState::Conflict => {
            return (
                false,
                Some(WorkCoverageReasonCode::Conflict),
                Some(format!(
                    "conflict: coverage is not computed while {} heads compete, and the later timestamp does not win",
                    count_word(competing_heads)
                )),
            )
        }
        WorkDeclarationState::Head | WorkDeclarationState::Stale => {}
    }
    // No plan blob means no criterion rows at all when nothing is bound yet.
    // The declaration still says why it is not complete, so "nothing to show"
    // can never be read as "nothing outstanding" (A5 decision 25).
    if !plan_resolved {
        return (
            false,
            Some(WorkCoverageReasonCode::PlanUnavailable),
            Some(format!(
                "the plan blob at {} was not supplied, so no criterion of this declaration can \
                 be evaluated",
                short_commit(&plan_ref.commit)
            )),
        );
    }
    let total = criteria.len();
    let not_covered: Vec<&WorkCriterionProjection> = criteria
        .iter()
        .filter(|criterion| criterion.status != WorkCriterionStatus::Covered)
        .collect();

    if not_covered.is_empty() {
        // Every criterion is covered. The remaining question is whether they
        // are covered at the *same* commit: five true statements about three
        // revisions prove nothing about the delivered one.
        if artifact_commits.len() == 1
            && candidate_artifact == artifact_commits.first().map(String::as_str)
        {
            return (true, None, None);
        }
        return (
            false,
            Some(WorkCoverageReasonCode::MixedArtifacts),
            Some(mixed_artifacts_reason(artifact_commits, candidate_artifact)),
        );
    }

    if state == WorkDeclarationState::Stale
        && not_covered.len() == total
        && criteria.iter().all(|criterion| {
            criterion.status == WorkCriterionStatus::Open && criterion.reason.is_none()
        })
    {
        return (
            false,
            Some(WorkCoverageReasonCode::CriteriaNotCovered),
            Some(format!(
                "the declaration is stale against the session's current goal, and all {total} criteria are open"
            )),
        );
    }
    // Two templates, both the contract's. When some criteria are covered and
    // the rest are too many to name, the fixture states the positive form —
    // "1 of 5 criteria is covered … the other four are open" — because the
    // list of what remains is a group, not a set of names.
    let covered = total - not_covered.len();
    if covered > 0 && not_covered.len() > 3 {
        return (
            false,
            Some(WorkCoverageReasonCode::CriteriaNotCovered),
            Some(format!(
                "{covered} of {total} criteria {} covered under this declaration: {}",
                if covered == 1 { "is" } else { "are" },
                not_covered_sentence(criteria)
            )),
        );
    }
    (
        false,
        Some(WorkCoverageReasonCode::CriteriaNotCovered),
        Some(format!(
            "{} of {total} criteria are not covered under this declaration: {}",
            not_covered.len(),
            not_covered_sentence(criteria)
        )),
    )
}

/// "every criterion is covered, but at three different artifact commits …".
fn mixed_artifacts_reason(artifact_commits: &[String], candidate: Option<&str>) -> String {
    let commits = artifact_commits
        .iter()
        .map(|commit| short_commit(commit))
        .collect::<Vec<_>>()
        .join(", ");
    let tail = candidate.map_or_else(
        || "no single commit is the one every criterion was proven at".to_owned(),
        |candidate| {
            format!(
                "the candidate is {} from the git-ref evidence, and nothing proves the delivered commit",
                short_commit(candidate)
            )
        },
    );
    format!(
        "every criterion is covered, but at {} different artifact commits ({commits}); {tail}",
        count_word(artifact_commits.len())
    )
}

/// Which criteria are not covered, grouped by why.
///
/// Three groups, in this order: criteria whose bound evidence **failed** its
/// predicate or could not be resolved, criteria that are `stale`, and
/// criteria nothing has been bound to. Each group names its criteria when
/// there are at most three of them, and counts them otherwise.
fn not_covered_sentence(criteria: &[WorkCriterionProjection]) -> String {
    let failed_criteria: Vec<&WorkCriterionProjection> = criteria
        .iter()
        .filter(|criterion| {
            matches!(
                criterion.status,
                WorkCriterionStatus::Open | WorkCriterionStatus::Unknown
            ) && criterion.reason.is_some()
        })
        .collect();
    let failed_kind = failed_kind(&failed_criteria);
    let failed: Vec<String> = failed_criteria
        .iter()
        .map(|criterion| criterion.criterion_id.clone())
        .collect();
    let unresolved = criteria
        .iter()
        .any(|criterion| criterion.status == WorkCriterionStatus::Unknown);
    let stale: Vec<String> = criteria
        .iter()
        .filter(|criterion| criterion.status == WorkCriterionStatus::Stale)
        .map(|criterion| criterion.criterion_id.clone())
        .collect();
    let open: Vec<String> = criteria
        .iter()
        .filter(|criterion| {
            criterion.status == WorkCriterionStatus::Open && criterion.reason.is_none()
        })
        .map(|criterion| criterion.criterion_id.clone())
        .collect();

    let groups: Vec<(&str, Vec<String>)> = [("failed", failed), ("stale", stale), ("open", open)]
        .into_iter()
        .filter(|(_, ids)| !ids.is_empty())
        .collect();
    // "all four" only when four is everything. With one criterion already
    // covered the remainder is "the other four", which is what the reader
    // needs to hear.
    let any_covered = criteria
        .iter()
        .any(|criterion| criterion.status == WorkCriterionStatus::Covered);
    let only_group = groups.len() == 1 && !any_covered;

    let mut sentence = String::new();
    for (index, (kind, ids)) in groups.iter().enumerate() {
        if index > 0 {
            // The failed group runs on into what follows; the others are
            // separate clauses. Both readings are the contract's, pinned by
            // `action-dirty` and `amendment` respectively.
            sentence.push_str(if groups[index - 1].0 == "failed" {
                ", and "
            } else {
                "; "
            });
        }
        sentence.push_str(&group_phrase(
            kind,
            ids,
            index == 0 && !any_covered,
            only_group,
            unresolved,
            failed_kind,
        ));
    }
    sentence
}

/// What a whole failed group has in common, when it has something.
///
/// The three A5 reasons read as facts about the *record*, not about a
/// predicate: evidence the team projection excludes, an approval that was
/// replaced, a run of another definition. Saying "failed its predicate"
/// about any of them would hide which of those three it is.
fn failed_kind(failed: &[&WorkCriterionProjection]) -> Option<WorkReasonCode> {
    let first = failed.first()?.reason_code?;
    let shared = failed
        .iter()
        .all(|criterion| criterion.reason_code == Some(first));
    // `wrong_run_or_hash` covers two different facts, and the fixtures
    // phrase them differently: a run of **another commit's** definition of
    // this action ("a run of another definition", `same-action-two-commits`)
    // and a run of an unrelated hash ("evidence that failed its predicate",
    // `action-hash-mismatch`). The per-criterion reason already distinguishes
    // them, and this reads that distinction rather than inventing a second
    // reason code the contract does not have.
    let another_definition = failed.iter().all(|criterion| {
        criterion
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("this declaration's plan commit"))
    });
    matches!(
        first,
        WorkReasonCode::ReportNotCanonical | WorkReasonCode::DispositionNotCanonical
    )
    .then_some(first)
    .or((first == WorkReasonCode::WrongRunOrHash && another_definition).then_some(first))
    .filter(|_| shared)
}

fn group_phrase(
    kind: &str,
    ids: &[String],
    first: bool,
    only: bool,
    unresolved: bool,
    failed_kind: Option<WorkReasonCode>,
) -> String {
    let named = ids.len() <= 3;
    let subject = if named {
        join_ids(ids)
    } else if only {
        format!("all {}", count_word(ids.len()))
    } else if first {
        count_word(ids.len())
    } else {
        format!("the other {}", count_word(ids.len()))
    };
    let singular = named && ids.len() == 1;
    match kind {
        "failed" => {
            match failed_kind {
                Some(WorkReasonCode::ReportNotCanonical) => {
                    let verb = if singular { "carries" } else { "carry" };
                    return format!("{subject} {verb} evidence the team projection excludes");
                }
                Some(WorkReasonCode::DispositionNotCanonical) => {
                    let verb = if singular { "was" } else { "were" };
                    return format!("{subject}'s approving disposition {verb} replaced");
                }
                Some(WorkReasonCode::WrongRunOrHash) => {
                    let verb = if singular { "carries" } else { "carry" };
                    return format!("{subject} {verb} a run of another definition");
                }
                _ => {}
            }
            let verb = if singular { "carries" } else { "carry" };
            let what = if unresolved {
                "evidence that failed its predicate or could not be resolved"
            } else {
                "evidence that failed its predicate"
            };
            format!("{subject} {verb} {what}")
        }
        other => {
            let verb = if singular { "is" } else { "are" };
            format!("{subject} {verb} {other}")
        }
    }
}
