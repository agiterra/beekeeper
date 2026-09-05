//! Gather the facts one seat worktree's disposition needs, and act on it.
//!
//! The decision itself is not here — it is
//! [`buzz_core::worktree_lifecycle::classify_seat_worktree`], pure and
//! testable. This module does the two things that cannot be pure: it reads the
//! disk (does the directory still exist, how many lines does `git status
//! --porcelain` print, how large is `target/`), and it removes things.
//!
//! Three rules run through all of it:
//!
//! * **Uncommitted work is never deleted, and never silently.** A `Held` tree
//!   is removed only by a founder's own click, which names its path and its
//!   file count first, and even then `git worktree remove` runs without
//!   `--force` so git's own refusal is the last guard.
//! * **The host removes only what it recorded cutting.** A tree found on disk
//!   with no record is listed and counted, never adopted and never removed.
//! * **Unknown is not false.** When this host cannot establish whether a
//!   branch tip is on the relay, the row says it could not confirm it — it
//!   does not say the branch was never pushed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use buzz_core_pkg::worktree_lifecycle::{
    build_output_reclaimable, classify_seat_worktree, render_reclaimable_bytes,
    SeatWorktreeDisposition, SeatWorktreeFacts, RECLAIMABLE_BUILD_DIRS,
};

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::{
    load_workdir_store, seat_worktree_key, CodingSessionSeatWorktree,
};
use crate::coding_sessions::worktree::remove_recorded_seat_worktree;
use crate::commands::project_git_exec::{build_local_git_auth_config, run_git};

/// Upper bound on `git status --porcelain` lines counted for one tree.
///
/// A count beyond this is still "a lot of uncommitted work", and the number
/// stops mattering long before the parse cost does.
const MAX_COUNTED_DIRTY_FILES: u32 = 100_000;

/// What the caller already knows about a session, from the relay.
///
/// This host does not fold 44230 or fetch kind 30618 itself: the surfaces that
/// call in already hold those answers, and re-deriving them here would give
/// the app two projections of the same facts that could disagree.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionWorktreeSessionFacts {
    /// Canonical umbrella session UUID.
    pub session_ref: String,
    /// A 44230 revision folds this session to `closed` or `archived`.
    pub session_settled: bool,
    /// An execution is still running for this session.
    pub execution_live: bool,
    /// Whether the branch tip is named by a relay-signed kind 30618 right now.
    ///
    /// `None` means this caller could not establish it, which is not the same
    /// as `Some(false)` and never renders as one.
    #[serde(default)]
    pub tip_on_relay: Option<bool>,
    /// Seconds since the closure revision settled the session.
    #[serde(default)]
    pub settled_for_secs: Option<u64>,
}

/// One recorded seat worktree, with everything a surface needs to render it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionSeatWorktreeRow {
    /// `<sessionRef>/<seatLabel>` — the record's own key.
    pub key: String,
    /// Umbrella session the seat belongs to.
    pub session_ref: String,
    /// The seat's label inside that session.
    pub seat_label: String,
    /// Absolute path of the worktree directory.
    pub path: String,
    /// Branch the worktree has checked out.
    pub branch: String,
    /// Repository the worktree belongs to.
    pub repo_root: String,
    /// The disposition's stable token — `prunable`, `held`, `unrecorded`, …
    pub disposition: String,
    /// `git status --porcelain` lines, which exclude ignored paths.
    pub dirty_files: u32,
    /// Bytes of rebuildable build output, when they could be measured.
    pub reclaimable_bytes: Option<u64>,
    /// `{N} GB`, or `unknown` — never `0` for an unmeasurable directory.
    pub reclaimable_label: String,
    /// Whether that build output may be removed right now.
    pub reclaimable_now: bool,
    /// Seconds still to run on the grace window, when one applies.
    pub grace_remaining_secs: Option<u64>,
    /// Whether the directory is still on disk.
    pub exists: bool,
    /// Whether the caller established the tip's relay state at all.
    pub tip_on_relay_known: bool,
    /// The one sentence a surface shows for this row.
    pub detail: String,
}

/// What a reclaim actually removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionWorktreeReclaimed {
    /// Directories removed, absolute.
    pub removed: Vec<String>,
    /// Bytes freed, when they could be measured before removal.
    pub freed_bytes: Option<u64>,
    /// `{N} GB`, or `unknown`.
    pub freed_label: String,
}

/// Whether a path is one nothing here may ever remove.
///
/// Structural, not a list of somebody's directories: a tree is protected when
/// it *is* its repository's main checkout, when it holds the running process's
/// own working directory, or when it does not sit inside
/// `<repo_root>.worktrees/`. The hot development checkout is protected by the
/// first of those and a production checkout beside it by the third, without
/// this file ever naming a person's disk.
pub(crate) fn is_protected_worktree(repo_root: &Path, path: &Path) -> bool {
    if path == repo_root || repo_root.starts_with(path) {
        return true;
    }
    if !crate::coding_sessions::workdir_store::is_inside_worktree_parent(repo_root, path) {
        return true;
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.starts_with(path),
        // A process with no readable cwd cannot rule the tree out, so it does
        // not get to rule it in either.
        Err(_) => true,
    }
}

/// Count `git status --porcelain` lines in one worktree.
///
/// Ignored paths are excluded by that listing, which is exactly why `target/`
/// and `node_modules` never make a finished tree look like it holds edits. A
/// tree git cannot read counts as one dirty file rather than zero: an
/// unreadable tree is not a clean one.
pub(crate) fn count_dirty_files(path: &Path) -> u32 {
    let Ok(auth) = build_local_git_auth_config() else {
        return 1;
    };
    match run_git(&["status", "--porcelain"], Some(path), &auth) {
        Ok(output) => output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            .min(MAX_COUNTED_DIRTY_FILES as usize) as u32,
        Err(_) => 1,
    }
}

/// Bytes held by one directory, or `None` when it cannot be measured.
fn directory_bytes(path: &Path) -> Option<u64> {
    if !path.is_dir() {
        return Some(0);
    }
    // The path is always absolute, so it can never read as an option and no
    // `--` separator is needed (BSD and GNU `du` disagree about accepting one).
    let output = std::process::Command::new("du")
        .arg("-sk")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let kilobytes: u64 = text.split_whitespace().next()?.parse().ok()?;
    kilobytes.checked_mul(1024)
}

/// Bytes of rebuildable build output under one worktree.
///
/// `None` when any one directory could not be measured: a partial total
/// reported as a whole one would be a number nobody could act on.
pub(crate) fn reclaimable_bytes(path: &Path) -> Option<u64> {
    let mut total: u64 = 0;
    for name in RECLAIMABLE_BUILD_DIRS {
        total = total.checked_add(directory_bytes(&path.join(name))?)?;
    }
    Some(total)
}

/// The one sentence a surface shows for a disposition.
///
/// Composed here so `bee` and the app cannot drift into two vocabularies for
/// the same tree.
pub(crate) fn worktree_detail(
    disposition: SeatWorktreeDisposition,
    path: &str,
    tip_on_relay_known: bool,
) -> String {
    match disposition {
        SeatWorktreeDisposition::Prunable => format!("{path}: clean, will be removed"),
        SeatWorktreeDisposition::Held { dirty_files } => {
            format!("held: {dirty_files} uncommitted files")
        }
        SeatWorktreeDisposition::NotSettled => {
            format!("{path}: the session is not closed, so nothing is removed")
        }
        SeatWorktreeDisposition::TipNotOnRelay if !tip_on_relay_known => format!(
            "{path}: this host could not confirm the branch is on the relay, so nothing is removed"
        ),
        SeatWorktreeDisposition::TipNotOnRelay => format!(
            "{path}: the relay's current ref state does not hold this branch, so nothing is removed"
        ),
        SeatWorktreeDisposition::ExecutionLive => {
            format!("{path}: an execution is still running here")
        }
        SeatWorktreeDisposition::Unrecorded => {
            format!("{path}: this host never recorded cutting it, so it never removes it")
        }
        SeatWorktreeDisposition::Protected => format!("{path}: protected, never removed"),
        SeatWorktreeDisposition::WithinGrace { remaining_secs } => {
            let days = remaining_secs.div_ceil(24 * 60 * 60);
            format!("{path}: clean and pushed, kept {days} more days")
        }
    }
}

/// Build one row from a record and the session facts the caller supplied.
pub(crate) fn build_row(
    key: &str,
    entry: &CodingSessionSeatWorktree,
    session: &CodingSessionWorktreeSessionFacts,
) -> CodingSessionSeatWorktreeRow {
    let path_text = entry.path.to_string_lossy().into_owned();
    let exists = entry.path.is_dir();
    let is_protected = is_protected_worktree(&entry.repo_root, &entry.path);
    let dirty_files = if exists && !is_protected {
        count_dirty_files(&entry.path)
    } else {
        0
    };
    let facts = SeatWorktreeFacts {
        session_settled: session.session_settled,
        execution_live: session.execution_live,
        tip_on_relay: session.tip_on_relay.unwrap_or(false),
        dirty_files,
        recorded: true,
        is_protected,
        settled_for_secs: session.settled_for_secs,
    };
    let disposition = classify_seat_worktree(&facts);
    let bytes = if exists {
        reclaimable_bytes(&entry.path)
    } else {
        Some(0)
    };
    let (seat_label, session_ref) = split_key(key, &session.session_ref);
    let detail = worktree_detail(disposition, &path_text, session.tip_on_relay.is_some());
    CodingSessionSeatWorktreeRow {
        key: key.to_string(),
        session_ref,
        seat_label,
        detail,
        path: path_text,
        branch: entry.branch.clone(),
        repo_root: entry.repo_root.to_string_lossy().into_owned(),
        disposition: disposition.token().to_string(),
        dirty_files,
        reclaimable_bytes: bytes,
        reclaimable_label: render_reclaimable_bytes(bytes),
        reclaimable_now: build_output_reclaimable(&facts),
        grace_remaining_secs: match disposition {
            SeatWorktreeDisposition::WithinGrace { remaining_secs } => Some(remaining_secs),
            _ => None,
        },
        exists,
        tip_on_relay_known: session.tip_on_relay.is_some(),
    }
}

/// Split `<sessionRef>/<seatLabel>` back apart, falling back to the caller's
/// session ref when the key is malformed rather than inventing one.
fn split_key(key: &str, session_ref: &str) -> (String, String) {
    match key.split_once('/') {
        Some((session, seat)) => (seat.to_string(), session.to_string()),
        None => (key.to_string(), session_ref.to_string()),
    }
}

/// Every recorded worktree for the sessions the caller named.
///
/// Sessions with no recorded tree contribute nothing; a recorded tree whose
/// directory is gone still appears, with `exists: false`, because a record
/// that quietly disappeared is a thing an operator needs to see.
#[tauri::command]
pub async fn list_coding_session_seat_worktrees(
    app: AppHandle,
    sessions: Vec<CodingSessionWorktreeSessionFacts>,
) -> Result<Vec<CodingSessionSeatWorktreeRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = load_workdir_store(&app)?;
        let mut rows = Vec::new();
        for session in &sessions {
            let prefix = format!("{}/", session.session_ref.trim());
            for (key, entry) in &store.worktrees {
                if key.starts_with(&prefix) {
                    rows.push(build_row(key, entry, session));
                }
            }
        }
        Ok(rows)
    })
    .await
    .map_err(|error| format!("worktree listing task failed: {error}"))?
}

/// Remove one recorded seat worktree, on a person's explicit word.
///
/// The caller is the founder clicking a button that already named the path and
/// the file count. Nothing sweeps through here, and nothing passes `--force`.
#[tauri::command]
pub async fn prune_coding_session_seat_worktree(
    app: AppHandle,
    state: State<'_, AppState>,
    session_ref: String,
    seat_label: String,
) -> Result<String, String> {
    remove_recorded_seat_worktree(
        &app,
        &state,
        &session_ref,
        &seat_label,
        "removed on a person's own click, after the path and file count were shown",
    )
}

/// Remove `target/` and `desktop/node_modules` from one recorded worktree.
///
/// Independent of whether the tree is held: no commit can be lost in either
/// directory, and both are rebuildable from what is already committed. Every
/// source file is left byte-identical.
#[tauri::command]
pub async fn reclaim_coding_session_seat_worktree(
    app: AppHandle,
    session_ref: String,
    seat_label: String,
) -> Result<CodingSessionWorktreeReclaimed, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = load_workdir_store(&app)?;
        let key = seat_worktree_key(&session_ref, &seat_label);
        let Some(entry) = store.worktrees.get(&key) else {
            return Err(format!("this host has no worktree recorded for {key}"));
        };
        reclaim_build_output(&entry.path)
    })
    .await
    .map_err(|error| format!("worktree reclaim task failed: {error}"))?
}

/// Remove the rebuildable directories under one worktree path.
pub(crate) fn reclaim_build_output(path: &Path) -> Result<CodingSessionWorktreeReclaimed, String> {
    let before = reclaimable_bytes(path);
    let mut removed: Vec<String> = Vec::new();
    for name in RECLAIMABLE_BUILD_DIRS {
        let target: PathBuf = path.join(name);
        if !target.is_dir() {
            continue;
        }
        std::fs::remove_dir_all(&target)
            .map_err(|error| format!("failed to remove {}: {error}", target.display()))?;
        removed.push(target.to_string_lossy().into_owned());
    }
    Ok(CodingSessionWorktreeReclaimed {
        removed,
        freed_bytes: before,
        freed_label: render_reclaimable_bytes(before),
    })
}

#[cfg(test)]
#[path = "worktree_prune_tests.rs"]
mod tests;
