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
//!
//! **Spread, not burst (ledger 406 F8).** The 30 s period is cut into
//! [`PRESENCE_SLOTS`] one-second slots and each subscribed channel is
//! assigned one slot by its position in the sorted channel list, so a
//! provider in N channels sends at most `ceil(N / 30)` frames in any second
//! instead of N at once. The relay admits at most ten ephemeral frames per
//! key per second; a provider in more than 300 channels cannot keep every
//! channel inside 30 s at that rate, so the period widens to `ceil(N / 10)`
//! seconds and the widening is logged. Above 900 channels that period
//! exceeds the 90 s TTL and presence will read expired between beats; the
//! provider says so rather than exceeding the admission rate.

use std::collections::HashMap;

use beekeeper_sdk::coding_session::{
    build_coding_session_provider_presence, coding_session_provider_presence_semantic_key,
    ProviderPresence, ProviderPresenceState, DEFAULT_PROVIDER_PRESENCE_TTL_SECS,
};
use nostr::Event;
use uuid::Uuid;

use crate::Provider;

/// Heartbeat period per channel: a third of the TTL, so two missed ticks
/// still read live.
pub(crate) const PRESENCE_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(DEFAULT_PROVIDER_PRESENCE_TTL_SECS / 3);

/// Slots the period is spread across; one slot per ticker tick.
pub(crate) const PRESENCE_SLOTS: u64 = DEFAULT_PROVIDER_PRESENCE_TTL_SECS / 3;

/// The ticker period: one slot.
pub(crate) const PRESENCE_SLOT: std::time::Duration =
    std::time::Duration::from_secs(PRESENCE_INTERVAL.as_secs() / PRESENCE_SLOTS);

/// Most heartbeats one tick may sign: the relay's per-key ephemeral
/// admission ceiling of ten frames a second
/// (`beekeeper-relay/src/handlers/event.rs`, per-kind per-pubkey gate).
pub(crate) const MAX_PRESENCE_FRAMES_PER_SLOT: u64 = 10;

/// Per-process heartbeat bookkeeping.
#[derive(Debug, Default)]
pub(crate) struct PresenceClock {
    /// Last `seq` signed; the next heartbeat is one higher.
    seq: u64,
    /// Latest signed heartbeat per semantic key not yet handed to the relay.
    pending: HashMap<String, Event>,
    /// Ticks taken so far; the current slot is this modulo the slot count.
    tick: u64,
    /// The slot count last used, so a widening is logged once per change.
    slots_in_use: u64,
}

impl PresenceClock {
    /// Heartbeats waiting for the relay task.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> impl Iterator<Item = (&String, &Event)> {
        self.pending.iter()
    }
}

/// The slot count that keeps `channels` channels within the admission
/// ceiling: [`PRESENCE_SLOTS`] until that would need more than
/// [`MAX_PRESENCE_FRAMES_PER_SLOT`] frames in one slot, then wider.
pub(crate) fn slots_for(channels: u64) -> u64 {
    PRESENCE_SLOTS.max(channels.div_ceil(MAX_PRESENCE_FRAMES_PER_SLOT))
}

/// The channels due in `slot` of `slots`: every `slots`-th entry of the
/// sorted list starting at `slot`, so no slot holds more than
/// `ceil(N / slots)` channels.
pub(crate) fn due_in_slot(channels: &[Uuid], slot: u64, slots: u64) -> Vec<Uuid> {
    channels
        .iter()
        .enumerate()
        .filter(|(index, _)| (*index as u64) % slots == slot)
        .map(|(_, channel)| *channel)
        .collect()
}

/// The presence ticker: one slot per tick, first tick one slot in, never
/// bursting after a stall.
pub(crate) fn ticker() -> tokio::time::Interval {
    let mut interval =
        tokio::time::interval_at(tokio::time::Instant::now() + PRESENCE_SLOT, PRESENCE_SLOT);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval
}

impl Provider {
    /// One ticker tick: sign the heartbeats of the channels whose slot this
    /// is. Returns how many were queued; a tick whose slot holds no channel
    /// queues none.
    pub(crate) fn queue_presence(&mut self) -> anyhow::Result<usize> {
        let channels: Vec<Uuid> = self.subscribed.iter().copied().collect();
        let tick = self.presence.tick;
        self.presence.tick = tick.wrapping_add(1);
        if channels.is_empty() {
            return Ok(0);
        }
        let slots = slots_for(channels.len() as u64);
        if slots != self.presence.slots_in_use {
            if slots > PRESENCE_SLOTS {
                tracing::warn!(
                    target: "csp::presence",
                    channels = channels.len(),
                    period_secs = slots,
                    ttl_secs = DEFAULT_PROVIDER_PRESENCE_TTL_SECS,
                    "presence period widened past {}s to stay under the relay's {MAX_PRESENCE_FRAMES_PER_SLOT}/s admission; presence reads expired between beats once the period exceeds the TTL",
                    PRESENCE_INTERVAL.as_secs()
                );
            }
            self.presence.slots_in_use = slots;
        }
        let due = due_in_slot(&channels, tick % slots, slots);
        self.queue_presence_for(&due)
    }

    /// Sign one heartbeat for each of `channels` from the current registry:
    /// `busy` when any live session in the channel has an open turn, and
    /// `liveTargets` naming every live, unclosed generation there.
    pub(crate) fn queue_presence_for(&mut self, channels: &[Uuid]) -> anyhow::Result<usize> {
        if channels.is_empty() {
            return Ok(0);
        }
        let live_ids: Vec<String> = self
            .sessions
            .live_session_ids()
            .map(str::to_owned)
            .collect();
        let mut queued = 0usize;
        for channel_id in channels.iter().copied() {
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
    fn presence_period_is_a_third_of_the_ttl_cut_into_one_second_slots() {
        assert_eq!(PRESENCE_INTERVAL, std::time::Duration::from_secs(30));
        assert_eq!(DEFAULT_PROVIDER_PRESENCE_TTL_SECS, 90);
        assert_eq!(PRESENCE_SLOTS, 30);
        assert_eq!(PRESENCE_SLOT, std::time::Duration::from_secs(1));
        assert_eq!(MAX_PRESENCE_FRAMES_PER_SLOT, 10);
    }

    /// Every channel lands in exactly one slot of the period, and no slot
    /// holds more than `ceil(N / slots)`; past 300 channels the period
    /// widens so no slot ever holds more than ten.
    #[test]
    fn slots_spread_channels_evenly_and_never_past_ten_a_second() {
        for n in [1usize, 2, 29, 30, 31, 40, 299, 300, 301, 400, 901] {
            let channels: Vec<Uuid> = (0..n).map(|_| Uuid::new_v4()).collect();
            let slots = slots_for(n as u64);
            if n <= 300 {
                assert_eq!(slots, 30, "{n} channels fit the 30 s period");
            } else {
                assert_eq!(
                    slots,
                    (n as u64).div_ceil(10),
                    "{n} channels widen the period"
                );
            }
            let mut seen: Vec<Uuid> = Vec::new();
            for slot in 0..slots {
                let due = due_in_slot(&channels, slot, slots);
                assert!(
                    due.len() as u64 <= (n as u64).div_ceil(slots),
                    "{n} channels, slot {slot} of {slots}: {} due",
                    due.len()
                );
                assert!(due.len() as u64 <= MAX_PRESENCE_FRAMES_PER_SLOT);
                seen.extend(due);
            }
            seen.sort_unstable();
            let mut all = channels.clone();
            all.sort_unstable();
            assert_eq!(seen, all, "{n} channels: each heartbeats once per period");
        }
    }

    /// F8 vector: a provider in 40 channels sends at most 2 frames on any
    /// tick, every channel exactly once over the 30 ticks of a period, with a
    /// rising `seq`; the next period replaces rather than accumulates.
    #[tokio::test]
    async fn forty_channels_heartbeat_at_most_two_a_tick_and_each_once_a_period() {
        let dir = tempfile::tempdir().expect("fixture");
        let mut provider = crate::tests::provider(&dir.path().join("state"), None);
        assert_eq!(provider.queue_presence().expect("nothing subscribed"), 0);
        let channels: Vec<Uuid> = (0..40).map(|_| Uuid::new_v4()).collect();
        for channel in &channels {
            provider.subscribed.insert(*channel);
        }
        let cap = 40u64.div_ceil(PRESENCE_SLOTS) as usize;
        assert_eq!(cap, 2);
        let mut per_tick = Vec::new();
        for _ in 0..PRESENCE_SLOTS {
            let queued = provider.queue_presence().expect("tick");
            assert!(queued <= cap, "a tick queued {queued} > {cap}");
            assert!(queued as u64 <= MAX_PRESENCE_FRAMES_PER_SLOT);
            per_tick.push(queued);
        }
        assert_eq!(per_tick.iter().sum::<usize>(), 40, "{per_tick:?}");
        assert_eq!(provider.presence.pending().count(), 40);
        let now = nostr::Timestamp::now().as_secs();
        let mut seqs = Vec::new();
        let mut seen = Vec::new();
        for (key, event) in provider.presence.pending() {
            assert_eq!(event.pubkey, provider.config.keys.public_key());
            event.verify().expect("provider signature");
            let validated = validate_provider_presence_envelope(event, now).expect("envelope");
            assert!(validated.live);
            assert!(channels.contains(&validated.channel_id));
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
            seen.push(validated.channel_id);
        }
        seqs.sort_unstable();
        assert_eq!(seqs, (1..=40).collect::<Vec<u64>>());
        seen.sort_unstable();
        let mut all = channels.clone();
        all.sort_unstable();
        assert_eq!(seen, all);
        // The next period's first tick re-signs slot 0's channels only, and
        // replaces rather than accumulates: still one pending per channel.
        assert_eq!(provider.queue_presence().expect("period 2"), per_tick[0]);
        assert_eq!(provider.presence.pending().count(), 40);
        let newest = provider
            .presence
            .pending()
            .map(|(_, e)| {
                validate_provider_presence_envelope(e, now)
                    .expect("envelope")
                    .payload
                    .seq
            })
            .max()
            .expect("some");
        assert_eq!(newest as usize, 40 + per_tick[0]);
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
        // One channel sits in slot 0: the first tick signs it.
        assert_eq!(provider.queue_presence().expect("idle heartbeat"), 1);
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
        // Slot 1 holds no channel: the next tick signs nothing.
        assert_eq!(provider.queue_presence().expect("empty slot"), 0);
        provider
            .queue_presence_for(&[channel])
            .expect("busy heartbeat");
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
