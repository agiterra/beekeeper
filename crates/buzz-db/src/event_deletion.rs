//! Event tombstones, thread counters, and source-publication serialization.

use buzz_core::CommunityId;
use chrono::DateTime;
use sqlx::PgPool;

use crate::{DbError, Result};

/// Soft-delete an event by setting `deleted_at = NOW()`.
///
/// Returns `Ok(true)` if the event was deleted, `Ok(false)` if already deleted
/// or not found. Callers are responsible for decrementing thread reply counts
/// when the deleted event is a thread reply.
pub async fn soft_delete_event(
    pool: &PgPool,
    community_id: CommunityId,
    event_id: &[u8],
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    if !crate::project_pack_source::lock_event_deletion(&mut tx, community_id, event_id).await? {
        tx.rollback().await?;
        return Ok(false);
    }
    let result = sqlx::query(
        "UPDATE events SET deleted_at = NOW() WHERE community_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
            .bind(community_id.as_uuid())
            .bind(event_id)
            .execute(&mut *tx)
            .await?;

    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

/// Soft-delete the live row for an addressable coordinate
/// `(kind, pubkey, d_tag)` — the NIP-33 replacement key — provided it is not
/// newer than the deletion request.
///
/// Used by `handle_a_tag_deletion` to honour NIP-09 a-tag deletions for any
/// parameterized-replaceable kind. The WHERE clause mirrors
/// `replace_parameterized_event` so the coordinate semantics stay consistent:
/// `channel_id` is intentionally NOT in the key (NIP-33 replacement is global
/// per the spec — `channel_id` is stored for query scoping, not identity).
///
/// `deletion_created_at_secs` is the deletion event's own `created_at`. NIP-09
/// scopes an `a`-tag deletion to versions at or before that instant, so a
/// delayed or replayed tombstone signed between two versions must not erase the
/// newer replacement. `events.created_at` is immutable per row, so the predicate
/// guarantees a tombstone can never erase a version newer than itself — the UPDATE
/// re-evaluates its WHERE clause after any lock wait, so a replacement that races
/// the deletion and lands with a later `created_at` is always spared.
///
/// This does NOT guarantee deletion completeness when a same-coordinate
/// replacement races the deletion: the deletion may evaluate its predicate before
/// the replacement arrives, miss the incoming head, and return `Ok(false)`. That
/// outcome is state-identical to the deletion having arrived first (old head
/// gone, new head present), which is a valid Nostr ordering — Nostr never fixes
/// the order of concurrent writes from different signers, and even same-signer
/// ordering is advisory. The return value feeds only a debug log, not a
/// correctness gate.
///
/// Returns `Ok(true)` if a row was deleted, `Ok(false)` if no live row matched
/// (already deleted, never existed, or strictly newer than the deletion).
pub async fn soft_delete_by_coordinate(
    pool: &PgPool,
    community_id: CommunityId,
    kind: i32,
    pubkey: &[u8],
    d_tag: &str,
    deletion_created_at_secs: i64,
) -> Result<bool> {
    let deletion_created_at = DateTime::from_timestamp(deletion_created_at_secs, 0)
        .ok_or(DbError::InvalidTimestamp(deletion_created_at_secs))?;
    let mut tx = pool.begin().await?;
    if kind == buzz_core::kind::KIND_PROJECT_PACK_SOURCE as i32 {
        crate::project_pack_source::lock_project(&mut tx, community_id, d_tag).await?;
    }
    let result = sqlx::query(
        "UPDATE events SET deleted_at = NOW() \
         WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND d_tag = $4 AND deleted_at IS NULL \
         AND created_at <= $5",
    )
    .bind(community_id.as_uuid())
    .bind(kind)
    .bind(pubkey)
    .bind(d_tag)
    .bind(deletion_created_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

/// Atomically soft-delete an event and decrement thread reply counters.
///
/// Wraps the delete + counter update in a single transaction so a crash between
/// them cannot leave counters permanently inflated. Returns `Ok(true)` if the
/// event was deleted this call.
pub async fn soft_delete_event_and_update_thread(
    pool: &PgPool,
    community_id: CommunityId,
    event_id: &[u8],
    parent_event_id: Option<&[u8]>,
    root_event_id: Option<&[u8]>,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    if !crate::project_pack_source::lock_event_deletion(&mut tx, community_id, event_id).await? {
        tx.rollback().await?;
        return Ok(false);
    }

    let result = sqlx::query(
        "UPDATE events SET deleted_at = NOW() WHERE community_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(community_id.as_uuid())
    .bind(event_id)
    .execute(&mut *tx)
    .await?;

    let deleted = result.rows_affected() > 0;

    if deleted {
        if let Some(pid) = parent_event_id {
            sqlx::query(
                "UPDATE thread_metadata \
                 SET reply_count = GREATEST(reply_count - 1, 0) \
                 WHERE community_id = $1 AND event_id = $2",
            )
            .bind(community_id.as_uuid())
            .bind(pid)
            .execute(&mut *tx)
            .await?;

            if let Some(root_id) = root_event_id {
                sqlx::query(
                    "UPDATE thread_metadata \
                     SET descendant_count = GREATEST(descendant_count - 1, 0) \
                     WHERE community_id = $1 AND event_id = $2",
                )
                .bind(community_id.as_uuid())
                .bind(root_id)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    tx.commit().await?;
    Ok(deleted)
}
