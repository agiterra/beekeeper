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
        ensure_agent_nest, find_managed_agent_mut, leave_shared_home, load_managed_agents,
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
    let saved_app = app.clone();
    let saved_pubkey = pubkey.clone();
    let result = tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut records = load_managed_agents(&app)?;

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
        super::agents::summarize_host_record(&app, record)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))??;
    let state = saved_app.state::<AppState>();
    let record = load_managed_agents(&saved_app)?
        .into_iter()
        .find(|r| r.pubkey == saved_pubkey)
        .ok_or("agent not found")?;
    if record.backend == crate::managed_agents::BackendKind::Local {
        let relay = crate::relay::effective_agent_relay_url(
            &record.relay_url,
            &crate::relay::relay_ws_url_with_override(&state),
        );
        super::agents::headless_management::reconcile_host_agent(
            &saved_app,
            &state,
            &saved_pubkey,
            &relay,
            false,
        )
        .await?;
    }
    Ok(result)
}
