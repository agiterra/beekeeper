//! The project's agents repository as this host reads it (spec § 4.11).
//!
//! `actions.yml` and `team.yml` live at the root of `<slug>-beekeeper-agents`,
//! not in the code checkout. The desktop records where this host's clone of
//! that repository is and which ref the project's kind:30624 pins
//! (`ProjectsFile::agents_repos`); this module reads a file from that
//! clone's **fetched tip** — `git fetch origin <ref>`, then `git show
//! FETCH_HEAD:<file>` — never from a working copy, so nothing an operator has
//! open in an editor is ever mistaken for the published definition.
//!
//! **The fetch goes to the recorded clone URL** (ledger 250). The desktop's
//! packs cache clones by URL and never configures an `origin` remote
//! (`desktop/src-tauri/src/managed_agents/packs_cache.rs`, `fetch`), so the
//! `git fetch origin <ref>` this module used to run failed on every call. The
//! failure fell through to the last `refs/remotes/origin/<branch>` the
//! desktop happened to write, and the refusal called that "as fetched at" the
//! mtime of a `FETCH_HEAD` the failed fetch had just truncated — in
//! kettle-control-2 run b60be720 that read "42bc7697, as fetched at 19:43:20Z"
//! while the relay's main was already 71098e4b. The record now carries the
//! URL; the fetch updates `refs/remotes/origin/<branch>` so the desktop and
//! this module agree on the tip.
//!
//! When the fetch fails (offline, relay down) the last fetched tip is read
//! instead and the answer says so: `stale` is `true`, `fetch_error` is what
//! failed just now, and `fetched_at` is when that tip was last *updated*
//! (the remote-tracking ref's reflog), never the mtime of a file the failed
//! fetch itself may have touched.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// How long one git invocation may take.
const GIT_TIMEOUT: Duration = Duration::from_secs(45);

/// Where this host keeps its clone of a project's agents repository, and the
/// ref the project pins. Written by the desktop into the projects file.
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

/// One file read from the agents repository's tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsFile {
    /// The file's bytes at `sha`, as UTF-8.
    pub text: String,
    /// The commit the file was read from.
    pub sha: String,
    /// `true` when the fetch failed and `sha` is the last tip this host had.
    pub stale: bool,
    /// When the tip was fetched (unix seconds), when known.
    pub fetched_at: Option<u64>,
    /// Why the fetch just now failed, when `stale`.
    pub fetch_error: Option<String>,
}

impl AgentsFile {
    /// "`abcd1234`" or "`abcd1234`, as fetched at `<time>`": what a message
    /// says about where the bytes came from.
    pub fn provenance(&self) -> String {
        let short: String = self.sha.chars().take(8).collect();
        if !self.stale {
            return short;
        }
        let when = match self.fetched_at {
            Some(at) => format!("as last fetched at {}", format_time(at)),
            None => "as last fetched".to_owned(),
        };
        let why = self
            .fetch_error
            .as_deref()
            .map(|error| error.lines().next().unwrap_or(error).trim().to_owned())
            .filter(|error| !error.is_empty())
            .unwrap_or_else(|| "no reason given".to_owned());
        format!("{short}, {when}; fetching just now failed: {why}")
    }
}

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsReadError {
    /// The recorded path is not a git repository this host can read.
    NoCheckout { path: PathBuf, detail: String },
    /// The fetch failed and no earlier tip of the ref is known.
    NoTip { ref_name: String, detail: String },
    /// The tip holds no such file.
    FileMissing { file: String, sha: String },
    /// Git failed in some other way.
    Git { detail: String },
}

impl std::fmt::Display for AgentsReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCheckout { path, detail } => write!(
                f,
                "this host's clone of the agents repository at {} is not readable: {detail}",
                path.display()
            ),
            Self::NoTip { ref_name, detail } => write!(
                f,
                "could not fetch {ref_name} of the agents repository and no earlier tip is known: {detail}"
            ),
            Self::FileMissing { file, sha } => {
                write!(f, "the agents repository at {} has no {file}", &sha[..sha.len().min(8)])
            }
            Self::Git { detail } => write!(f, "git failed: {detail}"),
        }
    }
}

/// The commit the record's ref points at: fetched just now, or the last tip
/// this host had when the fetch fails (`stale`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsTip {
    /// The commit.
    pub sha: String,
    /// `true` when the fetch failed and `sha` is the last tip this host had.
    pub stale: bool,
    /// When the tip was fetched (unix seconds), when known.
    pub fetched_at: Option<u64>,
    /// Why the fetch just now failed, when `stale`.
    pub fetch_error: Option<String>,
}

/// Fetch the record's ref and answer its tip.
///
/// # Errors
/// No readable clone, or no tip at all.
pub async fn pinned_tip(record: &AgentsRepoRecord) -> Result<AgentsTip, AgentsReadError> {
    if !record.path.join(".git").exists() {
        return Err(AgentsReadError::NoCheckout {
            path: record.path.clone(),
            detail: "no .git directory".to_owned(),
        });
    }
    let branch = record
        .ref_name
        .strip_prefix("refs/heads/")
        .unwrap_or(&record.ref_name);
    let remote_ref = format!("refs/remotes/origin/{branch}");
    let (sha, fetch_error) = match fetch_tip(record, &remote_ref).await {
        Ok(()) => (
            git(&record.path, &["rev-parse", "--verify", &remote_ref])
                .await
                .map_err(|detail| AgentsReadError::Git { detail })?
                .trim()
                .to_owned(),
            None,
        ),
        Err(fetch_error) => {
            let sha = match git(
                &record.path,
                &["rev-parse", "--verify", "--quiet", &remote_ref],
            )
            .await
            {
                Ok(sha) if !sha.trim().is_empty() => sha.trim().to_owned(),
                _ => {
                    return Err(AgentsReadError::NoTip {
                        ref_name: record.ref_name.clone(),
                        detail: fetch_error,
                    })
                }
            };
            (sha, Some(fetch_error))
        }
    };
    let stale = fetch_error.is_some();
    let fetched_at = if stale {
        last_updated(&record.path, &remote_ref).await
    } else {
        Some(now_secs())
    };
    Ok(AgentsTip {
        sha,
        stale,
        fetched_at,
        fetch_error,
    })
}

/// Read `file` from the tip of the record's ref, fetching first.
pub async fn read_agents_file(
    record: &AgentsRepoRecord,
    file: &str,
) -> Result<AgentsFile, AgentsReadError> {
    let AgentsTip {
        sha,
        stale,
        fetched_at,
        fetch_error,
    } = pinned_tip(record).await?;
    let spec = format!("{sha}:{file}");
    match git(&record.path, &["show", &spec]).await {
        Ok(text) => Ok(AgentsFile {
            text,
            sha,
            stale,
            fetched_at,
            fetch_error,
        }),
        Err(detail)
            if detail.contains("does not exist")
                || detail.contains("exists on disk, but not in") =>
        {
            Err(AgentsReadError::FileMissing {
                file: file.to_owned(),
                sha,
            })
        }
        Err(detail) => Err(AgentsReadError::Git { detail }),
    }
}

/// Fetch the record's ref into `remote_ref`: from the recorded URL, or —
/// for a record written before the URL was — from an `origin` remote if the
/// clone has one. `Err` is git's own complaint, or the sentence saying there
/// is nowhere to fetch from.
async fn fetch_tip(record: &AgentsRepoRecord, remote_ref: &str) -> Result<(), String> {
    let source =
        match record.url.as_deref().map(str::trim) {
            Some(url) if !url.is_empty() => url.to_owned(),
            _ => match git(&record.path, &["remote", "get-url", "origin"]).await {
                Ok(url) if !url.trim().is_empty() => "origin".to_owned(),
                _ => return Err(
                    "this host recorded no clone URL for the agents repository and the clone has \
                     no origin remote; open the project's Actions tab to record it again"
                        .to_owned(),
                ),
            },
        };
    let refspec = format!("+{}:{remote_ref}", record.ref_name);
    git(
        &record.path,
        &["fetch", "--quiet", "--no-tags", "--", &source, &refspec],
    )
    .await
    .map(|_| ())
}

/// When `remote_ref` last moved (unix seconds), from its reflog. `None` when
/// the clone keeps no reflog for it — an unknown time, never a guessed one.
async fn last_updated(dir: &Path, remote_ref: &str) -> Option<u64> {
    git(
        dir,
        &["log", "-g", "-1", "--format=%gd", "--date=unix", remote_ref],
    )
    .await
    .ok()
    .and_then(|line| {
        let inner = line
            .trim()
            .rsplit_once('{')?
            .1
            .strip_suffix('}')?
            .to_owned();
        inner.parse::<u64>().ok()
    })
}

/// Stage a read-only clone of the record's pinned tip at `dest`, for an
/// execution with no seat clone of its own (an ordinary Solo session).
///
/// The clone is taken from this host's own clone — implementation storage the
/// execution never sees — and checked out host-side with global and system
/// configuration, hooks and fsmonitor off, in a repository whose configuration
/// the host just wrote and the execution can only read: nothing of the
/// project's runs here. A later call refreshes it to the current tip.
///
/// # Errors
/// No tip could be read, or Git failed.
pub async fn stage_read_only_clone(
    record: &AgentsRepoRecord,
    dest: &Path,
) -> Result<AgentsTip, AgentsReadError> {
    let tip = pinned_tip(record).await?;
    let failed = |detail: String| AgentsReadError::Git { detail };
    let source = record.path.to_string_lossy().into_owned();
    if dest.join(".git").is_dir() {
        clone_git(
            dest,
            &["fetch", "--quiet", "--no-tags", "--", &source, &tip.sha],
        )
        .await
        .map_err(failed)?;
    } else {
        let parent = dest.parent().unwrap_or(dest);
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| failed(error.to_string()))?;
        let dest_text = dest.to_string_lossy().into_owned();
        clone_git(
            parent,
            &[
                "clone",
                "--quiet",
                "--no-checkout",
                "--no-hardlinks",
                "--",
                &source,
                &dest_text,
            ],
        )
        .await
        .map_err(failed)?;
    }
    clone_git(
        dest,
        &["checkout", "--quiet", "--force", "--detach", &tip.sha],
    )
    .await
    .map_err(failed)?;
    Ok(tip)
}

/// Git on a host-staged clone: hermetic, so no operator or project setting
/// can run a program.
async fn clone_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        cmd.env_remove(var);
    }
    cmd.args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
    ])
    .args(args)
    .current_dir(dir)
    .env("GIT_CONFIG_GLOBAL", "/dev/null")
    .env("GIT_CONFIG_NOSYSTEM", "1")
    .env("GIT_TERMINAL_PROMPT", "0")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    let output = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("git {} timed out", args.join(" ")))?
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    // The host's own clone: its configuration is the host's, but no hook of
    // it runs either way.
    cmd.args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
    ])
    .args(args)
    .current_dir(dir)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    let output = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("git {} timed out", args.join(" ")))?
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_time(unix: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(unix as i64, 0)
        .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| unix.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .await
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// An origin with `actions.yml` on `main`, and this host's clone of it.
    async fn origin_and_clone(tmp: &Path) -> (PathBuf, AgentsRepoRecord) {
        let origin = tmp.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        sh(&origin, &["init", "--quiet", "--initial-branch", "main"]).await;
        std::fs::write(origin.join("actions.yml"), "schema: v1\nactions: []\n").unwrap();
        sh(&origin, &["add", "--all"]).await;
        sh(&origin, &["commit", "--quiet", "-m", "seed"]).await;
        let clone = tmp.join("clone");
        sh(
            tmp,
            &[
                "clone",
                "--quiet",
                origin.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        )
        .await;
        (
            origin,
            AgentsRepoRecord {
                path: clone,
                ref_name: "refs/heads/main".into(),
                url: None,
            },
        )
    }

    #[tokio::test]
    async fn the_file_comes_from_the_fetched_tip_never_the_working_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, record) = origin_and_clone(tmp.path()).await;
        let first = read_agents_file(&record, "actions.yml").await.unwrap();
        assert_eq!(first.text, "schema: v1\nactions: []\n");
        assert!(!first.stale);
        assert_eq!(first.provenance(), first.sha[..8].to_owned());

        // A new commit on the origin is what the next read sees…
        std::fs::write(origin.join("actions.yml"), "schema: v1\nactions: [x]\n").unwrap();
        sh(&origin, &["commit", "--quiet", "-am", "edit"]).await;
        // …even though the clone's working copy still holds the old bytes
        // and an operator scribbled on it.
        std::fs::write(record.path.join("actions.yml"), "scribble\n").unwrap();
        let second = read_agents_file(&record, "actions.yml").await.unwrap();
        assert_eq!(second.text, "schema: v1\nactions: [x]\n");
        assert_ne!(second.sha, first.sha);

        assert!(matches!(
            read_agents_file(&record, "team.yml").await.unwrap_err(),
            AgentsReadError::FileMissing { .. }
        ));
    }

    #[tokio::test]
    async fn a_failed_fetch_reads_the_last_tip_and_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, record) = origin_and_clone(tmp.path()).await;
        read_agents_file(&record, "actions.yml").await.unwrap();
        sh(
            &record.path,
            &["remote", "set-url", "origin", "/nonexistent/path"],
        )
        .await;
        let stale = read_agents_file(&record, "actions.yml").await.unwrap();
        assert!(stale.stale);
        assert_eq!(stale.text, "schema: v1\nactions: []\n");
        assert!(
            stale.provenance().contains("fetching just now failed"),
            "{}",
            stale.provenance()
        );

        let nowhere = AgentsRepoRecord {
            path: tmp.path().join("nowhere"),
            ref_name: "refs/heads/main".into(),
            url: None,
        };
        assert!(matches!(
            read_agents_file(&nowhere, "actions.yml").await.unwrap_err(),
            AgentsReadError::NoCheckout { .. }
        ));
    }

    /// Ledger 250, kettle-control-2 run b60be720 (46023 `dd359da3…`): the
    /// packs cache clones by URL and configures no `origin` remote, so the
    /// host's pack copy sat one commit behind the relay's main and every
    /// fetch failed. A trigger naming an action that exists only on main was
    /// refused `ACTION_UNKNOWN`, "as fetched at" the moment of the failure.
    /// The fetch goes to the recorded URL; the action resolves, and the
    /// answer names the commit it resolved at.
    #[tokio::test]
    async fn a_pack_copy_without_an_origin_remote_fetches_main_by_url() {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        sh(&origin, &["init", "--quiet", "--initial-branch", "main"]).await;
        let old = "schema: v1\nactions:\n  build:\n    steps: []\n";
        std::fs::write(origin.join("actions.yml"), old).unwrap();
        sh(&origin, &["add", "--all"]).await;
        sh(&origin, &["commit", "--quiet", "-m", "seed"]).await;
        // The packs cache's own shape: init, fetch by URL into
        // refs/remotes/origin/*, detach — no remote named origin.
        let copy = tmp.path().join("copy");
        std::fs::create_dir_all(&copy).unwrap();
        sh(&copy, &["init", "--quiet"]).await;
        sh(
            &copy,
            &[
                "fetch",
                "--quiet",
                "--",
                origin.to_str().unwrap(),
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        )
        .await;
        sh(
            &copy,
            &[
                "checkout",
                "--quiet",
                "--detach",
                "refs/remotes/origin/main",
            ],
        )
        .await;
        assert_eq!(
            sh(&copy, &["remote"]).await,
            "",
            "no origin remote, as in the pack cache"
        );
        // The lead commits `verify` to main after the copy was synced.
        let new = "schema: v1\nactions:\n  verify:\n    steps: []\n";
        std::fs::write(origin.join("actions.yml"), new).unwrap();
        sh(&origin, &["commit", "--quiet", "-am", "add verify"]).await;
        let main = sh(&origin, &["rev-parse", "HEAD"]).await;

        let record = AgentsRepoRecord {
            path: copy.clone(),
            ref_name: "refs/heads/main".into(),
            url: Some(origin.to_str().unwrap().to_owned()),
        };
        let read = read_agents_file(&record, "actions.yml").await.unwrap();
        assert!(
            !read.stale,
            "the fetch just happened: {}",
            read.provenance()
        );
        assert_eq!(
            read.sha, main,
            "resolved at the relay's main, not the copy's tip"
        );
        assert_eq!(read.text, new);
        // The desktop's view of the tip moves with it.
        assert_eq!(
            sh(&copy, &["rev-parse", "refs/remotes/origin/main"]).await,
            main
        );

        // A record written before the URL was recorded, on a copy with no
        // origin: the refusal says there is nowhere to fetch from, and never
        // calls the old tip "fetched" just now.
        let legacy = AgentsRepoRecord {
            url: None,
            ..record.clone()
        };
        let stale = read_agents_file(&legacy, "actions.yml").await.unwrap();
        assert!(stale.stale);
        assert!(
            stale.provenance().contains("no origin remote"),
            "{}",
            stale.provenance()
        );
    }
}
