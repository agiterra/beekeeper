//! The explicit, permanent association of a managed agent with a project.
//!
//! Distinct from borrowing, which this build does not support: it records the
//! same local association project setup records, republishes the agent's
//! kind:30177 (as a digest), and changes no channel membership or role. The
//! local decision is the pure
//! [`crate::managed_agents::project_agent_association::decide_association`];
//! the authority to make it is proved from signed project state by
//! [`crate::managed_agents::project_association_authority`].
//!
//! Since ledger 173 the association also puts the agent on the project's
//! roster as a collaborator (kind 9010, [`crate::managed_agents::project_roster`])
//! when the viewer is the project's creator or an owner, so the agent can
//! write Pulse and to-dos under its own key. The roster step's outcome is
//! reported on the result beside the summary, never swallowed; the
//! association itself lands either way.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{
    app_state::AppState,
    managed_agents::{
        load_managed_agents,
        project_agent_association::{
            decide_association, normalize_project_ref, AssociationDecision,
            ASSOCIATION_MALFORMED_PROJECT,
        },
        project_association_authority::{read_association_authority, ASSOCIATION_IDENTITY_CHANGED},
        project_roster, save_managed_agents, ManagedAgentSummary,
    },
    util::now_iso,
};

/// The associated agent's summary plus what the roster step did.
///
/// The summary's own fields are flattened in, so a reader of the previous
/// shape (`ManagedAgentSummary`) still finds them; `rosterAdded` and
/// `rosterError` are the two additions.
#[derive(Debug, Clone, Serialize)]
pub struct AssociateManagedAgentWithProjectResult {
    #[serde(flatten)]
    pub agent: ManagedAgentSummary,
    /// The agent was put on the project's roster as a collaborator by this
    /// call. `false` when it was already there, or when `rosterError` says
    /// why it was not.
    #[serde(rename = "rosterAdded")]
    pub roster_added: bool,
    /// Why the agent is not on the roster after this call, in words.
    #[serde(rename = "rosterError")]
    pub roster_error: Option<String>,
}

/// Associate the managed agent `pubkey` on this computer with the project
/// `project_ref` (`30621:<owner>:<dtag>`), returning its summary and the
/// roster outcome.
///
/// First proves, from the project's signed head and relay-signed roster, that
/// the active signing identity is the project's creator or a current owner or
/// collaborator; it refuses when that state cannot be read. Then refuses a
/// malformed coordinate, an unknown agent, a builtin agent, a setup actor, an
/// agent without a primary role, and an agent already associated with another
/// project. The same project is a no-op that returns the agent, and it too
/// requires the authority check to pass. Records the project's visibility
/// with the association. Then puts the agent on the project's roster as a
/// collaborator — the relay admits that op only from the creator or an
/// owner, so a collaborator's association reports `rosterError` instead.
#[tauri::command]
pub async fn associate_managed_agent_with_project(
    app: AppHandle,
    pubkey: String,
    project_ref: String,
) -> Result<AssociateManagedAgentWithProjectResult, String> {
    if normalize_project_ref(&project_ref).is_none() {
        return Err(ASSOCIATION_MALFORMED_PROJECT.to_string());
    }
    let keys = app.state::<AppState>().signing_keys()?;
    // The relay reads hold no store lock: a slow relay never blocks other
    // agent writes. Any read or verification failure refuses here.
    let visibility =
        read_association_authority(&app.state::<AppState>(), &project_ref, &keys).await?;
    let reader = keys.public_key();
    let store_app = app.clone();
    let store_project = project_ref.clone();
    let agent_pubkey = pubkey.trim().to_string();
    let store_pubkey = agent_pubkey.clone();
    let agent = tokio::task::spawn_blocking(move || {
        let app = store_app;
        let pubkey = store_pubkey;
        let project_ref = store_project;
        let state = app.state::<AppState>();
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        if state.signing_keys()?.public_key() != reader {
            return Err(ASSOCIATION_IDENTITY_CHANGED.to_string());
        }
        let mut records = load_managed_agents(&app)?;
        let index = records
            .iter()
            .position(|record| record.pubkey == pubkey)
            .ok_or_else(|| format!("agent {pubkey} is not a managed agent on this computer"))?;
        let public = Some(visibility.is_public());
        let record = &mut records[index];
        let changed = match decide_association(record, &project_ref)? {
            AssociationDecision::Associate(project) => {
                record.project_ref = Some(project);
                record.project_public = public;
                true
            }
            AssociationDecision::AlreadyAssociated => {
                let changed = record.project_public != public;
                record.project_public = public;
                changed
            }
        };
        if changed {
            record.updated_at = now_iso();
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
    .map_err(|error| format!("spawn_blocking failed: {error}"))??;

    // The association landed; the roster is reported, never a refusal of it.
    let roster = project_roster::ensure_project_agents_on_roster(
        &app.state::<AppState>(),
        &keys,
        &project_ref,
        std::slice::from_ref(&agent_pubkey),
    )
    .await;
    Ok(AssociateManagedAgentWithProjectResult {
        agent,
        roster_added: !roster.added.is_empty(),
        roster_error: roster.error,
    })
}
