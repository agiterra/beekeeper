//! The seat worktrees this host cut, as records rather than as behaviour.
//!
//! Split out of `workdir_store.rs` unchanged, because that file had reached
//! the repository's 1000-line ceiling and the next change to it had to move
//! code out rather than raise a limit (plan § 8 A1.6). Nothing here is new:
//! the two record shapes, the key they are filed under, the placement
//! predicate and the one-shot migration that rescued finding 60's stranded
//! hints all read exactly as they did before the move.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{CodingSessionWorkdirStore, MAX_SEAT_WORKTREES, MIGRATED_HINT_PREFIX};
use crate::util::now_iso;

/// One git worktree this host cut for one seat, recorded when it was created.
///
/// Written **only** by the create path. Nothing that merely observed a
/// directory ever writes one of these: the whole point of the record is that
/// the host can name what it made, and a tree it did not make is a tree it
/// must not remove. Like every other field here, these paths name one
/// person's disk and are never published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionSeatWorktree {
    /// Absolute path of the worktree directory.
    pub path: PathBuf,
    /// Branch created with it, which shares the directory's slug.
    pub branch: String,
    /// Repository the worktree belongs to.
    pub repo_root: PathBuf,
    /// When the host cut it, ISO-8601.
    pub created_at: String,
    /// The producer-minted session id running in this tree, when the caller
    /// knew one.
    ///
    /// This is the whole of finding 82's fix on the host side. The provider
    /// resolves a gate row's directory by re-reading its projects file at
    /// every gate ([`CodingSessionProjectsView::sessions`]); that map is built
    /// from this field, so the moment the host records a tree at a new path
    /// the next gate is measured there. Without it the provider can only know
    /// the path the create resolved, which after a relocation is a directory
    /// that no longer exists — and every row it minted said
    /// `headSha: null, dirty: null`.
    ///
    /// `#[serde(default)]` and optional: a record written before this field,
    /// or by a caller that genuinely does not know the session id yet (a tree
    /// cut before its genesis is signed), reads as `None` and simply
    /// contributes no override, which is exactly today's behaviour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The seat's agents-repository clone beside this tree (spec § 4.11), removed with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_clone: Option<PathBuf>,
    /// The git identity the host configured on this tree before the seat ran.
    ///
    /// Recorded so a person can read what a seat's commits will be authored
    /// as without opening its config, and so a later repair knows whether this
    /// host ever set one. Absent means the create named no seat — the founder's
    /// own tree, or a pre-239 record — not that the identity failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_identity: Option<CodingSessionSeatCommitIdentity>,
    /// The seat's own actor pubkey, lowercase hex, when the caller named one.
    ///
    /// Recorded so a later read can go **actor → seat label** without
    /// guessing from `commit_identity.email` (`<pubkey8>@beekeeper.local`,
    /// which is a truncated prefix and must never be prefix-matched back to a
    /// full key). Absent on a record cut before this field existed, or on a
    /// founder's own tree, which names no seat at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_pubkey: Option<String>,
}

/// The `user.name`/`user.email` one seat worktree was configured with.
///
/// A record of what the host did, never an input to anything: the values are
/// derived from the seat's own key by
/// [`buzz_core_pkg::seat_commit_identity`], so reading a stale one back and
/// re-applying it could never be more correct than deriving it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionSeatCommitIdentity {
    /// The configured `user.name`, e.g. `builder · kettle-control`.
    pub name: String,
    /// The configured `user.email`, always `<pubkey8>@beekeeper.local`.
    pub email: String,
    /// `local` or `worktree` — which config file the two lines went to.
    pub scope: String,
}

/// One worktree this host removed, and why it was allowed to.
///
/// Kept after the directory is gone so a person can find out what happened to
/// a folder they remember. Bounded by [`MAX_PRUNED_WORKTREES`]; the oldest
/// entries fall off the end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionPrunedWorktree {
    /// Absolute path that was removed.
    pub path: PathBuf,
    /// Branch it had checked out.
    pub branch: String,
    /// Repository it belonged to.
    pub repo_root: PathBuf,
    /// When the host removed it, ISO-8601.
    pub pruned_at: String,
    /// The sentence the host would have shown for the disposition that
    /// admitted the removal — never a bare token.
    pub reason: String,
}

/// The key one seat worktree is filed under: `<sessionRef>/<seatLabel>`.
///
/// A seat is unique inside its session, so this is the whole identity. It is
/// deliberately not the path: a path can be renamed out from under the host,
/// and then the record would silently name a directory nobody cut.
pub(crate) fn seat_worktree_key(session_ref: &str, seat_label: &str) -> String {
    format!("{}/{}", session_ref.trim(), seat_label.trim())
}

/// Whether `path` sits inside the one folder worktrees are allowed to live in.
///
/// `<repo_root>.worktrees/…` and nothing else. A record naming a directory
/// outside it would give the prune path a licence over somewhere it has no
/// business, so the write is refused rather than trusted.
pub(crate) fn is_inside_worktree_parent(repo_root: &Path, path: &Path) -> bool {
    // The rule itself lives in `buzz_core::worktree_placement`, shared with
    // the prune guard and with `bee`. Three guards that disagree with where
    // placement actually cuts is a silent, total failure: the tree lands
    // somewhere no guard admits, so it can never be recorded or removed.
    buzz_core_pkg::worktree_placement::is_managed_worktree_path(repo_root, path, &[])
}

/// Repository root implied by a worktree path, from the path alone.
///
/// The inverse of the two *holder* shapes in
/// `buzz_core::worktree_placement`: `<repo>/.worktrees/<slug>` and the legacy
/// sibling container `<repo>.worktrees/<slug>`. Both name their repository
/// unambiguously, so no `git` invocation and no `stat` is needed.
///
/// The per-worktree sibling shape `<stem>-wt-<slug>` is deliberately **not**
/// inverted: `a-wt-b-wt-c` has two readings and only a stat could choose
/// between them, so a hint in that shape is left alone rather than attributed
/// to a repository that may not be its own. Say so rather than guess.
fn repo_root_of_holder_path(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors().skip(1) {
        let name = ancestor.file_name()?.to_str()?;
        if name == ".worktrees" {
            return ancestor.parent().map(Path::to_path_buf);
        }
        if let Some(stem) = name.strip_suffix(".worktrees") {
            if stem.is_empty() {
                return None;
            }
            return Some(ancestor.with_file_name(stem));
        }
    }
    None
}

/// Move the seat worktrees stranded in `pending` into `worktrees`, once.
///
/// # Why anything needs moving (live-run finding 60)
///
/// `pending` is a **one-shot create hint**, cleared the moment its receipt
/// arrives. `worktrees` is the durable record `bee sessions worktree
/// status/prune/reclaim` and Pulse's disk row read. On the machine that
/// produced the finding, `pending` held 27 seat worktrees and `worktrees` was
/// empty: the host staged every seat into the hint map and never promoted one,
/// so the product's own reclaim was blind to every tree it had cut.
///
/// # What is and is not migrated
///
/// Only a hint whose path is inside a `.worktrees` holder of the repository
/// that path itself names, checked with the same shared predicate the record
/// and prune guards use. That is what keeps a person's ordinary checkout —
/// which is also staged as a hint, on every non-worktree create — from being
/// recorded as a seat's disposable tree.
///
/// A migrated entry is keyed [`MIGRATED_HINT_PREFIX`]`<commandId>/<basename>`.
/// The prefix is load-bearing: `bee` reads the whole map so the tree becomes
/// *visible*, while no real session ref can equal the key's session half, so
/// no settlement fact ever attaches to it and `classify_seat_worktree` answers
/// `not-settled` — listable, never removable. Finding 60 asked for the trees
/// to stop being invisible, not for a sweep to start deleting them.
///
/// Idempotent: the key is derived, and an existing key is never overwritten,
/// so a second load changes nothing. The hint itself is left in `pending` —
/// it may still be steering an in-flight create, and clearing it here would
/// break that create for a record it has already made.
pub(crate) fn migrate_pending_worktrees(store: &mut CodingSessionWorkdirStore) -> usize {
    let candidates: Vec<(String, PathBuf)> = store
        .pending
        .iter()
        .map(|(command_id, path)| (command_id.clone(), path.clone()))
        .collect();
    let mut migrated = 0usize;
    for (command_id, path) in candidates {
        let Some(repo_root) = repo_root_of_holder_path(&path) else {
            continue;
        };
        if !is_inside_worktree_parent(&repo_root, &path) {
            continue;
        }
        let Some(label) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let key = format!("{MIGRATED_HINT_PREFIX}{command_id}/{label}");
        if store.worktrees.contains_key(&key) {
            continue;
        }
        if store.worktrees.len() >= MAX_SEAT_WORKTREES {
            break;
        }
        // The branch is not knowable from a path, and inventing one would put
        // a name into a record whose purpose is naming what may be removed.
        // The empty string is the honest answer and reads as "unknown" in
        // every surface, all of which refuse to remove this entry anyway.
        store.worktrees.insert(
            key,
            CodingSessionSeatWorktree {
                path,
                branch: String::new(),
                repo_root,
                created_at: now_iso(),
                session_id: None,
                agents_clone: None,
                commit_identity: None,
                actor_pubkey: None,
            },
        );
        migrated += 1;
    }
    migrated
}
