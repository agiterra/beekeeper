//! Desktop host for the first-party coding-session provider.
//!
//! The provider (`buzz-session-provider`) is a supervised child process that
//! answers coding-session commands (kinds 44220/44221) on the relay and
//! publishes transcripts back. This module owns everything the desktop must do
//! for it to exist: mint and store its identity, seed the trust allowlist the
//! consumer reads, assemble its environment, and keep exactly one instance
//! alive while the app runs.
//!
//! # Why this is not a managed agent
//!
//! It would be tempting to reuse [`crate::managed_agents`] wholesale — the
//! spawn, keyring, and log machinery is right there. That is deliberately not
//! done. A [`crate::managed_agents::ManagedAgentRecord`] is a *published*
//! identity: it flows into kind:30177 records, the agents UI, member pickers,
//! team snapshots, and the reconcile/retention pipelines. The coding-session
//! provider is infrastructure — it must never appear as a chattable agent, and
//! it must never be started, stopped, deleted, or snapshotted through those
//! surfaces. Keeping a separate record store is what makes that true
//! structurally rather than by convention.
//!
//! What *is* shared with `managed_agents` are the low-level primitives:
//! [`crate::secret_store::SecretStore`] (same OS keyring service, different key
//! namespace), `atomic_write_json_restricted`, the log open/rotate helpers, and
//! `resolve_command` for binary discovery.
//!
//! # Layout on disk
//!
//! ```text
//! <app-data>/session-provider/
//!   coding-session-provider.json      # records, one per relay URL, 0o600
//!   logs/<pubkey>.log                 # supervised child stdout+stderr
//!   <provider-pubkey>/                # BUZZ_CSP_STATE_DIR
//!     projects.json                   # BUZZ_CSP_PROJECTS_FILE (written later)
//!     …                               # watermarks, outbox, seq counters
//! ```
//!
//! The state directory is keyed by **provider pubkey**, not by relay or by a
//! fixed name. The provider's durable outbox stores pre-signed events; rows
//! signed by a key other than the one currently loaded are dropped at startup.
//! A rotated identity therefore gets a structurally fresh state directory
//! instead of a directory full of undeliverable rows.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

pub(crate) mod commands;
pub(crate) mod env;
pub(crate) mod runtimes;
pub(crate) mod store;
pub(crate) mod supervisor;
pub(crate) mod trust;

#[cfg(test)]
mod steer_guard_tests;
#[cfg(test)]
mod tests;

pub(crate) use supervisor::{
    shutdown_coding_session_provider, start_provider_if_provisioned, CodingSessionProviderState,
};

/// Root directory for every coding-session provider artifact.
///
/// Created on demand so a fresh install does not need a migration step.
pub(crate) fn session_provider_base_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?
        .join("session-provider");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create session-provider dir: {error}"))?;
    Ok(dir)
}

/// Where this desktop prepares the project boundary its own Git runs inside
/// when it checks out, re-stages or inspects a project workspace
/// (`buzz_session_provider_pkg::execution_scope_host`).
pub(crate) fn host_git_state_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?;
    let dir = buzz_session_provider_pkg::execution_scope_host::desktop_host_state_dir(&app_data);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create the host Git state dir: {error}"))?;
    Ok(dir)
}

/// Per-identity state directory handed to the child as `BUZZ_CSP_STATE_DIR`.
///
/// See the module docs for why this is keyed by pubkey.
pub(crate) fn provider_state_dir(
    app: &AppHandle,
    provider_pubkey: &str,
) -> Result<PathBuf, String> {
    if !crate::managed_agents::is_lowercase_hex_pubkey(provider_pubkey) {
        return Err("provider pubkey must be 64-character lowercase hex".to_string());
    }
    let dir = session_provider_base_dir(app)?.join(provider_pubkey);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create provider state dir: {error}"))?;
    Ok(dir)
}

/// Log file for the supervised child, alongside the record store.
pub(crate) fn provider_log_path(app: &AppHandle, provider_pubkey: &str) -> Result<PathBuf, String> {
    if !crate::managed_agents::is_lowercase_hex_pubkey(provider_pubkey) {
        return Err("provider pubkey must be 64-character lowercase hex".to_string());
    }
    let dir = session_provider_base_dir(app)?.join("logs");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create session-provider logs dir: {error}"))?;
    Ok(dir.join(format!("{provider_pubkey}.log")))
}

/// Canonical key for the per-relay record map.
///
/// Records are keyed by relay because one desktop can be pointed at several
/// communities, and a provider identity is only meaningful against the relay
/// whose owner attested it. Normalization is deliberately conservative — case
/// folding plus trailing-slash removal — so an operator typing the same relay
/// two ways does not mint two identities.
pub(crate) fn canonical_relay_key(relay_url: &str) -> String {
    relay_url.trim().trim_end_matches('/').to_ascii_lowercase()
}
