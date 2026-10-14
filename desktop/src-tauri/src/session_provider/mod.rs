//! Desktop client adapters for the first-party coding-session provider.
//!
//! The provider (`beekeeper-session-provider`) answers coding-session commands
//! (kinds 44220/44221) on the relay and publishes transcripts back. It is
//! supervised by `beekeeper-host`, never by this app: the desktop does not
//! spawn it, keep it alive or stop it on quit. This module owns only the
//! bounded client-side steps: mint and store its identity in the Keychain,
//! seed the trust allowlist the consumer reads, assemble commissioning
//! output, and ask the host to start it.
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
//!   <provider-pubkey>/                # BEEKEEPER_CSP_STATE_DIR
//!     projects.json                   # BEEKEEPER_CSP_PROJECTS_FILE (written later)
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

/// Keying the per-relay record map, shared with `beekeeper-host` so a desktop and a
/// host cannot disagree about which relay a record belongs to.
pub(crate) use beekeeper_host_core::record::canonical_relay_key;

pub(crate) mod commands;
pub(crate) mod env;
pub(crate) mod full_access;
pub(crate) mod runtimes;
pub(crate) mod status;
pub(crate) mod store;
pub(crate) mod trust;

#[cfg(test)]
mod steer_guard_tests;
#[cfg(test)]
mod tests;

/// Root directory for every coding-session provider artifact.
///
/// Created on demand so a fresh install does not need a migration step.
pub(crate) fn session_provider_base_dir(_app: &AppHandle) -> Result<PathBuf, String> {
    let home = beekeeper_host_core::layout::home_dir()?;
    let instance = crate::agent_host::instance();
    // Existing host state (including Solo sessions) retains its explicitly
    // commissioned path. New installations use the same host-owned root as CLI.
    let config = beekeeper_host_core::config::HostConfig::load(
        &beekeeper_host_core::layout::host_config_path(&home, instance),
    )?;
    let dir = config
        .map(|c| c.session_provider_base_dir)
        .unwrap_or_else(|| {
            beekeeper_host_core::layout::host_dir(&home, instance).join("service/session-provider")
        });
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&dir)?;
    Ok(dir)
}

/// Where this desktop prepares the project boundary its own Git runs inside
/// when it checks out, re-stages or inspects a project workspace
/// (`beekeeper_session_provider_pkg::execution_scope_host`).
pub(crate) fn host_git_state_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?;
    let dir =
        beekeeper_session_provider_pkg::execution_scope_host::desktop_host_state_dir(&app_data);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create the host Git state dir: {error}"))?;
    Ok(dir)
}

/// Per-identity state directory handed to the child as `BEEKEEPER_CSP_STATE_DIR`.
///
/// See the module docs for why this is keyed by pubkey.
pub(crate) fn provider_state_dir(
    app: &AppHandle,
    provider_pubkey: &str,
) -> Result<PathBuf, String> {
    let dir = beekeeper_host_core::record::provider_state_dir_in(
        &session_provider_base_dir(app)?,
        provider_pubkey,
    )?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create provider state dir: {error}"))?;
    Ok(dir)
}

/// Log file for the supervised child, alongside the record store.
///
/// The **host** writes this file now; the app creates the directory at
/// commissioning so the host's first spawn has somewhere to write, and knows
/// the path so it can offer the log to a person. Derived through the shared
/// crate so the two cannot end up naming different files.
#[allow(dead_code)] // Offered to the log viewer in a later slice.
pub(crate) fn provider_log_path(app: &AppHandle, provider_pubkey: &str) -> Result<PathBuf, String> {
    let path = beekeeper_host_core::record::provider_log_path_in(
        &session_provider_base_dir(app)?,
        provider_pubkey,
    )?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("failed to create session-provider logs dir: {error}"))?;
    }
    Ok(path)
}
