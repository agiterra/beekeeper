//! Re-apply a role agent's pack: the explicit recovery for a missing or
//! diverged definition pin (ledger 401). The definition goes back to the pack,
//! the agent is re-pinned, and execution settings found on the shared role are
//! applied to the agent through the ordinary `update_managed_agent`.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    app_state::AppState,
    managed_agents::{
        crew_roles::scan_role_packs,
        load_managed_agents, load_personas, managed_agents_base_dir,
        role_source::{plan_reapply, MovedExecution},
        save_managed_agents, save_personas, try_regenerate_nest, UpdateManagedAgentResponse,
    },
    util::now_iso,
};

/// What re-applying did. `apply_error` set means the role was restored and
/// re-pinned but the moved execution settings were not applied to the agent.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReapplyRoleSourceResponse {
    moved: MovedExecution,
    cleared: Vec<&'static str>,
    replaced: Vec<&'static str>,
    recovery_copy: String,
    applied: Option<UpdateManagedAgentResponse>,
    apply_error: Option<String>,
}

#[tauri::command]
pub async fn reapply_role_source(
    pubkey: String,
    discard_instruction_edits: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ReapplyRoleSourceResponse, String> {
    let (plan, recovery_copy) = tokio::task::spawn_blocking({
        let app = app.clone();
        let pubkey = pubkey.clone();
        move || {
            let state = app.state::<AppState>();
            let _store_guard = state
                .managed_agents_store_lock
                .lock()
                .map_err(|error| error.to_string())?;
            let mut agents = load_managed_agents(&app)?;
            let mut personas = load_personas(&app)?;
            let agent = agents
                .iter_mut()
                .find(|agent| agent.pubkey == pubkey)
                .ok_or_else(|| format!("agent {pubkey} not found"))?;
            let (Some(id), Some(dir), Some(name)) = (
                agent.persona_id.clone(),
                agent.persona_team_dir.clone(),
                agent.persona_name_in_team.clone(),
            ) else {
                return Err("this agent was not installed from a role pack".to_string());
            };
            let folder = dir
                .parent()
                .ok_or("this agent's role pack folder is unavailable")?;
            let scan = scan_role_packs(folder).map_err(|error| {
                format!("role pack folder {} unavailable: {error}", folder.display())
            })?;
            let pack = scan
                .packs
                .iter()
                .find(|pack| pack.dir == dir && pack.persona_name == name)
                .ok_or_else(|| format!("role pack {} no longer holds {name}", dir.display()))?;
            let definition = personas
                .iter_mut()
                .find(|definition| definition.id == id)
                .ok_or("this agent's role definition is unavailable")?;
            let plan = plan_reapply(definition, pack, discard_instruction_edits, &now_iso())?;

            let directory = managed_agents_base_dir(&app)?.join("role-source-recovery");
            beekeeper_host_core::atomic_write::create_dir_all_restricted(&directory)?;
            let copy = directory.join(format!(
                "{}-{}.json",
                &pubkey[..pubkey.len().min(16)],
                plan.definition.updated_at.replace(':', "-")
            ));
            crate::managed_agents::storage::atomic_write_json_restricted(
                &copy,
                &serde_json::to_vec_pretty(definition).map_err(|error| error.to_string())?,
            )?;

            *definition = plan.definition.clone();
            agent.system_prompt = Some(plan.definition.system_prompt.clone());
            agent.persona_source_version = Some(plan.pin.clone());
            agent.updated_at = now_iso();
            // Definitions first, as the installer writes them.
            save_personas(&app, &personas)?;
            save_managed_agents(&app, &agents)?;
            if let Some(record) = agents.iter().find(|agent| agent.pubkey == pubkey) {
                crate::commands::retain_managed_agent_pending(&app, &state, record);
            }
            Ok::<_, String>((plan, copy.display().to_string()))
        }
    })
    .await
    .map_err(|error| error.to_string())??;
    try_regenerate_nest(&app);

    let (applied, apply_error) = if plan.moved.is_empty() {
        (None, None)
    } else {
        // Only moved fields: `null` means "clear" to this request, not "keep".
        let mut fields = serde_json::Map::new();
        fields.insert("pubkey".into(), pubkey.clone().into());
        for (key, value) in [
            ("runtime", &plan.moved.runtime),
            ("model", &plan.moved.model),
            ("provider", &plan.moved.provider),
        ] {
            if let Some(value) = value {
                fields.insert(key.into(), value.clone().into());
            }
        }
        let input = serde_json::from_value(serde_json::Value::Object(fields))
            .map_err(|error| error.to_string())?;
        match crate::commands::update_managed_agent(input, app.clone(), state).await {
            Ok(response) => (Some(response), None),
            Err(error) => (None, Some(error)),
        }
    };
    // The outcome must survive a webview reload: it can name settings that
    // were moved off the role but not applied.
    eprintln!(
        "beekeeper-desktop: reapply_role_source {}: moved {:?}, cleared {:?}, replaced {:?}, applied {}, apply_error {:?}",
        &pubkey[..pubkey.len().min(16)],
        plan.moved,
        plan.cleared,
        plan.replaced,
        applied.is_some(),
        apply_error
    );
    Ok(ReapplyRoleSourceResponse {
        moved: plan.moved,
        cleared: plan.cleared,
        replaced: plan.replaced,
        recovery_copy,
        applied,
        apply_error,
    })
}
