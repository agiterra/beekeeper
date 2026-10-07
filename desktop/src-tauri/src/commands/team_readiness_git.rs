use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::TeamReadinessSource;
// The seven variables that override `-C <path>` and can retarget this probe
// at a hook's repository instead of the one it was given, shared with the
// seat-hook installer and `run_git` rather than kept as a second copy here —
// see that const's doc comment for why it must be cleared.
use crate::commands::project_git_exec::GIT_REPO_SELECTION_VARS;

const GIT_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn parse_embedded_source(
    commit: Option<&str>,
    dirty: Option<&str>,
) -> TeamReadinessSource {
    let app_commit = commit
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase);
    let app_source_dirty = match dirty {
        Some("1") => Some(true),
        _ => None,
    };
    TeamReadinessSource {
        app_commit,
        app_source_dirty,
        ..Default::default()
    }
}

pub(super) fn embedded_source() -> TeamReadinessSource {
    parse_embedded_source(
        option_env!("BEEKEEPER_DESKTOP_BUILD_SOURCE_SHA"),
        option_env!("BEEKEEPER_DESKTOP_BUILD_SOURCE_DIRTY"),
    )
}

fn git_run(checkout: &Path, args: &[&str], capture: bool) -> Result<(i32, String), String> {
    let mut command = Command::new("git");
    for var in GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    let mut child = command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(checkout)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("git could not start: {error}"))?;
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("git probe timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut output = String::new();
    if capture {
        child
            .stdout
            .as_mut()
            .ok_or("git stdout unavailable")?
            .read_to_string(&mut output)
            .map_err(|error| error.to_string())?;
    }
    Ok((status.code().unwrap_or(-1), output.trim().into()))
}

pub(super) fn checkout_source(checkout: &Path) -> Result<(String, bool), String> {
    let (code, commit) = git_run(checkout, &["rev-parse", "HEAD"], true)?;
    validate_head(code, &commit)?;
    let (tracked, _) = git_run(checkout, &["diff-index", "--quiet", "HEAD", "--"], false)?;
    let (untracked, _) = git_run(
        checkout,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--error-unmatch",
            "--",
            "*",
        ],
        false,
    )?;
    Ok((
        commit.to_ascii_lowercase(),
        validate_dirty(tracked, untracked)?,
    ))
}

fn validate_head(code: i32, commit: &str) -> Result<(), String> {
    if code == 0 && commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("Git HEAD is unavailable".into())
    }
}

pub(super) fn validate_dirty(tracked: i32, untracked: i32) -> Result<bool, String> {
    if !matches!(tracked, 0 | 1) {
        return Err(format!(
            "Git tracked-state probe failed with exit {tracked}"
        ));
    }
    if !matches!(untracked, 0 | 1) {
        return Err(format!(
            "Git untracked-state probe failed with exit {untracked}"
        ));
    }
    Ok(tracked == 1 || untracked == 0)
}
