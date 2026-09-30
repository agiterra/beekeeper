use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::path::Path;

use serde::de::IgnoredAny;
use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::agent_host::AgentHost;
use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::{
    load_workdir_store_readonly, CodingSessionWorkdirStore,
};
use crate::managed_agents::{load_managed_agent_readiness_metadata, ManagedAgentReadinessMetadata};
use crate::session_provider::runtimes::{runtime_readiness_metadata, StrictRuntimeDiagnostic};
use crate::session_provider::status::{
    CodingSessionProviderProcessState, CodingSessionProviderStatus,
};
use crate::session_provider::store::{
    load_provider_readiness_store, CodingSessionProviderReadinessStore,
};

const MAX_READINESS_GLOBAL_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BRIDGE_LABEL_BYTES: usize = 80;

#[derive(Deserialize)]
struct ReadinessTrustProjection {
    #[serde(default, rename = "allowed-bridge-pubkeys")]
    allowed_bridge_pubkeys: Vec<ReadinessBridgeIdentity>,
    #[serde(flatten)]
    _ignored: BTreeMap<String, IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadinessBridgeIdentity {
    pubkey: String,
    #[serde(default)]
    label: String,
}

pub(super) fn load_allowed_bridge_pubkeys_readonly_from(
    path: &Path,
) -> Result<Vec<String>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("failed to open readiness trust config: {error}")),
    };
    let size = file
        .metadata()
        .map_err(|error| format!("failed to inspect readiness trust config: {error}"))?
        .len();
    if size > MAX_READINESS_GLOBAL_CONFIG_BYTES {
        return Err("readiness trust config exceeds the metadata-only limit".into());
    }
    let projection: ReadinessTrustProjection = serde_json::from_reader(file)
        .map_err(|error| format!("failed to parse readiness trust config: {error}"))?;
    let mut seen = HashSet::new();
    let mut pubkeys = Vec::with_capacity(projection.allowed_bridge_pubkeys.len());
    for entry in projection.allowed_bridge_pubkeys {
        if entry.pubkey.len() != 64
            || !entry
                .pubkey
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !seen.insert(entry.pubkey.clone())
            || entry.label.as_bytes().contains(&0)
            || entry.label.len() > MAX_BRIDGE_LABEL_BYTES
        {
            return Err("readiness trust config contains an invalid provider identity".into());
        }
        pubkeys.push(entry.pubkey);
    }
    pubkeys.sort();
    Ok(pubkeys)
}

pub(super) fn load_allowed_bridge_pubkeys_readonly(app: &AppHandle) -> Result<Vec<String>, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?
        .join("agents")
        .join("global-agent-config.json");
    load_allowed_bridge_pubkeys_readonly_from(&path)
}

pub(super) trait ReadinessHost {
    fn owner_pubkey(&self) -> Result<String, String>;
    fn workdirs(&self) -> Result<CodingSessionWorkdirStore, String>;
    fn agents(
        &self,
        expected_owner: Option<&str>,
    ) -> Result<Vec<ManagedAgentReadinessMetadata>, String>;
    fn live_agent_pubkeys(&self) -> Result<HashSet<String>, String>;
    fn runtimes(&self) -> Vec<StrictRuntimeDiagnostic>;
    fn provider_store(
        &self,
        expected_owner: Option<&str>,
    ) -> Result<CodingSessionProviderReadinessStore, String>;
    fn relay_url(&self) -> String;
    fn provider_process(&self, pubkey: &str) -> CodingSessionProviderProcessState;
}

pub(super) struct AppReadinessHost<'a>(pub(super) &'a AppHandle);

impl ReadinessHost for AppReadinessHost<'_> {
    fn owner_pubkey(&self) -> Result<String, String> {
        self.0
            .state::<AppState>()
            .signing_public_key_for_readiness()
    }

    fn workdirs(&self) -> Result<CodingSessionWorkdirStore, String> {
        load_workdir_store_readonly(self.0)
    }

    fn agents(
        &self,
        expected_owner: Option<&str>,
    ) -> Result<Vec<ManagedAgentReadinessMetadata>, String> {
        load_managed_agent_readiness_metadata(self.0, expected_owner)
    }

    fn live_agent_pubkeys(&self) -> Result<HashSet<String>, String> {
        self.0
            .state::<AppState>()
            .managed_agent_processes
            .lock()
            .map(|rows| {
                rows.iter()
                    .filter(|(_, runtime)| {
                        !matches!(
                            runtime.lifecycle,
                            crate::managed_agents::ManagedAgentRuntimeLifecycle::Failed
                                | crate::managed_agents::ManagedAgentRuntimeLifecycle::Stopped
                        )
                    })
                    .map(|(key, _)| key.pubkey.clone())
                    .collect()
            })
            .map_err(|_| "managed-agent process inventory is unavailable".into())
    }

    fn runtimes(&self) -> Vec<StrictRuntimeDiagnostic> {
        runtime_readiness_metadata()
    }

    fn provider_store(
        &self,
        expected_owner: Option<&str>,
    ) -> Result<CodingSessionProviderReadinessStore, String> {
        load_provider_readiness_store(self.0, expected_owner)
    }

    fn relay_url(&self) -> String {
        crate::relay::relay_ws_url_with_override(&self.0.state::<AppState>())
    }

    /// Ask the agent host. Blocking, because `gather` is — see
    /// `AgentHost::snapshot_blocking`.
    ///
    /// The `pubkey` argument still matters: a host serving a *different*
    /// identity is not this record running, and reporting its live child as
    /// ours is how a launch gate passes over a provider that will never answer
    /// for this relay.
    fn provider_process(&self, pubkey: &str) -> CodingSessionProviderProcessState {
        let host = self.0.state::<AgentHost>();
        let snapshot = host.snapshot_blocking();
        let provider_matches = snapshot
            .status
            .as_ref()
            .is_some_and(|status| status.provider_pubkey == pubkey);
        let status = CodingSessionProviderStatus::from_snapshot(None, &host, snapshot);
        match status.process_state() {
            // A live child for another identity is not this one running.
            CodingSessionProviderProcessState::Live { .. } if !provider_matches => {
                CodingSessionProviderProcessState::HostUnreachable {
                    reason: "the agent host is serving a different provider identity than this                              relay's record names"
                        .to_string(),
                }
            }
            other => other,
        }
    }
}
