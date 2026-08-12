//! Tauri surface for the coding-session provider host.
//!
//! Four commands, deliberately narrow: read status, provision (which also seeds
//! trust and starts), ensure running, stop. There is no "delete" — retiring an
//! identity would strand every transcript already signed by it, so that belongs
//! to an explicit rotation flow rather than a button.

use nostr::ToBech32;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::coding_sessions::workdir_store::remateralize_provider_projects_view;
use crate::relay::relay_ws_url_with_override;
use crate::session_provider::store::{
    load_provider_store, save_provider_store, CodingSessionProviderRecord,
};
use crate::session_provider::supervisor::{
    ensure_running, provider_status, stop_provider, CodingSessionProviderState,
    CodingSessionProviderStatus,
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
