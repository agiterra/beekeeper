//! Seeding the consumer's bridge allowlist with the local provider.
//!
//! The coding-session consumer is fail-closed: it renders only events signed by
//! a pubkey listed in `GlobalAgentConfig::allowed_bridge_pubkeys`. Provisioning
//! a local provider is therefore also a trust decision, and it is made here —
//! once, idempotently, at the moment the user asks for a provider.
//!
//! Seeding is re-applied on every start attempt rather than only at
//! provisioning. The allowlist lives in a file an unrelated settings save can
//! rewrite, and a provider that runs while the desktop refuses to render its
//! output is a silent, confusing failure. Re-asserting the entry costs one file
//! read and makes that state unreachable.

use tauri::AppHandle;

use crate::managed_agents::{
    load_global_agent_config, save_global_agent_config, AllowedBridgePubkey, GlobalAgentConfig,
};

/// Label attached to a self-provisioned provider entry. Display metadata only.
pub(crate) const LOCAL_PROVIDER_LABEL: &str = "This computer (coding sessions)";

/// Add `pubkey` to the allowlist if it is not already present.
///
/// Returns `true` when `config` changed. Pure so the idempotence property is
/// testable without an `AppHandle` or a real config file.
///
/// Comparison is case-insensitive even though stored entries are normalized to
/// lowercase: a hand-edited config with an uppercase entry must be recognized
/// as "already trusted" rather than duplicated — `validate_global_config`
/// rejects duplicates, so a second entry would make the config unsavable.
pub(crate) fn append_allowed_bridge_pubkey(
    config: &mut GlobalAgentConfig,
    pubkey: &str,
    label: &str,
) -> bool {
    let normalized = pubkey.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    if config
        .allowed_bridge_pubkeys
        .iter()
        .any(|entry| entry.pubkey.trim().eq_ignore_ascii_case(&normalized))
    {
        return false;
    }
    config.allowed_bridge_pubkeys.push(AllowedBridgePubkey {
        pubkey: normalized,
        label: label.to_string(),
    });
    true
}

/// Load, append, and persist — a no-op write when the entry already exists.
pub(crate) fn seed_provider_trust(app: &AppHandle, provider_pubkey: &str) -> Result<(), String> {
    let mut config = load_global_agent_config(app)?;
    if !append_allowed_bridge_pubkey(&mut config, provider_pubkey, LOCAL_PROVIDER_LABEL) {
        return Ok(());
    }
    save_global_agent_config(app, &config)
}
