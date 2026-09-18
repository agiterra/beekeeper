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
//! When the fetch fails (offline, relay down) the last fetched tip is read
//! instead and the answer says so: `stale` is `true` and `fetched_at` is when
//! that tip arrived, so a refusal or a receipt can say "as fetched at …"
//! rather than pass off an old file as current.

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
}

impl AgentsFile {
    /// "`abcd1234`" or "`abcd1234`, as fetched at `<time>`": what a message
    /// says about where the bytes came from.
    pub fn provenance(&self) -> String {
        let short: String = self.sha.chars().take(8).collect();
        if !self.stale {
            return short;
        }
        match self.fetched_at {
            Some(at) => format!("{short}, as fetched at {}", format_time(at)),
            None => format!("{short}, as last fetched (the fetch just now failed)"),
        }
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

/// Read `file` from the tip of the record's ref, fetching first.
pub async fn read_agents_file(
    record: &AgentsRepoRecord,
    file: &str,
) -> Result<AgentsFile, AgentsReadError> {
    if !record.path.join(".git").exists() {
        return Err(AgentsReadError::NoCheckout {
            path: record.path.clone(),
            detail: "no .git directory".to_owned(),
        });
    }
    let (sha, stale, fetched_at) = match git(
        &record.path,
        &["fetch", "--quiet", "origin", &record.ref_name],
    )
    .await
    {
        Ok(_) => {
            let sha = git(&record.path, &["rev-parse", "FETCH_HEAD"])
                .await
                .map_err(|detail| AgentsReadError::Git { detail })?;
            (sha.trim().to_owned(), false, Some(now_secs()))
        }
        Err(fetch_error) => {
            let branch = record
                .ref_name
                .strip_prefix("refs/heads/")
                .unwrap_or(&record.ref_name);
            let remote_ref = format!("refs/remotes/origin/{branch}");
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
            let fetched_at = std::fs::metadata(record.path.join(".git").join("FETCH_HEAD"))
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            (sha, true, fetched_at)
        }
    };
    let spec = format!("{sha}:{file}");
    match git(&record.path, &["show", &spec]).await {
        Ok(text) => Ok(AgentsFile {
            text,
            sha,
            stale,
            fetched_at,
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

async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
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
            stale.provenance().contains("as fetched"),
            "{}",
            stale.provenance()
        );

        let nowhere = AgentsRepoRecord {
            path: tmp.path().join("nowhere"),
            ref_name: "refs/heads/main".into(),
        };
        assert!(matches!(
            read_agents_file(&nowhere, "actions.yml").await.unwrap_err(),
            AgentsReadError::NoCheckout { .. }
        ));
    }
}
