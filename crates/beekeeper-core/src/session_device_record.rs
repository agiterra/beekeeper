//! NIP-SDV kind 44255: the provider-signed session device record.
//!
//! Re-exported from [`crate::session_device`]; split out to keep each file
//! small.

use nostr::{Event, EventId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{parse_device_head, parse_slot, validate_device_command_id, DevicePlatform};
use super::{
    MAX_DEVICE_TEXT_BYTES, MAX_SESSION_DEVICE_RECORD_CONTENT_BYTES, MINTED_ID_TAGS,
    SESSION_DEVICE_TAG_VERSION,
};
use crate::coding_session_command::MAX_IDENTIFIER_BYTES;
use crate::kind::{event_kind_u32, KIND_SESSION_DEVICE_RECORD};
use crate::surface_watch::{
    match_ordered_tags, refuse_host_local, refuse_host_local_tags, require_hex, required_value,
    slot_value, TagSlot,
};

/// Which kind of fact a 44255 record states (`sdv-type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceRecordType {
    /// What this provider can offer, once per generation.
    Availability,
    /// The device's lifecycle state.
    State,
    /// A command the provider will not run.
    Refused,
    /// A pointer to the 44253 snapshot answering a screenshot command.
    Shot,
}

impl DeviceRecordType {
    /// The exact `sdv-type` tag value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Availability => "availability",
            Self::State => "state",
            Self::Refused => "refused",
            Self::Shot => "shot",
        }
    }

    fn from_wire(value: &str) -> Option<Self> {
        match value {
            "availability" => Some(Self::Availability),
            "state" => Some(Self::State),
            "refused" => Some(Self::Refused),
            "shot" => Some(Self::Shot),
            _ => None,
        }
    }
}

/// Whether one platform is offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformAvailability {
    /// Whether the platform can be opened on this machine.
    pub available: bool,
    /// Why not (or a caveat), in words for a person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Both platforms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePlatforms {
    /// iOS Simulator.
    pub ios: PlatformAvailability,
    /// Android emulator.
    pub android: PlatformAvailability,
}

/// Whether the pinned `agent-device` driver is usable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDeviceAvailability {
    /// Whether it is installed and runnable.
    pub installed: bool,
    /// The installed version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Why it is not usable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Device lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceState {
    /// Booting (not terminal).
    Booting,
    /// Open and bound to the session.
    Open,
    /// Released.
    Closed,
    /// Could not be opened or was lost.
    Failed,
}

/// Who may drive the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceDriver {
    /// The session's agent.
    #[serde(rename = "agent")]
    Agent,
    /// The person who owns the host machine.
    #[serde(rename = "host-owner")]
    HostOwner,
}

/// How frames are captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceCapture {
    /// Always `snapshot-poll` in this iteration.
    pub mode: String,
    /// Longest interval between polls while watched.
    pub max_interval_ms: u64,
}

/// Machine-readable refusal reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRefusalCode {
    /// The author is neither a seat of the generation nor a session person.
    NoStanding,
    /// The command names no open device.
    NoDeviceOpen,
    /// The requested platform is not offered here.
    PlatformUnavailable,
    /// The driving toolchain is not usable here.
    ToolchainUnavailable,
    /// The device is open but its snapshot could not be captured or uploaded.
    CaptureFailed,
    /// The command is malformed for this provider.
    InvalidCommand,
    /// Another command holds the device.
    Busy,
}

/// Content of a kind 44255 record; `type` equals the `sdv-type` tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum DeviceRecordPayload {
    /// What this provider offers.
    #[serde(rename_all = "camelCase")]
    Availability {
        /// Per platform.
        platforms: DevicePlatforms,
        /// The driver.
        agent_device: AgentDeviceAvailability,
    },
    /// Lifecycle state of the slot's device.
    #[serde(rename_all = "camelCase")]
    State {
        /// The state.
        state: DeviceState,
        /// Platform.
        platform: DevicePlatform,
        /// Model name, e.g. `iPhone 17`.
        model: String,
        /// OS version, e.g. `27.0`.
        os_version: String,
        /// Who may drive.
        drivers: Vec<DeviceDriver>,
        /// Capture mode.
        capture: DeviceCapture,
        /// Why (expected on `failed`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A refusal.
    Refused {
        /// Machine reason.
        code: DeviceRefusalCode,
        /// Words for a person.
        reason: String,
    },
    /// A snapshot pointer; the `e` tag names the 44253.
    Shot,
}

impl DeviceRecordPayload {
    /// The `sdv-type` this payload belongs under.
    pub fn record_type(&self) -> DeviceRecordType {
        match self {
            Self::Availability { .. } => DeviceRecordType::Availability,
            Self::State { .. } => DeviceRecordType::State,
            Self::Refused { .. } => DeviceRecordType::Refused,
            Self::Shot => DeviceRecordType::Shot,
        }
    }
}

/// A structurally valid kind 44255 record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDeviceRecord {
    /// The session channel (`h`).
    pub channel_id: Uuid,
    /// `cs-target`.
    pub target_key: String,
    /// `csl-command`: the lifecycle command that minted that generation.
    pub lifecycle_command_id: String,
    /// `sdv-slot` (state, shot; optional on refused).
    pub slot: Option<String>,
    /// `sdv-cmd`: the command this record answers.
    pub command_id: Option<String>,
    /// `e`: the 44253 a shot points at.
    pub snapshot: Option<EventId>,
    /// The content.
    pub payload: DeviceRecordPayload,
}

const RECORD_LAYOUT: [TagSlot; 8] = [
    TagSlot::required("h"),
    TagSlot::required("sdv-v"),
    TagSlot::required("cs-target"),
    TagSlot::required("csl-command"),
    TagSlot::required("sdv-type"),
    TagSlot::optional("sdv-slot"),
    TagSlot::optional("sdv-cmd"),
    TagSlot::optional_arity("e", 4),
];

impl SessionDeviceRecord {
    /// The exact ordered tags of this record.
    pub fn tags(&self) -> Vec<Vec<String>> {
        let mut rows = vec![
            vec!["h".into(), self.channel_id.to_string()],
            vec!["sdv-v".into(), SESSION_DEVICE_TAG_VERSION.into()],
            vec!["cs-target".into(), self.target_key.clone()],
            vec!["csl-command".into(), self.lifecycle_command_id.clone()],
            vec![
                "sdv-type".into(),
                self.payload.record_type().as_str().into(),
            ],
        ];
        if let Some(slot) = &self.slot {
            rows.push(vec!["sdv-slot".into(), slot.clone()]);
        }
        if let Some(command) = &self.command_id {
            rows.push(vec!["sdv-cmd".into(), command.clone()]);
        }
        if let Some(snapshot) = &self.snapshot {
            rows.push(vec![
                "e".into(),
                snapshot.to_hex(),
                String::new(),
                "snapshot".into(),
            ]);
        }
        rows
    }

    /// The exact content of this record.
    pub fn content(&self) -> Result<String, String> {
        serde_json::to_string(&self.payload).map_err(|error| error.to_string())
    }

    /// Whether this record ends its command (every command gets exactly one).
    pub fn is_terminal(&self) -> bool {
        match &self.payload {
            DeviceRecordPayload::State { state, .. } => *state != DeviceState::Booting,
            DeviceRecordPayload::Refused { .. } | DeviceRecordPayload::Shot => true,
            DeviceRecordPayload::Availability { .. } => false,
        }
    }
}

/// Validate a kind 44255 record from its tags and content.
pub fn validate_session_device_record_parts(
    tags: &[&[String]],
    content: &str,
) -> Result<SessionDeviceRecord, String> {
    if content.len() > MAX_SESSION_DEVICE_RECORD_CONTENT_BYTES {
        return Err(format!(
            "device record content exceeds {MAX_SESSION_DEVICE_RECORD_CONTENT_BYTES} bytes"
        ));
    }
    let slots = match_ordered_tags(tags, &RECORD_LAYOUT, "device record")?;
    refuse_host_local_tags(tags, &MINTED_ID_TAGS)?;
    let (channel_id, target_key) = parse_device_head(&slots)?;
    let lifecycle_command_id = required_value(&slots, 3);
    if lifecycle_command_id.trim().is_empty()
        || lifecycle_command_id.len() > MAX_IDENTIFIER_BYTES
        || lifecycle_command_id.chars().any(char::is_control)
    {
        return Err(format!(
            "csl-command must be 1..={MAX_IDENTIFIER_BYTES} bytes of text"
        ));
    }
    let record_type = DeviceRecordType::from_wire(required_value(&slots, 4))
        .ok_or_else(|| "sdv-type must be availability, state, refused or shot".to_owned())?;
    let slot = parse_slot(slot_value(slots[5]))?;
    let command_id = slot_value(slots[6])
        .map(|id| validate_device_command_id(id).map(|()| id.to_owned()))
        .transpose()?;
    let snapshot = slots[7]
        .map(|tag| {
            require_hex(&tag[1], 64, "e")?;
            if !tag[2].is_empty() || tag[3] != "snapshot" {
                return Err("e must be [e, <44253 id>, \"\", snapshot]".to_owned());
            }
            EventId::from_hex(&tag[1]).map_err(|_| "e is not an event id".to_owned())
        })
        .transpose()?;
    let (slot_rule, command_rule, snapshot_rule) = match record_type {
        DeviceRecordType::Availability => (Rule::Forbidden, Rule::Forbidden, Rule::Forbidden),
        DeviceRecordType::State => (Rule::Required, Rule::Optional, Rule::Forbidden),
        DeviceRecordType::Refused => (Rule::Optional, Rule::Required, Rule::Forbidden),
        DeviceRecordType::Shot => (Rule::Required, Rule::Required, Rule::Required),
    };
    slot_rule.check(slot.is_some(), "sdv-slot", record_type)?;
    command_rule.check(command_id.is_some(), "sdv-cmd", record_type)?;
    snapshot_rule.check(snapshot.is_some(), "e", record_type)?;

    let value: Value = serde_json::from_str(content)
        .map_err(|_| "device record content is not JSON".to_owned())?;
    refuse_host_local_strings(&value)?;
    let payload: DeviceRecordPayload = serde_json::from_value(value)
        .map_err(|error| format!("device record content is not sdv1 JSON: {error}"))?;
    if payload.record_type() != record_type {
        return Err("device record content type does not match sdv-type".into());
    }
    if let DeviceRecordPayload::State {
        model,
        os_version,
        drivers,
        capture,
        ..
    } = &payload
    {
        if model.trim().is_empty() || os_version.trim().is_empty() {
            return Err("device state names its model and osVersion".into());
        }
        if capture.mode != "snapshot-poll" {
            return Err("device capture mode must be snapshot-poll".into());
        }
        let repeated = drivers
            .iter()
            .enumerate()
            .any(|(index, driver)| drivers[..index].contains(driver));
        if repeated {
            return Err("device drivers must not repeat".into());
        }
    }
    Ok(SessionDeviceRecord {
        channel_id,
        target_key,
        lifecycle_command_id: lifecycle_command_id.to_owned(),
        slot,
        command_id,
        snapshot,
        payload,
    })
}

/// [`validate_session_device_record_parts`] over a signed event, checking its
/// kind.
pub fn validate_session_device_record_envelope(
    event: &Event,
) -> Result<SessionDeviceRecord, String> {
    if event_kind_u32(event) != KIND_SESSION_DEVICE_RECORD {
        return Err("event is not a device record (kind 44255)".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    validate_session_device_record_parts(&tags, &event.content)
}

/// The newest valid `state` record for `slot` in `channel_id`, by
/// (`created_at`, then the lower id). Its signer is the slot's frame
/// authority once its (`csl-command`, `cs-target`) resolves to that signer.
pub fn newest_device_state_record<'a>(
    records: &'a [Event],
    channel_id: Uuid,
    slot: &str,
) -> Option<(&'a Event, SessionDeviceRecord)> {
    let mut best: Option<(&Event, SessionDeviceRecord)> = None;
    for event in records {
        let Ok(record) = validate_session_device_record_envelope(event) else {
            continue;
        };
        if record.channel_id != channel_id
            || record.slot.as_deref() != Some(slot)
            || record.payload.record_type() != DeviceRecordType::State
        {
            continue;
        }
        let newer = best.as_ref().is_none_or(|(current, _)| {
            (event.created_at, std::cmp::Reverse(event.id))
                > (current.created_at, std::cmp::Reverse(current.id))
        });
        if newer {
            best = Some((event, record));
        }
    }
    best
}

#[derive(Clone, Copy)]
enum Rule {
    Required,
    Optional,
    Forbidden,
}

impl Rule {
    fn check(self, present: bool, tag: &str, record_type: DeviceRecordType) -> Result<(), String> {
        match (self, present) {
            (Self::Required, false) => {
                Err(format!("a {} record requires {tag}", record_type.as_str()))
            }
            (Self::Forbidden, true) => Err(format!(
                "a {} record carries no {tag}",
                record_type.as_str()
            )),
            _ => Ok(()),
        }
    }
}

fn refuse_host_local_strings(value: &Value) -> Result<(), String> {
    match value {
        Value::String(text) => {
            if text.len() > MAX_DEVICE_TEXT_BYTES || text.chars().any(char::is_control) {
                return Err(format!(
                    "device record text must be one line of at most {MAX_DEVICE_TEXT_BYTES} bytes"
                ));
            }
            refuse_host_local("device record content", text)
        }
        Value::Array(items) => items.iter().try_for_each(refuse_host_local_strings),
        Value::Object(map) => map.values().try_for_each(refuse_host_local_strings),
        _ => Ok(()),
    }
}
