//! A seat's clone of the project's agents repository (spec § 4.11).
//!
//! When `team.yml` grants a role `workspace.agents_repo: read | write`, the
//! host cuts a clone of `<slug>-beekeeper-agents` beside the seat's worktree
//! (`<worktree>-agents`) from its own packs cache, points `origin` at the
//! relay so a `write` seat's push lands there, and records the path on the
//! seat's worktree record so disposal removes it with the tree. What the
//! seat may do in it is the relay's push gate and, on Claude, the write
//! fence's call — this module only puts the files where the briefing says.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::app_state::AppState;
use crate::commands::project_git_exec::run_git;
use crate::managed_agents::packs_cache;

/// The suffix the clone's directory carries beside the seat's worktree.
pub const SEAT_AGENTS_CLONE_SUFFIX: &str = "-agents";

/// Where a seat's agents clone goes: the worktree's sibling `<name>-agents`.
pub fn seat_agents_clone_path(worktree: &Path) -> Option<PathBuf> {
    let name = worktree.file_name()?.to_str()?;
    Some(worktree.with_file_name(format!("{name}{SEAT_AGENTS_CLONE_SUFFIX}")))
}

/// Cut (or reuse) the seat's clone of the agents repository `source` names,
/// and record it on the seat's worktree record.
///
/// Refuses a source that is not an agents repository (its `path` is not the
/// repository root) or that pins a sha rather than a branch: a seat that
/// may write needs a branch to push.
pub(crate) fn cut_seat_agents_clone(
    app: &AppHandle,
    state: &AppState,
    source: &packs_cache::ProjectPackSource,
    worktree: &Path,
) -> Result<PathBuf, String> {
    if !buzz_core_pkg::project_pack_source::is_root_pack_path(&source.path) {
        return Err(format!(
            "the project's role source ({}, path {}) is not an agents repository, so this seat \
             cannot be given one",
            source.repo, source.path
        ));
    }
    let Some(ref_name) = source.git_ref.as_deref() else {
        return Err(format!(
            "the project's role source ({}) pins a commit rather than a branch, so a seat has \
             no branch of the agents repository to work on",
            source.repo
        ));
    };
    let branch = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
    let (owner, id) = packs_cache::parse_repo_coordinate(&source.repo)?;
    let packs_root = packs_cache::packs_root(app)?;
    let cache = packs_cache::packs_checkout_dir(&packs_root, &owner, &id);
    if !cache.join(".git").is_dir() {
        return Err(format!(
            "this computer has no packs cache for {} to clone the agents repository from",
            source.repo
        ));
    }
    let dest = seat_agents_clone_path(worktree).ok_or_else(|| {
        format!(
            "{} has no name to put an agents clone beside",
            worktree.display()
        )
    })?;
    let auth = crate::commands::project_git_exec::build_git_auth_config(state)?;
    let relay_http =
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(state));
    let origin = packs_cache::packs_clone_url(&relay_http, &owner, &id);
    if !dest.join(".git").is_dir() {
        let cache_str = cache.to_string_lossy().into_owned();
        let dest_str = dest.to_string_lossy().into_owned();
        run_git(
            &[
                "clone", "--quiet", "--branch", branch, "--", &cache_str, &dest_str,
            ],
            None,
            &auth,
        )
        .map_err(|error| format!("could not clone the agents repository for this seat: {error}"))?;
    }
    run_git(
        &["remote", "set-url", "origin", "--", &origin],
        Some(&dest),
        &auth,
    )?;
    if let Err(error) =
        crate::coding_sessions::workdir_store::attach_agents_clone(app, state, worktree, &dest)
    {
        tracing::warn!(
            target: "seat_agents_clone",
            %error,
            "the seat's agents clone was cut but not recorded on its worktree; it will not be \
             removed with the tree"
        );
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clone_sits_beside_the_worktree() {
        assert_eq!(
            seat_agents_clone_path(Path::new("/src/proj.worktrees/lane")),
            Some(PathBuf::from("/src/proj.worktrees/lane-agents"))
        );
        assert_eq!(seat_agents_clone_path(Path::new("/")), None);
    }
}
