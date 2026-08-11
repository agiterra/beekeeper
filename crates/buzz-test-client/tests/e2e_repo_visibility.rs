//! End-to-end tests for private-project repository gating (NIP-MP Buzz
//! access extension, phase 2): a kind:30617 announcement carrying a
//! `["project", "30621:<owner>:<d>"]` back-reference into a private project
//! hides the repo's whole event surface from readers outside the project.
//!
//! The buzz-core unit tests pin the per-event predicate
//! (`repo_event_hidden_from`) and the ingest tests pin the 30617 project-tag
//! rules; these tests prove the gate is wired into every live surface:
//! - the announcement, relay-signed kind:30618 ref state, and kind:1621
//!   issues are withheld from strangers across WS REQ, `ids` lookup, live
//!   fan-out, HTTP `POST /query`, and HTTP `POST /count`, while staying
//!   visible to the repo owner and the project's invited members;
//! - writes of NIP-34 child events into a private project's repo are
//!   rejected for outsiders and accepted for members;
//! - announcing a repo *into* a private project requires being admitted to
//!   that project, and malformed `project` tags are rejected outright;
//! - republishing the announcement without the tag re-reveals everything;
//! - repos in public projects (and project-less repos) stay visible.
//!
//! See `docs/nips/NIP-MP.md` §Repository access (phase 2).
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p buzz-test-client --test e2e_repo_visibility -- --ignored
//! ```

use std::time::Duration;

use buzz_test_client::BuzzTestClient;
use buzz_ws_client::RelayMessage;
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag, Timestamp};
use reqwest::Client;
use serde_json::Value;

const PROJECT_KIND: u16 = 30621;
const REPO_ANNOUNCEMENT_KIND: u16 = 30617;
const REPO_STATE_KIND: u16 = 30618;
const ISSUE_KIND: u16 = 1621;

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
    format!("e2e-repo-vis-{name}-{}", uuid::Uuid::new_v4())
}

/// A short unique suffix so concurrent runs never collide on a repo name
/// (repo names are community-unique).
fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn project_coordinate(owner: &Keys, d_tag: &str) -> String {
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

fn repo_coordinate(owner: &Keys, repo_d: &str) -> String {
    format!(
        "{REPO_ANNOUNCEMENT_KIND}:{}:{repo_d}",
        owner.public_key().to_hex()
    )
}

/// Build a private (or public/tag-less) project head with invited members.
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

/// Build a repo announcement, optionally back-referencing a project.
fn repo_event(
    keys: &Keys,
    repo_d: &str,
    project_ref: Option<&str>,
    created_at: Option<u64>,
) -> nostr::Event {
    let mut tags = vec![
        Tag::parse(["d", repo_d]).unwrap(),
        Tag::parse(["name", repo_d]).unwrap(),
    ];
    if let Some(project_ref) = project_ref {
        tags.push(Tag::parse(["project", project_ref]).unwrap());
    }
    let builder = EventBuilder::new(Kind::Custom(REPO_ANNOUNCEMENT_KIND), "").tags(tags);
    match created_at {
        Some(ts) => builder.custom_created_at(Timestamp::from(ts)),
        None => builder,
    }
    .sign_with_keys(keys)
    .unwrap()
}

/// Build a NIP-34 issue targeting a repo coordinate.
fn issue_event(keys: &Keys, repo_coord: &str, content: &str) -> nostr::Event {
    EventBuilder::new(Kind::Custom(ISSUE_KIND), content)
        .tags(vec![
            Tag::parse(["a", repo_coord]).unwrap(),
            Tag::parse(["subject", "e2e issue"]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap()
}

fn announcement_filter(owner: &Keys, repo_d: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(REPO_ANNOUNCEMENT_KIND))
        .author(owner.public_key())
        .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [repo_d])
}

fn ref_state_filter(repo_d: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(REPO_STATE_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::D), [repo_d])
}

fn issues_filter(repo_coord: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(ISSUE_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::A), [repo_coord])
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

/// Owner sets up a private project (with `member` invited) and announces a
/// repo inside it. Returns the repo `d` and the repo coordinate.
async fn setup_private_repo(
    owner_client: &mut BuzzTestClient,
    owner: &Keys,
    member: &Keys,
) -> (String, String, String) {
    let project_d = unique("secret-proj");
    let ok = owner_client
        .send_event(project_event(owner, &project_d, Some("private"), &[member]))
        .await
        .expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);

    let repo_d = unique("secret-repo");
    let coord = project_coordinate(owner, &project_d);
    let ok = owner_client
        .send_event(repo_event(owner, &repo_d, Some(&coord), None))
        .await
        .expect("send 30617");
    assert!(ok.accepted, "repo announcement rejected: {}", ok.message);
    let repo_coord = repo_coordinate(owner, &repo_d);
    (project_d, repo_d, repo_coord)
}

/// The full repo event surface — 30617 announcement, relay-signed 30618 ref
/// state, and a 1621 issue — is visible to the repo owner and the project
/// member, and withheld from a stranger across WS REQ, `ids` lookup, HTTP
/// `/query`, and HTTP `/count`.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_private_repo_surface_hidden_from_stranger_everywhere() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let (_project_d, repo_d, repo_coord) =
        setup_private_repo(&mut owner_client, &owner, &member).await;

    let issue = issue_event(&owner, &repo_coord, "owner-filed issue");
    let issue_id = issue.id;
    let ok = owner_client.send_event(issue).await.expect("send issue");
    assert!(ok.accepted, "owner issue rejected: {}", ok.message);

    // Owner sees announcement, ref state, and issue.
    let seen = query(
        &mut owner_client,
        "own-ann",
        announcement_filter(&owner, &repo_d),
    )
    .await;
    assert_eq!(seen.len(), 1, "owner must see their announcement");
    let seen = query(&mut owner_client, "own-state", ref_state_filter(&repo_d)).await;
    assert_eq!(
        seen.len(),
        1,
        "owner must see the relay-signed initial ref state"
    );
    let seen = query(&mut owner_client, "own-issue", issues_filter(&repo_coord)).await;
    assert_eq!(seen.len(), 1, "owner must see the issue");

    // Invited project member sees all three.
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let seen = query(
        &mut member_client,
        "member-ann",
        announcement_filter(&owner, &repo_d),
    )
    .await;
    assert_eq!(seen.len(), 1, "project member must see the announcement");
    let seen = query(
        &mut member_client,
        "member-state",
        ref_state_filter(&repo_d),
    )
    .await;
    assert_eq!(seen.len(), 1, "project member must see the ref state");
    let seen = query(
        &mut member_client,
        "member-issue",
        issues_filter(&repo_coord),
    )
    .await;
    assert_eq!(seen.len(), 1, "project member must see the issue");

    // Stranger sees none of it via WS…
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(
        &mut stranger_client,
        "stranger-ann",
        announcement_filter(&owner, &repo_d),
    )
    .await;
    assert!(seen.is_empty(), "stranger must not see the announcement");
    let seen = query(
        &mut stranger_client,
        "stranger-state",
        ref_state_filter(&repo_d),
    )
    .await;
    assert!(seen.is_empty(), "stranger must not see the ref state");
    let seen = query(
        &mut stranger_client,
        "stranger-issue",
        issues_filter(&repo_coord),
    )
    .await;
    assert!(seen.is_empty(), "stranger must not see the issue");

    // …nor via a kindless known-id lookup…
    let seen = query(
        &mut stranger_client,
        "stranger-ids",
        Filter::new().id(issue_id),
    )
    .await;
    assert!(
        seen.is_empty(),
        "known event id must not bypass the repo gate"
    );

    // …nor via the HTTP bridge, where COUNT must not leak existence either.
    let http = http_client();
    let stranger_hex = stranger.public_key().to_hex();
    let rows = query_events_http(
        &http,
        &stranger_hex,
        vec![
            announcement_filter(&owner, &repo_d),
            issues_filter(&repo_coord),
        ],
    )
    .await;
    assert!(
        rows.is_empty(),
        "HTTP /query must withhold the repo surface"
    );
    for filter in [
        announcement_filter(&owner, &repo_d),
        ref_state_filter(&repo_d),
        issues_filter(&repo_coord),
    ] {
        let n = count_events_http(&http, &stranger_hex, vec![filter]).await;
        assert_eq!(n, 0, "HTTP /count must not leak repo activity");
    }
    // Member counts all three over HTTP.
    let member_hex = member.public_key().to_hex();
    let n = count_events_http(&http, &member_hex, vec![issues_filter(&repo_coord)]).await;
    assert_eq!(n, 1, "member's issue count must include the issue");
}

/// Writes into a private project's repo: a stranger's issue is rejected with
/// `restricted:`, the invited member's is accepted; the same stranger CAN
/// file an issue against a project-less repo.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_repo_child_write_gate() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let (_project_d, _repo_d, repo_coord) =
        setup_private_repo(&mut owner_client, &owner, &member).await;

    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let ok = stranger_client
        .send_event(issue_event(&stranger, &repo_coord, "drive-by issue"))
        .await
        .expect("send stranger issue");
    assert!(
        !ok.accepted,
        "stranger issue into a private project's repo must be rejected"
    );
    assert!(
        ok.message.starts_with("restricted:"),
        "expected restricted rejection, got: {}",
        ok.message
    );

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let ok = member_client
        .send_event(issue_event(&member, &repo_coord, "member issue"))
        .await
        .expect("send member issue");
    assert!(ok.accepted, "member issue rejected: {}", ok.message);

    // Control: the stranger can still write issues against an ungated repo.
    let open_repo_d = unique("open-repo");
    let ok = owner_client
        .send_event(repo_event(&owner, &open_repo_d, None, None))
        .await
        .expect("send open repo");
    assert!(ok.accepted, "open repo rejected: {}", ok.message);
    let ok = stranger_client
        .send_event(issue_event(
            &stranger,
            &repo_coordinate(&owner, &open_repo_d),
            "issue on open repo",
        ))
        .await
        .expect("send open issue");
    assert!(
        ok.accepted,
        "issue on an ungated repo must be accepted: {}",
        ok.message
    );
}

/// Announcing a repo INTO a private project requires being admitted to it:
/// a stranger's back-reference is rejected, an invited member's is accepted,
/// and a malformed `project` tag is rejected outright.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_private_link_requires_project_membership() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();
    let project_d = unique("link-proj");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let ok = owner_client
        .send_event(project_event(
            &owner,
            &project_d,
            Some("private"),
            &[&member],
        ))
        .await
        .expect("send project");
    assert!(ok.accepted, "private project rejected: {}", ok.message);
    let coord = project_coordinate(&owner, &project_d);

    // Stranger cannot link their repo into someone else's private project.
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let ok = stranger_client
        .send_event(repo_event(&stranger, &unique("squat"), Some(&coord), None))
        .await
        .expect("send stranger repo");
    assert!(
        !ok.accepted,
        "stranger linking into a private project must be rejected"
    );
    assert!(
        ok.message.starts_with("restricted:"),
        "expected restricted rejection, got: {}",
        ok.message
    );

    // The invited member can.
    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let ok = member_client
        .send_event(repo_event(
            &member,
            &unique("member-repo"),
            Some(&coord),
            None,
        ))
        .await
        .expect("send member repo");
    assert!(
        ok.accepted,
        "member's repo link into the project rejected: {}",
        ok.message
    );

    // Malformed project tags are rejected, never silently ignored.
    let ok = owner_client
        .send_event(repo_event(&owner, &unique("badref"), Some("junk"), None))
        .await
        .expect("send malformed");
    assert!(!ok.accepted, "malformed project tag must be rejected");
    assert!(
        ok.message.starts_with("invalid:"),
        "expected invalid rejection, got: {}",
        ok.message
    );
}

/// Republishing the announcement WITHOUT the project tag unlinks the repo:
/// the announcement and its issues become visible to everyone again.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_unlink_republish_re_reveals() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let (_project_d, repo_d, repo_coord) =
        setup_private_repo(&mut owner_client, &owner, &member).await;
    let ok = owner_client
        .send_event(issue_event(&owner, &repo_coord, "hidden then revealed"))
        .await
        .expect("send issue");
    assert!(ok.accepted, "issue rejected: {}", ok.message);

    // Hidden first.
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(
        &mut stranger_client,
        "before-unlink",
        announcement_filter(&owner, &repo_d),
    )
    .await;
    assert!(seen.is_empty(), "announcement must start hidden");

    // Republish with no project tag (newer created_at wins the LWW guard).
    let now = Timestamp::now().as_secs();
    let ok = owner_client
        .send_event(repo_event(&owner, &repo_d, None, Some(now + 1)))
        .await
        .expect("send unlink");
    assert!(ok.accepted, "unlinking republish rejected: {}", ok.message);

    // The link drop invalidates the gating caches; poll briefly.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut revealed = false;
    while tokio::time::Instant::now() < deadline {
        let ann = query(
            &mut stranger_client,
            "after-unlink-ann",
            announcement_filter(&owner, &repo_d),
        )
        .await;
        let issues = query(
            &mut stranger_client,
            "after-unlink-issue",
            issues_filter(&repo_coord),
        )
        .await;
        if ann.len() == 1 && issues.len() == 1 {
            revealed = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert!(
        revealed,
        "unlinking the repo must re-reveal its announcement and issues"
    );
}

/// Live fan-out: with open kind-1621 subscriptions, an issue in a private
/// project's repo reaches the invited member's connection and never the
/// stranger's.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_repo_issue_fanout_filtered_live() {
    let owner = Keys::generate();
    let member = Keys::generate();
    let stranger = Keys::generate();

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let (_project_d, _repo_d, repo_coord) =
        setup_private_repo(&mut owner_client, &owner, &member).await;

    let mut member_client = BuzzTestClient::connect(&relay_url(), &member)
        .await
        .expect("member connect");
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");

    let member_sid = sub_id("live-member");
    let stranger_sid = sub_id("live-stranger");
    let live_filter = Filter::new().kind(Kind::Custom(ISSUE_KIND));
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

    let issue = issue_event(&owner, &repo_coord, "live issue");
    let issue_id = issue.id;
    let ok = owner_client.send_event(issue).await.expect("send issue");
    assert!(ok.accepted, "issue rejected: {}", ok.message);

    // Member receives the live event.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut member_got = false;
    while tokio::time::Instant::now() < deadline {
        match member_client.recv_event(Duration::from_secs(2)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == member_sid && event.id == issue_id => {
                member_got = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(member_got, "project member must receive the live issue");

    // Stranger receives nothing for this issue within the wait window.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match stranger_client.recv_event(Duration::from_secs(1)).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == stranger_sid => {
                assert_ne!(
                    event.id, issue_id,
                    "stranger must not receive a private repo's issue via fan-out"
                );
            }
            Ok(_) => {}
            Err(_) => {} // timeout tick — keep waiting out the window
        }
    }
}

/// Repos linked to public projects — and project-less repos — stay visible
/// to everyone; the pre-extension default is unchanged.
#[tokio::test]
#[ignore = "requires running relay"]
async fn test_public_project_repo_stays_visible() {
    let owner = Keys::generate();
    let stranger = Keys::generate();
    let project_d = unique("pub-proj");

    let mut owner_client = BuzzTestClient::connect(&relay_url(), &owner)
        .await
        .expect("owner connect");
    let ok = owner_client
        .send_event(project_event(&owner, &project_d, Some("public"), &[]))
        .await
        .expect("send project");
    assert!(ok.accepted, "public project rejected: {}", ok.message);

    let pub_repo_d = unique("pub-repo");
    let coord = project_coordinate(&owner, &project_d);
    let ok = owner_client
        .send_event(repo_event(&owner, &pub_repo_d, Some(&coord), None))
        .await
        .expect("send repo");
    assert!(ok.accepted, "repo rejected: {}", ok.message);

    let bare_repo_d = unique("bare-repo");
    let ok = owner_client
        .send_event(repo_event(&owner, &bare_repo_d, None, None))
        .await
        .expect("send bare repo");
    assert!(ok.accepted, "bare repo rejected: {}", ok.message);

    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(
        &mut stranger_client,
        "pub",
        announcement_filter(&owner, &pub_repo_d),
    )
    .await;
    assert_eq!(seen.len(), 1, "public-project repo must stay visible");
    let seen = query(
        &mut stranger_client,
        "bare",
        announcement_filter(&owner, &bare_repo_d),
    )
    .await;
    assert_eq!(seen.len(), 1, "project-less repo must stay visible");
}
