use super::*;

fn project(slug: &str) -> String {
    format!("30621:{}:{slug}", "ab".repeat(32))
}

fn agent(pubkey: &str, name: &str, home_role: Option<&str>) -> ManagedAgentRecord {
    let mut record: ManagedAgentRecord = serde_json::from_value(serde_json::json!({
        "pubkey": pubkey,
        "name": name,
        "relay_url": "wss://relay.example",
        "acp_command": "beekeeper-acp",
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
    record.home_role = home_role.map(str::to_owned);
    record
}

fn claim<'a>(project_ref: &'a str, role: &'a str, agent_pubkey: &'a str) -> InstalledRoleClaim<'a> {
    InstalledRoleClaim {
        project_ref,
        role,
        agent_pubkey,
    }
}

// ── Record shape ─────────────────────────────────────────────────────────

#[test]
fn project_ref_defaults_to_none_and_is_omitted_when_unset() {
    let record = agent("a", "Builder", Some("builder"));
    assert_eq!(
        record.project_ref, None,
        "older stores deserialize unassociated"
    );
    let json = serde_json::to_string(&record).expect("serialize");
    assert!(!json.contains("project_ref"));

    let mut associated = record;
    associated.project_ref = Some(project("tank-loop"));
    let json = serde_json::to_string(&associated).expect("serialize");
    let round: ManagedAgentRecord = serde_json::from_str(&json).expect("round trip");
    assert_eq!(round.project_ref, Some(project("tank-loop")));
}

// ── Native staging check ─────────────────────────────────────────────────

#[test]
fn seat_check_imposes_nothing_without_a_requirement() {
    let record = agent("a", "Bob", Some("builder"));
    assert_eq!(seat_project_refusal(&record, None), None);
    assert_eq!(seat_project_refusal(&record, Some("   ")), None);
}

#[test]
fn seat_check_admits_only_the_required_projects_agent() {
    let mut record = agent("a", "Builder", Some("builder"));
    record.project_ref = Some(project("tank-loop"));
    assert_eq!(
        seat_project_refusal(&record, Some(&project("tank-loop"))),
        None
    );
    // Owner case and surrounding space are normalized, not a mismatch.
    let upper = format!("  30621:{}:tank-loop ", "AB".repeat(32));
    assert_eq!(seat_project_refusal(&record, Some(&upper)), None);

    assert_eq!(
        seat_project_refusal(&record, Some(&project("other"))),
        Some(SEAT_NOT_PROJECT_AGENT),
        "another project's agent is never borrowed"
    );
    assert_eq!(
        seat_project_refusal(&record, Some("30621:not-a-coordinate")),
        Some(SEAT_NOT_PROJECT_AGENT),
        "a malformed requirement matches no agent"
    );

    let unassociated = agent("b", "Bob", Some("builder"));
    assert_eq!(
        seat_project_refusal(&unassociated, Some(&project("tank-loop"))),
        Some(SEAT_NOT_PROJECT_AGENT),
        "a matching role name is not project membership"
    );
}

#[test]
fn refused_preview_stages_nothing_and_carries_the_sentence() {
    let preview = refused_seat_preview(Some(" builder "), SEAT_NOT_PROJECT_AGENT);
    assert!(!preview.pack_staged);
    assert_eq!(preview.role.as_deref(), Some("builder"));
    assert_eq!(preview.refusal.as_deref(), Some(SEAT_NOT_PROJECT_AGENT));
    assert!(preview.pack_dir.is_none() && preview.pack_ref.is_none());
    let json = serde_json::to_value(&preview).expect("serialize");
    assert_eq!(json["refusal"], SEAT_NOT_PROJECT_AGENT);
    assert_eq!(json["packStaged"], false);
}

// ── Explicit association ─────────────────────────────────────────────────

#[test]
fn association_refuses_a_malformed_coordinate_first() {
    let mut builtin = agent("a", "Solo", None);
    builtin.is_builtin = true;
    assert_eq!(
        decide_association(&builtin, "30177:ab:tank-loop"),
        Err(ASSOCIATION_MALFORMED_PROJECT.to_string())
    );
}

#[test]
fn association_refuses_builtin_setup_actor_and_roleless_agents() {
    let mut builtin = agent("a", "Solo", Some("builder"));
    builtin.is_builtin = true;
    let error = decide_association(&builtin, &project("tank-loop")).unwrap_err();
    assert!(error.contains("built-in"), "{error}");

    let mut setup = agent("b", "Project Setup", Some("setup"));
    setup.persona_id = Some("project-team-setup:abc".to_string());
    let error = decide_association(&setup, &project("tank-loop")).unwrap_err();
    assert!(error.contains("setup agent"), "{error}");

    for role in [None, Some("  ")] {
        let roleless = agent("c", "Helper", role);
        assert_eq!(
            decide_association(&roleless, &project("tank-loop")),
            Err(ASSOCIATION_NEEDS_PRIMARY_ROLE.to_string())
        );
    }
}

#[test]
fn association_never_moves_an_agent_between_projects() {
    let mut record = agent("a", "Builder", Some("builder"));
    record.project_ref = Some(project("tank-loop"));
    assert_eq!(
        decide_association(&record, &project("other")),
        Err("Builder belongs to another project; borrowing is not supported.".to_string())
    );
    record.project_ref = Some("garbage".to_string());
    assert!(
        decide_association(&record, &project("other")).is_err(),
        "an unreadable recorded association is never silently replaced"
    );
}

#[test]
fn association_records_the_normalized_coordinate_and_repeats_as_a_no_op() {
    let mut record = agent("a", "Builder", Some("builder"));
    let upper = format!("30621:{}:tank-loop", "AB".repeat(32));
    assert_eq!(
        decide_association(&record, &upper),
        Ok(AssociationDecision::Associate(project("tank-loop")))
    );
    record.project_ref = Some(project("tank-loop"));
    assert_eq!(
        decide_association(&record, &upper),
        Ok(AssociationDecision::AlreadyAssociated)
    );
}

// ── Installation and journal backfill ────────────────────────────────────

#[test]
fn backfill_associates_matching_roles_and_is_idempotent() {
    let tank = project("tank-loop");
    let mut agents = vec![
        agent("lead", "Loom", Some("lead")),
        agent("builder", "Builder", Some("builder")),
        agent("bob", "Bob", Some("builder")),
    ];
    let claims = [
        claim(&tank, "lead", "lead"),
        claim(&tank, "builder", "builder"),
    ];

    let first = apply_installed_role_claims(&mut agents, claims.iter().cloned());
    assert_eq!(
        first.associated,
        vec!["lead".to_string(), "builder".to_string()]
    );
    assert!(first.conflicts.is_empty());
    let after_first = agents.clone();

    let second = apply_installed_role_claims(&mut agents, claims.iter().cloned());
    assert_eq!(second, AssociationOutcome::default());
    assert_eq!(agents, after_first, "a second backfill changes nothing");
    assert_eq!(agents[0].project_ref.as_deref(), Some(tank.as_str()));
    assert_eq!(agents[1].project_ref.as_deref(), Some(tank.as_str()));
    assert_eq!(agents[2].project_ref, None, "Bob stays unassociated");
}

#[test]
fn backfill_leaves_another_projects_agent_untouched() {
    let other = project("beekeeper");
    let tank = project("tank-loop");
    let mut builder = agent("builder", "Builder", Some("builder"));
    builder.project_ref = Some(other.clone());
    let mut agents = vec![builder.clone()];

    let outcome = apply_installed_role_claims(&mut agents, [claim(&tank, "builder", "builder")]);
    assert!(outcome.associated.is_empty());
    assert_eq!(
        outcome.conflicts,
        vec![AssociationConflict {
            agent_pubkey: "builder".to_string(),
            agent_name: "Builder".to_string(),
            role: "builder".to_string(),
            existing: other,
            wanted: tank,
        }]
    );
    assert_eq!(
        agents,
        vec![builder],
        "the conflicting record is not modified"
    );
}

#[test]
fn backfill_ignores_missing_records_role_mismatches_and_malformed_claims() {
    let tank = project("tank-loop");
    let mut setup = agent("setup", "Project Setup", Some("builder"));
    setup.persona_id = Some("project-team-setup:x".to_string());
    let mut agents = vec![
        agent("verifier", "Verifier", Some("verifier")),
        agent("roleless", "Roleless", None),
        agent("builder", "Builder", Some("builder")),
        setup,
    ];
    let before = agents.clone();
    let outcome = apply_installed_role_claims(
        &mut agents,
        [
            claim(&tank, "lead", "missing"),
            claim(&tank, "builder", "verifier"),
            claim(&tank, "builder", "roleless"),
            claim("30621:not-hex:tank-loop", "builder", "builder"),
            claim(&tank, "builder", "setup"),
        ],
    );
    assert_eq!(outcome, AssociationOutcome::default());
    assert_eq!(agents.len(), 4, "no record is created or deleted");
    assert_eq!(agents, before);
}

fn installed(role: &str, agent_pubkey: &str) -> super::super::crew_roles::InstalledCrewRole {
    super::super::crew_roles::InstalledCrewRole {
        persona_id: role.to_string(),
        persona_name: role.to_string(),
        role: role.to_string(),
        agent_pubkey: agent_pubkey.to_string(),
        agent_name: role.to_string(),
        pack_dir: "/packs".to_string(),
        refreshed: false,
        renamed: false,
        seated: true,
    }
}

#[test]
fn installation_associates_its_agents_and_refuses_to_claim_another_projects() {
    let tank = project("tank-loop");
    let mut agents = vec![
        agent("lead", "Loom", Some("lead")),
        agent("builder", "Builder", Some("builder")),
    ];
    let roles = [installed("lead", "lead"), installed("builder", "builder")];
    let outcome = associate_installation(&mut agents, &tank, &roles).expect("associate");
    assert_eq!(outcome.associated.len(), 2);
    assert!(agents
        .iter()
        .all(|a| a.project_ref.as_deref() == Some(tank.as_str())));
    // A retried installation is a no-op.
    let again = associate_installation(&mut agents, &tank, &roles).expect("retry");
    assert!(again.associated.is_empty());

    let error = associate_installation(&mut agents, &project("other"), &roles).unwrap_err();
    assert!(error.contains("belongs to another project"), "{error}");
    assert!(agents
        .iter()
        .all(|a| a.project_ref.as_deref() == Some(tank.as_str())));
}

// ── New-selection seat rule ──────────────────────────────────────────────

#[test]
fn without_new_selection_the_seat_rule_is_the_project_check_alone() {
    let mut record = agent("a", "Builder", Some("builder"));
    record.project_ref = Some(project("tank-loop"));
    for flag in [None, Some(false)] {
        // No projectless refusal and no role refusal: resume and restage.
        assert_eq!(
            new_seat_refusal(&record, Some("verifier"), None, flag),
            None
        );
        assert_eq!(
            new_seat_refusal(&record, Some("verifier"), Some(&project("other")), flag),
            Some(SEAT_NOT_PROJECT_AGENT.to_string()),
            "an explicit requirement still applies"
        );
        assert_eq!(
            new_seat_refusal(&record, Some("verifier"), Some(&project("tank-loop")), flag),
            None
        );
    }
}

#[test]
fn a_new_selection_in_a_project_takes_only_that_projects_agent() {
    let mut record = agent("a", "Builder", Some("builder"));
    record.project_ref = Some(project("tank-loop"));
    let yes = Some(true);
    assert_eq!(
        new_seat_refusal(&record, Some("builder"), Some(&project("tank-loop")), yes),
        None
    );
    assert_eq!(
        new_seat_refusal(&record, Some("builder"), Some(&project("other")), yes),
        Some(SEAT_NOT_PROJECT_AGENT.to_string())
    );
    let bob = agent("b", "Bob", Some("builder"));
    assert_eq!(
        new_seat_refusal(&bob, Some("builder"), Some(&project("tank-loop")), yes),
        Some(SEAT_NOT_PROJECT_AGENT.to_string()),
        "a matching role is not membership"
    );
}

#[test]
fn a_projectless_new_selection_never_takes_a_projects_agent() {
    let mut record = agent("a", "Builder", Some("builder"));
    record.project_ref = Some(project("tank-loop"));
    for required in [None, Some(""), Some(" \t ")] {
        assert_eq!(
            new_seat_refusal(&record, Some("builder"), required, Some(true)),
            Some(SEAT_PROJECT_AGENT_OUTSIDE_PROJECT.to_string()),
            "{required:?}"
        );
    }
    assert_eq!(
        SEAT_PROJECT_AGENT_OUTSIDE_PROJECT,
        "This agent belongs to a project, so it cannot take a new seat in a session outside that project. Borrowing agents from other projects is not supported."
    );
    // An unreadable recorded association is still one.
    record.project_ref = Some("garbage".to_string());
    assert_eq!(
        new_seat_refusal(&record, Some("builder"), None, Some(true)),
        Some(SEAT_PROJECT_AGENT_OUTSIDE_PROJECT.to_string())
    );
    let bob = agent("b", "Bob", Some("builder"));
    assert_eq!(
        new_seat_refusal(&bob, Some("builder"), None, Some(true)),
        None
    );
}

#[test]
fn a_new_selection_sits_only_in_the_agents_primary_role() {
    let mut record = agent("a", "Builder", Some("builder"));
    record.project_ref = Some(project("tank-loop"));
    let tank = project("tank-loop");
    assert_eq!(
        new_seat_refusal(&record, Some(" verifier "), Some(&tank), Some(true)),
        Some(
            "A new seat takes the agent's primary role. Builder is a builder, so it cannot be seated as verifier; hire or pick a verifier agent instead."
                .to_string()
        )
    );
    assert_eq!(
        new_seat_refusal(&record, Some(" builder "), Some(&tank), Some(true)),
        None,
        "a trimmed match is the primary role"
    );
    assert_eq!(
        new_seat_refusal(&record, None, Some(&tank), Some(true)),
        None,
        "a stage that names no role is not a role change"
    );
    // The association refusal comes first.
    assert_eq!(
        new_seat_refusal(
            &record,
            Some("verifier"),
            Some(&project("other")),
            Some(true)
        ),
        Some(SEAT_NOT_PROJECT_AGENT.to_string())
    );
    let roleless = agent("c", "Helper", None);
    assert_eq!(
        new_seat_refusal(&roleless, Some("verifier"), None, Some(true)),
        None,
        "an agent without a primary role is unaffected"
    );
    assert_eq!(SEAT_ROLE_NOT_PRIMARY, "SEAT_ROLE_NOT_PRIMARY");
}

#[test]
fn project_refs_trim_ascii_whitespace_only() {
    let tank = project("tank-loop");
    assert_eq!(
        normalize_project_ref(&format!("\t{tank}\r\n")),
        Some(tank.clone())
    );
    assert_eq!(
        normalize_project_ref(&format!("{tank}\u{feff}")),
        Some(format!("{tank}\u{feff}")),
        "U+FEFF is not trimmed"
    );
    assert_eq!(normalize_project_ref(&format!("{tank}\u{85}")), None);
}

// ── Reinstall keeps the association ──────────────────────────────────────

fn write_role_pack(root: &std::path::Path, role: &str) {
    let dir = root.join(role);
    std::fs::create_dir_all(dir.join(".plugin")).expect("pack .plugin dir");
    std::fs::create_dir_all(dir.join("personas")).expect("pack personas dir");
    std::fs::write(
        dir.join(".plugin").join("plugin.json"),
        format!(
            r#"{{"id":"com.test.{role}","name":"{role}","version":"0.1.0","personas":["personas/{role}.persona.md"]}}"#
        ),
    )
    .expect("plugin.json");
    std::fs::write(
        dir.join("personas").join(format!("{role}.persona.md")),
        format!("---\nname: {role}\nrole: {role}\ndisplay_name: \"{role}\"\ndescription: \"the {role}\"\n---\n\nYou are {role}.\n"),
    )
    .expect("persona file");
}

fn install_team(
    scan: &super::super::crew_roles::RolePackScan,
    previous: Option<&super::super::crew_roles::CrewRoleInstall>,
    agents: Vec<ManagedAgentRecord>,
) -> super::super::crew_roles::CrewRoleInstall {
    let mut minted = 0usize;
    let mut mint = || {
        minted += 1;
        Ok(super::super::crew_roles::MintedCrewIdentity {
            pubkey: format!("{minted:0>64}"),
            private_key_nsec: format!("nsec1test{minted}"),
            auth_tag: None,
        })
    };
    super::super::crew_roles::install_role_packs_in_named_team(
        scan,
        previous.map(|p| p.definitions.clone()).unwrap_or_default(),
        agents,
        &previous.map(|p| vec![p.team.clone()]).unwrap_or_default(),
        "2026-09-14T00:00:00Z",
        &Default::default(),
        &mut mint,
        "team-tank".to_string(),
        "Project team",
        super::super::crew_roles::NameScope::Project(project("tank-loop")),
    )
    .expect("install succeeds")
}

/// A project whose roles moved out of a packs repository and into its own
/// agents repository installs from a different directory, so the exact
/// reuse key matches nothing. Without adoption by role, every identity the
/// operator already knows would be shadowed by a second one.
#[test]
fn a_role_installed_from_a_new_directory_adopts_the_identity_the_team_already_has() {
    let old_root = tempfile::tempdir().expect("temp dir");
    write_role_pack(old_root.path(), "lead");
    write_role_pack(old_root.path(), "builder");
    let old_scan = super::super::crew_roles::scan_role_packs(old_root.path()).expect("scan");
    let first = install_team(&old_scan, None, Vec::new());
    assert_eq!(first.agents.len(), 2);
    let before: Vec<String> = {
        let mut keys: Vec<String> = first.agents.iter().map(|a| a.pubkey.clone()).collect();
        keys.sort();
        keys
    };

    // The same two roles, now converted into an agents repository, which
    // this host stages from a different cache directory.
    let new_root = tempfile::tempdir().expect("temp dir");
    write_role_pack(new_root.path(), "lead");
    write_role_pack(new_root.path(), "builder");
    let new_scan = super::super::crew_roles::scan_role_packs(new_root.path()).expect("scan");

    let second = install_team(&new_scan, Some(&first), first.agents.clone());

    assert_eq!(second.agents.len(), 2, "no second set of identities");
    assert_eq!(
        second.installed.iter().filter(|r| r.refreshed).count(),
        2,
        "both roles were refreshed, not minted"
    );
    let after: Vec<String> = {
        let mut keys: Vec<String> = second.agents.iter().map(|a| a.pubkey.clone()).collect();
        keys.sort();
        keys
    };
    assert_eq!(after, before, "the identities kept their keys");
}

/// Two identities filling one role is an arrangement someone made on
/// purpose; adopting one of them would be a guess, so the install mints.
#[test]
fn a_role_two_identities_already_fill_is_not_adopted_by_guessing() {
    let root = tempfile::tempdir().expect("temp dir");
    write_role_pack(root.path(), "builder");
    let scan = super::super::crew_roles::scan_role_packs(root.path()).expect("scan");
    let first = install_team(&scan, None, Vec::new());

    // A second builder on the same team, minted from somewhere else.
    let mut agents = first.agents.clone();
    let mut twin = agents[0].clone();
    twin.pubkey = "f".repeat(64);
    twin.name = format!("{} 2", twin.name);
    twin.persona_team_dir = Some(std::path::PathBuf::from("/somewhere/else"));
    agents.push(twin);

    let other_root = tempfile::tempdir().expect("temp dir");
    write_role_pack(other_root.path(), "builder");
    let other = super::super::crew_roles::scan_role_packs(other_root.path()).expect("scan");
    // A mint that cannot collide with either existing key: `install_team`'s
    // counter restarts at 1 every call, and a minted pubkey equal to an
    // existing one is matched by pubkey and *replaces* that record, which
    // would read as "nothing was minted" whatever the reuse rule did.
    let mut minted = 0usize;
    let mut mint = || {
        minted += 1;
        Ok(super::super::crew_roles::MintedCrewIdentity {
            pubkey: format!("a{minted:0>63}"),
            private_key_nsec: format!("nsec1fresh{minted}"),
            auth_tag: None,
        })
    };
    let second = super::super::crew_roles::install_role_packs_in_named_team(
        &other,
        first.definitions.clone(),
        agents,
        std::slice::from_ref(&first.team),
        "2026-09-14T00:00:00Z",
        &Default::default(),
        &mut mint,
        "team-tank".to_string(),
        "Project team",
        super::super::crew_roles::NameScope::Project(project("tank-loop")),
    )
    .expect("install succeeds");

    assert_eq!(
        second.agents.len(),
        3,
        "the ambiguous role minted rather than adopting one of the two"
    );
}

#[test]
fn reinstalling_team_roles_keeps_an_associated_agents_project() {
    let root = tempfile::tempdir().expect("temp dir");
    write_role_pack(root.path(), "lead");
    write_role_pack(root.path(), "builder");
    let scan = super::super::crew_roles::scan_role_packs(root.path()).expect("scan");
    let tank = project("tank-loop");

    let first = install_team(&scan, None, Vec::new());
    let role_of = |agents: &[ManagedAgentRecord], role: &str| {
        agents
            .iter()
            .find(|a| a.home_role.as_deref() == Some(role))
            .cloned()
            .expect("installed role agent")
    };
    let mut agents = first.agents.clone();
    for record in agents.iter_mut() {
        if record.home_role.as_deref() == Some("builder") {
            record.project_ref = Some(tank.clone());
        }
    }
    let second = install_team(&scan, Some(&first), agents);

    assert_eq!(second.installed.iter().filter(|r| r.refreshed).count(), 2);
    assert_eq!(second.agents.len(), 2, "no duplicate agents");
    assert_eq!(
        role_of(&second.agents, "builder").project_ref.as_deref(),
        Some(tank.as_str()),
        "Install team roles never drops an explicit association"
    );
    assert_eq!(
        role_of(&second.agents, "lead").project_ref,
        None,
        "no association appears"
    );
}

#[test]
fn an_installation_retry_refuses_an_agent_of_another_project_rather_than_moving_it() {
    let root = tempfile::tempdir().expect("temp dir");
    write_role_pack(root.path(), "lead");
    write_role_pack(root.path(), "builder");
    let scan = super::super::crew_roles::scan_role_packs(root.path()).expect("scan");
    let tank = project("tank-loop");
    let other = project("other");

    let mut first = install_team(&scan, None, Vec::new());
    associate_installation(&mut first.agents, &tank, &first.installed).expect("associate");

    // Same project: the retry sees the carried association and is a no-op.
    let mut same = install_team(&scan, Some(&first), first.agents.clone());
    let outcome = associate_installation(&mut same.agents, &tank, &same.installed)
        .expect("retry for the same project");
    assert!(outcome.associated.is_empty());

    // Another project: the rebuilt records still carry Tank Loop, so the
    // installation is refused and nothing is moved.
    let mut moved = install_team(&scan, Some(&first), first.agents.clone());
    let error = associate_installation(&mut moved.agents, &other, &moved.installed).unwrap_err();
    assert!(error.contains("belongs to another project"), "{error}");
    assert!(moved
        .agents
        .iter()
        .all(|a| a.project_ref.as_deref() == Some(tank.as_str())));
}
