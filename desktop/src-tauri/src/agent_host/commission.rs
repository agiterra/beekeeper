//! Handing a provisioned identity to the agent host.
//!
//! Commissioning is the app's: it mints the provider keypair and gets the
//! owner key to attest it (NIP-OA), and the host deliberately cannot do
//! either. What the app then writes down is everything the host needs to serve
//! that identity with nobody logged in:
//!
//! - `host.json` — the relay, the pubkey, the runtime offer, the stored
//!   limits, and **the absolute path of the provider's state directory**. That
//!   directory stays under the app's own app-data tree, because five app
//!   modules write into it; a host that reconstructed the path from its own
//!   idea of where app data lives would be right on exactly one machine.
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

use beekeeper_host_core::atomic_write::{atomic_write_json_restricted, create_dir_all_restricted};
use beekeeper_host_core::config::{HostConfig, HOST_CONFIG_VERSION};
use beekeeper_host_core::layout;
use beekeeper_host_core::record::{CodingSessionProviderRecord, CodingSessionProviderStore};
use tauri::AppHandle;

/// Why the app is writing the host's config — which shapes nothing about the
/// files, only which op is sent and therefore how it reads in the host's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Commissioning {
    /// A provider identity was just provisioned and handed over.
    FirstIdentity,
    /// The active community changed, so the host must serve another relay.
    RelayChanged,
}

/// Write `host.json` and the key file for `record`, then ask a running host to
/// pick them up.
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
    let host_dir = layout::host_dir(&home, instance);
    create_dir_all_restricted(&host_dir)?;

    let base_dir = super::session_provider_base_dir(app)?;
    let state_dir = crate::session_provider::provider_state_dir(app, &record.provider_pubkey)?;

    // The key first, then the config. A host that saw a config naming an
    // identity whose key is not yet written would refuse and publish
    // `key_unresolved` for the window between the two writes — briefly true,
    // and needlessly alarming. The other order is never wrong.
    write_key_file(&home, instance, record)?;

    let config = HostConfig {
        version: HOST_CONFIG_VERSION,
        instance,
        relay_url: relay_url.to_string(),
        provider_pubkey: record.provider_pubkey.clone(),
        session_provider_base_dir: base_dir,
        provider_state_dir: state_dir,
        runtimes: crate::session_provider::runtimes::build_runtime_descriptors(),
        max_sessions: store.max_sessions,
        turn_idle_timeout_secs: store.turn_idle_timeout_secs,
        turn_budget: store.turn_budget,
        // Resolved by the host itself: it knows where its own sidecars are,
        // and naming a path here would pin the provider this app shipped with
        // across an update of the host.
        provider_command: None,
        written_at: crate::util::now_iso(),
    };
    config.validate()?;
    write_json(&layout::host_config_path(&home, instance), &config)?;

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

/// Write the nsec to the host's `0600` key file.
fn write_key_file(
    home: &std::path::Path,
    instance: layout::Instance,
    record: &CodingSessionProviderRecord,
) -> Result<(), String> {
    if record.private_key_nsec.trim().is_empty() {
        return Err(format!(
            "the coding-session provider {} has no private key available — the OS keyring may be \
             unreachable. Refusing to commission the agent host without an identity, because a \
             replacement key would strand every event the real one signed.",
            record.provider_pubkey
        ));
    }
    let path = layout::host_key_file_path(home, instance);
    // `atomic_write_json_restricted` sets 0600 on the temp file *before* the
    // secret bytes go in, so there is no window where the key is readable.
    ensure_exists(&path)?;
    atomic_write_json_restricted(&path, record.private_key_nsec.trim().as_bytes())
}

fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
    ensure_exists(path)?;
    atomic_write_json_restricted(path, &payload)
}

/// `atomic_write_json_restricted` opens an existing path, so a first write
/// needs the file to exist. Created empty and owner-only, never with content.
fn ensure_exists(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
        file.flush()
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, b"")
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record with no key must be refused *before* anything is written, and
    /// the refusal must say why a replacement is not minted instead.
    #[test]
    fn a_keyless_record_is_refused_rather_than_commissioned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let record = CodingSessionProviderRecord {
            provider_pubkey: "a".repeat(64),
            instance_id: "a".repeat(16),
            auth_tag: None,
            created_at: "2026-09-30T00:00:00Z".to_string(),
            relay_url: "wss://hive.example.org".to_string(),
            private_key_nsec: String::new(),
        };
        let error = write_key_file(dir.path(), layout::Instance::Production, &record)
            .expect_err("must be refused");
        assert!(error.contains("would strand every event"), "{error}");
        assert!(
            !layout::host_key_file_path(dir.path(), layout::Instance::Production).exists(),
            "nothing may be written for a keyless record"
        );
    }

    /// The key file must be owner-only from the moment it exists — there must
    /// be no window between `create` and `chmod` where it is readable.
    #[cfg(unix)]
    #[test]
    fn the_key_file_is_owner_only_and_holds_exactly_the_nsec() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path();
        create_dir_all_restricted(&layout::host_dir(home, layout::Instance::Production))
            .expect("host dir");
        let record = CodingSessionProviderRecord {
            provider_pubkey: "b".repeat(64),
            instance_id: "b".repeat(16),
            auth_tag: None,
            created_at: "2026-09-30T00:00:00Z".to_string(),
            relay_url: "wss://hive.example.org".to_string(),
            private_key_nsec: "  nsec1example  ".to_string(),
        };
        write_key_file(home, layout::Instance::Production, &record).expect("write");
        let path = layout::host_key_file_path(home, layout::Instance::Production);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "nsec1example",
            "the file holds the trimmed nsec and nothing else — no JSON, no newline"
        );
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        // And rewriting it (a re-commission) keeps the mode.
        write_key_file(home, layout::Instance::Production, &record).expect("rewrite");
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
