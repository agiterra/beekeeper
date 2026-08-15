//! End-to-end tests for session-transport channels (`channel_type =
//! 'transport'`): hidden per-project channels whose access follows the
//! project ACL instead of explicit channel membership.
//!
//! The buzz-db/relay changes under test:
//! - a private transport channel bound to a project admits the project's
//!   owner and invited members for historical reads (REQ), discovery
//!   (kind:39000), and live fan-out — with no `channel_members` row;
//! - a project member (non-channel-member) may PUT_USER (kind:9000) a bot
//!   into the transport channel, but may not grant elevated roles;
//! - the grant is transport-only: an ordinary private `stream` channel with
//!   the same `project` ref admits nobody through the ACL;
//! - republishing the project head without a member revokes their access.
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p buzz-test-client --test e2e_transport_channel -- --ignored
//! ```

use std::time::Duration;

use buzz_test_client::BuzzTestClient;
use buzz_ws_client::RelayMessage;
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag, Timestamp};

const PROJECT_KIND: u16 = 30621;
const MESSAGE_KIND: u16 = 9;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_string())
}

fn sub_id(name: &str) -> String {
    format!("e2e-transport-{name}-{}", uuid::Uuid::new_v4())
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn project_coordinate(owner: &Keys, d_tag: &str) -> String {
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

/// Build a project head with invited-member `p` tags.
fn project_event(
    keys: &Keys,
    d_tag: &str,
    access: Option<&str>,
    members: &[&Keys],
    created_at: Option<u64>,
) -> nostr::Event {
    let mut tags = vec![
        Tag::parse(["d", d_tag]).unwrap(),
        Tag::parse(["title", "transport e2e project"]).unwrap(),
    ];
    if let Some(access) = access {
        tags.push(Tag::parse(["buzz-access", access]).unwrap());
    }
    for member in members {
        tags.push(Tag::parse(["p", &member.public_key().to_hex()]).unwrap());
    }
    let mut builder = EventBuilder::new(Kind::Custom(PROJECT_KIND), "").tags(tags);
    if let Some(ts) = created_at {
        builder = builder.custom_created_at(Timestamp::from(ts));
    }
    builder.sign_with_keys(keys).unwrap()
}

/// Create a private channel of `channel_type` bound to `project_ref`.
async fn create_private_channel(
    client: &mut BuzzTestClient,
    keys: &Keys,
    channel_type: &str,
    project_ref: &str,
) -> String {
    let channel_uuid = uuid::Uuid::new_v4();
    let channel_name = format!("transport-e2e-{channel_uuid}");
    let tags = vec![
        Tag::parse(["h", &channel_uuid.to_string()]).unwrap(),
        Tag::parse(["name", &channel_name]).unwrap(),
        Tag::parse(["channel_type", channel_type]).unwrap(),
        Tag::parse(["visibility", "private"]).unwrap(),
        Tag::parse(["project", project_ref]).unwrap(),
    ];
    let event = EventBuilder::new(Kind::Custom(9007), "")
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap();
    let ok = client.send_event(event).await.expect("send 9007");
    assert!(ok.accepted, "channel creation rejected: {}", ok.message);
    channel_uuid.to_string()
}

fn channel_messages_filter(channel_id: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(MESSAGE_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::H), [channel_id])
}

fn discovery_filter(channel_id: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(39000))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [channel_id])
}

/// Subscribe with `filter` and drain to EOSE.
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

/// Subscribe expecting the relay to CLOSE the subscription with `restricted:`.
async fn expect_restricted(client: &mut BuzzTestClient, name: &str, filter: Filter) {
    let sid = sub_id(name);
    client
        .subscribe(&sid, vec![filter])
        .await
        .expect("subscribe");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or(Duration::ZERO);
        assert!(!remaining.is_zero(), "expected CLOSED, timed out");
        match client.recv_event(remaining).await.expect("recv") {
            RelayMessage::Closed {
                subscription_id,
                message,
            } if subscription_id == sid => {
                assert!(
                    message.starts_with("restricted:"),
                    "expected restricted close, got: {message}"
                );
                return;
            }
            RelayMessage::Eose { subscription_id } if subscription_id == sid => {
                panic!("expected CLOSED for inaccessible channel, got EOSE");
            }
            _ => {}
        }
    }
}

/// Post a kind-9 message into a channel and assert acceptance.
async fn post_message(
    client: &mut BuzzTestClient,
    keys: &Keys,
    channel_id: &str,
) -> nostr::EventId {
    let event = EventBuilder::new(Kind::Custom(MESSAGE_KIND), "transport e2e message")
        .tags(vec![Tag::parse(["h", channel_id]).unwrap()])
        .sign_with_keys(keys)
        .unwrap();
    let id = event.id;
    let ok = client.send_event(event).await.expect("send message");
    assert!(ok.accepted, "message rejected: {}", ok.message);
    id
}

/// A private-project member reads a transport channel's history and
/// discovery record without channel membership; a stranger gets neither.
/// The identical grant does NOT apply to a private stream channel.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_transport_channel_admits_project_members_readonly_paths() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("read");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[&member], None);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let coordinate = project_coordinate(&owner, &d_tag);
    let transport_id =
        create_private_channel(&mut owner_client, &owner, "transport", &coordinate).await;
    let stream_id = create_private_channel(&mut owner_client, &owner, "stream", &coordinate).await;
    let message_id = post_message(&mut owner_client, &owner, &transport_id).await;

    // Invited project member: history + discovery, no channel_members row.
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let events = query(
        &mut member_client,
        "member-history",
        channel_messages_filter(&transport_id),
    )
    .await;
    assert!(
        events.iter().any(|e| e.id == message_id),
        "project member must read transport-channel history"
    );
    let discovery = query(
        &mut member_client,
        "member-discovery",
        discovery_filter(&transport_id),
    )
    .await;
    assert!(
        !discovery.is_empty(),
        "project member must see the transport channel's 39000"
    );

    // The same member has no special access to a private *stream* channel of
    // the same project — the grant is transport-only.
    expect_restricted(
        &mut member_client,
        "member-stream-denied",
        channel_messages_filter(&stream_id),
    )
    .await;

    // Stranger: nothing.
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    expect_restricted(
        &mut stranger_client,
        "stranger-denied",
        channel_messages_filter(&transport_id),
    )
    .await;
}

/// Live fan-out: the project member receives a transport-channel message in
/// real time without channel membership; the revoked member stops receiving
/// reads after an owner republish drops them.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_transport_channel_live_fanout_and_revocation() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let d_tag = unique("live");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let base_ts = Timestamp::now().as_secs();
    let head = project_event(&owner, &d_tag, Some("private"), &[&member], Some(base_ts));
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let coordinate = project_coordinate(&owner, &d_tag);
    let transport_id =
        create_private_channel(&mut owner_client, &owner, "transport", &coordinate).await;

    // Member subscribes live (allowed through the transport arm), then the
    // owner posts.
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let member_sid = sub_id("live-member");
    member_client
        .subscribe(&member_sid, vec![channel_messages_filter(&transport_id)])
        .await
        .expect("member subscribe");
    member_client
        .collect_until_eose(&member_sid, Duration::from_secs(10))
        .await
        .expect("member eose");

    let message_id = post_message(&mut owner_client, &owner, &transport_id).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut member_got = false;
    while tokio::time::Instant::now() < deadline {
        match member_client.recv_event(Duration::from_secs(2)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == member_sid && event.id == message_id => {
                member_got = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(
        member_got,
        "project member must receive live transport fan-out"
    );
    member_client.close_subscription(&member_sid).await.ok();

    // Republish the head without the member — a fresh REQ is now refused.
    let revoked_head = project_event(&owner, &d_tag, Some("private"), &[], Some(base_ts + 10));
    let ok = owner_client
        .send_event(revoked_head)
        .await
        .expect("send revoked head");
    assert!(ok.accepted, "revoking republish rejected: {}", ok.message);

    let mut revoked_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("revoked member connect");
    expect_restricted(
        &mut revoked_client,
        "revoked-denied",
        channel_messages_filter(&transport_id),
    )
    .await;
}

/// A project member without a channel_members row may add a bot (member-level
/// PUT_USER) to a transport channel, but not grant elevated roles; a stranger
/// may not PUT_USER at all.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_transport_channel_bot_add_authority() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let bot = Keys::generate();
    let d_tag = unique("botadd");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[&member], None);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let coordinate = project_coordinate(&owner, &d_tag);
    let transport_id =
        create_private_channel(&mut owner_client, &owner, "transport", &coordinate).await;

    let put_user = |keys: &Keys, target: &Keys, role: &str| {
        EventBuilder::new(Kind::Custom(9000), "")
            .tags(vec![
                Tag::parse(["h", &transport_id]).unwrap(),
                Tag::parse(["p", &target.public_key().to_hex()]).unwrap(),
                Tag::parse(["role", role]).unwrap(),
            ])
            .sign_with_keys(keys)
            .unwrap()
    };

    // Project member adds their provider bot: allowed, member-level.
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let ok = member_client
        .send_event(put_user(&member, &bot, "bot"))
        .await
        .expect("send bot add");
    assert!(
        ok.accepted,
        "project member's bot add must be accepted: {}",
        ok.message
    );

    // The same member may NOT grant an elevated role.
    let elevated = Keys::generate();
    let ok = member_client
        .send_event(put_user(&member, &elevated, "admin"))
        .await
        .expect("send elevated add");
    assert!(
        !ok.accepted,
        "transport admittance must not grant elevated-role authority"
    );

    // A stranger may not PUT_USER at all.
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let ok = stranger_client
        .send_event(put_user(&stranger, &bot, "bot"))
        .await
        .expect("send stranger add");
    assert!(!ok.accepted, "stranger PUT_USER must be rejected");
}
