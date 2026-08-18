//! End-to-end tests for project member roles and relay-managed membership
//! ops (NIP-MP Buzz roles extension).
//!
//! The buzz-db unit tests pin the `admits_read`/`admits_write` matrix and
//! the ingest tests pin the op envelopes; these tests prove the tiers are
//! wired into the live surfaces:
//! - a `viewer` reads a private project's container and channel but every
//!   write into the project is refused, while a `collaborator` writes;
//! - a kind:9010 put-member from the creator seats a co-`owner`, who can
//!   then invite a third member; a stranger's op is refused, and ops
//!   naming the creator are refused;
//! - once the roster is ops-sourced, a creator head republish that drops
//!   every `p` tag does NOT evict op-added members (stale-head no-evict);
//! - the relay-signed kind:39010 roster projection is visible to members
//!   and withheld from strangers.
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p buzz-test-client --test e2e_project_roles -- --ignored
//! ```

use std::time::Duration;

use buzz_test_client::BuzzTestClient;
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag, Timestamp};

const PROJECT_KIND: u16 = 30621;
const PUT_MEMBER_KIND: u16 = 9010;
const REMOVE_MEMBER_KIND: u16 = 9011;
const ROSTER_KIND: u16 = 39010;
const MESSAGE_KIND: u16 = 9;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_string())
}

fn sub_id(name: &str) -> String {
    format!("e2e-project-roles-{name}-{}", uuid::Uuid::new_v4())
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn project_coordinate(owner: &Keys, d_tag: &str) -> String {
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

/// Build a private project head with role-tagged invites.
fn project_event(
    keys: &Keys,
    d_tag: &str,
    members: &[(&Keys, &str)],
    created_at: Option<u64>,
) -> nostr::Event {
    let mut tags = vec![
        Tag::parse(["d", d_tag]).unwrap(),
        Tag::parse(["name", d_tag]).unwrap(),
        Tag::parse(["buzz-access", "private"]).unwrap(),
    ];
    for (member, role) in members {
        tags.push(Tag::parse(["p", &member.public_key().to_hex(), "", role]).unwrap());
    }
    let builder = EventBuilder::new(Kind::Custom(PROJECT_KIND), "").tags(tags);
    match created_at {
        Some(ts) => builder.custom_created_at(Timestamp::from(ts)),
        None => builder,
    }
    .sign_with_keys(keys)
    .unwrap()
}

/// Build a kind:9010 put-member op.
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

/// Create an open channel bound to `project_ref`, signed by `keys`.
async fn create_project_channel(
    client: &mut BuzzTestClient,
    keys: &Keys,
    project_ref: &str,
) -> String {
    let channel_uuid = uuid::Uuid::new_v4();
    let channel_name = format!("project-roles-e2e-{channel_uuid}");
    let tags = vec![
        Tag::parse(["h", &channel_uuid.to_string()]).unwrap(),
        Tag::parse(["name", &channel_name]).unwrap(),
        Tag::parse(["channel_type", "stream"]).unwrap(),
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

fn channel_message(keys: &Keys, channel_id: &str, text: &str) -> nostr::Event {
    EventBuilder::new(Kind::Custom(MESSAGE_KIND), text)
        .tags(vec![Tag::parse(["h", channel_id]).unwrap()])
        .sign_with_keys(keys)
        .unwrap()
}

fn roster_filter(coordinate: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(ROSTER_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [coordinate])
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

/// The role element of every `p` tag on the newest roster projection.
fn roster_roles(events: &[nostr::Event]) -> Vec<(String, String)> {
    let Some(head) = events.iter().max_by_key(|e| e.created_at) else {
        return Vec::new();
    };
    head.tags
        .iter()
        .filter_map(|t| {
            let parts = t.as_slice();
            if parts.first().map(|s| s.as_str()) == Some("p") {
                Some((parts.get(1)?.to_string(), parts.get(3)?.to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// Viewer tier: reads the private container and its channel, but writes into
/// the project are refused; a collaborator's writes land.
#[tokio::test]
#[ignore]
async fn viewer_reads_but_never_writes_live() {
    let owner = Keys::generate();
    let viewer = Keys::generate();
    let collaborator = Keys::generate();
    let d_tag = unique("roles-view");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(
        &owner,
        &d_tag,
        &[(&viewer, "viewer"), (&collaborator, "collaborator")],
        None,
    );
    let ok = owner_client.send_event(head).await.expect("send head");
    assert!(ok.accepted, "project head rejected: {}", ok.message);
    let channel_id = create_project_channel(&mut owner_client, &owner, &coordinate).await;

    // Viewer reads the container.
    let mut viewer_client = BuzzTestClient::connect(&relay_url(), &viewer)
        .await
        .expect("viewer connect");
    let container = query(
        &mut viewer_client,
        "viewer-container",
        Filter::new()
            .kind(Kind::Custom(PROJECT_KIND))
            .author(owner.public_key())
            .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [d_tag.as_str()]),
    )
    .await;
    assert_eq!(container.len(), 1, "viewer must see the private container");

    // Viewer write into the project channel is refused.
    let refused = viewer_client
        .send_event(channel_message(&viewer, &channel_id, "viewer says hi"))
        .await
        .expect("send viewer message");
    assert!(
        !refused.accepted,
        "a project viewer must not write into project channels"
    );
    assert!(
        refused.message.starts_with("restricted:"),
        "expected restricted refusal, got: {}",
        refused.message
    );

    // Collaborator writes land.
    let mut collab_client = BuzzTestClient::connect(&relay_url(), &collaborator)
        .await
        .expect("collaborator connect");
    let accepted = collab_client
        .send_event(channel_message(
            &collaborator,
            &channel_id,
            "collab says hi",
        ))
        .await
        .expect("send collaborator message");
    assert!(
        accepted.accepted,
        "collaborator write refused: {}",
        accepted.message
    );
}

/// Membership ops: the creator seats a co-owner via 9010; the co-owner
/// invites a third member; a stranger's op and creator-targeting ops are
/// refused; after ops, a head republish cannot evict op-added members; the
/// 39010 projection tracks it all and hides from strangers.
#[tokio::test]
#[ignore]
async fn membership_ops_seat_co_owners_and_survive_head_replay_live() {
    let creator = Keys::generate();
    let co_owner = Keys::generate();
    let third = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("roles-ops");
    let coordinate = project_coordinate(&creator, &d_tag);

    let mut creator_client = BuzzTestClient::connect(&relay_url(), &creator)
        .await
        .expect("creator connect");
    let head = project_event(&creator, &d_tag, &[], Some(Timestamp::now().as_secs()));
    let ok = creator_client.send_event(head).await.expect("send head");
    assert!(ok.accepted, "project head rejected: {}", ok.message);

    // A stranger cannot manage the roster.
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let refused = stranger_client
        .send_event(put_member_op(&stranger, &coordinate, &[(&third, "viewer")]))
        .await
        .expect("send stranger op");
    assert!(!refused.accepted, "stranger op must be refused");

    // The creator seats a co-owner.
    let ok = creator_client
        .send_event(put_member_op(
            &creator,
            &coordinate,
            &[(&co_owner, "owner")],
        ))
        .await
        .expect("send co-owner op");
    assert!(ok.accepted, "co-owner put refused: {}", ok.message);

    // Ops naming the creator are refused outright.
    let refused = creator_client
        .send_event(put_member_op(
            &creator,
            &coordinate,
            &[(&creator, "viewer")],
        ))
        .await
        .expect("send creator-target op");
    assert!(!refused.accepted, "creator-targeting put must be refused");
    let refused = creator_client
        .send_event(remove_member_op(&creator, &coordinate, &[&creator]))
        .await
        .expect("send creator-target remove");
    assert!(
        !refused.accepted,
        "creator-targeting remove must be refused"
    );

    // The co-owner can now invite a third member.
    let mut co_owner_client = BuzzTestClient::connect(&relay_url(), &co_owner)
        .await
        .expect("co-owner connect");
    let ok = co_owner_client
        .send_event(put_member_op(&co_owner, &coordinate, &[(&third, "viewer")]))
        .await
        .expect("send third-member op");
    assert!(ok.accepted, "co-owner's invite refused: {}", ok.message);

    // The roster projection reflects both members for a member reader.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let roster = query(
        &mut co_owner_client,
        "roster-members",
        roster_filter(&coordinate),
    )
    .await;
    let roles = roster_roles(&roster);
    assert!(
        roles
            .iter()
            .any(|(pk, role)| *pk == co_owner.public_key().to_hex() && role == "owner"),
        "roster must list the co-owner: {roles:?}"
    );
    assert!(
        roles
            .iter()
            .any(|(pk, role)| *pk == third.public_key().to_hex() && role == "viewer"),
        "roster must list the third member: {roles:?}"
    );

    // The projection is withheld from strangers.
    let hidden = query(
        &mut stranger_client,
        "roster-stranger",
        roster_filter(&coordinate),
    )
    .await;
    assert!(
        hidden.is_empty(),
        "a private project's roster must be withheld from strangers"
    );

    // A creator head republish with NO p tags does not evict op-added
    // members: the roster went ops-sourced with the first accepted op.
    let replay = project_event(&creator, &d_tag, &[], Some(Timestamp::now().as_secs() + 5));
    let ok = creator_client
        .send_event(replay)
        .await
        .expect("send replay");
    assert!(ok.accepted, "head republish rejected: {}", ok.message);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let roster = query(
        &mut co_owner_client,
        "roster-after-replay",
        roster_filter(&coordinate),
    )
    .await;
    let roles = roster_roles(&roster);
    assert!(
        roles
            .iter()
            .any(|(pk, _)| *pk == co_owner.public_key().to_hex()),
        "head replay must not evict the op-seated co-owner: {roles:?}"
    );

    // Removal works and lands in the projection.
    let ok = co_owner_client
        .send_event(remove_member_op(&co_owner, &coordinate, &[&third]))
        .await
        .expect("send remove op");
    assert!(ok.accepted, "remove refused: {}", ok.message);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let roster = query(
        &mut co_owner_client,
        "roster-after-remove",
        roster_filter(&coordinate),
    )
    .await;
    let roles = roster_roles(&roster);
    assert!(
        !roles
            .iter()
            .any(|(pk, _)| *pk == third.public_key().to_hex()),
        "removed member must leave the roster: {roles:?}"
    );
}

/// The `general` project can never be private, and an unknown role element
/// on a head invite is rejected.
#[tokio::test]
#[ignore]
async fn envelope_guards_live() {
    let keys = Keys::generate();
    let mut client = BuzzTestClient::connect(&relay_url(), &keys)
        .await
        .expect("connect");

    let general_private = project_event(&keys, "general", &[], None);
    let refused = client
        .send_event(general_private)
        .await
        .expect("send general-private");
    assert!(
        !refused.accepted,
        "the general project must never be private"
    );

    let d_tag = unique("roles-env");
    let other = Keys::generate();
    let bad_role = EventBuilder::new(Kind::Custom(PROJECT_KIND), "")
        .tags(vec![
            Tag::parse(["d", &d_tag]).unwrap(),
            Tag::parse(["buzz-access", "private"]).unwrap(),
            Tag::parse(["p", &other.public_key().to_hex(), "", "superuser"]).unwrap(),
        ])
        .sign_with_keys(&keys)
        .unwrap();
    let refused = client.send_event(bad_role).await.expect("send bad role");
    assert!(!refused.accepted, "unknown role element must be rejected");
}
