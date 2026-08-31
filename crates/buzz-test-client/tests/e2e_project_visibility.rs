//! End-to-end tests for project visibility levels (NIP-MP Buzz access
//! extension, kind:30621 + `["buzz-access","private"]` + invited-member `p`
//! tags).
//!
//! The buzz-core unit tests pin the per-event predicate and the ingest tests
//! pin the envelope rules; these tests prove the gate is wired into every live
//! surface:
//! - a private container is withheld from strangers across WS REQ, `ids`
//!   lookup, WS live fan-out, HTTP `POST /query`, and HTTP `POST /count`,
//!   while staying visible to its author and each invited `p`-tag member;
//! - a channel bound to a private project (via the `project` ref on its
//!   kind:9007 create) refuses non-member reads AND writes, admits invited
//!   members, and reverts to open after the project is deleted (NIP-09);
//! - an owner republish that drops a member revokes that member's access;
//! - the roster, not the head, is what the container gate reads: a member
//!   seated by a kind:9010 op sees a head that names nobody, and a member
//!   dropped by a kind:9011 op stops seeing a head whose `p` tag still names
//!   them (nothing can rewrite a creator-signed event, which is why the ACL
//!   projection has to be the authority);
//! - tag-less and explicitly-public projects stay community-readable.
//!
//! See `docs/nips/NIP-MP.md` §Access levels for the normative contract.
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p buzz-test-client --test e2e_project_visibility -- --ignored
//! ```

use std::time::Duration;

use buzz_test_client::BuzzTestClient;
use buzz_ws_client::RelayMessage;
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag, Timestamp};
use reqwest::Client;
use serde_json::Value;

const PROJECT_KIND: u16 = 30621;
const PUT_MEMBER_KIND: u16 = 9010;
const REMOVE_MEMBER_KIND: u16 = 9011;
const MESSAGE_KIND: u16 = 9;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_string())
}

fn relay_http_url() -> String {
    relay_url()
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .trim_end_matches('/')
        .to_string()
}

fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client")
}

fn sub_id(name: &str) -> String {
    format!("e2e-project-vis-{name}-{}", uuid::Uuid::new_v4())
}

/// A short unique suffix so concurrent runs never collide on a `d` tag.
fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn project_coordinate(owner: &Keys, d_tag: &str) -> String {
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

/// Build a project head. `access` is the `buzz-access` value (`None` = no
/// tag); `members` are invited pubkeys emitted as `p` tags.
fn project_event(
    keys: &Keys,
    d_tag: &str,
    access: Option<&str>,
    members: &[&Keys],
    created_at: Option<u64>,
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
    let builder = EventBuilder::new(Kind::Custom(PROJECT_KIND), "").tags(tags);
    match created_at {
        Some(ts) => builder.custom_created_at(Timestamp::from(ts)),
        None => builder,
    }
    .sign_with_keys(keys)
    .unwrap()
}

/// Build a kind:9010 put-member op — the roster write every Buzz client
/// actually makes when a member is added after creation. It cannot touch the
/// creator-signed head, so the roster it grows is invisible to any gate that
/// reads `p` tags.
fn put_member_op(signer: &Keys, coordinate: &str, targets: &[(&Keys, &str)]) -> nostr::Event {
    let mut tags = vec![Tag::parse(["a", coordinate]).unwrap()];
    for (target, role) in targets {
        tags.push(Tag::parse(["p", &target.public_key().to_hex(), "", role]).unwrap());
    }
    EventBuilder::new(Kind::Custom(PUT_MEMBER_KIND), "")
        .tags(tags)
        .sign_with_keys(signer)
        .unwrap()
}

/// Build a kind:9011 remove-member op.
fn remove_member_op(signer: &Keys, coordinate: &str, targets: &[&Keys]) -> nostr::Event {
    let mut tags = vec![Tag::parse(["a", coordinate]).unwrap()];
    for target in targets {
        tags.push(Tag::parse(["p", &target.public_key().to_hex()]).unwrap());
    }
    EventBuilder::new(Kind::Custom(REMOVE_MEMBER_KIND), "")
        .tags(tags)
        .sign_with_keys(signer)
        .unwrap()
}

/// A NIP-09 `a`-tag-only deletion at the project coordinate.
fn project_delete(keys: &Keys, d_tag: &str) -> nostr::Event {
    let coord = project_coordinate(keys, d_tag);
    EventBuilder::new(Kind::Custom(5), "")
        .tags(vec![Tag::parse(["a", coord.as_str()]).unwrap()])
        .sign_with_keys(keys)
        .unwrap()
}

/// Create an open-visibility channel, optionally bound to a project via the
/// `project` ref tag captured into `channels.project_ref` on ingest.
async fn create_channel(
    client: &mut BuzzTestClient,
    keys: &Keys,
    project_ref: Option<&str>,
) -> String {
    let channel_uuid = uuid::Uuid::new_v4();
    let channel_name = format!("project-vis-e2e-{channel_uuid}");
    let mut tags = vec![
        Tag::parse(["h", &channel_uuid.to_string()]).unwrap(),
        Tag::parse(["name", &channel_name]).unwrap(),
        Tag::parse(["channel_type", "stream"]).unwrap(),
    ];
    if let Some(project_ref) = project_ref {
        tags.push(Tag::parse(["project", project_ref]).unwrap());
    }
    let event = EventBuilder::new(Kind::Custom(9007), "")
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap();
    let ok = client.send_event(event).await.expect("send 9007");
    assert!(ok.accepted, "channel creation rejected: {}", ok.message);
    channel_uuid.to_string()
}

fn container_filter(owner: &Keys, d_tag: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(PROJECT_KIND))
        .author(owner.public_key())
        .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [d_tag])
}

fn channel_messages_filter(channel_id: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(MESSAGE_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::H), [channel_id])
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

/// Query events via the NIP-98 HTTP bridge (`POST /query`).
async fn query_events_http(client: &Client, pubkey_hex: &str, filters: Vec<Filter>) -> Vec<Value> {
    let resp = client
        .post(format!("{}/query", relay_http_url()))
        .header("X-Pubkey", pubkey_hex)
        .header("Content-Type", "application/json")
        .json(&filters)
        .send()
        .await
        .expect("query events");
    assert!(
        resp.status().is_success(),
        "query failed: {}",
        resp.status()
    );
    resp.json::<Vec<Value>>()
        .await
        .expect("parse query response")
}

/// Count events via the NIP-98 HTTP bridge (`POST /count`).
async fn count_events_http(client: &Client, pubkey_hex: &str, filters: Vec<Filter>) -> u64 {
    let resp = client
        .post(format!("{}/count", relay_http_url()))
        .header("X-Pubkey", pubkey_hex)
        .header("Content-Type", "application/json")
        .json(&filters)
        .send()
        .await
        .expect("count events");
    assert!(
        resp.status().is_success(),
        "count failed: {}",
        resp.status()
    );
    let body: Value = resp.json().await.expect("parse count response");
    body["count"].as_u64().unwrap_or(0)
}

/// The container gate across WS REQ, `ids` lookup, HTTP `/query`, and HTTP
/// `/count`: author and invited member see the private head, a stranger never
/// does — and never learns it exists.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_private_container_hidden_from_stranger_everywhere() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("secret");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[&member], None);
    let head_id = head.id;
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    // Author sees their own head.
    let seen = query(&mut owner_client, "own", container_filter(&owner, &d_tag)).await;
    assert_eq!(seen.len(), 1, "author must see their private project");

    // Invited member sees it.
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let seen = query(
        &mut member_client,
        "member",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert_eq!(seen.len(), 1, "invited member must see the private project");

    // Stranger sees nothing via the coordinate filter…
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(
        &mut stranger_client,
        "stranger",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert!(seen.is_empty(), "stranger must not see a private project");

    // …nor via a kindless known-id lookup…
    let seen = query(
        &mut stranger_client,
        "stranger-ids",
        Filter::new().id(head_id),
    )
    .await;
    assert!(
        seen.is_empty(),
        "known event id must not bypass the project gate"
    );

    // …nor via the HTTP bridge.
    let http = http_client();
    let rows = query_events_http(
        &http,
        &stranger.public_key().to_hex(),
        vec![container_filter(&owner, &d_tag)],
    )
    .await;
    assert!(
        rows.is_empty(),
        "HTTP /query must withhold private projects"
    );
    let n = count_events_http(
        &http,
        &stranger.public_key().to_hex(),
        vec![container_filter(&owner, &d_tag)],
    )
    .await;
    assert_eq!(n, 0, "HTTP /count must not leak private project existence");

    // Owner and member DO count it over HTTP.
    let n = count_events_http(
        &http,
        &owner.public_key().to_hex(),
        vec![container_filter(&owner, &d_tag)],
    )
    .await;
    assert_eq!(n, 1, "author's own count must include the head");
    let n = count_events_http(
        &http,
        &member.public_key().to_hex(),
        vec![container_filter(&owner, &d_tag)],
    )
    .await;
    assert_eq!(n, 1, "member's count must include the head");
}

/// Live fan-out: with open kind-30621 subscriptions, publishing a private head
/// reaches the invited member's connection and never the stranger's.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_private_container_fanout_filtered_live() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("live");

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");

    let member_sid = sub_id("live-member");
    let stranger_sid = sub_id("live-stranger");
    let live_filter = Filter::new().kind(Kind::Custom(PROJECT_KIND));
    member_client
        .subscribe(&member_sid, vec![live_filter.clone()])
        .await
        .expect("member subscribe");
    member_client
        .collect_until_eose(&member_sid, Duration::from_secs(10))
        .await
        .expect("member eose");
    stranger_client
        .subscribe(&stranger_sid, vec![live_filter])
        .await
        .expect("stranger subscribe");
    stranger_client
        .collect_until_eose(&stranger_sid, Duration::from_secs(10))
        .await
        .expect("stranger eose");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[&member], None);
    let head_id = head.id;
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    // Member receives the live event.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut member_got = false;
    while tokio::time::Instant::now() < deadline {
        match member_client.recv_event(Duration::from_secs(2)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == member_sid && event.id == head_id => {
                member_got = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(member_got, "invited member must receive the live fan-out");

    // Stranger receives nothing for this head within the wait window.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match stranger_client.recv_event(Duration::from_secs(1)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == stranger_sid => {
                assert_ne!(
                    event.id, head_id,
                    "stranger must not receive a private project via fan-out"
                );
            }
            Ok(_) => {}
            Err(_) => {} // timeout tick — keep waiting out the window
        }
    }
}

/// Live fan-out follows the roster too: a member seated by a kind:9010 op
/// receives the head's republish, and the connection gate resolves the grant
/// from the ACL rather than the event in flight.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_roster_op_member_receives_live_fanout() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let d_tag = unique("ops-live");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let now = Timestamp::now().as_secs();
    let ok = owner_client
        .send_event(project_event(
            &owner,
            &d_tag,
            Some("private"),
            &[],
            Some(now),
        ))
        .await
        .expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);
    let ok = owner_client
        .send_event(put_member_op(&owner, &coordinate, &[(&member, "viewer")]))
        .await
        .expect("send put-member");
    assert!(ok.accepted, "put-member op rejected: {}", ok.message);

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let member_sid = sub_id("ops-live-member");
    member_client
        .subscribe(
            &member_sid,
            vec![Filter::new().kind(Kind::Custom(PROJECT_KIND))],
        )
        .await
        .expect("member subscribe");
    member_client
        .collect_until_eose(&member_sid, Duration::from_secs(10))
        .await
        .expect("member eose");

    // Republish the head (still naming nobody) — the roster is ops-sourced
    // now, so the republish neither grants nor revokes anything.
    let head = project_event(&owner, &d_tag, Some("private"), &[], Some(now + 1));
    let head_id = head.id;
    let ok = owner_client.send_event(head).await.expect("send republish");
    assert!(ok.accepted, "republish rejected: {}", ok.message);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut member_got = false;
    while tokio::time::Instant::now() < deadline {
        match member_client.recv_event(Duration::from_secs(2)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == member_sid && event.id == head_id => {
                member_got = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(
        member_got,
        "a roster-op member must receive the private head's live fan-out"
    );
}

/// A member added by a kind:9010 roster op reads the container on every
/// surface — the head names nobody, so a `p`-tag gate would show this project
/// to its creator alone.
///
/// This is the shape every Buzz client produces: `ProjectMembersManager` and
/// `bee projects members put` publish a 9010, and nothing republishes the
/// creator-signed head afterwards (nor could a co-owner sign one).
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_roster_op_member_reads_the_container_everywhere() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("ops-roster");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    // A private head with no invites at all.
    let head = project_event(&owner, &d_tag, Some("private"), &[], None);
    let head_id = head.id;
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let seen = query(
        &mut member_client,
        "pre-op",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert!(
        seen.is_empty(),
        "a pubkey on no roster must not see the project"
    );

    let ok = owner_client
        .send_event(put_member_op(
            &owner,
            &coordinate,
            &[(&member, "collaborator")],
        ))
        .await
        .expect("send put-member");
    assert!(ok.accepted, "put-member op rejected: {}", ok.message);

    // WS REQ by coordinate.
    let seen = query(
        &mut member_client,
        "post-op",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert_eq!(
        seen.len(),
        1,
        "a member added by a roster op must see the private project"
    );

    // The unfiltered project listing the desktop actually sends.
    let listed = query(
        &mut member_client,
        "post-op-list",
        Filter::new().kind(Kind::Custom(PROJECT_KIND)),
    )
    .await;
    assert!(
        listed.iter().any(|e| e.id == head_id),
        "the project must appear in a plain {{\"kinds\":[30621]}} listing"
    );

    // Kindless known-id lookup.
    let seen = query(&mut member_client, "post-op-ids", Filter::new().id(head_id)).await;
    assert_eq!(seen.len(), 1, "member must resolve the head by id");

    // HTTP bridge: /query and /count.
    let http = http_client();
    let rows = query_events_http(
        &http,
        &member.public_key().to_hex(),
        vec![container_filter(&owner, &d_tag)],
    )
    .await;
    assert_eq!(rows.len(), 1, "HTTP /query must serve the roster member");
    let n = count_events_http(
        &http,
        &member.public_key().to_hex(),
        vec![container_filter(&owner, &d_tag)],
    )
    .await;
    assert_eq!(n, 1, "HTTP /count must include the head for a member");

    // A stranger is unaffected by any of it.
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(
        &mut stranger_client,
        "stranger-after-op",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert!(seen.is_empty(), "a roster op admits only its targets");
    let n = count_events_http(
        &http,
        &stranger.public_key().to_hex(),
        vec![container_filter(&owner, &d_tag)],
    )
    .await;
    assert_eq!(n, 0, "HTTP /count must not leak the head to a stranger");
}

/// A kind:9011 removal revokes container access even though the removed
/// member's `p` tag is still on the creator-signed head — nothing can rewrite
/// that event, so the roster has to be what decides.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_roster_op_removal_revokes_a_stale_head_invite() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let d_tag = unique("ops-revoke");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    // The head invites the member directly, and is never republished.
    let head = project_event(&owner, &d_tag, Some("private"), &[&member], None);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let seen = query(
        &mut member_client,
        "head-invited",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert_eq!(
        seen.len(),
        1,
        "a head-carried invite is the bootstrap roster and must still work"
    );

    let ok = owner_client
        .send_event(remove_member_op(&owner, &coordinate, &[&member]))
        .await
        .expect("send remove-member");
    assert!(ok.accepted, "remove-member op rejected: {}", ok.message);

    let seen = query(
        &mut member_client,
        "ops-removed",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert!(
        seen.is_empty(),
        "the removal must hold even though the head still carries the p tag"
    );
    let n = count_events_http(
        &http_client(),
        &member.public_key().to_hex(),
        vec![container_filter(&owner, &d_tag)],
    )
    .await;
    assert_eq!(n, 0, "HTTP /count must respect the removal too");
}

/// Channel contents: a channel bound to a private project refuses stranger
/// reads and writes, admits the invited member for both, and reverts to open
/// when the project is deleted.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_private_project_channel_gates_reads_and_writes() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("chan");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[&member], None);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let coord = project_coordinate(&owner, &d_tag);
    let channel_id = create_channel(&mut owner_client, &owner, Some(&coord)).await;

    let ok = owner_client
        .send_text_message(&owner, &channel_id, "owner message", MESSAGE_KIND)
        .await
        .expect("owner send");
    assert!(ok.accepted, "owner write rejected: {}", ok.message);

    // Member (not an explicit channel member — admitted via project ACL) can
    // read and write.
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let seen = query(
        &mut member_client,
        "member-read",
        channel_messages_filter(&channel_id),
    )
    .await;
    assert_eq!(
        seen.len(),
        1,
        "project member must read the private project's channel"
    );
    let ok = member_client
        .send_text_message(&member, &channel_id, "member message", MESSAGE_KIND)
        .await
        .expect("member send");
    assert!(ok.accepted, "member write rejected: {}", ok.message);

    // Stranger: read is CLOSED restricted, write is rejected.
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    expect_restricted(
        &mut stranger_client,
        "stranger-read",
        channel_messages_filter(&channel_id),
    )
    .await;
    let ok = stranger_client
        .send_text_message(&stranger, &channel_id, "stranger message", MESSAGE_KIND)
        .await
        .expect("stranger send transport");
    assert!(
        !ok.accepted,
        "stranger write into a private project's channel must be rejected"
    );

    // Delete the project — the channel reverts to its own (open) access rules.
    let ok = owner_client
        .send_event(project_delete(&owner, &d_tag))
        .await
        .expect("send delete");
    assert!(ok.accepted, "project deletion rejected: {}", ok.message);
    // The ACL drop invalidates access caches; poll briefly for propagation.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut revealed = false;
    while tokio::time::Instant::now() < deadline {
        let mut fresh = BuzzTestClient::connect(&relay_url(), &stranger)
            .await
            .expect("stranger reconnect");
        let sid = sub_id("stranger-after-delete");
        fresh
            .subscribe(&sid, vec![channel_messages_filter(&channel_id)])
            .await
            .expect("subscribe");
        if let Ok(events) = fresh.collect_until_eose(&sid, Duration::from_secs(5)).await {
            if events.len() >= 2 {
                revealed = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert!(
        revealed,
        "deleting the project must revert its channel to open access"
    );
}

/// Owner republish without a member's `p` tag revokes that member's access to
/// the container.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_membership_republish_revokes_access() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let d_tag = unique("revoke");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let now = Timestamp::now().as_secs();
    let head = project_event(&owner, &d_tag, Some("private"), &[&member], Some(now));
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let seen = query(
        &mut member_client,
        "before-revoke",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert_eq!(seen.len(), 1, "member must see the project while invited");

    // Republish with an empty invite list (newer created_at wins).
    let head = project_event(&owner, &d_tag, Some("private"), &[], Some(now + 1));
    let ok = owner_client.send_event(head).await.expect("send revoke");
    assert!(ok.accepted, "revoking republish rejected: {}", ok.message);

    let seen = query(
        &mut member_client,
        "after-revoke",
        container_filter(&owner, &d_tag),
    )
    .await;
    assert!(
        seen.is_empty(),
        "removed member must no longer see the private project"
    );
}

/// Tag-less and explicitly-public projects stay readable by everyone — the
/// pre-extension default is unchanged.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_public_and_tagless_projects_stay_visible() {
    let owner = Keys::generate();
    let stranger = Keys::generate();
    let tagless_d = unique("tagless");
    let public_d = unique("public");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let ok = owner_client
        .send_event(project_event(&owner, &tagless_d, None, &[], None))
        .await
        .expect("send tagless");
    assert!(ok.accepted, "tagless project rejected: {}", ok.message);
    let ok = owner_client
        .send_event(project_event(&owner, &public_d, Some("public"), &[], None))
        .await
        .expect("send public");
    assert!(ok.accepted, "public project rejected: {}", ok.message);

    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(
        &mut stranger_client,
        "tagless",
        container_filter(&owner, &tagless_d),
    )
    .await;
    assert_eq!(seen.len(), 1, "tag-less project must stay public");
    let seen = query(
        &mut stranger_client,
        "public",
        container_filter(&owner, &public_d),
    )
    .await;
    assert_eq!(seen.len(), 1, "explicitly-public project must stay public");
}

/// The relay refuses malformed access envelopes: unknown `buzz-access` values,
/// duplicate access tags, and malformed invites — wired into the live write
/// path, not just the validator.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_malformed_access_envelopes_rejected() {
    let owner = Keys::generate();
    let mut client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("connect");

    // Unknown access value.
    let ev = EventBuilder::new(Kind::Custom(PROJECT_KIND), "")
        .tags(vec![
            Tag::parse(["d", &unique("bad")]).unwrap(),
            Tag::parse(["buzz-access", "privat"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    let ok = client.send_event(ev).await.expect("send");
    assert!(!ok.accepted, "unknown buzz-access value must be rejected");

    // Duplicate access tags.
    let ev = EventBuilder::new(Kind::Custom(PROJECT_KIND), "")
        .tags(vec![
            Tag::parse(["d", &unique("dup")]).unwrap(),
            Tag::parse(["buzz-access", "private"]).unwrap(),
            Tag::parse(["buzz-access", "public"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    let ok = client.send_event(ev).await.expect("send");
    assert!(!ok.accepted, "duplicate buzz-access tags must be rejected");

    // Uppercase invite pubkey.
    let ev = EventBuilder::new(Kind::Custom(PROJECT_KIND), "")
        .tags(vec![
            Tag::parse(["d", &unique("upper")]).unwrap(),
            Tag::parse(["buzz-access", "private"]).unwrap(),
            Tag::parse(["p", &"C".repeat(64)]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    let ok = client.send_event(ev).await.expect("send");
    assert!(!ok.accepted, "uppercase invite pubkey must be rejected");
}
