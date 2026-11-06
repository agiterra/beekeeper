//! Windows rewind refuses before touching files: Unix inode/path guarantees
//! have not yet been ported. Foreground Host execution does not require rewind.
use super::{CaptureFailure, UnavailableCode};
use crate::execution_scope_host::HostLaunchPlan;
use std::path::Path;

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

fn unsupported() -> CaptureFailure {
    CaptureFailure::new(
        UnavailableCode::BoundaryUnprepared,
        "Working-tree rewind is unsupported on Windows; no files were restored.",
    )
}
pub(crate) async fn objects_present(
    _cwd: &Path,
    _scope: Option<&HostLaunchPlan>,
    _base: &str,
    _commit: &str,
) -> Result<bool, CaptureFailure> {
    Err(unsupported())
}
pub(crate) async fn restore_obstruction(
    _cwd: &Path,
    _scope: Option<&HostLaunchPlan>,
    _base: &str,
    _omitted: &[String],
) -> Result<Option<String>, CaptureFailure> {
    Ok(Some(unsupported().sentence))
}
pub(crate) async fn restore_tree(
    request: RestoreRequest<'_>,
) -> Result<RestoreReport, RestoreError> {
    let _ = (
        request.cwd,
        request.scope,
        request.base_tree,
        request.pre_tree,
        request.omitted,
    );
    Err(RestoreError::Refused(
        "Working-tree rewind is unsupported on Windows; no files were restored.".into(),
    ))
}
