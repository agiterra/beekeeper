//! Product contracts for the neutral role packs this build actually ships.
//!
//! Project procedures belong in project-owned copies. Historical Beekeeper
//! operating rules are intentionally not requirements of this seed corpus.
//!
//! Since 2026-09-18 (spec § 4.11) a shipped pack is thin: its persona is two
//! include lines — the role's own `kind: role` template and the shared
//! working contract — and its skills live under the template. So what a seat
//! actually receives is the *composed* role, and that is what the contracts
//! below read.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use buzz_persona::compose::{compose_role, ComposeOptions, RoleSource};
use buzz_persona::template::TemplateCatalog;
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

fn personas_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under the repository's crates directory")
        .join("personas")
}

fn roles_root() -> PathBuf {
    personas_root().join("roles")
}

fn templates_root() -> PathBuf {
    personas_root().join("templates")
}

fn shipped_catalog() -> TemplateCatalog {
    TemplateCatalog::load(&templates_root(), "test").expect("the shipped catalog loads")
}

/// The role as a seat receives it: the shipped pack composed against the
/// shipped catalog.
fn composed(role: &str) -> buzz_persona::compose::ComposedRole {
    compose_role(
        &RoleSource::Pack {
            dir: roles_root().join(role),
            role: role.to_owned(),
            persona: None,
        },
        &shipped_catalog(),
        &ComposeOptions::local(format!("personas/roles/{role}")),
    )
    .unwrap_or_else(|error| panic!("{role} composes: {error}"))
}

fn children(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .expect("read directory")
        .map(|entry| entry.expect("read directory entry").path())
        .collect()
}

#[test]
fn shipped_roles_are_thin_packs_that_compose_with_complete_skills_and_no_provider_requirement() {
    let root = roles_root();
    let actual: BTreeSet<_> = children(&root)
        .into_iter()
        .filter(|path| path.is_dir())
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(actual, ROLES.iter().map(|role| role.to_string()).collect());

    let catalog = shipped_catalog();
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
        // Thin: the pack itself carries no skills and no prose of its own;
        // both come from the role's template.
        assert!(
            persona.skills.is_empty(),
            "{role} declares pack-local skills; they belong to its template"
        );
        assert!(
            !role_dir.join("skills").exists(),
            "{role} carries a skills directory"
        );
        assert_eq!(
            persona.prompt.trim(),
            format!("![[beekeeper/{role}@^1.0.0]]\n\n![[beekeeper/working-contract@^1.0.0]]"),
            "{role} is not the two include lines"
        );

        // Composed: the template's skills are declared, present and valid.
        let template = catalog
            .versions(role)
            .last()
            .unwrap_or_else(|| panic!("{role} has a shipped template"));
        assert_eq!(template.kind, buzz_persona::template::TemplateKind::Role);
        assert!(!template.skills.is_empty(), "{role} declares no procedure");
        let composed = composed(role);
        let mut declared = BTreeSet::new();
        for skill in &template.skills {
            let relative = skill
                .strip_prefix("./skills/")
                .expect("shipped skill is template-local")
                .trim_end_matches('/');
            assert!(!relative.contains('/'), "unexpected nested skill {skill}");
            assert!(declared.insert(relative.to_owned()), "duplicate skill");
            let meta = skill_meta::read_skill_meta(&template.dir.join("skills").join(relative))
                .expect("declared skill has valid metadata and readable content");
            assert_eq!(meta.name, relative);
            assert!(!meta.description.is_empty(), "skill has no description");
        }
        let present: BTreeSet<_> = children(&template.dir.join("skills"))
            .into_iter()
            .filter(|path| path.is_dir())
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(declared, present, "{role} has an undeclared shared skill");
        let staged: BTreeSet<_> = composed.skills.iter().map(|s| s.name.clone()).collect();
        assert_eq!(staged, declared, "{role} composes with different skills");
    }
}

// Scan the complete shipped tree — packs and templates, manifests and
// unreferenced files: the desktop's seeder copies all of it, not only loaded
// persona instructions.
fn shipped_files() -> Vec<PathBuf> {
    let mut files = files_under(&roles_root());
    files.extend(files_under(&templates_root()));
    files
}

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
    for path in shipped_files() {
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
    let body = std::fs::read_to_string(
        templates_root().join("project-setup/1.0.0/skills/setup-project/SKILL.md"),
    )
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

/// Reads a shipped file under `personas/` with line wrapping collapsed, so
/// contracts match wording rather than where a paragraph happens to wrap.
fn shipped_text(relative: &str) -> String {
    let text = std::fs::read_to_string(personas_root().join(relative)).expect("shipped file");
    collapsed(&text)
}

fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn setup_reconciles_existing_instructions_instead_of_copying_them() {
    let body = shipped_text("templates/project-setup/1.0.0/skills/setup-project/SKILL.md");
    for contract in [
        "## Reconcile existing instructions",
        "project policy to reconcile, not text to copy or discard",
        "lead-coordinated team whose lead picks workers by task",
        "Ordinary solo sessions remain a supported way to work",
        "Preserve every product, security, testing and independent review requirement",
        "names a specific agent, reviewer, tool, budget, model or staffing arrangement",
        "record the underlying requirement",
        "list the name or assumption for the owner to reconcile instead of making it a role dependency",
        "never removes the independent review it stood for",
        "generated from a source file by a generator, never edit the generated output",
        "name the source and generator in the report and propose changes there",
        "Never silently rewrite project policy files or drop an independent review requirement",
        "Inspect tools, commands, configuration and repository state yourself",
        "Ask the owner only for what inspection cannot establish",
    ] {
        assert!(body.contains(contract), "setup omits {contract:?}");
    }
}

#[test]
fn every_role_treats_named_staffing_as_requirements_to_confirm() {
    for role in ROLES {
        let persona = format!("{role}, composed");
        let body = collapsed(&composed(role).persona.prompt);
        for contract in [
            "authoritative for product, security, testing and review requirements",
            "Named historical agents, reviewers, budgets, models and staffing assumptions",
            "requirements to confirm, not current staffing",
            "keep the underlying requirement and flag the name for reconciliation",
        ] {
            assert!(body.contains(contract), "{persona} omits {contract:?}");
        }
    }
}

#[test]
fn lead_meets_required_independent_review_without_mandating_a_second_worker() {
    let body = collapsed(&composed("lead").persona.prompt);
    assert!(body.contains(
        "Do not require a second worker for routine work unless project policy requires \
         independent review; then meet that requirement with an available role."
    ));
}

#[test]
fn lead_confirms_figures_and_inspects_before_asking() {
    let choose = shipped_text("templates/lead/1.0.0/skills/choose-model/SKILL.md");
    for contract in [
        "Budget and model figures",
        "confirm them against current project grants and routing before repeating or relying on them",
    ] {
        assert!(choose.contains(contract), "choose-model omits {contract:?}");
    }
    let ruling = shipped_text("templates/lead/1.0.0/skills/ask-for-a-ruling/SKILL.md");
    for contract in [
        "Inspect tools, configuration, grants and repository state before asking a person",
        "ask only for what inspection cannot establish",
        "keep the underlying requirement",
        "flag the name for the owner to reconcile rather than waiting on it",
    ] {
        assert!(
            ruling.contains(contract),
            "ask-for-a-ruling omits {contract:?}"
        );
    }
}

/// The lead discovers its project's agents before hiring, and knows the host
/// never seats another project's agent in their place.
#[test]
fn lead_discovers_project_agents_and_hires_only_within_the_project() {
    let hire = shipped_text("templates/lead/1.0.0/skills/hire/SKILL.md");
    for contract in [
        "`bee projects agents`",
        "A private project does not publish its agents",
        "use the agent list in the session's first message",
        "hire by role with `bee sessions hire`",
        "The host seats only agents that belong to the session's project",
        "never another project's agent",
        "A refused hire names its remedy",
    ] {
        assert!(hire.contains(contract), "hire skill omits {contract:?}");
    }
}

// Word-level scan: role text stays generic, so no agent, person, project or
// model name may appear. A model name would pin work to one provider, and a
// fixed reviewer count would make a team mandatory.
#[test]
fn shipped_seed_names_no_agent_person_project_or_model() {
    let forbidden_words = [
        "astra", "fable", "loom", "amas", "tank", "opus", "sonnet", "haiku", "claude", "codex",
        "gpt", "gemini",
    ];
    let forbidden_phrases = ["two reviewers", "two independent reviewers", "at least two"];
    for path in shipped_files() {
        let text = std::fs::read_to_string(&path)
            .expect("shipped content is text")
            .to_lowercase();
        for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
            assert!(
                !forbidden_words.contains(&word),
                "{} names {word:?}",
                path.display()
            );
        }
        for phrase in forbidden_phrases {
            assert!(
                !text.contains(phrase),
                "{} fixes a reviewer count {phrase:?}",
                path.display()
            );
        }
    }
}
