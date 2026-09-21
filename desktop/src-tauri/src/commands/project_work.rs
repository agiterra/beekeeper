//! `project_work_coverage` — the session's work coverage, folded natively.
//!
//! The division of labour is the team-transaction fold's, for the same
//! reasons: **the frontend owns the subscription, the native side owns the
//! fold.** TypeScript fetches the kind:44249 records and the events they
//! reference and hands them over whole; `buzz-core` establishes the facts,
//! folds them and writes every sentence a person reads. There is no
//! TypeScript fold and there is no second assembler — the CLI
//! (`bee sessions work status`), this command and the provider all go through
//! `buzz_core::project_work_inputs::assemble_fold_inputs`, so the app and the
//! CLI cannot disagree about a criterion while both sound confident.
//!
//! Two rules this file exists to hold:
//!
//! - **Plan bytes are read at the declaration's pinned commit, with
//!   `git show <commit>:<path>`, from the host's own agents cache** — never
//!   from a seat's working copy and never from a fetched tip. A seat's tree
//!   is mutable and mid-edit; a contract read out of one is not the contract
//!   that was adopted.
//! - **An input that cannot be read is left out, never guessed at.** The fold
//!   then reports the criterion that needed it as `unknown` with a reason,
//!   which is the honest answer; this command additionally returns the list
//!   of plans it could not read, so a surface can say *which* plan and *why*
//!   rather than showing an empty contract.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use buzz_core_pkg::project_plan::{parse_plan, Plan, PlanProof};
use buzz_core_pkg::project_work::{ProjectWorkDeclared, ProjectWorkEvent, ProjectWorkPlanRef};
use buzz_core_pkg::project_work_fold::{fold_work, WorkActionDefinition, WorkProjection};
use buzz_core_pkg::project_work_inputs::{
    assemble_fold_inputs, RawAuthorityContext, RawWorkInputs,
};

/// Wire-schema string this command accepts, echoed back on every answer.
pub const PROJECT_WORK_REQUEST_SCHEMA: &str = "buzz-project-work-request/v1";

/// Wire-schema string of the response envelope.
pub const PROJECT_WORK_RESPONSE_SCHEMA: &str = "buzz-project-work-response/v1";

/// One active seat, as the caller's accepted kind:44228 projection holds it.
///
/// The desktop already computes this projection for the team-transaction
/// fold — from relay *acceptance receipts*, not from the transitions alone —
/// and it is handed over rather than recomputed here, because recomputing it
/// without the receipts would admit a transition the relay never accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectWorkSeatInput {
    /// Lowercase 64-hex actor pubkey.
    pub actor_pubkey: String,
    /// Role slug the seat was granted for.
    pub role: String,
}

/// One active operator grant, from the same accepted projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectWorkGrantInput {
    /// Lowercase 64-hex actor pubkey.
    pub actor_pubkey: String,
    /// Event id of the transition that granted it.
    pub grant_event_ref: String,
    /// Whether the grant carries steering, which `may_lead` admits.
    pub may_steer: bool,
}

/// Everything the frontend fetched, before anything is derived from it.
///
/// `deny_unknown_fields`: a caller that sends a key this command does not
/// know is refused by name rather than having it silently ignored, which is
/// the only way a renamed field fails loudly instead of quietly dropping
/// evidence.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectWorkRequest {
    /// Exactly [`PROJECT_WORK_REQUEST_SCHEMA`].
    pub schema: String,
    /// The session this projection is about, canonical lowercase uuid.
    pub session_ref: String,
    /// The project, `30621:<64-hex owner>:<d>`.
    pub project_ref: String,
    /// Lowercase 64-hex founder pubkey, off the session record.
    pub founder_pubkey: String,
    /// The relay's NIP-11 `self` key. `None` is a disclosed non-answer: ref
    /// observations and host echoes then read `unknown` rather than believed.
    #[serde(default)]
    pub relay_self_key: Option<String>,
    #[serde(default)]
    pub active_seats: Vec<ProjectWorkSeatInput>,
    #[serde(default)]
    pub active_grants: Vec<ProjectWorkGrantInput>,
    /// Signed kind:44249 work records for this session.
    pub work_events: Vec<ProjectWorkEvent>,
    /// Signed kind:44244 team transactions for this session, **with their
    /// signatures**.
    ///
    /// The assembler folds them with the canonical 44244 fold, which verifies
    /// what it judges, so these cross the boundary as full signed events
    /// rather than the plain shape the other lists use (A5 decision 23).
    #[serde(default)]
    pub team_events: Vec<nostr::Event>,
    /// The session's channel uuid, needed to scope that fold.
    #[serde(default)]
    pub channel_ref: Option<String>,
    /// The session genesis event id, for the same reason.
    #[serde(default)]
    pub genesis_ref: Option<String>,
    /// Signed kind:44227 goal events for this session.
    #[serde(default)]
    pub goal_events: Vec<ProjectWorkEvent>,
    /// Signed kind:46023 / 46014 / 46013 host-step events for the channel, in
    /// one list. They are split by kind here rather than by the caller, so a
    /// frontend cannot mis-sort a result into the echo list.
    #[serde(default)]
    pub host_events: Vec<ProjectWorkEvent>,
    /// Signed kind:30618 ref states for the project's repositories.
    #[serde(default)]
    pub ref_states: Vec<ProjectWorkEvent>,
}

/// A plan a declaration pinned that this host could not read, with its reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkUnreadablePlan {
    /// The repository coordinate the declaration named.
    pub repository: String,
    /// The pinned commit.
    pub commit: String,
    /// The path under the repository root.
    pub path: String,
    /// `plan_unreadable`, or `actions_uncompilable` for a plan whose
    /// `action` criteria could not be compiled at the same commit.
    pub reason_code: String,
    /// What failed, in the failing command's own words.
    pub reason: String,
}

/// The coverage projection, verbatim, plus what this read could not see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkResponse {
    /// Exactly [`PROJECT_WORK_RESPONSE_SCHEMA`].
    pub schema: String,
    /// Which implementation folded it. Always `buzz-core`.
    pub implementation: String,
    /// The fold's own output, unmodified.
    pub coverage: WorkProjection,
    /// Plans this host could not read at their commit. Empty is a real
    /// answer; a non-empty list is why some criterion reads `unknown`.
    pub unreadable_plans: Vec<ProjectWorkUnreadablePlan>,
    /// Whether an agents checkout was available at all.
    pub agents_repo_read: bool,
}

/// The bytes of `path` at `commit`, read with `git show`.
///
/// Read-only by construction: `git show` neither checks anything out nor
/// touches the index, so this can run against the host's live agents cache
/// without disturbing a sync in flight.
fn blob_at_commit(dir: &str, commit: &str, path: &str) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("show")
        .arg(format!("{commit}:{path}"))
        .output()
        .map_err(|error| format!("git show failed to start: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("{path} at {commit} is not valid UTF-8: {error}"))
}

/// Compile the actions a plan's criteria name, from `actions.yml` at the same
/// commit, with the publication compiler.
///
/// The hash comes from the compiler, never from an event: evidence that
/// nominated its own expected hash would prove nothing.
fn compile_plan_actions(
    plan: &Plan,
    project_ref: &str,
    actions_yml: Option<&str>,
) -> Result<BTreeMap<String, WorkActionDefinition>, String> {
    let mut compiled = BTreeMap::new();
    let needed: BTreeSet<(&str, &str)> = plan
        .criteria
        .iter()
        .filter_map(|criterion| match &criterion.proof {
            PlanProof::Action { name, step } => Some((name.as_str(), step.as_str())),
            _ => None,
        })
        .collect();
    if needed.is_empty() {
        return Ok(compiled);
    }
    let Some(text) = actions_yml else {
        return Err(format!(
            "this plan's criteria name {} action(s) and actions.yml could not be read at the \
             same commit",
            needed.len()
        ));
    };
    let entries = buzz_workflow_pkg::parse_actions_yml(text, project_ref)
        .map_err(|error| format!("actions.yml: {error}"))?;
    for (name, step) in needed {
        let Some(entry) = entries.iter().find(|entry| entry.name == name) else {
            return Err(format!(
                "actions.yml at this commit defines no action named {name:?}"
            ));
        };
        let steps: Vec<String> = entry.def.steps.iter().map(|item| item.id.clone()).collect();
        if !steps.iter().any(|known| known == step) {
            return Err(format!(
                "action {name:?} defines no step {step:?} (it defines {})",
                steps.join(", ")
            ));
        }
        compiled.insert(
            name.to_owned(),
            WorkActionDefinition {
                definition_hash: entry.hash.clone(),
                steps,
            },
        );
    }
    Ok(compiled)
}

/// Every declared body in a record set, in record order.
fn declarations(events: &[ProjectWorkEvent]) -> Vec<ProjectWorkDeclared> {
    events
        .iter()
        .filter_map(|event| {
            let payload: serde_json::Value = serde_json::from_str(&event.content).ok()?;
            serde_json::from_value(payload.get("body")?.clone()).ok()
        })
        .collect()
}

/// `(repository coordinate, commit, path-or-action-name)`, the key the
/// assembler takes plan blobs and compiled definitions under.
type PlanKey = (String, String, String);

/// What one pass over the declarations established.
struct ReadPlans {
    blobs: BTreeMap<PlanKey, String>,
    actions: BTreeMap<PlanKey, WorkActionDefinition>,
    unreadable: Vec<ProjectWorkUnreadablePlan>,
}

/// Read every declaration's plan blob at its own commit, and compile the
/// actions those plans name.
fn read_plans(
    agents_repo_path: Option<&str>,
    project_ref: &str,
    work_events: &[ProjectWorkEvent],
) -> ReadPlans {
    let mut plan_blobs = BTreeMap::new();
    let mut action_definitions = BTreeMap::new();
    let mut unreadable = Vec::new();
    let unreadable_entry =
        |plan_ref: &ProjectWorkPlanRef, code: &str, reason: String| ProjectWorkUnreadablePlan {
            repository: plan_ref.repository.clone(),
            commit: plan_ref.commit.clone(),
            path: plan_ref.path.clone(),
            reason_code: code.to_owned(),
            reason,
        };
    for declared in declarations(work_events) {
        let plan_ref = &declared.plan_ref;
        let key = (
            plan_ref.repository.clone(),
            plan_ref.commit.clone(),
            plan_ref.path.clone(),
        );
        if plan_blobs.contains_key(&key)
            || unreadable.iter().any(|entry: &ProjectWorkUnreadablePlan| {
                entry.repository == key.0 && entry.commit == key.1 && entry.path == key.2
            })
        {
            continue;
        }
        let Some(dir) = agents_repo_path else {
            unreadable.push(unreadable_entry(
                plan_ref,
                "plan_unreadable",
                "this computer has no clone of the project's agents repository, so no plan can \
                 be read at its commit"
                    .to_owned(),
            ));
            continue;
        };
        let bytes = match blob_at_commit(dir, &plan_ref.commit, &plan_ref.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                unreadable.push(unreadable_entry(plan_ref, "plan_unreadable", error));
                continue;
            }
        };
        if let Ok(plan) = parse_plan(bytes.as_bytes()) {
            let actions_yml =
                blob_at_commit(dir, &plan_ref.commit, buzz_workflow_pkg::ACTIONS_YML).ok();
            match compile_plan_actions(&plan, project_ref, actions_yml.as_deref()) {
                Ok(compiled) => {
                    for (name, definition) in compiled {
                        action_definitions.insert(
                            (plan_ref.repository.clone(), plan_ref.commit.clone(), name),
                            definition,
                        );
                    }
                }
                // The plan itself was read; only its actions were not. The
                // blob still goes in, so the criteria that need no action are
                // judged; those that do read `unknown`.
                Err(error) => {
                    unreadable.push(unreadable_entry(plan_ref, "actions_uncompilable", error))
                }
            }
        }
        plan_blobs.insert(key, bytes);
    }
    ReadPlans {
        blobs: plan_blobs,
        actions: action_definitions,
        unreadable,
    }
}

/// Assemble and fold one session's work coverage. Pure but for `git show`.
///
/// `agents_repo_path` is resolved by the caller from **this host's** workdir
/// record, never supplied by the frontend: a path that crossed the boundary
/// could be a seat's worktree, and a contract read out of a mutable,
/// mid-edit tree is not the contract that was adopted.
pub(crate) fn project_work_coverage_inner(
    request: ProjectWorkRequest,
    agents_repo_path: Option<String>,
) -> Result<ProjectWorkResponse, String> {
    if request.schema != PROJECT_WORK_REQUEST_SCHEMA {
        return Err(format!(
            "unsupported project-work request schema {:?}",
            request.schema
        ));
    }
    let plans = read_plans(
        agents_repo_path.as_deref(),
        &request.project_ref,
        &request.work_events,
    );
    let split = |kind: u32| -> Vec<ProjectWorkEvent> {
        request
            .host_events
            .iter()
            .filter(|event| event.kind == kind)
            .cloned()
            .collect()
    };
    let raw = RawWorkInputs {
        work_events: request.work_events.clone(),
        team_events: request.team_events,
        host_results: split(buzz_core_pkg::kind::KIND_HOST_STEP_RESULT),
        host_echoes: split(buzz_core_pkg::kind::KIND_WORKFLOW_HOST_STEP_EXITED),
        host_requests: split(buzz_core_pkg::kind::KIND_WORKFLOW_HOST_STEP_REQUESTED),
        ref_states: request.ref_states,
        goal_events: request.goal_events,
        authority: RawAuthorityContext {
            channel_ref: request.channel_ref,
            genesis_ref: request.genesis_ref,
            genesis_event: None,
            founder_pubkey: Some(request.founder_pubkey),
            active_seats: request
                .active_seats
                .into_iter()
                .map(|seat| buzz_core_pkg::project_work_fold::WorkActiveSeat {
                    actor_pubkey: seat.actor_pubkey,
                    role: seat.role,
                })
                .collect(),
            active_grants: request
                .active_grants
                .into_iter()
                .map(|grant| buzz_core_pkg::project_work_fold::WorkActiveGrant {
                    actor_pubkey: grant.actor_pubkey,
                    grant_event_ref: grant.grant_event_ref,
                    may_steer: grant.may_steer,
                })
                .collect(),
        },
        relay_self_key: request.relay_self_key,
        plan_blobs: plans.blobs,
        action_definitions: plans.actions,
        session_ref: Some(request.session_ref),
        project_ref: Some(request.project_ref),
    };
    let inputs = assemble_fold_inputs(raw).map_err(|refusal| refusal.to_string())?;
    Ok(ProjectWorkResponse {
        schema: PROJECT_WORK_RESPONSE_SCHEMA.into(),
        implementation: "buzz-core".into(),
        coverage: fold_work(&inputs),
        unreadable_plans: plans.unreadable,
        agents_repo_read: agents_repo_path.is_some(),
    })
}

/// Fold one session's work coverage from the events the frontend fetched.
///
/// The agents checkout is this host's own record for the project
/// (`workdir_store`'s `agentsRepos`, spec § 4.11) — the same clone the
/// provider reads `actions.yml` from. When there is none, every criterion
/// reads `unknown` with its reason, which is the honest answer.
#[tauri::command]
pub async fn project_work_coverage(
    app: tauri::AppHandle,
    request: ProjectWorkRequest,
) -> Result<ProjectWorkResponse, String> {
    let agents_repo_path = crate::coding_sessions::workdir_store::agents_repo_path_for_project(
        &app,
        &request.project_ref,
    );
    tauri::async_runtime::spawn_blocking(move || {
        project_work_coverage_inner(request, agents_repo_path)
    })
    .await
    .map_err(|error| format!("project-work fold task failed: {error}"))?
}

#[cfg(test)]
#[path = "project_work_tests.rs"]
mod tests;
