//! End-to-end tests for NIP-AD agents-repository draft ops (kind:44250).
//!
//! The buzz-core unit tests pin the envelope and the fold, and the ingest
//! tests pin scope and shape. These prove what only a live relay can:
//!
//! - a project *collaborator* publishes a draft and a read-only *viewer* is
//!   refused with **403** (CLI exit 3), not 400;
//! - a draft naming a repository other than the one the project's
//!   kind:30624 pins is refused at ingest, and so is a draft for a project
//!   with no source at all;
//! - a `commit.record` naming a commit that is not the repository's `main`
//!   is refused — the relay checks the manifest chain, it does not take the
//!   committer's word;
//! - the ops read back over WS REQ and HTTP `/query` and fold to the head
//!   the contract promises, the same fold `bee agents-repo` and Desktop use;
//! - a private project's drafts are withheld from a stranger.
//!
//! See `docs/nips/NIP-AD.md` for the normative contract.
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p beekeeper-test-client --test e2e_agents_repo_drafts -- --ignored
//! ```

use std::time::Duration;

use beekeeper_core::agents_repo_draft::{AgentsRepoDraftOp, AgentsRepoDraftOpValue, DraftBase};
use beekeeper_core::agents_repo_draft_fold::{fold_agents_repo_drafts, DraftFoldEvent};
use beekeeper_core::project_pack_source::{build_project_pack_source, PackPin};
use beekeeper_test_client::BuzzTestClient;
use nostr::{Alphabet, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PROJECT_KIND: u16 = 30621;
const REPO_KIND: u16 = 30617;
const PACK_SOURCE_KIND: u16 = 30624;
const DRAFT_OP_KIND: u16 = 44250;

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

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn project_coordinate(owner: &Keys, d_tag: &str) -> String {
    format!("{PROJECT_KIND}:{}:{d_tag}", owner.public_key().to_hex())
}

fn repo_coordinate(owner: &Keys, id: &str) -> String {
    format!("{REPO_KIND}:{}:{id}", owner.public_key().to_hex())
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

fn repo_announcement(keys: &Keys, id: &str, project: &str) -> nostr::Event {
    EventBuilder::new(Kind::Custom(REPO_KIND), "")
        .tags([
            Tag::parse(["d", id]).unwrap(),
            Tag::parse(["project", project]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap()
}

fn pack_source(keys: &Keys, project: &str, repo: &str) -> nostr::Event {
    let draft = build_project_pack_source(
        project,
        repo,
        &PackPin::Ref("refs/heads/main".into()),
        Some("."),
        None,
    )
    .expect("a valid draft");
    let tags: Vec<Tag> = draft
        .tags
        .iter()
        .map(|tag| Tag::parse(tag.clone()).expect("tag parses"))
        .collect();
    EventBuilder::new(Kind::Custom(PACK_SOURCE_KIND), draft.content)
        .tags(tags)
        .sign_with_keys(keys)
        .expect("signs")
}

fn put_op(repo: &str, path: &str, text: &str, prev: Option<&str>) -> AgentsRepoDraftOp {
    AgentsRepoDraftOp {
        repo: repo.to_owned(),
        message: None,
        value: AgentsRepoDraftOpValue::FilePut {
            path: path.to_owned(),
            text: text.to_owned(),
            base: DraftBase {
                base: None,
                base_commit: None,
                prev: prev.map(str::to_owned),
            },
        },
    }
}

fn signed(keys: &Keys, coordinate: &str, op: &AgentsRepoDraftOp) -> nostr::Event {
    beekeeper_sdk::builders::build_agents_repo_draft_op(coordinate, op)
        .expect("builds")
        .sign_with_keys(keys)
        .expect("signs")
}

/// [`signed`] stamped at `created_at`: the fold's only clock is
/// `(created_at, id)`, and a save made after reading the head is stamped
/// past it (`bee` does this with `next_replaceable_created_at`).
fn signed_at(
    keys: &Keys,
    coordinate: &str,
    op: &AgentsRepoDraftOp,
    created_at: u64,
) -> nostr::Event {
    beekeeper_sdk::builders::build_agents_repo_draft_op(coordinate, op)
        .expect("builds")
        .custom_created_at(nostr::Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("signs")
}

fn draft_filter(coordinate: &str) -> Filter {
    Filter::new()
        .kind(Kind::Custom(DRAFT_OP_KIND))
        .custom_tags(SingleLetterTag::lowercase(Alphabet::A), [coordinate])
}

async fn query(client: &mut BuzzTestClient, name: &str, filter: Filter) -> Vec<nostr::Event> {
    let sid = format!("e2e-draft-{name}-{}", uuid::Uuid::new_v4());
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

/// A project with an agents repository pinned: head, announcement, source.
async fn project_with_source(
    owner: &Keys,
    access: Option<&str>,
    members: &[(&Keys, &str)],
) -> (BuzzTestClient, String, String) {
    let d_tag = unique("draft-proj");
    let coordinate = project_coordinate(owner, &d_tag);
    let repo_id = format!("{d_tag}-beekeeper-agents");
    let repo = repo_coordinate(owner, &repo_id);
    let mut client = BuzzTestClient::connect(&relay_url(), owner)
        .await
        .expect("owner connect");
    send_ok(
        &mut client,
        project_event(owner, &d_tag, access, members),
        "project",
    )
    .await;
    send_ok(
        &mut client,
        repo_announcement(owner, &repo_id, &coordinate),
        "announcement",
    )
    .await;
    send_ok(
        &mut client,
        pack_source(owner, &coordinate, &repo),
        "pack source",
    )
    .await;
    (client, coordinate, repo)
}

#[tokio::test]
#[ignore = "requires running relay"]
async fn test_collaborator_drafts_viewer_is_refused_403_and_the_fold_agrees() {
    let owner = Keys::generate();
    let collaborator = Keys::generate();
    let viewer = Keys::generate();
    let (mut owner_client, coordinate, repo) = project_with_source(
        &owner,
        Some("private"),
        &[(&collaborator, "collaborator"), (&viewer, "viewer")],
    )
    .await;

    let first_event = signed(
        &owner,
        &coordinate,
        &put_op(&repo, "plans/rpg.md", "# RPG v1\n", None),
    );
    let first_at = first_event.created_at.as_secs();
    let first = send_ok(&mut owner_client, first_event, "owner draft").await;

    let mut collab_client = BuzzTestClient::connect(&relay_url(), &collaborator)
        .await
        .expect("collaborator connect");
    let second = send_ok(
        &mut collab_client,
        signed_at(
            &collaborator,
            &coordinate,
            &put_op(&repo, "plans/rpg.md", "# RPG v2\n", Some(&first.to_hex())),
            first_at + 1,
        ),
        "collaborator draft",
    )
    .await;

    // The viewer is refused with an authorization failure: 403 over HTTP.
    let http = http_client();
    let viewer_event = signed(
        &viewer,
        &coordinate,
        &put_op(&repo, "plans/rpg.md", "nope\n", None),
    );
    let resp = http
        .post(format!("{}/events", relay_http_url()))
        .header("X-Pubkey", viewer.public_key().to_hex())
        .header("Content-Type", "application/json")
        .json(&viewer_event)
        .send()
        .await
        .expect("post");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "a viewer's draft is a 403"
    );

    // Read back and fold: the collaborator's save is the head, built on the owner's.
    let seen = query(&mut collab_client, "collab", draft_filter(&coordinate)).await;
    assert_eq!(seen.len(), 2, "both drafts read back");
    let events: Vec<DraftFoldEvent> = seen.iter().map(DraftFoldEvent::from).collect();
    let digest = fold_agents_repo_drafts(&coordinate, &repo, &events);
    assert_eq!(digest.paths.len(), 1);
    assert_eq!(digest.paths[0].path, "plans/rpg.md");
    assert_eq!(digest.paths[0].head.id, second.to_hex());
    assert_eq!(digest.paths[0].superseded.len(), 1);
    assert!(!digest.paths[0].diverged);
    assert_eq!(digest.ignored, 0);

    // The HTTP bridge accepts the project-scoped shape.
    let (status, body) = {
        let resp = http
            .post(format!("{}/query", relay_http_url()))
            .header("X-Pubkey", collaborator.public_key().to_hex())
            .header("Content-Type", "application/json")
            .json(&json!([{ "kinds": [DRAFT_OP_KIND], "#a": [coordinate], "limit": 50 }]))
            .send()
            .await
            .expect("query");
        let status = resp.status();
        (status, resp.json::<Value>().await.unwrap_or(Value::Null))
    };
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(2));

    // A stranger sees nothing of the private project.
    let stranger = Keys::generate();
    let mut stranger_client = BuzzTestClient::connect(&relay_url(), &stranger)
        .await
        .expect("stranger connect");
    let seen = query(&mut stranger_client, "stranger", draft_filter(&coordinate)).await;
    assert!(
        seen.is_empty(),
        "a stranger must not read a private project's drafts"
    );
}

#[tokio::test]
#[ignore = "requires running relay"]
async fn test_ingest_refuses_the_wrong_repository_and_a_project_with_no_source() {
    let owner = Keys::generate();
    let (mut owner_client, coordinate, _repo) = project_with_source(&owner, None, &[]).await;

    let other = repo_coordinate(&owner, "somewhere-else");
    send_refused(
        &mut owner_client,
        signed(
            &owner,
            &coordinate,
            &put_op(&other, "plans/rpg.md", "x\n", None),
        ),
        "is not this project's agents repository",
    )
    .await;

    // A second project with a head but no 30624.
    let d_tag = unique("draft-nosource");
    let bare = project_coordinate(&owner, &d_tag);
    send_ok(
        &mut owner_client,
        project_event(&owner, &d_tag, None, &[]),
        "bare project",
    )
    .await;
    let repo = repo_coordinate(&owner, &format!("{d_tag}-beekeeper-agents"));
    send_refused(
        &mut owner_client,
        signed(&owner, &bare, &put_op(&repo, "plans/rpg.md", "x\n", None)),
        "has no agents repository",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires running relay"]
async fn test_a_commit_record_for_a_never_pushed_repository_is_refused() {
    let owner = Keys::generate();
    let (mut owner_client, coordinate, repo) = project_with_source(&owner, None, &[]).await;
    let draft = send_ok(
        &mut owner_client,
        signed(
            &owner,
            &coordinate,
            &put_op(&repo, "plans/rpg.md", "x\n", None),
        ),
        "draft",
    )
    .await;
    let record = AgentsRepoDraftOp {
        repo: repo.clone(),
        message: Some("landed".into()),
        value: AgentsRepoDraftOpValue::CommitRecord {
            commit: "c".repeat(40),
            paths: vec!["plans/rpg.md".into()],
            drafts: vec![draft.to_hex()],
        },
    };
    send_refused(
        &mut owner_client,
        signed(&owner, &coordinate, &record),
        "is not refs/heads/main",
    )
    .await;

    // The draft stays open: the fold saw no record.
    let seen = query(&mut owner_client, "owner", draft_filter(&coordinate)).await;
    let events: Vec<DraftFoldEvent> = seen.iter().map(DraftFoldEvent::from).collect();
    let digest = fold_agents_repo_drafts(&coordinate, &repo, &events);
    assert_eq!(digest.paths.len(), 1);
    assert!(digest.commits.is_empty());
}
