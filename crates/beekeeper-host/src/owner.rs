//! Which host owns a provider state directory.
//!
//! The provider itself writes `provider.lock` in that directory, holding the
//! flock for its lifetime, with its own pid as the contents. That file is the
//! at-most-one-live-instance invariant and **is not changed here**: it is
//! parsed by whoever reads it as one trimmed integer, in both directions, so a
//! second line would make an older reader's `parse::<u32>()` fail and silently
//! stop taking over a stale provider.
//!
//! So the *host* writes a separate file saying who is supervising, and the
//! takeover path consults it before signalling anybody. Without it, two
//! launchers of the same identity — a pre-upgrade desktop with its own
//! supervisor, and a new host — would each SIGINT the other's provider and
//! restart their own, five times per ten-minute window each, until both gave
//! up. `docs/remote-agents.md` § I4 is explicit that the protocol "cannot and
//! does not promise a global singleton across unrelated launchers of the same
//! nsec", so this has to be handled locally or not at all.
//!
//! **What this file proves today**: that a process with that pid is running.
//! That is enough to stop the restart war, because the war needs both sides
//! alive. It does not prove the pid is still *this* host rather than a reused
//! pid, which is why the takeover path treats it as a reason to refuse loudly
//! rather than as a lock.

use std::path::{Path, PathBuf};

use beekeeper_host_core::atomic_write::atomic_write_json_restricted;
use beekeeper_host_core::layout::host_owner_file_name;
use serde::{Deserialize, Serialize};

/// The claim a running host makes on a provider state directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostOwner {
    /// Pid of the supervising host.
    pub host_pid: u32,
    /// RFC 3339 stamp of when that host started supervising.
    pub host_started_at: String,
    /// The host's control socket, so a reader can ask it directly.
    pub socket: PathBuf,
}

/// The claim file inside a provider state directory.
pub fn owner_path(state_dir: &Path) -> PathBuf {
    state_dir.join(host_owner_file_name())
}

/// Record that this host is supervising the provider in `state_dir`.
pub fn claim(state_dir: &Path, socket: PathBuf) -> Result<HostOwner, String> {
    let owner = HostOwner {
        host_pid: std::process::id(),
        host_started_at: beekeeper_host_core::logs::now_iso(),
        socket,
    };
    let payload = serde_json::to_vec_pretty(&owner)
        .map_err(|error| format!("failed to encode the host owner file: {error}"))?;
    let path = owner_path(state_dir);
    // `atomic_write_json_restricted` opens an existing path; create it first so
    // a fresh state directory works without a special case.
    if !path.exists() {
        std::fs::write(&path, b"{}")
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    }
    atomic_write_json_restricted(&path, &payload)?;
    Ok(owner)
}

/// Drop this host's claim. Best effort: a claim left behind by a crash is
/// handled by the liveness check, not by the file's absence.
pub fn release(state_dir: &Path) {
    let _ = std::fs::remove_file(owner_path(state_dir));
}

/// The claim recorded in `state_dir`, if there is a readable one.
pub fn read(state_dir: &Path) -> Option<HostOwner> {
    let content = std::fs::read_to_string(owner_path(state_dir)).ok()?;
    serde_json::from_str(&content).ok()
}

/// A claim held by a *different*, still-running host, if there is one.
///
/// Our own claim is not a conflict, and neither is a claim whose process is
/// gone — that is a crashed host, and taking over from it is the right thing.
pub fn conflicting_owner(state_dir: &Path) -> Option<HostOwner> {
    let owner = read(state_dir)?;
    if owner.host_pid == std::process::id() {
        return None;
    }
    crate::terminate::pid_is_running(owner.host_pid).then_some(owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_claim_is_not_a_conflict_and_a_dead_hosts_claim_is_not_either() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(conflicting_owner(dir.path()), None, "no claim at all");

        let ours = claim(dir.path(), PathBuf::from("/tmp/host.sock")).expect("claim");
        assert_eq!(ours.host_pid, std::process::id());
        assert_eq!(read(dir.path()), Some(ours));
        assert_eq!(
            conflicting_owner(dir.path()),
            None,
            "a host must not refuse to take over from itself"
        );

        // A claim from a pid that cannot exist is a crashed host: taking over
        // is correct, so it must not read as a conflict.
        let dead = HostOwner {
            host_pid: u32::MAX - 1,
            host_started_at: "2026-09-30T00:00:00Z".to_string(),
            socket: PathBuf::from("/tmp/host.sock"),
        };
        std::fs::write(
            owner_path(dir.path()),
            serde_json::to_vec(&dead).expect("encode"),
        )
        .expect("write");
        assert_eq!(conflicting_owner(dir.path()), None);

        // A live *other* process is a conflict. pid 1 is alive on every Unix.
        #[cfg(unix)]
        {
            let live = HostOwner {
                host_pid: 1,
                ..dead
            };
            std::fs::write(
                owner_path(dir.path()),
                serde_json::to_vec(&live).expect("encode"),
            )
            .expect("write");
            assert_eq!(conflicting_owner(dir.path()), Some(live));
        }
    }

    #[test]
    fn a_released_claim_leaves_nothing_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        claim(dir.path(), PathBuf::from("/tmp/host.sock")).expect("claim");
        release(dir.path());
        assert_eq!(read(dir.path()), None);
        // And releasing twice is not an error.
        release(dir.path());
    }

    #[test]
    fn the_claim_file_is_not_the_providers_lock_file() {
        // Restated as a test because writing into `provider.lock` is the
        // mistake: every reader parses that file as one trimmed integer.
        let dir = tempfile::tempdir().expect("tempdir");
        assert_ne!(
            owner_path(dir.path()).file_name(),
            Some(std::ffi::OsStr::new("provider.lock"))
        );
        claim(dir.path(), PathBuf::from("/tmp/host.sock")).expect("claim");
        assert!(!dir.path().join("provider.lock").exists());
    }
}
