//! Tests for the closed kind:44249 envelope, bound to the frozen fixtures.
//!
//! Every invalid record fixture declares its own refusal as
//! `"<code>: <why>"`. These tests read that declaration out of the file
//! rather than restating it, so the fixture and the validator cannot drift:
//! change a fixture's declared code and the test demands the new one.

use super::*;

macro_rules! fixture {
    ($path:literal) => {
        include_str!(concat!(
            "../../../conformance/project-work/fixtures/",
            $path
        ))
    };
}

const VALID: [(&str, &str); 3] = [
    (
        "work-declared",
        fixture!("records/valid/work-declared.json"),
    ),
    (
        "work-assignment-bound",
        fixture!("records/valid/work-assignment-bound.json"),
    ),
    (
        "work-evidence-bound",
        fixture!("records/valid/work-evidence-bound.json"),
    ),
];

const INVALID: [(&str, &str); 18] = [
    (
        "a-tag-not-canonical",
        fixture!("records/invalid/a-tag-not-canonical.json"),
    ),
    (
        "criterion-id-not-a-slug",
        fixture!("records/invalid/criterion-id-not-a-slug.json"),
    ),
    (
        "d-tag-mismatch",
        fixture!("records/invalid/d-tag-mismatch.json"),
    ),
    (
        "duplicate-criterion-ids",
        fixture!("records/invalid/duplicate-criterion-ids.json"),
    ),
    (
        "empty-criterion-ids",
        fixture!("records/invalid/empty-criterion-ids.json"),
    ),
    (
        "empty-evidence-refs",
        fixture!("records/invalid/empty-evidence-refs.json"),
    ),
    (
        "evidence-kind-unknown",
        fixture!("records/invalid/evidence-kind-unknown.json"),
    ),
    (
        "missing-null-key",
        fixture!("records/invalid/missing-null-key.json"),
    ),
    (
        "plan-path-escapes",
        fixture!("records/invalid/plan-path-escapes.json"),
    ),
    (
        "plan-repository-bare-id",
        fixture!("records/invalid/plan-repository-bare-id.json"),
    ),
    (
        "self-reference",
        fixture!("records/invalid/self-reference.json"),
    ),
    (
        "short-artifact-commit",
        fixture!("records/invalid/short-artifact-commit.json"),
    ),
    (
        "short-plan-commit",
        fixture!("records/invalid/short-plan-commit.json"),
    ),
    ("tag-count", fixture!("records/invalid/tag-count.json")),
    (
        "type-tag-mismatch",
        fixture!("records/invalid/type-tag-mismatch.json"),
    ),
    (
        "unknown-body-key",
        fixture!("records/invalid/unknown-body-key.json"),
    ),
    (
        "unsupported-schema",
        fixture!("records/invalid/unsupported-schema.json"),
    ),
    ("wrong-kind", fixture!("records/invalid/wrong-kind.json")),
];

fn fixture_event(json: &str) -> ProjectWorkEvent {
    serde_json::from_str(json).expect("fixture event")
}

/// `{"refusal": "<code>: <why>", "event": {…}}`
fn refusal_case(json: &str) -> (String, ProjectWorkEvent) {
    let value: serde_json::Value = serde_json::from_str(json).expect("fixture");
    let declared = value["refusal"]
        .as_str()
        .expect("an invalid fixture declares its refusal")
        .split(':')
        .next()
        .expect("a refusal declaration names a code")
        .trim()
        .to_owned();
    let event = serde_json::from_value(value["event"].clone()).expect("fixture event");
    (declared, event)
}

#[test]
fn every_valid_record_round_trips_byte_for_byte_in_canonical_form() {
    for (name, json) in VALID {
        let event = fixture_event(json);
        let payload = validate_project_work_envelope(&event)
            .unwrap_or_else(|refusal| panic!("{name} must validate: {refusal}"));
        assert_eq!(
            payload.canonical_content().expect("canonical"),
            event.content,
            "{name}: the canonical form is not the bytes on the wire"
        );
        let tags: Vec<Vec<String>> = payload
            .canonical_tags(&event.tags[0][1])
            .into_iter()
            .map(|pair| pair.to_vec())
            .collect();
        assert_eq!(tags, event.tags, "{name}: canonical tags");
        validate_project_work_payload(&payload)
            .unwrap_or_else(|refusal| panic!("{name} must pass the publication check: {refusal}"));
    }
}

#[test]
fn every_refused_record_is_refused_for_the_reason_it_names() {
    for (name, json) in INVALID {
        let (declared, event) = refusal_case(json);
        let refusal =
            validate_project_work_envelope(&event).expect_err(&format!("{name} must be refused"));
        assert_eq!(
            refusal.code.as_str(),
            declared,
            "{name}: refused as {} but the fixture declares {declared}; message {:?}",
            refusal.code,
            refusal.message
        );
    }
}

#[test]
fn the_three_bodies_decode_to_the_variants_the_contract_names() {
    let declared = validate_project_work_envelope(&fixture_event(VALID[0].1)).expect("declared");
    let ProjectWorkBody::Declared(body) = &declared.body else {
        panic!("work.declared decodes to a declaration");
    };
    assert_eq!(body.work_id, "9d0f0f0f-1111-4222-8333-444444444444");
    assert_eq!(body.plan_ref.path, "plans/kettle.md");
    assert_eq!(
        body.plan_ref.blob_key(),
        format!(
            "{}@{}:{}",
            body.plan_ref.repository, body.plan_ref.commit, body.plan_ref.path
        )
    );
    assert!(body.supersedes.is_empty());

    let bound = validate_project_work_envelope(&fixture_event(VALID[1].1)).expect("assignment");
    let ProjectWorkBody::AssignmentBound(body) = &bound.body else {
        panic!("work.assignment_bound decodes to an assignment binding");
    };
    assert_eq!(body.criterion_ids.len(), 2);
    assert_eq!(body.replaces_binding, None);

    let evidence = validate_project_work_envelope(&fixture_event(VALID[2].1)).expect("evidence");
    let ProjectWorkBody::EvidenceBound(body) = &evidence.body else {
        panic!("work.evidence_bound decodes to an evidence binding");
    };
    assert_eq!(body.evidence_refs.len(), 2);
    assert_eq!(body.evidence_refs[0].kind, ProjectWorkEvidenceKind::Report);
    assert_eq!(body.completion_ref, None);
}

#[test]
fn a_nullable_key_is_present_and_written_as_null() {
    // Absent, not null, is the refusal `missing-null-key` pins. The other
    // direction — that the canonical form writes the key — is here.
    let payload = validate_project_work_envelope(&fixture_event(VALID[1].1)).expect("assignment");
    let content = payload.canonical_content().expect("canonical");
    assert!(
        content.contains("\"replacesBinding\":null"),
        "an unset optional is on the wire as JSON null: {content}"
    );
    let payload = validate_project_work_envelope(&fixture_event(VALID[2].1)).expect("evidence");
    let content = payload.canonical_content().expect("canonical");
    assert!(content.contains("\"completionRef\":null"), "{content}");
}

#[test]
fn content_over_the_ceiling_is_refused_before_it_is_parsed() {
    let oversize = "x".repeat(MAX_PROJECT_WORK_CONTENT_BYTES + 1);
    let refusal = decode_project_work_content(&oversize).expect_err("over the ceiling");
    assert_eq!(refusal.code, ProjectWorkRefusalCode::TooLarge);
}

#[test]
fn an_unknown_record_type_is_refused_by_name_at_both_ends() {
    let mut event = fixture_event(VALID[0].1);
    event.tags[5][1] = "work.retired".to_owned();
    let refusal = validate_project_work_envelope(&event).expect_err("unknown pwk-type");
    assert_eq!(refusal.code, ProjectWorkRefusalCode::RecordType);
    assert_eq!(refusal.path, "tags.pwk-type");

    let mut event = fixture_event(VALID[0].1);
    event.content = event.content.replace("work.declared", "work.retired");
    event.tags[5][1] = "work.retired".to_owned();
    let refusal = validate_project_work_envelope(&event).expect_err("unknown content type");
    assert_eq!(refusal.code, ProjectWorkRefusalCode::RecordType);
}

#[test]
fn an_unknown_schema_version_is_refused_in_the_tag_as_well_as_the_content() {
    let mut event = fixture_event(VALID[0].1);
    event.tags[3][1] = "buzz-project-work/v2".to_owned();
    let refusal = validate_project_work_envelope(&event).expect_err("v2 tag");
    assert_eq!(refusal.code, ProjectWorkRefusalCode::Schema);
    assert_eq!(refusal.path, "tags.pwk-v");
}

#[test]
fn the_tag_order_is_fixed_not_merely_the_tag_set() {
    let mut event = fixture_event(VALID[0].1);
    event.tags.swap(1, 2);
    let refusal = validate_project_work_envelope(&event).expect_err("reordered tags");
    assert_eq!(refusal.code, ProjectWorkRefusalCode::TagCount);
}

#[test]
fn a_repository_coordinate_is_canonical_and_full() {
    assert!(is_canonical_repository_coordinate(&format!(
        "30617:{}:pivot-test-beekeeper-agents",
        "1e".repeat(32)
    )));
    for bad in [
        "pivot-test-beekeeper-agents",
        "30621:1ead:pivot-test",
        &format!("30617:{}:pivot-test", "1E".repeat(32)),
        &format!("30617:{}:Pivot-Test", "1e".repeat(32)),
        &format!("30617:{}:", "1e".repeat(32)),
    ] {
        assert!(!is_canonical_repository_coordinate(bad), "{bad:?}");
    }
}

#[test]
fn a_project_coordinate_is_canonical_lowercase() {
    assert!(is_canonical_project_coordinate(&format!(
        "30621:{}:kettle",
        "1e".repeat(32)
    )));
    assert!(
        !is_canonical_project_coordinate(&format!("30621:{}:kettle", "1E".repeat(32))),
        "an uppercase pubkey is not canonical; normalizing it here would let two \
         readers disagree about which string names this project"
    );
}

#[test]
fn the_publication_check_refuses_what_the_reader_would_refuse() {
    let mut payload =
        validate_project_work_envelope(&fixture_event(VALID[1].1)).expect("assignment");
    let ProjectWorkBody::AssignmentBound(body) = &mut payload.body else {
        panic!("assignment");
    };
    body.criterion_ids = vec!["CLI-Behaviour".to_owned()];
    let refusal = validate_project_work_payload(&payload).expect_err("not a slug");
    assert_eq!(refusal.code, ProjectWorkRefusalCode::Slug);

    let mut payload = validate_project_work_envelope(&fixture_event(VALID[0].1)).expect("declared");
    payload.record_type = ProjectWorkRecordType::EvidenceBound;
    let refusal = validate_project_work_payload(&payload).expect_err("type and body disagree");
    assert_eq!(refusal.code, ProjectWorkRefusalCode::TagParity);
}
