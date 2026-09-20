//! Host-executed workflow steps: the claim, the result, and the resume.
//!
//! A `run_on_host` step parks its run as `waiting_host` and publishes a
//! relay-signed kind:46013 request (`buzz_workflow::suspend`). A session
//! provider answers with a kind:46022 claim; exactly one claim wins the row
//! (`buzz_db::workflow::claim_host_step`, a single conditional UPDATE), and
//! every loser's ingest response says `claimed by <hex>` so an operator can
//! read who executed. The winner's kind:46023 result closes the row, is
//! echoed as a relay-signed kind:46014, and the run resumes at the next step
//! with the result as the host step's output (spec § 5.4–5.6).
//!
//! A `refused` result that carries no `claimEventId` — the host declined
//! before claiming, e.g. definition drift — is stored as an event and
//! nothing else: the row stays `requested` so another host may still claim.
//!
//! SECURITY: like `command_executor`, this module runs only after the ingest
//! pipeline has verified the signature, timestamp, pubkey/auth match and
//! scope. What it adds: the signer must be a member of the workflow's
//! channel, the event's `h` must be that channel, and the claim must name
//! the request the row records.

use std::collections::HashMap;
use std::sync::Arc;

use nostr::Event;
use uuid::Uuid;

use buzz_core::host_step::{
    build_host_step_exited, decode_host_step_claim, decode_host_step_result, HostStepDisposition,
    HostStepExited, HOST_STEP_SCHEMA,
};
use buzz_core::kind::KIND_WORKFLOW_HOST_STEP_EXITED;
use buzz_core::tenant::{CommunityId, TenantContext};
use buzz_db::workflow::{
    HostStepClaimOutcome, HostStepRecord, HostStepResultParams, HostStepStatus, RunStatus,
};
use buzz_db::DbError;
use buzz_workflow::executor::TriggerContext;

use crate::state::AppState;
use crate::workflow_sink::publish_relay_event;

use super::command_executor::{persist_command_event, PersistResult};
use super::ingest::{IngestAuth, IngestError, IngestResult};

/// Trace status written for a host step whose result the relay accepted.
pub const TRACE_HOST_STEP_COMPLETED: &str = "completed";

/// The host step row, its workflow's channel, and the signer's bytes — the
/// facts both handlers establish before touching anything.
struct Admitted {
    row: HostStepRecord,
    channel_id: Uuid,
    signer: Vec<u8>,
}

/// Load the row for `(run_id, step_id)`, check the event's channel is the
/// workflow's channel, and check the signer is a member of it.
async fn admit(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    auth: &IngestAuth,
    run_id: &str,
    step_id: &str,
    channel_id: &str,
) -> Result<Admitted, IngestError> {
    let run_id = Uuid::parse_str(run_id)
        .map_err(|_| IngestError::Rejected("invalid: bad run id format".into()))?;
    let channel_id = Uuid::parse_str(channel_id)
        .map_err(|_| IngestError::Rejected("invalid: bad channel_id format".into()))?;
    let signer = auth.pubkey().to_bytes().to_vec();

    let row = match state
        .db
        .get_host_step(tenant.community(), run_id, step_id)
        .await
    {
        Ok(row) => row,
        Err(DbError::NotFound(_)) => {
            return Err(IngestError::Rejected("invalid: unknown host step".into()));
        }
        Err(error) => {
            return Err(IngestError::Internal(format!(
                "error: db get_host_step: {error}"
            )));
        }
    };

    let workflow = state
        .db
        .get_workflow(tenant.community(), row.workflow_id)
        .await
        .map_err(|error| IngestError::Internal(format!("error: db get_workflow: {error}")))?;
    if workflow.channel_id != Some(channel_id) {
        return Err(IngestError::Rejected(
            "invalid: h tag is not the workflow's channel".into(),
        ));
    }

    let is_member = state
        .is_member_cached(tenant.community(), channel_id, &signer)
        .await
        .map_err(|error| IngestError::Internal(format!("error: membership check: {error}")))?;
    if !is_member {
        return Err(IngestError::Rejected(
            "forbidden: not a member of this channel".into(),
        ));
    }

    Ok(Admitted {
        row,
        channel_id,
        signer,
    })
}

fn accepted(event: &Event, response: serde_json::Value) -> IngestResult {
    IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!("response:{response}"),
    }
}

fn duplicate(event: &Event) -> IngestResult {
    IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: "duplicate: already processed".into(),
    }
}

/// The relay's `hex` view of an optional stored id, for comparisons with
/// what a host wrote in its payload.
fn hex_of(bytes: &Option<Vec<u8>>) -> Option<String> {
    bytes.as_ref().map(hex::encode)
}

/// Handle a kind:46022 claim.
///
/// Response on success: `{"status":"claimed"}`. A second claim by the same
/// host is an accepted duplicate; a claim by any other host after the row is
/// taken is rejected with `claimed by <hex>` naming the winner.
pub async fn handle_host_step_claim(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let claim = decode_host_step_claim(event)
        .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    let Admitted { row, signer, .. } = admit(
        tenant,
        state,
        auth,
        &claim.run_id,
        &claim.step_id,
        &claim.channel_id,
    )
    .await?;

    if let Some(requested) = hex_of(&row.requested_event_id) {
        if requested != claim.requested_event_id {
            return Err(IngestError::Rejected(
                "invalid: claim does not name the recorded request event".into(),
            ));
        }
    }

    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => return Ok(duplicate(event)),
        PersistResult::Inserted(tx) => tx,
    };

    let outcome = state
        .db
        .claim_host_step(
            tenant.community(),
            row.run_id,
            &row.step_id,
            &signer,
            event.id.as_bytes(),
        )
        .await
        .map_err(|error| IngestError::Internal(format!("error: db claim_host_step: {error}")))?;

    match outcome {
        HostStepClaimOutcome::Claimed(_) => {}
        HostStepClaimOutcome::AlreadyClaimed(existing) => {
            if existing.claimed_by.as_deref() == Some(signer.as_slice()) {
                // The same host claiming again (a retry after its first claim
                // event was lost) holds the row already.
                tx.commit().await.map_err(|error| {
                    IngestError::Internal(format!("error: commit transaction: {error}"))
                })?;
                return Ok(duplicate(event));
            }
            return Err(IngestError::Rejected(format!(
                "claimed by {}",
                existing
                    .claimed_by
                    .as_deref()
                    .map(hex::encode)
                    .unwrap_or_default()
            )));
        }
        HostStepClaimOutcome::Expired(_) => {
            return Err(IngestError::Rejected("expired".into()));
        }
        HostStepClaimOutcome::NotFound => {
            return Err(IngestError::Rejected("invalid: unknown host step".into()));
        }
    }

    tx.commit()
        .await
        .map_err(|error| IngestError::Internal(format!("error: commit transaction: {error}")))?;

    Ok(accepted(event, serde_json::json!({ "status": "claimed" })))
}

/// The snake_case wire word for a disposition, as the row stores it.
fn disposition_str(disposition: HostStepDisposition) -> &'static str {
    match disposition {
        HostStepDisposition::Exited => "exited",
        HostStepDisposition::TimedOut => "timed_out",
        HostStepDisposition::LostOnRestart => "lost_on_restart",
        HostStepDisposition::Refused => "refused",
    }
}

/// Handle a kind:46023 result.
///
/// A result without a `claimEventId` (a refusal before claiming) is stored
/// and answered `{"status":"refusal_recorded"}`; the row and the run are
/// untouched. Otherwise only the claiming host's first result closes the
/// row: the relay echoes it as kind:46014, records the echo's id, and
/// resumes the run at the next step. A repeated result from the claiming
/// host is an accepted duplicate; a result from any other host is rejected.
pub async fn handle_host_step_result(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let result = decode_host_step_result(event)
        .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    let Admitted {
        row,
        channel_id,
        signer,
    } = admit(
        tenant,
        state,
        auth,
        &result.run_id,
        &result.step_id,
        &result.channel_id,
    )
    .await?;

    if let Some(requested) = hex_of(&row.requested_event_id) {
        if requested != result.requested_event_id {
            return Err(IngestError::Rejected(
                "invalid: result does not name the recorded request event".into(),
            ));
        }
    }

    let Some(claim_event_id) = result.claim_event_id.clone() else {
        // Refused before claiming: on the record, but the row stays open for
        // another host.
        let tx = match persist_command_event(&state.db, tenant, event, None).await? {
            PersistResult::Duplicate => return Ok(duplicate(event)),
            PersistResult::Inserted(tx) => tx,
        };
        tx.commit().await.map_err(|error| {
            IngestError::Internal(format!("error: commit transaction: {error}"))
        })?;
        return Ok(accepted(
            event,
            serde_json::json!({ "status": "refusal_recorded" }),
        ));
    };

    if row.claimed_by.as_deref() != Some(signer.as_slice()) {
        return Err(IngestError::Rejected("not the claiming host".into()));
    }
    if let Some(recorded_claim) = hex_of(&row.claim_event_id) {
        if recorded_claim != claim_event_id {
            return Err(IngestError::Rejected(
                "invalid: result does not name the recorded claim event".into(),
            ));
        }
    }

    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => return Ok(duplicate(event)),
        PersistResult::Inserted(tx) => tx,
    };

    let result_value = serde_json::to_value(&result)
        .map_err(|error| IngestError::Internal(format!("error: encode result: {error}")))?;
    let status = if result.disposition == HostStepDisposition::LostOnRestart {
        HostStepStatus::Lost
    } else {
        HostStepStatus::Exited
    };
    let closed = state
        .db
        .record_host_step_result(HostStepResultParams {
            community_id: tenant.community(),
            run_id: row.run_id,
            step_id: &row.step_id,
            reporter: &signer,
            result_event_id: event.id.as_bytes(),
            status,
            exit_code: result.exit_code,
            disposition: disposition_str(result.disposition),
            timed_out: result.timed_out,
            duration_ms: result.duration_ms.map(|ms| ms.min(i64::MAX as u64) as i64),
            head_sha: result.head_sha.as_deref(),
            dirty: result.dirty,
            artifact_ref: result.artifact_path.as_deref(),
            result: &result_value,
        })
        .await
        .map_err(|error| {
            IngestError::Internal(format!("error: db record_host_step_result: {error}"))
        })?;

    let Some(closed) = closed else {
        // The conditional UPDATE touched nothing: the row is no longer
        // `claimed`. The claiming host repeating its result is a duplicate;
        // anything else is not the claiming host's report.
        let latest = state
            .db
            .get_host_step(tenant.community(), row.run_id, &row.step_id)
            .await
            .map_err(|error| IngestError::Internal(format!("error: db get_host_step: {error}")))?;
        let already_closed = matches!(latest.status, HostStepStatus::Exited | HostStepStatus::Lost);
        if latest.claimed_by.as_deref() == Some(signer.as_slice()) && already_closed {
            tx.commit().await.map_err(|error| {
                IngestError::Internal(format!("error: commit transaction: {error}"))
            })?;
            // The row closed on an earlier delivery, but the resume runs
            // post-commit in a spawned task: a crash there leaves the run
            // parked on a closed row. The claiming host's retry is the one
            // signal that reaches us, so re-arm the resume if the run is
            // still waiting; the resume itself is idempotent on the trace.
            let run = state
                .db
                .get_workflow_run(tenant.community(), row.run_id)
                .await
                .map_err(|error| IngestError::Internal(format!("error: db get_run: {error}")))?;
            if run.status == RunStatus::WaitingHost {
                let output = latest
                    .result
                    .as_ref()
                    .and_then(|value| serde_json::from_value(value.clone()).ok())
                    .map(|stored: buzz_core::host_step::HostStepResult| {
                        buzz_workflow::host_step_output(&stored)
                    });
                if let Some(output) = output {
                    spawn_resume(state, tenant.community(), &latest, output);
                }
            }
            return Ok(duplicate(event));
        }
        return Err(IngestError::Rejected("not the claiming host".into()));
    };

    tx.commit()
        .await
        .map_err(|error| IngestError::Internal(format!("error: commit transaction: {error}")))?;

    // Echo the accepted result as a relay-signed kind:46014. A failure here
    // is logged, not fatal: the row already records the result event, and
    // the run must still resume.
    let exited = HostStepExited {
        schema: HOST_STEP_SCHEMA.into(),
        result: result.clone(),
        claimed_by: hex::encode(&signer),
        result_event_id: event.id.to_hex(),
    };
    match build_host_step_exited(&exited) {
        Ok((tags, content)) => {
            match publish_relay_event(
                state,
                tenant.community(),
                KIND_WORKFLOW_HOST_STEP_EXITED,
                tags,
                content,
                channel_id,
            )
            .await
            {
                Ok(exited_event_id) => match hex::decode(&exited_event_id) {
                    Ok(bytes) => {
                        if let Err(error) = state
                            .db
                            .set_host_step_exited_event(
                                tenant.community(),
                                row.run_id,
                                &row.step_id,
                                &bytes,
                            )
                            .await
                        {
                            tracing::error!(
                                run_id = %row.run_id,
                                step_id = %row.step_id,
                                "host step: could not record kind:46014 id: {error}"
                            );
                        }
                    }
                    Err(error) => tracing::error!(
                        run_id = %row.run_id,
                        "host step: kind:46014 id is not hex: {error}"
                    ),
                },
                Err(error) => tracing::error!(
                    run_id = %row.run_id,
                    step_id = %row.step_id,
                    "host step: could not publish kind:46014: {error}"
                ),
            }
        }
        Err(error) => tracing::error!(
            run_id = %row.run_id,
            step_id = %row.step_id,
            "host step: could not build kind:46014: {error}"
        ),
    }

    // Resume post-commit, off the ingest path. A `lost_on_restart` or
    // `timed_out` result resumes too: later steps see `exit_code` null / 124
    // and decide with `if:`.
    spawn_resume(
        state,
        tenant.community(),
        &closed,
        buzz_workflow::host_step_output(&result),
    );

    Ok(accepted(
        event,
        serde_json::json!({
            "status": status,
            "run_id": row.run_id.to_string(),
            "step_id": row.step_id,
        }),
    ))
}

/// Resume the run a closed host step belongs to, post-commit and off the
/// ingest path. Idempotent: the resume checks the run is still `waiting_host`
/// and skips a trace entry it already wrote.
fn spawn_resume(
    state: &Arc<AppState>,
    community_id: CommunityId,
    closed: &buzz_db::workflow::HostStepRecord,
    output: serde_json::Value,
) {
    let engine = Arc::clone(&state.workflow_engine);
    let db = state.db.clone();
    let run_id = closed.run_id;
    let workflow_id = closed.workflow_id;
    let step_index = closed.step_index.max(0) as usize;
    tokio::spawn(async move {
        resume_workflow_after_host_step(
            engine,
            db,
            community_id,
            run_id,
            workflow_id,
            step_index,
            output,
        )
        .await;
    });
}

/// Rebuild the `step_id → output` map earlier steps left in a run's trace.
///
/// Entries without an `output` key — a parked step's `awaiting_approval` or
/// `requested_on_host` marker — contribute nothing.
pub fn outputs_from_trace(trace: &serde_json::Value) -> HashMap<String, serde_json::Value> {
    let mut outputs = HashMap::new();
    if let Some(entries) = trace.as_array() {
        for entry in entries {
            if let (Some(step_id), Some(output)) = (
                entry.get("step_id").and_then(|v| v.as_str()),
                entry.get("output"),
            ) {
                outputs.insert(step_id.to_string(), output.clone());
            }
        }
    }
    outputs
}

/// The trace entry a completed host step leaves behind.
pub fn host_step_trace_entry(step_id: &str, output: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "step_id": step_id,
        "status": TRACE_HOST_STEP_COMPLETED,
        "output": output,
    })
}

/// Resume a run parked as `waiting_host` once its step at `step_index` has a
/// result.
///
/// The trace is written back with the host step's `completed` entry before
/// the next step runs, so a crash between the two leaves a run whose trace
/// says what the host reported. Then `execute_from_step(step_index + 1)`
/// with the host step's output in `initial_outputs`, and `finalize_run`.
pub async fn resume_workflow_after_host_step(
    engine: Arc<buzz_workflow::WorkflowEngine>,
    db: buzz_db::Db,
    community_id: CommunityId,
    run_id: Uuid,
    workflow_id: Uuid,
    step_index: usize,
    output: serde_json::Value,
) {
    let run = match db.get_workflow_run(community_id, run_id).await {
        Ok(run) => run,
        Err(error) => {
            tracing::error!("resume_host_step: failed to fetch run {run_id}: {error}");
            return;
        }
    };
    if run.status != RunStatus::WaitingHost {
        tracing::warn!(
            "resume_host_step: run {run_id} has status '{}', expected 'waiting_host'",
            run.status
        );
        return;
    }

    let workflow = match db.get_workflow(community_id, workflow_id).await {
        Ok(workflow) => workflow,
        Err(error) => {
            tracing::error!("resume_host_step: failed to fetch workflow {workflow_id}: {error}");
            return;
        }
    };
    let def: buzz_workflow::WorkflowDef = match serde_json::from_value(workflow.definition.clone())
    {
        Ok(def) => def,
        Err(error) => {
            tracing::error!("resume_host_step: failed to parse workflow definition: {error}");
            if let Err(db_err) = db
                .update_workflow_run(
                    community_id,
                    run_id,
                    RunStatus::Failed,
                    run.current_step,
                    &run.execution_trace,
                    Some(buzz_db::workflow::WorkflowRunFailure {
                        code: "invalid_definition",
                        message: &format!("definition parse error: {error}"),
                    }),
                )
                .await
            {
                tracing::error!("resume_host_step: failed to mark run as failed: {db_err}");
            }
            return;
        }
    };
    let Some(step_id) = def.steps.get(step_index).map(|step| step.id.clone()) else {
        tracing::error!(
            "resume_host_step: run {run_id} has no step at index {step_index} in its definition"
        );
        return;
    };

    let mut initial_outputs = outputs_from_trace(&run.execution_trace);
    initial_outputs.insert(step_id.clone(), output.clone());

    let mut trace = run.execution_trace.as_array().cloned().unwrap_or_default();
    // Idempotent: a re-armed resume after a crash finds the entry already
    // written and must not append it twice.
    let already_recorded = trace.last().is_some_and(|entry| {
        entry.get("step_id").and_then(serde_json::Value::as_str) == Some(step_id.as_str())
            && entry.get("status").and_then(serde_json::Value::as_str) == Some("completed")
    });
    if !already_recorded {
        trace.push(host_step_trace_entry(&step_id, &output));
    }
    if let Err(error) = db
        .update_workflow_run(
            community_id,
            run_id,
            RunStatus::WaitingHost,
            step_index as i32,
            &serde_json::Value::Array(trace.clone()),
            None,
        )
        .await
    {
        tracing::error!(
            "resume_host_step: failed to record host step output for {run_id}: {error}"
        );
        return;
    }

    let trigger_ctx: TriggerContext = run
        .trigger_context
        .as_ref()
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();

    let result = buzz_workflow::executor::execute_from_step(
        &engine,
        community_id,
        run_id,
        &def,
        &trigger_ctx,
        step_index + 1,
        Some(initial_outputs),
    )
    .await;
    engine
        .finalize_run(community_id, run_id, result, Some(trace))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outputs_from_trace_skips_parked_markers() {
        let trace = serde_json::json!([
            {"step_id": "first", "status": "completed", "output": {"event_id": "abc"}},
            {"step_id": "gate", "status": "awaiting_approval", "approval_ref": "ff"},
            {"step_id": "build", "status": "requested_on_host"},
        ]);
        let outputs = outputs_from_trace(&trace);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs["first"]["event_id"], "abc");
        assert!(outputs_from_trace(&serde_json::json!(null)).is_empty());
    }

    #[test]
    fn host_step_trace_entry_carries_output_for_reconstruction() {
        let output = serde_json::json!({"exit_code": 1, "disposition": "exited"});
        let entry = host_step_trace_entry("build", &output);
        assert_eq!(entry["status"], TRACE_HOST_STEP_COMPLETED);
        let outputs = outputs_from_trace(&serde_json::json!([entry]));
        assert_eq!(outputs["build"]["exit_code"], 1);
    }

    #[test]
    fn disposition_words_match_the_wire_enum() {
        for disposition in [
            HostStepDisposition::Exited,
            HostStepDisposition::TimedOut,
            HostStepDisposition::LostOnRestart,
            HostStepDisposition::Refused,
        ] {
            let wire = serde_json::to_value(disposition).expect("encode");
            assert_eq!(wire, disposition_str(disposition));
        }
    }

    use buzz_core::channel::{ChannelType, ChannelVisibility, MemberRole};
    use buzz_core::host_step::{build_host_step_claim, build_host_step_result, HostStepClaim};
    use buzz_core::kind::{KIND_HOST_STEP_CLAIM, KIND_HOST_STEP_RESULT};
    use buzz_db::workflow::CreateHostStepParams;
    use buzz_db::CreateCommunityWithOwnerResult;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn signed(keys: &Keys, kind: u32, tags: Vec<Vec<String>>, content: String) -> Event {
        let tags: Vec<Tag> = tags
            .into_iter()
            .map(|tag| Tag::parse(tag).expect("tag"))
            .collect();
        EventBuilder::new(Kind::from(kind as u16), content)
            .tags(tags)
            .sign_with_keys(keys)
            .expect("sign")
    }

    fn http_auth(keys: &Keys) -> IngestAuth {
        IngestAuth::Http {
            pubkey: keys.public_key(),
            scopes: vec![buzz_auth::Scope::MessagesWrite],
            auth_method: super::super::ingest::HttpAuthMethod::Nip98,
        }
    }

    fn rejection(result: Result<IngestResult, IngestError>) -> String {
        match result {
            Err(IngestError::Rejected(message)) => message,
            Err(other) => panic!("unexpected refusal: {other:?}"),
            Ok(accepted) => panic!("expected a rejection, got {}", accepted.message),
        }
    }

    /// Spec § 5.4: exactly one host executes; the loser's ingest response
    /// says `claimed by <host>`; only the claiming host's result closes the
    /// row, and the run resumes.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn second_claim_is_told_the_winner_and_only_the_winner_reports() {
        let state = crate::state::tests::test_state().await;
        let owner = Keys::generate();
        let host_a = Keys::generate();
        let host_b = Keys::generate();
        let owner_bytes = owner.public_key().to_bytes().to_vec();

        let host = format!("host-steps-{}.example", Uuid::new_v4().simple());
        let community = match state
            .db
            .create_community_with_owner(&host, &owner.public_key().to_hex())
            .await
            .expect("create community")
        {
            CreateCommunityWithOwnerResult::Created(rec) => rec.id,
            other => panic!("expected fresh community, got {other:?}"),
        };
        let tenant = TenantContext::resolved(community, host);

        let channel = state
            .db
            .create_channel(
                community,
                "actions",
                ChannelType::Stream,
                ChannelVisibility::Private,
                None,
                &owner_bytes,
                None,
                None,
            )
            .await
            .expect("create channel");
        for keys in [&host_a, &host_b] {
            let bytes = keys.public_key().to_bytes().to_vec();
            state
                .db
                .ensure_user(community, &bytes)
                .await
                .expect("ensure host user");
            state
                .db
                .add_member(
                    community,
                    channel.id,
                    &bytes,
                    MemberRole::Member,
                    Some(&owner_bytes),
                )
                .await
                .expect("add host member");
        }

        state
            .db
            .ensure_user(community, &owner_bytes)
            .await
            .expect("ensure owner user");
        let yaml = format!(
            "name: nightly\nproject: '30621:{}:pulse'\ntrigger:\n  on: manual\nsteps:\n  - id: build\n    action: run_on_host\n    command: [\"true\"]\n",
            "1".repeat(64)
        );
        let (_, canonical) = buzz_workflow::WorkflowEngine::parse_yaml(&yaml).expect("parse");
        let definition: serde_json::Value = serde_json::from_str(&canonical).expect("json");
        let hash = buzz_workflow::hash_definition_value(&definition).expect("hash");
        let workflow_id = Uuid::new_v4();
        state
            .db
            .upsert_workflow(
                community,
                workflow_id,
                Some(channel.id),
                &owner_bytes,
                "nightly",
                &canonical,
                &hash,
                None,
            )
            .await
            .expect("upsert workflow");

        let trigger_ctx = serde_json::to_value(TriggerContext::default()).expect("ctx");
        let run_id = state
            .db
            .create_workflow_run(community, workflow_id, None, Some(&trigger_ctx))
            .await
            .expect("create run");
        let parked_trace = serde_json::json!([
            {"step_id": "build", "status": "requested_on_host"}
        ]);
        state
            .db
            .update_workflow_run(
                community,
                run_id,
                RunStatus::WaitingHost,
                0,
                &parked_trace,
                None,
            )
            .await
            .expect("park run");
        state
            .db
            .create_host_step(CreateHostStepParams {
                community_id: community,
                run_id,
                workflow_id,
                step_id: "build",
                step_index: 0,
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            })
            .await
            .expect("create host step");
        let requested_id = [0xaa; 32];
        state
            .db
            .set_host_step_requested_event(community, run_id, "build", &requested_id)
            .await
            .expect("record request id");

        let claim_for = |keys: &Keys| {
            let claim = HostStepClaim {
                schema: HOST_STEP_SCHEMA.into(),
                run_id: run_id.to_string(),
                step_id: "build".into(),
                requested_event_id: hex::encode(requested_id),
                channel_id: channel.id.to_string(),
                host: Default::default(),
            };
            let (tags, content) = build_host_step_claim(&claim).expect("build claim");
            signed(keys, KIND_HOST_STEP_CLAIM, tags, content)
        };

        // A claims first and wins.
        let claim_a = claim_for(&host_a);
        let won = handle_host_step_claim(&tenant, &state, &claim_a, &http_auth(&host_a))
            .await
            .expect("claim by A");
        assert!(won.accepted);
        assert_eq!(won.message, r#"response:{"status":"claimed"}"#);

        // B's claim names the winner.
        let claim_b = claim_for(&host_b);
        let lost =
            rejection(handle_host_step_claim(&tenant, &state, &claim_b, &http_auth(&host_b)).await);
        assert_eq!(lost, format!("claimed by {}", host_a.public_key().to_hex()));

        // A repeating its claim is a harmless duplicate.
        let again = handle_host_step_claim(&tenant, &state, &claim_a, &http_auth(&host_a))
            .await
            .expect("repeat claim by A");
        assert!(again.message.starts_with("duplicate:"));

        let result_for = |keys: &Keys, claim_event_id: &Event| {
            let result = buzz_core::host_step::HostStepResult {
                schema: HOST_STEP_SCHEMA.into(),
                run_id: run_id.to_string(),
                step_id: "build".into(),
                requested_event_id: hex::encode(requested_id),
                claim_event_id: Some(claim_event_id.id.to_hex()),
                channel_id: channel.id.to_string(),
                disposition: HostStepDisposition::Exited,
                exit_code: Some(0),
                refusal: None,
                timed_out: false,
                duration_ms: Some(42),
                head_sha: Some("b".repeat(40)),
                dirty: Some(false),
                checkout: None,
                stdout_tail: "ok".into(),
                stderr_tail: String::new(),
                truncated: false,
                artifact_path: None,
                routed: None,
                artifacts: Vec::new(),
            };
            let (tags, content) = build_host_step_result(&result).expect("build result");
            signed(keys, KIND_HOST_STEP_RESULT, tags, content)
        };

        // B cannot report on A's claim.
        let result_b = result_for(&host_b, &claim_b);
        let refused = rejection(
            handle_host_step_result(&tenant, &state, &result_b, &http_auth(&host_b)).await,
        );
        assert_eq!(refused, "not the claiming host");

        // A's result closes the row and resumes the run.
        let result_a = result_for(&host_a, &claim_a);
        let closed = handle_host_step_result(&tenant, &state, &result_a, &http_auth(&host_a))
            .await
            .expect("result by A");
        assert!(closed.accepted);
        assert!(
            closed.message.contains(r#""status":"exited""#),
            "{}",
            closed.message
        );

        let row = state
            .db
            .get_host_step(community, run_id, "build")
            .await
            .expect("row");
        assert_eq!(row.status, HostStepStatus::Exited);
        assert_eq!(
            row.claimed_by.as_deref(),
            Some(host_a.public_key().to_bytes().as_slice())
        );
        assert_eq!(
            row.result_event_id.as_deref(),
            Some(result_a.id.as_bytes().as_slice())
        );
        assert_eq!(row.exit_code, Some(0));
        assert_eq!(row.disposition.as_deref(), Some("exited"));

        // The resume runs off the ingest path; the only step was the host
        // step, so the run completes with the host output in its trace.
        let mut run = state
            .db
            .get_workflow_run(community, run_id)
            .await
            .expect("run");
        for _ in 0..50 {
            if run.status != RunStatus::WaitingHost {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            run = state
                .db
                .get_workflow_run(community, run_id)
                .await
                .expect("run");
        }
        assert_eq!(
            run.status,
            RunStatus::Completed,
            "trace: {}",
            run.execution_trace
        );
        let outputs = outputs_from_trace(&run.execution_trace);
        assert_eq!(outputs["build"]["exit_code"], 0);
        assert_eq!(outputs["build"]["head_sha"], "b".repeat(40));

        // A repeating its result after the row closed is a duplicate, and
        // the kind:46014 echo was recorded on the row.
        let repeat = handle_host_step_result(&tenant, &state, &result_a, &http_auth(&host_a))
            .await
            .expect("repeat result by A");
        assert!(repeat.message.starts_with("duplicate:"));
        let row = state
            .db
            .get_host_step(community, run_id, "build")
            .await
            .expect("row");
        assert!(row.exited_event_id.is_some(), "kind:46014 id recorded");
    }
}
