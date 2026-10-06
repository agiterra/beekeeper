//! Read side of the project-action delegation (NIP-CSAT, ledger 186).
//!
//! The relay admits a project action's publication (kind 30620) and its manual
//! run (kind 46020) for the project's creator, a roster owner or an endorsed
//! repository's founder. A team session's lead seat is none of those, which is
//! why a session told to write and run a `verify` action had to stop and ask a
//! person (finding 178(f)). A project owner may now delegate exactly those two
//! acts, for exactly one project, by signing a `grant-project-actions` link
//! into the session's authority chain.
//!
//! This module answers one question for the relay's admission path: **does
//! `grantee` hold a live delegation of `project_ref` in this channel, and who
//! signed it**. It deliberately answers nothing else. Whether the granter is
//! *still* one of the project's owners is re-asked by the caller against the
//! project's current records, so a granter who has since lost ownership leaves
//! no capability behind; and whether the grantee still holds the session's lead
//! seat is returned here beside the grant so the caller can require it without
//! a second read.
//!
//! # Why a truncated chain refuses instead of deciding
//!
//! A chain is only meaningful whole: `seq` and `prevAccepted` are what prove a
//! revocation came after its grant. So the scan is bounded, and a channel with
//! more transitions than the bound returns
//! [`ProjectActionGrantLookup::Undecidable`] rather than a decision taken on a
//! prefix. The same reasoning as the seat projection's contiguity check in
//! [`crate::coding_session_acl`]: disclose nothing rather than activate a
//! stale link.

use std::collections::BTreeMap;

use beekeeper_core::coding_session_authority_transition::decode_coding_session_authority_transition;
use beekeeper_core::coding_session_project_action_grant::{
    fold_project_action_grants, ProjectActionGrant, ProjectActionGrantLink,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;
use crate::CommunityId;

/// Largest number of kind-44228 events this lookup will read from one channel.
///
/// A project's coding-session channel accumulates one transition per grant,
/// seat and claim across every session it has ever held. Ten thousand is far
/// above anything observed (the busiest recorded run wrote fewer than fifty)
/// and still bounds the query.
pub const MAX_PROJECT_ACTION_GRANT_TRANSITIONS: usize = 10_000;

/// A live delegation and the seat its holder occupies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectActionGrantStanding {
    /// The live grant: who, which project, who granted it, and the accepted
    /// event that is the evidence.
    pub grant: ProjectActionGrant,
    /// Genesis event id (hex) of the session whose chain carries the grant.
    pub genesis_ref: String,
    /// The grantee's active seat role in that session, when it holds one —
    /// `Some("lead")` for the case this feature exists for.
    pub seat_role: Option<String>,
}

/// What the lookup could establish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectActionGrantLookup {
    /// The chains were read whole; this is every live delegation of the
    /// requested project held by the requested pubkey (at most one per
    /// session chain).
    Decided(Vec<ProjectActionGrantStanding>),
    /// The channel holds more transitions than this lookup reads, so no
    /// answer can be given from a whole chain. The caller fails closed.
    Undecidable,
}

struct StoredLink {
    event_id: Vec<u8>,
    signer: Vec<u8>,
    payload:
        beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload,
}

/// Every live delegation of `project_ref` held by `grantee` in `channel_id`.
///
/// `grantee` is raw pubkey bytes; `project_ref` is the exact coordinate the
/// grant names, compared verbatim (the relay's workflow rows carry the same
/// string, so a normalization here would be a second opinion nobody asked
/// for).
pub async fn live_project_action_grants(
    pool: &PgPool,
    community: CommunityId,
    channel_id: Uuid,
    project_ref: &str,
    grantee: &[u8],
) -> Result<ProjectActionGrantLookup> {
    let rows: Vec<(Vec<u8>, Vec<u8>, String)> = sqlx::query_as(
        "SELECT id, pubkey, content FROM events \
         WHERE community_id = $1 AND channel_id = $2 AND kind = $3 \
         ORDER BY created_at ASC, id ASC LIMIT $4",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(beekeeper_core::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION as i32)
    .bind(i64::try_from(MAX_PROJECT_ACTION_GRANT_TRANSITIONS + 1).unwrap_or(i64::MAX))
    .fetch_all(pool)
    .await?;
    if rows.len() > MAX_PROJECT_ACTION_GRANT_TRANSITIONS {
        return Ok(ProjectActionGrantLookup::Undecidable);
    }

    // Group by the genesis each link names. A decode failure is skipped, not
    // fatal: every accepted transition passed the same decoder before it was
    // stored, so an undecodable row is one this build cannot read, and reading
    // it as part of a chain would be guessing.
    let mut chains: BTreeMap<String, Vec<StoredLink>> = BTreeMap::new();
    for (event_id, signer, content) in rows {
        let Ok(payload) = decode_coding_session_authority_transition(&content) else {
            continue;
        };
        chains
            .entry(payload.genesis_ref.clone())
            .or_default()
            .push(StoredLink {
                event_id,
                signer,
                payload,
            });
    }

    let grantee_hex = hex::encode(grantee);
    let mut standings = Vec::new();
    for (genesis_ref, mut links) in chains {
        links.sort_by_key(|link| link.payload.seq);
        if !chain_is_contiguous(&links) {
            continue;
        }
        let grants = fold_project_action_grants(links.iter().map(|link| ProjectActionGrantLink {
            seq: link.payload.seq,
            accepted_event_id: hex::encode(&link.event_id),
            transition_type: link.payload.transition_type,
            signer_pubkey: hex::encode(&link.signer),
            grantee_pubkey: link.payload.grantee_pubkey.clone(),
            project_ref: link.payload.project_ref.clone(),
        }));
        let Some(grant) =
            beekeeper_core::coding_session_project_action_grant::find_project_action_grant(
                &grants,
                &grantee_hex,
                project_ref,
            )
        else {
            continue;
        };
        standings.push(ProjectActionGrantStanding {
            grant: grant.clone(),
            genesis_ref,
            seat_role: active_seat_role(&links, &grantee_hex),
        });
    }
    Ok(ProjectActionGrantLookup::Decided(standings))
}

/// Whether the sorted links form one chain: `seq` 1..n with each
/// `prevAccepted` naming its predecessor's event id.
fn chain_is_contiguous(links: &[StoredLink]) -> bool {
    let mut expected_prev: Option<&[u8]> = None;
    for (index, link) in links.iter().enumerate() {
        let expected_seq = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if link.payload.seq != expected_seq {
            return false;
        }
        let actual_prev = link
            .payload
            .prev_accepted
            .as_deref()
            .and_then(|value| hex::decode(value).ok());
        if actual_prev.as_deref() != expected_prev {
            return false;
        }
        expected_prev = Some(link.event_id.as_slice());
    }
    true
}

/// The actor's active seat role at the end of a contiguous chain.
fn active_seat_role(links: &[StoredLink], actor_hex: &str) -> Option<String> {
    use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType as Type;

    let mut role = None;
    for link in links {
        if link.payload.grantee_pubkey != actor_hex {
            continue;
        }
        match link.payload.transition_type {
            Type::GrantSeat => role = link.payload.role.clone(),
            Type::RevokeSeat => role = None,
            Type::GrantOperator
            | Type::GrantViewer
            | Type::Revoke
            | Type::Takeover
            | Type::Transfer
            | Type::GrantProjectActions
            | Type::RevokeProjectActions => {}
        }
    }
    role
}
