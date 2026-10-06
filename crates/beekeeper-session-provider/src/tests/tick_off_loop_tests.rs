//! SV-76: the tick's relay-identity retry and authority-chain re-read run off
//! the run loop, bounded, one at a time ([`crate::off_loop`]).
//!
//! The same class as SV-72 ([`super::team_wake_fetch_tests`]): the tick used
//! to await these relay reads inline, and while it waited no transcript row
//! was published. These tests stand a relay up that accepts a connection and
//! never answers.

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
/// timeout of its own — so only the off-loop bound can end a read.
fn provider_reading_from(state_dir: &Path, base_url: String) -> Provider {
    let keys = Keys::generate();
    let mut provider = Provider::new(config_of(
        keys.clone(),
        state_dir,
        None,
        "missing-agent".into(),
    ))
    .expect("provider");
    provider.set_rest_client(RestClient {
        http: reqwest::Client::new(),
        base_url,
        keys,
        auth_tag_json: None,
    });
    provider
}

/// Queue one transcript row and prove it publishes within a second.
async fn a_transcript_row_publishes_at_once(provider: &mut Provider) {
    let channel = Uuid::new_v4();
    let transcript = nostr::EventBuilder::new(nostr::Kind::Custom(44225), "{}")
        .tags(vec![
            nostr::Tag::parse(["h", &channel.to_string()]).expect("tag")
        ])
        .sign_with_keys(&provider.config.keys)
        .expect("sign");
    let transcript_id = transcript.id;
    provider
        .outbox
        .enqueue(
            44225,
            &format!("transcript-{transcript_id}"),
            Priority::Live,
            transcript,
        )
        .expect("enqueue");
    let sink = CollectingSink::new();
    let published = tokio::time::timeout(Duration::from_secs(1), provider.flush_one(&sink))
        .await
        .expect("publication does not wait on the off-loop read")
        .expect("flush");
    assert_eq!(published, 1);
    assert!(sink
        .events
        .lock()
        .expect("lock")
        .iter()
        .any(|event| event.id == transcript_id));
}

async fn reached(accepted: &mut mpsc::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(5), accepted.recv())
        .await
        .expect("the read reached the relay")
        .expect("server alive");
}

async fn nothing_more_reaches(accepted: &mut mpsc::Receiver<()>) {
    assert!(
        tokio::time::timeout(Duration::from_millis(200), accepted.recv())
            .await
            .is_err(),
        "one read at a time"
    );
}

// ── The relay identity ──────────────────────────────────────────────────────

#[tokio::test]
async fn a_hung_relay_identity_read_does_not_hold_transcript_publication() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mut accepted, server) = spawn_hung_relay().await;
    let mut provider = provider_reading_from(&dir.path().join("state"), base_url);

    // The tick starts the read and returns; it does not wait for the relay.
    provider.start_relay_identity_witness();
    assert!(
        provider.relay_identity_fetch_busy(),
        "the read is in flight"
    );
    reached(&mut accepted).await;

    a_transcript_row_publishes_at_once(&mut provider).await;
    assert!(provider.relay_identity_fetch_busy(), "still unanswered");

    // A second tick does not stack a second read on the hung one.
    provider.start_relay_identity_witness();
    nothing_more_reaches(&mut accepted).await;
    server.abort();
}

#[tokio::test]
async fn a_relay_identity_read_that_never_answers_is_bounded_and_retried() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mut accepted, server) = spawn_hung_relay().await;
    let mut provider = provider_reading_from(&dir.path().join("state"), base_url);
    provider.relay_identity_fetch.timeout = Duration::from_millis(200);
    let mut answers = provider.take_relay_identity_answers();

    provider.start_relay_identity_witness();
    let answer = tokio::time::timeout(Duration::from_secs(5), off_loop::next(&mut answers))
        .await
        .expect("the bound ends the read");
    let error = answer.as_ref().expect_err("a hung read is not an answer");
    assert!(error.contains("did not finish within"), "{error}");
    assert!(!provider.finish_relay_identity_witness(answer));
    assert!(!provider.relay_identity_fetch_busy(), "the slot is free");
    assert!(provider.relay_self.is_none(), "no identity was invented");

    // The next tick tries again.
    reached(&mut accepted).await;
    provider.start_relay_identity_witness();
    reached(&mut accepted).await;
    server.abort();
}

#[tokio::test]
async fn a_late_identity_answer_never_replaces_a_witnessed_identity() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let witnessed = "aa".repeat(32);
    provider.set_relay_self(witnessed.clone());
    assert!(provider.finish_relay_identity_witness(Ok(Some("bb".repeat(32)))));
    assert_eq!(provider.relay_self.as_deref(), Some(witnessed.as_str()));
}

// ── The authority-chain re-read ─────────────────────────────────────────────

/// A provider with one open execution whose umbrella chain is still unread.
fn provider_with_an_unverified_umbrella(
    dir: &tempfile::TempDir,
    base_url: String,
) -> (Provider, Uuid, String) {
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let mut provider = provider_reading_from(&dir.path().join("state"), base_url);
    provider.set_relay_self(Keys::generate().public_key().to_hex());
    let channel_id = Uuid::new_v4();
    let genesis_ref = "ab".repeat(32);
    let record = governed_record(channel_id, &cwd, &genesis_ref);
    provider.state.insert_session(record).expect("insert");
    provider
        .claims_pending_reverification
        .insert(genesis_ref.clone());
    (provider, channel_id, genesis_ref)
}

#[tokio::test]
async fn a_hung_chain_re_read_does_not_hold_transcript_publication() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mut accepted, server) = spawn_hung_relay().await;
    let (mut provider, _channel, genesis_ref) =
        provider_with_an_unverified_umbrella(&dir, base_url);

    provider.start_claim_reverification();
    assert!(provider.claim_reverify_busy(), "the read is in flight");
    reached(&mut accepted).await;

    a_transcript_row_publishes_at_once(&mut provider).await;
    assert!(
        provider
            .claims_pending_reverification
            .contains(&genesis_ref),
        "an unanswered read releases nothing"
    );

    provider.start_claim_reverification();
    nothing_more_reaches(&mut accepted).await;
    server.abort();
}

#[tokio::test]
async fn a_chain_re_read_that_never_answers_is_bounded_and_keeps_the_fence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (base_url, _accepted, server) = spawn_hung_relay().await;
    let (mut provider, _channel, genesis_ref) =
        provider_with_an_unverified_umbrella(&dir, base_url);
    provider.claim_reverify_fetch.timeout = Duration::from_millis(200);
    let mut answers = provider.take_claim_reverify_answers();

    provider.start_claim_reverification();
    let answer = tokio::time::timeout(Duration::from_secs(5), off_loop::next(&mut answers))
        .await
        .expect("the bound ends the read");
    assert!(answer.is_err(), "a hung read is not an answer");
    provider.finish_claim_reverification(answer).await;
    assert!(!provider.claim_reverify_busy(), "the slot is free");
    assert!(
        provider
            .claims_pending_reverification
            .contains(&genesis_ref),
        "the fence stays up"
    );
    server.abort();
}

#[tokio::test]
async fn no_chain_re_read_starts_before_the_relay_identity_is_known() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mut accepted, server) = spawn_hung_relay().await;
    let (mut provider, _channel, _genesis_ref) =
        provider_with_an_unverified_umbrella(&dir, base_url);
    provider.relay_self = None;
    provider.start_claim_reverification();
    assert!(!provider.claim_reverify_busy());
    nothing_more_reaches(&mut accepted).await;
    server.abort();
}

#[tokio::test]
async fn a_whole_chain_read_releases_the_umbrella_it_was_taken_for() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, genesis_ref) =
        provider_with_an_unverified_umbrella(&dir, "http://127.0.0.1:9".into());
    let read = claim_reverify_fetch::UmbrellaRead::taken_now(
        &provider,
        &genesis_ref,
        vec![(channel_id, AcceptedChain::complete(Vec::new()))],
    );
    provider.finish_claim_reverification(Ok(vec![read])).await;
    assert!(
        !provider
            .claims_pending_reverification
            .contains(&genesis_ref),
        "a whole chain with nothing in it, read for these records, is verified"
    );
}

#[tokio::test]
async fn a_read_whose_records_moved_meanwhile_is_dropped_and_the_fence_stays() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, genesis_ref) =
        provider_with_an_unverified_umbrella(&dir, "http://127.0.0.1:9".into());
    let read = claim_reverify_fetch::UmbrellaRead::taken_now(
        &provider,
        &genesis_ref,
        vec![(channel_id, AcceptedChain::complete(Vec::new()))],
    );
    // A live receipt folds while the read is out.
    let session_id = provider
        .state
        .sessions()
        .next()
        .expect("record")
        .session_id
        .clone();
    provider
        .state
        .update_session(&session_id, |record| record.authority_seq = 1)
        .expect("update");
    provider.finish_claim_reverification(Ok(vec![read])).await;
    assert!(
        provider
            .claims_pending_reverification
            .contains(&genesis_ref),
        "a read of older state never lifts the fence"
    );
}

#[tokio::test]
async fn an_incomplete_read_keeps_the_fence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, genesis_ref) =
        provider_with_an_unverified_umbrella(&dir, "http://127.0.0.1:9".into());
    let read = claim_reverify_fetch::UmbrellaRead::taken_now(
        &provider,
        &genesis_ref,
        vec![(
            channel_id,
            AcceptedChain::incomplete("the receipt query failed".into()),
        )],
    );
    provider.finish_claim_reverification(Ok(vec![read])).await;
    assert!(provider
        .claims_pending_reverification
        .contains(&genesis_ref));
}
