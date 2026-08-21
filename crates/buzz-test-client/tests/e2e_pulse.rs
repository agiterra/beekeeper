//! End-to-end tests for NIP-MP Project Pulse entries (kind:44240).
//!
//! The buzz-core unit tests pin the envelope, the ingest tests pin write
//! admission, and the buzz-db tests pin the SQL pushdown. These tests prove the
//! gate and the query shape are wired into the *live* surfaces, which no
//! in-process test can reach:
//!
//! - a private project's entries are withheld from strangers over **WS REQ**,
//!   **HTTP `POST /query`**, **HTTP `POST /count`**, and **live fan-out** —
//!   four separate surfaces, so all four are asserted;
//! - the WS REQ path is gated per event, not by the bridge's request-shape
//!   rule: `["REQ","x",{"kinds":[44240]}]` with no `#a` at all returns nothing
//!   of a private project;
//! - an inadmissible private-project read is an empty **200**, never a 403 —
//!   a 403 would tell a stranger the project exists;
//! - unscoped and multi-project 44240 bridge queries fail closed with **400**;
//! - a project *writer* publishes and a read-only *viewer* does not (the
//!   fixture project is private on purpose: a public project admits any
//!   community member by design, which would make the assertion vacuous);
//! - project authorization never widens channel authorization: an entry
//!   carrying `h` needs channel admission too;
//! - a community cannot retrieve another community's coordinate;
//! - supersession is a fold-time claim — it never mutates or deletes its
//!   target.
//!
//! See `docs/PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md`
//! §5.2–5.3 for the normative contract.
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p buzz-test-client --test e2e_pulse -- --ignored
//! ```
//!
//! The cross-community test needs two *hosts* on one relay process, since the
//! community is derived from the request host. Its defaults —
//! `ws://localhost:3000` and `ws://127.0.0.1:3000` — are two distinct hosts
//! that both resolve to a plain local relay, so it runs unmodified against
//! `just relay`; override with `RELAY_URL_A` / `RELAY_URL_B` for a real
//! two-host deployment (the `tests/conformance_multitenant.rs` convention).

use std::time::Duration;

use buzz_test_client::BuzzTestClient;
use buzz_ws_client::RelayMessage;
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PROJECT_KIND: u16 = 30621;
const PULSE_ENTRY_KIND: u16 = 44240;
const PULSE_TAG_VERSION: &str = "pu1-1";
const MESSAGE_KIND: u16 = 9;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_string())
}

/// Host A's WebSocket URL for the two-community isolation test.
fn relay_url_a() -> String {
    std::env::var("RELAY_URL_A").unwrap_or_else(|_| relay_url())
}

/// Host B's WebSocket URL — the same relay process reached through a *different
/// host*, which is what makes it a different community. `127.0.0.1` and
/// `localhost` are distinct hosts to the tenant binder even though they address
/// the same socket, so the default pair works against a plain local relay.
fn relay_url_b() -> String {
    std::env::var("RELAY_URL_B").unwrap_or_else(|_| "ws://127.0.0.1:3000".to_string())
}

fn to_http(ws_url: &str) -> String {
    ws_url
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .trim_end_matches('/')
        .to_string()
}

fn relay_http_url() -> String {
    to_http(&relay_url())
}

fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client")
}

fn sub_id(name: &str) -> String {
    format!("e2e-pulse-{name}-{}", uuid::Uuid::new_v4())
}

/// A short unique suffix so concurrent runs never collide on a `d` tag.
fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn project_coordinate(owner: &Keys, d_tag: &str) -> String {
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

/// Build a project head. `access` is the `buzz-access` value (`None` = no tag,
/// i.e. public); `members` are invited pubkeys with their NIP-MP roles.
fn project_event(
    keys: &Keys,
    d_tag: &str,
    access: Option<&str>,
    members: &[(&Keys, &str)],
) -> nostr::Event {
    let mut tags = vec![
        Tag::parse(["d", d_tag]).unwrap(),
        Tag::parse(["name", d_tag]).unwrap(),
    ];
    if let Some(access) = access {
        tags.push(Tag::parse(["buzz-access", access]).unwrap());
    }
    for (member, role) in members {
        tags.push(Tag::parse(["p", &member.public_key().to_hex(), "", role]).unwrap());
    }
    EventBuilder::new(Kind::Custom(PROJECT_KIND), "")
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap()
}

/// Build a kind:44240 Pulse entry in the canonical tag order
/// (`a` / `pu-v` / `pu-type` / `[h]`).
fn pulse_entry(
    keys: &Keys,
    coordinate: &str,
    entry_type: &str,
    text: &str,
    channel: Option<&str>,
    supersedes: Option<&str>,
) -> nostr::Event {
    let mut content = json!({
        "schema": "buzz-pulse-entry/v1",
        "type": entry_type,
        "text": text,
    });
    if let Some(target) = supersedes {
        content["supersedes"] = json!(target);
    }
    let mut tags = vec![
        Tag::parse(["a", coordinate]).unwrap(),
        Tag::parse(["pu-v", PULSE_TAG_VERSION]).unwrap(),
        Tag::parse(["pu-type", entry_type]).unwrap(),
    ];
    if let Some(channel) = channel {
        tags.push(Tag::parse(["h", channel]).unwrap());
    }
    EventBuilder::new(Kind::Custom(PULSE_ENTRY_KIND), content.to_string())
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap()
}

/// The canonical Slice 1 read filter: `{"kinds":[44240],"#a":["30621:…"]}`.
fn pulse_filter(coordinate: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(PULSE_ENTRY_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::A), [coordinate])
}

/// Create a channel signed by `keys`. `visibility` is the 9007 tag value.
async fn create_channel(client: &mut BuzzTestClient, keys: &Keys, visibility: &str) -> String {
    let channel_uuid = uuid::Uuid::new_v4();
    let tags = vec![
        Tag::parse(["h", &channel_uuid.to_string()]).unwrap(),
        Tag::parse(["name", &format!("pulse-e2e-{channel_uuid}")]).unwrap(),
        Tag::parse(["channel_type", "stream"]).unwrap(),
        Tag::parse(["visibility", visibility]).unwrap(),
    ];
    let event = EventBuilder::new(Kind::Custom(9007), "")
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap();
    let ok = client.send_event(event).await.expect("send 9007");
    assert!(ok.accepted, "channel creation rejected: {}", ok.message);
    channel_uuid.to_string()
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

/// `POST /query` returning the raw status alongside the body, so a test can
/// assert *which* status a shape produces (200-empty vs 400) rather than
/// asserting success and losing the distinction.
async fn post_query_raw(
    client: &Client,
    base_http: &str,
    pubkey_hex: &str,
    filters: &Value,
) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{base_http}/query"))
        .header("X-Pubkey", pubkey_hex)
        .header("Content-Type", "application/json")
        .json(filters)
        .send()
        .await
        .expect("post /query");
    let status = resp.status();
    let body = resp.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

/// `POST /count`, same raw shape as [`post_query_raw`].
async fn post_count_raw(
    client: &Client,
    base_http: &str,
    pubkey_hex: &str,
    filters: &Value,
) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{base_http}/count"))
        .header("X-Pubkey", pubkey_hex)
        .header("Content-Type", "application/json")
        .json(filters)
        .send()
        .await
        .expect("post /count");
    let status = resp.status();
    let body = resp.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

/// The canonical single-project bridge filter body.
fn pulse_query_body(coordinate: &str) -> Value {
    json!([{ "kinds": [PULSE_ENTRY_KIND], "#a": [coordinate], "limit": 50 }])
}

async fn query_pulse_http(client: &Client, pubkey_hex: &str, coordinate: &str) -> Vec<Value> {
    let (status, body) = post_query_raw(
        client,
        &relay_http_url(),
        pubkey_hex,
        &pulse_query_body(coordinate),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "pulse /query failed: {body}");
    body.as_array().cloned().unwrap_or_default()
}

async fn count_pulse_http(client: &Client, pubkey_hex: &str, coordinate: &str) -> u64 {
    let (status, body) = post_count_raw(
        client,
        &relay_http_url(),
        pubkey_hex,
        &pulse_query_body(coordinate),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "pulse /count failed: {body}");
    body["count"].as_u64().unwrap_or(0)
}

/// Publish a project head and one entry, returning the entry's id.
async fn seed_entry(
    client: &mut BuzzTestClient,
    author: &Keys,
    coordinate: &str,
    text: &str,
) -> nostr::EventId {
    let entry = pulse_entry(author, coordinate, "plan", text, None, None);
    let id = entry.id;
    let ok = client.send_event(entry).await.expect("send 44240");
    assert!(ok.accepted, "pulse entry rejected: {}", ok.message);
    id
}

/// A private project's entries are visible to its members and withheld from a
/// stranger across WS REQ, HTTP `/query`, and HTTP `/count`.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_pulse_entries_visible_to_members_hidden_from_stranger() {
    let owner = Keys::generate();
    let viewer = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("pulse-private");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[(&viewer, "viewer")]);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let entry_id = seed_entry(
        &mut owner_client,
        &owner,
        &coordinate,
        "Refactoring session creation; pool.rs will churn.",
    )
    .await;

    // Author reads their own entry back over WS REQ.
    let seen = query(&mut owner_client, "owner", pulse_filter(&coordinate)).await;
    assert_eq!(seen.len(), 1, "author must read their own pulse entry");
    assert_eq!(seen[0].id, entry_id);

    // A read-only member reads it too — viewers read the Pulse.
    let mut viewer_client = BuzzTestClient::connect(&relay_url(), &viewer)
        .await
        .expect("viewer connect");
    let seen = query(&mut viewer_client, "viewer", pulse_filter(&coordinate)).await;
    assert_eq!(seen.len(), 1, "invited member must read the pulse entry");

    // A stranger sees nothing, on any surface.
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(&mut stranger_client, "stranger", pulse_filter(&coordinate)).await;
    assert!(
        seen.is_empty(),
        "WS REQ must withhold a private project's pulse entries"
    );

    // …not even through a kindless known-id lookup, which skips `#a` entirely.
    let seen = query(
        &mut stranger_client,
        "stranger-ids",
        Filter::new().id(entry_id),
    )
    .await;
    assert!(
        seen.is_empty(),
        "a known event id must not bypass the pulse gate"
    );

    let http = http_client();
    let stranger_hex = stranger.public_key().to_hex();
    let rows = query_pulse_http(&http, &stranger_hex, &coordinate).await;
    assert!(rows.is_empty(), "HTTP /query must withhold pulse entries");
    let n = count_pulse_http(&http, &stranger_hex, &coordinate).await;
    assert_eq!(n, 0, "HTTP /count must not leak entry existence");

    // Members do see them over HTTP.
    let rows = query_pulse_http(&http, &owner.public_key().to_hex(), &coordinate).await;
    assert_eq!(rows.len(), 1, "author's HTTP /query must return the entry");
    let n = count_pulse_http(&http, &viewer.public_key().to_hex(), &coordinate).await;
    assert_eq!(n, 1, "member's HTTP /count must include the entry");
}

/// The WS REQ path is gated **per event**, not by the bridge's request-shape
/// rule: an unscoped `{"kinds":[44240]}` subscription — which the bridge would
/// reject at 400 but WebSocket accepts — still returns nothing belonging to a
/// private project the reader is not admitted to.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_unscoped_ws_req_cannot_read_private_project_entries() {
    let owner = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("pulse-ws");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[]);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);
    let entry_id = seed_entry(
        &mut owner_client,
        &owner,
        &coordinate,
        "Private coordination note.",
    )
    .await;

    let unscoped = || {
        Filter::new()
            .kind(Kind::Custom(PULSE_ENTRY_KIND))
            .limit(200)
    };

    // Control: the same unscoped subscription *does* return the entry to its
    // author, so the assertion below is the gate and not an empty relay.
    let seen = query(&mut owner_client, "ws-unscoped-owner", unscoped()).await;
    assert!(
        seen.iter().any(|e| e.id == entry_id),
        "control: an unscoped WS REQ must return the author's own entry"
    );

    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(&mut stranger_client, "ws-unscoped", unscoped()).await;
    assert!(
        seen.iter().all(|e| e.id != entry_id),
        "an unscoped WS REQ must not return a private project's pulse entry"
    );
}

/// Live fan-out is a separate surface from historical reads: a newly published
/// entry reaches the member's open 44240 subscription and never the stranger's.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_pulse_entry_fanout_filtered_live() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("pulse-live");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(
        &owner,
        &d_tag,
        Some("private"),
        &[(&member, "collaborator")],
    );
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");

    let member_sid = sub_id("live-member");
    let stranger_sid = sub_id("live-stranger");
    let live_filter = Filter::new().kind(Kind::Custom(PULSE_ENTRY_KIND));
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

    let entry_id = seed_entry(
        &mut owner_client,
        &owner,
        &coordinate,
        "Live coordination claim.",
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut member_got = false;
    while tokio::time::Instant::now() < deadline {
        match member_client.recv_event(Duration::from_secs(2)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == member_sid && event.id == entry_id => {
                member_got = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(member_got, "project member must receive the live fan-out");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match stranger_client.recv_event(Duration::from_secs(1)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == stranger_sid => {
                assert_ne!(
                    event.id, entry_id,
                    "stranger must never receive a private project's pulse entry live"
                );
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

/// Write admission is role-aware on a **private** project: a collaborator
/// publishes, a read-only viewer is refused and still reads.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_private_project_writer_publishes_viewer_cannot() {
    let owner = Keys::generate();
    let writer = Keys::generate();
    let viewer = Keys::generate();
    let d_tag = unique("pulse-roles");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(
        &owner,
        &d_tag,
        Some("private"),
        &[(&writer, "collaborator"), (&viewer, "viewer")],
    );
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let mut writer_client = BuzzTestClient::connect(&relay_url(), &writer)
        .await
        .expect("writer connect");
    let entry = pulse_entry(
        &writer,
        &coordinate,
        "plan",
        "Collaborator claims the ingest path.",
        None,
        None,
    );
    let ok = writer_client.send_event(entry).await.expect("writer send");
    assert!(ok.accepted, "collaborator write rejected: {}", ok.message);

    let mut viewer_client = BuzzTestClient::connect(&relay_url(), &viewer)
        .await
        .expect("viewer connect");
    let entry = pulse_entry(
        &viewer,
        &coordinate,
        "plan",
        "Viewer should not be able to claim anything.",
        None,
        None,
    );
    let rejected = viewer_client.send_event(entry).await.expect("viewer send");
    assert!(
        !rejected.accepted,
        "a read-only member must not publish a pulse entry"
    );
    assert!(
        rejected.message.starts_with("restricted:"),
        "expected a restricted rejection, got: {}",
        rejected.message
    );

    // The same refusal over HTTP must be a **403**, not a 400. §5.4's exit-code
    // table routes a refused write to CLI exit 3 (auth) via
    // `CliError::Relay{status:403}`; a 400 maps to exit 2 (transport), which an
    // agent script cannot tell apart from a network failure. WebSocket cannot
    // observe this — `Rejected` and `AuthFailed` are both `OK false` on the
    // wire — so the status is only checkable here.
    let http = http_client();
    let refused = pulse_entry(
        &viewer,
        &coordinate,
        "plan",
        "Viewer write over HTTP must be 403.",
        None,
        None,
    );
    let resp = http
        .post(format!("{}/events", relay_http_url()))
        .header("X-Pubkey", viewer.public_key().to_hex())
        .header("Content-Type", "application/json")
        .json(&serde_json::to_value(&refused).expect("serialize entry"))
        .send()
        .await
        .expect("post /events");
    let status = resp.status();
    let body = resp.json::<Value>().await.unwrap_or(Value::Null);
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a refused pulse write must answer 403 (CLI exit 3), not 400 (exit 2): {body}"
    );

    // The viewer still reads what the collaborator wrote.
    let seen = query(&mut viewer_client, "viewer-read", pulse_filter(&coordinate)).await;
    assert_eq!(seen.len(), 1, "a viewer reads the Pulse it cannot write");
}

/// A 44240 naming a coordinate no kind:30621 event ever created is refused —
/// an unknown coordinate is in nobody's hidden set, so storing it would show
/// everyone a coordination fact with no project behind it.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_pulse_entry_for_unknown_coordinate_is_rejected() {
    let author = Keys::generate();
    let coordinate = project_coordinate(&author, &unique("never-created"));
    let mut client = BuzzTestClient::connect(&relay_url(), &author)
        .await
        .expect("connect");
    let entry = pulse_entry(
        &author,
        &coordinate,
        "note",
        "A claim about a project that does not exist.",
        None,
        None,
    );
    let rejected = client.send_event(entry).await.expect("send 44240");
    assert!(
        !rejected.accepted,
        "an unknown project coordinate must be refused"
    );
    assert!(
        rejected.message.contains("unknown project coordinate"),
        "unexpected rejection message: {}",
        rejected.message
    );
}

/// Project authorization never widens channel authorization: an entry carrying
/// `h` must also clear the channel's own membership gate.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_h_tagged_entry_requires_channel_admission() {
    let owner = Keys::generate();
    let outsider = Keys::generate();
    let d_tag = unique("pulse-h");
    let coordinate = project_coordinate(&owner, &d_tag);

    // A public project, so the *only* thing that can refuse the write below is
    // the channel gate.
    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("public"), &[]);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "public project rejected: {}", ok.message);

    // A private channel owned by someone else entirely.
    let mut outsider_client = BuzzTestClient::connect(&relay_url(), &outsider)
        .await
        .expect("outsider connect");
    let private_channel = create_channel(&mut outsider_client, &outsider, "private").await;

    // The project owner may write the project's Pulse, but not into that
    // channel.
    let entry = pulse_entry(
        &owner,
        &coordinate,
        "note",
        "Trying to land in a channel I am not in.",
        Some(&private_channel),
        None,
    );
    let rejected = owner_client.send_event(entry).await.expect("send 44240");
    assert!(
        !rejected.accepted,
        "an h-tagged entry must clear the channel gate too"
    );

    // Control: the same author writes an ordinary message into that channel and
    // is refused for the same reason, so the assertion above is about channel
    // admission and not about 44240 specifically.
    let message = EventBuilder::new(Kind::Custom(MESSAGE_KIND), "control")
        .tags(vec![Tag::parse(["h", &private_channel]).unwrap()])
        .sign_with_keys(&owner)
        .unwrap();
    let control = owner_client.send_event(message).await.expect("send 9");
    assert!(
        !control.accepted,
        "control: a non-member must not write into a private channel"
    );

    // And an entry with no `h` at all still lands.
    let entry = pulse_entry(
        &owner,
        &coordinate,
        "note",
        "Channel-less entries are unaffected.",
        None,
        None,
    );
    let ok = owner_client.send_event(entry).await.expect("send 44240");
    assert!(ok.accepted, "channel-less entry rejected: {}", ok.message);
}

/// The bridge's request-shape guards: an unscoped, multi-project, mixed-kind,
/// or non-canonical 44240 query is a 400 on both `/query` and `/count`.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_unscoped_and_multi_project_pulse_queries_fail_closed() {
    let owner = Keys::generate();
    let d_tag = unique("pulse-shape");
    let coordinate = project_coordinate(&owner, &d_tag);
    let other = project_coordinate(&owner, &unique("pulse-other"));
    let http = http_client();
    let pubkey_hex = owner.public_key().to_hex();
    let base = relay_http_url();

    let bad_shapes: Vec<(&str, Value)> = vec![
        (
            "no #a at all",
            json!([{ "kinds": [PULSE_ENTRY_KIND], "limit": 10 }]),
        ),
        (
            "empty #a",
            json!([{ "kinds": [PULSE_ENTRY_KIND], "#a": [] }]),
        ),
        (
            "two projects",
            json!([{ "kinds": [PULSE_ENTRY_KIND], "#a": [coordinate, other] }]),
        ),
        (
            "mixed kinds",
            json!([{ "kinds": [PULSE_ENTRY_KIND, 44223], "#a": [coordinate] }]),
        ),
        (
            "non-canonical coordinate",
            json!([{
                "kinds": [PULSE_ENTRY_KIND],
                "#a": [format!("{PROJECT_KIND}:{}:{d_tag}", pubkey_hex.to_uppercase())],
            }]),
        ),
    ];

    for (label, body) in bad_shapes {
        let (status, response) = post_query_raw(&http, &base, &pubkey_hex, &body).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "/query must reject {label}: {response}"
        );
        let (status, response) = post_count_raw(&http, &base, &pubkey_hex, &body).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "/count must reject {label}: {response}"
        );
    }

    // The canonical single-project shape is accepted (200, empty for a project
    // that does not exist — an unknown coordinate is not an error).
    let (status, response) =
        post_query_raw(&http, &base, &pubkey_hex, &pulse_query_body(&coordinate)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the canonical shape must be accepted: {response}"
    );
}

/// An inadmissible private-project read is an empty 200 — never a 403, which
/// would tell a stranger the project exists.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_inadmissible_pulse_read_is_empty_200_never_403() {
    let owner = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("pulse-403");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[]);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);
    seed_entry(&mut owner_client, &owner, &coordinate, "Secret plan.").await;

    let http = http_client();
    let stranger_hex = stranger.public_key().to_hex();
    let body = pulse_query_body(&coordinate);

    let (status, response) = post_query_raw(&http, &relay_http_url(), &stranger_hex, &body).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an inadmissible pulse read must be a 200, got body {response}"
    );
    assert_eq!(
        response.as_array().map(Vec::len),
        Some(0),
        "an inadmissible pulse read must return no events"
    );

    let (status, response) = post_count_raw(&http, &relay_http_url(), &stranger_hex, &body).await;
    assert_eq!(status, StatusCode::OK, "count must be a 200: {response}");
    assert_eq!(response["count"].as_u64(), Some(0));

    // A coordinate that names no project at all produces the *same* answer, so
    // the response cannot be used to distinguish "private" from "absent".
    let unknown = project_coordinate(&owner, &unique("pulse-absent"));
    let (status, response) = post_query_raw(
        &http,
        &relay_http_url(),
        &stranger_hex,
        &pulse_query_body(&unknown),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response.as_array().map(Vec::len), Some(0));
}

/// Supersession is a fold-time claim. The relay stores the superseding entry
/// and never touches its target: both events remain readable, byte-identical.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_supersession_does_not_mutate_the_old_event() {
    let owner = Keys::generate();
    let d_tag = unique("pulse-supersede");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("public"), &[]);
    let ok = client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "public project rejected: {}", ok.message);

    let original = pulse_entry(
        &owner,
        &coordinate,
        "plan",
        "First plan: touch pool.rs.",
        None,
        None,
    );
    let original_id = original.id;
    let original_content = original.content.clone();
    let original_created_at = original.created_at;
    let ok = client.send_event(original).await.expect("send original");
    assert!(ok.accepted, "original entry rejected: {}", ok.message);

    let replacement = pulse_entry(
        &owner,
        &coordinate,
        "plan",
        "Revised plan: pool.rs plus session.rs.",
        None,
        Some(&original_id.to_hex()),
    );
    let replacement_id = replacement.id;
    let ok = client
        .send_event(replacement)
        .await
        .expect("send replacement");
    assert!(ok.accepted, "superseding entry rejected: {}", ok.message);

    let seen = query(&mut client, "supersede", pulse_filter(&coordinate)).await;
    assert_eq!(
        seen.len(),
        2,
        "supersession must not delete or replace the target"
    );
    let stored_original = seen
        .iter()
        .find(|e| e.id == original_id)
        .expect("the superseded entry must still be readable");
    assert_eq!(
        stored_original.content, original_content,
        "the superseded entry's content must be untouched"
    );
    assert_eq!(
        stored_original.created_at, original_created_at,
        "the superseded entry's created_at must be untouched"
    );
    assert!(seen.iter().any(|e| e.id == replacement_id));
}

/// Two communities on one relay process: an entry published against community
/// A's project is not retrievable from community B, and B cannot publish
/// against A's coordinate either (the project does not exist in B).
#[tokio::test]
#[ignore = "requires a two-host relay (RELAY_URL_A / RELAY_URL_B)"]
async fn test_pulse_entries_do_not_cross_communities() {
    let owner = Keys::generate();
    let d_tag = unique("pulse-xcomm");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut client_a = BuzzTestClient::connect(&relay_url_a(), &owner)
        .await
        .expect("community A connect");
    let head = project_event(&owner, &d_tag, Some("public"), &[]);
    let ok = client_a.send_event(head).await.expect("send project on A");
    assert!(ok.accepted, "project rejected on A: {}", ok.message);
    let entry_id = seed_entry(&mut client_a, &owner, &coordinate, "A-side plan.").await;

    let seen = query(&mut client_a, "comm-a", pulse_filter(&coordinate)).await;
    assert!(
        seen.iter().any(|e| e.id == entry_id),
        "community A must read its own entry"
    );

    // The same identity, the same coordinate, the other community.
    let mut client_b = BuzzTestClient::connect(&relay_url_b(), &owner)
        .await
        .expect("community B connect");

    // Positive control first: community B is a working community for this
    // identity, so the empty result below is isolation and not a dead host.
    let b_tag = unique("pulse-xcomm-b");
    let b_coordinate = project_coordinate(&owner, &b_tag);
    let head_b = project_event(&owner, &b_tag, Some("public"), &[]);
    let ok = client_b
        .send_event(head_b)
        .await
        .expect("send project on B");
    assert!(ok.accepted, "project rejected on B: {}", ok.message);
    let b_entry_id = seed_entry(&mut client_b, &owner, &b_coordinate, "B-side plan.").await;
    let seen = query(&mut client_b, "comm-b-own", pulse_filter(&b_coordinate)).await;
    assert!(
        seen.iter().any(|e| e.id == b_entry_id),
        "control: community B must read its own entry"
    );
    // …and A cannot see B's, so the isolation is symmetric.
    let seen = query(&mut client_a, "comm-a-of-b", pulse_filter(&b_coordinate)).await;
    assert!(
        seen.is_empty(),
        "community A must not retrieve community B's pulse entries"
    );

    let seen = query(&mut client_b, "comm-b", pulse_filter(&coordinate)).await;
    assert!(
        seen.is_empty(),
        "community B must not retrieve community A's pulse entries"
    );

    let http = http_client();
    let (status, response) = post_query_raw(
        &http,
        &to_http(&relay_url_b()),
        &owner.public_key().to_hex(),
        &pulse_query_body(&coordinate),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "community B /query: {response}");
    assert_eq!(
        response.as_array().map(Vec::len),
        Some(0),
        "community B's HTTP bridge must not retrieve community A's entries"
    );

    // The project head does not exist in B, so a write there is refused —
    // proof the existence probe is community-scoped.
    let entry = pulse_entry(
        &owner,
        &coordinate,
        "note",
        "Should not land in the other community.",
        None,
        None,
    );
    let rejected = client_b.send_event(entry).await.expect("send 44240 on B");
    assert!(
        !rejected.accepted,
        "community B must refuse an entry for community A's project"
    );
    assert!(
        rejected.message.contains("unknown project coordinate"),
        "unexpected rejection on B: {}",
        rejected.message
    );
}

/// The bridge's channel-window read model (`top_level: true` on `POST /query`)
/// is gated per row like every other read surface.
///
/// This is a *separate* surface from the `#a`-scoped bridge query the tests
/// above cover: a window filter is dispatched before the catchall loop, so it
/// does not inherit the per-event gate or the 44240 pre-query skip that path
/// applies. An `h`-tagged 44240 is stored with a channel and gets no
/// thread-metadata row, so it satisfies the window's top-level predicate and
/// the window SQL — which scopes on community, channel, deletion and cursor,
/// never on `#a` — would otherwise hand a private project's entries to any
/// member of the channel.
///
/// Both attack shapes are asserted: `kinds:[44240]` and a kindless window
/// (the shape a plain GUI page fetch sends).
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_channel_window_withholds_private_project_entries() {
    let owner = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("pulse-window");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[]);
    let ok = owner_client.send_event(head).await.expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    // An *open* channel: the stranger is admitted here, so the only thing
    // that can withhold the entry below is the project gate. "open" is the
    // wire value — `ChannelVisibility` is `open | private`
    // (crates/buzz-core/src/channel.rs:22-26); "public" is rejected at
    // ingest with `invalid visibility` (ingest.rs:4083).
    let channel = create_channel(&mut owner_client, &owner, "open").await;

    let entry = pulse_entry(
        &owner,
        &coordinate,
        "plan",
        "Private project claim, posted into a public channel.",
        Some(&channel),
        None,
    );
    let entry_id = entry.id;
    let ok = owner_client.send_event(entry).await.expect("send 44240");
    assert!(ok.accepted, "h-tagged entry rejected: {}", ok.message);

    // A plain message in the same channel is the positive control: it must come
    // back in the stranger's window, so an absent 44240 is the read gate and
    // not an empty or inaccessible window.
    let message = EventBuilder::new(Kind::Custom(MESSAGE_KIND), "window control")
        .tags(vec![Tag::parse(["h", &channel]).unwrap()])
        .sign_with_keys(&owner)
        .unwrap();
    let message_id = message.id;
    let ok = owner_client.send_event(message).await.expect("send 9");
    assert!(ok.accepted, "control message rejected: {}", ok.message);

    let http = http_client();
    let base = relay_http_url();
    let ids = |body: &Value| -> Vec<String> {
        body.as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };

    // Every way of aiming a *channel* window at a 44240 without naming the
    // project fails closed at the filter, before any per-row gate runs. Assert
    // each refusal by its own status: these are stronger guarantees than
    // withholding, and asserting a 200-with-no-entry would be asserting an
    // outcome the relay will not produce.
    //
    // 1. Kindless: the p-gate refuses it (bridge.rs:1035).
    let (status, response) = post_query_raw(
        &http,
        &base,
        &stranger.public_key().to_hex(),
        &json!([{ "#h": [channel], "top_level": true, "limit": 50 }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a kindless window must be refused by the p-gate, not served: {response}"
    );

    // 2. Mixed with another kind: refused 400, so a 44240 can never ride back
    //    alongside ordinary messages where a per-row miss would leak it.
    let (status, response) = post_query_raw(
        &http,
        &base,
        &stranger.public_key().to_hex(),
        &json!([{
            "kinds": [MESSAGE_KIND, PULSE_ENTRY_KIND],
            "#h": [channel],
            "top_level": true,
            "limit": 50,
        }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a window mixing 44240 with another kind must fail closed: {response}"
    );

    // 3. 44240-only but scoped by channel alone: refused 400, because a Pulse
    //    query must name exactly one project coordinate in `#a`. A channel
    //    window therefore cannot enumerate a project's Pulse at all — the
    //    reader has to name the project, at which point the project gate
    //    below is the thing standing in the way.
    let (status, response) = post_query_raw(
        &http,
        &base,
        &stranger.public_key().to_hex(),
        &json!([{
            "kinds": [PULSE_ENTRY_KIND],
            "#h": [channel],
            "top_level": true,
            "limit": 50,
        }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a 44240 window scoped only by channel must fail closed: {response}"
    );

    // 3b. An *explicitly empty* `kinds` array. This is the shape that used to
    //     be exploitable: `"kinds":[]` deserializes to `Some(∅)`, not `None`,
    //     so it slipped past every `.any(...)` predicate (an empty iterator
    //     yields `false`) — the p-gate did not refuse it, the Pulse shape guard
    //     did not see a 44240, the reader's hidden-project set was never
    //     resolved, and the window SQL omits its kind clause for an empty
    //     slice. The result was a 200 carrying the private project's entry.
    //     The window path now treats an empty kind list the way the catchall
    //     SQL always has — matches nothing.
    let (status, response) = post_query_raw(
        &http,
        &base,
        &stranger.public_key().to_hex(),
        &json!([{
            "kinds": [],
            "#h": [channel],
            "top_level": true,
            "limit": 50,
        }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an empty-kinds window is a client mistake, not an auth error: {response}"
    );
    assert!(
        !ids(&response).contains(&entry_id.to_hex()),
        "an empty `kinds` array must not act as a wildcard that serves a \
         private project's entry to a non-member: {response}"
    );
    assert!(
        ids(&response).is_empty(),
        "an empty `kinds` array matches nothing, exactly as the catchall \
         query path treats it: {response}"
    );

    // 4. The one shape that *is* served — coordinate-scoped, channel-scoped —
    //    reaches the per-row gate, and it withholds the private project's
    //    entry from the stranger.
    let pulse_window = json!([{
        "kinds": [PULSE_ENTRY_KIND],
        "#a": [coordinate],
        "#h": [channel],
        "top_level": true,
        "limit": 50,
    }]);
    let (status, response) =
        post_query_raw(&http, &base, &stranger.public_key().to_hex(), &pulse_window).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "coordinate-scoped window: {response}"
    );
    assert!(
        !ids(&response).contains(&entry_id.to_hex()),
        "the coordinate-scoped window must withhold a private project's entry \
         from a non-member: {response}"
    );

    // Control 1: the stranger *can* read this channel — their kind-9 window
    // returns the ordinary message. So the entry's absence above is the
    // project gate, not an inaccessible channel.
    let (status, response) = post_query_raw(
        &http,
        &base,
        &stranger.public_key().to_hex(),
        &json!([{
            "kinds": [MESSAGE_KIND],
            "#h": [channel],
            "top_level": true,
            "limit": 50,
        }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "stranger control window: {response}"
    );
    assert!(
        ids(&response).contains(&message_id.to_hex()),
        "control: the stranger's window must return the channel's ordinary message: {response}"
    );

    // Control 2: the owner's identical 44240-only window *does* carry the
    // entry, so the assertion above is the per-row gate and not the window
    // path dropping 44240s wholesale.
    let (status, response) =
        post_query_raw(&http, &base, &owner.public_key().to_hex(), &pulse_window).await;
    assert_eq!(status, StatusCode::OK, "owner window: {response}");
    assert!(
        ids(&response).contains(&entry_id.to_hex()),
        "control: the project owner's window must return their own entry: {response}"
    );
}
