use std::collections::BTreeSet;
use std::path::PathBuf;

use super::*;

fn project(slug: &str) -> String {
    format!("30621:{}:{slug}", "ab".repeat(32))
}

fn agent(pubkey: &str, name: &str) -> ManagedAgentRecord {
    serde_json::from_value(serde_json::json!({
        "pubkey": pubkey,
        "name": name,
        "relay_url": "wss://relay.example",
        "acp_command": "buzz-acp",
        "agent_command": "goose",
        "agent_args": [],
        "mcp_command": "",
        "turn_timeout_seconds": 320,
        "system_prompt": null,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "last_started_at": null,
        "last_stopped_at": null,
        "last_exit_code": null,
        "last_error": null
    }))
    .expect("record fixture")
}

fn team(id: &str, name: &str) -> TeamRecord {
    TeamRecord {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        instructions: None,
        persona_ids: Vec::new(),
        crew: None,
        is_builtin: false,
        // The fact the whole ordering hangs on: a project team is never
        // directory-backed, so `delete_team_with_cascade` cascades nothing.
        source_dir: None,
        is_symlink: false,
        symlink_target: None,
        version: None,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    }
}

fn definition(id: &str, source_team: Option<&str>) -> AgentDefinition {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "display_name": id,
        "avatar_url": null,
        "system_prompt": "",
        "source_team": source_team,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
    }))
    .expect("definition fixture")
}

fn doomed(records: &[&ManagedAgentRecord]) -> BTreeSet<String> {
    records.iter().map(|record| record.pubkey.clone()).collect()
}

#[test]
fn an_agent_that_holds_the_team_but_not_the_project_is_still_enumerated() {
    // `associate_installation` writes `project_ref` *after* the installer
    // returns and can fail. Such an agent is invisible to a `project_ref`
    // enumeration but still blocks `agents_referencing_team` — which is how
    // the team delete used to become permanently impossible.
    let team = team("team-1", "Project team x");
    let mut associated = agent("a1", "Lead");
    associated.project_ref = Some(project("tank-loop"));
    associated.team_id = Some("team-1".into());
    let mut orphaned = agent("a2", "Builder");
    orphaned.team_id = Some("team-1".into());

    let agents = vec![associated, orphaned];
    let found = teardown_agents(&agents, &project("tank-loop"), Some(&team));
    assert_eq!(
        found.iter().map(|r| r.pubkey.as_str()).collect::<Vec<_>>(),
        vec!["a1", "a2"],
    );
}

#[test]
fn an_agent_matched_only_by_its_pack_directory_is_enumerated() {
    // The third arm of `agents_referencing_team`. It does not fire in
    // practice — the key is a UUID and the directory is a role slug — but
    // enumeration and refusal must be the same predicate or the deadlock
    // comes back through whichever arm was left out.
    let team = team("team-1", "Project team x");
    let mut by_dir = agent("a1", "Runner");
    by_dir.persona_team_dir = Some(PathBuf::from("/packs/installs/abc/team-1"));

    let agents = vec![by_dir];
    let found = teardown_agents(&agents, &project("tank-loop"), Some(&team));
    assert_eq!(found.len(), 1, "the pack-directory arm must be enumerated");
}

#[test]
fn removing_the_enumerated_set_clears_the_team_delete() {
    // The invariant asserted directly rather than inferred: if this is ever
    // false, the teardown deletes the agents and then refuses its own team
    // delete, stranding the definitions for good.
    let team = team("team-1", "Project team x");
    let mut by_project = agent("a1", "Lead");
    by_project.project_ref = Some(project("tank-loop"));
    let mut by_team = agent("a2", "Builder");
    by_team.team_id = Some("team-1".into());
    let mut by_dir = agent("a3", "Runner");
    by_dir.persona_team_dir = Some(PathBuf::from("/packs/installs/abc/team-1"));
    let elsewhere = agent("a4", "Someone else");

    let agents = vec![by_project, by_team, by_dir, elsewhere];
    let found = teardown_agents(&agents, &project("tank-loop"), Some(&team));
    let set = doomed(&found);
    assert!(
        enumeration_clears_the_team(&agents, &set, &team),
        "the enumerated set must be a superset of what blocks the team delete",
    );
    assert!(!set.contains("a4"), "an unrelated agent is never touched");
}

#[test]
fn a_project_arm_alone_does_not_clear_a_team_another_agent_still_holds() {
    // The failing-before case: enumerating on `project_ref` alone leaves
    // `a2` holding the team, and `delete_team_with_cascade` then refuses.
    let team = team("team-1", "Project team x");
    let mut by_project = agent("a1", "Lead");
    by_project.project_ref = Some(project("tank-loop"));
    by_project.team_id = Some("team-1".into());
    let mut by_team_only = agent("a2", "Builder");
    by_team_only.team_id = Some("team-1".into());

    let agents = vec![by_project, by_team_only];
    let project_arm_only: BTreeSet<String> = ["a1".to_string()].into_iter().collect();
    assert!(
        !enumeration_clears_the_team(&agents, &project_arm_only, &team),
        "this is the deadlock the union exists to prevent",
    );
}

#[test]
fn definitions_come_from_the_team_and_from_orphans_but_never_builtins() {
    let team = team("team-1", "Project team x");
    let mut doomed_agent = agent("a1", "Lead");
    doomed_agent.team_id = Some("team-1".into());
    doomed_agent.persona_id = Some("crew-role:orphan".into());
    let agents = vec![doomed_agent];
    let set = doomed(&agents.iter().collect::<Vec<_>>());

    let mut builtin = definition("builtin", Some("team-1"));
    builtin.is_builtin = true;
    let definitions = vec![
        definition("crew-role:lead", Some("team-1")),
        // Already orphaned by an earlier attempt that died between removing
        // the agents and removing the definitions: reclaimed here.
        definition("crew-role:orphan", None),
        definition("crew-role:other-project", Some("team-2")),
        builtin,
    ];
    let found = teardown_definitions(&definitions, &agents, &set, Some(&team));
    assert_eq!(
        found.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
        vec!["crew-role:lead", "crew-role:orphan"],
    );
}

#[test]
fn a_definition_a_surviving_agent_still_uses_is_left_alone() {
    let team = team("team-1", "Project team x");
    let mut doomed_agent = agent("a1", "Lead");
    doomed_agent.team_id = Some("team-1".into());
    doomed_agent.persona_id = Some("crew-role:shared".into());
    let mut survivor = agent("a2", "Someone else");
    survivor.persona_id = Some("crew-role:shared".into());

    let agents = vec![doomed_agent, survivor];
    let set: BTreeSet<String> = ["a1".to_string()].into_iter().collect();
    let definitions = vec![definition("crew-role:shared", None)];
    let found = teardown_definitions(&definitions, &agents, &set, Some(&team));
    assert!(
        found.is_empty(),
        "a definition another project's agent still points at is not ours to delete",
    );
}

#[test]
fn a_builtin_team_is_never_taken_for_a_project_team() {
    let mut builtin = team("team-1", "Project team x");
    builtin.is_builtin = true;
    let teams = vec![builtin];
    assert!(teardown_team(&teams, "Project team x").is_none());
}

#[test]
fn a_remote_deployed_agent_is_reported_for_refusal() {
    let mut remote = agent("a1", "Lead");
    remote.backend = BackendKind::Provider {
        id: "blox".into(),
        config: serde_json::Value::Null,
    };
    remote.backend_agent_id = Some("deployment-1".into());
    let local = agent("a2", "Builder");
    let agents = vec![remote, local];
    let set = doomed(&agents.iter().collect::<Vec<_>>());
    let found = remote_deployed_agents(&agents, &set);
    assert_eq!(
        found.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["Lead"],
    );
}
