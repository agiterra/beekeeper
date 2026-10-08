//! NIP-SDV: the session device command (kind 44254) and record (kind 44255),
//! and the opaque slot id that names a device on the wire.
//!
//! The provider (inside `beekeeper-host`, outside the seat boundary) owns the
//! device. Seats and people ask with a signed stored command; the provider
//! answers every command with exactly one terminal record (`state`
//! open/closed/failed, a `shot` pointer to a 44253, or `refused`). Standing
//! — whether the author is a seat of the generation or a person in the
//! session — is the provider's decision; the relay validates structure and
//! the host-local rule ([`crate::surface_watch::refuse_host_local`]) only.
//!
//! The UDID, daemon URL and token stay in a 0600 slot file on the machine;
//! the wire carries [`device_slot_id`], the first 16 hex of
//! `sha256(provider pubkey ‖ udid)`.

use nostr::{Event, PublicKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::coding_session_title::parse_coding_session_target_key;
use crate::kind::{event_kind_u32, KIND_SESSION_DEVICE_COMMAND};
use crate::surface_watch::{
    match_ordered_tags, parse_channel, refuse_host_local, refuse_host_local_tags, required_value,
    slot_value, validate_surface_key, Surface, TagSlot,
};

/// Exact `sdv-v` tag value.
pub const SESSION_DEVICE_TAG_VERSION: &str = "sdv1";
/// Maximum UTF-8 byte length of a 44254 command's content.
pub const MAX_SESSION_DEVICE_COMMAND_CONTENT_BYTES: usize = 8 * 1024;
/// Maximum UTF-8 byte length of a 44255 record's content.
pub const MAX_SESSION_DEVICE_RECORD_CONTENT_BYTES: usize = 4 * 1024;
/// Hex length of an `sdv-slot`.
pub const DEVICE_SLOT_HEX_LEN: usize = 16;
/// Maximum byte length of an `sdv-cmd` command id.
pub const MAX_DEVICE_COMMAND_ID_BYTES: usize = 64;
/// Maximum byte length of a device `model` name.
pub const MAX_DEVICE_MODEL_BYTES: usize = 64;
/// Maximum byte length of any free-text string in a record.
pub const MAX_DEVICE_TEXT_BYTES: usize = 512;

/// The opaque wire id of one device on one provider: the first 16 lowercase
/// hex of `sha256(provider.to_bytes() ‖ udid UTF-8)`, the UDID as `simctl`
/// prints it.
pub fn device_slot_id(provider: &PublicKey, udid: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(provider.to_bytes());
    hasher.update(udid.as_bytes());
    let digest = hex::encode(hasher.finalize());
    digest[..DEVICE_SLOT_HEX_LEN].to_owned()
}

/// Validate an `sdv-cmd` id: 1..=64 of `[A-Za-z0-9._-]`.
pub fn validate_device_command_id(value: &str) -> Result<(), String> {
    let ok = !value.is_empty()
        && value.len() <= MAX_DEVICE_COMMAND_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err("sdv-cmd must be 1..=64 of [A-Za-z0-9._-]".into())
    }
}

/// Device platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DevicePlatform {
    /// iOS Simulator.
    Ios,
    /// Android emulator.
    Android,
}

/// A device command's operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceOp {
    /// Boot (or reuse) a device and bind it to the session.
    Open,
    /// Release the device.
    Close,
    /// Publish one 44253 snapshot.
    Screenshot,
    /// Drive the device (agent-device action).
    Action,
}

/// One driving action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceAction {
    /// Action name, 1..=32 of `[a-z0-9_-]`.
    #[serde(rename = "type")]
    pub action_type: String,
    /// Action arguments (a JSON object; the provider validates it).
    #[serde(default = "empty_object")]
    pub args: Value,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// Content of a kind 44254 command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceCommandPayload {
    /// The operation.
    pub op: DeviceOp,
    /// `open` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<DevicePlatform>,
    /// `open` only: the device model name, e.g. `iPhone 17`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `close` only: also shut the device down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutdown: Option<bool>,
    /// `action` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<DeviceAction>,
}

impl DeviceCommandPayload {
    /// Validate the per-op field rules.
    pub fn validate(&self) -> Result<(), String> {
        let open_fields = self.platform.is_some() || self.model.is_some();
        if open_fields && self.op != DeviceOp::Open {
            return Err("platform and model belong to op open only".into());
        }
        if self.shutdown.is_some() && self.op != DeviceOp::Close {
            return Err("shutdown belongs to op close only".into());
        }
        if self.action.is_some() != (self.op == DeviceOp::Action) {
            return Err("action is required for op action and forbidden otherwise".into());
        }
        if let Some(model) = &self.model {
            if model.trim().is_empty()
                || model.len() > MAX_DEVICE_MODEL_BYTES
                || model.chars().any(char::is_control)
            {
                return Err(format!(
                    "model must be 1..={MAX_DEVICE_MODEL_BYTES} bytes of text"
                ));
            }
            refuse_host_local("model", model)?;
        }
        if let Some(action) = &self.action {
            let name_ok = !action.action_type.is_empty()
                && action.action_type.len() <= 32
                && action.action_type.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-')
                });
            if !name_ok {
                return Err("action type must be 1..=32 of [a-z0-9_-]".into());
            }
            if !action.args.is_object() {
                return Err("action args must be a JSON object".into());
            }
        }
        Ok(())
    }
}

/// A structurally valid kind 44254 command.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionDeviceCommand {
    /// The session channel (`h`).
    pub channel_id: Uuid,
    /// `cs-target`: the generation the device is (to be) bound to.
    pub target_key: String,
    /// `sdv-cmd`.
    pub command_id: String,
    /// `sdv-slot` (required for close, screenshot and action).
    pub slot: Option<String>,
    /// The content.
    pub payload: DeviceCommandPayload,
}

const COMMAND_LAYOUT: [TagSlot; 5] = [
    TagSlot::required("h"),
    TagSlot::required("sdv-v"),
    TagSlot::required("cs-target"),
    TagSlot::required("sdv-cmd"),
    TagSlot::optional("sdv-slot"),
];

/// Tags whose values Beekeeper or a runtime mints; exempt from the
/// UUID-shape half of the host-local rule only.
const MINTED_ID_TAGS: [&str; 4] = ["h", "cs-target", "csl-command", "sdv-cmd"];

impl SessionDeviceCommand {
    /// The exact ordered tags of this command.
    pub fn tags(&self) -> Vec<Vec<String>> {
        let mut rows = vec![
            vec!["h".into(), self.channel_id.to_string()],
            vec!["sdv-v".into(), SESSION_DEVICE_TAG_VERSION.into()],
            vec!["cs-target".into(), self.target_key.clone()],
            vec!["sdv-cmd".into(), self.command_id.clone()],
        ];
        if let Some(slot) = &self.slot {
            rows.push(vec!["sdv-slot".into(), slot.clone()]);
        }
        rows
    }

    /// The exact content of this command.
    pub fn content(&self) -> Result<String, String> {
        serde_json::to_string(&self.payload).map_err(|error| error.to_string())
    }
}

/// Validate a kind 44254 command from its tags and content.
pub fn validate_session_device_command_parts(
    tags: &[&[String]],
    content: &str,
) -> Result<SessionDeviceCommand, String> {
    if content.len() > MAX_SESSION_DEVICE_COMMAND_CONTENT_BYTES {
        return Err(format!(
            "device command content exceeds {MAX_SESSION_DEVICE_COMMAND_CONTENT_BYTES} bytes"
        ));
    }
    let slots = match_ordered_tags(tags, &COMMAND_LAYOUT, "device command")?;
    refuse_host_local_tags(tags, &MINTED_ID_TAGS)?;
    let (channel_id, target_key) = parse_device_head(&slots)?;
    let command_id = required_value(&slots, 3);
    validate_device_command_id(command_id)?;
    let slot = parse_slot(slot_value(slots[4]))?;
    let payload: DeviceCommandPayload = serde_json::from_str(content)
        .map_err(|error| format!("device command content is not sdv1 JSON: {error}"))?;
    payload.validate()?;
    match (payload.op, &slot) {
        (DeviceOp::Open, Some(_)) => return Err("op open carries no sdv-slot".into()),
        (DeviceOp::Close | DeviceOp::Screenshot | DeviceOp::Action, None) => {
            return Err("op close, screenshot and action name their sdv-slot".into());
        }
        _ => {}
    }
    Ok(SessionDeviceCommand {
        channel_id,
        target_key,
        command_id: command_id.to_owned(),
        slot,
        payload,
    })
}

/// [`validate_session_device_command_parts`] over a signed event, checking
/// its kind.
pub fn validate_session_device_command_envelope(
    event: &Event,
) -> Result<SessionDeviceCommand, String> {
    if event_kind_u32(event) != KIND_SESSION_DEVICE_COMMAND {
        return Err("event is not a device command (kind 44254)".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    validate_session_device_command_parts(&tags, &event.content)
}

fn parse_device_head(slots: &[Option<&[String]>]) -> Result<(Uuid, String), String> {
    let channel_id = parse_channel(required_value(slots, 0))?;
    if required_value(slots, 1) != SESSION_DEVICE_TAG_VERSION {
        return Err("unsupported sdv-v".into());
    }
    let target_key = required_value(slots, 2);
    parse_coding_session_target_key(target_key)?;
    Ok((channel_id, target_key.to_owned()))
}

fn parse_slot(value: Option<&str>) -> Result<Option<String>, String> {
    value
        .map(|slot| validate_surface_key(Surface::Device, slot).map(|()| slot.to_owned()))
        .transpose()
}

#[path = "session_device_record.rs"]
mod record;

pub use record::*;

#[cfg(test)]
#[path = "session_device_tests.rs"]
mod tests;
