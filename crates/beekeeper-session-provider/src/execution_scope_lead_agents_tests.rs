//! A project lead's conversation and its sibling agents clone, through the
//! production grant and the real boundary. Each allowance is paired with what
//! must still be refused, and with the conversation it must not cost.

use super::launch::lead_agents_clone;
use super::tests::launched::run_in;
use super::tests::{fixture, inputs, Fixture};
use super::*;

const PROJECT: &str = "30621:aa:project-a";
const URL: &str = "https://relay.fixture/git/aa/project-a-beekeeper-agents";

fn prepared(plan: Result<ExecutionPlan, CreateFailure>) -> Box<PreparedExecution> {
    match plan {
        Ok(ExecutionPlan::Prepared(plan)) => plan,
        other => panic!("prepared: {other:?}"),
    }
}

struct Lead {
    fx: Fixture,
    provider: crate::Provider,
    actor: String,
    clone: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    super::tests::git(dir, args);
}

fn clone_beside(tree: &Path, url: &str, access: &str) -> PathBuf {
    let clone = beekeeper_core::model_registry_source::seat_agents_clone_path(tree).expect("name");
    std::fs::create_dir_all(clone.join("plans")).expect("clone");
    git(&clone, &["init", "-q"]);
    git(&clone, &["remote", "add", "origin", url]);
    std::fs::write(
        clone.join("team.yml"),
        format!(
            "schema: beekeeper-team/v1\nname: fixture\nversion: 0.1.0\nlead: lead\nroles:\n  \
             lead: {{ workspace: {{ agents_repo: {access} }} }}\n"
        ),
    )
    .expect("team");
    std::fs::write(clone.join("plans/CURRENT_STATE.md"), "the map\n").expect("map");
    clone
}

fn context(provider: &crate::Provider, actor: &str, project: &str) {
    let keys = nostr::Keys::generate();
    let genesis = nostr::EventBuilder::new(nostr::Kind::Custom(1), "fixture")
        .sign_with_keys(&keys)
        .expect("signed");
    let dir = provider.config.state_dir.join("conversation-projects");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join(format!("{actor}.json")),
        serde_json::to_vec(&serde_json::json!({
            "project": project, "channel": uuid::Uuid::new_v4(), "actor": actor, "genesis": genesis,
        }))
        .expect("json"),
    )
    .expect("context");
}

fn lead(access: &str) -> Lead {
    let fx = fixture();
    let provider = crate::tests::provider(&fx.state_dir, None);
    let actor = nostr::Keys::generate().public_key().to_hex();
    context(&provider, &actor, PROJECT);
    let clone = clone_beside(&fx.repo_a, URL, access);
    Lead {
        fx,
        provider,
        actor,
        clone,
    }
}

fn projects(fx: &Fixture, url: Option<&str>) -> crate::commands::ProjectsFile {
    let mut file = crate::commands::ProjectsFile::default();
    file.projects.insert(PROJECT.to_owned(), fx.repo_a.clone());
    if let Some(url) = url {
        file.agents_repos.insert(
            PROJECT.to_owned(),
            crate::agents_checkout::AgentsRepoRecord {
                path: fx.root.join("packs"),
                ref_name: "refs/heads/main".to_owned(),
                url: Some(url.to_owned()),
            },
        );
    }
    file
}

impl Lead {
    fn grant(&self, projects: &crate::commands::ProjectsFile) -> Option<(PathBuf, bool)> {
        lead_agents_clone(
            &self.provider.config,
            projects,
            Some(&self.actor),
            Some(PROJECT),
            &self.fx.repo_a,
        )
    }
    fn plan(&self, session: &str, agents: Option<(&Path, bool)>) -> Box<PreparedExecution> {
        let mut scope = inputs(&self.fx, session, &self.fx.repo_a, &[], &[]);
        scope.actor = Some(&self.actor);
        scope.agents_checkout = agents;
        scope.agents_outside_digest = agents.is_some();
        prepared(prepare(&scope))
    }
}

#[test]
fn an_installed_lead_gets_its_own_clone_at_the_access_team_yml_gives() {
    let write = lead("write");
    let granted = write
        .grant(&projects(&write.fx, Some(URL)))
        .expect("granted");
    assert_eq!(
        granted,
        (write.clone.canonicalize().expect("clone"), true),
        "write, from team.yml"
    );
    let read = lead("read");
    assert_eq!(
        read.grant(&projects(&read.fx, Some(URL))).map(|g| g.1),
        Some(false)
    );
    let none = lead("none");
    assert_eq!(
        none.grant(&projects(&none.fx, Some(URL))),
        None,
        "none is no grant"
    );
}

#[test]
fn nothing_else_earns_the_grant() {
    let held = lead("write");
    let good = projects(&held.fx, Some(URL));
    let trust_of_nothing = projects(&held.fx, None);
    assert_eq!(
        held.grant(&trust_of_nothing),
        None,
        "no trusted agents repository"
    );
    assert_eq!(
        held.grant(&projects(
            &held.fx,
            Some("https://relay.fixture/git/aa/other-agents")
        )),
        None,
        "a clone of another project's agents repository"
    );
    let config = &held.provider.config;
    let stranger = nostr::Keys::generate().public_key().to_hex();
    assert_eq!(
        lead_agents_clone(
            config,
            &good,
            Some(&stranger),
            Some(PROJECT),
            &held.fx.repo_a
        ),
        None,
        "an actor with no project context"
    );
    assert_eq!(
        lead_agents_clone(
            config,
            &good,
            Some(&held.actor),
            Some("30621:aa:project-b"),
            &held.fx.repo_a
        ),
        None,
        "another project"
    );
    assert_eq!(
        lead_agents_clone(
            config,
            &good,
            Some(&held.actor),
            Some(PROJECT),
            &held.fx.seat_a
        ),
        None,
        "a tree that is not the trusted checkout"
    );
    assert_eq!(
        lead_agents_clone(config, &good, None, Some(PROJECT), &held.fx.repo_a),
        None
    );
    std::fs::remove_dir_all(held.clone.join(".git")).expect("no clone");
    assert_eq!(held.grant(&good), None, "not a clone");
}

#[test]
fn the_grant_does_not_change_the_scope_so_the_conversation_continues() {
    let held = lead("write");
    let before = held.plan("lead-session", None);
    let clone = held.clone.canonicalize().expect("clone");
    let after = held.plan("lead-session", Some((&clone, true)));
    assert_eq!(before.binding.scope_digest, after.binding.scope_digest);
    assert_eq!(after.binding.agents.as_deref(), Some(clone.as_path()));

    let mut scope = inputs(&held.fx, "lead-session", &held.fx.repo_a, &[], &[]);
    scope.actor = Some(&held.actor);
    scope.agents_checkout = Some((&clone, true));
    scope.agents_outside_digest = true;
    scope.prior = Some((Some(&before.binding), Some("native-cursor")));
    let continued = prepared(prepare(&scope));
    assert!(
        continued.native_refusal.is_none(),
        "the history written before the grant still attaches: {:?}",
        continued.native_refusal
    );
}

#[test]
fn the_boundary_opens_the_clone_and_no_other_projects_plans() {
    let held = lead("read");
    let clone = held.clone.canonicalize().expect("clone");
    let plan = held.plan("lead-read", Some((&clone, false)));
    let map = clone.join("plans/CURRENT_STATE.md");
    let foreign = held.fx.b_plan.display().to_string();

    let read = run_in(&plan, &held.fx.repo_a, &format!("cat '{}'", map.display()));
    assert!(
        read.status.success(),
        "the lead reads its own plans: {read:?}"
    );
    assert_eq!(String::from_utf8_lossy(&read.stdout), "the map\n");

    let other = run_in(&plan, &held.fx.repo_a, &format!("cat '{foreign}'"));
    assert!(
        !other.status.success(),
        "another project's plans stay refused: {other:?}"
    );
    assert!(String::from_utf8_lossy(&other.stderr).contains("Operation not permitted"));

    let written = run_in(
        &plan,
        &held.fx.repo_a,
        &format!("echo x >> '{}'", map.display()),
    );
    assert!(
        !written.status.success(),
        "a read grant writes nothing: {written:?}"
    );

    let held = lead("write");
    let clone = held.clone.canonicalize().expect("clone");
    let plan = held.plan("lead-write", Some((&clone, true)));
    let map = clone.join("plans/CURRENT_STATE.md");
    let written = run_in(
        &plan,
        &held.fx.repo_a,
        &format!("echo edit >> '{}'", map.display()),
    );
    assert!(
        written.status.success(),
        "a write grant commits plans: {written:?}"
    );
    let other = run_in(
        &plan,
        &held.fx.repo_a,
        &format!("cat '{}'", held.fx.b_plan.display()),
    );
    assert!(
        !other.status.success(),
        "write here opens nothing elsewhere: {other:?}"
    );
}

#[test]
fn only_a_writable_leads_briefing_gains_the_paragraph() {
    let held = lead("write");
    let clone = held.clone.canonicalize().expect("clone");
    let plan = held.plan("lead-brief", Some((&clone, true)));
    let briefing = crate::session::boundary_briefing(&ExecutionPlan::Prepared(plan));
    assert!(
        briefing.contains(&format!("checked out at {}", clone.display())),
        "{briefing}"
    );
    assert!(
        briefing.contains("You may commit there and push to main"),
        "{briefing}"
    );
}
