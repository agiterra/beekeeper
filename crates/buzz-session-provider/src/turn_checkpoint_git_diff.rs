//! Tree-to-tree file lists for [`super`]: `diff-tree -r -z --numstat -M`
//! and `--name-status`, merged into added / modified / deleted / renamed
//! (with the old path), the shape T3 Code's `checkpointing/Diffs.ts` parses —
//! except that a binary file's counts are `None` here, not `0`: a binary has
//! no line count, and zero would read as "unchanged".
//!
//! Paths stay git's raw bytes through parsing and merging, as in
//! `turn_checkpoint_git_omit.rs`: two Latin-1 names decoded lossily would
//! both read `caf\u{FFFD}.txt`, collide, and attach one file's counts to the
//! other. A path that is not UTF-8 is counted in [`DiffResult::not_listed`]
//! rather than named.

use std::path::Path;

use super::run::{Git, StepError};
use super::{is_oid, missing_cwd, CaptureFailure, UnavailableCode, DIFF_TIMEOUT};
use crate::execution_scope_host::HostLaunchPlan;

/// How a file differs between two trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileChange {
    /// Present only in the newer tree.
    Added,
    /// Present in both with different content or type.
    Modified,
    /// Present only in the older tree.
    Deleted,
    /// Moved (`from` names the old path), possibly also edited.
    Renamed,
}

impl FileChange {
    /// The wire spelling.
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
        }
    }
}

/// One file in a tree-to-tree diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangedFile {
    /// Repository-relative path in the newer tree (the older one for a
    /// deletion).
    pub(crate) path: String,
    /// How it changed.
    pub(crate) status: FileChange,
    /// The old path of a rename.
    pub(crate) from: Option<String>,
    /// Lines added; `None` for a binary file, which has no line count.
    pub(crate) additions: Option<u64>,
    /// Lines deleted; `None` for a binary file.
    pub(crate) deletions: Option<u64>,
}

/// List the files that differ from `base_tree` to `tree`, with rename
/// detection and line counts (binary files have none). A file whose path is
/// not UTF-8 is counted in [`DiffResult::not_listed`] rather than named.
///
/// # Errors
/// A [`CaptureFailure`] when the trees cannot be read.
pub(crate) async fn diff_tree_files(
    cwd: &Path,
    scope: Option<&HostLaunchPlan>,
    base_tree: &str,
    tree: &str,
) -> Result<DiffResult, CaptureFailure> {
    if !is_oid(base_tree) || !is_oid(tree) {
        return Err(CaptureFailure::new(
            UnavailableCode::GitFailed,
            "A tree to compare is not an object id, so no diff was computed.",
        ));
    }
    if !cwd.is_dir() {
        return Err(missing_cwd());
    }
    let Some(plan) = scope else {
        return Err(CaptureFailure::new(
            UnavailableCode::BoundaryUnprepared,
            "The working tree's Git boundary could not be prepared, so the trees were not \
             compared.",
        ));
    };
    let work = async {
        let git = Git::new(cwd, plan, None);
        let common = [
            "diff-tree",
            "-r",
            "-z",
            "-M",
            "--no-ext-diff",
            "--no-textconv",
        ];
        let numstat = git
            .run(
                "diff-tree --numstat",
                &[&common[..], &["--numstat", base_tree, tree]].concat(),
                None,
            )
            .await
            .map_err(StepError::into_failure)?;
        let names = git
            .run(
                "diff-tree --name-status",
                &[&common[..], &["--name-status", base_tree, tree]].concat(),
                None,
            )
            .await
            .map_err(StepError::into_failure)?;
        Ok(merge_diff(
            &parse_name_status(&names),
            &parse_numstat(&numstat),
        ))
    };
    match tokio::time::timeout(DIFF_TIMEOUT, work).await {
        Ok(result) => result,
        Err(_elapsed) => Err(CaptureFailure::new(
            UnavailableCode::TimedOut,
            format!(
                "Comparing the two trees took longer than {} ms, so it was abandoned.",
                DIFF_TIMEOUT.as_millis()
            ),
        )),
    }
}

/// A path as git listed it: raw bytes, which on Linux need not be UTF-8.
type RawPath = Vec<u8>;

/// `diff-tree -z --numstat -M`: path (destination of a rename) → counts,
/// `None` for a binary file's `-`. Paths stay git's bytes so a name that is
/// not UTF-8 never collides with another under lossy decoding.
pub(super) fn parse_numstat(raw: &[u8]) -> Vec<(RawPath, Option<u64>, Option<u64>)> {
    let records: Vec<&[u8]> = raw.split(|byte| *byte == 0).collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        let mut parts = record.splitn(3, |byte| *byte == b'\t');
        let (Some(added), Some(deleted), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let count = |bytes: &[u8]| {
            std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| text.parse::<u64>().ok())
        };
        let path = if path.is_empty() {
            // A rename: the next two records are the source and destination.
            let destination = records.get(index + 1).map(|bytes| bytes.to_vec());
            index += 2;
            destination.unwrap_or_default()
        } else {
            path.to_vec()
        };
        if !path.is_empty() {
            out.push((path, count(added), count(deleted)));
        }
    }
    out
}

/// `diff-tree -z --name-status -M`: `(status, path, from)`, paths as git's
/// raw bytes.
pub(super) fn parse_name_status(raw: &[u8]) -> Vec<(FileChange, RawPath, Option<RawPath>)> {
    let records: Vec<&[u8]> = raw.split(|byte| *byte == 0).collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let status = records[index];
        index += 1;
        let Some(&letter) = status.first() else {
            continue;
        };
        let take = |at: usize| {
            records
                .get(at)
                .map(|bytes| bytes.to_vec())
                .unwrap_or_default()
        };
        match letter {
            b'R' | b'C' => {
                let from = take(index);
                let to = take(index + 1);
                index += 2;
                // A copy keeps its source, so the destination is an addition.
                let (change, from) = if letter == b'R' {
                    (FileChange::Renamed, Some(from))
                } else {
                    (FileChange::Added, None)
                };
                out.push((change, to, from));
            }
            _ => {
                let path = take(index);
                index += 1;
                let change = match letter {
                    b'A' => FileChange::Added,
                    b'D' => FileChange::Deleted,
                    _ => FileChange::Modified,
                };
                out.push((change, path, None));
            }
        }
    }
    out.retain(|(_, path, _)| !path.is_empty());
    out
}

/// A tree-to-tree diff as reported: the files that can be named, and a count
/// of the rest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DiffResult {
    /// Files whose path (and, for a rename, old path) is UTF-8, sorted by
    /// path.
    pub(crate) files: Vec<ChangedFile>,
    /// Files that changed but whose path — or a rename's old path — is not
    /// UTF-8. They are counted rather than named, never published under a
    /// lossily decoded name that does not exist (the wire's
    /// `filesNotListed`).
    pub(crate) not_listed: u64,
}

/// Join names and counts on git's raw path bytes, then name only the files
/// whose paths are UTF-8 and count the rest.
pub(super) fn merge_diff(
    names: &[(FileChange, RawPath, Option<RawPath>)],
    counts: &[(RawPath, Option<u64>, Option<u64>)],
) -> DiffResult {
    let counts: std::collections::HashMap<&[u8], (Option<u64>, Option<u64>)> = counts
        .iter()
        .map(|(path, added, deleted)| (path.as_slice(), (*added, *deleted)))
        .collect();
    let mut result = DiffResult::default();
    for (status, path, from) in names {
        let (additions, deletions) = counts.get(path.as_slice()).copied().unwrap_or((None, None));
        let named = String::from_utf8(path.clone()).ok();
        let from = match from {
            Some(bytes) => match String::from_utf8(bytes.clone()) {
                Ok(text) => Some(Some(text)),
                Err(_) => None,
            },
            None => Some(None),
        };
        match (named, from) {
            (Some(path), Some(from)) => result.files.push(ChangedFile {
                path,
                status: *status,
                from,
                additions,
                deletions,
            }),
            _ => result.not_listed = result.not_listed.saturating_add(1),
        }
    }
    result
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    result
}
