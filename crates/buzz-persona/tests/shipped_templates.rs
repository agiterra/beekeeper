//! The shipped catalog under `personas/templates` and the shipped role packs
//! under `personas/roles` are real inputs to the composer, and this pins
//! them: the catalog loads and validates clean, every shipped role composes
//! from its pack byte-for-byte (no includes yet — spec slice A5 adds them),
//! and the `working-contract` template is exactly the paragraph those roles
//! repeat today, so A5 can replace it without changing a single seat's
//! instructions.

use std::path::{Path, PathBuf};

use buzz_persona::compose::{compose_role, write_staged_pack, ComposeOptions, RoleSource};
use buzz_persona::template::{validate_catalog, TemplateCatalog, TemplateRange};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("crate lives two levels under the repo root")
}

fn shipped_templates() -> PathBuf {
    repo_root().join("personas").join("templates")
}

fn shipped_roles() -> PathBuf {
    repo_root().join("personas").join("roles")
}

const SHIPPED_ROLES: [&str; 8] = [
    "architect",
    "builder",
    "designer",
    "lead",
    "poker",
    "project-setup",
    "runner",
    "verifier",
];

#[test]
fn the_shipped_catalog_loads_and_validates_clean() {
    let catalog = TemplateCatalog::load(&shipped_templates(), "test").expect("catalog loads");
    let names: Vec<&str> = catalog.names().collect();
    assert_eq!(names, vec!["memory", "project-pulse", "working-contract"]);
    for name in &names {
        let latest = catalog
            .resolve(name, &TemplateRange::Latest)
            .unwrap_or_else(|e| panic!("{name}@latest: {e}"));
        assert!(latest.warning.is_none(), "{name}@latest is deprecated");
        assert!(!latest.template.description.is_empty());
        assert!(!latest.template.body.trim().is_empty());
    }
    let report = validate_catalog(&shipped_templates());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert!(!report.has_warnings(), "{:?}", report.diagnostics);
}

#[test]
fn the_working_contract_template_is_the_paragraph_every_shipped_role_repeats() {
    let catalog = TemplateCatalog::load(&shipped_templates(), "test").expect("catalog loads");
    let template = catalog
        .resolve("working-contract", &TemplateRange::Latest)
        .expect("working-contract@latest")
        .template;
    for role in SHIPPED_ROLES {
        let persona = std::fs::read_to_string(
            shipped_roles()
                .join(role)
                .join("personas")
                .join(format!("{role}.persona.md")),
        )
        .expect("persona file");
        let section = persona
            .find("## Working contract")
            .map(|at| &persona[at..])
            .unwrap_or_else(|| panic!("{role} has no Working contract section"));
        assert_eq!(
            section, template.body,
            "{role}'s working contract differs from the template"
        );
    }
}

#[test]
fn every_shipped_role_composes_from_its_pack_byte_identical_and_restages_as_a_valid_pack() {
    let catalog = TemplateCatalog::load(&shipped_templates(), "test").expect("catalog loads");
    let staging = tempfile::tempdir().expect("tempdir");
    for role in SHIPPED_ROLES {
        let dir = shipped_roles().join(role);
        let source = RoleSource::Pack {
            dir: dir.clone(),
            role: role.to_owned(),
            persona: None,
        };
        let composed = compose_role(
            &source,
            &catalog,
            &ComposeOptions::local(format!("personas/roles/{role}")),
        )
        .unwrap_or_else(|e| panic!("{role}: {e}"));
        let original = buzz_persona::resolve::resolve_persona_by_name(&dir, role)
            .unwrap_or_else(|e| panic!("{role}: {e}"));
        assert_eq!(
            composed.persona.prompt, original.system_prompt,
            "{role} body changed"
        );
        assert!(
            composed.provenance.includes.is_empty(),
            "{role} has no includes yet"
        );
        assert!(
            composed.provenance.warnings.is_empty(),
            "{role}: {:?}",
            composed.provenance.warnings
        );

        let dest = staging.path().join(role);
        write_staged_pack(&composed, &dest).unwrap_or_else(|e| panic!("{role}: {e}"));
        let staged = buzz_persona::resolve::resolve_persona_by_name(&dest, role)
            .unwrap_or_else(|e| panic!("{role} staged: {e}"));
        assert_eq!(staged.system_prompt, original.system_prompt);
        assert_eq!(staged.role.as_deref(), Some(role));
        let mut expected_skills = original.skills.clone();
        expected_skills.sort();
        let mut staged_skills = staged.skills.clone();
        staged_skills.sort();
        assert_eq!(staged_skills, expected_skills, "{role} skills changed");
        let report = buzz_persona::validate::validate_pack(&dest);
        assert!(!report.has_errors(), "{role}: {:?}", report.diagnostics);
    }
}
