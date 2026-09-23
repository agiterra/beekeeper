//! Authorized structured reads for workflow execution state.
//!
//! Runs and approvals are relay-owned database rows, not Nostr events. These
//! endpoints expose those read models without inventing synthetic events.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use buzz_core::TenantContext;

use crate::{
    api::{api_error, bridge, internal_error},
    state::AppState,
};

const DEFAULT_RUN_LIMIT: i64 = 20;
const MAX_RUN_LIMIT: i64 = 100;

/// Pagination query for workflow run history.
#[derive(Debug, Deserialize, Default)]
pub struct RunsQuery {
    before: Option<DateTime<Utc>>,
    before_id: Option<Uuid>,
    limit: Option<i64>,
}

fn request_path(path: &str, raw_query: Option<&str>) -> String {
    match raw_query {
        Some(query) if !query.is_empty() => format!("{path}?{query}"),
        _ => path.to_string(),
    }
}

async fn authorize_workflow_read(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    path: &str,
    raw_query: Option<&str>,
    workflow_id: Uuid,
) -> Result<TenantContext, (StatusCode, Json<Value>)> {
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let tenant = crate::tenant::bind_community(&state.db, raw_host)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::NOT_FOUND,
                "relay: no community is configured for this host",
            )
        })?;

    let path_with_query = request_path(path, raw_query);
    let url = bridge::nip98_expected_url(&state.config.relay_url, &tenant, &path_with_query);
    let bridge::VerifiedBridgeAuth {
        pubkey,
        event_id_bytes,
        signed_created_at,
    } = bridge::verify_bridge_auth(headers, "GET", &url, None, state.config.require_auth_token)?;
    bridge::enforce_http_admission(state, &tenant, &pubkey).await?;
    bridge::check_nip98_replay(state, &tenant, event_id_bytes).await?;

    let pubkey_bytes = pubkey.to_bytes().to_vec();
    let auth_tag = super::relay_members::extract_auth_tag_header(headers);
    super::relay_members::enforce_relay_membership(
        state,
        tenant.community(),
        &pubkey_bytes,
        auth_tag,
        signed_created_at,
    )
    .await?;

    let workflow = state
        .db
        .get_workflow(tenant.community(), workflow_id)
        .await
        .map_err(|error| match error {
            buzz_db::error::DbError::NotFound(_) => {
                api_error(StatusCode::NOT_FOUND, "workflow not found")
            }
            other => internal_error(&format!("get workflow for run read: {other}")),
        })?;
    let channel_id = workflow
        .channel_id
        .ok_or_else(|| api_error(StatusCode::FORBIDDEN, "workflow is not channel-scoped"))?;
    let accessible = state
        .get_accessible_channel_ids_cached(tenant.community(), &pubkey_bytes)
        .await
        .map_err(|error| internal_error(&format!("workflow channel access lookup: {error}")))?;
    if !accessible.contains(&channel_id) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "workflow is not accessible",
        ));
    }

    Ok(tenant)
}

/// `GET /workflows/{workflow_id}/runs` — one authorized, keyset-paginated page.
pub async fn workflow_runs(
    State(state): State<Arc<AppState>>,
    Path(workflow_id): Path<Uuid>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<RunsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if query.before.is_some() != query.before_id.is_some() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "before and before_id must be supplied together",
        ));
    }
    let limit = query.limit.unwrap_or(DEFAULT_RUN_LIMIT);
    if !(1..=MAX_RUN_LIMIT).contains(&limit) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "limit must be between 1 and 100",
        ));
    }

    let path = format!("/workflows/{workflow_id}/runs");
    let tenant =
        authorize_workflow_read(&state, &headers, &path, raw_query.as_deref(), workflow_id).await?;
    let workflow = state
        .db
        .get_workflow(tenant.community(), workflow_id)
        .await
        .map_err(|error| internal_error(&format!("get workflow for run read: {error}")))?;
    let mut rows = state
        .db
        .list_workflow_runs_page(
            tenant.community(),
            workflow_id,
            query.before,
            query.before_id,
            limit + 1,
        )
        .await
        .map_err(|error| internal_error(&format!("list workflow runs: {error}")))?;

    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = if has_more {
        rows.last().map(|last| {
            serde_json::json!({
                "before": last.created_at,
                "before_id": last.id,
            })
        })
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "runs": rows
            .iter()
            .map(|run| run_json(run, &workflow.name))
            .collect::<Vec<_>>(),
        "next": next,
    })))
}

/// Whether `caller` may read one run of a workflow (ledger 252).
///
/// A member of the workflow's channel always may. Beyond the channel, the
/// run's own trigger author may (a seat that started a run must be able to
/// follow it — control run 3's lead got `403 workflow is not accessible` for
/// the run it had just triggered, because setup filed `verify` in a channel
/// only the owner held), and so may anyone on the roster of the project the
/// workflow is bound to (`roster` = the owner and every invited member,
/// lowercase hex). Nothing wider: a workflow bound to no project admits only
/// its channel and its trigger author.
fn run_read_admitted(
    channel_accessible: bool,
    caller_hex: &str,
    trigger_author: Option<&str>,
    roster: &[String],
) -> bool {
    if channel_accessible {
        return true;
    }
    let caller = caller_hex.to_ascii_lowercase();
    trigger_author.is_some_and(|author| author.eq_ignore_ascii_case(&caller))
        || roster
            .iter()
            .any(|member| member.eq_ignore_ascii_case(&caller))
}

/// Authorize a read keyed by `run_id` alone: resolve the run's owning
/// workflow, then apply the same channel-accessibility check
/// [`authorize_workflow_read`] applies when the caller already knows the
/// workflow id. Returns the tenant, the run and its workflow so the caller
/// does not have to look either up again.
async fn authorize_run_read(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    path: &str,
    run_id: Uuid,
) -> Result<
    (
        TenantContext,
        buzz_db::workflow::WorkflowRunRecord,
        buzz_db::workflow::WorkflowRecord,
    ),
    (StatusCode, Json<Value>),
> {
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let tenant = crate::tenant::bind_community(&state.db, raw_host)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::NOT_FOUND,
                "relay: no community is configured for this host",
            )
        })?;

    let url = bridge::nip98_expected_url(&state.config.relay_url, &tenant, path);
    let bridge::VerifiedBridgeAuth {
        pubkey,
        event_id_bytes,
        signed_created_at,
    } = bridge::verify_bridge_auth(headers, "GET", &url, None, state.config.require_auth_token)?;
    bridge::enforce_http_admission(state, &tenant, &pubkey).await?;
    bridge::check_nip98_replay(state, &tenant, event_id_bytes).await?;

    let pubkey_bytes = pubkey.to_bytes().to_vec();
    let auth_tag = super::relay_members::extract_auth_tag_header(headers);
    super::relay_members::enforce_relay_membership(
        state,
        tenant.community(),
        &pubkey_bytes,
        auth_tag,
        signed_created_at,
    )
    .await?;

    let run = state
        .db
        .get_workflow_run(tenant.community(), run_id)
        .await
        .map_err(|error| match error {
            buzz_db::error::DbError::NotFound(_) => {
                api_error(StatusCode::NOT_FOUND, "workflow run not found")
            }
            other => internal_error(&format!("get workflow run for run-status read: {other}")),
        })?;
    let workflow = state
        .db
        .get_workflow(tenant.community(), run.workflow_id)
        .await
        .map_err(|error| internal_error(&format!("get workflow for run-status read: {error}")))?;
    let channel_id = workflow
        .channel_id
        .ok_or_else(|| api_error(StatusCode::FORBIDDEN, "workflow is not channel-scoped"))?;
    let accessible = state
        .get_accessible_channel_ids_cached(tenant.community(), &pubkey_bytes)
        .await
        .map_err(|error| internal_error(&format!("workflow channel access lookup: {error}")))?
        .contains(&channel_id);
    let caller_hex = hex::encode(&pubkey_bytes);
    let trigger_author = run
        .trigger_context
        .as_ref()
        .and_then(|context| context.get("author"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    // The roster is read only when neither the channel nor the trigger
    // author already decided the read.
    let roster = match workflow.project_ref.as_deref() {
        Some(project)
            if !run_read_admitted(accessible, &caller_hex, trigger_author.as_deref(), &[]) =>
        {
            match state
                .db
                .get_project_roster(tenant.community(), project)
                .await
                .map_err(|error| internal_error(&format!("workflow project roster: {error}")))?
            {
                Some(roster) => std::iter::once(hex::encode(&roster.owner))
                    .chain(roster.members.iter().map(|(member, _)| hex::encode(member)))
                    .collect(),
                None => Vec::new(),
            }
        }
        _ => Vec::new(),
    };
    if !run_read_admitted(accessible, &caller_hex, trigger_author.as_deref(), &roster) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "workflow is not accessible",
        ));
    }

    Ok((tenant, run, workflow))
}

/// `GET /workflow-runs/{run_id}` — one run's full state keyed by run id alone
/// (the caller does not have to already know the workflow id): the run row,
/// its host steps and its approvals, in one authorized read.
///
/// Exists because every other run read is nested under
/// `/workflows/{workflow_id}/...`, and a run id surfaces on its own — in a
/// kind:46010 approval request's content, a kind:46013 host-step request's
/// `d` tag, a kind:46023 host result — long before a caller has occasion to
/// look up which workflow it belongs to.
pub async fn run_status(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let path = format!("/workflow-runs/{run_id}");
    let (tenant, run, workflow) = authorize_run_read(&state, &headers, &path, run_id).await?;

    let host_steps = state
        .db
        .list_run_host_steps(tenant.community(), run_id)
        .await
        .map_err(|error| internal_error(&format!("list run host steps: {error}")))?;
    let approvals = state
        .db
        .get_run_approvals(tenant.community(), workflow.id, run_id)
        .await
        .map_err(|error| internal_error(&format!("list run approvals: {error}")))?;

    let mut run_wire = run_json(&run, &workflow.name);
    run_wire["host_steps"] = Value::Array(host_steps.iter().map(host_step_json).collect());
    run_wire["approvals"] = Value::Array(approvals.iter().map(approval_json).collect());
    Ok(Json(run_wire))
}

/// `GET /workflows/{workflow_id}/runs/{run_id}/approvals` — approvals for a run.
pub async fn run_approvals(
    State(state): State<Arc<AppState>>,
    Path((workflow_id, run_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let path = format!("/workflows/{workflow_id}/runs/{run_id}/approvals");
    let tenant = authorize_workflow_read(&state, &headers, &path, None, workflow_id).await?;

    let run = state
        .db
        .get_workflow_run(tenant.community(), run_id)
        .await
        .map_err(|error| match error {
            buzz_db::error::DbError::NotFound(_) => {
                api_error(StatusCode::NOT_FOUND, "workflow run not found")
            }
            other => internal_error(&format!("get workflow run for approval read: {other}")),
        })?;
    if run.workflow_id != workflow_id {
        return Err(api_error(StatusCode::NOT_FOUND, "workflow run not found"));
    }

    let approvals = state
        .db
        .get_run_approvals(tenant.community(), workflow_id, run_id)
        .await
        .map_err(|error| internal_error(&format!("list run approvals: {error}")))?;
    Ok(Json(serde_json::json!({
        "approvals": approvals.iter().map(approval_json).collect::<Vec<_>>(),
    })))
}

/// `GET /workflows/{workflow_id}/autorun` — every autorun grant for a
/// workflow, newest first, each marked whether it binds the definition as
/// stored *now* (an edit changes the hash, so an old grant no longer applies).
pub async fn workflow_autorun(
    State(state): State<Arc<AppState>>,
    Path(workflow_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let path = format!("/workflows/{workflow_id}/autorun");
    let tenant = authorize_workflow_read(&state, &headers, &path, None, workflow_id).await?;
    let workflow = state
        .db
        .get_workflow(tenant.community(), workflow_id)
        .await
        .map_err(|error| match error {
            buzz_db::error::DbError::NotFound(_) => {
                api_error(StatusCode::NOT_FOUND, "workflow not found")
            }
            other => internal_error(&format!("get workflow for autorun read: {other}")),
        })?;
    let grants = state
        .db
        .list_autorun_grants(tenant.community(), workflow_id)
        .await
        .map_err(|error| internal_error(&format!("list autorun grants: {error}")))?;
    let active = grants.iter().any(|grant| {
        grant.revoked_at.is_none() && grant.definition_hash == workflow.definition_hash
    });
    Ok(Json(serde_json::json!({
        "definition_hash": hex::encode(&workflow.definition_hash),
        "active": active,
        "grants": grants
            .iter()
            .map(|grant| serde_json::json!({
                "id": grant.id,
                "definition_hash": hex::encode(&grant.definition_hash),
                "matches_current": grant.definition_hash == workflow.definition_hash,
                "granted_by": hex::encode(&grant.granted_by),
                "grant_event_id": hex::encode(&grant.grant_event_id),
                "granted_at": grant.granted_at,
                "revoked_at": grant.revoked_at,
                "revoke_event_id": grant.revoke_event_id.as_ref().map(hex::encode),
            }))
            .collect::<Vec<_>>(),
    })))
}

/// `GET /workflows/{workflow_id}/runs/{run_id}/host-steps` — every
/// `run_on_host` step of a run: who claimed it, how it ended, and the event
/// ids that prove each transition.
pub async fn run_host_steps(
    State(state): State<Arc<AppState>>,
    Path((workflow_id, run_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let path = format!("/workflows/{workflow_id}/runs/{run_id}/host-steps");
    let tenant = authorize_workflow_read(&state, &headers, &path, None, workflow_id).await?;

    let run = state
        .db
        .get_workflow_run(tenant.community(), run_id)
        .await
        .map_err(|error| match error {
            buzz_db::error::DbError::NotFound(_) => {
                api_error(StatusCode::NOT_FOUND, "workflow run not found")
            }
            other => internal_error(&format!("get workflow run for host step read: {other}")),
        })?;
    if run.workflow_id != workflow_id {
        return Err(api_error(StatusCode::NOT_FOUND, "workflow run not found"));
    }

    let host_steps = state
        .db
        .list_run_host_steps(tenant.community(), run_id)
        .await
        .map_err(|error| internal_error(&format!("list run host steps: {error}")))?;
    Ok(Json(serde_json::json!({
        "host_steps": host_steps.iter().map(host_step_json).collect::<Vec<_>>(),
    })))
}

/// `name` is the owning workflow's current name — not stored on the run row
/// itself, so every call site passes it in from its own `get_workflow` read
/// rather than this function re-deriving it.
fn run_json(run: &buzz_db::workflow::WorkflowRunRecord, workflow_name: &str) -> Value {
    let trigger_field = |name: &str| {
        run.trigger_context
            .as_ref()
            .and_then(|context| context.get(name))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let trigger_author = trigger_field("author");
    // The commit this run is bound to, from the trigger context lane 184 put
    // it on (`TriggerContext::checkout`). Until now it left the relay only
    // once a host had claimed the step and recorded a result, so an approval
    // card for a *waiting* run could not say which commit it would test —
    // which is the one fact an approver needs before saying yes. `null` means
    // the run names no commit and a host step will run in the recorded
    // project directory as it is found, exactly as an unbound manual trigger
    // does (ledger 184(c)).
    let checkout = trigger_field("checkout");
    serde_json::json!({
        "id": run.id,
        "workflow_id": run.workflow_id,
        "workflow_name": workflow_name,
        "status": run.status,
        "current_step": run.current_step,
        "execution_trace": run.execution_trace,
        "trigger_event_id": run.trigger_event_id.as_ref().map(hex::encode),
        "trigger_author": trigger_author,
        "checkout": checkout,
        "started_at": run.started_at.map(|value| value.timestamp()),
        "completed_at": run.completed_at.map(|value| value.timestamp()),
        "error_code": run.error_code,
        "error_message": run.error_message,
        // The definition this run was created from, so a reader can tell a
        // run bound to the published action from one that was stopped by an
        // edit (`error_code` `definition_changed`) and from a pre-binding run
        // (`definition_unknown`). `null` is a disclosed non-answer: the run
        // predates the binding, never that the relay declined to say.
        "definition_hash": run.definition_hash.as_ref().map(hex::encode),
        "created_at": run.created_at.timestamp(),
    })
}

fn approval_json(approval: &buzz_db::workflow::ApprovalRecord) -> Value {
    serde_json::json!({
        "approval_ref": hex::encode(&approval.token),
        "workflow_id": approval.workflow_id,
        "run_id": approval.run_id,
        "step_id": approval.step_id,
        "step_index": approval.step_index,
        "approver_spec": approval.approver_spec,
        "status": approval.status,
        "approver_pubkey": approval.approver_pubkey.as_ref().map(hex::encode),
        "note": approval.note,
        "expires_at": approval.expires_at,
        "created_at": approval.created_at.timestamp(),
    })
}

/// The wire shape of one host-step row. Event ids and the claiming host are
/// lowercase hex or `null`; timestamps are RFC 3339 like `expires_at` on an
/// approval.
fn host_step_json(step: &buzz_db::workflow::HostStepRecord) -> Value {
    serde_json::json!({
        "run_id": step.run_id,
        "step_id": step.step_id,
        "workflow_id": step.workflow_id,
        "step_index": step.step_index,
        "status": step.status,
        "requested_event_id": step.requested_event_id.as_ref().map(hex::encode),
        "expires_at": step.expires_at,
        "claimed_by": step.claimed_by.as_ref().map(hex::encode),
        "claimed_at": step.claimed_at,
        "claim_event_id": step.claim_event_id.as_ref().map(hex::encode),
        "result_event_id": step.result_event_id.as_ref().map(hex::encode),
        "exited_event_id": step.exited_event_id.as_ref().map(hex::encode),
        "exit_code": step.exit_code,
        "disposition": step.disposition,
        "timed_out": step.timed_out,
        "duration_ms": step.duration_ms,
        "head_sha": step.head_sha,
        "dirty": step.dirty,
        "artifact_ref": step.artifact_ref,
        // Spec § 5.7: where a `wake_agent` step delivered its brief, off the
        // accepted result; null on a command step and on a refusal.
        "routed_agent": step
            .result
            .as_ref()
            .and_then(|result| result.get("routed"))
            .and_then(|routed| routed.get("agent"))
            .and_then(Value::as_str),
        "routed_command_id": step
            .result
            .as_ref()
            .and_then(|result| result.get("routed"))
            .and_then(|routed| routed.get("commandId"))
            .and_then(Value::as_str),
        "artifacts": step
            .result
            .as_ref()
            .and_then(|result| result.get("artifacts"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "routed_hired_role": step
            .result
            .as_ref()
            .and_then(|result| result.get("routed"))
            .and_then(|routed| routed.get("hiredRole"))
            .and_then(Value::as_str),
        // How the tree this step ran in was established (lane 184,
        // ledger 178(g)/(l)): `mode` names a detached-worktree commit or
        // "working directory as found"; `sha`/`headShaBefore`/`dirtyBefore`
        // are present exactly when the host could establish them. `null`
        // on a result recorded before lane 184.
        "checkout": step
            .result
            .as_ref()
            .and_then(|result| result.get("checkout"))
            .cloned(),
        "exited_at": step.exited_at,
        "created_at": step.created_at,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_run_is_readable_by_its_trigger_author_and_its_project_roster_only() {
        let author = "1650fa10".repeat(8);
        let owner = "3d3b7169".repeat(8);
        let stranger = "deadbeef".repeat(8);
        let roster = vec![owner.clone()];
        // The channel still admits its members.
        assert!(super::run_read_admitted(true, &stranger, None, &[]));
        // The seat that triggered the run, outside the workflow's channel.
        assert!(super::run_read_admitted(false, &author, Some(&author), &[]));
        assert!(super::run_read_admitted(
            false,
            &author.to_uppercase(),
            Some(&author),
            &[]
        ));
        // A project roster member, outside the channel.
        assert!(super::run_read_admitted(
            false,
            &owner,
            Some(&author),
            &roster
        ));
        // Nobody else.
        assert!(!super::run_read_admitted(
            false,
            &stranger,
            Some(&author),
            &roster
        ));
        assert!(!super::run_read_admitted(false, &stranger, None, &[]));
    }
    use super::*;

    #[test]
    fn request_path_preserves_signed_query_verbatim() {
        assert_eq!(
            request_path("/workflows/id/runs", Some("limit=20&before_id=abc")),
            "/workflows/id/runs?limit=20&before_id=abc"
        );
        assert_eq!(
            request_path("/workflows/id/runs", None),
            "/workflows/id/runs"
        );
    }

    #[test]
    fn approval_wire_does_not_expose_hash_as_token() {
        let approval = buzz_db::workflow::ApprovalRecord {
            token: vec![0xab; 32],
            workflow_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            step_id: "review".to_string(),
            step_index: 1,
            approver_spec: "any".to_string(),
            status: buzz_db::workflow::ApprovalStatus::Pending,
            approver_pubkey: None,
            note: None,
            expires_at: Utc::now(),
            created_at: Utc::now(),
        };
        let wire = approval_json(&approval);
        assert!(wire.get("token").is_none());
        assert_eq!(wire["approval_ref"], hex::encode([0xab; 32]));
    }

    #[test]
    fn host_step_wire_hex_encodes_ids_and_keeps_nulls() {
        let claimed_by = vec![0x02; 33];
        let step = buzz_db::workflow::HostStepRecord {
            run_id: Uuid::new_v4(),
            step_id: "build".to_string(),
            workflow_id: Uuid::new_v4(),
            step_index: 1,
            status: buzz_db::workflow::HostStepStatus::Exited,
            requested_event_id: Some(vec![0xaa; 32]),
            expires_at: Utc::now(),
            claimed_by: Some(claimed_by.clone()),
            claimed_at: Some(Utc::now()),
            claim_event_id: Some(vec![0xbb; 32]),
            result_event_id: Some(vec![0xcc; 32]),
            exited_event_id: None,
            exit_code: Some(124),
            disposition: Some("timed_out".to_string()),
            timed_out: Some(true),
            duration_ms: Some(1_800_000),
            head_sha: Some("a".repeat(40)),
            dirty: Some(false),
            artifact_ref: Some("/tmp/actions/run/build".to_string()),
            result: Some(serde_json::json!({"stdoutTail": "secret-free"})),
            exited_at: Some(Utc::now()),
            created_at: Utc::now(),
        };
        let wire = host_step_json(&step);
        assert_eq!(wire["status"], "exited");
        assert_eq!(wire["requested_event_id"], hex::encode([0xaa; 32]));
        assert_eq!(wire["claimed_by"], hex::encode(&claimed_by));
        assert_eq!(wire["claim_event_id"], hex::encode([0xbb; 32]));
        assert_eq!(wire["result_event_id"], hex::encode([0xcc; 32]));
        assert!(wire["exited_event_id"].is_null());
        assert_eq!(wire["exit_code"], 124);
        assert_eq!(wire["disposition"], "timed_out");
        assert_eq!(wire["timed_out"], true);
        assert_eq!(wire["head_sha"], "a".repeat(40));
        assert!(
            wire["expires_at"].as_str().is_some_and(|s| s.contains('T')),
            "timestamps are RFC 3339 strings"
        );
        assert!(
            wire.get("result").is_none(),
            "the verbatim result is not part of the listing; the 46014 echo carries it"
        );
        assert_eq!(wire["step_id"], "build");
        assert_eq!(wire["step_index"], 1);
        assert!(
            wire["checkout"].is_null(),
            "no checkout sub-object on this result"
        );
    }

    #[test]
    fn host_step_wire_surfaces_the_checkout_sub_object() {
        // Lane 184 / ledger 178(g)/(l): a result recorded after that fix
        // carries `checkout` (mode, and sha/headShaBefore/dirtyBefore when
        // the host could establish them). `bee workflows run-status` is the
        // one place this reaches a caller, so it must not be dropped here.
        let step = buzz_db::workflow::HostStepRecord {
            run_id: Uuid::new_v4(),
            step_id: "build".to_string(),
            workflow_id: Uuid::new_v4(),
            step_index: 0,
            status: buzz_db::workflow::HostStepStatus::Exited,
            requested_event_id: Some(vec![0xaa; 32]),
            expires_at: Utc::now(),
            claimed_by: None,
            claimed_at: None,
            claim_event_id: None,
            result_event_id: Some(vec![0xcc; 32]),
            exited_event_id: None,
            exit_code: Some(0),
            disposition: Some("ok".to_string()),
            timed_out: Some(false),
            duration_ms: Some(4070),
            head_sha: Some("fa927fd".repeat(6)),
            dirty: Some(false),
            artifact_ref: None,
            result: Some(serde_json::json!({
                "checkout": {
                    "mode": "commit fa927fd",
                    "sha": "fa927fd".repeat(6),
                    "headShaBefore": "e682191".repeat(6),
                    "dirtyBefore": false,
                },
            })),
            exited_at: Some(Utc::now()),
            created_at: Utc::now(),
        };
        let wire = host_step_json(&step);
        assert_eq!(wire["checkout"]["mode"], "commit fa927fd");
        assert_eq!(wire["checkout"]["sha"], "fa927fd".repeat(6));
        assert_eq!(wire["checkout"]["dirtyBefore"], false);
    }

    fn sample_run(status: buzz_db::workflow::RunStatus) -> buzz_db::workflow::WorkflowRunRecord {
        buzz_db::workflow::WorkflowRunRecord {
            id: Uuid::new_v4(),
            community_id: buzz_core::tenant::CommunityId::from_uuid(Uuid::new_v4()),
            workflow_id: Uuid::new_v4(),
            status,
            trigger_event_id: Some(vec![0xde; 32]),
            current_step: 0,
            execution_trace: serde_json::json!([]),
            trigger_context: Some(serde_json::json!({"author": "abc123"})),
            started_at: None,
            completed_at: None,
            error_message: None,
            error_code: None,
            definition_hash: Some(vec![0xab; 32]),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn run_wire_names_the_workflow_and_the_trigger() {
        let run = sample_run(buzz_db::workflow::RunStatus::WaitingHost);
        let wire = run_json(&run, "nightly build");
        assert_eq!(wire["workflow_name"], "nightly build");
        assert_eq!(wire["status"], "waiting_host");
        assert_eq!(wire["trigger_event_id"], hex::encode([0xde; 32]));
        assert_eq!(wire["trigger_author"], "abc123");
        assert!(
            wire["checkout"].is_null(),
            "a run whose trigger bound no commit says so"
        );
    }

    /// Ledger 206 B: a run bound to a commit names it on both run reads,
    /// before any host has claimed a step. The desktop's approval card reads
    /// this to say which commit a waiting run will test.
    #[test]
    fn run_wire_names_the_commit_a_bound_run_will_test() {
        let sha = "fa927fd".repeat(6);
        let mut run = sample_run(buzz_db::workflow::RunStatus::WaitingApproval);
        run.trigger_context = Some(serde_json::json!({
            "author": "abc123",
            "checkout": sha,
        }));
        let wire = run_json(&run, "verify");
        assert_eq!(wire["checkout"], sha);
        assert_eq!(wire["status"], "waiting_approval");
        // An empty string is the absent case on the wire (the field is
        // `skip_serializing_if = "String::is_empty"`), never a commit.
        run.trigger_context = Some(serde_json::json!({"author": "abc123", "checkout": ""}));
        assert!(run_json(&run, "verify")["checkout"].is_null());
    }

    #[test]
    fn run_wire_trigger_author_is_null_without_a_trigger_context() {
        let mut run = sample_run(buzz_db::workflow::RunStatus::Completed);
        run.trigger_context = None;
        run.trigger_event_id = None;
        let wire = run_json(&run, "nightly build");
        assert!(wire["trigger_event_id"].is_null());
        assert!(wire["trigger_author"].is_null());
    }
}
