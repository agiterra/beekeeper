//! End-to-end tests for NIP-TD project to-do ops (kind:44248).
//!
//! The buzz-core unit tests pin the envelope and the fold, the ingest tests
//! pin write admission, and the buzz-db tests pin the SQL pushdown. These
//! tests prove the gate and the query shape are wired into the *live*
//! surfaces, which no in-process test can reach:
//!
//! - a private project's ops are withheld from strangers over **WS REQ**,
//!   **HTTP `POST /query`**, **HTTP `POST /count`**, and **live fan-out**;
//! - a project *writer* publishes and a read-only *viewer* does not, and the
//!   HTTP refusal is a **403** (CLI exit 3), not a 400;
//! - an op for an unknown coordinate, an op carrying `h`, an op with an
//!   unknown content key, and an op whose tags disagree with its content are
//!   all refused at ingest;
//! - the bridge accepts `{"kinds":[44240,44248],"#a":[…]}` (one gate, one
//!   coordinate) and refuses an unscoped or mixed-with-other-kinds shape
//!   with **400**;
//! - two writers' ops read back from the relay fold to one list with the
//!   state the contract promises — the same fold `bee todos` and Desktop use;
//! - a community cannot retrieve another community's coordinate.
//!
//! See `docs/nips/NIP-TD.md` for the normative contract.
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p beekeeper-test-client --test e2e_project_todos -- --ignored
//! ```
//!
//! The cross-community test uses `RELAY_URL_A` / `RELAY_URL_B` with the same
//! two-host defaults as `e2e_pulse.rs`.

use std::time::Duration;

use beekeeper_core::project_todo_fold::{fold_project_todos, TodoFoldEvent};
use beekeeper_test_client::BuzzTestClient;
use beekeeper_ws_client::RelayMessage;
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PROJECT_KIND: u16 = 30621;
const PULSE_ENTRY_KIND: u16 = 44240;
const TODO_OP_KIND: u16 = 44248;
const TODO_TAG_VERSION: &str = "td1-1";
const TODO_SCHEMA: &str = "buzz-project-todo/v1";

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_string())
}

fn relay_url_a() -> String {
    std::env::var("RELAY_URL_A").unwrap_or_else(|_| relay_url())
}

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
    format!("e2e-todo-{name}-{}", uuid::Uuid::new_v4())
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn project_coordinate(owner: &Keys, d_tag: &str) -> String {
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

/// A fresh 32-hex list or item id.
fn todo_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

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

/// Build a kind:44248 op from a content object. `content` must carry `op`
/// and `listId` (and `itemId` for item ops); the tags are derived from it,
/// then `extra_tags` are appended and `created_at` applied when given.
fn todo_op(
    keys: &Keys,
    coordinate: &str,
    mut content: Value,
    extra_tags: &[[&str; 2]],
    created_at: Option<u64>,
) -> nostr::Event {
    todo_op_vis(
        keys,
        coordinate,
        content.take(),
        extra_tags,
        created_at,
        "project",
    )
}

/// [`todo_op`] for a list of the given visibility (`project` | `personal`):
/// the `td-vis` tag on every op, and `visibility` in `list.create` content.
fn todo_op_vis(
    keys: &Keys,
    coordinate: &str,
    mut content: Value,
    extra_tags: &[[&str; 2]],
    created_at: Option<u64>,
    visibility: &str,
) -> nostr::Event {
    content["schema"] = json!(TODO_SCHEMA);
    let op = content["op"].as_str().unwrap().to_owned();
    if op == "list.create" && content.get("visibility").is_none() {
        content["visibility"] = json!(visibility);
    }
    let list_id = content["listId"].as_str().unwrap().to_owned();
    let mut tags = vec![
        Tag::parse(["a", coordinate]).unwrap(),
        Tag::parse(["td-v", TODO_TAG_VERSION]).unwrap(),
        Tag::parse(["td-op", &op]).unwrap(),
        Tag::parse(["td-list", &list_id]).unwrap(),
        Tag::parse(["td-vis", visibility]).unwrap(),
    ];
    if let Some(item_id) = content["itemId"].as_str() {
        tags.push(Tag::parse(["td-item", item_id]).unwrap());
    }
    for [k, v] in extra_tags {
        tags.push(Tag::parse([*k, *v]).unwrap());
    }
    let mut builder = EventBuilder::new(Kind::Custom(TODO_OP_KIND), content.to_string()).tags(tags);
    if let Some(ts) = created_at {
        builder = builder.custom_created_at(nostr::Timestamp::from(ts));
    }
    builder.sign_with_keys(keys).unwrap()
}

fn list_create(keys: &Keys, coordinate: &str, list_id: &str, title: &str) -> nostr::Event {
    todo_op(
        keys,
        coordinate,
        json!({ "op": "list.create", "listId": list_id, "title": title }),
        &[],
        None,
    )
}

fn item_add(
    keys: &Keys,
    coordinate: &str,
    list_id: &str,
    item_id: &str,
    text: &str,
    rank: &str,
) -> nostr::Event {
    todo_op(
        keys,
        coordinate,
        json!({ "op": "item.add", "listId": list_id, "itemId": item_id, "text": text, "rank": rank }),
        &[],
        None,
    )
}

fn todo_filter(coordinate: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(TODO_OP_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::A), [coordinate])
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

async fn post_raw(
    client: &Client,
    base_http: &str,
    path: &str,
    pubkey_hex: &str,
    body: &Value,
) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{base_http}{path}"))
        .header("X-Pubkey", pubkey_hex)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("post {path}: {e}"));
    let status = resp.status();
    let body = resp.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

fn todo_query_body(coordinate: &str) -> Value {
    json!([{ "kinds": [TODO_OP_KIND], "#a": [coordinate], "limit": 50 }])
}

async fn send_ok(client: &mut BuzzTestClient, event: nostr::Event, what: &str) -> nostr::EventId {
    let id = event.id;
    let ok = client.send_event(event).await.expect("send");
    assert!(ok.accepted, "{what} rejected: {}", ok.message);
    id
}

async fn send_refused(client: &mut BuzzTestClient, event: nostr::Event, needle: &str) {
    let rejected = client.send_event(event).await.expect("send");
    assert!(
        !rejected.accepted,
        "expected refusal containing {needle:?}, got accepted"
    );
    assert!(
        rejected.message.contains(needle),
        "expected refusal containing {needle:?}, got: {}",
        rejected.message
    );
}

/// A private project's ops are visible to its members and withheld from a
/// stranger across WS REQ, HTTP `/query`, and HTTP `/count`.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_todo_ops_visible_to_members_hidden_from_stranger() {
    let owner = Keys::generate();
    let viewer = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("todo-private");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let head = project_event(&owner, &d_tag, Some("private"), &[(&viewer, "viewer")]);
    send_ok(&mut owner_client, head, "private project").await;

    let list_id = todo_id();
    let op_id = send_ok(
        &mut owner_client,
        list_create(&owner, &coordinate, &list_id, "Launch"),
        "list.create",
    )
    .await;

    let seen = query(&mut owner_client, "owner", todo_filter(&coordinate)).await;
    assert_eq!(seen.len(), 1, "author must read their own op");
    assert_eq!(seen[0].id, op_id);

    let mut viewer_client = BuzzTestClient::connect(&relay_url(), &viewer)
        .await
        .expect("viewer connect");
    let seen = query(&mut viewer_client, "viewer", todo_filter(&coordinate)).await;
    assert_eq!(seen.len(), 1, "invited viewer must read the op");

    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(&mut stranger_client, "stranger", todo_filter(&coordinate)).await;
    assert!(
        seen.is_empty(),
        "WS REQ must withhold a private project's ops"
    );
    let seen = query(
        &mut stranger_client,
        "stranger-ids",
        Filter::new().id(op_id),
    )
    .await;
    assert!(seen.is_empty(), "a known event id must not bypass the gate");

    let http = http_client();
    let stranger_hex = stranger.public_key().to_hex();
    let (status, rows) = post_raw(
        &http,
        &relay_http_url(),
        "/query",
        &stranger_hex,
        &todo_query_body(&coordinate),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a withheld read is an empty 200, never 403: {rows}"
    );
    assert_eq!(
        rows.as_array().map(Vec::len),
        Some(0),
        "HTTP /query must withhold ops"
    );
    let (status, count) = post_raw(
        &http,
        &relay_http_url(),
        "/count",
        &stranger_hex,
        &todo_query_body(&coordinate),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{count}");
    assert_eq!(
        count["count"].as_u64(),
        Some(0),
        "HTTP /count must not leak existence"
    );

    let (status, rows) = post_raw(
        &http,
        &relay_http_url(),
        "/query",
        &viewer.public_key().to_hex(),
        &todo_query_body(&coordinate),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rows}");
    assert_eq!(
        rows.as_array().map(Vec::len),
        Some(1),
        "viewer's HTTP /query returns the op"
    );
}

/// Live fan-out: a member receives an op as it is published, a stranger never
/// does.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_todo_op_fanout_filtered_live() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let d_tag = unique("todo-live");
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
    send_ok(&mut owner_client, head, "private project").await;

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");

    let member_sid = sub_id("live-member");
    let stranger_sid = sub_id("live-stranger");
    let live_filter = todo_filter(&coordinate);
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

    let list_id = todo_id();
    let op_id = send_ok(
        &mut owner_client,
        list_create(&owner, &coordinate, &list_id, "Live"),
        "list.create",
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut member_got = false;
    while tokio::time::Instant::now() < deadline {
        match member_client.recv_event(Duration::from_secs(2)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == member_sid && event.id == op_id => {
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
                    event.id, op_id,
                    "stranger must never receive a private op live"
                );
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

/// A collaborator publishes; a viewer is refused (403 over HTTP) and still
/// reads.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_private_project_writer_publishes_viewer_cannot() {
    let owner = Keys::generate();
    let writer = Keys::generate();
    let viewer = Keys::generate();
    let d_tag = unique("todo-roles");
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
    send_ok(&mut owner_client, head, "private project").await;

    let list_id = todo_id();
    let mut writer_client = BuzzTestClient::connect(&relay_url(), &writer)
        .await
        .expect("writer connect");
    send_ok(
        &mut writer_client,
        list_create(&writer, &coordinate, &list_id, "Writer's list"),
        "collaborator list.create",
    )
    .await;

    let mut viewer_client = BuzzTestClient::connect(&relay_url(), &viewer)
        .await
        .expect("viewer connect");
    send_refused(
        &mut viewer_client,
        item_add(
            &viewer,
            &coordinate,
            &list_id,
            &todo_id(),
            "viewer add",
            "a0",
        ),
        "restricted:",
    )
    .await;

    let http = http_client();
    let refused = item_add(
        &viewer,
        &coordinate,
        &list_id,
        &todo_id(),
        "viewer http",
        "a0",
    );
    let (status, body) = post_raw(
        &http,
        &relay_http_url(),
        "/events",
        &viewer.public_key().to_hex(),
        &serde_json::to_value(&refused).expect("serialize"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a refused todo write must answer 403 (CLI exit 3), not 400: {body}"
    );

    let seen = query(&mut viewer_client, "viewer-read", todo_filter(&coordinate)).await;
    assert_eq!(seen.len(), 1, "a viewer reads the list it cannot write");
}

/// Ingest refusals: unknown coordinate, `h` tag, unknown content key,
/// tag/content disagreement.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_malformed_and_unscoped_ops_are_refused() {
    let owner = Keys::generate();
    let d_tag = unique("todo-shape");
    let coordinate = project_coordinate(&owner, &d_tag);
    let mut client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("connect");

    let never = project_coordinate(&owner, &unique("never-created"));
    send_refused(
        &mut client,
        list_create(&owner, &never, &todo_id(), "ghost"),
        "unknown project coordinate",
    )
    .await;

    send_ok(
        &mut client,
        project_event(&owner, &d_tag, Some("public"), &[]),
        "public project",
    )
    .await;
    let list_id = todo_id();
    send_ok(
        &mut client,
        list_create(&owner, &coordinate, &list_id, "Shapes"),
        "list.create",
    )
    .await;

    let channel = uuid::Uuid::new_v4().to_string();
    send_refused(
        &mut client,
        todo_op(
            &owner,
            &coordinate,
            json!({ "op": "item.add", "listId": list_id, "itemId": todo_id(), "text": "h", "rank": "a0" }),
            &[["h", &channel]],
            None,
        ),
        "must not carry an h tag",
    )
    .await;

    send_refused(
        &mut client,
        todo_op(
            &owner,
            &coordinate,
            json!({ "op": "item.add", "listId": list_id, "itemId": todo_id(), "text": "k", "rank": "a0", "priority": "high" }),
            &[],
            None,
        ),
        "unsupported field",
    )
    .await;

    // Tags disagree with content: td-op says item.text, content says item.add.
    let item_id = todo_id();
    let content = json!({ "schema": TODO_SCHEMA, "op": "item.add", "listId": list_id, "itemId": item_id, "text": "m", "rank": "a0" });
    let mismatched = EventBuilder::new(Kind::Custom(TODO_OP_KIND), content.to_string())
        .tags(vec![
            Tag::parse(["a", &coordinate]).unwrap(),
            Tag::parse(["td-v", TODO_TAG_VERSION]).unwrap(),
            Tag::parse(["td-op", "item.text"]).unwrap(),
            Tag::parse(["td-list", &list_id]).unwrap(),
            Tag::parse(["td-vis", "project"]).unwrap(),
            Tag::parse(["td-item", &item_id]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    send_refused(&mut client, mismatched, "does not match content op").await;

    // A rank ending in 0 is refused at ingest, not just by the fold.
    send_refused(
        &mut client,
        todo_op(
            &owner,
            &coordinate,
            json!({ "op": "item.add", "listId": list_id, "itemId": todo_id(), "text": "r", "rank": "a0V0" }),
            &[],
            None,
        ),
        "rank",
    )
    .await;
}

/// Bridge request shapes: Pulse and to-do ops may share one `#a` filter;
/// an unscoped or other-kind-mixed filter is a 400.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_bridge_shape_rules_for_project_scoped_kinds() {
    let owner = Keys::generate();
    let d_tag = unique("todo-bridge");
    let coordinate = project_coordinate(&owner, &d_tag);
    let mut client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("connect");
    send_ok(
        &mut client,
        project_event(&owner, &d_tag, Some("public"), &[]),
        "public project",
    )
    .await;
    let list_id = todo_id();
    let op_id = send_ok(
        &mut client,
        list_create(&owner, &coordinate, &list_id, "Bridge"),
        "list.create",
    )
    .await;

    let http = http_client();
    let me = owner.public_key().to_hex();
    let base = relay_http_url();

    let (status, rows) = post_raw(
        &http,
        &base,
        "/query",
        &me,
        &json!([{ "kinds": [PULSE_ENTRY_KIND, TODO_OP_KIND], "#a": [coordinate], "limit": 50 }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "mixed 44240+44248 must be accepted: {rows}"
    );
    assert!(
        rows.as_array()
            .map(|r| r.iter().any(|e| e["id"].as_str() == Some(&op_id.to_hex())))
            .unwrap_or(false),
        "the op must come back through the shared filter: {rows}"
    );

    let (status, body) = post_raw(
        &http,
        &base,
        "/query",
        &me,
        &json!([{ "kinds": [TODO_OP_KIND], "limit": 50 }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unscoped 44248 must be 400: {body}"
    );

    let (status, body) = post_raw(
        &http,
        &base,
        "/query",
        &me,
        &json!([{ "kinds": [TODO_OP_KIND, 9], "#a": [coordinate], "limit": 50 }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "44248 mixed with kind 9 must be 400: {body}"
    );

    let (status, body) = post_raw(
        &http,
        &base,
        "/count",
        &me,
        &json!([{ "kinds": [TODO_OP_KIND], "#a": [coordinate, coordinate.replace(&d_tag, "other")] }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "multi-project #a must be 400: {body}"
    );
}

/// Two writers' ops, read back by a third member, fold to the list the
/// contract promises: earliest create wins, the collaborator's done lands
/// in `completed`, a rank move reorders, a remove disappears.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_two_writers_fold_to_one_list() {
    let owner = Keys::generate();
    let collaborator = Keys::generate();
    let viewer = Keys::generate();
    let d_tag = unique("todo-fold");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    send_ok(
        &mut owner_client,
        project_event(
            &owner,
            &d_tag,
            Some("private"),
            &[(&collaborator, "collaborator"), (&viewer, "viewer")],
        ),
        "private project",
    )
    .await;
    let mut collab_client = BuzzTestClient::connect(&relay_url(), &collaborator)
        .await
        .expect("collaborator connect");

    let list_id = todo_id();
    let (one, two, three) = (todo_id(), todo_id(), todo_id());
    let base = nostr::Timestamp::now().as_secs();
    let at = |offset: u64| Some(base + offset);
    let op = |keys: &Keys, content: Value, created_at: Option<u64>| {
        todo_op(keys, &coordinate, content, &[], created_at)
    };

    send_ok(
        &mut owner_client,
        op(
            &owner,
            json!({ "op": "list.create", "listId": list_id, "title": "Shared" }),
            at(0),
        ),
        "create",
    )
    .await;
    send_ok(
        &mut owner_client,
        op(&owner, json!({ "op": "item.add", "listId": list_id, "itemId": one, "text": "one", "rank": "a0" }), at(1)),
        "add one",
    )
    .await;
    send_ok(
        &mut collab_client,
        op(&collaborator, json!({ "op": "item.add", "listId": list_id, "itemId": two, "text": "two", "rank": "a1" }), at(2)),
        "add two",
    )
    .await;
    send_ok(
        &mut collab_client,
        op(&collaborator, json!({ "op": "item.add", "listId": list_id, "itemId": three, "text": "three", "rank": "a2" }), at(3)),
        "add three",
    )
    .await;
    // Collaborator finishes one; owner assigns and dates two; collaborator
    // moves three to the top; owner removes... nothing — instead the owner
    // retitles, and the collaborator edits one's text after it was done.
    send_ok(
        &mut collab_client,
        op(
            &collaborator,
            json!({ "op": "item.done", "listId": list_id, "itemId": one, "done": true }),
            at(4),
        ),
        "done one",
    )
    .await;
    send_ok(
        &mut owner_client,
        op(&owner, json!({ "op": "item.assignee", "listId": list_id, "itemId": two, "assignee": collaborator.public_key().to_hex() }), at(5)),
        "assign two",
    )
    .await;
    send_ok(
        &mut owner_client,
        op(
            &owner,
            json!({ "op": "item.due", "listId": list_id, "itemId": two, "due": "2026-12-31" }),
            at(6),
        ),
        "due two",
    )
    .await;
    send_ok(
        &mut collab_client,
        op(
            &collaborator,
            json!({ "op": "item.rank", "listId": list_id, "itemId": three, "rank": "Zz" }),
            at(7),
        ),
        "move three first",
    )
    .await;
    send_ok(
        &mut owner_client,
        op(
            &owner,
            json!({ "op": "list.title", "listId": list_id, "title": "Shared v2" }),
            at(8),
        ),
        "retitle",
    )
    .await;
    send_ok(
        &mut collab_client,
        op(
            &collaborator,
            json!({ "op": "item.text", "listId": list_id, "itemId": one, "text": "one (edited)" }),
            at(9),
        ),
        "edit one",
    )
    .await;
    // A duplicate create for the same list, later, must not retitle it.
    send_ok(
        &mut collab_client,
        op(
            &collaborator,
            json!({ "op": "list.create", "listId": list_id, "title": "Hijack" }),
            at(10),
        ),
        "duplicate create",
    )
    .await;

    let mut viewer_client = BuzzTestClient::connect(&relay_url(), &viewer)
        .await
        .expect("viewer connect");
    let events = query(&mut viewer_client, "viewer-fold", todo_filter(&coordinate)).await;
    assert_eq!(events.len(), 11, "the viewer reads every op");
    let inputs: Vec<TodoFoldEvent> = events.iter().map(TodoFoldEvent::from).collect();
    let digest = fold_project_todos(&coordinate, &inputs);

    assert_eq!(
        digest.ignored, 1,
        "the duplicate create is the one ignored op"
    );
    assert_eq!(digest.lists.len(), 1);
    let list = &digest.lists[0];
    assert_eq!(list.id, list_id);
    assert_eq!(list.title, "Shared v2");
    assert_eq!(list.created_by, owner.public_key().to_hex());
    assert_eq!(
        list.updated_at,
        base + 9,
        "the ignored duplicate does not bump updatedAt"
    );
    let open: Vec<&str> = list.open.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        open,
        vec![three.as_str(), two.as_str()],
        "three moved to the top"
    );
    assert_eq!(
        list.open[1].assignee.as_deref(),
        Some(collaborator.public_key().to_hex().as_str())
    );
    assert_eq!(list.open[1].due.as_deref(), Some("2026-12-31"));
    assert_eq!(list.completed.len(), 1);
    assert_eq!(list.completed[0].id, one);
    assert_eq!(list.completed[0].text, "one (edited)");
    assert_eq!(list.completed[0].completed_at, Some(base + 4));
    assert_eq!(
        list.completed[0].completed_by.as_deref(),
        Some(collaborator.public_key().to_hex().as_str())
    );
}

/// A personal list is its author's alone: another member of the same
/// project — even the owner — reads none of its ops over WS REQ, `/query`
/// or `/count`, receives none live, and the author still reads everything.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_personal_list_is_withheld_from_other_members() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let d_tag = unique("todo-personal");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    send_ok(
        &mut owner_client,
        project_event(&owner, &d_tag, Some("public"), &[(&member, "collaborator")]),
        "public project",
    )
    .await;
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");

    // The owner subscribes live on its own connection before the member
    // writes anything; the connection does nothing else until the receive
    // loop, since a REQ drained on the same socket would swallow the live
    // events of every other subscription.
    let mut owner_live = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner live connect");
    let owner_sid = sub_id("personal-owner-live");
    owner_live
        .subscribe(&owner_sid, vec![todo_filter(&coordinate)])
        .await
        .expect("owner subscribe");
    owner_live
        .collect_until_eose(&owner_sid, Duration::from_secs(10))
        .await
        .expect("owner eose");

    // The member keeps one personal list and one project list.
    let personal = todo_id();
    let shared = todo_id();
    let personal_create = send_ok(
        &mut member_client,
        todo_op_vis(
            &member,
            &coordinate,
            json!({ "op": "list.create", "listId": personal, "title": "Mine" }),
            &[],
            None,
            "personal",
        ),
        "personal create",
    )
    .await;
    send_ok(
        &mut member_client,
        todo_op_vis(
            &member,
            &coordinate,
            json!({ "op": "item.add", "listId": personal, "itemId": todo_id(), "text": "secret", "rank": "a0" }),
            &[],
            None,
            "personal",
        ),
        "personal add",
    )
    .await;
    let shared_create = send_ok(
        &mut member_client,
        list_create(&member, &coordinate, &shared, "Ours"),
        "shared create",
    )
    .await;

    // The author reads all three.
    let seen = query(
        &mut member_client,
        "personal-author",
        todo_filter(&coordinate),
    )
    .await;
    assert_eq!(
        seen.len(),
        3,
        "the author reads personal and shared ops alike"
    );

    // The project owner reads only the shared one, on every surface.
    let seen = query(
        &mut owner_client,
        "personal-owner",
        todo_filter(&coordinate),
    )
    .await;
    let ids: Vec<_> = seen.iter().map(|e| e.id).collect();
    assert_eq!(
        ids,
        vec![shared_create],
        "WS REQ must withhold another member's personal ops"
    );
    let seen = query(
        &mut owner_client,
        "personal-owner-ids",
        Filter::new().id(personal_create),
    )
    .await;
    assert!(
        seen.is_empty(),
        "a known id must not bypass the personal gate"
    );

    let http = http_client();
    let owner_hex = owner.public_key().to_hex();
    let (status, rows) = post_raw(
        &http,
        &relay_http_url(),
        "/query",
        &owner_hex,
        &todo_query_body(&coordinate),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rows}");
    assert_eq!(
        rows.as_array().map(Vec::len),
        Some(1),
        "HTTP /query must withhold personal ops: {rows}"
    );
    let (status, count) = post_raw(
        &http,
        &relay_http_url(),
        "/count",
        &owner_hex,
        &todo_query_body(&coordinate),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{count}");
    assert_eq!(
        count["count"].as_u64(),
        Some(1),
        "HTTP /count must not count personal ops"
    );

    // Live: the owner's subscription saw the shared create and never the
    // personal ones.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut saw_shared = false;
    while tokio::time::Instant::now() < deadline {
        match owner_live.recv_event(Duration::from_secs(1)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == owner_sid => {
                assert_ne!(
                    event.id, personal_create,
                    "a personal op must never fan out to another member"
                );
                assert!(
                    !event
                        .tags
                        .iter()
                        .any(|t| t.as_slice() == ["td-vis", "personal"]),
                    "a personal op must never fan out to another member"
                );
                if event.id == shared_create {
                    saw_shared = true;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(
        saw_shared,
        "the shared create must still fan out to the owner"
    );

    // A `list.create` whose content disagrees with its tag is refused.
    send_refused(
        &mut member_client,
        todo_op_vis(
            &member,
            &coordinate,
            json!({ "op": "list.create", "listId": todo_id(), "title": "x", "visibility": "personal" }),
            &[],
            None,
            "project",
        ),
        "does not match its td-vis tag",
    )
    .await;
}

/// A community cannot retrieve another community's coordinate, and the
/// existence probe is community-scoped.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_todo_ops_do_not_cross_communities() {
    let owner = Keys::generate();
    let d_tag = unique("todo-xcomm");
    let coordinate = project_coordinate(&owner, &d_tag);

    let mut client_a = BuzzTestClient::connect(&relay_url_a(), &owner)
        .await
        .expect("community A connect");
    send_ok(
        &mut client_a,
        project_event(&owner, &d_tag, Some("public"), &[]),
        "project on A",
    )
    .await;
    let list_id = todo_id();
    let op_id = send_ok(
        &mut client_a,
        list_create(&owner, &coordinate, &list_id, "A-side"),
        "A-side create",
    )
    .await;
    let seen = query(&mut client_a, "comm-a", todo_filter(&coordinate)).await;
    assert!(
        seen.iter().any(|e| e.id == op_id),
        "community A reads its own op"
    );

    let mut client_b = BuzzTestClient::connect(&relay_url_b(), &owner)
        .await
        .expect("community B connect");
    let b_tag = unique("todo-xcomm-b");
    let b_coordinate = project_coordinate(&owner, &b_tag);
    send_ok(
        &mut client_b,
        project_event(&owner, &b_tag, Some("public"), &[]),
        "project on B",
    )
    .await;
    let b_op = send_ok(
        &mut client_b,
        list_create(&owner, &b_coordinate, &todo_id(), "B-side"),
        "B-side create",
    )
    .await;
    let seen = query(&mut client_b, "comm-b-own", todo_filter(&b_coordinate)).await;
    assert!(
        seen.iter().any(|e| e.id == b_op),
        "control: community B reads its own op"
    );

    let seen = query(&mut client_b, "comm-b", todo_filter(&coordinate)).await;
    assert!(
        seen.is_empty(),
        "community B must not retrieve community A's ops"
    );
    let seen = query(&mut client_a, "comm-a-of-b", todo_filter(&b_coordinate)).await;
    assert!(
        seen.is_empty(),
        "community A must not retrieve community B's ops"
    );

    send_refused(
        &mut client_b,
        item_add(&owner, &coordinate, &list_id, &todo_id(), "cross", "a0"),
        "unknown project coordinate",
    )
    .await;
}
