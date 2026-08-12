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

use crate::session_provider::store::CodingSessionProviderRecord;

/// Filename of the host-local working-directory map inside the state dir.
///
/// The desktop writes this file in a later slice; the variable is exported now
/// so the provider's hot-reload path is wired from the first spawn and the
/// contract does not change under it later.
pub(crate) const PROJECTS_FILE_NAME: &str = "projects.json";

/// Default log filter for the child. Deliberately quiet: the provider's own
/// lifecycle lines are what matter in the log file, not relay chatter.
pub(crate) const DEFAULT_RUST_LOG: &str = "info,buzz_session_provider=info";

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
    /// Resolved Claude Code CLI, exported as `CLAUDE_CODE_EXECUTABLE` so the
    /// ACP adapter the provider spawns per session finds the same binary
    /// managed agents use. `None` leaves the adapter's own PATH lookup in
    /// charge.
    pub claude_code_executable: Option<PathBuf>,
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
    env.insert("RUST_LOG".to_string(), DEFAULT_RUST_LOG.to_string());
    if let Some(cli) = &inputs.claude_code_executable {
        env.insert(
            "CLAUDE_CODE_EXECUTABLE".to_string(),
            cli.to_string_lossy().into_owned(),
        );
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
    "BUZZ_ACP_PRIVATE_KEY",
    "BUZZ_API_TOKEN",
    "NOSTR_PRIVATE_KEY",
];
