//! NIP-ST owner-side broadcasting: announces, watcher bookkeeping, and the
//! frame stream project members observe.
//!
//! Announces (kind:30623, durable) go straight to the relay over HTTP via
//! `relay::submit`. Frames (kind:24311) and everything ephemeral are WS-only
//! on the relay, so they are **signed here** and handed to the TS pump
//! (`shellBroadcast.ts`) through the `shell-broadcast-publish` Tauri event;
//! the pump publishes them over the app's relay WebSocket. Watch events
//! (kind:24310) arrive the opposite way: the pump subscribes and calls the
//! `shell_broadcast_watch` command.
//!
//! The single enforcement chokepoint is [`may_broadcast`]: no announce and no
//! frame is ever produced for a session that is not (a) assigned to a real
//! `30621:` project coordinate and (b) marked shared. Frames additionally
//! flow only while at least one watcher is live (45 s keepalive expiry), and
//! are coalesced to at most one per 100 ms per session.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use nostr::JsonUtil;
use tauri::{AppHandle, Emitter, Manager};

use super::manager::{self, ShellSessionInfo};

/// Tauri event carrying a signed, ready-to-publish frame event JSON.
const PUBLISH_EVENT: &str = "shell-broadcast-publish";
/// Tauri event carrying `{ sessionId, watchers: [pubkeyHex] }` on change.
const WATCHERS_EVENT: &str = "shell-broadcast-watchers";
/// A watcher expires this long after its last keepalive (3 missed beats).
const WATCHER_TTL: Duration = Duration::from_secs(45);
/// Minimum interval between frames for one session.
const EMIT_INTERVAL: Duration = Duration::from_millis(100);
/// Scrollback tail sent to a joining watcher.
const ATTACH_TAIL_BYTES: usize = 64 * 1024;
/// Raw bytes per tail chunk (base64 expands ~4/3; the relay caps at 96 KiB).
const TAIL_CHUNK_BYTES: usize = 48 * 1024;
/// Bound the watcher map — the relay rate-limits watch events, but a slow
/// leak of spoofed pubkeys must not grow memory unbounded.
const MAX_WATCHERS: usize = 128;

struct BroadcastState {
    /// Watcher pubkey (hex) → last keepalive.
    watchers: HashMap<String, Instant>,
    seq: u64,
    /// Fresh per broadcast entry, so observers detect app restarts.
    epoch: String,
    /// Screen at the last emitted frame; `None` forces a snapshot.
    prev_screen: Option<vt100::Screen>,
    last_emit: Instant,
    dirty: bool,
}

impl BroadcastState {
    fn new() -> Self {
        BroadcastState {
            watchers: HashMap::new(),
            seq: 0,
            epoch: uuid::Uuid::new_v4().to_string(),
            prev_screen: None,
            last_emit: Instant::now() - EMIT_INTERVAL,
            dirty: false,
        }
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static BROADCASTS: OnceLock<Mutex<HashMap<String, BroadcastState>>> = OnceLock::new();
static SWEEPER: OnceLock<()> = OnceLock::new();

fn broadcasts() -> &'static Mutex<HashMap<String, BroadcastState>> {
    BROADCASTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Store the app handle so manager-thread hooks can reach signing + events.
/// Called once at setup (before any session exists).
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The NIP-ST broadcast gate: `Some(coordinate)` only for a session assigned
/// to a real project coordinate AND with someone admitted to observe it —
/// project-wide sharing on, or at least one invited roster member. Everything
/// that announces or streams passes through here; *who* may watch is decided
/// per-watcher in [`watch`].
pub(crate) fn may_broadcast(info: &ShellSessionInfo) -> Option<String> {
    if !info.shared && info.roster.is_empty() {
        return None;
    }
    let coord = info.project_ref.as_deref()?;
    let mut parts = coord.splitn(3, ':');
    let (kind, owner, slug) = (parts.next()?, parts.next()?, parts.next()?);
    let owner_ok = owner.len() == 64 && owner.bytes().all(|b| b.is_ascii_hexdigit());
    (kind == "30621" && owner_ok && !slug.is_empty()).then(|| coord.to_string())
}

/// Publish (or refresh) the session's kind:30623 announce. `status` is
/// `"open"` or `"closed"`; a `closed` announce goes out even for an unshared
/// session (revoking the share must retract the listing), an `open` one only
/// when [`may_broadcast`] admits it. No project coordinate → no-op.
pub fn announce(app: &AppHandle, info: &ShellSessionInfo, status: &str) {
    let coordinate = if status == "closed" {
        match info.project_ref.clone() {
            Some(c) => c,
            None => return,
        }
    } else {
        match may_broadcast(info) {
            Some(c) => c,
            None => return,
        }
    };
    announce_with_coordinate(app, info, &coordinate, status);
}

/// Publish a `closed` announce under a coordinate the session no longer
/// carries (it was just moved out of that project). Same address, so the
/// replace retracts the old project's listing.
pub fn announce_closed_previous(app: &AppHandle, info: &ShellSessionInfo, coordinate: &str) {
    announce_with_coordinate(app, info, coordinate, "closed");
}

fn announce_with_coordinate(
    app: &AppHandle,
    info: &ShellSessionInfo,
    coordinate: &str,
    status: &str,
) {
    let mut tags = vec![
        nostr::Tag::identifier(info.session_id.clone()),
        tag(&["a", coordinate]),
        tag(&["title", &info.title]),
        tag(&["status", status]),
        tag(&["dims", &format!("{}x{}", info.rows, info.cols)]),
    ];
    // The invite roster rides the announce as arity-4 `p` tags
    // (["p", <hex>, "", <role>]) — the relay enforces shape/cap/no-dupes at
    // ingest, so skip (never "fix up") anything malformed here to keep a bad
    // entry from sinking the whole announce.
    for entry in &info.roster {
        let pubkey = entry.pubkey.trim().to_ascii_lowercase();
        let pubkey_ok = pubkey.len() == 64 && pubkey.bytes().all(|b| b.is_ascii_hexdigit());
        if !pubkey_ok || !buzz_core_pkg::kind::is_valid_shell_role(&entry.role) {
            continue;
        }
        tags.push(tag(&["p", &pubkey, "", &entry.role]));
    }
    let builder = nostr::EventBuilder::new(
        nostr::Kind::Custom(buzz_core_pkg::kind::KIND_SHELL_SESSION as u16),
        "",
    )
    .tags(tags);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<crate::app_state::AppState>();
        if let Err(e) = crate::relay::submit_event(builder, &state).await {
            eprintln!("shell-broadcast: announce failed: {e}");
        }
    });
}

/// Reader-thread hook: output arrived. Cheap when nobody watches.
pub fn on_output(session_id: &str) {
    let Ok(mut map) = broadcasts().lock() else {
        return;
    };
    let Some(entry) = map.get_mut(session_id) else {
        return;
    };
    if entry.watchers.is_empty() {
        return;
    }
    if entry.last_emit.elapsed() >= EMIT_INTERVAL {
        emit_screen_frame(session_id, entry);
    } else {
        // The sweeper flushes this within one tick.
        entry.dirty = true;
    }
}

/// Manager hook: the owner's grid changed. Observers follow it.
pub fn on_resize(session_id: &str, rows: u16, cols: u16) {
    let Ok(mut map) = broadcasts().lock() else {
        return;
    };
    let Some(entry) = map.get_mut(session_id) else {
        return;
    };
    if entry.watchers.is_empty() {
        return;
    }
    entry.seq += 1;
    // Force the next content frame to be a full snapshot on the new grid.
    entry.prev_screen = None;
    entry.dirty = true;
    let frame = build_frame(
        session_id,
        "resize",
        entry.seq,
        &entry.epoch,
        Some((rows, cols)),
        None,
        Vec::new(),
    );
    publish_frame(frame);
}

/// Manager hook: the session closed, exited, was unshared, or left its
/// project. Ends any active stream and drops the broadcast entry.
pub fn session_ended(session_id: &str) {
    let Ok(mut map) = broadcasts().lock() else {
        return;
    };
    let Some(mut entry) = map.remove(session_id) else {
        return;
    };
    if entry.watchers.is_empty() {
        return;
    }
    entry.seq += 1;
    let frame = build_frame(
        session_id,
        "end",
        entry.seq,
        &entry.epoch,
        None,
        None,
        Vec::new(),
    );
    publish_frame(frame);
    emit_watchers(session_id, &[]);
}

/// A watch event arrived (validated + relay-gated). Registers/refreshes/
/// removes the watcher and returns the signed attach-bundle events the TS
/// pump must publish (empty unless a new watcher joined or asked to resync).
pub fn watch(session_id: &str, watcher_pubkey: &str, action: &str) -> Result<Vec<String>, String> {
    // Never trust the caller: re-resolve the session and re-check the gate.
    let info =
        manager::info(session_id).ok_or_else(|| format!("shell session {session_id} not found"))?;
    let Some(coordinate) = may_broadcast(&info) else {
        return Err("session is not shared".to_string());
    };
    // Per-watcher admission: project members (the relay already vetted their
    // project membership before delivering the watch event) are admitted only
    // while project-wide sharing is on; invited roster members (any role) are
    // always admitted. Everyone else is refused before registration, so
    // "private + invited only" leaks no frames to the merely project-adjacent.
    let watcher_norm = watcher_pubkey.trim().to_ascii_lowercase();
    let on_roster = info.roster.iter().any(|e| e.pubkey == watcher_norm);
    if !info.shared && !on_roster {
        return Err("you are not invited to this session".to_string());
    }
    ensure_sweeper();

    let mut map = broadcasts()
        .lock()
        .map_err(|_| "broadcast registry lock poisoned".to_string())?;
    let entry = map
        .entry(session_id.to_string())
        .or_insert_with(BroadcastState::new);

    match action {
        "stop" => {
            if entry.watchers.remove(watcher_pubkey).is_some() {
                let watchers: Vec<String> = entry.watchers.keys().cloned().collect();
                emit_watchers(session_id, &watchers);
            }
            Ok(Vec::new())
        }
        "watch" | "resync" => {
            if entry.watchers.len() >= MAX_WATCHERS && !entry.watchers.contains_key(watcher_pubkey)
            {
                return Err("too many watchers".to_string());
            }
            let is_new = entry
                .watchers
                .insert(watcher_pubkey.to_string(), Instant::now())
                .is_none();
            if is_new {
                let watchers: Vec<String> = entry.watchers.keys().cloned().collect();
                emit_watchers(session_id, &watchers);
            }
            if is_new || action == "resync" {
                attach_bundle(session_id, &coordinate, entry)
            } else {
                Ok(Vec::new())
            }
        }
        other => Err(format!("unknown watch action: {other}")),
    }
}

/// Re-check every registered watcher of a session against its *current*
/// share flag + roster, dropping any no longer admitted (sharing toggled
/// off, or an invite revoked). The refreshed announce is the observer's
/// signal to stop; this ends their frame flow immediately rather than at
/// keepalive expiry.
pub fn refresh_admission(session_id: &str) {
    let Some(info) = manager::info(session_id) else {
        return;
    };
    let Ok(mut map) = broadcasts().lock() else {
        return;
    };
    let Some(entry) = map.get_mut(session_id) else {
        return;
    };
    let before = entry.watchers.len();
    entry.watchers.retain(|pubkey, _| {
        let normalized = pubkey.trim().to_ascii_lowercase();
        info.shared || info.roster.iter().any(|e| e.pubkey == normalized)
    });
    if entry.watchers.len() != before {
        let watchers: Vec<String> = entry.watchers.keys().cloned().collect();
        drop(map);
        emit_watchers(session_id, &watchers);
    }
}

/// The pubkeys currently watching a session (for the owner's indicator).
pub fn watchers(session_id: &str) -> Vec<String> {
    broadcasts()
        .lock()
        .ok()
        .and_then(|map| {
            map.get(session_id)
                .map(|entry| entry.watchers.keys().cloned().collect())
        })
        .unwrap_or_default()
}

/// Build the tail + snapshot bundle a joining watcher needs. Consumes seqs so
/// the observer's ordering stays gap-free.
fn attach_bundle(
    session_id: &str,
    _coordinate: &str,
    entry: &mut BroadcastState,
) -> Result<Vec<String>, String> {
    let mut events = Vec::new();

    let scrollback = manager::raw_scrollback(session_id)?;
    let tail_start = scrollback.len().saturating_sub(ATTACH_TAIL_BYTES);
    let tail = &scrollback[tail_start..];
    if !tail.is_empty() {
        let chunks: Vec<&[u8]> = tail.chunks(TAIL_CHUNK_BYTES).collect();
        let total = chunks.len();
        for (i, chunk) in chunks.into_iter().enumerate() {
            entry.seq += 1;
            if let Some(event) = build_frame(
                session_id,
                "tail",
                entry.seq,
                &entry.epoch,
                None,
                Some((i + 1, total)),
                chunk.to_vec(),
            ) {
                events.push(event);
            }
        }
    }

    let (screen, rows, cols) = manager::clone_screen(session_id)?;
    entry.seq += 1;
    if let Some(event) = build_frame(
        session_id,
        "snap",
        entry.seq,
        &entry.epoch,
        Some((rows, cols)),
        None,
        screen.contents_formatted(),
    ) {
        events.push(event);
    }
    entry.prev_screen = Some(screen);
    entry.last_emit = Instant::now();
    entry.dirty = false;
    Ok(events)
}

/// Emit one content frame for a session: a diff against the last emitted
/// screen, or a full snapshot when there is nothing to diff against.
fn emit_screen_frame(session_id: &str, entry: &mut BroadcastState) {
    let Ok((screen, rows, cols)) = manager::clone_screen(session_id) else {
        return;
    };
    entry.seq += 1;
    let frame = match entry.prev_screen.take() {
        Some(prev) => build_frame(
            session_id,
            "diff",
            entry.seq,
            &entry.epoch,
            None,
            None,
            screen.contents_diff(&prev),
        ),
        None => build_frame(
            session_id,
            "snap",
            entry.seq,
            &entry.epoch,
            Some((rows, cols)),
            None,
            screen.contents_formatted(),
        ),
    };
    entry.prev_screen = Some(screen);
    entry.last_emit = Instant::now();
    entry.dirty = false;
    publish_frame(frame);
}

/// Build + sign one kind:24311 frame event; `None` (with a log line) when the
/// session's gate or signing fails.
fn build_frame(
    session_id: &str,
    frame_type: &str,
    seq: u64,
    epoch: &str,
    dims: Option<(u16, u16)>,
    chunk: Option<(usize, usize)>,
    raw: Vec<u8>,
) -> Option<String> {
    let info = manager::info(session_id)?;
    let coordinate = may_broadcast(&info)?;
    let app = APP.get()?;
    let keys = match app.state::<crate::app_state::AppState>().signing_keys() {
        Ok(keys) => keys,
        Err(e) => {
            eprintln!("shell-broadcast: no signing keys: {e}");
            return None;
        }
    };

    let mut tags = vec![
        nostr::Tag::identifier(session_id.to_string()),
        tag(&["a", &coordinate]),
        tag(&["t", frame_type]),
        tag(&["seq", &seq.to_string()]),
        tag(&["epoch", epoch]),
    ];
    if let Some((rows, cols)) = dims {
        tags.push(tag(&["dims", &format!("{rows}x{cols}")]));
    }
    if let Some((i, n)) = chunk {
        tags.push(tag(&["chunk", &format!("{i}/{n}")]));
    }
    let content = base64::engine::general_purpose::STANDARD.encode(raw);
    let event = nostr::EventBuilder::new(
        nostr::Kind::Custom(buzz_core_pkg::kind::KIND_SHELL_FRAME as u16),
        content,
    )
    .tags(tags)
    .sign_with_keys(&keys);
    match event {
        Ok(event) => Some(event.as_json()),
        Err(e) => {
            eprintln!("shell-broadcast: frame signing failed: {e}");
            None
        }
    }
}

/// Hand a signed frame to the TS pump for WS publish.
fn publish_frame(frame: Option<String>) {
    let (Some(frame), Some(app)) = (frame, APP.get()) else {
        return;
    };
    let _ = app.emit(PUBLISH_EVENT, frame);
}

fn emit_watchers(session_id: &str, watchers: &[String]) {
    if let Some(app) = APP.get() {
        let _ = app.emit(
            WATCHERS_EVENT,
            serde_json::json!({ "sessionId": session_id, "watchers": watchers }),
        );
    }
}

/// Start the shared sweeper thread lazily on the first watcher: it flushes
/// coalesced frames and expires silent watchers. One thread for the module;
/// with no watchers anywhere each tick is one uncontended lock.
fn ensure_sweeper() {
    SWEEPER.get_or_init(|| {
        let _ = std::thread::Builder::new()
            .name("shell-broadcast-sweeper".into())
            .spawn(|| loop {
                std::thread::sleep(EMIT_INTERVAL);
                sweep();
            });
    });
}

fn sweep() {
    let Ok(mut map) = broadcasts().lock() else {
        return;
    };
    let mut roster_changes: Vec<(String, Vec<String>)> = Vec::new();
    for (session_id, entry) in map.iter_mut() {
        let before = entry.watchers.len();
        entry
            .watchers
            .retain(|_, seen| seen.elapsed() < WATCHER_TTL);
        if entry.watchers.len() != before {
            roster_changes.push((session_id.clone(), entry.watchers.keys().cloned().collect()));
        }
        if !entry.watchers.is_empty() && entry.dirty && entry.last_emit.elapsed() >= EMIT_INTERVAL {
            emit_screen_frame(session_id, entry);
        }
    }
    drop(map);
    for (session_id, watchers) in roster_changes {
        emit_watchers(&session_id, &watchers);
    }
}

fn tag(parts: &[&str]) -> nostr::Tag {
    nostr::Tag::parse(parts.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .expect("static tag shapes parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(project_ref: Option<&str>, shared: bool) -> ShellSessionInfo {
        ShellSessionInfo {
            session_id: "sess-1".to_string(),
            title: "shell".to_string(),
            current_directory: "/tmp".to_string(),
            shell: "/bin/zsh".to_string(),
            created_at: 1,
            rows: 24,
            cols: 80,
            running: true,
            restorable: false,
            project_ref: project_ref.map(str::to_string),
            shared,
            roster: Vec::new(),
        }
    }

    const OWNER: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

    #[test]
    fn may_broadcast_requires_shared_and_real_coordinate() {
        let coord = format!("30621:{OWNER}:proj");
        assert_eq!(
            may_broadcast(&info(Some(&coord), true)),
            Some(coord.clone())
        );
        // Unshared, missing, or malformed coordinates never broadcast.
        assert_eq!(may_broadcast(&info(Some(&coord), false)), None);
        assert_eq!(may_broadcast(&info(None, true)), None);
        assert_eq!(may_broadcast(&info(Some("local:general"), true)), None);
        assert_eq!(
            may_broadcast(&info(Some(&format!("30617:{OWNER}:repo")), true)),
            None
        );
        assert_eq!(
            may_broadcast(&info(Some("30621:shortkey:proj"), true)),
            None
        );
        assert_eq!(
            may_broadcast(&info(Some(&format!("30621:{OWNER}:")), true)),
            None
        );
    }

    #[test]
    fn may_broadcast_admits_unshared_session_with_roster() {
        let coord = format!("30621:{OWNER}:proj");
        let mut unshared = info(Some(&coord), false);
        assert_eq!(may_broadcast(&unshared), None);
        // "Private + invited only": a non-empty roster opens the gate even
        // with project-wide sharing off …
        unshared.roster.push(super::super::manager::RosterEntry {
            pubkey: "ab".repeat(32),
            role: "viewer".to_string(),
        });
        assert_eq!(may_broadcast(&unshared), Some(coord));
        // … but never without a real project coordinate.
        let mut no_project = info(None, false);
        no_project.roster.push(super::super::manager::RosterEntry {
            pubkey: "ab".repeat(32),
            role: "collaborator".to_string(),
        });
        assert_eq!(may_broadcast(&no_project), None);
    }

    #[test]
    fn tail_chunking_boundaries() {
        // 100 KiB tail → 64 KiB kept → chunks of 48 KiB → 2 chunks (48 + 16).
        let tail_len = ATTACH_TAIL_BYTES;
        let chunks: Vec<usize> = (0..tail_len)
            .collect::<Vec<_>>()
            .chunks(TAIL_CHUNK_BYTES)
            .map(|c| c.len())
            .collect();
        assert_eq!(chunks, vec![TAIL_CHUNK_BYTES, tail_len - TAIL_CHUNK_BYTES]);
    }
}
