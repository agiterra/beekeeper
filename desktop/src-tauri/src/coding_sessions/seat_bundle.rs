//! Remove a seat's skill bundle once its session is done with it.
//!
//! A seat's role skills are materialized outside every checkout, at
//! `<app data dir>/agents/seats/<session id>/`, so a seat run never leaves an
//! untracked directory in the tree it is working in. Nothing removed them, so
//! they accumulated the way worktrees did before the close-time reaper.
//!
//! The rules here are the ones [`super::worktree_prune`] already runs on
//! directories, applied to bundles:
//!
//! * **The host removes only what it can name from its own records.** A bundle
//!   is located from the session id on this host's own seat-worktree record.
//!   When that record carries no session id, the bundle is not guessed at — the
//!   outcome says the host could not name one.
//! * **A live session keeps its bundle.** The skills in it are what the
//!   execution is running on; a reattach resolves the same path.
//! * **Never a path outside `agents/seats/`.** The directory name is one
//!   component, and the resolved path is checked against the root it must sit
//!   under — after canonicalization when it exists, so a symlinked bundle
//!   cannot walk out.
//! * **Disclose rather than guess.** Absent, unreadable and refused are each
//!   their own answer with its own sentence; none of them reads as "removed".

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::coding_sessions::workdir_store::load_workdir_store;

/// The directory layout and the session-id sanitizer, shared with the provider
/// that creates these bundles.
///
/// This host locates a bundle by **recomputing** its name, so these must be
/// the same definitions the creating side used. They are the same ones now,
/// rather than a copy kept in step by comment.
pub(crate) use buzz_core_pkg::coding_session_seat_bundle::{
    bundle_directory_name, seat_bundle_dir_in as seat_bundle_dir,
    seat_bundles_root_in as seat_bundles_root, SEAT_BUNDLE_SKILLS_DIR,
};
/// The manifest one bundle carries, naming the pack it was built from.
///
/// Owned by the crate that writes it. Used here only to recognise a directory
/// as a bundle, never to read one.
pub(crate) use buzz_persona_pkg::skills::SKILL_BUNDLE_MANIFEST_FILE as SEAT_BUNDLE_MANIFEST;

/// What happened to one seat's bundle, and the sentence that says so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SeatBundleRemoval {
    /// Whether a directory was actually removed. Only ever true for `removed`.
    pub removed: bool,
    /// The bundle path this outcome is about, when the host could name one.
    pub path: Option<String>,
    /// Stable token: `removed`, `absent`, `unnamed_session`, `session_live`,
    /// `outside_root`, or `failed`.
    pub token: String,
    /// The one sentence explaining this outcome.
    pub detail: String,
}

impl SeatBundleRemoval {
    /// The tree was kept, so its skills were kept with it.
    ///
    /// A held tree is one somebody may still come back to; taking its skills
    /// away would leave a directory that no longer runs the way it did.
    pub(crate) fn kept_with_tree() -> Self {
        Self {
            removed: false,
            path: None,
            token: "tree_held".to_owned(),
            detail: "The worktree was kept, so the seat's skills were kept with it.".to_owned(),
        }
    }

    /// This host could not resolve its own app data directory.
    ///
    /// Its own answer rather than a failure: the tree really was removed, and
    /// the bundle really was not, so both need saying.
    pub(crate) fn unresolved_app_data(error: &str) -> Self {
        Self {
            removed: false,
            path: None,
            token: "unresolved_app_data".to_owned(),
            detail: format!(
                "This host could not resolve its app data directory ({error}), so it \
                 could not find the seat's skills to remove them."
            ),
        }
    }

    fn new(token: &str, detail: String, path: Option<&Path>, removed: bool) -> Self {
        Self {
            removed,
            path: path.map(|p| p.to_string_lossy().into_owned()),
            token: token.to_owned(),
            detail,
        }
    }
}

/// Remove the bundle belonging to one closed seat.
///
/// `session_id` is the producer-minted id from this host's own seat-worktree
/// record; `None` is the ordinary case for a tree cut before its genesis was
/// signed, and it is disclosed rather than guessed at. `execution_live` is the
/// same flag the close path decided the worktree's disposition from.
pub(crate) fn remove_seat_bundle(
    app_data_dir: &Path,
    session_id: Option<&str>,
    execution_live: bool,
) -> SeatBundleRemoval {
    let Some(session_id) = session_id.filter(|id| !id.trim().is_empty()) else {
        return SeatBundleRemoval::new(
            "unnamed_session",
            "This host recorded no session id for the seat, so it cannot name a \
             bundle to remove; nothing was searched for and nothing was removed."
                .to_owned(),
            None,
            false,
        );
    };

    let dir = seat_bundle_dir(app_data_dir, session_id);

    if execution_live {
        return SeatBundleRemoval::new(
            "session_live",
            "The session is still running, so its skills were left in place.".to_owned(),
            Some(&dir),
            false,
        );
    }

    let root = seat_bundles_root(app_data_dir);
    if let Err(reason) = bundle_is_inside_root(&root, &dir) {
        return SeatBundleRemoval::new("outside_root", reason, Some(&dir), false);
    }

    if !dir.exists() {
        return SeatBundleRemoval::new(
            "absent",
            "No bundle directory was there to remove.".to_owned(),
            Some(&dir),
            false,
        );
    }

    match std::fs::remove_dir_all(&dir) {
        Ok(()) => SeatBundleRemoval::new(
            "removed",
            "The seat's skill bundle was removed; it holds no commit and is \
             rebuilt from the pack on the next run."
                .to_owned(),
            Some(&dir),
            true,
        ),
        Err(error) => SeatBundleRemoval::new(
            "failed",
            format!("The seat's skill bundle could not be removed: {error}."),
            Some(&dir),
            false,
        ),
    }
}

/// Refuse any resolved bundle path that is not one component under the root.
///
/// Two checks, because they answer different questions. The lexical one
/// rejects a name that climbed out (`..`, an absolute component, a separator).
/// The canonical one rejects a directory that *is* inside the root by name but
/// resolves elsewhere — a symlinked bundle. A path that does not exist yet can
/// only be checked lexically, which is enough, because nothing is removed at a
/// path that does not exist.
fn bundle_is_inside_root(root: &Path, dir: &Path) -> Result<(), String> {
    if dir.parent() != Some(root) {
        return Err(format!(
            "The resolved bundle path is not directly under {}, so nothing was removed.",
            root.display()
        ));
    }
    let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
        return Err(
            "The resolved bundle path has no ordinary directory name, so nothing was removed."
                .to_owned(),
        );
    };
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(
            "The resolved bundle name is not a single directory component, so nothing was removed."
                .to_owned(),
        );
    }
    if dir.exists() {
        let (Ok(real_root), Ok(real_dir)) = (root.canonicalize(), dir.canonicalize()) else {
            return Err(
                "The bundle path could not be resolved on disk, so nothing was removed.".to_owned(),
            );
        };
        if !real_dir.starts_with(&real_root) {
            return Err(format!(
                "The bundle path resolves to {}, outside {}, so nothing was removed.",
                real_dir.display(),
                real_root.display()
            ));
        }
    }
    Ok(())
}

/// One bundle on disk that no live record accounts for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrphanSeatBundle {
    /// Absolute path of the bundle directory.
    pub path: String,
    /// The directory's own name — the sanitized session id.
    pub name: String,
    /// Whether it looks like a bundle this provider wrote (`skills/` or
    /// `manifest.json` inside). A directory that looks like neither is still
    /// listed, so a person sees everything under the root, but it is marked.
    pub looks_like_bundle: bool,
}

/// List every bundle under the root that no recorded session accounts for.
///
/// Listing is the whole of this function: nothing is removed here, the same
/// way an unrecorded worktree is listed and counted rather than adopted. The
/// caller shows the list, and only then may remove from it.
pub(crate) fn list_orphan_seat_bundles(
    app_data_dir: &Path,
    recorded_session_ids: &BTreeSet<String>,
) -> Vec<OrphanSeatBundle> {
    let root = seat_bundles_root(app_data_dir);
    let known: BTreeSet<String> = recorded_session_ids
        .iter()
        .map(|id| bundle_directory_name(id))
        .collect();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut orphans: Vec<OrphanSeatBundle> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if known.contains(&name) {
                return None;
            }
            let path = entry.path();
            let looks_like_bundle = path.join(SEAT_BUNDLE_SKILLS_DIR).is_dir()
                || path.join(SEAT_BUNDLE_MANIFEST).is_file();
            Some(OrphanSeatBundle {
                path: path.to_string_lossy().into_owned(),
                name,
                looks_like_bundle,
            })
        })
        .collect();
    orphans.sort_by(|a, b| a.name.cmp(&b.name));
    orphans
}

/// Remove one orphan bundle by its directory name, after it was listed.
///
/// Takes the name rather than a path so a caller cannot hand in somewhere
/// else: the path is rebuilt here, under this host's own root, and checked
/// again before anything is removed.
pub(crate) fn remove_orphan_seat_bundle(app_data_dir: &Path, name: &str) -> SeatBundleRemoval {
    let root = seat_bundles_root(app_data_dir);
    let dir = root.join(name);
    if let Err(reason) = bundle_is_inside_root(&root, &dir) {
        return SeatBundleRemoval::new("outside_root", reason, Some(&dir), false);
    }
    if !dir.exists() {
        return SeatBundleRemoval::new(
            "absent",
            "No bundle directory was there to remove.".to_owned(),
            Some(&dir),
            false,
        );
    }
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => SeatBundleRemoval::new(
            "removed",
            "The orphan skill bundle was removed.".to_owned(),
            Some(&dir),
            true,
        ),
        Err(error) => SeatBundleRemoval::new(
            "failed",
            format!("The orphan skill bundle could not be removed: {error}."),
            Some(&dir),
            false,
        ),
    }
}

/// Every session id this host still has a seat-worktree record for.
fn recorded_session_ids(app: &AppHandle) -> Result<BTreeSet<String>, String> {
    let store = load_workdir_store(app)?;
    Ok(store
        .worktrees
        .values()
        .filter_map(|entry| entry.session_id.clone())
        .collect())
}

/// List seat bundles this host has no session record for.
///
/// Listing only. This is the counterpart of listing an unrecorded worktree:
/// the host shows what it found and how much of it there is, and a person
/// decides. Nothing is removed by asking.
#[tauri::command]
pub async fn coding_session_list_orphan_seat_bundles(
    app: AppHandle,
) -> Result<Vec<OrphanSeatBundle>, String> {
    let recorded = recorded_session_ids(&app)?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || list_orphan_seat_bundles(&app_data_dir, &recorded))
        .await
        .map_err(|error| format!("seat bundle listing task failed: {error}"))
}

/// Remove one orphan seat bundle by the name a listing gave.
///
/// Re-checks that the name still has no session record, so a bundle whose
/// session was recorded between the listing and the click is refused rather
/// than removed from under it.
#[tauri::command]
pub async fn coding_session_remove_orphan_seat_bundle(
    app: AppHandle,
    name: String,
) -> Result<SeatBundleRemoval, String> {
    let recorded = recorded_session_ids(&app)?;
    if recorded.iter().any(|id| bundle_directory_name(id) == name) {
        return Ok(SeatBundleRemoval::new(
            "session_recorded",
            "This host now has a session record for that bundle, so it is no longer \
             an orphan and was left alone."
                .to_owned(),
            None,
            false,
        ));
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || remove_orphan_seat_bundle(&app_data_dir, &name))
        .await
        .map_err(|error| format!("seat bundle removal task failed: {error}"))
}

#[cfg(test)]
#[path = "seat_bundle_tests.rs"]
mod tests;
