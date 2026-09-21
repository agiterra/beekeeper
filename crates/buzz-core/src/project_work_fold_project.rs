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
    // One wording for the criterion's own reason (A6), and still two facts
    // for the declaration's sentence: a run of **another commit's**
    // definition of the same action is not the same event as a run of an
    // unrelated hash, and `same-action-two-commits` vs `action-hash-mismatch`
    // pin both. The discriminator lives here, where the inputs are, instead
    // of being read back out of a message.
    let ran_other_definition = criteria.iter().any(|criterion| {
        criterion.reason_code == Some(WorkReasonCode::WrongRunOrHash)
            && criterion.evidence.iter().any(|reference| {
                matches!(
                    inputs.evidence.get(&reference.event_id),
                    Some(WorkEvidenceFact::ActionResult {
                        action_name,
                        definition_hash,
                        ..
                    }) if ran_another_commits_definition(action_name, definition_hash, inputs)
                )
            })
    });
    let (coverage_complete, coverage_reason_code, coverage_reason) = coverage(
        state,
        &body.plan_ref,
        plan_resolved,
        ran_other_definition,
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
    // When the plan has a `git-ref` criterion, only that criterion's evidence
    // says which commit was delivered: the candidate is its commit, and
    // `null` until it is covered (A6 ruling — the README wins; nothing else
    // may nominate a delivered revision nobody observed).
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
        &declared.plan_ref,
        inputs,
    );
    WorkCriterionProjection {
        criterion_id: criterion_id.to_owned(),
        proof: Some(criterion.proof.clone()),
        status: outcome.status,
        assignment_refs,
        evidence: refs,
        // A binding whose evidence the team contract excludes names no
        // revision this projection will repeat: nothing canonical claims that
        // commit, and printing it would lend the claim standing.
        artifact_commit: (!matches!(
            outcome.reason_code,
            Some(WorkReasonCode::ReportNotCanonical)
                | Some(WorkReasonCode::DispositionNotCanonical)
        ))
        .then_some(artifact_commit),
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
    plan_ref: &ProjectWorkPlanRef,
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
            evaluate_action(refs, artifact_commit, name, step, plan_ref, inputs)
        }
        PlanProof::GitRef => evaluate_git_ref(refs, artifact_commit, plan, inputs),
    }
}

/// `review` — an approving 44244 disposition the **canonical team
/// projection includes**, on a report that projection also includes, signed
/// by that assignment's assignee, for an assignment bound to this criterion,
/// at the binding's artifact commit.
///
/// The precedence below is the contract's, step for step
/// (`conformance/project-work/README.md` § (c) "The three proof predicates"),
/// so two implementations name the same reason for the same fact:
/// (1) evidence missing, (2) report not canonical, (3) disposition signer not
/// `may_lead`, (4) disposition not included, (5) not an approval,
/// (6) revision mismatch.
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
            report_ref,
            ..
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
        // (1) The report the ruling is about must have been supplied.
        let Some(WorkEvidenceFact::Report {
            event_id: report_id,
            signer: report_signer,
            assignment_ref: report_assignment,
            head_sha,
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
        // (2) The report must be one the team contract admits: included in
        // the canonical projection, signed by its assignment's assignee, and
        // answering an assignment bound to this criterion. A lead binding a
        // channel peer's well-formed report about somebody else's assignment
        // changes nothing (A5 decision 23).
        if let Some(reason) = report_not_canonical(
            report_id,
            report_signer,
            report_assignment,
            assignment_refs,
            inputs,
        ) {
            last = Some(Outcome::open(WorkReasonCode::ReportNotCanonical, reason));
            continue;
        }
        // (3) The ruling's signer.
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
        // (4) A ruling a later one replaced is history, not a current fact —
        // and an empty projection proves no ruling current (A7.4).
        if !inputs.team_projection.includes(event_id) {
            last = Some(Outcome::open(
                WorkReasonCode::DispositionNotCanonical,
                disposition_not_canonical_reason(event_id, report_ref, inputs),
            ));
            continue;
        }
        // (5) The decision itself.
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
        // (6) The revision the report is about.
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

/// Why this report is not one the team contract admits, if it is not.
///
/// **Empty means unproved** (A7.4, re-check R5). Every check here is
/// *positive*: the projection must carry the assignment, must include the
/// report, and the criterion must be bound to the assignment the report
/// answers. An absent projection or an unassigned criterion used to waive the
/// predicate, which let a report and an approval that named an assignment
/// nobody holds — both excluded by the team fold — reach `covered`.
///
/// Six branches, one sentence each: a wrong signer is one fact, exclusion is
/// another, and a projection with no records at all is a third. The
/// precedence is the one the fixtures pin: the assignment row first (without
/// it nothing names an assignee), then an empty projection, then the signer,
/// then inclusion, then the criterion's own bindings.
fn report_not_canonical(
    report_id: &str,
    report_signer: &str,
    report_assignment: &str,
    assignment_refs: &[String],
    inputs: &WorkFoldInputs,
) -> Option<String> {
    let projection = &inputs.team_projection;
    let Some(assignee) = projection.assignee(report_assignment) else {
        return Some(format!(
            "report {} answers assignment {}, which the team projection does not carry, so \
             nothing names its assignee",
            short(report_id),
            short(report_assignment)
        ));
    };
    if projection.included_event_ids.is_empty() {
        return Some(format!(
            "report {} cannot be shown canonical: the team projection includes no records at all",
            short(report_id)
        ));
    }
    if assignee != report_signer {
        return Some(format!(
            "report {} is signed by {}, not by assignment {}'s assignee {}",
            short(report_id),
            short(report_signer),
            short(report_assignment),
            short(assignee)
        ));
    }
    if !projection.includes(report_id) {
        return Some(format!(
            "report {} is not in the team projection, so nothing here says it answers \
             assignment {}",
            short(report_id),
            short(report_assignment)
        ));
    }
    if assignment_refs.is_empty() {
        return Some(format!(
            "report {} answers assignment {}, and no assignment is bound to this criterion \
             under this declaration",
            short(report_id),
            short(report_assignment)
        ));
    }
    if !assignment_refs.iter().any(|id| id == report_assignment) {
        return Some(format!(
            "report {} answers assignment {}, which this criterion is not bound to",
            short(report_id),
            short(report_assignment)
        ));
    }
    None
}

/// Name the ruling that replaced this one, when the projection carries it.
fn disposition_not_canonical_reason(
    event_id: &str,
    report_ref: &str,
    inputs: &WorkFoldInputs,
) -> String {
    let replacement = inputs
        .evidence
        .values()
        .filter_map(|fact| match fact {
            WorkEvidenceFact::Verdict {
                event_id: other,
                subtype,
                report_ref: other_report,
                ..
            } if subtype == "disposition"
                && other_report == report_ref
                && other != event_id
                && inputs.team_projection.includes(other) =>
            {
                Some(other.as_str())
            }
            _ => None,
        })
        .next();
    match replacement {
        Some(replacement) => format!(
            "disposition {} is not in the team projection: it was superseded by {}",
            short(event_id),
            short(replacement)
        ),
        None => format!(
            "disposition {} is not in the team projection, so it is not a current ruling",
            short(event_id)
        ),
    }
}

/// `action` — a clean, exit-0 host result for the definition the caller
/// compiled at the plan commit, on the artifact commit.
fn evaluate_action(
    refs: &[ProjectWorkEvidenceRef],
    artifact_commit: &str,
    name: &str,
    step: &str,
    plan_ref: &ProjectWorkPlanRef,
    inputs: &WorkFoldInputs,
) -> Outcome {
    // Only the definition compiled at **this declaration's** plan commit. The
    // same action name at another commit is another definition, and letting
    // it answer here is what made an amended head's correct evidence fail
    // while the superseded plan's evidence passed (A5 decision 24).
    let key = crate::project_work_fold::action_definition_key(
        &plan_ref.repository,
        &plan_ref.commit,
        name,
    );
    let Some(definition) = inputs.action_definitions.get(&key) else {
        return Outcome::unknown(
            WorkReasonCode::EvidenceUnavailable,
            format!(
                "the {name} action was not compiled at this declaration's plan commit {}, \
                 so nothing here says what its result proves",
                short_commit(&plan_ref.commit)
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
                    "host result {} was echoed by {}, not the relay's self key; the {name} \
                     definition compiled at this declaration's plan commit {} is unproved",
                    short(event_id),
                    short(echo_signer),
                    short_commit(&plan_ref.commit)
                ),
            ));
            continue;
        }
        if definition_hash != &definition.definition_hash || action_name != name {
            last = Some(Outcome::open(
                WorkReasonCode::WrongRunOrHash,
                // Each branch says what actually failed and then names the
                // commit whose definition stays unproved (A6, narrowed by
                // A7.4): naming the plan commit never licenses claiming a
                // hash mismatch nobody observed.
                format!(
                    "host result {} ran definition hash {}, not the {name} definition compiled \
                     at this declaration's plan commit {} ({})",
                    short(event_id),
                    short(definition_hash),
                    short_commit(&plan_ref.commit),
                    short(&definition.definition_hash)
                ),
            ));
            continue;
        }
        if step_id != step || !definition.steps.iter().any(|known| known == step) {
            last = Some(Outcome::open(
                WorkReasonCode::WrongRunOrHash,
                format!(
                    "host result {} ran step {step_id}, not {step} of the {name} definition \
                     compiled at this declaration's plan commit {}",
                    short(event_id),
                    short_commit(&plan_ref.commit)
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
            format!(
                "no host result for {name}/{step} of the definition compiled at this \
                 declaration's plan commit {} is bound",
                short_commit(&plan_ref.commit)
            ),
        )
    })
}

/// Whether the hash a run executed is this action's definition at **another**
/// plan commit.
///
/// The criterion's reason says one thing either way (A6); the declaration's
/// sentence still distinguishes a provenance mismatch from an unrelated
/// definition, because they are different facts about what happened.
fn ran_another_commits_definition(name: &str, hash: &str, inputs: &WorkFoldInputs) -> bool {
    let suffix = format!("#{name}");
    inputs
        .action_definitions
        .iter()
        .any(|(key, definition)| key.ends_with(&suffix) && definition.definition_hash == hash)
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
