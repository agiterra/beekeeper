//! A project's default agents (spec § 4.11): one managed identity per role
//! the agents repository's `team.yml` names, minted when the project is
//! created on this computer and associated with the project.
//!
//! Before the pivot the project-team setup flow did this from a pack-layout
//! snapshot ([`super::project_team_setup_activation`]); a project created
//! today has a flat agents repository and no setup journal, so the same
//! installer ([`super::crew_roles::install_role_packs_in_named_team`]) is
//! driven from the roles the composer stages out of that repository. The
//! staged packs are copied under `<packs root>/installs/<owner8>-<id>/<role>/`
//! — one stable directory per role — so a rerun (**Finish repository
//! setup**) refreshes the agents it already installed rather than minting a
//! second set.
//!
//! Identities are minted the one way every managed agent is
//! ([`crate::commands::mint_agent_identity`]: a fresh key with the owner's
//! NIP-OA attestation) and named as `team.yml` `agents[]` names them.

use std::collections::HashMap;

use serde::Serialize;
use tauri::AppHandle;

use crate::app_state::AppState;
use crate::managed_agents::{
    crew_roles, load_managed_agents, load_personas, load_teams, packs_cache, packs_repo,
    project_agent_association as association, save_managed_agents, save_personas, save_teams,
};

/// Where a project's staged role packs are laid out for the installer.
pub const INSTALLS_DIR: &str = "installs";

/// One agent the install produced, for the create's result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledDefaultAgent {
    pub role: String,
    pub name: String,
    pub pubkey: String,
    /// `true` when this run found the agent already installed and refreshed
    /// it rather than minting a new identity.
    pub refreshed: bool,
}

/// The team every project's default agents belong to on this computer.
pub fn project_team_name(project_ref: &str) -> String {
    format!("Project team {project_ref}")
}

/// Mint and associate the project's default agents from `source` — its
/// agents repository — returning what was installed, in role order.
pub(crate) fn install_default_agents(
    app: &AppHandle,
    state: &AppState,
    project_ref: &str,
    source: &packs_cache::ProjectPackSource,
) -> Result<Vec<InstalledDefaultAgent>, String> {
    let keys = state.signing_keys()?;
    let auth = crate::commands::project_git_exec::build_git_auth_config(state)?;
    let relay_http =
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(state));
    let packs_root = packs_cache::packs_root(app)?;
    let catalog = packs_cache::template_catalog(app);
    let (owner, id) = packs_cache::parse_repo_coordinate(&source.repo)?;
    let path = packs_cache::validate_pack_path(&source.path)?;
    let checkout = packs_cache::packs_checkout_dir(&packs_root, &owner, &id);
    let clone_url = packs_cache::packs_clone_url(&relay_http, &owner, &id);
    packs_cache::sync_packs_checkout(&checkout, &clone_url, source, &auth)?;
    let root = if buzz_core_pkg::project_pack_source::is_root_pack_path(&path) {
        checkout.clone()
    } else {
        checkout.join(&path)
    };

    // The roles and names come from the repository's own manifest; a
    // manifest that is there and wrong refuses, a missing one means the
    // role files alone.
    let team = buzz_persona_pkg::team::load_team(&root).map_err(|error| error.to_string())?;
    let mut roles: Vec<String> = team
        .as_ref()
        .map(|team| team.roles.keys().cloned().collect())
        .unwrap_or_default();
    if roles.is_empty() {
        roles = role_files(&root);
    }
    if roles.is_empty() {
        return Err(format!(
            "the agents repository {} names no roles to install agents for",
            source.repo
        ));
    }
    let names: HashMap<String, String> = team
        .as_ref()
        .map(|team| {
            let mut names = HashMap::new();
            for agent in &team.agents {
                names
                    .entry(agent.role.clone())
                    .or_insert_with(|| agent.name.clone());
            }
            names
        })
        .unwrap_or_default();

    // One stable directory per role, so a rerun refreshes rather than
    // re-mints: the installer matches an existing agent by pack dir.
    let install_dir = packs_root
        .join(INSTALLS_DIR)
        .join(packs_cache::pack_cache_dir_name(&owner, &id));
    for role in &roles {
        let staged = packs_cache::stage_project_role_pack(
            &packs_root,
            &relay_http,
            source,
            role,
            &auth,
            &catalog,
        )
        .map_err(|error| format!("role {role}: {error}"))?;
        let dest = install_dir.join(role);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)
                .map_err(|error| format!("clear {}: {error}", dest.display()))?;
        }
        packs_repo::copy_tree(&staged.dir, &dest)?;
    }
    let scan = crew_roles::scan_role_packs(&install_dir)?;
    if scan.packs.is_empty() {
        return Err(format!(
            "none of the staged roles ({}) is a pack the installer can read",
            roles.join(", ")
        ));
    }

    let _guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|_| "managed-agent storage lock is unavailable".to_string())?;
    let definitions = load_personas(app)?;
    let agents = load_managed_agents(app)?;
    let teams = load_teams(app)?;
    let team_name = project_team_name(project_ref);
    let team_id = teams
        .iter()
        .find(|team| team.name == team_name)
        .map(|team| team.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut mint = || crate::commands::mint_agent_identity(&keys).map(|(_, minted)| minted);
    let mut result = crew_roles::install_role_packs_in_named_team(
        &scan,
        definitions,
        agents,
        &teams,
        &crate::util::now_iso(),
        &names,
        &mut mint,
        team_id,
        &team_name,
    )
    .map_err(|error| error.detail)?;
    // A pack pins no runtime; without this every record lands on the app's
    // default harness and no hire can seat it (ledger 165).
    let global = crate::managed_agents::load_global_agent_config(app)?;
    crew_roles::pin_default_runtime(&mut result, global.preferred_runtime.as_deref());
    association::associate_installation(&mut result.agents, project_ref, &result.installed)?;
    save_personas(app, &result.definitions)?;
    save_managed_agents(app, &result.agents)?;
    association::retain_installed_agents(app, state, &result.agents, &result.installed);
    let mut next_teams: Vec<_> = teams
        .into_iter()
        .filter(|team| team.id != result.team.id)
        .collect();
    next_teams.push(result.team);
    save_teams(app, &next_teams)?;
    drop(_guard);
    // The webview's managed-agent list re-reads only on this event while no
    // agent is running (`useManagedAgentsQuery`); without it the new agents
    // are on disk and absent from every picker until an unrelated refresh.
    {
        use tauri::Emitter;
        let _ = app.emit("agents-data-changed", ());
    }

    // Publish the associations, as the setup flow does after installing.
    let owner_hex = keys.public_key().to_hex();
    association::backfill_project_agents_logged(app, &owner_hex);
    crate::managed_agents::project_association_authority::spawn_project_visibility_verification(
        app.clone(),
    );

    let mut installed: Vec<InstalledDefaultAgent> = result
        .installed
        .into_iter()
        .map(|role| InstalledDefaultAgent {
            role: role.role,
            name: role.agent_name,
            pubkey: role.agent_pubkey,
            refreshed: role.refreshed,
        })
        .collect();
    installed.sort_by(|a, b| a.role.cmp(&b.role));
    Ok(installed)
}

/// The pubkeys of every managed agent on this computer associated with
/// `project_ref` — the installed defaults and any agent the Agents tab
/// associated since — sorted and deduplicated. What creation and **Finish
/// repository setup** put on the project's roster (ledger 173). Takes the
/// store lock for the read.
pub(crate) fn project_agent_pubkeys(
    app: &AppHandle,
    state: &AppState,
    project_ref: &str,
) -> Result<Vec<String>, String> {
    let Some(project) = association::normalize_project_ref(project_ref) else {
        return Err(association::ASSOCIATION_MALFORMED_PROJECT.to_string());
    };
    let _guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|_| "managed-agent storage lock is unavailable".to_string())?;
    let mut pubkeys: Vec<String> = load_managed_agents(app)?
        .iter()
        .filter(|record| {
            record
                .project_ref
                .as_deref()
                .and_then(association::normalize_project_ref)
                .as_deref()
                == Some(project.as_str())
        })
        .map(|record| record.pubkey.to_ascii_lowercase())
        .collect();
    pubkeys.sort();
    pubkeys.dedup();
    Ok(pubkeys)
}

/// The role slugs named by `<root>/roles/<role>.md`, sorted; `roles/archive/`
/// is a directory and is skipped by construction.
fn role_files(root: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(buzz_persona_pkg::compose::FLAT_ROLES_DIR))
    else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()?
                .strip_suffix(".md")
                .map(str::to_owned)
        })
        .filter(|name| packs_cache::is_role_slug(name))
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_team_is_named_by_the_project_and_role_files_skip_the_archive() {
        assert_eq!(
            project_team_name("30621:aa:demo"),
            "Project team 30621:aa:demo"
        );
        let tmp = tempfile::tempdir().expect("tempdir");
        let roles = tmp.path().join("roles");
        std::fs::create_dir_all(roles.join("archive")).expect("mkdir");
        std::fs::write(roles.join("lead.md"), "Lead.\n").expect("write");
        std::fs::write(roles.join("builder.md"), "Build.\n").expect("write");
        std::fs::write(roles.join("archive").join("old.md"), "Old.\n").expect("write");
        std::fs::write(roles.join("README.txt"), "not a role\n").expect("write");
        assert_eq!(role_files(tmp.path()), vec!["builder", "lead"]);
        assert!(role_files(&tmp.path().join("nowhere")).is_empty());
    }
}
