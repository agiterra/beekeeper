use super::*;

use buzz_core_pkg::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamMissionCompleted, CodingSessionTeamReport, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionPayload, CodingSessionTeamVerdict,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use buzz_core_pkg::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::json;

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const OTHER_CHANNEL: &str = "15ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn id(byte: &str) -> String {
    byte.repeat(32)
}

fn context(founder: &Keys) -> CodingSessionTeamFoldAdapterContext {
    CodingSessionTeamFoldAdapterContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        founder_pubkey: founder.public_key().to_hex(),
        authority_head_event_id: Some(id("cd")),
        authority_head_seq: 4,
        active_seats: Vec::new(),
        active_grants: Vec::new(),
    }
}

fn request(founder: &Keys, events: &[Event]) -> CodingSessionTeamFoldAdapterRequest {
    let mut input_event_ids = events
        .iter()
        .map(|event| event.id.to_hex())
        .collect::<Vec<_>>();
    input_event_ids.sort();
    CodingSessionTeamFoldAdapterRequest {
        schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA.into(),
        context: context(founder),
        input_event_ids,
        events: events.iter().map(event_value).collect(),
    }
}

fn payload(body: CodingSessionTeamTransactionBody) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        transaction_type: body.transaction_type(),
        supersedes: None,
        delivery_command_id: None,
        body,
    }
}

fn assignment(actor: &Keys) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Assignment(
        CodingSessionTeamAssignment {
            assignee_actor: actor.public_key().to_hex(),
            assignee_role: "builder".into(),
            objective: "Build the native fold adapter".into(),
            brief: "Implement and verify the bounded Tauri slice.".into(),
            branch: Some("portable-team-loop".into()),
            base_sha: None,
            file_ownership: vec!["desktop/src-tauri".into()],
            acceptance_steps: vec!["cargo test".into()],
        },
    ))
}

fn report(assignment_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: assignment_ref.into(),
            summary: "Adapter and tests complete".into(),
            branch: Some("portable-team-loop".into()),
            base_sha: None,
            head_sha: None,
            files: vec!["desktop/src-tauri/src/commands/coding_session_team_fold.rs".into()],
            tests: Vec::new(),
            red_before_green: Some(true),
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        },
    ))
}

fn disposition(assignment_ref: &str, report_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            refutation_ref: None,
            decision: CodingSessionTeamDispositionDecision::Approve,
            summary: "Approved".into(),
            findings: Vec::new(),
            required_action: None,
        },
    ))
}

fn acknowledgement(disposition_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Acknowledgement(
        CodingSessionTeamAcknowledgement {
            acknowledged_event_ref: disposition_ref.into(),
            status: CodingSessionTeamAcknowledgementStatus::Received,
            note: None,
        },
    ))
}

fn completed(assignment_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionCompleted(
        CodingSessionTeamMissionCompleted {
            assignment_refs: vec![assignment_ref.into()],
            landed_shas: Vec::new(),
            summary: "Mission complete".into(),
            follow_ups: Vec::new(),
        },
    ))
}

fn signed(payload: &CodingSessionTeamTransactionPayload, keys: &Keys, created_at: u64) -> Event {
    signed_in_channel(payload, keys, created_at, CHANNEL)
}

fn signed_in_channel(
    payload: &CodingSessionTeamTransactionPayload,
    keys: &Keys,
    created_at: u64,
    channel: &str,
) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("serialize payload"),
    )
    .tags([
        Tag::parse(["h", channel]).expect("h tag"),
        Tag::parse(["d", payload.session_ref.as_str()]).expect("d tag"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("version tag"),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).expect("genesis tag"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign event")
}

fn event_value(event: &Event) -> serde_json::Value {
    serde_json::to_value(event).expect("serialize signed event")
}

fn full_chain() -> (Keys, Vec<Event>, String, String, String, String, String) {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment = signed(&assignment(&actor), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex()), &actor, 2);
    let disposition = signed(
        &disposition(&assignment.id.to_hex(), &report.id.to_hex()),
        &founder,
        3,
    );
    let acknowledgement = signed(&acknowledgement(&disposition.id.to_hex()), &actor, 4);
    let completed = signed(&completed(&assignment.id.to_hex()), &founder, 5);
    let ids = (
        assignment.id.to_hex(),
        report.id.to_hex(),
        disposition.id.to_hex(),
        acknowledgement.id.to_hex(),
        completed.id.to_hex(),
    );
    (
        founder,
        vec![completed, report, acknowledgement, assignment, disposition],
        ids.0,
        ids.1,
        ids.2,
        ids.3,
        ids.4,
    )
}

#[test]
fn full_approval_chain_returns_closed_provenance_bound_projection() {
    let (founder, events, assignment, report, disposition, acknowledgement, completed) =
        full_chain();
    let expected_context = context(&founder);
    let response = fold_adapter(request(&founder, &events)).expect("canonical fold");

    assert_eq!(response.schema, "buzz-coding-session-team-fold-adapter/v1");
    assert_eq!(
        response.context,
        CodingSessionTeamFoldAdapterContextEcho::from(&expected_context)
    );
    assert_eq!(response.implementation, "buzz-core");
    assert!(response
        .input_event_ids
        .windows(2)
        .all(|pair| pair[0] < pair[1]));
    assert_eq!(response.included_event_ids.len(), 5);
    assert!(response.excluded.is_empty());
    assert!(response.conflicts.is_empty());
    assert_eq!(response.assignments.len(), 1);
    assert_eq!(
        response.assignments[0],
        CodingSessionTeamFoldAdapterSettlement {
            assignment_event_id: assignment,
            governed_report_event_id: Some(report),
            disposition_event_id: Some(disposition),
            acknowledgement_event_id: Some(acknowledgement),
            settled: true,
        }
    );
    assert_eq!(
        response.canonical_terminal,
        Some(CodingSessionTeamFoldAdapterTerminal {
            event_id: completed,
            transaction_type: "mission.completed".into(),
        })
    );

    let wire = serde_json::to_value(response).expect("serialize response");
    let mut response_keys = wire
        .as_object()
        .expect("response object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    response_keys.sort_unstable();
    assert_eq!(
        response_keys,
        vec![
            "assignments",
            "canonicalTerminal",
            "conflicts",
            "context",
            "excluded",
            "implementation",
            "includedEventIds",
            "inputEventIds",
            "schema",
        ]
    );
    assert_eq!(wire["schema"], "buzz-coding-session-team-fold-adapter/v1");
    assert_eq!(wire["context"]["channelRef"], CHANNEL);
    assert_eq!(wire["context"]["sessionRef"], SESSION);
    assert_eq!(wire["context"]["genesisRef"], id("ab"));
    assert_eq!(wire["context"]["authorityHeadEventId"], id("cd"));
    assert_eq!(wire["context"]["authorityHeadSeq"], 4);
    assert!(wire["context"].get("activeSeats").is_none());
    assert_eq!(wire["canonicalTerminal"]["type"], "mission.completed");
}

#[test]
fn response_is_deterministic_for_the_same_signed_input_set() {
    let (founder, events, ..) = full_chain();
    let forward = fold_adapter(request(&founder, &events)).expect("forward fold");
    let reversed = events.iter().cloned().rev().collect::<Vec<_>>();
    let reverse = fold_adapter(request(&founder, &reversed)).expect("reverse fold");

    assert_eq!(forward, reverse);
    assert_eq!(
        serde_json::to_vec(&forward).expect("forward response"),
        serde_json::to_vec(&reverse).expect("reverse response")
    );
}

#[test]
fn request_cannot_supply_native_exclusions_or_hide_the_terminal() {
    let (founder, events, ..) = full_chain();
    let mut value = serde_json::to_value(request(&founder, &events)).expect("request json");
    value["exclusions"] = json!([{
        "eventId": id("ef"),
        "code": "unauthorized",
        "reason": "caller assertion"
    }]);
    value["terminal"] = serde_json::Value::Null;

    let error = serde_json::from_value::<CodingSessionTeamFoldAdapterRequest>(value)
        .expect_err("native outputs are not request fields");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn request_schema_and_sorted_input_id_echo_are_exact() {
    let (founder, events, ..) = full_chain();

    let mut wrong_schema = request(&founder, &events);
    wrong_schema.schema = "buzz-coding-session-team-fold-request/v2".into();
    let error = fold_adapter(wrong_schema).expect_err("wrong request schema");
    assert!(error.contains("buzz-coding-session-team-fold-request/v1"));

    let mut missing_id = request(&founder, &events);
    missing_id.input_event_ids.pop();
    let error = fold_adapter(missing_id).expect_err("missing bound input id");
    assert!(error.contains("do not exactly match"));

    let mut unsorted = request(&founder, &events);
    unsorted.input_event_ids.reverse();
    let error = fold_adapter(unsorted).expect_err("unsorted ids");
    assert!(error.contains("strictly sorted"));
}

#[test]
fn authority_head_sequence_rejects_values_above_the_canonical_u32_width() {
    let founder = Keys::generate();
    let mut value = serde_json::to_value(request(&founder, &[])).expect("request json");
    value["context"]["authorityHeadSeq"] = json!(4_294_967_296_u64);

    let error = serde_json::from_value::<CodingSessionTeamFoldAdapterRequest>(value)
        .expect_err("sequence above u32");
    assert!(error.to_string().contains("invalid value"));
}

#[test]
fn duplicate_malformed_tampered_and_incomplete_inputs_fail_closed() {
    let (founder, events, _, report_id, ..) = full_chain();
    let duplicate_events = vec![events[0].clone(), events[0].clone()];
    let duplicate = fold_adapter(request(&founder, &duplicate_events)).expect_err("duplicate id");
    assert!(duplicate.contains("strictly sorted without duplicates"));

    let malformed = fold_adapter(CodingSessionTeamFoldAdapterRequest {
        schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA.into(),
        context: context(&founder),
        input_event_ids: vec![id("ee")],
        events: vec![json!({"kind": KIND_CODING_SESSION_TEAM_TRANSACTION})],
    })
    .expect_err("malformed event");
    assert!(malformed.contains("is not a signed Nostr event"));

    let mut tampered = event_value(&events[0]);
    tampered["id"] = json!(id("00"));
    let invalid_signature = fold_adapter(CodingSessionTeamFoldAdapterRequest {
        schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA.into(),
        context: context(&founder),
        input_event_ids: vec![id("00")],
        events: vec![tampered],
    })
    .expect_err("tampered input id");
    assert!(invalid_signature.contains("invalid team-transaction signature"));

    let report_only = events
        .iter()
        .find(|event| event.id.to_hex() == report_id)
        .expect("report event");
    let incomplete = fold_adapter(request(&founder, std::slice::from_ref(report_only)))
        .expect_err("dangling assignment");
    assert!(incomplete.contains("dangling reference"));
}

#[test]
fn wrong_scope_and_unbound_authority_head_fail() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment = signed_in_channel(&assignment(&actor), &founder, 1, OTHER_CHANNEL);
    let wrong_channel = fold_adapter(request(&founder, std::slice::from_ref(&assignment)))
        .expect_err("cross-channel event");
    assert!(wrong_channel.contains("crosses the supplied channel"));

    let mut zero_head = context(&founder);
    zero_head.authority_head_seq = 0;
    let error = fold_adapter(CodingSessionTeamFoldAdapterRequest {
        schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA.into(),
        context: zero_head,
        input_event_ids: Vec::new(),
        events: Vec::new(),
    })
    .expect_err("zero authority sequence");
    assert!(error.contains("authorityHeadSeq must be at least 1"));

    let mut malformed_head = context(&founder);
    malformed_head.authority_head_event_id = Some(id("AB"));
    let error = fold_adapter(CodingSessionTeamFoldAdapterRequest {
        schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA.into(),
        context: malformed_head,
        input_event_ids: Vec::new(),
        events: Vec::new(),
    })
    .expect_err("noncanonical authority head");
    assert!(error.contains("lowercase 64-hex"));

    let mut missing_head = context(&founder);
    missing_head.authority_head_event_id = None;
    missing_head.authority_head_seq = 0;
    missing_head
        .active_seats
        .push(CodingSessionTeamActiveSeatInput {
            actor_pubkey: actor.public_key().to_hex(),
            role: "verifier".into(),
            grant_event_ref: id("de"),
        });
    let error = fold_adapter(CodingSessionTeamFoldAdapterRequest {
        schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA.into(),
        context: missing_head,
        input_event_ids: Vec::new(),
        events: Vec::new(),
    })
    .expect_err("active projection without provenance");
    assert!(error.contains("requires authority-head provenance"));

    let mut sequence_without_head = context(&founder);
    sequence_without_head.authority_head_event_id = None;
    let error = fold_adapter(CodingSessionTeamFoldAdapterRequest {
        schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA.into(),
        context: sequence_without_head,
        input_event_ids: Vec::new(),
        events: Vec::new(),
    })
    .expect_err("sequence without event id");
    assert!(error.contains("must be present"));
}
