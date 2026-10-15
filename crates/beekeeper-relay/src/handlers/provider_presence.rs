//! Dedicated WebSocket admission for provider-signed presence heartbeats
//! (kind 24224, NIP-CSL § Provider presence).
//!
//! The same seam as [`super::session_lease`]: the signed event is never
//! written to Postgres; it is validated, gated on channel membership, placed
//! in the Redis presence register, and fanned out to channel subscribers only
//! when the register accepted it. A cold `REQ` for kind 24224 is answered
//! from that register (`super::req`), so an idle provider stays readable
//! between heartbeats and a dead one reads as expired, then absent.
//!
//! Admission is channel membership (ledger 406 F4): the relay does not look
//! up whether the signer has authored a 44223 here — that is a Postgres
//! query per heartbeat on a path that runs every second — so instead the
//! register caps live instance keys per (channel, pubkey) at
//! [`PROVIDER_PRESENCE_INSTANCE_CAP`] and the ninth is refused with
//! [`PRESENCE_INSTANCE_CAP_REFUSAL`]. Readers still verify the provider key
//! and `instanceId` against the session's 44223, so a stray heartbeat never
//! makes a session read `live`.

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

/// The `OK false` code a capped heartbeat is refused with.
pub(crate) const PRESENCE_INSTANCE_CAP_REFUSAL: &str = "PROVIDER_PRESENCE_INSTANCE_CAP";

/// What a register outcome means for admission: `Ok(true)` fan out,
/// `Ok(false)` accepted silently (replay), `Err` the `OK false` message.
pub(crate) fn admission_of(outcome: &PresenceApplyOutcome) -> Result<bool, String> {
    match outcome {
        PresenceApplyOutcome::Applied(_) => Ok(true),
        PresenceApplyOutcome::Duplicate(_) => Ok(false),
        PresenceApplyOutcome::Conflict(_) => {
            Err("invalid: conflicting event for existing presence seq".into())
        }
        PresenceApplyOutcome::Stale(_) => Err("invalid: stale presence seq".into()),
        PresenceApplyOutcome::Capped {
            live_instances,
            cap,
        } => Err(format!(
            "restricted: {PRESENCE_INSTANCE_CAP_REFUSAL}: this key already holds {live_instances} live presence instances in this channel (cap {cap}); a new instanceId is admitted once one expires"
        )),
    }
}

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

    if admission_of(&outcome)? {
        super::event::fan_out_admitted_channel_ephemeral(
            event,
            presence.channel_id,
            conn_id,
            &state,
            &conn.tenant,
        )
        .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_pubsub::provider_presence::PROVIDER_PRESENCE_INSTANCE_CAP;

    #[test]
    fn a_capped_heartbeat_is_refused_with_the_cap_code_and_nothing_else_changes() {
        let refused = admission_of(&PresenceApplyOutcome::Capped {
            live_instances: 8,
            cap: PROVIDER_PRESENCE_INSTANCE_CAP,
        })
        .expect_err("capped is a refusal");
        assert!(refused.starts_with("restricted: PROVIDER_PRESENCE_INSTANCE_CAP: "));
        assert!(refused.contains("8 live presence instances"));
        assert!(refused.contains("(cap 8)"));
        assert_eq!(PROVIDER_PRESENCE_INSTANCE_CAP, 8);
    }
}
