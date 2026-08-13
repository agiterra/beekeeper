//! On-disk persistence for built-in shell sessions.
//!
//! A live shell *process* cannot outlive the app (its PTY master dies with us,
//! and a reboot kills it outright). What we can persist is enough to bring a
//! session back: its metadata (title, shell, working directory) and its
//! scrollback. On the next launch those become "restorable" sessions — reopen
//! one and it spawns a fresh shell in the saved directory with the old history
//! replayed above the new prompt.
//!
//! Controlled by a default-on "persist" setting. Files live under
//! `app_data_dir/shell-sessions/`: one `<id>.json` (metadata) + `<id>.log` (raw
//! scrollback, ANSI intact) per session, plus `settings.json` for the toggle.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// Cap on persisted scrollback per session, matching the in-memory cap.
fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    #[serde(default = "default_true")]
    persist: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { persist: true }
    }
}

/// Persisted session metadata (everything but the scrollback).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedMeta {
    pub session_id: String,
    pub title: String,
    pub shell: String,
    /// Working directory at the last save — where a resume respawns the shell.
    pub cwd: String,
    pub created_at: u64,
    /// The project container this session was tagged with, if any. Absent
    /// (`None`) on files written before this field existed — `#[serde(default)]`
    /// makes that a clean load rather than a parse failure.
    #[serde(default)]
    pub project_ref: Option<String>,
}

/// A restorable session loaded from disk: metadata + its saved scrollback.
pub struct PersistedSession {
    pub meta: PersistedMeta,
    pub scrollback: Vec<u8>,
}

fn base_dir(app: &AppHandle) -> Result<PathBuf, String> {
    dir(app)
}

/// The shell-session history directory (`app_data_dir/shell-sessions`). Public
/// so the manager can hand it to the detached host, which writes the same
/// reboot-fallback `<id>.json`/`<id>.log` files this module reads.
pub fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?
        .join("shell-sessions");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create shell-sessions dir: {e}"))?;
    Ok(dir)
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(base_dir(app)?.join("settings.json"))
}

fn meta_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(base_dir(app)?.join(format!("{id}.json")))
}

fn log_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(base_dir(app)?.join(format!("{id}.log")))
}

/// Whether session persistence is on. Missing/corrupt settings default to on.
pub fn enabled(app: &AppHandle) -> bool {
    let Ok(path) = settings_path(app) else {
        return true;
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Settings>(&bytes)
            .map(|s| s.persist)
            .unwrap_or(true),
        // No file yet → default on.
        Err(_) => true,
    }
}

/// Turn persistence on or off. Turning it off purges everything on disk so no
/// stale history lingers.
pub fn set_enabled(app: &AppHandle, persist: bool) -> Result<(), String> {
    let path = settings_path(app)?;
    let json = serde_json::to_vec_pretty(&Settings { persist })
        .map_err(|e| format!("failed to encode shell settings: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("failed to write shell settings: {e}"))?;
    if !persist {
        purge_all(app);
    }
    Ok(())
}

// History is written by the detached host (`buzz-shell-host`), which owns the
// scrollback and keeps checkpointing even while the app is closed. This module
// only reads it back (`load_all`) for the reboot fallback and cleans it up.

/// Update the persisted title for a session. Used to rename a *dormant*
/// (restorable) session — a live session's title is owned and persisted by its
/// host process instead. A missing/unparseable metadata file is a no-op.
pub fn rename(app: &AppHandle, id: &str, title: &str) {
    let Ok(path) = meta_path(app, id) else {
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let Ok(mut meta) = serde_json::from_slice::<PersistedMeta>(&bytes) else {
        return;
    };
    meta.title = title.to_string();
    if let Ok(json) = serde_json::to_vec_pretty(&meta) {
        let _ = std::fs::write(&path, json);
    }
}

/// Update the persisted project-container tag for a session. Used to persist
/// `set_project_ref` for a *dormant* (restorable) session — a live session's
/// tag is app-side-only bookkeeping (see `manager::set_project_ref`). A
/// missing/unparseable metadata file is a no-op.
pub fn set_project_ref(app: &AppHandle, id: &str, project_ref: Option<String>) {
    let Ok(path) = meta_path(app, id) else {
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let Ok(mut meta) = serde_json::from_slice::<PersistedMeta>(&bytes) else {
        return;
    };
    meta.project_ref = project_ref;
    if let Ok(json) = serde_json::to_vec_pretty(&meta) {
        let _ = std::fs::write(&path, json);
    }
}

/// Forget a persisted session (its files). Missing files are not an error.
pub fn remove(app: &AppHandle, id: &str) {
    if let Ok(p) = meta_path(app, id) {
        let _ = std::fs::remove_file(p);
    }
    if let Ok(p) = log_path(app, id) {
        let _ = std::fs::remove_file(p);
    }
}

/// Load every persisted session, oldest first. Entries whose metadata can't be
/// parsed are skipped rather than failing the whole load.
pub fn load_all(app: &AppHandle) -> Vec<PersistedSession> {
    let Ok(dir) = base_dir(app) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("settings.json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_slice::<PersistedMeta>(&bytes) else {
            continue;
        };
        let scrollback = log_path(app, &meta.session_id)
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .unwrap_or_default();
        out.push(PersistedSession { meta, scrollback });
    }
    out.sort_by_key(|s| s.meta.created_at);
    out
}

/// Delete all persisted session files (keeps `settings.json`).
pub fn purge_all(app: &AppHandle) {
    let Ok(dir) = base_dir(app) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("settings.json") {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_on_and_missing_file_reads_true() {
        // A default Settings and an absent file both mean persistence is on.
        assert!(Settings::default().persist);
        let parsed: Settings = serde_json::from_str("{}").expect("empty settings");
        assert!(parsed.persist, "missing field must default to on");
        let off: Settings = serde_json::from_str(r#"{"persist":false}"#).expect("off");
        assert!(!off.persist);
    }

    #[test]
    fn meta_round_trips_through_json() {
        let meta = PersistedMeta {
            session_id: "abc".to_string(),
            title: "innovo".to_string(),
            shell: "/bin/zsh".to_string(),
            cwd: "/Users/andy/Code/innovo".to_string(),
            created_at: 42,
            project_ref: Some("30178:deadbeef:my-project".to_string()),
        };
        let json = serde_json::to_string(&meta).expect("encode");
        // camelCase on the wire.
        assert!(json.contains("\"sessionId\":\"abc\""));
        assert!(json.contains("\"createdAt\":42"));
        assert!(json.contains("\"projectRef\":\"30178:deadbeef:my-project\""));
        let back: PersistedMeta = serde_json::from_str(&json).expect("decode");
        assert_eq!(back.cwd, meta.cwd);
        assert_eq!(back.title, meta.title);
        assert_eq!(back.project_ref, meta.project_ref);
    }

    #[test]
    fn meta_without_project_ref_field_defaults_to_none() {
        // Files written before project containers existed lack the key
        // entirely; they must still parse, with the field defaulting to None.
        let legacy =
            r#"{"sessionId":"abc","title":"innovo","shell":"/bin/zsh","cwd":"/tmp","createdAt":1}"#;
        let meta: PersistedMeta = serde_json::from_str(legacy).expect("legacy meta parses");
        assert_eq!(meta.project_ref, None);
    }
}
