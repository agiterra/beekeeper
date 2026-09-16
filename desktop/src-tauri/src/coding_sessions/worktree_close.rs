//! What the host does with a seat's worktree when its session closes.
//!
//! # The gap this closes (SESSION_STATE §3 item 9)
//!
//! `bee sessions worktree prune` and the closure dialog's own button both
//! exist, and both need somebody to run them. Nothing ran when a hired seat's
//! session closed, so every tree a hire cut stayed on disk with its `target/`
//! intact — "the thing that put the disk at 100% during this batch". This
//! module is the host doing it by itself, at the one moment it is certain the
//! work is finished.
//!
//! # The predicate is not re-decided here
//!
//! The disposition comes from [`classify_seat_worktree`], the same pure
//! function `bee sessions worktree prune` and the app's own rows use, and the
//! sentence comes from [`worktree_detail`]. A close-time sweep with its own
//! private notion of "finished" is exactly how a person's uncommitted work
//! gets deleted by something they never told to run.
//!
//! What this module contributes is the one fact the shared predicate cannot
//! establish for itself here: **is the branch on the relay right now**. It
//! asks the git transport directly — `git ls-remote --heads <remote>` from
//! inside the tree, with the NIP-98 credential helper the founder already has
//! configured — rather than folding kind 30618 as `bee` does. Same question,
//! answered by the party that would serve a fetch, and answerable from a host
//! that holds no relay subscription of its own. Like 30618 it says where refs
//! stand **now**; neither is a push history.
//!
//! # Three removals, three rules
//!
//! * **The tree** goes only when the shared predicate answers `prunable`,
//!   which includes the seven-day grace window a clean, pushed tree gets from
//!   its closure. A close is not an override of that window, and this module
//!   deliberately does not have one.
//! * **The build output** (`target/`, `desktop/node_modules`) goes as soon as
//!   the session is settled and no execution is live, because
//!   [`build_output_reclaimable`] needs no grace at all — no commit can be
//!   lost in either directory. That is the part that answers the disk, and it
//!   is why a close that keeps the tree still frees the gigabytes.
//! * **The seat's skill bundle** (`<app data dir>/agents/seats/<session id>`)
//!   goes with the tree, not with the build output: it is what the seat's
//!   execution is running on, so a held tree — one somebody may still come
//!   back to — keeps its skills. See [`super::seat_bundle`], which owns every
//!   rule about which directory may be removed.
//!
//! Everything else is a refusal carrying the sentence for its disposition:
//! `held: {N} uncommitted files` for dirty, and the tip-not-on-relay sentence
//! for unpushed.
//!
//! # A deletion is an ending too
//!
//! `session_deleted` reaches this path for the same reason `session_settled`
//! does: an accepted whole-session deletion ends the work, but it takes the
//! 44230 closures with it, so nothing is left for a later reader to fold.
//! Ledger 135(f) — after `bee sessions delete` on `cc5cb114` every seat tree
//! still answered "the session is not closed, so nothing is removed", nothing
//! here ever ran, and the trees and the bundles were removed by hand. The
//! deletion buys no exemption from anything: the shared predicate still holds
//! a dirty tree, still refuses an unpushed one, and still runs the grace
//! window — from the deletion, because that is when the work stopped.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use buzz_core_pkg::worktree_lifecycle::{
    build_output_reclaimable, classify_seat_worktree, SeatWorktreeDisposition, SeatWorktreeFacts,
};

use crate::app_state::AppState;
use crate::coding_sessions::seat_bundle::{remove_seat_bundle, SeatBundleRemoval};
use crate::coding_sessions::workdir_store::{load_workdir_store, seat_worktree_key};
use crate::coding_sessions::worktree::remove_recorded_seat_worktree;
use crate::coding_sessions::worktree_prune::{
    count_dirty_files, is_protected_worktree, reclaim_build_output, worktree_detail,
};
use crate::commands::project_git_exec::{
    build_git_auth_config, build_local_git_auth_config, run_git, GitAuthConfig,
};

/// What the host did — or refused to do — with one seat's tree on close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionWorktreeCloseOutcome {
    /// `<sessionRef>/<seatLabel>` — the record's own key.
    pub key: String,
    /// Absolute path of the tree this outcome is about.
    pub path: String,
    /// The shared predicate's stable token for it.
    pub disposition: String,
    /// Whether the directory was removed.
    pub pruned: bool,
    /// Bytes of build output removed, when they could be measured first.
    pub reclaimed_bytes: Option<u64>,
    /// Whether the branch's presence on the relay was established at all.
    ///
    /// `false` never renders as "not pushed": a host that could not ask is not
    /// a host that got a no.
    pub tip_on_relay_known: bool,
    /// The one sentence explaining this outcome. Always present, for a prune
    /// as much as for a refusal.
    pub detail: String,
    /// What happened to the seat's skill bundle, always disclosed.
    ///
    /// Never inferred from `pruned`: a removed tree whose record carried no
    /// session id names no bundle, and that is its own answer rather than a
    /// silent success.
    pub bundle: SeatBundleRemoval,
}

/// Whether the tree's `HEAD` is reachable from a ref the remote holds now.
///
/// `None` — never `Some(false)` — when the question could not be asked at all:
/// no remote, a transport that failed, a tree git cannot read. The caller
/// passes that through as "could not confirm", which the shared predicate
/// treats exactly as conservatively as a no while the *sentence* says
/// something different and true.
///
/// The remote name is read from `git remote`, never hard-coded: two pre-push
/// guards hard-coded one and both broke silently the day the names moved.
///
/// `auth` carries the identity the NIP-98 credential helper signs with, which
/// is why the caller passes the app's own config rather than the local-only
/// one: `ls-remote` against `hive.agiterra.org` is authenticated, and a
/// helperless invocation would sit on a username prompt it can never answer.
pub(crate) fn tip_on_remote(path: &Path, auth: &GitAuthConfig) -> Option<bool> {
    let remotes = run_git(&["remote"], Some(path), auth).ok()?;
    let remote = remotes
        .lines()
        .map(str::trim)
        .find(|line| *line == "origin")
        .or_else(|| remotes.lines().map(str::trim).find(|line| !line.is_empty()))?;
    let listing = run_git(&["ls-remote", "--heads", remote], Some(path), auth).ok()?;
    let heads: Vec<String> = listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_ascii_lowercase)
        .filter(|oid| oid.len() == 40 && oid.chars().all(|c| c.is_ascii_hexdigit()))
        .collect();
    if heads.is_empty() {
        // A remote that holds no branches answers the question: nothing there
        // holds this work. That is a real `false`, not an unasked question.
        return Some(false);
    }
    let tip = run_git(&["rev-parse", "HEAD"], Some(path), auth)
        .ok()?
        .trim()
        .to_ascii_lowercase();
    if heads.contains(&tip) {
        return Some(true);
    }
    // Merged-and-clean is the other admissible shape: the seat's commits are
    // an ancestor of something the remote holds, which is what a landed branch
    // looks like after its own ref is deleted. An oid this checkout does not
    // have cannot be an ancestor test, and `--is-ancestor` answering non-zero
    // for that reason is indistinguishable from a real "no" — which is why
    // this is an `any`, not an `all`.
    Some(heads.iter().any(|oid| {
        run_git(
            &["merge-base", "--is-ancestor", &tip, oid],
            Some(path),
            auth,
        )
        .is_ok()
    }))
}

/// Decide and act for one recorded tree whose session has settled.
///
/// `execution_live` and `settled_for_secs` are the caller's facts, for the
/// same reason [`crate::coding_sessions::worktree_prune`] takes them: the
/// surfaces that call in already hold the 44230 fold, and re-deriving it here
/// would give the app two projections of one fact that could disagree.
#[tauri::command]
pub async fn close_coding_session_seat_worktree(
    app: AppHandle,
    state: State<'_, AppState>,
    session_ref: String,
    seat_label: String,
    execution_live: bool,
    settled_for_secs: Option<u64>,
    session_deleted: Option<bool>,
) -> Result<CodingSessionWorktreeCloseOutcome, String> {
    // Absent means "the caller did not say", which is `false`: this host never
    // infers that a session it was told nothing about was deleted.
    let session_deleted = session_deleted.unwrap_or(false);
    let key = seat_worktree_key(&session_ref, &seat_label);
    let entry = {
        let store = load_workdir_store(&app)?;
        store
            .worktrees
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("this host has no worktree recorded for {key}"))?
    };

    // The app's own git identity, so `ls-remote` can be authenticated by the
    // NIP-98 credential helper. Falls back to the local-only config when this
    // install holds no signing key: the round trip then answers only for a
    // remote that needs none, and "could not confirm" for every other, which
    // is the conservative outcome.
    let auth = build_git_auth_config(&state).or_else(|_| build_local_git_auth_config())?;
    let path_text = entry.path.to_string_lossy().into_owned();
    let decided = close_disposition(
        &entry.repo_root,
        &entry.path,
        execution_live,
        settled_for_secs,
        session_deleted,
        &auth,
    );
    let CloseDecision {
        disposition,
        tip,
        detail,
        facts,
    } = decided;

    // Build output first, and independently of the tree's own disposition: it
    // is rebuildable, it holds no commit, and it is the whole of the disk
    // problem. A tree kept for its grace window still gives its gigabytes back.
    let reclaimed_bytes = if entry.path.is_dir() && build_output_reclaimable(&facts) {
        reclaim_build_output(&entry.path)
            .ok()
            .and_then(|reclaimed| reclaimed.freed_bytes)
    } else {
        None
    };

    if !disposition.is_host_prunable() {
        return Ok(CodingSessionWorktreeCloseOutcome {
            key,
            path: path_text,
            disposition: disposition.token().to_string(),
            pruned: false,
            reclaimed_bytes,
            tip_on_relay_known: tip.is_some(),
            detail,
            bundle: SeatBundleRemoval::kept_with_tree(),
        });
    }

    // `git worktree remove` runs without `--force` inside here, so git's own
    // refusal is still the last guard under everything decided above.
    remove_recorded_seat_worktree(&app, &state, &session_ref, &seat_label, &detail)?;

    // Only now, and only for a tree this host actually removed. The bundle is
    // located from the session id on the host's own record: a record without
    // one names no bundle, which `remove_seat_bundle` discloses rather than
    // guessing at a directory. A failure to remove it is reported, never
    // promoted to a failure of the close — the tree is already gone, and
    // saying the close failed would be the less true of the two answers.
    let bundle = match app.path().app_data_dir() {
        Ok(app_data_dir) => {
            remove_seat_bundle(&app_data_dir, entry.session_id.as_deref(), execution_live)
        }
        Err(error) => SeatBundleRemoval::unresolved_app_data(&error.to_string()),
    };

    Ok(CodingSessionWorktreeCloseOutcome {
        key,
        path: path_text,
        disposition: disposition.token().to_string(),
        pruned: true,
        reclaimed_bytes,
        tip_on_relay_known: tip.is_some(),
        detail,
        bundle,
    })
}

/// Everything the close path decided about one tree, before it acted.
pub(crate) struct CloseDecision {
    /// What the shared predicate answered.
    pub disposition: SeatWorktreeDisposition,
    /// Whether the branch's relay presence was established, and its answer.
    pub tip: Option<bool>,
    /// The one sentence for that disposition.
    pub detail: String,
    /// The facts it was decided from, so the caller can ask
    /// [`build_output_reclaimable`] the same question without rebuilding them.
    pub facts: SeatWorktreeFacts,
}

/// Decide one tree's disposition the way the close path decides it.
///
/// Split from the command so it can be exercised against real repositories in
/// a `TempDir` with no Tauri `AppHandle`. The command is this, plus the record
/// lookup and the two removals.
pub(crate) fn close_disposition(
    repo_root: &Path,
    path: &Path,
    execution_live: bool,
    settled_for_secs: Option<u64>,
    session_deleted: bool,
    auth: &GitAuthConfig,
) -> CloseDecision {
    let exists = path.is_dir();
    let is_protected = is_protected_worktree(repo_root, path);
    let dirty_files = if exists && !is_protected {
        count_dirty_files(path)
    } else {
        0
    };
    // Not asked when the tree is gone, protected or dirty: each of those
    // decides the disposition on its own, and `ls-remote` is a transport round
    // trip whose answer nothing would read.
    let tip = if exists && !is_protected && dirty_files == 0 {
        tip_on_remote(path, auth)
    } else {
        None
    };
    let facts = SeatWorktreeFacts {
        // Only ever called for a session the caller has already folded to
        // closed or archived — or one an accepted deletion has ended. That is
        // this path's whole precondition, and `session_deleted` says which of
        // the two it was so the sentence can too.
        session_settled: true,
        session_deleted,
        execution_live,
        tip_on_relay: tip.unwrap_or(false),
        dirty_files,
        recorded: true,
        is_protected,
        settled_for_secs,
    };
    let disposition = classify_seat_worktree(&facts);
    let detail = worktree_detail(
        disposition,
        &path.to_string_lossy(),
        tip.is_some(),
        session_deleted,
    );
    CloseDecision {
        disposition,
        tip,
        detail,
        facts,
    }
}

#[cfg(test)]
#[path = "worktree_close_tests.rs"]
mod tests;
