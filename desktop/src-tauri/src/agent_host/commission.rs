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

/// Why the app is writing the host's config — which shapes nothing about the
/// files, only which op is sent and therefore how it reads in the host's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Commissioning {
    /// A provider identity was just provisioned and handed over.
    FirstIdentity,
    /// The active community changed, so the host must serve another relay.
    RelayChanged,
}

/// Preserve existing same-scope commissioning, or write first defaults and
/// scoped provider custody, then ask a running host to pick them up.
///
/// Returns whether a host was listening. `false` is not a failure — a machine
/// where the host is not yet installed or not yet running is the ordinary case
/// during an update, and the files are what matter: the host reads them the
/// next time it starts.
pub(crate) async fn commission_host(
    app: &AppHandle,
    host: &super::AgentHost,
    relay_url: &str,
    record: &CodingSessionProviderRecord,
    store: &CodingSessionProviderStore,
    reason: Commissioning,
) -> Result<bool, String> {
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
        let config = selection::select(
            selection::load_existing(&layout::host_config_path(&home, instance))?,
            config,
            record,
            store,
            reason,
            crate::session_provider::runtimes::build_runtime_descriptors,
        )?;
        beekeeper_host_core::provision::write_commissioning(&home, &config, record)?;
    }

    // The secret does not travel on the wire; this only says "look again".
    // Which op is sent is chosen for the host's log rather than for behaviour
    // — both re-read the same files — because "the app handed me an identity"
    // and "the app switched communities" are worth telling apart when reading
    // back what happened on a machine.
    let asked = match reason {
        Commissioning::FirstIdentity => host.adopt_identity().await,
        Commissioning::RelayChanged => host.bind().await,
    };
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

/// Rewrite `host.json` for a new active relay and ask the host to rebind.
///
/// This is the community switch. It replaces the app's old stop-and-start of a
/// child it owned: the host keeps its socket up throughout, so nothing
/// watching it sees the host disappear and come back.
pub(crate) async fn rebind_host(
    app: &AppHandle,
    host: &super::AgentHost,
    relay_url: &str,
) -> Result<bool, String> {
    let store = crate::session_provider::store::load_provider_store(app)?;
    let Some(record) = store.get(relay_url).cloned() else {
        // No identity for this relay: nothing to bind to. The host keeps
        // serving whatever it was serving, which the app's own status display
        // then reports as a provider for a different relay — true, and the
        // thing a person needs to see.
        return Ok(false);
    };
    commission_host(
        app,
        host,
        relay_url,
        &record,
        &store,
        Commissioning::RelayChanged,
    )
    .await
}
