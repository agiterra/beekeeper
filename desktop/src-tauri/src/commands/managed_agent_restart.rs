use tauri::{AppHandle, State};

use super::agents::{start_local_agent_with_preflight, stop_managed_agent, workspace_owner_hex};
use crate::{
    app_state::AppState, managed_agents::ManagedAgentSummary, relay::relay_ws_url_with_override,
};

fn restart_relay_for_active(active: &str, expected: &str) -> Result<String, String> {
    crate::session_provider::commands::provider_command_relay_for_active(active, Some(expected))
}

/// Restart one managed identity only while the caller's community remains active.
#[tauri::command]
pub async fn restart_managed_agent(
    pubkey: String,
    expected_relay_url: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ManagedAgentSummary, String> {
    let pinned_relay =
        restart_relay_for_active(&relay_ws_url_with_override(&state), &expected_relay_url)?;
    stop_managed_agent(pubkey.clone(), app.clone()).await?;
    // The stop may wait for process termination. Re-resolve afterwards so a
    // community switch during that wait can never redirect the subsequent
    // start to another relay.
    restart_relay_for_active(&relay_ws_url_with_override(&state), &pinned_relay)?;
    let owner_hex = workspace_owner_hex(&state)?;
    start_local_agent_with_preflight(
        &app,
        &state,
        &pubkey,
        &owner_hex,
        false,
        Some(&pinned_relay),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_to_b_switch_after_stop() {
        let pinned = restart_relay_for_active("wss://relay-a.example/", " WSS://RELAY-A.EXAMPLE ")
            .expect("same relay before stop");
        assert_eq!(pinned, "WSS://RELAY-A.EXAMPLE");
        let error = restart_relay_for_active("wss://relay-b.example", &pinned)
            .expect_err("switch during stop must prevent start");
        assert!(error.contains("active community changed"));
    }
}
