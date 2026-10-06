//! `bee sessions seat-repair`, driven end to end against a recording relay.
//!
//! These are not pure-function tests, deliberately. The claim under test is
//! *"this command never builds or submits a 44221"* — a claim about what
//! reaches the wire, which a fold or a report shape cannot answer. So the
//! command runs against a local axum server that stores every submitted event
//! and mints the relay's own acceptance receipts, and every assertion is made
//! against what that server actually received.
//!
//! The relay is a real one in the only sense that matters here: it verifies
//! nothing, but it answers `POST /query` from its own store, so the second
//! run of a repair sees the grant the first run wrote.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Response, StatusCode};
use axum::Router;
use beekeeper_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionPayload,
    CodingSessionAuthorityTransitionType,
};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload;
use beekeeper_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_payload::{
    Capabilities, LifecycleReceipt, ReceiptError, ReceiptStatus, SessionMetadata, SessionStatus,
    LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA,
};
use beekeeper_core::kind::{
    KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_SYSTEM_MESSAGE,
};
use beekeeper_sdk::builders::{
    build_coding_session_authority_transition, build_coding_session_genesis,
    build_coding_session_lifecycle_command, build_coding_session_lifecycle_receipt,
    build_coding_session_metadata,
};
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use uuid::Uuid;

use super::super::crew::{seat_repair_exit_code, SeatRepairOutcome};
use super::{cmd_seat_repair, seat_repair_document};
use crate::client::BeekeeperClient;
use crate::error::CliError;

const CHANNEL: &str = "f4829942-15a8-4e74-accd-51c8448f250f";
const SESSION: &str = "41712db5-519f-492f-a6e7-f8407c8ba1dc";
const COMMAND_ID: &str = "csl-53c9515e-repair";
const PROVIDER_INSTANCE: &str = "provider-1";

/// The relay's own store: what it will answer queries with, and everything a
/// command has asked it to store.
struct RecordingRelay {
    events: Mutex<Vec<Value>>,
    submitted: Mutex<Vec<Value>>,
    keys: Keys,
}

impl RecordingRelay {
    fn submitted_kinds(&self) -> Vec<u64> {
        self.submitted
            .lock()
            .expect("submitted lock")
            .iter()
            .filter_map(|event| event.get("kind").and_then(Value::as_u64))
            .collect()
    }

    fn submitted(&self) -> Vec<Value> {
        self.submitted.lock().expect("submitted lock").clone()
    }
}

fn tag_values<'a>(event: &'a Value, name: &str) -> Vec<&'a str> {
    event
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| {
                    let tag = tag.as_array()?;
                    (tag.first()?.as_str()? == name).then(|| tag.get(1)?.as_str())?
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `POST /query` carries an ARRAY of filters, ORed the way a Nostr REQ is.
fn matches_any_filter(event: &Value, filters: &Value) -> bool {
    filters
        .as_array()
        .is_some_and(|filters| filters.iter().any(|filter| matches_filter(event, filter)))
}

fn matches_filter(event: &Value, filter: &Value) -> bool {
    let Some(object) = filter.as_object() else {
        return false;
    };
    for (key, wanted) in object {
        let Some(wanted) = wanted.as_array() else {
            // `limit` and friends: not a selector.
            continue;
        };
        let wanted: Vec<&str> = wanted.iter().filter_map(Value::as_str).collect();
        let matched = match key.as_str() {
            "ids" => event
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| wanted.contains(&id)),
            "authors" => event
                .get("pubkey")
                .and_then(Value::as_str)
                .is_some_and(|author| wanted.contains(&author)),
            "kinds" => {
                let kinds: Vec<u64> = object["kinds"]
                    .as_array()
                    .map(|kinds| kinds.iter().filter_map(Value::as_u64).collect())
                    .unwrap_or_default();
                event
                    .get("kind")
                    .and_then(Value::as_u64)
                    .is_some_and(|kind| kinds.contains(&kind))
            }
            name if name.starts_with('#') => tag_values(event, &name[1..])
                .into_iter()
                .any(|value| wanted.contains(&value)),
            _ => continue,
        };
        if !matched {
            return false;
        }
    }
    true
}

/// The relay's acceptance receipt for one authority transition — the fact the
/// CLI's projection trusts, and the only thing that makes a submitted 44228
/// count as accepted.
fn acceptance_receipt(transition: &Value, relay: &Keys) -> Value {
    let event: Event = serde_json::from_value(transition.clone()).expect("transition event");
    let payload =
        decode_coding_session_authority_transition(&event.content).expect("transition payload");
    let mut content = json!({
        "type": "coding_session_authority_transition_accepted",
        "genesisRef": payload.genesis_ref,
        "acceptedEventId": event.id.to_hex(),
        "seq": payload.seq,
        "transitionType": payload.transition_type,
        "granteePubkey": payload.grantee_pubkey,
    });
    if let (Some(object), Some(role)) = (content.as_object_mut(), payload.role) {
        object.insert("role".into(), Value::String(role));
    }
    let receipt = EventBuilder::new(
        Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        content.to_string(),
    )
    .tags([Tag::parse(["h", CHANNEL]).expect("h tag")])
    .sign_with_keys(relay)
    .expect("sign acceptance receipt");
    serde_json::to_value(receipt).expect("receipt JSON")
}

async fn serve(relay: Arc<RecordingRelay>) -> String {
    let app =
        Router::new()
            .route(
                "/",
                axum::routing::get(|State(relay): State<Arc<RecordingRelay>>| async move {
                    let body = json!({ "self": relay.keys.public_key().to_hex() }).to_string();
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .expect("info response")
                }),
            )
            .route(
                "/query",
                axum::routing::post(
                    |State(relay): State<Arc<RecordingRelay>>,
                     _headers: HeaderMap,
                     body: String| async move {
                        let filter: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                        let events = relay.events.lock().expect("events lock");
                        let matched: Vec<Value> = events
                            .iter()
                            .filter(|event| matches_any_filter(event, &filter))
                            .cloned()
                            .collect();
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(Value::Array(matched).to_string()))
                            .expect("query response")
                    },
                ),
            )
            .route(
                "/events",
                axum::routing::post(
                    |State(relay): State<Arc<RecordingRelay>>,
                     _headers: HeaderMap,
                     body: String| async move {
                        let event: Value =
                            serde_json::from_str(&body).expect("submitted event JSON");
                        relay
                            .submitted
                            .lock()
                            .expect("submitted lock")
                            .push(event.clone());
                        let kind = event
                            .get("kind")
                            .and_then(Value::as_u64)
                            .unwrap_or_default();
                        let id = event
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        {
                            let mut events = relay.events.lock().expect("events lock");
                            events.push(event.clone());
                            if kind == u64::from(KIND_CODING_SESSION_AUTHORITY_TRANSITION) {
                                events.push(acceptance_receipt(&event, &relay.keys));
                            }
                        }
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(json!({"accepted": true, "id": id}).to_string()))
                            .expect("write response")
                    },
                ),
            )
            .with_state(relay);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    format!("http://{addr}")
}

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "acp".into(),
        instance_id: PROVIDER_INSTANCE.into(),
        session_id: "seat-session".into(),
        generation: 1,
    }
}

/// Everything the wire held after the 2026-09-01 hire: a genesis, a seated
/// create, the provider's `created` receipt, and the provider's metadata —
/// and no `grant-seat` anywhere.
struct Wire {
    events: Vec<Value>,
    founder: Keys,
    provider: Keys,
    actor: String,
    genesis: String,
    create_id: String,
    receipt_id: String,
}

struct WireOptions {
    role: &'static str,
    receipt_command_id: &'static str,
    receipt_status: ReceiptStatus,
    receipt_signer: Option<Keys>,
    receipt_instance: &'static str,
    create_genesis: Option<String>,
    with_metadata: bool,
    metadata_role: &'static str,
}

impl Default for WireOptions {
    fn default() -> Self {
        Self {
            role: "builder",
            receipt_command_id: COMMAND_ID,
            receipt_status: ReceiptStatus::Created,
            receipt_signer: None,
            receipt_instance: PROVIDER_INSTANCE,
            create_genesis: None,
            with_metadata: true,
            metadata_role: "builder",
        }
    }
}

fn wire(options: WireOptions) -> Wire {
    let channel = Uuid::parse_str(CHANNEL).expect("channel UUID");
    let founder = Keys::generate();
    let provider = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();

    let genesis_event = build_coding_session_genesis(
        channel,
        &CodingSessionGenesisPayload::new(SESSION.to_owned()),
    )
    .expect("genesis builder")
    .sign_with_keys(&founder)
    .expect("sign genesis");
    let genesis = genesis_event.id.to_hex();

    let create_payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: COMMAND_ID.into(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(SESSION.into()),
            genesis_ref: Some(options.create_genesis.clone().unwrap_or(genesis.clone())),
            provider_instance_ref: PROVIDER_INSTANCE.try_into().expect("alias"),
            provider_authority_pubkey: provider.public_key().to_hex(),
            model: None,
            title: None,
            initial_turn: Some("Read the brief".into()),
            actor: Some(actor.clone()),
            role: Some(options.role.into()),
            hire_ref: None,
            routing: None,
        },
    };
    let create = build_coding_session_lifecycle_command(channel, &create_payload)
        .expect("create builder")
        .sign_with_keys(&founder)
        .expect("sign create");

    let receipt_target = CodingSessionTarget {
        instance_id: options.receipt_instance.into(),
        ..target()
    };
    let failed = !matches!(
        options.receipt_status,
        ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
    );
    let receipt_payload = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
        command_id: COMMAND_ID.into(),
        status: options.receipt_status,
        session: (!failed).then(|| receipt_target.clone()),
        error: failed.then(|| ReceiptError {
            code: "ACTOR_UNAVAILABLE".into(),
            message: "this computer holds no identity for that role".into(),
        }),
        turn_id: None,
    };
    let receipt_keys = options.receipt_signer.clone().unwrap_or(provider.clone());
    let receipt = build_coding_session_lifecycle_receipt(
        channel,
        options.receipt_command_id,
        &serde_json::to_string(&receipt_payload).expect("serialize receipt"),
    )
    .expect("receipt builder")
    .sign_with_keys(&receipt_keys)
    .expect("sign receipt");

    let mut events = vec![
        serde_json::to_value(&genesis_event).expect("genesis JSON"),
        serde_json::to_value(&create).expect("create JSON"),
        serde_json::to_value(&receipt).expect("receipt JSON"),
    ];
    if options.with_metadata {
        let metadata_payload = SessionMetadata {
            schema: METADATA_SCHEMA.into(),
            session: target(),
            project_ref: None,
            repo_ref: None,
            title: None,
            agent_ref: Some(actor.clone()),
            role: Some(options.metadata_role.into()),
            provider: Some(PROVIDER_INSTANCE.try_into().expect("alias")),
            runtime: Some("acp".try_into().expect("runtime")),
            model: None,
            status: SessionStatus::Idle,
            branch: None,
            capabilities: Capabilities::v1_claude(),
            session_ref: Some(SESSION.into()),
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
        let metadata = build_coding_session_metadata(
            channel,
            &target(),
            &serde_json::to_string(&metadata_payload).expect("serialize metadata"),
        )
        .expect("metadata builder")
        .sign_with_keys(&provider)
        .expect("sign metadata");
        events.push(serde_json::to_value(metadata).expect("metadata JSON"));
    }

    Wire {
        events,
        founder,
        provider,
        actor,
        genesis,
        create_id: create.id.to_hex(),
        receipt_id: receipt.id.to_hex(),
    }
}

/// Append an already-accepted `grant-seat` at the head of the chain, exactly
/// the way the relay would have if the hire had ever written one.
fn seed_seat_grant(wire: &mut Wire, relay: &Keys, actor: &str, role: &str) -> String {
    let channel = Uuid::parse_str(CHANNEL).expect("channel UUID");
    let payload = CodingSessionAuthorityTransitionPayload::new_grant_seat(
        &wire.genesis,
        None,
        1,
        actor,
        role,
    );
    let transition = build_coding_session_authority_transition(channel, &payload)
        .expect("transition builder")
        .sign_with_keys(&wire.founder)
        .expect("sign transition");
    let transition_value = serde_json::to_value(&transition).expect("transition JSON");
    wire.events
        .push(acceptance_receipt(&transition_value, relay));
    wire.events.push(transition_value);
    transition.id.to_hex()
}

struct Harness {
    relay: Arc<RecordingRelay>,
    client: BeekeeperClient,
    actor: String,
    genesis: String,
    create_id: String,
    receipt_id: String,
}

async fn harness(wire: Wire, relay_keys: Keys) -> Harness {
    let relay = Arc::new(RecordingRelay {
        events: Mutex::new(wire.events),
        submitted: Mutex::new(Vec::new()),
        keys: relay_keys,
    });
    let url = serve(relay.clone()).await;
    // The founder signs: the host that seated the role is the party whose
    // authority the chain accepts, and the CLI runs as it.
    let client = BeekeeperClient::new(url, wire.founder, None, None).expect("client");
    Harness {
        relay,
        client,
        actor: wire.actor,
        genesis: wire.genesis,
        create_id: wire.create_id,
        receipt_id: wire.receipt_id,
    }
}

async fn repair(harness: &Harness, genesis: Option<&str>) -> Result<(), CliError> {
    cmd_seat_repair(
        &harness.client,
        CHANNEL,
        SESSION,
        genesis,
        &harness.actor,
        &crate::OutputFormat::Json,
    )
    .await
}

fn grant_seat_payloads(relay: &RecordingRelay) -> Vec<CodingSessionAuthorityTransitionPayload> {
    relay
        .submitted()
        .iter()
        .filter(|event| {
            event.get("kind").and_then(Value::as_u64)
                == Some(u64::from(KIND_CODING_SESSION_AUTHORITY_TRANSITION))
        })
        .map(|event| {
            decode_coding_session_authority_transition(
                event.get("content").and_then(Value::as_str).unwrap_or(""),
            )
            .expect("submitted transition payload")
        })
        .collect()
}

/// Nothing this command does may ever put a hire on the wire. Asserted after
/// every case, because the failure mode being prevented — a repair that
/// "helpfully" re-hires and seats a second agent — is silent and expensive.
fn assert_never_hired(relay: &RecordingRelay) {
    assert!(
        !relay
            .submitted_kinds()
            .contains(&u64::from(KIND_CODING_SESSION_LIFECYCLE_COMMAND)),
        "seat-repair submitted a lifecycle command (44221): {:?}",
        relay.submitted()
    );
}

/// Acceptance #7: a create receipt that arrived after the CLI window closed
/// is still enough to grant the seat, exactly once.
#[tokio::test]
async fn seat_repair_grants_once_is_idempotent_and_never_hires() {
    let harness = harness(wire(WireOptions::default()), Keys::generate()).await;

    repair(&harness, Some(&harness.genesis))
        .await
        .expect("first repair grants");

    let payloads = grant_seat_payloads(&harness.relay);
    assert_eq!(payloads.len(), 1, "expected exactly one authority write");
    assert_eq!(
        payloads[0].transition_type,
        CodingSessionAuthorityTransitionType::GrantSeat
    );
    assert_eq!(payloads[0].grantee_pubkey, harness.actor);
    assert_eq!(payloads[0].role.as_deref(), Some("builder"));
    assert_eq!(harness.relay.submitted_kinds().len(), 1);
    assert_never_hired(&harness.relay);

    // Second run, against the chain the first one wrote: a no-op.
    repair(&harness, Some(&harness.genesis))
        .await
        .expect("second repair is a no-op");
    assert_eq!(
        harness.relay.submitted_kinds().len(),
        1,
        "the idempotent run wrote again: {:?}",
        harness.relay.submitted()
    );
    assert_never_hired(&harness.relay);
}

/// The genesis is resolved from the channel when `--genesis` is omitted,
/// exactly as `hire` resolves it.
#[tokio::test]
async fn seat_repair_resolves_the_genesis_from_the_channel() {
    let harness = harness(wire(WireOptions::default()), Keys::generate()).await;
    repair(&harness, None).await.expect("repair grants");
    assert_eq!(grant_seat_payloads(&harness.relay).len(), 1);
    assert_never_hired(&harness.relay);
}

/// C-T2: a seated create with no provider receipt is unfinished, not wrong.
#[tokio::test]
async fn no_provider_receipt_yet_writes_nothing_and_exits_five() {
    let mut wire = wire(WireOptions::default());
    // Drop the receipt: the create and the provider metadata remain.
    let receipt_id = wire.receipt_id.clone();
    wire.events
        .retain(|event| event.get("id").and_then(Value::as_str) != Some(receipt_id.as_str()));
    let harness = harness(wire, Keys::generate()).await;

    let error = repair(&harness, Some(&harness.genesis))
        .await
        .expect_err("no receipt is not a success");
    assert_eq!(crate::error::exit_code(&error), 5, "{error}");
    assert!(
        error
            .to_string()
            .contains("has a bound provider lifecycle receipt yet"),
        "{error}"
    );
    assert!(error.to_string().contains("[no bound receipt]"), "{error}");
    assert!(harness.relay.submitted().is_empty());
    assert_never_hired(&harness.relay);
}

/// C-T3 (acceptance #8): every way evidence can fail to prove this exact
/// execution ends with **no write**, and says which kind of failure it was.
///
/// Fix round 1 split what used to be one outcome in two, and the split is the
/// point. A receipt that is not *bound* to the create — forged signer, tags
/// naming another command, a target on another provider instance — is now
/// ignored rather than fatal (REVIEW-C attack 6: a later forgery must not be
/// able to deny the only recovery path), so the candidate simply has no bound
/// receipt: `no_receipt_yet`, exit 5, with the ignored receipts counted out
/// loud. A receipt that IS bound and still fails the full chain, or one the
/// provider used to refuse, is a real refusal: exit 1. Both write nothing.
#[tokio::test]
async fn forged_mismatched_and_unbacked_evidence_never_grants_a_seat() {
    // (name, options, expected exit code, expected substring)
    let cases: Vec<(&str, WireOptions, i32, &str)> = vec![
        (
            "receipt signed by a key that is not the create's provider authority",
            WireOptions {
                receipt_signer: Some(Keys::generate()),
                ..WireOptions::default()
            },
            5,
            "unbound receipt(s) ignored",
        ),
        (
            "receipt bound to another command id",
            WireOptions {
                receipt_command_id: "csl-someone-elses-create",
                ..WireOptions::default()
            },
            5,
            "unbound receipt(s) ignored",
        ),
        (
            "receipt naming another provider instance",
            WireOptions {
                receipt_instance: "provider-2",
                ..WireOptions::default()
            },
            // Binds (same signer, command and tags — the alias is not comparable to
            // the target's cryptographic instance id, see hire_evidence.rs), then
            // fails verification against the provider's own metadata: refused, exit 1.
            1,
            "did not verify",
        ),
        (
            "create naming a genesis this umbrella was not founded on",
            WireOptions {
                create_genesis: Some("ab".repeat(32)),
                ..WireOptions::default()
            },
            1,
            "did not verify",
        ),
        (
            "provider metadata absent",
            WireOptions {
                with_metadata: false,
                ..WireOptions::default()
            },
            1,
            "did not verify",
        ),
        (
            "provider metadata naming another role",
            WireOptions {
                metadata_role: "verifier",
                ..WireOptions::default()
            },
            1,
            "did not verify",
        ),
        (
            "provider refused the create",
            WireOptions {
                receipt_status: ReceiptStatus::Failed,
                ..WireOptions::default()
            },
            1,
            "ACTOR_UNAVAILABLE",
        ),
    ];

    for (name, options, expected_code, expected_text) in cases {
        let harness = harness(wire(options), Keys::generate()).await;
        let Err(error) = repair(&harness, Some(&harness.genesis)).await else {
            panic!("{name}: unproven evidence granted a seat");
        };
        assert_eq!(
            crate::error::exit_code(&error),
            expected_code,
            "{name}: {error}"
        );
        assert!(error.to_string().contains(expected_text), "{name}: {error}");
        assert!(
            harness.relay.submitted().is_empty(),
            "{name}: wrote {:?}",
            harness.relay.submitted()
        );
        assert_never_hired(&harness.relay);
    }
}

/// C-T4: an actor already seated in a different role is never overwritten.
#[tokio::test]
async fn an_actor_holding_another_role_is_refused_without_a_write() {
    let relay_keys = Keys::generate();
    let mut wire = wire(WireOptions::default());
    let actor = wire.actor.clone();
    seed_seat_grant(&mut wire, &relay_keys, &actor, "verifier");
    let harness = harness(wire, relay_keys).await;

    let error = repair(&harness, Some(&harness.genesis))
        .await
        .expect_err("a role change is not a repair");
    assert_eq!(crate::error::exit_code(&error), 1, "{error}");
    assert!(error.to_string().contains("already holds role"), "{error}");
    assert!(harness.relay.submitted().is_empty());
    assert_never_hired(&harness.relay);
}

/// The output document is the same key set for every outcome, so a script can
/// tell an absent fact from an unreported one.
#[tokio::test]
async fn the_repair_document_reports_the_create_and_receipt_it_acted_on() {
    let harness = harness(wire(WireOptions::default()), Keys::generate()).await;
    assert_eq!(harness.create_id.len(), 64);
    assert_eq!(harness.receipt_id.len(), 64);
    repair(&harness, Some(&harness.genesis))
        .await
        .expect("repair grants");
    let submitted = harness.relay.submitted();
    assert_eq!(submitted.len(), 1);
    assert_eq!(
        submitted[0].get("kind").and_then(Value::as_u64),
        Some(u64::from(KIND_CODING_SESSION_AUTHORITY_TRANSITION))
    );
}

/// C-T7: the exit-code mapping, stated once and pinned.
#[test]
fn seat_repair_exit_codes_are_stable() {
    assert_eq!(seat_repair_exit_code(SeatRepairOutcome::Granted), 0);
    assert_eq!(seat_repair_exit_code(SeatRepairOutcome::AlreadyGranted), 0);
    assert_eq!(seat_repair_exit_code(SeatRepairOutcome::Refused), 1);
    assert_eq!(seat_repair_exit_code(SeatRepairOutcome::Ambiguous), 1);
    assert_eq!(seat_repair_exit_code(SeatRepairOutcome::NoReceiptYet), 5);
    assert_eq!(SeatRepairOutcome::Granted.as_str(), "granted");
    assert_eq!(
        SeatRepairOutcome::AlreadyGranted.as_str(),
        "already_granted"
    );
    assert_eq!(SeatRepairOutcome::NoReceiptYet.as_str(), "no_receipt_yet");
    assert_eq!(SeatRepairOutcome::Refused.as_str(), "refused");
    assert_eq!(SeatRepairOutcome::Ambiguous.as_str(), "ambiguous");
}

/// The printed document, for every outcome and both formats.
///
/// Asserted here rather than by scraping stdout, and asserted for the outcomes
/// that have no create or receipt to name as well as the ones that do: `null`
/// under `createEventId` is a fact, and a missing key is not.
#[test]
fn every_outcome_prints_the_same_keys_with_nulls_where_a_fact_is_absent() {
    let wire = wire(WireOptions::default());

    for outcome in [
        SeatRepairOutcome::Granted,
        SeatRepairOutcome::AlreadyGranted,
        SeatRepairOutcome::Ambiguous,
        SeatRepairOutcome::NoReceiptYet,
        SeatRepairOutcome::Refused,
    ] {
        let full = seat_repair_document(
            outcome,
            &wire.actor,
            None,
            None,
            None,
            None,
            "nothing to report",
            &crate::OutputFormat::Json,
        );
        let mut keys = full
            .as_object()
            .expect("document object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "actor",
                "createEventId",
                "detail",
                "outcome",
                "receiptEventId",
                "role",
                "seatGrantEventId",
            ],
            "{outcome:?}"
        );
        assert_eq!(full["outcome"], outcome.as_str());
        assert_eq!(full["actor"], wire.actor);
        assert_eq!(full["role"], Value::Null);
        assert_eq!(full["createEventId"], Value::Null);
        assert_eq!(full["receiptEventId"], Value::Null);
        assert_eq!(full["seatGrantEventId"], Value::Null);

        let compact = seat_repair_document(
            outcome,
            &wire.actor,
            Some("builder"),
            Some(&wire.create_id),
            Some(&wire.receipt_id),
            Some("ab"),
            "nothing to report",
            &crate::OutputFormat::Compact,
        );
        let mut keys = compact
            .as_object()
            .expect("compact object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["actor", "outcome", "role", "seatGrantEventId"],
            "{outcome:?}"
        );
    }

    let named = seat_repair_document(
        SeatRepairOutcome::Granted,
        &wire.actor,
        Some("builder"),
        Some(&wire.create_id),
        Some(&wire.receipt_id),
        Some("cd"),
        "granted",
        &crate::OutputFormat::Json,
    );
    assert_eq!(named["role"], "builder");
    assert_eq!(named["createEventId"], wire.create_id);
    assert_eq!(named["receiptEventId"], wire.receipt_id);
    assert_eq!(named["seatGrantEventId"], "cd");
    assert_eq!(named["detail"], "granted");
}

// ── Fix round 1 (REVIEW-C F1): evidence-driven selection ─────────────────────
//
// The reviewer's attack 5 needed no attacker at all: a benign earlier hire the
// provider never answered shadowed the seat that was actually running, because
// selection took the earliest self-asserted `created_at`. These build the
// multi-create channels that case requires.

/// One extra seated create appended to a channel, with whatever evidence the
/// scenario wants behind it.
struct ExtraSeat {
    command_id: &'static str,
    session_id: &'static str,
    role: &'static str,
    /// Signed by the founder unless a scenario is testing a stranger's create.
    signer: Option<Keys>,
    receipt: Option<ReceiptStatus>,
    /// Signed by the provider unless a scenario is testing a forged receipt.
    receipt_signer: Option<Keys>,
    with_metadata: bool,
}

impl Default for ExtraSeat {
    fn default() -> Self {
        Self {
            command_id: "csl-extra",
            session_id: "extra-session",
            role: "builder",
            signer: None,
            receipt: None,
            receipt_signer: None,
            with_metadata: true,
        }
    }
}

/// Append a second (third, …) seated create for the SAME actor and umbrella.
fn add_seat(wire: &mut Wire, extra: ExtraSeat) -> String {
    let channel = Uuid::parse_str(CHANNEL).expect("channel UUID");
    let signer = extra.signer.clone().unwrap_or(wire.founder.clone());
    let create_payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: extra.command_id.into(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(SESSION.into()),
            genesis_ref: Some(wire.genesis.clone()),
            provider_instance_ref: PROVIDER_INSTANCE.try_into().expect("alias"),
            provider_authority_pubkey: wire.provider.public_key().to_hex(),
            model: None,
            title: None,
            initial_turn: Some("Read the brief".into()),
            actor: Some(wire.actor.clone()),
            role: Some(extra.role.into()),
            hire_ref: None,
            routing: None,
        },
    };
    let create = build_coding_session_lifecycle_command(channel, &create_payload)
        .expect("extra create builder")
        .sign_with_keys(&signer)
        .expect("sign extra create");
    let create_id = create.id.to_hex();
    wire.events
        .push(serde_json::to_value(create).expect("extra create JSON"));

    let extra_target = CodingSessionTarget {
        session_id: extra.session_id.into(),
        ..target()
    };
    if let Some(status) = extra.receipt {
        let failed = !matches!(
            status,
            ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
        );
        let receipt_payload = LifecycleReceipt {
            schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
            command_id: extra.command_id.into(),
            status,
            session: (!failed).then(|| extra_target.clone()),
            error: failed.then(|| ReceiptError {
                code: "ACTOR_UNAVAILABLE".into(),
                message: "this computer holds no identity for that role".into(),
            }),
            turn_id: None,
        };
        let keys = extra
            .receipt_signer
            .clone()
            .unwrap_or(wire.provider.clone());
        let receipt = build_coding_session_lifecycle_receipt(
            channel,
            extra.command_id,
            &serde_json::to_string(&receipt_payload).expect("serialize extra receipt"),
        )
        .expect("extra receipt builder")
        .sign_with_keys(&keys)
        .expect("sign extra receipt");
        wire.events
            .push(serde_json::to_value(receipt).expect("extra receipt JSON"));
    }
    if extra.with_metadata && extra.receipt.is_some() {
        let metadata_payload = SessionMetadata {
            schema: METADATA_SCHEMA.into(),
            session: extra_target.clone(),
            project_ref: None,
            repo_ref: None,
            title: None,
            agent_ref: Some(wire.actor.clone()),
            role: Some(extra.role.into()),
            provider: Some(PROVIDER_INSTANCE.try_into().expect("alias")),
            runtime: Some("acp".try_into().expect("runtime")),
            model: None,
            status: SessionStatus::Idle,
            branch: None,
            capabilities: Capabilities::v1_claude(),
            session_ref: Some(SESSION.into()),
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
        let metadata = build_coding_session_metadata(
            channel,
            &extra_target,
            &serde_json::to_string(&metadata_payload).expect("serialize extra metadata"),
        )
        .expect("extra metadata builder")
        .sign_with_keys(&wire.provider)
        .expect("sign extra metadata");
        wire.events
            .push(serde_json::to_value(metadata).expect("extra metadata JSON"));
    }
    create_id
}

/// **REVIEW-C attack 5, no attacker.** An earlier founder-signed hire the
/// provider never answered must not shadow the seat that is actually running.
///
/// Before the fix this returned `no_receipt_yet` exit 5 naming the shadow
/// create, leaving the live seat permanently unrepairable and telling the
/// operator to wait for a receipt that already existed.
#[tokio::test]
async fn a_benign_unanswered_earlier_hire_never_shadows_the_running_seat() {
    let mut wire = wire(WireOptions::default());
    // The unanswered hire. No receipt at all — exactly the `seating` outcome.
    add_seat(
        &mut wire,
        ExtraSeat {
            command_id: "csl-first-hire",
            session_id: "abandoned-session",
            receipt: None,
            ..ExtraSeat::default()
        },
    );
    let harness = harness(wire, Keys::generate()).await;

    repair(&harness, Some(&harness.genesis))
        .await
        .expect("the running seat is repaired despite the shadow create");

    let payloads = grant_seat_payloads(&harness.relay);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].grantee_pubkey, harness.actor);
    assert_eq!(payloads[0].role.as_deref(), Some("builder"));
    assert_never_hired(&harness.relay);
}

/// **REVIEW-C attack 5, with an attacker.** Any channel member can publish a
/// 44221; none of them may park the only recovery path.
#[tokio::test]
async fn a_stranger_signed_create_is_not_even_a_candidate() {
    let mut wire = wire(WireOptions::default());
    add_seat(
        &mut wire,
        ExtraSeat {
            command_id: "csl-shadow-create",
            session_id: "shadow-session",
            signer: Some(Keys::generate()),
            receipt: None,
            ..ExtraSeat::default()
        },
    );
    let harness = harness(wire, Keys::generate()).await;

    repair(&harness, Some(&harness.genesis))
        .await
        .expect("a stranger's create cannot deny the repair");
    assert_eq!(grant_seat_payloads(&harness.relay).len(), 1);
    assert_never_hired(&harness.relay);
}

/// **REVIEW-C attack 6.** A later forged receipt carrying the real commandId
/// must be invisible, not fatal. Selection filters by binding before it looks
/// at anything else, so the genuine receipt still wins.
#[tokio::test]
async fn a_forged_later_receipt_cannot_deny_the_repair() {
    let mut wire = wire(WireOptions::default());
    let channel = Uuid::parse_str(CHANNEL).expect("channel UUID");
    // Same commandId, same content, signed by somebody who is not the
    // provider authority, and claiming a far-future created_at.
    let forged_payload = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
        command_id: COMMAND_ID.into(),
        status: ReceiptStatus::Created,
        session: Some(target()),
        error: None,
        turn_id: None,
    };
    let forged = build_coding_session_lifecycle_receipt(
        channel,
        COMMAND_ID,
        &serde_json::to_string(&forged_payload).expect("serialize forged receipt"),
    )
    .expect("forged receipt builder")
    .custom_created_at(nostr::Timestamp::from_secs(4_102_444_800))
    .sign_with_keys(&Keys::generate())
    .expect("sign forged receipt");
    wire.events
        .push(serde_json::to_value(forged).expect("forged receipt JSON"));
    let harness = harness(wire, Keys::generate()).await;

    repair(&harness, Some(&harness.genesis))
        .await
        .expect("a forged receipt is ignored, not fatal");
    assert_eq!(grant_seat_payloads(&harness.relay).len(), 1);
    assert_never_hired(&harness.relay);
}

/// **REVIEW-C2 G1.** Two verifying creates that imply the IDENTICAL write are
/// the same answer arriving twice, not a contradiction.
///
/// A grant writes `(actor, role)` and nothing else, and every candidate is
/// already filtered to one actor — so two builder creates disagree about
/// nothing. Refusing them, as fix round 1 did, broke the repair on any umbrella
/// that had ever hired the same actor twice.
#[tokio::test]
async fn two_verifying_creates_implying_the_same_write_grant_once() {
    let mut wire = wire(WireOptions::default());
    add_seat(
        &mut wire,
        ExtraSeat {
            command_id: "csl-second-live",
            session_id: "second-live-session",
            receipt: Some(ReceiptStatus::Created),
            ..ExtraSeat::default()
        },
    );
    let harness = harness(wire, Keys::generate()).await;

    repair(&harness, Some(&harness.genesis))
        .await
        .expect("identical writes are not a contradiction");

    let payloads = grant_seat_payloads(&harness.relay);
    assert_eq!(payloads.len(), 1, "exactly one grant, not two");
    assert_eq!(payloads[0].grantee_pubkey, harness.actor);
    assert_eq!(payloads[0].role.as_deref(), Some("builder"));
    assert_never_hired(&harness.relay);
}

/// **REVIEW-C2 G2.** The idempotent second run — REPORT-C §7 calls it "the real
/// acceptance" — must survive an umbrella that produced two creates.
#[tokio::test]
async fn a_healthy_seat_with_two_identical_candidates_is_already_granted() {
    let relay_keys = Keys::generate();
    let mut wire = wire(WireOptions::default());
    add_seat(
        &mut wire,
        ExtraSeat {
            command_id: "csl-second-live",
            session_id: "second-live-session",
            receipt: Some(ReceiptStatus::Created),
            ..ExtraSeat::default()
        },
    );
    let actor = wire.actor.clone();
    seed_seat_grant(&mut wire, &relay_keys, &actor, "builder");
    let harness = harness(wire, relay_keys).await;

    // Exit 0, not the fix-round-1 `ambiguous` exit 1.
    repair(&harness, Some(&harness.genesis))
        .await
        .expect("an already-granted seat is not ambiguous");
    assert!(
        harness.relay.submitted().is_empty(),
        "already_granted must write nothing: {:?}",
        harness.relay.submitted()
    );
    assert_never_hired(&harness.relay);
}

/// **REVIEW-C2 G1/G3.** Only a real disagreement about the ROLE is ambiguous,
/// and the message names each disputed role with the commandIds claiming it.
#[tokio::test]
async fn two_verifying_creates_with_different_roles_are_ambiguous() {
    let mut wire = wire(WireOptions::default());
    add_seat(
        &mut wire,
        ExtraSeat {
            command_id: "csl-verifier-seat",
            session_id: "verifier-session",
            role: "verifier",
            receipt: Some(ReceiptStatus::Created),
            ..ExtraSeat::default()
        },
    );
    let harness = harness(wire, Keys::generate()).await;

    let error = repair(&harness, Some(&harness.genesis))
        .await
        .expect_err("two roles is a decision only the founder can make");
    assert_eq!(crate::error::exit_code(&error), 1, "{error}");
    let text = error.to_string();
    // Both roles, each with the commandId claiming it.
    assert!(text.contains("builder ("), "{text}");
    assert!(text.contains("verifier ("), "{text}");
    assert!(text.contains(COMMAND_ID), "{text}");
    assert!(text.contains("csl-verifier-seat"), "{text}");
    // The remedy must not point at something that cannot help.
    assert!(
        !text.contains("Revoke or end the seats"),
        "the non-convergent remedy is back: {text}"
    );
    assert!(text.contains("already_granted"), "{text}");
    assert!(harness.relay.submitted().is_empty());
    assert_never_hired(&harness.relay);
}

/// **REVIEW-C2 G3 — the remedy must actually converge.**
///
/// The `ambiguous` message tells the founder to grant the role they mean and
/// re-run. This asserts that sentence is true: with the same two disagreeing
/// creates on the wire, an accepted `grant-seat` for one of the roles turns the
/// refusal into `already_granted` exit 0 with no write. Without this the
/// remedy would be the same non-convergent class of copy bug the whole lane
/// exists to fix.
#[tokio::test]
async fn granting_the_intended_role_converges_an_ambiguous_repair() {
    let relay_keys = Keys::generate();
    let mut wire = wire(WireOptions::default());
    add_seat(
        &mut wire,
        ExtraSeat {
            command_id: "csl-verifier-seat",
            session_id: "verifier-session",
            role: "verifier",
            receipt: Some(ReceiptStatus::Created),
            ..ExtraSeat::default()
        },
    );
    // The founder settles it on the accepted authority chain.
    let actor = wire.actor.clone();
    seed_seat_grant(&mut wire, &relay_keys, &actor, "verifier");
    let harness = harness(wire, relay_keys).await;

    repair(&harness, Some(&harness.genesis))
        .await
        .expect("the founder's recorded decision settles the disagreement");
    assert!(
        harness.relay.submitted().is_empty(),
        "converging must write nothing: {:?}",
        harness.relay.submitted()
    );
    assert_never_hired(&harness.relay);
}

/// **REVIEW-C2, fourth shape.** One commandId published twice — a retried
/// submit under a new event id — is one candidate, listed once.
#[tokio::test]
async fn one_command_id_published_twice_is_one_candidate() {
    let mut wire = wire(WireOptions::default());
    // Same commandId and same role as the original create, republished.
    add_seat(
        &mut wire,
        ExtraSeat {
            command_id: COMMAND_ID,
            session_id: "republished-session",
            receipt: None,
            ..ExtraSeat::default()
        },
    );
    let candidates = super::super::crew::founder_seated_creates_for_actor(
        &wire.events,
        SESSION,
        &wire.actor,
        &wire.founder.public_key().to_hex(),
    );
    assert_eq!(
        candidates.len(),
        1,
        "a retried submit is one logical command, got {:?}",
        candidates
            .iter()
            .map(|seat| seat.command_id.as_str())
            .collect::<Vec<_>>()
    );

    let harness = harness(wire, Keys::generate()).await;
    repair(&harness, Some(&harness.genesis))
        .await
        .expect("a republished create still repairs");
    assert_eq!(grant_seat_payloads(&harness.relay).len(), 1);
    assert_never_hired(&harness.relay);
}

/// A provider that REFUSED the create and a provider that has not answered are
/// different facts, and the operator sees every candidate either way.
#[tokio::test]
async fn unverified_outcomes_name_every_candidate_not_just_one() {
    // No receipt anywhere: `no_receipt_yet`, both commandIds listed.
    let mut pending = wire(WireOptions::default());
    let receipt_id = pending.receipt_id.clone();
    pending
        .events
        .retain(|event| event.get("id").and_then(Value::as_str) != Some(receipt_id.as_str()));
    add_seat(
        &mut pending,
        ExtraSeat {
            command_id: "csl-first-hire",
            receipt: None,
            ..ExtraSeat::default()
        },
    );
    let pending_harness = harness(pending, Keys::generate()).await;
    let error = repair(&pending_harness, Some(&pending_harness.genesis))
        .await
        .expect_err("no receipt is not a success");
    assert_eq!(crate::error::exit_code(&error), 5, "{error}");
    assert!(error.to_string().contains(COMMAND_ID), "{error}");
    assert!(error.to_string().contains("csl-first-hire"), "{error}");
    assert!(error.to_string().contains("no bound receipt"), "{error}");
    assert!(pending_harness.relay.submitted().is_empty());
    assert_never_hired(&pending_harness.relay);

    // A provider refusal names itself, and still lists the rest.
    let mut refused = wire(WireOptions {
        receipt_status: ReceiptStatus::Failed,
        ..WireOptions::default()
    });
    add_seat(
        &mut refused,
        ExtraSeat {
            command_id: "csl-first-hire",
            receipt: None,
            ..ExtraSeat::default()
        },
    );
    let harness = harness(refused, Keys::generate()).await;
    let error = repair(&harness, Some(&harness.genesis))
        .await
        .expect_err("a refused create is not repairable");
    assert_eq!(crate::error::exit_code(&error), 1, "{error}");
    assert!(error.to_string().contains("ACTOR_UNAVAILABLE"), "{error}");
    assert!(error.to_string().contains("csl-first-hire"), "{error}");
    assert!(harness.relay.submitted().is_empty());
    assert_never_hired(&harness.relay);
}

/// An actor nothing seated used to print no document at all (REVIEW-C F5).
#[tokio::test]
async fn an_actor_with_no_seated_create_still_prints_a_document() {
    let harness = harness(wire(WireOptions::default()), Keys::generate()).await;
    let stranger = Keys::generate().public_key().to_hex();
    let document = seat_repair_document(
        SeatRepairOutcome::Refused,
        &stranger,
        None,
        None,
        None,
        None,
        "no founder-signed seated create …",
        &crate::OutputFormat::Json,
    );
    assert_eq!(document["outcome"], "refused");
    assert_eq!(document["role"], Value::Null);
    assert_eq!(document["createEventId"], Value::Null);

    let error = cmd_seat_repair(
        &harness.client,
        CHANNEL,
        SESSION,
        Some(&harness.genesis),
        &stranger,
        &crate::OutputFormat::Json,
    )
    .await
    .expect_err("an unseated actor is refused");
    assert_eq!(crate::error::exit_code(&error), 1, "{error}");
    assert!(
        error.to_string().contains("there is no seat to repair"),
        "{error}"
    );
    assert!(harness.relay.submitted().is_empty());
}

/// Selection must not depend on the order the relay hands the events over.
#[tokio::test]
async fn selection_is_independent_of_relay_event_order() {
    for reversed in [false, true] {
        let mut wire = wire(WireOptions::default());
        add_seat(
            &mut wire,
            ExtraSeat {
                command_id: "csl-first-hire",
                session_id: "abandoned-session",
                receipt: None,
                ..ExtraSeat::default()
            },
        );
        add_seat(
            &mut wire,
            ExtraSeat {
                command_id: "csl-shadow-create",
                session_id: "shadow-session",
                signer: Some(Keys::generate()),
                receipt: None,
                ..ExtraSeat::default()
            },
        );
        if reversed {
            wire.events.reverse();
        }
        let harness = harness(wire, Keys::generate()).await;
        repair(&harness, Some(&harness.genesis))
            .await
            .unwrap_or_else(|error| panic!("reversed={reversed}: {error}"));
        let payloads = grant_seat_payloads(&harness.relay);
        assert_eq!(payloads.len(), 1, "reversed={reversed}");
        assert_eq!(payloads[0].role.as_deref(), Some("builder"));
        assert_never_hired(&harness.relay);
    }
}
