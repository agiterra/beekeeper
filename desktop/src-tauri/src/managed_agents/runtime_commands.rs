//! Host-only lifecycle adapters and observations. This client owns no executor.
use super::{
    load_managed_agents, ManagedAgentRuntimeKey, ManagedAgentRuntimeLifecycle,
    ManagedAgentRuntimeStatus,
};
use crate::app_state::AppState;
use tauri::{AppHandle, Emitter, Manager};

fn status_for(
    app: &AppHandle,
    key: &ManagedAgentRuntimeKey,
    requested: Option<String>,
) -> ManagedAgentRuntimeStatus {
    use beekeeper_host_core::{
        layout,
        ownership::{self, Phase},
    };
    let transfer = layout::home_dir().and_then(|home| {
        ownership::for_pair(
            &home,
            crate::agent_host::instance(),
            &key.pubkey,
            &key.relay_url,
        )
    });
    let (lifecycle, error) = match transfer {
        Ok(Some(t)) if matches!(t.phase, Phase::Adopted | Phase::HostStopped) => (
            ManagedAgentRuntimeLifecycle::HostManaged,
            Some(
                "Execution belongs to the host; relay session evidence determines availability."
                    .into(),
            ),
        ),
        Ok(Some(_)) => (
            ManagedAgentRuntimeLifecycle::Failed,
            Some("One-way host migration is pending; desktop execution is unavailable.".into()),
        ),
        Ok(None) => (
            ManagedAgentRuntimeLifecycle::Failed,
            Some("Host adoption is required; repair migration custody or commissioning.".into()),
        ),
        Err(error) => (ManagedAgentRuntimeLifecycle::Failed, Some(error)),
    };
    ManagedAgentRuntimeStatus {
        pubkey: key.pubkey.clone(),
        relay_url: key.relay_url.clone(),
        requested_relay_url: requested,
        local_setup: false,
        lifecycle,
        pid: None,
        error,
        log_path: super::managed_agent_runtime_log_path(app, key)
            .ok()
            .map(|path| path.display().to_string()),
    }
}
fn emit(app: &AppHandle, status: &ManagedAgentRuntimeStatus) {
    let _ = app.emit("managed-agent-runtime-status", status);
}

#[tauri::command]
pub fn list_managed_agent_runtimes(
    app: AppHandle,
) -> Result<Vec<ManagedAgentRuntimeStatus>, String> {
    let state = app.state::<AppState>();
    let active = crate::relay::relay_ws_url_with_override(&state);
    load_managed_agents(&app)?
        .iter()
        .filter(|record| record.backend == super::BackendKind::Local)
        .map(|record| {
            let relay = crate::relay::effective_agent_relay_url(&record.relay_url, &active);
            ManagedAgentRuntimeKey::new(&record.pubkey, &relay)
                .map(|key| status_for(&app, &key, None))
        })
        .collect()
}

#[tauri::command]
pub async fn start_managed_agent_runtime(
    pubkey: String,
    relay_url: String,
    app: AppHandle,
) -> Result<ManagedAgentRuntimeStatus, String> {
    let key = ManagedAgentRuntimeKey::new(pubkey, &relay_url)?;
    crate::commands::agents::headless_management::start_host_pair(
        &app,
        &key.pubkey,
        &key.relay_url,
    )
    .await?;
    let status = status_for(&app, &key, None);
    emit(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn stop_managed_agent_runtime(
    pubkey: String,
    relay_url: String,
    app: AppHandle,
) -> Result<ManagedAgentRuntimeStatus, String> {
    let key = ManagedAgentRuntimeKey::new(pubkey, &relay_url)?;
    crate::commands::agents::headless_management::stop_host_pair(&app, &key.pubkey, &key.relay_url)
        .await?;
    let status = status_for(&app, &key, None);
    emit(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn restart_managed_agent_runtime(
    pubkey: String,
    relay_url: String,
    app: AppHandle,
) -> Result<ManagedAgentRuntimeStatus, String> {
    stop_managed_agent_runtime(pubkey.clone(), relay_url.clone(), app.clone()).await?;
    start_managed_agent_runtime(pubkey, relay_url, app).await
}

/// Reconcile configured communities through the same host service, never a process registry.
#[tauri::command]
pub async fn reconcile_managed_agent_runtimes(
    communities: Vec<super::ManagedAgentCommunityTarget>,
    app: AppHandle,
) -> Result<Vec<ManagedAgentRuntimeStatus>, String> {
    let records = load_managed_agents(&app)?;
    let mut rows = Vec::new();
    for community in communities {
        for record in records
            .iter()
            .filter(|record| record.backend == super::BackendKind::Local)
        {
            let key = ManagedAgentRuntimeKey::new(&record.pubkey, &community.relay_url)?;
            rows.push(status_for(&app, &key, Some(community.relay_url.clone())));
        }
    }
    for row in &rows {
        emit(&app, row);
    }
    Ok(rows)
}
