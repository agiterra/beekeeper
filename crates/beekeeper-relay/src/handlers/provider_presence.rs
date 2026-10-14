//! Dedicated WebSocket admission for provider-signed presence heartbeats
//! (kind 24224, NIP-CSL § Provider presence).
//!
//! The same seam as [`super::session_lease`]: the signed event is never
//! written to Postgres; it is validated, gated on channel membership, placed
//! in the Redis presence register, and fanned out to channel subscribers only
//! when the register accepted it. A cold `REQ` for kind 24224 is answered
//! from that register (`super::req`), so an idle provider stays readable
//! between heartbeats and a dead one reads as expired, then absent.

use std::sync::Arc;

use beekeeper_core::kind::KIND_CODING_SESSION_PROVIDER_PRESENCE;
use beekeeper_core::verification::verify_event;
use beekeeper_pubsub::provider_presence::PresenceApplyOutcome;
use beekeeper_sdk::coding_session::validate_provider_presence_envelope;
use nostr::Event;
use uuid::Uuid;

use crate::connection::ConnectionState;
use crate::protocol::RelayMessage;
use crate::state::AppState;

/// Seconds a heartbeat's `created_at` may sit ahead of the relay clock.
/// Same allowance the lease envelope grants.
const MAX_FUTURE_SKEW_SECS: u64 =
    beekeeper_core::coding_session_lease::CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS;

/// Validate and atomically register one kind-24224 WebSocket event.
// Transport context arrives as separate values from the existing event handler,
// exactly as for leases.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_provider_presence_event(
    event: Event,
    conn_id: Uuid,
    pubkey_bytes: Vec<u8>,
    token_channel_ids: Option<Vec<Uuid>>,
    event_id_hex: &str,
    conn: Arc<ConnectionState>,
    state: Arc<AppState>,
) {
    if let Err(message) = admit_provider_presence(
        event,
        conn_id,
        &pubkey_bytes,
        token_channel_ids.as_deref(),
        Arc::clone(&conn),
        state,
    )
    .await
    {
        conn.send(RelayMessage::ok(event_id_hex, false, &message));
        return;
    }
    conn.send(RelayMessage::ok(event_id_hex, true, ""));
}

async fn admit_provider_presence(
    event: Event,
    conn_id: Uuid,
    pubkey_bytes: &[u8],
    token_channel_ids: Option<&[Uuid]>,
    conn: Arc<ConnectionState>,
    state: Arc<AppState>,
) -> Result<(), String> {
    let event_to_verify = event.clone();
    match tokio::task::spawn_blocking(move || verify_event(&event_to_verify)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(format!("invalid: {error}")),
        Err(_) => return Err("error: internal error".into()),
    }

    let relay_now = u64::try_from(chrono::Utc::now().timestamp())
        .map_err(|_| "error: relay clock is before Unix epoch".to_string())?;
    let presence = validate_provider_presence_envelope(&event, relay_now)
        .map_err(|error| format!("invalid: {error}"))?;
    if event.created_at.as_secs() > relay_now.saturating_add(MAX_FUTURE_SKEW_SECS) {
        return Err("invalid: provider-presence created_at is too far in the future".into());
    }
    if !presence.live {
        // A heartbeat that is already past its own TTL says nothing about now.
        return Err("invalid: provider-presence heartbeat is already expired".into());
    }
    if token_channel_ids.is_some_and(|allowed| !allowed.contains(&presence.channel_id)) {
        return Err("restricted: token does not have access to this channel".into());
    }
    super::ingest::check_coding_session_membership(
        &conn.tenant,
        &state,
        presence.channel_id,
        pubkey_bytes,
        KIND_CODING_SESSION_PROVIDER_PRESENCE,
        &event,
    )
    .await?;

    let outcome = state
        .pubsub
        .apply_provider_presence(
            &conn.tenant,
            presence.channel_id,
            &event.pubkey.to_hex(),
            &presence.payload.instance_id,
            &event,
            presence.payload.seq,
            presence.payload.ttl_secs,
        )
        .await
        .map_err(|error| format!("error: provider presence register failed: {error}"))?;

    match outcome {
        PresenceApplyOutcome::Applied(_) => {
            super::event::fan_out_admitted_channel_ephemeral(
                event,
                presence.channel_id,
                conn_id,
                &state,
                &conn.tenant,
            )
            .await;
            Ok(())
        }
        PresenceApplyOutcome::Duplicate(_) => Ok(()),
        PresenceApplyOutcome::Conflict(_) => {
            Err("invalid: conflicting event for existing presence seq".into())
        }
        PresenceApplyOutcome::Stale(_) => Err("invalid: stale presence seq".into()),
    }
}
