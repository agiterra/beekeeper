//! Shared-terminal roster projection (NIP-ST).
//!
//! Store+project projection of kind:30623 announce heads, keyed by
//! `(community, owner, session_id)` with the same LWW `head_created_at`
//! guard as [`crate::project_acl`]. The owner-signed announce stays
//! authoritative; these rows let the ephemeral input (24312) and watch
//! (24310) gates resolve roster membership without re-parsing the stored
//! head per event. Maintained during 30623 ingest; backfilled by
//! `migrations/0039_shell_session_roster.sql`.

use std::str::FromStr;

use sqlx::PgPool;

pub use buzz_core::channel::ShellRole;

use crate::error::Result;
use crate::CommunityId;

/// One roster entry: an invited pubkey and its role.
pub type ShellMember = (Vec<u8>, ShellRole);

/// The resolved roster of one shared terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellRoster {
    /// The announce head's project coordinate — input events must bind to
    /// the same coordinate, so a sender cannot route input at a session
    /// under a coordinate the announce never claimed.
    pub coordinate: String,
    /// `open` or `closed` — input/watch gates refuse closed sessions.
    pub status: String,
    /// Invited members with roles (the owner is implicit, never listed).
    pub members: Vec<ShellMember>,
}

impl ShellRoster {
    /// The role `pubkey` holds on this terminal's roster, if any.
    pub fn role_of(&self, pubkey: &[u8]) -> Option<ShellRole> {
        self.members
            .iter()
            .find(|(member, _)| member == pubkey)
            .map(|(_, role)| *role)
    }

    /// May `pubkey` type into this terminal (roster collaborator)? The
    /// owner is authorized separately by signature, never via the roster.
    pub fn admits_input(&self, pubkey: &[u8]) -> bool {
        matches!(self.role_of(pubkey), Some(ShellRole::Collaborator))
    }

    /// May `pubkey` watch this terminal via its roster (any role)? Project
    /// membership grants watching separately.
    pub fn admits_watch(&self, pubkey: &[u8]) -> bool {
        self.role_of(pubkey).is_some()
    }
}

/// Upsert the roster projection for a freshly-ingested 30623 head and
/// replace its member set from the head's role-tagged `p` tags.
/// Republish-latest with the standard LWW guard; a stale replay never
/// resurrects a revoked invite.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_shell_session_acl(
    pool: &PgPool,
    community: CommunityId,
    owner: &[u8],
    session_id: &str,
    coordinate: &str,
    status: &str,
    members: &[ShellMember],
    head_created_at: i64,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let applied = sqlx::query(
        r#"
        INSERT INTO shell_session_acl (community_id, owner, session_id, coordinate, status, head_created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        ON CONFLICT (community_id, owner, session_id) DO UPDATE SET
            coordinate      = EXCLUDED.coordinate,
            status          = EXCLUDED.status,
            head_created_at = EXCLUDED.head_created_at,
            updated_at      = NOW()
        WHERE EXCLUDED.head_created_at >= shell_session_acl.head_created_at
        "#,
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(session_id)
    .bind(coordinate)
    .bind(status)
    .bind(head_created_at)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if applied > 0 {
        sqlx::query(
            "DELETE FROM shell_session_acl_members \
             WHERE community_id = $1 AND owner = $2 AND session_id = $3",
        )
        .bind(community.as_uuid())
        .bind(owner)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        for (pubkey, role) in members {
            sqlx::query(
                r#"
                INSERT INTO shell_session_acl_members (community_id, owner, session_id, pubkey, role)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(community.as_uuid())
            .bind(owner)
            .bind(session_id)
            .bind(pubkey)
            .bind(role.as_str())
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Drop the roster projection for a NIP-09-deleted 30623 coordinate,
/// scoped to heads at or before the deletion (a stale tombstone must not
/// erase a newer replacement). Returns whether a row was removed.
pub async fn delete_shell_session_acl(
    pool: &PgPool,
    community: CommunityId,
    owner: &[u8],
    session_id: &str,
    deleted_at: i64,
) -> Result<bool> {
    let deleted = sqlx::query(
        r#"
        DELETE FROM shell_session_acl
        WHERE community_id = $1 AND owner = $2 AND session_id = $3 AND head_created_at <= $4
        "#,
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(session_id)
    .bind(deleted_at)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(deleted > 0)
}

/// Resolve one terminal's roster, or `None` when no announce head is
/// projected — callers fail closed (no roster grants nothing beyond the
/// project gate).
pub async fn get_shell_roster(
    pool: &PgPool,
    community: CommunityId,
    owner: &[u8],
    session_id: &str,
) -> Result<Option<ShellRoster>> {
    type RosterRow = (String, String, Vec<Vec<u8>>, Vec<String>);
    let row: Option<RosterRow> = sqlx::query_as(
        r#"
        SELECT sa.coordinate, sa.status,
               COALESCE(
                   array_agg(sam.pubkey ORDER BY sam.pubkey) FILTER (WHERE sam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_pubkeys,
               COALESCE(
                   array_agg(sam.role ORDER BY sam.pubkey) FILTER (WHERE sam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_roles
        FROM shell_session_acl sa
        LEFT JOIN shell_session_acl_members sam
          ON sam.community_id = sa.community_id
         AND sam.owner = sa.owner
         AND sam.session_id = sa.session_id
        WHERE sa.community_id = $1 AND sa.owner = $2 AND sa.session_id = $3
        GROUP BY sa.coordinate, sa.status
        "#,
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(coordinate, status, pubkeys, roles)| ShellRoster {
        coordinate,
        status,
        members: pubkeys
            .into_iter()
            .zip(roles)
            .map(|(pubkey, role)| {
                // Fail closed: an unknown stored role (impossible under the
                // CHECK constraint) demotes to watch-only.
                (
                    pubkey,
                    ShellRole::from_str(&role).unwrap_or(ShellRole::Viewer),
                )
            })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_role_checks() {
        let roster = ShellRoster {
            coordinate: "30621:aa:proj".into(),
            status: "open".into(),
            members: vec![
                (vec![0x01; 32], ShellRole::Collaborator),
                (vec![0x02; 32], ShellRole::Viewer),
            ],
        };
        assert!(roster.admits_input(&[0x01; 32]));
        assert!(!roster.admits_input(&[0x02; 32]), "viewers never type");
        assert!(!roster.admits_input(&[0x03; 32]));
        assert!(roster.admits_watch(&[0x01; 32]));
        assert!(roster.admits_watch(&[0x02; 32]));
        assert!(!roster.admits_watch(&[0x03; 32]));
    }
}
