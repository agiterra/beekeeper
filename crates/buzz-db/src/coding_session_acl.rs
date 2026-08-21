//! Coding-session authority-grant queries (NIP-CSAT).
//!
//! Read side of the `coding_session_authority_acl` projection maintained by
//! [`crate::event::insert_coding_session_authority_transition_event`] inside
//! the chain's advisory-locked transaction. One row per live grant
//! (`operator` steers, `viewer` reads); the founder never has a row — their
//! standing is the kind:44226 genesis signature, resolved from the events
//! table here.
//!
//! Relay-side these checks are defense in depth at the channel grain: a
//! 44220 turn command addresses a provider-minted execution id the relay
//! cannot map to a genesis, so the relay requires founder-or-operator
//! standing on *some* genesis-rooted session in the channel while the
//! session provider enforces the exact per-session rule (verified through
//! relay-signed acceptance receipts). Kinds that carry the umbrella UUID
//! (44227 goals) resolve exactly.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;
use crate::CommunityId;

/// The resolved authority of one genesis-rooted session: its founder plus
/// the live grant sets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionAuthority {
    /// Genesis signer — may always steer and owns the lifecycle.
    pub founder: Vec<u8>,
    /// Pubkeys with a live `operator` grant (may steer).
    pub operators: Vec<Vec<u8>>,
    /// Pubkeys with a live `viewer` grant (read-only).
    pub viewers: Vec<Vec<u8>>,
}

impl SessionAuthority {
    /// May `pubkey` steer this session (turn start/interrupt, goal edits)?
    pub fn may_steer(&self, pubkey: &[u8]) -> bool {
        self.founder == pubkey || self.operators.iter().any(|op| op == pubkey)
    }
}

/// Returns `true` when the channel contains at least one coding-session
/// genesis (kind 44226). Channels without one predate the authority chain —
/// steering falls back to the legacy membership rule there, matching the
/// provider's own treatment of no-genesis records.
pub async fn channel_has_genesis_sessions(
    pool: &PgPool,
    community: CommunityId,
    channel_id: Uuid,
) -> Result<bool> {
    let (exists,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM events \
         WHERE community_id = $1 AND channel_id = $2 AND kind = 44226)",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Returns `true` when `pubkey` founded a genesis-rooted session in the
/// channel or holds a live `operator` grant on one — the relay-grain
/// steering requirement for kind 44220 commands. Soft-deleted geneses still
/// count (a genesis is a permanent identity anchor).
pub async fn has_steer_standing_in_channel(
    pool: &PgPool,
    community: CommunityId,
    channel_id: Uuid,
    pubkey: &[u8],
) -> Result<bool> {
    let (exists,): (bool,) = sqlx::query_as(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM events
            WHERE community_id = $1 AND channel_id = $2 AND kind = 44226 AND pubkey = $3
        ) OR EXISTS (
            SELECT 1 FROM coding_session_authority_acl
            WHERE community_id = $1 AND channel_id = $2 AND grantee = $3 AND role = 'operator'
        )
        "#,
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(pubkey)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Returns `true` when `pubkey` holds any live grant (operator or viewer)
/// on a session in the channel — the positive read grant that admits an
/// external session invitee to the session's transport channel.
pub async fn has_session_grant_in_channel(
    pool: &PgPool,
    community: CommunityId,
    channel_id: Uuid,
    pubkey: &[u8],
) -> Result<bool> {
    let (exists,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM coding_session_authority_acl \
         WHERE community_id = $1 AND channel_id = $2 AND grantee = $3)",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(pubkey)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Resolve the exact authority of the session labelled `session_ref` in
/// `channel_id`: founder from its genesis event (soft-deleted rows count),
/// grants from the ACL. `None` when no genesis claims the label — the
/// caller decides the legacy fallback.
pub async fn session_authority_by_ref(
    pool: &PgPool,
    community: CommunityId,
    channel_id: Uuid,
    session_ref: &str,
) -> Result<Option<SessionAuthority>> {
    let probe = serde_json::json!([["csg-session", session_ref]]);
    let founder: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT pubkey FROM events \
         WHERE community_id = $1 AND channel_id = $2 AND kind = 44226 AND tags @> $3::jsonb \
         LIMIT 1",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(&probe)
    .fetch_optional(pool)
    .await?;
    let Some((founder,)) = founder else {
        return Ok(None);
    };
    let rows: Vec<(Vec<u8>, String)> = sqlx::query_as(
        "SELECT grantee, role FROM coding_session_authority_acl \
         WHERE community_id = $1 AND channel_id = $2 AND session_ref = $3",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(session_ref)
    .fetch_all(pool)
    .await?;
    let mut authority = SessionAuthority {
        founder,
        ..Default::default()
    };
    for (grantee, role) in rows {
        if role == "operator" {
            authority.operators.push(grantee);
        } else {
            authority.viewers.push(grantee);
        }
    }
    Ok(Some(authority))
}
