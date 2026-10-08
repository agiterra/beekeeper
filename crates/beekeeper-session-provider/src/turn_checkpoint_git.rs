//! The git half of a turn checkpoint (SV-28/SV-30): write the working tree
//! as a tree object without touching the person's index, `HEAD` or branches,
//! pin it under a hidden ref, and list what changed between two such trees.
//!
//! # The capture
//!
//! The same plumbing T3 Code uses (`apps/server/src/vcs/GitVcsDriver.ts`
//! `captureCheckpoint`) and `bee sessions handover` already uses for its
//! patch (`beekeeper-cli/src/commands/sessions/handover_git.rs`):
//!
//! ```text
//! cp <real index> <git-dir>/beekeeper-checkpoint-index-<uuid>
//! GIT_INDEX_FILE=<tmp>  git read-tree --reset HEAD               (then mtime − 1 s)
//! GIT_INDEX_FILE=<tmp>  git add -A --sparse --pathspec-from-file=-   (. and the exclusions)
//! GIT_INDEX_FILE=<tmp>  git write-tree
//!                       git commit-tree --no-gpg-sign [-p HEAD] <tree>
//!                       git update-ref refs/beekeeper/checkpoints/<session>/<generation>/<leaf> <commit> 0000…
//! ```
//!
//! **Seeding by copy, measured.** Seeding the scratch index with `read-tree
//! HEAD` throws away the stat cache, so `add -A` rehashes every tracked file:
//! 1,183 ms p50 on a 6,958-file `git clone --local` of this repository,
//! against 163 ms for a copy of the real index (2026-10-04, ten warm runs
//! each after a warm-up, index refreshed by `git status`; `git status` itself
//! is 40 ms there). Right after a mass checkout — a fresh clone, a branch
//! switch — every entry is racy and even the copy rehashes: 1.2–1.6 s p50,
//! still inside [`BASELINE_CAPTURE_TIMEOUT`].
//! The copy is reset to `HEAD` (`read-tree --reset`, as T3 does), so it keeps
//! the real index's stat data only where an entry already matches `HEAD` and
//! carries none of the person's staged or unmerged state; its mtime is then
//! set one second below the original's so git's racy-entry check still
//! re-reads any file written in the second the real index was. A copied index carries the
//! person's `assume-unchanged` (and, outside a sparse checkout,
//! `skip-worktree`) flags, which would hide edits from `add -A`, so an index
//! holding either is replaced by a fresh `read-tree HEAD` — slower, correct.
//!
//! **Sparse checkouts (SV-51).** `add -A` runs with `--sparse`: without it git
//! refuses outright (exit 1) when any untracked file sits outside the
//! sparse-checkout definition, and no capture is made. `--sparse` only lets
//! such a file be staged; an entry the checkout marks skip-worktree keeps its
//! `HEAD` content, because the seed keeps those marks. A fresh `read-tree
//! HEAD` would not keep them — every file outside the cone would read as
//! deleted — so a sparse checkout whose index cannot be copied is refused
//! rather than captured wrong.
//!
//! **Off the runtime (SV-52).** The per-file checks (size and readability of
//! untracked files, readability of tracked ones) and the index copy are
//! blocking filesystem work, so they run under `spawn_blocking`, bounded by
//! the same deadline as the capture: a timeout cannot cancel a blocking
//! thread, so the work checks the deadline between files itself, and a copy
//! that finishes after the capture was abandoned removes what it wrote.
//!
//! # What it will not do
//!
//! - **Run a hook or sign.** Every invocation carries `core.hooksPath=/dev/null`
//!   and `core.fsmonitor=false`, so neither the seat's `post-commit` wip push
//!   nor a `reference-transaction` or `post-index-change` hook runs, and
//!   `commit-tree` gets `--no-gpg-sign`: seats configure a Nostr signing
//!   program, and `commit-tree` honours `commit.gpgSign`.
//! - **Depend on the host's identity.** Author and committer are given
//!   explicitly, so a host without `user.email` still captures.
//! - **Drop a file silently.** The capture is complete or enumerated, the
//!   handover rule: an untracked file over [`MAX_UNTRACKED_FILE_BYTES`] is
//!   excluded by literal pathspec — its exact bytes, so a name that is not
//!   UTF-8 is excluded too — and named in [`CapturedTree::omitted`] (or, when
//!   its name is not UTF-8, counted in [`CapturedTree::omitted_not_listed`]);
//!   so is a file the host cannot read.
//! - **Move a checkpoint ref.** A leaf already pinned is kept and reported as
//!   [`CaptureFailure::already_pinned`]; the ref is written create-only
//!   (`turn_checkpoint_git_pin.rs`).
//! - **Publish a host path.** Nothing returned carries an absolute path: tree
//!   and commit ids, a branch shortname, repo-relative paths, and failure
//!   sentences composed here rather than copied from git's stderr.
//! - **Leave its scratch index behind**, on success, failure or timeout: a drop
//!   guard removes it and its `.lock`.
//!
//! # Where each step runs
//!
//! `add -A` can run the project's own clean filters, so it — and every other
//! step that reads the tree — runs inside the tree's prepared host-Git
//! boundary ([`HostLaunchPlan::git_command`]), the way `git_probe::status`
//! does. Inside that boundary a linked worktree may write its own Git
//! administration and the shared object store, so the scratch index lives in
//! `--git-dir` (the worktree's own administration; the same directory as the
//! common dir for an ordinary checkout) and the objects land where they
//! should. The boundary does **not** let a seat write shared refs other than
//! its own branch (`execution_scope_git.rs`), which is right: a checkpoint
//! ref the agent could move or delete would be worth nothing as a record. So
//! the one ref write, `update-ref`, is done by the host itself with hooks and
//! fsmonitor off ([`crate::host_command::metadata_git_command`]) — a command
//! that executes no project code, exactly like the host's write to
//! `info/exclude` in `git_exclude.rs`. The boundary's grants are unchanged.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::execution_scope_host::HostLaunchPlan;

#[path = "turn_checkpoint_git_blocking.rs"]
mod blocking;
#[path = "turn_checkpoint_git_diff.rs"]
mod diff;
#[path = "turn_checkpoint_git_omit.rs"]
mod omit;
#[path = "turn_checkpoint_git_pin.rs"]
mod pin;
#[path = "turn_checkpoint_restore.rs"]
pub(crate) mod restore;
#[path = "turn_checkpoint_git_run.rs"]
mod run;

use blocking::{off_runtime, Deadline};
pub(crate) use diff::DiffResult;
pub(crate) use diff::{diff_tree_files, ChangedFile, FileChange};
#[cfg(test)]
use diff::{merge_diff, parse_name_status, parse_numstat};
#[cfg(test)]
use omit::{exclusion_pathspecs, normalize_omitted, Exclusion};
use omit::{recovery_exclusions, scan_untracked, stage, ReportedOmissions};
pub(crate) use pin::PinnedCheckpoint;
use run::{Git, StepError};

/// Ceiling on the baseline capture taken before a turn's prompt is sent.
///
/// The prompt waits on it, so it is short: past it the turn runs with no
/// baseline and the checkpoint says so (`baseTree: null`).
pub(crate) const BASELINE_CAPTURE_TIMEOUT: Duration = Duration::from_secs(3);

/// Ceiling on the capture taken after a turn ends.
///
/// Nothing waits on it — it is spawned — so it is generous enough for a cold
/// checkout full of new files while still bounding a wedged `git`.
pub(crate) const END_CAPTURE_TIMEOUT: Duration = Duration::from_secs(60);

/// Ceiling on the capture a rewind takes before it touches any file.
///
/// The rewind waits on it (off the loop) and refuses `files: restore` when it
/// fails, so it is bounded tighter than a turn end and looser than a baseline.
pub(crate) const PRE_REWIND_CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);

/// Ceiling on listing the files changed between two captured trees.
pub(crate) const DIFF_TIMEOUT: Duration = Duration::from_secs(20);

/// Largest untracked file a capture carries. Larger ones are omitted by name.
pub(crate) const MAX_UNTRACKED_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The hidden ref namespace checkpoints are pinned under, so `gc` keeps them.
pub(crate) const CHECKPOINT_REF_ROOT: &str = "refs/beekeeper/checkpoints";

/// Longest failure sentence, in bytes (the wire's `unavailable.sentence`).
pub(crate) const MAX_SENTENCE_BYTES: usize = 512;

/// Longest branch name a checkpoint carries, in bytes (the wire's
/// `MAX_CHECKPOINT_BRANCH_BYTES`).
pub(crate) const MAX_BRANCH_BYTES: usize = 512;

/// Longest session id accepted as a ref path segment.
const MAX_SESSION_ID_LEN: usize = 128;

/// Prefix of the scratch index file inside the git directory.
const SCRATCH_INDEX_PREFIX: &str = "beekeeper-checkpoint-index-";

/// Who a checkpoint commit says made it. Explicit, so the capture never
/// depends on — or borrows — the host's or the seat's git identity.
const CHECKPOINT_AUTHOR_NAME: &str = "Beekeeper checkpoint";
const CHECKPOINT_AUTHOR_EMAIL: &str = "checkpoint@beekeeper.invalid";

/// Configuration every invocation carries: no hook, no fsmonitor, no split
/// index (which would write a `sharedindex.*` beside the scratch index), no
/// automatic maintenance.
const BASE_CONFIG: [&str; 10] = [
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.splitIndex=false",
    "-c",
    "gc.auto=0",
    "-c",
    "maintenance.auto=false",
];

/// Flush objects and refs before they are relied on. T3 found that git's
/// default rename-without-fsync leaves 0-byte refs after an unclean restart,
/// and a 0-byte ref under `refs/` breaks every later fetch and push in the
/// person's repository — far worse than a slower checkpoint.
const DURABLE_CONFIG: [&str; 4] = [
    "-c",
    "core.fsync=objects,reference",
    "-c",
    "core.fsyncMethod=fsync",
];

/// Which capture this is, which picks its ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapturePhase {
    /// Before the turn's prompt is sent ([`BASELINE_CAPTURE_TIMEOUT`]).
    Baseline,
    /// After the turn's terminal result ([`END_CAPTURE_TIMEOUT`]).
    TurnEnd,
    /// Before a rewind touches anything ([`PRE_REWIND_CAPTURE_TIMEOUT`]).
    PreRewind,
}

impl CapturePhase {
    /// The ceiling for this phase.
    #[must_use]
    pub(crate) fn timeout(self) -> Duration {
        match self {
            Self::Baseline => BASELINE_CAPTURE_TIMEOUT,
            Self::TurnEnd => END_CAPTURE_TIMEOUT,
            Self::PreRewind => PRE_REWIND_CAPTURE_TIMEOUT,
        }
    }
}

/// The last segment of a checkpoint ref.
///
/// Each variant has its own spelling, so no capture can overwrite another's
/// ref: a `pre_rewind` capture's coverage normally ends at the same seq as the
/// last turn's (that turn's terminal result), and pinning it at
/// `<throughSeq>` would silently replace the turn's own checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefLeaf<'a> {
    /// A turn's end, named by the terminal result's `eventSeq`: `<throughSeq>`.
    Through(u64),
    /// A turn's baseline, named by its prompt's `eventSeq`: `base-<firstSeq>`.
    Base(u64),
    /// The capture a rewind takes before it touches any file:
    /// `pre-rewind-<throughSeq>-<command>`, where `command` is the
    /// `session.rewind` lifecycle command's event id (64 hex). Keyed by the
    /// command, so a second rewind attempt of the same generation — after the
    /// first restored some files and failed — cannot overwrite the capture
    /// that holds the state before either touched anything.
    PreRewind {
        /// The last transcript seq the rewound generation wrote.
        through_seq: u64,
        /// The rewind command's event id.
        command: &'a str,
    },
}

impl RefLeaf<'_> {
    /// The segment, or `None` when a part of it could not be one safely.
    fn segment(self) -> Option<String> {
        match self {
            Self::Through(seq) => Some(seq.to_string()),
            Self::Base(seq) => Some(format!("base-{seq}")),
            Self::PreRewind {
                through_seq,
                command,
            } => (command.len() == 64 && command.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| format!("pre-rewind-{through_seq}-{}", command.to_ascii_lowercase())),
        }
    }
}

/// `refs/beekeeper/checkpoints/<sessionId>/<generation>/<leaf>`, or `None`
/// when the session id could not be a single, safe ref path segment (or a
/// [`RefLeaf::PreRewind`] command is not an event id).
///
/// The id is checked here rather than trusted: the ref is written by the
/// host, outside the tree's boundary, so a `..` or a `/` in it must not be
/// able to name any other ref.
#[must_use]
pub(crate) fn checkpoint_ref(
    session_id: &str,
    generation: u64,
    leaf: RefLeaf<'_>,
) -> Option<String> {
    let prefix = checkpoint_session_prefix(session_id)?;
    let segment = leaf.segment()?;
    Some(format!("{prefix}{generation}/{segment}"))
}

/// `refs/beekeeper/checkpoints/<sessionId>/` — everything one session's
/// checkpoints are pinned under, ending in `/` so `sess-1` never matches
/// `sess-10` — or `None` when the id could not be a single, safe ref path
/// segment (so no checkpoint could ever have been written for it).
#[must_use]
pub(crate) fn checkpoint_session_prefix(session_id: &str) -> Option<String> {
    let safe = !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_LEN
        && !session_id.starts_with('.')
        && !session_id.ends_with('.')
        && !session_id.ends_with(".lock")
        && !session_id.contains("..")
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    safe.then(|| format!("{CHECKPOINT_REF_ROOT}/{session_id}/"))
}

/// Why there is no capture, in the wire's vocabulary (`unavailable.code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnavailableCode {
    /// The working directory is not inside a git working tree (or is gone).
    NotARepository,
    /// No host-Git boundary could be prepared for the tree.
    BoundaryUnprepared,
    /// The phase's ceiling elapsed.
    TimedOut,
    /// Git ran and refused, or its answer could not be used.
    GitFailed,
}

impl UnavailableCode {
    /// The wire spelling.
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NotARepository => "NOT_A_REPOSITORY",
            Self::BoundaryUnprepared => "BOUNDARY_UNPREPARED",
            Self::TimedOut => "TIMED_OUT",
            Self::GitFailed => "GIT_FAILED",
        }
    }
}

/// A capture or diff that did not happen, with a sentence a person can read.
///
/// The sentence is composed here, never copied from git, so it carries no
/// host path, and it is at most [`MAX_SENTENCE_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureFailure {
    /// The classification.
    pub(crate) code: UnavailableCode,
    /// One sentence, ≤ [`MAX_SENTENCE_BYTES`].
    pub(crate) sentence: String,
    /// Set only when the leaf's ref already names a checkpoint, which was
    /// kept rather than replaced (a provider restart or a retried turn end
    /// re-running a capture). The caller can stand on that checkpoint — it
    /// may already be published — instead of reporting the turn as having
    /// none. `code` is [`UnavailableCode::GitFailed`] for a caller that does
    /// not look.
    pub(crate) already_pinned: Option<PinnedCheckpoint>,
}

impl CaptureFailure {
    fn new(code: UnavailableCode, sentence: impl Into<String>) -> Self {
        Self {
            code,
            sentence: bounded_sentence(sentence.into()),
            already_pinned: None,
        }
    }
}

/// Why a path is missing from a capture (`omitted[].reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OmitReason {
    /// An untracked file over [`MAX_UNTRACKED_FILE_BYTES`].
    TooLarge,
    /// A file the host could not read, or a nested repository git could not
    /// record (one with no commit checked out).
    Unreadable,
}

impl OmitReason {
    /// The wire spelling.
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::Unreadable => "unreadable",
        }
    }
}

/// One path a capture left out, by name (a UTF-8 name; one that is not is
/// counted in [`CapturedTree::omitted_not_listed`] instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OmittedPath {
    /// Repository-relative path.
    pub(crate) path: String,
    /// Why.
    pub(crate) reason: OmitReason,
}

/// A working tree written as a tree and pinned by a checkpoint commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapturedTree {
    /// The tree object id.
    pub(crate) tree: String,
    /// The checkpoint commit (the tree, parented on `head` when there is one).
    pub(crate) commit: String,
    /// The `HEAD` commit the checkpoint is parented on; `None` on an unborn
    /// branch.
    pub(crate) head: Option<String>,
    /// The branch shortname; `None` only when git said `HEAD` is detached
    /// (a failed read fails the capture rather than claiming detached).
    pub(crate) branch: Option<String>,
    /// Whether every non-ignored path is in `tree` (nothing omitted, named
    /// or not).
    pub(crate) complete: bool,
    /// Every path left out whose name is UTF-8, sorted by path, each named
    /// once. Uncapped: the wire names at most 32, so the mapper must carry
    /// this list's full length, plus [`Self::omitted_not_listed`], as the
    /// count of files not captured rather than the truncated list's.
    pub(crate) omitted: Vec<OmittedPath>,
    /// Paths left out whose names are not UTF-8, so cannot be named on the
    /// wire (its `omittedNotListed`). Each was still excluded by its exact
    /// bytes, so none of them is in `tree`.
    pub(crate) omitted_not_listed: u64,
    /// Whether the steps that can run project code ran inside a prepared
    /// boundary. `false` only on a platform with no boundary backend
    /// ([`HostLaunchPlan::Unenforced`]), where the session itself is
    /// unbounded too; the caller discloses it rather than this module hiding it.
    pub(crate) boundary_enforced: bool,
}

/// Capture `cwd`'s working tree and pin it at
/// `refs/beekeeper/checkpoints/<session_id>/<generation>/<leaf>`.
///
/// `scope` is the tree's prepared host-Git boundary
/// ([`crate::execution_scope_host::HostGitRequest::prepare`]); `None` means it
/// could not be prepared, and the capture refuses rather than run the
/// project's filters unbounded. `parent_head` is the `HEAD` commit the caller
/// already observed, or `None` to resolve it here.
///
/// As T3 does, the capture covers `cwd` and everything under it; a path of the
/// repository outside `cwd` keeps its `HEAD` content.
///
/// # Errors
/// A [`CaptureFailure`] classifying why nothing was pinned. The scratch index
/// is gone on every path.
pub(crate) async fn capture_tree(
    cwd: &Path,
    scope: Option<&HostLaunchPlan>,
    session_id: &str,
    generation: u64,
    leaf: RefLeaf<'_>,
    parent_head: Option<&str>,
    phase: CapturePhase,
) -> Result<CapturedTree, CaptureFailure> {
    capture_tree_within(
        cwd,
        scope,
        session_id,
        generation,
        leaf,
        parent_head,
        phase.timeout(),
    )
    .await
}

/// [`capture_tree`] with an explicit ceiling, so a test can drive it to zero.
pub(crate) async fn capture_tree_within(
    cwd: &Path,
    scope: Option<&HostLaunchPlan>,
    session_id: &str,
    generation: u64,
    leaf: RefLeaf<'_>,
    parent_head: Option<&str>,
    ceiling: Duration,
) -> Result<CapturedTree, CaptureFailure> {
    let ref_name = checkpoint_ref(session_id, generation, leaf).ok_or_else(|| {
        CaptureFailure::new(
            UnavailableCode::GitFailed,
            "The session id (or the rewind command id) cannot name a checkpoint ref, so nothing \
             was captured.",
        )
    })?;
    if !cwd.is_dir() {
        return Err(missing_cwd());
    }
    let Some(plan) = scope else {
        return Err(CaptureFailure::new(
            UnavailableCode::BoundaryUnprepared,
            "The working tree's Git boundary could not be prepared, so the project's Git was \
             not run to capture it.",
        ));
    };
    let deadline = Deadline::after(ceiling);
    match tokio::time::timeout(
        ceiling,
        capture(cwd, plan, &ref_name, parent_head, deadline),
    )
    .await
    {
        Ok(result) => result,
        Err(_elapsed) => Err(deadline.timed_out()),
    }
}

fn missing_cwd() -> CaptureFailure {
    CaptureFailure::new(
        UnavailableCode::NotARepository,
        "The session's working directory is not there, so there is no repository to capture.",
    )
}

/// The capture itself, unbounded in time (the caller bounds it).
async fn capture(
    cwd: &Path,
    plan: &HostLaunchPlan,
    ref_name: &str,
    parent_head: Option<&str>,
    deadline: Deadline,
) -> Result<CapturedTree, CaptureFailure> {
    let probe = Git::new(cwd, plan, None);
    let layout = read_layout(&probe).await?;
    // A pinned leaf is never re-captured: its commit may already be
    // published (see `turn_checkpoint_git_pin.rs`). `pin` guards the write
    // too, so a capture racing this check cannot replace it either.
    if let Some(existing) = pin::read_pinned(cwd, ref_name).await? {
        return Err(pin::already_pinned(existing));
    }
    let head = match parent_head {
        Some(head) if is_oid(head) => Some(head.to_ascii_lowercase()),
        Some(_) => {
            return Err(CaptureFailure::new(
                UnavailableCode::GitFailed,
                "The parent commit given for the checkpoint is not an object id.",
            ))
        }
        None => resolve_head(&probe).await?,
    };
    let branch = read_branch(&probe).await?;

    let scratch = ScratchIndex::new(&layout.git_dir);
    let git = Git::new(cwd, plan, Some(scratch.path()));
    seed_index(&git, &scratch, &layout, head.as_deref(), deadline).await?;

    let mut omitted = scan_untracked(&git, cwd, deadline).await?;
    if let Err(first) = stage(&git, &omitted).await {
        // Recovery, once: a nested repository with no commit, or a tracked
        // file the host cannot read, fails `add -A` outright. Name them and
        // try again without them; anything else is git's refusal.
        let extra = recovery_exclusions(&git, cwd, deadline).await?;
        if extra.is_empty() {
            return Err(first.into_failure());
        }
        omitted.extend(extra);
        stage(&git, &omitted)
            .await
            .map_err(StepError::into_failure)?;
    }

    let tree = oid_output(
        git.run(
            "write-tree",
            &[&DURABLE_CONFIG[..], &["write-tree"]].concat(),
            None,
        )
        .await,
    )?;
    let message = format!("Beekeeper checkpoint {ref_name}");
    let mut commit_args: Vec<&str> = DURABLE_CONFIG.to_vec();
    commit_args.extend(["commit-tree", "--no-gpg-sign", &tree, "-m", &message]);
    if let Some(head) = head.as_deref() {
        commit_args.extend(["-p", head]);
    }
    let commit = oid_output(git.run("commit-tree", &commit_args, None).await)?;
    // The scratch index is done with before the ref is written.
    drop(scratch);

    pin::pin(cwd, ref_name, &commit).await?;

    let ReportedOmissions { named, not_listed } = omit::normalize_omitted(omitted, &layout.prefix);
    Ok(CapturedTree {
        tree,
        commit,
        head,
        branch,
        complete: named.is_empty() && not_listed == 0,
        omitted: named,
        omitted_not_listed: not_listed,
        boundary_enforced: matches!(plan, HostLaunchPlan::Bounded(_)),
    })
}

/// Where the repository keeps its administration, as git reports it from
/// `cwd`. Host paths, used to place the scratch index and never returned.
struct Layout {
    git_dir: PathBuf,
    index: PathBuf,
    /// `cwd` relative to the top of the working tree, `""` or ending in `/`.
    prefix: String,
}

async fn read_layout(git: &Git<'_>) -> Result<Layout, CaptureFailure> {
    let out = git
        .run(
            "rev-parse",
            &[
                "rev-parse",
                "--is-inside-work-tree",
                "--show-prefix",
                "--path-format=absolute",
                "--git-dir",
                "--git-path",
                "index",
            ],
            None,
        )
        .await
        .map_err(StepError::into_failure)?;
    let text = String::from_utf8(out).map_err(|_| {
        CaptureFailure::new(
            UnavailableCode::GitFailed,
            "The repository's location is not valid UTF-8, so it was not captured.",
        )
    })?;
    let mut lines = text.split('\n');
    let inside = lines.next().unwrap_or_default();
    let prefix = lines.next().unwrap_or_default().to_owned();
    let git_dir = lines.next().unwrap_or_default();
    let index = lines.next().unwrap_or_default();
    if inside != "true" {
        return Err(CaptureFailure::new(
            UnavailableCode::NotARepository,
            "The working directory is inside a repository's administration, not a working tree.",
        ));
    }
    if git_dir.is_empty() || index.is_empty() {
        return Err(CaptureFailure::new(
            UnavailableCode::GitFailed,
            "git did not say where the repository keeps its index.",
        ));
    }
    Ok(Layout {
        git_dir: PathBuf::from(git_dir),
        index: PathBuf::from(index),
        prefix,
    })
}

async fn resolve_head(git: &Git<'_>) -> Result<Option<String>, CaptureFailure> {
    match git
        .run(
            "rev-parse HEAD",
            &["rev-parse", "-q", "--verify", "HEAD^{commit}"],
            None,
        )
        .await
    {
        Ok(out) => {
            let oid = String::from_utf8_lossy(&out).trim().to_ascii_lowercase();
            Ok(is_oid(&oid).then_some(oid))
        }
        // Exit 1 with nothing printed: an unborn branch, no commit yet.
        Err(StepError { exit: Some(1), .. }) => Ok(None),
        Err(error) => Err(error.into_failure()),
    }
}

/// The branch `HEAD` names, `None` only when git says `HEAD` is detached.
///
/// `None` is a claim on the wire ("detached"), so it is never a fallback: a
/// failed read, a name that is not UTF-8, a `HEAD` naming something other than
/// a branch, or a name longer than the wire carries
/// ([`MAX_BRANCH_BYTES`]) fails the capture instead of becoming one.
async fn read_branch(git: &Git<'_>) -> Result<Option<String>, CaptureFailure> {
    let out = match git
        .run("symbolic-ref HEAD", &["symbolic-ref", "-q", "HEAD"], None)
        .await
    {
        Ok(out) => out,
        // `-q`: exit 1 with nothing printed means `HEAD` is not symbolic —
        // detached, the one case `None` stands for.
        Err(StepError {
            exit: Some(1),
            kind: run::StepFailure::Exit,
            ..
        }) => return Ok(None),
        Err(error) => return Err(error.into_failure()),
    };
    let full = String::from_utf8(out).map_err(|_| {
        CaptureFailure::new(
            UnavailableCode::GitFailed,
            "The checked-out branch's name is not valid UTF-8, so the checkpoint could not name \
             it and none was made.",
        )
    })?;
    let full = full.trim_end_matches(['\n', '\r']);
    let Some(name) = full
        .strip_prefix("refs/heads/")
        .filter(|name| !name.is_empty())
    else {
        return Err(CaptureFailure::new(
            UnavailableCode::GitFailed,
            "HEAD names something other than a branch, so the checkpoint could not name one and \
             none was made.",
        ));
    };
    if name.len() > MAX_BRANCH_BYTES {
        return Err(CaptureFailure::new(
            UnavailableCode::GitFailed,
            format!(
                "The checked-out branch's name is longer than the {MAX_BRANCH_BYTES} bytes a \
                 checkpoint carries, so none was made."
            ),
        ));
    }
    Ok(Some(name.to_owned()))
}

/// Fill the scratch index: a copy of the real one reset to `HEAD` when its
/// flags allow, otherwise a fresh `HEAD`, otherwise nothing (an unborn branch).
///
/// The copy is only a stat cache. As T3 does (`GitVcsDriver.ts`
/// `captureCheckpoint`, "Retain stat data only where the copied index already
/// matches HEAD"), `read-tree --reset HEAD` runs on it before anything is
/// staged, so nothing the person staged and no unmerged entry reaches the
/// tree: a path outside `cwd` holds its `HEAD` content whatever the person's
/// index says. `read-tree` rewrites the file, so the racy-check stamp is put
/// back afterwards.
///
/// A fresh `read-tree HEAD` carries no skip-worktree marks, so in a sparse
/// checkout it would record every file outside the cone as deleted; a sparse
/// checkout whose index cannot be copied is refused instead.
async fn seed_index(
    git: &Git<'_>,
    scratch: &ScratchIndex,
    layout: &Layout,
    head: Option<&str>,
    deadline: Deadline,
) -> Result<(), CaptureFailure> {
    // With no `HEAD` there is nothing to reset the copy to, so the person's
    // staged entries would survive into the tree; start empty instead.
    let Some(head) = head else {
        return Ok(());
    };
    if reset_copied_index(git, scratch, layout, head, deadline).await? {
        let flags = git
            .run("ls-files -v", &["ls-files", "-v", "-z"], None)
            .await
            .map_err(StepError::into_failure)?;
        let (hidden, skip_worktree) = index_flags(&flags);
        if !hidden && !skip_worktree {
            return Ok(());
        }
        let sparse = sparse_checkout(git).await;
        if !hidden && sparse {
            // A sparse checkout's skip-worktree entries are its exclusions;
            // `add -A` leaves them alone, which is what a capture should do.
            return Ok(());
        }
        if sparse {
            return Err(CaptureFailure::new(
                UnavailableCode::GitFailed,
                "This sparse checkout marks files assume-unchanged, so a capture could not be \
                 complete and none was made.",
            ));
        }
        scratch.remove();
    } else if sparse_checkout(git).await {
        return Err(CaptureFailure::new(
            UnavailableCode::GitFailed,
            "This sparse checkout's index could not be copied, and without it every file \
             outside the checkout would read as deleted, so no capture was made.",
        ));
    }
    git.run("read-tree", &["read-tree", head], None)
        .await
        .map_err(StepError::into_failure)?;
    Ok(())
}

/// Whether the repository is a sparse checkout (`core.sparseCheckout`); a
/// failed read counts as not sparse, as git itself treats an unset key.
async fn sparse_checkout(git: &Git<'_>) -> bool {
    git.run(
        "config core.sparseCheckout",
        &["config", "--bool", "core.sparseCheckout"],
        None,
    )
    .await
    .map(|out| String::from_utf8_lossy(&out).trim() == "true")
    .unwrap_or(false)
}

/// Copy the real index to the scratch path, reset it to `head` and stamp it
/// one second below the real index's mtime; `Ok(false)` (with the scratch
/// file gone) when any step fails, so the caller seeds from `HEAD` alone.
///
/// # Errors
/// [`Deadline::timed_out`] when the ceiling passed before the copy began.
async fn reset_copied_index(
    git: &Git<'_>,
    scratch: &ScratchIndex,
    layout: &Layout,
    head: &str,
    deadline: Deadline,
) -> Result<bool, CaptureFailure> {
    let real = layout.index.clone();
    let target = scratch.path().to_path_buf();
    let abandoned = scratch.abandoned();
    let copied = off_runtime(move || {
        if deadline.passed() {
            return Err(deadline.timed_out());
        }
        Ok(copy_index(&real, &target, &abandoned))
    })
    .await?;
    let Some(earlier) = copied else {
        return Ok(false);
    };
    let reset = git
        .run("read-tree --reset", &["read-tree", "--reset", head], None)
        .await;
    if reset.is_err() || !stamp(scratch.path(), earlier) {
        scratch.remove();
        return Ok(false);
    }
    Ok(true)
}

/// Copy the real index to `scratch` and return the mtime one second below
/// the original's, or `None` when there is no real index to copy (or it
/// cannot be copied or stamped), or when the capture was `abandoned` while it
/// copied — the copy is then removed, since the scratch guard that would have
/// removed it is already gone.
fn copy_index(real: &Path, scratch: &Path, abandoned: &AtomicBool) -> Option<SystemTime> {
    let modified = std::fs::metadata(real)
        .and_then(|meta| meta.modified())
        .ok()?;
    let earlier = modified.checked_sub(Duration::from_secs(1))?;
    std::fs::copy(real, scratch).ok()?;
    if abandoned.load(Ordering::SeqCst) || !stamp(scratch, earlier) {
        let _ = std::fs::remove_file(scratch);
        return None;
    }
    Some(earlier)
}

/// Set `path`'s mtime to `earlier`. Without the earlier stamp git's racy
/// check could trust a stale entry; a fresh `read-tree` is slower and cannot.
fn stamp(path: &Path, earlier: SystemTime) -> bool {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(earlier))
        .is_ok()
}

/// `(assume-unchanged present, skip-worktree present)` from `ls-files -v -z`.
fn index_flags(listing: &[u8]) -> (bool, bool) {
    let mut hidden = false;
    let mut skip = false;
    for record in listing.split(|byte| *byte == 0) {
        match record.first() {
            Some(tag) if tag.is_ascii_lowercase() => hidden = true,
            Some(b'S') => skip = true,
            _ => {}
        }
    }
    (hidden, skip)
}

fn oid_output(result: Result<Vec<u8>, StepError>) -> Result<String, CaptureFailure> {
    let out = result.map_err(StepError::into_failure)?;
    let oid = String::from_utf8_lossy(&out).trim().to_ascii_lowercase();
    if is_oid(&oid) {
        Ok(oid)
    } else {
        Err(CaptureFailure::new(
            UnavailableCode::GitFailed,
            "git returned something other than an object id while writing the checkpoint.",
        ))
    }
}

/// A lowercase or uppercase hex object id of either object format.
fn is_oid(text: &str) -> bool {
    (text.len() == 40 || text.len() == 64) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Trim `text` to [`MAX_SENTENCE_BYTES`] on a character boundary.
fn bounded_sentence(text: String) -> String {
    if text.len() <= MAX_SENTENCE_BYTES {
        return text;
    }
    let mut end = MAX_SENTENCE_BYTES - '…'.len_utf8();
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The scratch index, removed (with any `.lock` git left) when dropped — on
/// success, on error, and when a timeout drops the capture mid-step.
struct ScratchIndex {
    path: PathBuf,
    /// Set (before the file is removed) when the guard is dropped, so a copy
    /// still running on the blocking pool removes what it writes afterwards.
    abandoned: Arc<AtomicBool>,
}

impl ScratchIndex {
    fn new(git_dir: &Path) -> Self {
        Self {
            path: git_dir.join(format!("{SCRATCH_INDEX_PREFIX}{}", uuid::Uuid::new_v4())),
            abandoned: Arc::new(AtomicBool::new(false)),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn abandoned(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.abandoned)
    }

    fn remove(&self) {
        let _ = std::fs::remove_file(&self.path);
        let mut lock = self.path.clone().into_os_string();
        lock.push(".lock");
        let _ = std::fs::remove_file(PathBuf::from(lock));
    }
}

impl Drop for ScratchIndex {
    fn drop(&mut self) {
        self.abandoned.store(true, Ordering::SeqCst);
        self.remove();
    }
}

#[cfg(test)]
#[path = "turn_checkpoint_git_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "turn_checkpoint_git_tests_refs.rs"]
mod tests_refs;

#[cfg(test)]
#[path = "turn_checkpoint_git_tests_diff.rs"]
mod tests_diff;

#[cfg(test)]
#[path = "turn_checkpoint_git_tests_index.rs"]
mod tests_index;

#[cfg(test)]
#[path = "turn_checkpoint_git_tests_blocking.rs"]
mod tests_blocking;
