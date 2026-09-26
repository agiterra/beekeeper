//! Host Git that writes the working tree of an existing project checkout
//! after a fetch: a pull's fast-forward, and an unborn checkout's first
//! branch. The fetch before it is transport only (hardened host Git). Writing
//! the tree runs the repository's own filters — configuration the operator
//! and the project's sessions can write — so it runs inside the checkout's
//! project boundary ([`crate::coding_sessions::host_git`]), and its result is
//! proved to be the fetched commit.

use super::project_git::first_output_line;
use super::project_git_exec::{run_git, GitAuthConfig};
use crate::coding_sessions::host_git::{materialize, Workspace};

/// The commit `remote`'s `branch` was fetched to (a ref read; runs nothing).
fn fetched_head(
    checkout: &std::path::Path,
    remote: &str,
    branch: &str,
    auth: &GitAuthConfig,
) -> Result<String, String> {
    let remote_ref = format!("refs/remotes/{remote}/{branch}");
    run_git(
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &remote_ref,
        ],
        Some(checkout),
        auth,
    )
    .ok()
    .and_then(|output| first_output_line(&output))
    .ok_or_else(|| format!("No fetched head for {remote}/{branch}."))
}

/// Move `checkout` to the fetched head of `remote`'s `branch`, fast-forward
/// only.
///
/// # Errors
/// No fetched head, a boundary that could not be prepared, or a merge that is
/// not a fast-forward.
pub(crate) fn fast_forward_checkout(
    checkout: &std::path::Path,
    remote: &str,
    branch: &str,
    auth: &GitAuthConfig,
) -> Result<(), String> {
    let target = fetched_head(checkout, remote, branch, auth)?;
    materialize(
        &Workspace {
            tree: checkout,
            repo_root: None,
            name: "pull",
            host_branch: None,
            host_read: &[],
        },
        &["merge", "--ff-only", "--quiet", "--end-of-options", &target],
        &target,
        false,
    )
}

/// Put an existing checkout with an unborn `HEAD` on its own `branch` at the
/// fetched `origin/<branch>`, and prove every tracked file matches it.
///
/// # Errors
/// No fetched head, a boundary that could not be prepared, or a checkout that
/// did not land on that commit.
pub(crate) fn check_out_fetched_branch(
    repo_dir: &std::path::Path,
    branch: &str,
    auth: &GitAuthConfig,
) -> Result<(), String> {
    let commit = fetched_head(repo_dir, "origin", branch, auth)?;
    materialize(
        &Workspace {
            tree: repo_dir,
            repo_root: None,
            name: "bootstrap",
            host_branch: Some(branch),
            host_read: &[],
        },
        &["checkout", "--quiet", "-B", branch, &commit],
        &commit,
        true,
    )
}

// The boundary backend is macOS's; elsewhere the plan is disclosed as
// unenforced.
#[cfg(all(test, target_os = "macos"))]
#[path = "project_git_materialize_tests.rs"]
mod tests;
