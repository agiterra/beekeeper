//! The project boundary around host-run project commands: `run_on_host`
//! action steps (including exact-SHA verification) and host Git that can run
//! a project's own code in its workspace.
//!
//! A verify, build or test step runs the project's own code — its scripts,
//! its build files, its test suite. So does Git in a project workspace when
//! it checks out, reports status or fetches: the repository's configuration
//! can name filter, hook, fsmonitor and transport programs, and a session of
//! that project can write that configuration. All of it needs exactly what a
//! session needs — its workspace, its declared toolchain and environment, its
//! Git transport — and must not read another project's material any more
//! than a session can. So it runs inside the same host preparation
//! ([`crate::execution_scope::prepare`], purpose
//! [`ScopePurpose::HostCommand`]): the project's tools and configuration, no
//! model runtime, no model login.
//!
//! The working tree is the project's recorded checkout, a linked worktree of
//! it, or a directory the host itself staged for the project (its detached
//! action worktree under `<state>/actions`, a seat's agents clone). The
//! step's environment is the platform/toolchain baseline, then the step's
//! declared `env` and the host variables it names in `env_from_host` (the
//! existing action contract), then the scope's own locations. A declaration
//! that names a scope-owned variable, or a `PATH` outside the boundary,
//! refuses the step before it runs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use beekeeper_acp::acp::BoundedLaunch;
use beekeeper_acp::exec_env::EnvSource;
use sha2::{Digest, Sha256};

use crate::execution_scope::{
    home_dir, prepare, ExecutionPlan, ScopeInputs, ScopePurpose, WorkspaceAssociation,
    EXECUTION_BOUNDARY_UNAVAILABLE,
};
use crate::host_command::PrepareRefusal;

/// Refusal code: the promised boundary around a host step could not be put
/// in place, so the step did not run.
pub const ACTION_BOUNDARY_UNAVAILABLE: &str = "ACTION_BOUNDARY_UNAVAILABLE";

/// What a host command's scope is derived from.
pub struct HostCommandScope<'a> {
    /// Provider (or host) state dir: policies and the command's private
    /// directories live under it.
    pub state_dir: &'a Path,
    /// The project coordinate the command belongs to, when it has one.
    pub project_ref: Option<&'a str>,
    /// The project's recorded checkout, when the host has one.
    pub checkout: Option<&'a Path>,
    /// Where the command runs: the checkout, a linked worktree of it, or a
    /// directory the host staged for the project.
    pub run_dir: &'a Path,
    /// The command's working directory (inside `run_dir`).
    pub cwd: &'a Path,
    /// Names the command's private directory (stable per step or workspace).
    pub name: &'a str,
    /// How the host ties `run_dir` to the project.
    pub association: WorkspaceAssociation,
    /// The step's declared literal environment.
    pub declared_env: &'a BTreeMap<String, String>,
    /// The host variables the step names, with this host's values.
    pub from_host: &'a [(String, String)],
    /// Hermit's package state on this host.
    pub hermit_state: Option<&'a Path>,
    /// A branch the command (host-decided) moves the worktree to.
    pub host_branch: Option<&'a str>,
    /// The branch the host's own record allocates to this tree, when the
    /// caller holds it (assignment establishment): the only branch the
    /// command may move.
    pub branch_authority: Option<&'a str>,
    /// Host-owned paths the command reads.
    pub host_read: &'a [PathBuf],
    /// Whether the command fetches with the operator's selected Git
    /// transport (see [`ScopeInputs::operator_git_auth`]). `false` for any
    /// command that runs project code without needing it.
    pub git_transport: bool,
}

impl<'a> HostCommandScope<'a> {
    /// A scope for host Git in `run_dir`: nothing declared, no extra rights.
    #[must_use]
    pub fn git(
        state_dir: &'a Path,
        project_ref: Option<&'a str>,
        checkout: Option<&'a Path>,
        run_dir: &'a Path,
        name: &'a str,
    ) -> Self {
        Self {
            state_dir,
            project_ref,
            checkout,
            run_dir,
            cwd: run_dir,
            name,
            association: WorkspaceAssociation::Unbound,
            declared_env: empty_env(),
            from_host: &[],
            hermit_state: None,
            host_branch: None,
            branch_authority: None,
            host_read: &[],
            git_transport: false,
        }
    }

    /// The same scope, for host Git that fetches with the operator's
    /// selected transport.
    #[must_use]
    pub fn fetching(mut self) -> Self {
        self.git_transport = true;
        self
    }
}

fn empty_env() -> &'static BTreeMap<String, String> {
    static EMPTY: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(BTreeMap::new)
}

/// What a host command runs inside.
#[derive(Debug, Clone)]
pub enum HostLaunchPlan {
    /// A prepared, verified boundary.
    Bounded(BoundedLaunch),
    /// No backend exists on this platform; disclosed, never described as
    /// bounded.
    Unenforced {
        /// Stable reason code.
        reason: &'static str,
    },
}

impl HostLaunchPlan {
    /// The launch state `host_command::spawn` takes.
    pub(crate) fn launch(&self) -> crate::host_command::HostLaunch<'_> {
        match self {
            Self::Bounded(launch) => crate::host_command::HostLaunch::Bounded(launch),
            Self::Unenforced { reason } => crate::host_command::HostLaunch::Unenforced { reason },
        }
    }

    /// `git <args>` in `dir`, as a blocking command: inside the boundary when
    /// one is prepared, otherwise the plain host command with hooks and
    /// fsmonitor off (disclosed as not enforced by the caller's plan).
    #[must_use]
    pub fn git_command(&self, dir: &Path, args: &[&str]) -> std::process::Command {
        match self {
            Self::Bounded(_) => self.program_command(dir, "git", args),
            Self::Unenforced { .. } => {
                let mut command = crate::host_command::metadata_git_command(dir);
                command.args(args);
                command
            }
        }
    }

    /// Any program in `dir`, as a blocking command, inside the boundary when
    /// one is prepared.
    ///
    /// The general form of [`HostLaunchPlan::git_command`], for the host work
    /// that is not git: a project's own setup recipe, say, which must run
    /// inside the new tree's boundary rather than beside it. Blocking on
    /// purpose — the desktop's worktree creation is synchronous inside a
    /// blocking task, and giving it an async-only path would mean a second way
    /// to run a host command.
    ///
    /// Where no backend exists this is an ordinary host command. That is not a
    /// silent downgrade: the plan says `Unenforced` with its reason, and every
    /// caller records that in what it discloses.
    #[must_use]
    pub fn program_command(
        &self,
        dir: &Path,
        program: &str,
        args: &[&str],
    ) -> std::process::Command {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        match self {
            Self::Bounded(launch) => {
                let (wrapped, argv) = launch.boundary().wrap(program, &args);
                let mut command = std::process::Command::new(wrapped);
                command.args(argv).current_dir(dir);
                launch.env().apply_to_std(&mut command);
                command
            }
            Self::Unenforced { .. } => {
                let mut command = std::process::Command::new(program);
                command.args(args).current_dir(dir);
                command
            }
        }
    }
}

/// Prepare the launch for one host command.
///
/// # Errors
/// A refusal naming what prevents a bounded run where a backend exists.
pub fn prepare_host_command(
    scope: &HostCommandScope<'_>,
) -> Result<HostLaunchPlan, PrepareRefusal> {
    let mut project_env: Vec<(String, String, EnvSource)> = scope
        .declared_env
        .iter()
        .map(|(name, value)| (name.clone(), value.clone(), EnvSource::Project))
        .collect();
    project_env.extend(
        scope
            .from_host
            .iter()
            .map(|(name, value)| (name.clone(), value.clone(), EnvSource::ProjectFromHost)),
    );
    let key = hex::encode(Sha256::digest(
        format!(
            "{}\n{}\n{}",
            scope.project_ref.unwrap_or_default(),
            scope.run_dir.display(),
            scope.name
        )
        .as_bytes(),
    ));
    let owner = format!("host-{}", &key[..24]);
    let mut inputs = ScopeInputs::new(
        ScopePurpose::HostCommand,
        scope.state_dir,
        &owner,
        scope.cwd,
    );
    inputs.project_ref = scope.project_ref;
    inputs.project_checkout = scope.checkout;
    inputs.association = scope.association;
    inputs.project_env = &project_env;
    inputs.hermit_state = scope.hermit_state;
    inputs.host_branch = scope.host_branch;
    inputs.branch_authority = scope.branch_authority;
    inputs.host_read = scope.host_read;
    inputs.operator_git_auth = scope.git_transport;
    if !scope.cwd.starts_with(scope.run_dir) {
        return Err(PrepareRefusal {
            code: crate::execution_scope::EXECUTION_SCOPE_INVALID.to_owned(),
            message: "the command's working directory is outside its tree".to_owned(),
        });
    }
    // One preparation per scope directory at a time: two at once collide in
    // its self-test (`.boundary-probe: File exists`), which would refuse a
    // tree that can be bounded.
    let scope_lock = scope_dir_lock(scope.state_dir, &owner);
    let _serialized = scope_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let prepared = prepare(&inputs);
    drop(_serialized);
    release_scope_dir_lock(scope.state_dir, &owner, scope_lock);
    match prepared {
        Ok(ExecutionPlan::Prepared(prepared)) => Ok(HostLaunchPlan::Bounded(prepared.launch)),
        Ok(ExecutionPlan::Legacy { reason }) => Ok(HostLaunchPlan::Unenforced { reason }),
        Err(failure) => Err(PrepareRefusal {
            code: if failure.code == EXECUTION_BOUNDARY_UNAVAILABLE {
                ACTION_BOUNDARY_UNAVAILABLE
            } else {
                failure.code
            }
            .to_owned(),
            message: failure.message,
        }),
    }
}

type ScopeDirKey = (PathBuf, String);
type ScopeDirLocks = BTreeMap<ScopeDirKey, Arc<Mutex<()>>>;

/// Per-process locks serializing [`prepare`] for one scope directory, keyed
/// by the state dir and the scope's owner (which together name the
/// directory). An entry lives only while some preparation holds it.
fn scope_dir_locks() -> &'static Mutex<ScopeDirLocks> {
    static LOCKS: OnceLock<Mutex<ScopeDirLocks>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn scope_dir_lock(state_dir: &Path, owner: &str) -> Arc<Mutex<()>> {
    let mut locks = scope_dir_locks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Arc::clone(
        locks
            .entry((state_dir.to_path_buf(), owner.to_owned()))
            .or_default(),
    )
}

/// Drop this caller's handle, and the map's entry when no one else holds it.
fn release_scope_dir_lock(state_dir: &Path, owner: &str, lock: Arc<Mutex<()>>) {
    let mut locks = scope_dir_locks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    drop(lock);
    let key = (state_dir.to_path_buf(), owner.to_owned());
    if locks
        .get(&key)
        .is_some_and(|held| Arc::strong_count(held) == 1)
    {
        locks.remove(&key);
    }
}

/// Owned facts for host Git in one session's tree, so the preparation (which
/// starts the boundary's self-test processes) can run off the caller's
/// thread.
#[derive(Debug, Clone)]
pub struct HostGitRequest {
    /// Provider state dir.
    pub state_dir: PathBuf,
    /// The session's project, when it has one.
    pub project_ref: Option<String>,
    /// The project's recorded checkout, when the host has one.
    pub checkout: Option<PathBuf>,
    /// The session's working tree.
    pub tree: PathBuf,
    /// How the host ties the tree to the project.
    pub association: WorkspaceAssociation,
}

impl HostGitRequest {
    /// Prepare the plan, or `None` (logged) when the tree cannot be bounded:
    /// the caller then reports what it could not observe rather than run the
    /// tree's Git unbounded.
    pub async fn prepare(self) -> Option<HostLaunchPlan> {
        tokio::task::spawn_blocking(move || {
            let mut scope = HostCommandScope::git(
                &self.state_dir,
                self.project_ref.as_deref(),
                self.checkout.as_deref(),
                &self.tree,
                "probe",
            );
            scope.association = self.association;
            prepare_host_command(&scope).map_err(|refusal| {
                tracing::warn!(
                    target: "csp::scope",
                    tree = %self.tree.display(),
                    code = %refusal.code,
                    "host Git in this tree was not run: {}",
                    refusal.message
                );
            })
        })
        .await
        .ok()?
        .ok()
    }
}

/// Split a prepared step's environment into what it declared literally and
/// what it named from this host (`env_from_host`), preserving the existing
/// action contract.
pub(crate) fn split_declared_env(
    env: &BTreeMap<String, String>,
    from_host_names: &[String],
) -> (BTreeMap<String, String>, Vec<(String, String)>) {
    let mut declared = BTreeMap::new();
    let mut from_host = Vec::new();
    for (name, value) in env {
        if from_host_names.contains(name) {
            from_host.push((name.clone(), value.clone()));
        } else {
            declared.insert(name.clone(), value.clone());
        }
    }
    (declared, from_host)
}

/// This host's Hermit state location.
#[must_use]
pub fn host_hermit_state() -> Option<PathBuf> {
    std::env::var_os("HERMIT_STATE_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home_dir().map(|home| home.join("Library/Caches/hermit")))
}

/// The state directory the desktop host uses for its own bounded Git in
/// project workspaces (worktree cut, seat restage, establishment), beside the
/// providers' own under `<app data>/session-provider/`.
#[must_use]
pub fn desktop_host_state_dir(app_data: &Path) -> PathBuf {
    app_data
        .join(crate::agent_fence::SESSION_PROVIDER_DIR)
        .join("host")
}

/// The explicit test-only plan for unit tests of probe and record semantics,
/// where the boundary itself is exercised elsewhere.
#[cfg(test)]
pub(crate) const UNBOUNDED_FOR_TESTS: HostLaunchPlan = HostLaunchPlan::Unenforced {
    reason: "unit-test",
};

/// A disposable host state dir, one per test process, for tests whose host
/// Git now runs inside a prepared boundary.
#[cfg(test)]
pub(crate) fn test_state_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("beekeeper-csp-host-tests-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.canonicalize().unwrap_or(dir)
    })
    .clone()
}

// Every case here needs the macOS boundary backend.
#[cfg(all(test, target_os = "macos"))]
#[path = "execution_scope_host_tests.rs"]
mod tests;
