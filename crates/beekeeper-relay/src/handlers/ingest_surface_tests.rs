//! The relay's half of the shared-observation kinds (NIP-SW, NIP-SP,
//! NIP-SDV): scope, channel scoping, the strict membership gate, structure,
//! and the host-local rule.
//!
//! Membership and frame authority need a database and a live socket; they
//! are exercised end to end by `beekeeper-test-client/tests/e2e_surface_watch.rs`
//! (non-member refused, frame from a non-announcer refused, oversize frame
//! refused, watch delivered only to its `p`). The authority decision itself
//! is `surface_watch::frame_author_admitted`, unit-tested beside it.

use beekeeper_core::kind::{
    KIND_SESSION_DEVICE_COMMAND, KIND_SESSION_DEVICE_RECORD, KIND_SESSION_PREVIEW_ANNOUNCE,
    KIND_SURFACE_FRAME, KIND_SURFACE_SNAPSHOT, KIND_SURFACE_WATCH,
};
use beekeeper_core::session_device::{
    DeviceCapture, DeviceCommandPayload, DeviceDriver, DeviceOp, DevicePlatform,
    DeviceRecordPayload, DeviceState, SessionDeviceCommand, SessionDeviceRecord,
};
use beekeeper_core::session_preview::{PreviewStatus, PreviewStream, SessionPreviewAnnounce};
use beekeeper_core::surface_snapshot::{SurfaceSnapshot, SurfaceSnapshotType};
use beekeeper_core::surface_watch::{
    validate_surface_frame_envelope, validate_surface_watch_envelope, Surface, SurfaceFrameHeader,
    SurfaceFrameType, SurfaceWatch, SurfaceWatchAction, MAX_SURFACE_FRAME_CONTENT_BYTES,
};
use beekeeper_sdk::surface::{
    build_session_device_command, build_session_device_record, build_session_preview_announce,
    build_surface_snapshot,
};

use super::*;

const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const SLOT: &str = "9f2c4e1a7b3d5c80";
const UDID: &str = "70E4C638-1A2B-4C3D-9E8F-0123456789AB";
const HOME_PATH: &str = "/Users/brian/Library/Developer/CoreSimulator/Devices/x";
const STORED_KINDS: [u32; 4] = [
    KIND_SESSION_PREVIEW_ANNOUNCE,
    KIND_SURFACE_SNAPSHOT,
    KIND_SESSION_DEVICE_COMMAND,
    KIND_SESSION_DEVICE_RECORD,
];

fn dummy_event() -> Event {
    nostr::EventBuilder::new(nostr::Kind::Custom(KIND_STREAM_MESSAGE as u16), "hello")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign")
}

fn target_key() -> String {
    "coding-session/v1|6:claude6:inst-11:S1:2".into()
}

fn sign_rows(kind: u32, content: &str, rows: Vec<Vec<String>>) -> Event {
    nostr::EventBuilder::new(nostr::Kind::Custom(kind as u16), content)
        .tags(
            rows.into_iter()
                .map(|row| nostr::Tag::parse(row).expect("tag")),
        )
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign")
}

/// Every valid stored event, signed, paired with its tags and content so a
/// test can corrupt one value at a time.
fn stored_fixtures(channel: Uuid) -> Vec<(u32, Vec<Vec<String>>, String)> {
    let keys = nostr::Keys::generate();
    let announce = SessionPreviewAnnounce {
        channel_id: channel,
        session_ref: SESSION_REF.into(),
        status: PreviewStatus::Open,
        target_key: Some(target_key()),
        provider: Some(keys.public_key()),
        page: Some("local:/settings".into()),
        title: Some("Settings".into()),
        viewport_width: 1280,
        viewport_height: 800,
        stream: PreviewStream::Frames,
    };
    let sha = "cd".repeat(32);
    let snapshot = SurfaceSnapshot {
        channel_id: channel,
        snapshot_type: SurfaceSnapshotType::Snapshot,
        surface: Surface::Device,
        key: SLOT.into(),
        url: format!("https://hive.agiterra.org/{sha}.png"),
        sha256: sha,
        mime: "image/png".into(),
        width: 1179,
        height: 2556,
        taken_at_ms: 1_791_374_512_345,
        provider: keys.public_key(),
        requested_by: None,
        commit: None,
        reference: None,
        page: None,
        title: None,
        alt: "Home screen".into(),
    };
    let command = SessionDeviceCommand {
        channel_id: channel,
        target_key: target_key(),
        command_id: "dev-shot-1".into(),
        slot: Some(SLOT.into()),
        payload: DeviceCommandPayload {
            op: DeviceOp::Screenshot,
            platform: None,
            model: None,
            shutdown: None,
            action: None,
        },
    };
    let record = SessionDeviceRecord {
        channel_id: channel,
        target_key: target_key(),
        lifecycle_command_id: "lc-create-1".into(),
        slot: Some(SLOT.into()),
        command_id: Some("dev-open-1".into()),
        snapshot: None,
        payload: DeviceRecordPayload::State {
            state: DeviceState::Open,
            platform: DevicePlatform::Ios,
            model: "iPhone 17".into(),
            os_version: "27.0".into(),
            drivers: vec![DeviceDriver::Agent, DeviceDriver::HostOwner],
            capture: DeviceCapture {
                mode: "snapshot-poll".into(),
                max_interval_ms: 3000,
            },
            reason: None,
        },
    };
    // The builders run the same validators; building proves the fixtures
    // are what an honest producer signs.
    build_session_preview_announce(&announce).expect("announce builds");
    build_surface_snapshot(&snapshot).expect("snapshot builds");
    build_session_device_command(&command).expect("command builds");
    build_session_device_record(&record).expect("record builds");
    vec![
        (
            KIND_SESSION_PREVIEW_ANNOUNCE,
            announce.tags(),
            String::new(),
        ),
        (KIND_SURFACE_SNAPSHOT, snapshot.tags(), snapshot.alt.clone()),
        (
            KIND_SESSION_DEVICE_COMMAND,
            command.tags(),
            command.content().expect("content"),
        ),
        (
            KIND_SESSION_DEVICE_RECORD,
            record.tags(),
            record.content().expect("content"),
        ),
    ]
}

/// 30626 and 44253–44255 are channel-scoped message writes under the strict
/// coding-session membership gate (active member or transport writer, no
/// open-channel fallback); none is global-only.
#[test]
fn stored_surface_kinds_are_strict_channel_scoped_message_writes() {
    let dummy = dummy_event();
    for kind in STORED_KINDS {
        assert_eq!(
            required_scope_for_kind(kind, &dummy).unwrap(),
            Scope::MessagesWrite,
            "kind {kind}"
        );
        assert!(requires_h_channel_scope(kind), "kind {kind}");
        assert!(!is_global_only_kind(kind), "kind {kind}");
        assert!(is_coding_session_kind(kind), "kind {kind}");
        assert!(
            requires_strict_coding_session_membership(kind),
            "kind {kind}"
        );
    }
    // The pair is WebSocket-only, never ingested through POST /events.
    assert!(websocket_only_ingest_kind(KIND_SURFACE_WATCH));
    assert!(websocket_only_ingest_kind(KIND_SURFACE_FRAME));
    for kind in STORED_KINDS {
        assert!(!websocket_only_ingest_kind(kind));
    }
}

#[test]
fn stored_surface_kinds_run_their_envelope_validators() {
    let channel = Uuid::new_v4();
    for (kind, rows, content) in stored_fixtures(channel) {
        let event = sign_rows(kind, &content, rows.clone());
        assert!(
            surface_shared_record_envelope(kind, &event).is_ok(),
            "valid kind {kind} refused"
        );
        // An extra tag, and a reordered pair, are refused.
        let mut extra = rows.clone();
        extra.push(vec!["udid".into(), "x".into()]);
        assert!(surface_shared_record_envelope(kind, &sign_rows(kind, &content, extra)).is_err());
        let mut swapped = rows.clone();
        swapped.swap(0, 1);
        assert!(
            surface_shared_record_envelope(kind, &sign_rows(kind, &content, swapped)).is_err(),
            "reordered kind {kind} accepted"
        );
    }
    // Other kinds pass untouched.
    assert!(surface_shared_record_envelope(KIND_STREAM_MESSAGE, &dummy_event()).is_ok());
}

/// The never-on-the-relay rule: a UUID-shaped value (a simulator UDID) or a
/// home/simulator path in any tag of these kinds is refused, wherever the
/// producer put it — except `h` (a channel UUID) and the minted ids that may
/// embed one (`cs-target`, `csl-command`, `sdv-cmd`; a sessionRef `d`).
#[test]
fn uuid_shaped_and_home_path_tag_values_are_refused() {
    let channel = Uuid::new_v4();
    let minted = ["h", "cs-target", "csl-command", "sdv-cmd"];
    for (kind, rows, content) in stored_fixtures(channel) {
        for index in 0..rows.len() {
            let name = rows[index][0].clone();
            for leak in [UDID.to_owned(), UDID.to_lowercase(), HOME_PATH.to_owned()] {
                let exempt_uuid = minted.contains(&name.as_str())
                    || (name == "d" && kind == KIND_SESSION_PREVIEW_ANNOUNCE);
                if exempt_uuid && leak != HOME_PATH {
                    continue;
                }
                let mut leaky = rows.clone();
                leaky[index][1] = format!("{}{leak}", if name == "page" { "local:/" } else { "" });
                let event = sign_rows(kind, &content, leaky);
                assert!(
                    surface_shared_record_envelope(kind, &event).is_err(),
                    "kind {kind} accepted {name}={leak}"
                );
            }
        }
    }
}

#[test]
fn ephemeral_pair_refuses_oversize_udid_and_home_paths() {
    let channel = Uuid::new_v4();
    let producer = nostr::Keys::generate().public_key();
    let watch = SurfaceWatch {
        channel_id: channel,
        surface: Surface::Device,
        key: SLOT.into(),
        producer,
        action: SurfaceWatchAction::Watch,
    };
    let good = sign_rows(KIND_SURFACE_WATCH, &watch.content(), watch.tags());
    assert!(validate_surface_watch_envelope(&good).is_ok());
    let mut udid = watch.tags();
    udid[2][1] = UDID.into();
    assert!(validate_surface_watch_envelope(&sign_rows(
        KIND_SURFACE_WATCH,
        &watch.content(),
        udid
    ))
    .is_err());

    let frame = SurfaceFrameHeader {
        channel_id: channel,
        surface: Surface::Device,
        key: SLOT.into(),
        frame_type: SurfaceFrameType::Frame,
        seq: 1,
        epoch: 1,
        cadence_ms: 3000,
        width: 414,
        height: 896,
        captured_at_ms: 1_791_374_512_345,
        actor: None,
        commit: None,
    };
    let jpeg = format!("/9j/{}", "A".repeat(1020));
    assert!(
        validate_surface_frame_envelope(&sign_rows(KIND_SURFACE_FRAME, &jpeg, frame.tags()))
            .is_ok()
    );
    let oversize = format!("/9j/{}", "A".repeat(MAX_SURFACE_FRAME_CONTENT_BYTES));
    assert!(validate_surface_frame_envelope(&sign_rows(
        KIND_SURFACE_FRAME,
        &oversize,
        frame.tags()
    ))
    .is_err());
    let mut home = frame.tags();
    home[2][1] = HOME_PATH.into();
    assert!(validate_surface_frame_envelope(&sign_rows(KIND_SURFACE_FRAME, &jpeg, home)).is_err());
    let mut udid = frame.tags();
    udid[2][1] = UDID.into();
    assert!(validate_surface_frame_envelope(&sign_rows(KIND_SURFACE_FRAME, &jpeg, udid)).is_err());
}

/// A frame is admitted only from the announced producer; nobody is the
/// producer of a surface nobody announced.
#[test]
fn frame_from_a_non_authority_is_refused() {
    use super::super::surface_watch::frame_author_admitted;
    use beekeeper_core::session_preview::resolve_preview_owner;

    let channel = Uuid::new_v4();
    let host = nostr::Keys::generate();
    let stranger = nostr::Keys::generate();
    let (_, rows, _) = stored_fixtures(channel).remove(0);
    let announce = nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_SESSION_PREVIEW_ANNOUNCE as u16),
        "",
    )
    .tags(
        rows.into_iter()
            .map(|row| nostr::Tag::parse(row).expect("tag")),
    )
    .sign_with_keys(&host)
    .expect("sign");
    let owner = resolve_preview_owner(&[announce], channel, SESSION_REF);
    assert_eq!(owner, Some(host.public_key()));
    assert!(frame_author_admitted(&host.public_key(), owner.as_ref()));
    assert!(!frame_author_admitted(
        &stranger.public_key(),
        owner.as_ref()
    ));
    assert!(!frame_author_admitted(
        &host.public_key(),
        resolve_preview_owner(&[], channel, SESSION_REF).as_ref()
    ));
}
