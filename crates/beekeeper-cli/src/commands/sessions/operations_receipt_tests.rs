use beekeeper_core::coding_session_authority_transition::{
    CodingSessionAuthorityTransitionPayload, CodingSessionAuthorityTransitionType,
    CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use beekeeper_core::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamTransactionBody,
};
use beekeeper_core::kind::{
    KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_TEAM_TRANSACTION,
    KIND_SYSTEM_MESSAGE,
};
use beekeeper_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};

use super::super::operations_authority::{
    project_receipt_backed_authority_chain, AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
};
use beekeeper_core::coding_session_authority_transition::decode_coding_session_authority_transition;

use super::super::operations_reads::{
    transaction_matches_context, transaction_query_filter, transaction_value_matches_context,
};
use super::*;

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

#[test]
fn operation_command_rejects_a_body_for_the_wrong_operation() {
    let report = json!({
        "assignmentRef": "11".repeat(32),
        "summary": "done",
        "branch": null,
        "baseSha": null,
        "headSha": null,
        "files": [],
        "tests": [],
        "redBeforeGreen": null,
        "deviations": [],
        "residuals": [],
        "anomalies": []
    });
    assert!(decode_body(CodingSessionTeamTransactionType::Assignment, report).is_err());
}

#[test]
fn body_file_syntax_does_not_guess_plain_strings_are_paths() {
    let body = read_json_argument(r#"{"status":"received"}"#).expect("inline JSON");
    assert_eq!(body["status"], "received");
}

#[tokio::test]
async fn team_operation_is_recorded_before_wake_and_wake_failure_stays_visible() {
    use std::sync::{Arc, Mutex};

    let operation_id = "ab".repeat(32);
    let order = Arc::new(Mutex::new(Vec::new()));
    let stored = Arc::new(Mutex::new(Vec::new()));
    let submit_order = Arc::clone(&order);
    let submit_stored = Arc::clone(&stored);
    let submitted_id = operation_id.clone();
    let wake_order = Arc::clone(&order);
    let wake_stored = Arc::clone(&stored);
    let wake_id = operation_id.clone();

    let output = submit_record_then_wake(
        &operation_id,
        move || async move {
            submit_order.lock().expect("order lock").push("record");
            submit_stored
                .lock()
                .expect("stored lock")
                .push(submitted_id.clone());
            Ok(json!({
                "event_id": submitted_id,
                "accepted": true,
                "message": ""
            })
            .to_string())
        },
        Some(move || async move {
            assert!(wake_stored.lock().expect("stored lock").contains(&wake_id));
            wake_order.lock().expect("order lock").push("wake");
            Err(CliError::Other("provider unavailable".into()))
        }),
        None,
    )
    .await
    .expect("stored record remains a successful operation");

    assert_eq!(*order.lock().expect("order lock"), ["record", "wake"]);
    assert_eq!(
        stored.lock().expect("stored lock").as_slice(),
        std::slice::from_ref(&operation_id)
    );
    assert_eq!(output["accepted"], true);
    assert_eq!(output["delivery"]["status"], "unconfirmed");
    assert_eq!(output["delivery"]["recordedOperationId"], operation_id);
    assert!(output["delivery"]["error"]
        .as_str()
        .expect("delivery error")
        .contains("provider unavailable"));
}

fn authority_event(
    signer: &Keys,
    transition_type: CodingSessionAuthorityTransitionType,
    grantee: &str,
    seq: u32,
    previous: Option<&Event>,
    role: Option<&str>,
) -> Event {
    let payload = match transition_type {
        CodingSessionAuthorityTransitionType::GrantSeat => {
            CodingSessionAuthorityTransitionPayload::new_grant_seat(
                GENESIS,
                previous.map(|event| event.id.to_hex()),
                seq,
                grantee,
                role.expect("seat role"),
            )
        }
        CodingSessionAuthorityTransitionType::RevokeSeat => {
            CodingSessionAuthorityTransitionPayload::new_revoke_seat(
                GENESIS,
                previous.map(|event| event.id.to_hex()),
                seq,
                grantee,
                role.expect("seat role"),
            )
        }
        _ => CodingSessionAuthorityTransitionPayload::new(
            transition_type,
            GENESIS,
            previous.map(|event| event.id.to_hex()),
            seq,
            grantee,
        ),
    };
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
        serde_json::to_string(&payload).expect("payload"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION]).expect("version"),
        Tag::parse(["csat-genesis", GENESIS]).expect("genesis"),
    ])
    .sign_with_keys(signer)
    .expect("transition")
}

fn receipt_value(transition: &Event) -> Value {
    let payload = decode_coding_session_authority_transition(&transition.content)
        .expect("transition payload");
    let mut content = json!({
        "type": AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
        "genesisRef": payload.genesis_ref,
        "acceptedEventId": transition.id.to_hex(),
        "seq": payload.seq,
        "transitionType": payload.transition_type,
        "granteePubkey": payload.grantee_pubkey,
    });
    if let (Some(object), Some(role)) = (content.as_object_mut(), payload.role) {
        object.insert("role".into(), Value::String(role));
    }
    if let (Some(object), Some(body_pubkey)) = (content.as_object_mut(), payload.body_pubkey) {
        object.insert("bodyPubkey".into(), Value::String(body_pubkey));
    }
    // The relay echoes the delegation's scope onto its receipt
    // (`buzz-relay/src/handlers/side_effects.rs`). A fixture that omitted it
    // would test a wire nobody serves — which is precisely how ledger 204's
    // regression reached an installed build with every gate green.
    if let (Some(object), Some(project_ref)) = (content.as_object_mut(), payload.project_ref) {
        object.insert("projectRef".into(), Value::String(project_ref));
    }
    content
}

fn receipt_event(content: Value, relay: &Keys, created_at: u64) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        content.to_string(),
    )
    .tags([Tag::parse(["h", CHANNEL]).expect("h")])
    .custom_created_at(Timestamp::from(created_at))
    .sign_with_keys(relay)
    .expect("receipt")
}

fn assignment_event(genesis: &str) -> Event {
    let body = CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
        assignee_actor: "cd".repeat(32),
        assignee_role: "builder".into(),
        objective: "Implement the slice".into(),
        brief: "Use the exact signed context.".into(),
        branch: None,
        base_sha: None,
        file_ownership: vec![],
        acceptance_steps: vec!["cargo test -p beekeeper-cli".into()],
    });
    let payload = coding_session_team_transaction_payload(SESSION, genesis, None, None, body);
    build_coding_session_team_transaction(CHANNEL, payload)
        .expect("builder")
        .sign_with_keys(&Keys::generate())
        .expect("assignment")
}

#[test]
fn transaction_query_and_defense_filter_pin_exact_genesis() {
    assert_eq!(
        transaction_query_filter(CHANNEL, SESSION, GENESIS),
        json!({
            "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
            "#h": [CHANNEL],
            "#d": [SESSION],
            "#cstx-genesis": [GENESIS],
        })
    );
    let requested = assignment_event(GENESIS);
    let poison = assignment_event(&"99".repeat(32));
    let requested_value = serde_json::to_value(&requested).expect("event JSON");
    let poison_value = serde_json::to_value(&poison).expect("event JSON");
    assert!(transaction_value_matches_context(
        &requested_value,
        CHANNEL,
        SESSION,
        GENESIS,
    ));
    assert!(!transaction_value_matches_context(
        &poison_value,
        CHANNEL,
        SESSION,
        GENESIS,
    ));
    assert!(transaction_matches_context(
        &requested, CHANNEL, SESSION, GENESIS
    ));
    assert!(!transaction_matches_context(
        &poison, CHANNEL, SESSION, GENESIS
    ));
}

#[test]
fn operation_pointer_query_resolves_only_from_the_exact_verified_record() {
    let requested = assignment_event(GENESIS);
    let requested_id = requested.id.to_hex();
    assert_eq!(
        operation_pointer_query_filter(&requested_id),
        json!({
            "ids": [requested_id],
            "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
        })
    );
    assert_eq!(
        operation_coordinates_from_value(
            &serde_json::to_value(&requested).expect("event JSON"),
            &requested.id.to_hex(),
        )
        .expect("signed operation supplies its own scope"),
        OperationCoordinates {
            channel: CHANNEL.into(),
            session_ref: SESSION.into(),
            genesis: GENESIS.into(),
        }
    );

    assert!(operation_coordinates_from_value(
        &serde_json::to_value(&requested).expect("event JSON"),
        &"99".repeat(32),
    )
    .is_err());

    let mut forged = serde_json::to_value(&requested).expect("event JSON");
    forged["content"] = Value::String(
        forged["content"]
            .as_str()
            .expect("content")
            .replace("Implement the slice", "Invent a different slice"),
    );
    assert!(operation_coordinates_from_value(&forged, &requested.id.to_hex()).is_err());
}

#[test]
fn only_receipt_backed_transitions_project_authority() {
    let founder = Keys::generate();
    let operator = Keys::generate().public_key().to_hex();
    let transition = authority_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantOperator,
        &operator,
        1,
        None,
        None,
    );
    let relay = Keys::generate();
    let empty = project_receipt_backed_authority_chain(
        std::slice::from_ref(&transition),
        &[],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect("raw unaccepted transition is ignored");
    assert!(empty.grants.is_empty());

    let receipt = receipt_event(receipt_value(&transition), &relay, 1_700_000_001);
    let projected = project_receipt_backed_authority_chain(
        &[transition],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect("accepted grant");
    assert!(projected
        .grants
        .iter()
        .any(|grant| grant.actor_pubkey == operator && grant.may_steer));
}

#[test]
fn forged_or_wrong_relay_receipts_are_rejected() {
    let founder = Keys::generate();
    let transition = authority_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantOperator,
        &Keys::generate().public_key().to_hex(),
        1,
        None,
        None,
    );
    let relay = Keys::generate();
    let impostor = Keys::generate();
    let wrong_signer = receipt_event(receipt_value(&transition), &impostor, 1_700_000_001);
    assert!(project_receipt_backed_authority_chain(
        std::slice::from_ref(&transition),
        &[wrong_signer],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .is_err());

    let valid = receipt_event(receipt_value(&transition), &relay, 1_700_000_002);
    let mut forged_json = serde_json::to_value(valid).expect("event JSON");
    let mut forged_content: Value =
        serde_json::from_str(forged_json["content"].as_str().expect("receipt content"))
            .expect("receipt JSON");
    forged_content["granteePubkey"] = Value::String("99".repeat(32));
    forged_json["content"] = Value::String(forged_content.to_string());
    let forged: Event = serde_json::from_value(forged_json).expect("forged event shape");
    assert!(project_receipt_backed_authority_chain(
        &[transition],
        &[forged],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .is_err());
}

#[test]
fn receipt_cannot_activate_a_transition_with_an_invalid_signature() {
    let founder = Keys::generate();
    let transition = authority_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantOperator,
        &Keys::generate().public_key().to_hex(),
        1,
        None,
        None,
    );
    let relay = Keys::generate();
    let receipt = receipt_event(receipt_value(&transition), &relay, 1_700_000_003);
    let mut forged_json = serde_json::to_value(transition).expect("event JSON");
    let mut forged_content: Value =
        serde_json::from_str(forged_json["content"].as_str().expect("transition content"))
            .expect("transition JSON");
    forged_content["granteePubkey"] = Value::String("99".repeat(32));
    forged_json["content"] = Value::String(forged_content.to_string());
    let forged_transition: Event =
        serde_json::from_value(forged_json).expect("forged transition shape");
    assert!(project_receipt_backed_authority_chain(
        &[forged_transition],
        &[receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .is_err());
}

#[test]
fn mismatched_receipt_facts_are_rejected() {
    let founder = Keys::generate();
    let worker = Keys::generate().public_key().to_hex();
    let transition = authority_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantSeat,
        &worker,
        1,
        None,
        Some("verifier"),
    );
    let relay = Keys::generate();
    for (index, mutate) in ["id", "role", "type"].into_iter().enumerate() {
        let mut content = receipt_value(&transition);
        match mutate {
            "id" => content["acceptedEventId"] = Value::String("99".repeat(32)),
            "role" => content["role"] = Value::String("builder".into()),
            "type" => content["transitionType"] = Value::String("revoke-seat".into()),
            _ => unreachable!(),
        }
        let receipt = receipt_event(content, &relay, 1_700_000_010 + index as u64);
        assert!(
            project_receipt_backed_authority_chain(
                std::slice::from_ref(&transition),
                &[receipt],
                CHANNEL,
                GENESIS,
                &founder.public_key().to_hex(),
                &relay.public_key().to_hex(),
            )
            .is_err(),
            "{mutate} mismatch must fail"
        );
    }
}

#[test]
fn gaps_and_duplicate_conflicts_are_rejected() {
    let founder = Keys::generate();
    let first = authority_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantOperator,
        &Keys::generate().public_key().to_hex(),
        1,
        None,
        None,
    );
    let second = authority_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantViewer,
        &Keys::generate().public_key().to_hex(),
        2,
        Some(&first),
        None,
    );
    let relay = Keys::generate();
    let second_receipt = receipt_event(receipt_value(&second), &relay, 1_700_000_020);
    assert!(project_receipt_backed_authority_chain(
        &[first.clone(), second.clone()],
        &[second_receipt],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .is_err());

    let first_receipt = receipt_event(receipt_value(&first), &relay, 1_700_000_021);
    let duplicate = receipt_event(receipt_value(&first), &relay, 1_700_000_022);
    assert!(project_receipt_backed_authority_chain(
        &[first, second],
        &[first_receipt, duplicate],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .is_err());
}

// ── ledger 204: the delegation receipt every team session now carries ───────

const PROJECT_REF: &str =
    "30621:efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef:kettle-smoke";

/// A `grant-project-actions` link, signed the way the desktop signs one at
/// launch for a project owner founding a team session with "Use roles" on.
fn project_action_grant_event(
    signer: &Keys,
    grantee: &str,
    seq: u32,
    previous: Option<&Event>,
    project_ref: &str,
) -> Event {
    let payload = CodingSessionAuthorityTransitionPayload::new_grant_project_actions(
        GENESIS,
        previous.map(|event| event.id.to_hex()),
        seq,
        grantee,
        project_ref,
    );
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
        serde_json::to_string(&payload).expect("payload"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION]).expect("version"),
        Tag::parse(["csat-genesis", GENESIS]).expect("genesis"),
    ])
    .sign_with_keys(signer)
    .expect("delegation")
}

/// The regression found live on `a5be5c1a5` against hive on 2026-09-20
/// (ledger 204): a chain whose second link is the project-action delegation
/// the desktop signs at launch could not be read at all, so `bee sessions
/// hire`, `seat-repair`, `report` and `operation get` each failed with
/// *malformed authority acceptance receipt: unknown field `projectRef`* and
/// no seat on such a session could publish a canonical report.
#[test]
fn a_delegation_receipt_carrying_project_ref_projects_the_chain() {
    let founder = Keys::generate();
    let relay = Keys::generate();
    let lead = Keys::generate().public_key().to_hex();

    let seat = authority_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantSeat,
        &lead,
        1,
        None,
        Some("lead"),
    );
    let delegation = project_action_grant_event(&founder, &lead, 2, Some(&seat), PROJECT_REF);
    let receipts = vec![
        receipt_event(receipt_value(&seat), &relay, 1_700_000_001),
        receipt_event(receipt_value(&delegation), &relay, 1_700_000_002),
    ];
    // The fixture must be the wire the relay actually signs, or this test
    // proves nothing.
    let delegation_receipt: Value =
        serde_json::from_str(&receipts[1].content).expect("receipt content");
    assert_eq!(delegation_receipt["projectRef"], json!(PROJECT_REF));

    let projected = project_receipt_backed_authority_chain(
        &[seat, delegation],
        &receipts,
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect("a delegation receipt must not break the chain");
    assert_eq!(projected.head_seq, 2);
    assert!(projected
        .seats
        .iter()
        .any(|held| held.actor_pubkey == lead && held.role == "lead"));
}

/// A receipt that names a different project than the link it accepts is not
/// evidence of either one: the binding refuses it the way it refuses a role
/// or a `bodyPubkey` the two disagree about.
#[test]
fn a_delegation_receipt_naming_another_project_does_not_bind() {
    let founder = Keys::generate();
    let relay = Keys::generate();
    let lead = Keys::generate().public_key().to_hex();
    let delegation = project_action_grant_event(&founder, &lead, 1, None, PROJECT_REF);
    let mut content = receipt_value(&delegation);
    content["projectRef"] = json!(
        "30621:efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef:other-project"
    );
    let error = project_receipt_backed_authority_chain(
        &[delegation],
        &[receipt_event(content, &relay, 1_700_000_001)],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
        &relay.public_key().to_hex(),
    )
    .expect_err("a scope the two disagree about must refuse");
    assert!(
        error.contains("do not match the accepted transition"),
        "{error}"
    );
}

/// The shared cross-reader vectors, run against the exact receipt decoder the
/// live projection runs (`conformance/authority-chain/README.md`).
#[test]
fn shared_authority_chain_vectors_match_the_receipt_decoder() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../conformance/authority-chain/fixtures/chain-vectors.json"
    ))
    .expect("fixture parses");
    assert_eq!(
        fixture["schema"],
        "buzz-coding-session-authority-chain-conformance/v1"
    );
    let vectors = fixture["vectors"].as_array().expect("vectors");
    assert!(!vectors.is_empty());
    for vector in vectors {
        let name = vector["name"].as_str().expect("name");
        let expected = vector["receiptValid"].as_bool().expect("receiptValid");
        assert_eq!(
            super::super::operations_authority::decode_authority_acceptance_receipt(
                vector["receipt"].clone()
            )
            .is_ok(),
            expected,
            "shared authority-chain vector {name}"
        );
    }
}
