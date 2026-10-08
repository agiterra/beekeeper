//! Pure pacing for the preview broadcaster: the frame [`Cadence`], live
//! [`Watchers`], and the announce [`AnnounceDebounce`]. Every function takes
//! the clock (unix ms) from its caller, so all of it is unit-tested.

use std::collections::{HashMap, VecDeque};

use beekeeper_core_pkg::session_preview::{
    PreviewStatus as AnnounceStatus, PreviewStream, SessionPreviewAnnounce,
};
use beekeeper_core_pkg::surface_watch::{
    PREVIEW_FRAME_BASE_CADENCE_MS, SURFACE_FRAME_MAX_PER_MIN,
    SURFACE_SNAPSHOT_REQUEST_MIN_INTERVAL_MS, SURFACE_WATCH_EXPIRY_MS,
};
use nostr::PublicKey;
use uuid::Uuid;

use super::{
    ANNOUNCE_MIN_INTERVAL_MS, BACKOFF_HOLD_MS, CAP_WINDOW_MS, MAX_BACKOFF_CADENCE_MS, MAX_WATCHERS,
};

// ---------------------------------------------------------------- cadence

/// Per-preview frame throttle: base spacing [`PREVIEW_FRAME_BASE_CADENCE_MS`],
/// at most [`SURFACE_FRAME_MAX_PER_MIN`] frames per rolling minute, doubled
/// (up to [`MAX_BACKOFF_CADENCE_MS`]) for [`BACKOFF_HOLD_MS`] after a
/// `rate-limited:` refusal.
#[derive(Debug, Clone)]
pub(crate) struct Cadence {
    pub(crate) interval_ms: u64,
    backoff_until: Option<u64>,
    emitted: VecDeque<u64>,
    last_attempt: Option<u64>,
}

impl Cadence {
    pub(crate) fn new() -> Self {
        Self {
            interval_ms: PREVIEW_FRAME_BASE_CADENCE_MS,
            backoff_until: None,
            emitted: VecDeque::new(),
            last_attempt: None,
        }
    }

    fn prune(&mut self, now: u64) {
        while let Some(oldest) = self.emitted.front() {
            if now.saturating_sub(*oldest) >= CAP_WINDOW_MS {
                self.emitted.pop_front();
            } else {
                break;
            }
        }
    }

    /// Frames sent inside the rolling minute.
    pub(crate) fn frames_in_window(&mut self, now: u64) -> u32 {
        self.prune(now);
        self.emitted.len() as u32
    }

    /// The per-minute budget has no room right now.
    pub(crate) fn cap_reached(&mut self, now: u64) -> bool {
        self.frames_in_window(now) >= SURFACE_FRAME_MAX_PER_MIN
    }

    /// A capture may start now: the interval elapsed since the last attempt
    /// and the budget has room.
    pub(crate) fn ready(&mut self, now: u64) -> bool {
        let spaced = self
            .last_attempt
            .is_none_or(|last| now.saturating_sub(last) >= self.interval_ms);
        spaced && !self.cap_reached(now)
    }

    /// A capture started (whether or not it produces a frame).
    pub(crate) fn attempt(&mut self, now: u64) {
        self.last_attempt = Some(now);
    }

    /// A frame went out; it counts against the budget.
    pub(crate) fn record(&mut self, now: u64) {
        self.prune(now);
        self.emitted.push_back(now);
    }

    /// The relay refused a frame under its quota. Returns whether the
    /// interval changed.
    pub(crate) fn rate_limited(&mut self, now: u64) -> bool {
        let before = self.interval_ms;
        self.interval_ms = (self.interval_ms * 2).min(MAX_BACKOFF_CADENCE_MS);
        self.backoff_until = Some(now + BACKOFF_HOLD_MS);
        self.interval_ms != before
    }

    /// Release an expired back-off. Returns whether the interval changed.
    pub(crate) fn tick(&mut self, now: u64) -> bool {
        match self.backoff_until {
            Some(until) if now >= until => {
                self.backoff_until = None;
                let changed = self.interval_ms != PREVIEW_FRAME_BASE_CADENCE_MS;
                self.interval_ms = PREVIEW_FRAME_BASE_CADENCE_MS;
                changed
            }
            _ => false,
        }
    }
}

// --------------------------------------------------------------- watchers

/// Why a watch was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WatchRefusal {
    /// [`MAX_WATCHERS`] are already live.
    Full,
}

/// Live watchers (pubkey hex → last keepalive, unix ms) and the time of each
/// watcher's last answered snapshot request.
#[derive(Debug, Clone, Default)]
pub(crate) struct Watchers {
    seen: HashMap<String, u64>,
    snapshot_requests: HashMap<String, u64>,
}

impl Watchers {
    /// Register or refresh a watcher. `Ok(true)` when it is new.
    pub(crate) fn touch(&mut self, pubkey: &str, now: u64) -> Result<bool, WatchRefusal> {
        if !self.seen.contains_key(pubkey) && self.seen.len() >= MAX_WATCHERS {
            return Err(WatchRefusal::Full);
        }
        Ok(self.seen.insert(pubkey.to_string(), now).is_none())
    }

    /// Remove a watcher at once. Returns whether it was live.
    pub(crate) fn remove(&mut self, pubkey: &str) -> bool {
        self.seen.remove(pubkey).is_some()
    }

    /// Drop watchers silent for [`SURFACE_WATCH_EXPIRY_MS`]. Returns whether
    /// any expired.
    pub(crate) fn expire(&mut self, now: u64) -> bool {
        let before = self.seen.len();
        self.seen
            .retain(|_, seen| now.saturating_sub(*seen) < SURFACE_WATCH_EXPIRY_MS);
        self.snapshot_requests
            .retain(|_, at| now.saturating_sub(*at) < SURFACE_SNAPSHOT_REQUEST_MIN_INTERVAL_MS);
        self.seen.len() != before
    }

    /// No watcher is live.
    pub(crate) fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// Live watcher pubkeys, sorted.
    pub(crate) fn list(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.seen.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Admit a snapshot request: at most one per watcher per
    /// [`SURFACE_SNAPSHOT_REQUEST_MIN_INTERVAL_MS`].
    pub(crate) fn allow_snapshot(&mut self, pubkey: &str, now: u64) -> bool {
        if let Some(last) = self.snapshot_requests.get(pubkey) {
            if now.saturating_sub(*last) < SURFACE_SNAPSHOT_REQUEST_MIN_INTERVAL_MS {
                return false;
            }
        }
        if !self.snapshot_requests.contains_key(pubkey)
            && self.snapshot_requests.len() >= MAX_WATCHERS
        {
            return false;
        }
        self.snapshot_requests.insert(pubkey.to_string(), now);
        true
    }
}

// --------------------------------------------------------------- announce

/// What a 30626 should say. `viewport` is `None` until it is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnnounceSpec {
    pub(crate) channel_id: Uuid,
    pub(crate) session_ref: String,
    pub(crate) status: AnnounceStatus,
    pub(crate) target_key: Option<String>,
    pub(crate) provider: Option<PublicKey>,
    /// `Some` on an open announce; a close carries no page (NIP-SP).
    pub(crate) page: Option<String>,
    /// `Some` on an open announce; a close carries no title.
    pub(crate) title: Option<String>,
    pub(crate) viewport: Option<(u32, u32)>,
    pub(crate) stream: PreviewStream,
}

impl AnnounceSpec {
    /// Equal in everything a republish is for (the viewport alone does not
    /// republish: resizing the pane would otherwise announce every 5 s).
    pub(crate) fn same_announcement(&self, other: &AnnounceSpec) -> bool {
        self.channel_id == other.channel_id
            && self.session_ref == other.session_ref
            && self.status == other.status
            && self.target_key == other.target_key
            && self.provider == other.provider
            && self.page == other.page
            && self.title == other.title
            && self.stream == other.stream
    }

    /// The close of this announce: same address, no page and no title.
    pub(crate) fn closed(&self) -> AnnounceSpec {
        AnnounceSpec {
            status: AnnounceStatus::Closed,
            page: None,
            title: None,
            ..self.clone()
        }
    }

    /// The core announce, at a known viewport.
    pub(crate) fn to_announce(&self, viewport: (u32, u32)) -> SessionPreviewAnnounce {
        SessionPreviewAnnounce {
            channel_id: self.channel_id,
            session_ref: self.session_ref.clone(),
            status: self.status,
            target_key: self.target_key.clone(),
            provider: self.provider,
            page: self.page.clone(),
            title: self.title.clone(),
            viewport_width: viewport.0,
            viewport_height: viewport.1,
            stream: self.stream,
        }
    }
}

/// Debounce for one preview's announces: at most one per
/// [`ANNOUNCE_MIN_INTERVAL_MS`]; changes inside the window coalesce (the
/// latest wins) and go out when it ends, so a `closed` after an `open` is
/// never lost.
#[derive(Debug, Clone, Default)]
pub(crate) struct AnnounceDebounce {
    pub(crate) last_sent: Option<AnnounceSpec>,
    /// What was last sent before `last_sent`, restored if it fails.
    prev_sent: Option<AnnounceSpec>,
    last_sent_at: Option<u64>,
    pub(crate) pending: Option<AnnounceSpec>,
}

impl AnnounceDebounce {
    /// Offer what should be announced now: `Some(open spec)`, or `None` when
    /// the preview is not open or not shared. Returns the spec to publish
    /// right now, if any.
    pub(crate) fn offer(
        &mut self,
        desired: Option<AnnounceSpec>,
        now: u64,
    ) -> Option<AnnounceSpec> {
        let candidate = match desired {
            Some(open) => open,
            None => match &self.last_sent {
                Some(sent) if sent.status == AnnounceStatus::Open => sent.closed(),
                // Never announced open (or already closed): nothing to say,
                // and an open still waiting in the window is withdrawn.
                _ => {
                    self.pending = None;
                    return None;
                }
            },
        };
        if self
            .last_sent
            .as_ref()
            .is_some_and(|sent| sent.same_announcement(&candidate))
        {
            self.pending = None;
            return None;
        }
        let window_open = self
            .last_sent_at
            .is_some_and(|at| now.saturating_sub(at) < ANNOUNCE_MIN_INTERVAL_MS);
        if window_open {
            self.pending = Some(candidate);
            return None;
        }
        self.pending = None;
        self.sent(candidate.clone(), now);
        Some(candidate)
    }

    /// The coalesced announce whose window has ended, if any.
    pub(crate) fn due(&mut self, now: u64) -> Option<AnnounceSpec> {
        let window_open = self
            .last_sent_at
            .is_some_and(|at| now.saturating_sub(at) < ANNOUNCE_MIN_INTERVAL_MS);
        if window_open {
            return None;
        }
        let spec = self.pending.take()?;
        self.sent(spec.clone(), now);
        Some(spec)
    }

    fn sent(&mut self, spec: AnnounceSpec, now: u64) {
        self.prev_sent = self.last_sent.replace(spec);
        self.last_sent_at = Some(now);
    }

    /// The viewport an announce went out with (measured after the offer).
    pub(crate) fn note_viewport(&mut self, spec: &AnnounceSpec, viewport: (u32, u32)) {
        if let Some(sent) = self.last_sent.as_mut() {
            if sent.same_announcement(spec) {
                sent.viewport = Some(viewport);
            }
        }
    }

    /// An announce could not be published: retry it after the window
    /// unless something newer is already waiting.
    pub(crate) fn failed(&mut self, spec: AnnounceSpec) {
        if self
            .last_sent
            .as_ref()
            .is_some_and(|sent| sent.same_announcement(&spec))
        {
            self.last_sent = self.prev_sent.take();
        }
        if self.pending.is_none() {
            self.pending = Some(spec);
        }
    }
}
