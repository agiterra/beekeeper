//! A team-wake relay read runs off the run loop and is bounded
//! ([`crate::team_wake_fetch`]).
//!
//! Live evidence, 2026-10-05: a session's transcript stopped for minutes while
//! its agent kept working, because the run loop was awaiting team-wake reads
//! against a relay that answered slowly or not at all. These tests stand a
//! relay up that accepts a connection and never answers.

use super::*;

use tokio::net::TcpListener;

/// A "relay" that accepts every connection, reports that it did, and never
/// writes a byte back. The sockets are held for the server's lifetime.
async fn spawn_hung_relay() -> (String, mpsc::Receiver<()>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (accepted_tx, accepted_rx) = mpsc::channel(16);
    let server = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
            let _ = accepted_tx.try_send(());
        }
    });
    (format!("http://{addr}"), accepted_rx, server)
}

/// A provider whose relay reads go to `base_url`, over a client with no
/// timeout of its own — so only the team-wake bound can end a read.
fn provider_reading_from(state_dir: &Path, base_url: String, channel: Uuid) -> Provider {
    let keys = Keys::generate();
    let mut provider = Provider::new(config_of(
        keys.clone(),
        state_dir,
        None,
        "missing-agent".into(),
    ))
    .expect("provider");
    provider.set_relay_self(Keys::generate().public_key().to_hex());
    provider.set_rest_client(RestClient {
        http: reqwest::Client::new(),
        base_url,
        keys,
        auth_tag_json: None,
    });
    // Subscribed and never scanned: the next tick reads its partition.
    provider.subscribed.insert(channel);
    provider
}

#[tokio::test]
async fn a_hung_team_wake_read_does_not_hold_transcript_publication() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mut accepted, server) = spawn_hung_relay().await;
    let channel = Uuid::new_v4();
    let mut provider = provider_reading_from(&dir.path().join("state"), base_url, channel);

    // The tick starts the read and returns; it does not wait for the relay.
    provider.start_team_wake_tick().expect("tick");
    assert!(provider.team_wake_fetch_busy(), "the read is in flight");
    tokio::time::timeout(Duration::from_secs(5), accepted.recv())
        .await
        .expect("the read reached the relay")
        .expect("server alive");

    // While that read hangs, a transcript row publishes at once.
    let transcript = nostr::EventBuilder::new(nostr::Kind::Custom(44225), "{}")
        .tags(vec![
            nostr::Tag::parse(["h", &channel.to_string()]).expect("tag")
        ])
        .sign_with_keys(&provider.config.keys)
        .expect("sign");
    let transcript_id = transcript.id;
    provider
        .outbox
        .enqueue(44225, "transcript-1", Priority::Live, transcript)
        .expect("enqueue");
    let sink = CollectingSink::new();
    let published = tokio::time::timeout(Duration::from_secs(1), provider.flush_one(&sink))
        .await
        .expect("publication does not wait on the team-wake read")
        .expect("flush");
    assert_eq!(published, 1);
    assert!(sink
        .events
        .lock()
        .expect("lock")
        .iter()
        .any(|event| event.id == transcript_id));
    assert!(
        provider.team_wake_fetch_busy(),
        "the relay never answered, so the read is still out"
    );

    // A second tick does not stack a second read on the hung one.
    provider.start_team_wake_tick().expect("tick");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), accepted.recv())
            .await
            .is_err(),
        "one team-wake read at a time"
    );
    server.abort();
}

#[tokio::test]
async fn a_team_wake_read_that_never_answers_is_bounded_and_retried() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (base_url, _accepted, server) = spawn_hung_relay().await;
    let channel = Uuid::new_v4();
    let mut provider = provider_reading_from(&dir.path().join("state"), base_url, channel);
    provider.team_wake_fetch.timeout = Duration::from_millis(200);
    let mut fetches = provider.take_team_wake_fetches();
    let (relay, _control, relay_server) =
        spawn_recording_test_relay(&provider.config.keys.clone(), Vec::new()).await;
    let publisher = relay.event_publisher();

    provider.start_team_wake_tick().expect("tick");
    let fetched = tokio::time::timeout(
        Duration::from_secs(5),
        team_wake_fetch::next_fetched(&mut fetches),
    )
    .await
    .expect("the bound ends the read");
    match &fetched {
        team_wake_fetch::TeamWakeFetched::Discovery { result, .. } => {
            let error = result.as_ref().expect_err("a hung read is not an answer");
            assert!(
                error.to_string().contains("did not finish within"),
                "{error}"
            );
        }
        team_wake_fetch::TeamWakeFetched::Snapshot { .. } => panic!("expected discovery"),
    }
    provider
        .finish_team_wake_fetch(fetched, &publisher)
        .await
        .expect("finish");

    // A transient failure: not scanned, backed off, slot free for the retry.
    assert!(!provider.team_wake_fetch_busy());
    assert!(!provider.team_wake_scanned_channels.contains(&channel));
    assert_eq!(
        provider
            .team_wake_discovery_backoff
            .get(&channel)
            .map(|backoff| backoff.reason.as_str()),
        Some("complete_partition_unavailable")
    );
    server.abort();
    relay_server.abort();
}

/// A tick can start the next read after the previous one finished but before
/// its answer was applied. Applying that older answer must not drop the newer
/// read's handle: a dropped handle reads as a free slot, and the next tick
/// would stack a second read beside the first.
#[tokio::test]
async fn applying_an_older_answer_keeps_the_running_read_in_the_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mut accepted, server) = spawn_hung_relay().await;
    let channel = Uuid::new_v4();
    let mut provider = provider_reading_from(&dir.path().join("state"), base_url, channel);
    let (relay, _control, relay_server) =
        spawn_recording_test_relay(&provider.config.keys.clone(), Vec::new()).await;
    let publisher = relay.event_publisher();

    provider.start_team_wake_tick().expect("tick");
    tokio::time::timeout(Duration::from_secs(5), accepted.recv())
        .await
        .expect("the read reached the relay")
        .expect("server alive");
    assert!(provider.team_wake_fetch_busy());

    // An older read's answer, for a channel no longer subscribed.
    let older = team_wake_fetch::TeamWakeFetched::Discovery {
        channel_id: Uuid::new_v4(),
        reprobing: false,
        result: Ok(Vec::new()),
    };
    provider
        .finish_team_wake_fetch(older, &publisher)
        .await
        .expect("finish");
    assert!(
        provider.team_wake_fetch_busy(),
        "the hung read still holds the slot"
    );
    provider.start_team_wake_tick().expect("tick");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), accepted.recv())
            .await
            .is_err(),
        "one team-wake read at a time"
    );
    server.abort();
    relay_server.abort();
}
