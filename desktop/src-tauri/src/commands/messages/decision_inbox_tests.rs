use std::collections::HashMap;

use buzz_core_pkg::coding_session_team_transaction::{
    CodingSessionTeamDecisionAnswer, CodingSessionTeamDecisionChoice,
    CodingSessionTeamDecisionRequest, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionPayload, CODING_SESSION_TEAM_DECISION_FOUNDER,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use buzz_core_pkg::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};

use super::*;

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "ab".repeat(32)
}

fn payload(body: CodingSessionTeamTransactionBody) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: genesis(),
        transaction_type: body.transaction_type(),
        supersedes: None,
        delivery_command_id: None,
        body,
    }
}

fn signed(payload: &CodingSessionTeamTransactionPayload, keys: &Keys, at: u64) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("serialize payload"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", payload.session_ref.as_str()]).expect("d"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("v"),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).expect("genesis"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type"),
    ])
    .custom_created_at(Timestamp::from_secs(at))
    .sign_with_keys(keys)
    .expect("sign")
}

/// The shape of run 2's `85266e05…`: a lead asks the founder to pick one.
fn founder_request(held_on: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::DecisionRequest(
        CodingSessionTeamDecisionRequest {
            question: "The seat has no git identity. Which should it commit as?".into(),
            options: vec![
                "Configure the seat's own identity".into(),
                "Commit as the founder".into(),
            ],
            held_on: held_on.into(),
            blocks: Vec::new(),
            recommendation: None,
        },
    ))
}

fn answer(request_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::DecisionAnswer(
        CodingSessionTeamDecisionAnswer {
            request_ref: request_ref.into(),
            choice: CodingSessionTeamDecisionChoice::Index(0),
            note: None,
            condition: None,
        },
    ))
}

#[test]
fn a_founder_held_request_reaches_the_founder_and_nobody_else() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let bystander = Keys::generate();
    let request = signed(
        &founder_request(CODING_SESSION_TEAM_DECISION_FOUNDER),
        &lead,
        10,
    );
    // The envelope a 44244 must keep: five tags, no `p` — the root of 249(A).
    assert_eq!(request.tags.len(), 5);
    assert!(request.tags.iter().all(|tag| tag.as_slice()[0] != "p"));

    let open = open_decision_requests(std::slice::from_ref(&request));
    assert_eq!(open.len(), 1);
    let signers = HashMap::from([(genesis(), founder.public_key().to_hex())]);
    let none = HashMap::new();
    assert!(admitted_for(
        &open[0],
        &founder.public_key().to_hex(),
        &signers,
        &none
    ));
    assert!(!admitted_for(
        &open[0],
        &bystander.public_key().to_hex(),
        &signers,
        &none
    ));
    // The asker does not judge its own question.
    assert!(!admitted_for(
        &open[0],
        &lead.public_key().to_hex(),
        &signers,
        &none
    ));
    // An unread genesis names nobody, rather than defaulting to this user.
    assert!(!admitted_for(
        &open[0],
        &founder.public_key().to_hex(),
        &HashMap::new(),
        &none
    ));
}

#[test]
fn a_seat_held_request_reaches_that_seat_and_its_owner_not_the_founder() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let architect = Keys::generate();
    let owner = Keys::generate();
    let request = signed(
        &founder_request(&architect.public_key().to_hex()),
        &lead,
        10,
    );
    let open = open_decision_requests(std::slice::from_ref(&request));
    let signers = HashMap::from([(genesis(), founder.public_key().to_hex())]);
    let owners = HashMap::from([(architect.public_key().to_hex(), owner.public_key().to_hex())]);
    assert!(admitted_for(
        &open[0],
        &architect.public_key().to_hex(),
        &signers,
        &owners
    ));
    assert!(admitted_for(
        &open[0],
        &owner.public_key().to_hex(),
        &signers,
        &owners
    ));
    assert!(!admitted_for(
        &open[0],
        &founder.public_key().to_hex(),
        &signers,
        &owners
    ));
}

#[test]
fn an_answered_or_superseded_request_leaves_the_inbox() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let asked = signed(
        &founder_request(CODING_SESSION_TEAM_DECISION_FOUNDER),
        &lead,
        10,
    );
    let answered = signed(&answer(&asked.id.to_hex()), &founder, 20);
    assert!(open_decision_requests(&[asked.clone(), answered]).is_empty());

    let mut correction = founder_request(CODING_SESSION_TEAM_DECISION_FOUNDER);
    correction.supersedes = Some(asked.id.to_hex());
    let corrected = signed(&correction, &lead, 30);
    let open = open_decision_requests(&[asked, corrected.clone()]);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].event_id, corrected.id.to_hex());
}
