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

use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
};
use buzz_core::coding_session_genesis::decode_coding_session_genesis;
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
    /// Active role seats projected from the relay-accepted 44228 chain.
    pub seats: Vec<SessionSeat>,
}

/// One active actor-role seat in a session's accepted authority chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSeat {
    /// Actor public key.
    pub actor: Vec<u8>,
    /// Exact normalized role slug.
    pub role: String,
}

impl SessionAuthority {
    /// May `pubkey` steer this session (turn start/interrupt, goal edits)?
    pub fn may_steer(&self, pubkey: &[u8]) -> bool {
        self.founder == pubkey || self.operators.iter().any(|op| op == pubkey)
    }

    /// Whether `pubkey` may hire `role` into this session.
    ///
    /// Founder and active steering operators may hire any role. An active
    /// accepted lead seat may hire only a non-lead role; seat state never
    /// bootstraps itself because only transitions already accepted by the
    /// relay's serialized authority transaction reach this projection.
    pub fn may_hire(&self, pubkey: &[u8], role: &str) -> bool {
        self.may_steer(pubkey)
            || (role != "lead"
                && self
                    .seats
                    .iter()
                    .any(|seat| seat.actor == pubkey && seat.role == "lead"))
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

/// Resolve authority by the hire's exact genesis reference and session label.
///
/// Unlike [`session_authority_by_ref`], this never selects authority by the
/// mutable-looking umbrella label. The referenced genesis event id is fetched
/// directly, checked against this channel and `session_ref`, and then its
/// accepted transition chain is folded. A stale, wrong-channel, or
/// wrong-session reference returns `None` and cannot borrow a lead seat from a
/// different authority root.
pub async fn session_authority_for_hire(
    pool: &PgPool,
    community: CommunityId,
    channel_id: Uuid,
    genesis_ref: &str,
    session_ref: &str,
) -> Result<Option<SessionAuthority>> {
    let Ok(genesis_id) = hex::decode(genesis_ref) else {
        return Ok(None);
    };
    let genesis: Option<(Vec<u8>, String)> = sqlx::query_as(
        "SELECT pubkey, content FROM events \
         WHERE community_id = $1 AND channel_id = $2 AND kind = 44226 AND id = $3",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(&genesis_id)
    .fetch_optional(pool)
    .await?;
    let Some((founder, genesis_content)) = genesis else {
        return Ok(None);
    };
    let Ok(payload) = decode_coding_session_genesis(&genesis_content) else {
        return Ok(None);
    };
    if payload.session_ref != session_ref {
        return Ok(None);
    }

    let rows: Vec<(Vec<u8>, String)> = sqlx::query_as(
        "SELECT grantee, role FROM coding_session_authority_acl \
         WHERE community_id = $1 AND channel_id = $2 AND genesis_ref = $3",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(&genesis_id)
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

    let probe = serde_json::json!([["csat-genesis", genesis_ref]]);
    let transition_rows: Vec<(Vec<u8>, String)> = sqlx::query_as(
        "SELECT id, content FROM events \
         WHERE community_id = $1 AND channel_id = $2 AND kind = 44228 \
         AND tags @> $3::jsonb",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(&probe)
    .fetch_all(pool)
    .await?;
    let mut transitions = transition_rows
        .into_iter()
        .filter_map(|(event_id, content)| {
            let payload = decode_coding_session_authority_transition(&content).ok()?;
            Some((payload.seq, event_id, payload))
        })
        .collect::<Vec<_>>();
    transitions.sort_by_key(|(seq, _, _)| *seq);

    let mut active = std::collections::HashMap::<String, String>::new();
    let mut expected_prev: Option<Vec<u8>> = None;
    for (index, (seq, event_id, transition)) in transitions.into_iter().enumerate() {
        let expected_seq = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let actual_prev = transition
            .prev_accepted
            .as_deref()
            .and_then(|value| hex::decode(value).ok());
        if seq != expected_seq || actual_prev != expected_prev {
            // Stored accepted chains are contiguous. If storage no longer
            // proves that invariant, disclose no seat authority rather than
            // activating a self-described or stale lead.
            authority.seats.clear();
            return Ok(Some(authority));
        }
        match transition.transition_type {
            CodingSessionAuthorityTransitionType::GrantSeat => {
                if let Some(role) = transition.role {
                    active.insert(transition.grantee_pubkey, role);
                }
            }
            CodingSessionAuthorityTransitionType::RevokeSeat => {
                active.remove(&transition.grantee_pubkey);
            }
            // This projection is the seat set and nothing else. A claim
            // (`takeover`/`transfer`) moves who is carrying the session, which
            // the provider's fence answers from the same chain; it grants no
            // seat and revokes none.
            CodingSessionAuthorityTransitionType::GrantOperator
            | CodingSessionAuthorityTransitionType::GrantViewer
            | CodingSessionAuthorityTransitionType::Revoke
            | CodingSessionAuthorityTransitionType::Takeover
            | CodingSessionAuthorityTransitionType::Transfer
            // Nor does a project-action delegation: it is about one project's
            // actions, never about this session's seats (ledger 186).
            | CodingSessionAuthorityTransitionType::GrantProjectActions
            | CodingSessionAuthorityTransitionType::RevokeProjectActions => {}
        }
        expected_prev = Some(event_id);
    }
    authority.seats = active
        .into_iter()
        .filter_map(|(actor, role)| {
            hex::decode(actor)
                .ok()
                .map(|actor| SessionSeat { actor, role })
        })
        .collect();
    authority
        .seats
        .sort_by(|left, right| left.actor.cmp(&right.actor));
    Ok(Some(authority))
}
