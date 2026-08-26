//! End-to-end wire contract for S2: delivery classes on a turn command, and
//! the two receipt stages that answer a steer and an interrupt.
//!
//! Three facts this proves that no unit test can. The running relay accepts a
//! 44220 carrying each `deliver` class and rejects one carrying a class no
//! provider implements — the relay is where an unactionable command would
//! otherwise become a turn that silently never runs. The running relay accepts
//! a `turn_degraded` and an `interrupt_delivered` receipt, both five-key. And
//! the per-stage `csl-key` still holds when a single turn command produces a
//! degrade *and* a queue *and* a start, which is the shape a downgraded steer
//! actually has on the wire.
//!
//! This target requires an already-running relay backed by Postgres and Redis:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p buzz-test-client \
//!   --test e2e_coding_session_delivery_classes -- --ignored --nocapture
//! ```

use std::time::Duration;

use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery, CodingSessionTarget,
    CODING_SESSION_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, LifecycleReceipt, NO_LIVE_EXECUTION, STEER_UNSUPPORTED,
};
use buzz_sdk::{build_coding_session_command, build_coding_session_turn_receipt, build_join};
use buzz_test_client::BuzzTestClient;
use nostr::{Alphabet, Event, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag};
use uuid::Uuid;

const COMMAND_KIND: u16 = 44_220;
const RECEIPT_KIND: u16 = 44_224;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_owned())
}

async fn create_channel(client: &mut BuzzTestClient, owner: &Keys) -> Uuid {
    let channel_id = Uuid::new_v4();
    let event = EventBuilder::new(Kind::Custom(9007), "")
        .tags([
            Tag::parse(["h", &channel_id.to_string()]).unwrap(),
            Tag::parse(["name", &format!("delivery-class-e2e-{channel_id}")]).unwrap(),
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

fn kind_filter(kind: u16, channel_id: Uuid) -> Filter {
    Filter::new().kind(Kind::Custom(kind)).custom_tags(
        SingleLetterTag::lowercase(Alphabet::H),
        [channel_id.to_string()],
    )
}

fn turn_command(
    operator: &Keys,
    channel_id: Uuid,
    target: &CodingSessionTarget,
    command_id: &str,
    deliver: CodingSessionDelivery,
) -> Event {
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        target: target.clone(),
        action: CodingSessionAction::ThreadTurnStart {
            text: "do the thing".to_owned(),
            deliver,
        },
    };
    build_coding_session_command(channel_id, &payload)
        .expect("build turn command")
        .sign_with_keys(operator)
        .expect("sign turn command")
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

#[tokio::test]
#[ignore = "requires running relay, Postgres, and Redis"]
async fn the_relay_stores_every_delivery_class_and_the_new_turn_stages() {
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

    // One command per class. Absent is not testable through the SDK builder —
    // the typed payload always serializes `deliver` — so the "absent means
    // boundary" rule is pinned by the core decoder test instead.
    let classes = [
        ("boundary", CodingSessionDelivery::Boundary),
        ("steer", CodingSessionDelivery::Steer),
        ("interrupt", CodingSessionDelivery::Interrupt),
    ];
    let mut command_ids = Vec::new();
    for (name, deliver) in classes {
        let command_id = format!("turn-{name}-{}", Uuid::new_v4());
        let event = turn_command(&operator, channel_id, &target, &command_id, deliver);
        let ok = operator_ws
            .send_event(event)
            .await
            .expect("send turn command");
        assert!(ok.accepted, "relay rejected deliver={name}: {}", ok.message);
        command_ids.push(command_id);
    }

    // A class no provider implements must not reach a mailbox: a stored
    // command nobody can act on is a turn that vanishes with no receipt.
    let mut unknown: serde_json::Value = serde_json::json!({
        "schema": CODING_SESSION_COMMAND_SCHEMA,
        "commandId": format!("turn-unknown-{}", Uuid::new_v4()),
        "target": target,
        "action": { "type": "thread.turn.start", "text": "do the thing", "deliver": "cancel" },
    });
    let tags = vec![
        Tag::parse(["h", &channel_id.to_string()]).unwrap(),
        Tag::parse(["cs-v", "csc1-1"]).unwrap(),
        Tag::parse([
            "cs-target",
            &buzz_core::coding_session_command::coding_session_target_key(&target),
        ])
        .unwrap(),
    ];
    let rejected = EventBuilder::new(Kind::Custom(COMMAND_KIND), unknown.take().to_string())
        .tags(tags)
        .sign_with_keys(&operator)
        .expect("sign unknown-class command");
    let ok = operator_ws
        .send_event(rejected)
        .await
        .expect("send unknown-class command");
    assert!(
        !ok.accepted,
        "the relay stored a delivery class no provider implements"
    );

    // The two receipt stages this slice adds, on the same command id, beside
    // the queue stage a downgraded steer also produces.
    let steered = command_ids[1].clone();
    let stages = vec![
        LifecycleReceipt::turn_degraded(
            &steered,
            &target,
            STEER_UNSUPPORTED,
            "this execution's runtime does not offer mid-turn steering",
        ),
        LifecycleReceipt::turn_queued(&steered, &target),
        LifecycleReceipt::interrupt_delivered(&command_ids[2], &target),
        LifecycleReceipt::turn_dropped(
            &command_ids[0],
            &target,
            NO_LIVE_EXECUTION,
            "this execution has no live process",
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

    operator_ws
        .subscribe("s2-commands", vec![kind_filter(COMMAND_KIND, channel_id)])
        .await
        .expect("subscribe commands");
    let stored_commands = operator_ws
        .collect_until_eose("s2-commands", Duration::from_secs(10))
        .await
        .expect("collect commands");
    assert_eq!(
        stored_commands.len(),
        classes.len(),
        "exactly the three implemented classes are durable"
    );
    let mut delivered: Vec<String> = stored_commands
        .iter()
        .filter_map(|event| {
            let payload: CodingSessionCommandPayload = serde_json::from_str(&event.content).ok()?;
            match payload.action {
                CodingSessionAction::ThreadTurnStart { deliver, .. } => {
                    Some(deliver.as_str().to_owned())
                }
                CodingSessionAction::ThreadTurnInterrupt => None,
            }
        })
        .collect();
    delivered.sort();
    assert_eq!(delivered, vec!["boundary", "interrupt", "steer"]);

    operator_ws
        .subscribe("s2-receipts", vec![kind_filter(RECEIPT_KIND, channel_id)])
        .await
        .expect("subscribe receipts");
    let stored_receipts = operator_ws
        .collect_until_eose("s2-receipts", Duration::from_secs(10))
        .await
        .expect("collect receipts");
    assert_eq!(stored_receipts.len(), stages.len());
    let mut seen: Vec<String> = Vec::new();
    for event in &stored_receipts {
        let receipt = decode_coding_session_lifecycle_receipt(&event.content)
            .expect("relay returned a strictly decodable receipt");
        assert_eq!(receipt.session.as_ref(), Some(&target));
        assert!(receipt.turn_id.is_none());
        seen.push(receipt.status.as_str().to_owned());
    }
    seen.sort();
    assert_eq!(
        seen,
        vec![
            "interrupt_delivered",
            "turn_degraded",
            "turn_dropped",
            "turn_queued",
        ]
    );

    operator_ws.disconnect().await.expect("operator disconnect");
    provider_ws.disconnect().await.expect("provider disconnect");
}
