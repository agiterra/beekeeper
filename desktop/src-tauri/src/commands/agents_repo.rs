//! Tauri commands behind the project Files tab (spec § 4.12): list the
//! agents repository's `main`, read one file at its tip, and land open
//! drafts on it. Reads never touch the packs cache's working copy; the
//! commit builds in a throwaway object database and pushes under a lease.

use tauri::{AppHandle, State};

use crate::managed_agents::agents_repo_commit::{
    commit_drafts, AgentsRepoCommitRequest, AgentsRepoCommitResult,
};
use crate::managed_agents::agents_repo_read::{
    list_tip, read_tip, resolve_agents_repo, AgentsRepoFile, AgentsRepoListing,
};
use crate::managed_agents::packs_cache::ProjectPackSource;
use crate::managed_agents::role_packs_view::fetch_project_pack_source;
use crate::AppState;

async fn source_for(state: &AppState, project_ref: &str) -> Result<ProjectPackSource, String> {
    let project = project_ref.trim();
    if project.is_empty() {
        return Err("the Files tab needs the project it is about".into());
    }
    fetch_project_pack_source(state, project)
        .await?
        .ok_or_else(|| {
            "this project has no agents repository yet (no kind:30624 source); finish repository \
             setup under Project settings → Packs"
                .to_owned()
        })
}

/// List every file on the agents repository's `main`.
///
/// `refresh` fetches first; otherwise the last fetched tip is listed and
/// `syncedAt` says how old it is.
#[tauri::command]
pub async fn agents_repo_ls(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    refresh: Option<bool>,
) -> Result<AgentsRepoListing, String> {
    let source = source_for(&state, &project_ref).await?;
    let refresh = refresh.unwrap_or(true);
    tokio::task::spawn_blocking(move || {
        use tauri::Manager;
        let state = app.state::<AppState>();
        let repo = resolve_agents_repo(&app, &state, &source, refresh)?;
        list_tip(&repo)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}

/// Read one file at the agents repository's `main` tip.
#[tauri::command]
pub async fn agents_repo_read(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    path: String,
    refresh: Option<bool>,
) -> Result<AgentsRepoFile, String> {
    let source = source_for(&state, &project_ref).await?;
    let refresh = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        use tauri::Manager;
        let state = app.state::<AppState>();
        let repo = resolve_agents_repo(&app, &state, &source, refresh)?;
        read_tip(&repo, &path)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}

/// Land open draft heads on `main`. Always fetches first; refuses a moved
/// tip, a stale base, or a tree the composer refuses, naming each; pushes
/// under a lease; a verify that could not run is reported `unknown`.
#[tauri::command]
pub async fn agents_repo_commit_drafts(
    app: AppHandle,
    state: State<'_, AppState>,
    request: AgentsRepoCommitRequest,
) -> Result<AgentsRepoCommitResult, String> {
    if request.drafts.is_empty() {
        return Err("nothing to commit: pick at least one open draft".into());
    }
    let source = source_for(&state, &request.project_ref).await?;
    let viewer = state.signing_keys()?.public_key().to_hex();
    let committer =
        crate::managed_agents::packs_repo::resolve_app_commit_identity(&state, &viewer).await;
    tokio::task::spawn_blocking(move || {
        use tauri::Manager;
        let state = app.state::<AppState>();
        let repo = resolve_agents_repo(&app, &state, &source, true)?;
        let result = commit_drafts(&app, &repo, &request, committer)?;
        if result.pushed != "no" {
            // Bring the cache to the new tip so the next read sees it.
            let _ = resolve_agents_repo(&app, &state, &source, true);
        }
        Ok(result)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}
