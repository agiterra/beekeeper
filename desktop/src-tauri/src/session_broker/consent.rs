//! Backend agent-interaction consent — the boundary the session broker enforces.
//!
//! A per-session (workspaceId) allowlist of sessions the owner has permitted
//! buzz **agents** to drive. Default-off. This is deliberately a Rust/backend
//! store, not the frontend localStorage the human "Interact" consent uses:
//! agents reach sessions only through the broker, and the broker checks this
//! store, so an agent process cannot bypass it. Persisted to a `0600` JSON file
//! in the app-data dir; the in-memory source of truth is a module-owned static,
//! hydrated from disk at setup.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// In-memory source of truth for agent consent, hydrated from disk at setup.
/// Module-owned (not on `AppState`) so it stays self-contained.
static AGENT_CONSENT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn cell() -> &'static Mutex<HashSet<String>> {
    AGENT_CONSENT.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConsentFile {
    /// Workspace ids agents may drive.
    #[serde(default)]
    consented: Vec<String>,
}

fn consent_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?
        .join("sessions");
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create sessions dir: {e}"))?;
    Ok(dir)
}

fn consent_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(consent_dir(app)?.join("agent-consent.json"))
}

/// Read the persisted agent-consent set. A missing/corrupt file reads as empty
/// (fail-closed: no session is agent-drivable unless explicitly recorded).
pub fn load(app: &AppHandle) -> HashSet<String> {
    let Ok(path) = consent_path(app) else {
        return HashSet::new();
    };
    load_from_path(&path)
}

fn load_from_path(path: &std::path::Path) -> HashSet<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return HashSet::new();
    };
    match serde_json::from_slice::<ConsentFile>(&bytes) {
        Ok(file) => file.consented.into_iter().collect(),
        Err(_) => HashSet::new(),
    }
}

/// Persist the agent-consent set as a `0600` JSON file.
pub fn save(app: &AppHandle, set: &HashSet<String>) -> Result<(), String> {
    let path = consent_path(app)?;
    save_to_path(&path, set)
}

fn save_to_path(path: &std::path::Path, set: &HashSet<String>) -> Result<(), String> {
    let mut consented: Vec<String> = set.iter().cloned().collect();
    consented.sort();
    let json = serde_json::to_vec_pretty(&ConsentFile { consented })
        .map_err(|e| format!("failed to encode agent consent: {e}"))?;
    write_owner_only(path, &json)
}

#[cfg(unix)]
fn write_owner_only(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("failed to open agent-consent file: {e}"))?;
    file.write_all(bytes)
        .map_err(|e| format!("failed to write agent-consent file: {e}"))
}

#[cfg(not(unix))]
fn write_owner_only(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("failed to write agent-consent file: {e}"))
}

/// Load the persisted set into memory at startup.
pub fn hydrate(app: &AppHandle) {
    let set = load(app);
    if let Ok(mut guard) = cell().lock() {
        *guard = set;
    }
}

/// Whether agents may drive this session right now.
pub fn is_agent_consented(workspace_id: &str) -> bool {
    cell()
        .lock()
        .map(|set| set.contains(workspace_id))
        .unwrap_or(false)
}

/// Grant/revoke agent consent for a session: update memory + persist.
pub fn set_agent_consented(
    app: &AppHandle,
    workspace_id: &str,
    allowed: bool,
) -> Result<(), String> {
    let snapshot = {
        let mut guard = cell()
            .lock()
            .map_err(|_| "agent-consent lock poisoned".to_string())?;
        if allowed {
            guard.insert(workspace_id.to_string());
        } else {
            guard.remove(workspace_id);
        }
        guard.clone()
    };
    save(app, &snapshot)
}

/// The current agent-consented workspace ids (sorted), for the settings UI.
pub fn list_agent_consented() -> Vec<String> {
    let mut ids: Vec<String> = cell()
        .lock()
        .map(|set| set.iter().cloned().collect())
        .unwrap_or_default();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("buzz-consent-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir.join(name)
    }

    #[test]
    fn missing_file_reads_empty() {
        assert!(load_from_path(&temp_path("does-not-exist.json")).is_empty());
    }

    #[test]
    fn corrupt_file_fails_closed_to_empty() {
        let path = temp_path("corrupt.json");
        std::fs::write(&path, b"{not json").expect("write");
        assert!(load_from_path(&path).is_empty());
    }

    #[test]
    fn round_trips_sorted_and_owner_only() {
        let path = temp_path("round-trip.json");
        let set: HashSet<String> = ["shell:b", "shell:a"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        save_to_path(&path, &set).expect("save");

        // Sorted, deterministic serialization.
        let raw = std::fs::read_to_string(&path).expect("read");
        assert!(raw.find("shell:a").expect("a") < raw.find("shell:b").expect("b"));

        // Owner-only permissions on unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "consent file must be 0600");
        }

        assert_eq!(load_from_path(&path), set);
    }
}
