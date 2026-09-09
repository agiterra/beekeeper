//! Unit tests for the **authority** wire: the accepted 44228 chain, its relay
//! receipts, and the race a claim can lose at the head.
//!
//! The relay-facing paths are exercised the way `operations_tests.rs`
//! exercises the authority chain: by handing the pure projection, folding and
//! envelope functions the signed events a relay would have served, so what is
//! tested is the rule rather than the transport. A child of `handover`
//! alongside `handover_tests.rs`, split only to keep each file under 1,000
//! lines.

use buzz_core::coding_session_authority_claim::ClaimState;
use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionPayload,
    CodingSessionAuthorityTransitionType, CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use buzz_core::coding_session_payload::ReceiptStatus;
use buzz_core::kind::{KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_SYSTEM_MESSAGE};
use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::json;

use super::super::operations_authority::{
    project_receipt_backed_authority_chain, AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
};
use super::*;

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "1f2e3d4c-5b6a-4798-8765-43210fedcba9";
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

fn hex64(byte: &str) -> String {
    byte.repeat(32)
}

// ── the claim fold, through the receipt-backed projection ────────────────

fn transition_event(
    signer: &Keys,
    payload: &CodingSessionAuthorityTransitionPayload,
) -> nostr::Event {
    let tags = [
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION]).expect("v"),
        Tag::parse(["csat-genesis", payload.genesis_ref.as_str()]).expect("genesis"),
    ];
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
        serde_json::to_string(payload).expect("payload"),
    )
    .tags(tags)
    .sign_with_keys(signer)
    .expect("sign")
}

fn receipt_event(transition: &nostr::Event, relay: &Keys) -> nostr::Event {
    let payload = decode_coding_session_authority_transition(&transition.content).expect("payload");
    let mut content = json!({
        "type": AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
        "genesisRef": payload.genesis_ref,
        "acceptedEventId": transition.id.to_hex(),
        "seq": payload.seq,
        "transitionType": payload.transition_type,
        "granteePubkey": payload.grantee_pubkey,
    });
    if let (Some(object), Some(role)) = (content.as_object_mut(), payload.role) {
        object.insert("role".into(), serde_json::Value::String(role));
    }
    // The relay stamps the body onto claim receipts
    // (`buzz-relay/src/handlers/side_effects.rs`), so the fixture must too —
    // a receipt fixture that omitted it would test a wire nobody serves.
    if let (Some(object), Some(body_pubkey)) = (content.as_object_mut(), payload.body_pubkey) {
        object.insert("bodyPubkey".into(), serde_json::Value::String(body_pubkey));
    }
    EventBuilder::new(
        Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        content.to_string(),
    )
    .tags([Tag::parse(["h", CHANNEL]).expect("h")])
    .sign_with_keys(relay)
    .expect("sign")
}

/// Build a chain: founder grants `operator`, operator takes over, then the
/// caller's choice of extra links.
fn chain(
    founder: &Keys,
    operator: &Keys,
    relay: &Keys,
    body: &str,
    extra: &[CodingSessionAuthorityTransitionType],
) -> (Vec<nostr::Event>, Vec<nostr::Event>) {
    chain_on(founder, operator, relay, body, GENESIS, extra)
}

/// [`chain`] against a genesis the caller chose, for the tests that need the
/// genesis event itself to exist and hash to the same id.
fn chain_on(
    founder: &Keys,
    operator: &Keys,
    relay: &Keys,
    body: &str,
    genesis: &str,
    extra: &[CodingSessionAuthorityTransitionType],
) -> (Vec<nostr::Event>, Vec<nostr::Event>) {
    let mut transitions = Vec::new();
    let mut receipts = Vec::new();
    let mut prev: Option<String> = None;
    let mut seq = 1;

    let grant = transition_event(
        founder,
        &CodingSessionAuthorityTransitionPayload::new_grant_operator(
            genesis,
            prev.clone(),
            seq,
            operator.public_key().to_hex(),
        ),
    );
    prev = Some(grant.id.to_hex());
    seq += 1;
    receipts.push(receipt_event(&grant, relay));
    transitions.push(grant);

    let takeover = transition_event(
        operator,
        &CodingSessionAuthorityTransitionPayload::new_takeover(
            genesis,
            prev.clone(),
            seq,
            operator.public_key().to_hex(),
            body.to_owned(),
        ),
    );
    prev = Some(takeover.id.to_hex());
    seq += 1;
    receipts.push(receipt_event(&takeover, relay));
    transitions.push(takeover);

    for transition_type in extra {
        let payload = match transition_type {
            CodingSessionAuthorityTransitionType::Revoke => {
                CodingSessionAuthorityTransitionPayload::new(
                    CodingSessionAuthorityTransitionType::Revoke,
                    genesis,
                    prev.clone(),
                    seq,
                    operator.public_key().to_hex(),
                )
            }
            CodingSessionAuthorityTransitionType::GrantOperator => {
                CodingSessionAuthorityTransitionPayload::new_grant_operator(
                    genesis,
                    prev.clone(),
                    seq,
                    operator.public_key().to_hex(),
                )
            }
            other => panic!("chain helper does not build {other:?}"),
        };
        let event = transition_event(founder, &payload);
        prev = Some(event.id.to_hex());
        seq += 1;
        receipts.push(receipt_event(&event, relay));
        transitions.push(event);
    }
    (transitions, receipts)
}

#[test]
fn an_accepted_takeover_folds_to_an_active_claim() {
    let founder = Keys::generate();
    let operator = Keys::generate();
    let relay = Keys::generate();
    let body = hex64("bb");
    let (transitions, receipts) = chain(&founder, &operator, &relay, &body, &[]);

    let projected = project_receipt_backed_authority_chain(
        &transitions,
        &receipts,
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect("project");

    match projected.claim {
        ClaimState::Active(claim) => {
            assert_eq!(claim.claimant, operator.public_key().to_hex());
            assert_eq!(claim.body_pubkey, body);
            assert_eq!(claim.seq, 2);
        }
        other => panic!("expected an active claim, got {other:?}"),
    }
    assert!(
        projected.claim_since.is_some(),
        "the accepting receipt's time is what a surface renders 'since' from"
    );
}

#[test]
fn revoking_the_claimant_voids_the_claim_and_a_regrant_does_not_restore_it() {
    let founder = Keys::generate();
    let operator = Keys::generate();
    let relay = Keys::generate();
    let body = hex64("bb");
    let (transitions, receipts) = chain(
        &founder,
        &operator,
        &relay,
        &body,
        &[
            CodingSessionAuthorityTransitionType::Revoke,
            CodingSessionAuthorityTransitionType::GrantOperator,
        ],
    );

    let projected = project_receipt_backed_authority_chain(
        &transitions,
        &receipts,
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect("project");

    match projected.claim {
        ClaimState::Voided { ref last, .. } => {
            assert_eq!(last.claimant, operator.public_key().to_hex());
        }
        other => panic!("a regrant must not resurrect a claim; got {other:?}"),
    }
    assert!(
        projected.claim.active().is_none(),
        "voided is not a claim in force"
    );
    assert_eq!(
        projected.claim_since, None,
        "there is no claim in force, so there is no 'since'"
    );
}

#[test]
fn a_takeover_claiming_the_session_for_somebody_else_is_refused() {
    let founder = Keys::generate();
    let relay = Keys::generate();
    let stranger = hex64("cc");
    let takeover = transition_event(
        &founder,
        &CodingSessionAuthorityTransitionPayload::new_takeover(
            GENESIS,
            None,
            1,
            stranger,
            hex64("bb"),
        ),
    );
    let receipt = receipt_event(&takeover, &relay);
    let error = project_receipt_backed_authority_chain(
        &[takeover],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect_err("must refuse");
    assert!(
        error.contains("claimant is not its signer"),
        "a takeover is a self-claim: {error}"
    );
}

#[test]
fn a_takeover_by_a_stranger_is_refused() {
    let founder = Keys::generate();
    let stranger = Keys::generate();
    let relay = Keys::generate();
    let takeover = transition_event(
        &stranger,
        &CodingSessionAuthorityTransitionPayload::new_takeover(
            GENESIS,
            None,
            1,
            stranger.public_key().to_hex(),
            hex64("bb"),
        ),
    );
    let receipt = receipt_event(&takeover, &relay);
    let error = project_receipt_backed_authority_chain(
        &[takeover],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect_err("must refuse");
    assert!(error.contains("unauthorized signer"), "got {error}");
}

// ── receipt readers ──────────────────────────────────────────────────────

#[test]
fn a_claim_receipt_is_only_believed_when_the_trusted_relay_signed_it() {
    let relay = Keys::generate();
    let impostor = Keys::generate();
    let accepted = hex64("aa");
    let content = json!({
        "type": AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
        "genesisRef": GENESIS,
        "acceptedEventId": accepted,
        "seq": 2,
        "transitionType": "takeover",
        "granteePubkey": hex64("0b"),
    })
    .to_string();
    let build = |signer: &Keys| {
        serde_json::to_value(
            EventBuilder::new(Kind::Custom(KIND_SYSTEM_MESSAGE as u16), content.clone())
                .tags([Tag::parse(["h", CHANNEL]).expect("h")])
                .sign_with_keys(signer)
                .expect("sign"),
        )
        .expect("value")
    };

    let relay_hex = relay.public_key().to_hex();
    assert!(
        super::super::handover_claim::find_claim_receipt(
            &[build(&impostor)],
            &accepted,
            &relay_hex
        )
        .is_none(),
        "a receipt signed by anybody else is not a receipt"
    );
    assert!(super::super::handover_claim::find_claim_receipt(
        &[build(&relay)],
        &accepted,
        &relay_hex
    )
    .is_some());
}

/// One kind-44224 lifecycle receipt, in the exact shape the core decoder
/// insists on: `turnId` present exactly for `turn_started`, an error object
/// exactly where the status requires one, and a target everywhere but a
/// failed create.
fn lifecycle_receipt(command_id: &str, status: &str, with_target: bool) -> serde_json::Value {
    let mut content = json!({
        "schema": "buzz-coding-session-lifecycle-receipt/v1",
        "commandId": command_id,
        "status": status,
        "session": serde_json::Value::Null,
        "error": serde_json::Value::Null,
    });
    if with_target {
        content["session"] = json!({
            "driver": "claude-agent-acp",
            "instanceId": "inst",
            "sessionId": "sess",
            "generation": 1,
        });
    }
    if status == "turn_started" {
        content["turnId"] = json!("turn-1");
    }
    if status == "failed" {
        content["error"] = json!({
            "code": "PROVIDER_UNAVAILABLE",
            "message": "no provider answered",
        });
    }
    let signer = Keys::generate();
    serde_json::to_value(
        EventBuilder::new(Kind::Custom(44224), content.to_string())
            .sign_with_keys(&signer)
            .expect("sign"),
    )
    .expect("value")
}

#[test]
fn turn_started_wins_over_turn_queued_in_the_same_second() {
    let events = vec![
        lifecycle_receipt("cmd-1", "turn_queued", true),
        lifecycle_receipt("cmd-1", "turn_started", true),
    ];
    let (status, _) =
        super::super::handover_continue::newest_stage_for(&events, "cmd-1").expect("stage");
    assert_eq!(status, ReceiptStatus::TurnStarted);
}

#[test]
fn a_receipt_for_another_command_answers_nothing() {
    let events = vec![lifecycle_receipt("cmd-2", "turn_started", true)];
    assert!(super::super::handover_continue::newest_stage_for(&events, "cmd-1").is_none());
}

#[test]
fn a_create_answer_is_read_only_from_a_terminal_create_status() {
    let queued = vec![lifecycle_receipt("cmd-1", "turn_queued", true)];
    assert!(
        super::super::handover_reconstruct::create_answer(&queued, "cmd-1").is_none(),
        "a turn stage is not a create outcome"
    );
    let created = vec![lifecycle_receipt("cmd-1", "created", true)];
    let (status, target) =
        super::super::handover_reconstruct::create_answer(&created, "cmd-1").expect("answer");
    assert_eq!(status, ReceiptStatus::Created);
    assert_eq!(target.expect("target").generation, 1);

    let failed = vec![lifecycle_receipt("cmd-1", "failed", false)];
    let (status, target) =
        super::super::handover_reconstruct::create_answer(&failed, "cmd-1").expect("answer");
    assert_eq!(status, ReceiptStatus::Failed);
    assert!(target.is_none(), "a failed create names no execution");
}

// ── the lost race, through the real submit path ──────────────────────────

/// The exact sentence `crates/buzz-relay/src/handlers/ingest.rs` renders for
/// `AuthorityTransitionRefusal::StaleHead`.
const RELAY_STALE_HEAD: &str =
    "invalid: prevAccepted does not match the chain's current head (expected \
     abababababababababababababababababababababababababababababababab)";

/// And for `SeqMismatch`.
const RELAY_SEQ_MISMATCH: &str = "invalid: seq does not extend the chain (expected 4)";

#[test]
fn the_race_matcher_reads_the_relay_s_own_sentences_not_its_enum_names() {
    use super::super::handover_claim::is_race_refusal;

    // The sentences a relay actually emits.
    assert!(is_race_refusal(RELAY_STALE_HEAD));
    assert!(is_race_refusal(RELAY_SEQ_MISMATCH));
    assert!(is_race_refusal(
        "invalid: prevAccepted must be null — this chain has no accepted transitions yet"
    ));

    // The variant names, which never reach a client. Matching only these is
    // what made the whole lost-race path dead code (REVIEW B2), so a future
    // edit that swaps back to them fails here.
    assert!(!is_race_refusal("StaleHead"));
    assert!(!is_race_refusal("SeqMismatch"));

    // And refusals that are not races stay out of it: a claimant told
    // "you may not do this" must not be told "somebody else won".
    assert!(!is_race_refusal(
        "invalid: signer is not the session's current owner (ab)"
    ));
    assert!(!is_race_refusal(
        "invalid: takeover claimant is not the transition's signer"
    ));
}

#[test]
fn a_refused_write_is_read_off_the_ok_body_not_off_a_transport_error() {
    use super::super::handover_claim::write_refusal_message;

    // The shape `submit_event` returns for a refusal: HTTP 2xx, accepted
    // false. `parse_write_response` would have flattened this to
    // `CliError::Other` (exit 4) with nothing a race check could read.
    let refused =
        json!({ "event_id": hex64("aa"), "accepted": false, "message": RELAY_STALE_HEAD })
            .to_string();
    assert_eq!(
        write_refusal_message(&refused).as_deref(),
        Some(RELAY_STALE_HEAD)
    );

    let accepted = json!({ "event_id": hex64("aa"), "accepted": true, "message": "" }).to_string();
    assert_eq!(
        write_refusal_message(&accepted),
        None,
        "an accepted write is not a refusal"
    );
    assert_eq!(
        write_refusal_message("not json"),
        None,
        "an unparseable body is not evidence of a race"
    );
}

/// Serve one channel's reads so `load_handover_state` can re-read the chain
/// after a refusal: NIP-11 `self`, the genesis, the accepted chain, its
/// receipts, and an empty 44247 set.
#[tokio::test]
async fn a_lost_race_exits_five_and_names_the_claimant_who_won() {
    use axum::body::Bytes;
    use axum::extract::State;
    use axum::response::Json;
    use axum::routing::{get, post};
    use axum::Router;
    use std::sync::Arc;

    use super::super::handover::load_handover_state;
    use super::super::handover_claim::claim_session;
    use super::super::handover_render::VerificationNotes;
    use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
    use buzz_sdk::builders::build_coding_session_genesis;

    // The founder, the winner, and the loser this test speaks as.
    let founder = Keys::generate();
    let winner = Keys::generate();
    let relay = Keys::generate();
    let body = hex64("bb");

    let genesis_event = build_coding_session_genesis(
        uuid::Uuid::parse_str(CHANNEL).expect("channel"),
        &CodingSessionGenesisPayload::new(SESSION.to_owned()),
    )
    .expect("genesis builder")
    .sign_with_keys(&founder)
    .expect("sign");
    let genesis_ref = genesis_event.id.to_hex();

    // The chain the relay already accepted: the founder granted `winner`
    // operator, and `winner` took the session over.
    let (transitions, receipts) = chain_on(&founder, &winner, &relay, &body, &genesis_ref, &[]);

    struct Fixture {
        genesis: serde_json::Value,
        transitions: Vec<serde_json::Value>,
        receipts: Vec<serde_json::Value>,
        relay_self: String,
    }
    let fixture = Arc::new(Fixture {
        genesis: serde_json::to_value(&genesis_event).expect("genesis json"),
        transitions: transitions
            .iter()
            .map(|event| serde_json::to_value(event).expect("json"))
            .collect(),
        receipts: receipts
            .iter()
            .map(|event| serde_json::to_value(event).expect("json"))
            .collect(),
        relay_self: relay.public_key().to_hex(),
    });

    let app = Router::new()
        .route(
            "/",
            get({
                let relay_self = fixture.relay_self.clone();
                move || {
                    let relay_self = relay_self.clone();
                    async move { Json(json!({ "self": relay_self })) }
                }
            }),
        )
        .route(
            "/events",
            // Every write is refused with the relay's real stale-head sentence.
            post(|body: Bytes| async move {
                let event: nostr::Event = serde_json::from_slice(&body).expect("event JSON");
                Json(json!({
                    "event_id": event.id.to_hex(),
                    "accepted": false,
                    "message": RELAY_STALE_HEAD,
                }))
            }),
        )
        .route(
            "/query",
            post(
                |State(fixture): State<Arc<Fixture>>, body: Bytes| async move {
                    let filters: serde_json::Value =
                        serde_json::from_slice(&body).unwrap_or(json!([]));
                    let kind = filters
                        .get(0)
                        .and_then(|filter| filter.get("kinds"))
                        .and_then(|kinds| kinds.get(0))
                        .and_then(serde_json::Value::as_u64);
                    let rows = match kind.map(|kind| u32::try_from(kind).unwrap_or_default()) {
                        Some(buzz_core::kind::KIND_CODING_SESSION_GENESIS) => {
                            vec![fixture.genesis.clone()]
                        }
                        Some(KIND_CODING_SESSION_AUTHORITY_TRANSITION) => {
                            fixture.transitions.clone()
                        }
                        Some(KIND_SYSTEM_MESSAGE) => fixture.receipts.clone(),
                        _ => Vec::new(),
                    };
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

    // The loser: the founder, who has standing and therefore gets past every
    // check before the write.
    let client = BuzzClient::new(url, founder.clone(), None, None).expect("client");
    let state = load_handover_state(&client, CHANNEL, SESSION, Some(&genesis_ref))
        .await
        .expect("state");
    let mut notes = VerificationNotes::default();
    let error = claim_session(&client, &state, &hex64("cc"), 1, &mut notes)
        .await
        .expect_err("a refused takeover must not read as accepted");

    assert!(
        matches!(error, CliError::Conflict(_)),
        "a lost race is a write conflict, not a generic failure: got {error}"
    );
    assert_eq!(
        crate::error::exit_code(&error),
        5,
        "the contract's exit code for a lost race"
    );
    let message = error.to_string();
    assert!(
        message.contains(&super::super::crew::short_pubkey(
            &winner.public_key().to_hex()
        )),
        "the loser is told who actually holds the session: {message}"
    );
    assert!(
        message.contains("lost the race at the relay"),
        "and why: {message}"
    );
    assert!(
        message.contains("Nothing was retried"),
        "and that nothing was retried into a moved head: {message}"
    );

    server.abort();
}

#[test]
fn a_transfer_with_no_claim_in_force_is_refused_exactly_as_the_relay_refuses_it() {
    // `buzz-db/src/event.rs` answers `NoActiveClaim` before it asks whether
    // the signer may transfer, and a voided claim answers that arm too: the
    // way back from a void is a fresh takeover, never a transfer. The
    // projection has to spell the same rule, or a client would fold a link the
    // relay would never have stored (REVIEW N12).
    let founder = Keys::generate();
    let relay = Keys::generate();
    let transfer = transition_event(
        &founder,
        &CodingSessionAuthorityTransitionPayload::new_transfer(
            GENESIS,
            None,
            1,
            hex64("0b"),
            hex64("bb"),
        ),
    );
    let receipt = receipt_event(&transfer, &relay);
    let error = project_receipt_backed_authority_chain(
        &[transfer],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect_err("a transfer from NoClaim must be refused, even signed by the founder");
    assert!(error.contains("unauthorized signer"), "got {error}");
}

#[test]
fn a_founder_transfer_after_a_void_is_refused_a_fresh_takeover_is_the_way_back() {
    let founder = Keys::generate();
    let operator = Keys::generate();
    let relay = Keys::generate();
    let body = hex64("bb");

    // grant → takeover → revoke: the claim is now Voided.
    let (mut transitions, mut receipts) = chain(
        &founder,
        &operator,
        &relay,
        &body,
        &[CodingSessionAuthorityTransitionType::Revoke],
    );
    let head = transitions.last().expect("head").id.to_hex();
    let transfer = transition_event(
        &founder,
        &CodingSessionAuthorityTransitionPayload::new_transfer(
            GENESIS,
            Some(head),
            4,
            hex64("0c"),
            body.clone(),
        ),
    );
    receipts.push(receipt_event(&transfer, &relay));
    transitions.push(transfer);

    let error = project_receipt_backed_authority_chain(
        &transitions,
        &receipts,
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect_err("a voided claim has nothing to transfer");
    assert!(error.contains("unauthorized signer"), "got {error}");
}

// ── claim receipts carry a body ──────────────────────────────────────────

/// Build a receipt by hand so its exact key set can be wrong on purpose.
fn raw_receipt(
    relay: &Keys,
    accepted_event_id: &str,
    transition_type: &str,
    seq: u32,
    grantee: &str,
    body_pubkey: Option<&str>,
) -> nostr::Event {
    let mut content = json!({
        "type": AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
        "genesisRef": GENESIS,
        "acceptedEventId": accepted_event_id,
        "seq": seq,
        "transitionType": transition_type,
        "granteePubkey": grantee,
    });
    if let (Some(object), Some(body)) = (content.as_object_mut(), body_pubkey) {
        object.insert(
            "bodyPubkey".into(),
            serde_json::Value::String(body.to_owned()),
        );
    }
    EventBuilder::new(
        Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        content.to_string(),
    )
    .tags([Tag::parse(["h", CHANNEL]).expect("h")])
    .sign_with_keys(relay)
    .expect("sign")
}

#[test]
fn a_takeover_receipt_carrying_body_pubkey_folds_to_an_active_claim() {
    // The blocker from composition run 5: the receipt struct was
    // `deny_unknown_fields` without `bodyPubkey`, so the first accepted
    // takeover made every later chain read fail with "unknown field
    // `bodyPubkey`" — all four verbs, after the first one succeeded.
    let founder = Keys::generate();
    let relay = Keys::generate();
    let body = hex64("bb");
    let takeover = transition_event(
        &founder,
        &CodingSessionAuthorityTransitionPayload::new_takeover(
            GENESIS,
            None,
            1,
            founder.public_key().to_hex(),
            body.clone(),
        ),
    );
    let receipt = raw_receipt(
        &relay,
        &takeover.id.to_hex(),
        "takeover",
        1,
        &founder.public_key().to_hex(),
        Some(&body),
    );
    let projected = project_receipt_backed_authority_chain(
        &[takeover],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect("a receipt in the shape the relay actually publishes must project");
    match projected.claim {
        ClaimState::Active(claim) => assert_eq!(claim.body_pubkey, body),
        other => panic!("expected an active claim, got {other:?}"),
    }
}

#[test]
fn a_grant_receipt_carrying_a_body_is_refused() {
    // A grant fences nothing, so a receipt naming a body is describing a fence
    // nobody raised.
    let founder = Keys::generate();
    let relay = Keys::generate();
    let grant = transition_event(
        &founder,
        &CodingSessionAuthorityTransitionPayload::new_grant_operator(GENESIS, None, 1, hex64("0b")),
    );
    let receipt = raw_receipt(
        &relay,
        &grant.id.to_hex(),
        "grant-operator",
        1,
        &hex64("0b"),
        Some(&hex64("bb")),
    );
    let error = project_receipt_backed_authority_chain(
        &[grant],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect_err("must refuse");
    assert!(
        error.contains("bodyPubkey presence does not match"),
        "got {error}"
    );
}

#[test]
fn a_takeover_receipt_without_a_body_is_refused() {
    // A claim receipt with no body names a claim the fold could not apply:
    // `fold_current_claim` skips a bodyless link, so the session would read as
    // unclaimed while the relay had accepted a takeover.
    let founder = Keys::generate();
    let relay = Keys::generate();
    let takeover = transition_event(
        &founder,
        &CodingSessionAuthorityTransitionPayload::new_takeover(
            GENESIS,
            None,
            1,
            founder.public_key().to_hex(),
            hex64("bb"),
        ),
    );
    let receipt = raw_receipt(
        &relay,
        &takeover.id.to_hex(),
        "takeover",
        1,
        &founder.public_key().to_hex(),
        None,
    );
    let error = project_receipt_backed_authority_chain(
        &[takeover],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect_err("must refuse");
    assert!(
        error.contains("bodyPubkey presence does not match"),
        "got {error}"
    );
}

#[test]
fn a_receipt_whose_body_disagrees_with_its_transition_is_refused() {
    let founder = Keys::generate();
    let relay = Keys::generate();
    let takeover = transition_event(
        &founder,
        &CodingSessionAuthorityTransitionPayload::new_takeover(
            GENESIS,
            None,
            1,
            founder.public_key().to_hex(),
            hex64("bb"),
        ),
    );
    let receipt = raw_receipt(
        &relay,
        &takeover.id.to_hex(),
        "takeover",
        1,
        &founder.public_key().to_hex(),
        Some(&hex64("cc")),
    );
    let error = project_receipt_backed_authority_chain(
        &[takeover],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect_err("must refuse");
    assert!(
        error.contains("do not match the accepted transition"),
        "the fence must never be raised on whichever of the two a reader trusted: {error}"
    );
}

// ── a duplicate of our own id is not a lost write ────────────────────────

#[test]
fn the_relay_already_holding_our_own_bytes_is_success_not_a_failure() {
    use super::super::handover::{classify_own_write, OwnWriteOutcome};

    // The relay's real answer for a stored duplicate: `accepted: true` with
    // the established `duplicate:` prefix (`ingest.rs`, `!was_inserted`). A
    // nostr id is the hash of its own content, so this is the relay confirming
    // our bytes are there — and treating it as a failure dropped the patch
    // artifact and published `preserved: "none"` over a patch that existed
    // (composition run 5, finding 2).
    let stored =
        json!({ "event_id": hex64("aa"), "accepted": true, "message": "duplicate:" }).to_string();
    assert_eq!(
        classify_own_write(&stored).expect("a duplicate is not an error"),
        OwnWriteOutcome::AlreadyPresent
    );

    // Some paths answer a duplicate with `accepted: false` and the same
    // prefix; about an id we computed it means the same thing.
    let refused_duplicate = json!({
        "event_id": hex64("aa"),
        "accepted": false,
        "message": "duplicate: coding-session already founded by event abab",
    })
    .to_string();
    assert_eq!(
        classify_own_write(&refused_duplicate).expect("still not an error"),
        OwnWriteOutcome::AlreadyPresent
    );

    let published = json!({ "event_id": hex64("aa"), "accepted": true, "message": "" }).to_string();
    assert_eq!(
        classify_own_write(&published).expect("accepted"),
        OwnWriteOutcome::Published
    );

    // And a real refusal is still a refusal.
    let refused = json!({
        "event_id": hex64("aa"),
        "accepted": false,
        "message": "invalid: membership required",
    })
    .to_string();
    let error = classify_own_write(&refused).expect_err("must fail");
    assert!(error.to_string().contains("membership required"), "{error}");
}
