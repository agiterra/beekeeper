//! Where a coding session runs on this machine.
//!
//! # Why this is a separate store, and not an event
//!
//! A 44221 create carries *intent*: which channel, which provider, which
//! project. It deliberately carries no working directory. A path like
//! `/Users/someone/src/private-thing` names a person's disk; publishing it
//! into a channel would hand every current and future member a durable map of
//! that machine, for a value only one machine can act on. So the path stays
//! here, and the provider learns it out of band through a host-written file.
//!
//! # The two files
//!
//! This module owns the **desktop's** record — preferences, defaults, MRU,
//! and one-shot create hints — in `coding-session-workdirs.json`. It also
//! *materializes* the narrower view the provider actually reads
//! (`BUZZ_CSP_PROJECTS_FILE`, i.e. `<state-dir>/projects.json`) on every
//! mutation. Two files rather than one because they answer different
//! questions: this one remembers what the human chose and when, the other is
//! the minimum a subprocess needs to resolve a cwd. The provider re-reads its
//! file per lifecycle command, so a fix here reaches a stuck session without a
//! restart.
//!
//! Materialization is best-effort by design: before any provider has been
//! provisioned there is no state directory to write into, and that is a normal
//! state, not a failure. Provisioning re-materializes, so nothing chosen early
//! is lost.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::app_state::AppState;
use crate::managed_agents::atomic_write_json_restricted;
use crate::relay::relay_ws_url_with_override;
use crate::session_provider::env::PROJECTS_FILE_NAME;
use crate::session_provider::provider_state_dir;
use crate::session_provider::store::load_provider_store;
use crate::util::now_iso;

/// Current on-disk schema version of the desktop's own record.
pub(crate) const WORKDIR_STORE_VERSION: u32 = 1;

/// Schema version written into the provider's `projects.json`.
pub(crate) const PROJECTS_VIEW_VERSION: u32 = 1;

/// How many recently used directories are remembered for the create flow.
pub(crate) const MAX_MRU_ENTRIES: usize = 10;

/// Upper bound on one-shot create hints held at once.
///
/// A hint is cleared when its receipt arrives, so the steady state is near
/// zero. The cap only bounds the pathological case where receipts never come
/// back — an unbounded map would grow for the life of the install.
pub(crate) const MAX_PENDING_HINTS: usize = 64;

/// A remembered directory choice, with the moment it was last set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionWorkdirEntry {
    pub path: PathBuf,
    pub updated_at: String,
}

/// One recently used directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionWorkdirMruEntry {
    pub path: PathBuf,
    pub last_used_at: String,
}

/// The desktop's full working-directory record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionWorkdirStore {
    pub version: u32,
    /// Keyed by NIP-MP project coordinate (`30621:<owner>:<dtag>`).
    #[serde(default)]
    pub by_project: BTreeMap<String, CodingSessionWorkdirEntry>,
    /// Keyed by channel UUID. The fallback when no project is involved.
    #[serde(default)]
    pub by_channel: BTreeMap<String, CodingSessionWorkdirEntry>,
    /// Most-recently-used directories, newest first, capped at
    /// [`MAX_MRU_ENTRIES`].
    #[serde(default)]
    pub mru: Vec<CodingSessionWorkdirMruEntry>,
    /// One-shot hints keyed by the 44221 `commandId` they belong to.
    #[serde(default)]
    pub pending: BTreeMap<String, PathBuf>,
}

impl Default for CodingSessionWorkdirStore {
    fn default() -> Self {
        Self {
            version: WORKDIR_STORE_VERSION,
            by_project: BTreeMap::new(),
            by_channel: BTreeMap::new(),
            mru: Vec::new(),
            pending: BTreeMap::new(),
        }
    }
}

/// Which keyspace a directory choice belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CodingSessionWorkdirScope {
    Project,
    Channel,
}

/// What the provider reads: the minimum needed to resolve a cwd.
///
/// Mirrors `buzz_session_provider::commands::ProjectsFile`. Deliberately a
/// separate type from the store above — the provider must not inherit the
/// desktop's MRU or timestamps, which are UI memory, not resolution inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionProjectsView {
    pub version: u32,
    pub pending: BTreeMap<String, PathBuf>,
    pub projects: BTreeMap<String, PathBuf>,
    pub channels: BTreeMap<String, PathBuf>,
}

/// Whether a candidate path is usable as a working directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionWorkdirValidation {
    pub exists: bool,
    pub is_dir: bool,
    /// Absolute paths only. The provider treats anything else as unconfigured,
    /// so the picker says so up front rather than letting a create fail later.
    pub is_absolute: bool,
}

impl CodingSessionWorkdirStore {
    /// Record a directory for one scope key, replacing any previous choice.
    pub(crate) fn set(&mut self, scope: CodingSessionWorkdirScope, key: &str, path: PathBuf) {
        let entry = CodingSessionWorkdirEntry {
            path,
            updated_at: now_iso(),
        };
        match scope {
            CodingSessionWorkdirScope::Project => {
                self.by_project.insert(key.to_string(), entry);
            }
            CodingSessionWorkdirScope::Channel => {
                self.by_channel.insert(key.to_string(), entry);
            }
        }
    }

    /// Move a directory to the head of the MRU list.
    ///
    /// De-duplicates first so re-using a directory promotes it instead of
    /// filling the list with copies of itself.
    pub(crate) fn record_use(&mut self, path: PathBuf) {
        self.mru.retain(|entry| entry.path != path);
        self.mru.insert(
            0,
            CodingSessionWorkdirMruEntry {
                path,
                last_used_at: now_iso(),
            },
        );
        self.mru.truncate(MAX_MRU_ENTRIES);
    }

    /// Stage the one-shot hint a create command will resolve against.
    ///
    /// Over the cap the oldest key is dropped. `BTreeMap` order is by
    /// `commandId`, which is not insertion order — acceptable precisely
    /// because reaching the cap already means receipts stopped arriving, and
    /// the alternative is unbounded growth.
    pub(crate) fn stage_hint(&mut self, command_id: &str, path: PathBuf) {
        self.pending.insert(command_id.to_string(), path);
        while self.pending.len() > MAX_PENDING_HINTS {
            let Some(oldest) = self.pending.keys().next().cloned() else {
                break;
            };
            self.pending.remove(&oldest);
        }
    }

    /// Drop a hint once its receipt has been seen.
    pub(crate) fn clear_hint(&mut self, command_id: &str) {
        self.pending.remove(command_id);
    }

    /// Project the desktop record down to what the provider reads.
    pub(crate) fn projects_view(&self) -> CodingSessionProjectsView {
        CodingSessionProjectsView {
            version: PROJECTS_VIEW_VERSION,
            pending: self.pending.clone(),
            projects: self
                .by_project
                .iter()
                .map(|(key, entry)| (key.clone(), entry.path.clone()))
                .collect(),
            channels: self
                .by_channel
                .iter()
                .map(|(key, entry)| (key.clone(), entry.path.clone()))
                .collect(),
        }
    }
}

/// Inspect a candidate directory without touching it.
pub(crate) fn validate_workdir(path: &Path) -> CodingSessionWorkdirValidation {
    let metadata = std::fs::metadata(path);
    CodingSessionWorkdirValidation {
        exists: metadata.is_ok(),
        is_dir: metadata.map(|meta| meta.is_dir()).unwrap_or(false),
        is_absolute: path.is_absolute(),
    }
}

fn workdir_store_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("failed to resolve app config dir: {error}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create app config dir: {error}"))?;
    Ok(dir.join("coding-session-workdirs.json"))
}

/// Read the record, treating a missing file as the empty steady state.
pub(crate) fn load_workdir_store(app: &AppHandle) -> Result<CodingSessionWorkdirStore, String> {
    let path = workdir_store_path(app)?;
    if !path.exists() {
        return Ok(CodingSessionWorkdirStore::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read coding-session workdir store: {error}"))?;
    serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse coding-session workdir store: {error}"))
}

/// Persist the record and re-materialize the provider's view.
///
/// The two writes are one operation on purpose: a desktop record that has
/// drifted from the file the provider reads is the failure mode this whole
/// seam exists to avoid.
pub(crate) fn save_workdir_store(
    app: &AppHandle,
    state: &AppState,
    store: &CodingSessionWorkdirStore,
) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(store)
        .map_err(|error| format!("failed to serialize coding-session workdir store: {error}"))?;
    atomic_write_json_restricted(&workdir_store_path(app)?, &payload)?;
    materialize_projects_view(app, state, store)
}

/// Write `<state-dir>/projects.json` for the provisioned provider, if any.
///
/// Returns `Ok(())` when no provider exists yet: there is nowhere to write and
/// nothing is lost, because provisioning calls this again.
pub(crate) fn materialize_projects_view(
    app: &AppHandle,
    state: &AppState,
    store: &CodingSessionWorkdirStore,
) -> Result<(), String> {
    let relay_url = relay_ws_url_with_override(state);
    let provider_store = load_provider_store(app)?;
    let Some(record) = provider_store.get(&relay_url) else {
        return Ok(());
    };
    let state_dir = provider_state_dir(app, &record.provider_pubkey)?;
    let payload = serde_json::to_vec_pretty(&store.projects_view())
        .map_err(|error| format!("failed to serialize coding-session projects view: {error}"))?;
    atomic_write_json_restricted(&state_dir.join(PROJECTS_FILE_NAME), &payload)
}

/// Re-materialize from whatever the desktop currently remembers.
///
/// Called after provisioning so directories chosen before a provider existed
/// reach it the moment one does.
pub(crate) fn remateralize_provider_projects_view(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), String> {
    let store = load_workdir_store(app)?;
    materialize_projects_view(app, state, &store)
}

fn mutate<F>(
    app: &AppHandle,
    state: &AppState,
    apply: F,
) -> Result<CodingSessionWorkdirStore, String>
where
    F: FnOnce(&mut CodingSessionWorkdirStore),
{
    let mut store = load_workdir_store(app)?;
    apply(&mut store);
    store.version = WORKDIR_STORE_VERSION;
    save_workdir_store(app, state, &store)?;
    Ok(store)
}

/// Read the whole host-local working-directory record.
#[tauri::command]
pub fn get_coding_session_workdir_state(
    app: AppHandle,
) -> Result<CodingSessionWorkdirStore, String> {
    load_workdir_store(&app)
}

/// Remember a directory for a project coordinate or a channel.
#[tauri::command]
pub fn set_coding_session_workdir(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: CodingSessionWorkdirScope,
    key: String,
    path: String,
) -> Result<CodingSessionWorkdirStore, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("a working-directory scope key is required".to_string());
    }
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return Err("a coding-session working directory must be an absolute path".to_string());
    }
    mutate(&app, &state, |store| store.set(scope, &key, path))
}

/// Promote a directory to the head of the MRU list.
#[tauri::command]
pub fn record_coding_session_workdir_use(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<CodingSessionWorkdirStore, String> {
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return Err("a coding-session working directory must be an absolute path".to_string());
    }
    mutate(&app, &state, |store| store.record_use(path))
}

/// Stage the directory a specific create command should run in.
#[tauri::command]
pub fn stage_coding_session_create_hint(
    app: AppHandle,
    state: State<'_, AppState>,
    command_id: String,
    path: String,
) -> Result<CodingSessionWorkdirStore, String> {
    let command_id = command_id.trim().to_string();
    if command_id.is_empty() {
        return Err("a coding-session command id is required".to_string());
    }
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return Err("a coding-session working directory must be an absolute path".to_string());
    }
    mutate(&app, &state, |store| store.stage_hint(&command_id, path))
}

/// Drop a staged hint once its receipt has settled the create.
#[tauri::command]
pub fn clear_coding_session_create_hint(
    app: AppHandle,
    state: State<'_, AppState>,
    command_id: String,
) -> Result<CodingSessionWorkdirStore, String> {
    let command_id = command_id.trim().to_string();
    mutate(&app, &state, |store| store.clear_hint(&command_id))
}

/// Check a candidate directory before it is committed to anything.
#[tauri::command]
pub fn validate_coding_session_workdir(path: String) -> CodingSessionWorkdirValidation {
    validate_workdir(Path::new(path.trim()))
}

/// Open the OS folder picker, returning the chosen absolute path.
///
/// `tauri-plugin-dialog` is already a dependency and already granted by the
/// default capability, so this adds a native picker without widening the app's
/// plugin surface.
#[tauri::command]
pub async fn pick_coding_session_workdir(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a working directory")
        .pick_folder(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx
        .await
        .map_err(|_| "the folder picker closed unexpectedly".to_string())?;
    Ok(picked.map(|path| path.to_string()))
}
