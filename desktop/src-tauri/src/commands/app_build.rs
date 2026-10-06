//! This binary's own build identity, as an app-global fact.
//!
//! The commit and its ordinal are already compiled in by
//! `desktop/src-tauri/build.rs`, but the only way to read them was
//! `team_readiness`, which is async, project-scoped, spawns `git` against a
//! checkout under a timeout, and scans role packs. Comparing this app's build
//! against the relay's needs none of that — three `option_env!` reads and no
//! I/O at all — so it gets its own command rather than a reason to call the
//! heavy one.
//!
//! Deliberately not merged with `team_readiness_git::embedded_source`: that
//! answers "what does the *checkout* look like next to this binary", and
//! carries checkout facts this does not.

use serde::Serialize;

#[cfg(test)]
#[path = "app_build_tests.rs"]
mod tests;

/// What this binary can say about the source it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppBuildIdentity {
    /// Full 40-hex commit, lowercased, or `None` when the build could not
    /// determine one (a tarball build, no git, a stripped environment).
    pub commit: Option<String>,
    /// `git rev-list --count` of [`Self::commit`], or `None`.
    ///
    /// Only ever emitted alongside a commit it describes, and never from a
    /// shallow checkout, where the number would be the graft's size rather
    /// than the commit's position — see `build.rs`.
    pub commit_count: Option<u32>,
    /// `Some(true)` only when the build script *observed* a modified tree.
    ///
    /// Never `Some(false)`: `build.rs` deliberately does not embed a clean
    /// claim, because Cargo cannot cheaply watch every untracked path and a
    /// stale false-clean would survive an incremental rebuild. So `None`
    /// means "not observed dirty", which is not the same as "clean", and a
    /// consumer must not render it as one.
    pub source_dirty: Option<bool>,
}

/// Parse the three compile-time stamps into an identity.
///
/// Pure, so the validation is testable without a Tauri app — the house
/// pattern `parse_embedded_source` already follows.
pub(crate) fn parse_app_build_identity(
    commit: Option<&str>,
    commit_count: Option<&str>,
    dirty: Option<&str>,
) -> AppBuildIdentity {
    let commit = commit
        .map(str::trim)
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase);
    // A count without a commit describes nothing, so it is dropped rather
    // than carried: the pair is the unit of meaning, exactly as it is on the
    // relay side (`resolve_stamp` in `crates/beekeeper-relay/build.rs`).
    let commit_count = commit.as_ref().and_then(|_| {
        commit_count
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse::<u32>().ok())
            // Never `0`: `rev-list --count` of a real commit is at least 1,
            // so a `0` could only come from a broken build, and unlike
            // `None` it would subtract as data on the comparison path.
            .filter(|count| *count >= 1)
    });
    AppBuildIdentity {
        commit,
        commit_count,
        source_dirty: match dirty {
            Some("1") => Some(true),
            _ => None,
        },
    }
}

fn embedded_app_build_identity() -> AppBuildIdentity {
    parse_app_build_identity(
        option_env!("BUZZ_DESKTOP_BUILD_SOURCE_SHA"),
        option_env!("BUZZ_DESKTOP_BUILD_SOURCE_COMMIT_COUNT"),
        option_env!("BUZZ_DESKTOP_BUILD_SOURCE_DIRTY"),
    )
}

/// This app's build identity. Compile-time constants; no I/O.
#[tauri::command]
pub fn get_app_build_identity() -> AppBuildIdentity {
    embedded_app_build_identity()
}
