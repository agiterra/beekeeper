//! Gather the verified facts one [`crate::work_brief`] is rendered from, and
//! put a copy where the seat can re-read it.
//!
//! Split from `work_brief.rs` on purpose: that module is pure and its golden
//! tests pin exact bytes, while this one is all relay reads, git reads and
//! host-store reads. Keeping the render free of I/O is what lets the render be
//! tested without a relay.
//!
//! **Every read here is best-effort and every failure is a sentence.** A brief
//! is a saving, never a gate: a relay that did not answer, a plan blob this
//! host does not have, a work record set that could not be paged — each of
//! those produces a brief with that section replaced by its reason, and the
//! turn opens exactly when it would have opened anyway. Nothing in this module
//! may refuse, defer or delay a turn.
//!
//! Plan amendment A3.1: the fold's inputs are assembled by
//! `buzz_core::project_work_inputs::assemble_fold_inputs` and by nothing else.
//! This module fetches events and blobs and hands them over; it decodes no
//! coverage of its own.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use buzz_core::coding_session_team_transaction::{
    validate_coding_session_team_transaction_envelope, CodingSessionTeamAssignment,
    CodingSessionTeamDecisionAnswer, CodingSessionTeamDecisionChoice,
    CodingSessionTeamDecisionRequest, CodingSessionTeamTransactionBody,
};
use buzz_core::kind::{KIND_CODING_SESSION_GOAL, KIND_PROJECT_WORK_RECORD};
use buzz_core::project_plan::parse_plan;
use buzz_core::project_work::{
    decode_project_work_content, ProjectWorkBody, ProjectWorkEvent, ProjectWorkPlanRef,
};
use buzz_core::project_work_fold::{fold_work, WorkDeclarationState};
use buzz_core::project_work_inputs::{assemble_fold_inputs, RawAuthorityContext, RawWorkInputs};
use nostr::Event;

use crate::agents_plan_blob::read_blob_at_commit;
use crate::work_brief::{
    assemble_work_brief, AssignmentFacts, CommandFacts, ContractFacts, CriterionExcerpt,
    DecisionFacts, DecisionSummary, InputEstablishment, PermissionFacts, RuntimeFacts,
    WorkBriefInputs, WORK_BRIEF_FILE_NAME,
};

/// Everything the caller already holds, handed over rather than looked up.
///
/// Deliberately an input set: this module never resolves a seat's working
/// directory, its seat bundle root or its agents grant on its own. The
/// provider holds all three as facts it wrote, and a second resolution here
/// could disagree with the one the fence used.
pub(crate) struct WorkBriefRequest<'a> {
    /// Relay read seam.
    pub rest: &'a buzz_acp::relay::RestClient,
    /// The relay's own verified key, for the snapshot's authority chain.
    pub relay_self: &'a str,
    /// Channel, session and genesis this turn belongs to.
    pub scope: &'a crate::team_wake::WakeScope,
    /// The assignment event id the turn's pointer names.
    pub operation_id: &'a str,
    /// The 44220 command id carrying this turn.
    pub command_id: &'a str,
    /// The seat's actor pubkey.
    pub actor: &'a str,
    /// The seat's role slug.
    pub role: &'a str,
    /// The project the session belongs to, when the record names one.
    pub project_ref: Option<&'a str>,
    /// The host's `projects.json`, which records the agents-repository clone.
    pub projects_file: Option<&'a Path>,
    /// The provider's state directory: the seat bundle and the host store
    /// both hang off it.
    pub state_dir: &'a Path,
    /// The execution's session id, which names its bundle.
    pub session_id: &'a str,
    /// The seat's worktree, as the host recorded it.
    pub cwd: Option<PathBuf>,
    /// (f), resolved by the caller from its own session record.
    pub runtime: RuntimeFacts,
}

/// Assemble and render one work brief, or `None` when this turn is not an
/// assignment turn this host can speak about.
///
/// `None` is the honest answer in exactly two cases: the pointer does not
/// resolve to a canonical assignment bound to this seat, role and command, or
/// the relay could not be read at all. Both mean the host has nothing
/// *verified* to say, and an unverified brief would be worse than none.
pub(crate) async fn collect_work_brief(request: WorkBriefRequest<'_>) -> Option<String> {
    let snapshot = match crate::team_wake::fetch_verified_snapshot(
        request.rest,
        request.relay_self,
        request.scope,
    )
    .await
    {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::debug!(
                target: "csp::work_brief",
                %error,
                "no work brief: the session's verified facts were unavailable"
            );
            return None;
        }
    };
    let assignment = canonical_assignment(
        &snapshot.team_events,
        request.operation_id,
        request.command_id,
        request.actor,
        request.role,
    )?;

    let establishment = establishment_for(
        request.state_dir,
        request.operation_id,
        assignment.base_sha.as_deref(),
        request.role,
    );
    let (contract, goal_ref, goal_first_line) = contract_for(&request, &snapshot).await;
    let decisions = answered_decisions(&snapshot.team_events, request.operation_id);
    let permissions = agents_grant(request.cwd.as_deref(), request.role);
    let report_ref = report_under_review(&snapshot, assignment.base_sha.as_deref(), request.role);

    let brief_path = crate::session::seat_bundle_dir(request.state_dir, request.session_id)
        .join(WORK_BRIEF_FILE_NAME);
    let inputs = WorkBriefInputs {
        assignment: AssignmentFacts {
            assignment_ref: request.operation_id.to_owned(),
            role: request.role.to_owned(),
            objective: assignment.objective.clone(),
            brief: assignment.brief.clone(),
            branch: assignment.branch.clone(),
            base_sha: assignment.base_sha.clone(),
            file_ownership: assignment.file_ownership.clone(),
            acceptance_steps: assignment.acceptance_steps.clone(),
            worktree: request.cwd.as_ref().map(|path| path.display().to_string()),
            establishment,
        },
        commands: CommandFacts {
            channel: request.scope.channel_ref.to_string(),
            session_ref: request.scope.session_ref.clone(),
            genesis_ref: request.scope.genesis_ref.clone(),
            assignment_ref: request.operation_id.to_owned(),
            role: request.role.to_owned(),
            report_ref,
        },
        contract,
        decisions: DecisionFacts {
            goal_ref,
            goal_first_line,
            decisions,
        },
        permissions,
        runtime: request.runtime,
        brief_path: Some(brief_path.display().to_string()),
    };
    let text = assemble_work_brief(&inputs).render();
    write_brief_copy(&brief_path, &text);
    Some(text)
}

/// The assignment behind the pointer, proven canonical and bound to this seat.
///
/// The same sequence [`crate::verification_input::turn_input_requirement`]
/// keeps — fold membership, envelope validation, then the actor/role/command
/// binding — minus its role gate, because a builder's brief is worth exactly
/// as much as a verifier's and that gate exists for a different question (what
/// a turn *claims about a commit*). Nothing here decides whether a turn opens.
fn canonical_assignment(
    events: &[Event],
    operation_id: &str,
    command_id: &str,
    actor: &str,
    role: &str,
) -> Option<CodingSessionTeamAssignment> {
    let event = events
        .iter()
        .find(|event| event.id.to_hex() == operation_id)?;
    let payload = validate_coding_session_team_transaction_envelope(event).ok()?;
    let CodingSessionTeamTransactionBody::Assignment(assignment) = payload.body else {
        return None;
    };
    if payload.delivery_command_id.as_deref() != Some(command_id)
        || !assignment.assignee_actor.eq_ignore_ascii_case(actor)
        || assignment.assignee_role != role
    {
        return None;
    }
    Some(assignment)
}

/// (e), from what this host can witness rather than from a grant record.
///
/// The seat's one-shot entry in `actor-seats.json` is deleted the moment the
/// adapter is spawned, so the grant it carried is gone by the time a turn
/// opens. Two facts survive and are checked here instead: whether the sibling
/// clone the host cuts beside a granted seat's worktree
/// (`buzz_core::model_registry_source::seat_agents_clone_path`) is actually on
/// disk, and what `team.yml` **in that clone** says this role's
/// `workspace.agents_repo` is — the same file the desktop read when it decided
/// to cut the clone at all. A clone that is not there means no grant, which is
/// the honest reading: a seat cannot read a repository that is not beside it.
fn agents_grant(cwd: Option<&Path>, role: &str) -> PermissionFacts {
    // A seated execution holds its own key and commits under it; the relay's
    // push gate, never this host, decides whether the push lands, and the
    // rendered sentence says exactly that.
    let may_push = true;
    let Some(clone) = cwd
        .and_then(buzz_core::model_registry_source::seat_agents_clone_path)
        .filter(|path| path.join(".git").exists())
    else {
        return PermissionFacts {
            agents_access: "none".to_owned(),
            agents_path: None,
            may_push,
        };
    };
    let access = match buzz_persona::team::load_team(&clone) {
        Ok(Some(manifest)) => match manifest.role(role).workspace.agents_repo {
            buzz_persona::team::AgentsRepoAccess::Write => "write",
            buzz_persona::team::AgentsRepoAccess::Read => "read",
            buzz_persona::team::AgentsRepoAccess::None => "read",
        },
        // The clone is there; `team.yml` is not readable from it. A clone the
        // host cut is at least readable, and claiming `write` on a file we
        // could not read would be the comfortable guess.
        _ => "read",
    };
    PermissionFacts {
        agents_access: access.to_owned(),
        agents_path: Some(clone.display().to_string()),
        may_push,
    }
}

/// What lane 202 did about this assignment's commit on this computer.
fn establishment_for(
    state_dir: &Path,
    assignment_ref: &str,
    base_sha: Option<&str>,
    role: &str,
) -> InputEstablishment {
    use crate::assignment_inputs as inputs;

    if base_sha.is_none() || !crate::verification_input::role_verifies_a_commit(role) {
        return InputEstablishment::NotRequired;
    }
    let Some(store) = inputs::host_store_from_pointer(state_dir) else {
        return InputEstablishment::Unrecorded;
    };
    let record = store
        .with_records(|records, _| inputs::assignment_input(records, assignment_ref).cloned())
        .ok()
        .flatten();
    match record {
        Some(record) if record.is_established() => match record.commit.clone() {
            Some(commit) => InputEstablishment::Established { commit },
            None => InputEstablishment::Unrecorded,
        },
        Some(record) if record.is_pending() => InputEstablishment::Unrecorded,
        Some(record) => InputEstablishment::Failed {
            words: record
                .message
                .unwrap_or_else(|| format!("the attempt settled as {}", record.outcome)),
        },
        None => InputEstablishment::Unrecorded,
    }
}

/// (c), plus the goal (d) needs, both derived from one assembled input set.
async fn contract_for(
    request: &WorkBriefRequest<'_>,
    snapshot: &crate::team_wake::VerifiedWakeSnapshot,
) -> (ContractFacts, Option<String>, Option<String>) {
    let work_events = match partition(request, KIND_PROJECT_WORK_RECORD).await {
        Ok(events) => events,
        Err(reason) => return (ContractFacts::Unread { reason }, None, None),
    };
    let goal_events = partition(request, KIND_CODING_SESSION_GOAL)
        .await
        .unwrap_or_default();
    let goal_text: BTreeMap<String, String> = goal_events
        .iter()
        .map(|event| (event.id.clone(), event.content.clone()))
        .collect();

    // Plan blobs, read at each declaration's own pinned commit. A declaration
    // whose blob this host does not have is simply absent from the map, and
    // the fold reports its criteria `unknown` rather than `open`.
    let mut plan_blobs: BTreeMap<(String, String, String), String> = BTreeMap::new();
    let mut blob_reason: Option<String> = None;
    let agents = request
        .project_ref
        .and_then(|project| agents_record(request.projects_file, project));
    for plan_ref in plan_refs(&work_events) {
        let key = (
            plan_ref.repository.clone(),
            plan_ref.commit.clone(),
            plan_ref.path.clone(),
        );
        if plan_blobs.contains_key(&key) {
            continue;
        }
        let Some(record) = agents.as_ref() else {
            blob_reason.get_or_insert_with(|| {
                "this host has no clone of the project's agents repository recorded".to_owned()
            });
            continue;
        };
        match read_blob_at_commit(record, &plan_ref.commit, &plan_ref.path).await {
            Ok(text) => {
                plan_blobs.insert(key, text);
            }
            Err(error) => {
                blob_reason.get_or_insert_with(|| error.to_string());
            }
        }
    }

    let raw = RawWorkInputs {
        work_events,
        goal_events,
        authority: RawAuthorityContext {
            founder_pubkey: Some(snapshot.founder_pubkey.clone()),
            ..RawAuthorityContext::default()
        },
        plan_blobs: plan_blobs.clone(),
        session_ref: Some(request.scope.session_ref.clone()),
        project_ref: request.project_ref.map(str::to_owned),
        ..RawWorkInputs::default()
    };
    let inputs = match assemble_fold_inputs(raw) {
        Ok(inputs) => inputs,
        Err(refusal) => {
            return (
                ContractFacts::Unread {
                    reason: refusal.to_string(),
                },
                None,
                None,
            )
        }
    };
    let goal_ref = inputs.current_goal_ref.clone();
    let goal_first_line = goal_ref
        .as_deref()
        .and_then(|id| goal_text.get(id))
        .map(|text| first_line(text));

    let projection = fold_work(&inputs);
    let bound = projection.declarations.iter().find(|declaration| {
        declaration.state == WorkDeclarationState::Head
            && declaration.criteria.iter().any(|criterion| {
                criterion
                    .assignment_refs
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(request.operation_id))
            })
    });
    let Some(declaration) = bound else {
        return (ContractFacts::None, goal_ref, goal_first_line);
    };
    let ids: Vec<String> = declaration
        .criteria
        .iter()
        .filter(|criterion| {
            criterion
                .assignment_refs
                .iter()
                .any(|id| id.eq_ignore_ascii_case(request.operation_id))
        })
        .map(|criterion| criterion.criterion_id.clone())
        .collect();
    let key = (
        declaration.plan_ref.repository.clone(),
        declaration.plan_ref.commit.clone(),
        declaration.plan_ref.path.clone(),
    );
    let plan = plan_blobs
        .get(&key)
        .and_then(|text| parse_plan(text.as_bytes()).ok());
    let Some(plan) = plan else {
        return (
            ContractFacts::PlanUnreadable {
                declaration_ref: declaration.declaration_ref.clone(),
                reason: blob_reason.unwrap_or_else(|| {
                    "the blob at that commit is not a beekeeper-plan/v1 file".to_owned()
                }),
                criterion_ids: ids,
            },
            goal_ref,
            goal_first_line,
        );
    };
    let provenance = plan_provenance(&declaration.plan_ref);
    let criteria = plan
        .criteria
        .iter()
        .filter(|criterion| ids.iter().any(|id| id == &criterion.id))
        .map(|criterion| CriterionExcerpt {
            id: criterion.id.clone(),
            accept: criterion.accept.clone(),
            provenance: format!("{provenance}#{}", criterion.id),
            proof: Some(criterion.proof.clone()),
        })
        .collect();
    (
        ContractFacts::Declared {
            declaration_ref: declaration.declaration_ref.clone(),
            plan_provenance: provenance,
            criteria,
        },
        goal_ref,
        goal_first_line,
    )
}

/// `<repository>@<commit-12>:<path>` — a coordinate a seat can re-read from.
fn plan_provenance(plan_ref: &ProjectWorkPlanRef) -> String {
    format!(
        "{}@{}:{}",
        plan_ref.repository,
        &plan_ref.commit[..plan_ref.commit.len().min(12)],
        plan_ref.path
    )
}

/// Every distinct plan the session's declarations pin.
fn plan_refs(events: &[ProjectWorkEvent]) -> Vec<ProjectWorkPlanRef> {
    events
        .iter()
        .filter_map(|event| {
            let payload = decode_project_work_content(&event.content).ok()?;
            match payload.body {
                ProjectWorkBody::Declared(declared) => Some(declared.plan_ref),
                _ => None,
            }
        })
        .collect()
}

/// One complete kind partition of the session's channel, as the fold's event
/// type, narrowed to this session by its `d` tag.
async fn partition(
    request: &WorkBriefRequest<'_>,
    kind: u32,
) -> Result<Vec<ProjectWorkEvent>, String> {
    let events = crate::context_projector::query_complete_kind_partition(
        request.rest,
        request.scope.channel_ref,
        kind,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(events
        .into_iter()
        .filter_map(|event| {
            let converted = ProjectWorkEvent {
                id: event.id.to_hex(),
                pubkey: event.pubkey.to_hex(),
                created_at: event.created_at.as_secs(),
                kind,
                tags: event.tags.iter().map(|tag| tag.clone().to_vec()).collect(),
                content: event.content.clone(),
            };
            (converted.tag_value("d") == Some(request.scope.session_ref.as_str()))
                .then_some(converted)
        })
        .collect())
}

/// Answered decisions whose request named this assignment in `blocks`.
fn answered_decisions(events: &[Event], assignment_ref: &str) -> Vec<DecisionSummary> {
    let mut requests: BTreeMap<String, CodingSessionTeamDecisionRequest> = BTreeMap::new();
    let mut answers: Vec<(String, CodingSessionTeamDecisionAnswer)> = Vec::new();
    for event in events {
        let Ok(payload) = validate_coding_session_team_transaction_envelope(event) else {
            continue;
        };
        match payload.body {
            CodingSessionTeamTransactionBody::DecisionRequest(request) => {
                requests.insert(event.id.to_hex(), request);
            }
            CodingSessionTeamTransactionBody::DecisionAnswer(answer) => {
                answers.push((event.id.to_hex(), answer));
            }
            _ => {}
        }
    }
    let mut summaries: Vec<DecisionSummary> = answers
        .into_iter()
        .filter_map(|(event_id, answer)| {
            let request = requests.get(&answer.request_ref)?;
            if !request
                .blocks
                .iter()
                .any(|id| id.eq_ignore_ascii_case(assignment_ref))
            {
                return None;
            }
            let chosen = match &answer.choice {
                CodingSessionTeamDecisionChoice::Index(index) => request
                    .options
                    .get(*index as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("option {index}")),
                CodingSessionTeamDecisionChoice::Text(text) => text.clone(),
            };
            Some(DecisionSummary {
                event_id,
                summary: format!("{} — answered: {}", first_line(&request.question), chosen),
            })
        })
        .collect();
    summaries.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    summaries
}

/// The report a verifier's assignment is about: the one whose `headSha` is
/// this assignment's base.
///
/// Derived, and `None` rather than guessed. An assignment carries no report
/// reference — the lead's `--verifies <report>` fills `baseSha` and nothing
/// else — so this is the only honest reconstruction, and it is right exactly
/// when one included report claims that head.
fn report_under_review(
    snapshot: &crate::team_wake::VerifiedWakeSnapshot,
    base_sha: Option<&str>,
    role: &str,
) -> Option<String> {
    if !role.eq_ignore_ascii_case("verifier") {
        return None;
    }
    let base = base_sha?.to_ascii_lowercase();
    let included: std::collections::BTreeSet<String> = snapshot
        .included_reports
        .iter()
        .map(|report| report.event_id.clone())
        .collect();
    let mut matches = snapshot.team_events.iter().filter_map(|event| {
        let id = event.id.to_hex();
        if !included.contains(&id) {
            return None;
        }
        let payload = validate_coding_session_team_transaction_envelope(event).ok()?;
        let CodingSessionTeamTransactionBody::Report(report) = payload.body else {
            return None;
        };
        report
            .head_sha
            .as_deref()
            .is_some_and(|head| head.eq_ignore_ascii_case(&base))
            .then_some(id)
    });
    let first = matches.next()?;
    // Two included reports claiming the same head is not a fact about which
    // one this assignment is for, so nothing is named.
    matches.next().is_none().then_some(first)
}

/// The first non-empty line of some text, bounded.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.len() <= 200 {
        return line.to_owned();
    }
    let mut end = 200;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &line[..end])
}

/// This host's clone of the project's agents repository, when it recorded one.
fn agents_record(
    projects_file: Option<&Path>,
    project_ref: &str,
) -> Option<crate::agents_checkout::AgentsRepoRecord> {
    crate::commands::ProjectsFile::load(projects_file)
        .agents_repos
        .get(project_ref)
        .cloned()
}

/// Put the same bytes in the seat's own bundle so the brief can be re-read
/// without scrolling a transcript.
///
/// Failure is logged and nothing else: the brief in the turn is the delivery,
/// and a seat whose disk is full still gets its work.
fn write_brief_copy(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::debug!(target: "csp::work_brief", %error, "the seat bundle directory could not be created for the work brief");
            return;
        }
    }
    if let Err(error) = std::fs::write(path, text) {
        tracing::debug!(target: "csp::work_brief", %error, "the work brief could not be written into the seat bundle");
    }
}
