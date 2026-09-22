//! Starting a local managed agent, with its preflight.
//!
//! Split out of `agents.rs` when that file was past the repository's 1000-line
//! ceiling. The seam is the one the module already had: `agents.rs` holds the
//! Tauri commands a person's click reaches (`create`, `stop`, `delete`, the
//! listing); this holds what starting a *local* agent has to check first, which
//! no command calls directly.

use super::*;

pub(crate) async fn start_local_agent_pairs_with_preflight(
    app: &AppHandle,
    state: &AppState,
    pubkey: &str,
    relay_urls: &[String],
) -> Result<ManagedAgentSummary, String> {
    let record_snapshot = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|e| e.to_string())?;
        load_managed_agents(app)?
            .into_iter()
            .find(|record| record.pubkey == pubkey)
            .ok_or_else(|| format!("agent {pubkey} not found"))?
    };
    if record_snapshot.backend != BackendKind::Local {
        return Err(format!("agent {pubkey} is not a local agent"));
    }
    let personas_for_preflight = load_personas(app).unwrap_or_default();
    let global_for_preflight =
        crate::managed_agents::load_global_agent_config(app).unwrap_or_default();
    let mesh_model_id =
        crate::managed_agents::effective_config::resolve_effective_relay_mesh_model_id(
            &record_snapshot,
            &personas_for_preflight,
            &global_for_preflight,
        );
    ensure_relay_mesh_for_record(app, mesh_model_id.as_deref(), false).await?;

    {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|e| e.to_string())?;
        let mut records = load_managed_agents(app)?;
        let record = find_managed_agent_mut(&mut records, pubkey)?;
        let personas = load_personas(app).unwrap_or_default();
        // Only write when the re-pin actually moved something: this save runs
        // BEFORE the spawn below, which can fail. See `resnapshot_linked_record`.
        if crate::managed_agents::resnapshot_linked_record(record, &personas) {
            record.updated_at = crate::util::now_iso();
            save_managed_agents(app, &records)?;
        }
        if let Some(saved_record) = records.iter().find(|record| record.pubkey == pubkey) {
            retain_managed_agent_pending(app, state, saved_record);
        }
    }

    let mut errors = Vec::new();
    for relay_url in relay_urls {
        if let Err(error) = crate::managed_agents::start_managed_agent_runtime_pair_lazy(
            pubkey.to_string(),
            relay_url.clone(),
            app.clone(),
        ) {
            errors.push(format!("{relay_url}: {error}"));
        }
    }
    if !errors.is_empty() {
        return Err(format!(
            "failed to restart one or more managed-agent runtime pairs: {}",
            errors.join("; ")
        ));
    }

    let _store_guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|e| e.to_string())?;
    let records = load_managed_agents(app)?;
    let runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|e| e.to_string())?;
    let record = records
        .iter()
        .find(|record| record.pubkey == pubkey)
        .ok_or_else(|| format!("agent {pubkey} not found"))?;
    summarize_from_disk(app, record, &runtimes)
}

pub(crate) async fn start_local_agent_with_preflight(
    app: &AppHandle,
    state: &AppState,
    pubkey: &str,
    allow_fresh_create_start: bool,
    expected_relay_url: Option<&str>,
    expected_signer_pubkey: Option<&str>,
    replay_floor_unix: Option<u64>,
) -> Result<ManagedAgentSummary, String> {
    let record_snapshot = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|e| e.to_string())?;
        let records = load_managed_agents(app)?;
        records
            .iter()
            .find(|record| record.pubkey == pubkey)
            .cloned()
            .ok_or_else(|| format!("agent {pubkey} not found"))?
    };

    if record_snapshot.backend != BackendKind::Local {
        return Err(format!("agent {pubkey} is not a local agent"));
    }
    let personas = load_personas(app).unwrap_or_default();
    let global = crate::managed_agents::load_global_agent_config(app).unwrap_or_default();
    let mesh_model_id =
        crate::managed_agents::effective_config::resolve_effective_relay_mesh_model_id(
            &record_snapshot,
            &personas,
            &global,
        );
    ensure_relay_mesh_for_record(app, mesh_model_id.as_deref(), allow_fresh_create_start).await?;

    // Bind the post-preflight workspace values once. The spawn below receives
    // these exact checked values instead of re-reading a mutable community or
    // identity after an await.
    let workspace_relay = crate::relay::bind_expected_relay_scope(
        expected_relay_url,
        crate::relay::relay_ws_url_with_override(state),
    )?;
    let workspace_owner =
        crate::relay::bind_expected_signer(expected_signer_pubkey, workspace_owner_hex(state)?)?;

    let _store_guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|e| e.to_string())?;
    let mut records = load_managed_agents(app)?;
    let mut runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|e| e.to_string())?;
    let record = find_managed_agent_mut(&mut records, pubkey)?;
    if record.backend != BackendKind::Local {
        return Err(format!("agent {pubkey} is no longer a local agent"));
    }
    // Re-snapshot the persona onto the record at every spawn so the agent always
    // starts with the current persona config (system_prompt, model, provider,
    // runtime). This clears the "out of date" drift badge without requiring a
    // delete+recreate. See `apply_persona_snapshot` for the precedence and
    // env-override self-heal rules.
    // Load personas once: used for snapshot application below and summary build
    // at the end — avoids a second disk read for the same file in the same call.
    let personas = load_personas(app).unwrap_or_default();
    if let Some(persona_id) = record.persona_id.clone() {
        match personas.iter().find(|p| p.id == persona_id) {
            Some(persona) => {
                crate::managed_agents::persona_events::apply_persona_snapshot(record, persona);
                record.updated_at = crate::util::now_iso();
            }
            None => {
                return Err(
                    crate::managed_agents::effective_config::ORPHANED_INSTANCE_ERROR.to_string(),
                );
            }
        }
    }
    start_managed_agent_process(
        app,
        record,
        &mut runtimes,
        Some(workspace_owner.as_str()),
        Some(workspace_relay.as_str()),
        replay_floor_unix,
    )?;
    save_managed_agents(app, &records)?;
    if let Some(saved_record) = records.iter().find(|r| r.pubkey == pubkey) {
        retain_managed_agent_pending(app, state, saved_record);
    }
    let record = records
        .iter()
        .find(|record| record.pubkey == pubkey)
        .ok_or_else(|| format!("agent {pubkey} not found"))?;
    build_managed_agent_summary(
        app,
        record,
        &runtimes,
        &personas,
        &load_teams(app).unwrap_or_default(),
        &crate::managed_agents::load_global_agent_config(app).unwrap_or_default(),
    )
}
