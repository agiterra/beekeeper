//! End-to-end wire contract for per-stage coding-session turn receipts.
//!
//! Two facts this proves that no unit test can: the relay accepts a
//! `turn_started` receipt's six-key content (the relay parses no
//! provider-authored content, but "we read the code" is a weaker claim than
//! "the running relay took it"), and the per-stage `csl-key` really does let
//! every stage of one turn command coexist as its own durable event rather
//! than collapsing into one.
//!
//! This target requires an already-running relay backed by Postgres and Redis:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p beekeeper-test-client \
//!   --test e2e_coding_session_turn_receipts -- --ignored --nocapture
//! ```

use std::time::Duration;

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, user_prompt_item, LifecycleReceipt, ReceiptStatus,
    TranscriptEnvelope, QUEUE_FULL, STALE_GENERATION,
};
use beekeeper_sdk::{
    build_coding_session_transcript_item, build_coding_session_turn_receipt, build_join,
};
use beekeeper_test_client::BuzzTestClient;
use nostr::{Alphabet, Event, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag};
use uuid::Uuid;

const RECEIPT_KIND: u16 = 44_224;
const TRANSCRIPT_KIND: u16 = 44_225;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_owned())
}

async fn create_channel(client: &mut BuzzTestClient, owner: &Keys) -> Uuid {
    let channel_id = Uuid::new_v4();
    let event = EventBuilder::new(Kind::Custom(9007), "")
        .tags([
            Tag::parse(["h", &channel_id.to_string()]).unwrap(),
            Tag::parse(["name", &format!("turn-receipt-e2e-{channel_id}")]).unwrap(),
            Tag::parse(["channel_type", "stream"]).unwrap(),
            Tag::parse(["visibility", "open"]).unwrap(),
        ])
        .sign_with_keys(owner)
        .unwrap();
    let ok = client.send_event(event).await.expect("create channel");
    assert!(ok.accepted, "channel creation rejected: {}", ok.message);
    channel_id
}

async fn join_channel(client: &mut BuzzTestClient, keys: &Keys, channel_id: Uuid) {
    let event = build_join(channel_id)
        .expect("build join")
        .sign_with_keys(keys)
        .expect("sign join");
    let ok = client.send_event(event).await.expect("join channel");
    assert!(ok.accepted, "channel join rejected: {}", ok.message);
}

fn receipt_event(provider: &Keys, channel_id: Uuid, receipt: &LifecycleReceipt) -> Event {
    build_coding_session_turn_receipt(
        channel_id,
        &receipt.command_id,
        receipt.status,
        &serde_json::to_string(receipt).expect("serialize receipt"),
    )
    .expect("build turn receipt")
    .sign_with_keys(provider)
    .expect("sign turn receipt")
}

fn kind_filter(kind: u16, channel_id: Uuid) -> Filter {
    Filter::new().kind(Kind::Custom(kind)).custom_tags(
        SingleLetterTag::lowercase(Alphabet::H),
        [channel_id.to_string()],
    )
}

#[tokio::test]
#[ignore = "requires running relay, Postgres, and Redis"]
async fn every_stage_of_one_turn_survives_the_relay_as_its_own_receipt() {
    let url = relay_url();
    let operator = Keys::generate();
    let provider = Keys::generate();
    let mut operator_ws = BuzzTestClient::connect(&url, &operator)
        .await
        .expect("operator connect");
    let mut provider_ws = BuzzTestClient::connect(&url, &provider)
        .await
        .expect("provider connect");

    let channel_id = create_channel(&mut operator_ws, &operator).await;
    join_channel(&mut provider_ws, &provider, channel_id).await;

    let target = CodingSessionTarget {
        driver: "claude-agent-acp".to_owned(),
        instance_id: "instance-1".to_owned(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let command_id = format!("turn-{}", Uuid::new_v4());
    let turn_id = Uuid::new_v4().to_string();

    // One turn command, four stages. `turn_dropped` and `turn_refused` would
    // not follow a `turn_started` in real life; they ride along here because
    // the point under test is that the key of each stage is its own.
    let stages = vec![
        LifecycleReceipt::turn_queued(&command_id, &target),
        LifecycleReceipt::turn_started(&command_id, &target, &turn_id),
        LifecycleReceipt::turn_dropped(
            &command_id,
            &target,
            QUEUE_FULL,
            "the execution's queue is full",
        ),
        LifecycleReceipt::turn_refused(
            &command_id,
            &target,
            STALE_GENERATION,
            "the addressed generation has been superseded",
        ),
    ];
    for receipt in &stages {
        let event = receipt_event(&provider, channel_id, receipt);
        let ok = provider_ws
            .send_event(event)
            .await
            .expect("send turn receipt");
        assert!(
            ok.accepted,
            "relay rejected a {} receipt: {}",
            receipt.status.as_str(),
            ok.message
        );
    }

    // The echo that makes the receipt joinable to the transcript.
    let envelope = TranscriptEnvelope::new(
        &target,
        1,
        0,
        Some(&turn_id),
        user_prompt_item("do the thing", false, None, Some(&command_id), None, 0),
    );
    let item = build_coding_session_transcript_item(
        channel_id,
        &target,
        1,
        &serde_json::to_string(&envelope).expect("serialize envelope"),
    )
    .expect("build transcript item")
    .sign_with_keys(&provider)
    .expect("sign transcript item");
    let ok = provider_ws
        .send_event(item)
        .await
        .expect("send transcript item");
    assert!(ok.accepted, "relay rejected the echo: {}", ok.message);

    operator_ws
        .subscribe("turn-receipts", vec![kind_filter(RECEIPT_KIND, channel_id)])
        .await
        .expect("subscribe receipts");
    let stored = operator_ws
        .collect_until_eose("turn-receipts", Duration::from_secs(10))
        .await
        .expect("collect receipts");

    // Four stages in, four stages out. The relay does not dedupe by `csl-key`
    // — the producer's outbox does — so what this proves is the other half:
    // four distinct stage receipts for one command are all admissible, so a
    // consumer really can read a turn's whole history off the wire.
    assert_eq!(
        stored.len(),
        stages.len(),
        "expected one durable event per stage, got {:?}",
        stored
            .iter()
            .map(|event| event.content.clone())
            .collect::<Vec<_>>()
    );

    let mut seen: Vec<String> = Vec::new();
    for event in &stored {
        let receipt = decode_coding_session_lifecycle_receipt(&event.content)
            .expect("relay returned a strictly decodable receipt");
        assert_eq!(receipt.command_id, command_id);
        assert_eq!(receipt.session.as_ref(), Some(&target));
        seen.push(receipt.status.as_str().to_owned());
        match receipt.status {
            ReceiptStatus::TurnStarted => assert_eq!(receipt.turn_id.as_deref(), Some(&*turn_id)),
            ReceiptStatus::TurnDropped => {
                assert!(receipt.turn_id.is_none());
                assert_eq!(receipt.error.as_ref().expect("error").code, QUEUE_FULL);
            }
            _ => assert!(receipt.turn_id.is_none()),
        }
    }
    seen.sort();
    assert_eq!(
        seen,
        vec![
            "turn_dropped",
            "turn_queued",
            "turn_refused",
            "turn_started"
        ]
    );

    operator_ws
        .subscribe("turn-echo", vec![kind_filter(TRANSCRIPT_KIND, channel_id)])
        .await
        .expect("subscribe transcript");
    let echoes = operator_ws
        .collect_until_eose("turn-echo", Duration::from_secs(10))
        .await
        .expect("collect transcript");
    assert_eq!(echoes.len(), 1);
    let echoed: serde_json::Value =
        serde_json::from_str(&echoes[0].content).expect("decode envelope");
    assert_eq!(echoed["item"]["commandId"], command_id.as_str());
    assert_eq!(echoed["turnId"], turn_id.as_str());

    operator_ws.disconnect().await.expect("operator disconnect");
    provider_ws.disconnect().await.expect("provider disconnect");
}
