//! The project-action authority row (ledger 186).

use super::*;

const PROJECT_OWNER: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SOMEONE_ELSE: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const ROSTER_OWNER: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const PROJECT: &str =
    "30621:1111111111111111111111111111111111111111111111111111111111111111:kettle";

fn owners(keys: &[&str]) -> ProjectActionOwnership {
    ProjectActionOwnership::Owners(keys.iter().map(|key| (*key).to_owned()).collect())
}

#[test]
fn a_project_owner_founding_a_team_delegates_its_actions() {
    let fact = project_action_authority_fact(Some(PROJECT_OWNER), &owners(&[PROJECT_OWNER]));
    assert_eq!(fact.code, PROJECT_ACTIONS_DELEGABLE);
    assert_eq!(fact.state, TeamReadinessFactState::Ready);
    assert_eq!(fact.scope, TeamReadinessScope::Wire);
    assert_eq!(fact.category, "actions");
    assert!(fact.remedy.is_none(), "a satisfied row prescribes nothing");
}

#[test]
fn a_roster_owner_counts_as_an_owner() {
    let fact =
        project_action_authority_fact(Some(ROSTER_OWNER), &owners(&[PROJECT_OWNER, ROSTER_OWNER]));
    assert_eq!(fact.code, PROJECT_ACTIONS_DELEGABLE);
}

#[test]
fn a_non_owner_founder_is_told_who_can_grant_it() {
    let fact =
        project_action_authority_fact(Some(SOMEONE_ELSE), &owners(&[PROJECT_OWNER, ROSTER_OWNER]));
    assert_eq!(fact.code, PROJECT_ACTIONS_NOT_DELEGABLE);
    assert_eq!(fact.state, TeamReadinessFactState::Limited);
    let summary = fact.summary;
    assert!(summary.contains("11111111"), "{summary}");
    assert!(summary.contains("33333333"), "{summary}");
    let remedy = fact.remedy.expect("a remedy names the way out");
    assert!(remedy.contains("owners"), "{remedy}");
    assert!(
        remedy.contains("plan, build, review and land"),
        "the row must not overstate what is refused: {remedy}"
    );
}

#[test]
fn an_unreadable_project_is_unknown_and_never_ready() {
    let fact = project_action_authority_fact(
        Some(PROJECT_OWNER),
        &ProjectActionOwnership::Unreadable {
            detail: "the relay did not answer".into(),
        },
    );
    assert_eq!(fact.code, PROJECT_ACTIONS_AUTHORITY_UNKNOWN);
    assert_eq!(fact.state, TeamReadinessFactState::Unknown);
    // Wire scope: it warns, it does not gate Start — a question about actions
    // must not refuse a session that never publishes one.
    assert_eq!(fact.scope, TeamReadinessScope::Wire);
    assert!(fact.summary.contains("the relay did not answer"));
}

#[test]
fn an_unknown_founder_key_is_unknown_not_unauthorized() {
    let fact = project_action_authority_fact(None, &owners(&[PROJECT_OWNER]));
    assert_eq!(fact.code, PROJECT_ACTIONS_AUTHORITY_UNKNOWN);
    assert_eq!(fact.state, TeamReadinessFactState::Unknown);
}

#[test]
fn a_project_with_no_named_owner_says_exactly_that() {
    let fact = project_action_authority_fact(Some(SOMEONE_ELSE), &owners(&[]));
    assert_eq!(fact.code, PROJECT_ACTIONS_NOT_DELEGABLE);
    assert!(fact.summary.contains("no owner at all"), "{}", fact.summary);
}

#[test]
fn only_a_project_coordinate_is_read() {
    assert_eq!(
        split_project_coordinate(PROJECT),
        Some((PROJECT_OWNER.to_owned(), "kettle".to_owned()))
    );
    for bad in [
        "30617:1111111111111111111111111111111111111111111111111111111111111111:repo",
        "30621:NOTHEX:kettle",
        "30621:1111111111111111111111111111111111111111111111111111111111111111:",
        "kettle",
    ] {
        assert!(split_project_coordinate(bad).is_none(), "accepted {bad:?}");
    }
}

#[test]
fn more_than_three_owners_are_counted_rather_than_listed() {
    let many: Vec<String> = (0..5).map(|index| format!("{index}").repeat(64)).collect();
    let line = name_owners(&many);
    assert!(line.contains("and 2 more"), "{line}");
}
