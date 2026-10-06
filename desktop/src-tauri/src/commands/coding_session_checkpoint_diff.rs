//! The per-turn git diff a client can request from a turn checkpoint (SV-30,
//! checkpoints brief slice S2, back-end half).
//!
//! A checkpoint (kind 44231) names two tree object ids, `baseTree` and `tree`,
//! written by the provider into the session's own repository and kept alive
//! by `refs/beekeeper/checkpoints/<sessionId>/…`. Those objects exist only on
//! the computer that ran the turn. This command answers, for one pair, either
//! the patch or *why there is none* — never an empty diff standing in for
//! "could not read":
//!
//! * `local` — both trees are in the session's recorded checkout here, and
//!   the patch was produced inside that checkout's project boundary;
//! * `objects_missing` — a checkout is recorded here but does not hold one or
//!   both trees (the turn ran on another machine, or the objects were pruned);
//! * `no_checkout` — this computer records no checkout for the session;
//! * `baseline_missing` — the checkpoint carried no `baseTree` (its baseline
//!   capture timed out), so the caller must name the previous checkpoint's
//!   tree and say that is what it did.
//!
//! The patch keeps `diff_from_repo`'s per-file 2,000-line cap and its refusal
//! to render a converter failure as "no change", but reads paths as Git's
//! bytes (`coding_session_checkpoint_diff_files.rs`): a name that is not UTF-8
//! is counted in `filesNotListed`, never listed under a name that does not
//! exist.
//!
//! T3 Code reads the same diff from two checkpoint refs
//! (`apps/server/src/checkpointing/CheckpointDiffQuery.ts`). Two deliberate
//! differences: whitespace changes are shown (T3 defaults to ignoring them, and
//! hiding a change is not ours to choose), and a missing endpoint is a typed
//! answer rather than an error, because here it is the ordinary state of a
//! session that ran on another computer.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use super::project_git_diff::ProjectRepoDiffInfo;
use crate::coding_sessions::workdir_store::{
    load_workdir_store_readonly, CodingSessionWorkdirStore,
};

/// Which recorded checkout answered, without naming where it is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointDiffCheckout {
    /// The seat worktree this host cut for the execution (`worktrees` record
    /// whose `sessionId` is the request's `target`).
    SeatWorktree,
    /// The project's own checkout on this computer (`byProject[projectRef]`),
    /// used only when no seat worktree is recorded for the execution.
    ProjectCheckout,
}

/// What one checkpoint diff request found.
#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CodingSessionCheckpointDiff {
    /// Both trees are here; the patch between them.
    Local {
        /// Which recorded checkout held them.
        checkout: CheckpointDiffCheckout,
        /// Files, counts and capped patches, as `get_project_local_repo_diff`.
        /// The totals cover every changed file, listed or not.
        diff: ProjectRepoDiffInfo,
        /// Changed files with no entry in `diff.files`: a name that is not
        /// UTF-8 (counted, never named), or past the 250-file list cap.
        #[serde(rename = "filesNotListed")]
        files_not_listed: u64,
    },
    /// A checkout is recorded here, but these tree ids are not in it.
    ObjectsMissing {
        /// The requested ids this checkout does not hold, in request order.
        missing: Vec<String>,
    },
    /// No checkout is recorded on this computer for the session.
    NoCheckout,
    /// The request named no `fromTree`: the checkpoint has no baseline.
    BaselineMissing,
}

/// A checkout the diff may run in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedCheckout {
    /// The working tree Git runs in.
    pub tree: PathBuf,
    /// The repository's main checkout, when the tree is a linked worktree.
    pub repo_root: Option<PathBuf>,
    /// Which record named it.
    pub source: CheckpointDiffCheckout,
}

/// A tree id as the checkpoint wire carries it: 40 (SHA-1) or 64 (SHA-256)
/// hex digits, lowercased. Anything else is refused before Git sees it, so no
/// id can be read as an option or a revision expression.
pub(crate) fn clean_tree_oid(value: &str) -> Result<String, String> {
    let value = value.trim();
    if matches!(value.len(), 40 | 64) && value.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err("a checkpoint tree id must be 40 or 64 hexadecimal digits".to_string())
    }
}

/// The checkout the workdir store records for one execution.
///
/// The join is the one the worktree list already uses to pair a running
/// execution with its tree: the `worktrees` map is keyed
/// `<sessionRef>/<seatLabel>` (`workdir_store_worktrees.rs`
/// `seat_worktree_key`), and each record's `sessionId` is the provider session
/// id, the 44223 `target` (`worktree_prune.rs` `CodingSessionSeatWorktreeRow
/// ::session_id`). A record whose directory is gone does not count. Only when
/// no seat worktree answers does the project's own checkout
/// (`byProject[projectRef]`, the directory a create in that project resolves
/// to) stand in; the object check then decides whether it really holds the
/// turn.
pub(crate) fn resolve_checkout(
    store: &CodingSessionWorkdirStore,
    session_ref: &str,
    target: &str,
    project_ref: Option<&str>,
) -> Option<ResolvedCheckout> {
    let prefix = format!("{}/", session_ref.trim());
    let target = target.trim();
    if !target.is_empty() {
        let seat = store.worktrees.iter().find(|(key, entry)| {
            key.starts_with(&prefix)
                && entry.session_id.as_deref().map(str::trim) == Some(target)
                && entry.path.is_dir()
        });
        if let Some((_, entry)) = seat {
            return Some(ResolvedCheckout {
                tree: entry.path.clone(),
                repo_root: Some(entry.repo_root.clone()),
                source: CheckpointDiffCheckout::SeatWorktree,
            });
        }
    }
    let project_ref = project_ref.map(str::trim).filter(|key| !key.is_empty())?;
    let entry = store.by_project.get(project_ref)?;
    entry.path.is_dir().then(|| ResolvedCheckout {
        tree: entry.path.clone(),
        repo_root: None,
        source: CheckpointDiffCheckout::ProjectCheckout,
    })
}

/// Whether `oid` is present in the checkout as a tree (or a commit, which
/// peels to one). `Ok(false)` is "not here"; an object that is here but is
/// not a tree, or a Git that could not answer, is an error — neither is the
/// ordinary "lives on another machine" state.
fn tree_present(
    plan: &beekeeper_session_provider_pkg::execution_scope_host::HostLaunchPlan,
    dir: &Path,
    oid: &str,
) -> Result<bool, String> {
    let mut command = plan.git_command(dir, &["cat-file", "-e", "--end-of-options", oid]);
    command.stdin(std::process::Stdio::null());
    crate::util::configure_no_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("git could not check object {oid}")
            } else {
                stderr
            });
        }
    }
    let peeled = format!("{oid}^{{tree}}");
    crate::coding_sessions::host_git::run(
        plan,
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &peeled,
        ],
    )
    .map(|_| true)
    .map_err(|_| format!("object {oid} is not a tree"))
}

/// The checkpoint diff against an already-resolved checkout.
///
/// Order is part of the contract: a malformed id is refused first, then a
/// missing baseline is named (it needs no checkout to know), then the absence
/// of a checkout, then absent objects, and only then does a diff run.
///
/// # Errors
/// A malformed tree id; the checkout's project boundary could not be
/// prepared; an id names a non-tree object; or Git failed producing the
/// patch.
pub(crate) fn checkpoint_diff_in(
    checkout: Option<&ResolvedCheckout>,
    from_tree: Option<&str>,
    to_tree: &str,
) -> Result<CodingSessionCheckpointDiff, String> {
    let to_tree = clean_tree_oid(to_tree)?;
    let from_tree = from_tree.map(clean_tree_oid).transpose()?;
    let Some(from_tree) = from_tree else {
        return Ok(CodingSessionCheckpointDiff::BaselineMissing);
    };
    let Some(checkout) = checkout else {
        return Ok(CodingSessionCheckpointDiff::NoCheckout);
    };
    let plan =
        crate::coding_sessions::host_git::prepare(&crate::coding_sessions::host_git::Workspace {
            tree: &checkout.tree,
            repo_root: checkout.repo_root.as_deref(),
            name: "checkpoint-diff",
            host_branch: None,
            host_read: &[],
        })?;
    let mut missing = Vec::new();
    for oid in [&from_tree, &to_tree] {
        if !missing.contains(oid) && !tree_present(&plan, &checkout.tree, oid)? {
            missing.push(oid.clone());
        }
    }
    if !missing.is_empty() {
        return Ok(CodingSessionCheckpointDiff::ObjectsMissing { missing });
    }
    let answer = files::checkpoint_diff_files(&plan, &checkout.tree, &from_tree, &to_tree)?;
    Ok(CodingSessionCheckpointDiff::Local {
        checkout: checkout.source,
        diff: answer.diff,
        files_not_listed: answer.files_not_listed,
    })
}

/// The git diff between two checkpoint trees of one coding-session execution,
/// read from this computer's recorded checkout for it.
///
/// `target` is the execution's provider session id (the 44223 target);
/// `projectRef`, optional, lets a session that ran in the project's own
/// checkout rather than a seat worktree be found. `fromTree: null` answers
/// `baseline_missing`.
///
/// # Errors
/// A malformed tree id, an unreadable workdir store, or Git failing (see
/// [`checkpoint_diff_in`]). Errors carry Git's own complaint, as
/// `get_project_local_repo_diff` does, and no path this module composed.
#[tauri::command]
pub async fn coding_session_checkpoint_diff(
    app: AppHandle,
    session_ref: String,
    target: String,
    from_tree: Option<String>,
    to_tree: String,
    project_ref: Option<String>,
) -> Result<CodingSessionCheckpointDiff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Refuse a malformed id before reading anything.
        clean_tree_oid(&to_tree)?;
        if let Some(from_tree) = &from_tree {
            clean_tree_oid(from_tree)?;
        }
        let checkout = if from_tree.is_some() {
            let store = load_workdir_store_readonly(&app)?;
            resolve_checkout(&store, &session_ref, &target, project_ref.as_deref())
        } else {
            None
        };
        checkpoint_diff_in(checkout.as_ref(), from_tree.as_deref(), &to_tree)
    })
    .await
    .map_err(|error| format!("checkpoint diff task failed: {error}"))?
}

#[path = "coding_session_checkpoint_diff_files.rs"]
mod files;

#[cfg(test)]
#[path = "coding_session_checkpoint_diff_tests.rs"]
mod tests;
