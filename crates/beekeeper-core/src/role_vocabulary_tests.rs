use super::*;

fn roles(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn the_default_is_lead_and_verifier_then_runner() {
    let vocabulary = RoleVocabulary::default();
    assert_eq!(vocabulary.lead(), "lead");
    assert_eq!(vocabulary.verdict_role(), "verifier");
    assert_eq!(vocabulary.verification_roles(), ["verifier", "runner"]);
    assert!(vocabulary.verifies_a_commit("verifier"));
    assert!(vocabulary.verifies_a_commit("runner"));
    assert!(!vocabulary.verifies_a_commit("builder"));
    assert!(!vocabulary.verifies_a_commit("lead"));
    assert!(vocabulary.is_lead("lead"));
    assert!(!vocabulary.is_lead("captain"));
    assert!(vocabulary.is_verdict_role("verifier"));
    assert!(
        !vocabulary.is_verdict_role("runner"),
        "a runner names its commit but does not refute"
    );
    assert!(vocabulary.is_default());
    assert_eq!(*RoleVocabulary::foundation(), vocabulary);
}

#[test]
fn absent_fields_apply_the_default_rule_one_at_a_time() {
    let lead_only = RoleVocabulary::from_team(Some("captain"), None).expect("parses");
    assert_eq!(lead_only.lead(), "captain");
    assert_eq!(lead_only.verification_roles(), ["verifier", "runner"]);
    assert!(!lead_only.is_default());

    let verification_only = RoleVocabulary::from_team(None, Some(&roles(&["qa"]))).expect("parses");
    assert_eq!(verification_only.lead(), "lead");
    assert_eq!(verification_only.verdict_role(), "qa");
    assert!(verification_only.verifies_a_commit("qa"));
    assert!(!verification_only.verifies_a_commit("verifier"));

    assert_eq!(
        RoleVocabulary::from_team(None, None).expect("parses"),
        RoleVocabulary::default()
    );
}

#[test]
fn captain_and_qa_is_a_whole_vocabulary() {
    let vocabulary =
        RoleVocabulary::from_team(Some("captain"), Some(&roles(&["qa"]))).expect("parses");
    assert!(vocabulary.is_lead("captain"));
    assert!(
        !vocabulary.is_lead("lead"),
        "the foundation's name is not this project's"
    );
    assert!(vocabulary.is_verdict_role("qa"));
    assert!(!vocabulary.is_verdict_role("verifier"));
    assert!(vocabulary.verifies_a_commit("qa"));
    assert!(!vocabulary.verifies_a_commit("runner"));
}

#[test]
fn the_first_verification_role_judges_and_the_rest_only_name_their_commit() {
    let vocabulary =
        RoleVocabulary::from_team(None, Some(&roles(&["qa", "bench"]))).expect("parses");
    assert_eq!(vocabulary.verdict_role(), "qa");
    assert!(vocabulary.verifies_a_commit("bench"));
    assert!(!vocabulary.is_verdict_role("bench"));
}

#[test]
fn bad_tokens_empty_lists_duplicates_and_a_judging_lead_are_refused() {
    let error = RoleVocabulary::from_team(Some("Captain"), None).unwrap_err();
    assert!(
        error.contains("lead") && error.contains("[a-z0-9-]"),
        "{error}"
    );

    let error = RoleVocabulary::from_team(None, Some(&roles(&["qa", "Q A"]))).unwrap_err();
    assert!(error.contains("verification_roles"), "{error}");

    let error = RoleVocabulary::from_team(None, Some(&[])).unwrap_err();
    assert!(error.contains("omit the key"), "{error}");

    let error = RoleVocabulary::from_team(None, Some(&roles(&["qa", "qa"]))).unwrap_err();
    assert!(error.contains("twice"), "{error}");

    let error = RoleVocabulary::from_team(Some("qa"), Some(&roles(&["qa"]))).unwrap_err();
    assert!(error.contains("also a verification role"), "{error}");
}

#[test]
fn the_wire_shape_round_trips() {
    let vocabulary =
        RoleVocabulary::from_team(Some("captain"), Some(&roles(&["qa"]))).expect("parses");
    let json = serde_json::to_value(&vocabulary).expect("serializes");
    assert_eq!(
        json,
        serde_json::json!({"lead": "captain", "verification": ["qa"]})
    );
    let back: RoleVocabulary = serde_json::from_value(json).expect("deserializes");
    assert_eq!(back, vocabulary);
}
