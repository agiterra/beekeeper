//! Starting and stopping the provider without restarting the host.
//!
//! The supervision loop owns one child and one restart ladder. Stopping the
//! provider therefore means ending that loop, and starting it again means a
//! fresh one — a new ladder with a clean failure count, which is what a person
//! clicking "Restart" is asking for. This type is the thing that holds the
//! current loop and swaps it.
//!
//! The host process itself is never taken down by any of this. A provider that
//! gave up, or one an operator stopped, is a state a client must be able to
//! read and act on; a host that exited would look identical to a host that was
//! never installed, which is the distinction the whole status design exists to
//! preserve.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use buzz_session_host_core::config::HostConfig;
use buzz_session_host_core::record::CodingSessionProviderRecord;

use crate::state::ProviderChildState;
use crate::supervisor::{PublishedState, Supervisor};

/// The currently running supervision loop, if any.
struct Running {
    stop: Arc<AtomicBool>,
    handle: tokio::task::JoinHandle<()>,
}

/// Everything the host needs to run, and the handle on what it is running.
pub struct HostControl {
    config: Mutex<HostConfig>,
    record: CodingSessionProviderRecord,
    nsec: String,
    log_path: std::path::PathBuf,
    socket_path: std::path::PathBuf,
    published: Arc<PublishedState>,
    running: Mutex<Option<Running>>,
}

impl HostControl {
    pub fn new(
        config: HostConfig,
        record: CodingSessionProviderRecord,
        nsec: String,
        log_path: std::path::PathBuf,
        socket_path: std::path::PathBuf,
    ) -> Self {
        Self {
            config: Mutex::new(config),
            record,
            nsec,
            log_path,
            socket_path,
            published: Arc::new(PublishedState::default()),
            running: Mutex::new(None),
        }
    }

    /// The config this host is serving.
    pub fn config(&self) -> HostConfig {
        self.config
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    /// The provider log this host writes into.
    pub fn log_path(&self) -> &std::path::Path {
        &self.log_path
    }

    /// The published child state.
    pub fn child_state(&self) -> ProviderChildState {
        self.published.child()
    }

    /// Whether a supervision loop is running right now.
    ///
    /// Distinct from "a child is live": a loop inside a backoff window is
    /// running and has no child, and the two are different answers.
    pub fn is_supervising(&self) -> bool {
        self.running
            .lock()
            .map(|guard| {
                guard
                    .as_ref()
                    .is_some_and(|running| !running.handle.is_finished())
            })
            .unwrap_or(false)
    }

    /// Start supervising, unless a loop is already running.
    ///
    /// Idempotent on purpose: a client that calls `start` twice, or calls it
    /// against a host that already recovered on its own, must not end up with
    /// two loops fighting over one state-directory lock.
    pub fn start(self: &Arc<Self>) -> Result<bool, String> {
        let mut guard = self
            .running
            .lock()
            .map_err(|_| "the host's supervision lock is poisoned".to_string())?;
        if guard
            .as_ref()
            .is_some_and(|running| !running.handle.is_finished())
        {
            return Ok(false);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let supervisor = Supervisor::new(
            self.config(),
            self.record.clone(),
            self.nsec.clone(),
            self.log_path.clone(),
            self.socket_path.clone(),
            Arc::clone(&self.published),
            Arc::clone(&stop),
        );
        let handle = tokio::spawn(supervisor.run());
        *guard = Some(Running { stop, handle });
        Ok(true)
    }

    /// Stop supervising and wait for the child to be gone.
    ///
    /// Awaited rather than fired and forgotten: a `stop` that returned before
    /// the child died would let the caller's next `start` race the outgoing
    /// child for the state-directory lock, and the symptom would be a provider
    /// that refuses to start for reasons nothing in the log explains.
    pub async fn stop(&self) -> bool {
        let running = match self.running.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let Some(running) = running else {
            return false;
        };
        running.stop.store(true, Ordering::Release);
        let _ = running.handle.await;
        true
    }

    /// Stop, then start. The new loop gets a clean failure count, which is
    /// what somebody asking for a restart means by it.
    pub async fn restart(self: &Arc<Self>) -> Result<(), String> {
        self.stop().await;
        self.start().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_session_host_core::layout::Instance;

    fn control(dir: &std::path::Path, provider: &std::path::Path) -> Arc<HostControl> {
        let state_dir = dir.join("state");
        std::fs::create_dir_all(&state_dir).expect("mkdir");
        Arc::new(HostControl::new(
            HostConfig {
                version: buzz_session_host_core::config::HOST_CONFIG_VERSION,
                instance: Instance::Production,
                relay_url: "wss://hive.example.org".to_string(),
                provider_pubkey: "e".repeat(64),
                session_provider_base_dir: dir.to_path_buf(),
                provider_state_dir: state_dir,
                runtimes: Vec::new(),
                max_sessions: None,
                turn_idle_timeout_secs: None,
                turn_budget: None,
                provider_command: Some(provider.to_path_buf()),
                written_at: buzz_session_host_core::logs::now_iso(),
            },
            CodingSessionProviderRecord {
                provider_pubkey: "e".repeat(64),
                instance_id: "e".repeat(16),
                auth_tag: None,
                created_at: buzz_session_host_core::logs::now_iso(),
                relay_url: "wss://hive.example.org".to_string(),
                private_key_nsec: String::new(),
            },
            "nsec1fake".to_string(),
            dir.join("provider.log"),
            dir.join("host.sock"),
        ))
    }

    fn sleeping_provider(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("sleepy-provider");
        std::fs::write(&path, "#!/bin/sh\nexec sleep 120\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        path
    }

    /// Two `start` calls must not produce two loops: they would fight over one
    /// state-directory lock, and the loser's restart ladder would spend itself
    /// on a lock it can never win.
    #[cfg(unix)]
    #[tokio::test]
    async fn starting_twice_leaves_exactly_one_loop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let provider = sleeping_provider(dir.path());
        let control = control(dir.path(), &provider);

        assert!(control.start().expect("first start"));
        // Let the loop reach a live child before asking again.
        for _ in 0..40 {
            if control.child_state().live_pid().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let first_pid = control.child_state().live_pid().expect("a live child");
        assert!(
            !control.start().expect("second start"),
            "a second start must be a no-op"
        );
        assert_eq!(control.child_state().live_pid(), Some(first_pid));

        assert!(control.stop().await);
        assert!(!control.is_supervising());
        assert!(
            !crate::terminate::pid_is_running(first_pid),
            "stop must wait for the child to actually be gone"
        );
        // And stopping again is not an error.
        assert!(!control.stop().await);
    }

    /// A restart gives the new loop a clean ladder — otherwise a person who
    /// fixed the cause would still be inside the old backoff.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_restart_replaces_the_child_and_the_failure_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let provider = sleeping_provider(dir.path());
        let control = control(dir.path(), &provider);
        control.start().expect("start");
        for _ in 0..40 {
            if control.child_state().live_pid().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let before = control.child_state().live_pid().expect("a live child");
        control.restart().await.expect("restart");
        for _ in 0..40 {
            if control
                .child_state()
                .live_pid()
                .is_some_and(|pid| pid != before)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let after = control.child_state().live_pid().expect("a live child");
        assert_ne!(before, after);
        assert!(!crate::terminate::pid_is_running(before));
        control.stop().await;
    }
}
