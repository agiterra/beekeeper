//! NIP-SW shared surface kinds: watch (24320) and frame (24321), for both the
//! session preview and the device surface.
//!
//! Called from the generic ephemeral branch in `event.rs` after the scope
//! check, the per-kind one-second ceiling (`admission::ephemeral_kind_limit`)
//! and the community lifecycle fence. This module adds, in order: signature
//! verification off-thread, the ±5 minute freshness window, the strict core
//! validators (exact ordered tags, the host-local rule, the frame byte cap),
//! the token's channel restriction, the session channel's membership gate
//! (the same h path every channel ephemeral takes), and — for a frame — the
//! **announcer authority**: the frame's author must be the announced producer
//! of `(h, surface, d)`.
//!
//! - preview: the owner of the session's preview per the 30626 fold
//!   ([`beekeeper_core::session_preview::resolve_preview_owner`]);
//! - device: the signer of the newest 44255 `state` for the slot, whose
//!   (`csl-command`, `cs-target`) must resolve to that signer through the
//!   coding-session generation resolver.
//!
//! Positive answers are cached for at most
//! [`SURFACE_AUTHORITY_CACHE_SECS`](beekeeper_core::surface_watch::SURFACE_AUTHORITY_CACHE_SECS);
//! a missing authority or a failed lookup refuses the frame (fail closed).
//! Clients additionally subscribe with `authors=[authority]`.
//!
//! Delivery: a watch reaches only its `p` and its author (the branch in
//! `filter_fanout_by_access`); a frame reaches the channel under the normal
//! channel ACL. The relay keeps no watcher state.

use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use beekeeper_core::coding_session_title::parse_coding_session_target_key;
use beekeeper_core::kind::{
    event_kind_u32, KIND_SESSION_DEVICE_RECORD, KIND_SESSION_PREVIEW_ANNOUNCE, KIND_SURFACE_FRAME,
    KIND_SURFACE_WATCH,
};
use beekeeper_core::session_device::newest_device_state_record;
use beekeeper_core::session_preview::resolve_preview_owner;
use beekeeper_core::surface_watch::{
    check_surface_freshness, validate_surface_frame_envelope, validate_surface_watch_envelope,
    Surface, SURFACE_AUTHORITY_CACHE_SECS,
};
use beekeeper_core::verification::verify_event;
use beekeeper_core::CommunityId;
use beekeeper_db::EventQuery;
use dashmap::DashMap;
use nostr::{Event, PublicKey};
use uuid::Uuid;

use crate::connection::ConnectionState;
use crate::protocol::RelayMessage;
use crate::state::AppState;

/// The refusal a frame from anyone but the announced producer gets.
pub(crate) const FRAME_NOT_AUTHORITY: &str =
    "restricted: surface frame author is not the announced producer";

/// Most 30626 / 44255 rows read to resolve one authority.
const AUTHORITY_QUERY_LIMIT: i64 = 64;
/// Cache entries kept before the cache is cleared wholesale.
const AUTHORITY_CACHE_MAX_ENTRIES: usize = 4096;

type AuthorityKey = (CommunityId, Uuid, Surface, String);

/// Positive authority answers, keyed by (community, h, surface, d).
static AUTHORITY_CACHE: LazyLock<DashMap<AuthorityKey, (PublicKey, Instant)>> =
    LazyLock::new(DashMap::new);

/// Admit one kind 24320/24321 WebSocket event end to end; sends the `OK`.
// Transport context arrives as separate values from the existing event
// handler, exactly as for the session lease.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_surface_event(
    event: Event,
    conn_id: Uuid,
    pubkey_bytes: Vec<u8>,
    token_channel_ids: Option<Vec<Uuid>>,
    event_id_hex: &str,
    conn: Arc<ConnectionState>,
    state: Arc<AppState>,
) {
    match admit_surface_event(
        event,
        conn_id,
        &pubkey_bytes,
        token_channel_ids.as_deref(),
        Arc::clone(&conn),
        state,
    )
    .await
    {
        Ok(()) => {
            conn.send(RelayMessage::ok(event_id_hex, true, ""));
        }
        Err(message) => {
            conn.send(RelayMessage::ok(event_id_hex, false, &message));
        }
    }
}

async fn admit_surface_event(
    event: Event,
    conn_id: Uuid,
    pubkey_bytes: &[u8],
    token_channel_ids: Option<&[Uuid]>,
    conn: Arc<ConnectionState>,
    state: Arc<AppState>,
) -> Result<(), String> {
    let to_verify = event.clone();
    match tokio::task::spawn_blocking(move || verify_event(&to_verify)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(format!("invalid: {error}")),
        Err(_) => return Err("error: internal error".into()),
    }
    let now = u64::try_from(chrono::Utc::now().timestamp())
        .map_err(|_| "error: relay clock is before Unix epoch".to_string())?;
    check_surface_freshness(event.created_at.as_secs(), now)
        .map_err(|error| format!("invalid: {error}"))?;

    let (channel_id, frame_scope) = match event_kind_u32(&event) {
        KIND_SURFACE_WATCH => {
            let watch = validate_surface_watch_envelope(&event)
                .map_err(|error| format!("invalid: {error}"))?;
            (watch.channel_id, None)
        }
        KIND_SURFACE_FRAME => {
            let frame = validate_surface_frame_envelope(&event)
                .map_err(|error| format!("invalid: {error}"))?;
            (frame.channel_id, Some((frame.surface, frame.key)))
        }
        _ => return Err("invalid: not a surface watch or frame".into()),
    };
    if token_channel_ids.is_some_and(|allowed| !allowed.contains(&channel_id)) {
        return Err("restricted: token does not have access to this channel".into());
    }
    super::ingest::check_channel_membership(&conn.tenant, &state, channel_id, pubkey_bytes, None)
        .await?;

    if let Some((surface, key)) = frame_scope {
        let authority = frame_authority(&state, conn.tenant.community(), channel_id, surface, &key)
            .await
            .map_err(|error| {
                tracing::warn!(%conn_id, %channel_id, "surface frame authority lookup failed: {error}");
                "error: surface frame authority lookup failed".to_string()
            })?;
        if !frame_author_admitted(&event.pubkey, authority.as_ref()) {
            return Err(FRAME_NOT_AUTHORITY.into());
        }
    }

    super::event::fan_out_admitted_channel_ephemeral(
        event,
        channel_id,
        conn_id,
        &state,
        &conn.tenant,
    )
    .await;
    Ok(())
}

/// A frame is admitted only when an authority exists and is its author.
pub(crate) fn frame_author_admitted(author: &PublicKey, authority: Option<&PublicKey>) -> bool {
    authority.is_some_and(|authority| authority == author)
}

/// Resolve (and cache) the announced producer of `(channel, surface, key)`.
/// `Ok(None)`: nobody holds the surface. `Err`: the lookup failed.
async fn frame_authority(
    state: &AppState,
    community: CommunityId,
    channel_id: Uuid,
    surface: Surface,
    key: &str,
) -> Result<Option<PublicKey>, String> {
    let cache_key: AuthorityKey = (community, channel_id, surface, key.to_owned());
    let ttl = Duration::from_secs(SURFACE_AUTHORITY_CACHE_SECS);
    if let Some(entry) = AUTHORITY_CACHE.get(&cache_key) {
        let (authority, at) = *entry.value();
        if at.elapsed() < ttl {
            return Ok(Some(authority));
        }
    }
    let resolved = match surface {
        Surface::Preview => preview_authority(state, community, channel_id, key).await?,
        Surface::Device => device_authority(state, community, channel_id, key).await?,
    };
    match resolved {
        Some(authority) => {
            if AUTHORITY_CACHE.len() >= AUTHORITY_CACHE_MAX_ENTRIES {
                AUTHORITY_CACHE.clear();
            }
            AUTHORITY_CACHE.insert(cache_key, (authority, Instant::now()));
        }
        None => {
            AUTHORITY_CACHE.remove(&cache_key);
        }
    }
    Ok(resolved)
}

async fn preview_authority(
    state: &AppState,
    community: CommunityId,
    channel_id: Uuid,
    session_ref: &str,
) -> Result<Option<PublicKey>, String> {
    let query = EventQuery {
        channel_id: Some(channel_id),
        kinds: Some(vec![KIND_SESSION_PREVIEW_ANNOUNCE as i32]),
        d_tag: Some(session_ref.to_owned()),
        limit: Some(AUTHORITY_QUERY_LIMIT),
        ..EventQuery::for_community(community)
    };
    let stored = state
        .db
        .query_events(&query)
        .await
        .map_err(|error| error.to_string())?;
    let events: Vec<Event> = stored.into_iter().map(|row| row.event).collect();
    Ok(resolve_preview_owner(&events, channel_id, session_ref))
}

async fn device_authority(
    state: &AppState,
    community: CommunityId,
    channel_id: Uuid,
    slot: &str,
) -> Result<Option<PublicKey>, String> {
    let query = EventQuery {
        channel_id: Some(channel_id),
        kinds: Some(vec![KIND_SESSION_DEVICE_RECORD as i32]),
        tags_containing: Some(vec![
            ("sdv-type".to_owned(), "state".to_owned()),
            ("sdv-slot".to_owned(), slot.to_owned()),
        ]),
        limit: Some(AUTHORITY_QUERY_LIMIT),
        ..EventQuery::for_community(community)
    };
    let stored = state
        .db
        .query_events(&query)
        .await
        .map_err(|error| error.to_string())?;
    let events: Vec<Event> = stored.into_iter().map(|row| row.event).collect();
    let Some((event, record)) = newest_device_state_record(&events, channel_id, slot) else {
        return Ok(None);
    };
    let target = parse_coding_session_target_key(&record.target_key)?;
    match state
        .db
        .resolve_coding_session_generation_authority(
            community,
            channel_id,
            &record.lifecycle_command_id,
            &target,
            &event.pubkey,
        )
        .await
    {
        Ok(_) => Ok(Some(event.pubkey)),
        Err(beekeeper_db::DbError::AccessDenied(_)) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    #[test]
    fn only_the_announced_producer_is_admitted() {
        let producer = Keys::generate().public_key();
        let other = Keys::generate().public_key();
        assert!(frame_author_admitted(&producer, Some(&producer)));
        assert!(!frame_author_admitted(&other, Some(&producer)));
        // No announce, no record: nobody may send frames.
        assert!(!frame_author_admitted(&producer, None));
    }
}
