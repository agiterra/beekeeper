//! Desktop custody/bootstrap and signing adapters for sole host management.
//! Legacy data is read only by the bounded importer; there is no desktop executor
//! or restore-legacy-owner operation.
use super::*;
use beekeeper_agent_management::{Context, PrivateConfiguration, RelayTransport};
use beekeeper_host_core::{layout, ownership};
use nostr::Event;
use serde_json::{json, Value};
use tauri::Manager;
#[path = "host_configuration.rs"]
mod configuration;
#[path = "host_import.rs"]
mod importer;
#[path = "host_lifecycle.rs"]
mod lifecycle;
#[path = "host_stop_intent.rs"]
mod stop_intent;
use super::headless_transport::Transport;
pub(crate) use configuration::{reconcile_host_agent, update_if_adopted};
pub(crate) use importer::{deletion_source, enroll_new_host_agent, import_legacy_agents_on_setup};
pub(crate) use lifecycle::{
    active_host_agent_pubkeys, delete_host_agent_snapshot, start_host_pair,
    start_host_pair_with_floor, stop_host_pair,
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
    let personas = load_personas(app)?;
    let global = crate::managed_agents::load_global_agent_config(app)?;
    let effective =
        crate::managed_agents::resolve_effective_harness_descriptor(record, &personas, &global)?;
    let mut private =
        serde_json::to_value(record).map_err(|_| "cannot encode private configuration")?;
    for key in [
        "private_key_nsec",
        "privateKeyNsec",
        "auth_tag",
        "authTag",
        "updated_at",
        "updatedAt",
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

/// Observe shared management and migration facts without claiming runtime readiness.
#[tauri::command]
pub async fn get_managed_agent_host_transfer(
    app: AppHandle,
    pubkey: String,
    local_only: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let relay = crate::relay::relay_ws_url_with_override(&state);
    let migration_eligible = record_for_pair(&app, &pubkey, &relay).is_ok_and(|record| {
        record.backend == BackendKind::Local
            && Keys::parse(&record.private_key_nsec).is_ok_and(|actor| {
                actor.public_key().to_hex() == pubkey
                    && record.auth_tag.as_deref().is_some_and(|auth| {
                        beekeeper_sdk_pkg::nip_oa::verify_auth_tag(auth, &actor.public_key())
                            .is_ok_and(|owner| {
                                state
                                    .signing_keys()
                                    .is_ok_and(|keys| owner == keys.public_key())
                            })
                    })
            })
    });
    let home = layout::home_dir()?;
    let config = beekeeper_host_core::config::HostConfig::load(&layout::host_config_path(
        &home,
        crate::agent_host::instance(),
    ))?;
    let transfer = ownership::for_pair(&home, crate::agent_host::instance(), &pubkey, &relay)?;
    let revision = if local_only.unwrap_or(false) {
        String::new()
    } else {
        state.signing_keys()?;
        head(&app, &relay, &pubkey)
            .await
            .map(|event| event.id.to_hex())
            .unwrap_or_default()
    };
    Ok(
        json!({"relayUrl":relay,"revision":revision,"commissioned":config.is_some_and(|c|
        beekeeper_host_core::record::canonical_relay_key(&c.relay_url)==beekeeper_host_core::record::canonical_relay_key(&relay)),
        "transfer":transfer,"migrationEligible":migration_eligible,"revisionAvailable":!revision.is_empty(),"legacyExecutionRetired":true,"channel":Value::Null,"cwd":Value::Null}),
    )
}
