//! Pure helpers behind `bee device`: session context, command ids, the
//! provider's host-local slot directory, and folding 44255 records.
//!
//! Nothing here touches the network, so every rule is testable without a
//! relay (`device_tests.rs`).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use beekeeper_core::coding_session_command::coding_session_target_key;
use beekeeper_core::coding_session_title::parse_coding_session_target_key;
use beekeeper_core::kind::KIND_SESSION_DEVICE_RECORD;
use beekeeper_core::preview_grant::decode_preview_grant_unverified;
use beekeeper_core::session_device::{
    validate_device_command_id, validate_session_device_record_envelope, DeviceRecordPayload,
    DeviceRecordType, DeviceState, SessionDeviceRecord, DEVICE_SLOT_HEX_LEN,
};
use nostr::{Event, PublicKey};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::CliError;

/// What `bee device list` says when no availability record exists for the
/// session's generation (WIRE-C5 § 7). Never "no devices".
pub const NO_DEVICES_OFFERED_NOTE: &str = "this machine's provider does not offer devices";

/// The usage sentence when neither flags nor a grant name the session.
pub const MISSING_CONTEXT: &str = "no session to address: pass --channel <uuid> and --target \
     <cs-target key>, or run inside a session whose $BEEKEEPER_PREVIEW_GRANT is set";

/// Which session a `bee device` call addresses, and whose records it trusts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceContext {
    /// The session channel (`h`).
    pub channel: Uuid,
    /// The `cs-target` key of the generation, when known.
    pub target_key: Option<String>,
    /// This machine's provider key, when known: records must be signed by it.
    pub provider: Option<PublicKey>,
}

impl DeviceContext {
    /// The `cs-target` key, or a usage error naming the flag.
    pub fn target(&self) -> Result<&str, CliError> {
        self.target_key.as_deref().ok_or_else(|| {
            CliError::Usage(
                "this needs the session's generation: pass --target <cs-target key> or run \
                 inside a session whose $BEEKEEPER_PREVIEW_GRANT is set"
                    .into(),
            )
        })
    }
}

/// Resolve the session from `--channel`/`--target`/`--provider` and, for
/// whatever they leave out, the execution's preview grant (decoded for its
/// claims only: the relay checks standing, the provider checks the command).
pub fn resolve_context(
    channel: Option<&str>,
    target: Option<&str>,
    provider: Option<&str>,
    grant: Option<&str>,
) -> Result<DeviceContext, CliError> {
    let channel = channel
        .map(|value| {
            Uuid::parse_str(value.trim())
                .map_err(|_| CliError::Usage(format!("--channel is not a UUID: {value}")))
        })
        .transpose()?;
    let target = target
        .map(|value| {
            parse_coding_session_target_key(value.trim())
                .map(|parsed| coding_session_target_key(&parsed))
                .map_err(|_| CliError::Usage(format!("--target is not a cs-target key: {value}")))
        })
        .transpose()?;
    let provider = provider
        .map(|value| {
            PublicKey::from_hex(value.trim())
                .map_err(|_| CliError::Usage(format!("--provider is not a pubkey: {value}")))
        })
        .transpose()?;

    let grant = grant.map(str::trim).filter(|value| !value.is_empty());
    let needs_grant = channel.is_none() || target.is_none() || provider.is_none();
    let claims = match (needs_grant, grant) {
        (true, Some(token)) => match decode_preview_grant_unverified(token) {
            Ok(claims) => Some(claims),
            Err(error) if channel.is_none() => {
                return Err(CliError::Usage(format!(
                    "$BEEKEEPER_PREVIEW_GRANT cannot be read ({}); pass --channel <uuid> and \
                     --target <cs-target key>",
                    error.code()
                )))
            }
            Err(_) => None,
        },
        _ => None,
    };
    let channel = channel
        .or(claims.as_ref().map(|claims| claims.channel_id))
        .ok_or_else(|| CliError::Usage(MISSING_CONTEXT.into()))?;
    let target_key = target.or_else(|| {
        claims
            .as_ref()
            .map(|claims| coding_session_target_key(&claims.target))
    });
    let provider = provider.or_else(|| {
        claims
            .as_ref()
            .and_then(|claims| PublicKey::from_hex(&claims.issuer).ok())
    });
    Ok(DeviceContext {
        channel,
        target_key,
        provider,
    })
}

/// The `sdv-cmd` for a command: `--command-id` when given (a retry reuses
/// it), else a fresh random id. Never derived from the clock.
pub fn command_id(flag: Option<&str>) -> Result<String, CliError> {
    match flag {
        Some(id) => {
            validate_device_command_id(id)
                .map_err(|error| CliError::Usage(format!("--command-id: {error}")))?;
            Ok(id.to_owned())
        }
        None => Ok(fresh_command_id()),
    }
}

/// `bee-<16 random hex>`.
pub fn fresh_command_id() -> String {
    format!("bee-{:016x}", rand::random::<u64>())
}

/// Whether `slot` is a wire slot id (16 lowercase hex).
pub fn is_slot_id(slot: &str) -> bool {
    slot.len() == DEVICE_SLOT_HEX_LEN
        && slot
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Validate a `--slot` flag.
pub fn parse_slot_flag(slot: &str) -> Result<String, CliError> {
    let slot = slot.trim();
    if is_slot_id(slot) {
        Ok(slot.to_owned())
    } else {
        Err(CliError::Usage(format!(
            "--slot must be {DEVICE_SLOT_HEX_LEN} lowercase hex characters: {slot}"
        )))
    }
}

/// Find this session's device directory (`<CSP state>/device/<session>`):
/// `$BEEKEEPER_DEVICE_DIR` when set, else the parent of the provider's shim
/// directory on `PATH` (`…/device/<session>/bin`). `Err` says why not.
pub fn discover_session_dir(
    device_dir: Option<&OsStr>,
    path: Option<&OsStr>,
) -> Result<PathBuf, String> {
    if let Some(dir) = device_dir.filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    if let Some(path) = path {
        for entry in std::env::split_paths(path) {
            if is_device_shim_dir(&entry) {
                if let Some(session_dir) = entry.parent() {
                    return Ok(session_dir.to_path_buf());
                }
            }
        }
    }
    Err(
        "this process cannot see the provider's device directory: $BEEKEEPER_DEVICE_DIR is \
         unset and no PATH entry ends in /device/<session>/bin"
            .into(),
    )
}

fn is_device_shim_dir(entry: &Path) -> bool {
    let session = entry.parent();
    entry.file_name() == Some(OsStr::new("bin"))
        && session.and_then(Path::file_name).is_some()
        && session.and_then(Path::parent).and_then(Path::file_name) == Some(OsStr::new("device"))
}

/// The few slot-file fields `bee device` reads (the provider's `SlotFile`,
/// camelCase, 0600). Host-local only: nothing here is ever published.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSlot {
    /// The wire slot id.
    pub slot: String,
    /// The model name.
    pub model: String,
    /// e.g. `iOS 27.0`.
    pub os_version: String,
    /// Where the provider writes `<sha256>.png`.
    pub shot_dir: PathBuf,
}

/// Read `<session dir>/<slot>.json`. `Err` says why it could not be read.
pub fn read_local_slot(session_dir: &Path, slot: &str) -> Result<LocalSlot, String> {
    if !is_slot_id(slot) {
        return Err(format!("{slot} is not a slot id"));
    }
    let path = session_dir.join(format!("{slot}.json"));
    let bytes = std::fs::read(&path).map_err(|error| {
        format!(
            "the provider's slot file for {slot} cannot be read here ({})",
            error.kind()
        )
    })?;
    let local: LocalSlot = serde_json::from_slice(&bytes)
        .map_err(|error| format!("the provider's slot file for {slot} is not readable: {error}"))?;
    if local.slot != slot {
        return Err(format!(
            "the provider's slot file names {}, not {slot}",
            local.slot
        ));
    }
    Ok(local)
}

/// Where the provider wrote a shot's PNG: `<shotDir>/<sha256>.png`.
pub fn shot_png_path(local: &LocalSlot, sha256: &str) -> Option<PathBuf> {
    let hex = sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
    hex.then(|| local.shot_dir.join(format!("{sha256}.png")))
}

/// The provider's quick-start text for an open device (mirrors
/// `beekeeper_session_provider::device::agent_device::quick_start`; a test
/// pins the two together).
pub fn quick_start(slot: &str, model: &str, os_label: &str) -> String {
    let target = format!("--session {slot}");
    [
        format!("The session is watching {model} ({os_label}) in the Device surface."),
        format!(
            "Drive it with agent-device. Always pass {target}; the shim adds the device and \
             daemon flags."
        ),
        "Typical loop:".to_owned(),
        format!("  agent-device open <bundle-id> {target}"),
        format!("  agent-device snapshot -i {target}        # accessibility tree with @eN refs"),
        format!("  agent-device click @e3 {target}"),
        format!("  agent-device fill @e5 \"text\" {target}"),
        "  bee device screenshot                      # a dated snapshot everyone in the session \
         sees"
            .to_owned(),
        "Prefer snapshot refs over coordinates. The Flutter debug bundle id is per worktree: \
         read it from `flutter run` output."
            .to_owned(),
        "First use builds an XCTest runner and can take a couple of minutes; later commands are \
         fast."
            .to_owned(),
    ]
    .join("\n")
}

/// A 44255 this context trusts, with who signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedRecord {
    /// The record event id.
    pub event_id: String,
    /// The signer.
    pub author: PublicKey,
    /// `created_at`, unix seconds.
    pub created_at: u64,
    /// The validated record.
    pub record: SessionDeviceRecord,
}

/// Accept `event` as a 44255 of this context: right kind, valid signature,
/// signed by the provider when the provider is known, structurally valid,
/// in this channel, and (when the context knows it) for this generation.
pub fn accept_record(event: &Event, ctx: &DeviceContext) -> Option<AcceptedRecord> {
    if u32::from(event.kind.as_u16()) != KIND_SESSION_DEVICE_RECORD || event.verify().is_err() {
        return None;
    }
    if ctx
        .provider
        .is_some_and(|provider| provider != event.pubkey)
    {
        return None;
    }
    let record = validate_session_device_record_envelope(event).ok()?;
    if record.channel_id != ctx.channel {
        return None;
    }
    if ctx
        .target_key
        .as_deref()
        .is_some_and(|target| target != record.target_key)
    {
        return None;
    }
    Some(AcceptedRecord {
        event_id: event.id.to_hex(),
        author: event.pubkey,
        created_at: event.created_at.as_secs(),
        record,
    })
}

/// The terminal 44255 answering `command_id`, if `event` is one.
pub fn match_terminal(
    event: &Event,
    ctx: &DeviceContext,
    command_id: &str,
) -> Option<AcceptedRecord> {
    let accepted = accept_record(event, ctx)?;
    (accepted.record.command_id.as_deref() == Some(command_id) && accepted.record.is_terminal())
        .then_some(accepted)
}

/// Turn a terminal record into the command's result: `Ok` with its JSON for
/// `state open|closed` and `shot`, `Err` (exit 4) for `refused` and
/// `state failed`, naming the provider's code and reason.
pub fn terminal_result(accepted: &AcceptedRecord) -> Result<Value, CliError> {
    let record = &accepted.record;
    match &record.payload {
        DeviceRecordPayload::Refused { code, reason } => {
            let code = serde_json::to_value(code)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "refused".into());
            Err(CliError::Other(format!(
                "the provider refused the command: {code}: {reason}"
            )))
        }
        DeviceRecordPayload::State {
            state: DeviceState::Failed,
            reason,
            model,
            ..
        } => Err(CliError::Other(format!(
            "the device failed: {} ({model})",
            reason.as_deref().unwrap_or("no reason given")
        ))),
        DeviceRecordPayload::State { .. } => Ok(state_json(accepted)),
        DeviceRecordPayload::Shot => Ok(json!({
            "recordId": accepted.event_id,
            "slot": record.slot,
            "commandId": record.command_id,
            "snapshotId": record.snapshot.map(|id| id.to_hex()),
        })),
        DeviceRecordPayload::Availability { .. } => Err(CliError::Other(
            "the provider answered with an availability record, which ends no command".into(),
        )),
    }
}

/// One state record as JSON.
pub fn state_json(accepted: &AcceptedRecord) -> Value {
    let record = &accepted.record;
    let mut out = json!({
        "recordId": accepted.event_id,
        "signer": accepted.author.to_hex(),
        "slot": record.slot,
        "target": record.target_key,
        "commandId": record.command_id,
        "updatedAt": accepted.created_at,
    });
    if let (Value::Object(map), Ok(Value::Object(payload))) =
        (&mut out, serde_json::to_value(&record.payload))
    {
        for (key, value) in payload {
            if key != "type" {
                map.insert(key, value);
            }
        }
    }
    out
}

/// The newest trusted state record of every slot in the channel, newest
/// first (ties: lower id first, as the core fold orders them).
pub fn newest_slot_states(events: &[Event], ctx: &DeviceContext) -> Vec<AcceptedRecord> {
    let mut newest: std::collections::BTreeMap<String, (&Event, AcceptedRecord)> =
        std::collections::BTreeMap::new();
    for event in events {
        let Some(accepted) = accept_record(event, ctx) else {
            continue;
        };
        if accepted.record.payload.record_type() != DeviceRecordType::State {
            continue;
        }
        let Some(slot) = accepted.record.slot.clone() else {
            continue;
        };
        let replace = newest.get(&slot).is_none_or(|(current, _)| {
            (event.created_at, std::cmp::Reverse(event.id))
                > (current.created_at, std::cmp::Reverse(current.id))
        });
        if replace {
            newest.insert(slot, (event, accepted));
        }
    }
    let mut out: Vec<(&Event, AcceptedRecord)> = newest.into_values().collect();
    out.sort_by_key(|(event, _)| {
        std::cmp::Reverse((event.created_at, std::cmp::Reverse(event.id)))
    });
    out.into_iter().map(|(_, accepted)| accepted).collect()
}

/// The slot `close`/`screenshot` address by default: the newest slot of this
/// generation whose newest state is `open`.
pub fn default_open_slot(events: &[Event], ctx: &DeviceContext) -> Result<String, CliError> {
    newest_slot_states(events, ctx)
        .into_iter()
        .find(|accepted| {
            matches!(
                accepted.record.payload,
                DeviceRecordPayload::State {
                    state: DeviceState::Open,
                    ..
                }
            )
        })
        .and_then(|accepted| accepted.record.slot)
        .ok_or_else(|| {
            CliError::Usage(
                "no device is open for this session: run `bee device open`, or pass --slot".into(),
            )
        })
}

/// The newest trusted availability record of this generation.
pub fn newest_availability(events: &[Event], ctx: &DeviceContext) -> Option<AcceptedRecord> {
    let mut best: Option<(&Event, AcceptedRecord)> = None;
    for event in events {
        let Some(accepted) = accept_record(event, ctx) else {
            continue;
        };
        if accepted.record.payload.record_type() != DeviceRecordType::Availability {
            continue;
        }
        let newer = best.as_ref().is_none_or(|(current, _)| {
            (event.created_at, std::cmp::Reverse(event.id))
                > (current.created_at, std::cmp::Reverse(current.id))
        });
        if newer {
            best = Some((event, accepted));
        }
    }
    best.map(|(_, accepted)| accepted)
}

/// `bee device list`'s output: this generation's availability (or the
/// "does not offer devices" note) and each slot's newest state.
pub fn list_output(events: &[Event], ctx: &DeviceContext) -> Value {
    let availability = newest_availability(events, ctx);
    let mut out = json!({
        "channel": ctx.channel.to_string(),
        "target": ctx.target_key,
        "provider": ctx.provider.map(|key| key.to_hex()),
        "availability": availability.as_ref().and_then(|accepted| {
            serde_json::to_value(&accepted.record.payload).ok().map(|mut value| {
                if let Value::Object(map) = &mut value {
                    map.remove("type");
                    map.insert("recordId".into(), json!(accepted.event_id));
                    map.insert("signer".into(), json!(accepted.author.to_hex()));
                }
                value
            })
        }),
        "slots": newest_slot_states(events, ctx).iter().map(state_json).collect::<Vec<_>>(),
    });
    if availability.is_none() {
        if let Value::Object(map) = &mut out {
            map.insert("note".into(), json!(NO_DEVICES_OFFERED_NOTE));
        }
    }
    out
}

/// Parse relay query rows into events, dropping anything that is not one.
pub fn events_from_rows(rows: Vec<Value>) -> Vec<Event> {
    rows.into_iter()
        .filter_map(|row| serde_json::from_value::<Event>(row).ok())
        .collect()
}
