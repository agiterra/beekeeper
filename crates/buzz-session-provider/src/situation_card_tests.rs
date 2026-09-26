//! The situation card's render is pinned byte for byte; its reads are driven
//! through the plan judge over the conformance kettle plan.

use super::*;

const KETTLE_PLAN: &str =
    include_str!("../../../conformance/project-work/fixtures/plans/valid/kettle.md");

const PUBKEY: &str = "1958c6c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b9644";
const HASH: &str = "5e7b73dd20f6aa0000000000000000000000000000000000000000000000beef";

fn verify_action() -> CardAction {
    CardAction {
        name: "verify".to_owned(),
        hash: HASH.to_owned(),
        steps: vec!["verify".to_owned()],
    }
}

fn fixed_facts() -> SituationFacts {
    let plans = judge_plans(
        &[
            ("plans/kettle.md".to_owned(), KETTLE_PLAN.to_owned()),
            // The map is not a plan and must not be listed as one.
            (
                "plans/CURRENT_STATE.md".to_owned(),
                "# Current state\n".to_owned(),
            ),
        ],
        &Ok(vec![verify_action()]),
    );
    SituationFacts {
        channel: "56ef0396-f1d0-4cbd-9cc2-4f4fb592e823".to_owned(),
        session_ref: Ok("ce7d32cb-b703-4db8-b06e-2adbd2b942b3".to_owned()),
        genesis: Ok("fc6f36d9".repeat(8)),
        project: Ok(format!(
            "kettle-control-5 (30621:{PUBKEY}:kettle-control-5)"
        )),
        seat_role: "lead".to_owned(),
        seat_pubkey: Ok(PUBKEY.to_owned()),
        seat_model: Ok("opus[1m]".to_owned()),
        seat_runtime: "claude-agent-acp".to_owned(),
        code_checkout: Ok("/src/kettle-control-5".to_owned()),
        worktree: Ok("/src/kettle-control-5.worktrees/lead".to_owned()),
        worktree_head: Ok("main 7070283aa".to_owned()),
        agents_repo: Ok("/src/kettle-control-5.worktrees/lead-agents".to_owned()),
        agents_commit: Ok("21ece7f946797575e79902ae8b1002913be0152c".to_owned()),
        plans: Ok(plans),
        actions: Ok(vec![verify_action()]),
        roster: Ok(vec![
            CardRosterEntry {
                role: "lead".to_owned(),
                name: Ok("Lead".to_owned()),
                pubkey: PUBKEY.to_owned(),
                seated: true,
                hire_command: hire_command(
                    "56ef0396-f1d0-4cbd-9cc2-4f4fb592e823",
                    &Ok("ce7d32cb-b703-4db8-b06e-2adbd2b942b3".to_owned()),
                    "lead",
                ),
            },
            CardRosterEntry {
                role: "builder".to_owned(),
                name: Err("no-profile-record".to_owned()),
                pubkey: BUILDER_PUBKEY.to_owned(),
                seated: false,
                hire_command: hire_command(
                    "56ef0396-f1d0-4cbd-9cc2-4f4fb592e823",
                    &Ok("ce7d32cb-b703-4db8-b06e-2adbd2b942b3".to_owned()),
                    "builder",
                ),
            },
        ]),
        card_path: Ok("/data/agents/seats/s1/situation-card.md".to_owned()),
        hired: None,
    }
}

const GOLDEN: &str = "```situation-card
channel: 56ef0396-f1d0-4cbd-9cc2-4f4fb592e823
session-ref: ce7d32cb-b703-4db8-b06e-2adbd2b942b3
genesis: fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9
project: kettle-control-5 (30621:1958c6c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b9644:kettle-control-5)
seat: role lead pubkey 1958c6c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b9644 model opus[1m] runtime claude-agent-acp
code-checkout: /src/kettle-control-5
worktree: /src/kettle-control-5.worktrees/lead
worktree-head: main 7070283aa
agents-repo: /src/kettle-control-5.worktrees/lead-agents
agents-commit: 21ece7f946797575e79902ae8b1002913be0152c
plan: plans/kettle.md adoptable yes
  criterion cli-behaviour: review
  criterion shared-storage-and-parser: review
  criterion verified-landed-revision: action verify step verify
  criterion usage-documentation: review
  criterion delivered-main: git-ref refs/heads/main
action: verify hash 5e7b73dd20f6aa0000000000000000000000000000000000000000000000beef steps verify
roster: role lead pubkey 1958c6c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b9644 seated yes name Lead hire $BEE sessions hire --channel 56ef0396-f1d0-4cbd-9cc2-4f4fb592e823 --session-ref ce7d32cb-b703-4db8-b06e-2adbd2b942b3 --role lead
roster: role builder pubkey 005a9324c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b96 seated no name unknown (no-profile-record) hire $BEE sessions hire --channel 56ef0396-f1d0-4cbd-9cc2-4f4fb592e823 --session-ref ce7d32cb-b703-4db8-b06e-2adbd2b942b3 --role builder
card-copy: /data/agents/seats/s1/situation-card.md
```";

#[test]
fn the_card_renders_a_fixed_state_to_its_golden_bytes() {
    let card = render_situation_card(&fixed_facts());
    assert_eq!(card, GOLDEN);
    assert!(card.lines().count() <= 40, "{card}");
}

const BUILDER_PUBKEY: &str = "005a9324c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b96";
const ASSIGNMENT: &str = "53f776f4c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b96";
const HIRE: &str = "7d4f2a11c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b96";
const LEAD_TARGET: &str = "coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed36:f543d7bd-6059-44e3-a50b-4275d8440dd51:1";

/// A hired builder: the shared facts, then what it owes and how it reports.
fn builder_facts() -> SituationFacts {
    let channel = "56ef0396-f1d0-4cbd-9cc2-4f4fb592e823";
    let session_ref: Fact = Ok("ce7d32cb-b703-4db8-b06e-2adbd2b942b3".to_owned());
    let genesis: Fact = Ok("fc6f36d9".repeat(8));
    let lead_target: Fact = Ok(LEAD_TARGET.to_owned());
    SituationFacts {
        seat_role: "builder".to_owned(),
        seat_pubkey: Ok(BUILDER_PUBKEY.to_owned()),
        seat_model: Ok("sonnet".to_owned()),
        worktree: Ok("/src/kettle-control-5.worktrees/builder-005a9324".to_owned()),
        worktree_head: Ok("wip/builder/005a9324 7070283aa".to_owned()),
        agents_repo: Ok("/src/kettle-control-5.worktrees/builder-005a9324-agents".to_owned()),
        roster: Ok(Vec::new()),
        card_path: Ok("/data/agents/seats/s2/situation-card.md".to_owned()),
        hired: Some(HiredSeatFacts {
            hire: Ok(HIRE.to_owned()),
            assignment: Ok(ASSIGNMENT.to_owned()),
            criteria: Ok(vec![
                ("cli-behaviour".to_owned(), "review".to_owned()),
                (
                    "delivered-main".to_owned(),
                    "git-ref refs/heads/main".to_owned(),
                ),
            ]),
            delivery_ref: Ok("refs/heads/main".to_owned()),
            report_command: report_command(channel, &session_ref, &genesis),
            send_command: send_command(channel, &lead_target),
            lead_target,
            lead_pubkey: Ok(PUBKEY.to_owned()),
        }),
        plans: Ok(Vec::new()),
        ..fixed_facts()
    }
}

const BUILDER_GOLDEN: &str = "```situation-card
channel: 56ef0396-f1d0-4cbd-9cc2-4f4fb592e823
session-ref: ce7d32cb-b703-4db8-b06e-2adbd2b942b3
genesis: fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9
project: kettle-control-5 (30621:1958c6c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b9644:kettle-control-5)
seat: role builder pubkey 005a9324c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b96 model sonnet runtime claude-agent-acp
code-checkout: /src/kettle-control-5
worktree: /src/kettle-control-5.worktrees/builder-005a9324
worktree-head: wip/builder/005a9324 7070283aa
agents-repo: /src/kettle-control-5.worktrees/builder-005a9324-agents
agents-commit: 21ece7f946797575e79902ae8b1002913be0152c
hire: 7d4f2a11c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b96
assignment: 53f776f4c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b96
assigned-criterion cli-behaviour: review
assigned-criterion delivered-main: git-ref refs/heads/main
delivery-ref: refs/heads/main
lead-target: coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed36:f543d7bd-6059-44e3-a50b-4275d8440dd51:1
lead-pubkey: 1958c6c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b9644
report-command: $BEE sessions report --channel 56ef0396-f1d0-4cbd-9cc2-4f4fb592e823 --session-ref ce7d32cb-b703-4db8-b06e-2adbd2b942b3 --genesis fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9fc6f36d9 --body - --wake-to lead
send-command: $BEE sessions send --channel 56ef0396-f1d0-4cbd-9cc2-4f4fb592e823 --to coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed36:f543d7bd-6059-44e3-a50b-4275d8440dd51:1
plan: none
plan-example: $BEE plans example
action: verify hash 5e7b73dd20f6aa0000000000000000000000000000000000000000000000beef steps verify
roster: none
card-copy: /data/agents/seats/s2/situation-card.md
```";

#[test]
fn a_hired_seat_card_renders_to_its_golden_bytes() {
    let card = render_situation_card(&builder_facts());
    assert_eq!(card, BUILDER_GOLDEN);
    assert!(card.lines().count() <= 40, "{card}");
}

#[test]
fn a_hired_seat_with_no_local_lead_says_so_in_both_commands() {
    let mut facts = builder_facts();
    let hired = facts.hired.as_mut().expect("hired");
    hired.lead_target = Err("lead-not-local".to_owned());
    hired.send_command = send_command("c", &hired.lead_target);
    hired.assignment = Err("no-pointer".to_owned());
    hired.criteria = Err("no-pointer".to_owned());
    let card = render_situation_card(&facts);
    for line in [
        "lead-target: unknown (lead-not-local)",
        "send-command: unknown (lead-not-local)",
        "assignment: unknown (no-pointer)",
        "assigned-criteria: unknown (no-pointer)",
    ] {
        assert!(
            card.lines().any(|got| got == line),
            "missing {line:?} in\n{card}"
        );
    }
}

#[test]
fn an_unknown_field_renders_unknown_with_its_reason() {
    let facts = SituationFacts {
        genesis: Err("unrecorded".to_owned()),
        code_checkout: Err("unrecorded".to_owned()),
        agents_commit: Err("no-checkout".to_owned()),
        plans: Err("no-checkout".to_owned()),
        actions: Err("no-project".to_owned()),
        roster: Err("team-yml-invalid".to_owned()),
        card_path: Err("write-failed".to_owned()),
        ..fixed_facts()
    };
    let card = render_situation_card(&facts);
    for line in [
        "genesis: unknown (unrecorded)",
        "code-checkout: unknown (unrecorded)",
        "agents-commit: unknown (no-checkout)",
        "plan: unknown (no-checkout)",
        "action: unknown (no-project)",
        "roster: unknown (team-yml-invalid)",
        "card-copy: unknown (write-failed)",
    ] {
        assert!(
            card.lines().any(|got| got == line),
            "missing {line:?} in\n{card}"
        );
    }
}

#[test]
fn a_plan_whose_action_does_not_resolve_is_not_adoptable() {
    let plans = judge_plans(
        &[("plans/kettle.md".to_owned(), KETTLE_PLAN.to_owned())],
        &Ok(Vec::new()),
    );
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].adoptable, Err("unresolved-action".to_owned()));

    let wrong_step = CardAction {
        steps: vec!["build".to_owned()],
        ..verify_action()
    };
    let plans = judge_plans(
        &[("plans/kettle.md".to_owned(), KETTLE_PLAN.to_owned())],
        &Ok(vec![wrong_step]),
    );
    assert_eq!(plans[0].adoptable, Err("unresolved-action-step".to_owned()));
}

#[test]
fn a_founder_goal_carries_the_card_above_the_unframed_words() {
    let framing = attach_situation_card(
        None,
        Some("CARD".to_owned()),
        Uuid::nil(),
        PUBKEY,
        CodingSessionDelivery::Boundary,
    )
    .expect("a card makes a frame");
    assert!(framing.unaddressed);
    assert_eq!(
        framing.render("build kettle"),
        "CARD\n\n---\n\nbuild kettle"
    );

    // No card: the founder's turn stays unframed.
    assert!(attach_situation_card(
        None,
        None,
        Uuid::nil(),
        PUBKEY,
        CodingSessionDelivery::Boundary
    )
    .is_none());
}

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The reads run against a real commit: plans under `plans/` are judged, the
/// map beside them is not listed, and `team.yml` names the lead and roster.
#[tokio::test]
async fn an_agents_commit_is_read_into_plans_and_a_roster() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("plans")).expect("plans dir");
    std::fs::write(dir.join("plans/kettle.md"), KETTLE_PLAN).expect("plan");
    std::fs::write(dir.join("plans/CURRENT_STATE.md"), "# map\n").expect("map");
    std::fs::write(
        dir.join("team.yml"),
        "schema: beekeeper-team/v1\nname: k\nversion: 0.1.0\nlead: lead\nroles:\n  lead: {}\n  \
         builder: {}\nagents:\n  - { name: \"Lead\", role: lead, lifetime: persistent }\n  - { \
         name: \"Builder\", role: builder, lifetime: ephemeral }\n",
    )
    .expect("team.yml");
    sh(dir, &["init", "-q", "-b", "main"]);
    sh(dir, &["add", "."]);
    sh(dir, &["commit", "-q", "-m", "seed"]);
    let commit = sh(dir, &["rev-parse", "HEAD"]);

    let reads = read_agents_commit(dir, &commit, None).await;
    let plans = reads.plans.expect("plans listed");
    assert_eq!(plans.len(), 1, "{plans:?}");
    assert_eq!(plans[0].path, "plans/kettle.md");
    // No project coordinate: the action cannot be compiled, so it is unknown
    // and the plan's action proof does not resolve.
    assert_eq!(reads.actions, Err("no-project".to_owned()));
    assert_eq!(plans[0].adoptable, Err("unresolved-action".to_owned()));
    let team = reads.team.expect("team.yml parses");
    assert_eq!(team.lead.as_deref(), Some("lead"));
    assert_eq!(team.agents.len(), 2);
}

const PROJECT: &str =
    "30621:1958c6c448e05eed32599f6a25e2293ba84c9d4095c7c6958397bd95176b9644:kettle-control-9";

fn write_host_agents(dir: &Path, records: serde_json::Value) {
    let agents_dir = dir.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir");
    std::fs::write(agents_dir.join("managed-agents.json"), records.to_string()).expect("store");
}

fn host_agent(index: u8, project: &str) -> serde_json::Value {
    serde_json::json!({
        "pubkey": format!("{index:02x}{}", "a".repeat(62)),
        "name": format!("Agent {index}"),
        "home_role": format!("role-{index}"),
        "project_ref": project,
    })
}

/// Ledger 268(d): a project with 8 hireable agents must list all 8, each
/// with its full 64-hex pubkey and a `bee sessions hire` command already
/// filled in — no discovery detour left for the lead to run.
#[test]
fn a_project_with_eight_hireable_agents_lists_eight_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let records: Vec<serde_json::Value> = (0..8).map(|i| host_agent(i, PROJECT)).collect();
    write_host_agents(dir.path(), serde_json::Value::Array(records));

    let agents = hireable_host_agents(Some(dir.path()), Some(PROJECT)).expect("agents");
    assert_eq!(agents.len(), 8, "{agents:?}");
    for agent in &agents {
        assert_eq!(
            agent.pubkey.len(),
            64,
            "{:?} is not full-length",
            agent.pubkey
        );
        assert!(agent.home_role.is_some());
    }

    // An agent of a different project is not this project's to hire.
    let mut other_project = host_agent(
        9,
        "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:elsewhere",
    );
    other_project["home_role"] = serde_json::json!("role-9");
    let mut records: Vec<serde_json::Value> = (0..8).map(|i| host_agent(i, PROJECT)).collect();
    records.push(other_project);
    write_host_agents(dir.path(), serde_json::Value::Array(records));
    let agents = hireable_host_agents(Some(dir.path()), Some(PROJECT)).expect("agents");
    assert_eq!(agents.len(), 8, "{agents:?}");
}

/// An agent with no `home_role` cannot be hired by role and is not a row.
#[test]
fn an_agent_with_no_home_role_is_not_a_roster_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut roleless = host_agent(0, PROJECT);
    roleless
        .as_object_mut()
        .expect("object")
        .remove("home_role");
    write_host_agents(dir.path(), serde_json::Value::Array(vec![roleless]));
    let agents = hireable_host_agents(Some(dir.path()), Some(PROJECT)).expect("agents");
    assert!(agents.is_empty(), "{agents:?}");
}

/// A row for a seated identity says so; the hire command is filled in with
/// this seat's own channel, session-ref and role — never a placeholder.
#[test]
fn a_seated_agent_row_says_seated_and_the_hire_command_is_filled_in() {
    let channel = "56ef0396-f1d0-4cbd-9cc2-4f4fb592e823";
    let session_ref: Fact = Ok("ce7d32cb-b703-4db8-b06e-2adbd2b942b3".to_owned());
    let agent = HostAgentRecord {
        pubkey: PUBKEY.to_owned(),
        name: "Lead".to_owned(),
        home_role: Some("lead".to_owned()),
        project_ref: Some(PROJECT.to_owned()),
    };
    let entry = roster_entry(agent, channel, &session_ref, true);
    assert!(entry.seated);
    assert_eq!(entry.role, "lead");
    assert_eq!(entry.name, Ok("Lead".to_owned()));
    assert_eq!(entry.pubkey, PUBKEY);
    assert_eq!(
        entry.hire_command,
        Ok(format!(
            "$BEE sessions hire --channel {channel} --session-ref ce7d32cb-b703-4db8-b06e-2adbd2b942b3 --role lead"
        ))
    );
}

/// An agent with no name on this host's own record says so in words, not a
/// blank field.
#[test]
fn an_agent_with_no_name_says_no_profile_record() {
    let agent = HostAgentRecord {
        pubkey: PUBKEY.to_owned(),
        name: String::new(),
        home_role: Some("builder".to_owned()),
        project_ref: Some(PROJECT.to_owned()),
    };
    let entry = roster_entry(agent, "c", &Ok("s".to_owned()), false);
    assert_eq!(entry.name, Err("no-profile-record".to_owned()));
    assert!(!entry.seated);
}

/// Delivery proof (ledger 266 D / 268(d)): the first adapter prompt a lead
/// seat's frame renders — `TurnFraming::render`, `session.rs:499` — carries
/// the card's fence and its roster rows, above the sender's own words.
#[test]
fn the_first_adapter_prompt_for_a_lead_seat_carries_the_card_fence_and_roster_rows() {
    let mut facts = fixed_facts();
    facts.roster = Ok(vec![roster_entry(
        HostAgentRecord {
            pubkey: BUILDER_PUBKEY.to_owned(),
            name: "Builder".to_owned(),
            home_role: Some("builder".to_owned()),
            project_ref: Some(PROJECT.to_owned()),
        },
        &facts.channel,
        &facts.session_ref.clone(),
        false,
    )]);
    let card = render_situation_card(&facts);
    let framing = attach_situation_card(
        None,
        Some(card.clone()),
        Uuid::nil(),
        PUBKEY,
        CodingSessionDelivery::Boundary,
    )
    .expect("a card makes a frame");
    let prompt = framing.render("kick off the plan");

    assert!(
        prompt.starts_with(&format!("```{SITUATION_CARD_FENCE}")),
        "prompt does not open on the card fence:\n{prompt}"
    );
    assert!(prompt.contains(SITUATION_CARD_FENCE), "{prompt}");
    assert!(
        prompt.contains(&format!("pubkey {BUILDER_PUBKEY}")),
        "roster row missing from the prompt:\n{prompt}"
    );
    assert!(
        prompt.ends_with("kick off the plan"),
        "the sender's own words did not survive verbatim:\n{prompt}"
    );
}

/// Run10's regression: the host's shared-cache clone of the project's agents
/// repository is never the card's answer. Only the seat's own clone is.
#[test]
fn a_seat_without_its_own_agents_clone_gets_no_checkout_not_the_host_cache() {
    let dir = tempfile::tempdir().expect("tempdir");
    let worktree = dir.path().join("repos/project-wt-lead");
    std::fs::create_dir_all(&worktree).expect("worktree");
    // A host-cache clone of the same project, with a plan in it.
    let host_cache = dir.path().join("app/packs/project-beekeeper-agents");
    std::fs::create_dir_all(host_cache.join(".git")).expect("host cache");
    std::fs::create_dir_all(host_cache.join("plans")).expect("plans");
    std::fs::write(host_cache.join("plans/answer.md"), "CACHED_ANSWER").expect("plan");
    assert_eq!(seat_agents_dir(Some(&worktree)), None);
    assert_eq!(seat_agents_dir(None), None);
    let own = dir.path().join("repos/project-wt-lead-agents");
    std::fs::create_dir_all(own.join(".git")).expect("own clone");
    assert_eq!(seat_agents_dir(Some(&worktree)), Some(own));
}
