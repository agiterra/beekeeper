//! End-to-end tests for NIP-ST shared terminals (kind:30623 session announce,
//! kind:24310 watch, kind:24311 frame) against a running relay.
//!
//! The buzz-core unit tests pin the predicates and the ingest/handler tests
//! pin the envelope rules; these tests prove the project gate is wired into
//! the live surfaces:
//! - a session announce inside a *private* project is readable by the owner
//!   and invited members but withheld from strangers (WS REQ);
//! - frames fan out live to members but never to strangers;
//! - a stranger can neither publish frames nor watch events into a private
//!   project's coordinate;
//! - malformed frames/watches are rejected outright;
//! - public-project announces and frames stay community-readable.
//!
//! See `docs/nips/NIP-ST.md` for the normative contract.
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p beekeeper-test-client --test e2e_shell_observe -- --ignored
//! ```

use std::time::Duration;

use beekeeper_test_client::BuzzTestClient;
use beekeeper_ws_client::RelayMessage;
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag};

const PROJECT_KIND: u16 = 30621;
const SHELL_SESSION_KIND: u16 = 30623;
const SHELL_WATCH_KIND: u16 = 24310;
const SHELL_FRAME_KIND: u16 = 24311;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_string())
}

fn sub_id(name: &str) -> String {
    format!("e2e-shell-observe-{name}-{}", uuid::Uuid::new_v4())
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn project_coordinate(owner: &Keys, d_tag: &str) -> String {
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

fn project_event(
    keys: &Keys,
    d_tag: &str,
    access: Option<&str>,
    members: &[&Keys],
) -> nostr::Event {
    let mut tags = vec![
        Tag::parse(["d", d_tag]).unwrap(),
        Tag::parse(["name", d_tag]).unwrap(),
    ];
    if let Some(access) = access {
        tags.push(Tag::parse(["buzz-access", access]).unwrap());
    }
    for member in members {
        tags.push(Tag::parse(["p", &member.public_key().to_hex()]).unwrap());
    }
    EventBuilder::new(Kind::Custom(PROJECT_KIND), "")
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap()
}

fn announce_event(keys: &Keys, session_id: &str, coordinate: &str, status: &str) -> nostr::Event {
    EventBuilder::new(Kind::Custom(SHELL_SESSION_KIND), "")
        .tags(vec![
            Tag::parse(["d", session_id]).unwrap(),
            Tag::parse(["a", coordinate]).unwrap(),
            Tag::parse(["title", "e2e shell"]).unwrap(),
            Tag::parse(["status", status]).unwrap(),
            Tag::parse(["dims", "24x80"]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap()
}

fn frame_event(keys: &Keys, session_id: &str, coordinate: &str, seq: u64) -> nostr::Event {
    EventBuilder::new(Kind::Custom(SHELL_FRAME_KIND), "aGVsbG8=")
        .tags(vec![
            Tag::parse(["d", session_id]).unwrap(),
            Tag::parse(["a", coordinate]).unwrap(),
            Tag::parse(["t", "snap"]).unwrap(),
            Tag::parse(["seq", &seq.to_string()]).unwrap(),
            Tag::parse(["epoch", "e2e-epoch"]).unwrap(),
            Tag::parse(["dims", "24x80"]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap()
}

fn watch_event(keys: &Keys, owner: &Keys, session_id: &str, coordinate: &str) -> nostr::Event {
    EventBuilder::new(Kind::Custom(SHELL_WATCH_KIND), r#"{"action":"watch"}"#)
        .tags(vec![
            Tag::parse(["p", &owner.public_key().to_hex()]).unwrap(),
            Tag::parse(["d", session_id]).unwrap(),
            Tag::parse(["a", coordinate]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap()
}

async fn query(client: &mut BuzzTestClient, name: &str, filter: Filter) -> Vec<nostr::Event> {
    let sid = sub_id(name);
    client
        .subscribe(&sid, vec![filter])
        .await
        .expect("subscribe");
    let events = client
        .collect_until_eose(&sid, Duration::from_secs(10))
        .await
        .expect("collect events");
    client.close_subscription(&sid).await.ok();
    events
}

fn announce_filter(session_id: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(SHELL_SESSION_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [session_id])
}

/// Wait for one specific event id on a live subscription; `true` if it landed.
async fn recv_specific(
    client: &mut BuzzTestClient,
    sid: &str,
    id: nostr::EventId,
    window: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + window;
    while tokio::time::Instant::now() < deadline {
        match client.recv_event(Duration::from_secs(1)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == sid && event.id == id => return true,
            Ok(_) => {}
            Err(_) => {}
        }
    }
    false
}

/// Private project: the announce is readable by owner + member, withheld from
/// a stranger; frames fan out to the member only; the stranger cannot publish
/// frames or watches into the coordinate.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_private_project_terminal_gated_end_to_end() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("shellproj");
    let session_id = unique("sess");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let ok = owner_client
        .send_event(project_event(&owner, &d_tag, Some("private"), &[&member]))
        .await
        .expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let ok = owner_client
        .send_event(announce_event(&owner, &session_id, &coordinate, "open"))
        .await
        .expect("send announce");
    assert!(ok.accepted, "announce rejected: {}", ok.message);

    // REQ: owner and member see the announce; the stranger gets nothing.
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");

    let owner_sees = query(&mut owner_client, "own", announce_filter(&session_id)).await;
    assert_eq!(owner_sees.len(), 1, "owner must read their own announce");
    let member_sees = query(&mut member_client, "member", announce_filter(&session_id)).await;
    assert_eq!(
        member_sees.len(),
        1,
        "invited member must read the announce"
    );
    let stranger_sees = query(
        &mut stranger_client,
        "stranger",
        announce_filter(&session_id),
    )
    .await;
    assert!(
        stranger_sees.is_empty(),
        "stranger must not read a private project's announce"
    );

    // Live fan-out: member receives frames, stranger does not.
    let member_sid = sub_id("frames-member");
    let stranger_sid = sub_id("frames-stranger");
    let frames_filter = Filter::new().kind(Kind::Custom(SHELL_FRAME_KIND));
    member_client
        .subscribe(&member_sid, vec![frames_filter.clone()])
        .await
        .expect("member subscribe");
    member_client
        .collect_until_eose(&member_sid, Duration::from_secs(10))
        .await
        .expect("member eose");
    stranger_client
        .subscribe(&stranger_sid, vec![frames_filter])
        .await
        .expect("stranger subscribe");
    stranger_client
        .collect_until_eose(&stranger_sid, Duration::from_secs(10))
        .await
        .expect("stranger eose");

    let frame = frame_event(&owner, &session_id, &coordinate, 1);
    let frame_id = frame.id;
    let ok = owner_client.send_event(frame).await.expect("send frame");
    assert!(ok.accepted, "owner frame rejected: {}", ok.message);

    assert!(
        recv_specific(
            &mut member_client,
            &member_sid,
            frame_id,
            Duration::from_secs(10)
        )
        .await,
        "invited member must receive the live frame"
    );
    assert!(
        !recv_specific(
            &mut stranger_client,
            &stranger_sid,
            frame_id,
            Duration::from_secs(5)
        )
        .await,
        "stranger must not receive a private project's frame"
    );

    // Publisher gate: the stranger cannot publish frames or watches into the
    // private coordinate; the member can watch.
    let ok = stranger_client
        .send_event(frame_event(&stranger, &session_id, &coordinate, 2))
        .await
        .expect("send stranger frame");
    assert!(!ok.accepted, "stranger frame must be rejected");
    assert!(
        ok.message.starts_with("restricted:"),
        "expected restricted, got: {}",
        ok.message
    );

    let ok = stranger_client
        .send_event(watch_event(&stranger, &owner, &session_id, &coordinate))
        .await
        .expect("send stranger watch");
    assert!(!ok.accepted, "stranger watch must be rejected");

    let ok = member_client
        .send_event(watch_event(&member, &owner, &session_id, &coordinate))
        .await
        .expect("send member watch");
    assert!(ok.accepted, "member watch rejected: {}", ok.message);
}

/// Public project: announce and frames are readable community-wide.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_public_project_terminal_open_to_members() {
    let owner = Keys::generate();
    let viewer = Keys::generate();
    let d_tag = unique("shellpub");
    let session_id = unique("sess");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let ok = owner_client
        .send_event(project_event(&owner, &d_tag, Some("public"), &[]))
        .await
        .expect("send project");
    assert!(ok.accepted);

    let ok = owner_client
        .send_event(announce_event(&owner, &session_id, &coordinate, "open"))
        .await
        .expect("send announce");
    assert!(ok.accepted, "announce rejected: {}", ok.message);

    let mut viewer_client = BuzzTestClient::connect(&relay_url(), &viewer)
        .await
        .expect("viewer connect");
    let seen = query(&mut viewer_client, "pub", announce_filter(&session_id)).await;
    assert_eq!(seen.len(), 1, "public-project announce must be readable");

    // Live frames reach any community member.
    let sid = sub_id("pub-frames");
    viewer_client
        .subscribe(
            &sid,
            vec![Filter::new().kind(Kind::Custom(SHELL_FRAME_KIND))],
        )
        .await
        .expect("subscribe");
    viewer_client
        .collect_until_eose(&sid, Duration::from_secs(10))
        .await
        .expect("eose");
    let frame = frame_event(&owner, &session_id, &coordinate, 1);
    let frame_id = frame.id;
    let ok = owner_client.send_event(frame).await.expect("send frame");
    assert!(ok.accepted);
    assert!(
        recv_specific(&mut viewer_client, &sid, frame_id, Duration::from_secs(10)).await,
        "public-project frame must fan out to members"
    );
}

/// Malformed shared-terminal events are rejected at ingest.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_malformed_shell_events_rejected() {
    let owner = Keys::generate();
    let d_tag = unique("shellval");
    let session_id = unique("sess");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("connect");
    let ok = client
        .send_event(project_event(&owner, &d_tag, Some("public"), &[]))
        .await
        .expect("send project");
    assert!(ok.accepted);

    // Frame without a project coordinate.
    let no_coord = EventBuilder::new(Kind::Custom(SHELL_FRAME_KIND), "")
        .tags(vec![
            Tag::parse(["d", &session_id]).unwrap(),
            Tag::parse(["t", "snap"]).unwrap(),
            Tag::parse(["seq", "1"]).unwrap(),
            Tag::parse(["epoch", "e"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    let ok = client.send_event(no_coord).await.expect("send");
    assert!(!ok.accepted, "frame without `a` must be rejected");

    // Frame with an unknown type.
    let bad_type = EventBuilder::new(Kind::Custom(SHELL_FRAME_KIND), "")
        .tags(vec![
            Tag::parse(["d", &session_id]).unwrap(),
            Tag::parse(["a", &coordinate]).unwrap(),
            Tag::parse(["t", "mystery"]).unwrap(),
            Tag::parse(["seq", "1"]).unwrap(),
            Tag::parse(["epoch", "e"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    let ok = client.send_event(bad_type).await.expect("send");
    assert!(!ok.accepted, "frame with unknown type must be rejected");

    // Announce with an unknown status (fail closed, never default).
    let bad_status = announce_event(&owner, &session_id, &coordinate, "sharing");
    let ok = client.send_event(bad_status).await.expect("send");
    assert!(
        !ok.accepted,
        "announce with unknown status must be rejected"
    );

    // Oversized watch content.
    let big = "x".repeat(2048);
    let big_watch = EventBuilder::new(Kind::Custom(SHELL_WATCH_KIND), big)
        .tags(vec![
            Tag::parse(["p", &owner.public_key().to_hex()]).unwrap(),
            Tag::parse(["d", &session_id]).unwrap(),
            Tag::parse(["a", &coordinate]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    let ok = client.send_event(big_watch).await.expect("send");
    assert!(!ok.accepted, "oversized watch must be rejected");
}
