use super::*;
use nostr::Keys;

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const SLOT: &str = "9f2c4e1a7b3d5c80";
const UDID: &str = "70E4C638-1A2B-4C3D-9E8F-0123456789AB";

fn parts(rows: &[Vec<String>]) -> Vec<&[String]> {
    rows.iter().map(Vec::as_slice).collect()
}

fn rows(raw: &[&[&str]]) -> Vec<Vec<String>> {
    raw.iter()
        .map(|tag| tag.iter().map(|value| (*value).to_owned()).collect())
        .collect()
}

fn watch(surface: Surface, key: &str) -> SurfaceWatch {
    SurfaceWatch {
        channel_id: Uuid::parse_str(CHANNEL).expect("uuid"),
        surface,
        key: key.into(),
        producer: Keys::generate().public_key(),
        action: SurfaceWatchAction::Watch,
    }
}

fn frame(surface: Surface, key: &str) -> SurfaceFrameHeader {
    SurfaceFrameHeader {
        channel_id: Uuid::parse_str(CHANNEL).expect("uuid"),
        surface,
        key: key.into(),
        frame_type: SurfaceFrameType::Frame,
        seq: 41,
        epoch: 1_791_374_400_123,
        cadence_ms: 2000,
        width: 1280,
        height: 800,
        captured_at_ms: 1_791_374_512_345,
        actor: Some(Keys::generate().public_key()),
        commit: Some("ab".repeat(20)),
    }
}

fn jpeg() -> String {
    encode_frame_content(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1])
}

#[test]
fn watch_round_trips_for_both_surfaces_and_every_action() {
    for (surface, key) in [(Surface::Preview, SESSION_REF), (Surface::Device, SLOT)] {
        for action in [
            SurfaceWatchAction::Watch,
            SurfaceWatchAction::Stop,
            SurfaceWatchAction::Resync,
            SurfaceWatchAction::Snapshot,
        ] {
            let mut expected = watch(surface, key);
            expected.action = action;
            let tags = expected.tags();
            let parsed =
                validate_surface_watch_parts(&parts(&tags), &expected.content()).expect("valid");
            assert_eq!(parsed, expected);
        }
    }
}

#[test]
fn watch_refuses_every_other_shape() {
    let good = watch(Surface::Device, SLOT);
    let producer = good.producer.to_hex();
    let content = good.content();
    let refused: Vec<(Vec<Vec<String>>, String)> = vec![
        // Unknown surface.
        (
            rows(&[
                &["h", CHANNEL],
                &["surface", "terminal"],
                &["d", SLOT],
                &["p", &producer],
            ]),
            content.clone(),
        ),
        // A UDID where the opaque slot belongs.
        (
            rows(&[
                &["h", CHANNEL],
                &["surface", "device"],
                &["d", UDID],
                &["p", &producer],
            ]),
            content.clone(),
        ),
        // A preview keyed by something that is not a sessionRef.
        (
            rows(&[
                &["h", CHANNEL],
                &["surface", "preview"],
                &["d", SLOT],
                &["p", &producer],
            ]),
            content.clone(),
        ),
        // Out of order.
        (
            rows(&[
                &["surface", "device"],
                &["h", CHANNEL],
                &["d", SLOT],
                &["p", &producer],
            ]),
            content.clone(),
        ),
        // Extra tag.
        (
            rows(&[
                &["h", CHANNEL],
                &["surface", "device"],
                &["d", SLOT],
                &["p", &producer],
                &["a", "30621:x:y"],
            ]),
            content.clone(),
        ),
        // Missing p.
        (
            rows(&[&["h", CHANNEL], &["surface", "device"], &["d", SLOT]]),
            content.clone(),
        ),
        // Uppercase channel.
        (
            rows(&[
                &["h", &CHANNEL.to_uppercase()],
                &["surface", "device"],
                &["d", SLOT],
                &["p", &producer],
            ]),
            content.clone(),
        ),
        // Unknown action, unknown key, prose, oversize.
        (good.tags(), "{\"action\":\"drive\"}".into()),
        (good.tags(), "{\"action\":\"watch\",\"udid\":\"x\"}".into()),
        (good.tags(), "watch".into()),
        (
            good.tags(),
            format!(
                "{{\"action\":\"watch\"}}{}",
                " ".repeat(MAX_SURFACE_WATCH_CONTENT_BYTES)
            ),
        ),
    ];
    for (tags, content) in refused {
        assert!(
            validate_surface_watch_parts(&parts(&tags), &content).is_err(),
            "accepted {tags:?} {content:?}"
        );
    }
}

#[test]
fn frame_round_trips_with_and_without_optional_tags() {
    let full = frame(Surface::Preview, SESSION_REF);
    let parsed = validate_surface_frame_parts(&parts(&full.tags()), &jpeg()).expect("valid");
    assert_eq!(parsed, full);

    let mut bare = frame(Surface::Device, SLOT);
    bare.actor = None;
    bare.commit = None;
    let parsed = validate_surface_frame_parts(&parts(&bare.tags()), &jpeg()).expect("valid");
    assert_eq!(parsed, bare);

    for frame_type in [SurfaceFrameType::Paused, SurfaceFrameType::End] {
        let mut header = frame(Surface::Device, SLOT);
        header.frame_type = frame_type;
        assert!(validate_surface_frame_parts(&parts(&header.tags()), "").is_ok());
        assert!(validate_surface_frame_parts(&parts(&header.tags()), &jpeg()).is_err());
    }
}

#[test]
fn frame_refuses_oversize_non_jpeg_and_bad_numbers() {
    let header = frame(Surface::Device, SLOT);
    let tags = header.tags();
    let oversize = format!(
        "/9j/{}",
        "A".repeat(MAX_SURFACE_FRAME_CONTENT_BYTES - 4 + 4)
    );
    assert!(validate_surface_frame_parts(&parts(&tags), &oversize).is_err());
    let at_cap = format!("/9j/{}", "A".repeat(MAX_SURFACE_FRAME_CONTENT_BYTES - 4));
    assert!(validate_surface_frame_parts(&parts(&tags), &at_cap).is_ok());
    // PNG bytes, not a JPEG.
    let png = encode_frame_content(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    assert!(validate_surface_frame_parts(&parts(&tags), &png).is_err());
    assert!(validate_surface_frame_parts(&parts(&tags), "").is_err());
    assert!(validate_surface_frame_parts(&parts(&tags), "/9j/ not base64").is_err());

    let mutate = |index: usize, value: &str| {
        let mut rows = header.tags();
        rows[index][1] = value.into();
        rows
    };
    for (index, value) in [
        (3, "tail"),
        (4, "041"),
        (4, "-1"),
        (5, "9007199254740992"),
        (6, "100"),
        (6, "60001"),
        (7, "1280X800"),
        (7, "0x800"),
        (7, "9000x800"),
        (8, "now"),
        (9, "not-a-key"),
        (10, "abc"),
    ] {
        let rows = mutate(index, value);
        assert!(
            validate_surface_frame_parts(&parts(&rows), &jpeg()).is_err(),
            "accepted tag {index}={value}"
        );
    }
}

#[test]
fn host_local_values_are_refused_and_redactable() {
    for value in [
        UDID,
        &UDID.to_lowercase(),
        "booted 70E4C638-1A2B-4C3D-9E8F-0123456789AB now",
        "/Users/brian/Library/Developer/CoreSimulator/Devices",
        "/home/ci/build",
        "/var/folders/xy/T/shot.png",
        "/private/var/folders/x",
        "~/Library/Developer/Xcode/DerivedData/App",
        "file:///tmp/x.html",
    ] {
        assert!(refuse_host_local("v", value).is_err(), "accepted {value}");
        assert!(
            refuse_host_local("v", &redact_free_text(value)).is_ok(),
            "redaction left a host-local fact in {value:?} -> {:?}",
            redact_free_text(value)
        );
    }
    for value in ["iPhone 17", "Settings", SLOT, &"ab".repeat(32), "27.0"] {
        assert!(refuse_host_local("v", value).is_ok(), "refused {value}");
        assert_eq!(redact_free_text(value), value);
    }
    assert_eq!(
        redact_free_text("saved to /Users/brian/shots/a.png ok"),
        "saved to ~/…/a.png ok"
    );
}

#[test]
fn freshness_window_is_five_minutes_either_way() {
    let now = 1_791_374_400;
    assert!(check_surface_freshness(now, now).is_ok());
    assert!(check_surface_freshness(now - 300, now).is_ok());
    assert!(check_surface_freshness(now + 300, now).is_ok());
    assert!(check_surface_freshness(now - 301, now).is_err());
    assert!(check_surface_freshness(now + 301, now).is_err());
}

#[test]
fn envelopes_check_the_kind() {
    let header = watch(Surface::Device, SLOT);
    let event = nostr::EventBuilder::new(nostr::Kind::Custom(24310), header.content())
        .tags(
            header
                .tags()
                .into_iter()
                .map(|row| nostr::Tag::parse(row).expect("tag")),
        )
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    assert!(validate_surface_watch_envelope(&event).is_err());
    assert!(validate_surface_frame_envelope(&event).is_err());
}
