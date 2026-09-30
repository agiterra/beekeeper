//! Reading what the app wrote: the config, the record and the key.
//!
//! Commissioning itself happens in Beekeeper — minting an identity and getting
//! it attested by the owner key is the app's job, and this host deliberately
//! cannot do it. What lives here is the other half: reading the result, and
//! refusing it by name when it does not add up.
//!
//! Every refusal below is a *disclosed* one. A client asking `status` must be
//! able to tell "no provider is commissioned" from "your key file is
//! unreadable" from "that key belongs to a different identity", because they
//! need different words and different actions from a person.

use std::path::Path;

use buzz_session_host_core::config::HostConfig;
use buzz_session_host_core::layout::{self, Instance};
use buzz_session_host_core::record::{load_provider_store_from, CodingSessionProviderRecord};

use crate::identity::{self, ResolvedKey};

/// Everything resolved and checked, before anything is spawned.
///
/// `Debug` is safe to derive because [`ResolvedKey`]'s own is written out to
/// redact the nsec — see there for why that is not left to a derive.
#[derive(Debug)]
pub struct Commissioned {
    pub config: HostConfig,
    pub record: CodingSessionProviderRecord,
    pub key: ResolvedKey,
}

impl Commissioned {
    /// The provider log this commissioning writes into.
    ///
    /// Derived from the config's base directory rather than passed in, so the
    /// host and the app cannot disagree about which file holds the child's
    /// output — that file is what gets pasted into a bug report.
    pub fn log_path(&self) -> std::path::PathBuf {
        self.config
            .session_provider_base_dir
            .join("logs")
            .join(format!("{}.log", self.config.provider_pubkey))
    }
}

/// Read the config and the identity, or say exactly what is missing.
pub fn commission(home: &Path, instance: Instance) -> Result<Commissioned, String> {
    let config_path = layout::host_config_path(home, instance);
    let config = HostConfig::load(&config_path)?.ok_or_else(|| {
        format!(
            "this host has not been commissioned: {} does not exist. Open Beekeeper and finish \
             setting up coding sessions, or write the file yourself for a headless install.",
            config_path.display()
        )
    })?;

    let store = load_provider_store_from(&config.record_store_path())?;
    let record = store.get(&config.relay_url).cloned().ok_or_else(|| {
        format!(
            "{} names {} but {} holds no record for that relay",
            config_path.display(),
            config.relay_url,
            config.record_store_path().display()
        )
    })?;
    if record.provider_pubkey != config.provider_pubkey {
        return Err(format!(
            "the record for {} is for a different provider than host.json names — re-commission \
             this host from Beekeeper",
            config.relay_url
        ));
    }

    let key = identity::resolve_key(home, instance, &record).map_err(|error| error.message())?;
    identity::matches_record(&key, &record).map_err(|error| error.message())?;
    Ok(Commissioned {
        config,
        record,
        key,
    })
}

/// Write a complete commissioning under `home`, for tests.
///
/// Shared with `control`'s tests rather than duplicated: two fixtures writing
/// the same four files is how they come to disagree about the shape the real
/// code reads.
#[cfg(test)]
pub(crate) fn write_test_commissioning(
    home: &Path,
    relay: &str,
    provider_command: Option<&Path>,
) -> nostr::Keys {
    use nostr::ToBech32;
    let keys = nostr::Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let base = home.join("app-data/session-provider");
    std::fs::create_dir_all(base.join(&pubkey)).expect("mkdir");
    std::fs::create_dir_all(base.join("logs")).expect("mkdir");
    std::fs::write(
        base.join(buzz_session_host_core::record::STORE_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "providers": {
                buzz_session_host_core::record::canonical_relay_key(relay): {
                    "providerPubkey": pubkey,
                    "instanceId": &pubkey[..16],
                    "createdAt": "2026-09-30T00:00:00Z",
                    "relayUrl": relay,
                }
            }
        })
        .to_string(),
    )
    .expect("record");

    let host_dir = layout::host_dir(home, Instance::Production);
    buzz_session_host_core::atomic_write::create_dir_all_restricted(&host_dir).expect("mkdir");
    let mut config = serde_json::json!({
        "version": 1,
        "instance": "production",
        "relayUrl": relay,
        "providerPubkey": pubkey,
        "sessionProviderBaseDir": base,
        "providerStateDir": base.join(&pubkey),
        "runtimes": [],
        "writtenAt": "2026-09-30T00:00:00Z",
    });
    if let Some(command) = provider_command {
        config["providerCommand"] = serde_json::json!(command);
    }
    std::fs::write(
        layout::host_config_path(home, Instance::Production),
        config.to_string(),
    )
    .expect("host.json");
    std::fs::write(
        layout::host_key_file_path(home, Instance::Production),
        keys.secret_key().to_bech32().expect("nsec"),
    )
    .expect("key");
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_commissioning(home: &Path, relay: &str) -> nostr::Keys {
        write_test_commissioning(home, relay, None)
    }

    #[test]
    fn a_full_commissioning_reads_back_and_names_its_own_log() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys = write_commissioning(dir.path(), "wss://hive.example.org");
        let commissioned =
            commission(dir.path(), Instance::Production).expect("a full commissioning");
        assert_eq!(
            commissioned.config.provider_pubkey,
            keys.public_key().to_hex()
        );
        assert_eq!(commissioned.key.source, identity::KeySource::File);
        assert!(commissioned
            .log_path()
            .ends_with(format!("logs/{}.log", keys.public_key().to_hex())));
    }

    /// A config naming a relay the record store knows nothing about must say
    /// so, naming both files. This is what an operator sees after hand-editing
    /// one of the two.
    #[test]
    fn a_config_and_a_record_that_disagree_name_both_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_commissioning(dir.path(), "wss://hive.example.org");
        let config_path = layout::host_config_path(dir.path(), Instance::Production);
        let mut config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).expect("read"))
                .expect("parse");
        config["relayUrl"] = serde_json::json!("wss://elsewhere.example.org");
        std::fs::write(&config_path, config.to_string()).expect("write");

        let error = commission(dir.path(), Instance::Production).expect_err("must be refused");
        assert!(error.contains("wss://elsewhere.example.org"), "{error}");
        assert!(error.contains("holds no record"), "{error}");
    }

    #[test]
    fn an_uncommissioned_host_names_the_file_it_wanted_and_what_to_do() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = commission(dir.path(), Instance::Production).expect_err("must be refused");
        assert!(error.contains("host.json"), "{error}");
        assert!(error.contains("Open Beekeeper"), "{error}");
    }
}
