//! Tests for the `beekeeper-plan/v1` parser, bound to the frozen fixtures.
//!
//! Every invalid fixture declares its own refusal in a
//! `# REFUSED: <code> — <why>` comment inside its frontmatter. These tests
//! read that declaration out of the file rather than restating it, so a
//! fixture and its test cannot drift apart: change the fixture's declared
//! code and the test demands the parser produce the new one.

use super::*;

const KETTLE: &str =
    include_str!("../../../conformance/project-work/fixtures/plans/valid/kettle.md");

/// Every refused fixture, as `(name, bytes)`.
const INVALID: [(&str, &str); 6] = [
    (
        "absolute-path-in-action-name",
        include_str!(
            "../../../conformance/project-work/fixtures/plans/invalid/absolute-path-in-action-name.md"
        ),
    ),
    (
        "duplicate-id",
        include_str!("../../../conformance/project-work/fixtures/plans/invalid/duplicate-id.md"),
    ),
    (
        "empty-accept",
        include_str!("../../../conformance/project-work/fixtures/plans/invalid/empty-accept.md"),
    ),
    (
        "oversize",
        include_str!("../../../conformance/project-work/fixtures/plans/invalid/oversize.md"),
    ),
    (
        "recycled-retired-id",
        include_str!(
            "../../../conformance/project-work/fixtures/plans/invalid/recycled-retired-id.md"
        ),
    ),
    (
        "unknown-key",
        include_str!("../../../conformance/project-work/fixtures/plans/invalid/unknown-key.md"),
    ),
];

/// Read the `# REFUSED: <code> — <why>` declaration out of a fixture.
fn declared_code(text: &str) -> PlanRefusalCode {
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("# REFUSED:"))
        .expect("every invalid plan fixture declares its refusal");
    let raw = line
        .trim_start()
        .trim_start_matches("# REFUSED:")
        .split_whitespace()
        .next()
        .expect("a refusal declaration names a code");
    plan_refusal_code_from_str(raw).unwrap_or_else(|| {
        panic!("fixture declares refusal code {raw:?}, which no variant carries")
    })
}

#[test]
fn the_kettle_plan_parses_exactly_as_the_contract_describes_it() {
    let plan = parse_plan(KETTLE.as_bytes()).expect("the worked example must parse");
    assert_eq!(plan.schema, PROJECT_PLAN_SCHEMA);
    assert_eq!(plan.id, "kettle-cli");
    assert_eq!(plan.status, PlanStatus::InForce);
    assert_eq!(plan.title, "Build and land kettle");
    assert_eq!(plan.code_repository, "pivot-test");
    assert_eq!(plan.delivery_ref, "refs/heads/main");
    assert_eq!(plan.retired_criteria, Vec::<String>::new());
    assert_eq!(plan.bytes, KETTLE.len());

    let ids: Vec<&str> = plan
        .criteria
        .iter()
        .map(|criterion| criterion.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "cli-behaviour",
            "shared-storage-and-parser",
            "verified-landed-revision",
            "usage-documentation",
            "delivered-main",
        ]
    );
    // All three proof forms appear, and the action proof resolves its pair.
    assert_eq!(plan.criteria[0].proof, PlanProof::Review);
    assert_eq!(
        plan.criteria[2].proof,
        PlanProof::Action {
            name: "verify".to_owned(),
            step: "verify".to_owned(),
        }
    );
    assert_eq!(plan.criteria[4].proof, PlanProof::GitRef);
    // The body below the frontmatter is context, never contract: nothing in
    // it reaches the parsed plan.
    assert!(plan.criteria.iter().all(|c| !c.accept.contains("Pulse")));
}

#[test]
fn every_refused_fixture_is_refused_for_the_reason_it_names() {
    for (name, text) in INVALID {
        let expected = declared_code(text);
        let refusal = parse_plan(text.as_bytes()).expect_err(&format!("{name} must be refused"));
        assert_eq!(
            refusal.code, expected,
            "{name}: refused as {} but the fixture declares {expected}; message {:?}",
            refusal.code, refusal.message
        );
    }
}

#[test]
fn the_oversize_fixture_is_refused_before_it_is_parsed() {
    let (_, oversize) = INVALID[3];
    assert!(oversize.len() > MAX_PLAN_FILE_BYTES);
    // Its frontmatter is valid; only the file's size is not.
    let refusal = parse_plan(oversize.as_bytes()).expect_err("over the ceiling");
    assert_eq!(refusal.code, PlanRefusalCode::PlanTooLarge);
    assert_eq!(refusal.path, "file");
}

#[test]
fn the_proof_forms_are_closed_in_both_directions() {
    // A review proof with an extra key.
    let refusal = parse_with_proof("{kind: review, name: verify}").expect_err("closed");
    assert_eq!(refusal.code, PlanRefusalCode::UnknownProofKey);
    // An action proof missing its step.
    let refusal = parse_with_proof("{kind: action, name: verify}").expect_err("closed");
    assert_eq!(refusal.code, PlanRefusalCode::UnknownProofKey);
    // A kind nobody defined.
    let refusal = parse_with_proof("{kind: screenshot}").expect_err("closed");
    assert_eq!(refusal.code, PlanRefusalCode::UnknownProofKind);
    // A git-ref proof does not get its own ref: the plan's delivery_ref is it.
    let refusal = parse_with_proof("{kind: git-ref, ref: refs/heads/main}").expect_err("closed");
    assert_eq!(refusal.code, PlanRefusalCode::UnknownProofKey);
    // An action step that is a shell string, not a slug.
    let refusal = parse_with_proof("{kind: action, name: verify, step: \"run && rm -rf /\"}")
        .expect_err("closed");
    assert_eq!(refusal.code, PlanRefusalCode::ActionStepNotASlug);
}

fn parse_with_proof(proof: &str) -> Result<Plan, PlanRefusal> {
    parse_plan(plan_text(proof, "refs/heads/main", "[]").as_bytes())
}

fn plan_text(proof: &str, delivery_ref: &str, retired: &str) -> String {
    format!(
        "---\nschema: {PROJECT_PLAN_SCHEMA}\nid: kettle-cli\nstatus: in-force\n\
         title: Build and land kettle\ncode_repository: pivot-test\n\
         delivery_ref: {delivery_ref}\ncriteria:\n  - id: cli-behaviour\n    \
         accept: add appends.\n    proof: {proof}\nretired_criteria: {retired}\n---\nBody.\n"
    )
}

#[test]
fn slug_grammar_is_exactly_the_contract_s() {
    for good in ["a", "0", "kettle-cli", "a-b-c", &"a".repeat(64)] {
        assert!(is_plan_slug(good), "{good:?} is a slug");
    }
    for bad in [
        "",
        "-a",
        "a-",
        "A",
        "CLI-Behaviour",
        "a_b",
        "a.b",
        "a/b",
        "a b",
        &"a".repeat(65),
    ] {
        assert!(!is_plan_slug(bad), "{bad:?} is not a slug");
    }
}

#[test]
fn a_slug_over_the_ceiling_is_refused_as_too_long_not_as_bad_grammar() {
    let long = "a".repeat(65);
    let text = plan_text("{kind: review}", "refs/heads/main", "[]").replace("cli-behaviour", &long);
    let refusal = parse_plan(text.as_bytes()).expect_err("over the ceiling");
    assert_eq!(refusal.code, PlanRefusalCode::SlugTooLong);
}

#[test]
fn a_delivery_ref_must_be_a_full_ref_without_a_glob() {
    for bad in ["main", "refs/heads/*", "refs/heads/../main", "refs/heads/"] {
        let text = plan_text("{kind: review}", bad, "[]");
        let refusal = parse_plan(text.as_bytes()).unwrap_err_for(bad);
        assert_eq!(refusal.code, PlanRefusalCode::InvalidDeliveryRef, "{bad:?}");
    }
    let text = plan_text("{kind: review}", "refs/tags/v1.0.0", "[]");
    assert!(
        parse_plan(text.as_bytes()).is_ok(),
        "a tag ref is a full ref"
    );
}

/// Small helper so a failing case names itself rather than panicking blind.
trait UnwrapErrFor {
    fn unwrap_err_for(self, what: &str) -> PlanRefusal;
}

impl UnwrapErrFor for Result<Plan, PlanRefusal> {
    fn unwrap_err_for(self, what: &str) -> PlanRefusal {
        match self {
            Ok(_) => panic!("{what:?} must be refused"),
            Err(refusal) => refusal,
        }
    }
}

#[test]
fn retired_criteria_is_required_and_written_even_when_empty() {
    let without =
        plan_text("{kind: review}", "refs/heads/main", "[]").replace("retired_criteria: []\n", "");
    let refusal = parse_plan(without.as_bytes()).expect_err("required");
    assert_eq!(refusal.code, PlanRefusalCode::MissingFrontmatterKey);
    assert_eq!(refusal.path, "frontmatter.retired_criteria");
}

#[test]
fn a_file_with_no_frontmatter_is_refused_as_that() {
    let refusal = parse_plan(b"# Kettle\n\nNo frontmatter here.\n").expect_err("no frontmatter");
    assert_eq!(refusal.code, PlanRefusalCode::MissingFrontmatter);
}

#[test]
fn an_unknown_schema_is_refused_before_its_keys_are_judged() {
    let text = plan_text("{kind: review}", "refs/heads/main", "[]")
        .replace(PROJECT_PLAN_SCHEMA, "beekeeper-plan/v2");
    let refusal = parse_plan(text.as_bytes()).expect_err("v2");
    assert_eq!(refusal.code, PlanRefusalCode::UnknownSchema);
}

#[test]
fn status_governs_new_adoption_only_and_archive_paths_refuse() {
    let mut plan = parse_plan(KETTLE.as_bytes()).expect("parse");
    assert!(check_plan_adoptable(&plan, "plans/kettle.md").is_ok());
    assert_eq!(
        check_plan_adoptable(&plan, "plans/archive/kettle.md")
            .expect_err("archived")
            .code,
        PlanRefusalCode::NotAdoptable
    );
    plan.status = PlanStatus::Superseded;
    assert_eq!(
        check_plan_adoptable(&plan, "plans/kettle.md")
            .expect_err("superseded")
            .code,
        PlanRefusalCode::NotAdoptable
    );
}

#[test]
fn a_plan_path_must_stay_under_plans() {
    assert!(validate_plan_path("plans/kettle.md").is_ok());
    assert!(validate_plan_path("plans/nested/kettle.md").is_ok());
    for bad in [
        "../actions.yml",
        "/plans/kettle.md",
        "actions.yml",
        "plans/../actions.yml",
        "plans/kettle.txt",
        "",
    ] {
        assert!(validate_plan_path(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn every_refusal_code_round_trips_through_its_string() {
    for code in [
        PlanRefusalCode::PlanTooLarge,
        PlanRefusalCode::RecycledRetiredId,
        PlanRefusalCode::ActionNameNotASlug,
        PlanRefusalCode::PlanPathSymlinked,
        PlanRefusalCode::NotAdoptable,
    ] {
        assert_eq!(plan_refusal_code_from_str(code.as_str()), Some(code));
    }
    assert_eq!(plan_refusal_code_from_str("no-such-code"), None);
}
