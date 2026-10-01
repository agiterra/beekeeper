//! Sessions the person has let out of the project boundary.
//!
//! The boundary stops one project's or session's work reaching another's
//! (plans/archive/2026-09-29-isolation-principle.md). Some jobs need what it
//! refuses — setting up a new laptop means Homebrew, Xcode, global toolchains —
//! so the person can grant a session **full access to this computer**: the
//! session is prepared exactly as before (its private runtime configuration,
//! its resolved environment, its branch), and its agent is then started
//! without the boundary around it (ledger 303).
//!
//! The grant is this computer's, so it lives on this computer: a file in the
//! provider's state directory, written by the desktop app, read at every
//! preparation. Nothing on the relay carries it, so no other machine can
//! grant it, and a bounded agent cannot write it — the state directory is not
//! among its grants.
//!
//! ```json
//! { "version": 1, "sessions": ["<session id>", "…"] }
//! ```
//!
//! An unreadable or malformed file grants nothing: the boundary stays, and the
//! provider says why.

use std::path::Path;

/// The file name, in the provider's state directory.
pub const FULL_ACCESS_FILE: &str = "full-access.json";

/// The schema version this provider reads.
const VERSION: u64 = 1;

/// Stable reason code disclosed when a granted session runs unbounded.
pub const FULL_ACCESS_REASON: &str = "full-access";

/// Whether the person granted `session_id` full access on this computer.
#[must_use]
pub fn granted(state_dir: &Path, session_id: &str) -> bool {
    let path = state_dir.join(FULL_ACCESS_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => {
            tracing::warn!(
                target: "csp::scope",
                "full-access grants could not be read, so none apply: {error}"
            );
            return false;
        }
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        tracing::warn!(
            target: "csp::scope",
            "full-access grants are not JSON, so none apply"
        );
        return false;
    };
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(VERSION) {
        tracing::warn!(
            target: "csp::scope",
            "full-access grants name an unknown version, so none apply"
        );
        return false;
    }
    value
        .get("sessions")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|sessions| {
            sessions
                .iter()
                .any(|entry| entry.as_str() == Some(session_id))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_listed_session_in_a_well_formed_file_is_granted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path();
        assert!(!granted(state, "s-1"), "no file grants nothing");

        let file = state.join(FULL_ACCESS_FILE);
        std::fs::write(&file, r#"{"version":1,"sessions":["s-1"]}"#).expect("write");
        assert!(granted(state, "s-1"));
        assert!(!granted(state, "s-2"), "another session stays bounded");

        for bad in [
            "not json",
            r#"{"version":2,"sessions":["s-1"]}"#,
            r#"{"sessions":["s-1"]}"#,
            r#"{"version":1,"sessions":"s-1"}"#,
        ] {
            std::fs::write(&file, bad).expect("write");
            assert!(!granted(state, "s-1"), "{bad} must grant nothing");
        }
    }
}
