//! Establishing a seat's exact input, owned by the party that opens its turn.
//!
//! # What moved here, and why
//!
//! Ledger 185 made "accept assignment → establish input → record result" a
//! durable queue instead of a React effect, but it built that queue in the
//! **desktop host**, and the host has no relay subscription of its own for
//! kind 44244: the first sighting of an assignment still arrived as an
//! argument from a mounted surface, and nothing ordered establishment against
//! the seat's wake. A wake that raced ahead was refused by the turn fence
//! ([`crate::verification_input`], ledger 133), which consumes the delivery
//! and costs a re-issued assignment and a model turn. On 2026-09-20 a
//! verifier's tree stayed on the wrong commit and the seat verified a scratch
//! archive by hand (ledger 178(d)).
//!
//! The provider holds both the governed subscription and the turn gate, so it
//! is the only party that can order `accept → establish → verify → start`.
//! This module is the Tauri-independent core of lane 185's queue and checkout,
//! moved whole: the intent record, the serial drain, the started-attempt count
//! persisted *before* the git work, terminal outcomes that are never retried,
//! one replay of an interrupted attempt and then
//! [`ASSIGNMENT_INPUT_ABANDONED`], every refusal named, and a dirty tree
//! preserved and refused.
//!
//! # One store, one lock, two processes
//!
//! There is deliberately **no second queue and no second file**. The records
//! are the same `assignmentInputs` rows in the same
//! `coding-session-workdirs.json` the desktop writes, taken under the same OS
//! lock — the sibling `coding-session-workdirs.lock`, opened `O_NOFOLLOW` at
//! mode `0600` and locked with [`std::fs::File::lock`]. The desktop app and
//! its sidecar provider are two processes over one file, which is exactly what
//! an advisory lock on one path is for; [`lock_store_file`] is now the single
//! implementation of that protocol, and the desktop's
//! `workdir_store_lock.rs` calls it.
//!
//! A writer here only ever *reads and replaces* the `assignmentInputs` key.
//! Every other key in the document — the desktop's preferences, MRU, create
//! hints, seat worktrees and prune records — is carried through verbatim in
//! [`AssignmentInputDocument::rest`], so the sidecar cannot clobber a key it
//! does not understand, and a desktop that gains a key tomorrow needs no
//! change here.
//!
//! The file is never *created* from nothing. A store this provider cannot
//! find is reported as [`EstablishAssignmentInputCode::UnrecordedTree`]:
//! inventing a document would mean inventing the desktop's own schema version
//! and defaults, and a host that has never cut a worktree has no tree to move
//! anyway.
//!
//! # What it will not do
//!
//! * **It never discards work.** A tree with anything in `git status
//!   --porcelain` is refused as `dirty_tree` with the count and the first
//!   paths, and nothing is written. No `reset --hard`, no `clean`, no `stash`.
//! * **It never adopts a tree the caller did not resolve.** The worktree
//!   arrives as a [`SeatCheckout`] the caller read from its own record — the
//!   desktop's `worktrees` map, or the provider's own session record — never
//!   from a search of the disk.
//! * **It never names a remote.** One is resolved at run time from git config
//!   by the same ladder `scripts/wip-post-commit.sh` uses. Two pre-push guards
//!   in this repository hard-coded `origin` and both broke silently the day
//!   the remote names moved.
//! * **A remote is a way of getting a missing object, not a precondition for
//!   trusting one.** A commit already in the seat's repository is established
//!   without resolving a remote at all.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Queued by an observation, with no attempt started yet.
pub const ASSIGNMENT_INPUT_INTENDED: &str = "intended";

/// An attempt has been started; this is written before git runs.
pub const ASSIGNMENT_INPUT_ESTABLISHING: &str = "establishing";

/// Two attempts were started and neither finished; nothing tries again.
pub const ASSIGNMENT_INPUT_ABANDONED: &str = "establish_abandoned";

/// The tree was moved to the commit the assignment names.
pub const ASSIGNMENT_INPUT_ESTABLISHED: &str = "established";

/// The tree already held that commit on that branch; nothing was written.
pub const ASSIGNMENT_INPUT_ALREADY_CURRENT: &str = "already_current";

/// How many attempts may be *started* for one assignment before the queue
/// stops replaying it.
///
/// Two, not one: the ordinary interruption is a person quitting the app
/// mid-checkout, and refusing to finish that would be worse than finishing it.
/// Three would be a loop with extra steps.
pub const MAX_ESTABLISH_ATTEMPTS: u32 = 2;

/// Upper bound on remembered assignment-input attempts.
///
/// One per assignment, replaced in place on a retry. Over the cap the oldest
/// `recordedAt` is evicted, because the newest attempt is the one somebody is
/// asking about.
pub const MAX_ASSIGNMENT_INPUT_RECORDS: usize = 256;

/// How many `git status --porcelain` lines are quoted back in `detail`.
const REFUSAL_DETAIL_LINES: usize = 5;

/// Largest store document this reader will parse, matching the desktop's own
/// readiness limit on the same file.
const MAX_STORE_BYTES: u64 = 1024 * 1024;

/// Every attempt this host has made, keyed by assignment id.
pub type AssignmentInputRecords = BTreeMap<String, AssignmentInputRecord>;

/// One attempt to establish an assignment's input, kept after the fact.
///
/// Written on every attempt, successful or refused, so a later check can say
/// what this computer did without re-running git — and so a retry can replay
/// the inputs from the assignment id alone. Paths here name one person's disk
/// and are never published; the record lives in the host-local store for
/// exactly that reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentInputRecord {
    /// The assignment this attempt answers, 64 lowercase hex.
    pub assignment_id: String,
    /// The session the seat belongs to, when the attempt got that far.
    pub session_ref: Option<String>,
    /// The seat label inside that session.
    pub seat_label: Option<String>,
    /// The commit that was asked for — not necessarily the one established.
    pub commit: Option<String>,
    /// The branch that was asked for, when the caller named one.
    pub branch: Option<String>,
    /// The worktree the caller resolved from its own record.
    pub path: Option<PathBuf>,
    /// The remote the ladder resolved, when it got that far.
    pub remote: Option<String>,
    /// `established`, `already_current`, `intended`, `establishing`,
    /// `establish_abandoned`, or one of the refusal codes.
    pub outcome: String,
    /// The refusal sentence, when this attempt was refused.
    pub message: Option<String>,
    /// The uncommitted-change count a `dirty_tree` refusal measured.
    #[serde(default)]
    pub changes: Option<u32>,
    /// How many attempts have been *started* for this assignment here.
    ///
    /// Counted before the git work rather than after it, which is the only
    /// count that can bound a replay: a record left mid-attempt by a quit or a
    /// crash is indistinguishable from one never tried.
    #[serde(default)]
    pub attempts: u32,
    /// Whether the blocker for a terminal *failure* has already been published
    /// to the lead.
    ///
    /// The bounded blocker is published once per assignment
    /// ([`crate::verification_input`]), and this is what makes "once" survive
    /// a restart: the record is the only thing that outlives the process.
    /// Cleared by [`requeue_assignment_input`], because a person asking again
    /// is a new question.
    #[serde(default)]
    pub blocker_published: bool,
    /// When the attempt finished, ISO-8601.
    pub recorded_at: String,
}

impl AssignmentInputRecord {
    /// A fresh intent for a commit nobody has attempted yet.
    #[must_use]
    pub fn intended(intent: &AssignmentIntent, checkout: Option<&SeatCheckout>) -> Self {
        Self {
            assignment_id: intent.assignment_id.clone(),
            session_ref: Some(intent.session_ref.clone()),
            seat_label: intent.seat_label.clone(),
            commit: Some(intent.base_sha.clone()),
            branch: intent.branch.clone(),
            path: checkout.map(|checkout| checkout.path.clone()),
            remote: None,
            outcome: ASSIGNMENT_INPUT_INTENDED.to_owned(),
            message: None,
            changes: None,
            attempts: 0,
            blocker_published: false,
            recorded_at: now_iso(),
        }
    }

    /// True while this host still owes an answer for the assignment.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        outcome_is_pending(&self.outcome)
    }

    /// True when the tree was measured to hold the commit that was asked for.
    #[must_use]
    pub fn is_established(&self) -> bool {
        outcome_is_established(&self.outcome)
    }
}

/// True for an outcome the host still owes an answer for.
#[must_use]
pub fn outcome_is_pending(outcome: &str) -> bool {
    outcome == ASSIGNMENT_INPUT_INTENDED || outcome == ASSIGNMENT_INPUT_ESTABLISHING
}

/// True for the two outcomes that mean the tree holds the named commit.
#[must_use]
pub fn outcome_is_established(outcome: &str) -> bool {
    outcome == ASSIGNMENT_INPUT_ESTABLISHED || outcome == ASSIGNMENT_INPUT_ALREADY_CURRENT
}

/// The current UTC instant in the exact shape the desktop writes.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// What the host established, measured after the fact rather than intended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstablishedAssignmentInput {
    /// The seat worktree the caller resolved.
    pub path: String,
    /// The branch the commit is checked out on.
    pub branch: String,
    /// The commit `HEAD` resolved to *after* the checkout, re-read from git.
    pub commit: String,
    /// The remote the objects were fetched from — never a constant, and
    /// `null` when nothing had to be fetched.
    pub remote: Option<String>,
    /// True when the tree already had that commit on that branch.
    pub already_current: bool,
}

/// Why the host refused, by name.
///
/// Every variant is a refusal the caller can act on. None of them is a
/// fallback for "something went wrong": each step maps its own failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstablishAssignmentInputCode {
    /// No record of a worktree for that seat, or no host store to read.
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
    #[must_use]
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
    pub changes: Option<u32>,
}

/// Build a refusal without evidence fields.
#[must_use]
pub fn refuse(
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

/// The seat's tree, as the caller's own record names it.
///
/// Deliberately an input rather than something this module looks up: the
/// desktop reads it from the `worktrees` map it wrote when it cut the tree,
/// and the provider reads it from the session record it is about to open a
/// turn against. Adopting a tree neither of them recorded is how a seat's
/// uncommitted work would get moved out from under somebody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatCheckout {
    /// Absolute path of the worktree directory.
    pub path: PathBuf,
    /// The branch this host cut the tree on.
    pub branch: String,
}

/// What a caller asked for. Everything but the assignment id may be absent,
/// in which case the last recorded attempt for that assignment supplies it.
#[derive(Debug, Clone, Default)]
pub struct EstablishAssignmentInputRequest {
    /// The assignment this attempt answers, 64 lowercase hex.
    pub assignment_id: String,
    /// The umbrella session the seat belongs to.
    pub session_ref: Option<String>,
    /// The seat label inside that session.
    pub seat_label: Option<String>,
    /// The commit to establish.
    pub commit: Option<String>,
    /// The branch to put it on, when the caller names one.
    pub branch: Option<String>,
}

/// One folded assignment, as its observer read it.
///
/// Deliberately not the signed event: the caller has already verified and
/// folded it, and this module acts on the assignment's *fields*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentIntent {
    /// The signed assignment event id, 64 lowercase hex.
    pub assignment_id: String,
    /// The umbrella session the seat belongs to.
    pub session_ref: String,
    /// The seat label the caller resolved, when it could resolve one.
    pub seat_label: Option<String>,
    /// The signed `baseSha`.
    pub base_sha: String,
    /// The signed branch, when the assignment carried one.
    pub branch: Option<String>,
}

/// What the host has to say about one observed assignment's input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentInputDisposition {
    /// The role does not start from a named revision.
    NotRequired,
    /// An input-bound role whose assignment names no commit. Nothing was
    /// queued and nothing could be.
    Unnamed,
    /// The observation could not be read as one: an id or a commit that is
    /// not an object id. Said out loud rather than dropped.
    Invalid,
    /// This host has no tree for that seat, so it has nothing to move.
    OffHost,
    /// There is a durable record for the assignment; read it.
    Recorded,
}

/// A lowercase hex object id of one of git's two widths.
#[must_use]
pub fn is_object_id(value: &str) -> bool {
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
#[must_use]
pub fn is_safe_branch_name(value: &str) -> bool {
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

/// File one attempt under its assignment id, replacing any earlier attempt.
///
/// Replacement rather than append is the point: an assignment has one current
/// input, and a list of attempts would leave every reader to work out which
/// one is in force.
pub fn record_assignment_input(
    records: &mut AssignmentInputRecords,
    record: AssignmentInputRecord,
) {
    let key = record.assignment_id.clone();
    records.insert(key.clone(), record);
    while records.len() > MAX_ASSIGNMENT_INPUT_RECORDS {
        let Some(oldest) = records
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
        records.remove(&oldest);
    }
}

/// The last attempt recorded for one assignment, if any.
#[must_use]
pub fn assignment_input<'a>(
    records: &'a AssignmentInputRecords,
    assignment_id: &str,
) -> Option<&'a AssignmentInputRecord> {
    records.get(assignment_id.trim())
}

/// Record the intent to establish one assignment's input.
///
/// A record that already names the **same** commit is dispositive and is left
/// exactly as it is — including a refusal, which is why nothing loops. A
/// record for a *different* commit is no answer about this one, so it is
/// replaced by a fresh intent with the attempt count back at zero.
pub fn queue_intent(
    records: &mut AssignmentInputRecords,
    intent: &AssignmentIntent,
    checkout: Option<&SeatCheckout>,
) -> AssignmentInputDisposition {
    let assignment_id = intent.assignment_id.trim();
    if assignment_id.len() != 64 || !is_object_id(assignment_id) {
        return AssignmentInputDisposition::Invalid;
    }
    let commit = intent.base_sha.trim();
    if commit.is_empty() {
        return AssignmentInputDisposition::Unnamed;
    }
    if !is_object_id(commit) {
        return AssignmentInputDisposition::Invalid;
    }
    if intent.session_ref.trim().is_empty() || checkout.is_none() {
        return AssignmentInputDisposition::OffHost;
    }
    let already = assignment_input(records, assignment_id)
        .is_some_and(|record| record.commit.as_deref() == Some(commit));
    if already {
        return AssignmentInputDisposition::Recorded;
    }
    let mut intent = intent.clone();
    intent.assignment_id = assignment_id.to_owned();
    intent.base_sha = commit.to_owned();
    intent.branch = intent
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_owned);
    record_assignment_input(records, AssignmentInputRecord::intended(&intent, checkout));
    AssignmentInputDisposition::Recorded
}

/// Every assignment still owed an input, in key order.
///
/// Key order, not assignment order: the queue is drained one at a time and
/// each item is independent, so the only property that matters is that all of
/// them are reached. The signed order is not recoverable from the store and
/// pretending otherwise would be a fiction.
#[must_use]
pub fn pending_assignment_inputs(records: &AssignmentInputRecords) -> Vec<String> {
    records
        .iter()
        .filter(|(_, record)| record.is_pending())
        .map(|(key, _)| key.clone())
        .collect()
}

/// Put a settled assignment back in the queue, from a person's own request.
///
/// The attempt count goes back to zero and the published blocker is forgotten:
/// a person asking again is a new question, and the bound exists to stop *the
/// host* repeating itself.
pub fn requeue_assignment_input(records: &mut AssignmentInputRecords, assignment_id: &str) -> bool {
    let Some(record) = assignment_input(records, assignment_id).cloned() else {
        return false;
    };
    let mut requeued = record;
    requeued.outcome = ASSIGNMENT_INPUT_INTENDED.to_owned();
    requeued.message = None;
    requeued.changes = None;
    requeued.attempts = 0;
    requeued.blocker_published = false;
    requeued.recorded_at = now_iso();
    record_assignment_input(records, requeued);
    true
}

/// Drain the queue, one assignment at a time, persisting as it goes.
///
/// `resolve` answers where that seat's tree is, from the caller's own record;
/// a record it cannot place is refused `unrecorded_tree` rather than searched
/// for. `persist` is called after the attempt count is written and again after
/// each terminal outcome, because the count is only a bound if it reaches the
/// disk before the git work it is counting.
///
/// Returns the assignment ids it touched, in the order it touched them.
pub fn drain_pending_assignment_inputs(
    records: &mut AssignmentInputRecords,
    resolve: impl Fn(&AssignmentInputRecord) -> Option<SeatCheckout>,
    mut persist: impl FnMut(&AssignmentInputRecords),
) -> Vec<String> {
    let mut drained = Vec::new();
    for assignment_id in pending_assignment_inputs(records) {
        let Some(record) = assignment_input(records, &assignment_id).cloned() else {
            continue;
        };
        let checkout = resolve(&record);
        if drain_one_assignment_input(records, &assignment_id, checkout.as_ref(), &mut persist) {
            drained.push(assignment_id);
        }
    }
    drained
}

/// Take one pending assignment from `intended` to a terminal outcome.
///
/// Returns false for a record that is absent or already settled — the only
/// two states in which nothing is owed. The attempt count is written and
/// persisted *before* the git work, because the count is only a bound if it
/// reaches the disk before the thing it is counting.
pub fn drain_one_assignment_input(
    records: &mut AssignmentInputRecords,
    assignment_id: &str,
    checkout: Option<&SeatCheckout>,
    persist: &mut impl FnMut(&AssignmentInputRecords),
) -> bool {
    let Some(record) = assignment_input(records, assignment_id).cloned() else {
        return false;
    };
    if !record.is_pending() {
        return false;
    }
    if record.attempts >= MAX_ESTABLISH_ATTEMPTS {
        record_assignment_input(records, abandon(record));
        persist(records);
        return true;
    }
    record_assignment_input(records, start_attempt(record));
    persist(records);
    // The attempt files its own terminal record, carrying the count forward.
    // Its refusal is the record; nothing here re-reads it, and nothing here
    // tries again.
    let _ = establish(
        records,
        &EstablishAssignmentInputRequest {
            assignment_id: assignment_id.to_owned(),
            ..EstablishAssignmentInputRequest::default()
        },
        checkout,
    );
    persist(records);
    true
}

/// The record an attempt that took the app down twice settles as.
fn abandon(record: AssignmentInputRecord) -> AssignmentInputRecord {
    let mut abandoned = record;
    abandoned.outcome = ASSIGNMENT_INPUT_ABANDONED.to_owned();
    abandoned.message = Some(format!(
        "{} attempts to establish this input were started on this computer and none finished, so \
         it will not be tried again on its own",
        abandoned.attempts
    ));
    abandoned.recorded_at = now_iso();
    abandoned
}

/// The record that says an attempt has been started, count already raised.
fn start_attempt(record: AssignmentInputRecord) -> AssignmentInputRecord {
    let mut started = record;
    started.outcome = ASSIGNMENT_INPUT_ESTABLISHING.to_owned();
    started.attempts += 1;
    started.message = None;
    started.recorded_at = now_iso();
    started
}

/// The inputs an attempt will run with, after the recorded attempt has filled
/// in whatever the caller left out.
struct ResolvedRequest {
    session_ref: String,
    /// The seat's label, when the caller knows one.
    ///
    /// Optional because the two callers know different things: the desktop
    /// resolves a tree *by* `<sessionRef>/<seatLabel>`, while the provider
    /// resolves it from the session record it is about to open a turn against
    /// and has no label to offer. It is descriptive either way — the tree
    /// itself is always supplied by the caller.
    seat_label: Option<String>,
    commit: String,
    branch: Option<String>,
}

/// The inputs one attempt was made with, filled in as far as it got.
#[derive(Debug, Clone, Default)]
struct AttemptFacts {
    session_ref: Option<String>,
    seat_label: Option<String>,
    commit: Option<String>,
    branch: Option<String>,
    path: Option<PathBuf>,
    remote: Option<String>,
}

fn blank_to_none(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Fill the request from the last recorded attempt, then validate it.
fn resolve_request(
    records: &AssignmentInputRecords,
    request: &EstablishAssignmentInputRequest,
) -> Result<ResolvedRequest, EstablishAssignmentInputError> {
    let previous = assignment_input(records, &request.assignment_id);
    let session_ref = blank_to_none(request.session_ref.as_ref())
        .or_else(|| previous.and_then(|record| record.session_ref.clone()));
    let seat_label = blank_to_none(request.seat_label.as_ref())
        .or_else(|| previous.and_then(|record| record.seat_label.clone()));
    let commit = blank_to_none(request.commit.as_ref())
        .or_else(|| previous.and_then(|record| record.commit.clone()));
    // The recorded branch is replayed whenever the caller names none, even
    // when the caller *does* name a new commit: moving the same seat branch to
    // a new revision is the ordinary retry.
    let branch = blank_to_none(request.branch.as_ref())
        .or_else(|| previous.and_then(|record| record.branch.clone()));

    let mut missing = Vec::new();
    if session_ref.is_none() {
        missing.push("sessionRef");
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
    let (Some(session_ref), Some(commit)) = (session_ref, commit) else {
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
                Some(branch.to_owned()),
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

/// Run git in `tree` with a hermetic environment.
///
/// Global and system config are off, so nothing here can read or write a
/// person's `~/.gitconfig`, and `GIT_TERMINAL_PROMPT=0` means a fetch that
/// needs a credential fails and says so instead of waiting forever on a prompt
/// no one will answer. The `GIT_DIR` family comes from the single shared
/// [`crate::git_probe::GIT_REPO_SELECTION_VARS`] list.
fn git(tree: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new("git");
    command.args(args);
    command.current_dir(tree);
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    // Git for Windows maps `/dev/null` to `NUL`, so this disables the global
    // file on every platform git supports.
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    for key in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// A `git config --get` that treats "unset" as `None`, since git exits 1 for
/// both an unset key and a failure.
fn config_get(tree: &Path, key: &str) -> Option<String> {
    git(tree, &["config", "--get", key])
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The remotes this repository actually has.
fn configured_remotes(tree: &Path) -> Vec<String> {
    git(tree, &["remote"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Resolve the remote the same way `scripts/wip-post-commit.sh` does.
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
            format!(
                "git config names the remote '{candidate}', which this repository has no remote for"
            ),
            Some(if remotes.is_empty() {
                "no remotes are configured".to_owned()
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
/// that request, so the second attempt is an ordinary fetch.
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
    records: &AssignmentInputRecords,
    request: &EstablishAssignmentInputRequest,
    checkout: Option<&SeatCheckout>,
    facts: &mut AttemptFacts,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let resolved = resolve_request(records, request)?;
    facts.session_ref = Some(resolved.session_ref.clone());
    facts.seat_label.clone_from(&resolved.seat_label);
    facts.commit = Some(resolved.commit.clone());
    facts.branch.clone_from(&resolved.branch);

    let Some(checkout) = checkout else {
        return Err(refuse(
            EstablishAssignmentInputCode::UnrecordedTree,
            "this host did not record cutting a worktree for that seat",
            Some(format!(
                "{}/{}",
                resolved.session_ref,
                resolved.seat_label.as_deref().unwrap_or("<unnamed seat>")
            )),
        ));
    };
    let tree = checkout.path.clone();
    facts.path = Some(tree.clone());
    if !tree.is_dir() {
        return Err(refuse(
            EstablishAssignmentInputCode::MissingTree,
            "the recorded worktree directory is gone",
            Some(tree.to_string_lossy().into_owned()),
        ));
    }
    if git(&tree, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Ok("true") {
        return Err(refuse(
            EstablishAssignmentInputCode::MissingTree,
            "the recorded path is no longer a git worktree",
            Some(tree.to_string_lossy().into_owned()),
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
        // changes is not a number anybody acts on.
        refusal.changes = Some(u32::try_from(dirty.len()).unwrap_or(u32::MAX));
        return Err(refusal);
    }

    let current_branch = git(&tree, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|branch| branch.trim().to_owned())
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
        .unwrap_or_else(|| checkout.branch.clone());
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
    let is_seats_own = target == checkout.branch
        || current_branch.as_deref() == Some(target.as_str())
        || !branch_exists(&tree, &target);
    if !is_seats_own {
        return Err(refuse(
            EstablishAssignmentInputCode::CheckoutFailed,
            format!("'{target}' is not this seat's branch, so it was not moved"),
            Some(format!("the seat's branch is '{}'", checkout.branch)),
        ));
    }

    let head = git(&tree, &["rev-parse", "HEAD"]).unwrap_or_default();
    if head == resolved.commit && current_branch.as_deref() == Some(target.as_str()) {
        return Ok(EstablishedAssignmentInput {
            path: tree.to_string_lossy().into_owned(),
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

    // Measured, not assumed: the whole point is that the caller does not have
    // to take anyone's word for where the tree ended up.
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
        .to_owned();
    if landed_branch != target {
        return Err(refuse(
            EstablishAssignmentInputCode::CheckoutFailed,
            format!("the commit is checked out on '{landed_branch}', not '{target}'"),
            None,
        ));
    }
    Ok(EstablishedAssignmentInput {
        path: tree.to_string_lossy().into_owned(),
        branch: target,
        commit: head,
        remote,
        already_current: false,
    })
}

/// Run one attempt against an in-memory record set and file its record.
///
/// The caller supplies the seat's tree; everything else — the dirty check, the
/// remote ladder, the checkout and the measurement afterwards — is this
/// function's, and every outcome becomes exactly one record.
pub fn establish(
    records: &mut AssignmentInputRecords,
    request: &EstablishAssignmentInputRequest,
    checkout: Option<&SeatCheckout>,
) -> Result<EstablishedAssignmentInput, EstablishAssignmentInputError> {
    let assignment_id = request.assignment_id.trim().to_owned();
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
    // Read before the attempt: the count and the published blocker belong to
    // the assignment, not to this attempt's record, and
    // `record_assignment_input` replaces the row rather than merging it.
    let (attempts, blocker_published) = assignment_input(records, &assignment_id)
        .map_or((0, false), |record| {
            (record.attempts, record.blocker_published)
        });
    let mut facts = AttemptFacts::default();
    let outcome = attempt(records, request, checkout, &mut facts);
    let (result_outcome, message) = match &outcome {
        Ok(established) if established.already_current => {
            (ASSIGNMENT_INPUT_ALREADY_CURRENT.to_owned(), None)
        }
        Ok(_) => (ASSIGNMENT_INPUT_ESTABLISHED.to_owned(), None),
        Err(error) => (error.code.as_str().to_owned(), Some(error.message.clone())),
    };
    let changes = outcome.as_ref().err().and_then(|error| error.changes);
    record_assignment_input(
        records,
        AssignmentInputRecord {
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
            blocker_published,
            recorded_at: now_iso(),
        },
    );
    outcome
}

/// The file a desktop host writes beside the provider's projects view, naming
/// where its `coding-session-workdirs.json` lives.
///
/// The path cannot be derived: the store sits in the app's *config* directory
/// and this provider's state directory hangs off the app's *data* directory,
/// which are the same folder on macOS and Windows and different ones on Linux.
/// Deriving it would be a guess on one platform, so the host says it instead,
/// in the same write that materializes `projects.json`.
pub const HOST_STORE_POINTER_FILE: &str = "host-workdir-store.json";

/// Schema version of [`HostStorePointer`].
pub const HOST_STORE_POINTER_VERSION: u32 = 1;

/// Where this computer's desktop host keeps the shared record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStorePointer {
    /// Schema version; a document from the future is ignored, not guessed at.
    pub version: u32,
    /// Absolute path of `coding-session-workdirs.json`.
    pub path: PathBuf,
}

/// Resolve the host store this provider shares, or `None` with the reason
/// logged.
///
/// `None` is an ordinary answer, not a failure: a provider running without a
/// desktop host beside it has no seat worktrees recorded anywhere, so there is
/// no tree it could move and nothing is lost by saying so.
#[must_use]
pub fn host_store_from_pointer(state_dir: &Path) -> Option<AssignmentInputStore> {
    let pointer_path = state_dir.join(HOST_STORE_POINTER_FILE);
    let raw = match std::fs::read(&pointer_path) {
        Ok(raw) => raw,
        Err(error) => {
            tracing::debug!(
                target: "csp::assignment_inputs",
                path = %pointer_path.display(),
                %error,
                "no host workdir-store pointer; this provider establishes no inputs"
            );
            return None;
        }
    };
    let pointer: HostStorePointer = match serde_json::from_slice(&raw) {
        Ok(pointer) => pointer,
        Err(error) => {
            tracing::warn!(
                target: "csp::assignment_inputs",
                %error,
                "the host workdir-store pointer could not be read"
            );
            return None;
        }
    };
    if pointer.version != HOST_STORE_POINTER_VERSION || !pointer.path.is_absolute() {
        tracing::warn!(
            target: "csp::assignment_inputs",
            version = pointer.version,
            path = %pointer.path.display(),
            "the host workdir-store pointer names a version or a path this build will not use"
        );
        return None;
    }
    Some(AssignmentInputStore::new(pointer.path))
}

/// The host-local store, as this module reads and writes it.
///
/// Only `assignmentInputs` is understood; every other key travels through
/// [`Self::rest`] untouched, which is what lets the desktop and its sidecar
/// write the same file without either clobbering the other's keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentInputDocument {
    /// Every attempt recorded on this computer.
    #[serde(default)]
    pub assignment_inputs: AssignmentInputRecords,
    /// Every other key of the desktop's record, carried through verbatim.
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// Take the shared advisory lock that serializes writers of one store file.
///
/// The lock is the sibling `.lock` file, opened `O_NOFOLLOW` at mode `0600`
/// and locked with [`std::fs::File::lock`]. Held from before the read until
/// after the write, so two writers — the desktop app and its sidecar provider
/// are the two that matter — serialize rather than interleave. A lock path
/// that is a symlink or not a regular file is refused rather than followed.
///
/// The returned file must be kept alive for the whole mutation: dropping it
/// releases the lock.
pub fn lock_store_file(store_path: &Path) -> Result<std::fs::File, String> {
    let path = store_path.with_extension("lock");
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err("The workdir store lock must be a regular file.".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(&path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("The workdir store lock must be a regular file.".into());
    }
    file.lock().map_err(|error| error.to_string())?;
    Ok(file)
}

/// The shared host store, addressed by path.
///
/// One instance per store file. Every mutation goes through
/// [`Self::with_records`], which holds the lock for the whole read-act-write.
#[derive(Debug, Clone)]
pub struct AssignmentInputStore {
    path: PathBuf,
}

impl AssignmentInputStore {
    /// Address the store at `path`, without touching it.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The store file this instance reads and writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the document, refusing rather than inventing one.
    ///
    /// A missing file is an error, not an empty document: the desktop owns
    /// this file's schema, and writing one from here would mean guessing the
    /// version and defaults of keys this module deliberately does not model.
    pub fn load(&self) -> Result<AssignmentInputDocument, String> {
        let file = std::fs::File::open(&self.path)
            .map_err(|error| format!("failed to read the host workdir store: {error}"))?;
        if file
            .metadata()
            .map_err(|error| format!("failed to inspect the host workdir store: {error}"))?
            .len()
            > MAX_STORE_BYTES
        {
            return Err("the host workdir store exceeds its readiness limit".into());
        }
        let document: AssignmentInputDocument = serde_json::from_reader(file)
            .map_err(|error| format!("failed to parse the host workdir store: {error}"))?;
        if document.assignment_inputs.len() > MAX_ASSIGNMENT_INPUT_RECORDS {
            return Err("the host workdir store exceeds its record limits".into());
        }
        Ok(document)
    }

    /// Write the document back, atomically and owner-only.
    pub fn save(&self, document: &AssignmentInputDocument) -> Result<(), String> {
        save_document(&self.path, document)
    }

    /// Run one mutation under the lock: lock, load, act, save.
    ///
    /// `act` is handed a `persist` it may call as often as it likes — the
    /// drain uses it to get the attempt count onto the disk before the git
    /// work it bounds. A `persist` that fails is reported where it happens and
    /// does not abandon the mutation: the tree is already in the state the
    /// record describes, so what is lost is the record, and saying so is
    /// better than pretending the checkout did not happen.
    pub fn with_records<T>(
        &self,
        act: impl FnOnce(&mut AssignmentInputRecords, &mut dyn FnMut(&AssignmentInputRecords)) -> T,
    ) -> Result<T, String> {
        let _lock = lock_store_file(&self.path)?;
        let mut document = self.load()?;
        let rest = document.rest.clone();
        let path = self.path.clone();
        let mut records = std::mem::take(&mut document.assignment_inputs);
        let mut persist = |records: &AssignmentInputRecords| {
            let snapshot = AssignmentInputDocument {
                assignment_inputs: records.clone(),
                rest: rest.clone(),
            };
            if let Err(error) = save_document(&path, &snapshot) {
                tracing::error!(
                    target: "csp::assignment_inputs",
                    %error,
                    "an assignment-input record could not be saved"
                );
            }
        };
        let answer = act(&mut records, &mut persist);
        persist(&records);
        Ok(answer)
    }
}

/// Write one document to `path`, atomically and owner-only.
fn save_document(path: &Path, document: &AssignmentInputDocument) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(document)
        .map_err(|error| format!("failed to serialize the host workdir store: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    write_restricted(&temporary, &payload)?;
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("failed to replace the host workdir store: {error}"))
}

/// Take one assignment from its recorded intent to a terminal outcome,
/// holding the store's lock only around the two writes.
///
/// The git work — which may include a fetch, and may take as long as a network
/// takes — runs **outside** the lock. Holding an advisory lock on a file the
/// desktop app writes on every preference change, for the length of a fetch,
/// would stall a person's UI on somebody else's network.
///
/// What the lock still covers is what it has to: the started-attempt count
/// reaches the disk before the git work begins, and the terminal record
/// replaces it afterwards. A process that dies in between leaves
/// `establishing` with the count already raised, which is exactly the state
/// [`MAX_ESTABLISH_ATTEMPTS`] bounds — one replay, then
/// [`ASSIGNMENT_INPUT_ABANDONED`].
///
/// Returns the record as it stands afterwards. A record that was already
/// settled is returned unchanged and nothing runs, so two callers racing over
/// one assignment cost one establishment.
pub fn establish_recorded_assignment(
    store: &AssignmentInputStore,
    assignment_id: &str,
    checkout: &SeatCheckout,
) -> Result<AssignmentInputRecord, String> {
    enum Step {
        Settled(AssignmentInputRecord),
        Started(AssignmentInputRecord),
    }

    let step = store.with_records(|records, persist| {
        let record = assignment_input(records, assignment_id).cloned()?;
        if !record.is_pending() {
            return Some(Step::Settled(record));
        }
        if record.attempts >= MAX_ESTABLISH_ATTEMPTS {
            let abandoned = abandon(record);
            record_assignment_input(records, abandoned.clone());
            persist(records);
            return Some(Step::Settled(abandoned));
        }
        let started = start_attempt(record);
        record_assignment_input(records, started.clone());
        persist(records);
        Some(Step::Started(started))
    })?;
    let started = match step {
        None => return Err(format!("no assignment-input record for {assignment_id}")),
        Some(Step::Settled(record)) => return Ok(record),
        Some(Step::Started(started)) => started,
    };

    // Unlocked, against a scratch record set holding only this assignment:
    // the inputs it replays are the ones just written, and nothing else in the
    // store can be touched by a failure here.
    let mut scratch = AssignmentInputRecords::new();
    record_assignment_input(&mut scratch, started);
    let _ = establish(
        &mut scratch,
        &EstablishAssignmentInputRequest {
            assignment_id: assignment_id.to_owned(),
            ..EstablishAssignmentInputRequest::default()
        },
        Some(checkout),
    );
    let Some(mut terminal) = scratch.get(assignment_id).cloned() else {
        return Err(format!("the attempt for {assignment_id} filed no record"));
    };

    store.with_records(|records, _| {
        // Whether a blocker was already published belongs to the assignment,
        // not to this attempt, and another writer may have set it while the
        // git work ran.
        terminal.blocker_published =
            assignment_input(records, assignment_id).is_some_and(|record| record.blocker_published);
        record_assignment_input(records, terminal.clone());
    })?;
    Ok(terminal)
}

/// Claim the one blocker a failed establishment is allowed to publish.
///
/// Answers true exactly once per assignment: the flag is durable, so a restart
/// between the claim and the publish costs the message rather than repeating
/// it, and a lead is never told the same thing twice by a retry loop.
/// [`requeue_assignment_input`] clears it, because a person asking again is a
/// new question.
pub fn claim_establishment_blocker(
    store: &AssignmentInputStore,
    assignment_id: &str,
) -> Result<bool, String> {
    store.with_records(|records, persist| {
        let Some(record) = assignment_input(records, assignment_id).cloned() else {
            return false;
        };
        if record.blocker_published {
            return false;
        }
        let mut claimed = record;
        claimed.blocker_published = true;
        record_assignment_input(records, claimed);
        persist(records);
        true
    })
}

/// Write `payload` to `path` through a temporary file, owner-readable only.
fn write_restricted(path: &Path, payload: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("failed to open the host workdir store for writing: {error}"))?;
    file.write_all(payload)
        .map_err(|error| format!("failed to write the host workdir store: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("failed to flush the host workdir store: {error}"))
}

#[cfg(test)]
#[path = "assignment_inputs_tests.rs"]
mod tests;
