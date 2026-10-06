//! How the host finds the provider's signing key — and says so when it cannot.
//!
//! **The host is never a keychain client.** That is a rule, not a preference,
//! and the reason is structural. The desktop keeps every secret in one
//! keychain entry through the *legacy* SecKeychain API, deliberately, so that
//! signed release builds and unsigned dev builds share one store
//! (`desktop/src-tauri/src/secret_store.rs`). Legacy keychain items carry a
//! per-item ACL keyed to a code signature, and
//! `docs/local-desktop-instances.md` already records the consequence: "the
//! keychain ACL is bound to the binary's signature, which changes per build."
//! A daemon reading that entry would mean a GUI prompt at login, a human at
//! the keyboard, and a grant that dissolves on the next rebuild. A server has
//! no keychain at all.
//!
//! So the desktop hands the key over at commissioning and the host resolves it
//! from its own routes, in this order:
//!
//! 1. `BEEKEEPER_HOST_PRIVATE_KEY` — the server and container path.
//! 2. `BEEKEEPER_HOST_KEY_FILE`, or `~/.local/state/buzz[-dev]/host/provider-key`
//!    — a `0600` file, the only fallback that works headless.
//! 3. The record's own inline nsec, when the desktop left one there (a build
//!    with no keyring backend, or a keyring outage).
//!
//! # What this costs, stated plainly
//!
//! On macOS the provider nsec's at-rest protection drops from an ACL-gated
//! keychain item to a `0600` file under `$HOME`. Three facts bound that:
//!
//! - it is the same protection `nokeyring` dev builds already use for this
//!   key and for managed-agent keys (`docs/local-desktop-instances.md`);
//! - it is the same protection the provider's **state directory** already
//!   has — a `0700` directory holding a durable outbox of *pre-signed events*
//!   — so compromising that directory already allows publishing as the
//!   provider;
//! - `BEEKEEPER_HOST_KEY_FILE` lets a hardened deployment point at a secrets mount
//!   or a tmpfs file with no code change.

use std::path::{Path, PathBuf};

use beekeeper_host_core::layout::{Instance, KEY_FILE_VAR, PRIVATE_KEY_VAR};
use beekeeper_host_core::record::CodingSessionProviderRecord;
use nostr::ToBech32;

/// Where a resolved key came from, for the log line and for `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    /// `BEEKEEPER_HOST_PRIVATE_KEY`.
    Environment,
    /// A `0600` file — either `BEEKEEPER_HOST_KEY_FILE` or the default path.
    File,
    /// The record's own inline nsec.
    Record,
}

/// Why the host has no key, named so the menu bar can say which.
///
/// A single "not running" for every cause is the dishonest-status bug class:
/// "no identity yet" and "your key file is unreadable" need different words
/// and different actions, and neither of them is "no agents are running".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "reason",
    content = "detail"
)]
pub enum KeyUnresolved {
    /// Nothing was found on any of the three routes.
    NotFound {
        /// Each route, in order, and what it said. Shown verbatim: an
        /// operator needs the path the host actually looked at, not a
        /// paraphrase of where keys usually live.
        tried: Vec<String>,
    },
    /// A route held something that is not a usable secret key.
    Malformed {
        /// Which route.
        source: KeySource,
        /// What was wrong. Never the value itself.
        problem: String,
    },
}

impl KeyUnresolved {
    /// One line for the log and for `status.message`.
    pub fn message(&self) -> String {
        match self {
            Self::NotFound { tried } => format!(
                "no provider key: {}. Refusing to start the provider — a new key would strand \
                 every event the real one signed.",
                tried.join("; ")
            ),
            Self::Malformed { source, problem } => format!(
                "the provider key from {source:?} is not usable: {problem}. Refusing to start \
                 the provider rather than minting a replacement."
            ),
        }
    }
}

/// A resolved provider secret, and where it came from.
pub struct ResolvedKey {
    /// The nsec, bech32-encoded as the provider's `BUZZ_PRIVATE_KEY` expects.
    pub nsec: String,
    /// The pubkey it derives to, for checking it against the record.
    pub public_key_hex: String,
    /// Which route answered.
    pub source: KeySource,
}

/// `Debug` is written out rather than derived, because a derived one would put
/// the nsec into the log the first time anything formatted this value — and
/// `{:?}` on an error path is exactly where that happens without anyone
/// intending it.
impl std::fmt::Debug for ResolvedKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedKey")
            .field("nsec", &"<redacted>")
            .field("public_key_hex", &self.public_key_hex)
            .field("source", &self.source)
            .finish()
    }
}

/// The default `0600` key file, honouring [`KEY_FILE_VAR`].
pub fn key_file_path(home: &Path, instance: Instance) -> PathBuf {
    match std::env::var_os(KEY_FILE_VAR) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => beekeeper_host_core::layout::host_key_file_path(home, instance),
    }
}

/// Parse one candidate secret, exactly as the provider's own config does
/// (`beekeeper_session_provider::config`: `Keys::parse` on the trimmed value).
///
/// Parsing it here as well is not duplication for its own sake: the host must
/// refuse an unusable key *before* spawning, so the failure reads as "your key
/// file is wrong" in `status` rather than as a child that dies on startup five
/// times and then gives up.
fn parse(candidate: &str, source: KeySource) -> Result<ResolvedKey, KeyUnresolved> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return Err(KeyUnresolved::Malformed {
            source,
            problem: "it is empty".to_string(),
        });
    }
    let keys = nostr::Keys::parse(trimmed).map_err(|error| KeyUnresolved::Malformed {
        source,
        problem: format!("{error}"),
    })?;
    let nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|error| KeyUnresolved::Malformed {
            source,
            problem: format!("it could not be re-encoded as an nsec: {error}"),
        })?;
    Ok(ResolvedKey {
        nsec,
        public_key_hex: keys.public_key().to_hex(),
        source,
    })
}

/// Resolve the provider's key, or say what was tried and what each route said.
///
/// `record` is the provisioned record for the relay being served; its inline
/// nsec is the third route. The record is also what the caller checks the
/// resolved pubkey against — a key that does not match the record would
/// publish under an identity the relay never attested, which is worse than
/// not starting.
pub fn resolve_key(
    home: &Path,
    instance: Instance,
    record: &CodingSessionProviderRecord,
) -> Result<ResolvedKey, KeyUnresolved> {
    let mut tried = Vec::new();

    match std::env::var(PRIVATE_KEY_VAR) {
        Ok(value) if !value.trim().is_empty() => return parse(&value, KeySource::Environment),
        Ok(_) => tried.push(format!("{PRIVATE_KEY_VAR} is set but empty")),
        Err(_) => tried.push(format!("{PRIVATE_KEY_VAR} is not set")),
    }

    let path = key_file_path(home, instance);
    match std::fs::read_to_string(&path) {
        Ok(value) if !value.trim().is_empty() => return parse(&value, KeySource::File),
        Ok(_) => tried.push(format!("{} is empty", path.display())),
        Err(error) => tried.push(format!("{} could not be read ({error})", path.display())),
    }

    if !record.private_key_nsec.trim().is_empty() {
        return parse(&record.private_key_nsec, KeySource::Record);
    }
    tried.push(format!(
        "the record for {} carries no inline key",
        record.provider_pubkey
    ));

    Err(KeyUnresolved::NotFound { tried })
}

/// Whether `resolved` is the identity `record` names.
///
/// Checked rather than assumed: a key file left behind by a previous
/// commissioning would otherwise have the host publish as an identity this
/// relay's owner never attested, and the relay would reject every event with
/// no local explanation.
pub fn matches_record(
    resolved: &ResolvedKey,
    record: &CodingSessionProviderRecord,
) -> Result<(), KeyUnresolved> {
    if resolved.public_key_hex == record.provider_pubkey {
        return Ok(());
    }
    Err(KeyUnresolved::Malformed {
        source: resolved.source,
        problem: format!(
            "it belongs to {} but this host was commissioned for {}",
            &resolved.public_key_hex[..16.min(resolved.public_key_hex.len())],
            &record.provider_pubkey[..16.min(record.provider_pubkey.len())]
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_for(keys: &nostr::Keys, inline: &str) -> CodingSessionProviderRecord {
        CodingSessionProviderRecord {
            provider_pubkey: keys.public_key().to_hex(),
            instance_id: keys.public_key().to_hex()[..16].to_string(),
            auth_tag: None,
            created_at: "2026-09-30T00:00:00Z".to_string(),
            relay_url: "wss://hive.example.org".to_string(),
            private_key_nsec: inline.to_string(),
        }
    }

    /// The env variable and the key file are process-global, so these cases
    /// run inside one test rather than racing each other across threads.
    #[test]
    fn the_routes_are_tried_in_order_and_each_miss_is_named() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path();
        let keys = nostr::Keys::generate();
        let nsec = keys.secret_key().to_bech32().expect("nsec");

        // Nothing anywhere: the error lists all three routes, with the path.
        let empty = record_for(&keys, "");
        let error = resolve_key(home, Instance::Production, &empty).expect_err("no key");
        let KeyUnresolved::NotFound { tried } = &error else {
            panic!("expected NotFound, got {error:?}");
        };
        assert_eq!(tried.len(), 3, "{tried:?}");
        assert!(tried[0].contains(PRIVATE_KEY_VAR), "{tried:?}");
        assert!(
            tried[1].contains("provider-key"),
            "the operator needs the path the host actually looked at: {tried:?}"
        );
        assert!(tried[2].contains(&empty.provider_pubkey), "{tried:?}");

        // Route 3: the record's inline key.
        let inline = record_for(&keys, &nsec);
        let resolved = resolve_key(home, Instance::Production, &inline).expect("inline");
        assert_eq!(resolved.source, KeySource::Record);
        assert_eq!(resolved.public_key_hex, keys.public_key().to_hex());

        // Route 2: the 0600 file beats the record.
        let key_path = beekeeper_host_core::layout::host_key_file_path(home, Instance::Production);
        beekeeper_host_core::atomic_write::create_dir_all_restricted(
            key_path.parent().expect("parent"),
        )
        .expect("host dir");
        let file_keys = nostr::Keys::generate();
        std::fs::write(
            &key_path,
            file_keys.secret_key().to_bech32().expect("nsec") + "\n",
        )
        .expect("write key file");
        let resolved = resolve_key(home, Instance::Production, &inline).expect("file");
        assert_eq!(resolved.source, KeySource::File);
        assert_eq!(resolved.public_key_hex, file_keys.public_key().to_hex());
        // And it is checked against the record, which it does not match.
        assert!(matches_record(&resolved, &inline).is_err());

        // A file holding something that is not a key fails by name, and does
        // not silently fall through to the record.
        std::fs::write(&key_path, "not-a-key").expect("write garbage");
        let error = resolve_key(home, Instance::Production, &inline).expect_err("malformed");
        assert!(
            matches!(&error, KeyUnresolved::Malformed { source, .. } if *source == KeySource::File),
            "{error:?}"
        );
        assert!(
            error.message().contains("Refusing to start"),
            "the message must say what it refuses to do: {}",
            error.message()
        );
    }

    /// A key that does not belong to the commissioned identity must be
    /// refused: publishing under an unattested pubkey means the relay rejects
    /// every event with nothing local to explain it.
    #[test]
    fn a_key_from_another_commissioning_is_refused_by_name() {
        let ours = nostr::Keys::generate();
        let theirs = nostr::Keys::generate();
        let record = record_for(&ours, "");
        let resolved = parse(
            &theirs.secret_key().to_bech32().expect("nsec"),
            KeySource::File,
        )
        .expect("parses");
        let error = matches_record(&resolved, &record).expect_err("must be refused");
        assert!(
            error.message().contains("was commissioned for"),
            "{}",
            error.message()
        );
        assert!(matches_record(
            &parse(
                &ours.secret_key().to_bech32().expect("nsec"),
                KeySource::File
            )
            .expect("parses"),
            &record
        )
        .is_ok());
    }

    /// An empty value on a route is a different fact from an absent one, and
    /// neither may read as a usable key.
    #[test]
    fn an_empty_candidate_is_malformed_rather_than_accepted() {
        let error = parse("   ", KeySource::Environment).expect_err("empty");
        assert!(matches!(
            error,
            KeyUnresolved::Malformed { problem, .. } if problem.contains("empty")
        ));
    }
}
