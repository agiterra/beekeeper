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

use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
};
use buzz_core::coding_session_genesis::{
    decode_coding_session_genesis, CodingSessionGenesisAdoption,
};
use buzz_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, validate_session_ref, CodingSessionLifecycleAction,
};
use buzz_core::coding_session_payload::{LifecycleReceipt, ReceiptStatus};
use buzz_core::kind::{
    event_kind_i32, is_ephemeral, is_parameterized_replaceable, KIND_AUTH,
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_EVENT_REMINDER, KIND_HUDDLE_STARTED,
    SHARED_GATED_KINDS,
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
    /// Restrict results to events with an `a` tag referencing any of these
    /// addressable coordinates (`<kind>:<owner-hex>:<dtag>`).
    ///
    /// Uses the same JSONB containment (`tags @> ...`) mechanism as
    /// [`EventQuery::e_tags`], served by the same GIN index. Needed by the
    /// NIP-MP Pulse read path (`{"kinds":[44240],"#a":["30621:…"]}`), whose
    /// entries are channel-less: without the pushdown a project's entries are
    /// starved off the page by unrelated global events before post-filtering.
    /// Matching is byte-exact, so callers must pass canonical coordinates.
    pub a_tags: Option<Vec<String>>,
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
    /// Private-project visibility pushdown (NIP-MP Buzz access extension).
    ///
    /// When set, `query_events` appends a pre-`LIMIT` clause excluding
    /// kind:30621 heads that carry `["buzz-access","private"]` unless the
    /// reader is the author or named in a `p` tag:
    /// `AND (kind <> 30621 OR NOT tags @> '[["buzz-access","private"]]'
    ///       OR pubkey = $reader OR tags @> '[["p","<reader-hex>"]]')`.
    /// Same starvation rationale and GIN-index mechanics as
    /// [`Self::shared_gated_reader`]; `event_visible_to_reader` stays as
    /// post-filter defense-in-depth.
    pub project_gated_reader: Option<Vec<u8>>,
    /// Private-project **repo** visibility pushdown (NIP-MP access extension
    /// phase 2).
    ///
    /// When set and the hidden set is non-empty, `query_events` appends a
    /// pre-`LIMIT` clause excluding NIP-34 repo-surface events
    /// ([`buzz_core::kind::GIT_PROJECT_GATED_KINDS`]) that belong to a repo
    /// the reader may not see: 30617/30618 match by `d_tag` against
    /// [`crate::git_repo::HiddenRepos::names`] (repo names are
    /// community-unique), child kinds by `a`-tag coordinate against
    /// [`crate::git_repo::HiddenRepos::coordinates`]. The reader's own events
    /// are always visible. Callers skip setting this when the reader's hidden
    /// set is empty — the common case stays zero-cost. The per-event
    /// `repo_event_hidden_from` re-check stays as post-filter
    /// defense-in-depth (it also normalizes case-variant coordinates the
    /// exact SQL probe would miss).
    pub git_gated_reader: Option<GitGatedReader>,
}

/// Reader identity + hidden-repo set for [`EventQuery::git_gated_reader`].
#[derive(Debug, Clone)]
pub struct GitGatedReader {
    /// The authenticated reader's 32-byte pubkey.
    pub reader: Vec<u8>,
    /// The repos hidden from this reader (see [`crate::git_repo::HiddenRepos`]).
    pub hidden: crate::git_repo::HiddenRepos,
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
            a_tags: None,
            channel_ids: None,
            channel_ids_include_global: true,
            max_limit: None,
            shared_gated_reader: None,
            project_gated_reader: None,
            git_gated_reader: None,
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
    if q.a_tags.as_deref().is_some_and(|a| a.is_empty()) {
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

    // a-tag pushdown via JSONB containment: tags @> '[["a","<coordinate>"]]'.
    // Same mechanism, index, and starvation rationale as the e-tag clause
    // above; `filters_match` still re-checks NIP-01 semantics afterwards.
    if let Some(ref a_tags) = q.a_tags {
        if !a_tags.is_empty() {
            qb.push(" AND (");
            for (i, coordinate) in a_tags.iter().enumerate() {
                if i > 0 {
                    qb.push(" OR ");
                }
                let containment = serde_json::json!([["a", coordinate]]);
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

    // Private-project visibility pushdown: exclude kind:30621 heads carrying
    // ["buzz-access","private"] that the reader neither authored nor is
    // invited to via a `p` tag.  Applied BEFORE ORDER/LIMIT for the same
    // starvation reason as the shared-gated clause above.  Both containment
    // probes are served by idx_events_tags_gin; ingest guarantees `p` values
    // are lowercase 64-hex, so the reader-hex containment is byte-exact.
    if let Some(ref reader_bytes) = q.project_gated_reader {
        let private_containment = serde_json::json!([["buzz-access", "private"]]);
        let reader_p_containment = serde_json::json!([["p", hex::encode(reader_bytes)]]);
        qb.push(format!(" AND ({col_prefix}kind <> "));
        qb.push_bind(buzz_core::kind::KIND_PROJECT as i32);
        qb.push(format!(" OR NOT {col_prefix}tags @> "));
        qb.push_bind(private_containment);
        qb.push(format!(" OR {col_prefix}pubkey = "));
        qb.push_bind(reader_bytes.clone());
        qb.push(format!(" OR {col_prefix}tags @> "));
        qb.push_bind(reader_p_containment);
        qb.push(")");
    }

    // Private-project repo visibility pushdown: exclude NIP-34 repo-surface
    // events belonging to repos hidden from the reader.  30617/30618 are
    // matched by repo name (their `d_tag`; community-unique, and 30618 is
    // relay-signed so its pubkey never identifies the owner), child kinds by
    // `a`-tag coordinate containment probed per hidden repo (served by
    // idx_events_tags_gin).  The leading kind guard short-circuits every
    // non-git row.  Applied BEFORE ORDER/LIMIT for the same starvation
    // reason as the clauses above.
    if let Some(ref git_gate) = q.git_gated_reader {
        if !git_gate.hidden.is_empty() {
            let hidden_names: Vec<String> = git_gate.hidden.names.iter().cloned().collect();
            qb.push(format!(" AND ({col_prefix}kind NOT IN ("));
            let mut sep = qb.separated(", ");
            for kind in buzz_core::kind::GIT_PROJECT_GATED_KINDS {
                sep.push_bind(*kind as i32);
            }
            qb.push(format!(") OR {col_prefix}pubkey = "));
            qb.push_bind(git_gate.reader.clone());
            // Announcement + ref-state: hidden when the d_tag names a hidden repo.
            qb.push(format!(" OR (({col_prefix}kind NOT IN ("));
            qb.push_bind(buzz_core::kind::KIND_GIT_REPO_ANNOUNCEMENT as i32);
            qb.push(", ");
            qb.push_bind(buzz_core::kind::KIND_GIT_REPO_STATE as i32);
            qb.push(format!(
                ") OR {col_prefix}d_tag IS NULL OR NOT ({col_prefix}d_tag = ANY("
            ));
            qb.push_bind(hidden_names);
            qb.push(")))");
            // Child kinds: hidden when any `a` tag carries a hidden coordinate.
            qb.push(format!(" AND ({col_prefix}kind IN ("));
            qb.push_bind(buzz_core::kind::KIND_GIT_REPO_ANNOUNCEMENT as i32);
            qb.push(", ");
            qb.push_bind(buzz_core::kind::KIND_GIT_REPO_STATE as i32);
            qb.push(") OR NOT (");
            let mut first = true;
            for coordinate in &git_gate.hidden.coordinates {
                if !first {
                    qb.push(" OR ");
                }
                first = false;
                qb.push(format!("{col_prefix}tags @> "));
                qb.push_bind(serde_json::json!([["a", coordinate]]));
            }
            if first {
                qb.push("FALSE");
            }
            qb.push("))");
            // Announcement's own `project` tag: hidden when it names a
            // private project that does not admit the reader — covers a
            // 30617 whose side-effect projection never landed (its name is
            // then absent from the hidden-names set above).
            qb.push(format!(" AND ({col_prefix}kind <> "));
            qb.push_bind(buzz_core::kind::KIND_GIT_REPO_ANNOUNCEMENT as i32);
            qb.push(" OR NOT (");
            let mut first = true;
            for coordinate in &git_gate.hidden.project_coordinates {
                if !first {
                    qb.push(" OR ");
                }
                first = false;
                qb.push(format!("{col_prefix}tags @> "));
                qb.push_bind(serde_json::json!([["project", coordinate]]));
            }
            if first {
                qb.push("FALSE");
            }
            qb.push("))))");
        }

        // NIP-MP Pulse (44240): a Pulse entry belongs to a *project*, not to a
        // repo, so none of the repo clauses above ever see it — 44240 is
        // absent from `GIT_PROJECT_GATED_KINDS`, whose leading `kind NOT IN`
        // guard would short-circuit the whole gate to TRUE for it. Exclude
        // entries whose `a` tag names a private project hidden from this
        // reader before ORDER/LIMIT, for the same starvation reason as the
        // clauses above. `pulse_entry_hidden_from` stays as post-filter
        // defense-in-depth: it normalizes case-variant coordinates this exact
        // probe would miss, and fails closed when no coordinate parses.
        if !git_gate.hidden.project_coordinates.is_empty() {
            qb.push(format!(" AND ({col_prefix}kind <> "));
            qb.push_bind(buzz_core::kind::KIND_PULSE_ENTRY as i32);
            qb.push(format!(" OR {col_prefix}pubkey = "));
            qb.push_bind(git_gate.reader.clone());
            qb.push(" OR NOT (");
            let mut first = true;
            for coordinate in &git_gate.hidden.project_coordinates {
                if !first {
                    qb.push(" OR ");
                }
                first = false;
                qb.push(format!("{col_prefix}tags @> "));
                qb.push_bind(serde_json::json!([["a", coordinate]]));
            }
            qb.push("))");
        }
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
    if q.a_tags.as_deref().is_some_and(|a| a.is_empty()) {
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

    // a-tag pushdown via JSONB containment — mirrors `query_events`.
    if let Some(ref a_tags) = q.a_tags {
        if !a_tags.is_empty() {
            qb.push(" AND (");
            for (i, coordinate) in a_tags.iter().enumerate() {
                if i > 0 {
                    qb.push(" OR ");
                }
                let containment = serde_json::json!([["a", coordinate]]);
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
///
/// Re-validates the canonical UUID form the relay's envelope validator already
/// enforced. That is not distrust of the caller so much as a precondition this
/// module relies on directly: the reference is interpolated into a `LIKE`
/// pattern by [`legacy_session_creates_tx`], and canonical form is what
/// guarantees it holds no `%` or `_` to widen that pattern with.
fn coding_session_genesis_session_ref(event: &Event) -> Result<&str> {
    let session_ref = event
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
        })?;
    validate_session_ref(session_ref).map_err(DbError::InvalidData)?;
    Ok(session_ref)
}

/// The tag both a lifecycle command (44221) and its receipt (44224) carry the
/// `commandId` in. Indexed by the events tag GIN, so the create→receipt join is
/// an index lookup rather than a scan.
const CODING_SESSION_COMMAND_TAG: &str = "csl-command";

/// A `session.create` already in this channel's history, decoded generically
/// — not yet filtered to any particular `sessionRef` or `commandId`. Two
/// different call sites narrow this differently: the R8 existence gate
/// filters by `sessionRef` ([`legacy_session_creates_tx`]); the R16 ambiguity
/// check filters by `commandId` ([`creates_by_command_id_tx`]).
#[derive(Debug, Clone)]
struct LegacySessionCreate {
    event_id: Vec<u8>,
    signer: Vec<u8>,
    command_id: String,
    session_ref: Option<String>,
    provider_authority_pubkey: String,
}

/// Why an explicit coding-session genesis adoption reference could not be
/// verified, or why an ordinary (non-adopting) genesis could not found a
/// `sessionRef` that legacy history already uses. Every variant is a refusal:
/// none is a fallback that founds the session anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenesisAdoptionRefusal {
    /// No `adopts` reference was given, but `session.create` history in this
    /// channel already uses the `sessionRef` — the relay no longer infers a
    /// founder from that history (R15); the publisher must resubmit with an
    /// explicit `adopts` reference.
    LegacyHistoryRequiresAdoption {
        /// A create bearing the reference, so the operator has something
        /// concrete to adopt.
        existing_create_event_id: Vec<u8>,
    },
    /// `adopts.createEventId` does not name a stored, decodable
    /// `session.create` (kind 44221) event.
    ReferencedCreateNotFound,
    /// `adopts.receiptEventId` does not name a stored, decodable lifecycle
    /// receipt (kind 44224) event.
    ReferencedReceiptNotFound,
    /// The referenced receipt does not genuinely answer the referenced
    /// create: its `commandId` disagrees, it was not signed by the create's
    /// named provider authority, or it minted no session at all.
    ReceiptDoesNotJoinCreate,
    /// The referenced create claims a different `sessionRef` than the
    /// genesis (or claims none).
    SessionRefMismatch,
    /// The genesis signer is not the referenced create's signer.
    SignerMismatch {
        /// The create's actual signer, for the rejection message.
        founder_pubkey: Vec<u8>,
    },
    /// The referenced create or receipt was not published in the genesis's
    /// own channel.
    WrongChannel,
    /// Other `session.create` events in this channel share the founding
    /// `commandId` but disagree with it on signer or `sessionRef` (R16) —
    /// mirrors the desktop's `codingSessionCreateObservations.ts` ambiguity
    /// rule, which disputes a commandId on either disagreement.
    CommandIdAmbiguous {
        /// Which agreement failed, for the refusal message.
        reason: &'static str,
    },
}

/// Decode a stored 44221 row as a `session.create`, generically — no
/// `sessionRef` or `commandId` filter applied.
///
/// Returns `None` for every other shape — a resume, a stop, or content this
/// build cannot decode. Undecodable content is skipped rather than failing
/// the transaction: a create the strict decoder rejects was never a create
/// any provider acted on, and one unparseable row must not make founding
/// *any* session impossible.
fn decode_session_create(
    event_id: Vec<u8>,
    signer: Vec<u8>,
    content: &str,
) -> Option<LegacySessionCreate> {
    let payload = decode_coding_session_lifecycle_command(content).ok()?;
    let CodingSessionLifecycleAction::SessionCreate {
        session_ref,
        provider_authority_pubkey,
        ..
    } = payload.action
    else {
        return None;
    };
    Some(LegacySessionCreate {
        event_id,
        signer,
        command_id: payload.command_id,
        session_ref,
        provider_authority_pubkey,
    })
}

/// [`decode_session_create`], filtered to creates claiming exactly
/// `session_ref`. Used by [`legacy_session_creates_tx`]'s R8 existence gate.
fn legacy_session_create(
    event_id: Vec<u8>,
    signer: Vec<u8>,
    content: &str,
    session_ref: &str,
) -> Option<LegacySessionCreate> {
    let create = decode_session_create(event_id, signer, content)?;
    if create.session_ref.as_deref() != Some(session_ref) {
        return None;
    }
    Some(create)
}

/// Every `session.create` in this channel that already claims `session_ref`.
///
/// # Why this is a scan and not an index lookup
///
/// A 44221 carries exactly three tags — `h`, `csl-v`, `csl-command` — and none
/// of them is the umbrella reference, which lives only in the signed content.
/// So there is no filter that finds creates by `sessionRef`; the narrowest
/// available predicate is `(community_id, kind, channel_id)`, which the
/// `(community_id, kind, created_at)` index serves, and the reference match
/// happens after.
///
/// The `LIKE` is a bandwidth prefilter over that set, not the test: the
/// authoritative check is [`legacy_session_create`]'s strict decode of the same
/// bytes. It is a safe superset because a JSON encoder that writes
/// `"sessionRef":"<uuid>"` writes those 36 characters literally — a canonical
/// UUID is hex and hyphens, which no mainstream serializer `\u`-escapes — and
/// because the same canonical form makes the pattern free of `%` and `_`.
/// A create the prefilter missed would be a create the strict decode never
/// sees, so this assumption is stated rather than buried.
///
/// # Soft-deleted rows count
///
/// For the same reason the genesis uniqueness probe counts them: deleting a
/// create must not reopen the "does legacy history exist" question. If
/// deleted rows were skipped, a founder who tidied up their own history — or a
/// moderator who removed one message — would silently make a `sessionRef`
/// claimable as a fresh founding again.
async fn legacy_session_creates_tx(
    tx: &mut Transaction<'_, Postgres>,
    community_id: CommunityId,
    channel_id: Uuid,
    session_ref: &str,
) -> Result<Vec<LegacySessionCreate>> {
    let rows: Vec<(Vec<u8>, Vec<u8>, String)> = sqlx::query_as(
        "SELECT id, pubkey, content FROM events \
         WHERE community_id = $1 AND kind = $2 AND channel_id = $3 \
         AND content LIKE '%' || $4 || '%' \
         ORDER BY created_at ASC, id ASC",
    )
    .bind(community_id.as_uuid())
    .bind(KIND_CODING_SESSION_LIFECYCLE_COMMAND as i32)
    .bind(channel_id)
    .bind(session_ref)
    .fetch_all(&mut **tx)
    .await?;

    // Ordered `created_at ASC, id ASC` in SQL and kept that way: `id` is raw
    // bytes here and lowercase hex in the consumer, and those two orderings
    // agree, so "earliest create" means the same event on both sides.
    Ok(rows
        .into_iter()
        .filter_map(|(event_id, signer, content)| {
            legacy_session_create(event_id, signer, &content, session_ref)
        })
        .collect())
}

/// Every `session.create` in this channel bearing `command_id`, regardless of
/// which `sessionRef` each one claims.
///
/// This is the scope R16 requires and [`legacy_session_creates_tx`] cannot
/// provide: that function prefilters by `sessionRef` first, so a rival create
/// sharing the same `commandId` but claiming a *different* `sessionRef` is
/// invisible to it — exactly the gap the desktop's
/// `codingSessionCreateObservations.ts` does not have, because its bucket key
/// is `(channelId, commandId)` alone. `csl-command` is an indexed tag (unlike
/// `sessionRef`), so this is an exact containment lookup, not a scan.
///
/// Soft-deleted rows count here for the same reason as
/// [`legacy_session_creates_tx`]: a disagreement does not stop being a
/// disagreement because one of the disagreeing creates was later deleted.
async fn creates_by_command_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    community_id: CommunityId,
    channel_id: Uuid,
    command_id: &str,
) -> Result<Vec<LegacySessionCreate>> {
    let probe = serde_json::json!([[CODING_SESSION_COMMAND_TAG, command_id]]);
    let rows: Vec<(Vec<u8>, Vec<u8>, String)> = sqlx::query_as(
        "SELECT id, pubkey, content FROM events \
         WHERE community_id = $1 AND kind = $2 AND channel_id = $3 \
         AND tags @> $4::jsonb \
         ORDER BY created_at ASC, id ASC",
    )
    .bind(community_id.as_uuid())
    .bind(KIND_CODING_SESSION_LIFECYCLE_COMMAND as i32)
    .bind(channel_id)
    .bind(&probe)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(event_id, signer, content)| {
            let create = decode_session_create(event_id, signer, &content)?;
            (create.command_id == command_id).then_some(create)
        })
        .collect())
}

/// Fetch one coding-session event (create or receipt) by raw event id,
/// **including soft-deleted rows** — an adoption reference must keep
/// resolving even if the referenced create or receipt was later removed from
/// the timeline, exactly as a genesis itself stays founded through deletion
/// (R6). `expected_kind` is pushed into the query as defense in depth on top
/// of the caller's own kind check on the decoded payload.
async fn fetch_coding_session_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    community_id: CommunityId,
    id_bytes: &[u8],
    expected_kind: i32,
) -> Result<Option<StoredEvent>> {
    let row = sqlx::query(
        "SELECT id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id \
         FROM events WHERE community_id = $1 AND id = $2 AND kind = $3 \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(community_id.as_uuid())
    .bind(id_bytes)
    .bind(expected_kind)
    .fetch_optional(&mut **tx)
    .await?;

    match row {
        Some(r) => row_to_stored_event(r),
        None => Ok(None),
    }
}

/// Verify that `create.provider_authority_pubkey` genuinely signed `receipt`
/// as the answer to `create.command_id`, and that the receipt minted an
/// actual session — the same "command id echo" pattern
/// [`creates_by_command_id_tx`]'s caller relies on elsewhere, reproduced here
/// for one specific referenced pair instead of a scan across candidates.
///
/// "Minted" is enforced by status, not merely by `session` being present. A
/// kind 44224 also carries the *turn* vocabulary (`turn_queued`,
/// `turn_started`, `turn_dropped`, `turn_refused`), and every one of those
/// names the addressed execution in `session` while proving nothing about a
/// create — `turn_refused` proves the opposite, that the provider has no such
/// session. Since a turn command's `commandId` is chosen by whoever signs the
/// 44220, accepting a turn stage here would let anyone manufacture a receipt
/// that echoes a create's `commandId` and adopt a create the provider
/// refused. Only the four statuses that mint an execution join a create.
fn receipt_joins_create(
    create: &LegacySessionCreate,
    receipt_signer: &[u8],
    receipt_content: &str,
) -> bool {
    let Ok(authority) = hex::decode(&create.provider_authority_pubkey) else {
        return false;
    };
    if authority.is_empty() || receipt_signer != authority.as_slice() {
        return false;
    }
    let Ok(receipt) = serde_json::from_str::<LifecycleReceipt>(receipt_content) else {
        return false;
    };
    let mints_a_session = matches!(
        receipt.status,
        ReceiptStatus::Created
            | ReceiptStatus::CreatedWithFailedInitialTurn
            | ReceiptStatus::Resumed
            | ReceiptStatus::ResumedWithoutContext
    );
    mints_a_session && receipt.command_id == create.command_id && receipt.session.is_some()
}

/// Verify an explicit `adopts` reference against the events it names.
///
/// Implements R15 step 2 (a)-(e) plus the R16 ambiguity parity fix: fetch the
/// referenced create and receipt by event id (never by scanning history for a
/// claim — see the module doc on
/// [`buzz_core::coding_session_genesis`]), verify the receipt genuinely joins
/// the create, that the create bears the genesis's `sessionRef` and was
/// signed by the genesis's signer, that both events sit in the genesis's own
/// channel, and finally that no other create in the channel sharing the
/// founding `commandId` disagrees on signer or `sessionRef`.
async fn adjudicate_explicit_adoption_tx(
    tx: &mut Transaction<'_, Postgres>,
    community_id: CommunityId,
    channel_id: Uuid,
    session_ref: &str,
    genesis_signer: &[u8],
    adopts: &CodingSessionGenesisAdoption,
) -> Result<Option<GenesisAdoptionRefusal>> {
    // The payload decoder already enforced 64-lowercase-hex, but a decode
    // failure here is handled rather than unwrapped — this function must
    // fail closed, never panic, on any input the caller passes it.
    let Ok(create_id) = hex::decode(&adopts.create_event_id) else {
        return Ok(Some(GenesisAdoptionRefusal::ReferencedCreateNotFound));
    };
    let Ok(receipt_id) = hex::decode(&adopts.receipt_event_id) else {
        return Ok(Some(GenesisAdoptionRefusal::ReferencedReceiptNotFound));
    };

    let Some(create_event) = fetch_coding_session_event_tx(
        tx,
        community_id,
        &create_id,
        KIND_CODING_SESSION_LIFECYCLE_COMMAND as i32,
    )
    .await?
    else {
        return Ok(Some(GenesisAdoptionRefusal::ReferencedCreateNotFound));
    };
    let Some(receipt_event) = fetch_coding_session_event_tx(
        tx,
        community_id,
        &receipt_id,
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT as i32,
    )
    .await?
    else {
        return Ok(Some(GenesisAdoptionRefusal::ReferencedReceiptNotFound));
    };

    let Some(create) = decode_session_create(
        create_event.event.id.as_bytes().to_vec(),
        create_event.event.pubkey.as_bytes().to_vec(),
        &create_event.event.content,
    ) else {
        return Ok(Some(GenesisAdoptionRefusal::ReferencedCreateNotFound));
    };

    if create_event.channel_id != Some(channel_id) || receipt_event.channel_id != Some(channel_id) {
        return Ok(Some(GenesisAdoptionRefusal::WrongChannel));
    }

    if !receipt_joins_create(
        &create,
        receipt_event.event.pubkey.as_bytes(),
        &receipt_event.event.content,
    ) {
        return Ok(Some(GenesisAdoptionRefusal::ReceiptDoesNotJoinCreate));
    }

    if create.session_ref.as_deref() != Some(session_ref) {
        return Ok(Some(GenesisAdoptionRefusal::SessionRefMismatch));
    }

    if create.signer != genesis_signer {
        return Ok(Some(GenesisAdoptionRefusal::SignerMismatch {
            founder_pubkey: create.signer.clone(),
        }));
    }

    // R16: every create in the channel sharing the founding commandId must
    // agree on signer and sessionRef — not just the ones this genesis
    // happens to reference. See `creates_by_command_id_tx`'s doc for why the
    // prior sessionRef-first scoping made this invisible.
    let siblings =
        creates_by_command_id_tx(tx, community_id, channel_id, &create.command_id).await?;
    let distinct_signers = siblings
        .iter()
        .map(|c| &c.signer)
        .collect::<std::collections::HashSet<_>>();
    if distinct_signers.len() > 1 {
        return Ok(Some(GenesisAdoptionRefusal::CommandIdAmbiguous {
            reason: "two signers claim the founding command",
        }));
    }
    let distinct_session_refs = siblings
        .iter()
        .map(|c| &c.session_ref)
        .collect::<std::collections::HashSet<_>>();
    if distinct_session_refs.len() > 1 {
        return Ok(Some(GenesisAdoptionRefusal::CommandIdAmbiguous {
            reason: "two sessionRefs claim the founding command",
        }));
    }

    Ok(None)
}

/// Decide whether this genesis may found `session_ref`: verify its explicit
/// `adopts` reference, or — absent one — refuse when legacy `session.create`
/// history already uses the reference (R8, kept, but no longer resolved by
/// inference: see the module doc on
/// [`buzz_core::coding_session_genesis`]).
async fn adjudicate_genesis_adoption_tx(
    tx: &mut Transaction<'_, Postgres>,
    community_id: CommunityId,
    channel_id: Uuid,
    session_ref: &str,
    event: &Event,
) -> Result<Option<GenesisAdoptionRefusal>> {
    let payload = decode_coding_session_genesis(&event.content).map_err(DbError::InvalidData)?;

    match &payload.adopts {
        Some(adopts) => {
            adjudicate_explicit_adoption_tx(
                tx,
                community_id,
                channel_id,
                session_ref,
                event.pubkey.as_bytes(),
                adopts,
            )
            .await
        }
        None => {
            let creates =
                legacy_session_creates_tx(tx, community_id, channel_id, session_ref).await?;
            match creates.first() {
                Some(existing) => Ok(Some(
                    GenesisAdoptionRefusal::LegacyHistoryRequiresAdoption {
                        existing_create_event_id: existing.event_id.clone(),
                    },
                )),
                None => Ok(None),
            }
        }
    }
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
    /// Either no `adopts` reference was given over `sessionRef` history that
    /// requires one, or the given reference did not verify. Nothing was
    /// written.
    AdoptionRefused {
        /// Why the adoption was refused.
        refusal: GenesisAdoptionRefusal,
    },
}

/// Atomically enforce one genesis per `(channel, sessionRef)`, refuse a claim
/// over pre-genesis create history, and store the event.
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
///
/// # Sessions that predate genesis
///
/// First-insert-wins uniqueness, on its own, is a hijack kit for every session
/// that already exists. Those sessions' `sessionRef`s are public in 44221
/// history, and the rule above would *defend* whoever published a genesis for
/// one first — the person with the fewest scruples, not the founder.
///
/// So an ordinary (no-`adopts`) genesis is refused whenever create history in
/// this channel already uses the reference. Adoption is the way through, and
/// it is **explicit** (R15): the genesis payload names the exact founding
/// `session.create` and its joining receipt by event id, and the transaction
/// verifies that reference — receipt genuinely joins create, create bears the
/// same `sessionRef`, genesis signer equals the create's signer, both events
/// sit in this channel — rather than inferring a founder from local history.
/// An earlier revision did the latter (a plain genesis whose signer happened
/// to match a projected founder was treated as an implicit adoption); that
/// was rejected precisely because a signed event's meaning would then depend
/// on which relay's database happened to hold the create history (R13, R15).
/// Adopted geneses are ordinary geneses afterwards, so the uniqueness probe
/// counts them exactly like any other.
///
/// See [`adjudicate_genesis_adoption_tx`] for the dispatch between the two
/// forms, [`adjudicate_explicit_adoption_tx`] for the reference verification,
/// and [`creates_by_command_id_tx`] for the R16 cross-`sessionRef` ambiguity
/// check.
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

    // Second, and inside the same lock: verify an explicit `adopts`
    // reference, or refuse a plain genesis over history that requires one.
    // See `Sessions that predate genesis` above.
    if let Some(refusal) =
        adjudicate_genesis_adoption_tx(&mut tx, community_id, channel_id, session_ref, event)
            .await?
    {
        tx.rollback().await?;
        return Ok(CodingSessionGenesisInsertOutcome::AdoptionRefused { refusal });
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

// ---- Coding-session authority chain (kind 44228) --------------------------

/// The tag an authority-transition event mirrors its genesis reference into,
/// for the same reason genesis mirrors `sessionRef` into `csg-session`: the
/// relay's storage transaction needs an indexed way to find every transition
/// for one genesis atomically with the insert, and content is opaque to the
/// tag GIN. Re-derived from the decoded payload rather than trusted — see
/// [`coding_session_authority_transition_genesis_ref`].
///
/// Consumers must never select the chain by this tag either, for the same
/// reason `csg-session` is enforcement/diagnostics-only (R7): the chain
/// resolves by walking `prevAccepted` links from an explicit id, not by
/// scanning for tag matches.
const CODING_SESSION_AUTHORITY_TAG: &str = "csat-genesis";

/// Domain separator for the authority-transition advisory-lock key space —
/// see [`CODING_SESSION_GENESIS_LOCK_DOMAIN`] for why a domain constant
/// exists at all and why an accidental collision with it is harmless.
const CODING_SESSION_AUTHORITY_LOCK_DOMAIN: &[u8] = b"buzz.coding-session.authority.v1";

/// Derive the advisory-lock key that serializes transition writes for one
/// `(community, channel, genesisRef)`.
///
/// Scoped like [`coding_session_genesis_lock_key`]: the signer is
/// deliberately absent (two rival transitions for the same genesis must
/// contend for the same lock regardless of who signed them), and
/// `genesis_ref` plus `channel_id` are both folded in so unrelated chains
/// never contend with each other.
fn coding_session_authority_lock_key(
    community_id: CommunityId,
    channel_id: Uuid,
    genesis_ref: &str,
) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(CODING_SESSION_AUTHORITY_LOCK_DOMAIN);
    hasher.update(community_id.as_uuid().as_bytes());
    hasher.update(channel_id.as_bytes());
    hasher.update((genesis_ref.len() as u64).to_le_bytes());
    hasher.update(genesis_ref.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 8];
    key.copy_from_slice(&digest[..8]);
    i64::from_le_bytes(key)
}

/// Read the genesis reference an authority-transition event mirrors into its
/// tags, re-validating it is well-formed hex — the same "re-derive, don't
/// trust" discipline as [`coding_session_genesis_session_ref`].
fn coding_session_authority_transition_genesis_ref(event: &Event) -> Result<&str> {
    event
        .tags
        .iter()
        .find_map(|tag| {
            let parts = tag.as_slice();
            match parts {
                [name, value, ..] if name == CODING_SESSION_AUTHORITY_TAG => Some(value.as_str()),
                _ => None,
            }
        })
        .ok_or_else(|| {
            DbError::InvalidData(format!(
                "coding-session authority transition is missing its {CODING_SESSION_AUTHORITY_TAG} tag"
            ))
        })
}

/// Why an authority transition could not be accepted. Every variant is a
/// refusal: nothing is written for any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityTransitionRefusal {
    /// `genesisRef` does not name a stored, decodable coding-session genesis
    /// (kind 44226) event. Soft-deleted genesis rows still count (R6: a
    /// genesis is a permanent identity anchor), so this variant means the
    /// reference never existed at all, not merely that it was later removed.
    GenesisNotFound,
    /// The referenced genesis was not published in this transition's own
    /// channel, or its own stored envelope does not agree with itself
    /// (`csg-session` disagreeing with its decoded `sessionRef`) — a defense-
    /// in-depth re-check of the reference's integrity rather than a
    /// consumer-facing distinction (R22's discipline applied here: verify the
    /// resolved reference's envelope fully, never trust it by relay-shape
    /// alone).
    WrongChannel,
    /// `prevAccepted` does not match the chain's current accepted head for
    /// this `genesisRef` (or the chain has a head but `prevAccepted` was
    /// `null`, or vice versa) — this transition named a stale or wrong
    /// predecessor.
    StaleHead {
        /// The predecessor this transition should have named, for the
        /// rejection message. `None` means the chain has no accepted
        /// transitions yet, so only `prevAccepted: null` would have been
        /// accepted.
        expected_prev_accepted: Option<Vec<u8>>,
    },
    /// `seq` does not equal one more than the current head's `seq` (or `1`
    /// when the chain is empty).
    SeqMismatch {
        /// The `seq` this transition should have carried.
        expected_seq: u32,
    },
    /// The signer is not the session's current owner. Only the genesis
    /// signer is ever the owner today — see
    /// [`current_coding_session_authority_owner`].
    SignerNotOwner {
        /// The actual owner's pubkey, for the rejection message.
        owner_pubkey: Vec<u8>,
    },
    /// A `revoke` named a pubkey holding no live grant at that point in the
    /// chain — a no-op link would burn a `seq` for nothing, so it is refused
    /// rather than accepted-and-ignored.
    NoSuchGrant,
}

/// Outcome of an attempted coding-session authority-transition (kind 44228)
/// insert.
#[derive(Debug)]
pub enum CodingSessionAuthorityTransitionInsertOutcome {
    /// This transition extended the chain (or was an idempotent replay of an
    /// already-accepted one). The transaction committed.
    Accepted {
        /// The stored transition event.
        stored_event: Box<StoredEvent>,
        /// Whether the event row itself was newly inserted. `false` means an
        /// idempotent resubmission of an already-accepted transition.
        was_inserted: bool,
    },
    /// The transition could not be accepted. Nothing was written.
    Refused {
        /// Why.
        refusal: AuthorityTransitionRefusal,
    },
}

/// Resolve today's session owner: the genesis signer (the founder).
///
/// This is the single seam future transition types change. `grant-operator`
/// never moves ownership — it only adds an operator — so today's owner
/// resolution never needs to consult the chain itself, only the genesis. A
/// future `transfer` or `takeover` type changes this function's body (to walk
/// the chain for the most recent ownership-moving transition) without
/// touching any of its callers, which continue to ask "who is the current
/// owner" exactly as they do today.
fn current_coding_session_authority_owner(genesis_event: &StoredEvent) -> Vec<u8> {
    genesis_event.event.pubkey.as_bytes().to_vec()
}

/// Fetch and re-validate the genesis an authority transition references.
///
/// Fetches by id including soft-deleted rows (R6: identity facts are
/// permanent, so a later deletion must not make a chain's root
/// unresolvable), then re-checks the resolved genesis's own envelope rather
/// than trusting its stored shape: it must decode, sit in the transition's
/// channel, and its `csg-session` tag must agree with its own decoded
/// `sessionRef` (R22's "validate the full envelope of a resolved reference,
/// fail closed on any mismatch" discipline, applied to the relay's own
/// storage-transaction reads).
async fn fetch_and_verify_authority_genesis_tx(
    tx: &mut Transaction<'_, Postgres>,
    community_id: CommunityId,
    channel_id: Uuid,
    genesis_ref: &str,
) -> Result<std::result::Result<StoredEvent, AuthorityTransitionRefusal>> {
    let Ok(genesis_id) = hex::decode(genesis_ref) else {
        return Ok(Err(AuthorityTransitionRefusal::GenesisNotFound));
    };
    let Some(genesis_event) = fetch_coding_session_event_tx(
        tx,
        community_id,
        &genesis_id,
        KIND_CODING_SESSION_GENESIS as i32,
    )
    .await?
    else {
        return Ok(Err(AuthorityTransitionRefusal::GenesisNotFound));
    };

    if genesis_event.channel_id != Some(channel_id) {
        return Ok(Err(AuthorityTransitionRefusal::WrongChannel));
    }

    let Ok(genesis_payload) = decode_coding_session_genesis(&genesis_event.event.content) else {
        return Ok(Err(AuthorityTransitionRefusal::GenesisNotFound));
    };
    let tag_session_ref = coding_session_genesis_session_ref(&genesis_event.event)
        .ok()
        .map(str::to_owned);
    if tag_session_ref.as_deref() != Some(genesis_payload.session_ref.as_str()) {
        return Ok(Err(AuthorityTransitionRefusal::WrongChannel));
    }

    Ok(Ok(genesis_event))
}

/// One transition already stored for `genesis_ref`, decoded — used to derive
/// the chain's current head and to fold the live grant set (which a `revoke`
/// must be validated against).
struct StoredAuthorityTransition {
    event_id: Vec<u8>,
    seq: u32,
    transition_type: CodingSessionAuthorityTransitionType,
    grantee_pubkey: String,
}

/// Fold accepted transitions (already sorted or not — sorted here) up to but
/// excluding `before_seq` into the live grant map: grantee hex → role string
/// (`"operator"` / `"viewer"`). `grant-*` sets, `revoke` removes.
fn fold_authority_grants(
    transitions: &[StoredAuthorityTransition],
    before_seq: u32,
) -> std::collections::HashMap<String, &'static str> {
    let mut ordered: Vec<&StoredAuthorityTransition> =
        transitions.iter().filter(|t| t.seq < before_seq).collect();
    ordered.sort_by_key(|t| t.seq);
    let mut grants = std::collections::HashMap::new();
    for t in ordered {
        match t.transition_type {
            CodingSessionAuthorityTransitionType::GrantOperator => {
                grants.insert(t.grantee_pubkey.clone(), "operator");
            }
            CodingSessionAuthorityTransitionType::GrantViewer => {
                grants.insert(t.grantee_pubkey.clone(), "viewer");
            }
            CodingSessionAuthorityTransitionType::Revoke => {
                grants.remove(&t.grantee_pubkey);
            }
        }
    }
    grants
}

/// Every already-stored authority transition for `genesis_ref` in this
/// channel, **excluding** `exclude_event_id`.
///
/// The exclusion is what makes resubmitting an already-accepted transition
/// idempotent rather than self-contradictory: without it, a transition that
/// is itself the current head would be compared against its own id as its
/// required predecessor and always fail. See
/// [`insert_coding_session_authority_transition_event`]'s `# Resubmission`
/// section — the same shape as genesis's own resubmission handling.
///
/// Decode failures are skipped rather than failing the transaction, mirroring
/// [`legacy_session_creates_tx`]: a row this build cannot decode was never a
/// row any prior accept-path produced, since every accepted transition passed
/// through [`decode_coding_session_authority_transition`] before it was
/// stored.
async fn stored_authority_transitions_tx(
    tx: &mut Transaction<'_, Postgres>,
    community_id: CommunityId,
    channel_id: Uuid,
    genesis_ref: &str,
    exclude_event_id: &[u8],
) -> Result<Vec<StoredAuthorityTransition>> {
    let probe = serde_json::json!([[CODING_SESSION_AUTHORITY_TAG, genesis_ref]]);
    let rows: Vec<(Vec<u8>, String)> = sqlx::query_as(
        "SELECT id, content FROM events \
         WHERE community_id = $1 AND kind = $2 AND channel_id = $3 \
         AND id <> $4 AND tags @> $5::jsonb",
    )
    .bind(community_id.as_uuid())
    .bind(buzz_core::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION as i32)
    .bind(channel_id)
    .bind(exclude_event_id)
    .bind(&probe)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(event_id, content)| {
            let payload = decode_coding_session_authority_transition(&content).ok()?;
            Some(StoredAuthorityTransition {
                event_id,
                seq: payload.seq,
                transition_type: payload.transition_type,
                grantee_pubkey: payload.grantee_pubkey,
            })
        })
        .collect())
}

/// Atomically validate one authority-transition event against the chain and
/// the current owner's standing, and store it.
///
/// # Why this lives here and not at ingest
///
/// Exactly [`insert_coding_session_genesis_event`]'s reasoning: "is this the
/// next accepted link in the chain" is a question only the storage
/// transaction can answer atomically, so `begin` → `pg_advisory_xact_lock` →
/// probe the chain and the genesis → insert → `commit`, with the lock held
/// across both the probe and the insert. Two rival transitions for the same
/// genesis cannot both observe the same head: the loser blocks on the lock,
/// and by the time it runs its own probe the winner's row is committed and
/// visible — which is what turns "a transition naming a stale head" from an
/// approximate check into the actual serialization point (this bite's whole
/// architectural bet).
///
/// # Resubmission
///
/// [`stored_authority_transitions_tx`] excludes this event's own id from the
/// chain scan, so replaying an already-accepted transition recomputes the
/// *same* expected `(prevAccepted, seq)` it originally satisfied, passes
/// validation again, and falls through to `ON CONFLICT DO NOTHING` —
/// `Accepted { was_inserted: false, .. }` — exactly as genesis's own replay
/// does. Only a genuinely different, non-extending transition is refused.
pub async fn insert_coding_session_authority_transition_event(
    pool: &PgPool,
    community_id: CommunityId,
    event: &Event,
    channel_id: Uuid,
    thread_meta: Option<ThreadMetadataParams<'_>>,
) -> Result<CodingSessionAuthorityTransitionInsertOutcome> {
    let genesis_ref = coding_session_authority_transition_genesis_ref(event)?.to_owned();
    let payload =
        decode_coding_session_authority_transition(&event.content).map_err(DbError::InvalidData)?;
    // All three pinned transition types are chain links with identical
    // envelope/authorization rules; their per-type meaning is applied to the
    // grant ACL below, after the chain checks pass.

    let lock_key = coding_session_authority_lock_key(community_id, channel_id, &genesis_ref);
    let id_bytes = event.id.as_bytes();

    let mut tx = pool.begin().await?;

    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *tx)
        .await?;

    let genesis_event = match fetch_and_verify_authority_genesis_tx(
        &mut tx,
        community_id,
        channel_id,
        &genesis_ref,
    )
    .await?
    {
        Ok(genesis_event) => genesis_event,
        Err(refusal) => {
            tx.rollback().await?;
            return Ok(CodingSessionAuthorityTransitionInsertOutcome::Refused { refusal });
        }
    };

    let others = stored_authority_transitions_tx(
        &mut tx,
        community_id,
        channel_id,
        &genesis_ref,
        id_bytes.as_slice(),
    )
    .await?;
    let current_head = others.iter().max_by_key(|t| t.seq);

    let expected_prev_accepted = current_head.map(|head| head.event_id.clone());
    let actual_prev_accepted = payload
        .prev_accepted
        .as_deref()
        .map(|hex_id| hex::decode(hex_id).unwrap_or_default());
    if actual_prev_accepted != expected_prev_accepted {
        tx.rollback().await?;
        return Ok(CodingSessionAuthorityTransitionInsertOutcome::Refused {
            refusal: AuthorityTransitionRefusal::StaleHead {
                expected_prev_accepted,
            },
        });
    }

    let expected_seq = current_head.map(|head| head.seq + 1).unwrap_or(1);
    if payload.seq != expected_seq {
        tx.rollback().await?;
        return Ok(CodingSessionAuthorityTransitionInsertOutcome::Refused {
            refusal: AuthorityTransitionRefusal::SeqMismatch { expected_seq },
        });
    }

    let owner_pubkey = current_coding_session_authority_owner(&genesis_event);
    if event.pubkey.as_bytes() != owner_pubkey.as_slice() {
        tx.rollback().await?;
        return Ok(CodingSessionAuthorityTransitionInsertOutcome::Refused {
            refusal: AuthorityTransitionRefusal::SignerNotOwner { owner_pubkey },
        });
    }

    // A `revoke` must name a pubkey with a live grant at its point in the
    // chain. Folding transitions with seq < payload.seq (rather than "all")
    // keeps resubmission idempotent: a replayed revoke recomputes the same
    // pre-state it was originally validated against, even though its own
    // application already emptied the grant.
    if payload.transition_type == CodingSessionAuthorityTransitionType::Revoke {
        let grants_before = fold_authority_grants(&others, payload.seq);
        if !grants_before.contains_key(&payload.grantee_pubkey) {
            tx.rollback().await?;
            return Ok(CodingSessionAuthorityTransitionInsertOutcome::Refused {
                refusal: AuthorityTransitionRefusal::NoSuchGrant,
            });
        }
    }

    let (stored_event, was_inserted) = insert_event_with_thread_metadata_tx(
        &mut tx,
        community_id,
        event,
        Some(channel_id),
        thread_meta,
    )
    .await?;

    // Maintain the grant ACL projection inside the same advisory-locked
    // transaction, and only for a genuinely new link — a replay
    // (`was_inserted == false`) already applied its mutation once.
    if was_inserted {
        let genesis_payload = decode_coding_session_genesis(&genesis_event.event.content)
            .map_err(DbError::InvalidData)?;
        let founder = current_coding_session_authority_owner(&genesis_event);
        let grantee = hex::decode(&payload.grantee_pubkey).map_err(|_| {
            DbError::InvalidData("granteePubkey failed hex decode after validation".into())
        })?;
        let genesis_id_bytes = genesis_event.event.id.as_bytes().to_vec();
        match payload.transition_type {
            CodingSessionAuthorityTransitionType::GrantOperator
            | CodingSessionAuthorityTransitionType::GrantViewer => {
                let role = if payload.transition_type
                    == CodingSessionAuthorityTransitionType::GrantOperator
                {
                    "operator"
                } else {
                    "viewer"
                };
                sqlx::query(
                    r#"
                    INSERT INTO coding_session_authority_acl
                        (community_id, channel_id, genesis_ref, session_ref, founder, grantee, role, granted_seq)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    ON CONFLICT (community_id, genesis_ref, grantee)
                        DO UPDATE SET role = EXCLUDED.role, granted_seq = EXCLUDED.granted_seq
                    "#,
                )
                .bind(community_id.as_uuid())
                .bind(channel_id)
                .bind(&genesis_id_bytes)
                .bind(&genesis_payload.session_ref)
                .bind(&founder)
                .bind(&grantee)
                .bind(role)
                .bind(payload.seq as i32)
                .execute(&mut *tx)
                .await?;
            }
            CodingSessionAuthorityTransitionType::Revoke => {
                sqlx::query(
                    "DELETE FROM coding_session_authority_acl \
                     WHERE community_id = $1 AND genesis_ref = $2 AND grantee = $3",
                )
                .bind(community_id.as_uuid())
                .bind(&genesis_id_bytes)
                .bind(&grantee)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    tx.commit().await?;

    Ok(CodingSessionAuthorityTransitionInsertOutcome::Accepted {
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

    /// Sign a Pulse-shaped event (kind 44240 + singleton `a` coordinate) at a
    /// fixed timestamp, so `a_tags` pushdown tests can control page order.
    fn make_a_tagged_event_at(
        keys: &Keys,
        kind: u16,
        coordinate: &str,
        content: &str,
        created_at: u64,
    ) -> nostr::Event {
        EventBuilder::new(Kind::Custom(kind), content)
            .tags(vec![Tag::parse(["a", coordinate]).expect("a tag")])
            .custom_created_at(nostr::Timestamp::from(created_at))
            .sign_with_keys(keys)
            .expect("sign a-tagged event")
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_tag_pushdown_scopes_both_sql_paths_to_the_requested_project() {
        let pool = setup_pool().await;
        let community_uuid = make_test_community(&pool).await;
        let community = CommunityId::from_uuid(community_uuid);
        let owner = Keys::generate();
        let owner_hex = owner.public_key().to_hex();
        let wanted = format!("30621:{owner_hex}:wanted");
        let other = format!("30621:{owner_hex}:other");
        let base = 1_800_000_100;

        let mine = make_a_tagged_event_at(&owner, 44_240, &wanted, "wanted project", base + 1);
        insert_event(&pool, community, &mine, None)
            .await
            .expect("insert wanted-project entry");
        // Newer rows for a different project: without the pushdown these
        // consume the page and starve the requested project's only entry.
        for i in 0..3 {
            let noise =
                make_a_tagged_event_at(&owner, 44_240, &other, "other project", base + 2 + i);
            insert_event(&pool, community, &noise, None)
                .await
                .expect("insert other-project entry");
        }
        let untagged = make_event_at(44_240, "no coordinate", base + 9);
        insert_event(&pool, community, &untagged, None)
            .await
            .expect("insert untagged entry");

        let events = query_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![44_240]),
                a_tags: Some(vec![wanted.clone()]),
                limit: Some(10),
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("query a-tag scoped page");
        assert_eq!(
            events.len(),
            1,
            "only the requested project's entry may be returned"
        );
        assert_eq!(events[0].event.id, mine.id);

        let counted = count_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![44_240]),
                a_tags: Some(vec![wanted.clone()]),
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("count a-tag scoped rows");
        assert_eq!(counted, 1, "the COUNT path must apply the same containment");

        let both = query_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![44_240]),
                a_tags: Some(vec![wanted.clone(), other.clone()]),
                limit: Some(10),
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("query multi-coordinate page");
        assert_eq!(both.len(), 4, "multiple coordinates OR together");

        let none = query_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![44_240]),
                a_tags: Some(vec![]),
                limit: Some(10),
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("query empty a_tags");
        assert!(none.is_empty(), "an empty #a list means match nothing");
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn git_gated_reader_excludes_pulse_entries_of_hidden_projects() {
        let pool = setup_pool().await;
        let community_uuid = make_test_community(&pool).await;
        let community = CommunityId::from_uuid(community_uuid);
        let author = Keys::generate();
        let author_hex = author.public_key().to_hex();
        let reader = Keys::generate();
        let hidden_coordinate = format!("30621:{author_hex}:secret");
        let open_coordinate = format!("30621:{author_hex}:open");
        let base = 1_800_000_200;

        let hidden_entry = make_a_tagged_event_at(
            &author,
            44_240,
            &hidden_coordinate,
            "private plan",
            base + 1,
        );
        insert_event(&pool, community, &hidden_entry, None)
            .await
            .expect("insert hidden-project entry");
        let open_entry =
            make_a_tagged_event_at(&author, 44_240, &open_coordinate, "public plan", base + 2);
        insert_event(&pool, community, &open_entry, None)
            .await
            .expect("insert open-project entry");

        let gate = |who: &Keys| crate::event::GitGatedReader {
            reader: who.public_key().to_bytes().to_vec(),
            hidden: crate::git_repo::HiddenRepos {
                coordinates: Vec::new(),
                names: std::collections::HashSet::new(),
                project_coordinates: std::collections::HashSet::from([hidden_coordinate.clone()]),
            },
        };

        let visible = query_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![44_240]),
                git_gated_reader: Some(gate(&reader)),
                limit: Some(10),
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("query as an outsider");
        assert_eq!(visible.len(), 1, "the hidden project's entry is withheld");
        assert_eq!(visible[0].event.id, open_entry.id);

        let as_author = query_events(
            &pool,
            &EventQuery {
                kinds: Some(vec![44_240]),
                git_gated_reader: Some(gate(&author)),
                limit: Some(10),
                ..EventQuery::for_community(community)
            },
        )
        .await
        .expect("query as the author");
        assert_eq!(
            as_author.len(),
            2,
            "an author always reads back their own entries"
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

    /// Build an explicit-adoption genesis event (R15): same three-tag
    /// envelope as [`make_genesis_event`], but content also carries `adopts`
    /// naming the founding create and its joining receipt by event id.
    fn make_genesis_adoption_event(
        keys: &Keys,
        channel_id: Uuid,
        session_ref: &str,
        create_event_id: &nostr::EventId,
        receipt_event_id: &nostr::EventId,
    ) -> nostr::Event {
        let channel = channel_id.to_string();
        let content = serde_json::json!({
            "sessionRef": session_ref,
            "v": 1,
            "adopts": {
                "createEventId": create_event_id.to_hex(),
                "receiptEventId": receipt_event_id.to_hex(),
            },
        })
        .to_string();
        EventBuilder::new(Kind::Custom(44226), content)
            .tags(vec![
                Tag::parse(["h", channel.as_str()]).expect("h tag"),
                Tag::parse(["csg-v", "csg1-1"]).expect("csg-v tag"),
                Tag::parse(["csg-session", session_ref]).expect("csg-session tag"),
            ])
            .sign_with_keys(keys)
            .expect("sign adoption genesis")
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

    // ---- Adoption of sessions that predate genesis (R8, R15, R16) ----------

    /// A signed `session.create` in the exact envelope the relay admits,
    /// claiming `session_ref` and addressed to `authority`.
    fn make_legacy_create(
        keys: &Keys,
        channel_id: Uuid,
        session_ref: &str,
        command_id: &str,
        authority: &Keys,
        created_at: u64,
    ) -> nostr::Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "sessionRef": session_ref,
                "providerInstanceRef": "instance-1",
                "providerAuthorityPubkey": authority.public_key().to_hex(),
                "model": null,
                "title": null,
                "initialTurn": null,
            },
        })
        .to_string();
        EventBuilder::new(Kind::Custom(44221), content)
            .tags(vec![
                Tag::parse(["h", &channel_id.to_string()]).expect("h tag"),
                Tag::parse(["csl-v", "csl1-1"]).expect("csl-v tag"),
                Tag::parse(["csl-command", command_id]).expect("csl-command tag"),
            ])
            .custom_created_at(nostr::Timestamp::from(created_at))
            .sign_with_keys(keys)
            .expect("sign create")
    }

    /// The provider's signed answer minting one execution for `command_id`,
    /// signed by `signer` (ordinarily the create's named authority; tests that
    /// want a non-joining receipt pass a different key).
    fn make_receipt_signed_by(signer: &Keys, channel_id: Uuid, command_id: &str) -> nostr::Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-receipt/v1",
            "commandId": command_id,
            "status": "created",
            "session": {
                "driver": "claude",
                "instanceId": "instance-1",
                "sessionId": "session-1",
                "generation": 1,
            },
            "error": null,
        })
        .to_string();
        EventBuilder::new(Kind::Custom(44224), content)
            .tags(vec![
                Tag::parse(["h", &channel_id.to_string()]).expect("h tag"),
                Tag::parse(["cslr-v", "cslr1-1"]).expect("cslr-v tag"),
                Tag::parse(["csl-command", command_id]).expect("csl-command tag"),
            ])
            .sign_with_keys(signer)
            .expect("sign receipt")
    }

    fn make_receipt(authority: &Keys, channel_id: Uuid, command_id: &str) -> nostr::Event {
        make_receipt_signed_by(authority, channel_id, command_id)
    }

    /// R15 step 2(c) asks whether a receipt *minted* the create's session.
    /// Kind 44224 also carries the per-turn vocabulary, and every turn status
    /// names an execution in `session` while proving nothing about a create —
    /// `turn_refused` proves the provider has no such session. A turn
    /// command's `commandId` is chosen by whoever signs the 44220, so a turn
    /// receipt echoing a create's `commandId` is attacker-reachable: it must
    /// never join a create.
    #[test]
    fn only_a_minting_receipt_joins_a_create() {
        let authority = Keys::generate();
        let create = LegacySessionCreate {
            event_id: vec![1_u8; 32],
            signer: vec![2_u8; 32],
            command_id: "command-1".to_owned(),
            session_ref: Some("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned()),
            provider_authority_pubkey: authority.public_key().to_hex(),
        };
        let signer = authority.public_key().to_bytes().to_vec();

        let content = |status: &str, turn_id: Option<&str>| {
            let mut body = serde_json::json!({
                "schema": "buzz-coding-session-lifecycle-receipt/v1",
                "commandId": create.command_id,
                "status": status,
                "session": {
                    "driver": "claude",
                    "instanceId": "instance-1",
                    "sessionId": "session-1",
                    "generation": 1,
                },
                "error": serde_json::Value::Null,
            });
            if let Some(turn_id) = turn_id {
                body["turnId"] = serde_json::Value::String(turn_id.to_owned());
            }
            body.to_string()
        };

        for status in [
            "created",
            "created_with_failed_initial_turn",
            "resumed",
            "resumed_without_context",
        ] {
            assert!(
                receipt_joins_create(&create, &signer, &content(status, None)),
                "{status} mints an execution and must join its create"
            );
        }

        for (status, turn_id) in [
            ("turn_queued", None),
            ("turn_started", Some("turn-1")),
            ("turn_dropped", None),
            ("turn_refused", None),
        ] {
            assert!(
                !receipt_joins_create(&create, &signer, &content(status, turn_id)),
                "{status} answers a 44220 turn, not this create"
            );
        }

        // The signer check still bites first: a minting receipt from anyone
        // other than the named authority joins nothing.
        let impostor = Keys::generate().public_key().to_bytes().to_vec();
        assert!(!receipt_joins_create(
            &create,
            &impostor,
            &content("created", None)
        ));
    }

    /// Store a legacy founding create + its joining receipt, returning their
    /// event ids for an `adopts` reference.
    async fn store_founding_pair(
        pool: &PgPool,
        community: CommunityId,
        channel: Uuid,
        session_ref: &str,
        founder: &Keys,
        authority: &Keys,
        command_id: &str,
    ) -> (nostr::EventId, nostr::EventId) {
        let create = make_legacy_create(
            founder,
            channel,
            session_ref,
            command_id,
            authority,
            1_700_000_000,
        );
        insert_event(pool, community, &create, Some(channel))
            .await
            .expect("store founding create");
        let receipt = make_receipt(authority, channel, command_id);
        insert_event(pool, community, &receipt, Some(channel))
            .await
            .expect("store founding receipt");
        (create.id, receipt.id)
    }

    /// A genesis without `adopts` over a `sessionRef` that legacy history
    /// already uses must be refused — regardless of who is asking, including
    /// the true founder. R15 removed the relay's founder-projection
    /// machinery entirely, so there is no longer an identity this path can
    /// verify; the only way through is the explicit `adopts` reference.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_without_adopts_is_refused_when_legacy_history_exists() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let (create_id, _receipt_id) = store_founding_pair(
            &pool,
            community,
            channel,
            &session_ref,
            &founder,
            &authority,
            "cmd-1",
        )
        .await;

        for (label, claimant) in [
            ("outsider", Keys::generate()),
            ("true founder", founder.clone()),
        ] {
            let genesis = make_genesis_event(&claimant, channel, &session_ref);
            match insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
                .await
                .unwrap_or_else(|e| panic!("{label} claim: {e}"))
            {
                CodingSessionGenesisInsertOutcome::AdoptionRefused {
                    refusal:
                        GenesisAdoptionRefusal::LegacyHistoryRequiresAdoption {
                            existing_create_event_id,
                        },
                } => {
                    assert_eq!(
                        existing_create_event_id,
                        create_id.as_bytes().as_slice(),
                        "{label}: the refusal must point at a create to adopt"
                    );
                }
                other => panic!(
                    "{label}: a bare genesis over legacy history must be refused, got {other:?}"
                ),
            }
            assert!(
                get_event_by_id(&pool, community, genesis.id.as_bytes())
                    .await
                    .expect("look up refused genesis")
                    .is_none(),
                "{label}: a refused genesis must leave nothing behind"
            );
        }
    }

    /// The way through: an explicit `adopts` reference to the founding create
    /// and its joining receipt. The adopted genesis is an ordinary genesis
    /// afterwards — it holds the reference against everyone else exactly as a
    /// fresh one would.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_explicit_adoption_by_the_referenced_founder_is_accepted() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let (create_id, receipt_id) = store_founding_pair(
            &pool,
            community,
            channel,
            &session_ref,
            &founder,
            &authority,
            "cmd-1",
        )
        .await;

        let adoption =
            make_genesis_adoption_event(&founder, channel, &session_ref, &create_id, &receipt_id);
        assert!(
            matches!(
                insert_coding_session_genesis_event(&pool, community, &adoption, channel, None)
                    .await
                    .expect("adopt"),
                CodingSessionGenesisInsertOutcome::Founded {
                    was_inserted: true,
                    ..
                }
            ),
            "an adoption referencing the true founding pair, signed by that founder, must succeed"
        );

        let rival = make_genesis_event(&Keys::generate(), channel, &session_ref);
        match insert_coding_session_genesis_event(&pool, community, &rival, channel, None)
            .await
            .expect("rival after adoption")
        {
            CodingSessionGenesisInsertOutcome::AlreadyFounded { existing_event_id } => {
                assert_eq!(existing_event_id, adoption.id.as_bytes().as_slice());
            }
            other => panic!("an adopted genesis holds its reference, got {other:?}"),
        }
    }

    /// R15 step 2(a): a `createEventId` naming nothing this relay has stored
    /// is refused, never treated as a fresh founding.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_explicit_adoption_is_refused_when_create_is_missing() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let receipt = make_receipt(&authority, channel, "cmd-1");
        insert_event(&pool, community, &receipt, Some(channel))
            .await
            .expect("store receipt");

        let nonexistent_create = nostr::EventId::from_hex(&"ab".repeat(32)).expect("event id");
        let genesis = make_genesis_adoption_event(
            &founder,
            channel,
            &session_ref,
            &nonexistent_create,
            &receipt.id,
        );
        match insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
            .await
            .expect("adoption over a missing create")
        {
            CodingSessionGenesisInsertOutcome::AdoptionRefused {
                refusal: GenesisAdoptionRefusal::ReferencedCreateNotFound,
            } => {}
            other => panic!("a missing create must refuse, got {other:?}"),
        }
    }

    /// R15 step 2(a): a `receiptEventId` naming nothing this relay has stored
    /// is refused.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_explicit_adoption_is_refused_when_receipt_is_missing() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let create = make_legacy_create(
            &founder,
            channel,
            &session_ref,
            "cmd-1",
            &authority,
            1_700_000_000,
        );
        insert_event(&pool, community, &create, Some(channel))
            .await
            .expect("store create");

        let nonexistent_receipt = nostr::EventId::from_hex(&"cd".repeat(32)).expect("event id");
        let genesis = make_genesis_adoption_event(
            &founder,
            channel,
            &session_ref,
            &create.id,
            &nonexistent_receipt,
        );
        match insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
            .await
            .expect("adoption over a missing receipt")
        {
            CodingSessionGenesisInsertOutcome::AdoptionRefused {
                refusal: GenesisAdoptionRefusal::ReferencedReceiptNotFound,
            } => {}
            other => panic!("a missing receipt must refuse, got {other:?}"),
        }
    }

    /// R15 step 2(b): a receipt that exists but was not signed by the
    /// create's named provider authority does not "genuinely join" it — a
    /// bystander's receipt for an unrelated command cannot be repurposed as
    /// adoption evidence.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_explicit_adoption_is_refused_when_receipt_does_not_join_create() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let create = make_legacy_create(
            &founder,
            channel,
            &session_ref,
            "cmd-1",
            &authority,
            1_700_000_000,
        );
        insert_event(&pool, community, &create, Some(channel))
            .await
            .expect("store create");

        // Signed by someone other than the create's named authority — a
        // structurally valid receipt that simply does not answer this create.
        let impostor = Keys::generate();
        let unjoined_receipt = make_receipt_signed_by(&impostor, channel, "cmd-1");
        insert_event(&pool, community, &unjoined_receipt, Some(channel))
            .await
            .expect("store unjoined receipt");

        let genesis = make_genesis_adoption_event(
            &founder,
            channel,
            &session_ref,
            &create.id,
            &unjoined_receipt.id,
        );
        match insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
            .await
            .expect("adoption over an unjoined receipt")
        {
            CodingSessionGenesisInsertOutcome::AdoptionRefused {
                refusal: GenesisAdoptionRefusal::ReceiptDoesNotJoinCreate,
            } => {}
            other => panic!("a non-joining receipt must refuse, got {other:?}"),
        }
    }

    /// R15 step 2(c): the referenced create must bear the genesis's own
    /// `sessionRef` — a valid create/receipt pair for a *different* umbrella
    /// cannot be repointed at this one.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_explicit_adoption_is_refused_on_session_ref_mismatch() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let create_session_ref = Uuid::new_v4().to_string();
        let genesis_session_ref = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let (create_id, receipt_id) = store_founding_pair(
            &pool,
            community,
            channel,
            &create_session_ref,
            &founder,
            &authority,
            "cmd-1",
        )
        .await;

        let genesis = make_genesis_adoption_event(
            &founder,
            channel,
            &genesis_session_ref,
            &create_id,
            &receipt_id,
        );
        match insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
            .await
            .expect("adoption with a mismatched sessionRef")
        {
            CodingSessionGenesisInsertOutcome::AdoptionRefused {
                refusal: GenesisAdoptionRefusal::SessionRefMismatch,
            } => {}
            other => panic!("a sessionRef mismatch must refuse, got {other:?}"),
        }
    }

    /// R15 step 2(d): the genesis signer must equal the create's signer — a
    /// correctly-joined create/receipt pair does not let a *different* signer
    /// borrow its founder status.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_explicit_adoption_is_refused_on_signer_mismatch() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let (create_id, receipt_id) = store_founding_pair(
            &pool,
            community,
            channel,
            &session_ref,
            &founder,
            &authority,
            "cmd-1",
        )
        .await;

        let impostor = Keys::generate();
        let genesis =
            make_genesis_adoption_event(&impostor, channel, &session_ref, &create_id, &receipt_id);
        match insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
            .await
            .expect("adoption signed by a non-founder")
        {
            CodingSessionGenesisInsertOutcome::AdoptionRefused {
                refusal: GenesisAdoptionRefusal::SignerMismatch { founder_pubkey },
            } => {
                assert_eq!(founder_pubkey, founder.public_key().to_bytes().to_vec());
            }
            other => panic!("a signer mismatch must refuse, got {other:?}"),
        }
    }

    /// R15 step 2(e): a create/receipt pair genuinely joined and correctly
    /// claiming this `sessionRef` still cannot found a genesis in a
    /// *different* channel — the events must sit in the genesis's own
    /// channel.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_explicit_adoption_is_refused_when_events_are_in_the_wrong_channel() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let create_channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let genesis_channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let (create_id, receipt_id) = store_founding_pair(
            &pool,
            community,
            create_channel,
            &session_ref,
            &founder,
            &authority,
            "cmd-1",
        )
        .await;

        let genesis = make_genesis_adoption_event(
            &founder,
            genesis_channel,
            &session_ref,
            &create_id,
            &receipt_id,
        );
        match insert_coding_session_genesis_event(&pool, community, &genesis, genesis_channel, None)
            .await
            .expect("adoption across channels")
        {
            CodingSessionGenesisInsertOutcome::AdoptionRefused {
                refusal: GenesisAdoptionRefusal::WrongChannel,
            } => {}
            other => panic!("cross-channel evidence must refuse, got {other:?}"),
        }
    }

    /// R16 (ambiguity parity): the founding `commandId` is shared by two
    /// creates in the channel that agree on signer but disagree on
    /// `sessionRef`. The prior `sessionRef`-first scoping made the second
    /// create invisible to this check; scanning by `commandId` alone (as the
    /// desktop's `codingSessionCreateObservations.ts` does) catches it.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_explicit_adoption_is_refused_on_cross_session_ref_command_id_disagreement() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref_a = Uuid::new_v4().to_string();
        let session_ref_b = Uuid::new_v4().to_string();

        let founder = Keys::generate();
        let authority = Keys::generate();
        let shared_command_id = "cmd-shared";

        // Same signer, same commandId, two different sessionRef claims.
        let create_a = make_legacy_create(
            &founder,
            channel,
            &session_ref_a,
            shared_command_id,
            &authority,
            1_700_000_000,
        );
        insert_event(&pool, community, &create_a, Some(channel))
            .await
            .expect("store create A");
        let create_b = make_legacy_create(
            &founder,
            channel,
            &session_ref_b,
            shared_command_id,
            &authority,
            1_700_000_100,
        );
        insert_event(&pool, community, &create_b, Some(channel))
            .await
            .expect("store create B");
        let receipt = make_receipt(&authority, channel, shared_command_id);
        insert_event(&pool, community, &receipt, Some(channel))
            .await
            .expect("store receipt");

        let genesis = make_genesis_adoption_event(
            &founder,
            channel,
            &session_ref_a,
            &create_a.id,
            &receipt.id,
        );
        match insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
            .await
            .expect("adoption over a cross-sessionRef-disputed command")
        {
            CodingSessionGenesisInsertOutcome::AdoptionRefused {
                refusal: GenesisAdoptionRefusal::CommandIdAmbiguous { reason },
            } => {
                assert!(
                    reason.contains("sessionRef"),
                    "the refusal must say sessionRefs disagreed, got {reason:?}"
                );
            }
            other => panic!(
                "a commandId shared by two conflicting sessionRef claims must refuse, got {other:?}"
            ),
        }
    }

    /// A create for *another* umbrella in the same channel must not block a
    /// fresh genesis, and neither must one whose content merely mentions the
    /// reference without claiming it. The `LIKE` in the probe is a bandwidth
    /// prefilter; the strict decode is the test.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn genesis_is_unaffected_by_unrelated_create_history() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let session_ref = Uuid::new_v4().to_string();
        let authority = Keys::generate();

        // Same channel, different umbrella.
        let other = make_legacy_create(
            &Keys::generate(),
            channel,
            &Uuid::new_v4().to_string(),
            "cmd-other",
            &authority,
            1_700_000_000,
        );
        insert_event(&pool, community, &other, Some(channel))
            .await
            .expect("store unrelated create");

        // Contains the reference in its bytes, but claims no umbrella at all.
        let mentions = EventBuilder::new(
            Kind::Custom(44221),
            serde_json::json!({
                "schema": "buzz-coding-session-lifecycle-command/v1",
                "commandId": "cmd-mentions",
                "action": {
                    "type": "session.create",
                    "projectRef": null,
                    "repoRef": null,
                    "sessionRef": null,
                    "providerInstanceRef": "instance-1",
                    "providerAuthorityPubkey": authority.public_key().to_hex(),
                    "model": null,
                    "title": format!("about {session_ref}"),
                    "initialTurn": null,
                },
            })
            .to_string(),
        )
        .tags(vec![
            Tag::parse(["h", &channel.to_string()]).expect("h tag"),
            Tag::parse(["csl-v", "csl1-1"]).expect("csl-v tag"),
            Tag::parse(["csl-command", "cmd-mentions"]).expect("csl-command tag"),
        ])
        .sign_with_keys(&Keys::generate())
        .expect("sign mention-only create");
        insert_event(&pool, community, &mentions, Some(channel))
            .await
            .expect("store mention-only create");

        let genesis = make_genesis_event(&Keys::generate(), channel, &session_ref);
        assert!(
            matches!(
                insert_coding_session_genesis_event(&pool, community, &genesis, channel, None)
                    .await
                    .expect("found a fresh session"),
                CodingSessionGenesisInsertOutcome::Founded {
                    was_inserted: true,
                    ..
                }
            ),
            "history that does not claim this reference must not govern it"
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
                    other => panic!(
                        "round {round}: a fresh reference has no create history, got {other:?}"
                    ),
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

    // ---- Coding-session authority chain (kind 44228) -----------------------

    /// Build an authority-transition event in the exact envelope the relay's
    /// validator admits, signed by `keys`.
    fn make_authority_transition_event(
        keys: &Keys,
        channel_id: Uuid,
        genesis_ref: &str,
        prev_accepted: Option<&str>,
        seq: u32,
        grantee_pubkey: &str,
    ) -> nostr::Event {
        let channel = channel_id.to_string();
        let prev_json = match prev_accepted {
            Some(prev) => format!("\"{prev}\""),
            None => "null".to_owned(),
        };
        let content = format!(
            r#"{{"genesisRef":"{genesis_ref}","prevAccepted":{prev_json},"seq":{seq},"type":"grant-operator","granteePubkey":"{grantee_pubkey}"}}"#
        );
        EventBuilder::new(Kind::Custom(44228), content)
            .tags(vec![
                Tag::parse(["h", channel.as_str()]).expect("h tag"),
                Tag::parse(["csat-v", "csat1-1"]).expect("csat-v tag"),
                Tag::parse(["csat-genesis", genesis_ref]).expect("csat-genesis tag"),
            ])
            .sign_with_keys(keys)
            .expect("sign authority transition")
    }

    /// Found a fresh session in `channel`, signed by `founder`, and return the
    /// stored genesis event. Panics (via `expect`/`assert`) on any outcome
    /// other than a clean founding — every authority-transition test starts
    /// from a genesis it can trust is real.
    async fn found_genesis_for_authority_tests(
        pool: &PgPool,
        community: CommunityId,
        channel: Uuid,
        founder: &Keys,
    ) -> nostr::Event {
        let session_ref = Uuid::new_v4().to_string();
        let genesis = make_genesis_event(founder, channel, &session_ref);
        let outcome = insert_coding_session_genesis_event(pool, community, &genesis, channel, None)
            .await
            .expect("found session for authority-transition test");
        assert!(matches!(
            outcome,
            CodingSessionGenesisInsertOutcome::Founded {
                was_inserted: true,
                ..
            }
        ));
        genesis
    }

    /// The chain's first link: `prevAccepted: null`, `seq: 1`, signed by the
    /// founder (today's owner).
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn authority_transition_first_grant_is_accepted() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let founder = Keys::generate();
        let genesis = found_genesis_for_authority_tests(&pool, community, channel, &founder).await;
        let grantee = Keys::generate().public_key().to_hex();

        let transition = make_authority_transition_event(
            &founder,
            channel,
            &genesis.id.to_hex(),
            None,
            1,
            &grantee,
        );
        let outcome = insert_coding_session_authority_transition_event(
            &pool,
            community,
            &transition,
            channel,
            None,
        )
        .await
        .expect("first grant");
        match outcome {
            CodingSessionAuthorityTransitionInsertOutcome::Accepted {
                was_inserted,
                stored_event,
            } => {
                assert!(was_inserted);
                assert_eq!(stored_event.event.id, transition.id);
            }
            other => panic!("first grant must be accepted, got {other:?}"),
        }
    }

    /// A second transition that correctly names the first as `prevAccepted`
    /// and increments `seq` extends the chain.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn authority_transition_second_grant_with_correct_linkage_is_accepted() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let founder = Keys::generate();
        let genesis = found_genesis_for_authority_tests(&pool, community, channel, &founder).await;
        let genesis_ref = genesis.id.to_hex();

        let first = make_authority_transition_event(
            &founder,
            channel,
            &genesis_ref,
            None,
            1,
            &Keys::generate().public_key().to_hex(),
        );
        insert_coding_session_authority_transition_event(&pool, community, &first, channel, None)
            .await
            .expect("first grant");

        let second = make_authority_transition_event(
            &founder,
            channel,
            &genesis_ref,
            Some(&first.id.to_hex()),
            2,
            &Keys::generate().public_key().to_hex(),
        );
        let outcome = insert_coding_session_authority_transition_event(
            &pool, community, &second, channel, None,
        )
        .await
        .expect("second grant");
        match outcome {
            CodingSessionAuthorityTransitionInsertOutcome::Accepted { was_inserted, .. } => {
                assert!(was_inserted);
            }
            other => panic!("second grant must be accepted, got {other:?}"),
        }
    }

    /// A transition naming anything other than the chain's actual current
    /// head — even with the numerically correct next `seq` — is refused. This
    /// is the serialization-point property the whole bite exists to prove.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn authority_transition_with_stale_prev_accepted_is_rejected() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let founder = Keys::generate();
        let genesis = found_genesis_for_authority_tests(&pool, community, channel, &founder).await;
        let genesis_ref = genesis.id.to_hex();

        let first = make_authority_transition_event(
            &founder,
            channel,
            &genesis_ref,
            None,
            1,
            &Keys::generate().public_key().to_hex(),
        );
        insert_coding_session_authority_transition_event(&pool, community, &first, channel, None)
            .await
            .expect("first grant");

        // Correct next seq (2), but a prevAccepted that names something other
        // than the real head (the genesis id, not the first transition's id).
        let stale = make_authority_transition_event(
            &founder,
            channel,
            &genesis_ref,
            Some(&genesis_ref),
            2,
            &Keys::generate().public_key().to_hex(),
        );
        let outcome = insert_coding_session_authority_transition_event(
            &pool, community, &stale, channel, None,
        )
        .await
        .expect("stale transition attempt");
        match outcome {
            CodingSessionAuthorityTransitionInsertOutcome::Refused {
                refusal:
                    AuthorityTransitionRefusal::StaleHead {
                        expected_prev_accepted,
                    },
            } => {
                assert_eq!(
                    expected_prev_accepted,
                    Some(first.id.as_bytes().to_vec()),
                    "the refusal must name the real head"
                );
            }
            other => panic!("stale prevAccepted must be refused, got {other:?}"),
        }
    }

    /// The numeric twin of the stale-head case: correct `prevAccepted`, wrong
    /// `seq`.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn authority_transition_with_wrong_seq_is_rejected() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let founder = Keys::generate();
        let genesis = found_genesis_for_authority_tests(&pool, community, channel, &founder).await;
        let genesis_ref = genesis.id.to_hex();

        let first = make_authority_transition_event(
            &founder,
            channel,
            &genesis_ref,
            None,
            1,
            &Keys::generate().public_key().to_hex(),
        );
        insert_coding_session_authority_transition_event(&pool, community, &first, channel, None)
            .await
            .expect("first grant");

        let wrong_seq = make_authority_transition_event(
            &founder,
            channel,
            &genesis_ref,
            Some(&first.id.to_hex()),
            5,
            &Keys::generate().public_key().to_hex(),
        );
        let outcome = insert_coding_session_authority_transition_event(
            &pool, community, &wrong_seq, channel, None,
        )
        .await
        .expect("wrong-seq transition attempt");
        match outcome {
            CodingSessionAuthorityTransitionInsertOutcome::Refused {
                refusal: AuthorityTransitionRefusal::SeqMismatch { expected_seq },
            } => {
                assert_eq!(expected_seq, 2);
            }
            other => panic!("wrong seq must be refused, got {other:?}"),
        }
    }

    /// Only the session's owner (today: the genesis signer) may extend the
    /// chain — a well-linked transition from anyone else is still refused.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn authority_transition_from_a_non_owner_signer_is_rejected() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let founder = Keys::generate();
        let impostor = Keys::generate();
        let genesis = found_genesis_for_authority_tests(&pool, community, channel, &founder).await;

        let transition = make_authority_transition_event(
            &impostor,
            channel,
            &genesis.id.to_hex(),
            None,
            1,
            &Keys::generate().public_key().to_hex(),
        );
        let outcome = insert_coding_session_authority_transition_event(
            &pool,
            community,
            &transition,
            channel,
            None,
        )
        .await
        .expect("non-owner transition attempt");
        match outcome {
            CodingSessionAuthorityTransitionInsertOutcome::Refused {
                refusal: AuthorityTransitionRefusal::SignerNotOwner { owner_pubkey },
            } => {
                assert_eq!(owner_pubkey, founder.public_key().to_bytes().to_vec());
            }
            other => panic!("non-owner signer must be refused, got {other:?}"),
        }
    }

    /// A `genesisRef` that names no stored genesis at all is refused —
    /// nothing to root the chain at.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn authority_transition_against_an_unknown_genesis_is_rejected() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let signer = Keys::generate();
        let fabricated_genesis_ref = "ab".repeat(32);

        let transition = make_authority_transition_event(
            &signer,
            channel,
            &fabricated_genesis_ref,
            None,
            1,
            &Keys::generate().public_key().to_hex(),
        );
        let outcome = insert_coding_session_authority_transition_event(
            &pool,
            community,
            &transition,
            channel,
            None,
        )
        .await
        .expect("unknown-genesis transition attempt");
        assert!(matches!(
            outcome,
            CodingSessionAuthorityTransitionInsertOutcome::Refused {
                refusal: AuthorityTransitionRefusal::GenesisNotFound,
            }
        ));
    }

    /// A genesis founded in one channel cannot anchor a transition published
    /// in a different one — the channel is part of the reference's identity,
    /// exactly as it is for genesis itself.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn authority_transition_against_a_cross_channel_genesis_is_rejected() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let home_channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let other_channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let founder = Keys::generate();
        let genesis =
            found_genesis_for_authority_tests(&pool, community, home_channel, &founder).await;

        let transition = make_authority_transition_event(
            &founder,
            other_channel,
            &genesis.id.to_hex(),
            None,
            1,
            &Keys::generate().public_key().to_hex(),
        );
        let outcome = insert_coding_session_authority_transition_event(
            &pool,
            community,
            &transition,
            other_channel,
            None,
        )
        .await
        .expect("cross-channel transition attempt");
        assert!(matches!(
            outcome,
            CodingSessionAuthorityTransitionInsertOutcome::Refused {
                refusal: AuthorityTransitionRefusal::WrongChannel,
            }
        ));
    }

    /// Replaying the exact same signed transition stays idempotent — the same
    /// resubmission guarantee genesis gives, and for the same reason (a
    /// dropped `OK` must not permanently look like someone else's chain).
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn authority_transition_resubmission_of_the_same_event_stays_idempotent() {
        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;
        let founder = Keys::generate();
        let genesis = found_genesis_for_authority_tests(&pool, community, channel, &founder).await;

        let transition = make_authority_transition_event(
            &founder,
            channel,
            &genesis.id.to_hex(),
            None,
            1,
            &Keys::generate().public_key().to_hex(),
        );

        for expected_insert in [true, false] {
            let outcome = insert_coding_session_authority_transition_event(
                &pool,
                community,
                &transition,
                channel,
                None,
            )
            .await
            .expect("resubmit transition");
            match outcome {
                CodingSessionAuthorityTransitionInsertOutcome::Accepted {
                    was_inserted, ..
                } => {
                    assert_eq!(was_inserted, expected_insert);
                }
                other => {
                    panic!("a replay of an accepted transition is not a refusal, got {other:?}")
                }
            }
        }
    }

    /// The claim this slice actually has to earn, mirrored from genesis's own
    /// concurrency proof: four rival transitions, each claiming to be the
    /// chain's first link, released simultaneously by a barrier. Exactly one
    /// may commit — the advisory lock is the serialization point, not a
    /// convention every caller has to honor.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires Postgres"]
    async fn authority_transition_concurrent_rivals_resolve_to_exactly_one_winner() {
        const RIVALS: usize = 4;
        const ROUNDS: usize = 8;

        let pool = setup_pool().await;
        let community = CommunityId::from_uuid(make_test_community(&pool).await);
        let channel = make_test_channel(&pool, *community.as_uuid(), None).await;

        for round in 0..ROUNDS {
            let founder = Keys::generate();
            let genesis =
                found_genesis_for_authority_tests(&pool, community, channel, &founder).await;
            let genesis_ref = genesis.id.to_hex();

            let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(RIVALS));
            let mut claims = Vec::with_capacity(RIVALS);

            for _ in 0..RIVALS {
                let rival = make_authority_transition_event(
                    &founder,
                    channel,
                    &genesis_ref,
                    None,
                    1,
                    &Keys::generate().public_key().to_hex(),
                );
                let pool = pool.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                claims.push(tokio::spawn(async move {
                    drop(pool.acquire().await.expect("warm a pooled connection"));
                    barrier.wait().await;
                    let outcome = insert_coding_session_authority_transition_event(
                        &pool, community, &rival, channel, None,
                    )
                    .await
                    .expect("rival transition");
                    (rival.id.as_bytes().to_vec(), outcome)
                }));
            }

            let mut winners = Vec::new();
            let mut refused = 0usize;
            for claim in claims {
                let (event_id, outcome) = claim.await.expect("join rival transition");
                match outcome {
                    CodingSessionAuthorityTransitionInsertOutcome::Accepted {
                        was_inserted,
                        ..
                    } => {
                        assert!(was_inserted, "round {round}: distinct rivals are new rows");
                        winners.push(event_id);
                    }
                    CodingSessionAuthorityTransitionInsertOutcome::Refused {
                        refusal: AuthorityTransitionRefusal::StaleHead { .. },
                    } => {
                        refused += 1;
                    }
                    other => panic!(
                        "round {round}: a losing rival must be refused as a stale head, got {other:?}"
                    ),
                }
            }

            assert_eq!(
                winners.len(),
                1,
                "round {round}: exactly one rival may extend the chain, got {} winners",
                winners.len()
            );
            assert_eq!(refused, RIVALS - 1, "round {round}");
        }
    }
}
