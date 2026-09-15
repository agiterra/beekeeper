use super::*;
use crate::managed_agents::project_team_setup::SetupStatus;

#[test]
fn encrypted_role_plan_recovers_the_same_identity_after_the_persona_write_boundary() {
    let owner = nostr::Keys::generate();
    let planned = reserve_role_identity(&owner, "lead").expect("reserve lead");
    let journal_bytes = serde_json::to_vec(&planned).expect("journal bytes");
    let recovered = recover_role_identity(&owner, &planned).expect("recover lead");

    assert_eq!(recovered.pubkey, planned.pubkey);
    assert!(!journal_bytes
        .windows(recovered.private_key_nsec.len())
        .any(|window| window == recovered.private_key_nsec.as_bytes()));
    // Simulate the first of the three independent store writes succeeding: the
    // durable plan still has the same key when the agent/team writes have not.
    let partial = InstallationJournal {
        team_id: uuid::Uuid::new_v4().to_string(),
        planned_roles: vec![planned.clone()],
        channel: None,
        roles: Vec::new(),
    };
    let reopened: InstallationJournal =
        serde_json::from_slice(&serde_json::to_vec(&partial).expect("durable partial install"))
            .expect("reopen partial install");
    assert_eq!(
        recover_role_identity(&owner, &reopened.planned_roles[0])
            .expect("reopen recovery")
            .pubkey,
        recovered.pubkey
    );
}

#[test]
fn saved_channel_request_reopens_with_the_same_uuid_and_signed_bytes() {
    let owner = nostr::Keys::generate();
    let channel_id = uuid::Uuid::new_v4();
    let draft = ProjectTeamSetupDraft {
        setup_id: uuid::Uuid::new_v4().to_string(),
        project_ref: format!("30621:{}:garden", owner.public_key().to_hex()),
        project_directory: "/tmp/project".to_string(),
        draft_directory: "/tmp/draft".to_string(),
        roles_directory: "/tmp/draft/personas/roles".to_string(),
        status: SetupStatus::Draft,
        intent: "grow".to_string(),
        owner_pubkey: owner.public_key().to_hex(),
        relay_url: "wss://example.test".to_string(),
        roles: vec!["lead".to_string()],
        expected_roles: Vec::new(),
        created_at: "2026-09-13T00:00:00Z".to_string(),
        latest_snapshot_id: None,
    };
    let saved = ActivationChannelJournal {
        channel_id: channel_id.to_string(),
        create_event: channel::channel_event(&draft, channel_id, &owner).expect("sign create"),
    };
    let reopened: ActivationChannelJournal =
        serde_json::from_slice(&serde_json::to_vec(&saved).expect("persist reservation"))
            .expect("reopen reservation");
    assert_eq!(reopened.channel_id, saved.channel_id);
    assert_eq!(reopened.create_event.id, saved.create_event.id);
    assert_eq!(
        serde_json::to_value(&reopened.create_event).expect("reopened bytes"),
        serde_json::to_value(&saved.create_event).expect("saved bytes"),
    );
}

#[test]
fn lead_staging_requires_the_exact_adopted_repo_sha_role_and_path() {
    let source = ProjectTeamActivationSource {
        repo_ref: "30617:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:packs"
            .to_string(),
        commit: "b".repeat(40),
        pack_path: "personas/roles".to_string(),
    };
    let input = exact_project_pack_source(&source);
    assert_eq!(input.repo, source.repo_ref);
    assert_eq!(input.sha.as_deref(), Some(source.commit.as_str()));
    assert_eq!(input.git_ref, None);
    assert_eq!(input.path.as_deref(), Some("personas/roles"));

    let expected = expected_pack_ref(&source, "lead");
    let staged = crate::managed_agents::actor_seats::StagedActorSeat {
        pack_staged: true,
        pack_ref: Some(expected.clone()),
    };
    require_exact_staged_pack(&staged, &expected).expect("exact pack");

    let wrong = crate::managed_agents::actor_seats::StagedActorSeat {
        pack_staged: true,
        pack_ref: Some(packs_cache::PackRef {
            sha: "c".repeat(40),
            ..expected
        }),
    };
    assert!(require_exact_staged_pack(&wrong, &expected_pack_ref(&source, "lead")).is_err());
}

#[test]
fn lead_first_message_asks_to_reconcile_earlier_staffing_instructions() {
    let source = ProjectTeamActivationSource {
        repo_ref: "30617:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:packs"
            .to_string(),
        commit: "b".repeat(40),
        pack_path: "personas/roles".to_string(),
    };
    let turn = projection::lead_initial_turn(&source, &[]);
    assert!(turn.starts_with("You are the project lead."));
    assert!(turn.contains(&source.repo_ref) && turn.contains(&source.commit));
    assert!(turn.contains(
        "Before depending on existing project instructions, reconcile any that name specific agents, reviewers, budgets or staffing arrangements from an earlier setup: keep the underlying product, security, testing and independent-review requirements, map them to available roles, and ask the owner once when the mapping is unclear. Inspect tools, configuration and repository state yourself before asking a person."
    ));
}

#[test]
fn lead_first_message_names_project_agent_discovery_and_hiring() {
    let source = ProjectTeamActivationSource {
        repo_ref: format!("30617:{}:packs", "a".repeat(64)),
        commit: "b".repeat(40),
        pack_path: "personas/roles".to_string(),
    };
    let turn = projection::lead_initial_turn(&source, &[]);
    assert!(turn.contains("`bee projects agents`"));
    assert!(turn.contains("your seat's project is the default"));
    assert!(turn.contains("`bee sessions hire`"));
    assert!(turn.contains("seats only agents that belong to this project"));
}

fn project_agent_record(
    pubkey: &str,
    name: &str,
    home_role: &str,
    project_ref: Option<&str>,
) -> ManagedAgentRecord {
    let mut record: ManagedAgentRecord = serde_json::from_value(json!({
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
    .expect("record fixture");
    record.home_role = Some(home_role.to_string());
    record.project_ref = project_ref.map(str::to_owned);
    record
}

fn installed_role(role: &str, agent_pubkey: &str) -> ProjectTeamInstalledRole {
    ProjectTeamInstalledRole {
        role: role.to_string(),
        agent_pubkey: agent_pubkey.to_string(),
        pack_ref: packs_cache::PackRef {
            repo: format!("30617:{}:packs", "a".repeat(64)),
            sha: "b".repeat(40),
            role: role.to_string(),
            path: format!("personas/roles/{role}"),
        },
    }
}

#[test]
fn lead_first_message_lists_this_projects_agents_on_the_hosting_computer() {
    let project = format!("30621:{}:tank-loop", "c".repeat(64));
    let other = format!("30621:{}:other", "c".repeat(64));
    let (lead, builder, verifier) = ("1".repeat(64), "2".repeat(64), "3".repeat(64));
    let (bob, stray, missing) = ("4".repeat(64), "5".repeat(64), "6".repeat(64));
    let agents = vec![
        project_agent_record(&lead, "Loom", "lead", Some(&project)),
        project_agent_record(&builder, "Builder", "builder", Some(&project)),
        // Associated by hand on the Agents tab, not by installation.
        project_agent_record(&verifier, "Vera", "verifier", Some(&project)),
        // Another project's builder and an unassociated runner are not listed.
        project_agent_record(&bob, "Bob", "builder", Some(&other)),
        project_agent_record(&stray, "Stray", "runner", None),
    ];
    let installed = vec![
        installed_role("lead", &lead),
        installed_role("builder", &builder),
        installed_role("runner", &stray),
        installed_role("designer", &missing),
    ];
    let listed = projection::local_project_agents(&project, &installed, &agents);
    assert_eq!(
        listed
            .iter()
            .map(|agent| agent.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Builder", "Loom", "Vera"]
    );
    let source = ProjectTeamActivationSource {
        repo_ref: format!("30617:{}:packs", "a".repeat(64)),
        commit: "b".repeat(40),
        pack_path: "personas/roles".to_string(),
    };
    let turn = projection::lead_initial_turn(&source, &listed);
    assert!(turn.contains(
        "- builder: Builder (22222222)\n- lead: Loom (11111111)\n- verifier: Vera (33333333)"
    ));
    assert!(!turn.contains("Bob") && !turn.contains("Stray"));
    assert!(turn.contains(
        "`bee projects agents` lists agents published for public projects; this project's agents on the hosting computer are listed above."
    ));
    assert!(projection::lead_initial_turn(&source, &[]).contains("- none"));
}
