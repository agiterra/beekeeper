//! The agent host for as long as this app runs, when nothing starts it at login.
//!
//! The host is the provider's only supervisor (`docs/agent-host.md`). Before
//! this module, a machine whose host was not registered at login — the person
//! declined *Keep your agents running?*, or had not answered it yet — had no
//! host and therefore no provider at all, while the prompt told them "Right
//! now your coding sessions end when you quit Beekeeper" (ledger 302(a)).
//! This makes that sentence true: without a login registration, the app runs
//! `beekeeper-host run` as its own child and stops it when it quits.
//!
//! A registration always wins. Saying yes stops this child before the login
//! item is written, so two hosts never race for one socket; a registered host
//! is launchd's to run and this module never starts one beside it.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use beekeeper_host_core::layout;

/// The host this app started, if it is running one.
static CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// How long a freshly started host gets to bind its control socket.
const SOCKET_WAIT: Duration = Duration::from_secs(10);

/// How long a host gets to stop its provider after SIGTERM. The provider
/// flushes its outbox on the way down; 14 s was measured on 2026-10-01.
const STOP_WAIT: Duration = Duration::from_secs(30);

/// Start the host as this app's child, unless a login registration owns it or
/// one is already running.
///
/// Blocking — it waits for the socket — so async callers go through
/// `spawn_blocking`.
///
/// # Errors
/// The host binary could not be found or started; disclosed by the caller,
/// whose status read carries the resulting reachability.
pub(crate) fn ensure_running() -> Result<(), String> {
    let home = layout::home_dir()?;
    let instance = super::instance();
    if super::autostart::status().installed {
        return Ok(());
    }
    if !layout::host_config_path(&home, instance).exists() {
        // Not commissioned: `run` would refuse. Commissioning calls back here.
        return Ok(());
    }
    let mut held = CHILD
        .lock()
        .map_err(|_| "the app's host lock is poisoned")?;
    if let Some(child) = held.as_mut() {
        if matches!(child.try_wait(), Ok(None)) {
            return Ok(());
        }
        *held = None;
    }
    let socket = layout::host_socket_path(&home, instance);
    if socket_answers(&socket) {
        // A host this app did not start — from a terminal, or a registration
        // removed while it ran. It is running; leave it be.
        return Ok(());
    }
    let binary = super::autostart::resolve_host_binary()
        .ok_or("beekeeper-host was not found beside this app")?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(layout::host_log_path(&home, instance))
        .map_err(|error| format!("could not open the host log: {error}"))?;
    let child = Command::new(&binary)
        .arg("run")
        .env(layout::INSTANCE_VAR, instance.namespace_value())
        .stdin(Stdio::null())
        .stdout(
            log.try_clone()
                .map_err(|error| format!("could not share the host log: {error}"))?,
        )
        .stderr(log)
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", binary.display()))?;
    *held = Some(child);
    drop(held);
    let deadline = Instant::now() + SOCKET_WAIT;
    while Instant::now() < deadline && !socket_answers(&socket) {
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Stop the host this app started, if any, and wait for it to stop its
/// provider. A host the app did not start is never touched.
pub(crate) fn stop() {
    let Ok(mut held) = CHILD.lock() else { return };
    let Some(mut child) = held.take() else { return };
    if !matches!(child.try_wait(), Ok(None)) {
        return;
    }
    // SIGTERM, which the host answers by stopping its provider cleanly. `kill`
    // rather than a signal call: this crate takes no `unsafe`.
    let _ = Command::new("/bin/kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let deadline = Instant::now() + STOP_WAIT;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Whether a host is listening on `socket`.
fn socket_answers(socket: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(socket).is_ok()
}
