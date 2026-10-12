//! Explicit scoped host limit changes; bootstrap never derives these from desktop defaults.
use crate::{
    atomic_write,
    config::HostConfig,
    layout::{self, Instance},
    ownership,
    record::{canonical_relay_key, CodingSessionProviderRecord},
};
use nostr::PublicKey;
use std::path::Path;

/// One deliberately edited limit; absent restores the worker default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitPatch {
    /// Concurrent session ceiling; zero retains existing unlimited semantics.
    MaxSessions(Option<usize>),
    /// Per-turn silence budget; zero is invalid.
    TurnIdleTimeout(Option<u64>),
    /// Founder-independent crew turn budget; zero means unlimited.
    TurnBudget(Option<u64>),
}

fn load(
    home: &Path,
    instance: Instance,
    record: &CodingSessionProviderRecord,
    owner: &PublicKey,
) -> Result<Option<HostConfig>, String> {
    let path = layout::host_config_path(home, instance);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() || metadata.len() > 4 * 1024 * 1024 {
                return Err("host limits require a bounded regular commissioned config".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err("host limits config custody is not restricted".into());
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("host limits config unavailable".into()),
    }
    let config =
        HostConfig::load(&path)?.ok_or("commissioned config changed during observation")?;
    let actor =
        PublicKey::from_hex(&record.provider_pubkey).map_err(|_| "provider identity invalid")?;
    let attested = beekeeper_sdk::nip_oa::verify_auth_tag(
        record
            .auth_tag
            .as_deref()
            .ok_or("provider attestation missing")?,
        &actor,
    )
    .map_err(|_| "provider attestation invalid")?;
    if config.instance != instance
        || config.provider_pubkey != record.provider_pubkey
        || canonical_relay_key(&config.relay_url) != canonical_relay_key(&record.relay_url)
        || attested != *owner
    {
        return Err(
            "host limit edit scope or owner conflicts; existing configuration retained".into(),
        );
    }
    Ok(Some(config))
}

/// Read desired limits from the exact commissioned identity/community.
/// Returns None before commissioning; reads no secret and performs no writes.
pub fn scoped_host_config(
    home: &Path,
    instance: Instance,
    record: &CodingSessionProviderRecord,
    owner: &PublicKey,
) -> Result<Option<HostConfig>, String> {
    load(home, instance, record, owner)
}

/// Patch exactly one explicit limit in an existing scoped commissioning.
/// Runtime offers, paths, provider command and custody are retained. The worker
/// observes the change on its next start; no restart or readiness is claimed.
/// None means no commissioning exists yet, so caller may save first-use defaults.
pub fn patch_host_limits(
    home: &Path,
    instance: Instance,
    record: &CodingSessionProviderRecord,
    owner: &PublicKey,
    patch: LimitPatch,
) -> Result<Option<HostConfig>, String> {
    if patch == LimitPatch::TurnIdleTimeout(Some(0)) {
        return Err("turn silence budget cannot be zero".into());
    }
    let _lock = ownership::management_lock(home, instance)?;
    let Some(mut config) = load(home, instance, record, owner)? else {
        return Ok(None);
    };
    match patch {
        LimitPatch::MaxSessions(value) => config.max_sessions = value,
        LimitPatch::TurnIdleTimeout(value) => config.turn_idle_timeout_secs = value,
        LimitPatch::TurnBudget(value) => config.turn_budget = value,
    }
    config.written_at = chrono::Utc::now().to_rfc3339();
    atomic_write::atomic_write_json_restricted(
        &layout::host_config_path(home, instance),
        &serde_json::to_vec_pretty(&config)
            .map_err(|_| "cannot encode scoped host limit change")?,
    )?;
    Ok(Some(config))
}

#[cfg(test)]
#[path = "host_limits_tests.rs"]
mod tests;
