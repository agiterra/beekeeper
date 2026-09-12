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

#[path = "workdir_store_lock.rs"]
mod lock;
pub(crate) use lock::lock_workdir_store;

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

/// Upper bound on remembered prunes.
///
/// A prune record outlives the directory it names, so unlike the worktree map
/// nothing ever drops one on its own. The cap is what bounds it.
pub(crate) const MAX_PRUNED_WORKTREES: usize = 512;

/// The key prefix a hint migrated out of `pending` is filed under.
///
/// Deliberately not a session ref, and deliberately not spellable as one: a
/// migrated hint names a directory this host cut but can no longer attribute
/// to a session, so it must be *visible* to `bee sessions worktree status`
/// without ever being matched by a real session's prefix — which is what
/// keeps it listable and unprunable. See [`migrate_pending_worktrees`].
pub(crate) const MIGRATED_HINT_PREFIX: &str = "unattributed-hint:";

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
    /// The producer-minted session id running in this tree, when the caller
    /// knew one.
    ///
    /// This is the whole of finding 82's fix on the host side. The provider
    /// resolves a gate row's directory by re-reading its projects file at
    /// every gate ([`CodingSessionProjectsView::sessions`]); that map is built
    /// from this field, so the moment the host records a tree at a new path
    /// the next gate is measured there. Without it the provider can only know
    /// the path the create resolved, which after a relocation is a directory
    /// that no longer exists — and every row it minted said
    /// `headSha: null, dirty: null`.
    ///
    /// `#[serde(default)]` and optional: a record written before this field,
    /// or by a caller that genuinely does not know the session id yet (a tree
    /// cut before its genesis is signed), reads as `None` and simply
    /// contributes no override, which is exactly today's behaviour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// One worktree this host removed, and why it was allowed to.
///
/// Kept after the directory is gone so a person can find out what happened to
/// a folder they remember. Bounded by [`MAX_PRUNED_WORKTREES`]; the oldest
/// entries fall off the end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionPrunedWorktree {
    /// Absolute path that was removed.
    pub path: PathBuf,
    /// Branch it had checked out.
    pub branch: String,
    /// Repository it belonged to.
    pub repo_root: PathBuf,
    /// When the host removed it, ISO-8601.
    pub pruned_at: String,
    /// The sentence the host would have shown for the disposition that
    /// admitted the removal — never a bare token.
    pub reason: String,
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
    // The rule itself lives in `buzz_core::worktree_placement`, shared with
    // the prune guard and with `bee`. Three guards that disagree with where
    // placement actually cuts is a silent, total failure: the tree lands
    // somewhere no guard admits, so it can never be recorded or removed.
    buzz_core_pkg::worktree_placement::is_managed_worktree_path(repo_root, path, &[])
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
    /// Worktree folders a person chose, keyed by canonical repository root.
    ///
    /// Keyed by the *repository*, not the working directory, which is the
    /// whole point of resolving through `--git-common-dir`: the same
    /// repository reached from a subdirectory, a linked worktree, or the bare
    /// folder must get the same answer. Additive and `#[serde(default)]`, so
    /// a v2 file reads with it empty — deliberately **not** a version bump,
    /// because `bee` hard-errors on a version above its own maximum and would
    /// stop working on every machine that opened the app once.
    #[serde(default)]
    pub worktree_parents: BTreeMap<String, PathBuf>,
    /// Worktrees this host removed, newest last, keyed by the record's key.
    ///
    /// Additive and `#[serde(default)]` for the same reason `worktrees` was:
    /// `bee` hard-errors on a store version above its own maximum, so this is
    /// **not** a version bump. A build that predates the map reads a file
    /// carrying one and simply does not show it.
    #[serde(default)]
    pub pruned: BTreeMap<String, CodingSessionPrunedWorktree>,
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
            worktree_parents: BTreeMap::new(),
            pruned: BTreeMap::new(),
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
    /// Where each running session's tree is **right now**, by session id.
    ///
    /// The one addition the provider's gate probe reads
    /// (`crates/buzz-session-provider/src/gate_cwd.rs`). Unlike the three maps
    /// above it is not a resolution input for a *create* — the provider never
    /// consults it to choose a directory — it is the host telling a session
    /// that already exists where its tree moved to. Rewritten on every
    /// mutation of this store, so a relocation reaches a live session at its
    /// next gate with no restart and no new command.
    ///
    /// Additive: a provider that predates it ignores the key, and a host that
    /// predates it writes no key, which reads as no override at all.
    pub sessions: BTreeMap<String, PathBuf>,
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

    /// Stage a create's one-shot hint and, when the create is project-scoped
    /// and the project has no directory of its own yet, remember its checkout.
    /// `remember_path` separates that checkout from an execution worktree;
    /// omitted callers retain the legacy behavior of remembering `path`.
    ///
    /// A hint dies with its receipt, so before this a project's directory
    /// existed only in the desktop's project settings — which nothing
    /// prompted anyone to open. The first create from the desktop is the one
    /// moment the host knows which tree the project lives in, and a phone
    /// create for that project (live finding 2026-09-08, refused with
    /// `PROJECT_CWD_UNRESOLVED`) can only ever resolve through
    /// `projects[projectRef]`. A directory already recorded for the project
    /// is left alone: settings win over a one-off choice.
    pub(crate) fn stage_hint_for_project(
        &mut self,
        command_id: &str,
        project_ref: Option<&str>,
        path: PathBuf,
        remember_path: Option<PathBuf>,
    ) {
        let project_ref = project_ref.map(str::trim).filter(|key| !key.is_empty());
        if let Some(project_ref) = project_ref {
            if !self.by_project.contains_key(project_ref) {
                self.set(
                    CodingSessionWorkdirScope::Project,
                    project_ref,
                    remember_path.unwrap_or_else(|| path.clone()),
                );
            }
        }
        self.stage_hint(command_id, path);
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

    /// Remember that this host removed one recorded worktree, and why.
    ///
    /// Called only after the directory is actually gone, so the record is a
    /// fact rather than an intention. Over the cap the oldest `prunedAt` is
    /// dropped — the newest prune is the one somebody is asking about.
    pub(crate) fn record_prune(
        &mut self,
        key: &str,
        entry: &CodingSessionSeatWorktree,
        reason: &str,
    ) {
        self.pruned.insert(
            key.to_string(),
            CodingSessionPrunedWorktree {
                path: entry.path.clone(),
                branch: entry.branch.clone(),
                repo_root: entry.repo_root.clone(),
                pruned_at: now_iso(),
                reason: reason.to_string(),
            },
        );
        while self.pruned.len() > MAX_PRUNED_WORKTREES {
            let Some(oldest) = self
                .pruned
                .iter()
                .min_by(|left, right| left.1.pruned_at.cmp(&right.1.pruned_at))
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.pruned.remove(&oldest);
        }
    }

    /// Project the desktop record down to what the provider reads.
    ///
    /// `worktrees` is deliberately **not** here in full. The provider resolves
    /// a cwd; it does not reap, and handing it a list of directories it may
    /// not touch would only invite something to try. What it *does* get is
    /// [`CodingSessionProjectsView::sessions`]: the path of the tree each
    /// running session is in, and nothing else about it — no branch, no
    /// repository, no record it could act on.
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
            sessions: self
                .worktrees
                .values()
                .filter_map(|entry| {
                    entry
                        .session_id
                        .as_ref()
                        .map(|id| (id.clone(), entry.path.clone()))
                })
                .collect(),
        }
    }
}

/// Repository root implied by a worktree path, from the path alone.
///
/// The inverse of the two *holder* shapes in
/// `buzz_core::worktree_placement`: `<repo>/.worktrees/<slug>` and the legacy
/// sibling container `<repo>.worktrees/<slug>`. Both name their repository
/// unambiguously, so no `git` invocation and no `stat` is needed.
///
/// The per-worktree sibling shape `<stem>-wt-<slug>` is deliberately **not**
/// inverted: `a-wt-b-wt-c` has two readings and only a stat could choose
/// between them, so a hint in that shape is left alone rather than attributed
/// to a repository that may not be its own. Say so rather than guess.
fn repo_root_of_holder_path(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors().skip(1) {
        let name = ancestor.file_name()?.to_str()?;
        if name == ".worktrees" {
            return ancestor.parent().map(Path::to_path_buf);
        }
        if let Some(stem) = name.strip_suffix(".worktrees") {
            if stem.is_empty() {
                return None;
            }
            return Some(ancestor.with_file_name(stem));
        }
    }
    None
}

/// Move the seat worktrees stranded in `pending` into `worktrees`, once.
///
/// # Why anything needs moving (live-run finding 60)
///
/// `pending` is a **one-shot create hint**, cleared the moment its receipt
/// arrives. `worktrees` is the durable record `bee sessions worktree
/// status/prune/reclaim` and Pulse's disk row read. On the machine that
/// produced the finding, `pending` held 27 seat worktrees and `worktrees` was
/// empty: the host staged every seat into the hint map and never promoted one,
/// so the product's own reclaim was blind to every tree it had cut.
///
/// # What is and is not migrated
///
/// Only a hint whose path is inside a `.worktrees` holder of the repository
/// that path itself names, checked with the same shared predicate the record
/// and prune guards use. That is what keeps a person's ordinary checkout —
/// which is also staged as a hint, on every non-worktree create — from being
/// recorded as a seat's disposable tree.
///
/// A migrated entry is keyed [`MIGRATED_HINT_PREFIX`]`<commandId>/<basename>`.
/// The prefix is load-bearing: `bee` reads the whole map so the tree becomes
/// *visible*, while no real session ref can equal the key's session half, so
/// no settlement fact ever attaches to it and `classify_seat_worktree` answers
/// `not-settled` — listable, never removable. Finding 60 asked for the trees
/// to stop being invisible, not for a sweep to start deleting them.
///
/// Idempotent: the key is derived, and an existing key is never overwritten,
/// so a second load changes nothing. The hint itself is left in `pending` —
/// it may still be steering an in-flight create, and clearing it here would
/// break that create for a record it has already made.
pub(crate) fn migrate_pending_worktrees(store: &mut CodingSessionWorkdirStore) -> usize {
    let candidates: Vec<(String, PathBuf)> = store
        .pending
        .iter()
        .map(|(command_id, path)| (command_id.clone(), path.clone()))
        .collect();
    let mut migrated = 0usize;
    for (command_id, path) in candidates {
        let Some(repo_root) = repo_root_of_holder_path(&path) else {
            continue;
        };
        if !is_inside_worktree_parent(&repo_root, &path) {
            continue;
        }
        let Some(label) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let key = format!("{MIGRATED_HINT_PREFIX}{command_id}/{label}");
        if store.worktrees.contains_key(&key) {
            continue;
        }
        if store.worktrees.len() >= MAX_SEAT_WORKTREES {
            break;
        }
        // The branch is not knowable from a path, and inventing one would put
        // a name into a record whose purpose is naming what may be removed.
        // The empty string is the honest answer and reads as "unknown" in
        // every surface, all of which refuse to remove this entry anyway.
        store.worktrees.insert(
            key,
            CodingSessionSeatWorktree {
                path,
                branch: String::new(),
                repo_root,
                created_at: now_iso(),
                session_id: None,
            },
        );
        migrated += 1;
    }
    migrated
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
    let mut store: CodingSessionWorkdirStore = serde_json::from_reader(file)
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
        || store.worktree_parents.len() > 4096
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
        .chain(store.worktrees.values().map(|entry| &entry.repo_root))
        .chain(store.worktree_parents.values());
    if paths.into_iter().any(|path| !path.is_absolute()) {
        return Err("coding-session workdir store contains a relative path".into());
    }
    // In memory only: this reader promises not to touch the filesystem. The
    // next mutation persists the same derivation, because `mutate` loads
    // through the writable reader below, which migrates too.
    migrate_pending_worktrees(&mut store);
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
    let mut store: CodingSessionWorkdirStore = serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse coding-session workdir store: {error}"))?;
    // Finding 60: the seat worktrees stranded in `pending` become records the
    // reclaim tool can see. Idempotent, so every load may run it; it is
    // written back by the next `mutate`.
    migrate_pending_worktrees(&mut store);
    Ok(store)
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
    let _lock = lock_workdir_store(app)?;
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
    let _lock = lock_workdir_store(app)?;
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
///
/// With `project_ref`, `remember_path` (or `path` when omitted) becomes the
/// project's default when it has none yet — see
/// [`CodingSessionWorkdirStore::stage_hint_for_project`].
#[tauri::command]
pub fn stage_coding_session_create_hint(
    app: AppHandle,
    state: State<'_, AppState>,
    command_id: String,
    path: String,
    project_ref: Option<String>,
    remember_path: Option<String>,
) -> Result<CodingSessionWorkdirStore, String> {
    let command_id = command_id.trim().to_string();
    if command_id.is_empty() {
        return Err("a coding-session command id is required".to_string());
    }
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return Err("a coding-session working directory must be an absolute path".to_string());
    }
    let remember_path = remember_path.map(|path| PathBuf::from(path.trim()));
    if remember_path
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err("a remembered project directory must be an absolute path".to_string());
    }
    mutate(&app, &state, |store| {
        store.stage_hint_for_project(&command_id, project_ref.as_deref(), path, remember_path)
    })
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

/// Stage one command's directory for a captured relay, without changing defaults.
/// Callers serialize their launch and validate the owner/relay before invoking it.
pub(crate) fn stage_coding_session_create_hint_at(
    app: &AppHandle,
    relay_url: &str,
    command_id: &str,
    path: &Path,
) -> Result<(), String> {
    if command_id.trim().is_empty() || !path.is_absolute() {
        return Err("a command id and an absolute working directory are required".to_string());
    }
    let _lock = lock_workdir_store(app)?;
    let mut store = load_workdir_store(app)?;
    store.stage_hint(command_id, path.to_path_buf());
    store.version = WORKDIR_STORE_VERSION;
    let payload = serde_json::to_vec_pretty(&store)
        .map_err(|error| format!("failed to serialize coding-session workdir store: {error}"))?;
    atomic_write_json_restricted(&workdir_store_path(app)?, &payload)?;
    materialize_projects_view_for_relay(app, relay_url, &store)
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
