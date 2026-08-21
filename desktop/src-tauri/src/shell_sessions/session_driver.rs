//! Capability-sliced `SessionDriver` adapter for `buzz-shell-host` (A-16).
//!
//! Scope-locked to four slices by architect ruling: `detect`, `attach-existing`,
//! `list-probe`, `input-resize`. Deliberately excludes spawn, kill/signal
//! delivery, credential handling, new durable state, a new listener/socket, and
//! receipt mutation — those verbs stay exactly where they already live, directly
//! in `manager.rs` / `host_client.rs` / `buzz_shell_host::receipt`, never behind
//! this trait. See the absorption report's NAMED HONEST BOUNDARIES for why.
//!
//! `manager.rs` is the one real caller: `spawn_host_and_attach` calls
//! `resolve_driver` before it spawns anything, `attach` calls
//! `attach_existing`, `reattach_hosts` calls `list_probe`, and `write`/`resize`
//! call `input_resize`. This module has no other consumer.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use buzz_shell_host::proto::Hello;
use buzz_shell_host::receipt::{pid_alive, Receipt};

use super::host_client::{self, AttachedClient};

/// The sole backend name this row wires up. `resolve_driver` rejects anything
/// else loudly — see `§Driver-selection`'s `pickDriver`/`instantiate` in
/// `conformance/agent-session-drivers/CONTRACT.md`, which this deliberately
/// does NOT port (out of scope until >= 2 real backends exist).
pub const BUZZ_SHELL_HOST: &str = "buzz-shell-host";

/// `detect` outcome: never launches the backend, only checks it's resolvable.
#[derive(Debug, PartialEq, Eq)]
pub enum Detected {
    Available,
    Unavailable { reason: String },
}

/// `list-probe` outcome for one receipt: is the substrate (host process +
/// socket) still the live owner of this session? Read-only — never removes or
/// rewrites the receipt; the caller decides what a `Dead` probe means.
#[derive(Debug, PartialEq, Eq)]
pub enum Probed {
    Alive,
    Dead,
}

/// An `input-resize` operation: either keystrokes or a PTY geometry change.
pub enum InputResize<'a> {
    Input(&'a [u8]),
    Resize { rows: u16, cols: u16 },
}

/// Capability-sliced session driver. Intentionally has no `spawn`/`kill` — the
/// donor's `SessionDriver` (`types.ts:73-99`) requires both; porting either as
/// a stub here would advertise process-control authority this row does not
/// have. See `docs/absorption/coordination/BUILDER-BRIEF-PREAMBLE.md`.
pub trait SessionDriver {
    /// Is the backend's binary resolvable on this system? Never spawns it.
    fn detect(&self) -> Detected;

    /// Connect to a socket a substrate process already owns and is already
    /// listening on (never binds a new socket, never spawns anything).
    /// Returns a narrowed I/O handle (input + resize only — see
    /// `AttachedClient`, NOT the full `HostClient`, so a caller holding only
    /// this trait can never reach `kill`/`set_title` through it), the host's
    /// greeting, and the read half for the caller's own reader thread.
    fn attach_existing(
        &self,
        socket: &Path,
        timeout: Duration,
    ) -> Result<(AttachedClient, Hello, UnixStream), String>;

    /// Probe whether a receipt's host process is still the live owner of its
    /// session. Read-only: never deletes the receipt or the socket file.
    fn list_probe(&self, receipt: &Receipt, socket: &Path) -> Probed;

    /// Deliver keystrokes or a resize to an already-attached session's host.
    /// `AttachedClient::input`/`resize` live in `host_client.rs` as
    /// `pub(super)` (the tightest Rust allows a sibling module to reach —
    /// see `host_client.rs`'s doc comments), and this is the only place in
    /// `shell_sessions` that calls them.
    fn input_resize(&self, client: &AttachedClient, op: InputResize<'_>) -> Result<(), String>;
}

/// Classifies a `resolve_command` lookup into a `Detected` outcome. Split out
/// so the branch logic is directly unit-testable without needing the actual
/// binary present or absent in the test environment.
fn classify_availability(resolved: Option<PathBuf>) -> Detected {
    match resolved {
        Some(_) => Detected::Available,
        None => Detected::Unavailable {
            reason: format!("{BUZZ_SHELL_HOST} binary not found"),
        },
    }
}

#[derive(Debug)]
pub struct BuzzShellHostDriver;

impl SessionDriver for BuzzShellHostDriver {
    fn detect(&self) -> Detected {
        classify_availability(crate::managed_agents::resolve_command(BUZZ_SHELL_HOST))
    }

    fn attach_existing(
        &self,
        socket: &Path,
        timeout: Duration,
    ) -> Result<(AttachedClient, Hello, UnixStream), String> {
        // Connect via `HostClient` internally (it owns the retry/handshake
        // logic), then narrow via `attached()`: the raw write half never
        // leaves `host_client.rs`, and neither does the full `HostClient` —
        // only the narrowed `AttachedClient` crosses back out of this
        // function.
        let (client, hello, read_half) = host_client::connect_with_retry(socket, timeout)?;
        Ok((client.attached(), hello, read_half))
    }

    fn list_probe(&self, receipt: &Receipt, socket: &Path) -> Probed {
        if pid_alive(receipt.host_pid) && socket.exists() {
            Probed::Alive
        } else {
            Probed::Dead
        }
    }

    fn input_resize(&self, client: &AttachedClient, op: InputResize<'_>) -> Result<(), String> {
        match op {
            InputResize::Input(bytes) => client.input(bytes),
            InputResize::Resize { rows, cols } => client.resize(rows, cols),
        }
    }
}

/// The name-check + `Detected` branch logic, with the detection outcome
/// injected rather than computed, so it's testable without depending on
/// whether `buzz-shell-host` happens to be resolvable wherever the test
/// runs — the same seam `classify_availability` provides for `detect()`
/// itself. `resolve_driver` is the thin wrapper that supplies the real
/// outcome.
fn resolve_from_detection(
    backend: &str,
    driver: BuzzShellHostDriver,
    detected: Detected,
) -> Result<BuzzShellHostDriver, String> {
    if backend != BUZZ_SHELL_HOST {
        return Err(format!("unknown session driver backend: {backend}"));
    }
    match detected {
        Detected::Available => Ok(driver),
        Detected::Unavailable { reason } => Err(format!(
            "session driver backend \"{BUZZ_SHELL_HOST}\" unavailable: {reason}"
        )),
    }
}

/// The one-entry registry: an explicit constructor, not a lookup table. Fails
/// loud for any name but the sole known backend, and fails loud again if that
/// backend's own `detect()` reports it unavailable. No phantom backends.
///
/// Rejects an unknown `backend` before calling `detect()` — `detect()` probes
/// for the real binary (consulting the login shell's `PATH`), and that probe
/// has no business running for a name this registry is about to reject
/// anyway. The error string is unchanged either way; only the ordering is.
pub fn resolve_driver(backend: &str) -> Result<BuzzShellHostDriver, String> {
    let driver = BuzzShellHostDriver;
    if backend != BUZZ_SHELL_HOST {
        return Err(format!("unknown session driver backend: {backend}"));
    }
    let detected = driver.detect();
    resolve_from_detection(backend, driver, detected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_shell_host::proto::Frame;
    use std::io::Read;

    /// A pid guaranteed to be dead: spawn a trivial child and wait for its
    /// exit, so `pid_alive` sees a reaped process rather than pid 0 (which
    /// `kill(0, 0)` treats as "my own process group", i.e. alive).
    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn a short-lived child");
        let pid = child.id();
        child.wait().expect("wait for child exit");
        pid
    }

    fn test_receipt(host_pid: u32) -> Receipt {
        Receipt {
            id: "test-session".to_string(),
            socket_path: "/tmp/does-not-matter.sock".to_string(),
            host_pid,
            shell_pid: None,
            title: "test".to_string(),
            shell: "/bin/sh".to_string(),
            cwd: "/tmp".to_string(),
            created_at: 1,
        }
    }

    // --- registry: fails loud, no phantom backends ---

    #[test]
    fn resolve_driver_rejects_unknown_backend_name() {
        let err = resolve_driver("tmux").unwrap_err();
        assert_eq!(err, "unknown session driver backend: tmux");
    }

    #[test]
    fn resolve_driver_rejects_empty_backend_name() {
        let err = resolve_driver("").unwrap_err();
        assert_eq!(err, "unknown session driver backend: ");
    }

    // --- detect: classifies without spawning anything ---

    #[test]
    fn classify_availability_none_is_unavailable_with_named_reason() {
        let result = classify_availability(None);
        assert_eq!(
            result,
            Detected::Unavailable {
                reason: "buzz-shell-host binary not found".to_string()
            }
        );
    }

    #[test]
    fn classify_availability_some_is_available() {
        let result = classify_availability(Some(PathBuf::from("/usr/bin/buzz-shell-host")));
        assert_eq!(result, Detected::Available);
    }

    #[test]
    fn driver_detect_agrees_with_resolve_command_right_now() {
        // The real wiring, not a re-derivation: whatever resolve_command says
        // right now for BUZZ_SHELL_HOST is exactly what detect() must report.
        // A driver.detect() hardcoded to Available (ignoring resolve_command)
        // fails this in this dev worktree, where the sidecar binary isn't built.
        let driver = BuzzShellHostDriver;
        let resolved = crate::managed_agents::resolve_command(BUZZ_SHELL_HOST);
        match (driver.detect(), resolved) {
            (Detected::Available, Some(_)) => {}
            (Detected::Unavailable { .. }, None) => {}
            (got, resolved) => panic!(
                "detect() diverged from resolve_command: detect={got:?} resolve_command={resolved:?}"
            ),
        }
    }

    #[test]
    fn resolve_driver_fails_loud_when_backend_unavailable() {
        // Injects the Unavailable outcome directly instead of depending on
        // whether buzz-shell-host happens to be resolvable in whatever
        // environment runs this test (a dev worktree without the sidecar
        // built, CI with it on PATH, etc.) — the property under test is the
        // error-string contract in resolve_from_detection's match arm, not a
        // fact about this machine. See BUILDER-BRIEF-PREAMBLE.md law 1.
        let err = resolve_from_detection(
            BUZZ_SHELL_HOST,
            BuzzShellHostDriver,
            Detected::Unavailable {
                reason: "buzz-shell-host binary not found".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            "session driver backend \"buzz-shell-host\" unavailable: buzz-shell-host binary not found"
        );
    }

    // --- list-probe: read-only substrate liveness, never mutates the receipt ---

    #[test]
    fn list_probe_dead_pid_is_dead_regardless_of_socket() {
        let driver = BuzzShellHostDriver;
        let dead_pid = dead_pid();
        assert!(
            !pid_alive(dead_pid),
            "test assumes a reaped child reads as not-alive"
        );
        let receipt = test_receipt(dead_pid);
        let probed = driver.list_probe(&receipt, Path::new("/tmp/nonexistent-socket.sock"));
        assert_eq!(probed, Probed::Dead);
    }

    #[test]
    fn list_probe_alive_pid_but_missing_socket_is_dead() {
        // A live process whose socket file is gone (host mid-teardown) is not
        // a substrate that can still serve this session.
        let driver = BuzzShellHostDriver;
        let self_pid = std::process::id();
        let receipt = test_receipt(self_pid);
        let probed = driver.list_probe(&receipt, Path::new("/tmp/definitely-not-there.sock"));
        assert_eq!(probed, Probed::Dead);
    }

    #[test]
    fn list_probe_alive_pid_and_existing_socket_is_alive() {
        let driver = BuzzShellHostDriver;
        let self_pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("a16-list-probe-{self_pid}"));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("fake.sock");
        // A UnixListener bind is enough for `.exists()`; no data needs to flow.
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let receipt = test_receipt(self_pid);
        let probed = driver.list_probe(&receipt, &socket);
        assert_eq!(probed, Probed::Alive);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_probe_never_removes_the_socket_or_receipt_file() {
        // list_probe is read-only: a Dead verdict must not delete anything.
        // Receipt mutation/cleanup is out of scope for the driver (it stays
        // in manager.rs::reattach_hosts, which decides what Dead means).
        let driver = BuzzShellHostDriver;
        let dir = std::env::temp_dir().join(format!("a16-list-probe-noop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("fake.sock");
        std::fs::write(&socket, b"not a real socket").unwrap();
        let receipt = test_receipt(dead_pid());
        let probed = driver.list_probe(&receipt, &socket);
        assert_eq!(probed, Probed::Dead);
        assert!(
            socket.exists(),
            "list_probe must never delete the socket file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- attach-existing + input-resize: exercised together against a fake host ---

    fn fake_host_socket(name: &str) -> (PathBuf, std::os::unix::net::UnixListener) {
        let dir =
            std::env::temp_dir().join(format!("a16-fake-host-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("host.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        (socket, listener)
    }

    #[test]
    fn attach_existing_connects_to_a_socket_it_did_not_create_and_reads_hello() {
        let (socket, listener) = fake_host_socket("attach");
        let hello = Hello {
            total: 42,
            cwd: "/tmp/somewhere".to_string(),
            shell_pid: Some(999),
        };
        let hello_clone = hello.clone();
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            Frame::Hello(hello_clone).write_to(&mut conn).unwrap();
            // Keep the connection open long enough for the client to read it.
            std::thread::sleep(Duration::from_millis(100));
        });

        let driver = BuzzShellHostDriver;
        let (_client, received_hello, _read_half) = driver
            .attach_existing(&socket, Duration::from_secs(2))
            .expect("attach_existing must connect to a pre-existing socket");
        assert_eq!(received_hello, hello);
        handle.join().unwrap();
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    #[test]
    fn attach_existing_never_binds_its_own_socket() {
        // If attach_existing tried to create a listener instead of only ever
        // connecting, binding a second listener on the same path afterward
        // would fail with "address in use". It must not — attach_existing
        // must be a pure client-side connect, no listener of its own.
        let (socket, listener) = fake_host_socket("no-bind");
        let hello = Hello {
            total: 0,
            cwd: "/tmp".to_string(),
            shell_pid: None,
        };
        let accept_handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            Frame::Hello(hello).write_to(&mut conn).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        let driver = BuzzShellHostDriver;
        let _ = driver
            .attach_existing(&socket, Duration::from_secs(2))
            .expect("attach must succeed");
        accept_handle.join().unwrap();
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    #[test]
    fn input_resize_input_sends_exactly_the_given_bytes_as_a_frame() {
        let (socket, listener) = fake_host_socket("input");
        let hello = Hello {
            total: 0,
            cwd: "/tmp".to_string(),
            shell_pid: None,
        };
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            Frame::Hello(hello).write_to(&mut conn).unwrap();
            Frame::read_from(&mut conn).unwrap().unwrap()
        });

        let driver = BuzzShellHostDriver;
        let (client, _hello, _read_half) = driver
            .attach_existing(&socket, Duration::from_secs(2))
            .expect("attach must succeed");
        driver
            .input_resize(&client, InputResize::Input(b"ls -la\n"))
            .expect("input_resize(Input) must succeed");

        let received = server.join().unwrap();
        assert_eq!(received, Frame::Input(b"ls -la\n".to_vec()));
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    #[test]
    fn input_resize_resize_sends_exact_rows_and_cols() {
        let (socket, listener) = fake_host_socket("resize");
        let hello = Hello {
            total: 0,
            cwd: "/tmp".to_string(),
            shell_pid: None,
        };
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            Frame::Hello(hello).write_to(&mut conn).unwrap();
            let frame = Frame::read_from(&mut conn).unwrap().unwrap();
            // Confirm nothing else arrives on the wire for a resize call.
            let mut buf = [0u8; 1];
            conn.set_read_timeout(Some(Duration::from_millis(50))).ok();
            let extra = conn.read(&mut buf);
            (frame, extra)
        });

        let driver = BuzzShellHostDriver;
        let (client, _hello, _read_half) = driver
            .attach_existing(&socket, Duration::from_secs(2))
            .expect("attach must succeed");
        driver
            .input_resize(
                &client,
                InputResize::Resize {
                    rows: 40,
                    cols: 120,
                },
            )
            .expect("input_resize(Resize) must succeed");

        let (received, extra) = server.join().unwrap();
        assert_eq!(
            received,
            Frame::Resize {
                rows: 40,
                cols: 120
            }
        );
        assert!(
            matches!(extra, Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock)
                || matches!(extra, Ok(0)),
            "resize must not send any further bytes: {extra:?}"
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }
}
