//! Tauri commands for the built-in shell experiment.
//!
//! Session lifecycle + PTY I/O for terminals hosted inside the app. The human
//! "Interact" consent is enforced frontend-side (same contract as cmux writes);
//! the backend agent consent is enforced by the session broker, which reaches
//! these sessions through `shell_sessions::manager` with a `shell:`-prefixed
//! workspace id.

use base64::Engine;
use tauri::AppHandle;

use crate::shell_sessions::access::{self, AccessRequest, Decision};
use crate::shell_sessions::manager::{self, ShellSessionInfo};

/// Spawn a new built-in shell session. Defaults: the user's `$SHELL`, `$HOME`.
/// `project_ref`, if given, tags the session with the project container
/// (`30178:<owner>:<slug>` coordinate) it was opened from.
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

/// WRITE keystrokes into the session's PTY. Callers MUST hold the owner's
/// per-session interaction consent (see sessionConsent.ts) — buzz never types
/// into a session without it.
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
/// `deny`. Wakes the waiting `buzz session request-access` call.
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
