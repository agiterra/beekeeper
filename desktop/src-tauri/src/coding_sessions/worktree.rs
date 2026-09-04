//! Git worktrees for coding sessions, created on this machine only.
//!
//! A session that runs in the checkout a person is also editing shares one
//! index, one HEAD, and one set of uncommitted changes with them. A worktree
//! gives the session its own directory and its own branch — off a chosen
//! source branch, the trunk by default — so the agent can commit, switch, and
//! stash without touching what the person has open.
//!
//! Placement is a sibling of the repository — `<repo>.worktrees/<slug>` — for
//! two reasons: nothing is written *inside* the repository (no untracked
//! directory appearing in every `git status`), and one predictable folder
//! holds every session's worktree so they can be found and deleted by hand.
//!
//! Nothing here is published. Like [`super::workdir_store`], a worktree path
//! names one person's disk and stays on it.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::{
    load_workdir_store, save_workdir_store, seat_worktree_key, CodingSessionSeatWorktree,
    WORKDIR_STORE_VERSION,
};
use crate::commands::project_git_exec::{build_local_git_auth_config, run_git};
use crate::util::now_iso;

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
    /// Branch the new branch starts from: the requested source, else the
    /// repository's default (`main`, then `master`). `None` means the
    /// checkout's current `HEAD` — the fallback when neither exists.
    pub source: Option<String>,
    /// The one sentence explaining why no worktree can be planned.
    pub problem: Option<String>,
}

/// The branches a session's worktree could start from.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionWorktreeBranches {
    /// Local branches, most recently committed first. Empty when the working
    /// directory is not a git checkout.
    pub branches: Vec<String>,
    /// The branch a new session should start from unless told otherwise:
    /// `main` when it exists, else `master`, else nothing.
    pub default_branch: Option<String>,
    /// The branch the checkout itself has checked out, when it is on one.
    pub head_branch: Option<String>,
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
    // Unchanged behaviour, stated in terms of the shared vocabulary: this is
    // still the legacy holder, and the next commit is what moves planning off
    // it. Named here so the one place that composes a path and the three
    // guards that admit one are visibly the same rule.
    let file_name = repo_root.file_name()?.to_str()?;
    let parent = repo_root.parent()?;
    Some(parent.join(format!("{file_name}.worktrees")))
}

/// What a folder turns out to belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRepo {
    /// The canonical repository folder: the *main* worktree's root, or, for a
    /// bare repository, the folder holding the common git dir.
    pub(crate) root: PathBuf,
    /// True when the repository itself has no working tree of its own.
    pub(crate) bare: bool,
}

/// Resolve the repository `folder` belongs to.
///
/// `Ok(None)` when the path is in no repository at all — the ordinary case
/// for a scratch directory, and not an error worth a red message. `Err` is
/// reserved for git being unusable.
///
/// Asks `--git-common-dir`, not `--show-toplevel`. `--show-toplevel` answers
/// about the *current* worktree, which is wrong twice over: inside a linked
/// worktree it names that worktree rather than the repository, so a second
/// worktree home gets built beside the first; and in a bare repository it
/// fails outright (`fatal: this operation must be run in a work tree`), which
/// this function used to swallow into "not a git checkout" for a directory
/// that is plainly a repository. The common dir is the same answer from every
/// vantage point — a subdirectory, a linked worktree, or the bare folder.
///
/// `--path-format=absolute` is load-bearing: without it git answers relative
/// to the cwd, and the derived root would be quietly wrong rather than
/// missing.
fn resolve_repo(folder: &Path) -> Result<Option<ResolvedRepo>, String> {
    let auth = build_local_git_auth_config()?;
    let Ok(output) = run_git(
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        Some(folder),
        &auth,
    ) else {
        // `not a git repository` and friends: a fact about the directory,
        // not a failure of the probe.
        return Ok(None);
    };
    let common_dir = output.trim();
    if common_dir.is_empty() {
        return Ok(None);
    }
    let root = buzz_core_pkg::worktree_placement::repo_root_from_common_dir(Path::new(common_dir));
    // Bareness is a property of the *repository*, and `--is-bare-repository`
    // answers for the current worktree — from a linked worktree of a bare repo
    // it says `false`. `worktree list --porcelain`'s first record is always
    // the main worktree and carries a `bare` line, so ask it instead.
    let bare = run_git(&["worktree", "list", "--porcelain"], Some(&root), &auth)
        .ok()
        .is_some_and(|listing| {
            listing
                .lines()
                .take_while(|line| !line.trim().is_empty())
                .any(|line| line.trim() == "bare")
        });
    Ok(Some(ResolvedRepo { root, bare }))
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

/// Local branches of `repo_root`, most recently committed first.
fn local_branches(repo_root: &Path) -> Result<Vec<String>, String> {
    let auth = build_local_git_auth_config()?;
    let output = run_git(
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
        Some(repo_root),
        &auth,
    )?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

/// The branch a session starts from when nobody chose one.
///
/// `main`, then `master` — the trunk, not whatever the checkout happens to
/// have checked out. Sessions used to branch from `HEAD`, and a checkout
/// parked on an old topic branch quietly became the ancestor of every new
/// session made from it. `None` when neither exists; only then does `HEAD`
/// remain the start point.
fn default_source(branches: &[String]) -> Option<String> {
    for candidate in ["main", "master"] {
        if branches.iter().any(|branch| branch == candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// The branch `repo_root` currently has checked out, when it is on one.
fn head_branch(repo_root: &Path) -> Option<String> {
    let auth = build_local_git_auth_config().ok()?;
    let output = run_git(
        &["symbolic-ref", "--short", "-q", "HEAD"],
        Some(repo_root),
        &auth,
    )
    .ok()?;
    let branch = output.trim();
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_string())
    }
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
fn plan(
    workdir: &str,
    name: &str,
    source: Option<&str>,
) -> Result<CodingSessionWorktreePlan, String> {
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
    let Some(resolved) = resolve_repo(workdir_path)? else {
        return Ok(CodingSessionWorktreePlan {
            // Only reached when git says the folder is in no repository at
            // all. A bare repository resolves like any other now, so it no
            // longer lands here claiming not to be a checkout.
            problem: Some(
                "That folder is not a git repository, so it has no worktrees.".to_string(),
            ),
            ..Default::default()
        });
    };
    let repo_root = resolved.root;
    let Some(parent) = worktree_parent(&repo_root) else {
        return Ok(CodingSessionWorktreePlan {
            repo_root: Some(repo_root.to_string_lossy().into_owned()),
            problem: Some("That repository has no parent directory to hold worktrees.".to_string()),
            ..Default::default()
        });
    };
    // The start point is checked against the repository's real branches, not
    // trusted from the caller: the branch list the dialog showed can go stale
    // between opening it and submitting.
    let branches = local_branches(&repo_root)?;
    let source = match source.map(str::trim).filter(|source| !source.is_empty()) {
        Some(requested) => {
            if !branches.iter().any(|branch| branch == requested) {
                return Ok(CodingSessionWorktreePlan {
                    repo_root: Some(repo_root.to_string_lossy().into_owned()),
                    problem: Some(format!(
                        "That repository has no branch named \"{requested}\"."
                    )),
                    ..Default::default()
                });
            }
            Some(requested.to_string())
        }
        None => default_source(&branches),
    };
    let (slug, disambiguated) = free_slug(&repo_root, &parent, &slug)?;
    Ok(CodingSessionWorktreePlan {
        repo_root: Some(repo_root.to_string_lossy().into_owned()),
        path: Some(parent.join(&slug).to_string_lossy().into_owned()),
        branch: Some(slug.clone()),
        slug: Some(slug),
        disambiguated,
        source,
        problem: None,
    })
}

/// Branches of the repository containing `workdir`, for the source picker.
///
/// Not being in a repository is an empty answer, not an error: the dialog
/// simply has no branches to offer, and the plan will say why in its own
/// words.
fn list_branches(workdir: &str) -> Result<CodingSessionWorktreeBranches, String> {
    let workdir_path = Path::new(workdir);
    if !workdir_path.is_dir() {
        return Ok(CodingSessionWorktreeBranches::default());
    }
    let Some(resolved) = resolve_repo(workdir_path)? else {
        return Ok(CodingSessionWorktreeBranches::default());
    };
    let repo_root = resolved.root;
    let branches = local_branches(&repo_root)?;
    let default_branch = default_source(&branches);
    let head_branch = head_branch(&repo_root);
    Ok(CodingSessionWorktreeBranches {
        branches,
        default_branch,
        head_branch,
    })
}

/// What would happen if this session used a worktree — checked, not guessed.
#[tauri::command]
pub async fn plan_coding_session_worktree(
    workdir: String,
    name: String,
    source: Option<String>,
) -> Result<CodingSessionWorktreePlan, String> {
    tauri::async_runtime::spawn_blocking(move || plan(&workdir, &name, source.as_deref()))
        .await
        .map_err(|error| format!("worktree plan task failed: {error}"))?
}

/// The branches a worktree here could start from, and which one is default.
#[tauri::command]
pub async fn list_coding_session_worktree_branches(
    workdir: String,
) -> Result<CodingSessionWorktreeBranches, String> {
    tauri::async_runtime::spawn_blocking(move || list_branches(&workdir))
        .await
        .map_err(|error| format!("worktree branches task failed: {error}"))?
}

/// Create the worktree and its branch, and answer with where they landed.
///
/// The plan is recomputed here rather than trusted from the caller: minutes
/// can pass between opening the dialog and submitting it, and creating a
/// worktree over a directory that appeared in the meantime would be worse
/// than renaming.
fn create(
    workdir: &str,
    name: &str,
    source: Option<&str>,
) -> Result<CodingSessionWorktreeCreated, String> {
    let planned = plan(workdir, name, source)?;
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
    // Branch from the planned source — the chosen branch, or the trunk — and
    // from `HEAD` only when the repository has neither. No argument can read
    // as an option: the path is absolute, the slug never starts with a
    // hyphen, and the source is spelled as a full ref.
    let start_point = match planned.source.as_deref() {
        Some(source) => format!("refs/heads/{source}"),
        None => "HEAD".to_string(),
    };
    run_git(
        &["worktree", "add", "-b", &branch, &path, &start_point],
        Some(Path::new(&repo_root)),
        &auth,
    )?;
    Ok(CodingSessionWorktreeCreated {
        path,
        branch,
        repo_root,
    })
}

/// Create a worktree, and record it when the caller says whose it is.
///
/// `sessionRef` and `seatLabel` are optional so the pre-L11 call shape still
/// works, but a tree cut without them is **unrecorded**: the host can never
/// name it and will never remove it. That is the whole gap this record closes,
/// so a hire path that omits them is a bug, not a style.
///
/// Recording is best-effort *after* the tree exists. A failed record leaves an
/// unrecorded worktree, which is the conservative outcome; failing the create
/// instead would leave a directory on disk and tell the caller it has none.
#[tauri::command]
pub async fn create_coding_session_worktree(
    app: AppHandle,
    state: State<'_, AppState>,
    workdir: String,
    name: String,
    source: Option<String>,
    session_ref: Option<String>,
    seat_label: Option<String>,
) -> Result<CodingSessionWorktreeCreated, String> {
    let created =
        tauri::async_runtime::spawn_blocking(move || create(&workdir, &name, source.as_deref()))
            .await
            .map_err(|error| format!("worktree create task failed: {error}"))??;
    if let (Some(session_ref), Some(seat_label)) = (session_ref, seat_label) {
        if let Err(error) =
            record_created_worktree(&app, &state, &session_ref, &seat_label, &created)
        {
            eprintln!("buzz-desktop: failed to record a seat worktree: {error}");
        }
    }
    Ok(created)
}

/// Write one created worktree into the host's durable record.
fn record_created_worktree(
    app: &AppHandle,
    state: &AppState,
    session_ref: &str,
    seat_label: &str,
    created: &CodingSessionWorktreeCreated,
) -> Result<(), String> {
    let mut store = load_workdir_store(app)?;
    store.record_seat_worktree(
        session_ref,
        seat_label,
        CodingSessionSeatWorktree {
            path: PathBuf::from(&created.path),
            branch: created.branch.clone(),
            repo_root: PathBuf::from(&created.repo_root),
            created_at: now_iso(),
        },
    )?;
    store.version = WORKDIR_STORE_VERSION;
    save_workdir_store(app, state, &store)
}

/// Remove one worktree directory and forget its record.
///
/// Never `--force`: git refuses to remove a tree holding modifications, and
/// that refusal is a feature here — it is the last guard under every decision
/// made further up. `git worktree prune` afterwards clears the administrative
/// entry the removal leaves behind.
pub(crate) fn remove_worktree(repo_root: &Path, path: &Path) -> Result<(), String> {
    let auth = build_local_git_auth_config()?;
    let path = path.to_string_lossy().into_owned();
    run_git(&["worktree", "remove", "--", &path], Some(repo_root), &auth)?;
    run_git(&["worktree", "prune"], Some(repo_root), &auth)?;
    Ok(())
}

/// Remove a recorded seat worktree and drop the record naming it.
pub(crate) fn remove_recorded_seat_worktree(
    app: &AppHandle,
    state: &AppState,
    session_ref: &str,
    seat_label: &str,
) -> Result<String, String> {
    let key = seat_worktree_key(session_ref, seat_label);
    let mut store = load_workdir_store(app)?;
    let Some(entry) = store.worktrees.get(&key).cloned() else {
        return Err(format!("this host has no worktree recorded for {key}"));
    };
    remove_worktree(&entry.repo_root, &entry.path)?;
    store.forget_seat_worktree(&key);
    store.version = WORKDIR_STORE_VERSION;
    save_workdir_store(app, state, &store)?;
    Ok(entry.path.to_string_lossy().into_owned())
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
