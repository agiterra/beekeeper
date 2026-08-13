//! The local **session broker** (buzz ↔ agent access to sessions).
//!
//! The `buzz session` CLI calls this owner-only Unix-socket broker inside the
//! desktop app, which is the single authority: it enforces a backend-held,
//! default-off, per-session **agent** consent (distinct from the human
//! "Interact" consent) before any write reaches a session.
//!
//! The protocol and CLI are backend-agnostic on purpose: built-in shell
//! sessions fulfill requests today; another session backend could be added
//! without changing the agent-facing surface (`buzz session`), the socket, or
//! the consent model.

pub mod consent;
pub mod model;
pub mod protocol;
pub mod server;

pub use server::spawn_session_broker;

/// Start every unix-only session service at app setup, in the order their
/// fail-closed guarantees require.
///
/// Extracted from `lib.rs`'s `setup()` so that file stays under the desktop size
/// ratchet; the ordering comments are why it is one function rather than three
/// call sites.
pub fn start_unix_session_services(app: &tauri::AppHandle) {
    // Persisted agent-consent must be hydrated BEFORE the broker can accept a
    // write, so a request is never judged against a store that has not loaded.
    consent::hydrate(app);

    // The owner-only unix socket the `buzz session` CLI calls to let agents act
    // on sessions, gated by that consent.
    let broker_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        spawn_session_broker(broker_handle).await;
    });

    // Built-in shell: reattach to detached host processes that survived the last
    // app run (their shells kept running), and register the rest from on-disk
    // history as restorable (the reboot fallback). Hosts own their own periodic
    // checkpointing.
    crate::shell_sessions::manager::reattach_hosts(app);
}
