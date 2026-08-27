//! Materializing a persona pack's skills into an agent's working directory.
//!
//! Split out of `nest.rs` for the repository file-size ratchet; the code under
//! test lives in the parent module.

use super::*;
use std::fs;
use std::path::{Path, PathBuf};

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

fn record(pack_dir: Option<PathBuf>, persona: Option<&str>) -> ManagedAgentRecord {
    let mut record = crate::managed_agents::types::AgentDefinition {
        id: "def".into(),
        display_name: "Builder".into(),
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
    record.persona_team_dir = pack_dir;
    record.persona_name_in_team = persona.map(str::to_owned);
    record
}

#[test]
fn a_pack_persona_gets_its_skills_in_the_workdir() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = role_pack(tmp.path());
    let workdir = tmp.path().join("nest");
    fs::create_dir_all(&workdir).unwrap();

    let written = materialize_persona_skills(&record(Some(pack), Some("builder")), &workdir)
        .expect("materializes");

    assert_eq!(written.len(), 1);
    assert_eq!(
        fs::read_to_string(workdir.join(".agents/skills/brief/SKILL.md")).unwrap(),
        "# Brief"
    );
    assert!(written[0].written);
}

#[test]
fn a_second_spawn_rewrites_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = role_pack(tmp.path());
    let workdir = tmp.path().join("nest");
    fs::create_dir_all(&workdir).unwrap();
    let record = record(Some(pack), Some("builder"));

    materialize_persona_skills(&record, &workdir).expect("first spawn");
    let again = materialize_persona_skills(&record, &workdir).expect("second spawn");

    assert!(!again[0].written, "an unchanged skill is rewritten");
}

#[test]
fn an_agent_with_no_pack_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let workdir = tmp.path().join("nest");
    fs::create_dir_all(&workdir).unwrap();

    // Hand-built agent: no pack at all.
    assert!(materialize_persona_skills(&record(None, None), &workdir)
        .expect("no-op")
        .is_empty());
    // Half-linked record: a pack with no persona named in it.
    let pack = role_pack(tmp.path());
    assert!(
        materialize_persona_skills(&record(Some(pack), None), &workdir)
            .expect("no-op")
            .is_empty()
    );
    assert!(!workdir.join(".agents").exists());
}

#[test]
fn a_record_built_the_way_production_builds_one_has_no_pack_link() {
    // Not a style point: `into_agent_record` sets persona_team_dir and
    // persona_name_in_team to None, no production code assigns them, and
    // detach clears them — so this call site materializes nothing in
    // current builds. A test that hand-sets both fields reads like a live
    // feature; this one is the production shape.
    let tmp = tempfile::tempdir().unwrap();
    let workdir = tmp.path().join("seat");
    fs::create_dir_all(&workdir).unwrap();
    let produced = record(None, None);
    assert!(produced.persona_team_dir.is_none());
    assert!(produced.persona_name_in_team.is_none());
    assert!(materialize_persona_skills(&produced, &workdir)
        .expect("no pack, no work")
        .is_empty());
}

#[test]
fn a_shared_workdir_is_refused_rather_than_written_into() {
    // The one live caller passes `default_agent_workdir()`: the shared
    // nest, or $HOME when the nest is missing or a symlink. Either way it
    // is not a seat's own directory — and in the $HOME case the write
    // would land on a person's own ~/.agents/skills/<name>/SKILL.md, which
    // `materialize_skills` overwrites whenever the bytes differ.
    //
    // The shared roots are stand-ins this test owns. Passing the real home
    // here would assert on whatever that home already contains: an earlier
    // build of this feature wrote `~/.agents/skills/brief/SKILL.md` on the
    // operator's machine, and this very assertion then failed forever on
    // that machine while the guard underneath it was working.
    let tmp = tempfile::tempdir().unwrap();
    let pack = role_pack(tmp.path());
    let home = tmp.path().join("home");
    let nest = home.join(".beekeeper");
    fs::create_dir_all(&nest).unwrap();
    let roots = vec![nest.clone(), home.clone()];

    for shared in [&nest, &home] {
        let error = materialize_persona_skills_outside(
            &record(Some(pack.clone()), Some("builder")),
            shared,
            &roots,
        )
        .expect_err("a shared workdir must be refused");

        assert!(error.contains("shared"), "{error}");
        assert!(
            !shared.join(".agents/skills/brief/SKILL.md").exists(),
            "nothing may be written into the shared directory"
        );
    }
    assert!(
        !home.join(".agents").exists(),
        "no .agents tree was created"
    );
}

#[test]
fn a_seat_directory_beside_the_shared_roots_is_still_written() {
    // The guard is "is this one of the shared directories", not "is this
    // anywhere near them" — a seat whose checkout sits inside the home
    // directory is an ordinary seat.
    let tmp = tempfile::tempdir().unwrap();
    let pack = role_pack(tmp.path());
    let home = tmp.path().join("home");
    let seat = home.join("Projects/checkout");
    fs::create_dir_all(&seat).unwrap();
    let roots = vec![home.join(".beekeeper"), home];

    let written =
        materialize_persona_skills_outside(&record(Some(pack), Some("builder")), &seat, &roots)
            .expect("a seat's own directory is not shared");

    assert_eq!(written.len(), 1);
    assert!(seat.join(".agents/skills/brief/SKILL.md").exists());
}

#[test]
fn the_live_shared_roots_are_the_nest_and_the_home_directory() {
    // The seam above is only honest if the production call still names the
    // real shared directories. Path values only — nothing here touches the
    // filesystem under a person's home.
    let roots = shared_agent_workdir_roots();
    assert!(
        roots.iter().any(|root| Some(root) == nest_dir().as_ref()),
        "the nest is a shared root: {roots:?}"
    );
    assert!(
        roots
            .iter()
            .any(|root| Some(root) == dirs::home_dir().as_ref()),
        "the home directory is a shared root: {roots:?}"
    );
}

#[test]
fn an_unreadable_pack_is_reported_not_swallowed() {
    let tmp = tempfile::tempdir().unwrap();
    let workdir = tmp.path().join("nest");
    fs::create_dir_all(&workdir).unwrap();

    let error = materialize_persona_skills(
        &record(Some(tmp.path().join("missing-pack")), Some("builder")),
        &workdir,
    )
    .expect_err("an absent pack is an error");

    assert!(error.contains("builder"), "{error}");
    assert!(error.contains("missing-pack"), "{error}");
}
