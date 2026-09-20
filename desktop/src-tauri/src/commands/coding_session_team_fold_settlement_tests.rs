//! Lane 183's two projections, across the Tauri boundary.
//!
//! Ledger 183(g) left `awaiting` and `pendingCompletion` on the CLI's fold
//! object and in `buzz-core`, and the adapter kept projecting only the four
//! older fields — so a person reading the app still saw the three nulls the
//! CLI no longer prints, and the 178(e) misreading stayed available to them.
//! These tests assert that the two facts cross the boundary, and that they
//! carry the *diagnosis*, not merely a flag.

use super::*;

use buzz_core_pkg::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamMissionCompleted, CodingSessionTeamReport,
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use buzz_core_pkg::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "ab".repeat(32)
}

fn context(founder: &Keys) -> CodingSessionTeamFoldAdapterContext {
    CodingSessionTeamFoldAdapterContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: genesis(),
        founder_pubkey: founder.public_key().to_hex(),
        authority_head_event_id: Some("cd".repeat(32)),
        authority_head_seq: 4,
        active_seats: Vec::new(),
        active_grants: Vec::new(),
        verifier_required: false,
    }
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

fn signed(payload: &CodingSessionTeamTransactionPayload, keys: &Keys, created_at: u64) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("serialize payload"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h tag"),
        Tag::parse(["d", SESSION]).expect("d tag"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("version tag"),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).expect("genesis tag"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign event")
}

fn fold(founder: &Keys, events: &[Event]) -> CodingSessionTeamFoldAdapterResponse {
    let mut input_event_ids = events
        .iter()
        .map(|event| event.id.to_hex())
        .collect::<Vec<_>>();
    input_event_ids.sort();
    fold_adapter(CodingSessionTeamFoldAdapterRequest {
        schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA.into(),
        context: context(founder),
        input_event_ids,
        events: events
            .iter()
            .map(|event| serde_json::to_value(event).expect("serialize event"))
            .collect(),
    })
    .expect("fold")
}

fn assignment_payload(actor: &Keys) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Assignment(
        CodingSessionTeamAssignment {
            assignee_actor: actor.public_key().to_hex(),
            assignee_role: "builder".into(),
            objective: "Build the thing".into(),
            brief: "Do it and prove it.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["desktop".into()],
            acceptance_steps: vec!["cargo test".into()],
        },
    ))
}

fn report_payload(assignment_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: assignment_ref.into(),
            summary: "Done".into(),
            branch: None,
            base_sha: None,
            head_sha: None,
            files: vec!["desktop/a.rs".into()],
            tests: Vec::new(),
            red_before_green: Some(true),
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        },
    ))
}

#[test]
fn an_assignment_with_no_report_names_the_assignee_who_owes_it() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment = signed(&assignment_payload(&actor), &founder, 1);
    let response = fold(&founder, std::slice::from_ref(&assignment));

    let settlement = &response.assignments[0];
    assert!(!settlement.settled);
    let awaiting = settlement.awaiting.as_ref().expect("a missing link");
    assert_eq!(awaiting.link, "report");
    assert_eq!(awaiting.owed_by_role, "builder");
    assert_eq!(
        awaiting.owed_by_actor.as_deref(),
        Some(actor.public_key().to_hex().as_str()),
        "exactly one party can file this report, so it is named"
    );
}

#[test]
fn a_report_with_no_ruling_names_no_actor_because_several_may_rule() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment = signed(&assignment_payload(&actor), &founder, 1);
    let report = signed(&report_payload(&assignment.id.to_hex()), &actor, 2);
    let response = fold(&founder, &[assignment, report]);

    let awaiting = response.assignments[0]
        .awaiting
        .as_ref()
        .expect("a missing link");
    assert_eq!(awaiting.link, "disposition");
    assert_eq!(awaiting.owed_by_role, "lead");
    // Founder, any active lead seat and any steer-grant holder may all rule.
    assert_eq!(awaiting.owed_by_actor, None);
}

#[test]
fn a_completion_held_for_a_late_prerequisite_crosses_as_pending_not_as_silence() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment = signed(&assignment_payload(&actor), &founder, 1);
    let completed = signed(
        &payload(CodingSessionTeamTransactionBody::MissionCompleted(
            CodingSessionTeamMissionCompleted {
                assignment_refs: vec![assignment.id.to_hex()],
                landed_shas: Vec::new(),
                summary: "Mission complete".into(),
                follow_ups: Vec::new(),
            },
        )),
        &founder,
        5,
    );
    let response = fold(&founder, &[assignment.clone(), completed.clone()]);

    assert!(
        response.canonical_terminal.is_none(),
        "a held completion is not a terminal"
    );
    let pending = response
        .pending_completion
        .as_ref()
        .expect("the held completion");
    assert_eq!(pending.event_id, completed.id.to_hex());
    assert!(
        !pending.reason.is_empty(),
        "the fold's own sentence, verbatim"
    );
    assert_eq!(
        pending.unsettled_assignment_event_ids,
        vec![assignment.id.to_hex()],
        "the assignment whose own `awaiting` says where the wait is"
    );
}
