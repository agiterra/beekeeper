//! Which directory a gate row is measured in, re-read at every gate.
//!
//! # The failure this exists to end (live-run finding 82)
//!
//! A seat's worktree was relocated by the app while its session was live. The
//! session record on this provider kept the path the create resolved, that
//! directory no longer existed, and [`crate::git_probe::probe`] degraded every
//! field to `None` exactly as it is designed to. The consequence was not a
//! quiet degradation: kind 44246 rows were minted carrying
//! `headSha: null, dirty: null`, the push gate could find no observed row
//! naming the commit, and arm (B) was unreachable until the session was
//! re-created. A row that names no commit is indistinguishable from a row
//! about a commit nobody can check.
//!
//! Two separate defects, fixed separately here:
//!
//! 1. **The path was never refreshed.** The host's own record is rewritten
//!    whenever it moves a tree, and the provider re-reads its projects file on
//!    every lifecycle command already — but the gate probe read
//!    [`crate::state::SessionRecord::cwd`], written once at create. This module
//!    re-reads the file at *every gate* and prefers the host's current answer.
//! 2. **A missing directory minted a row anyway.** It no longer can:
//!    [`resolve`] answers [`GateCwd::Missing`] and the caller publishes
//!    nothing at all, with the sentence [`GateCwd::refusal`] returns.
//!
//! # The channel between host and provider
//!
//! There is exactly one and it already exists: the file named by
//! `BEEKEEPER_CSP_PROJECTS_FILE` ([`crate::config::Config::projects_file`]), which
//! the desktop host rewrites on every mutation of its own working-directory
//! record (`desktop/src-tauri/src/coding_sessions/workdir_store.rs`,
//! `save_workdir_store`). No new env var, no new command, no restart: a tree
//! the host moves is a file the host rewrites, and the next gate reads it.
//!
//! This module reads a **narrow slice** of that file — the `sessions` map
//! alone, keyed by the producer-minted session id — for the same reason
//! `bee`'s worktree command reads a slice of the desktop store: an unknown
//! field must never be able to stop a gate from resolving, and a resolver has
//! no business holding the rest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The `sessions` slice of the host's projects file.
///
/// Every other key is ignored by construction. `#[serde(default)]` on the map
/// means a file written by a host that predates it — every host today —
/// resolves to "no override", which falls back to the recorded `cwd` and is
/// exactly the behaviour before this module existed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionWorkdirs {
    /// Current working directory per producer-minted session id.
    #[serde(default)]
    sessions: BTreeMap<String, PathBuf>,
}

/// Where one gate's facts may be measured, or why they may not be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GateCwd {
    /// The directory exists right now; probe it.
    Present(PathBuf),
    /// The directory the host names for this session is not there.
    ///
    /// Carries the path so the refusal can name it. The caller mints **no**
    /// row: a gate whose subject cannot be located has not been observed, and
    /// saying so is the whole point.
    Missing(PathBuf),
}

impl GateCwd {
    /// The one sentence a missing directory is reported with.
    ///
    /// Fixed wording — `cwd missing: <path>` — because it is what an operator
    /// greps for after a relocation, and because finding 82's symptom was the
    /// *absence* of any sentence at all.
    pub(crate) fn refusal(&self) -> Option<String> {
        match self {
            Self::Present(_) => None,
            Self::Missing(path) => Some(format!("cwd missing: {}", path.display())),
        }
    }

    /// The directory to probe, when there is one.
    pub(crate) fn present(&self) -> Option<&Path> {
        match self {
            Self::Present(path) => Some(path),
            Self::Missing(_) => None,
        }
    }
}

/// Decide where this session's gate facts are measured, right now.
///
/// `projects_file` is re-read on every call. That is deliberate: a gate runs
/// at most a few times per turn, the file is a few kilobytes, and the
/// alternative — caching the create's answer — is the bug. When the host
/// names a directory for `session_id` that answer wins over `recorded`,
/// because the host is the only party that knows it moved the tree.
///
/// A host answer that does not exist on disk is **not** silently replaced by
/// the recorded path: that would put gate rows back in whichever directory the
/// create happened to resolve, which after a relocation is the founder's own
/// checkout. Both a stale record and a stale override refuse the same way.
pub(crate) fn resolve(projects_file: Option<&Path>, session_id: &str, recorded: &Path) -> GateCwd {
    let chosen = host_workdir(projects_file, session_id).unwrap_or_else(|| recorded.to_path_buf());
    if chosen.is_dir() {
        GateCwd::Present(chosen)
    } else {
        GateCwd::Missing(chosen)
    }
}

/// The directory the host currently names for `session_id`, if any.
///
/// Every failure — no configured file, a missing file, unreadable bytes,
/// unparsable JSON, a relative path — answers `None`, which falls back to the
/// recorded path. A resolver that took a provider down, or that accepted a
/// relative path git would interpret against its own cwd, would be worse than
/// the stale answer it replaced.
fn host_workdir(projects_file: Option<&Path>, session_id: &str) -> Option<PathBuf> {
    let path = projects_file?;
    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(
                target: "csp::git",
                "cannot read {} for a gate cwd: {error}",
                path.display()
            );
            return None;
        }
    };
    let parsed: SessionWorkdirs = match serde_json::from_str(&body) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(
                target: "csp::git",
                "cannot parse {} for a gate cwd: {error}",
                path.display()
            );
            return None;
        }
    };
    let candidate = parsed.sessions.get(session_id)?;
    if !candidate.is_absolute() {
        tracing::warn!(
            target: "csp::git",
            "ignoring relative session working directory {}",
            candidate.display()
        );
        return None;
    }
    Some(candidate.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a projects file whose `sessions` map holds one entry.
    fn projects_file(dir: &Path, session_id: &str, path: &Path) -> PathBuf {
        let file = dir.join("projects.json");
        std::fs::write(
            &file,
            format!(
                r#"{{"version":1,"pending":{{}},"projects":{{}},"channels":{{}},"sessions":{{"{session_id}":"{}"}}}}"#,
                path.display()
            ),
        )
        .expect("write projects file");
        file
    }

    #[test]
    fn with_no_projects_file_the_recorded_directory_is_used() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            resolve(None, "s1", dir.path()),
            GateCwd::Present(dir.path().to_path_buf())
        );
    }

    #[test]
    fn the_hosts_current_answer_beats_the_recorded_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let moved = dir.path().join("moved");
        std::fs::create_dir(&moved).expect("mkdir");
        let file = projects_file(dir.path(), "s1", &moved);
        let stale = dir.path().join("gone");
        assert_eq!(
            resolve(Some(&file), "s1", &stale),
            GateCwd::Present(moved.clone())
        );
    }

    #[test]
    fn a_session_the_host_does_not_name_falls_back_to_the_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let moved = dir.path().join("moved");
        std::fs::create_dir(&moved).expect("mkdir");
        let file = projects_file(dir.path(), "other", &moved);
        assert_eq!(
            resolve(Some(&file), "s1", dir.path()),
            GateCwd::Present(dir.path().to_path_buf())
        );
    }

    /// Finding 82's exact shape: the tree was relocated, the record still
    /// names the old path, and nothing on disk answers to it.
    #[test]
    fn a_relocated_worktree_with_no_host_answer_refuses_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gone = dir.path().join("relocated-away");
        let resolved = resolve(None, "s1", &gone);
        assert_eq!(resolved, GateCwd::Missing(gone.clone()));
        assert_eq!(
            resolved.refusal().as_deref(),
            Some(format!("cwd missing: {}", gone.display()).as_str())
        );
        assert!(resolved.present().is_none());
    }

    /// A stale override is refused rather than quietly replaced by the
    /// recorded path — otherwise a relocation would silently move gate rows
    /// back into whichever checkout the create resolved.
    #[test]
    fn a_host_answer_that_is_gone_refuses_rather_than_falling_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gone = dir.path().join("host-says-here-but-it-is-not");
        let file = projects_file(dir.path(), "s1", &gone);
        assert_eq!(
            resolve(Some(&file), "s1", dir.path()),
            GateCwd::Missing(gone)
        );
    }

    #[test]
    fn an_unreadable_or_unparsable_file_falls_back_to_the_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let broken = dir.path().join("broken.json");
        std::fs::write(&broken, "{not json").expect("write");
        assert_eq!(
            resolve(Some(&broken), "s1", dir.path()),
            GateCwd::Present(dir.path().to_path_buf())
        );
        assert_eq!(
            resolve(Some(&dir.path().join("absent.json")), "s1", dir.path()),
            GateCwd::Present(dir.path().to_path_buf())
        );
    }

    /// A file from a host that predates the `sessions` map reads as no
    /// override at all, never as an error.
    #[test]
    fn a_projects_file_without_the_sessions_map_is_no_override() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("projects.json");
        std::fs::write(
            &file,
            r#"{"version":1,"pending":{},"projects":{},"channels":{}}"#,
        )
        .expect("write");
        assert_eq!(
            resolve(Some(&file), "s1", dir.path()),
            GateCwd::Present(dir.path().to_path_buf())
        );
    }

    #[test]
    fn a_relative_host_answer_is_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("projects.json");
        std::fs::write(&file, r#"{"version":1,"sessions":{"s1":"relative/path"}}"#).expect("write");
        assert_eq!(
            resolve(Some(&file), "s1", dir.path()),
            GateCwd::Present(dir.path().to_path_buf())
        );
    }
}
