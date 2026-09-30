//! Tauri surface for the coding-session provider.
//!
//! Deliberately narrow: read status, provision (which also seeds trust,
//! commissions the agent host and starts), ensure running, stop. There is no
//! "delete" — retiring an identity would strand every transcript already
//! signed by it, so that belongs to an explicit rotation flow rather than a
//! button.
//!
//! **Every one of these is now a request to `beekeeper-host`, not an action this
//! app takes.** The app writes the files the host reads and then asks over the
//! control socket; it spawns nothing and reaps nothing. What that buys is the
//! whole point of the split: a coding session outlives this window.

use std::collections::BTreeMap;

use nostr::ToBech32;
use tauri::{AppHandle, State};

use crate::agent_host::{commission, AgentHost};
use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::remateralize_provider_projects_view;
use crate::relay::relay_ws_url_with_override;
use crate::session_provider::status::{
    provider_status, resolve_claude_code_executable, CodingSessionProviderStatus,
};
use crate::session_provider::store::{
    load_provider_store, save_provider_store, CodingSessionProviderRecord,
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

/// Whether a provider identity exists for the active relay, and what the agent
/// host says about it.
///
/// No longer "whether this desktop is supervising it": that was a different
/// claim from "a provider is running" and read identically in the UI.
#[tauri::command]
pub async fn coding_session_provider_status(
    app: AppHandle,
    state: State<'_, AppState>,
    host: State<'_, AgentHost>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = relay_ws_url_with_override(&state);
    provider_status(&app, &host, &relay_url).await
}

/// Register the agent host — and the menu bar app — to start at login.
///
/// The answer to the question `status.host.login == "shouldAsk"` asks. Also
/// the way back from a refusal, because `install` clears it: a person who
/// changes their mind in settings must not have to find a file.
///
/// Returns the whole status rather than just the registration, so the surface
/// that called this re-renders from one answer instead of stitching two
/// together.
#[tauri::command]
pub async fn install_agent_host_autostart(
    app: AppHandle,
    state: State<'_, AppState>,
    host: State<'_, AgentHost>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = relay_ws_url_with_override(&state);
    // Warnings are not an error: a registration that could not be written
    // travels in `status.host.autostart.warnings`, and failing the command
    // would leave the surface with nothing to show but a toast.
    let registration = crate::agent_host::autostart::ensure_registered(
        &app,
        // A person asked, so look properly rather than cheaply.
        crate::agent_host::autostart::Probe::both(),
    );
    for warning in &registration.warnings {
        eprintln!("buzz-desktop: agent-host: {warning}");
    }
    provider_status(&app, &host, &relay_url).await
}

/// Record that this machine's operator does not want the host at login.
///
/// Removes both registrations and remembers the answer, so nothing proposes it
/// again. The provider keeps running if it is running — declining the *login*
/// registration is not a request to stop anything now.
#[tauri::command]
pub async fn decline_agent_host_autostart(
    app: AppHandle,
    state: State<'_, AppState>,
    host: State<'_, AgentHost>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = relay_ws_url_with_override(&state);
    crate::agent_host::autostart::decline()?;
    provider_status(&app, &host, &relay_url).await
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
///
/// The "running" half comes from the host, which knows what it started the
/// child with. It is `None` whenever nothing is running *or* the host could
/// not be reached — both of which mean "no ceiling is being enforced", which
/// is the honest answer and the one the panel already renders.
#[tauri::command]
pub async fn coding_session_capacity_settings(
    app: AppHandle,
    host: State<'_, AgentHost>,
) -> Result<CodingSessionCapacitySettings, String> {
    let store = crate::session_provider::store::load_provider_store(&app)?;
    let in_force = host
        .snapshot()
        .await
        .status
        .and_then(|status| status.provider_settings_in_force);
    Ok(CodingSessionCapacitySettings {
        max_sessions: store.max_sessions,
        default_max_sessions: buzz_session_provider_pkg::config::DEFAULT_MAX_SESSIONS,
        running_max_sessions: in_force.and_then(|settings| settings.max_sessions),
        turn_idle_timeout_secs: store.turn_idle_timeout_secs,
        default_turn_idle_timeout_secs:
            buzz_session_provider_pkg::config::DEFAULT_IDLE_TIMEOUT_SECS,
        running_turn_idle_timeout_secs: in_force
            .and_then(|settings| settings.turn_idle_timeout_secs),
        turn_budget: store.turn_budget,
        default_turn_budget: buzz_session_provider_pkg::config::DEFAULT_TURN_BUDGET,
        running_turn_budget: in_force.and_then(|settings| settings.turn_budget),
    })
}

/// Store a new ceiling. `None` restores the provider default; `Some(0)` is
/// unlimited.
#[tauri::command]
pub async fn set_coding_session_capacity(
    app: AppHandle,
    host: State<'_, AgentHost>,
    max_sessions: Option<usize>,
) -> Result<CodingSessionCapacitySettings, String> {
    let mut store = crate::session_provider::store::load_provider_store(&app)?;
    store.max_sessions = max_sessions;
    crate::session_provider::store::save_provider_store(&app, &store)?;
    coding_session_capacity_settings(app, host).await
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
    host: State<'_, AgentHost>,
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
    coding_session_capacity_settings(app, host).await
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
    host: State<'_, AgentHost>,
    turn_budget: Option<u64>,
) -> Result<CodingSessionCapacitySettings, String> {
    let mut store = crate::session_provider::store::load_provider_store(&app)?;
    store.turn_budget = turn_budget;
    crate::session_provider::store::save_provider_store(&app, &store)?;
    coding_session_capacity_settings(app, host).await
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
    host: State<'_, AgentHost>,
    expected_relay_url: Option<String>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = {
        let _provision_guard = PROVISION_LOCK
            .lock()
            .map_err(|_| "coding-session provider provisioning lock is poisoned".to_string())?;
        // Resolve after taking the lock: another provisioning call may have
        // waited here while the person switched communities. An explicit
        // caller pin never falls through to whichever relay happens to be
        // active now.
        let relay_url = provider_command_relay(&state, expected_relay_url.as_deref())?;
        let mut store = load_provider_store(&app)?;
        if store.get(&relay_url).is_none() {
            let owner_keys = state.signing_keys()?;
            let record = mint_provider_record(&owner_keys, &relay_url)?;
            store.upsert(&relay_url, record);
            save_provider_store(&app, &store)?;
        }
        if store.get(&relay_url).is_none() {
            return Err("failed to provision a coding-session provider identity".to_string());
        }
        relay_url
    };
    // The guard is released before the `await`s below on purpose: a
    // `std::sync::Mutex` held across an await point would block the executor
    // thread, and everything it protects — the load-mint-save sequence — is
    // already done.

    let store = load_provider_store(&app)?;
    let Some(record) = store.get(&relay_url).cloned() else {
        return Err("failed to provision a coding-session provider identity".to_string());
    };
    // Trust seeding is part of provisioning, not a side effect of starting: a
    // provider whose output the desktop refuses to render is worse than one
    // that never started.
    trust::seed_provider_trust(&app, &record.provider_pubkey)?;
    // The state directory only exists from here on, so any working directory
    // the operator chose before provisioning has had nowhere to land. Write it
    // now, before the child starts reading the file.
    if let Err(error) = remateralize_provider_projects_view(&app, &relay_url) {
        eprintln!("buzz-desktop: failed to materialize coding-session projects view: {error}");
    }

    // Hand the identity to the agent host: write `host.json` and the 0600 key
    // file, then ask it to look. This is the moment "after Beekeeper is
    // installed and commissioned" refers to.
    commission::commission_host(
        &app,
        &host,
        &relay_url,
        &record,
        &store,
        commission::Commissioning::FirstIdentity,
    )
    .await?;

    // "After Beekeeper is installed and commissioned" — this is that moment.
    // Never fatal, always disclosed: the registration is read back and travels
    // in `status.host.autostart`, because a commissioning that succeeded while
    // the registration silently did not is how a person comes to believe their
    // agents survive a reboot when they do not.
    for warning in &crate::agent_host::autostart::ensure_registered(
        &app,
        crate::agent_host::autostart::Probe::both(),
    )
    .warnings
    {
        eprintln!("buzz-desktop: agent-host: {warning}");
    }

    ensure_host_running(&app, &host, &relay_url).await;
    provider_status(&app, &host, &relay_url).await
}

/// Ask the host to start the provider, then repair this machine's roster
/// standing for every project it has a saved association with (ledger 266).
///
/// The admission half is not optional and not conditional on the start
/// succeeding: a provider that has been running since before this app launched
/// never passes through a start path at all, which is exactly how run 8
/// (2026-09-25) left the host off the roster. Reconciling on every attempt is
/// what makes that unreachable.
async fn ensure_host_running(app: &AppHandle, host: &AgentHost, relay_url: &str) {
    if let Err(error) = host.start().await {
        // Disclosed, not fatal: the status this returns to carries the
        // reachability, and the surface renders it. Failing the whole command
        // would hide a provisioning that did succeed.
        eprintln!("buzz-desktop: agent-host: could not start the provider: {error}");
    }
    crate::managed_agents::project_admission::reconcile_saved_project_admissions(app, relay_url);
}

/// Start the provisioned provider if it is not already supervised, then
/// reconcile this host's admission to every project it has a saved
/// association with (ledger 266).
#[tauri::command]
pub async fn ensure_coding_session_provider_running(
    app: AppHandle,
    state: State<'_, AppState>,
    host: State<'_, AgentHost>,
    expected_relay_url: Option<String>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = provider_command_relay(&state, expected_relay_url.as_deref())?;
    // Re-assert the commissioning first: the host may have been installed,
    // reinstalled or pointed at another community since this identity was
    // provisioned, and `host.json` is the only thing that tells it which.
    if let Err(error) = commission::rebind_host(&app, &host, &relay_url).await {
        eprintln!("buzz-desktop: agent-host: could not rebind: {error}");
    }
    ensure_host_running(&app, &host, &relay_url).await;
    provider_status(&app, &host, &relay_url).await
}

/// Resolve an optional caller-owned relay pin without ever redirecting it to a
/// newly active community. Legacy callers omit the pin and retain the original
/// active-community behavior.
pub(crate) fn provider_command_relay(
    state: &AppState,
    expected_relay_url: Option<&str>,
) -> Result<String, String> {
    let active = relay_ws_url_with_override(state);
    provider_command_relay_for_active(&active, expected_relay_url)
}

pub(crate) fn provider_command_relay_for_active(
    active: &str,
    expected_relay_url: Option<&str>,
) -> Result<String, String> {
    let Some(expected) = expected_relay_url else {
        return Ok(active.to_string());
    };
    let expected = expected.trim().trim_end_matches('/');
    if expected.is_empty()
        || crate::session_provider::canonical_relay_key(expected)
            != crate::session_provider::canonical_relay_key(active)
    {
        return Err(
            "the active community changed before the provider operation; retry Prepare".into(),
        );
    }
    Ok(expected.to_string())
}

/// Ask the host to stop the provider. The record and its state directory
/// survive, so a later start resumes the same identity and outbox.
///
/// This is now the *only* way a person stops a provider from this app —
/// quitting Beekeeper no longer does it, which is the point.
#[tauri::command]
pub async fn stop_coding_session_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    host: State<'_, AgentHost>,
) -> Result<CodingSessionProviderStatus, String> {
    let relay_url = relay_ws_url_with_override(&state);
    // A failure here is reported, not swallowed: "stop" is a request a person
    // made, and a stop that did not happen must not read as one that did.
    host.stop().await?;
    provider_status(&app, &host, &relay_url).await
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

/// One recovered redaction, as the UI receives it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedRedaction {
    /// The value as it was before redaction.
    pub plaintext: String,
    /// The rule that caught it — `host-path` or `structural`.
    pub class: String,
}

/// Resolve published redaction digests against this machine's own vault.
///
/// The provider redacts host-private values *before signing*, so the plaintext
/// exists only on the machine that produced the transcript. This is that
/// machine asking itself what it removed — never the network.
///
/// Three gates, in order:
///
/// 1. **Locality.** `provider_pubkey` must be an identity this desktop
///    provisioned for the active relay. A transcript signed by someone else's
///    provider resolves nothing, even if a digest happens to match: the vault
///    is keyed by session id, and two machines can legitimately redact the same
///    path.
/// 2. **Class.** Only recoverable classes were ever written
///    (`RedactionClass::is_recoverable`), so a credential cannot be returned
///    here regardless of what is asked for.
/// 3. **Path safety.** The session id comes from a relay event; the vault
///    proves it names one component inside the vault before opening anything.
///
/// An empty result is not a claim. "Never recorded", "expired", and "this is
/// not the machine that produced it" are indistinguishable to the caller, and
/// the UI must not invent a label for a state it cannot prove.
#[tauri::command]
pub async fn coding_session_resolve_redactions(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_pubkey: String,
    session_id: String,
    digests: Vec<String>,
) -> Result<BTreeMap<String, ResolvedRedaction>, String> {
    /// Bound on one lookup, so a hostile or broken transcript cannot ask this
    /// command to scan a vault file once per digest for an unbounded list.
    const MAX_DIGESTS_PER_LOOKUP: usize = 512;

    if digests.is_empty() {
        return Ok(BTreeMap::new());
    }
    if digests.len() > MAX_DIGESTS_PER_LOOKUP {
        return Err(format!(
            "too many digests in one lookup: {} (max {MAX_DIGESTS_PER_LOOKUP})",
            digests.len()
        ));
    }

    let relay_url = relay_ws_url_with_override(&state);
    let store = load_provider_store(&app)?;
    let local = store
        .get(&relay_url)
        .is_some_and(|record| record.provider_pubkey == provider_pubkey);
    if !local {
        // Not this machine's transcript. Silence rather than an error: a remote
        // session is an ordinary, expected state, not a failure.
        return Ok(BTreeMap::new());
    }

    let state_dir = crate::session_provider::provider_state_dir(&app, &provider_pubkey)?;
    let found =
        buzz_session_provider_pkg::redaction_vault::resolve(&state_dir, &session_id, &digests)
            .map_err(|error| format!("failed to read the redaction vault: {error}"))?;

    Ok(found
        .into_iter()
        .map(|(digest, entry)| {
            (
                digest,
                ResolvedRedaction {
                    plaintext: entry.plaintext,
                    class: entry.class,
                },
            )
        })
        .collect())
}
