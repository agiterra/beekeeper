use nostr::{EventBuilder, Keys, Kind, Tag};

use super::*;

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const TARGET: &str = "coding-session/v1|6:claude4:inst36:5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a101:1";

fn event(keys: &Keys, kind: u16, tags: &[&[&str]], content: &str) -> Event {
    let tags: Vec<Tag> = tags
        .iter()
        .map(|t| Tag::parse(t.iter().copied()).expect("tag"))
        .collect();
    EventBuilder::new(Kind::from(kind), content)
        .tags(tags)
        .sign_with_keys(keys)
        .expect("sign")
}

fn command(keys: &Keys, slot: Option<&str>, content: &str) -> Event {
    let mut tags: Vec<&[&str]> = vec![
        &["h", CHANNEL],
        &["sdv-v", "sdv1"],
        &["cs-target", TARGET],
        &["sdv-cmd", "cmd-1"],
    ];
    let slot_tag;
    if let Some(slot) = slot {
        slot_tag = ["sdv-slot", slot];
        tags.push(&slot_tag);
    }
    event(keys, KIND_SESSION_DEVICE_COMMAND, &tags, content)
}

#[test]
fn every_op_decodes_and_open_defaults_to_ios() {
    let keys = Keys::generate();
    let open = decode_command(&command(
        &keys,
        None,
        r#"{"op":"open","model":"iPhone 17"}"#,
    ))
    .expect("open");
    assert_eq!(
        open.op,
        DeviceOp::Open {
            platform: "ios".into(),
            model: Some("iPhone 17".into())
        }
    );
    assert_eq!(
        (
            open.channel.as_str(),
            open.cs_target.as_str(),
            open.sdv_cmd.as_str()
        ),
        (CHANNEL, TARGET, "cmd-1")
    );
    assert_eq!(open.author, keys.public_key().to_hex());
    let slot = Some("0123456789abcdef");
    assert_eq!(
        decode_command(&command(&keys, slot, r#"{"op":"close","shutdown":true}"#))
            .expect("close")
            .op,
        DeviceOp::Close { shutdown: true }
    );
    assert_eq!(
        decode_command(&command(&keys, slot, r#"{"op":"screenshot"}"#))
            .expect("shot")
            .op,
        DeviceOp::Screenshot
    );
    assert_eq!(
        decode_command(&command(
            &keys,
            slot,
            r#"{"op":"action","action":{"type":"tap","args":{"ref":"@e1"}}}"#
        ))
        .expect("action")
        .op,
        DeviceOp::Action { kind: "tap".into() }
    );
}

#[test]
fn malformed_commands_are_refused_structurally() {
    let keys = Keys::generate();
    let slot = Some("0123456789abcdef");
    for (slot, content) in [
        (None, r#"{"op":"reboot"}"#),
        (None, r#"{"op":"open","extra":1}"#),
        (slot, r#"{"op":"open"}"#),
        (None, r#"{"op":"screenshot"}"#),
        (slot, r#"{"op":"screenshot","model":"x"}"#),
        (None, r#"{"op":"open","platform":"windows"}"#),
        (slot, r#"{"op":"action","action":{"type":"TAP"}}"#),
        (Some("ABCDEF0123456789"), r#"{"op":"screenshot"}"#),
        (None, "not json"),
    ] {
        assert!(
            decode_command(&command(&keys, slot, content)).is_err(),
            "{slot:?} {content}"
        );
    }
    let reordered = event(
        &keys,
        KIND_SESSION_DEVICE_COMMAND,
        &[
            &["sdv-v", "sdv1"],
            &["h", CHANNEL],
            &["cs-target", TARGET],
            &["sdv-cmd", "c"],
        ],
        r#"{"op":"open"}"#,
    );
    assert!(decode_command(&reordered).is_err());
    let bad_id = event(
        &keys,
        KIND_SESSION_DEVICE_COMMAND,
        &[
            &["h", CHANNEL],
            &["sdv-v", "sdv1"],
            &["cs-target", TARGET],
            &["sdv-cmd", "has space"],
        ],
        r#"{"op":"open"}"#,
    );
    assert!(decode_command(&bad_id).is_err());
}

#[test]
fn watches_decode_only_for_device_addressed_to_me() {
    let keys = Keys::generate();
    let me = Keys::generate().public_key().to_hex();
    let watch = |surface: &str, p: &str, content: &str| {
        event(
            &keys,
            KIND_SURFACE_WATCH,
            &[
                &["h", CHANNEL],
                &["surface", surface],
                &["d", "0123456789abcdef"],
                &["p", p],
            ],
            content,
        )
    };
    let request = decode_watch(&watch("device", &me, r#"{"action":"watch"}"#), &me).expect("watch");
    assert_eq!(request.action, WatchAction::Watch);
    assert_eq!(request.slot, "0123456789abcdef");
    assert_eq!(
        decode_watch(&watch("device", &me, r#"{"action":"snapshot"}"#), &me)
            .expect("s")
            .action,
        WatchAction::Snapshot
    );
    assert!(decode_watch(&watch("preview", &me, r#"{"action":"watch"}"#), &me).is_err());
    assert!(decode_watch(
        &watch("device", &"0".repeat(64), r#"{"action":"watch"}"#),
        &me
    )
    .is_err());
    assert!(decode_watch(&watch("device", &me, r#"{"action":"dance"}"#), &me).is_err());
    assert!(decode_watch(&watch("device", &me, r#"{"action":"watch","x":1}"#), &me).is_err());
}

fn names(tags: &[Vec<String>]) -> Vec<&str> {
    tags.iter().map(|t| t[0].as_str()).collect()
}

#[test]
fn records_snapshots_and_frames_carry_tags_in_wire_order() {
    let scope = RecordScope {
        channel: CHANNEL.into(),
        cs_target: TARGET.into(),
        csl_command: "cmd-0".into(),
    };
    assert_eq!(
        names(&record_tags(
            &scope,
            RecordType::Availability,
            None,
            None,
            None
        )),
        ["h", "sdv-v", "cs-target", "csl-command", "sdv-type"]
    );
    let shot = record_tags(
        &scope,
        RecordType::Shot,
        Some("0123456789abcdef"),
        Some("c"),
        Some(&"e".repeat(64)),
    );
    assert_eq!(
        names(&shot),
        [
            "h",
            "sdv-v",
            "cs-target",
            "csl-command",
            "sdv-type",
            "sdv-slot",
            "sdv-cmd",
            "e"
        ]
    );
    assert_eq!(
        shot[7],
        vec![
            "e".to_owned(),
            "e".repeat(64),
            String::new(),
            "snapshot".into()
        ]
    );
    let fields = SnapshotFields {
        channel: CHANNEL,
        slot: "0123456789abcdef",
        sha256: &"a".repeat(64),
        url: "https://hive.example/aaaa.png",
        mime: "image/png",
        width: 1206,
        height: 2622,
        taken_at_ms: 1_791_374_512_345,
        provider: &"b".repeat(64),
        requested_by: Some(&"c".repeat(64)),
        command_event: Some(&"d".repeat(64)),
    };
    let snapshot = snapshot_tags(&fields);
    assert_eq!(
        names(&snapshot),
        [
            "h", "ssn-v", "ssn-type", "surface", "d", "x", "url", "m", "dim", "taken-at",
            "provider", "p", "e"
        ]
    );
    assert_eq!(snapshot[8][1], "1206x2622");
    assert_eq!(snapshot[11][3], "requested-by");
    assert_eq!(snapshot[12][3], "command");
    let header = FrameHeader {
        channel: CHANNEL.into(),
        slot: "0123456789abcdef".into(),
        seq: 4,
        epoch: 9,
        cadence_ms: 3000,
        width: 414,
        height: 900,
        captured_at_ms: 10,
    };
    let frame = frame_tags(&header, FrameType::Frame);
    assert_eq!(
        names(&frame),
        [
            "h",
            "surface",
            "d",
            "t",
            "seq",
            "epoch",
            "cadence-ms",
            "dim",
            "captured-at"
        ]
    );
    assert_eq!(frame[3][1], "frame");
    assert_eq!(frame_tags(&header, FrameType::Paused)[3][1], "paused");
}

#[test]
fn record_contents_match_the_wire_examples() {
    let availability: serde_json::Value = serde_json::from_str(&availability_content(
        &PlatformAvailability {
            available: true,
            reason: None,
        },
        &PlatformAvailability {
            available: false,
            reason: Some("no".into()),
        },
        &AgentDeviceAvailability {
            installed: false,
            version: None,
            reason: Some("Node 22.12 or newer not found".into()),
        },
    ))
    .expect("json");
    assert_eq!(availability["type"], "availability");
    assert_eq!(
        availability["platforms"]["ios"],
        serde_json::json!({"available": true})
    );
    assert_eq!(
        availability["agentDevice"]["reason"],
        "Node 22.12 or newer not found"
    );
    let state: serde_json::Value = serde_json::from_str(
        &StateContent {
            state: "open",
            platform: "ios".into(),
            model: "iPhone 17".into(),
            os_version: "27.0".into(),
            drivers: vec!["host-owner"],
            reason: None,
        }
        .to_json(),
    )
    .expect("json");
    assert_eq!(
        state["capture"],
        serde_json::json!({"mode": "snapshot-poll", "maxIntervalMs": 3000})
    );
    assert_eq!(state["osVersion"], "27.0");
    let refused: serde_json::Value =
        serde_json::from_str(&refused_content(RefusalCode::NoStanding, "r")).expect("json");
    assert_eq!(refused["code"], "no_standing");
}

#[test]
fn signing_refuses_host_local_values() {
    let keys = Keys::generate();
    let tags = vec![
        vec!["h".to_owned(), CHANNEL.to_owned()],
        vec!["d".into(), "70E4C638-DE97-4AB1-990D-D0FC30018372".into()],
    ];
    assert!(sign(&keys, KIND_SURFACE_FRAME, tags, String::new()).is_err());
    let tags = vec![vec!["h".to_owned(), CHANNEL.to_owned()]];
    assert!(sign(
        &keys,
        KIND_SESSION_DEVICE_RECORD,
        tags.clone(),
        r#"{"reason":"see /Users/brian/x"}"#.into()
    )
    .is_err());
    assert!(sign(&keys, KIND_SESSION_DEVICE_RECORD, tags, "{}".into()).is_ok());
}
