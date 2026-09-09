//! What the native declared-work adapter accepts, refuses, and discloses.

use buzz_core_pkg::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionPayload, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use buzz_core_pkg::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use buzz_core_pkg::pulse_declared_work::PulseDeclaredAssignmentStatus;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

use super::*;

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "ab".repeat(32)
}

fn request() -> PulseDeclaredWorkRequest {
    PulseDeclaredWorkRequest {
        schema: PULSE_DECLARED_WORK_REQUEST_SCHEMA.to_owned(),
        project: "30621:11:beekeeper".to_owned(),
        channel_ids: vec![CHANNEL.to_owned()],
        now_unix: 1_756_800_960,
        viewer_pubkey: None,
        sessions: Vec::new(),
        read_errors: Vec::new(),
    }
}

fn session(session_key: &str, founder: &str) -> PulseDeclaredWorkSessionInput {
    PulseDeclaredWorkSessionInput {
        session_key: session_key.to_owned(),
        channel_ref: CHANNEL.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: genesis(),
        founder_pubkey: founder.to_owned(),
        name: Some("Declared work".to_owned()),
        lifecycle: PulseDeclaredWorkLifecycle::Open,
        latest_observation_at: Some(1_756_800_600),
        active_seats: Vec::new(),
        active_grants: Vec::new(),
        team_events: Vec::new(),
    }
}

fn assignment_event(founder: &Keys, actor: &Keys, created_at: u64) -> Event {
    let body = CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
        assignee_actor: actor.public_key().to_hex(),
        assignee_role: "builder".to_owned(),
        objective: "Build the declared-work wire".to_owned(),
        brief: "Implement the bounded assigned slice.".to_owned(),
        branch: None,
        base_sha: None,
        file_ownership: vec!["crates/buzz-core/src".to_owned()],
        acceptance_steps: vec!["cargo test -p buzz-core".to_owned()],
    });
    let payload = CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: genesis(),
        transaction_type: body.transaction_type(),
        supersedes: None,
        delivery_command_id: None,
        body,
    };
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(&payload).expect("payload"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", SESSION]).expect("d"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("v"),
        Tag::parse(["cstx-genesis", genesis().as_str()]).expect("genesis"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(founder)
    .expect("sign")
}

#[test]
fn an_unknown_request_schema_is_refused_by_name() {
    let mut request = request();
    request.schema = "buzz-pulse-declared-work-request/v2".to_owned();
    let error = declared_work(request).expect_err("refused");
    assert_eq!(
        error,
        "unsupported pulse-declared-work request schema: buzz-pulse-declared-work-request/v2"
    );
}

#[test]
fn more_sessions_than_the_cap_are_refused_rather_than_quietly_trimmed() {
    let mut request = request();
    request.sessions = (0..9)
        .map(|index| session(&format!("session-{index}"), &"11".repeat(32)))
        .collect();
    let error = declared_work(request).expect_err("refused");
    assert_eq!(
        error,
        "pulse-declared-work request carries 9 sessions; the cap is 8"
    );
}

#[test]
fn sessions_come_back_in_request_order_with_the_callers_read_errors_first() {
    let mut request = request();
    request.viewer_pubkey = Some("33".repeat(32));
    request.read_errors = vec![PulseDeclaredWorkError {
        scope: "declared".to_owned(),
        message: "relay closed the subscription".to_owned(),
    }];
    request.sessions = vec![
        session("session-b", &"11".repeat(32)),
        session("session-a", &"22".repeat(32)),
    ];

    let response = declared_work(request).expect("projected");

    assert_eq!(response.schema, "buzz-pulse-declared-work/v1");
    assert_eq!(
        response.viewer_pubkey.as_deref(),
        Some("33".repeat(32).as_str())
    );
    assert_eq!(
        response
            .sessions
            .iter()
            .map(|session| session.session_key.as_str())
            .collect::<Vec<_>>(),
        vec!["session-b", "session-a"],
        "request order, never a re-sort this adapter invented"
    );
    // A session with no records is an empty umbrella, not an unreadable one.
    assert!(response
        .sessions
        .iter()
        .all(|session| session.unreadable.is_none()));
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].scope, "declared");
}

#[test]
fn an_unreadable_session_is_returned_and_named_rather_than_dropped() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let event = assignment_event(&founder, &actor, 10);
    let mut broken = session("session-broken", &founder.public_key().to_hex());
    // The same event twice: a caller filter defect, and a hard fold error.
    broken.team_events = vec![event.clone(), event];
    let mut request = request();
    request.sessions = vec![broken];

    let response = declared_work(request).expect("projected");

    assert_eq!(response.sessions.len(), 1, "the session is still returned");
    let session = &response.sessions[0];
    assert!(session.unreadable.is_some());
    assert!(
        session.assignments.is_empty(),
        "an unreadable session claims no work"
    );
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].scope, "declared:session-broken");
    assert_eq!(
        response.errors[0].message,
        session.unreadable.clone().expect("the fold's sentence"),
        "the error carries the fold's own words"
    );
}

#[test]
fn a_signed_assignment_reaches_the_response_with_its_source_and_its_word() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let event = assignment_event(&founder, &actor, 1_756_790_160);
    let mut input = session("session-a", &founder.public_key().to_hex());
    input.team_events = vec![event.clone()];
    let mut request = request();
    request.sessions = vec![input];

    let response = declared_work(request).expect("projected");

    let session = &response.sessions[0];
    assert_eq!(session.channel_id, CHANNEL);
    assert_eq!(session.session_ref, SESSION);
    // The genesis the caller supplied reaches the response, so the details
    // block can show which signed record these 44244s name (contract §6).
    assert_eq!(session.genesis_ref, genesis());
    assert_eq!(session.lifecycle, PulseDeclaredWorkLifecycle::Open);
    assert_eq!(session.excluded_count, 0);
    assert_eq!(session.assignments.len(), 1);
    let declared = &session.assignments[0];
    assert_eq!(declared.source_event_id, event.id.to_hex());
    assert_eq!(declared.assigner_pubkey, founder.public_key().to_hex());
    assert_eq!(declared.assignee_actor, actor.public_key().to_hex());
    assert_eq!(declared.status, PulseDeclaredAssignmentStatus::Unresolved);
    assert!(response.errors.is_empty());
}

/// The request refuses a key it does not know, and names the session shape it
/// expects — the seat and grant shapes the mission adapter already declares.
#[test]
fn a_request_with_an_unknown_key_is_refused_rather_than_absorbed() {
    let error = serde_json::from_value::<PulseDeclaredWorkRequest>(serde_json::json!({
        "schema": PULSE_DECLARED_WORK_REQUEST_SCHEMA,
        "project": "30621:11:beekeeper",
        "channelIds": [CHANNEL],
        "nowUnix": 1_756_800_960,
        "viewerPubkey": null,
        "sessions": [],
        "readErrors": [],
        "displayNames": {},
    }))
    .expect_err("an unknown key is refused");
    assert!(
        error.to_string().contains("displayNames"),
        "the refusal names the key: {error}"
    );
}

#[test]
fn a_session_input_decodes_from_the_contracts_exact_camel_case_keys() {
    let session: PulseDeclaredWorkSessionInput = serde_json::from_value(serde_json::json!({
        "sessionKey": "session-a",
        "channelRef": CHANNEL,
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "founderPubkey": "11".repeat(32),
        "name": null,
        "lifecycle": "closed",
        "latestObservationAt": null,
        "activeSeats": [{"actorPubkey": "22".repeat(32), "role": "builder"}],
        "activeGrants": [{
            "actorPubkey": "33".repeat(32),
            "grantEventRef": "cc".repeat(32),
            "maySteer": true,
            "acceptedAt": 1_756_700_000,
            "granted": true,
        }],
        "teamEvents": [],
    }))
    .expect("the contract's own shape decodes");
    assert_eq!(session.lifecycle, PulseDeclaredWorkLifecycle::Closed);
    assert_eq!(session.active_seats.len(), 1);
    assert_eq!(session.active_grants.len(), 1);
}
