//! Where a coding session's working tree is on **this machine**, without
//! ever telling the renderer the path.
//!
//! The session view's Files surface (SV-24) and its terminal drawer (SV-25,
//! DB9) both need "the tree the agent works in". That is a host-local fact:
//! the workdir store files it under the session id a worktree was cut for,
//! with the project's and the channel's remembered directories as defaults
//! (`workdir_store.rs`). A path names one person's disk, so it stays on this
//! side of the IPC boundary: the renderer gets a resolution — available or
//! not, which record answered, a label and a reason — and relative entries
//! under the root. Never the root itself.
//!
//! # Lookup order
//!
//! 1. The worktree recorded under the focused execution's session id. If this
//!    host cut the tree, this host has it, whoever's provider runs the agent.
//! 2. The project's remembered checkout, then the channel's remembered
//!    folder — **only when this machine's provider runs the execution, and
//!    never for a hired seat**. A teammate who happens to have the same
//!    project checked out must not see their own checkout labelled as this
//!    session's tree; and a hired seat works only in the worktree cut for it,
//!    so for a hire the project's checkout is "exactly the wrong answer"
//!    (`hired_seat_cwd_refusal`, `SEAT_CWD_PROJECT_ROOT`, in
//!    `crates/beekeeper-session-provider/src/commands.rs`).
//!
//! The project-before-channel order is the provider's own
//! (`ProjectsFile::resolve_default` in
//! `crates/beekeeper-session-provider/src/commands.rs`): a default the provider
//! would not have chosen is not where the agent works. DB9 in
//! `plans/SESSION_VIEW_PARITY_WAVE_B.md` (agents repository) was amended on
//! 2026-10-04 to this order, so plan and code agree; a test pins it. The
//! labels say which record answered, so a default is never called the
//! session's own tree.
//!
//! # What a listing refuses
//!
//! An absolute path, any `..`, a `.git` component, anything that
//! canonicalizes outside the root (a symlink out), and anything that
//! canonicalizes *into* a `.git` directory (a symlink such as `gitdir -> .git`).
//! Entries are relative, `.git` is hidden, a listing stops at
//! [`MAX_TREE_ENTRIES`] and says so, and entries it could not represent (a
//! name that is not UTF-8, a type it could not read) are counted in
//! `omitted` rather than silently dropped.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::workdir_store::{load_workdir_store_readonly, CodingSessionWorkdirStore};

/// The most entries one listing returns. Past it the listing says
/// `truncated: true` rather than pretending the directory ends there.
pub(crate) const MAX_TREE_ENTRIES: usize = 2_000;

/// Which session the caller means. Every field is a relay-visible reference
/// or a local fact the renderer already holds; none of them is a path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionTreeQuery {
    /// The provider-minted session id of the focused execution
    /// (`commandTarget.sessionId`), when known.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The session's channel id.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// The NIP-MP project coordinate the session's create named, when any.
    #[serde(default)]
    pub project_ref: Option<String>,
    /// Whether this machine's provider runs the focused execution. Gates the
    /// project and channel defaults (DB9).
    #[serde(default)]
    pub is_local_provider: bool,
    /// An agent is seated on the focused execution (a hire). Refuses the
    /// project and channel defaults: a hire works only in its own worktree.
    #[serde(default)]
    pub is_hired_seat: bool,
}

/// Which record a resolved tree came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CodingSessionTreeSource {
    /// A worktree this host cut, filed under the session id.
    Session,
    /// The project's remembered checkout on this machine.
    Project,
    /// The channel's remembered folder on this machine.
    Channel,
}

/// Why no tree resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CodingSessionTreeRefusal {
    /// Another machine's provider runs the session and this host cut no tree
    /// for it.
    NotLocal,
    /// This machine runs it, but no record names a directory that exists.
    NotRecorded,
    /// The host-local store could not be read.
    StoreUnreadable,
}

/// The renderer's view of a tree: never a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionTreeResolution {
    pub available: bool,
    pub source: Option<CodingSessionTreeSource>,
    /// What the tree is, in words: "this session's worktree", "this
    /// project's checkout", "this channel's folder", or "no working tree".
    pub label: String,
    /// The sentence that explains an unavailable tree; `None` when available.
    pub reason: Option<String>,
    pub refusal: Option<CodingSessionTreeRefusal>,
}

/// One entry under the root, relative to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionTreeEntry {
    pub name: String,
    /// `/`-separated, relative to the root, never starting with `/`.
    pub rel_path: String,
    pub kind: CodingSessionTreeEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CodingSessionTreeEntryKind {
    Directory,
    File,
    /// Listed, never followed: its target may be outside the root.
    Symlink,
}

/// One directory's listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionTreeListing {
    pub entries: Vec<CodingSessionTreeEntry>,
    /// True when the directory held more than [`MAX_TREE_ENTRIES`] entries.
    pub truncated: bool,
    /// Entries present but not listed because they could not be represented
    /// (a non-UTF-8 name, an unreadable entry or type). Never counts `.git`,
    /// which is hidden by contract. Non-zero means the listing is incomplete.
    pub omitted: usize,
}

/// A refusal with the sentence the surface shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodingSessionTreeError {
    pub refusal: CodingSessionTreeRefusal,
    pub message: String,
}

fn non_empty(value: Option<&String>) -> Option<&str> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
}

/// The canonical form of `path` when it is an existing directory.
fn canonical_dir(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let canonical = std::fs::canonicalize(path).ok()?;
    canonical.is_dir().then_some(canonical)
}

/// Resolve the tree root from an already-loaded store. Pure apart from the
/// `stat`s that check a recorded directory still exists.
pub(crate) fn resolve_tree_root_in_store(
    store: &CodingSessionWorkdirStore,
    query: &CodingSessionTreeQuery,
) -> Result<(PathBuf, CodingSessionTreeSource), CodingSessionTreeError> {
    if let Some(session_id) = non_empty(query.session_id.as_ref()) {
        let recorded = store
            .worktrees
            .values()
            .filter(|entry| entry.session_id.as_deref().map(str::trim) == Some(session_id))
            .find_map(|entry| canonical_dir(&entry.path));
        if let Some(root) = recorded {
            return Ok((root, CodingSessionTreeSource::Session));
        }
    }
    if !query.is_local_provider {
        return Err(CodingSessionTreeError {
            refusal: CodingSessionTreeRefusal::NotLocal,
            message: "The working tree is on another computer.".to_string(),
        });
    }
    if query.is_hired_seat {
        return Err(CodingSessionTreeError {
            refusal: CodingSessionTreeRefusal::NotRecorded,
            message: "No worktree cut for this hired seat is recorded on this computer, and the \
                      project's checkout is not where it works."
                .to_string(),
        });
    }
    let project = non_empty(query.project_ref.as_ref())
        .and_then(|key| store.by_project.get(key))
        .and_then(|entry| canonical_dir(&entry.path))
        .map(|root| (root, CodingSessionTreeSource::Project));
    let channel = || {
        non_empty(query.channel_id.as_ref())
            .and_then(|key| store.by_channel.get(key))
            .and_then(|entry| canonical_dir(&entry.path))
            .map(|root| (root, CodingSessionTreeSource::Channel))
    };
    project
        .or_else(channel)
        .ok_or_else(|| CodingSessionTreeError {
            refusal: CodingSessionTreeRefusal::NotRecorded,
            message: "No working tree for this session is recorded on this computer.".to_string(),
        })
}

/// Resolve the session's tree root on this machine.
///
/// For host code only (B4's terminal resolves its `cwd` here). The path this
/// returns must never be sent to the renderer or into an event.
pub(crate) fn resolve_coding_session_tree_root(
    app: &AppHandle,
    query: &CodingSessionTreeQuery,
) -> Result<(PathBuf, CodingSessionTreeSource), CodingSessionTreeError> {
    resolve_tree_root_from_loaded(load_workdir_store_readonly(app), query)
}

/// Resolve against the outcome of loading the store. A load error stays on
/// this side of the boundary: serde's messages quote the offending value,
/// which in this store is a path, so the error goes to the host log and the
/// renderer gets a fixed sentence.
pub(crate) fn resolve_tree_root_from_loaded(
    loaded: Result<CodingSessionWorkdirStore, String>,
    query: &CodingSessionTreeQuery,
) -> Result<(PathBuf, CodingSessionTreeSource), CodingSessionTreeError> {
    let store = loaded.map_err(|error| {
        tracing::warn!(%error, "session tree: working-directory record unreadable");
        CodingSessionTreeError {
            refusal: CodingSessionTreeRefusal::StoreUnreadable,
            message: "The working-directory record on this computer could not be read.".to_string(),
        }
    })?;
    resolve_tree_root_in_store(&store, query)
}

fn source_label(source: CodingSessionTreeSource) -> &'static str {
    match source {
        CodingSessionTreeSource::Session => "this session's worktree",
        CodingSessionTreeSource::Project => "this project's checkout",
        CodingSessionTreeSource::Channel => "this channel's folder",
    }
}

/// The renderer-safe answer for a resolve attempt.
pub(crate) fn tree_resolution(
    resolved: Result<(PathBuf, CodingSessionTreeSource), CodingSessionTreeError>,
) -> CodingSessionTreeResolution {
    match resolved {
        Ok((_, source)) => CodingSessionTreeResolution {
            available: true,
            source: Some(source),
            label: source_label(source).to_string(),
            reason: None,
            refusal: None,
        },
        Err(error) => CodingSessionTreeResolution {
            available: false,
            source: None,
            label: "no working tree".to_string(),
            reason: Some(error.message),
            refusal: Some(error.refusal),
        },
    }
}

fn refuse(message: &str) -> CodingSessionTreeError {
    CodingSessionTreeError {
        refusal: CodingSessionTreeRefusal::NotRecorded,
        message: message.to_string(),
    }
}

/// Join `rel_path` under `root`, refusing every escape.
///
/// `root` must already be canonical. The join is checked lexically first
/// (absolute, `..`, `.git`), then canonicalized and checked again, which is
/// what catches a symlink that points out of the tree.
pub(crate) fn resolve_relative_dir(
    root: &Path,
    rel_path: &str,
) -> Result<PathBuf, CodingSessionTreeError> {
    let trimmed = rel_path.trim();
    let relative = Path::new(trimmed);
    if relative.is_absolute() || trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(refuse("An absolute path is not inside the session's tree."));
    }
    for component in relative.components() {
        match component {
            Component::Normal(part) if part == ".git" => {
                return Err(refuse("The repository's .git directory is not listed."));
            }
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(refuse("A path that climbs out with `..` is refused."));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(refuse("An absolute path is not inside the session's tree."));
            }
        }
    }
    let joined = root.join(relative);
    let canonical = std::fs::canonicalize(&joined)
        .map_err(|_| refuse("That folder does not exist in the session's tree."))?;
    let Ok(inside) = canonical.strip_prefix(root) else {
        return Err(refuse("That path leads outside the session's tree."));
    };
    // The lexical check above sees only what was asked for; a symlink inside
    // the tree (`gitdir -> .git`) can still land in the repository's internals.
    if inside
        .components()
        .any(|component| matches!(component, Component::Normal(part) if part == ".git"))
    {
        return Err(refuse("The repository's .git directory is not listed."));
    }
    if !canonical.is_dir() {
        return Err(refuse("That path is not a folder."));
    }
    Ok(canonical)
}

/// List one directory under `root`, relative entries only.
pub(crate) fn list_tree_entries(
    root: &Path,
    rel_path: &str,
) -> Result<CodingSessionTreeListing, CodingSessionTreeError> {
    let dir = resolve_relative_dir(root, rel_path)?;
    let read = std::fs::read_dir(&dir).map_err(|_| refuse("That folder could not be read."))?;
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut omitted = 0usize;
    for item in read {
        let Ok(item) = item else {
            omitted += 1;
            continue;
        };
        let Some(name) = item.file_name().to_str().map(str::to_string) else {
            omitted += 1;
            continue;
        };
        if name == ".git" {
            continue;
        }
        if entries.len() >= MAX_TREE_ENTRIES {
            truncated = true;
            break;
        }
        let Ok(file_type) = item.file_type() else {
            omitted += 1;
            continue;
        };
        let kind = if file_type.is_symlink() {
            CodingSessionTreeEntryKind::Symlink
        } else if file_type.is_dir() {
            CodingSessionTreeEntryKind::Directory
        } else {
            CodingSessionTreeEntryKind::File
        };
        let Ok(relative) = item.path().strip_prefix(root).map(Path::to_path_buf) else {
            omitted += 1;
            continue;
        };
        let rel_path = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        entries.push(CodingSessionTreeEntry {
            name,
            rel_path,
            kind,
        });
    }
    entries.sort_by(|left, right| {
        let left_dir = left.kind == CodingSessionTreeEntryKind::Directory;
        let right_dir = right.kind == CodingSessionTreeEntryKind::Directory;
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(CodingSessionTreeListing {
        entries,
        truncated,
        omitted,
    })
}

/// Where this session's working tree is on this computer, without its path.
#[tauri::command]
pub fn resolve_coding_session_tree(
    app: AppHandle,
    session: CodingSessionTreeQuery,
) -> CodingSessionTreeResolution {
    tree_resolution(resolve_coding_session_tree_root(&app, &session))
}

/// One folder of the session's working tree, as relative entries.
#[tauri::command]
pub fn list_coding_session_tree_entries(
    app: AppHandle,
    session: CodingSessionTreeQuery,
    rel_path: String,
) -> Result<CodingSessionTreeListing, String> {
    let (root, _) =
        resolve_coding_session_tree_root(&app, &session).map_err(|error| error.message)?;
    list_tree_entries(&root, &rel_path).map_err(|error| error.message)
}

#[cfg(test)]
#[path = "session_tree_tests.rs"]
mod tests;
