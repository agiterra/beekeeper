//! Event storage and retrieval.
//!
//! AUTH events (kind 22242) are never stored — they carry bearer tokens.
//! Ephemeral events (kinds 20000–29999) are never stored — Redis pub/sub only.
//! Deduplication is application-layer: ON CONFLICT DO NOTHING.

use chrono::{DateTime, Utc};
use nostr::Event;
use sha2::{Digest as _, Sha256};
use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction};
use uuid::Uuid;

use buzz_core::kind::{
    event_kind_i32, is_ephemeral, is_parameterized_replaceable, KIND_AUTH, KIND_EVENT_REMINDER,
    KIND_HUDDLE_STARTED, SHARED_GATED_KINDS,
};
use buzz_core::{CommunityId, StoredEvent};

use crate::error::{DbError, Result};

/// Largest page [`query_events`] will return when [`EventQuery::max_limit`] is
/// unset — the effective ceiling on any client-requested `limit`.
///
/// This is the value the relay advertises as NIP-11 `limitation.max_limit`, so
/// the advertised ceiling and the enforced one cannot drift.
pub const DEFAULT_MAX_PAGE_LIMIT: i64 = 1_000;

/// Optional filters for [`query_events`].
#[derive(Debug, Clone)]
pub struct EventQuery {
    /// Server-resolved community scope.
    pub community_id: CommunityId,
    /// Restrict results to this channel.
    pub channel_id: Option<Uuid>,
    /// Restrict results to these kind values (stored as `i32` in Postgres).
    pub kinds: Option<Vec<i32>>,
    /// Restrict results to events from this pubkey.
    pub pubkey: Option<Vec<u8>>,
    /// Return events created at or after this time.
    pub since: Option<DateTime<Utc>>,
    /// Return events created at or before this time.
    pub until: Option<DateTime<Utc>>,
    /// Maximum number of events to return.
    pub limit: Option<i64>,
    /// Number of events to skip (for pagination).
    pub offset: Option<i64>,
    /// Restrict to events with a `p` tag mentioning this hex pubkey.
    /// Joins against `event_mentions` table (indexed).
    pub p_tag_hex: Option<String>,
    /// Restrict to events with this exact `d_tag` value (NIP-33).
    /// Pushed into SQL via the `idx_events_parameterized` index.
    pub d_tag: Option<String>,
    /// Restrict to events with any of these `d_tag` values (multi-value NIP-33 pushdown).
    /// Used when a filter has multiple `#d` values and targets only NIP-33 kinds.
    pub d_tags: Option<Vec<String>>,
    /// Composite keyset cursor: exclude events at or "after" this (created_at, id) pair.
    /// Used with `until` for stable pagination: events where
    /// `created_at < until OR (created_at = until AND id > before_id)`.
    /// When set, `until` must also be set.
    pub before_id: Option<Vec<u8>>,
    /// When true, restricts results to global events (`channel_id IS NULL`).
    /// Use for endpoints that serve non-channel data (e.g. kind:1 notes) to
    /// defensively prevent leaking channel-scoped events if the ingest
    /// invariant (`is_global_only_kind`) ever changes.
    /// Mutually exclusive with `channel_id`.
    pub global_only: bool,
    /// Restrict results to events from any of these pubkeys (multi-author `IN` pushdown).
    pub authors: Option<Vec<Vec<u8>>>,
    /// Restrict results to events with any of these IDs (multi-id `IN` pushdown).
    pub ids: Option<Vec<Vec<u8>>>,
    /// Restrict results to events with an `e` tag referencing any of these event IDs (hex).
    /// Uses JSONB containment (`tags @> ...`) against the `tags` column.
    pub e_tags: Option<Vec<String>>,
    /// Restrict results to events in any of these channels. By default,
    /// channel-less global events are retained so this can enforce a viewer's
    /// accessible-channel scope without hiding global events. Set
    /// [`EventQuery::channel_ids_include_global`] to `false` for an explicit
    /// multi-channel `#h` filter, which must match only requested channels.
    /// Applied before SQL `LIMIT` so access- and filter-scoped historical pages
    /// have exact exhaustion semantics.
    pub channel_ids: Option<Vec<uuid::Uuid>>,
    /// Whether [`EventQuery::channel_ids`] also retains channel-less global
    /// events. Defaults to `true` for access-scope queries.
    pub channel_ids_include_global: bool,
    /// Override the default page clamp ([`DEFAULT_MAX_PAGE_LIMIT`]). Used by
    /// the COUNT fallback path, which needs to fetch all matching events for
    /// post-filter counting. When None, the default clamp applies.
    pub max_limit: Option<i64>,
    /// Shared-gated visibility reader: when set, append an SQL visibility
    /// clause for every kind in [`SHARED_GATED_KINDS`] before ORDER/LIMIT so
    /// private events are excluded from the candidate page rather than
    /// discarded after it.
    ///
    /// The clause is: `AND (kind NOT IN (...) OR pubkey = $reader OR tags @> ?)`,
    /// where the `IN` list is [`SHARED_GATED_KINDS`] and `?` is the JSONB
    /// literal `[["shared","true"]]`.  The GIN index on `tags` (migration 0004,
    /// jsonb_path_ops) makes the containment check fast.
    ///
    /// NOTE: `tags @> '[["shared","true"]]'` uses JSONB containment, which
    /// matches any tag array that is a superset of `[["shared","true"]]` — it
    /// would match `["shared","true","extra"]` too.  The ingest `parts.len() ==
    /// 2` exact-shape check ensures such malformed tags are never stored, so the
    /// SQL pushdown is sound.  Keeping `event_visible_to_reader` as post-filter
    /// defense-in-depth catches any residual mismatch.
    pub shared_gated_reader: Option<Vec<u8>>,
}

impl EventQuery {
    /// Construct an unconstrained query inside a server-resolved community.
    ///
    /// `community_id` has no safe default. This keeps call sites concise while
    /// making tenant provenance explicit at construction.
    #[must_use]
    pub const fn for_community(community_id: CommunityId) -> Self {
        Self {
            community_id,
            channel_id: None,
            kinds: None,
            pubkey: None,
            since: None,
            until: None,
            limit: None,
            offset: None,
            p_tag_hex: None,
            d_tag: None,
            d_tags: None,
            before_id: None,
            global_only: false,
            authors: None,
            ids: None,
            e_tags: None,
            channel_ids: None,
            channel_ids_include_global: true,
            max_limit: None,
            shared_gated_reader: None,
        }
    }
}

/// Result of atomically inserting a kind:7 reaction event and its reaction row.
#[derive(Debug)]
pub enum ReactionEventInsertOutcome {
    /// Target event was absent in this community, or was soft-deleted. No writes committed.
    TargetMissing,
    /// The active `(target, actor, emoji)` reaction already exists. No event was stored.
    Duplicate,
    /// Reaction row and event transaction committed.
    Inserted {
        /// Stored reaction event.
        stored_event: Box<StoredEvent>,
        /// Whether the event row itself was newly inserted.
        was_inserted: bool,
    },
}

/// Maximum length for a `d_tag` value (bytes). NIP-33 d-tags are short identifiers;
/// anything beyond this is either a bug or abuse.
pub const D_TAG_MAX_LEN: usize = 1024;

/// Maximum huddle-start content bytes considered by the parent-link lookup.
///
/// The canonical content is a small JSON object containing one UUID. Rejecting
/// oversized candidates keeps a malformed lifecycle event from making audio
/// admission pull large text rows into memory.
const HUDDLE_LINK_CONTENT_MAX_BYTES: i64 = 512;
/// Maximum candidate rows inspected after SQL prefiltering by parent, creator,
/// kind, and UUID substring.
const HUDDLE_LINK_CANDIDATE_LIMIT: i64 = 32;

/// Extract the `d_tag` value for storage.
///
/// For NIP-33 parameterized replaceable events (kind 30000–39999): returns the first
/// `d` tag's value, or `""` if no `d` tag is present (per NIP-33 spec).
/// For all other events: returns `None` (column stays NULL).
pub fn extract_d_tag(event: &Event) -> Option<String> {
    let kind_u32 = event.kind.as_u16() as u32;
    if !is_parameterized_replaceable(kind_u32) {
        return None;
    }
    let val = event
        .tags
        .iter()
        .find_map(|tag| {
            let parts = tag.as_slice();
            if parts.len() >= 2 && parts[0] == "d" {
                Some(parts[1].to_string())
            } else {
                None
            }
        })
        .unwrap_or_default(); // Missing d tag → empty string per NIP-33
    Some(val)
}

/// Extract the `not_before` timestamp for materialization in the `events` table.
///
/// Only applies to `kind:30300` (NIP-ER event reminders). Returns the first
/// valid `not_before` tag value as an `i64` Unix timestamp, or `None` if the
/// event is not a reminder or has no `not_before` tag.
pub fn extract_not_before(event: &Event) -> Option<i64> {
    let kind_u32 = event.kind.as_u16() as u32;
    if kind_u32 != KIND_EVENT_REMINDER {
        return None;
    }
    event.tags.iter().find_map(|tag| {
        let parts = tag.as_slice();
        if parts.len() >= 2 && parts[0] == "not_before" {
            parts[1].parse::<i64>().ok()
        } else {
            None
        }
    })
}

fn huddle_started_content_links(content: &str, ephemeral_channel_id: Uuid) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| {
            value
                .get("ephemeral_channel_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
        })
        .is_some_and(|id| id == ephemeral_channel_id)
}

/// Return whether `parent_channel_id` has a creator-signed huddle-start event
/// that links to `ephemeral_channel_id`.
///
/// The creator constraint matters: a member of some unrelated channel can post
/// their own kind:48100 event there, but they cannot sign as the creator of the
/// target ephemeral channel.
pub async fn huddle_started_link_exists(
    pool: &PgPool,
    community_id: CommunityId,
    parent_channel_id: Uuid,
    ephemeral_channel_id: Uuid,
    creator_pubkey: &[u8],
) -> Result<bool> {
    let uuid_needle = format!("%{}%", ephemeral_channel_id);
    let candidates: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT content
        FROM events
        WHERE deleted_at IS NULL
          AND community_id = $1
          AND channel_id = $2
          AND kind = $3
          AND pubkey = $4
          AND octet_length(content) <= $5
          AND content ILIKE $6
        ORDER BY created_at DESC, id ASC
        LIMIT $7
        "#,
    )
    .bind(community_id.as_uuid())
    .bind(parent_channel_id)
    .bind(KIND_HUDDLE_STARTED as i32)
    .bind(creator_pubkey)
    .bind(HUDDLE_LINK_CONTENT_MAX_BYTES)
    .bind(uuid_needle)
    .bind(HUDDLE_LINK_CANDIDATE_LIMIT)
    .fetch_all(pool)
    .await?;

    Ok(candidates
        .iter()
        .any(|content| huddle_started_content_links(content, ephemeral_channel_id)))
}

/// Insert a Nostr event. Rejects AUTH and ephemeral kinds.
///
/// Returns `(StoredEvent, was_inserted)` — `was_inserted` is `false` on duplicate.
pub async fn insert_event(
    pool: &PgPool,
    community_id: CommunityId,
    event: &Event,
    channel_id: Option<Uuid>,
) -> Result<(StoredEvent, bool)> {
    let kind_u16 = event.kind.as_u16();
    let kind_u32 = u32::from(kind_u16);

    if kind_u32 == KIND_AUTH {
        return Err(DbError::AuthEventRejected);
    }
    if is_ephemeral(kind_u32) {
        return Err(DbError::EphemeralEventRejected(kind_u16));
    }

    let id_bytes = event.id.as_bytes();
    let pubkey_bytes = event.pubkey.to_bytes();
    let sig_bytes = event.sig.serialize();
    let tags_json = serde_json::to_value(&event.tags)?;
    // Cast chain: nostr Kind (u16) → i32 (Postgres INT column). Safe: all Buzz kinds fit in i32.
    let kind_i32 = event_kind_i32(event);
    let created_at_secs = event.created_at.as_secs() as i64;
    let created_at = DateTime::from_timestamp(created_at_secs, 0)
        .ok_or(DbError::InvalidTimestamp(created_at_secs))?;
    let received_at = Utc::now();
    let d_tag = extract_d_tag(event);
    let not_before = extract_not_before(event);
    let result = sqlx::query(
        r#"
        INSERT INTO events (community_id, id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id, d_tag, not_before)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(community_id.as_uuid())
    .bind(id_bytes.as_slice())
    .bind(pubkey_bytes.as_slice())
    .bind(created_at)
    .bind(kind_i32)
    .bind(&tags_json)
    .bind(&event.content)
    .bind(sig_bytes.as_slice())
    .bind(received_at)
    .bind(channel_id)
    .bind(d_tag.as_deref())
    .bind(not_before)
    .execute(pool)
    .await?;

    let was_inserted = result.rows_affected() > 0;

    Ok((
        StoredEvent::with_received_at(event.clone(), received_at, channel_id, true),
        was_inserted,
    ))
}

/// Query events with optional filters. Results ordered by `created_at DESC`.
///
/// Uses `QueryBuilder` for dynamic filter composition — avoids string concatenation
/// while keeping all user values in bind parameters.
pub async fn query_events(pool: &PgPool, q: &EventQuery) -> Result<Vec<StoredEvent>> {
    let mut conn = pool.acquire().await?;
    query_events_on(&mut conn, q).await
}

/// [`query_events`] on a specific session — the replica-routing path runs
/// follow-up (aux) queries on the exact reader connection whose heartbeat
/// observation proved coverage for the page they annotate.
pub(crate) async fn query_events_on(
    conn: &mut sqlx::PgConnection,
    q: &EventQuery,
) -> Result<Vec<StoredEvent>> {
    // Composite cursor requires both halves.
    if q.before_id.is_some() && q.until.is_none() {
        return Err(DbError::InvalidData(
            "before_id requires until to be set".to_string(),
        ));
    }

    // global_only and channel_id are mutually exclusive.
    if q.global_only && q.channel_id.is_some() {
        return Err(DbError::InvalidData(
            "global_only and channel_id are mutually exclusive".to_string(),
        ));
    }

    // Empty list means "match nothing" — return empty immediately.
    if q.kinds.as_deref().is_some_and(|k| k.is_empty()) {
        return Ok(vec![]);
    }
    if q.authors.as_deref().is_some_and(|a| a.is_empty()) {
        return Ok(vec![]);
    }
    if q.ids.as_deref().is_some_and(|i| i.is_empty()) {
        return Ok(vec![]);
    }
    if q.e_tags.as_deref().is_some_and(|e| e.is_empty()) {
        return Ok(vec![]);
    }

    let clamp = q.max_limit.unwrap_or(DEFAULT_MAX_PAGE_LIMIT);
    let limit_val = q.limit.unwrap_or(100).min(clamp);
    let offset_val = q.offset.unwrap_or(0);

    let mut qb: QueryBuilder<sqlx::Postgres> = if let Some(ref p_hex) = q.p_tag_hex {
        // Join against event_mentions for #p-filtered queries (indexed).
        let mut b = QueryBuilder::new(
            "SELECT e.id, e.pubkey, e.created_at, e.kind, e.tags, e.content, \
             e.sig, e.received_at, e.channel_id \
             FROM events e \
             INNER JOIN event_mentions m \
                ON e.community_id = m.community_id AND e.id = m.event_id \
             WHERE e.community_id = ",
        );
        b.push_bind(q.community_id.as_uuid());
        b.push(" AND m.community_id = ");
        b.push_bind(q.community_id.as_uuid());
        b.push(" AND e.deleted_at IS NULL AND m.pubkey_hex = ");
        b.push_bind(p_hex.to_ascii_lowercase());
        b
    } else {
        let mut b = QueryBuilder::new(
            "SELECT id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id \
             FROM events WHERE community_id = ",
        );
        b.push_bind(q.community_id.as_uuid());
        b.push(" AND deleted_at IS NULL");
        b
    };

    // Use unqualified column names when no join, qualified when joined.
    let col_prefix = if q.p_tag_hex.is_some() { "e." } else { "" };

    if let Some(ch) = q.channel_id {
        qb.push(format!(" AND {col_prefix}channel_id = "))
            .push_bind(ch);
    } else if q.global_only {
        qb.push(format!(" AND {col_prefix}channel_id IS NULL"));
    }

    // Multi-channel IN pushdown. Access-scope queries retain global events;
    // explicit multi-value #h filters do not.
    //
    // SECURITY: Some(empty vec) means "match no channels". Access-scope
    // queries still retain globals; explicit #h queries match nothing.
    if let Some(ref ch_ids) = q.channel_ids {
        if ch_ids.is_empty() {
            if q.channel_ids_include_global {
                qb.push(format!(" AND {col_prefix}channel_id IS NULL"));
            } else {
                qb.push(" AND FALSE");
            }
        } else {
            qb.push(" AND (");
            if q.channel_ids_include_global {
                qb.push(format!("{col_prefix}channel_id IS NULL OR "));
            }
            qb.push(format!("{col_prefix}channel_id IN ("));
            let mut sep = qb.separated(", ");
            for ch in ch_ids {
                sep.push_bind(*ch);
            }
            qb.push("))");
        }
    }

    if let Some(ks) = q.kinds.as_deref().filter(|k| !k.is_empty()) {
        qb.push(format!(" AND {col_prefix}kind IN ("));
        let mut sep = qb.separated(", ");
        for k in ks {
            sep.push_bind(*k);
        }
        qb.push(")");
    }

    if let Some(ref pk) = q.pubkey {
        qb.push(format!(" AND {col_prefix}pubkey = "))
            .push_bind(pk.clone());
    }

    // Multi-author IN pushdown (mutually exclusive with single pubkey in practice).
    if let Some(ref authors) = q.authors {
        if !authors.is_empty() {
            qb.push(format!(" AND {col_prefix}pubkey IN ("));
            let mut sep = qb.separated(", ");
            for a in authors {
                sep.push_bind(a.clone());
            }
            qb.push(")");
        }
    }

    // Multi-id IN pushdown.
    if let Some(ref ids) = q.ids {
        if !ids.is_empty() {
            qb.push(format!(" AND {col_prefix}id IN ("));
            let mut sep = qb.separated(", ");
            for id in ids {
                sep.push_bind(id.clone());
            }
            qb.push(")");
        }
    }

    // e-tag pushdown via JSONB containment: tags @> '[["e","<hex>"]]'.
    // Multiple e-tags use OR (any match). Served by idx_events_tags_gin
    // (GIN, jsonb_path_ops — migrations/0004): the channel-window aux closure
    // fans this out once per retained row, which made unindexed containment
    // the dominant scroll-back cost (~1.7s/page on staging).
    if let Some(ref e_tags) = q.e_tags {
        if !e_tags.is_empty() {
            qb.push(" AND (");
            for (i, hex_id) in e_tags.iter().enumerate() {
                if i > 0 {
                    qb.push(" OR ");
                }
                // Build the JSONB literal: [["e","<hex>"]]
                let containment = serde_json::json!([["e", hex_id]]);
                qb.push(format!("{col_prefix}tags @> "));
                qb.push_bind(containment);
            }
            qb.push(")");
        }
    }

    if let Some(s) = q.since {
        qb.push(format!(" AND {col_prefix}created_at >= "))
            .push_bind(s);
    }
    if let Some(u) = q.until {
        if let Some(ref bid) = q.before_id {
            // Composite keyset cursor for stable pagination.
            // With ORDER BY created_at DESC, id ASC, "next page" means:
            //   created_at < cursor_ts OR (created_at = cursor_ts AND id > cursor_id)
            qb.push(format!(" AND ({col_prefix}created_at < "));
            qb.push_bind(u);
            qb.push(format!(" OR ({col_prefix}created_at = "));
            qb.push_bind(u);
            qb.push(format!(" AND {col_prefix}id > "));
            qb.push_bind(bid.clone());
            qb.push("))");
        } else {
            qb.push(format!(" AND {col_prefix}created_at <= "))
                .push_bind(u);
        }
    }

    if let Some(ref d) = q.d_tag {
        qb.push(format!(" AND {col_prefix}d_tag = "))
            .push_bind(d.clone());
    } else if let Some(ref ds) = q.d_tags {
        if !ds.is_empty() {
            qb.push(format!(" AND {col_prefix}d_tag IN ("));
            let mut sep = qb.separated(", ");
            for d in ds {
                sep.push_bind(d.clone());
            }
            qb.push(")");
        }
    }

    // Shared-gated visibility pushdown: exclude SHARED_GATED_KINDS events that
    // are neither authored by the reader nor explicitly shared.  Applied BEFORE
    // ORDER/LIMIT so that a page of newer private events does not push visible
    // shared ones off the end of the result set (the catalog query pattern).
    //
    // Clause: AND (kind NOT IN (30175, 30178) OR pubkey = $reader
    //              OR tags @> '[["shared","true"]]')
    //
    // The JSONB containment check is served by idx_events_tags_gin (migration
    // 0004, jsonb_path_ops).  `tags @> '[["shared","true"]]'` matches any array
    // that contains exactly the sub-array — a two-element `["shared","true"]`
    // tag passes; a tag-absent event does not.  Because ingest requires exactly
    // two elements for the shared tag (parts.len() == 2), no stored event can
    // carry a three-element superset.
    if let Some(ref reader_bytes) = q.shared_gated_reader {
        let shared_containment = serde_json::json!([["shared", "true"]]);
        qb.push(format!(" AND ({col_prefix}kind NOT IN ("));
        let mut sep = qb.separated(", ");
        for kind in SHARED_GATED_KINDS {
            sep.push_bind(*kind as i32);
        }
        qb.push(format!(") OR {col_prefix}pubkey = "));
        qb.push_bind(reader_bytes.clone());
        qb.push(format!(" OR {col_prefix}tags @> "));
        qb.push_bind(shared_containment);
        qb.push(")");
    }

    // Composite ordering for deterministic pagination across ALL callers of
    // query_events (WebSocket REQ, REST endpoints, canvas, notes, etc.).
    // The `id ASC` tiebreaker ensures stable results when events share the
    // same second.  No existing index covers this trailing column — Postgres
    // sorts in memory, which is fine at current scale.  If query performance
    // degrades, add a composite index like `(pubkey, kind, created_at DESC, id ASC)`.
    qb.push(format!(
        " ORDER BY {col_prefix}created_at DESC, {col_prefix}id ASC LIMIT "
    ));
    qb.push_bind(limit_val);
    qb.push(" OFFSET ").push_bind(offset_val);

    let rows = qb.build().fetch_all(&mut *conn).await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(ev) = row_to_stored_event(row)? {
            out.push(ev);
        }
    }
    Ok(out)
}

pub(crate) fn row_to_stored_event(row: sqlx::postgres::PgRow) -> Result<Option<StoredEvent>> {
    let id_bytes: Vec<u8> = row.try_get("id")?;
    let pubkey_bytes: Vec<u8> = row.try_get("pubkey")?;
    let created_at: DateTime<Utc> = row.try_get("created_at")?;
    let kind_i32: i32 = row.try_get("kind")?;
    let tags_json: serde_json::Value = row.try_get("tags")?;
    let content: String = row.try_get("content")?;
    let sig_bytes: Vec<u8> = row.try_get("sig")?;
    let received_at: DateTime<Utc> = row.try_get("received_at")?;

    let channel_id: Option<Uuid> = row.try_get("channel_id")?;

    // kind is stored as i32 (Postgres INT) but Nostr uses u16. Values > 65535 are corrupt.
    let kind_u16 = u16::try_from(kind_i32)
        .map_err(|_| DbError::InvalidData(format!("kind out of u16 range: {kind_i32}")))?;

    let event_json = serde_json::json!({
        "id": hex::encode(&id_bytes),
        "pubkey": hex::encode(&pubkey_bytes),
        "created_at": created_at.timestamp(),
        "kind": kind_u16,
        "tags": tags_json,
        "content": content,
        "sig": hex::encode(&sig_bytes),
    });

    // Avoid the Value → String → parse round-trip: deserialize directly from the Value.
    let event: nostr::Event = match serde_json::from_value(event_json) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("failed to reconstruct event from DB row: {e}");
            return Ok(None);
        }
    };

    Ok(Some(StoredEvent::with_received_at(
        event,
        received_at,
        channel_id,
        true,
    )))
}

/// Count events matching the given query parameters (NIP-45 COUNT support).
///
/// Uses the same filter logic as `query_events` but returns only the count.
pub async fn count_events(pool: &PgPool, q: &EventQuery) -> Result<i64> {
    let mut conn = pool.acquire().await?;
    count_events_on(&mut conn, q).await
}

/// [`count_events`] on a specific session — the replica-routing path runs
/// the count on the exact reader connection whose heartbeat observation
/// proved its predicate.
pub(crate) async fn count_events_on(conn: &mut sqlx::PgConnection, q: &EventQuery) -> Result<i64> {
    // Empty list means "match nothing" — return 0 immediately.
    if q.kinds.as_deref().is_some_and(|k| k.is_empty()) {
        return Ok(0);
    }
    if q.authors.as_deref().is_some_and(|a| a.is_empty()) {
        return Ok(0);
    }
    if q.ids.as_deref().is_some_and(|i| i.is_empty()) {
        return Ok(0);
    }
    if q.e_tags.as_deref().is_some_and(|e| e.is_empty()) {
        return Ok(0);
    }

    let mut qb: QueryBuilder<sqlx::Postgres> = if let Some(ref p_hex) = q.p_tag_hex {
        let mut b = QueryBuilder::new(
            "SELECT COUNT(*) as cnt FROM events e \
             INNER JOIN event_mentions m \
                ON e.community_id = m.community_id AND e.id = m.event_id \
             WHERE e.community_id = ",
        );
        b.push_bind(q.community_id.as_uuid());
        b.push(" AND m.community_id = ");
        b.push_bind(q.community_id.as_uuid());
        b.push(" AND e.deleted_at IS NULL AND m.pubkey_hex = ");
        b.push_bind(p_hex.to_ascii_lowercase());
        b
    } else {
        let mut b = QueryBuilder::new("SELECT COUNT(*) as cnt FROM events WHERE community_id = ");
        b.push_bind(q.community_id.as_uuid());
        b.push(" AND deleted_at IS NULL");
        b
    };

    let col_prefix = if q.p_tag_hex.is_some() { "e." } else { "" };

    if let Some(ch) = q.channel_id {
        qb.push(format!(" AND {col_prefix}channel_id = "))
            .push_bind(ch);
    } else if q.global_only {
        qb.push(format!(" AND {col_prefix}channel_id IS NULL"));
    }

    // Multi-channel IN pushdown for COUNT. Access-scope queries retain global
    // events; explicit multi-value #h filters do not.
    if let Some(ref ch_ids) = q.channel_ids {
        if ch_ids.is_empty() {
            if q.channel_ids_include_global {
                qb.push(format!(" AND {col_prefix}channel_id IS NULL"));
            } else {
                qb.push(" AND FALSE");
            }
        } else {
            qb.push(" AND (");
            if q.channel_ids_include_global {
                qb.push(format!("{col_prefix}channel_id IS NULL OR "));
            }
            qb.push(format!("{col_prefix}channel_id IN ("));
            let mut sep = qb.separated(", ");
            for ch in ch_ids {
                sep.push_bind(*ch);
            }
            qb.push("))");
        }
    }

    if let Some(ks) = q.kinds.as_deref().filter(|k| !k.is_empty()) {
        qb.push(format!(" AND {col_prefix}kind IN ("));
        let mut sep = qb.separated(", ");
        for k in ks {
            sep.push_bind(*k);
        }
        qb.push(")");
    }

    if let Some(ref pk) = q.pubkey {
        qb.push(format!(" AND {col_prefix}pubkey = "))
            .push_bind(pk.clone());
    }

    if let Some(ref authors) = q.authors {
        if !authors.is_empty() {
            qb.push(format!(" AND {col_prefix}pubkey IN ("));
            let mut sep = qb.separated(", ");
            for a in authors {
                sep.push_bind(a.clone());
            }
            qb.push(")");
        }
    }

    if let Some(ref ids) = q.ids {
        if !ids.is_empty() {
            qb.push(format!(" AND {col_prefix}id IN ("));
            let mut sep = qb.separated(", ");
            for id in ids {
                sep.push_bind(id.clone());
            }
            qb.push(")");
        }
    }

    if let Some(ref e_tags) = q.e_tags {
        if !e_tags.is_empty() {
            qb.push(" AND (");
            for (i, hex_id) in e_tags.iter().enumerate() {
                if i > 0 {
                    qb.push(" OR ");
                }
                let containment = serde_json::json!([["e", hex_id]]);
                qb.push(format!("{col_prefix}tags @> "));
                qb.push_bind(containment);
            }
            qb.push(")");
        }
    }

    if let Some(s) = q.since {
        qb.push(format!(" AND {col_prefix}created_at >= "))
            .push_bind(s);
    }
    if let Some(u) = q.until {
        qb.push(format!(" AND {col_prefix}created_at <= "))
            .push_bind(u);
    }

    if let Some(ref d) = q.d_tag {
        qb.push(format!(" AND {col_prefix}d_tag = "))
            .push_bind(d.clone());
    } else if let Some(ref ds) = q.d_tags {
        if !ds.is_empty() {
            qb.push(format!(" AND {col_prefix}d_tag IN ("));
            let mut sep = qb.separated(", ");
            for d in ds {
                sep.push_bind(d.clone());
            }
            qb.push(")");
        }
    }

    let row = qb.build().fetch_one(&mut *conn).await?;
    let cnt: i64 = row.try_get("cnt")?;

    Ok(cnt)
}

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
    let result = sqlx::query(
        "UPDATE events SET deleted_at = NOW() WHERE community_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
            .bind(community_id.as_uuid())
            .bind(event_id)
            .execute(pool)
            .await?;

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
    .execute(pool)
    .await?;

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

/// Returns the `created_at` timestamp of the most recent non-deleted event in a channel.
pub async fn get_last_message_at(
    pool: &PgPool,
    community_id: CommunityId,
    channel_id: uuid::Uuid,
) -> Result<Option<DateTime<Utc>>> {
    let row = sqlx::query(
        "SELECT created_at FROM events \
         WHERE community_id = $1 AND channel_id = $2 AND deleted_at IS NULL \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(community_id.as_uuid())
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => Ok(Some(r.try_get("created_at")?)),
        None => Ok(None),
    }
}

/// Bulk-fetch the most recent `created_at` for a set of channel IDs.
///
/// Returns a map of `channel_id → last_message_at`. Channels with no events are omitted.
/// Single query regardless of input size.
pub async fn get_last_message_at_bulk(
    pool: &PgPool,
    community_id: CommunityId,
    channel_ids: &[uuid::Uuid],
) -> Result<std::collections::HashMap<uuid::Uuid, DateTime<Utc>>> {
    if channel_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "SELECT channel_id, MAX(created_at) as last_at FROM events \
         WHERE community_id = ",
    );
    qb.push_bind(community_id.as_uuid());
    qb.push(" AND deleted_at IS NULL AND channel_id IN (");
    let mut sep = qb.separated(", ");
    for id in channel_ids {
        sep.push_bind(*id);
    }
    qb.push(") GROUP BY channel_id");

    let rows = qb.build().fetch_all(pool).await?;

    let mut map = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        let id: Uuid = row.try_get("channel_id")?;
        let last_at: DateTime<Utc> = row.try_get("last_at")?;
        map.insert(id, last_at);
    }
    Ok(map)
}

/// Fetches a single non-deleted event by its raw 32-byte ID.
///
/// Returns `None` if the event does not exist or has been soft-deleted.
/// Use [`get_event_by_id_including_deleted`] when you need to inspect
/// tombstoned rows (e.g. audit, undelete).
pub async fn get_event_by_id(
    pool: &PgPool,
    community_id: CommunityId,
    id_bytes: &[u8],
) -> Result<Option<StoredEvent>> {
    let row = sqlx::query(
        "SELECT id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id \
         FROM events WHERE community_id = $1 AND id = $2 AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
    )
    .bind(community_id.as_uuid())
    .bind(id_bytes)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => row_to_stored_event(r),
        None => Ok(None),
    }
}

/// Fetches the latest global (non-channel, `channel_id IS NULL`) replaceable event
/// for a (kind, pubkey) pair.
///
/// Uses canonical NIP-16 ordering: `created_at DESC, id ASC LIMIT 1`.
/// This matches the write path's tie-breaking logic and handles historical
/// duplicate survivors where multiple live rows share the same timestamp.
pub async fn get_latest_global_replaceable(
    pool: &PgPool,
    community_id: CommunityId,
    kind: i32,
    pubkey_bytes: &[u8],
) -> Result<Option<StoredEvent>> {
    let row = sqlx::query(
        "SELECT id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id \
         FROM events \
         WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND channel_id IS NULL AND deleted_at IS NULL \
         ORDER BY created_at DESC, id ASC \
         LIMIT 1",
    )
    .bind(community_id.as_uuid())
    .bind(kind)
    .bind(pubkey_bytes)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => row_to_stored_event(r),
        None => Ok(None),
    }
}

/// Fetches a single event by its raw 32-byte ID, **including soft-deleted rows**.
///
/// Most callers should use [`get_event_by_id`] instead. This variant is needed
/// when the caller must distinguish "never existed" from "was deleted" (e.g.
/// audit trails, compliance queries).
pub async fn get_event_by_id_including_deleted(
    pool: &PgPool,
    community_id: CommunityId,
    id_bytes: &[u8],
) -> Result<Option<StoredEvent>> {
    let row = sqlx::query(
        "SELECT id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id \
         FROM events WHERE community_id = $1 AND id = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(community_id.as_uuid())
    .bind(id_bytes)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => row_to_stored_event(r),
        None => Ok(None),
    }
}

/// Batch-fetch non-deleted events by their raw 32-byte IDs.
///
/// Returns events in arbitrary order — callers reorder as needed.
/// Uses a single `WHERE id IN (...)` query regardless of input size.
pub async fn get_events_by_ids(
    pool: &PgPool,
    community_id: CommunityId,
    ids: &[&[u8]],
) -> Result<Vec<StoredEvent>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut conn = pool.acquire().await?;
    get_events_by_ids_on(&mut conn, community_id, ids).await
}

/// [`get_events_by_ids`] on a specific session — the replica-routing path
/// runs the query on the exact reader connection whose heartbeat
/// observation proved its predicate.
pub(crate) async fn get_events_by_ids_on(
    conn: &mut sqlx::PgConnection,
    community_id: CommunityId,
    ids: &[&[u8]],
) -> Result<Vec<StoredEvent>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    debug_assert!(ids.len() <= 500, "batch fetch should be bounded by caller");

    let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "SELECT id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id \
         FROM events WHERE community_id = ",
    );
    qb.push_bind(community_id.as_uuid());
    qb.push(" AND deleted_at IS NULL AND id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(id.to_vec());
    }
    qb.push(")");

    let rows = qb.build().fetch_all(&mut *conn).await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(ev) = row_to_stored_event(row)? {
            out.push(ev);
        }
    }
    Ok(out)
}

/// Parameters for [`insert_event_with_thread_metadata`].
#[derive(Debug)]
pub struct ThreadMetadataParams<'a> {
    /// The Nostr event ID of this message.
    pub event_id: &'a [u8],
    /// When the event was created.
    pub event_created_at: DateTime<Utc>,
    /// The channel this event belongs to.
    pub channel_id: Uuid,
    /// Event ID of the direct parent, if this is a reply.
    pub parent_event_id: Option<&'a [u8]>,
    /// When the parent event was created.
    pub parent_event_created_at: Option<DateTime<Utc>>,
    /// Event ID of the thread root, if this is a nested reply.
    pub root_event_id: Option<&'a [u8]>,
    /// When the root event was created.
    pub root_event_created_at: Option<DateTime<Utc>>,
    /// Nesting depth (root = 0).
    pub depth: i32,
    /// Whether this reply is broadcast to the channel timeline.
    pub broadcast: bool,
}

pub(crate) async fn insert_event_with_thread_metadata_tx(
    tx: &mut Transaction<'_, Postgres>,
    community_id: CommunityId,
    event: &Event,
    channel_id: Option<Uuid>,
    thread_meta: Option<ThreadMetadataParams<'_>>,
) -> Result<(StoredEvent, bool)> {
    let kind_u16 = event.kind.as_u16();
    let kind_u32 = u32::from(kind_u16);

    if kind_u32 == KIND_AUTH {
        return Err(DbError::AuthEventRejected);
    }
    if is_ephemeral(kind_u32) {
        return Err(DbError::EphemeralEventRejected(kind_u16));
    }

    let id_bytes = event.id.as_bytes();
    let pubkey_bytes = event.pubkey.to_bytes();
    let sig_bytes = event.sig.serialize();
    let tags_json = serde_json::to_value(&event.tags)?;
    let kind_i32 = event_kind_i32(event);
    let created_at_secs = event.created_at.as_secs() as i64;
    let created_at = DateTime::from_timestamp(created_at_secs, 0)
        .ok_or(DbError::InvalidTimestamp(created_at_secs))?;
    let received_at = Utc::now();
    let d_tag = extract_d_tag(event);
    let not_before = extract_not_before(event);

    let result = sqlx::query(
        r#"
        INSERT INTO events (community_id, id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id, d_tag, not_before)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(community_id.as_uuid())
    .bind(id_bytes.as_slice())
    .bind(pubkey_bytes.as_slice())
    .bind(created_at)
    .bind(kind_i32)
    .bind(&tags_json)
    .bind(&event.content)
    .bind(sig_bytes.as_slice())
    .bind(received_at)
    .bind(channel_id)
    .bind(d_tag.as_deref())
    .bind(not_before)
    .execute(&mut **tx)
    .await?;

    let was_inserted = result.rows_affected() > 0;

    if was_inserted {
        if let Some(ref meta) = thread_meta {
            let broadcast_val: bool = meta.broadcast;

            let tm_result = sqlx::query(
                r#"
                INSERT INTO thread_metadata
                    (community_id, event_created_at, event_id, channel_id,
                     parent_event_id, parent_event_created_at,
                     root_event_id, root_event_created_at,
                     depth, broadcast)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(community_id.as_uuid())
            .bind(meta.event_created_at)
            .bind(meta.event_id)
            .bind(meta.channel_id)
            .bind(meta.parent_event_id)
            .bind(meta.parent_event_created_at)
            .bind(meta.root_event_id)
            .bind(meta.root_event_created_at)
            .bind(meta.depth)
            .bind(broadcast_val)
            .execute(&mut **tx)
            .await?;

            // Only bump reply counts if the metadata row was actually inserted.
            if tm_result.rows_affected() > 0 {
                if let Some(pid) = meta.parent_event_id {
                    // Ensure the parent has a thread_metadata row so the UPDATE
                    // below has something to hit. Root (depth=0) messages don't
                    // get a row on first insert, so we create a stub here.
                    let parent_ts = meta
                        .parent_event_created_at
                        .unwrap_or(meta.event_created_at);
                    sqlx::query(
                        r#"
                        INSERT INTO thread_metadata
                            (community_id, event_created_at, event_id, channel_id,
                             parent_event_id, parent_event_created_at,
                             root_event_id, root_event_created_at,
                             depth, broadcast)
                        VALUES ($1, $2, $3, $4, NULL, NULL, NULL, NULL, 0, false)
                        ON CONFLICT DO NOTHING
                        "#,
                    )
                    .bind(community_id.as_uuid())
                    .bind(parent_ts)
                    .bind(pid)
                    .bind(meta.channel_id)
                    .execute(&mut **tx)
                    .await?;

                    // Ensure the root also has a row (may differ from parent for nested replies).
                    if let Some(root_id) = meta.root_event_id {
                        if root_id != pid {
                            let root_ts =
                                meta.root_event_created_at.unwrap_or(meta.event_created_at);
                            sqlx::query(
                                r#"
                                INSERT INTO thread_metadata
                                    (community_id, event_created_at, event_id, channel_id,
                                     parent_event_id, parent_event_created_at,
                                     root_event_id, root_event_created_at,
                                     depth, broadcast)
                                VALUES ($1, $2, $3, $4, NULL, NULL, NULL, NULL, 0, false)
                                ON CONFLICT DO NOTHING
                                "#,
                            )
                            .bind(community_id.as_uuid())
                            .bind(root_ts)
                            .bind(root_id)
                            .bind(meta.channel_id)
                            .execute(&mut **tx)
                            .await?;
                        }
                    }

                    sqlx::query(
                        r#"
                        UPDATE thread_metadata
                        SET reply_count = reply_count + 1, last_reply_at = NOW()
                        WHERE community_id = $1 AND event_id = $2
                        "#,
                    )
                    .bind(community_id.as_uuid())
                    .bind(pid)
                    .execute(&mut **tx)
                    .await?;

                    if let Some(root_id) = meta.root_event_id {
                        sqlx::query(
                            r#"
                            UPDATE thread_metadata
                            SET descendant_count = descendant_count + 1
                            WHERE community_id = $1 AND event_id = $2
                            "#,
                        )
                        .bind(community_id.as_uuid())
                        .bind(root_id)
                        .execute(&mut **tx)
                        .await?;
                    }
                }
            }
        }
    }

    Ok((
        StoredEvent::with_received_at(event.clone(), received_at, channel_id, true),
        was_inserted,
    ))
}

/// Atomically insert an event and its optional thread metadata.
///
/// `insert_event` and `insert_thread_metadata` calls could leave reply counters
/// inconsistent if one succeeded and the other failed. Keep this as one
/// transaction so reply metadata and counters commit together with the event.
///
/// Returns `(StoredEvent, was_inserted)`.
pub async fn insert_event_with_thread_metadata(
    pool: &PgPool,
    community_id: CommunityId,
    event: &Event,
    channel_id: Option<Uuid>,
    thread_meta: Option<ThreadMetadataParams<'_>>,
) -> Result<(StoredEvent, bool)> {
    let mut tx = pool.begin().await?;
    let result =
        insert_event_with_thread_metadata_tx(&mut tx, community_id, event, channel_id, thread_meta)
            .await?;
    tx.commit().await?;
    Ok(result)
}

/// Atomically insert a kind:7 reaction event and its reaction row.
///
/// Ordering is load-bearing: resolve target, upsert/reactivate the reaction row,
/// check `rows_affected`, then insert the kind:7 event. Active duplicates return
/// before event insertion so duplicate reactions never store a duplicate kind:7.
#[allow(clippy::too_many_arguments)]
pub async fn insert_reaction_event_with_thread_metadata(
    pool: &PgPool,
    community_id: CommunityId,
    reaction_event: &Event,
    channel_id: Option<Uuid>,
    thread_meta: Option<ThreadMetadataParams<'_>>,
    target_event_id: &[u8],
    actor_pubkey: &[u8],
    emoji: &str,
) -> Result<ReactionEventInsertOutcome> {
    let mut tx = pool.begin().await?;

    let target_row = sqlx::query(
        "SELECT created_at FROM events \
         WHERE community_id = $1 AND id = $2 AND deleted_at IS NULL \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(community_id.as_uuid())
    .bind(target_event_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(target_row) = target_row else {
        tx.rollback().await?;
        return Ok(ReactionEventInsertOutcome::TargetMissing);
    };
    let target_created_at: DateTime<Utc> = target_row.get("created_at");

    // Preserve add_reaction's exact new / re-activate / active-duplicate semantics.
    let reaction_inserted = crate::reaction::add_reaction_tx(
        &mut tx,
        community_id,
        target_event_id,
        target_created_at,
        actor_pubkey,
        emoji,
        Some(reaction_event.id.as_bytes()),
    )
    .await?;

    if !reaction_inserted {
        tx.rollback().await?;
        return Ok(ReactionEventInsertOutcome::Duplicate);
    }

    let (stored_event, was_inserted) = insert_event_with_thread_metadata_tx(
        &mut tx,
        community_id,
        reaction_event,
        channel_id,
        thread_meta,
    )
    .await?;

    tx.commit().await?;

    Ok(ReactionEventInsertOutcome::Inserted {
        stored_event: Box::new(stored_event),
        was_inserted,
    })
}

/// The tag a coding-session genesis mirrors its umbrella reference into.
///
/// The relay's envelope validator has already proved this tag exists and agrees
/// with the signed content, so reading it here re-derives the uniqueness key
/// from the exact bytes that are about to be stored rather than from a
/// separately-passed argument that could drift from them.
const CODING_SESSION_TAG: &str = "csg-session";

/// Domain separator for the coding-session genesis advisory-lock key space.
///
/// Postgres advisory locks share one flat `bigint` key space across the whole
/// database, and this repo already keys five unrelated domains into it
/// (addressable replacement, channel names, push leases, community deletion,
/// audit chains). Folding a constant that no other domain hashes means genesis
/// keys cannot collide with another domain *systematically* — the way they
/// would if this reused [`crate::event_replacement_lock_key`]'s inputs.
///
/// See [`coding_session_genesis_lock_key`] for why an accidental collision is
/// nonetheless harmless, and why "provably collision-free" is not on offer.
const CODING_SESSION_GENESIS_LOCK_DOMAIN: &[u8] = b"buzz.coding-session.genesis.v1";

/// Derive the advisory-lock key that serializes genesis writes for one
/// `(community, channel, sessionRef)`.
///
/// # Scope
///
/// Exactly the tuple uniqueness is defined over — and deliberately **not** the
/// signer. Two rival founders racing to claim one reference is the case this
/// lock exists to decide, so folding the pubkey in would hand both of them
/// their own lock and let both commit. Conversely `sessionRef` and `channel_id`
/// are both folded in so that founding unrelated sessions never contends.
///
/// # Collisions
///
/// A 64-bit key derived from unbounded input cannot be injective, so this is
/// *not* provably collision-free against the other advisory-lock domains, and
/// no choice of domain constant would make it so. What the constant buys is the
/// absence of *structured* collision: no input this function accepts maps onto
/// another domain's key by construction, only by a ~2⁻⁶⁴ accident.
///
/// That residual accident is safe, because an advisory-lock collision can only
/// ever *add* mutual exclusion. Every domain's correctness argument is "no two
/// transactions in this domain run concurrently"; sharing a key with a foreign
/// domain gives a superset of that exclusion, never a subset. A collision costs
/// throughput and cannot cost correctness.
///
/// Nor can it deadlock. A deadlock needs a transaction that waits on a second
/// advisory lock while holding a first, and
/// [`insert_coding_session_genesis_event`] takes exactly one advisory lock, as
/// its first statement, and never takes another. A transaction that never waits
/// on a second lock cannot be an edge in a wait cycle, so genesis can be
/// *delayed* by a colliding domain but can never be part of a deadlock with
/// one.
fn coding_session_genesis_lock_key(
    community_id: CommunityId,
    channel_id: Uuid,
    session_ref: &str,
) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(CODING_SESSION_GENESIS_LOCK_DOMAIN);
    hasher.update(community_id.as_uuid().as_bytes());
    hasher.update(channel_id.as_bytes());
    // Length-prefixed so no reference can be confused with a different one by
    // running into the bytes that follow it.
    hasher.update((session_ref.len() as u64).to_le_bytes());
    hasher.update(session_ref.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 8];
    key.copy_from_slice(&digest[..8]);
    i64::from_le_bytes(key)
}

/// Read the umbrella reference a genesis event mirrors into its tags.
fn coding_session_genesis_session_ref(event: &Event) -> Result<&str> {
    event
        .tags
        .iter()
        .find_map(|tag| {
            let parts = tag.as_slice();
            match parts {
                [name, value, ..] if name == CODING_SESSION_TAG => Some(value.as_str()),
                _ => None,
            }
        })
        .ok_or_else(|| {
            DbError::InvalidData(format!(
                "coding-session genesis is missing its {CODING_SESSION_TAG} tag"
            ))
        })
}

/// Outcome of an attempted coding-session genesis (kind 44226) insert.
#[derive(Debug)]
pub enum CodingSessionGenesisInsertOutcome {
    /// This event founded the session. The transaction committed.
    Founded {
        /// The stored genesis event.
        stored_event: Box<StoredEvent>,
        /// Whether the event row itself was newly inserted. `false` means this
        /// exact event id was already stored — an idempotent resubmission, not
        /// a rival claim.
        was_inserted: bool,
    },
    /// A different genesis already founded this `(channel, sessionRef)`.
    /// Nothing was written. The winner may itself be soft-deleted — a founded
    /// reference is never released, see the `Soft deletion` section on
    /// [`insert_coding_session_genesis_event`].
    AlreadyFounded {
        /// Raw event id of the genesis that won, for the rejection message.
        existing_event_id: Vec<u8>,
    },
}

/// Atomically enforce one genesis per `(channel, sessionRef)` and store
/// the event.
///
/// # Why this lives here and not at ingest
///
/// The relay's ingest validators are pure functions running hundreds of lines
/// and several round trips ahead of storage, holding no transaction — a
/// `SELECT` there would be a check-then-insert race across relay processes, and
/// the property at stake is *which pubkey is a session's founder*. So the check
/// is moved into the storage transaction itself:
/// `begin` → `pg_advisory_xact_lock` → probe → insert → `commit`. The lock is
/// transaction-scoped, so every abort path releases it, and it is held across
/// both the probe and the insert. Two rival genesis events therefore cannot
/// both observe an empty probe: the loser blocks on the lock, and by the time
/// it runs its own probe the winner's row is committed and visible.
///
/// A unique index is not an option — `events` is `PARTITION BY RANGE
/// (created_at)`, so any unique constraint must include `created_at`, and two
/// rival claims at different timestamps would both satisfy it.
///
/// # Not a receipt
///
/// A rejected duplicate is a plain refusal. Nothing is written for it, and the
/// caller is expected to answer `OK false` — acceptance receipts are a separate
/// concern that does not belong in a uniqueness check.
///
/// # Resubmission
///
/// The probe excludes this event's own id, so replaying an identical genesis
/// stays idempotent: it falls through to `ON CONFLICT DO NOTHING` and returns
/// [`CodingSessionGenesisInsertOutcome::Founded`] with `was_inserted: false`,
/// exactly as any other replayed event does. Only a *different* event claiming
/// an already-founded reference is rejected.
///
/// # Soft deletion
///
/// The probe counts soft-deleted rows too, deliberately breaking with every
/// other uniqueness and replacement probe in this crate. A genesis is not a
/// piece of content occupying a name that a later writer may take over; it is
/// the permanent identity anchor for one umbrella. Releasing its reference on
/// deletion would hand a session's foundership to whoever asked next — the
/// exact outcome the lock above exists to prevent — so a founded reference
/// stays founded forever. Sessions end through lifecycle facts, never by
/// freeing identity.
///
/// The relay refuses NIP-09 deletion of a genesis outright, so a soft-deleted
/// genesis should not arise through the ordinary path at all. Counting deleted
/// rows here is defense in depth: any future path that marks one deleted (an
/// operator statement, a moderation tool, a bug) must not also silently reopen
/// the identity question.
///
/// This is not a claim that genesis *content* is unredactable. Identity facts
/// are permanent; redacting content is a separate question with a separate
/// answer, and conflating the two is what makes "delete" look like a way to
/// re-found a session.
pub async fn insert_coding_session_genesis_event(
    pool: &PgPool,
    community_id: CommunityId,
    event: &Event,
    channel_id: Uuid,
    thread_meta: Option<ThreadMetadataParams<'_>>,
) -> Result<CodingSessionGenesisInsertOutcome> {
    let session_ref = coding_session_genesis_session_ref(event)?;
    let lock_key = coding_session_genesis_lock_key(community_id, channel_id, session_ref);
    let kind_i32 = event_kind_i32(event);
    let session_probe = serde_json::json!([[CODING_SESSION_TAG, session_ref]]);
    let id_bytes = event.id.as_bytes();

    let mut tx = pool.begin().await?;

    // Taken first, before any read or write, and never joined by a second
    // advisory lock — see `coding_session_genesis_lock_key` on why that shape
    // makes genesis undeadlockable.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *tx)
        .await?;

    // `kind` leads the useful index here, not the tag GIN: genesis events are
    // rare, so `(community_id, kind, created_at)` narrows to the community's
    // sessions and the containment test runs over that handful.
    // `ORDER BY created_at ASC, id ASC` makes the reported winner deterministic
    // if history already holds more than one claim.
    //
    // No `deleted_at IS NULL` — a soft-deleted genesis still occupies its
    // reference. See the `Soft deletion` section above.
    let existing: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT id FROM events \
         WHERE community_id = $1 AND kind = $2 AND channel_id = $3 \
         AND id <> $4 AND tags @> $5::jsonb \
         ORDER BY created_at ASC, id ASC LIMIT 1",
    )
    .bind(community_id.as_uuid())
    .bind(kind_i32)
    .bind(channel_id)
    .bind(id_bytes.as_slice())
    .bind(&session_probe)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some((existing_event_id,)) = existing {
        tx.rollback().await?;
        return Ok(CodingSessionGenesisInsertOutcome::AlreadyFounded { existing_event_id });
    }

    let (stored_event, was_inserted) = insert_event_with_thread_metadata_tx(
        &mut tx,
        community_id,
        event,
        Some(channel_id),
        thread_meta,
    )
    .await?;

    tx.commit().await?;

    Ok(CodingSessionGenesisInsertOutcome::Founded {
        stored_event: Box::new(stored_event),
        was_inserted,
    })
}

/// A due reminder row returned by [`query_due_reminders`].
#[derive(Debug)]
pub struct DueReminder {
    /// Server-resolved community this reminder row belongs to.
    pub community_id: CommunityId,
    /// Normalized host mapped to that community.
    pub host: String,
    /// The event's raw ID bytes.
    pub id: Vec<u8>,
    /// The event's pubkey bytes.
    pub pubkey: Vec<u8>,
    /// The event's `created_at` timestamp.
    pub created_at: DateTime<Utc>,
    /// The event's kind (always 30300).
    pub kind: i32,
    /// The event's JSONB tags.
    pub tags: serde_json::Value,
    /// The event's encrypted content.
    pub content: String,
    /// The event's signature bytes.
    pub sig: Vec<u8>,
    /// The channel ID (always None for reminders — global events).
    pub channel_id: Option<Uuid>,
}

/// Query due reminders: latest-per-address `kind:30300` rows where
/// `not_before <= now`, `deleted_at IS NULL`, `delivered_at IS NULL`.
///
/// Returns the latest head per `(pubkey, d_tag)` using canonical NIP-16
/// ordering (`created_at DESC, id ASC`).
pub async fn query_due_reminders(
    pool: &PgPool,
    now_secs: i64,
    batch_limit: i64,
) -> Result<Vec<DueReminder>> {
    let kind_i32 = KIND_EVENT_REMINDER as i32;
    let rows = sqlx::query(
        r#"
        SELECT DISTINCT ON (e.community_id, e.pubkey, e.d_tag)
            e.community_id, c.host, e.id, e.pubkey, e.created_at, e.kind, e.tags, e.content, e.sig, e.channel_id
        FROM events AS e
        JOIN communities AS c ON c.id = e.community_id
        WHERE e.kind = $1
          AND e.not_before IS NOT NULL
          AND e.not_before <= $2
          AND e.deleted_at IS NULL
          AND e.delivered_at IS NULL
          AND c.archived_at IS NULL
        ORDER BY e.community_id, e.pubkey, e.d_tag, e.created_at DESC, e.id ASC
        LIMIT $3
        "#,
    )
    .bind(kind_i32)
    .bind(now_secs)
    .bind(batch_limit)
    .fetch_all(pool)
    .await?;

    let results = rows
        .into_iter()
        .map(|row| DueReminder {
            community_id: CommunityId::from_uuid(row.get("community_id")),
            host: row.get("host"),
            id: row.get("id"),
            pubkey: row.get("pubkey"),
            created_at: row.get("created_at"),
            kind: row.get("kind"),
            tags: row.get("tags"),
            content: row.get("content"),
            sig: row.get("sig"),
            channel_id: row.get("channel_id"),
        })
        .collect();

    Ok(results)
}

/// Atomically claim a due reminder for delivery. Returns `Some(id)` if this
/// caller won the claim (set `delivered_at`), or `None` if another pod already
/// claimed it. Mirrors the reaper's `archived_at IS NULL` guard for cross-pod
/// idempotency.
pub async fn claim_due_reminder(
    pool: &PgPool,
    community_id: CommunityId,
    event_id: &[u8],
    event_created_at: DateTime<Utc>,
) -> Result<bool> {
    claim_due_reminder_with_stamp(
        pool,
        community_id,
        event_id,
        event_created_at,
        Utc::now().timestamp(),
    )
    .await
}

/// Atomically claim a due reminder using a caller-supplied delivery stamp.
///
/// The same stamp should be passed to [`release_due_reminder`] if the publish
/// side effect fails, so rollback can compare-and-clear only this pod's claim.
///
/// Scoped by `community_id`: `events` is keyed `(community_id, created_at, id)`,
/// and the same Nostr event id (hence the same `id`/`created_at` pair) is
/// allowed across communities. Without the community predicate a claim for
/// `A/X` would also mark `B/X` delivered. The caller already holds the owning
/// community on the `DueReminder` row.
pub async fn claim_due_reminder_with_stamp(
    pool: &PgPool,
    community_id: CommunityId,
    event_id: &[u8],
    event_created_at: DateTime<Utc>,
    delivery_stamp: i64,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE events
        SET delivered_at = $1
        WHERE community_id = $2 AND created_at = $3 AND id = $4 AND delivered_at IS NULL
        "#,
    )
    .bind(delivery_stamp)
    .bind(community_id.as_uuid())
    .bind(event_created_at)
    .bind(event_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Release a previously claimed reminder when publish fails.
///
/// The `delivery_stamp` must be the exact value written by the claiming pod;
/// that compare-and-clear prevents one pod from rolling back another pod's
/// later claim after a retry/race.
///
/// Scoped by `community_id` for the same reason as the claim: a release for
/// `A/X` must not clear `B/X` even when their `id`/`created_at`/stamp coincide.
pub async fn release_due_reminder(
    pool: &PgPool,
    community_id: CommunityId,
    event_id: &[u8],
    event_created_at: DateTime<Utc>,
    delivery_stamp: i64,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE events
        SET delivered_at = NULL
        WHERE community_id = $1
          AND created_at = $2
          AND id = $3
          AND delivered_at = $4
        "#,
    )
    .bind(community_id.as_uuid())
    .bind(event_created_at)
    .bind(event_id)
    .bind(delivery_stamp)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1

    async fn setup_pool() -> PgPool {
        let database_url = std::env::var("BUZZ_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| TEST_DB_URL.to_owned());

        PgPool::connect(&database_url)
            .await
            .expect("connect to test DB")
    }

    async fn make_test_community(pool: &PgPool) -> Uuid {
        let id = Uuid::new_v4();
        let host = format!("event-test-{}.example", id.simple());
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(id)
            .bind(host)
            .execute(pool)
            .await
            .expect("insert test community");
        id
    }

    async fn make_test_channel(
        pool: &PgPool,
        community_id: Uuid,
        ttl_seconds: Option<i32>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO channels \
             (id, community_id, name, created_by, ttl_seconds, ttl_deadline) \
             VALUES ($1, $2, $3, $4, $5, \
                     CASE WHEN $5 IS NULL THEN NULL \
                          ELSE clock_timestamp() + make_interval(secs => $5) END)",
        )
        .bind(id)
        .bind(community_id)
        .bind(format!("event-ttl-test-{}", id.simple()))
        .bind(vec![7_u8; 32])
        .bind(ttl_seconds)
        .execute(pool)
        .await
        .expect("insert test channel");
        id
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn event_insert_ttl_trigger_handles_permanent_ephemeral_duplicate_and_activation_race() {
        let pool = setup_pool().await;
        let community_uuid = make_test_community(&pool).await;
        let community = CommunityId::from_uuid(community_uuid);

        let permanent = make_test_channel(&pool, community_uuid, None).await;
        let permanent_event = make_text_event("permanent channel event");
        assert!(
            insert_event(&pool, community, &permanent_event, Some(permanent))
                .await
                .expect("insert permanent event")
                .1
        );
        let permanent_deadline: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT ttl_deadline FROM channels WHERE community_id = $1 AND id = $2",
        )
        .bind(community_uuid)
        .bind(permanent)
        .fetch_one(&pool)
        .await
        .expect("read permanent deadline");
        assert_eq!(permanent_deadline, None);

        let ephemeral = make_test_channel(&pool, community_uuid, Some(60)).await;
        let initial_deadline: DateTime<Utc> = sqlx::query_scalar(
            "SELECT ttl_deadline FROM channels WHERE community_id = $1 AND id = $2",
        )
        .bind(community_uuid)
        .bind(ephemeral)
        .fetch_one(&pool)
        .await
        .expect("read initial ephemeral deadline");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let ephemeral_event = make_text_event("ephemeral channel event");
        assert!(
            insert_event(&pool, community, &ephemeral_event, Some(ephemeral))
                .await
                .expect("insert ephemeral event")
                .1
        );
        let bumped_deadline: DateTime<Utc> = sqlx::query_scalar(
            "SELECT ttl_deadline FROM channels WHERE community_id = $1 AND id = $2",
        )
        .bind(community_uuid)
        .bind(ephemeral)
        .fetch_one(&pool)
        .await
        .expect("read bumped ephemeral deadline");
        assert!(bumped_deadline > initial_deadline);
        assert!(
            !insert_event(&pool, community, &ephemeral_event, Some(ephemeral))
                .await
                .expect("insert duplicate event")
                .1
        );
        let duplicate_deadline: DateTime<Utc> = sqlx::query_scalar(
            "SELECT ttl_deadline FROM channels WHERE community_id = $1 AND id = $2",
        )
        .bind(community_uuid)
        .bind(ephemeral)
        .fetch_one(&pool)
        .await
        .expect("read deadline after duplicate");
        assert_eq!(duplicate_deadline, bumped_deadline);

        // Reproduce the blocked stale-prefetch ordering: ingest has already
        // observed a permanent channel, then TTL activation locks/updates the
        // row before the event INSERT reaches its trigger. The trigger must
        // wait and refresh from the later event after activation commits.
        let racing = make_test_channel(&pool, community_uuid, None).await;
        let stale_ttl: Option<i32> = sqlx::query_scalar(
            "SELECT ttl_seconds FROM channels WHERE community_id = $1 AND id = $2",
        )
        .bind(community_uuid)
        .bind(racing)
        .fetch_one(&pool)
        .await
        .expect("prefetch permanent channel");
        assert_eq!(stale_ttl, None);

        let mut activation = pool.begin().await.expect("begin TTL activation");
        // Model the repaired update_channel protocol (migration 0024): the
        // TTL transition holds the per-channel advisory key EXCLUSIVE, which
        // is what the event trigger's shared acquisition now waits on.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("buzz_channel_ttl:{community_uuid}:{racing}"))
            .execute(&mut *activation)
            .await
            .expect("acquire exclusive channel TTL key");
        let activation_deadline: DateTime<Utc> = sqlx::query_scalar(
            "UPDATE channels \
             SET ttl_seconds = 60, ttl_deadline = clock_timestamp() + interval '60 seconds' \
             WHERE community_id = $1 AND id = $2 RETURNING ttl_deadline",
        )
        .bind(community_uuid)
        .bind(racing)
        .fetch_one(&mut *activation)
        .await
        .expect("activate TTL while holding channel row lock");

        let race_pool = pool.clone();
        let racing_event = make_text_event("event after stale permanent prefetch");
        let insert = tokio::spawn(async move {
            insert_event(&race_pool, community, &racing_event, Some(racing)).await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            !insert.is_finished(),
            "event trigger must wait on TTL activation"
        );
        activation.commit().await.expect("commit TTL activation");
        assert!(
            insert
                .await
                .expect("join racing insert")
                .expect("racing insert")
                .1
        );

        let final_deadline: DateTime<Utc> = sqlx::query_scalar(
            "SELECT ttl_deadline FROM channels WHERE community_id = $1 AND id = $2",
        )
        .bind(community_uuid)
        .bind(racing)
        .fetch_one(&pool)
        .await
        .expect("read deadline after racing event");
        assert!(
            final_deadline > activation_deadline + chrono::Duration::milliseconds(50),
            "later event must extend TTL beyond activation deadline: activation={activation_deadline}, final={final_deadline}"
        );
    }

    /// T1a repair regression test (migration 0024): permanent-channel event
    /// commits must not serialize on the channel row. The 0022 trigger took
    /// `FOR UPDATE` on the channel tuple before testing `ttl_seconds`, so
    /// concurrent commits into one hot permanent channel queued at commit
    /// time (deferred trigger) — invisible to any single-connection test.
    /// This holds N insert transactions at a barrier past their INSERTs,
    /// then proves (a) while all N sit pre-commit, no transaction holds a
    /// row-level lock on the channel tuple, and (b) all N commits succeed
    /// with the channel row untouched (permanent ⇒ no deadline write).
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn permanent_channel_event_commits_do_not_lock_the_channel_row() {
        const N: usize = 8;
        // setup_pool's default cap (10) covers N held transactions plus the
        // pg_locks inspector connection.
        let pool = setup_pool().await;
        let community_uuid = make_test_community(&pool).await;
        let channel = make_test_channel(&pool, community_uuid, None).await;

        // Open N transactions, run the full event INSERT in each (the deferred
        // trigger fires at COMMIT), and park them at a barrier.
        let mut txs = Vec::new();
        for i in 0..N {
            let mut tx = pool.begin().await.expect("begin insert txn");
            let event = make_text_event(&format!("hot channel event {i}"));
            sqlx::query(
                "INSERT INTO events (community_id,id,pubkey,created_at,kind,tags,content,sig,received_at,channel_id) \
                 VALUES ($1,$2,$3,$4,9,$5,$6,$7,now(),$8)",
            )
            .bind(community_uuid)
            .bind(event.id.as_bytes().as_slice())
            .bind(event.pubkey.as_bytes().as_slice())
            .bind(DateTime::from_timestamp(event.created_at.as_secs() as i64, 0).unwrap())
            .bind(serde_json::to_value(&event.tags).unwrap())
            .bind(&event.content)
            .bind(event.sig.serialize().as_slice())
            .bind(channel)
            .execute(&mut *tx)
            .await
            .expect("insert event inside held txn");
            txs.push(tx);
        }

        // With all N transactions holding completed INSERTs, none may hold a
        // row-level lock on the channels tuple. (The 0022 trigger would not
        // have taken it yet either — it locks at COMMIT — so also verify the
        // commit phase below completes without mutual blocking.)
        let tuple_locks: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks l \
             JOIN pg_class c ON c.oid = l.relation \
             WHERE c.relname = 'channels' AND l.locktype = 'tuple'",
        )
        .fetch_one(&pool)
        .await
        .expect("inspect pg_locks");
        assert_eq!(tuple_locks, 0, "no channel tuple locks while txns are held");

        // Release all commits concurrently. Under 0022 these serialized on the
        // channel row (each holding it across its WAL flush); under 0024 the
        // shared advisory key admits them all. Join with a timeout so a
        // regression fails fast instead of hanging the suite.
        let commits = txs
            .into_iter()
            .map(|tx| tokio::spawn(async move { tx.commit().await }))
            .collect::<Vec<_>>();
        for c in commits {
            tokio::time::timeout(std::time::Duration::from_secs(10), c)
                .await
                .expect("concurrent permanent-channel commits must not block")
                .expect("join commit task")
                .expect("commit succeeds");
        }

        let deadline: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT ttl_deadline FROM channels WHERE community_id = $1 AND id = $2",
        )
        .bind(community_uuid)
        .bind(channel)
        .fetch_one(&pool)
        .await
        .expect("read deadline after commits");
        assert_eq!(deadline, None, "permanent channel must remain untouched");
        let stored: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM events WHERE community_id = $1 AND channel_id = $2",
        )
        .bind(community_uuid)
        .bind(channel)
        .fetch_one(&pool)
        .await
        .expect("count stored events");
        assert_eq!(stored as usize, N);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn get_event_by_id_is_scoped_when_event_id_collides_across_communities() {
        let pool = setup_pool().await;
        let community_a = CommunityId::from_uuid(make_test_community(&pool).await);
        let community_b = CommunityId::from_uuid(make_test_community(&pool).await);
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(9), "same signed event")
            .sign_with_keys(&keys)
            .expect("sign event");

        insert_event(&pool, community_a, &event, None)
            .await
            .expect("insert in community A");
        insert_event(&pool, community_b, &event, None)
            .await
            .expect("insert same event in community B");

        sqlx::query("UPDATE events SET content = $1 WHERE community_id = $2 AND id = $3")
            .bind("community-a-copy")
            .bind(community_a.as_uuid())
            .bind(event.id.as_bytes())
            .execute(&pool)
            .await
            .expect("mark community A row");
        sqlx::query("UPDATE events SET content = $1 WHERE community_id = $2 AND id = $3")
            .bind("community-b-copy")
            .bind(community_b.as_uuid())
            .bind(event.id.as_bytes())
            .execute(&pool)
            .await
            .expect("mark community B row");

        let a = get_event_by_id(&pool, community_a, event.id.as_bytes())
            .await
            .expect("lookup community A")
            .expect("community A row exists");
        let b = get_event_by_id(&pool, community_b, event.id.as_bytes())
            .await
            .expect("lookup community B")
            .expect("community B row exists");

        assert_eq!(a.event.content, "community-a-copy");
        assert_eq!(b.event.content, "community-b-copy");
    }

    fn make_event_with_kind_and_tags(kind: u16, tags: Vec<Tag>) -> nostr::Event {
        let keys = Keys::generate();
        EventBuilder::new(Kind::Custom(kind), "test")
            .tags(tags)
            .sign_with_keys(&keys)
            .expect("sign")
    }

    fn make_event_at(kind: u16, content: &str, created_at: u64) -> nostr::Event {
        EventBuilder::new(Kind::Custom(kind), content)
            .custom_created_at(nostr::Timestamp::from(created_at))
            .sign_with_keys(&Keys::generate())
            .expect("sign timestamped event")
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn explicit_multi_channel_scope_is_applied_before_historical_page_limit() {
        let pool = setup_pool().await;
        let community_uuid = make_test_community(&pool).await;
        let community = CommunityId::from_uuid(community_uuid);
        let channel_a = make_test_channel(&pool, community_uuid, None).await;
        let channel_b = make_test_channel(&pool, community_uuid, None).await;
        let unrelated_c = make_test_channel(&pool, community_uuid, None).await;
        let base = 1_800_000_000;

        let older_a = make_event_at(39_000, "older requested A", base + 1);
        insert_event(&pool, community, &older_a, Some(channel_a))
            .await
            .expect("insert requested A candidate");
        let requested_b = make_event_at(39_000, "requested B", base + 2);
        insert_event(&pool, community, &requested_b, Some(channel_b))
            .await
            .expect("insert requested B candidate");
        let newer_c = make_event_at(39_000, "newer unrelated C", base + 3);
        insert_event(&pool, community, &newer_c, Some(unrelated_c))
            .await
            .expect("insert unrelated C candidate");
        let global = make_event_at(39_000, "global candidate", base + 4);
        insert_event(&pool, community, &global, None)
            .await
            .expect("insert global candidate");

        let events = query_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![39_000]),
                channel_ids: Some(vec![channel_a, channel_b]),
                channel_ids_include_global: false,
                limit: Some(1),
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("query explicit multi-channel page");

        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].event.id, requested_b.id,
            "newer unrelated channel C must not consume the requested A/B limit"
        );

        let partial_authorization_count = count_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![39_000]),
                channel_ids: Some(vec![channel_a]),
                channel_ids_include_global: false,
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("count one authorized channel from a multi-channel request");
        assert_eq!(
            partial_authorization_count, 1,
            "partial authorization must exclude requested B, unrelated C, and global rows"
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn access_scope_is_applied_before_historical_page_limit() {
        let pool = setup_pool().await;
        let community_uuid = make_test_community(&pool).await;
        let community = CommunityId::from_uuid(community_uuid);
        let accessible = make_test_channel(&pool, community_uuid, None).await;
        let inaccessible = make_test_channel(&pool, community_uuid, None).await;
        let base = 1_800_000_000;

        // This is the bridge underfetch shape: newer inaccessible candidates
        // outnumber the requested page, while the visible match is older.
        for offset in 10..13 {
            let event = make_event_at(39_000, "newer inaccessible", base + offset);
            insert_event(&pool, community, &event, Some(inaccessible))
                .await
                .expect("insert inaccessible candidate");
        }
        let global = make_event_at(39_000, "newer global", base + 2);
        insert_event(&pool, community, &global, None)
            .await
            .expect("insert global candidate");
        let older_accessible = make_event_at(39_000, "older accessible", base + 1);
        insert_event(&pool, community, &older_accessible, Some(accessible))
            .await
            .expect("insert accessible candidate");

        let events = query_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![39_000]),
                channel_ids: Some(vec![accessible]),
                limit: Some(2),
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("query access-scoped page");

        assert_eq!(events.len(), 2, "visible page must be filled before EOF");
        assert_eq!(events[0].event.id, global.id, "global rows remain visible");
        assert_eq!(
            events[1].event.id, older_accessible.id,
            "older accessible row must not be hidden behind newer inaccessible rows"
        );
    }

    fn make_text_event(content: &str) -> nostr::Event {
        let keys = Keys::generate();
        EventBuilder::new(Kind::Custom(9), content)
            .sign_with_keys(&keys)
            .expect("sign text event")
    }

    fn make_reaction_event(keys: &Keys, target_id_hex: &str, emoji: &str) -> nostr::Event {
        let nonce = Uuid::new_v4().to_string();
        EventBuilder::new(Kind::Custom(7), emoji)
            .tags(vec![
                Tag::parse(["e", target_id_hex]).expect("reaction e tag"),
                Tag::parse(["nonce", nonce.as_str()]).expect("nonce tag"),
            ])
            .sign_with_keys(keys)
            .expect("sign reaction event")
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn reaction_single_tx_stores_wrapped_max_shortcode() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let target = make_text_event("long custom emoji target");
        insert_event(&pool, community, &target, None)
            .await
            .expect("insert target");

        let actor = Keys::generate();
        let emoji = format!(":{}:", "a".repeat(64));
        let reaction = make_reaction_event(&actor, &target.id.to_hex(), &emoji);
        let outcome = insert_reaction_event_with_thread_metadata(
            &pool,
            community,
            &reaction,
            None,
            None,
            target.id.as_bytes(),
            &actor.public_key().to_bytes(),
            &emoji,
        )
        .await
        .expect("store wrapped 64-character shortcode");

        assert!(matches!(
            outcome,
            ReactionEventInsertOutcome::Inserted {
                was_inserted: true,
                ..
            }
        ));
        assert_eq!(emoji.chars().count(), 66);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn reaction_single_tx_duplicate_short_circuit_stores_no_event() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let target = make_text_event("reaction target");
        insert_event(&pool, community, &target, None)
            .await
            .expect("insert target");

        let actor = Keys::generate();
        let actor_pubkey = actor.public_key().to_bytes();
        let target_hex = target.id.to_hex();
        let first = make_reaction_event(&actor, &target_hex, "👍");
        let second = make_reaction_event(&actor, &target_hex, "👍");

        let first_outcome = insert_reaction_event_with_thread_metadata(
            &pool,
            community,
            &first,
            None,
            None,
            target.id.as_bytes(),
            &actor_pubkey,
            "👍",
        )
        .await
        .expect("first reaction insert");
        assert!(matches!(
            first_outcome,
            ReactionEventInsertOutcome::Inserted {
                was_inserted: true,
                ..
            }
        ));

        let duplicate = insert_reaction_event_with_thread_metadata(
            &pool,
            community,
            &second,
            None,
            None,
            target.id.as_bytes(),
            &actor_pubkey,
            "👍",
        )
        .await
        .expect("duplicate reaction insert");
        assert!(matches!(duplicate, ReactionEventInsertOutcome::Duplicate));

        let duplicate_event = get_event_by_id(&pool, community, second.id.as_bytes())
            .await
            .expect("lookup duplicate reaction event");
        assert!(
            duplicate_event.is_none(),
            "active duplicate reaction must short-circuit before storing kind:7 event"
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn reaction_single_tx_cross_community_target_rejected() {
        let pool = setup_pool().await;
        let community_a = CommunityId::from_uuid(make_test_community(&pool).await);
        let community_b = CommunityId::from_uuid(make_test_community(&pool).await);
        let target = make_text_event("community A target only");
        insert_event(&pool, community_a, &target, None)
            .await
            .expect("insert target in A");

        let actor = Keys::generate();
        let actor_pubkey = actor.public_key().to_bytes();
        let reaction = make_reaction_event(&actor, &target.id.to_hex(), "👍");

        let outcome = insert_reaction_event_with_thread_metadata(
            &pool,
            community_b,
            &reaction,
            None,
            None,
            target.id.as_bytes(),
            &actor_pubkey,
            "👍",
        )
        .await
        .expect("cross-community reaction attempt");
        assert!(matches!(outcome, ReactionEventInsertOutcome::TargetMissing));

        assert!(
            get_event_by_id(&pool, community_b, reaction.id.as_bytes())
                .await
                .expect("lookup B reaction event")
                .is_none(),
            "reaction event must not store when target exists only in another community"
        );
        assert!(
            crate::reaction::get_active_reaction_record(
                &pool,
                community_b,
                target.id.as_bytes(),
                DateTime::from_timestamp(target.created_at.as_secs() as i64, 0).unwrap(),
                &actor_pubkey,
                "👍",
            )
            .await
            .expect("lookup B reaction row")
            .is_none(),
            "reaction row must not be inserted for cross-community target miss"
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn reaction_single_tx_event_insert_failure_rolls_back_reaction() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let target = make_text_event("rollback target");
        insert_event(&pool, community, &target, None)
            .await
            .expect("insert target");

        let actor = Keys::generate();
        let actor_pubkey = actor.public_key().to_bytes();
        let target_hex = target.id.to_hex();
        let bad_reaction = EventBuilder::new(Kind::Custom(20000), "👍")
            .tags(vec![
                Tag::parse(["e", target_hex.as_str()]).expect("reaction e tag")
            ])
            .sign_with_keys(&actor)
            .expect("sign ephemeral reaction-shaped event");
        let target_created_at = DateTime::from_timestamp(target.created_at.as_secs() as i64, 0)
            .expect("target timestamp");

        let err = insert_reaction_event_with_thread_metadata(
            &pool,
            community,
            &bad_reaction,
            None,
            None,
            target.id.as_bytes(),
            &actor_pubkey,
            "👍",
        )
        .await
        .expect_err("ephemeral event insert must fail after reaction upsert attempt");
        assert!(matches!(err, DbError::EphemeralEventRejected(20000)));

        assert!(
            crate::reaction::get_active_reaction_record(
                &pool,
                community,
                target.id.as_bytes(),
                target_created_at,
                &actor_pubkey,
                "👍",
            )
            .await
            .expect("lookup reaction row after rollback")
            .is_none(),
            "transaction rollback must remove the reaction row when event insert fails"
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn reaction_single_tx_reactivates_soft_deleted_reaction() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let target = make_text_event("reactivation target");
        insert_event(&pool, community, &target, None)
            .await
            .expect("insert target");

        let actor = Keys::generate();
        let actor_pubkey = actor.public_key().to_bytes();
        let target_hex = target.id.to_hex();
        let target_created_at = DateTime::from_timestamp(target.created_at.as_secs() as i64, 0)
            .expect("target timestamp");
        let first = make_reaction_event(&actor, &target_hex, "👍");
        let second = make_reaction_event(&actor, &target_hex, "👍");

        assert!(matches!(
            insert_reaction_event_with_thread_metadata(
                &pool,
                community,
                &first,
                None,
                None,
                target.id.as_bytes(),
                &actor_pubkey,
                "👍",
            )
            .await
            .expect("first reaction insert"),
            ReactionEventInsertOutcome::Inserted { .. }
        ));
        assert!(crate::reaction::remove_reaction(
            &pool,
            community,
            target.id.as_bytes(),
            target_created_at,
            &actor_pubkey,
            "👍",
        )
        .await
        .expect("soft delete reaction"));

        let outcome = insert_reaction_event_with_thread_metadata(
            &pool,
            community,
            &second,
            None,
            None,
            target.id.as_bytes(),
            &actor_pubkey,
            "👍",
        )
        .await
        .expect("reactivate reaction");
        assert!(matches!(
            outcome,
            ReactionEventInsertOutcome::Inserted {
                was_inserted: true,
                ..
            }
        ));

        let active = crate::reaction::get_active_reaction_record(
            &pool,
            community,
            target.id.as_bytes(),
            target_created_at,
            &actor_pubkey,
            "👍",
        )
        .await
        .expect("active record after reactivation")
        .expect("reaction active after reactivation");
        assert_eq!(
            active.reaction_event_id.as_deref(),
            Some(second.id.as_bytes().as_slice()),
            "reactivation through the tx path must preserve add_reaction's source-id update semantics"
        );
    }

    #[test]
    fn extract_d_tag_from_nip33_event() {
        let event = make_event_with_kind_and_tags(
            30023,
            vec![Tag::parse(["d", "my-article-slug"]).unwrap()],
        );
        assert_eq!(extract_d_tag(&event), Some("my-article-slug".to_string()));
    }

    #[test]
    fn extract_d_tag_first_d_wins() {
        let event = make_event_with_kind_and_tags(
            30023,
            vec![
                Tag::parse(["d", "first"]).unwrap(),
                Tag::parse(["d", "second"]).unwrap(),
            ],
        );
        assert_eq!(extract_d_tag(&event), Some("first".to_string()));
    }

    #[test]
    fn extract_d_tag_missing_becomes_empty_string() {
        // NIP-33: "if there is no d tag, the d tag is considered to be ''"
        let event =
            make_event_with_kind_and_tags(30023, vec![Tag::parse(["p", "abc123"]).unwrap()]);
        assert_eq!(extract_d_tag(&event), Some(String::new()));
    }

    #[test]
    fn extract_d_tag_empty_value_preserved() {
        let event = make_event_with_kind_and_tags(30023, vec![Tag::parse(["d", ""]).unwrap()]);
        assert_eq!(extract_d_tag(&event), Some(String::new()));
    }

    #[test]
    fn extract_d_tag_non_nip33_returns_none() {
        // kind:1 (text note) — not parameterized replaceable
        let event =
            make_event_with_kind_and_tags(1, vec![Tag::parse(["d", "should-be-ignored"]).unwrap()]);
        assert_eq!(extract_d_tag(&event), None);
    }

    #[test]
    fn extract_d_tag_nip29_group_metadata() {
        // kind:39000 is in the 30000–39999 range — d_tag should be extracted
        let event =
            make_event_with_kind_and_tags(39000, vec![Tag::parse(["d", "group-id"]).unwrap()]);
        assert_eq!(extract_d_tag(&event), Some("group-id".to_string()));
    }

    #[test]
    fn extract_d_tag_boundary_kinds() {
        // kind:29999 — just below range
        let below = make_event_with_kind_and_tags(29999, vec![Tag::parse(["d", "val"]).unwrap()]);
        assert_eq!(extract_d_tag(&below), None);

        // kind:30000 — lower bound
        let lower = make_event_with_kind_and_tags(30000, vec![Tag::parse(["d", "val"]).unwrap()]);
        assert_eq!(extract_d_tag(&lower), Some("val".to_string()));

        // kind:39999 — upper bound
        let upper = make_event_with_kind_and_tags(39999, vec![Tag::parse(["d", "val"]).unwrap()]);
        assert_eq!(extract_d_tag(&upper), Some("val".to_string()));

        // kind:40000 — just above range
        let above = make_event_with_kind_and_tags(40000, vec![Tag::parse(["d", "val"]).unwrap()]);
        assert_eq!(extract_d_tag(&above), None);
    }

    #[test]
    fn extract_d_tag_single_element_d_tag_ignored() {
        // A d tag with only one element (no value) should not match — parts.len() < 2
        let event = make_event_with_kind_and_tags(30023, vec![Tag::parse(["d"]).unwrap()]);
        // No d tag with a value → empty string per NIP-33
        assert_eq!(extract_d_tag(&event), Some(String::new()));
    }

    #[test]
    fn extract_d_tag_preserves_full_value() {
        // extract_d_tag returns the full value — length enforcement is at the ingest layer.
        let long_val = "x".repeat(2048);
        let event =
            make_event_with_kind_and_tags(30023, vec![Tag::parse(["d", &long_val]).unwrap()]);
        let result = extract_d_tag(&event).unwrap();
        assert_eq!(result.len(), 2048);
        assert_eq!(result, long_val);
    }

    #[test]
    fn extract_not_before_from_reminder() {
        let event = make_event_with_kind_and_tags(
            KIND_EVENT_REMINDER as u16,
            vec![Tag::parse(["not_before", "1717000000"]).unwrap()],
        );
        assert_eq!(extract_not_before(&event), Some(1_717_000_000));
    }

    #[test]
    fn extract_not_before_absent_returns_none() {
        // A bookmark/terminal reminder carries no `not_before` tag.
        let event = make_event_with_kind_and_tags(
            KIND_EVENT_REMINDER as u16,
            vec![Tag::parse(["d", "abc"]).unwrap()],
        );
        assert_eq!(extract_not_before(&event), None);
    }

    #[test]
    fn extract_not_before_non_reminder_returns_none() {
        // Only kind:30300 materializes `not_before`; other kinds stay NULL.
        let event = make_event_with_kind_and_tags(
            30023,
            vec![Tag::parse(["not_before", "1717000000"]).unwrap()],
        );
        assert_eq!(extract_not_before(&event), None);
    }

    #[test]
    fn extract_not_before_non_numeric_returns_none() {
        // Malformed values are rejected by ingest; materialization just skips them.
        let event = make_event_with_kind_and_tags(
            KIND_EVENT_REMINDER as u16,
            vec![Tag::parse(["not_before", "not-a-number"]).unwrap()],
        );
        assert_eq!(extract_not_before(&event), None);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn query_due_reminders_returns_row_community_and_host_per_tenant() {
        let pool = setup_pool().await;
        let community_a_uuid = make_test_community(&pool).await;
        let community_b_uuid = make_test_community(&pool).await;
        let community_a = CommunityId::from_uuid(community_a_uuid);
        let community_b = CommunityId::from_uuid(community_b_uuid);
        let host_a: String = sqlx::query_scalar("SELECT host FROM communities WHERE id = $1")
            .bind(community_a_uuid)
            .fetch_one(&pool)
            .await
            .expect("load host A");
        let host_b: String = sqlx::query_scalar("SELECT host FROM communities WHERE id = $1")
            .bind(community_b_uuid)
            .fetch_one(&pool)
            .await
            .expect("load host B");

        let not_before = Utc::now().timestamp() - 1;
        let keys_a = Keys::generate();
        let keys_b = Keys::generate();
        let event_a = EventBuilder::new(Kind::Custom(KIND_EVENT_REMINDER as u16), "a")
            .tags([
                Tag::parse(["d", "due-reminder-scope-a"]).unwrap(),
                Tag::parse(["not_before", &not_before.to_string()]).unwrap(),
            ])
            .sign_with_keys(&keys_a)
            .expect("sign A");
        let event_b = EventBuilder::new(Kind::Custom(KIND_EVENT_REMINDER as u16), "b")
            .tags([
                Tag::parse(["d", "due-reminder-scope-b"]).unwrap(),
                Tag::parse(["not_before", &not_before.to_string()]).unwrap(),
            ])
            .sign_with_keys(&keys_b)
            .expect("sign B");

        insert_event(&pool, community_a, &event_a, None)
            .await
            .expect("insert A");
        insert_event(&pool, community_b, &event_b, None)
            .await
            .expect("insert B");

        let due = query_due_reminders(&pool, Utc::now().timestamp(), 100)
            .await
            .expect("query due reminders");

        assert!(due.iter().any(|row| {
            row.id == event_a.id.as_bytes() && row.community_id == community_a && row.host == host_a
        }));
        assert!(due.iter().any(|row| {
            row.id == event_b.id.as_bytes() && row.community_id == community_b && row.host == host_b
        }));
    }

    /// Two pods race to claim the same due reminder: exactly one wins. The
    /// scheduler publishes only on a winning claim (`Ok(true)`) and `continue`s
    /// on the loser (`Ok(false)`), so a single winning claim *is* the proof of
    /// exactly one publish side effect across N pods.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn claim_due_reminder_is_won_by_exactly_one_of_two_racing_pods() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let not_before = Utc::now().timestamp() - 1;
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(KIND_EVENT_REMINDER as u16), "due")
            .tags([
                Tag::parse(["d", "due-reminder-claim-race"]).unwrap(),
                Tag::parse(["not_before", &not_before.to_string()]).unwrap(),
            ])
            .sign_with_keys(&keys)
            .expect("sign reminder");
        insert_event(&pool, community, &event, None)
            .await
            .expect("insert reminder");

        let id = event.id.as_bytes().to_vec();
        let created_at = event.created_at.as_secs() as i64;
        let created_at = chrono::DateTime::from_timestamp(created_at, 0).expect("created_at");

        // Two pods, two distinct per-attempt stamps, same reminder.
        let stamp_p1: i64 = 0x1111_1111_1111_1111;
        let stamp_p2: i64 = 0x2222_2222_2222_2222;
        let won_p1 = claim_due_reminder_with_stamp(&pool, community, &id, created_at, stamp_p1)
            .await
            .expect("p1 claim");
        let won_p2 = claim_due_reminder_with_stamp(&pool, community, &id, created_at, stamp_p2)
            .await
            .expect("p2 claim");

        assert!(
            won_p1 ^ won_p2,
            "exactly one pod must win the claim (p1={won_p1}, p2={won_p2}) — \
             the loser never reaches the publish side effect"
        );
    }

    /// A failed publish releases the claim so the reminder is redeliverable,
    /// and the compare-and-clear stamp guard prevents one pod from rolling back
    /// another pod's claim.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn release_due_reminder_rolls_back_only_the_matching_stamp() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let not_before = Utc::now().timestamp() - 1;
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(KIND_EVENT_REMINDER as u16), "due")
            .tags([
                Tag::parse(["d", "due-reminder-release"]).unwrap(),
                Tag::parse(["not_before", &not_before.to_string()]).unwrap(),
            ])
            .sign_with_keys(&keys)
            .expect("sign reminder");
        insert_event(&pool, community, &event, None)
            .await
            .expect("insert reminder");

        let id = event.id.as_bytes().to_vec();
        let created_at = event.created_at.as_secs() as i64;
        let created_at = chrono::DateTime::from_timestamp(created_at, 0).expect("created_at");
        let stamp: i64 = 0x3333_3333_3333_3333;

        assert!(
            claim_due_reminder_with_stamp(&pool, community, &id, created_at, stamp)
                .await
                .expect("claim"),
            "first claim wins"
        );

        // A release with the *wrong* stamp must be a no-op (does not clear
        // another pod's claim).
        assert!(
            !release_due_reminder(&pool, community, &id, created_at, stamp ^ 0xFFFF)
                .await
                .expect("wrong-stamp release"),
            "release with a non-matching stamp must not clear the claim"
        );
        assert!(
            !claim_due_reminder_with_stamp(&pool, community, &id, created_at, stamp)
                .await
                .expect("re-claim after no-op release"),
            "reminder must still be claimed after a no-op release"
        );

        // The matching-stamp release rolls the claim back; the reminder is
        // redeliverable and a subsequent claim wins again.
        assert!(
            release_due_reminder(&pool, community, &id, created_at, stamp)
                .await
                .expect("matching-stamp release"),
            "release with the claiming stamp must clear the claim"
        );
        assert!(
            claim_due_reminder_with_stamp(&pool, community, &id, created_at, stamp)
                .await
                .expect("re-claim after release"),
            "released reminder must be reclaimable for retry"
        );
    }

    /// Cross-community confinement: the same Nostr reminder event (identical
    /// `id` and `created_at`) inserted into communities A and B must claim and
    /// release independently. A claim/release for `A/X` must never touch `B/X`.
    ///
    /// This is the primitive the scheduler's exactly-once-publish proof rests
    /// on: `events` is keyed `(community_id, created_at, id)`, so without the
    /// community predicate a claim for A would mark B delivered (suppressing
    /// B's reminder) and a matching-stamp release for A would clear B.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn reminder_claim_and_release_are_confined_to_their_community() {
        let pool = setup_pool().await;
        let community_a = CommunityId::from_uuid(make_test_community(&pool).await);
        let community_b = CommunityId::from_uuid(make_test_community(&pool).await);

        // One signed event, inserted into both communities — same id/created_at.
        let not_before = Utc::now().timestamp() - 1;
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(KIND_EVENT_REMINDER as u16), "due")
            .tags([
                Tag::parse(["d", "due-reminder-cross-community"]).unwrap(),
                Tag::parse(["not_before", &not_before.to_string()]).unwrap(),
            ])
            .sign_with_keys(&keys)
            .expect("sign reminder");
        insert_event(&pool, community_a, &event, None)
            .await
            .expect("insert A/X");
        insert_event(&pool, community_b, &event, None)
            .await
            .expect("insert B/X");

        let id = event.id.as_bytes().to_vec();
        let created_at = event.created_at.as_secs() as i64;
        let created_at = chrono::DateTime::from_timestamp(created_at, 0).expect("created_at");
        let stamp: i64 = 0x4444_4444_4444_4444;

        // Claim A/X. B/X must remain claimable — A's claim did not mark B.
        assert!(
            claim_due_reminder_with_stamp(&pool, community_a, &id, created_at, stamp)
                .await
                .expect("claim A"),
            "A/X claim wins"
        );
        assert!(
            claim_due_reminder_with_stamp(&pool, community_b, &id, created_at, stamp)
                .await
                .expect("claim B"),
            "B/X must still be claimable after A/X is claimed — \
             a claim for A must not mark B delivered"
        );

        // Both are now claimed under the same stamp. A matching-stamp release
        // for A/X must clear only A/X; B/X must stay claimed.
        assert!(
            release_due_reminder(&pool, community_a, &id, created_at, stamp)
                .await
                .expect("release A"),
            "A/X release with the claiming stamp clears A/X"
        );
        assert!(
            !claim_due_reminder_with_stamp(&pool, community_b, &id, created_at, stamp)
                .await
                .expect("re-claim B after A release"),
            "B/X must remain claimed after A/X is released — \
             a release for A must not clear B"
        );
        // And A/X is genuinely redeliverable (the release was real, not a no-op).
        assert!(
            claim_due_reminder_with_stamp(&pool, community_a, &id, created_at, stamp)
                .await
                .expect("re-claim A after release"),
            "A/X must be reclaimable after its own release"
        );
    }

    #[test]
    fn huddle_started_content_requires_matching_ephemeral_field() {
        let channel_id = Uuid::new_v4();
        let matching = serde_json::json!({
            "ephemeral_channel_id": channel_id.to_string(),
        })
        .to_string();
        assert!(huddle_started_content_links(&matching, channel_id));

        let wrong_field = serde_json::json!({
            "other": channel_id.to_string(),
        })
        .to_string();
        assert!(!huddle_started_content_links(&wrong_field, channel_id));
        assert!(!huddle_started_content_links("not-json", channel_id));
    }

    // ---- Coding-session genesis uniqueness (kind 44226) --------------------

    /// Build a genesis event in the exact envelope the relay's validator
    /// admits, signed by `keys`. Rival claims differ only in their signer.
    fn make_genesis_event(keys: &Keys, channel_id: Uuid, session_ref: &str) -> nostr::Event {
        let channel = channel_id.to_string();
        EventBuilder::new(
            Kind::Custom(44226),
            format!(r#"{{"sessionRef":"{session_ref}","v":1}}"#),
        )
        .tags(vec![
            Tag::parse(["h", channel.as_str()]).expect("h tag"),
            Tag::parse(["csg-v", "csg1-1"]).expect("csg-v tag"),
            Tag::parse(["csg-session", session_ref]).expect("csg-session tag"),
        ])
        .sign_with_keys(keys)
        .expect("sign genesis")
    }

    async fn count_live_genesis(pool: &PgPool, community: CommunityId, session_ref: &str) -> i64 {
        let probe = serde_json::json!([["csg-session", session_ref]]);
        sqlx::query_scalar(
            "SELECT count(*) FROM events \
             WHERE community_id = $1 AND kind = 44226 AND deleted_at IS NULL \
             AND tags @> $2::jsonb",
        )
        .bind(community.as_uuid())
        .bind(&probe)
        .fetch_one(pool)
        .await
        .expect("count genesis rows")
    }

    /// The lock must cover the uniqueness tuple and nothing else: every
    /// component moves the key (so unrelated sessions never contend), and the
    /// signer is structurally absent from the signature (so rival founders
    /// always do).
    #[test]
    fn genesis_lock_key_covers_exactly_the_uniqueness_scope() {
        let community = CommunityId::from_uuid(Uuid::new_v4());
        let other_community = CommunityId::from_uuid(Uuid::new_v4());
        let channel = Uuid::new_v4();
        let other_channel = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let other_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a11";

        let base = coding_session_genesis_lock_key(community, channel, session_ref);
        assert_eq!(
            base,
            coding_session_genesis_lock_key(community, channel, session_ref),
            "the key must be stable across processes and restarts"
        );
        for (label, other) in [
            (
                "community",
                coding_session_genesis_lock_key(other_community, channel, session_ref),
            ),
            (
                "channel",
                coding_session_genesis_lock_key(community, other_channel, session_ref),
            ),
            (
                "sessionRef",
                coding_session_genesis_lock_key(community, channel, other_ref),
            ),
        ] {
            assert_ne!(base, other, "{label} must move the lock key");
        }
    }

    #[test]
    fn genesis_session_ref_is_read_from_the_stored_tag() {
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let event = make_genesis_event(&Keys::generate(), Uuid::new_v4(), session_ref);
        assert_eq!(
            coding_session_genesis_session_ref(&event).expect("read tag"),
            session_ref
        );

        // Defensive: the relay's envelope validator makes this unreachable, but
        // a genesis whose reference could not be read must fail loudly rather
        // than be stored under some default key that serializes nothing.
        let untagged = EventBuilder::new(Kind::Custom(44226), "{}")
            .sign_with_keys(&Keys::generate())
            .expect("sign untagged");
        assert!(coding_session_genesis_session_ref(&untagged).is_err());
    }

    /// First claim founds the session; a rival claiming the same reference in
    /// the same channel is refused and leaves nothing behind.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_second_claim_on_one_session_ref_is_rejected() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = make_genesis_event(&Keys::generate(), channel, &session_ref);
        let rival = make_genesis_event(&Keys::generate(), channel, &session_ref);

        let first = insert_coding_session_genesis_event(&pool, community, &founder, channel, None)
            .await
            .expect("found the session");
        assert!(matches!(
            first,
            CodingSessionGenesisInsertOutcome::Founded {
                was_inserted: true,
                ..
            }
        ));

        let second = insert_coding_session_genesis_event(&pool, community, &rival, channel, None)
            .await
            .expect("rival claim");
        match second {
            CodingSessionGenesisInsertOutcome::AlreadyFounded { existing_event_id } => {
                assert_eq!(
                    existing_event_id,
                    founder.id.as_bytes().as_slice(),
                    "the refusal must name the founder the rival has to resolve to"
                );
            }
            other => panic!("rival claim must be refused, got {other:?}"),
        }

        assert!(
            get_event_by_id(&pool, community, rival.id.as_bytes())
                .await
                .expect("look up rival")
                .is_none(),
            "a refused genesis must not be stored — a rejection is not a receipt"
        );
        assert_eq!(count_live_genesis(&pool, community, &session_ref).await, 1);
    }

    /// Deleting a genesis must not release its reference.
    ///
    /// The relay refuses NIP-09 deletion of kind 44226 outright, so this is the
    /// backstop for every *other* way a row could end up soft-deleted. If the
    /// probe skipped deleted rows, "delete your genesis" would be a supported
    /// way to hand your session's foundership to the next person who asks.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_soft_deletion_does_not_release_the_session_ref() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = make_genesis_event(&Keys::generate(), channel, &session_ref);
        insert_coding_session_genesis_event(&pool, community, &founder, channel, None)
            .await
            .expect("found the session");

        assert!(
            soft_delete_event(&pool, community, founder.id.as_bytes())
                .await
                .expect("soft-delete the genesis"),
            "the fixture must actually mark the founder deleted"
        );
        assert_eq!(
            count_live_genesis(&pool, community, &session_ref).await,
            0,
            "the row must really be soft-deleted, or this test proves nothing"
        );

        let rival = make_genesis_event(&Keys::generate(), channel, &session_ref);
        match insert_coding_session_genesis_event(&pool, community, &rival, channel, None)
            .await
            .expect("rival claim over a deleted founder")
        {
            CodingSessionGenesisInsertOutcome::AlreadyFounded { existing_event_id } => {
                assert_eq!(
                    existing_event_id,
                    founder.id.as_bytes().as_slice(),
                    "a deleted genesis still names itself as the founder to resolve to"
                );
            }
            other => panic!("a deleted reference must stay claimed, got {other:?}"),
        }

        assert!(
            get_event_by_id(&pool, community, rival.id.as_bytes())
                .await
                .expect("look up rival")
                .is_none(),
            "the refused rival must leave nothing behind"
        );
    }

    /// Uniqueness is per `(channel, sessionRef)`. Neither axis alone may
    /// contend: the same reference in another channel is a different umbrella,
    /// and another reference in this channel is a different session.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_uniqueness_is_scoped_to_channel_and_session_ref() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let other_channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();
        let other_ref = Uuid::new_v4().to_string();

        for (label, target_channel, target_ref) in [
            ("first claim", channel, &session_ref),
            ("same reference, other channel", other_channel, &session_ref),
            ("other reference, same channel", channel, &other_ref),
        ] {
            let event = make_genesis_event(&Keys::generate(), target_channel, target_ref);
            let outcome =
                insert_coding_session_genesis_event(&pool, community, &event, target_channel, None)
                    .await
                    .unwrap_or_else(|e| panic!("{label} must store: {e}"));
            assert!(
                matches!(
                    outcome,
                    CodingSessionGenesisInsertOutcome::Founded {
                        was_inserted: true,
                        ..
                    }
                ),
                "{label} must found its own umbrella"
            );
        }
    }

    /// A client retrying the *same* signed genesis is not a rival. It must keep
    /// the ordinary replayed-event answer, or every dropped OK turns into a
    /// permanent "someone else founded your session".
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_resubmission_of_the_same_event_stays_idempotent() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();
        let genesis = make_genesis_event(&Keys::generate(), channel, &session_ref);

        for expected_insert in [true, false] {
            let outcome =
                insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
                    .await
                    .expect("resubmit genesis");
            match outcome {
                CodingSessionGenesisInsertOutcome::Founded { was_inserted, .. } => {
                    assert_eq!(was_inserted, expected_insert);
                }
                other => panic!("a replay of the founder is not a rival, got {other:?}"),
            }
        }
        assert_eq!(count_live_genesis(&pool, community, &session_ref).await, 1);
    }

    /// The claim this slice actually has to earn.
    ///
    /// Sequential inserts prove nothing about a check-then-insert race — the
    /// window they miss is exactly the one an attacker aims at. So four rival
    /// genesis events, each on its own pooled connection and its own runtime
    /// worker, are released simultaneously by a barrier and all four race for
    /// one `(channel, sessionRef)`. Exactly one may commit.
    ///
    /// The concurrency is real: separate Postgres backends contend for a real
    /// `pg_advisory_xact_lock`, which is what a horizontally-scaled relay's
    /// competing processes do. It is not, however, *multi-process* — one client
    /// process is enough to exercise the lock, since the lock lives in Postgres
    /// and is oblivious to who connected.
    ///
    /// Rounds are repeated because a single race can be won by scheduling luck
    /// rather than by the lock.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires Postgres"]
    async fn genesis_concurrent_claims_resolve_to_exactly_one_winner() {
        const RIVALS: usize = 4;
        const ROUNDS: usize = 12;

        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;

        for round in 0..ROUNDS {
            let session_ref = Uuid::new_v4().to_string();
            let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(RIVALS));
            let mut claims = Vec::with_capacity(RIVALS);

            for _ in 0..RIVALS {
                let rival = make_genesis_event(&Keys::generate(), channel, &session_ref);
                let pool = pool.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                claims.push(tokio::spawn(async move {
                    // Establish the connection before the barrier so the racers
                    // contend over the advisory lock rather than over the
                    // pool's TCP and startup handshake.
                    drop(pool.acquire().await.expect("warm a pooled connection"));
                    barrier.wait().await;
                    let outcome = insert_coding_session_genesis_event(
                        &pool, community, &rival, channel, None,
                    )
                    .await
                    .expect("rival claim");
                    (rival.id.as_bytes().to_vec(), outcome)
                }));
            }

            let mut winners = Vec::new();
            let mut refused = Vec::new();
            for claim in claims {
                let (event_id, outcome) = claim.await.expect("join rival claim");
                match outcome {
                    CodingSessionGenesisInsertOutcome::Founded { was_inserted, .. } => {
                        assert!(was_inserted, "round {round}: distinct rivals are new rows");
                        winners.push(event_id);
                    }
                    CodingSessionGenesisInsertOutcome::AlreadyFounded { existing_event_id } => {
                        refused.push(existing_event_id);
                    }
                }
            }

            assert_eq!(
                winners.len(),
                1,
                "round {round}: exactly one rival may found a session, got {} winners",
                winners.len()
            );
            assert_eq!(refused.len(), RIVALS - 1, "round {round}");
            for named in &refused {
                assert_eq!(
                    named, &winners[0],
                    "round {round}: every refusal must name the one committed founder"
                );
            }
            assert_eq!(
                count_live_genesis(&pool, community, &session_ref).await,
                1,
                "round {round}: the store must agree with the outcomes it handed out"
            );
        }
    }
}
