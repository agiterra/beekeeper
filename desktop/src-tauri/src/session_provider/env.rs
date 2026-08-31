//! The child environment, assembled as data.
//!
//! The provider takes its entire configuration from the environment (see
//! `crates/buzz-session-provider/src/config.rs`), which makes the env map the
//! real interface between host and provider. Building it as a plain
//! `BTreeMap` rather than mutating a `Command` in place is what lets the tests
//! assert the two properties that matter and cannot be checked by reading the
//! spawn code: `BUZZ_AUTH_TAG` is a JSON array of strings, and the owner's
//! secret key appears nowhere in it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use buzz_core_pkg::coding_session_runtime::RuntimeDescriptor;

use crate::session_provider::store::CodingSessionProviderRecord;

/// Filename of the host-local working-directory map inside the state dir.
///
/// The desktop writes this file in a later slice; the variable is exported now
/// so the provider's hot-reload path is wired from the first spawn and the
/// contract does not change under it later.
pub(crate) const PROJECTS_FILE_NAME: &str = "projects.json";

/// Default log filter for the child. Deliberately quiet: the provider's own
/// lifecycle lines are what matter in the log file, not relay chatter.
///
/// A **default**, not an override — see `rust_log` on [`ProviderEnvInputs`].
pub(crate) const DEFAULT_RUST_LOG: &str = "info,buzz_session_provider=info";

/// Debug switch asking the adapter to forward every raw SDK message.
///
/// Passed through explicitly rather than left to inheritance. The child does
/// inherit the desktop's environment, so this would arrive anyway — but a
/// documented debug switch that works by accident is one `env_clear()` away
/// from silently doing nothing, and the failure would look like the adapter
/// ignoring the request.
pub(crate) const EMIT_RAW_SDK_FRAMES_VAR: &str = "BUZZ_CSP_EMIT_RAW_SDK_FRAMES";

/// Everything the env map is derived from.
pub(crate) struct ProviderEnvInputs<'a> {
    /// The provisioned identity. Supplies key, instance id, and attestation.
    pub record: &'a CodingSessionProviderRecord,
    /// Relay the child connects to.
    pub relay_url: &'a str,
    /// `BUZZ_CSP_STATE_DIR`.
    pub state_dir: &'a Path,
    /// Resolved ACP adapter executable. The managed Node tools directory is not
    /// guaranteed to be on the desktop process's inherited PATH.
    pub agent_command: Option<PathBuf>,
    /// Resolved private read-only context MCP sidecar. The provider passes it
    /// only to sessions for which a verified relay package was projected.
    pub context_mcp_command: Option<PathBuf>,
    /// Resolved Claude Code CLI, exported as `CLAUDE_CODE_EXECUTABLE` so the
    /// ACP adapter the provider spawns per session finds the same binary
    /// managed agents use. `None` leaves the adapter's own PATH lookup in
    /// charge.
    pub claude_code_executable: Option<PathBuf>,
    /// The full runtime list, exported as `BUZZ_CSP_RUNTIMES`. A newer sidecar
    /// reads this as the complete offer; an older one ignores it and keeps
    /// using the legacy variables above, which stay exported alongside it.
    pub runtimes: Vec<RuntimeDescriptor>,
    /// Ceiling on concurrently live agent processes, or `None` to leave the
    /// provider's own default (10) in charge. `Some(0)` is unlimited.
    pub max_sessions: Option<usize>,
    /// Per-turn silence budget in seconds; `None` keeps the provider default.
    pub turn_idle_timeout_secs: Option<u64>,
    /// Log filter for the child, when the host was launched with one.
    ///
    /// `None` uses [`DEFAULT_RUST_LOG`]. This used to be set unconditionally,
    /// which meant a `RUST_LOG` chosen for a debugging session was silently
    /// discarded — the one moment somebody actually cares what the child
    /// logs.
    pub rust_log: Option<String>,
    /// Whether to ask the adapter for raw SDK frames. See
    /// [`EMIT_RAW_SDK_FRAMES_VAR`].
    pub emit_raw_sdk_frames: bool,
    /// Turns one crew session may start; `None` keeps the provider default
    /// (200), `Some(0)` removes the budget.
    pub turn_budget: Option<u64>,
    /// Augmented `PATH` for the provider and every adapter it spawns. A
    /// Finder-launched desktop inherits the bare GUI `PATH` (no `node`), and
    /// the ACP adapters are npm shims with `#!/usr/bin/env node` shebangs —
    /// without this the provider spawns them into `env: node: No such file or
    /// directory`. `None` leaves the inherited `PATH` untouched.
    pub augmented_path: Option<String>,
    /// The repository checkout this app is itself running out of, when it has
    /// one (a `cargo tauri dev` build; a bundled app launched from Finder has
    /// none).
    ///
    /// Exported as [`SHARED_WORKDIRS_VAR`] so the provider can refuse to seat
    /// an agent there. The provider owns that refusal — only the host knows
    /// which directory it is. Item 87(d), found live: a Team launch seated its
    /// lead in the operator's own hot checkout and wrote the seat's skills
    /// into it, while every seat the lead hired got a worktree.
    pub app_checkout: Option<PathBuf>,
}

/// The variable the provider reads its host-named shared directories from.
///
/// Kept byte-for-byte in step with `SHARED_WORKDIRS_VAR` in
/// `crates/buzz-session-provider/src/session.rs`; a drift here is a refusal
/// that silently stops happening.
pub(crate) const SHARED_WORKDIRS_VAR: &str = "BUZZ_CSP_SHARED_WORKDIRS";

/// The repository `dir` sits inside, or `None`.
///
/// Walks up looking for a `.git` entry, stopping at the filesystem root. A
/// `home` that is itself a repository is deliberately not a checkout: the
/// provider already refuses the operator's home by name, and calling it "the
/// checkout the app runs from" would be the wrong sentence to read.
///
/// `home` is a parameter so this can be proved against directories a test
/// owns.
pub(crate) fn resolve_app_checkout(dir: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let mut current = Some(dir);
    while let Some(candidate) = current {
        if candidate.join(".git").exists() {
            if home.is_some_and(|home| home == candidate) {
                return None;
            }
            return Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

/// [`resolve_app_checkout`] against this process's own working directory.
pub(crate) fn app_checkout_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    resolve_app_checkout(&cwd, dirs::home_dir().as_deref())
}

/// Build the child's Buzz-owned environment.
///
/// Only variables the host is authoritative for are set. The ACP command is
/// included only after the desktop resolves the provider's default binary to
/// an installed path; model lists, caps, and timeouts remain provider-owned so
/// the host cannot silently pin stale defaults across upgrades.
pub(crate) fn build_provider_env(inputs: &ProviderEnvInputs<'_>) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert(
        "BUZZ_PRIVATE_KEY".to_string(),
        inputs.record.private_key_nsec.clone(),
    );
    env.insert("BUZZ_RELAY_URL".to_string(), inputs.relay_url.to_string());
    if let Some(auth_tag) = &inputs.record.auth_tag {
        // Verbatim: the value minted by `nip_oa::compute_auth_tag` already is
        // the JSON array of strings the provider parses. Re-encoding here is
        // how the tag stops verifying.
        env.insert("BUZZ_AUTH_TAG".to_string(), auth_tag.clone());
    }
    env.insert(
        "BUZZ_CSP_STATE_DIR".to_string(),
        inputs.state_dir.to_string_lossy().into_owned(),
    );
    if let Some(max_sessions) = inputs.max_sessions {
        // Absent means "provider's own default", so the variable is exported
        // only when a person actually chose a number — an unset ceiling and a
        // ceiling that happens to equal the default are different facts.
        env.insert(
            "BUZZ_CSP_MAX_SESSIONS".to_string(),
            max_sessions.to_string(),
        );
    }
    if let Some(idle_timeout) = inputs.turn_idle_timeout_secs {
        env.insert(
            "BUZZ_CSP_IDLE_TIMEOUT".to_string(),
            idle_timeout.to_string(),
        );
    }
    if let Some(turn_budget) = inputs.turn_budget {
        // Same rule as the ceiling above: exported only when a person chose a
        // number, because "unset" and "happens to equal the default" are
        // different facts and the settings panel discloses which one is live.
        env.insert("BUZZ_CSP_TURN_BUDGET".to_string(), turn_budget.to_string());
    }
    env.insert(
        "BUZZ_CSP_PROJECTS_FILE".to_string(),
        inputs
            .state_dir
            .join(PROJECTS_FILE_NAME)
            .to_string_lossy()
            .into_owned(),
    );
    env.insert(
        "BUZZ_CSP_INSTANCE_ID".to_string(),
        inputs.record.instance_id.clone(),
    );
    if let Some(agent_command) = &inputs.agent_command {
        env.insert(
            "BUZZ_CSP_AGENT_COMMAND".to_string(),
            agent_command.to_string_lossy().into_owned(),
        );
    }
    if let Some(context_mcp_command) = &inputs.context_mcp_command {
        env.insert(
            "BUZZ_CSP_CONTEXT_MCP_COMMAND".to_string(),
            context_mcp_command.to_string_lossy().into_owned(),
        );
    }
    if let Some(path) = &inputs.augmented_path {
        env.insert("PATH".to_string(), path.clone());
    }
    if let Some(checkout) = &inputs.app_checkout {
        // Absent rather than empty when there is none: an empty list would
        // read as "the host looked and found nothing shared", which is a
        // different claim than "the host could not tell".
        env.insert(
            SHARED_WORKDIRS_VAR.to_string(),
            checkout.to_string_lossy().into_owned(),
        );
    }
    env.insert(
        "RUST_LOG".to_string(),
        inputs
            .rust_log
            .clone()
            .unwrap_or_else(|| DEFAULT_RUST_LOG.to_string()),
    );
    if inputs.emit_raw_sdk_frames {
        // Absent rather than "false" when off, so a provider that never asked
        // sends no `emitRawSDKMessages` key at all.
        env.insert(EMIT_RAW_SDK_FRAMES_VAR.to_string(), "true".to_string());
    }
    if let Some(cli) = &inputs.claude_code_executable {
        env.insert(
            "CLAUDE_CODE_EXECUTABLE".to_string(),
            cli.to_string_lossy().into_owned(),
        );
    }
    if !inputs.runtimes.is_empty() {
        match serde_json::to_string(&inputs.runtimes) {
            Ok(json) => {
                env.insert("BUZZ_CSP_RUNTIMES".to_string(), json);
            }
            Err(error) => {
                // Unreachable for this shape; the legacy variables above keep a
                // claude-only sidecar working if it ever happens.
                eprintln!("buzz-desktop: session-provider: failed to encode runtimes: {error}");
            }
        }
    }
    env
}

/// Variables that must never survive from the desktop's own environment into
/// the child, because a stale inherited value would be indistinguishable from
/// a deliberate one.
///
/// `BUZZ_AUTH_TAG` is the important case: when the record carries no
/// attestation the child must present none, not whatever the developer had
/// exported in their shell.
pub(crate) const INHERITED_KEYS_TO_CLEAR: &[&str] = &[
    "BUZZ_AUTH_TAG",
    SHARED_WORKDIRS_VAR,
    "BUZZ_ACP_PRIVATE_KEY",
    "BUZZ_API_TOKEN",
    "BUZZ_CSP_RUNTIMES",
    "BUZZ_CSP_CONTEXT_MCP_COMMAND",
    "NOSTR_PRIVATE_KEY",
];
