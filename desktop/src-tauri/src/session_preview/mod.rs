//! The session **Browser**: a live view of a local dev server, docked in a
//! coding session's Browser surface, that the person and the session's agent
//! both drive (SV-33 S1/S2; contract `WIRE-C4.md`).
//!
//! # Shape
//!
//! - **Native view.** One WKWebView per session channel, built through wry's
//!   `build_as_child` directly (`view.rs`), so Tauri never sees it: no Tauri
//!   IPC is attached to the page, `get_webview_window("main")` keeps working,
//!   and no `unstable` Tauri feature is needed. It sits above the hosting
//!   window's main webview as a sibling NSView, so no DOM can cover it: the UI
//!   reports overlay leases (`set_occluded`) and the view hides behind a
//!   freeze frame while one is up.
//! - **Custody.** wry views are main-thread objects. They live in a
//!   thread-local map on the main thread (`view.rs`), reached only through
//!   `run_on_main_thread`; everything else (status, URL, binding, driving)
//!   lives here in a plain mutex so commands and the broker can read it from
//!   any thread.
//! - **Egress.** Loopback only, at three points (`policy.rs`).
//! - **Driving.** Agents reach a preview through the session broker
//!   (`broker.rs`), authorized by the per-session preview grant, never by the
//!   socket alone. Ops run in an isolated JavaScript world (`driver.rs`).
//!
//! Isolation, not security: the boundary stops one session's agent from
//! accidentally driving another session's page and keeps a dev page from
//! reaching the internet through the app. It is not a sandbox against a
//! hostile local process.

// Off macOS every view op is a stub returning `not_macos`, so the native
// half of this module (rule list, data store ids, main-thread plumbing) is
// compiled but never called there.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

pub mod broadcast;
#[cfg(unix)]
pub mod broker;
pub mod commands;
pub mod driver;
pub mod geometry;
pub mod policy;
pub mod ports;
pub mod view;

#[cfg(target_os = "macos")]
mod snapshot_macos;
#[cfg(target_os = "macos")]
mod webkit_macos;

use std::collections::HashMap;
use std::sync::Mutex;

use beekeeper_core_pkg::coding_session_command::CodingSessionTarget;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use geometry::{RectSequence, SlotRect};

/// Window label prefix of a popped-out preview. Matches no window pattern in
/// `capabilities/default.json`, so even the bare page the window was created
/// with can call nothing (pinned by `popout_label_matches_no_capability`).
pub const POPOUT_LABEL_PREFIX: &str = "session-preview-";

/// Event: a preview's state changed. Payload [`PreviewState`].
pub const STATE_EVENT: &str = "session-preview://state";
/// Event: an agent ran an op. Payload `{channelId, executionId, sessionId, op, at}`.
pub const ACTIVITY_EVENT: &str = "session-preview://activity";
/// Event: an agent started or stopped driving.
pub const DRIVING_EVENT: &str = "session-preview://driving";
/// Event: an agent opened a preview with no slot registered.
pub const OPEN_REQUESTED_EVENT: &str = "session-preview://open-requested";

/// Seconds after an agent's last op before it no longer counts as driving.
pub const DRIVING_LINGER_SECS: u64 = 10;

/// Lifecycle of one session's preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    /// Nothing open.
    Absent,
    /// A navigation has started and not finished.
    Loading,
    /// The page finished loading.
    Ready,
    /// The person closed it; agents are refused until it is reopened.
    ClosedByPerson,
    /// It cannot run here; see [`PreviewRecord::unavailable`].
    Unavailable,
}

/// Where the native view is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewPlacement {
    /// In the session's Browser slot (the docked pane or the floating
    /// mini-player; Rust cannot tell them apart and does not need to).
    Docked,
    /// In its own `session-preview-<channelId>` window.
    PoppedOut,
    /// Not drawn: no slot is mounted and it is not popped out.
    None,
}

/// Which session may drive a preview (WIRE-C4 §9 item 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// Opened from a surface with no session: undriveable until an agent
    /// `open`s, which rebinds it.
    None,
    /// Opened by the person in this session's Browser surface.
    Person {
        /// The session whose surface it was opened in.
        target: CodingSessionTarget,
    },
    /// Opened by this session's agent.
    Agent {
        /// The session the agent's grant names.
        target: CodingSessionTarget,
        /// The execution that opened (or last rebound) it.
        execution_id: String,
    },
}

impl Binding {
    /// The session target, if the preview is bound to one.
    pub fn target(&self) -> Option<&CodingSessionTarget> {
        match self {
            Binding::None => None,
            Binding::Person { target } | Binding::Agent { target, .. } => Some(target),
        }
    }
}

/// `PreviewState.boundTo` on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BoundTo {
    /// Undriveable.
    None,
    /// Person-opened, bound to the surface's session.
    #[serde(rename_all = "camelCase")]
    Person {
        /// Bound session id.
        session_id: String,
        /// Bound generation.
        generation: u64,
    },
    /// Agent-opened.
    #[serde(rename_all = "camelCase")]
    Agent {
        /// Execution that opened it.
        execution_id: String,
        /// Bound session id.
        session_id: String,
        /// Bound generation.
        generation: u64,
    },
}

impl From<&Binding> for BoundTo {
    fn from(binding: &Binding) -> Self {
        match binding {
            Binding::None => BoundTo::None,
            Binding::Person { target } => BoundTo::Person {
                session_id: target.session_id.clone(),
                generation: target.generation,
            },
            Binding::Agent {
                target,
                execution_id,
            } => BoundTo::Agent {
                execution_id: execution_id.clone(),
                session_id: target.session_id.clone(),
                generation: target.generation,
            },
        }
    }
}

/// The agent currently driving, for the driving strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Driving {
    /// The driving execution.
    pub execution_id: String,
    /// Its session.
    pub session_id: String,
    /// The last op's verb.
    pub last_op: String,
    /// Unix seconds of the last op.
    pub at: u64,
}

/// Why a preview cannot run, as `{code, sentence}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unavailable {
    /// WIRE-C4 §4 code.
    pub code: &'static str,
    /// The sentence the UI shows.
    pub sentence: &'static str,
}

impl Unavailable {
    /// Not macOS.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub const NOT_MACOS: Unavailable = Unavailable {
        code: "not_macos",
        sentence: "The Browser runs on macOS in this version.",
    };
    /// The `WKContentRuleList` did not compile (fail closed).
    pub const CONTENT_FILTER_FAILED: Unavailable = Unavailable {
        code: "content_filter_failed",
        sentence: "The Browser could not start its local-only filter, so it stays off.",
    };
    /// The web view could not be built.
    pub const WEBVIEW_FAILED: Unavailable = Unavailable {
        code: "webview_failed",
        sentence: "The Browser could not start a web view on this computer.",
    };
}

/// An error a command rejects with and the broker returns: `{code, message}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewError {
    /// Stable WIRE-C4 code.
    pub code: String,
    /// The sentence for a person or an agent.
    pub message: String,
}

impl PreviewError {
    /// Build from a code and a sentence.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    /// `preview_unavailable` for a reason.
    pub fn unavailable(reason: &Unavailable) -> Self {
        Self::new("preview_unavailable", reason.sentence)
    }

    /// `preview_not_open`.
    pub fn not_open() -> Self {
        Self::new(
            "preview_not_open",
            "No preview is open for this session. Run `bee preview open --port <n>`.",
        )
    }

    /// `preview_closed_by_person`.
    pub fn closed_by_person() -> Self {
        Self::new(
            "preview_closed_by_person",
            "The person closed this preview. Open it again only if they ask.",
        )
    }

    /// `preview_timeout`.
    pub fn timeout() -> Self {
        Self::new("preview_timeout", "The page did not get there in time.")
    }

    /// `preview_bad_request` with a reason.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("preview_bad_request", message)
    }
}

impl From<policy::PolicyRefusal> for PreviewError {
    fn from(refusal: policy::PolicyRefusal) -> Self {
        Self::new(refusal.code, refusal.message)
    }
}

/// Everything known about one session's preview, off the main thread.
#[derive(Debug, Clone)]
pub struct PreviewRecord {
    /// The session's umbrella channel id (lowercase UUID).
    pub channel_id: String,
    /// Lifecycle.
    pub status: PreviewStatus,
    /// Last committed URL.
    pub url: Option<String>,
    /// Document title.
    pub title: Option<String>,
    /// Committed top-level navigations so far; stamps refs.
    pub generation: u64,
    /// History state, read after each navigation event.
    pub can_go_back: bool,
    /// History state, read after each navigation event.
    pub can_go_forward: bool,
    /// Whether it is popped out (else it follows the slot).
    pub popped: bool,
    /// It popped out only because no slot was mounted (an agent opened it,
    /// or drove it while no Browser surface was showing). The next slot that
    /// mounts docks it; a pop-out the person chose stays out.
    pub auto_popped: bool,
    /// The window hosting the slot (`main` or `coding-session-*`).
    pub slot_window: Option<String>,
    /// The slot rect last applied, in the slot window's logical points.
    pub slot: Option<SlotRect>,
    /// Last-write-wins sequence for the slot.
    pub slot_seq: RectSequence,
    /// An overlay lease intersects the slot.
    pub occluded: bool,
    /// The view is currently hidden (occluded, collapsed, or unplaced).
    pub hidden: bool,
    /// Data URL of the snapshot shown while hidden by an overlay.
    pub freeze_frame: Option<String>,
    /// Who may drive it.
    pub binding: Binding,
    /// The agent currently driving.
    pub driving: Option<Driving>,
    /// Counts agent ops, so the "stopped driving" timer of an older op can
    /// tell a newer op came after it.
    pub driving_seq: u64,
    /// `per_session` or `incognito`.
    pub data_store: &'static str,
    /// Why it cannot run, when it cannot.
    pub unavailable: Option<Unavailable>,
    /// A native view exists for it.
    pub has_view: bool,
}

impl PreviewRecord {
    /// A fresh, absent record.
    pub fn new(channel_id: &str) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            status: PreviewStatus::Absent,
            url: None,
            title: None,
            generation: 0,
            can_go_back: false,
            can_go_forward: false,
            popped: false,
            auto_popped: false,
            slot_window: None,
            slot: None,
            slot_seq: RectSequence::default(),
            occluded: false,
            hidden: true,
            freeze_frame: None,
            binding: Binding::None,
            driving: None,
            driving_seq: 0,
            data_store: "per_session",
            unavailable: None,
            has_view: false,
        }
    }

    /// Where the view is drawn right now.
    pub fn placement(&self) -> PreviewPlacement {
        if !self.has_view {
            PreviewPlacement::None
        } else if self.popped {
            PreviewPlacement::PoppedOut
        } else if self.slot.is_some() && self.slot_window.is_some() {
            PreviewPlacement::Docked
        } else {
            PreviewPlacement::None
        }
    }

    /// The wire state.
    pub fn state(&self) -> PreviewState {
        PreviewState {
            channel_id: self.channel_id.clone(),
            status: self.status,
            url: self.url.clone(),
            title: self.title.clone(),
            generation: self.generation,
            can_go_back: self.can_go_back,
            can_go_forward: self.can_go_forward,
            placement: self.placement(),
            hidden: self.hidden,
            occluded: self.occluded,
            freeze_frame: self.freeze_frame.clone(),
            bound_to: BoundTo::from(&self.binding),
            driving: self.driving.clone(),
            data_store: self.data_store,
            unavailable: self.unavailable.clone(),
        }
    }
}

/// `PreviewState` (WIRE-C4 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewState {
    /// Channel id.
    pub channel_id: String,
    /// Lifecycle.
    pub status: PreviewStatus,
    /// Committed URL.
    pub url: Option<String>,
    /// Title.
    pub title: Option<String>,
    /// Navigation generation.
    pub generation: u64,
    /// History.
    pub can_go_back: bool,
    /// History.
    pub can_go_forward: bool,
    /// Placement.
    pub placement: PreviewPlacement,
    /// Hidden.
    pub hidden: bool,
    /// Occluded by an overlay.
    pub occluded: bool,
    /// Freeze frame data URL.
    pub freeze_frame: Option<String>,
    /// Binding.
    pub bound_to: BoundTo,
    /// Driving agent.
    pub driving: Option<Driving>,
    /// Data store kind.
    pub data_store: &'static str,
    /// Unavailable reason.
    pub unavailable: Option<Unavailable>,
}

static REGISTRY: Mutex<Option<HashMap<String, PreviewRecord>>> = Mutex::new(None);

/// Read or change one channel's record (created absent on first touch).
/// Never call native code while inside `f`: WebKit callbacks take this lock.
pub fn with_record<T>(channel_id: &str, f: impl FnOnce(&mut PreviewRecord) -> T) -> T {
    let mut guard = REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let record = map
        .entry(channel_id.to_string())
        .or_insert_with(|| PreviewRecord::new(channel_id));
    f(record)
}

/// Channels whose preview has a native view or is opening one.
pub fn open_channel_ids() -> Vec<String> {
    let guard = REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .as_ref()
        .map(|map| {
            map.values()
                .filter(|record| {
                    record.has_view
                        || matches!(record.status, PreviewStatus::Loading | PreviewStatus::Ready)
                })
                .map(|record| record.channel_id.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The record's current wire state.
pub fn state_of(channel_id: &str) -> PreviewState {
    with_record(channel_id, |record| record.state())
}

/// Emit `session-preview://state` for a channel and return the state.
pub fn emit_state(app: &AppHandle, channel_id: &str) -> PreviewState {
    let state = state_of(channel_id);
    if let Err(error) = app.emit(STATE_EVENT, &state) {
        eprintln!("session-preview: emit state failed: {error}");
    }
    broadcast::on_preview_state(app, channel_id);
    state
}

/// Validate and normalize a channel id: a UUID, lowercased and hyphenated.
pub fn normalize_channel_id(raw: &str) -> Result<String, PreviewError> {
    uuid::Uuid::parse_str(raw.trim())
        .map(|id| id.hyphenated().to_string())
        .map_err(|_| PreviewError::bad_request(format!("{raw:?} is not a channel id.")))
}

/// The pop-out window label for a channel.
pub fn popout_label(channel_id: &str) -> String {
    format!("{POPOUT_LABEL_PREFIX}{channel_id}")
}

/// The app's dev frontend origin, refused by the URL policy in dev builds.
pub fn refused_origin(app: &AppHandle) -> Option<policy::RefusedOrigin> {
    if !tauri::is_dev() {
        return None;
    }
    policy::RefusedOrigin::from_dev_url(app.config().build.dev_url.as_ref())
}

/// Unix seconds now.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
