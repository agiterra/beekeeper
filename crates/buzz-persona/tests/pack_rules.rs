//! Product contracts for the neutral role packs this build actually ships.
//!
//! Project procedures belong in project-owned copies. Historical Beekeeper
//! operating rules are intentionally not requirements of this seed corpus.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use buzz_persona::{pack, skill_meta, validate};

const ROLES: &[&str] = &[
    "architect",
    "builder",
    "designer",
    "lead",
    "poker",
    "project-setup",
    "runner",
    "verifier",
];

fn roles_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under the repository's crates directory")
        .join("personas/roles")
}

fn children(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .expect("read directory")
        .map(|entry| entry.expect("read directory entry").path())
        .collect()
}

#[test]
fn shipped_roles_load_with_complete_local_skills_and_no_provider_requirement() {
    let root = roles_root();
    let actual: BTreeSet<_> = children(&root)
        .into_iter()
        .filter(|path| path.is_dir())
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(actual, ROLES.iter().map(|role| role.to_string()).collect());

    let mut ids = BTreeSet::new();
    for role in ROLES {
        let role_dir = root.join(role);
        let validation = validate::validate_pack(&role_dir);
        assert!(!validation.has_errors(), "{role}: {validation:?}");
        let loaded = pack::load_pack(&role_dir).expect("load actual shipped pack");
        assert!(ids.insert(loaded.manifest.id.clone()), "duplicate pack id");
        assert_eq!(loaded.manifest.id, format!("com.beekeeper.crew.{role}"));
        assert_eq!(loaded.personas.len(), 1, "one responsibility per role pack");
        let persona = &loaded.personas[0];
        assert_eq!(persona.role.as_deref(), Some(*role));
        assert_eq!(persona.name, *role);
        assert!(persona.model.is_none(), "{role} requires a model");
        assert!(persona.runtime.is_none(), "{role} requires a runtime");
        assert!(loaded.shared_mcp_config.is_none());
        assert!(persona.mcp_servers.is_empty());
        assert!(!persona.skills.is_empty(), "{role} declares no procedure");

        let mut declared = BTreeSet::new();
        for skill in &persona.skills {
            let relative = skill
                .strip_prefix("./skills/")
                .expect("shipped skill is pack-local")
                .trim_end_matches('/');
            assert!(!relative.contains('/'), "unexpected nested skill {skill}");
            assert!(declared.insert(relative.to_owned()), "duplicate skill");
            let meta = skill_meta::read_skill_meta(&role_dir.join("skills").join(relative))
                .expect("declared skill has valid metadata and readable content");
            assert_eq!(meta.name, relative);
            assert!(!meta.description.is_empty(), "skill has no description");
        }
        let present: BTreeSet<_> = children(&role_dir.join("skills"))
            .into_iter()
            .filter(|path| path.is_dir())
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(declared, present, "{role} has an undeclared shared skill");
    }
}

// Scan the complete shipped tree, including manifests and unreferenced files:
// the desktop's seeder copies all of it, not only loaded persona instructions.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in children(root) {
        assert!(!path.is_symlink(), "shipped seed includes a symlink");
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn shipped_seed_contains_no_specific_project_or_machine_procedures() {
    let forbidden = [
        "agiterra",
        "tankloop",
        "brian",
        "andy",
        "/users/",
        "c:\\users\\",
        "activate-hermit",
        "just ci",
        "tauri dev",
        "origin/main",
        "git push origin",
        "docs/current_state.md",
        "docs/session_state.md",
        "docs/integration.md",
        "beekeeper-project",
        "live-run finding",
        "ledger item",
        "--no-verify",
    ];
    for path in files_under(&roles_root()) {
        let text = std::fs::read_to_string(&path).expect("shipped content is text");
        let text = text.to_lowercase();
        for marker in forbidden {
            assert!(
                !text.contains(marker),
                "{} contains project-specific instruction {marker:?}",
                path.display()
            );
        }
    }
}

#[test]
fn setup_preserves_project_copies_and_requires_scoped_publication() {
    let body =
        std::fs::read_to_string(roles_root().join("project-setup/skills/setup-project/SKILL.md"))
            .expect("setup skill");
    for contract in [
        "preserve unrelated customizations",
        "expected source\nrevision",
        "scoped publication capability",
        "do not impersonate the owner",
        "do not say the setup was saved",
        "Never claim adoption from publication alone",
    ] {
        assert!(body.contains(contract), "setup omits {contract:?}");
    }
}
