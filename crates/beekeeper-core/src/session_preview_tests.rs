use super::*;
use crate::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn target_key() -> String {
    coding_session_target_key(&CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        // Runtime session ids are often UUIDs; cs-target may carry one.
        session_id: "0f8e2d6c-1b3a-4c5d-8e7f-9a0b1c2d3e4f".into(),
        generation: 2,
    })
}

fn announce(status: PreviewStatus) -> SessionPreviewAnnounce {
    SessionPreviewAnnounce {
        channel_id: Uuid::parse_str(CHANNEL).expect("uuid"),
        session_ref: SESSION_REF.into(),
        status,
        target_key: Some(target_key()),
        provider: Some(Keys::generate().public_key()),
        // A close carries no page or title (NIP-SP).
        page: (status == PreviewStatus::Open).then(|| "local:/settings".into()),
        title: (status == PreviewStatus::Open).then(|| "Settings".into()),
        viewport_width: 1280,
        viewport_height: 800,
        stream: PreviewStream::Frames,
    }
}

fn parts(rows: &[Vec<String>]) -> Vec<&[String]> {
    rows.iter().map(Vec::as_slice).collect()
}

fn sign(announce: &SessionPreviewAnnounce, keys: &Keys, at: u64) -> Event {
    EventBuilder::new(Kind::Custom(KIND_SESSION_PREVIEW_ANNOUNCE as u16), "")
        .tags(
            announce
                .tags()
                .into_iter()
                .map(|row| Tag::parse(row).expect("tag")),
        )
        .custom_created_at(Timestamp::from(at))
        .sign_with_keys(keys)
        .expect("sign")
}

#[test]
fn announce_round_trips_with_and_without_optional_tags() {
    let full = announce(PreviewStatus::Open);
    assert_eq!(
        validate_session_preview_announce_parts(&parts(&full.tags()), "").expect("valid"),
        full
    );
    let mut person = announce(PreviewStatus::Closed);
    person.target_key = None;
    person.provider = None;
    person.page = None;
    person.title = None;
    person.stream = PreviewStream::Snapshots;
    assert_eq!(
        validate_session_preview_announce_parts(&parts(&person.tags()), "").expect("valid"),
        person
    );
}

#[test]
fn announce_refuses_bad_status_content_and_host_local_values() {
    let good = announce(PreviewStatus::Open);
    assert!(validate_session_preview_announce_parts(&parts(&good.tags()), "x").is_err());
    let index_of = |name: &str| {
        good.tags()
            .iter()
            .position(|row| row[0] == name)
            .expect("tag present")
    };
    for (name, value) in [
        ("status", "paused"),
        ("spa-v", "2"),
        ("d", "not-a-ref"),
        ("cs-target", "coding-session/v1|bad"),
        ("provider", "localhost"),
        ("page", "http://localhost:5173/settings"),
        ("page", "local:/settings?token=abc"),
        ("page", "local:/Users/brian/site/index.html"),
        ("title", "Order 70E4C638-1A2B-4C3D-9E8F-0123456789AB"),
        ("title", &"t".repeat(MAX_PREVIEW_TITLE_BYTES + 1)),
        ("viewport", "wide"),
        ("stream", "video"),
        ("input", "trusted"),
    ] {
        let mut rows = good.tags();
        rows[index_of(name)][1] = value.into();
        assert!(
            validate_session_preview_announce_parts(&parts(&rows), "").is_err(),
            "accepted {name}={value}"
        );
    }
}

#[test]
fn page_grammar_and_redaction() {
    for ok in [
        "local:/",
        "local:/settings",
        "https://example.com",
        "https://example.com/docs/a",
    ] {
        assert!(validate_page(ok).is_ok(), "refused {ok}");
    }
    for bad in [
        "",
        "local:settings",
        "local://evil.com/x",
        "https://example.com:8443/x",
        "https://user@example.com/x",
        "https://127.0.0.1/x",
        "https://192.168.1.4/x",
        "https://my-mac.local/x",
        "https://example.com/x#frag",
        "ftp://example.com/x",
        "local:/a b",
    ] {
        assert!(validate_page(bad).is_err(), "accepted {bad}");
    }
    let cases = [
        (
            "http://localhost:5173/settings?tab=2#x",
            Some("local:/settings"),
        ),
        ("http://127.0.0.1:3000/", Some("local:/")),
        ("http://[::1]:8080/a/b", Some("local:/a/b")),
        ("http://10.0.0.5/admin", Some("local:/admin")),
        (
            "https://user:pw@Example.com:8443/docs?q=1",
            Some("https://example.com/docs"),
        ),
        (
            "file:///Users/brian/site/index.html",
            Some("local:/index.html"),
        ),
        ("chrome://settings", None),
        ("not a url", None),
    ];
    for (raw, expected) in cases {
        assert_eq!(redact_page_url(raw).as_deref(), expected, "for {raw}");
    }
}

#[test]
fn earliest_open_announce_owns_the_preview() {
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");
    let first = Keys::generate();
    let second = Keys::generate();
    let open = announce(PreviewStatus::Open);
    let closed = announce(PreviewStatus::Closed);

    // Nobody: no announces, or only a close.
    assert_eq!(resolve_preview_owner(&[], channel, SESSION_REF), None);
    let only_closed = [sign(&closed, &first, 100)];
    assert_eq!(
        resolve_preview_owner(&only_closed, channel, SESSION_REF),
        None
    );

    // The earlier open wins over a later one.
    let both = [sign(&open, &second, 200), sign(&open, &first, 100)];
    assert_eq!(
        resolve_preview_owner(&both, channel, SESSION_REF),
        Some(first.public_key())
    );

    // A later close from the earlier owner hands it to the other signer.
    let handed = [
        sign(&open, &first, 100),
        sign(&open, &second, 200),
        sign(&closed, &first, 300),
    ];
    assert_eq!(
        resolve_preview_owner(&handed, channel, SESSION_REF),
        Some(second.public_key())
    );

    // Another session's announce is ignored.
    let mut other = announce(PreviewStatus::Open);
    other.session_ref = "11111111-2222-4333-8444-555555555555".into();
    let unrelated = [sign(&other, &first, 50), sign(&open, &second, 200)];
    assert_eq!(
        resolve_preview_owner(&unrelated, channel, SESSION_REF),
        Some(second.public_key())
    );
}

#[test]
fn a_closed_announce_carries_no_page_or_title_and_an_open_one_must() {
    let open_rows = announce(PreviewStatus::Open).tags();
    let without = |rows: &[Vec<String>], names: &[&str]| -> Vec<Vec<String>> {
        rows.iter()
            .filter(|row| !names.contains(&row[0].as_str()))
            .cloned()
            .collect()
    };
    let with_status = |rows: &[Vec<String>], status: &str| -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| {
                if row[0] == "status" {
                    vec!["status".into(), status.into()]
                } else {
                    row.clone()
                }
            })
            .collect()
    };
    let closed_rows = with_status(&open_rows, "closed");

    // A close says only that the preview closed: no page, no title.
    let bare = without(&closed_rows, &["page", "title"]);
    assert!(
        validate_session_preview_announce_parts(&parts(&bare), "").is_ok(),
        "a closed announce without page/title must validate"
    );
    for leak in [
        closed_rows.clone(),
        without(&closed_rows, &["page"]),
        without(&closed_rows, &["title"]),
    ] {
        assert!(
            validate_session_preview_announce_parts(&parts(&leak), "").is_err(),
            "a closed announce carrying page or title was accepted"
        );
    }
    // An open announce still needs both.
    for missing in [
        without(&open_rows, &["page"]),
        without(&open_rows, &["title"]),
    ] {
        assert!(validate_session_preview_announce_parts(&parts(&missing), "").is_err());
    }
}
