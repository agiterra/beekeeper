//! Built-in shell sessions (the "Built-in Shell" experiment).
//!
//! The embedded alternative to the cmux integration. Each session's shell runs
//! in a detached `buzz-shell-host` process that owns the PTY and outlives this
//! app, so a shell survives app restarts/updates; the app is a socket client
//! that mirrors the host's output into a local scrollback + `vt100` parser for
//! snapshot reads and streams it to the frontend over Tauri events.
//!
//! Agents reach these sessions through the same session broker and the same
//! default-off, per-session agent consent as cmux sessions (see
//! `session_broker`); shell sessions are addressed there with a `shell:`
//! -prefixed workspace id so the broker can route by backend.

pub mod access;
pub mod broadcast;
pub mod host_client;
pub mod keys;
pub mod manager;
pub mod persist;
pub mod session_driver;

/// Workspace-id prefix distinguishing built-in shell sessions from cmux
/// sessions on the shared session surface (broker + consent store).
pub const WORKSPACE_ID_PREFIX: &str = "shell:";

/// The broker-facing workspace id for a shell session id.
pub fn workspace_id(session_id: &str) -> String {
    format!("{WORKSPACE_ID_PREFIX}{session_id}")
}

/// The shell session id for a broker workspace id, if it names one.
pub fn session_id_from_workspace(workspace_id: &str) -> Option<&str> {
    workspace_id.strip_prefix(WORKSPACE_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_id_round_trips() {
        let ws = workspace_id("abc-123");
        assert_eq!(ws, "shell:abc-123");
        assert_eq!(session_id_from_workspace(&ws), Some("abc-123"));
        assert_eq!(session_id_from_workspace("WS-cmux"), None);
    }
}
