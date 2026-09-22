//! What the per-project name migration renames, and — more importantly — what
//! it refuses to.

use super::*;

fn project(slug: &str) -> String {
    format!("30621:{}:{slug}", "ab".repeat(32))
}

fn base_record(pubkey: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
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
    })
}

/// An instance the installer minted: role, crew-role persona link, team.
fn instance(
    pubkey: &str,
    name: &str,
    role: &str,
    slug: &str,
    team: &str,
    project_ref: Option<&str>,
) -> serde_json::Value {
    let mut value = base_record(pubkey, name);
    let map = value.as_object_mut().expect("object");
    map.insert("home_role".into(), serde_json::json!(role));
    map.insert("persona_id".into(), serde_json::json!(slug));
    map.insert("team_id".into(), serde_json::json!(team));
    if let Some(project_ref) = project_ref {
        map.insert("project_ref".into(), serde_json::json!(project_ref));
    }
    value
}

/// The definition the installer wrote beside it: key-less, same name, same
/// `display_name`.
fn definition(slug: &str, name: &str) -> serde_json::Value {
    let mut value = base_record("", name);
    let map = value.as_object_mut().expect("object");
    map.insert("slug".into(), serde_json::json!(slug));
    map.insert("display_name".into(), serde_json::json!(name));
    value
}

struct Store {
    dir: tempfile::TempDir,
}

impl Store {
    fn new(records: Vec<serde_json::Value>, teams: Vec<(&str, &str)>) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("managed-agents.json"),
            serde_json::to_vec_pretty(&records).expect("records"),
        )
        .expect("write records");
        let teams: Vec<serde_json::Value> = teams
            .into_iter()
            .map(|(id, name)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "description": null,
                    "persona_ids": [],
                    "is_builtin": false,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                })
            })
            .collect();
        std::fs::write(
            dir.path().join("teams.json"),
            serde_json::to_vec_pretty(&teams).expect("teams"),
        )
        .expect("write teams");
        Self { dir }
    }

    fn run(&self) -> NameScopeOutcome {
        scope_agent_names_to_projects_in_dir(self.dir.path()).expect("migration succeeds")
    }

    fn records(&self) -> Vec<ManagedAgentRecord> {
        serde_json::from_str(
            &std::fs::read_to_string(self.dir.path().join("managed-agents.json"))
                .expect("read back"),
        )
        .expect("parse back")
    }

    fn named(&self, pubkey: &str) -> String {
        self.records()
            .into_iter()
            .find(|record| record.pubkey == pubkey)
            .expect("the record")
            .name
    }
}

const ALPHA_TEAM: &str = "team-alpha";
const LEGACY_TEAM: &str = "team-roles";

fn project_teams() -> Vec<(&'static str, &'static str)> {
    vec![
        (ALPHA_TEAM, "Project team 30621:abab:alpha"),
        (LEGACY_TEAM, "Team roles"),
    ]
}

// ── What it renames ──────────────────────────────────────────────────────

/// The headline. A project's `Lead 4` is only called that because another
/// project got there first.
#[test]
fn a_project_teams_minted_suffix_is_stripped() {
    let store = Store::new(
        vec![
            definition("crew-role:lead", "Lead"),
            definition("crew-role:lead-4", "Lead 4"),
            instance(
                "a".repeat(64).as_str(),
                "Lead",
                "lead",
                "crew-role:lead",
                LEGACY_TEAM,
                None,
            ),
            instance(
                "b".repeat(64).as_str(),
                "Lead 4",
                "lead",
                "crew-role:lead-4",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
        ],
        project_teams(),
    );

    let outcome = store.run();
    assert_eq!(outcome.renamed.len(), 1, "only the project one moves");
    assert_eq!(store.named(&"b".repeat(64)), "Lead");
    assert_eq!(
        store.named(&"a".repeat(64)),
        "Lead",
        "the no-project Lead keeps the name it always had"
    );
}

/// The definition follows the instance, and its slug does not move — the slug
/// is a published kind:30175 `d` tag and every instance's foreign key.
#[test]
fn the_definition_is_renamed_in_step_and_its_slug_is_not() {
    let store = Store::new(
        vec![
            definition("crew-role:lead", "Lead"),
            definition("crew-role:lead-4", "Lead 4"),
            instance(
                "a".repeat(64).as_str(),
                "Lead",
                "lead",
                "crew-role:lead",
                LEGACY_TEAM,
                None,
            ),
            instance(
                "b".repeat(64).as_str(),
                "Lead 4",
                "lead",
                "crew-role:lead-4",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
        ],
        project_teams(),
    );
    store.run();

    let card = store
        .records()
        .into_iter()
        .find(|record| record.slug.as_deref() == Some("crew-role:lead-4"))
        .expect("the card");
    assert_eq!(card.name, "Lead");
    assert_eq!(card.display_name.as_deref(), Some("Lead"));
    assert_eq!(
        card.slug.as_deref(),
        Some("crew-role:lead-4"),
        "the slug is a published d tag; renumbering it would move events"
    );
}

/// The kind:0 republish is queued with the POST-rename name, and the queue is
/// written before the store so a crash in between leaves an inert entry rather
/// than a rename nobody will ever publish.
#[test]
fn the_owed_profile_publish_is_queued_under_the_new_name() {
    let store = Store::new(
        vec![
            definition("crew-role:lead", "Lead"),
            definition("crew-role:lead-4", "Lead 4"),
            instance(
                "a".repeat(64).as_str(),
                "Lead",
                "lead",
                "crew-role:lead",
                LEGACY_TEAM,
                None,
            ),
            instance(
                "b".repeat(64).as_str(),
                "Lead 4",
                "lead",
                "crew-role:lead-4",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
        ],
        project_teams(),
    );
    store.run();

    let queue = super::super::profile_reconcile::read_profile_reconcile_queue(
        &super::super::profile_reconcile::profile_reconcile_queue_path(
            &store.dir.path().join("managed-agents.json"),
        ),
    )
    .expect("queue readable");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].pubkey, "b".repeat(64));
    assert_eq!(
        queue[0].expected_name, "Lead",
        "the drain only fires while the queued name matches the record"
    );
}

// ── What it refuses ──────────────────────────────────────────────────────

/// The machine-wide `Team roles` teams keep their numbers. There is no project
/// to be the namespace, and on a real store two of them hold an `Architect` and
/// an `Architect 2` that are different identities.
#[test]
fn a_suffix_outside_a_project_team_is_kept_and_the_reason_is_said() {
    let store = Store::new(
        vec![
            definition("crew-role:architect", "Architect"),
            definition("crew-role:architect-2", "Architect 2"),
            instance(
                "a".repeat(64).as_str(),
                "Architect",
                "architect",
                "crew-role:architect",
                LEGACY_TEAM,
                None,
            ),
            instance(
                "b".repeat(64).as_str(),
                "Architect 2",
                "architect",
                "crew-role:architect-2",
                "team-roles-two",
                None,
            ),
        ],
        vec![
            (LEGACY_TEAM, "Team roles"),
            ("team-roles-two", "Team roles"),
        ],
    );

    let outcome = store.run();
    assert!(outcome.renamed.is_empty());
    assert_eq!(outcome.kept.len(), 1);
    assert!(
        outcome.kept[0].reason.contains("no project"),
        "the refusal has to say why: {}",
        outcome.kept[0].reason
    );
    assert_eq!(store.named(&"b".repeat(64)), "Architect 2");
}

/// An operator who typed `Builder 2` over an installer-minted `Builder` broke
/// the definition's equality with it. That is the signal the last write was a
/// person's, not the installer's.
#[test]
fn a_name_an_operator_typed_is_left_alone() {
    let mut card = definition("crew-role:builder-2", "Builder");
    card.as_object_mut()
        .expect("object")
        .insert("display_name".into(), serde_json::json!("Builder"));
    let store = Store::new(
        vec![
            definition("crew-role:builder", "Builder"),
            card,
            instance(
                "a".repeat(64).as_str(),
                "Builder",
                "builder",
                "crew-role:builder",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
            instance(
                "b".repeat(64).as_str(),
                "Builder 2",
                "builder",
                "crew-role:builder-2",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
        ],
        project_teams(),
    );

    let outcome = store.run();
    assert!(
        outcome.renamed.is_empty(),
        "the card still says Builder, so the rename was a person's"
    );
    assert_eq!(store.named(&"b".repeat(64)), "Builder 2");
}

/// A hand-made agent has no `home_role`; the installer never guesses one.
#[test]
fn an_agent_the_installer_never_minted_is_left_alone() {
    let mut hand_made = base_record(&"b".repeat(64), "Builder 2");
    hand_made
        .as_object_mut()
        .expect("object")
        .insert("team_id".into(), serde_json::json!(ALPHA_TEAM));
    let store = Store::new(
        vec![
            definition("crew-role:builder", "Builder"),
            instance(
                "a".repeat(64).as_str(),
                "Builder",
                "builder",
                "crew-role:builder",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
            hand_made,
        ],
        project_teams(),
    );

    assert!(store.run().renamed.is_empty());
    assert_eq!(store.named(&"b".repeat(64)), "Builder 2");
}

/// A lone `Agent 7` was not this installer working around a collision, because
/// there is no collision on disk to work around.
#[test]
fn a_suffix_with_no_collision_behind_it_is_not_this_installers() {
    let store = Store::new(
        vec![
            definition("crew-role:builder-7", "Agent 7"),
            instance(
                "b".repeat(64).as_str(),
                "Agent 7",
                "builder",
                "crew-role:builder-7",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
        ],
        project_teams(),
    );

    let outcome = store.run();
    assert!(outcome.renamed.is_empty());
    assert_eq!(outcome.kept.len(), 1);
    assert!(outcome.kept[0].reason.contains("was not this installer"));
}

/// A strip that would land on a name already standing leaves the record
/// untouched. Never renumber, never move the other record out of the way.
#[test]
fn a_strip_that_would_collide_leaves_the_record_as_it_is() {
    let store = Store::new(
        vec![
            definition("crew-role:builder", "Builder"),
            definition("crew-role:builder-2", "Builder 2"),
            // Already holds the plain name, in the same project.
            instance(
                "a".repeat(64).as_str(),
                "Builder",
                "builder",
                "crew-role:builder",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
            instance(
                "b".repeat(64).as_str(),
                "Builder 2",
                "builder",
                "crew-role:builder-2",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
        ],
        project_teams(),
    );

    let outcome = store.run();
    assert!(outcome.renamed.is_empty());
    assert_eq!(store.named(&"b".repeat(64)), "Builder 2");
    assert!(outcome.kept[0].reason.contains("already taken"));
}

/// A name nothing minted a number onto is not a subject at all.
#[test]
fn a_deliberately_chosen_name_is_never_a_subject() {
    assert_eq!(super::split_minted_suffix("Keystone"), None);
    assert_eq!(
        super::split_minted_suffix("Lead 1"),
        None,
        "the installer starts at 2"
    );
    assert_eq!(super::split_minted_suffix("Lead 02"), None, "leading zero");
    assert_eq!(
        super::split_minted_suffix("Lead 2 3"),
        None,
        "not a shape it produces"
    );
    assert_eq!(super::split_minted_suffix(" 2"), None);
    assert_eq!(super::split_minted_suffix("Lead 4"), Some(("Lead", 4)));
}

// ── Running twice ────────────────────────────────────────────────────────

/// A second boot changes nothing, and writes nothing.
#[test]
fn a_second_run_is_a_no_op() {
    let store = Store::new(
        vec![
            definition("crew-role:lead", "Lead"),
            definition("crew-role:lead-4", "Lead 4"),
            instance(
                "a".repeat(64).as_str(),
                "Lead",
                "lead",
                "crew-role:lead",
                LEGACY_TEAM,
                None,
            ),
            instance(
                "b".repeat(64).as_str(),
                "Lead 4",
                "lead",
                "crew-role:lead-4",
                ALPHA_TEAM,
                Some(&project("alpha")),
            ),
        ],
        project_teams(),
    );
    store.run();
    let after_first = std::fs::read(store.dir.path().join("managed-agents.json")).expect("read");

    let second = store.run();
    assert!(second.renamed.is_empty());
    assert_eq!(
        std::fs::read(store.dir.path().join("managed-agents.json")).expect("read"),
        after_first,
        "a second boot rewrites nothing"
    );
}

/// An empty store is a clean no-op; a missing one too.
#[test]
fn nothing_to_do_writes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(
        scope_agent_names_to_projects_in_dir(dir.path()).expect("succeeds"),
        NameScopeOutcome::default()
    );
    assert!(!dir.path().join("managed-agents.json").exists());
}
