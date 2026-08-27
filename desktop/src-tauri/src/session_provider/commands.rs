//! Tauri surface for the coding-session provider host.
//!
//! Four commands, deliberately narrow: read status, provision (which also seeds
//! trust and starts), ensure running, stop. There is no "delete" — retiring an
//! identity would strand every transcript already signed by it, so that belongs
//! to an explicit rotation flow rather than a button.

use std::collections::BTreeMap;

use nostr::ToBech32;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::remateralize_provider_projects_view;
use crate::relay::relay_ws_url_with_override;
use crate::session_provider::store::{
    load_provider_store, save_provider_store, CodingSessionProviderRecord,
};
use crate::session_provider::supervisor::{
    ensure_running, provider_status, resolve_claude_code_executable, stop_provider,
    CodingSessionProviderState, CodingSessionProviderStatus,
};
use crate::session_provider::trust;
use crate::util::now_iso;

/// Serialize the load-mint-save sequence. React development mode may issue the
/// provisioning command twice while checking effect cleanup; without a lock,
/// both calls can observe an empty store and mint competing identities.
static PROVISION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Number of hex characters of the provider pubkey used as the instance id.
///
/// Must match `buzz_session_provider::config::INSTANCE_ID_PUBKEY_PREFIX_LEN`:
/// the provider derives the same value when `BUZZ_CSP_INSTANCE_ID` is unset,
/// and a mismatch would silently split one provider across two `cs-target`
/// identities.
const INSTANCE_ID_PUBKEY_PREFIX_LEN: usize = 16;

/// Model catalog for one coding-session runtime.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionProviderModels {
    /// The runtime instance these models belong to.
    pub instance_ref: String,
    /// Adapter selection used when the person makes no explicit choice.
    pub default_model: String,
    /// Every adapter-advertised selection value, in adapter order.
    pub allowed_models: Vec<String>,
}

/// Whether a provider identity exists for the active relay, and whether the
/// desktop is currently supervising it.
#[tauri::command]
pub async fn coding_session_provider_status(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: State<'_, CodingSessionProviderState>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = relay_ws_url_with_override(&state);
    provider_status(&app, &provider, &relay_url)
}

/// The session-capacity setting, and what the running provider is enforcing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionCapacitySettings {
    /// The person's stored ceiling: `None` for the provider default, `Some(0)`
    /// for unlimited, `Some(n)` for n live agent processes.
    pub max_sessions: Option<usize>,
    /// The provider's own default, so the UI can name it rather than repeat a
    /// number that lives in the sidecar.
    pub default_max_sessions: usize,
    /// The ceiling the **running** provider started with, when one is running.
    ///
    /// A change to `max_sessions` reaches the child only at its next start, so
    /// a surface that shows the stored value alone would claim a ceiling that
    /// is not being enforced.
    pub running_max_sessions: Option<usize>,
    /// The person's stored per-turn silence budget, or `None` for the default.
    pub turn_idle_timeout_secs: Option<u64>,
    /// The provider's own default silence budget, in seconds.
    pub default_turn_idle_timeout_secs: u64,
    /// The budget the **running** provider started with, when one is running.
    pub running_turn_idle_timeout_secs: Option<u64>,
    /// The person's stored crew turn budget, or `None` for the default.
    pub turn_budget: Option<u64>,
    /// The provider's own default crew turn budget.
    pub default_turn_budget: u64,
    /// The budget the **running** provider started with, when one is running.
    pub running_turn_budget: Option<u64>,
}

/// Read the stored ceiling alongside what is actually in force.
#[tauri::command]
pub async fn coding_session_capacity_settings(
    app: AppHandle,
    provider: State<'_, CodingSessionProviderState>,
) -> Result<CodingSessionCapacitySettings, String> {
    let store = crate::session_provider::store::load_provider_store(&app)?;
    Ok(CodingSessionCapacitySettings {
        max_sessions: store.max_sessions,
        default_max_sessions: buzz_session_provider_pkg::config::DEFAULT_MAX_SESSIONS,
        running_max_sessions: provider.running_max_sessions(),
        turn_idle_timeout_secs: store.turn_idle_timeout_secs,
        default_turn_idle_timeout_secs:
            buzz_session_provider_pkg::config::DEFAULT_IDLE_TIMEOUT_SECS,
        running_turn_idle_timeout_secs: provider.running_turn_idle_timeout_secs(),
        turn_budget: store.turn_budget,
        default_turn_budget: buzz_session_provider_pkg::config::DEFAULT_TURN_BUDGET,
        running_turn_budget: provider.running_turn_budget(),
    })
}

/// Store a new ceiling. `None` restores the provider default; `Some(0)` is
/// unlimited.
#[tauri::command]
pub async fn set_coding_session_capacity(
    app: AppHandle,
    provider: State<'_, CodingSessionProviderState>,
    max_sessions: Option<usize>,
) -> Result<CodingSessionCapacitySettings, String> {
    let mut store = crate::session_provider::store::load_provider_store(&app)?;
    store.max_sessions = max_sessions;
    crate::session_provider::store::save_provider_store(&app, &store)?;
    coding_session_capacity_settings(app, provider).await
}

/// Store a new per-turn silence budget. `None` restores the provider default.
///
/// A turn dies when the adapter says *nothing* for this long — every line it
/// writes resets the clock — so the number a person wants here is "the longest
/// my agent may run a silent command", not "the longest a turn may take". A
/// build or a test suite that reports only on completion is what spends it.
#[tauri::command]
pub async fn set_coding_session_turn_idle_timeout(
    app: AppHandle,
    provider: State<'_, CodingSessionProviderState>,
    turn_idle_timeout_secs: Option<u64>,
) -> Result<CodingSessionCapacitySettings, String> {
    if turn_idle_timeout_secs == Some(0) {
        // Zero would be "die on the first quiet millisecond", which is not a
        // setting anyone wants and is not what the provider reads it as.
        return Err("a turn idle timeout of zero seconds would end every turn immediately; leave it unset for the provider default".to_string());
    }
    let mut store = crate::session_provider::store::load_provider_store(&app)?;
    store.turn_idle_timeout_secs = turn_idle_timeout_secs;
    crate::session_provider::store::save_provider_store(&app, &store)?;
    coding_session_capacity_settings(app, provider).await
}

/// Store a new crew turn budget. `None` restores the provider default;
/// `Some(0)` removes the budget.
///
/// The budget bounds one *umbrella* — every execution a crew session launched,
/// counted together — and it bounds only turns the session's founder did not
/// sign. It is the floor under an unattended crew: a seat that has taken its
/// allowance is refused with a signed receipt naming the two numbers, rather
/// than looping unobserved.
#[tauri::command]
pub async fn set_coding_session_turn_budget(
    app: AppHandle,
    provider: State<'_, CodingSessionProviderState>,
    turn_budget: Option<u64>,
) -> Result<CodingSessionCapacitySettings, String> {
    let mut store = crate::session_provider::store::load_provider_store(&app)?;
    store.turn_budget = turn_budget;
    crate::session_provider::store::save_provider_store(&app, &store)?;
    coding_session_capacity_settings(app, provider).await
}

/// Probe one runtime's model surface.
///
/// Every runtime that opts into discovery is probed through **its own**
/// adapter: the probe itself is driver-agnostic (`buzz-acp models --json`
/// drives whatever `BUZZ_ACP_AGENT_COMMAND` names), and resolving
/// `claude-agent-acp` here regardless of the runtime asked about would report
/// Claude's models under another runtime's label. Runtimes that do not opt in
/// advertise the static `"default"` alias without spawning anything — their
/// adapters resolve the real model at session time. An unknown ref is an error.
#[tauri::command]
pub async fn coding_session_provider_models(
    instance_ref: Option<String>,
) -> Result<CodingSessionProviderModels, String> {
    let instance_ref = instance_ref.unwrap_or_else(|| "claude-primary".to_string());
    let Some(discover_models) =
        crate::session_provider::runtimes::known_instance_ref(&instance_ref)
    else {
        return Err(format!(
            "unknown coding-session runtime instanceRef: {instance_ref}"
        ));
    };
    if !discover_models {
        return Ok(CodingSessionProviderModels {
            instance_ref,
            default_model: "default".to_string(),
            allowed_models: vec!["default".to_string()],
        });
    }

    let resolved_acp = crate::managed_agents::resolve_command("buzz-acp")
        .ok_or_else(|| "buzz-acp was not found; rebuild the desktop sidecars".to_string())?;
    let probe = crate::session_provider::runtimes::runtime_probe_target(&instance_ref)?;
    let mut env = BTreeMap::new();
    if probe.needs_claude_executable {
        if let Some(claude) = resolve_claude_code_executable() {
            env.insert(
                "CLAUDE_CODE_EXECUTABLE".to_string(),
                claude.to_string_lossy().into_owned(),
            );
        }
    }
    let response = crate::commands::agent_model_process::run_agent_models_command(
        resolved_acp,
        probe.agent_command.to_string_lossy().into_owned(),
        probe.agent_args.clone(),
        None,
        env,
    )
    .await?;
    coding_session_provider_models_from_response(&instance_ref, response, probe.label)
}

/// Every runtime this desktop can offer, installed or not, with install/auth
/// readiness — the picker needs uninstalled rows to render install and sign-in
/// affordances. Fast CLI auth probes only; never spawns ACP adapters.
#[tauri::command]
pub async fn coding_session_provider_runtimes(
) -> Result<Vec<crate::session_provider::runtimes::CodingSessionProviderRuntime>, String> {
    // The auth probes are blocking child-process waits (up to 10s each), so
    // they run off the async executor.
    tauri::async_runtime::spawn_blocking(crate::session_provider::runtimes::list_runtimes)
        .await
        .map_err(|error| format!("runtime discovery task failed: {error}"))
}

pub(crate) fn coding_session_provider_models_from_response(
    instance_ref: &str,
    response: crate::managed_agents::AgentModelsResponse,
    runtime_label: &str,
) -> Result<CodingSessionProviderModels, String> {
    let allowed_models: Vec<String> = response.models.into_iter().map(|model| model.id).collect();
    let default_model = response
        .agent_default_model
        .filter(|model| allowed_models.contains(model))
        .or_else(|| allowed_models.first().cloned())
        .ok_or_else(|| format!("the {runtime_label} ACP adapter returned no selectable models"))?;
    Ok(CodingSessionProviderModels {
        instance_ref: instance_ref.to_string(),
        default_model,
        allowed_models,
    })
}

/// Provision a provider identity for the active relay, seed the bridge trust
/// allowlist, and start it.
///
/// Idempotent: an existing record is reused rather than replaced. Re-minting
/// would orphan the previous identity's state directory and every 442xx event
/// already attributed to it.
#[tauri::command]
pub async fn provision_coding_session_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: State<'_, CodingSessionProviderState>,
) -> Result<CodingSessionProviderStatus, String> {
    let _provision_guard = PROVISION_LOCK
        .lock()
        .map_err(|_| "coding-session provider provisioning lock is poisoned".to_string())?;
    let relay_url = relay_ws_url_with_override(&state);
    let mut store = load_provider_store(&app)?;
    if store.get(&relay_url).is_none() {
        let owner_keys = state.signing_keys()?;
        let record = mint_provider_record(&owner_keys, &relay_url)?;
        store.upsert(&relay_url, record);
        save_provider_store(&app, &store)?;
    }

    let Some(record) = store.get(&relay_url) else {
        return Err("failed to provision a coding-session provider identity".to_string());
    };
    // Trust seeding is part of provisioning, not a side effect of starting: a
    // provider whose output the desktop refuses to render is worse than one
    // that never started.
    trust::seed_provider_trust(&app, &record.provider_pubkey)?;
    // The state directory only exists from here on, so any working directory
    // the operator chose before provisioning has had nowhere to land. Write it
    // now, before the child starts reading the file.
    if let Err(error) = remateralize_provider_projects_view(&app, &state) {
        eprintln!("buzz-desktop: failed to materialize coding-session projects view: {error}");
    }

    ensure_running(&app, &provider, &relay_url)?;
    provider_status(&app, &provider, &relay_url)
}

/// Start the provisioned provider if it is not already supervised.
#[tauri::command]
pub async fn ensure_coding_session_provider_running(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: State<'_, CodingSessionProviderState>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = relay_ws_url_with_override(&state);
    ensure_running(&app, &provider, &relay_url)?;
    provider_status(&app, &provider, &relay_url)
}

/// Stop the supervised provider. The record and its state directory survive, so
/// a later start resumes the same identity and outbox.
#[tauri::command]
pub async fn stop_coding_session_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: State<'_, CodingSessionProviderState>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = relay_ws_url_with_override(&state);
    stop_provider(&provider);
    provider_status(&app, &provider, &relay_url)
}

/// Generate a provider keypair and attest it with the owner key.
///
/// Extracted from the command body so the record shape and the NIP-OA minting
/// contract are unit-testable without an `AppHandle`.
pub(crate) fn mint_provider_record(
    owner_keys: &nostr::Keys,
    relay_url: &str,
) -> Result<CodingSessionProviderRecord, String> {
    let keys = nostr::Keys::generate();
    let provider_pubkey = keys.public_key().to_hex();
    let private_key_nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|error| format!("failed to encode provider private key: {error}"))?;
    let instance_id = provider_pubkey
        .get(..INSTANCE_ID_PUBKEY_PREFIX_LEN)
        .unwrap_or(&provider_pubkey)
        .to_string();

    // Empty conditions, matching every other Buzz-minted attestation: the tag
    // proves ownership, and the relay's own scope rules decide what the key may
    // write. `compute_auth_tag` already returns the JSON array of strings that
    // `BUZZ_AUTH_TAG` carries, so it is stored verbatim.
    let auth_tag = buzz_sdk_pkg::nip_oa::compute_auth_tag(owner_keys, &keys.public_key(), "")
        .map_err(|error| format!("failed to compute NIP-OA auth tag: {error}"))?;

    Ok(CodingSessionProviderRecord {
        provider_pubkey,
        instance_id,
        auth_tag: Some(auth_tag),
        created_at: now_iso(),
        relay_url: relay_url.to_string(),
        private_key_nsec,
    })
}
