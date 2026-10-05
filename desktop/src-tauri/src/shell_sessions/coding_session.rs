//! A built-in shell opened for a coding session (SV-25, DB9, DB11).
//!
//! The session view's terminal drawer opens the person's own login shell in
//! the session's working tree **as this machine recorded it**. The renderer
//! never names a directory: it sends a reference to the session (which
//! session, which channel, which project) and Rust resolves the directory
//! through the coding-session tree lookup
//! (`coding_sessions::session_tree::resolve_coding_session_tree_root`). When
//! that lookup refuses, the shell is refused with the tree's own sentence —
//! there is no fallback to `$HOME`, because a shell that opens somewhere else
//! while its header says "this session's tree" would be a lie.
//!
//! The resolved path stays on this side of the IPC boundary: a session
//! shell's `ShellSessionInfo` reaches the renderer with an empty
//! `current_directory` ([`renderer_view`]), and its 30623 announce carries
//! the session reference, never a path ([`announce_session_tag`]).
//!
//! The shell is **not** the agent's sandbox. It runs with the person's own
//! keychain, credentials and tools; the drawer's header says "not sandboxed".

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::coding_sessions::session_tree::{
    resolve_coding_session_tree_root, CodingSessionTreeQuery,
};

use super::manager::ShellSessionInfo;

/// The longest session reference an announce carries. A `sessionRef` is a
/// relay coordinate or an `implicit:<target>` key, both far shorter.
pub const MAX_SESSION_TAG_CHARS: usize = 256;

/// Which coding session a shell was opened for. Host-local bookkeeping
/// (persisted in the app-owned sidecar map); only `session_ref` ever leaves
/// this machine, as the announce's `session` tag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellCodingSessionRef {
    /// The session view's key: the umbrella `sessionRef`, or its implicit
    /// key for a session of one. Announced as `["session", <this>]`.
    pub session_ref: String,
    /// The focused execution's provider-minted session id, when known.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The session's channel id.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// The session's NIP-MP project coordinate, when it names one.
    #[serde(default)]
    pub project_ref: Option<String>,
    /// This machine's provider runs the focused execution (gates the
    /// project and channel defaults, DB9).
    #[serde(default)]
    pub is_local_provider: bool,
    /// An agent is seated on the execution (refuses the defaults).
    #[serde(default)]
    pub is_hired_seat: bool,
}

impl ShellCodingSessionRef {
    /// The tree lookup this reference stands for.
    pub(crate) fn tree_query(&self) -> CodingSessionTreeQuery {
        CodingSessionTreeQuery {
            session_id: self.session_id.clone(),
            channel_id: self.channel_id.clone(),
            project_ref: self.project_ref.clone(),
            is_local_provider: self.is_local_provider,
            is_hired_seat: self.is_hired_seat,
        }
    }
}

/// Refuse a reference that names no session.
pub(crate) fn validate_reference(reference: &ShellCodingSessionRef) -> Result<(), String> {
    let session_ref = reference.session_ref.trim();
    if session_ref.is_empty() {
        return Err("A session terminal needs the session it belongs to.".to_string());
    }
    if session_ref.chars().count() > MAX_SESSION_TAG_CHARS
        || session_ref.chars().any(char::is_control)
    {
        return Err("That session reference is not one a terminal can carry.".to_string());
    }
    Ok(())
}

/// The directory a session shell starts in: the session's tree root on this
/// machine, or the tree's refusal sentence. Never `$HOME`.
pub(crate) fn resolve_session_cwd(
    app: &AppHandle,
    reference: &ShellCodingSessionRef,
) -> Result<String, String> {
    validate_reference(reference)?;
    let (root, _) = resolve_coding_session_tree_root(app, &reference.tree_query())
        .map_err(|error| error.message)?;
    Ok(root.to_string_lossy().into_owned())
}

/// The directory a new session shell starts in. A directory from the caller
/// is refused, not trusted: the renderer never chooses where a session shell
/// opens (DB9).
pub(crate) fn create_cwd(
    app: &AppHandle,
    requested: Option<&str>,
    reference: &ShellCodingSessionRef,
) -> Result<String, String> {
    if requested.is_some_and(|cwd| !cwd.trim().is_empty()) {
        return Err(
            "A session terminal opens in the session's tree; it takes no directory.".to_string(),
        );
    }
    resolve_session_cwd(app, reference)
}

/// Where a resumed session shell starts: the directory it was last in when
/// that is still inside the session's tree, else the tree's root, else the
/// tree's refusal. Never `$HOME`.
pub(crate) fn resume_session_cwd(
    app: &AppHandle,
    reference: &ShellCodingSessionRef,
    saved_cwd: &str,
) -> Result<String, String> {
    let root = resolve_session_cwd(app, reference)?;
    Ok(choose_resume_cwd(Path::new(&root), saved_cwd))
}

/// [`resolve_session_cwd`] against an already-loaded store (the testable
/// half: everything but reading the store from disk).
#[cfg(test)]
pub(crate) fn resolve_session_cwd_in_store(
    store: &crate::coding_sessions::workdir_store::CodingSessionWorkdirStore,
    reference: &ShellCodingSessionRef,
) -> Result<String, String> {
    validate_reference(reference)?;
    let (root, _) = crate::coding_sessions::session_tree::resolve_tree_root_in_store(
        store,
        &reference.tree_query(),
    )
    .map_err(|error| error.message)?;
    Ok(root.to_string_lossy().into_owned())
}

/// The pure half of [`resume_session_cwd`]: `saved_cwd` when it canonicalizes
/// to a directory under `root`, otherwise `root`.
pub(crate) fn choose_resume_cwd(root: &Path, saved_cwd: &str) -> String {
    let inside = std::fs::canonicalize(saved_cwd)
        .ok()
        .filter(|saved| saved.is_dir())
        .zip(std::fs::canonicalize(root).ok())
        .is_some_and(|(saved, root)| saved.starts_with(root));
    if inside {
        saved_cwd.to_string()
    } else {
        root.to_string_lossy().into_owned()
    }
}

/// The title a session shell gets when the caller names none: never the
/// directory's name, which would put a piece of the path into the announce.
pub(crate) const SESSION_SHELL_DEFAULT_TITLE: &str = "Terminal";

/// What the renderer may see of a shell: a session shell's directory is
/// blanked, since the tree's path is a host-local fact (DB9).
pub fn renderer_view(mut info: ShellSessionInfo) -> ShellSessionInfo {
    if info.coding_session.is_some() {
        info.current_directory = String::new();
    }
    info
}

/// The value of the announce's `["session", …]` tag (DB11), when the shell
/// was opened for a session and its reference is one an announce may carry.
pub(crate) fn announce_session_tag(info: &ShellSessionInfo) -> Option<String> {
    let reference = info.coding_session.as_ref()?;
    validate_reference(reference).ok()?;
    Some(reference.session_ref.trim().to_string())
}

#[cfg(test)]
#[path = "coding_session_tests.rs"]
mod tests;
