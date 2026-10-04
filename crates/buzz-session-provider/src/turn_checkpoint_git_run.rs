//! The one way [`super`] runs git: inside the tree's boundary, with no hook,
//! no fsmonitor, the checkpoint identity and a C locale, and a failure that
//! is classified without ever copying git's stderr (which can carry host
//! paths and output from repository-configured programs).

use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::{
    CaptureFailure, UnavailableCode, BASE_CONFIG, CHECKPOINT_AUTHOR_EMAIL, CHECKPOINT_AUTHOR_NAME,
};
use crate::execution_scope_host::HostLaunchPlan;

/// One failed git step: which, and how, without its stderr (which can carry
/// host paths and output from repository-configured programs).
#[derive(Debug)]
pub(super) struct StepError {
    pub(super) step: &'static str,
    pub(super) exit: Option<i32>,
    pub(super) kind: StepFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StepFailure {
    /// git said the directory is not in a repository.
    NotARepository,
    /// git refused a repository owned by someone else (`safe.directory`).
    DubiousOwnership,
    /// git could not be started.
    Spawn,
    /// Any other non-zero exit.
    Exit,
}

impl StepError {
    pub(super) fn into_failure(self) -> CaptureFailure {
        let exit = self.exit.map_or_else(
            || "no exit status".to_owned(),
            |code| format!("exit {code}"),
        );
        match self.kind {
            StepFailure::NotARepository => CaptureFailure::new(
                UnavailableCode::NotARepository,
                "The working directory is not in a git repository.",
            ),
            StepFailure::DubiousOwnership => CaptureFailure::new(
                UnavailableCode::GitFailed,
                "git refused the repository because another user owns it (safe.directory).",
            ),
            StepFailure::Spawn => CaptureFailure::new(
                UnavailableCode::GitFailed,
                format!("git could not be started for `{}`.", self.step),
            ),
            StepFailure::Exit => CaptureFailure::new(
                UnavailableCode::GitFailed,
                format!("git {} failed ({exit}).", self.step),
            ),
        }
    }
}

/// Runs git in one working directory, inside the tree's boundary, optionally
/// against the scratch index.
pub(super) struct Git<'a> {
    cwd: &'a Path,
    plan: &'a HostLaunchPlan,
    index: Option<&'a Path>,
}

impl<'a> Git<'a> {
    pub(super) fn new(cwd: &'a Path, plan: &'a HostLaunchPlan, index: Option<&'a Path>) -> Self {
        Self { cwd, plan, index }
    }

    pub(super) async fn run(
        &self,
        step: &'static str,
        args: &[&str],
        stdin: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, StepError> {
        let mut full: Vec<&str> = BASE_CONFIG.to_vec();
        full.push("--no-optional-locks");
        full.extend_from_slice(args);
        let mut command = Command::from(self.plan.git_command(self.cwd, &full));
        for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        if let Some(index) = self.index {
            command.env("GIT_INDEX_FILE", index);
        }
        finish(command, step, stdin).await
    }
}

/// Spawn `command` with the checkpoint identity and a C locale, feed it
/// `stdin`, and classify the outcome.
pub(super) async fn finish(
    mut command: Command,
    step: &'static str,
    stdin: Option<Vec<u8>>,
) -> Result<Vec<u8>, StepError> {
    command
        .env("GIT_AUTHOR_NAME", CHECKPOINT_AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", CHECKPOINT_AUTHOR_EMAIL)
        .env("GIT_COMMITTER_NAME", CHECKPOINT_AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", CHECKPOINT_AUTHOR_EMAIL)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let spawn_error = |_: std::io::Error| StepError {
        step,
        exit: None,
        kind: StepFailure::Spawn,
    };
    let mut child = command.spawn().map_err(spawn_error)?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A failed write surfaces as git's own refusal below.
        let _ = pipe.write_all(&bytes).await;
        drop(pipe);
    }
    let output = child.wait_with_output().await.map_err(spawn_error)?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let kind = if stderr.contains("not a git repository") {
        StepFailure::NotARepository
    } else if stderr.contains("dubious ownership") {
        StepFailure::DubiousOwnership
    } else {
        StepFailure::Exit
    };
    tracing::debug!(target: "csp::checkpoint", step, status = %output.status,
        "a checkpoint git step did not succeed");
    Err(StepError {
        step,
        exit: output.status.code(),
        kind,
    })
}
