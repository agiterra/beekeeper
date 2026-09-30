//! The desktop app as a *client* of the agent host, not its owner.
//!
//! Quitting Beekeeper used to end every coding session, because the app
//! spawned the provider as its own child and reaped it on the way out. It no
//! longer does either. `beekeeper-host` owns the provider; this module is how the
//! app asks it questions, tells it to start and stop, and hands it a
//! commissioning.
//!
//! # The distinction everything here exists to preserve
//!
//! Four situations, which must never collapse into one another:
//!
//! 1. **not installed** — no socket and no login registration;
//! 2. **installed but not running** — a registration, no socket;
//! 3. **running, no provider** — the socket answers, with a named reason;
//! 4. **running, provider live, relay unreachable** — the socket answers, the
//!    child is live, and the app's own relay connection says otherwise.
//!
//! The host can only speak to 3 and 4. The app answers 1 and 2 from the
//! socket's absence plus whether a registration exists, which is why
//! [`HostReachability`] separates *absent* from *unresponsive* rather than
//! reporting a single boolean. A status that read "no agents are running" over
//! a host nobody asked would be the same class of bug as a crash.
//!
//! # Commissioning
//!
//! The app mints the identity and gets the owner key to attest it — only it
//! can, and the host deliberately cannot. What it then writes down is
//! `host.json` (which relay, which identity, where the state directory is) and
//! a `0600` key file. **The secret never crosses the socket**: the app writes
//! the file and the socket call only says "look again".

pub mod autostart;
pub mod client;
pub mod commission;
pub mod seat_restage;

use std::path::PathBuf;

use beekeeper_host_core::layout::{self, Instance};
use tauri::AppHandle;

pub(crate) use client::{AgentHost, HostReachability, HostSnapshot};

/// Which Beekeeper instance this app is, in the host's vocabulary.
///
/// Derived from the nest directory rather than from the bundle identifier,
/// because the nest is what already decides the dev/prod split for the shell
/// sessions' state root — and the host's socket lives beside those. Two
/// different answers here would put the app and the host on different sockets,
/// which renders as "the host is not running" while it sits listening one
/// directory over.
pub(crate) fn instance() -> Instance {
    crate::managed_agents::nest_dir()
        .and_then(|nest| {
            nest.file_name()
                .map(|name| Instance::from_nest_dir_name(&name.to_string_lossy()))
        })
        .unwrap_or(Instance::Production)
}

/// The control socket this app talks to.
pub(crate) fn socket_path() -> Result<PathBuf, String> {
    Ok(layout::host_socket_path(&layout::home_dir()?, instance()))
}

/// The `session-provider` directory, as an absolute path for `host.json`.
///
/// The host never derives this. The directory stays under the app's own
/// app-data tree because several app modules write into it, and a launcher
/// that reconstructed the path from its own idea of where app data lives would
/// be right on exactly one machine.
pub(crate) fn session_provider_base_dir(app: &AppHandle) -> Result<PathBuf, String> {
    crate::session_provider::session_provider_base_dir(app)
}

/// Forward this app's live managed-agent rows to the agent host.
///
/// Replaces `update_tray_agent_activity`, which drew a native tray from inside
/// this app. The tray is the menu bar app's now, and this app's part is to
/// contribute what only it knows: which managed agents are working, in which
/// channel, under which name.
///
/// Failure is returned but is not interesting to the caller — on a machine
/// with no agent host installed every push fails, which is an ordinary state
/// and not this app's problem. The frontend logs at debug and carries on.
#[tauri::command]
pub async fn push_agent_activity(
    host: tauri::State<'_, AgentHost>,
    rows: Vec<beekeeper_host::activity::PushedActivity>,
) -> Result<(), String> {
    host.push_activity(rows).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app and the host must agree about which socket they are on. The
    /// nest name is the only input to that, and this is the one place it is
    /// translated.
    #[test]
    fn the_instance_comes_from_the_nest_name_and_defaults_to_production() {
        // `nest_dir` falls back to the production nest in unit tests, so this
        // asserts the default rather than the dev branch; the translation
        // itself is proved in `beekeeper_host_core::layout`.
        assert_eq!(instance(), Instance::Production);
        assert_eq!(
            Instance::from_nest_dir_name(".beekeeper-dev"),
            Instance::Dev
        );
    }
}
