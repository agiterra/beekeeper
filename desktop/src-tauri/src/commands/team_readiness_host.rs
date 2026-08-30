use std::collections::HashSet;

use tauri::{AppHandle, Manager};

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::{
    load_workdir_store_readonly, CodingSessionWorkdirStore,
};
use crate::managed_agents::{load_managed_agent_readiness_metadata, ManagedAgentReadinessMetadata};
use crate::session_provider::runtimes::{runtime_readiness_metadata, StrictRuntimeDiagnostic};
use crate::session_provider::store::{
    load_provider_readiness_store, CodingSessionProviderReadinessStore,
};
use crate::session_provider::supervisor::{
    CodingSessionProviderProcessState, CodingSessionProviderState,
};

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

    fn provider_process(&self, pubkey: &str) -> CodingSessionProviderProcessState {
        self.0
            .state::<CodingSessionProviderState>()
            .readiness_process_state(pubkey)
    }
}
