//! The on-disk receipt a running host writes so a (re)launched app can find and
//! reattach to it.
//!
//! One `<id>.json` per session under the hosts directory
//! (`~/.local/state/buzz/shell-hosts/`). It records where the socket is and
//! which pids to liveness-check before trusting it. The app treats a receipt as
//! live only when the host pid is alive **and** the socket connects; otherwise
//! it falls back to the on-disk history restore.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub id: String,
    pub socket_path: String,
    /// The host process's pid — liveness-checked before reattaching.
    pub host_pid: u32,
    /// The shell process's pid, for cwd probing after reattach.
    pub shell_pid: Option<u32>,
    pub title: String,
    pub shell: String,
    /// Directory the shell was launched in (the receipt's cwd is refreshed as
    /// the checkpoint runs).
    pub cwd: String,
    pub created_at: u64,
}

impl Receipt {
    pub fn path_in(dir: &Path, id: &str) -> PathBuf {
        dir.join(format!("{id}.json"))
    }

    /// Write the receipt to `<dir>/<id>.json`.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(Self::path_in(dir, &self.id), json)
    }

    /// Load a receipt from a `<id>.json` path.
    pub fn load(path: &Path) -> Option<Receipt> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Best-effort delete of `<dir>/<id>.json`.
    pub fn remove(dir: &Path, id: &str) {
        let _ = std::fs::remove_file(Self::path_in(dir, id));
    }
}

/// Whether a process id is currently alive (`kill(pid, 0)` on Unix).
#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    // SAFETY: kill with signal 0 performs error checking without sending a
    // signal; it never dereferences memory.
    unsafe {
        libc::kill(pid as libc::pid_t, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(not(unix))]
pub fn pid_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_round_trips() {
        let r = Receipt {
            id: "abc".to_string(),
            socket_path: "/tmp/abc.sock".to_string(),
            host_pid: 42,
            shell_pid: Some(43),
            title: "innovo".to_string(),
            shell: "/bin/zsh".to_string(),
            cwd: "/Users/andy/Code/innovo".to_string(),
            created_at: 7,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"hostPid\":42"));
        let back: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn our_own_pid_is_alive() {
        assert!(pid_alive(std::process::id()));
    }

    #[test]
    fn implausible_pid_is_not_alive() {
        // A pid far above any real one shouldn't exist.
        assert!(!pid_alive(2_000_000_000));
    }
}
