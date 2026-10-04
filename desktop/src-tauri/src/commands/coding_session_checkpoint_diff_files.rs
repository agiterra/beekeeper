//! The file list and patches of a checkpoint diff, with every path kept as
//! Git's bytes until it is named (SV-30).
//!
//! `diff_from_repo` reads `git diff --numstat` as text: Git C-quotes any name
//! with a byte outside printable ASCII (`"caf\303\251.txt"`), and a name that
//! is not UTF-8 at all would be decoded lossily into U+FFFD. Either way the
//! listed path is a file that does not exist, the per-file patch run against
//! it matches nothing, and an empty patch reads as "no change". Two names that
//! differ only in their invalid bytes would also collapse into one.
//!
//! So this reads `-z` records as bytes and follows the rule the provider's
//! capture sets (`turn_checkpoint_git_omit.rs`): a path that is not UTF-8 is
//! **counted, never named** — it adds to `filesNotListed` and to the line
//! totals, and gets no row. A rename either side of which is not UTF-8 is
//! counted the same way, since its patch could not be asked for by name. Files
//! past [`MAX_LISTED_FILES`] are counted too, where `diff_from_repo` drops
//! them without saying so.
//!
//! Renames are detected; copies are not. A copy's patch, limited to its two
//! paths, would also carry any edit to the unchanged-name source, so a copy is
//! shown as the addition it is on disk, in its counts and its patch alike.

use std::path::Path;

use buzz_session_provider_pkg::execution_scope_host::HostLaunchPlan;

use super::super::project_git_diff::{
    truncate_patch, ProjectRepoDiffFileInfo, ProjectRepoDiffInfo,
};

/// The most files named in one answer, as `diff_from_repo` names.
pub(super) const MAX_LISTED_FILES: usize = 250;

/// One `git diff -z --numstat` record, paths as Git's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NumstatRecord {
    /// The path after the change (the destination of a rename).
    pub path: Vec<u8>,
    /// The source of a rename.
    pub from: Option<Vec<u8>>,
    /// Lines added; 0 for a binary file's `-`.
    pub additions: usize,
    /// Lines deleted; 0 for a binary file's `-`.
    pub deletions: usize,
}

/// A checkpoint diff: the named files, and how many it could not name.
pub(super) struct CheckpointDiffFiles {
    /// Named files, counts and capped patches. The totals cover every changed
    /// file, named or not.
    pub diff: ProjectRepoDiffInfo,
    /// Changed files with no row: a non-UTF-8 name, or past the list cap.
    pub files_not_listed: u64,
}

/// Parses `git diff -z --numstat` output: `A\tD\tpath\0`, or for a rename
/// `A\tD\t\0from\0to\0`.
pub(super) fn parse_numstat_z(raw: &[u8]) -> Vec<NumstatRecord> {
    let records: Vec<&[u8]> = raw.split(|byte| *byte == 0).collect();
    let count = |field: &[u8]| {
        std::str::from_utf8(field)
            .ok()
            .and_then(|text| text.parse::<usize>().ok())
            .unwrap_or(0)
    };
    let mut out = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let header = records[index];
        index += 1;
        let mut fields = header.splitn(3, |byte| *byte == b'\t');
        let (Some(added), Some(deleted), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (path, from) = if path.is_empty() {
            let from = records.get(index).map(|bytes| bytes.to_vec());
            let to = records.get(index + 1).map(|bytes| bytes.to_vec());
            index += 2;
            match (from, to) {
                (Some(from), Some(to)) if !to.is_empty() => (to, Some(from)),
                _ => continue,
            }
        } else {
            (path.to_vec(), None)
        };
        out.push(NumstatRecord {
            path,
            from,
            additions: count(added),
            deletions: count(deleted),
        });
    }
    out
}

/// The UTF-8 names of a record, or `None` when either side is not UTF-8.
fn nameable(record: &NumstatRecord) -> Option<(String, Option<String>)> {
    let path = String::from_utf8(record.path.clone()).ok()?;
    let from = match &record.from {
        Some(from) => Some(String::from_utf8(from.clone()).ok()?),
        None => None,
    };
    Some((path, from))
}

/// Run `git <args>` in `dir` inside `plan`, answering stdout as bytes.
fn git_bytes(plan: &HostLaunchPlan, dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = plan.git_command(dir, args);
    command.stdin(std::process::Stdio::null());
    crate::util::configure_no_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git exited with status {}", output.status)
        } else {
            stderr
        })
    }
}

/// The diff between two validated tree ids, inside `plan`'s boundary.
///
/// # Errors
/// Git failed listing the change, or producing a named file's patch — a patch
/// that could not be produced is an error naming the file, never an empty
/// patch that reads as no change.
pub(super) fn checkpoint_diff_files(
    plan: &HostLaunchPlan,
    dir: &Path,
    from_tree: &str,
    to_tree: &str,
) -> Result<CheckpointDiffFiles, String> {
    let numstat = git_bytes(
        plan,
        dir,
        &[
            "diff",
            "-z",
            "--numstat",
            "--no-color",
            "--no-relative",
            "--no-ext-diff",
            "--find-renames",
            "--end-of-options",
            from_tree,
            to_tree,
        ],
    )?;
    let records = parse_numstat_z(&numstat);
    let mut files = Vec::new();
    let mut files_not_listed: u64 = 0;
    for record in &records {
        let named = if files.len() < MAX_LISTED_FILES {
            nameable(record)
        } else {
            None
        };
        let Some((path, from)) = named else {
            files_not_listed += 1;
            continue;
        };
        // `:(literal)` so a name holding `*` or `?` matches only itself.
        let mut pathspecs = vec![format!(":(literal){path}")];
        if let Some(from) = &from {
            pathspecs.push(format!(":(literal){from}"));
        }
        let mut args = vec![
            "diff",
            "--no-color",
            "--no-relative",
            "--no-ext-diff",
            "--find-renames",
            "--unified=80",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--end-of-options",
            from_tree,
            to_tree,
            "--",
        ];
        args.extend(pathspecs.iter().map(String::as_str));
        let patch = git_bytes(plan, dir, &args)
            .map_err(|error| format!("could not produce the diff of {path}: {error}"))?;
        let (patch, truncated) = truncate_patch(String::from_utf8_lossy(&patch).into_owned());
        files.push(ProjectRepoDiffFileInfo {
            path,
            additions: record.additions,
            deletions: record.deletions,
            patch,
            truncated,
        });
    }
    Ok(CheckpointDiffFiles {
        diff: ProjectRepoDiffInfo {
            additions: records.iter().map(|record| record.additions).sum(),
            deletions: records.iter().map(|record| record.deletions).sum(),
            commit_body: None,
            files,
        },
        files_not_listed,
    })
}
