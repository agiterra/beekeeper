//! Desktop custody/bootstrap and signing adapters for sole host management.
//! Retained old data is reference only; no migration or desktop execution.
use super::*;
use beekeeper_agent_management::{Context, PrivateConfiguration, RelayTransport};
use beekeeper_host_core::{layout, ownership};
use nostr::Event;
use serde_json::{json, Value};
use tauri::Manager;
#[path = "host_configuration.rs"]
mod configuration;
#[path = "host_delete_intent.rs"]
mod deletion;
#[path = "host_enrollment.rs"]
mod enrollment;
use enrollment::{client_guard, pair_directory, private_bytes, save};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[path = "host_fresh_snapshot.rs"]
mod fresh_snapshot;
pub(crate) use fresh_snapshot::{
    enroll_fresh_snapshot, preflight_fresh_snapshot, reserve_fresh_snapshot,
};
#[path = "host_lifecycle.rs"]
mod lifecycle;
#[path = "host_stop_intent.rs"]
mod stop_intent;
use super::headless_transport::Transport;
pub(crate) use configuration::{
    reconcile_host_agent, refresh_accepted_host_agents, update_if_adopted,
};
pub(crate) use deletion::deletion_source;
pub(crate) use enrollment::{enroll_new_host_agent, require_clean_setup, reserve_fresh_host_agent};
#[cfg(test)]
pub(crate) use lifecycle::UnobservedHostAgent;
pub(crate) use lifecycle::{
    active_host_agent_pubkeys, delete_host_agent_snapshot, start_host_pair,
    start_host_pair_with_floor, stop_host_pair, HostAgentAvailability,
};

fn context() -> Result<Context, String> {
    Ok(Context {
        home: layout::home_dir()?,
        instance: crate::agent_host::instance(),
        native_home: std::env::var_os("HOME").map(std::path::PathBuf::from),
    })
}
fn record_for_pair(
    app: &AppHandle,
    actor: &str,
    relay: &str,
) -> Result<ManagedAgentRecord, String> {
    let state = app.state::<AppState>();
    let record = load_managed_agents(app)?
        .into_iter()
        .find(|r| r.pubkey == actor)
        .ok_or("managed identity is not in this client's custody")?;
    let effective = crate::relay::effective_agent_relay_url(
        &record.relay_url,
        &crate::relay::relay_ws_url_with_override(&state),
    );
    if beekeeper_host_core::record::canonical_relay_key(&effective)
        != beekeeper_host_core::record::canonical_relay_key(relay)
    {
        return Err("agent belongs to another community; no fallback or local conversion".into());
    }
    Ok(record)
}
async fn head(app: &AppHandle, relay: &str, actor: &str) -> Result<Event, String> {
    let transport = Transport::owner(app, relay)?;
    let owner = transport.keys().public_key();
    transport
        .query_all(json!({"kinds":[30177],"authors":[owner.to_hex()],"#d":[actor]}))
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Event>(v).ok())
        .filter(|e| {
            e.kind.as_u16() == 30177
                && e.pubkey == owner
                && e.verify().is_ok()
                && e.tags.iter().any(|t| t.as_slice() == ["d", actor])
        })
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| b.id.cmp(&a.id))
        })
        .ok_or_else(|| {
            "accepted owner definition is unavailable; repair publication before administration"
                .into()
        })
}
fn configuration_view(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<
    (
        beekeeper_core_pkg::managed_agent::ManagedAgentEventContent,
        PrivateConfiguration,
    ),
    String,
> {
    configuration_view_with_personas(app, record, &load_personas(app)?)
}
fn configuration_view_with_personas(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    personas: &[crate::managed_agents::AgentDefinition],
) -> Result<
    (
        beekeeper_core_pkg::managed_agent::ManagedAgentEventContent,
        PrivateConfiguration,
    ),
    String,
> {
    let global = crate::managed_agents::load_global_agent_config(app)?;
    let effective =
        crate::managed_agents::resolve_effective_harness_descriptor(record, personas, &global)?;
    let mut private =
        serde_json::to_value(record).map_err(|_| "cannot encode private configuration")?;
    strip_client_bookkeeping(&mut private);
    private["effective_command"] = json!(effective.command);
    private["runtime"] = json!(record
        .runtime
        .clone()
        .or_else(
            || crate::managed_agents::known_acp_runtime(&effective.command)
                .map(|known| known.id.to_string())
        )
        .unwrap_or_else(|| "custom".into()));
    if let Some(command) = crate::managed_agents::known_acp_runtime(&effective.command)
        .and_then(|known| known.mcp_command)
        .filter(|command| !command.trim().is_empty())
    {
        private["effective_mcp_command"] = json!(command);
    }
    private["effective_args"] = json!(effective.args);
    private["effective_env"] = json!(effective.env);
    private["effective_parallelism"] = json!(crate::managed_agents::effective_parallelism(
        &effective.command,
        record.parallelism
    ));
    private["enforced_owner_only"] = json!(crate::managed_agents::owner_only_access_build());
    Ok((
        crate::managed_agents::agent_events::agent_event_content(record),
        PrivateConfiguration { record: private },
    ))
}

/// Observe modern scoped host state; retained legacy records are never imported.
#[tauri::command]
pub async fn get_managed_agent_host_status(
    app: AppHandle,
    pubkey: String,
    local_only: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let relay = crate::relay::relay_ws_url_with_override(&state);
    let ctx = context()?;
    let config = beekeeper_host_core::config::HostConfig::load(&layout::host_config_path(
        &ctx.home,
        ctx.instance,
    ))?;
    let held = ownership::for_pair(&ctx.home, ctx.instance, &pubkey, &relay)?;
    let mut phase = held
        .as_ref()
        .filter(|h| ownership::is_modern_enrollment(h))
        .and_then(|h| match h.phase {
            ownership::Phase::Adopted => Some("adopted"),
            ownership::Phase::HostStopped => Some("hostStopped"),
            _ => None,
        });
    let record = record_for_pair(&app, &pubkey, &relay)?;
    let fresh_receipt = enrollment::fresh_reserved(&app, &pubkey, &relay)?;
    let fresh_pending = enrollment::fresh_pending(&app, &pubkey, &relay, held.as_ref())?;
    if fresh_pending {
        phase = None;
    }
    let legacy_conflict = enrollment::require_no_legacy_conflict(&app, &record, &relay).err();
    let ownership_pending_reason = if fresh_pending {
        Some("Fresh Create outcome is pending; retry its retained exact operation")
    } else if held.is_some() && phase.is_none() {
        Some("Retained ownership is pending or uncertain; diagnose its original operation, not recreation or duplicate admission")
    } else {
        None
    };
    let revision = if local_only.unwrap_or(false) {
        None
    } else {
        head(&app, &relay, &pubkey)
            .await
            .ok()
            .map(|e| e.id.to_hex())
    };
    let available = if local_only.unwrap_or(false) || phase.is_none() {
        None
    } else {
        observe_host_runtime_availability(&app, &pubkey, &relay).await
    };
    Ok(
        json!({"relayUrl":relay,"commissioned":config.is_some_and(|c|beekeeper_host_core::record::canonical_relay_key(&c.relay_url)==beekeeper_host_core::record::canonical_relay_key(&relay)),"revisionAvailable":revision.is_some(),"revision":revision,"ownershipPhase":phase,"legacyRecreationRequired":record.backend==BackendKind::Local&&held.is_none()&&!fresh_receipt,"legacyConflict":legacy_conflict,"ownershipPendingReason":ownership_pending_reason,"freshCreationPending":fresh_pending,"runtimeAvailable":available}),
    )
}

/// Read signed execution availability for an already-enrolled exact community.
/// Ownership, local processes and idle metadata never imply a live execution.
/// `None` discloses an unreadable/unconfirmed observation; `false` means no
/// current verified live lease was returned by the shared management service.
pub(crate) async fn observe_host_runtime_availability(
    app: &AppHandle,
    actor: &str,
    relay: &str,
) -> Option<bool> {
    let ctx = context().ok()?;
    stop_intent::require_host_ownership(&ctx, actor, relay).ok()?;
    let transport = Transport::owner(app, relay).ok()?;
    availability_from_transport(actor, &transport).await
}
async fn availability_from_transport(actor: &str, client: &impl RelayTransport) -> Option<bool> {
    let request = beekeeper_agent_management::LifecycleRequest {
        agent_pubkey: actor.into(),
        ..Default::default()
    };
    let observed = beekeeper_agent_management::status(&request, client)
        .await
        .ok()?;
    observed
        .get("responsibilities")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| row["runtime_available"] == true))
}
#[cfg(test)]
#[path = "host_runtime_availability_tests.rs"]
mod availability_tests;

/// Exclude custody and client lifecycle bookkeeping from executor configuration.
fn strip_client_bookkeeping(private: &mut Value) {
    for key in [
        "private_key_nsec",
        "privateKeyNsec",
        "auth_tag",
        "authTag",
        "updated_at",
        "updatedAt",
        "start_on_app_launch",
        "startOnAppLaunch",
        "runtime_pid",
        "runtimePid",
        "last_started_at",
        "lastStartedAt",
        "last_stopped_at",
        "lastStoppedAt",
        "last_exit_code",
        "lastExitCode",
        "last_error",
        "lastError",
        "last_error_code",
        "lastErrorCode",
    ] {
        if let Some(object) = private.as_object_mut() {
            object.remove(key);
        }
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn host_enabled_cache_toggle_does_not_change_executor_configuration() {
        let before = json!({"start_on_app_launch":false,"startOnAppLaunch":false,
            "system_prompt":"same prompt","model":"same model",
            "effective_env":{"SAFE":"retained"},"runtime_pid":null});
        let mut after = before.clone();
        after["start_on_app_launch"] = json!(true);
        after["startOnAppLaunch"] = json!(true);
        after["runtime_pid"] = json!(42);
        let mut projected_before = before.clone();
        strip_client_bookkeeping(&mut projected_before);
        strip_client_bookkeeping(&mut after);
        assert_eq!(projected_before, after);
        assert_eq!(
            serde_json::to_vec(&projected_before).ok(),
            serde_json::to_vec(&after).ok()
        );
        assert_eq!(before["start_on_app_launch"], false);
        after["system_prompt"] = json!("changed runtime configuration");
        assert_ne!(projected_before, after);
    }
}
