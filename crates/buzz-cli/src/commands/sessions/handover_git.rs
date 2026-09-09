//! The git half of `bee sessions handover`: reading a worktree honestly, and
//! putting one back together somewhere else.
//!
//! # Complete or enumerated
//!
//! The rule this module exists to keep (`docs/HANDOVER_IMPL.md` §4) is that a
//! working-tree capture is either **complete** or **enumerated**. Every byte
//! that is not in the patch is named by path, with the reason, so the
//! checkpoint can say `preserved: "partial"` and mean it. There is no path
//! through [`capture_working_tree`] that silently drops a file: a path that
//! exceeds a bound is excluded from the diff *by name* and returned in
//! [`CapturedTree::omitted`].
//!
//! The capture itself is one `git diff` against a **temporary index**, so
//! staged, unstaged, untracked and binary content all land in a single patch
//! bound to one base commit and the caller's real index is never touched:
//!
//! ```text
//! GIT_INDEX_FILE=<tmp>  git read-tree <headSha>
//! GIT_INDEX_FILE=<tmp>  git add -A
//! GIT_INDEX_FILE=<tmp>  git diff --cached --binary --full-index <headSha>
//! ```
//!
//! # Two things it will not do
//!
//! * **Force-push.** The wip namespace tolerates force in the seat's
//!   post-commit hook because that hook only ever pushes `HEAD` of a ref it
//!   derived. A checkpoint is published by a person or an agent at an
//!   arbitrary moment, so this pushes without `--force` and reports the
//!   refusal rather than overwriting somebody's ref.
//! * **Name a remote.** CLAUDE.md is explicit — two pre-push guards
//!   hard-coded one and both broke silently the day the remote names moved —
//!   so [`resolve_push_remote`] walks git's own push configuration in the same
//!   order `scripts/wip-post-commit.sh` does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CliError;

use super::worktree::git_command;

/// A scratch file inside the repository's own git directory, removed on drop.
///
/// Deliberately not a system temp directory: `tempfile` is a dev-dependency of
/// this crate and adding it to the production graph for two scratch paths is a
/// bigger change than this needs. The git directory is already writable by
/// whoever is running the command, is on the same filesystem as the objects
/// git is about to read, and — being outside the worktree — cannot be picked
/// up by the `git add -A` this module runs.
struct GitScratch {
    path: PathBuf,
}

impl GitScratch {
    /// Reserve a uniquely named scratch path under `cwd`'s git directory.
    fn new(cwd: &Path, prefix: &str) -> Result<Self, String> {
        let git_dir = run_git(cwd, &["rev-parse", "--absolute-git-dir"])?;
        let git_dir = PathBuf::from(git_dir.trim());
        let unique = uuid::Uuid::new_v4();
        Ok(Self {
            path: git_dir.join(format!("buzz-{prefix}-{unique}")),
        })
    }

    /// The reserved path.
    fn path(&self) -> &Path {
        &self.path
    }

    /// The reserved path as UTF-8, which git needs it to be.
    fn path_str(&self) -> Result<&str, String> {
        self.path
            .to_str()
            .ok_or_else(|| "scratch path is not valid UTF-8".to_owned())
    }
}

impl Drop for GitScratch {
    fn drop(&mut self) {
        // Best effort: a leftover scratch file in `.git/` is untidy, and
        // failing a completed capture over it would be worse.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Largest single file the capture will carry, in bytes.
///
/// A file above this is omitted **by path** and listed under the checkpoint's
/// `missing`, which is what makes `preserved: "partial"` a fact rather than a
/// hedge (`docs/HANDOVER_IMPL.md` §4).
pub const MAX_CAPTURE_FILE_BYTES: u64 = 256 * 1024;

/// Largest whole patch the capture will carry, in bytes.
pub const MAX_CAPTURE_PATCH_BYTES: u64 = 1024 * 1024;

/// What the worktree said about the revision the work sits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRevision {
    /// The commit `HEAD` points at, lowercase hex.
    pub head_sha: String,
    /// The checked-out branch, or `None` on a detached `HEAD`.
    pub branch: Option<String>,
    /// Merge base with the default integration branch, when one resolves.
    pub base_sha: Option<String>,
    /// Whether anything was uncommitted — staged, unstaged or untracked.
    pub dirty: bool,
}

/// One path the capture deliberately left out, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmittedPath {
    /// Repository-relative path, exactly as git spells it.
    pub path: String,
    /// The reason, in a sentence naming the bound that excluded it.
    pub reason: String,
}

impl OmittedPath {
    /// The single `missing` line this omission contributes to a checkpoint.
    pub fn line(&self) -> String {
        format!("{}: {}", self.path, self.reason)
    }
}

/// A working-tree capture: one patch, plus everything it does not contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedTree {
    /// The unified diff against `head_sha`, empty when nothing was captured.
    pub patch: String,
    /// Paths excluded from `patch`, each with the bound that excluded it.
    pub omitted: Vec<OmittedPath>,
    /// Every path that differed from `head_sha`, whether captured or not.
    pub changed_paths: Vec<String>,
    /// Ignored paths present in the worktree, collapsed to directories, and
    /// how many there were.
    ///
    /// **Why a capture reports what it deliberately did not take.** `git add
    /// -A` honours `.gitignore` and `git status --porcelain` does not list
    /// ignored files at all, so a worktree holding a modified `.env` reads as
    /// clean, captures nothing, and would have signed `preserved: "all"` —
    /// a checkpoint claiming every byte travelled while the one file the next
    /// participant actually needs sat on the old machine. The capture cannot
    /// carry those bytes (they are ignored for good reasons, and some are
    /// secrets), so it names them instead.
    pub ignored: IgnoredPaths,
}

/// Ignored paths a capture saw and did not take.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IgnoredPaths {
    /// Up to [`MAX_IGNORED_PATHS_LISTED`] entries, directories collapsed.
    pub listed: Vec<String>,
    /// How many entries `git ls-files` reported in total.
    pub total: usize,
}

impl IgnoredPaths {
    /// Whether the worktree holds anything git is ignoring.
    pub fn any(&self) -> bool {
        self.total > 0
    }

    /// The single `missing` line these contribute to a checkpoint.
    pub fn line(&self) -> String {
        format!(
            "{} path(s) ignored by .gitignore, not captured — they are not in the patch and not \
             in any commit ({}{}): if the next participant needs any of them, hand them over \
             out of band",
            self.total,
            self.listed.join(", "),
            if self.total > self.listed.len() {
                ", …"
            } else {
                ""
            }
        )
    }
}

/// How many ignored entries a checkpoint enumerates before it says "and more".
pub const MAX_IGNORED_PATHS_LISTED: usize = 8;

impl CapturedTree {
    /// Whether the patch carries bytes at all.
    pub fn has_patch(&self) -> bool {
        !self.patch.trim().is_empty()
    }
}

/// Run one `git` invocation in `cwd`, returning stdout on success.
///
/// Errors carry git's own stderr: a capture that fails because the directory
/// is not a repository should say so in git's words rather than this module's.
fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    run_git_with_env(cwd, args, &BTreeMap::new())
}

/// [`run_git`] with extra environment variables — used for `GIT_INDEX_FILE`.
fn run_git_with_env(
    cwd: &Path,
    args: &[&str],
    env: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut command: Command = git_command(cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
        .args(args)
        .output()
        .map_err(|error| format!("could not run git {args:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The branches a base commit is looked for against, newest convention first.
///
/// Tried in order and the first that resolves wins; when none does, `baseSha`
/// is `null`, which is an answer and not a failure — a repository with no
/// integration branch reachable from here has no base to name.
const BASE_CANDIDATES: [&str; 4] = ["main", "origin/main", "master", "origin/master"];

/// Read the revision facts a checkpoint records.
///
/// # Errors
/// When `cwd` is not a git worktree, or `HEAD` names no commit.
pub fn read_revision(cwd: &Path) -> Result<WorktreeRevision, CliError> {
    let head_sha = run_git(cwd, &["rev-parse", "HEAD"])
        .map(|out| out.trim().to_ascii_lowercase())
        .map_err(|error| {
            CliError::Usage(format!(
                "--cwd {} has no readable HEAD, so there is no revision to check point: {error}",
                cwd.display()
            ))
        })?;
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|out| out.trim().to_owned())
        .filter(|name| !name.is_empty() && name != "HEAD");
    let base_sha = BASE_CANDIDATES.iter().find_map(|candidate| {
        run_git(cwd, &["merge-base", candidate, "HEAD"])
            .ok()
            .map(|out| out.trim().to_ascii_lowercase())
            .filter(|sha| !sha.is_empty())
    });
    let dirty = !run_git(cwd, &["status", "--porcelain"])
        .map_err(|error| CliError::Other(format!("could not read the worktree's status: {error}")))?
        .trim()
        .is_empty();
    Ok(WorktreeRevision {
        head_sha,
        branch,
        base_sha,
        dirty,
    })
}

/// Resolve the remote to push a wip ref to, without ever naming one.
///
/// The exact ladder `scripts/wip-post-commit.sh` walks: an explicit
/// `buzz.wipRemote`, the branch's `pushRemote`, `remote.pushDefault`, the
/// branch's `remote`, and finally the sole remote when there is exactly one.
/// `None` means git's own configuration does not say, which is reported rather
/// than guessed at.
pub fn resolve_push_remote(cwd: &Path, branch: Option<&str>) -> Option<String> {
    let config = |key: &str| -> Option<String> {
        run_git(cwd, &["config", "--get", key])
            .ok()
            .map(|out| out.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    if let Some(remote) = config("buzz.wipRemote") {
        return Some(remote);
    }
    if let Some(branch) = branch {
        if let Some(remote) = config(&format!("branch.{branch}.pushRemote")) {
            return Some(remote);
        }
    }
    if let Some(remote) = config("remote.pushDefault") {
        return Some(remote);
    }
    if let Some(branch) = branch {
        if let Some(remote) = config(&format!("branch.{branch}.remote")) {
            return Some(remote);
        }
    }
    let remotes = run_git(cwd, &["remote"]).ok()?;
    let mut names = remotes
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let only = names.next()?;
    names.next().is_none().then(|| only.to_owned())
}

/// Push `HEAD` to `ref_name` on `remote`, never with `--force`.
///
/// `--no-verify` because a pre-push hook that runs a test suite would turn
/// "record where this work stands" into a gate; the ref is in the wip
/// namespace, which nothing integrates from.
///
/// # Errors
/// git's own message, so a rejected non-fast-forward reads as one.
pub fn push_wip_ref(cwd: &Path, remote: &str, ref_name: &str) -> Result<(), String> {
    if !ref_name.starts_with("refs/heads/wip/") {
        return Err(format!(
            "refusing to push {ref_name}: a handover checkpoint only ever writes the wip namespace"
        ));
    }
    run_git(
        cwd,
        &["push", "--no-verify", remote, &format!("HEAD:{ref_name}")],
    )
    .map(|_| ())
}

/// Capture the whole working tree as one patch against `head_sha`.
///
/// Complete or enumerated: every path that differs from `head_sha` is either
/// in [`CapturedTree::patch`] or in [`CapturedTree::omitted`] with the bound
/// that excluded it. Two bounds apply, in this order — the per-file bound
/// first, so one enormous file cannot evict a dozen small ones, then the
/// whole-patch bound, which evicts the largest remaining paths until the patch
/// fits and names each one.
///
/// # Errors
/// When the temporary index cannot be created, or git refuses to read the
/// tree — never because a file was too big, which is an outcome and not a
/// failure.
pub fn capture_working_tree(
    cwd: &Path,
    head_sha: &str,
    max_file_bytes: u64,
    max_patch_bytes: u64,
) -> Result<CapturedTree, CliError> {
    let index = GitScratch::new(cwd, "handover-index").map_err(|error| {
        CliError::Other(format!(
            "could not reserve a temporary index for the capture: {error}"
        ))
    })?;
    let index_path = index.path_str().map_err(CliError::Other)?.to_owned();
    let mut env = BTreeMap::new();
    env.insert("GIT_INDEX_FILE".to_owned(), index_path);

    // Seed the temporary index from HEAD so a file deleted in the worktree is
    // captured as a deletion rather than silently absent, then stage
    // everything the repository is not ignoring.
    run_git_with_env(cwd, &["read-tree", head_sha], &env)
        .map_err(|error| CliError::Other(format!("could not seed the capture index: {error}")))?;
    run_git_with_env(cwd, &["add", "-A"], &env)
        .map_err(|error| CliError::Other(format!("could not stage the worktree: {error}")))?;

    let changed_paths = changed_paths(cwd, head_sha, &env)?;
    let mut omitted: Vec<OmittedPath> = Vec::new();
    let mut carried: Vec<String> = Vec::new();
    for path in &changed_paths {
        let size = worktree_file_bytes(cwd, path);
        if size > max_file_bytes {
            omitted.push(OmittedPath {
                path: path.clone(),
                reason: format!(
                    "{size} bytes exceeds the {max_file_bytes}-byte per-file capture bound, so \
                     its uncommitted content stays on this machine"
                ),
            });
        } else {
            carried.push(path.clone());
        }
    }

    let mut patch = diff_excluding(cwd, head_sha, &omitted, &env)?;
    // The whole-patch bound. Largest first so the fewest paths are dropped,
    // and each eviction is re-measured rather than estimated.
    while patch.len() as u64 > max_patch_bytes && !carried.is_empty() {
        let mut sizes: Vec<(u64, String)> = carried
            .iter()
            .map(|path| (worktree_file_bytes(cwd, path), path.clone()))
            .collect();
        sizes.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        let (size, path) = match sizes.first() {
            Some(entry) => entry.clone(),
            None => break,
        };
        carried.retain(|held| held != &path);
        omitted.push(OmittedPath {
            path,
            reason: format!(
                "dropped at {size} bytes to keep the capture under the {max_patch_bytes}-byte \
                 patch bound, so its uncommitted content stays on this machine"
            ),
        });
        patch = diff_excluding(cwd, head_sha, &omitted, &env)?;
    }

    omitted.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(CapturedTree {
        patch,
        omitted,
        changed_paths,
        ignored: list_ignored_paths(cwd),
    })
}

/// Ignored paths present in the worktree, collapsed to directories.
///
/// `--directory` so `target/` and `node_modules/` are one entry each rather
/// than a hundred thousand, and the list is bounded — a checkpoint enumerates
/// what a person can act on, and the total says how much more there was.
/// A read that fails answers "none reported", because a capture must not fail
/// over a disclosure it could not make; the count is then honestly zero rather
/// than a guess.
fn list_ignored_paths(cwd: &Path) -> IgnoredPaths {
    let Ok(raw) = run_git(
        cwd,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ],
    ) else {
        return IgnoredPaths::default();
    };
    let entries: Vec<String> = raw
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    IgnoredPaths {
        listed: entries
            .iter()
            .take(MAX_IGNORED_PATHS_LISTED)
            .cloned()
            .collect(),
        total: entries.len(),
    }
}

/// Every path the temporary index says differs from `head_sha`.
fn changed_paths(
    cwd: &Path,
    head_sha: &str,
    env: &BTreeMap<String, String>,
) -> Result<Vec<String>, CliError> {
    let raw = run_git_with_env(
        cwd,
        &["diff", "--cached", "--name-only", "-z", head_sha],
        env,
    )
    .map_err(|error| CliError::Other(format!("could not list the captured paths: {error}")))?;
    Ok(raw
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The diff against `head_sha` with every omitted path excluded by name.
fn diff_excluding(
    cwd: &Path,
    head_sha: &str,
    omitted: &[OmittedPath],
    env: &BTreeMap<String, String>,
) -> Result<String, CliError> {
    let mut args: Vec<String> = vec![
        "diff".to_owned(),
        "--cached".to_owned(),
        "--binary".to_owned(),
        "--full-index".to_owned(),
        head_sha.to_owned(),
    ];
    if !omitted.is_empty() {
        args.push("--".to_owned());
        args.push(".".to_owned());
        for entry in omitted {
            args.push(format!(":(exclude,literal,top){}", entry.path));
        }
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git_with_env(cwd, &borrowed, env)
        .map_err(|error| CliError::Other(format!("could not produce the capture patch: {error}")))
}

/// Size of `path` in the worktree, or 0 when it no longer exists.
///
/// A deletion contributes no bytes, so it is never the reason a capture
/// exceeds a bound.
fn worktree_file_bytes(cwd: &Path, path: &str) -> u64 {
    std::fs::symlink_metadata(cwd.join(path))
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map_or(0, |metadata| metadata.len())
}

/// Fetch `ref_name` from `remote` and check `sha` out on `branch`.
///
/// `git checkout -B` so a rerun lands on the same branch rather than failing
/// on "already exists" — the idempotent rerun §4 requires — and the sha is
/// verified to exist locally after the fetch before anything is checked out.
///
/// The branch is the **session's** (`handover/<session8>`), so a second
/// reconstruction lands on the same one a first left behind. That is what
/// makes [`refuse_unreachable_branch`] necessary rather than theoretical.
///
/// # Errors
/// git's own message from the first step that failed, or the containment
/// refusal.
pub fn fetch_and_checkout(
    cwd: &Path,
    remote: &str,
    ref_name: &str,
    sha: &str,
    branch: &str,
) -> Result<(), String> {
    // Both untrusted: the ref comes out of somebody else's signed checkpoint
    // and the remote out of git config, and both reach a subprocess argv.
    let ref_name = safe_wip_ref(ref_name)?;
    let remote = safe_remote_name(remote)?;
    // A **destination-free** refspec, `<src>:`. `git fetch <remote> <src>:<dst>`
    // writes `<dst>` in this repository, so the empty destination is what makes
    // the fetch a read: the objects land, FETCH_HEAD is suppressed, and no
    // local ref moves.
    run_git(
        cwd,
        &[
            "fetch",
            "--no-write-fetch-head",
            &remote,
            &format!("{ref_name}:"),
        ],
    )?;
    let resolved = run_git(
        cwd,
        &["rev-parse", "--verify", &format!("{sha}^{{commit}}")],
    )
    .map_err(|error| {
        format!("{sha} is not present after fetching {ref_name} from {remote}: {error}")
    })?;
    let resolved = resolved.trim().to_ascii_lowercase();
    if !resolved.starts_with(&sha.to_ascii_lowercase()) {
        return Err(format!(
            "fetched {ref_name} resolves to {resolved}, not the checkpoint's {sha}"
        ));
    }
    refuse_unreachable_branch(cwd, branch, &resolved)?;
    run_git(cwd, &["checkout", "-B", branch, &resolved]).map(|_| ())
}

/// Characters a ref this command fetches may never contain.
///
/// `:` is the one that matters. `git fetch <remote> <src>:<dst>` **writes
/// `<dst>` in this repository**, so a checkpoint whose artifact named
/// `+refs/heads/wip/x:refs/heads/main` would rewrite the caller's own `main` —
/// and it would do it during the fetch, before the sha check below could
/// refuse anything. `+` forces that write past a non-fast-forward. The rest
/// (`~ ^ ? * [ \`) are revision or glob syntax `git check-ref-format` refuses
/// anyway.
///
/// Kind 44247 validates artifact refs for **length and text only** — the relay
/// adjudicates structure, not meaning — so this is where the shape is
/// enforced, exactly as `desktop/src-tauri/src/commands/handover.rs` enforces
/// it for the same value arriving the same way.
const REFUSED_REF_CHARACTERS: [char; 8] = [':', '+', '~', '^', '?', '*', '[', '\\'];

/// A wip ref and nothing else: `refs/heads/<name>`, conservatively spelled.
///
/// Deliberately narrower than git's own rules. The only shape this command has
/// any business fetching is the branch ref a seat hook pushes.
fn safe_wip_ref(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let name = trimmed.strip_prefix("refs/heads/").unwrap_or("");
    if name.is_empty()
        || trimmed.contains("..")
        || trimmed.ends_with('/')
        || trimmed.contains("//")
        || trimmed.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || REFUSED_REF_CHARACTERS.contains(&character)
        })
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._/-".contains(character))
    {
        return Err(format!(
            "refusing to fetch {value:?}: a checkpoint artifact's ref must be a plain \
             refs/heads/… branch ref, and a ref carrying a destination (`src:dst`) would write a \
             local branch during the fetch, before any check here could refuse it"
        ));
    }
    Ok(trimmed.to_owned())
}

/// A remote name, under the same rules minus the `refs/heads/` prefix.
///
/// Resolved from git config rather than from a checkpoint, but it lands in the
/// same argv position and a config value beginning `-` would be read as a flag.
fn safe_remote_name(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('-')
        || trimmed.contains("..")
        || trimmed.ends_with('/')
        || trimmed.contains("//")
        || trimmed.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || REFUSED_REF_CHARACTERS.contains(&character)
        })
        || !trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._/-".contains(character))
    {
        return Err(format!(
            "refusing to fetch from remote {value:?}: a remote name must be a plain \
             alphanumeric name"
        ));
    }
    Ok(trimmed.to_owned())
}

/// Refuse to move `branch` onto `target` when that would discard commits.
///
/// `git checkout -B` **moves** an existing branch, taking whatever was on it
/// with no word to anybody. A second reconstruction into a checkout where the
/// first one already committed would therefore throw that work away — and with
/// the branch named after the session (one session, one branch) that is the
/// ordinary case rather than an exotic one.
///
/// A branch that does not exist, or whose tip is contained in `target`
/// (an ancestor of it, or equal to it), is safe to place: nothing is lost, and
/// that is the rerun this command promises. Anything else is somebody's work,
/// and the refusal names both the branch and its tip so it can be recovered.
fn refuse_unreachable_branch(cwd: &Path, branch: &str, target: &str) -> Result<(), String> {
    let Ok(tip) = run_git(
        cwd,
        &["rev-parse", "--verify", &format!("{branch}^{{commit}}")],
    ) else {
        return Ok(());
    };
    let tip = tip.trim().to_ascii_lowercase();
    if tip == target {
        return Ok(());
    }
    if run_git(cwd, &["merge-base", "--is-ancestor", &tip, target]).is_ok() {
        return Ok(());
    }
    Err(format!(
        "branch {branch} already exists at {tip}, which is not contained in {target}: checking \
         out there would discard those commits, and a previous reconstruction's work is not this \
         command's to throw away. Merge or rename {branch} first, or re-run against a checkout \
         that does not hold it."
    ))
}

/// Apply a captured patch onto the current checkout, or change nothing.
///
/// Two guards, because one is not enough. `git apply --check` runs first, so a
/// patch that plainly does not fit returns before anything is written. But
/// `--3way` can pass that check and then leave **conflict markers** — git
/// prints "Applied patch to 'x' with conflicts" and exits non-zero — which is
/// exactly the half-applied tree §4 forbids. So the whole worktree is
/// snapshotted into a tree object first and restored on any failure.
///
/// # Errors
/// git's own message, prefixed by what state the checkout was left in. A
/// restore that itself fails is reported rather than hidden: the caller is
/// then holding a tree somebody has to look at, and being told so is the
/// difference between a recoverable mess and a silent one.
pub fn apply_patch(cwd: &Path, patch: &str) -> Result<(), String> {
    if patch.trim().is_empty() {
        return Ok(());
    }
    let scratch = GitScratch::new(cwd, "handover-patch")?;
    std::fs::write(scratch.path(), patch)
        .map_err(|error| format!("could not write the patch to apply: {error}"))?;
    let patch_arg = scratch.path_str()?;
    run_git(cwd, &["apply", "--check", "--binary", "--3way", patch_arg]).map_err(|error| {
        format!(
            "the checkpoint's patch does not apply to this checkout, so nothing was applied: \
             {error}"
        )
    })?;

    let snapshot = TreeSnapshot::take(cwd)?;
    match run_git(cwd, &["apply", "--binary", "--3way", "--index", patch_arg]) {
        Ok(_) => Ok(()),
        Err(error) => match snapshot.restore(cwd) {
            Ok(()) => Err(format!(
                "the checkpoint's patch does not apply cleanly to this checkout, so nothing was \
                 applied and the tree was restored to how it was: {error}"
            )),
            Err(restore_error) => Err(format!(
                "the checkpoint's patch failed to apply ({error}) AND the checkout could not be \
                 restored ({restore_error}): this working tree is half-applied and needs a person"
            )),
        },
    }
}

/// The whole checkout as it stood, in the two parts a restore must put back.
///
/// **Both parts, because the tree alone is not the state.** A first cut
/// restored only the worktree, with `read-tree --reset -u <tree>` against the
/// real index — and that tree came from `add -A`, so everything the caller had
/// left *unstaged* came back **staged**. `git status` then read differently
/// before and after a failed apply, which is precisely the "the tree was
/// restored to how it was" claim the error message makes. So the raw index
/// file is copied too and written back byte-for-byte (REVIEW S6).
struct TreeSnapshot {
    /// Tree object of the worktree, tracked and untracked alike.
    tree: String,
    /// The exact bytes of the repository's index, or `None` when there was no
    /// index file — a distinction that matters, because restoring an index
    /// onto a repository that had none is not a restore.
    index_bytes: Option<Vec<u8>>,
    /// Where that index file lives.
    index_path: PathBuf,
}

impl TreeSnapshot {
    /// Capture the worktree and the index before anything is written.
    fn take(cwd: &Path) -> Result<Self, String> {
        let index_path = PathBuf::from(
            run_git(cwd, &["rev-parse", "--absolute-git-dir"])?
                .trim()
                .to_owned(),
        )
        .join("index");
        let index_bytes = std::fs::read(&index_path).ok();
        Ok(Self {
            tree: snapshot_tree(cwd)?,
            index_bytes,
            index_path,
        })
    }

    /// Put the worktree and the index back exactly as they were.
    ///
    /// Worktree first: `read-tree --reset -u` decides what to remove from the
    /// working directory by comparing against the index it is writing, so the
    /// saved index bytes go back **after** it, overwriting whatever that step
    /// left behind.
    fn restore(&self, cwd: &Path) -> Result<(), String> {
        run_git(cwd, &["read-tree", "--reset", "-u", &self.tree])?;
        match &self.index_bytes {
            Some(bytes) => std::fs::write(&self.index_path, bytes)
                .map_err(|error| format!("could not restore the index file: {error}")),
            None => match std::fs::remove_file(&self.index_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!("could not remove the restored index: {error}")),
            },
        }
    }
}

/// Write the whole working tree — tracked, staged and untracked alike — into a
/// tree object, without touching the caller's index.
///
/// The same temporary-index technique the capture uses, for the same reason:
/// `git write-tree` against the real index would see only what is staged, and
/// a restore from that would discard everything that was not.
fn snapshot_tree(cwd: &Path) -> Result<String, String> {
    let index = GitScratch::new(cwd, "handover-restore-index")?;
    let mut env = BTreeMap::new();
    env.insert("GIT_INDEX_FILE".to_owned(), index.path_str()?.to_owned());
    if let Ok(head) = run_git(cwd, &["rev-parse", "HEAD"]) {
        run_git_with_env(cwd, &["read-tree", head.trim()], &env)?;
    }
    run_git_with_env(cwd, &["add", "-A"], &env)?;
    Ok(run_git_with_env(cwd, &["write-tree"], &env)?
        .trim()
        .to_owned())
}

#[cfg(test)]
#[path = "handover_git_tests.rs"]
mod tests;

#[cfg(test)]
use buzz_core::coding_session_handover::CodingSessionHandoverPreserved;
