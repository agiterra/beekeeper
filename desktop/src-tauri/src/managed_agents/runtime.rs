//! Modern host management observation; no desktop executor.
use super::{
    known_acp_runtime, managed_agent_log_path, normalize_agent_args, ManagedAgentRecord,
    ManagedAgentSummary,
};
use tauri::AppHandle;
mod path;
pub(in crate::managed_agents) use path::build_augmented_path;
pub(crate) use path::{compose_path_entries, should_skip_claude_executable, should_use_inherited};
mod workspace_pair;
pub(crate) use workspace_pair::workspace_pair_key;
mod metadata;
#[cfg(test)]
pub(crate) use metadata::REPLAY_FLOOR_ENV_VAR;
pub(crate) use metadata::{
    resolve_session_title, runtime_metadata_env_vars, DISPLAY_NAME_ENV_VAR, SESSION_TITLE_ENV_VAR,
};
mod effective_summary;
#[cfg(test)]
mod test_fixtures;
/// Configure the underlying CLI for a runtime discovery probe; never launches it.
pub(crate) fn configure_runtime_cli(
    command: &mut std::process::Command,
    runtime: Option<&super::KnownAcpRuntime>,
) {
    let Some(runtime) = runtime else {
        return;
    };
    if runtime.id != "claude" {
        return;
    }
    if let Some(path) = runtime.underlying_cli.and_then(super::resolve_command) {
        if !should_skip_claude_executable(&path, cfg!(windows)) {
            command.env("CLAUDE_CODE_EXECUTABLE", path);
        }
    }
}

/// Classify an agent's persona against the live catalog for the Agents-menu
/// drift indicator. Returns `(out_of_date, orphaned)`.
///
/// Drift basis is the RECORD's `persona_source_version`, never the engram:
/// - persona_id set + persona present: out_of_date when the snapshot hash
///   differs from the persona's current content hash.
/// - persona_id set + persona gone: orphaned (no current hash to respawn into,
///   so never out_of_date — we must not tell the user to respawn into nothing).
/// - no persona_id: neither — a hand-built agent has no persona to drift from.
fn persona_drift_state(
    record: &ManagedAgentRecord,
    personas: &[crate::managed_agents::types::AgentDefinition],
) -> (bool, bool) {
    let Some(persona_id) = record.persona_id.as_deref() else {
        return (false, false);
    };
    let Some(persona) = personas.iter().find(|p| p.id == persona_id) else {
        return (false, true);
    };
    let current = crate::managed_agents::persona_events::persona_content_hash(
        &crate::managed_agents::persona_events::persona_event_content(persona),
    );
    let out_of_date = record
        .persona_source_version
        .as_deref()
        .is_some_and(|pinned| pinned != current);
    (out_of_date, false)
}

pub fn build_managed_agent_summary(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    personas: &[crate::managed_agents::types::AgentDefinition],
    teams: &[crate::managed_agents::TeamRecord],
    global_config: &crate::managed_agents::GlobalAgentConfig,
) -> Result<ManagedAgentSummary, String> {
    use crate::managed_agents::BackendKind;

    // Community-scoped truth: this summary describes the pair for the active
    // workspace relay. An agent running only in another community must read
    // as stopped here — matching by pubkey alone would show every community a
    // green light as long as any pair anywhere is alive.
    let pair_key = workspace_pair_key(app, record);
    let transfer = if record.backend == BackendKind::Local {
        pair_key.as_ref().and_then(|key| {
            let home = beekeeper_host_core::layout::home_dir().ok()?;
            beekeeper_host_core::ownership::for_pair(
                &home,
                crate::agent_host::instance(),
                &key.pubkey,
                &key.relay_url,
            )
            .ok()
            .flatten()
        })
    } else {
        None
    };
    let (host_owned, enabled_on_host) = if record.backend == BackendKind::Local {
        modern_host_summary(transfer.as_ref())
    } else {
        (false, record.start_on_app_launch)
    };
    let (status, pid, log_path) = if record.backend != BackendKind::Local {
        (
            if record.backend_agent_id.is_some() {
                "deployed"
            } else {
                "not_deployed"
            }
            .to_string(),
            None,
            String::new(),
        )
    } else {
        (
            if host_owned {
                "host_managed"
            } else {
                "recreation_required"
            }
            .to_string(),
            None,
            managed_agent_log_path(app, &record.pubkey)?
                .display()
                .to_string(),
        )
    };

    let (persona_out_of_date, persona_orphaned) = persona_drift_state(record, personas);
    let persona_source_state = super::role_source::source_state(
        record,
        record
            .persona_id
            .as_deref()
            .and_then(|id| personas.iter().find(|persona| persona.id == id)),
    );

    let (effective_model, effective_provider, effective_prompt, model_source) =
        effective_summary::summary_effective_fields(record, personas, global_config);
    let (effective_runtime, runtime_source) =
        effective_summary::summary_effective_runtime(record, personas, global_config);

    // Accepted host configuration and relay execution evidence own restart state.
    // A desktop process stamp cannot certify that state.
    let restart_diff = Vec::new();
    let needs_restart = false;

    // Resolve the effective harness via the single typed descriptor — same resolver
    // as spawn, so the UI reflects the persona's current harness (or explicit pin).
    let descriptor = crate::managed_agents::resolve_effective_harness_descriptor(
        record,
        personas,
        global_config,
    )
    .unwrap_or_else(|e| {
        // Dangling harness — surface the missing id so the UI tells the same
        // story as spawn (which refuses with a sentence), rather than silently
        // showing the default-command fallback as if the agent were healthy.
        let cmd = match crate::managed_agents::dangling_harness_id(&e) {
            Some(id) => crate::managed_agents::dangling_harness_display(id),
            None => crate::managed_agents::record_agent_command(record, personas),
        };
        let args = normalize_agent_args(&cmd, record.agent_args.clone());
        crate::managed_agents::readiness::EffectiveHarnessDescriptor {
            command: cmd,
            args,
            env: Default::default(),
        }
    });
    let effective_mcp_command = known_acp_runtime(&descriptor.command)
        .and_then(|r| r.mcp_command)
        .unwrap_or("")
        .to_string();

    Ok(ManagedAgentSummary {
        pubkey: record.pubkey.clone(),
        name: record.name.clone(),
        persona_id: record.persona_id.clone(),
        runtime: record.runtime.clone(),
        effective_runtime,
        runtime_source,
        team_id: record.team_id.clone(),
        home_role: record.home_role.clone(),
        project_ref: record.project_ref.clone(),
        project_public: record.project_public,
        carried_project_digest: record.carried_project_digest.clone(),
        // Computed against the caller's one `load_teams` — never a per-record
        // disk read. `false` is the honest answer for an agent whose pack this
        // computer cannot read: its seats carry no role skills.
        has_role_pack: crate::managed_agents::actor_seats::resolve_seat_pack(record, teams)
            .is_some(),
        agent_home: crate::managed_agents::agent_home(app, record),
        pack_refused_shared_home: crate::managed_agents::pack_refused_by_shared_home(app, record),
        relay_url: record.relay_url.clone(),
        acp_command: record.acp_command.clone(),
        agent_command: descriptor.command,
        agent_command_override: record.agent_command_override.clone(),
        agent_args: descriptor.args,
        mcp_command: effective_mcp_command,
        turn_timeout_seconds: record.turn_timeout_seconds,
        idle_timeout_seconds: record.idle_timeout_seconds,
        max_turn_duration_seconds: record.max_turn_duration_seconds,
        parallelism: record.parallelism,
        system_prompt: effective_prompt,
        avatar_url: record.avatar_url.clone(),
        model: effective_model,
        model_source,
        provider: effective_provider,
        persona_out_of_date,
        persona_orphaned,
        persona_source_state,
        needs_restart,
        restart_diff,
        env_vars: record.env_vars.clone(),
        backend: record.backend.clone(),
        backend_agent_id: record.backend_agent_id.clone(),
        status,
        pid,
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
        last_started_at: record.last_started_at.clone(),
        last_stopped_at: record.last_stopped_at.clone(),
        last_exit_code: record.last_exit_code,
        last_error: record.last_error.clone(),
        last_error_code: record.last_error_code,
        start_on_app_launch: enabled_on_host,
        auto_restart_on_config_change: record.auto_restart_on_config_change,
        log_path,
        respond_to: record.respond_to,
        respond_to_allowlist: record.respond_to_allowlist.clone(),
    })
}

pub fn find_managed_agent_mut<'a>(
    records: &'a mut [ManagedAgentRecord],
    pubkey: &str,
) -> Result<&'a mut ManagedAgentRecord, String> {
    records
        .iter_mut()
        .find(|record| record.pubkey == pubkey)
        .ok_or_else(|| format!("agent {pubkey} not found"))
}

/// Host ownership and enabled intent must come from canonical modern enrollment.
fn modern_host_summary(
    transfer: Option<&beekeeper_host_core::ownership::Transfer>,
) -> (bool, bool) {
    use beekeeper_host_core::ownership::{is_modern_enrollment, Phase};
    match transfer.filter(|transfer| is_modern_enrollment(transfer)) {
        Some(transfer) if transfer.phase == Phase::Adopted => (true, true),
        Some(transfer) if transfer.phase == Phase::HostStopped => (true, false),
        _ => (false, false),
    }
}

#[cfg(test)]
mod ownership_summary_tests {
    use super::*;
    use beekeeper_host_core::ownership::{Phase, Transfer};

    #[test]
    fn historical_adoption_never_projects_modern_host_ownership_or_enabled_state() {
        let operation = uuid::Uuid::new_v4().to_string();
        let mut held = Transfer {
            operation_id: operation.clone(),
            actor_pubkey: "actor".into(),
            relay_url: "ws://isolated.invalid".into(),
            legacy_nonce: uuid::Uuid::new_v4().to_string(),
            definition_event_id: "retained-head".into(),
            target: None,
            phase: Phase::Adopted,
        };
        assert_eq!(modern_host_summary(Some(&held)), (false, false));
        held.legacy_nonce = format!("fresh-identity:{operation}");
        assert_eq!(modern_host_summary(Some(&held)), (true, true));
        held.phase = Phase::HostStopped;
        assert_eq!(modern_host_summary(Some(&held)), (true, false));
        held.phase = Phase::Prepared;
        assert_eq!(modern_host_summary(Some(&held)), (false, false));
        assert_eq!(modern_host_summary(None), (false, false));
    }
}
