//! Tauri commands behind the project Files tab (spec § 4.12): list the
//! agents repository's `main`, read one file at its tip, and land open
//! drafts on it. Reads never touch the packs cache's working copy; the
//! commit builds in a throwaway object database and pushes under a lease.

use tauri::{AppHandle, State};

use crate::managed_agents::agents_repo_commit::{
    commit_drafts, AgentsRepoCommitRequest, AgentsRepoCommitResult, AssetBytes,
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
    // An asset's bytes live in the relay's media store, not in the op, so they
    // are fetched here — while there is still an async runtime and a signing
    // key — and handed to the blocking commit as plain bytes.
    let assets = fetch_commit_assets(&state, &request).await?;
    tokio::task::spawn_blocking(move || {
        use tauri::Manager;
        let state = app.state::<AppState>();
        let repo = resolve_agents_repo(&app, &state, &source, true)?;
        let result = commit_drafts(&app, &repo, &request, committer, &assets)?;
        if result.pushed != "no" {
            // Bring the cache to the new tip so the next read sees it.
            let _ = resolve_agents_repo(&app, &state, &source, true);
        }
        Ok(result)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}

/// Fetch the media blob behind every `asset.put` in `request`, keyed by
/// sha256.
///
/// The bytes are **sha-verified here**: the relay says what it stored, and a
/// commit writes what it was handed into a repository other people read, so
/// the one place to check that those agree is before the write. A blob that
/// cannot be fetched, or whose bytes hash to something else, refuses the whole
/// commit by name rather than landing a file nobody asked for.
async fn fetch_commit_assets(
    state: &AppState,
    request: &AgentsRepoCommitRequest,
) -> Result<AssetBytes, String> {
    use sha2::{Digest, Sha256};

    let mut assets = AssetBytes::new();
    let base = crate::relay::relay_api_base_url_with_override(state);
    for change in &request.drafts {
        if change.op != "asset.put" {
            continue;
        }
        let sha256 = change
            .sha256
            .as_deref()
            .ok_or_else(|| format!("the draft for {} names no blob", change.path))?;
        if assets.contains_key(sha256) {
            continue;
        }
        let extension = change
            .path
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .ok_or_else(|| format!("{} has no extension", change.path))?;
        let url = format!("{base}/media/{sha256}.{extension}");
        let mut get = state
            .http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(60));
        // `url` is always `{relay base}/media/…`, so the token cannot reach a
        // third-party origin (the mint_media_get_auth safety contract).
        if let Some(auth) = crate::commands::media::mint_media_get_auth(state, &base) {
            get = get.header("authorization", auth);
        }
        let response = get.send().await.map_err(|error| {
            format!(
                "could not fetch the image for {}: {error} — nothing was pushed",
                change.path
            )
        })?;
        if !response.status().is_success() {
            return Err(format!(
                "the relay answered {} for the image at {} — nothing was pushed",
                response.status(),
                change.path
            ));
        }
        let bytes = response.bytes().await.map_err(|error| {
            format!(
                "could not read the image for {}: {error} — nothing was pushed",
                change.path
            )
        })?;
        let digest = hex::encode(Sha256::digest(&bytes));
        if digest != sha256 {
            return Err(format!(
                "the bytes served for {} hash to {digest}, not the {sha256} the draft names — \
                 nothing was pushed",
                change.path
            ));
        }
        assets.insert(sha256.to_owned(), bytes.to_vec());
    }
    Ok(assets)
}
