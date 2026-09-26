//! What each supported runtime needs inside the boundary, and nothing more.
//!
//! Every runtime's configuration, native history, memory and state live in
//! the execution's own host-owned directory (see
//! [`crate::execution_scope`]): nothing in the operator's `~/.claude`,
//! `~/.claude.json` or `~/.codex` (other than Codex's one login file) is
//! granted.
//!
//! # Claude Code (claude-agent-acp)
//!
//! Measured 2026-09-26 against Claude Code 2.1.283 through claude-agent-acp
//! 0.70.0 (investigation report `claude-history-result.md`, to be archived
//! with the project-integrity evidence): with `CLAUDE_CONFIG_DIR` pointing at
//! a private directory **and** `CLAUDE_SECURESTORAGE_CONFIG_DIR` set to the
//! empty string, the CLI keeps using the default Keychain item — the
//! operator's existing login — while its settings, `.claude.json`, native
//! transcripts and memory live in the private directory. Two executions with
//! an identical working directory keep separate histories; neither can read
//! the other's. `CLAUDE_SECURESTORAGE_CONFIG_DIR` is an internal variable of
//! the CLI, so the host proves the login before any model work
//! ([`claude_auth_preflight`]) rather than assume it: a CLI that stops
//! honouring it fails loudly as "not logged in", never with a silent new
//! login. A login kept only in the plaintext credentials file (not the
//! Keychain) fails that preflight too; it is not supported here.
//!
//! # Codex (codex-acp, candidate A)
//!
//! Measured 2026-09-26 (codex-acp 1.6.2, codex-cli 0.148.0; report
//! `result.md`, to be archived): a host-created private `CODEX_HOME` keeps
//! Codex's config, history, memory, state databases and sessions private to
//! the execution. The existing login is reached one of two ways:
//!
//! * **A configured API key** (`OPENAI_API_KEY` or `CODEX_API_KEY` in the
//!   runtime descriptor or the host environment): passed as the selected
//!   runtime's model-login variable; no login file is involved.
//! * **The file credential store**: `auth.json` in the private home is a
//!   symlink to the operator's one `~/.codex/auth.json`, which Codex writes
//!   *through* on refresh. The boundary grants that one file and refuses
//!   replacing the link, so a replace-style write fails loudly instead of
//!   forking the login.
//!
//! A keyring credential store is not supported for project executions and
//! is refused as such — never reported as a missing login. Limitation,
//! disclosed: the model's own tool processes can read the selected login.

use std::path::{Path, PathBuf};

use buzz_acp::exec_boundary::{Access, Grant, PreparedBoundary};
use buzz_acp::exec_env::{EnvSource, ResolvedEnv};

use crate::execution_scope::{
    canonical, executable_grants, refuse, EXECUTION_BOUNDARY_UNAVAILABLE,
};
use crate::session::CreateFailure;

/// Claude Code versions this contract was measured against.
pub const CLAUDE_VERSIONS_MEASURED: &[&str] = &["2.1.283"];

/// Claude Code's executable and its existing (Keychain) login.
pub(crate) fn claude_runtime_grants(executable: &Path, home: &Path) -> Vec<Grant> {
    let mut grants = executable_grants(executable, "Claude Code CLI");
    if let Some(keychains) = canonical(&home.join("Library/Keychains")) {
        grants.push(Grant::tree(
            keychains,
            Access::ReadWrite,
            "login keychain holding the existing Claude login (token refresh writes it)",
        ));
    }
    grants
}

/// The CLI a Claude execution runs: the descriptor's `CLAUDE_CODE_EXECUTABLE`,
/// else the host's own.
pub(crate) fn claude_executable(agent_env: &[(String, String)]) -> Option<PathBuf> {
    agent_env
        .iter()
        .find(|(name, _)| name == "CLAUDE_CODE_EXECUTABLE")
        .map(|(_, value)| PathBuf::from(value))
        .or_else(|| std::env::var_os("CLAUDE_CODE_EXECUTABLE").map(PathBuf::from))
        .filter(|path| path.is_absolute())
}

/// Prove, under the execution's own boundary and environment, that the CLI
/// is logged in with the operator's existing login **and** keeps its state in
/// the private configuration directory. Runs before the adapter exists.
///
/// # Errors
/// A precise refusal naming which half failed.
pub(crate) fn claude_auth_preflight(
    boundary: &PreparedBoundary,
    env: &ResolvedEnv,
    cwd: &Path,
    executable: &Path,
    private_config: &Path,
) -> Result<(), CreateFailure> {
    let (program, args) = boundary.wrap(
        &executable.to_string_lossy(),
        &["auth".to_owned(), "status".to_owned(), "--json".to_owned()],
    );
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .stdin(std::process::Stdio::null());
    for (name, value) in env.vars() {
        command.env(name, value);
    }
    let output = command.output().map_err(|error| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not check the Claude login inside the project boundary: {error}"),
        )
    })?;
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            "Claude Code did not report its login state inside the project boundary; this CLI \
             version is not supported for project executions",
        )
    })?;
    if status.get("loggedIn").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(logged_out_inside_boundary(executable));
    }
    let reported = status
        .get("configDirectory")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .and_then(|dir| canonical(&dir));
    if reported.as_deref() != canonical(private_config).as_deref() {
        return Err(refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            "Claude Code did not keep its configuration in the execution's private directory, \
             so its history and settings could not be kept to this project",
        ));
    }
    Ok(())
}

/// Why the scoped CLI reported no login: either this computer has no Claude
/// login at all (sign in), or it has one the CLI cannot reach from the
/// private configuration (a compatibility failure — a new login would not
/// help and must not be prescribed). Decided by the CLI's own, free,
/// host-side `auth status`; neither payload is logged.
fn logged_out_inside_boundary(executable: &Path) -> CreateFailure {
    let host = std::process::Command::new(executable)
        .args(["auth", "status", "--json"])
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_SECURESTORAGE_CONFIG_DIR")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .and_then(|output| serde_json::from_slice::<serde_json::Value>(&output.stdout).ok())
        .and_then(|status| status.get("loggedIn").and_then(serde_json::Value::as_bool));
    let version = std::process::Command::new(executable)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|version| !version.is_empty() && version.len() < 80)
        .unwrap_or_else(|| "an unreported version".to_owned());
    match host {
        Some(true) => refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!(
                "this computer's existing Claude login could not be reached from inside the \
                 project boundary with Claude Code {version} (measured working: {}). No new \
                 login is needed; this Claude Code version is not supported for project \
                 executions yet.",
                CLAUDE_VERSIONS_MEASURED.join(", ")
            ),
        ),
        _ => refuse(
            crate::payload::PROVIDER_AUTH_REQUIRED,
            "Claude Code is not signed in on this computer; sign in to Claude Code and start the \
             session again",
        ),
    }
}

/// Codex candidate A: the private home, its guarded link to the existing
/// login, and the grants for both.
pub(crate) struct CodexHome {
    /// `CODEX_HOME` for the execution.
    pub home: PathBuf,
    /// Grants: the one real login file, and the guard on its link.
    pub grants: Vec<Grant>,
}

/// The model-login variables that select Codex's API-key route.
const CODEX_API_KEY_VARS: &[&str] = &["OPENAI_API_KEY", "CODEX_API_KEY"];

/// Whether this execution reaches Codex through a configured API key: set
/// by the runtime descriptor, or inherited from the host environment as the
/// selected runtime's model login.
fn codex_api_key_configured(agent_env: &[(String, String)]) -> bool {
    CODEX_API_KEY_VARS.iter().any(|name| {
        agent_env
            .iter()
            .any(|(key, value)| key == name && !value.trim().is_empty())
            || std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
    })
}

/// The Codex login this host selected: where Codex's file store is, and
/// whether the API-key route is configured. Production reads it from the host
/// ([`CodexLogin::from_host`]); a test names a disposable one, so it never
/// consults the person's own login or the process environment.
#[derive(Debug, Clone)]
pub(crate) struct CodexLogin {
    /// The operator's Codex home (`$CODEX_HOME`, else `~/.codex`).
    pub operator_home: PathBuf,
    /// An API key selects Codex's API-key route.
    pub api_key: bool,
}

impl CodexLogin {
    /// This host's selection, for an execution whose runtime descriptor sets
    /// `agent_env`.
    pub(crate) fn from_host(home: &Path, agent_env: &[(String, String)]) -> Self {
        Self {
            operator_home: std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(".codex")),
            api_key: codex_api_key_configured(agent_env),
        }
    }
}

/// Prepare `<state>/codex-home`.
///
/// # Errors
/// A precise refusal when neither supported route is configured: a keyring
/// credential store is named as unsupported; no login file and no API key
/// says exactly that, without prescribing a new sign-in when Codex may
/// already be signed in some other way.
pub(crate) fn prepare_codex_home(
    state: &Path,
    login: &CodexLogin,
) -> Result<CodexHome, CreateFailure> {
    let operator_home = &login.operator_home;
    let codex_home = crate::execution_scope::host_dir(state, "codex-home")?;
    let anchor = Grant::file(
        &codex_home,
        Access::NoUnlink,
        "a runtime directory's own anchor",
    );
    let io = |error: std::io::Error| {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not prepare the execution's Codex home: {error}"),
        )
    };
    if login.api_key {
        return Ok(CodexHome {
            home: codex_home,
            grants: vec![anchor],
        });
    }
    if let Some(store) = configured_credential_store(&operator_home.join("config.toml")) {
        if store != "file" {
            return Err(refuse(
                EXECUTION_BOUNDARY_UNAVAILABLE,
                format!(
                    "the Codex login on this computer is kept in the \"{store}\" credential \
                     store, which project executions do not support yet; Codex's file store or \
                     a configured API key works"
                ),
            ));
        }
    }
    let real_auth = canonical(&operator_home.join("auth.json"))
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            refuse(
                EXECUTION_BOUNDARY_UNAVAILABLE,
                "this computer has no Codex login in Codex's file store (~/.codex/auth.json) \
                 and no Codex API key is configured; project executions support those two \
                 routes",
            )
        })?;
    crate::execution_scope::write_host_file(
        &codex_home.join("config.toml"),
        b"# Written by Beekeeper for one project execution.\ncli_auth_credentials_store = \"file\"\n",
        false,
    )?;
    let link = codex_home.join("auth.json");
    let not_a_link = || {
        refuse(
            EXECUTION_BOUNDARY_UNAVAILABLE,
            "the execution's Codex home holds a login file that is not the link to this \
             computer's Codex login; refusing rather than use a copy",
        )
    };
    match std::fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let target = std::fs::read_link(&link).map_err(io)?;
            if canonical(&target).as_deref() != Some(real_auth.as_path()) {
                return Err(refuse(
                    EXECUTION_BOUNDARY_UNAVAILABLE,
                    "the execution's Codex login link points somewhere other than this \
                     computer's Codex login",
                ));
            }
        }
        Ok(_) => return Err(not_a_link()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&real_auth, &link).map_err(io)?;
            // Never reached: no project boundary backend exists off macOS, so
            // `prepare` returns the disclosed legacy plan before this.
            #[cfg(not(unix))]
            return Err(refuse(
                EXECUTION_BOUNDARY_UNAVAILABLE,
                "the Codex login link is supported only where a project boundary backend exists",
            ));
        }
        Err(error) => return Err(io(error)),
    }
    Ok(CodexHome {
        home: codex_home,
        grants: vec![
            Grant::file(
                &real_auth,
                Access::ReadWrite,
                "the existing Codex login file (written through on refresh)",
            ),
            Grant::file(&link, Access::NoUnlink, "the link to the Codex login"),
            anchor,
        ],
    })
}

/// Whether the Claude CLI protects a name from project and local settings
/// by itself (measured 2026-09-26, `mcp-native-result.md`): its own
/// configuration, temp and home locations.
fn claude_protects(name: &str) -> bool {
    name.starts_with("CLAUDE_")
        || name.starts_with("XDG_")
        || matches!(name, "HOME" | "TMPDIR" | "TMP" | "TEMP")
}

/// Write the host's flag-tier Claude settings for one execution.
///
/// Flag settings outrank project and local settings (measured: flag, then
/// local, then project, then the process environment). The file pins every
/// scope and identity value the host resolved — the seat's relay, key and
/// project coordinate,
/// Git identity, the scope's Git configuration and caches — so a project's
/// `.claude/settings*.json` `env` cannot re-point them, in the CLI or in any
/// process it starts (the host's MCP servers included). It holds nothing the
/// execution does not already hold in its own environment, lives in the
/// host's control directory (read-only to the child), is created mode 0600,
/// and is never logged; no value travels on a command line. Names the CLI
/// protects itself are left to it, and `PATH` stays the project's. Account
/// connectors are off, and Claude's own Bash sandbox is off because the host
/// boundary already contains every process and the nested sandbox cannot
/// start inside it.
///
/// # Errors
/// The file could not be written.
pub(crate) fn write_claude_settings(path: &Path, env: &ResolvedEnv) -> Result<(), CreateFailure> {
    let pinned: serde_json::Map<String, serde_json::Value> = env
        .provenance()
        .into_iter()
        .filter(|(name, source)| {
            matches!(source, EnvSource::Scope | EnvSource::Identity)
                && name != "PATH"
                && !claude_protects(name)
        })
        .filter_map(|(name, _)| {
            env.get(&name)
                .map(|value| (name, serde_json::Value::String(value.to_owned())))
        })
        .collect();
    let settings = serde_json::json!({
        "sandbox": { "enabled": false },
        "disableClaudeAiConnectors": true,
        "env": pinned,
    });
    crate::execution_scope::write_host_file(path, settings.to_string().as_bytes(), true)
}

/// The top-level `cli_auth_credentials_store` of a Codex `config.toml`.
fn configured_credential_store(config: &Path) -> Option<String> {
    let text = std::fs::read_to_string(config).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    value
        .get("cli_auth_credentials_store")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
#[path = "execution_scope_runtime_tests.rs"]
mod tests;
