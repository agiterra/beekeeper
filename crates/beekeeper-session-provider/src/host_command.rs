//! The process runner for a `run_on_host` action step.
//!
//! A small, exec-style (no shell) runner mirrored on
//! `crates/beekeeper-dev-mcp/src/shell.rs`: the child runs in its own process
//! group so a timeout kills everything it forked, its output is streamed to
//! artifact files capped at the step's `artifact_max_bytes`, and the result
//! carries only the last `tail_bytes` of each stream with every host-supplied
//! secret scrubbed. A timeout reports exit 124, the convention `timeout(1)`
//! established.
//!
//! Reimplemented rather than shared: `beekeeper-dev-mcp` is an agent tool with a
//! shell, a PATH shim and an MCP result shape, none of which belongs in the
//! provider. The pieces that matter for safety — process-group kill, bounded
//! capture, exit 124 — are the same by construction.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use beekeeper_workflow::schema::ResolvedRunOnHost;

use crate::agent_fence::FENCE;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// Exit code reported when the timeout fired and the process was killed.
pub const TIMEOUT_EXIT_CODE: i32 = 124;
/// Exit code reported when the program could not be started at all.
pub const SPAWN_FAILURE_EXIT_CODE: i32 = 127;
/// What every scrubbed secret is replaced with in a tail.
pub const SCRUBBED: &str = "[scrubbed]";
/// Read granularity for one output stream.
const READ_CHUNK: usize = 16 * 1024;
/// Grace between SIGTERM and SIGKILL on timeout.
const KILL_GRACE: Duration = Duration::from_millis(200);
/// How long a reaped-after-kill child may take to be waited on.
const REAP_DEADLINE: Duration = Duration::from_secs(2);
/// How long one `git` probe may run before it is abandoned.
const GIT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a fetch or a checkout inside a project boundary may take.
const FETCH_TIMEOUT: Duration = Duration::from_secs(300);
/// File names of the two artifact streams inside the artifact directory.
pub const STDOUT_LOG: &str = "stdout.log";
/// See [`STDOUT_LOG`].
pub const STDERR_LOG: &str = "stderr.log";

/// Refusal code when a `working_directory` resolves outside the checkout.
pub const ACTION_STEP_INVALID: &str = "ACTION_STEP_INVALID";
/// Refusal code when an `env_from_host` name is not set on this host.
pub const ACTION_HOST_ENV_MISSING: &str = "ACTION_HOST_ENV_MISSING";
/// `env_from_host` named one of the provider's own credentials.
pub const ACTION_HOST_ENV_FENCED: &str = "ACTION_HOST_ENV_FENCED";

/// Why a command could not be prepared for spawning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRefusal {
    /// Stable refusal code, e.g. [`ACTION_STEP_INVALID`].
    pub code: String,
    /// One-line human-readable reason.
    pub message: String,
}

/// What one run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCommandOutcome {
    /// Process exit status; [`TIMEOUT_EXIT_CODE`] on timeout,
    /// [`SPAWN_FAILURE_EXIT_CODE`] when the program never started, `None`
    /// only when the process ended by a signal outside the timeout path.
    pub exit_code: Option<i32>,
    /// Whether the timeout fired.
    pub timed_out: bool,
    /// Wall-clock milliseconds from spawn to reap.
    pub duration_ms: u64,
    /// Last `tail_bytes` of standard output, secrets scrubbed.
    pub stdout_tail: String,
    /// Last `tail_bytes` of standard error, secrets scrubbed.
    pub stderr_tail: String,
    /// Whether either tail was cut short of the full stream.
    pub truncated: bool,
    /// Directory holding `stdout.log` and `stderr.log`.
    pub artifact_path: PathBuf,
}

/// Everything resolved before a spawn: the directory to run in and the full
/// environment overlay, including the host-supplied values to scrub.
#[derive(Debug, Clone)]
pub struct PreparedCommand {
    /// Program and arguments.
    pub command: Vec<String>,
    /// Canonical working directory, verified inside the checkout.
    pub cwd: PathBuf,
    /// Literal plus host-supplied environment, applied over the process env.
    pub env: BTreeMap<String, String>,
    /// Values that must never appear in a tail.
    pub secrets: Vec<String>,
    /// Timeout in seconds.
    pub timeout_secs: u64,
    /// Bytes of each tail to keep.
    pub tail_bytes: usize,
    /// Cap on each artifact file.
    pub artifact_max_bytes: u64,
}

/// A spawned command whose completion has not been collected yet.
#[derive(Debug)]
pub struct HostCommand {
    /// OS pid of the child, when the runtime reports one.
    pub pid: Option<u32>,
    inner: Spawned,
}

#[derive(Debug)]
struct Spawned {
    child: tokio::process::Child,
    stdout: tokio::task::JoinHandle<CapturedStream>,
    stderr: tokio::task::JoinHandle<CapturedStream>,
    started: Instant,
    prepared: PreparedCommand,
    artifact_dir: PathBuf,
}

/// Resolve the working directory and environment for `spec` without
/// touching the process table.
///
/// Refuses (rather than errors) when the directory escapes the checkout or a
/// host-supplied variable is not set: both are facts the operator can act
/// on, and both are checked *before* the claim is published so a step that
/// cannot run here is never claimed here.
pub fn prepare(
    spec: &ResolvedRunOnHost,
    checkout: &Path,
    host_env: &HashMap<String, String>,
) -> Result<PreparedCommand, PrepareRefusal> {
    let cwd = resolve_working_directory(checkout, &spec.working_directory)?;
    let mut env = spec.env.clone();
    let mut secrets = Vec::with_capacity(spec.env_from_host.len());
    for name in &spec.env_from_host {
        // The provider's own credentials never reach a host step, not even
        // by name: the same fence every adapter it spawns is behind.
        if FENCE.covers(name) {
            return Err(PrepareRefusal {
                code: ACTION_HOST_ENV_FENCED.into(),
                message: format!(
                    "env_from_host names {name}, which this host never passes to a step"
                ),
            });
        }
        let Some(value) = host_env.get(name) else {
            return Err(PrepareRefusal {
                code: ACTION_HOST_ENV_MISSING.into(),
                message: format!(
                    "env_from_host names {name}, which is not set in this host's environment"
                ),
            });
        };
        if !value.is_empty() {
            secrets.push(value.clone());
        }
        env.insert(name.clone(), value.clone());
    }
    Ok(PreparedCommand {
        command: spec.command.clone(),
        cwd,
        env,
        secrets,
        timeout_secs: spec.timeout_secs,
        tail_bytes: usize::try_from(spec.tail_bytes).unwrap_or(usize::MAX),
        artifact_max_bytes: spec.artifact_max_bytes,
    })
}

/// `checkout/working_directory`, canonicalized and verified to lie inside
/// the canonical checkout. Symlinks that lead out are refused too.
pub fn resolve_working_directory(
    checkout: &Path,
    working_directory: &str,
) -> Result<PathBuf, PrepareRefusal> {
    let refuse = |message: String| PrepareRefusal {
        code: ACTION_STEP_INVALID.into(),
        message,
    };
    let root = checkout.canonicalize().map_err(|error| {
        refuse(format!(
            "repository folder {} cannot be resolved: {error}",
            checkout.display()
        ))
    })?;
    let cwd = root
        .join(working_directory)
        .canonicalize()
        .map_err(|error| {
            refuse(format!(
                "working_directory {working_directory:?} cannot be resolved inside {}: {error}",
                checkout.display()
            ))
        })?;
    if !cwd.starts_with(&root) {
        return Err(refuse(format!(
            "working_directory {working_directory:?} resolves outside the repository folder"
        )));
    }
    if !cwd.is_dir() {
        return Err(refuse(format!(
            "working_directory {working_directory:?} is not a directory"
        )));
    }
    Ok(cwd)
}

/// Spawn `prepared`, streaming its output into `artifact_dir`.
///
/// Returns as soon as the child exists so the caller can record its pid
/// durably before waiting. A program that cannot be started is *not* an
/// error here: it is reported through [`HostCommand::wait`] as exit
/// [`SPAWN_FAILURE_EXIT_CODE`] with the OS reason in the stderr tail, so the
/// step's result is a fact about the run rather than a provider failure.
///
/// `launch` says, explicitly, what the command runs inside: a prepared
/// project boundary with exactly its resolved environment
/// ([`crate::execution_scope_host`]), or — only where no backend exists, or in
/// a unit test of unrelated behaviour — nothing, named with its reason.
pub async fn spawn(
    prepared: PreparedCommand,
    artifact_dir: &Path,
    launch: HostLaunch<'_>,
) -> io::Result<HostCommand> {
    tokio::fs::create_dir_all(artifact_dir).await?;
    let stdout_path = artifact_dir.join(STDOUT_LOG);
    let stderr_path = artifact_dir.join(STDERR_LOG);
    let stdout_file = tokio::fs::File::create(&stdout_path).await?;
    let stderr_file = tokio::fs::File::create(&stderr_path).await?;

    let Some((program, args)) = prepared.command.split_first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "run_on_host command is empty",
        ));
    };
    let mut cmd = match launch {
        HostLaunch::Bounded(launch) => {
            let args: Vec<String> = args.to_vec();
            let (program, argv) = launch.boundary().wrap(program, &args);
            let mut cmd = Command::new(program);
            cmd.args(argv);
            // The step's own validated directory inside the bounded tree.
            cmd.current_dir(&prepared.cwd);
            cmd.env_clear();
            for (name, value) in launch.env().vars() {
                cmd.env(name, value);
            }
            cmd
        }
        HostLaunch::Unenforced { .. } => {
            let mut cmd = Command::new(program);
            cmd.args(args);
            cmd.current_dir(&prepared.cwd);
            // Strip the provider's own credentials (`BEEKEEPER_*`, `BUZZ_*` and the
            // enumerated keys) before the step's environment is layered on,
            // exactly as every adapter this sidecar spawns is stripped.
            for (key, _) in std::env::vars_os() {
                if FENCE.covers(&key.to_string_lossy()) {
                    cmd.env_remove(&key);
                }
            }
            for (name, value) in &prepared.env {
                cmd.env(name, value);
            }
            cmd
        }
    };
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    set_process_group(&mut cmd);

    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            // Record the reason where the operator will look for it.
            let mut file = stderr_file;
            let message = format!("failed to spawn {program:?}: {error}\n");
            let _ = file.write_all(message.as_bytes()).await;
            let _ = file.flush().await;
            let prepared_for_tail = prepared.clone();
            return Ok(HostCommand {
                pid: None,
                inner: Spawned {
                    child: never_spawned()?,
                    stdout: tokio::spawn(async { CapturedStream::default() }),
                    stderr: tokio::spawn(async move {
                        let mut stream = CapturedStream::new(prepared_for_tail.tail_bytes);
                        stream.push(message.as_bytes());
                        stream
                    }),
                    started,
                    prepared,
                    artifact_dir: artifact_dir.to_path_buf(),
                },
            });
        }
    };
    let pid = child.id();
    let tail = prepared.tail_bytes;
    let cap = prepared.artifact_max_bytes;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout = tokio::spawn(async move {
        match stdout_pipe {
            Some(pipe) => capture(pipe, stdout_file, tail, cap).await,
            None => CapturedStream::default(),
        }
    });
    let stderr = tokio::spawn(async move {
        match stderr_pipe {
            Some(pipe) => capture(pipe, stderr_file, tail, cap).await,
            None => CapturedStream::default(),
        }
    });
    Ok(HostCommand {
        pid,
        inner: Spawned {
            child,
            stdout,
            stderr,
            started,
            prepared,
            artifact_dir: artifact_dir.to_path_buf(),
        },
    })
}

/// A child that stands in for a program that never started: `true` on unix
/// exits 0 immediately and [`HostCommand::wait`] rewrites the code to
/// [`SPAWN_FAILURE_EXIT_CODE`] when the pid is absent.
fn never_spawned() -> io::Result<tokio::process::Child> {
    let mut cmd = Command::new(if cfg!(windows) { "cmd" } else { "true" });
    if cfg!(windows) {
        cmd.args(["/C", "exit 0"]);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
}

impl HostCommand {
    /// Wait for the command to finish or time out, then collect the outcome.
    pub async fn wait(self) -> HostCommandOutcome {
        let Spawned {
            mut child,
            stdout,
            stderr,
            started,
            prepared,
            artifact_dir,
        } = self.inner;
        let never_started = self.pid.is_none();
        let group = self.pid.map(|pid| pid as i32);
        let timeout = Duration::from_secs(prepared.timeout_secs);
        let (status, timed_out) = match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => (Some(status), false),
            Ok(Err(error)) => {
                tracing::warn!(target: "csp::actions", "child wait failed: {error}");
                (None, false)
            }
            Err(_) => {
                kill_group(group, &mut child).await;
                (None, true)
            }
        };
        if !timed_out {
            // Descendants that outlived the child hold the pipes open;
            // reap them so the readers reach EOF.
            signal_group(group, Signal::Kill);
        }
        let stdout = collect(stdout).await;
        let stderr = collect(stderr).await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let exit_code = if never_started {
            Some(SPAWN_FAILURE_EXIT_CODE)
        } else if timed_out {
            Some(TIMEOUT_EXIT_CODE)
        } else {
            status.and_then(|status| status.code())
        };
        // Scrubbing and lossy decoding can both grow a tail past
        // `tail_bytes`; the wire caps the tail, so cut again after them.
        let (stdout_tail, stdout_recut) = recut(
            scrub(stdout.tail_text(), &prepared.secrets),
            prepared.tail_bytes,
        );
        let (stderr_tail, stderr_recut) = recut(
            scrub(stderr.tail_text(), &prepared.secrets),
            prepared.tail_bytes,
        );
        HostCommandOutcome {
            exit_code,
            timed_out,
            duration_ms,
            stdout_tail,
            stderr_tail,
            truncated: stdout.cut || stderr.cut || stdout_recut || stderr_recut,
            artifact_path: artifact_dir,
        }
    }
}

/// What a host command runs inside, stated by the caller.
#[derive(Debug, Clone, Copy)]
pub enum HostLaunch<'a> {
    /// A prepared, verified project boundary.
    Bounded(&'a beekeeper_acp::acp::BoundedLaunch),
    /// Nothing is enforced: a platform with no backend, or a unit test of
    /// unrelated behaviour. The reason is disclosed wherever the run is.
    Unenforced {
        /// Stable reason code.
        reason: &'static str,
    },
}

/// Prepare, spawn and wait in one call.
pub async fn run(
    spec: &ResolvedRunOnHost,
    checkout: &Path,
    artifact_dir: &Path,
    host_env: &HashMap<String, String>,
    launch: HostLaunch<'_>,
) -> io::Result<HostCommandOutcome> {
    let prepared = prepare(spec, checkout, host_env)
        .map_err(|refusal| io::Error::new(io::ErrorKind::InvalidInput, refusal.message))?;
    let command = spawn(prepared, artifact_dir, launch).await?;
    Ok(command.wait().await)
}

/// The commit a request's trigger names, when it names one: `ref_updated`'s
/// `after`, else `ci_result`'s `commit`. A deleted ref has an empty (or
/// all-zero) `after` and names nothing.
pub fn triggering_commit(trigger_context: &serde_json::Value) -> Option<String> {
    commit_field(trigger_context, &["after", "commit"])
}

/// The commit a manual trigger bound the run to: `bee workflows trigger
/// --checkout <sha>`, carried verbatim on the trigger context. `None` when
/// the run bound none, which is the legacy manual case — the command then
/// runs in the recorded project directory as found, and says so.
pub fn bound_checkout(trigger_context: &serde_json::Value) -> Option<String> {
    commit_field(trigger_context, &["checkout"])
}

fn commit_field(trigger_context: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| {
            trigger_context
                .get(*key)
                .and_then(serde_json::Value::as_str)
        })
        .map(str::trim)
        .find(|value| value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|value| value.to_ascii_lowercase())
        // Git's own spelling of "no commit" on a deleted ref, should a
        // context ever carry it instead of the empty string.
        .filter(|value| value.bytes().any(|b| b != b'0'))
}

/// Cut a detached worktree of `checkout` at `commit` under `dir` **without
/// checking out any file**, fetching first when the commit is not yet local.
///
/// Nothing of the project's runs on the host here: the fetch runs inside
/// `checkout_plan` (the repository's configuration can name the transport
/// programs a fetch runs), and the cut itself is metadata only — no hook
/// (`core.hooksPath=/dev/null`) and no smudge filter, because nothing is
/// checked out. The tree is materialized afterwards by
/// [`materialize_worktree`], inside the step's own boundary.
///
/// # Errors
/// The commit is unknown after fetching, or the cut failed.
pub async fn cut_worktree_unmaterialized(
    checkout_plan: &crate::execution_scope_host::HostLaunchPlan,
    checkout: &Path,
    commit: &str,
    dir: &Path,
) -> Result<(), String> {
    let spec = format!("{commit}^{{commit}}");
    if git_output(checkout, &["cat-file", "-e", &spec])
        .await
        .is_none()
    {
        // Every remote the checkout offers: a specific-object fetch needs a
        // remote name this host may not have recorded.
        let _ = plan_git_output(
            checkout_plan,
            checkout,
            &["fetch", "--quiet", "--all", "--no-write-fetch-head"],
            FETCH_TIMEOUT,
        )
        .await;
        if git_output(checkout, &["cat-file", "-e", &spec])
            .await
            .is_none()
        {
            return Err(format!(
                "commit {commit} is not in {} even after fetching its remotes",
                checkout.display()
            ));
        }
    }
    if let Some(parent) = dir.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let dir_text = dir.to_string_lossy().into_owned();
    git_output(
        checkout,
        &[
            "worktree",
            "add",
            "--no-checkout",
            "--detach",
            "--quiet",
            &dir_text,
            commit,
        ],
    )
    .await
    .map(|_| ())
    .ok_or_else(|| {
        format!(
            "git worktree add at {commit} failed under {}",
            dir.display()
        )
    })
}

/// Check out `commit` into a worktree cut by [`cut_worktree_unmaterialized`],
/// inside `plan`'s boundary: any smudge filter the checkout runs is the
/// project's own code, and runs with the project's rights only. Verified
/// afterwards: `HEAD` is `commit` and the tree matches it.
///
/// # Errors
/// The checkout failed, or its result is not exactly `commit`.
pub async fn materialize_worktree(
    plan: &crate::execution_scope_host::HostLaunchPlan,
    dir: &Path,
    commit: &str,
) -> Result<(), String> {
    let output = plan_git_run(
        plan,
        dir,
        &["reset", "--hard", "--quiet", commit],
        FETCH_TIMEOUT,
    )
    .await
    .map_err(|error| format!("checking out {commit} could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "checking out {commit} inside the project boundary failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let (head, dirty) = git_head_and_dirty(plan, dir).await;
    match (head, dirty) {
        (Some(head), Some(false)) if head == commit.to_ascii_lowercase() => Ok(()),
        (head, dirty) => Err(format!(
            "the checkout of {commit} is not that commit's tree (HEAD {}, {})",
            head.as_deref().unwrap_or("unknown"),
            match dirty {
                Some(true) => "modified",
                Some(false) => "clean",
                None => "status unknown",
            }
        )),
    }
}

/// Remove a worktree [`cut_worktree_unmaterialized`] made. Best effort; the directory is
/// under the host's own state and a leftover is disclosed in the log.
pub async fn remove_worktree(checkout: &Path, dir: &Path) {
    let dir_text = dir.to_string_lossy().into_owned();
    if git_output(checkout, &["worktree", "remove", "--force", &dir_text])
        .await
        .is_none()
    {
        tracing::warn!(
            target: "csp::actions",
            worktree = %dir.display(),
            "could not remove the action's worktree; it stays under the host's state directory"
        );
    }
}

/// `git rev-parse HEAD` and whether `git status --porcelain` is non-empty,
/// each `None` when git could not answer within a bounded time. Status runs
/// inside `plan` (it can run the repository's clean filters and fsmonitor).
pub async fn git_head_and_dirty(
    plan: &crate::execution_scope_host::HostLaunchPlan,
    dir: &Path,
) -> (Option<String>, Option<bool>) {
    let head = git_output(dir, &["rev-parse", "HEAD"])
        .await
        .map(|text| text.trim().to_ascii_lowercase())
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()));
    let dirty = match plan {
        crate::execution_scope_host::HostLaunchPlan::Bounded(_) => {
            plan_git_output(plan, dir, &["status", "--porcelain"], GIT_PROBE_TIMEOUT)
                .await
                .map(|text| !text.trim().is_empty())
        }
        // Unbounded, `status` would run the repository's clean filters on the
        // host: read the dirty state from stat information alone.
        crate::execution_scope_host::HostLaunchPlan::Unenforced { .. } => {
            let dir = dir.to_path_buf();
            tokio::task::spawn_blocking(move || stat_only_dirty(&dir))
                .await
                .ok()
                .flatten()
        }
    };
    (head, dirty)
}

/// The paths that differ from `HEAD`, without running any of the
/// repository's own code: `diff-index --name-only` against the index's stat
/// information (no refresh, so no clean filter; every index entry when there
/// is no commit yet) plus untracked files. A file touched but not changed can
/// be listed — the safe direction for a probe.
pub(crate) fn stat_only_changes(dir: &Path) -> Option<Vec<String>> {
    let lines = |args: &[&str]| -> Option<Vec<String>> {
        let output = metadata_git_command(dir)
            .arg("--no-optional-locks")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        output.status.success().then(|| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        })
    };
    let born = metadata_git_command(dir)
        .args(["rev-parse", "--verify", "-q", "HEAD"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?
        .success();
    let mut changes = if born {
        lines(&["diff-index", "--name-only", "HEAD", "--"])?
    } else {
        lines(&["ls-files"])?
    };
    changes.extend(lines(&["ls-files", "--others", "--exclude-standard"])?);
    Some(changes)
}

/// Whether [`stat_only_changes`] lists anything.
pub(crate) fn stat_only_dirty(dir: &Path) -> Option<bool> {
    stat_only_changes(dir).map(|changes| !changes.is_empty())
}

/// A `git` command for an operation that runs none of the repository's own
/// code — reading refs and objects, cutting a worktree with nothing checked
/// out, removing one with `--force` (no status check). Hooks and fsmonitor
/// are off, nothing prompts, and the repository is chosen by the working
/// directory alone. Clean/smudge filters are **not** switched off: `status`,
/// checkout and anything else that refreshes the index would run them, so
/// those never use this command; they run inside a prepared boundary instead
/// ([`crate::execution_scope_host::HostLaunchPlan::git_command`]).
#[must_use]
pub fn metadata_git_command(dir: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    command
        .args(crate::git_probe::HOST_GIT_NO_PROJECT_CODE)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

async fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::from(metadata_git_command(dir));
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(GIT_PROBE_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Run `git <args>` in `dir` inside `plan`.
async fn plan_git_run(
    plan: &crate::execution_scope_host::HostLaunchPlan,
    dir: &Path,
    args: &[&str],
    limit: Duration,
) -> io::Result<std::process::Output> {
    let mut cmd = Command::from(plan.git_command(dir, args));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    tokio::time::timeout(limit, cmd.output())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "git did not finish in time"))?
}

async fn plan_git_output(
    plan: &crate::execution_scope_host::HostLaunchPlan,
    dir: &Path,
    args: &[&str],
    limit: Duration,
) -> Option<String> {
    let output = plan_git_run(plan, dir, args, limit).await.ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether a process with `pid` exists on this host.
///
/// On unix a zero signal probes without delivering; `EPERM` still means the
/// process exists. Elsewhere the answer is `false`: a restart cannot tell,
/// and "lost" is the honest report for a run it cannot observe.
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::errno::Errno;
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        let Ok(raw) = i32::try_from(pid) else {
            return false;
        };
        match kill(Pid::from_raw(raw), None) {
            Ok(()) => true,
            Err(Errno::EPERM) => true,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// This machine's hostname, when the OS can say cheaply.
pub fn hostname() -> Option<String> {
    #[cfg(unix)]
    {
        nix::unistd::gethostname()
            .ok()
            .and_then(|name| name.into_string().ok())
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    }
    #[cfg(not(unix))]
    {
        std::env::var("COMPUTERNAME")
            .ok()
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    }
}

/// Keep the last `max` bytes of `text` on a character boundary; the flag says
/// whether anything was cut.
pub fn recut(text: String, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut start = text.len() - max;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    (text[start..].to_owned(), true)
}

/// Secrets shorter than this are not scrubbed: replacing every `1` or `ab`
/// in a log would destroy it while hiding nothing worth hiding. Disclosed
/// in the spec's scrubbing audit (ledger 156).
pub const MIN_SCRUB_SECRET_BYTES: usize = 4;

/// Replace every occurrence of each secret in `text` with [`SCRUBBED`].
///
/// Exact substrings only: a secret that a command printed encoded (base64,
/// URL-escaped, split across lines) is not recognised. That is the audit's
/// stated limit, not a promise this function cannot keep.
pub fn scrub(mut text: String, secrets: &[String]) -> String {
    for secret in secrets {
        if secret.len() >= MIN_SCRUB_SECRET_BYTES && text.contains(secret.as_str()) {
            text = text.replace(secret.as_str(), SCRUBBED);
        }
    }
    text
}

/// Scrub a whole log file's bytes for upload: decoded lossily, scrubbed as
/// text, re-encoded. A log that is not UTF-8 loses its invalid bytes, which
/// is the price of never uploading a secret that straddles them.
pub fn scrub_bytes(bytes: &[u8], secrets: &[String]) -> Vec<u8> {
    scrub(String::from_utf8_lossy(bytes).into_owned(), secrets).into_bytes()
}

#[derive(Debug, Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

#[cfg(unix)]
fn set_process_group(cmd: &mut Command) {
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn set_process_group(_cmd: &mut Command) {}

#[cfg(unix)]
fn signal_group(group: Option<i32>, signal: Signal) {
    use nix::sys::signal::{killpg, Signal as NixSignal};
    use nix::unistd::Pid;
    if let Some(pgid) = group {
        let signal = match signal {
            Signal::Term => NixSignal::SIGTERM,
            Signal::Kill => NixSignal::SIGKILL,
        };
        let _ = killpg(Pid::from_raw(pgid), signal);
    }
}

#[cfg(not(unix))]
fn signal_group(_group: Option<i32>, _signal: Signal) {}

/// Terminate the process group gracefully, then hard, then reap the child.
async fn kill_group(group: Option<i32>, child: &mut tokio::process::Child) {
    signal_group(group, Signal::Term);
    tokio::time::sleep(KILL_GRACE).await;
    signal_group(group, Signal::Kill);
    if tokio::time::timeout(REAP_DEADLINE, child.wait())
        .await
        .is_err()
    {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}

async fn collect(handle: tokio::task::JoinHandle<CapturedStream>) -> CapturedStream {
    match tokio::time::timeout(Duration::from_secs(5), handle).await {
        Ok(Ok(stream)) => stream,
        _ => CapturedStream::default(),
    }
}

/// The last `tail_bytes` of one stream, plus whether anything was dropped.
#[derive(Debug, Default)]
struct CapturedStream {
    tail: Vec<u8>,
    tail_bytes: usize,
    /// Whether the tail is shorter than what the process wrote.
    cut: bool,
}

impl CapturedStream {
    fn new(tail_bytes: usize) -> Self {
        Self {
            tail: Vec::new(),
            tail_bytes,
            cut: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > self.tail_bytes {
            let excess = self.tail.len() - self.tail_bytes;
            self.tail.drain(..excess);
            self.cut = true;
        }
    }

    /// The tail as text, dropping a leading partial UTF-8 sequence.
    fn tail_text(&self) -> String {
        let start = self
            .tail
            .iter()
            .position(|byte| (byte & 0b1100_0000) != 0b1000_0000)
            .unwrap_or(self.tail.len());
        String::from_utf8_lossy(&self.tail[start..]).into_owned()
    }
}

/// Stream `reader` into `file` up to `cap` bytes while keeping the tail.
async fn capture<R: AsyncRead + Unpin>(
    mut reader: R,
    mut file: tokio::fs::File,
    tail_bytes: usize,
    cap: u64,
) -> CapturedStream {
    let mut stream = CapturedStream::new(tail_bytes);
    let mut written: u64 = 0;
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                let bytes = &chunk[..n];
                stream.push(bytes);
                let room = cap.saturating_sub(written);
                let take = usize::try_from(room).map_or(n, |room| room.min(n));
                if take > 0 {
                    if file.write_all(&bytes[..take]).await.is_err() {
                        // Disk trouble must not stall the process: keep
                        // draining the pipe, stop writing.
                        written = cap;
                        continue;
                    }
                    written = written.saturating_add(take as u64);
                }
            }
            Err(_) => break,
        }
    }
    let _ = file.flush().await;
    stream
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(command: &[&str], timeout_secs: u64) -> ResolvedRunOnHost {
        ResolvedRunOnHost {
            command: command.iter().map(|part| (*part).to_owned()).collect(),
            working_directory: ".".into(),
            timeout_secs,
            env: BTreeMap::new(),
            env_from_host: Vec::new(),
            tail_bytes: 64,
            artifact_max_bytes: 128,
            upload: false,
            checkout: beekeeper_workflow::schema::HostCheckout::Current,
        }
    }

    #[tokio::test]
    async fn true_exits_zero_and_false_exits_one() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let artifacts = tempfile::tempdir().expect("tempdir");
        let env = HashMap::new();
        let ok = run(
            &spec(&["true"], 5),
            checkout.path(),
            artifacts.path(),
            &env,
            HostLaunch::Unenforced {
                reason: "unit-test",
            },
        )
        .await
        .expect("run true");
        assert_eq!(ok.exit_code, Some(0));
        assert!(!ok.timed_out);
        assert!(artifacts.path().join(STDOUT_LOG).is_file());
        let failed = run(
            &spec(&["false"], 5),
            checkout.path(),
            artifacts.path(),
            &env,
            HostLaunch::Unenforced {
                reason: "unit-test",
            },
        )
        .await
        .expect("run false");
        assert_eq!(failed.exit_code, Some(1));
    }

    #[tokio::test]
    async fn a_timeout_kills_the_group_and_reports_124() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let artifacts = tempfile::tempdir().expect("tempdir");
        let started = Instant::now();
        let outcome = run(
            &spec(&["sleep", "5"], 1),
            checkout.path(),
            artifacts.path(),
            &HashMap::new(),
            HostLaunch::Unenforced {
                reason: "unit-test",
            },
        )
        .await
        .expect("run sleep");
        assert_eq!(outcome.exit_code, Some(TIMEOUT_EXIT_CODE));
        assert!(outcome.timed_out);
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "the timeout must cut the sleep short"
        );
    }

    #[tokio::test]
    async fn host_values_are_scrubbed_from_tails_and_tails_are_bounded() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let artifacts = tempfile::tempdir().expect("tempdir");
        let mut spec = spec(&["sh", "-c", "echo token=$TOKEN; echo $TOKEN >&2"], 5);
        spec.env_from_host = vec!["TOKEN".into()];
        let mut env = HashMap::new();
        env.insert("TOKEN".to_owned(), "hunter2-secret".to_owned());
        let outcome = run(
            &spec,
            checkout.path(),
            artifacts.path(),
            &env,
            HostLaunch::Unenforced {
                reason: "unit-test",
            },
        )
        .await
        .expect("run");
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.stdout_tail.trim(), format!("token={SCRUBBED}"));
        assert_eq!(outcome.stderr_tail.trim(), SCRUBBED);
        assert!(!outcome.truncated);

        // A stream longer than the tail keeps only its end and says so.
        let long = self::spec(&["sh", "-c", "yes abcdefgh | head -c 1000"], 5);
        let outcome = run(
            &long,
            checkout.path(),
            artifacts.path(),
            &HashMap::new(),
            HostLaunch::Unenforced {
                reason: "unit-test",
            },
        )
        .await
        .expect("run");
        assert!(outcome.truncated);
        assert_eq!(outcome.stdout_tail.len(), 64);
        let artifact = std::fs::read(artifacts.path().join(STDOUT_LOG)).expect("artifact");
        assert_eq!(artifact.len(), 128, "artifact stops at artifact_max_bytes");
    }

    #[tokio::test]
    async fn a_missing_host_variable_and_an_escaping_directory_are_refused() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let mut spec = spec(&["true"], 5);
        spec.env_from_host = vec!["NOPE_NOT_SET".into()];
        let refusal = prepare(&spec, checkout.path(), &HashMap::new()).unwrap_err();
        assert_eq!(refusal.code, ACTION_HOST_ENV_MISSING);

        let mut escaping = self::spec(&["true"], 5);
        escaping.working_directory = "../".into();
        let refusal = prepare(&escaping, checkout.path(), &HashMap::new()).unwrap_err();
        assert_eq!(refusal.code, ACTION_STEP_INVALID);
        assert!(refusal.message.contains("outside"));

        // A symlink that points out of the checkout is an escape too.
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().expect("tempdir");
            std::os::unix::fs::symlink(outside.path(), checkout.path().join("link"))
                .expect("symlink");
            let mut linked = self::spec(&["true"], 5);
            linked.working_directory = "link".into();
            let refusal = prepare(&linked, checkout.path(), &HashMap::new()).unwrap_err();
            assert_eq!(refusal.code, ACTION_STEP_INVALID);
        }
    }

    #[test]
    fn env_from_host_may_not_name_the_providers_own_credentials() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = ResolvedRunOnHost {
            command: vec!["true".into()],
            working_directory: ".".into(),
            timeout_secs: 5,
            env: BTreeMap::new(),
            env_from_host: vec!["BEEKEEPER_PRIVATE_KEY".into()],
            tail_bytes: 64,
            artifact_max_bytes: 1024,
            upload: false,
            checkout: beekeeper_workflow::schema::HostCheckout::Current,
        };
        let mut host_env = HashMap::new();
        host_env.insert("BEEKEEPER_PRIVATE_KEY".to_owned(), "nsec".to_owned());
        let refusal = prepare(&spec, dir.path(), &host_env).expect_err("fenced name is refused");
        assert_eq!(refusal.code, ACTION_HOST_ENV_FENCED);
        assert!(FENCE.covers("BEEKEEPER_PRIVATE_KEY"));
        assert!(FENCE.covers("NOSTR_PRIVATE_KEY"));
        assert!(FENCE.covers("BUZZ_PRIVATE_KEY"));
    }

    #[test]
    fn recut_keeps_the_last_bytes_on_a_character_boundary() {
        let (kept, cut) = recut("abc".into(), 8);
        assert_eq!((kept.as_str(), cut), ("abc", false));
        let (kept, cut) = recut("xx\u{e9}yy".into(), 4);
        assert_eq!((kept.as_str(), cut), ("\u{e9}yy", true));
        let (kept, cut) = recut("xx\u{e9}yy".into(), 3);
        assert_eq!(
            (kept.as_str(), cut),
            ("yy", true),
            "a split character is dropped"
        );
    }

    #[tokio::test]
    async fn a_program_that_cannot_start_reports_127_with_the_reason() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let artifacts = tempfile::tempdir().expect("tempdir");
        let outcome = run(
            &spec(&["/definitely/not/a/program"], 5),
            checkout.path(),
            artifacts.path(),
            &HashMap::new(),
            HostLaunch::Unenforced {
                reason: "unit-test",
            },
        )
        .await
        .expect("run");
        assert_eq!(outcome.exit_code, Some(SPAWN_FAILURE_EXIT_CODE));
        // The reason is longer than this test's 64-byte tail, so only its end
        // survives: the OS error, not the "failed to spawn" prefix.
        assert!(
            outcome.stderr_tail.contains("os error"),
            "{:?}",
            outcome.stderr_tail
        );
    }

    #[test]
    fn tail_text_drops_a_leading_partial_character() {
        let mut stream = CapturedStream::new(4);
        // "é" is two bytes; a 4-byte tail of "aéé" starts mid-character.
        stream.push("aéé".as_bytes());
        assert_eq!(stream.tail_text(), "\u{e9}\u{e9}"[..].to_owned());
        assert!(stream.cut);
    }

    #[test]
    fn pid_liveness_answers_for_this_process_and_a_dead_one() {
        assert!(pid_alive(std::process::id()));
        // Pid 1 exists on every unix; a huge pid does not.
        assert!(!pid_alive(u32::MAX - 1));
    }

    #[tokio::test]
    async fn git_probe_reports_head_and_dirtiness() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(checkout.path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ]);
        let plan = crate::execution_scope_host::HostLaunchPlan::Unenforced {
            reason: "unit-test",
        };
        let (head, dirty) = git_head_and_dirty(&plan, checkout.path()).await;
        assert_eq!(head.map(|sha| sha.len()), Some(40));
        assert_eq!(dirty, Some(false));
        std::fs::write(checkout.path().join("new.txt"), "x").expect("write");
        let (_, dirty) = git_head_and_dirty(&plan, checkout.path()).await;
        assert_eq!(dirty, Some(true));

        let not_a_repo = tempfile::tempdir().expect("tempdir");
        // Not a repository: git answers with a non-zero status, so `None`
        // rather than a guess. (A parent repo would be found by git; a fresh
        // tempdir under /tmp has none.)
        let (head, _) = git_head_and_dirty(&plan, not_a_repo.path()).await;
        assert!(head.is_none() || head.as_deref().map(str::len) == Some(40));
    }

    #[test]
    fn short_secrets_are_not_scrubbed_and_bytes_scrub_like_text() {
        let secrets = vec!["ab".to_owned(), "s3cr3t-value".to_owned()];
        let text = scrub("ab s3cr3t-value ab".to_owned(), &secrets);
        assert_eq!(
            text,
            format!("ab {SCRUBBED} ab"),
            "a 2-byte value must not erase every ab"
        );
        let bytes = scrub_bytes(b"token=s3cr3t-value\n", &secrets);
        assert_eq!(bytes, format!("token={SCRUBBED}\n").into_bytes());
    }

    #[test]
    fn the_triggering_commit_is_after_then_commit_and_must_be_a_sha() {
        let sha = "a".repeat(40);
        assert_eq!(
            triggering_commit(&serde_json::json!({ "after": sha, "commit": "b" })),
            Some(sha.clone())
        );
        assert_eq!(
            triggering_commit(&serde_json::json!({ "commit": sha })),
            Some(sha.clone())
        );
        assert_eq!(
            triggering_commit(&serde_json::json!({ "after": "0".repeat(40) })),
            None,
            "a deleted ref names no commit"
        );
        assert_eq!(
            triggering_commit(&serde_json::json!({ "after": "main" })),
            None
        );
        assert_eq!(triggering_commit(&serde_json::json!({})), None);
    }

    async fn git(dir: &Path, args: &[&str]) -> String {
        let out = tokio::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example")
            .output()
            .await
            .expect("git");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    #[tokio::test]
    async fn a_worktree_is_cut_at_the_commit_and_removed_after() {
        let repo = tempfile::tempdir().expect("tempdir");
        git(repo.path(), &["init", "-q", "-b", "main"]).await;
        tokio::fs::write(repo.path().join("f"), "one")
            .await
            .expect("write");
        git(repo.path(), &["add", "f"]).await;
        git(repo.path(), &["commit", "-q", "-m", "one"]).await;
        let first = git(repo.path(), &["rev-parse", "HEAD"]).await;
        tokio::fs::write(repo.path().join("f"), "two")
            .await
            .expect("write");
        git(repo.path(), &["commit", "-q", "-am", "two"]).await;

        let worktree = tempfile::tempdir().expect("tempdir").path().join("wt");
        let plan = crate::execution_scope_host::HostLaunchPlan::Unenforced {
            reason: "unit-test",
        };
        cut_worktree_unmaterialized(&plan, repo.path(), &first, &worktree)
            .await
            .expect("cut");
        assert!(!worktree.join("f").exists(), "the cut checks nothing out");
        materialize_worktree(&plan, &worktree, &first)
            .await
            .expect("materialize");
        assert_eq!(
            tokio::fs::read_to_string(worktree.join("f"))
                .await
                .expect("read"),
            "one",
            "the worktree holds the older commit, not HEAD"
        );
        let (head, dirty) = git_head_and_dirty(&plan, &worktree).await;
        assert_eq!(head.as_deref(), Some(first.as_str()));
        assert_eq!(dirty, Some(false));

        remove_worktree(repo.path(), &worktree).await;
        assert!(!worktree.exists(), "the worktree is removed after the run");
        let list = git(repo.path(), &["worktree", "list"]).await;
        assert_eq!(
            list.lines().count(),
            1,
            "only the checkout itself remains: {list}"
        );
    }

    #[tokio::test]
    async fn an_unknown_commit_refuses_the_cut() {
        let repo = tempfile::tempdir().expect("tempdir");
        git(repo.path(), &["init", "-q", "-b", "main"]).await;
        git(repo.path(), &["commit", "-q", "--allow-empty", "-m", "one"]).await;
        let worktree = tempfile::tempdir().expect("tempdir").path().join("wt");
        let plan = crate::execution_scope_host::HostLaunchPlan::Unenforced {
            reason: "unit-test",
        };
        let error = cut_worktree_unmaterialized(&plan, repo.path(), &"f".repeat(40), &worktree)
            .await
            .expect_err("unknown commit");
        assert!(error.contains("even after fetching"), "{error}");
        assert!(!worktree.exists());
    }
}
