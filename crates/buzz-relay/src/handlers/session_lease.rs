//! Dedicated WebSocket admission for provider-signed coding-session leases.

use std::sync::Arc;

use buzz_core::coding_session_command::coding_session_target_key;
use buzz_core::coding_session_lease::validate_coding_session_lease_envelope;
use buzz_core::kind::KIND_CODING_SESSION_LEASE;
use buzz_core::verification::verify_event;
use buzz_pubsub::session_lease::{LeaseApplyOutcome, SessionLeaseProof};
use nostr::Event;
use uuid::Uuid;

use crate::connection::ConnectionState;
use crate::protocol::RelayMessage;
use crate::state::AppState;

/// Validate authority and atomically register one kind-24223 WebSocket event.
///
/// This seam is deliberately outside generic ephemeral handling: the signed
/// event is never written to Postgres, and a positive `OK` is sent only after
/// immutable lifecycle proof has resolved and Redis has accepted the register.
// Transport context arrives as separate values from the existing event handler;
// bundling it here would duplicate `ConnectionState` and obscure which fields
// are authenticated versus event-authored.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_session_lease_event(
    event: Event,
    conn_id: Uuid,
    pubkey_bytes: Vec<u8>,
    token_channel_ids: Option<Vec<Uuid>>,
    event_id_hex: &str,
    conn: Arc<ConnectionState>,
    state: Arc<AppState>,
) {
    if let Err(message) = admit_session_lease(
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

async fn admit_session_lease(
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
    let lease = validate_coding_session_lease_envelope(&event, relay_now)
        .map_err(|error| format!("invalid: {error}"))?;
    if token_channel_ids.is_some_and(|allowed| !allowed.contains(&lease.channel_id)) {
        return Err("restricted: token does not have access to this channel".into());
    }
    super::ingest::check_coding_session_membership(
        &conn.tenant,
        &state,
        lease.channel_id,
        pubkey_bytes,
        KIND_CODING_SESSION_LEASE,
        &event,
    )
    .await?;

    let authority = state
        .db
        .resolve_coding_session_generation_authority(
            conn.tenant.community(),
            lease.channel_id,
            &lease.command_id,
            &lease.payload.target,
            &event.pubkey,
        )
        .await
        .map_err(|error| format!("restricted: lease authority not established: {error}"))?;
    let proof = SessionLeaseProof {
        authority_command_event_id: authority.command_event_id.to_hex(),
        authority_receipt_event_id: authority.receipt_event_id.to_hex(),
    };
    let target_key = coding_session_target_key(&lease.payload.target);
    let outcome = state
        .pubsub
        .apply_session_lease(
            &conn.tenant,
            lease.channel_id,
            &target_key,
            &event,
            lease.payload.lease_sequence,
            lease.payload.state,
            &proof,
        )
        .await
        .map_err(|error| format!("error: session lease register failed: {error}"))?;

    match outcome {
        LeaseApplyOutcome::Applied(_) => {
            super::event::fan_out_admitted_channel_ephemeral(
                event,
                lease.channel_id,
                conn_id,
                &state,
                &conn.tenant,
            )
            .await;
            Ok(())
        }
        LeaseApplyOutcome::Duplicate(_) => Ok(()),
        LeaseApplyOutcome::Conflict(_) => {
            Err("invalid: conflicting event for existing lease sequence".into())
        }
        LeaseApplyOutcome::Stale(_) => Err("invalid: stale lease sequence".into()),
    }
}
