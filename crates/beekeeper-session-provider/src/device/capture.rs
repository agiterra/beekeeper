//! Watch-driven capture: polled JPEG frames for everyone watching a slot.
//!
//! Capture runs only while someone is watching. The first live 24320 `watch`
//! for a slot starts a [`spawn_capture`] task; each tick it takes a
//! `simctl io screenshot --type=jpeg` on a blocking thread, downscales to a
//! 900 px long edge, hashes the *pixels*, and publishes a 24321 only when
//! the hash changed (a static screen sends nothing), at most once per
//! [`DEVICE_FRAME_BASE_CADENCE_MS`] and [`SURFACE_FRAME_MAX_PER_MIN`] per
//! minute. When the last watcher stops or goes unheard for
//! [`SURFACE_WATCH_EXPIRY_MS`], the task publishes `t=paused`, logs that
//! capture stopped, and exits.
//!
//! The task never touches the provider's run loop or the relay: frames go to
//! the device service's outbound queue with `try_send`, so a slow relay
//! drops frames (the first thing to drop, brief § 6) instead of stalling
//! capture or anything else (SV-72/76).

use std::collections::{BTreeMap, VecDeque};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, Notify};

use super::simctl::{ShotFormat, Simctl};
use super::wire::{
    FrameHeader, FrameType, DEVICE_FRAME_BASE_CADENCE_MS, DEVICE_FRAME_BUDGET_BYTES,
    DEVICE_FRAME_MAX_LONG_EDGE, SURFACE_FRAME_MAX_PER_MIN, SURFACE_WATCH_EXPIRY_MS,
};

/// The JPEG qualities tried, best first, until a frame fits its budget.
const QUALITY_LADDER: [u8; 6] = [80, 70, 60, 50, 40, 30];

/// Watchers of one slot and when each was last heard (unix ms).
#[derive(Debug, Default, Clone)]
pub struct WatcherRegistry {
    last_seen: BTreeMap<String, u64>,
}

impl WatcherRegistry {
    /// Record a `watch` (or any keepalive) from `watcher` at `now_ms`.
    pub fn touch(&mut self, watcher: &str, now_ms: u64) {
        self.last_seen.insert(watcher.to_owned(), now_ms);
    }

    /// Record a `stop`: the watcher leaves at once.
    pub fn stop(&mut self, watcher: &str) {
        self.last_seen.remove(watcher);
    }

    /// Drop expired watchers and count the live ones.
    pub fn live(&mut self, now_ms: u64) -> usize {
        self.last_seen
            .retain(|_, seen| now_ms.saturating_sub(*seen) < SURFACE_WATCH_EXPIRY_MS);
        self.last_seen.len()
    }
}

/// Why a captured frame was or was not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// Publish it.
    Send,
    /// Same pixels as the last frame sent.
    Unchanged,
    /// Too soon after the last frame, or the per-minute budget is spent.
    Throttled,
}

/// Change-only, cadence- and budget-limited admission of frames.
#[derive(Debug, Clone)]
pub struct CaptureGate {
    cadence_ms: u64,
    last_hash: Option<[u8; 32]>,
    sent: VecDeque<u64>,
}

impl CaptureGate {
    /// A gate at `cadence_ms`.
    pub fn new(cadence_ms: u64) -> Self {
        Self {
            cadence_ms,
            last_hash: None,
            sent: VecDeque::new(),
        }
    }

    /// Forget the last hash, so the next frame is sent even if unchanged
    /// (`resync`).
    pub fn resync(&mut self) {
        self.last_hash = None;
    }

    /// Decide for a frame with pixel hash `hash` captured at `now_ms`.
    pub fn admit(&mut self, now_ms: u64, hash: [u8; 32]) -> GateDecision {
        while self
            .sent
            .front()
            .is_some_and(|at| now_ms.saturating_sub(*at) >= 60_000)
        {
            self.sent.pop_front();
        }
        if self.last_hash == Some(hash) {
            return GateDecision::Unchanged;
        }
        let too_soon = self
            .sent
            .back()
            .is_some_and(|at| now_ms.saturating_sub(*at) < self.cadence_ms);
        if too_soon || self.sent.len() >= SURFACE_FRAME_MAX_PER_MIN as usize {
            return GateDecision::Throttled;
        }
        self.sent.push_back(now_ms);
        self.last_hash = Some(hash);
        GateDecision::Send
    }
}

/// One frame ready to publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    /// Standard padded base64 of the JPEG.
    pub base64: String,
    /// Width after downscaling.
    pub width: u32,
    /// Height after downscaling.
    pub height: u32,
}

/// Decode `bytes`, downscale to `max_edge`, and return the pixels' hash and
/// the image. Blocking (CPU).
pub fn prepare_frame(bytes: &[u8], max_edge: u32) -> Result<([u8; 32], image::RgbImage), String> {
    let decoded =
        image::load_from_memory(bytes).map_err(|error| format!("frame decode: {error}"))?;
    let scaled = if decoded.width().max(decoded.height()) > max_edge {
        decoded.resize(max_edge, max_edge, image::imageops::FilterType::Triangle)
    } else {
        decoded
    };
    let rgb = scaled.to_rgb8();
    let hash: [u8; 32] = Sha256::digest(rgb.as_raw()).into();
    Ok((hash, rgb))
}

/// Encode `rgb` as JPEG, stepping quality (then size) down until its base64
/// fits `budget` bytes. Blocking (CPU).
pub fn encode_frame(rgb: &image::RgbImage, budget: usize) -> Result<EncodedFrame, String> {
    let mut current = rgb.clone();
    for _ in 0..4 {
        for quality in QUALITY_LADDER {
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut Cursor::new(&mut jpeg), quality)
                .encode_image(&current)
                .map_err(|error| format!("frame encode: {error}"))?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&jpeg);
            if encoded.len() <= budget {
                return Ok(EncodedFrame {
                    base64: encoded,
                    width: current.width(),
                    height: current.height(),
                });
            }
        }
        let (w, h) = (current.width() * 3 / 4, current.height() * 3 / 4);
        if w == 0 || h == 0 {
            break;
        }
        current = image::imageops::resize(&current, w, h, image::imageops::FilterType::Triangle);
    }
    Err("the frame does not fit its budget at any quality".into())
}

/// What a capture task hands the device service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureOutput {
    /// A 24321 to sign and send: header, type, base64 content.
    Frame(FrameHeader, FrameType, String),
    /// The task ended (no watchers, or the slot closed).
    Ended {
        /// The slot.
        slot: String,
        /// The epoch that ended.
        epoch: u64,
    },
}

/// Shared between the service and one slot's capture task.
#[derive(Debug, Clone)]
pub struct CaptureHandle {
    /// The slot's watchers.
    pub watchers: Arc<Mutex<WatcherRegistry>>,
    /// Wakes the task early (resync, a new watcher, close).
    pub wake: Arc<Notify>,
    /// Set by `resync`: the next frame ignores the change-only rule.
    pub resync: Arc<std::sync::atomic::AtomicBool>,
    /// Set on close: the task sends `t=end` and exits.
    pub closed: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for CaptureHandle {
    fn default() -> Self {
        Self {
            watchers: Arc::new(Mutex::new(WatcherRegistry::default())),
            wake: Arc::new(Notify::new()),
            resync: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

/// Unix ms now.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// What one capture task captures.
#[derive(Debug, Clone)]
pub struct CaptureTarget {
    /// Channel.
    pub channel: String,
    /// Slot.
    pub slot: String,
    /// UDID (host-local; used only for simctl).
    pub udid: String,
}

/// Start one slot's capture task. It runs until the slot has no live
/// watchers or is closed, then reports [`CaptureOutput::Ended`].
pub fn spawn_capture(
    simctl: Simctl,
    target: CaptureTarget,
    handle: CaptureHandle,
    out: mpsc::Sender<CaptureOutput>,
    cadence: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run_capture(simctl, target, handle, out, cadence))
}

async fn run_capture(
    simctl: Simctl,
    target: CaptureTarget,
    handle: CaptureHandle,
    out: mpsc::Sender<CaptureOutput>,
    cadence: Duration,
) {
    use std::sync::atomic::Ordering;
    let epoch = now_ms();
    let cadence_ms = (cadence.as_millis() as u64).max(1);
    let mut gate = CaptureGate::new(cadence_ms);
    let mut seq: u64 = 0;
    let mut last_dim = (1, 1);
    tracing::info!(target: "csp::device", slot = %target.slot, epoch, "device capture started");
    let header = |seq: u64, dim: (u32, u32), at: u64| FrameHeader {
        channel: target.channel.clone(),
        slot: target.slot.clone(),
        seq,
        epoch,
        cadence_ms: cadence_ms.clamp(500, 60_000),
        width: dim.0,
        height: dim.1,
        captured_at_ms: at,
    };
    let ending = loop {
        if handle.closed.load(Ordering::SeqCst) {
            break FrameType::End;
        }
        let live = handle
            .watchers
            .lock()
            .map(|mut watchers| watchers.live(now_ms()))
            .unwrap_or(0);
        if live == 0 {
            break FrameType::Paused;
        }
        if handle.resync.swap(false, Ordering::SeqCst) {
            gate.resync();
        }
        let shooter = simctl.clone();
        let udid = target.udid.clone();
        let captured = tokio::task::spawn_blocking(move || {
            let bytes = shooter.screenshot(&udid, ShotFormat::Jpeg)?;
            prepare_frame(&bytes, DEVICE_FRAME_MAX_LONG_EDGE)
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
        let at = now_ms();
        match captured {
            Ok((hash, rgb)) => {
                if gate.admit(at, hash) == GateDecision::Send {
                    let encoded = tokio::task::spawn_blocking(move || {
                        encode_frame(&rgb, DEVICE_FRAME_BUDGET_BYTES)
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result);
                    match encoded {
                        Ok(frame) => {
                            seq += 1;
                            last_dim = (frame.width, frame.height);
                            // A full queue drops the frame: frames are the
                            // first thing to give way to the transcript.
                            let _ = out.try_send(CaptureOutput::Frame(
                                header(seq, last_dim, at),
                                FrameType::Frame,
                                frame.base64,
                            ));
                        }
                        Err(reason) => {
                            tracing::warn!(target: "csp::device", slot = %target.slot, "frame dropped: {reason}")
                        }
                    }
                }
            }
            Err(reason) => {
                tracing::warn!(target: "csp::device", slot = %target.slot, "device capture failed: {reason}")
            }
        }
        let _ = tokio::time::timeout(cadence, handle.wake.notified()).await;
    };
    seq += 1;
    let _ = out
        .send(CaptureOutput::Frame(
            header(seq, last_dim, now_ms()),
            ending,
            String::new(),
        ))
        .await;
    tracing::info!(
        target: "csp::device",
        slot = %target.slot,
        epoch,
        reason = if ending == FrameType::End { "slot closed" } else { "no live watchers" },
        "device capture stopped"
    );
    let _ = out
        .send(CaptureOutput::Ended {
            slot: target.slot.clone(),
            epoch,
        })
        .await;
}

/// The default capture cadence.
pub fn default_cadence() -> Duration {
    Duration::from_millis(DEVICE_FRAME_BASE_CADENCE_MS)
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
