//! `bee plans example`: the example is a valid, adoptable, task-neutral
//! `beekeeper-plan/v1` file, by the real parser and the real validator.

use buzz_core::project_plan::{
    check_plan_adoptable, parse_plan, PlanProof, PlanStatus, PROJECT_PLAN_SCHEMA,
};

use super::{DESCRIBED_ACTION_PROOF, EXAMPLE};

#[test]
fn the_example_parses_and_is_adoptable_by_the_real_contract() {
    let plan = parse_plan(EXAMPLE.as_bytes()).expect("the example is a valid plan");
    assert_eq!(plan.schema, PROJECT_PLAN_SCHEMA);
    assert_eq!(plan.id, "example-plan");
    assert_eq!(plan.status, PlanStatus::InForce);
    assert_eq!(plan.code_repository, "example-code-repository");
    assert_eq!(plan.delivery_ref, "refs/heads/main");
    assert_eq!(
        plan.criteria
            .iter()
            .map(|criterion| (criterion.id.as_str(), &criterion.proof))
            .collect::<Vec<_>>(),
        vec![
            ("example-behaviour", &PlanProof::Review),
            ("delivered", &PlanProof::GitRef),
        ]
    );
    assert!(plan.retired_criteria.is_empty());
    check_plan_adoptable(&plan, "plans/example-plan.md")
        .expect("adoptable where it says to save it");
}

/// The `action` form the example only describes, written exactly as the
/// comment shows it, parses to the action and step it names.
#[test]
fn the_described_action_proof_is_real_syntax() {
    assert!(EXAMPLE.contains(DESCRIBED_ACTION_PROOF));
    let with_action = EXAMPLE.replacen(
        "proof: {kind: review}",
        &format!("proof: {DESCRIBED_ACTION_PROOF}"),
        1,
    );
    let plan = parse_plan(with_action.as_bytes()).expect("the described action proof parses");
    assert_eq!(
        plan.criteria[0].proof,
        PlanProof::Action {
            name: "verify".into(),
            step: "verify".into()
        }
    );
}

/// Every value marked SUBSTITUTE is a real key: replacing each with a
/// different project's value still parses, and the replacement is what the
/// parser reads.
#[test]
fn every_substitution_point_is_the_key_it_claims() {
    let substituted: String = EXAMPLE
        .lines()
        .map(|line| {
            let key = line.split(':').next().unwrap_or_default().trim();
            match key {
                "id" => "id: search-index".to_owned(),
                "title" => "title: Index documents for search".to_owned(),
                "code_repository" => "code_repository: docs-site".to_owned(),
                "- id" if line.contains("example-behaviour") => {
                    "  - id: query-returns-matches".to_owned()
                }
                _ => line.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let plan = parse_plan(substituted.as_bytes()).expect("a substituted example parses");
    assert_eq!(plan.id, "search-index");
    assert_eq!(plan.title, "Index documents for search");
    assert_eq!(plan.code_repository, "docs-site");
    assert_eq!(plan.criteria[0].id, "query-returns-matches");
    assert_eq!(
        EXAMPLE.matches("SUBSTITUTE").count(),
        6,
        "id, title, code_repository, the criteria list, one accept, and the header naming the marker"
    );
}

/// Nothing from an earlier project's plan: no product, customer or task, and
/// it says plainly that it is an example.
#[test]
fn the_example_is_task_neutral_and_says_it_is_an_example() {
    let lower = EXAMPLE.to_ascii_lowercase();
    for earlier in [
        "kettle", "tank", "loop", "rpg", "pivot", "loom", "brian", "python", "readme",
    ] {
        assert!(!lower.contains(earlier), "carries {earlier:?}");
    }
    assert!(EXAMPLE.contains("an EXAMPLE of the shape, not a plan to commit as-is"));
    assert!(EXAMPLE.contains("This is an example of the plan format."));
}

/// The command a lead actually runs next accepts the example: saved into a
/// throwaway agents repository, `bee sessions work validate` reads it from
/// the working copy and from a commit.
#[test]
fn bee_sessions_work_validate_accepts_the_example() {
    use crate::commands::sessions::work::{cmd_validate, WorkValidateArgs};
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("plans")).expect("mkdir");
    std::fs::write(dir.join("plans/example-plan.md"), EXAMPLE).expect("write");
    let dir_arg = dir.display().to_string();
    cmd_validate(&WorkValidateArgs {
        plan: "plans/example-plan.md".into(),
        agents_repo: Some(dir_arg.clone()),
        commit: None,
    })
    .expect("the working copy validates");

    let git = |args: &[&str]| {
        let output = crate::commands::sessions::worktree::git_command(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "example")
            .env("GIT_AUTHOR_EMAIL", "example@example.invalid")
            .env("GIT_COMMITTER_NAME", "example")
            .env("GIT_COMMITTER_EMAIL", "example@example.invalid")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet", "--initial-branch=main"]);
    git(&["add", "--all"]);
    git(&["commit", "--quiet", "-m", "example"]);
    cmd_validate(&WorkValidateArgs {
        plan: "plans/example-plan.md".into(),
        agents_repo: Some(dir_arg),
        commit: Some("HEAD".into()),
    })
    .expect("the committed blob validates");
}
