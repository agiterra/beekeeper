//! Canonical executor-local project and authorized agents-repository materialization.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Where this host keeps its clone of a project's agents repository, and the
/// ref the project pins. Written by authorized local management into the projects file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsRepoRecord {
    /// Absolute path of the clone.
    pub path: PathBuf,
    /// The pinned ref, fully qualified (`refs/heads/main`).
    #[serde(rename = "ref")]
    pub ref_name: String,
    /// The repository's clone URL on the relay. `None` in a record written
    /// before ledger 250; the fetch then falls back to an `origin` remote,
    /// and without one refuses by name rather than reading an old tip as
    /// current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Host-local map from session coordinates to working directories.
///
/// This file is the entire seam between signed intent and the machine a session
/// actually runs on. It is written by authorized local management, re-read on every
/// lifecycle command (so an operator can fix a missing entry and republish
/// without restarting the provider), and never travels anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectsFile {
    /// Format version. Reserved; unknown versions are still read best-effort.
    pub version: u32,
    /// One-shot hint keyed by `commandId`, for a directory chosen at create time.
    pub pending: BTreeMap<String, PathBuf>,
    /// The project each `pending` hint binds its directory to, by
    /// `commandId`. Additive: a host that predates it writes none, and its
    /// hints are then accepted for a project only when the directory is the
    /// project's recorded checkout or a linked worktree of it.
    pub pending_projects: BTreeMap<String, String>,
    /// Directory per NIP-MP project coordinate.
    pub projects: BTreeMap<String, PathBuf>,
    /// Fallback directory per channel.
    pub channels: BTreeMap<Uuid, PathBuf>,
    /// This host's clone of each project's agents repository and the ref
    /// its kind:30624 pins (spec § 4.11), by project coordinate. Where
    /// `actions.yml` and `team.yml` are read from — never from `projects`.
    pub agents_repos: BTreeMap<String, AgentsRepoRecord>,
    /// Directory holding one-shot per-command hint files, beside the projects
    /// file itself. Never part of the file's own JSON.
    #[serde(skip)]
    pub hints_dir: Option<PathBuf>,
}

/// Name of the directory holding one-shot create hints, beside the projects
/// file.
///
/// It exists because `pending[commandId]` is not a durable place to leave one.
/// The desktop rematerializes the whole projects file from its canonical store
/// whenever anything unrelated is saved, which erases a `pending` entry the CLI
/// wrote moments earlier and before the create is admitted — a race whose only
/// symptom is a session that ran in the wrong directory. A separate directory
/// the rematerialization never touches removes the race rather than narrowing
/// it.
pub const PENDING_HINTS_DIR: &str = "pending-hints";

/// One `pending-hints/<commandId>.json` file.
///
/// `writtenAt` is not consulted for resolution — a hint is either the one this
/// command names or it is nothing — but it is what lets the provider prune
/// files whose create never arrived, so an abandoned hint cannot sit on disk
/// naming a directory forever.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingHint {
    /// The create's `commandId`. Must match the file name's stem, or the file
    /// is ignored: a hint that disagrees with its own name is not evidence
    /// about either command.
    pub command_id: String,
    /// Absolute path of the directory the session should run in.
    pub path: PathBuf,
    /// The project the writer bound this directory to, when it named one.
    #[serde(default)]
    pub project_ref: Option<String>,
    /// When the CLI wrote it, Unix seconds.
    #[serde(default)]
    pub written_at: u64,
}

/// The marker a consumed hint leaves behind.
///
/// A hint is **not deleted** when it is spent, and the difference matters.
/// Deleting it makes "this command's directory was already decided" look
/// exactly like "this command never had a hint", so a create that arrives late
/// — a replay, or the same durable create after a lost state file — would
/// resolve to the project or channel default and run the work in a different
/// folder than the one it ran in the first time. Silently. The marker records
/// what was decided so that case is answered by name instead.
///
/// `sessionId` is optional so the shape the rename leaves behind mid-crash (a
/// `.consumed` file still holding the [`PendingHint`] body) still reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedHint {
    /// The create's `commandId`.
    pub command_id: String,
    /// The directory that was resolved for it.
    pub path: PathBuf,
    /// The execution the create minted, when it minted one.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Path of the live hint file for one command.
pub fn pending_hint_path(hints_dir: &Path, command_id: &str) -> PathBuf {
    hints_dir.join(format!("{command_id}.json"))
}

/// Path of the marker left when that hint is consumed.
pub fn consumed_hint_path(hints_dir: &Path, command_id: &str) -> PathBuf {
    hints_dir.join(format!("{command_id}.consumed"))
}

/// The marker for `command_id`, if this command's hint has been spent.
///
/// A marker that cannot be read at all is treated as present-but-opaque by the
/// caller rather than as absent: the whole point is that "already decided"
/// never degrades into "never had one".
pub fn read_consumed_hint(hints_dir: &Path, command_id: &str) -> Option<ConsumedHint> {
    let path = consumed_hint_path(hints_dir, command_id);
    let body = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<ConsumedHint>(&body) {
        Ok(hint) => Some(hint),
        Err(error) => {
            tracing::warn!(
                target: "csp::projects",
                "consumed-hint marker {} is unreadable: {error}",
                path.display()
            );
            Some(ConsumedHint {
                command_id: command_id.to_owned(),
                path: PathBuf::new(),
                session_id: None,
            })
        }
    }
}

/// Whether this command's hint has already been spent.
pub fn hint_is_consumed(hints_dir: &Path, command_id: &str) -> bool {
    consumed_hint_path(hints_dir, command_id).exists()
}

/// Read the one-shot hint for `command_id`, if there is a usable one.
///
/// Every failure is "no hint": a missing file, unreadable bytes, malformed
/// JSON, a `commandId` that disagrees with the file name, or a path that is
/// not an existing absolute directory. Each is warned about and none is fatal,
/// because the alternative to falling through to the project and channel
/// entries is a create refused for a reason the operator did not cause.
pub fn read_pending_hint(hints_dir: &Path, command_id: &str) -> Option<PathBuf> {
    read_pending_hint_record(hints_dir, command_id).map(|hint| hint.path)
}

/// The whole one-shot hint for `command_id`, under the same rules as
/// [`read_pending_hint`].
pub fn read_pending_hint_record(hints_dir: &Path, command_id: &str) -> Option<PendingHint> {
    let path = pending_hint_path(hints_dir, command_id);
    let body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(
                target: "csp::projects",
                "cannot read create hint {}: {error}",
                path.display()
            );
            return None;
        }
    };
    let hint: PendingHint = match serde_json::from_str(&body) {
        Ok(hint) => hint,
        Err(error) => {
            tracing::warn!(
                target: "csp::projects",
                "ignoring malformed create hint {}: {error}",
                path.display()
            );
            return None;
        }
    };
    if hint.command_id != command_id {
        tracing::warn!(
            target: "csp::projects",
            "ignoring create hint {} whose commandId names {:?}",
            path.display(),
            hint.command_id
        );
        return None;
    }
    usable_directory(&hint.path).then_some(hint)
}

impl ProjectsFile {
    /// Read the file, treating every failure as "no entries".
    ///
    /// A missing or malformed projects file must not take the provider down: it
    /// degrades every affected create into a `PROJECT_CWD_UNRESOLVED` receipt,
    /// which is a message the operator can act on, unlike a dead process.
    pub fn load(path: Option<&Path>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        // Set on every path below, including the ones that read nothing: a
        // create's one-shot hint lives beside this file rather than in it, and
        // a projects file that is missing or unreadable says nothing about
        // whether a hint was written.
        let hints_dir = path.parent().map(|parent| parent.join(PENDING_HINTS_DIR));
        let empty = || Self {
            hints_dir: hints_dir.clone(),
            ..Self::default()
        };
        let body = match std::fs::read_to_string(path) {
            Ok(body) => body,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return empty(),
            Err(error) => {
                tracing::warn!(target: "csp::projects", "cannot read {}: {error}", path.display());
                return empty();
            }
        };
        let mut file: Self = match serde_json::from_str(&body) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(target: "csp::projects", "cannot parse {}: {error}", path.display());
                Self::default()
            }
        };
        file.hints_dir = hints_dir;
        file
    }

    /// Resolve a working directory: in-file pending hint, then the one-shot
    /// hint file, then project, then channel.
    ///
    /// The hint **file** sits second rather than first because an in-file
    /// `pending` entry is the stronger statement when it survived: the host
    /// wrote it into its own canonical store. The file is what covers the case
    /// where that entry did not survive, which is the ordinary case whenever
    /// the desktop saved anything between the CLI writing it and the create
    /// arriving (see [`PENDING_HINTS_DIR`]).
    ///
    /// A configured path that is not an existing absolute directory resolves to
    /// `None` — the same outcome as no entry at all — because handing a relative
    /// or missing path to `session/new` fails later and less legibly.
    pub fn resolve(
        &self,
        command_id: &str,
        project_ref: Option<&str>,
        channel_id: Uuid,
    ) -> Option<PathBuf> {
        if let Some(path) = self.resolve_hinted(command_id) {
            return Some(path);
        }
        // A spent hint that no longer names a usable directory resolves to
        // nothing rather than to a default (see `resolve_hinted`).
        if self
            .hints_dir
            .as_deref()
            .is_some_and(|hints_dir| read_consumed_hint(hints_dir, command_id).is_some())
        {
            return None;
        }
        self.resolve_default(project_ref, channel_id)
    }

    /// The directory staged for this one command, from any hint source, and
    /// never a project or channel default.
    ///
    /// A hired seat resolves through this alone: its directory is the
    /// worktree the host cut for it, and a missing hint means that fact was
    /// lost, not that the project's checkout will do (control run 7).
    pub fn resolve_hinted(&self, command_id: &str) -> Option<PathBuf> {
        if let Some(path) = self
            .pending
            .get(command_id)
            .filter(|path| usable_directory(path))
        {
            return Some(path.clone());
        }
        if let Some(path) = self
            .hints_dir
            .as_deref()
            .and_then(|hints_dir| read_pending_hint(hints_dir, command_id))
        {
            return Some(path);
        }
        // A spent hint still answers for its own command. Falling through to
        // the project or channel default here is the silent wrong-folder case
        // this whole mechanism exists to remove; a marker whose path is
        // unusable resolves to nothing at all, which
        // [`consumed_hint_without_record`] turns into a named refusal.
        if let Some(hints_dir) = self.hints_dir.as_deref() {
            if let Some(consumed) = read_consumed_hint(hints_dir, command_id) {
                return usable_directory(&consumed.path).then_some(consumed.path);
            }
        }
        None
    }

    /// Whether this command's live hint binds its directory to `project_ref`:
    /// the in-file `pending` entry with its `pendingProjects` project, or the
    /// hint file with its `projectRef`. A hint naming no project, or another
    /// one, binds nothing.
    pub fn hint_binds_project(&self, command_id: &str, project_ref: &str) -> bool {
        if self.pending.contains_key(command_id) {
            return self
                .pending_projects
                .get(command_id)
                .is_some_and(|bound| bound.trim() == project_ref);
        }
        self.hints_dir
            .as_deref()
            .and_then(|hints_dir| read_pending_hint_record(hints_dir, command_id))
            .and_then(|hint| hint.project_ref)
            .is_some_and(|bound| bound.trim() == project_ref)
    }

    /// The project's, then the channel's, remembered directory.
    pub fn resolve_default(&self, project_ref: Option<&str>, channel_id: Uuid) -> Option<PathBuf> {
        [
            project_ref.and_then(|project_ref| self.projects.get(project_ref)),
            self.channels.get(&channel_id),
        ]
        .into_iter()
        .flatten()
        .find(|path| usable_directory(path))
        .cloned()
    }

    /// Project coordinates advertised in the catalog's optional `projects[]`.
    pub fn project_refs(&self) -> impl Iterator<Item = &str> {
        self.projects.keys().map(String::as_str)
    }
}

fn usable_directory(path: &Path) -> bool {
    if !path.is_absolute() {
        tracing::warn!(target: "csp::projects", "ignoring relative working directory {}", path.display());
        return false;
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => true,
        Ok(_) => {
            tracing::warn!(target: "csp::projects", "ignoring non-directory working directory {}", path.display());
            false
        }
        Err(_) => {
            tracing::warn!(target: "csp::projects", "ignoring missing working directory {}", path.display());
            false
        }
    }
}
