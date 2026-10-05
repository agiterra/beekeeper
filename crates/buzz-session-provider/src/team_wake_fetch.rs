//! Team-wake relay reads, run off the provider's run loop and bounded.
//!
//! A team wake is decided from a complete, verified read of the umbrella's
//! signed facts: a genesis, an authority chain, a kind-44244 partition and a
//! context package of a dozen paged kind partitions. On a busy channel or a
//! slow link that read takes tens of seconds, and a single request can cost
//! four attempts of ten seconds each before it gives up.
//!
//! That read used to be awaited inside the run loop's tick arm. The loop is a
//! single `select!`, so while it waited nothing else ran — no session event was
//! recorded and no transcript row (44225) was published — and because every
//! tick that outlives the two-second runtime tick leaves the interval already due,
//! the next tick won the biased select straight away and the publish arm was
//! starved for as long as the relay stayed slow. Observed 2026-10-05: a live
//! session's transcript stopped for four and a half minutes while its agent
//! kept working, behind repeated `context relay query failed` team-wake reads.
//!
//! Here the read runs as one spawned task at a time. The loop only starts it
//! ([`Provider::start_team_wake_tick`]) and applies its answer when it arrives
//! ([`Provider::finish_team_wake_fetch`]); both halves are short and touch no
//! network except the wake publish itself, which the publisher already bounds.
//! Every read, spawned or inline, is bounded by [`TEAM_WAKE_FETCH_TIMEOUT`],
//! and a read that runs out of time is an ordinary transient failure: the
//! intent stays durable and is retried after its backoff.

use std::time::Duration;

use buzz_acp::relay::{RelayEventPublisher, RestClient};
use buzz_core::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::Event;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::context_projector::{self, ContextProjectionError};
use crate::team_wake::{self, VerifiedWakeSnapshot, WakeIntent, WakeSnapshotError};
use crate::Provider;

/// The longest one team-wake read may take, end to end, before it is treated
/// as a transient failure.
///
/// Generous on purpose: a complete read is many paged requests, and a read cut
/// short is retried from the beginning. It exists so a relay that accepts the
/// connection and never answers cannot hold the team-wake slot forever.
pub(crate) const TEAM_WAKE_FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// One relay read the team-wake scheduler needs before it can decide.
pub(crate) enum TeamWakeFetch {
    /// The channel's complete stored kind-44244 partition.
    Discovery { channel_id: Uuid, reprobing: bool },
    /// The verified snapshot an in-flight intent is judged against.
    Snapshot {
        channel_id: Uuid,
        intent: Box<WakeIntent>,
        relay_self: String,
    },
}

/// A finished read, delivered back to the run loop.
pub(crate) enum TeamWakeFetched {
    Discovery {
        channel_id: Uuid,
        reprobing: bool,
        result: Result<Vec<Event>, ContextProjectionError>,
    },
    Snapshot {
        channel_id: Uuid,
        intent: Box<WakeIntent>,
        result: Box<Result<VerifiedWakeSnapshot, WakeSnapshotError>>,
    },
}

/// The single slot a spawned team-wake read occupies.
pub(crate) struct TeamWakeFetchSlot {
    tx: mpsc::Sender<TeamWakeFetched>,
    rx: Option<mpsc::Receiver<TeamWakeFetched>>,
    task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) timeout: Duration,
}

impl TeamWakeFetchSlot {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel(1);
        Self {
            tx,
            rx: Some(rx),
            task: None,
            timeout: TEAM_WAKE_FETCH_TIMEOUT,
        }
    }

    /// Whether a read is still running. A task that ended without reporting
    /// (it cannot, short of a panic) frees the slot rather than wedging it.
    pub(crate) fn busy(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }
}

impl Drop for TeamWakeFetchSlot {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Wait for the next finished read, or forever when the receiver is gone.
pub(crate) async fn next_fetched(
    rx: &mut Option<mpsc::Receiver<TeamWakeFetched>>,
) -> TeamWakeFetched {
    match rx.as_mut() {
        Some(rx) => match rx.recv().await {
            Some(fetched) => fetched,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}

fn timed_out(timeout: Duration) -> String {
    format!(
        "team-wake relay read did not finish within {}s",
        timeout.as_secs_f64()
    )
}

/// The channel's complete team-transaction partition, bounded by `timeout`.
pub(crate) async fn fetch_discovery(
    rest: &RestClient,
    channel_id: Uuid,
    timeout: Duration,
) -> Result<Vec<Event>, ContextProjectionError> {
    match tokio::time::timeout(
        timeout,
        context_projector::query_complete_kind_partition(
            rest,
            channel_id,
            KIND_CODING_SESSION_TEAM_TRANSACTION,
        ),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(ContextProjectionError::Relay(timed_out(timeout))),
    }
}

/// The verified wake snapshot for `intent`, bounded by `timeout`.
pub(crate) async fn fetch_snapshot(
    rest: &RestClient,
    relay_self: &str,
    intent: &WakeIntent,
    timeout: Duration,
) -> Result<VerifiedWakeSnapshot, WakeSnapshotError> {
    match tokio::time::timeout(
        timeout,
        team_wake::fetch_verified_snapshot(rest, relay_self, &intent.scope),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(ContextProjectionError::Relay(timed_out(timeout)).into()),
    }
}

async fn run_fetch(rest: RestClient, fetch: TeamWakeFetch, timeout: Duration) -> TeamWakeFetched {
    match fetch {
        TeamWakeFetch::Discovery {
            channel_id,
            reprobing,
        } => TeamWakeFetched::Discovery {
            channel_id,
            reprobing,
            result: fetch_discovery(&rest, channel_id, timeout).await,
        },
        TeamWakeFetch::Snapshot {
            channel_id,
            intent,
            relay_self,
        } => {
            let result = Box::new(fetch_snapshot(&rest, &relay_self, &intent, timeout).await);
            TeamWakeFetched::Snapshot {
                channel_id,
                intent,
                result,
            }
        }
    }
}

impl Provider {
    /// The run loop's half of [`Provider::take_team_wake_fetches`]: the queue
    /// finished reads arrive on, held outside the provider for the same borrow
    /// reason as the CI listener's.
    pub(crate) fn take_team_wake_fetches(&mut self) -> Option<mpsc::Receiver<TeamWakeFetched>> {
        self.team_wake_fetch.rx.take()
    }

    /// Whether a team-wake read is in flight.
    #[cfg(test)]
    pub(crate) fn team_wake_fetch_busy(&self) -> bool {
        self.team_wake_fetch.busy()
    }

    fn spawn_team_wake_fetch(&mut self, rest: RestClient, fetch: TeamWakeFetch) {
        let tx = self.team_wake_fetch.tx.clone();
        let timeout = self.team_wake_fetch.timeout;
        self.team_wake_fetch.task = Some(tokio::spawn(async move {
            let fetched = run_fetch(rest, fetch, timeout).await;
            let _ = tx.send(fetched).await;
        }));
    }

    /// Start the next team-wake read without waiting for it.
    ///
    /// The same channel choice as the inline `run_one_team_wake_tick`; the
    /// difference is that the relay read runs on its own task and its answer is
    /// applied by [`Provider::finish_team_wake_fetch`] when it arrives. At most
    /// one read is in flight, so relay work stays one channel per tick.
    pub(crate) fn start_team_wake_tick(&mut self) -> anyhow::Result<()> {
        if self.team_wake_fetch.busy() {
            return Ok(());
        }
        let Some(channel_id) = self.next_team_wake_channel()? else {
            return Ok(());
        };
        if self.team_wake_discovery_needed(channel_id) {
            if let Some((rest, reprobing)) = self.prepare_team_wake_discovery(channel_id) {
                self.spawn_team_wake_fetch(
                    rest,
                    TeamWakeFetch::Discovery {
                        channel_id,
                        reprobing,
                    },
                );
                return Ok(());
            }
        }
        self.start_team_wake_snapshot(channel_id)
    }

    fn start_team_wake_snapshot(&mut self, channel_id: Uuid) -> anyhow::Result<()> {
        // One read in flight: called from `finish_team_wake_fetch`, another
        // read may already be running (one a tick started while this answer
        // waited in the queue). Its next round-robin turn reads this channel.
        if self.team_wake_fetch.busy() || !self.team_wake_processing_needed(channel_id) {
            return Ok(());
        }
        if let Some((rest, relay_self, intent)) = self.prepare_team_wake_snapshot(channel_id)? {
            self.spawn_team_wake_fetch(
                rest,
                TeamWakeFetch::Snapshot {
                    channel_id,
                    intent: Box::new(intent),
                    relay_self,
                },
            );
        }
        Ok(())
    }

    /// Apply a finished read on the run loop.
    ///
    /// A snapshot is applied only to the intent it was read for: if the
    /// channel's in-flight intent changed while the read ran, the answer is
    /// dropped and the next tick reads again for what is there now.
    pub(crate) async fn finish_team_wake_fetch(
        &mut self,
        fetched: TeamWakeFetched,
        publisher: &RelayEventPublisher,
    ) -> anyhow::Result<()> {
        // Clear the slot only if its task is done. A tick can start the next
        // read after this one finished but before its answer was taken off
        // the queue; dropping that newer handle would detach a running read
        // and let a further tick start a second one beside it.
        if !self.team_wake_fetch.busy() {
            self.team_wake_fetch.task = None;
        }
        match fetched {
            TeamWakeFetched::Discovery {
                channel_id,
                reprobing,
                result,
            } => {
                if !self.subscribed.contains(&channel_id) {
                    return Ok(());
                }
                self.apply_team_wake_discovery(channel_id, reprobing, result);
                // The tick that chose this channel would have gone on to
                // process it; do the same now that the read is in.
                self.start_team_wake_snapshot(channel_id)
            }
            TeamWakeFetched::Snapshot {
                channel_id,
                intent,
                result,
            } => {
                let current = self.team_wakes.pending_for_channel(channel_id)?;
                if current.as_ref() != Some(intent.as_ref()) {
                    return Ok(());
                }
                self.apply_team_wake_snapshot(channel_id, *intent, *result, publisher)
                    .await
            }
        }
    }
}
