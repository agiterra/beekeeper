use super::*;

use beekeeper_core_pkg::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDecisionRequest,
    CodingSessionTeamDispositionDecision, CodingSessionTeamMissionCompleted, CodingSessionTeamNote,
    CodingSessionTeamReport, CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CodingSessionTeamVerdict, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use beekeeper_core_pkg::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
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
        // What a caller that has not read the policy set must say. The
        // founder-acts surface supplies the real value from the policy hook;
        // with `false` this adapter folds exactly as it did before the field
        // existed.
        verifier_required: false,
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
            // Lane 210: the receipt is on the wire, so it takes the label
            // even though this disposition asks nothing.
            settled_by: Some("acknowledgement".into()),
            awaiting: None,
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
            // B1c: the two state-free verbs and the waiting state they create.
            "decisions",
            "excluded",
            "implementation",
            "includedEventIds",
            "inputEventIds",
            "notes",
            // Ledger 183(a)/(b): a completion held for a late prerequisite.
            "pendingCompletion",
            "schema",
            "unseatedReports",
            "waitingOnDecision",
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

/// L8.3 (REVIEW-L8 F5) joined to L7: `verifierRequired` crosses this boundary,
/// comes back echoed, **and is folded with**.
///
/// The echo alone was the honest claim while core had no such field. It does
/// now (`CodingSessionTeamFoldContext::verifier_required`), so this asserts the
/// stronger thing: the same events with the flag off and on give different
/// projections. A test that still only checked the echo would let the wiring
/// rot back out without going red.
#[test]
fn verifier_required_crosses_the_boundary_and_is_folded_with() {
    let (founder, events, ..) = full_chain();

    for asked in [true, false] {
        let mut request = request(&founder, &events);
        request.context.verifier_required = asked;
        let response = fold_adapter(request).expect("fold");
        assert_eq!(
            response.context.verifier_required, asked,
            "the adapter echoes what it was asked with"
        );
    }

    // Absent is refused, not defaulted: a caller that has not read the policy
    // set has to say `false` on purpose.
    let mut value = serde_json::to_value(request(&founder, &events)).expect("serialize");
    value["context"]
        .as_object_mut()
        .expect("context object")
        .remove("verifierRequired");
    let error = serde_json::from_value::<CodingSessionTeamFoldAdapterRequest>(value)
        .expect_err("verifierRequired is required");
    assert!(error.to_string().contains("verifierRequired"));
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
fn duplicate_malformed_and_tampered_inputs_fail_closed() {
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

    // An incomplete set is NOT a caller error: the one record that names an
    // event nobody supplied is excluded and disclosed, and the fold still
    // answers. (Batch 2 2026-09-01, B1b — the live c737be4c denial of service.)
    let report_only = events
        .iter()
        .find(|event| event.id.to_hex() == report_id)
        .expect("report event");
    let incomplete = fold_adapter(request(&founder, std::slice::from_ref(report_only)))
        .expect("an unsupplied parent is one record's defect, not a fold failure");
    assert!(incomplete.included_event_ids.is_empty());
    assert_eq!(incomplete.excluded.len(), 1);
    assert_eq!(incomplete.excluded[0].event_id, report_id);
    assert_eq!(
        incomplete.excluded[0].code,
        CodingSessionTeamFoldAdapterExclusionCode::DanglingReference
    );
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

// ── Unseated-report disclosure (batch 2026-09-01, §1d) ───────────────────────

#[test]
fn an_included_report_without_an_active_seat_crosses_the_boundary_as_disclosure() {
    let (founder, events, assignment, report, ..) = full_chain();
    let report_author = events
        .iter()
        .find(|event| event.id.to_hex() == report)
        .expect("report event")
        .pubkey
        .to_hex();

    let response = fold_adapter(request(&founder, &events)).expect("canonical fold");
    assert!(response.included_event_ids.contains(&report));
    assert_eq!(
        response.unseated_reports,
        vec![CodingSessionTeamFoldAdapterUnseatedReport {
            event_id: report.clone(),
            author_pubkey: report_author.clone(),
            assignment_ref: assignment,
            assignee_role: "builder".into(),
        }]
    );

    let wire = serde_json::to_value(&response).expect("serialize response");
    assert_eq!(wire["unseatedReports"][0]["eventId"], report);
    assert_eq!(wire["unseatedReports"][0]["authorPubkey"], report_author);
    assert_eq!(wire["unseatedReports"][0]["assigneeRole"], "builder");
    let mut keys = wire["unseatedReports"][0]
        .as_object()
        .expect("row object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["assigneeRole", "assignmentRef", "authorPubkey", "eventId"]
    );
}

#[test]
fn a_seated_report_author_crosses_the_boundary_with_an_empty_disclosure() {
    let (founder, events, .., report, _, _, _) = full_chain();
    let report_author = events
        .iter()
        .find(|event| event.id.to_hex() == report)
        .expect("report event")
        .pubkey
        .to_hex();
    let mut seated = request(&founder, &events);
    seated
        .context
        .active_seats
        .push(CodingSessionTeamActiveSeatInput {
            actor_pubkey: report_author,
            role: "builder".into(),
            grant_event_ref: id("de"),
        });

    let response = fold_adapter(seated).expect("canonical fold");
    assert!(response.unseated_reports.is_empty());
    let wire = serde_json::to_value(&response).expect("serialize response");
    assert_eq!(wire["unseatedReports"], json!([]));
}

#[test]
fn a_dangling_assignment_ref_is_excluded_and_the_rest_of_the_fold_survives() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let good_report = signed(&report(&assignment_event.id.to_hex()), &actor, 2);
    let bad_report = signed(&report(&id("77")), &actor, 3);
    let assignment_id = assignment_event.id.to_hex();
    let good_report_id = good_report.id.to_hex();
    let bad_report_id = bad_report.id.to_hex();
    let events = vec![assignment_event, good_report, bad_report];

    let response = fold_adapter(request(&founder, &events))
        .expect("one malformed record cannot fail the fold");

    assert_eq!(
        response.included_event_ids,
        vec![assignment_id, good_report_id]
    );
    assert_eq!(response.excluded.len(), 1);
    assert_eq!(response.excluded[0].event_id, bad_report_id);
    assert_eq!(
        response.excluded[0].code,
        CodingSessionTeamFoldAdapterExclusionCode::DanglingReference
    );
    assert!(response.excluded[0].reason.contains(&id("77")));

    let wire = serde_json::to_value(&response).expect("serialize response");
    let mut exclusion_keys = wire["excluded"][0]
        .as_object()
        .expect("exclusion object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    exclusion_keys.sort_unstable();
    assert_eq!(exclusion_keys, vec!["code", "eventId", "reason"]);
    assert_eq!(wire["excluded"][0]["code"], "dangling_reference");
}

#[test]
fn an_invalid_correction_is_excluded_alone_and_crosses_the_boundary_as_a_code() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let other_actor = Keys::generate();
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let other_assignment = signed(&assignment(&other_actor), &founder, 2);
    let report_event = signed(&report(&assignment_event.id.to_hex()), &actor, 3);
    // Fix round 2's live shape: a correction that moves the report to a
    // different assignment, which changes its logical subject.
    let mut correction_payload = report(&other_assignment.id.to_hex());
    correction_payload.supersedes = Some(report_event.id.to_hex());
    let correction = signed(&correction_payload, &actor, 4);
    let report_id = report_event.id.to_hex();
    let correction_id = correction.id.to_hex();
    let events = vec![assignment_event, other_assignment, report_event, correction];

    let response = fold_adapter(request(&founder, &events))
        .expect("an invalid correction cannot fail the whole fold");

    assert_eq!(response.included_event_ids.len(), 3);
    assert!(response.included_event_ids.contains(&report_id));
    assert_eq!(response.excluded.len(), 1);
    assert_eq!(response.excluded[0].event_id, correction_id);
    assert_eq!(
        response.excluded[0].code,
        CodingSessionTeamFoldAdapterExclusionCode::InvalidCorrection
    );
    assert!(response.excluded[0].reason.contains(&report_id));

    let wire = serde_json::to_value(&response).expect("serialize response");
    assert_eq!(wire["excluded"][0]["code"], "invalid_correction");
}

#[test]
fn a_wrong_type_reference_is_excluded_alone_and_crosses_the_boundary_as_a_code() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let good_report = signed(&report(&assignment_event.id.to_hex()), &actor, 2);
    // Fix round 3: a report whose `assignmentRef` names a report, not an
    // assignment. One record's defect, not the session's.
    let wrong_type = signed(&report(&good_report.id.to_hex()), &actor, 3);
    let good_report_id = good_report.id.to_hex();
    let wrong_type_id = wrong_type.id.to_hex();
    let events = vec![assignment_event, good_report, wrong_type];

    let response = fold_adapter(request(&founder, &events))
        .expect("a wrong-type reference cannot fail the whole fold");

    assert_eq!(response.included_event_ids.len(), 2);
    assert!(response.included_event_ids.contains(&good_report_id));
    assert_eq!(response.excluded.len(), 1);
    assert_eq!(response.excluded[0].event_id, wrong_type_id);
    assert_eq!(
        response.excluded[0].code,
        CodingSessionTeamFoldAdapterExclusionCode::WrongTypeReference
    );
    assert!(response.excluded[0].reason.contains("wrong-type"));

    let wire = serde_json::to_value(&response).expect("serialize response");
    assert_eq!(wire["excluded"][0]["code"], "wrong_type_reference");
}

// --- B1c: the two state-free verbs cross this boundary with exact fields.

fn note(text: &str, refs: Vec<String>) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Note(
        CodingSessionTeamNote {
            text: text.into(),
            refs,
        },
    ))
}

fn decision_request(held_on: &str, blocks: Vec<String>) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::DecisionRequest(
        CodingSessionTeamDecisionRequest {
            question: "Ship the CLI fix now, or after the app rebuild?".into(),
            options: vec!["now".into(), "after the rebuild".into()],
            held_on: held_on.into(),
            blocks,
            recommendation: None,
        },
    ))
}

#[test]
fn notes_decisions_and_the_waiting_state_cross_the_boundary_with_exact_fields() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let note_event = signed(
        &note("Lane B is rebasing; nothing is blocked.", vec![id("77")]),
        &founder,
        2,
    );
    let request_event = signed(
        &decision_request("founder", vec![assignment_id.clone()]),
        &founder,
        3,
    );
    let note_id = note_event.id.to_hex();
    let request_id = request_event.id.to_hex();
    let events = vec![assignment_event, note_event, request_event];

    let response = fold_adapter(request(&founder, &events)).expect("canonical fold");
    let wire = serde_json::to_value(&response).expect("serialize response");

    // A note is listed, and its unresolvable pointer costs it nothing.
    let mut note_keys = wire["notes"][0]
        .as_object()
        .expect("note object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    note_keys.sort_unstable();
    assert_eq!(note_keys, vec!["authorPubkey", "eventId", "refs"]);
    assert_eq!(
        wire["notes"],
        json!([{
            "eventId": note_id,
            "authorPubkey": founder.public_key().to_hex(),
            "refs": [id("77")],
        }])
    );

    let mut decision_keys = wire["decisions"][0]
        .as_object()
        .expect("decision object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    decision_keys.sort_unstable();
    assert_eq!(
        decision_keys,
        vec!["answerId", "answeredBy", "blocks", "heldOn", "requestId"]
    );
    assert_eq!(
        wire["decisions"],
        json!([{
            "requestId": request_id,
            "heldOn": "founder",
            "blocks": [assignment_id],
            "answeredBy": null,
            "answerId": null,
        }])
    );
    assert_eq!(
        wire["waitingOnDecision"],
        json!({ "requestId": request_id, "heldOn": "founder" })
    );
    // Waiting on a person carries no terminal. That is the whole point.
    assert_eq!(wire["canonicalTerminal"], json!(null));
    assert!(wire["excluded"].as_array().expect("excluded").is_empty());
}

#[test]
fn a_quiet_session_answers_empty_note_and_decision_collections() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let response = fold_adapter(request(&founder, &[assignment_event])).expect("canonical fold");
    let wire = serde_json::to_value(&response).expect("serialize response");

    // Present and empty, never absent: a reader must be able to tell "nothing
    // was said" from "this adapter does not disclose notes".
    assert_eq!(wire["notes"], json!([]));
    assert_eq!(wire["decisions"], json!([]));
    assert_eq!(wire["waitingOnDecision"], json!(null));
}

/// Path of the fixture the Desktop decoder test reads, relative to this crate.
const TS_DECODER_FIXTURE: &str =
    "../src/features/coding-sessions/lib/codingSessionTeamFoldAdapterResponse.fixture.json";

/// Deterministic keys, so the generated fixture is byte-stable across runs.
fn fixed_keys(byte: u8) -> Keys {
    Keys::parse(&format!("{byte:02x}").repeat(32)).expect("fixed secret key")
}

#[test]
fn the_typescript_decoder_fixture_is_this_adapter_s_real_output() {
    // REVIEW-B1c B1: the Desktop decoder's own fixtures were hand-written, so
    // `pnpm test` stayed green (7310/7310) while the decoder would have thrown
    // for every real session. This fixture is generated from the adapter, and
    // the Desktop test decodes exactly this file, so the two sides can never
    // silently diverge again. Regenerate with
    // `BEEKEEPER_UPDATE_FIXTURES=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml the_typescript_decoder_fixture`.
    let founder = fixed_keys(0x11);
    let actor = fixed_keys(0x22);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let report_event = signed(&report(&assignment_id), &actor, 2);
    let note_event = signed(
        &note("Lane B is rebasing; nothing is blocked.", vec![id("77")]),
        &actor,
        3,
    );
    let request_event = signed(
        &decision_request("founder", vec![assignment_id.clone()]),
        &actor,
        4,
    );
    let events = vec![assignment_event, report_event, note_event, request_event];

    let mut seated = request(&founder, &events);
    seated
        .context
        .active_seats
        .push(CodingSessionTeamActiveSeatInput {
            actor_pubkey: actor.public_key().to_hex(),
            role: "builder".into(),
            grant_event_ref: id("de"),
        });
    let response = fold_adapter(seated).expect("canonical fold");
    let generated = serde_json::to_string_pretty(&response).expect("serialize response") + "\n";

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TS_DECODER_FIXTURE);
    if std::env::var("BEEKEEPER_UPDATE_FIXTURES").is_ok() {
        std::fs::write(&path, &generated).expect("write fixture");
    }
    let stored = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("read {}: {error}", path.display());
    });
    assert_eq!(
        stored, generated,
        "the Desktop decoder fixture is stale; regenerate it with BEEKEEPER_UPDATE_FIXTURES=1"
    );

    // The fixture must actually exercise all three new collections, or it
    // proves nothing about the decoder change it exists to pin.
    let wire: serde_json::Value = serde_json::from_str(&generated).expect("fixture JSON");
    assert_eq!(wire["notes"].as_array().expect("notes").len(), 1);
    assert_eq!(wire["decisions"].as_array().expect("decisions").len(), 1);
    assert!(wire["waitingOnDecision"].is_object());
}

#[test]
fn a_completion_blocked_by_an_open_decision_crosses_as_its_own_code() {
    // REVIEW-B1c F1. The rail must be able to say *why* a completion is not
    // canonical, and "blocked by an open ruling" is a different sentence from
    // "its approvals are incomplete".
    let founder = fixed_keys(0x33);
    let actor = fixed_keys(0x44);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment_event.id.to_hex();
    let report_event = signed(&report(&assignment_id), &actor, 2);
    let disposition_event = signed(
        &disposition(&assignment_id, &report_event.id.to_hex()),
        &founder,
        3,
    );
    let acknowledgement_event = signed(&acknowledgement(&disposition_event.id.to_hex()), &actor, 4);
    let request_event = signed(
        &decision_request("founder", vec![assignment_id.clone()]),
        &actor,
        5,
    );
    let completed_event = signed(&completed(&assignment_id), &founder, 6);
    let completed_id = completed_event.id.to_hex();

    let mut seated = request(
        &founder,
        &[
            assignment_event,
            report_event,
            disposition_event,
            acknowledgement_event,
            request_event,
            completed_event,
        ],
    );
    // The asker must hold a seat, or its request is `Unauthorized` and blocks
    // nothing — which would make this test pass for the wrong reason.
    seated
        .context
        .active_seats
        .push(CodingSessionTeamActiveSeatInput {
            actor_pubkey: actor.public_key().to_hex(),
            role: "builder".into(),
            grant_event_ref: id("de"),
        });
    let response = fold_adapter(seated).expect("canonical fold");

    assert!(response.canonical_terminal.is_none());
    let exclusion = response
        .excluded
        .iter()
        .find(|item| item.event_id == completed_id)
        .expect("the completion is excluded");
    assert_eq!(
        exclusion.code,
        CodingSessionTeamFoldAdapterExclusionCode::CompletionBlockedByOpenDecision
    );
    let wire = serde_json::to_value(&response).expect("serialize response");
    let code = wire["excluded"]
        .as_array()
        .expect("excluded")
        .iter()
        .find(|item| item["eventId"] == completed_id)
        .expect("excluded row")["code"]
        .clone();
    assert_eq!(code, "completion_blocked_by_open_decision");
    assert!(wire["waitingOnDecision"].is_object());
}

/// **Item G, live run 3.** A verifier reported FAIL on the wire and the
/// completion was structurally unaffected. With `verifierRequired` set, a
/// completion that settled on a report no active verifier ruled on crosses the
/// adapter boundary as its own code — not as `completion_not_approved`, which
/// would send a reader to fix the approval chain that is already complete.
#[test]
fn a_completion_missing_its_verifier_crosses_as_its_own_code() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment.id.to_hex();
    let report = signed(&report(&assignment_id), &actor, 2);
    let report_id = report.id.to_hex();
    let disposition = signed(&disposition(&assignment_id, &report_id), &founder, 3);
    let acknowledgement = signed(&acknowledgement(&disposition.id.to_hex()), &actor, 4);
    let completed = signed(
        &payload(CodingSessionTeamTransactionBody::MissionCompleted(
            CodingSessionTeamMissionCompleted {
                assignment_refs: vec![assignment_id.clone()],
                landed_shas: Vec::new(),
                summary: "Complete".into(),
                follow_ups: Vec::new(),
            },
        )),
        &founder,
        5,
    );
    let completed_id = completed.id.to_hex();
    let events = vec![assignment, report, disposition, acknowledgement, completed];

    // Without the flag — every Desktop fold until L8 lands — it folds as today.
    let today = fold_adapter(request(&founder, &events)).expect("canonical fold");
    assert_eq!(
        today
            .canonical_terminal
            .as_ref()
            .map(|terminal| terminal.event_id.as_str()),
        Some(completed_id.as_str())
    );

    let mut gated = request(&founder, &events);
    gated.context.verifier_required = true;
    let response = fold_adapter(gated).expect("canonical fold");
    assert!(response.canonical_terminal.is_none());
    assert_eq!(
        response
            .excluded
            .iter()
            .find(|item| item.event_id == completed_id)
            .map(|item| item.code),
        Some(CodingSessionTeamFoldAdapterExclusionCode::CompletionNotVerified)
    );
    let wire = serde_json::to_value(&response).expect("serialize response");
    let code = wire["excluded"]
        .as_array()
        .expect("excluded")
        .iter()
        .find(|item| item["eventId"] == completed_id)
        .expect("excluded row")["code"]
        .clone();
    assert_eq!(code, "completion_not_verified");
}
