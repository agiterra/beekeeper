//! Wire tests for `bee sessions stop`: against a recording relay, the exact
//! 44221 `session.stop` it publishes, and that every refusal publishes
//! nothing at all.

use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::response::Json;
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};

use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_payload::{
    Capabilities, LifecycleReceipt, ReceiptError, ReceiptStatus, SessionMetadata, SessionStatus,
    LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA,
};
use beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND;
use beekeeper_sdk::builders::build_coding_session_lifecycle_receipt;
use beekeeper_sdk::kind::KIND_CODING_SESSION_METADATA;

use super::stop::{classify_stop_receipts, cmd_stop, StopAnswer};
use crate::client::BeekeeperClient;
use crate::error::CliError;

const CHANNEL: &str = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const SESSION: &str = "5f0c2a8e-1b7d-4c3e-9f60-2d4a8b1c7e90";

fn target(session_id: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "instance-1".into(),
        session_id: session_id.into(),
        generation,
    }
}

/// A 44223 row as `sessions list` reads it. Unsigned: the channel read does
/// not verify signatures (see the module docs of `sessions`).
fn metadata_event(
    signer: &str,
    id: &str,
    at: i64,
    target: &CodingSessionTarget,
    status: SessionStatus,
) -> Value {
    let payload = SessionMetadata {
        schema: METADATA_SCHEMA.to_owned(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: None,
        role: None,
        provider: Some("claude-primary".try_into().expect("alias")),
        runtime: Some("claude".try_into().expect("runtime")),
        model: Some("claude-opus".into()),
        status,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: None,
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        handover: None,
        compose_ref: None,
    };
    json!({
        "id": id,
        "pubkey": signer,
        "kind": KIND_CODING_SESSION_METADATA,
        "created_at": at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["csm-v", "csm1-1"],
                 ["cs-target", coding_session_target_key(target)]],
        "content": serde_json::to_string(&payload).expect("metadata"),
    })
}

/// A provider-signed 44224 answering `command_id`, built by the SDK builder
/// the provider uses.
fn signed_receipt(
    keys: &nostr::Keys,
    command_id: &str,
    status: ReceiptStatus,
    session: Option<CodingSessionTarget>,
    error: Option<ReceiptError>,
) -> Value {
    let receipt = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        status,
        session,
        error,
        turn_id: None,
        rewind: None,
    };
    let event = build_coding_session_lifecycle_receipt(
        uuid::Uuid::parse_str(CHANNEL).expect("channel"),
        command_id,
        &serde_json::to_string(&receipt).expect("receipt"),
    )
    .expect("receipt builder")
    .sign_with_keys(keys)
    .expect("sign receipt");
    serde_json::to_value(event).expect("event json")
}

/// What the provider answers to a stop's receipt query.
#[derive(Clone, Copy)]
enum Answer {
    Stopped,
    Refused,
    Silent,
}

#[derive(Default)]
struct Records {
    published: Vec<nostr::Event>,
    receipt_queries: usize,
}

#[derive(Clone)]
struct Relay {
    provider: nostr::Keys,
    channel: Arc<Vec<Value>>,
    answer: Answer,
    records: Arc<Mutex<Records>>,
}

/// A relay that records every publish, serves `channel` to channel reads, and
/// answers a stop's exact receipt query with `answer`.
async fn spawn_relay(
    provider: nostr::Keys,
    channel: Vec<Value>,
    answer: Answer,
) -> (String, Relay, tokio::task::JoinHandle<()>) {
    let relay = Relay {
        provider,
        channel: Arc::new(channel),
        answer,
        records: Arc::new(Mutex::new(Records::default())),
    };
    let app = Router::new()
        .route(
            "/events",
            post(|State(relay): State<Relay>, body: Bytes| async move {
                let event: nostr::Event = serde_json::from_slice(&body).expect("event JSON");
                let event_id = event.id.to_hex();
                relay.records.lock().expect("records").published.push(event);
                Json(json!({ "event_id": event_id, "accepted": true, "message": "" }))
            }),
        )
        .route(
            "/query",
            post(|State(relay): State<Relay>, body: Bytes| async move {
                let filters: Vec<Value> = serde_json::from_slice(&body).expect("filter JSON");
                let filter = filters.first().expect("one filter").clone();
                assert_eq!(filter["#h"], json!([CHANNEL]), "every read is h-scoped");
                if let Some(command_id) = filter["#csl-command"][0].as_str() {
                    relay.records.lock().expect("records").receipt_queries += 1;
                    let published = relay
                        .records
                        .lock()
                        .expect("records")
                        .published
                        .last()
                        .cloned()
                        .expect("a receipt query follows its publish");
                    let decoded =
                        decode_coding_session_lifecycle_command(&published.content).expect("44221");
                    let CodingSessionLifecycleAction::SessionStop { session, .. } = decoded.action
                    else {
                        panic!("published something other than a stop");
                    };
                    let events = match relay.answer {
                        Answer::Stopped => vec![signed_receipt(
                            &relay.provider,
                            command_id,
                            ReceiptStatus::Stopped,
                            Some(session),
                            None,
                        )],
                        Answer::Refused => vec![signed_receipt(
                            &relay.provider,
                            command_id,
                            ReceiptStatus::Failed,
                            None,
                            Some(ReceiptError {
                                code: "UNAUTHORIZED_OPERATOR".into(),
                                message: "only the session founder may stop this execution".into(),
                            }),
                        )],
                        Answer::Silent => Vec::new(),
                    };
                    return Json(json!(events));
                }
                let kinds: Vec<u64> = filter["kinds"]
                    .as_array()
                    .expect("explicit kinds")
                    .iter()
                    .filter_map(Value::as_u64)
                    .collect();
                let events: Vec<Value> = relay
                    .channel
                    .iter()
                    .filter(|event| {
                        event["kind"]
                            .as_u64()
                            .is_some_and(|kind| kinds.contains(&kind))
                    })
                    .cloned()
                    .collect();
                Json(json!(events))
            }),
        )
        .with_state(relay.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("recording relay");
    });
    (url, relay, server)
}

fn client(url: String) -> BeekeeperClient {
    BeekeeperClient::new(url, nostr::Keys::generate(), None, None).expect("client")
}

/// Two generations of one live execution: the stop must name the newest.
fn live_channel(provider: &str) -> Vec<Value> {
    vec![
        metadata_event(
            provider,
            &"01".repeat(32),
            1_000,
            &target(SESSION, 1),
            SessionStatus::Disconnected,
        ),
        metadata_event(
            provider,
            &"02".repeat(32),
            2_000,
            &target(SESSION, 2),
            SessionStatus::Idle,
        ),
    ]
}

#[tokio::test]
async fn stop_publishes_the_exact_current_generation_and_reports_the_signed_receipt() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let (url, relay, server) =
        spawn_relay(provider, live_channel(&authority), Answer::Stopped).await;

    cmd_stop(&client(url), CHANNEL, SESSION, &authority, true, Some(5))
        .await
        .expect("stop confirmed");

    let records = relay.records.lock().expect("records");
    assert_eq!(records.published.len(), 1, "exactly one publish");
    assert_eq!(
        records.receipt_queries, 1,
        "answered on the first receipt query"
    );
    let event = &records.published[0];
    assert_eq!(
        u32::from(event.kind.as_u16()),
        KIND_CODING_SESSION_LIFECYCLE_COMMAND
    );
    let payload = decode_coding_session_lifecycle_command(&event.content).expect("strict decode");
    assert_eq!(payload.schema, CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA);
    assert_eq!(payload.schema, "buzz-coding-session-lifecycle-command/v1");
    assert!(
        uuid::Uuid::parse_str(&payload.command_id).is_ok(),
        "fresh uuid commandId"
    );
    assert_eq!(
        payload.action,
        CodingSessionLifecycleAction::SessionStop {
            session: target(SESSION, 2),
            provider_authority_pubkey: authority.clone(),
        },
        "the newest generation, addressed to the named provider"
    );
    let tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    assert_eq!(
        tags.len(),
        3,
        "the relay's exactly-three-tags rule: {tags:?}"
    );
    assert_eq!(tags[0], vec!["h".to_owned(), CHANNEL.to_owned()]);
    assert_eq!(tags[1][0], "csl-v");
    assert_eq!(
        tags[2],
        vec!["csl-command".to_owned(), payload.command_id.clone()]
    );
    drop(records);
    server.abort();
}

#[tokio::test]
async fn a_provider_refusal_is_reported_as_refused_with_exit_1() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let (url, relay, server) =
        spawn_relay(provider, live_channel(&authority), Answer::Refused).await;

    let error = cmd_stop(&client(url), CHANNEL, SESSION, &authority, true, Some(5))
        .await
        .expect_err("refused");
    assert!(
        matches!(&error, CliError::Refused(message) if message.contains("UNAUTHORIZED_OPERATOR")),
        "{error:?}"
    );
    assert_eq!(crate::error::exit_code(&error), 1);
    assert_eq!(relay.records.lock().expect("records").published.len(), 1);
    server.abort();
}

#[tokio::test]
async fn no_receipt_in_time_is_unconfirmed_never_failure_and_never_a_second_stop() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let (url, relay, server) =
        spawn_relay(provider, live_channel(&authority), Answer::Silent).await;

    let error = cmd_stop(&client(url), CHANNEL, SESSION, &authority, true, Some(1))
        .await
        .expect_err("unconfirmed");
    let command_id = {
        let records = relay.records.lock().expect("records");
        assert_eq!(records.published.len(), 1, "one stop, never retried");
        assert!(records.receipt_queries >= 1);
        decode_coding_session_lifecycle_command(&records.published[0].content)
            .expect("44221")
            .command_id
    };
    assert!(
        matches!(&error, CliError::Unconfirmed(message) if message.contains(&command_id)),
        "unconfirmed keeps the commandId: {error:?}"
    );
    assert_eq!(crate::error::exit_code(&error), 5);
    server.abort();
}

/// Each refusal: exit 1, and the relay saw no publish and no receipt wait.
async fn assert_refused_without_publishing(
    channel: Vec<Value>,
    provider: nostr::Keys,
    authority: &str,
    session: &str,
    needle: &str,
) {
    let (url, relay, server) = spawn_relay(provider, channel, Answer::Stopped).await;
    let error = cmd_stop(&client(url), CHANNEL, session, authority, true, Some(5))
        .await
        .expect_err("refused before publishing");
    assert_eq!(crate::error::exit_code(&error), 1, "{error:?}");
    assert!(error.to_string().contains(needle), "{needle:?} in {error}");
    let records = relay.records.lock().expect("records");
    assert!(records.published.is_empty(), "nothing published: {error}");
    assert_eq!(records.receipt_queries, 0);
    drop(records);
    server.abort();
}

#[tokio::test]
async fn an_unknown_session_publishes_nothing() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    assert_refused_without_publishing(
        live_channel(&authority),
        provider,
        &authority,
        "00000000-0000-4000-8000-000000000000",
        "no execution with sessionId",
    )
    .await;
}

#[tokio::test]
async fn an_already_stopped_session_publishes_nothing() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let mut channel = live_channel(&authority);
    channel.push(metadata_event(
        &authority,
        &"03".repeat(32),
        3_000,
        &target(SESSION, 2),
        SessionStatus::Stopped,
    ));
    assert_refused_without_publishing(channel, provider, &authority, SESSION, "already stopped")
        .await;
}

#[tokio::test]
async fn a_stopped_receipt_ahead_of_metadata_still_counts_as_stopped() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let mut channel = live_channel(&authority);
    channel.push(signed_receipt(
        &provider,
        "earlier-stop",
        ReceiptStatus::Stopped,
        Some(target(SESSION, 2)),
        None,
    ));
    assert_refused_without_publishing(channel, provider, &authority, SESSION, "already stopped")
        .await;
}

#[tokio::test]
async fn an_authority_that_is_not_the_targets_provider_publishes_nothing() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let stranger = nostr::Keys::generate().public_key().to_hex();
    assert_refused_without_publishing(
        live_channel(&authority),
        provider,
        &stranger,
        SESSION,
        "is not the provider of",
    )
    .await;
}

#[tokio::test]
async fn two_executions_sharing_a_session_id_publish_nothing() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let other = nostr::Keys::generate().public_key().to_hex();
    let mut channel = live_channel(&authority);
    channel.push(metadata_event(
        &other,
        &"04".repeat(32),
        2_500,
        &target(SESSION, 1),
        SessionStatus::Idle,
    ));
    assert_refused_without_publishing(
        channel,
        provider,
        &authority,
        SESSION,
        "different executions",
    )
    .await;
}

#[tokio::test]
async fn malformed_arguments_publish_nothing() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    for (authority, wait, timeout) in [
        (authority.to_uppercase(), true, Some(5)),
        (authority.clone(), true, Some(0)),
        (authority.clone(), false, Some(5)),
    ] {
        assert_refused_without_publishing_args(
            live_channel(&provider.public_key().to_hex()),
            provider.clone(),
            &authority,
            wait,
            timeout,
        )
        .await;
    }
}

async fn assert_refused_without_publishing_args(
    channel: Vec<Value>,
    provider: nostr::Keys,
    authority: &str,
    wait: bool,
    timeout: Option<u64>,
) {
    let (url, relay, server) = spawn_relay(provider, channel, Answer::Stopped).await;
    let error = cmd_stop(&client(url), CHANNEL, SESSION, authority, wait, timeout)
        .await
        .expect_err("usage refusal");
    assert_eq!(crate::error::exit_code(&error), 1, "{error:?}");
    assert!(relay.records.lock().expect("records").published.is_empty());
    server.abort();
}

#[test]
fn a_stopped_receipt_naming_another_target_or_signer_is_never_a_confirmation() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let wanted = target(SESSION, 2);

    let wrong_target = signed_receipt(
        &provider,
        "cmd-1",
        ReceiptStatus::Stopped,
        Some(target(SESSION, 1)),
        None,
    );
    assert!(
        classify_stop_receipts(&[wrong_target], CHANNEL, "cmd-1", &authority, &wanted).is_err()
    );

    let stranger = signed_receipt(
        &nostr::Keys::generate(),
        "cmd-1",
        ReceiptStatus::Stopped,
        Some(wanted.clone()),
        None,
    );
    assert_eq!(
        classify_stop_receipts(&[stranger], CHANNEL, "cmd-1", &authority, &wanted),
        Ok(None),
        "a receipt the named provider did not sign answers nothing"
    );

    let right = signed_receipt(
        &provider,
        "cmd-1",
        ReceiptStatus::Stopped,
        Some(wanted.clone()),
        None,
    );
    assert!(matches!(
        classify_stop_receipts(&[right], CHANNEL, "cmd-1", &authority, &wanted),
        Ok(Some(StopAnswer::Stopped { .. }))
    ));
}
