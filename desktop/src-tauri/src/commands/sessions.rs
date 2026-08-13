//! Tauri commands for the durable "session" surface (slice C1c).
//!
//! These manage the backend **agent-interaction consent** the session broker
//! enforces — the boundary that lets buzz agents drive a session. Distinct from
//! the human "Interact" consent (frontend, C1b); this one lives in the backend
//! so an agent process cannot bypass it.

use tauri::AppHandle;

use crate::session_broker::consent;

/// Allow or disallow buzz **agents** to drive a session (default off).
#[tauri::command]
pub async fn set_session_agent_consent(
    app: AppHandle,
    workspace_id: String,
    allowed: bool,
) -> Result<(), String> {
    consent::set_agent_consented(&app, &workspace_id, allowed)
}

/// The workspace ids agents are currently allowed to drive.
#[tauri::command]
pub fn list_session_agent_consent() -> Vec<String> {
    consent::list_agent_consented()
}
