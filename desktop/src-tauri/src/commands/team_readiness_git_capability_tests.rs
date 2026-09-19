//! Ledger 168's one readiness fact, tested apart from
//! `team_readiness_tests.rs` only because that file is at the repository's
//! 1,000-line ceiling.

use super::*;

/// Ledger 168: an old git is a Limited fact until the project's packs need a
/// fetch, and it always names the git it found.
#[test]
fn an_old_git_is_disclosed_as_limited_and_names_itself() {
    let capability = crate::commands::project_git_version::GitCapability {
        path: Some("/usr/bin/git".into()),
        version: Some("2.39.5".into()),
        meets_minimum: false,
        minimum: "2.46".into(),
    };
    let fact = git_capability_fact(&capability, false).expect("a fact");
    assert_eq!(fact.code, "GIT_TOO_OLD_FOR_RELAY");
    assert_eq!(fact.state, TeamReadinessFactState::Limited);
    assert!(
        fact.summary.contains("git 2.39.5 at /usr/bin/git"),
        "{}",
        fact.summary
    );
    assert!(fact.summary.contains("2.46 or newer"), "{}", fact.summary);

    let blocked = git_capability_fact(&capability, true).expect("a fact");
    assert_eq!(blocked.state, TeamReadinessFactState::Blocked);
    assert!(
        blocked.summary.contains("no seat could be staged"),
        "{}",
        blocked.summary
    );
}

#[test]
fn a_capable_git_says_nothing_and_a_missing_one_is_not_given_a_version() {
    assert!(git_capability_fact(
        &crate::commands::project_git_version::GitCapability {
            path: Some("/opt/homebrew/bin/git".into()),
            version: Some("2.55.0".into()),
            meets_minimum: true,
            minimum: "2.46".into(),
        },
        true,
    )
    .is_none());

    let nothing = git_capability_fact(
        &crate::commands::project_git_version::GitCapability {
            path: None,
            version: None,
            meets_minimum: false,
            minimum: "2.46".into(),
        },
        false,
    )
    .expect("a fact");
    assert!(nothing.summary.contains("no git"), "{}", nothing.summary);
}
