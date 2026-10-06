//! Git this desktop runs in a project workspace, inside that workspace's
//! project boundary.
//!
//! A checkout, a status or a clean check can run the repository's own code:
//! its configuration names smudge/clean filters and an fsmonitor program, and
//! a coding session of the project can write that configuration. So those
//! operations run inside the host preparation the provider uses for its own
//! host commands (`beekeeper_session_provider_pkg::execution_scope_host`): the
//! workspace, its Git administration and the project's tools, nothing of
//! another project's. Operations that run none of the repository's code —
//! cutting a worktree with nothing checked out, reading refs — stay ordinary
//! host Git with hooks off (`project_git_exec::run_git`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use beekeeper_session_provider_pkg::execution_scope_host::{
    prepare_host_command, HostCommandScope, HostLaunchPlan,
};

static STATE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Record where this desktop prepares host-Git boundaries. Called once at
/// startup with [`crate::session_provider::host_git_state_dir`].
pub(crate) fn set_state_dir(dir: PathBuf) {
    let _ = STATE_DIR.set(dir);
}

fn state_dir() -> Result<&'static Path, String> {
    // Tests run no app setup: each test process prepares under its own
    // disposable directory.
    #[cfg(test)]
    let _ = STATE_DIR.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("beekeeper-host-git-tests-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.canonicalize().unwrap_or(dir)
    });
    STATE_DIR
        .get()
        .map(PathBuf::as_path)
        .ok_or_else(|| "the host Git state directory was not set up".to_string())
}

/// What one host-Git operation is prepared for.
pub(crate) struct Workspace<'a> {
    /// The tree the operation runs in.
    pub tree: &'a Path,
    /// The repository the tree belongs to (its main checkout), when known.
    pub repo_root: Option<&'a Path>,
    /// Names the operation's private directory.
    pub name: &'a str,
    /// A branch the host moves the tree to.
    pub host_branch: Option<&'a str>,
    /// Host-owned paths the operation reads (a fetch source).
    pub host_read: &'a [PathBuf],
}

/// Prepare the boundary for one operation.
///
/// # Errors
/// The sentence naming why the workspace could not be bounded; nothing ran.
pub(crate) fn prepare(workspace: &Workspace<'_>) -> Result<HostLaunchPlan, String> {
    let mut scope = HostCommandScope::git(
        state_dir()?,
        None,
        workspace.repo_root,
        workspace.tree,
        workspace.name,
    );
    scope.host_branch = workspace.host_branch;
    scope.host_read = workspace.host_read;
    prepare_host_command(&scope).map_err(|refusal| format!("{}: {}", refusal.code, refusal.message))
}

/// Run `git <args>` in `dir` inside `plan`, answering stdout or git's own
/// complaint.
///
/// # Errors
/// Git failed to start or exited non-zero.
pub(crate) fn run(plan: &HostLaunchPlan, dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = plan.git_command(dir, args);
    command.stdin(std::process::Stdio::null());
    crate::util::configure_no_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git exited with status {}", output.status)
        } else {
            stderr
        })
    }
}

/// Run the checkout `args` inside the tree's boundary, then prove the
/// result: `HEAD` is `commit` and, for a `fresh` tree, `git status` (the
/// repository's own clean filters) finds no tracked file differing from it.
/// A tree that already held work (a seat's own clone) may carry the seat's
/// edits across the checkout exactly as Git does; its `HEAD` is still proved.
///
/// # Errors
/// The boundary could not be prepared, the checkout failed, or its result is
/// not `commit`.
pub(crate) fn materialize(
    workspace: &Workspace<'_>,
    args: &[&str],
    commit: &str,
    fresh: bool,
) -> Result<(), String> {
    let plan = prepare(workspace)?;
    run(&plan, workspace.tree, args)?;
    let head = run(&plan, workspace.tree, &["rev-parse", "HEAD"])?;
    if head.trim() != commit {
        return Err(format!(
            "the checkout landed on {} instead of {commit}",
            head.trim()
        ));
    }
    if !fresh {
        return Ok(());
    }
    let status = run(
        &plan,
        workspace.tree,
        &["status", "--porcelain", "--untracked-files=no"],
    )?;
    if !status.trim().is_empty() {
        return Err(format!(
            "the checkout of {commit} does not match that commit: {}",
            status.lines().take(5).collect::<Vec<_>>().join("; ")
        ));
    }
    Ok(())
}

/// `git status --porcelain` in `tree`, inside its boundary.
///
/// # Errors
/// The boundary could not be prepared, or git failed.
pub(crate) fn status(tree: &Path, repo_root: Option<&Path>) -> Result<String, String> {
    let workspace = Workspace {
        tree,
        repo_root,
        name: "status",
        host_branch: None,
        host_read: &[],
    };
    run(&prepare(&workspace)?, tree, &["status", "--porcelain"])
}
