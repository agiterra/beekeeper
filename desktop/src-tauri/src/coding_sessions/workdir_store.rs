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
use crate::session_provider::store::{
    load_provider_store, CodingSessionProviderRecord, CodingSessionProviderStore,
};
use crate::util::now_iso;

/// Current on-disk schema version of the desktop's own record.
///
/// Version 2 (L11) adds [`CodingSessionWorkdirStore::worktrees`]. Version 1
/// files still load: the map is `#[serde(default)]`, so a v1 record reads with
/// it empty and re-saves as v2. That empty map is the honest answer — the
/// worktrees cut before this record existed were never recorded and are never
/// removed by the host.
pub(crate) const WORKDIR_STORE_VERSION: u32 = 2;

/// Oldest on-disk schema version this build still reads.
pub(crate) const MIN_WORKDIR_STORE_VERSION: u32 = 1;

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

/// Upper bound on recorded seat worktrees held at once.
///
/// One per seat per session. The cap exists so a corrupted or hostile file
/// cannot make the host allocate without limit; the steady state is small,
/// because entries are dropped when their tree is removed.
pub(crate) const MAX_SEAT_WORKTREES: usize = 4096;

/// One git worktree this host cut for one seat, recorded when it was created.
///
/// Written **only** by the create path. Nothing that merely observed a
/// directory ever writes one of these: the whole point of the record is that
/// the host can name what it made, and a tree it did not make is a tree it
/// must not remove. Like every other field here, these paths name one
/// person's disk and are never published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionSeatWorktree {
    /// Absolute path of the worktree directory.
    pub path: PathBuf,
    /// Branch created with it, which shares the directory's slug.
    pub branch: String,
    /// Repository the worktree belongs to.
    pub repo_root: PathBuf,
    /// When the host cut it, ISO-8601.
    pub created_at: String,
}

/// The key one seat worktree is filed under: `<sessionRef>/<seatLabel>`.
///
/// A seat is unique inside its session, so this is the whole identity. It is
/// deliberately not the path: a path can be renamed out from under the host,
/// and then the record would silently name a directory nobody cut.
pub(crate) fn seat_worktree_key(session_ref: &str, seat_label: &str) -> String {
    format!("{}/{}", session_ref.trim(), seat_label.trim())
}

/// Whether `path` sits inside the one folder worktrees are allowed to live in.
///
/// `<repo_root>.worktrees/…` and nothing else. A record naming a directory
/// outside it would give the prune path a licence over somewhere it has no
/// business, so the write is refused rather than trusted.
pub(crate) fn is_inside_worktree_parent(repo_root: &Path, path: &Path) -> bool {
    let Some(file_name) = repo_root.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(parent) = repo_root.parent() else {
        return false;
    };
    let holder = parent.join(format!("{file_name}.worktrees"));
    path.is_absolute() && path.starts_with(&holder) && path != holder
}

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
    /// Worktrees this host cut, keyed by [`seat_worktree_key`].
    ///
    /// Unlike `pending`, this is **durable**: a one-shot hint is cleared the
    /// moment its receipt arrives, which is exactly why the host could not
    /// name a single one of the 65 trees on this machine. `#[serde(default)]`
    /// is what makes a v1 file readable.
    #[serde(default)]
    pub worktrees: BTreeMap<String, CodingSessionSeatWorktree>,
}

impl Default for CodingSessionWorkdirStore {
    fn default() -> Self {
        Self {
            version: WORKDIR_STORE_VERSION,
            by_project: BTreeMap::new(),
            by_channel: BTreeMap::new(),
            mru: Vec::new(),
            pending: BTreeMap::new(),
            worktrees: BTreeMap::new(),
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

    /// Forget the directory recorded for one scope key.
    pub(crate) fn clear(&mut self, scope: CodingSessionWorkdirScope, key: &str) {
        match scope {
            CodingSessionWorkdirScope::Project => {
                self.by_project.remove(key);
            }
            CodingSessionWorkdirScope::Channel => {
                self.by_channel.remove(key);
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

    /// Record a worktree this host has just cut for one seat.
    ///
    /// Refuses a path outside `<repo_root>.worktrees/`, and refuses an empty
    /// session ref or seat label — a record that cannot be trusted to name
    /// what the host made is worse than no record, because the prune path
    /// believes it.
    pub(crate) fn record_seat_worktree(
        &mut self,
        session_ref: &str,
        seat_label: &str,
        entry: CodingSessionSeatWorktree,
    ) -> Result<(), String> {
        if session_ref.trim().is_empty() || seat_label.trim().is_empty() {
            return Err("a seat worktree record needs a session ref and a seat label".to_string());
        }
        if !is_inside_worktree_parent(&entry.repo_root, &entry.path) {
            return Err(format!(
                "refusing to record a worktree outside the repository's worktrees folder: {}",
                entry.path.display()
            ));
        }
        if self.worktrees.len() >= MAX_SEAT_WORKTREES
            && !self
                .worktrees
                .contains_key(&seat_worktree_key(session_ref, seat_label))
        {
            return Err("this host already records the maximum number of seat worktrees".into());
        }
        self.worktrees
            .insert(seat_worktree_key(session_ref, seat_label), entry);
        Ok(())
    }

    /// Drop the record for one seat worktree, after its directory is gone.
    ///
    /// Answers whether there was one, so a caller never reports removing a
    /// record it did not hold.
    pub(crate) fn forget_seat_worktree(&mut self, key: &str) -> bool {
        self.worktrees.remove(key).is_some()
    }

    /// Project the desktop record down to what the provider reads.
    ///
    /// `worktrees` is deliberately **not** here. The provider resolves a cwd;
    /// it does not reap, and handing it a list of directories it may not touch
    /// would only invite something to try.
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

/// Resolve the desktop workdir store without creating its parent directory.
pub(crate) fn workdir_store_path_readonly(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|error| format!("failed to resolve app config dir: {error}"))?
        .join("coding-session-workdirs.json"))
}

/// Read an explicit workdir store path without mutating the filesystem.
pub(crate) fn load_workdir_store_readonly_from(
    path: &Path,
) -> Result<CodingSessionWorkdirStore, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CodingSessionWorkdirStore::default());
        }
        Err(error) => {
            return Err(format!(
                "failed to read coding-session workdir store: {error}"
            ));
        }
    };
    if file
        .metadata()
        .map_err(|error| format!("failed to inspect coding-session workdir store: {error}"))?
        .len()
        > 1024 * 1024
    {
        return Err("coding-session workdir store exceeds readiness limit".into());
    }
    let store: CodingSessionWorkdirStore = serde_json::from_reader(file)
        .map_err(|error| format!("failed to parse coding-session workdir store: {error}"))?;
    // A range, not an equality: a v1 file predates the worktree record and
    // reads with that map empty, which is exactly true of it.
    if store.version < MIN_WORKDIR_STORE_VERSION || store.version > WORKDIR_STORE_VERSION {
        return Err(format!(
            "unsupported coding-session workdir store version: {}",
            store.version
        ));
    }
    if store.by_project.len() > 4096
        || store.by_channel.len() > 4096
        || store.mru.len() > MAX_MRU_ENTRIES
        || store.pending.len() > MAX_PENDING_HINTS
        || store.worktrees.len() > MAX_SEAT_WORKTREES
    {
        return Err("coding-session workdir store exceeds readiness record limits".into());
    }
    let paths = store
        .by_project
        .values()
        .chain(store.by_channel.values())
        .map(|entry| &entry.path)
        .chain(store.mru.iter().map(|entry| &entry.path))
        .chain(store.pending.values())
        .chain(store.worktrees.values().map(|entry| &entry.path))
        .chain(store.worktrees.values().map(|entry| &entry.repo_root));
    if paths.into_iter().any(|path| !path.is_absolute()) {
        return Err("coding-session workdir store contains a relative path".into());
    }
    Ok(store)
}

/// Read the host-local project checkout inventory. This path never creates the
/// config directory and never materializes a provider view.
pub(crate) fn load_workdir_store_readonly(
    app: &AppHandle,
) -> Result<CodingSessionWorkdirStore, String> {
    load_workdir_store_readonly_from(&workdir_store_path_readonly(app)?)
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
    materialize_projects_view_for_relay(app, &relay_url, store)
}

/// Write the provider view for one caller-pinned relay.
///
/// Provisioning crosses a lock boundary before it reaches this write. Passing
/// the relay captured under that lock prevents a later community switch from
/// redirecting the projects view to another provider identity.
fn materialize_projects_view_for_relay(
    app: &AppHandle,
    relay_url: &str,
    store: &CodingSessionWorkdirStore,
) -> Result<(), String> {
    let provider_store = load_provider_store(app)?;
    let Some(record) = projects_view_provider_for_relay(&provider_store, relay_url) else {
        return Ok(());
    };
    let state_dir = provider_state_dir(app, &record.provider_pubkey)?;
    let payload = serde_json::to_vec_pretty(&store.projects_view())
        .map_err(|error| format!("failed to serialize coding-session projects view: {error}"))?;
    atomic_write_json_restricted(&state_dir.join(PROJECTS_FILE_NAME), &payload)
}

/// Re-materialize what the desktop remembers for one caller-pinned relay.
///
/// Called after provisioning so directories chosen before a provider existed
/// reach it the moment one does. The relay is deliberately not re-read here:
/// provisioning captured it while holding its serialization lock.
pub(crate) fn remateralize_provider_projects_view(
    app: &AppHandle,
    relay_url: &str,
) -> Result<(), String> {
    let store = load_workdir_store(app)?;
    materialize_projects_view_for_relay(app, relay_url, &store)
}

pub(super) fn projects_view_provider_for_relay<'a>(
    provider_store: &'a CodingSessionProviderStore,
    relay_url: &str,
) -> Option<&'a CodingSessionProviderRecord> {
    provider_store.get(relay_url)
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

/// Forget the directory remembered for a project coordinate or a channel.
///
/// Goes through `mutate` like `set` does, so the provider's projects view is
/// re-materialized without the entry.
#[tauri::command]
pub fn clear_coding_session_workdir(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: CodingSessionWorkdirScope,
    key: String,
) -> Result<CodingSessionWorkdirStore, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("a working-directory scope key is required".to_string());
    }
    mutate(&app, &state, |store| store.clear(scope, &key))
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
