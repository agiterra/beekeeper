//! The local **session broker** (Beekeeper ↔ agent access to sessions).
//!
//! The `bee session` CLI calls this owner-only Unix-socket broker inside the
//! desktop app, which is the single authority: before any write reaches a
//! session it requires the calling agent's pubkey to hold a **collaborator**
//! entry on that session's invite roster (or to be this app's own identity).
//! The roster lives on the shell-session manager and rides the kind:30623
//! announce, so the same grant that lets an agent type also shows up to
//! observers.
//!
//! The protocol and CLI are backend-agnostic on purpose: built-in shell
//! sessions fulfill requests today; another session backend could be added
//! without changing the agent-facing surface (`bee session`), the socket, or
//! the access model.

pub mod model;
pub mod protocol;
pub mod server;

pub use server::spawn_session_broker;

/// Start every unix-only session service at app setup, in the order their
/// fail-closed guarantees require.
///
/// Extracted from `lib.rs`'s `setup()` so that file stays under the desktop size
/// ratchet; the ordering comments are why it is one function rather than two
/// call sites.
pub fn start_unix_session_services(app: &tauri::AppHandle) {
    // The owner-only unix socket the `bee session` CLI calls to let agents act
    // on sessions, gated per-session by the invite roster (see `server.rs`).
    let broker_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        spawn_session_broker(broker_handle).await;
    });

    // NIP-ST broadcasting needs the app handle for signing + Tauri events
    // before any session can announce — reattach below announces.
    crate::shell_sessions::broadcast::init(app);

    // Built-in shell: reattach to detached host processes that survived the last
    // app run (their shells kept running), and register the rest from on-disk
    // history as restorable (the reboot fallback). Hosts own their own periodic
    // checkpointing.
    crate::shell_sessions::manager::reattach_hosts(app);
}
