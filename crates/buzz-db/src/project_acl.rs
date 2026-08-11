//! Project access-control projection (NIP-MP Buzz access extension).
//!
//! Store+project projection of `kind:30621` project heads, keyed by
//! `(community_id, owner, dtag)` — the same store+project pattern as reactions.
//! The signed event stays authoritative; these rows let the accessible-channels
//! query ([`crate::channel::get_accessible_channel_ids`]) and the ingest
//! write-path membership check resolve "is this channel inside a private
//! project the reader cannot see" inside one SQL statement. Maintained during
//! ingest of 30621 heads and NIP-09 coordinate deletions; backfilled by
//! `migrations/0030_project_acl.sql`.

use sqlx::PgPool;

use crate::error::Result;
use crate::CommunityId;

/// Upsert the ACL row for a freshly-ingested 30621 head and replace its
/// invited-member set from the head's `p` tags.
///
/// Republish-latest: a newer head (greater `head_created_at`) wins; a stale or
/// out-of-order replay is ignored by the `head_created_at` guard. Members are
/// replaced only when the head actually applied, so a replayed old head can
/// never resurrect a revoked invite. Idempotent — re-applying the same head is
/// a no-op ending in the same row and member set.
pub async fn upsert_project_acl(
    pool: &PgPool,
    community: CommunityId,
    owner: &[u8],
    dtag: &str,
    visibility: &str,
    member_pubkeys: &[Vec<u8>],
    head_created_at: i64,
) -> Result<()> {
    let coordinate = format!("30621:{}:{}", hex::encode(owner), dtag);
    let mut tx = pool.begin().await?;
    let applied = sqlx::query(
        r#"
        INSERT INTO project_acl (community_id, owner, dtag, coordinate, visibility, head_created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        ON CONFLICT (community_id, owner, dtag) DO UPDATE SET
            coordinate      = EXCLUDED.coordinate,
            visibility      = EXCLUDED.visibility,
            head_created_at = EXCLUDED.head_created_at,
            updated_at      = NOW()
        WHERE EXCLUDED.head_created_at >= project_acl.head_created_at
        "#,
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(dtag)
    .bind(&coordinate)
    .bind(visibility)
    .bind(head_created_at)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if applied > 0 {
        sqlx::query(
            "DELETE FROM project_acl_members WHERE community_id = $1 AND owner = $2 AND dtag = $3",
        )
        .bind(community.as_uuid())
        .bind(owner)
        .bind(dtag)
        .execute(&mut *tx)
        .await?;
        for pubkey in member_pubkeys {
            sqlx::query(
                r#"
                INSERT INTO project_acl_members (community_id, owner, dtag, pubkey)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(community.as_uuid())
            .bind(owner)
            .bind(dtag)
            .bind(pubkey)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Drop the ACL row (and, via CASCADE, its members) for a NIP-09-deleted
/// project coordinate. Contents gated by the project revert to their own
/// access rules — a channel whose `project_ref` no longer resolves is public.
///
/// Scoped to heads at or before the deletion's `created_at`, mirroring
/// `soft_delete_by_coordinate`: a stale tombstone must not erase the ACL of a
/// newer replacement head. Returns `true` if a row was removed.
pub async fn delete_project_acl(
    pool: &PgPool,
    community: CommunityId,
    owner: &[u8],
    dtag: &str,
    deleted_at: i64,
) -> Result<bool> {
    let deleted = sqlx::query(
        r#"
        DELETE FROM project_acl
        WHERE community_id = $1 AND owner = $2 AND dtag = $3 AND head_created_at <= $4
        "#,
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(dtag)
    .bind(deleted_at)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(deleted > 0)
}

/// The resolved gate of one private project: its owner plus invited members.
///
/// Loaded per channel by [`get_channel_project_gate`] and cached relay-side so
/// live fan-out can filter recipients in memory without per-recipient DB hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGate {
    /// The project signer — always an implicit member.
    pub owner: Vec<u8>,
    /// Invited-member pubkeys (the head's `p` tags).
    pub members: Vec<Vec<u8>>,
}

impl ProjectGate {
    /// Returns `true` if `pubkey` is the project owner or an invited member.
    pub fn admits(&self, pubkey: &[u8]) -> bool {
        self.owner == pubkey || self.members.iter().any(|m| m == pubkey)
    }
}

/// Resolve the private-project gate of a channel, if it has one.
///
/// Returns `None` when the channel does not exist, has no `project_ref`, or
/// its project is unknown or public — all meaning "no gate". Used by live
/// fan-out and the ingest write path; the historical read path pushes the
/// equivalent predicate into [`crate::channel::get_accessible_channel_ids`].
pub async fn get_channel_project_gate(
    pool: &PgPool,
    community: CommunityId,
    channel_id: uuid::Uuid,
) -> Result<Option<ProjectGate>> {
    let row: Option<(Vec<u8>, Vec<Vec<u8>>)> = sqlx::query_as(
        r#"
        SELECT pa.owner,
               COALESCE(
                   array_agg(pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS members
        FROM channels c
        JOIN project_acl pa
          ON pa.community_id = c.community_id
         AND pa.coordinate = c.project_ref
         AND pa.visibility = 'private'
        LEFT JOIN project_acl_members pam
          ON pam.community_id = pa.community_id
         AND pam.owner = pa.owner
         AND pam.dtag = pa.dtag
        WHERE c.community_id = $1 AND c.id = $2 AND c.deleted_at IS NULL
        GROUP BY pa.owner
        "#,
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(owner, members)| ProjectGate { owner, members }))
}

/// Resolve the gate of the **private** project at `coordinate`, or `None`
/// when the coordinate is unknown or the project is public ("no gate").
///
/// Used by live fan-out for a 30617 announcement's own `project` tag —
/// the defense-in-depth path that still gates an announcement whose
/// `git_repo_names` projection never landed.
pub async fn get_project_gate_by_coordinate(
    pool: &PgPool,
    community: CommunityId,
    coordinate: &str,
) -> Result<Option<ProjectGate>> {
    let row: Option<(Vec<u8>, Vec<Vec<u8>>)> = sqlx::query_as(
        r#"
        SELECT pa.owner,
               COALESCE(
                   array_agg(pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS members
        FROM project_acl pa
        LEFT JOIN project_acl_members pam
          ON pam.community_id = pa.community_id
         AND pam.owner = pa.owner
         AND pam.dtag = pa.dtag
        WHERE pa.community_id = $1 AND pa.coordinate = $2 AND pa.visibility = 'private'
        GROUP BY pa.owner
        "#,
    )
    .bind(community.as_uuid())
    .bind(coordinate)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(owner, members)| ProjectGate { owner, members }))
}

/// Returns `true` if `pubkey` is positively admitted to the **private**
/// project at `coordinate` — its owner or an invited member.
///
/// Unlike [`can_access_project_contents`] this is a positive membership
/// grant, not a gate: an unknown or public project returns `false` (there is
/// no private membership to grant). Used by the git smart-HTTP read gate to
/// let project members clone a private project's repos without bound-channel
/// membership.
pub async fn is_private_project_member(
    pool: &PgPool,
    community: CommunityId,
    coordinate: &str,
    pubkey: &[u8],
) -> Result<bool> {
    let row: Option<(bool,)> = sqlx::query_as(
        r#"
        SELECT (pa.owner = $3
                OR EXISTS (
                    SELECT 1 FROM project_acl_members pam
                    WHERE pam.community_id = pa.community_id
                      AND pam.owner = pa.owner
                      AND pam.dtag = pa.dtag
                      AND pam.pubkey = $3
                ))
        FROM project_acl pa
        WHERE pa.community_id = $1 AND pa.coordinate = $2 AND pa.visibility = 'private'
        "#,
    )
    .bind(community.as_uuid())
    .bind(coordinate)
    .bind(pubkey)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(ok,)| ok).unwrap_or(false))
}

/// Returns `true` if `pubkey` may read contents of the project at
/// `coordinate`: the project is unknown/public, or the reader is its owner or
/// an invited member. Fail direction on an absent row is open — an
/// unresolvable `project_ref` means "no gate", matching the deletion
/// semantics above.
///
/// Used by the ingest write path (`check_channel_membership`) for open
/// channels inside a private project; the read path pushes the equivalent
/// predicate into SQL in [`crate::channel::get_accessible_channel_ids`].
pub async fn can_access_project_contents(
    pool: &PgPool,
    community: CommunityId,
    coordinate: &str,
    pubkey: &[u8],
) -> Result<bool> {
    let row: Option<(bool,)> = sqlx::query_as(
        r#"
        SELECT (pa.owner = $3
                OR EXISTS (
                    SELECT 1 FROM project_acl_members pam
                    WHERE pam.community_id = pa.community_id
                      AND pam.owner = pa.owner
                      AND pam.dtag = pa.dtag
                      AND pam.pubkey = $3
                ))
        FROM project_acl pa
        WHERE pa.community_id = $1 AND pa.coordinate = $2 AND pa.visibility = 'private'
        "#,
    )
    .bind(community.as_uuid())
    .bind(coordinate)
    .bind(pubkey)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(ok,)| ok).unwrap_or(true))
}
