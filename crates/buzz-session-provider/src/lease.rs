//! Provider-owned ephemeral liveness leases for exact session generations.

use buzz_core::coding_session_command::coding_session_target_key;
use buzz_core::coding_session_lease::{CodingSessionLease, CodingSessionLeaseState};
use nostr::{Event, Keys};

use crate::state::SessionRecord;

/// Signed latest-state publication waiting to be handed to the relay task.
#[derive(Clone)]
pub(crate) struct PendingLease {
    pub(crate) semantic_key: String,
    pub(crate) event: Event,
    pub(crate) state: CodingSessionLeaseState,
}

/// Provider heartbeat period. Relay storage expires a lease after three of
/// these periods, so one delayed tick does not create a false disconnect.
pub(crate) const LEASE_RENEWAL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

pub(crate) fn first_renewal_at(now: tokio::time::Instant) -> tokio::time::Instant {
    now + LEASE_RENEWAL_INTERVAL
}

/// Create the heartbeat clock. The first live lease is lifecycle-driven;
/// periodic renewal begins one full cadence later and missed ticks never burst.
pub(crate) fn renewal_interval() -> tokio::time::Interval {
    let mut interval = tokio::time::interval_at(
        first_renewal_at(tokio::time::Instant::now()),
        LEASE_RENEWAL_INTERVAL,
    );
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval
}

pub(crate) fn renewal_interval_with(period: std::time::Duration) -> tokio::time::Interval {
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval
}

/// Build and sign one exact-generation lease using the lifecycle command that
/// minted that generation.
pub(crate) fn build_signed_lease(
    record: &SessionRecord,
    instance_id: &str,
    keys: &Keys,
    state: CodingSessionLeaseState,
    sequence: u64,
) -> anyhow::Result<(String, Event)> {
    let target = record.target(instance_id);
    let payload =
        CodingSessionLease::new(target.clone(), state, sequence).map_err(anyhow::Error::msg)?;
    let event = buzz_sdk::builders::build_coding_session_lease(
        record.channel_id,
        record.generation_command_id(),
        &payload,
    )?
    .sign_with_keys(keys)?;
    Ok((coding_session_target_key(&target), event))
}

/// Reserve before signing, returning one reconnect-safe publication.
pub(crate) fn reserve_and_build(
    store: &mut crate::state::StateStore,
    session_id: &str,
    instance_id: &str,
    keys: &Keys,
    state: CodingSessionLeaseState,
) -> anyhow::Result<Option<PendingLease>> {
    let Some(record) = store.session(session_id).cloned() else {
        return Ok(None);
    };
    let Some(sequence) = store.allocate_lease_sequence(session_id)? else {
        return Ok(None);
    };
    let (semantic_key, event) = build_signed_lease(&record, instance_id, keys, state, sequence)?;
    Ok(Some(PendingLease {
        semantic_key,
        event,
        state,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use buzz_core::coding_session_lease::{
        validate_coding_session_lease_envelope, CodingSessionLeaseState,
    };
    use nostr::Keys;
    use uuid::Uuid;

    use super::*;

    fn record() -> SessionRecord {
        SessionRecord {
            session_id: "11111111-1111-4111-8111-111111111111".into(),
            generation: 2,
            channel_id: Uuid::nil(),
            command_id: "create-1".into(),
            generation_command_id: Some("resume-2".into()),
            provider_instance_ref: "claude-primary".into(),
            runtime: "claude".into(),
            driver: "claude-agent-acp".into(),
            cwd: PathBuf::from("/checkout"),
            project_ref: None,
            repo_ref: None,
            session_ref: None,
            genesis_ref: None,
            actor: None,
            role: None,
            founder_pubkey: Some("ab".repeat(32)),
            granted_operators: BTreeSet::new(),
            granted_viewers: BTreeSet::new(),
            authority_seq: 0,
            model: None,
            routing: None,
            resume_cursor: None,
            title: None,
            created_at_ms: 1,
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
        }
    }

    #[test]
    fn live_and_released_leases_bind_the_exact_generation_command_and_sequence() {
        let keys = Keys::generate();
        for state in [
            CodingSessionLeaseState::Live,
            CodingSessionLeaseState::Released,
        ] {
            let (semantic_key, event) =
                build_signed_lease(&record(), "instance-1", &keys, state, 7).expect("lease");
            let validated =
                validate_coding_session_lease_envelope(&event, event.created_at.as_secs())
                    .expect("valid lease");

            assert_eq!(validated.command_id, "resume-2");
            assert_eq!(validated.payload.target.generation, 2);
            assert_eq!(validated.payload.lease_sequence, 7);
            assert_eq!(validated.payload.state, state);
            assert_eq!(
                semantic_key,
                buzz_core::coding_session_command::coding_session_target_key(
                    &validated.payload.target
                )
            );
        }
    }

    #[test]
    fn renewal_cadence_waits_sixty_seconds_before_the_first_heartbeat() {
        let now = tokio::time::Instant::now();
        assert_eq!(
            first_renewal_at(now),
            now + std::time::Duration::from_secs(60)
        );
    }
}
