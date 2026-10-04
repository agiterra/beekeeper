//! What a capture leaves out, and how it is excluded (see [`super`] § What it
//! will not do).
//!
//! A path is git's bytes until the very end. The exclusion pathspec `add -A`
//! receives is those exact bytes: a name that is not UTF-8 (a Latin-1 byte on
//! Linux, say) converted to a `String` first would carry U+FFFD, match no real
//! file, and let the file it was meant to keep out into the checkpoint while
//! the record claimed it was omitted. Only when the list is reported is a path
//! turned into text, and one that is not UTF-8 is counted rather than named —
//! the wire's `omittedNotListed`, never a name that does not exist.

use std::path::{Path, PathBuf};

use super::run::{Git, StepError};
use super::{CaptureFailure, OmitReason, OmittedPath, DURABLE_CONFIG, MAX_UNTRACKED_FILE_BYTES};

/// One path the capture keeps out, as git's raw bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Exclusion {
    /// The path relative to `cwd`, exactly as git listed it.
    pub(super) raw: Vec<u8>,
    /// Why.
    pub(super) reason: OmitReason,
}

/// The omissions as reported: the nameable ones, and a count of the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReportedOmissions {
    /// Repository-relative UTF-8 paths, sorted, each once.
    pub(super) named: Vec<OmittedPath>,
    /// Paths left out whose names are not UTF-8, so cannot be named.
    pub(super) not_listed: u64,
}

/// Prefix each exclusion with `cwd`'s place in the repository, sort by path,
/// keep one entry per path, and split off the names that are not UTF-8.
///
/// A path excluded up front and again by the recovery pass could carry two
/// reasons, and the wire refuses a path named twice; the first reason recorded
/// (the up-front scan's) is kept — the sort is stable. Sorting raw bytes is
/// the same order as sorting the UTF-8 strings they spell.
pub(super) fn normalize_omitted(mut excluded: Vec<Exclusion>, prefix: &str) -> ReportedOmissions {
    for entry in &mut excluded {
        let mut full = prefix.as_bytes().to_vec();
        full.extend_from_slice(&entry.raw);
        entry.raw = full;
    }
    excluded.sort_by(|left, right| left.raw.cmp(&right.raw));
    excluded.dedup_by(|later, earlier| later.raw == earlier.raw);
    let mut named = Vec::new();
    let mut not_listed = 0_u64;
    for entry in excluded {
        match String::from_utf8(entry.raw) {
            Ok(path) => named.push(OmittedPath {
                path,
                reason: entry.reason,
            }),
            Err(_) => not_listed = not_listed.saturating_add(1),
        }
    }
    ReportedOmissions { named, not_listed }
}

/// Untracked, non-ignored files under `cwd` that the capture must leave out,
/// with paths relative to `cwd` (the caller adds the prefix).
pub(super) async fn scan_untracked(
    git: &Git<'_>,
    cwd: &Path,
) -> Result<Vec<Exclusion>, CaptureFailure> {
    let listing = git
        .run(
            "ls-files --others",
            &["ls-files", "-z", "--others", "--exclude-standard"],
            None,
        )
        .await
        .map_err(StepError::into_failure)?;
    let mut excluded = Vec::new();
    for record in listing.split(|byte| *byte == 0) {
        if record.is_empty() || record.ends_with(b"/") {
            continue;
        }
        let path = cwd.join(bytes_to_path(record));
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let reason = if meta.len() > MAX_UNTRACKED_FILE_BYTES {
            OmitReason::TooLarge
        } else if permission_denied(&path) {
            OmitReason::Unreadable
        } else {
            continue;
        };
        excluded.push(Exclusion {
            raw: record.to_vec(),
            reason,
        });
    }
    Ok(excluded)
}

/// What to leave out after `add -A` failed: nested repositories, and tracked
/// files the host cannot open.
pub(super) async fn recovery_exclusions(git: &Git<'_>, cwd: &Path) -> Vec<Exclusion> {
    let mut extra = Vec::new();
    if let Ok(listing) = git
        .run(
            "ls-files --others",
            &["ls-files", "-z", "--others", "--exclude-standard"],
            None,
        )
        .await
    {
        for record in listing.split(|byte| *byte == 0) {
            if record.ends_with(b"/") && record.len() > 1 {
                extra.push(Exclusion {
                    raw: record[..record.len() - 1].to_vec(),
                    reason: OmitReason::Unreadable,
                });
            }
        }
    }
    if let Ok(listing) = git
        .run("ls-files --cached", &["ls-files", "-z", "--cached"], None)
        .await
    {
        for record in listing.split(|byte| *byte == 0) {
            if record.is_empty() {
                continue;
            }
            let path = cwd.join(bytes_to_path(record));
            let is_file = std::fs::symlink_metadata(&path)
                .map(|meta| meta.is_file())
                .unwrap_or(false);
            if is_file && permission_denied(&path) {
                extra.push(Exclusion {
                    raw: record.to_vec(),
                    reason: OmitReason::Unreadable,
                });
            }
        }
    }
    extra
}

fn permission_denied(path: &Path) -> bool {
    matches!(
        std::fs::File::open(path),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
    )
}

/// The NUL-separated pathspecs `add -A` reads: `.`, then each exclusion's
/// exact bytes under `:(exclude,literal)`.
pub(super) fn exclusion_pathspecs(excluded: &[Exclusion]) -> Vec<u8> {
    let mut pathspecs = b".\0".to_vec();
    for entry in excluded {
        pathspecs.extend_from_slice(b":(exclude,literal)");
        pathspecs.extend_from_slice(&entry.raw);
        pathspecs.push(0);
    }
    pathspecs
}

/// `add -A` of `cwd` into the scratch index, each exclusion excluded by
/// literal pathspec. Pathspecs travel on stdin, so no list is too long for
/// an argument vector.
pub(super) async fn stage(git: &Git<'_>, excluded: &[Exclusion]) -> Result<(), StepError> {
    let args = [
        &DURABLE_CONFIG[..],
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
    ]
    .concat();
    git.run("add -A", &args, Some(exclusion_pathspecs(excluded)))
        .await
        .map(|_| ())
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}
