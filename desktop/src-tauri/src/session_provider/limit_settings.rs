//! Resolve desktop scope/signing consent, then use the shared explicit limits API.
use super::*;
use beekeeper_host_core::{host_limits, layout, record::CodingSessionProviderStore};
use tauri::Manager;

pub(super) fn desired(
    app: &AppHandle,
    store: &CodingSessionProviderStore,
) -> Result<Option<beekeeper_host_core::config::HostConfig>, String> {
    let home = layout::home_dir()?;
    let instance = crate::agent_host::instance();
    if !config_present(&layout::host_config_path(&home, instance))? {
        return Ok(None);
    }
    let state = app.state::<AppState>();
    let relay = relay_ws_url_with_override(&state);
    let record = store
        .get(&relay)
        .ok_or("active community has no verified provider record; existing host limits retained")?;
    let owner = state.signing_keys()?.public_key();
    host_limits::scoped_host_config(&home, instance, record, &owner)
}
pub(super) fn edit(
    app: &AppHandle,
    store: &CodingSessionProviderStore,
    patch: host_limits::LimitPatch,
) -> Result<(), String> {
    let home = layout::home_dir()?;
    let instance = crate::agent_host::instance();
    if !config_present(&layout::host_config_path(&home, instance))? {
        return Ok(());
    }
    let state = app.state::<AppState>();
    let relay = relay_ws_url_with_override(&state);
    let record = store
        .get(&relay)
        .ok_or("active community has no verified provider record; existing host limits retained")?;
    let owner = state.signing_keys()?.public_key();
    host_limits::patch_host_limits(&home, instance, record, &owner, patch)?;
    Ok(())
}

fn config_present(path: &std::path::Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("host config observation failed".into()),
    }
}
