//! The agents-repository record of the workdir store (spec § 4.11): where
//! this host's clone of each project's `<slug>-beekeeper-agents` is and the
//! ref its kind:30624 pins, materialized into the provider's view as
//! `agentsRepos` so host steps read `actions.yml` and `team.yml` from there.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::app_state::AppState;
use crate::util::now_iso;

use super::{mutate, CodingSessionWorkdirStore};

/// This host's clone of one project's agents repository (spec § 4.11) and
/// the ref its kind:30624 pins: where the provider reads `actions.yml` and
/// `team.yml` from. Recorded when the repository is created here, when a
/// seat is staged from it, or when the Actions tab reads the project's
/// source. A host-local path, never published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionAgentsRepo {
    pub path: PathBuf,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub updated_at: String,
}

/// The provider's view of [`CodingSessionAgentsRepo`]: path and ref only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionAgentsRepoView {
    pub path: PathBuf,
    #[serde(rename = "ref")]
    pub ref_name: String,
}

impl CodingSessionWorkdirStore {
    /// Record where this host's clone of a project's agents repository is.
    pub(crate) fn set_agents_repo(&mut self, project: &str, path: PathBuf, ref_name: &str) {
        self.agents_repos.insert(
            project.to_string(),
            CodingSessionAgentsRepo {
                path,
                ref_name: ref_name.to_string(),
                updated_at: now_iso(),
            },
        );
    }
}

/// Record this host's clone of a project's agents repository and the pinned
/// ref, re-materializing the provider's view so its next host step reads
/// `actions.yml` from there (spec § 4.11).
pub(crate) fn record_agents_repo(
    app: &AppHandle,
    state: &AppState,
    project: &str,
    path: PathBuf,
    ref_name: &str,
) -> Result<(), String> {
    let project = project.trim().to_string();
    if project.is_empty() {
        return Err("a project coordinate is required".to_string());
    }
    if !path.is_absolute() {
        return Err("an agents repository clone must be an absolute path".to_string());
    }
    mutate(app, state, |store| {
        store.set_agents_repo(&project, path, ref_name)
    })
    .map(|_| ())
}

/// Record the agents clone cut beside `worktree` on that tree's record, so
/// disposal removes it with the tree. `Err` when no tree at that path is
/// recorded — the clone still exists; the caller discloses it.
pub(crate) fn attach_agents_clone(
    app: &AppHandle,
    state: &AppState,
    worktree: &std::path::Path,
    clone: &std::path::Path,
) -> Result<(), String> {
    let store = mutate(app, state, |store| {
        if let Some(entry) = store
            .worktrees
            .values_mut()
            .find(|entry| entry.path == worktree)
        {
            entry.agents_clone = Some(clone.to_path_buf());
        }
    })?;
    if store
        .worktrees
        .values()
        .any(|entry| entry.path == worktree && entry.agents_clone.as_deref() == Some(clone))
    {
        Ok(())
    } else {
        Err(format!(
            "no worktree is recorded at {}, so the agents clone at {} could not be attached to one",
            worktree.display(),
            clone.display()
        ))
    }
}

/// The provider's view of the record: path and ref, by project.
pub(super) fn view_of(
    records: &BTreeMap<String, CodingSessionAgentsRepo>,
) -> BTreeMap<String, CodingSessionAgentsRepoView> {
    records
        .iter()
        .map(|(key, entry)| {
            (
                key.clone(),
                CodingSessionAgentsRepoView {
                    path: entry.path.clone(),
                    ref_name: entry.ref_name.clone(),
                },
            )
        })
        .collect()
}

/// This host's clone of `project`'s agents repository, when one is recorded.
///
/// Read-only and failure-tolerant on purpose: a caller that cannot read the
/// record gets `None` and reports every criterion `unknown` with a reason,
/// which is a better answer than a refusal that hides the whole contract.
pub(crate) fn agents_repo_path_for_project(app: &AppHandle, project: &str) -> Option<String> {
    super::load_workdir_store_readonly(app)
        .ok()?
        .agents_repos
        .get(project.trim())
        .map(|record| record.path.display().to_string())
}
