use nostr::Keys;
use tauri::{AppHandle, State};

use super::managed_agent_definition::validate_create_definition;

use crate::{
    app_state::AppState,
    managed_agents::{
        build_managed_agent_summary, ensure_persona_is_active, find_managed_agent_mut,
        load_managed_agents, load_personas, load_teams, managed_agent_avatar_url,
        normalize_agent_args, resolve_provider_binary, save_managed_agents, try_regenerate_nest,
        validate_provider_config, BackendKind, CreateManagedAgentRequest,
        CreateManagedAgentResponse, ManagedAgentRecord, ManagedAgentSummary, RelayMeshConfig,
        DEFAULT_ACP_COMMAND, DEFAULT_AGENT_PARALLELISM, DEFAULT_AGENT_TURN_TIMEOUT_SECONDS,
    },
    relay::{relay_ws_url_with_override, sync_managed_agent_profile},
    util::now_iso,
};

/// Read the workspace owner pubkey without holding the lock. Used to populate `BEEKEEPER_ACP_AGENT_OWNER`
/// as a fallback for legacy agent records that have no NIP-OA `auth_tag`.
pub(super) fn workspace_owner_hex(state: &AppState) -> Result<String, String> {
    let keys = state.keys.lock().map_err(|e| e.to_string())?;
    Ok(keys.public_key().to_hex())
}

/// Build a summary from fresh disk state (personas, teams, global config).
/// For one-shot command paths only — the 5s list poll calls
/// `build_managed_agent_summary` directly with stores loaded once per call,
/// not once per record.
pub(super) fn summarize_from_disk(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<ManagedAgentSummary, String> {
    build_managed_agent_summary(
        app,
        record,
        &load_personas(app).unwrap_or_default(),
        &load_teams(app).unwrap_or_default(),
        &crate::managed_agents::load_global_agent_config(app).unwrap_or_default(),
    )
}

/// Render the shared host ownership projection without a desktop process registry.
pub(crate) fn summarize_host_record(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<ManagedAgentSummary, String> {
    summarize_from_disk(app, record)
}

/// Retain a freshly authored managed-agent event in the local store, flagged
/// for relay sync. MUST be called inside the `managed_agents_store_lock`-held
/// body after `save_managed_agents`, NEVER across an `.await`: it acquires
/// `state.keys` and a retention-db connection, both `std::sync` guards, and
/// drops them before returning.
///
/// Owner-authored, mirroring `commands::personas::retain_persona_pending`: the
/// owner keys sign, the d_tag is the agent's pubkey, so the coordinate is
/// `30177:<owner>:<agent_pubkey>`. The event content is the opt-IN
/// [`agent_event_content`] projection — the retention upsert's content-equality
/// guard compares this projection, so an operational start/stop that mutates
/// only runtime fields produces an identical row and never re-enqueues a
/// publish. Best-effort: a failure here is logged and swallowed so a retention
/// hiccup never blocks the disk-authoritative write.
pub(crate) fn retain_managed_agent_pending(
    app: &AppHandle,
    state: &AppState,
    record: &ManagedAgentRecord,
) {
    use crate::managed_agents::{reconcile::retain_agent_record, retention::open_retention_db};

    let result = (|| -> Result<(), String> {
        let scope = crate::managed_agents::retention::active_retention_scope(app, state)?;
        let conn = open_retention_db(&scope.db_path)?;
        // Shared engine with the boot-time reconcile: projection content diff
        // (no republish for runtime-only churn) + monotonic created_at bump
        // past the retained head (NIP-AP step 3).
        retain_agent_record(&conn, &scope.owner_keys, record).map(|_| ())
    })();
    if let Err(e) = result {
        eprintln!("beekeeper-desktop: agent-retain: {e}");
    }
}

fn normalize_relay_mesh(
    config: Option<&RelayMeshConfig>,
    backend: &BackendKind,
) -> Result<Option<RelayMeshConfig>, String> {
    let Some(config) = config else {
        return Ok(None);
    };

    let model_ref = config.model_ref.trim();
    if model_ref.is_empty() {
        return Err("Beekeeper shared compute model is required".to_string());
    }
    if backend != &BackendKind::Local {
        return Err("Beekeeper shared compute agents must use the local backend".to_string());
    }

    Ok(Some(RelayMeshConfig {
        model_ref: model_ref.to_string(),
    }))
}

fn trim_to_optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn resolve_created_avatar_url(
    requested_avatar_url: Option<&str>,
    persona_avatar_url: Option<String>,
    agent_command: &str,
) -> Option<String> {
    requested_avatar_url
        .and_then(trim_to_optional_string)
        .or_else(|| {
            persona_avatar_url
                .as_deref()
                .and_then(trim_to_optional_string)
        })
        .or_else(|| managed_agent_avatar_url(agent_command))
}

pub(crate) use provider_deploy::{deploy_to_provider, ProviderDeployOptions};

// Async: the blocking body (record reads, liveness syscalls, a possible save)
// runs via spawn_blocking, not the UI thread (it beachballed the agents menu).
// State is re-derived from the owned AppHandle: `State<'_, _>` is borrowed and
// `std::sync::MutexGuard` is not `Send`.
#[tauri::command]
pub async fn list_managed_agents(app: AppHandle) -> Result<Vec<ManagedAgentSummary>, String> {
    use tauri::Manager;
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let records = load_managed_agents(&app)?;

        let personas = load_personas(&app).unwrap_or_default();
        // One disk read for the whole list — build_managed_agent_summary takes
        // teams and config as parameters precisely so this poll-every-5s call
        // does not re-read them per record.
        let teams = load_teams(&app).unwrap_or_default();
        let global_config =
            crate::managed_agents::load_global_agent_config(&app).unwrap_or_default();
        records
            .iter()
            .map(|record| {
                build_managed_agent_summary(&app, record, &personas, &teams, &global_config)
            })
            .collect()
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

#[tauri::command]
pub async fn create_managed_agent(
    input: CreateManagedAgentRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CreateManagedAgentResponse, String> {
    let name = input.name.trim().to_string();
    let requested_persona_id = input
        .persona_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    validate_create_definition(&name, requested_persona_id.as_deref(), &input)?;
    if let Some(parallelism) = input.parallelism {
        if !(1..=32).contains(&parallelism) {
            return Err("parallelism must be between 1 and 32".to_string());
        }
    }
    crate::managed_agents::validate_user_env_keys(&input.env_vars)?;

    // Validate & normalize the respond-to allowlist BEFORE any side effects.
    // The harness has its own validator (beekeeper-acp/src/config.rs) but we want
    // to catch malformed input at the boundary so the agent never tries to
    // start with a list that will crash it on launch. The mode/allowlist
    // pairing (and the definition-default fallback) is resolved later at the
    // mint site via `resolve_mint_behavioral_defaults`, where the linked
    // definition is in hand.
    let respond_to_allowlist =
        crate::managed_agents::validate_respond_to_allowlist(&input.respond_to_allowlist)?;
    if input.respond_to == Some(crate::managed_agents::RespondTo::Allowlist)
        && respond_to_allowlist.is_empty()
    {
        return Err(
            "respond-to mode 'allowlist' requires at least one pubkey in the allowlist".to_string(),
        );
    }

    // ── Phase 1: mint the identity (sync lock) ───────────────────────────────
    // Keys and the NIP-OA auth tag come from the one shared minting path
    // (`mint_agent_identity`), so a crew-role install and a hand-made agent
    // can never disagree about how an agent identity is attested.
    let owner_signing_keys = state.signing_keys()?;
    let (agent_keys, private_key_nsec, pubkey, auth_tag, resolved_relay_url, input) = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let records = load_managed_agents(&app)?;

        if let Some(persona_id) = requested_persona_id.as_deref() {
            let personas = load_personas(&app)?;
            ensure_persona_is_active(&personas, persona_id)?;
        }
        let (keys, minted) = mint_agent_identity(&owner_signing_keys)?;
        let pubkey = minted.pubkey;
        if records.iter().any(|record| record.pubkey == pubkey) {
            return Err(format!("agent {pubkey} already exists"));
        }
        let private_key_nsec = minted.private_key_nsec;
        let auth_tag = minted.auth_tag;

        // Store the relay override exactly as supplied (trimmed). An explicit
        // value pins the agent; empty stays empty and resolves to the active
        // workspace relay at read-time. Uniform for Local and Provider.
        let resolved_relay_url = input
            .relay_url
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .to_string();

        (
            keys,
            private_key_nsec,
            pubkey,
            auth_tag,
            resolved_relay_url,
            input,
        )
    };

    // ── Pre-Phase 2: validate provider config BEFORE any side effects ────────
    if let BackendKind::Provider { ref config, ref id } = input.backend {
        validate_provider_config(config)?;
        // Validate via discovered candidates — not raw resolve_command.
        resolve_provider_binary(id)?;
    }

    let relay_mesh = normalize_relay_mesh(input.relay_mesh.as_ref(), &input.backend)?;

    // ── Phase 3: save record (sync lock) ───────────────────────────────────────
    let (_, resolved_avatar_url) = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut records = load_managed_agents(&app)?;

        // Guard against a duplicate pubkey appearing between phase 1 and phase 3
        // (extremely unlikely but safe to check).
        if records.iter().any(|record| record.pubkey == pubkey) {
            return Err(format!("agent {pubkey} already exists"));
        }
        // Provider config was already validated in Pre-Phase 2; cache the discovered binary path for deploy_to_provider.
        let provider_binary_path = if let BackendKind::Provider { ref id, .. } = input.backend {
            // Use resolve_provider_binary (discovered candidates only).
            resolve_provider_binary(id)
                .ok()
                .map(|p| p.display().to_string())
        } else {
            None
        };

        // Load personas once for harness/pack/avatar resolution below.
        let personas = load_personas(&app).unwrap_or_default();

        // Harness resolution: the persona's runtime is authoritative. A
        // persona-backed create stores an `agent_command_override` ONLY when the
        // user deliberately picked a divergent runtime (`harness_override`) —
        // e.g. AddChannelBotDialog's runtime selector. A divergence WITHOUT that
        // flag is a missing-runtime fallback from `resolvePersonaRuntime`, not a
        // pin, and must inherit so it doesn't freeze on the fallback harness once
        // the persona's runtime is installed. A persona-less create always
        // preserves the picked command as a real pin.
        let agent_command_override = crate::managed_agents::create_time_agent_command_override(
            requested_persona_id.as_deref(),
            &personas,
            input.agent_command.as_deref(),
            input.harness_override,
        );
        // The create-time snapshot used for arg/mcp/avatar derivations and
        // legacy reconcile. Authoritative spawn resolution re-derives this via
        // `effective_agent_command` at use-time.
        let agent_command = crate::managed_agents::effective_agent_command(
            requested_persona_id.as_deref(),
            &personas,
            agent_command_override.as_deref(),
        );
        let agent_args = normalize_agent_args(
            &agent_command,
            input
                .agent_args
                .iter()
                .map(|arg| arg.trim().to_string())
                .filter(|arg| !arg.is_empty())
                .collect::<Vec<_>>(),
        );

        // Derive MCP command exclusively from the runtime catalog — the
        // per-record field is never read at spawn time so user-supplied input
        // is silently discarded. Always sourcing from the catalog ensures
        // new agents pick up the correct value without any stored override.
        let mcp_command = match crate::managed_agents::known_acp_runtime(&agent_command) {
            Some(p) => p.mcp_command.unwrap_or("").to_string(),
            None => String::new(),
        };

        let team_id = input
            .team_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(team_id) = &team_id {
            if !load_teams(&app)?.iter().any(|team| &team.id == team_id) {
                return Err(format!("team {team_id} not found"));
            }
        }

        // Resolve the avatar URL once at creation and persist it on the record.
        // Explicit input wins, then the persona's own avatar, then the runtime
        // fallback. Storing it lets reconciliation compare against what was
        // actually published instead of re-deriving it.
        let persona_avatar_url = requested_persona_id.as_ref().and_then(|persona_id| {
            personas
                .iter()
                .find(|persona| persona.id == *persona_id)?
                .avatar_url
                .clone()
        });
        let resolved_avatar_url = resolve_created_avatar_url(
            input.avatar_url.as_deref(),
            persona_avatar_url,
            &agent_command,
        );

        // Pin the persona config onto the record at create. After this, spawn
        // and deploy read these snapshotted fields, never the live persona, so
        // the agent stays on the config it was created with across restarts;
        // delete+respawn re-runs create and rewrites the snapshot. env_vars are
        // NOT pinned: `record.env_vars` holds agent-level overrides only
        // (input.env_vars), and the live persona env is merged underneath at
        // read time (spawn / readiness / deploy) so persona credential edits
        // refresh on the next spawn like prompt/model/provider already do.
        let linked_persona = requested_persona_id.as_deref().and_then(|pid| {
            load_personas(&app)
                .ok()?
                .into_iter()
                .find(|persona| persona.id == pid)
        });
        let persona_snapshot = linked_persona
            .as_ref()
            .map(crate::managed_agents::persona_events::persona_snapshot);
        let snapshot_prompt = persona_snapshot
            .as_ref()
            .and_then(|s| s.system_prompt.clone());
        let snapshot_model = persona_snapshot.as_ref().and_then(|s| s.model.clone());
        let snapshot_provider = persona_snapshot.as_ref().and_then(|s| s.provider.clone());
        let snapshot_source_version = persona_snapshot.as_ref().map(|s| s.source_version.clone());
        let effective_provider = snapshot_provider
            .or_else(|| input.provider.as_deref().and_then(trim_to_optional_string));
        let mut effective_model =
            snapshot_model.or_else(|| input.model.as_deref().and_then(trim_to_optional_string));
        if effective_provider.as_deref() == Some(crate::managed_agents::RELAY_MESH_PROVIDER_ID)
            && effective_model.is_none()
        {
            effective_model = Some(crate::managed_agents::RELAY_MESH_AUTO_MODEL_ID.to_string());
        }

        // Mint-time behavioral quad: explicit input wins, then the linked
        // definition's NIP-AP defaults, then client defaults. The ONLY parse
        // point for definition behavioral strings — fails loudly on a bad
        // mode/range instead of minting an agent the author didn't describe.
        let minted = crate::managed_agents::resolve_mint_behavioral_defaults(
            input.respond_to,
            respond_to_allowlist.clone(),
            input.parallelism,
            linked_persona.as_ref(),
        )?;

        let record = crate::managed_agents::ManagedAgentRecord {
            pubkey: pubkey.clone(),
            name: name.clone(),
            persona_id: requested_persona_id.clone(),
            team_id,
            private_key_nsec: private_key_nsec.clone(),
            auth_tag: auth_tag.clone(),
            relay_url: resolved_relay_url.clone(),
            avatar_url: resolved_avatar_url.clone(),
            acp_command: input
                .acp_command
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(DEFAULT_ACP_COMMAND)
                .to_string(),
            agent_command,
            agent_command_override,
            agent_args,
            mcp_command,
            // BEEKEEPER_ACP_TURN_TIMEOUT is deprecated and ignored by the harness: schema default
            // only. idle_timeout_seconds / max_turn_duration_seconds control turn length.
            turn_timeout_seconds: DEFAULT_AGENT_TURN_TIMEOUT_SECONDS,
            // 0 or None → harness uses its own default (320s idle, 3600s max), and the CLI also clamps 0 → minimum.
            idle_timeout_seconds: input.idle_timeout_seconds.filter(|s| *s > 0),
            max_turn_duration_seconds: input.max_turn_duration_seconds.filter(|s| *s > 0),
            parallelism: minted.parallelism.unwrap_or(DEFAULT_AGENT_PARALLELISM),
            system_prompt: snapshot_prompt.or_else(|| {
                input
                    .system_prompt
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            }),
            model: effective_model.clone(),
            provider: effective_provider.clone(),
            persona_source_version: snapshot_source_version,
            // Provider agents are managed externally — force false.
            start_on_app_launch: if input.backend != BackendKind::Local {
                false
            } else {
                input.start_on_app_launch
            },
            auto_restart_on_config_change: true,
            runtime_pid: None,
            backend: input.backend.clone(),
            backend_agent_id: None,
            provider_policy_pending: false,
            provider_binary_path,
            persona_team_dir: None,
            persona_name_in_team: None,
            home_role: None,
            project_ref: None,
            project_public: None,
            carried_project_digest: None,
            project_publication_withdrawn: false,
            reserves_name_globally: false,
            env_vars: input.env_vars.clone(),
            created_at: now_iso(),
            updated_at: now_iso(),
            last_started_at: None,
            last_stopped_at: None,
            last_exit_code: None,
            last_error: None,
            last_error_code: None,
            respond_to: minted.respond_to,
            respond_to_allowlist: minted.respond_to_allowlist.clone(),
            display_name: None,
            slug: None,
            runtime: None,
            name_pool: Vec::new(),
            is_builtin: false,
            is_active: true,
            shared: false,
            source_team: None,
            source_team_persona_slug: None,
            catalog_source: None,
            definition_respond_to: None,
            definition_respond_to_allowlist: Vec::new(),
            definition_parallelism: None,
            relay_mesh: if effective_provider.as_deref()
                == Some(crate::managed_agents::RELAY_MESH_PROVIDER_ID)
            {
                effective_model
                    .clone()
                    .map(|model_ref| RelayMeshConfig { model_ref })
            } else {
                relay_mesh.clone()
            },
        };

        records.push(record);

        save_managed_agents(&app, &records)?;

        let record = records
            .iter()
            .find(|record| record.pubkey == pubkey)
            .ok_or_else(|| "created agent disappeared unexpectedly".to_string())?;
        // Publish the agent to the relay. Inside the Phase-3 lock, after save,
        // before any .await — owner-authored, every agent (Will's ruling: no
        // is_builtin/persona-membership gate).
        retain_managed_agent_pending(&app, &state, record);
        (summarize_from_disk(&app, record)?, resolved_avatar_url)
    };

    // Enroll through the same Rust service as the CLI. The saved record is
    // client custody/cache; it never grants desktop execution ownership.
    let mut spawn_error = None;
    if input.backend == BackendKind::Local {
        if let Err(error) = host_transfer::enroll_new_host_agent(
            &app,
            &state,
            &pubkey,
            &resolved_relay_url,
            input.spawn_after_create,
        )
        .await
        {
            spawn_error = Some(error.clone());
            let _store = state
                .managed_agents_store_lock
                .lock()
                .map_err(|e| e.to_string())?;
            let mut records = load_managed_agents(&app)?;
            let record = find_managed_agent_mut(&mut records, &pubkey)?;
            record.last_error = Some(format!("HOST_ENROLLMENT_UNCONFIRMED: {error}"));
            record.last_error_code = None;
            record.updated_at = now_iso();
            save_managed_agents(&app, &records)?;
        }
    }
    let records = load_managed_agents(&app)?;
    let record = records
        .iter()
        .find(|r| r.pubkey == pubkey)
        .ok_or("created agent unavailable")?;
    let agent = summarize_host_record(&app, record)?;

    try_regenerate_nest(&app);

    // ── Phase 4: sync agent profile on relay (async, outside lock) ───────────
    // Use the avatar persisted on the record so the published profile and any
    // later reconciliation agree on the same value.
    let profile_relay_url = crate::relay::effective_agent_relay_url(
        &resolved_relay_url,
        &relay_ws_url_with_override(&state),
    );
    let mut profile_sync_error = (sync_managed_agent_profile(
        &state,
        &profile_relay_url,
        &agent_keys,
        &name,
        resolved_avatar_url.as_deref(),
        auth_tag.as_deref(),
    )
    .await)
        .err();
    profile_sync_error =
        super::agent_models::flush_managed_agent_policy(&app, &state, profile_sync_error).await;

    let spawn_error = if input.spawn_after_create && input.backend != BackendKind::Local {
        // Keep create-time payload validation before the provider call. The
        // deploy boundary rebuilds after its per-agent lock so the provider
        // receives the newest saved policy rather than this earlier snapshot.
        {
            let _g = state
                .managed_agents_store_lock
                .lock()
                .map_err(|e| e.to_string())?;
            let records = load_managed_agents(&app)?;
            let rec = records
                .iter()
                .find(|r| r.pubkey == pubkey)
                .ok_or_else(|| "agent disappeared".to_string())?;
            build_deploy_payload(&app, &state, rec)?;
        }
        match deploy_to_provider(&app, &state, &pubkey, ProviderDeployOptions::default()).await {
            Ok(()) => spawn_error,
            Err(e) => Some(e),
        }
    } else {
        spawn_error
    };

    // Rebuild summary if provider deploy may have updated backend_agent_id.
    let final_agent = if input.backend != BackendKind::Local && spawn_error.is_none() {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|e| e.to_string())?;
        let records = load_managed_agents(&app)?;
        let record = records
            .iter()
            .find(|r| r.pubkey == pubkey)
            .ok_or_else(|| "agent disappeared".to_string())?;
        summarize_from_disk(&app, record)?
    } else {
        agent
    };

    Ok(CreateManagedAgentResponse {
        agent: final_agent,
        private_key_nsec,
        profile_sync_error,
        spawn_error,
    })
}

#[tauri::command]
pub async fn stop_managed_agent(
    pubkey: String,
    app: AppHandle,
) -> Result<ManagedAgentSummary, String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let relay = relay_ws_url_with_override(&state);
    host_transfer::stop_host_pair(&app, &pubkey, &relay).await?;
    let records = load_managed_agents(&app)?;
    let record = records
        .iter()
        .find(|r| r.pubkey == pubkey)
        .ok_or("agent not found")?;
    summarize_host_record(&app, record)
}

/// Retire shared execution before removing this client's cached custody.
#[tauri::command]
pub async fn delete_managed_agent(
    pubkey: String,
    force_remote_delete: Option<bool>,
    app: AppHandle,
) -> Result<(), String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let active_relay = relay_ws_url_with_override(&state);
    let cached = load_managed_agents(&app)?
        .into_iter()
        .find(|r| r.pubkey == pubkey);
    let cleanup_retry = cached.is_none();
    let expected = match cached {
        Some(record) => record,
        None => host_transfer::deletion_source(&pubkey, &active_relay)?,
    };
    if expected.backend != BackendKind::Local
        && expected.backend_agent_id.is_some()
        && !force_remote_delete.unwrap_or(false)
    {
        return Err(
            "cannot delete a deployed remote agent without force_remote_delete: true".into(),
        );
    }
    let relay = crate::relay::effective_agent_relay_url(
        &expected.relay_url,
        &relay_ws_url_with_override(&state),
    );
    host_transfer::delete_host_agent_snapshot(&app, &state, &expected, &relay).await?;
    if cleanup_retry {
        purge::purge_managed_agent_side_effects(
            &app,
            &state,
            &pubkey,
            expected.persona_id.as_deref(),
        )?;
        return Ok(());
    }
    let _store = state
        .managed_agents_store_lock
        .lock()
        .map_err(|e| e.to_string())?;
    let mut records = load_managed_agents(&app)?;
    let current = records
        .iter()
        .find(|r| r.pubkey == pubkey)
        .ok_or("agent cache changed during deletion")?;
    if current != &expected {
        return Err("agent configuration changed during deletion; shared outcome retained, source custody not purged".into());
    }
    records.retain(|r| r.pubkey != pubkey);
    save_managed_agents(&app, &records)?;
    purge::purge_managed_agent_side_effects(&app, &state, &pubkey, expected.persona_id.as_deref())?;
    try_regenerate_nest(&app);
    Ok(())
}

// Remote agent shutdown is handled entirely by the frontend:
// 1. Frontend sends "!shutdown" @mention via WebSocket (signed by user's key)
// 2. Harness sees it, exits gracefully, sets presence to "offline"
// 3. Desktop's existing presence polling sees "offline" — UI updates automatically
// No backend Tauri command needed. Presence IS the status.

#[path = "agents_purge.rs"]
mod purge;
pub(crate) use purge::purge_managed_agent_side_effects;

#[path = "agents_deploy.rs"]
mod deploy;
mod headless_transport;
pub(crate) mod host_transfer;
mod mint;
pub(super) mod provider_access;
mod provider_deploy;
mod start;
pub(super) use deploy::build_deploy_payload;
#[cfg(test)]
use deploy::{deploy_payload_json, DeployProjections};
#[cfg(test)]
use deploy::{ensure_remote_provider_supported, resolve_deploy_model_provider};
pub use host_transfer::get_managed_agent_host_transfer;
pub(crate) use mint::mint_agent_identity;
pub use start::start_managed_agent;

#[path = "agents_profile.rs"]
mod profile;
pub(crate) use profile::*;
#[cfg(test)]
use profile::{profile_needs_sync, resolve_legacy_avatar};

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
