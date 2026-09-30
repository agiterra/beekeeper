//! What the host knows about its child, in a shape a client can act on.
//!
//! The rule this module exists to enforce: **four situations must be
//! distinguishable without inference.**
//!
//! 1. the host is not installed — no socket, no registration;
//! 2. the host is installed but not running — registration, no socket;
//! 3. the host is running but the provider is not — with the reason;
//! 4. the host is running, the provider is live, and the relay is unreachable.
//!
//! Collapsing any two of them is the dishonest-status bug class. "No agents
//! are running" over a host that never answered is the same defect as a status
//! reading Idle over a disconnected provider.
//!
//! The host can only speak to 3 and 4 — 1 and 2 are answered by the client,
//! from the socket's absence and the registration's presence. So every variant
//! below carries its own reason, and none of them is a bare `false`.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// The strongest fact the host has about the provider process.
///
/// Extends the enum the desktop's supervisor already had
/// (`NotSupervised`/`Backoff`/`Live`/`Unknown`) rather than inventing a
/// parallel vocabulary, so a reader who knows one knows the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "state"
)]
pub enum ProviderChildState {
    /// The host is not trying to keep a provider alive — it has not been
    /// commissioned.
    NotSupervised,
    /// A provider is commissioned but the host could not resolve its key, so
    /// it refuses to start one. Carries the named reason.
    KeyUnresolved {
        /// Which routes were tried and what each said.
        #[serde(flatten)]
        reason: crate::identity::KeyUnresolved,
    },
    /// The child is down inside a restart window.
    Backoff {
        /// Consecutive failures counted in the current window.
        failures: u32,
        /// When the next spawn is due, RFC 3339. Present so a menu bar can
        /// say "restarting (attempt 3)" and mean it.
        next_at: String,
    },
    /// The child is running with this pid.
    Live {
        pid: u32,
        /// RFC 3339 stamp of when this child was spawned. Absolute, never a
        /// pre-formatted elapsed string: a formatted duration is only true at
        /// the instant it was written, and it is the whole reason a ticking
        /// clock would otherwise need a round trip per second.
        started_at: String,
    },
    /// The child failed too often, too fast. The host has stopped trying and
    /// will not start again until something asks.
    GaveUp {
        /// Failures counted before giving up.
        failures: u32,
        /// RFC 3339 stamp of the decision.
        at: String,
    },
    /// Another process holds the provider's state-directory lock.
    ///
    /// Never reported as "stopped": "somebody else owns this" is a different
    /// fact, needs different words, and needs a different action.
    LockHeldElsewhere {
        /// The pid recorded in `provider.lock`, when it was readable.
        #[serde(skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
        /// What kind of owner it appears to be.
        kind: LockOwnerKind,
    },
}

/// Who appears to hold the provider's state-directory lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LockOwnerKind {
    /// Another `beekeeper-host`, which said so in `host-owner.json`.
    AnotherHost,
    /// A provider nobody claimed — most likely a pre-upgrade desktop's child,
    /// or an orphan reparented to init.
    UnclaimedProvider,
    /// The lock is held but nothing readable says by whom.
    Unknown,
}

impl ProviderChildState {
    /// The live child's pid, if there is one.
    pub fn live_pid(&self) -> Option<u32> {
        match self {
            Self::Live { pid, .. } => Some(*pid),
            _ => None,
        }
    }

    /// One line a human can act on.
    pub fn message(&self) -> String {
        match self {
            // Says only what the variant knows. It used to assert "no provider
            // is commissioned", which is the *common* way to reach this state
            // and not the only one: a provider that was deliberately stopped
            // lands here too, and the host then logged "stopped: no provider is
            // commissioned on this machine" about a provider it had just
            // stopped. Whether an identity exists is a question `host.json` and
            // the record store answer, not this enum.
            Self::NotSupervised => {
                "the host is not running a provider — if coding sessions are not set up on this \
                 machine yet, open Beekeeper to finish setup"
                    .to_string()
            }
            Self::KeyUnresolved { reason } => reason.message(),
            Self::Backoff { failures, next_at } => {
                format!("the provider is restarting (attempt {failures}, next at {next_at})")
            }
            Self::Live { pid, started_at } => {
                format!("the provider is running as pid {pid}, started {started_at}")
            }
            Self::GaveUp { failures, at } => format!(
                "the provider failed {failures} times in a row and the host stopped trying at \
                 {at} — restart it to try again"
            ),
            Self::LockHeldElsewhere { pid, kind } => match (kind, pid) {
                (LockOwnerKind::AnotherHost, Some(pid)) => {
                    format!("another agent host (pid {pid}) is already managing this provider")
                }
                (LockOwnerKind::AnotherHost, None) => {
                    "another agent host is already managing this provider".to_string()
                }
                (LockOwnerKind::UnclaimedProvider, Some(pid)) => format!(
                    "a provider this host did not start (pid {pid}) holds the state directory — \
                     another Beekeeper may be managing it"
                ),
                (LockOwnerKind::UnclaimedProvider, None) => {
                    "a provider this host did not start holds the state directory".to_string()
                }
                (LockOwnerKind::Unknown, _) => {
                    "the provider state directory is locked and nothing readable says by whom"
                        .to_string()
                }
            },
        }
    }
}

/// The settings the **running** provider was started with.
///
/// Published separately from the child state because these belong to the
/// supervisor's run rather than to the process: the child reads every one of
/// them from its environment at startup, so what a supervisor started with is
/// what is *in force* until the provider next restarts — which is not
/// necessarily what is stored. A surface that showed the stored value alone
/// would claim a ceiling nothing is enforcing.
///
/// `None` in any field means "the provider's own default". `Some(0)` for
/// `max_sessions` and `turn_budget` means unlimited, which is a choice and
/// must survive as one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSettings {
    pub max_sessions: Option<usize>,
    pub turn_idle_timeout_secs: Option<u64>,
    pub turn_budget: Option<u64>,
}

/// What the host can honestly say about the provider's relay connection.
///
/// **Always `Unknown` in v1**, and that is a design consequence rather than an
/// omission. The host *supervises* the provider rather than linking it in, so
/// it holds no relay socket of its own and genuinely cannot see the child's.
/// Synthesising `connected` from "the child is alive" is precisely the
/// comfortable guess this codebase treats as a crash-severity bug.
///
/// The question is answered by the *desktop*, from its own live relay
/// connection plus the provider's published kind:44222 catalog — the only
/// evidence that actually proves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "state"
)]
pub enum RelayConnectionState {
    Unknown {
        /// Why the host cannot tell. Carried so a client renders the reason
        /// rather than an empty badge.
        why: String,
    },
}

impl RelayConnectionState {
    /// The only answer a supervising host can give.
    pub fn unknowable() -> Self {
        Self::Unknown {
            why: "the host supervises the provider as a child process and holds no relay \
                  connection of its own; ask the app, which does"
                .to_string(),
        }
    }
}

/// An RFC 3339 stamp `now + delay`, for `Backoff::next_at`.
pub fn stamp_in(delay: Duration) -> String {
    let at = SystemTime::now() + delay;
    chrono::DateTime::<chrono::Utc>::from(at).to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_says_something_a_person_could_act_on() {
        let states = [
            ProviderChildState::NotSupervised,
            ProviderChildState::KeyUnresolved {
                reason: crate::identity::KeyUnresolved::NotFound {
                    tried: vec!["BEEKEEPER_HOST_PRIVATE_KEY is not set".to_string()],
                },
            },
            ProviderChildState::Backoff {
                failures: 3,
                next_at: "2026-09-30T00:00:08Z".to_string(),
            },
            ProviderChildState::Live {
                pid: 42,
                started_at: "2026-09-30T00:00:00Z".to_string(),
            },
            ProviderChildState::GaveUp {
                failures: 5,
                at: "2026-09-30T00:10:00Z".to_string(),
            },
            ProviderChildState::LockHeldElsewhere {
                pid: Some(7),
                kind: LockOwnerKind::AnotherHost,
            },
        ];
        for state in &states {
            let message = state.message();
            assert!(!message.is_empty(), "{state:?}");
            assert!(
                !message.to_lowercase().contains("no agents are running"),
                "the one sentence no state may produce: {message}"
            );
        }
        // And none of them is silently the same as another.
        for (i, left) in states.iter().enumerate() {
            for right in &states[i + 1..] {
                assert_ne!(left.message(), right.message());
            }
        }
    }

    /// "Somebody else owns this" must never read as "stopped".
    #[test]
    fn a_lock_held_elsewhere_names_the_owner_rather_than_reading_as_stopped() {
        for kind in [
            LockOwnerKind::AnotherHost,
            LockOwnerKind::UnclaimedProvider,
            LockOwnerKind::Unknown,
        ] {
            let message = ProviderChildState::LockHeldElsewhere { pid: None, kind }.message();
            assert!(!message.contains("stopped"), "{kind:?}: {message}");
        }
    }

    /// The wire carries an absolute start time, never a formatted duration:
    /// a formatted duration is true only at the instant it was written, and it
    /// is what would force a poll per second to keep a clock ticking.
    #[test]
    fn the_wire_carries_an_absolute_start_time_and_no_elapsed_string() {
        let json = serde_json::to_string(&ProviderChildState::Live {
            pid: 42,
            started_at: "2026-09-30T00:00:00Z".to_string(),
        })
        .expect("encode");
        assert!(json.contains("\"startedAt\""), "{json}");
        assert!(!json.contains("elapsed"), "{json}");
    }

    #[test]
    fn the_relay_state_says_why_it_cannot_tell_rather_than_guessing() {
        let RelayConnectionState::Unknown { why } = RelayConnectionState::unknowable();
        assert!(why.contains("ask the app"), "{why}");
        let json = serde_json::to_string(&RelayConnectionState::unknowable()).expect("encode");
        assert!(json.contains("\"unknown\""), "{json}");
        assert!(
            !json.contains("connected"),
            "a supervising host must never claim a relay state: {json}"
        );
    }

    #[test]
    fn a_backoff_stamp_is_in_the_future_and_parses_as_rfc_3339() {
        let stamp = stamp_in(Duration::from_secs(8));
        let parsed = chrono::DateTime::parse_from_rfc3339(&stamp).expect("rfc 3339");
        assert!(parsed > chrono::Utc::now());
    }
}
