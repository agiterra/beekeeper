//! The one authority for which branch a working tree may move (ledger 275
//! A1/A2).
//!
//! A child can re-point its own worktree's `HEAD` (its administration is
//! writable), so `HEAD` is never evidence of which branch a seat owns once a
//! child has run. The authority, in order:
//!
//! 1. the host's own worktree record (`worktrees` in the desktop's workdir
//!    store, reached through [`crate::assignment_inputs::host_store_from_pointer`])
//!    — the branch the host cut the tree on;
//! 2. the branch the session's host-owned record already bound;
//! 3. a host-owned pin, `branch-pins/<sha256(tree)>` in the provider's state,
//!    written the first time the host prepared the tree;
//! 4. for a tree none of those names, its `HEAD` when the host first prepares
//!    it — before any child has run there — which is then pinned.
//!
//! Every storage step either succeeds or refuses: a pin that cannot be
//! written, read, or that is empty or malformed stops the preparation. The
//! pin is created atomically (a complete file, linked into place only if no
//! pin exists), so concurrent preparations agree on one value.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Host-owned branch pins, one file per working tree, beside (never inside)
/// the executions' state.
pub(crate) const BRANCH_PINS_DIR: &str = "branch-pins";

/// Why the branch authority could not be established. Preparation refuses
/// with this message rather than falling back to the tree's `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthorityError(pub String);

/// Whether `branch` can be a branch under `refs/heads/` and nothing else.
pub(crate) fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.ends_with(".lock")
        && !branch.contains("..")
        && !branch
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
        && !branch
            .split('/')
            .any(|part| part.is_empty() || part == "." || part.starts_with('.'))
}

fn validated(branch: &str, source: &str) -> Result<String, AuthorityError> {
    let branch = branch.trim();
    if valid_branch(branch) {
        Ok(branch.to_owned())
    } else {
        Err(AuthorityError(format!(
            "the {source} names no usable branch, so no branch could be granted"
        )))
    }
}

/// The branch the host recorded cutting `tree` on, from the desktop's own
/// workdir store. `Ok(None)` when this provider has no host store, or the
/// store records no tree at that path; an unreadable store refuses.
pub(crate) fn host_recorded_branch(
    state_dir: &Path,
    tree: &Path,
) -> Result<Option<String>, AuthorityError> {
    let Some(store) = crate::assignment_inputs::host_store_from_pointer(state_dir) else {
        return Ok(None);
    };
    let document = store.load().map_err(|error| {
        AuthorityError(format!(
            "the host's worktree record could not be read ({error})"
        ))
    })?;
    let Some(worktrees) = document
        .rest
        .get("worktrees")
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(None);
    };
    let tree = canonical_or_self(tree);
    for entry in worktrees.values() {
        let Some(path) = entry.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if canonical_or_self(Path::new(path)) != tree {
            continue;
        }
        let branch = entry
            .get("branch")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        return validated(branch, "host's worktree record").map(Some);
    }
    Ok(None)
}

fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn pin_path(state_dir: &Path, tree: &Path) -> PathBuf {
    let key = hex::encode(Sha256::digest(tree.to_string_lossy().as_bytes()));
    state_dir.join(BRANCH_PINS_DIR).join(&key[..32])
}

/// The pinned branch for `tree`, if one was written. A pin that exists but
/// cannot be read, or holds no usable branch, refuses.
fn read_pin(pin: &Path) -> Result<Option<String>, AuthorityError> {
    match std::fs::read_to_string(pin) {
        Ok(text) => validated(&text, "branch pin").map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AuthorityError(format!(
            "the branch pin could not be read ({error})"
        ))),
    }
}

/// Write `branch` as the pin for `tree`, atomically: a complete temporary
/// file linked into place only if no pin exists yet. When another
/// preparation won, its pin is returned instead.
fn write_pin(pin: &Path, branch: &str) -> Result<String, AuthorityError> {
    let failed = |what: &str, error: std::io::Error| {
        AuthorityError(format!("the branch pin could not be {what} ({error})"))
    };
    let dir = pin
        .parent()
        .ok_or_else(|| AuthorityError("the branch pin has no directory".to_owned()))?;
    std::fs::create_dir_all(dir).map_err(|error| failed("stored", error))?;
    static STAGING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = dir.join(format!(
        ".pin-{}-{}",
        std::process::id(),
        STAGING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(branch.as_bytes())?;
            file.sync_all()
        });
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed("written", error));
    }
    let linked = std::fs::hard_link(&temporary, pin);
    let _ = std::fs::remove_file(&temporary);
    match linked {
        Ok(()) => Ok(branch.to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_pin(pin)?
            .ok_or_else(|| AuthorityError("the branch pin vanished while it was read".to_owned())),
        Err(error) => Err(failed("stored", error)),
    }
}

/// The branch `tree` may move, by the order in the module documentation.
/// `Ok(None)` only when nothing records a branch and the tree is on none.
pub(crate) fn branch_authority(
    state_dir: &Path,
    tree: &Path,
    bound: Option<&str>,
) -> Result<Option<String>, AuthorityError> {
    if let Some(recorded) = host_recorded_branch(state_dir, tree)? {
        return Ok(Some(recorded));
    }
    if let Some(bound) = bound {
        return validated(bound, "session's recorded binding").map(Some);
    }
    let pin = pin_path(state_dir, tree);
    if let Some(pinned) = read_pin(&pin)? {
        return Ok(Some(pinned));
    }
    let Some(head) = crate::execution_scope_git::worktree_branch(tree) else {
        return Ok(None);
    };
    write_pin(&pin, &validated(&head, "working tree's HEAD")?).map(Some)
}

#[cfg(test)]
#[path = "branch_authority_tests.rs"]
mod tests;
