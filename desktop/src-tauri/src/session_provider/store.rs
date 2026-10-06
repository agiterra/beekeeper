//! Provider identity records: one per relay URL, key in the OS keyring.
//!
//! The record *shape* is the launcher contract and lives in
//! `beekeeper_host_core::record`, because `beekeeper-host` reads the same file.
//! What stays here is the half that is genuinely the desktop's: hydrating each
//! nsec out of the OS keyring on read and pushing it back in on write.
//!
//! The nsec lives in the same OS keyring service the human identity and
//! managed agents use, under its own key namespace, and falls back to the
//! `0o600` JSON file only on builds without a keyring backend (or during a
//! keyring outage), exactly like `managed_agents::storage`. A headless host
//! has no keychain at all and resolves the key its own way — which is the
//! whole reason the shape and the secret store are separated.

use std::collections::BTreeMap;

use serde::Deserialize;
use tauri::{AppHandle, Manager};

pub(crate) use beekeeper_host_core::record::{
    CodingSessionProviderRecord, CodingSessionProviderStore, STORE_FILE_NAME, STORE_VERSION,
};

use crate::app_state::keyring_service;
use crate::managed_agents::atomic_write_json_restricted;
use crate::secret_store::SecretStore;
use crate::session_provider::{canonical_relay_key, session_provider_base_dir};

/// Keyring key for a provider nsec, namespaced away from `"identity"` (the
/// human) and `"agent:<pubkey>"` (managed agents), which share the service.
fn provider_keyring_name(provider_pubkey: &str) -> String {
    format!("coding-session-provider:{provider_pubkey}")
}

/// The provider secret store, or `None` on a build with no keyring backend —
/// in which case the nsec stays inline in the `0o600` record file.
///
/// Uses `SecretStore::shared` so this module, the identity store, and the agent
/// store share one instance, one cache, and one mutex. They all write into a
/// single keychain blob; separate instances would race last-writer-wins.
fn provider_secret_store() -> Option<&'static SecretStore> {
    if cfg!(feature = "system-keyring") {
        Some(SecretStore::shared(keyring_service()))
    } else {
        None
    }
}

/// Public, non-secret provider inventory used by team readiness.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CodingSessionProviderReadinessStore {
    pub version: u32,
    pub providers: BTreeMap<String, CodingSessionProviderReadinessRecord>,
    pub max_sessions: Option<usize>,
    pub turn_idle_timeout_secs: Option<u64>,
    pub turn_budget: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodingSessionProviderReadinessRecord {
    pub provider_pubkey: String,
    pub instance_id: String,
    pub auth_tag_present: bool,
    pub auth_tag_owner: Option<String>,
    pub auth_tag_invalid: bool,
    pub auth_tag_owner_mismatch: bool,
    pub created_at: String,
    pub relay_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodingSessionProviderReadinessWireStore {
    version: u32,
    #[serde(default)]
    providers: BTreeMap<String, CodingSessionProviderReadinessWireRecord>,
    #[serde(default)]
    max_sessions: Option<usize>,
    #[serde(default)]
    turn_idle_timeout_secs: Option<u64>,
    #[serde(default)]
    turn_budget: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodingSessionProviderReadinessWireRecord {
    provider_pubkey: String,
    instance_id: String,
    #[serde(default)]
    auth_tag: Option<String>,
    created_at: String,
    #[serde(default)]
    relay_url: String,
}

impl CodingSessionProviderReadinessWireStore {
    fn into_readiness(self, expected_owner: Option<&str>) -> CodingSessionProviderReadinessStore {
        CodingSessionProviderReadinessStore {
            version: self.version,
            providers: self
                .providers
                .into_iter()
                .map(|(key, record)| {
                    let auth = crate::readiness_auth::inspect_auth_tag(
                        record.auth_tag.as_deref(),
                        &record.provider_pubkey,
                        expected_owner,
                    );
                    (
                        key,
                        CodingSessionProviderReadinessRecord {
                            provider_pubkey: record.provider_pubkey,
                            instance_id: record.instance_id,
                            auth_tag_present: auth.present,
                            auth_tag_owner: auth.verified_owner,
                            auth_tag_invalid: auth.invalid,
                            auth_tag_owner_mismatch: auth.owner_mismatch,
                            created_at: record.created_at,
                            relay_url: record.relay_url,
                        },
                    )
                })
                .collect(),
            max_sessions: self.max_sessions,
            turn_idle_timeout_secs: self.turn_idle_timeout_secs,
            turn_budget: self.turn_budget,
        }
    }
}

impl CodingSessionProviderReadinessStore {
    pub(crate) fn get(&self, relay_url: &str) -> Option<&CodingSessionProviderReadinessRecord> {
        self.providers.get(&canonical_relay_key(relay_url))
    }
}

/// Resolve the provider record without creating `session-provider/`.
pub(crate) fn provider_store_path_readonly(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?
        .join("session-provider")
        .join(STORE_FILE_NAME))
}

/// Read non-secret provider metadata from an explicit path.
pub(crate) fn load_provider_readiness_store_from(
    path: &std::path::Path,
    expected_owner: Option<&str>,
) -> Result<CodingSessionProviderReadinessStore, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CodingSessionProviderReadinessStore::default());
        }
        Err(error) => {
            return Err(format!(
                "failed to read coding-session provider store: {error}"
            ));
        }
    };
    if file
        .metadata()
        .map_err(|error| format!("failed to inspect coding-session provider store: {error}"))?
        .len()
        > 1024 * 1024
    {
        return Err("coding-session provider store exceeds readiness limit".into());
    }
    let wire = serde_json::from_reader::<_, CodingSessionProviderReadinessWireStore>(file)
        .map_err(|error| format!("failed to parse coding-session provider store: {error}"))?;
    if wire.version != STORE_VERSION {
        return Err(format!(
            "unsupported provider store version: {}",
            wire.version
        ));
    }
    if wire.providers.len() > 256 {
        return Err("coding-session provider store has too many records".into());
    }
    for (key, record) in &wire.providers {
        let valid_pubkey = record.provider_pubkey.len() == 64
            && record
                .provider_pubkey
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
        if !valid_pubkey {
            return Err("provider pubkey must be 64-character lowercase hex".into());
        }
        if record.instance_id.is_empty()
            || record.instance_id.len() > 128
            || !record
                .instance_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("provider instanceId is invalid".into());
        }
        if chrono::DateTime::parse_from_rfc3339(&record.created_at).is_err() {
            return Err("provider createdAt is not RFC 3339".into());
        }
        if record.relay_url.trim().is_empty()
            || !matches!(url::Url::parse(&record.relay_url), Ok(url) if matches!(url.scheme(), "ws" | "wss"))
            || canonical_relay_key(&record.relay_url) != *key
        {
            return Err("provider relay coordinates are invalid".into());
        }
    }
    Ok(wire.into_readiness(expected_owner))
}

/// Read provider metadata without secret hydration or filesystem mutation.
pub(crate) fn load_provider_readiness_store(
    app: &AppHandle,
    expected_owner: Option<&str>,
) -> Result<CodingSessionProviderReadinessStore, String> {
    load_provider_readiness_store_from(&provider_store_path_readonly(app)?, expected_owner)
}

fn store_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(session_provider_base_dir(app)?.join(STORE_FILE_NAME))
}

/// Read the record file, hydrating each nsec from the keyring.
///
/// A missing file is not an error — it is the un-provisioned steady state.
pub(crate) fn load_provider_store(app: &AppHandle) -> Result<CodingSessionProviderStore, String> {
    let mut store = beekeeper_host_core::record::load_provider_store_from(&store_path(app)?)?;
    hydrate_keys(&mut store);
    Ok(store)
}

/// Fill in empty inline nsecs from the keyring.
///
/// A keyring read error is logged and left as an empty key: the caller's spawn
/// path refuses to start without an identity, which is the correct behavior
/// during an outage. Silently minting a replacement key would strand every
/// event already signed by the real one.
fn hydrate_keys(store: &mut CodingSessionProviderStore) {
    let Some(secrets) = provider_secret_store() else {
        return;
    };
    for record in store.providers.values_mut() {
        if !record.private_key_nsec.is_empty() {
            continue;
        }
        match secrets.load(&provider_keyring_name(&record.provider_pubkey)) {
            Ok(Some(nsec)) => record.private_key_nsec = nsec,
            Ok(None) => eprintln!(
                "beekeeper-desktop: coding-session provider {} has no key in JSON or keyring",
                record.provider_pubkey
            ),
            Err(error) => eprintln!(
                "beekeeper-desktop: coding-session provider {} key unavailable — keyring read failed \
                 ({error}); the provider will not start until the keyring is reachable",
                record.provider_pubkey
            ),
        }
    }
}

/// Persist the record file, moving each nsec into the keyring first.
///
/// The inline copy is blanked only after a verified keyring write, so a
/// keyring outage degrades to "key stays in the 0o600 file" rather than
/// "key is lost".
pub(crate) fn save_provider_store(
    app: &AppHandle,
    store: &CodingSessionProviderStore,
) -> Result<(), String> {
    let mut to_write = store.clone();
    persist_keys(&mut to_write);
    let payload = serde_json::to_vec_pretty(&to_write)
        .map_err(|error| format!("failed to serialize coding-session provider store: {error}"))?;
    atomic_write_json_restricted(&store_path(app)?, &payload)
}

fn persist_keys(store: &mut CodingSessionProviderStore) {
    let Some(secrets) = provider_secret_store() else {
        return;
    };
    for record in store.providers.values_mut() {
        if record.private_key_nsec.is_empty() {
            continue;
        }
        let name = provider_keyring_name(&record.provider_pubkey);
        match secrets.store(&name, &record.private_key_nsec) {
            Ok(()) => match secrets.verify_stored_raw(&name, &record.private_key_nsec) {
                Ok(true) => record.private_key_nsec.clear(),
                _ => eprintln!(
                    "beekeeper-desktop: keyring read-back verify failed for coding-session provider {} \
                     — keeping the key inline",
                    record.provider_pubkey
                ),
            },
            Err(error) => eprintln!(
                "beekeeper-desktop: keyring write failed for coding-session provider {} ({error}) — \
                 keeping the key inline",
                record.provider_pubkey
            ),
        }
    }
}

/// Best-effort keyring cleanup for a retired provider identity.
#[allow(dead_code)]
pub(crate) fn delete_provider_key(provider_pubkey: &str) {
    let Some(secrets) = provider_secret_store() else {
        return;
    };
    if let Err(error) = secrets.delete(&provider_keyring_name(provider_pubkey)) {
        eprintln!(
            "beekeeper-desktop: failed to delete coding-session provider key {provider_pubkey}: {error}"
        );
    }
}
