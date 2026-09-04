//! Moving one agent out of the shared home into a nest of its own.
//!
//! Every agent minted from now on gets its own nest
//! (`crate::managed_agents::agent_nest`). The agents already on this computer
//! keep the shared home they have been running in — their work, their
//! checkouts and their harness credentials are in that directory, and moving
//! them behind the operator's back would be a change they never asked for and
//! could not see. The agent card names the ones whose packs the shared home is
//! refusing; this command is the button beside that sentence.

use tauri::{AppHandle, Manager};

use crate::{
    app_state::AppState,
    managed_agents::{
        current_instance_id, ensure_agent_nest, find_managed_agent_mut, leave_shared_home,
        load_managed_agents, save_managed_agents, sync_managed_agent_processes,
        ManagedAgentSummary,
    },
};

/// Give one agent a nest of its own.
///
/// Creates the nest first and only then takes the agent off the shared-home
/// list, so a failure to create it leaves the agent exactly where it was rather
/// than pointing it at a directory that does not exist. Idempotent: an agent
/// already in its own nest is returned unchanged.
///
/// The agent's running process keeps the working directory it was spawned
/// with — a process's `cwd` cannot be changed from outside it — so the move
/// takes effect at the agent's next start, and the button says so.
#[tauri::command]
pub async fn give_agent_its_own_nest(
    pubkey: String,
    app: AppHandle,
) -> Result<ManagedAgentSummary, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut records = load_managed_agents(&app)?;
        let mut runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|error| error.to_string())?;

        let (sync_changed, exited_pubkeys) =
            sync_managed_agent_processes(&mut records, &mut runtimes, &current_instance_id(&app));
        if sync_changed {
            save_managed_agents(&app, &records)?;
        }
        for exited in &exited_pubkeys {
            state.clear_agent_session_caches(exited);
        }

        // Refuse an unknown agent before touching the disk.
        find_managed_agent_mut(&mut records, &pubkey)?;
        // The directory first, then the list that points at it: a failure to
        // create the nest leaves the agent exactly where it was.
        ensure_agent_nest(&app, &pubkey)?;
        leave_shared_home(&app, &pubkey)?;

        let record = records
            .iter()
            .find(|record| record.pubkey == pubkey)
            .ok_or_else(|| format!("agent {pubkey} not found"))?;
        super::agents::summarize_from_disk(&app, record, &runtimes)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}
