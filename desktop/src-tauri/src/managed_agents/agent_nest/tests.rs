//! One nest per agent.
//!
//! Every path here is a temporary directory this test owns. Nothing reads or
//! writes the operator's home — the lesson `skill_materialization_tests` paid
//! for once already.

use super::*;
use crate::managed_agents::nest::{materialize_persona_skills, materialize_persona_skills_outside};
use crate::managed_agents::types::AgentDefinition;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// A role pack holding one persona, `builder`, with one skill.
fn role_pack(root: &Path) -> PathBuf {
    let pack = root.join("pack");
    fs::create_dir_all(pack.join(".plugin")).unwrap();
    fs::create_dir_all(pack.join("personas")).unwrap();
    fs::create_dir_all(pack.join("skills/brief")).unwrap();
    fs::write(
        pack.join(".plugin/plugin.json"),
        r#"{"id":"com.test.roles","name":"Roles","version":"0.1.0","personas":["personas/builder.persona.md"]}"#,
    )
    .unwrap();
    fs::write(
        pack.join("personas/builder.persona.md"),
        "---\nname: builder\ndisplay_name: Builder\ndescription: Builds.\nrole: builder\n---\nYou build.\n",
    )
    .unwrap();
    fs::write(pack.join("skills/brief/SKILL.md"), "# Brief").unwrap();
    pack
}

/// A record shaped the way the crew-role installer leaves one: a pack link and
/// a home role. `pubkey` is what names its nest.
fn packed_record(pubkey: &str, pack: Option<PathBuf>) -> ManagedAgentRecord {
    let mut record = AgentDefinition {
        id: "def".into(),
        display_name: "Bob".into(),
        avatar_url: None,
        system_prompt: String::new(),
        runtime: None,
        model: None,
        provider: None,
        name_pool: vec![],
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        env_vars: Default::default(),
        respond_to: None,
        respond_to_allowlist: vec![],
        parallelism: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
    .into_agent_record();
    record.pubkey = pubkey.to_string();
    record.persona_name_in_team = pack.as_ref().map(|_| "builder".to_string());
    record.persona_team_dir = pack;
    record.home_role = Some("builder".into());
    record
}

const KEY_A: &str = "ede63017aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KEY_B: &str = "b0bb0bb0bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn a_nest_is_named_by_the_first_eight_hex_of_the_agents_key() {
    assert_eq!(agent_nest_name(KEY_A).as_deref(), Some("ede63017"));
    assert_eq!(agent_nest_name(KEY_B).as_deref(), Some("b0bb0bb0"));
    // Two agents, two nests: the whole point.
    assert_ne!(agent_nest_name(KEY_A), agent_nest_name(KEY_B));
}

#[test]
fn a_key_that_is_not_a_key_names_no_nest() {
    // The pubkey becomes a path component. A record hand-edited to carry a
    // traversal must not point `create_dir_all` at somewhere else on the disk.
    for bad in [
        "",
        "../../etc",
        "ede63017",
        "ede63017aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", // 62
        "ede63017aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", // 66
        "EDE63017AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "ede63017/../../../tmp/xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    ] {
        assert!(
            agent_nest_name(bad).is_none(),
            "{bad:?} must not name a nest"
        );
    }
}

#[test]
fn an_agents_own_nest_takes_the_packs_skills_the_shared_home_refuses() {
    // LANE-L33, the whole finding in one test. The same record, the same
    // pack: refused in the shared home, written in a nest of its own.
    let tmp = tempfile::tempdir().unwrap();
    let pack = role_pack(tmp.path());
    let home = tmp.path().join("home");
    let shared = home.join(".beekeeper-dev");
    fs::create_dir_all(&shared).unwrap();
    let shared_roots = vec![shared.clone(), home.clone()];
    let record = packed_record(KEY_A, Some(pack));

    let refusal = materialize_persona_skills_outside(&record, &shared, &shared_roots)
        .expect_err("the shared home refuses");
    assert!(refusal.contains("shared"), "{refusal}");
    assert!(!shared.join(".agents/skills/brief/SKILL.md").exists());

    let nest = tmp.path().join("app-data/agents/ede63017");
    ensure_agent_nest_at(&nest, None).expect("a nest of its own");
    let written = materialize_persona_skills_outside(&record, &nest, &shared_roots)
        .expect("an agent's own nest is not shared");

    assert_eq!(written.len(), 1);
    assert_eq!(
        fs::read_to_string(nest.join(".agents/skills/brief/SKILL.md")).unwrap(),
        "# Brief"
    );
}

#[test]
fn two_agents_never_write_into_each_others_skills() {
    // Why the shared home had to go: one persona's `brief/SKILL.md` was
    // overwriting another's at every spawn.
    let tmp = tempfile::tempdir().unwrap();
    let pack = role_pack(tmp.path());
    let one = tmp.path().join("agents/ede63017");
    let two = tmp.path().join("agents/b0bb0bb0");
    ensure_agent_nest_at(&one, None).unwrap();
    ensure_agent_nest_at(&two, None).unwrap();

    materialize_persona_skills(&packed_record(KEY_A, Some(pack.clone())), &one).unwrap();
    materialize_persona_skills(&packed_record(KEY_B, Some(pack)), &two).unwrap();

    assert!(one.join(".agents/skills/brief/SKILL.md").exists());
    assert!(two.join(".agents/skills/brief/SKILL.md").exists());
    assert_ne!(one, two);
}

#[test]
fn a_nest_is_a_nest_with_the_shared_repos() {
    // An agent moved out of the shared home keeps the operator's checkouts:
    // `REPOS` is a symlink at the shared nest's, not a fresh empty directory.
    let tmp = tempfile::tempdir().unwrap();
    let shared_repos = tmp.path().join("shared/REPOS");
    fs::create_dir_all(&shared_repos).unwrap();
    let nest = tmp.path().join("agents/ede63017");

    ensure_agent_nest_at(&nest, Some(&shared_repos)).expect("a nest");

    assert!(nest.join("AGENTS.md").is_file(), "the orientation file");
    assert!(nest.join("WORK_LOGS").is_dir());
    assert!(
        nest.join(".agents/skills/buzz-cli/SKILL.md").is_file(),
        "the buzz-cli skill"
    );
    #[cfg(unix)]
    assert!(
        nest.join("REPOS").symlink_metadata().unwrap().is_symlink(),
        "REPOS is shared, not a second empty directory"
    );
}

#[test]
fn creating_a_nest_twice_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let nest = tmp.path().join("agents/ede63017");
    ensure_agent_nest_at(&nest, None).unwrap();
    fs::write(nest.join("RESEARCH/note.md"), "kept").unwrap();

    ensure_agent_nest_at(&nest, None).expect("idempotent");

    assert_eq!(
        fs::read_to_string(nest.join("RESEARCH/note.md")).unwrap(),
        "kept",
        "a second call must not wipe what the agent accumulated"
    );
}

#[test]
fn the_shared_home_list_is_captured_once_and_never_recaptured() {
    // Nothing is migrated behind the operator's back, and nothing is dragged
    // back either: the list names the agents that were here when nests
    // arrived, and a later boot must not re-capture the ones since moved.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nests/shared-home.json");
    let both: BTreeSet<String> = [KEY_A.to_string(), KEY_B.to_string()].into();

    assert!(seed_shared_home_at(&path, &both).expect("first boot writes it"));
    assert!(
        !seed_shared_home_at(&path, &BTreeSet::new()).expect("a later boot writes nothing"),
        "a second seed must not overwrite the captured list"
    );
    assert_eq!(read_shared_home_at(&path).unwrap(), both);

    leave_shared_home_at(&path, KEY_A).expect("one agent moves out");
    assert_eq!(
        read_shared_home_at(&path).unwrap(),
        [KEY_B.to_string()].into()
    );
    // And the boot after that leaves the move alone.
    assert!(!seed_shared_home_at(&path, &both).unwrap());
    assert_eq!(
        read_shared_home_at(&path).unwrap(),
        [KEY_B.to_string()].into(),
        "a re-seed must not drag a moved agent back into the shared home"
    );
}

#[test]
fn an_unreadable_list_leaves_every_agent_where_it_is() {
    // Fail closed. `None` here makes `agent_home` answer `Shared` for
    // everyone: a disk error must never quietly move an agent out of the
    // directory its work is in.
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nests/shared-home.json");
    assert!(read_shared_home_at(&missing).is_none(), "absent");

    let garbage = tmp.path().join("garbage.json");
    fs::write(&garbage, "{not json").unwrap();
    assert!(read_shared_home_at(&garbage).is_none(), "malformed");

    // An empty list is a real answer, not a missing one: this computer had no
    // agents when nests arrived, so every agent it has now is a new one.
    let empty = tmp.path().join("empty/shared-home.json");
    seed_shared_home_at(&empty, &BTreeSet::new()).unwrap();
    assert_eq!(read_shared_home_at(&empty).unwrap(), BTreeSet::new());
}

#[test]
fn taking_an_agent_off_a_list_it_is_not_on_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nests/shared-home.json");
    seed_shared_home_at(&path, &[KEY_B.to_string()].into()).unwrap();

    leave_shared_home_at(&path, KEY_A).expect("a no-op, not an error");

    assert_eq!(
        read_shared_home_at(&path).unwrap(),
        [KEY_B.to_string()].into()
    );
}
