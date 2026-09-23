//! Remove what a project created on *this computer*.
//!
//! Deleting a project on the relay never touched its local identities, so
//! every deleted project left behind one managed agent per role — with its
//! signing key still in the keyring — a `Project team <coordinate>` team, and
//! one `crew-role:` definition per role.
//!
//! Two commands, not one with a `dry_run` flag, because the confirmation has
//! to show a truthful inventory *before* the user acts and hold it while they
//! decide. That is the shape `coding_session_list_orphan_seat_bundles` /
//! `coding_session_remove_orphan_seat_bundle` already uses, and the remover
//! there re-checks the listing's premise for the same reason this one does.
//!
//! The ordering, and why each step sits where it does, is in
//! [`crate::managed_agents::project_teardown`]. The short version: agents,
//! then definitions, then the team — any other order leaves rows that nothing
//! can ever delete.

use std::collections::BTreeSet;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::app_state::AppState;
use crate::managed_agents::project_teardown::{
    remote_deployed_agents, teardown_agents, teardown_definitions, teardown_team, TeardownAgent,
    TeardownDefinition, TeardownTeam,
};
use crate::managed_agents::{
    default_agents::project_team_name, load_managed_agents, load_personas,
};

#[path = "project_agent_teardown_execute.rs"]
mod execute_impl;
use execute_impl::{execute, live_project_sessions, retained_paths};

/// What a teardown would remove, and what it would refuse.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAgentTeardownPlan {
    /// The normalized project coordinate this plan is for.
    pub project_ref: String,
    pub team: Option<TeardownTeam>,
    pub agents: Vec<TeardownAgent>,
    pub definitions: Vec<TeardownDefinition>,
    /// Agents deployed to a remote provider. Their presence refuses the whole
    /// teardown: removing the local record would orphan a live deployment,
    /// and a cascade has no honest way to ask about each one.
    pub remote_deployed: Vec<TeardownAgent>,
    /// Sessions this project's provider still has open, named so the person
    /// deleting knows what they are about to strand. **Disclosed, not
    /// refused** — see the note on [`live_project_sessions`].
    pub live_sessions: Vec<String>,
    /// Paths this teardown deliberately does not remove, each with its reason.
    pub retained: Vec<RetainedPath>,
}

/// One thing the teardown leaves on disk, and why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedPath {
    pub path: String,
    pub reason: String,
}

/// What a teardown actually did.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAgentTeardownReceipt {
    pub agents_deleted: Vec<String>,
    pub definitions_removed: Vec<String>,
    pub team_deleted: Option<String>,
    /// Where the pre-teardown copies of the two stores were written.
    pub backups: Vec<String>,
    /// Anything that did not go, named. Never empty and `complete` at once.
    pub skipped: Vec<String>,
    /// True only when every skip list is empty.
    ///
    /// Derived rather than set, so a caller cannot report a clean delete over
    /// a partial one — the rule the delete dialog already keeps for foreign
    /// workflows.
    pub complete: bool,
}

/// Enumerate what a teardown would remove. Read-only.
#[tauri::command]
pub async fn plan_project_agent_teardown(
    project_ref: String,
    app: AppHandle,
) -> Result<ProjectAgentTeardownPlan, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        build_plan(&app, &state, &project_ref)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}

/// Remove the project's agents, definitions and team from this computer.
///
/// `expect_agents` is the plan's agent list. The set is re-enumerated under
/// the store lock and the teardown refuses if it has moved, so a set that
/// grew between the count and the click is never silently widened.
#[tauri::command]
pub async fn run_project_agent_teardown(
    project_ref: String,
    expect_agents: Vec<String>,
    app: AppHandle,
) -> Result<ProjectAgentTeardownReceipt, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let receipt = execute(&app, &state, &project_ref, &expect_agents)?;
        // Outside the lock. Without this the deleted agents stay in every
        // picker and list until some unrelated refresh, which reads as the
        // delete having silently failed.
        crate::managed_agents::try_regenerate_nest(&app);
        let _ = app.emit("agents-data-changed", ());
        Ok(receipt)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))?
}

/// Normalize, or refuse. A coordinate this computer cannot normalize must
/// never open a teardown scope of its own — the same refusal
/// `project_agent_pubkeys` makes.
pub(super) fn normalized(project_ref: &str) -> Result<String, String> {
    crate::managed_agents::project_agent_association::normalize_project_ref(project_ref).ok_or_else(
        || {
            crate::managed_agents::project_agent_association::ASSOCIATION_MALFORMED_PROJECT
                .to_string()
        },
    )
}

fn build_plan(
    app: &AppHandle,
    state: &AppState,
    project_ref: &str,
) -> Result<ProjectAgentTeardownPlan, String> {
    let project = normalized(project_ref)?;
    let live_sessions = live_project_sessions(app, state, &project).unwrap_or_default();

    let _guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|_| "managed-agent storage lock is unavailable".to_string())?;

    let teams = crate::managed_agents::load_teams(app)?;
    let team = teardown_team(&teams, &project_team_name(&project));
    let agents = load_managed_agents(app)?;
    let doomed_records = teardown_agents(&agents, &project, team);
    let doomed: BTreeSet<String> = doomed_records
        .iter()
        .map(|record| record.pubkey.clone())
        .collect();
    let definitions = load_personas(app)?;

    let retained = retained_paths(app, &project);
    Ok(ProjectAgentTeardownPlan {
        project_ref: project,
        team: team.map(|team| TeardownTeam {
            id: team.id.clone(),
            name: team.name.clone(),
        }),
        agents: doomed_records
            .iter()
            .map(|record| TeardownAgent {
                pubkey: record.pubkey.clone(),
                name: record.name.clone(),
            })
            .collect(),
        definitions: teardown_definitions(&definitions, &agents, &doomed, team)
            .iter()
            .map(|definition| TeardownDefinition {
                id: definition.id.clone(),
                display_name: definition.display_name.clone(),
                d_tag: crate::managed_agents::persona_events::persona_d_tag(definition),
            })
            .collect(),
        remote_deployed: remote_deployed_agents(&agents, &doomed)
            .iter()
            .map(|record| TeardownAgent {
                pubkey: record.pubkey.clone(),
                name: record.name.clone(),
            })
            .collect(),
        live_sessions,
        retained,
    })
}

#[cfg(test)]
#[path = "project_agent_teardown_tests.rs"]
mod tests;
