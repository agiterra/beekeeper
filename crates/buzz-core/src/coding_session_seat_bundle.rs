//! Where a seat's skill bundle lives on one machine.
//!
//! A seat's role skills are materialized outside every checkout, at
//! `<app data dir>/agents/seats/<session id>/`, so a seat run leaves nothing
//! untracked in the tree it is working in. Two parties need that path and they
//! are in different crates: the session provider, which creates the bundle,
//! and the desktop host, which removes it when the seat's worktree goes.
//!
//! Both used to hold their own copy of the directory names and of the
//! session-id sanitizer. That was not a style problem. The host finds a bundle
//! by **recomputing its name**, so a sanitizer that disagreed by one character
//! would compose a path nothing is at, report the bundle absent, and leave it
//! on disk forever — with no error anywhere, because "absent" is an ordinary
//! answer. One definition, in the crate both already depend on, makes that
//! disagreement unrepresentable rather than something a test has to keep
//! catching.
//!
//! This module is path arithmetic only. It reads nothing, creates nothing and
//! removes nothing; each side keeps its own rules about what it may do with
//! the directory once it knows where it is.

use std::path::{Path, PathBuf};

/// The directory under the app's data directory that holds every seat bundle.
///
/// Relative, and joined onto an app data directory by
/// [`seat_bundle_dir_in`] — never used as an absolute path on its own.
pub const SEAT_BUNDLES_DIR: &str = "agents/seats";

/// The skills directory inside one seat's bundle.
///
/// The bundle's other entry is its manifest, whose filename belongs to
/// `buzz-persona` (`skills::SKILL_BUNDLE_MANIFEST_FILE`) because that is the
/// crate that writes it. It is deliberately not restated here: a third copy of
/// a name is the problem this module exists to remove, not a service it should
/// offer.
pub const SEAT_BUNDLE_SKILLS_DIR: &str = "skills";

/// The directory name used when a session id sanitizes away to nothing.
pub const UNNAMED_SESSION_BUNDLE: &str = "unnamed-session";

/// A session id as exactly one ordinary directory name.
///
/// Session ids a provider mints are UUIDs, and the ones it reads back come
/// from its own durable records, so in practice this changes nothing. It
/// exists for the ids that are neither: "the bundle is this seat's own" is a
/// claim about a path, and a path assembled from a string is only as safe as
/// the string.
///
/// Every character that is not ASCII alphanumeric, `-` or `_` becomes `_`, so
/// the result can never contain a separator, never be `..`, and never be
/// absolute. An id that maps to nothing at all gets
/// [`UNNAMED_SESSION_BUNDLE`], because an empty name would silently resolve to
/// the seats root itself — which is every seat's bundle, not this one's.
pub fn bundle_directory_name(session_id: &str) -> String {
    let mapped: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if mapped.is_empty() {
        UNNAMED_SESSION_BUNDLE.to_owned()
    } else {
        mapped
    }
}

/// The root every seat bundle on one machine sits under.
pub fn seat_bundles_root_in(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SEAT_BUNDLES_DIR)
}

/// Where one session's bundle lives, whether or not it exists yet.
///
/// `<app data dir>/agents/seats/<sanitized session id>`. The result is always
/// exactly one component below [`seat_bundles_root_in`], whatever the id was.
///
/// Callers differ in how they reach an app data directory — the provider
/// derives it from its own state directory, the host asks the platform — so
/// that resolution stays with them and only the composition is shared.
pub fn seat_bundle_dir_in(app_data_dir: &Path, session_id: &str) -> PathBuf {
    seat_bundles_root_in(app_data_dir).join(bundle_directory_name(session_id))
}

#[cfg(test)]
#[path = "coding_session_seat_bundle_tests.rs"]
mod tests;
