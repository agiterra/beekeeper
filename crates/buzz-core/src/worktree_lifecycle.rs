//! What may be done with a coding session's git worktree, decided from facts.
//!
//! A worktree is created by a machine and, until this module existed, deleted
//! by nobody: 65 trees and 298 GB accumulated on one developer's disk before
//! anything could say which of them were finished. The hard part is not the
//! removal, it is being *sure* — a directory holding a person's uncommitted
//! work must never be removed, and never silently.
//!
//! So the decision is pure and lives here, where a test can reach it. Nothing
//! in this module touches the filesystem, runs `git`, or reads the network.
//! The caller gathers the facts; this decides what they mean.
//!
//! # The facts, and what each one is worth
//!
//! * `session_settled` — a 44230 closure revision whose action is `closed` or
//!   `archived`. Until that exists the session is still someone's live work.
//! * `session_deleted` — an accepted whole-session deletion (kind 5 over the
//!   session's genesis and its chain). A deletion is not a closure — no 44230
//!   survives it, because the deletion removes them too — but it settles the
//!   session's work just as finally, and the host that held its trees must
//!   dispose of them under the same rules. Before this fact existed a deleted
//!   session's trees read `not-settled` for ever and no reaper ever ran over
//!   them (ledger 135(f)).
//! * `execution_live` — a provider execution is still running in the tree.
//! * `tip_on_relay` — the branch tip is named by, or is an ancestor of, a ref
//!   in a **relay-signed kind 30618** for that repository. Because 30618 is
//!   parameterized-replaceable it says where a ref stands **now** and is never
//!   a push history: a branch pushed and then deleted on the relay reads as
//!   not-on-relay, correctly, because the commits are no longer reachable
//!   there. Say that wherever the answer is shown.
//! * `dirty_files` — the number of `git status --porcelain` lines. That
//!   listing **excludes ignored paths**, so `target/` and `node_modules`
//!   never make a finished tree look like it holds edits.
//! * `recorded` — the host wrote this tree into its own durable record when it
//!   created it. A tree the host merely *found* is never recorded, is never
//!   removed by the host, and is only ever listed.
//! * `is_protected` — the hot development checkout, a production checkout, the
//!   repository's main worktree, or the running process's own directory.
//!
//! # The grace window
//!
//! A clean, pushed seat worktree is not removed the instant its session
//! closes: it survives [`SEAT_WORKTREE_GRACE_SECS`] measured **from the
//! closure**, so a person who closes a session and then wants the directory
//! back has a week to say so. Build output is the exception and is handled by
//! [`build_output_reclaimable`], which needs no grace at all.

/// How long a clean, pushed seat worktree survives after its session closes.
///
/// Seven days from the closure, not from the last commit: the closure is the
/// moment a person declared the work finished, and it is the only one of the
/// two that is a decision rather than an accident of when someone last typed.
pub const SEAT_WORKTREE_GRACE_SECS: u64 = 7 * 24 * 60 * 60;

/// Every fact [`classify_seat_worktree`] is allowed to consider.
///
/// Deliberately plain data. A caller that cannot establish one of these
/// answers `false` for it, which always makes the disposition *more*
/// conservative, never less.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeatWorktreeFacts {
    /// A 44230 revision for this session folds to `closed` or `archived`.
    pub session_settled: bool,
    /// An accepted whole-session deletion removed this session from the relay.
    ///
    /// Settles the session exactly as a closure does, and *only* that: every
    /// protection below still applies in full, so a deleted session holding
    /// uncommitted work is still [`SeatWorktreeDisposition::Held`] and is
    /// still removed by a person rather than by a sweep.
    pub session_deleted: bool,
    /// A provider execution is still running against this tree.
    pub execution_live: bool,
    /// The branch tip is named by, or an ancestor of, a ref in a relay-signed
    /// kind 30618 for this repository.
    pub tip_on_relay: bool,
    /// Lines of `git status --porcelain`, which exclude ignored paths.
    pub dirty_files: u32,
    /// The host's own durable record names this tree.
    pub recorded: bool,
    /// The tree is one nothing may ever remove.
    pub is_protected: bool,
    /// Seconds elapsed since the closure revision — or the deletion — settled
    /// the session.
    ///
    /// `None` when the session is neither settled nor deleted, or when the
    /// event carries no timestamp the caller trusts — which holds the tree,
    /// never releases it.
    pub settled_for_secs: Option<u64>,
}

/// What may be done with one seat worktree. A closed set: every caller must
/// handle every arm, and no arm means "probably fine".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatWorktreeDisposition {
    /// Settled, pushed, clean, recorded, past its grace window. Removable.
    Prunable,
    /// It holds uncommitted work. Removed only by a person, naming the count.
    Held {
        /// How many `git status --porcelain` lines the tree reports.
        dirty_files: u32,
    },
    /// The session has neither a `closed`/`archived` closure revision nor an
    /// accepted whole-session deletion.
    NotSettled,
    /// Settled and clean, but the branch tip is not on the relay right now.
    TipNotOnRelay,
    /// An execution is still running in it.
    ExecutionLive,
    /// The host never recorded cutting this tree, so it never removes it.
    Unrecorded,
    /// The hot checkout and its kin. Nothing removes these, ever.
    Protected,
    /// Removable except that its grace window has not run out yet.
    WithinGrace {
        /// Seconds still to run before the tree becomes [`Self::Prunable`].
        remaining_secs: u64,
    },
}

impl SeatWorktreeDisposition {
    /// Whether the host may remove this tree with no further human word.
    ///
    /// Exactly one arm answers `true`. `Held` never does: uncommitted work is
    /// removed by a person's click, never by a sweep.
    pub fn is_host_prunable(self) -> bool {
        matches!(self, Self::Prunable)
    }

    /// The one stable word a surface may use for this disposition.
    ///
    /// Wire- and UI-facing callers use this rather than inventing their own
    /// spelling, so the CLI and the app never disagree about what a tree is.
    pub fn token(self) -> &'static str {
        match self {
            Self::Prunable => "prunable",
            Self::Held { .. } => "held",
            Self::NotSettled => "not-settled",
            Self::TipNotOnRelay => "tip-not-on-relay",
            Self::ExecutionLive => "execution-live",
            Self::Unrecorded => "unrecorded",
            Self::Protected => "protected",
            Self::WithinGrace { .. } => "within-grace",
        }
    }
}

/// Decide what may be done with one seat worktree.
///
/// The order of the checks *is* the policy, strongest reason first:
///
/// 1. `is_protected` — beats every other input, including a settled session
///    and a clean tree. The hot checkout is never a candidate.
/// 2. `execution_live` — an agent is writing in there right now.
/// 3. `!recorded` — the host cannot own what it never recorded cutting.
/// 4. `!session_settled && !session_deleted` — the work is not finished. A
///    deletion settles the session here and nowhere else: it buys no exemption
///    from any check below it.
/// 5. `dirty_files > 0` — [`SeatWorktreeDisposition::Held`], which outranks
///    every remaining reason because it is the one a person must see.
/// 6. `!tip_on_relay` — clean, but nothing off this disk holds the commits.
/// 7. the grace window, then [`SeatWorktreeDisposition::Prunable`].
pub fn classify_seat_worktree(facts: &SeatWorktreeFacts) -> SeatWorktreeDisposition {
    if facts.is_protected {
        return SeatWorktreeDisposition::Protected;
    }
    if facts.execution_live {
        return SeatWorktreeDisposition::ExecutionLive;
    }
    if !facts.recorded {
        return SeatWorktreeDisposition::Unrecorded;
    }
    if !facts.session_settled && !facts.session_deleted {
        return SeatWorktreeDisposition::NotSettled;
    }
    if facts.dirty_files > 0 {
        return SeatWorktreeDisposition::Held {
            dirty_files: facts.dirty_files,
        };
    }
    if !facts.tip_on_relay {
        return SeatWorktreeDisposition::TipNotOnRelay;
    }
    match facts.settled_for_secs {
        // No trusted closure timestamp holds the tree rather than releasing
        // it: an unknown age is not a long one.
        None => SeatWorktreeDisposition::WithinGrace {
            remaining_secs: SEAT_WORKTREE_GRACE_SECS,
        },
        Some(elapsed) if elapsed < SEAT_WORKTREE_GRACE_SECS => {
            SeatWorktreeDisposition::WithinGrace {
                remaining_secs: SEAT_WORKTREE_GRACE_SECS - elapsed,
            }
        }
        Some(_) => SeatWorktreeDisposition::Prunable,
    }
}

/// Whether this tree's build output may be reclaimed now.
///
/// *Which* directories those are is a separate question, answered by
/// [`crate::sandbox_manifest::reclaim_plan`] from the project's own
/// declaration, or by [`RECLAIMABLE_BUILD_DIRS`] when it declares none. This
/// decides only the timing.
///
/// Build output is rebuildable, so it is removable the moment the session
/// settles or is deleted — independently of [`SeatWorktreeDisposition::Held`],
/// of the grace window, and of whether anything was pushed. No commit can be
/// lost in any of those directories: all of them are ignored by git, which is
/// exactly why they never appear in `dirty_files`.
///
/// A live execution still blocks it: deleting `target/` under a running build
/// breaks the build rather than losing work, but breaking it is not this
/// module's to do.
pub fn build_output_reclaimable(facts: &SeatWorktreeFacts) -> bool {
    (facts.session_settled || facts.session_deleted) && !facts.execution_live && !facts.is_protected
}

/// The build directories [`build_output_reclaimable`] covers in a project
/// that declares none of its own.
///
/// Repository-relative, and still a closed list: anything not named here is
/// source until someone proves otherwise. But this is now the **fallback**,
/// not the answer. A project that ships a `sandbox.yml` is reclaimed by what
/// it declares there, because the directories worth freeing are the same ones
/// worth seeding and one declaration should not disagree with itself — see
/// [`crate::sandbox_manifest::reclaim_plan`], which names which of the two it
/// used. This list stayed `["target", "desktop/node_modules"]` while a second
/// cargo workspace grew under `desktop/src-tauri/target`, so a built tree had
/// roughly half of its build output freed by something reporting success.
pub const RECLAIMABLE_BUILD_DIRS: &[&str] = &["target", "desktop/node_modules"];

/// Render a byte count as the `{N} GB` a person reads, one decimal.
///
/// `None` — a directory whose size could not be measured — renders `unknown`,
/// never `0 GB`. A tree whose size is unknown is not an empty one, and saying
/// `0` would be a claim the caller cannot back up.
pub fn render_reclaimable_bytes(bytes: Option<u64>) -> String {
    match bytes {
        None => "unknown".to_string(),
        Some(bytes) => {
            let gigabytes = bytes as f64 / 1_000_000_000.0;
            format!("{gigabytes:.1} GB")
        }
    }
}

#[cfg(test)]
#[path = "worktree_lifecycle_tests.rs"]
mod tests;
