//! Client side of the detached shell host: the desktop app's connection to a
//! `buzz-shell-host` process over its Unix socket.
//!
//! A `HostClient` owns the write half (kill, rename/receipt-mutation). The
//! read half is handed back to the manager, which runs a thread reading
//! `Output`/`Synced`/`Exit` frames into the session's `SharedState` — the
//! same downstream flow the old local-PTY reader thread had, only the source
//! is a socket now.
//!
//! `HostClient` deliberately does NOT expose keystrokes/resize — that authority
//! lives only in `AttachedClient` below, whose `input`/`resize` are the sole
//! way `session_driver::SessionDriver::input_resize` reaches the wire. Both
//! types share one underlying `Arc<Mutex<UnixStream>>`, but that raw write
//! half is a private field on both structs and this module never hands it out
//! — only the narrowed `AttachedClient` (via `HostClient::attached`) or the
//! full `HostClient` (via `AttachedClient::full_client`) cross the module
//! boundary. `HostClient` itself is `pub(super)`-reachable so `manager.rs`
//! (which already owned kill/rename before the driver existed) can still get
//! one.

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use beekeeper_shell_host::proto::{Frame, Hello};

/// A connection to a running shell host. Cheap to clone-share via the inner Arc.
#[derive(Clone)]
pub struct HostClient {
    write_half: Arc<Mutex<UnixStream>>,
}

impl HostClient {
    /// Terminate the shell (an explicit close, distinct from just disconnecting).
    pub fn kill(&self) -> Result<(), String> {
        self.send(Frame::Kill)
    }

    /// Rename the session. The host persists the new title into its receipt and
    /// metadata so the rename survives an app restart or reboot.
    pub fn set_title(&self, title: &str) -> Result<(), String> {
        self.send(Frame::SetTitle(title.to_string()))
    }

    /// Narrow to an `AttachedClient` sharing this same connection: the write
    /// half itself never leaves this module, only the narrowed handle does.
    /// `pub(super)`: `session_driver::BuzzShellHostDriver::attach_existing`
    /// (the sole caller) is a sibling module, not a child, so `pub(super)` —
    /// visible to all of `shell_sessions` — is the narrowest visibility Rust
    /// offers here; `pub(in path)` requires `path` to be an ancestor of this
    /// module, which a sibling can't name. See the module doc / the driver's
    /// report for why nothing else in `shell_sessions` calls this.
    pub(super) fn attached(&self) -> AttachedClient {
        AttachedClient {
            write_half: self.write_half.clone(),
        }
    }

    fn send(&self, frame: Frame) -> Result<(), String> {
        let mut w = self
            .write_half
            .lock()
            .map_err(|_| "shell-host client lock poisoned".to_string())?;
        frame
            .write_to(&mut *w)
            .map_err(|e| format!("failed to send to shell host: {e}"))
    }
}

/// A driver-attached session's I/O handle: everything `attach_existing` hands
/// back to a `SessionDriver` caller, and nothing more. Shares the same
/// underlying write half a `HostClient` would, but `input`/`resize` are the
/// only sends this type permits — no `kill`, no `set_title`. `full_client` is
/// the one deliberate escape hatch, `pub(super)` so only `manager.rs` — which
/// already had kill/rename authority before this driver existed — can use it.
#[derive(Clone)]
pub struct AttachedClient {
    write_half: Arc<Mutex<UnixStream>>,
}

impl AttachedClient {
    fn send(&self, frame: Frame) -> Result<(), String> {
        let mut w = self
            .write_half
            .lock()
            .map_err(|_| "shell-host client lock poisoned".to_string())?;
        frame
            .write_to(&mut *w)
            .map_err(|e| format!("failed to send to shell host: {e}"))
    }

    /// `pub(super)`: the only caller is
    /// `session_driver::SessionDriver::input_resize`, in the sibling
    /// `session_driver.rs` module — see `HostClient::attached` for why
    /// `pub(super)` (not a tighter `pub(in path)`) is the narrowest Rust
    /// allows across sibling modules. No other file in `shell_sessions`
    /// calls this today (mechanically checked by grep, see the report).
    pub(super) fn input(&self, bytes: &[u8]) -> Result<(), String> {
        self.send(Frame::Input(bytes.to_vec()))
    }

    /// See `input`'s doc for the visibility rationale.
    pub(super) fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        self.send(Frame::Resize { rows, cols })
    }

    /// Reconstruct the full `HostClient` (kill, rename) sharing this same
    /// connection. `pub(super)`: reserved for `manager.rs`, which held that
    /// authority before the driver existed — not a new grant, just relocated
    /// so `attach_existing`'s return type can't carry it implicitly.
    pub(super) fn full_client(&self) -> HostClient {
        HostClient {
            write_half: self.write_half.clone(),
        }
    }
}

/// Connect to a host socket and read its greeting. Returns the client (write
/// side), the `Hello`, and the read half for the manager's reader thread.
pub fn connect(socket_path: &Path) -> Result<(HostClient, Hello, UnixStream), String> {
    let stream = UnixStream::connect(socket_path).map_err(|e| {
        format!(
            "failed to connect to shell host {}: {e}",
            socket_path.display()
        )
    })?;
    let write_half = stream
        .try_clone()
        .map_err(|e| format!("failed to clone shell-host socket: {e}"))?;
    let mut read_half = stream;
    let hello = match Frame::read_from(&mut read_half) {
        Ok(Some(Frame::Hello(h))) => h,
        Ok(Some(_)) => return Err("shell host did not greet with Hello".to_string()),
        Ok(None) => return Err("shell host closed before greeting".to_string()),
        Err(e) => return Err(format!("failed to read shell-host greeting: {e}")),
    };
    Ok((
        HostClient {
            write_half: Arc::new(Mutex::new(write_half)),
        },
        hello,
        read_half,
    ))
}

/// Wait (briefly) for a host socket to appear after spawning the host, then
/// connect. The host binds its socket within the first few milliseconds; poll
/// up to `timeout`.
pub fn connect_with_retry(
    socket_path: &Path,
    timeout: Duration,
) -> Result<(HostClient, Hello, UnixStream), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if socket_path.exists() {
            match connect(socket_path) {
                Ok(triple) => return Ok(triple),
                // The socket file can exist a beat before it accepts; retry.
                Err(_) if Instant::now() < deadline => {}
                Err(e) => return Err(e),
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "shell host socket {} did not become ready in time",
                socket_path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
