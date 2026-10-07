//! File paths an agent wrote in a coding-session answer, resolved on **this
//! machine** (SV-32).
//!
//! An answer that says `` `desktop/src/app/App.tsx:42` `` names a file in the
//! session's working tree. Whether that tree is on this computer, and where, is
//! a host-local fact decided by the same resolver the Files surface and the
//! terminal use ([`resolve_tree_root_in_store`]): the worktree cut for the
//! execution's session id, then — only when this machine's provider runs the
//! execution and it is not a hired seat — the project's checkout, then the
//! channel's folder. One resolver, so the chips and the Files surface never
//! disagree about "this session's folder".
//!
//! # What crosses the IPC boundary
//!
//! For a session whose tree is on this computer the renderer gets, per
//! candidate, whether it exists, whether it is a folder, its path relative to
//! the folder it resolved against, and its full path (for "Copy full path").
//! For any other session it gets no refs at all — only *why*, in a sentence.
//! Nothing here is ever written into an event.
//!
//! # Opening is re-resolved here
//!
//! [`coding_session_open_file_ref`] and [`coding_session_reveal_file_ref`]
//! take the session scope and the candidate **as the agent wrote it**, never a
//! path the renderer computed. They resolve the tree and the candidate again at
//! click time, require the file to exist, and only then call the opener from
//! Rust. The opener capability granted to the webview is not widened.
//!
//! # Containment
//!
//! Lookup reports a `../` candidate (or an in-tree symlink whose target is
//! elsewhere) with `relativePath: null`, so the surface shows it as plain text
//! saying it is outside the session's folder. Open and reveal refuse it: the
//! text an answer carries never launches anything outside the folder it was
//! resolved against.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::session_tree::{
    resolve_tree_root_from_loaded, resolve_tree_root_in_store, CodingSessionTreeError,
    CodingSessionTreeQuery, CodingSessionTreeRefusal, CodingSessionTreeSource,
};
use super::workdir_store::{load_workdir_store_readonly, CodingSessionWorkdirStore};

/// The most candidates one lookup may ask about. One assistant message rarely
/// names more than a dozen paths; past this the request is refused rather than
/// silently truncated.
pub(crate) const MAX_FILE_REF_CANDIDATES: usize = 256;

/// The longest candidate considered. Longer text is not a path an agent wrote
/// in backticks; it is left unresolved (and so stays plain code).
pub(crate) const MAX_FILE_REF_CANDIDATE_BYTES: usize = 1_024;

/// The sentence for an execution another computer ran.
pub(crate) const NOT_LOCAL_REASON: &str = "Written on another computer — open it there";

/// The sentence for a worktree this computer cut whose folder no longer exists.
pub(crate) const FOLDER_GONE_REASON: &str = "This computer ran this agent, but its folder is gone";

/// Why a path that resolves outside the session's folder is not opened.
const OUTSIDE_FOLDER_REASON: &str =
    "That path is outside this session's folder, so it is not opened from here.";

/// Which execution's answer is being resolved. Every field is a relay-visible
/// reference or a fact the renderer already holds; none of them is a path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionFileRefScope {
    /// The session's channel id.
    pub channel_id: String,
    /// The provider-minted session id of the execution that wrote the answer.
    #[serde(default)]
    pub provider_session_id: Option<String>,
    /// The NIP-MP project coordinate the execution's create named, when any.
    #[serde(default)]
    pub project_ref: Option<String>,
    /// An agent is seated on the execution (a hire): only its own worktree
    /// counts.
    #[serde(default)]
    pub is_hired_seat: bool,
    /// This machine's provider signed the execution's items.
    #[serde(default)]
    pub is_local_provider: bool,
}

impl CodingSessionFileRefScope {
    fn tree_query(&self) -> CodingSessionTreeQuery {
        CodingSessionTreeQuery {
            session_id: self.provider_session_id.clone(),
            channel_id: Some(self.channel_id.clone()),
            project_ref: self.project_ref.clone(),
            is_local_provider: self.is_local_provider,
            is_hired_seat: self.is_hired_seat,
        }
    }
}

/// A lookup: one execution's scope and the candidates its answer contains.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionFileRefsRequest {
    #[serde(flatten)]
    pub scope: CodingSessionFileRefScope,
    /// Candidates exactly as the agent wrote them (`path[:line[:col]]`).
    #[serde(default)]
    pub candidates: Vec<String>,
}

/// An open or reveal: the scope and the one candidate clicked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionFileRefActionRequest {
    #[serde(flatten)]
    pub scope: CodingSessionFileRefScope,
    /// The candidate as the agent wrote it — never a resolved path.
    pub candidate: String,
}

/// Where the execution's folder is, as far as this computer can prove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CodingSessionFileRefWhere {
    /// The tree resolved on this computer; refs are filled in.
    ThisComputer,
    /// This computer cut a worktree for the session, and it no longer exists.
    FolderGone,
    /// Another computer's provider ran it and this host cut no tree for it.
    NotLocal,
    /// No record on this computer names a folder for it.
    NotRecorded,
    /// The host-local record could not be read.
    StoreUnreadable,
}

/// One candidate, resolved under the session's folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionFileRef {
    pub exists: bool,
    pub is_dir: bool,
    /// `/`-separated, relative to the folder it resolved against; `None` when
    /// it lies outside that folder (a `../` path, a symlink out) or does not
    /// exist.
    pub relative_path: Option<String>,
    /// The canonical path on this computer, only when it exists. For "Copy
    /// full path"; never for an event.
    pub full_path: Option<String>,
    /// The `:line` suffix, when one was given and is not zero.
    pub line: Option<u32>,
    /// The `:line:col` column, when given and not zero.
    pub column: Option<u32>,
}

/// The renderer-safe answer to a lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionFileRefsAnswer {
    #[serde(rename = "where")]
    pub where_: CodingSessionFileRefWhere,
    /// Which record answered, when the tree resolved.
    pub source: Option<CodingSessionTreeSource>,
    /// The sentence that explains plain text; `None` on this computer.
    pub reason: Option<String>,
    /// RFC 3339; the moment this answer was true. Open/reveal never trust it.
    pub checked_at: String,
    /// Keyed by the candidate exactly as asked. Empty unless `thisComputer`;
    /// a candidate that is not path-shaped (absolute, empty, too long) is
    /// absent rather than guessed at.
    pub refs: BTreeMap<String, CodingSessionFileRef>,
}

/// A candidate split into its path and position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedFileRefCandidate {
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

fn positive(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok().filter(|number| *number > 0)
}

/// Split a trailing `:line[:col]` off and refuse anything that is not a
/// relative path. Absolute paths are refused: on the wire they are elided
/// before signing, and the vault-recovered ones are a later slice (S3).
pub(crate) fn parse_file_ref_candidate(raw: &str) -> Option<ParsedFileRefCandidate> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_FILE_REF_CANDIDATE_BYTES || trimmed.contains('\0')
    {
        return None;
    }
    let mut path = trimmed;
    let mut line = None;
    let mut column = None;
    // `a.ts:12:3` → column first, then line; `a.ts:12` → line only.
    if let Some((head, last)) = path.rsplit_once(':') {
        if last.bytes().all(|byte| byte.is_ascii_digit()) && !last.is_empty() {
            if let Some((head2, middle)) = head.rsplit_once(':') {
                if middle.bytes().all(|byte| byte.is_ascii_digit()) && !middle.is_empty() {
                    path = head2;
                    line = positive(middle);
                    column = line.and(positive(last));
                } else {
                    path = head;
                    line = positive(last);
                }
            } else {
                path = head;
                line = positive(last);
            }
        }
    }
    let path = path.replace('\\', "/");
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('~')
        || path.contains(':')
        || Path::new(&path).is_absolute()
    {
        return None;
    }
    Some(ParsedFileRefCandidate { path, line, column })
}

/// The nearest folder at or above `root` that holds a `.git` entry (a
/// directory, or the file a linked worktree has). What `git rev-parse
/// --show-toplevel` answers, without spawning git.
pub(crate) fn git_toplevel(root: &Path) -> Option<PathBuf> {
    root.ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

fn relative_under(base: &Path, path: &Path) -> Option<String> {
    let inside = path.strip_prefix(base).ok()?;
    let parts = inside
        .components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(parts.join("/"))
}

/// Resolve one candidate against `root` (canonical), then the root's git
/// toplevel. The first base under which it exists wins.
pub(crate) fn resolve_file_ref(
    root: &Path,
    parsed: &ParsedFileRefCandidate,
) -> CodingSessionFileRef {
    let toplevel = git_toplevel(root)
        .and_then(|top| std::fs::canonicalize(top).ok())
        .filter(|top| top != root);
    let bases = std::iter::once(root.to_path_buf()).chain(toplevel);
    for base in bases {
        let Ok(canonical) = std::fs::canonicalize(base.join(&parsed.path)) else {
            continue;
        };
        return CodingSessionFileRef {
            exists: true,
            is_dir: canonical.is_dir(),
            relative_path: relative_under(&base, &canonical),
            full_path: canonical.to_str().map(str::to_string),
            line: parsed.line,
            column: parsed.column,
        };
    }
    CodingSessionFileRef {
        exists: false,
        is_dir: false,
        relative_path: None,
        full_path: None,
        line: parsed.line,
        column: parsed.column,
    }
}

/// True when this host cut a worktree for the session and its folder is gone.
fn session_folder_gone(store: &CodingSessionWorkdirStore, session_id: Option<&str>) -> bool {
    let Some(session_id) = session_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return false;
    };
    let mut recorded = store
        .worktrees
        .values()
        .filter(|entry| entry.session_id.as_deref().map(str::trim) == Some(session_id))
        .peekable();
    recorded.peek().is_some() && recorded.all(|entry| !entry.path.is_dir())
}

fn refusal_answer(
    error: CodingSessionTreeError,
    folder_gone: bool,
    checked_at: String,
) -> CodingSessionFileRefsAnswer {
    let (where_, reason) = if folder_gone {
        (
            CodingSessionFileRefWhere::FolderGone,
            FOLDER_GONE_REASON.to_string(),
        )
    } else {
        match error.refusal {
            CodingSessionTreeRefusal::NotLocal => (
                CodingSessionFileRefWhere::NotLocal,
                NOT_LOCAL_REASON.to_string(),
            ),
            CodingSessionTreeRefusal::NotRecorded => {
                (CodingSessionFileRefWhere::NotRecorded, error.message)
            }
            CodingSessionTreeRefusal::StoreUnreadable => {
                (CodingSessionFileRefWhere::StoreUnreadable, error.message)
            }
        }
    };
    CodingSessionFileRefsAnswer {
        where_,
        source: None,
        reason: Some(reason),
        checked_at,
        refs: BTreeMap::new(),
    }
}

fn resolve_root(
    loaded: Result<CodingSessionWorkdirStore, String>,
    scope: &CodingSessionFileRefScope,
) -> Result<(PathBuf, CodingSessionTreeSource), (CodingSessionTreeError, bool)> {
    let query = scope.tree_query();
    match loaded {
        Ok(store) => resolve_tree_root_in_store(&store, &query).map_err(|error| {
            let gone = session_folder_gone(&store, scope.provider_session_id.as_deref());
            (error, gone)
        }),
        Err(error) => resolve_tree_root_from_loaded(Err(error), &query).map_err(|e| (e, false)),
    }
}

/// Answer a lookup from the outcome of loading the store.
pub(crate) fn file_refs_from_loaded(
    loaded: Result<CodingSessionWorkdirStore, String>,
    request: &CodingSessionFileRefsRequest,
    checked_at: String,
) -> Result<CodingSessionFileRefsAnswer, String> {
    if request.candidates.len() > MAX_FILE_REF_CANDIDATES {
        return Err(format!(
            "At most {MAX_FILE_REF_CANDIDATES} paths can be looked up at once; {} were asked for.",
            request.candidates.len()
        ));
    }
    let (root, source) = match resolve_root(loaded, &request.scope) {
        Ok(resolved) => resolved,
        Err((error, gone)) => return Ok(refusal_answer(error, gone, checked_at)),
    };
    let refs = request
        .candidates
        .iter()
        .filter_map(|candidate| {
            let parsed = parse_file_ref_candidate(candidate)?;
            Some((candidate.clone(), resolve_file_ref(&root, &parsed)))
        })
        .collect();
    Ok(CodingSessionFileRefsAnswer {
        where_: CodingSessionFileRefWhere::ThisComputer,
        source: Some(source),
        reason: None,
        checked_at,
        refs,
    })
}

/// Re-resolve the clicked candidate and hand its path to `launch` only when it
/// exists on this computer. Every refusal is a sentence without a path in it.
pub(crate) fn run_file_ref_action(
    loaded: Result<CodingSessionWorkdirStore, String>,
    request: &CodingSessionFileRefActionRequest,
    launch: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let (root, _) = resolve_root(loaded, &request.scope).map_err(|(error, gone)| {
        if gone {
            FOLDER_GONE_REASON.to_string()
        } else if error.refusal == CodingSessionTreeRefusal::NotLocal {
            NOT_LOCAL_REASON.to_string()
        } else {
            error.message
        }
    })?;
    let parsed = parse_file_ref_candidate(&request.candidate)
        .ok_or_else(|| "That is not a path inside this session's folder.".to_string())?;
    let resolved = resolve_file_ref(&root, &parsed);
    if resolved.exists && resolved.relative_path.is_none() {
        return Err(OUTSIDE_FOLDER_REASON.to_string());
    }
    let path = match (resolved.exists, resolved.full_path) {
        (true, Some(path)) => PathBuf::from(path),
        _ => return Err("That file is no longer in this session's folder.".to_string()),
    };
    if !path.exists() {
        return Err("That file is no longer in this session's folder.".to_string());
    }
    launch(&path)
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Resolve the paths in one execution's answer against its folder on this
/// computer. Refuses more than [`MAX_FILE_REF_CANDIDATES`].
#[tauri::command]
pub fn coding_session_file_refs(
    app: AppHandle,
    request: CodingSessionFileRefsRequest,
) -> Result<CodingSessionFileRefsAnswer, String> {
    file_refs_from_loaded(load_workdir_store_readonly(&app), &request, now_rfc3339())
}

/// Open the clicked path in this computer's default app, after resolving it
/// again here.
#[tauri::command]
pub fn coding_session_open_file_ref(
    app: AppHandle,
    request: CodingSessionFileRefActionRequest,
) -> Result<(), String> {
    run_file_ref_action(load_workdir_store_readonly(&app), &request, |path| {
        let path = path
            .to_str()
            .ok_or_else(|| "That path cannot be opened on this computer.".to_string())?;
        app.opener().open_path(path, None::<&str>).map_err(|error| {
            tracing::warn!(%error, "file ref: open failed");
            "This computer could not open that file.".to_string()
        })
    })
}

/// Reveal the clicked path in the file manager, after resolving it again here.
#[tauri::command]
pub fn coding_session_reveal_file_ref(
    app: AppHandle,
    request: CodingSessionFileRefActionRequest,
) -> Result<(), String> {
    run_file_ref_action(load_workdir_store_readonly(&app), &request, |path| {
        app.opener().reveal_item_in_dir(path).map_err(|error| {
            tracing::warn!(%error, "file ref: reveal failed");
            "This computer could not show that file in its folder.".to_string()
        })
    })
}

#[cfg(test)]
#[path = "file_refs_tests.rs"]
mod tests;
