//! Gather the facts one seat worktree's disposition needs, and act on it.
//!
//! The decision itself is not here — it is
//! [`beekeeper_core::worktree_lifecycle::classify_seat_worktree`], pure and
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

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use beekeeper_core_pkg::sandbox_manifest::reclaim_plan_for;
use beekeeper_core_pkg::sandbox_seed::{
    estimate_reclaimable, reclaim, render_reclaim_estimate, ReclaimDisposition,
};
use beekeeper_core_pkg::sandbox_seed_fs::{NoGitAnswers, StdSeedOps};
use beekeeper_core_pkg::worktree_lifecycle::{
    build_output_reclaimable, classify_seat_worktree, render_reclaimable_bytes,
    SeatWorktreeDisposition, SeatWorktreeFacts,
};

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::{
    load_workdir_store, seat_worktree_key, CodingSessionSeatWorktree,
};
use crate::coding_sessions::worktree::remove_recorded_seat_worktree;

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
    /// An accepted whole-session deletion removed this session from the relay.
    ///
    /// A deletion is not a closure — it takes the closures with it, so nothing
    /// is left to fold — but it ends the work just as finally, and the trees
    /// this host cut for it must be disposed of under the same rules. Absent
    /// in an older caller's payload, which reads as `false`: this host does
    /// not guess that a session it was told nothing about was deleted.
    #[serde(default)]
    pub session_deleted: bool,
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
    /// Whether the caller named this session as deleted.
    pub session_deleted: bool,
    /// The provider session id recorded at cut time, or `null` for a tree cut
    /// before its execution had one. The join a surface uses to pair a
    /// running execution (its 44223 target) with its tree (spec § 4.9).
    pub session_id: Option<String>,
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
///
/// The status runs inside the tree's project boundary: it can run the
/// repository's own clean filters and fsmonitor.
pub(crate) fn count_dirty_files(path: &Path) -> u32 {
    match super::host_git::status(path, None) {
        Ok(output) => output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            .min(MAX_COUNTED_DIRTY_FILES as usize) as u32,
        Err(_) => 1,
    }
}

/// What reclaiming one worktree would free, and how a person should read it.
///
/// Which directories count is the project's to declare, in its own
/// `sandbox.yml`; the built-in list is the fallback for a project that declares
/// none. Reclaim asks git nothing, so this carries no git.
///
/// The label is not just the number formatted: a cloned build directory
/// measures its full logical size while freeing it releases only what this
/// sandbox itself wrote, so the number alone would overstate the gain.
pub(crate) fn reclaimable(path: &Path) -> (Option<u64>, String) {
    let (plan, _) = reclaim_plan_for(path);
    let estimate =
        estimate_reclaimable(&plan, path, &StdSeedOps::new(NoGitAnswers).measuring(true));
    (estimate.bytes, render_reclaim_estimate(estimate))
}

/// The one sentence a surface shows for a disposition.
///
/// Composed here so `bee` and the app cannot drift into two vocabularies for
/// the same tree.
pub(crate) fn worktree_detail(
    disposition: SeatWorktreeDisposition,
    path: &str,
    tip_on_relay_known: bool,
    session_deleted: bool,
) -> String {
    let sentence = disposition_sentence(disposition, path, tip_on_relay_known);
    if session_deleted {
        // A deleted session's tree must never read "the session is not
        // closed": that arm is unreachable once a deletion settles it, and the
        // sentence a reader gets has to say which of the two ends this was.
        // Ledger 135(f) is what saying the wrong one costs.
        return format!("{sentence} (the session was deleted, not closed)");
    }
    sentence
}

fn disposition_sentence(
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
        session_deleted: session.session_deleted,
        execution_live: session.execution_live,
        tip_on_relay: session.tip_on_relay.unwrap_or(false),
        dirty_files,
        recorded: true,
        is_protected,
        settled_for_secs: session.settled_for_secs,
    };
    let disposition = classify_seat_worktree(&facts);
    let (bytes, reclaimable_label) = if exists {
        reclaimable(&entry.path)
    } else {
        (Some(0), render_reclaimable_bytes(Some(0)))
    };
    let (seat_label, session_ref) = split_key(key, &session.session_ref);
    let detail = worktree_detail(
        disposition,
        &path_text,
        session.tip_on_relay.is_some(),
        session.session_deleted,
    );
    CodingSessionSeatWorktreeRow {
        key: key.to_string(),
        session_id: entry.session_id.clone(),
        session_ref,
        seat_label,
        detail,
        path: path_text,
        branch: entry.branch.clone(),
        repo_root: entry.repo_root.to_string_lossy().into_owned(),
        disposition: disposition.token().to_string(),
        dirty_files,
        reclaimable_bytes: bytes,
        reclaimable_label,
        reclaimable_now: build_output_reclaimable(&facts),
        grace_remaining_secs: match disposition {
            SeatWorktreeDisposition::WithinGrace { remaining_secs } => Some(remaining_secs),
            _ => None,
        },
        exists,
        tip_on_relay_known: session.tip_on_relay.is_some(),
        session_deleted: session.session_deleted,
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
    // Read before the removal: the record is forgotten with the tree, and it
    // is the only thing that names the provider session whose checkpoints
    // live in the repository's shared refs.
    let key = seat_worktree_key(&session_ref, &seat_label);
    let entry = load_workdir_store(&app)
        .ok()
        .and_then(|store| store.worktrees.get(&key).cloned());
    let removed = remove_recorded_seat_worktree(
        &app,
        &state,
        &session_ref,
        &seat_label,
        "removed on a person's own click, after the path and file count were shown",
    )?;
    if let Some(entry) = entry {
        let remaining = load_workdir_store(&app).map(|store| store.worktrees);
        match remaining {
            Ok(remaining) => retire_checkpoint_refs_after_prune(&entry, &remaining),
            Err(error) => tracing::warn!(
                target: "worktree",
                %error,
                "checkpoint refs left in place: the record could not be re-read after the prune"
            ),
        }
    }
    Ok(removed)
}

/// The namespace one provider session's turn checkpoints live under
/// (checkpoints brief, Host/provider step 2).
pub(crate) const CHECKPOINT_REF_ROOT: &str = "refs/beekeeper/checkpoints";

/// Whether `session_id` can name a checkpoint ref directory without any
/// reading of it being a guess: one path component of ASCII letters, digits,
/// `-`, `_` and `.`, with no `..`, no leading or trailing `.` and no `.lock`
/// suffix. This is exactly the rule the provider applies before writing one
/// (`buzz-session-provider` `turn_checkpoint_git::checkpoint_ref`), so every
/// directory it can write is one this can retire; anything else could never
/// have been written, so nothing is deleted for it.
fn is_checkpoint_session_component(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && !session_id.starts_with('.')
        && !session_id.ends_with('.')
        && !session_id.ends_with(".lock")
        && !session_id.contains("..")
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Which session's checkpoint refs a removed seat worktree takes with it.
///
/// `None` — leave every ref — when the record names no provider session
/// (a tree cut before its execution had one: the host does not guess which
/// session's refs were its own), when the id cannot be a ref component, or
/// when another surviving record still names the same session (a relocated
/// tree still diffs against them).
pub(crate) fn checkpoint_refs_to_retire<'a>(
    entry: &'a CodingSessionSeatWorktree,
    remaining: &std::collections::BTreeMap<String, CodingSessionSeatWorktree>,
) -> Option<&'a str> {
    let session_id = entry.session_id.as_deref()?.trim();
    if !is_checkpoint_session_component(session_id) {
        return None;
    }
    let still_named = remaining
        .values()
        .any(|other| other.session_id.as_deref().map(str::trim) == Some(session_id));
    (!still_named).then_some(session_id)
}

/// Delete `refs/beekeeper/checkpoints/<session_id>/…` from the repository
/// whose common directory `repo_root` names, and answer how many went.
///
/// Reads and deletes refs only — no repository code runs — so this is
/// ordinary host Git with hooks off. Another session's refs are outside the
/// prefix and are never listed.
///
/// # Errors
/// The id is not a ref component, or Git failed to list or delete.
pub(crate) fn delete_session_checkpoint_refs(
    repo_root: &Path,
    session_id: &str,
) -> Result<usize, String> {
    if !is_checkpoint_session_component(session_id) {
        return Err("the session id cannot name a checkpoint ref".to_string());
    }
    let auth = crate::commands::project_git_exec::build_local_git_auth_config()?;
    let prefix = format!("{CHECKPOINT_REF_ROOT}/{session_id}/");
    let listed = crate::commands::project_git_exec::run_git(
        &["for-each-ref", "--format=%(refname)", &prefix],
        Some(repo_root),
        &auth,
    )?;
    let mut deleted = 0;
    for name in listed.lines().map(str::trim) {
        if !name.starts_with(&prefix) {
            continue;
        }
        crate::commands::project_git_exec::run_git(
            &["update-ref", "-d", name],
            Some(repo_root),
            &auth,
        )?;
        deleted += 1;
    }
    Ok(deleted)
}

/// Retention (checkpoints brief, Host/provider step 6): once a seat's tree is
/// gone, its session's checkpoint refs go too. Never fails the prune that
/// already happened; a ref that would not go is logged, and a record with no
/// session id leaves every ref where it is.
pub(crate) fn retire_checkpoint_refs_after_prune(
    entry: &CodingSessionSeatWorktree,
    remaining: &std::collections::BTreeMap<String, CodingSessionSeatWorktree>,
) {
    let Some(session_id) = checkpoint_refs_to_retire(entry, remaining) else {
        tracing::info!(
            target: "worktree",
            "checkpoint refs left in place: the pruned record names no provider session this host may retire"
        );
        return;
    };
    if let Err(error) = delete_session_checkpoint_refs(&entry.repo_root, session_id) {
        tracing::warn!(
            target: "worktree",
            %error,
            "the pruned session's checkpoint refs could not be deleted"
        );
    }
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

/// Remove what this project calls build state under one worktree path.
///
/// One implementation, shared with `bee sessions worktree reclaim`, reading the
/// project's own `sandbox.yml` — so the two surfaces cannot free different sets
/// of directories. A declared path that turns out to be a link has the link
/// removed and whatever it points at left alone.
pub(crate) fn reclaim_build_output(path: &Path) -> Result<CodingSessionWorktreeReclaimed, String> {
    let (plan, refusal) = reclaim_plan_for(path);
    let receipt = reclaim(&plan, path, &StdSeedOps::new(NoGitAnswers).measuring(true));
    let refused: Vec<&str> = receipt
        .outcomes
        .iter()
        .filter(|outcome| matches!(outcome.disposition, ReclaimDisposition::Refused { .. }))
        .map(|outcome| outcome.detail.as_str())
        .collect();
    if !refused.is_empty() {
        return Err(refused.join("; "));
    }
    let removed = receipt
        .outcomes
        .iter()
        .filter(|outcome| {
            matches!(
                outcome.disposition,
                ReclaimDisposition::Removed | ReclaimDisposition::Unlinked
            )
        })
        .map(|outcome| path.join(&outcome.path).to_string_lossy().into_owned())
        .collect();
    let (freed, any_unmeasurable) = receipt.freed();
    // `at least`, not a flat number: what a clone releases is unknown, so a
    // total that silently counted its logical size would overstate the gain.
    let freed_label = if any_unmeasurable {
        format!(
            "at least {} — some of what was removed was cloned, so its exclusive share could \
             not be measured",
            render_reclaimable_bytes(Some(freed))
        )
    } else {
        render_reclaimable_bytes(Some(freed))
    };
    if let Some(refusal) = refusal {
        tracing::warn!(
            target: "beekeeper::sandbox",
            %refusal,
            "the project's sandbox.yml was not used; reclaimed the fallback list instead"
        );
    }
    Ok(CodingSessionWorktreeReclaimed {
        removed,
        freed_bytes: Some(freed),
        freed_label,
    })
}

#[cfg(test)]
#[path = "worktree_prune_tests.rs"]
mod tests;
