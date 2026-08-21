/// Service name for the desktop OS keyring. Debug builds default to a distinct
/// service, while standalone worktree launches may request a scoped dev service.
fn dev_keyring_service(configured: Option<String>) -> String {
    configured
        .filter(|service| service.starts_with("beekeeper-desktop-dev."))
        .unwrap_or_else(|| "beekeeper-desktop-dev".to_string())
}

pub(crate) fn keyring_service() -> &'static str {
    if cfg!(debug_assertions) {
        static DEV_SERVICE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        DEV_SERVICE
            .get_or_init(|| dev_keyring_service(std::env::var("BUZZ_DEV_KEYRING_SERVICE").ok()))
            .as_str()
    } else {
        "beekeeper-desktop"
    }
}

pub(super) fn migration_marker_name(service: &str, default_name: &str) -> String {
    if service == "beekeeper-desktop" || service == "beekeeper-desktop-dev" {
        default_name.to_string()
    } else {
        format!("identity.{service}.migrated")
    }
}

/// The Buzz-era keyring service that `service` was called before the Bee Keeper
/// rename, or `None` when `service` is not one of ours.
///
/// The rename split Bee Keeper's keychain entries away from stock Buzz's, which
/// is required now that both apps can be installed side by side. But the
/// identifier is the *only* thing standing between an existing install and its
/// nsec: unlike the app-data directory, `SecretStore` has no legacy-service
/// fallback, so renaming blind would look like a fresh install with no key.
/// [`migrate_legacy_keyring_service`] closes that.
fn legacy_keyring_service(service: &str) -> Option<String> {
    service
        .strip_prefix("beekeeper-desktop")
        .map(|rest| format!("buzz-desktop{rest}"))
}

/// Copy the whole secret blob from the Buzz-era keyring service into the
/// current one, once.
///
/// Runs at boot before identity resolution. Idempotent and self-marking: a
/// non-empty blob under the current service means the copy already happened (or
/// the user has keys of their own), so the legacy service is never touched
/// again — which matters, because on macOS every distinct service read is a
/// potential keychain prompt.
///
/// Non-fatal in every branch. A keychain that is locked or denied leaves the
/// legacy entries exactly where they are, so a later boot can still migrate;
/// destroying nothing is the whole point.
pub(crate) fn migrate_legacy_keyring_service() {
    if !cfg!(feature = "system-keyring") {
        return;
    }
    let service = keyring_service();
    let Some(legacy) = legacy_keyring_service(service) else {
        return;
    };

    let current = crate::secret_store::SecretStore::shared(service);
    match current.load_all_readonly() {
        // Already has secrets — nothing to import, and no legacy read.
        Ok(Some(map)) if !map.is_empty() => return,
        Ok(_) => {}
        Err(e) => {
            eprintln!("buzz-desktop: keyring-rename: cannot read {service}: {e}");
            return;
        }
    }

    let legacy_store = crate::secret_store::SecretStore::keyring(legacy.clone());
    let entries = match legacy_store.load_all_readonly() {
        Ok(Some(map)) if !map.is_empty() => map,
        Ok(_) => return,
        Err(e) => {
            eprintln!("buzz-desktop: keyring-rename: cannot read legacy {legacy}: {e}");
            return;
        }
    };

    let count = entries.len();
    match current.store_all(&entries) {
        Ok(()) => eprintln!(
            "buzz-desktop: keyring-rename: copied {count} secret(s) from {legacy} to {service}"
        ),
        Err(e) => eprintln!("buzz-desktop: keyring-rename: cannot write {service}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{dev_keyring_service, legacy_keyring_service, migration_marker_name};

    #[test]
    fn standalone_scope_must_remain_under_dev_service() {
        assert_eq!(
            dev_keyring_service(Some("beekeeper-desktop-dev.example".to_string())),
            "beekeeper-desktop-dev.example"
        );
        assert_eq!(
            dev_keyring_service(Some("beekeeper-desktop".to_string())),
            "beekeeper-desktop-dev"
        );
    }

    #[test]
    fn standalone_scope_uses_its_own_migration_marker() {
        assert_eq!(
            migration_marker_name("beekeeper-desktop", "identity.migrated"),
            "identity.migrated"
        );
        assert_eq!(
            migration_marker_name("beekeeper-desktop-dev", "identity.migrated"),
            "identity.migrated"
        );
        assert_eq!(
            migration_marker_name("beekeeper-desktop-dev.example", "identity.migrated"),
            "identity.beekeeper-desktop-dev.example.migrated"
        );
    }

    #[test]
    fn every_service_shape_maps_back_to_its_buzz_era_name() {
        assert_eq!(
            legacy_keyring_service("beekeeper-desktop").as_deref(),
            Some("buzz-desktop")
        );
        assert_eq!(
            legacy_keyring_service("beekeeper-desktop-dev").as_deref(),
            Some("buzz-desktop-dev")
        );
        // Worktree-scoped dev services keep their suffix, so a standalone
        // instance migrates from the service it actually used before.
        assert_eq!(
            legacy_keyring_service("beekeeper-desktop-dev.example").as_deref(),
            Some("buzz-desktop-dev.example")
        );
        // Anything not ours is left alone rather than guessed at.
        assert_eq!(legacy_keyring_service("some-other-app"), None);
    }
}
