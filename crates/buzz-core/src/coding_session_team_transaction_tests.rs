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
