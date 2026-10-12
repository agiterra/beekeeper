//! Scoped shared-service start; no desktop executor or fallback.
use super::*;

#[tauri::command]
pub async fn start_managed_agent(
    pubkey: String,
    expected_relay_url: Option<String>,
    expected_signer_pubkey: Option<String>,
    replay_floor_unix: Option<u64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ManagedAgentSummary, String> {
    let owner = workspace_owner_hex(&state)?;
    let signer = crate::relay::bind_expected_signer(expected_signer_pubkey.as_deref(), owner)?;
    let relay = crate::relay::bind_expected_relay_scope(
        expected_relay_url.as_deref(),
        relay_ws_url_with_override(&state),
    )?;
    let record = load_managed_agents(&app)?
        .into_iter()
        .find(|r| r.pubkey == pubkey)
        .ok_or("agent not found")?;
    if record.backend == BackendKind::Local {
        host_transfer::start_host_pair_with_floor(&app, &pubkey, relay.as_str(), replay_floor_unix)
            .await?;
    } else {
        deploy_to_provider(
            &app,
            &state,
            &pubkey,
            ProviderDeployOptions {
                expected_relay_url: Some(relay.as_str()),
                expected_signer_pubkey: Some(signer.as_str()),
                replay_floor_unix,
            },
        )
        .await?;
    }
    let record = load_managed_agents(&app)?
        .into_iter()
        .find(|r| r.pubkey == pubkey)
        .ok_or("agent cache unavailable after start")?;
    summarize_host_record(&app, &record)
}
