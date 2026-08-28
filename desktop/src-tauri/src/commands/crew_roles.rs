//! Tauri surface for the crew-role installer.
//!
//! Two commands: pick a folder, and install every role pack in it. The policy
//! itself lives in [`crate::managed_agents::crew_roles`] so it is testable
//! without an `AppHandle`; this module is the store IO, the key minting, and
//! the relay publish around it.

use tauri::{AppHandle, Manager, State};

use crate::{
    app_state::AppState,
    managed_agents::{
        crew_roles::{install_role_packs, scan_role_packs, InstallCrewRolePacksResponse},
        load_managed_agents, load_personas, load_teams, save_managed_agents, save_personas,
        save_teams, try_regenerate_nest,
    },
    util::now_iso,
};

/// Open the OS folder picker for a folder of role packs.
///
/// The same picker `pick_coding_session_workdir` uses — `tauri-plugin-dialog`
/// is already a dependency and already granted, so this adds no plugin surface.
#[tauri::command]
pub async fn pick_crew_role_packs_directory(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a folder of role packs")
        .pick_folder(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx
        .await
        .map_err(|_| "the folder picker closed unexpectedly".to_string())?;
    Ok(picked.map(|path| path.to_string()))
}

/// Install every role pack in `directory` as an agent carrying its home role,
/// all joined into one team that is a crew.
///
/// Idempotent: an agent already installed from a pack is refreshed, never
/// duplicated, and the team is updated rather than re-created.
///
/// # Errors
///
/// Returns the "That folder could not be read" disclosure for a path that is
/// not a readable directory, and propagates any store or minting failure —
/// an install that cannot write its agents must not report success.
#[tauri::command]
pub async fn install_crew_role_packs(
    app: AppHandle,
    state: State<'_, AppState>,
    directory: String,
) -> Result<InstallCrewRolePacksResponse, String> {
    let owner_keys = state.signing_keys()?;
    let directory = directory.trim().to_string();
    if directory.is_empty() {
        return Err("That folder could not be read: no folder was chosen".to_string());
    }

    tokio::task::spawn_blocking(move || {
        let path = std::path::PathBuf::from(&directory);
        if !path.is_dir() {
            return Err(format!(
                "That folder could not be read: {} is not a directory",
                path.display()
            ));
        }
        let scan = scan_role_packs(&path)
            .map_err(|error| format!("That folder could not be read: {error}"))?;

        let state = app.state::<AppState>();
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;

        let definitions = load_personas(&app)?;
        let agents = load_managed_agents(&app)?;
        let teams = load_teams(&app)?;

        let mut mint =
            || crate::commands::agents::mint_agent_identity(&owner_keys).map(|(_, minted)| minted);
        let result = install_role_packs(&scan, definitions, agents, &teams, &now_iso(), &mut mint)?;

        // Definitions first: `save_personas` preserves the instance half of the
        // unified store, and `save_managed_agents` preserves the definition
        // half, so writing them in this order never drops either.
        save_personas(&app, &result.definitions)?;
        save_managed_agents(&app, &result.agents)?;

        let mut teams: Vec<_> = load_teams(&app)?
            .into_iter()
            .filter(|team| team.id != result.team.id)
            .collect();
        teams.push(result.team.clone());
        save_teams(&app, &teams)?;

        // Publish what was authored here, exactly as create/update do.
        for row in &result.installed {
            if let Some(record) = result
                .agents
                .iter()
                .find(|record| record.pubkey == row.agent_pubkey)
            {
                super::agents::retain_managed_agent_pending(&app, &state, record);
            }
        }
        super::teams::retain_team_pending(&app, &state, &result.team);
        drop(_store_guard);
        try_regenerate_nest(&app);

        Ok(InstallCrewRolePacksResponse {
            team_id: result.team.id.clone(),
            team_name: result.team.name.clone(),
            installed: result.installed,
            skipped: scan.skipped,
        })
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}
