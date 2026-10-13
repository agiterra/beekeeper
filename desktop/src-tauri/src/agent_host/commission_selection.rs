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

pub(super) fn select(
    existing: Option<HostConfig>,
    mut target: HostConfig,
    record: &CodingSessionProviderRecord,
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
    Err("commissioned host scope conflicts; viewing another community cannot transfer execution; use explicit operator commissioning".into())
}

/// Check requested scope before any key mint, file write or control request.
pub(super) fn check_scope(
    config: Option<&HostConfig>,
    served: Option<&beekeeper_host::protocol::Status>,
    instance: layout::Instance,
    relay: &str,
    provider: Option<&str>,
) -> Result<(), String> {
    if let Some(config) = config {
        validate(config)?;
        if config.instance != instance
            || canonical_relay_key(&config.relay_url) != canonical_relay_key(relay)
            || provider.is_some_and(|provider| provider != config.provider_pubkey)
        {
            return Err("this executor is commissioned for another scope; changing the viewing community does not transfer execution; no identity, files or host control were changed".into());
        }
    }
    if let Some(served) = served {
        let config = config.ok_or("running executor has no matching disk commission; repair explicitly without replacing its ownership")?;
        if served.protocol_version != beekeeper_host::protocol::PROTOCOL_VERSION
            || canonical_relay_key(&served.relay_url) != canonical_relay_key(&config.relay_url)
            || served.provider_pubkey != config.provider_pubkey
            || served.provider_state_dir != config.provider_state_dir
        {
            return Err("running executor serves another or unconfirmed scope; no implicit Bind or Start was requested".into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "commission_selection_tests.rs"]
mod tests;
