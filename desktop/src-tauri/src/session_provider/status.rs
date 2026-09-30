//! What this app can say about the coding-session provider.
//!
//! This file replaces the in-process supervisor. The app no longer spawns the
//! provider, no longer restarts it, and no longer stops it when it quits —
//! `beekeeper-host` does all three. What is left is reading the host's answer and
//! reporting it without embellishment.
//!
//! # `running` is nullable, and that is the point
//!
//! It used to mean *"this desktop is supervising it"*, which was a different
//! claim from *"a provider is running"* and read identically in the UI. Now it
//! is `Option<bool>`:
//!
//! - `Some(true)` — the host says the provisioned identity is live;
//! - `Some(false)` — the host answered and it is not;
//! - `None` — **the host could not be reached, so this is not known.**
//!
//! A nullable boolean forces every call site, including every TypeScript one,
//! to handle the third case. Leaving it `false` would let a surface draw a calm
//! "stopped" over something nobody asked, which is the same class of defect as
//! a status reading Idle over a disconnected provider.

use std::path::PathBuf;

use serde::Serialize;

use crate::agent_host::{AgentHost, HostReachability, HostSnapshot};

/// The strongest process fact readiness can prove.
///
/// The first four variants are the ones the in-process supervisor had, kept
/// verbatim so a reader who knew the old vocabulary knows this one. The fifth
/// is what the split adds, and it must never be folded into
/// `NotSupervised`: "the provider is not running" and "nobody could tell me
/// whether the provider is running" call for different words and different
/// actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodingSessionProviderProcessState {
    NotSupervised,
    Backoff,
    Live {
        pid: u32,
    },
    /// The agent host did not answer, or answered that it will not start a
    /// provider — so nothing here is known.
    ///
    /// This replaces the old `Unknown`, which meant "the in-process
    /// supervisor's mutex was poisoned" and is now unreachable: there is no
    /// in-process supervisor. An enum variant nothing can produce claims a
    /// state exists, so it is gone rather than kept for symmetry.
    HostUnreachable {
        reason: String,
    },
}

/// What the app knows about the agent host itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentHostStatus {
    /// Whether the socket answered, and which silence it was if not.
    pub reachability: HostReachability,
    /// The control socket this app looked at, so a person can check it.
    pub socket: PathBuf,
    /// The host's own account of the provider child, when it answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<beekeeper_host::state::ProviderChildState>,
    /// Whether the host is registered to start at login, and anything wrong
    /// with that registration.
    ///
    /// The other half of telling *not installed* from *installed but not
    /// running*: the socket's absence alone cannot distinguish them, and a
    /// surface that told somebody to start something they have not installed
    /// would be worse than silent.
    pub autostart: beekeeper_host::install::Registration,
    /// One line for a person, whatever happened.
    pub message: String,
}

/// Status of the coding-session provider, as reported to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionProviderStatus {
    /// A provider identity exists for this relay.
    pub provisioned: bool,
    /// Whether the provisioned provider is running — `null` when the agent
    /// host could not be reached. See the module docs.
    pub running: Option<bool>,
    /// The agent host: always present, so a surface cannot forget to ask.
    pub host: AgentHostStatus,
    /// Present only when provisioned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_pubkey: Option<String>,
    /// Present only when provisioned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// The provider state directory **as the host reported it**.
    ///
    /// Not serialized to the frontend, which has no use for a filesystem path;
    /// carried so the app's own seat-restage watcher writes into the directory
    /// the running provider actually reads.
    #[serde(skip)]
    pub host_state_dir: Option<PathBuf>,
}

impl CodingSessionProviderStatus {
    /// Build a status from a record and one poll of the host.
    pub(crate) fn from_snapshot(
        record: Option<&beekeeper_host_core::record::CodingSessionProviderRecord>,
        host: &AgentHost,
        snapshot: HostSnapshot,
    ) -> Self {
        let running = match record {
            Some(record) => snapshot.running(&record.provider_pubkey),
            // Nothing is provisioned for this relay, and the host answered:
            // that is a definite "not running", not an unknown.
            None if snapshot.reachability.is_reachable() => Some(false),
            None => None,
        };
        let host_state_dir = snapshot
            .status
            .as_ref()
            .map(|status| status.provider_state_dir.clone());
        Self {
            provisioned: record.is_some(),
            running,
            host_state_dir,
            host: AgentHostStatus {
                reachability: snapshot.reachability,
                socket: host.socket().to_path_buf(),
                provider_state: snapshot.status.map(|status| status.provider),
                autostart: crate::agent_host::autostart::status(),
                message: snapshot.message,
            },
            provider_pubkey: record.map(|record| record.provider_pubkey.clone()),
            instance_id: record.map(|record| record.instance_id.clone()),
        }
    }

    /// The live provider pid, when the host reported one.
    pub(crate) fn live_pid(&self) -> Option<u32> {
        self.host
            .provider_state
            .as_ref()
            .and_then(|state| state.live_pid())
    }

    /// The process state readiness should gate on.
    pub(crate) fn process_state(&self) -> CodingSessionProviderProcessState {
        use beekeeper_host::state::ProviderChildState as Child;
        let Some(state) = &self.host.provider_state else {
            return CodingSessionProviderProcessState::HostUnreachable {
                reason: self.host.message.clone(),
            };
        };
        match state {
            Child::Live { pid, .. } => CodingSessionProviderProcessState::Live { pid: *pid },
            Child::Backoff { .. } => CodingSessionProviderProcessState::Backoff,
            Child::NotSupervised => CodingSessionProviderProcessState::NotSupervised,
            // The host is running and told us it will not start a provider,
            // with its reason. That is a fact, not an unknown — and it is not
            // "not supervised" either, because a person has something specific
            // to fix.
            Child::KeyUnresolved { .. }
            | Child::GaveUp { .. }
            | Child::LockHeldElsewhere { .. } => {
                CodingSessionProviderProcessState::HostUnreachable {
                    reason: state.message(),
                }
            }
        }
    }
}

/// Read the current status for `relay_url`.
pub(crate) async fn provider_status(
    app: &tauri::AppHandle,
    host: &AgentHost,
    relay_url: &str,
) -> Result<CodingSessionProviderStatus, String> {
    let store = crate::session_provider::store::load_provider_store(app)?;
    let snapshot = host.snapshot().await;
    let status = CodingSessionProviderStatus::from_snapshot(store.get(relay_url), host, snapshot);

    // Re-assert the render gate whenever the host names an identity.
    //
    // This used to happen on every start attempt, and `trust.rs` says exactly
    // why it matters: "a provider that runs while the desktop refuses to
    // render its output is a silent, confusing failure." The app no longer
    // starts anything, so the status poll is where the property has to be
    // kept. Best-effort by design — a failure here must not make the status
    // call fail, because a status nobody can read is worse than an allowlist
    // that is one poll behind.
    if let Some(pubkey) = &status.provider_pubkey {
        if let Err(error) = crate::session_provider::trust::seed_provider_trust(app, pubkey) {
            eprintln!("buzz-desktop: session-provider: failed to seed bridge trust: {error}");
        }
    }

    // And re-stage any agent seats the live child is waiting on. The app's
    // supervisor used to do this at spawn time; the status poll is where it
    // belongs now that the app does not spawn anything. Fire-and-forget: a
    // status call must answer quickly and must not fail because a re-stage
    // did.
    match (status.live_pid(), provider_state_dir_from(&status)) {
        (Some(pid), Some(state_dir)) => {
            crate::agent_host::seat_restage::restage_if_needed(app, &state_dir, Some(pid))
        }
        // No live child, or a host that did not say where its state dir is:
        // clear the claim so the next child gets served.
        _ => {
            crate::agent_host::seat_restage::restage_if_needed(app, std::path::Path::new(""), None)
        }
    }
    Ok(status)
}

/// The provider state directory, as the *host* reported it.
///
/// Read from the host's answer rather than derived locally, deliberately: the
/// host is the process actually running the provider, and if the two ever
/// disagreed about where its state lives, the app would be writing seat
/// requests into a directory nothing reads.
fn provider_state_dir_from(status: &CodingSessionProviderStatus) -> Option<PathBuf> {
    status.host_state_dir.clone()
}

/// Resolve the Claude Code CLI exactly as the managed-agent claude runtime
/// does, for the model probe and the runtime table.
///
/// The app still needs this even though it no longer spawns the provider: the
/// runtime descriptors it writes into `host.json` name the CLI, and the models
/// probe drives the adapter directly.
pub(crate) fn resolve_claude_code_executable() -> Option<PathBuf> {
    let cli = crate::managed_agents::known_acp_runtime_exact("claude")?.underlying_cli?;
    let path = crate::managed_agents::resolve_command(cli)?;
    if crate::managed_agents::should_skip_claude_executable(&path, cfg!(windows)) {
        return None;
    }
    Some(path)
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
