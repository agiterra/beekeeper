//! What a pack-to-flat conversion must preserve, and what it may not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::compose::{compose_role, ComposeOptions, RoleSource};
use crate::team::{load_team, AgentsRepoAccess};
use crate::template::TemplateCatalog;

/// A temporary directory removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        // A counter, not a clock: two tests starting in the same
        // microsecond on macOS otherwise share a directory, and each then
        // converts the other's packs.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("buzz-migrate-{tag}-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&base).expect("create temp dir");
        Self(base)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, text).expect("write");
}

/// A pack directory: manifest, one persona, and the named skills.
fn write_pack(root: &Path, role: &str, frontmatter: &str, body: &str, skills: &[(&str, &str)]) {
    let dir = root.join(role);
    write(
        &dir.join(".plugin/plugin.json"),
        &format!(
            "{{\n  \"id\": \"com.example.{role}\",\n  \"name\": \"{role}\",\n  \"version\": \"0.4.0\",\n  \"personas\": [\"personas/{role}.persona.md\"]\n}}\n"
        ),
    );
    write(
        &dir.join(format!("personas/{role}.persona.md")),
        &format!("---\n{frontmatter}---\n{body}"),
    );
    for (name, text) in skills {
        write(&dir.join("skills").join(name).join("SKILL.md"), text);
    }
}

/// The lead pack the other tests convert: two claimed skills, one the
/// frontmatter never names.
fn write_lead_pack(root: &Path) {
    write_pack(
        root,
        "lead",
        "name: lead\nrole: lead\ndisplay_name: \"Lead\"\ndescription: \"Rules, briefs lanes, reads reports.\"\nskills:\n  - \"./skills/write-brief/\"\n  - \"./skills/hire/\"\n",
        "You are the lead seat of a team.\n\n1. **Rule** — decide tiers.\n",
        &[
            ("write-brief", "---\nname: write-brief\ndescription: \"Write a brief.\"\n---\n\nA brief is law.\n"),
            ("hire", "---\nname: hire\ndescription: \"Hire a seat.\"\n---\n\nHire with the CLI.\n"),
            ("unclaimed", "---\nname: unclaimed\ndescription: \"Shared by the pack rule.\"\n---\n\nStill reaches the seat.\n"),
        ],
    );
}

fn catalog() -> TemplateCatalog {
    TemplateCatalog::empty("test")
}

#[test]
fn a_converted_role_keeps_its_body_and_every_skill_byte_for_byte() {
    let src = TempDir::new("src");
    let dest = TempDir::new("dest");
    write_lead_pack(src.path());

    let report = convert_pack_tree(src.path(), dest.path(), "demo").expect("convert");
    assert_eq!(report.role_slugs(), vec!["lead".to_owned()]);
    assert_eq!(report.lead, "lead");

    let from_pack = compose_role(
        &RoleSource::Pack {
            dir: src.path().join("lead"),
            role: "lead".to_owned(),
            persona: None,
        },
        &catalog(),
        &ComposeOptions::local("pack"),
    )
    .expect("compose the pack");
    let from_flat = compose_role(
        &RoleSource::Flat {
            root: dest.path().to_path_buf(),
            role: "lead".to_owned(),
        },
        &catalog(),
        &ComposeOptions::local("flat"),
    )
    .expect("compose the converted role");

    // The prompt is the seat's instructions: identical, byte for byte.
    assert_eq!(from_flat.persona.prompt, from_pack.persona.prompt);
    assert_eq!(from_flat.persona.description, from_pack.persona.description);
    assert_eq!(
        from_flat.persona.display_name,
        from_pack.persona.display_name
    );

    // Every skill the pack carried is still there, with the same bytes.
    let names = |composed: &crate::compose::ComposedRole| {
        let mut names: Vec<String> = composed.skills.iter().map(|s| s.name.clone()).collect();
        names.sort();
        names
    };
    assert_eq!(names(&from_flat), names(&from_pack));
    for skill in &from_pack.skills {
        let after = from_flat
            .skills
            .iter()
            .find(|s| s.name == skill.name)
            .unwrap_or_else(|| panic!("{} survived", skill.name));
        assert_eq!(
            std::fs::read(after.dir.join("SKILL.md")).expect("read converted skill"),
            std::fs::read(skill.dir.join("SKILL.md")).expect("read pack skill"),
            "{} kept its bytes",
            skill.name
        );
    }
}

#[test]
fn the_frontmatter_skills_list_is_dropped_and_the_drop_is_reported() {
    let src = TempDir::new("src");
    let dest = TempDir::new("dest");
    write_lead_pack(src.path());

    let report = convert_pack_tree(src.path(), dest.path(), "demo").expect("convert");
    assert_eq!(report.roles[0].dropped_keys, vec!["skills".to_owned()]);

    let text = std::fs::read_to_string(dest.path().join("roles/lead.md")).expect("role file");
    assert!(!text.contains("skills:"), "no skills list: {text}");
    assert!(text.contains("description:"), "description kept: {text}");
    assert!(
        text.contains("You are the lead seat of a team."),
        "body verbatim: {text}"
    );

    // The skills still reach the seat — as role-private directories.
    assert!(dest
        .path()
        .join("roles/lead/skills/hire/SKILL.md")
        .is_file());
    assert!(dest
        .path()
        .join("roles/lead/skills/unclaimed/SKILL.md")
        .is_file());
}

#[test]
fn every_role_may_read_the_agents_repository_and_the_lead_may_write() {
    let src = TempDir::new("src");
    let dest = TempDir::new("dest");
    write_lead_pack(src.path());
    write_pack(
        src.path(),
        "builder",
        "name: builder\nrole: builder\ndescription: \"Implements a locked brief.\"\n",
        "You build what the brief says.\n",
        &[],
    );

    let report = convert_pack_tree(src.path(), dest.path(), "demo").expect("convert");
    assert_eq!(
        report.role_slugs(),
        vec!["builder".to_owned(), "lead".to_owned()]
    );

    let team = load_team(dest.path())
        .expect("team.yml parses")
        .expect("team.yml exists");
    assert_eq!(team.lead.as_deref(), Some("lead"));
    assert_eq!(
        team.role("lead").workspace.agents_repo,
        Some(AgentsRepoAccess::Write)
    );
    assert_eq!(
        team.role("builder").workspace.agents_repo,
        Some(AgentsRepoAccess::Read)
    );
    let agents: BTreeMap<&str, &str> = team
        .agents
        .iter()
        .map(|a| (a.role.as_str(), a.name.as_str()))
        .collect();
    assert_eq!(agents.get("lead"), Some(&"Lead"));
    assert_eq!(agents.get("builder"), Some(&"Builder"));
}

#[test]
fn the_conversion_writes_the_same_furniture_a_seed_does() {
    let src = TempDir::new("src");
    let dest = TempDir::new("dest");
    write_lead_pack(src.path());

    let report = convert_pack_tree(src.path(), dest.path(), "demo").expect("convert");
    for expected in [
        "README.md",
        "team.yml",
        "actions.yml",
        "model-registry.yaml",
        "roles/archive/.gitkeep",
        "skills/.gitkeep",
        "plans/.gitkeep",
        "plans/archive/.gitkeep",
    ] {
        assert!(
            report.files.iter().any(|f| f == expected),
            "{expected} written; got {:?}",
            report.files
        );
        assert!(dest.path().join(expected).exists(), "{expected} on disk");
    }
}

#[test]
fn a_tree_with_no_packs_refuses_and_writes_nothing() {
    let src = TempDir::new("src");
    let dest = TempDir::new("dest");
    std::fs::create_dir_all(src.path().join("not-a-pack")).expect("create");

    let error = convert_pack_tree(src.path(), dest.path(), "demo").expect_err("refuses");
    assert!(
        matches!(error, MigrateError::NoRoles { .. }),
        "got {error:?}"
    );
    assert_eq!(
        std::fs::read_dir(dest.path()).expect("read dest").count(),
        0,
        "nothing written"
    );
}

#[test]
fn a_non_empty_destination_refuses_before_reading_anything() {
    let src = TempDir::new("src");
    let dest = TempDir::new("dest");
    write_lead_pack(src.path());
    write(&dest.path().join("README.md"), "someone's work\n");

    let error = convert_pack_tree(src.path(), dest.path(), "demo").expect_err("refuses");
    assert!(matches!(error, MigrateError::Seed(_)), "got {error:?}");
    assert_eq!(
        std::fs::read_to_string(dest.path().join("README.md")).expect("read"),
        "someone's work\n",
        "the existing file is untouched"
    );
}

#[test]
fn a_persona_declaring_another_role_refuses_by_name() {
    let src = TempDir::new("src");
    let dest = TempDir::new("dest");
    write_pack(
        src.path(),
        "builder",
        "name: builder\nrole: lead\ndescription: \"Mislabelled.\"\n",
        "Body.\n",
        &[],
    );

    let error = convert_pack_tree(src.path(), dest.path(), "demo").expect_err("refuses");
    match error {
        MigrateError::RoleMismatch { declared, role, .. } => {
            assert_eq!(declared, "lead");
            assert_eq!(role, "builder");
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn a_preview_lists_what_a_conversion_would_convert_without_writing() {
    let src = TempDir::new("src");
    write_lead_pack(src.path());
    write_pack(
        src.path(),
        "runner",
        "role: runner\ndescription: \"Runs commands.\"\n",
        "You run commands.\n",
        &[],
    );

    assert_eq!(
        preview_pack_tree(src.path()).expect("preview"),
        vec!["lead".to_owned(), "runner".to_owned()]
    );
}

/// Convert a real pack tree named by `BUZZ_MIGRATE_FIXTURE` and prove every
/// role's prompt and skills survive it. Ignored by default: it needs a
/// packs checkout this machine happens to have.
///
/// ```text
/// BUZZ_MIGRATE_FIXTURE="$HOME/Library/Application Support/io.agiterra.beekeeper.app/packs/3d3b7169-agiterra-packs/personas/roles" \
///   cargo test -p beekeeper-persona --lib migrate -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs a real packs checkout named by BUZZ_MIGRATE_FIXTURE"]
fn a_real_pack_tree_converts_with_every_prompt_and_skill_intact() {
    let Ok(fixture) = std::env::var("BUZZ_MIGRATE_FIXTURE") else {
        panic!("set BUZZ_MIGRATE_FIXTURE to a directory holding one pack per role");
    };
    let src = PathBuf::from(fixture);
    let dest = TempDir::new("real");

    let report = convert_pack_tree(&src, dest.path(), "bee-keeper").expect("convert");
    println!(
        "converted {} roles: {:?}",
        report.roles.len(),
        report.role_slugs()
    );

    for converted in &report.roles {
        let role = converted.role.clone();
        let from_pack = compose_role(
            &RoleSource::Pack {
                dir: src.join(&role),
                role: role.clone(),
                persona: None,
            },
            &catalog(),
            &ComposeOptions::local("pack"),
        )
        .unwrap_or_else(|error| panic!("compose pack {role}: {error}"));
        let from_flat = compose_role(
            &RoleSource::Flat {
                root: dest.path().to_path_buf(),
                role: role.clone(),
            },
            &catalog(),
            &ComposeOptions::local("flat"),
        )
        .unwrap_or_else(|error| panic!("compose converted {role}: {error}"));

        assert_eq!(
            from_flat.persona.prompt, from_pack.persona.prompt,
            "{role}: prompt byte-identical"
        );
        assert_eq!(
            from_flat.persona.description, from_pack.persona.description,
            "{role}: description kept"
        );
        let names = |composed: &crate::compose::ComposedRole| {
            let mut names: Vec<String> = composed.skills.iter().map(|s| s.name.clone()).collect();
            names.sort();
            names
        };
        assert_eq!(names(&from_flat), names(&from_pack), "{role}: same skills");
        for skill in &from_pack.skills {
            let after = from_flat
                .skills
                .iter()
                .find(|s| s.name == skill.name)
                .unwrap_or_else(|| panic!("{role}: {} survived", skill.name));
            assert_eq!(
                std::fs::read(after.dir.join("SKILL.md")).expect("read converted"),
                std::fs::read(skill.dir.join("SKILL.md")).expect("read pack"),
                "{role}: skill {} kept its bytes",
                skill.name
            );
        }
        println!(
            "  {role}: {} bytes of prompt, {} skills, dropped {:?}",
            from_flat.persona.prompt.len(),
            from_flat.skills.len(),
            converted.dropped_keys
        );
    }
}
