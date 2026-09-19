//! The project-folder record of the workdir store (`by_project`, ledger
//! 174): the checkout of a project's code repository this host cloned at
//! creation, which a founded session pre-fills and a lead's hires are cut
//! from. A child module of `workdir_store.rs`, split out for the file-size gate.

use std::path::PathBuf;

use tauri::AppHandle;

use crate::app_state::AppState;

use super::{load_workdir_store_readonly, mutate, CodingSessionWorkdirScope};

/// The folder recorded for `project_ref` (`by_project`), if any. A store
/// that cannot be read answers `None` and logs why: the caller is about to
/// decide whether to clone, and an unreadable record must not become a
/// refusal to record.
pub(crate) fn recorded_project_checkout(app: &AppHandle, project_ref: &str) -> Option<PathBuf> {
    match load_workdir_store_readonly(app) {
        Ok(store) => store
            .by_project
            .get(project_ref.trim())
            .map(|entry| entry.path.clone()),
        Err(error) => {
            tracing::warn!(target: "workdir_store", %error, "could not read the recorded project folders");
            None
        }
    }
}

/// Record `path` as the project's folder (`by_project[project_ref]`) — what
/// a founded session pre-fills and the lead's hires are cut from (ledger
/// 174). Goes through `mutate`, so the provider's projects view is
/// re-materialized with it.
pub(crate) fn set_project_checkout(
    app: &AppHandle,
    state: &AppState,
    project_ref: &str,
    path: PathBuf,
) -> Result<(), String> {
    let project = project_ref.trim().to_string();
    if project.is_empty() {
        return Err("a project coordinate is required".to_string());
    }
    if !path.is_absolute() {
        return Err("a project folder must be an absolute path".to_string());
    }
    mutate(app, state, |store| {
        store.set(CodingSessionWorkdirScope::Project, &project, path)
    })
    .map(|_| ())
}
