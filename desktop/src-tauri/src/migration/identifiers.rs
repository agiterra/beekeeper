//! Tauri app-data directory identifiers.
//!
//! There is deliberately no mapping back to a predecessor. Beekeeper is a
//! fork of Buzz, not a rename of it: both apps stay installed and keep
//! running, so copying `xyz.block.buzz.app` forward would fork a live app's
//! state — see the note in [`super::run_boot_migrations`].

use std::path::{Path, PathBuf};

pub(crate) const CANONICAL_DEV_IDENTIFIER: &str = "io.agiterra.beekeeper.app.dev";
/// Returns `true` when `name` is a dev data dir name — i.e. it is exactly the
/// canonical dev identifier or a worktree variant separated by a `.` (e.g.
/// `io.agiterra.beekeeper.app.dev.my-branch`). Rejects prefix-collisions such as
/// `io.agiterra.beekeeper.app.developer`. This is the authoritative dev/prod
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_discriminator_rejects_prefix_collisions() {
        assert!(is_dev_data_dir_name("io.agiterra.beekeeper.app.dev"));
        assert!(is_dev_data_dir_name(
            "io.agiterra.beekeeper.app.dev.my-branch"
        ));
        assert!(!is_dev_data_dir_name("io.agiterra.beekeeper.app"));
        assert!(!is_dev_data_dir_name("io.agiterra.beekeeper.app.developer"));
    }

    #[test]
    fn no_identifier_here_belongs_to_another_installed_app() {
        // Beekeeper forked from Buzz rather than replacing it: both stay
        // installed and both keep running. Anything that mapped one of our
        // identifiers onto `xyz.block.buzz.app` would make a migration fork a
        // live app's state, and would make a reset wipe it.
        let id = CANONICAL_DEV_IDENTIFIER;
        assert!(id.starts_with("io.agiterra.beekeeper"), "{id} is not ours");
        assert!(!id.contains("block.buzz"), "{id} names another app");
    }

    #[test]
    fn canonical_dev_data_dir_replaces_only_the_last_component() {
        assert_eq!(
            canonical_dev_data_dir(Path::new("/s/io.agiterra.beekeeper.app.dev.my-branch")),
            Some(PathBuf::from("/s/io.agiterra.beekeeper.app.dev"))
        );
        // Already canonical: returns the same path. `sync_shared_agent_data`
        // relies on that equality to decide there is nothing to sync.
        let canonical = Path::new("/s/io.agiterra.beekeeper.app.dev");
        assert_eq!(
            canonical_dev_data_dir(canonical),
            Some(canonical.to_path_buf())
        );
        // A root path has no parent.
        assert_eq!(canonical_dev_data_dir(Path::new("/")), None);
    }
}
