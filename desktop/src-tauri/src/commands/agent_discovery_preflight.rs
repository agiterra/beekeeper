//! The runtime login pre-flight command (finding 71), split from
//! `agent_discovery.rs` for size. The check itself lives in
//! `managed_agents::discovery::auth_preflight`.

/// Exercise a runtime's credential with a real, bounded call and report
/// which of three states it is in: verified live, credential dead (with the
/// CLI's own sentence and the remedy), or unknown.
///
/// `claude auth status` reads the credential file and never refreshes the
/// token, so it reported `loggedIn: true` through a whole run of failed
/// turns (finding 71). This is the check that tells the truth. A verdict is
/// cached for ten minutes; `force` makes the call again regardless, which is
/// what the UI's "Verify login" does after the operator has re-logged in.
///
/// Returns an error only for a runtime that has no pre-flight; a runtime
/// whose CLI is missing gets an `unknown` verdict that says so.
#[tauri::command]
pub async fn check_acp_runtime_auth_preflight(
    runtime_id: String,
    force: Option<bool>,
) -> Result<crate::managed_agents::AuthPreflightVerdict, String> {
    let force = force.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        crate::managed_agents::run_runtime_auth_preflight(&runtime_id, force)
            .ok_or_else(|| format!("runtime {runtime_id} has no login pre-flight"))
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}
