//! Lane D's local mirror of WIRE-C5 (`/tmp/wavec/C5/WIRE-C5.md`) for the
//! kinds this provider reads and writes: 24320 watch, 24321 frame, 44253
//! snapshot, 44254 command, 44255 record.
//!
//! Lane P owns the canonical types in `beekeeper_core::{surface_watch,
//! surface_snapshot, session_device}`. They landed (uncommitted, same tree)
//! while this lane was building, so the constants below are re-exported
//! from core, and the builders here are checked against core's validators
//! by the engine tests (`listener_tests.rs` `assert_nothing_host_local`):
//! every event the engine publishes passes the same envelope check the
//! relay runs at ingest. The builders stay local because they compose from
//! the engine's string state; swapping them for core's typed structs is a
//! mechanical follow-up, not a wire change. Every event built here also
//! passes [`super::slot::assert_publishable`] before it is signed.

use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use serde::Deserialize;
use serde_json::{json, Value};

use beekeeper_core::kind as core_kind;

use super::slot::assert_publishable;

/// kind 24320 — surface watch (ephemeral, member → producer).
pub const KIND_SURFACE_WATCH: u16 = core_kind::KIND_SURFACE_WATCH as u16;
/// kind 24321 — surface frame (ephemeral, producer → channel).
pub const KIND_SURFACE_FRAME: u16 = core_kind::KIND_SURFACE_FRAME as u16;
/// kind 44253 — surface snapshot (stored).
pub const KIND_SURFACE_SNAPSHOT: u16 = core_kind::KIND_SURFACE_SNAPSHOT as u16;
/// kind 44254 — session device command (stored).
pub const KIND_SESSION_DEVICE_COMMAND: u16 = core_kind::KIND_SESSION_DEVICE_COMMAND as u16;
/// kind 44255 — session device record (stored, provider-signed).
pub const KIND_SESSION_DEVICE_RECORD: u16 = core_kind::KIND_SESSION_DEVICE_RECORD as u16;

pub use beekeeper_core::session_device::{
    DEVICE_SLOT_HEX_LEN, MAX_SESSION_DEVICE_COMMAND_CONTENT_BYTES, SESSION_DEVICE_TAG_VERSION,
};
pub use beekeeper_core::surface_snapshot::SURFACE_SNAPSHOT_TAG_VERSION;
pub use beekeeper_core::surface_watch::{
    DEVICE_FRAME_BASE_CADENCE_MS, DEVICE_FRAME_BUDGET_BYTES, DEVICE_FRAME_MAX_LONG_EDGE,
    SURFACE_DEVICE, SURFACE_FRAME_MAX_PER_MIN, SURFACE_SNAPSHOT_REQUEST_MIN_INTERVAL_MS,
    SURFACE_WATCH_EXPIRY_MS,
};

fn tag(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn tag_value<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
    event
        .tags
        .iter()
        .map(Tag::as_slice)
        .find(|values| values.first().map(String::as_str) == Some(name))
        .and_then(|values| values.get(1))
        .map(String::as_str)
}

fn tag_names(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter_map(|tag| tag.as_slice().first().cloned())
        .collect()
}

/// Sign `tags` + `content` as `kind`, refusing anything host-local first.
pub fn sign(
    keys: &Keys,
    kind: u16,
    tags: Vec<Vec<String>>,
    content: String,
) -> Result<Event, String> {
    assert_publishable(&tags, &content)?;
    let parsed = tags
        .iter()
        .map(|values| {
            Tag::parse(values.iter().map(String::as_str)).map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    EventBuilder::new(Kind::from(kind), content)
        .tags(parsed)
        .sign_with_keys(keys)
        .map_err(|error| error.to_string())
}

/// The `op` of a 44254.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceOp {
    /// Boot (or reuse) a simulator for the session.
    Open {
        /// `ios` (default) or `android`.
        platform: String,
        /// A model name, or `None` for the provider's choice.
        model: Option<String>,
    },
    /// Unbind the slot, optionally shutting the simulator down.
    Close {
        /// Shut the simulator down too.
        shutdown: bool,
    },
    /// Publish one durable snapshot.
    Screenshot,
    /// A typed control action (S3; not offered by this iteration).
    Action {
        /// The action type.
        kind: String,
    },
}

/// A decoded 44254.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCommand {
    /// Event id.
    pub event_id: String,
    /// Author pubkey hex.
    pub author: String,
    /// Channel UUID text.
    pub channel: String,
    /// The `cs-target` key.
    pub cs_target: String,
    /// The caller's stable command id.
    pub sdv_cmd: String,
    /// The slot, for close/screenshot/action.
    pub slot: Option<String>,
    /// What it asks.
    pub op: DeviceOp,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandJson {
    op: String,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    shutdown: Option<bool>,
    #[serde(default)]
    action: Option<Value>,
}

fn valid_cmd_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Whether `value` is a 16-lowercase-hex slot id.
pub fn valid_slot(value: &str) -> bool {
    value.len() == DEVICE_SLOT_HEX_LEN && value.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// Decode a 44254 (structure only: the relay already validated it, and this
/// is the provider's own fail-closed second look). Signature included.
pub fn decode_command(event: &Event) -> Result<DeviceCommand, String> {
    if event.kind != Kind::from(KIND_SESSION_DEVICE_COMMAND) {
        return Err("not a device command".into());
    }
    event
        .verify()
        .map_err(|error| format!("bad signature: {error}"))?;
    let names = tag_names(event);
    let has_slot = names.len() == 5;
    let expected: &[&str] = if has_slot {
        &["h", "sdv-v", "cs-target", "sdv-cmd", "sdv-slot"]
    } else {
        &["h", "sdv-v", "cs-target", "sdv-cmd"]
    };
    if names != expected {
        return Err(format!(
            "device command tags {names:?} are not {expected:?}"
        ));
    }
    if tag_value(event, "sdv-v") != Some(SESSION_DEVICE_TAG_VERSION) {
        return Err("unknown sdv-v".into());
    }
    let channel = tag_value(event, "h").unwrap_or_default().to_owned();
    let cs_target = tag_value(event, "cs-target").unwrap_or_default().to_owned();
    if !cs_target.starts_with("coding-session/v1|") {
        return Err("cs-target is not a coding-session key".into());
    }
    let sdv_cmd = tag_value(event, "sdv-cmd").unwrap_or_default().to_owned();
    if !valid_cmd_id(&sdv_cmd) {
        return Err("sdv-cmd is not a valid command id".into());
    }
    let slot = tag_value(event, "sdv-slot").map(str::to_owned);
    if slot.as_deref().is_some_and(|slot| !valid_slot(slot)) {
        return Err("sdv-slot is not 16 lowercase hex".into());
    }
    if event.content.len() > MAX_SESSION_DEVICE_COMMAND_CONTENT_BYTES {
        return Err("device command content is too large".into());
    }
    let body: CommandJson = serde_json::from_str(&event.content)
        .map_err(|error| format!("device command content: {error}"))?;
    let only_open = body.platform.is_some() || body.model.is_some();
    let op = match body.op.as_str() {
        "open" => {
            if slot.is_some() || body.shutdown.is_some() || body.action.is_some() {
                return Err("open carries no slot, shutdown or action".into());
            }
            let platform = body.platform.unwrap_or_else(|| "ios".into());
            if !matches!(platform.as_str(), "ios" | "android") {
                return Err("platform must be ios or android".into());
            }
            if body.model.as_ref().is_some_and(|model| model.len() > 64) {
                return Err("model is longer than 64 bytes".into());
            }
            DeviceOp::Open {
                platform,
                model: body.model.filter(|model| !model.trim().is_empty()),
            }
        }
        "close" if !only_open && body.action.is_none() => DeviceOp::Close {
            shutdown: body.shutdown.unwrap_or(false),
        },
        "screenshot" if !only_open && body.action.is_none() && body.shutdown.is_none() => {
            DeviceOp::Screenshot
        }
        "action" if !only_open && body.shutdown.is_none() => {
            let kind = body
                .action
                .as_ref()
                .and_then(|action| action.get("type"))
                .and_then(Value::as_str)
                .filter(|kind| {
                    (1..=32).contains(&kind.len())
                        && kind
                            .chars()
                            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
                })
                .ok_or("action needs a type")?
                .to_owned();
            DeviceOp::Action { kind }
        }
        other => return Err(format!("unknown or malformed op {other:?}")),
    };
    if !matches!(op, DeviceOp::Open { .. }) && slot.is_none() {
        return Err("close, screenshot and action need sdv-slot".into());
    }
    Ok(DeviceCommand {
        event_id: event.id.to_hex(),
        author: event.pubkey.to_hex(),
        channel,
        cs_target,
        sdv_cmd,
        slot,
        op,
    })
}

/// A 24320 action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchAction {
    /// Start or keep watching.
    Watch,
    /// Stop watching.
    Stop,
    /// Send a full frame now.
    Resync,
    /// Publish one 44253 requested by this watcher.
    Snapshot,
}

/// A decoded 24320 addressed to this provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRequest {
    /// Watcher pubkey hex.
    pub watcher: String,
    /// Channel UUID text.
    pub channel: String,
    /// Slot id.
    pub slot: String,
    /// What it asks.
    pub action: WatchAction,
}

/// Decode a 24320 for `surface=device` addressed to `me`.
pub fn decode_watch(event: &Event, me: &str) -> Result<WatchRequest, String> {
    if event.kind != Kind::from(KIND_SURFACE_WATCH) {
        return Err("not a surface watch".into());
    }
    event
        .verify()
        .map_err(|error| format!("bad signature: {error}"))?;
    if tag_names(event) != ["h", "surface", "d", "p"] {
        return Err("surface watch tags are not h, surface, d, p".into());
    }
    if tag_value(event, "surface") != Some(SURFACE_DEVICE) {
        return Err("not a device watch".into());
    }
    if tag_value(event, "p") != Some(me) {
        return Err("addressed to another producer".into());
    }
    let slot = tag_value(event, "d").unwrap_or_default().to_owned();
    if !valid_slot(&slot) {
        return Err("device watch d is not a slot".into());
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        action: String,
    }
    let body: Body =
        serde_json::from_str(&event.content).map_err(|error| format!("watch content: {error}"))?;
    let action = match body.action.as_str() {
        "watch" => WatchAction::Watch,
        "stop" => WatchAction::Stop,
        "resync" => WatchAction::Resync,
        "snapshot" => WatchAction::Snapshot,
        other => return Err(format!("unknown watch action {other:?}")),
    };
    Ok(WatchRequest {
        watcher: event.pubkey.to_hex(),
        channel: tag_value(event, "h").unwrap_or_default().to_owned(),
        slot,
        action,
    })
}

/// The `sdv-type` of a 44255.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    /// What this provider offers.
    Availability,
    /// The slot's device state.
    State,
    /// A command this provider will not run.
    Refused,
    /// A command answered by a 44253.
    Shot,
}

impl RecordType {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Availability => "availability",
            Self::State => "state",
            Self::Refused => "refused",
            Self::Shot => "shot",
        }
    }
}

/// Where a 44255 belongs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordScope {
    /// Channel UUID text.
    pub channel: String,
    /// `cs-target` key.
    pub cs_target: String,
    /// The lifecycle command that minted the generation.
    pub csl_command: String,
}

/// Build a 44255's tags in WIRE-C5 § 7 order.
pub fn record_tags(
    scope: &RecordScope,
    kind: RecordType,
    slot: Option<&str>,
    sdv_cmd: Option<&str>,
    snapshot_id: Option<&str>,
) -> Vec<Vec<String>> {
    let mut tags = vec![
        tag(&["h", &scope.channel]),
        tag(&["sdv-v", SESSION_DEVICE_TAG_VERSION]),
        tag(&["cs-target", &scope.cs_target]),
        tag(&["csl-command", &scope.csl_command]),
        tag(&["sdv-type", kind.as_str()]),
    ];
    if let Some(slot) = slot {
        tags.push(tag(&["sdv-slot", slot]));
    }
    if let Some(cmd) = sdv_cmd {
        tags.push(tag(&["sdv-cmd", cmd]));
    }
    if let Some(id) = snapshot_id {
        tags.push(tag(&["e", id, "", "snapshot"]));
    }
    tags
}

/// One platform's availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformAvailability {
    /// Whether it can be opened.
    pub available: bool,
    /// Why not.
    pub reason: Option<String>,
}

impl PlatformAvailability {
    fn json(&self) -> Value {
        match &self.reason {
            Some(reason) => json!({"available": self.available, "reason": reason}),
            None => json!({"available": self.available}),
        }
    }
}

/// The agent-device half of availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDeviceAvailability {
    /// Whether the pinned version is installed.
    pub installed: bool,
    /// The pinned version.
    pub version: Option<String>,
    /// Why driving is unavailable (Node missing or too old, install failed).
    pub reason: Option<String>,
}

/// `availability` content.
pub fn availability_content(
    ios: &PlatformAvailability,
    android: &PlatformAvailability,
    agent: &AgentDeviceAvailability,
) -> String {
    let mut agent_json = json!({"installed": agent.installed});
    if let Some(version) = &agent.version {
        agent_json["version"] = json!(version);
    }
    if let Some(reason) = &agent.reason {
        agent_json["reason"] = json!(reason);
    }
    json!({
        "type": "availability",
        "platforms": {"ios": ios.json(), "android": android.json()},
        "agentDevice": agent_json,
    })
    .to_string()
}

/// `state` content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateContent {
    /// `booting|open|closed|failed`.
    pub state: &'static str,
    /// `ios`.
    pub platform: String,
    /// Model name.
    pub model: String,
    /// `27.0` (version only).
    pub os_version: String,
    /// Who drives: `agent` when agent-device is ready, `host-owner` always.
    pub drivers: Vec<&'static str>,
    /// Why, for `failed`.
    pub reason: Option<String>,
}

impl StateContent {
    /// Serialize.
    pub fn to_json(&self) -> String {
        let mut value = json!({
            "type": "state",
            "state": self.state,
            "platform": self.platform,
            "model": self.model,
            "osVersion": self.os_version,
            "drivers": self.drivers,
            "capture": {"mode": "snapshot-poll", "maxIntervalMs": DEVICE_FRAME_BASE_CADENCE_MS},
        });
        if let Some(reason) = &self.reason {
            value["reason"] = json!(reason);
        }
        value.to_string()
    }
}

/// `refused.code` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalCode {
    /// The author is not a seat of the generation or a session person.
    NoStanding,
    /// No device is open in this slot.
    NoDeviceOpen,
    /// The platform is not offered here.
    PlatformUnavailable,
    /// simctl/agent-device could not do it.
    ToolchainUnavailable,
    /// The snapshot could not be captured or uploaded.
    CaptureFailed,
    /// The command is malformed for this provider.
    InvalidCommand,
    /// Another command holds the slot.
    Busy,
}

impl RefusalCode {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoStanding => "no_standing",
            Self::NoDeviceOpen => "no_device_open",
            Self::PlatformUnavailable => "platform_unavailable",
            Self::ToolchainUnavailable => "toolchain_unavailable",
            Self::CaptureFailed => "capture_failed",
            Self::InvalidCommand => "invalid_command",
            Self::Busy => "busy",
        }
    }
}

/// `refused` content.
pub fn refused_content(code: RefusalCode, reason: &str) -> String {
    json!({"type": "refused", "code": code.as_str(), "reason": reason}).to_string()
}

/// `shot` content.
pub fn shot_content() -> String {
    json!({"type": "shot"}).to_string()
}

/// A 44253 for a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFields<'a> {
    /// Channel.
    pub channel: &'a str,
    /// Slot.
    pub slot: &'a str,
    /// Blob sha256.
    pub sha256: &'a str,
    /// Blob URL.
    pub url: &'a str,
    /// MIME type.
    pub mime: &'a str,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Unix ms.
    pub taken_at_ms: u64,
    /// This provider's pubkey.
    pub provider: &'a str,
    /// Who asked.
    pub requested_by: Option<&'a str>,
    /// The 44254 it answers.
    pub command_event: Option<&'a str>,
}

/// 44253 tags in WIRE-C5 § 6 order (device: no `page`, no `title`).
pub fn snapshot_tags(fields: &SnapshotFields<'_>) -> Vec<Vec<String>> {
    let dim = format!("{}x{}", fields.width, fields.height);
    let taken = fields.taken_at_ms.to_string();
    let mut tags = vec![
        tag(&["h", fields.channel]),
        tag(&["ssn-v", SURFACE_SNAPSHOT_TAG_VERSION]),
        tag(&["ssn-type", "snapshot"]),
        tag(&["surface", SURFACE_DEVICE]),
        tag(&["d", fields.slot]),
        tag(&["x", fields.sha256]),
        tag(&["url", fields.url]),
        tag(&["m", fields.mime]),
        tag(&["dim", &dim]),
        tag(&["taken-at", &taken]),
        tag(&["provider", fields.provider]),
    ];
    if let Some(who) = fields.requested_by {
        tags.push(tag(&["p", who, "", "requested-by"]));
    }
    if let Some(id) = fields.command_event {
        tags.push(tag(&["e", id, "", "command"]));
    }
    tags
}

/// `t` of a 24321.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// A JPEG.
    Frame,
    /// Capture paused (no watchers).
    Paused,
    /// The slot closed.
    End,
}

/// A 24321's header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    /// Channel.
    pub channel: String,
    /// Slot.
    pub slot: String,
    /// Sequence within the epoch.
    pub seq: u64,
    /// The capture run's epoch (ms).
    pub epoch: u64,
    /// Cadence in force.
    pub cadence_ms: u64,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Unix ms.
    pub captured_at_ms: u64,
}

/// 24321 tags in WIRE-C5 § 4 order.
pub fn frame_tags(header: &FrameHeader, kind: FrameType) -> Vec<Vec<String>> {
    let t = match kind {
        FrameType::Frame => "frame",
        FrameType::Paused => "paused",
        FrameType::End => "end",
    };
    vec![
        tag(&["h", &header.channel]),
        tag(&["surface", SURFACE_DEVICE]),
        tag(&["d", &header.slot]),
        tag(&["t", t]),
        tag(&["seq", &header.seq.to_string()]),
        tag(&["epoch", &header.epoch.to_string()]),
        tag(&["cadence-ms", &header.cadence_ms.to_string()]),
        tag(&[
            "dim",
            &format!("{}x{}", header.width.max(1), header.height.max(1)),
        ]),
        tag(&["captured-at", &header.captured_at_ms.to_string()]),
    ]
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
