//! The destructive half of the project teardown: phases, in order.
//!
//! Split from the command module so neither file approaches the repository's
//! 1000-line ceiling, and because the ordering here deserves to be read on
//! its own. See [`crate::managed_agents::project_teardown`] for *why* the
//! order is what it is.

use std::collections::BTreeSet;

use super::{ProjectAgentTeardownReceipt, RetainedPath};
use crate::app_state::AppState;
use crate::managed_agents::project_teardown::{
    enumeration_clears_the_team, remote_deployed_agents, teardown_agents, teardown_definitions,
    teardown_team,
};
use crate::managed_agents::{
    default_agents::project_team_name, load_managed_agents, load_personas, save_managed_agents,
    save_personas,
};
use tauri::AppHandle;

/// Sessions this project's provider still holds open.
///
/// **Disclosed, never a refusal.** The provider retires a session only on a
/// relay-signed kind:40099 receipt naming it, or a founder-signed kind:5
/// carrying the record's own `genesis_ref` plus a confirmed-absent read
/// (`beekeeper-session-provider`'s `retirement`). A kind:5 naming the project
/// coordinate is neither, so this teardown cannot retire them and must not
/// pretend to. Refusing on every un-retired session would refuse almost
/// always — `!closed && !retired` means "open", not "running" — so the
/// honest answer is to name them where the person is deciding and let the
/// relay-side session deletes do the retiring.
///
/// Read as JSON rather than through the provider's own store, which would do
/// write-shaped work behind a running provider's back — the same seam, and
/// the same reasoning, as `seat_agents_clone`'s provider lookup.
///
/// Any failure to read answers "none recorded" rather than propagating: this
/// is a disclosure, and a provider that has never run is the ordinary case.
pub(super) fn live_project_sessions(
    app: &AppHandle,
    state: &AppState,
    project_ref: &str,
) -> Result<Vec<String>, String> {
    let relay_url = crate::relay::relay_ws_url_with_override(state);
    let store = crate::session_provider::store::load_provider_store(app)?;
    let Some(record) = store.get(&relay_url) else {
        return Ok(Vec::new());
    };
    let path = crate::session_provider::provider_state_dir(app, &record.provider_pubkey)?
        .join("state.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let Ok(snapshot) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(Vec::new());
    };
    Ok(open_project_sessions(&snapshot, project_ref))
}

/// The open sessions in a provider `state.json` snapshot, for one project.
///
/// Pure, so the key names can be pinned by a test: they are read out of
/// another process's file, and a rename upstream must fail here rather than
/// silently answer "none open" forever.
pub(super) fn open_project_sessions(
    snapshot: &serde_json::Value,
    project_ref: &str,
) -> Vec<String> {
    // `sessions` is a map keyed by session id, not an array — the shape
    // `seat_agents_clone`'s reader already pins. The id also lives on the
    // record as `sessionId`; the map key is used only as the fallback.
    let Some(sessions) = snapshot.get("sessions").and_then(|value| value.as_object()) else {
        return Vec::new();
    };
    let mut open: Vec<String> = sessions
        .iter()
        .filter(|(_, session)| {
            session.get("projectRef").and_then(|v| v.as_str()) == Some(project_ref)
                && session.get("closed").and_then(|v| v.as_bool()) != Some(true)
                && session.get("retired").is_none_or(|v| v.is_null())
        })
        .map(|(key, session)| {
            let id = session
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or(key.as_str());
            let role = session.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role.is_empty() {
                id.to_string()
            } else {
                format!("{id} ({role})")
            }
        })
        .collect();
    open.sort();
    open
}

/// Things the teardown leaves alone, each with the reason it is not ours.
pub(super) fn retained_paths(app: &AppHandle, project_ref: &str) -> Vec<RetainedPath> {
    let mut retained = Vec::new();
    if let Some(path) =
        crate::coding_sessions::workdir_store::project::recorded_project_checkout(app, project_ref)
    {
        retained.push(RetainedPath {
            path: path.display().to_string(),
            reason: "Your checkout of the project's code. It may hold uncommitted work, so \
                     nothing here removes it — only the record that it belongs to this project."
                .to_string(),
        });
    }
    if let Some(path) =
        crate::coding_sessions::workdir_store::agents_repo_path_for_project(app, project_ref)
    {
        retained.push(RetainedPath {
            path,
            reason: "Your clone of the project's agents repository, which holds its roles and \
                     plans. A real git checkout, so it is left where it is."
                .to_string(),
        });
    }
    retained.push(RetainedPath {
        path: "Installed role packs".to_string(),
        reason: "Packs are keyed by the agents repository, not the project, so another project \
                 on the same repository shares them. Removing them would cut that project's \
                 roles out from under it."
            .to_string(),
    });
    retained
}

/// Run the teardown. Everything fallible happens before anything destructive.
pub(super) fn execute(
    app: &AppHandle,
    state: &AppState,
    project_ref: &str,
    expect_agents: &[String],
) -> Result<ProjectAgentTeardownReceipt, String> {
    let project = super::normalized(project_ref)?;

    let _guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|_| "managed-agent storage lock is unavailable".to_string())?;

    // ── Phase 1: stage. Every read, every refusal. An error here leaves the
    // whole machine untouched and the command safe to retry.
    let teams = crate::managed_agents::load_teams(app)?;
    let team = teardown_team(&teams, &project_team_name(&project)).cloned();
    let mut agents = load_managed_agents(app)?;
    let doomed_records = teardown_agents(&agents, &project, team.as_ref());
    let doomed: BTreeSet<String> = doomed_records
        .iter()
        .map(|record| record.pubkey.clone())
        .collect();

    let expected: BTreeSet<String> = expect_agents.iter().cloned().collect();
    if expected != doomed {
        return Err(format!(
            "the set of agents changed since you were shown the list \
             (expected {}, found {}) — nothing was deleted; reopen the dialog and try again",
            expected.len(),
            doomed.len()
        ));
    }

    let remote = remote_deployed_agents(&agents, &doomed);
    if !remote.is_empty() {
        let names: Vec<&str> = remote.iter().map(|record| record.name.as_str()).collect();
        return Err(format!(
            "these agents are deployed to a remote provider and deleting their local records \
             would orphan the deployments: {}. Delete them one at a time first — nothing \
             was deleted.",
            names.join(", ")
        ));
    }

    if let Some(team) = team.as_ref() {
        // The invariant, checked rather than assumed: if this fails the
        // teardown would delete the agents and then refuse its own team
        // delete, stranding every definition permanently.
        if !enumeration_clears_the_team(&agents, &doomed, team) {
            return Err(
                "refusing: removing these agents would still leave the project team referenced, \
                 and its definitions would become undeletable. Nothing was deleted."
                    .to_string(),
            );
        }
    }

    let definitions = load_personas(app)?;
    let doomed_definitions: Vec<(String, String)> =
        teardown_definitions(&definitions, &agents, &doomed, team.as_ref())
            .iter()
            .map(|definition| {
                (
                    definition.id.clone(),
                    crate::managed_agents::persona_events::persona_d_tag(definition),
                )
            })
            .collect();
    // Read before the record is dropped: the archive request needs it and the
    // record is gone by the time the side effects run.
    let persona_ids: Vec<(String, Option<String>)> = doomed_records
        .iter()
        .map(|record| (record.pubkey.clone(), record.persona_id.clone()))
        .collect();

    // ── Phase 2: back up, or do not proceed.
    let backups = back_up_stores(app, &project)?;

    // ── Phase 3: stop. A stuck process is logged, never fatal — one of them
    // must not abandon a multi-agent teardown half-way.
    let mut skipped: Vec<String> = Vec::new();
    stop_doomed_processes(app, state, &mut agents, &doomed, &mut skipped);

    // ── Phase 4: commit, in the one order that works.
    agents.retain(|record| !doomed.contains(&record.pubkey));
    save_managed_agents(app, &agents)?;
    let agents_deleted: Vec<String> = doomed.iter().cloned().collect();

    // Directly, not through `delete_persona`: `validate_persona_deletion`
    // refuses every one of these because they are team-sourced, and that
    // refusal is right for a person deleting one agent. The teardown owns the
    // team, so it is the flow entitled to remove the team's definitions.
    let removing: BTreeSet<&str> = doomed_definitions
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    let mut remaining = definitions.clone();
    remaining.retain(|definition| !removing.contains(definition.id.as_str()));
    save_personas(app, &remaining)?;
    let definitions_removed: Vec<String> = doomed_definitions
        .iter()
        .map(|(id, _)| id.clone())
        .collect();

    let mut team_deleted = None;
    if let Some(team) = team.as_ref() {
        match crate::managed_agents::delete_team_with_cascade(app, &team.id) {
            Ok(_) => team_deleted = Some(team.name.clone()),
            Err(error) => skipped.push(format!("team \"{}\" ({error})", team.name)),
        }
    }

    // ── Phase 5: relay side effects, strictly after the records left disk.
    // A tombstone published for a record that then failed to delete would say
    // the agent is gone while this computer still runs it.
    for (pubkey, persona_id) in &persona_ids {
        crate::commands::purge_managed_agent_side_effects(
            app,
            state,
            pubkey,
            persona_id.as_deref(),
        );
    }
    for (_, d_tag) in &doomed_definitions {
        crate::commands::personas::tombstone_persona_pending(app, state, d_tag);
    }

    Ok(ProjectAgentTeardownReceipt {
        agents_deleted,
        definitions_removed,
        team_deleted,
        backups,
        // Derived, never assigned: a caller cannot report a clean delete over
        // a partial one.
        complete: skipped.is_empty(),
        skipped,
    })
}

/// Copy both stores beside themselves before the first destructive write.
///
/// Named per run rather than once: `create_restricted_backup_once` uses
/// `create_new`, so a fixed name would write once ever and silently give the
/// *second* project you delete no backup at all. Still through that helper,
/// so a same-second retry cannot overwrite a pristine copy with a half-torn
/// one.
///
/// `actor-seats.json` is deliberately not backed up — it carries live signing
/// keys for other sessions, and a durable copy is exactly the at-rest leak
/// its one-shot custody exists to avoid.
fn back_up_stores(app: &AppHandle, project_ref: &str) -> Result<Vec<String>, String> {
    let dir = crate::managed_agents::storage::managed_agents_base_dir(app)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let slug: String = project_ref
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let mut written = Vec::new();
    for name in ["managed-agents.json", "teams.json"] {
        let source = dir.join(name);
        let Ok(bytes) = std::fs::read(&source) else {
            continue;
        };
        let backup = dir.join(format!("{name}.pre-teardown-{slug}-{stamp}.bak"));
        crate::util::create_restricted_backup_once(&backup, &bytes)?;
        written.push(backup.display().to_string());
    }
    Ok(written)
}

/// Stop each doomed agent's process, holding the process lock per agent.
///
/// The process lock is taken and released around each stop rather than held
/// across the loop: `stop` polls for up to a second, and holding it for the
/// whole set would block every other agent operation for that long.
fn stop_doomed_processes(
    app: &AppHandle,
    state: &AppState,
    agents: &mut [crate::managed_agents::ManagedAgentRecord],
    doomed: &BTreeSet<String>,
    skipped: &mut Vec<String>,
) {
    for record in agents.iter_mut().filter(|r| doomed.contains(&r.pubkey)) {
        let Ok(mut runtimes) = state.managed_agent_processes.lock() else {
            skipped.push(format!("stopping \"{}\" (lock unavailable)", record.name));
            continue;
        };
        if let Err(error) =
            crate::managed_agents::stop_managed_agent_process(app, record, &mut runtimes)
        {
            // Logged, not fatal: see the phase comment above.
            skipped.push(format!("stopping \"{}\" ({error})", record.name));
        }
    }
}
