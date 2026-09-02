//! Live end-to-end tests for the coding-session GENESIS contract (kind 44226).
//!
//! These tests require a running relay instance and exercise the genesis
//! contract exactly as a real founder/provider pair would: fresh founding,
//! `sessionRef` uniqueness, and explicit legacy adoption of pre-genesis
//! `session.create`/receipt history. See `crates/buzz-core/src/coding_session_genesis.rs`
//! for the full contract this test demos.
//!
//! # Running
//!
//! Start the relay, then run:
//!
//! ```text
//! cargo test -p buzz-test-client --test e2e_genesis -- --ignored --nocapture
//! ```
//!
//! Override the relay URL with the `RELAY_URL` environment variable.

use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
use buzz_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::LifecycleReceipt;
use buzz_sdk::{
    build_coding_session_genesis, build_coding_session_lifecycle_command,
    build_coding_session_lifecycle_receipt, build_join,
};
use buzz_test_client::BuzzTestClient;
use nostr::{EventBuilder, Keys, Kind, Tag};
use std::time::Duration;
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
/// Mirrors `e2e_relay.rs`'s helper of the same name.
async fn create_test_channel(keys: &Keys) -> Uuid {
    let client = reqwest::Client::new();
    let pubkey_hex = keys.public_key().to_hex();
    let channel_uuid = Uuid::new_v4();
    let channel_name = format!("genesis-e2e-{}", channel_uuid);

    let event = EventBuilder::new(Kind::Custom(9007), "")
        .tags(vec![
            Tag::parse(["h", &channel_uuid.to_string()]).unwrap(),
            Tag::parse(["name", &channel_name]).unwrap(),
            Tag::parse(["channel_type", "stream"]).unwrap(),
            Tag::parse(["visibility", "open"]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap();

    let resp = client
        .post(format!("{}/events", relay_http_url()))
        .header("X-Pubkey", &pubkey_hex)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&event).unwrap())
        .send()
        .await
        .expect("submit create-channel event");
    assert!(
        resp.status().is_success(),
        "channel creation event failed: {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.expect("parse event response");
    assert!(
        body["accepted"].as_bool().unwrap_or(false),
        "channel creation not accepted: {}",
        body
    );

    channel_uuid
}

/// Join an open channel via a signed kind:9021 (NIP-29 join-request) event over
/// the WebSocket connection. Coding-session kinds require strict active
/// membership, so every non-founder participant in these tests joins first.
async fn join_channel(ws: &mut BuzzTestClient, keys: &Keys, channel_id: Uuid) {
    let event = build_join(channel_id)
        .expect("build join event")
        .sign_with_keys(keys)
        .expect("sign join event");
    let ok = ws.send_event(event).await.expect("send join event");
    assert!(ok.accepted, "join request rejected: {}", ok.message);
}

/// End-to-end demo: a founder founds a fresh `sessionRef` via genesis; a rival
/// member cannot found the same reference, and neither can the founder found
/// it a second time. Uniqueness is enforced per `(channel, sessionRef)` by the
/// relay's storage transaction, not by event identity.
#[tokio::test]
#[ignore]
async fn genesis_fresh_founding_is_unique_live() {
    let url = relay_url();
    let founder = Keys::generate();
    let channel_id = create_test_channel(&founder).await;

    let mut founder_ws = BuzzTestClient::connect(&url, &founder)
        .await
        .expect("founder connect");

    let session_ref = Uuid::new_v4().to_string();

    // 1. Founder publishes a fresh genesis for a brand-new sessionRef.
    let genesis_payload = CodingSessionGenesisPayload::new(session_ref.clone());
    let genesis_event = build_coding_session_genesis(channel_id, &genesis_payload)
        .expect("build genesis")
        .sign_with_keys(&founder)
        .expect("sign genesis");
    let founding_id = genesis_event.id.to_hex();
    let ok = founder_ws
        .send_event(genesis_event)
        .await
        .expect("send fresh genesis");
    assert!(
        ok.accepted,
        "fresh founding genesis should be accepted: {}",
        ok.message
    );
    println!("ACCEPTED fresh genesis {founding_id}");

    // 2. A second member joins the channel, then attempts a rival genesis for
    //    the SAME sessionRef — must lose the uniqueness race.
    let rival = Keys::generate();
    let mut rival_ws = BuzzTestClient::connect(&url, &rival)
        .await
        .expect("rival connect");
    join_channel(&mut rival_ws, &rival, channel_id).await;

    let rival_payload = CodingSessionGenesisPayload::new(session_ref.clone());
    let rival_event = build_coding_session_genesis(channel_id, &rival_payload)
        .expect("build rival genesis")
        .sign_with_keys(&rival)
        .expect("sign rival genesis");
    let rival_ok = rival_ws
        .send_event(rival_event)
        .await
        .expect("send rival genesis");
    assert!(
        !rival_ok.accepted,
        "rival genesis over an already-founded sessionRef must be rejected"
    );
    assert!(
        rival_ok.message.contains("duplicate"),
        "rejection should mention 'duplicate', got: {}",
        rival_ok.message
    );
    assert!(
        rival_ok.message.contains(&founding_id),
        "rejection should name the winning founder event {founding_id}, got: {}",
        rival_ok.message
    );
    println!("REJECTED rival genesis: {}", rival_ok.message);

    // 3. The founder cannot found the same sessionRef a second time either —
    //    uniqueness is per (channel, sessionRef), not per signer.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let refound_payload = CodingSessionGenesisPayload::new(session_ref.clone());
    let refound_event = build_coding_session_genesis(channel_id, &refound_payload)
        .expect("build second founder genesis")
        .sign_with_keys(&founder)
        .expect("sign second founder genesis");
    assert_ne!(
        refound_event.id.to_hex(),
        founding_id,
        "the second attempt must be a distinct event for this to be a meaningful uniqueness test"
    );
    let refound_ok = founder_ws
        .send_event(refound_event)
        .await
        .expect("send second founder genesis");
    assert!(
        !refound_ok.accepted,
        "founder re-founding the same sessionRef with a new event must also be rejected"
    );
    assert!(
        refound_ok.message.contains("duplicate"),
        "rejection should mention 'duplicate', got: {}",
        refound_ok.message
    );
    println!("REJECTED founder re-founding: {}", refound_ok.message);

    founder_ws.disconnect().await.expect("founder disconnect");
    rival_ws.disconnect().await.expect("rival disconnect");
}

/// End-to-end demo: legacy pre-genesis history (a `session.create` +
/// answering lifecycle receipt) cannot be silently claimed by a plain
/// genesis, must be adopted with the correct create/receipt reference, and
/// only the create's actual signer (the founder) may successfully adopt it.
#[tokio::test]
#[ignore]
async fn genesis_adoption_validates_referenced_history_live() {
    let url = relay_url();
    let founder = Keys::generate();
    let channel_id = create_test_channel(&founder).await;

    let mut founder_ws = BuzzTestClient::connect(&url, &founder)
        .await
        .expect("founder connect");

    // A separate provider identity, made a channel member so its receipt is
    // accepted by the relay's strict coding-session membership gate.
    let provider = Keys::generate();
    let provider_pubkey_hex = provider.public_key().to_hex();
    let mut provider_ws = BuzzTestClient::connect(&url, &provider)
        .await
        .expect("provider connect");
    join_channel(&mut provider_ws, &provider, channel_id).await;

    let session_ref = Uuid::new_v4().to_string();
    let command_id = format!("create-{}", Uuid::new_v4());

    // 1. Founder publishes a legacy-style 44221 session.create claiming the
    //    sessionRef and naming the provider as its authority.
    let create_payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_string(),
        command_id: command_id.clone(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(session_ref.clone()),
            genesis_ref: None,
            provider_instance_ref: "claude-primary".to_string(),
            provider_authority_pubkey: provider_pubkey_hex.clone(),
            model: None,
            title: None,
            initial_turn: None,
            actor: None,
            role: None,
            hire_ref: None,
            routing: None,
        },
    };
    let create_event = build_coding_session_lifecycle_command(channel_id, &create_payload)
        .expect("build session.create")
        .sign_with_keys(&founder)
        .expect("sign session.create");
    let create_event_id = create_event.id.to_hex();
    let create_ok = founder_ws
        .send_event(create_event)
        .await
        .expect("send session.create");
    assert!(
        create_ok.accepted,
        "legacy session.create should be accepted: {}",
        create_ok.message
    );
    println!("ACCEPTED legacy session.create {create_event_id}");

    // 2. Provider publishes a 44224 lifecycle receipt that genuinely joins the
    //    create: same commandId, minted session target, signed by the
    //    provider authority the create named.
    let target = CodingSessionTarget {
        driver: "claude".to_string(),
        instance_id: "claude-primary".to_string(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let receipt = LifecycleReceipt::created(&command_id, &target);
    let receipt_content = serde_json::to_string(&receipt).expect("serialize receipt");
    let receipt_event =
        build_coding_session_lifecycle_receipt(channel_id, &command_id, &receipt_content)
            .expect("build lifecycle receipt")
            .sign_with_keys(&provider)
            .expect("sign lifecycle receipt");
    let receipt_event_id = receipt_event.id.to_hex();
    let receipt_ok = provider_ws
        .send_event(receipt_event)
        .await
        .expect("send lifecycle receipt");
    assert!(
        receipt_ok.accepted,
        "lifecycle receipt should be accepted: {}",
        receipt_ok.message
    );
    println!("ACCEPTED lifecycle receipt {receipt_event_id}");

    // 3a. A PLAIN genesis (no adopts) over a sessionRef that legacy history
    //     already uses must be rejected — the relay never infers a founder.
    let plain_payload = CodingSessionGenesisPayload::new(session_ref.clone());
    let plain_event = build_coding_session_genesis(channel_id, &plain_payload)
        .expect("build plain genesis")
        .sign_with_keys(&founder)
        .expect("sign plain genesis");
    let plain_ok = founder_ws
        .send_event(plain_event)
        .await
        .expect("send plain genesis");
    assert!(
        !plain_ok.accepted,
        "a plain genesis over sessionRef with legacy history must be rejected"
    );
    assert!(
        plain_ok.message.contains("adopts"),
        "rejection should point at the missing adopts reference, got: {}",
        plain_ok.message
    );
    println!(
        "REJECTED plain genesis over legacy history: {}",
        plain_ok.message
    );

    // 3b. A DIFFERENT member's adoption genesis, correctly referencing the
    //     create+receipt, must be rejected: the genesis signer must equal the
    //     create's actual signer (the founder), not merely name valid events.
    let outsider = Keys::generate();
    let mut outsider_ws = BuzzTestClient::connect(&url, &outsider)
        .await
        .expect("outsider connect");
    join_channel(&mut outsider_ws, &outsider, channel_id).await;

    let outsider_adoption = CodingSessionGenesisPayload::new_adoption(
        session_ref.clone(),
        create_event_id.clone(),
        receipt_event_id.clone(),
    );
    let outsider_event = build_coding_session_genesis(channel_id, &outsider_adoption)
        .expect("build outsider adoption genesis")
        .sign_with_keys(&outsider)
        .expect("sign outsider adoption genesis");
    let outsider_ok = outsider_ws
        .send_event(outsider_event)
        .await
        .expect("send outsider adoption genesis");
    assert!(
        !outsider_ok.accepted,
        "an adoption genesis signed by someone other than the create's signer must be rejected"
    );
    assert!(
        outsider_ok.message.contains("signer"),
        "rejection should mention the signer mismatch, got: {}",
        outsider_ok.message
    );
    println!(
        "REJECTED outsider adoption (signer mismatch): {}",
        outsider_ok.message
    );

    // 3c. The founder's own adoption genesis, referencing the same
    //     create+receipt, must be ACCEPTED.
    let founder_adoption = CodingSessionGenesisPayload::new_adoption(
        session_ref.clone(),
        create_event_id.clone(),
        receipt_event_id.clone(),
    );
    let founder_adoption_event = build_coding_session_genesis(channel_id, &founder_adoption)
        .expect("build founder adoption genesis")
        .sign_with_keys(&founder)
        .expect("sign founder adoption genesis");
    let founder_adoption_id = founder_adoption_event.id.to_hex();
    let founder_adoption_ok = founder_ws
        .send_event(founder_adoption_event)
        .await
        .expect("send founder adoption genesis");
    assert!(
        founder_adoption_ok.accepted,
        "the founder's own adoption genesis should be accepted: {}",
        founder_adoption_ok.message
    );
    println!("ACCEPTED founder adoption genesis {founder_adoption_id}");

    // 3d. After acceptance, the sessionRef is founded: a rival adoption (or a
    //     fresh genesis) for it must be rejected on uniqueness grounds.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let rival_adoption = CodingSessionGenesisPayload::new_adoption(
        session_ref.clone(),
        create_event_id.clone(),
        receipt_event_id.clone(),
    );
    let rival_adoption_event = build_coding_session_genesis(channel_id, &rival_adoption)
        .expect("build rival adoption genesis")
        .sign_with_keys(&outsider)
        .expect("sign rival adoption genesis");
    let rival_adoption_ok = outsider_ws
        .send_event(rival_adoption_event)
        .await
        .expect("send rival adoption genesis");
    assert!(
        !rival_adoption_ok.accepted,
        "a second adoption attempt over an already-founded sessionRef must be rejected"
    );
    assert!(
        rival_adoption_ok.message.contains("duplicate"),
        "the uniqueness probe runs before adoption verification, so even a signer-mismatched \
         rival must be refused as a duplicate, not an invalid adoption — got: {}",
        rival_adoption_ok.message
    );
    println!(
        "REJECTED rival adoption after founding: {}",
        rival_adoption_ok.message
    );

    let fresh_after_founding = CodingSessionGenesisPayload::new(session_ref.clone());
    let fresh_after_founding_event =
        build_coding_session_genesis(channel_id, &fresh_after_founding)
            .expect("build fresh genesis after founding")
            .sign_with_keys(&founder)
            .expect("sign fresh genesis after founding");
    let fresh_after_founding_ok = founder_ws
        .send_event(fresh_after_founding_event)
        .await
        .expect("send fresh genesis after founding");
    assert!(
        !fresh_after_founding_ok.accepted,
        "a fresh (non-adopting) genesis for an already-founded sessionRef must be rejected"
    );
    assert!(
        fresh_after_founding_ok.message.contains("duplicate"),
        "rejection should mention 'duplicate', got: {}",
        fresh_after_founding_ok.message
    );
    println!(
        "REJECTED fresh genesis after founding: {}",
        fresh_after_founding_ok.message
    );

    founder_ws.disconnect().await.expect("founder disconnect");
    provider_ws.disconnect().await.expect("provider disconnect");
    outsider_ws.disconnect().await.expect("outsider disconnect");
}

/// End-to-end bootstrap happy path: a fresh genesis is accepted first, then
/// the founder's create explicitly names that exact event id. This is the
/// only authority join consumers and providers may follow; the diagnostic
/// `csg-session` tag is deliberately not used for selection.
#[tokio::test]
#[ignore]
async fn genesis_is_linked_from_session_create_live() {
    let url = relay_url();
    let founder = Keys::generate();
    let channel_id = create_test_channel(&founder).await;
    let mut founder_ws = BuzzTestClient::connect(&url, &founder)
        .await
        .expect("founder connect");

    let provider = Keys::generate();
    let mut provider_ws = BuzzTestClient::connect(&url, &provider)
        .await
        .expect("provider connect");
    join_channel(&mut provider_ws, &provider, channel_id).await;

    let session_ref = Uuid::new_v4().to_string();
    let genesis = build_coding_session_genesis(
        channel_id,
        &CodingSessionGenesisPayload::new(session_ref.clone()),
    )
    .expect("build genesis")
    .sign_with_keys(&founder)
    .expect("sign genesis");
    let genesis_ref = genesis.id.to_hex();
    let genesis_ok = founder_ws.send_event(genesis).await.expect("send genesis");
    assert!(
        genesis_ok.accepted,
        "founding genesis should be accepted: {}",
        genesis_ok.message
    );

    let command_id = format!("create-{}", Uuid::new_v4());
    let create_payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_string(),
        command_id: command_id.clone(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(session_ref),
            genesis_ref: Some(genesis_ref.clone()),
            provider_instance_ref: "claude-primary".to_string(),
            provider_authority_pubkey: provider.public_key().to_hex(),
            model: None,
            title: Some("Genesis link E2E".to_string()),
            initial_turn: None,
            actor: None,
            role: None,
            hire_ref: None,
            routing: None,
        },
    };
    let create = build_coding_session_lifecycle_command(channel_id, &create_payload)
        .expect("build genesis-bearing create")
        .sign_with_keys(&founder)
        .expect("sign genesis-bearing create");
    let create_ok = founder_ws
        .send_event(create)
        .await
        .expect("send genesis-bearing create");
    assert!(
        create_ok.accepted,
        "create linked to accepted genesis {genesis_ref} should be accepted: {}",
        create_ok.message
    );

    founder_ws.disconnect().await.expect("founder disconnect");
    provider_ws.disconnect().await.expect("provider disconnect");
}
