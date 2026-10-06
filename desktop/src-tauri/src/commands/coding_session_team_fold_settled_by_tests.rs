//! Lane 210's settlement rule, across the Tauri boundary.
//!
//! A new file rather than lines in
//! `coding_session_team_fold_settlement_tests.rs` because this repository
//! splits files rather than raising its 1,000-line ceiling, and because the
//! question is its own: not *where is this chain waiting* (183(g)) but *which
//! rule settled it* — the one thing a person reading a settled row with no
//! acknowledgement id would otherwise have to guess.

use super::settlement_tests::{assignment_payload, fold, payload, report_payload, signed};
use beekeeper_core_pkg::coding_session_team_transaction::{
    CodingSessionTeamDispositionDecision, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionPayload, CodingSessionTeamVerdict,
};
use nostr::Keys;

fn disposition_payload(
    assignment_ref: &str,
    report_ref: &str,
    required_action: Option<&str>,
) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            refutation_ref: None,
            decision: CodingSessionTeamDispositionDecision::Approve,
            summary: "Governed".into(),
            findings: Vec::new(),
            required_action: required_action.map(std::borrow::ToOwned::to_owned),
        },
    ))
}

#[test]
fn an_approving_disposition_without_an_ask_crosses_as_settled_and_says_why() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment = signed(&assignment_payload(&actor), &founder, 1);
    let report = signed(&report_payload(&assignment.id.to_hex()), &actor, 2);
    let disposition = signed(
        &disposition_payload(&assignment.id.to_hex(), &report.id.to_hex(), None),
        &founder,
        3,
    );
    let response = fold(&founder, &[assignment, report.clone(), disposition.clone()]);

    let settlement = &response.assignments[0];
    assert!(settlement.settled);
    assert_eq!(
        settlement.settled_by.as_deref(),
        Some("approving_disposition_without_ask"),
        "the adapter must carry the fold's own word, not a derived one"
    );
    assert_eq!(
        settlement.acknowledgement_event_id, None,
        "no receipt exists, and the row says so rather than hiding it"
    );
    assert_eq!(
        settlement.disposition_event_id.as_deref(),
        Some(disposition.id.to_hex().as_str())
    );
    assert!(settlement.awaiting.is_none());

    let wire = serde_json::to_value(&response).expect("serialize the response");
    assert_eq!(
        wire["assignments"][0]["settledBy"],
        serde_json::json!("approving_disposition_without_ask"),
        "the TypeScript decoder reads this exact key"
    );
}

#[test]
fn a_disposition_that_asks_still_crosses_as_awaiting_an_acknowledgement() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let assignment = signed(&assignment_payload(&actor), &founder, 1);
    let report = signed(&report_payload(&assignment.id.to_hex()), &actor, 2);
    let disposition = signed(
        &disposition_payload(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            Some("Confirm you have read the residuals."),
        ),
        &founder,
        3,
    );
    let response = fold(&founder, &[assignment, report, disposition]);

    let settlement = &response.assignments[0];
    assert!(!settlement.settled);
    assert_eq!(settlement.settled_by, None, "null exactly when not settled");
    assert_eq!(
        settlement
            .awaiting
            .as_ref()
            .map(|awaiting| awaiting.link.as_str()),
        Some("acknowledgement")
    );
}
