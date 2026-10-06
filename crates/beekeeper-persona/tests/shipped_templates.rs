//! The shipped catalog under `personas/templates` and the shipped role packs
//! under `personas/roles` are real inputs to the composer, and this pins
//! them. Since 2026-09-18 (spec § 4.11) every shipped role is a `kind: role`
//! template plus a thin pack of two include lines, and a project's seeded
//! `roles/<role>.md` is an include of the same template. Three things must
//! stay true:
//!
//! - the catalog loads and validates clean, with the eight roles and the
//!   three shared fragments;
//! - every thin pack composes to the **current** text of its role template
//!   and the current shared contract, stages and validates, and keeps the
//!   identity (description, display name) its persona had before it was
//!   thinned (`tests/fixtures/shipped-roles-2026-09-18/`). Until 1.1.0 the
//!   composed *body* was pinned to those same fixture bytes; role text is
//!   now expected to advance through new template versions (1.1.0, then
//!   1.2.0), so every assertion here derives the expected text from the
//!   version a `@^1.0.0` include resolves to, and the guarantee that a
//!   published version never changes is pinned by hash in
//!   `src/role_work_contract_tests.rs`;
//! - the seed writer, given this catalog, yields roles that compose with
//!   the template's skills and the role's own paragraph.

use std::path::{Path, PathBuf};

use beekeeper_persona::compose::{compose_role, write_staged_pack, ComposeOptions, RoleSource};
use beekeeper_persona::persona::parse_persona_md;
use beekeeper_persona::seed::{write_agents_repo_seed, SHARED_FRAGMENTS};
use beekeeper_persona::template::{
    validate_catalog, Template, TemplateCatalog, TemplateKind, TemplateRange,
};

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

/// The persona files as they were before the roles became templates: the
/// bytes a seat received on 2026-09-17.
fn fixture(role: &str) -> beekeeper_persona::persona::PersonaConfig {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/shipped-roles-2026-09-18")
        .join(format!("{role}.persona.md"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    parse_persona_md(&text).unwrap_or_else(|e| panic!("{role} fixture: {e}"))
}

/// The version of `name` a `@^1.0.0` include resolves to in this build: what
/// a seat composed today actually receives.
fn current(catalog: &TemplateCatalog, name: &str) -> Template {
    let range = TemplateRange::parse(name, "^1.0.0").expect("caret range parses");
    let resolved = catalog
        .resolve(name, &range)
        .unwrap_or_else(|error| panic!("{name}@^1.0.0: {error}"));
    assert!(resolved.warning.is_none(), "{name}@^1.0.0: {resolved:?}");
    resolved.template
}

/// The skills a composed role must carry: its role template's, by name.
fn template_skill_names(template: &Template) -> Vec<String> {
    let mut names: Vec<String> = template
        .skills
        .iter()
        .map(|s| {
            s.trim_start_matches("./skills/")
                .trim_end_matches('/')
                .to_owned()
        })
        .collect();
    names.sort();
    names
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
    assert_eq!(
        names,
        vec![
            "architect",
            "builder",
            "designer",
            "lead",
            "memory",
            "poker",
            "project-pulse",
            "project-setup",
            "runner",
            "verifier",
            "working-contract",
        ]
    );
    for name in &names {
        let latest = catalog
            .resolve(name, &TemplateRange::Latest)
            .unwrap_or_else(|e| panic!("{name}@latest: {e}"));
        assert!(latest.warning.is_none(), "{name}@latest is deprecated");
        assert!(!latest.template.description.is_empty());
        assert!(!latest.template.body.trim().is_empty());
        let expected_kind = if SHIPPED_ROLES.contains(name) {
            TemplateKind::Role
        } else {
            TemplateKind::Fragment
        };
        assert_eq!(latest.template.kind, expected_kind, "{name}");
    }
    let roles: Vec<&str> = catalog
        .role_templates()
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(roles, SHIPPED_ROLES);
    for fragment in SHARED_FRAGMENTS {
        assert!(names.contains(&fragment), "the seed needs {fragment}");
    }
    let report = validate_catalog(&shipped_templates());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert!(!report.has_warnings(), "{:?}", report.diagnostics);
}

#[test]
fn every_thin_shipped_pack_composes_to_its_current_template_text_and_restages_as_a_valid_pack() {
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
        let before = fixture(role);
        // The body is the *current* text of the two templates the thin
        // persona includes, in order. A new template version reaches a hire
        // through the caret range; what may never change is a published
        // version's own bytes, which `role_work_contract_tests.rs` pins.
        let role_template = &current(&catalog, role);
        let contract = current(&catalog, "working-contract");
        assert!(
            composed.persona.prompt.contains(role_template.body.trim()),
            "{role}: the composed body is not {}@{}",
            role_template.name,
            role_template.version
        );
        assert!(
            composed.persona.prompt.contains(contract.body.trim()),
            "{role}: the composed body lost working-contract@{}",
            contract.version
        );
        // The pack's own frontmatter still names the role, unchanged since
        // the personas were thinned.
        assert_eq!(composed.persona.description, before.description, "{role}");
        assert_eq!(composed.persona.display_name, before.display_name, "{role}");
        let refs: Vec<&str> = composed
            .provenance
            .includes
            .iter()
            .map(|i| i.reference.as_str())
            .collect();
        assert_eq!(
            refs,
            vec![
                format!("beekeeper/{role}@^1.0.0"),
                "beekeeper/working-contract@^1.0.0".to_owned()
            ],
            "{role} includes"
        );
        assert!(
            composed.provenance.warnings.is_empty(),
            "{role}: {:?}",
            composed.provenance.warnings
        );

        let dest = staging.path().join(role);
        write_staged_pack(&composed, &dest).unwrap_or_else(|e| panic!("{role}: {e}"));
        let staged = beekeeper_persona::resolve::resolve_persona_by_name(&dest, role)
            .unwrap_or_else(|e| panic!("{role} staged: {e}"));
        assert_eq!(staged.system_prompt, composed.persona.prompt);
        assert_eq!(staged.role.as_deref(), Some(role));
        let expected_skills = template_skill_names(role_template);
        let mut staged_skills = staged.skills.clone();
        staged_skills.sort();
        assert_eq!(staged_skills, expected_skills, "{role} skills changed");
        for skill in &staged_skills {
            assert!(
                dest.join("skills").join(skill).join("SKILL.md").is_file(),
                "{role}: staged skill {skill} has no SKILL.md"
            );
        }
        let report = beekeeper_persona::validate::validate_pack(&dest);
        assert!(!report.has_errors(), "{role}: {:?}", report.diagnostics);
    }
}

#[test]
fn the_seed_of_the_shipped_catalog_composes_every_role_by_reference() {
    let catalog = TemplateCatalog::load(&shipped_templates(), "test").expect("catalog loads");
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("demo-beekeeper-agents");
    let report = write_agents_repo_seed(&root, &catalog, "demo").expect("seed");
    assert_eq!(report.roles, SHIPPED_ROLES);
    assert_eq!(report.lead, "lead");
    for role in SHIPPED_ROLES {
        let composed = compose_role(
            &RoleSource::Flat {
                root: root.clone(),
                role: role.to_owned(),
            },
            &catalog,
            &ComposeOptions::local(format!("roles/{role}")),
        )
        .unwrap_or_else(|e| panic!("seeded {role}: {e}"));
        // The seeded role carries the shipped role's whole current text —
        // the paragraph and the working contract — plus the two extra
        // fragments. The seed writes the newest role version's own
        // description, so that is what it is compared against.
        let role_template = &current(&catalog, role);
        assert!(
            composed.persona.prompt.contains(role_template.body.trim()),
            "seeded {role} lost the shipped text"
        );
        for fragment in SHARED_FRAGMENTS {
            let fragment = current(&catalog, fragment);
            assert!(
                composed.persona.prompt.contains(fragment.body.trim()),
                "seeded {role} lost {}@{}",
                fragment.name,
                fragment.version
            );
        }
        assert_eq!(composed.persona.description, role_template.description);
        let mut skills: Vec<String> = composed.skills.iter().map(|s| s.name.clone()).collect();
        skills.sort();
        let expected = template_skill_names(role_template);
        assert_eq!(skills, expected, "seeded {role} skills");
        assert_eq!(composed.pack_id, "project:demo");
        assert!(
            composed.provenance.warnings.is_empty(),
            "{:?}",
            composed.provenance.warnings
        );
        assert_eq!(
            composed.provenance.includes.len(),
            1 + SHARED_FRAGMENTS.len()
        );
    }
}
