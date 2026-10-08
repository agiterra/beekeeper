use super::support::*;
use super::watch::*;
use super::*;

use std::ffi::OsString;

use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::preview_grant::{
    mint_preview_grant, preview_grant_audience, PreviewGrantRequest,
};
use beekeeper_core::session_device::{
    AgentDeviceAvailability, DevicePlatforms, PlatformAvailability,
};
use beekeeper_core::session_device::{
    DeviceCapture, DeviceDriver, DeviceRecordPayload, DeviceRefusalCode, DeviceState,
    SessionDeviceRecord,
};
use beekeeper_core::session_preview::{PreviewStatus, PreviewStream, SessionPreviewAnnounce};
use beekeeper_core::surface_watch::{
    encode_frame_content, Surface, SurfaceFrameHeader, SurfaceFrameType,
};
use clap::{CommandFactory, Parser};
use nostr::{EventBuilder, Keys, Timestamp};
use uuid::Uuid;

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const SLOT: &str = "9f2c4e1a7b3d5c80";
const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

#[derive(Debug, Parser)]
#[command(name = "device")]
struct TestCli {
    #[command(subcommand)]
    cmd: DeviceCmd,
}

fn parse(args: &[&str]) -> Result<DeviceCmd, clap::Error> {
    let mut argv = vec!["device"];
    argv.extend_from_slice(args);
    TestCli::try_parse_from(argv).map(|cli| cli.cmd)
}

fn channel() -> Uuid {
    Uuid::parse_str(CHANNEL).expect("uuid")
}

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        session_id: "S".into(),
        generation: 2,
    }
}

fn target_key() -> String {
    coding_session_target_key(&target())
}

fn ctx(provider: Option<&Keys>) -> DeviceContext {
    DeviceContext {
        channel: channel(),
        target_key: Some(target_key()),
        provider: provider.map(Keys::public_key),
    }
}

fn sign(builder: EventBuilder, keys: &Keys, at: u64) -> nostr::Event {
    builder
        .custom_created_at(Timestamp::from(at))
        .sign_with_keys(keys)
        .expect("sign")
}

fn record(
    payload: DeviceRecordPayload,
    slot: Option<&str>,
    cmd: Option<&str>,
) -> SessionDeviceRecord {
    let snapshot = matches!(payload, DeviceRecordPayload::Shot)
        .then(|| nostr::EventId::from_hex(&"ab".repeat(32)).expect("id"));
    SessionDeviceRecord {
        channel_id: channel(),
        target_key: target_key(),
        lifecycle_command_id: "csl-1".into(),
        slot: slot.map(str::to_owned),
        command_id: cmd.map(str::to_owned),
        snapshot,
        payload,
    }
}

fn state(state: DeviceState) -> DeviceRecordPayload {
    DeviceRecordPayload::State {
        state,
        platform: DevicePlatform::Ios,
        model: "iPhone 17".into(),
        os_version: "27.0".into(),
        drivers: vec![DeviceDriver::Agent],
        capture: DeviceCapture {
            mode: "snapshot-poll".into(),
            max_interval_ms: 3000,
        },
        reason: (state == DeviceState::Failed).then(|| "Xcode not found".into()),
    }
}

fn signed_record(rec: &SessionDeviceRecord, keys: &Keys, at: u64) -> nostr::Event {
    sign(
        beekeeper_sdk::surface::build_session_device_record(rec).expect("record"),
        keys,
        at,
    )
}

#[test]
fn every_subcommand_parses() {
    TestCli::command().debug_assert();
    let mut names: Vec<String> = TestCli::command()
        .get_subcommands()
        .map(|sub| sub.get_name().to_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["close", "list", "open", "screenshot", "watch"]);

    assert!(matches!(
        parse(&["list", "--channel", CHANNEL]),
        Ok(DeviceCmd::List { .. })
    ));
    match parse(&[
        "open",
        "--model",
        "iPhone 17",
        "--wait",
        "30",
        "--command-id",
        "c-1",
    ]) {
        Ok(DeviceCmd::Open {
            model,
            wait,
            command_id,
            platform: PlatformArg::Ios,
            ..
        }) => {
            assert_eq!(model.as_deref(), Some("iPhone 17"));
            assert_eq!(wait, 30);
            assert_eq!(command_id.as_deref(), Some("c-1"));
        }
        other => panic!("open: {other:?}"),
    }
    match parse(&["screenshot", "--slot", SLOT, "--out", "shot.png"]) {
        Ok(DeviceCmd::Screenshot {
            slot, out, wait, ..
        }) => {
            assert_eq!(slot.as_deref(), Some(SLOT));
            assert_eq!(out, Some(PathBuf::from("shot.png")));
            assert_eq!(wait, 120);
        }
        other => panic!("screenshot: {other:?}"),
    }
    assert!(matches!(
        parse(&["close", "--shutdown"]),
        Ok(DeviceCmd::Close { shutdown: true, .. })
    ));
    match parse(&[
        "watch",
        "--surface",
        "preview",
        "--count",
        "2",
        "--snapshot",
    ]) {
        Ok(DeviceCmd::Watch {
            surface,
            count,
            timeout,
            snapshot,
            ..
        }) => {
            assert_eq!(surface, SurfaceArg::Preview);
            assert_eq!((count, timeout, snapshot), (2, 60, true));
        }
        other => panic!("watch: {other:?}"),
    }
    assert!(parse(&["watch"]).is_err(), "watch names its surface");
}

#[test]
fn the_context_comes_from_a_minted_grant_and_flags_override_it() {
    let provider = Keys::generate();
    let owner = Keys::generate();
    let token = mint_preview_grant(
        &provider,
        &PreviewGrantRequest {
            channel_id: channel(),
            target: target(),
            execution_id: "exec-1".into(),
            audience: preview_grant_audience(&owner.public_key()),
            ttl_secs: 3600,
        },
        Timestamp::now().as_secs(),
    )
    .expect("mint");

    let from_grant = resolve_context(None, None, None, Some(&token)).expect("context");
    assert_eq!(from_grant, ctx(Some(&provider)));

    let other = "11111111-2222-4333-8444-555555555555";
    let flagged = resolve_context(Some(other), None, None, Some(&token)).expect("context");
    assert_eq!(flagged.channel.to_string(), other);
    assert_eq!(flagged.target_key, Some(target_key()));

    let flags_only =
        resolve_context(Some(CHANNEL), Some(&target_key()), None, Some("garbage")).expect("flags");
    assert_eq!(
        flags_only.provider, None,
        "an unreadable grant names no provider"
    );

    match resolve_context(None, None, None, None) {
        Err(CliError::Usage(message)) => assert_eq!(message, MISSING_CONTEXT),
        other => panic!("missing: {other:?}"),
    }
    match resolve_context(None, None, None, Some("garbage")) {
        Err(CliError::Usage(message)) => {
            assert!(
                message.contains("--channel") && message.contains("--target"),
                "{message}"
            )
        }
        other => panic!("bad grant: {other:?}"),
    }
    assert!(matches!(
        resolve_context(Some("nope"), None, None, None),
        Err(CliError::Usage(_))
    ));
    assert!(matches!(
        resolve_context(Some(CHANNEL), Some("coding-session/v1|bad"), None, None),
        Err(CliError::Usage(_))
    ));
    let no_target = resolve_context(Some(CHANNEL), None, None, None).expect("channel only");
    assert!(matches!(no_target.target(), Err(CliError::Usage(_))));
}

#[test]
fn command_ids_are_random_or_the_callers() {
    let first = command_id(None).expect("fresh");
    let second = command_id(None).expect("fresh");
    assert_ne!(first, second);
    assert_eq!(first.len(), 20);
    assert!(first.starts_with("bee-") && first[4..].bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(command_id(Some("retry-7")).expect("given"), "retry-7");
    assert!(matches!(
        command_id(Some("has space")),
        Err(CliError::Usage(_))
    ));
    assert!(parse_slot_flag(SLOT).is_ok());
    assert!(parse_slot_flag("../../etc").is_err());
}

#[test]
fn the_session_dir_is_found_from_env_or_the_shim_on_path() {
    let env = OsString::from("/state/device/sess-a");
    assert_eq!(
        discover_session_dir(Some(&env), None).expect("env"),
        PathBuf::from("/state/device/sess-a")
    );
    let path = OsString::from("/usr/bin:/x/csp/device/sess-1/bin:/bin");
    assert_eq!(
        discover_session_dir(Some(&OsString::new()), Some(&path)).expect("path"),
        PathBuf::from("/x/csp/device/sess-1")
    );
    let nothing = OsString::from("/usr/bin:/x/device/bin:/x/sess/bin");
    let why = discover_session_dir(None, Some(&nothing)).expect_err("none");
    assert!(why.contains("BEEKEEPER_DEVICE_DIR"), "{why}");
}

#[test]
fn the_slot_file_is_read_for_the_png_and_the_quick_start() {
    let dir = tempfile::tempdir().expect("tempdir");
    let shots = dir.path().join("shots").join(SLOT);
    let body = serde_json::json!({
        "version": 1, "slot": SLOT, "udid": "ABCD", "platform": "ios", "model": "iPhone 17",
        "osVersion": "iOS 27.0", "sessionName": SLOT, "shotDir": shots,
    });
    std::fs::write(dir.path().join(format!("{SLOT}.json")), body.to_string()).expect("write");
    let local = read_local_slot(dir.path(), SLOT).expect("slot");
    assert_eq!(local.model, "iPhone 17");
    let sha = "c".repeat(64);
    assert_eq!(
        shot_png_path(&local, &sha),
        Some(shots.join(format!("{sha}.png")))
    );
    assert_eq!(shot_png_path(&local, "../x"), None);
    assert!(read_local_slot(dir.path(), "0123456789abcdef").is_err());
    assert!(read_local_slot(dir.path(), "../secret").is_err());
    assert_eq!(
        quick_start(SLOT, "iPhone 17", "iOS 27.0"),
        beekeeper_session_provider::device::agent_device::quick_start(
            SLOT,
            "iPhone 17",
            "iOS 27.0"
        ),
        "the CLI prints the provider's own quick-start text"
    );
}

#[test]
fn only_the_providers_terminal_record_for_this_command_ends_the_wait() {
    let provider = Keys::generate();
    let stranger = Keys::generate();
    let context = ctx(Some(&provider));
    let open = record(state(DeviceState::Open), Some(SLOT), Some("cmd-1"));

    let good = signed_record(&open, &provider, 100);
    let matched = match_terminal(&good, &context, "cmd-1").expect("terminal");
    let out = terminal_result(&matched).expect("ok");
    assert_eq!(out["state"], "open");
    assert_eq!(out["model"], "iPhone 17");
    assert_eq!(out["osVersion"], "27.0");
    assert_eq!(out["slot"], SLOT);

    assert!(
        match_terminal(&signed_record(&open, &stranger, 100), &context, "cmd-1").is_none(),
        "a record signed by anyone but the provider is ignored"
    );
    assert!(
        match_terminal(&good, &context, "cmd-2").is_none(),
        "another command's record"
    );
    let booting = record(state(DeviceState::Booting), Some(SLOT), Some("cmd-1"));
    assert!(
        match_terminal(&signed_record(&booting, &provider, 101), &context, "cmd-1").is_none(),
        "booting is not terminal"
    );
    let mut elsewhere = open.clone();
    elsewhere.target_key = coding_session_target_key(&CodingSessionTarget {
        generation: 3,
        ..target()
    });
    assert!(match_terminal(
        &signed_record(&elsewhere, &provider, 100),
        &context,
        "cmd-1"
    )
    .is_none());

    let shot = record(DeviceRecordPayload::Shot, Some(SLOT), Some("cmd-1"));
    let shot =
        match_terminal(&signed_record(&shot, &provider, 102), &context, "cmd-1").expect("shot");
    assert_eq!(
        terminal_result(&shot).expect("ok")["snapshotId"],
        "ab".repeat(32)
    );

    let refused = record(
        DeviceRecordPayload::Refused {
            code: DeviceRefusalCode::NoDeviceOpen,
            reason: "No device is open".into(),
        },
        None,
        Some("cmd-1"),
    );
    let refused = match_terminal(&signed_record(&refused, &provider, 103), &context, "cmd-1")
        .expect("refused");
    let error = terminal_result(&refused).expect_err("refused");
    assert_eq!(crate::error::exit_code(&error), 4);
    assert!(
        error
            .to_string()
            .contains("no_device_open: No device is open"),
        "{error}"
    );

    let failed = record(state(DeviceState::Failed), Some(SLOT), Some("cmd-1"));
    let failed =
        match_terminal(&signed_record(&failed, &provider, 104), &context, "cmd-1").expect("failed");
    let error = terminal_result(&failed).expect_err("failed");
    assert_eq!(crate::error::exit_code(&error), 4);
    assert!(error.to_string().contains("Xcode not found"));
}

#[test]
fn the_default_slot_is_the_newest_still_open_one() {
    let provider = Keys::generate();
    let context = ctx(Some(&provider));
    let other_slot = "0123456789abcdef";
    let events = vec![
        signed_record(
            &record(state(DeviceState::Open), Some(SLOT), None),
            &provider,
            100,
        ),
        signed_record(
            &record(state(DeviceState::Open), Some(other_slot), None),
            &provider,
            110,
        ),
        signed_record(
            &record(state(DeviceState::Closed), Some(other_slot), None),
            &provider,
            120,
        ),
    ];
    assert_eq!(default_open_slot(&events, &context).expect("slot"), SLOT);
    assert!(matches!(
        default_open_slot(&events[2..], &context),
        Err(CliError::Usage(_))
    ));
}

#[test]
fn no_availability_says_the_provider_does_not_offer_devices() {
    let provider = Keys::generate();
    let context = ctx(Some(&provider));
    let out = list_output(&[], &context);
    assert_eq!(out["availability"], Value::Null);
    assert_eq!(out["note"], NO_DEVICES_OFFERED_NOTE);
    assert_eq!(
        NO_DEVICES_OFFERED_NOTE,
        "this machine's provider does not offer devices"
    );

    let availability = record(
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
        },
        None,
        None,
    );
    let events = vec![
        signed_record(&availability, &provider, 100),
        signed_record(
            &record(state(DeviceState::Open), Some(SLOT), None),
            &provider,
            110,
        ),
    ];
    let out = list_output(&events, &context);
    assert_eq!(out["availability"]["platforms"]["ios"]["available"], true);
    assert!(out.get("note").is_none());
    assert_eq!(out["slots"][0]["slot"], SLOT);
    assert_eq!(out["slots"][0]["state"], "open");

    let stranger_only = vec![signed_record(&availability, &Keys::generate(), 100)];
    assert_eq!(
        list_output(&stranger_only, &context)["note"],
        NO_DEVICES_OFFERED_NOTE
    );
}

#[test]
fn a_frame_line_names_its_author_and_the_decoded_jpeg() {
    let producer = Keys::generate();
    let jpeg: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4, 5];
    let header = SurfaceFrameHeader {
        channel_id: channel(),
        surface: Surface::Device,
        key: SLOT.into(),
        frame_type: SurfaceFrameType::Frame,
        seq: 41,
        epoch: 1_791_374_400_123,
        cadence_ms: 3000,
        width: 900,
        height: 600,
        captured_at_ms: 1_791_374_512_345,
        actor: None,
        commit: None,
    };
    let event = sign(
        beekeeper_sdk::surface::build_surface_frame(&header, &encode_frame_content(&jpeg))
            .expect("frame"),
        &producer,
        Timestamp::now().as_secs(),
    );
    let line = frame_line(&event, &header, &producer.public_key());
    assert_eq!(line["type"], "frame");
    assert_eq!(line["author"], producer.public_key().to_hex());
    assert_eq!(line["authorityMatch"], true);
    assert_eq!(line["t"], "frame");
    assert_eq!(
        (line["seq"].as_u64(), line["epoch"].as_u64()),
        (Some(41), Some(1_791_374_400_123))
    );
    assert_eq!(line["capturedAt"], 1_791_374_512_345u64);
    assert_eq!(line["cadenceMs"], 3000);
    assert_eq!(line["dim"], "900x600");
    assert_eq!(line["bytes"], jpeg.len());
    assert_eq!(
        line["sha256"],
        hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&jpeg))
    );
    let other = Keys::generate().public_key();
    assert_eq!(frame_line(&event, &header, &other)["authorityMatch"], false);

    let summary = summary_line(3, 1, &producer.public_key(), true);
    assert_eq!(summary["note"], SAME_MACHINE_NOTE);
    assert_eq!(
        SAME_MACHINE_NOTE,
        "same machine, two identities — not cross-machine proof"
    );
    assert_eq!(beat_action(0), SurfaceWatchActionAlias::Resync);
    assert_eq!(beat_action(2), SurfaceWatchActionAlias::Watch);
}

use beekeeper_core::surface_watch::SurfaceWatchAction as SurfaceWatchActionAlias;

fn announce(status: PreviewStatus, target: Option<String>) -> SessionPreviewAnnounce {
    SessionPreviewAnnounce {
        channel_id: channel(),
        session_ref: SESSION_REF.into(),
        status,
        target_key: target,
        provider: None,
        // A close carries no page or title (NIP-SP).
        page: (status == PreviewStatus::Open).then(|| "local:/settings".into()),
        title: (status == PreviewStatus::Open).then(|| "Settings".into()),
        viewport_width: 1280,
        viewport_height: 800,
        stream: PreviewStream::Frames,
    }
}

#[test]
fn authorities_fold_like_the_relay() {
    let first = Keys::generate();
    let second = Keys::generate();
    let build = |a: &SessionPreviewAnnounce| {
        beekeeper_sdk::surface::build_session_preview_announce(a).expect("announce")
    };
    let open = announce(PreviewStatus::Open, Some(target_key()));
    let events = vec![
        sign(build(&open), &second, 200),
        sign(build(&open), &first, 100),
    ];
    assert_eq!(
        preview_authority(&events, channel(), SESSION_REF),
        Some(first.public_key()),
        "the earliest open announce owns the preview"
    );
    let mut closed = events.clone();
    closed.push(sign(
        build(&announce(PreviewStatus::Closed, None)),
        &first,
        300,
    ));
    assert_eq!(
        preview_authority(&closed, channel(), SESSION_REF),
        Some(second.public_key())
    );
    assert_eq!(preview_authority(&[], channel(), SESSION_REF), None);

    let provider = Keys::generate();
    let newer = Keys::generate();
    let records = vec![
        signed_record(
            &record(state(DeviceState::Open), Some(SLOT), None),
            &provider,
            100,
        ),
        signed_record(
            &record(state(DeviceState::Open), Some(SLOT), None),
            &newer,
            200,
        ),
    ];
    assert_eq!(
        device_authority(&records, channel(), SLOT),
        Some(newer.public_key())
    );
    assert_eq!(
        device_authority(&records, channel(), "0123456789abcdef"),
        None
    );
}
