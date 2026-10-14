//! Provider presence heartbeats (kind 24224, NIP-CSL § Provider presence).
//!
//! A lease (24223) says "this exact generation has a live actor". It says
//! nothing when there is no execution at all, so an idle provider and a dead
//! one read the same from the relay. Presence closes that gap: every
//! `ttlSecs / 3` while it runs — idle included — the provider signs one
//! heartbeat per subscribed channel naming its instance, a monotonic `seq`,
//! the TTL, whether a turn is in flight, and the live targets in that
//! channel. It rides the same latest-ephemeral handoff the leases use
//! (`lease.rs`), so a heartbeat that could not be delivered is replaced by
//! the next rather than queued behind it.

use std::collections::HashMap;

use beekeeper_sdk::coding_session::{
    build_coding_session_provider_presence, coding_session_provider_presence_semantic_key,
    ProviderPresence, ProviderPresenceState, DEFAULT_PROVIDER_PRESENCE_TTL_SECS,
};
use nostr::Event;
use uuid::Uuid;

use crate::Provider;

/// Heartbeat period: a third of the TTL, so two missed ticks still read live.
pub(crate) const PRESENCE_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(DEFAULT_PROVIDER_PRESENCE_TTL_SECS / 3);

/// Per-process heartbeat bookkeeping.
#[derive(Debug, Default)]
pub(crate) struct PresenceClock {
    /// Last `seq` signed; the next heartbeat is one higher.
    seq: u64,
    /// Latest signed heartbeat per semantic key not yet handed to the relay.
    pending: HashMap<String, Event>,
}

impl PresenceClock {
    /// Heartbeats waiting for the relay task.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> impl Iterator<Item = (&String, &Event)> {
        self.pending.iter()
    }
}

/// The presence ticker: first heartbeat one period in, never bursting.
pub(crate) fn ticker() -> tokio::time::Interval {
    let mut interval = tokio::time::interval_at(
        tokio::time::Instant::now() + PRESENCE_INTERVAL,
        PRESENCE_INTERVAL,
    );
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval
}

impl Provider {
    /// Sign one heartbeat per subscribed channel from the current registry:
    /// `busy` when any live session in the channel has an open turn, and
    /// `liveTargets` naming every live, unclosed generation there.
    pub(crate) fn queue_presence(&mut self) -> anyhow::Result<usize> {
        let channels: Vec<Uuid> = self.subscribed.iter().copied().collect();
        if channels.is_empty() {
            return Ok(0);
        }
        let live_ids: Vec<String> = self
            .sessions
            .live_session_ids()
            .map(str::to_owned)
            .collect();
        let mut queued = 0usize;
        for channel_id in channels {
            let mut live_targets = Vec::new();
            let mut busy = false;
            for session_id in &live_ids {
                let Some(record) = self.state.session(session_id) else {
                    continue;
                };
                if record.closed || record.channel_id != channel_id {
                    continue;
                }
                busy |= record.open_turn.is_some();
                live_targets.push(self.target_for(record));
            }
            let state = if busy {
                ProviderPresenceState::Busy
            } else {
                ProviderPresenceState::Idle
            };
            let seq = self.presence.seq.saturating_add(1);
            let presence = ProviderPresence::new(
                &self.config.instance_id,
                seq,
                DEFAULT_PROVIDER_PRESENCE_TTL_SECS,
                state,
                live_targets,
            )
            .map_err(anyhow::Error::msg)?;
            let event = build_coding_session_provider_presence(channel_id, &presence)?
                .sign_with_keys(&self.config.keys)?;
            self.presence.seq = seq;
            let key =
                coding_session_provider_presence_semantic_key(channel_id, &self.config.instance_id);
            self.presence.pending.insert(key, event);
            queued += 1;
        }
        Ok(queued)
    }

    /// Hand every pending heartbeat to the relay task, newest per channel.
    pub(crate) async fn flush_pending_presence(
        &mut self,
        publisher: &beekeeper_acp::relay::RelayEventPublisher,
    ) -> anyhow::Result<usize> {
        let mut keys: Vec<String> = self.presence.pending.keys().cloned().collect();
        keys.sort_unstable();
        let mut published = 0usize;
        for key in keys {
            let Some(event) = self.presence.pending.get(&key).cloned() else {
                continue;
            };
            publisher
                .publish_latest_ephemeral(key.clone(), event.clone())
                .await?;
            if self
                .presence
                .pending
                .get(&key)
                .is_some_and(|pending| pending.id == event.id)
            {
                self.presence.pending.remove(&key);
            }
            published += 1;
        }
        Ok(published)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_sdk::coding_session::validate_provider_presence_envelope;

    #[test]
    fn presence_period_is_a_third_of_the_ttl() {
        assert_eq!(PRESENCE_INTERVAL, std::time::Duration::from_secs(30));
        assert_eq!(DEFAULT_PROVIDER_PRESENCE_TTL_SECS, 90);
    }

    #[tokio::test]
    async fn an_idle_provider_heartbeats_every_subscribed_channel_with_a_rising_seq() {
        let dir = tempfile::tempdir().expect("fixture");
        let mut provider = crate::tests::provider(&dir.path().join("state"), None);
        assert_eq!(provider.queue_presence().expect("nothing subscribed"), 0);
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        provider.subscribed.insert(a);
        provider.subscribed.insert(b);
        assert_eq!(provider.queue_presence().expect("two channels"), 2);
        let now = nostr::Timestamp::now().as_secs();
        let mut seqs = Vec::new();
        for (key, event) in provider.presence.pending() {
            assert_eq!(event.pubkey, provider.config.keys.public_key());
            event.verify().expect("provider signature");
            let validated = validate_provider_presence_envelope(event, now).expect("envelope");
            assert!(validated.live);
            assert!([a, b].contains(&validated.channel_id));
            assert_eq!(validated.payload.instance_id, provider.config.instance_id);
            assert_eq!(validated.payload.state, ProviderPresenceState::Idle);
            assert!(validated.payload.live_targets.is_empty());
            assert_eq!(
                *key,
                coding_session_provider_presence_semantic_key(
                    validated.channel_id,
                    &provider.config.instance_id
                )
            );
            seqs.push(validated.payload.seq);
        }
        seqs.sort_unstable();
        assert_eq!(seqs, [1, 2]);
        // A second tick replaces, never accumulates: one pending per channel.
        assert_eq!(provider.queue_presence().expect("again"), 2);
        assert_eq!(provider.presence.pending().count(), 2);
        let newest: Vec<u64> = provider
            .presence
            .pending()
            .map(|(_, e)| {
                validate_provider_presence_envelope(e, now)
                    .expect("envelope")
                    .payload
                    .seq
            })
            .collect();
        assert!(newest.iter().all(|seq| *seq >= 3), "{newest:?}");
    }

    #[tokio::test]
    async fn a_live_execution_with_an_open_turn_reads_busy_and_is_listed() {
        let dir = tempfile::tempdir().expect("fixture");
        let cwd = dir.path().join("ordinary-work");
        std::fs::create_dir(&cwd).expect("workspace");
        let channel = Uuid::new_v4();
        let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&dir.path().join("state"), Some(&projects));
        let event = crate::tests::create_event(&provider, channel, "create-presence");
        provider
            .handle_command_event(channel, &event)
            .await
            .expect("real ACP child create");
        provider.subscribed.insert(channel);
        let target = provider
            .state
            .sessions()
            .next()
            .expect("session")
            .target(&provider.config.instance_id);
        let now = nostr::Timestamp::now().as_secs();
        let read = |provider: &Provider| {
            let (_, event) = provider.presence.pending().next().expect("heartbeat");
            validate_provider_presence_envelope(event, now)
                .expect("envelope")
                .payload
        };
        provider.queue_presence().expect("idle heartbeat");
        let idle = read(&provider);
        assert_eq!(idle.state, ProviderPresenceState::Idle);
        assert_eq!(idle.live_targets, vec![target.clone()]);
        provider
            .state
            .update_session(&target.session_id, |record| {
                record.open_turn = Some(crate::state::OpenTurn {
                    turn_id: "turn-1".into(),
                    command_id: Some("turn-cmd-1".into()),
                    team_wake_eligible: false,
                    started_at_ms: 1,
                    operator_pubkey: None,
                });
            })
            .expect("open turn");
        provider.queue_presence().expect("busy heartbeat");
        let busy = read(&provider);
        assert_eq!(busy.state, ProviderPresenceState::Busy);
        assert_eq!(busy.live_targets, vec![target.clone()]);
        assert!(busy.seq > idle.seq);
        provider
            .sessions
            .shutdown_and_join(&target.session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
}
