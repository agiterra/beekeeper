//! Resolving the runtime-pair key a managed-agent record maps to.
//!
//! Lifted out of `runtime.rs` so the file stays inside the repo's size
//! ratchet; the logic is unchanged.

use tauri::AppHandle;

use crate::managed_agents::{ManagedAgentRecord, ManagedAgentRuntimeKey};

/// Resolve the runtime-pair key this record maps to for the active
/// workspace: always the active workspace relay (the legacy per-record relay
/// pin is ignored — see `effective_agent_relay_url`). Returns `None` for
/// records that cannot form a valid pair key yet (e.g. key-less agents that
/// mint keys on first start).
pub(crate) fn workspace_pair_key(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Option<ManagedAgentRuntimeKey> {
    use tauri::Manager;
    let state = app.state::<crate::app_state::AppState>();
    resolve_workspace_pair_key(
        &record.pubkey,
        &record.relay_url,
        &crate::relay::relay_ws_url_with_override(&state),
    )
}

/// Pure core of [`workspace_pair_key`]: workspace-relay resolution (legacy
/// record pins ignored) plus canonical key construction, kept `AppHandle`-free
/// so summary/stop scoping semantics are unit-testable.
pub(crate) fn resolve_workspace_pair_key(
    pubkey: &str,
    record_relay_url: &str,
    workspace_relay_url: &str,
) -> Option<ManagedAgentRuntimeKey> {
    let effective_relay =
        crate::relay::effective_agent_relay_url(record_relay_url, workspace_relay_url);
    ManagedAgentRuntimeKey::new(pubkey.to_string(), &effective_relay).ok()
}
