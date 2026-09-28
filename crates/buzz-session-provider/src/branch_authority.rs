//! The one authority for which branch a working tree may move (ledger 275
//! A1/A2, 277).
//!
//! A child can re-point its own worktree's `HEAD` (its administration is
//! writable), so `HEAD` is never evidence of which branch a seat owns once a
//! child has run. The invariant: **once a tree has established branch
//! authority, no later preparation treats it as first-seen** — not because a
//! source disappeared, and not because one failed.
//!
//! The sources, in order:
//!
//! 1. the host's own worktree record (`worktrees` in the desktop's workdir
//!    store, reached through the strict
//!    [`crate::assignment_inputs::declared_host_store`]) — the branch the host
//!    cut the tree on. It is an explicit host allocation, so it replaces a
//!    differing pin;
//! 2. a host-owned provenance pin, `branch-pins/<sha256(tree)>` beside the
//!    host's workdir store (one per computer, shared by every provider
//!    identity and the desktop's own host commands), or in a standalone
//!    provider's state: the branch (or "none") the tree was established with
//!    and where that came from;
//! 3. the branch the session's host-owned record already bound, which must
//!    agree with the pin when both exist;
//! 4. only for a tree nothing above names, its `HEAD` at this first
//!    preparation — before any child has run there.
//!
//! Whatever decides, the pin is written before the preparation returns, so a
//! child never runs in a tree whose authority lives only in a source that
//! could later vanish. Every step either succeeds or refuses: a pointer or
//! store that exists but cannot be used, a pin that cannot be written or
//! read, or a binding that disagrees with the pin stops the preparation.
//! Standalone is a provider that has never had a host pointer; once one was
//! declared, the provider keeps a durable note of it, and a pointer that later
//! goes missing (or names another store) refuses. An absent store file behind
//! a valid pointer records nothing (the desktop reads it the same way) and
//! cannot erase an established pin.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Host-owned branch pins, one file per working tree: beside the host's
/// workdir store when a host is declared, else in the provider's state —
/// never inside any execution's state.
pub(crate) const BRANCH_PINS_DIR: &str = "branch-pins";

/// Schema version of [`Pin`].
const PIN_VERSION: u32 = 1;

/// Why the branch authority could not be established. Preparation refuses
/// with this message rather than falling back to the tree's `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthorityError(pub String);

/// Where an established branch authority came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PinSource {
    /// The host's worktree record.
    HostRecord,
    /// A host caller that holds the allocation (assignment establishment).
    HostAllocation,
    /// The session's host-owned execution binding.
    SessionBinding,
    /// The tree's `HEAD` at its first preparation, before any child ran.
    FirstHead,
}

/// The durable provenance of one tree's branch authority. `branch: None`
/// records that the tree was established on no branch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pin {
    version: u32,
    pub(crate) branch: Option<String>,
    pub(crate) source: PinSource,
}

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

/// What the host declares about `tree`: where this computer's pins live,
/// and the branch the host recorded cutting the tree on, if it did.
///
/// With a declared host, pins live beside the host's workdir store, so every
/// provider identity and the desktop's own host commands on this computer
/// share one record per tree; a standalone provider keeps them in its state.
/// The recorded branch is `None` when there is no host pointer, the store
/// file does not exist, or it records no tree at that path. A pointer or
/// store that exists and cannot be used refuses.
struct HostAuthority {
    pins_root: PathBuf,
    recorded: Option<String>,
}

fn host_authority(state_dir: &Path, tree: &Path) -> Result<HostAuthority, AuthorityError> {
    let store = crate::assignment_inputs::declared_host_store(state_dir).map_err(|error| {
        AuthorityError(format!(
            "the host's worktree record is declared but unusable: {error}"
        ))
    })?;
    let declared = read_declared_host(state_dir)?;
    let Some(store) = store else {
        if let Some(previous) = declared {
            return Err(AuthorityError(format!(
                "this provider's host declared its worktree record at {} and the pointer is \
                 now missing; its branch authority cannot be established without it",
                previous.display()
            )));
        }
        return Ok(HostAuthority {
            pins_root: state_dir.to_path_buf(),
            recorded: None,
        });
    };
    match declared {
        Some(previous) if previous != store.path() => {
            return Err(AuthorityError(format!(
                "the host's worktree record moved from {} to {}; branch authority established \
                 against the first cannot be read from the second",
                previous.display(),
                store.path().display()
            )))
        }
        Some(_) => {}
        None => declare_host(state_dir, store.path())?,
    }
    let pins_root = store
        .path()
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AuthorityError("the host's worktree record has no directory".to_owned()))?;
    let recorded = recorded_branch(&store, tree)?;
    Ok(HostAuthority {
        pins_root,
        recorded,
    })
}

/// The provider's own durable note that a host declared its worktree record
/// here, and where: once written, an absent pointer is missing authority,
/// never the standalone configuration.
const HOST_DECLARED_FILE: &str = "host-authority-declared.json";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredHost {
    version: u32,
    store: PathBuf,
}

fn read_declared_host(state_dir: &Path) -> Result<Option<PathBuf>, AuthorityError> {
    let path = state_dir.join(HOST_DECLARED_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AuthorityError(format!(
                "the record of this provider's host could not be read ({error})"
            )))
        }
    };
    serde_json::from_str::<DeclaredHost>(&text)
        .ok()
        .filter(|declared| declared.version == PIN_VERSION && declared.store.is_absolute())
        .map(|declared| Some(declared.store))
        .ok_or_else(|| AuthorityError("the record of this provider's host is corrupt".to_owned()))
}

fn declare_host(state_dir: &Path, store: &Path) -> Result<(), AuthorityError> {
    let bytes = serde_json::to_vec(&DeclaredHost {
        version: PIN_VERSION,
        store: store.to_path_buf(),
    })
    .map_err(|error| AuthorityError(format!("the host record could not be encoded ({error})")))?;
    let path = state_dir.join(HOST_DECLARED_FILE);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| {
            file.write_all(&bytes)?;
            file.sync_all()
        });
    match written {
        Ok(()) => Ok(()),
        // Another preparation declared it first; it must name the same store.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            match read_declared_host(state_dir)? {
                Some(previous) if previous == store => Ok(()),
                _ => Err(AuthorityError(
                    "the record of this provider's host disagrees with its pointer".to_owned(),
                )),
            }
        }
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            Err(AuthorityError(format!(
                "the record of this provider's host could not be stored ({error})"
            )))
        }
    }
}

fn recorded_branch(
    store: &crate::assignment_inputs::AssignmentInputStore,
    tree: &Path,
) -> Result<Option<String>, AuthorityError> {
    if let Err(error) = std::fs::symlink_metadata(store.path()) {
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
    }
    let document = store.load().map_err(|error| {
        AuthorityError(format!(
            "the host's worktree record could not be read ({error})"
        ))
    })?;
    let Some(worktrees) = document.rest.get("worktrees") else {
        return Ok(None);
    };
    let worktrees = worktrees.as_object().ok_or_else(|| {
        AuthorityError("the host's worktree record is malformed (`worktrees`)".to_owned())
    })?;
    let tree = canonical_or_self(tree);
    for entry in worktrees.values() {
        let Some(path) = entry.get("path").and_then(serde_json::Value::as_str) else {
            return Err(AuthorityError(
                "the host's worktree record holds an entry with no path".to_owned(),
            ));
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

/// The tree a pin is keyed by: the canonical top of the working tree `path`
/// is in, so every caller — whatever spelling or subdirectory it holds —
/// reaches the same pin. `None` when `path` is in no Git working tree.
fn tree_key(path: &Path) -> Option<PathBuf> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(crate::git_probe::HOST_GIT_NO_PROJECT_CODE)
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path);
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        cmd.env_remove(var);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(canonical_or_self(Path::new(text.trim())))
}

fn pin_path(pins_root: &Path, tree: &Path) -> PathBuf {
    let key = hex::encode(Sha256::digest(tree.to_string_lossy().as_bytes()));
    pins_root.join(BRANCH_PINS_DIR).join(&key[..32])
}

/// The pin for `tree`, if one was written. A pin that exists but cannot be
/// read, does not parse, or names no usable branch refuses.
fn read_pin(pin: &Path) -> Result<Option<Pin>, AuthorityError> {
    let text = match std::fs::read_to_string(pin) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AuthorityError(format!(
                "the branch pin could not be read ({error})"
            )))
        }
    };
    let parsed: Pin = serde_json::from_str(&text)
        .ok()
        .filter(|parsed: &Pin| parsed.version == PIN_VERSION)
        .ok_or_else(|| AuthorityError("the branch pin is corrupt".to_owned()))?;
    if let Some(branch) = &parsed.branch {
        if !valid_branch(branch) {
            return Err(AuthorityError(
                "the branch pin names no usable branch".to_owned(),
            ));
        }
    }
    Ok(Some(parsed))
}

/// Write `pin` for a tree, atomically. `replace: false` links a complete
/// file into place only if no pin exists yet, and returns the winner's pin
/// when another preparation got there first; `replace: true` (an explicit
/// host allocation) renames over whatever is there.
fn write_pin(path: &Path, pin: &Pin, replace: bool) -> Result<Pin, AuthorityError> {
    let failed = |what: &str, error: std::io::Error| {
        AuthorityError(format!("the branch pin could not be {what} ({error})"))
    };
    let dir = path
        .parent()
        .ok_or_else(|| AuthorityError("the branch pin has no directory".to_owned()))?;
    std::fs::create_dir_all(dir).map_err(|error| failed("stored", error))?;
    let bytes = serde_json::to_vec(pin).map_err(|error| {
        AuthorityError(format!("the branch pin could not be encoded ({error})"))
    })?;
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
            file.write_all(&bytes)?;
            file.sync_all()
        });
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed("written", error));
    }
    if replace {
        let renamed = std::fs::rename(&temporary, path);
        if let Err(error) = renamed {
            let _ = std::fs::remove_file(&temporary);
            return Err(failed("stored", error));
        }
        return Ok(pin.clone());
    }
    let linked = std::fs::hard_link(&temporary, path);
    let _ = std::fs::remove_file(&temporary);
    match linked {
        Ok(()) => Ok(pin.clone()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_pin(path)?
            .ok_or_else(|| AuthorityError("the branch pin vanished while it was read".to_owned())),
        Err(error) => Err(failed("stored", error)),
    }
}

fn pin(branch: Option<String>, source: PinSource) -> Pin {
    Pin {
        version: PIN_VERSION,
        branch,
        source,
    }
}

/// Record an explicit host allocation of `branch` for `tree` — the caller
/// holds the host's authority (assignment establishment from the host's
/// worktree record or from [`branch_authority`]) — before any child runs
/// under it. A differing pin is replaced: the host decided.
pub(crate) fn record_allocation(
    state_dir: &Path,
    tree: &Path,
    branch: &str,
) -> Result<(), AuthorityError> {
    let branch = validated(branch, "host's allocation")?;
    let host = host_authority(state_dir, tree)?;
    let Some(key) = tree_key(tree) else {
        return Ok(());
    };
    let path = pin_path(&host.pins_root, &key);
    if read_pin(&path)?
        .and_then(|current| current.branch)
        .as_deref()
        == Some(branch.as_str())
    {
        return Ok(());
    }
    write_pin(&path, &pin(Some(branch), PinSource::HostAllocation), true).map(|_| ())
}

/// The branch `tree` may move, by the order in the module documentation,
/// with its provenance durably pinned before this returns. `Ok(None)` when
/// the tree is in no Git working tree, or was established on no branch.
pub(crate) fn branch_authority(
    state_dir: &Path,
    tree: &Path,
    bound: Option<&str>,
) -> Result<Option<String>, AuthorityError> {
    let HostAuthority {
        pins_root,
        recorded,
    } = host_authority(state_dir, tree)?;
    let Some(key) = tree_key(tree) else {
        // Not a Git working tree: there is no branch to grant, and nothing a
        // child could re-point.
        return Ok(recorded);
    };
    let path = pin_path(&pins_root, &key);
    let existing = read_pin(&path)?;

    if let Some(recorded) = recorded {
        // The host's own allocation decides, and is made durable first.
        if existing.and_then(|current| current.branch).as_deref() != Some(recorded.as_str()) {
            write_pin(
                &path,
                &pin(Some(recorded.clone()), PinSource::HostRecord),
                true,
            )?;
        }
        return Ok(Some(recorded));
    }
    let bound = bound
        .map(|bound| validated(bound, "session's recorded binding"))
        .transpose()?;
    if let Some(existing) = existing {
        // Established before: the pin holds, whatever sources went away.
        if let Some(bound) = &bound {
            if existing.branch.as_deref() != Some(bound.as_str()) {
                return Err(AuthorityError(format!(
                    "the session's recorded binding ({bound}) disagrees with the tree's \
                     established branch ({}); the host must reallocate the tree",
                    existing.branch.as_deref().unwrap_or("no branch")
                )));
            }
        }
        return Ok(existing.branch);
    }
    if let Some(bound) = bound {
        let written = write_pin(
            &path,
            &pin(Some(bound.clone()), PinSource::SessionBinding),
            false,
        )?;
        if written.branch.as_deref() != Some(bound.as_str()) {
            return Err(AuthorityError(
                "a concurrent preparation established a different branch for this tree".to_owned(),
            ));
        }
        return Ok(Some(bound));
    }
    // First seen: nothing records this tree, and no child has run in it.
    let head = crate::execution_scope_git::worktree_branch(&key)
        .map(|head| validated(&head, "working tree's HEAD"))
        .transpose()?;
    Ok(write_pin(&path, &pin(head, PinSource::FirstHead), false)?.branch)
}

#[cfg(test)]
#[path = "branch_authority_tests.rs"]
mod tests;
