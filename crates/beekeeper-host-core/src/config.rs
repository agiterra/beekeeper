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

/// The variable `capabilities` is exported to the provider in, as a
/// comma-separated list (`gpu,toolchain:rust`). Kept byte-for-byte in step
/// with `HOST_CAPABILITIES_ENV` in
/// `crates/beekeeper-session-provider/src/action_steps.rs`.
pub const HOST_CAPABILITIES_VAR: &str = "BEEKEEPER_CSP_CAPABILITIES";

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
    /// Publish one worktree checkpoint before a `session.stop` reaches
    /// quiescence, when the tree changed since the last one
    /// (`docs/agent-host.md` § Checkpoint before stop). Exported as
    /// `BEEKEEPER_CSP_CHECKPOINT_ON_STOP=1`; off by default and omitted from
    /// the file when off, so an older host.json reads unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub checkpoint_on_stop: bool,

    /// Capability tags this host declares — `gpu`, `toolchain:rust`,
    /// `os:macos` — free strings a project's `run_on_host` steps name under
    /// `requires`. Carried on every kind:46022 claim the provider signs;
    /// the relay refuses a claim that does not cover a step's `requires`
    /// (`HOST_CAPABILITY_MISSING`, NIP-HS). Exported as
    /// [`HOST_CAPABILITIES_VAR`], comma-separated, only when non-empty, so a
    /// host that declares nothing reads and runs exactly as before. Nothing
    /// verifies the declaration: it is the commissioning operator's word.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,

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
        beekeeper_core::host_step::validate_host_capabilities("capabilities", &self.capabilities)?;
        Ok(())
    }

    /// The value of [`HOST_CAPABILITIES_VAR`] for the provider, or `None`
    /// when this host declares nothing — absent rather than empty, so an
    /// unset list and a host that never heard of the field read the same.
    #[must_use]
    pub fn capabilities_env_value(&self) -> Option<String> {
        if self.capabilities.is_empty() {
            None
        } else {
            Some(self.capabilities.join(","))
        }
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
            checkpoint_on_stop: false,
            capabilities: Vec::new(),
            provider_command: None,
            written_at: "2026-09-30T00:00:00Z".to_string(),
        }
    }

    /// Round 2 Lane F: `capabilities` round-trips through `host.json`, is
    /// omitted when empty, exports as one comma-separated variable, and a
    /// tag the relay would refuse on a claim is refused here first.
    #[test]
    fn capabilities_round_trip_export_and_are_validated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("host.json");
        let mut config = valid();
        assert_eq!(config.capabilities_env_value(), None);
        std::fs::write(&path, serde_json::to_vec_pretty(&config).expect("encode")).expect("write");
        assert!(
            !std::fs::read_to_string(&path)
                .expect("read")
                .contains("capabilities"),
            "a host that declares nothing writes no key"
        );

        config.capabilities = vec!["gpu".into(), "toolchain:rust".into()];
        std::fs::write(&path, serde_json::to_vec_pretty(&config).expect("encode")).expect("write");
        let loaded = HostConfig::load(&path)
            .expect("load")
            .expect("commissioned");
        assert_eq!(loaded, config);
        assert_eq!(
            loaded.capabilities_env_value().as_deref(),
            Some("gpu,toolchain:rust")
        );

        for (tags, expected) in [
            (vec!["GPU".to_owned()], "not a capability tag"),
            (vec!["gpu".to_owned(), "gpu".to_owned()], "twice"),
        ] {
            let mut broken = valid();
            broken.capabilities = tags;
            let error = broken.validate().expect_err("must be refused");
            assert!(
                error.contains(expected),
                "expected {expected:?} in {error:?}"
            );
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

    /// `host.json` carries a plugged runtime's descriptor whole: the
    /// variables it reads and the operator's label survive the round trip,
    /// and a known runtime written without them reads back without them.
    #[test]
    fn a_plugged_runtime_descriptor_round_trips_through_host_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("host.json");
        let mut config = valid();
        config.runtimes = vec![RuntimeDescriptor {
            instance_ref: "acme-primary".into(),
            driver: "acme-acp".into(),
            runtime: "acme".into(),
            agent_command: "/opt/acme/bin/acme-acp".into(),
            agent_args: vec!["--acp".into()],
            cli_env: None,
            default_model: "default".into(),
            allowed_models: vec!["default".into()],
            discover_models: false,
            capabilities: None,
            steer_idle_guard: None,
            title_model: Some(None),
            credential_env: vec!["ACME_TOKEN".into()],
            credential_source: Some("acme-vault".into()),
            execution: None,
            context: None,
        }];
        std::fs::write(&path, serde_json::to_vec_pretty(&config).expect("encode")).expect("write");
        let loaded = HostConfig::load(&path)
            .expect("load")
            .expect("commissioned");
        assert_eq!(loaded, config);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("\"credentialEnv\""));
        assert!(text.contains("\"credentialSource\""));

        // Written by a commissioning that predates the plug: still readable,
        // and the descriptor says it names no variables.
        let legacy = serde_json::json!({
            "version": HOST_CONFIG_VERSION,
            "instance": "production",
            "relayUrl": "wss://hive.example.org",
            "providerPubkey": "c".repeat(64),
            "sessionProviderBaseDir": "/data/session-provider",
            "providerStateDir": "/data/session-provider/cccc",
            "runtimes": [{
                "instanceRef": "claude-primary",
                "driver": "claude-agent-acp",
                "runtime": "claude",
                "agentCommand": "/opt/bin/claude-agent-acp"
            }],
            "writtenAt": "2026-09-30T00:00:00Z"
        });
        std::fs::write(&path, legacy.to_string()).expect("write");
        let loaded = HostConfig::load(&path)
            .expect("load")
            .expect("commissioned");
        assert!(loaded.runtimes[0].credential_env.is_empty());
        assert_eq!(loaded.runtimes[0].credential_source, None);
    }

    #[test]
    fn the_record_store_sits_beside_the_state_directories() {
        assert_eq!(
            valid().record_store_path(),
            PathBuf::from("/data/session-provider/coding-session-provider.json")
        );
    }
}
