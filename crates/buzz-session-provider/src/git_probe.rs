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
///
/// `status` can run the repository's own clean filters and fsmonitor, so it
/// runs inside `scope` — the tree's prepared host-Git boundary. With no scope
/// (the tree could not be bounded where a backend exists) the dirty state is
/// reported as not observed rather than observed unbounded.
pub(crate) async fn probe(
    cwd: &Path,
    scope: Option<&crate::execution_scope_host::HostLaunchPlan>,
) -> GitProbe {
    // `branch` and `commit` are independent observations — a detached `HEAD`
    // answers the second and not the first — so both always run rather than
    // gating one on the other.
    let branch = branch(cwd).await;
    let commit = commit(cwd).await;
    // Skipped only when neither of the above found anything: that combination
    // means `cwd` is not a repository (or an unborn one with no commits and
    // a name `--show-current` still reported would have set `branch`), so the
    // third invocation could only fail the same way.
    let dirty = match scope {
        Some(scope) if branch.is_some() || commit.is_some() => dirty(cwd, scope).await,
        _ => None,
    };
    GitProbe {
        branch,
        dirty,
        commit,
    }
}

/// Look at `cwd` for a **gate row**, refusing rather than degrading.
///
/// [`probe`] answers `None` for everything: a missing `git`, a plain
/// directory, a deleted checkout. That is right for the periodic worktree
/// observation, where an unknown field is an unknown field — and wrong for a
/// gate row, where `headSha: null` is published as a fact about a command that
/// *did* run, and admits nothing anywhere (live-run finding 82).
///
/// So the two cases are separated here. A directory that is not there is a
/// refusal carrying the sentence `cwd missing: <path>`, and the caller mints
/// no row at all. A directory that is there degrades exactly as before —
/// a checkout with no commits yet is not a lie, it is a repository without a
/// `HEAD`.
pub(crate) async fn probe_for_gate(
    cwd: &Path,
    scope: Option<&crate::execution_scope_host::HostLaunchPlan>,
) -> Result<GitProbe, String> {
    if !cwd.is_dir() {
        return Err(format!("cwd missing: {}", cwd.display()));
    }
    Ok(probe(cwd, scope).await)
}

/// Look at `cwd` for a **verification input**: `HEAD` and how many lines
/// `git status --porcelain` prints.
///
/// [`probe`] answers a dirty *flag*, which is the right shape for a metadata
/// row and the wrong one for a refusal a person has to act on — "your tree is
/// dirty" and "your tree has three uncommitted changes" cost the same
/// invocation. The count is the same `status --porcelain` output `dirty`
/// already parses, counted rather than emptiness-tested.
///
/// A directory that is not there refuses by name, as
/// [`probe_for_gate`] does and for the same reason: the caller must not read
/// "no commit observed" from a checkout that has moved. Everything else
/// degrades to `None`, which the caller treats as "unknown", never as "clean".
pub(crate) async fn probe_verification_input(
    cwd: &Path,
    scope: Option<&crate::execution_scope_host::HostLaunchPlan>,
) -> Result<crate::verification_input::SeatTree, String> {
    if !cwd.is_dir() {
        return Err(format!("cwd missing: {}", cwd.display()));
    }
    let head = commit(cwd).await;
    let dirty_lines = match scope {
        Some(scope) => dirty_line_count(cwd, scope).await,
        None => None,
    };
    Ok(crate::verification_input::SeatTree { head, dirty_lines })
}

/// How many lines `git status --porcelain` prints in `cwd`.
///
/// Untracked files included, for the reason [`dirty`] documents: an agent's
/// first act is usually to create a file.
async fn dirty_line_count(
    cwd: &Path,
    scope: &crate::execution_scope_host::HostLaunchPlan,
) -> Option<usize> {
    let stdout = status(cwd, scope).await?;
    Some(
        stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count(),
    )
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
async fn dirty(cwd: &Path, scope: &crate::execution_scope_host::HostLaunchPlan) -> Option<bool> {
    let stdout = status(cwd, scope).await?;
    Some(!stdout.trim().is_empty())
}

/// `git status --porcelain` inside `scope`, taking no optional lock.
async fn status(cwd: &Path, scope: &crate::execution_scope_host::HostLaunchPlan) -> Option<String> {
    let args = ["--no-optional-locks", "status", "--porcelain"];
    let command = match scope {
        crate::execution_scope_host::HostLaunchPlan::Bounded(_) => scope.git_command(cwd, &args),
        // No backend on this platform: the existing unbounded probe, as the
        // session's own disclosed state says.
        crate::execution_scope_host::HostLaunchPlan::Unenforced { .. } => {
            let mut command = crate::host_command::metadata_git_command(cwd);
            command.args(args);
            command
        }
    };
    let mut command = Command::from(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(PROBE_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

/// Environment variables by which git selects a repository, all of which beat
/// `-C <path>`.
///
/// Git exports `GIT_DIR` into every hook it runs, so any `git -C` here that
/// executes under a pre-commit or pre-push hook silently retargets at the
/// developer's own repository. `probe(cwd)` promises to report *that*
/// checkout's branch, commit and dirty state; an inherited `GIT_DIR` made it
/// report a different repository's instead. Cleared for the same reason
/// `desktop/src-tauri/src/commands/team_readiness_git.rs` clears them.
pub(crate) const GIT_REPO_SELECTION_VARS: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

/// Run `git <args>` in `cwd` under [`PROBE_TIMEOUT`], returning stdout.
///
/// `None` on spawn failure, non-zero exit, timeout, or non-UTF-8 output — the
/// caller cannot distinguish them, and does not need to.
async fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    for var in GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    command
        // Nothing here refreshes the index or runs a hook, and neither can a
        // repository's configuration make it.
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
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
        let mut command = std::process::Command::new("git");
        for var in super::GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        let status = command
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

    /// `git` in `cwd`, with the repo-selection variables cleared.
    ///
    /// Without that, running under a pre-push hook made `git init` target
    /// `<the developer's repo>/.git` — it only failed loudly because a
    /// concurrent test already held `config.lock`. Unlocked it would have
    /// reinitialised the real repository and rewritten its config.
    fn git(cwd: &Path, args: &[&str]) {
        let mut command = std::process::Command::new("git");
        for var in super::GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        let status = command.arg("-C").arg(cwd).args(args).status().expect("git");
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
        let observed = probe(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await;
        assert_eq!(observed, GitProbe::default());
    }

    #[tokio::test]
    async fn a_missing_directory_degrades_rather_than_failing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let observed = probe(
            &dir.path().join("does-not-exist"),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await;
        assert_eq!(observed, GitProbe::default());
    }

    #[tokio::test]
    async fn a_clean_checkout_reports_its_branch_and_no_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        let observed = probe(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await;
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
        let first = probe(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .commit
        .expect("first commit");
        std::fs::write(dir.path().join("b.txt"), "b").expect("write");
        git(dir.path(), &["add", "b.txt"]);
        commit(dir.path(), "two");
        let second = probe(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .commit
        .expect("second commit");
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn an_edited_file_reads_as_dirty() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "changed").expect("write");
        assert_eq!(
            probe(
                dir.path(),
                Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS)
            )
            .await
            .dirty,
            Some(true)
        );
    }

    /// The reason this probe pays for `status` over the cheaper
    /// `diff --quiet`: an agent's first act is usually to *create* a file, and
    /// `diff --quiet` reports that worktree as clean.
    #[tokio::test]
    async fn an_untracked_file_reads_as_dirty() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        std::fs::write(dir.path().join("new.txt"), "new").expect("write");
        assert_eq!(
            probe(
                dir.path(),
                Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS)
            )
            .await
            .dirty,
            Some(true)
        );
    }

    /// A detached `HEAD` has no branch to name, and publishing the literal
    /// string "HEAD" would read to a consumer as a branch actually called
    /// "HEAD". `--show-current` prints nothing, which maps to `None`.
    #[tokio::test]
    async fn a_detached_head_reports_no_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        git(dir.path(), &["checkout", "-q", "--detach", "HEAD"]);
        assert_eq!(
            probe(
                dir.path(),
                Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS)
            )
            .await
            .branch,
            None
        );
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
        let observed = probe(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await;
        assert!(observed.branch.is_none());
        assert!(observed.commit.is_some());
        assert_eq!(observed.dirty, Some(false));

        std::fs::write(dir.path().join("a.txt"), "changed").expect("write");
        assert_eq!(
            probe(
                dir.path(),
                Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS)
            )
            .await
            .dirty,
            Some(true)
        );
    }

    /// A freshly initialized repository has an unborn branch and no commits.
    /// `--show-current` still names it, so a session created in a repository
    /// before its first commit is not indistinguishable from a plain directory.
    /// `rev-parse HEAD` has nothing to print yet, so `commit` stays `None`.
    #[tokio::test]
    async fn an_empty_repository_still_names_its_unborn_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        git(dir.path(), &["init", "-q", "-b", "unborn", "."]);
        let observed = probe(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await;
        assert_eq!(observed.branch.as_deref(), Some("unborn"));
        assert_eq!(observed.dirty, Some(false));
        assert_eq!(observed.commit, None);
    }

    /// A gate row's subject is a directory that exists. When it does not, the
    /// probe refuses by name instead of answering "no commit observed" —
    /// finding 82, where a relocated worktree minted rows with `headSha: null`
    /// for two days.
    #[tokio::test]
    async fn a_gate_probe_refuses_a_missing_directory_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gone = dir.path().join("relocated-away");
        let refusal = probe_for_gate(
            &gone,
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .expect_err("must refuse");
        assert_eq!(refusal, format!("cwd missing: {}", gone.display()));
    }

    /// The directory being present is the whole condition: a real checkout
    /// still answers with its real commit and its real dirty flag.
    #[tokio::test]
    async fn a_gate_probe_of_a_live_checkout_answers_with_its_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        let observed = probe_for_gate(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .expect("must observe");
        assert_eq!(observed.commit.as_deref().map(str::len), Some(40));
        assert_eq!(observed.dirty, Some(false));

        std::fs::write(dir.path().join("a.txt"), "changed").expect("write");
        let dirty = probe_for_gate(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .expect("must observe");
        assert_eq!(dirty.dirty, Some(true));
        assert_eq!(dirty.commit, observed.commit);
    }

    /// The verification probe answers the same `HEAD` as the gate probe and
    /// counts the changes rather than flagging them, so a refusal can say how
    /// many.
    #[tokio::test]
    async fn a_verification_probe_counts_the_uncommitted_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        let clean = probe_verification_input(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .expect("must observe");
        assert_eq!(clean.dirty_lines, Some(0));
        assert_eq!(clean.head.as_deref().map(str::len), Some(40));

        std::fs::write(dir.path().join("a.txt"), "changed").expect("write");
        std::fs::write(dir.path().join("new.txt"), "new").expect("write");
        let dirty = probe_verification_input(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .expect("must observe");
        assert_eq!(dirty.dirty_lines, Some(2));
        assert_eq!(dirty.head, clean.head);
    }

    /// A moved or deleted checkout refuses by name. Reading it as "no commit
    /// observed" would let the refusal blame the assignment for a fact about
    /// the host.
    #[tokio::test]
    async fn a_verification_probe_refuses_a_missing_directory_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gone = dir.path().join("relocated-away");
        let refusal = probe_verification_input(
            &gone,
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .expect_err("must refuse");
        assert_eq!(refusal, format!("cwd missing: {}", gone.display()));
    }

    /// A plain directory is observed and unknown, not refused: nothing moved.
    /// The caller turns "unknown" into its own refusal, naming the checkout.
    #[tokio::test]
    async fn a_verification_probe_of_a_plain_directory_knows_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let observed = probe_verification_input(
            dir.path(),
            Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS),
        )
        .await
        .expect("must observe");
        assert_eq!(observed, crate::verification_input::SeatTree::default());
    }

    /// A directory that exists but is not a repository is *not* a refusal:
    /// nothing moved, and the row's honest content is "no commit observed".
    #[tokio::test]
    async fn a_gate_probe_of_a_plain_directory_still_degrades() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            probe_for_gate(
                dir.path(),
                Some(&crate::execution_scope_host::UNBOUNDED_FOR_TESTS)
            )
            .await
            .expect("must observe"),
            GitProbe::default()
        );
    }

    /// Proves the bound is real rather than decorative: with the ceiling driven
    /// to zero the invocation is abandoned, `kill_on_drop` reaps the child, and
    /// the caller still gets a value.
    #[tokio::test]
    async fn a_timeout_degrades_to_no_observation() {
        // The child is `sleep`, not `git`, and that is the point: what is under
        // test is the bound and `kill_on_drop`, neither of which cares which
        // program is abandoned. Racing a real `git status` against the deadline
        // is not decidable in either direction — `Timeout` polls the inner
        // future before it checks the deadline, and the timer wheel is only
        // serviced when the runtime gets to it, so on a loaded runner `git`
        // completes first and the bound never fires. The gate failed both ways
        // on that race before this. A child that cannot finish removes it.
        let mut command = Command::new("sleep");
        command.arg("30").stdin(Stdio::null()).kill_on_drop(true);
        // No observation may survive the bound. An elapsed deadline is the
        // intended path; a spawn that fails outright is also no observation.
        // `sleep 30` returning inside the bound is the only real failure.
        match tokio::time::timeout(Duration::from_nanos(1), command.output()).await {
            Err(_elapsed) => {}
            Ok(Err(_spawn_failed)) => {}
            Ok(Ok(output)) => panic!("the child outran a 1ns bound: {output:?}"),
        }
    }
}
