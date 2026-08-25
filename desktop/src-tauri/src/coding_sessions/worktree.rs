//! Git worktrees for coding sessions, created on this machine only.
//!
//! A session that runs in the checkout a person is also editing shares one
//! index, one HEAD, and one set of uncommitted changes with them. A worktree
//! gives the session its own directory and its own branch off the checkout's
//! current HEAD, so the agent can commit, switch, and stash without touching
//! what the person has open.
//!
//! Placement is a sibling of the repository — `<repo>.worktrees/<slug>` — for
//! two reasons: nothing is written *inside* the repository (no untracked
//! directory appearing in every `git status`), and one predictable folder
//! holds every session's worktree so they can be found and deleted by hand.
//!
//! Nothing here is published. Like [`super::workdir_store`], a worktree path
//! names one person's disk and stays on it.

use std::path::{Path, PathBuf};

use crate::commands::project_git_exec::{build_local_git_auth_config, run_git};

/// Longest slug accepted. Long enough for a four-word session name, short
/// enough that the resulting path stays workable on every platform.
const MAX_WORKTREE_SLUG_LEN: usize = 48;

/// What creating a worktree for this working directory would do.
///
/// Every field is either a fact about this machine or `None` with a
/// [`Self::problem`] that says why. The dialog renders it verbatim rather
/// than guessing a path it cannot verify.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionWorktreePlan {
    /// Repository root of the working directory, when it is inside one.
    pub repo_root: Option<String>,
    /// Directory the worktree would occupy. Absent when there is a problem.
    pub path: Option<String>,
    /// Branch that would be created, matching the directory's slug.
    pub branch: Option<String>,
    /// The slug actually chosen — the requested one, or a disambiguated form.
    pub slug: Option<String>,
    /// True when the requested slug was already taken and this one differs.
    pub disambiguated: bool,
    /// The one sentence explaining why no worktree can be planned.
    pub problem: Option<String>,
}

/// A worktree that now exists.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionWorktreeCreated {
    pub path: String,
    pub branch: String,
    pub repo_root: String,
}

/// Reduce a free-text name to the slug a directory and a branch can share.
///
/// Lowercase, ASCII alphanumerics and single hyphens, no leading or trailing
/// hyphen. Everything else — spaces, punctuation, non-ASCII — collapses to a
/// separator, so "Fix the push timeout!" becomes "fix-the-push-timeout". An
/// empty result is `None`: there is no safe fallback slug, and inventing one
/// would name a branch after nothing.
pub fn worktree_slug(name: &str) -> Option<String> {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        return None;
    }
    let mut slug: String = slug.chars().take(MAX_WORKTREE_SLUG_LEN).collect();
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

/// Four hex characters of a fresh v4 UUID — enough to separate two sessions
/// that chose the same name, short enough to keep the branch readable.
fn disambiguator() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..4].to_string()
}

/// The directory holding every worktree for `repo_root`.
fn worktree_parent(repo_root: &Path) -> Option<PathBuf> {
    let file_name = repo_root.file_name()?.to_str()?;
    let parent = repo_root.parent()?;
    Some(parent.join(format!("{file_name}.worktrees")))
}

/// Resolve the repository root containing `workdir`.
///
/// Returns `Ok(None)` when the path is simply not in a git checkout — the
/// ordinary case for a scratch directory, and not an error worth a red
/// message. `Err` is reserved for git being unusable.
fn repo_root_of(workdir: &Path) -> Result<Option<PathBuf>, String> {
    let auth = build_local_git_auth_config()?;
    match run_git(
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
        Some(workdir),
        &auth,
    ) {
        Ok(output) => {
            let root = output.trim();
            if root.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(root)))
            }
        }
        // `not a git repository` and friends: a fact about the directory,
        // not a failure of the probe.
        Err(_) => Ok(None),
    }
}

/// Whether `branch` already exists in `repo_root`.
fn branch_exists(repo_root: &Path, branch: &str) -> Result<bool, String> {
    let auth = build_local_git_auth_config()?;
    let reference = format!("refs/heads/{branch}");
    Ok(run_git(
        &["show-ref", "--verify", "--quiet", &reference],
        Some(repo_root),
        &auth,
    )
    .is_ok())
}

/// The first slug whose directory and branch are both free, starting from
/// `requested` and falling back to a random suffix.
fn free_slug(repo_root: &Path, parent: &Path, requested: &str) -> Result<(String, bool), String> {
    if !parent.join(requested).exists() && !branch_exists(repo_root, requested)? {
        return Ok((requested.to_string(), false));
    }
    // Bounded: each attempt draws fresh randomness, so exhausting eight is
    // not a collision problem but a signal that something else is wrong.
    for _ in 0..8 {
        let candidate = format!("{requested}-{}", disambiguator());
        if !parent.join(&candidate).exists() && !branch_exists(repo_root, &candidate)? {
            return Ok((candidate, true));
        }
    }
    Err(format!(
        "could not find a free worktree name near \"{requested}\""
    ))
}

/// Plan a worktree without creating anything.
fn plan(workdir: &str, name: &str) -> Result<CodingSessionWorktreePlan, String> {
    let Some(slug) = worktree_slug(name) else {
        return Ok(CodingSessionWorktreePlan {
            problem: Some("Give the worktree a name — letters and numbers.".to_string()),
            ..Default::default()
        });
    };
    let workdir_path = Path::new(workdir);
    if !workdir_path.is_dir() {
        return Ok(CodingSessionWorktreePlan {
            problem: Some("Choose an existing working directory first.".to_string()),
            ..Default::default()
        });
    }
    let Some(repo_root) = repo_root_of(workdir_path)? else {
        return Ok(CodingSessionWorktreePlan {
            problem: Some(
                "That working directory is not a git checkout, so it has no worktrees.".to_string(),
            ),
            ..Default::default()
        });
    };
    let Some(parent) = worktree_parent(&repo_root) else {
        return Ok(CodingSessionWorktreePlan {
            repo_root: Some(repo_root.to_string_lossy().into_owned()),
            problem: Some("That repository has no parent directory to hold worktrees.".to_string()),
            ..Default::default()
        });
    };
    let (slug, disambiguated) = free_slug(&repo_root, &parent, &slug)?;
    Ok(CodingSessionWorktreePlan {
        repo_root: Some(repo_root.to_string_lossy().into_owned()),
        path: Some(parent.join(&slug).to_string_lossy().into_owned()),
        branch: Some(slug.clone()),
        slug: Some(slug),
        disambiguated,
        problem: None,
    })
}

/// What would happen if this session used a worktree — checked, not guessed.
#[tauri::command]
pub async fn plan_coding_session_worktree(
    workdir: String,
    name: String,
) -> Result<CodingSessionWorktreePlan, String> {
    tauri::async_runtime::spawn_blocking(move || plan(&workdir, &name))
        .await
        .map_err(|error| format!("worktree plan task failed: {error}"))?
}

/// Create the worktree and its branch, and answer with where they landed.
///
/// The plan is recomputed here rather than trusted from the caller: minutes
/// can pass between opening the dialog and submitting it, and creating a
/// worktree over a directory that appeared in the meantime would be worse
/// than renaming.
#[tauri::command]
pub async fn create_coding_session_worktree(
    workdir: String,
    name: String,
) -> Result<CodingSessionWorktreeCreated, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let planned = plan(&workdir, &name)?;
        if let Some(problem) = planned.problem {
            return Err(problem);
        }
        let (Some(repo_root), Some(path), Some(branch)) =
            (planned.repo_root, planned.path, planned.branch)
        else {
            return Err("could not plan a worktree for this working directory".to_string());
        };
        if let Some(parent) = Path::new(&path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create worktree folder: {error}"))?;
        }
        let auth = build_local_git_auth_config()?;
        // Branch from the checkout's current HEAD: the session starts from
        // what the person is looking at, which is what "use a worktree" means
        // to them. Neither argument can read as an option — the path is
        // absolute and the slug never starts with a hyphen.
        run_git(
            &["worktree", "add", "-b", &branch, &path, "HEAD"],
            Some(Path::new(&repo_root)),
            &auth,
        )?;
        Ok(CodingSessionWorktreeCreated {
            path,
            branch,
            repo_root,
        })
    })
    .await
    .map_err(|error| format!("worktree create task failed: {error}"))?
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
