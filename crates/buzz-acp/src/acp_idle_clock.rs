//! The monotonic clock the ACP turn deadlines are measured against.
//!
//! `read_until_response_with_idle_timeout` runs three deadlines — idle, hard,
//! and answer-stall — against `tokio::time::Instant`. That is the right clock
//! in production and the wrong one for a test that spawns a real subprocess:
//! the turn's budget then covers the operating system's willingness to schedule
//! that subprocess, which on a box running a full build is not bounded by
//! anything the test controls. `claude_named_adapter_wire_lifecycle_records_prompt_and_cost`
//! failed exactly that way — `frames: 0, bytes: 0, quiet_for: 2.003s`, meaning
//! the child had not written its first line inside a two-second window.
//!
//! The seam separates the two questions. Production keeps real time
//! ([`SystemTurnClock`]). A test that is asserting *wire bookkeeping* installs
//! a [`ManualTurnClock`] and the deadlines simply do not advance, so the test
//! measures what it is about. A test that is asserting *deadline behaviour*
//! also installs a `ManualTurnClock` and advances it by hand, so it proves the
//! idle window resets (or expires) at an exact offset rather than hoping the
//! scheduler cooperates.

use std::future::Future;
use std::pin::Pin;

use tokio::time::Instant;

/// A future produced by [`TurnClock::sleep_until`]. Borrowed from the clock so
/// a manual clock can wait on its own notifier.
pub type SleepUntil<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// Source of the instants the ACP read loop measures its turn deadlines
/// against.
///
/// Implementors must be consistent: `sleep_until(d)` resolves once and only
/// once [`now`](TurnClock::now) has reached `d`.
pub trait TurnClock: std::fmt::Debug + Send + Sync {
    /// The current instant on this clock.
    fn now(&self) -> Instant;

    /// Resolve once this clock reaches `deadline`.
    fn sleep_until(&self, deadline: Instant) -> SleepUntil<'_>;
}

/// The production clock: real monotonic time, `tokio`'s timer.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTurnClock;

impl TurnClock for SystemTurnClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep_until(&self, deadline: Instant) -> SleepUntil<'_> {
        Box::pin(tokio::time::sleep_until(deadline))
    }
}

/// A clock that only moves when a test moves it.
///
/// Wall time never advances this clock, so a turn's idle and hard deadlines
/// cannot expire because of load. `advance` moves it and wakes every pending
/// `sleep_until`.
#[cfg(test)]
#[derive(Debug)]
pub struct ManualTurnClock {
    base: Instant,
    elapsed: std::sync::Mutex<std::time::Duration>,
    tick: tokio::sync::Notify,
}

#[cfg(test)]
impl ManualTurnClock {
    /// A clock frozen at the moment of construction.
    pub fn frozen() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            base: Instant::now(),
            elapsed: std::sync::Mutex::new(std::time::Duration::ZERO),
            tick: tokio::sync::Notify::new(),
        })
    }

    /// Move the clock forward by `by` and wake everything waiting on it.
    pub fn advance(&self, by: std::time::Duration) {
        {
            let mut elapsed = self.elapsed.lock().expect("manual clock poisoned");
            *elapsed += by;
        }
        self.tick.notify_waiters();
    }
}

#[cfg(test)]
impl TurnClock for ManualTurnClock {
    fn now(&self) -> Instant {
        self.base + *self.elapsed.lock().expect("manual clock poisoned")
    }

    fn sleep_until(&self, deadline: Instant) -> SleepUntil<'_> {
        Box::pin(async move {
            loop {
                // `enable()` registers the waiter *before* the deadline check.
                // Merely constructing `notified()` does not: the future is not
                // registered until first polled, so an `advance` landing
                // between the check and the await would be missed and this
                // sleeper would never wake.
                let ticked = self.tick.notified();
                tokio::pin!(ticked);
                ticked.as_mut().enable();
                if self.now() >= deadline {
                    return;
                }
                ticked.await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manual_clock_does_not_move_with_wall_time() {
        let clock = ManualTurnClock::frozen();
        let start = clock.now();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            clock.now(),
            start,
            "a manual clock must be immune to wall time"
        );
    }

    #[tokio::test]
    async fn sleep_until_resolves_only_once_the_clock_is_advanced() {
        let clock = ManualTurnClock::frozen();
        let deadline = clock.now() + std::time::Duration::from_secs(2);

        let pending = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            clock.sleep_until(deadline),
        )
        .await;
        assert!(
            pending.is_err(),
            "sleep_until must not resolve while the clock is frozen short of the deadline"
        );

        clock.advance(std::time::Duration::from_secs(2));
        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            clock.sleep_until(deadline),
        )
        .await
        .expect("sleep_until must resolve once the clock reaches the deadline");
    }

    #[tokio::test]
    async fn advance_wakes_a_sleeper_already_waiting() {
        let clock = ManualTurnClock::frozen();
        let deadline = clock.now() + std::time::Duration::from_secs(1);
        let waiter = clock.sleep_until(deadline);

        let advancer = async {
            tokio::task::yield_now().await;
            clock.advance(std::time::Duration::from_secs(1));
        };

        tokio::join!(
            async {
                tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
                    .await
                    .expect("a waiting sleeper must be woken by advance");
            },
            advancer
        );
    }
}
