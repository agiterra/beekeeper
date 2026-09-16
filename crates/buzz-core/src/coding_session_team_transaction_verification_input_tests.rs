//! A verification role's assignment must name the commit it judges.
//!
//! A child of `coding_session_team_transaction_tests`, split out only to keep
//! every file under 1,000 lines. `use super::*` gives it that module's fixture
//! helpers unchanged.
//!
//! A hired seat's worktree is cut from the trunk, not from the revision under
//! judgement, so a verifier or runner that is handed no `baseSha` answers about
//! whatever main happened to be. These tests pin the rule that refuses that
//! assignment at the schema, the shape rule it shares with every other git
//! object id, and the one place the roles are named.

use super::*;

/// Build an assignment payload in `role` carrying `base_sha` as supplied.
fn assignment_in_role(role: &str, base_sha: Option<&str>) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: id("ab"),
        transaction_type: CodingSessionTeamTransactionType::Assignment,
        supersedes: None,
        delivery_command_id: None,
        body: CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
            assignee_actor: id("cd"),
            assignee_role: role.to_owned(),
            objective: "Answer about the candidate".to_owned(),
            brief: "Run the gates against the named commit and report.".to_owned(),
            branch: Some("work/candidate".to_owned()),
            base_sha: base_sha.map(str::to_owned),
            file_ownership: Vec::new(),
            acceptance_steps: vec!["cargo test -p buzz-core".to_owned()],
        }),
    }
}

/// The same assignment as JSON, for the strict decode path.
fn assignment_json(role: &str, base_sha: Value) -> String {
    envelope(
        "assignment",
        Value::Null,
        serde_json::json!({
            "assigneeActor": id("cd"),
            "assigneeRole": role,
            "objective": "Answer about the candidate",
            "brief": "Run the gates against the named commit and report.",
            "branch": "work/candidate",
            "baseSha": base_sha,
            "fileOwnership": [],
            "acceptanceSteps": ["cargo test -p buzz-core"],
        }),
    )
}

/// The same assignment with the `baseSha` key left out altogether.
fn assignment_json_without_the_key(role: &str) -> String {
    envelope(
        "assignment",
        Value::Null,
        serde_json::json!({
            "assigneeActor": id("cd"),
            "assigneeRole": role,
            "objective": "Answer about the candidate",
            "brief": "Run the gates against the named commit and report.",
            "branch": "work/candidate",
            "fileOwnership": [],
            "acceptanceSteps": ["cargo test -p buzz-core"],
        }),
    )
}

/// Tolerance is for `"baseSha": null`, not for a key that is not there.
///
/// Worth pinning because the distinction is invisible from the ledger wording
/// "tolerant of an absent value": the recorded stance relaxes the *rule* about
/// what a verifier assignment must name, not the *schema* about which keys an
/// assignment has. Anyone hand-writing an old-shape fixture will reach for the
/// shorter JSON and get a decode error rather than the tolerance they expected.
#[test]
fn a_missing_base_sha_key_is_not_the_same_as_a_null_one() {
    for role in ROLES_REQUIRING_VERIFICATION_INPUT {
        // Present-as-null: accepted by the recorded reader, which is what lets
        // the one already-published assignment keep folding.
        assert!(
            decode_recorded_coding_session_team_transaction(&assignment_json(role, Value::Null))
                .is_ok(),
            "{role}: an explicit null must still be tolerated"
        );

        // Absent entirely: refused by both stances, and not with the rule's
        // sentence — this never reaches the rule.
        let error =
            decode_recorded_coding_session_team_transaction(&assignment_json_without_the_key(role))
                .unwrap_err();
        assert_ne!(
            error,
            missing_verification_input_message(role),
            "{role}: a missing key must not be reported as the role rule"
        );
        assert!(
            decode_coding_session_team_transaction(&assignment_json_without_the_key(role)).is_err(),
            "{role}: the publication path refuses it too"
        );
    }
}

#[test]
fn a_verifier_assignment_without_a_base_sha_is_refused_naming_the_field_and_the_role() {
    let error = assignment_in_role("verifier", None).validate().unwrap_err();
    assert_eq!(
        error,
        "baseSha is required for a verifier assignment: a verification input must name the exact commit to verify"
    );
    assert!(error.contains("baseSha"), "the refusal names the field");
    assert!(error.contains("verifier"), "the refusal names the role");
}

#[test]
fn a_runner_assignment_without_a_base_sha_is_refused_naming_the_field_and_the_role() {
    assert_eq!(
        assignment_in_role("runner", None).validate().unwrap_err(),
        "baseSha is required for a runner assignment: a verification input must name the exact commit to verify"
    );
}

#[test]
fn a_builder_assignment_without_a_base_sha_is_accepted() {
    assert!(assignment_in_role("builder", None).validate().is_ok());
}

#[test]
fn a_role_outside_the_set_needs_no_verification_input() {
    for role in [
        "lead",
        "designer",
        "refuter",
        "finalizer",
        "builder-secondary",
    ] {
        assert!(
            assignment_in_role(role, None).validate().is_ok(),
            "{role} does not answer about a revision and needs no input"
        );
    }
}

#[test]
fn a_verification_input_may_be_forty_or_sixty_four_hex() {
    // 40 hex is a sha1 repository's object id; 64 hex is a sha256 one's. Both
    // are valid object ids, so both are accepted — the rule is the shared shape
    // rule, not a second hand-rolled hex check.
    for input in [&"1".repeat(40), &"a".repeat(64)] {
        assert!(
            assignment_in_role("verifier", Some(input))
                .validate()
                .is_ok(),
            "a {}-hex object id is a valid verification input",
            input.len()
        );
        assert!(assignment_in_role("runner", Some(input)).validate().is_ok());
    }
}

#[test]
fn a_malformed_verification_input_is_refused_by_the_shared_shape_rule() {
    // Shape is judged before presence, so the role rule never masks the
    // sha rule's more specific refusal.
    for malformed in [
        "1".repeat(39),
        "1".repeat(41),
        "A".repeat(40),
        "g".repeat(40),
        String::new(),
    ] {
        assert_eq!(
            assignment_in_role("verifier", Some(&malformed))
                .validate()
                .unwrap_err(),
            "baseSha must be a lowercase 40- or 64-hex git object id",
            "malformed input {malformed:?}"
        );
    }
}

#[test]
fn the_refusal_survives_the_strict_decode_path() {
    // The decode path is the choke point every writer crosses: it ends in
    // `payload.validate()`, so the rule is inherited rather than restated.
    for role in ROLES_REQUIRING_VERIFICATION_INPUT {
        assert_eq!(
            decode_coding_session_team_transaction(&assignment_json(role, Value::Null)).unwrap_err(),
            format!(
                "baseSha is required for a {role} assignment: a verification input must name the exact commit to verify"
            )
        );
        assert!(decode_coding_session_team_transaction(&assignment_json(
            role,
            Value::String("9".repeat(40)),
        ))
        .is_ok());
    }
    assert!(
        decode_coding_session_team_transaction(&assignment_json("builder", Value::Null)).is_ok()
    );
}

#[test]
fn the_roles_requiring_an_exact_input_are_named_once() {
    assert_eq!(ROLES_REQUIRING_VERIFICATION_INPUT, ["verifier", "runner"]);
    for role in ROLES_REQUIRING_VERIFICATION_INPUT {
        assert!(role_requires_verification_input(role));
        assert_eq!(
            missing_verification_input_message(role),
            assignment_in_role(role, None).validate().unwrap_err(),
            "the exported message is the one the validator returns"
        );
    }
    for role in ["builder", "lead", "verifier-secondary", "runner-2", ""] {
        assert!(
            !role_requires_verification_input(role),
            "{role} is not an exact member of the set"
        );
    }
}

// --- The publication / recorded split.
//
// The rule above governs what a writer may sign. It must not govern what a
// reader may read: verifier assignment `0aaf3387`, one of 77 kind-44244 events
// live on hive when this rule was written, was published on 2026-09-14 with no
// `baseSha`, and `fold_coding_session_team_transactions` fails **whole** on a
// single invalid envelope rather than excluding one row. A strict reader would
// therefore cost that mission its entire history.

/// Sign an assignment in `role` with `base_sha`, as a publisher that predates
/// the rule would have signed it.
fn published_assignment_event(role: &str, base_sha: Option<&str>, keys: &Keys) -> Event {
    event_with_keys(&assignment_in_role(role, base_sha), keys)
}

#[test]
fn an_already_published_verifier_assignment_without_an_input_still_reads_and_folds() {
    let founder = Keys::generate();
    let recorded = published_assignment_event("verifier", None, &founder);

    // The reader path every consumer crosses: the fold, the relay's ingest
    // arm, Pulse, verdict admission, the provider's wake router, the desktop.
    let payload = validate_coding_session_team_transaction_envelope(&recorded)
        .expect("a record signed before the rule still reads");
    let CodingSessionTeamTransactionBody::Assignment(body) = &payload.body else {
        panic!("expected an assignment body");
    };
    assert_eq!(body.assignee_role, "verifier");
    assert!(body.base_sha.is_none());

    let context = CodingSessionTeamFoldContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: id("ab"),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: Vec::new(),
        active_grants: Vec::new(),
        verifier_required: false,
    };
    let fold = fold_coding_session_team_transactions(std::slice::from_ref(&recorded), &context)
        .expect("the whole mission still folds");
    let recorded_id = recorded.id.to_hex();
    assert!(
        fold.included_event_ids.contains(&recorded_id),
        "the assignment is present, not dropped: {:?}",
        fold.excluded
    );
    assert_eq!(fold.assignments.len(), 1);
    assert_eq!(fold.assignments[0].assignment_event_id, recorded_id);
    assert!(fold.excluded.is_empty(), "nothing was excluded for it");
}

#[test]
fn a_new_verification_assignment_without_an_input_is_still_refused_before_signing() {
    for role in ROLES_REQUIRING_VERIFICATION_INPUT {
        let expected = missing_verification_input_message(role);
        // The publication path: what a signer round-trips through.
        assert_eq!(
            decode_coding_session_team_transaction(&assignment_json(role, Value::Null))
                .unwrap_err(),
            expected
        );
        assert_eq!(
            assignment_in_role(role, None).validate().unwrap_err(),
            expected
        );
        // ...and the recorded path is tolerant of exactly this one absence.
        assert!(
            decode_recorded_coding_session_team_transaction(&assignment_json(role, Value::Null))
                .is_ok()
        );
        assert!(assignment_in_role(role, None).validate_recorded().is_ok());
    }
}

#[test]
fn a_malformed_verification_input_is_refused_on_both_paths() {
    // Tolerance is for the absent field, never for a malformed one: a record
    // that names a commit must name a real object id however old it is.
    for role in ["verifier", "runner", "builder"] {
        for malformed in ["1".repeat(39), "Z".repeat(40), "  ".to_owned()] {
            let json = assignment_json(role, Value::String(malformed.clone()));
            let expected = "baseSha must be a lowercase 40- or 64-hex git object id";
            assert_eq!(
                decode_coding_session_team_transaction(&json).unwrap_err(),
                expected,
                "{role} publication, {malformed:?}"
            );
            assert_eq!(
                decode_recorded_coding_session_team_transaction(&json).unwrap_err(),
                expected,
                "{role} recorded, {malformed:?}"
            );
            assert_eq!(
                assignment_in_role(role, Some(&malformed))
                    .validate_recorded()
                    .unwrap_err(),
                expected
            );
        }
    }
    // And an envelope carrying one is refused on the reader path too, so a
    // malformed record never reaches the fold under the guise of history.
    let malformed =
        published_assignment_event("verifier", Some("1".repeat(39).as_str()), &Keys::generate());
    assert_eq!(
        validate_coding_session_team_transaction_envelope(&malformed).unwrap_err(),
        "baseSha must be a lowercase 40- or 64-hex git object id"
    );
}

#[test]
fn a_builder_assignment_is_unaffected_on_both_paths() {
    for base_sha in [Value::Null, Value::String("1".repeat(40))] {
        assert!(decode_coding_session_team_transaction(&assignment_json(
            "builder",
            base_sha.clone()
        ))
        .is_ok());
        assert!(
            decode_recorded_coding_session_team_transaction(&assignment_json("builder", base_sha))
                .is_ok()
        );
    }
    assert!(assignment_in_role("builder", None).validate().is_ok());
    assert!(assignment_in_role("builder", None)
        .validate_recorded()
        .is_ok());
    let published = published_assignment_event("builder", None, &Keys::generate());
    assert!(validate_coding_session_team_transaction_envelope(&published).is_ok());
}
