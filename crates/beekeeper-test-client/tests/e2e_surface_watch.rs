//! End-to-end tests for NIP-SW shared surfaces (kind 24320 watch, kind 24321
//! frame) with NIP-SP's kind 30626 preview announce as the authority, over two
//! (or three) WebSocket clients against a running relay.
//!
//! The core and ingest unit tests pin the envelopes and the host-local rule;
//! these prove the live wiring:
//! - a member's watch reaches its `p` (the producer) and nobody else in the
//!   channel;
//! - the announced producer's frame reaches the watcher;
//! - a frame from a member who is not the announced producer is refused, and
//!   so is any frame for a surface nobody announced (device slot with no
//!   44255 state);
//! - a non-member of a private session channel can neither watch nor frame;
//! - an oversize frame and a UDID-shaped `d` are refused.
//!
//! # Running
//!
//! Requires a relay backed by Postgres and Redis (CI runs it):
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p beekeeper-test-client \
//!   --test e2e_surface_watch -- --ignored --nocapture
//! ```

use std::time::Duration;

use beekeeper_core::session_preview::{PreviewStatus, PreviewStream, SessionPreviewAnnounce};
use beekeeper_core::surface_watch::{
    encode_frame_content, Surface, SurfaceFrameHeader, SurfaceFrameType, SurfaceWatch,
    SurfaceWatchAction, MAX_SURFACE_FRAME_CONTENT_BYTES,
};
use beekeeper_sdk::surface::{
    build_session_preview_announce, build_surface_frame, build_surface_watch,
};
use beekeeper_sdk::{build_add_member, build_create_channel, Visibility};
use beekeeper_test_client::{BeekeeperTestClient, RelayMessage};
use nostr::{Alphabet, Event, EventBuilder, EventId, Filter, Keys, Kind, SingleLetterTag, Tag};
use uuid::Uuid;

const WATCH_KIND: u16 = 24_320;
const FRAME_KIND: u16 = 24_321;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_owned())
}

fn sub_id(name: &str) -> String {
    format!("e2e-surface-{name}-{}", Uuid::new_v4())
}

async fn connect(keys: &Keys) -> BeekeeperTestClient {
    BeekeeperTestClient::connect(&relay_url(), keys)
        .await
        .expect("connect")
}

/// A private channel owned by `owner` with `members` added.
async fn private_channel(
    client: &mut BeekeeperTestClient,
    owner: &Keys,
    members: &[&Keys],
) -> Uuid {
    let channel_id = Uuid::new_v4();
    let create = build_create_channel(
        channel_id,
        &format!("surface-e2e-{channel_id}"),
        Some(Visibility::Private),
        None,
        None,
        None,
    )
    .expect("build create")
    .sign_with_keys(owner)
    .expect("sign create");
    let ok = client.send_event(create).await.expect("create channel");
    assert!(ok.accepted, "channel creation rejected: {}", ok.message);
    for member in members {
        let add = build_add_member(channel_id, &member.public_key().to_hex(), None)
            .expect("build add")
            .sign_with_keys(owner)
            .expect("sign add");
        let ok = client.send_event(add).await.expect("add member");
        assert!(ok.accepted, "add member rejected: {}", ok.message);
    }
    channel_id
}

fn channel_filter(kind: u16, channel_id: Uuid) -> Filter {
    Filter::new().kind(Kind::Custom(kind)).custom_tags(
        SingleLetterTag::lowercase(Alphabet::H),
        [channel_id.to_string()],
    )
}

fn watch(keys: &Keys, channel_id: Uuid, surface: Surface, key: &str, producer: &Keys) -> Event {
    build_surface_watch(&SurfaceWatch {
        channel_id,
        surface,
        key: key.to_owned(),
        producer: producer.public_key(),
        action: SurfaceWatchAction::Watch,
    })
    .expect("build watch")
    .sign_with_keys(keys)
    .expect("sign watch")
}

fn frame_header(channel_id: Uuid, surface: Surface, key: &str, seq: u64) -> SurfaceFrameHeader {
    SurfaceFrameHeader {
        channel_id,
        surface,
        key: key.to_owned(),
        frame_type: SurfaceFrameType::Frame,
        seq,
        epoch: 1,
        cadence_ms: 2000,
        width: 640,
        height: 400,
        captured_at_ms: 1_791_374_512_345,
        actor: None,
        commit: None,
    }
}

fn frame(keys: &Keys, channel_id: Uuid, surface: Surface, key: &str, seq: u64) -> Event {
    let jpeg = encode_frame_content(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F']);
    build_surface_frame(&frame_header(channel_id, surface, key, seq), &jpeg)
        .expect("build frame")
        .sign_with_keys(keys)
        .expect("sign frame")
}

fn announce(keys: &Keys, channel_id: Uuid, session_ref: &str) -> Event {
    build_session_preview_announce(&SessionPreviewAnnounce {
        channel_id,
        session_ref: session_ref.to_owned(),
        status: PreviewStatus::Open,
        target_key: None,
        provider: None,
        page: Some("local:/".into()),
        title: Some("e2e".into()),
        viewport_width: 640,
        viewport_height: 400,
        stream: PreviewStream::Frames,
    })
    .expect("build announce")
    .sign_with_keys(keys)
    .expect("sign announce")
}

/// Wait for one event id on a live subscription; `true` if it arrived.
async fn arrives(
    client: &mut BeekeeperTestClient,
    sid: &str,
    id: EventId,
    window: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + window;
    while tokio::time::Instant::now() < deadline {
        if let Ok(RelayMessage::Event {
            subscription_id,
            event,
        }) = client.recv_event(Duration::from_millis(500)).await
        {
            if subscription_id == sid && event.id == id {
                return true;
            }
        }
    }
    false
}

async fn live(client: &mut BeekeeperTestClient, name: &str, filter: Filter) -> String {
    let sid = sub_id(name);
    client
        .subscribe(&sid, vec![filter])
        .await
        .expect("subscribe");
    client
        .collect_until_eose(&sid, Duration::from_secs(5))
        .await
        .expect("eose");
    sid
}

/// Watch → frame between two identities on one relay, with a third member
/// who must not see the watch and may not publish frames, and a stranger who
/// may do neither.
#[tokio::test]
#[ignore = "requires running relay"]
async fn preview_watch_and_frame_path_is_gated_end_to_end() {
    let host = Keys::generate();
    let watcher = Keys::generate();
    let bystander = Keys::generate();
    let stranger = Keys::generate();
    let session_ref = Uuid::new_v4().to_string();

    let mut host_ws = connect(&host).await;
    let channel_id = private_channel(&mut host_ws, &host, &[&watcher, &bystander]).await;
    let mut watcher_ws = connect(&watcher).await;
    let mut bystander_ws = connect(&bystander).await;
    let mut stranger_ws = connect(&stranger).await;

    let ok = host_ws
        .send_event(announce(&host, channel_id, &session_ref))
        .await
        .expect("announce");
    assert!(ok.accepted, "announce rejected: {}", ok.message);

    let host_sid = live(
        &mut host_ws,
        "host-watch",
        channel_filter(WATCH_KIND, channel_id),
    )
    .await;
    let bystander_sid = live(
        &mut bystander_ws,
        "bystander-watch",
        channel_filter(WATCH_KIND, channel_id),
    )
    .await;
    let watcher_sid = live(
        &mut watcher_ws,
        "watcher-frames",
        channel_filter(FRAME_KIND, channel_id).author(host.public_key()),
    )
    .await;

    // The watch reaches the producer, and only the producer.
    let w = watch(&watcher, channel_id, Surface::Preview, &session_ref, &host);
    let ok = watcher_ws.send_event(w.clone()).await.expect("watch");
    assert!(ok.accepted, "watch rejected: {}", ok.message);
    assert!(arrives(&mut host_ws, &host_sid, w.id, Duration::from_secs(5)).await);
    assert!(
        !arrives(
            &mut bystander_ws,
            &bystander_sid,
            w.id,
            Duration::from_secs(2)
        )
        .await
    );

    // The announced producer's frame reaches the watcher.
    let f = frame(&host, channel_id, Surface::Preview, &session_ref, 1);
    let ok = host_ws.send_event(f.clone()).await.expect("frame");
    assert!(ok.accepted, "producer frame rejected: {}", ok.message);
    assert!(arrives(&mut watcher_ws, &watcher_sid, f.id, Duration::from_secs(5)).await);

    // A member who is not the announced producer is refused.
    let forged = frame(&bystander, channel_id, Surface::Preview, &session_ref, 2);
    let ok = bystander_ws.send_event(forged).await.expect("forged frame");
    assert!(!ok.accepted, "non-authority frame accepted");
    assert!(ok.message.starts_with("restricted:"), "got {}", ok.message);

    // A device slot nobody recorded has no producer at all.
    let orphan = frame(&host, channel_id, Surface::Device, "9f2c4e1a7b3d5c80", 1);
    let ok = host_ws.send_event(orphan).await.expect("orphan frame");
    assert!(!ok.accepted, "frame for an unrecorded device accepted");

    // A non-member of the private channel can neither watch nor frame.
    let ok = stranger_ws
        .send_event(watch(
            &stranger,
            channel_id,
            Surface::Preview,
            &session_ref,
            &host,
        ))
        .await
        .expect("stranger watch");
    assert!(!ok.accepted, "non-member watch accepted");
    let ok = stranger_ws
        .send_event(frame(
            &stranger,
            channel_id,
            Surface::Preview,
            &session_ref,
            3,
        ))
        .await
        .expect("stranger frame");
    assert!(!ok.accepted, "non-member frame accepted");
}

/// Oversize frames and host-local values are refused at the relay even when
/// a client skips the builder.
#[tokio::test]
#[ignore = "requires running relay"]
async fn oversize_and_host_local_frames_are_refused() {
    let host = Keys::generate();
    let session_ref = Uuid::new_v4().to_string();
    let mut host_ws = connect(&host).await;
    let channel_id = private_channel(&mut host_ws, &host, &[]).await;
    let ok = host_ws
        .send_event(announce(&host, channel_id, &session_ref))
        .await
        .expect("announce");
    assert!(ok.accepted, "announce rejected: {}", ok.message);

    let sign_raw = |content: String, key: &str| -> Event {
        let tags = frame_header(channel_id, Surface::Preview, &session_ref, 9)
            .tags()
            .into_iter()
            .map(|mut row| {
                if row[0] == "d" {
                    row[1] = key.to_owned();
                }
                Tag::parse(row).expect("tag")
            });
        EventBuilder::new(Kind::Custom(FRAME_KIND), content)
            .tags(tags)
            .sign_with_keys(&host)
            .expect("sign")
    };
    let oversize = format!("/9j/{}", "A".repeat(MAX_SURFACE_FRAME_CONTENT_BYTES));
    let ok = host_ws
        .send_event(sign_raw(oversize, &session_ref))
        .await
        .expect("oversize");
    assert!(!ok.accepted, "oversize frame accepted");

    let jpeg = encode_frame_content(&[0xFF, 0xD8, 0xFF, 0xE0]);
    let ok = host_ws
        .send_event(sign_raw(jpeg, "70E4C638-1A2B-4C3D-9E8F-0123456789AB"))
        .await
        .expect("udid");
    assert!(!ok.accepted, "UDID-shaped d accepted");
}
