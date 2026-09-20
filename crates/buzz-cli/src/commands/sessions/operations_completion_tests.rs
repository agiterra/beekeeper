//! Ledger 183: `bee sessions complete` publishes an early completion and says
//! what it is waiting for, instead of refusing it.

use super::*;

use buzz_core::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamActiveSeat, CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamFoldContext, CodingSessionTeamMissionCompleted, CodingSessionTeamReport,
    CodingSessionTeamTransactionBody, CodingSessionTeamVerdict,
};
use buzz_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};
use nostr::Keys;

use super::super::super::operations_precheck::{FetchedSession, PrecheckedOperation};

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

fn signed(body: CodingSessionTeamTransactionBody, keys: &Keys) -> Event {
    let payload = coding_session_team_transaction_payload(
        SESSION.to_owned(),
        GENESIS.to_owned(),
        None,
        None,
        body,
    );
    build_coding_session_team_transaction(CHANNEL, payload)
        .expect("the record builds")
        .sign_with_keys(keys)
        .expect("the record signs")
}

fn context(founder: &Keys, actor: &Keys) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: vec![CodingSessionTeamActiveSeat {
            actor_pubkey: actor.public_key().to_hex(),
            role: "builder".into(),
        }],
        active_grants: Vec::new(),
        verifier_required: false,
    }
}

fn assignment(actor: &Keys) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
        assignee_actor: actor.public_key().to_hex(),
        assignee_role: "builder".into(),
        objective: "Build the slice".into(),
        brief: "Implement the bounded assigned slice.".into(),
        branch: None,
        base_sha: None,
        file_ownership: vec!["crates/buzz-core/src".into()],
        acceptance_steps: vec!["cargo test -p buzz-core".into()],
    })
}

fn report(assignment_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
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
    })
}

fn approval(assignment_ref: &str, report_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
        assignment_ref: assignment_ref.into(),
        report_ref: report_ref.into(),
        refutation_ref: None,
        decision: CodingSessionTeamDispositionDecision::Approve,
        summary: "Governed".into(),
        findings: Vec::new(),
        required_action: None,
    })
}

fn completion(assignment_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::MissionCompleted(CodingSessionTeamMissionCompleted {
        assignment_refs: vec![assignment_ref.into()],
        landed_shas: Vec::new(),
        summary: "Mission finished".into(),
        follow_ups: Vec::new(),
    })
}

fn prechecked(events: Vec<Event>, context: CodingSessionTeamFoldContext) -> PrecheckedOperation {
    PrecheckedOperation {
        supersedes: None,
        session: Some(FetchedSession { events, context }),
    }
}

/// The exact refusal of ledger 179(a), now an answer: the completion is
/// classified `pending`, and the missing acknowledgement is named with the
/// role and actor that owe it.
#[test]
fn a_completion_missing_an_acknowledgement_is_pending_and_names_the_owed_receipt() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, &actor);
    let assignment = signed(assignment(&actor), &founder);
    let report = signed(report(&assignment.id.to_hex()), &actor);
    let approval = signed(
        approval(&assignment.id.to_hex(), &report.id.to_hex()),
        &founder,
    );
    let candidate = signed(completion(&assignment.id.to_hex()), &founder);
    let prechecked = prechecked(vec![assignment.clone(), report, approval], context.clone());

    let outcome = classify_completion_before_submit(&prechecked, &candidate)
        .expect("an early completion is published, not refused");
    let CompletionOutcome::Pending(pending) = &outcome else {
        panic!("expected a pending completion, got {outcome:?}");
    };
    assert_eq!(pending["code"], json!("completion_not_approved"));
    assert_eq!(pending["awaiting"].as_array().expect("a list").len(), 1);
    let awaiting = &pending["awaiting"][0];
    assert_eq!(awaiting["assignmentEventId"], json!(assignment.id.to_hex()));
    assert_eq!(awaiting["link"], json!("acknowledgement"));
    assert_eq!(awaiting["owedByRole"], json!("builder"));
    assert_eq!(awaiting["owedByActor"], json!(actor.public_key().to_hex()));
    // Present and null, never absent: no ruling is open, and "no ruling" and
    // "not disclosed" must stay different answers.
    assert_eq!(pending["waitingOnDecision"], Value::Null);
    assert_eq!(pending["message"], json!(COMPLETION_PENDING_MESSAGE));

    // And the answer a caller reads says `pending` with that object attached.
    let mut output = json!({"eventId": candidate.id.to_hex(), "accepted": true});
    disclose_completion_outcome(&mut output, &outcome);
    assert_eq!(output["outcome"], json!("pending"));
    assert_eq!(output["pending"]["code"], json!("completion_not_approved"));
}

/// The complete chain still folds terminal, and says so in the same two keys,
/// so a reader parses one shape either way.
#[test]
fn a_complete_chain_is_terminal_and_discloses_no_pending_wait() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, &actor);
    let assignment = signed(assignment(&actor), &founder);
    let report = signed(report(&assignment.id.to_hex()), &actor);
    let approval = signed(
        approval(&assignment.id.to_hex(), &report.id.to_hex()),
        &founder,
    );
    let acknowledgement = signed(
        CodingSessionTeamTransactionBody::Acknowledgement(CodingSessionTeamAcknowledgement {
            acknowledged_event_ref: approval.id.to_hex(),
            status: CodingSessionTeamAcknowledgementStatus::Received,
            note: None,
        }),
        &actor,
    );
    let candidate = signed(completion(&assignment.id.to_hex()), &founder);
    let prechecked = prechecked(vec![assignment, report, approval, acknowledgement], context);

    let outcome = classify_completion_before_submit(&prechecked, &candidate)
        .expect("a settled mission completes");
    assert_eq!(outcome, CompletionOutcome::Terminal);
    let mut output = json!({"eventId": candidate.id.to_hex()});
    disclose_completion_outcome(&mut output, &outcome);
    assert_eq!(output["outcome"], json!("terminal"));
    assert_eq!(output["pending"], Value::Null);
}

/// A completion nobody was entitled to sign is still refused before it reaches
/// the relay: no fact that later arrives would make it terminal, so calling it
/// "pending" would be a promise the fold will never keep.
#[test]
fn an_unauthorized_completion_is_still_refused_before_publication() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let context = context(&founder, &actor);
    let assignment = signed(assignment(&actor), &founder);
    let candidate = signed(completion(&assignment.id.to_hex()), &stranger);
    let prechecked = prechecked(vec![assignment], context);

    let error = classify_completion_before_submit(&prechecked, &candidate)
        .expect_err("an unauthorized completion is refused");
    let message = error.to_string();
    assert!(
        message.contains("completion refused"),
        "unexpected refusal: {message}"
    );
}

/// The pending sentence is printed to a person, so its shape is pinned exactly
/// as `COMPLETION_REFUSED_FALLBACK`'s is (REVIEW-L7 F3 found eighteen spaces
/// in the middle of that one).
#[test]
fn the_pending_message_reads_as_prose() {
    assert!(
        !COMPLETION_PENDING_MESSAGE.contains("  "),
        "the pending message carries a run of spaces: {COMPLETION_PENDING_MESSAGE:?}"
    );
    assert!(!COMPLETION_PENDING_MESSAGE.contains('\n'));
    assert!(COMPLETION_PENDING_MESSAGE.contains("no further turn from anybody"));
}
