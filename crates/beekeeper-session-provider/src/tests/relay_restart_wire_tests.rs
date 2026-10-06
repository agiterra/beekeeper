use super::*;

/// The production run loop, durable store and publisher talk over a real
/// loopback WebSocket. The relay double records EVENT frames and acknowledges
/// them; it does not implement an alternate scheduler or call a model service.
#[tokio::test]
async fn restart_wire_backlog_stays_below_quota_and_create_receipt_overtakes_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let checkout = dir.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("checkout");
    let keys = Keys::generate();
    let channels: Vec<_> = (0..39).map(|_| Uuid::new_v4()).collect();
    let projects = write_projects(dir.path(), channels[0], &checkout);
    let mut config = config_of(
        keys.clone(),
        &state_dir,
        Some(&projects),
        fake_agent(dir.path(), "wire-agent", GOOD_AGENT),
    );
    let mut seeded = Provider::new(config.clone()).expect("initial provider");
    for index in 0..57 {
        let mut record = governed_record(channels[index % channels.len()], &checkout, "");
        record.genesis_ref = None;
        record.session_ref = None;
        record.command_id = format!("old-create-{index}");
        seeded
            .state
            .insert_session(record)
            .expect("persist old session");
    }
    // A command arriving while the provider was stopped is delivered through
    // its actual startup subscription and replay window, not direct dispatch.
    let create = create_event(&seeded, channels[0], "wire-fresh-create");
    let mut events = vec![create];
    for channel in &channels {
        events.push(
            nostr::EventBuilder::new(nostr::Kind::Custom(39002), "")
                .tags([
                    nostr::Tag::parse(["d", &channel.to_string()]).expect("d tag"),
                    nostr::Tag::parse(["p", &keys.public_key().to_hex()]).expect("p tag"),
                ])
                .sign_with_keys(&keys)
                .expect("membership"),
        );
    }
    drop(seeded);
    let (relay, recording, server) = spawn_recording_test_relay(&keys, events).await;
    drop(relay);
    config.relay_url = recording.url.clone();
    let started = Instant::now();
    let observation = async {
        let receipt_at = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let received =
                    recording
                        .published
                        .lock()
                        .expect("wire events")
                        .iter()
                        .any(|event| {
                            u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_LIFECYCLE_RECEIPT
                                && serde_json::from_str::<serde_json::Value>(&event.content)
                                    .expect("receipt json")["commandId"]
                                    == "wire-fresh-create"
                        });
                if received {
                    let events = recording.published.lock().expect("wire events");
                    let receipt = events
                        .iter()
                        .find(|event| {
                            u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_LIFECYCLE_RECEIPT
                                && event.content.contains("wire-fresh-create")
                        })
                        .expect("receipt");
                    let body: serde_json::Value =
                        serde_json::from_str(&receipt.content).expect("receipt JSON");
                    assert_eq!(
                        body["status"], "created",
                        "receipt must confirm an actual new session: {body}"
                    );
                    let snapshot: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(state_dir.join("state.json")).expect("persisted state")
                    ).expect("state JSON");
                    let session_id = body["session"]["sessionId"].as_str().expect("created session id");
                    let created_at_ms = snapshot["sessions"][session_id]["createdAtMs"].as_i64().expect("created time");
                    let delivery_ms = now_ms().saturating_sub(created_at_ms);
                    assert!(delivery_ms < 1_000, "receipt after runtime opened: {delivery_ms}ms");
                    eprintln!("create receipt reached WebSocket relay {delivery_ms}ms after runtime opened and session record created");
                    break started.elapsed();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("new create receipt must not wait behind 96 background facts");
        // Allow process scheduling under the full parallel suite, startup
        // replay and shell-double initialization. Receipt delivery itself is
        // separately bounded to <1s above; background rows are paced at 3s.
        assert!(receipt_at < Duration::from_secs(30), "{receipt_at:?}");
        tokio::time::sleep_until((started + Duration::from_secs(60)).into()).await;
        let received = recording.published.lock().expect("wire events").clone();
        let background = received
            .iter()
            .filter(|event| {
                u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_PROVIDER_CATALOG
                    || (u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_METADATA
                        && event.content.contains("disconnected"))
            })
            .count();
        assert!(
            (1..=21).contains(&background),
            "paced background frames: {background}"
        );
        assert!(
            received.len() < 30,
            "actual EVENT frames in 60s: {}",
            received.len()
        );
        assert!(
            background < 96,
            "the fresh receipt overtook a real remaining backlog"
        );
        eprintln!("restart wire proof: {} EVENT frames / 60s; {background} background; create receipt after {receipt_at:?} (includes replay window + fake runtime startup)", received.len());
    };
    tokio::select! {
        result = run_with(config) => panic!("provider stopped during proof: {result:?}"),
        () = observation => {}
    }
    server.abort();
}

#[tokio::test]
async fn unchanged_recovery_metadata_after_second_restart_sends_no_wire_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let keys = Keys::generate();
    let channel = Uuid::new_v4();
    let config = config_of(
        keys.clone(),
        &state_dir,
        None,
        fake_agent(dir.path(), "wire-agent", GOOD_AGENT),
    );
    let (relay, recording, server) = spawn_recording_test_relay(&keys, vec![]).await;
    let publisher = relay.event_publisher();
    let mut seeded = Provider::new(config.clone()).expect("provider");
    let mut record = governed_record(channel, dir.path(), "");
    record.genesis_ref = None;
    record.session_ref = None;
    seeded
        .state
        .insert_session(record)
        .expect("persist session");
    drop(seeded);
    let mut first = Provider::new(config.clone()).expect("first restart");
    first.membership_known = true;
    first.subscribed.insert(channel);
    first.recover().await.expect("recover");
    first.flush(&publisher).await.expect("real websocket ACK");
    assert_eq!(recording.published.lock().expect("wire events").len(), 1);
    drop(first);
    let mut second = Provider::new(config).expect("second restart");
    second.membership_known = true;
    second.subscribed.insert(channel);
    second.recover().await.expect("recover unchanged");
    assert_eq!(second.flush(&publisher).await.expect("flush"), 0);
    assert_eq!(
        recording.published.lock().expect("wire events").len(),
        1,
        "unchanged accepted metadata must not create another EVENT frame"
    );
    server.abort();
}
