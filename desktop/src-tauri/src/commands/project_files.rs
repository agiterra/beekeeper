//! Reading one named file out of a project checkout, and refusing everything
//! else.
//!
//! # Why this exists
//!
//! The desktop routes coding-session hires against `team/model-registry.yaml`
//! — the file the team edits, versions and reviews. Until this command existed
//! the app had no way to read *any* project file: `scanProjectRolePacks` and
//! `pickCrewRolePacksDirectory` answer with a role, a name and a directory,
//! never with content. So the hire host refused every routed hire with
//! `HIRE_NO_ROUTE — registry not readable on this host`, and the Agents badge
//! said "Registry: unknown (not readable)". Both were honest and both were
//! dead ends.
//!
//! The alternative — compiling a registry copy into the app — would have been
//! worse than the gap: nobody could check it against the file on disk, and
//! every decision it produced would cite a version that was never there.
//!
//! # Why it is a gate and not a file API
//!
//! A checkout is a person's disk. A general "read a project file" command
//! would hand the webview `~/src/thing/.env` and every key in it. So this is
//! deliberately the narrowest thing that does the job:
//!
//! - **read-only** — nothing here creates, writes, moves or deletes;
//! - **allowlisted** — [`READABLE_PROJECT_FILES`] is the entire set of paths
//!   that can ever be named, and it holds one entry today;
//! - **shape-checked before the allowlist** — absolute paths and `..`
//!   segments are refused on their own terms, so the guard survives the day
//!   the allowlist grows;
//! - **containment-checked after resolution** — the file is canonicalized and
//!   must still live under the canonicalized checkout, which is what stops a
//!   symlink planted at an allowlisted path from reading anything it points
//!   at;
//! - **bounded** — [`MAX_PROJECT_FILE_BYTES`] caps what can cross the IPC
//!   boundary, and invalid UTF-8 is refused rather than replaced.
//!
//! Every refusal carries a stable `code` and a sentence naming the path
//! involved, because a caller told only "not readable" has nowhere to go.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use crate::coding_sessions::workdir_store::load_workdir_store;

/// The shared model registry's path inside a project **code** checkout.
///
/// One spelling for every reader:
/// `buzz_core::coding_session_routing::DEFAULT_REGISTRY_RELATIVE_PATH`, which
/// is also what `bee sessions route` walks up looking for.
pub const MODEL_REGISTRY_RELATIVE_PATH: &str =
    buzz_core_pkg::coding_session_routing::DEFAULT_REGISTRY_RELATIVE_PATH;

/// The shared model registry's path inside a project's **agents repository**
/// (spec § 4.11) — at the root, beside `team.yml`.
///
/// Allowlisted here because that is where a project's registry lives now: the
/// agents-repository seed writes it, and the code checkout rung is kept only
/// for Beekeeper's own repository. See [`super::model_registry`] for the
/// order the two are read in and why (ledger 178(a)).
pub const AGENTS_REPO_MODEL_REGISTRY_RELATIVE_PATH: &str =
    buzz_core_pkg::model_registry_source::AGENTS_REPO_REGISTRY_FILE;

/// Every relative path this command will ever read.
///
/// Extending this list is the only way to widen the command's reach, which is
/// the point: adding a path is a visible, reviewable edit rather than a
/// parameter a caller can choose.
pub const READABLE_PROJECT_FILES: &[&str] = &[
    MODEL_REGISTRY_RELATIVE_PATH,
    AGENTS_REPO_MODEL_REGISTRY_RELATIVE_PATH,
];

/// The largest project file that may cross the IPC boundary: 256 KiB.
///
/// The registry is a few hundred lines. The ceiling exists so that a file
/// swapped for a huge one cannot stall the webview, and it is inclusive — a
/// file of exactly this size is read.
pub const MAX_PROJECT_FILE_BYTES: u64 = 256 * 1024;

/// One allowlisted project file, and where it was really read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileRead {
    /// The absolute, symlink-resolved path the bytes came from.
    pub path: String,
    /// The file's contents, as UTF-8.
    pub text: String,
}

/// Why a read was refused.
///
/// `code` is stable and machine-readable; `message` is the sentence a person
/// reads, and it always names the path or the limit involved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileRefusal {
    pub code: &'static str,
    pub message: String,
}

impl ProjectFileRefusal {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// The allowlist, rendered for a refusal sentence.
fn allowlist_sentence() -> String {
    READABLE_PROJECT_FILES.join(", ")
}

/// Is every component of `relative_path` an ordinary name?
///
/// Rejects absolute paths, drive prefixes, `.` and `..` in one pass, so a new
/// allowlist entry cannot accidentally admit a traversal.
fn is_plain_relative_path(relative_path: &Path) -> Result<(), ProjectFileRefusal> {
    if relative_path.as_os_str().is_empty() {
        return Err(ProjectFileRefusal::new(
            "path-not-relative",
            "no project file was named",
        ));
    }
    for component in relative_path.components() {
        match component {
            Component::Normal(_) => {}
            Component::ParentDir => {
                return Err(ProjectFileRefusal::new(
                    "path-escapes-checkout",
                    format!(
                        "{} contains a `..` segment; project files are read by their exact relative path",
                        relative_path.display()
                    ),
                ))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ProjectFileRefusal::new(
                    "path-not-relative",
                    format!(
                        "{} is an absolute path; only paths relative to the project checkout are read",
                        relative_path.display()
                    ),
                ))
            }
            Component::CurDir => {
                return Err(ProjectFileRefusal::new(
                    "path-escapes-checkout",
                    format!(
                        "{} contains a `.` segment; project files are read by their exact relative path",
                        relative_path.display()
                    ),
                ))
            }
        }
    }
    Ok(())
}

/// Read one allowlisted file from `checkout_root`, or say why not.
///
/// This is the whole policy, and it takes no `AppHandle` so every refusal is
/// testable without an app.
///
/// # Errors
///
/// Returns a [`ProjectFileRefusal`] when the path is not relative, contains a
/// `..` or `.` segment, is not on [`READABLE_PROJECT_FILES`], resolves outside
/// the checkout, is missing, is not a regular file, exceeds
/// [`MAX_PROJECT_FILE_BYTES`], or does not hold UTF-8.
pub fn read_allowlisted_project_file(
    checkout_root: &Path,
    relative_path: &str,
) -> Result<ProjectFileRead, ProjectFileRefusal> {
    let trimmed = relative_path.trim();
    let requested = Path::new(trimmed);
    is_plain_relative_path(requested)?;

    if !READABLE_PROJECT_FILES.contains(&trimmed) {
        return Err(ProjectFileRefusal::new(
            "path-not-allowlisted",
            format!(
                "{trimmed} is not a project file this app reads; it reads only {}",
                allowlist_sentence()
            ),
        ));
    }

    let root = checkout_root.canonicalize().map_err(|error| {
        ProjectFileRefusal::new(
            "checkout-missing",
            format!(
                "the project checkout {} cannot be opened on this computer: {error}",
                checkout_root.display()
            ),
        )
    })?;

    let candidate = root.join(requested);
    let resolved = candidate.canonicalize().map_err(|error| {
        ProjectFileRefusal::new(
            "file-missing",
            format!("{} cannot be opened: {error}", candidate.display()),
        )
    })?;
    if !resolved.starts_with(&root) {
        return Err(ProjectFileRefusal::new(
            "outside-checkout",
            format!(
                "{} resolves to {}, which is outside the project checkout {}",
                candidate.display(),
                resolved.display(),
                root.display()
            ),
        ));
    }

    let metadata = std::fs::metadata(&resolved).map_err(|error| {
        ProjectFileRefusal::new(
            "file-missing",
            format!("{} cannot be inspected: {error}", resolved.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(ProjectFileRefusal::new(
            "not-a-file",
            format!("{} is not a regular file", resolved.display()),
        ));
    }
    if metadata.len() > MAX_PROJECT_FILE_BYTES {
        return Err(ProjectFileRefusal::new(
            "too-large",
            format!(
                "{} is {} bytes; this app reads project files up to {MAX_PROJECT_FILE_BYTES} bytes",
                resolved.display(),
                metadata.len()
            ),
        ));
    }

    let bytes = std::fs::read(&resolved).map_err(|error| {
        ProjectFileRefusal::new(
            "unreadable",
            format!("{} could not be read: {error}", resolved.display()),
        )
    })?;
    let text = String::from_utf8(bytes).map_err(|_| {
        ProjectFileRefusal::new(
            "not-utf8",
            format!("{} is not valid UTF-8 text", resolved.display()),
        )
    })?;

    Ok(ProjectFileRead {
        path: resolved.to_string_lossy().to_string(),
        text,
    })
}

/// The checkout this computer has recorded for a project coordinate.
///
/// The same record the Agents tab resolves its project from
/// (`CodingSessionWorkdirStore::by_project`, keyed by the NIP-MP coordinate
/// `30621:<owner>:<dtag>`). A project with no recorded checkout has no
/// directory to read, and says so rather than guessing at one.
fn recorded_checkout(app: &AppHandle, project_ref: &str) -> Result<PathBuf, ProjectFileRefusal> {
    let store = load_workdir_store(app).map_err(|error| {
        ProjectFileRefusal::new(
            "no-checkout-recorded",
            format!("this computer's project directory record could not be read: {error}"),
        )
    })?;
    store
        .by_project
        .get(project_ref)
        .map(|entry| entry.path.clone())
        .ok_or_else(|| {
            ProjectFileRefusal::new(
                "no-checkout-recorded",
                format!(
                    "this computer has no checkout directory recorded for project {project_ref}; \
                     open the project and choose one"
                ),
            )
        })
}

/// Read one allowlisted file out of a project's checkout on this computer.
///
/// Read-only. See the module docs for the full refusal list.
///
/// # Errors
///
/// Returns a [`ProjectFileRefusal`] naming the path involved — including when
/// no checkout directory has been recorded for `project_ref`.
#[tauri::command]
pub fn read_project_file(
    app: AppHandle,
    project_ref: String,
    relative_path: String,
) -> Result<ProjectFileRead, ProjectFileRefusal> {
    let project_ref = project_ref.trim().to_string();
    if project_ref.is_empty() {
        return Err(ProjectFileRefusal::new(
            "no-checkout-recorded",
            "no project was named, so there is no checkout to read from",
        ));
    }
    let root = recorded_checkout(&app, &project_ref)?;
    read_allowlisted_project_file(&root, &relative_path)
}

#[cfg(test)]
#[path = "project_files_tests.rs"]
mod tests;
