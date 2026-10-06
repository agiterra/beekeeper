//! Retirement: an accepted whole-session deletion, consumed before anything
//! is republished.
//!
//! The bug these pin is on the record. A founder deleted an umbrella, the
//! relay applied it, and a provider that had been offline came back and
//! published fresh `disconnected` metadata for every local record — new event
//! ids the deletion had never named, so a session whose whole history was gone
//! read as an execution waiting to be resumed
//! (`review-2026-09-08-release-candidate/continuity-diagnosis.md`).
//!
//! Half of these tests are therefore about **not** retiring: a query that
//! failed, a request the relay never applied, a signer who is not the founder.
//! "I could not check" and "it was deleted" are different answers, and a
//! provider that conflates them stops sessions nobody deleted — which is the
//! same class of lie in the opposite direction.

use super::*;

use crate::payload::SESSION_RETIRED;

/// The `type` the relay stamps on a whole-session deletion receipt.
///
/// Written out rather than imported so this file states the exact bytes the
/// provider must accept; if the relay's word ever changes, the mismatch shows
/// up here as a failing test rather than as silence.
const DELETION_RECEIPT_TYPE: &str = "coding_session_deletion_accepted";

/// A kind 5 naming `genesis_ref`, signed by `signer`, scoped to the channel.
fn deletion_event(channel_id: Uuid, genesis_ref: &str, signer: &Keys) -> Event {
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_DELETION as u16),
        "removing this session",
    )
    .tags(vec![
        nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag"),
        nostr::Tag::parse(["e", genesis_ref]).expect("tag"),
    ])
    .sign_with_keys(signer)
    .expect("sign deletion")
}

/// The relay's signed 40099 statement that it applied a deletion.
fn deletion_receipt(
    relay_keys: &Keys,
    channel_id: Uuid,
    genesis_ref: &str,
    session_ref: &str,
    deletion_event_id: &str,
) -> Event {
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        serde_json::json!({
            "type": DELETION_RECEIPT_TYPE,
            "genesisRef": genesis_ref,
            "sessionRef": session_ref,
            "deletionEventId": deletion_event_id,
            "channelId": channel_id.to_string(),
        })
        .to_string(),
    )
    .tags(vec![
        nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
    ])
    .sign_with_keys(relay_keys)
    .expect("sign deletion receipt")
}

/// The `sessionRef` [`governed_record`] mints.
const RETIRED_SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

/// A provider holding one governed record on `channel_id`, wired to a relay
/// serving exactly `events`, ready for `recover`.
///
/// The channel is the caller's because every deletion artifact is scoped to
/// it: building the events first and the provider second is what lets each
/// test hand the relay precisely the history it means to test against, with
/// nothing else in it.
async fn recovering_provider(
    dir: &Path,
    relay_keys: &Keys,
    channel_id: Uuid,
    genesis_ref: &str,
    events: Vec<Event>,
) -> (
    Provider,
    HarnessRelay,
    tokio::task::JoinHandle<()>,
    CodingSessionTarget,
) {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let provider_keys = Keys::generate();
    let state_dir = dir.join("state");
    let agent = fake_agent(state_dir_parent(&state_dir), "good-agent", GOOD_AGENT);
    let mut provider =
        Provider::new(config_of(provider_keys.clone(), &state_dir, None, agent)).expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    let record = governed_record(channel_id, &cwd, genesis_ref);
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");
    let (relay, _control, server) = spawn_recording_test_relay(&provider_keys, events).await;
    provider.set_rest_client(relay.rest_client());
    (provider, relay, server, target)
}

/// The seat-request rows the provider last wrote, straight off disk.
fn seat_request_rows(state_dir: &Path) -> Vec<serde_json::Value> {
    let path = crate::seat_requests::seat_requests_path(state_dir);
    if !path.exists() {
        return Vec::new();
    }
    let body = std::fs::read_to_string(&path).expect("read seat requests");
    serde_json::from_str::<serde_json::Value>(&body)
        .expect("seat requests parse")
        .get("requests")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Every 44223 the provider queued, drained through a sink.
async fn published_metadata(provider: &mut Provider) -> Vec<serde_json::Value> {
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    sink.contents_of(KIND_CODING_SESSION_METADATA)
}

// -------------------------------------------------------------------------
// The two proofs that do retire
// -------------------------------------------------------------------------

/// (a) A relay-signed receipt. It needs no second read: the relay is saying it
/// applied the deletion, verified against the identity this provider
/// witnessed. Recovery retires the record and publishes **nothing**.
#[tokio::test]
async fn a_relay_signed_deletion_receipt_retires_the_umbrella_before_recovery_publishes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let channel_id = Uuid::new_v4();
    let genesis_ref = "ab".repeat(32);
    let deletion_id = "ee".repeat(32);

    let receipt = deletion_receipt(
        &relay_keys,
        channel_id,
        &genesis_ref,
        RETIRED_SESSION_REF,
        &deletion_id,
    );
    let (mut provider, relay, server, target) = recovering_provider(
        dir.path(),
        &relay_keys,
        channel_id,
        &genesis_ref,
        vec![receipt.clone()],
    )
    .await;

    provider.recover().await.expect("recover");

    let record = provider
        .state()
        .session(&target.session_id)
        .expect("the record survives, retired");
    let retirement = record.retired.as_ref().expect("the record is retired");
    assert_eq!(retirement.deletion_event_id, deletion_id);
    assert_eq!(
        retirement.receipt_event_id.as_deref(),
        Some(receipt.id.to_hex().as_str()),
        "a receipt-proved retirement names the receipt it read"
    );
    assert!(
        published_metadata(&mut provider).await.is_empty(),
        "a retired session publishes no metadata, not even once"
    );
    assert!(
        seat_request_rows(&provider.config.state_dir).is_empty(),
        "a retired session asks for no seat custody"
    );

    // And every later command answers by name.
    let decision = lifecycle_decision_by(
        &provider,
        channel_id,
        "resume-after-deletion",
        "session.resume",
        &target,
        test_operator_keys(),
    );
    match decision {
        LifecycleDecision::Fail { code, message, .. } => {
            assert_eq!(code, SESSION_RETIRED);
            assert!(
                message.contains(&deletion_id),
                "the refusal names the deletion it read: {message}"
            );
        }
        other => panic!("expected SESSION_RETIRED, got {other:?}"),
    }

    relay.shutdown().await;
    server.abort();
}

/// (b) A founder-signed kind 5 whose genesis no longer reads back. The
/// signature says who asked; the absence says the relay agreed. Neither alone
/// is enough, and the two tests after this one prove it.
#[tokio::test]
async fn a_founder_signed_deletion_with_the_genesis_absent_retires_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let channel_id = Uuid::new_v4();
    let genesis_ref = "ab".repeat(32);

    // `governed_record`'s founder is the test operator, so this is their own
    // request. The relay holds it and, deliberately, not the genesis.
    let deletion = deletion_event(channel_id, &genesis_ref, test_operator_keys());
    let (mut provider, relay, server, target) = recovering_provider(
        dir.path(),
        &relay_keys,
        channel_id,
        &genesis_ref,
        vec![deletion.clone()],
    )
    .await;

    provider.recover().await.expect("recover");

    let record = provider
        .state()
        .session(&target.session_id)
        .expect("the record survives, retired");
    let retirement = record.retired.as_ref().expect("the record is retired");
    assert_eq!(retirement.deletion_event_id, deletion.id.to_hex());
    assert_eq!(
        retirement.receipt_event_id, None,
        "this half of the rule is proved by absence, not by a receipt"
    );
    assert!(published_metadata(&mut provider).await.is_empty());

    relay.shutdown().await;
    server.abort();
}

// -------------------------------------------------------------------------
// The three things that are never authority
// -------------------------------------------------------------------------

/// Anyone who can sign can publish a kind 5 naming any event id. A deletion
/// request from somebody who is not this record's founder retires nothing —
/// even though the genesis is absent from the relay here, so the *absence*
/// half of the rule is satisfied and only the signature is not.
#[tokio::test]
async fn a_deletion_signed_by_anyone_but_the_founder_retires_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let channel_id = Uuid::new_v4();
    let genesis_ref = "ab".repeat(32);

    let impostor = deletion_event(channel_id, &genesis_ref, &Keys::generate());
    let (mut provider, relay, server, target) = recovering_provider(
        dir.path(),
        &relay_keys,
        channel_id,
        &genesis_ref,
        vec![impostor],
    )
    .await;

    provider.recover().await.expect("recover");

    assert!(
        !provider
            .state()
            .session(&target.session_id)
            .expect("record")
            .is_retired(),
        "a kind 5 from a non-founder is a signed request, not an accepted deletion"
    );
    assert!(
        !published_metadata(&mut provider).await.is_empty(),
        "and the session is still ordinary, so recovery published its status"
    );

    relay.shutdown().await;
    server.abort();
}

/// A founder's own request whose genesis still reads back. The request exists;
/// the relay did not act on it. Nothing is retired — the signed intent is not
/// the effect.
#[tokio::test]
async fn a_founder_deletion_whose_genesis_still_reads_back_retires_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let channel_id = Uuid::new_v4();

    let genesis = genesis_event(channel_id, RETIRED_SESSION_REF);
    let genesis_ref = genesis.id.to_hex();
    let deletion = deletion_event(channel_id, &genesis_ref, test_operator_keys());
    let (mut provider, relay, server, target) = recovering_provider(
        dir.path(),
        &relay_keys,
        channel_id,
        &genesis_ref,
        vec![deletion, genesis],
    )
    .await;

    provider.recover().await.expect("recover");

    assert!(
        !provider
            .state()
            .session(&target.session_id)
            .expect("record")
            .is_retired(),
        "a genesis that still reads back was not deleted, whatever was asked"
    );

    relay.shutdown().await;
    server.abort();
}

/// The read itself failed. "I could not check" is not "it was deleted": the
/// record is left exactly as it was, and recovery proceeds normally.
#[tokio::test]
async fn a_failed_read_is_never_a_deletion() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let channel_id = Uuid::new_v4();
    let genesis_ref = "ab".repeat(32);

    let deletion = deletion_event(channel_id, &genesis_ref, test_operator_keys());
    let (mut provider, relay, server, target) = recovering_provider(
        dir.path(),
        &relay_keys,
        channel_id,
        &genesis_ref,
        vec![deletion],
    )
    .await;
    // The relay goes away before recovery reads it: every query errors, and
    // the founder's request that *is* on it is never seen.
    relay.shutdown().await;
    server.abort();

    provider
        .recover()
        .await
        .expect("recover survives an unreachable relay");

    assert!(
        !provider
            .state()
            .session(&target.session_id)
            .expect("record")
            .is_retired(),
        "an unreachable relay must never retire a session"
    );
    // And, per the reverification gate, it publishes nothing either: a chain
    // this process could not read cannot say whether the session was handed
    // over, and `disconnected` with no handover block is the comfortable
    // guess rather than the honest silence.
    assert!(
        published_metadata(&mut provider).await.is_empty(),
        "an unreadable chain holds the publish rather than guessing at it"
    );
}

// -------------------------------------------------------------------------
// Live, replayed, and after a restart
// -------------------------------------------------------------------------

/// A deletion observed while the provider is running retires the record and
/// releases the process it was holding — the same cleanup a closure does, for
/// the same reason: the work is over and the slot is not. Replaying the same
/// deletion changes nothing.
#[tokio::test]
async fn a_live_deletion_retires_the_record_and_releases_its_process() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&dir.path().join("state"), Some(&projects));
    let relay_keys = Keys::generate();
    provider.set_relay_self(relay_keys.public_key().to_hex());

    let genesis = genesis_event(channel_id, RETIRED_SESSION_REF);
    let genesis_ref = genesis.id.to_hex();
    // The genesis is served for the create's founder resolution and removed
    // afterwards, which is exactly the sequence a real deletion produces.
    let (mut relay, control, server) =
        spawn_recording_test_relay(&provider.config.keys, vec![genesis]).await;
    provider.set_rest_client(relay.rest_client());

    let create = create_event_with_genesis_ref(
        &provider,
        channel_id,
        "create-governed",
        RETIRED_SESSION_REF,
        &genesis_ref,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");
    let record = provider.state().sessions().next().expect("session").clone();
    let target = record.target(&provider.config.instance_id);
    assert_eq!(
        provider.sessions.live_count(),
        1,
        "the create started a process"
    );

    // The founder deletes the session, and the relay applies it.
    control.events.lock().expect("events").clear();
    let deletion = deletion_event(channel_id, &genesis_ref, test_operator_keys());
    provider
        .handle_relay_event(&mut relay, channel_id, &deletion)
        .await
        .expect("observe the deletion");

    let record = provider
        .state()
        .session(&target.session_id)
        .expect("the record survives, retired");
    assert!(record.is_retired(), "the live deletion retired the record");
    assert!(record.open_turn.is_none(), "its turn is not left in flight");
    assert_eq!(
        provider.sessions.live_count(),
        0,
        "and the process it was holding is released"
    );

    // Replay: the same deletion arriving twice changes nothing.
    let before = record.retired.clone();
    provider
        .handle_relay_event(&mut relay, channel_id, &deletion)
        .await
        .expect("replayed deletion");
    assert_eq!(
        provider
            .state()
            .session(&target.session_id)
            .expect("record")
            .retired,
        before,
        "a replayed deletion is idempotent"
    );

    relay.shutdown().await;
    server.abort();
}

/// A provider that restarts over a state file already carrying a retirement
/// publishes nothing at all for it — no metadata, no seat request — and needs
/// no relay to decide that.
#[tokio::test]
async fn a_restart_after_retirement_publishes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let state_dir = dir.path().join("state");
    let channel_id = Uuid::new_v4();
    let genesis_ref = "ab".repeat(32);

    {
        let mut provider = provider(&state_dir, None);
        let mut record = governed_record(channel_id, &cwd, &genesis_ref);
        record.actor = Some("cd".repeat(32));
        record.role = Some("builder".into());
        record.retired = Some(crate::state::Retirement {
            deletion_event_id: "ee".repeat(32),
            receipt_event_id: None,
            at: 1,
        });
        provider.state.insert_session(record).expect("insert");
    }

    let agent = fake_agent(state_dir_parent(&state_dir), "good-agent", GOOD_AGENT);
    let mut restarted =
        Provider::new(config_of(Keys::generate(), &state_dir, None, agent)).expect("provider");
    restarted.recover().await.expect("recover");

    assert!(
        published_metadata(&mut restarted).await.is_empty(),
        "a retired session is never re-advertised"
    );
    let rows = seat_request_rows(&state_dir);
    assert!(rows.is_empty(), "and its seat is never re-staged: {rows:?}");
}
