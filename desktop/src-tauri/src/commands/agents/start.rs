//! Managed-agent start command.
//!
//! The command binds a detached caller's relay and signer scope, selects the
//! local or provider start path, and schedules profile reconciliation after a
//! successful start.

use super::*;

#[tauri::command]
pub async fn start_managed_agent(
    pubkey: String,
    expected_relay_url: Option<String>,
    expected_signer_pubkey: Option<String>,
    replay_floor_unix: Option<u64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ManagedAgentSummary, String> {
    // Snapshot the workspace owner pubkey for the legacy auth_tag fallback.
    // Read outside the records lock to keep lock ordering simple.
    let owner_hex = workspace_owner_hex(&state)?;
    crate::relay::assert_expected_relay_scope(
        expected_relay_url.as_deref(),
        &crate::relay::relay_api_base_url_with_override(&state),
    )?;
    crate::relay::assert_expected_signer(expected_signer_pubkey.as_deref(), &owner_hex)?;
    let reconcile_relay = crate::relay::bind_expected_relay_scope(
        expected_relay_url.as_deref(),
        relay_ws_url_with_override(&state),
    )?;
    enum StartTarget {
        Local,
        Provider,
    }

    // Collect backend info under lock; async preflight/spawn happens below.
    // Also snapshot profile reconciliation data for the background task.
    let (target, reconcile_data) = {
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
        for pubkey in &exited_pubkeys {
            state.clear_agent_session_caches(pubkey);
        }

        let record = find_managed_agent_mut(&mut records, &pubkey)?;

        // Resolve the effective harness for the avatar-fallback derivation in
        // profile reconcile (the create-time snapshot may be empty or stale for
        // a persona-inherited harness).
        let reconcile_personas = load_personas(&app).unwrap_or_default();
        let mut reconcile = profile_reconcile_data(record, &reconcile_personas);
        reconcile.target_relay_url = Some(crate::relay::effective_agent_relay_url(
            &record.relay_url,
            reconcile_relay.as_str(),
        ));

        let target = if record.backend == BackendKind::Local {
            StartTarget::Local
        } else {
            // Validate the current effective payload while collecting the
            // target. The provider boundary rebuilds it after its own lock.
            build_deploy_payload(&app, &state, record)?;
            StartTarget::Provider
        };

        (target, reconcile)
    };

    let result = match target {
        StartTarget::Local => {
            start_local_agent_with_preflight(
                &app,
                &state,
                &pubkey,
                false,
                expected_relay_url.as_deref(),
                expected_signer_pubkey.as_deref(),
                replay_floor_unix,
            )
            .await
        }
        StartTarget::Provider => {
            deploy_to_provider(
                &app,
                &state,
                &pubkey,
                ProviderDeployOptions {
                    expected_relay_url: expected_relay_url.as_deref(),
                    expected_signer_pubkey: expected_signer_pubkey.as_deref(),
                    replay_floor_unix,
                },
            )
            .await?;

            // Return updated summary.
            let _store_guard = state
                .managed_agents_store_lock
                .lock()
                .map_err(|e| e.to_string())?;
            let records = load_managed_agents(&app)?;
            let runtimes = state
                .managed_agent_processes
                .lock()
                .map_err(|e| e.to_string())?;
            let record = records
                .iter()
                .find(|r| r.pubkey == pubkey)
                .ok_or_else(|| format!("agent {pubkey} not found"))?;
            summarize_from_disk(&app, record, &runtimes)
        }
    };

    // ── Profile reconciliation (fire-and-forget) ────────────────────────────
    // On successful start, spawn a background task to ensure the agent's kind:0
    // profile is published on the relay. This self-heals cases where the initial
    // profile sync at creation time failed silently. For legacy records (pre-PR-921)
    // with no persisted avatar, this also backfills the avatar from the relay.
    if result.is_ok()
        && state
            .managed_agent_profile_reconcile_enabled()
            .load(std::sync::atomic::Ordering::Acquire)
    {
        let reconcile_pubkey = pubkey.clone();
        let reconcile_app = app.clone();
        tauri::async_runtime::spawn(async move {
            use tauri::Manager;
            let state = reconcile_app.state::<AppState>();
            if let Err(e) =
                reconcile_agent_profile(&state, &reconcile_app, &reconcile_pubkey, &reconcile_data)
                    .await
            {
                eprintln!(
                    "beekeeper-desktop: profile reconciliation failed for agent {reconcile_pubkey}: {e}"
                );
            }
        });
    }

    result
}
