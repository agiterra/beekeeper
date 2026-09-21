//! Ledger 183: which arriving records make the provider re-read a channel.

use super::*;

use buzz_core::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDecisionAnswer, CodingSessionTeamDecisionChoice,
    CodingSessionTeamDispositionDecision, CodingSessionTeamMissionCompleted, CodingSessionTeamNote,
    CodingSessionTeamRefutationDecision, CodingSessionTeamReport,
};
use buzz_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};
use nostr::{EventBuilder, Keys, Kind};

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn id(byte: &str) -> String {
    byte.repeat(32)
}

fn signed(body: CodingSessionTeamTransactionBody) -> Event {
    let payload =
        coding_session_team_transaction_payload(SESSION.to_owned(), id("ab"), None, None, body);
    build_coding_session_team_transaction(CHANNEL, payload)
        .expect("the record builds")
        .sign_with_keys(&Keys::generate())
        .expect("the record signs")
}

#[test]
fn the_three_records_a_held_completion_waits_on_are_settlement_facts() {
    let acknowledgement = signed(CodingSessionTeamTransactionBody::Acknowledgement(
        CodingSessionTeamAcknowledgement {
            acknowledged_event_ref: id("cd"),
            status: CodingSessionTeamAcknowledgementStatus::Received,
            note: None,
        },
    ));
    assert_eq!(
        settlement_fact(&acknowledgement),
        Some(SettlementFact::Acknowledgement)
    );
    assert_eq!(SettlementFact::Acknowledgement.as_str(), "acknowledgement");

    let answer = signed(CodingSessionTeamTransactionBody::DecisionAnswer(
        CodingSessionTeamDecisionAnswer {
            request_ref: id("cd"),
            choice: CodingSessionTeamDecisionChoice::Text("ship it".into()),
            note: None,
            condition: None,
        },
    ));
    assert_eq!(
        settlement_fact(&answer),
        Some(SettlementFact::DecisionAnswer)
    );

    let refutation = signed(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Refutation {
            assignment_ref: id("cd"),
            report_ref: id("ef"),
            decision: CodingSessionTeamRefutationDecision::NotRefuted,
            summary: "No refutation found".into(),
            findings: Vec::new(),
            required_action: None,
        },
    ));
    assert_eq!(
        settlement_fact(&refutation),
        Some(SettlementFact::VerifierRefutation)
    );
}

#[test]
fn nothing_else_asks_for_a_re_read() {
    let assignment = signed(CodingSessionTeamTransactionBody::Assignment(
        CodingSessionTeamAssignment {
            assignee_actor: id("cd"),
            assignee_role: "builder".into(),
            objective: "Build the slice".into(),
            brief: "Implement the bounded assigned slice.".into(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["crates/buzz-core/src".into()],
            acceptance_steps: vec!["cargo test -p buzz-core".into()],
        },
    ));
    assert_eq!(settlement_fact(&assignment), None);

    let report = signed(CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: id("cd"),
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
    ));
    // A report is the *other* route: it is a wake candidate, and must never be
    // consumed as a settlement fact instead.
    assert_eq!(settlement_fact(&report), None);

    // A disposition that **asks** the assignee for something leaves the
    // acknowledgement owed, so it settles nothing on its own. The approving
    // disposition that asks nothing is the opposite case and has its own test
    // below — lane 210 is exactly the difference between these two bodies.
    let disposition = signed(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: id("cd"),
            report_ref: id("ef"),
            refutation_ref: None,
            decision: CodingSessionTeamDispositionDecision::Approve,
            summary: "Governed".into(),
            findings: Vec::new(),
            required_action: Some("Confirm you have read the residuals.".into()),
        },
    ));
    assert_eq!(settlement_fact(&disposition), None);

    let changes = signed(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: id("cd"),
            report_ref: id("ef"),
            refutation_ref: None,
            decision: CodingSessionTeamDispositionDecision::ChangesRequested,
            summary: "Not yet".into(),
            findings: Vec::new(),
            required_action: None,
        },
    ));
    assert_eq!(settlement_fact(&changes), None);

    let note = signed(CodingSessionTeamTransactionBody::Note(
        CodingSessionTeamNote {
            text: "Said, nothing changed".into(),
            refs: Vec::new(),
        },
    ));
    assert_eq!(settlement_fact(&note), None);

    let completion = signed(CodingSessionTeamTransactionBody::MissionCompleted(
        CodingSessionTeamMissionCompleted {
            assignment_refs: vec![id("cd")],
            landed_shas: Vec::new(),
            summary: "Mission finished".into(),
            follow_ups: Vec::new(),
        },
    ));
    assert_eq!(settlement_fact(&completion), None);
}

/// An event that is not a team transaction at all answers `None` rather than
/// panicking or being read as a fact about a mission.
#[test]
fn a_foreign_event_is_not_a_settlement_fact() {
    let event = EventBuilder::new(Kind::Custom(1), "hello")
        .sign_with_keys(&Keys::generate())
        .expect("it signs");
    assert_eq!(settlement_fact(&event), None);
}

/// Lane 210: an approving disposition that asks the assignee for nothing
/// settles its assignment on arrival, so it is a settlement fact — the
/// provider must re-read the channel when one lands, or a held completion
/// sits terminal-in-fact and pending-in-view until something else happens to
/// wake the pass.
#[test]
fn an_approving_disposition_that_asks_nothing_is_a_settlement_fact() {
    for decision in [
        CodingSessionTeamDispositionDecision::Approve,
        CodingSessionTeamDispositionDecision::ApproveWithNotes,
    ] {
        let disposition = signed(CodingSessionTeamTransactionBody::Verdict(
            CodingSessionTeamVerdict::Disposition {
                assignment_ref: id("cd"),
                report_ref: id("ef"),
                refutation_ref: None,
                decision,
                // Findings are the ruling's own reasoning, never an ask.
                findings: vec!["The gates ran on the exact commit.".into()],
                summary: "Governed".into(),
                required_action: None,
            },
        ));
        assert_eq!(
            settlement_fact(&disposition),
            Some(SettlementFact::ApprovingDispositionWithoutAsk)
        );
    }
    assert_eq!(
        SettlementFact::ApprovingDispositionWithoutAsk.as_str(),
        "approving_disposition_without_ask"
    );
}

/// The provider's reader of the shared settlement conformance vectors
/// (ledger 204's rule; `conformance/team-settlement/README.md`).
///
/// Every record in every vector declares whether its arrival is a settlement
/// fact. The provider's classifier must agree with the fold's rule on all of
/// them, or a held completion sits terminal-in-fact and pending-in-view until
/// something unrelated happens to wake the discovery pass.
#[test]
fn every_conformance_record_is_classified_as_the_vectors_declare() {
    const SETTLEMENT_VECTORS: &str =
        include_str!("../../../conformance/team-settlement/fixtures/settlement-vectors.json");
    let fixture: serde_json::Value =
        serde_json::from_str(SETTLEMENT_VECTORS).expect("the fixture is JSON");
    let mut checked = 0usize;
    for vector in fixture["vectors"].as_array().expect("vectors") {
        let name = vector["name"].as_str().expect("a name");
        for record in vector["records"].as_array().expect("records") {
            // Assignments carry no `settlementFact` key at all: an assignment
            // can only ever add a prerequisite, and the vectors say so by
            // omission rather than by repeating `null` on every row.
            let Some(expected) = record.get("settlementFact") else {
                continue;
            };
            let body = match record["type"].as_str().expect("a type") {
                "report" => CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
                    assignment_ref: id("cd"),
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
                }),
                "disposition" => CodingSessionTeamTransactionBody::Verdict(
                    CodingSessionTeamVerdict::Disposition {
                        assignment_ref: id("cd"),
                        report_ref: id("ef"),
                        refutation_ref: None,
                        decision: match record["decision"].as_str().expect("a decision") {
                            "approve" => CodingSessionTeamDispositionDecision::Approve,
                            "approve-with-notes" => {
                                CodingSessionTeamDispositionDecision::ApproveWithNotes
                            }
                            "changes-requested" => {
                                CodingSessionTeamDispositionDecision::ChangesRequested
                            }
                            "reject" => CodingSessionTeamDispositionDecision::Reject,
                            _ => CodingSessionTeamDispositionDecision::Blocked,
                        },
                        summary: "Governed".into(),
                        findings: record["findings"]
                            .as_array()
                            .expect("findings")
                            .iter()
                            .map(|item| item.as_str().expect("a finding").to_owned())
                            .collect(),
                        required_action: record["requiredAction"]
                            .as_str()
                            .map(std::borrow::ToOwned::to_owned),
                    },
                ),
                "acknowledgement" => CodingSessionTeamTransactionBody::Acknowledgement(
                    CodingSessionTeamAcknowledgement {
                        acknowledged_event_ref: id("cd"),
                        status: CodingSessionTeamAcknowledgementStatus::Received,
                        note: None,
                    },
                ),
                "completion" => CodingSessionTeamTransactionBody::MissionCompleted(
                    CodingSessionTeamMissionCompleted {
                        assignment_refs: vec![id("cd")],
                        landed_shas: Vec::new(),
                        summary: "Mission finished".into(),
                        follow_ups: Vec::new(),
                    },
                ),
                other => panic!("{name}: an unknown record type {other}"),
            };
            let event = signed(body);
            assert_eq!(
                settlement_fact(&event).map(SettlementFact::as_str),
                expected.as_str(),
                "{name}: record {}",
                record["id"].as_str().unwrap_or("?")
            );
            checked += 1;
        }
    }
    assert!(checked >= 20, "the vectors must exercise the classifier");
}
