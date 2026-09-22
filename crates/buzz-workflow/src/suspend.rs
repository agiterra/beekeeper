//! Making a suspension durable, then announcing it.
//!
//! A step that cannot finish on the relay — an approval gate, or a
//! `run_on_host` step — parks the run. The order here is the contract: the
//! approval or host-step row is written, then the run's status and full
//! trace, and only then is the relay-signed request published. A client that
//! acts on the request therefore always finds the run in the state the
//! request describes; there is no window in which a grant or a claim can
//! arrive for a run that still says `running`.
//!
//! `finalize_run` does not repeat these writes: a suspended
//! [`ExecutionResult`](crate::executor::ExecutionResult) is already durable
//! when the executor returns it.

use buzz_core::host_step::{HostStepApproval, HostStepRequested, HOST_STEP_SCHEMA};
use buzz_core::tenant::CommunityId;
use buzz_db::workflow::{
    ApprovalStatus, CreateApprovalParams, CreateHostStepParams, RunStatus, WorkflowRecord,
    WorkflowRunRecord,
};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::action_sink::ApprovalRequest;
use crate::error::WorkflowError;
use crate::executor::RunStop;
use crate::schema::{ActionDef, WorkflowDef};
use crate::WorkflowEngine;

/// How long a synthetic approval gate waits before it expires.
pub const SYNTHETIC_APPROVAL_TIMEOUT_SECS: u64 = 24 * 60 * 60;
/// How long the relay accepts a claim for a published host step.
pub const HOST_STEP_CLAIM_WINDOW_SECS: u64 = 24 * 60 * 60;
/// Trace status written for a step parked on approval.
pub const TRACE_AWAITING_APPROVAL: &str = "awaiting_approval";
/// Trace status written for a step handed to a host.
pub const TRACE_REQUESTED_ON_HOST: &str = "requested_on_host";

/// Why the executor stopped short of the next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suspension {
    /// The run waits for a kind:46030 grant.
    Approval {
        /// The gated step.
        step_id: String,
        /// Who may approve, in `check_approver_spec` terms.
        approver_spec: String,
        /// Text shown to the approver.
        message: String,
        /// Seconds until the approval expires.
        timeout_secs: u64,
        /// True when the engine inserted the gate before a `run_on_host` step.
        synthetic: bool,
    },
    /// The run waits for a host's kind:46023 result.
    HostStep {
        /// The step handed to the host.
        step_id: String,
        /// `run_on_host` or `wake_agent`.
        step_kind: String,
        /// The approval that released it, when one was required.
        approval: Option<HostStepApproval>,
        /// The earlier steps' outputs, keyed by step id, for the request.
        inputs: serde_json::Value,
    },
}

impl Suspension {
    /// The id of the step the run is parked on.
    pub fn step_id(&self) -> &str {
        match self {
            Suspension::Approval { step_id, .. } | Suspension::HostStep { step_id, .. } => step_id,
        }
    }
}

/// The step index execution resumes at once the approval bound to
/// `step_index` is granted.
///
/// An approval bound to a `run_on_host` step is the engine's own gate on that
/// step (spec § 5.4): the step has not run yet, so execution resumes **at**
/// it. Any other approval belongs to an authored `request_approval` step,
/// which is complete once granted, so execution resumes after it.
pub fn resume_index_after_approval(def: &WorkflowDef, step_index: usize) -> usize {
    match def.steps.get(step_index) {
        Some(step) if matches!(step.action, ActionDef::RunOnHost { .. }) => step_index,
        _ => step_index + 1,
    }
}

/// Find the granted approval, if any, that releases `step_id` in this run.
pub async fn granted_approval_for_step(
    engine: &WorkflowEngine,
    community_id: CommunityId,
    workflow_id: Uuid,
    run_id: Uuid,
    step_id: &str,
) -> Result<Option<HostStepApproval>, WorkflowError> {
    let approvals = engine
        .db
        .get_run_approvals(community_id, workflow_id, run_id)
        .await?;
    Ok(approvals
        .into_iter()
        .find(|approval| approval.status == ApprovalStatus::Granted && approval.step_id == step_id)
        .map(|approval| HostStepApproval {
            approval_ref: hex::encode(&approval.token),
            scope: "run".into(),
        }))
}

/// Persist a suspension, then publish the request that announces it.
///
/// `trace` is the run's complete trace, prefix and this segment together,
/// **without** an entry for the suspended step; this function appends one.
/// On success the run is `waiting_approval` or `waiting_host` and the
/// request event has been published, and `Ok(None)` is returned. `Ok(Some)`
/// means nothing was written or published because the run is no longer bound
/// to the stored definition; the caller ends the run with that reason. If the publish fails after the rows are
/// written, the run is left parked and the error is returned: the approval
/// is still discoverable through the relay's approvals listing, and a host
/// step row without a request event is visible as exactly that.
pub async fn persist_and_publish(
    engine: &WorkflowEngine,
    community_id: CommunityId,
    run_id: Uuid,
    step_index: usize,
    suspension: &Suspension,
    trace: &mut Vec<serde_json::Value>,
) -> Result<Option<RunStop>, WorkflowError> {
    let run = engine.db.get_workflow_run(community_id, run_id).await?;
    let workflow = engine
        .db
        .get_workflow(community_id, run.workflow_id)
        .await?;

    // The emission fence. A kind:46013 carries a `definitionHash` and no
    // command text: the host compiles the command from its own copy of the
    // project's `actions.yml` and runs whatever that hash names. So this —
    // the moment the request is built from the *workflow* row — is the last
    // place a substituted definition could reach an operator's machine, and
    // the request is never built for a run whose binding no longer holds
    // (ledger 193). An approval request is fenced the same way: asking an
    // operator to approve a step of a definition the run is not bound to
    // would be asking them to consent to something that cannot run.
    if let Some(stop) = crate::executor::run_definition_stop(&run, &workflow) {
        tracing::warn!(
            run_id = %run_id,
            step_id = suspension.step_id(),
            reason = stop.code(),
            "Refusing to publish a suspension request: {}",
            stop.message()
        );
        return Ok(Some(stop));
    }
    let channel_id = workflow
        .channel_id
        .ok_or_else(|| WorkflowError::SuspensionFailed("workflow has no channel".into()))?
        .to_string();
    let owner_pubkey_hex = nostr::PublicKey::from_slice(&workflow.owner_pubkey)
        .map(|key| key.to_hex())
        .map_err(|error| WorkflowError::SuspensionFailed(format!("owner pubkey: {error}")))?;

    match suspension {
        Suspension::Approval {
            step_id,
            approver_spec,
            message,
            timeout_secs,
            synthetic,
        } => {
            let token = Uuid::new_v4().to_string();
            let approval_ref = hex::encode(Sha256::digest(token.as_bytes()));
            let expires_at = Utc::now() + Duration::seconds(*timeout_secs as i64);
            engine
                .db
                .create_approval(CreateApprovalParams {
                    community_id,
                    token: &token,
                    workflow_id: run.workflow_id,
                    run_id,
                    step_id,
                    step_index: step_index as i32,
                    approver_spec,
                    expires_at,
                })
                .await?;
            trace.push(serde_json::json!({
                "step_id": step_id,
                "status": TRACE_AWAITING_APPROVAL,
                "approval_ref": approval_ref,
                "synthetic": synthetic,
            }));
            park(
                engine,
                community_id,
                run_id,
                RunStatus::WaitingApproval,
                step_index,
                trace,
            )
            .await?;
            let request = ApprovalRequest {
                approval_ref,
                run_id,
                workflow_id: run.workflow_id,
                workflow_name: workflow.name.clone(),
                step_id: step_id.clone(),
                step_index,
                approver_spec: approver_spec.clone(),
                message: message.clone(),
                expires_at: expires_at.timestamp().max(0) as u64,
                channel_id,
                owner_pubkey_hex,
                synthetic: *synthetic,
            };
            engine
                .action_sink()?
                .request_approval(community_id, &request)
                .await
                .map_err(|error| {
                    WorkflowError::SuspensionFailed(format!(
                        "approval row written and run parked, but the request could not be \
                         published: {error}"
                    ))
                })?;
            Ok(None)
        }
        Suspension::HostStep {
            step_id,
            step_kind,
            approval,
            inputs,
        } => {
            let expires_at = Utc::now() + Duration::seconds(HOST_STEP_CLAIM_WINDOW_SECS as i64);
            engine
                .db
                .create_host_step(CreateHostStepParams {
                    community_id,
                    run_id,
                    workflow_id: run.workflow_id,
                    step_id,
                    step_index: step_index as i32,
                    expires_at,
                })
                .await?;
            trace.push(serde_json::json!({
                "step_id": step_id,
                "status": TRACE_REQUESTED_ON_HOST,
            }));
            park(
                engine,
                community_id,
                run_id,
                RunStatus::WaitingHost,
                step_index,
                trace,
            )
            .await?;
            let request = host_step_request(
                &run,
                &workflow,
                step_id,
                step_index,
                step_kind,
                approval.clone(),
                inputs.clone(),
                &channel_id,
                expires_at.timestamp().max(0) as u64,
            );
            let event_id = engine
                .action_sink()?
                .request_host_step(community_id, &request)
                .await
                .map_err(|error| {
                    WorkflowError::SuspensionFailed(format!(
                        "host step row written and run parked, but the request could not be \
                         published: {error}"
                    ))
                })?;
            let event_bytes = hex::decode(&event_id).map_err(|error| {
                WorkflowError::SuspensionFailed(format!("request event id is not hex: {error}"))
            })?;
            engine
                .db
                .set_host_step_requested_event(community_id, run_id, step_id, &event_bytes)
                .await?;
            Ok(None)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn host_step_request(
    run: &WorkflowRunRecord,
    workflow: &WorkflowRecord,
    step_id: &str,
    step_index: usize,
    step_kind: &str,
    approval: Option<HostStepApproval>,
    inputs: serde_json::Value,
    channel_id: &str,
    expires_at: u64,
) -> HostStepRequested {
    let trigger_context = run
        .trigger_context
        .clone()
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    HostStepRequested {
        schema: HOST_STEP_SCHEMA.into(),
        run_id: run.id.to_string(),
        workflow_id: workflow.id.to_string(),
        workflow_name: workflow.name.clone(),
        step_id: step_id.to_owned(),
        step_index: step_index as u32,
        definition_hash: hex::encode(&workflow.definition_hash),
        step_kind: step_kind.to_owned(),
        channel_id: channel_id.to_owned(),
        project: workflow
            .definition
            .get("project")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        approval,
        trigger_context,
        inputs: if inputs.is_object() {
            inputs
        } else {
            serde_json::Value::Object(Default::default())
        },
        expires_at,
    }
}

async fn park(
    engine: &WorkflowEngine,
    community_id: CommunityId,
    run_id: Uuid,
    status: RunStatus,
    step_index: usize,
    trace: &[serde_json::Value],
) -> Result<(), WorkflowError> {
    engine
        .db
        .update_workflow_run(
            community_id,
            run_id,
            status,
            step_index as i32,
            &serde_json::Value::Array(trace.to_vec()),
            None,
        )
        .await?;
    Ok(())
}

/// The output a completed host step contributes to later steps' templates
/// and `if:` conditions: `steps_<id>_output_exit_code` and friends.
pub fn host_step_output(result: &buzz_core::host_step::HostStepResult) -> serde_json::Value {
    serde_json::json!({
        "exit_code": result.exit_code,
        "timed_out": result.timed_out,
        "disposition": result.disposition,
        "refusal_code": result.refusal.as_ref().map(|refusal| refusal.code.clone()),
        "duration_ms": result.duration_ms,
        "head_sha": result.head_sha,
        "dirty": result.dirty,
        // How the tree was established, so a later step's brief can name the
        // commit that was actually tested rather than implying one.
        "checkout_mode": result.checkout.as_ref().map(|c| c.mode.clone()),
        "checkout_sha": result.checkout.as_ref().and_then(|c| c.sha.clone()),
        "head_sha_before": result.checkout.as_ref().and_then(|c| c.head_sha_before.clone()),
        "dirty_before": result.checkout.as_ref().and_then(|c| c.dirty_before),
        "stdout_tail": result.stdout_tail,
        "stderr_tail": result.stderr_tail,
        "truncated": result.truncated,
        "artifact_path": result.artifact_path,
        "routed": result.routed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse_yaml;

    const YAML: &str = "name: nightly\nproject: '30621:1111111111111111111111111111111111111111111111111111111111111111:pulse'\ntrigger:\n  on: manual\nsteps:\n  - id: gate\n    action: request_approval\n    from: any\n    message: ok?\n  - id: build\n    action: run_on_host\n    command: [\"true\"]\n  - id: after\n    action: send_message\n    text: done\n";

    #[test]
    fn approval_on_a_host_step_resumes_at_the_step_itself() {
        let (def, _) = parse_yaml(YAML).expect("parse");
        assert_eq!(
            resume_index_after_approval(&def, 0),
            1,
            "authored gate resumes after itself"
        );
        assert_eq!(
            resume_index_after_approval(&def, 1),
            1,
            "synthetic gate resumes at the host step"
        );
        assert_eq!(resume_index_after_approval(&def, 2), 3);
        assert_eq!(
            resume_index_after_approval(&def, 9),
            10,
            "out of range falls through"
        );
    }

    #[test]
    fn host_step_output_exposes_exit_code_for_conditions() {
        let result = buzz_core::host_step::HostStepResult {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: Uuid::nil().to_string(),
            step_id: "build".into(),
            requested_event_id: "a".repeat(64),
            claim_event_id: Some("b".repeat(64)),
            channel_id: Uuid::nil().to_string(),
            disposition: buzz_core::host_step::HostStepDisposition::Exited,
            exit_code: Some(1),
            refusal: None,
            timed_out: false,
            duration_ms: Some(5),
            head_sha: None,
            agents_commit: None,
            dirty: None,
            checkout: None,
            stdout_tail: "x".into(),
            stderr_tail: String::new(),
            truncated: false,
            artifact_path: None,
            routed: None,
            artifacts: Vec::new(),
        };
        let output = host_step_output(&result);
        assert_eq!(output["exit_code"], 1);
        assert_eq!(output["disposition"], "exited");
        assert!(output["refusal_code"].is_null());
    }
}
