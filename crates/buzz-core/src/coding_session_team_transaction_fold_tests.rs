use super::*;
use std::collections::HashMap;

use crate::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDecisionAnswer, CodingSessionTeamDecisionChoice,
    CodingSessionTeamDecisionRequest, CodingSessionTeamDispositionDecision,
    CodingSessionTeamMissionBlocked, CodingSessionTeamMissionCompleted, CodingSessionTeamNote,
    CodingSessionTeamRefutationDecision, CodingSessionTeamReport,
    CODING_SESSION_TEAM_DECISION_FOUNDER, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const OTHER_CHANNEL: &str = "15ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn id(byte: &str) -> String {
    byte.repeat(32)
}

fn context(founder: &Keys, seats: Vec<(&Keys, &str)>) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: seats
            .into_iter()
            .map(|(keys, role)| CodingSessionTeamActiveSeat {
                actor_pubkey: keys.public_key().to_hex(),
                role: role.into(),
            })
            .collect(),
        active_grants: Vec::new(),
        // Every fixture in this file predates `gates.verifierRequired`, and
        // `false` is what a session with no policy folds under: these tests
        // are the proof the new pass changes nothing there.
        verifier_required: false,
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
            objective: "Build the protocol".into(),
            brief: "Implement the bounded assigned slice.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["crates/buzz-core/src".into()],
            acceptance_steps: vec!["cargo test -p buzz-core".into()],
        },
    ))
}

fn report(assignment_ref: &str, summary: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: assignment_ref.into(),
            summary: summary.into(),
            branch: None,
            base_sha: None,
            head_sha: None,
            files: Vec::new(),
            tests: Vec::new(),
            red_before_green: None,
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        },
    ))
}

fn refutation(assignment_ref: &str, report_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Refutation {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            decision: CodingSessionTeamRefutationDecision::NotRefuted,
            summary: "No refutation found".into(),
            findings: Vec::new(),
            required_action: None,
        },
    ))
}

fn disposition(
    assignment_ref: &str,
    report_ref: &str,
    refutation_ref: Option<String>,
    decision: CodingSessionTeamDispositionDecision,
) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            refutation_ref,
            decision,
            summary: "Governed".into(),
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

fn blocked(summary: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionBlocked(
        CodingSessionTeamMissionBlocked {
            assignment_refs: Vec::new(),
            summary: summary.into(),
            blockers: vec!["External dependency".into()],
            held_on: None,
            required_action: "Resolve it".into(),
        },
    ))
}

fn completed(assignment_ref: &str, summary: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionCompleted(
        CodingSessionTeamMissionCompleted {
            assignment_refs: vec![assignment_ref.into()],
            landed_shas: Vec::new(),
            summary: summary.into(),
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
        serde_json::to_string(payload).unwrap(),
    )
    .tags([
        Tag::parse(["h", channel]).unwrap(),
        Tag::parse(["d", payload.session_ref.as_str()]).unwrap(),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).unwrap(),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).unwrap(),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).unwrap(),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .unwrap()
}

#[test]
fn approval_requires_disposition_and_assigned_actor_acknowledgement() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let verifier = Keys::generate();
    let context = context(&founder, vec![(&verifier, "verifier")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex(), "Done"), &actor, 2);
    let refutation = signed(
        &refutation(&assignment.id.to_hex(), &report.id.to_hex()),
        &verifier,
        3,
    );
    let disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            Some(refutation.id.to_hex()),
            CodingSessionTeamDispositionDecision::ApproveWithNotes,
        ),
        &founder,
        4,
    );
    let acknowledgement = signed(&acknowledgement(&disposition.id.to_hex()), &actor, 5);
    let completed = signed(
        &payload(CodingSessionTeamTransactionBody::MissionCompleted(
            CodingSessionTeamMissionCompleted {
                assignment_refs: vec![assignment.id.to_hex()],
                landed_shas: vec!["1".repeat(40)],
                summary: "Complete".into(),
                follow_ups: Vec::new(),
            },
        )),
        &founder,
        6,
    );

    let fold = fold_coding_session_team_transactions(
        &[
            assignment,
            report,
            refutation,
            disposition,
            acknowledgement,
            completed.clone(),
        ],
        &context,
    )
    .unwrap();
    assert!(fold.assignments[0].settled);
    assert_eq!(
        fold.canonical_terminal.unwrap().event_id,
        completed.id.to_hex()
    );
    assert!(fold.excluded.is_empty());
}

#[test]
fn nonapproval_or_missing_assigned_actor_ack_cannot_complete() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let context = context(&founder, Vec::new());
    let assignment = signed(&assignment(&actor), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex(), "Done"), &actor, 2);
    let disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::ChangesRequested,
        ),
        &founder,
        3,
    );
    let wrong_ack = signed(&acknowledgement(&disposition.id.to_hex()), &stranger, 4);
    let completed = signed(
        &payload(CodingSessionTeamTransactionBody::MissionCompleted(
            CodingSessionTeamMissionCompleted {
                assignment_refs: vec![assignment.id.to_hex()],
                landed_shas: Vec::new(),
                summary: "Premature".into(),
                follow_ups: Vec::new(),
            },
        )),
        &founder,
        5,
    );
    let fold = fold_coding_session_team_transactions(
        &[assignment, report, disposition, wrong_ack, completed],
        &context,
    )
    .unwrap();
    assert!(!fold.assignments[0].settled);
    assert!(fold.canonical_terminal.is_none());
    assert!(fold
        .excluded
        .iter()
        .any(|item| { item.code == CodingSessionTeamFoldExclusionCode::CompletionNotApproved }));
    assert!(fold
        .excluded
        .iter()
        .any(|item| item.code == CodingSessionTeamFoldExclusionCode::Unauthorized));
}

#[test]
fn parallel_reports_remain_facts_and_only_governed_report_settles() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let assignment = signed(&assignment(&actor), &founder, 1);
    let first = signed(&report(&assignment.id.to_hex(), "First"), &actor, 2);
    let second = signed(&report(&assignment.id.to_hex(), "Second"), &actor, 3);
    let disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &second.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        4,
    );
    let ack = signed(&acknowledgement(&disposition.id.to_hex()), &actor, 5);
    let fold = fold_coding_session_team_transactions(
        &[assignment, first.clone(), second.clone(), disposition, ack],
        &context,
    )
    .unwrap();
    assert!(fold.included_event_ids.contains(&first.id.to_hex()));
    assert!(fold.included_event_ids.contains(&second.id.to_hex()));
    assert_eq!(
        fold.assignments[0].governed_report_event_id,
        Some(second.id.to_hex())
    );
}

#[test]
fn rejects_cross_context_references() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let cross_channel = signed_in_channel(&assignment(&actor), &founder, 2, OTHER_CHANNEL);
    assert!(
        fold_coding_session_team_transactions(&[cross_channel], &context)
            .unwrap_err()
            .contains("channel")
    );

    let mut cross_session_payload = assignment(&actor);
    cross_session_payload.session_ref = "6b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into();
    let cross_session = signed(&cross_session_payload, &founder, 2);
    assert!(
        fold_coding_session_team_transactions(&[cross_session], &context)
            .unwrap_err()
            .contains("session")
    );

    let mut cross_genesis_payload = assignment(&actor);
    cross_genesis_payload.genesis_ref = id("bc");
    let cross_genesis = signed(&cross_genesis_payload, &founder, 2);
    assert!(
        fold_coding_session_team_transactions(&[cross_genesis], &context)
            .unwrap_err()
            .contains("genesis")
    );
}

#[test]
fn correction_fork_uses_timestamp_then_event_id_and_discloses_conflict() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let original = signed(&assignment(&actor), &founder, 1);
    let mut left_payload = assignment(&actor);
    left_payload.supersedes = Some(original.id.to_hex());
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut left_payload.body {
        body.objective = "Left correction".into();
    }
    let left = signed(&left_payload, &founder, 2);
    let mut right_payload = assignment(&actor);
    right_payload.supersedes = Some(original.id.to_hex());
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut right_payload.body {
        body.objective = "Right correction".into();
    }
    let right = signed(&right_payload, &founder, 2);
    let expected = std::cmp::max(left.id.to_hex(), right.id.to_hex());
    let fold = fold_coding_session_team_transactions(&[original, left, right], &context).unwrap();
    assert_eq!(fold.conflicts.len(), 1);
    assert_eq!(fold.conflicts[0].winner_event_id, expected);
    assert_eq!(fold.assignments[0].assignment_event_id, expected);
}

#[test]
// Fix round 2: both shapes were hard errors until 2026-09-01. They are now
// exclusions of the correcting record alone, with the same diagnostic text
// carried in `reason` instead of in a whole-fold `Err`.
fn excludes_correction_subject_and_verdict_subtype_changes() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let verifier = Keys::generate();
    let context = context(&founder, vec![(&verifier, "verifier")]);
    let original = signed(&assignment(&actor), &founder, 1);
    let mut changed = assignment(&Keys::generate());
    changed.supersedes = Some(original.id.to_hex());
    let changed = signed(&changed, &founder, 2);
    let original_id = original.id.to_hex();
    let changed_id = changed.id.to_hex();
    let fold = fold_coding_session_team_transactions(&[original, changed], &context).unwrap();
    assert_eq!(fold.included_event_ids, vec![original_id]);
    assert_eq!(
        exclusion_code_string(&fold, &changed_id),
        "InvalidCorrection"
    );
    assert!(exclusion_reason(&fold, &changed_id).contains("logical subject"));

    let assignment = signed(&assignment(&actor), &founder, 3);
    let report = signed(&report(&assignment.id.to_hex(), "Done"), &actor, 4);
    let refutation = signed(
        &refutation(&assignment.id.to_hex(), &report.id.to_hex()),
        &verifier,
        5,
    );
    let mut changed = disposition(
        &assignment.id.to_hex(),
        &report.id.to_hex(),
        None,
        CodingSessionTeamDispositionDecision::Approve,
    );
    changed.supersedes = Some(refutation.id.to_hex());
    let changed = signed(&changed, &verifier, 6);
    let refutation_id = refutation.id.to_hex();
    let changed_id = changed.id.to_hex();
    let fold =
        fold_coding_session_team_transactions(&[assignment, report, refutation, changed], &context)
            .unwrap();
    assert_eq!(
        exclusion_code_string(&fold, &changed_id),
        "InvalidCorrection"
    );
    assert!(exclusion_reason(&fold, &changed_id).contains("subtype"));
    // The refutation it tried to correct keeps its place.
    assert!(fold.included_event_ids.contains(&refutation_id));
}

#[test]
fn active_steering_grant_qualifies_operator_but_nonsteering_grant_does_not() {
    let founder = Keys::generate();
    let operator = Keys::generate();
    let actor = Keys::generate();
    let mut context = context(&founder, Vec::new());
    context.active_grants.push(CodingSessionTeamActiveGrant {
        actor_pubkey: operator.public_key().to_hex(),
        grant_event_ref: id("44"),
        may_steer: false,
    });
    let assignment = signed(&assignment(&actor), &operator, 1);
    let fold =
        fold_coding_session_team_transactions(std::slice::from_ref(&assignment), &context).unwrap();
    assert_eq!(
        fold.excluded[0].code,
        CodingSessionTeamFoldExclusionCode::Unauthorized
    );
    context.active_grants[0].may_steer = true;
    let fold = fold_coding_session_team_transactions(&[assignment], &context).unwrap();
    assert!(fold.excluded.is_empty());
}

#[test]
fn terminal_conflict_chooses_newest_by_timestamp_then_id() {
    let founder = Keys::generate();
    let context = context(&founder, Vec::new());
    let older = signed(&blocked("Older"), &founder, 1);
    let newer_left = signed(&blocked("Newer left"), &founder, 2);
    let newer_right = signed(&blocked("Newer right"), &founder, 2);
    let expected = std::cmp::max(newer_left.id.to_hex(), newer_right.id.to_hex());
    let fold =
        fold_coding_session_team_transactions(&[older, newer_left, newer_right], &context).unwrap();
    assert_eq!(fold.canonical_terminal.unwrap().event_id, expected);
    assert!(fold.conflicts.iter().any(|item| item.subject == "terminal"));
    assert_eq!(
        fold.excluded
            .iter()
            .filter(|item| item.code == CodingSessionTeamFoldExclusionCode::TerminalConflict)
            .count(),
        2
    );
}

#[test]
fn cycle_detector_rejects_a_supplied_reference_cycle() {
    let keys = Keys::generate();
    let first_event = signed(&blocked("First"), &keys, 1);
    let second_event = signed(&blocked("Second"), &keys, 2);
    let first_id = first_event.id.to_hex();
    let second_id = second_event.id.to_hex();
    let mut first_payload = blocked("First");
    first_payload.supersedes = Some(second_id.clone());
    let mut second_payload = blocked("Second");
    second_payload.supersedes = Some(first_id.clone());
    let records = vec![
        Record {
            event: &first_event,
            id: first_id.clone(),
            author: keys.public_key().to_hex(),
            payload: first_payload,
        },
        Record {
            event: &second_event,
            id: second_id.clone(),
            author: keys.public_key().to_hex(),
            payload: second_payload,
        },
    ];
    let by_id = HashMap::from([(first_id, 0), (second_id, 1)]);
    assert!(reject_cycles(&records, &by_id)
        .unwrap_err()
        .contains("cycle"));
}

#[test]
fn unauthorized_self_assignment_cannot_authorize_its_report_or_dependents() {
    let founder = Keys::generate();
    let attacker = Keys::generate();
    let verifier = Keys::generate();
    let context = context(&founder, vec![(&verifier, "verifier")]);
    let assignment = signed(&assignment(&attacker), &attacker, 1);
    let report = signed(
        &report(&assignment.id.to_hex(), "Self report"),
        &attacker,
        2,
    );
    let refutation = signed(
        &refutation(&assignment.id.to_hex(), &report.id.to_hex()),
        &verifier,
        3,
    );
    let disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            Some(refutation.id.to_hex()),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        4,
    );
    let acknowledgement = signed(&acknowledgement(&disposition.id.to_hex()), &attacker, 5);
    let ids = [
        assignment.id.to_hex(),
        report.id.to_hex(),
        refutation.id.to_hex(),
        disposition.id.to_hex(),
        acknowledgement.id.to_hex(),
    ];
    let fold = fold_coding_session_team_transactions(
        &[assignment, report, refutation, disposition, acknowledgement],
        &context,
    )
    .unwrap();

    assert!(fold.included_event_ids.is_empty());
    assert_eq!(
        exclusion_code(&fold, &ids[0]),
        CodingSessionTeamFoldExclusionCode::Unauthorized
    );
    assert_eq!(
        exclusion_code(&fold, &ids[1]),
        CodingSessionTeamFoldExclusionCode::DependentOnUnauthorized
    );
    assert_eq!(
        exclusion_code(&fold, &ids[2]),
        CodingSessionTeamFoldExclusionCode::DependentOnUnauthorized
    );
    assert_eq!(
        exclusion_code(&fold, &ids[3]),
        CodingSessionTeamFoldExclusionCode::DependentOnUnauthorized
    );
    assert_eq!(
        exclusion_code(&fold, &ids[4]),
        CodingSessionTeamFoldExclusionCode::DependentOnExcluded
    );
}

#[test]
fn dependency_on_a_superseded_assignment_is_excluded_by_stable_code() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let original = signed(&assignment(&actor), &founder, 1);
    let mut correction_payload = assignment(&actor);
    correction_payload.supersedes = Some(original.id.to_hex());
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut correction_payload.body {
        body.objective = "Corrected objective".into();
    }
    let correction = signed(&correction_payload, &founder, 2);
    let report = signed(&report(&original.id.to_hex(), "Old assignment"), &actor, 3);
    let original_id = original.id.to_hex();
    let correction_id = correction.id.to_hex();
    let report_id = report.id.to_hex();
    let fold =
        fold_coding_session_team_transactions(&[original, correction, report], &context).unwrap();

    assert!(fold.included_event_ids.contains(&correction_id));
    assert_eq!(
        exclusion_code(&fold, &original_id),
        CodingSessionTeamFoldExclusionCode::Superseded
    );
    assert_eq!(
        exclusion_code(&fold, &report_id),
        CodingSessionTeamFoldExclusionCode::DependentOnSuperseded
    );
}

#[test]
fn invalid_signature_is_rejected_before_authority_projection() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let event = signed(&assignment(&actor), &founder, 1);
    let mut json: serde_json::Value = serde_json::from_str(&event.as_json()).unwrap();
    json["sig"] = serde_json::Value::String("0".repeat(128));
    let tampered = Event::from_json(json.to_string()).unwrap();
    assert!(fold_coding_session_team_transactions(&[tampered], &context)
        .unwrap_err()
        .contains("signature"));
}

#[test]
fn multi_link_correction_chain_selects_only_the_last_valid_head() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let original = signed(&assignment(&actor), &founder, 1);
    let mut middle_payload = assignment(&actor);
    middle_payload.supersedes = Some(original.id.to_hex());
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut middle_payload.body {
        body.objective = "Middle".into();
    }
    let middle = signed(&middle_payload, &founder, 2);
    let mut head_payload = assignment(&actor);
    head_payload.supersedes = Some(middle.id.to_hex());
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut head_payload.body {
        body.objective = "Head".into();
    }
    let head = signed(&head_payload, &founder, 3);
    let original_id = original.id.to_hex();
    let middle_id = middle.id.to_hex();
    let head_id = head.id.to_hex();
    let fold = fold_coding_session_team_transactions(&[original, middle, head], &context).unwrap();

    assert_eq!(fold.assignments[0].assignment_event_id, head_id);
    assert_eq!(
        exclusion_code(&fold, &original_id),
        CodingSessionTeamFoldExclusionCode::Superseded
    );
    assert_eq!(
        exclusion_code(&fold, &middle_id),
        CodingSessionTeamFoldExclusionCode::Superseded
    );
}

#[test]
fn acknowledgement_conflict_is_disclosed_and_uses_deterministic_winner() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let assignment = signed(&assignment(&actor), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex(), "Done"), &actor, 2);
    let disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let left = signed(&acknowledgement(&disposition.id.to_hex()), &actor, 4);
    let mut right_payload = acknowledgement(&disposition.id.to_hex());
    if let CodingSessionTeamTransactionBody::Acknowledgement(body) = &mut right_payload.body {
        body.note = Some("Received again".into());
    }
    let right = signed(&right_payload, &actor, 4);
    let expected = std::cmp::max(left.id.to_hex(), right.id.to_hex());
    let subject = format!("acknowledgement:{}", disposition.id.to_hex());
    let fold = fold_coding_session_team_transactions(
        &[assignment, report, disposition, left, right],
        &context,
    )
    .unwrap();

    let conflict = fold
        .conflicts
        .iter()
        .find(|conflict| conflict.subject == subject)
        .unwrap();
    assert_eq!(conflict.winner_event_id, expected);
    assert_eq!(fold.assignments[0].acknowledgement_event_id, Some(expected));
}

#[test]
fn governance_conflict_is_disclosed_without_erasing_parallel_reports() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let assignment = signed(&assignment(&actor), &founder, 1);
    let first_report = signed(&report(&assignment.id.to_hex(), "First"), &actor, 2);
    let second_report = signed(&report(&assignment.id.to_hex(), "Second"), &actor, 3);
    let first_disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &first_report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        4,
    );
    let second_disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &second_report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::ApproveWithNotes,
        ),
        &founder,
        5,
    );
    let first_ack = signed(&acknowledgement(&first_disposition.id.to_hex()), &actor, 6);
    let second_ack = signed(&acknowledgement(&second_disposition.id.to_hex()), &actor, 7);
    let subject = format!("governance:{}", assignment.id.to_hex());
    let second_report_id = second_report.id.to_hex();
    let second_disposition_id = second_disposition.id.to_hex();
    let fold = fold_coding_session_team_transactions(
        &[
            assignment,
            first_report.clone(),
            second_report,
            first_disposition,
            second_disposition,
            first_ack,
            second_ack,
        ],
        &context,
    )
    .unwrap();

    assert!(fold.included_event_ids.contains(&first_report.id.to_hex()));
    assert!(fold.included_event_ids.contains(&second_report_id));
    assert_eq!(
        fold.assignments[0].disposition_event_id,
        Some(second_disposition_id.clone())
    );
    assert_eq!(
        fold.conflicts
            .iter()
            .find(|conflict| conflict.subject == subject)
            .unwrap()
            .winner_event_id,
        second_disposition_id
    );
}

#[test]
fn valid_correction_cannot_skip_an_invalid_correction_parent() {
    let founder = Keys::generate();
    let first_actor = Keys::generate();
    let second_actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let unapproved_assignment = signed(&assignment(&first_actor), &founder, 1);
    let approved_assignment = signed(&assignment(&second_actor), &founder, 2);
    let report = signed(
        &report(&approved_assignment.id.to_hex(), "Approved work"),
        &second_actor,
        3,
    );
    let disposition = signed(
        &disposition(
            &approved_assignment.id.to_hex(),
            &report.id.to_hex(),
            None,
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        4,
    );
    let acknowledgement = signed(&acknowledgement(&disposition.id.to_hex()), &second_actor, 5);
    let invalid_parent = signed(
        &completed(&unapproved_assignment.id.to_hex(), "Premature"),
        &founder,
        6,
    );
    let mut correction_payload = completed(&approved_assignment.id.to_hex(), "Would be valid");
    correction_payload.supersedes = Some(invalid_parent.id.to_hex());
    let correction = signed(&correction_payload, &founder, 7);
    let invalid_parent_id = invalid_parent.id.to_hex();
    let correction_id = correction.id.to_hex();
    let fold = fold_coding_session_team_transactions(
        &[
            unapproved_assignment,
            approved_assignment,
            report,
            disposition,
            acknowledgement,
            invalid_parent,
            correction,
        ],
        &context,
    )
    .unwrap();

    assert_eq!(
        exclusion_code(&fold, &invalid_parent_id),
        CodingSessionTeamFoldExclusionCode::CompletionNotApproved
    );
    assert_eq!(
        exclusion_code(&fold, &correction_id),
        CodingSessionTeamFoldExclusionCode::DependentOnExcluded
    );
    assert!(fold.canonical_terminal.is_none());
}

fn exclusion_code(
    fold: &CodingSessionTeamFold,
    event_id: &str,
) -> CodingSessionTeamFoldExclusionCode {
    fold.excluded
        .iter()
        .find(|item| item.event_id == event_id)
        .map(|item| item.code)
        .unwrap()
}

/// The exclusion code exactly as `fold_json` and the Tauri adapter print it,
/// so these tests pin the wire string and not only the variant.
fn exclusion_code_string(fold: &CodingSessionTeamFold, event_id: &str) -> String {
    format!("{:?}", exclusion_code(fold, event_id))
}

fn exclusion_reason(fold: &CodingSessionTeamFold, event_id: &str) -> String {
    fold.excluded
        .iter()
        .find(|item| item.event_id == event_id)
        .map(|item| item.reason.clone())
        .unwrap()
}

// ── Unseated-report disclosure (batch 2026-09-01, §1d) ───────────────────────
//
// Report inclusion is assignee-equality by design: the assignment names a
// target, not an authorship claim. The seat is a separate fact, and before
// this the fold kept it to itself while the CLI told leads the opposite.

#[test]
fn an_included_report_from_an_unseated_author_is_disclosed_rather_than_excluded() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    // No seat at all for the assignee: exactly the 2026-08-28 hired builder.
    let context = context(&founder, Vec::new());
    let assignment = signed(&assignment(&actor), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex(), "Done"), &actor, 2);
    let assignment_id = assignment.id.to_hex();
    let report_id = report.id.to_hex();

    let fold = fold_coding_session_team_transactions(&[assignment, report], &context).unwrap();

    assert!(fold.included_event_ids.contains(&report_id));
    assert!(fold.excluded.iter().all(|item| item.event_id != report_id));
    assert_eq!(
        fold.unseated_reports,
        vec![CodingSessionTeamUnseatedReport {
            event_id: report_id,
            author_pubkey: actor.public_key().to_hex(),
            assignment_ref: assignment_id,
            assignee_role: "builder".into(),
        }]
    );
}

#[test]
fn a_seated_report_author_is_never_disclosed_as_unseated() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let seated = context(&founder, vec![(&actor, "builder")]);
    // A seat for the wrong role is not a seat for this assignment's role.
    let wrong_role = context(&founder, vec![(&actor, "verifier")]);
    let assignment_event = signed(&assignment(&actor), &founder, 1);
    let report_event = signed(&report(&assignment_event.id.to_hex(), "Done"), &actor, 2);
    let report_id = report_event.id.to_hex();
    let events = [assignment_event, report_event];

    let fold = fold_coding_session_team_transactions(&events, &seated).unwrap();
    assert!(fold.included_event_ids.contains(&report_id));
    assert!(fold.unseated_reports.is_empty());

    let fold = fold_coding_session_team_transactions(&events, &wrong_role).unwrap();
    assert_eq!(fold.unseated_reports.len(), 1);
    assert_eq!(fold.unseated_reports[0].assignee_role, "builder");
}

#[test]
fn a_report_excluded_for_another_reason_is_never_listed_as_unseated() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let context = context(&founder, Vec::new());
    let assignment = signed(&assignment(&actor), &founder, 1);
    // Authored by somebody the assignment never named: excluded, unauthorized.
    let report = signed(&report(&assignment.id.to_hex(), "Done"), &stranger, 2);
    let report_id = report.id.to_hex();

    let fold = fold_coding_session_team_transactions(&[assignment, report], &context).unwrap();

    assert_eq!(
        exclusion_code(&fold, &report_id),
        CodingSessionTeamFoldExclusionCode::Unauthorized
    );
    assert!(!fold.included_event_ids.contains(&report_id));
    assert!(fold.unseated_reports.is_empty());
}

#[test]
fn unseated_disclosure_follows_included_order_and_is_input_order_independent() {
    let founder = Keys::generate();
    let first_actor = Keys::generate();
    let second_actor = Keys::generate();
    let context = context(&founder, Vec::new());
    let first_assignment = signed(&assignment(&first_actor), &founder, 1);
    let second_assignment = signed(&assignment(&second_actor), &founder, 2);
    let first_report = signed(
        &report(&first_assignment.id.to_hex(), "First"),
        &first_actor,
        3,
    );
    let second_report = signed(
        &report(&second_assignment.id.to_hex(), "Second"),
        &second_actor,
        4,
    );
    let events = vec![
        first_assignment,
        second_assignment,
        first_report,
        second_report,
    ];

    let forward = fold_coding_session_team_transactions(&events, &context).unwrap();
    let reversed: Vec<Event> = events.iter().cloned().rev().collect();
    let backward = fold_coding_session_team_transactions(&reversed, &context).unwrap();

    assert_eq!(forward.unseated_reports, backward.unseated_reports);
    assert_eq!(forward.unseated_reports.len(), 2);
    let disclosed: Vec<&String> = forward
        .unseated_reports
        .iter()
        .map(|item| &item.event_id)
        .collect();
    let included: Vec<&String> = forward
        .included_event_ids
        .iter()
        .filter(|id| disclosed.contains(id))
        .collect();
    assert_eq!(disclosed, included);
}

// --- B1c: `note` and `decision.*` in the fold.

fn note(text: &str, refs: Vec<String>) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Note(
        CodingSessionTeamNote {
            text: text.into(),
            refs,
        },
    ))
}

fn decision_request(
    question: &str,
    held_on: &str,
    blocks: Vec<String>,
) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::DecisionRequest(
        CodingSessionTeamDecisionRequest {
            question: question.into(),
            options: vec!["now".into(), "after the rebuild".into()],
            held_on: held_on.into(),
            blocks,
            recommendation: None,
        },
    ))
}

fn decision_answer(request_ref: &str, index: u32) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::DecisionAnswer(
        CodingSessionTeamDecisionAnswer {
            request_ref: request_ref.into(),
            choice: CodingSessionTeamDecisionChoice::Index(index),
            note: None,
            condition: None,
        },
    ))
}

// The regression suites for the four 2026-09-01 fix rounds, and the fuzz,
// live in sibling files so no file here passes 1,000 lines (REVIEW-B1b R6).
// They are children of this module, so every fixture helper above is in
// scope unchanged.
#[path = "coding_session_team_transaction_fold_regression_tests.rs"]
mod regression;

#[path = "coding_session_team_transaction_fold_fuzz_tests.rs"]
mod fuzz;
