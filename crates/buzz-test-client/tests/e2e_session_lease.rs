//! End-to-end authority, register, and cold-read contract for session leases.
//!
//! This target requires an already-running relay backed by Postgres and Redis:
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 cargo test -p buzz-test-client \
//!   --test e2e_session_lease -- --ignored --nocapture
//! ```
//!
//! `DATABASE_URL` defaults to the repository's local development database and
//! is used only to prove that accepted kind-24223 events never become rows.

use std::time::Duration;

use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_lease::{CodingSessionLease, CodingSessionLeaseState};
use buzz_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::LifecycleReceipt;
use buzz_sdk::{
    build_coding_session_lease, build_coding_session_lifecycle_command,
    build_coding_session_lifecycle_receipt, build_join,
};
use buzz_test_client::{BuzzTestClient, RelayMessage};
use nostr::{Alphabet, Event, EventBuilder, Filter, Keys, Kind, SingleLetterTag, Tag};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

const LEASE_KIND: u16 = 24_223;

fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_owned())
}

fn relay_http_url() -> String {
    relay_url()
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .trim_end_matches('/')
        .to_owned()
}

fn lease_filter(channel_id: Uuid) -> Filter {
    Filter::new().kind(Kind::Custom(LEASE_KIND)).custom_tags(
        SingleLetterTag::lowercase(Alphabet::H),
        [channel_id.to_string()],
    )
}

async fn create_channel(client: &mut BuzzTestClient, owner: &Keys) -> Uuid {
    let channel_id = Uuid::new_v4();
    let event = EventBuilder::new(Kind::Custom(9007), "")
        .tags([
            Tag::parse(["h", &channel_id.to_string()]).unwrap(),
            Tag::parse(["name", &format!("session-lease-e2e-{channel_id}")]).unwrap(),
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

fn lease_event(
    keys: &Keys,
    channel_id: Uuid,
    command_id: &str,
    target: CodingSessionTarget,
    state: CodingSessionLeaseState,
    sequence: u64,
) -> Event {
    let payload = CodingSessionLease::new(target, state, sequence).expect("valid lease payload");
    build_coding_session_lease(channel_id, command_id, &payload)
        .expect("build lease")
        .sign_with_keys(keys)
        .expect("sign lease")
}

async fn ws_query(client: &mut BuzzTestClient, name: &str, filter: Filter) -> Vec<Event> {
    let sub_id = format!("e2e-session-lease-{name}-{}", Uuid::new_v4());
    client
        .subscribe(&sub_id, vec![filter])
        .await
        .expect("subscribe");
    let events = client
        .collect_until_eose(&sub_id, Duration::from_secs(10))
        .await
        .expect("collect until EOSE");
    client.close_subscription(&sub_id).await.ok();
    events
}

async fn recv_event_id(client: &mut BuzzTestClient, sub_id: &str, event_id: nostr::EventId) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .expect("event fan-out timed out");
        match client
            .recv_event(remaining)
            .await
            .expect("receive live event")
        {
            RelayMessage::Event {
                subscription_id,
                event,
            } if subscription_id == sub_id && event.id == event_id => return,
            _ => {}
        }
    }
}

async fn assert_no_event_id(client: &mut BuzzTestClient, sub_id: &str, event_id: nostr::EventId) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) {
        match client.recv_event(remaining).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == sub_id && event.id == event_id => {
                panic!("duplicate lease replay fanned out a second time")
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

async fn http_query_raw(pubkey: &str, filters: Value) -> (StatusCode, Value) {
    let response = Client::new()
        .post(format!("{}/query", relay_http_url()))
        .header("X-Pubkey", pubkey)
        .json(&filters)
        .send()
        .await
        .expect("POST /query");
    let status = response.status();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn assert_ws_unscoped_query_rejected(client: &mut BuzzTestClient) {
    let sub_id = format!("e2e-session-lease-unscoped-{}", Uuid::new_v4());
    client
        .subscribe(&sub_id, vec![Filter::new().kind(Kind::Custom(LEASE_KIND))])
        .await
        .expect("subscribe unscoped");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .expect("unscoped REQ was neither rejected nor completed");
        match client.recv_event(remaining).await.expect("receive CLOSED") {
            RelayMessage::Closed {
                subscription_id,
                message,
            } if subscription_id == sub_id => {
                assert!(
                    message.contains("explicit #h"),
                    "unexpected unscoped rejection: {message}"
                );
                return;
            }
            _ => {}
        }
    }
}

async fn assert_events_absent_from_postgres(events: &[&Event]) {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://buzz:buzz_dev@localhost:5432/buzz".to_owned() // sadscan:disable np.postgres.1
    });
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("connect to e2e Postgres");
    for event in events {
        let row = sqlx::query("SELECT COUNT(*) AS count FROM events WHERE id = $1")
            .bind(event.id.to_bytes().to_vec())
            .fetch_one(&pool)
            .await
            .expect("query lease persistence");
        let count: i64 = row.try_get("count").expect("decode count");
        assert_eq!(count, 0, "ephemeral lease {} reached Postgres", event.id);
    }
}

#[tokio::test]
#[ignore = "requires running relay, Postgres, and Redis"]
async fn session_lease_authority_register_and_cold_reads_are_end_to_end() {
    let url = relay_url();
    let operator = Keys::generate();
    let provider = Keys::generate();
    let attacker = Keys::generate();
    let mut operator_ws = BuzzTestClient::connect(&url, &operator)
        .await
        .expect("operator connect");
    let mut provider_ws = BuzzTestClient::connect(&url, &provider)
        .await
        .expect("provider connect");
    let mut attacker_ws = BuzzTestClient::connect(&url, &attacker)
        .await
        .expect("attacker connect");

    let channel_id = create_channel(&mut operator_ws, &operator).await;
    join_channel(&mut provider_ws, &provider, channel_id).await;
    join_channel(&mut attacker_ws, &attacker, channel_id).await;
    let other_channel_id = create_channel(&mut operator_ws, &operator).await;
    join_channel(&mut provider_ws, &provider, other_channel_id).await;

    let command_id = format!("create-{}", Uuid::new_v4());
    let target = CodingSessionTarget {
        driver: "codex-acp".to_owned(),
        instance_id: format!("provider-{}", Uuid::new_v4()),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let create = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: None,
            genesis_ref: None,
            provider_instance_ref: target
                .instance_id
                .as_str()
                .try_into()
                .expect("instance ref is a valid alias"),
            provider_authority_pubkey: provider.public_key().to_hex(),
            model: None,
            title: None,
            initial_turn: None,
            actor: None,
            role: None,
            hire_ref: None,
            routing: None,
        },
    };
    let command_event = build_coding_session_lifecycle_command(channel_id, &create)
        .expect("build create")
        .sign_with_keys(&operator)
        .expect("sign create");
    let ok = operator_ws
        .send_event(command_event.clone())
        .await
        .expect("send create");
    assert!(ok.accepted, "create command rejected: {}", ok.message);

    let receipt = LifecycleReceipt::created(&command_id, &target);
    let receipt_event = build_coding_session_lifecycle_receipt(
        channel_id,
        &command_id,
        &serde_json::to_string(&receipt).unwrap(),
    )
    .expect("build receipt")
    .sign_with_keys(&provider)
    .expect("sign receipt");
    let ok = provider_ws
        .send_event(receipt_event.clone())
        .await
        .expect("send receipt");
    assert!(ok.accepted, "provider receipt rejected: {}", ok.message);

    let live_one = lease_event(
        &provider,
        channel_id,
        &command_id,
        target.clone(),
        CodingSessionLeaseState::Live,
        1,
    );
    let ok = provider_ws
        .send_event(live_one.clone())
        .await
        .expect("send first live lease");
    assert!(ok.accepted, "valid provider lease rejected: {}", ok.message);

    let wrong_signer = lease_event(
        &attacker,
        channel_id,
        &command_id,
        target.clone(),
        CodingSessionLeaseState::Live,
        2,
    );
    assert!(
        !attacker_ws
            .send_event(wrong_signer.clone())
            .await
            .expect("send wrong-signer lease")
            .accepted,
        "wrong signer established liveness"
    );
    let wrong_command = lease_event(
        &provider,
        channel_id,
        "missing-command",
        target.clone(),
        CodingSessionLeaseState::Live,
        2,
    );
    assert!(
        !provider_ws
            .send_event(wrong_command.clone())
            .await
            .expect("send wrong-command lease")
            .accepted,
        "wrong command established liveness"
    );
    let mut mismatched_target = target.clone();
    mismatched_target.session_id = Uuid::new_v4().to_string();
    let wrong_target = lease_event(
        &provider,
        channel_id,
        &command_id,
        mismatched_target,
        CodingSessionLeaseState::Live,
        2,
    );
    assert!(
        !provider_ws
            .send_event(wrong_target.clone())
            .await
            .expect("send wrong-target lease")
            .accepted,
        "wrong target established liveness"
    );
    let wrong_channel = lease_event(
        &provider,
        other_channel_id,
        &command_id,
        target.clone(),
        CodingSessionLeaseState::Live,
        2,
    );
    assert!(
        !provider_ws
            .send_event(wrong_channel.clone())
            .await
            .expect("send wrong-channel lease")
            .accepted,
        "cross-channel authority established liveness"
    );

    let live_sub = format!("e2e-session-lease-live-{}", Uuid::new_v4());
    provider_ws
        .subscribe(&live_sub, vec![lease_filter(channel_id)])
        .await
        .expect("subscribe live leases");
    let initial = provider_ws
        .collect_until_eose(&live_sub, Duration::from_secs(10))
        .await
        .expect("initial cold lease");
    assert_eq!(initial, vec![live_one.clone()]);

    let live_two = lease_event(
        &provider,
        channel_id,
        &command_id,
        target.clone(),
        CodingSessionLeaseState::Live,
        2,
    );
    let ok = provider_ws
        .send_event(live_two.clone())
        .await
        .expect("send higher live lease");
    assert!(ok.accepted, "higher live lease rejected: {}", ok.message);
    recv_event_id(&mut provider_ws, &live_sub, live_two.id).await;

    let ok = provider_ws
        .send_event(live_two.clone())
        .await
        .expect("replay exact duplicate");
    assert!(
        ok.accepted,
        "exact duplicate was not idempotent: {}",
        ok.message
    );
    assert_no_event_id(&mut provider_ws, &live_sub, live_two.id).await;

    let replaced = ws_query(&mut operator_ws, "higher-wins", lease_filter(channel_id)).await;
    assert_eq!(replaced, vec![live_two.clone()]);

    let released = lease_event(
        &provider,
        channel_id,
        &command_id,
        target.clone(),
        CodingSessionLeaseState::Released,
        4,
    );
    let ok = provider_ws
        .send_event(released.clone())
        .await
        .expect("send release");
    assert!(ok.accepted, "release rejected: {}", ok.message);
    recv_event_id(&mut provider_ws, &live_sub, released.id).await;

    let delayed_lower_live = lease_event(
        &provider,
        channel_id,
        &command_id,
        target,
        CodingSessionLeaseState::Live,
        3,
    );
    assert!(
        !provider_ws
            .send_event(delayed_lower_live.clone())
            .await
            .expect("send delayed lower live")
            .accepted,
        "release tombstone did not fence a delayed lower live lease"
    );
    provider_ws.close_subscription(&live_sub).await.ok();

    let cold_ws = ws_query(&mut operator_ws, "cold-release", lease_filter(channel_id)).await;
    assert_eq!(cold_ws, vec![released.clone()]);
    assert_ws_unscoped_query_rejected(&mut operator_ws).await;

    let scoped_body = json!([{
        "kinds": [LEASE_KIND],
        "#h": [channel_id.to_string()],
        "limit": 10
    }]);
    let (status, body) = http_query_raw(&operator.public_key().to_hex(), scoped_body).await;
    assert_eq!(status, StatusCode::OK, "scoped HTTP query failed: {body}");
    let cold_http: Vec<Event> = serde_json::from_value(body).expect("decode HTTP lease events");
    assert_eq!(cold_http, vec![released.clone()]);
    assert_eq!(cold_http[0].pubkey, provider.public_key());
    assert_eq!(cold_http[0].sig, released.sig);

    let (status, body) = http_query_raw(
        &operator.public_key().to_hex(),
        json!([{ "kinds": [LEASE_KIND] }]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unscoped HTTP lease query was not rejected: {body}"
    );

    assert_events_absent_from_postgres(&[
        &live_one,
        &wrong_signer,
        &wrong_command,
        &wrong_target,
        &wrong_channel,
        &live_two,
        &released,
        &delayed_lower_live,
    ])
    .await;

    // The public cold-read contract preserves the provider event id, signer,
    // and signature as asserted above. Redis's acceptedAt/expiresAt and the
    // command/receipt proof IDs are intentionally not exposed until the future
    // relay-signed kind-39011 projection, so this black-box target cannot assert
    // those internal provenance fields without violating the public seam.
    assert_ne!(command_event.id, receipt_event.id);
}
