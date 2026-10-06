//! The seed a project's agents repository starts with (spec § 4.11).
//!
//! Every project gets `<slug>-beekeeper-agents`, created with the project
//! and pinned as its kind:30624 source at `path: "."`. This module writes
//! what that repository holds on its first commit:
//!
//! ```text
//! README.md              the layout and the archive rule, for people and agents
//! team.yml               beekeeper-team/v1: every seeded role, `lead: lead`
//! actions.yml            buzz-project-actions/v1, one active manual `verify`
//! model-registry.yaml    the routing registry, so hires route per project
//! roles/<role>.md        one per shipped `kind: role` template — an include, not a copy
//! roles/archive/         retired roles; never hireable, never included
//! skills/                shared by every role; empty
//! plans/                 plans in force
//! plans/archive/         retired plans
//! ```
//!
//! **Seed by reference.** A seeded role file is four include lines: the
//! shipped role's own template and the three shared fragments, each pinned
//! with a caret range on the version this build ships. So the project takes
//! Beekeeper's minor revisions at its next hire and opts into a major one by
//! editing a line; the role's bytes never silently change under a running
//! seat (spec § 4.5). Nothing is copied: a project that wants to own a
//! paragraph clones it with `bee pack clone-template`.
//!
//! The writer refuses a root that already holds anything but `.git`: a seed
//! is a first commit, never a merge over someone's work.

use std::path::{Path, PathBuf};

use crate::compose::{FLAT_ROLES_DIR, FLAT_SKILLS_DIR};
use crate::team::{ARCHIVE_DIR, TEAM_SCHEMA, TEAM_YML};
use crate::template::{Template, TemplateCatalog, TemplateKind};

/// The directory holding plans in force.
pub const PLANS_DIR: &str = "plans";

/// The actions file at the root (`buzz-workflow` reads it; this crate only
/// seeds it, so the schema string is repeated here and pinned by a test in
/// `buzz-workflow`).
pub const ACTIONS_YML: &str = "actions.yml";

/// The schema `actions.yml` must name — `beekeeper_workflow::actions_file::ACTIONS_SCHEMA`.
pub const ACTIONS_SCHEMA: &str = "buzz-project-actions/v1";

/// The command the seeded `verify` action runs when the person creating the
/// project names none: the standard-library test runner over `tests/`, the
/// Kettle projects' gate. It is project-specific, so project creation asks
/// for it (defaulting to this) rather than fixing it here.
pub const DEFAULT_VERIFY_COMMAND: [&str; 6] =
    ["python3", "-m", "unittest", "discover", "-s", "tests"];

/// [`DEFAULT_VERIFY_COMMAND`] as owned strings.
pub fn default_verify_command() -> Vec<String> {
    DEFAULT_VERIFY_COMMAND.map(str::to_owned).to_vec()
}

/// The `actions.yml` a new agents repository starts with: the schema line and
/// one **active** manual `verify` action — one `run_on_host` step with
/// `checkout: required` — running [`DEFAULT_VERIFY_COMMAND`].
pub fn seeded_actions_yml() -> String {
    seeded_actions_yml_with_verify(&default_verify_command())
}

/// [`seeded_actions_yml`] with the verify step running `command` (argv, no
/// shell).
///
/// The action is live from the first commit (ledger 248, superseding the
/// commented example of 206 A): project setup publishes it and collects the
/// one hash-bound "allow future runs of this exact definition" grant from the
/// person whose computer runs it, so no seat has to author, publish and wait
/// on approval for the gate every run needs. Editing it changes its
/// definition hash, and a changed definition asks for that consent again.
///
/// Each argument is written as a double-quoted YAML scalar, so no argument
/// can be read as a boolean, a number or a second key. `buzz-cli` holds the
/// test that runs the real parser over this text.
pub fn seeded_actions_yml_with_verify(command: &[String]) -> String {
    let argv = command
        .iter()
        .map(|arg| yaml_string(arg))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "\
# This project's actions (spec § 5). Each entry is a workflow definition;
# `bee actions publish` reads this file from the agents repository root, and
# `bee actions example` prints a complete, valid file with every trigger kind.
#
# `verify` was published when the project was set up, and whoever's computer
# runs it agreed there to future runs of this exact definition. Editing it
# changes its definition hash, so the edited action asks for that consent
# again before it runs anywhere; running it unchanged asks nobody.
#
# `checkout: required` makes the relay refuse a run that names no commit, so
# it can only be started as
#   bee workflows trigger --workflow <id> --checkout <40-hex sha>
# and the host runs it in a fresh detached worktree at that commit, never in
# the project folder as it happens to be checked out.
schema: {ACTIONS_SCHEMA}
actions:
  - name: verify
    description: Run the project's tests against one named commit.
    trigger:
      on: manual
    steps:
      - id: verify
        action: run_on_host
        command: [{argv}]
        working_directory: \".\"
        checkout: required
        timeout: 30m
"
    )
}

/// The README the seed writes.
pub const README_MD: &str = "README.md";

/// The routing registry at the agents repository's root.
///
/// Named to match `beekeeper_core::model_registry_source::AGENTS_REPO_REGISTRY_FILE`
/// — the name every *reader* composes — and repeated here because this crate
/// does not depend on `buzz-core`. A test in the desktop host, which depends
/// on both, asserts the two agree; a disagreement would be invisible, seeding
/// a file no router would ever look at.
pub const MODEL_REGISTRY_YML: &str = "model-registry.yaml";

/// The registry the seed writes, embedded from this repository's
/// `team/model-registry.yaml` at build time.
///
/// Embedded rather than copied at runtime for one reason: a project's
/// registry must not depend on a Beekeeper checkout being present on the
/// machine that creates the project. `include_str!` also makes drift
/// impossible — the bytes are the file's, taken when this build was compiled
/// — and the test `the_seeded_registry_is_this_repositorys_own_file` pins
/// that the build's copy still matches the working tree.
///
/// A seeded registry is a **copy, not a reference**, unlike a seeded role.
/// Routing is the project's own policy: once seeded, the project edits these
/// rows and Beekeeper's later opinions do not reach into a running team's
/// cost decisions.
pub const SEEDED_MODEL_REGISTRY: &str = include_str!("../../../team/model-registry.yaml");

/// The fragments every seeded role includes after its own template, in
/// this order. Each must exist in the catalog or the seed refuses.
pub const SHARED_FRAGMENTS: [&str; 3] = ["working-contract", "memory", "project-pulse"];

/// The role hired first when the catalog ships it (D14).
pub const DEFAULT_LEAD: &str = "lead";

/// The `version` the seeded `team.yml` starts at.
pub const SEED_TEAM_VERSION: &str = "0.1.0";

#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("{root} is not empty ({entry} is there); a seed is a first commit, never a merge")]
    NotEmpty { root: PathBuf, entry: String },

    #[error(
        "this build's template catalog ships no `kind: role` template, so there is no role to seed"
    )]
    NoRoleTemplates,

    #[error("this build's template catalog has no current {name:?} template; every seeded role includes it")]
    MissingFragment { name: String },

    #[error("failed to {operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// What the seed wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedReport {
    /// The roles seeded, ascending: one per `kind: role` template.
    pub roles: Vec<String>,
    /// The lead named in `team.yml`.
    pub lead: String,
    /// Every file written, relative to the root, in write order.
    pub files: Vec<String>,
}

/// Write the seed into `root`, which must be empty (a `.git` directory is
/// allowed). `name` becomes `team.yml`'s `name` — the project slug.
///
/// # Errors
/// [`SeedError::NotEmpty`], [`SeedError::NoRoleTemplates`],
/// [`SeedError::MissingFragment`] before anything is written; an I/O error
/// names the file.
pub fn write_agents_repo_seed(
    root: &Path,
    catalog: &TemplateCatalog,
    name: &str,
) -> Result<SeedReport, SeedError> {
    write_agents_repo_seed_with_verify(root, catalog, name, &default_verify_command())
}

/// [`write_agents_repo_seed`] with the seeded `verify` action running
/// `verify_command` (see [`seeded_actions_yml_with_verify`]).
///
/// # Errors
/// As [`write_agents_repo_seed`].
pub fn write_agents_repo_seed_with_verify(
    root: &Path,
    catalog: &TemplateCatalog,
    name: &str,
    verify_command: &[String],
) -> Result<SeedReport, SeedError> {
    refuse_non_empty(root)?;
    let roles = catalog.role_templates();
    if roles.is_empty() {
        return Err(SeedError::NoRoleTemplates);
    }
    let mut fragments: Vec<&Template> = Vec::with_capacity(SHARED_FRAGMENTS.len());
    for fragment in SHARED_FRAGMENTS {
        let current = catalog
            .versions(fragment)
            .iter()
            .rev()
            .find(|t| t.deprecated.is_none())
            .ok_or_else(|| SeedError::MissingFragment {
                name: fragment.to_owned(),
            })?;
        fragments.push(current);
    }
    let lead = if roles.iter().any(|t| t.name == DEFAULT_LEAD) {
        DEFAULT_LEAD.to_owned()
    } else {
        roles[0].name.clone()
    };

    let mut files = Vec::new();
    let mut write = |rel: &str, bytes: &str| -> Result<(), SeedError> {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| SeedError::Io {
                operation: "create",
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&path, bytes).map_err(|source| SeedError::Io {
            operation: "write",
            path: path.clone(),
            source,
        })?;
        files.push(rel.to_owned());
        Ok(())
    };

    write(README_MD, &readme(name))?;
    write(TEAM_YML, &team_yml(name, &lead, &roles))?;
    write(ACTIONS_YML, &seeded_actions_yml_with_verify(verify_command))?;
    // The registry travels with the project (ledger 178(a)): without it here
    // a routed hire is refused on every project but Beekeeper's own, and the
    // unrouted retry runs the identity's pin — the most expensive target.
    write(MODEL_REGISTRY_YML, SEEDED_MODEL_REGISTRY)?;
    for template in &roles {
        write(
            &format!("{FLAT_ROLES_DIR}/{}.md", template.name),
            &role_md(template, &fragments),
        )?;
    }
    for dir in [
        format!("{FLAT_ROLES_DIR}/{ARCHIVE_DIR}"),
        FLAT_SKILLS_DIR.to_owned(),
        PLANS_DIR.to_owned(),
        format!("{PLANS_DIR}/{ARCHIVE_DIR}"),
    ] {
        write(&format!("{dir}/.gitkeep"), "")?;
    }

    Ok(SeedReport {
        roles: roles.iter().map(|t| t.name.clone()).collect(),
        lead,
        files,
    })
}

/// The seeded `roles/<role>.md`: frontmatter carrying the template's
/// description, then the includes.
pub fn role_md(template: &Template, fragments: &[&Template]) -> String {
    debug_assert_eq!(template.kind, TemplateKind::Role);
    let mut out = format!(
        "---\ndescription: {}\n---\n\n![[beekeeper/{}@^{}]]\n",
        yaml_string(&template.description),
        template.name,
        template.version
    );
    for fragment in fragments {
        out.push_str(&format!(
            "\n![[beekeeper/{}@^{}]]\n",
            fragment.name, fragment.version
        ));
    }
    out
}

fn team_yml(name: &str, lead: &str, roles: &[&Template]) -> String {
    let mut out = format!(
        "# The project's team (spec § 4.2). Roles are files under roles/; a role\n\
         # listed here gets advisory runtime/model hints and an agents-repository\n\
         # grant (workspace.agents_repo: none | read | write). Retired roles live\n\
         # under roles/archive/ and may not be named here.\n\
         schema: {TEAM_SCHEMA}\nname: {}\nversion: {SEED_TEAM_VERSION}\nlead: {lead}\nroles:\n",
        yaml_string(name)
    );
    // The lead drafts, commits and adopts the project's plans, so its grant
    // is written out (spec § 4.11); other roles take the default (no clone).
    for template in roles {
        if template.name == lead {
            out.push_str(&format!(
                "  {}: {{ workspace: {{ agents_repo: write }} }}\n",
                template.name
            ));
        } else {
            out.push_str(&format!("  {}: {{}}\n", template.name));
        }
    }
    // One default agent per role (spec § 4.11): the host mints the
    // identities when the project is created; these are the names it uses
    // and the names `actions.yml` may route to. The lead is persistent;
    // every other role is hired per task.
    out.push_str("agents:\n");
    for template in roles {
        let lifetime = if template.name == DEFAULT_LEAD {
            "persistent"
        } else {
            "ephemeral"
        };
        out.push_str(&format!(
            "  - {{ name: {}, role: {}, lifetime: {lifetime} }}\n",
            yaml_string(&agent_display_name(&template.name)),
            template.name
        ));
    }
    out
}

/// The default agent name for a role: the slug in title case
/// (`project-setup` → `Project Setup`), which is what the shipped packs call
/// their personas.
pub fn agent_display_name(role: &str) -> String {
    role.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn readme(name: &str) -> String {
    format!(
        "# {name} — agents repository\n\n\
         This repository holds the project's agent team and plans; the code lives\n\
         in the project's own repository. Beekeeper reads it as the project's role\n\
         source (kind:30624, `path: .`, pinned to `main`).\n\n\
         | path | what it is |\n\
         | --- | --- |\n\
         | `team.yml` | the team manifest: roles, the lead, per-role grants |\n\
         | `actions.yml` | the project's actions (`bee actions publish`) |\n\
         | `model-registry.yaml` | the execution targets hires route against (`bee sessions route`) |\n\
         | `roles/<role>.md` | a role **in force**; an include of Beekeeper's shipped role plus the shared fragments |\n\
         | `roles/<role>/skills/` | skills private to that role |\n\
         | `roles/archive/` | **retired** roles, kept for history; never hireable, never included |\n\
         | `skills/` | skills every role receives |\n\
         | `plans/<plan>.md` | a plan **in force** |\n\
         | `plans/archive/` | **retired** plans, kept for history |\n\n\
         The archive rule: what is under an `archive/` directory is not in force.\n\
         Read it only when a current document points you there. To retire a role\n\
         or plan, move the file into the sibling `archive/`; to revive it, move it\n\
         back.\n\n\
         A seeded role is a reference, not a copy: `![[beekeeper/<role>@^1.0.0]]`\n\
         resolves against the templates the Beekeeper build ships, so the project\n\
         takes minor revisions at its next hire and opts into a major one by\n\
         editing the line. To own a paragraph outright, clone it:\n\
         `bee pack clone-template <name>@<version> --into .` and include the copy\n\
         with `![[./templates/<name>.md]]`.\n\n\
         `model-registry.yaml` is the opposite: a copy this project owns. Routing\n\
         is the team's own cost policy, so edit these rows here — nothing in\n\
         Beekeeper reaches into them again. `bee sessions registry check` says\n\
         whether they are still true about what this host is serving.\n"
    )
}

/// Quote a string for YAML the way `serde_yaml` would for anything that is
/// not a plain scalar: double quotes with `"` and `\\` escaped.
pub(crate) fn yaml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

pub(crate) fn refuse_non_empty(root: &Path) -> Result<(), SeedError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(SeedError::Io {
                operation: "read",
                path: root.to_path_buf(),
                source,
            })
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != ".git" {
            return Err(SeedError::NotEmpty {
                root: root.to_path_buf(),
                entry: name,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::{compose_role, ComposeOptions, RoleSource};
    use crate::team::{load_team, AgentsRepoAccess};
    use crate::template::TEMPLATE_MD;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn catalog(dir: &Path) -> TemplateCatalog {
        write(
            &dir.join("lead/1.0.0").join(TEMPLATE_MD),
            "---\nname: lead\nversion: 1.0.0\ndescription: \"Leads, \\\"quoted\\\".\"\nkind: role\nskills:\n  - ./skills/hire/\n---\nLead the work.\n",
        );
        write(
            &dir.join("lead/1.0.0/skills/hire/SKILL.md"),
            "---\nname: hire\ndescription: h\n---\nhire\n",
        );
        write(
            &dir.join("builder/1.2.0").join(TEMPLATE_MD),
            "---\nname: builder\nversion: 1.2.0\ndescription: Builds.\nkind: role\n---\nBuild the thing.\n",
        );
        for fragment in SHARED_FRAGMENTS {
            write(
                &dir.join(fragment).join("1.0.0").join(TEMPLATE_MD),
                &format!("---\nname: {fragment}\nversion: 1.0.0\ndescription: {fragment}\n---\n## {fragment}\n\nText.\n"),
            );
        }
        TemplateCatalog::load(dir, "0.6.0").unwrap()
    }

    #[test]
    fn the_seed_writes_the_layout_and_every_role_composes_by_reference() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("demo-beekeeper-agents");
        let report = write_agents_repo_seed(&root, &catalog, "demo").unwrap();
        assert_eq!(report.roles, vec!["builder", "lead"]);
        assert_eq!(report.lead, "lead");
        for rel in [
            "README.md",
            "team.yml",
            "actions.yml",
            "model-registry.yaml",
            "roles/lead.md",
            "roles/builder.md",
            "roles/archive/.gitkeep",
            "skills/.gitkeep",
            "plans/.gitkeep",
            "plans/archive/.gitkeep",
        ] {
            assert!(root.join(rel).is_file(), "{rel} missing");
            assert!(report.files.contains(&rel.to_owned()), "{rel} unreported");
        }

        let lead = std::fs::read_to_string(root.join("roles/lead.md")).unwrap();
        assert_eq!(
            lead,
            "---\ndescription: \"Leads, \\\"quoted\\\".\"\n---\n\n![[beekeeper/lead@^1.0.0]]\n\n![[beekeeper/working-contract@^1.0.0]]\n\n![[beekeeper/memory@^1.0.0]]\n\n![[beekeeper/project-pulse@^1.0.0]]\n"
        );
        let builder = std::fs::read_to_string(root.join("roles/builder.md")).unwrap();
        assert!(builder.contains("![[beekeeper/builder@^1.2.0]]"));

        let team = load_team(&root).unwrap().expect("team.yml");
        assert_eq!(team.name.as_deref(), Some("demo"));
        assert_eq!(team.lead.as_deref(), Some("lead"));
        assert_eq!(team.version, SEED_TEAM_VERSION);
        assert_eq!(
            team.roles.keys().cloned().collect::<Vec<_>>(),
            vec!["builder", "lead"]
        );
        // Run11 (ledger 272, defect 3): a seeded lead got no agents clone and
        // improvised one. The seeded lead writes its plans.
        assert_eq!(
            team.role("lead").workspace.agents_repo,
            Some(AgentsRepoAccess::Write)
        );
        assert_eq!(team.role("builder").workspace.agents_repo, None);
        assert_eq!(team.agents_access("builder"), AgentsRepoAccess::None);
        let agents: Vec<(&str, &str, bool)> = team
            .agents
            .iter()
            .map(|a| {
                (
                    a.name.as_str(),
                    a.role.as_str(),
                    a.lifetime == crate::team::AgentLifetime::Persistent,
                )
            })
            .collect();
        assert_eq!(
            agents,
            vec![("Builder", "builder", false), ("Lead", "lead", true)]
        );
        assert_eq!(agent_display_name("project-setup"), "Project Setup");

        let composed = compose_role(
            &RoleSource::Flat {
                root: root.clone(),
                role: "lead".to_owned(),
            },
            &catalog,
            &ComposeOptions::local("roles/lead"),
        )
        .unwrap();
        assert_eq!(
            composed.persona.prompt,
            "\nLead the work.\n\n## working-contract\n\nText.\n\n## memory\n\nText.\n\n## project-pulse\n\nText.\n"
        );
        assert_eq!(composed.persona.description, "Leads, \"quoted\".");
        let skills: Vec<&str> = composed.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(skills, vec!["hire"]);
        assert_eq!(composed.pack_id, "project:demo");
        assert!(composed.provenance.warnings.is_empty());

        let actions = std::fs::read_to_string(root.join("actions.yml")).unwrap();
        assert!(actions.contains(&format!("schema: {ACTIONS_SCHEMA}")));
        assert!(actions.contains("        checkout: required"));
        assert_eq!(actions, seeded_actions_yml());
    }

    /// Ledger 178(a): the registry a project routes against must be this
    /// repository's own file, byte for byte, and it must actually be written.
    ///
    /// `include_str!` makes the *build* honest; this makes the working tree
    /// honest, which is the half a reader can check. A registry that drifted
    /// from the file the team reviews would produce decisions citing a
    /// version that was never on disk.
    #[test]
    fn the_seeded_registry_is_this_repositorys_own_file() {
        let repo_registry = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("team")
            .join("model-registry.yaml");
        let on_disk =
            std::fs::read_to_string(&repo_registry).expect("read team/model-registry.yaml");
        assert_eq!(
            SEEDED_MODEL_REGISTRY,
            on_disk,
            "the embedded registry and {} have drifted",
            repo_registry.display()
        );

        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("demo-beekeeper-agents");
        write_agents_repo_seed(&root, &catalog, "demo").unwrap();
        let seeded = std::fs::read_to_string(root.join(MODEL_REGISTRY_YML)).expect("seeded");
        assert_eq!(seeded, on_disk);
        // And it parses as a registry to the reader that matters: `version`
        // and at least one target row, the two things routing cannot do
        // without.
        assert!(seeded.contains("version: 1"), "no version");
        assert!(seeded.contains("targets:"), "no targets");
    }

    #[test]
    fn the_seed_refuses_a_root_that_is_not_empty_and_a_catalog_missing_what_it_needs() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("taken");
        write(&root.join("notes.md"), "mine");
        let error = write_agents_repo_seed(&root, &catalog, "demo").unwrap_err();
        assert!(error.to_string().contains("notes.md"), "{error}");
        assert!(!root.join("team.yml").exists(), "nothing written");

        // `.git` alone does not count.
        let fresh = tmp.path().join("fresh");
        std::fs::create_dir_all(fresh.join(".git")).unwrap();
        write_agents_repo_seed(&fresh, &catalog, "demo").unwrap();

        let no_roles = tmp.path().join("no-roles");
        for fragment in SHARED_FRAGMENTS {
            write(
                &no_roles.join(fragment).join("1.0.0").join(TEMPLATE_MD),
                &format!("---\nname: {fragment}\nversion: 1.0.0\ndescription: f\n---\nf\n"),
            );
        }
        let catalog = TemplateCatalog::load(&no_roles, "0.6.0").unwrap();
        assert!(matches!(
            write_agents_repo_seed(&tmp.path().join("a"), &catalog, "demo"),
            Err(SeedError::NoRoleTemplates)
        ));

        let no_memory = tmp.path().join("no-memory");
        write(
            &no_memory.join("lead/1.0.0").join(TEMPLATE_MD),
            "---\nname: lead\nversion: 1.0.0\ndescription: l\nkind: role\n---\nl\n",
        );
        let catalog = TemplateCatalog::load(&no_memory, "0.6.0").unwrap();
        let error = write_agents_repo_seed(&tmp.path().join("b"), &catalog, "demo").unwrap_err();
        assert!(error.to_string().contains("working-contract"), "{error}");
    }
}
