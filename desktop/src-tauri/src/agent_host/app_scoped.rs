//! Bootstrap a persistent host; the desktop never owns or stops it on quit.
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use beekeeper_host_core::layout;

/// Serialize bootstrap attempts without retaining host process ownership.
static BOOTSTRAP: Mutex<()> = Mutex::new(());

/// How long a freshly started host gets to bind its control socket.
const SOCKET_WAIT: Duration = Duration::from_secs(10);

/// Ensure the persistent host is reachable. The app does not own its lifetime.
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
    let _bootstrap = BOOTSTRAP.lock().map_err(|_| "host bootstrap lock failed")?;
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
        .env(layout::HOME_VAR, &home)
        .env_remove("BEEKEEPER_PRIVATE_KEY")
        .env_remove("NOSTR_PRIVATE_KEY")
        .env_remove("BEEKEEPER_AUTH_TAG")
        .env_remove("BUZZ_PRIVATE_KEY")
        .env_remove("BUZZ_AUTH_TAG")
        .env_remove("BEEKEEPER_CSP_PRIVATE_KEY")
        .env_remove("BEEKEEPER_ACP_PRIVATE_KEY")
        .env_remove("BUZZ_ACP_PRIVATE_KEY")
        .stdin(Stdio::null())
        .stdout(
            log.try_clone()
                .map_err(|error| format!("could not share the host log: {error}"))?,
        )
        .stderr(log)
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", binary.display()))?;
    // Dropping the bootstrap handle neither kills nor waits for the host.
    drop(child);
    let deadline = Instant::now() + SOCKET_WAIT;
    while Instant::now() < deadline && !socket_answers(&socket) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if !socket_answers(&socket) {
        return Err("Persistent host did not become reachable; inspect its restricted log".into());
    }
    Ok(())
}

/// Whether a host is listening on `socket`.
fn socket_answers(socket: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(socket).is_ok()
}
