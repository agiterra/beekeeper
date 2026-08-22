//! Tauri commands for the built-in shell experiment.
//!
//! Session lifecycle + PTY I/O for terminals hosted inside the app. The owner
//! always has interact rights on their own sessions; agents reach these
//! sessions through the session broker (workspace id `shell:<sessionId>`),
//! gated on a collaborator entry in the session's invite roster.

use base64::Engine;
use nostr::JsonUtil;
use tauri::AppHandle;

use crate::shell_sessions::access::{self, AccessRequest, Decision};
use crate::shell_sessions::manager::{self, RosterEntry, ShellSessionInfo};

/// Spawn a new built-in shell session. Defaults: the user's `$SHELL`, `$HOME`.
/// `project_ref`, if given, tags the session with the project container
/// (`30621:<owner>:<slug>` coordinate) it was opened from.
#[tauri::command]
pub fn create_shell_session(
    app: AppHandle,
    cwd: Option<String>,
    title: Option<String>,
    command: Option<String>,
    project_ref: Option<String>,
) -> Result<ShellSessionInfo, String> {
    manager::create(&app, cwd, title, command, project_ref)
}

/// All built-in shell sessions, oldest first (live + restorable).
#[tauri::command]
pub fn list_shell_sessions() -> Vec<ShellSessionInfo> {
    manager::list()
}

/// Bring a restored (restorable) session back to life: spawn a fresh shell in
/// its saved directory with the persisted history replayed.
#[tauri::command]
pub fn resume_shell_session(
    app: AppHandle,
    session_id: String,
) -> Result<ShellSessionInfo, String> {
    manager::resume(&app, &session_id)
}

/// Kill the shell (if running) and remove the session, forgetting its persisted
/// history so it won't be restored on the next launch.
#[tauri::command]
pub fn close_shell_session(app: AppHandle, session_id: String) -> Result<(), String> {
    manager::close(&app, &session_id)
}

/// Rename a session. The new title shows immediately and persists across app
/// restarts (a live session's host records it; a restored session's metadata is
/// rewritten). An empty/whitespace title is rejected.
#[tauri::command]
pub fn rename_shell_session(
    app: AppHandle,
    session_id: String,
    title: String,
) -> Result<(), String> {
    manager::rename(&app, &session_id, &title)
}

/// Tag (or untag, with `project_ref: null`) a session with the project
/// container it belongs to. Local-only bookkeeping — never sent to the relay.
#[tauri::command]
pub fn set_shell_session_project(
    app: AppHandle,
    session_id: String,
    project_ref: Option<String>,
) -> Result<(), String> {
    manager::set_project_ref(&app, &session_id, project_ref)
}

/// Whether shell sessions are persisted across restarts (default on).
#[tauri::command]
pub fn shell_persistence_enabled(app: AppHandle) -> bool {
    crate::shell_sessions::persist::enabled(&app)
}

/// Turn session persistence on or off. Turning it off purges saved history.
#[tauri::command]
pub fn set_shell_persistence_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    crate::shell_sessions::persist::set_enabled(&app, enabled)
}

/// WRITE keystrokes into the session's PTY. Reachable only from this app's
/// own UI — the owner always has interact rights on their own sessions.
/// Remote collaborators go through `shell_remote_input` (signature-verified);
/// agents go through the session broker (roster-gated).
#[tauri::command]
pub fn write_shell_session(session_id: String, data: String) -> Result<(), String> {
    manager::write(&session_id, data.as_bytes())
}

/// Resize the session's PTY to match the frontend terminal.
#[tauri::command]
pub fn resize_shell_session(session_id: String, rows: u16, cols: u16) -> Result<(), String> {
    manager::resize(&session_id, rows, cols)
}

/// ANSI-stripped text snapshot of the session (read-only). Same read surface
/// the broker serves to agents.
#[tauri::command]
pub fn read_shell_session(session_id: String, scrollback: bool) -> Result<String, String> {
    manager::snapshot(&session_id, scrollback)
}

/// The raw scrollback as base64, for terminal replay when (re)opening a
/// session's screen. Read-only.
#[tauri::command]
pub fn attach_shell_session(session_id: String) -> Result<String, String> {
    let raw = manager::raw_scrollback(&session_id)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(raw))
}

/// The access requests agents currently have pending for the owner to answer.
/// Lets the approval UI show requests that arrived before it mounted.
#[tauri::command]
pub fn list_shell_access_requests() -> Vec<AccessRequest> {
    access::list()
}

/// Answer a pending access request. `decision` is `once` (run the requested
/// command a single time), `full` (enable agent consent for the session), or
/// `deny`. Wakes the waiting `bee session request-access` call.
#[tauri::command]
pub fn resolve_shell_access_request(
    app: AppHandle,
    request_id: String,
    decision: String,
) -> Result<(), String> {
    let decision = Decision::parse(&decision)
        .ok_or_else(|| format!("unknown decision '{decision}' (expected once|full|deny)"))?;
    access::resolve(&app, &request_id, decision)
}

/// Flip a session's NIP-ST share flag (observable by project members).
/// Turning sharing off retracts the announce and ends any observer stream.
#[tauri::command]
pub fn set_shell_session_shared(
    app: AppHandle,
    session_id: String,
    shared: bool,
) -> Result<(), String> {
    manager::set_shared(&app, &session_id, shared)
}

/// Replace a session's invite roster (collaborator | viewer entries). The
/// refreshed kind:30623 announce carries the roster as arity-4 `p` tags, so
/// this is both the grant and the revocation signal observers see.
#[tauri::command]
pub fn set_shell_session_roster(
    app: AppHandle,
    session_id: String,
    roster: Vec<RosterEntry>,
) -> Result<(), String> {
    manager::set_roster(&app, &session_id, roster)
}

/// A raw kind:24312 remote-input event arrived for this owner (relayed by
/// the TS pump). The full event JSON is passed through so the Rust side can
/// verify the signature and authorization itself — the pump is untrusted.
#[tauri::command]
pub fn shell_remote_input(app: AppHandle, event_json: String) -> Result<(), String> {
    crate::shell_sessions::remote_input::handle_event_json(&app, &event_json)
}

/// A validated NIP-ST watch event arrived for one of this owner's sessions
/// (relayed by the TS pump). Registers/refreshes/stops the watcher and
/// returns the signed attach-bundle frame events the pump must publish.
#[tauri::command]
pub fn shell_broadcast_watch(
    session_id: String,
    watcher_pubkey: String,
    action: String,
) -> Result<Vec<String>, String> {
    crate::shell_sessions::broadcast::watch(&session_id, &watcher_pubkey, &action)
}

/// The pubkeys currently watching a session (pull fallback for the owner's
/// "N watching" indicator; live updates ride the shell-broadcast-watchers
/// Tauri event).
#[tauri::command]
pub fn shell_broadcast_watchers(session_id: String) -> Vec<String> {
    crate::shell_sessions::broadcast::watchers(&session_id)
}

/// Build + sign a NIP-ST kind:24310 watch event for the observer side (the
/// relay requires signed events; signing lives in Rust with the keys).
#[tauri::command]
pub fn build_shell_watch_event(
    state: tauri::State<'_, crate::app_state::AppState>,
    owner_pubkey: String,
    session_id: String,
    project_ref: String,
    action: String,
) -> Result<String, String> {
    if !matches!(action.as_str(), "watch" | "stop" | "resync") {
        return Err(format!("unknown watch action: {action}"));
    }
    let owner = nostr::PublicKey::from_hex(owner_pubkey.trim())
        .map_err(|e| format!("invalid owner pubkey: {e}"))?;
    let keys = state.signing_keys()?;
    let content = serde_json::json!({ "action": action }).to_string();
    let event = nostr::EventBuilder::new(
        nostr::Kind::Custom(buzz_core_pkg::kind::KIND_SHELL_WATCH as u16),
        content,
    )
    .tags([
        nostr::Tag::public_key(owner),
        nostr::Tag::identifier(session_id),
        nostr::Tag::parse(["a".to_string(), project_ref]).map_err(|e| e.to_string())?,
    ])
    .sign_with_keys(&keys)
    .map_err(|e| format!("sign watch event failed: {e}"))?;
    Ok(event.as_json())
}

/// Build + sign a kind:24312 remote-input event for a session this identity
/// collaborates on (observer side). `content_b64` is the base64 of the raw
/// input bytes; the caller chunks so each event stays ≤ 8 KiB. The relay
/// gates delivery on the owner's roster; the owner host re-verifies again.
#[tauri::command]
pub fn build_shell_input_event(
    state: tauri::State<'_, crate::app_state::AppState>,
    owner_pubkey: String,
    session_id: String,
    project_ref: String,
    content_b64: String,
) -> Result<String, String> {
    if content_b64.len() > 8 * 1024 {
        return Err("input chunk exceeds the 8 KiB event cap".to_string());
    }
    let owner = nostr::PublicKey::from_hex(owner_pubkey.trim())
        .map_err(|e| format!("invalid owner pubkey: {e}"))?;
    let keys = state.signing_keys()?;
    let event = nostr::EventBuilder::new(
        nostr::Kind::Custom(buzz_core_pkg::kind::KIND_SHELL_INPUT as u16),
        content_b64,
    )
    // The owner typing into their own session from another device p-tags
    // their own key; the builder strips self-references by default.
    .allow_self_tagging()
    .tags([
        nostr::Tag::public_key(owner),
        nostr::Tag::identifier(session_id),
        nostr::Tag::parse(["a".to_string(), project_ref]).map_err(|e| e.to_string())?,
    ])
    .sign_with_keys(&keys)
    .map_err(|e| format!("sign input event failed: {e}"))?;
    Ok(event.as_json())
}
