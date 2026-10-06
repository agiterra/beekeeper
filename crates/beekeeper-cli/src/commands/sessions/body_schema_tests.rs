//! Tests for the `--example` bodies and the full-schema refusal.
//!
//! The load-bearing test is [`every_example_passes_the_publication_validator`]:
//! an example is only worth printing if the relay would accept it, and the
//! whole point of generating them from the validating types is that a drift
//! fails here rather than in a live mission (ledger 182).

use super::*;

use beekeeper_core::coding_session_team_transaction::{
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};

/// Every verb that takes a `--body`, with the subcommand name it is reached by.
const BODY_VERBS: [(&str, CodingSessionTeamTransactionType); 6] = [
    ("assign", CodingSessionTeamTransactionType::Assignment),
    ("report", CodingSessionTeamTransactionType::Report),
    ("verdict", CodingSessionTeamTransactionType::Verdict),
    (
        "acknowledge",
        CodingSessionTeamTransactionType::Acknowledgement,
    ),
    (
        "complete",
        CodingSessionTeamTransactionType::MissionCompleted,
    ),
    ("block", CodingSessionTeamTransactionType::MissionBlocked),
];

const SESSION_REF: &str = "4b0f4a2e-4a1e-4c5f-9c1d-9a0e4b0f4a2e";

/// Decode a body exactly as the publish path does, then validate the whole
/// payload under the publication stance.
fn round_trip(
    transaction_type: CodingSessionTeamTransactionType,
    value: &Value,
) -> Result<(), String> {
    let body = match transaction_type {
        CodingSessionTeamTransactionType::Assignment => {
            CodingSessionTeamTransactionBody::Assignment(
                serde_json::from_value(value.clone()).map_err(|error| error.to_string())?,
            )
        }
        CodingSessionTeamTransactionType::Report => CodingSessionTeamTransactionBody::Report(
            serde_json::from_value(value.clone()).map_err(|error| error.to_string())?,
        ),
        CodingSessionTeamTransactionType::Verdict => CodingSessionTeamTransactionBody::Verdict(
            serde_json::from_value(value.clone()).map_err(|error| error.to_string())?,
        ),
        CodingSessionTeamTransactionType::Acknowledgement => {
            CodingSessionTeamTransactionBody::Acknowledgement(
                serde_json::from_value(value.clone()).map_err(|error| error.to_string())?,
            )
        }
        CodingSessionTeamTransactionType::MissionCompleted => {
            CodingSessionTeamTransactionBody::MissionCompleted(
                serde_json::from_value(value.clone()).map_err(|error| error.to_string())?,
            )
        }
        CodingSessionTeamTransactionType::MissionBlocked => {
            CodingSessionTeamTransactionBody::MissionBlocked(
                serde_json::from_value(value.clone()).map_err(|error| error.to_string())?,
            )
        }
        other => return Err(format!("{} takes no JSON body", other.as_str())),
    };
    let payload = CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.to_owned(),
        session_ref: SESSION_REF.to_owned(),
        genesis_ref: PLACEHOLDER_EVENT_ID.to_owned(),
        transaction_type,
        supersedes: None,
        delivery_command_id: None,
        body,
    };
    payload.validate()
}

#[test]
fn every_example_passes_the_publication_validator() {
    for (command, transaction_type) in BODY_VERBS {
        let examples = team_body_examples(transaction_type).expect("examples serialize");
        assert!(
            !examples.is_empty(),
            "`bee sessions {command}` takes --body but offers no example"
        );
        for example in examples {
            round_trip(transaction_type, &example.value).unwrap_or_else(|error| {
                panic!(
                    "`bee sessions {command} --example {}` is not publishable: {error}",
                    example.label
                )
            });
        }
    }
}

#[test]
fn every_example_is_accepted_by_the_same_check_a_write_runs() {
    for (command, transaction_type) in BODY_VERBS {
        for example in team_body_examples(transaction_type).expect("examples serialize") {
            check_body(command, transaction_type, example.value.clone()).unwrap_or_else(|error| {
                panic!(
                    "`bee sessions {command} --example {}` failed the write check: {error}",
                    example.label
                )
            });
        }
    }
}

#[test]
fn verification_role_examples_carry_a_base_sha() {
    let examples =
        team_body_examples(CodingSessionTeamTransactionType::Assignment).expect("examples");
    for role in ROLES_REQUIRING_VERIFICATION_INPUT {
        let example = examples
            .iter()
            .find(|example| example.label == role)
            .unwrap_or_else(|| panic!("assign offers no {role} example"));
        assert!(
            example
                .value
                .get("baseSha")
                .and_then(Value::as_str)
                .is_some(),
            "the {role} example must show the field the fence reads"
        );
    }
    let builder = examples
        .iter()
        .find(|example| example.label == "builder")
        .expect("assign offers a builder example");
    assert_eq!(builder.value.get("baseSha"), Some(&Value::Null));
}

#[test]
fn a_missing_field_refusal_names_every_required_key_and_the_example_command() {
    // Exactly the shape ledger 178(c) recorded: a body with one field, probed
    // to find out what the others are called.
    let error = check_body(
        "assign",
        CodingSessionTeamTransactionType::Assignment,
        serde_json::json!({ "objective": "do the thing" }),
    )
    .expect_err("a one-field assignment body must be refused");
    let message = error.to_string();
    for key in [
        "assigneeActor",
        "assigneeRole",
        "objective",
        "brief",
        "branch",
        "baseSha",
        "fileOwnership",
        "acceptanceSteps",
    ] {
        assert!(
            message.contains(key),
            "the refusal must name {key}; got: {message}"
        );
    }
    assert!(
        message.contains("bee sessions assign --example"),
        "the refusal must name the command that prints a valid body; got: {message}"
    );
    assert!(
        message.contains("builder, verifier, runner"),
        "an assignment refusal must offer the per-role examples; got: {message}"
    );
}

#[test]
fn an_empty_body_refusal_names_every_required_key_for_each_verb() {
    for (command, transaction_type) in BODY_VERBS {
        let error = check_body(command, transaction_type, serde_json::json!({}))
            .expect_err("an empty body must be refused");
        let message = error.to_string();
        let examples = team_body_examples(transaction_type).expect("examples");
        for key in required_keys(&examples) {
            assert!(
                message.contains(&key),
                "`{command}` refusal must name {key}; got: {message}"
            );
        }
        assert!(
            message.contains(&format!("bee sessions {command} --example")),
            "`{command}` refusal must name its --example; got: {message}"
        );
    }
}

#[test]
fn a_verdict_refusal_names_both_subtypes_and_their_keys() {
    let error = check_body(
        "verdict",
        CodingSessionTeamTransactionType::Verdict,
        serde_json::json!({ "summary": "s", "findings": ["f"] }),
    )
    .expect_err("the placeholder verdict of ledger 178(c) must be refused locally");
    let message = error.to_string();
    for key in ["subtype", "assignmentRef", "reportRef", "refutationRef"] {
        assert!(
            message.contains(key),
            "the verdict refusal must name {key}; got: {message}"
        );
    }
    assert!(message.contains("refutation, disposition"), "{message}");
}

#[test]
fn an_unknown_example_label_lists_the_ones_that_exist() {
    let error = print_team_body_example(
        "assign",
        CodingSessionTeamTransactionType::Assignment,
        "reviewer",
    )
    .expect_err("an unknown label must be refused");
    let message = error.to_string();
    assert!(message.contains("reviewer"), "{message}");
    assert!(message.contains("builder, verifier, runner"), "{message}");
}

#[test]
fn a_verb_with_no_json_body_says_so_rather_than_printing_nothing() {
    let error = print_team_body_example("note", CodingSessionTeamTransactionType::Note, "default")
        .expect_err("note takes no --body");
    assert!(error.to_string().contains("takes no --body JSON"));
}

#[test]
fn required_keys_are_read_off_the_examples_not_restated() {
    let examples = team_body_examples(CodingSessionTeamTransactionType::Report).expect("examples");
    let keys = required_keys(&examples);
    let object = examples[0].value.as_object().expect("report is an object");
    assert_eq!(keys.len(), object.len());
    for key in object.keys() {
        assert!(keys.contains(key), "{key} missing from the required list");
    }
}

#[test]
fn an_absent_envelope_flag_is_named_before_any_relay_read() {
    let args = TeamTransactionWriteArgs {
        channel: String::new(),
        session_ref: SESSION_REF.to_owned(),
        genesis: PLACEHOLDER_EVENT_ID.to_owned(),
        body: "{}".to_owned(),
        supersedes: None,
        delivery_command_id: None,
        wake_to: None,
        example: None,
        verifies: None,
        // Lane 201: the completion coverage gate's two flags.
        without_coverage: None,
        agents_repo: None,
    };
    let error = require_envelope("assign", &args).expect_err("an absent --channel is refused");
    assert!(error.to_string().contains("--channel is required"));
}

#[test]
fn a_complete_envelope_passes() {
    let args = TeamTransactionWriteArgs {
        channel: "85b8db75-0000-4000-8000-000000000000".to_owned(),
        session_ref: SESSION_REF.to_owned(),
        genesis: PLACEHOLDER_EVENT_ID.to_owned(),
        body: "{}".to_owned(),
        supersedes: None,
        delivery_command_id: None,
        wake_to: None,
        example: None,
        verifies: None,
        // Lane 201: the completion coverage gate's two flags.
        without_coverage: None,
        agents_repo: None,
    };
    assert!(require_envelope("assign", &args).is_ok());
}

#[test]
fn a_body_read_from_a_literal_is_parsed_and_a_bad_one_says_what_the_value_may_be() {
    let value = read_body_argument("{\"a\":1}").expect("literal JSON parses");
    assert_eq!(value.get("a").and_then(Value::as_u64), Some(1));
    let error = read_body_argument("not json").expect_err("garbage is refused");
    assert!(error.to_string().contains("`@path`"), "{error}");
}

#[test]
fn verifies_fills_an_absent_base_sha_from_the_report() {
    let mut body = team_body_examples(CodingSessionTeamTransactionType::Assignment)
        .expect("examples")[1]
        .value
        .clone();
    body.as_object_mut()
        .expect("an object")
        .insert("baseSha".to_owned(), Value::Null);
    merge_base_sha(&mut body, "fa927fd", PLACEHOLDER_EVENT_ID)
        .expect("an absent baseSha is filled");
    assert_eq!(body.get("baseSha").and_then(Value::as_str), Some("fa927fd"));
}

#[test]
fn verifies_accepts_a_base_sha_that_agrees() {
    let mut body = serde_json::json!({ "baseSha": PLACEHOLDER_SHA });
    merge_base_sha(&mut body, PLACEHOLDER_SHA, PLACEHOLDER_EVENT_ID)
        .expect("agreement is not a conflict");
    assert_eq!(
        body.get("baseSha").and_then(Value::as_str),
        Some(PLACEHOLDER_SHA)
    );
}

#[test]
fn verifies_refuses_a_base_sha_that_disagrees_with_the_report() {
    // Ledger 178(d): the assignment named `fa927fd` in prose and carried
    // `baseSha: e682191`, and the fence sent the verifier to `e682191`.
    let mut body = serde_json::json!({ "baseSha": "e682191" });
    let error = merge_base_sha(&mut body, "fa927fd", PLACEHOLDER_EVENT_ID)
        .expect_err("two different commits must be refused, not silently resolved");
    let message = error.to_string();
    assert!(message.contains("e682191"), "{message}");
    assert!(message.contains("fa927fd"), "{message}");
    assert!(message.contains("baseSha"), "{message}");
    // The body is left exactly as the caller wrote it.
    assert_eq!(body.get("baseSha").and_then(Value::as_str), Some("e682191"));
}

#[test]
fn verdict_assignment_ref_agreeing_with_the_report_is_accepted() {
    check_verdict_assignment_ref(
        PLACEHOLDER_EVENT_ID,
        PLACEHOLDER_EVENT_ID_2,
        PLACEHOLDER_EVENT_ID,
    )
    .expect("the verdict names the same assignment the report names");
}

#[test]
fn verdict_assignment_ref_naming_the_verifiers_own_assignment_is_refused() {
    // Control run 6 (2026-09-24): verdict 3489665f72bc... set assignmentRef
    // to the VERIFIER's own assignment (3fb377a6...) while its reportRef
    // (08247d54...) named the builder's report, whose own assignmentRef was
    // e8047f31... (the builder's assignment). That mismatch silently excluded
    // the verdict from `validate_causal_types` as a WrongTypeReference
    // (coding_session_team_transaction_fold_defects.rs), so the independent
    // verdict was lost and the lead signed both approvals itself.
    let verifiers_own_assignment = "3fb377a6".repeat(8);
    let report_ref = "08247d54".repeat(8);
    let builders_assignment = "e8047f31".repeat(8);
    let error =
        check_verdict_assignment_ref(&verifiers_own_assignment, &report_ref, &builders_assignment)
            .expect_err(
            "a verdict naming its own author's assignment instead of the report's must be refused",
        );
    let message = error.to_string();
    assert!(
        message.contains(&builders_assignment),
        "refusal must name the report's assignmentRef so the fix is one copy-paste: {message}"
    );
    assert!(message.contains(&report_ref), "{message}");
    assert!(message.contains("assignmentRef"), "{message}");
}
