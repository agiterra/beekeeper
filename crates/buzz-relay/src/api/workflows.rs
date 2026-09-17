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
        "runs": rows.iter().map(run_json).collect::<Vec<_>>(),
        "next": next,
    })))
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

fn run_json(run: &buzz_db::workflow::WorkflowRunRecord) -> Value {
    serde_json::json!({
        "id": run.id,
        "workflow_id": run.workflow_id,
        "status": run.status,
        "current_step": run.current_step,
        "execution_trace": run.execution_trace,
        "started_at": run.started_at.map(|value| value.timestamp()),
        "completed_at": run.completed_at.map(|value| value.timestamp()),
        "error_code": run.error_code,
        "error_message": run.error_message,
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
        "routed_hired_role": step
            .result
            .as_ref()
            .and_then(|result| result.get("routed"))
            .and_then(|routed| routed.get("hiredRole"))
            .and_then(Value::as_str),
        "exited_at": step.exited_at,
        "created_at": step.created_at,
    })
}

#[cfg(test)]
mod tests {
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
    }
}
