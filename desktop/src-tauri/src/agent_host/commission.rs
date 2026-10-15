//! Handing a provisioned identity to the agent host.
//!
//! The desktop resolves its owner signer and custody, then calls the shared
//! provisioning service. The CLI uses that same service; the running host
//! never needs the owner's signing key. Commissioning writes everything needed to serve
//! that identity with nobody logged in:
//!
//! - `host.json` — the relay, pubkey, explicit runtime offer, limits and
//!   absolute custody paths. Existing operator commissioning is preserved
//!   on desktop reopen, including a CLI-selected state directory and runtimes
//!   absent from the desktop's PATH. Discovery supplies first-commissioning
//!   defaults; it never replaces an existing same-scope configuration.
//! - `provider-key` — a `0600` file holding the nsec.
//!
//! # Why the app hands the key over at all
//!
//! Because the host must never be a keychain client, and that is structural
//! rather than a preference. The app keeps every secret in one keychain entry
//! through the *legacy* SecKeychain API, deliberately, so signed release
//! builds and unsigned dev builds share one store (`secret_store.rs`). Legacy
//! items carry a per-item ACL keyed to a code signature, and
//! `docs/local-desktop-instances.md` already records the consequence: "the
//! keychain ACL is bound to the binary's signature, which changes per build."
//! A daemon reading that entry means a GUI prompt at login, a human at the
//! keyboard, and a grant that dissolves on the next rebuild. A server has no
//! keychain at all.
//!
//! # What that costs, stated plainly
//!
//! On macOS the provider nsec's at-rest protection drops from an ACL-gated
//! keychain item to a `0600` file under `$HOME`. Three facts bound it:
//!
//! - it is the same protection `nokeyring` dev builds already use for this
//!   key and for managed-agent keys (`docs/local-desktop-instances.md`);
//! - it is the same protection the provider's **state directory** already has
//!   — a `0700` directory holding a durable outbox of *pre-signed events* —
//!   so compromising that directory already allows publishing as the provider;
//! - `BEEKEEPER_HOST_KEY_FILE` lets a hardened deployment point at a secrets mount
//!   or a tmpfs file with no code change.
//!
//! The keychain entry is **not** deleted. It stays as the recovery path and as
//! the thing `reset.rs` already knows how to clear.

use beekeeper_host_core::config::{HostConfig, HOST_CONFIG_VERSION};
use beekeeper_host_core::layout;
use beekeeper_host_core::record::{CodingSessionProviderRecord, CodingSessionProviderStore};
use tauri::AppHandle;

#[path = "commission_selection.rs"]
mod selection;

/// Preserve existing same-scope commissioning, or write first defaults and
/// scoped provider custody, then ask a running host to pick them up.
///
/// Returns whether a first identity handover reached the host. Existing
/// same-scope commissioning returns `false` without rewriting or notifying it;
/// a first commission with an absent host is retained for subsequent bootstrap.
pub(crate) async fn commission_host(
    app: &AppHandle,
    host: &super::AgentHost,
    relay_url: &str,
    record: &CodingSessionProviderRecord,
    store: &CodingSessionProviderStore,
) -> Result<bool, String> {
    preflight(host, relay_url, Some(&record.provider_pubkey)).await?;
    let instance = super::instance();
    let home = layout::home_dir()?;
    let base_dir = super::session_provider_base_dir(app)?;
    let state_dir = crate::session_provider::provider_state_dir(app, &record.provider_pubkey)?;

    let config = HostConfig {
        version: HOST_CONFIG_VERSION,
        instance,
        relay_url: relay_url.to_string(),
        provider_pubkey: record.provider_pubkey.clone(),
        session_provider_base_dir: base_dir,
        provider_state_dir: state_dir,
        runtimes: Vec::new(),
        max_sessions: store.max_sessions,
        turn_idle_timeout_secs: store.turn_idle_timeout_secs,
        turn_budget: store.turn_budget,
        checkpoint_on_stop: store.checkpoint_on_stop,
        capabilities: Vec::new(),
        // Resolved by the host itself: it knows where its own sidecars are,
        // and naming a path here would pin the provider this app shipped with
        // across an update of the host.
        provider_command: None,
        written_at: crate::util::now_iso(),
    };
    {
        // Serialize preservation with explicit scoped limit edits. Drop before
        // notifying the host: no client lock is held across IPC.
        let _lock = beekeeper_host_core::ownership::management_lock(&home, instance)?;
        let existing = selection::load_existing(&layout::host_config_path(&home, instance))?;
        let config = selection::select(
            existing.clone(),
            config,
            record,
            crate::session_provider::runtimes::build_runtime_descriptors,
        )?;
        if existing.as_ref() == Some(&config) {
            return Ok(false);
        }
        beekeeper_host_core::provision::write_commissioning(&home, &config, record)?;
    }

    // Only a first commission hands over custody. Existing same-scope state
    // is observed without rewriting files or notifying a running supervisor.
    let asked = host.adopt_identity().await;
    match asked {
        Ok(()) => Ok(true),
        Err(error) => {
            // Disclosed rather than swallowed: on a machine with no host yet
            // this is expected, and on a machine with a wedged one it is the
            // only trace.
            eprintln!("beekeeper-desktop: agent-host: could not hand the identity over: {error}");
            Ok(false)
        }
    }
}

/// Validate protected disk scope before a serialized mint or control attempt.
pub(crate) fn disk_scope(
    relay: &str,
    provider: Option<&str>,
) -> Result<Option<HostConfig>, String> {
    let instance = super::instance();
    let home = layout::home_dir()?;
    let existing = selection::load_existing(&layout::host_config_path(&home, instance))?;
    selection::check_scope(existing.as_ref(), None, instance, relay, provider)?;
    Ok(existing)
}

/// Refuse implicit commissioning or Start outside the existing disk and served
/// scope. An absent host is allowed; an unreadable host is not proof of absence.
pub(crate) async fn preflight(
    host: &super::AgentHost,
    relay: &str,
    provider: Option<&str>,
) -> Result<Option<HostConfig>, String> {
    let instance = super::instance();
    let existing = disk_scope(relay, provider)?;
    let observed = host.snapshot().await;
    if observed.status.is_none()
        && !matches!(
            observed.reachability,
            super::HostReachability::Absent { .. }
        )
    {
        return Err(format!(
            "executor scope is unconfirmed: {}; no commissioning or Start was requested",
            observed.message
        ));
    }
    selection::check_scope(
        existing.as_ref(),
        observed.status.as_ref(),
        instance,
        relay,
        provider,
    )?;
    Ok(existing)
}
