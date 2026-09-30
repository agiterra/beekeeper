//! When to revive the provider, and when to stop trying.
//!
//! Pure: the caller supplies how the child exited, the current in-window
//! failure count and how long ago the window opened, so the policy is provable
//! without clocks or processes.
//!
//! This matters far more in a daemon than it did in the desktop app. There, a
//! wrong decision lasted until somebody quit the app; here the supervisor
//! outlives every login session and nothing a human does at the keyboard will
//! clear it.

use std::time::Duration;

/// First restart delay. Doubles per consecutive failure.
pub const BASE_RESTART_DELAY: Duration = Duration::from_secs(2);
/// Ceiling on a single restart delay.
pub const MAX_RESTART_DELAY: Duration = Duration::from_secs(60);
/// Restarts allowed inside one [`RESTART_WINDOW`] before the host gives up.
pub const MAX_RESTARTS_PER_WINDOW: u32 = 5;
/// Sliding window over which restarts are counted.
pub const RESTART_WINDOW: Duration = Duration::from_secs(600);

/// How a supervised child stopped running.
///
/// The distinction is the whole point: `docs/remote-agents.md` § I5 says a
/// launcher's restart policy "MAY revive an abnormal death and MUST NOT revive
/// an intentional clean exit". Passing an exit *code* alone cannot express it —
/// `None` from `ExitStatus::code()` means "died on a signal", which is a
/// different fact from "exited 0".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildExit {
    /// The child exited with this status code.
    Code(i32),
    /// The child died on a signal, or its status could not be read.
    Signal,
}

impl ChildExit {
    /// Whether this exit was the child saying it was finished.
    pub fn is_intentional_clean_exit(self) -> bool {
        matches!(self, Self::Code(0))
    }

    /// How the exit reads in a log marker.
    pub fn describe(self) -> String {
        match self {
            Self::Code(code) => code.to_string(),
            Self::Signal => "signal".to_string(),
        }
    }
}

/// What the supervisor should do after the child exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// Sleep `delay`, then respawn. `failures` is the new in-window count.
    Retry {
        /// How long to wait before respawning.
        delay: Duration,
        /// Failure count after recording this exit.
        failures: u32,
    },
    /// Stop supervising and leave the provider down until something
    /// explicitly asks for it again.
    GiveUp,
}

/// Decide whether to restart after the child exited.
///
/// A clean `exit(0)` is never revived: the provider asked to stop, and a host
/// that brought it straight back would make `!shutdown` — and any future
/// self-terminating path — impossible to observe.
///
/// A window that has aged out resets the count to one rather than zero: the
/// exit that just happened is itself the first failure of the new window.
pub fn plan_restart(
    exit: ChildExit,
    failures_in_window: u32,
    window_elapsed: Duration,
) -> RestartDecision {
    if exit.is_intentional_clean_exit() {
        return RestartDecision::GiveUp;
    }
    let failures = if window_elapsed >= RESTART_WINDOW {
        1
    } else {
        failures_in_window.saturating_add(1)
    };
    if failures > MAX_RESTARTS_PER_WINDOW {
        return RestartDecision::GiveUp;
    }
    // `failures` is bounded by MAX_RESTARTS_PER_WINDOW, but the saturating
    // shift keeps this correct if that constant ever grows past 32.
    let multiplier = 1u32.checked_shl(failures - 1).unwrap_or(u32::MAX);
    let delay = BASE_RESTART_DELAY
        .checked_mul(multiplier)
        .unwrap_or(MAX_RESTART_DELAY)
        .min(MAX_RESTART_DELAY);
    RestartDecision::Retry { delay, failures }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CRASH: ChildExit = ChildExit::Code(1);

    fn delay_of(decision: RestartDecision) -> Duration {
        match decision {
            RestartDecision::Retry { delay, .. } => delay,
            RestartDecision::GiveUp => panic!("expected a retry, got GiveUp"),
        }
    }

    /// `docs/remote-agents.md` § I5: a launcher MUST NOT revive an intentional
    /// clean exit. The desktop's supervisor did, because it never looked at the
    /// status — a gap that was survivable while quitting the app cleared it and
    /// is not once the supervisor starts at login and outlives every session.
    #[test]
    fn a_clean_exit_is_never_revived_however_fresh_the_window() {
        for (failures, elapsed) in [
            (0, Duration::ZERO),
            (0, RESTART_WINDOW),
            (MAX_RESTARTS_PER_WINDOW, Duration::ZERO),
        ] {
            assert_eq!(
                plan_restart(ChildExit::Code(0), failures, elapsed),
                RestartDecision::GiveUp,
                "failures={failures} elapsed={elapsed:?}"
            );
        }
        // And the abnormal exits around it still are revived, so the assertion
        // above is about the status and not about the counters.
        assert!(matches!(
            plan_restart(ChildExit::Code(1), 0, Duration::ZERO),
            RestartDecision::Retry { .. }
        ));
        assert!(matches!(
            plan_restart(ChildExit::Signal, 0, Duration::ZERO),
            RestartDecision::Retry { .. }
        ));
    }

    #[test]
    fn backoff_doubles_from_the_base_delay() {
        assert_eq!(
            delay_of(plan_restart(CRASH, 0, Duration::ZERO)),
            BASE_RESTART_DELAY
        );
        assert_eq!(
            delay_of(plan_restart(CRASH, 1, Duration::ZERO)),
            BASE_RESTART_DELAY * 2
        );
        assert_eq!(
            delay_of(plan_restart(CRASH, 2, Duration::ZERO)),
            BASE_RESTART_DELAY * 4
        );
    }

    #[test]
    fn backoff_is_capped() {
        assert_eq!(
            delay_of(plan_restart(
                CRASH,
                MAX_RESTARTS_PER_WINDOW - 1,
                Duration::ZERO
            )),
            MAX_RESTART_DELAY.min(BASE_RESTART_DELAY * (1 << (MAX_RESTARTS_PER_WINDOW - 1)))
        );
        assert!(delay_of(plan_restart(CRASH, 3, Duration::ZERO)) <= MAX_RESTART_DELAY);
    }

    #[test]
    fn supervision_gives_up_after_the_window_budget() {
        assert_eq!(
            plan_restart(CRASH, MAX_RESTARTS_PER_WINDOW, Duration::ZERO),
            RestartDecision::GiveUp
        );
    }

    #[test]
    fn an_expired_window_resets_the_failure_count() {
        // The exit that just happened is the first failure of the new window,
        // so the delay is the base delay again rather than zero.
        assert_eq!(
            plan_restart(CRASH, MAX_RESTARTS_PER_WINDOW, RESTART_WINDOW),
            RestartDecision::Retry {
                delay: BASE_RESTART_DELAY,
                failures: 1
            }
        );
    }

    #[test]
    fn a_signal_death_reads_as_a_signal_in_the_log() {
        assert_eq!(ChildExit::Signal.describe(), "signal");
        assert_eq!(ChildExit::Code(137).describe(), "137");
    }
}
