//! Project-wide serialization for source publication and NIP-09 deletion.
//!
//! The lock is independent of the source author. Legacy v1 records retain
//! their raw NIP-33 coordinate and per-author ordering; noncanonical legacy
//! aliases remain invisible to canonical `#d` readers, as before. Conditional
//! writes use exactly that canonical reader head, across all authors.

use buzz_core::kind::{normalize_project_coordinate, KIND_PROJECT_PACK_SOURCE};
use buzz_core::project_pack_source::{decode_project_pack_source, PackSourceExpectation};
use buzz_core::{CommunityId, StoredEvent};
use chrono::{DateTime, Utc};
use nostr::Event;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{DbError, Result};

/// Lock the community before the project, preserving deletion-executor GUCs.
pub(crate) async fn lock_project(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    coordinate: &str,
) -> Result<()> {
    // The SQL guard enforces READ COMMITTED and also recognizes an admitted
    // serving lease or deletion executor. An active-only Rust guard would
    // incorrectly reject those existing authorized lifecycle operations.
    sqlx::query("SELECT assert_community_write_allowed($1)")
        .bind(community.as_uuid())
        .execute(&mut **tx)
        .await?;
    // New writes have already passed the strict decoder. Historical pre-gate
    // malformed rows must remain deletable, including a missing d tag; they
    // occupy a raw fallback lock and cannot be a canonical reader head.
    let project = normalize_project_coordinate(coordinate).unwrap_or_else(|| coordinate.to_owned());
    let lock = format!("buzz-project-pack-source:{community}:{project}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Discover an immutable source coordinate without taking row locks first.
pub(crate) async fn lock_event_deletion(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    event_id: &[u8],
) -> Result<bool> {
    let target: Option<(i32, Option<String>)> = sqlx::query_as(
        "SELECT kind, d_tag FROM events WHERE community_id = $1 AND id = $2 \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(community.as_uuid())
    .bind(event_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((kind, coordinate)) = target else {
        // Do not issue a later UPDATE: a previously absent source could be
        // inserted between these statements, bypassing its project lock.
        return Ok(false);
    };
    if kind == KIND_PROJECT_PACK_SOURCE as i32 {
        lock_project(tx, community, coordinate.as_deref().unwrap_or_default()).await?;
    }
    Ok(true)
}

/// Store one source atomically, including legacy writes and exact retries.
pub(crate) async fn replace(
    pool: &PgPool,
    community: CommunityId,
    event: &Event,
    d_tag: &str,
    channel_id: Option<Uuid>,
) -> Result<(StoredEvent, bool)> {
    let source = decode_project_pack_source(event).map_err(DbError::InvalidData)?;
    if crate::event::extract_d_tag(event).as_deref() != Some(d_tag) {
        return Err(DbError::InvalidData(
            "pack source d tag disagrees with storage coordinate".into(),
        ));
    }
    if !matches!(source.expectation(), PackSourceExpectation::Unconditional)
        && d_tag != source.project()
    {
        return Err(DbError::InvalidData(
            "conditional pack source d tag must be canonical".into(),
        ));
    }
    if channel_id.is_some() {
        return Err(DbError::InvalidData(
            "pack sources are global events".into(),
        ));
    }
    let created_secs = event.created_at.as_secs() as i64;
    let created_at =
        DateTime::from_timestamp(created_secs, 0).ok_or(DbError::InvalidTimestamp(created_secs))?;
    let id = event.id.as_bytes().as_slice();
    let author = event.pubkey.to_bytes();
    let mut tx = pool.begin().await?;
    lock_project(&mut tx, community, source.project()).await?;

    // Retained IDs reconcile before checking the old expectation. Their
    // original publication can have succeeded and subsequently been replaced
    // or deleted. A retry is never another source switch.
    let retained: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM events WHERE community_id = $1 AND id = $2)",
    )
    .bind(community.as_uuid())
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if retained {
        tx.rollback().await?;
        return Ok((
            StoredEvent::with_received_at(event.clone(), Utc::now(), None, false),
            false,
        ));
    }

    if let PackSourceExpectation::Expected(expected) = source.expectation() {
        let head: Option<(DateTime<Utc>, Vec<u8>)> = sqlx::query_as(
            "SELECT created_at, id FROM events WHERE community_id = $1 \
             AND kind = $2 AND d_tag = $3 AND deleted_at IS NULL \
             ORDER BY created_at DESC, id ASC LIMIT 1",
        )
        .bind(community.as_uuid())
        .bind(KIND_PROJECT_PACK_SOURCE as i32)
        .bind(source.project())
        .fetch_optional(&mut *tx)
        .await?;
        let current = head.as_ref().map(|(_, id)| hex::encode(id));
        if &current != expected {
            return Err(DbError::PackSourceConflict(format!(
                "project {} expected source {}, current source {}",
                source.project(),
                expected.as_deref().unwrap_or("none"),
                current.as_deref().unwrap_or("none")
            )));
        }
        if head.as_ref().is_some_and(|(timestamp, head_id)| {
            created_at < *timestamp || (created_at == *timestamp && id >= head_id.as_slice())
        }) {
            return Err(DbError::PackSourceConflict(
                "conditional source must outrank its expected head".into(),
            ));
        }
    }

    let author_head: Option<(DateTime<Utc>, Vec<u8>)> = sqlx::query_as(
        "SELECT created_at, id FROM events WHERE community_id = $1 \
         AND kind = $2 AND pubkey = $3 AND d_tag = $4 AND deleted_at IS NULL \
         ORDER BY created_at DESC, id ASC LIMIT 1",
    )
    .bind(community.as_uuid())
    .bind(KIND_PROJECT_PACK_SOURCE as i32)
    .bind(author.as_slice())
    .bind(d_tag)
    .fetch_optional(&mut *tx)
    .await?;
    if author_head.as_ref().is_some_and(|(timestamp, head_id)| {
        created_at < *timestamp || (created_at == *timestamp && id >= head_id.as_slice())
    }) {
        tx.rollback().await?;
        return Ok((
            StoredEvent::with_received_at(event.clone(), Utc::now(), None, false),
            false,
        ));
    }
    sqlx::query(
        "UPDATE events SET deleted_at = NOW() WHERE community_id = $1 \
         AND kind = $2 AND pubkey = $3 AND d_tag = $4 AND deleted_at IS NULL",
    )
    .bind(community.as_uuid())
    .bind(KIND_PROJECT_PACK_SOURCE as i32)
    .bind(author.as_slice())
    .bind(d_tag)
    .execute(&mut *tx)
    .await?;
    let received_at = Utc::now();
    let inserted = sqlx::query(
        "INSERT INTO events (community_id, id, pubkey, created_at, kind, tags, content, \
         sig, received_at, channel_id, d_tag) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,NULL,$10) \
         ON CONFLICT DO NOTHING",
    )
    .bind(community.as_uuid())
    .bind(id)
    .bind(author.as_slice())
    .bind(created_at)
    .bind(KIND_PROJECT_PACK_SOURCE as i32)
    .bind(serde_json::to_value(&event.tags)?)
    .bind(&event.content)
    .bind(event.sig.serialize().as_slice())
    .bind(received_at)
    .bind(d_tag)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    if !inserted {
        // Never commit retirement if storage did not accept the successor.
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }
    Ok((
        StoredEvent::with_received_at(event.clone(), received_at, None, inserted),
        inserted,
    ))
}

#[cfg(test)]
#[path = "project_pack_source_tests.rs"]
mod tests;
