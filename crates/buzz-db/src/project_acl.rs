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

use std::str::FromStr;

use sqlx::PgPool;

pub use buzz_core::channel::ProjectRole;

use crate::error::Result;
use crate::CommunityId;

/// One roster entry: an invited member pubkey and its role.
pub type ProjectMember = (Vec<u8>, ProjectRole);

/// Parse a stored role string, failing closed: an unknown value (impossible
/// under the CHECK constraint, but a projection must not guess) demotes to
/// the read-only tier rather than granting anything.
fn parse_role_fail_closed(role: &str) -> ProjectRole {
    ProjectRole::from_str(role).unwrap_or(ProjectRole::Viewer)
}

/// Upsert the ACL row for a freshly-ingested 30621 head and — while the
/// roster is still head-sourced — replace its invited-member set from the
/// head's `p` tags.
///
/// Republish-latest: a newer head (greater `head_created_at`) wins; a stale or
/// out-of-order replay is ignored by the `head_created_at` guard. Members are
/// replaced only when the head actually applied, so a replayed old head can
/// never resurrect a revoked invite. Idempotent — re-applying the same head is
/// a no-op ending in the same row and member set.
///
/// Once a project's `roster_source` is `'ops'` (the first accepted 9010/9011
/// flipped it), head `p` tags are ignored entirely: the roster belongs to the
/// relay-managed ops from then on, and a creator-signed head replay must not
/// evict members a co-owner added. Returns `true` when the head's members
/// were applied (roster still head-sourced), `false` when they were ignored.
pub async fn upsert_project_acl(
    pool: &PgPool,
    community: CommunityId,
    owner: &[u8],
    dtag: &str,
    visibility: &str,
    members: &[ProjectMember],
    head_created_at: i64,
) -> Result<bool> {
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

    let mut members_applied = false;
    if applied > 0 {
        let (roster_source,): (String,) = sqlx::query_as(
            "SELECT roster_source FROM project_acl WHERE community_id = $1 AND owner = $2 AND dtag = $3",
        )
        .bind(community.as_uuid())
        .bind(owner)
        .bind(dtag)
        .fetch_one(&mut *tx)
        .await?;
        if roster_source == "head" {
            sqlx::query(
                "DELETE FROM project_acl_members WHERE community_id = $1 AND owner = $2 AND dtag = $3",
            )
            .bind(community.as_uuid())
            .bind(owner)
            .bind(dtag)
            .execute(&mut *tx)
            .await?;
            for (pubkey, role) in members {
                sqlx::query(
                    r#"
                    INSERT INTO project_acl_members (community_id, owner, dtag, pubkey, role)
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT DO NOTHING
                    "#,
                )
                .bind(community.as_uuid())
                .bind(owner)
                .bind(dtag)
                .bind(pubkey)
                .bind(role.as_str())
                .execute(&mut *tx)
                .await?;
            }
            members_applied = true;
        }
    }
    tx.commit().await?;
    Ok(members_applied)
}

/// Why a project membership op (kind 9010/9011) was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectMemberOpRefusal {
    /// The `a`-tag coordinate does not resolve to a stored project ACL row.
    /// Fails closed — an op against an unknown project grants nothing.
    ProjectNotFound,
    /// The signer is neither the project creator nor a roster `owner`.
    ActorNotOwner,
    /// The op targets the project creator, who is the project's address:
    /// always an implicit Owner, never on the roster, never removable or
    /// demotable. Refused outright so the error is intentional, not
    /// incidental.
    TargetsCreator,
    /// Applying the op would push the roster past [`PROJECT_ROSTER_CAP`].
    RosterFull,
}

/// Maximum roster size, matching the head `p`-tag invite cap in
/// `buzz-relay`'s ingest validation (`PROJECT_INVITE_CAP`).
pub const PROJECT_ROSTER_CAP: i64 = 256;

/// Outcome of an attempted project membership op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectMemberOpOutcome {
    /// The op was applied; the roster is now ops-sourced.
    Applied,
    /// The op was refused; nothing changed.
    Refused(ProjectMemberOpRefusal),
}

/// Authorize the actor inside an open transaction: the project creator or a
/// roster `owner` may manage membership. Row-locks the ACL row so two rival
/// ops (or an op racing a head republish) serialize.
async fn lock_and_authorize_member_op(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    community: CommunityId,
    owner: &[u8],
    dtag: &str,
    actor: &[u8],
) -> Result<std::result::Result<(), ProjectMemberOpRefusal>> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT owner FROM project_acl WHERE community_id = $1 AND owner = $2 AND dtag = $3 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(dtag)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((creator,)) = row else {
        return Ok(Err(ProjectMemberOpRefusal::ProjectNotFound));
    };
    if creator == actor {
        return Ok(Ok(()));
    }
    let (is_roster_owner,): (bool,) = sqlx::query_as(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM project_acl_members
            WHERE community_id = $1 AND owner = $2 AND dtag = $3
              AND pubkey = $4 AND role = 'owner'
        )
        "#,
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(dtag)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    if is_roster_owner {
        Ok(Ok(()))
    } else {
        Ok(Err(ProjectMemberOpRefusal::ActorNotOwner))
    }
}

/// Flip the roster to ops-sourced. From this point head `p` tags are ignored
/// by [`upsert_project_acl`]; there is deliberately no way back.
async fn flip_roster_source_to_ops(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    community: CommunityId,
    owner: &[u8],
    dtag: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE project_acl SET roster_source = 'ops', updated_at = NOW()
        WHERE community_id = $1 AND owner = $2 AND dtag = $3 AND roster_source = 'head'
        "#,
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(dtag)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Apply a kind:9010 put-member op: add each `(pubkey, role)` to the roster,
/// or change the role of an existing member. Authorized for the project
/// creator or a roster `owner`; the creator can never be a target. The first
/// applied op flips `roster_source` to `'ops'`.
pub async fn put_project_members(
    pool: &PgPool,
    community: CommunityId,
    owner: &[u8],
    dtag: &str,
    actor: &[u8],
    members: &[ProjectMember],
) -> Result<ProjectMemberOpOutcome> {
    let mut tx = pool.begin().await?;
    if let Err(refusal) =
        lock_and_authorize_member_op(&mut tx, community, owner, dtag, actor).await?
    {
        tx.rollback().await?;
        return Ok(ProjectMemberOpOutcome::Refused(refusal));
    }
    if members.iter().any(|(pubkey, _)| pubkey.as_slice() == owner) {
        tx.rollback().await?;
        return Ok(ProjectMemberOpOutcome::Refused(
            ProjectMemberOpRefusal::TargetsCreator,
        ));
    }
    flip_roster_source_to_ops(&mut tx, community, owner, dtag).await?;
    for (pubkey, role) in members {
        sqlx::query(
            r#"
            INSERT INTO project_acl_members (community_id, owner, dtag, pubkey, role)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (community_id, owner, dtag, pubkey) DO UPDATE SET role = EXCLUDED.role
            "#,
        )
        .bind(community.as_uuid())
        .bind(owner)
        .bind(dtag)
        .bind(pubkey)
        .bind(role.as_str())
        .execute(&mut *tx)
        .await?;
    }
    let (roster_len,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM project_acl_members WHERE community_id = $1 AND owner = $2 AND dtag = $3",
    )
    .bind(community.as_uuid())
    .bind(owner)
    .bind(dtag)
    .fetch_one(&mut *tx)
    .await?;
    if roster_len > PROJECT_ROSTER_CAP {
        tx.rollback().await?;
        return Ok(ProjectMemberOpOutcome::Refused(
            ProjectMemberOpRefusal::RosterFull,
        ));
    }
    tx.commit().await?;
    Ok(ProjectMemberOpOutcome::Applied)
}

/// Apply a kind:9011 remove-member op: drop each pubkey from the roster
/// (removing an absent pubkey is a no-op, so removes are idempotent).
/// Authorized for the project creator or a roster `owner`; the creator can
/// never be a target. The first applied op flips `roster_source` to `'ops'`.
pub async fn remove_project_members(
    pool: &PgPool,
    community: CommunityId,
    owner: &[u8],
    dtag: &str,
    actor: &[u8],
    member_pubkeys: &[Vec<u8>],
) -> Result<ProjectMemberOpOutcome> {
    let mut tx = pool.begin().await?;
    if let Err(refusal) =
        lock_and_authorize_member_op(&mut tx, community, owner, dtag, actor).await?
    {
        tx.rollback().await?;
        return Ok(ProjectMemberOpOutcome::Refused(refusal));
    }
    if member_pubkeys.iter().any(|pk| pk.as_slice() == owner) {
        tx.rollback().await?;
        return Ok(ProjectMemberOpOutcome::Refused(
            ProjectMemberOpRefusal::TargetsCreator,
        ));
    }
    flip_roster_source_to_ops(&mut tx, community, owner, dtag).await?;
    for pubkey in member_pubkeys {
        sqlx::query(
            r#"
            DELETE FROM project_acl_members
            WHERE community_id = $1 AND owner = $2 AND dtag = $3 AND pubkey = $4
            "#,
        )
        .bind(community.as_uuid())
        .bind(owner)
        .bind(dtag)
        .bind(pubkey)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(ProjectMemberOpOutcome::Applied)
}

/// A project's full roster, resolved for the relay-signed kind:39010
/// projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoster {
    /// The project creator (the 30621 address pubkey).
    pub owner: Vec<u8>,
    /// The project's `d` tag.
    pub dtag: String,
    /// `public` or `private`.
    pub visibility: String,
    /// Invited members with roles (creator excluded — implicit Owner).
    pub members: Vec<ProjectMember>,
}

/// Resolve a project's roster by coordinate, or `None` when unknown.
pub async fn get_project_roster(
    pool: &PgPool,
    community: CommunityId,
    coordinate: &str,
) -> Result<Option<ProjectRoster>> {
    type RosterRow = (Vec<u8>, String, String, Vec<Vec<u8>>, Vec<String>);
    let row: Option<RosterRow> = sqlx::query_as(
        r#"
        SELECT pa.owner, pa.dtag, pa.visibility,
               COALESCE(
                   array_agg(pam.pubkey ORDER BY pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_pubkeys,
               COALESCE(
                   array_agg(pam.role ORDER BY pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_roles
        FROM project_acl pa
        LEFT JOIN project_acl_members pam
          ON pam.community_id = pa.community_id
         AND pam.owner = pa.owner
         AND pam.dtag = pa.dtag
        WHERE pa.community_id = $1 AND pa.coordinate = $2
        GROUP BY pa.owner, pa.dtag, pa.visibility
        "#,
    )
    .bind(community.as_uuid())
    .bind(coordinate)
    .fetch_optional(pool)
    .await?;
    Ok(
        row.map(|(owner, dtag, visibility, pubkeys, roles)| ProjectRoster {
            owner,
            dtag,
            visibility,
            members: zip_members(pubkeys, roles),
        }),
    )
}

/// Pair the parallel pubkey/role arrays a roster query aggregates (both
/// ordered by pubkey so the pairing is deterministic).
pub(crate) fn zip_members(pubkeys: Vec<Vec<u8>>, roles: Vec<String>) -> Vec<ProjectMember> {
    pubkeys
        .into_iter()
        .zip(roles)
        .map(|(pubkey, role)| (pubkey, parse_role_fail_closed(&role)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> ProjectGate {
        ProjectGate {
            owner: vec![0xAA; 32],
            members: vec![
                (vec![0x01; 32], ProjectRole::Owner),
                (vec![0x02; 32], ProjectRole::Collaborator),
                (vec![0x03; 32], ProjectRole::Viewer),
            ],
        }
    }

    /// The full read/write matrix: creator and roster owner do everything,
    /// collaborators write but a viewer only reads, strangers get nothing.
    #[test]
    fn gate_read_write_matrix() {
        let gate = gate();
        let creator = vec![0xAA; 32];
        let roster_owner = vec![0x01; 32];
        let collaborator = vec![0x02; 32];
        let viewer = vec![0x03; 32];
        let stranger = vec![0x04; 32];

        for admitted in [&creator, &roster_owner, &collaborator, &viewer] {
            assert!(gate.admits_read(admitted), "read for {admitted:02x?}");
        }
        assert!(!gate.admits_read(&stranger));

        for writer in [&creator, &roster_owner, &collaborator] {
            assert!(gate.admits_write(writer), "write for {writer:02x?}");
        }
        assert!(!gate.admits_write(&viewer), "viewers are read-only");
        assert!(!gate.admits_write(&stranger));

        assert_eq!(gate.role_of(&creator), Some(ProjectRole::Owner));
        assert_eq!(gate.role_of(&viewer), Some(ProjectRole::Viewer));
        assert_eq!(gate.role_of(&stranger), None);
    }

    /// Stored roles outside the vocabulary (impossible under the CHECK
    /// constraint) demote to the read-only tier — a projection must not
    /// guess a grant.
    #[test]
    fn unknown_stored_role_fails_closed_to_viewer() {
        let members = zip_members(vec![vec![0x01; 32]], vec!["superadmin".to_string()]);
        assert_eq!(members, vec![(vec![0x01; 32], ProjectRole::Viewer)]);
    }
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

/// The resolved gate of one project: its owner plus invited members with
/// roles.
///
/// Loaded per channel by [`get_channel_project_gate`] and cached relay-side so
/// live fan-out can filter recipients in memory without per-recipient DB hits.
///
/// There is deliberately no plain `admits()` any more: every caller must
/// decide whether the operation at hand is a read (any role) or a write
/// (owner/collaborator only), so adding the role tier was a compile-time
/// sweep of every gate decision rather than a silent behavior change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGate {
    /// The project creator — always an implicit Owner.
    pub owner: Vec<u8>,
    /// Invited members with roles.
    pub members: Vec<ProjectMember>,
}

impl ProjectGate {
    /// The role `pubkey` holds in this project, if any. The creator is an
    /// implicit [`ProjectRole::Owner`].
    pub fn role_of(&self, pubkey: &[u8]) -> Option<ProjectRole> {
        if self.owner == pubkey {
            return Some(ProjectRole::Owner);
        }
        self.members
            .iter()
            .find(|(member, _)| member == pubkey)
            .map(|(_, role)| *role)
    }

    /// Returns `true` if `pubkey` may read what this gate protects: the
    /// creator or any-role member (the pre-role `admits()` semantics).
    pub fn admits_read(&self, pubkey: &[u8]) -> bool {
        self.role_of(pubkey).is_some()
    }

    /// Returns `true` if `pubkey` may write into what this gate protects:
    /// the creator or a member whose role is owner/collaborator. Viewers are
    /// read-only everywhere in the project.
    pub fn admits_write(&self, pubkey: &[u8]) -> bool {
        self.role_of(pubkey).is_some_and(ProjectRole::can_write)
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
    let row: Option<GateRow> = sqlx::query_as(
        r#"
        SELECT pa.owner,
               COALESCE(
                   array_agg(pam.pubkey ORDER BY pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_pubkeys,
               COALESCE(
                   array_agg(pam.role ORDER BY pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_roles
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
    Ok(row.map(GateRow::into_gate))
}

/// Row shape shared by every gate query: the creator plus parallel
/// pubkey/role arrays (both ordered by pubkey so the pairing is
/// deterministic).
type GateRowTuple = (Vec<u8>, Vec<Vec<u8>>, Vec<String>);

/// Newtype so the gate queries share one tuple→[`ProjectGate`] conversion.
struct GateRow(GateRowTuple);

impl GateRow {
    fn into_gate(self) -> ProjectGate {
        let (owner, pubkeys, roles) = self.0;
        ProjectGate {
            owner,
            members: zip_members(pubkeys, roles),
        }
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for GateRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> std::result::Result<Self, sqlx::Error> {
        Ok(Self(GateRowTuple::from_row(row)?))
    }
}

/// Resolve the project gate of a session-**transport** channel, if it is one.
///
/// Returns `None` when the channel does not exist, is not `channel_type =
/// 'transport'`, has no `project_ref`, or its project is unknown — all
/// meaning "explicit channel members only". Unlike
/// [`get_channel_project_gate`] there is **no** private-visibility filter:
/// transport admittance is a *positive* membership grant (the project's
/// owner and invited members), and `project_acl` stores rows for public
/// heads too, so a public project's members are admitted while everyone
/// else still is not. An unresolvable project fails closed to members-only.
pub async fn get_channel_transport_gate(
    pool: &PgPool,
    community: CommunityId,
    channel_id: uuid::Uuid,
) -> Result<Option<ProjectGate>> {
    let row: Option<GateRow> = sqlx::query_as(
        r#"
        SELECT pa.owner,
               COALESCE(
                   array_agg(pam.pubkey ORDER BY pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_pubkeys,
               COALESCE(
                   array_agg(pam.role ORDER BY pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_roles
        FROM channels c
        JOIN project_acl pa
          ON pa.community_id = c.community_id
         AND pa.coordinate = c.project_ref
        LEFT JOIN project_acl_members pam
          ON pam.community_id = pa.community_id
         AND pam.owner = pa.owner
         AND pam.dtag = pa.dtag
        WHERE c.community_id = $1 AND c.id = $2
          AND c.channel_type = 'transport' AND c.deleted_at IS NULL
        GROUP BY pa.owner
        "#,
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(GateRow::into_gate))
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
    let row: Option<GateRow> = sqlx::query_as(
        r#"
        SELECT pa.owner,
               COALESCE(
                   array_agg(pam.pubkey ORDER BY pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_pubkeys,
               COALESCE(
                   array_agg(pam.role ORDER BY pam.pubkey) FILTER (WHERE pam.pubkey IS NOT NULL),
                   '{}'
               ) AS member_roles
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
    Ok(row.map(GateRow::into_gate))
}

/// Returns `true` when a project head with `coordinate` exists in `community`,
/// at any visibility.
///
/// [`get_project_gate_by_coordinate`] returns `None` for *both* "public" and
/// "never created", because its query filters `visibility = 'private'`. That
/// conflation is safe for a soft back-reference and unsafe for a required
/// singleton `a` tag: a Pulse entry naming a coordinate no kind:30621 event
/// ever created is in nobody's hidden set, so it would be stored and shown to
/// everyone as a coordination fact invented out of nothing. This is the same
/// query minus the visibility clause, so ingest can tell the two apart.
///
/// Indexed by `idx_project_acl_coordinate` on `(community_id, coordinate)`
/// (`migrations/0033_project_acl.sql`).
pub async fn project_exists_by_coordinate(
    pool: &PgPool,
    community: CommunityId,
    coordinate: &str,
) -> Result<bool> {
    let row: Option<(i32,)> = sqlx::query_as(
        r#"
        SELECT 1
        FROM project_acl pa
        WHERE pa.community_id = $1 AND pa.coordinate = $2
        "#,
    )
    .bind(community.as_uuid())
    .bind(coordinate)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
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

/// Returns `true` if `pubkey` may **write** contents into the project at
/// `coordinate`: the project is unknown/public (no gate, matching
/// [`can_access_project_contents`]'s fail-open direction for a dangling
/// `project_ref`), or the reader is its creator or an owner/collaborator
/// member. A private project's viewers read but never write.
pub async fn can_write_project_contents(
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
                      AND pam.role IN ('owner', 'collaborator')
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
