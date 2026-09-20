//! Ledger 183: the pending completion and the named missing link.
//!
//! A separate file from `coding_session_team_transaction_fold_tests.rs`
//! because that one is 993 lines and this repository splits rather than raises
//! its 1,000-line ceiling.

use super::*;

use crate::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDecisionRequest,
    CodingSessionTeamDispositionDecision, CodingSessionTeamMissionCompleted,
    CodingSessionTeamReport, CODING_SESSION_TEAM_DECISION_FOUNDER,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "ab".repeat(32)
}

fn context(founder: &Keys, seats: Vec<(&Keys, &str)>) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: genesis(),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: seats
            .into_iter()
            .map(|(keys, role)| CodingSessionTeamActiveSeat {
                actor_pubkey: keys.public_key().to_hex(),
                role: role.into(),
            })
            .collect(),
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

fn signed(
    payload: &CodingSessionTeamTransactionPayload,
    keys: &Keys,
    created_at: u64,
) -> nostr::Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).unwrap(),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).unwrap(),
        Tag::parse(["d", SESSION]).unwrap(),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).unwrap(),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).unwrap(),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).unwrap(),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .unwrap()
}

fn assignment(actor: &Keys) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Assignment(
        CodingSessionTeamAssignment {
            assignee_actor: actor.public_key().to_hex(),
            assignee_role: "builder".into(),
            objective: "Build the slice".into(),
            brief: "Implement the bounded assigned slice.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["crates/buzz-core/src".into()],
            acceptance_steps: vec!["cargo test -p buzz-core".into()],
        },
    ))
}

fn report(assignment_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: assignment_ref.into(),
            summary: "Done".into(),
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

fn disposition(
    assignment_ref: &str,
    report_ref: &str,
    decision: CodingSessionTeamDispositionDecision,
) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            refutation_ref: None,
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

fn completed(assignment_ref: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionCompleted(
        CodingSessionTeamMissionCompleted {
            assignment_refs: vec![assignment_ref.into()],
            landed_shas: Vec::new(),
            summary: "Mission finished".into(),
            follow_ups: Vec::new(),
        },
    ))
}

/// The exact shape of ledger 179(a): the lead publishes the completion while
/// the acknowledgement is still in flight, its turn ends, and the last
/// acknowledgement arrives with nobody awake.
#[test]
fn a_pending_completion_becomes_terminal_when_its_acknowledgement_arrives() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let report = signed(&report(&assignment.id.to_hex()), &actor, 2);
    let disposition = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let completion = signed(&completed(&assignment.id.to_hex()), &founder, 4);
    let before = [
        assignment.clone(),
        report.clone(),
        disposition.clone(),
        completion.clone(),
    ];
    let fold = fold_coding_session_team_transactions(&before, &context).unwrap();

    // Published, durable, and held back rather than refused.
    assert!(fold.canonical_terminal.is_none());
    let pending = fold
        .pending_completion
        .as_ref()
        .expect("a pending completion");
    assert_eq!(pending.event_id, completion.id.to_hex());
    assert_eq!(
        pending.code,
        CodingSessionTeamFoldExclusionCode::CompletionNotApproved
    );
    assert_eq!(
        pending.unsettled_assignment_event_ids,
        vec![assignment.id.to_hex()]
    );
    assert!(completion_exclusion_is_pending(pending.code));

    // The acknowledgement arrives. Nothing else is published, no turn is
    // opened, and the same completion is now the session's terminal.
    let acknowledgement = signed(&acknowledgement(&disposition.id.to_hex()), &actor, 5);
    let after = [
        assignment.clone(),
        report,
        disposition,
        completion.clone(),
        acknowledgement,
    ];
    let settled = fold_coding_session_team_transactions(&after, &context).unwrap();
    assert_eq!(
        settled
            .canonical_terminal
            .as_ref()
            .map(|terminal| terminal.event_id.as_str()),
        Some(completion.id.to_hex().as_str())
    );
    assert!(settled.pending_completion.is_none());
    assert!(settled.assignments[0].settled);
    assert!(settled.assignments[0].awaiting.is_none());
    assert!(settled.excluded.is_empty());
}

/// Ledger 178(e): the nulls used to be the whole diagnosis, and the lead read
/// them as a missing refutation. Each missing link now names itself.
#[test]
fn every_missing_link_names_itself_and_the_party_who_owes_it() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment.id.to_hex();

    // 1. Nothing has answered the assignment.
    let fold =
        fold_coding_session_team_transactions(std::slice::from_ref(&assignment), &context).unwrap();
    let awaiting = fold.assignments[0].awaiting.as_ref().expect("awaiting");
    assert_eq!(awaiting.link, CodingSessionTeamSettlementLink::Report);
    assert_eq!(awaiting.link.as_str(), "report");
    assert_eq!(awaiting.owed_by_role, "builder");
    assert_eq!(
        awaiting.owed_by_actor.as_deref(),
        Some(actor.public_key().to_hex().as_str())
    );

    // 2. The report is in and nobody has ruled on it. No single party owes a
    //    disposition, so no actor is named.
    let report = signed(&report(&assignment_id), &actor, 2);
    let fold =
        fold_coding_session_team_transactions(&[assignment.clone(), report.clone()], &context)
            .unwrap();
    let awaiting = fold.assignments[0].awaiting.as_ref().expect("awaiting");
    assert_eq!(awaiting.link, CodingSessionTeamSettlementLink::Disposition);
    assert_eq!(awaiting.owed_by_role, "lead");
    assert_eq!(awaiting.owed_by_actor, None);

    // 3. A ruling that is not an approval leaves the chain exactly there.
    let changes = signed(
        &disposition(
            &assignment_id,
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::ChangesRequested,
        ),
        &founder,
        3,
    );
    let fold = fold_coding_session_team_transactions(
        &[assignment.clone(), report.clone(), changes],
        &context,
    )
    .unwrap();
    assert_eq!(
        fold.assignments[0].awaiting.as_ref().map(|item| item.link),
        Some(CodingSessionTeamSettlementLink::Disposition)
    );

    // 4. The approval is in and the assignee has not acknowledged it — the
    //    state ledger 178(e) rendered as three nulls and no sentence.
    let approve = signed(
        &disposition(
            &assignment_id,
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        4,
    );
    let fold =
        fold_coding_session_team_transactions(&[assignment, report, approve], &context).unwrap();
    let settlement = &fold.assignments[0];
    assert!(!settlement.settled);
    assert_eq!(settlement.disposition_event_id, None);
    let awaiting = settlement.awaiting.as_ref().expect("awaiting");
    assert_eq!(
        awaiting.link,
        CodingSessionTeamSettlementLink::Acknowledgement
    );
    assert_eq!(awaiting.link.as_str(), "acknowledgement");
    assert_eq!(awaiting.owed_by_role, "builder");
    assert_eq!(
        awaiting.owed_by_actor.as_deref(),
        Some(actor.public_key().to_hex().as_str())
    );
}

/// A completion held on an unanswered question is also a wait, and says which
/// question through `waiting_on_decision` rather than through this field.
#[test]
fn a_completion_blocked_by_an_open_ruling_is_pending_not_rejected() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let assignment_id = assignment.id.to_hex();
    let report = signed(&report(&assignment_id), &actor, 2);
    let disposition = signed(
        &disposition(
            &assignment_id,
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        3,
    );
    let acknowledgement = signed(&acknowledgement(&disposition.id.to_hex()), &actor, 4);
    let request = signed(
        &payload(CodingSessionTeamTransactionBody::DecisionRequest(
            CodingSessionTeamDecisionRequest {
                question: "Ship it?".into(),
                options: Vec::new(),
                held_on: CODING_SESSION_TEAM_DECISION_FOUNDER.into(),
                blocks: vec![assignment_id.clone()],
                recommendation: None,
            },
        )),
        &founder,
        5,
    );
    let completion = signed(&completed(&assignment_id), &founder, 6);
    let fold = fold_coding_session_team_transactions(
        &[
            assignment,
            report,
            disposition,
            acknowledgement,
            request.clone(),
            completion.clone(),
        ],
        &context,
    )
    .unwrap();

    assert!(fold.canonical_terminal.is_none());
    let pending = fold
        .pending_completion
        .as_ref()
        .expect("a pending completion");
    assert_eq!(pending.event_id, completion.id.to_hex());
    assert_eq!(
        pending.code,
        CodingSessionTeamFoldExclusionCode::CompletionBlockedByOpenDecision
    );
    // The assignment itself settled, so nothing is owed on the chain: the wait
    // is the ruling, and it is named where rulings live.
    assert!(pending.unsettled_assignment_event_ids.is_empty());
    assert_eq!(
        fold.waiting_on_decision
            .as_ref()
            .map(|item| item.request_event_id.as_str()),
        Some(request.id.to_hex().as_str())
    );
}

/// A completion nobody was entitled to sign is not "waiting": no arriving fact
/// makes it terminal, so it must never be disclosed as pending.
#[test]
fn an_unauthorized_completion_is_never_disclosed_as_pending() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let completion = signed(&completed(&assignment.id.to_hex()), &stranger, 2);
    let fold = fold_coding_session_team_transactions(&[assignment, completion], &context).unwrap();
    assert!(fold.pending_completion.is_none());
    assert_eq!(
        fold.excluded[0].code,
        CodingSessionTeamFoldExclusionCode::Unauthorized
    );
    for code in PENDING_COMPLETION_CODES {
        assert!(completion_exclusion_is_pending(*code));
    }
    assert!(!completion_exclusion_is_pending(
        CodingSessionTeamFoldExclusionCode::Unauthorized
    ));
    assert!(!completion_exclusion_is_pending(
        CodingSessionTeamFoldExclusionCode::TerminalConflict
    ));
}
