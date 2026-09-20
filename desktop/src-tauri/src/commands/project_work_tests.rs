use super::*;

use std::fs;
use std::path::Path;
use std::process::Command;

const OWNER: &str = "1e";
const SESSION: &str = "11111111-2222-4333-8444-555555555555";

fn owner_hex() -> String {
    OWNER.repeat(32)
}

fn project_ref() -> String {
    format!("30621:{}:kettle", owner_hex())
}

fn repository() -> String {
    format!("30617:{}:kettle-beekeeper-agents", owner_hex())
}

fn fold_with(
    work_events: Vec<ProjectWorkEvent>,
    dir: Option<&Path>,
) -> Result<ProjectWorkResponse, String> {
    project_work_coverage_inner(
        request(work_events),
        dir.map(|path| path.display().to_string()),
    )
}

fn request(work_events: Vec<ProjectWorkEvent>) -> ProjectWorkRequest {
    ProjectWorkRequest {
        schema: PROJECT_WORK_REQUEST_SCHEMA.into(),
        session_ref: SESSION.into(),
        project_ref: project_ref(),
        founder_pubkey: owner_hex(),
        relay_self_key: None,
        active_seats: Vec::new(),
        active_grants: Vec::new(),
        work_events,
        team_events: Vec::new(),
        goal_events: Vec::new(),
        host_events: Vec::new(),
        ref_states: Vec::new(),
    }
}

/// A throwaway git repository holding one plan at one commit.
///
/// Never the real agents cache and never a worktree of this repository:
/// tooling tests that write into a live checkout are how a lane once put
/// hooks in the hot repo (ledger note, git tooling tests).
fn repo_with_plan(plan: &str, actions: Option<&str>) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git");
        assert!(
            status.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.email", "lane@example.invalid"]);
    git(&["config", "user.name", "Lane"]);
    fs::create_dir_all(path.join("plans")).expect("plans dir");
    fs::write(path.join("plans/kettle.md"), plan).expect("plan");
    if let Some(actions) = actions {
        fs::write(path.join("actions.yml"), actions).expect("actions");
    }
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "plan"]);
    let head = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse");
    let commit = String::from_utf8_lossy(&head.stdout).trim().to_string();
    (dir, commit)
}

fn plan_text() -> String {
    // Frozen-contract shape: `conformance/project-work/fixtures/plans/valid/`.
    r#"---
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Kettle
code_repository: kettle
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: The CLI does the thing.
    proof: {kind: review}
retired_criteria: []
---
# Kettle

Body.
"#
    .to_owned()
}

fn declaration_event(commit: &str, path: &str) -> ProjectWorkEvent {
    let body = serde_json::json!({
        "workId": "9d0f0f0f-1111-4222-8333-444444444444",
        "goalRef": "ab".repeat(32),
        "decisionRef": null,
        "responsibleActor": owner_hex(),
        "planRef": {"repository": repository(), "commit": commit, "path": path},
        "supersedes": [],
    });
    let content = serde_json::json!({
        "schema": "buzz-project-work/v1",
        "sessionRef": SESSION,
        "genesisRef": "9e".repeat(32),
        "projectRef": project_ref(),
        "type": "work.declared",
        "body": body,
    });
    ProjectWorkEvent {
        id: "de".repeat(32),
        pubkey: owner_hex(),
        kind: buzz_core_pkg::kind::KIND_PROJECT_WORK_RECORD,
        created_at: 10,
        tags: vec![
            vec!["h".into(), "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2".into()],
            vec!["d".into(), SESSION.into()],
            vec!["a".into(), project_ref()],
            vec!["pwk-v".into(), "buzz-project-work/v1".into()],
            vec!["pwk-genesis".into(), "9e".repeat(32)],
            vec!["pwk-type".into(), "work.declared".into()],
        ],
        content: content.to_string(),
    }
}

#[test]
fn an_unknown_request_schema_is_refused_by_name() {
    let mut input = request(Vec::new());
    input.schema = "buzz-project-work-request/v2".into();
    assert_eq!(
        project_work_coverage_inner(input, None).unwrap_err(),
        "unsupported project-work request schema \"buzz-project-work-request/v2\""
    );
}

#[test]
fn a_host_with_no_agents_clone_says_so_and_folds_unknown_not_open() {
    // The distinction this command exists to keep: "nobody has done this" and
    // "we cannot see the list" are different answers.
    let events = vec![declaration_event(&"ab".repeat(20), "plans/kettle.md")];
    let answer = fold_with(events, None).expect("fold");
    assert!(!answer.agents_repo_read);
    assert_eq!(answer.unreadable_plans.len(), 1);
    assert_eq!(answer.unreadable_plans[0].reason_code, "plan_unreadable");
    assert!(answer.unreadable_plans[0]
        .reason
        .contains("no clone of the project's agents repository"));
    let declaration = &answer.coverage.declarations[0];
    assert!(!declaration.plan_resolved);
    assert!(
        declaration
            .criteria
            .iter()
            .all(|criterion| criterion.status.as_str() == "unknown"),
        "an unread plan is unknown, never open"
    );
}

#[test]
fn a_plan_read_at_its_pinned_commit_resolves_and_folds() {
    let (dir, commit) = repo_with_plan(&plan_text(), None);
    let events = vec![declaration_event(&commit, "plans/kettle.md")];
    let answer = fold_with(events, Some(dir.path())).expect("fold");
    assert!(answer.agents_repo_read);
    assert!(
        answer.unreadable_plans.is_empty(),
        "{:?}",
        answer.unreadable_plans
    );
    let declaration = &answer.coverage.declarations[0];
    assert!(declaration.plan_resolved);
    assert_eq!(declaration.criteria.len(), 1);
    assert_eq!(declaration.criteria[0].criterion_id, "cli-behaviour");
    // Nothing has been bound, so it is open — and that is a real answer,
    // because the plan was read.
    assert_eq!(declaration.criteria[0].status.as_str(), "open");
    assert!(!declaration.coverage_complete);
}

#[test]
fn a_plan_pinned_to_a_commit_this_clone_does_not_have_names_the_git_refusal() {
    let (dir, _commit) = repo_with_plan(&plan_text(), None);
    let events = vec![declaration_event(&"cd".repeat(20), "plans/kettle.md")];
    let answer = fold_with(events, Some(dir.path())).expect("fold");
    assert_eq!(answer.unreadable_plans.len(), 1);
    assert_eq!(answer.unreadable_plans[0].reason_code, "plan_unreadable");
    assert!(
        !answer.unreadable_plans[0].reason.is_empty(),
        "git's own words, not a summary"
    );
    assert!(!answer.coverage.declarations[0].plan_resolved);
}

#[test]
fn the_response_carries_the_folds_own_schema_and_implementation() {
    let answer = fold_with(Vec::new(), None).expect("fold");
    assert_eq!(answer.schema, PROJECT_WORK_RESPONSE_SCHEMA);
    assert_eq!(answer.implementation, "buzz-core");
    assert_eq!(answer.coverage.schema, "buzz-project-work-coverage/v1");
    assert!(answer.coverage.declarations.is_empty());
}
