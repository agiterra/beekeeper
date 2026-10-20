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
