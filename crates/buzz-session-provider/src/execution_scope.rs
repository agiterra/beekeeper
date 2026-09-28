//! One prepared execution scope per launch: what a project execution may
//! read, write and inherit, derived by the host from the session's own record
//! before the adapter exists.
//!
//! The provider already knows everything that decides a project execution's
//! rights: the project coordinate and seat on the [`crate::state::SessionRecord`],
//! the seat's worktree, its agents clone and access, its role bundle, and the
//! runtime. This module turns those facts — never a path the child supplied —
//! into a [`PreparedExecution`]:
//!
//! * a [`buzz_acp::exec_boundary::PreparedBoundary`] (macOS) whose complete
//!   read policy grants the execution's own files, a verified private Git
//!   layout ([`crate::execution_scope_git`]), runtime code, the runtime state
//!   the login needs ([`crate::execution_scope_runtime`]), scoped dependency
//!   caches and execution-private state — and nothing else: not the host's
//!   shared pack and project caches, not other projects, not the operator's
//!   Claude or Codex configuration, history or memory;
//! * a [`buzz_acp::exec_env::ResolvedEnv`] built by source rather than
//!   inherited;
//! * an [`ExecutionBinding`] — the project, seat and rights digest the
//!   session record keeps, so native history is only ever reattached to the
//!   execution scope that wrote it.
//!
//! Failure is a refusal with a precise code, before any model work. There is
//! no unbounded fallback on a platform that has a backend.

use std::path::{Path, PathBuf};

use buzz_acp::acp::BoundedLaunch;
use buzz_acp::exec_boundary::{self, Access, BoundaryError, BoundarySpec, Grant};
use buzz_acp::exec_env::{EnvSource, ModelAuth, ResolvedEnv};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent_fence::FENCE;
use crate::session::CreateFailure;

/// Receipt code: the promised boundary could not be put in place.
pub const EXECUTION_BOUNDARY_UNAVAILABLE: &str = "EXECUTION_BOUNDARY_UNAVAILABLE";
/// Receipt code: the execution's working directory cannot be bounded.
pub const EXECUTION_SCOPE_INVALID: &str = "EXECUTION_SCOPE_INVALID";

/// Native-history refusal: the record predates project binding.
pub const NATIVE_HISTORY_UNBOUND: &str =
    "native history predates project binding, so it was not reattached";
/// Native-history refusal: project, seat or rights differ from the writer's.
pub const NATIVE_HISTORY_FOREIGN: &str =
    "native history belongs to a different project, seat or rights scope, so it was not reattached";

/// Scope fields every digest covers. Bumped when the digest's meaning changes.
const SCOPE_VERSION: u32 = 1;

/// Directory under the provider state dir holding each execution's private
/// temp and runtime state.
pub(crate) const EXECUTIONS_DIR: &str = "executions";
/// Directory under the provider state dir holding the content-addressed
/// policies. A sibling of [`EXECUTIONS_DIR`], never inside any grant.
pub(crate) const BOUNDARIES_DIR: &str = "boundaries";
/// Directory under the provider state dir holding each project scope's own
/// dependency caches.
pub(crate) const SCOPE_CACHES_DIR: &str = "scope-caches";
/// Directory under the provider state dir holding each runtime's empty
/// model-discovery working directory.
pub(crate) const DISCOVERY_DIR: &str = "discovery";

/// The project, seat and rights a session's native history belongs to.
///
/// Stored on the session record. Wake-specific data (turn text, OAuth
/// refreshes, a new package id, the session or generation id) is not part of
/// it, so a routine wake never forces a new conversation; a change of
/// project, seat, working tree, agents access or runtime does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionBinding {
    /// SHA-256 over the scope facts (hex).
    pub scope_digest: String,
    /// The project coordinate the scope was bound for, or `None` for an
    /// execution with no project.
    pub project_ref: Option<String>,
    /// The seat, or `None` for an unseated execution.
    pub actor: Option<String>,
    /// The canonical working tree the scope was granted. A continuation whose
    /// tree now resolves anywhere else is refused, never granted the new
    /// target: the child could have replaced its own tree with a link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<PathBuf>,
    /// The canonical agents clone the scope was granted, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<PathBuf>,
    /// The working tree's branch, pinned when the host first prepared the
    /// tree. Continuations grant this branch's ref, never whatever the child
    /// left in `HEAD`; the briefing and work brief name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Whether the agents clone above was granted for writing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agents_writable: bool,
}

/// Whether an enforced boundary surrounds the execution, disclosed as such.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryState {
    /// The adapter and every descendant run inside a verified boundary.
    Enforced {
        /// Backend identifier, e.g. `macos-seatbelt`.
        backend: &'static str,
        /// Digest of the exact policy enforced.
        policy_digest: String,
    },
    /// No backend exists on this platform; nothing is enforced and the
    /// execution must never be described as protected.
    NotEnforced {
        /// Stable reason code.
        reason: &'static str,
    },
}

/// What boundary a generation ran inside, as recorded on the host and
/// disclosed in the transcript. No paths and no values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedBoundary {
    /// `true` when a verified boundary surrounded the whole process tree.
    pub enforced: bool,
    /// Backend identifier when enforced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Digest of the exact policy enforced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_digest: Option<String>,
    /// Why nothing was enforced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The recorded form of a boundary state.
#[must_use]
pub fn recorded_state(state: &BoundaryState) -> Option<RecordedBoundary> {
    Some(match state {
        BoundaryState::Enforced {
            backend,
            policy_digest,
        } => RecordedBoundary {
            enforced: true,
            backend: Some((*backend).to_owned()),
            policy_digest: Some(policy_digest.clone()),
            reason: None,
        },
        BoundaryState::NotEnforced { reason } => RecordedBoundary {
            enforced: false,
            backend: None,
            policy_digest: None,
            reason: Some((*reason).to_owned()),
        },
    })
}

/// Transcript status slug disclosing an enforced project boundary.
pub const STATUS_BOUNDARY_ENFORCED: &str = "execution_boundary_enforced";
/// Transcript status slug disclosing that no boundary is enforced.
pub const STATUS_BOUNDARY_NOT_ENFORCED: &str = "execution_boundary_not_enforced";

/// The transcript status item disclosing what this generation runs inside.
///
/// Status items are additive by contract: a reader that does not know the
/// slug renders a generic status row rather than dropping the event, so this
/// needs no change to the exact-key 44223 metadata.
#[must_use]
pub fn boundary_status_item(state: &BoundaryState) -> serde_json::Value {
    match state {
        BoundaryState::Enforced { backend, .. } => serde_json::json!({
            "kind": "status",
            "status": STATUS_BOUNDARY_ENFORCED,
            "reason": backend,
        }),
        BoundaryState::NotEnforced { reason } => serde_json::json!({
            "kind": "status",
            "status": STATUS_BOUNDARY_NOT_ENFORCED,
            "reason": reason,
        }),
    }
}

/// Where the runtime's native history lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeHistory {
    /// In the execution's own host-owned state directory, keyed by session
    /// and scope: no other execution or project can read or write it.
    Private,
    /// Test doubles keep no history.
    NotApplicable,
}

/// The runtime a launch runs, decided by the driver the host resolved — never
/// by the name of an executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProfile {
    /// Claude Code through claude-agent-acp.
    Claude,
    /// Codex through codex-acp (candidate A).
    Codex,
    /// A runtime whose state and context requirements have not been verified
    /// under the boundary. Refused where a backend exists.
    Unsupported,
    /// No model runtime: a host-run project command, which needs the
    /// project's tools and configuration but no model login.
    NoModel,
    /// A shell test double with no runtime state. Test builds only.
    #[cfg(test)]
    TestDouble,
}

impl RuntimeProfile {
    /// The profile for a driver slug.
    #[must_use]
    pub fn for_driver(driver: &str) -> Self {
        match driver {
            crate::agent_fence::CLAUDE_DRIVER => Self::Claude,
            "codex-acp" => Self::Codex,
            _ => Self::Unsupported,
        }
    }

    fn model_auth(self) -> ModelAuth {
        match self {
            Self::Claude => ModelAuth::Claude,
            Self::Codex => ModelAuth::Codex,
            Self::Unsupported | Self::NoModel => ModelAuth::None,
            #[cfg(test)]
            Self::TestDouble => ModelAuth::None,
        }
    }
}

/// Everything a launch needs, prepared and verified by the host.
#[derive(Clone)]
pub struct PreparedExecution {
    /// Boundary, environment and working directory.
    pub launch: BoundedLaunch,
    /// The binding to record for this generation.
    pub binding: ExecutionBinding,
    /// Native-history ownership.
    pub native_history: NativeHistory,
    /// Why a recorded native cursor must not be reattached, when it must not.
    pub native_refusal: Option<&'static str>,
    /// The runtime the scope was prepared for.
    pub runtime: RuntimeProfile,
    /// The claude-agent-acp session options (`_meta.claudeCode.options`) a
    /// Claude execution opens with; `None` for every other runtime.
    pub claude_options: Option<serde_json::Map<String, serde_json::Value>>,
    /// The project's agents repository this execution reads, and whether it
    /// may write there.
    pub agents: Option<(PathBuf, bool)>,
}

impl PreparedExecution {
    /// What this execution runs inside — read from the verified boundary it
    /// launches through, so it can never disagree with the launch.
    #[must_use]
    pub fn state(&self) -> BoundaryState {
        let boundary = self.launch.boundary();
        BoundaryState::Enforced {
            backend: boundary.backend(),
            policy_digest: boundary.digest().to_owned(),
        }
    }
}

// The launch holds host paths and environment values; diagnostics get the
// enforcement state, the digests, and names with sources.
impl std::fmt::Debug for PreparedExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedExecution")
            .field("state", &self.state())
            .field("scope_digest", &self.binding.scope_digest)
            .field("boundary", self.launch.boundary())
            .field("env", self.launch.env())
            .field("native_history", &self.native_history)
            .field("native_refusal", &self.native_refusal)
            .field("runtime", &self.runtime)
            .field(
                "agents",
                &self.agents.as_ref().map(|(_, writable)| writable),
            )
            .finish_non_exhaustive()
    }
}

/// How a launch is carried out.
#[derive(Debug, Clone)]
pub enum ExecutionPlan {
    /// A host-prepared scope. The only plan production mints on a platform
    /// with a backend.
    Prepared(Box<PreparedExecution>),
    /// The existing inherited-environment spawn, used only where no backend
    /// exists (disclosed as not enforced) and by unit tests of unrelated
    /// behaviour.
    Legacy {
        /// Stable reason code, disclosed.
        reason: &'static str,
    },
}

impl ExecutionPlan {
    /// The disclosed boundary state.
    #[must_use]
    pub fn state(&self) -> BoundaryState {
        match self {
            Self::Prepared(prepared) => prepared.state(),
            Self::Legacy { reason } => BoundaryState::NotEnforced { reason },
        }
    }

    /// The binding to record, when one was prepared.
    #[must_use]
    pub fn binding(&self) -> Option<&ExecutionBinding> {
        match self {
            Self::Prepared(prepared) => Some(&prepared.binding),
            Self::Legacy { .. } => None,
        }
    }

    /// Why a native cursor must not be reattached.
    #[must_use]
    pub fn native_refusal(&self) -> Option<&'static str> {
        match self {
            Self::Prepared(prepared) => prepared.native_refusal,
            Self::Legacy { .. } => None,
        }
    }
}

/// What a scope is for. One preparation serves all three; the purpose only
/// decides which capabilities are added on top of the project's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopePurpose {
    /// A coding session's model runtime: the adapter and every process it
    /// starts, working in a project workspace.
    Session,
    /// Startup model discovery: no project, no model turn. Its working
    /// directory is an empty host-owned directory under the provider's state,
    /// so the adapter's `session/new` opens no conversation anywhere a
    /// project or the operator's own history lives.
    Discovery,
    /// A host-run project command: an action step (verify, build, test), or
    /// host Git that can run the project's own code (checkout, status,
    /// fetch) in a project workspace. The project's tools and configuration,
    /// no model runtime and no model login.
    HostCommand,
}

/// How the host ties a working tree to the launch's project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAssociation {
    /// No host record binds the directory to the project: it is accepted as
    /// the project's workspace only when it *is* the project's recorded
    /// checkout or a linked worktree of it. A channel default or a hint that
    /// predates project binding lands here.
    Unbound,
    /// A host record binds this directory to this project: a create hint that
    /// names the project, a session record whose binding was validated when it
    /// was created, or a directory the host itself staged for the project
    /// (an action worktree, a setup draft, an agents clone).
    HostBound,
}

/// The facts a scope is derived from. All come from the host's own record
/// and custody, never from the child.
#[derive(Debug, Clone)]
pub struct ScopeInputs<'a> {
    /// What the scope is for.
    pub purpose: ScopePurpose,
    /// Provider state dir (`BUZZ_CSP_STATE_DIR`).
    pub state_dir: &'a Path,
    /// Names the execution's host-owned directory: the session id, or a
    /// host command's own stable name.
    pub session_id: &'a str,
    /// Project coordinate from the record, if any.
    pub project_ref: Option<&'a str>,
    /// The project's recorded checkout, when the host has one.
    pub project_checkout: Option<&'a Path>,
    /// How the host ties the working tree to `project_ref`.
    pub association: WorkspaceAssociation,
    /// Seat pubkey, if seated.
    pub actor: Option<&'a str>,
    /// Driver slug, e.g. `claude-agent-acp`.
    pub driver: &'a str,
    /// The runtime, decided from the driver.
    pub runtime: RuntimeProfile,
    /// Working directory.
    pub cwd: &'a Path,
    /// ACP adapter command.
    pub agent_command: &'a str,
    /// Adapter argv after the command, from the host's runtime descriptor.
    pub agent_args: &'a [String],
    /// Descriptor `cli_env` (e.g. `CLAUDE_CODE_EXECUTABLE`).
    pub agent_env: &'a [(String, String)],
    /// Installation roots of the runtime the host resolved, read-only, beside
    /// the Beekeeper-managed runtimes (`runtime_install_grants`).
    pub runtime_roots: &'a [PathBuf],
    /// Seat identity variables, applied last.
    pub identity_env: &'a [(String, String)],
    /// What this project's own authorized configuration declares for a host
    /// command (an action step's `env` and `env_from_host`), with its source.
    pub project_env: &'a [(String, String, EnvSource)],
    /// The project's agents repository clone this execution reads, and
    /// whether it may write there: a seat's own clone, or the read-only clone
    /// the host staged for an unseated execution.
    pub agents_checkout: Option<(&'a Path, bool)>,
    /// The seat's role bundle directory (created by the host if it does not
    /// exist yet: the skills are materialized into it before the spawn).
    pub seat_bundle: Option<&'a Path>,
    /// The pinned role contract the seat runs: role, persona, pack and
    /// composition revision, from the seat's own custody. Part of the scope
    /// digest, so a restaged role is never combined with the old role's
    /// native conversation.
    pub role_contract: Option<&'a str>,
    /// Context-MCP executable and package directory, when attached.
    pub context_mcp: Option<(&'a Path, &'a Path)>,
    /// The `bee` the host chose for this execution.
    pub seat_bee: Option<&'a Path>,
    /// Hermit's package state on this host, used only when the working tree
    /// declares Hermit packages (`bin/hermit` and `bin/.<pkg>.pkg`).
    pub hermit_state: Option<&'a Path>,
    /// A branch a host command (never the child) moves this worktree to.
    pub host_branch: Option<&'a str>,
    /// Host-owned paths a host command reads (the host's own clone a fetch
    /// takes objects from). Never granted to a session.
    pub host_read: &'a [PathBuf],
    /// Whether this execution authenticates Git with the operator's own
    /// selected transport (the staged `nostr.keyfile`): an unseated session,
    /// or host Git that fetches for the project. Never a seat, which carries
    /// its own key, and never a host command that runs project code.
    pub operator_git_auth: bool,
    /// The record's previous binding and native cursor, on a resume/restore.
    pub prior: Option<(Option<&'a ExecutionBinding>, Option<&'a str>)>,
    /// The host environment the baseline is resolved from. `None` (always,
    /// in production) reads this process's own environment; a test names
    /// one so it can prove what an operator-level value does and does not
    /// reach, without mutating the test process.
    pub ambient_env: Option<&'a [(std::ffi::OsString, std::ffi::OsString)]>,
}

impl<'a> ScopeInputs<'a> {
    /// Inputs with no project, seat, runtime or extra capability: the caller
    /// sets what applies.
    #[must_use]
    pub fn new(
        purpose: ScopePurpose,
        state_dir: &'a Path,
        session_id: &'a str,
        cwd: &'a Path,
    ) -> Self {
        Self {
            purpose,
            state_dir,
            session_id,
            project_ref: None,
            project_checkout: None,
            association: WorkspaceAssociation::Unbound,
            actor: None,
            driver: "",
            runtime: RuntimeProfile::NoModel,
            cwd,
            agent_command: "",
            agent_args: &[],
            agent_env: &[],
            runtime_roots: &[],
            identity_env: &[],
            project_env: &[],
            agents_checkout: None,
            seat_bundle: None,
            role_contract: None,
            context_mcp: None,
            seat_bee: None,
            hermit_state: None,
            host_branch: None,
            host_read: &[],
            operator_git_auth: false,
            prior: None,
            ambient_env: None,
        }
    }
}

pub(crate) fn refuse(code: &'static str, message: impl Into<String>) -> CreateFailure {
    CreateFailure {
        code,
        message: message.into(),
    }
}

pub(crate) fn canonical(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

/// Where `path` resolves once it exists: its nearest existing ancestor,
/// canonical, with the remaining components appended. `None` for a relative
/// path or one whose missing part climbs with `..`.
pub(crate) fn prospective(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    if let Some(real) = canonical(path) {
        return Some(real);
    }
    let mut missing = Vec::new();
    let mut ancestor = path;
    loop {
        let name = ancestor.file_name()?;
        if name == ".." {
            return None;
        }
        missing.push(name.to_os_string());
        ancestor = ancestor.parent()?;
        if let Some(real) = canonical(ancestor) {
            let mut full = real;
            for part in missing.iter().rev() {
                full.push(part);
            }
            return Some(full);
        }
    }
}

/// The operator's home, canonical.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .and_then(|home| canonical(&home))
}

/// Prepare the scope for one launch.
///
/// # Errors
/// A [`CreateFailure`] naming the fact that prevents enforcement. On a
/// platform with no backend this does **not** fail: it returns
/// [`ExecutionPlan::Legacy`] with a disclosed reason, and the launch keeps the
/// existing spawn rather than a Unix environment built for macOS.
pub fn prepare(inputs: &ScopeInputs<'_>) -> Result<ExecutionPlan, CreateFailure> {
    if !cfg!(target_os = "macos") {
        return Ok(ExecutionPlan::Legacy {
            reason: "no-backend-for-platform",
        });
    }
    let model = inputs.purpose != ScopePurpose::HostCommand;
    if model
        && matches!(
            inputs.runtime,
            RuntimeProfile::Unsupported | RuntimeProfile::NoModel
        )
    {
        return Err(refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!(
                "the {} runtime has not been verified to run inside the project execution \
                 boundary on this Mac, so this project execution was not started. Use a Claude \
                 or Codex runtime for project work.",
                inputs.driver
            ),
        ));
    }
    let home = home_dir().ok_or_else(|| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            "HOME is not set, so the operator's runtime locations cannot be named",
        )
    })?;
    let cwd = canonical(inputs.cwd).ok_or_else(|| {
        refuse(
            EXECUTION_SCOPE_INVALID,
            format!(
                "the working directory {} does not exist",
                inputs.cwd.display()
            ),
        )
    })?;
    let state_dir = canonical(inputs.state_dir).ok_or_else(|| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            "the provider state directory does not exist",
        )
    })?;

    // The project workspace, and its tie to the project.
    let tree = match inputs.purpose {
        ScopePurpose::Discovery => {
            if !cwd.starts_with(state_dir.join(DISCOVERY_DIR)) {
                return Err(refuse(
                    EXECUTION_SCOPE_INVALID,
                    "model discovery runs only in its host-owned directory",
                ));
            }
            cwd.clone()
        }
        ScopePurpose::Session | ScopePurpose::HostCommand => {
            let tree = git_toplevel(&cwd).unwrap_or_else(|| cwd.clone());
            validate_workspace_placement(&tree, inputs.association, &home, &state_dir)?;
            tree
        }
    };
    let mut grants: Vec<Grant> = Vec::new();
    grants.push(Grant::tree(
        &tree,
        Access::ReadWrite,
        "this execution's working tree",
    ));
    // The tree's own entry stays where the host found it: the child works
    // inside it, but cannot remove it and leave a link to somewhere else.
    grants.push(Grant::file(
        &tree,
        Access::NoUnlink,
        "the working tree's own anchor",
    ));
    // The branch is pinned by the host when it first prepares this tree —
    // before any child has run in it — and every later preparation of the
    // tree, for any purpose, grants that branch, never the child-writable
    // `HEAD`.
    let branch = pinned_branch(
        &state_dir,
        &tree,
        inputs
            .prior
            .and_then(|(prior, _)| prior)
            .filter(|prior| prior.tree.as_deref() == Some(tree.as_path()))
            .and_then(|prior| prior.branch.as_deref()),
    );
    let linked = crate::execution_scope_git::linked_worktree_admin(
        &tree,
        inputs.project_checkout,
        branch.as_deref(),
        inputs.host_branch,
    )?;
    if inputs.purpose != ScopePurpose::Discovery {
        check_association(inputs, &tree, linked.is_some())?;
    }
    grants.extend(linked.unwrap_or_default());

    // The project's own context: its agents repository, the seat's role
    // bundle and the host's context service.
    let mut agents_digest = String::new();
    let mut agents = None;
    if let (ScopePurpose::Session, Some((path, writable))) =
        (inputs.purpose, inputs.agents_checkout)
    {
        let path = canonical(path).ok_or_else(|| {
            refuse(
                EXECUTION_SCOPE_INVALID,
                format!(
                    "the project's agents clone {} does not exist; it must be prepared before \
                     the session starts",
                    path.display()
                ),
            )
        })?;
        if !path.join(".git").is_dir()
            || crate::execution_scope_git::linked_worktree_admin(&path, None, None, None)?.is_some()
        {
            return Err(refuse(
                EXECUTION_SCOPE_INVALID,
                "the project's agents clone is not a self-contained clone",
            ));
        }
        validate_workspace_placement(&path, WorkspaceAssociation::HostBound, &home, &state_dir)?;
        let access = if writable {
            Access::ReadWrite
        } else {
            Access::ReadOnly
        };
        agents_digest = format!("{}|{}", path.display(), access.as_str());
        grants.push(Grant::tree(
            &path,
            access,
            "this project's agents repository clone",
        ));
        grants.push(Grant::file(
            &path,
            Access::NoUnlink,
            "the agents clone's own anchor",
        ));
        agents = Some((path, writable));
    }
    check_continuation(
        inputs,
        &tree,
        agents.as_ref().map(|(path, _)| path.as_path()),
    )?;
    if inputs.purpose == ScopePurpose::Session {
        if let Some(bundle) = inputs.seat_bundle {
            grants.push(Grant::tree(
                prepare_seat_bundle(bundle, &state_dir, &tree)?,
                Access::ReadOnly,
                "this seat's role bundle",
            ));
        }
        if let Some((command, package_dir)) = inputs.context_mcp {
            if let Some(command) = canonical(command) {
                grants.push(Grant::file(
                    command,
                    Access::ReadOnly,
                    "context MCP executable",
                ));
            }
            if let Some(dir) = canonical(package_dir) {
                grants.push(Grant::tree(
                    dir,
                    Access::ReadOnly,
                    "this execution's context package",
                ));
            }
        }
    }
    if inputs.purpose == ScopePurpose::HostCommand {
        for path in inputs.host_read.iter().filter_map(|path| canonical(path)) {
            grants.push(Grant::tree(
                path,
                Access::ReadOnly,
                "a host-owned source this command reads",
            ));
        }
    }

    // The execution's own directories: host control records the child reads
    // at most, and runtime state it owns. A changed project, seat, tree,
    // agents access or role contract gets new ones, never the old history.
    let digest = scope_digest(inputs, &tree, &agents_digest);
    let dirs = claim_owner_dir(&state_dir, inputs.session_id, &digest)?;
    let temp = host_dir(&dirs.state, "tmp")?;
    let xdg = host_dir(&dirs.state, "xdg")?;
    grants.push(Grant::tree(
        &dirs.state,
        Access::ReadWrite,
        "execution-private temp and runtime state",
    ));
    grants.push(Grant::file(
        &dirs.state,
        Access::NoUnlink,
        "the execution state's own anchor",
    ));
    for anchor in [&temp, &xdg] {
        grants.push(Grant::file(
            anchor,
            Access::NoUnlink,
            "a runtime directory's own anchor",
        ));
    }

    // The host's own tools, found by name: a host-owned directory holding a
    // link to each, readable as a directory without lending the directory the
    // tool was installed in.
    // Always staged: the `mktemp` that keeps ordinary temporary files in this
    // execution's private temp (see `mktemp_shim.sh`), for sessions and host
    // commands alike.
    let bin = host_dir(&dirs.control, "bin")?;
    stage_host_script(&bin, "mktemp", MKTEMP_SHIM)?;
    grants.push(Grant::tree(
        &bin,
        Access::ReadOnly,
        "the host's tools, by name",
    ));
    if let (ScopePurpose::Session, Some(bee)) = (inputs.purpose, inputs.seat_bee) {
        link_tool(&bin, bee, "bee")?;
        grants.extend(executable_grants(bee, "the bee CLI this host chose"));
    }
    let tool_dir = Some(bin);

    // Git transport: the operator's selected identity and credential
    // helpers, staged host-side as a read-only file.
    let operator_auth = inputs.operator_git_auth
        || (inputs.actor.is_none() && inputs.purpose == ScopePurpose::Session);
    let git = crate::execution_scope_git::stage_git_config(
        &dirs.control,
        operator_auth,
        inputs.seat_bee.and_then(Path::parent),
    )?;
    grants.push(Grant::file(
        &git.config,
        Access::ReadOnly,
        "this execution's Git configuration",
    ));
    for helper in &git.helpers {
        grants.extend(executable_grants(helper, "Git credential helper"));
    }
    if let Some(keyfile) = &git.keyfile {
        grants.push(Grant::file(
            keyfile,
            Access::ReadOnly,
            "the operator's own Git key, for an unseated session or a host fetch",
        ));
    }
    if model || !git.helpers.is_empty() {
        if let Some(login) = keychain_login_grant(&home, inputs.runtime, !git.helpers.is_empty()) {
            grants.push(login);
        }
    }

    // This project's own dependency caches: shared by its executions, never
    // by another project's.
    let cache_key = hex::encode(Sha256::digest(
        inputs
            .project_ref
            .map_or_else(|| tree.to_string_lossy().into_owned(), str::to_owned)
            .as_bytes(),
    ));
    let caches = state_dir.join(SCOPE_CACHES_DIR).join(&cache_key[..24]);
    std::fs::create_dir_all(&caches).map_err(|error| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not create the project's dependency cache: {error}"),
        )
    })?;
    grants.push(Grant::tree(
        &caches,
        Access::ReadWrite,
        "this project's own dependency caches",
    ));

    // Development services: runtime and toolchain installations.
    if model {
        grants.extend(runtime_install_grants(inputs, &home));
    }
    grants.extend(toolchain_grants(&home));
    let hermit_state = hermit_declared_grants(&tree, inputs.hermit_state, &mut grants);
    if let Some(prefix) = xcrun_cache_dir() {
        grants.push(Grant::scratch(
            prefix,
            "xcrun_db",
            Access::ReadWrite,
            "xcrun tool-lookup cache written by Apple's developer-tool shims",
        ));
    }
    if let Some(sock) = std::env::var_os("SSH_AUTH_SOCK")
        .map(PathBuf::from)
        .and_then(|p| canonical(&p))
    {
        grants.push(Grant::file(
            sock,
            Access::ReadWrite,
            "ssh agent socket for Git transport",
        ));
    }

    let binding = ExecutionBinding {
        scope_digest: digest.clone(),
        project_ref: inputs.project_ref.map(str::to_owned),
        actor: inputs.actor.map(str::to_owned),
        tree: Some(tree.clone()),
        agents: agents.as_ref().map(|(path, _)| path.clone()),
        branch,
        agents_writable: agents.as_ref().is_some_and(|(_, writable)| *writable),
    };
    let native_refusal = inputs.prior.and_then(|(prior, cursor)| {
        cursor?;
        match prior {
            None => Some(NATIVE_HISTORY_UNBOUND),
            Some(prior) if prior.scope_digest != digest => Some(NATIVE_HISTORY_FOREIGN),
            Some(_) => None,
        }
    });

    let mut codex_home = None;
    let mut claude = None;
    let native_history = match (model, inputs.runtime) {
        (true, RuntimeProfile::Claude) => {
            let executable = crate::execution_scope_runtime::claude_executable(inputs.agent_env)
                .ok_or_else(|| {
                    refuse(
                        EXECUTION_BOUNDARY_UNAVAILABLE,
                        "no Claude Code CLI is configured for this runtime, so its login cannot \
                         be checked inside the project boundary",
                    )
                })?;
            grants.extend(crate::execution_scope_runtime::claude_runtime_grants(
                &executable,
                &home,
            ));
            let config = host_dir(&dirs.state, "claude-config")?;
            grants.push(Grant::file(
                &config,
                Access::NoUnlink,
                "a runtime directory's own anchor",
            ));
            let settings = dirs.control.join(CLAUDE_SETTINGS_FILE);
            grants.push(Grant::file(
                &settings,
                Access::ReadOnly,
                "the host's pinned Claude settings for this execution",
            ));
            claude = Some((executable, config, settings));
            NativeHistory::Private
        }
        (true, RuntimeProfile::Codex) => {
            let prepared = crate::execution_scope_runtime::prepare_codex_home(
                &dirs.state,
                &crate::execution_scope_runtime::CodexLogin::from_host(&home, inputs.agent_env),
            )?;
            grants.extend(prepared.grants);
            codex_home = Some(prepared.home);
            NativeHistory::Private
        }
        _ => NativeHistory::NotApplicable,
    };

    let probe_readable = temp.join(".boundary-probe");
    write_host_file(&probe_readable, b"probe\n", false)?;
    let boundary = exec_boundary::prepare(BoundarySpec {
        grants,
        policy_dir: state_dir.join(BOUNDARIES_DIR),
        probe_readable,
    })
    .map_err(|error| boundary_failure(&error))?;

    let scope_env = ScopeEnv {
        temp: &temp,
        xdg: &xdg,
        git_config: &git.config,
        caches: &caches,
        codex_home: codex_home.as_deref(),
        claude_config: claude.as_ref().map(|(_, config, _)| config.as_path()),
        hermit_state: hermit_state.as_deref(),
    };
    let env = resolve_env(&boundary, inputs, &scope_env, tool_dir.as_deref())?;
    let mut claude_options = None;
    if let Some((executable, config, settings)) = &claude {
        crate::execution_scope_runtime::write_claude_settings(settings, &env)?;
        crate::execution_scope_runtime::claude_auth_preflight(
            &boundary, &env, &cwd, executable, config,
        )?;
        claude_options = Some(claude_session_options(settings, inputs.purpose));
    }
    tracing::info!(
        target: "csp::scope",
        session_id = %inputs.session_id,
        purpose = ?inputs.purpose,
        backend = boundary.backend(),
        policy = %boundary.digest(),
        scope = %digest,
        grants = boundary.grants().len(),
        env = ?env,
        native_history = ?native_history,
        native_refusal = ?native_refusal,
        "project execution boundary prepared"
    );
    Ok(ExecutionPlan::Prepared(Box::new(PreparedExecution {
        launch: BoundedLaunch::new(boundary, env, cwd),
        binding,
        native_history,
        native_refusal,
        runtime: inputs.runtime,
        claude_options,
        agents,
    })))
}

/// Refuse a launch whose working tree the host has not tied to its project.
///
/// A project-bound launch runs in the project's recorded checkout, a linked
/// worktree of it ([`crate::execution_scope_git::linked_worktree_admin`] has
/// already verified the repository), or a directory a host record binds to
/// this project. A channel default or an unbound hint naming some other
/// directory is never promoted into the project's workspace.
fn check_association(
    inputs: &ScopeInputs<'_>,
    tree: &Path,
    linked: bool,
) -> Result<(), CreateFailure> {
    let Some(project) = inputs.project_ref else {
        return Ok(());
    };
    if inputs.association == WorkspaceAssociation::HostBound {
        return Ok(());
    }
    let checkout = inputs.project_checkout.and_then(canonical);
    match checkout {
        Some(checkout) if checkout == tree || linked => Ok(()),
        _ => Err(refuse(
            EXECUTION_SCOPE_INVALID,
            format!(
                "{} is not project {project}'s recorded checkout or a worktree of it, and no \
                 host record binds it to that project, so it was not used as the project's \
                 workspace",
                tree.display()
            ),
        )),
    }
}

/// File name, in an execution's control directory, of the Claude settings
/// the host pins for it.
pub(crate) const CLAUDE_SETTINGS_FILE: &str = "claude-settings.json";

/// The claude-agent-acp session options a bounded Claude execution opens
/// with.
///
/// * `settingSources: ["project", "local"]` — the project's own settings,
///   instructions and `.mcp.json` servers load natively (measured through the
///   installed adapter: stdio and HTTP servers, the CLI's own `${VAR}`
///   interpolation); the operator's user-level settings, skills, agents,
///   hooks and plugins do not, and are unreadable under the boundary
///   regardless.
/// * `settings: <file>` — the host-written, child-read-only flag-tier file
///   ([`crate::execution_scope_runtime::write_claude_settings`]): account
///   connectors off, Claude's own Bash sandbox off (it cannot start inside the
///   host boundary, which already contains every process), and the host's
///   scope and identity values pinned above anything a project setting says.
/// * Discovery adds `strictMcpConfig`: it has no project whose servers belong
///   in it.
///
/// The host's own servers (the session context) ride on the ACP open and
/// replace a same-named project server.
#[must_use]
pub fn claude_session_options(
    settings: &Path,
    purpose: ScopePurpose,
) -> serde_json::Map<String, serde_json::Value> {
    let mut options = serde_json::Map::new();
    options.insert(
        "settingSources".to_owned(),
        serde_json::json!(["project", "local"]),
    );
    options.insert(
        "settings".to_owned(),
        serde_json::Value::String(settings.display().to_string()),
    );
    if purpose == ScopePurpose::Discovery {
        options.insert("strictMcpConfig".to_owned(), serde_json::json!(true));
    }
    options
}

/// The ACP mode a Codex execution must run in inside the boundary: Codex's
/// own command sandbox cannot start inside the host boundary (every command
/// fails `sandbox_apply`), so the host boundary is the enforcement. Asserted
/// only after the host boundary is verified.
pub const CODEX_BOUNDED_MODE: &str = "agent-full-access";

/// An execution's host-owned directories.
#[derive(Debug, Clone)]
pub(crate) struct ExecutionDirs {
    /// Host control records: the owner claim, the staged Git configuration,
    /// pinned runtime settings, the host's tool links. Never writable by the
    /// child; files the runtime must read get a literal read grant.
    pub control: PathBuf,
    /// The child's own runtime state: temp, configuration, native history,
    /// memory. Read-write for the child, nobody else's.
    pub state: PathBuf,
}

/// Claim the execution's host-owned directories for this session and scope.
///
/// `<state>/executions/<session>-<digest16>/` is created with an exclusive
/// `mkdir` and an `control/owner.json` written with `O_EXCL`; an existing
/// directory is reused only when its owner record names exactly this session
/// and scope digest. The owner record is directory-claim validation; which
/// native history a session may reattach is decided by the binding on the
/// host's session record, not by this file. Every directory is checked with
/// [`host_dir`]: a child that replaced its state directory with a link is
/// refused, never granted the link's target.
pub(crate) fn claim_owner_dir(
    state_dir: &Path,
    session_id: &str,
    digest: &str,
) -> Result<ExecutionDirs, CreateFailure> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(refuse(
            EXECUTION_SCOPE_INVALID,
            "the session id cannot name a private directory",
        ));
    }
    let unavailable = |what: &str, error: std::io::Error| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not {what} the execution's private directory: {error}"),
        )
    };
    let state_dir = canonical(state_dir).ok_or_else(|| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            "the provider state directory does not exist",
        )
    })?;
    let root = host_dir(&state_dir, EXECUTIONS_DIR)?;
    let name = format!("{session_id}-{}", &digest[..16]);
    let fresh = !root.join(&name).exists();
    let dir = host_dir(&root, &name)?;
    let control = host_dir(&dir, "control")?;
    let owner_file = control.join("owner.json");
    let owner = serde_json::json!({ "sessionId": session_id, "scopeDigest": digest }).to_string();
    if fresh {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&owner_file)
            .map_err(|error| unavailable("claim", error))?;
        std::io::Write::write_all(&mut file, owner.as_bytes())
            .map_err(|error| unavailable("claim", error))?;
    } else {
        let recorded = std::fs::read_to_string(&owner_file).unwrap_or_default();
        if recorded != owner {
            return Err(refuse(
                EXECUTION_BOUNDARY_UNAVAILABLE,
                "the execution's private directory is recorded for a different session or \
                 scope; refusing to reuse it",
            ));
        }
    }
    let state = host_dir(&dir, "state")?;
    Ok(ExecutionDirs { control, state })
}

/// A directory the host creates for an execution under `parent` (already
/// verified): created when absent; otherwise it must be a real directory —
/// not a link a child left in its place — whose canonical path is exactly
/// `parent/name`. Returns that path.
///
/// # Errors
/// The entry is a link, a file, or resolves anywhere else: the execution's
/// private state was replaced, and it is refused rather than followed.
pub(crate) fn host_dir(parent: &Path, name: &str) -> Result<PathBuf, CreateFailure> {
    let path = parent.join(name);
    match std::fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(refuse(
                EXECUTION_BOUNDARY_UNAVAILABLE,
                format!("could not create the execution's {name} directory: {error}"),
            ))
        }
    }
    let real_dir = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir());
    let expected = canonical(parent).map(|parent| parent.join(name));
    match (real_dir, canonical(&path)) {
        (true, Some(real)) if Some(&real) == expected.as_ref() => Ok(real),
        _ => Err(refuse(
            EXECUTION_SCOPE_INVALID,
            format!(
                "this execution's private {name} directory was replaced by something else, so \
                 it was not reused; start the session fresh"
            ),
        )),
    }
}

/// Write a host-owned file without following anything left at its path:
/// whatever is there is removed, and the file is created exclusively —
/// `private` files with mode 0600 from the moment they exist.
///
/// # Errors
/// The file could not be written.
pub(crate) fn write_host_file(
    path: &Path,
    contents: &[u8],
    private: bool,
) -> Result<(), CreateFailure> {
    let failed = |error: std::io::Error| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not write {}: {error}", path.display()),
        )
    };
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            return Err(refuse(
                EXECUTION_SCOPE_INVALID,
                format!(
                    "{} was replaced by a directory, so it was not written",
                    path.display()
                ),
            ))
        }
        Ok(_) => std::fs::remove_file(path).map_err(failed)?,
        Err(_) => {}
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path).map_err(failed)?;
    std::io::Write::write_all(&mut file, contents).map_err(failed)
}

/// Host-owned branch pins, one file per working tree, beside (never inside)
/// the executions' state.
const BRANCH_PINS_DIR: &str = "branch-pins";

/// The branch pinned for `tree`: the record's own, else the host's pin file,
/// else the tree's branch now — which is then pinned. The first preparation
/// of a tree happens before any child runs in it (a new worktree is cut by
/// the host); a tree first seen by this code after an upgrade is pinned from
/// its `HEAD` at that moment.
fn pinned_branch(state_dir: &Path, tree: &Path, recorded: Option<&str>) -> Option<String> {
    let dir = state_dir.join(BRANCH_PINS_DIR);
    let pin = dir.join(&hex::encode(Sha256::digest(tree.to_string_lossy().as_bytes()))[..32]);
    let read_pin = || {
        std::fs::read_to_string(&pin)
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|branch| !branch.is_empty())
    };
    let chosen = recorded
        .map(str::to_owned)
        .or_else(read_pin)
        .or_else(|| crate::execution_scope_git::worktree_branch(tree))?;
    if read_pin().is_none() && std::fs::create_dir_all(&dir).is_ok() {
        use std::io::Write as _;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pin)
        {
            Ok(mut file) => {
                let _ = file.write_all(chosen.as_bytes());
            }
            // Another preparation pinned it first: that pin decides.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return read_pin().or(Some(chosen));
            }
            Err(_) => {}
        }
    }
    Some(chosen)
}

/// The host's `mktemp` for an execution: routes macOS's default and `-t`
/// forms, which ignore `TMPDIR`, to the execution's private temp.
const MKTEMP_SHIM: &str = include_str!("mktemp_shim.sh");

/// Write a host-owned script into the execution's read-only tool directory,
/// replacing any earlier copy (a continuation re-stages the current one).
fn stage_host_script(bin: &Path, name: &str, body: &str) -> Result<(), CreateFailure> {
    let path = bin.join(name);
    let failed = |error: std::io::Error| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not stage the host's {name}: {error}"),
        )
    };
    // Already current: leave it, so a running execution never sees it change.
    if std::fs::read(&path).is_ok_and(|bytes| bytes == body.as_bytes()) {
        return Ok(());
    }
    // Written under a name no other preparation uses and renamed into place:
    // an execution sharing this directory finds the old script or the new
    // one, never a missing or half-written one.
    static STAGING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let building = bin.join(format!(
        ".{name}-{}-{}",
        std::process::id(),
        STAGING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let staged = std::fs::write(&building, body)
        .and_then(|()| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&building, std::fs::Permissions::from_mode(0o555))?;
            }
            std::fs::rename(&building, &path)
        })
        .map_err(failed);
    if staged.is_err() {
        let _ = std::fs::remove_file(&building);
    }
    staged
}

/// The directories macOS's `path_helper` would give a login shell
/// (`/etc/paths`, then `/etc/paths.d/*` in name order). The boundary denies
/// the helper so a login shell keeps the prepared `PATH`; the host appends
/// these instead, after the prepared directories.
fn system_search_path() -> String {
    let mut dirs: Vec<String> = Vec::new();
    let mut read = |file: &Path| {
        if let Ok(text) = std::fs::read_to_string(file) {
            dirs.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(str::to_owned),
            );
        }
    };
    read(Path::new("/etc/paths"));
    if let Ok(entries) = std::fs::read_dir("/etc/paths.d") {
        let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for file in files {
            read(&file);
        }
    }
    dirs.join(":")
}

/// A link named `name` in the host's tool directory to the executable the
/// host chose.
fn link_tool(bin: &Path, target: &Path, name: &str) -> Result<(), CreateFailure> {
    let link = bin.join(name);
    let failed = |error: std::io::Error| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not stage the host's {name}: {error}"),
        )
    };
    if std::fs::symlink_metadata(&link).is_ok() {
        std::fs::remove_file(&link).map_err(failed)?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &link).map_err(failed)?;
    #[cfg(not(unix))]
    std::fs::copy(target, &link).map(|_| ()).map_err(failed)?;
    Ok(())
}

/// Refuse a continuation whose working tree or agents clone no longer
/// resolves to the directory its binding was granted: a child can replace a
/// directory it may write with a link, and the next preparation must not
/// grant the link's target. A changed target refuses the launch; it does not
/// merely drop the native history.
fn check_continuation(
    inputs: &ScopeInputs<'_>,
    tree: &Path,
    agents: Option<&Path>,
) -> Result<(), CreateFailure> {
    let Some((Some(prior), _)) = inputs.prior else {
        return Ok(());
    };
    if prior
        .tree
        .as_deref()
        .is_some_and(|recorded| recorded != tree)
    {
        return Err(refuse(
            EXECUTION_SCOPE_INVALID,
            format!(
                "this session's working tree now resolves to {} instead of the directory it \
                 was bound to, so it was not continued there",
                tree.display()
            ),
        ));
    }
    if prior.agents.is_some() && prior.agents.as_deref() != agents {
        return Err(refuse(
            EXECUTION_SCOPE_INVALID,
            "this session's agents clone no longer resolves to the clone it was bound to, so it \
             was not continued",
        ));
    }
    Ok(())
}

/// The login keychain file, read-only, for a runtime or Git credential
/// helper that keeps its login there — the Claude runtime already holds the
/// whole keychain directory ([`crate::execution_scope_runtime`]). Measured
/// 2026-09-26 (`solo-transports-result.md`, G8): `gh auth git-credential`
/// answers from this one file.
fn keychain_login_grant(
    home: &Path,
    runtime: RuntimeProfile,
    credential_helpers: bool,
) -> Option<Grant> {
    if runtime == RuntimeProfile::Claude || !credential_helpers {
        return None;
    }
    canonical(&home.join("Library/Keychains/login.keychain-db")).map(|file| {
        Grant::file(
            file,
            Access::ReadOnly,
            "the login keychain a configured Git credential helper answers from",
        )
    })
}

/// The seat's role bundle: host-computed under this app's data, created now
/// if this is the seat's first launch (its skills are written into it before
/// the adapter starts), and never inside the working tree.
fn prepare_seat_bundle(
    bundle: &Path,
    state_dir: &Path,
    tree: &Path,
) -> Result<PathBuf, CreateFailure> {
    let app_data = crate::agent_fence::app_data_dir_from_state_dir(state_dir)
        .unwrap_or_else(|| state_dir.to_path_buf());
    let invalid = || {
        refuse(
            EXECUTION_SCOPE_INVALID,
            "the seat's role bundle is not a host-owned directory under this app's data",
        )
    };
    let parent = bundle.parent().ok_or_else(invalid)?;
    std::fs::create_dir_all(bundle).map_err(|error| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not prepare the seat's role bundle: {error}"),
        )
    })?;
    let real = canonical(bundle).ok_or_else(invalid)?;
    let parent = canonical(parent).ok_or_else(invalid)?;
    if !parent.starts_with(&app_data) || real.starts_with(tree) || tree.starts_with(&real) {
        return Err(invalid());
    }
    Ok(real)
}

pub(crate) fn boundary_failure(error: &BoundaryError) -> CreateFailure {
    refuse(
        EXECUTION_BOUNDARY_UNAVAILABLE,
        format!(
            "the project execution boundary could not be put in place, so nothing was started \
             ({error})"
        ),
    )
}

/// Directory, under the provider state dir, of the read-only agents clones
/// the host stages for unseated executions.
pub(crate) const AGENTS_CLONES_DIR: &str = "agents-clones";

/// Host-owned workspaces inside this app's data: the host's own action
/// worktrees, the agents clones it staged, and project-setup drafts. Each is
/// one project's material, never an ancestor of another's.
fn host_workspace_roots(state_dir: &Path, app_data: &Path) -> [PathBuf; 3] {
    [
        state_dir.join(crate::action_steps::ARTIFACTS_DIR),
        state_dir.join(AGENTS_CLONES_DIR),
        app_data.join("project-team-setup"),
    ]
}

/// Refuse a workspace root the boundary cannot hold around: one overlapping
/// a system location every execution reads, containing the operator's home,
/// or overlapping this app's data — except a host-owned workspace inside it
/// ([`host_workspace_roots`]) that a host record binds to the launch.
pub(crate) fn validate_workspace_placement(
    root: &Path,
    association: WorkspaceAssociation,
    home: &Path,
    state_dir: &Path,
) -> Result<(), CreateFailure> {
    if let Some(system) = exec_boundary::overlaps_system_root(root) {
        return Err(refuse(
            EXECUTION_SCOPE_INVALID,
            format!(
                "{} overlaps the system location {system}, which every execution may read; a \
                 project there cannot be kept apart from its neighbours. Move the checkout.",
                root.display()
            ),
        ));
    }
    if home.starts_with(root) {
        return Err(refuse(
            EXECUTION_SCOPE_INVALID,
            format!(
                "{} contains the operator's whole home directory; a project execution cannot be \
                 granted it",
                root.display()
            ),
        ));
    }
    let state_dir = canonical(state_dir).unwrap_or_else(|| state_dir.to_path_buf());
    let app_data = crate::agent_fence::app_data_dir_from_state_dir(&state_dir)
        .unwrap_or_else(|| state_dir.clone());
    let host_owned = association == WorkspaceAssociation::HostBound
        && host_workspace_roots(&state_dir, &app_data)
            .iter()
            .any(|host_root| root.starts_with(host_root) && root != host_root);
    if app_data.starts_with(root) || (root.starts_with(&app_data) && !host_owned) {
        return Err(refuse(
            EXECUTION_SCOPE_INVALID,
            format!(
                "{} overlaps this app's own data directory, which holds other projects' caches",
                root.display()
            ),
        ));
    }
    Ok(())
}

/// `git rev-parse --show-toplevel`, run by the host with repository
/// selection variables cleared.
fn git_toplevel(cwd: &Path) -> Option<PathBuf> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(crate::git_probe::HOST_GIT_NO_PROJECT_CODE)
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd);
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        cmd.env_remove(var);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    canonical(Path::new(text.trim())).filter(|top| cwd.starts_with(top))
}

/// Read grants for an executable: inside an app bundle, the bundle's
/// `Contents` (its sidecars ride with it); otherwise the file alone.
pub(crate) fn executable_grants(path: &Path, reason: &str) -> Vec<Grant> {
    let mut grants = Vec::new();
    if path.is_absolute() {
        grants.push(Grant::file(path, Access::ReadOnly, reason));
    }
    let Some(real) = canonical(path) else {
        return grants;
    };
    let bundle_contents = real
        .ancestors()
        .find(|dir| {
            dir.file_name().is_some_and(|name| name == "Contents")
                && dir
                    .parent()
                    .and_then(Path::extension)
                    .is_some_and(|ext| ext == "app")
        })
        .map(Path::to_path_buf);
    match bundle_contents {
        Some(contents) => grants.push(Grant::tree(contents, Access::ReadOnly, reason)),
        None => grants.push(Grant::file(real, Access::ReadOnly, reason)),
    }
    grants
}

/// Where Beekeeper installs the runtimes it manages (the ACP adapters' npm
/// prefix and the Node.js it runs them with), relative to the home. The
/// desktop resolves runtime adapters from exactly these roots first.
const MANAGED_RUNTIME_ROOTS: &[&str] = &[
    "Library/Application Support/Beekeeper/node-tools",
    "Library/Application Support/Beekeeper/runtimes",
];

/// The selected runtime's code, from host facts only: the adapter executable
/// the host resolved, any absolute file the host's runtime descriptor names
/// in its argv (never a project's), the Beekeeper-managed runtime
/// installations, and installation roots the host resolved for this runtime.
/// Nothing is inferred from a script's shebang or from directories around the
/// adapter: a runtime installed anywhere else fails its start inside the
/// boundary rather than widen it.
fn runtime_install_grants(inputs: &ScopeInputs<'_>, home: &Path) -> Vec<Grant> {
    let mut grants = Vec::new();
    if let Some(command) = resolve_command(inputs.agent_command) {
        grants.extend(executable_grants(&command, "the ACP adapter"));
    }
    for arg in inputs.agent_args {
        let path = Path::new(arg);
        if path.is_absolute() && path.is_file() {
            grants.extend(executable_grants(
                path,
                "a file the runtime descriptor names",
            ));
        }
    }
    let roots = MANAGED_RUNTIME_ROOTS
        .iter()
        .map(|relative| home.join(relative))
        .chain(inputs.runtime_roots.iter().cloned());
    for root in roots.filter_map(|root| canonical(&root)) {
        if root != home && !home.starts_with(&root) {
            grants.push(Grant::tree(root, Access::ReadOnly, "an installed runtime"));
        }
    }
    grants
}

fn resolve_command(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(command))
            .find(|candidate| candidate.is_file())
    })
}

/// Read-only toolchain *installations* under the home directory.
///
/// Only tool binaries: no dependency caches (a Cargo git cache or private
/// registry can hold private repositories), no global configuration (mise,
/// Cargo or Git configuration can name unrelated projects). Dependency caches
/// are the project's own (see `SCOPE_CACHES_DIR`); Git configuration is staged
/// per execution ([`crate::execution_scope_git::stage_git_config`]).
const TOOLCHAIN_INSTALLATIONS: &[(&str, &str)] = &[
    (".rustup/toolchains", "installed Rust toolchains"),
    (
        ".rustup/settings.toml",
        "rustup default-toolchain selection",
    ),
    (".cargo/bin", "rustup toolchain proxies"),
];

pub(crate) fn toolchain_grants(home: &Path) -> Vec<Grant> {
    TOOLCHAIN_INSTALLATIONS
        .iter()
        .filter_map(|(relative, reason)| {
            let real = canonical(&home.join(relative))?;
            if real == home || home.starts_with(&real) {
                return None;
            }
            Some(if real.is_dir() {
                Grant::tree(real, Access::ReadOnly, *reason)
            } else {
                Grant::file(real, Access::ReadOnly, *reason)
            })
        })
        .collect()
}

/// The Hermit packages this working tree declares, and what Hermit itself
/// needs to run them: each declared `<pkg>-<version>` installation, Hermit's
/// own executable, its public package manifests (read-only), and its
/// self-update etag directory. Measured 2026-09-26: `bin/cargo`, `bin/rustc`,
/// `bin/just` and `bin/node` of this repository run under exactly these
/// grants. A package the host has not installed yet is not downloaded from
/// inside the boundary; Hermit reports it missing. Returns the state
/// directory to export when anything was granted.
pub(crate) fn hermit_declared_grants(
    tree: &Path,
    state: Option<&Path>,
    grants: &mut Vec<Grant>,
) -> Option<PathBuf> {
    let bin = tree.join("bin");
    if !bin.join("hermit").is_file() {
        return None;
    }
    let state = canonical(state?)?;
    let pkg = state.join("pkg");
    let mut declared = 0;
    for entry in std::fs::read_dir(&bin).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(package) = name
            .strip_prefix('.')
            .and_then(|name| name.strip_suffix(".pkg"))
            .filter(|package| !package.is_empty() && !package.contains('/'))
        else {
            continue;
        };
        if let Some(installed) = canonical(&pkg.join(package)).filter(|dir| dir.starts_with(&pkg)) {
            grants.push(Grant::tree(
                installed,
                Access::ReadOnly,
                "a Hermit package this project declares",
            ));
            declared += 1;
        }
    }
    if declared == 0 {
        return None;
    }
    for entry in std::fs::read_dir(&pkg).ok()?.flatten() {
        if entry.file_name().to_string_lossy().starts_with("hermit@") {
            grants.push(Grant::tree(
                entry.path(),
                Access::ReadOnly,
                "Hermit's own executable",
            ));
        }
    }
    for source in declared_hermit_sources(&bin, &state) {
        grants.push(Grant::tree(
            source,
            Access::ReadOnly,
            "a Hermit manifest source this project declares",
        ));
    }
    // Hermit opens its download cache on start; it must exist, but it is
    // shared by every project on this host, so it is never granted: a
    // package missing from `pkg/` fails rather than downloads from inside.
    let _ = std::fs::create_dir_all(state.join("cache"));
    let metadata = state.join("metadata");
    if std::fs::create_dir_all(&metadata).is_ok() {
        if let Some(metadata) = canonical(&metadata) {
            grants.push(Grant::tree(
                metadata,
                Access::ReadWrite,
                "Hermit's self-update etags",
            ));
        }
    }
    Some(state)
}

/// Hermit's default manifest source, used when `bin/hermit.hcl` declares
/// none (Hermit's own documented default).
const HERMIT_DEFAULT_SOURCE: &str = "https://github.com/cashapp/hermit-packages.git";

/// The manifest-source clones under `<state>/sources` whose `origin` is a
/// source this project declares in `bin/hermit.hcl` (or Hermit's default).
/// Source directories are named by an opaque hash and may hold other
/// projects' private manifests, so none is granted by directory alone.
fn declared_hermit_sources(bin: &Path, state: &Path) -> Vec<PathBuf> {
    let config = std::fs::read_to_string(bin.join("hermit.hcl")).unwrap_or_default();
    let mut declared: Vec<String> = config
        .split_once("sources")
        .and_then(|(_, rest)| rest.split_once('['))
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| {
            list.split('"')
                .skip(1)
                .step_by(2)
                .map(|source| source.trim().to_owned())
                .filter(|source| !source.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if declared.is_empty() {
        declared.push(HERMIT_DEFAULT_SOURCE.to_owned());
    }
    let normalize = |url: &str| {
        url.trim()
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_owned()
    };
    let declared: Vec<String> = declared.iter().map(|url| normalize(url)).collect();
    let Ok(entries) = std::fs::read_dir(state.join("sources")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let dir = canonical(&entry.path())?;
            let mut read = std::process::Command::new("git");
            read.args(["config", "--file"])
                .arg(dir.join(".git/config"))
                .args(["--get", "remote.origin.url"]);
            let origin = read.output().ok().filter(|out| out.status.success())?;
            let origin = normalize(&String::from_utf8_lossy(&origin.stdout));
            declared.contains(&origin).then_some(dir)
        })
        .collect()
}

/// `DARWIN_USER_TEMP_DIR`, where Apple's tool shims keep an `xcrun_db` file.
pub(crate) fn xcrun_cache_dir() -> Option<PathBuf> {
    let output = std::process::Command::new("/usr/bin/getconf")
        .arg("DARWIN_USER_TEMP_DIR")
        .output()
        .ok()?;
    let dir = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    canonical(Path::new(&dir))
}

/// The scope digest: project, seat, runtime, working tree and agents rights.
fn scope_digest(inputs: &ScopeInputs<'_>, tree: &Path, agents: &str) -> String {
    let facts = format!(
        "v{SCOPE_VERSION}\nproject={}\nactor={}\ndriver={}\nruntime={:?}\ntree={}\nagents={agents}\nrole={}\n",
        inputs.project_ref.unwrap_or(""),
        inputs.actor.unwrap_or(""),
        inputs.driver,
        inputs.runtime,
        tree.display(),
        inputs.role_contract.unwrap_or(""),
    );
    hex::encode(Sha256::digest(facts.as_bytes()))
}

/// The scope-owned locations the environment points at.
struct ScopeEnv<'a> {
    temp: &'a Path,
    xdg: &'a Path,
    git_config: &'a Path,
    caches: &'a Path,
    codex_home: Option<&'a Path>,
    claude_config: Option<&'a Path>,
    hermit_state: Option<&'a Path>,
}

/// The child's environment: baseline by source; the runtime's own values
/// (model purposes); the project's declared values (host commands); the
/// scope's locations; then the seat identity, with the host's `bee` first on
/// `PATH`.
fn resolve_env(
    boundary: &exec_boundary::PreparedBoundary,
    inputs: &ScopeInputs<'_>,
    scope: &ScopeEnv<'_>,
    tool_dir: Option<&Path>,
) -> Result<ResolvedEnv, CreateFailure> {
    let usable = |path: &Path| prospective(path).is_some_and(|real| boundary.permits_read(&real));
    let model = inputs.purpose != ScopePurpose::HostCommand;
    let auth = if model {
        inputs.runtime.model_auth()
    } else {
        ModelAuth::None
    };
    let mut env = match inputs.ambient_env {
        Some(ambient) => ResolvedEnv::baseline(ambient.iter().cloned(), auth, &FENCE, &usable),
        None => ResolvedEnv::baseline(std::env::vars_os(), auth, &FENCE, &usable),
    };
    if model {
        for (name, value) in inputs.agent_env {
            env.runtime(name, value, &FENCE);
        }
    }
    // A host command's declarations are the action's own contract, refused
    // under its code; a session's scope values are the host's.
    let code = if model {
        EXECUTION_SCOPE_INVALID
    } else {
        crate::host_command::ACTION_STEP_INVALID
    };
    let invalid = |refusal: buzz_acp::exec_env::EnvRefusal| {
        refuse(
            code,
            format!("the project's environment was refused: {refusal}"),
        )
    };
    for (name, value, source) in inputs.project_env {
        env.project(name, value, *source, &FENCE, &usable)
            .map_err(invalid)?;
    }
    let text = |path: &Path| path.display().to_string();
    let mut scoped: Vec<(&str, String)> = vec![
        ("TMPDIR", format!("{}/", scope.temp.display())),
        ("XDG_CONFIG_HOME", text(scope.xdg)),
        ("GIT_CONFIG_GLOBAL", text(scope.git_config)),
        // The staged file is the whole selection (system entries included).
        ("GIT_CONFIG_NOSYSTEM", "1".to_owned()),
        ("CARGO_HOME", text(&scope.caches.join("cargo"))),
        // clang and swiftc otherwise build modules under the per-user Darwin
        // cache directory, outside the boundary.
        (
            "CLANG_MODULE_CACHE_PATH",
            text(&scope.caches.join("clang-module-cache")),
        ),
        ("npm_config_cache", text(&scope.caches.join("npm"))),
        (
            "npm_config_store_dir",
            text(&scope.caches.join("pnpm-store")),
        ),
    ];
    if !model {
        // A host command has nobody to answer a prompt.
        scoped.push(("GIT_TERMINAL_PROMPT", "0".to_owned()));
    }
    if let Some(hermit) = scope.hermit_state {
        scoped.push(("HERMIT_STATE_DIR", text(hermit)));
    }
    if let Some(config) = scope.claude_config {
        scoped.push(("CLAUDE_CONFIG_DIR", text(config)));
        // Empty on purpose: keeps the CLI on the default Keychain item — the
        // existing login — while its state is private.
        scoped.push(("CLAUDE_SECURESTORAGE_CONFIG_DIR", String::new()));
        scoped.push(("CLAUDE_CODE_TMPDIR", text(scope.temp)));
    }
    if let Some(home) = scope.codex_home {
        scoped.push(("CODEX_HOME", text(home)));
    }
    for (name, value) in scoped {
        env.scope(name, &value).map_err(invalid)?;
    }
    // Codex's own state locations are scope-owned: never inherited.
    env.remove("CODEX_SQLITE_HOME");
    if inputs.purpose == ScopePurpose::Session {
        for (name, value) in inputs.identity_env {
            // The seat custody's PATH is its `bee` directory followed by the
            // whole host PATH; only the `bee` directory is the host's choice,
            // and it is added below. Prepending the rest would put host
            // directories ahead of the project's own tools.
            if name != "PATH" {
                env.identity(name, value, &usable);
            }
        }
        if let Some(bee) = inputs.seat_bee {
            // The host's `bee`, named absolutely (`$BEE`) and found by name
            // through the host's tool directory, first on `PATH`: offline
            // commands work for every session; relay commands still need the
            // identity only a seat is given.
            env.identity(
                crate::seat_bee::BEE_ENV,
                &bee.display().to_string(),
                &usable,
            );
        }
    }
    // The host's tool directory first, for sessions and host commands; the
    // system search path last, composed here rather than by a login shell.
    if let Some(tools) = tool_dir {
        env.prepend_path(&tools.display().to_string(), EnvSource::Scope, &usable);
    }
    env.append_path(&system_search_path(), EnvSource::Platform, &usable);
    Ok(env)
}

/// The launch facts a provider path has in hand, before the scope exists.
pub(crate) struct LaunchFacts<'a> {
    pub session_id: &'a str,
    pub project_ref: Option<&'a str>,
    /// How the host ties `cwd` to `project_ref` ([`WorkspaceAssociation`]).
    pub association: WorkspaceAssociation,
    pub actor: Option<&'a str>,
    pub driver: &'a str,
    pub cwd: &'a Path,
    pub agent_command: &'a str,
    pub agent_args: &'a [String],
    pub agent_env: &'a [(String, String)],
    pub identity_env: &'a [(String, String)],
    pub seat_skills: Option<&'a crate::session::SeatSkills>,
    /// The read-only agents clone the host staged for an unseated execution.
    pub unseated_agents: Option<&'a Path>,
    /// The seat's role word, from the record.
    pub role: Option<&'a str>,
    pub rehydration: Option<&'a crate::session::RehydrationMcpDescriptor>,
    /// `(recorded binding, recorded native cursor)` on a resume or restore.
    pub prior: Option<(Option<&'a ExecutionBinding>, Option<&'a str>)>,
}

/// The immutable role contract a seat runs, from its own custody: role,
/// persona, pack commit and composition digest. `None` for an unseated
/// execution with no pack.
fn role_contract(
    role: Option<&str>,
    skills: Option<&crate::session::SeatSkills>,
) -> Option<String> {
    if role.is_none() && skills.is_none() {
        return None;
    }
    let pack = skills.and_then(|skills| skills.pack_ref.as_ref());
    let compose = skills.and_then(|skills| skills.compose_ref.as_ref());
    Some(format!(
        "role={}|persona={}|pack={}@{}:{}|compose={}",
        role.unwrap_or(""),
        skills.map_or("", |skills| skills.persona_id.as_str()),
        pack.map_or("", |pack| pack.repo.as_str()),
        pack.map_or("", |pack| pack.sha.as_str()),
        pack.map_or("", |pack| pack.path.as_str()),
        compose.map_or("", |compose| compose.digest.as_str()),
    ))
}

/// How a resumed or restored record's working tree is tied to its project:
/// host-bound when the record carries a binding for that same project — the
/// association was validated when that binding was minted — and otherwise
/// only as the project's recorded checkout or a worktree of it.
#[must_use]
pub fn recorded_association(record: &crate::state::SessionRecord) -> WorkspaceAssociation {
    let bound = record
        .execution_binding
        .as_ref()
        .is_some_and(|binding| binding.project_ref == record.project_ref);
    if bound {
        WorkspaceAssociation::HostBound
    } else {
        WorkspaceAssociation::Unbound
    }
}

impl crate::Provider {
    /// Stage (or refresh) the read-only clone of the project's agents
    /// repository an unseated execution reads, from the repository this host
    /// recorded for the project.
    ///
    /// `Ok(None)` when the project has no agents repository recorded here, or
    /// on a platform with no boundary.
    ///
    /// # Errors
    /// A recorded repository that could not be staged: the execution's own
    /// plans and instructions are missing, so it does not start as though the
    /// project had none.
    pub(crate) async fn stage_unseated_agents(
        &self,
        project_ref: Option<&str>,
        session_id: &str,
    ) -> Result<Option<PathBuf>, CreateFailure> {
        if !cfg!(target_os = "macos") {
            return Ok(None);
        }
        let projects = crate::commands::ProjectsFile::load(self.config.projects_file.as_deref());
        let Some(record) = project_ref
            .and_then(|project| projects.agents_repos.get(project))
            .cloned()
        else {
            return Ok(None);
        };
        if session_id.is_empty()
            || !session_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(refuse(
                EXECUTION_SCOPE_INVALID,
                "the session id cannot name a clone",
            ));
        }
        let unavailable = |message: String| {
            refuse(
                EXECUTION_BOUNDARY_UNAVAILABLE,
                format!("the project's agents repository could not be prepared for this session: {message}"),
            )
        };
        let state_dir = canonical(&self.config.state_dir)
            .ok_or_else(|| unavailable("the provider state directory does not exist".to_owned()))?;
        let clones = host_dir(&state_dir, AGENTS_CLONES_DIR)?;
        let dest = clones.join(session_id);
        if std::fs::symlink_metadata(&dest).is_ok() {
            host_dir(&clones, session_id)?;
        }
        let tip = crate::agents_checkout::stage_read_only_clone(&record, &dest)
            .await
            .map_err(|error| unavailable(error.to_string()))?;
        tracing::info!(
            target: "csp::scope",
            %session_id,
            sha = %tip.sha,
            stale = tip.stale,
            "staged the project's agents repository read-only"
        );
        Ok(Some(dest))
    }

    /// Prepare the execution scope for a create, resume or restore.
    ///
    /// The one path every producer takes, so the project, seat, rights and
    /// native-history binding are decided the same way for all of them.
    pub(crate) fn execution_plan(
        &self,
        facts: &LaunchFacts<'_>,
    ) -> Result<ExecutionPlan, CreateFailure> {
        let projects = crate::commands::ProjectsFile::load(self.config.projects_file.as_deref());
        let project_checkout = facts
            .project_ref
            .and_then(|project| projects.projects.get(project))
            .cloned();
        let agents = facts
            .seat_skills
            .and_then(|skills| skills.agents_checkout.as_ref())
            .map(|agents| (agents.path.as_path(), agents.writable()))
            .or_else(|| facts.unseated_agents.map(|path| (path, false)));
        let role_contract = role_contract(facts.role, facts.seat_skills);
        let hermit_state = crate::execution_scope_host::host_hermit_state();
        // Every bounded session gets the host's own `bee`: offline commands
        // work for all, and relay commands still need the identity only a
        // seat is given.
        let bee = crate::seat_bee::host_seat_bee().map(|(bee, _)| bee.path.clone());
        let mut inputs = ScopeInputs::new(
            ScopePurpose::Session,
            &self.config.state_dir,
            facts.session_id,
            facts.cwd,
        );
        inputs.project_ref = facts.project_ref;
        inputs.project_checkout = project_checkout.as_deref();
        inputs.association = facts.association;
        inputs.actor = facts.actor;
        inputs.driver = facts.driver;
        inputs.runtime = self
            .config
            .runtime_profile_override
            .unwrap_or_else(|| RuntimeProfile::for_driver(facts.driver));
        inputs.agent_command = facts.agent_command;
        inputs.agent_args = facts.agent_args;
        inputs.agent_env = facts.agent_env;
        inputs.identity_env = facts.identity_env;
        inputs.agents_checkout = agents;
        inputs.seat_bundle = facts.seat_skills.map(|skills| skills.bundle_dir.as_path());
        inputs.role_contract = role_contract.as_deref();
        inputs.context_mcp = facts
            .rehydration
            .map(|mcp| (mcp.command.as_path(), mcp.package_dir.as_path()));
        inputs.seat_bee = bee.as_deref();
        inputs.hermit_state = hermit_state.as_deref();
        inputs.prior = facts.prior;
        prepare(&inputs)
    }
}

#[cfg(test)]
#[path = "execution_scope_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "execution_scope_prep_tests.rs"]
mod prep_tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "execution_scope_live_tests.rs"]
mod live_tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "execution_scope_live_workflow_tests.rs"]
mod live_workflow;
