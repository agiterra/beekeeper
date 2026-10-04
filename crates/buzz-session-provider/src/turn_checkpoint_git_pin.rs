//! Pinning a checkpoint commit under its ref — create-only.
//!
//! A checkpoint ref, once written, is never moved. Its commit may already be
//! named by a published 44231 (`git.commit`/`tree`); moving the ref would
//! leave that commit unreferenced for `git gc` to collect, and every later
//! diff or rewind against the published checkpoint would fail on a missing
//! object while the relay still advertised it. A provider restart, or a
//! retried turn end, re-runs a capture for a leaf that is already pinned, so
//! this is the ordinary path, not a corner: the capture checks first (as T3
//! Code's `CheckpointService` does with `hasCheckpointRef`) and writes the ref
//! with an all-zero old value, so even a write racing the check cannot
//! replace it. Either way the result is [`CaptureFailure::already_pinned`],
//! carrying the commit and tree that stay pinned.

use std::path::Path;

use tokio::process::Command;

use super::run::{finish, StepError};
use super::{is_oid, CaptureFailure, UnavailableCode, BASE_CONFIG, DURABLE_CONFIG};

/// The checkpoint already pinned at a ref, kept as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PinnedCheckpoint {
    /// The commit the ref names.
    pub(crate) commit: String,
    /// That commit's tree.
    pub(crate) tree: String,
}

/// The failure a capture returns for a leaf that is already pinned.
pub(super) fn already_pinned(existing: PinnedCheckpoint) -> CaptureFailure {
    let mut failure = CaptureFailure::new(
        UnavailableCode::GitFailed,
        "A checkpoint is already pinned at this point in the session; it was kept unchanged, \
         and this capture was not recorded.",
    );
    failure.already_pinned = Some(existing);
    failure
}

/// The checkpoint pinned at `ref_name`, or `None` when there is none.
///
/// Run as the host, like the write. A ref that exists but does not name a
/// commit is a refusal, not an absence: it must not be overwritten either.
pub(super) async fn read_pinned(
    cwd: &Path,
    ref_name: &str,
) -> Result<Option<PinnedCheckpoint>, CaptureFailure> {
    let mut command = Command::from(crate::host_command::metadata_git_command(cwd));
    command.args(BASE_CONFIG).args([
        "for-each-ref",
        "--format=%(refname)%00%(objectname)%00%(tree)",
        ref_name,
    ]);
    let out = finish(command, "for-each-ref", None)
        .await
        .map_err(StepError::into_failure)?;
    for line in out.split(|byte| *byte == b'\n') {
        let mut fields = line.split(|byte| *byte == 0);
        if fields.next() != Some(ref_name.as_bytes()) {
            continue;
        }
        let commit =
            String::from_utf8_lossy(fields.next().unwrap_or_default()).to_ascii_lowercase();
        let tree = String::from_utf8_lossy(fields.next().unwrap_or_default()).to_ascii_lowercase();
        if is_oid(&commit) && is_oid(&tree) {
            return Ok(Some(PinnedCheckpoint { commit, tree }));
        }
        return Err(CaptureFailure::new(
            UnavailableCode::GitFailed,
            "The checkpoint ref already exists but does not name a commit, so it was left alone \
             and nothing was captured.",
        ));
    }
    Ok(None)
}

/// Pin `commit` at `ref_name` only if the ref does not exist, as the host
/// (see [`super`]: the tree's boundary does not grant shared refs, and must
/// not).
pub(super) async fn pin(cwd: &Path, ref_name: &str, commit: &str) -> Result<(), CaptureFailure> {
    // The all-zero old value of the repository's object width: "create only".
    let absent = "0".repeat(commit.len());
    let mut command = Command::from(crate::host_command::metadata_git_command(cwd));
    command.args(BASE_CONFIG).args(DURABLE_CONFIG).args([
        "update-ref",
        "-m",
        "beekeeper checkpoint",
        ref_name,
        commit,
        &absent,
    ]);
    let Err(error) = finish(command, "update-ref", None).await else {
        return Ok(());
    };
    match read_pinned(cwd, ref_name).await {
        Ok(Some(existing)) if existing.commit.eq_ignore_ascii_case(commit) => Ok(()),
        Ok(Some(existing)) => Err(already_pinned(existing)),
        _ => Err(error.into_failure()),
    }
}
