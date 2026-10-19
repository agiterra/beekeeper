//! Durable workflow dispatch queue (migration 0047).
//!
//! Every committed kind:50000 application event and kind:46008 CI result gets
//! one `workflow_dispatch_queue` row, written by the `events` AFTER INSERT
//! trigger inside the transaction that makes the event durable. A worker
//! claims rows, freezes the candidate workflows into
//! `workflow_dispatch_outcomes`, and admits each candidate at most once
//! through the partial unique index `workflow_runs_dispatch_once`.
//!
//! Rows are never deleted when attempts run out or the source event is gone:
//! they end in state `failed` with a reason, so a dispatch that did not
//! happen stays visible.

use std::time::Duration;

use beekeeper_core::kind::{KIND_APPLICATION_EVENT, KIND_CI_RESULT};
use beekeeper_core::{CommunityId, StoredEvent};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::{DbError, Result};

/// Event kinds the `enqueue_workflow_dispatch()` trigger enqueues.
///
/// Must stay identical to the `IN (...)` list in migration 0047 and in
/// `schema/schema.sql`; a unit test pins all three.
pub const DISPATCH_KINDS: [u32; 2] = [KIND_APPLICATION_EVENT, KIND_CI_RESULT];

/// Claims a queue row may take before it ends `failed` (plan § 5).
pub const MAX_DISPATCH_ATTEMPTS: i32 = 8;

/// `terminal_reason` of a row whose source event is absent or soft-deleted.
pub const REASON_SOURCE_EVENT_MISSING: &str = "source_event_missing";
/// `terminal_reason` of a row that used every attempt.
pub const REASON_ATTEMPTS_EXHAUSTED: &str = "attempts_exhausted";

const BACKOFF_BASE: Duration = Duration::from_secs(1);
const BACKOFF_CAP: Duration = Duration::from_secs(600);

/// Retry delay after the `attempt`-th claim failed: 1 s doubling, capped at
/// 10 minutes. `attempt` starts at 1.
pub fn dispatch_backoff(attempt: i32) -> Duration {
    let exponent = u32::try_from(attempt.saturating_sub(1))
        .unwrap_or(0)
        .min(16);
    BACKOFF_BASE
        .saturating_mul(1_u32 << exponent)
        .min(BACKOFF_CAP)
}

/// One claimed queue row whose source event still exists.
#[derive(Debug, Clone)]
pub struct DispatchJob {
    /// The non-deleted source event, loaded after the claim committed.
    pub event: StoredEvent,
    /// Event kind as recorded by the trigger.
    pub kind: i32,
    /// Claim number, starting at one.
    pub attempt: i32,
    /// Whether an earlier claim already froze this event's candidates.
    pub candidates_frozen: bool,
}

/// Rows claimed together from ONE community under one `claim_id`.
#[derive(Debug, Clone)]
pub struct ClaimedDispatchBatch {
    /// The community every job belongs to.
    pub community: CommunityId,
    /// Fencing token: completion and rescheduling only apply under it.
    pub claim_id: Uuid,
    /// Jobs whose source event was loaded. Rows whose event was missing were
    /// already moved to `failed` / [`REASON_SOURCE_EVENT_MISSING`].
    pub jobs: Vec<DispatchJob>,
}

/// Exclusively claim up to `limit` due rows from one community.
///
/// Due means `attempts < MAX_DISPATCH_ATTEMPTS`, `next_attempt_at <= now()`,
/// and `pending` or `claimed` with an expired lease, in a community whose
/// writes are allowed. Rows are taken `FOR UPDATE SKIP LOCKED`, so concurrent
/// claimers get disjoint rows and never wait on each other. Each claimed row
/// gets `attempts + 1`, a fresh `claim_id` and `lease_until`.
///
/// A claimed row whose source event is absent or soft-deleted is moved to
/// `failed` with [`REASON_SOURCE_EVENT_MISSING`] — never deleted — and is not
/// returned. Returns `None` when nothing was due (or every claimed row was
/// missing its event).
pub async fn claim_dispatch_batch(
    pool: &PgPool,
    limit: i64,
    lease_until: DateTime<Utc>,
) -> Result<Option<ClaimedDispatchBatch>> {
    claim_dispatch_batch_in(pool, None, limit, lease_until).await
}

/// [`claim_dispatch_batch`], optionally restricted to one community.
pub(crate) async fn claim_dispatch_batch_in(
    pool: &PgPool,
    only_community: Option<CommunityId>,
    limit: i64,
    lease_until: DateTime<Utc>,
) -> Result<Option<ClaimedDispatchBatch>> {
    let claim_id = Uuid::new_v4();
    let rows = sqlx::query(
        r#"
        WITH target AS (
            SELECT community_id
            FROM workflow_dispatch_queue
            WHERE attempts < $3
              AND next_attempt_at <= now()
              AND (state = 'pending' OR (state = 'claimed' AND lease_until < now()))
              AND ($5::uuid IS NULL OR community_id = $5)
              AND community_write_allowed(community_id)
            ORDER BY next_attempt_at, created_at
            LIMIT 1
        ),
        candidates AS (
            SELECT q.community_id, q.event_id
            FROM workflow_dispatch_queue q
            JOIN target t ON q.community_id = t.community_id
            WHERE q.attempts < $3
              AND q.next_attempt_at <= now()
              AND (q.state = 'pending' OR (q.state = 'claimed' AND q.lease_until < now()))
            ORDER BY q.next_attempt_at, q.created_at
            FOR UPDATE OF q SKIP LOCKED
            LIMIT $4
        )
        UPDATE workflow_dispatch_queue q
        SET state = 'claimed', claim_id = $1, lease_until = $2, attempts = q.attempts + 1
        FROM candidates c
        WHERE q.community_id = c.community_id AND q.event_id = c.event_id
        RETURNING q.community_id, q.event_id, q.kind, q.attempts, q.candidates_frozen
        "#,
    )
    .bind(claim_id)
    .bind(lease_until)
    .bind(MAX_DISPATCH_ATTEMPTS)
    .bind(limit)
    .bind(only_community.map(|c| *c.as_uuid()))
    .fetch_all(pool)
    .await?;
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let community = CommunityId::from_uuid(first.try_get("community_id")?);
    let mut claimed = std::collections::HashMap::with_capacity(rows.len());
    for row in &rows {
        let event_id: Vec<u8> = row.try_get("event_id")?;
        let kind: i32 = row.try_get("kind")?;
        let attempt: i32 = row.try_get("attempts")?;
        let frozen: bool = row.try_get("candidates_frozen")?;
        claimed.insert(event_id, (kind, attempt, frozen));
    }
    let ids: Vec<&[u8]> = claimed.keys().map(Vec::as_slice).collect();
    let events = crate::event::get_events_by_ids(pool, community, &ids).await?;
    let mut jobs = Vec::with_capacity(events.len());
    for event in events {
        let Some((kind, attempt, candidates_frozen)) =
            claimed.remove(event.event.id.as_bytes().as_slice())
        else {
            return Err(DbError::InvalidData(format!(
                "dispatch claim loaded event {} it did not claim",
                event.event.id.to_hex()
            )));
        };
        jobs.push(DispatchJob {
            event,
            kind,
            attempt,
            candidates_frozen,
        });
    }
    // Whatever was not loaded has no live source event. That is terminal and
    // stays visible: unlike the push matcher, a dispatch row is never deleted.
    let missing: Vec<Vec<u8>> = claimed.into_keys().collect();
    if !missing.is_empty() {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "UPDATE workflow_dispatch_outcomes SET disposition = 'failed', reason = $3, \
             updated_at = now() \
             WHERE community_id = $1 AND disposition = 'pending' AND event_id = ANY($2)",
        )
        .bind(community.as_uuid())
        .bind(&missing)
        .bind(REASON_SOURCE_EVENT_MISSING)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE workflow_dispatch_queue SET state = 'failed', terminal_reason = $4, \
             finished_at = now(), lease_until = NULL \
             WHERE community_id = $1 AND claim_id = $2 AND state = 'claimed' \
               AND event_id = ANY($3)",
        )
        .bind(community.as_uuid())
        .bind(claim_id)
        .bind(&missing)
        .bind(REASON_SOURCE_EVENT_MISSING)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
    }
    if jobs.is_empty() {
        return Ok(None);
    }
    Ok(Some(ClaimedDispatchBatch {
        community,
        claim_id,
        jobs,
    }))
}

/// Result of [`freeze_candidates`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeOutcome {
    /// This call wrote the candidate set.
    Frozen,
    /// An earlier claim already froze it; nothing was written. Retries never
    /// pick up workflows enabled after the first claim.
    AlreadyFrozen,
}

/// Record the candidate workflows of a claimed event, once.
///
/// In one transaction: lock the queue row under `claim_id`, insert one
/// `pending` outcome per workflow, set `candidates_frozen`. A second call
/// (any claim) is [`FreezeOutcome::AlreadyFrozen`] and writes nothing. A
/// claim that is no longer held is an error, and nothing is written.
pub async fn freeze_candidates(
    pool: &PgPool,
    community: CommunityId,
    event_id: &[u8],
    claim_id: Uuid,
    workflow_ids: &[Uuid],
) -> Result<FreezeOutcome> {
    let mut tx = pool.begin().await?;
    let frozen: Option<bool> = sqlx::query_scalar(
        "SELECT candidates_frozen FROM workflow_dispatch_queue \
         WHERE community_id = $1 AND event_id = $2 AND claim_id = $3 AND state = 'claimed' \
         FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(claim_id)
    .fetch_optional(&mut *tx)
    .await?;
    match frozen {
        None => Err(claim_lost(community, event_id, claim_id)),
        Some(true) => {
            tx.commit().await?;
            Ok(FreezeOutcome::AlreadyFrozen)
        }
        Some(false) => {
            sqlx::query(
                "INSERT INTO workflow_dispatch_outcomes (community_id, event_id, workflow_id) \
                 SELECT $1, $2, unnest($3::uuid[]) ON CONFLICT DO NOTHING",
            )
            .bind(community.as_uuid())
            .bind(event_id)
            .bind(workflow_ids)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE workflow_dispatch_queue SET candidates_frozen = true \
                 WHERE community_id = $1 AND event_id = $2",
            )
            .bind(community.as_uuid())
            .bind(event_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(FreezeOutcome::Frozen)
        }
    }
}

fn claim_lost(community: CommunityId, event_id: &[u8], claim_id: Uuid) -> DbError {
    DbError::AccessDenied(format!(
        "dispatch claim {claim_id} no longer holds event {} in community {community}",
        hex::encode(event_id)
    ))
}

/// One frozen candidate of a dispatch event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// The candidate workflow.
    pub workflow_id: Uuid,
    /// `pending`, `admitted`, `skipped` or `failed`.
    pub disposition: String,
    /// The run, iff `admitted`.
    pub run_id: Option<Uuid>,
    /// Skip / failure reason, or the last retryable error while `pending`.
    pub reason: Option<String>,
    /// Retryable errors recorded against this candidate.
    pub attempts: i32,
}

/// The frozen candidates of one event, ordered by workflow id.
pub async fn list_dispatch_outcomes(
    pool: &PgPool,
    community: CommunityId,
    event_id: &[u8],
) -> Result<Vec<DispatchOutcome>> {
    let rows = sqlx::query(
        "SELECT workflow_id, disposition, run_id, reason, attempts \
         FROM workflow_dispatch_outcomes WHERE community_id = $1 AND event_id = $2 \
         ORDER BY workflow_id",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(DispatchOutcome {
                workflow_id: row.try_get("workflow_id")?,
                disposition: row.try_get("disposition")?,
                run_id: row.try_get("run_id")?,
                reason: row.try_get("reason")?,
                attempts: row.try_get("attempts")?,
            })
        })
        .collect()
}

/// A non-admission result for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutcomeUpdate {
    /// Final: the workflow will never run for this event (definition parse
    /// failure, trigger or filter miss, owner authority, definition changed,
    /// disabled).
    Skipped(String),
    /// Transient: stays `pending`, `attempts + 1`, the error kept as reason.
    Retryable(String),
}

/// Record a skip or a retryable error for a `pending` candidate.
///
/// Returns `false` when the candidate is not `pending` (already decided, or
/// never frozen); a decided outcome is never overwritten.
pub async fn record_outcome(
    pool: &PgPool,
    community: CommunityId,
    event_id: &[u8],
    workflow_id: Uuid,
    update: &OutcomeUpdate,
) -> Result<bool> {
    let (disposition, increment, reason) = match update {
        OutcomeUpdate::Skipped(reason) => ("skipped", 0, reason),
        OutcomeUpdate::Retryable(error) => ("pending", 1, error),
    };
    let updated = sqlx::query(
        "UPDATE workflow_dispatch_outcomes \
         SET disposition = $4, reason = $5, attempts = attempts + $6, updated_at = now() \
         WHERE community_id = $1 AND event_id = $2 AND workflow_id = $3 \
           AND disposition = 'pending'",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(workflow_id)
    .bind(disposition)
    .bind(reason)
    .bind(increment)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

/// Mark a claimed row `done`. Only the holder of `claim_id` can; a lost
/// claim is an error and changes nothing.
pub async fn complete_dispatch(
    pool: &PgPool,
    community: CommunityId,
    event_id: &[u8],
    claim_id: Uuid,
) -> Result<()> {
    let updated = sqlx::query(
        "UPDATE workflow_dispatch_queue \
         SET state = 'done', finished_at = now(), lease_until = NULL, last_error = NULL \
         WHERE community_id = $1 AND event_id = $2 AND claim_id = $3 AND state = 'claimed'",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(claim_id)
    .execute(pool)
    .await?
    .rows_affected();
    if updated == 1 {
        Ok(())
    } else {
        Err(claim_lost(community, event_id, claim_id))
    }
}

/// What [`reschedule_dispatch`] did with the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescheduleOutcome {
    /// Back to `pending`, due after the backoff.
    Rescheduled,
    /// The attempt was the last one: the row is now `failed` /
    /// [`REASON_ATTEMPTS_EXHAUSTED`] and its pending outcomes `failed`.
    Exhausted,
}

/// Return a claimed row to `pending` after a retryable failure, or fail it
/// terminally when this was attempt [`MAX_DISPATCH_ATTEMPTS`].
pub async fn reschedule_dispatch(
    pool: &PgPool,
    community: CommunityId,
    event_id: &[u8],
    claim_id: Uuid,
    error: &str,
    backoff: Duration,
) -> Result<RescheduleOutcome> {
    let mut tx = pool.begin().await?;
    let attempts: Option<i32> = sqlx::query_scalar(
        "SELECT attempts FROM workflow_dispatch_queue \
         WHERE community_id = $1 AND event_id = $2 AND claim_id = $3 AND state = 'claimed' \
         FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(claim_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(attempts) = attempts else {
        return Err(claim_lost(community, event_id, claim_id));
    };
    if attempts >= MAX_DISPATCH_ATTEMPTS {
        fail_row_in(
            &mut tx,
            community,
            event_id,
            REASON_ATTEMPTS_EXHAUSTED,
            Some(error),
        )
        .await?;
        tx.commit().await?;
        return Ok(RescheduleOutcome::Exhausted);
    }
    sqlx::query(
        "UPDATE workflow_dispatch_queue \
         SET state = 'pending', lease_until = NULL, last_error = $3, \
             next_attempt_at = now() + make_interval(secs => $4) \
         WHERE community_id = $1 AND event_id = $2",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(error)
    .bind(backoff.as_secs_f64())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(RescheduleOutcome::Rescheduled)
}

/// Fail a claimed row terminally with `reason` (for example a source event
/// that cannot be dispatched). Its `pending` outcomes become `failed` with
/// the same reason. The row is kept.
pub async fn fail_dispatch_terminal(
    pool: &PgPool,
    community: CommunityId,
    event_id: &[u8],
    claim_id: Uuid,
    reason: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let held: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM workflow_dispatch_queue \
         WHERE community_id = $1 AND event_id = $2 AND claim_id = $3 AND state = 'claimed' \
         FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(claim_id)
    .fetch_optional(&mut *tx)
    .await?;
    if held.is_none() {
        return Err(claim_lost(community, event_id, claim_id));
    }
    fail_row_in(&mut tx, community, event_id, reason, None).await?;
    tx.commit().await?;
    Ok(())
}

async fn fail_row_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    community: CommunityId,
    event_id: &[u8],
    reason: &str,
    last_error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE workflow_dispatch_outcomes \
         SET disposition = 'failed', \
             reason = CASE WHEN reason IS NULL THEN $3 ELSE $3 || ': ' || reason END, \
             updated_at = now() \
         WHERE community_id = $1 AND event_id = $2 AND disposition = 'pending'",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE workflow_dispatch_queue \
         SET state = 'failed', terminal_reason = $3, finished_at = now(), lease_until = NULL, \
             last_error = COALESCE($4, last_error) \
         WHERE community_id = $1 AND event_id = $2",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(reason)
    .bind(last_error)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Sweep: fail every row that used its last attempt and is no longer held
/// (a worker died on the final claim, or a row was left `pending` at the
/// cap). Such rows are unclaimable, so without this they would sit forever.
/// They are marked `failed` / [`REASON_ATTEMPTS_EXHAUSTED`], never deleted.
/// Returns the number of rows failed.
pub async fn fail_exhausted_dispatches(pool: &PgPool) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let exhausted: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT community_id, event_id FROM workflow_dispatch_queue \
         WHERE attempts >= $1 \
           AND (state = 'pending' OR (state = 'claimed' AND lease_until < now())) \
           AND community_write_allowed(community_id) \
         FOR UPDATE SKIP LOCKED",
    )
    .bind(MAX_DISPATCH_ATTEMPTS)
    .fetch_all(&mut *tx)
    .await?;
    for (community, event_id) in &exhausted {
        fail_row_in(
            &mut tx,
            CommunityId::from_uuid(*community),
            event_id,
            REASON_ATTEMPTS_EXHAUSTED,
            None,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(exhausted.len() as u64)
}

/// Result of [`create_dispatch_run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchRunOutcome {
    /// This call created the run; the caller spawns its execution.
    Created(Uuid),
    /// A run for this (workflow, event) already existed — a retry after a
    /// crash, or a racing claimer. The caller must NOT spawn it again.
    Existing(Uuid),
    /// The published definition is no longer the one the caller parsed; no
    /// run exists and the outcome is unchanged (ledger 199: never retried).
    DefinitionChanged {
        /// The hash the caller named.
        expected: Vec<u8>,
        /// The hash published now.
        current: Vec<u8>,
    },
}

/// Admit one frozen candidate: create its run (at most once) and mark the
/// outcome `admitted`, in one transaction.
///
/// The run insert carries `dispatch_event_id = event_id` and uses
/// `ON CONFLICT ... DO NOTHING` on the partial unique index
/// `workflow_runs_dispatch_once`; on conflict the existing run id is
/// returned as [`DispatchRunOutcome::Existing`]. The outcome row must exist
/// (the candidate was frozen) and be `pending` or already `admitted` with the
/// same run; otherwise this is an error and nothing commits.
pub async fn create_dispatch_run(
    pool: &PgPool,
    community: CommunityId,
    event_id: &[u8],
    workflow_id: Uuid,
    trigger_context: Option<&serde_json::Value>,
    expected_definition_hash: &[u8],
) -> Result<DispatchRunOutcome> {
    let mut tx = pool.begin().await?;
    let created: Option<Uuid> = sqlx::query_scalar(
        r#"
        INSERT INTO workflow_runs
            (community_id, id, workflow_id, status, trigger_event_id, current_step,
             execution_trace, trigger_context, definition_hash, dispatch_event_id)
        SELECT $1, $2, $3, 'pending', $4, 0, '[]', $5, w.definition_hash, $4
        FROM workflows w
        WHERE w.community_id = $1 AND w.id = $3 AND w.definition_hash = $6
        ON CONFLICT (community_id, workflow_id, dispatch_event_id)
            WHERE dispatch_event_id IS NOT NULL
            DO NOTHING
        RETURNING id
        "#,
    )
    .bind(community.as_uuid())
    .bind(Uuid::new_v4())
    .bind(workflow_id)
    .bind(event_id)
    .bind(trigger_context)
    .bind(expected_definition_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let (outcome, run_id) = match created {
        Some(run_id) => (DispatchRunOutcome::Created(run_id), run_id),
        None => {
            let existing: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM workflow_runs \
                 WHERE community_id = $1 AND workflow_id = $2 AND dispatch_event_id = $3",
            )
            .bind(community.as_uuid())
            .bind(workflow_id)
            .bind(event_id)
            .fetch_optional(&mut *tx)
            .await?;
            match existing {
                Some(run_id) => (DispatchRunOutcome::Existing(run_id), run_id),
                None => {
                    let current: Option<Vec<u8>> = sqlx::query_scalar(
                        "SELECT definition_hash FROM workflows \
                         WHERE community_id = $1 AND id = $2",
                    )
                    .bind(community.as_uuid())
                    .bind(workflow_id)
                    .fetch_optional(&mut *tx)
                    .await?;
                    tx.rollback().await?;
                    return match current {
                        Some(current) => Ok(DispatchRunOutcome::DefinitionChanged {
                            expected: expected_definition_hash.to_vec(),
                            current,
                        }),
                        None => Err(DbError::NotFound(format!("workflow {workflow_id}"))),
                    };
                }
            }
        }
    };
    let admitted = sqlx::query(
        "UPDATE workflow_dispatch_outcomes \
         SET disposition = 'admitted', run_id = $4, reason = NULL, updated_at = now() \
         WHERE community_id = $1 AND event_id = $2 AND workflow_id = $3 \
           AND (disposition = 'pending' OR (disposition = 'admitted' AND run_id = $4))",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .bind(workflow_id)
    .bind(run_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if admitted != 1 {
        tx.rollback().await?;
        return Err(DbError::InvalidData(format!(
            "workflow {workflow_id} is not a pending candidate of dispatch event {}",
            hex::encode(event_id)
        )));
    }
    tx.commit().await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kind list lives in SQL (the trigger) and in Rust (this module);
    /// a drift in either would silently stop dispatch for one kind.
    #[test]
    fn trigger_kind_list_matches_dispatch_kinds() {
        let expected = format!(
            "IF NEW.kind IN ({}) THEN",
            DISPATCH_KINDS
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let migration = include_str!("../../../migrations/0047_workflow_dispatch_queue.sql");
        let schema = include_str!("../../../schema/schema.sql");
        assert!(migration.contains(&expected), "migration 0047: {expected}");
        let function_start = schema
            .find("CREATE FUNCTION enqueue_workflow_dispatch()")
            .expect("schema.sql defines enqueue_workflow_dispatch");
        assert!(
            schema[function_start..].contains(&expected),
            "schema.sql: {expected}"
        );
    }

    #[test]
    fn backoff_doubles_from_one_second_and_caps_at_ten_minutes() {
        assert_eq!(dispatch_backoff(1), Duration::from_secs(1));
        assert_eq!(dispatch_backoff(2), Duration::from_secs(2));
        assert_eq!(dispatch_backoff(4), Duration::from_secs(8));
        assert_eq!(dispatch_backoff(10), Duration::from_secs(512));
        assert_eq!(dispatch_backoff(11), Duration::from_secs(600));
        assert_eq!(dispatch_backoff(i32::MAX), Duration::from_secs(600));
        assert_eq!(dispatch_backoff(0), Duration::from_secs(1));
        assert_eq!(dispatch_backoff(-3), Duration::from_secs(1));
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::event::{insert_event, insert_event_with_thread_metadata_tx};
    use beekeeper_core::CommunityId;
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use sqlx::{AssertSqlSafe, PgPool};
    use uuid::Uuid;

    const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1

    fn test_db_url() -> String {
        std::env::var("BEEKEEPER_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| TEST_DB_URL.to_owned())
    }

    async fn setup_pool() -> PgPool {
        PgPool::connect(&test_db_url())
            .await
            .expect("connect to test DB")
    }

    async fn make_community(pool: &PgPool) -> CommunityId {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(id)
            .bind(format!("dispatch-{}.example", id.simple()))
            .execute(pool)
            .await
            .expect("insert test community");
        CommunityId::from_uuid(id)
    }

    fn event_of_kind(kind: u32) -> nostr::Event {
        EventBuilder::new(Kind::Custom(kind as u16), format!("kind {kind}"))
            .tags([Tag::parse(["h", &Uuid::new_v4().to_string()]).expect("h tag")])
            .sign_with_keys(&Keys::generate())
            .expect("sign event")
    }

    async fn queue_rows(pool: &PgPool, community: CommunityId, event_id: &[u8]) -> Vec<i32> {
        sqlx::query_scalar(
            "SELECT kind FROM workflow_dispatch_queue WHERE community_id = $1 AND event_id = $2",
        )
        .bind(community.as_uuid())
        .bind(event_id)
        .fetch_all(pool)
        .await
        .expect("read queue")
    }

    async fn make_workflow(pool: &PgPool, community: CommunityId) -> Uuid {
        let id = Uuid::new_v4();
        let owner = Keys::generate().public_key().to_bytes();
        crate::user::ensure_user(pool, community, &owner)
            .await
            .expect("insert workflow owner");
        crate::workflow::upsert_workflow(
            pool,
            community,
            id,
            None,
            &owner,
            "dispatch test",
            r#"{"name":"dispatch test","trigger":{"on":"manual"},"steps":[]}"#,
            &[3_u8; 32],
            None,
        )
        .await
        .expect("insert workflow");
        id
    }

    /// (a) Both dispatch kinds are enqueued by the insert's own transaction:
    /// visible inside it, invisible to another connection until commit,
    /// present after.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn dispatch_kinds_enqueue_in_the_inserting_transaction() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        for kind in DISPATCH_KINDS {
            let event = event_of_kind(kind);
            let id = event.id.as_bytes().to_vec();
            let mut tx = pool.begin().await.expect("begin");
            let (_, inserted) =
                insert_event_with_thread_metadata_tx(&mut tx, community, &event, None, None)
                    .await
                    .expect("insert event");
            assert!(inserted);
            let inside: Vec<i32> = sqlx::query_scalar(
                "SELECT kind FROM workflow_dispatch_queue \
                 WHERE community_id = $1 AND event_id = $2 AND state = 'pending'",
            )
            .bind(community.as_uuid())
            .bind(&id)
            .fetch_all(&mut *tx)
            .await
            .expect("read queue inside tx");
            assert_eq!(inside, vec![kind as i32], "kind {kind} enqueued in-tx");
            assert!(
                queue_rows(&pool, community, &id).await.is_empty(),
                "kind {kind}: the queue row is not visible before commit"
            );
            tx.commit().await.expect("commit");
            assert_eq!(
                queue_rows(&pool, community, &id).await,
                vec![kind as i32],
                "kind {kind}: exactly one queue row after commit"
            );
        }
    }

    /// (b) A rolled-back insert leaves neither the event nor a queue row.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn rolled_back_insert_leaves_no_queue_row() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        for kind in DISPATCH_KINDS {
            let event = event_of_kind(kind);
            let id = event.id.as_bytes().to_vec();
            let mut tx = pool.begin().await.expect("begin");
            insert_event_with_thread_metadata_tx(&mut tx, community, &event, None, None)
                .await
                .expect("insert event");
            tx.rollback().await.expect("rollback");
            assert!(queue_rows(&pool, community, &id).await.is_empty());
            let events: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM events WHERE community_id = $1 AND id = $2",
            )
            .bind(community.as_uuid())
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("count events");
            assert_eq!(events, 0);
        }
    }

    /// (c) Non-dispatch kinds are not enqueued (kind 9 is the busiest).
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn other_kinds_are_not_enqueued() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        // 46001 is a neighbouring workflow-execution kind; 9 a chat message.
        for kind in [9_u32, 46_001] {
            let event = event_of_kind(kind);
            let (_, inserted) = insert_event(&pool, community, &event, None)
                .await
                .expect("insert event");
            assert!(inserted);
            assert!(
                queue_rows(&pool, community, event.id.as_bytes())
                    .await
                    .is_empty(),
                "kind {kind} must not be enqueued"
            );
        }
    }

    /// (d) F2 regression: `ref_updated` creates one run per changed ref, all
    /// with the same trigger_event_id and no dispatch_event_id. The new
    /// exactly-once index must not constrain that.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn runs_sharing_a_trigger_event_id_still_insert() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let workflow = make_workflow(&pool, community).await;
        let trigger = [42_u8; 32];
        for _ in 0..2 {
            let outcome = crate::workflow::create_workflow_run(
                &pool,
                community,
                workflow,
                Some(&trigger),
                None,
                &[3_u8; 32],
            )
            .await
            .expect("ref_updated-style run inserts");
            assert!(matches!(
                outcome,
                crate::workflow::CreateRunOutcome::Created(_)
            ));
        }
        let runs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workflow_runs \
             WHERE community_id = $1 AND workflow_id = $2 AND trigger_event_id = $3",
        )
        .bind(community.as_uuid())
        .bind(workflow)
        .bind(&trigger[..])
        .fetch_one(&pool)
        .await
        .expect("count runs");
        assert_eq!(runs, 2);
    }

    /// (e) A second run for the same (workflow, dispatch_event_id) is a
    /// unique violation; another workflow, or another event, is not.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn duplicate_dispatch_event_id_conflicts() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let workflow = make_workflow(&pool, community).await;
        let other_workflow = make_workflow(&pool, community).await;
        let insert = |workflow: Uuid, dispatch: [u8; 32]| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO workflow_runs (community_id, id, workflow_id, dispatch_event_id) \
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(community.as_uuid())
                .bind(Uuid::new_v4())
                .bind(workflow)
                .bind(&dispatch[..])
                .execute(&pool)
                .await
            }
        };
        insert(workflow, [7; 32]).await.expect("first dispatch run");
        let duplicate = insert(workflow, [7; 32])
            .await
            .expect_err("second run for the same dispatch event must conflict");
        let code = duplicate
            .as_database_error()
            .and_then(|e| e.code())
            .map(|c| c.into_owned());
        assert_eq!(code.as_deref(), Some("23505"), "{duplicate}");
        insert(other_workflow, [7; 32])
            .await
            .expect("another workflow may admit the same event");
        insert(workflow, [8; 32])
            .await
            .expect("the same workflow may admit another event");
    }

    /// (f) The pgschema bootstrap path: pgschema emits partition children as
    /// standalone tables carrying copies of the parent's triggers, and
    /// `scripts/reconcile-schema-after-pgschema.sql` must drop those copies
    /// before re-attaching (PostgreSQL rejects an ATTACH whose child already
    /// has a same-named trigger). Simulated on a throwaway database built
    /// from schema.sql: detach the partition current events land in, give it
    /// standalone copies of every events trigger, run the reconcile script,
    /// and prove the child is re-attached with the dispatch trigger inherited
    /// and firing exactly once.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn pgschema_reconcile_leaves_the_dispatch_trigger_on_partition_children() {
        let base_url = test_db_url();
        let admin = PgPool::connect(&base_url).await.expect("connect admin");
        let probe_db = format!("beekeeper_dispatch_reconcile_{}", Uuid::new_v4().simple());
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {probe_db}")))
            .execute(&admin)
            .await
            .expect("create probe database");
        let (prefix, _) = base_url.rsplit_once('/').expect("database url has a path");
        let probe = PgPool::connect(&format!("{prefix}/{probe_db}"))
            .await
            .expect("connect probe database");

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let schema =
            std::fs::read_to_string(root.join("schema/schema.sql")).expect("read schema.sql");
        sqlx::raw_sql(AssertSqlSafe(schema))
            .execute(&probe)
            .await
            .expect("apply desired-state schema");

        // The partition an event created now lands in.
        let child: String = sqlx::query_scalar(
            "SELECT c.relname::text FROM pg_inherits i \
             JOIN pg_class c ON c.oid = i.inhrelid \
             WHERE i.inhparent = 'events'::regclass \
               AND pg_get_expr(c.relpartbound, c.oid) LIKE '%MAXVALUE)'",
        )
        .fetch_one(&probe)
        .await
        .expect("find the open-ended events partition");
        sqlx::raw_sql(AssertSqlSafe(format!(
            "ALTER TABLE events DETACH PARTITION {child}"
        )))
        .execute(&probe)
        .await
        .expect("detach the child as pgschema would leave it");
        // pgschema's standalone child carries its own copy of each parent
        // row trigger. Recreate any the detach did not leave behind.
        let copies = [
            (
                "events_enqueue_push_match",
                "AFTER INSERT",
                "enqueue_push_match_job",
            ),
            (
                "events_enqueue_workflow_dispatch",
                "AFTER INSERT",
                "enqueue_workflow_dispatch",
            ),
        ];
        for (name, timing, function) in copies {
            let present: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_trigger \
                 WHERE tgrelid = $1::regclass AND tgname = $2)",
            )
            .bind(&child)
            .bind(name)
            .fetch_one(&probe)
            .await
            .expect("inspect child triggers");
            if !present {
                sqlx::raw_sql(AssertSqlSafe(format!(
                    "CREATE TRIGGER {name} {timing} ON {child} \
                     FOR EACH ROW EXECUTE FUNCTION {function}()"
                )))
                .execute(&probe)
                .await
                .expect("create standalone trigger copy");
            }
        }
        let standalone_copy: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid = $1::regclass \
             AND tgname = 'events_enqueue_workflow_dispatch' AND tgparentid = 0)",
        )
        .bind(&child)
        .fetch_one(&probe)
        .await
        .expect("inspect standalone copy");
        assert!(
            standalone_copy,
            "precondition: the detached child carries a standalone dispatch trigger"
        );

        let reconcile =
            std::fs::read_to_string(root.join("scripts/reconcile-schema-after-pgschema.sql"))
                .expect("read reconcile script");
        let reconciled = sqlx::raw_sql(AssertSqlSafe(reconcile))
            .execute(&probe)
            .await;

        let attached: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_inherits \
             WHERE inhparent = 'events'::regclass AND inhrelid = $1::regclass)",
        )
        .bind(&child)
        .fetch_one(&probe)
        .await
        .expect("inspect attachment");
        let inherited: Vec<i64> = sqlx::query_scalar(
            "SELECT count(*) FROM pg_trigger WHERE tgrelid = $1::regclass \
             AND tgname = 'events_enqueue_workflow_dispatch' AND tgparentid <> 0",
        )
        .bind(&child)
        .fetch_all(&probe)
        .await
        .expect("inspect inherited trigger");

        let community = make_community(&probe).await;
        let event = event_of_kind(KIND_APPLICATION_EVENT);
        let inserted = insert_event(&probe, community, &event, None).await;
        let rows = queue_rows(&probe, community, event.id.as_bytes()).await;

        probe.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {probe_db} WITH (FORCE)"
        )))
        .execute(&admin)
        .await
        .expect("drop probe database");

        reconciled.expect("the reconcile script re-attaches the child");
        assert!(attached, "{child} is attached to events again");
        assert_eq!(inherited, vec![1], "{child} inherits the dispatch trigger");
        inserted.expect("insert into the re-attached partition");
        assert_eq!(rows, vec![KIND_APPLICATION_EVENT as i32]);
    }

    // ---- DB API (claim / freeze / outcome / completion / run creation) ----

    fn lease(secs: i64) -> DateTime<Utc> {
        Utc::now() + chrono::Duration::seconds(secs)
    }

    /// Insert `n` application events; returns their ids.
    async fn enqueue_events(pool: &PgPool, community: CommunityId, n: usize) -> Vec<Vec<u8>> {
        let mut ids = Vec::with_capacity(n);
        for _ in 0..n {
            let event = event_of_kind(KIND_APPLICATION_EVENT);
            insert_event(pool, community, &event, None)
                .await
                .expect("insert dispatch event");
            ids.push(event.id.as_bytes().to_vec());
        }
        ids
    }

    async fn queue_state(
        pool: &PgPool,
        community: CommunityId,
        event_id: &[u8],
    ) -> Option<(String, i32, Option<String>)> {
        sqlx::query_as(
            "SELECT state, attempts, terminal_reason FROM workflow_dispatch_queue \
             WHERE community_id = $1 AND event_id = $2",
        )
        .bind(community.as_uuid())
        .bind(event_id)
        .fetch_optional(pool)
        .await
        .expect("read queue state")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires Postgres"]
    async fn concurrent_claimers_get_disjoint_rows() {
        const ROWS: usize = 24;
        const CLAIMERS: usize = 4;
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let ids = enqueue_events(&pool, community, ROWS).await;
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(CLAIMERS));
        let mut calls = Vec::new();
        for _ in 0..CLAIMERS {
            let pool = pool.clone();
            let barrier = barrier.clone();
            calls.push(tokio::spawn(async move {
                barrier.wait().await;
                claim_dispatch_batch_in(&pool, Some(community), 6, lease(60))
                    .await
                    .expect("claim")
            }));
        }
        let mut seen = std::collections::HashMap::new();
        let mut claim_ids = std::collections::HashSet::new();
        for call in calls {
            if let Some(batch) = call.await.expect("join claimer") {
                assert_eq!(batch.community, community);
                assert!(claim_ids.insert(batch.claim_id));
                for job in batch.jobs {
                    assert_eq!(job.attempt, 1);
                    *seen
                        .entry(job.event.event.id.as_bytes().to_vec())
                        .or_insert(0) += 1;
                }
            }
        }
        // Drain the rest serially; nothing may be claimed twice overall.
        while let Some(batch) = claim_dispatch_batch_in(&pool, Some(community), 6, lease(60))
            .await
            .expect("drain claim")
        {
            for job in batch.jobs {
                *seen
                    .entry(job.event.event.id.as_bytes().to_vec())
                    .or_insert(0) += 1;
            }
        }
        assert!(
            seen.values().all(|count| *count == 1),
            "a row was claimed twice: {seen:?}"
        );
        assert_eq!(seen.len(), ROWS);
        for id in &ids {
            assert!(seen.contains_key(id));
        }
    }

    /// SKIP LOCKED, directly: a row another transaction holds is skipped,
    /// not waited on.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_claimer_skips_a_row_another_claimer_holds() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let ids = enqueue_events(&pool, community, 2).await;
        let mut holder = pool.begin().await.expect("begin holder");
        sqlx::query(
            "SELECT 1 FROM workflow_dispatch_queue \
             WHERE community_id = $1 AND event_id = $2 FOR UPDATE",
        )
        .bind(community.as_uuid())
        .bind(&ids[0])
        .execute(&mut *holder)
        .await
        .expect("hold row 0");
        let claimed = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            claim_dispatch_batch_in(&pool, Some(community), 10, lease(60)),
        )
        .await
        .expect("a claim must not wait on a row another claimer holds")
        .expect("claim")
        .expect("the free row is claimable");
        let got: Vec<Vec<u8>> = claimed
            .jobs
            .iter()
            .map(|job| job.event.event.id.as_bytes().to_vec())
            .collect();
        assert_eq!(got, vec![ids[1].clone()]);
        holder.rollback().await.expect("release holder");
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_lease_expired_row_is_reclaimed_and_the_old_claim_is_fenced() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let ids = enqueue_events(&pool, community, 1).await;
        let first = claim_dispatch_batch_in(&pool, Some(community), 10, lease(-1))
            .await
            .expect("claim")
            .expect("due row");
        assert_eq!(first.jobs[0].attempt, 1);
        let second = claim_dispatch_batch_in(&pool, Some(community), 10, lease(60))
            .await
            .expect("reclaim")
            .expect("lease-expired row is due again");
        assert_eq!(second.jobs.len(), 1);
        assert_eq!(second.jobs[0].attempt, 2);
        assert_ne!(first.claim_id, second.claim_id);
        // A live lease is not reclaimable.
        assert!(
            claim_dispatch_batch_in(&pool, Some(community), 10, lease(60))
                .await
                .expect("claim under live lease")
                .is_none()
        );
        complete_dispatch(&pool, community, &ids[0], first.claim_id)
            .await
            .expect_err("the expired claim can no longer complete");
        complete_dispatch(&pool, community, &ids[0], second.claim_id)
            .await
            .expect("the current claim completes");
        assert_eq!(
            queue_state(&pool, community, &ids[0]).await,
            Some(("done".to_owned(), 2, None))
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn exhaustion_fails_the_row_and_keeps_it() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let workflow = make_workflow(&pool, community).await;
        let ids = enqueue_events(&pool, community, 1).await;
        let mut outcome = None;
        for attempt in 1..=MAX_DISPATCH_ATTEMPTS {
            let batch = claim_dispatch_batch_in(&pool, Some(community), 10, lease(60))
                .await
                .expect("claim")
                .unwrap_or_else(|| panic!("attempt {attempt} is due"));
            assert_eq!(batch.jobs[0].attempt, attempt);
            if attempt == 1 {
                freeze_candidates(&pool, community, &ids[0], batch.claim_id, &[workflow])
                    .await
                    .expect("freeze");
            }
            assert!(record_outcome(
                &pool,
                community,
                &ids[0],
                workflow,
                &OutcomeUpdate::Retryable(format!("db error {attempt}")),
            )
            .await
            .expect("record retryable"));
            outcome = Some(
                reschedule_dispatch(
                    &pool,
                    community,
                    &ids[0],
                    batch.claim_id,
                    "retryable",
                    Duration::ZERO,
                )
                .await
                .expect("reschedule"),
            );
        }
        assert_eq!(outcome, Some(RescheduleOutcome::Exhausted));
        assert_eq!(
            queue_state(&pool, community, &ids[0]).await,
            Some((
                "failed".to_owned(),
                MAX_DISPATCH_ATTEMPTS,
                Some(REASON_ATTEMPTS_EXHAUSTED.to_owned())
            )),
            "an exhausted row is still present, failed, with its reason"
        );
        let outcomes = list_dispatch_outcomes(&pool, community, &ids[0])
            .await
            .expect("outcomes");
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].disposition, "failed");
        assert_eq!(outcomes[0].attempts, MAX_DISPATCH_ATTEMPTS);
        assert!(outcomes[0]
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with(REASON_ATTEMPTS_EXHAUSTED)));
        assert!(
            claim_dispatch_batch_in(&pool, Some(community), 10, lease(60))
                .await
                .expect("claim after exhaustion")
                .is_none()
        );
    }

    /// A worker that dies holding the final attempt leaves an unclaimable
    /// row; the sweep fails it — and keeps it.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn an_abandoned_final_attempt_is_swept_to_failed_and_kept() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let ids = enqueue_events(&pool, community, 1).await;
        sqlx::query(
            "UPDATE workflow_dispatch_queue SET attempts = $3 \
             WHERE community_id = $1 AND event_id = $2",
        )
        .bind(community.as_uuid())
        .bind(&ids[0])
        .bind(MAX_DISPATCH_ATTEMPTS - 1)
        .execute(&pool)
        .await
        .expect("advance attempts");
        let batch = claim_dispatch_batch_in(&pool, Some(community), 10, lease(-1))
            .await
            .expect("claim")
            .expect("final attempt");
        assert_eq!(batch.jobs[0].attempt, MAX_DISPATCH_ATTEMPTS);
        assert!(fail_exhausted_dispatches(&pool).await.expect("sweep") >= 1);
        assert_eq!(
            queue_state(&pool, community, &ids[0]).await,
            Some((
                "failed".to_owned(),
                MAX_DISPATCH_ATTEMPTS,
                Some(REASON_ATTEMPTS_EXHAUSTED.to_owned())
            ))
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_missing_or_deleted_source_event_fails_terminally_and_keeps_the_row() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let live = enqueue_events(&pool, community, 1).await;
        let deleted = enqueue_events(&pool, community, 1).await;
        sqlx::query("UPDATE events SET deleted_at = now() WHERE community_id = $1 AND id = $2")
            .bind(community.as_uuid())
            .bind(&deleted[0])
            .execute(&pool)
            .await
            .expect("soft-delete source event");
        let absent = [9_u8; 32];
        sqlx::query(
            "INSERT INTO workflow_dispatch_queue (community_id, event_id, kind) \
             VALUES ($1, $2, $3)",
        )
        .bind(community.as_uuid())
        .bind(&absent[..])
        .bind(KIND_APPLICATION_EVENT as i32)
        .execute(&pool)
        .await
        .expect("queue row without an event");

        let batch = claim_dispatch_batch_in(&pool, Some(community), 10, lease(60))
            .await
            .expect("claim")
            .expect("the live row is returned");
        let got: Vec<Vec<u8>> = batch
            .jobs
            .iter()
            .map(|job| job.event.event.id.as_bytes().to_vec())
            .collect();
        assert_eq!(got, live);
        for missing in [&deleted[0][..], &absent[..]] {
            assert_eq!(
                queue_state(&pool, community, missing).await,
                Some((
                    "failed".to_owned(),
                    1,
                    Some(REASON_SOURCE_EVENT_MISSING.to_owned())
                )),
                "a missing source event is failed and kept, never deleted"
            );
        }
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn candidates_freeze_once_and_decided_outcomes_are_not_overwritten() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let first = make_workflow(&pool, community).await;
        let second = make_workflow(&pool, community).await;
        let late = make_workflow(&pool, community).await;
        let ids = enqueue_events(&pool, community, 1).await;
        let batch = claim_dispatch_batch_in(&pool, Some(community), 10, lease(-1))
            .await
            .expect("claim")
            .expect("due");
        assert!(!batch.jobs[0].candidates_frozen);
        assert_eq!(
            freeze_candidates(&pool, community, &ids[0], batch.claim_id, &[first, second])
                .await
                .expect("freeze"),
            FreezeOutcome::Frozen
        );
        // A stale claim cannot freeze or touch the row.
        let retry = claim_dispatch_batch_in(&pool, Some(community), 10, lease(60))
            .await
            .expect("reclaim")
            .expect("expired lease");
        assert!(retry.jobs[0].candidates_frozen);
        freeze_candidates(&pool, community, &ids[0], batch.claim_id, &[late])
            .await
            .expect_err("a lost claim cannot freeze");
        assert_eq!(
            freeze_candidates(&pool, community, &ids[0], retry.claim_id, &[late])
                .await
                .expect("refreeze"),
            FreezeOutcome::AlreadyFrozen,
            "a workflow enabled after the first claim is never added"
        );
        assert!(record_outcome(
            &pool,
            community,
            &ids[0],
            first,
            &OutcomeUpdate::Skipped("filter miss".into())
        )
        .await
        .expect("skip"));
        assert!(!record_outcome(
            &pool,
            community,
            &ids[0],
            first,
            &OutcomeUpdate::Retryable("late error".into())
        )
        .await
        .expect("a decided outcome is not overwritten"));
        let outcomes = list_dispatch_outcomes(&pool, community, &ids[0])
            .await
            .expect("outcomes");
        let workflows: Vec<Uuid> = outcomes.iter().map(|o| o.workflow_id).collect();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(workflows, expected);
        let skipped = outcomes
            .iter()
            .find(|o| o.workflow_id == first)
            .expect("first outcome");
        assert_eq!(skipped.disposition, "skipped");
        assert_eq!(skipped.reason.as_deref(), Some("filter miss"));
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn create_dispatch_run_is_idempotent_and_admits_in_the_same_transaction() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let workflow = make_workflow(&pool, community).await;
        let stranger = make_workflow(&pool, community).await;
        let ids = enqueue_events(&pool, community, 1).await;
        let batch = claim_dispatch_batch_in(&pool, Some(community), 10, lease(60))
            .await
            .expect("claim")
            .expect("due");
        freeze_candidates(&pool, community, &ids[0], batch.claim_id, &[workflow])
            .await
            .expect("freeze");
        let context = serde_json::json!({"text": "hello"});
        let first = create_dispatch_run(
            &pool,
            community,
            &ids[0],
            workflow,
            Some(&context),
            &[3; 32],
        )
        .await
        .expect("first admission");
        let DispatchRunOutcome::Created(run_id) = first else {
            panic!("first admission creates the run, got {first:?}");
        };
        let second = create_dispatch_run(
            &pool,
            community,
            &ids[0],
            workflow,
            Some(&context),
            &[3; 32],
        )
        .await
        .expect("retried admission");
        assert_eq!(second, DispatchRunOutcome::Existing(run_id));
        let runs: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workflow_runs \
             WHERE community_id = $1 AND workflow_id = $2 AND dispatch_event_id = $3",
        )
        .bind(community.as_uuid())
        .bind(workflow)
        .bind(&ids[0])
        .fetch_all(&pool)
        .await
        .expect("runs");
        assert_eq!(runs, vec![run_id], "exactly one run per (workflow, event)");
        let run = crate::workflow::get_workflow_run(&pool, community, run_id)
            .await
            .expect("load run");
        assert_eq!(run.trigger_event_id.as_deref(), Some(&ids[0][..]));
        assert_eq!(run.definition_hash.as_deref(), Some(&[3_u8; 32][..]));
        let outcomes = list_dispatch_outcomes(&pool, community, &ids[0])
            .await
            .expect("outcomes");
        assert_eq!(outcomes[0].disposition, "admitted");
        assert_eq!(outcomes[0].run_id, Some(run_id));

        // A workflow that is not a frozen candidate is refused, and no run
        // is left behind (the insert rolled back with the outcome update).
        create_dispatch_run(&pool, community, &ids[0], stranger, None, &[3; 32])
            .await
            .expect_err("not a candidate");
        let stray: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workflow_runs WHERE community_id = $1 AND workflow_id = $2",
        )
        .bind(community.as_uuid())
        .bind(stranger)
        .fetch_one(&pool)
        .await
        .expect("count stray runs");
        assert_eq!(stray, 0);

        // A changed definition creates nothing and leaves the outcome alone.
        let other_ids = enqueue_events(&pool, community, 1).await;
        let other = claim_dispatch_batch_in(&pool, Some(community), 10, lease(60))
            .await
            .expect("claim")
            .expect("due");
        freeze_candidates(&pool, community, &other_ids[0], other.claim_id, &[workflow])
            .await
            .expect("freeze");
        let changed =
            create_dispatch_run(&pool, community, &other_ids[0], workflow, None, &[4; 32])
                .await
                .expect("definition check");
        assert!(matches!(
            changed,
            DispatchRunOutcome::DefinitionChanged { .. }
        ));
        let pending = list_dispatch_outcomes(&pool, community, &other_ids[0])
            .await
            .expect("outcomes");
        assert_eq!(pending[0].disposition, "pending");
    }

    /// The 46008 candidate lookup: enabled, active `ci_result` workflows of
    /// exactly this project, in this community.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn ci_result_candidates_are_scoped_to_project_trigger_and_enablement() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let other_community = make_community(&pool).await;
        let project = format!("30621:{}:p-{}", "a".repeat(64), Uuid::new_v4().simple());
        let ci = r#"{"name":"c","trigger":{"on":"ci_result","check":"ci","conclusion":["success"]},"steps":[]}"#;
        let manual = r#"{"name":"m","trigger":{"on":"manual"},"steps":[]}"#;
        let insert = |community: CommunityId, def: &'static str, project: Option<String>| {
            let pool = pool.clone();
            async move {
                let id = Uuid::new_v4();
                let owner = Keys::generate().public_key().to_bytes();
                crate::user::ensure_user(&pool, community, &owner)
                    .await
                    .expect("owner");
                crate::workflow::upsert_workflow(
                    &pool,
                    community,
                    id,
                    None,
                    &owner,
                    "w",
                    def,
                    &[1; 32],
                    project.as_deref(),
                )
                .await
                .expect("workflow");
                id
            }
        };
        let wanted = insert(community, ci, Some(project.clone())).await;
        let disabled = insert(community, ci, Some(project.clone())).await;
        crate::workflow::set_workflow_enabled(&pool, community, disabled, false)
            .await
            .expect("disable");
        insert(community, manual, Some(project.clone())).await;
        insert(community, ci, Some(format!("{project}-other"))).await;
        insert(community, ci, None).await;
        insert(other_community, ci, Some(project.clone())).await;
        let found: Vec<Uuid> =
            crate::workflow::list_enabled_ci_result_workflows(&pool, community, &project)
                .await
                .expect("lookup")
                .into_iter()
                .map(|w| w.id)
                .collect();
        assert_eq!(found, vec![wanted]);
    }
}
