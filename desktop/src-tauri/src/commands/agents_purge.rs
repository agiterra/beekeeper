//! Client cache/custody cleanup after confirmed shared retirement.
use crate::app_state::AppState;
use tauri::AppHandle;

/// Remove this client's stale publication cache and Keychain custody only
/// after the shared service has verified terminal retirement. Protected host
/// custody and migration backups remain available for explicit recovery.
pub(crate) fn purge_managed_agent_side_effects(
    app: &AppHandle,
    state: &AppState,
    pubkey: &str,
    _persona_id: Option<&str>,
) -> Result<(), String> {
    use crate::managed_agents::retention::{
        active_retention_scope, delete_retained_event, open_retention_db,
    };
    let scope = active_retention_scope(app, state)?;
    let conn = open_retention_db(&scope.db_path)?;
    delete_retained_event(
        &conn,
        beekeeper_core_pkg::kind::KIND_MANAGED_AGENT,
        &scope.owner_keys.public_key().to_hex(),
        pubkey,
    )?;
    state.clear_agent_session_caches(pubkey);
    crate::managed_agents::storage::try_delete_agent_key(pubkey).map_err(|error| {
        format!("shared retirement completed; client Keychain cleanup is incomplete: {error}")
    })?;
    Ok(())
}
