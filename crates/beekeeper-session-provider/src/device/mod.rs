//! Provider-owned iOS Simulator for coding sessions (SV-34 S1/S2,
//! `plans/SESSION_PARITY_SPEC_DEVICE.md`, wire `WIRE-C5`).
//!
//! The provider runs outside the seat boundary, so it owns the device: it
//! boots and captures through simctl, installs the pinned agent-device and
//! runs its daemon, and signs every fact about the device. A seat (or a
//! session person) asks with a stored 44254; the provider answers with
//! exactly one terminal 44255, plus a 44253 snapshot for `screenshot`.
//! Watching is the shared 24320/24321 pair: frames flow only while someone
//! watches.
//!
//! Host-local facts — UDID, daemon port and token, every path — stay in the
//! 0600 slot file ([`slot`]); every signed event passes
//! [`slot::assert_publishable`].
//!
//! # Shape
//!
//! [`DeviceService::spawn`] starts one task holding its own authenticated
//! relay connection (the [`crate::action_step_listener`] pattern) and the
//! synchronous [`listener::Engine`]. The provider's run loop only calls
//! [`DeviceService::serve`] with the generations it serves, which is a
//! `watch` send — nothing here is awaited on the provider's loop.

pub mod agent_device;
pub mod capture;
pub mod journal;
pub mod listener;
pub mod simctl;
pub mod slot;
pub mod toolchain;
pub mod wire;
pub mod work;

#[cfg(test)]
mod test_support;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use beekeeper_ws_client::{NostrWsConnection, RelayMessage};
use nostr::{Keys, Tag};
use serde_json::json;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

pub use listener::DeviceSessionView;
use listener::{Engine, Journal, Outgoing};
use simctl::{CommandRunner, Simctl, SystemRunner};
use slot::DevicePaths;
use work::{Worker, WorkerConfig};

/// The seat variable naming its session's device directory
/// (`<CSP state>/device/<session>`), read by `bee device`.
pub const DEVICE_DIR_ENV: &str = "BEEKEEPER_DEVICE_DIR";

/// Subscription id prefix.
const SUBSCRIPTION_PREFIX: &str = "csp-device";
/// How often the subscription is re-issued as a liveness probe.
const PROBE_INTERVAL: Duration = Duration::from_secs(90);
/// How long a probe waits for `EOSE`.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Reconnect backoff bounds.
const FIRST_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Queue depths.
const WORK_CAPACITY: usize = 32;
const CAPTURE_CAPACITY: usize = 8;
/// Stored events queued while disconnected, beyond the journal's own.
const MAX_UNSENT: usize = 256;

/// What the device service needs that does not change while it runs.
#[derive(Clone)]
pub struct DeviceConfig {
    /// Relay WebSocket URL.
    pub relay_url: String,
    /// This provider's keys: the device records' signer.
    pub keys: Keys,
    /// NIP-OA auth tag, when the deployment requires one.
    pub auth_tag: Option<Tag>,
    /// `BUZZ_CSP_STATE_DIR`.
    pub state_dir: PathBuf,
    /// `Some(reason)` when this host turned devices off (published as the
    /// availability reason; every command is refused with it).
    pub disabled: Option<String>,
}

impl DeviceConfig {
    /// Where `node` may live for this provider process.
    fn node_candidates(&self) -> Vec<PathBuf> {
        let override_path = std::env::var_os("BEEKEEPER_DEVICE_NODE").map(PathBuf::from);
        let path_var = std::env::var("PATH").ok();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        toolchain::node_candidates(
            override_path.as_deref(),
            path_var.as_deref(),
            home.as_deref(),
        )
    }
}

/// The provider's handle on the device service.
pub struct DeviceService {
    views: watch::Sender<Arc<Vec<DeviceSessionView>>>,
    paths: DevicePaths,
    task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for DeviceService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DeviceService")
    }
}

impl DeviceService {
    /// Start the service with the real simctl.
    pub fn spawn(config: DeviceConfig) -> Self {
        Self::spawn_with(config, Arc::new(SystemRunner))
    }

    /// Start the service with `runner` (tests substitute a fake).
    pub fn spawn_with(config: DeviceConfig, runner: Arc<dyn CommandRunner>) -> Self {
        let paths = DevicePaths::new(&config.state_dir);
        let (views, views_rx) = watch::channel(Arc::new(Vec::new()));
        let (work_tx, work_rx) = mpsc::channel(WORK_CAPACITY);
        let (capture_tx, capture_rx) = mpsc::channel(CAPTURE_CAPACITY);
        let uploader = crate::artifact_upload::ArtifactUploader::new(
            &config.relay_url,
            config.keys.clone(),
            config.auth_tag.as_ref(),
        );
        let worker = Worker::new(
            WorkerConfig {
                me: config.keys.public_key().to_hex(),
                paths: paths.clone(),
                node_candidates: config.node_candidates(),
                disabled: config.disabled.clone(),
                developer_dirs: toolchain::DeveloperDirs::host(),
            },
            runner.clone(),
            uploader,
            work_tx,
        );
        let engine = Engine::new(
            config.keys.clone(),
            paths.clone(),
            Simctl::new(runner),
            worker,
            capture_tx,
            Journal::load(paths.journal_file()),
        );
        let task = tokio::spawn(run(config, engine, views_rx, work_rx, capture_rx));
        Self { views, paths, task }
    }

    /// Replace the generations this provider serves. A no-op when unchanged.
    pub fn serve(&self, views: Vec<DeviceSessionView>) {
        if **self.views.borrow() == views {
            return;
        }
        let _ = self.views.send(Arc::new(views));
    }

    /// The directory a seat of `session_id` prepends to its PATH for the
    /// pinned `agent-device` shim (written on the first successful open).
    pub fn shim_dir(&self, session_id: &str) -> PathBuf {
        self.paths.shim_dir(session_id)
    }

    /// The directory a seat of `session_id` is granted read access to: its
    /// slot files and snapshots.
    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.paths.session_dir(session_id)
    }

    /// The pinned tool install root (read + execute for a device seat).
    pub fn tools_dir(&self) -> PathBuf {
        self.paths.tools_dir()
    }

    /// What a device-enabled seat of `session_id` needs from its boundary.
    pub fn seat_access(&self, session_id: &str) -> SeatDeviceAccess {
        seat_access(&self.paths, session_id)
    }

    /// Stop the service.
    pub fn shutdown(self) {
        self.task.abort();
    }
}

/// The boundary facts for one session's seat (brief § 4 Lane D, integrator
/// requests): read-only trees, the Node to execute, the daemon port to allow
/// under a loopback-proxy egress, and the shim directory to put first on
/// PATH. Reads host files only; never publishes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatDeviceAccess {
    /// Read-only trees: this session's slot/snapshot directory and the pinned
    /// tool install. Nothing of any other session.
    pub read_trees: Vec<PathBuf>,
    /// The Node the shim runs, when a device with agent-device is open.
    pub node: Option<PathBuf>,
    /// The agent-device daemon's loopback port, when one is running.
    pub daemon_port: Option<u16>,
    /// Prepend to the seat's PATH (`seat_bee::compose_seat_path`).
    pub shim_dir: PathBuf,
}

/// [`DeviceService::seat_access`] without a running service. Creates the
/// session directory so a grant on it exists before the first open.
pub fn seat_access(paths: &DevicePaths, session_id: &str) -> SeatDeviceAccess {
    let session_dir = paths.session_dir(session_id);
    let _ = std::fs::create_dir_all(&session_dir);
    let mut node = None;
    let mut daemon_port = agent_device::read_daemon_file(paths).map(|file| file.http_port);
    if let Ok(entries) = std::fs::read_dir(&session_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(slot) = name
                .strip_suffix(".json")
                .filter(|slot| wire::valid_slot(slot))
            else {
                continue;
            };
            if let Ok(file) = slot::read_slot_file(paths, session_id, slot) {
                node = node.or(file.node);
                daemon_port = file.daemon_port.or(daemon_port);
            }
        }
    }
    SeatDeviceAccess {
        read_trees: vec![session_dir, paths.tools_dir()],
        node,
        daemon_port,
        shim_dir: paths.shim_dir(session_id),
    }
}

async fn run(
    config: DeviceConfig,
    mut engine: Engine,
    mut views: watch::Receiver<Arc<Vec<DeviceSessionView>>>,
    mut work: mpsc::Receiver<work::WorkDone>,
    mut capture: mpsc::Receiver<capture::CaptureOutput>,
) {
    let mut unsent: VecDeque<Outgoing> = engine.recover().into();
    let mut backoff = FIRST_BACKOFF;
    loop {
        let current = views.borrow_and_update().clone();
        unsent.extend(engine.serve(current.as_ref().clone()));
        if engine.filters().is_none() {
            // Nothing served: wait for views, still applying finished work.
            tokio::select! {
                changed = views.changed() => if changed.is_err() { engine.stop_all(); return; },
                Some(done) = work.recv() => unsent.extend(engine.on_work(done)),
                Some(output) = capture.recv() => { engine.on_capture(output); }
            }
            continue;
        }
        match serve_connection(
            &config,
            &mut engine,
            &mut views,
            &mut work,
            &mut capture,
            &mut unsent,
        )
        .await
        {
            Ok(Some(())) => backoff = FIRST_BACKOFF,
            Ok(None) => {
                engine.stop_all();
                return;
            }
            Err(reason) => {
                tracing::warn!(target: "csp::device", backoff_secs = backoff.as_secs(), "device connection ended: {reason}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// Send one outgoing event. Stored events wait for the relay's `OK`;
/// frames are fire-and-forget.
async fn send(
    conn: &mut NostrWsConnection,
    engine: &mut Engine,
    outgoing: Outgoing,
) -> Result<(), (String, Option<Outgoing>)> {
    match outgoing {
        Outgoing::Ephemeral(event) => conn
            .send_raw(&json!(["EVENT", event]))
            .await
            .map_err(|error| (format!("send frame: {error}"), None)),
        Outgoing::Stored { key, event, last } => {
            let retry = Outgoing::Stored {
                key: key.clone(),
                event: event.clone(),
                last,
            };
            let ok = conn
                .send_event(event)
                .await
                .map_err(|error| (format!("send record: {error}"), Some(retry)))?;
            let accepted = ok.accepted || ok.message.starts_with("duplicate");
            if !accepted {
                // A refusal is not retried: the same bytes would be refused
                // again. Logged so the gap is visible.
                tracing::warn!(target: "csp::device", event_id = %ok.event_id, "the relay refused a device record: {}", ok.message);
            }
            if last {
                if let Some(key) = key {
                    engine.mark_published(&key);
                }
            }
            Ok(())
        }
    }
}

async fn serve_connection(
    config: &DeviceConfig,
    engine: &mut Engine,
    views: &mut watch::Receiver<Arc<Vec<DeviceSessionView>>>,
    work: &mut mpsc::Receiver<work::WorkDone>,
    capture: &mut mpsc::Receiver<capture::CaptureOutput>,
    unsent: &mut VecDeque<Outgoing>,
) -> Result<Option<()>, String> {
    let mut conn = NostrWsConnection::connect_authenticated(
        &config.relay_url,
        &config.keys,
        config.auth_tag.as_ref(),
    )
    .await
    .map_err(|error| format!("connect: {error}"))?;
    let mut sequence: u64 = 0;
    let mut filters = engine.filters();
    let mut current = subscribe(&mut conn, filters.as_ref(), None, sequence).await?;
    let mut probe_at = Instant::now() + PROBE_INTERVAL;
    let mut eose_by = Some(Instant::now() + PROBE_TIMEOUT);
    // Journal answers first, then anything queued while disconnected.
    let mut queue: VecDeque<Outgoing> = engine.unpublished().into();
    queue.extend(
        unsent
            .drain(..)
            .filter(|outgoing| matches!(outgoing, Outgoing::Stored { key: None, .. })),
    );
    loop {
        while let Some(outgoing) = queue.pop_front() {
            if let Err((reason, retry)) = send(&mut conn, engine, outgoing).await {
                unsent.extend(retry);
                unsent.extend(
                    queue
                        .drain(..)
                        .filter(|o| matches!(o, Outgoing::Stored { key: None, .. })),
                );
                unsent.truncate(MAX_UNSENT);
                return Err(reason);
            }
        }
        let now = Instant::now();
        if eose_by.is_some_and(|deadline| now >= deadline) {
            return Err(format!(
                "no EOSE for {current} within {} s",
                PROBE_TIMEOUT.as_secs()
            ));
        }
        if now >= probe_at {
            sequence += 1;
            current = subscribe(&mut conn, filters.as_ref(), Some(&current), sequence).await?;
            probe_at = now + PROBE_INTERVAL;
            eose_by = Some(now + PROBE_TIMEOUT);
            continue;
        }
        let mut wait = probe_at.saturating_duration_since(now);
        if let Some(deadline) = eose_by {
            wait = wait.min(deadline.saturating_duration_since(now));
        }
        let wait = wait.max(Duration::from_millis(1));
        tokio::select! {
            changed = views.changed() => {
                if changed.is_err() {
                    return Ok(None);
                }
                let current_views = views.borrow_and_update().clone();
                queue.extend(engine.serve(current_views.as_ref().clone()));
                let next = engine.filters();
                if next != filters {
                    filters = next;
                    if filters.is_none() {
                        return Ok(Some(()));
                    }
                    sequence += 1;
                    current = subscribe(&mut conn, filters.as_ref(), Some(&current), sequence).await?;
                    eose_by = Some(Instant::now() + PROBE_TIMEOUT);
                }
            }
            Some(done) = work.recv() => queue.extend(engine.on_work(done)),
            Some(output) = capture.recv() => queue.extend(engine.on_capture(output)),
            message = conn.next_event(wait) => match message {
                Ok(RelayMessage::Event { subscription_id, event }) if subscription_id.starts_with(SUBSCRIPTION_PREFIX) => {
                    queue.extend(engine.on_event(&event));
                }
                Ok(RelayMessage::Eose { subscription_id }) if subscription_id == current => eose_by = None,
                Ok(RelayMessage::Closed { subscription_id, message }) if subscription_id == current => {
                    return Err(format!("the relay closed {subscription_id}: {message}"));
                }
                Ok(_) => {}
                Err(beekeeper_ws_client::WsClientError::Timeout) => {}
                Err(error) => return Err(format!("read: {error}")),
            },
        }
    }
}

async fn subscribe(
    conn: &mut NostrWsConnection,
    filters: Option<&serde_json::Value>,
    previous: Option<&str>,
    sequence: u64,
) -> Result<String, String> {
    if let Some(previous) = previous {
        conn.send_raw(&json!(["CLOSE", previous]))
            .await
            .map_err(|error| format!("close: {error}"))?;
    }
    let id = format!("{SUBSCRIPTION_PREFIX}-{sequence}");
    let mut frame = vec![json!("REQ"), json!(id)];
    if let Some(serde_json::Value::Array(filters)) = filters {
        frame.extend(filters.iter().cloned());
    }
    conn.send_raw(&serde_json::Value::Array(frame))
        .await
        .map_err(|error| format!("subscribe: {error}"))?;
    tracing::info!(target: "csp::device", subscription_id = %id, "device listener subscribed");
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seat_access_names_only_this_session_and_the_tools() {
        let dir = tempfile::tempdir().expect("state");
        let paths = DevicePaths::new(dir.path());
        let slot = slot::SlotFile {
            version: slot::SLOT_FILE_VERSION,
            slot: "0123456789abcdef".into(),
            udid: "u".into(),
            platform: "ios".into(),
            model: "iPhone 17".into(),
            os_version: "iOS 27.0".into(),
            session_name: "bk-0123456789abcdef".into(),
            agent_device_config: None,
            daemon_port: Some(4123),
            node: Some(PathBuf::from("/opt/node/bin/node")),
            shot_dir: paths.shot_dir("a", "0123456789abcdef"),
        };
        slot::write_slot_file(&paths, "a", &slot).expect("write");
        let access = seat_access(&paths, "a");
        assert_eq!(
            access.read_trees,
            vec![paths.session_dir("a"), paths.tools_dir()]
        );
        assert_eq!(access.daemon_port, Some(4123));
        assert_eq!(access.node, Some(PathBuf::from("/opt/node/bin/node")));
        assert_eq!(access.shim_dir, paths.shim_dir("a"));
        let other = seat_access(&paths, "b");
        assert_eq!((other.daemon_port, other.node), (None, None));
        assert!(!other.read_trees.contains(&paths.session_dir("a")));
    }
}
