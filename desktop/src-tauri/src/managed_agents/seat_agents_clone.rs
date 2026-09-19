//! A seat's clone of the project's agents repository (spec § 4.11).
//!
//! When `team.yml` grants a role `workspace.agents_repo: read | write`, the
//! host cuts a clone of `<slug>-beekeeper-agents` beside the seat's worktree
//! (`<worktree>-agents`) from its own packs cache, points `origin` at the
//! relay so a `write` seat's push lands there, and records the path on the
//! seat's worktree record so disposal removes it with the tree. What the
//! seat may do in it is the relay's push gate and, on Claude, the write
//! fence's call — this module only puts the files where the briefing says.
//!
//! **Why the cache and not the relay.** The clone is cut from this host's
//! packs cache, which is the branch's last sync rather than the relay's tip
//! (ledger 162), and `origin` is pointed at the relay immediately after — so
//! a `write` seat has a real upstream from its first command: `git fetch`
//! brings it to the relay's tip and `git push` goes to the relay under the
//! seat's own credentials and its owner's tier. Cutting from the relay URL
//! instead would make every hire wait on a network clone, would need the
//! host's credentials for a repository the *seat* is meant to authenticate
//! to, and would fetch bytes the cache already holds — for a repository the
//! host had, by definition, just synced to stage this seat's role pack. Spec
//! § 4.11 says "clones the repository from its packs cache", and that is what
//! this does.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::app_state::AppState;
use crate::commands::project_git_exec::{build_local_clone_git_auth_config, run_git};
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
    // The clone and the `set-url` after it are both local: the remote is a
    // directory in this host's packs cache, and nothing here talks to the
    // relay. So this runs with the local configuration, which carries no
    // credential helper and no nsec — and, unlike every remote
    // configuration, allows git's `file` transport. With the remote
    // configuration this clone failed outright, `fatal: transport 'file' not
    // allowed`, and the hire it was staging died with it (ledger 169).
    let auth = build_local_clone_git_auth_config()?;
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

    /// The clone's own transport, in a throwaway repository.
    ///
    /// This is ledger 169's first bug as a test: with the configuration every
    /// *remote* operation uses, git refuses a clone whose remote is a path
    /// (`protocol.file.allow=never`), and the hire that was staging this seat
    /// dies with it. With the local clone configuration the same command
    /// succeeds. Nothing here touches the real repository or a real cache.
    #[test]
    fn a_clone_from_the_cache_needs_the_file_transport_only_the_local_config_allows() {
        use crate::commands::project_git_exec::{
            build_git_auth_config_for_keys, build_local_clone_git_auth_config,
            build_test_git_auth_config,
        };
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("cache");
        std::fs::create_dir(&source).expect("source dir");
        let seed = build_test_git_auth_config().expect("seed auth");
        run_git(
            &["init", "--quiet", "--initial-branch", "main"],
            Some(&source),
            &seed,
        )
        .expect("git init");
        std::fs::write(source.join("team.yml"), "schema: beekeeper-team/v1\n").expect("team.yml");
        run_git(&["add", "team.yml"], Some(&source), &seed).expect("git add");
        run_git(&["commit", "--quiet", "-m", "seed"], Some(&source), &seed).expect("git commit");

        let source_str = source.to_string_lossy().into_owned();
        let remote_auth =
            build_git_auth_config_for_keys(&nostr::Keys::generate()).expect("remote auth");
        let refused_dest = temp.path().join("refused").to_string_lossy().into_owned();
        let refusal = run_git(
            &[
                "clone",
                "--quiet",
                "--branch",
                "main",
                "--",
                &source_str,
                &refused_dest,
            ],
            None,
            &remote_auth,
        )
        .expect_err("the remote configuration must refuse a clone from a path");
        assert!(
            refusal.contains("transport 'file' not allowed"),
            "unexpected refusal: {refusal}"
        );

        let dest = temp.path().join("seat-agents");
        let dest_str = dest.to_string_lossy().into_owned();
        run_git(
            &[
                "clone",
                "--quiet",
                "--branch",
                "main",
                "--",
                &source_str,
                &dest_str,
            ],
            None,
            &build_local_clone_git_auth_config().expect("local clone auth"),
        )
        .expect("the local clone configuration clones from a path");
        assert!(dest.join(".git").is_dir(), "no clone at {}", dest.display());
        assert!(dest.join("team.yml").is_file());
    }

    #[test]
    fn the_clone_sits_beside_the_worktree() {
        assert_eq!(
            seat_agents_clone_path(Path::new("/src/proj.worktrees/lane")),
            Some(PathBuf::from("/src/proj.worktrees/lane-agents"))
        );
        assert_eq!(seat_agents_clone_path(Path::new("/")), None);
    }
}
