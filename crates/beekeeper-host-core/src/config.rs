//! `host.json` — what the host serves when no desktop is present.
//!
//! Today the provider is bound to the *active workspace relay*, chosen at
//! runtime by the desktop app. A host that starts at login has no workspace
//! and no Tauri `app_data_dir()`, so everything it cannot derive is written
//! down here by whoever commissioned it: the relay, the identity, the runtime
//! list, and — the one that matters most — the absolute path of the provider's
//! state directory.
//!
//! The host never *guesses* that path. The directory stays under the desktop's
//! app-data tree because several desktop modules write into it, and a launcher
//! that reconstructed the path from its own idea of where app data lives would
//! be right on exactly one machine.

use std::path::{Path, PathBuf};

use beekeeper_core::coding_session_runtime::RuntimeDescriptor;
use serde::{Deserialize, Serialize};

use crate::layout::Instance;

/// Current `host.json` schema version.
pub const HOST_CONFIG_VERSION: u32 = 1;

/// What the host was commissioned to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostConfig {
    pub version: u32,

    /// Which Beekeeper instance this host belongs to. Recorded rather than
    /// inferred so a config file read out of context still says which socket
    /// and which nest it goes with.
    pub instance: Instance,

    /// The relay to serve. A community switch rewrites this and asks the host
    /// to rebind, instead of stopping and starting a child the desktop owns.
    pub relay_url: String,

    /// The provisioned provider identity's pubkey. The host uses it to find
    /// the record, the state directory and the log file; it is not a secret.
    pub provider_pubkey: String,

    /// Absolute path of `session-provider/` — the directory holding the record
    /// store and the per-identity state directories.
    pub session_provider_base_dir: PathBuf,

    /// Absolute path of `BEEKEEPER_CSP_STATE_DIR` for this identity.
    ///
    /// Written out in full rather than derived from the base directory and the
    /// pubkey: if the desktop ever changes that derivation, a host reading an
    /// older config must keep pointing at the directory that actually holds
    /// the outbox, not at a fresh empty one.
    pub provider_state_dir: PathBuf,

    /// The runtime offer, as the desktop resolved it. `BEEKEEPER_CSP_RUNTIMES` is
    /// built from this.
    #[serde(default)]
    pub runtimes: Vec<RuntimeDescriptor>,

    /// Ceiling on concurrently live agent processes; `None` leaves the
    /// provider's own default in charge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_sessions: Option<usize>,
    /// Per-turn silence budget in seconds; `None` keeps the provider default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_idle_timeout_secs: Option<u64>,
    /// Turns one crew session may start; `None` keeps the provider default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_budget: Option<u64>,

    /// The `beekeeper-session-provider` binary to run, when the host must be told.
    ///
    /// `None` means "resolve it the usual way" — beside the host's own
    /// executable, then the workspace, then `PATH`. Set explicitly by an app
    /// that knows where its sidecars are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_command: Option<PathBuf>,

    /// RFC 3339 stamp of the write that produced this file, for the log line
    /// that says which commissioning a running host is acting on.
    pub written_at: String,
}

impl HostConfig {
    /// Read `host.json`, or `None` when the host has not been commissioned.
    ///
    /// A missing file is the un-commissioned steady state and must not read as
    /// an error: that is the difference between "no identity yet" and "your
    /// config is broken", and the menu bar shows different words for each.
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
        };
        let config: Self = serde_json::from_str(&content)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        config.validate()?;
        Ok(Some(config))
    }

    /// Refuse a config the host cannot act on, by name.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != HOST_CONFIG_VERSION {
            return Err(format!(
                "unsupported host config version {} (this host understands {HOST_CONFIG_VERSION})",
                self.version
            ));
        }
        if !crate::record::is_lowercase_hex_pubkey(&self.provider_pubkey) {
            return Err("providerPubkey must be 64-character lowercase hex".to_string());
        }
        match url::Url::parse(&self.relay_url) {
            Ok(url) if matches!(url.scheme(), "ws" | "wss") => {}
            _ => return Err(format!("relayUrl is not a ws/wss URL: {}", self.relay_url)),
        }
        if !self.provider_state_dir.is_absolute() {
            return Err("providerStateDir must be an absolute path".to_string());
        }
        if !self.session_provider_base_dir.is_absolute() {
            return Err("sessionProviderBaseDir must be an absolute path".to_string());
        }
        Ok(())
    }

    /// The record store this config's identity lives in.
    pub fn record_store_path(&self) -> PathBuf {
        self.session_provider_base_dir
            .join(crate::record::STORE_FILE_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> HostConfig {
        HostConfig {
            version: HOST_CONFIG_VERSION,
            instance: Instance::Production,
            relay_url: "wss://hive.example.org".to_string(),
            provider_pubkey: "c".repeat(64),
            session_provider_base_dir: PathBuf::from("/data/session-provider"),
            provider_state_dir: PathBuf::from("/data/session-provider/cccc"),
            runtimes: Vec::new(),
            max_sessions: None,
            turn_idle_timeout_secs: None,
            turn_budget: None,
            provider_command: None,
            written_at: "2026-09-30T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn an_absent_config_is_uncommissioned_not_broken() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            HostConfig::load(&dir.path().join("host.json")).expect("absent is not an error"),
            None
        );
    }

    #[test]
    fn a_config_round_trips_and_names_its_own_refusals() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("host.json");
        let config = valid();
        std::fs::write(&path, serde_json::to_vec_pretty(&config).expect("encode")).expect("write");
        assert_eq!(HostConfig::load(&path).expect("load"), Some(config.clone()));

        for (mutate, expected) in [
            (
                Box::new(|c: &mut HostConfig| c.version = 99) as Box<dyn Fn(&mut HostConfig)>,
                "unsupported host config version",
            ),
            (
                Box::new(|c: &mut HostConfig| c.provider_pubkey = "NOPE".into()),
                "providerPubkey must be",
            ),
            (
                Box::new(|c: &mut HostConfig| c.relay_url = "https://hive.example.org".into()),
                "relayUrl is not a ws/wss URL",
            ),
            (
                Box::new(|c: &mut HostConfig| c.provider_state_dir = "relative/path".into()),
                "providerStateDir must be an absolute path",
            ),
        ] {
            let mut broken = valid();
            mutate(&mut broken);
            let error = broken.validate().expect_err("must be refused");
            assert!(
                error.contains(expected),
                "expected {expected:?} in {error:?}"
            );
        }
    }

    #[test]
    fn the_record_store_sits_beside_the_state_directories() {
        assert_eq!(
            valid().record_store_path(),
            PathBuf::from("/data/session-provider/coding-session-provider.json")
        );
    }
}
