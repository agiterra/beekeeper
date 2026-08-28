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
        crew_roles::{
            install_role_packs, scan_role_packs, CrewRoleInstallError, InstallCrewRolePacksResponse,
        },
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
/// `lead_name` names the lead identity (D11); every other role keeps its pack's
/// name. Idempotent: an agent already installed from a pack is refreshed, never
/// duplicated, and the team is updated rather than re-created.
///
/// # Errors
///
/// Returns a [`CrewRoleInstallError`] naming the stage that failed — the
/// folder, the keychain, or a store — so the dialog can say which. It never
/// blames the folder for a failure that happened after the scan, and an
/// install that cannot write its agents never reports success.
#[tauri::command]
pub async fn install_crew_role_packs(
    app: AppHandle,
    state: State<'_, AppState>,
    directory: String,
    lead_name: Option<String>,
) -> Result<InstallCrewRolePacksResponse, CrewRoleInstallError> {
    let owner_keys = state.signing_keys().map_err(CrewRoleInstallError::keys)?;
    let directory = directory.trim().to_string();
    if directory.is_empty() {
        return Err(CrewRoleInstallError::folder("no folder was chosen"));
    }

    tokio::task::spawn_blocking(move || {
        let path = std::path::PathBuf::from(&directory);
        if !path.is_dir() {
            return Err(CrewRoleInstallError::folder(format!(
                "{} is not a directory",
                path.display()
            )));
        }
        let scan = scan_role_packs(&path).map_err(CrewRoleInstallError::folder)?;

        let state = app.state::<AppState>();
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| CrewRoleInstallError::store(error.to_string()))?;

        let definitions = load_personas(&app).map_err(CrewRoleInstallError::store)?;
        let agents = load_managed_agents(&app).map_err(CrewRoleInstallError::store)?;
        let teams = load_teams(&app).map_err(CrewRoleInstallError::store)?;

        let mut mint =
            || crate::commands::agents::mint_agent_identity(&owner_keys).map(|(_, minted)| minted);
        let result = install_role_packs(
            &scan,
            definitions,
            agents,
            &teams,
            &now_iso(),
            lead_name.as_deref(),
            &mut mint,
        )?;

        // Definitions first: `save_personas` preserves the instance half of the
        // unified store, and `save_managed_agents` preserves the definition
        // half, so writing them in this order never drops either.
        save_personas(&app, &result.definitions).map_err(CrewRoleInstallError::store)?;
        save_managed_agents(&app, &result.agents).map_err(CrewRoleInstallError::store)?;

        let mut teams: Vec<_> = load_teams(&app)
            .map_err(CrewRoleInstallError::store)?
            .into_iter()
            .filter(|team| team.id != result.team.id)
            .collect();
        teams.push(result.team.clone());
        save_teams(&app, &teams).map_err(CrewRoleInstallError::store)?;

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
            seated: result.seated,
            dropped: result.dropped,
        })
    })
    .await
    .map_err(|e| CrewRoleInstallError::store(format!("spawn_blocking failed: {e}")))?
}
