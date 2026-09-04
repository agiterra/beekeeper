//! One nest per agent, so a pack has somewhere to land.
//!
//! Every managed agent used to spawn in the same directory: the nest
//! (`~/.beekeeper`, or `~/.beekeeper-dev` on a dev build), or the operator's
//! own home when that nest is missing or is a symlink. That directory belongs
//! to no single agent, so [`super::nest::materialize_persona_skills`] refuses
//! to write a pack's skills into it — one persona's `brief/SKILL.md` would
//! overwrite another's, and in the `$HOME` case it would overwrite a person's
//! own. The refusal is right; the shared workdir was the bug (finding 68: every
//! seat spawn on run 5 printed the refusal and then ran with no skills).
//!
//! An agent's own nest is `<app data dir>/agents/nests/<first 8 hex of its
//! pubkey>/`. It is a full nest — the same `AGENTS.md`, the same subdirectories
//! and the same `buzz-cli` skill [`super::nest::ensure_nest_at`] writes — with
//! `REPOS` symlinked at the shared nest's `REPOS`, so agents keep sharing
//! checkouts while each keeps its own `.agents/skills`.
//!
//! # Nothing is migrated silently
//!
//! An agent already on this computer keeps the shared home it has been running
//! in, with everything it accumulated there. [`SHARED_HOME_FILE`] is the list
//! of exactly those agents: it is written **once**, at the first boot that
//! finds it absent, from the agents then in the store ([`seed_shared_home`]).
//! After that, an agent missing from the list is an agent that did not exist
//! when nests arrived, and it gets one of its own.
//!
//! Fail-closed in both directions that matter: while the list cannot be read
//! *every* agent is treated as shared, so a disk error can never quietly move
//! an agent out of the directory its work is in; and the seed refuses to
//! overwrite an existing list, so a later boot cannot re-capture the agents an
//! operator has since moved.
//!
//! # Why not a field on the record
//!
//! An agent's home is host-local, like `persona_team_dir` and the packs cache:
//! it names a directory on **this** computer. `ManagedAgentRecord` is projected
//! into kind:30177 events and into agent snapshots that travel to other
//! machines, where a path from here means nothing — `agent_snapshot` already
//! excludes `persona_team_dir` for exactly that reason. Keeping the home beside
//! the nests keeps a host-local fact host-local.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::types::ManagedAgentRecord;

/// Directory under the app data dir that holds every agent's own nest.
///
/// `agents/nests`, not `agents`: `<app data dir>/agents` is already the
/// managed-agent *store* — `managed-agents.json` and its backups, `teams.json`,
/// `logs/`, `agent-pids/`, `retention/` (`storage.rs:40`). Nests keep their own
/// subdirectory rather than sitting among those files, where a future sweep
/// over the store directory would have to know which entries are agents' homes.
pub(crate) const AGENT_NESTS_DIR: &str = "agents/nests";

/// The agents that were already living in the shared home when nests arrived.
pub(crate) const SHARED_HOME_FILE: &str = "shared-home.json";

/// How many hex characters of an agent's pubkey name its nest.
///
/// Eight is what every other surface in this app abbreviates a key to, so an
/// operator reading a path recognises the agent without a lookup.
const NEST_KEY_PREFIX_LEN: usize = 8;

/// Which directory a managed agent's process runs in, and therefore where its
/// pack's skills can be written.
///
/// The two are one question: `materialize_persona_skills` writes a persona's
/// skills into the agent's working directory and refuses a directory every
/// agent shares, so an agent in the shared home runs with no role skills at all
/// (finding 68).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHome {
    /// The nest every agent on this computer shares, falling back to the
    /// operator's own home directory when that nest is absent or is a symlink.
    /// The answer for every agent named in [`SHARED_HOME_FILE`], and the
    /// fail-closed answer while that file cannot be read.
    #[default]
    Shared,
    /// This agent's own nest under the app data directory.
    Nest,
}

/// The on-disk shape of [`SHARED_HOME_FILE`].
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedHomeFile {
    /// When the list was captured, for an operator reading the file later.
    seeded_at: String,
    /// Pubkeys of the agents that were in the store at that moment.
    agents: BTreeSet<String>,
}

/// The directory name an agent's nest takes, or `None` when `pubkey` is not a
/// 64-character lowercase-hex Nostr public key.
///
/// The pubkey becomes a path component, so it is validated rather than trusted:
/// a record hand-edited to carry `../..` must not point a `create_dir_all` at
/// somewhere else on the disk.
pub fn agent_nest_name(pubkey: &str) -> Option<String> {
    let pubkey = pubkey.trim();
    if pubkey.len() != 64 || !pubkey.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if pubkey.bytes().any(|b| b.is_ascii_uppercase()) {
        return None;
    }
    Some(pubkey[..NEST_KEY_PREFIX_LEN].to_string())
}

/// `<app data dir>/agents/nests` — the root every per-agent nest sits under.
///
/// Created if absent. Under the app data directory rather than under the shared
/// nest, so a per-agent nest is never itself inside the directory it exists to
/// stay out of.
pub fn agent_nests_root(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?
        .join(AGENT_NESTS_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create {}: {error}", dir.display()))?;
    Ok(dir)
}

/// Where this agent's own nest is, whether or not it exists yet.
pub fn agent_nest_dir(app: &AppHandle, pubkey: &str) -> Result<PathBuf, String> {
    let name = agent_nest_name(pubkey)
        .ok_or_else(|| "agent pubkey is not a 64-character lowercase hex key".to_string())?;
    Ok(agent_nests_root(app)?.join(name))
}

/// Create an agent's own nest at `root`, sharing `shared_repos` if given.
///
/// Idempotent, like [`super::nest::ensure_nest_at`], which it delegates the
/// scaffolding to. The `REPOS` symlink is created *first*, because
/// `ensure_nest_at` provisions a real `REPOS` directory for any nest that has
/// none and then leaves an existing symlink alone.
///
/// A failure to link `REPOS` is reported, not fatal: the agent gets a nest with
/// its own empty `REPOS`, which is worse than sharing the operator's checkouts
/// but is not a reason to refuse it a home.
pub fn ensure_agent_nest_at(root: &Path, shared_repos: Option<&Path>) -> Result<(), String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("failed to create {}: {error}", root.display()))?;
    if let Some(repos) = shared_repos {
        let link = root.join("REPOS");
        if link.symlink_metadata().is_err() {
            if let Err(error) = crate::util::create_symlink(repos, &link) {
                tracing::warn!(
                    nest = %root.display(),
                    repos = %repos.display(),
                    %error,
                    "agent nest: could not share the nest's REPOS; this agent gets its own"
                );
            }
        }
    }
    super::nest::ensure_nest_at(root)
}

/// The shared nest's `REPOS`, when there is a shared nest to share it from.
fn shared_repos_dir() -> Option<PathBuf> {
    super::nest::nest_dir()
        .map(|nest| nest.join("REPOS"))
        .filter(|repos| repos.symlink_metadata().is_ok())
}

/// Create (if needed) and return this agent's own nest.
pub fn ensure_agent_nest(app: &AppHandle, pubkey: &str) -> Result<PathBuf, String> {
    let root = agent_nest_dir(app, pubkey)?;
    ensure_agent_nest_at(&root, shared_repos_dir().as_deref())?;
    Ok(root)
}

/// Read the shared-home list, or `None` when there is not one to read.
///
/// `None` is the fail-closed answer: absent, unreadable and malformed all mean
/// "this computer has not established which agents were already here", and
/// [`agent_home`] then leaves every agent exactly where it is.
fn read_shared_home_at(path: &Path) -> Option<BTreeSet<String>> {
    let raw = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<SharedHomeFile>(&raw) {
        Ok(file) => Some(file.agents),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "agent nest: the shared-home list could not be read; \
                 every agent keeps the home it has until it can be"
            );
            None
        }
    }
}

/// Capture, once, the agents that were already living in the shared home.
///
/// Writes nothing when the list already exists: re-capturing it would pull back
/// every agent the operator has since given a nest. Returns `true` when this
/// call is the one that wrote it.
pub fn seed_shared_home_at(path: &Path, pubkeys: &BTreeSet<String>) -> Result<bool, String> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let file = SharedHomeFile {
        seeded_at: crate::util::now_iso(),
        agents: pubkeys.clone(),
    };
    let json = serde_json::to_string_pretty(&file)
        .map_err(|error| format!("failed to serialize the shared-home list: {error}"))?;
    std::fs::write(path, json)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(true)
}

/// [`seed_shared_home_at`] for the running app, from the store on disk.
///
/// Called once at boot. Best-effort: a failure is reported and leaves every
/// agent in the shared home, which is where they already are.
pub fn seed_shared_home(app: &AppHandle) {
    let result = (|| -> Result<bool, String> {
        let path = agent_nests_root(app)?.join(SHARED_HOME_FILE);
        let pubkeys = super::load_managed_agents(app)?
            .into_iter()
            .filter(|record| !record.pubkey.trim().is_empty())
            .map(|record| record.pubkey)
            .collect();
        seed_shared_home_at(&path, &pubkeys)
    })();
    match result {
        Ok(true) => tracing::info!(
            "agent nest: recorded the agents already living in the shared home; \
             agents minted from now on get a nest of their own"
        ),
        Ok(false) => {}
        Err(error) => tracing::warn!(
            %error,
            "agent nest: could not record which agents share this computer's home; \
             every agent keeps the home it has"
        ),
    }
}

/// Which directory this agent runs in.
///
/// [`AgentHome::Shared`] for an agent named in the shared-home list, and for
/// every agent while that list cannot be read. [`AgentHome::Nest`] for an agent
/// that was not on this computer when nests arrived.
pub fn agent_home(app: &AppHandle, record: &ManagedAgentRecord) -> AgentHome {
    let listed = agent_nests_root(app)
        .ok()
        .map(|root| root.join(SHARED_HOME_FILE))
        .and_then(|path| read_shared_home_at(&path));
    match listed {
        Some(shared) if !shared.contains(&record.pubkey) => AgentHome::Nest,
        // Either this agent is on the list, or there is no list yet. Both mean
        // leave it where it is.
        _ => AgentHome::Shared,
    }
}

/// The directory this agent's process runs in, and the directory its pack's
/// skills are written into.
///
/// `None` — inherit the parent's working directory — only when neither a nest
/// nor the shared workdir can be resolved, which is
/// [`super::default_agent_workdir`]'s existing sandboxed-environment answer. A
/// nest that cannot be created is reported and falls back to the shared
/// workdir: an agent that ran yesterday still runs today.
pub fn agent_workdir(app: &AppHandle, record: &ManagedAgentRecord) -> Option<PathBuf> {
    match agent_home(app, record) {
        AgentHome::Shared => super::default_agent_workdir(),
        AgentHome::Nest => match ensure_agent_nest(app, &record.pubkey) {
            Ok(nest) => Some(nest),
            Err(error) => {
                tracing::warn!(
                    agent = %record.name,
                    pubkey = %record.pubkey,
                    %error,
                    "agent nest: could not create this agent's own nest; \
                     falling back to the shared workdir, where its pack will be refused"
                );
                super::default_agent_workdir()
            }
        },
    }
}

/// Take this agent off the shared-home list, so its next spawn is in its own
/// nest.
///
/// A no-op for an agent that is not on the list.
pub fn leave_shared_home_at(path: &Path, pubkey: &str) -> Result<(), String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut file: SharedHomeFile = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    if !file.agents.remove(pubkey) {
        return Ok(());
    }
    let json = serde_json::to_string_pretty(&file)
        .map_err(|error| format!("failed to serialize the shared-home list: {error}"))?;
    std::fs::write(path, json)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

/// [`leave_shared_home_at`] for the running app.
///
/// No list means no agent is being held in the shared home, so there is nothing
/// to take this one off — the nest its caller just created is already the
/// answer, and writing a list here would name the wrong agents.
pub fn leave_shared_home(app: &AppHandle, pubkey: &str) -> Result<(), String> {
    let path = agent_nests_root(app)?.join(SHARED_HOME_FILE);
    if !path.exists() {
        return Ok(());
    }
    leave_shared_home_at(&path, pubkey)
}

/// Does this agent have a pack that its shared home is refusing?
///
/// Exactly the condition [`super::nest::materialize_persona_skills`] refuses
/// on: a record that carries a pack link, running in the shared home. An agent
/// with no pack loses nothing to a shared home and must not be labelled as if
/// it did.
pub fn pack_refused_by_shared_home(app: &AppHandle, record: &ManagedAgentRecord) -> bool {
    record.persona_team_dir.is_some()
        && record.persona_name_in_team.is_some()
        && agent_home(app, record) == AgentHome::Shared
}

#[cfg(test)]
mod tests;
