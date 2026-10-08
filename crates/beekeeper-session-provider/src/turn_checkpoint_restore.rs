//! SV-29 `files: restore`: return a working tree to a checkpoint's
//! `baseTree`, touching nothing a rewind has no business touching.
//!
//! # The two steps
//!
//! ```text
//! GIT_INDEX_FILE=<empty scratch>  git restore --source=<baseTree> --worktree --overlay \
//!     --pathspec-from-file=- --pathspec-file-nul      (. and :(top,exclude,literal)<omitted>…)
//! git diff-tree -r -z --no-renames --diff-filter=A --name-only --relative <baseTree> <preTree>
//!     → unlink exactly those paths, then their empty parents up to cwd
//! ```
//!
//! - **Overlay.** The default (no-overlay) mode deletes index-tracked paths the
//!   source lacks — a decision made from the person's *index*, not from what
//!   the rewind measured. Overlay only writes; deletion is left to the list,
//!   which is exactly `paths(preRewind) − paths(baseTree)`.
//! - **An empty scratch index.** `restore` is pointed at a scratch index that
//!   does not exist, so the person's real index — staged work, stat cache —
//!   is never read or rewritten. It is removed afterwards if git made one.
//! - **Never** `--staged`, `clean`, `checkout` or `reset`; `HEAD` and branches
//!   are not moved. Ignored paths are in neither tree, so neither step sees
//!   them; paths the pre-rewind capture omitted (too large, unreadable) are
//!   excluded from the restore and are not in the deletion list either.
//! - **Filters on, hooks off.** Every invocation carries the checkpoint
//!   runner's `core.hooksPath=/dev/null` and `core.fsmonitor=false`, and runs
//!   inside the tree's host-Git boundary, because a smudge filter (LFS) is the
//!   project's own code. Filters stay on, as `git checkout` would run them.
//!
//! A deletion never follows a symlink: a path whose parent inside `cwd` is a
//! symlink, or which is itself a directory (a nested repository the turn
//! added), is left in place and counted in [`RestoreReport::left`].
//!
//! # What is checked around the two steps
//!
//! - **Before anything is written**, every path of the base tree is looked up
//!   in the working tree ([`obstruction`]). A forced restore replaces whatever
//!   stands where it writes: a directory where the base has a file goes,
//!   whole, with the ignored files, omitted files and nested repositories in
//!   it that no capture holds. So a base file that is now a directory, or a
//!   base directory that is now a file or a symlink, refuses the restore and
//!   nothing is touched.
//! - **A deletion never takes a restored file.** On a case-insensitive (or
//!   normalization-insensitive) filesystem a turn's rename `Readme.md` →
//!   `README.md` lists `README.md` as added, and unlinking it after
//!   `Readme.md` is written back would delete the restored file. A path whose
//!   name folds to a base path's, or which is the same file (device and inode)
//!   as a restored base file, is never unlinked.
//! - **After both steps**, every base file must exist; one that does not is a
//!   failed restore ([`RestoreReport::missing`]), never a restored one.

use std::collections::HashSet;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use super::run::{Git, StepError};
use super::{is_oid, read_layout, CaptureFailure, UnavailableCode};
use crate::execution_scope_host::HostLaunchPlan;

/// Ceiling on the whole restore (both steps).
pub(crate) const RESTORE_TIMEOUT: Duration = Duration::from_secs(120);

/// Ceiling on checking a checkpoint's objects are present.
pub(crate) const OBJECTS_TIMEOUT: Duration = Duration::from_secs(10);

/// What a restore did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RestoreReport {
    /// Files the base tree lacked that were removed.
    pub(crate) removed: usize,
    /// Paths that should have gone and were left because removing them would
    /// have followed a symlink, removed a directory, or could have taken a
    /// restored file with them.
    pub(crate) left: Vec<String>,
    /// Base-tree files that are not in the working tree after the restore.
    pub(crate) missing: Vec<String>,
}

/// Why a restore did not end with the base tree in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestoreError {
    /// Refused before anything was written: the working tree is untouched.
    /// The sentence names the path.
    Refused(String),
    /// A step failed; the tree may be partly restored.
    Failed(CaptureFailure),
}

impl From<CaptureFailure> for RestoreError {
    fn from(failure: CaptureFailure) -> Self {
        Self::Failed(failure)
    }
}

/// One path of the base tree, relative to `cwd`.
#[derive(Debug, Clone)]
struct BaseEntry {
    /// As git printed it (not necessarily UTF-8).
    raw: PathBuf,
    /// The same, lossily, for sentences and case folding.
    path: String,
    /// A tree (directory); otherwise a blob (a file or a symlink).
    tree: bool,
}

/// What [`restore_tree`] restores from and to.
pub(crate) struct RestoreRequest<'a> {
    /// The session's working directory; nothing outside it is touched.
    pub(crate) cwd: &'a Path,
    /// The tree's prepared host-Git boundary, or `None` (refused).
    pub(crate) scope: Option<&'a HostLaunchPlan>,
    /// The checkpoint's `baseTree`: the target.
    pub(crate) base_tree: &'a str,
    /// The pre-rewind capture's tree: what is there now.
    pub(crate) pre_tree: &'a str,
    /// Repo-relative paths the pre-rewind capture omitted.
    pub(crate) omitted: &'a [String],
}

fn failed(sentence: &str) -> CaptureFailure {
    CaptureFailure::new(UnavailableCode::GitFailed, sentence)
}

fn unbounded() -> CaptureFailure {
    CaptureFailure::new(
        UnavailableCode::BoundaryUnprepared,
        "The working tree's Git boundary could not be prepared, so nothing was restored.",
    )
}

/// Whether `baseTree` (as a tree) and `commit` (as a commit) are both in the
/// repository's object store. `Ok(false)` is "absent"; `Err` is "could not
/// ask".
pub(crate) async fn objects_present(
    cwd: &Path,
    scope: Option<&HostLaunchPlan>,
    base_tree: &str,
    commit: &str,
) -> Result<bool, CaptureFailure> {
    let plan = scope.ok_or_else(unbounded)?;
    if !is_oid(base_tree) || !is_oid(commit) {
        return Ok(false);
    }
    let git = Git::new(cwd, plan, None);
    let check = async {
        for spec in [
            format!("{base_tree}^{{tree}}"),
            format!("{commit}^{{commit}}"),
        ] {
            match git
                .run("cat-file -e", &["cat-file", "-e", &spec], None)
                .await
            {
                Ok(_) => {}
                Err(StepError { exit: Some(_), .. }) => return Ok(false),
                Err(error) => return Err(error.into_failure()),
            }
        }
        Ok(true)
    };
    tokio::time::timeout(OBJECTS_TIMEOUT, check)
        .await
        .unwrap_or_else(|_| {
            Err(CaptureFailure::new(
                UnavailableCode::TimedOut,
                "Checking the checkpoint's objects timed out.",
            ))
        })
}

/// Restore `cwd` to `base_tree` (module docs). Bounded by
/// [`RESTORE_TIMEOUT`]; a timeout or a failed step may leave the tree part
/// restored, which the caller reports as `restore_failed` — the pre-rewind
/// checkpoint holds what was there.
pub(crate) async fn restore_tree(
    request: RestoreRequest<'_>,
) -> Result<RestoreReport, RestoreError> {
    let plan = request.scope.ok_or_else(unbounded)?;
    if !is_oid(request.base_tree) || !is_oid(request.pre_tree) {
        return Err(
            failed("The checkpoint's trees are not object ids, so nothing was restored.").into(),
        );
    }
    match tokio::time::timeout(RESTORE_TIMEOUT, restore(request, plan)).await {
        Ok(result) => result,
        Err(_) => Err(CaptureFailure::new(
            UnavailableCode::TimedOut,
            "Restoring the working tree timed out; it may be partly restored.",
        )
        .into()),
    }
}

/// Whether restoring `base_tree` into `cwd` would replace something no
/// capture holds (module docs): `Ok(Some(sentence))` names the first such
/// path. Reads only; the caller refuses the restore on `Some`.
pub(crate) async fn restore_obstruction(
    cwd: &Path,
    scope: Option<&HostLaunchPlan>,
    base_tree: &str,
    omitted: &[String],
) -> Result<Option<String>, CaptureFailure> {
    let plan = scope.ok_or_else(unbounded)?;
    if !is_oid(base_tree) {
        return Err(failed("The checkpoint's tree is not an object id."));
    }
    let probe = Git::new(cwd, plan, None);
    let layout = read_layout(&probe).await?;
    let entries = base_entries(&probe, base_tree, &layout.prefix, omitted).await?;
    let root = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || obstruction(&root, &entries))
        .await
        .map_err(|_| failed("Checking the working tree before the restore did not finish."))
}

/// The base tree's files and directories under `cwd`, without the paths the
/// restore excludes (omitted ones, and anything under them). Gitlinks are
/// left out: the restore writes nothing for them.
async fn base_entries(
    git: &Git<'_>,
    base_tree: &str,
    prefix: &str,
    omitted: &[String],
) -> Result<Vec<BaseEntry>, CaptureFailure> {
    let listing = git
        .run("ls-tree", &["ls-tree", "-r", "-t", "-z", base_tree], None)
        .await
        .map_err(StepError::into_failure)?;
    let excluded = |path: &str| {
        let full = format!("{prefix}{path}");
        omitted.iter().any(|omit| {
            full == *omit
                || full
                    .strip_prefix(omit.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let mut entries = Vec::new();
    for record in listing.split(|byte| *byte == 0) {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            continue;
        };
        let (meta, raw) = (&record[..tab], &record[tab + 1..]);
        let tree = match meta.split(|byte| *byte == b' ').nth(1) {
            Some(b"tree") => true,
            Some(b"blob") => false,
            _ => continue,
        };
        let raw = PathBuf::from(std::ffi::OsStr::from_bytes(raw));
        let path = raw.to_string_lossy().into_owned();
        if inside(Path::new("/"), &raw).is_none() || excluded(&path) {
            continue;
        }
        entries.push(BaseEntry { raw, path, tree });
    }
    Ok(entries)
}

/// The first base path the working tree has replaced with something the
/// restore would delete: a directory where the base has a file, or a file or
/// symlink where the base has a directory.
fn obstruction(cwd: &Path, entries: &[BaseEntry]) -> Option<String> {
    for entry in entries {
        let Some(full) = inside(cwd, &entry.raw) else {
            continue;
        };
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            continue;
        };
        let kind = meta.file_type();
        let now = if entry.tree && kind.is_symlink() {
            "a symlink"
        } else if entry.tree && !kind.is_dir() {
            "a file"
        } else if !entry.tree && kind.is_dir() {
            "a directory"
        } else {
            continue;
        };
        let was = if entry.tree { "a directory" } else { "a file" };
        return Some(format!(
            "the files cannot be restored: \"{}\" was {was} before the turn and is now {now}; \
             restoring it would delete what is there, which no checkpoint holds, so nothing was \
             touched",
            entry.path
        ));
    }
    None
}

async fn restore(
    request: RestoreRequest<'_>,
    plan: &HostLaunchPlan,
) -> Result<RestoreReport, RestoreError> {
    let cwd = request.cwd;
    let probe = Git::new(cwd, plan, None);
    let layout = read_layout(&probe).await?;
    // The list is taken before anything is written: it is a fact about the
    // two captured trees, not about the working tree.
    let added = probe
        .run(
            "diff-tree",
            &[
                "diff-tree",
                "-r",
                "-z",
                "--no-renames",
                "--diff-filter=A",
                "--name-only",
                "--relative",
                request.base_tree,
                request.pre_tree,
            ],
            None,
        )
        .await
        .map_err(StepError::into_failure)?;
    let added: Vec<String> = added
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect();
    let entries = base_entries(&probe, request.base_tree, &layout.prefix, request.omitted).await?;
    // Re-made here, just before the first write: the tree may have changed
    // since the rewind was prepared.
    let check_root = cwd.to_path_buf();
    let check_entries = entries.clone();
    let blocked = tokio::task::spawn_blocking(move || obstruction(&check_root, &check_entries))
        .await
        .map_err(|_| failed("Checking the working tree before the restore did not finish."))?;
    if let Some(sentence) = blocked {
        return Err(RestoreError::Refused(sentence));
    }

    let scratch = layout
        .git_dir
        .join(format!("beekeeper-rewind-index-{}", uuid::Uuid::new_v4()));
    let _cleanup = ScratchGuard(scratch.clone());
    let git = Git::new(cwd, plan, Some(&scratch));
    let mut pathspec = b".\0".to_vec();
    for path in request.omitted {
        if path.contains('\0') {
            continue;
        }
        pathspec.extend_from_slice(format!(":(top,exclude,literal){path}\0").as_bytes());
    }
    let source = format!("--source={}", request.base_tree);
    git.run(
        "restore",
        &[
            "restore",
            &source,
            "--worktree",
            "--overlay",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ],
        Some(pathspec),
    )
    .await
    .map_err(StepError::into_failure)?;

    let root = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let base = BaseIndex::read(&root, &entries);
        let mut report = remove_added(&root, &added, &base);
        report.missing = missing(&root, &entries);
        report
    })
    .await
    .map_err(|_| failed("Removing the files the turns added did not finish.").into())
}

/// What a deletion must never take: the base tree's names, folded, and the
/// restored base files' identities.
#[derive(Debug, Default)]
struct BaseIndex {
    /// Every base path, lower-cased.
    folded: HashSet<String>,
    /// Device and inode of every base file present after the restore.
    files: HashSet<(u64, u64)>,
}

impl BaseIndex {
    fn read(cwd: &Path, entries: &[BaseEntry]) -> Self {
        let mut index = Self::default();
        for entry in entries {
            index.folded.insert(entry.path.to_lowercase());
            if entry.tree {
                continue;
            }
            if let Some(meta) =
                inside(cwd, &entry.raw).and_then(|full| std::fs::symlink_metadata(full).ok())
            {
                index.files.insert((meta.dev(), meta.ino()));
            }
        }
        index
    }
}

/// Base files absent from the working tree.
fn missing(cwd: &Path, entries: &[BaseEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| !entry.tree)
        .filter(|entry| {
            inside(cwd, &entry.raw).is_some_and(|full| std::fs::symlink_metadata(full).is_err())
        })
        .map(|entry| entry.path.clone())
        .collect()
}

/// Removes the scratch index (and its lock) however the restore ends.
struct ScratchGuard(PathBuf);

impl Drop for ScratchGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let mut lock = self.0.clone().into_os_string();
        lock.push(".lock");
        let _ = std::fs::remove_file(PathBuf::from(lock));
    }
}

/// A path git printed relative to `cwd`, if it stays strictly inside it.
fn inside(cwd: &Path, path: impl AsRef<Path>) -> Option<PathBuf> {
    let relative = path.as_ref();
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(cwd.join(relative))
}

/// Whether any directory between `cwd` (exclusive) and `full` (exclusive) is a
/// symlink or not a directory.
fn crosses_a_link(cwd: &Path, full: &Path) -> bool {
    let mut dir = full.parent();
    while let Some(current) = dir {
        if current == cwd || !current.starts_with(cwd) {
            break;
        }
        match std::fs::symlink_metadata(current) {
            Ok(meta) if meta.file_type().is_dir() => {}
            _ => return true,
        }
        dir = current.parent();
    }
    false
}

fn remove_added(cwd: &Path, added: &[String], base: &BaseIndex) -> RestoreReport {
    let mut report = RestoreReport::default();
    let mut parents = Vec::new();
    for path in added {
        let Some(full) = inside(cwd, path) else {
            report.left.push(path.clone());
            continue;
        };
        if crosses_a_link(cwd, &full) {
            report.left.push(path.clone());
            continue;
        }
        let folded = base.folded.contains(&path.to_lowercase());
        let meta = match std::fs::symlink_metadata(&full) {
            // Already gone: the tree no longer has it, which is the goal.
            Err(_) => continue,
            // The base has a directory here (a turn replaced it with a file,
            // and the restore wrote it back): what the restore meant.
            Ok(meta) if meta.file_type().is_dir() && folded => continue,
            Ok(meta) if meta.file_type().is_dir() => {
                report.left.push(path.clone());
                continue;
            }
            Ok(meta) => meta,
        };
        let restored = base.files.contains(&(meta.dev(), meta.ino()));
        match (folded, restored) {
            // A base file under another spelling of its name: the restored
            // file itself, on a case- or normalization-insensitive tree.
            (true, true) => continue,
            // A name that differs from a base path's only by case, or a
            // second link to a restored file: never unlinked.
            (true, false) | (false, true) => {
                report.left.push(path.clone());
                continue;
            }
            (false, false) => {}
        }
        if std::fs::remove_file(&full).is_ok() {
            report.removed += 1;
            if let Some(parent) = full.parent() {
                parents.push(parent.to_path_buf());
            }
        } else {
            report.left.push(path.clone());
        }
    }
    // Deepest first, so a chain of directories the turns created goes.
    parents.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    parents.dedup();
    for parent in parents {
        let mut dir = Some(parent.as_path());
        while let Some(current) = dir {
            if current == cwd || !current.starts_with(cwd) {
                break;
            }
            // Only an empty directory is removed; anything in it stays.
            if std::fs::remove_dir(current).is_err() {
                break;
            }
            dir = current.parent();
        }
    }
    report
}

#[cfg(test)]
#[path = "turn_checkpoint_restore_tests.rs"]
mod tests;
