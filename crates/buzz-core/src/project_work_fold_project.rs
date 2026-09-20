//! The per-declaration and per-criterion projection of the coverage fold.
//!
//! A child of `project_work_fold`, split out only to keep every file under
//! 1,000 lines. Every rule here is the contract's
//! (`conformance/project-work/README.md` § (c)) and every message it composes
//! is pinned by a sequence fixture.

use super::*;

type Assignments<'a> = [(&'a Record, &'a ProjectWorkAssignmentBound)];
type Evidence<'a> = [(&'a Record, &'a ProjectWorkEvidenceBound)];

/// A criterion's outcome before it is dressed as a projection.
struct Outcome {
    status: WorkCriterionStatus,
    reason_code: Option<WorkReasonCode>,
    reason: Option<String>,
}

impl Outcome {
    fn covered() -> Self {
        Self {
            status: WorkCriterionStatus::Covered,
            reason_code: None,
            reason: None,
        }
    }

    fn open(code: WorkReasonCode, reason: impl Into<String>) -> Self {
        Self {
            status: WorkCriterionStatus::Open,
            reason_code: Some(code),
            reason: Some(reason.into()),
        }
    }

    fn unknown(code: WorkReasonCode, reason: impl Into<String>) -> Self {
        Self {
            status: WorkCriterionStatus::Unknown,
            reason_code: Some(code),
            reason: Some(reason.into()),
        }
    }

    fn stale(code: WorkReasonCode, reason: impl Into<String>) -> Self {
        Self {
            status: WorkCriterionStatus::Stale,
            reason_code: Some(code),
            reason: Some(reason.into()),
        }
    }
}

/// Project one declaration: its state, its reason, its criteria and the one
/// artifact its coverage is about.
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

    let mut heads: Vec<String> = declarations
        .iter()
        .filter(|(other, other_body)| {
            other_body.work_id == body.work_id
                && states
                    .get(&other.id)
                    .is_some_and(|(s, _)| *s == WorkDeclarationState::Conflict)
        })
        .map(|(other, _)| other.id.clone())
        .collect();
    heads.sort();

    let (state_reason_code, state_reason) = match state {
        WorkDeclarationState::Head => (None, None),
        WorkDeclarationState::Superseded => (
            Some(WorkStateReasonCode::Superseded),
            Some(format!(
                "superseded by {}",
                join_ids(&superseded_by.iter().map(|id| short(id)).collect::<Vec<_>>())
            )),
        ),
        WorkDeclarationState::Stale => (
            Some(WorkStateReasonCode::GoalChanged),
            inputs.current_goal_ref.as_ref().map(|current| {
                format!(
                    "adopted against goal {}; the session's current goal is {}. \
                     The contract stays pinned until the lead adopts an amendment",
                    short(&body.goal_ref),
                    short(current)
                )
            }),
        ),
        WorkDeclarationState::Conflict => (
            Some(WorkStateReasonCode::Conflict),
            Some(conflict_message(&heads)),
        ),
    };

    let criteria = if !state.projects_criteria() {
        Vec::new()
    } else if let Some(plan) = plan.as_ref() {
        plan.criteria
            .iter()
            .map(|criterion| {
                project_criterion(
                    criterion,
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

    let artifact_commits = covering_artifacts(&criteria);
    let candidate_artifact = candidate_artifact(plan.as_ref(), &criteria, &artifact_commits);
    let (coverage_complete, coverage_reason_code, coverage_reason) = coverage(
        state,
        plan_resolved,
        &criteria,
        heads.len(),
        &artifact_commits,
        candidate_artifact.as_deref(),
    );
    WorkDeclarationProjection {
        work_id: body.work_id.clone(),
        declaration_ref: record.id.clone(),
        plan_ref: body.plan_ref.clone(),
        state,
        // `supersedes` is reported exactly as recorded, in every state;
        // `supersededBy` is the derived inverse. A projection that dropped
        // an edge in one direction while asserting it in the other would be
        // lying about lineage (A3 ruling on W1's finding).
        supersedes: body.supersedes.clone(),
        superseded_by,
        state_reason_code,
        state_reason,
        plan_resolved,
        candidate_artifact,
        artifact_commits,
        criteria,
        coverage_complete,
        coverage_reason_code,
        coverage_reason,
    }
}

/// The distinct commits the **covering** evidence names, ascending.
///
/// Only covering evidence: a criterion whose claim did not hold names a
/// commit, but it is not a commit anything was proven at.
fn covering_artifacts(criteria: &[WorkCriterionProjection]) -> Vec<String> {
    let mut commits: BTreeSet<String> = BTreeSet::new();
    for criterion in criteria
        .iter()
        .filter(|criterion| criterion.status == WorkCriterionStatus::Covered)
    {
        if let Some(commit) = criterion.artifact_commit.as_ref() {
            commits.insert(commit.clone());
        }
    }
    commits.into_iter().collect()
}

/// The one delivered revision this declaration's coverage is about.
///
/// When the plan has a `git-ref` criterion, only that criterion's evidence
/// says which commit was delivered, so the candidate is its commit and
/// `null` until it is covered. Otherwise the candidate is the single commit
/// every covering criterion shares.
fn candidate_artifact(
    plan: Option<&Plan>,
    criteria: &[WorkCriterionProjection],
    artifact_commits: &[String],
) -> Option<String> {
    let has_git_ref = plan.is_some_and(|plan| {
        plan.criteria
            .iter()
            .any(|criterion| criterion.proof == PlanProof::GitRef)
    });
    if has_git_ref {
        return criteria
            .iter()
            .find(|criterion| {
                criterion.proof.as_ref() == Some(&PlanProof::GitRef)
                    && criterion.status == WorkCriterionStatus::Covered
            })
            .and_then(|criterion| criterion.artifact_commit.clone());
    }
    match artifact_commits {
        [only] => Some(only.clone()),
        _ => None,
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
            reason_code: Some(WorkReasonCode::PlanUnreadable),
            reason: Some(reason.clone()),
        })
        .collect()
}

/// Project one criterion under one declaration.
#[allow(clippy::too_many_arguments)]
fn project_criterion(
    criterion: &PlanCriterion,
    record: &Record,
    declared: &ProjectWorkDeclared,
    declarations: &Declarations<'_>,
    assignments: &Assignments<'_>,
    evidence: &Evidence<'_>,
    plan: &Plan,
    inputs: &WorkFoldInputs,
) -> WorkCriterionProjection {
    let criterion_id = criterion.id.as_str();
    let mut assignment_refs: Vec<String> = Vec::new();
    for (_, body) in assignments.iter().filter(|(_, body)| {
        body.declaration_ref == record.id && body.criterion_ids.iter().any(|id| id == criterion_id)
    }) {
        if !assignment_refs.contains(&body.assignment_ref) {
            assignment_refs.push(body.assignment_ref.clone());
        }
    }

    let mut bound: Vec<&(&Record, &ProjectWorkEvidenceBound)> = evidence
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
        let outcome = elsewhere.map(|(_, body)| {
            Outcome::stale(
                WorkReasonCode::BoundToSupersededDeclaration,
                format!(
                    "evidence for this criterion is bound to declaration {}, which is not the head",
                    short(&body.declaration_ref)
                ),
            )
        });
        return WorkCriterionProjection {
            criterion_id: criterion_id.to_owned(),
            proof: Some(criterion.proof.clone()),
            status: outcome
                .as_ref()
                .map_or(WorkCriterionStatus::Open, |outcome| outcome.status),
            // Every valid assignment binding for this criterion under this
            // declaration, evidence or not: the projection's job is "what
            // remains and **who owes it**" (A3 ruling on W1's finding).
            assignment_refs,
            evidence: Vec::new(),
            artifact_commit: None,
            reason_code: outcome.as_ref().and_then(|outcome| outcome.reason_code),
            reason: outcome.and_then(|outcome| outcome.reason),
        };
    }

    bound.sort_by_key(|(binding, _)| binding.key());
    let mut refs: Vec<ProjectWorkEvidenceRef> = Vec::new();
    for (_, body) in &bound {
        for reference in &body.evidence_refs {
            if !refs.contains(reference) {
                refs.push(reference.clone());
            }
        }
    }
    let artifact_commit = bound
        .last()
        .map(|(_, body)| body.artifact_commit.clone())
        .unwrap_or_default();
    let outcome = evaluate(
        criterion,
        &refs,
        &artifact_commit,
        &assignment_refs,
        plan,
        inputs,
    );
    WorkCriterionProjection {
        criterion_id: criterion_id.to_owned(),
        proof: Some(criterion.proof.clone()),
        status: outcome.status,
        assignment_refs,
        evidence: refs,
        artifact_commit: Some(artifact_commit),
        reason_code: outcome.reason_code,
        reason: outcome.reason,
    }
}

/// Judge one criterion's bound evidence against the predicate its `proof`
/// form requires.
///
/// Absent evidence is `unknown`; evidence that resolved and failed is `open`
/// with its reason named. A binding is a claim, and this is where the claim
/// is checked.
fn evaluate(
    criterion: &PlanCriterion,
    refs: &[ProjectWorkEvidenceRef],
    artifact_commit: &str,
    assignment_refs: &[String],
    plan: &Plan,
    inputs: &WorkFoldInputs,
) -> Outcome {
    for reference in refs {
        let resolved = match reference.kind {
            ProjectWorkEvidenceKind::RefObservation => inputs
                .ref_states
                .iter()
                .any(|state| state.id == reference.event_id),
            _ => inputs.evidence.contains_key(&reference.event_id),
        };
        if !resolved {
            return Outcome::unknown(
                WorkReasonCode::EvidenceUnavailable,
                format!(
                    "evidence {} was not supplied to the fold; nothing here says what it proves",
                    short(&reference.event_id)
                ),
            );
        }
    }
    match &criterion.proof {
        PlanProof::Review => evaluate_review(refs, artifact_commit, assignment_refs, inputs),
        PlanProof::Action { name, step } => {
            evaluate_action(refs, artifact_commit, name, step, inputs)
        }
        PlanProof::GitRef => evaluate_git_ref(refs, artifact_commit, plan, inputs),
    }
}

/// `review` — an approving 44244 disposition by an actor `may_lead` admits,
/// on a report for a bound assignment at the binding's artifact commit.
fn evaluate_review(
    refs: &[ProjectWorkEvidenceRef],
    artifact_commit: &str,
    assignment_refs: &[String],
    inputs: &WorkFoldInputs,
) -> Outcome {
    let mut last: Option<Outcome> = None;
    for reference in refs
        .iter()
        .filter(|reference| reference.kind == ProjectWorkEvidenceKind::Verdict)
    {
        let Some(WorkEvidenceFact::Verdict {
            event_id,
            signer,
            subtype,
            decision,
            assignment_ref,
            report_ref,
        }) = inputs.evidence.get(&reference.event_id)
        else {
            continue;
        };
        if subtype != "disposition" {
            last = Some(Outcome::open(
                WorkReasonCode::NotApproving,
                format!(
                    "verdict {} is a {subtype}; only an approving disposition satisfies a review criterion",
                    short(event_id)
                ),
            ));
            continue;
        }
        if !inputs.authority.may_lead(signer) {
            last = Some(Outcome::open(
                WorkReasonCode::WrongSigner,
                format!(
                    "verdict {} is signed by {}, who does not satisfy may_lead for this session",
                    short(event_id),
                    short(signer)
                ),
            ));
            continue;
        }
        if !matches!(decision.as_str(), "approve" | "approve-with-notes") {
            last = Some(Outcome::open(
                WorkReasonCode::NotApproving,
                format!(
                    "disposition {} decided {decision}; only approve or approve-with-notes satisfy a review criterion",
                    short(event_id)
                ),
            ));
            continue;
        }
        if !assignment_refs.is_empty() && !assignment_refs.iter().any(|id| id == assignment_ref) {
            last = Some(Outcome::open(
                WorkReasonCode::RevisionMismatch,
                format!(
                    "disposition {} governs assignment {}, which this criterion is not bound to",
                    short(event_id),
                    short(assignment_ref)
                ),
            ));
            continue;
        }
        let Some(WorkEvidenceFact::Report {
            event_id: report_id,
            head_sha,
            ..
        }) = inputs.evidence.get(report_ref)
        else {
            last = Some(Outcome::unknown(
                WorkReasonCode::EvidenceUnavailable,
                format!(
                    "the report {} rules on was not supplied to the fold",
                    short(event_id)
                ),
            ));
            continue;
        };
        if head_sha != artifact_commit {
            last = Some(Outcome::open(
                WorkReasonCode::RevisionMismatch,
                format!(
                    "report {} names headSha {}, not the binding's artifactCommit {}",
                    short(report_id),
                    short_commit(head_sha),
                    short_commit(artifact_commit)
                ),
            ));
            continue;
        }
        return Outcome::covered();
    }
    last.unwrap_or_else(|| {
        Outcome::open(
            WorkReasonCode::NotApproving,
            "no approving disposition is bound; a review criterion is answered by one",
        )
    })
}

/// `action` — a clean, exit-0 host result for the definition the caller
/// compiled at the plan commit, on the artifact commit.
fn evaluate_action(
    refs: &[ProjectWorkEvidenceRef],
    artifact_commit: &str,
    name: &str,
    step: &str,
    inputs: &WorkFoldInputs,
) -> Outcome {
    let Some(definition) = inputs.action_definitions.get(name) else {
        return Outcome::unknown(
            WorkReasonCode::EvidenceUnavailable,
            format!(
                "the {name} action was not compiled at the plan commit, \
                 so nothing here says what its result proves"
            ),
        );
    };
    let mut last: Option<Outcome> = None;
    for reference in refs
        .iter()
        .filter(|reference| reference.kind == ProjectWorkEvidenceKind::ActionResult)
    {
        let Some(WorkEvidenceFact::ActionResult {
            event_id,
            echo_signer,
            action_name,
            step_id,
            definition_hash,
            exit_code,
            checkout,
            dirty,
            ..
        }) = inputs.evidence.get(&reference.event_id)
        else {
            continue;
        };
        if inputs.relay_self_key.as_deref() != Some(echo_signer.as_str()) {
            last = Some(Outcome::open(
                WorkReasonCode::WrongRunOrHash,
                format!(
                    "host result {} was echoed by {}, not the relay's self key",
                    short(event_id),
                    short(echo_signer)
                ),
            ));
            continue;
        }
        if definition_hash != &definition.definition_hash || action_name != name {
            last = Some(Outcome::open(
                WorkReasonCode::WrongRunOrHash,
                format!(
                    "host result {} ran definition hash {}, not the {name} definition compiled at the plan commit ({})",
                    short(event_id),
                    short(definition_hash),
                    short(&definition.definition_hash)
                ),
            ));
            continue;
        }
        if step_id != step || !definition.steps.iter().any(|known| known == step) {
            last = Some(Outcome::open(
                WorkReasonCode::WrongRunOrHash,
                format!(
                    "host result {} ran step {step_id}, not {step}",
                    short(event_id)
                ),
            ));
            continue;
        }
        if checkout.sha != artifact_commit {
            last = Some(Outcome::open(
                WorkReasonCode::RevisionMismatch,
                format!(
                    "host result {} ran on {}, not the binding's artifactCommit {}",
                    short(event_id),
                    short_commit(&checkout.sha),
                    short_commit(artifact_commit)
                ),
            ));
            continue;
        }
        if *exit_code != 0 {
            last = Some(Outcome::open(
                WorkReasonCode::ActionFailed,
                format!("host result {} exited {exit_code}", short(event_id)),
            ));
            continue;
        }
        if checkout.dirty_before {
            last = Some(Outcome::open(
                WorkReasonCode::DirtyRevision,
                format!(
                    "host result {} ran on a tree that was already dirty before the command",
                    short(event_id)
                ),
            ));
            continue;
        }
        if *dirty {
            last = Some(Outcome::open(
                WorkReasonCode::DirtyRevision,
                format!(
                    "host result {} left the tree dirty after the command",
                    short(event_id)
                ),
            ));
            continue;
        }
        return Outcome::covered();
    }
    last.unwrap_or_else(|| {
        Outcome::open(
            WorkReasonCode::WrongRunOrHash,
            format!("no host result for {name}/{step} is bound"),
        )
    })
}

/// `git-ref` — judge delivery **at evaluation time**, from the newest
/// relay-signed ref state: the branch moved on is exactly what `stale` says.
fn evaluate_git_ref(
    refs: &[ProjectWorkEvidenceRef],
    artifact_commit: &str,
    plan: &Plan,
    inputs: &WorkFoldInputs,
) -> Outcome {
    if !refs
        .iter()
        .any(|reference| reference.kind == ProjectWorkEvidenceKind::RefObservation)
    {
        return Outcome::open(
            WorkReasonCode::RevisionMismatch,
            "a git-ref proof is answered by a relay-signed ref state, and none is bound",
        );
    }
    let Some(relay_self_key) = inputs.relay_self_key.as_deref() else {
        return Outcome::unknown(
            WorkReasonCode::EvidenceUnavailable,
            "the relay's self key was not supplied, so a ref observation cannot be judged",
        );
    };
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
        return Outcome::unknown(
            WorkReasonCode::EvidenceUnavailable,
            format!(
                "no relay-signed ref state for {repository} was supplied, \
                 so delivery cannot be judged"
            ),
        );
    };
    let delivery_ref = plan.delivery_ref_tag();
    let Some(observed) = newest.tag_value(delivery_ref) else {
        return Outcome::unknown(
            WorkReasonCode::EvidenceUnavailable,
            format!(
                "the newest relay-signed ref state for {repository} ({}) names no {delivery_ref}",
                short(&newest.id)
            ),
        );
    };
    if observed == artifact_commit {
        Outcome::covered()
    } else {
        Outcome::stale(
            WorkReasonCode::RefObservationSuperseded,
            format!(
                "the newest relay-signed ref state for {repository} ({}) names {delivery_ref} at {}, not the bound artifact commit",
                short(&newest.id),
                short_commit(observed)
            ),
        )
    }
}

#[path = "project_work_fold_coverage.rs"]
mod coverage_reason;
use coverage_reason::coverage;
