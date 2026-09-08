//! `list_project_role_packs` and `compare_project_pack_revisions` — the Roles
//! tab's two reads.
//!
//! The renderer names a project; this computer answers, for every role it
//! could stage there, the pack it *would* stage, the way
//! `stage_coding_session_actor_seat` would pick it. The walk itself lives in
//! [`crate::managed_agents::role_packs_view`]; this file is the Tauri
//! boundary: read the project's kind:30624 off the relay on the async
//! runtime, then do the disk and `git` work on a blocking thread under the
//! managed-agents store lock, the way `list_teams` does.

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::managed_agents::pack_revisions::{
    compare_project_pack_revisions_blocking, ProjectPackRevisionComparison,
};
use crate::managed_agents::role_packs_view::{
    fetch_project_pack_source, list_project_role_packs_blocking, RolePackSummary,
};

/// Every role this computer could stage for `project_ref`, one row per
/// role, sorted by slug, from the rung of the staging ladder a seat in that
/// role would be staged from.
///
/// `project_ref` is the project's `30621:<owner>:<slug>` coordinate, or
/// `null` for the local rungs alone (no kind:30624 is read, no checkout is
/// consulted). With a project: its newest kind:30624 is read from the relay
/// and its packs repository is synced — read-only, nothing is staged and
/// nothing is published — so the answer is the answer and not a hope. A role
/// the project's repository does not hold still appears, from the rung this
/// computer found it at, carrying the sentence a hire would be refused with.
///
/// # Errors
/// A sentence when the relay could not be read (staging refuses in that case
/// too), when this computer's agent records or checkout inventory could not
/// be read, or when the blocking task failed to run.
#[tauri::command]
pub async fn list_project_role_packs(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: Option<String>,
) -> Result<Vec<RolePackSummary>, String> {
    let project_ref = project_ref
        .map(|project| project.trim().to_owned())
        .filter(|project| !project.is_empty());
    let source = match project_ref.as_deref() {
        Some(project) => fetch_project_pack_source(&state, project).await?,
        None => None,
    };
    tokio::task::spawn_blocking(move || {
        list_project_role_packs_blocking(&app, project_ref.as_deref(), source)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}

/// How each revision in `shas` — the `packRef.sha` values executions reported
/// on their kind:44223 — relates to the commit this computer's packs checkout
/// for `project_ref` is on.
///
/// Read-only in the strongest sense: the project's newest kind:30624 is read
/// from the relay to find *which* checkout to look at, and then only local
/// `git` runs. Nothing is fetched, nothing is checked out, nothing is staged,
/// nothing is published — so opening the Packs tab cannot change the revision
/// it is describing.
///
/// The answer is one row per distinct sha, in the order they were named,
/// saying `current`, `earlier` (with how far behind this machine), `later`
/// (with how far ahead), `unrelated`, or `unknown-here`. When this computer
/// has no checkout — no source, nothing resolved yet, or a `git` that refused
/// — every row is `unknown-here` and `reason` says which, because a reader
/// acting on a wrong `current` is worse served than one told nothing.
///
/// # Errors
/// A sentence when `project_ref` is blank, when the relay could not be read,
/// when a reported sha is not 40-character lowercase hex, when this computer's
/// packs cache or git could not be prepared, or when the blocking task failed
/// to run.
#[tauri::command]
pub async fn compare_project_pack_revisions(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    shas: Vec<String>,
) -> Result<ProjectPackRevisionComparison, String> {
    let project_ref = project_ref.trim().to_owned();
    if project_ref.is_empty() {
        return Err("a pack revision comparison needs the project it is about".to_string());
    }
    let source = fetch_project_pack_source(&state, &project_ref).await?;
    tokio::task::spawn_blocking(move || compare_project_pack_revisions_blocking(&app, source, shas))
        .await
        .map_err(|error| format!("spawn_blocking failed: {error}"))?
}
