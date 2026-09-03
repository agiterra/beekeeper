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
            "code": "dangling_reference",
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
    assert_eq!(wire["excluded"][0]["code"], "invalid_correction");
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
    assert_eq!(wire["excluded"][0]["code"], "wrong_type_reference");
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

// ── Finding 19: a request held on an actor wakes that actor ─────────────────

/// **Live run 2, 2026-09-02.** Bob published a `decision.request` held on
/// Keystone's pubkey and **no wake was published at all** — Keystone learned
/// nothing until a person looked. `decide answer` has defaulted its wake to the
/// asker's role since REVIEW-B1c F4; `decide request` defaulted nothing, and
/// `--wake-to` speaks cs-target keys, session ids and role slugs, never
/// pubkeys, while `heldOn` is exactly a pubkey. So even a caller who wanted to
/// wake the held-on party had no name for them.
///
/// The rule: with no explicit `--wake-to`, a request held on an **actor**
/// resolves that pubkey to the role of its seated execution, through the same
/// receipt-backed 44228 projection the fold uses.
#[test]
fn a_request_held_on_an_actor_wakes_that_actors_seated_execution() {
    let keystone = "11".repeat(32);
    let bob = "22".repeat(32);
    let seats = vec![
        CodingSessionTeamActiveSeat {
            actor_pubkey: keystone.clone(),
            role: "lead".to_owned(),
        },
        CodingSessionTeamActiveSeat {
            actor_pubkey: bob.clone(),
            role: "builder".to_owned(),
        },
    ];

    assert_eq!(
        held_on_wake_role(&keystone, &seats),
        Some("lead".to_owned()),
        "the pubkey heldOn names resolves to the role --wake-to understands"
    );
    assert_eq!(held_on_wake_role(&bob, &seats), Some("builder".to_owned()));
}

/// The founder is a person, not an execution, and an actor with no seat has no
/// execution to wake. Both answer `None` — the request is still published, and
/// the absent `delivery` key is how a reader sees that nobody was woken.
#[test]
fn a_founder_held_request_and_an_unseated_actor_wake_nobody() {
    let seats = vec![CodingSessionTeamActiveSeat {
        actor_pubkey: "11".repeat(32),
        role: "lead".to_owned(),
    }];
    assert_eq!(
        held_on_wake_role(CODING_SESSION_TEAM_DECISION_FOUNDER, &seats),
        None,
        "a ruling held on the founder wakes no execution: the founder is a person"
    );
    assert_eq!(
        held_on_wake_role(&"33".repeat(32), &seats),
        None,
        "an actor holding no active seat has no execution to wake, and inventing one is a guess"
    );
    assert_eq!(held_on_wake_role(&"11".repeat(32), &[]), None);
}

// ── Fix round 1: F4, three distinguishable "nobody was woken" shapes ────────

/// **REVIEW-L1 F4.** `publish_operation` inserted `delivery` only when a wake
/// was actually published, so three different facts produced byte-identical
/// JSON: the ruling is held on the founder (nothing to wake — fine), the
/// caller asked for no wake (fine), and **the party the mission is waiting on
/// holds no seat and will not hear about it** (not fine). Empty meant unknown,
/// which is the shape §0.8 and I9 forbid, and only the third case is a problem.
///
/// Now each says which it is, and every one of them says nobody was woken.
#[test]
fn each_reason_nobody_was_woken_has_its_own_shape() {
    let held_on = "ede6301723c5772cd47166b61c680cf321359cd9b89e1c887225c1ef895a3602";

    let founder = wake_omission_delivery(&WakeOmission::FounderHeld);
    assert_eq!(founder["status"], "founder-held");
    assert_eq!(founder["published"], false);
    assert!(founder["heldOn"].is_null(), "{founder}");

    let no_seat = wake_omission_delivery(&WakeOmission::NoSeat {
        held_on: held_on.to_owned(),
    });
    assert_eq!(no_seat["status"], "no-seat");
    assert_eq!(no_seat["published"], false);
    assert_eq!(
        no_seat["heldOn"], held_on,
        "the whole pubkey, so the reader can go and grant it a seat: {no_seat}"
    );

    let not_requested = wake_omission_delivery(&WakeOmission::NotRequested);
    assert_eq!(not_requested["status"], "not-requested");
    assert_eq!(not_requested["published"], false);

    // Distinguishable: three different status words and three different
    // sentences, and every sentence says nobody was woken.
    let all = [&founder, &no_seat, &not_requested];
    let statuses: std::collections::BTreeSet<&str> = all
        .iter()
        .map(|value| value["status"].as_str().expect("status"))
        .collect();
    assert_eq!(statuses.len(), 3, "{all:?}");
    let messages: std::collections::BTreeSet<&str> = all
        .iter()
        .map(|value| value["message"].as_str().expect("message"))
        .collect();
    assert_eq!(messages.len(), 3, "{all:?}");
    for value in all {
        assert!(
            value["message"]
                .as_str()
                .expect("message")
                .contains("nobody was woken"),
            "{value}"
        );
    }
    // The one that is actually a problem names the remedy.
    assert!(
        no_seat["message"]
            .as_str()
            .expect("message")
            .contains("no active seat"),
        "{no_seat}"
    );
}

/// The omission reaches the caller's JSON under the same `delivery` key a real
/// wake uses, so a reader parses one field rather than two.
#[tokio::test]
async fn an_omission_is_reported_under_the_same_delivery_key_as_a_wake() {
    let operation_id = "ab".repeat(32);
    let submitted_id = operation_id.clone();
    let output = submit_record_then_wake(
        &operation_id,
        move || async move {
            Ok(json!({ "event_id": submitted_id, "accepted": true, "message": "" }).to_string())
        },
        None::<fn() -> std::future::Ready<Result<Value, CliError>>>,
        Some(WakeOmission::FounderHeld),
    )
    .await
    .expect("a record with no wake is still a stored record");

    assert_eq!(output["accepted"], true);
    assert_eq!(
        output["delivery"]["status"], "founder-held",
        "the key is present and says why, rather than being absent: {output}"
    );
    assert_eq!(output["delivery"]["published"], false);
}

// ── Finding 21: a ruling may name a condition instead of a commit ───────────

/// **Live run 2, 11:33.** A builder asked the same question twice because the
/// founder's first answer had been given about one SHA and a second SHA needed
/// the identical ruling. `--condition` is where the class goes.
///
/// Asserted on the **built event**, not on the struct: the point is that the
/// text the caller typed reaches the signed content byte-for-byte, through the
/// SDK's builder and its strict decoder, with nothing normalising it on the
/// way.
#[test]
fn a_condition_reaches_the_signed_content_verbatim() {
    let condition = "any SHA whose buzz-acp diff against origin/main is empty";
    let payload = coding_session_team_transaction_payload(
        "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned(),
        GENESIS.to_owned(),
        None,
        None,
        CodingSessionTeamTransactionBody::DecisionAnswer(CodingSessionTeamDecisionAnswer {
            request_ref: "cd".repeat(32),
            choice: CodingSessionTeamDecisionChoice::Index(0),
            note: None,
            condition: Some(condition.to_owned()),
        }),
    );
    let event = build_coding_session_team_transaction(CHANNEL, payload)
        .expect("the answer builds")
        .sign_with_keys(&Keys::generate())
        .expect("the answer signs");

    let content: Value = serde_json::from_str(&event.content).expect("signed content is JSON");
    assert_eq!(content["body"]["condition"], json!(condition));
    // And the wire body still carries exactly seven keys, in the one shape the
    // relay's decoder accepts.
    let mut keys: Vec<&String> = content["body"]
        .as_object()
        .expect("a body object")
        .keys()
        .collect();
    keys.sort();
    assert_eq!(keys, vec!["choice", "condition", "note", "requestRef"]);

    // Omitted, the key is still present as JSON null — absent is not null.
    let without = coding_session_team_transaction_payload(
        "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned(),
        GENESIS.to_owned(),
        None,
        None,
        CodingSessionTeamTransactionBody::DecisionAnswer(CodingSessionTeamDecisionAnswer {
            request_ref: "cd".repeat(32),
            choice: CodingSessionTeamDecisionChoice::Index(0),
            note: None,
            condition: None,
        }),
    );
    let event = build_coding_session_team_transaction(CHANNEL, without)
        .expect("the answer builds")
        .sign_with_keys(&Keys::generate())
        .expect("the answer signs");
    let content: Value = serde_json::from_str(&event.content).expect("signed content is JSON");
    assert_eq!(content["body"]["condition"], Value::Null);
}

/// **REVIEW-L7 F3.** The fallback refusal is printed to a person, so its shape
/// is a fact worth pinning: no run of spaces, no newline, and it names the
/// three things a completion actually needs.
#[test]
fn the_completion_refusal_fallback_reads_as_one_sentence() {
    assert!(
        !COMPLETION_REFUSED_FALLBACK.contains("  "),
        "the refusal carries a run of spaces: {COMPLETION_REFUSED_FALLBACK:?}"
    );
    assert!(!COMPLETION_REFUSED_FALLBACK.contains('\n'));
    assert_eq!(
        COMPLETION_REFUSED_FALLBACK,
        "every referenced assignment must have an active report, an approving disposition, \
         and the assigned actor's acknowledgement"
    );
}
