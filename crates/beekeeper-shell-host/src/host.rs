//! The detached PTY host: owns a shell in a PTY and serves attaching clients.
//!
//! Lifecycle: `setsid()` to detach from the app's session (so it survives the
//! app quitting), open a PTY and spawn the shell, write a receipt, then serve a
//! Unix socket. Each client that connects gets a `Hello` + the retained
//! scrollback, then the live output stream; its input/resize/kill frames drive
//! the PTY. The process exits (cleaning up socket, receipt, and persisted
//! files) only when the shell itself exits or a client sends `Kill` — not when
//! clients merely disconnect, which is the whole point.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;

use crate::proto::{Frame, Hello};
use crate::receipt::Receipt;

/// Options the binary parses and hands to [`run`].
pub struct HostOptions {
    pub id: String,
    pub socket_path: PathBuf,
    /// Directory for the reattach receipt (`<hosts_dir>/<id>.json`).
    pub hosts_dir: PathBuf,
    /// Directory for the reboot-fallback history (`<persist_dir>/<id>.{json,log}`).
    pub persist_dir: PathBuf,
    pub cwd: String,
    pub shell: String,
    pub title: String,
    pub rows: u16,
    pub cols: u16,
    pub created_at: u64,
}

/// Cap on retained scrollback, matching the app's in-memory + on-disk caps.
const SCROLLBACK_CAP: usize = 1024 * 1024;
/// Chunk size for replaying scrollback so one frame stays well under the max.
const REPLAY_CHUNK: usize = 256 * 1024;

struct Shared {
    scrollback: Vec<u8>,
    total: u64,
    cwd: String,
    /// The session title. Mutable at runtime via a client `SetTitle` frame; the
    /// checkpoint + `SetTitle` handler persist it into the receipt and metadata.
    title: String,
    shell_pid: Option<u32>,
    clients: Vec<UnixStream>,
}

/// The static per-host paths + identity the checkpoint thread and client input
/// readers need to persist state (history, cwd, title) to disk.
#[derive(Clone)]
struct HostPaths {
    id: String,
    persist_dir: PathBuf,
    hosts_dir: PathBuf,
    shell: String,
    created_at: u64,
}

/// Daemonize so this process fully outlives the spawning app: double-fork +
/// `setsid` (the app's direct child exits immediately and is reparented to
/// init, so no zombie lingers under the app), and ignore SIGHUP. Called before
/// any threads are spawned.
fn detach() {
    // SAFETY: fork/setsid/signal/_exit are async-signal-safe libc calls with no
    // memory arguments, invoked here before any threads exist.
    unsafe {
        // First fork: the parent (app's direct child) exits so we're not a
        // process-group leader when we call setsid.
        match libc::fork() {
            -1 => {} // fork failed; continue best-effort in-process
            0 => {}  // child continues
            _ => libc::_exit(0),
        }
        libc::setsid();
        // Second fork: guarantee we can never reacquire a controlling terminal.
        match libc::fork() {
            -1 => {}
            0 => {}
            _ => libc::_exit(0),
        }
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
}

/// Run the host until the shell exits or a client kills it. Returns the process
/// exit code.
pub fn run(opts: HostOptions) -> i32 {
    detach();

    let pty = native_pty_system();
    let pair = match pty.openpty(PtySize {
        rows: opts.rows,
        cols: opts.cols,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("beekeeper-shell-host: openpty failed: {e}");
            return 1;
        }
    };

    let mut builder = CommandBuilder::new(&opts.shell);
    builder.cwd(&opts.cwd);
    builder.env("TERM", "xterm-256color");
    let mut child = match pair.slave.spawn_command(builder) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("beekeeper-shell-host: spawn {} failed: {e}", opts.shell);
            return 1;
        }
    };
    drop(pair.slave);

    // Seed scrollback from on-disk history for this id, if any — this is what
    // makes a reboot-fallback resume show the old output above the new prompt.
    // A live host started fresh (new id) finds nothing and seeds empty.
    let seed = read_seed(&opts.persist_dir, &opts.id);
    let seed_total = seed.len() as u64;

    let shell_pid = child.process_id();
    let killer = Arc::new(Mutex::new(child.clone_killer()));
    let mut reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("beekeeper-shell-host: clone reader failed: {e}");
            return 1;
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("beekeeper-shell-host: take writer failed: {e}");
            return 1;
        }
    };
    let writer = Arc::new(Mutex::new(writer));
    let master = Arc::new(Mutex::new(pair.master));

    let shared = Arc::new(Mutex::new(Shared {
        scrollback: seed,
        total: seed_total,
        cwd: opts.cwd.clone(),
        title: opts.title.clone(),
        shell_pid,
        clients: Vec::new(),
    }));

    let paths = Arc::new(HostPaths {
        id: opts.id.clone(),
        persist_dir: opts.persist_dir.clone(),
        hosts_dir: opts.hosts_dir.clone(),
        shell: opts.shell.clone(),
        created_at: opts.created_at,
    });

    // Write the reattach receipt now, before serving, so an app relaunch during
    // startup can already find us.
    let receipt = Receipt {
        id: opts.id.clone(),
        socket_path: opts.socket_path.to_string_lossy().into_owned(),
        host_pid: std::process::id(),
        shell_pid,
        title: opts.title.clone(),
        shell: opts.shell.clone(),
        cwd: opts.cwd.clone(),
        created_at: opts.created_at,
    };
    if let Err(e) = receipt.write(&opts.hosts_dir) {
        eprintln!("beekeeper-shell-host: failed to write receipt: {e}");
    }

    let listener = match bind_socket(&opts.socket_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "beekeeper-shell-host: bind {} failed: {e}",
                opts.socket_path.display()
            );
            return 1;
        }
    };

    // PTY reader thread: pump output into scrollback + fan out to clients.
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        if let Ok(mut st) = shared.lock() {
                            st.scrollback.extend_from_slice(chunk);
                            st.total += n as u64;
                            if st.scrollback.len() > SCROLLBACK_CAP {
                                let excess = st.scrollback.len() - SCROLLBACK_CAP;
                                st.scrollback.drain(..excess);
                            }
                            let frame = Frame::Output(chunk.to_vec());
                            fan_out(&mut st.clients, &frame);
                        }
                    }
                }
            }
            // Shell exited: tell clients, checkpoint nothing more, clean up.
            if let Ok(mut st) = shared.lock() {
                fan_out(&mut st.clients, &Frame::Exit);
            }
            let _ = child.wait();
        });
    }

    // Checkpoint thread: refresh cwd + persist history/title for recovery.
    {
        let shared = shared.clone();
        let paths = paths.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(10));
            let (scrollback, cwd, title) = {
                let Ok(mut st) = shared.lock() else { return };
                let cwd = live_cwd(st.shell_pid, &st.cwd);
                st.cwd = cwd.clone();
                (st.scrollback.clone(), cwd, st.title.clone())
            };
            persist_state(&paths, &title, &cwd, &scrollback);
        });
    }

    // Accept loop: serve clients until the shell exits. `serve` polls in
    // non-blocking mode so the host tears down after the shell dies even with
    // no client attached.
    serve(
        &listener, &shared, &paths, &writer, &master, &killer, shell_pid,
    );

    // Teardown: shell has exited or we were killed. Remove socket, receipt, and
    // persisted files so a relaunch does not treat this as restorable.
    let _ = std::fs::remove_file(&opts.socket_path);
    Receipt::remove(&opts.hosts_dir, &opts.id);
    remove_persist(&opts.persist_dir, &opts.id);
    0
}

/// Accept clients until the shell process is gone.
fn serve(
    listener: &UnixListener,
    shared: &Arc<Mutex<Shared>>,
    paths: &Arc<HostPaths>,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
    master: &Arc<Mutex<Box<dyn MasterPty + Send>>>,
    killer: &Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    shell_pid: Option<u32>,
) {
    listener
        .set_nonblocking(true)
        .unwrap_or_else(|e| eprintln!("beekeeper-shell-host: set_nonblocking failed: {e}"));
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                register_and_serve_client(stream, shared, paths, writer, master, killer);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No pending connection. Exit once the shell is gone.
                if let Some(pid) = shell_pid {
                    if !crate::receipt::pid_alive(pid) {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("beekeeper-shell-host: accept failed: {e}");
                return;
            }
        }
    }
}

/// Send the greeting + scrollback snapshot atomically (under the state lock so
/// no live bytes are missed or duplicated), register the client for fan-out,
/// and spawn its input reader.
fn register_and_serve_client(
    stream: UnixStream,
    shared: &Arc<Mutex<Shared>>,
    paths: &Arc<HostPaths>,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
    master: &Arc<Mutex<Box<dyn MasterPty + Send>>>,
    killer: &Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
) {
    // `listener` runs non-blocking so its accept loop can poll for the shell
    // exiting; on macOS that flag is inherited by the connection `accept()`
    // hands back. Client I/O must block: the per-client reader thread below
    // calls `Frame::read_from` expecting it to block until a frame arrives, and
    // `read_exact_or_eof` treats `WouldBlock` as a fatal error — with the
    // inherited flag, the reader's first read fires before the client has sent
    // anything, hits `WouldBlock`, and the thread exits immediately, silently
    // dropping every Input/Resize/Kill/SetTitle frame the client ever sends.
    if let Err(e) = stream.set_nonblocking(false) {
        eprintln!("beekeeper-shell-host: failed to clear client nonblocking flag: {e}");
    }

    let mut write_half = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("beekeeper-shell-host: client clone failed: {e}");
            return;
        }
    };
    {
        let Ok(mut st) = shared.lock() else { return };
        let hello = Frame::Hello(Hello {
            total: st.total,
            cwd: st.cwd.clone(),
            shell_pid: st.shell_pid,
        });
        if hello.write_to(&mut write_half).is_err() {
            return;
        }
        for chunk in st.scrollback.chunks(REPLAY_CHUNK) {
            if Frame::Output(chunk.to_vec())
                .write_to(&mut write_half)
                .is_err()
            {
                return;
            }
        }
        // Boundary between replayed history and live output.
        if Frame::Synced.write_to(&mut write_half).is_err() {
            return;
        }
        st.clients.push(write_half);
    }

    // Per-client input reader.
    let mut read_half = stream;
    let shared = shared.clone();
    let paths = paths.clone();
    let writer = writer.clone();
    let master = master.clone();
    let killer = killer.clone();
    std::thread::spawn(move || loop {
        match Frame::read_from(&mut read_half) {
            Ok(Some(Frame::Input(bytes))) => {
                if let Ok(mut w) = writer.lock() {
                    let _ = w.write_all(&bytes).and_then(|()| w.flush());
                }
            }
            Ok(Some(Frame::SetTitle(title))) => {
                // Update the live title and persist it immediately so a rename
                // survives an app restart (reattach reads the receipt) and a
                // reboot (restore reads the metadata), not just the next tick.
                let snapshot = shared.lock().ok().map(|mut st| {
                    st.title = title.clone();
                    (st.cwd.clone(), st.scrollback.clone())
                });
                if let Some((cwd, scrollback)) = snapshot {
                    persist_state(&paths, &title, &cwd, &scrollback);
                }
            }
            Ok(Some(Frame::Resize { rows, cols })) => {
                if let Ok(m) = master.lock() {
                    let _ = m.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
            }
            Ok(Some(Frame::Kill)) => {
                if let Ok(mut k) = killer.lock() {
                    let _ = k.kill();
                }
                return;
            }
            // Other frame kinds are host→client only; ignore.
            Ok(Some(_)) => {}
            // Clean disconnect or error: the client detached. Leave the shell
            // running — that is the point of the host.
            Ok(None) | Err(_) => return,
        }
    });
}

/// Write a frame to every client, dropping any that error (disconnected).
fn fan_out(clients: &mut Vec<UnixStream>, frame: &Frame) {
    clients.retain_mut(|c| frame.write_to(c).is_ok());
}

fn bind_socket(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    restrict(path);
    Ok(listener)
}

fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// The shell's live working directory (Linux `/proc`, macOS `lsof`).
fn live_cwd(pid: Option<u32>, fallback: &str) -> String {
    let Some(pid) = pid else {
        return fallback.to_string();
    };
    #[cfg(target_os = "linux")]
    {
        if let Ok(path) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
            return path.to_string_lossy().into_owned();
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("lsof")
            .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
            .output()
        {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Some(p) = line.strip_prefix('n') {
                    if !p.is_empty() {
                        return p.to_string();
                    }
                }
            }
        }
    }
    fallback.to_string()
}

// ── Reboot-fallback persistence (same on-disk shape the desktop app reads) ──

/// Mirrors the desktop app's `shell_sessions::persist::PersistedMeta` so its
/// `load_all` reads what the host writes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistMeta {
    session_id: String,
    title: String,
    shell: String,
    cwd: String,
    created_at: u64,
}

/// Load prior history for `id` (the `<id>.log`), capped to the retained tail.
fn read_seed(dir: &Path, id: &str) -> Vec<u8> {
    let mut bytes = std::fs::read(dir.join(format!("{id}.log"))).unwrap_or_default();
    if bytes.len() > SCROLLBACK_CAP {
        bytes.drain(..bytes.len() - SCROLLBACK_CAP);
    }
    bytes
}

/// Persist the current session state: history + metadata for reboot restore,
/// and a refreshed receipt (cwd + title) for reattach. Called from both the
/// periodic checkpoint and the `SetTitle` handler.
fn persist_state(paths: &HostPaths, title: &str, cwd: &str, scrollback: &[u8]) {
    let meta = PersistMeta {
        session_id: paths.id.clone(),
        title: title.to_string(),
        shell: paths.shell.clone(),
        cwd: cwd.to_string(),
        created_at: paths.created_at,
    };
    checkpoint(&paths.persist_dir, &meta, scrollback);
    if let Some(mut r) = Receipt::load(&Receipt::path_in(&paths.hosts_dir, &paths.id)) {
        r.cwd = cwd.to_string();
        r.title = title.to_string();
        let _ = r.write(&paths.hosts_dir);
    }
}

fn checkpoint(dir: &Path, meta: &PersistMeta, scrollback: &[u8]) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_vec_pretty(meta) {
        let _ = std::fs::write(dir.join(format!("{}.json", meta.session_id)), json);
    }
    let tail = if scrollback.len() > SCROLLBACK_CAP {
        &scrollback[scrollback.len() - SCROLLBACK_CAP..]
    } else {
        scrollback
    };
    let _ = std::fs::write(dir.join(format!("{}.log", meta.session_id)), tail);
}

fn remove_persist(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
    let _ = std::fs::remove_file(dir.join(format!("{id}.log")));
}
