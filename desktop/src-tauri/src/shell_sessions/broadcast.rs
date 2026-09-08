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
//! are throttled per session by [`Cadence`]: one content frame per second
//! (the latest screen wins — a diff is computed at flush time, so a burst of
//! updates inside the interval collapses to one frame), a hard cap of 40
//! frames per rolling minute, and a back-off (interval doubled, up to 4 s,
//! held for 60 s) whenever the relay refuses a frame under its quota.
//!
//! Why so slow: frames are ephemeral EVENTs and the relay counts them against
//! the owner's per-key message quota (60/min) alongside chat and session
//! turns. At the old 100 ms cadence a single shared terminal alone was 600
//! EVENTs/min; at 1 s it is at most 40, leaving a third of the budget for
//! everything else. The cadence is observable through [`cadence`] (and the
//! `shell-broadcast-cadence` Tauri event) so the UI can say so.

use std::collections::{HashMap, VecDeque};
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
/// Tauri event carrying a [`BroadcastCadence`] whenever a session's cadence
/// changes (relay back-off engaged or released).
const CADENCE_EVENT: &str = "shell-broadcast-cadence";
/// A watcher expires this long after its last keepalive (3 missed beats).
const WATCHER_TTL: Duration = Duration::from_secs(45);
/// Baseline minimum interval between content frames for one session.
const EMIT_INTERVAL: Duration = Duration::from_secs(1);
/// The interval never backs off beyond this.
const MAX_EMIT_INTERVAL: Duration = Duration::from_secs(4);
/// How long a relay-refused frame keeps the interval backed off before it
/// returns to [`EMIT_INTERVAL`].
const BACKOFF_HOLD: Duration = Duration::from_secs(60);
/// Hard cap on frames of any type per session per [`FRAME_CAP_WINDOW`].
const FRAME_CAP: usize = 40;
/// Rolling window for [`FRAME_CAP`].
const FRAME_CAP_WINDOW: Duration = Duration::from_secs(60);
/// The sweeper's tick: how late a coalesced frame can flush after its
/// interval elapses, and the granularity of watcher expiry.
const SWEEP_TICK: Duration = Duration::from_millis(250);
/// The `reason` a [`BroadcastCadence`] reports: the throttle exists because
/// frames are charged to the owner's per-key relay quota.
const CADENCE_REASON: &str = "quota";
/// Scrollback tail sent to a joining watcher.
const ATTACH_TAIL_BYTES: usize = 64 * 1024;
/// Raw bytes per tail chunk (base64 expands ~4/3; the relay caps at 96 KiB).
const TAIL_CHUNK_BYTES: usize = 48 * 1024;
/// Bound the watcher map — the relay rate-limits watch events, but a slow
/// leak of spoofed pubkeys must not grow memory unbounded.
const MAX_WATCHERS: usize = 128;

/// Per-session frame throttle. Pure over the `now` its callers pass in, so
/// the cadence is testable under a fake clock without sleeping.
struct Cadence {
    /// Current minimum spacing between content frames.
    interval: Duration,
    /// While set, the interval is backed off; it returns to baseline once
    /// this instant passes.
    backoff_until: Option<Instant>,
    /// Emit instants inside the rolling cap window, oldest first.
    emitted: VecDeque<Instant>,
    /// The last content frame (`snap`/`diff`/attach snapshot).
    last_content: Option<Instant>,
}

impl Cadence {
    fn new() -> Self {
        Cadence {
            interval: EMIT_INTERVAL,
            backoff_until: None,
            emitted: VecDeque::new(),
            last_content: None,
        }
    }

    fn prune(&mut self, now: Instant) {
        while let Some(oldest) = self.emitted.front() {
            if now.duration_since(*oldest) >= FRAME_CAP_WINDOW {
                self.emitted.pop_front();
            } else {
                break;
            }
        }
    }

    /// Frames of any type emitted inside the current rolling window.
    fn frames_in_window(&mut self, now: Instant) -> usize {
        self.prune(now);
        self.emitted.len()
    }

    /// The hard cap leaves no room for another frame right now.
    fn cap_reached(&mut self, now: Instant) -> bool {
        self.frames_in_window(now) >= FRAME_CAP
    }

    /// A content frame may go out now: the interval has elapsed since the
    /// last one and the rolling cap has room.
    fn ready(&mut self, now: Instant) -> bool {
        let spaced = self
            .last_content
            .is_none_or(|last| now.duration_since(last) >= self.interval);
        spaced && !self.cap_reached(now)
    }

    /// Record a frame that went out. `content` marks the ones the interval
    /// spaces (`snap`/`diff`); every frame counts toward the cap.
    fn record(&mut self, now: Instant, content: bool) {
        self.prune(now);
        self.emitted.push_back(now);
        if content {
            self.last_content = Some(now);
        }
    }

    /// The relay refused a frame under its quota: double the interval (up to
    /// [`MAX_EMIT_INTERVAL`]) and hold it for [`BACKOFF_HOLD`] from now.
    /// Returns whether the interval changed.
    fn rate_limited(&mut self, now: Instant) -> bool {
        let before = self.interval;
        self.interval = (self.interval * 2).min(MAX_EMIT_INTERVAL);
        self.backoff_until = Some(now + BACKOFF_HOLD);
        self.interval != before
    }

    /// Release an expired back-off. Returns whether the interval changed.
    fn tick(&mut self, now: Instant) -> bool {
        match self.backoff_until {
            Some(until) if now >= until => {
                self.backoff_until = None;
                let changed = self.interval != EMIT_INTERVAL;
                self.interval = EMIT_INTERVAL;
                changed
            }
            _ => false,
        }
    }

    fn snapshot(&mut self, session_id: &str, now: Instant) -> BroadcastCadence {
        let backing_off_ms = self
            .backoff_until
            .map(|until| until.saturating_duration_since(now).as_millis() as u64)
            .unwrap_or(0);
        BroadcastCadence {
            session_id: session_id.to_string(),
            interval_ms: self.interval.as_millis() as u64,
            base_interval_ms: EMIT_INTERVAL.as_millis() as u64,
            cap_per_minute: FRAME_CAP as u32,
            frames_last_minute: self.frames_in_window(now) as u32,
            backing_off_ms,
            reason: CADENCE_REASON,
        }
    }
}

/// The frame cadence of one shared session, for the owner's UI. Also the
/// payload of the `shell-broadcast-cadence` Tauri event.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastCadence {
    /// The session this describes.
    pub session_id: String,
    /// Current minimum spacing between content frames, in milliseconds.
    pub interval_ms: u64,
    /// The baseline interval the cadence returns to (1000).
    pub base_interval_ms: u64,
    /// Hard cap on frames per rolling minute (40).
    pub cap_per_minute: u32,
    /// Frames of any type sent inside the current rolling minute.
    pub frames_last_minute: u32,
    /// Milliseconds until a relay-induced back-off releases; 0 at baseline.
    pub backing_off_ms: u64,
    /// Why the stream is throttled at all: `"quota"` — frames are charged to
    /// the owner's per-key relay message quota.
    pub reason: &'static str,
}

struct BroadcastState {
    /// Watcher pubkey (hex) → last keepalive.
    watchers: HashMap<String, Instant>,
    seq: u64,
    /// Fresh per broadcast entry, so observers detect app restarts.
    epoch: String,
    /// Screen at the last emitted frame; `None` forces a snapshot.
    prev_screen: Option<vt100::Screen>,
    cadence: Cadence,
    dirty: bool,
}

impl BroadcastState {
    fn new() -> Self {
        BroadcastState {
            watchers: HashMap::new(),
            seq: 0,
            epoch: uuid::Uuid::new_v4().to_string(),
            prev_screen: None,
            cadence: Cadence::new(),
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
    if entry.cadence.ready(Instant::now()) {
        emit_screen_frame(session_id, entry);
    } else {
        // Interval not yet elapsed, or the cap is full: the sweeper flushes
        // the *latest* screen once the cadence allows — a diff against the
        // last emitted screen, so everything in between collapses.
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
    // Force the next content frame to be a full snapshot on the new grid;
    // that snapshot carries `dims` too, so when the cap is full the resize
    // frame is skipped rather than queued and the observer follows the grid
    // from the snapshot.
    entry.prev_screen = None;
    entry.dirty = true;
    let now = Instant::now();
    if entry.cadence.cap_reached(now) {
        return;
    }
    entry.seq += 1;
    let frame = build_frame(
        session_id,
        "resize",
        entry,
        Some((rows, cols)),
        None,
        Vec::new(),
    );
    entry.cadence.record(now, false);
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
    // The end frame is never throttled: it is the last thing observers see.
    entry.seq += 1;
    let frame = build_frame(session_id, "end", &entry, None, None, Vec::new());
    publish_frame(frame);
    emit_watchers(session_id, &[]);
}

/// The TS pump reports the relay's OK for a frame it published. A refusal
/// under the relay's quota (`rate-limited:` in the message) backs the
/// session's cadence off; anything else is ignored here — dropped frames
/// self-heal through observer resync.
pub fn note_publish_result(session_id: &str, accepted: bool, message: &str) {
    if accepted || !message.contains("rate-limited:") {
        return;
    }
    let Ok(mut map) = broadcasts().lock() else {
        return;
    };
    let Some(entry) = map.get_mut(session_id) else {
        return;
    };
    let now = Instant::now();
    if entry.cadence.rate_limited(now) {
        let snapshot = entry.cadence.snapshot(session_id, now);
        drop(map);
        emit_cadence(&snapshot);
    }
}

/// The current frame cadence of a session. A session that has never been
/// watched reports the baseline.
pub fn cadence(session_id: &str) -> BroadcastCadence {
    let now = Instant::now();
    broadcasts()
        .lock()
        .ok()
        .and_then(|mut map| {
            map.get_mut(session_id)
                .map(|entry| entry.cadence.snapshot(session_id, now))
        })
        .unwrap_or_else(|| Cadence::new().snapshot(session_id, now))
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
        // The bundle is what a joining watcher needs, so it bypasses the cap
        // but is counted against it: a join costs the session up to three
        // frames of the next minute.
        let now = Instant::now();
        for (i, chunk) in chunks.into_iter().enumerate() {
            entry.seq += 1;
            if let Some(event) = build_frame(
                session_id,
                "tail",
                entry,
                None,
                Some((i + 1, total)),
                chunk.to_vec(),
            ) {
                events.push(event);
                entry.cadence.record(now, false);
            }
        }
    }

    let (screen, rows, cols) = manager::clone_screen(session_id)?;
    entry.seq += 1;
    if let Some(event) = build_frame(
        session_id,
        "snap",
        entry,
        Some((rows, cols)),
        None,
        screen.contents_formatted(),
    ) {
        events.push(event);
    }
    entry.prev_screen = Some(screen);
    entry.cadence.record(Instant::now(), true);
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
            entry,
            None,
            None,
            screen.contents_diff(&prev),
        ),
        None => build_frame(
            session_id,
            "snap",
            entry,
            Some((rows, cols)),
            None,
            screen.contents_formatted(),
        ),
    };
    entry.prev_screen = Some(screen);
    entry.cadence.record(Instant::now(), true);
    entry.dirty = false;
    publish_frame(frame);
}

/// Build + sign one kind:24311 frame event; `None` (with a log line) when the
/// session's gate or signing fails. `seq` and `epoch` come from `entry` (the
/// caller bumps `seq` first); every frame also carries the session's current
/// `cadence` (interval in ms) so an observer can state the rate it gets.
fn build_frame(
    session_id: &str,
    frame_type: &str,
    entry: &BroadcastState,
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
        tag(&["seq", &entry.seq.to_string()]),
        tag(&["epoch", &entry.epoch]),
        tag(&["cadence", &entry.cadence.interval.as_millis().to_string()]),
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

fn emit_cadence(cadence: &BroadcastCadence) {
    if let Some(app) = APP.get() {
        let _ = app.emit(CADENCE_EVENT, cadence);
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
                std::thread::sleep(SWEEP_TICK);
                sweep();
            });
    });
}

fn sweep() {
    let Ok(mut map) = broadcasts().lock() else {
        return;
    };
    let now = Instant::now();
    let mut roster_changes: Vec<(String, Vec<String>)> = Vec::new();
    let mut cadence_changes: Vec<BroadcastCadence> = Vec::new();
    for (session_id, entry) in map.iter_mut() {
        let before = entry.watchers.len();
        entry
            .watchers
            .retain(|_, seen| seen.elapsed() < WATCHER_TTL);
        if entry.watchers.len() != before {
            roster_changes.push((session_id.clone(), entry.watchers.keys().cloned().collect()));
        }
        if entry.cadence.tick(now) {
            cadence_changes.push(entry.cadence.snapshot(session_id, now));
        }
        if !entry.watchers.is_empty() && entry.dirty && entry.cadence.ready(now) {
            emit_screen_frame(session_id, entry);
        }
    }
    drop(map);
    for (session_id, watchers) in roster_changes {
        emit_watchers(&session_id, &watchers);
    }
    for cadence in &cadence_changes {
        emit_cadence(cadence);
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

    const MS: Duration = Duration::from_millis(1);

    /// Drive the cadence the way `on_output` + `sweep` do: every update
    /// either emits (when ready) or marks dirty, and a sweep at the same
    /// instant flushes a dirty entry that has become ready.
    fn drive(cadence: &mut Cadence, t0: Instant, updates: impl Iterator<Item = u64>) -> usize {
        let mut emitted = 0;
        for ms in updates {
            let now = t0 + MS * ms as u32;
            cadence.tick(now);
            if cadence.ready(now) {
                cadence.record(now, true);
                emitted += 1;
            }
        }
        emitted
    }

    #[test]
    fn hundred_updates_in_one_second_collapse_to_one_frame_plus_the_boundary() {
        let t0 = Instant::now();
        let mut cadence = Cadence::new();
        // 100 updates at 10 ms spacing inside [0, 1 s): the first goes out,
        // the rest coalesce (dirty) …
        let inside = drive(&mut cadence, t0, (0..100).map(|i| i * 10));
        assert_eq!(inside, 1, "only the first update inside the interval emits");
        // … and the sweeper flushes exactly one frame at the boundary.
        assert!(!cadence.ready(t0 + MS * 999));
        assert!(cadence.ready(t0 + MS * 1000));
        cadence.record(t0 + MS * 1000, true);
        assert!(!cadence.ready(t0 + MS * 1500));
        assert_eq!(cadence.frames_in_window(t0 + MS * 1500), 2);
    }

    #[test]
    fn sixty_seconds_of_steady_updates_stay_under_the_cap() {
        let t0 = Instant::now();
        let mut cadence = Cadence::new();
        // An update every 10 ms for 60 s would be 60 frames at 1 frame/s;
        // the rolling cap holds it to 40.
        let emitted = drive(&mut cadence, t0, (0..6_000).map(|i| i * 10));
        assert_eq!(emitted, FRAME_CAP);
        assert!(cadence.cap_reached(t0 + MS * 59_999));
        // The window rolls: 60 s after the first frame there is room again.
        assert!(cadence.ready(t0 + MS * 60_000));
        // A steady second minute is capped the same way.
        let second = drive(&mut cadence, t0, (6_000..12_000).map(|i| i * 10));
        assert!(second <= FRAME_CAP, "second minute emitted {second}");
        let snapshot = cadence.snapshot("s", t0 + MS * 120_000);
        assert!(snapshot.frames_last_minute as usize <= FRAME_CAP);
        assert_eq!(snapshot.cap_per_minute as usize, FRAME_CAP);
    }

    #[test]
    fn rate_limited_ok_doubles_the_interval_and_it_decays_back() {
        let t0 = Instant::now();
        let mut cadence = Cadence::new();
        assert_eq!(cadence.interval, EMIT_INTERVAL);
        cadence.record(t0, true);
        // One refusal: 1 s → 2 s, so the frame that was due at +1 s waits.
        assert!(cadence.rate_limited(t0 + MS * 100));
        assert_eq!(cadence.interval, Duration::from_secs(2));
        assert!(!cadence.ready(t0 + MS * 1000));
        assert!(cadence.ready(t0 + MS * 2000));
        // Repeated refusals climb to the 4 s ceiling and stay there.
        assert!(cadence.rate_limited(t0 + MS * 2000));
        assert_eq!(cadence.interval, Duration::from_secs(4));
        assert!(!cadence.rate_limited(t0 + MS * 3000));
        assert_eq!(cadence.interval, MAX_EMIT_INTERVAL);
        let snapshot = cadence.snapshot("s", t0 + MS * 3000);
        assert_eq!(snapshot.interval_ms, 4000);
        assert_eq!(snapshot.base_interval_ms, 1000);
        assert_eq!(snapshot.backing_off_ms, 60_000);
        assert_eq!(snapshot.reason, "quota");
        // The hold runs 60 s from the *last* refusal (at +3 s) …
        assert!(!cadence.tick(t0 + MS * 62_999));
        assert_eq!(cadence.interval, MAX_EMIT_INTERVAL);
        // … then the interval returns to baseline in one step.
        assert!(cadence.tick(t0 + MS * 63_000));
        assert_eq!(cadence.interval, EMIT_INTERVAL);
        assert_eq!(cadence.snapshot("s", t0 + MS * 63_000).backing_off_ms, 0);
        assert!(!cadence.tick(t0 + MS * 64_000));
    }

    #[test]
    fn publish_result_feeds_the_session_cadence() {
        let session_id = "cadence-test-session";
        broadcasts()
            .lock()
            .expect("test registry lock")
            .insert(session_id.to_string(), BroadcastState::new());
        // Accepted, and refused for any other reason: no change.
        note_publish_result(session_id, true, "");
        note_publish_result(
            session_id,
            false,
            "invalid: unknown shared-terminal frame type",
        );
        assert_eq!(cadence(session_id).interval_ms, 1000);
        note_publish_result(
            session_id,
            false,
            "rate-limited: quota exceeded; retry in 1s",
        );
        let after = cadence(session_id);
        assert_eq!(after.interval_ms, 2000);
        assert!(after.backing_off_ms > 0 && after.backing_off_ms <= 60_000);
        assert_eq!(after.session_id, session_id);
        // An unknown session reports the baseline rather than an error.
        assert_eq!(cadence("never-watched").interval_ms, 1000);
        assert_eq!(cadence("never-watched").reason, "quota");
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
