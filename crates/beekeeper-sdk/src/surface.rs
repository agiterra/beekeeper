//! Typed builders for the shared-observation kinds: NIP-SW surface watch
//! (24320), surface frame (24321) and surface snapshot (44253), NIP-SP
//! preview announce (30626), and NIP-SDV device command (44254) and record
//! (44255).
//!
//! Every builder serializes the typed value with its own `tags()` and runs
//! the same core validator the relay runs before returning bytes to a
//! signer, so nothing a builder returns is refused at ingest — including
//! the host-local rule (no UDID, no home or simulator path, no localhost
//! URL). The caller signs and publishes.

use beekeeper_core::kind::{
    KIND_SESSION_DEVICE_COMMAND, KIND_SESSION_DEVICE_RECORD, KIND_SESSION_PREVIEW_ANNOUNCE,
    KIND_SURFACE_FRAME, KIND_SURFACE_SNAPSHOT, KIND_SURFACE_WATCH,
};
use beekeeper_core::session_device::{
    validate_session_device_command_parts, validate_session_device_record_parts,
    SessionDeviceCommand, SessionDeviceRecord,
};
use beekeeper_core::session_preview::{
    validate_session_preview_announce_parts, SessionPreviewAnnounce,
};
use beekeeper_core::surface_snapshot::{validate_surface_snapshot_parts, SurfaceSnapshot};
use beekeeper_core::surface_watch::{
    validate_surface_frame_parts, validate_surface_watch_parts, SurfaceFrameHeader, SurfaceWatch,
};
use nostr::{EventBuilder, Kind, Tag};

use crate::SdkError;

/// Build a kind 24320 surface watch addressed to `watch.producer`.
pub fn build_surface_watch(watch: &SurfaceWatch) -> Result<EventBuilder, SdkError> {
    let rows = watch.tags();
    let content = watch.content();
    validate_surface_watch_parts(&as_parts(&rows), &content).map_err(SdkError::InvalidInput)?;
    finish(KIND_SURFACE_WATCH, rows, content)
}

/// Build a kind 24321 surface frame. `content` is the base64 JPEG for a
/// `frame` (see `beekeeper_core::surface_watch::encode_frame_content`) and
/// empty for `paused` and `end`.
pub fn build_surface_frame(
    header: &SurfaceFrameHeader,
    content: &str,
) -> Result<EventBuilder, SdkError> {
    let rows = header.tags();
    validate_surface_frame_parts(&as_parts(&rows), content).map_err(SdkError::InvalidInput)?;
    finish(KIND_SURFACE_FRAME, rows, content.to_owned())
}

/// Build a kind 30626 session preview announce (content is empty). Redact
/// the page first with `beekeeper_core::session_preview::redact_page_url`.
pub fn build_session_preview_announce(
    announce: &SessionPreviewAnnounce,
) -> Result<EventBuilder, SdkError> {
    let rows = announce.tags();
    validate_session_preview_announce_parts(&as_parts(&rows), "")
        .map_err(SdkError::InvalidInput)?;
    finish(KIND_SESSION_PREVIEW_ANNOUNCE, rows, String::new())
}

/// Build a kind 44253 surface snapshot; its alt text is `snapshot.alt`.
pub fn build_surface_snapshot(snapshot: &SurfaceSnapshot) -> Result<EventBuilder, SdkError> {
    let rows = snapshot.tags();
    validate_surface_snapshot_parts(&as_parts(&rows), &snapshot.alt)
        .map_err(SdkError::InvalidInput)?;
    finish(KIND_SURFACE_SNAPSHOT, rows, snapshot.alt.clone())
}

/// Build a kind 44254 session device command. `command.command_id` must be
/// caller-chosen and stable across retries (never derived from the clock).
pub fn build_session_device_command(
    command: &SessionDeviceCommand,
) -> Result<EventBuilder, SdkError> {
    let rows = command.tags();
    let content = command.content().map_err(SdkError::InvalidInput)?;
    validate_session_device_command_parts(&as_parts(&rows), &content)
        .map_err(SdkError::InvalidInput)?;
    finish(KIND_SESSION_DEVICE_COMMAND, rows, content)
}

/// Build a kind 44255 provider-signed session device record.
pub fn build_session_device_record(record: &SessionDeviceRecord) -> Result<EventBuilder, SdkError> {
    let rows = record.tags();
    let content = record.content().map_err(SdkError::InvalidInput)?;
    validate_session_device_record_parts(&as_parts(&rows), &content)
        .map_err(SdkError::InvalidInput)?;
    finish(KIND_SESSION_DEVICE_RECORD, rows, content)
}

fn as_parts(rows: &[Vec<String>]) -> Vec<&[String]> {
    rows.iter().map(Vec::as_slice).collect()
}

fn finish(kind: u32, rows: Vec<Vec<String>>, content: String) -> Result<EventBuilder, SdkError> {
    let kind =
        u16::try_from(kind).map_err(|_| SdkError::InvalidInput("kind exceeds u16".into()))?;
    let tags = rows
        .into_iter()
        .map(|row| Tag::parse(row).map_err(|error| SdkError::InvalidTag(error.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventBuilder::new(Kind::Custom(kind), content).tags(tags))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core::session_device::{validate_session_device_command_envelope, *};
    use beekeeper_core::session_preview::{
        validate_session_preview_announce_envelope, PreviewStatus, PreviewStream,
    };
    use beekeeper_core::surface_snapshot::{
        validate_surface_snapshot_envelope, SurfaceSnapshotType,
    };
    use beekeeper_core::surface_watch::{
        encode_frame_content, validate_surface_frame_envelope, validate_surface_watch_envelope,
        Surface, SurfaceFrameType, SurfaceWatchAction,
    };
    use nostr::Keys;
    use uuid::Uuid;

    const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    const SLOT: &str = "9f2c4e1a7b3d5c80";

    fn target_key() -> String {
        "coding-session/v1|6:claude6:inst-11:S1:2".into()
    }

    #[test]
    fn every_builder_signs_what_the_relay_accepts() {
        let keys = Keys::generate();
        let channel = Uuid::new_v4();
        let watch = SurfaceWatch {
            channel_id: channel,
            surface: Surface::Device,
            key: SLOT.into(),
            // nostr drops a self `p` tag at signing; a watch always names
            // someone else.
            producer: Keys::generate().public_key(),
            action: SurfaceWatchAction::Snapshot,
        };
        let event = build_surface_watch(&watch)
            .and_then(|b| {
                b.sign_with_keys(&keys)
                    .map_err(|e| SdkError::InvalidInput(e.to_string()))
            })
            .expect("watch");
        assert_eq!(
            validate_surface_watch_envelope(&event).expect("valid"),
            watch
        );

        let frame = SurfaceFrameHeader {
            channel_id: channel,
            surface: Surface::Preview,
            key: SESSION_REF.into(),
            frame_type: SurfaceFrameType::Frame,
            seq: 1,
            epoch: 7,
            cadence_ms: 2000,
            width: 640,
            height: 400,
            captured_at_ms: 1_791_374_512_345,
            actor: None,
            commit: None,
        };
        let jpeg = encode_frame_content(&[0xFF, 0xD8, 0xFF, 0xE0]);
        let event = build_surface_frame(&frame, &jpeg)
            .expect("frame")
            .sign_with_keys(&keys)
            .expect("sign");
        assert_eq!(
            validate_surface_frame_envelope(&event).expect("valid"),
            frame
        );
        assert!(build_surface_frame(&frame, "not a jpeg").is_err());

        let announce = SessionPreviewAnnounce {
            channel_id: channel,
            session_ref: SESSION_REF.into(),
            status: PreviewStatus::Open,
            target_key: Some(target_key()),
            provider: None,
            page: Some("local:/".into()),
            title: Some(String::new()),
            viewport_width: 1280,
            viewport_height: 800,
            stream: PreviewStream::Frames,
        };
        let event = build_session_preview_announce(&announce)
            .expect("announce")
            .sign_with_keys(&keys)
            .expect("sign");
        assert_eq!(
            validate_session_preview_announce_envelope(&event).expect("valid"),
            announce
        );
        let mut leaky = announce.clone();
        leaky.page = Some("http://localhost:5173/".into());
        assert!(build_session_preview_announce(&leaky).is_err());

        let sha = "cd".repeat(32);
        let snapshot = SurfaceSnapshot {
            channel_id: channel,
            snapshot_type: SurfaceSnapshotType::Snapshot,
            surface: Surface::Device,
            key: SLOT.into(),
            url: format!("https://hive.agiterra.org/{sha}"),
            sha256: sha,
            mime: "image/png".into(),
            width: 1179,
            height: 2556,
            taken_at_ms: 1,
            provider: keys.public_key(),
            requested_by: None,
            commit: None,
            reference: None,
            page: None,
            title: None,
            alt: String::new(),
        };
        let event = build_surface_snapshot(&snapshot)
            .expect("snapshot")
            .sign_with_keys(&keys)
            .expect("sign");
        assert_eq!(
            validate_surface_snapshot_envelope(&event).expect("valid"),
            snapshot
        );

        let command = SessionDeviceCommand {
            channel_id: channel,
            target_key: target_key(),
            command_id: "dev-1".into(),
            slot: None,
            payload: DeviceCommandPayload {
                op: DeviceOp::Open,
                platform: Some(DevicePlatform::Ios),
                model: Some("iPhone 17".into()),
                shutdown: None,
                action: None,
            },
        };
        let event = build_session_device_command(&command)
            .expect("command")
            .sign_with_keys(&keys)
            .expect("sign");
        assert_eq!(
            validate_session_device_command_envelope(&event).expect("valid"),
            command
        );

        let record = SessionDeviceRecord {
            channel_id: channel,
            target_key: target_key(),
            lifecycle_command_id: "lc-1".into(),
            slot: None,
            command_id: Some("dev-1".into()),
            snapshot: None,
            payload: DeviceRecordPayload::Refused {
                code: DeviceRefusalCode::PlatformUnavailable,
                reason: "Not offered by this machine: Xcode not found".into(),
            },
        };
        let event = build_session_device_record(&record)
            .expect("record")
            .sign_with_keys(&keys)
            .expect("sign");
        assert_eq!(
            validate_session_device_record_envelope(&event).expect("valid"),
            record
        );
        let mut leaky = record;
        leaky.payload = DeviceRecordPayload::Refused {
            code: DeviceRefusalCode::Busy,
            reason: "slot file /Users/brian/.state/device/x.json is locked".into(),
        };
        assert!(build_session_device_record(&leaky).is_err());
    }
}
