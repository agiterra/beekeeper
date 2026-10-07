//! Client-side registry for the built-in shell.
//!
//! Each session's shell runs in a **detached host process** (`beekeeper-shell-host`)
//! that owns the PTY and outlives this app, so a shell (and whatever is running
//! in it) survives an app restart/update. This module is the *client*: it
//! spawns/attaches hosts over their Unix sockets and mirrors each host's output
//! into a local `SharedState` (scrollback + `vt100` parser) for reads and the
//! `shell-session-output` frontend event — the exact downstream flow the old
//! local-PTY reader thread had; only the byte source changed to a socket.
//!
//! Module-owned static registry (same pattern as `shell_sessions::access`):
//! the broker reaches sessions without a Tauri `State` handle. Session state is
//! machine-scoped; the *host processes* persist across app restarts, and disk
//! history (`persist`) is the cold fallback for when a host is gone (reboot).

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use beekeeper_shell_host::proto::Frame;
use beekeeper_shell_host::receipt::Receipt;
use tauri::{AppHandle, Emitter};

use super::coding_session::{self, ShellCodingSessionRef};
use super::session_driver::{self, InputResize, Probed, SessionDriver};

mod roster;
mod types;
pub use roster::{add_roster_entry, set_roster};
use types::{dormant, lock_registry, registry, DormantSession, SharedState, ShellSession};
pub use types::{RosterEntry, ShellRead, ShellSessionInfo};

/// Roster size cap, matching the relay's ingest limit for kind:30623.
const MAX_ROSTER: usize = 64;

/// Cap on retained raw scrollback per session. Old output is dropped from the
/// front; 1 MiB of text is far more than any snapshot read needs.
const SCROLLBACK_CAP: usize = 1024 * 1024;

/// How much of the scrollback tail a plain, cursor-less snapshot renders —
/// roughly "the visible screen plus a little history".
const SNAPSHOT_TAIL_BYTES: usize = 8 * 1024;

/// Frontend event carrying a base64 chunk of PTY output.
const OUTPUT_EVENT: &str = "shell-session-output";
/// Frontend event fired when a session's shell exits.
const EXIT_EVENT: &str = "shell-session-exit";

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn default_shell() -> String {
    if cfg!(windows) {
        return "powershell.exe".to_string();
    }
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

fn default_cwd() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// The directory holding host sockets + reattach receipts
/// (`<state_dir>/shell-hosts`, next to the session-broker socket). Namespaced
/// per instance (buzz vs buzz-dev) so two apps on one machine never adopt
/// each other's detached sessions at reattach.
fn hosts_dir() -> Result<PathBuf, String> {
    let dir = crate::shell_sessions::state_dir()?.join("shell-hosts");
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create shell-hosts dir: {e}"))?;
    Ok(dir)
}

fn socket_path_for(dir: &std::path::Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.sock"))
}

/// Spawn a new detached shell session running `command` (default: `$SHELL`) in
/// `cwd` (default: `$HOME`). The shell runs in a `beekeeper-shell-host` process that
/// outlives this app. `project_ref`, if given, tags the session with the
/// project container it was opened from (see `set_project_ref`).
///
/// With `coding_session` (SV-25) the directory is the session's tree as this
/// machine resolves it; a `cwd` from the caller is refused rather than
/// trusted, and a session with no tree here is refused with the tree's own
/// reason — never `$HOME`.
pub fn create(
    app: &AppHandle,
    cwd: Option<String>,
    title: Option<String>,
    command: Option<String>,
    project_ref: Option<String>,
    coding_session: Option<ShellCodingSessionRef>,
) -> Result<ShellSessionInfo, String> {
    let shell = command.unwrap_or_else(default_shell);
    let cwd = match &coding_session {
        Some(reference) => coding_session::create_cwd(app, cwd.as_deref(), reference)?,
        None => cwd.filter(|c| !c.is_empty()).unwrap_or_else(default_cwd),
    };
    if !std::path::Path::new(&cwd).is_dir() {
        return Err(format!("directory does not exist: {cwd}"));
    }
    let title = title.filter(|t| !t.is_empty()).unwrap_or_else(|| {
        if coding_session.is_some() {
            return coding_session::SESSION_SHELL_DEFAULT_TITLE.to_string();
        }
        std::path::Path::new(&cwd)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| shell.clone())
    });
    let info = ShellSessionInfo {
        session_id: uuid::Uuid::new_v4().to_string(),
        title,
        current_directory: cwd.clone(),
        shell: shell.clone(),
        created_at: now_unix(),
        rows: 24,
        cols: 80,
        running: true,
        restorable: false,
        project_ref,
        shared: true,
        roster: Vec::new(),
        coding_session,
    };
    let info = spawn_host_and_attach(app, info, &cwd)?;
    crate::shell_sessions::persist::set_app_meta(
        app,
        &info.session_id,
        crate::shell_sessions::persist::AppMeta::from_info(&info),
    );
    crate::shell_sessions::broadcast::announce(app, &info, "open");
    Ok(info)
}

/// Spawn a fresh host process for `info` in `cwd`, then attach to it. Used by
/// `create` (new id, no history) and `resume` (reused id — the host seeds its
/// scrollback from the on-disk history for that id).
fn spawn_host_and_attach(
    app: &AppHandle,
    info: ShellSessionInfo,
    cwd: &str,
) -> Result<ShellSessionInfo, String> {
    let hosts = hosts_dir()?;
    let socket = socket_path_for(&hosts, &info.session_id);
    let persist = crate::shell_sessions::persist::dir(app)?;
    // Backend availability gate before any spawn is attempted — the driver's
    // `detect` slice only checks resolvability, it never launches anything;
    // the actual spawn below stays outside the trait (spawn is forbidden on
    // `SessionDriver` by architect ruling — see `session_driver.rs`).
    session_driver::resolve_driver(session_driver::BEEKEEPER_SHELL_HOST)?;
    let host_bin = crate::managed_agents::resolve_command("beekeeper-shell-host")
        .ok_or_else(|| "beekeeper-shell-host binary not found".to_string())?;

    // Spawn detached with stdio to /dev/null; the host double-forks + setsid so
    // it survives this app. Reap our short-lived direct child.
    let child = std::process::Command::new(&host_bin)
        .args([
            "--id".into(),
            info.session_id.clone(),
            "--socket".into(),
            socket.to_string_lossy().into_owned(),
            "--hosts-dir".into(),
            hosts.to_string_lossy().into_owned(),
            "--persist-dir".into(),
            persist.to_string_lossy().into_owned(),
            "--cwd".into(),
            cwd.to_string(),
            "--shell".into(),
            info.shell.clone(),
            "--title".into(),
            info.title.clone(),
            "--rows".into(),
            info.rows.to_string(),
            "--cols".into(),
            info.cols.to_string(),
            "--created-at".into(),
            info.created_at.to_string(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn shell host: {e}"))?;
    reap(child);

    attach(app, info, &socket)
}

/// Reap the short-lived direct child (the host double-forks, so it exits fast).
fn reap(mut child: std::process::Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// Connect to a host at `socket`, synchronously drain its greeting + scrollback
/// replay into a fresh `SharedState` (so a caller's frontend sees history the
/// moment this returns), register the session live, and spawn the live reader
/// thread. Used for both freshly-spawned hosts and reattached surviving ones.
fn attach(
    app: &AppHandle,
    mut info: ShellSessionInfo,
    socket: &std::path::Path,
) -> Result<ShellSessionInfo, String> {
    // Connecting to an already-listening socket has no dependency on the
    // backend binary being currently resolvable (a surviving host doesn't
    // need it to keep running), so this constructs the driver directly
    // rather than going through `resolve_driver`'s `detect` gate — that gate
    // is reserved for the point a *new* host is about to be spawned.
    let driver = session_driver::BeekeeperShellHostDriver;
    let (io, hello, mut read_half) = driver.attach_existing(socket, Duration::from_secs(10))?;
    // `attach_existing` only ever hands back the narrowed `io` handle (see
    // `host_client::AttachedClient`); `manager.rs` reconstitutes the full
    // `HostClient` here for the kill/rename authority it already had before
    // the driver existed (`close`, `rename`) — not a new grant, just moved
    // off the trait's return type so no other driver caller gets it.
    let client = io.full_client();

    let state = Arc::new(Mutex::new(SharedState {
        scrollback: Vec::new(),
        total: 0,
        last_output_at: None,
        parser: vt100::Parser::new(info.rows, info.cols, 0),
    }));

    // Synchronous replay drain: read frames until `Synced`, appending history
    // into `state` silently (the frontend shows it via the attach snapshot, not
    // via live events). Bounded by a read timeout so a bad host can't hang us.
    read_half
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("failed to set socket timeout: {e}"))?;
    let exited = drain_replay(&mut read_half, hello.total, &state)?;
    // Live output must block indefinitely.
    let _ = read_half.set_read_timeout(None);

    // `exited` means the host's shell had already ended before we attached; we
    // still register the session (running=false) so the UI shows the ended state.
    info.running = !exited;
    info.restorable = false;
    let session_id = info.session_id.clone();
    {
        let mut sessions = lock_registry()?;
        sessions.insert(
            session_id.clone(),
            ShellSession {
                info: info.clone(),
                client,
                io,
                state: state.clone(),
                shell_pid: hello.shell_pid,
            },
        );
    }

    spawn_reader_thread(app.clone(), session_id, read_half, state);
    Ok(info)
}

/// Drain a host's greeting replay: read frames until `Synced` (seed `total`
/// from the host) or `Exit` (shell already ended). Replay `Output` is appended
/// to `state` without bumping the cursor — the frontend shows history via its
/// attach snapshot, so it must not also arrive as live events. Returns whether
/// the shell had already exited.
fn drain_replay(
    read_half: &mut impl std::io::Read,
    hello_total: u64,
    state: &Arc<Mutex<SharedState>>,
) -> Result<bool, String> {
    loop {
        match Frame::read_from(read_half) {
            Ok(Some(Frame::Output(bytes))) => {
                if let Ok(mut st) = state.lock() {
                    append_output(&mut st, &bytes, false);
                }
            }
            Ok(Some(Frame::Synced)) => {
                if let Ok(mut st) = state.lock() {
                    st.total = hello_total;
                }
                return Ok(false);
            }
            Ok(Some(Frame::Exit)) => return Ok(true),
            Ok(Some(_)) => {}
            Ok(None) => return Err("shell host closed during replay".to_string()),
            Err(e) => return Err(format!("failed to read shell-host replay: {e}")),
        }
    }
}

/// Append a host output chunk to the session state (scrollback + cursor + vt100).
fn append_output(st: &mut SharedState, chunk: &[u8], count_total: bool) {
    st.scrollback.extend_from_slice(chunk);
    if count_total {
        st.total += chunk.len() as u64;
        st.last_output_at = Some(Instant::now());
    }
    if st.scrollback.len() > SCROLLBACK_CAP {
        let excess = st.scrollback.len() - SCROLLBACK_CAP;
        st.scrollback.drain(..excess);
    }
    st.parser.process(chunk);
}

/// The live reader: host `Output` → `SharedState` + frontend event; `Exit` →
/// shell exited (host cleans up its files); socket EOF without `Exit` → host
/// gone unexpectedly (keep the on-disk history for reboot restore).
fn spawn_reader_thread(
    app: AppHandle,
    session_id: String,
    mut read_half: UnixStream,
    state: Arc<Mutex<SharedState>>,
) {
    let _ = std::thread::Builder::new()
        .name(format!("shell-session-{session_id}"))
        .spawn(move || {
            loop {
                match Frame::read_from(&mut read_half) {
                    Ok(Some(Frame::Output(bytes))) => {
                        if let Ok(mut st) = state.lock() {
                            append_output(&mut st, &bytes, true);
                        }
                        let _ = app.emit(
                            OUTPUT_EVENT,
                            serde_json::json!({
                                "sessionId": session_id,
                                "dataB64": base64::engine::general_purpose::STANDARD.encode(&bytes),
                            }),
                        );
                        crate::shell_sessions::broadcast::on_output(&session_id);
                    }
                    Ok(Some(Frame::Exit)) => break,
                    // Ignore host→client-only frames we don't act on here.
                    Ok(Some(_)) => {}
                    // Clean close or error: the host is gone. Treat as ended.
                    Ok(None) | Err(_) => break,
                }
            }
            if let Ok(mut sessions) = registry().lock() {
                if let Some(session) = sessions.get_mut(&session_id) {
                    session.info.running = false;
                }
            }
            if let Some(session_info) = info(&session_id) {
                crate::shell_sessions::broadcast::announce(&app, &session_info, "closed");
            }
            crate::shell_sessions::broadcast::session_ended(&session_id);
            let _ = app.emit(EXIT_EVENT, serde_json::json!({ "sessionId": session_id }));
        });
}

/// At startup: reattach to hosts that survived the last app run, and register
/// the rest (dead host / disk-only history) as dormant, restorable sessions.
pub fn reattach_hosts(app: &AppHandle) {
    // App-owned per-session metadata (project tag + share flag) — the host's
    // receipts and checkpoints never carry these.
    let app_meta = crate::shell_sessions::persist::load_app_meta(app);
    // 1. Live hosts: a receipt whose host pid is alive and socket connects
    //    (list-probe — read-only, never deletes the receipt itself).
    let driver = session_driver::BeekeeperShellHostDriver;
    let mut reattached: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(dir) = hosts_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Some(receipt) = Receipt::load(&path) else {
                    continue;
                };
                let socket = PathBuf::from(&receipt.socket_path);
                if driver.list_probe(&receipt, &socket) == Probed::Alive {
                    // The host never learns app-side fields (project tag,
                    // share flag); they come from the app-owned sidecar map.
                    let app_meta = app_meta.get(&receipt.id).cloned().unwrap_or_default();
                    let info = ShellSessionInfo {
                        session_id: receipt.id.clone(),
                        title: receipt.title.clone(),
                        current_directory: receipt.cwd.clone(),
                        shell: receipt.shell.clone(),
                        created_at: receipt.created_at,
                        rows: 24,
                        cols: 80,
                        running: true,
                        restorable: false,
                        project_ref: app_meta.project_ref,
                        shared: app_meta.shared,
                        roster: app_meta.roster,
                        coding_session: app_meta.coding_session,
                    };
                    match attach(app, info, &socket) {
                        Ok(reattached_info) => {
                            reattached.insert(receipt.id.clone());
                            crate::shell_sessions::broadcast::announce(
                                app,
                                &reattached_info,
                                "open",
                            );
                        }
                        Err(e) => eprintln!("shell-host: reattach {} failed: {e}", receipt.id),
                    }
                } else {
                    // Stale receipt for a dead host — clean it up; the disk
                    // history (below) is the restore path.
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::remove_file(&socket);
                }
            }
        }
    }

    // 2. Disk history for sessions not now live → dormant/restorable (reboot).
    if !crate::shell_sessions::persist::enabled(app) {
        return;
    }
    let restored = crate::shell_sessions::persist::load_all(app);
    let Ok(mut map) = dormant().lock() else {
        return;
    };
    for session in restored {
        let meta = session.meta;
        if reattached.contains(&meta.session_id) {
            continue;
        }
        let (rows, cols) = (24u16, 80u16);
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(&session.scrollback);
        let total = session.scrollback.len() as u64;
        // The host's checkpoint rewrites `<id>.json` without app-side fields,
        // so the sidecar map wins over whatever the meta file carries.
        let session_app_meta = app_meta.get(&meta.session_id).cloned();
        let info = ShellSessionInfo {
            session_id: meta.session_id.clone(),
            title: meta.title,
            current_directory: meta.cwd.clone(),
            shell: meta.shell.clone(),
            created_at: meta.created_at,
            rows,
            cols,
            running: false,
            restorable: true,
            project_ref: session_app_meta
                .as_ref()
                .and_then(|m| m.project_ref.clone())
                .or_else(|| meta.project_ref.clone()),
            shared: session_app_meta.as_ref().map(|m| m.shared).unwrap_or(true),
            coding_session: session_app_meta
                .as_ref()
                .and_then(|m| m.coding_session.clone()),
            roster: session_app_meta.map(|m| m.roster).unwrap_or_default(),
        };
        map.insert(
            meta.session_id.clone(),
            DormantSession {
                info,
                state: Arc::new(Mutex::new(SharedState {
                    scrollback: session.scrollback,
                    total,
                    last_output_at: None,
                    parser,
                })),
                cwd: meta.cwd,
            },
        );
    }
}

/// Bring a restorable session back to life: spawn a fresh host in its saved
/// directory (falling back to `$HOME` if gone). The host seeds its scrollback
/// from the on-disk history for this id, so the old output shows above the new
/// prompt.
pub fn resume(app: &AppHandle, session_id: &str) -> Result<ShellSessionInfo, String> {
    let session = {
        let mut map = dormant()
            .lock()
            .map_err(|_| "shell-session dormant lock poisoned".to_string())?;
        map.remove(session_id)
    };
    let Some(session) = session else {
        // Already live? Return its info; otherwise it's unknown.
        return info(session_id).ok_or_else(|| format!("no restorable session {session_id}"));
    };
    // A session shell resumes inside the session's tree or not at all.
    let resumed_cwd = match session.info.coding_session.as_ref() {
        Some(reference) => coding_session::resume_session_cwd(app, reference, &session.cwd),
        None if std::path::Path::new(&session.cwd).is_dir() => Ok(session.cwd.clone()),
        None => Ok(default_cwd()),
    };
    let cwd = match resumed_cwd {
        Ok(cwd) => cwd,
        Err(reason) => {
            // Put it back: refusing to resume must not forget the session.
            if let Ok(mut map) = dormant().lock() {
                map.insert(session_id.to_string(), session);
            }
            return Err(reason);
        }
    };
    let info = spawn_host_and_attach(app, session.info, &cwd)?;
    crate::shell_sessions::broadcast::announce(app, &info, "open");
    Ok(info)
}

/// All sessions (live + restorable), oldest first.
pub fn list() -> Vec<ShellSessionInfo> {
    let mut infos: Vec<ShellSessionInfo> = Vec::new();
    if let Ok(sessions) = registry().lock() {
        infos.extend(sessions.values().map(|s| s.info.clone()));
    }
    if let Ok(map) = dormant().lock() {
        infos.extend(map.values().map(|s| s.info.clone()));
    }
    infos.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    infos
}

/// Info for one session (live or dormant), if it exists.
pub fn info(session_id: &str) -> Option<ShellSessionInfo> {
    if let Ok(sessions) = registry().lock() {
        if let Some(s) = sessions.get(session_id) {
            return Some(s.info.clone());
        }
    }
    dormant()
        .lock()
        .ok()
        .and_then(|m| m.get(session_id).map(|s| s.info.clone()))
}

/// Kill the shell and drop the session, live or dormant. Tells the host to
/// terminate (it then cleans up its own socket/receipt/history) and best-effort
/// forgets any files here — an explicit close means "gone", not restorable.
/// (Merely quitting the app never calls this: hosts are left running to reattach.)
pub fn close(app: &AppHandle, session_id: &str) -> Result<(), String> {
    // Announce the close (and end any observer stream) while the info is
    // still resolvable.
    if let Some(session_info) = info(session_id) {
        crate::shell_sessions::broadcast::announce(app, &session_info, "closed");
        crate::shell_sessions::broadcast::session_ended(session_id);
    }
    crate::shell_sessions::persist::remove_app_meta(app, session_id);
    let removed_live = {
        let mut sessions = lock_registry()?;
        sessions.remove(session_id).inspect(|s| {
            let _ = s.client.kill();
        })
    };
    let removed_dormant = dormant()
        .lock()
        .ok()
        .and_then(|mut m| m.remove(session_id))
        .is_some();
    // Belt-and-suspenders cleanup in case the host is already gone.
    crate::shell_sessions::persist::remove(app, session_id);
    if let Ok(dir) = hosts_dir() {
        Receipt::remove(&dir, session_id);
        let _ = std::fs::remove_file(socket_path_for(&dir, session_id));
    }
    if removed_live.is_some() || removed_dormant {
        Ok(())
    } else {
        Err(format!("shell session {session_id} not found"))
    }
}

/// Rename a session (live or dormant). A live session's title is updated in the
/// registry (for the immediate UI) and pushed to its host, which owns the
/// receipt + persisted metadata and so makes the new name survive a restart. A
/// dormant (restorable) session has no host, so its in-memory info and on-disk
/// metadata are updated directly.
pub fn rename(app: &AppHandle, session_id: &str, title: &str) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("shell session title must not be empty".to_string());
    }

    let live_client = {
        let mut sessions = lock_registry()?;
        match sessions.get_mut(session_id) {
            Some(session) => {
                session.info.title = title.to_string();
                Some(session.client.clone())
            }
            None => None,
        }
    };
    if let Some(client) = live_client {
        client.set_title(title)?;
        if let Some(session_info) = info(session_id) {
            crate::shell_sessions::broadcast::announce(app, &session_info, "open");
        }
        return Ok(());
    }

    let renamed = {
        let mut map = dormant()
            .lock()
            .map_err(|_| "shell-session dormant lock poisoned".to_string())?;
        match map.get_mut(session_id) {
            Some(session) => {
                session.info.title = title.to_string();
                true
            }
            None => false,
        }
    };
    if renamed {
        crate::shell_sessions::persist::rename(app, session_id, title);
        Ok(())
    } else {
        Err(format!("shell session {session_id} not found"))
    }
}

/// Set (or clear) a session's project container tag. Local-only bookkeeping —
/// never sent to the host or the relay. A live session's in-memory info is
/// updated directly (lost on the next app restart, like the rest of a live
/// host's client-side info, unless it's since gone dormant); a dormant
/// (restorable) session's in-memory info *and* on-disk metadata are updated,
/// mirroring `rename`'s dormant path, so it survives a restart.
pub fn set_project_ref(
    app: &AppHandle,
    session_id: &str,
    project_ref: Option<String>,
) -> Result<(), String> {
    let old_ref = info(session_id).and_then(|i| i.project_ref);
    let updated = {
        let mut sessions = lock_registry()?;
        match sessions.get_mut(session_id) {
            Some(session) => {
                session.info.project_ref = project_ref.clone();
                Some(session.info.clone())
            }
            None => None,
        }
    };
    let updated = match updated {
        Some(info) => Some(info),
        None => {
            let mut map = dormant()
                .lock()
                .map_err(|_| "shell-session dormant lock poisoned".to_string())?;
            match map.get_mut(session_id) {
                Some(session) => {
                    session.info.project_ref = project_ref.clone();
                    let info = session.info.clone();
                    drop(map);
                    crate::shell_sessions::persist::set_project_ref(
                        app,
                        session_id,
                        project_ref.clone(),
                    );
                    Some(info)
                }
                None => None,
            }
        }
    };
    let Some(info) = updated else {
        return Err(format!("shell session {session_id} not found"));
    };
    crate::shell_sessions::persist::set_app_meta(
        app,
        session_id,
        crate::shell_sessions::persist::AppMeta::from_info(&info),
    );
    // Announce under the (possibly new) coordinate — the addressable replace
    // drops the announce from the old project's list automatically. Clearing
    // the ref publishes `closed` under the old coordinate (same address) and
    // ends any active observer stream.
    if info.project_ref.is_some() {
        crate::shell_sessions::broadcast::announce(app, &info, "open");
    } else if let Some(old_coord) = old_ref {
        crate::shell_sessions::broadcast::announce_closed_previous(app, &info, &old_coord);
        crate::shell_sessions::broadcast::session_ended(session_id);
    }
    Ok(())
}

/// Flip a session's NIP-ST share flag. Turning sharing off closes the
/// announce and ends any active observer stream.
pub fn set_shared(app: &AppHandle, session_id: &str, shared: bool) -> Result<(), String> {
    let updated = {
        let mut sessions = lock_registry()?;
        match sessions.get_mut(session_id) {
            Some(session) => {
                session.info.shared = shared;
                Some(session.info.clone())
            }
            None => {
                drop(sessions);
                let mut map = dormant()
                    .lock()
                    .map_err(|_| "shell-session dormant lock poisoned".to_string())?;
                match map.get_mut(session_id) {
                    Some(session) => {
                        session.info.shared = shared;
                        Some(session.info.clone())
                    }
                    None => None,
                }
            }
        }
    };
    let Some(info) = updated else {
        return Err(format!("shell session {session_id} not found"));
    };
    crate::shell_sessions::persist::set_app_meta(
        app,
        session_id,
        crate::shell_sessions::persist::AppMeta::from_info(&info),
    );
    if crate::shell_sessions::broadcast::may_broadcast(&info).is_some() {
        crate::shell_sessions::broadcast::announce(app, &info, "open");
        // Sharing off but invited members remain: drop any project-member
        // watchers immediately instead of waiting out their keepalive TTL.
        crate::shell_sessions::broadcast::refresh_admission(session_id);
    } else {
        // Neither project-wide sharing nor invited members remain — retract
        // the announce and end any observer stream.
        crate::shell_sessions::broadcast::announce(app, &info, "closed");
        crate::shell_sessions::broadcast::session_ended(session_id);
    }
    Ok(())
}

/// A clone of the session's rendered screen (live or dormant), plus its
/// grid, for the NIP-ST broadcast module's snapshot/diff frames.
pub(crate) fn clone_screen(session_id: &str) -> Result<(vt100::Screen, u16, u16), String> {
    let (state, rows, cols) = {
        if let Ok(sessions) = registry().lock() {
            if let Some(s) = sessions.get(session_id) {
                (s.state.clone(), s.info.rows, s.info.cols)
            } else {
                let map = dormant()
                    .lock()
                    .map_err(|_| "shell-session dormant lock poisoned".to_string())?;
                let s = map
                    .get(session_id)
                    .ok_or_else(|| format!("shell session {session_id} not found"))?;
                (s.state.clone(), s.info.rows, s.info.cols)
            }
        } else {
            return Err("shell-session registry lock poisoned".to_string());
        }
    };
    let st = state.lock().map_err(|_| lock_err())?;
    Ok((st.parser.screen().clone(), rows, cols))
}

/// The live shell's pid, when its host reported one (the foreground read).
pub(crate) fn live_shell_pid(session_id: &str) -> Option<u32> {
    registry()
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(session_id).and_then(|s| s.shell_pid))
}

/// Write raw bytes (keystrokes) to the session's host.
pub fn write(session_id: &str, data: &[u8]) -> Result<(), String> {
    let (io, running) = {
        let sessions = lock_registry()?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("shell session {session_id} not found"))?;
        (session.io.clone(), session.info.running)
    };
    if !running {
        return Err(format!("shell session {session_id} has exited"));
    }
    session_driver::BeekeeperShellHostDriver.input_resize(&io, InputResize::Input(data))
}

/// Resize the session's PTY via its host (the frontend terminal drives this).
pub fn resize(session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
    let io = {
        let mut sessions = lock_registry()?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("shell session {session_id} not found"))?;
        session.info.rows = rows;
        session.info.cols = cols;
        if let Ok(mut st) = session.state.lock() {
            st.parser.set_size(rows, cols);
        }
        session.io.clone()
    };
    session_driver::BeekeeperShellHostDriver
        .input_resize(&io, InputResize::Resize { rows, cols })?;
    crate::shell_sessions::broadcast::on_resize(session_id, rows, cols);
    Ok(())
}

/// Clone the shared state Arc so a read can work without holding the registry
/// lock (and without blocking the reader thread beyond a brief state lock).
/// Resolves live sessions first, then dormant (restorable) ones, so reads show
/// a restored session's history before it's resumed.
fn state_of(session_id: &str) -> Result<Arc<Mutex<SharedState>>, String> {
    if let Ok(sessions) = registry().lock() {
        if let Some(s) = sessions.get(session_id) {
            return Ok(s.state.clone());
        }
    }
    dormant()
        .lock()
        .map_err(|_| "shell-session dormant lock poisoned".to_string())?
        .get(session_id)
        .map(|s| s.state.clone())
        .ok_or_else(|| format!("shell session {session_id} not found"))
}

/// The current output cursor (total bytes seen) for a session.
pub fn cursor(session_id: &str) -> Result<u64, String> {
    let state = state_of(session_id)?;
    let st = state.lock().map_err(|_| lock_err())?;
    Ok(st.total)
}

/// How long since the session last produced output, or `None` if it never has.
pub fn idle_for(session_id: &str) -> Result<Option<Duration>, String> {
    let state = state_of(session_id)?;
    let st = state.lock().map_err(|_| lock_err())?;
    Ok(st.last_output_at.map(|t| t.elapsed()))
}

fn lock_err() -> String {
    "shell-session state lock poisoned".to_string()
}

/// The raw scrollback buffer, for frontend terminal replay on (re)attach.
pub fn raw_scrollback(session_id: &str) -> Result<Vec<u8>, String> {
    let state = state_of(session_id)?;
    let st = state.lock().map_err(|_| lock_err())?;
    Ok(st.scrollback.clone())
}

/// The current input line (cursor row) and cursor column from the emulator.
fn input_state(st: &SharedState) -> (String, u16) {
    let screen = st.parser.screen();
    let (row, col) = screen.cursor_position();
    let contents = screen.contents();
    let line = contents
        .split('\n')
        .nth(row as usize)
        .unwrap_or_default()
        .trim_end()
        .to_string();
    (line, col)
}

/// Estimate how many terminal rows a byte window occupies (newlines + line
/// wrapping) so a throwaway parser can render it without losing lines off the
/// top. Deliberately generous — extra blank rows are trimmed, whereas too few
/// would drop output. Escape bytes are counted as width, which only
/// over-estimates.
fn estimate_rows(bytes: &[u8], cols: u16) -> u16 {
    let cols = cols.max(1) as usize;
    let mut rows: usize = 1;
    let mut line_len: usize = 0;
    for &b in bytes {
        match b {
            b'\n' => {
                rows += 1;
                line_len = 0;
            }
            b'\r' => line_len = 0,
            _ => {
                line_len += 1;
                if line_len >= cols {
                    rows += 1;
                    line_len = 0;
                }
            }
        }
    }
    (rows + 2).clamp(24, 20_000) as u16
}

/// Render a raw PTY byte window into clean text by replaying it through a fresh
/// terminal emulator, so cursor motion resolves into a real 2-D grid: column
/// spacing done with cursor-forward becomes real spaces, and line-editor
/// redraws (which naive ANSI-stripping would duplicate — e.g. `ccd`) collapse
/// to their final glyphs. `vt100` trims trailing blank cells and rows.
fn render_clean(bytes: &[u8], cols: u16) -> String {
    let rows = estimate_rows(bytes, cols);
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(bytes);
    parser.screen().contents()
}

/// Rendered text of the raw bytes at absolute offsets `[from, total)`. Returns
/// the text, the new cursor (`total`), and whether `from` predated the retained
/// buffer (some output dropped).
fn output_since(st: &SharedState, from: u64) -> (String, u64, bool) {
    let buf_start = st.total - st.scrollback.len() as u64;
    let truncated = from < buf_start;
    let slice_start = from
        .saturating_sub(buf_start)
        .min(st.scrollback.len() as u64) as usize;
    let cols = st.parser.screen().size().1;
    let text = render_clean(&st.scrollback[slice_start..], cols);
    (text, st.total, truncated)
}

/// A full read for an agent. `rendered` returns the live vt100 screen (the
/// current visible terminal); otherwise the byte stream is replayed through a
/// terminal emulator into clean, parseable text (real newlines, columns and
/// spacing preserved, no echo duplication). `since` returns only output after
/// that cursor; else `scrollback` returns the whole retained buffer and the
/// default returns the tail. Always includes the cursor + input-line state.
pub fn read(
    session_id: &str,
    rendered: bool,
    scrollback: bool,
    since: Option<u64>,
) -> Result<ShellRead, String> {
    let state = state_of(session_id)?;
    let st = state.lock().map_err(|_| lock_err())?;
    let (input_line, cursor_col) = input_state(&st);

    let (text, cursor, truncated) = if rendered {
        (st.parser.screen().contents(), st.total, false)
    } else if let Some(from) = since {
        output_since(&st, from)
    } else if scrollback {
        output_since(&st, 0)
    } else {
        // Plain tail: render the last chunk of retained bytes. Clamp the start
        // to the retained buffer so a short session isn't flagged truncated.
        let buf_start = st.total - st.scrollback.len() as u64;
        let from = st
            .total
            .saturating_sub(SNAPSHOT_TAIL_BYTES as u64)
            .max(buf_start);
        output_since(&st, from)
    };

    Ok(ShellRead {
        text,
        cursor,
        truncated,
        input_line,
        cursor_col,
    })
}

/// Rendered snapshot (tail, or all with `scrollback`) — the read the broker
/// serves to agents that don't ask for cursor/rendered detail.
pub fn snapshot(session_id: &str, scrollback: bool) -> Result<String, String> {
    Ok(read(session_id, false, scrollback, None)?.text)
}

/// Just the current input line (no output render) — for the session list, which
/// wants the pending-input hint per session without paying for a tail render.
pub fn input_line(session_id: &str) -> Option<String> {
    let state = state_of(session_id).ok()?;
    let st = state.lock().ok()?;
    Some(input_state(&st).0)
}

// Persistence (history checkpoint + live-cwd tracking) now lives in the
// detached host process (`beekeeper-shell-host`), which owns the authoritative
// scrollback and keeps writing across app restarts. The app only reads that
// on-disk history back (`persist::load_all`) as the reboot fallback.

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
