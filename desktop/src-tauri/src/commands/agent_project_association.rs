//! The explicit, permanent association of a managed agent with a project.
//!
//! Distinct from borrowing, which this build does not support: it records the
//! same local association project setup records, republishes the agent's
//! kind:30177 (as a digest), and changes no relay ACL, channel membership or
//! role. The decision itself is the pure
//! [`crate::managed_agents::project_agent_association::decide_association`].

use tauri::{AppHandle, Manager};

use crate::{
    app_state::AppState,
    managed_agents::{
        load_managed_agents,
        project_agent_association::{
            decide_association, normalize_project_ref, AssociationDecision,
            ASSOCIATION_MALFORMED_PROJECT,
        },
        save_managed_agents, ManagedAgentSummary,
    },
    util::now_iso,
};

/// Associate the managed agent `pubkey` on this computer with the project
/// `project_ref` (`30621:<owner>:<dtag>`), returning its summary.
///
/// Refuses a malformed coordinate, an unknown agent, a builtin agent, a setup
/// actor, an agent without a primary role, and an agent already associated
/// with another project. The same project is a no-op that returns the agent.
#[tauri::command]
pub async fn associate_managed_agent_with_project(
    app: AppHandle,
    pubkey: String,
    project_ref: String,
) -> Result<ManagedAgentSummary, String> {
    tokio::task::spawn_blocking(move || {
        if normalize_project_ref(&project_ref).is_none() {
            return Err(ASSOCIATION_MALFORMED_PROJECT.to_string());
        }
        let pubkey = pubkey.trim().to_string();
        let state = app.state::<AppState>();
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut records = load_managed_agents(&app)?;
        let index = records
            .iter()
            .position(|record| record.pubkey == pubkey)
            .ok_or_else(|| format!("agent {pubkey} is not a managed agent on this computer"))?;
        if let AssociationDecision::Associate(project) =
            decide_association(&records[index], &project_ref)?
        {
            records[index].project_ref = Some(project);
            records[index].updated_at = now_iso();
            save_managed_agents(&app, &records)?;
        }
        // Content-diffed: a no-op association queues nothing new, and a
        // publish an earlier save missed is re-queued.
        super::agents::retain_managed_agent_pending(&app, &state, &records[index]);
        let runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|error| error.to_string())?;
        super::agents::summarize_from_disk(&app, &records[index], &runtimes)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}
