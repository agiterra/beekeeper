//! Desktop bootstrap observes commissioned operator settings instead of rediscovering them.
use super::*;
use beekeeper_core_pkg::coding_session_runtime::{parse_runtime_descriptors, RuntimeDescriptor};
use beekeeper_host_core::record::canonical_relay_key;
use std::path::Path;

pub(super) fn load_existing(path: &Path) -> Result<Option<HostConfig>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() || metadata.len() > 4 * 1024 * 1024 {
                return Err(
                    "commissioned host config must be a bounded regular file; repair explicitly"
                        .into(),
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err(
                        "commissioned host config permissions are too broad; repair explicitly"
                            .into(),
                    );
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "commissioned host config is unreadable; refusing discovery fallback".into(),
            )
        }
    }
    HostConfig::load(path)
}

fn validate(config: &HostConfig) -> Result<(), String> {
    config.validate()?;
    let runtimes = serde_json::to_string(&config.runtimes)
        .map_err(|_| "commissioned runtime table cannot be validated")?;
    parse_runtime_descriptors(&runtimes)?;
    Ok(())
}
fn owner(record: &CodingSessionProviderRecord) -> Result<nostr::PublicKey, String> {
    let actor = nostr::PublicKey::from_hex(&record.provider_pubkey)
        .map_err(|_| "provider identity invalid")?;
    let tag = record
        .auth_tag
        .as_deref()
        .ok_or("explicit rebind requires provider owner attestation")?;
    beekeeper_sdk_pkg::nip_oa::verify_auth_tag(tag, &actor)
        .map_err(|_| "provider owner attestation invalid".into())
}

pub(super) fn select(
    existing: Option<HostConfig>,
    mut target: HostConfig,
    record: &CodingSessionProviderRecord,
    store: &CodingSessionProviderStore,
    reason: Commissioning,
    discover: impl FnOnce() -> Vec<RuntimeDescriptor>,
) -> Result<HostConfig, String> {
    target.validate()?;
    if canonical_relay_key(&record.relay_url) != canonical_relay_key(&target.relay_url)
        || record.provider_pubkey != target.provider_pubkey
    {
        return Err("selected provider record and community do not match".into());
    }
    let Some(existing) = existing else {
        target.runtimes = discover();
        validate(&target)?;
        return Ok(target);
    };
    validate(&existing)?;
    if existing.instance != target.instance {
        return Err(
            "commissioned host belongs to another instance; no implicit replacement".into(),
        );
    }
    if existing.provider_pubkey == target.provider_pubkey
        && canonical_relay_key(&existing.relay_url) == canonical_relay_key(&target.relay_url)
    {
        // Keep paths, limits, commands, models and every capability verbatim.
        return Ok(existing);
    }
    if reason != Commissioning::RelayChanged
        || canonical_relay_key(&existing.relay_url) == canonical_relay_key(&target.relay_url)
    {
        return Err(
            "commissioned host scope conflicts; use explicit authorized rebind/commissioning"
                .into(),
        );
    }
    let old = store
        .get(&existing.relay_url)
        .filter(|old| old.provider_pubkey == existing.provider_pubkey)
        .ok_or("foreign commissioned provider custody cannot be rebound by this desktop")?;
    if owner(old)? != owner(record)? {
        return Err(
            "cross-community rebind cannot transfer another owner's provider custody".into(),
        );
    }
    // Explicit same-owner rebind retains this machine's operator runtime offer;
    // only selected new community identity/custody paths are replaced.
    target.runtimes = existing.runtimes;
    target.max_sessions = existing.max_sessions;
    target.turn_idle_timeout_secs = existing.turn_idle_timeout_secs;
    target.turn_budget = existing.turn_budget;
    target.provider_command = existing.provider_command;
    validate(&target)?;
    Ok(target)
}

#[cfg(test)]
#[path = "commission_selection_tests.rs"]
mod tests;
