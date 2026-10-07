//! Provider identity records — the contract between a launcher and the app.
//!
//! The record is the *public* half of a provisioned coding-session provider
//! identity: pubkey, instance id, NIP-OA attestation, creation stamp. It is
//! safe to read for status display, and it is what a launcher needs in order
//! to build the child's environment.
//!
//! This module deliberately knows nothing about keyrings. The desktop app
//! hydrates `private_key_nsec` from its OS keyring after reading; a headless
//! host resolves the key its own way (see `beekeeper-host`). Putting the shape here
//! and the secret resolution at each end is what lets two processes agree on
//! the file without agreeing on a secret store — the desktop has a keychain
//! and a server does not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Current on-disk schema version. Bumped only on a breaking shape change.
pub const STORE_VERSION: u32 = 1;

/// Filename of the record store inside the `session-provider` directory.
pub const STORE_FILE_NAME: &str = "coding-session-provider.json";

/// Canonical key for the per-relay record map.
///
/// Records are keyed by relay because one machine can be pointed at several
/// communities, and a provider identity is only meaningful against the relay
/// whose owner attested it. Normalization is deliberately conservative — case
/// folding plus trailing-slash removal — so an operator typing the same relay
/// two ways does not mint two identities.
pub fn canonical_relay_key(relay_url: &str) -> String {
    relay_url.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Whether `value` is a 64-character lowercase hex pubkey.
///
/// Every path that turns a pubkey into a directory name or a keyring key goes
/// through this first: the pubkey is attacker-adjacent input in the sense that
/// it arrives from a file on disk, and `..` in a state-dir name is a different
/// bug than a malformed record.
pub fn is_lowercase_hex_pubkey(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// One provisioned provider identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionProviderRecord {
    /// 64-character lowercase hex pubkey of the provider identity.
    pub provider_pubkey: String,

    /// Stable `cs-target` instance id. Derived from the pubkey prefix so it
    /// survives restarts with no extra persistence, and matches what the
    /// provider computes for itself when `BEEKEEPER_CSP_INSTANCE_ID` is unset.
    pub instance_id: String,

    /// NIP-OA owner attestation, verbatim as minted: a JSON array of strings.
    /// Handed to the child as `BEEKEEPER_AUTH_TAG` without re-encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_tag: Option<String>,

    /// RFC 3339 timestamp of provisioning.
    pub created_at: String,

    /// The relay this identity was attested against, as supplied.
    #[serde(default)]
    pub relay_url: String,

    /// Inline nsec. Empty (and omitted) whenever a secret store holds the key.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub private_key_nsec: String,
}

/// The whole record file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionProviderStore {
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
    /// `beekeeper_session_provider::config::UNLIMITED_MAX_SESSIONS`.
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
    /// budget entirely. Machine-wide for the same reason as `max_sessions`.
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
    pub fn get(&self, relay_url: &str) -> Option<&CodingSessionProviderRecord> {
        self.providers.get(&canonical_relay_key(relay_url))
    }

    /// Insert or replace the record for `relay_url`.
    pub fn upsert(&mut self, relay_url: &str, record: CodingSessionProviderRecord) {
        self.providers
            .insert(canonical_relay_key(relay_url), record);
    }
}

/// Read the record file at `path`, leaving secrets exactly as stored.
///
/// A missing file is not an error — it is the un-provisioned steady state.
/// Callers that hold a secret store hydrate the empty nsecs afterwards; a
/// launcher with no keyring resolves the key by its own route instead.
pub fn load_provider_store_from(path: &Path) -> Result<CodingSessionProviderStore, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content)
            .map_err(|error| format!("failed to parse coding-session provider store: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(CodingSessionProviderStore::default())
        }
        Err(error) => Err(format!(
            "failed to read coding-session provider store: {error}"
        )),
    }
}

/// The per-identity state directory handed to the child as `BEEKEEPER_CSP_STATE_DIR`.
///
/// Keyed by **provider pubkey**, not by relay or by a fixed name. The
/// provider's durable outbox stores pre-signed events; rows signed by a key
/// other than the one currently loaded are dropped at startup. A rotated
/// identity therefore gets a structurally fresh state directory instead of a
/// directory full of undeliverable rows.
///
/// `base` is the `session-provider` directory. This function only computes the
/// path — it creates nothing, because a launcher reads the path from its
/// config rather than deriving it, and a reader must not have the side effect
/// of conjuring the directory it is asking about.
pub fn provider_state_dir_in(base: &Path, provider_pubkey: &str) -> Result<PathBuf, String> {
    if !is_lowercase_hex_pubkey(provider_pubkey) {
        return Err("provider pubkey must be 64-character lowercase hex".to_string());
    }
    Ok(base.join(provider_pubkey))
}

/// The supervised child's log file, beside the record store.
///
/// Derived here because two processes name this file: the host writes it and
/// the app offers it to a person. Two derivations would eventually point at
/// two paths, and the symptom is a log viewer that is always empty.
pub fn provider_log_path_in(base: &Path, provider_pubkey: &str) -> Result<PathBuf, String> {
    if !is_lowercase_hex_pubkey(provider_pubkey) {
        return Err("provider pubkey must be 64-character lowercase hex".to_string());
    }
    Ok(base.join("logs").join(format!("{provider_pubkey}.log")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_keys_fold_case_and_trailing_slashes_but_nothing_else() {
        assert_eq!(
            canonical_relay_key(" WSS://Hive.Example.Org/ "),
            "wss://hive.example.org"
        );
        // Not normalized: a path is part of the identity of a relay.
        assert_ne!(
            canonical_relay_key("wss://hive.example.org/relay"),
            canonical_relay_key("wss://hive.example.org")
        );
    }

    #[test]
    fn a_pubkey_that_could_escape_a_directory_is_refused() {
        assert!(is_lowercase_hex_pubkey(&"a".repeat(64)));
        assert!(!is_lowercase_hex_pubkey(&"A".repeat(64)));
        assert!(!is_lowercase_hex_pubkey("../etc"));
        assert!(!is_lowercase_hex_pubkey(&"a".repeat(63)));
        assert!(provider_state_dir_in(Path::new("/tmp"), "../etc").is_err());
    }

    #[test]
    fn the_log_path_is_derived_in_one_place_for_both_processes() {
        assert_eq!(
            provider_log_path_in(Path::new("/data/session-provider"), &"a".repeat(64))
                .expect("valid pubkey"),
            Path::new("/data/session-provider/logs").join(format!("{}.log", "a".repeat(64)))
        );
        assert!(provider_log_path_in(Path::new("/data"), "../etc").is_err());
    }

    #[test]
    fn a_missing_store_reads_as_unprovisioned_rather_than_as_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = load_provider_store_from(&dir.path().join(STORE_FILE_NAME))
            .expect("a missing file is the un-provisioned steady state");
        assert_eq!(store, CodingSessionProviderStore::default());
        assert!(store.providers.is_empty());
    }

    #[test]
    fn a_record_round_trips_with_the_nsec_omitted_when_a_secret_store_holds_it() {
        let mut store = CodingSessionProviderStore::default();
        store.upsert(
            "WSS://Hive.Example.Org/",
            CodingSessionProviderRecord {
                provider_pubkey: "b".repeat(64),
                instance_id: "cs-bbbb".to_string(),
                auth_tag: Some("[\"owner\",\"sig\"]".to_string()),
                created_at: "2026-09-30T00:00:00Z".to_string(),
                relay_url: "wss://hive.example.org".to_string(),
                private_key_nsec: String::new(),
            },
        );
        let json = serde_json::to_string(&store).expect("serialize");
        assert!(
            !json.contains("privateKeyNsec"),
            "an empty nsec must not be written at all: {json}"
        );
        let parsed: CodingSessionProviderStore = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed, store);
        assert!(parsed.get("wss://hive.example.org/").is_some());
    }
}
