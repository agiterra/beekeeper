//! Read-only listing of the project roles this computer installed, taken from
//! the retained setup publication journals. Listing never creates, locks for
//! write, repairs or publishes anything; installation is not permission.

use super::*;

/// The adopted immutable source an installation resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInstalledRolesSource {
    pub repo_ref: String,
    pub sha: String,
    pub pack_path: String,
}

/// One setup journal's recorded installation for the active owner and relay.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInstalledRoles {
    pub project_ref: String,
    pub setup_id: String,
    pub publication_id: String,
    pub team_id: String,
    /// `None` when the journal's source is no longer determinably adopted.
    pub source: Option<ProjectInstalledRolesSource>,
    /// The lead journal's channel, else the installation's channel reservation.
    pub lead_channel_id: Option<String>,
    pub roles: Vec<activation::ProjectTeamInstalledRole>,
}

/// One journal directory's installation, `Ok(None)` when it is bound to this
/// scope but records no installed roles. A foreign or malformed entry is an
/// `Err` describing why it was skipped.
fn installed_entry(
    root: &Path,
    directory: &Path,
    owner: &str,
    relay: &str,
) -> Result<Option<ProjectInstalledRoles>, String> {
    let metadata = std::fs::symlink_metadata(directory).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("not a real directory".to_string());
    }
    let record_path = directory.join("record.json");
    tree::check_regular_file(&record_path, 64 * 1024).map_err(|error| error.message)?;
    let record: ProjectTeamSetupDraft =
        serde_json::from_slice(&std::fs::read(&record_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("unreadable draft record: {error}"))?;
    if record.owner_pubkey != owner || record.relay_url != relay {
        return Err("bound to another owner or relay".to_string());
    }
    let scope =
        SetupScope::new(&record.project_ref, owner, relay).map_err(|error| error.message)?;
    if scope.directory(root) != directory {
        return Err("stored under a directory that does not match its scope".to_string());
    }
    let draft = read_draft(root, &scope)
        .map_err(|error| error.message)?
        .ok_or_else(|| "draft disappeared while listing".to_string())?;
    let Some(journal) = load_journal(&draft).map_err(|error| error.message)? else {
        return Ok(None);
    };
    let Some(installation) = journal
        .installation
        .as_ref()
        .filter(|installation| !installation.roles.is_empty())
    else {
        return Ok(None);
    };
    let source = adopted_source(&journal)
        .ok()
        .map(|source| ProjectInstalledRolesSource {
            repo_ref: source.repo_ref,
            sha: source.commit,
            pack_path: source.pack_path,
        });
    let lead_channel_id = journal
        .lead
        .as_ref()
        .map(|lead| lead.channel_id.clone())
        .or_else(|| {
            installation
                .channel
                .as_ref()
                .map(|channel| channel.channel_id.clone())
        });
    Ok(Some(ProjectInstalledRoles {
        project_ref: journal.project_ref.clone(),
        setup_id: journal.setup_id.clone(),
        publication_id: journal.publication_id.clone(),
        team_id: installation.team_id.clone(),
        source,
        lead_channel_id,
        roles: installation.roles.clone(),
    }))
}

/// Scan `root` (this app's `project-team-setup` directory) for installations
/// bound to `owner` (hex) and the canonical `relay` key. Read-only.
pub(crate) fn list_installed_roles(
    root: &Path,
    owner: &str,
    relay: &str,
) -> Result<Vec<ProjectInstalledRoles>, SetupError> {
    if !root.try_exists()? {
        return Ok(Vec::new());
    }
    tree::ensure_contained_directory(root, root)?;
    let root = std::fs::canonicalize(root)?;
    let mut installed = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!("skipping unreadable project setup entry: {error}");
                continue;
            }
        };
        // `.preparing-*` directories are in-flight drafts, not journals.
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let directory = entry.path();
        match installed_entry(&root, &directory, owner, relay) {
            Ok(Some(found)) => installed.push(found),
            Ok(None) => {}
            Err(reason) => tracing::warn!(
                "skipping project setup journal {}: {reason}",
                directory.display()
            ),
        }
    }
    installed.sort_by(|a, b| {
        (a.project_ref.as_str(), a.setup_id.as_str())
            .cmp(&(b.project_ref.as_str(), b.setup_id.as_str()))
    });
    Ok(installed)
}

/// List the project roles this computer installed for the active owner on the
/// expected relay. Never creates, repairs, installs or publishes anything.
#[tauri::command]
pub async fn project_team_list_installed_roles(
    app: AppHandle,
    state: State<'_, AppState>,
    expected_relay_url: String,
) -> Result<Vec<ProjectInstalledRoles>, SetupError> {
    let relay = crate::session_provider::commands::provider_command_relay_for_active(
        &crate::relay::relay_ws_url_with_override(&state),
        Some(&expected_relay_url),
    )
    .map_err(|message| SetupError::new("scope_changed", message))?;
    let scope = SetupScope {
        project: String::new(),
        owner: state
            .signing_keys()
            .map_err(|message| SetupError::new("scope_changed", message))?
            .public_key()
            .to_hex(),
        relay: crate::session_provider::canonical_relay_key(&relay),
    };
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| SetupError::new("filesystem", error.to_string()))?
        .join("project-team-setup");
    let (owner, relay) = (scope.owner.clone(), scope.relay.clone());
    let installed =
        tokio::task::spawn_blocking(move || list_installed_roles(&root, &owner, &relay))
            .await
            .map_err(|error| SetupError::new("filesystem", error.to_string()))??;
    verify_context(&state, &scope)?;
    Ok(installed)
}

#[cfg(test)]
#[path = "project_team_installed_roles_tests.rs"]
mod tests;
