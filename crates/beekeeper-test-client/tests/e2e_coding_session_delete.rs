//! Live end-to-end test for deleting a whole coding session (NIP-09 `kind:5`).
//!
//! This exists because the feature shipped with tests that never crossed
//! ingest. `validate_standard_deletion_event` was exercised directly and
//! agreed a session could be deleted; ingest's own single-target rule, three
//! screens further down the same function, then refused every real one with
//!
//! ```text
//! invalid: deletion events must reference exactly one target via e or a tag (got e=26, a=0)
//! ```
//!
//! A unit test on the validator cannot see that, which is the whole point of
//! putting this one on the wire: the assertion is the relay's own OK message
//! for an event a real client would build.
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! cargo test -p beekeeper-test-client --test e2e_coding_session_delete -- --ignored --nocapture
//! ```
//!
//! Override the relay URL with the `RELAY_URL` environment variable.

use beekeeper_core::coding_session_closure::{
    CodingSessionClosureAction, CodingSessionClosurePayload,
};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload;
use beekeeper_sdk::{
    build_coding_session_closure, build_coding_session_genesis, build_coding_session_goal,
    build_coding_session_metadata, build_coding_session_transcript_item, build_join,
};
use beekeeper_test_client::BuzzTestClient;
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use uuid::Uuid;

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

/// Create a real channel via a signed kind:9007 event submitted to POST /events.
/// Mirrors `e2e_genesis.rs`'s helper of the same name.
async fn create_test_channel(keys: &Keys) -> Uuid {
    let client = reqwest::Client::new();
    let channel_uuid = Uuid::new_v4();
    let event = EventBuilder::new(Kind::Custom(9007), "")
        .tags(vec![
            Tag::parse(["h", &channel_uuid.to_string()]).unwrap(),
            Tag::parse(["name", &format!("session-delete-e2e-{channel_uuid}")]).unwrap(),
            Tag::parse(["channel_type", "stream"]).unwrap(),
            Tag::parse(["visibility", "open"]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap();

    let resp = client
        .post(format!("{}/events", relay_http_url()))
        .header("X-Pubkey", keys.public_key().to_hex())
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&event).unwrap())
        .send()
        .await
        .expect("submit create-channel event");
    let body: serde_json::Value = resp.json().await.expect("parse event response");
    assert!(
        body["accepted"].as_bool().unwrap_or(false),
        "channel creation not accepted: {body}"
    );
    channel_uuid
}

async fn join_channel(ws: &mut BuzzTestClient, keys: &Keys, channel_id: Uuid) {
    let event = build_join(channel_id)
        .expect("build join event")
        .sign_with_keys(keys)
        .expect("sign join event");
    let ok = ws.send_event(event).await.expect("send join event");
    assert!(ok.accepted, "join request rejected: {}", ok.message);
}

/// Publish one event and assert the relay took it, returning its id.
async fn publish(ws: &mut BuzzTestClient, event: Event, what: &str) -> String {
    let id = event.id.to_hex();
    let ok = ws.send_event(event).await.expect("send event");
    assert!(ok.accepted, "{what} was not accepted: {}", ok.message);
    id
}

fn target(session_id: &str) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude".to_string(),
        instance_id: "instance-e2e".to_string(),
        session_id: session_id.to_string(),
        generation: 1,
    }
}

/// A whole session — genesis, goal, closure, and the provider's metadata and
/// transcript — deleted by its founder in one `kind:5`.
///
/// Every kind a real session owns is present, and the two the founder did not
/// sign are present on purpose: a session's metadata and transcript are signed
/// by the host that ran it, so this deletion could never pass an
/// authorship-only rule. Six targets in one event is also five more than the
/// gate this test exists for used to allow.
#[tokio::test]
#[ignore = "requires running relay"]
async fn a_founder_deletes_a_whole_session_live() {
    let url = relay_url();
    let founder = Keys::generate();
    let provider = Keys::generate();
    let channel_id = create_test_channel(&founder).await;

    let mut founder_ws = BuzzTestClient::connect(&url, &founder)
        .await
        .expect("founder connect");
    let mut provider_ws = BuzzTestClient::connect(&url, &provider)
        .await
        .expect("provider connect");
    join_channel(&mut provider_ws, &provider, channel_id).await;

    let session_ref = Uuid::new_v4().to_string();
    let execution = target(&Uuid::new_v4().to_string());

    let genesis_id = publish(
        &mut founder_ws,
        build_coding_session_genesis(
            channel_id,
            &CodingSessionGenesisPayload::new(session_ref.clone()),
        )
        .expect("build genesis")
        .sign_with_keys(&founder)
        .expect("sign genesis"),
        "genesis",
    )
    .await;

    let goal_id = publish(
        &mut founder_ws,
        build_coding_session_goal(channel_id, &session_ref, "Fix the flaky timeout.")
            .expect("build goal")
            .sign_with_keys(&founder)
            .expect("sign goal"),
        "goal",
    )
    .await;

    let closure_id = publish(
        &mut founder_ws,
        build_coding_session_closure(
            channel_id,
            &CodingSessionClosurePayload::new(
                CodingSessionClosureAction::Closed,
                genesis_id.clone(),
                session_ref.clone(),
            ),
        )
        .expect("build closure")
        .sign_with_keys(&founder)
        .expect("sign closure"),
        "closure",
    )
    .await;

    let metadata_id = publish(
        &mut provider_ws,
        build_coding_session_metadata(
            channel_id,
            &execution,
            &serde_json::json!({ "sessionRef": session_ref, "v": 1 }).to_string(),
        )
        .expect("build metadata")
        .sign_with_keys(&provider)
        .expect("sign metadata"),
        "metadata",
    )
    .await;

    let transcript_id = publish(
        &mut provider_ws,
        build_coding_session_transcript_item(channel_id, &execution, 1, "{}")
            .expect("build transcript item")
            .sign_with_keys(&provider)
            .expect("sign transcript item"),
        "transcript item",
    )
    .await;

    let owned = [
        genesis_id,
        goal_id,
        closure_id,
        metadata_id.clone(),
        transcript_id.clone(),
    ];
    let deletion = EventBuilder::new(Kind::EventDeletion, format!("Delete session {session_ref}"))
        .tags(
            owned
                .iter()
                .map(|id| Tag::parse(["e", id]).expect("e tag"))
                .collect::<Vec<_>>(),
        )
        .sign_with_keys(&founder)
        .expect("sign deletion");

    let ok = founder_ws
        .send_event(deletion)
        .await
        .expect("send session deletion");
    assert!(
        ok.accepted,
        "a whole-session deletion naming {} targets must be accepted, got: {}",
        owned.len(),
        ok.message
    );
    println!("ACCEPTED session deletion over {} targets", owned.len());

    founder_ws.disconnect().await.expect("founder disconnect");
    provider_ws.disconnect().await.expect("provider disconnect");
}

/// The rule the exception is carved out of is still the rule.
///
/// Two ordinary messages, both the actor's own, named by one `kind:5`: no
/// genesis, so nothing authorized it as a session delete, so the single-target
/// gate applies exactly as it did before.
#[tokio::test]
#[ignore = "requires running relay"]
async fn an_ordinary_deletion_still_names_one_target_live() {
    let url = relay_url();
    let author = Keys::generate();
    let channel_id = create_test_channel(&author).await;
    let mut ws = BuzzTestClient::connect(&url, &author)
        .await
        .expect("author connect");

    let mut ids = Vec::new();
    for index in 0..2 {
        let message = EventBuilder::new(
            Kind::Custom(beekeeper_core::kind::KIND_STREAM_MESSAGE_V2 as u16),
            format!("message {index}"),
        )
        .tags(vec![Tag::parse(["h", &channel_id.to_string()]).unwrap()])
        .sign_with_keys(&author)
        .expect("sign message");
        ids.push(publish(&mut ws, message, "message").await);
    }

    let deletion = EventBuilder::new(Kind::EventDeletion, "")
        .tags(
            ids.iter()
                .map(|id| Tag::parse(["e", id]).expect("e tag"))
                .collect::<Vec<_>>(),
        )
        .sign_with_keys(&author)
        .expect("sign deletion");

    let ok = ws.send_event(deletion).await.expect("send deletion");
    assert!(
        !ok.accepted,
        "a two-target deletion that is not a session delete must still be refused"
    );
    assert!(
        ok.message.contains("exactly one target"),
        "the single-target rule must be what refuses it, got: {}",
        ok.message
    );
    println!("REJECTED ordinary multi-target deletion: {}", ok.message);

    ws.disconnect().await.expect("disconnect");
}
