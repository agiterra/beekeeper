use super::*;

use std::collections::{HashMap, HashSet};

use beekeeper_core::coding_session_command::{CodingSessionAction, CodingSessionCommandPayload};

use crate::team_wake::{DiscoveryCapture, WakeIntentStore, WakeScope, WakeSource};

fn pressure_scope(channel_ref: Uuid) -> WakeScope {
    WakeScope {
        channel_ref,
        session_ref: channel_ref.to_string(),
        genesis_ref: "ab".repeat(32),
    }
}

fn pressure_report(value: usize, created_at: u64) -> WakeSource {
    WakeSource::Report {
        operation_id: format!("{value:064x}"),
        operation_type: "assignment_report".into(),
        author_pubkey: format!("{:064x}", value.saturating_add(100_000)),
        created_at,
    }
}

/// v3.1 §6 pressure complement to the combined driver T0.
///
/// The combined test exercises full verified driver transitions at a bounded
/// fixture size. This test separately keeps the real RelayEventPublisher and
/// fake WebSocket EVENT/OK boundary under the exact 5,121-source load while
/// the production store crosses 4,096 permanent resolutions. Publication is
/// counted only from EVENT frames received by the endpoint; store selection
/// is never treated as publication evidence.
#[tokio::test]
async fn team_wake_pressure_resolves_5121_sources_through_real_publisher() {
    let dir = tempfile::tempdir().expect("tempdir");
    let keys = Keys::generate();
    let (relay, control, server) = spawn_recording_test_relay(&keys, Vec::new()).await;
    let publisher = relay.event_publisher();
    let channels = [
        Uuid::from_u128(0x101),
        Uuid::from_u128(0x102),
        Uuid::from_u128(0x103),
        Uuid::from_u128(0x104),
    ];
    let counts = [1_281_usize, 1_280, 1_280, 1_280];
    assert_eq!(counts.iter().sum::<usize>(), 5_121);

    let mut all = HashMap::<Uuid, Vec<WakeSource>>::new();
    let mut expected_sources = HashSet::new();
    let mut value = 1_usize;
    for (channel, count) in channels.into_iter().zip(counts) {
        let mut reports = Vec::with_capacity(count);
        for index in 0..count {
            let created_at = match (channel, index) {
                (candidate, 0) if candidate == channels[0] => 1,
                (candidate, 0) if candidate == channels[1] => u64::MAX,
                _ => value as u64,
            };
            let source = pressure_report(value, created_at);
            expected_sources.insert(source.event_id().to_owned());
            reports.push(source);
            value = value.saturating_add(1);
        }
        all.insert(channel, reports);
    }

    let target = CodingSessionTarget {
        driver: "pressure-acp".into(),
        instance_id: "pressure-provider".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let mut store = WakeIntentStore::open(dir.path()).expect("open pressure store");
    let mut scan = HashMap::<Uuid, usize>::new();
    let mut resolved = 0_usize;
    let mut restarted_after_first = false;
    let mut iteration = 0_usize;

    while resolved < expected_sources.len() {
        iteration = iteration.saturating_add(1);
        assert!(iteration < 100_000, "pressure publisher loop stalled");
        for channel in channels {
            let sources = all.get(&channel).expect("channel sources");
            let cursor = scan.entry(channel).or_default();
            while *cursor < sources.len() {
                match store
                    .capture_report(pressure_scope(channel), sources[*cursor].clone())
                    .expect("capture pressure source")
                {
                    DiscoveryCapture::Admitted | DiscoveryCapture::Duplicate => {
                        *cursor = cursor.saturating_add(1);
                    }
                    DiscoveryCapture::Saturated => break,
                    DiscoveryCapture::ResolvedLedgerFull | DiscoveryCapture::Refused(_) => {
                        panic!("pressure fixture crossed the inherited truth envelope")
                    }
                }
            }
        }

        let Some(channel) = store
            .next_tick_channel(channels)
            .expect("select pressure channel")
        else {
            continue;
        };
        let Some(intent) = store
            .pending_for_channel(channel)
            .expect("promote pressure source")
        else {
            continue;
        };
        let command_id = crate::team_wake::command_id(intent.source.event_id(), &target, 0);
        let event = crate::team_wake::build_wake_event(
            &keys,
            &intent.scope,
            &intent.source,
            target.clone(),
            command_id,
        )
        .expect("build pressure wake");
        publisher
            .publish(event)
            .await
            .expect("fake relay accepts pressure wake");
        store
            .retire_in_flight(channel)
            .expect("resolve published pressure source");
        resolved = resolved.saturating_add(1);

        if !restarted_after_first {
            drop(store);
            store = WakeIntentStore::open(dir.path()).expect("restart after first publication");
            restarted_after_first = true;
        } else if resolved == 4_097 {
            drop(store);
            store = WakeIntentStore::open(dir.path()).expect("restart past v2 FIFO depth");
        }
    }

    let frames: Vec<Event> = control
        .published
        .lock()
        .expect("published lock")
        .iter()
        .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_COMMAND)
        .cloned()
        .collect();
    assert_eq!(frames.len(), 5_121);
    let actual_sources: HashSet<String> = frames
        .iter()
        .map(|event| {
            let payload: CodingSessionCommandPayload =
                serde_json::from_str(&event.content).expect("pressure wake payload");
            let CodingSessionAction::ThreadTurnStart { text, .. } = payload.action else {
                panic!("pressure wake is not a turn start");
            };
            serde_json::from_str::<serde_json::Value>(&text)
                .expect("pressure wake pointer")
                .get("operationId")
                .and_then(serde_json::Value::as_str)
                .expect("pressure report pointer")
                .to_owned()
        })
        .collect();
    assert_eq!(actual_sources, expected_sources);
    assert_eq!(
        channels
            .iter()
            .map(|channel| store.channel_counts(*channel).0)
            .sum::<usize>(),
        5_121
    );

    drop(store);
    let mut reopened = WakeIntentStore::open(dir.path()).expect("final pressure restart");
    for channel in channels {
        for source in all.get(&channel).expect("replay sources") {
            assert_eq!(
                reopened
                    .capture_report(pressure_scope(channel), source.clone())
                    .expect("pressure replay"),
                DiscoveryCapture::Duplicate
            );
        }
    }
    assert_eq!(
        control.published.lock().expect("published lock").len(),
        5_121
    );

    relay.shutdown().await;
    server.abort();
}
