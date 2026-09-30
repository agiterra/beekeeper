//! Taking a provider state directory over from a stale owner — or refusing to.
//!
//! The provider holds an exclusive flock on its state directory for the life
//! of the process (one live instance per directory). An orphan from a previous
//! desktop run, reparented to launchd, still holds it, so the child we are
//! about to spawn would fail its own lock. "Stop the stale one, then start" is
//! the takeover this architecture supports: a supervisor owns the provider
//! strictly as a child process and has no channel to adopt a foreign one.
//!
//! # The refusal is the important half
//!
//! The desktop's version signalled whoever held the lock as soon as a
//! *different* live pid held it. With two launchers of the same identity alive
//! — a pre-upgrade desktop with its own restart ladder, and a new host — each
//! would SIGINT the other's provider and restart its own, five times per
//! ten-minute window each, until both gave up. That is a bounded, transient
//! population (the old desktop gets replaced), so this does not try to *win*
//! it; it tries to make it loud and to refuse the one case it can recognise:
//! another `buzz-host` that said so in `host-owner.json`.

use std::path::Path;
use std::time::Duration;

use buzz_session_host_core::logs::{append_log_marker, now_iso};

use crate::state::LockOwnerKind;
use crate::terminate::{pid_is_running, terminate_gracefully_blocking};

/// Lock file the provider holds inside its state directory (see
/// `buzz-session-provider`'s `state::acquire_state_dir_lock`). Its contents
/// are the owner's pid, written while holding the lock.
pub const PROVIDER_LOCK_FILE: &str = "provider.lock";
/// How long a stale owner's lock is given to clear after it was signalled.
const TAKEOVER_LOCK_WAIT: Duration = Duration::from_secs(5);

/// The owning pid recorded in a provider lock file, if the contents are one.
///
/// The whole trimmed file is parsed as one integer, in both directions. Pid 0
/// and 1 are refused: neither is ever a provider, and signalling them would be
/// a machine-wide accident.
pub fn parse_lock_owner_pid(contents: &str) -> Option<u32> {
    let pid = contents.trim().parse::<u32>().ok()?;
    (pid > 1).then_some(pid)
}

/// What a takeover attempt concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Takeover {
    /// Nothing holds the lock; the child can take it.
    Free,
    /// A stale owner was stopped and the lock is now free.
    TookOver { from_pid: u32 },
    /// The lock is held and this host will not fight for it.
    Refused {
        pid: Option<u32>,
        kind: LockOwnerKind,
    },
}

/// Decide what to do about whoever holds `state_dir`'s lock, and do it.
///
/// Best effort by design: when nothing holds the lock this is a no-op, and
/// when a takeover fails the child spawned next fails its own lock with a
/// clear log line rather than corrupting shared state. The graceful SIGINT
/// first lets a stale provider flush its durable outbox before it dies.
pub fn take_over_stale_provider(state_dir: &Path, log_path: &Path) -> Takeover {
    // Another host that announced itself is refused by name, before anything
    // is signalled. This is the restart-war guard.
    if let Some(owner) = crate::owner::conflicting_owner(state_dir) {
        let _ = append_log_marker(
            log_path,
            &format!(
                "=== refusing to take over: agent host pid {} has been supervising this provider \
                 since {} (socket {}) ===",
                owner.host_pid,
                owner.host_started_at,
                owner.socket.display()
            ),
        );
        return Takeover::Refused {
            pid: Some(owner.host_pid),
            kind: LockOwnerKind::AnotherHost,
        };
    }

    let lock_path = state_dir.join(PROVIDER_LOCK_FILE);
    let Ok(file) = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&lock_path)
    else {
        // No lock file: no provider has ever owned this directory.
        return Takeover::Free;
    };
    if file.try_lock().is_ok() {
        // Nothing holds it. Release before spawning so the child can lock it.
        let _ = file.unlock();
        return Takeover::Free;
    }

    let owner_pid = std::fs::read_to_string(&lock_path)
        .ok()
        .and_then(|contents| parse_lock_owner_pid(&contents));
    let Some(pid) = owner_pid else {
        let _ = append_log_marker(
            log_path,
            "=== a stale provider holds the state-dir lock but recorded no readable pid; the new \
             child will refuse to start until it exits ===",
        );
        return Takeover::Refused {
            pid: None,
            kind: LockOwnerKind::Unknown,
        };
    };
    if pid == std::process::id() {
        return Takeover::Free;
    }
    if !pid_is_running(pid) {
        // The lock is held but the recorded pid is gone: the file is stale and
        // somebody else holds the flock. Refuse rather than signal a pid that
        // may since have been reused by an unrelated process.
        let _ = append_log_marker(
            log_path,
            &format!(
                "=== the state-dir lock is held but its recorded pid {pid} is gone; refusing to \
                 signal a pid that may have been reused ==="
            ),
        );
        return Takeover::Refused {
            pid: Some(pid),
            kind: LockOwnerKind::Unknown,
        };
    }

    let _ = append_log_marker(
        log_path,
        &format!(
            "=== taking over the provider state dir from stale pid {pid} at {} ===",
            now_iso()
        ),
    );
    terminate_gracefully_blocking(pid);
    // The OS drops the lock when the owner dies; wait for that to be visible
    // so the child we spawn next does not lose a takeover race it just won.
    let deadline = std::time::Instant::now() + TAKEOVER_LOCK_WAIT;
    while std::time::Instant::now() < deadline {
        if file.try_lock().is_ok() {
            let _ = file.unlock();
            return Takeover::TookOver { from_pid: pid };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = append_log_marker(
        log_path,
        &format!("=== stale pid {pid} did not release the state-dir lock in time ==="),
    );
    Takeover::Refused {
        pid: Some(pid),
        kind: LockOwnerKind::UnclaimedProvider,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_owner_pid_parsing_rejects_garbage_and_system_pids() {
        assert_eq!(parse_lock_owner_pid("4242\n"), Some(4242));
        assert_eq!(parse_lock_owner_pid("  77  "), Some(77));
        assert_eq!(parse_lock_owner_pid(""), None);
        assert_eq!(parse_lock_owner_pid("not-a-pid"), None);
        assert_eq!(parse_lock_owner_pid("0"), None);
        assert_eq!(parse_lock_owner_pid("1"), None, "pid 1 is never a provider");
        assert_eq!(
            parse_lock_owner_pid("42\nhost-pid 7"),
            None,
            "the whole trimmed file is one integer — a second line must not \
             parse as the first, in either direction"
        );
    }

    #[test]
    fn a_directory_no_provider_ever_owned_is_free() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("provider.log");
        assert_eq!(take_over_stale_provider(dir.path(), &log), Takeover::Free);
        assert!(!log.exists(), "a no-op must not write a log marker");
    }

    #[test]
    fn an_unlocked_lock_file_is_free_and_left_unlocked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("provider.log");
        std::fs::write(dir.path().join(PROVIDER_LOCK_FILE), "4242\n").expect("lock file");
        assert_eq!(take_over_stale_provider(dir.path(), &log), Takeover::Free);
        // The child must be able to take it.
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .open(dir.path().join(PROVIDER_LOCK_FILE))
            .expect("open");
        assert!(file.try_lock().is_ok(), "the lock must be free afterwards");
    }

    /// The restart-war guard: another host that announced itself is refused by
    /// name, and nothing is signalled.
    #[cfg(unix)]
    #[test]
    fn another_live_host_is_refused_by_name_before_anything_is_signalled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("provider.log");
        // pid 1 stands in for "a live process that is not us".
        let owner = crate::owner::HostOwner {
            host_pid: 1,
            host_started_at: "2026-09-30T00:00:00Z".to_string(),
            socket: std::path::PathBuf::from("/tmp/other-host.sock"),
        };
        std::fs::write(
            crate::owner::owner_path(dir.path()),
            serde_json::to_vec(&owner).expect("encode"),
        )
        .expect("write");
        assert_eq!(
            take_over_stale_provider(dir.path(), &log),
            Takeover::Refused {
                pid: Some(1),
                kind: LockOwnerKind::AnotherHost
            }
        );
        let logged = std::fs::read_to_string(&log).expect("log");
        assert!(logged.contains("refusing to take over"), "{logged}");
        assert!(
            logged.contains("/tmp/other-host.sock"),
            "the refusal must name where to ask the other host: {logged}"
        );
    }

    /// A held lock whose recorded pid is gone must not be signalled: that pid
    /// may have been reused by something unrelated.
    #[test]
    fn a_held_lock_with_a_dead_recorded_pid_is_refused_rather_than_signalled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("provider.log");
        let lock_path = dir.path().join(PROVIDER_LOCK_FILE);
        std::fs::write(&lock_path, format!("{}\n", u32::MAX - 1)).expect("lock file");
        // Hold the flock from this process so the path under test sees it as
        // held while the recorded pid is absent.
        let holder = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&lock_path)
            .expect("open");
        holder.lock().expect("hold the lock");
        assert_eq!(
            take_over_stale_provider(dir.path(), &log),
            Takeover::Refused {
                pid: Some(u32::MAX - 1),
                kind: LockOwnerKind::Unknown
            }
        );
        assert!(std::fs::read_to_string(&log)
            .expect("log")
            .contains("may have been reused"));
        let _ = holder.unlock();
    }
}
