use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State};

use crate::{
    app_state::AppState,
    managed_agents::{
        find_managed_agent_mut, load_managed_agents, save_managed_agents, ManagedAgentSummary,
    },
    util::now_iso,
};

#[tauri::command]
pub fn set_agent_managed_profiles(enabled: bool, state: State<'_, AppState>) {
    state
        .managed_agent_profile_reconcile_enabled()
        .store(!enabled, Ordering::Release);
}

#[tauri::command]
pub fn set_thread_scoped_acp_sessions(enabled: bool, state: State<'_, AppState>) {
    state
        .thread_scoped_acp_sessions_enabled()
        .store(enabled, Ordering::Release);
}

#[tauri::command]
pub async fn set_managed_agent_start_on_app_launch(
    pubkey: String,
    start_on_app_launch: bool,
    app: AppHandle,
) -> Result<ManagedAgentSummary, String> {
    let state = app.state::<AppState>();
    let source = load_managed_agents(&app)?
        .into_iter()
        .find(|r| r.pubkey == pubkey)
        .ok_or("agent not found")?;
    if source.backend != crate::managed_agents::BackendKind::Local {
        return Err("Enabled on host is unsupported for a remote provider without explicit start/stop acknowledgment".into());
    }
    let relay = crate::relay::effective_agent_relay_url(
        &source.relay_url,
        &crate::relay::relay_ws_url_with_override(&state),
    );
    if start_on_app_launch {
        super::agents::host_transfer::start_host_pair(&app, &pubkey, &relay).await?;
    } else {
        super::agents::host_transfer::stop_host_pair(&app, &pubkey, &relay).await?;
    }
    let _guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|e| e.to_string())?;
    let mut records = load_managed_agents(&app)?;
    let record = find_managed_agent_mut(&mut records, &pubkey)?;
    if *record != source {
        return Err(
            "host lifecycle completed; cached settings changed during request; refresh required"
                .into(),
        );
    }
    record.start_on_app_launch = start_on_app_launch;
    record.updated_at = now_iso();
    save_managed_agents(&app, &records)?;
    super::agents::summarize_host_record(
        &app,
        records
            .iter()
            .find(|r| r.pubkey == pubkey)
            .ok_or("agent cache missing")?,
    )
}

#[tauri::command]
pub async fn set_managed_agent_auto_restart(
    pubkey: String,
    auto_restart_on_config_change: bool,
    app: AppHandle,
) -> Result<ManagedAgentSummary, String> {
    let saved_app = app.clone();
    let saved_pubkey = pubkey.clone();
    let result = tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut records = load_managed_agents(&app)?;

        {
            let record = find_managed_agent_mut(&mut records, &pubkey)?;
            record.auto_restart_on_config_change = auto_restart_on_config_change;
            record.updated_at = now_iso();
        }

        save_managed_agents(&app, &records)?;
        let record = records
            .iter()
            .find(|record| record.pubkey == pubkey)
            .ok_or_else(|| format!("agent {pubkey} not found"))?;
        super::agents::summarize_host_record(&app, record)
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))??;
    let state = saved_app.state::<AppState>();
    let record = load_managed_agents(&saved_app)?
        .into_iter()
        .find(|r| r.pubkey == saved_pubkey)
        .ok_or("agent not found")?;
    if record.backend == crate::managed_agents::BackendKind::Local {
        let relay = crate::relay::effective_agent_relay_url(
            &record.relay_url,
            &crate::relay::relay_ws_url_with_override(&state),
        );
        super::agents::host_transfer::reconcile_host_agent(
            &saved_app,
            &state,
            &saved_pubkey,
            &relay,
            false,
        )
        .await?;
    }
    Ok(result)
}
