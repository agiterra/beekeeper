//! End-to-end tests for relay plugins (`POST /plugins/{name}/invoke`).
//!
//! The plugin host's own tests prove containment with no relay. These prove
//! the relay wiring that no in-process test reaches: NIP-98 on the real
//! route, tenant binding by host, the project gate decided before any plugin
//! code runs, the reader's visibility applied to every record a plugin reads,
//! and drafts that a member signs as themselves and ingest accepts.
//!
//! # Running
//!
//! The relay must be started with the test registry:
//!
//! ```text
//! scripts/build-plugins.sh
//! BEEKEEPER_PLUGINS_FILE=target/plugins/registry.test.toml <start the relay>
//! RELAY_URL=ws://localhost:<port> RELAY_URL_B=ws://127.0.0.1:<port> \
//!   cargo test -p beekeeper-test-client --test e2e_plugins -- --ignored
//! ```
//!
//! `RELAY_URL_B` must be a host mapped to a *different* community on the same
//! relay; `scripts/planning-plugin-check.sh` seeds both and runs everything.

use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use beekeeper_test_client::BeekeeperTestClient;
use nostr::{EventBuilder, Keys, Kind, Tag};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PROJECT_KIND: u16 = 30621;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_string())
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

fn http() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("http client")
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

/// A NIP-98 `Authorization` header for `method url` with `body`.
fn nip98(keys: &Keys, method: &str, url: &str, body: Option<&[u8]>) -> String {
    let mut tags = vec![
        Tag::parse(["u", url]).unwrap(),
        Tag::parse(["method", method]).unwrap(),
        // Two calls in one second would otherwise sign the same event, which
        // the relay rightly refuses as a replay; real clients add a nonce too.
        Tag::parse(["nonce", &uuid::Uuid::new_v4().simple().to_string()]).unwrap(),
    ];
    if let Some(body) = body {
        tags.push(Tag::parse(["payload", &hex::encode(Sha256::digest(body))]).unwrap());
    }
    let event = EventBuilder::new(Kind::Custom(27235), "")
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap();
    format!(
        "Nostr {}",
        BASE64.encode(serde_json::to_string(&event).unwrap())
    )
}

async fn get_plugins(base_ws: &str, keys: &Keys) -> (StatusCode, Value) {
    let url = format!("{}/plugins", to_http(base_ws));
    let resp = http()
        .get(&url)
        .header("Authorization", nip98(keys, "GET", &url, None))
        .send()
        .await
        .expect("GET /plugins");
    let status = resp.status();
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn invoke_at(
    base_ws: &str,
    keys: &Keys,
    plugin: &str,
    operation: &str,
    project: &str,
    input: Value,
) -> (StatusCode, Value) {
    let url = format!("{}/plugins/{plugin}/invoke", to_http(base_ws));
    let body = serde_json::to_vec(&json!({
        "operation": operation,
        "scope": { "project": project },
        "input": input,
    }))
    .unwrap();
    let resp = http()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", nip98(keys, "POST", &url, Some(&body)))
        .body(body)
        .send()
        .await
        .expect("POST invoke");
    let status = resp.status();
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn invoke(
    keys: &Keys,
    plugin: &str,
    operation: &str,
    project: &str,
    input: Value,
) -> (StatusCode, Value) {
    invoke_at(&relay_url(), keys, plugin, operation, project, input).await
}

async fn ok(keys: &Keys, plugin: &str, operation: &str, project: &str, input: Value) -> Value {
    let (status, body) = invoke(keys, plugin, operation, project, input).await;
    assert_eq!(status, StatusCode::OK, "{plugin}/{operation}: {body}");
    body
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

async fn create_project(
    base_ws: &str,
    owner: &Keys,
    access: Option<&str>,
    members: &[(&Keys, &str)],
) -> String {
    let d_tag = unique("plugin");
    let mut client = BeekeeperTestClient::connect(base_ws, owner)
        .await
        .expect("connect");
    let ok = client
        .send_event(project_event(owner, &d_tag, access, members))
        .await
        .expect("send project");
    assert!(ok.accepted, "project refused: {}", ok.message);
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

/// Sign each draft with `keys` exactly as given and publish it, in order.
/// Returns the signed events.
async fn publish_drafts(base_ws: &str, keys: &Keys, drafts: &Value) -> Vec<nostr::Event> {
    let mut client = BeekeeperTestClient::connect(base_ws, keys)
        .await
        .expect("connect");
    let mut published = Vec::new();
    for draft in drafts.as_array().expect("drafts") {
        let tags: Vec<Tag> = draft["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let parts: Vec<String> = t
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect();
                Tag::parse(parts).unwrap()
            })
            .collect();
        let event = EventBuilder::new(
            Kind::Custom(draft["kind"].as_u64().unwrap() as u16),
            draft["content"].as_str().unwrap(),
        )
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from(draft["createdAt"].as_u64().unwrap()))
        .sign_with_keys(keys)
        .unwrap();
        let ok = client.send_event(event.clone()).await.expect("send draft");
        assert!(ok.accepted, "draft refused by ingest: {}", ok.message);
        published.push(event);
    }
    published
}

fn issues(read: &Value) -> Vec<Value> {
    read["result"]["issues"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[tokio::test]
#[ignore = "requires running relay"]
async fn discovery_lists_each_registered_plugin_with_its_pin_and_api() {
    let keys = Keys::generate();
    let (status, body) = get_plugins(&relay_url(), &keys).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let plugins = body["plugins"].as_array().expect("plugins");
    let planning = plugins
        .iter()
        .find(|p| p["name"] == "planning")
        .expect("planning registered");
    assert_eq!(planning["api"], json!(["beekeeper.planning/1"]));
    assert_eq!(planning["sha256"].as_str().unwrap().len(), 64);
    assert!(planning["operations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|op| op["name"] == "set-status" && op["effect"] == "draft"));
    let fixture = plugins
        .iter()
        .find(|p| p["name"] == "planning-fixture")
        .expect("fixture registered the same way");
    assert_eq!(fixture["api"], json!(["beekeeper.planning/1"]));
}

#[tokio::test]
#[ignore = "requires running relay"]
async fn the_route_requires_nip98_for_this_exact_request() {
    let keys = Keys::generate();
    let url = format!("{}/plugins", to_http(&relay_url()));
    let resp = http().get(&url).send().await.expect("unauthenticated");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    // Signed for another path.
    let wrong = format!("{}/query", to_http(&relay_url()));
    let resp = http()
        .get(&url)
        .header("Authorization", nip98(&keys, "GET", &wrong, None))
        .send()
        .await
        .expect("wrong url");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// A member creates an issue through the plugin, signs the drafts as
/// themselves, and the read — through the plugin — attributes it to them.
/// Another member then changes its status the same way.
#[tokio::test]
#[ignore = "requires running relay"]
async fn drafts_are_signed_by_the_member_and_read_back_through_the_plugin() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let project = create_project(&relay_url(), &owner, None, &[]).await;

    let empty = ok(&member, "planning", "read", &project, json!({})).await;
    assert_eq!(issues(&empty).len(), 0);
    assert_eq!(empty["plugin"]["name"], "planning");
    assert_eq!(empty["access"], "write");

    let created = ok(
        &member,
        "planning",
        "create-issue",
        &project,
        json!({ "title": "Plugin-made issue", "description": "Drafted in WASM.",
                "statusId": "in_progress", "priorityId": "high",
                "assignee": owner.public_key().to_hex(), "containerId": null }),
    )
    .await;
    let drafts = &created["drafts"];
    assert_eq!(drafts.as_array().unwrap().len(), 7, "{drafts}");
    let published = publish_drafts(&relay_url(), &member, drafts).await;
    assert!(published.iter().all(|e| e.pubkey == member.public_key()));

    let read = ok(&owner, "planning", "read", &project, json!({})).await;
    let issue = &issues(&read)[0];
    assert_eq!(issue["key"], created["result"]["key"]);
    assert_eq!(issue["title"], "Plugin-made issue");
    assert_eq!(issue["createdBy"], member.public_key().to_hex());
    assert_eq!(issue["status"]["id"], "in_progress");
    assert_eq!(issue["priority"]["id"], "high");
    assert_eq!(issue["assignee"], owner.public_key().to_hex());

    let status = ok(
        &owner,
        "planning",
        "set-status",
        &project,
        json!({ "containerId": issue["containerId"], "issueId": issue["issueId"],
                "statusId": "done" }),
    )
    .await;
    publish_drafts(&relay_url(), &owner, &status["drafts"]).await;
    let read = ok(&member, "planning", "read", &project, json!({})).await;
    let issue = &issues(&read)[0];
    assert_eq!(issue["status"]["id"], "done");
    assert!(issue["completedAt"].is_u64());

    let activity = ok(
        &member,
        "planning",
        "activity",
        &project,
        json!({ "containerId": issue["containerId"], "issueId": issue["issueId"] }),
    )
    .await;
    let actors: Vec<&str> = activity["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["actor"].as_str().unwrap())
        .collect();
    assert!(actors.contains(&member.public_key().to_hex().as_str()));
    assert!(actors.contains(&owner.public_key().to_hex().as_str()));
}

#[tokio::test]
#[ignore = "requires running relay"]
async fn the_project_gate_is_decided_before_any_plugin_code_runs() {
    let owner = Keys::generate();
    let viewer = Keys::generate();
    let stranger = Keys::generate();
    let project = create_project(
        &relay_url(),
        &owner,
        Some("private"),
        &[(&viewer, "viewer")],
    )
    .await;

    let (status, body) = invoke(&stranger, "planning", "read", &project, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "scope-forbidden");

    let read = ok(&viewer, "planning", "read", &project, json!({})).await;
    assert_eq!(read["access"], "read");
    let (status, body) = invoke(
        &viewer,
        "planning",
        "create-issue",
        &project,
        json!({ "title": "no", "description": null, "statusId": "todo",
                "priorityId": "none", "assignee": null, "containerId": null }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "write-not-allowed");

    let unknown = format!(
        "{PROJECT_KIND}:{}:nope-{}",
        owner.public_key().to_hex(),
        uuid::Uuid::new_v4()
    );
    let (status, body) = invoke(&owner, "planning", "read", &unknown, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "unknown-project");

    let shouty = project.replace(
        &owner.public_key().to_hex(),
        &owner.public_key().to_hex().to_uppercase(),
    );
    let (status, _) = invoke(&owner, "planning", "read", &shouty, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = invoke(&owner, "no-such-plugin", "read", &project, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "plugin-not-registered");
    let (status, body) = invoke(&owner, "planning", "no-such-op", &project, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "unknown-operation");
}

/// A personal list's ops are their author's alone; a plugin reading on
/// another member's behalf never receives them.
#[tokio::test]
#[ignore = "requires running relay"]
async fn a_plugin_reads_only_what_its_caller_could_read() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let project = create_project(&relay_url(), &owner, None, &[]).await;

    // Owner's personal list, created through the same plugin path: a
    // personal list is not something Planning drafts, so build it here.
    let list = uuid::Uuid::new_v4().simple().to_string();
    let content = json!({ "schema": "buzz-project-todo/v1", "op": "list.create",
                          "listId": list, "title": "Mine", "visibility": "personal" });
    let personal = EventBuilder::new(Kind::Custom(44248), content.to_string())
        .tags([
            Tag::parse(["a", &project]).unwrap(),
            Tag::parse(["td-v", "td1-1"]).unwrap(),
            Tag::parse(["td-op", "list.create"]).unwrap(),
            Tag::parse(["td-list", &list]).unwrap(),
            Tag::parse(["td-vis", "personal"]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap();
    let mut client = BeekeeperTestClient::connect(&relay_url(), &owner)
        .await
        .expect("connect");
    assert!(
        client
            .send_event(personal.clone())
            .await
            .expect("send")
            .accepted
    );
    let shared = ok(
        &member,
        "planning",
        "create-issue",
        &project,
        json!({ "title": "Shared", "description": null, "statusId": "todo",
                "priorityId": "none", "assignee": null, "containerId": null }),
    )
    .await;
    publish_drafts(&relay_url(), &member, &shared["drafts"]).await;

    let as_owner = ok(&owner, "test-faults", "echo", &project, json!({})).await;
    let as_member = ok(&member, "test-faults", "echo", &project, json!({})).await;
    let ids = |v: &Value| -> Vec<String> {
        v["result"]["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_str().unwrap().to_owned())
            .collect()
    };
    assert!(
        ids(&as_owner).contains(&personal.id.to_hex()),
        "author sees own"
    );
    assert!(
        !ids(&as_member).contains(&personal.id.to_hex()),
        "another member's plugin read must not receive a personal op"
    );
    assert_eq!(as_member["result"]["caller"], member.public_key().to_hex());
    assert_eq!(as_member["result"]["coordinates"], json!([project]));
}

/// The same coordinate on another community's host is another tenant: the
/// project does not exist there, and nothing of the first leaks.
#[tokio::test]
#[ignore = "requires running relay"]
async fn a_plugin_call_never_crosses_communities() {
    let owner = Keys::generate();
    let project_a = create_project(&relay_url(), &owner, None, &[]).await;
    let created = ok(
        &owner,
        "planning",
        "create-issue",
        &project_a,
        json!({ "title": "Only in A", "description": null, "statusId": "todo",
                "priorityId": "none", "assignee": null, "containerId": null }),
    )
    .await;
    publish_drafts(&relay_url(), &owner, &created["drafts"]).await;

    let (status, body) = invoke_at(
        &relay_url_b(),
        &owner,
        "planning",
        "read",
        &project_a,
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "B must not know A's project: {body}"
    );
    assert_eq!(body["code"], "unknown-project");

    // The owner makes a project in B with the same d-tag shape; its reads see
    // nothing written in A.
    let project_b = create_project(&relay_url_b(), &owner, None, &[]).await;
    let (status, body) = invoke_at(
        &relay_url_b(),
        &owner,
        "test-faults",
        "echo",
        &project_b,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["records"], 0);
    let (status, body) = invoke_at(
        &relay_url_b(),
        &owner,
        "planning",
        "read",
        &project_b,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(issues(&body).len(), 0);
}

#[tokio::test]
#[ignore = "requires running relay"]
async fn a_misbehaving_plugin_is_contained_and_the_relay_keeps_serving() {
    let owner = Keys::generate();
    let project = create_project(&relay_url(), &owner, None, &[]).await;

    let (status, body) = invoke(&owner, "test-faults", "spin", &project, json!({})).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["code"], "plugin-trapped");
    assert_eq!(body["trap"], "fuel");

    let (status, body) = invoke(&owner, "test-faults", "panic", &project, json!({})).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["trap"], "trap");

    let (status, body) = invoke(&owner, "test-faults", "grow", &project, json!({})).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["trap"], "memory");

    let (status, body) = invoke(
        &owner,
        "test-faults",
        "drafts-from-a-read",
        &project,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["code"], "contract-violation");

    let (status, body) = invoke(
        &owner,
        "test-faults",
        "draft-other-scope",
        &project,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");

    let denied = ok(
        &owner,
        "test-faults",
        "read-ungranted-kind",
        &project,
        json!({}),
    )
    .await;
    assert_eq!(denied["result"]["denied"], true);
    let flood = ok(&owner, "test-faults", "query-flood", &project, json!({})).await;
    assert_eq!(flood["result"]["stopped"], "budget");

    // Still serving, and planning untouched.
    ok(&owner, "planning", "read", &project, json!({})).await;
}

/// A second provider of the same API, built separately, registered the same
/// way, answers through the same route; its refusal of a write is honest.
#[tokio::test]
#[ignore = "requires running relay"]
async fn a_replacement_provider_answers_through_the_same_route() {
    let owner = Keys::generate();
    let project = create_project(&relay_url(), &owner, None, &[]).await;
    let read = ok(&owner, "planning-fixture", "read", &project, json!({})).await;
    assert_eq!(read["plugin"]["api"], json!(["beekeeper.planning/1"]));
    assert_eq!(issues(&read)[0]["title"], "Served by the fixture provider");
    assert_eq!(issues(&read)[0]["createdBy"], owner.public_key().to_hex());
    let (status, body) = invoke(
        &owner,
        "planning-fixture",
        "set-title",
        &project,
        json!({ "containerId": "fixture", "issueId": "1", "title": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
    assert_eq!(body["code"], "unsupported");
    assert!(body["error"].as_str().unwrap().contains("read-only"));
}

/// More than a page of history, with the newest thousand-odd ops all hidden
/// from the reader (other people's personal lists): the read must still
/// reach the shared issue behind them, and must not claim to be complete
/// when it is not.
#[tokio::test]
#[ignore = "requires running relay"]
async fn a_read_reaches_history_behind_more_than_a_page_of_hidden_ops() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let project = create_project(&relay_url(), &owner, None, &[]).await;

    let shared = ok(
        &member,
        "planning",
        "create-issue",
        &project,
        json!({ "title": "Older than a page", "description": null, "statusId": "todo",
                "priorityId": "none", "assignee": null, "containerId": null }),
    )
    .await;
    publish_drafts(&relay_url(), &member, &shared["drafts"]).await;

    // 1,120 ops on 28 people's personal lists (40 each, under the relay's
    // per-key message quota), all newer than the issue and all withheld from
    // the member.
    let base = nostr::Timestamp::now().as_secs();

    for author_n in 0..28u64 {
        let author = if author_n == 0 {
            owner.clone()
        } else {
            Keys::generate()
        };
        let list = uuid::Uuid::new_v4().simple().to_string();
        let mut client = BeekeeperTestClient::connect(&relay_url(), &author)
            .await
            .expect("connect");
        for n in 0..40u64 {
            let (op, content) = if n == 0 {
                (
                    "list.create",
                    json!({ "schema": "buzz-project-todo/v1", "op": "list.create",
                                        "listId": list, "title": "Mine", "visibility": "personal" }),
                )
            } else {
                (
                    "list.title",
                    json!({ "schema": "buzz-project-todo/v1", "op": "list.title",
                                       "listId": list, "title": format!("Mine {n}") }),
                )
            };
            let event = EventBuilder::new(Kind::Custom(44248), content.to_string())
                .tags([
                    Tag::parse(["a", &project]).unwrap(),
                    Tag::parse(["td-v", "td1-1"]).unwrap(),
                    Tag::parse(["td-op", op]).unwrap(),
                    Tag::parse(["td-list", &list]).unwrap(),
                    Tag::parse(["td-vis", "personal"]).unwrap(),
                ])
                .custom_created_at(nostr::Timestamp::from(base + 2 + n / 10))
                .sign_with_keys(&author)
                .unwrap();
            let sent = client.send_event(event).await.expect("send");
            assert!(sent.accepted, "op {author_n}/{n} refused: {}", sent.message);
        }
    }

    let read = ok(&member, "planning", "read", &project, json!({})).await;
    let titles: Vec<&str> = read["result"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Older than a page"], "{read}");
    assert_eq!(read["result"]["truncated"], false);

    // The owner's read sees their own personal ops too, and still one issue:
    // personal lists are not Planning containers.
    let read = ok(&owner, "planning", "read", &project, json!({})).await;
    assert_eq!(issues(&read).len(), 1);
    assert_eq!(read["result"]["truncated"], false);
}

/// The replacement provider drafts its own ops; the member signs them and
/// ingest accepts them, and the first-party provider reads them back — two
/// independently built plugins, one record format.
#[tokio::test]
#[ignore = "requires running relay"]
async fn a_replacement_providers_drafts_are_member_signed_and_accepted() {
    let owner = Keys::generate();
    let project = create_project(&relay_url(), &owner, None, &[]).await;
    let created = ok(
        &owner,
        "planning-fixture",
        "create-issue",
        &project,
        json!({ "title": "Filed via the fixture" }),
    )
    .await;
    assert_eq!(created["drafts"].as_array().unwrap().len(), 2);
    let published = publish_drafts(&relay_url(), &owner, &created["drafts"]).await;
    assert!(published.iter().all(|e| e.pubkey == owner.public_key()));
    let read = ok(&owner, "planning", "read", &project, json!({})).await;
    assert_eq!(issues(&read)[0]["title"], "Filed via the fixture");
    assert_eq!(issues(&read)[0]["key"], created["result"]["key"]);
}

/// A valid `list.create` proposal's data tags and content, with no scope:
/// what a plugin may say. `extra` tags are appended to the proposal.
fn told_list_create(extra: Value) -> Value {
    let list = uuid::Uuid::new_v4().simple().to_string();
    let mut tags = json!([
        ["td-v", "td1-1"],
        ["td-op", "list.create"],
        ["td-list", list],
        ["td-vis", "project"]
    ]);
    for tag in extra.as_array().cloned().unwrap_or_default() {
        tags.as_array_mut().unwrap().push(tag);
    }
    json!({ "drafts": [{ "kind": 44248, "tags": tags,
        "content": json!({ "schema": "buzz-project-todo/v1", "op": "list.create",
            "listId": list, "title": "Told", "visibility": "project" }).to_string() }] })
}

/// The relay writes every draft's scope itself, from the project the caller
/// was authorized for: a plugin that leaves it out gets it added, and one
/// that names another project, names it twice, or proposes any routing tag
/// the kind does not allow has its whole output discarded.
#[tokio::test]
#[ignore = "requires running relay"]
async fn the_relay_writes_each_drafts_scope_and_refuses_routing_it_did_not_allow() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let project = create_project(&relay_url(), &owner, None, &[]).await;
    let other = create_project(&relay_url(), &owner, None, &[]).await;

    let sealed = ok(
        &member,
        "test-faults",
        "draft-as-told",
        &project,
        told_list_create(json!([])),
    )
    .await;
    let tags = sealed["drafts"][0]["tags"].as_array().unwrap().clone();
    let scopes: Vec<&Value> = tags.iter().filter(|t| t[0] == "a").collect();
    assert_eq!(scopes, vec![&json!(["a", project])], "{tags:?}");
    let published = publish_drafts(&relay_url(), &member, &sealed["drafts"]).await;
    assert_eq!(published.len(), 1, "ingest accepts the sealed draft");

    for extra in [
        json!([["a", other]]),
        json!([["a", project], ["a", other]]),
        json!([["a", project], ["a", project]]),
        json!([["p", owner.public_key().to_hex()]]),
        json!([["e", "e".repeat(64)]]),
        json!([["h", "a-channel"]]),
        json!([["unknown", "x"]]),
    ] {
        let (status, body) = invoke(
            &member,
            "test-faults",
            "draft-as-told",
            &project,
            told_list_create(extra.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{extra}: {body}");
        assert_eq!(body["code"], "contract-violation", "{extra}: {body}");
        assert!(
            body.get("drafts").is_none(),
            "nothing reaches a signer: {body}"
        );
    }

    // A valid first draft and one stamped half an hour ago — inside the old
    // hour-long window, outside what ingest takes after transport: the
    // caller gets nothing to sign, so not even the first can land.
    let now = nostr::Timestamp::now().as_secs();
    let mut two = told_list_create(json!([]));
    let mut stale = two["drafts"][0].clone();
    stale["createdAt"] = json!(now - 1_800);
    two["drafts"].as_array_mut().unwrap().push(stale);
    let (status, body) = invoke(&member, "test-faults", "draft-as-told", &project, two).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.get("drafts").is_none(), "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("draft 1: created_at"),
        "{body}"
    );

    // 64 proposed tags pass the proposal's cap; with the relay's scope they
    // would be 65.
    let filler: Vec<Value> = (0..60).map(|_| json!(["td-v", "td1-1"])).collect();
    let (status, body) = invoke(
        &member,
        "test-faults",
        "draft-as-told",
        &project,
        told_list_create(json!(filler)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("constructed tags: 65"),
        "{body}"
    );
}

/// The latest kind 30621 head of `project`, as `owner` reads it: its id and
/// its roster (`p` tags), which is what the project's gate is built from.
async fn project_head(owner: &Keys, project: &str) -> (String, Vec<Vec<String>>) {
    let d_tag = project.splitn(3, ':').nth(2).expect("d tag");
    let mut client = BeekeeperTestClient::connect(&relay_url(), owner)
        .await
        .expect("connect");
    let sid = unique("head");
    let filter = nostr::Filter::new()
        .kind(Kind::Custom(PROJECT_KIND))
        .author(owner.public_key())
        .identifier(d_tag);
    client
        .subscribe(&sid, vec![filter])
        .await
        .expect("subscribe");
    let events = client
        .collect_until_eose(&sid, Duration::from_secs(10))
        .await
        .expect("collect");
    let head = events
        .into_iter()
        .max_by_key(|e| e.created_at)
        .expect("project head");
    let roster = head
        .tags
        .iter()
        .map(|t| t.as_slice().to_vec())
        .filter(|t| t[0] == "p")
        .collect();
    (head.id.to_hex(), roster)
}

/// An assignee is data: an `item.assignee` op sets one folded field. It
/// grants no access, carries no `p` tag and notifies no one, and ingest
/// takes it from anyone allowed to write — the To-Do tab and `bee todos`
/// assign any well-formed pubkey. So the plugin route holds it to the same
/// rule. An agent associated with a project but not on its roster (which
/// the AssigneePicker offers) can be assigned through Planning exactly as
/// through a direct op, and assigning it changes nothing about who reads.
#[tokio::test]
#[ignore = "requires running relay"]
async fn an_off_roster_assignee_lands_through_the_plugin_as_it_does_directly() {
    let owner = Keys::generate();
    let collaborator = Keys::generate();
    let viewer = Keys::generate();
    let agent = Keys::generate();
    let agent_hex = agent.public_key().to_hex();
    let project = create_project(
        &relay_url(),
        &owner,
        Some("private"),
        &[(&collaborator, "collaborator"), (&viewer, "viewer")],
    )
    .await;
    let before = project_head(&owner, &project).await;

    // Through the plugin: create an issue assigned to the agent, then clear
    // it and assign it again with set-assignee.
    let created = ok(
        &collaborator,
        "planning",
        "create-issue",
        &project,
        json!({ "title": "For the agent", "description": null, "statusId": "todo",
                "priorityId": "none", "assignee": agent_hex, "containerId": null }),
    )
    .await;
    let mut published = publish_drafts(&relay_url(), &collaborator, &created["drafts"]).await;
    let read = ok(&viewer, "planning", "read", &project, json!({})).await;
    let issue = issues(&read)
        .into_iter()
        .find(|i| i["key"] == created["result"]["key"])
        .expect("created");
    assert_eq!(issue["assignee"], agent_hex);
    let target = json!({ "containerId": issue["containerId"], "issueId": issue["issueId"] });
    for assignee in [Value::Null, json!(agent_hex)] {
        let mut input = target.clone();
        input["assignee"] = assignee.clone();
        let edit = ok(&owner, "planning", "set-assignee", &project, input).await;
        published.extend(publish_drafts(&relay_url(), &owner, &edit["drafts"]).await);
        let read = ok(&viewer, "planning", "read", &project, json!({})).await;
        let issue = issues(&read)
            .into_iter()
            .find(|i| i["key"] == created["result"]["key"])
            .expect("issue");
        assert_eq!(issue["assignee"], assignee);
    }

    // Directly, as the To-Do tab and `bee todos` sign it: the same op from
    // the same collaborator, with no plugin in the path, is accepted too.
    let list = issue["containerId"].as_str().expect("list id");
    let item = issue["issueId"].as_str().expect("item id");
    let direct = EventBuilder::new(
        Kind::Custom(44248),
        json!({ "schema": "buzz-project-todo/v1", "op": "item.assignee",
                "listId": list, "itemId": item, "assignee": agent_hex })
        .to_string(),
    )
    .tags([
        Tag::parse(["a", &project]).unwrap(),
        Tag::parse(["td-v", "td1-1"]).unwrap(),
        Tag::parse(["td-op", "item.assignee"]).unwrap(),
        Tag::parse(["td-list", list]).unwrap(),
        Tag::parse(["td-vis", "project"]).unwrap(),
        Tag::parse(["td-item", item]).unwrap(),
    ])
    .sign_with_keys(&collaborator)
    .unwrap();
    let mut client = BeekeeperTestClient::connect(&relay_url(), &collaborator)
        .await
        .expect("connect");
    let accepted = client.send_event(direct.clone()).await.expect("send");
    assert!(accepted.accepted, "direct assign: {}", accepted.message);
    published.push(direct);

    // No routing: no op names the agent in a tag, so nothing addressed by
    // `p` (mentions, notifications, DMs) ever reaches it.
    for event in &published {
        assert!(
            event
                .tags
                .iter()
                .all(|t| t.as_slice()[0] != "p" && !t.as_slice().iter().any(|v| v == &agent_hex)),
            "an assignment op must not route to its assignee: {:?}",
            event.tags
        );
    }
    // No access: the roster is the same head, and the agent still cannot
    // read the project through the plugin or over a raw REQ.
    assert_eq!(project_head(&owner, &project).await, before);
    let (status, body) = invoke(&agent, "planning", "read", &project, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "scope-forbidden");
    let mut agent_client = BeekeeperTestClient::connect(&relay_url(), &agent)
        .await
        .expect("agent connect");
    let sid = unique("agent-read");
    let filter = nostr::Filter::new().kind(Kind::Custom(44248)).custom_tags(
        nostr::SingleLetterTag::lowercase(nostr::Alphabet::A),
        [project.as_str()],
    );
    agent_client
        .subscribe(&sid, vec![filter])
        .await
        .expect("subscribe");
    let seen = agent_client
        .collect_until_eose(&sid, Duration::from_secs(10))
        .await
        .expect("collect");
    assert!(
        seen.is_empty(),
        "a private project's ops stay withheld from its assignee"
    );
}
