//! Unit tests for the signed team-transaction transport.
//!
//! A child of `operations`, split out only to keep that file under 1,000 lines
//! (batch 2 lane B2). `use super::*` gives them the parent's private items
//! exactly as the inline module did; no test changed.

use buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload;
use nostr::{EventBuilder, Keys, Kind, Tag};

use super::*;
use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
    CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use buzz_core::kind::{KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_SYSTEM_MESSAGE};

use super::super::operations_authority::{
    project_receipt_backed_authority_chain, AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
};

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

fn seat_event(
    signer: &Keys,
    transition_type: CodingSessionAuthorityTransitionType,
    grantee: &str,
    role: &str,
    seq: u32,
    previous: Option<&Event>,
) -> Event {
    let payload = match transition_type {
        CodingSessionAuthorityTransitionType::GrantSeat => {
            CodingSessionAuthorityTransitionPayload::new_grant_seat(
                GENESIS,
                previous.map(|event| event.id.to_hex()),
                seq,
                grantee,
                role,
            )
        }
        CodingSessionAuthorityTransitionType::RevokeSeat => {
            CodingSessionAuthorityTransitionPayload::new_revoke_seat(
                GENESIS,
                previous.map(|event| event.id.to_hex()),
                seq,
                grantee,
                role,
            )
        }
        _ => panic!("seat helper requires a seat transition"),
    };
    let tags = [
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION]).expect("version"),
        Tag::parse(["csat-genesis", GENESIS]).expect("genesis"),
    ];
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
        serde_json::to_string(&payload).expect("payload"),
    )
    .tags(tags)
    .sign_with_keys(signer)
    .expect("sign")
}

fn authority_receipt(transition: &Event, relay: &Keys) -> Event {
    let payload = decode_coding_session_authority_transition(&transition.content)
        .expect("transition payload");
    let mut content = json!({
        "type": AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
        "genesisRef": payload.genesis_ref,
        "acceptedEventId": transition.id.to_hex(),
        "seq": payload.seq,
        "transitionType": payload.transition_type,
        "granteePubkey": payload.grantee_pubkey,
    });
    if let (Some(object), Some(role)) = (content.as_object_mut(), payload.role) {
        object.insert("role".into(), Value::String(role));
    }
    EventBuilder::new(
        Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        content.to_string(),
    )
    .tags([Tag::parse(["h", CHANNEL]).expect("h")])
    .sign_with_keys(relay)
    .expect("receipt")
}

fn project_authority_chain(
    events: &[Event],
    channel: &str,
    genesis: &str,
    founder: &str,
) -> Result<ProjectedAuthority, String> {
    let relay = Keys::generate();
    let receipts: Vec<Event> = events
        .iter()
        .map(|event| authority_receipt(event, &relay))
        .collect();
    project_receipt_backed_authority_chain(
        events,
        &receipts,
        channel,
        genesis,
        founder,
        &relay.public_key().to_hex(),
    )
}

#[test]
fn fold_json_discloses_unseated_reports_under_fold() {
    let fold = CodingSessionTeamFold {
        included_event_ids: vec![id("11"), id("22")],
        excluded: Vec::new(),
        conflicts: Vec::new(),
        assignments: Vec::new(),
        unseated_reports: vec![
            buzz_core::coding_session_team_transaction::CodingSessionTeamUnseatedReport {
                event_id: id("22"),
                author_pubkey: id("33"),
                assignment_ref: id("11"),
                assignee_role: "builder".into(),
            },
        ],
        notes: Vec::new(),
        decisions: Vec::new(),
        waiting_on_decision: None,
        canonical_terminal: None,
    };

    let wire = fold_json(&fold);
    assert_eq!(
        wire["unseatedReports"],
        json!([{
            "eventId": id("22"),
            "authorPubkey": id("33"),
            "assignmentRef": id("11"),
            "assigneeRole": "builder",
        }])
    );

    let empty = CodingSessionTeamFold {
        included_event_ids: Vec::new(),
        excluded: Vec::new(),
        conflicts: Vec::new(),
        assignments: Vec::new(),
        unseated_reports: Vec::new(),
        notes: Vec::new(),
        decisions: Vec::new(),
        waiting_on_decision: None,
        canonical_terminal: None,
    };
    // Present and empty, never absent: an unknown disclosure and "no
    // unseated reports" are different answers.
    assert_eq!(fold_json(&empty)["unseatedReports"], json!([]));
}

#[test]
fn fold_json_prints_the_dangling_reference_code() {
    let excluded = buzz_core::coding_session_team_transaction::CodingSessionTeamFoldExclusion {
        event_id: id("22"),
        code: buzz_core::coding_session_team_transaction::CodingSessionTeamFoldExclusionCode::DanglingReference,
        reason: format!(
            "reference {} is absent from the supplied transaction set",
            id("77")
        ),
    };
    let fold = CodingSessionTeamFold {
        included_event_ids: vec![id("11")],
        excluded: vec![excluded],
        conflicts: Vec::new(),
        assignments: Vec::new(),
        unseated_reports: Vec::new(),
        notes: Vec::new(),
        decisions: Vec::new(),
        waiting_on_decision: None,
        canonical_terminal: None,
    };

    let wire = fold_json(&fold);
    assert_eq!(
        wire["excluded"],
        json!([{
            "eventId": id("22"),
            "code": "DanglingReference",
            "reason": format!(
                "reference {} is absent from the supplied transaction set",
                id("77")
            ),
        }])
    );
    // `included` is still answered: one malformed record no longer denies
    // every operation in the session (batch 2 2026-09-01, B1b).
    assert_eq!(wire["includedEventIds"], json!([id("11")]));
}

#[test]
fn fold_json_prints_the_invalid_correction_code() {
    let excluded = buzz_core::coding_session_team_transaction::CodingSessionTeamFoldExclusion {
        event_id: id("33"),
        code: buzz_core::coding_session_team_transaction::CodingSessionTeamFoldExclusionCode::InvalidCorrection,
        reason: format!(
            "correction of {} is invalid: a correction must preserve its logical subject",
            id("22")
        ),
    };
    let fold = CodingSessionTeamFold {
        included_event_ids: vec![id("11"), id("22")],
        excluded: vec![excluded],
        conflicts: Vec::new(),
        assignments: Vec::new(),
        unseated_reports: Vec::new(),
        notes: Vec::new(),
        decisions: Vec::new(),
        waiting_on_decision: None,
        canonical_terminal: None,
    };

    let wire = fold_json(&fold);
    assert_eq!(wire["excluded"][0]["code"], "InvalidCorrection");
    assert!(wire["excluded"][0]["reason"]
        .as_str()
        .expect("reason string")
        .contains(&id("22")));
    // The corrected record keeps its place in the projection.
    assert_eq!(wire["includedEventIds"], json!([id("11"), id("22")]));
}

#[test]
fn fold_json_prints_the_wrong_type_reference_code() {
    let excluded = buzz_core::coding_session_team_transaction::CodingSessionTeamFoldExclusion {
        event_id: id("44"),
        code: buzz_core::coding_session_team_transaction::CodingSessionTeamFoldExclusionCode::WrongTypeReference,
        reason: format!("team transaction {} has a wrong-type reference", id("44")),
    };
    let fold = CodingSessionTeamFold {
        included_event_ids: vec![id("11"), id("22")],
        excluded: vec![excluded],
        conflicts: Vec::new(),
        assignments: Vec::new(),
        unseated_reports: Vec::new(),
        notes: Vec::new(),
        decisions: Vec::new(),
        waiting_on_decision: None,
        canonical_terminal: None,
    };

    let wire = fold_json(&fold);
    assert_eq!(wire["excluded"][0]["code"], "WrongTypeReference");
    assert_eq!(wire["includedEventIds"], json!([id("11"), id("22")]));
}

fn id(byte: &str) -> String {
    byte.repeat(32)
}

#[test]
fn fold_json_lists_notes_decisions_and_the_waiting_state() {
    use buzz_core::coding_session_team_transaction::{
        CodingSessionTeamFoldDecision, CodingSessionTeamFoldNote,
        CodingSessionTeamFoldWaitingOnDecision,
    };

    let fold = CodingSessionTeamFold {
        included_event_ids: vec![id("11"), id("22"), id("33")],
        excluded: Vec::new(),
        conflicts: Vec::new(),
        assignments: Vec::new(),
        unseated_reports: Vec::new(),
        notes: vec![CodingSessionTeamFoldNote {
            event_id: id("22"),
            author_pubkey: id("aa"),
            refs: vec![id("11")],
        }],
        decisions: vec![CodingSessionTeamFoldDecision {
            request_event_id: id("33"),
            held_on: "founder".into(),
            blocks: vec![id("11")],
            answered_by: None,
            answer_event_id: None,
        }],
        waiting_on_decision: Some(CodingSessionTeamFoldWaitingOnDecision {
            request_event_id: id("33"),
            held_on: "founder".into(),
        }),
        canonical_terminal: None,
    };

    let wire = fold_json(&fold);
    assert_eq!(
        wire["notes"],
        json!([{ "eventId": id("22"), "authorPubkey": id("aa"), "refs": [id("11")] }])
    );
    assert_eq!(
        wire["decisions"],
        json!([{
            "requestId": id("33"),
            "heldOn": "founder",
            "blocks": [id("11")],
            "answeredBy": null,
            "answerId": null,
        }])
    );
    assert_eq!(
        wire["waitingOnDecision"],
        json!({ "requestId": id("33"), "heldOn": "founder" })
    );
    // Waiting on a person is not a terminal, and the CLI must not print one.
    assert_eq!(wire["canonicalTerminal"], json!(null));

    let quiet = CodingSessionTeamFold {
        included_event_ids: Vec::new(),
        excluded: Vec::new(),
        conflicts: Vec::new(),
        assignments: Vec::new(),
        unseated_reports: Vec::new(),
        notes: Vec::new(),
        decisions: Vec::new(),
        waiting_on_decision: None,
        canonical_terminal: None,
    };
    let quiet_wire = fold_json(&quiet);
    // Present and empty, never absent: "nothing was said" and "notes were
    // not disclosed" are different answers.
    assert_eq!(quiet_wire["notes"], json!([]));
    assert_eq!(quiet_wire["decisions"], json!([]));
    assert_eq!(quiet_wire["waitingOnDecision"], json!(null));
}

#[test]
fn decode_body_answers_every_type_in_the_closed_vocabulary() {
    // One arm per wire token: a type the CLI cannot decode is a verb no
    // seat can publish, however well the core schema supports it.
    for (transaction_type, body) in [
        (
            CodingSessionTeamTransactionType::Note,
            json!({"text": "said", "refs": []}),
        ),
        (
            CodingSessionTeamTransactionType::DecisionRequest,
            json!({
                "question": "Ship now?",
                "options": ["yes"],
                "heldOn": "founder",
                "blocks": [],
                "recommendation": null,
            }),
        ),
        (
            CodingSessionTeamTransactionType::DecisionAnswer,
            json!({"requestRef": id("22"), "choice": 0, "note": null}),
        ),
    ] {
        let decoded = decode_body(transaction_type, body).expect("decode body");
        assert_eq!(decoded.transaction_type(), transaction_type);
    }
    assert!(decode_body(
        CodingSessionTeamTransactionType::Note,
        json!({"text": "said", "refs": [], "extra": 1})
    )
    .is_err());
}

#[test]
fn authority_projection_folds_lead_verifier_role_change_and_revoke() {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let worker = Keys::generate();
    let lead_grant = seat_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantSeat,
        &lead.public_key().to_hex(),
        "lead",
        1,
        None,
    );
    let verifier_grant = seat_event(
        &lead,
        CodingSessionAuthorityTransitionType::GrantSeat,
        &worker.public_key().to_hex(),
        "verifier",
        2,
        Some(&lead_grant),
    );
    let role_change = seat_event(
        &founder,
        CodingSessionAuthorityTransitionType::GrantSeat,
        &worker.public_key().to_hex(),
        "builder",
        3,
        Some(&verifier_grant),
    );
    let authority = project_authority_chain(
        &[
            lead_grant.clone(),
            verifier_grant.clone(),
            role_change.clone(),
        ],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
    )
    .expect("seat projection");
    assert!(authority
        .seats
        .iter()
        .any(|seat| { seat.actor_pubkey == lead.public_key().to_hex() && seat.role == "lead" }));
    assert!(authority.seats.iter().any(|seat| {
        seat.actor_pubkey == worker.public_key().to_hex() && seat.role == "builder"
    }));

    let revoke = seat_event(
        &founder,
        CodingSessionAuthorityTransitionType::RevokeSeat,
        &worker.public_key().to_hex(),
        "builder",
        4,
        Some(&role_change),
    );
    let authority = project_authority_chain(
        &[lead_grant, verifier_grant, role_change, revoke],
        CHANNEL,
        GENESIS,
        &founder.public_key().to_hex(),
    )
    .expect("revoke projection");
    assert!(!authority
        .seats
        .iter()
        .any(|seat| seat.actor_pubkey == worker.public_key().to_hex()));
}

// ── REVIEW-B2 F4: a completion says which terminal it corrected ──────────────

/// `bee sessions complete` adopts the lead's own canonical `mission.blocked` as
/// its `supersedes` when the caller named none. The author asked to publish a
/// completion and published a correction of their own terminal — a materially
/// different signed record — and the only output they see must say so.
#[test]
fn a_completion_answer_names_the_terminal_it_corrected() {
    let adopted = "1a".repeat(32);
    let mut output = json!({"eventId": "ab".repeat(32), "accepted": true});
    disclose_adopted_correction(
        &mut output,
        CodingSessionTeamTransactionType::MissionCompleted,
        None,
        Some(adopted.as_str()),
    );
    assert_eq!(output["supersedes"], Value::String(adopted.clone()));
    let sentence = output["correctedTerminal"].as_str().expect("sentence");
    assert!(sentence.contains(&adopted), "{sentence}");
    assert!(
        sentence.contains("corrects your mission.blocked"),
        "{sentence}"
    );
}

/// Present and null, never absent: "corrected nothing" and "did not say" are
/// different answers.
#[test]
fn a_completion_that_corrected_nothing_still_says_so() {
    let mut output = json!({"eventId": "ab".repeat(32), "accepted": true});
    disclose_adopted_correction(
        &mut output,
        CodingSessionTeamTransactionType::MissionCompleted,
        None,
        None,
    );
    assert!(output
        .as_object()
        .expect("object")
        .contains_key("supersedes"));
    assert_eq!(output["supersedes"], Value::Null);
    assert!(!output
        .as_object()
        .expect("object")
        .contains_key("correctedTerminal"));
}

/// A caller who typed `--supersedes` gets the id echoed but no "corrected"
/// sentence: nothing was adopted on their behalf.
#[test]
fn an_explicit_supersedes_is_echoed_without_the_adoption_sentence() {
    let named = "1a".repeat(32);
    let mut output = json!({"eventId": "ab".repeat(32), "accepted": true});
    disclose_adopted_correction(
        &mut output,
        CodingSessionTeamTransactionType::MissionCompleted,
        Some(named.as_str()),
        Some(named.as_str()),
    );
    assert_eq!(output["supersedes"], Value::String(named));
    assert!(!output
        .as_object()
        .expect("object")
        .contains_key("correctedTerminal"));
}

/// Every other verb's answer is untouched — its `--supersedes` is exactly what
/// the caller typed, so there is nothing to disclose.
#[test]
fn other_verbs_answers_are_untouched() {
    for transaction_type in [
        CodingSessionTeamTransactionType::Report,
        CodingSessionTeamTransactionType::MissionBlocked,
        CodingSessionTeamTransactionType::Note,
    ] {
        let mut output = json!({"eventId": "ab".repeat(32), "accepted": true});
        disclose_adopted_correction(
            &mut output,
            transaction_type,
            None,
            Some("1a".repeat(32).as_str()),
        );
        assert!(!output
            .as_object()
            .expect("object")
            .contains_key("supersedes"));
    }
}
