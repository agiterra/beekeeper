//! Shared identity minting and host commissioning, independent of a desktop.
//!
//! Callers resolve their own custody and paths. The owner's signing key is
//! used only for attestation and is never persisted here. Writing configuration
//! is preparation, not evidence that a host or runtime is ready.

use std::path::Path;

use nostr::ToBech32;

use crate::atomic_write::{atomic_write_json_restricted, create_dir_all_restricted};
use crate::config::HostConfig;
use crate::layout;
use crate::record::{canonical_relay_key, CodingSessionProviderRecord};

/// Pubkey prefix used by the provider when deriving its default instance ID.
pub const INSTANCE_ID_PUBKEY_PREFIX_LEN: usize = 16;

/// An owner-attested managed identity, before a caller places it in custody.
///
/// Deliberately not serializable or printable: it contains signing material.
pub struct MintedAgentIdentity {
    /// Scoped identity's signing keys, never the owner's keys.
    pub keys: nostr::Keys,
    /// Public identity, used for routing and persisted records.
    pub pubkey: String,
    /// Secret to hand to the caller's custody adapter.
    pub private_key_nsec: String,
    /// NIP-OA attestation under the existing empty-conditions contract.
    pub auth_tag: String,
}

/// Mint a scoped identity attested by the supplied owner, without writing files.
pub fn mint_agent_identity(owner_keys: &nostr::Keys) -> Result<MintedAgentIdentity, String> {
    let keys = nostr::Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let private_key_nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|error| format!("failed to encode private key: {error}"))?;
    let auth_tag = beekeeper_sdk::nip_oa::compute_auth_tag(owner_keys, &keys.public_key(), "")
        .map_err(|error| format!("failed to compute NIP-OA auth tag: {error}"))?;
    Ok(MintedAgentIdentity {
        keys,
        pubkey,
        private_key_nsec,
        auth_tag,
    })
}

/// Mint one provider record for a valid relay using the shared attestation path.
///
/// Enrollment callers must look up an existing record first; retrying enrollment
/// must reuse that record rather than call this minting primitive again.
pub fn mint_provider_record(
    owner_keys: &nostr::Keys,
    relay_url: &str,
) -> Result<CodingSessionProviderRecord, String> {
    let relay_url = relay_url.trim();
    match url::Url::parse(relay_url) {
        Ok(url) if matches!(url.scheme(), "ws" | "wss") && url.host_str().is_some() => {}
        _ => return Err("provider relay must be a ws/wss URL with a host".to_string()),
    }
    let minted = mint_agent_identity(owner_keys)?;
    let instance_id = minted
        .pubkey
        .get(..INSTANCE_ID_PUBKEY_PREFIX_LEN)
        .ok_or_else(|| "minted provider pubkey is too short".to_string())?
        .to_string();
    Ok(CodingSessionProviderRecord {
        provider_pubkey: minted.pubkey,
        instance_id,
        auth_tag: Some(minted.auth_tag),
        created_at: chrono::Utc::now().to_rfc3339(),
        relay_url: relay_url.to_string(),
        private_key_nsec: minted.private_key_nsec,
    })
}

/// Write a validated provider key and host configuration in the host's layout.
///
/// Validates everything before writing; repeating with the same identity is
/// safe. The scoped key is written atomically before publishing the config.
/// These are two atomic writes, not a transaction or ownership handoff: callers
/// must quiesce an old owner and obtain adoption acknowledgement separately.
/// Existing desktop custody is retained as a recovery source.
pub fn write_commissioning(
    home: &Path,
    config: &HostConfig,
    record: &CodingSessionProviderRecord,
) -> Result<(), String> {
    config.validate()?;
    if record.private_key_nsec.trim().is_empty() {
        return Err(format!(
            "the coding-session provider {} has no private key available — the OS keyring may be \
             unreachable. Refusing to commission the agent host without an identity, because a \
             replacement key would strand every event the real one signed.",
            record.provider_pubkey
        ));
    }
    let keys = nostr::Keys::parse(record.private_key_nsec.trim())
        .map_err(|_| "provider key is invalid".to_string())?;
    if keys.public_key().to_hex() != record.provider_pubkey
        || config.provider_pubkey != record.provider_pubkey
    {
        return Err("provider key, record and host config identities do not match".to_string());
    }
    if canonical_relay_key(&config.relay_url) != canonical_relay_key(&record.relay_url) {
        return Err("provider record and host config relay scopes do not match".to_string());
    }
    if let Some(auth_tag) = &record.auth_tag {
        beekeeper_sdk::nip_oa::verify_auth_tag(auth_tag, &keys.public_key())
            .map_err(|_| "provider owner attestation is invalid".to_string())?;
    }
    let payload = serde_json::to_vec_pretty(config)
        .map_err(|error| format!("failed to encode host config: {error}"))?;
    let key_path = layout::host_key_file_path(home, config.instance);
    let config_path = layout::host_config_path(home, config.instance);
    // The atomic helper follows existing symlinks for other callers. Custody
    // targets must instead stay in the selected host layout.
    for path in [&key_path, &config_path] {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(format!(
                    "commissioning target is not a regular file: {}",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("inspect {}: {error}", path.display())),
        }
    }
    create_dir_all_restricted(&layout::host_dir(home, config.instance))?;
    atomic_write_json_restricted(&key_path, record.private_key_nsec.trim().as_bytes())?;
    atomic_write_json_restricted(&config_path, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HOST_CONFIG_VERSION;

    fn enrollment(home: &Path) -> (CodingSessionProviderRecord, HostConfig, nostr::Keys) {
        let owner = nostr::Keys::generate();
        let record = mint_provider_record(&owner, "wss://relay.example.test").expect("mint");
        let config = HostConfig {
            version: HOST_CONFIG_VERSION,
            instance: layout::Instance::Production,
            relay_url: record.relay_url.clone(),
            provider_pubkey: record.provider_pubkey.clone(),
            session_provider_base_dir: home.join("service/session-provider"),
            provider_state_dir: home
                .join("service/session-provider")
                .join(&record.provider_pubkey),
            runtimes: Vec::new(),
            max_sessions: None,
            turn_idle_timeout_secs: None,
            turn_budget: None,
            provider_command: None,
            written_at: record.created_at.clone(),
        };
        (record, config, owner)
    }

    #[test]
    fn commissioning_reuses_identity_and_never_persists_owner_secret() {
        let dir = tempfile::tempdir().expect("home");
        let (record, config, owner) = enrollment(dir.path());
        let scoped = nostr::PublicKey::from_hex(&record.provider_pubkey).expect("pubkey");
        assert_eq!(
            beekeeper_sdk::nip_oa::verify_auth_tag(
                record.auth_tag.as_deref().expect("attestation"),
                &scoped
            )
            .expect("valid attestation"),
            owner.public_key()
        );
        write_commissioning(dir.path(), &config, &record).expect("commission");
        write_commissioning(dir.path(), &config, &record).expect("retry");
        let key = std::fs::read_to_string(layout::host_key_file_path(dir.path(), config.instance))
            .expect("key");
        assert_eq!(key, record.private_key_nsec);
        assert_ne!(key, owner.secret_key().to_bech32().expect("owner nsec"));
        let saved = HostConfig::load(&layout::host_config_path(dir.path(), config.instance))
            .expect("config")
            .expect("present");
        assert_eq!(saved, config);
        assert!(!serde_json::to_string(&saved).expect("json").contains(&key));
    }

    #[test]
    fn invalid_commissioning_is_refused_before_any_write() {
        let dir = tempfile::tempdir().expect("home");
        let (record, config, _) = enrollment(dir.path());
        let mut no_key = record.clone();
        no_key.private_key_nsec.clear();
        assert!(write_commissioning(dir.path(), &config, &no_key)
            .expect_err("no key")
            .contains("would strand every event"));
        let mut wrong_key = record.clone();
        wrong_key.private_key_nsec = nostr::Keys::generate()
            .secret_key()
            .to_bech32()
            .expect("nsec");
        assert!(write_commissioning(dir.path(), &config, &wrong_key)
            .expect_err("wrong key")
            .contains("identities do not match"));
        let mut wrong_config = config.clone();
        wrong_config.provider_pubkey = "a".repeat(64);
        assert!(write_commissioning(dir.path(), &wrong_config, &record).is_err());
        wrong_config = config.clone();
        wrong_config.relay_url = "wss://other.example.test".to_string();
        assert!(write_commissioning(dir.path(), &wrong_config, &record).is_err());
        assert!(!layout::host_dir(dir.path(), config.instance).exists());
    }

    #[test]
    fn invalid_retry_preserves_previously_commissioned_files() {
        let dir = tempfile::tempdir().expect("home");
        let (record, config, _) = enrollment(dir.path());
        write_commissioning(dir.path(), &config, &record).expect("commission");
        let key_path = layout::host_key_file_path(dir.path(), config.instance);
        let config_path = layout::host_config_path(dir.path(), config.instance);
        let previous_key = std::fs::read(&key_path).expect("key");
        let previous_config = std::fs::read(&config_path).expect("config");
        let mut invalid = record.clone();
        invalid.auth_tag = Some("invalid attestation".to_string());
        assert!(write_commissioning(dir.path(), &config, &invalid).is_err());
        invalid.private_key_nsec.clear();
        assert!(write_commissioning(dir.path(), &config, &invalid).is_err());
        assert_eq!(std::fs::read(key_path).expect("key"), previous_key);
        assert_eq!(std::fs::read(config_path).expect("config"), previous_config);
    }

    #[cfg(unix)]
    #[test]
    fn custody_files_are_restricted_on_first_write_and_retry() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("home");
        let (record, config, _) = enrollment(dir.path());
        for _ in 0..2 {
            write_commissioning(dir.path(), &config, &record).expect("write");
            for path in [
                layout::host_key_file_path(dir.path(), config.instance),
                layout::host_config_path(dir.path(), config.instance),
            ] {
                assert_eq!(
                    std::fs::metadata(path)
                        .expect("metadata")
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
            assert_eq!(
                std::fs::metadata(layout::host_dir(dir.path(), config.instance))
                    .expect("directory")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn commissioning_does_not_follow_a_custody_symlink() {
        let dir = tempfile::tempdir().expect("home");
        let (record, config, _) = enrollment(dir.path());
        create_dir_all_restricted(&layout::host_dir(dir.path(), config.instance)).expect("dir");
        let outside = dir.path().join("other-key");
        std::fs::write(&outside, "existing").expect("existing");
        std::os::unix::fs::symlink(
            &outside,
            layout::host_key_file_path(dir.path(), config.instance),
        )
        .expect("symlink");
        assert!(write_commissioning(dir.path(), &config, &record).is_err());
        assert_eq!(std::fs::read_to_string(outside).expect("read"), "existing");
    }
}
