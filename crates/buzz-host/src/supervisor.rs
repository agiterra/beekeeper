//! Keeping exactly one provider alive, for as long as the host runs.
//!
//! The loop is the desktop's, moved: spawn, wait, decide, back off, respawn.
//! Three things changed in the move, and each of them is a change this code
//! needed anyway:
//!
//! - the restart decision now takes the **exit status**, so a clean `exit(0)`
//!   is not revived ([`crate::restart_policy`], `docs/remote-agents.md` § I5);
//! - the current child state is **published** rather than inferred from a
//!   handle, so a client can tell the four situations apart
//!   ([`crate::state`]);
//! - the takeover **refuses** a lock another host announced, instead of
//!   signalling whoever holds it ([`crate::takeover`]).
//!
//! Seat re-staging deliberately did **not** move. It reads the relay and the
//! desktop's managed-agent store, which stay desktop-side, so the desktop now
//! watches `seat-requests.json` against the pid the host reports instead of
//! being told by its own supervisor. The cost is disclosed: with no desktop
//! running, seats are not re-staged until one next runs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use buzz_session_host_core::config::HostConfig;
use buzz_session_host_core::logs::{append_log_marker, now_iso};
use buzz_session_host_core::record::CodingSessionProviderRecord;

use crate::restart_policy::{plan_restart, ChildExit, RestartDecision};
use crate::state::{stamp_in, ProviderChildState, RunSettings};
use crate::takeover::{take_over_stale_provider, Takeover};
use crate::terminate::terminate_gracefully_async;

/// Poll interval for child exit while supervising.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Everything a client can read about this host's provider, published by the
/// supervision loop as it goes.
///
/// A `Mutex` around a small enum rather than a channel: every reader wants the
/// *current* state, and none of them wants a history of transitions.
#[derive(Default)]
pub struct PublishedState {
    child: Mutex<Option<ProviderChildState>>,
    in_force: Mutex<Option<RunSettings>>,
    /// What the most recent spawn was configured with. Becomes `in_force` when
    /// that spawn is published as `Live`.
    spawn_settings: Mutex<Option<RunSettings>>,
}

impl PublishedState {
    /// The current child state, or `NotSupervised` before the loop has
    /// published anything.
    ///
    /// A poisoned lock reads as `NotSupervised` with no pretence: the
    /// alternative is claiming a state nobody wrote.
    pub fn child(&self) -> ProviderChildState {
        self.child
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .unwrap_or(ProviderChildState::NotSupervised)
    }

    /// The settings the running provider was started with, if one is running.
    ///
    /// `None` means nothing is running, which is a different answer from
    /// "running on the provider's own defaults". Collapsing the two would have
    /// a settings panel claim a ceiling is enforced when nothing is enforcing
    /// anything.
    pub fn in_force(&self) -> Option<RunSettings> {
        self.in_force.lock().ok().and_then(|guard| *guard)
    }

    /// Publish a child state, and the settings in force alongside it.
    ///
    /// The two are set together, in one place, deliberately: "what is in
    /// force" is meaningful only while a child is live, and every other state
    /// must clear it. Two separate publishers would eventually leave a stale
    /// ceiling on display over a stopped provider, and nothing about the
    /// display would look wrong.
    fn publish(&self, state: ProviderChildState) {
        let settings = match &state {
            ProviderChildState::Live { .. } => self.spawn_settings.lock().ok().and_then(|g| *g),
            _ => None,
        };
        if let Ok(mut guard) = self.in_force.lock() {
            *guard = settings;
        }
        if let Ok(mut guard) = self.child.lock() {
            *guard = Some(state);
        }
    }

    /// Record what the next spawn was configured with, before it is published.
    fn record_spawn_settings(&self, settings: RunSettings) {
        if let Ok(mut guard) = self.spawn_settings.lock() {
            *guard = Some(settings);
        }
    }
}

/// A running supervision loop.
pub struct Supervisor {
    config: HostConfig,
    record: CodingSessionProviderRecord,
    nsec: String,
    log_path: std::path::PathBuf,
    socket_path: std::path::PathBuf,
    published: Arc<PublishedState>,
    stop: Arc<AtomicBool>,
}

impl Supervisor {
    /// Prepare a supervisor. Nothing is spawned until [`Self::run`].
    pub fn new(
        config: HostConfig,
        record: CodingSessionProviderRecord,
        nsec: String,
        log_path: std::path::PathBuf,
        socket_path: std::path::PathBuf,
        published: Arc<PublishedState>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config,
            record,
            nsec,
            log_path,
            socket_path,
            published,
            stop,
        }
    }

    /// Supervise until the stop flag is set, or until the policy gives up.
    ///
    /// Returns when there is nothing left to supervise. The host itself keeps
    /// running: a provider that gave up is a state a client must be able to
    /// read and act on, not a reason to take the control socket down.
    pub async fn run(self) {
        let binary = match crate::spawn::resolve_provider_binary(&self.config) {
            Ok(binary) => binary,
            Err(error) => {
                let _ = append_log_marker(&self.log_path, &format!("=== {error} ==="));
                self.published.publish(ProviderChildState::GaveUp {
                    failures: 0,
                    at: now_iso(),
                });
                tracing::error!("{error}");
                return;
            }
        };

        let mut failures = 0u32;
        let mut window_start = Instant::now();

        loop {
            if self.stop.load(Ordering::Acquire) {
                break;
            }

            match take_over_stale_provider(&self.config.provider_state_dir, &self.log_path) {
                Takeover::Free => {}
                Takeover::TookOver { from_pid } => {
                    tracing::warn!(
                        from_pid,
                        "took the provider state dir over from a stale owner"
                    );
                }
                Takeover::Refused { pid, kind } => {
                    let state = ProviderChildState::LockHeldElsewhere { pid, kind };
                    tracing::warn!("{}", state.message());
                    self.published.publish(state);
                    return;
                }
            }

            // Claim the directory only once the lock is ours to take, so a
            // refusal never leaves this host's name on somebody else's work.
            if let Err(error) =
                crate::owner::claim(&self.config.provider_state_dir, self.socket_path.clone())
            {
                // Not fatal: the claim is a courtesy to the *next* host, and
                // failing to write it costs a louder takeover later, not this
                // provider's life.
                tracing::warn!("could not record this host as the provider's owner: {error}");
            }

            let mut child = match crate::spawn::spawn_provider_child(
                &binary,
                &self.config,
                &self.record,
                &self.nsec,
                &self.log_path,
            ) {
                Ok(child) => child,
                Err(error) => {
                    tracing::error!("{error}");
                    let _ = append_log_marker(&self.log_path, &format!("=== {error} ==="));
                    // A spawn failure is an abnormal death for policy purposes:
                    // the binary resolved, so this is transient (a busy lock, a
                    // resource limit) far more often than it is permanent.
                    match plan_restart(ChildExit::Signal, failures, window_start.elapsed()) {
                        RestartDecision::Retry { delay, failures: n } => {
                            if n == 1 {
                                window_start = Instant::now();
                            }
                            failures = n;
                            self.published.publish(ProviderChildState::Backoff {
                                failures: n,
                                next_at: stamp_in(delay),
                            });
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        RestartDecision::GiveUp => {
                            self.give_up(failures);
                            return;
                        }
                    }
                }
            };

            let pid = child.id();
            // Recorded from the config *this* spawn read: what is in force is
            // what the running process was started with, not what is stored.
            self.published.record_spawn_settings(RunSettings {
                max_sessions: self.config.max_sessions,
                turn_idle_timeout_secs: self.config.turn_idle_timeout_secs,
                turn_budget: self.config.turn_budget,
            });
            self.published.publish(ProviderChildState::Live {
                pid,
                started_at: now_iso(),
            });
            tracing::info!(pid, relay = %self.config.relay_url, "provider started");

            let exit = self.wait_for_exit(&mut child).await;
            if self.stop.load(Ordering::Acquire) {
                break;
            }
            let Some(exit) = exit else { break };

            let _ = append_log_marker(
                &self.log_path,
                &format!(
                    "=== coding-session provider exited ({}) at {} ===",
                    exit.describe(),
                    now_iso()
                ),
            );

            match plan_restart(exit, failures, window_start.elapsed()) {
                RestartDecision::Retry { delay, failures: n } => {
                    if n == 1 {
                        window_start = Instant::now();
                    }
                    failures = n;
                    self.published.publish(ProviderChildState::Backoff {
                        failures: n,
                        next_at: stamp_in(delay),
                    });
                    tracing::warn!(
                        attempt = n,
                        delay_secs = delay.as_secs(),
                        "provider exited ({}); restarting",
                        exit.describe()
                    );
                    tokio::time::sleep(delay).await;
                }
                RestartDecision::GiveUp if exit.is_intentional_clean_exit() => {
                    // Not a failure: the provider asked to stop. Reviving it
                    // would make an intentional shutdown impossible to observe.
                    let _ = append_log_marker(
                        &self.log_path,
                        "=== the coding-session provider exited cleanly; the host is not \
                         restarting it ===",
                    );
                    tracing::info!("provider exited cleanly; not restarting");
                    self.published.publish(ProviderChildState::GaveUp {
                        failures: 0,
                        at: now_iso(),
                    });
                    crate::owner::release(&self.config.provider_state_dir);
                    return;
                }
                RestartDecision::GiveUp => {
                    self.give_up(failures);
                    return;
                }
            }
        }

        crate::owner::release(&self.config.provider_state_dir);
        self.published.publish(ProviderChildState::NotSupervised);
        tracing::info!("supervision stopped");
    }

    fn give_up(&self, failures: u32) {
        let _ = append_log_marker(
            &self.log_path,
            "=== coding-session provider restarted too often; supervision stopped ===",
        );
        let state = ProviderChildState::GaveUp {
            failures,
            at: now_iso(),
        };
        tracing::error!("{}", state.message());
        self.published.publish(state);
        crate::owner::release(&self.config.provider_state_dir);
    }

    /// Poll the child until it exits or a stop is requested.
    ///
    /// `None` means the caller should stop: either a stop was requested (and
    /// the child has been signalled and reaped here), or the child could not
    /// be polled at all.
    async fn wait_for_exit(&self, child: &mut std::process::Child) -> Option<ChildExit> {
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Some(match status.code() {
                        Some(code) => ChildExit::Code(code),
                        None => ChildExit::Signal,
                    })
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::error!("failed to poll the provider child: {error}");
                    return None;
                }
            }
            if self.stop.load(Ordering::Acquire) {
                let _ = append_log_marker(
                    &self.log_path,
                    &format!("=== stopping coding-session provider at {} ===", now_iso()),
                );
                terminate_gracefully_async(child).await;
                return None;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_published_yet_reads_as_not_supervised_rather_than_as_stopped() {
        let published = PublishedState::default();
        assert_eq!(published.child(), ProviderChildState::NotSupervised);
        assert!(published.child().message().contains("open Beekeeper"));
    }

    #[test]
    fn the_published_state_is_the_current_one_not_a_history() {
        let published = PublishedState::default();
        published.publish(ProviderChildState::Backoff {
            failures: 1,
            next_at: "2026-09-30T00:00:02Z".to_string(),
        });
        published.publish(ProviderChildState::Live {
            pid: 42,
            started_at: "2026-09-30T00:00:02Z".to_string(),
        });
        assert_eq!(published.child().live_pid(), Some(42));
    }

    /// "In force" is meaningful only while a child is live. Every other state
    /// must clear it, or a settings panel ends up showing a ceiling over a
    /// stopped provider and nothing about the display looks wrong.
    #[test]
    fn the_settings_in_force_are_cleared_by_every_state_but_live() {
        let published = PublishedState::default();
        assert_eq!(
            published.in_force(),
            None,
            "nothing running, nothing in force"
        );

        let settings = RunSettings {
            max_sessions: Some(4),
            turn_idle_timeout_secs: Some(900),
            turn_budget: Some(0),
        };
        published.record_spawn_settings(settings);
        published.publish(ProviderChildState::Live {
            pid: 42,
            started_at: "2026-09-30T00:00:02Z".to_string(),
        });
        assert_eq!(published.in_force(), Some(settings));
        // `Some(0)` is "unlimited", a choice, and survives as one rather than
        // collapsing into "unset".
        assert_eq!(published.in_force().and_then(|s| s.turn_budget), Some(0));

        for state in [
            ProviderChildState::Backoff {
                failures: 1,
                next_at: "2026-09-30T00:00:04Z".to_string(),
            },
            ProviderChildState::GaveUp {
                failures: 5,
                at: "2026-09-30T00:10:00Z".to_string(),
            },
            ProviderChildState::NotSupervised,
        ] {
            published.record_spawn_settings(settings);
            published.publish(state.clone());
            assert_eq!(published.in_force(), None, "{state:?}");
        }
    }
}
