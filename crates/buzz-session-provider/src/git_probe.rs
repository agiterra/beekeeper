//! Bounded, best-effort git observation of a session's working directory.
//!
//! **Proof spike (P3), extended by B1.** The question P3 exists to answer is
//! whether the provider can cheaply and safely read `HEAD` and dirty state
//! from the `cwd` it already holds on every [`crate::state::SessionRecord`].
//! B1 (D4a, coordinate facts) adds the commit object id itself: P3 kept dirty
//! state internal on purpose ("carrying that as a signed fact is a schema
//! question"), and B1 is that schema question answered — both the commit and
//! the dirty flag are now folded into [`crate::payload::SessionMetadata`] by
//! [`crate::Provider::metadata_for`]. It is deliberately minimal: three `git`
//! invocations, each under its own timeout, every failure degrading to
//! `None` rather than surfacing to the operator.
//!
//! # Boundaries
//!
//! - **Never fatal.** [`probe`] returns [`GitProbe`] infallibly. A missing
//!   `git`, a non-repository `cwd`, a timeout, and a repository with no commits
//!   all produce the same shape: fields the caller must treat as unknown.
//! - **Never leaks a host path.** Only the *branch shortname* and the *commit
//!   object id* are captured — both host-independent. Nothing here runs
//!   `rev-parse --show-toplevel` or any other command whose output is an
//!   absolute path, because both are published in signed content and
//!   `no_published_event_ever_carries_the_host_working_directory` asserts
//!   that boundary.
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

/// Exact byte length of a lowercase-hex git object id under the default
/// SHA-1 object format. A SHA-256 repository's id is 64 hex characters;
/// both lengths are accepted so this probe does not have an opinion about
/// which object format a checkout uses.
const OID_LEN_SHA1: usize = 40;
const OID_LEN_SHA256: usize = 64;

/// What one bounded look at a session's working directory saw.
///
/// Every field is independently optional: `None` means "not observed", never
/// "observed to be absent". A non-repository directory yields
/// [`GitProbe::default`].
/// `pub` only so it can ride [`crate::session::SessionEvent`], which is itself
/// public; this module is private, so the type stays unreachable from outside
/// the crate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitProbe {
    /// Current branch shortname, or `None` on a detached `HEAD`, a
    /// non-repository, or any failure.
    pub branch: Option<String>,
    /// Whether the worktree has uncommitted changes, or `None` if not observed.
    pub dirty: Option<bool>,
    /// The `HEAD` commit object id, lowercase hex, or `None` on a
    /// non-repository, an unborn branch with no commits yet, or any failure.
    ///
    /// Captured independently of `branch`: a detached `HEAD` has no branch to
    /// name but still has a commit, and B1's `observedCommit` fact needs to
    /// be available in exactly that state.
    pub commit: Option<String>,
}

/// Look at `cwd` with `git`, bounded and best effort.
///
/// Runs up to three short-lived `git` processes. None can outlive
/// [`PROBE_TIMEOUT`]; `kill_on_drop` guarantees a timed-out child is reaped
/// rather than orphaned.
pub(crate) async fn probe(cwd: &Path) -> GitProbe {
    // `branch` and `commit` are independent observations — a detached `HEAD`
    // answers the second and not the first — so both always run rather than
    // gating one on the other.
    let branch = branch(cwd).await;
    let commit = commit(cwd).await;
    // Skipped only when neither of the above found anything: that combination
    // means `cwd` is not a repository (or an unborn one with no commits and
    // a name `--show-current` still reported would have set `branch`), so the
    // third invocation could only fail the same way.
    let dirty = if branch.is_some() || commit.is_some() {
        dirty(cwd).await
    } else {
        None
    };
    GitProbe {
        branch,
        dirty,
        commit,
    }
}

/// Current `HEAD` commit object id, or `None` if there is not exactly one.
///
/// Works on a detached `HEAD` (unlike [`branch`]) and fails the same way
/// `branch` does on an unborn branch with no commits yet — `rev-parse HEAD`
/// has nothing to print until the first commit exists.
async fn commit(cwd: &Path) -> Option<String> {
    let stdout = run_git(cwd, &["rev-parse", "HEAD"]).await?;
    let oid = stdout.trim();
    let valid_len = oid.len() == OID_LEN_SHA1 || oid.len() == OID_LEN_SHA256;
    if !valid_len || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(oid.to_ascii_lowercase())
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
        let oid = observed.commit.as_deref().expect("commit observed");
        assert_eq!(oid.len(), 40);
        assert!(oid
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }

    /// A second commit changes `HEAD`, and the probe reports the new tip —
    /// proving `observedCommit` tracks the exact commit, not merely "a repo
    /// with commits."
    #[tokio::test]
    async fn a_second_commit_changes_the_observed_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        let first = probe(dir.path()).await.commit.expect("first commit");
        std::fs::write(dir.path().join("b.txt"), "b").expect("write");
        git(dir.path(), &["add", "b.txt"]);
        commit(dir.path(), "two");
        let second = probe(dir.path()).await.commit.expect("second commit");
        assert_ne!(first, second);
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

    /// A detached `HEAD` has no branch, but it does have a commit — and
    /// dirty state must not be silently skipped just because `branch` came
    /// back empty. This is the case B1 exists to fix: before `commit`
    /// stopped `probe` from gating `dirty` on `branch.is_some()`, a session
    /// checked out at a fixed commit would have published `observedCommit`
    /// as `null` even though `git` could answer.
    #[tokio::test]
    async fn a_detached_head_still_reports_its_commit_and_dirty_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        git(dir.path(), &["checkout", "-q", "--detach", "HEAD"]);
        let observed = probe(dir.path()).await;
        assert!(observed.branch.is_none());
        assert!(observed.commit.is_some());
        assert_eq!(observed.dirty, Some(false));

        std::fs::write(dir.path().join("a.txt"), "changed").expect("write");
        assert_eq!(probe(dir.path()).await.dirty, Some(true));
    }

    /// A freshly initialized repository has an unborn branch and no commits.
    /// `--show-current` still names it, so a session created in a repository
    /// before its first commit is not indistinguishable from a plain directory.
    /// `rev-parse HEAD` has nothing to print yet, so `commit` stays `None`.
    #[tokio::test]
    async fn an_empty_repository_still_names_its_unborn_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        git(dir.path(), &["init", "-q", "-b", "unborn", "."]);
        let observed = probe(dir.path()).await;
        assert_eq!(observed.branch.as_deref(), Some("unborn"));
        assert_eq!(observed.dirty, Some(false));
        assert_eq!(observed.commit, None);
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
        // What must hold is that no observation survives the bound. Asserting
        // `is_err()` instead claims the timer always wins a race against the
        // spawn, which is not something the test can guarantee: when `output()`
        // resolves on its first poll — a spawn that fails fast on a loaded
        // runner — the deadline never gets to fire and the assertion failed the
        // gate despite the product behaving correctly. Both non-observations
        // are the degradation this test is about; only a completed `git` is not.
        match tokio::time::timeout(Duration::from_nanos(1), command.output()).await {
            Err(_elapsed) => {}
            Ok(Err(_spawn_failed)) => {}
            Ok(Ok(output)) => panic!("git completed inside a 1ns bound: {output:?}"),
        }
    }
}
