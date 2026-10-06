//! Asking the agent host, and saying honestly when it did not answer.

use std::path::PathBuf;

use beekeeper_host::client::{self, ClientError};
use beekeeper_host::protocol::{Request, Status};
use serde::Serialize;

/// Whether the host answered, and if not, which kind of silence it was.
///
/// Three variants rather than a boolean, because a client that could not tell
/// them apart would show the same words for "you have not installed this",
/// "it is installed and stopped" and "it is wedged" — and at most one of those
/// sentences would be true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    rename_all = "camelCase",
    tag = "state",
    rename_all_fields = "camelCase"
)]
pub enum HostReachability {
    /// The socket answered.
    Reachable,
    /// Nothing is listening. Combined with whether a login registration
    /// exists, this is *not installed* or *installed but not running*.
    Absent { socket: PathBuf },
    /// The socket accepted a connection and then went quiet, or answered
    /// something this app could not read.
    Unresponsive { reason: String },
}

impl HostReachability {
    fn of(error: &ClientError) -> Self {
        match error {
            ClientError::NotRunning { socket } => Self::Absent {
                socket: socket.clone(),
            },
            other => Self::Unresponsive {
                reason: other.message(),
            },
        }
    }

    /// Whether the host answered at all.
    pub fn is_reachable(&self) -> bool {
        matches!(self, Self::Reachable)
    }
}

/// One poll of the host: whether it answered, and what it said.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    pub reachability: HostReachability,
    /// The host's answer, when it gave one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// One line for a person, whatever happened. Never empty, and never
    /// something that would read as "nothing is wrong".
    pub message: String,
}

impl HostSnapshot {
    /// The live provider pid, when there is one and the host said so.
    pub fn live_pid(&self) -> Option<u32> {
        self.status
            .as_ref()
            .and_then(|status| status.provider.live_pid())
    }

    /// Whether the provider this app expects is the one running.
    ///
    /// `None` — not `false` — when the host could not be reached. A nullable
    /// answer forces every call site to handle the third case; a `false` would
    /// let a surface draw a calm "stopped" over something it does not know.
    pub fn running(&self, expected_pubkey: &str) -> Option<bool> {
        let status = self.status.as_ref()?;
        Some(self.live_pid().is_some() && status.provider_pubkey == expected_pubkey)
    }
}

/// The app's handle on the host. Tauri-managed state.
///
/// Holds no connection: every call is one short-lived socket round trip, so
/// there is no reconnect state to go stale and nothing to invalidate when the
/// host restarts under it.
pub struct AgentHost {
    socket: std::sync::OnceLock<PathBuf>,
}

impl AgentHost {
    /// A handle whose socket is resolved at first use, not here.
    ///
    /// This is constructed while the Tauri builder is assembled, before
    /// `setup` initializes the nest directory that decides dev versus
    /// production. Resolving it here read the production fallback, so every
    /// Dev app asked `~/.local/state/buzz/host/host.sock` while its host
    /// listened under `buzz-dev` — team readiness read
    /// `PROVIDER_HOST_UNREACHABLE` over a live provider (ledger 302(h)).
    pub fn new() -> Self {
        Self {
            socket: std::sync::OnceLock::new(),
        }
    }

    /// Where this app looks for the host.
    pub fn socket(&self) -> &std::path::Path {
        self.socket.get_or_init(|| {
            super::socket_path().unwrap_or_else(|error| {
                // A home directory this app cannot resolve is not something to
                // panic over: the socket path then names the failure and every
                // call reports the host as absent, which is honest.
                eprintln!("beekeeper-desktop: agent-host: {error}");
                PathBuf::from("/nonexistent/beekeeper-host.sock")
            })
        })
    }

    /// Poll the host, patiently: one fast attempt, then one retry inside
    /// `PATIENT_READ_BUDGET` before calling it unresponsive.
    ///
    /// Every caller here gates something — team readiness blocks launch on
    /// it, the status poll draws the provider row from it, the roster checks
    /// it before admitting a seat — so a busy machine that answers in 1.2s
    /// must not read as a missing host. The menu bar and `bee host status`
    /// keep their own single fast read.
    pub async fn snapshot(&self) -> HostSnapshot {
        match client::status_patiently(self.socket()).await {
            Ok(status) => HostSnapshot {
                reachability: HostReachability::Reachable,
                message: status.provider.message(),
                status: Some(status),
            },
            Err(error) => HostSnapshot {
                reachability: HostReachability::of(&error),
                status: None,
                message: error.message(),
            },
        }
    }

    /// Poll the host from a blocking context.
    ///
    /// Team readiness gathers its whole local inventory inside
    /// `spawn_blocking`, synchronously, and making that path async would mean
    /// restructuring the readiness gate to reach one socket. A short-lived
    /// current-thread runtime is what `spawn_blocking` exists to host, and the
    /// call it drives is bounded by the client's patient budget (a 1s attempt
    /// and one retry, 5s in all) — so the worst case is the readiness gate
    /// taking five seconds longer and then reporting, truthfully, that the
    /// host did not answer.
    ///
    /// Never call this from an async context: building a runtime inside a
    /// runtime panics. Every caller today is inside `spawn_blocking`.
    pub fn snapshot_blocking(&self) -> HostSnapshot {
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(self.snapshot()),
            Err(error) => HostSnapshot {
                reachability: HostReachability::Unresponsive {
                    reason: format!("could not start a runtime to reach the agent host: {error}"),
                },
                status: None,
                message: format!("could not reach the agent host: {error}"),
            },
        }
    }

    /// Ask the host to start the provider, then report what it says.
    pub async fn start(&self) -> Result<(), String> {
        self.lifecycle(Request::Start).await
    }

    /// Ask the host to stop the provider.
    pub async fn stop(&self) -> Result<(), String> {
        self.lifecycle(Request::Stop).await
    }

    /// Ask the host to re-read `host.json` — the community switch.
    pub async fn bind(&self) -> Result<(), String> {
        self.lifecycle(Request::Bind).await
    }

    /// Ask the host to re-read the identity after the key file was written.
    pub async fn adopt_identity(&self) -> Result<(), String> {
        self.lifecycle(Request::AdoptIdentity).await
    }

    /// Tell the host which managed agents are working.
    ///
    /// The host cannot know: they are this app's children, and it cannot know
    /// channel or agent *names* at all — those live on the relay, which it
    /// deliberately does not talk to. So this app says, and the rows expire if
    /// it stops saying, which is how they vanish when it quits.
    pub async fn push_activity(
        &self,
        rows: Vec<beekeeper_host::activity::PushedActivity>,
    ) -> Result<(), String> {
        self.lifecycle(Request::PushActivity { rows }).await
    }

    pub(crate) async fn lifecycle(&self, request: Request) -> Result<(), String> {
        client::call(
            self.socket(),
            &request,
            beekeeper_host::client::LIFECYCLE_READ_TIMEOUT,
        )
        .await
        .map(|_| ())
        .map_err(|error| error.message())
    }
}

impl Default for AgentHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three silences must be three different facts. A single boolean here
    /// is how "no agents are running" ends up printed over a host nobody
    /// asked.
    #[test]
    fn the_reachability_variants_do_not_collapse() {
        let absent = HostReachability::of(&ClientError::NotRunning {
            socket: PathBuf::from("/tmp/host.sock"),
        });
        let wedged = HostReachability::of(&ClientError::NotResponding {
            after: std::time::Duration::from_secs(1),
        });
        assert_eq!(
            absent,
            HostReachability::Absent {
                socket: PathBuf::from("/tmp/host.sock")
            }
        );
        assert!(matches!(wedged, HostReachability::Unresponsive { .. }));
        assert!(!absent.is_reachable());
        assert!(!wedged.is_reachable());
        assert!(HostReachability::Reachable.is_reachable());
        // A refusal is the host talking, so it is not absence.
        assert!(matches!(
            HostReachability::of(&ClientError::Refused {
                message: "no identity".into()
            }),
            HostReachability::Unresponsive { .. }
        ));
    }

    /// `running` must be `None` rather than `false` when the host did not
    /// answer, so no call site can draw a calm "stopped" over an unknown.
    #[test]
    fn running_is_unknown_rather_than_false_when_the_host_is_absent() {
        let snapshot = HostSnapshot {
            reachability: HostReachability::Absent {
                socket: PathBuf::from("/tmp/host.sock"),
            },
            status: None,
            message: "the agent host is not running".to_string(),
        };
        assert_eq!(snapshot.running(&"a".repeat(64)), None);
        assert_eq!(snapshot.live_pid(), None);
        assert!(!snapshot.message.is_empty());
    }

    /// The wire keeps the reachability state machine-readable, so the frontend
    /// switches on it rather than matching on prose.
    #[test]
    fn reachability_serializes_as_a_tagged_state() {
        let json = serde_json::to_string(&HostReachability::Absent {
            socket: PathBuf::from("/tmp/host.sock"),
        })
        .expect("encode");
        assert!(json.contains("\"state\":\"absent\""), "{json}");
        assert!(json.contains("\"socket\""), "{json}");
        assert_eq!(
            serde_json::to_string(&HostReachability::Reachable).expect("encode"),
            r#"{"state":"reachable"}"#
        );
    }
}
