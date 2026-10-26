//! Signed fake-relay tests through real clap parsing, dispatch and HTTP transport.
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use beekeeper_core::{continuity as c, engram};
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
};

const CHANNEL: &str = "00000000-0000-0000-0000-000000000001";
fn actor() -> Keys {
    Keys::parse(&"1".repeat(64)).unwrap()
}
fn owner() -> Keys {
    Keys::parse(&"2".repeat(64)).unwrap()
}

#[derive(Default)]
struct Relay {
    heads: Vec<Event>,
    evidence: Vec<Event>,
    publishes: Vec<Event>,
    queries: Vec<Value>,
    lose_ack: bool,
    fail_readback: bool,
    deny_evidence: bool,
    malformed: bool,
    race: Option<Event>,
}
type Shared = Arc<Mutex<Relay>>;
async fn query(State(state): State<Shared>, Json(filters): Json<Value>) -> Response {
    let mut state = state.lock().unwrap();
    state.queries.push(filters.clone());
    let filter = &filters[0];
    if filter["kinds"][0] == 30174 {
        if state.fail_readback && !state.publishes.is_empty() {
            return (StatusCode::BAD_REQUEST, "unavailable").into_response();
        }
        if state.malformed {
            return Json(json!([{"invalid":true}])).into_response();
        }
        Json(json!(state.heads)).into_response()
    } else {
        assert!(filter["ids"].is_array());
        assert_eq!(filter["#h"][0], CHANNEL);
        if state.deny_evidence {
            return (StatusCode::FORBIDDEN, "private denial canary").into_response();
        }
        let events: Vec<_> = state
            .evidence
            .iter()
            .filter(|event| filter["ids"][0] == event.id.to_hex())
            .cloned()
            .collect();
        Json(json!(events)).into_response()
    }
}
async fn publish(State(state): State<Shared>, Json(event): Json<Event>) -> Response {
    event.verify().unwrap();
    let mut state = state.lock().unwrap();
    state.publishes.push(event.clone());
    state.heads = vec![state.race.clone().unwrap_or(event)];
    // An unusable acknowledgement models body loss without a long socket timeout.
    if state.lose_ack {
        return "truncated acknowledgement".into_response();
    }
    Json(json!({"accepted":true,"message":"saved"})).into_response()
}
struct Fixture {
    url: String,
    state: Shared,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let state = Arc::new(Mutex::new(Relay::default()));
        let router = Router::new()
            .route("/query", post(query))
            .route("/events", post(publish))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, state, task }
    }
    fn scope(&self) -> c::Scope {
        c::Scope::new(&self.url, CHANNEL, None).unwrap()
    }
    fn record(&self, text: &str) -> c::Record {
        c::Record {
            schema: c::SCHEMA.into(),
            scope: self.scope(),
            base_event_id: None,
            claims: vec![c::Claim {
                id: "deployment".into(),
                state: c::ClaimState::Asserted,
                text: text.into(),
                observed_at: 1,
                evidence: vec![],
                correction: None,
            }],
        }
    }
    fn event(&self, record: &c::Record, time: u64) -> Event {
        engram::build_event(
            &actor(),
            &owner().public_key(),
            &record.to_body().unwrap(),
            time,
        )
        .unwrap()
    }
    async fn run(
        &self,
        args: &[&str],
        value: Option<&str>,
        owner_read: bool,
        auth: bool,
    ) -> Output {
        let url = self.url.clone();
        let args: Vec<String> = args.iter().map(|v| v.to_string()).collect();
        let input = value.map(str::to_owned);
        tokio::task::spawn_blocking(move || {
            let mut command = Command::new(env!("CARGO_BIN_EXE_bee"));
            command
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("BEEKEEPER_RELAY_URL", url)
                .env(
                    "BEEKEEPER_PRIVATE_KEY",
                    if owner_read {
                        "2".repeat(64)
                    } else {
                        "1".repeat(64)
                    },
                )
                .env("BEEKEEPER_TIMEOUT_SECS", "1")
                .args(["--format", "compact", "mem", "continuity"])
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if auth {
                command.env(
                    "BEEKEEPER_AUTH_TAG",
                    beekeeper_sdk::nip_oa::compute_auth_tag(&owner(), &actor().public_key(), "")
                        .unwrap(),
                );
            }
            let mut child = command.spawn().unwrap();
            if let Some(input) = input {
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(input.as_bytes())
                    .unwrap();
            }
            child.wait_with_output().unwrap()
        })
        .await
        .unwrap()
    }
    async fn put(&self, record: &c::Record, base: &str) -> Output {
        self.run(
            &[
                "put",
                "--channel",
                CHANNEL,
                "--file",
                "-",
                "--base-event",
                base,
            ],
            Some(&serde_json::to_string(record).unwrap()),
            false,
            true,
        )
        .await
    }
    async fn get(&self) -> Output {
        self.run(&["get", "--channel", CHANNEL], None, false, true)
            .await
    }
}
fn output(result: &Output, code: i32) -> Value {
    assert_eq!(
        result.status.code(),
        Some(code),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
fn code(result: &Output, code: i32) {
    assert_eq!(
        result.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn signed_evidence() -> Event {
    EventBuilder::new(Kind::from(9), "Observed B")
        .tags([Tag::parse(["h", CHANNEL]).unwrap()])
        .sign_with_keys(&owner())
        .unwrap()
}
fn reference(event: &Event) -> c::Evidence {
    c::Evidence {
        kind: 9,
        event_id: event.id.to_hex(),
        channel: CHANNEL.into(),
        sha256: c::value_hash(event.content.as_bytes()),
        excerpt: "Observed B".into(),
    }
}

#[tokio::test]
async fn create_correct_withdraw_and_owner_read_through_cli() {
    let f = Fixture::new().await;
    let mut record = f.record("A");
    let a = output(&f.put(&record, "absent").await, 0);
    assert_eq!(a["status"], "confirmed_at_read");
    let a_id = a["event_id"].as_str().unwrap();
    let source = signed_evidence();
    record.base_event_id = Some(a_id.into());
    record.claims[0].text = "B".into();
    record.claims[0].evidence = vec![reference(&source)];
    record.claims[0].correction = Some(c::Correction {
        prior_head: a_id.into(),
        prior_text: "A".into(),
        reason: "new observation".into(),
    });
    f.state.lock().unwrap().evidence.push(source);
    let b = output(&f.put(&record, a_id).await, 0);
    let b_id = b["event_id"].as_str().unwrap();
    assert_ne!(a_id, b_id);
    let got = output(&f.get().await, 0);
    assert_eq!(
        got["claims"][0]["status"],
        "assertion_with_readable_evidence"
    );
    assert_eq!(got["claims"][0]["claim"]["text"], "B");
    assert_eq!(got["claims"][0]["claim"]["correction"]["prior_text"], "A");
    let recovered = f
        .run(
            &[
                "get",
                "--channel",
                CHANNEL,
                "--agent",
                &actor().public_key().to_hex(),
            ],
            None,
            true,
            false,
        )
        .await;
    assert_eq!(output(&recovered, 0)["head_event_id"], b_id);
    record.base_event_id = Some(b_id.into());
    record.claims[0].state = c::ClaimState::Withdrawn;
    record.claims[0].correction = Some(c::Correction {
        prior_head: b_id.into(),
        prior_text: "B".into(),
        reason: "withdrawn".into(),
    });
    output(&f.put(&record, b_id).await, 0);
    let retired = output(&f.get().await, 0);
    assert_eq!(retired["claims"][0]["status"], "withdrawn");
    assert!(retired["claims"][0].get("claim").is_none());
    assert_eq!(f.state.lock().unwrap().publishes.len(), 3);
}

#[tokio::test]
async fn stale_base_and_invalid_transition_never_publish() {
    let f = Fixture::new().await;
    let record = f.record("A");
    let head = f.event(&record, 1);
    f.state.lock().unwrap().heads.push(head.clone());
    assert_eq!(
        output(&f.put(&record, "absent").await, 5)["status"],
        "conflict"
    );
    let mut changed = record.clone();
    changed.base_event_id = Some(head.id.to_hex());
    changed.claims[0].text = "bad correction".into();
    code(&f.put(&changed, &head.id.to_hex()).await, 1);
    changed.claims.clear();
    code(&f.put(&changed, &head.id.to_hex()).await, 1);
    assert!(f.state.lock().unwrap().publishes.is_empty());
}

#[tokio::test]
async fn lost_ack_and_readback_are_unknown_with_exact_attempt_id_and_one_send() {
    for lose_ack in [true, false] {
        let f = Fixture::new().await;
        {
            let mut state = f.state.lock().unwrap();
            state.lose_ack = lose_ack;
            state.fail_readback = !lose_ack;
        }
        let result = output(&f.put(&f.record("A"), "absent").await, 2);
        assert_eq!(result["status"], "submitted_unknown");
        let state = f.state.lock().unwrap();
        assert_eq!(state.publishes.len(), 1);
        assert_eq!(result["event_id"], state.publishes[0].id.to_hex());
    }
}

#[tokio::test]
async fn readback_race_is_conflict_and_later_write_can_replace_confirmed() {
    let f = Fixture::new().await;
    let competitor = f.event(&f.record("concurrent"), 2);
    f.state.lock().unwrap().race = Some(competitor.clone());
    assert_eq!(
        output(&f.put(&f.record("A"), "absent").await, 5)["observed_head"],
        competitor.id.to_hex()
    );
    let f = Fixture::new().await;
    let confirmed = output(&f.put(&f.record("A"), "absent").await, 0);
    let later = f.event(&f.record("later unseen write"), u64::MAX - 1);
    f.state.lock().unwrap().heads = vec![later.clone()];
    let observed = output(&f.get().await, 0);
    assert_ne!(confirmed["event_id"], observed["head_event_id"]);
    assert_eq!(observed["head_event_id"], later.id.to_hex());
}

#[tokio::test]
async fn malformed_newest_and_unparseable_candidate_never_fall_back() {
    let f = Fixture::new().await;
    let old = f.event(&f.record("A"), 1);
    let bad = engram::build_event(
        &actor(),
        &owner().public_key(),
        &engram::Body::Memory {
            slug: f.scope().slug().unwrap(),
            value: Some("{}".into()),
        },
        2,
    )
    .unwrap();
    f.state.lock().unwrap().heads = vec![old, bad];
    let got = output(&f.get().await, 2);
    assert_eq!(got["reason"], "invalid_head");
    assert_eq!(got["claims"], json!([]));
    code(&f.put(&f.record("A"), "absent").await, 2);
    f.state.lock().unwrap().malformed = true;
    assert_eq!(output(&f.get().await, 2)["reason"], "invalid_candidate");
    assert!(f.state.lock().unwrap().publishes.is_empty());
}

#[tokio::test]
async fn evidence_is_fresh_and_missing_or_revoked_is_unknown() {
    let f = Fixture::new().await;
    let event = signed_evidence();
    let mut record = f.record("B");
    record.claims[0].evidence.push(reference(&event));
    {
        let mut state = f.state.lock().unwrap();
        state.heads = vec![f.event(&record, 1)];
        state.evidence.push(event);
    }
    assert_eq!(
        output(&f.get().await, 0)["claims"][0]["status"],
        "assertion_with_readable_evidence"
    );
    f.state.lock().unwrap().evidence.clear();
    assert_eq!(
        output(&f.get().await, 0)["claims"][0]["reasons"][0],
        "evidence_unavailable"
    );
    f.state.lock().unwrap().deny_evidence = true;
    let denied = f.get().await;
    assert_eq!(
        output(&denied, 0)["claims"][0]["reasons"][0],
        "evidence_unavailable"
    );
    assert!(!String::from_utf8_lossy(&denied.stderr).contains("private denial canary"));
}

#[tokio::test]
async fn visible_fork_is_unknown_and_requires_explicit_correction() {
    let f = Fixture::new().await;
    let a = f.record("A");
    let b = f.record("B");
    let winner = f.event(&b, 2);
    f.state.lock().unwrap().heads = vec![f.event(&a, 1), winner.clone()];
    assert_eq!(
        output(&f.get().await, 0)["claims"][0]["reasons"][0],
        "conflict"
    );
    let mut unresolved = b.clone();
    unresolved.base_event_id = Some(winner.id.to_hex());
    code(&f.put(&unresolved, &winner.id.to_hex()).await, 5);
    unresolved.claims[0].correction = Some(c::Correction {
        prior_head: winner.id.to_hex(),
        prior_text: "B".into(),
        reason: "resolve visible fork as B".into(),
    });
    output(&f.put(&unresolved, &winner.id.to_hex()).await, 0);
}

#[tokio::test]
async fn custody_flags_and_strict_input_are_enforced() {
    let f = Fixture::new().await;
    code(
        &f.run(
            &[
                "get",
                "--channel",
                CHANNEL,
                "--owner",
                &owner().public_key().to_hex(),
            ],
            None,
            false,
            false,
        )
        .await,
        3,
    );
    code(
        &f.run(
            &[
                "get",
                "--channel",
                CHANNEL,
                "--owner",
                &actor().public_key().to_hex(),
            ],
            None,
            false,
            true,
        )
        .await,
        3,
    );
    code(
        &f.run(
            &[
                "get",
                "--channel",
                CHANNEL,
                "--agent",
                &actor().public_key().to_hex(),
                "--owner",
                &owner().public_key().to_hex(),
            ],
            None,
            true,
            false,
        )
        .await,
        1,
    );
    code(
        &f.run(
            &["put", "--channel", CHANNEL, "--file", "-"],
            None,
            false,
            true,
        )
        .await,
        1,
    );
    code(
        &f.run(&["get", "--channel", "bad-uuid"], None, false, true)
            .await,
        1,
    );
    code(
        &f.run(
            &["get", "--channel", CHANNEL, "--claim", "INVALID"],
            None,
            false,
            true,
        )
        .await,
        1,
    );
    let raw = serde_json::to_string(&f.record("A")).unwrap();
    let duplicate = raw.replacen('{', "{\"schema\":\"beekeeper-continuity/1\",", 1);
    code(
        &f.run(
            &[
                "put",
                "--channel",
                CHANNEL,
                "--file",
                "-",
                "--base-event",
                "absent",
            ],
            Some(&duplicate),
            false,
            true,
        )
        .await,
        1,
    );
    assert!(f.state.lock().unwrap().queries.is_empty());
    assert!(f.state.lock().unwrap().publishes.is_empty());
}

#[tokio::test]
async fn cap_tombstone_and_future_clock_refuse_create() {
    let f = Fixture::new().await;
    let record = f.record("A");
    let head = f.event(&record, 1);
    f.state.lock().unwrap().heads = vec![head; 16];
    assert_eq!(output(&f.get().await, 2)["reason"], "incomplete");
    let tomb = engram::build_event(
        &actor(),
        &owner().public_key(),
        &engram::Body::Memory {
            slug: f.scope().slug().unwrap(),
            value: None,
        },
        2,
    )
    .unwrap();
    f.state.lock().unwrap().heads = vec![tomb];
    assert_eq!(output(&f.get().await, 0)["status"], "withdrawn_record");
    code(&f.put(&record, "absent").await, 5);
    let future = f.event(&record, u64::MAX - 1);
    f.state.lock().unwrap().heads = vec![future.clone()];
    let mut next = record.clone();
    next.base_event_id = Some(future.id.to_hex());
    code(&f.put(&next, &future.id.to_hex()).await, 1);
    assert!(f.state.lock().unwrap().publishes.is_empty());
}

#[tokio::test]
async fn socket_loss_after_store_does_not_resubmit() {
    use std::io::Read;
    let mut f = Fixture::new().await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    f.url = format!("http://{}", listener.local_addr().unwrap());
    let wire = std::thread::spawn(move || {
        let mut published = None;
        for index in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let boundary = loop {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
                if bytes.ends_with(b"\r\n\r\n") {
                    break bytes.len();
                }
            };
            let headers = String::from_utf8(bytes.clone()).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            bytes.resize(boundary + length, 0);
            socket.read_exact(&mut bytes[boundary..]).unwrap();
            if index == 0 {
                assert!(headers.starts_with("POST /query "));
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]",
                    )
                    .unwrap();
            } else {
                assert!(headers.starts_with("POST /events "));
                let event: Event = serde_json::from_slice(&bytes[boundary..]).unwrap();
                event.verify().unwrap();
                published = Some(event.id.to_hex());
                // Store completed, then drop the connection before sending any acknowledgement.
            }
        }
        published.unwrap()
    });
    let result = output(&f.put(&f.record("A"), "absent").await, 2);
    let id = wire.join().unwrap();
    assert_eq!(result["status"], "submitted_unknown");
    assert_eq!(result["event_id"], id);
}

#[tokio::test]
async fn source_integrity_and_signature_failures_are_not_current_assertions() {
    let f = Fixture::new().await;
    let source = signed_evidence();
    for reason in ["no_evidence", "evidence_hash", "evidence_excerpt"] {
        let mut record = f.record("B");
        if reason != "no_evidence" {
            let mut r = reference(&source);
            if reason == "evidence_hash" {
                r.sha256 = "0".repeat(64);
            } else {
                r.excerpt = "not in source".into();
            }
            record.claims[0].evidence.push(r);
        }
        {
            let mut state = f.state.lock().unwrap();
            state.heads = vec![f.event(&record, 1)];
            state.evidence = vec![source.clone()];
        }
        assert_eq!(output(&f.get().await, 0)["claims"][0]["reasons"][0], reason);
    }
    let mut broken: Value = serde_json::to_value(f.event(&f.record("A"), 1)).unwrap();
    broken["content"] = json!("modified ciphertext");
    f.state.lock().unwrap().heads = vec![serde_json::from_value(broken).unwrap()];
    assert_eq!(output(&f.get().await, 2)["reason"], "invalid_candidate");
}

#[tokio::test]
async fn fork_only_claim_remains_refused_until_core_can_express_resolution() {
    let f = Fixture::new().await;
    let losing = f.record("losing sibling only");
    let mut winner = losing.clone();
    winner.claims.clear();
    let head = f.event(&winner, 2);
    f.state.lock().unwrap().heads = vec![f.event(&losing, 1), head.clone()];
    assert_eq!(
        output(&f.get().await, 0)["claims"][0]["reasons"][0],
        "conflict"
    );
    winner.base_event_id = Some(head.id.to_hex());
    code(&f.put(&winner, &head.id.to_hex()).await, 5);
    // Current core contract forbids corrections on IDs absent from the winner.
    // Do not invent a different wire convention in this CLI lane.
    winner.claims = losing.claims;
    winner.claims[0].correction = Some(c::Correction {
        prior_head: head.id.to_hex(),
        prior_text: "losing sibling only".into(),
        reason: "explicit resolution requested".into(),
    });
    code(&f.put(&winner, &head.id.to_hex()).await, 1);
    assert!(f.state.lock().unwrap().publishes.is_empty());
}
