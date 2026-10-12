//! Scoped shared-service start; no desktop executor or fallback.
use super::*;

#[tauri::command]
pub async fn start_managed_agent(
    pubkey: String,
    expected_relay_url: Option<String>,
    expected_signer_pubkey: Option<String>,
    replay_floor_unix: Option<u64>,
    conversation_channel_id: Option<uuid::Uuid>,
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
        headless_management::start_host_pair_with_floor(
            &app,
            &pubkey,
            relay.as_str(),
            replay_floor_unix,
            conversation_channel_id,
        )
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
    let record = {
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|e| e.to_string())?;
        let mut records = load_managed_agents(&app)?;
        let record = find_managed_agent_mut(&mut records, &pubkey)?;
        // A failed initial enrollment is historical once the shared Start
        // has verified this execution. Keep unrelated runtime errors intact.
        let stale_enrollment = record.backend == BackendKind::Local
            && record
                .last_error
                .as_deref()
                .is_some_and(|message| message.starts_with("HOST_ENROLLMENT_UNCONFIRMED:"));
        if stale_enrollment {
            record.last_error = None;
            record.last_error_code = None;
        }
        let record = record.clone();
        if stale_enrollment {
            save_managed_agents(&app, &records)?;
        }
        record
    };
    summarize_host_record(&app, &record)
}
