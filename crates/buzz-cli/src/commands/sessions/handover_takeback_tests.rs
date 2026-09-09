//! Tests for what a takeback tells its caller to do next.
//!
//! A claim receipt means the **fence moved**. It does not mean an execution
//! resumed, and the composition run showed what conflating the two costs: a
//! takeback printed success and the claimant's next turn came back
//! `turn_dropped/NO_LIVE_EXECUTION`. So the run reads the claimed body's own
//! liveness afterwards and names the route that is actually open.
//!
//! Both live and dead cases go through the real `claim_session` against a
//! local axum relay — nothing here reaches a real one, and no Postgres or
//! Redis is involved.

use nostr::Keys;
use serde_json::json;

use super::*;

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "1f2e3d4c-5b6a-4798-8765-43210fedcba9";

fn hex64(byte: &str) -> String {
    byte.repeat(32)
}

// ── a takeback moves the fence; it does not resume anything ──────────────

#[test]
fn the_next_action_after_a_takeback_says_which_route_is_open() {
    use super::super::handover::{next_action_line, BodyLiveness, CLAIM_MOVES_THE_FENCE};

    assert_eq!(
        CLAIM_MOVES_THE_FENCE,
        "a claim receipt means the fence moved, never that an execution resumed"
    );

    let body = hex64("bb");
    let live = next_action_line(
        BodyLiveness::Live,
        &body,
        CHANNEL,
        SESSION,
        Some("coding-session/v1|x"),
    );
    assert_eq!(
        live,
        format!(
            "the execution on {} is live: steer it with `bee sessions send --channel {CHANNEL} \
             --session-ref {SESSION} --to coding-session/v1|x`",
            super::super::crew::short_pubkey(&body)
        )
    );

    let dead = next_action_line(BodyLiveness::NotLive, &body, CHANNEL, SESSION, None);
    assert_eq!(
        dead,
        format!(
            "no live execution on {}: resume it (desktop, or a 44221 session.resume) or \
             re-address an owed turn with `bee sessions send --channel {CHANNEL} --readdress \
             <commandId>`",
            super::super::crew::short_pubkey(&body)
        ),
        "the reconnect half is the sentence `plan_readdress` already prints, and the re-address \
         half is the form `sessions send --help` documents: there is no `bee sessions resume`, \
         and this must not invent one"
    );

    let unknown = next_action_line(BodyLiveness::Unknown, &body, CHANNEL, SESSION, None);
    assert!(
        unknown.contains("failed, so whether an execution is live there is unknown")
            && unknown.contains("resume it (desktop, or a 44221 session.resume)")
            && unknown.contains("--readdress <commandId>"),
        "a failed read names both routes and says the read failed: {unknown}"
    );
}

/// One takeback against a fake relay, with the claimed body's lease present or
/// absent, returning what the run concluded and everything it published.
async fn takeback_against(lease_live: bool) -> (String, Option<String>, Vec<nostr::Event>) {
    use axum::body::Bytes;
    use axum::extract::State;
    use axum::response::Json;
    use axum::routing::{get, post};
    use axum::Router;
    use std::sync::{Arc, Mutex};

    use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
    use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
    use buzz_core::coding_session_lease::{CodingSessionLease, CodingSessionLeaseState};
    use buzz_sdk::builders::build_coding_session_genesis;

    use super::super::handover::{
        body_liveness_of, fetch_executions, live_target_on, load_handover_state, next_action_line,
        BodyLiveness,
    };
    use super::super::handover_claim::claim_session;
    use super::super::handover_render::VerificationNotes;

    let founder = Keys::generate();
    let relay = Keys::generate();
    // The body: the provider key that signs the execution's metadata and lease.
    let body_keys = Keys::generate();
    let body = body_keys.public_key().to_hex();

    let genesis_event = build_coding_session_genesis(
        uuid::Uuid::parse_str(CHANNEL).expect("channel"),
        &CodingSessionGenesisPayload::new(SESSION.to_owned()),
    )
    .expect("genesis builder")
    .sign_with_keys(&founder)
    .expect("sign");
    let genesis_ref = genesis_event.id.to_hex();

    let target = CodingSessionTarget {
        driver: "claude-agent-acp".to_owned(),
        instance_id: "instance-1".to_owned(),
        session_id: "session-1".to_owned(),
        generation: 1,
    };
    let target_key = coding_session_target_key(&target);
    // Built through the typed payload, not by hand: `decode_metadata`
    // deserializes `SessionMetadata`, and a hand-written object missing one of
    // its required keys decodes to nothing and yields no execution at all —
    // which reads as "not live" and would have made the live case pass for the
    // wrong reason.
    let payload = buzz_core::coding_session_payload::SessionMetadata {
        schema: buzz_core::coding_session_payload::METADATA_SCHEMA.to_owned(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: None,
        role: None,
        provider: Some("claude-primary".try_into().expect("alias")),
        runtime: Some("claude".try_into().expect("runtime")),
        model: Some("claude-opus".to_owned()),
        status: buzz_core::coding_session_payload::SessionStatus::Idle,
        branch: None,
        capabilities: buzz_core::coding_session_payload::Capabilities::v1_claude(),
        session_ref: Some(SESSION.to_owned()),
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        handover: None,
    };
    let metadata = json!({
        "id": "aa".repeat(32),
        "pubkey": body,
        "kind": buzz_sdk::kind::KIND_CODING_SESSION_METADATA,
        "created_at": 1_700_000_000,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["csm-v", "csm1-1"], ["cs-target", target_key]],
        "content": serde_json::to_string(&payload).expect("serialize"),
    });

    let lease =
        CodingSessionLease::new(target.clone(), CodingSessionLeaseState::Live, 1).expect("lease");
    let lease_event = json!({
        "id": "bb".repeat(32),
        "pubkey": body,
        "kind": buzz_core::kind::KIND_CODING_SESSION_LEASE,
        "created_at": 1_700_000_001,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["cs-target", target_key]],
        "content": serde_json::to_string(&lease).expect("serialize"),
    });

    struct Fixture {
        rows: Vec<serde_json::Value>,
        published: Mutex<Vec<nostr::Event>>,
    }
    let mut rows = vec![
        serde_json::to_value(&genesis_event).expect("json"),
        metadata,
    ];
    if lease_live {
        rows.push(lease_event);
    }
    let fixture = Arc::new(Fixture {
        rows,
        published: Mutex::new(Vec::new()),
    });

    let app = Router::new()
        .route(
            "/",
            get({
                let relay_self = relay.public_key().to_hex();
                move || {
                    let relay_self = relay_self.clone();
                    async move { Json(json!({ "self": relay_self })) }
                }
            }),
        )
        .route(
            "/events",
            post(
                |State(fixture): State<Arc<Fixture>>, body: Bytes| async move {
                    let event: nostr::Event = serde_json::from_slice(&body).expect("event JSON");
                    let event_id = event.id.to_hex();
                    fixture.published.lock().expect("published").push(event);
                    Json(json!({ "event_id": event_id, "accepted": true, "message": "" }))
                },
            ),
        )
        .route(
            "/query",
            post(
                // Filter the whole fixture by the kinds each read asks for,
                // rather than guessing which read is which.
                |State(fixture): State<Arc<Fixture>>, body: Bytes| async move {
                    let filters: serde_json::Value =
                        serde_json::from_slice(&body).unwrap_or(json!([]));
                    let wanted: Vec<u64> = filters
                        .get(0)
                        .and_then(|filter| filter.get("kinds"))
                        .and_then(serde_json::Value::as_array)
                        .map(|kinds| kinds.iter().filter_map(serde_json::Value::as_u64).collect())
                        .unwrap_or_default();
                    let rows: Vec<serde_json::Value> = fixture
                        .rows
                        .iter()
                        .filter(|row| {
                            row.get("kind")
                                .and_then(serde_json::Value::as_u64)
                                .is_some_and(|kind| wanted.contains(&kind))
                        })
                        .cloned()
                        .collect();
                    Json(serde_json::Value::Array(rows))
                },
            ),
        )
        .with_state(fixture.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("fake relay");
    });

    let client = BuzzClient::new(url, founder.clone(), None, None).expect("client");
    let state = load_handover_state(&client, CHANNEL, SESSION, Some(&genesis_ref))
        .await
        .expect("state");
    let mut notes = VerificationNotes::default();
    // The real claim path, and then the real liveness read `cmd_claim` does.
    claim_session(&client, &state, &body, 1, &mut notes)
        .await
        .expect("the founder may take their own session over");
    let rows = fetch_executions(&client, CHANNEL, SESSION)
        .await
        .expect("executions");
    assert!(
        rows.iter().any(|row| row.execution.signer == body),
        "the fixture must actually produce an execution on the claimed body — with none,          `not live` would be right for the wrong reason and the live case could never pass"
    );
    let liveness = body_liveness_of(&rows, &body);
    let line = next_action_line(
        liveness,
        &body,
        CHANNEL,
        SESSION,
        live_target_on(&rows, &body).as_deref(),
    );
    assert_ne!(
        liveness,
        BodyLiveness::Unknown,
        "the fixture answers every read, so `unknown` would mean a broken fixture"
    );

    let published = fixture.published.lock().expect("published").clone();
    server.abort();
    (line, Some(liveness.word().to_owned()), published)
}

#[tokio::test]
async fn a_takeback_onto_a_dead_body_names_reconnect_and_readdress() {
    // The composition run: the takeback succeeded, printed success, and the
    // claimant's next turn came back `turn_dropped/NO_LIVE_EXECUTION`.
    let (line, word, published) = takeback_against(false).await;
    assert_eq!(word.as_deref(), Some("not live"));
    assert!(
        line.starts_with("no live execution on ")
            && line.contains("resume it (desktop, or a 44221 session.resume)")
            && line.contains("--readdress <commandId>"),
        "{line}"
    );
    assert!(
        !line.contains("steer it with"),
        "a dead body must not be offered the steer route: {line}"
    );
    assert_published_only_the_takeover(&published);
}

#[tokio::test]
async fn a_takeback_onto_a_live_body_names_the_steer_route() {
    let (line, word, published) = takeback_against(true).await;
    assert_eq!(word.as_deref(), Some("live"));
    assert!(
        line.contains("is live: steer it with `bee sessions send")
            && line.contains("--session-ref"),
        "{line}"
    );
    assert!(
        !line.contains("session.resume"),
        "a live body is not offered the reconnect route: {line}"
    );
    assert_published_only_the_takeover(&published);
}

/// A claim publishes exactly one event: its own 44228 link.
fn assert_published_only_the_takeover(published: &[nostr::Event]) {
    let kinds: Vec<u32> = published
        .iter()
        .map(|event| u32::from(event.kind.as_u16()))
        .collect();
    assert_eq!(
        kinds,
        vec![buzz_core::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION],
        "claiming a session steers nothing: no 44220 turn and no 44221 resume may be published \
         by `handover claim`, whatever the body's liveness turns out to be"
    );
}
