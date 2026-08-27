//! Provider identity records: one per relay URL, key in the OS keyring.
//!
//! The record is the *public* half — pubkey, instance id, NIP-OA attestation,
//! creation stamp — and is safe to read for status display. The nsec lives in
//! the same OS keyring service the human identity and managed agents use, under
//! its own key namespace, and falls back to the `0o600` JSON file only on
//! builds without a keyring backend (or during a keyring outage), exactly like
//! `managed_agents::storage`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::app_state::keyring_service;
use crate::managed_agents::atomic_write_json_restricted;
use crate::secret_store::SecretStore;
use crate::session_provider::{canonical_relay_key, session_provider_base_dir};

/// Current on-disk schema version. Bumped only on a breaking shape change.
pub(crate) const STORE_VERSION: u32 = 1;

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

/// One provisioned provider identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionProviderRecord {
    /// 64-character lowercase hex pubkey of the provider identity.
    pub provider_pubkey: String,

    /// Stable `cs-target` instance id. Derived from the pubkey prefix so it
    /// survives restarts with no extra persistence, and matches what the
    /// provider computes for itself when `BUZZ_CSP_INSTANCE_ID` is unset.
    pub instance_id: String,

    /// NIP-OA owner attestation, verbatim as minted: a JSON array of strings.
    /// Handed to the child as `BUZZ_AUTH_TAG` without re-encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_tag: Option<String>,

    /// RFC 3339 timestamp of provisioning.
    pub created_at: String,

    /// The relay this identity was attested against, as supplied.
    #[serde(default)]
    pub relay_url: String,

    /// Inline nsec. Empty (and omitted) whenever the keyring holds the key —
    /// the same `skip_serializing_if` mechanism managed agents use.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub private_key_nsec: String,
}

/// The whole record file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionProviderStore {
    pub version: u32,
    /// Keyed by [`canonical_relay_key`].
    #[serde(default)]
    pub providers: BTreeMap<String, CodingSessionProviderRecord>,

    /// How many agent processes the provider may hold at once, or `None` for
    /// the provider's own default.
    ///
    /// Machine-wide rather than per relay: the ceiling is about this
    /// computer's capacity, and a person running two communities has one set
    /// of CPUs. `Some(0)` is unlimited, matching
    /// `buzz_session_provider::config::UNLIMITED_MAX_SESSIONS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_sessions: Option<usize>,
    /// How long one turn may go with no word from the agent, in seconds.
    ///
    /// `None` leaves the provider's own default (15 minutes). Every line the
    /// adapter writes resets the clock, so this is a silence budget, not a
    /// runtime budget — and a single long tool call that reports nothing until
    /// it finishes is exactly what spends it. A turn that ran a 16-minute
    /// build was killed as "no agent activity" (reported 2026-08-24).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_idle_timeout_secs: Option<u64>,
    /// How many turns one crew session (an umbrella `sessionRef`) may start
    /// before the provider refuses further turns from anyone but its founder.
    ///
    /// `None` leaves the provider's own default (200). `Some(0)` removes the
    /// budget entirely. Machine-wide for the same reason as `max_sessions`:
    /// it bounds what this computer's agent processes will do unattended, and
    /// a person running two communities has one machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_budget: Option<u64>,
}

impl Default for CodingSessionProviderStore {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            providers: BTreeMap::new(),
            max_sessions: None,
            turn_idle_timeout_secs: None,
            turn_budget: None,
        }
    }
}

impl CodingSessionProviderStore {
    /// The record provisioned for `relay_url`, if any.
    pub(crate) fn get(&self, relay_url: &str) -> Option<&CodingSessionProviderRecord> {
        self.providers.get(&canonical_relay_key(relay_url))
    }

    /// Insert or replace the record for `relay_url`.
    pub(crate) fn upsert(&mut self, relay_url: &str, record: CodingSessionProviderRecord) {
        self.providers
            .insert(canonical_relay_key(relay_url), record);
    }
}

fn store_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(session_provider_base_dir(app)?.join("coding-session-provider.json"))
}

/// Read the record file, hydrating each nsec from the keyring.
///
/// A missing file is not an error — it is the un-provisioned steady state.
pub(crate) fn load_provider_store(app: &AppHandle) -> Result<CodingSessionProviderStore, String> {
    let path = store_path(app)?;
    if !path.exists() {
        return Ok(CodingSessionProviderStore::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read coding-session provider store: {error}"))?;
    let mut store: CodingSessionProviderStore = serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse coding-session provider store: {error}"))?;
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
                "buzz-desktop: coding-session provider {} has no key in JSON or keyring",
                record.provider_pubkey
            ),
            Err(error) => eprintln!(
                "buzz-desktop: coding-session provider {} key unavailable — keyring read failed \
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
                    "buzz-desktop: keyring read-back verify failed for coding-session provider {} \
                     — keeping the key inline",
                    record.provider_pubkey
                ),
            },
            Err(error) => eprintln!(
                "buzz-desktop: keyring write failed for coding-session provider {} ({error}) — \
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
            "buzz-desktop: failed to delete coding-session provider key {provider_pubkey}: {error}"
        );
    }
}
