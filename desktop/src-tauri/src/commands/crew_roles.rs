//! Tauri surface for the crew-role installer.
//!
//! Two commands: pick a folder, and install every role pack in it. The policy
//! itself lives in [`crate::managed_agents::crew_roles`] so it is testable
//! without an `AppHandle`; this module is the store IO, the key minting, and
//! the relay publish around it.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    app_state::AppState,
    managed_agents::{
        crew_roles::{
            install_role_packs, role_name_choices, role_profile_publishes, scan_project_role_packs,
            scan_role_packs, CrewRoleInstallError, CrewRoleNameChoice,
            InstallCrewRolePacksResponse, ProjectRolePacksScan, SkippedCrewRolePack,
        },
        load_managed_agents, load_personas, load_teams, save_managed_agents, save_personas,
        save_teams, try_regenerate_nest,
    },
    relay::{relay_ws_url_with_override, sync_managed_agent_profile},
    util::now_iso,
};

fn crew_role_install_relay_for_active(
    active: &str,
    expected: &str,
) -> Result<String, CrewRoleInstallError> {
    crate::session_provider::commands::provider_command_relay_for_active(active, Some(expected))
        .map_err(CrewRoleInstallError::relay)
}

fn crew_role_install_relay(
    state: &AppState,
    expected: &str,
) -> Result<String, CrewRoleInstallError> {
    crew_role_install_relay_for_active(&relay_ws_url_with_override(state), expected)
}

/// A chosen folder, together with what one scan of it found.
///
/// The scan travels with the pick because the dialog asks a name per role pack
/// (ledger 84) and cannot render that list before something has read the
/// folder. Scanning is read-only — it mints nothing and writes nothing — so
/// doing it at pick time costs the operator nothing and lets the field list
/// appear the moment the folder is chosen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedCrewRolePacks {
    pub directory: String,
    /// One row per role pack, lead first, each with the name its identity
    /// currently carries on this computer.
    pub packs: Vec<CrewRoleNameChoice>,
    /// Children of the folder that produced no role, and why — the same list
    /// the install reports, shown before anything is written.
    pub skipped: Vec<SkippedCrewRolePack>,
}

/// Open the OS folder picker for a folder of role packs, and scan what was
/// picked.
///
/// The same picker `pick_coding_session_workdir` uses — `tauri-plugin-dialog`
/// is already a dependency and already granted, so this adds no plugin surface.
///
/// # Errors
///
/// Returns the folder's own words when it cannot be read. Nothing here writes,
/// so a failure leaves this computer exactly as it was.
#[tauri::command]
pub async fn pick_crew_role_packs_directory(
    app: AppHandle,
) -> Result<Option<PickedCrewRolePacks>, String> {
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
    let Some(directory) = picked.map(|path| path.to_string()) else {
        return Ok(None);
    };

    tokio::task::spawn_blocking(move || {
        let path = std::path::PathBuf::from(&directory);
        if !path.is_dir() {
            return Err(format!("{} is not a directory", path.display()));
        }
        let scan = scan_role_packs(&path)?;
        // Read-only: the names already installed here are what the fields
        // default to, so re-running the installer offers the identity's own
        // name rather than proposing to rename it back to its role.
        let agents = crate::managed_agents::load_managed_agents(&app).unwrap_or_default();
        Ok(Some(PickedCrewRolePacks {
            directory,
            packs: role_name_choices(&scan, &agents),
            skipped: scan.skipped,
        }))
    })
    .await
    .map_err(|error| format!("the folder scan did not finish: {error}"))?
}

/// Look at `<checkout>/personas/roles` for the project the operator is in,
/// without opening a picker and without writing anything.
///
/// This is how the installer opens on a folder that is already chosen
/// (ledger 85): a new operator had to know where a project keeps its packs.
/// The answer says which of the three states it found — no such folder, an
/// empty one, or one that scans — because the dialog has to say which rather
/// than showing an empty list for both of the first two.
///
/// # Errors
///
/// Returns the folder's own words when a folder that *is* there cannot be
/// read. A folder that is simply absent is reported in the answer, not as an
/// error: a checkout with no role packs is an ordinary state, not a fault.
#[tauri::command]
pub async fn scan_project_role_packs_directory(
    app: AppHandle,
    checkout_dir: String,
) -> Result<ProjectRolePacksScan, String> {
    let checkout_dir = checkout_dir.trim().to_string();
    if checkout_dir.is_empty() {
        return Err("no checkout directory was given".to_string());
    }
    tokio::task::spawn_blocking(move || {
        // Read-only, exactly like the picker's scan: the names already
        // installed here are what the fields default to.
        let agents = load_managed_agents(&app).unwrap_or_default();
        scan_project_role_packs(std::path::Path::new(&checkout_dir), &agents)
    })
    .await
    .map_err(|error| format!("the folder scan did not finish: {error}"))?
}

/// Install every role pack in `directory` as an agent carrying its home role,
/// all joined into one team that is a crew.
///
/// `names` maps a role to the name the operator gave that identity (D11); a
/// role the operator left alone keeps its pack's name. Idempotent: an agent
/// already installed from a pack is refreshed and renamed in place, never
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
    names: Option<HashMap<String, String>>,
    expected_relay_url: String,
) -> Result<InstallCrewRolePacksResponse, CrewRoleInstallError> {
    let names = names.unwrap_or_default();
    let pinned_relay = crew_role_install_relay(&state, &expected_relay_url)?;
    let owner_keys = state.signing_keys().map_err(CrewRoleInstallError::keys)?;
    let directory = directory.trim().to_string();
    if directory.is_empty() {
        return Err(CrewRoleInstallError::folder("no folder was chosen"));
    }

    let (mut response, profiles) = tokio::task::spawn_blocking(move || {
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
        // The agent list as it stood before this install, kept so the profile
        // plan can say which identities were renamed rather than minted.
        let before = agents.clone();
        let teams = load_teams(&app).map_err(CrewRoleInstallError::store)?;

        let mut mint =
            || crate::commands::agents::mint_agent_identity(&owner_keys).map(|(_, minted)| minted);
        let mut result = install_role_packs(
            &scan,
            definitions,
            agents,
            &teams,
            &now_iso(),
            &names,
            &mut mint,
        )?;
        // A pack pins no runtime; without this every record lands on the
        // app's default harness and no hire can seat it (ledger 165).
        let global = crate::managed_agents::load_global_agent_config(&app)
            .map_err(CrewRoleInstallError::store)?;
        crate::managed_agents::crew_roles::pin_default_runtime(
            &mut result,
            global.preferred_runtime.as_deref(),
        );

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

        // The kind:0 half, planned inside the lock and published outside it.
        // The record's own avatar comes from the pack; the fallback is the
        // effective harness's default, exactly as a rename from the agent
        // dialog resolves it.
        let personas = load_personas(&app).unwrap_or_default();
        let profiles: Vec<PendingProfilePublish> = role_profile_publishes(&before, &result)
            .into_iter()
            .filter_map(|publish| {
                let record = result
                    .agents
                    .iter()
                    .find(|record| record.pubkey == publish.pubkey)?;
                let effective_command =
                    crate::managed_agents::record_agent_command(record, &personas);
                Some(PendingProfilePublish {
                    display_name: publish.display_name,
                    private_key_nsec: record.private_key_nsec.clone(),
                    record_relay_url: record.relay_url.clone(),
                    avatar_url: record.avatar_url.clone().or_else(|| {
                        crate::managed_agents::managed_agent_avatar_url(&effective_command)
                    }),
                    auth_tag: record.auth_tag.clone(),
                })
            })
            .collect();

        drop(_store_guard);
        try_regenerate_nest(&app);

        Ok((
            InstallCrewRolePacksResponse {
                team_id: result.team.id.clone(),
                team_name: result.team.name.clone(),
                installed: result.installed,
                skipped: scan.skipped,
                seated: result.seated,
                dropped: result.dropped,
                profile_sync_error: None,
            },
            profiles,
        ))
    })
    .await
    .map_err(|e| CrewRoleInstallError::store(format!("spawn_blocking failed: {e}")))??;

    // Ledger 80 (e): a seat's name on the wire is its identity's kind:0
    // profile, so an install that names an identity and stops has renamed it
    // only on this computer. Profile failures are reported explicitly in the
    // response because the stores are already written; callers that require a
    // fully prepared identity must treat `profile_sync_error` as incomplete.
    crew_role_install_relay(&state, &pinned_relay)?;
    let mut failures: Vec<String> = Vec::new();
    for profile in profiles {
        let keys = match nostr::Keys::parse(&profile.private_key_nsec) {
            Ok(keys) => keys,
            Err(error) => {
                failures.push(format!(
                    "{}: unreadable key ({error})",
                    profile.display_name
                ));
                continue;
            }
        };
        crew_role_install_relay(&state, &pinned_relay)?;
        let relay_url =
            crate::relay::effective_agent_relay_url(&profile.record_relay_url, &pinned_relay);
        if let Err(error) = sync_managed_agent_profile(
            &state,
            &relay_url,
            &keys,
            &profile.display_name,
            profile.avatar_url.as_deref(),
            profile.auth_tag.as_deref(),
        )
        .await
        {
            failures.push(format!("{}: {error}", profile.display_name));
        }
        crew_role_install_relay(&state, &pinned_relay)?;
    }
    if !failures.is_empty() {
        response.profile_sync_error = Some(format!(
            "these identities were installed but the relay still knows them by their previous \
             name — {}",
            failures.join("; ")
        ));
    }
    Ok(response)
}

/// One profile publish carried out of the blocking store section.
///
/// Holds an `nsec`, so it is deliberately not `Debug`: the key exists here for
/// exactly as long as it takes to sign one kind:0 event.
struct PendingProfilePublish {
    display_name: String,
    private_key_nsec: String,
    record_relay_url: String,
    avatar_url: Option<String>,
    auth_tag: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::crew_role_install_relay_for_active;
    use crate::managed_agents::crew_roles::CrewRoleInstallFailure;

    #[test]
    fn install_and_profile_publish_reject_a_to_b_community_switch() {
        assert_eq!(
            crew_role_install_relay_for_active(
                "wss://relay-a.example/",
                " WSS://RELAY-A.EXAMPLE ",
            )
            .expect("same relay"),
            "WSS://RELAY-A.EXAMPLE",
        );
        let error =
            crew_role_install_relay_for_active("wss://relay-b.example", "wss://relay-a.example")
                .expect_err("community switch must fail closed");
        assert_eq!(error.failure, CrewRoleInstallFailure::Relay);
        assert!(error.detail.contains("active community changed"));
    }
}
