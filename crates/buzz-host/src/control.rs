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
use buzz_session_host_core::layout::Instance;

use crate::commission::{commission, Commissioned};
use crate::state::ProviderChildState;
use crate::supervisor::{PublishedState, Supervisor};

/// The currently running supervision loop, if any.
struct Running {
    stop: Arc<AtomicBool>,
    handle: tokio::task::JoinHandle<()>,
}

/// Everything the host needs to run, and the handle on what it is running.
///
/// The commissioning is behind a lock because it can be **re-read while the
/// host runs**: a community switch rewrites `host.json`, and handing an
/// identity over writes the key file. Both then ask the host to pick the
/// change up, which is a supervisor swap rather than a process restart — the
/// control socket stays up throughout, so a client never sees the host
/// disappear and come back.
pub struct HostControl {
    home: std::path::PathBuf,
    instance: Instance,
    commissioned: Mutex<Commissioned>,
    socket_path: std::path::PathBuf,
    published: Arc<PublishedState>,
    running: Mutex<Option<Running>>,
}

impl HostControl {
    pub fn new(
        home: std::path::PathBuf,
        instance: Instance,
        commissioned: Commissioned,
        socket_path: std::path::PathBuf,
    ) -> Self {
        Self {
            home,
            instance,
            commissioned: Mutex::new(commissioned),
            socket_path,
            published: Arc::new(PublishedState::default()),
            running: Mutex::new(None),
        }
    }

    fn with_commissioned<T>(&self, read: impl FnOnce(&Commissioned) -> T) -> T {
        match self.commissioned.lock() {
            Ok(guard) => read(&guard),
            Err(poisoned) => read(&poisoned.into_inner()),
        }
    }

    /// The config this host is serving.
    pub fn config(&self) -> HostConfig {
        self.with_commissioned(|commissioned| commissioned.config.clone())
    }

    /// The provider log this host writes into.
    pub fn log_path(&self) -> std::path::PathBuf {
        self.with_commissioned(|commissioned| commissioned.log_path())
    }

    /// Re-read `host.json`, the record store and the key, then swap the
    /// supervisor onto the result.
    ///
    /// This is what `bind` (a community switch) and `adopt-identity` (a key
    /// handed over at commissioning) both do. Two properties matter:
    ///
    /// - **The new commissioning is validated before the old one is
    ///   discarded.** A `host.json` an operator broke while editing must leave
    ///   the running provider exactly as it was, and say what is wrong — not
    ///   stop a working provider in order to fail.
    /// - **The child is stopped before the new one starts**, because both want
    ///   the same state-directory lock when the identity did not change.
    pub async fn recommission(self: &Arc<Self>) -> Result<(), String> {
        let next = commission(&self.home, self.instance)?;
        let was_supervising = self.is_supervising();
        self.stop().await;
        match self.commissioned.lock() {
            Ok(mut guard) => *guard = next,
            Err(poisoned) => *poisoned.into_inner() = next,
        }
        if was_supervising {
            self.start()?;
        }
        Ok(())
    }

    /// The published child state.
    pub fn child_state(&self) -> ProviderChildState {
        self.published.child()
    }

    /// What the running provider was started with, or `None`.
    pub fn settings_in_force(&self) -> Option<crate::state::RunSettings> {
        self.published.in_force()
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
        let supervisor = self.with_commissioned(|commissioned| {
            Supervisor::new(
                commissioned.config.clone(),
                commissioned.record.clone(),
                commissioned.key.nsec.clone(),
                commissioned.log_path(),
                self.socket_path.clone(),
                Arc::clone(&self.published),
                Arc::clone(&stop),
            )
        });
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
    use crate::commission::write_test_commissioning;

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

    fn control(home: &std::path::Path) -> Arc<HostControl> {
        let provider = sleeping_provider(home);
        write_test_commissioning(home, "wss://hive.example.org", Some(&provider));
        let commissioned =
            commission(home, Instance::Production).expect("the fixture is a full commissioning");
        Arc::new(HostControl::new(
            home.to_path_buf(),
            Instance::Production,
            commissioned,
            home.join("host.sock"),
        ))
    }

    async fn wait_for_live(control: &Arc<HostControl>) -> u32 {
        for _ in 0..60 {
            if let Some(pid) = control.child_state().live_pid() {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!(
            "the supervisor never reported a live child: {:?}",
            control.child_state()
        );
    }

    #[test]
    fn nothing_started_yet_is_not_supervising() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!control(dir.path()).is_supervising());
    }

    /// Two `start` calls must not produce two loops: they would fight over one
    /// state-directory lock, and the loser's restart ladder would spend itself
    /// on a lock it can never win.
    #[cfg(unix)]
    #[tokio::test]
    async fn starting_twice_leaves_exactly_one_loop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let control = control(dir.path());

        assert!(control.start().expect("first start"));
        let first_pid = wait_for_live(&control).await;
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
    async fn a_restart_replaces_the_child() {
        let dir = tempfile::tempdir().expect("tempdir");
        let control = control(dir.path());
        control.start().expect("start");
        let before = wait_for_live(&control).await;
        control.restart().await.expect("restart");
        for _ in 0..60 {
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

    /// A community switch: rewrite `host.json`, ask the host to pick it up,
    /// and the provider comes back serving the new relay.
    #[cfg(unix)]
    #[tokio::test]
    async fn recommissioning_picks_up_a_rewritten_config_and_restarts_the_child() {
        let dir = tempfile::tempdir().expect("tempdir");
        let control = control(dir.path());
        control.start().expect("start");
        let before = wait_for_live(&control).await;
        assert_eq!(control.config().relay_url, "wss://hive.example.org");

        // The app rewrites host.json for the new community. The record store
        // must name the new relay too — it is the same identity either way in
        // this fixture, which is the case a community switch back and forth
        // produces.
        let config_path =
            buzz_session_host_core::layout::host_config_path(dir.path(), Instance::Production);
        let mut config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).expect("read"))
                .expect("parse");
        let store_path =
            std::path::PathBuf::from(config["sessionProviderBaseDir"].as_str().expect("base"))
                .join(buzz_session_host_core::record::STORE_FILE_NAME);
        let mut store: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&store_path).expect("read"))
                .expect("parse");
        let record = store["providers"]["wss://hive.example.org"].clone();
        store["providers"] = serde_json::json!({ "wss://other.example.org": record });
        std::fs::write(&store_path, store.to_string()).expect("write");
        config["relayUrl"] = serde_json::json!("wss://other.example.org");
        std::fs::write(&config_path, config.to_string()).expect("write");

        control.recommission().await.expect("recommission");
        assert_eq!(control.config().relay_url, "wss://other.example.org");
        let after = wait_for_live(&control).await;
        assert_ne!(after, before, "the child restarts onto the new relay");
        control.stop().await;
    }

    /// A `host.json` somebody broke while editing must leave the running
    /// provider exactly as it was, and say what is wrong. Stopping a working
    /// provider in order to fail is the worst of both.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_broken_recommission_leaves_the_running_provider_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let control = control(dir.path());
        control.start().expect("start");
        let before = wait_for_live(&control).await;

        let config_path =
            buzz_session_host_core::layout::host_config_path(dir.path(), Instance::Production);
        std::fs::write(&config_path, "{ not json").expect("write");

        let error = control.recommission().await.expect_err("must be refused");
        assert!(error.contains("failed to parse"), "{error}");
        assert_eq!(
            control.child_state().live_pid(),
            Some(before),
            "the running child must be untouched by a failed recommission"
        );
        assert!(crate::terminate::pid_is_running(before));
        control.stop().await;
    }
}
