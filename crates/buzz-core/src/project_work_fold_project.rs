//! The per-declaration and per-criterion projection of the coverage fold.
//!
//! A child of `project_work_fold`, split out only to keep every file under
//! 1,000 lines. Every rule here is the contract's
//! (`conformance/project-work/README.md` § (c)) and every message it composes
//! is pinned by a sequence fixture.

use super::*;

type Assignments<'a> = [(&'a Record, &'a ProjectWorkAssignmentBound)];
type Evidence<'a> = [(&'a Record, &'a ProjectWorkEvidenceBound)];

/// Project one declaration: its state, its reason, and its criteria.
#[allow(clippy::too_many_arguments)]
pub(super) fn project_declaration(
    record: &Record,
    body: &ProjectWorkDeclared,
    states: &BTreeMap<String, (WorkDeclarationState, Vec<String>)>,
    declarations: &Declarations<'_>,
    assignments: &Assignments<'_>,
    evidence: &Evidence<'_>,
    inputs: &WorkFoldInputs,
) -> WorkDeclarationProjection {
    let (state, superseded_by) = states
        .get(&record.id)
        .cloned()
        .unwrap_or((WorkDeclarationState::Head, Vec::new()));
    let plan = inputs
        .plan_blobs
        .get(&body.plan_ref.blob_key())
        .and_then(|blob| parse_plan(blob.as_bytes()).ok());
    let plan_resolved = plan.is_some();

    let heads: Vec<String> = declarations
        .iter()
        .filter(|(other, other_body)| {
            other_body.work_id == body.work_id
                && states
                    .get(&other.id)
                    .is_some_and(|(s, _)| *s == WorkDeclarationState::Conflict)
        })
        .map(|(other, _)| other.id.clone())
        .collect();

    let state_reason = match state {
        WorkDeclarationState::Head => None,
        WorkDeclarationState::Superseded => Some(format!(
            "superseded by {}",
            join_ids(&superseded_by.iter().map(|id| short(id)).collect::<Vec<_>>())
        )),
        WorkDeclarationState::Stale => inputs.current_goal_ref.as_ref().map(|current| {
            format!(
                "adopted against goal {}; the session's current goal is {}. \
                 The contract stays pinned until the lead adopts an amendment",
                short(&body.goal_ref),
                short(current)
            )
        }),
        WorkDeclarationState::Conflict => {
            let mut sorted = heads.clone();
            sorted.sort();
            Some(conflict_message(declarations, &sorted))
        }
    };

    let criteria = if !state.projects_criteria() {
        Vec::new()
    } else if let Some(plan) = plan.as_ref() {
        plan.criteria
            .iter()
            .map(|criterion| {
                project_criterion(
                    &criterion.id,
                    Some(criterion.proof.clone()),
                    record,
                    body,
                    declarations,
                    assignments,
                    evidence,
                    plan,
                    inputs,
                )
            })
            .collect()
    } else {
        unresolved_criteria(record, assignments, evidence, &body.plan_ref)
    };

    let (coverage_complete, coverage_reason) =
        coverage(state, plan_resolved, &criteria, heads.len());
    WorkDeclarationProjection {
        work_id: body.work_id.clone(),
        declaration_ref: record.id.clone(),
        plan_ref: body.plan_ref.clone(),
        state,
        supersedes: body.supersedes.clone(),
        superseded_by,
        state_reason,
        plan_resolved,
        criteria,
        coverage_complete,
        coverage_reason,
    }
}

/// When the plan blob is missing, the fold still knows which criteria have
/// been *named* by bindings. It reports exactly those, every one `unknown`,
/// and never `open` — which would read as "nothing has been done" when the
/// truth is "we cannot see the list".
fn unresolved_criteria(
    record: &Record,
    assignments: &Assignments<'_>,
    evidence: &Evidence<'_>,
    plan_ref: &ProjectWorkPlanRef,
) -> Vec<WorkCriterionProjection> {
    let mut named: BTreeSet<&str> = BTreeSet::new();
    for (_, body) in assignments
        .iter()
        .filter(|(_, body)| body.declaration_ref == record.id)
    {
        named.extend(body.criterion_ids.iter().map(String::as_str));
    }
    for (_, body) in evidence
        .iter()
        .filter(|(_, body)| body.declaration_ref == record.id)
    {
        named.extend(body.criterion_ids.iter().map(String::as_str));
    }
    let reason = format!(
        "the plan blob at {} was not supplied, so this criterion cannot be judged",
        short_commit(&plan_ref.commit)
    );
    named
        .into_iter()
        .map(|criterion_id| WorkCriterionProjection {
            criterion_id: criterion_id.to_owned(),
            proof: None,
            status: WorkCriterionStatus::Unknown,
            assignment_refs: Vec::new(),
            evidence: Vec::new(),
            artifact_commit: None,
            reason: Some(reason.clone()),
        })
        .collect()
}

/// Project one criterion under one declaration.
#[allow(clippy::too_many_arguments)]
fn project_criterion(
    criterion_id: &str,
    proof: Option<PlanProof>,
    record: &Record,
    declared: &ProjectWorkDeclared,
    declarations: &Declarations<'_>,
    assignments: &Assignments<'_>,
    evidence: &Evidence<'_>,
    plan: &Plan,
    inputs: &WorkFoldInputs,
) -> WorkCriterionProjection {
    let mut assignment_refs: Vec<String> = Vec::new();
    for (_, body) in assignments.iter().filter(|(binding, body)| {
        body.declaration_ref == record.id
            && body.criterion_ids.iter().any(|id| id == criterion_id)
            && binding.id != record.id
    }) {
        if !assignment_refs.contains(&body.assignment_ref) {
            assignment_refs.push(body.assignment_ref.clone());
        }
    }

    let bound: Vec<&(&Record, &ProjectWorkEvidenceBound)> = evidence
        .iter()
        .filter(|(_, body)| {
            body.declaration_ref == record.id
                && body.criterion_ids.iter().any(|id| id == criterion_id)
        })
        .collect();

    if bound.is_empty() {
        // The late-green-for-P case: evidence exists, but under a revision
        // that is not the head. It does not carry forward.
        let elsewhere = evidence
            .iter()
            .filter(|(_, body)| {
                body.criterion_ids.iter().any(|id| id == criterion_id)
                    && body.declaration_ref != record.id
                    && declarations.iter().any(|(other, other_body)| {
                        other.id == body.declaration_ref && other_body.work_id == declared.work_id
                    })
            })
            .max_by_key(|(binding, _)| binding.key());
        let reason = elsewhere.map(|(_, body)| {
            format!(
                "evidence for this criterion is bound to declaration {}, which is not the head",
                short(&body.declaration_ref)
            )
        });
        let status = if reason.is_some() {
            WorkCriterionStatus::Stale
        } else {
            WorkCriterionStatus::Open
        };
        return WorkCriterionProjection {
            criterion_id: criterion_id.to_owned(),
            proof,
            status,
            // An `open` criterion reports no bindings. This is the contract's
            // shape, pinned by `sequences/goal-changed`, where a criterion
            // carrying an assignment binding is nonetheless projected with
            // `assignmentRefs: []`. It is also the one place this projection
            // is *less* informative than it could be — an open criterion's
            // owner is exactly "who owes it" — and lane W1 has reported it to
            // the orchestrator as a contract defect rather than diverging
            // from the frozen fixture on its own authority.
            assignment_refs: if status == WorkCriterionStatus::Open {
                Vec::new()
            } else {
                assignment_refs
            },
            evidence: Vec::new(),
            artifact_commit: None,
            reason,
        };
    }

    let mut ordered = bound;
    ordered.sort_by_key(|(binding, _)| binding.key());
    let mut refs: Vec<ProjectWorkEvidenceRef> = Vec::new();
    for (_, body) in &ordered {
        for reference in &body.evidence_refs {
            if !refs.contains(reference) {
                refs.push(reference.clone());
            }
        }
    }
    let artifact_commit = ordered
        .last()
        .map(|(_, body)| body.artifact_commit.clone())
        .unwrap_or_default();
    let (status, reason) = evaluate_proof(proof.as_ref(), &refs, &artifact_commit, plan, inputs);
    WorkCriterionProjection {
        criterion_id: criterion_id.to_owned(),
        proof,
        status,
        assignment_refs,
        evidence: refs,
        artifact_commit: Some(artifact_commit),
        reason,
    }
}

/// Whether the bound evidence satisfies the proof form the plan requires.
///
/// A reference the caller did not establish makes the criterion `unknown`,
/// never `covered`; evidence of the wrong form leaves it `open` with a reason
/// naming what is missing, because the obligation is still unanswered.
fn evaluate_proof(
    proof: Option<&PlanProof>,
    refs: &[ProjectWorkEvidenceRef],
    artifact_commit: &str,
    plan: &Plan,
    inputs: &WorkFoldInputs,
) -> (WorkCriterionStatus, Option<String>) {
    if let Some(established) = inputs.resolved_evidence_ids.as_ref() {
        for reference in refs
            .iter()
            .filter(|reference| reference.kind != ProjectWorkEvidenceKind::RefObservation)
        {
            if !established.contains(&reference.event_id) {
                return (
                    WorkCriterionStatus::Unknown,
                    Some(format!(
                        "evidence event {} was not supplied, so this criterion cannot be judged",
                        short(&reference.event_id)
                    )),
                );
            }
        }
    }
    let has = |kinds: &[ProjectWorkEvidenceKind]| {
        refs.iter().any(|reference| kinds.contains(&reference.kind))
    };
    match proof {
        None => (
            WorkCriterionStatus::Unknown,
            Some(
                "the plan blob was not supplied, so the required proof form is unknown".to_owned(),
            ),
        ),
        Some(PlanProof::Review) => {
            if has(&[
                ProjectWorkEvidenceKind::Report,
                ProjectWorkEvidenceKind::Verdict,
            ]) {
                (WorkCriterionStatus::Covered, None)
            } else {
                (
                    WorkCriterionStatus::Open,
                    Some(
                        "a review proof is answered by a report or a verdict, and neither is bound"
                            .to_owned(),
                    ),
                )
            }
        }
        Some(PlanProof::Action { name, step }) => {
            if has(&[ProjectWorkEvidenceKind::ActionResult]) {
                (WorkCriterionStatus::Covered, None)
            } else {
                (
                    WorkCriterionStatus::Open,
                    Some(format!(
                        "an action proof is answered by the result of {name}/{step}, and none is bound"
                    )),
                )
            }
        }
        Some(PlanProof::GitRef) => evaluate_git_ref(refs, artifact_commit, plan, inputs),
    }
}

/// Judge delivery **at evaluation time**, from the newest relay-signed ref
/// state: the branch moved on is exactly what `stale` must say.
fn evaluate_git_ref(
    refs: &[ProjectWorkEvidenceRef],
    artifact_commit: &str,
    plan: &Plan,
    inputs: &WorkFoldInputs,
) -> (WorkCriterionStatus, Option<String>) {
    let observations: Vec<&ProjectWorkEvidenceRef> = refs
        .iter()
        .filter(|reference| reference.kind == ProjectWorkEvidenceKind::RefObservation)
        .collect();
    if observations.is_empty() {
        return (
            WorkCriterionStatus::Open,
            Some(
                "a git-ref proof is answered by a relay-signed ref state, and none is bound"
                    .to_owned(),
            ),
        );
    }
    let Some(relay_self_key) = inputs.relay_self_key.as_deref() else {
        return (
            WorkCriterionStatus::Unknown,
            Some(
                "the relay's self key was not supplied, so a ref observation cannot be judged"
                    .to_owned(),
            ),
        );
    };
    for observation in &observations {
        if !inputs
            .ref_states
            .iter()
            .any(|state| state.id == observation.event_id)
        {
            return (
                WorkCriterionStatus::Unknown,
                Some(format!(
                    "ref state {} was not supplied, so delivery cannot be judged",
                    short(&observation.event_id)
                )),
            );
        }
    }
    let repository = plan.code_repository.as_str();
    // An owner-signed claim about its own branch is not an observation: only
    // the relay that hosts the repository signs its ref state.
    let newest = inputs
        .ref_states
        .iter()
        .filter(|state| {
            state.kind == KIND_REF_STATE
                && state.pubkey == relay_self_key
                && state.tag_value("d") == Some(repository)
        })
        .max_by_key(|state| state.order_key());
    let Some(newest) = newest else {
        return (
            WorkCriterionStatus::Unknown,
            Some(format!(
                "no relay-signed ref state for {repository} was supplied, so delivery cannot be judged"
            )),
        );
    };
    let delivery_ref = plan.delivery_ref_tag();
    let Some(observed) = newest.tag_value(delivery_ref) else {
        return (
            WorkCriterionStatus::Unknown,
            Some(format!(
                "the newest relay-signed ref state for {repository} ({}) names no {delivery_ref}",
                short(&newest.id)
            )),
        );
    };
    if observed == artifact_commit {
        (WorkCriterionStatus::Covered, None)
    } else {
        (
            WorkCriterionStatus::Stale,
            Some(format!(
                "the newest relay-signed ref state for {repository} ({}) names {delivery_ref} at {}, not the bound artifact commit",
                short(&newest.id),
                short_commit(observed)
            )),
        )
    }
}

/// `coverageComplete`, and the sentence that says which clause failed.
fn coverage(
    state: WorkDeclarationState,
    plan_resolved: bool,
    criteria: &[WorkCriterionProjection],
    competing_heads: usize,
) -> (bool, Option<String>) {
    match state {
        WorkDeclarationState::Superseded => {
            return (
                false,
                Some("superseded: coverage is computed for the head declaration only".to_owned()),
            )
        }
        WorkDeclarationState::Conflict => {
            return (
                false,
                Some(format!(
                    "conflict: coverage is not computed while {} heads compete, and the later timestamp does not win",
                    count_word(competing_heads)
                )),
            )
        }
        WorkDeclarationState::Head | WorkDeclarationState::Stale => {}
    }
    if !plan_resolved {
        return (
            false,
            Some(
                "the plan blob for this declaration was not supplied, so coverage is unknown"
                    .to_owned(),
            ),
        );
    }
    let total = criteria.len();
    let not_covered: Vec<&WorkCriterionProjection> = criteria
        .iter()
        .filter(|criterion| criterion.status != WorkCriterionStatus::Covered)
        .collect();
    if not_covered.is_empty() {
        return (true, None);
    }
    if state == WorkDeclarationState::Stale
        && not_covered.len() == total
        && criteria
            .iter()
            .all(|criterion| criterion.status == WorkCriterionStatus::Open)
    {
        return (
            false,
            Some(format!(
                "the declaration is stale against the session's current goal, and all {total} criteria are open"
            )),
        );
    }
    let buckets = [
        WorkCriterionStatus::Stale,
        WorkCriterionStatus::Open,
        WorkCriterionStatus::Unknown,
    ]
    .into_iter()
    .map(|status| {
        (
            status,
            criteria
                .iter()
                .filter(|criterion| criterion.status == status)
                .map(|criterion| criterion.criterion_id.clone())
                .collect::<Vec<_>>(),
        )
    })
    .filter(|(_, ids)| !ids.is_empty())
    .collect::<Vec<_>>();
    let segments: Vec<String> = buckets
        .iter()
        .map(|(status, ids)| {
            if ids.len() <= 3 {
                let verb = if ids.len() == 1 { "is" } else { "are" };
                format!("{} {verb} {}", join_ids(ids), status.as_str())
            } else if buckets.len() == 1 {
                format!("all {} are {}", count_word(ids.len()), status.as_str())
            } else {
                format!(
                    "the other {} are {}",
                    count_word(ids.len()),
                    status.as_str()
                )
            }
        })
        .collect();
    (
        false,
        Some(format!(
            "{} of {total} criteria are not covered under this declaration: {}",
            not_covered.len(),
            segments.join("; ")
        )),
    )
}
