//! Desktop custody adapter for the shared, reversible ownership journal.
use super::*;
use beekeeper_host_core::{
    atomic_write, layout,
    ownership::{self, Phase, Transfer},
};
use serde::{Deserialize, Serialize};
#[path = "host_transfer_evidence.rs"]
mod evidence;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StagedInput {
    channel: String,
    cwd: PathBuf,
    name: String,
    prompt: String,
    model: Option<String>,
    attempted: bool,
}
fn stage_dir(home: &Path, operation: &str) -> PathBuf {
    layout::host_dir(home, crate::agent_host::instance())
        .join("management/transfer-stage")
        .join(operation)
}

fn existing_preparation(
    old: Option<Transfer>,
    operation: &str,
    revision: &str,
) -> Result<Option<Transfer>, String> {
    let Some(old) = old else { return Ok(None) };
    if old.operation_id != operation && old.phase == Phase::LegacyRestored {
        // A fresh move must revalidate custody, current configuration and the
        // tracked idle generation. The shared journal retains restored history.
        return Ok(None);
    }
    if old.operation_id != operation || old.definition_event_id != revision {
        return Err("ownership transfer operation conflicts".into());
    }
    Ok(Some(old))
}
fn save_input(dir: &Path, input: &StagedInput) -> Result<(), String> {
    atomic_write::atomic_write_json_restricted(
        &dir.join("input.json"),
        &serde_json::to_vec(input).map_err(|e| e.to_string())?,
    )
}
fn read_input(dir: &Path) -> Result<StagedInput, String> {
    serde_json::from_slice(&std::fs::read(dir.join("input.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn transfer(home: &Path, actor: &str, relay: &str, operation: &str) -> Result<Transfer, String> {
    let t = ownership::for_pair(home, crate::agent_host::instance(), actor, relay)?
        .ok_or("ownership transfer is not staged")?;
    if t.operation_id != operation {
        return Err("ownership transfer operation conflicts".into());
    }
    Ok(t)
}
async fn cli(state: &AppState, relay: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let owner = state.keys.lock().map_err(|e| e.to_string())?.clone();
    let binary =
        crate::managed_agents::resolve_command("bee").ok_or("headless bee CLI is not installed")?;
    let mut command = tokio::process::Command::new(binary);
    command
        .env_clear()
        .env(
            "PATH",
            crate::managed_agents::login_shell_path().unwrap_or_default(),
        )
        .env("BEEKEEPER_PRIVATE_KEY", owner.secret_key().to_secret_hex())
        .env(
            "BEEKEEPER_HOST_INSTANCE",
            if crate::agent_host::instance() == layout::Instance::Dev {
                "dev"
            } else {
                "production"
            },
        );
    if let Ok(home) = std::env::var("HOME") {
        command.env("HOME", home);
    }
    let output = command
        .arg("--relay")
        .arg(relay)
        .arg("--format")
        .arg("compact")
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "headless management refused: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("invalid management receipt: {e}"))
}

/// Stage existing scoped custody, then acknowledge shutdown of its tracked idle generation.
// Preserve the existing named Tauri IPC arguments; AppHandle/State are injected.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn prepare_managed_agent_host_transfer(
    pubkey: String,
    relay_url: String,
    operation_id: String,
    definition_event_id: String,
    channel: String,
    cwd: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Transfer, String> {
    uuid::Uuid::parse_str(&operation_id).map_err(|_| "transfer requires a UUID operation")?;
    uuid::Uuid::parse_str(&channel).map_err(|_| "transfer requires a channel UUID")?;
    let cwd = PathBuf::from(cwd);
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err("transfer requires an existing absolute working directory".into());
    }
    let api = crate::relay::relay_api_base_url_with_override(&state);
    crate::relay::assert_expected_relay_scope(Some(&relay_url), &api)?;
    let owner = state.keys.lock().map_err(|e| e.to_string())?.clone();
    let events=crate::relay::query_relay_at_with_keys(&state,&api,&[serde_json::json!({"kinds":[30177],"authors":[owner.public_key().to_hex()],"#d":[&pubkey]})],&owner,None).await?;
    let latest = events
        .into_iter()
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| b.id.cmp(&a.id))
        })
        .ok_or("current public agent definition is unavailable")?;
    if latest.pubkey != owner.public_key()
        || latest.kind.as_u16() != 30177
        || !latest.tags.iter().any(|tag| {
            tag.as_slice().first().is_some_and(|v| v == "d")
                && tag.as_slice().get(1).is_some_and(|v| v == &pubkey)
        })
    {
        return Err("agent definition has invalid authority or coordinate".into());
    }
    latest
        .verify()
        .map_err(|_| "agent definition signature is invalid")?;
    if latest.id.to_hex() != definition_event_id {
        return Err("agent definition changed; refresh before transfer".into());
    }
    tokio::task::spawn_blocking(move || {
        use tauri::Manager;
        let state = app.state::<AppState>();
        let home = layout::home_dir()?;
        let _management = ownership::management_lock(&home, crate::agent_host::instance())?;
        if let Some(old) = existing_preparation(
            ownership::for_pair(&home, crate::agent_host::instance(), &pubkey, &relay_url)?,
            &operation_id,
            &definition_event_id,
        )? {
            let staged = read_input(&stage_dir(&home, &operation_id))?;
            if staged.channel != channel || staged.cwd != cwd {
                return Err("ownership transfer operation conflicts with staged inputs".into());
            }
            if old.phase != Phase::Prepared {
                return Ok(old);
            }
            // The ACP acknowledgment survives a crash between child exit and
            // the journal transition. A PID alone never supplies this fact.
            let key = crate::managed_agents::ManagedAgentRuntimeKey::new(&pubkey, &relay_url)?;
            let receipt_path = crate::managed_agents::managed_agent_runtime_log_path(&app, &key)?
                .with_file_name(format!("quiesced-{}.json", old.legacy_nonce));
            if receipt_path.exists() {
                let receipt: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(receipt_path).map_err(|e| e.to_string())?,
                )
                .map_err(|_| "invalid persisted quiescence acknowledgment")?;
                let pid = receipt
                    .get("harnessPid")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or("quiescence acknowledgment has no harness identity")?;
                if receipt.get("schema").and_then(|v| v.as_str())
                    != Some("beekeeper-local-quiescence/v1")
                    || receipt.get("startNonce").and_then(|v| v.as_str())
                        != Some(old.legacy_nonce.as_str())
                    || receipt.get("actorPubkey").and_then(|v| v.as_str()) != Some(pubkey.as_str())
                    || receipt.get("acceptedWorkDrained").and_then(|v| v.as_bool()) != Some(true)
                    || receipt
                        .get("relayUrl")
                        .and_then(|v| v.as_str())
                        .map(beekeeper_host_core::record::canonical_relay_key)
                        .as_deref()
                        != Some(
                            beekeeper_host_core::record::canonical_relay_key(&relay_url).as_str(),
                        )
                {
                    return Err(
                        "persisted quiescence acknowledgment does not match staged generation"
                            .into(),
                    );
                }
                if crate::managed_agents::process_is_running(pid) {
                    return Err(
                        "acknowledged legacy harness has not exited yet; retry transfer".into(),
                    );
                }
                let mut quiesced = old;
                quiesced.phase = Phase::Quiesced;
                ownership::advance(&home, crate::agent_host::instance(), quiesced.clone())?;
                return Ok(quiesced);
            }
        }
        let _transition = state
            .managed_agent_runtime_transition
            .lock()
            .map_err(|e| e.to_string())?;
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|e| e.to_string())?;
        let mut records = load_managed_agents(&app)?;
        let record = find_managed_agent_mut(&mut records, &pubkey)?;
        let published: beekeeper_core_pkg::managed_agent::ManagedAgentEventContent =
            serde_json::from_str(&latest.content)
                .map_err(|_| "public agent definition is malformed")?;
        let local = crate::managed_agents::agent_events::agent_event_content(record);
        if serde_json::to_value(published.public_projection()).map_err(|e| e.to_string())?
            != serde_json::to_value(local).map_err(|e| e.to_string())?
        {
            return Err(
                "local settings differ from the accepted definition; reconcile before transfer"
                    .into(),
            );
        }
        let config = beekeeper_host_core::config::HostConfig::load(&layout::host_config_path(
            &home,
            crate::agent_host::instance(),
        ))?
        .ok_or("host must be commissioned before transfer")?;
        if beekeeper_host_core::record::canonical_relay_key(&config.relay_url)
            != beekeeper_host_core::record::canonical_relay_key(&relay_url)
        {
            return Err("commissioned host belongs to another relay".into());
        }
        let mut runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|e| e.to_string())?;
        let key = crate::managed_agents::ManagedAgentRuntimeKey::new(&pubkey, &relay_url)?;
        let runtime = runtimes
            .get(&key)
            .ok_or("transfer requires the tracked legacy runtime; PID receipts are insufficient")?;
        if !matches!(
            runtime.lifecycle,
            crate::managed_agents::ManagedAgentRuntimeLifecycle::Listening
                | crate::managed_agents::ManagedAgentRuntimeLifecycle::Ready
        ) {
            return Err("legacy runtime is busy or unavailable; wait for an idle boundary".into());
        }
        let actor = nostr::Keys::parse(&record.private_key_nsec)
            .map_err(|_| "preserved scoped key is unavailable")?;
        if actor.public_key().to_hex() != pubkey {
            return Err("preserved scoped key does not match actor".into());
        }
        let auth = record
            .auth_tag
            .as_deref()
            .ok_or("preserved owner attestation is unavailable")?;
        if beekeeper_sdk_pkg::nip_oa::verify_auth_tag(auth, &actor.public_key())
            .map_err(|_| "invalid owner attestation")?
            != owner.public_key()
        {
            return Err("preserved identity belongs to another owner".into());
        }
        let dir = stage_dir(&home, &operation_id);
        atomic_write::create_dir_all_restricted(&dir)?;
        let staged = StagedInput {
            channel,
            cwd,
            name: record.name.clone(),
            prompt: record.system_prompt.clone().unwrap_or_default(),
            model: record.model.clone(),
            attempted: false,
        };
        if dir.join("input.json").exists() && read_input(&dir)? != staged {
            return Err("ownership transfer operation conflicts with staged inputs".into());
        }
        atomic_write::atomic_write_json_restricted(
            &dir.join("key"),
            record.private_key_nsec.as_bytes(),
        )?;
        atomic_write::atomic_write_json_restricted(&dir.join("auth"), auth.as_bytes())?;
        save_input(&dir, &staged)?;
        let mut t = Transfer {
            operation_id,
            actor_pubkey: pubkey,
            relay_url,
            legacy_nonce: runtime.start_nonce.clone(),
            definition_event_id,
            target: None,
            phase: Phase::Prepared,
        };
        ownership::advance(&home, crate::agent_host::instance(), t.clone())?;
        crate::managed_agents::quiesce_managed_agent_pair(&app, record, &mut runtimes, &key)?;
        save_managed_agents(&app, &records)?;
        t.phase = Phase::Quiesced;
        ownership::advance(&home, crate::agent_host::instance(), t.clone())?;
        Ok(t)
    })
    .await
    .map_err(|e| format!("legacy quiescence task failed: {e}"))?
}

/// Ask the existing management CLI to adopt the exact staged identity and verify its receipt.
#[tauri::command]
pub async fn adopt_managed_agent_host_transfer(
    pubkey: String,
    relay_url: String,
    operation_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let home = layout::home_dir()?;
    let dir = stage_dir(&home, &operation_id);
    let mut input = {
        let _lock = ownership::management_lock(&home, crate::agent_host::instance())?;
        let t = transfer(&home, &pubkey, &relay_url, &operation_id)?;
        if !matches!(t.phase, Phase::Quiesced | Phase::Adopted) {
            return Err("legacy generation has not acknowledged shutdown".into());
        }
        let mut input = read_input(&dir)?;
        input.attempted = true;
        save_input(&dir, &input)?;
        input
    };
    let mut args = vec![
        "agents".into(),
        "create".into(),
        "--operation-id".into(),
        operation_id.clone(),
        "--channel".into(),
        input.channel,
        "--display-name".into(),
        input.name,
        "--system-prompt".into(),
        input.prompt,
        "--home".into(),
        home.to_string_lossy().into_owned(),
        "--cwd".into(),
        input.cwd.to_string_lossy().into_owned(),
        "--adopt-agent-pubkey".into(),
        pubkey.clone(),
        "--key-file".into(),
        dir.join("key").to_string_lossy().into_owned(),
        "--auth-tag-file".into(),
        dir.join("auth").to_string_lossy().into_owned(),
    ];
    if let Some(model) = input.model.take() {
        args.extend(["--model".into(), model]);
    }
    let outcome = cli(&state, &relay_url, &args).await?;
    let t = transfer(&home, &pubkey, &relay_url, &operation_id)?;
    if t.phase != Phase::Adopted
        || t.target.is_none()
        || outcome.get("host_admitted").and_then(|v| v.as_bool()) != Some(true)
    {
        return Err("adoption lacks a verified accepted execution".into());
    }
    Ok(outcome)
}

/// Restore preserved legacy custody only after the adopted generation is proved stopped.
#[tauri::command]
pub async fn rollback_managed_agent_host_transfer(
    pubkey: String,
    relay_url: String,
    operation_id: String,
    state: State<'_, AppState>,
) -> Result<Transfer, String> {
    let home = layout::home_dir()?;
    let mut t = transfer(&home, &pubkey, &relay_url, &operation_id)?;
    let input = read_input(&stage_dir(&home, &operation_id))?;
    if t.phase == Phase::Prepared {
        let _lock = ownership::management_lock(&home, crate::agent_host::instance())?;
        t.phase = Phase::LegacyRestored;
        ownership::advance(&home, crate::agent_host::instance(), t.clone())?;
        return Ok(t);
    }
    if t.phase == Phase::Quiesced && input.attempted {
        return Err(
            "adoption outcome is uncertain; reconcile the same operation before rollback".into(),
        );
    }
    if matches!(
        t.phase,
        Phase::Adopted | Phase::HostStopped | Phase::Restoring
    ) {
        {
            let _management = ownership::management_lock(&home, crate::agent_host::instance())?;
            let _admission = ownership::admission_lock(
                &home,
                crate::agent_host::instance(),
                &pubkey,
                &relay_url,
            )?;
            t = ownership::begin_restoration(
                &home,
                crate::agent_host::instance(),
                &pubkey,
                &relay_url,
            )?;
        }
        stop_actor_contexts(&state, &home, &pubkey, &relay_url).await?;
    }
    require_actor_stopped(&state, &home, &pubkey, &relay_url, None).await?;
    let _lock = ownership::management_lock(&home, crate::agent_host::instance())?;
    if !matches!(
        t.phase,
        Phase::Quiesced | Phase::Restoring | Phase::LegacyRestored
    ) {
        return Err("rollback requires a verified stopped owner".into());
    }
    if t.phase != Phase::LegacyRestored {
        t.phase = Phase::LegacyRestored;
        ownership::advance(&home, crate::agent_host::instance(), t.clone())?;
    }
    Ok(t)
}

/// Stop an adopted target using the existing relay lifecycle; never signal its PID.
pub(super) async fn stop_if_adopted(app: &AppHandle, pubkey: &str) -> Result<bool, String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let relay = crate::relay::relay_ws_url_with_override(&state);
    let home = layout::home_dir()?;
    // The journal contains every actor: serialize its read-modify-write first,
    // then take the participant admission barrier in the common lock order.
    // sessions.stop does not acquire either lock and can publish its receipts.
    let _management = ownership::management_lock(&home, crate::agent_host::instance())?;
    let Some(t) = ownership::for_pair(&home, crate::agent_host::instance(), pubkey, &relay)? else {
        return Ok(false);
    };
    if !matches!(t.phase, Phase::Adopted | Phase::HostStopped) {
        ownership::require_legacy_owner(&home, crate::agent_host::instance(), pubkey, &relay)?;
        return Ok(false);
    }
    let target = t.target.ok_or("host ownership has no accepted target")?;
    let _admission =
        ownership::admission_lock(&home, crate::agent_host::instance(), pubkey, &relay)?;
    let config = beekeeper_host_core::config::HostConfig::load(&layout::host_config_path(
        &home,
        crate::agent_host::instance(),
    ))?
    .ok_or("commissioned host missing")?;
    let snapshot: ProviderSnapshot = serde_json::from_slice(
        &std::fs::read(config.provider_state_dir.join("state.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if !snapshot
        .sessions
        .values()
        .any(|r| r.actor.as_deref() == Some(pubkey) && r.target(&target.instance_id) == target)
    {
        return Err("accepted primary responsibility is missing from provider inventory; stop remains unconfirmed".into());
    }
    stop_actor_contexts(&state, &home, pubkey, &relay).await?;
    require_actor_stopped(&state, &home, pubkey, &relay, None).await?;
    ownership::host_stopped(
        &home,
        crate::agent_host::instance(),
        pubkey,
        &relay,
        &target,
    )?;
    Ok(true)
}

#[derive(Deserialize)]
struct ProviderSnapshot {
    sessions:
        std::collections::BTreeMap<String, beekeeper_session_provider_pkg::state::SessionRecord>,
}

async fn require_actor_stopped(
    state: &AppState,
    home: &Path,
    actor: &str,
    relay: &str,
    pending_operation: Option<&str>,
) -> Result<(), String> {
    let config = beekeeper_host_core::config::HostConfig::load(&layout::host_config_path(
        home,
        crate::agent_host::instance(),
    ))?
    .ok_or("commissioned host configuration missing")?;
    if beekeeper_host_core::record::canonical_relay_key(&config.relay_url)
        != beekeeper_host_core::record::canonical_relay_key(relay)
    {
        return Err("rollback relay differs from host custody".into());
    }
    let provider_store =
        beekeeper_host_core::record::load_provider_store_from(&config.record_store_path())?;
    let provider = provider_store
        .get(relay)
        .or_else(|| provider_store.get(&config.relay_url))
        .ok_or("provider identity missing")?;
    let snapshot: ProviderSnapshot = serde_json::from_slice(
        &std::fs::read(config.provider_state_dir.join("state.json"))
            .map_err(|e| format!("cannot inventory actor responsibilities: {e}"))?,
    )
    .map_err(|e| format!("invalid responsibility inventory: {e}"))?;
    let api = crate::relay::relay_api_base_url_with_override(state);
    crate::relay::assert_expected_relay_scope(Some(relay), &api)?;
    for record in snapshot.sessions.values().filter(|r| {
        r.actor.as_deref() == Some(actor) && pending_operation != Some(r.command_id.as_str())
    }) {
        let target = record.target(&provider.instance_id);
        let events=crate::relay::query_relay_at(state,&api,&[serde_json::json!({"kinds":[44223,44224],"authors":[&config.provider_pubkey],"#h":[record.channel_id.to_string()]})]).await?;
        evidence::require_target_stopped(actor, &target, &config.provider_pubkey, events)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SuccessorInput {
    operation_id: String,
    input: StagedInput,
}

/// Start a new responsibility after terminal stop, preserving the same actor.
/// This is a new context; a terminal execution is never presented as resumed.
pub(super) async fn start_if_adopted(
    app: &AppHandle,
    pubkey: &str,
    relay: &str,
) -> Result<bool, String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let home = layout::home_dir()?;
    let Some(t) = ownership::for_pair(&home, crate::agent_host::instance(), pubkey, relay)? else {
        return Ok(false);
    };
    if t.phase == Phase::Adopted {
        return Ok(true); // Existing admitted responsibility; never create a duplicate.
    }
    if t.phase != Phase::HostStopped {
        ownership::require_legacy_owner(&home, crate::agent_host::instance(), pubkey, relay)?;
        return Ok(false);
    }
    let old_dir = stage_dir(&home, &t.operation_id);
    let next_path = old_dir.join("successor.json");
    let pending: Option<SuccessorInput> = if next_path.exists() {
        Some(
            serde_json::from_slice(&std::fs::read(&next_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    require_actor_stopped(
        &state,
        &home,
        pubkey,
        relay,
        pending.as_ref().map(|p| p.operation_id.as_str()),
    )
    .await?;
    let api = crate::relay::relay_api_base_url_with_override(&state);
    let owner = state.keys.lock().map_err(|e| e.to_string())?.clone();
    let heads=crate::relay::query_relay_at_with_keys(&state,&api,&[serde_json::json!({"kinds":[30177],"authors":[owner.public_key().to_hex()],"#d":[pubkey]})],&owner,None).await?;
    let head = heads
        .into_iter()
        .filter(|e| {
            e.pubkey == owner.public_key() && e.kind.as_u16() == 30177 && e.verify().is_ok()
        })
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| b.id.cmp(&a.id))
        })
        .ok_or("current agent definition unavailable")?;
    let definition: beekeeper_core_pkg::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&head.content).map_err(|_| "current agent definition malformed")?;
    let next: SuccessorInput = {
        let _lock = ownership::management_lock(&home, crate::agent_host::instance())?;
        if next_path.exists() {
            serde_json::from_slice(&std::fs::read(&next_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
        } else {
            let mut input = read_input(&old_dir)?;
            input.name = definition.name;
            input.prompt = definition.system_prompt.unwrap_or_default();
            input.model = definition.model;
            let next = SuccessorInput {
                operation_id: uuid::Uuid::new_v4().to_string(),
                input,
            };
            atomic_write::atomic_write_json_restricted(
                &next_path,
                &serde_json::to_vec(&next).map_err(|e| e.to_string())?,
            )?;
            next
        }
    };
    let mut args = vec![
        "agents".into(),
        "create".into(),
        "--operation-id".into(),
        next.operation_id.clone(),
        "--agent-pubkey".into(),
        pubkey.into(),
        "--channel".into(),
        next.input.channel.clone(),
        "--display-name".into(),
        next.input.name.clone(),
        "--system-prompt".into(),
        next.input.prompt.clone(),
        "--home".into(),
        home.to_string_lossy().into_owned(),
        "--cwd".into(),
        next.input.cwd.to_string_lossy().into_owned(),
    ];
    if let Some(model) = &next.input.model {
        args.extend(["--model".into(), model.clone()]);
    }
    // Persist successor inputs before the CLI can atomically advance ownership.
    // A close/crash after its accepted receipt must not leave the new journal
    // operation referring to a stage directory that was never written.
    {
        let _lock = ownership::management_lock(&home, crate::agent_host::instance())?;
        let next_dir = stage_dir(&home, &next.operation_id);
        atomic_write::create_dir_all_restricted(&next_dir)?;
        save_input(&next_dir, &next.input)?;
    }
    let outcome = cli(&state, relay, &args).await?;
    if outcome.get("host_admitted").and_then(|v| v.as_bool()) != Some(true)
        || outcome.get("agent_pubkey").and_then(|v| v.as_str()) != Some(pubkey)
        || outcome.get("operation_id").and_then(|v| v.as_str()) != Some(&next.operation_id)
    {
        return Err("successor has no verified matching management receipt".into());
    }
    let target = serde_json::from_value(
        outcome
            .get("target")
            .cloned()
            .ok_or("successor has no accepted target")?,
    )
    .map_err(|e| e.to_string())?;
    let revision = outcome
        .get("revision")
        .and_then(|v| v.as_str())
        .ok_or("successor has no accepted revision")?;
    let _lock = ownership::management_lock(&home, crate::agent_host::instance())?;
    let current = ownership::for_pair(&home, crate::agent_host::instance(), pubkey, relay)?
        .ok_or("successor ownership receipt is missing")?;
    if current.phase != Phase::Adopted
        || current.operation_id != next.operation_id
        || current.definition_event_id != revision
        || current.target.as_ref() != Some(&target)
    {
        return Err("successor ownership does not match the verified CLI receipt".into());
    }
    Ok(true)
}

async fn stop_actor_contexts(
    state: &AppState,
    home: &Path,
    actor: &str,
    relay: &str,
) -> Result<(), String> {
    let config = beekeeper_host_core::config::HostConfig::load(&layout::host_config_path(
        home,
        crate::agent_host::instance(),
    ))?
    .ok_or("commissioned host missing")?;
    if beekeeper_host_core::record::canonical_relay_key(&config.relay_url)
        != beekeeper_host_core::record::canonical_relay_key(relay)
    {
        return Err("stop relay differs from commissioned host custody".into());
    }
    let store = beekeeper_host_core::record::load_provider_store_from(&config.record_store_path())?;
    let provider = store
        .get(&config.relay_url)
        .ok_or("commissioned provider missing")?;
    let snapshot: ProviderSnapshot = serde_json::from_slice(
        &std::fs::read(config.provider_state_dir.join("state.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    for record in snapshot
        .sessions
        .values()
        .filter(|r| r.actor.as_deref() == Some(actor))
    {
        let target = record.target(&provider.instance_id);
        let outcome = cli(
            state,
            relay,
            &[
                "sessions".into(),
                "stop".into(),
                "--channel".into(),
                record.channel_id.to_string(),
                "--session".into(),
                target.session_id.clone(),
                "--expected-target".into(),
                serde_json::to_string(&target).map_err(|e| e.to_string())?,
                "--provider-authority".into(),
                config.provider_pubkey.clone(),
                "--wait".into(),
                "--timeout-secs".into(),
                "60".into(),
            ],
        )
        .await;
        match outcome {
            Ok(value) if value.get("outcome").and_then(|v| v.as_str()) == Some("stopped") => {}
            // The existing CLI intentionally publishes no second stop when it
            // sees a signed terminal receipt. The all-context verification
            // below still checks the fact rather than trusting this text.
            Err(error) if error.contains("already stopped; nothing was published") => {}
            Err(error) => return Err(error),
            Ok(_) => {
                return Err("responsibility stop remains unconfirmed; restoration is fenced".into())
            }
        }
    }
    Ok(())
}

/// Read the active community's transfer boundary and, unless local-only, its
/// accepted definition. Local-only observations leave the current revision empty
/// and permit lifecycle presentation without a relay or published definition.
#[tauri::command]
pub async fn get_managed_agent_host_transfer(
    pubkey: String,
    local_only: Option<bool>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let relay = crate::relay::relay_ws_url_with_override(&state);
    let home = layout::home_dir()?;
    let config = beekeeper_host_core::config::HostConfig::load(&layout::host_config_path(
        &home,
        crate::agent_host::instance(),
    ))?;
    let t = ownership::for_pair(&home, crate::agent_host::instance(), &pubkey, &relay)?;
    let input = t
        .as_ref()
        .map(|t| read_input(&stage_dir(&home, &t.operation_id)))
        .transpose()?;
    let revision = if local_only.unwrap_or(false) {
        String::new()
    } else {
        let owner = state.keys.lock().map_err(|e| e.to_string())?.clone();
        let api = crate::relay::relay_api_base_url_with_override(&state);
        let events=crate::relay::query_relay_at_with_keys(&state,&api,&[serde_json::json!({"kinds":[30177],"authors":[owner.public_key().to_hex()],"#d":[&pubkey]})],&owner,None).await?;
        events
            .into_iter()
            .filter(|e| e.pubkey == owner.public_key() && e.verify().is_ok())
            .max_by(|a, b| {
                a.created_at
                    .cmp(&b.created_at)
                    .then_with(|| b.id.cmp(&a.id))
            })
            .ok_or("accepted agent definition unavailable for this owner")?
            .id
            .to_hex()
    };
    Ok(
        serde_json::json!({"relayUrl":relay,"revision":revision,"commissioned":config.is_some_and(|c|beekeeper_host_core::record::canonical_relay_key(&c.relay_url)==beekeeper_host_core::record::canonical_relay_key(&relay)),"transfer":t,"channel":input.as_ref().map(|i|&i.channel),"cwd":input.as_ref().map(|i|&i.cwd)}),
    )
}

pub(crate) async fn update_if_adopted(
    input: &crate::managed_agents::UpdateManagedAgentRequest,
    app: &AppHandle,
    state: &AppState,
) -> Result<Option<crate::managed_agents::UpdateManagedAgentResponse>, String> {
    let relay = crate::relay::relay_ws_url_with_override(state);
    let home = layout::home_dir()?;
    let Some(t) = ownership::for_pair(&home, crate::agent_host::instance(), &input.pubkey, &relay)?
    else {
        return Ok(None);
    };
    if matches!(t.phase, Phase::Prepared | Phase::LegacyRestored) {
        return Ok(None);
    }
    if !matches!(t.phase, Phase::Adopted | Phase::HostStopped) {
        return Err(
            "ownership transfer is in progress; reconcile before changing configuration".into(),
        );
    }
    let records = load_managed_agents(app)?;
    let current = records
        .iter()
        .find(|r| r.pubkey == input.pubkey)
        .ok_or("managed identity unavailable")?;
    let unsupported_change = input
        .env_vars
        .as_ref()
        .is_some_and(|v| v != &current.env_vars)
        || input.parallelism.is_some_and(|v| v != current.parallelism)
        || input.relay_url.as_ref().is_some_and(|v| {
            beekeeper_host_core::record::canonical_relay_key(v)
                != beekeeper_host_core::record::canonical_relay_key(&current.relay_url)
        })
        || input
            .acp_command
            .as_ref()
            .is_some_and(|v| v != &current.acp_command)
        || input
            .agent_command
            .as_ref()
            .is_some_and(|v| v != &current.agent_command)
        || input
            .agent_args
            .as_ref()
            .is_some_and(|v| v != &current.agent_args)
        || input
            .provider
            .as_ref()
            .is_some_and(|v| v != &current.provider)
        || input
            .runtime
            .as_ref()
            .is_some_and(|v| v != &current.runtime)
        || input.respond_to.is_some_and(|v| v != current.respond_to)
        || input
            .respond_to_allowlist
            .as_ref()
            .is_some_and(|v| v != &current.respond_to_allowlist);
    if unsupported_change {
        return Err("this host-managed editor supports name, instructions and model; runtime, credentials and access changes require supported host administration".into());
    }
    if matches!(input.model, Some(None)) {
        return Err(
            "clearing a host-managed model is not supported; select an explicit model".into(),
        );
    }
    let owner = state.keys.lock().map_err(|e| e.to_string())?.clone();
    let api = crate::relay::relay_api_base_url_with_override(state);
    let events=crate::relay::query_relay_at_with_keys(state,&api,&[serde_json::json!({"kinds":[30177],"authors":[owner.public_key().to_hex()],"#d":[&input.pubkey]})],&owner,None).await?;
    let head = events
        .into_iter()
        .filter(|e| e.pubkey == owner.public_key() && e.verify().is_ok())
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| b.id.cmp(&a.id))
        })
        .ok_or("current agent revision unavailable")?;
    let mut args = vec![
        "agents".into(),
        "update".into(),
        "--operation-id".into(),
        uuid::Uuid::new_v4().to_string(),
        "--agent-pubkey".into(),
        input.pubkey.clone(),
        "--expected-revision".into(),
        head.id.to_hex(),
        "--home".into(),
        home.to_string_lossy().into_owned(),
    ];
    if let Some(name) = &input.name {
        args.extend(["--display-name".into(), name.clone()]);
    }
    if let Some(prompt) = &input.system_prompt {
        args.extend(["--system-prompt".into(), prompt.clone().unwrap_or_default()]);
    }
    if let Some(Some(model)) = &input.model {
        args.extend(["--model".into(), model.clone()]);
    }
    let result = cli(state, &relay, &args).await?;
    let accepted = result
        .get("effective_revision")
        .or_else(|| result.get("revision"))
        .and_then(|v| v.as_str())
        .ok_or("configuration update has no effective revision")?;
    let events=crate::relay::query_relay_at_with_keys(state,&api,&[serde_json::json!({"kinds":[30177],"authors":[owner.public_key().to_hex()],"ids":[accepted]})],&owner,None).await?;
    let event = events
        .into_iter()
        .find(|e| e.id.to_hex() == accepted && e.pubkey == owner.public_key() && e.verify().is_ok())
        .ok_or("accepted configuration cannot be verified")?;
    let content: beekeeper_core_pkg::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&event.content).map_err(|e| e.to_string())?;
    let _store = state
        .managed_agents_store_lock
        .lock()
        .map_err(|e| e.to_string())?;
    let mut records = load_managed_agents(app)?;
    let record = find_managed_agent_mut(&mut records, &input.pubkey)?;
    record.name = content.name;
    record.system_prompt = content.system_prompt;
    record.model = content.model;
    save_managed_agents(app, &records)?;
    let record = records
        .iter()
        .find(|r| r.pubkey == input.pubkey)
        .ok_or("materialized agent disappeared")?;
    let runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|e| e.to_string())?;
    Ok(Some(crate::managed_agents::UpdateManagedAgentResponse{agent:summarize_from_disk(app,record,&runtimes)?,profile_sync_error:Some("Configuration accepted; runtime application awaits the next supported execution boundary.".into())}))
}

#[cfg(test)]
mod preparation_tests {
    use super::*;

    #[test]
    fn restored_transfer_allows_fresh_validation_but_live_boundaries_refuse_replacement() {
        let original = Transfer {
            operation_id: "old-operation".into(),
            actor_pubkey: "ab".repeat(32),
            relay_url: "ws://127.0.0.1:3037".into(),
            legacy_nonce: "original-generation".into(),
            definition_event_id: "cd".repeat(32),
            target: None,
            phase: Phase::LegacyRestored,
        };
        assert_eq!(
            existing_preparation(
                Some(original.clone()),
                "old-operation",
                &original.definition_event_id
            )
            .unwrap(),
            Some(original.clone()),
            "same-operation retry preserves the restored journal rather than staging new execution",
        );
        assert!(
            existing_preparation(Some(original.clone()), "old-operation", "changed-revision")
                .is_err()
        );
        assert_eq!(
            existing_preparation(
                Some(original.clone()),
                "fresh-operation",
                "new-current-revision"
            )
            .unwrap(),
            None,
            "a new operation must continue full preparation instead of replaying the old custody",
        );
        for phase in [
            Phase::Prepared,
            Phase::Quiesced,
            Phase::Adopted,
            Phase::HostStopped,
            Phase::Restoring,
        ] {
            let mut active = original.clone();
            active.phase = phase;
            assert!(existing_preparation(
                Some(active),
                "fresh-operation",
                &original.definition_event_id
            )
            .is_err());
        }
    }
}
