//! Bounded, one-way custody importer at setup/create boundaries.
use super::*;
use beekeeper_agent_management::{CreateRequest, IdentityInput, NativeContext};
use beekeeper_host_core::{
    atomic_write,
    migration_backup::{self, BackupFile, BackupScope},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportStage {
    operation_id: Uuid,
    record: ManagedAgentRecord,
    relay_url: String,
    channel: Uuid,
    cwd: PathBuf,
    activate: bool,
    generation: Option<Generation>,
    quiesced: bool,
    native_context: Option<NativeContext>,
    fresh: bool,
    definition_revision: Option<String>,
    generation_inspected: bool,
    replay_floor_unix: Option<u64>,
    #[serde(default)]
    source_configuration: Option<PrivateConfiguration>,
    #[serde(default)]
    backfill_since_unix: Option<u64>,
}
#[path = "host_import_replay.rs"]
mod replay;
#[path = "host_import_revision.rs"]
mod revision;
#[path = "host_import_source.rs"]
mod source_compare;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Generation {
    pid: u32,
    nonce: String,
    instance: String,
    log_path: PathBuf,
    launch: crate::managed_agents::LegacyNativeLaunch,
}

pub(super) async fn import_guard(
    app: &AppHandle,
    actor: &str,
    relay: &str,
) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    let key = crate::managed_agents::ManagedAgentRuntimeKey::new(actor, relay)?;
    let state = app.state::<AppState>();
    let lock = {
        let mut locks = state
            .provider_deploy_locks
            .lock()
            .map_err(|_| "client custody lock unavailable")?;
        locks
            .entry(format!("import:{}", key.runtime_id()))
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    Ok(lock.lock_owned().await)
}

fn pair_directory(actor: &str, relay: &str) -> Result<PathBuf, String> {
    let key = crate::managed_agents::ManagedAgentRuntimeKey::new(actor, relay)?;
    Ok(
        layout::host_dir(&context()?.home, crate::agent_host::instance())
            .join("management/desktop-import")
            .join(key.runtime_id()),
    )
}
fn private_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "protected import source unavailable")?;
    if !metadata.file_type().is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err("protected import source must be a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("protected import source permissions are too broad".into());
        }
    }
    std::fs::read(path).map_err(|_| "protected import source unreadable".into())
}

fn save(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        atomic_write::create_dir_all_restricted(parent)?;
    }
    atomic_write::atomic_write_json_restricted(
        path,
        &serde_json::to_vec(value).map_err(|_| "cannot encode import journal")?,
    )
}

#[path = "host_import_journal.rs"]
mod journal;
use journal::{enrollment_stage, save_enrollment_stage};

fn verify_source_unchanged(app: &AppHandle, staged: &ImportStage) -> Result<(), String> {
    let current = record_for_pair(app, &staged.record.pubkey, &staged.relay_url)?;
    let (_, private) = configuration_view(app, &current)?;
    let (_, old_private) = configuration_view(app, &staged.record)?;
    let personas = load_personas(app)?;
    if !source_compare::unchanged(
        &staged.record,
        &current,
        staged.source_configuration.as_ref().unwrap_or(&old_private),
        &private,
        &personas,
    ) {
        return Err("source configuration/custody changed during host handoff; execution is not admitted, protected backup retained for repair".into());
    }
    Ok(())
}
#[path = "host_delete_intent.rs"]
mod deletion;
pub(super) use deletion::deletion_intent;
pub(crate) use deletion::deletion_source;

async fn bootstrap(
    app: &AppHandle,
    relay: &str,
) -> Result<beekeeper_host_core::config::HostConfig, String> {
    let ctx = context()?;
    let path = layout::host_config_path(&ctx.home, ctx.instance);
    let config = beekeeper_host_core::config::HostConfig::load(&path)?;
    if config.as_ref().is_some_and(|c| {
        beekeeper_host_core::record::canonical_relay_key(&c.relay_url)
            != beekeeper_host_core::record::canonical_relay_key(relay)
    }) {
        return Err("this host is commissioned for another community; commission an independently scoped host instead of replacing its running ownership".into());
    }
    if config.is_none() {
        crate::session_provider::commands::provision_coding_session_provider(
            app.clone(),
            app.state::<AppState>(),
            app.state::<crate::agent_host::AgentHost>(),
            Some(relay.into()),
        )
        .await?;
    }
    crate::agent_host::app_scoped::ensure_running()?;
    app.state::<crate::agent_host::AgentHost>().start().await?;
    beekeeper_host_core::config::HostConfig::load(&path)?
        .ok_or_else(|| "host commissioning remains unconfirmed".into())
}

#[path = "host_import_channel.rs"]
mod discovery;
use discovery::channel;

fn legacy_generation(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
) -> Result<Option<Generation>, String> {
    let key = crate::managed_agents::ManagedAgentRuntimeKey::new(&record.pubkey, relay)?;
    let pair_log = crate::managed_agents::managed_agent_runtime_log_path(app, &key)?;
    let older_log = crate::managed_agents::managed_agent_log_path(app, &record.pubkey)?;
    let log = if pair_log.exists() || !older_log.exists() {
        pair_log
    } else {
        older_log
    };
    let native_artifact_location = record
        .env_vars
        .get("CODEX_HOME")
        .map(|home| PathBuf::from(home).join("sessions"))
        .or_else(|| {
            record
                .env_vars
                .get("CLAUDE_CONFIG_DIR")
                .map(|home| PathBuf::from(home).join("projects"))
        });
    let known_native_state = native_artifact_location
        .as_ref()
        .is_some_and(|path| path.exists());
    let mut receipts: Vec<_> = crate::managed_agents::read_all_agent_runtime_receipts(app)
        .into_iter()
        .filter(|(_, r)| r.key == key)
        .collect();
    if receipts.len() > 1 {
        return Err(
            "multiple legacy generation receipts require source repair; no PID was signalled"
                .into(),
        );
    }
    let inspected = if let Some((_, receipt)) = receipts.pop() {
        Some(crate::managed_agents::inspect_legacy_generation(
            &key,
            receipt.pid,
            &receipt.desktop_instance_id,
            log,
        )?)
    } else if record.runtime_pid.is_some()
        || record.last_started_at.is_some()
        || record.last_stopped_at.is_some()
        || log.exists()
        || known_native_state
    {
        Some(crate::managed_agents::inspect_stopped_legacy_generation(
            &key,
            &app.config().identifier,
            log,
        )?)
    } else {
        return Err("legacy execution history has no proven generation/context boundary; pre-migration backup retained. Select the original launch log, clean-drain ACK and native transcript for offline repair, or explicitly authorize a fresh context after inspecting the retained source. Missing PID/history fields do not certify never-started state".into());
    };
    let Some(inspected) = inspected else {
        return Ok(None);
    };
    let launch = if crate::managed_agents::process_is_running(inspected.pid) {
        crate::managed_agents::inspect_legacy_launch(&inspected)?
    } else {
        let environment: std::collections::BTreeMap<_, _> = record
            .env_vars
            .iter()
            .filter(|(key, _)| matches!(key.as_str(), "HOME" | "CODEX_HOME" | "CLAUDE_CONFIG_DIR"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !environment.contains_key("HOME")
            || !(environment.contains_key("CODEX_HOME")
                || environment.contains_key("CLAUDE_CONFIG_DIR"))
        {
            return Err("stopped native context needs preserved explicit HOME and runtime configuration-home provenance; backup retained, select original installation artifacts for repair".into());
        }
        let (public, private) = configuration_view(app, record)?;
        drop(public);
        let command = private
            .record
            .get("effective_command")
            .and_then(Value::as_str)
            .ok_or("stopped source runtime unavailable")?
            .to_owned();
        crate::managed_agents::resolve_stopped_legacy_workspace(&inspected, &command, &environment)?
    };
    Ok(Some(Generation {
        pid: inspected.pid,
        nonce: inspected.start_nonce,
        instance: inspected.desktop_instance_id,
        log_path: inspected.log_path,
        launch,
    }))
}

fn native_path(generation: &Generation) -> PathBuf {
    generation
        .log_path
        .with_file_name(format!("native-context-{}.json", generation.nonce))
}

async fn stage(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
    activate: bool,
    fresh: bool,
) -> Result<(PathBuf, ImportStage), String> {
    let dir = pair_directory(&record.pubkey, relay)?;
    let path = dir.join("stage.json");
    // Reserve exact custody and operation before any backup write or network
    // await. An interrupted capture resumes these protected source bytes.
    let mut held = {
        let ctx = context()?;
        let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
        if path.exists() {
            let held: ImportStage = serde_json::from_slice(&private_bytes(&path)?)
                .map_err(|_| "import stage invalid")?;
            if held.record.pubkey != record.pubkey
                || beekeeper_host_core::record::canonical_relay_key(&held.relay_url)
                    != beekeeper_host_core::record::canonical_relay_key(relay)
            {
                return Err("import stage scope conflicts".into());
            }
            let (_, old_private) = configuration_view(app, &held.record)?;
            let (_, new_private) = configuration_view(app, record)?;
            let personas = load_personas(app)?;
            if !source_compare::unchanged(
                &held.record,
                record,
                held.source_configuration.as_ref().unwrap_or(&old_private),
                &new_private,
                &personas,
            ) {
                return Err("source configuration/custody changed during migration; protected stage retained for explicit repair".into());
            }
            held
        } else {
            let actor = Keys::parse(&record.private_key_nsec)
                .map_err(|_| "source scoped custody unavailable; unlock credentials and retry")?;
            if actor.public_key().to_hex() != record.pubkey {
                return Err("source custody belongs to another identity".into());
            }
            let auth = record
                .auth_tag
                .as_deref()
                .ok_or("source owner attestation unavailable")?;
            if beekeeper_sdk_pkg::nip_oa::verify_auth_tag(auth, &actor.public_key())
                .map_err(|_| "source attestation invalid")?
                != app.state::<AppState>().signing_keys()?.public_key()
            {
                return Err("source belongs to another owner".into());
            }
            let held = ImportStage {
                operation_id: Uuid::new_v4(),
                record: record.clone(),
                relay_url: relay.into(),
                channel: Uuid::nil(),
                cwd: PathBuf::new(),
                activate,
                quiesced: fresh,
                generation: None,
                native_context: None,
                fresh,
                definition_revision: None,
                generation_inspected: fresh,
                // Inclusive capture boundary for bounded migration replay, not
                // a consumed-input watermark. Prior uncertain inputs receive
                // explicit dispositions; input during drain remains eligible.
                replay_floor_unix: (!fresh)
                    .then(|| {
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                    })
                    .transpose()
                    .map_err(|_| "migration capture time unavailable")?,
                source_configuration: Some(configuration_view(app, record)?.1),
                backfill_since_unix: None,
            };
            save(&path, &held)?;
            held
        }
    };
    let original =
        serde_json::to_vec(&held.record).map_err(|_| "source configuration cannot be captured")?;
    let effective = serde_json::to_vec(&held.source_configuration)
        .map_err(|_| "effective source configuration cannot be captured")?;
    let auth = held
        .record
        .auth_tag
        .as_deref()
        .ok_or("source attestation unavailable")?;
    migration_backup::capture(
        &dir.join("backup-before"),
        BackupScope {
            actor_pubkey: held.record.pubkey.clone(),
            relay_url: held.relay_url.clone(),
            operation_id: held.operation_id.to_string(),
        },
        &[
            BackupFile {
                name: "record.json",
                bytes: &original,
            },
            BackupFile {
                name: "effective-configuration.json",
                bytes: &effective,
            },
            BackupFile {
                name: "identity-key",
                bytes: held.record.private_key_nsec.as_bytes(),
            },
            BackupFile {
                name: "auth.json",
                bytes: auth.as_bytes(),
            },
        ],
    )?;
    migration_backup::verify(&dir.join("backup-before"))?;
    if !held.generation_inspected {
        held.generation = legacy_generation(app, &held.record, relay)?;
        held.quiesced = held.generation.is_none();
        held.generation_inspected = true;
        save(&path, &held)?;
    }
    let launch=serde_json::to_vec(&json!({"operationId":held.operation_id,"actorPubkey":held.record.pubkey,"relayUrl":held.relay_url,"generation":held.generation,"fresh":held.fresh})).map_err(|_|"source launch cannot be captured")?;
    migration_backup::capture(
        &dir.join("backup-launch"),
        BackupScope {
            actor_pubkey: held.record.pubkey.clone(),
            relay_url: held.relay_url.clone(),
            operation_id: Uuid::new_v5(&held.operation_id, b"source-launch").to_string(),
        },
        &[BackupFile {
            name: "source-stage.json",
            bytes: &launch,
        }],
    )?;
    if held.channel.is_nil() {
        held.channel = channel(app, relay, &held.record).await?;
        held.cwd = context()?
            .native_home
            .ok_or("native work root unavailable")?
            .join(".beekeeper/workspaces")
            .join(&held.record.pubkey);
        atomic_write::create_dir_all_restricted(&held.cwd)?;
        if !held.fresh {
            let source_definition = head(app, relay, &held.record.pubkey).await?;
            held.definition_revision = Some(source_definition.id.to_hex());
            let ctx = context()?;
            let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
            ownership::advance(
                &ctx.home,
                ctx.instance,
                ownership::Transfer {
                    operation_id: held.operation_id.to_string(),
                    actor_pubkey: held.record.pubkey.clone(),
                    relay_url: relay.into(),
                    legacy_nonce: held
                        .generation
                        .as_ref()
                        .map(|g| g.nonce.clone())
                        .ok_or("legacy generation provenance is missing; no fresh-context inference is permitted")?,
                    definition_event_id: held
                        .definition_revision
                        .clone()
                        .ok_or("source revision missing")?,
                    target: None,
                    phase: ownership::Phase::Prepared,
                },
            )?;
            drop(_lock);
            revision::capture(app, &held, &source_definition)?;
        }
        save(&path, &held)?;
    }
    Ok((dir, held))
}

async fn admit(app: &AppHandle, dir: &Path, mut staged: ImportStage) -> Result<(), String> {
    verify_source_unchanged(app, &staged)?;
    if !staged.fresh && staged.generation.is_none() {
        return Err("legacy migration lacks exact generation/context provenance; source backup retained, explicit repair required".into());
    }
    let host_owned = ownership::for_pair(
        &context()?.home,
        crate::agent_host::instance(),
        &staged.record.pubkey,
        &staged.relay_url,
    )?
    .is_some_and(|t| {
        matches!(
            t.phase,
            ownership::Phase::Adopted | ownership::Phase::HostStopped
        )
    });
    let config = bootstrap(app, &staged.relay_url).await?;
    if !staged.fresh && !host_owned && !staged.quiesced {
        revision::ensure_source_evidence(app, &staged).await?;
    }
    if !staged.quiesced {
        let g = staged
            .generation
            .as_ref()
            .ok_or("legacy generation has no verified import boundary")?;
        let generation = crate::managed_agents::LegacyAgentGeneration {
            key: crate::managed_agents::ManagedAgentRuntimeKey::new(
                &staged.record.pubkey,
                &staged.relay_url,
            )?,
            pid: g.pid,
            start_nonce: g.nonce.clone(),
            desktop_instance_id: g.instance.clone(),
            log_path: g.log_path.clone(),
        };
        // Validate the supported source before stopping a working old owner.
        // A stopped source uses its exact clean ACK; PID absence alone never
        // establishes that it lacked native context.
        if crate::managed_agents::process_is_running(g.pid) {
            crate::managed_agents::preflight_legacy_native_context(&generation, &g.launch)?;
        } else {
            crate::managed_agents::read_legacy_native_context(&generation, &g.launch)?;
        }
        let ack = tokio::task::spawn_blocking(move || {
            crate::managed_agents::quiesce_legacy_generation(&generation)
        })
        .await
        .map_err(|_| "legacy drain task interrupted")??;
        let offline = if ack
            .get("nativeContextExportVersion")
            .and_then(Value::as_u64)
            != Some(1)
        {
            Some(crate::managed_agents::read_legacy_native_context(
                &crate::managed_agents::LegacyAgentGeneration {
                    key: crate::managed_agents::ManagedAgentRuntimeKey::new(
                        &staged.record.pubkey,
                        &staged.relay_url,
                    )?,
                    pid: g.pid,
                    start_nonce: g.nonce.clone(),
                    desktop_instance_id: g.instance.clone(),
                    log_path: g.log_path.clone(),
                },
                &g.launch,
            )?)
        } else {
            None
        };
        if let Some(expected) = ack
            .get("nativeContextSha256")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                offline
                    .as_ref()
                    .and_then(|value| serde_json::to_vec(value).ok())
                    .map(|bytes| {
                        use sha2::{Digest, Sha256};
                        hex::encode(Sha256::digest(bytes))
                    })
            })
        {
            use sha2::{Digest, Sha256};
            let bytes = if let Some(value) = offline {
                serde_json::to_vec(&value).map_err(|_| "offline native context invalid")?
            } else {
                private_bytes(&native_path(g))?
            };
            if hex::encode(Sha256::digest(&bytes)) != expected {
                return Err(
                    "native export differs from drained acknowledgment; import refused".into(),
                );
            }
            let mut native: NativeContext =
                serde_json::from_slice(&bytes).map_err(|_| "native export incompatible")?;
            if native.actor_pubkey != staged.record.pubkey
                || native.harness_pid != g.pid
                || native.start_nonce != g.nonce
                || beekeeper_host_core::record::canonical_relay_key(&native.relay_url)
                    != beekeeper_host_core::record::canonical_relay_key(&staged.relay_url)
            {
                return Err("native export custody/generation mismatch".into());
            }
            let ack_bytes = serde_json::to_vec(&ack).map_err(|_| "drain evidence invalid")?;
            let mut selected = Vec::new();
            for session in &native.sessions {
                for artifact in &session.native_artifacts {
                    let bytes = std::fs::read(&artifact.source_path)
                        .map_err(|_| "selected native transcript unavailable; import blocked")?;
                    if bytes.len() as u64 != artifact.bytes
                        || hex::encode(Sha256::digest(&bytes)) != artifact.sha256
                    {
                        return Err(
                            "selected native transcript changed after drain; import blocked".into(),
                        );
                    }
                    let since = replay::source_start(
                        &session.runtime_name,
                        &bytes,
                        &session.resume_cursor,
                        &session.cwd,
                        staged
                            .replay_floor_unix
                            .ok_or("migration capture boundary missing")?,
                    )?;
                    staged.backfill_since_unix = Some(
                        staged
                            .backfill_since_unix
                            .map_or(since, |held| held.min(since)),
                    );
                    selected.push((
                        format!("native/{}", artifact.relative_path.to_string_lossy()),
                        bytes,
                    ));
                }
            }
            let mut files = vec![
                BackupFile {
                    name: "native-context.json",
                    bytes: &bytes,
                },
                BackupFile {
                    name: "drained.json",
                    bytes: &ack_bytes,
                },
            ];
            for (name, bytes) in &selected {
                files.push(BackupFile { name, bytes });
            }
            migration_backup::capture(
                &dir.join("backup-drained"),
                BackupScope {
                    actor_pubkey: staged.record.pubkey.clone(),
                    relay_url: staged.relay_url.clone(),
                    operation_id: Uuid::new_v5(&staged.operation_id, b"drained-context")
                        .to_string(),
                },
                &files,
            )?;
            for session in &mut native.sessions {
                for artifact in &mut session.native_artifacts {
                    artifact.source_path = dir
                        .join("backup-drained/native")
                        .join(&artifact.relative_path);
                }
            }
            if native.sessions.len() != 1 {
                return Err("native source has multiple contexts; explicit context selection required, backup retained".into());
            }
            staged.channel = Uuid::parse_str(&native.sessions[0].channel_id)
                .map_err(|_| "native source channel invalid")?;
            staged.cwd = native.sessions[0].cwd.clone();
            staged.native_context = Some(native);
        } else if ack.get("nativeContextAvailable").and_then(Value::as_bool) != Some(false) {
            return Err("native source availability is unconfirmed; import blocked".into());
        }
        // A clean drain certifies termination of this generation, not consumption
        // of every input before its ACK. Never derive a replay watermark from
        // filesystem time (which can also change when artifacts are copied).
        staged.quiesced = true;
        save(&dir.join("stage.json"), &staged)?;
    }
    if !staged.fresh && !host_owned {
        let ctx = context()?;
        let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
        let mut transfer = ownership::for_pair(
            &ctx.home,
            ctx.instance,
            &staged.record.pubkey,
            &staged.relay_url,
        )?
        .ok_or("staged ownership missing")?;
        if transfer.phase == ownership::Phase::Prepared {
            transfer.phase = ownership::Phase::Quiesced;
            ownership::advance(&ctx.home, ctx.instance, transfer)?;
        }
    }
    if !staged.fresh && !host_owned {
        revision::reconcile(app, &staged).await?;
    }
    let (public, mut private) = configuration_view(app, &staged.record)?;
    verify_source_unchanged(app, &staged)?;
    if let Some(since) = staged.backfill_since_unix {
        private.record["migration_backfill_since_unix"] = json!(since);
    }
    let command = private
        .record
        .get("effective_command")
        .and_then(Value::as_str)
        .ok_or("effective runtime unavailable")?;
    let runtime = config
        .runtimes
        .iter()
        .find(|r| {
            r.agent_command == command
                || crate::managed_agents::known_acp_runtime(command)
                    .is_some_and(|known| known.id == r.runtime)
        })
        .or_else(|| {
            config
                .runtimes
                .iter()
                .find(|r| r.instance_ref == beekeeper_host_core::managed_runtime::INSTANCE_REF)
        })
        .ok_or(
            "selected runtime is not commissioned; configure/install its host runtime and retry",
        )?;
    let request = CreateRequest {
        operation_id: staged.operation_id,
        channel: staged.channel,
        display_name: staged.record.name.clone(),
        system_prompt: staged.record.system_prompt.clone().unwrap_or_default(),
        home: Some(context()?.home.clone()),
        cwd: staged.cwd.clone(),
        project: staged.record.project_ref.clone(),
        model: staged.record.model.clone(),
        provider_instance: Some(runtime.instance_ref.clone()),
        agent_pubkey: host_owned.then(|| staged.record.pubkey.clone()),
        adopt_agent_pubkey: (!staged.fresh && !host_owned).then(|| staged.record.pubkey.clone()),
        key_file: (!staged.fresh).then(|| dir.join("backup-before/identity-key")),
        auth_tag_file: (!staged.fresh).then(|| dir.join("backup-before/auth.json")),
        public_definition: Some(public),
        private_config: Some(private),
        identity: Some(IdentityInput {
            keys: Keys::parse(&staged.record.private_key_nsec)
                .map_err(|_| "source identity unavailable")?,
            auth_tag: staged
                .record
                .auth_tag
                .clone()
                .ok_or("source attestation unavailable")?,
        }),
        activate: staged.activate,
        native_context: staged.native_context.clone(),
        replay_floor_unix: staged.replay_floor_unix,
        ..Default::default()
    };
    let mut request = request;
    request.wait_secs = 60;
    // Save the exact start intent before effects. The original import stage and
    // backup remain immutable source evidence; this journal follows successor
    // operations so reply loss does not resurrect the original context/floor.
    save_enrollment_stage(dir, &staged)?;
    let transport = Transport::owner(app, &staged.relay_url)?;
    let outcome = if host_owned && staged.activate {
        beekeeper_agent_management::start(&request, &transport, &context()?).await
    } else {
        beekeeper_agent_management::create(&request, &transport, &context()?).await
    }
    .map_err(|e| e.to_string())?;
    if host_owned
        && staged.activate
        && outcome.get("runtime_available").and_then(Value::as_bool) != Some(true)
    {
        return Err("host start is accepted but runtime availability remains unconfirmed; inspect shared status and repair provider/runtime".into());
    }
    if !host_owned
        && staged.activate
        && outcome.get("host_admitted").and_then(Value::as_bool) != Some(true)
    {
        return Err("host admission remains unconfirmed; source backup retained".into());
    }
    save(&dir.join("outcome.json"), &outcome)?;
    Ok(())
}

pub(crate) async fn enroll_new_host_agent(
    app: &AppHandle,
    _state: &AppState,
    actor: &str,
    relay: &str,
    activate: bool,
) -> Result<(), String> {
    let _guard = import_guard(app, actor, relay).await?;
    let record = record_for_pair(app, actor, relay)?;
    let (dir, staged) = stage(app, &record, relay, activate, true).await?;
    admit(app, &dir, staged).await
}

pub(super) async fn ensure_enrolled(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
    activate: bool,
) -> Result<(), String> {
    ensure_enrolled_with_floor(app, record, relay, activate, None).await
}
pub(super) async fn ensure_enrolled_with_floor(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
    activate: bool,
    requested_floor: Option<u64>,
) -> Result<(), String> {
    let _guard = import_guard(app, &record.pubkey, relay).await?;
    let existing = ownership::for_pair(
        &context()?.home,
        crate::agent_host::instance(),
        &record.pubkey,
        relay,
    )?;
    if let Some(held) = existing {
        if held.phase == ownership::Phase::Adopted {
            let dir = pair_directory(&record.pubkey, relay)?;
            let staged: ImportStage = serde_json::from_slice(&private_bytes(&enrollment_stage(
                &dir,
                &held.operation_id,
            )?)?)
            .map_err(|_| "host enrollment inputs invalid")?;
            if requested_floor.is_some() && requested_floor != staged.replay_floor_unix {
                return Err("this responsibility is already adopted; stop and start a new context to change its replay floor".into());
            }
            if staged.activate != activate {
                return Err(
                    "use shared Stop to change an adopted responsibility's activation".into(),
                );
            }
            return admit(app, &dir, staged).await;
        }
        if held.phase == ownership::Phase::HostStopped {
            let dir = pair_directory(&record.pubkey, relay)?;
            let bytes = private_bytes(&enrollment_stage(&dir, &held.operation_id)?)?;
            let mut staged: ImportStage =
                serde_json::from_slice(&bytes).map_err(|_| "host enrollment inputs invalid")?;
            let next = Uuid::parse_str(&held.operation_id)
                .map_err(|_| "prior ownership operation invalid")?;
            // A successor start is a new immutable request. The shared worker
            // verifies the original configured enrollment before admitting it.
            staged.operation_id = Uuid::new_v5(
                &next,
                format!("explicit-host-start:{requested_floor:?}").as_bytes(),
            );
            if let Some(pending) = journal::pending_successor(&dir, staged.operation_id)? {
                if pending.activate != activate {
                    return Err("pending start intent differs; protected operation retained".into());
                }
                return admit(app, &dir, pending).await;
            }
            staged.replay_floor_unix = requested_floor.or(staged.replay_floor_unix);
            staged.record = record.clone();
            staged.source_configuration = Some(configuration_view(app, record)?.1);
            staged.activate = activate;
            staged.native_context = None;
            return admit(app, &pair_directory(&record.pubkey, relay)?, staged).await;
        }
    }
    if requested_floor.is_some() {
        return Err("legacy enrollment requires its verified drain replay boundary; import first, then use an explicit floor for a new host context".into());
    }
    let (dir, staged) = stage(app, record, relay, activate, false).await?;
    admit(app, &dir, staged).await
}

pub(crate) async fn import_legacy_agents_on_setup(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let relay = crate::relay::relay_ws_url_with_override(&state);
    state.signing_keys()?;
    let mut errors = Vec::new();
    for record in load_managed_agents(app)? {
        // Unified store also holds key-less persona definitions. They have no
        // execution identity to migrate; malformed populated identities still
        // take the normal refusal path.
        if record.pubkey.is_empty() && record.private_key_nsec.is_empty() {
            continue;
        }
        if record.backend != BackendKind::Local {
            continue;
        }
        let effective = crate::relay::effective_agent_relay_url(&record.relay_url, &relay);
        if beekeeper_host_core::record::canonical_relay_key(&effective)
            != beekeeper_host_core::record::canonical_relay_key(&relay)
        {
            continue;
        }
        if ownership::for_pair(
            &context()?.home,
            crate::agent_host::instance(),
            &record.pubkey,
            &relay,
        )?
        .is_some_and(|t| {
            matches!(
                t.phase,
                ownership::Phase::Adopted | ownership::Phase::HostStopped
            )
        }) {
            continue;
        }
        let activate = record.runtime_pid.is_some() || record.start_on_app_launch;
        if let Err(error) = ensure_enrolled(app, &record, &relay, activate).await {
            errors.push(format!("{}: {error}", record.pubkey));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "agent migration blocked; backups retained and no desktop fallback: {}",
            errors.join("; ")
        ))
    }
}

#[cfg(test)]
#[path = "host_import_tests.rs"]
mod tests;
