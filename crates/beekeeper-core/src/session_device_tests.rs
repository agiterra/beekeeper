use super::*;
use crate::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use crate::kind::KIND_SESSION_DEVICE_RECORD;
use nostr::{EventBuilder, EventId, Keys, Kind, Tag, Timestamp};
use serde_json::json;

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const SLOT: &str = "9f2c4e1a7b3d5c80";
const UDID: &str = "70E4C638-1A2B-4C3D-9E8F-0123456789AB";

fn channel() -> Uuid {
    Uuid::parse_str(CHANNEL).expect("uuid")
}

fn target_key() -> String {
    coding_session_target_key(&CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        session_id: "0f8e2d6c-1b3a-4c5d-8e7f-9a0b1c2d3e4f".into(),
        generation: 2,
    })
}

fn parts(rows: &[Vec<String>]) -> Vec<&[String]> {
    rows.iter().map(Vec::as_slice).collect()
}

fn command(payload: DeviceCommandPayload, slot: Option<&str>) -> SessionDeviceCommand {
    SessionDeviceCommand {
        channel_id: channel(),
        target_key: target_key(),
        command_id: "dev-open-1".into(),
        slot: slot.map(str::to_owned),
        payload,
    }
}

fn open_payload() -> DeviceCommandPayload {
    DeviceCommandPayload {
        op: DeviceOp::Open,
        platform: Some(DevicePlatform::Ios),
        model: Some("iPhone 17".into()),
        shutdown: None,
        action: None,
    }
}

fn op(op: DeviceOp) -> DeviceCommandPayload {
    DeviceCommandPayload {
        op,
        platform: None,
        model: None,
        shutdown: None,
        action: None,
    }
}

fn state_payload(state: DeviceState) -> DeviceRecordPayload {
    DeviceRecordPayload::State {
        state,
        platform: DevicePlatform::Ios,
        model: "iPhone 17".into(),
        os_version: "27.0".into(),
        drivers: vec![DeviceDriver::Agent, DeviceDriver::HostOwner],
        capture: DeviceCapture {
            mode: "snapshot-poll".into(),
            max_interval_ms: 3000,
        },
        reason: None,
    }
}

fn record(payload: DeviceRecordPayload) -> SessionDeviceRecord {
    let record_type = payload.record_type();
    SessionDeviceRecord {
        channel_id: channel(),
        target_key: target_key(),
        lifecycle_command_id: "lc-create-1".into(),
        slot: matches!(
            record_type,
            DeviceRecordType::State | DeviceRecordType::Shot
        )
        .then(|| SLOT.to_owned()),
        command_id: (record_type != DeviceRecordType::Availability).then(|| "dev-open-1".into()),
        snapshot: (record_type == DeviceRecordType::Shot)
            .then(|| EventId::from_hex(&"ef".repeat(32)).expect("id")),
        payload,
    }
}

fn availability() -> DeviceRecordPayload {
    DeviceRecordPayload::Availability {
        platforms: DevicePlatforms {
            ios: PlatformAvailability {
                available: true,
                reason: None,
            },
            android: PlatformAvailability {
                available: false,
                reason: Some("Android SDK not found".into()),
            },
        },
        agent_device: AgentDeviceAvailability {
            installed: true,
            version: Some("0.21.12".into()),
            reason: None,
        },
    }
}

#[test]
fn slot_id_is_sixteen_hex_and_hides_the_udid() {
    let provider = Keys::generate().public_key();
    let slot = device_slot_id(&provider, UDID);
    assert_eq!(slot.len(), DEVICE_SLOT_HEX_LEN);
    assert!(crate::surface_watch::validate_surface_key(Surface::Device, &slot).is_ok());
    assert_eq!(slot, device_slot_id(&provider, UDID));
    assert_ne!(slot, device_slot_id(&Keys::generate().public_key(), UDID));
    assert!(!slot.contains(&UDID[..8].to_lowercase()));
}

#[test]
fn commands_round_trip_and_enforce_per_op_fields() {
    let mut close = op(DeviceOp::Close);
    close.shutdown = Some(true);
    let mut action = op(DeviceOp::Action);
    action.action = Some(DeviceAction {
        action_type: "tap".into(),
        args: json!({"ref": "@e12"}),
    });
    for expected in [
        command(open_payload(), None),
        command(close, Some(SLOT)),
        command(op(DeviceOp::Screenshot), Some(SLOT)),
        command(action, Some(SLOT)),
    ] {
        let content = expected.content().expect("content");
        let parsed =
            validate_session_device_command_parts(&parts(&expected.tags()), &content).expect("ok");
        assert_eq!(parsed, expected);
    }

    let refused = [
        (
            command(open_payload(), Some(SLOT)).tags(),
            json!({"op":"open"}),
        ),
        (
            command(op(DeviceOp::Screenshot), None).tags(),
            json!({"op":"screenshot"}),
        ),
        (
            command(op(DeviceOp::Screenshot), Some(SLOT)).tags(),
            json!({"op":"screenshot","model":"iPhone 17"}),
        ),
        (
            command(op(DeviceOp::Close), Some(SLOT)).tags(),
            json!({"op":"close","action":{"type":"tap"}}),
        ),
        (
            command(op(DeviceOp::Action), Some(SLOT)).tags(),
            json!({"op":"action"}),
        ),
        (
            command(op(DeviceOp::Action), Some(SLOT)).tags(),
            json!({"op":"action","action":{"type":"Tap!","args":{}}}),
        ),
        (
            command(open_payload(), None).tags(),
            json!({"op":"open","udid":UDID}),
        ),
        (
            command(open_payload(), None).tags(),
            json!({"op":"open","model":UDID}),
        ),
        (command(open_payload(), None).tags(), json!({"op":"reboot"})),
    ];
    for (tags, content) in refused {
        assert!(
            validate_session_device_command_parts(&parts(&tags), &content.to_string()).is_err(),
            "accepted {content}"
        );
    }

    let good = command(open_payload(), None);
    let content = good.content().expect("content");
    for (index, value) in [
        (1, "sdv2"),
        (2, "coding-session/v1|bad"),
        (3, "has space"),
        (3, ""),
    ] {
        let mut rows = good.tags();
        rows[index][1] = value.into();
        assert!(validate_session_device_command_parts(&parts(&rows), &content).is_err());
    }
    let mut udid_slot = command(op(DeviceOp::Screenshot), Some(SLOT)).tags();
    udid_slot[4][1] = UDID.into();
    assert!(
        validate_session_device_command_parts(&parts(&udid_slot), "{\"op\":\"screenshot\"}")
            .is_err()
    );
}

#[test]
fn records_round_trip_for_every_type() {
    for expected in [
        record(availability()),
        record(state_payload(DeviceState::Open)),
        record(DeviceRecordPayload::Refused {
            code: DeviceRefusalCode::NoStanding,
            reason: "Only a seat of this session or its person may drive the device".into(),
        }),
        record(DeviceRecordPayload::Shot),
    ] {
        let content = expected.content().expect("content");
        let parsed =
            validate_session_device_record_parts(&parts(&expected.tags()), &content).expect("ok");
        assert_eq!(parsed, expected);
    }
    assert!(!record(state_payload(DeviceState::Booting)).is_terminal());
    assert!(record(state_payload(DeviceState::Closed)).is_terminal());
    assert!(record(DeviceRecordPayload::Shot).is_terminal());
    assert!(!record(availability()).is_terminal());
}

#[test]
fn records_refuse_mismatches_and_host_local_content() {
    let state = record(state_payload(DeviceState::Open));
    let content = state.content().expect("content");

    // Content type disagrees with sdv-type.
    let mut wrong_type = state.tags();
    wrong_type[4][1] = "shot".into();
    assert!(validate_session_device_record_parts(&parts(&wrong_type), &content).is_err());

    // State without its slot; availability with a slot; shot without e.
    let mut no_slot = state.clone();
    no_slot.slot = None;
    assert!(validate_session_device_record_parts(&parts(&no_slot.tags()), &content).is_err());
    let mut slotted = record(availability());
    slotted.slot = Some(SLOT.into());
    let avail = slotted.content().expect("content");
    assert!(validate_session_device_record_parts(&parts(&slotted.tags()), &avail).is_err());
    let mut shot = record(DeviceRecordPayload::Shot);
    shot.snapshot = None;
    assert!(
        validate_session_device_record_parts(&parts(&shot.tags()), "{\"type\":\"shot\"}").is_err()
    );

    // Host-local facts anywhere in the content.
    for leak in [
        json!({"type":"state","state":"open","platform":"ios","model":"iPhone 17","osVersion":"27.0",
               "drivers":["agent"],"capture":{"mode":"snapshot-poll","maxIntervalMs":3000},
               "reason":format!("booted {UDID}")}),
        json!({"type":"refused","code":"busy","reason":"see /Users/brian/.beekeeper/device/slot.json"}),
        json!({"type":"state","state":"open","platform":"ios","model":"iPhone 17","osVersion":"27.0",
               "drivers":["agent"],"capture":{"mode":"snapshot-poll","maxIntervalMs":3000},
               "daemonUrl":"http://127.0.0.1:4723"}),
        json!({"type":"state","state":"open","platform":"ios","model":"iPhone 17","osVersion":"27.0",
               "drivers":["agent","agent"],"capture":{"mode":"snapshot-poll","maxIntervalMs":3000}}),
    ] {
        let tags = if leak["type"] == "refused" {
            record(DeviceRecordPayload::Refused {
                code: DeviceRefusalCode::Busy,
                reason: "x".into(),
            })
            .tags()
        } else {
            state.tags()
        };
        assert!(
            validate_session_device_record_parts(&parts(&tags), &leak.to_string()).is_err(),
            "accepted {leak}"
        );
    }
}

fn sign_record(record: &SessionDeviceRecord, keys: &Keys, at: u64) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_SESSION_DEVICE_RECORD as u16),
        record.content().expect("content"),
    )
    .tags(
        record
            .tags()
            .into_iter()
            .map(|row| Tag::parse(row).expect("tag")),
    )
    .custom_created_at(Timestamp::from(at))
    .sign_with_keys(keys)
    .expect("sign")
}

#[test]
fn newest_state_record_for_the_slot_wins() {
    let first = Keys::generate();
    let second = Keys::generate();
    let open = record(state_payload(DeviceState::Open));
    let mut other_slot = open.clone();
    other_slot.slot = Some("0000000000000000".into());
    let events = [
        sign_record(&open, &first, 100),
        sign_record(&open, &second, 200),
        sign_record(&other_slot, &first, 300),
        sign_record(&record(DeviceRecordPayload::Shot), &first, 400),
    ];
    let (event, found) = newest_device_state_record(&events, channel(), SLOT).expect("found");
    assert_eq!(event.pubkey, second.public_key());
    assert_eq!(found.lifecycle_command_id, "lc-create-1");
    assert!(newest_device_state_record(&events, channel(), "1111111111111111").is_none());
}
