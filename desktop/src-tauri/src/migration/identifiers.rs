//! Tauri app-data directory identifiers, and the mapping from a current one
//! back to the name the previous product rename shipped under.
//!
//! The Tauri identifier controls the app-data path, so a product rename looks
//! like a fresh install unless something copies the old directory forward —
//! see [`super::migrate_legacy_app_data_dir`], which this module feeds.

use std::path::{Path, PathBuf};

pub(crate) const CANONICAL_DEV_IDENTIFIER: &str = "io.agiterra.beekeeper.dev";
pub(crate) const CANONICAL_RELEASE_IDENTIFIER: &str = "io.agiterra.beekeeper";
/// One rename back. The Sprout-era pair these replaced is not chained: a
/// two-hop migration needs the intermediate directory, which no longer exists.
const LEGACY_CANONICAL_DEV_IDENTIFIER: &str = "xyz.block.buzz.app.dev";
const LEGACY_RELEASE_IDENTIFIER: &str = "xyz.block.buzz.app";

/// Returns `true` when `name` is a dev data dir name — i.e. it is exactly the
/// canonical dev identifier or a worktree variant separated by a `.` (e.g.
/// `io.agiterra.beekeeper.dev.my-branch`). Rejects prefix-collisions such as
/// `io.agiterra.beekeeper.developer`. This is the authoritative dev/prod
/// discriminator shared by `run_boot_migrations`, `sync_shared_agent_data`,
/// and `reconcile_target_dir`.
pub(crate) fn is_dev_data_dir_name(name: &str) -> bool {
    name == CANONICAL_DEV_IDENTIFIER
        || name
            .strip_prefix(CANONICAL_DEV_IDENTIFIER)
            .is_some_and(|rest| rest.starts_with('.'))
}

pub(crate) fn canonical_dev_data_dir(current: &Path) -> Option<PathBuf> {
    current.parent().map(|p| p.join(CANONICAL_DEV_IDENTIFIER))
}

/// The sibling directory `current` would have been called before the rename,
/// preserving any worktree suffix. `None` when `current` is not one of ours.
pub(crate) fn legacy_app_data_dir(current: &Path) -> Option<PathBuf> {
    let name = current.file_name()?.to_str()?;
    // Dev first: the dev identifier starts with the release identifier, so the
    // opposite order would rewrite every dev dir with the release mapping.
    let legacy_name = if name.starts_with(CANONICAL_DEV_IDENTIFIER) {
        name.replacen(CANONICAL_DEV_IDENTIFIER, LEGACY_CANONICAL_DEV_IDENTIFIER, 1)
    } else if name.starts_with(CANONICAL_RELEASE_IDENTIFIER) {
        name.replacen(CANONICAL_RELEASE_IDENTIFIER, LEGACY_RELEASE_IDENTIFIER, 1)
    } else {
        return None;
    };
    current.parent().map(|parent| parent.join(legacy_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_discriminator_rejects_prefix_collisions() {
        assert!(is_dev_data_dir_name("io.agiterra.beekeeper.dev"));
        assert!(is_dev_data_dir_name("io.agiterra.beekeeper.dev.my-branch"));
        assert!(!is_dev_data_dir_name("io.agiterra.beekeeper"));
        assert!(!is_dev_data_dir_name("io.agiterra.beekeeper.developer"));
    }

    #[test]
    fn canonical_dev_data_dir_replaces_only_the_last_component() {
        assert_eq!(
            canonical_dev_data_dir(Path::new("/s/io.agiterra.beekeeper.dev.my-branch")),
            Some(PathBuf::from("/s/io.agiterra.beekeeper.dev"))
        );
        // Already canonical: returns the same path. `sync_shared_agent_data`
        // relies on that equality to decide there is nothing to sync.
        let canonical = Path::new("/s/io.agiterra.beekeeper.dev");
        assert_eq!(
            canonical_dev_data_dir(canonical),
            Some(canonical.to_path_buf())
        );
        // A root path has no parent.
        assert_eq!(canonical_dev_data_dir(Path::new("/")), None);
    }

    #[test]
    fn a_dev_dir_maps_to_the_dev_legacy_name_not_the_release_one() {
        assert_eq!(
            legacy_app_data_dir(Path::new("/d/io.agiterra.beekeeper.dev")),
            Some(PathBuf::from("/d/xyz.block.buzz.app.dev"))
        );
        assert_eq!(
            legacy_app_data_dir(Path::new("/d/io.agiterra.beekeeper.dev.wt")),
            Some(PathBuf::from("/d/xyz.block.buzz.app.dev.wt"))
        );
        assert_eq!(
            legacy_app_data_dir(Path::new("/d/io.agiterra.beekeeper")),
            Some(PathBuf::from("/d/xyz.block.buzz.app"))
        );
        assert_eq!(legacy_app_data_dir(Path::new("/d/com.other.app")), None);
    }
}
