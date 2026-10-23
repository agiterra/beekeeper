//! Durable workflow dispatch worker (GitHub-plugin slice, plan § 5).
//!
//! Every committed kind:50000 application event and kind:46008 CI result has
//! a `workflow_dispatch_queue` row, written by the `events` trigger in the
//! insert's own transaction (migration 0047). This worker is the only thing
//! that turns those rows into workflow runs. What it guarantees is durable,
//! at-most-once ADMISSION — a committed run row per (workflow, event) — not
//! execution:
//!
//! 1. claim a batch from one community (lease, `SKIP LOCKED`), no larger than
//!    the engine's free run permits;
//! 2. for each event, [`WorkflowEngine::dispatch_claimed_event`] freezes the
//!    candidates on the first claim and admits each one at most once
//!    (`workflow_runs_dispatch_once`);
//! 3. all decided → `done`; any retryable → back to `pending` with backoff;
//!    the 8th failed attempt → `failed` with the reason kept; an event that
//!    can never be dispatched → `failed` with that reason.
//!
//! A crash anywhere leaves the row `claimed`; its lease expires and the next
//! claim resumes it. Admission is idempotent, so a resumed row never creates
//! a second run for a workflow that was already admitted. It also never
//! starts the existing one: a run is started only by the process that
//! created it, so a crash after the run commits and before `execute_run`
//! marks it `running` leaves it `pending`, and nothing restarts it yet. The
//! sweep reports such runs ([`STRANDED_AFTER`]); it does not recover them.
//!
//! Retention: the periodic sweep deletes `done` rows 7 days after they
//! finish and `failed` rows after 30 days (`prune_dispatch_queue`, bounded
//! per call), and fails rows stranded at the attempt cap.
//!
//! [`WorkflowEngine::dispatch_claimed_event`]: beekeeper_workflow::WorkflowEngine::dispatch_claimed_event

use std::sync::Arc;
use std::time::Duration;

use beekeeper_core::CommunityId;
use beekeeper_db::workflow_dispatch::{
    dispatch_backoff, ClaimedDispatchBatch, RescheduleOutcome, DONE_RETENTION, FAILED_RETENTION,
    STRANDED_REPORT_LIMIT,
};
use chrono::{TimeDelta, Utc};
use tracing::{error, info, warn};

use crate::state::AppState;

/// Lease on a claimed batch. Long enough for one batch's admissions, short
/// enough that a crashed pod's rows resume within a minute.
pub const CLAIM_SECS: i64 = 60;
/// Most events claimed in one batch.
pub const BATCH_LIMIT: usize = 16;
/// Poll interval when idle; a commit of a dispatchable event wakes the
/// worker earlier through `AppState::workflow_dispatch_notify`.
pub const IDLE_POLL: Duration = Duration::from_secs(1);
/// How often the exhausted-row and retention sweep runs.
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(60);
/// A dispatch-admitted run still `pending` and unstarted this long after it
/// was created is reported as stranded by the sweep. Far above the time
/// between a run's commit and `execute_run` marking it `running`.
pub const STRANDED_AFTER: Duration = Duration::from_secs(5 * 60);

/// Start the worker. Called unconditionally at startup.
pub fn spawn(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    info!("workflow dispatch worker started");
    tokio::spawn(run(state))
}

/// The worker loop: sweep periodically, drain due batches, then wait for a
/// notify or the idle poll.
pub async fn run(state: Arc<AppState>) {
    let mut last_sweep: Option<tokio::time::Instant> = None;
    loop {
        if last_sweep.is_none_or(|at| at.elapsed() >= SWEEP_INTERVAL) {
            sweep(&state).await;
            last_sweep = Some(tokio::time::Instant::now());
        }
        match run_once(&state, None).await {
            Ok(processed) if processed > 0 => continue,
            Ok(_) => {}
            Err(e) => error!("workflow dispatch claim failed: {e}"),
        }
        tokio::select! {
            _ = tokio::time::sleep(IDLE_POLL) => {}
            _ = state.workflow_dispatch_notify.notified() => {}
        }
    }
}

async fn sweep(state: &AppState) {
    match state.db.fail_exhausted_dispatches().await {
        Ok(0) => {}
        Ok(failed) => warn!(
            failed,
            "workflow dispatch: rows stranded at the attempt cap failed"
        ),
        Err(e) => error!("workflow dispatch exhausted-row sweep failed: {e}"),
    }
    match state
        .db
        .prune_dispatch_queue(DONE_RETENTION, FAILED_RETENTION)
        .await
    {
        Ok((0, 0)) => {}
        Ok((done, failed)) => info!(done, failed, "workflow dispatch retention pruned rows"),
        Err(e) => error!("workflow dispatch retention sweep failed: {e}"),
    }
    match state
        .db
        .list_stranded_dispatch_runs(STRANDED_AFTER, STRANDED_REPORT_LIMIT)
        .await
    {
        Ok((0, _)) => {}
        Ok((total, runs)) => {
            let ids = runs
                .iter()
                .map(|run| format!("{}/{}", run.community_id.as_uuid(), run.run_id))
                .collect::<Vec<_>>()
                .join(", ");
            warn!(
                total,
                listed = runs.len(),
                runs = %ids,
                "workflow dispatch: admitted runs never started (creator died before                  execute_run); not recovered"
            );
        }
        Err(e) => error!("workflow dispatch stranded-run check failed: {e}"),
    }
}

/// Claim and process one batch, optionally from one community only.
/// Returns how many events were processed (0 when nothing was due, or when
/// no run permit is free).
pub async fn run_once(
    state: &Arc<AppState>,
    only_community: Option<CommunityId>,
) -> beekeeper_db::Result<usize> {
    let permits = state
        .workflow_engine
        .available_run_permits()
        .min(BATCH_LIMIT);
    if permits == 0 {
        return Ok(0);
    }
    let until = Utc::now() + TimeDelta::seconds(CLAIM_SECS);
    let Some(batch) = state
        .db
        .claim_dispatch_batch_in(only_community, permits as i64, until)
        .await?
    else {
        return Ok(0);
    };
    let processed = batch.jobs.len();
    process_batch(state, batch).await;
    Ok(processed)
}

async fn process_batch(state: &Arc<AppState>, batch: ClaimedDispatchBatch) {
    let community = batch.community;
    for job in batch.jobs {
        let event_id = job.event.event.id.as_bytes().to_vec();
        let event_hex = job.event.event.id.to_hex();
        let report = state
            .workflow_engine
            .dispatch_claimed_event(community, batch.claim_id, &job.event, job.candidates_frozen)
            .await;
        let result = match report {
            Ok(report) if report.terminal.is_some() => {
                let reason = report.terminal.unwrap_or_default();
                warn!(event = %event_hex, %reason, "workflow dispatch: event cannot be dispatched");
                state
                    .db
                    .fail_dispatch_terminal(community, &event_id, batch.claim_id, &reason)
                    .await
            }
            Ok(report) if report.is_done() => {
                if !report.admitted.is_empty() {
                    info!(
                        event = %event_hex,
                        admitted = report.admitted.len(),
                        skipped = report.skipped.len(),
                        "workflow dispatch: event dispatched"
                    );
                }
                state
                    .db
                    .complete_dispatch(community, &event_id, batch.claim_id)
                    .await
            }
            Ok(report) => {
                let error = report
                    .retryable
                    .iter()
                    .map(|(workflow, error)| format!("{workflow}: {error}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                reschedule(
                    state,
                    community,
                    &event_id,
                    batch.claim_id,
                    job.attempt,
                    &error,
                )
                .await
            }
            Err(e) => {
                reschedule(
                    state,
                    community,
                    &event_id,
                    batch.claim_id,
                    job.attempt,
                    &e.to_string(),
                )
                .await
            }
        };
        if let Err(e) = result {
            // The claim was lost (lease expired and another worker took the
            // row) or the DB failed; the row's lease lets it be resumed.
            error!(event = %event_hex, "workflow dispatch: could not settle claimed row: {e}");
        }
    }
}

async fn reschedule(
    state: &AppState,
    community: CommunityId,
    event_id: &[u8],
    claim_id: uuid::Uuid,
    attempt: i32,
    error: &str,
) -> beekeeper_db::Result<()> {
    match state
        .db
        .reschedule_dispatch(
            community,
            event_id,
            claim_id,
            error,
            dispatch_backoff(attempt),
        )
        .await?
    {
        RescheduleOutcome::Rescheduled => {
            warn!(attempt, "workflow dispatch: retrying event: {error}");
        }
        RescheduleOutcome::Exhausted => {
            error!(
                attempt,
                "workflow dispatch: attempts exhausted, row failed: {error}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "workflow_dispatch_runtime_tests.rs"]
mod tests;
