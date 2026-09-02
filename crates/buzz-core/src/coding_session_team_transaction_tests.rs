use nostr::Event;
use serde_json::Value;

use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag};

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn id(byte: &str) -> String {
    byte.repeat(32)
}

fn assignment(supersedes: Option<String>) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: id("ab"),
        transaction_type: CodingSessionTeamTransactionType::Assignment,
        supersedes,
        delivery_command_id: Some("wake-builder-1".to_owned()),
        body: CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
            assignee_actor: id("cd"),
            assignee_role: "builder".to_owned(),
            objective: "Ship the typed record".to_owned(),
            brief: "Implement and prove the core schema.".to_owned(),
            branch: Some("team-transactions".to_owned()),
            base_sha: Some("1".repeat(40)),
            file_ownership: vec!["crates/buzz-core/src/new.rs".to_owned()],
            acceptance_steps: vec!["cargo test -p buzz-core".to_owned()],
        }),
    }
}

fn report() -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        transaction_type: CodingSessionTeamTransactionType::Report,
        supersedes: None,
        delivery_command_id: None,
        body: CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
            assignment_ref: id("11"),
            summary: "Done".into(),
            branch: None,
            base_sha: None,
            head_sha: None,
            files: Vec::new(),
            tests: vec![CodingSessionTeamTransactionTest {
                name: "core".into(),
                command: "cargo test".into(),
                outcome: CodingSessionTeamTransactionTestOutcome::Passed,
                evidence: None,
            }],
            red_before_green: None,
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        }),
    }
}

fn event_with_keys(payload: &CodingSessionTeamTransactionPayload, keys: &Keys) -> Event {
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
    .sign_with_keys(keys)
    .unwrap()
}

fn event(payload: &CodingSessionTeamTransactionPayload) -> Event {
    event_with_keys(payload, &Keys::generate())
}

#[test]
fn accepts_exact_assignment_and_signature_is_only_author() {
    let event = event(&assignment(None));
    let decoded = validate_coding_session_team_transaction_envelope(&event).unwrap();
    assert_eq!(
        decoded.transaction_type,
        CodingSessionTeamTransactionType::Assignment
    );
    assert!(!event.content.contains("author"));
}

#[test]
fn accepts_every_closed_operation_and_exposes_causal_refs() {
    let bodies = [
        CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
            assignment_ref: id("11"),
            summary: "Done".into(),
            branch: None,
            base_sha: None,
            head_sha: Some("2".repeat(40)),
            files: vec!["src/lib.rs".into()],
            tests: vec![CodingSessionTeamTransactionTest {
                name: "core".into(),
                command: "cargo test -p buzz-core".into(),
                outcome: CodingSessionTeamTransactionTestOutcome::Passed,
                evidence: Some("1 passed; exit 0".into()),
            }],
            red_before_green: Some(true),
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        }),
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
            assignment_ref: id("11"),
            report_ref: id("22"),
            refutation_ref: None,
            decision: CodingSessionTeamDispositionDecision::Approve,
            summary: "Accepted".into(),
            findings: Vec::new(),
            required_action: None,
        }),
        CodingSessionTeamTransactionBody::Acknowledgement(CodingSessionTeamAcknowledgement {
            acknowledged_event_ref: id("33"),
            status: CodingSessionTeamAcknowledgementStatus::Received,
            note: None,
        }),
        CodingSessionTeamTransactionBody::MissionCompleted(CodingSessionTeamMissionCompleted {
            assignment_refs: vec![id("11")],
            landed_shas: vec!["4".repeat(40)],
            summary: "Landed".into(),
            follow_ups: Vec::new(),
        }),
        CodingSessionTeamTransactionBody::MissionBlocked(CodingSessionTeamMissionBlocked {
            assignment_refs: Vec::new(),
            summary: "Held".into(),
            blockers: vec!["Need a key".into()],
            held_on: Some("founder".into()),
            required_action: "Unlock the keychain".into(),
        }),
    ];

    for body in bodies {
        let transaction_type = body.transaction_type();
        let payload = CodingSessionTeamTransactionPayload {
            schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
            session_ref: SESSION.into(),
            genesis_ref: id("ab"),
            transaction_type,
            supersedes: None,
            delivery_command_id: Some("wake any operation".into()),
            body,
        };
        let decoded = validate_coding_session_team_transaction_envelope(&event(&payload)).unwrap();
        assert_eq!(decoded.transaction_type, transaction_type);
    }
}

#[test]
fn rejects_unknown_missing_duplicate_and_type_body_mismatch() {
    let valid = serde_json::to_value(assignment(None)).unwrap();
    let mut unknown = valid.clone();
    unknown["author"] = Value::String(id("ef"));
    assert!(decode_coding_session_team_transaction(&unknown.to_string()).is_err());

    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("supersedes");
    assert!(decode_coding_session_team_transaction(&missing.to_string()).is_err());

    let mut missing_command_parity = valid.clone();
    missing_command_parity
        .as_object_mut()
        .unwrap()
        .remove("deliveryCommandId");
    assert!(decode_coding_session_team_transaction(&missing_command_parity.to_string()).is_err());

    let duplicate = serde_json::to_string(&assignment(None)).unwrap().replace(
        "\"schema\":",
        "\"schema\":\"buzz-coding-session-team-transaction/v1\",\"schema\":",
    );
    assert!(decode_coding_session_team_transaction(&duplicate).is_err());

    let mut mismatch = valid;
    mismatch["type"] = Value::String("report".into());
    assert!(decode_coding_session_team_transaction(&mismatch.to_string()).is_err());

    let mut missing_nested = serde_json::to_value(report()).unwrap();
    missing_nested["body"]["tests"][0]
        .as_object_mut()
        .unwrap()
        .remove("evidence");
    assert!(decode_coding_session_team_transaction(&missing_nested.to_string()).is_err());
}

#[test]
fn verdict_subtypes_have_disjoint_closed_decisions() {
    let refutation = CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        transaction_type: CodingSessionTeamTransactionType::Verdict,
        supersedes: None,
        delivery_command_id: None,
        body: CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            assignment_ref: id("11"),
            report_ref: id("22"),
            decision: CodingSessionTeamRefutationDecision::Confirmed,
            summary: "Found a failing vector".into(),
            findings: vec!["Failure reproduced".into()],
            required_action: Some("Repair it".into()),
        }),
    };
    let mut invalid_refutation = serde_json::to_value(&refutation).unwrap();
    invalid_refutation["body"]["decision"] = Value::String("approve".into());
    assert!(decode_coding_session_team_transaction(&invalid_refutation.to_string()).is_err());

    let disposition = CodingSessionTeamTransactionPayload {
        body: CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
            assignment_ref: id("11"),
            report_ref: id("22"),
            refutation_ref: None,
            decision: CodingSessionTeamDispositionDecision::Approve,
            summary: "Accepted".into(),
            findings: Vec::new(),
            required_action: None,
        }),
        ..refutation
    };
    let mut invalid_disposition = serde_json::to_value(&disposition).unwrap();
    invalid_disposition["body"]["decision"] = Value::String("not-refuted".into());
    assert!(decode_coding_session_team_transaction(&invalid_disposition.to_string()).is_err());
    invalid_disposition = serde_json::to_value(&disposition).unwrap();
    invalid_disposition["body"]
        .as_object_mut()
        .unwrap()
        .remove("refutationRef");
    assert!(decode_coding_session_team_transaction(&invalid_disposition.to_string()).is_err());
}

#[test]
fn prose_only_test_claim_cannot_become_structured_test_evidence() {
    let mut string_claim = serde_json::to_value(report()).unwrap();
    string_claim["body"]["tests"] = serde_json::json!(["3/3 passing"]);
    assert!(decode_coding_session_team_transaction(&string_claim.to_string()).is_err());

    let mut object_claim = serde_json::to_value(report()).unwrap();
    object_claim["body"]["tests"] = serde_json::json!([{"claim": "3/3 passing"}]);
    assert!(decode_coding_session_team_transaction(&object_claim.to_string()).is_err());
}

#[test]
fn rejects_bad_bounds_ids_and_collections() {
    let mut payload = assignment(None);
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut payload.body {
        body.assignee_actor = "AB".repeat(32);
    }
    assert!(payload.validate().is_err());

    let mut payload = assignment(None);
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut payload.body {
        body.acceptance_steps = vec!["x".into(); MAX_TEAM_TRANSACTION_ITEMS + 1];
    }
    assert!(payload.validate().is_err());

    assert!(decode_coding_session_team_transaction(
        &"x".repeat(MAX_TEAM_TRANSACTION_CONTENT_BYTES + 1)
    )
    .is_err());

    let mut bad_role = assignment(None);
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut bad_role.body {
        body.assignee_role = "build_er".into();
    }
    assert!(bad_role.validate().is_err());

    let mut command = assignment(None);
    command.delivery_command_id = Some("turn-command-1".into());
    assert!(command.validate().is_ok());
    command.delivery_command_id = Some(" ".into());
    assert!(command.validate().is_err());
    command.delivery_command_id =
        Some("x".repeat(MAX_TEAM_TRANSACTION_DELIVERY_COMMAND_ID_BYTES + 1));
    assert!(command.validate().is_err());
}

#[test]
fn rejects_reordered_extra_and_disagreeing_tags() {
    let payload = assignment(None);
    let content = serde_json::to_string(&payload).unwrap();
    let cases = [
        vec![
            Tag::parse(["d", SESSION]).unwrap(),
            Tag::parse(["h", CHANNEL]).unwrap(),
            Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).unwrap(),
            Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).unwrap(),
            Tag::parse(["cstx-type", "assignment"]).unwrap(),
        ],
        vec![
            Tag::parse(["h", CHANNEL]).unwrap(),
            Tag::parse(["d", SESSION]).unwrap(),
            Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).unwrap(),
            Tag::parse(["cstx-genesis", &id("ef")]).unwrap(),
            Tag::parse(["cstx-type", "assignment"]).unwrap(),
        ],
        vec![
            Tag::parse(["h", CHANNEL]).unwrap(),
            Tag::parse(["d", SESSION]).unwrap(),
            Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).unwrap(),
            Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).unwrap(),
            Tag::parse(["cstx-type", "report"]).unwrap(),
        ],
    ];
    for tags in cases {
        let invalid = EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
            &content,
        )
        .tags(tags)
        .sign_with_keys(&Keys::generate())
        .unwrap();
        assert!(validate_coding_session_team_transaction_envelope(&invalid).is_err());
    }

    let wrong_kind = EventBuilder::new(Kind::TextNote, content)
        .tags([
            Tag::parse(["h", CHANNEL]).unwrap(),
            Tag::parse(["d", SESSION]).unwrap(),
            Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).unwrap(),
            Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).unwrap(),
            Tag::parse(["cstx-type", "assignment"]).unwrap(),
        ])
        .sign_with_keys(&Keys::generate())
        .unwrap();
    assert!(validate_coding_session_team_transaction_envelope(&wrong_kind).is_err());
}

#[test]
fn correction_requires_same_author_type_session_and_channel() {
    let keys = Keys::generate();
    let previous = event_with_keys(&assignment(None), &keys);
    let current = event_with_keys(&assignment(Some(previous.id.to_hex())), &keys);
    assert!(validate_coding_session_team_transaction_supersession(&current, &previous).is_ok());

    let wrong_author = event(&assignment(Some(previous.id.to_hex())));
    assert!(
        validate_coding_session_team_transaction_supersession(&wrong_author, &previous).is_err()
    );
}

#[test]
fn utf8_and_collection_boundaries_are_byte_exact() {
    let mut command = assignment(None);
    command.delivery_command_id = Some("é".repeat(128));
    assert!(command.validate().is_ok());
    command.delivery_command_id = Some("é".repeat(129));
    assert!(command.validate().is_err());
    command.delivery_command_id = Some("wake\nnow".into());
    assert!(command.validate().is_err());

    let test = CodingSessionTeamTransactionTest {
        name: "core".into(),
        command: "cargo test".into(),
        outcome: CodingSessionTeamTransactionTestOutcome::Passed,
        evidence: None,
    };
    let mut test_cap = report();
    if let CodingSessionTeamTransactionBody::Report(body) = &mut test_cap.body {
        body.tests = vec![test.clone(); MAX_TEAM_TRANSACTION_TESTS];
    }
    assert!(test_cap.validate().is_ok());
    if let CodingSessionTeamTransactionBody::Report(body) = &mut test_cap.body {
        body.tests.push(test);
    }
    assert!(test_cap.validate().is_err());

    let mut path_cap = report();
    if let CodingSessionTeamTransactionBody::Report(body) = &mut path_cap.body {
        body.files = vec!["x".repeat(MAX_TEAM_TRANSACTION_PATH_BYTES)];
    }
    assert!(path_cap.validate().is_ok());
    if let CodingSessionTeamTransactionBody::Report(body) = &mut path_cap.body {
        body.files = vec!["x".repeat(MAX_TEAM_TRANSACTION_PATH_BYTES + 1)];
    }
    assert!(path_cap.validate().is_err());

    let mut text_cap = report();
    if let CodingSessionTeamTransactionBody::Report(body) = &mut text_cap.body {
        body.summary = "é".repeat(MAX_TEAM_TRANSACTION_TEXT_BYTES / 2);
    }
    assert!(text_cap.validate().is_ok());
    if let CodingSessionTeamTransactionBody::Report(body) = &mut text_cap.body {
        body.summary = format!("{}x", "é".repeat(MAX_TEAM_TRANSACTION_TEXT_BYTES / 2));
    }
    assert!(text_cap.validate().is_err());

    let mut brief_cap = assignment(None);
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut brief_cap.body {
        body.brief = "x".repeat(MAX_TEAM_TRANSACTION_LONG_TEXT_BYTES);
    }
    assert!(brief_cap.validate().is_ok());
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut brief_cap.body {
        body.brief.push('x');
    }
    assert!(brief_cap.validate().is_err());
}

#[test]
fn duplicate_collections_are_rejected_at_schema_boundary() {
    let mut duplicate_paths = assignment(None);
    if let CodingSessionTeamTransactionBody::Assignment(body) = &mut duplicate_paths.body {
        body.file_ownership = vec!["src/lib.rs".into(), "src/lib.rs".into()];
    }
    assert!(duplicate_paths.validate().is_err());

    let mut duplicate_assignments = CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        transaction_type: CodingSessionTeamTransactionType::MissionCompleted,
        supersedes: None,
        delivery_command_id: None,
        body: CodingSessionTeamTransactionBody::MissionCompleted(
            CodingSessionTeamMissionCompleted {
                assignment_refs: vec![id("11"), id("11")],
                landed_shas: Vec::new(),
                summary: "Duplicate".into(),
                follow_ups: Vec::new(),
            },
        ),
    };
    assert!(duplicate_assignments.validate().is_err());
    if let CodingSessionTeamTransactionBody::MissionCompleted(body) =
        &mut duplicate_assignments.body
    {
        body.assignment_refs = vec![id("11")];
        body.landed_shas = vec!["1".repeat(40), "1".repeat(40)];
    }
    assert!(duplicate_assignments.validate().is_err());
}

#[test]
fn shared_schema_conformance_vectors_match_the_core_decoder() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../conformance/coding-session-team-transaction/fixtures/schema-vectors.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["schema"],
        "buzz-coding-session-team-transaction-conformance/v1"
    );
    let vectors = fixture["vectors"].as_array().unwrap();
    assert!(!vectors.is_empty());
    for vector in vectors {
        let name = vector["name"].as_str().unwrap();
        let expected = vector["valid"].as_bool().unwrap();
        let content = serde_json::to_string(&vector["content"]).unwrap();
        assert_eq!(
            decode_coding_session_team_transaction(&content).is_ok(),
            expected,
            "shared conformance vector {name}"
        );
    }
}

// --- B1c: the two state-free verbs, and the terminal that cannot clear itself.
//
// These are written against the JSON string boundary rather than the typed
// structs so that they compile, and fail, on a tree that has never heard of
// `note` or `decision.*`.

// The bounds are named here as literals rather than imported from the module
// under test, so that this whole group compiles — and fails — on a tree that
// has never heard of these verbs. `bounds_match_the_shipped_constants` below
// pins them to the shipped values.
const MAX_NOTE_REFS: usize = 16;
const MAX_DECISION_OPTIONS: usize = 8;
const MAX_DECISION_OPTION_BYTES: usize = 512;
const MAX_DECISION_BLOCKS: usize = 16;
const MAX_SHORT_TEXT_BYTES: usize = 2 * 1024;

fn envelope(transaction_type: &str, supersedes: Value, body: Value) -> String {
    serde_json::json!({
        "schema": CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": id("ab"),
        "type": transaction_type,
        "supersedes": supersedes,
        "deliveryCommandId": null,
        "body": body,
    })
    .to_string()
}

fn note_body() -> Value {
    serde_json::json!({
        "text": "Lane B is rebasing; nothing is blocked.",
        "refs": [id("11")],
    })
}

fn decision_request_body() -> Value {
    serde_json::json!({
        "question": "Ship the CLI fix now, or after the app rebuild?",
        "options": ["now", "after the rebuild"],
        "heldOn": "founder",
        "blocks": [id("11")],
        "recommendation": "after the rebuild — seats run the bundled bee",
    })
}

fn decision_answer_body(choice: Value) -> Value {
    serde_json::json!({
        "requestRef": id("22"),
        "choice": choice,
        "note": null,
    })
}

#[test]
fn note_accepts_the_exact_body_and_refuses_every_other_shape() {
    let decoded =
        decode_coding_session_team_transaction(&envelope("note", Value::Null, note_body()))
            .expect("exact note body");
    assert_eq!(decoded.transaction_type.as_str(), "note");
    // A note's refs are pointers, never causal: the fold must not need them.
    assert!(decoded.causal_references().is_empty());

    for bad in [
        serde_json::json!({"text": "said", "refs": [], "extra": 1}),
        serde_json::json!({"text": "said"}),
        serde_json::json!({"refs": []}),
        serde_json::json!({"text": "", "refs": []}),
        serde_json::json!({"text": "said", "refs": ["nothex"]}),
        serde_json::json!({"text": "said", "refs": [id("11"), id("11")]}),
        serde_json::json!({
            "text": "said",
            "refs": (0..=MAX_NOTE_REFS)
                .map(|index| format!("{index:064x}"))
                .collect::<Vec<_>>(),
        }),
        serde_json::json!({
            "text": "x".repeat(MAX_TEAM_TRANSACTION_TEXT_BYTES + 1),
            "refs": [],
        }),
    ] {
        assert!(
            decode_coding_session_team_transaction(&envelope("note", Value::Null, bad.clone()))
                .is_err(),
            "note body must be refused: {bad}"
        );
    }

    // Exactly at the caps is still valid.
    assert!(decode_coding_session_team_transaction(&envelope(
        "note",
        Value::Null,
        serde_json::json!({
            "text": "x".repeat(MAX_TEAM_TRANSACTION_TEXT_BYTES),
            "refs": (0..MAX_NOTE_REFS)
                .map(|index| format!("{index:064x}"))
                .collect::<Vec<_>>(),
        })
    ))
    .is_ok());
}

#[test]
fn a_note_never_supersedes_another_record() {
    let error = decode_coding_session_team_transaction(&envelope(
        "note",
        Value::String(id("33")),
        note_body(),
    ))
    .expect_err("a note carrying supersedes must be refused");
    assert_eq!(error, "a note never supersedes another record");
}

#[test]
fn decision_request_accepts_the_exact_body_and_bounds_every_collection() {
    let decoded = decode_coding_session_team_transaction(&envelope(
        "decision.request",
        Value::Null,
        decision_request_body(),
    ))
    .expect("exact decision.request body");
    assert_eq!(decoded.transaction_type.as_str(), "decision.request");
    // `blocks` are pointers, not causal references (REVIEW-B1c F3): correcting
    // the assignment a question is about must never delete the question.
    assert!(decoded.causal_references().is_empty());

    let with = |mutate: &dyn Fn(&mut serde_json::Map<String, Value>)| {
        let mut body = decision_request_body();
        if let Some(object) = body.as_object_mut() {
            mutate(object);
        }
        decode_coding_session_team_transaction(&envelope("decision.request", Value::Null, body))
    };

    // heldOn is exactly `founder` or a 64-hex actor; nothing else.
    assert!(with(&|body| {
        body.insert("heldOn".into(), Value::String(id("cd")));
    })
    .is_ok());
    for bad_held_on in ["", "Founder", "lead", "cd"] {
        assert!(
            with(&|body| {
                body.insert("heldOn".into(), Value::String(bad_held_on.into()));
            })
            .is_err(),
            "heldOn must refuse {bad_held_on:?}"
        );
    }
    assert!(with(&|body| {
        body.insert(
            "options".into(),
            Value::Array(
                (0..=MAX_DECISION_OPTIONS)
                    .map(|index| Value::String(format!("option {index}")))
                    .collect(),
            ),
        );
    })
    .is_err());
    assert!(with(&|body| {
        body.insert(
            "options".into(),
            Value::Array(vec![Value::String(
                "x".repeat(MAX_DECISION_OPTION_BYTES + 1),
            )]),
        );
    })
    .is_err());
    assert!(with(&|body| {
        body.insert(
            "options".into(),
            Value::Array(vec![
                Value::String("same".into()),
                Value::String("same".into()),
            ]),
        );
    })
    .is_err());
    assert!(with(&|body| {
        body.insert(
            "blocks".into(),
            Value::Array(
                (0..=MAX_DECISION_BLOCKS)
                    .map(|index| Value::String(format!("{index:064x}")))
                    .collect(),
            ),
        );
    })
    .is_err());
    assert!(with(&|body| {
        body.insert(
            "recommendation".into(),
            Value::String("x".repeat(MAX_SHORT_TEXT_BYTES + 1)),
        );
    })
    .is_err());
    assert!(with(&|body| {
        body.insert("unexpected".into(), Value::Bool(true));
    })
    .is_err());
    assert!(with(&|body| {
        body.remove("recommendation");
    })
    .is_err());
    // An open question with no options and no blocked assignment is still a
    // real request.
    assert!(with(&|body| {
        body.insert("options".into(), Value::Array(Vec::new()));
        body.insert("blocks".into(), Value::Array(Vec::new()));
        body.insert("recommendation".into(), Value::Null);
    })
    .is_ok());
}

#[test]
fn decision_answer_takes_an_option_index_or_bounded_text_and_nothing_else() {
    let indexed = decode_coding_session_team_transaction(&envelope(
        "decision.answer",
        Value::Null,
        decision_answer_body(serde_json::json!(1)),
    ))
    .expect("indexed answer");
    assert_eq!(indexed.transaction_type.as_str(), "decision.answer");
    assert_eq!(indexed.causal_references(), vec![id("22").as_str()]);
    assert!(decode_coding_session_team_transaction(&envelope(
        "decision.answer",
        Value::Null,
        decision_answer_body(serde_json::json!("neither; hold until the rebuild")),
    ))
    .is_ok());

    for bad_choice in [
        serde_json::json!(MAX_DECISION_OPTIONS),
        serde_json::json!(-1),
        serde_json::json!(""),
        serde_json::json!("x".repeat(MAX_SHORT_TEXT_BYTES + 1)),
        serde_json::json!(null),
        serde_json::json!(true),
        serde_json::json!(["now"]),
    ] {
        assert!(
            decode_coding_session_team_transaction(&envelope(
                "decision.answer",
                Value::Null,
                decision_answer_body(bad_choice.clone()),
            ))
            .is_err(),
            "choice must be refused: {bad_choice}"
        );
    }

    for bad in [
        serde_json::json!({"requestRef": id("22"), "choice": 0}),
        serde_json::json!({"requestRef": id("22"), "choice": 0, "note": null, "extra": 1}),
        serde_json::json!({"requestRef": "nothex", "choice": 0, "note": null}),
    ] {
        assert!(
            decode_coding_session_team_transaction(&envelope(
                "decision.answer",
                Value::Null,
                bad.clone()
            ))
            .is_err(),
            "decision.answer body must be refused: {bad}"
        );
    }
}

#[test]
fn a_blocked_correction_naming_no_blocker_is_refused_with_the_remedy() {
    // Keystone's exact shape on 2026-09-01: a second `mission.blocked`
    // correcting the first, saying nothing is blocked any more.
    let clearing = serde_json::json!({
        "assignmentRefs": [],
        "summary": "Nothing is blocked; work resumed.",
        "blockers": [],
        "heldOn": null,
        "requiredAction": "None — the lanes are running again.",
    });
    let error = decode_coding_session_team_transaction(&envelope(
        "mission.blocked",
        Value::String(id("44")),
        clearing.clone(),
    ))
    .expect_err("a terminal correcting itself to empty must be refused");
    assert_eq!(
        error,
        "use a note or a decision.answer to clear a blocker; a terminal cannot clear itself"
    );
    assert!(error.contains("note"));
    assert!(error.contains("decision.answer"));

    // The same body without `supersedes` keeps its own, older refusal: this
    // rule adds a remedy, it does not relax anything.
    assert!(decode_coding_session_team_transaction(&envelope(
        "mission.blocked",
        Value::Null,
        clearing
    ))
    .is_err());

    // A correction that still names a blocker is a legitimate correction.
    assert!(decode_coding_session_team_transaction(&envelope(
        "mission.blocked",
        Value::String(id("44")),
        serde_json::json!({
            "assignmentRefs": [],
            "summary": "Signing is still held.",
            "blockers": ["the keychain is locked"],
            "heldOn": "founder",
            "requiredAction": "Unlock the signing key",
        })
    ))
    .is_ok());
}

#[test]
fn the_new_verbs_carry_the_exact_five_tag_envelope() {
    for (transaction_type, body) in [
        ("note", note_body()),
        ("decision.request", decision_request_body()),
        (
            "decision.answer",
            decision_answer_body(serde_json::json!(0)),
        ),
    ] {
        let content = envelope(transaction_type, Value::Null, body);
        let payload: CodingSessionTeamTransactionPayload = serde_json::from_str(&content).unwrap();
        let event = event(&payload);
        let decoded = validate_coding_session_team_transaction_envelope(&event).unwrap();
        assert_eq!(decoded.transaction_type.as_str(), transaction_type);
        assert_eq!(
            event.tags.as_slice()[4].as_slice(),
            ["cstx-type".to_owned(), transaction_type.to_owned()]
        );
    }
}

#[test]
fn bounds_match_the_shipped_constants() {
    assert_eq!(MAX_NOTE_REFS, MAX_TEAM_TRANSACTION_NOTE_REFS);
    assert_eq!(MAX_DECISION_OPTIONS, MAX_TEAM_TRANSACTION_DECISION_OPTIONS);
    assert_eq!(
        MAX_DECISION_OPTION_BYTES,
        MAX_TEAM_TRANSACTION_DECISION_OPTION_BYTES
    );
    assert_eq!(MAX_DECISION_BLOCKS, MAX_TEAM_TRANSACTION_DECISION_BLOCKS);
    assert_eq!(MAX_SHORT_TEXT_BYTES, MAX_TEAM_TRANSACTION_SHORT_TEXT_BYTES);
    assert_eq!(
        TERMINAL_CANNOT_CLEAR_ITSELF,
        "use a note or a decision.answer to clear a blocker; a terminal cannot clear itself"
    );
    assert_eq!(CODING_SESSION_TEAM_DECISION_FOUNDER, "founder");
}

#[test]
fn a_blocked_correction_must_change_its_blockers() {
    // REVIEW-B1c F5, stronger reading. This needs both records in hand, so it
    // lives in the supersession validator rather than the single-record schema.
    let founder = Keys::generate();
    let blocked = |summary: &str, blockers: Vec<String>, supersedes: Option<String>| {
        CodingSessionTeamTransactionPayload {
            schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
            session_ref: SESSION.into(),
            genesis_ref: id("ab"),
            transaction_type: CodingSessionTeamTransactionType::MissionBlocked,
            supersedes,
            delivery_command_id: None,
            body: CodingSessionTeamTransactionBody::MissionBlocked(
                CodingSessionTeamMissionBlocked {
                    assignment_refs: Vec::new(),
                    summary: summary.into(),
                    blockers,
                    held_on: Some("founder".into()),
                    required_action: "Unlock the signing key".into(),
                },
            ),
        }
    };

    let first = event_with_keys(
        &blocked(
            "Signing is held",
            vec!["the keychain is locked".into()],
            None,
        ),
        &founder,
    );
    let prose_only = event_with_keys(
        &blocked(
            "Signing is still held, per the 23:06 sync",
            vec!["the keychain is locked".into()],
            Some(first.id.to_hex()),
        ),
        &founder,
    );
    assert_eq!(
        validate_coding_session_team_transaction_supersession(&prose_only, &first).unwrap_err(),
        TERMINAL_PROSE_EDIT_NEEDS_A_NOTE
    );
    assert_eq!(
        TERMINAL_PROSE_EDIT_NEEDS_A_NOTE,
        "a mission.blocked correction must change its blockers; use a note to add context"
    );

    // Order is prose too.
    let two = event_with_keys(
        &blocked("Two", vec!["keychain".into(), "relay".into()], None),
        &founder,
    );
    let reordered = event_with_keys(
        &blocked(
            "Two, reordered",
            vec!["relay".into(), "keychain".into()],
            Some(two.id.to_hex()),
        ),
        &founder,
    );
    assert!(validate_coding_session_team_transaction_supersession(&reordered, &two).is_err());

    // A correction that changes what is blocking is still a correction.
    let real = event_with_keys(
        &blocked(
            "The relay is down too",
            vec!["the keychain is locked".into(), "the relay is down".into()],
            Some(first.id.to_hex()),
        ),
        &founder,
    );
    assert!(validate_coding_session_team_transaction_supersession(&real, &first).is_ok());
}

#[path = "coding_session_team_transaction_terminal_tests.rs"]
mod terminal_tests;
