//! Sharing a session's Browser with the session (C5, `WIRE-C5.md` §3–§6):
//! the hosting desktop announces the preview (kind 30626), streams frames
//! (kind 24321) while someone watches (kind 24320), and publishes dated
//! snapshots (kind 44253).
//!
//! Modelled on the NIP-ST broadcaster (`shell_sessions/broadcast.rs`):
//!
//! - **Announces and snapshots** are durable, so they go straight to the
//!   relay over HTTP (`relay::submit_event`), signed by this desktop's
//!   identity.
//! - **Frames** are ephemeral and WS-only on the relay, so they are signed
//!   here and handed to the TS pump (`SessionPreviewBroadcastPump.tsx`)
//!   through [`BROADCAST_PUBLISH_EVENT`]; the pump publishes them over the
//!   app's relay WebSocket and reports each OK back through
//!   `session_preview_share_note_publish`. Watch events arrive the other way:
//!   the pump subscribes and calls `session_preview_share_watch`.
//!
//! Every pacing decision is pure over a millisecond clock its caller passes
//! in ([`Cadence`], [`Watchers`], [`AnnounceDebounce`], [`ShareEntry::plan_tick`]),
//! so the budget, back-off, expiry and debounce are unit-tested without
//! sleeping. One sweeper task drives them every [`SWEEP_TICK`].
//!
//! # What the announce says
//!
//! - `status open` while C4's record is `loading`/`ready` and the person's
//!   surface configured a sessionRef and Share is on; `closed` (no `page`,
//!   no `title`) once it is absent, closed by the person, unavailable or no
//!   longer shared — only if this desktop announced it open.
//! - `viewport`: the Browser slot's size in logical points (the size the page
//!   lays out at) when a slot is mounted; otherwise the pixel size of the
//!   last picture taken of it; otherwise a picture is taken to measure it.
//!   Never a made-up default.
//! - `provider`: this machine's coding-session provider key, read from the
//!   provider record for the active relay; the UI's value only when that
//!   record is absent. Neither → no `provider` tag (and no snapshots, which
//!   require one).
//!
//! Frames flow only while at least one watcher is live (45 s keepalive
//! expiry), sharing is on, and the preview is ready and visible: hidden or
//! occluded sends one `t=paused`; the preview closing sends one `t=end`.
//!
//! **Share off publishes nothing**: no open announce (one bare `closed` if
//! an open went out earlier, with no `page` or `title`), no frames, no
//! snapshots — so the strip's "Local only · not shared" is true.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use beekeeper_core_pkg::coding_session_command::coding_session_target_key;
use beekeeper_core_pkg::session_preview::{
    redact_page_url, PreviewStatus as AnnounceStatus, PreviewStream, MAX_PREVIEW_TITLE_BYTES,
    PREVIEW_ANNOUNCE_MIN_INTERVAL_SECS,
};
use beekeeper_core_pkg::surface_watch::{
    redact_free_text, SurfaceFrameType, MAX_SURFACE_DIMENSION,
};
use nostr::PublicKey;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use super::{with_record, Binding, PreviewError, PreviewRecord, PreviewStatus, Unavailable};

#[path = "broadcast_capture.rs"]
mod capture;
#[path = "broadcast_pacing.rs"]
mod pacing;
pub(crate) use pacing::{AnnounceDebounce, AnnounceSpec, Cadence, Watchers};
#[path = "share_commands.rs"]
pub mod share_commands;

/// Event: a channel's [`SessionPreviewShareState`] changed.
pub const SHARE_STATE_EVENT: &str = "session-preview://share-state";
/// Event: `{channelId, event}` — a signed frame event JSON for the pump.
pub const BROADCAST_PUBLISH_EVENT: &str = "session-preview://broadcast-publish";

/// Bound on live watchers per preview (a slow leak of spoofed watch events
/// must not grow memory).
pub(crate) const MAX_WATCHERS: usize = 128;
/// The frame interval never backs off beyond this.
pub(crate) const MAX_BACKOFF_CADENCE_MS: u64 = 8_000;
/// How long a `rate-limited:` refusal holds the back-off.
pub(crate) const BACKOFF_HOLD_MS: u64 = 60_000;
/// Rolling window for [`SURFACE_FRAME_MAX_PER_MIN`].
pub(crate) const CAP_WINDOW_MS: u64 = 60_000;
/// Minimum spacing between two announces of one preview.
pub(crate) const ANNOUNCE_MIN_INTERVAL_MS: u64 = PREVIEW_ANNOUNCE_MIN_INTERVAL_SECS * 1_000;
/// The sweeper's tick.
const SWEEP_TICK: Duration = Duration::from_millis(250);

/// `unavailable.code` sentences (V-CONTRACT).
const NO_SESSION_REF_SENTENCE: &str =
    "This session has no session reference, so its Browser cannot be shared.";
pub(crate) const SHARE_OFF_SENTENCE: &str =
    "Sharing is off for this Browser, so nothing from it is published.";
pub(crate) const NO_PROVIDER_SENTENCE: &str =
    "This computer's provider key is not known, so snapshots cannot name the machine.";

/// Unix milliseconds now.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

// ------------------------------------------------------------- the preview

/// What the broadcaster needs from C4's [`PreviewRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewFacts {
    pub(crate) status: PreviewStatus,
    pub(crate) url: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) hidden: bool,
    pub(crate) occluded: bool,
    /// `cs-target` of an agent binding; `None` for a person or no binding.
    pub(crate) target_key: Option<String>,
    /// The slot's size in logical points, when one is mounted.
    pub(crate) slot: Option<(u32, u32)>,
    pub(crate) not_macos: bool,
}

impl PreviewFacts {
    pub(crate) fn of(record: &PreviewRecord) -> Self {
        let target_key = match &record.binding {
            Binding::Agent { target, .. } => Some(coding_session_target_key(target)),
            Binding::Person { .. } | Binding::None => None,
        };
        let slot = record
            .slot
            .and_then(|rect| clamp_dims(rect.width.round(), rect.height.round()));
        Self {
            status: record.status,
            url: record.url.clone(),
            title: record.title.clone(),
            hidden: record.hidden,
            occluded: record.occluded,
            target_key,
            slot,
            not_macos: record
                .unavailable
                .as_ref()
                .is_some_and(|reason| reason.code == Unavailable::NOT_MACOS.code),
        }
    }

    /// A page is open (loading or loaded).
    pub(crate) fn open(&self) -> bool {
        matches!(self.status, PreviewStatus::Loading | PreviewStatus::Ready)
    }
}

/// Clamp floating dimensions to the wire's `WxH` range, `None` when empty.
pub(crate) fn clamp_dims(width: f64, height: f64) -> Option<(u32, u32)> {
    if !(width >= 1.0 && height >= 1.0) {
        return None;
    }
    let max = MAX_SURFACE_DIMENSION as f64;
    Some((width.min(max) as u32, height.min(max) as u32))
}

/// A title that may travel: redacted, one line, at most
/// [`MAX_PREVIEW_TITLE_BYTES`] (cut on a character boundary).
pub(crate) fn clean_title(raw: &str) -> String {
    let one_line: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut title = redact_free_text(one_line.trim());
    if title.len() > MAX_PREVIEW_TITLE_BYTES {
        let mut cut = MAX_PREVIEW_TITLE_BYTES;
        while !title.is_char_boundary(cut) {
            cut -= 1;
        }
        title.truncate(cut);
    }
    title.trim_end().to_string()
}

/// The `page` for a committed URL (`local:/` when nothing safe remains).
pub(crate) fn page_for(url: Option<&str>) -> String {
    url.and_then(redact_page_url)
        .unwrap_or_else(|| "local:/".to_string())
}

// ------------------------------------------------------------- the entry

/// What the person's surface configured for one preview.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ShareConfig {
    pub(crate) session_ref: Option<String>,
    pub(crate) share: bool,
    /// The machine's provider key: Rust's record first, the UI's otherwise.
    pub(crate) provider: Option<PublicKey>,
}

/// One action the sweeper must carry out after [`ShareEntry::plan_tick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TickAction {
    /// Take a picture; `force` sends it even if unchanged.
    Capture {
        force: bool,
        last_hash: Option<Vec<u8>>,
    },
    /// Send this signed-to-be paused/end frame header.
    Frame(FramePlan),
    /// Capture stopped: no live watchers.
    CaptureStopped,
    /// Publish this announce.
    Announce(AnnounceSpec),
    /// The share state changed.
    StateChanged,
}

/// A frame header to sign (everything but the content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FramePlan {
    pub(crate) frame_type: SurfaceFrameType,
    pub(crate) seq: u64,
    pub(crate) epoch: u64,
    pub(crate) cadence_ms: u64,
    pub(crate) dims: (u32, u32),
    pub(crate) captured_at_ms: u64,
}

/// A picture ready to send (see `broadcast_capture.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedFrame {
    /// sha256 of the downscaled RGB pixels.
    pub(crate) hash: Vec<u8>,
    /// Base64 JPEG content.
    pub(crate) content: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Everything the broadcaster keeps for one preview.
#[derive(Debug, Clone)]
pub(crate) struct ShareEntry {
    pub(crate) config: ShareConfig,
    pub(crate) watchers: Watchers,
    pub(crate) cadence: Cadence,
    pub(crate) debounce: AnnounceDebounce,
    /// Status of the last 30626 the relay accepted from us.
    pub(crate) announced: Option<AnnounceStatus>,
    pub(crate) facts: Option<PreviewFacts>,
    pub(crate) epoch: u64,
    pub(crate) seq: u64,
    pub(crate) last_hash: Option<Vec<u8>>,
    pub(crate) resync_pending: bool,
    pub(crate) last_frame_at: Option<u64>,
    pub(crate) last_dims: Option<(u32, u32)>,
    /// A frame or `paused` went out in this epoch and no `end` yet.
    pub(crate) stream_open: bool,
    pub(crate) paused_sent: bool,
    pub(crate) capture_active: bool,
    pub(crate) in_flight: bool,
}

impl ShareEntry {
    pub(crate) fn new(now: u64) -> Self {
        Self {
            config: ShareConfig {
                share: true,
                ..ShareConfig::default()
            },
            watchers: Watchers::default(),
            cadence: Cadence::new(),
            debounce: AnnounceDebounce::default(),
            announced: None,
            facts: None,
            epoch: now,
            seq: 0,
            last_hash: None,
            resync_pending: false,
            last_frame_at: None,
            last_dims: None,
            stream_open: false,
            paused_sent: false,
            capture_active: false,
            in_flight: false,
        }
    }

    /// The open announce the current config and facts call for, or `None`.
    ///
    /// Share off is `None`: nothing about the page leaves the machine, and
    /// the debounce turns an earlier open into one bare `closed`.
    pub(crate) fn desired_announce(&self, channel_id: Uuid) -> Option<AnnounceSpec> {
        if !self.config.share {
            return None;
        }
        let facts = self.facts.as_ref()?;
        let session_ref = self.config.session_ref.clone()?;
        if !facts.open() {
            return None;
        }
        Some(AnnounceSpec {
            channel_id,
            session_ref,
            status: AnnounceStatus::Open,
            target_key: facts.target_key.clone(),
            provider: self.config.provider,
            page: Some(page_for(facts.url.as_deref())),
            title: Some(clean_title(facts.title.as_deref().unwrap_or(""))),
            viewport: facts.slot.or(self.last_dims),
            stream: PreviewStream::Frames,
        })
    }

    fn next_header(
        &mut self,
        frame_type: SurfaceFrameType,
        dims: (u32, u32),
        now: u64,
    ) -> FramePlan {
        self.seq += 1;
        FramePlan {
            frame_type,
            seq: self.seq,
            epoch: self.epoch,
            cadence_ms: self.cadence.interval_ms,
            dims,
            captured_at_ms: now,
        }
    }

    fn start_epoch(&mut self, now: u64) {
        // Strictly newer than the last epoch even within one millisecond.
        self.epoch = now.max(self.epoch + 1);
        self.seq = 0;
    }

    /// One sweeper step over `facts` (C4's record, read just before).
    pub(crate) fn plan_tick(&mut self, facts: PreviewFacts, now: u64) -> Vec<TickAction> {
        let mut actions = Vec::new();
        self.facts = Some(facts.clone());
        if self.watchers.expire(now) {
            actions.push(TickAction::StateChanged);
        }
        if self.cadence.tick(now) {
            actions.push(TickAction::StateChanged);
        }
        if let Some(spec) = self.debounce.due(now) {
            actions.push(TickAction::Announce(spec));
        }
        if !self.config.share {
            // Share off: no frame of any kind, no pictures, and nobody left
            // watching (a watch is refused while off). The close announce
            // above is the whole notice.
            if !self.watchers.is_empty() {
                self.watchers = Watchers::default();
                actions.push(TickAction::StateChanged);
            }
            if self.capture_active || self.stream_open {
                self.capture_active = false;
                self.stream_open = false;
                self.paused_sent = false;
                self.last_hash = None;
                actions.push(TickAction::CaptureStopped);
                actions.push(TickAction::StateChanged);
            }
            return actions;
        }
        if self.watchers.is_empty() {
            if self.capture_active {
                self.capture_active = false;
                self.stream_open = false;
                self.paused_sent = false;
                actions.push(TickAction::CaptureStopped);
                actions.push(TickAction::StateChanged);
            }
            return actions;
        }
        if !facts.open() {
            if self.stream_open {
                // `end` is never throttled: it is the last thing a watcher sees.
                let dims = self.last_dims.or(facts.slot).unwrap_or((1, 1));
                let plan = self.next_header(SurfaceFrameType::End, dims, now);
                self.cadence.record(now);
                actions.push(TickAction::Frame(plan));
                self.stream_open = false;
                self.paused_sent = false;
                self.last_hash = None;
                self.capture_active = false;
                actions.push(TickAction::StateChanged);
            }
            return actions;
        }
        if !self.capture_active {
            self.capture_active = true;
            if !self.stream_open {
                self.start_epoch(now);
            }
        }
        if facts.hidden || facts.occluded {
            if !self.paused_sent {
                if let Some(dims) = self.last_dims.or(facts.slot) {
                    let plan = self.next_header(SurfaceFrameType::Paused, dims, now);
                    self.cadence.record(now);
                    actions.push(TickAction::Frame(plan));
                    self.paused_sent = true;
                    self.stream_open = true;
                    // The next picture after a pause is always sent.
                    self.resync_pending = true;
                    actions.push(TickAction::StateChanged);
                }
            }
            return actions;
        }
        if facts.status != PreviewStatus::Ready || self.in_flight || !self.cadence.ready(now) {
            return actions;
        }
        self.cadence.attempt(now);
        self.in_flight = true;
        actions.push(TickAction::Capture {
            force: self.resync_pending,
            last_hash: self.last_hash.clone(),
        });
        actions
    }

    /// A capture finished. Returns the header to send with the prepared
    /// content, or `None` when nothing goes out (unchanged and no resync, or
    /// the stream stopped meanwhile).
    pub(crate) fn finish_capture(
        &mut self,
        prepared: Option<&PreparedFrame>,
        now: u64,
    ) -> Option<FramePlan> {
        self.in_flight = false;
        let prepared = prepared?;
        if !self.capture_active || self.watchers.is_empty() {
            return None;
        }
        let unchanged = self.last_hash.as_deref() == Some(prepared.hash.as_slice());
        if unchanged && !self.resync_pending {
            return None;
        }
        let plan = self.next_header(
            SurfaceFrameType::Frame,
            (prepared.width, prepared.height),
            now,
        );
        self.cadence.record(now);
        self.last_hash = Some(prepared.hash.clone());
        self.resync_pending = false;
        self.paused_sent = false;
        self.stream_open = true;
        self.last_frame_at = Some(now);
        self.last_dims = Some((prepared.width, prepared.height));
        Some(plan)
    }

    /// Apply a watch action (already checked against the config). Returns
    /// whether the share state changed and whether a snapshot was admitted.
    pub(crate) fn apply_watch(
        &mut self,
        watcher: &str,
        action: WatchAction,
        now: u64,
    ) -> Result<WatchOutcome, PreviewError> {
        let mut outcome = WatchOutcome::default();
        match action {
            WatchAction::Stop => {
                outcome.state_changed = self.watchers.remove(watcher);
            }
            WatchAction::Watch | WatchAction::Resync => {
                let is_new = self.watchers.touch(watcher, now).map_err(|_| {
                    PreviewError::new(
                        "preview_share_unavailable",
                        "Too many people are watching this Browser.",
                    )
                })?;
                // A new watcher has seen no frame yet; a resync asks for one.
                if is_new || action == WatchAction::Resync {
                    self.resync_pending = true;
                }
                outcome.state_changed = is_new;
            }
            WatchAction::Snapshot => {
                outcome.snapshot = self.watchers.allow_snapshot(watcher, now);
            }
        }
        Ok(outcome)
    }

    /// The share state for the UI.
    pub(crate) fn share_state(&mut self, channel_id: &str, now: u64) -> SessionPreviewShareState {
        let not_macos = self.facts.as_ref().is_some_and(|facts| facts.not_macos)
            || cfg!(not(target_os = "macos"));
        let unavailable = if not_macos {
            Some(ShareUnavailable {
                code: Unavailable::NOT_MACOS.code,
                sentence: Unavailable::NOT_MACOS.sentence,
            })
        } else if self.config.session_ref.is_none() {
            Some(ShareUnavailable {
                code: "no_session_ref",
                sentence: NO_SESSION_REF_SENTENCE,
            })
        } else if self.config.provider.is_none() {
            Some(ShareUnavailable {
                code: "no_provider",
                sentence: NO_PROVIDER_SENTENCE,
            })
        } else {
            None
        };
        SessionPreviewShareState {
            channel_id: channel_id.to_string(),
            session_ref: self.config.session_ref.clone(),
            share: self.config.share,
            announced: match self.announced {
                Some(AnnounceStatus::Open) => "open",
                Some(AnnounceStatus::Closed) => "closed",
                None => "none",
            },
            stream: if self.config.share { "frames" } else { "none" },
            watchers: self.watchers.list(),
            cadence_ms: self.cadence.interval_ms,
            frames_last_minute: self.cadence.frames_in_window(now),
            last_frame_at: self.last_frame_at,
            unavailable,
        }
    }
}

/// A 24320 action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchAction {
    Watch,
    Stop,
    Resync,
    Snapshot,
}

impl WatchAction {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "watch" => Some(Self::Watch),
            "stop" => Some(Self::Stop),
            "resync" => Some(Self::Resync),
            "snapshot" => Some(Self::Snapshot),
            _ => None,
        }
    }
}

/// What a watch changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WatchOutcome {
    pub(crate) state_changed: bool,
    pub(crate) snapshot: bool,
}

/// `unavailable` in [`SessionPreviewShareState`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShareUnavailable {
    /// `no_session_ref`, `no_provider` or `not_macos`.
    pub code: &'static str,
    /// The sentence the UI shows.
    pub sentence: &'static str,
}

/// `SessionPreviewShareState` (V-CONTRACT, VB → VC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPreviewShareState {
    /// Channel id.
    pub channel_id: String,
    /// The session's umbrella sessionRef, when configured.
    pub session_ref: Option<String>,
    /// The person's Share toggle.
    pub share: bool,
    /// `open`, `closed` or `none`: the last 30626 status the relay accepted.
    pub announced: &'static str,
    /// `frames` (share on) or `none` (share off: nothing is published).
    pub stream: &'static str,
    /// Live watcher pubkeys (hex).
    pub watchers: Vec<String>,
    /// Current frame spacing in ms.
    pub cadence_ms: u64,
    /// Frames sent in the rolling minute.
    pub frames_last_minute: u32,
    /// Unix ms of the last frame sent.
    pub last_frame_at: Option<u64>,
    /// Why sharing cannot work fully, or `None`.
    pub unavailable: Option<ShareUnavailable>,
}

// --------------------------------------------------------------- registry

static APP: OnceLock<AppHandle> = OnceLock::new();
static ENTRIES: Mutex<Option<HashMap<String, ShareEntry>>> = Mutex::new(None);
static SWEEPER: OnceLock<()> = OnceLock::new();

/// Read or change one channel's share entry (created on first touch).
/// Never call C4's `with_record` or any view op while inside `f`.
pub(crate) fn with_entry<T>(channel_id: &str, f: impl FnOnce(&mut ShareEntry) -> T) -> T {
    let mut guard = ENTRIES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let entry = map
        .entry(channel_id.to_string())
        .or_insert_with(|| ShareEntry::new(now_ms()));
    f(entry)
}

/// Change an existing entry only.
fn with_existing<T>(channel_id: &str, f: impl FnOnce(&mut ShareEntry) -> T) -> Option<T> {
    let mut guard = ENTRIES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.as_mut()?.get_mut(channel_id).map(f)
}

fn channel_ids() -> Vec<String> {
    let guard = ENTRIES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .as_ref()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

fn remember_app(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// C4's facts for a channel. Takes C4's registry lock; never call while
/// holding [`ENTRIES`].
pub(crate) fn read_facts(channel_id: &str) -> PreviewFacts {
    with_record(channel_id, |record| PreviewFacts::of(record))
}

/// Emit [`SHARE_STATE_EVENT`] for a channel and return the state.
pub(crate) fn emit_share_state(channel_id: &str) -> SessionPreviewShareState {
    let now = now_ms();
    let state = with_entry(channel_id, |entry| entry.share_state(channel_id, now));
    if let Some(app) = APP.get() {
        if let Err(error) = app.emit(SHARE_STATE_EVENT, &state) {
            eprintln!("session-preview-broadcast: emit share state failed: {error}");
        }
    }
    state
}

/// Hook from C4's `emit_state`: the preview's state changed. Re-offers the
/// announce; frames follow on the next sweep. Cheap for a channel nobody
/// configured for sharing.
pub fn on_preview_state(app: &AppHandle, channel_id: &str) {
    remember_app(app);
    if with_existing(channel_id, |_| ()).is_none() {
        return;
    }
    let facts = read_facts(channel_id);
    reoffer(channel_id, Some(facts));
}

/// Offer the current announce for a channel (after a config or state
/// change) and publish it if the debounce lets it go now.
pub(crate) fn reoffer(channel_id: &str, facts: Option<PreviewFacts>) {
    let Ok(channel_uuid) = Uuid::parse_str(channel_id) else {
        return;
    };
    let now = now_ms();
    let spec = with_entry(channel_id, |entry| {
        if let Some(facts) = facts {
            entry.facts = Some(facts);
        }
        let desired = entry.desired_announce(channel_uuid);
        entry.debounce.offer(desired, now)
    });
    if let Some(spec) = spec {
        spawn_announce(channel_id, spec);
    }
}

fn spawn_announce(channel_id: &str, spec: AnnounceSpec) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let channel = channel_id.to_string();
    tauri::async_runtime::spawn(async move {
        capture::publish_announce(&app, &channel, spec).await;
    });
}

/// The pump reports the relay's OK for a frame it published. A
/// `rate-limited:` refusal backs the cadence off.
pub(crate) fn note_publish(channel_id: &str, accepted: bool, message: &str) {
    if accepted || !message.contains("rate-limited:") {
        return;
    }
    let now = now_ms();
    let changed = with_existing(channel_id, |entry| entry.cadence.rate_limited(now));
    if changed == Some(true) {
        emit_share_state(channel_id);
    }
}

/// Start the one sweeper task (idempotent).
pub(crate) fn ensure_sweeper(app: &AppHandle) {
    remember_app(app);
    SWEEPER.get_or_init(|| {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(SWEEP_TICK).await;
                sweep(&app);
            }
        });
    });
}

fn sweep(app: &AppHandle) {
    for channel_id in channel_ids() {
        let facts = read_facts(&channel_id);
        let now = now_ms();
        let actions = with_entry(&channel_id, |entry| entry.plan_tick(facts, now));
        let mut state_changed = false;
        for action in actions {
            match action {
                TickAction::StateChanged => state_changed = true,
                TickAction::CaptureStopped => {
                    eprintln!(
                        "session-preview-broadcast: capture stopped for {channel_id}: no live watchers"
                    );
                }
                TickAction::Announce(spec) => spawn_announce(&channel_id, spec),
                TickAction::Frame(plan) => capture::emit_frame(app, &channel_id, &plan, ""),
                TickAction::Capture { force, last_hash } => {
                    let app = app.clone();
                    let channel = channel_id.clone();
                    tauri::async_runtime::spawn(async move {
                        capture::capture_frame(&app, &channel, last_hash, force).await;
                    });
                }
            }
        }
        if state_changed {
            emit_share_state(&channel_id);
        }
    }
}

#[cfg(test)]
#[path = "broadcast_tests.rs"]
mod tests;
