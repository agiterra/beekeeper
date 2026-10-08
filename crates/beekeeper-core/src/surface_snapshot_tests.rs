use super::*;
use nostr::Keys;

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const SLOT: &str = "9f2c4e1a7b3d5c80";

fn parts(rows: &[Vec<String>]) -> Vec<&[String]> {
    rows.iter().map(Vec::as_slice).collect()
}

fn device_snapshot() -> SurfaceSnapshot {
    let sha = "cd".repeat(32);
    SurfaceSnapshot {
        channel_id: Uuid::parse_str(CHANNEL).expect("uuid"),
        snapshot_type: SurfaceSnapshotType::Snapshot,
        surface: Surface::Device,
        key: SLOT.into(),
        url: format!("https://hive.agiterra.org/{sha}.png"),
        sha256: sha,
        mime: "image/png".into(),
        width: 1179,
        height: 2556,
        taken_at_ms: 1_791_374_512_345,
        provider: Keys::generate().public_key(),
        requested_by: Some(Keys::generate().public_key()),
        commit: Some(SnapshotCommit {
            sha: "ab".repeat(20),
            dirty: true,
        }),
        reference: Some((
            EventId::from_hex(&"ef".repeat(32)).expect("id"),
            SnapshotReferenceMarker::Command,
        )),
        page: None,
        title: None,
        alt: "Home screen after login".into(),
    }
}

fn preview_snapshot() -> SurfaceSnapshot {
    SurfaceSnapshot {
        surface: Surface::Preview,
        key: SESSION_REF.into(),
        requested_by: None,
        commit: None,
        reference: None,
        page: Some("local:/settings".into()),
        title: Some("Settings".into()),
        ..device_snapshot()
    }
}

#[test]
fn snapshot_round_trips_for_both_surfaces() {
    for snapshot in [device_snapshot(), preview_snapshot()] {
        let parsed = validate_surface_snapshot_parts(&parts(&snapshot.tags()), &snapshot.alt)
            .expect("valid");
        assert_eq!(parsed, snapshot);
    }
    let mut annotation = preview_snapshot();
    annotation.snapshot_type = SurfaceSnapshotType::Annotation;
    annotation.reference = Some((
        EventId::from_hex(&"aa".repeat(32)).expect("id"),
        SnapshotReferenceMarker::Annotation,
    ));
    assert!(validate_surface_snapshot_parts(&parts(&annotation.tags()), "").is_ok());
    annotation.reference = None;
    assert!(validate_surface_snapshot_parts(&parts(&annotation.tags()), "").is_err());
}

#[test]
fn snapshot_refuses_surface_mismatches_and_host_local_facts() {
    let mut device_with_page = device_snapshot();
    device_with_page.page = Some("local:/x".into());
    assert!(validate_surface_snapshot_parts(&parts(&device_with_page.tags()), "").is_err());

    let mut preview_without_page = preview_snapshot();
    preview_without_page.page = None;
    assert!(validate_surface_snapshot_parts(&parts(&preview_without_page.tags()), "").is_err());

    let good = device_snapshot();
    assert!(validate_surface_snapshot_parts(
        &parts(&good.tags()),
        "shot of 70E4C638-1A2B-4C3D-9E8F-0123456789AB"
    )
    .is_err());
    assert!(
        validate_surface_snapshot_parts(&parts(&good.tags()), "/Users/brian/shot.png").is_err()
    );
    assert!(validate_surface_snapshot_parts(
        &parts(&good.tags()),
        &"a".repeat(MAX_SURFACE_SNAPSHOT_ALT_BYTES + 1)
    )
    .is_err());

    let index_of = |name: &str| {
        good.tags()
            .iter()
            .position(|row| row[0] == name)
            .expect("tag")
    };
    for (name, field, value) in [
        ("ssn-v", 1, "2"),
        ("ssn-type", 1, "photo"),
        ("surface", 1, "terminal"),
        ("d", 1, "70E4C638-1A2B-4C3D-9E8F-0123456789AB"),
        ("x", 1, "CD"),
        ("url", 1, "https://hive.agiterra.org/other.png"),
        ("url", 1, "file:///Users/brian/shot.png"),
        ("m", 1, "image/gif"),
        ("dim", 1, "big"),
        ("taken-at", 1, "yesterday"),
        ("provider", 1, "my-macbook.local"),
        ("p", 3, "author"),
        ("commit", 2, "maybe"),
        ("e", 3, "reply"),
    ] {
        let mut rows = good.tags();
        rows[index_of(name)][field] = value.into();
        assert!(
            validate_surface_snapshot_parts(&parts(&rows), "").is_err(),
            "accepted {name}[{field}]={value}"
        );
    }
    let mut extra = good.tags();
    extra.push(vec!["udid".into(), "x".into()]);
    assert!(validate_surface_snapshot_parts(&parts(&extra), "").is_err());
}

#[test]
fn evidence_tokens_accept_the_alias() {
    let id = EventId::from_hex(&"ef".repeat(32)).expect("id");
    let token = snapshot_evidence_token(&id);
    assert_eq!(token, format!("snapshot:{}", "ef".repeat(32)));
    assert_eq!(parse_snapshot_evidence_token(&token), Some(id));
    assert_eq!(
        parse_snapshot_evidence_token(&format!("preview:{}", "ef".repeat(32))),
        Some(id)
    );
    for bad in [
        "snapshot:",
        "snapshot:EF",
        "shot:abc",
        &format!("snapshot:{}", "EF".repeat(32)),
    ] {
        assert_eq!(parse_snapshot_evidence_token(bad), None, "accepted {bad}");
    }
}
