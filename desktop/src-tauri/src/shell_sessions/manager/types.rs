//! Shared types + session registries for the shell-session manager.
//!
//! Split out of `manager.rs` (which owns all the behavior) to keep that file
//! under the repo's file-size guard. Purely data/registry plumbing: the
//! session info type returned to the frontend, the internal per-session
//! state shared with the reader thread, and the two module-owned registries
//! (live + dormant sessions) manager.rs operates on.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::shell_sessions::host_client::{AttachedClient, HostClient};

/// One invited member on a shared terminal's roster: a pubkey (lowercase
/// 64-hex) and their role (`"collaborator"` may watch and type via
/// kind:24312; `"viewer"` may only watch). The owner signs the announce and
/// is never listed. Mirrored onto the kind:30623 announce as arity-4 `p`
/// tags, so the roster IS the revocable grant observers see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterEntry {
    pub pubkey: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSessionInfo {
    pub session_id: String,
    pub title: String,
    pub current_directory: String,
    pub shell: String,
    /// Unix seconds the session was created, for stable ordering in the UI.
    pub created_at: u64,
    pub rows: u16,
    pub cols: u16,
    /// False once the shell has exited (the entry lingers until closed so the
    /// UI can show the exit instead of a vanishing terminal).
    pub running: bool,
    /// True for a session restored from disk that has no live shell yet —
    /// opening it (or calling `resume`) respawns a shell in its saved directory
    /// with the persisted history replayed.
    #[serde(default)]
    pub restorable: bool,
    /// The `30621:<owner>:<slug>` coordinate of the project container this
    /// session belongs to, if any. Set via `set_project_ref` and persisted in
    /// the app-owned sidecar map (`persist::AppMeta`); a session with a real
    /// project ref is announced to that project per NIP-ST.
    #[serde(default)]
    pub project_ref: Option<String>,
    /// Whether this session is observable (read-only) by its project's
    /// members (NIP-ST). Default on; meaningless without a `project_ref`.
    #[serde(default)]
    pub shared: bool,
    /// Individually invited members (collaborator | viewer). Admitted to
    /// watch regardless of `shared`; collaborators may also type remotely.
    /// Persisted in the app-owned sidecar map (`persist::AppMeta`).
    #[serde(default)]
    pub roster: Vec<RosterEntry>,
}

/// A read of a session's output for an agent: rendered/plain text plus the
/// cursor and input-line state needed to drive it without guesswork.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellRead {
    /// The requested text (plain ANSI-stripped stream, or vt100-rendered screen).
    pub text: String,
    /// Monotonic byte cursor: pass back as `since` to read only new output.
    pub cursor: u64,
    /// True if `since` pointed before the retained buffer, so some output
    /// between the old cursor and `text` was dropped.
    pub truncated: bool,
    /// The current terminal input line (row under the cursor), so an agent can
    /// see leftover typed text before sending.
    pub input_line: String,
    /// Cursor column within `input_line`.
    pub cursor_col: u16,
}

/// Mutable per-session state shared with the reader thread.
pub(super) struct SharedState {
    /// Raw PTY bytes (ANSI intact), capped at `SCROLLBACK_CAP` from the front.
    pub(super) scrollback: Vec<u8>,
    /// Total bytes ever appended (monotonic; survives front-drops). The byte at
    /// absolute offset `o` lives at `scrollback[o - (total - scrollback.len())]`.
    pub(super) total: u64,
    /// When the last chunk arrived, for exec quiescence detection.
    pub(super) last_output_at: Option<Instant>,
    /// Terminal emulator fed the same stream, for rendered reads + input line.
    pub(super) parser: vt100::Parser,
}

pub(super) struct ShellSession {
    pub(super) info: ShellSessionInfo,
    /// Full-authority write side of the host connection: kill (`close`) and
    /// rename (`rename`) only. Deliberately has no `input`/`resize` — those
    /// go through `io` instead, so `write`/`resize` can only reach the host
    /// via the driver (see `host_client::AttachedClient`).
    pub(super) client: HostClient,
    /// Driver-narrowed I/O handle for keystrokes/resize, sharing the same
    /// connection as `client`. `write`/`resize` use this exclusively.
    pub(super) io: AttachedClient,
    pub(super) state: Arc<Mutex<SharedState>>,
}

/// A session restored from disk with no live host (reboot fallback). Holds the
/// replayed history (in `state`) so reads show it; `resume` spawns a new host.
pub(super) struct DormantSession {
    pub(super) info: ShellSessionInfo,
    pub(super) state: Arc<Mutex<SharedState>>,
    /// The saved working directory a resume respawns in.
    pub(super) cwd: String,
}

static SESSIONS: OnceLock<Mutex<HashMap<String, ShellSession>>> = OnceLock::new();
static DORMANT: OnceLock<Mutex<HashMap<String, DormantSession>>> = OnceLock::new();

pub(super) fn registry() -> &'static Mutex<HashMap<String, ShellSession>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn dormant() -> &'static Mutex<HashMap<String, DormantSession>> {
    DORMANT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn lock_registry(
) -> Result<std::sync::MutexGuard<'static, HashMap<String, ShellSession>>, String> {
    registry()
        .lock()
        .map_err(|_| "shell-session registry lock poisoned".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info_with(project_ref: Option<&str>) -> ShellSessionInfo {
        ShellSessionInfo {
            session_id: "sess-1".to_string(),
            title: "shell".to_string(),
            current_directory: "/tmp".to_string(),
            shell: "/bin/zsh".to_string(),
            created_at: 1,
            rows: 24,
            cols: 80,
            running: true,
            restorable: false,
            project_ref: project_ref.map(str::to_string),
            shared: true,
            roster: Vec::new(),
        }
    }

    #[test]
    fn project_ref_serializes_camel_case_when_present() {
        let json =
            serde_json::to_string(&info_with(Some("30621:deadbeef:my-project"))).expect("encode");
        assert!(
            json.contains("\"projectRef\":\"30621:deadbeef:my-project\""),
            "json: {json}"
        );
    }

    #[test]
    fn project_ref_serializes_null_when_absent() {
        let json = serde_json::to_string(&info_with(None)).expect("encode");
        assert!(json.contains("\"projectRef\":null"), "json: {json}");
    }
}
