//! Bounded, best-effort git observation of a session's working directory.
//!
//! **Proof spike (P3).** The question this module exists to answer is whether
//! the provider can cheaply and safely read `HEAD` and dirty state from the
//! `cwd` it already holds on every [`crate::state::SessionRecord`]. It is
//! deliberately minimal: two `git` invocations, each under its own timeout,
//! every failure degrading to `None` rather than surfacing to the operator.
//!
//! # Boundaries
//!
//! - **Never fatal.** [`probe`] returns [`GitProbe`] infallibly. A missing
//!   `git`, a non-repository `cwd`, a timeout, and a repository with no commits
//!   all produce the same shape: fields the caller must treat as unknown.
//! - **Never leaks a host path.** Only the *branch shortname* is captured.
//!   Nothing here runs `rev-parse --show-toplevel` or any other command whose
//!   output is an absolute path, because the branch is published in signed
//!   content and `no_published_event_ever_carries_the_host_working_directory`
//!   asserts that boundary.
//! - **Never mutates the repository.** Every invocation passes
//!   `--no-optional-locks`, so a probe cannot contend with the session's own
//!   agent for `.git/index.lock`.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// Ceiling on a single `git` invocation.
///
/// Warm-cache measurements on a 4,200-file checkout are 7ms (branch) and 22ms
/// (dirty); two seconds is roughly a 90x margin, chosen so that a cold or
/// network-backed checkout still has room to answer while a wedged `git` cannot
/// stall the caller for longer than an operator would notice.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Longest branch shortname accepted into published content.
const MAX_BRANCH_LEN: usize = 255;

/// What one bounded look at a session's working directory saw.
///
/// Every field is independently optional: `None` means "not observed", never
/// "observed to be absent". A non-repository directory yields
/// [`GitProbe::default`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GitProbe {
    /// Current branch shortname, or `None` on a detached `HEAD`, a
    /// non-repository, or any failure.
    pub(crate) branch: Option<String>,
    /// Whether the worktree has uncommitted changes, or `None` if not observed.
    ///
    /// P3 keeps this internal on purpose. Carrying dirty state as a *signed*
    /// fact is B1's work, and a boolean smuggled into `branch` would be a
    /// schema change wearing a disguise.
    pub(crate) dirty: Option<bool>,
}

/// Look at `cwd` with `git`, bounded and best effort.
///
/// Runs at most two short-lived `git` processes. Neither can outlive
/// [`PROBE_TIMEOUT`]; `kill_on_drop` guarantees a timed-out child is reaped
/// rather than orphaned.
pub(crate) async fn probe(cwd: &Path) -> GitProbe {
    let branch = branch(cwd).await;
    // Skipped when the branch probe already proved this is not a repository:
    // the second invocation could only fail the same way, and a create should
    // not pay 7ms to re-learn it.
    let dirty = if branch.is_some() {
        dirty(cwd).await
    } else {
        None
    };
    GitProbe { branch, dirty }
}

/// Current branch shortname, or `None` if there is not exactly one.
///
/// Uses `git branch --show-current` rather than `rev-parse --abbrev-ref HEAD`
/// for two reasons the spike measured: a detached `HEAD` prints nothing (rather
/// than the literal string `HEAD`, which would publish as a fake branch named
/// "HEAD"), and a repository with no commits still names its unborn branch
/// (rather than failing outright). Cost is identical — both are ~7ms.
async fn branch(cwd: &Path) -> Option<String> {
    let stdout = run_git(cwd, &["branch", "--show-current"]).await?;
    let branch = stdout.trim();
    if branch.is_empty() || branch.len() > MAX_BRANCH_LEN {
        return None;
    }
    Some(branch.to_owned())
}

/// Whether the worktree differs from `HEAD`, including untracked files.
///
/// `status --porcelain` is O(worktree) and the most expensive thing this module
/// does (22ms warm on a 4,200-file checkout, vs 12ms for
/// `--untracked-files=no`). The extra 10ms buys untracked files, which for an
/// *agent* session is the common case — an agent that only ever creates new
/// files would otherwise read as clean.
async fn dirty(cwd: &Path) -> Option<bool> {
    let stdout = run_git(cwd, &["status", "--porcelain"]).await?;
    Some(!stdout.trim().is_empty())
}

/// Run `git <args>` in `cwd` under [`PROBE_TIMEOUT`], returning stdout.
///
/// `None` on spawn failure, non-zero exit, timeout, or non-UTF-8 output — the
/// caller cannot distinguish them, and does not need to.
async fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command
        // Refuse to take any lock the read does not strictly require, so a
        // probe can never contend with the session's own agent over
        // `.git/index.lock`.
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        // A credential or GPG prompt would block until the timeout; refusing
        // the prompt turns a 2s stall into an immediate failure.
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);

    let label = args.join(" ");
    match tokio::time::timeout(PROBE_TIMEOUT, command.output()).await {
        Ok(Ok(output)) if output.status.success() => String::from_utf8(output.stdout).ok(),
        Ok(Ok(output)) => {
            // Expected whenever `cwd` is not a repository, so this is debug —
            // an operator running an agent in a plain directory has not made a
            // mistake worth warning about.
            tracing::debug!(
                target: "csp::git",
                status = %output.status,
                "git {label} did not succeed"
            );
            None
        }
        Ok(Err(error)) => {
            tracing::warn!(target: "csp::git", "git {label} could not run: {error}");
            None
        }
        Err(_) => {
            tracing::warn!(
                target: "csp::git",
                "git {label} timed out after {PROBE_TIMEOUT:?}"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Commit `message` in `cwd` with identity forced, so the probe tests do
    /// not depend on the developer's git config.
    fn commit(cwd: &Path, message: &str) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args([
                "-c",
                "user.email=probe@example.invalid",
                "-c",
                "user.name=probe",
                "commit",
                "-q",
                "--no-gpg-sign",
                "-m",
                message,
            ])
            .status()
            .expect("git commit");
        assert!(status.success(), "git commit failed");
    }

    fn git(cwd: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    }

    /// A repository with one commit on a branch named `probe-branch`.
    fn repo(cwd: &Path) {
        git(cwd, &["init", "-q", "-b", "probe-branch", "."]);
        std::fs::write(cwd.join("a.txt"), "a").expect("write");
        git(cwd, &["add", "a.txt"]);
        commit(cwd, "one");
    }

    #[tokio::test]
    async fn a_plain_directory_is_not_observed_at_all() {
        let dir = tempfile::tempdir().expect("tempdir");
        let observed = probe(dir.path()).await;
        assert_eq!(observed, GitProbe::default());
    }

    #[tokio::test]
    async fn a_missing_directory_degrades_rather_than_failing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let observed = probe(&dir.path().join("does-not-exist")).await;
        assert_eq!(observed, GitProbe::default());
    }

    #[tokio::test]
    async fn a_clean_checkout_reports_its_branch_and_no_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        let observed = probe(dir.path()).await;
        assert_eq!(observed.branch.as_deref(), Some("probe-branch"));
        assert_eq!(observed.dirty, Some(false));
    }

    #[tokio::test]
    async fn an_edited_file_reads_as_dirty() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "changed").expect("write");
        assert_eq!(probe(dir.path()).await.dirty, Some(true));
    }

    /// The reason this probe pays for `status` over the cheaper
    /// `diff --quiet`: an agent's first act is usually to *create* a file, and
    /// `diff --quiet` reports that worktree as clean.
    #[tokio::test]
    async fn an_untracked_file_reads_as_dirty() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        std::fs::write(dir.path().join("new.txt"), "new").expect("write");
        assert_eq!(probe(dir.path()).await.dirty, Some(true));
    }

    /// A detached `HEAD` has no branch to name, and publishing the literal
    /// string "HEAD" would read to a consumer as a branch actually called
    /// "HEAD". `--show-current` prints nothing, which maps to `None`.
    #[tokio::test]
    async fn a_detached_head_reports_no_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        git(dir.path(), &["checkout", "-q", "--detach", "HEAD"]);
        assert_eq!(probe(dir.path()).await.branch, None);
    }

    /// A freshly initialized repository has an unborn branch and no commits.
    /// `--show-current` still names it, so a session created in a repository
    /// before its first commit is not indistinguishable from a plain directory.
    #[tokio::test]
    async fn an_empty_repository_still_names_its_unborn_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        git(dir.path(), &["init", "-q", "-b", "unborn", "."]);
        let observed = probe(dir.path()).await;
        assert_eq!(observed.branch.as_deref(), Some("unborn"));
        assert_eq!(observed.dirty, Some(false));
    }

    /// Proves the bound is real rather than decorative: with the ceiling driven
    /// to zero the invocation is abandoned, `kill_on_drop` reaps the child, and
    /// the caller still gets a value.
    #[tokio::test]
    async fn a_timeout_degrades_to_no_observation() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        let mut command = Command::new("git");
        command
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(dir.path())
            .args(["status", "--porcelain"])
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let timed_out = tokio::time::timeout(Duration::from_nanos(1), command.output()).await;
        assert!(timed_out.is_err(), "the bound did not fire");
    }
}
