//! How often to ask the host, and what to do when it does not answer.
//!
//! Two decisions, both pure so they can be asserted:
//!
//! - **Cadence.** Fast while work is in flight, slow when idle. A menu bar
//!   that polled at one rate would either lag behind a finishing turn or wake
//!   a laptop for nothing.
//! - **Backoff.** While the socket is absent, back off to a ceiling and reset
//!   on the first success. Without this, a machine that never installed the
//!   host does a socket connect twice a second forever.
//!
//! The elapsed times on the rows are *not* driven by polling. The host sends
//! absolute start instants, so this app relabels from its own clock once a
//! second and asks the host far less often than that. Polling for a ticking
//! clock is the mistake the wire format exists to avoid.

use std::path::Path;
use std::time::Duration;

use beekeeper_host::client::{self, ClientError};

use crate::model::HostView;

/// While at least one agent is working.
pub const BUSY_INTERVAL: Duration = Duration::from_secs(5);
/// While the host is reachable and nothing is working.
pub const IDLE_INTERVAL: Duration = Duration::from_secs(15);
/// The backoff ladder used while the host is unreachable, in seconds.
pub const BACKOFF_LADDER: [u64; 6] = [1, 2, 4, 8, 15, 30];
/// How often the rows are relabelled from the local clock.
pub const RELABEL_INTERVAL: Duration = Duration::from_secs(1);

/// How long to wait before the next poll.
///
/// `consecutive_failures` is 0 after any answer — including an answer that
/// says the provider is not running, which *is* an answer.
pub fn next_interval(reachable: bool, working: bool, consecutive_failures: u32) -> Duration {
    if reachable {
        return if working {
            BUSY_INTERVAL
        } else {
            IDLE_INTERVAL
        };
    }
    let index = consecutive_failures.saturating_sub(1) as usize;
    let seconds = BACKOFF_LADDER
        .get(index)
        .copied()
        .unwrap_or_else(|| BACKOFF_LADDER[BACKOFF_LADDER.len() - 1]);
    Duration::from_secs(seconds)
}

/// One poll: ask the host, and fold in the login registration when it did not
/// answer.
///
/// The registration is only consulted on failure, and that is deliberate: it
/// is a filesystem read, and it is the *only* thing that distinguishes "not
/// installed" from "installed but not running". Reading it on every successful
/// poll would be work for an answer nobody needs.
pub async fn poll(
    socket: &Path,
    home: &Path,
    instance: beekeeper_host_core::layout::Instance,
) -> HostView {
    match client::status(socket).await {
        Ok(status) => HostView::Reachable(Box::new(status)),
        Err(error) => HostView::Unreachable {
            installed: matches!(error, ClientError::NotRunning { .. })
                && beekeeper_host::install::status(home, instance).installed,
            reason: error.message(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fast while work is in flight, slow when it is not — and an answer that
    /// says "nothing is running" is still an answer, so it uses the idle
    /// cadence rather than the backoff ladder.
    #[test]
    fn the_cadence_follows_the_work_not_the_failures() {
        assert_eq!(next_interval(true, true, 0), BUSY_INTERVAL);
        assert_eq!(next_interval(true, false, 0), IDLE_INTERVAL);
        // A stale failure count must not slow down a host that is answering.
        assert_eq!(next_interval(true, true, 9), BUSY_INTERVAL);
        assert_eq!(next_interval(true, false, 9), IDLE_INTERVAL);
    }

    /// The ladder climbs and then stops climbing. Without a ceiling a laptop
    /// left closed for a day would wake to a ten-minute gap; without a ladder
    /// a machine that never installed the host would connect twice a second
    /// forever.
    #[test]
    fn the_backoff_climbs_to_a_ceiling_and_stays_there() {
        let seconds: Vec<u64> = (1..=8)
            .map(|failures| next_interval(false, false, failures).as_secs())
            .collect();
        assert_eq!(seconds, vec![1, 2, 4, 8, 15, 30, 30, 30]);
        // The first failure is the first rung, not the zeroth: a count of zero
        // cannot happen while unreachable, and reading it as one is safer than
        // indexing past the start.
        assert_eq!(next_interval(false, false, 0).as_secs(), 1);
    }

    /// The clock ticks from the local clock, far more often than the host is
    /// asked — the whole reason the wire carries an absolute start.
    #[test]
    fn relabelling_is_faster_than_polling() {
        assert!(RELABEL_INTERVAL < BUSY_INTERVAL);
        assert!(BUSY_INTERVAL < IDLE_INTERVAL);
    }
}
