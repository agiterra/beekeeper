//! Put the revision a seat was hired to work on into the seat's own tree.
//!
//! # The hole this fills
//!
//! A hired seat's worktree is cut from the trunk: the hire path passes no
//! source revision (`useCodingSessionHire.ts` sends `source: null`, resolved in
//! [`crate::coding_sessions::worktree`]), so a verifier or a runner opens its
//! editor on `main` and not on the commit it was hired to verify. Until now the
//! only way to close that gap was to ask the seat to do it, and an agent asked
//! to check out a commit reports success whether or not the tree moved.
//!
//! This command moves the tree from the host side and then *measures* it: the
//! success value names the path, the branch, the commit `HEAD` actually
//! resolves to and the remote the objects came from. Everything it cannot do,
//! it refuses by name. There is no branch here that guesses.
//!
//! # What it will not do
//!
//! * **It never discards work.** A tree with anything in `git status
//!   --porcelain` is refused as `dirty_tree`, with the count and the first
//!   paths, and nothing is written. No `reset --hard`, no `clean`, no `stash` —
//!   an uncommitted change in a seat's tree is the one thing in this whole
//!   system nobody else has a copy of.
//! * **It never adopts a tree this host did not cut.** The worktree is resolved
//!   from [`CodingSessionWorkdirStore::worktrees`], the record written when the
//!   host created it. No record is `unrecorded_tree`, not a search of the disk.
//! * **It never names a remote.** When one is needed, it is resolved at run
//!   time from git config by the same ladder `scripts/wip-post-commit.sh` uses.
//!   Two pre-push guards in this repo hard-coded `origin` and both broke
//!   silently the day the remote names moved.
//! * **A remote is a way of getting a missing object, not a precondition for
//!   trusting one.** A commit already in the seat's repository is established
//!   without resolving a remote at all, because a sha is content-addressed and
//!   a fetch could prove nothing further about its identity. `no_remote`,
//!   `ambiguous_remote`, `fetch_failed` and `unknown_commit` are therefore
//!   only ever reached when an object really was missing.
//!
//! # The durable record
//!
//! Every attempt — established, already current, or refused — is filed in the
//! workdir store under its assignment id (see
//! [`CodingSessionAssignmentInputRecord`]). A later provider check, and the
//! mission inspector, read that record instead of re-running git, and a retry
//! that carries nothing but the assignment id replays the inputs from it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::{
    load_workdir_store, load_workdir_store_readonly, lock_workdir_store, save_workdir_store,
    seat_worktree_key, CodingSessionWorkdirStore, WORKDIR_STORE_VERSION,
};
use crate::commands::project_git_exec::GIT_REPO_SELECTION_VARS;
use crate::util::now_iso;

/// Upper bound on remembered assignment-input attempts.
///
/// One per assignment, replaced in place on a retry, so the steady state is
/// the number of assignments this host has ever established inputs for. The
/// cap is what stops a long-lived install, or a hostile file, growing the
/// record without limit; over it, the oldest attempt is evicted first.
pub(crate) const MAX_ASSIGNMENT_INPUT_RECORDS: usize = 256;

/// How many `git status --porcelain` lines are quoted back in `detail`.
const REFUSAL_DETAIL_LINES: usize = 5;

/// What the host established, measured after the fact rather than intended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstablishedAssignmentInput {
    /// The seat worktree, as this host recorded cutting it.
    pub path: String,
    /// The branch the commit is checked out on.
    pub branch: String,
    /// The commit `HEAD` resolved to *after* the checkout, re-read from git.
    pub commit: String,
    /// The remote the objects were fetched from — never a constant, and
    /// `null` when nothing had to be fetched.
    ///
    /// A sha is content-addressed: an object already in the seat's repository
    /// *is* that commit, and no remote was needed to establish it. Reporting a
    /// remote there would name a means that was never used.
    pub remote: Option<String>,
    /// True when the tree already had that commit on that branch and nothing
    /// was written.
    pub already_current: bool,
}

/// Why the host refused, by name.
///
/// Every variant is a refusal the caller can act on. None of them is a
/// fallback for "something went wrong": a state this module cannot classify
/// does not exist, because each step maps its own failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstablishAssignmentInputCode {
    /// This host has no record of cutting a worktree for that seat.
    UnrecordedTree,
    /// The recorded path is gone, or is no longer a git worktree.
    MissingTree,
    /// The tree has uncommitted work. Nothing was written.
    DirtyTree,
    /// No remote could be resolved from config or from a sole configured one.
    NoRemote,
    /// Several remotes exist and no config key chooses between them.
    AmbiguousRemote,
    /// The fetch itself failed; `detail` carries git's stderr.
    FetchFailed,
    /// The fetch succeeded and the commit is still not in this repository.
    UnknownCommit,
    /// Git refused the checkout, or `HEAD` did not land on the commit.
    CheckoutFailed,
    /// The request could not be read as a request.
    InvalidInput,
}

impl EstablishAssignmentInputCode {
    /// The exact word this code is published as, on the wire and in the
    /// durable record. Pinned against serde by a test, so the record and the
    /// command response can never drift apart.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnrecordedTree => "unrecorded_tree",
            Self::MissingTree => "missing_tree",
            Self::DirtyTree => "dirty_tree",
            Self::NoRemote => "no_remote",
            Self::AmbiguousRemote => "ambiguous_remote",
            Self::FetchFailed => "fetch_failed",
            Self::UnknownCommit => "unknown_commit",
            Self::CheckoutFailed => "checkout_failed",
            Self::InvalidInput => "invalid_input",
        }
    }
}

/// A refusal: the code to branch on, a sentence for a person, and the raw
/// evidence when there is any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstablishAssignmentInputError {
    /// One of the nine documented codes.
    pub code: EstablishAssignmentInputCode,
    /// What happened, in words, without inventing a cause.
    pub message: String,
    /// Git's own output, or the paths involved, when the refusal has any.
    pub detail: Option<String>,
    /// How many `git status --porcelain` lines the tree had, on `dirty_tree`
    /// and on nothing else.
    ///
    /// The fact, beside the sentence that renders it. A caller that needs the
    /// number reads this field; parsing it back out of `message` makes the
    /// wording load-bearing, and the wording is not a contract.
    pub changes: Option<u32>,
}

/// One attempt to establish an assignment's input, kept after the fact.
///
/// Written on every attempt, successful or refused, so a later check can say
/// what this host did without re-running git — and so a retry can replay the
/// inputs from the assignment id alone. Paths here name one person's disk and
/// are never published; this record lives in the host-local store for exactly
/// that reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionAssignmentInputRecord {
    /// The hire event this attempt answers, 64 lowercase hex.
    pub assignment_id: String,
    /// The session the seat belongs to, when the attempt got that far.
    pub session_ref: Option<String>,
    /// The seat label inside that session.
    pub seat_label: Option<String>,
    /// The commit that was asked for — not necessarily the one established.
    pub commit: Option<String>,
    /// The branch that was asked for, when the caller named one.
    pub branch: Option<String>,
    /// The worktree this host resolved from its own record.
    pub path: Option<PathBuf>,
    /// The remote the ladder resolved, when it got that far.
    pub remote: Option<String>,
    /// `established`, `already_current`, or the refusal code.
    pub outcome: String,
    /// The refusal sentence, when this attempt was refused.
    pub message: Option<String>,
    /// The uncommitted-change count a `dirty_tree` refusal measured, so a
    /// later reader has the number without re-running git.
    #[serde(default)]
    pub changes: Option<u32>,
    /// How many attempts have been *started* for this assignment on this host.
    ///
    /// Counted before the git work rather than after it, which is the only
    /// count that can bound a replay: a record left mid-attempt by a quit or a
    /// crash is indistinguishable from one never tried, and without this the
    /// durable queue would start the same attempt again at every launch. See
    /// [`crate::coding_sessions::assignment_establishment`].
    ///
    /// `#[serde(default)]`, so a record written before the queue existed reads
    /// as zero attempts — which is what it is: nobody was counting.
    #[serde(default)]
    pub attempts: u32,
    /// When the attempt finished, ISO-8601.
    pub recorded_at: String,
}

/// The inputs one attempt was made with, filled in as far as it got.
///
/// Carried alongside the result so a refusal records everything already
/// established — the path resolved before the dirty check, say — rather than
/// only what the caller passed in.
#[derive(Debug, Clone, Default)]
struct AttemptFacts {
    session_ref: Option<String>,
    seat_label: Option<String>,
    commit: Option<String>,
    branch: Option<String>,
    path: Option<PathBuf>,
    remote: Option<String>,
}

/// What a caller asked for. Everything but the assignment id may be absent,
/// in which case the last recorded attempt for that assignment supplies it.
#[derive(Debug, Clone, Default)]
pub(crate) struct EstablishAssignmentInputRequest {
    pub assignment_id: String,
    pub session_ref: Option<String>,
    pub seat_label: Option<String>,
    pub commit: Option<String>,
    pub branch: Option<String>,
}

impl CodingSessionWorkdirStore {
    /// File one attempt under its assignment id, replacing any earlier attempt
    /// for the same assignment.
    ///
    /// Replacement rather than append is the point: an assignment has one
    /// current input, and a list of attempts would leave every reader to work
    /// out which one is in force. Over [`MAX_ASSIGNMENT_INPUT_RECORDS`] the
    /// oldest `recordedAt` is evicted, because the newest attempt is the one
    /// somebody is asking about.
    pub(crate) fn record_assignment_input(&mut self, record: CodingSessionAssignmentInputRecord) {
        let key = record.assignment_id.clone();
        self.assignment_inputs.insert(key.clone(), record);
        while self.assignment_inputs.len() > MAX_ASSIGNMENT_INPUT_RECORDS {
            let Some(oldest) = self
                .assignment_inputs
                .iter()
                .filter(|(candidate, _)| *candidate != &key)
                .min_by(|left, right| {
                    left.1
                        .recorded_at
                        .cmp(&right.1.recorded_at)
                        .then_with(|| left.0.cmp(right.0))
                })
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.assignment_inputs.remove(&oldest);
        }
    }

    /// The last attempt recorded for one assignment, if any.
    pub(crate) fn assignment_input(
        &self,
        assignment_id: &str,
    ) -> Option<&CodingSessionAssignmentInputRecord> {
        self.assignment_inputs.get(assignment_id.trim())
    }
}

/// Run git in `tree` with the same hermetic environment the seat-hook
/// installer uses.
///
/// Copied — not shared — from `fn git` in
/// `desktop/src-tauri/src/commands/coding_session_seat_hooks.rs`, because that
/// module is private (`mod coding_session_seat_hooks` in `commands/mod.rs`)
/// and making it reachable would mean editing a file this lane does not own.
/// The environment itself is not copied twice over: the `GIT_DIR` family comes
/// from the single shared [`GIT_REPO_SELECTION_VARS`] list, which is what the
/// original had drifted on.
///
/// Global and system config are off, so nothing here can read or write a
/// person's `~/.gitconfig`, and `GIT_TERMINAL_PROMPT=0` means a fetch that
/// needs a credential fails and says so instead of waiting forever on a prompt
/// no one will answer.
fn git(tree: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new("git");
    command.args(args);
    command.current_dir(tree);
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    // Git for Windows maps `/dev/null` to `NUL`, so this disables the global
    // file on every platform git supports.
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    crate::util::configure_no_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn refuse(
    code: EstablishAssignmentInputCode,
    message: impl Into<String>,
    detail: Option<String>,
) -> EstablishAssignmentInputError {
    EstablishAssignmentInputError {
        code,
        message: message.into(),
        detail,
        changes: None,
    }
}

/// A lowercase hex object id of one of git's two widths.
pub(super) fn is_object_id(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
}

/// Whether a branch name can be handed to git without becoming an option, a
/// refspec, or a second argument.
///
/// Deliberately stricter than `git check-ref-format`: this name is written
/// into a command line and into a durable record, so anything that could be
/// read as something other than one branch is refused rather than escaped.
fn is_safe_branch_name(value: &str) -> bool {
    const FORBIDDEN: [char; 9] = ['~', '^', ':', '?', '*', '[', '\\', '"', '\''];
    !value.is_empty()
        && !value.starts_with('-')
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.ends_with(".lock")
        && !value.contains("..")
        && !value.contains("//")
        && !value.contains("@{")
        && value != "@"
        && !value.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || FORBIDDEN.contains(&character)
                || character == '\u{7f}'
        })
}

/// The inputs an attempt will run with, after the recorded attempt has filled
/// in whatever the caller left out.
struct ResolvedRequest {
    session_ref: String,
    seat_label: String,
    commit: String,
    branch: Option<String>,
}

fn blank_to_none(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Fill the request from the last recorded attempt, then validate it.
///
/// A supplied value always wins over a recorded one, and a *different* commit
/// is a new input for the same assignment rather than a second attempt beside
/// it — which is why the record is keyed by assignment id and replaced.
fn resolve_request(
    store: &CodingSessionWorkdirStore,
    request: &EstablishAssignmentInputRequest,
) -> Result<ResolvedRequest, EstablishAssignmentInputError> {
    let previous = store.assignment_input(&request.assignment_id);
    let session_ref = blank_to_none(request.session_ref.as_ref())
        .or_else(|| previous.and_then(|record| record.session_ref.clone()));
    let seat_label = blank_to_none(request.seat_label.as_ref())
        .or_else(|| previous.and_then(|record| record.seat_label.clone()));
    let commit = blank_to_none(request.commit.as_ref())
        .or_else(|| previous.and_then(|record| record.commit.clone()));
    // The recorded branch is replayed whenever the caller names none, even
    // when the caller *does* name a new commit: moving the same seat branch to
    // a new revision is the ordinary retry, and inventing a different branch
    // for it would leave the established input somewhere nobody is looking.
    let branch = blank_to_none(request.branch.as_ref())
        .or_else(|| previous.and_then(|record| record.branch.clone()));

    let mut missing = Vec::new();
    if session_ref.is_none() {
        missing.push("sessionRef");
    }
    if seat_label.is_none() {
        missing.push("seatLabel");
    }
    if commit.is_none() {
        missing.push("commit");
    }
    if !missing.is_empty() {
        return Err(refuse(
            EstablishAssignmentInputCode::InvalidInput,
            format!(
                "this host has no recorded attempt for that assignment, so the request must carry {}",
                missing.join(", ")
            ),
            None,
        ));
    }
    let (Some(session_ref), Some(seat_label), Some(commit)) = (session_ref, seat_label, commit)
    else {
        // Unreachable: `missing` is empty exactly when all three are `Some`.
        return Err(refuse(
            EstablishAssignmentInputCode::InvalidInput,
            "the request is incomplete",
            None,
        ));
    };
    if !is_object_id(&commit) {
        return Err(refuse(
            EstablishAssignmentInputCode::InvalidInput,
            "a commit must be 40 or 64 lowercase hex characters",
            Some(commit),
        ));
    }
    if let Some(branch) = branch.as_deref() {
        if !is_safe_branch_name(branch) {
            return Err(refuse(
                EstablishAssignmentInputCode::InvalidInput,
                "that branch name cannot be handed to git as a single branch",
                Some(branch.to_string()),
            ));
        }
    }
    Ok(ResolvedRequest {
        session_ref,
        seat_label,
        commit,
        branch,
    })
}

/// A `git config --get` that treats "unset" as `None`, since git exits 1 for
/// both an unset key and a failure.
fn config_get(tree: &Path, key: &str) -> Option<String> {
    git(tree, &["config", "--get", key])
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The remotes this repository actually has.
fn configured_remotes(tree: &Path) -> Vec<String> {
    git(tree, &["remote"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Resolve the remote the same way `scripts/wip-post-commit.sh` does.
///
/// `buzz.wipRemote`, then `branch.<current>.pushRemote`, then
/// `remote.pushDefault`, then `branch.<current>.remote`, then the sole
/// configured remote when there is exactly one. Never a literal name: the
/// remotes in this project were renamed once already, and both guards that
/// spelled one out broke silently that day.
fn resolve_remote(
    tree: &Path,
    current_branch: Option<&str>,
) -> Result<String, EstablishAssignmentInputError> {
    let mut candidate = config_get(tree, "buzz.wipRemote");
    if candidate.is_none() {
        if let Some(branch) = current_branch {
            candidate = config_get(tree, &format!("branch.{branch}.pushRemote"));
        }
    }
    if candidate.is_none() {
        candidate = config_get(tree, "remote.pushDefault");
    }
    if candidate.is_none() {
        if let Some(branch) = current_branch {
            candidate = config_get(tree, &format!("branch.{branch}.remote"));
        }
    }
    let remotes = configured_remotes(tree);
    if let Some(candidate) = candidate {
        if remotes.iter().any(|remote| remote == &candidate) {
            return Ok(candidate);
        }
        return Err(refuse(
            EstablishAssignmentInputCode::NoRemote,
            format!("git config names the remote '{candidate}', which this repository has no remote for"),
            Some(if remotes.is_empty() {
                "no remotes are configured".to_string()
            } else {
                remotes.join(", ")
            }),
        ));
    }
    match remotes.len() {
        0 => Err(refuse(
            EstablishAssignmentInputCode::NoRemote,
            "this repository has no remote configured, and no config key names one",
            None,
        )),
        1 => Ok(remotes[0].clone()),
        _ => Err(refuse(
            EstablishAssignmentInputCode::AmbiguousRemote,
            format!(
                "{} remotes are configured and no buzz.wipRemote, pushRemote, remote.pushDefault or branch remote chooses between them",
                remotes.len()
            ),
            Some(remotes.join(", ")),
        )),
    }
}

/// Whether this object is already in the seat's repository.
fn commit_is_present(tree: &Path, commit: &str) -> bool {
    git(tree, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_ok()
}

/// Bring the commit in from the resolved remote.
///
/// Asks for the object by name first, because a server that allows it sends
/// one commit instead of every ref; a server that does not allow it refuses
/// that request, so the second attempt is an ordinary fetch. Only when both
/// fail is this a `fetch_failed` — the object still being absent afterwards is
/// a different answer, and gets a different code.
fn fetch_commit(
    tree: &Path,
    remote: &str,
    commit: &str,
) -> Result<(), EstablishAssignmentInputError> {
    if git(tree, &["fetch", "--no-tags", "--quiet", remote, commit]).is_ok() {
        return Ok(());
    }
    match git(tree, &["fetch", "--no-tags", "--quiet", remote]) {
        Ok(_) => Ok(()),
        Err(stderr) => Err(refuse(
            EstablishAssignmentInputCode::FetchFailed,
            format!("fetching from '{remote}' failed"),
            Some(stderr),
        )),
    }
}

/// Does this repository already have a branch by that name?
fn branch_exists(tree: &Path, branch: &str) -> bool {
    git(
        tree,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

/// The single attempt, with every refusal named at the step that found it.
fn attempt(
    store: &CodingSessionWorkdirStore,
    request: &EstablishAssignmentInputRequest,
    facts: &mut AttemptFacts,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let resolved = resolve_request(store, request)?;
    facts.session_ref = Some(resolved.session_ref.clone());
    facts.seat_label = Some(resolved.seat_label.clone());
    facts.commit = Some(resolved.commit.clone());
    facts.branch.clone_from(&resolved.branch);

    let key = seat_worktree_key(&resolved.session_ref, &resolved.seat_label);
    let Some(recorded) = store.worktrees.get(&key) else {
        return Err(refuse(
            EstablishAssignmentInputCode::UnrecordedTree,
            "this host did not record cutting a worktree for that seat",
            Some(key),
        ));
    };
    let tree = recorded.path.clone();
    facts.path = Some(tree.clone());
    if !tree.is_dir() {
        return Err(refuse(
            EstablishAssignmentInputCode::MissingTree,
            "the recorded worktree directory is gone",
            Some(tree.to_string_lossy().to_string()),
        ));
    }
    if git(&tree, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Ok("true") {
        return Err(refuse(
            EstablishAssignmentInputCode::MissingTree,
            "the recorded path is no longer a git worktree",
            Some(tree.to_string_lossy().to_string()),
        ));
    }

    // Uncommitted work stops everything, before a single write.
    let status = git(&tree, &["status", "--porcelain"]).map_err(|stderr| {
        refuse(
            EstablishAssignmentInputCode::MissingTree,
            "git could not read the state of the recorded worktree",
            Some(stderr),
        )
    })?;
    let dirty: Vec<&str> = status.lines().filter(|line| !line.is_empty()).collect();
    if !dirty.is_empty() {
        let mut refusal = refuse(
            EstablishAssignmentInputCode::DirtyTree,
            format!(
                "the seat's worktree has {} uncommitted change(s); nothing was changed",
                dirty.len()
            ),
            Some(
                dirty
                    .iter()
                    .take(REFUSAL_DETAIL_LINES)
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        );
        // Saturating rather than lossy: a tree with more than `u32::MAX`
        // changes is not a number anybody acts on, and reporting a wrapped one
        // would be worse than reporting the ceiling.
        refusal.changes = Some(u32::try_from(dirty.len()).unwrap_or(u32::MAX));
        return Err(refusal);
    }

    let current_branch = git(&tree, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty());

    // The remote ladder runs only when the object is missing. Resolving it
    // first would refuse `no_remote` about a commit this host demonstrably
    // holds — a false refusal, and the kind that teaches a caller to distrust
    // the true ones.
    let mut remote = None;
    if !commit_is_present(&tree, &resolved.commit) {
        let resolved_remote = resolve_remote(&tree, current_branch.as_deref())?;
        facts.remote = Some(resolved_remote.clone());
        fetch_commit(&tree, &resolved_remote, &resolved.commit)?;
        if !commit_is_present(&tree, &resolved.commit) {
            return Err(refuse(
                EstablishAssignmentInputCode::UnknownCommit,
                format!(
                    "'{resolved_remote}' fetched, and the commit is still not in this repository"
                ),
                Some(resolved.commit.clone()),
            ));
        }
        remote = Some(resolved_remote);
    }

    let target = resolved
        .branch
        .clone()
        .unwrap_or_else(|| recorded.branch.clone());
    if !is_safe_branch_name(&target) {
        return Err(refuse(
            EstablishAssignmentInputCode::InvalidInput,
            "the recorded seat branch cannot be handed to git as a single branch",
            Some(target),
        ));
    }
    // Only this seat's own branch may be moved: its recorded branch, the one
    // already checked out here, or a name no branch holds yet. Anything else
    // belongs to somebody, and resetting it would rewrite their work.
    let is_seats_own = target == recorded.branch
        || current_branch.as_deref() == Some(target.as_str())
        || !branch_exists(&tree, &target);
    if !is_seats_own {
        return Err(refuse(
            EstablishAssignmentInputCode::CheckoutFailed,
            format!("'{target}' is not this seat's branch, so it was not moved"),
            Some(format!("the seat's branch is '{}'", recorded.branch)),
        ));
    }

    let head = git(&tree, &["rev-parse", "HEAD"]).unwrap_or_default();
    if head == resolved.commit && current_branch.as_deref() == Some(target.as_str()) {
        return Ok(EstablishedAssignmentInput {
            path: tree.to_string_lossy().to_string(),
            branch: target,
            commit: resolved.commit,
            remote,
            already_current: true,
        });
    }

    git(
        &tree,
        &["checkout", "--quiet", "-B", &target, &resolved.commit, "--"],
    )
    .map_err(|stderr| {
        refuse(
            EstablishAssignmentInputCode::CheckoutFailed,
            format!("git refused to put {} on '{target}'", resolved.commit),
            Some(stderr),
        )
    })?;

    // Measured, not assumed: the whole point of the command is that the caller
    // does not have to take anyone's word for where the tree ended up.
    let head = git(&tree, &["rev-parse", "HEAD"]).map_err(|stderr| {
        refuse(
            EstablishAssignmentInputCode::CheckoutFailed,
            "the checkout reported success and HEAD could not be read",
            Some(stderr),
        )
    })?;
    if head != resolved.commit {
        return Err(refuse(
            EstablishAssignmentInputCode::CheckoutFailed,
            format!("the checkout reported success and HEAD is {head}"),
            Some(resolved.commit),
        ));
    }
    let landed_branch = git(&tree, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .unwrap_or_default()
        .trim()
        .to_string();
    if landed_branch != target {
        return Err(refuse(
            EstablishAssignmentInputCode::CheckoutFailed,
            format!("the commit is checked out on '{landed_branch}', not '{target}'"),
            None,
        ));
    }
    Ok(EstablishedAssignmentInput {
        path: tree.to_string_lossy().to_string(),
        branch: target,
        commit: head,
        remote,
        already_current: false,
    })
}

/// Run one attempt against an in-memory store and file its record.
///
/// Split from the command so the behaviour is testable against real
/// repositories without a Tauri app handle; the command is this plus the
/// store's lock, load and save.
pub(crate) fn establish(
    store: &mut CodingSessionWorkdirStore,
    request: &EstablishAssignmentInputRequest,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let assignment_id = request.assignment_id.trim().to_string();
    if assignment_id.len() != 64 || !is_object_id(&assignment_id) {
        // Refused before any record is written, and deliberately: a record has
        // to be filed under an assignment id, and this request has none worth
        // the name.
        return Err(refuse(
            EstablishAssignmentInputCode::InvalidInput,
            "an assignment id must be 64 lowercase hex characters",
            None,
        ));
    }
    // Read before the attempt: the count belongs to the assignment, not to
    // this attempt's record, and `record_assignment_input` replaces the row
    // rather than merging it.
    let attempts = store
        .assignment_input(&assignment_id)
        .map_or(0, |record| record.attempts);
    let mut facts = AttemptFacts::default();
    let outcome = attempt(store, request, &mut facts);
    let (result_outcome, message) = match &outcome {
        Ok(established) if established.already_current => ("already_current".to_string(), None),
        Ok(_) => ("established".to_string(), None),
        Err(error) => (error.code.as_str().to_string(), Some(error.message.clone())),
    };
    let changes = outcome.as_ref().err().and_then(|error| error.changes);
    store.record_assignment_input(CodingSessionAssignmentInputRecord {
        assignment_id,
        session_ref: facts.session_ref,
        seat_label: facts.seat_label,
        commit: facts.commit,
        branch: facts.branch,
        path: facts.path,
        remote: facts.remote,
        outcome: result_outcome,
        message,
        changes,
        attempts,
        recorded_at: now_iso(),
    });
    outcome
}

/// Put the commit an assignment names into that seat's own worktree.
///
/// `commit`, `branch`, `sessionRef` and `seatLabel` may be omitted on a retry:
/// the last recorded attempt for `assignmentId` supplies them, and anything
/// supplied wins over what was recorded. Idempotent — a second call with the
/// same inputs answers `alreadyCurrent: true` and writes nothing to the tree.
#[tauri::command]
pub async fn coding_session_establish_assignment_input(
    app: AppHandle,
    state: State<'_, AppState>,
    assignment_id: String,
    session_ref: Option<String>,
    seat_label: Option<String>,
    commit: Option<String>,
    branch: Option<String>,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let request = EstablishAssignmentInputRequest {
        assignment_id,
        session_ref,
        seat_label,
        commit,
        branch,
    };
    // A store this host cannot read is reported as a missing record rather
    // than as some other failure: what the caller needs to know is that the
    // seat's tree could not be resolved from the record, which is true.
    let unreadable = |error: String| {
        refuse(
            EstablishAssignmentInputCode::UnrecordedTree,
            "this host could not read the record it writes when it cuts a worktree",
            Some(error),
        )
    };
    let _lock = lock_workdir_store(&app).map_err(unreadable)?;
    let mut store = load_workdir_store(&app).map_err(unreadable)?;
    let outcome = establish(&mut store, &request);
    store.version = WORKDIR_STORE_VERSION;
    if let Err(error) = save_workdir_store(&app, &state, &store) {
        // The disk is already in the state the outcome describes, so the
        // outcome stands; what is lost is the durable record of it, and that
        // is said out loud rather than folded into the result.
        eprintln!("coding session: assignment input record could not be saved: {error}");
    }
    outcome
}

/// What this host last established for one assignment, without touching git.
///
/// The read the mission inspector and a later provider check use: it answers
/// from the record alone, so it costs nothing and cannot move a tree.
#[tauri::command]
pub async fn coding_session_assignment_input_record(
    app: AppHandle,
    assignment_id: String,
) -> Result<Option<CodingSessionAssignmentInputRecord>, String> {
    let store = load_workdir_store_readonly(&app)?;
    Ok(store.assignment_input(&assignment_id).cloned())
}

#[cfg(test)]
#[path = "assignment_input_tests.rs"]
mod tests;
