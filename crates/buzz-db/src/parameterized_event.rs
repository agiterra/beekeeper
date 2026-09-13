//! Atomic NIP-33 replacement, with project-source dispatch.

use buzz_core::{CommunityId, StoredEvent};
use buzz_datastore_tracing::datastore_span;
use uuid::Uuid;

use crate::{event, event_replacement_lock_key, project_pack_source, Db, DbError, Result};

impl Db {
    /// Atomically replace a NIP-33 parameterized replaceable event (kind 30000–39999).
    ///
    /// Keeps only the event with the highest `created_at` per `(kind, pubkey, d_tag)`.
    /// Same-second ties are broken by lowest event `id` (deterministic ordering).
    /// The entire check → retire old payload → insert runs in a single transaction
    /// with an advisory lock to prevent concurrent-insert races. NIP-RS read-state
    /// coordinates hard-delete the superseded payload and preserve a compact
    /// ordering watermark. Buzz mesh status coordinates also hard-delete their
    /// superseded heartbeat payload because only the live head has product
    /// value; other NIP-33 kinds retain soft-deleted history.
    ///
    /// **Channel policy:** NIP-33 replacement keys on `(kind, pubkey, d_tag)` globally —
    /// `channel_id` is NOT part of the replacement key. This matches the Nostr spec:
    /// an author's parameterized replaceable event is a single global resource identified
    /// by its d-tag, regardless of which channel it was submitted to. The `channel_id`
    /// parameter is stored on the new row for query scoping but does not affect replacement.
    ///
    /// Note: `replace_addressable_event()` keys on `channel_id` because it serves
    /// relay-signed NIP-29 group metadata (kind 39000–39002) where the relay is the
    /// author and channel_id distinguishes groups. User-submitted NIP-33 events use
    /// this function instead, where the author's pubkey + d-tag is the natural key.
    #[datastore_span(name = "replace_parameterized_event", system = "postgresql")]
    pub async fn replace_parameterized_event(
        &self,
        community_id: CommunityId,
        event: &nostr::Event,
        d_tag: &str,
        channel_id: Option<Uuid>,
    ) -> Result<(StoredEvent, bool)> {
        if buzz_core::kind::event_kind_u32(event) == buzz_core::kind::KIND_PROJECT_PACK_SOURCE {
            return project_pack_source::replace(
                &self.pool,
                community_id,
                event,
                d_tag,
                channel_id,
            )
            .await;
        }
        let kind_i32 = buzz_core::kind::event_kind_i32(event);
        let pubkey_bytes = event.pubkey.to_bytes();
        let created_at_secs = event.created_at.as_secs() as i64;
        let created_at = chrono::DateTime::from_timestamp(created_at_secs, 0)
            .ok_or(DbError::InvalidTimestamp(created_at_secs))?;

        let lock_key = event_replacement_lock_key(
            community_id,
            kind_i32,
            pubkey_bytes.as_slice(),
            Some(d_tag.as_bytes()),
        );

        let mut tx = self.pool.begin().await?;

        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(lock_key)
            .execute(&mut *tx)
            .await?;

        let d_tag_count = event
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().is_some_and(|part| part == "d"))
            .count();
        let has_exact_d_tag = event.tags.iter().any(|tag| {
            let parts = tag.as_slice();
            parts.len() >= 2 && parts[0] == "d" && parts[1] == d_tag
        });
        let read_state_t_tag_count = event
            .tags
            .iter()
            .filter(|tag| {
                let parts = tag.as_slice();
                parts.len() == 2 && parts[0] == "t" && parts[1] == "read-state"
            })
            .count();
        let is_nip_rs = kind_i32 == buzz_core::kind::KIND_READ_STATE as i32
            && d_tag_count == 1
            && has_exact_d_tag
            && d_tag.strip_prefix("read-state:").is_some_and(|slot| {
                slot.len() == 32
                    && slot
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
            && read_state_t_tag_count == 1;
        let is_buzz_mesh_status = kind_i32 == buzz_core::kind::KIND_BOOKMARK_SET as i32
            && d_tag.starts_with("buzz-mesh-member-status:")
            && event.tags.iter().any(|tag| {
                let parts = tag.as_slice();
                parts.len() == 2 && parts[0] == "k" && parts[1] == "buzz-mesh-status"
            });
        let hard_delete_superseded = is_nip_rs || is_buzz_mesh_status;

        // Check the live head and, for NIP-RS, the compact historical ordering
        // watermark. The watermark remains after a NIP-09 coordinate deletion,
        // preventing a previously accepted signed blob from being resurrected.
        let existing: Option<(chrono::DateTime<chrono::Utc>, Vec<u8>)> = sqlx::query_as(
            "SELECT created_at, id FROM events \
             WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND d_tag = $4 AND deleted_at IS NULL \
             ORDER BY created_at DESC, id ASC LIMIT 1",
        )
        .bind(community_id.as_uuid())
        .bind(kind_i32)
        .bind(pubkey_bytes.as_slice())
        .bind(d_tag)
        .fetch_optional(&mut *tx)
        .await?;
        let watermark: Option<(chrono::DateTime<chrono::Utc>, Vec<u8>)> = if is_nip_rs {
            sqlx::query_as(
                "SELECT created_at, event_id FROM parameterized_event_watermarks \
                 WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND d_tag = $4",
            )
            .bind(community_id.as_uuid())
            .bind(kind_i32)
            .bind(pubkey_bytes.as_slice())
            .bind(d_tag)
            .fetch_optional(&mut *tx)
            .await?
        } else {
            None
        };

        // Stale-write protection: reject if either durable ordering source
        // dominates the incoming tuple. Equal timestamps use lowest event id.
        let incoming_id = event.id.as_bytes().as_slice();
        let dominated =
            existing
                .iter()
                .chain(watermark.iter())
                .any(|(accepted_ts, accepted_id)| {
                    created_at < *accepted_ts
                        || (created_at == *accepted_ts && incoming_id >= accepted_id.as_slice())
                });
        if dominated {
            tx.rollback().await?;
            let received_at = chrono::Utc::now();
            return Ok((
                StoredEvent::with_received_at(event.clone(), received_at, channel_id, false),
                false,
            ));
        }

        if existing.is_some() {
            if is_nip_rs {
                // Migration 0011 rejects regex-coordinate hard deletes from
                // pre-fix writers. Authorize only this corrected NIP-RS delete,
                // transaction-locally so pooled connections cannot leak it.
                sqlx::query("SELECT set_config('buzz.nip_rs_hard_delete', 'on', true)")
                    .execute(&mut *tx)
                    .await?;
            }
            let statement = if hard_delete_superseded {
                "DELETE FROM events \
                 WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND d_tag = $4 AND deleted_at IS NULL"
            } else {
                "UPDATE events SET deleted_at = NOW() \
                 WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND d_tag = $4 AND deleted_at IS NULL"
            };
            sqlx::query(statement)
                .bind(community_id.as_uuid())
                .bind(kind_i32)
                .bind(pubkey_bytes.as_slice())
                .bind(d_tag)
                .execute(&mut *tx)
                .await?;

            if hard_delete_superseded {
                if let Some((_, existing_id)) = &existing {
                    // Event first, mentions second: migration 0009's live-event
                    // fence uses this global lock order to avoid deadlocks.
                    sqlx::query(
                        "DELETE FROM event_mentions WHERE community_id = $1 AND event_id = $2",
                    )
                    .bind(community_id.as_uuid())
                    .bind(existing_id)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }

        // Insert the new event inside the transaction.
        let sig_bytes = event.sig.serialize();
        let tags_json = serde_json::to_value(&event.tags)?;
        let received_at = chrono::Utc::now();

        let insert_result = sqlx::query(
            "INSERT INTO events (community_id, id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id, d_tag, not_before) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             ON CONFLICT DO NOTHING",
        )
        .bind(community_id.as_uuid())
        .bind(event.id.as_bytes().as_slice())
        .bind(pubkey_bytes.as_slice())
        .bind(created_at)
        .bind(kind_i32)
        .bind(&tags_json)
        .bind(&event.content)
        .bind(sig_bytes.as_slice())
        .bind(received_at)
        .bind(channel_id)
        .bind(d_tag)
        .bind(event::extract_not_before(event))
        .execute(&mut *tx)
        .await?;

        let was_inserted = insert_result.rows_affected() > 0;
        if !was_inserted {
            tx.rollback().await?;
            return Ok((
                StoredEvent::with_received_at(event.clone(), received_at, channel_id, false),
                false,
            ));
        }

        if is_nip_rs {
            sqlx::query(
                "INSERT INTO parameterized_event_watermarks \
                     (community_id, kind, pubkey, d_tag, created_at, event_id) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (community_id, kind, pubkey, d_tag) DO UPDATE SET \
                     created_at = EXCLUDED.created_at, event_id = EXCLUDED.event_id",
            )
            .bind(community_id.as_uuid())
            .bind(kind_i32)
            .bind(pubkey_bytes.as_slice())
            .bind(d_tag)
            .bind(created_at)
            .bind(incoming_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        // Mentions are a denormalized index — safe outside the transaction.
        if let Err(e) = crate::insert_mentions(&self.pool, community_id, event, channel_id).await {
            tracing::warn!(event_id = %event.id, "Failed to insert mentions: {e}");
        }

        Ok((
            StoredEvent::with_received_at(event.clone(), received_at, channel_id, true),
            true,
        ))
    }
}
