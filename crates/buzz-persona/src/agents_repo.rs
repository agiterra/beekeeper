//! Validating a whole agents-repository tree before it is committed.
//!
//! A committer (the desktop's Commit button, `bee agents-repo commit`)
//! builds the tree `main` would become, materializes it into a directory,
//! and asks this module whether every seat could still be staged from it:
//! the manifest parses, every role it names exists and is not archived,
//! every live role composes against the shipped templates, every skill's
//! `SKILL.md` reads, and `actions.yml` — which this crate cannot parse, so
//! the caller hands in the parser — parses. Nothing is pushed until this
//! answers, and the answer names every path that refuses, not the first.
//!
//! What it does not do: judge prose. A plan is markdown with no parser
//! (spec § 4.11), and this module never reads `plans/`.

use std::path::Path;

use crate::compose::{archived_role_files, compose_role, ComposeOptions, RoleSource};
use crate::skill_meta::read_skill_meta;
use crate::team::{is_archived_path, load_team, ARCHIVE_DIR, TEAM_YML};
use crate::template::TemplateCatalog;

/// The actions manifest's file name at the agents repository root.
pub const ACTIONS_YML: &str = "actions.yml";

/// Whether `actions.yml` was checked, and how many actions it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionsCheck {
    /// The file is absent; a project with no actions is legal.
    Absent,
    /// The caller's parser accepted it with this many actions.
    Checked(usize),
    /// The file exists but the caller supplied no parser; the reason says
    /// which caller, so nobody reads "validated" where it was not.
    NotChecked(String),
}

/// What a clean tree holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeReport {
    /// Live roles that composed, sorted.
    pub roles: Vec<String>,
    /// Retired roles under `roles/archive/`, sorted.
    pub archived: Vec<String>,
    /// Skill directories whose `SKILL.md` read, relative to the root, sorted.
    pub skills: Vec<String>,
    /// The actions manifest's state.
    pub actions: ActionsCheck,
    /// Composer warnings, each prefixed by the role.
    pub warnings: Vec<String>,
}

/// One path that refuses, with the composer's or parser's own sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRefusal {
    /// Relative to the root.
    pub path: String,
    /// Why.
    pub reason: String,
}

/// A parser for `actions.yml` text: `Ok(count)` or the parser's sentence.
pub type ActionsParser<'a> = &'a mut dyn FnMut(&str) -> Result<usize, String>;

/// Validate the tree at `root`. Every refusal is collected; an empty
/// refusal list is the only clean answer.
pub fn validate_root(
    root: &Path,
    catalog: &TemplateCatalog,
    actions: Option<ActionsParser<'_>>,
) -> Result<TreeReport, Vec<TreeRefusal>> {
    let mut refusals = Vec::new();
    let mut warnings = Vec::new();

    let team = match load_team(root) {
        Ok(team) => team,
        Err(error) => {
            refusals.push(TreeRefusal {
                path: TEAM_YML.to_owned(),
                reason: error.to_string(),
            });
            None
        }
    };

    let mut roles = live_role_slugs(root);
    let archived = archived_role_files(root);
    if let Some(team) = &team {
        for (slug, entry) in &team.roles {
            let file = entry
                .file
                .clone()
                .unwrap_or_else(|| format!("roles/{slug}.md"));
            if is_archived_path(&file) {
                refusals.push(TreeRefusal {
                    path: TEAM_YML.to_owned(),
                    reason: format!(
                        "role {slug:?} points at {file}, which is under roles/{ARCHIVE_DIR}/ and never in force"
                    ),
                });
            } else if !root.join(&file).is_file() {
                refusals.push(TreeRefusal {
                    path: TEAM_YML.to_owned(),
                    reason: format!("role {slug:?} names {file}, which does not exist"),
                });
            } else if !roles.contains(slug) {
                roles.push(slug.clone());
            }
        }
        for agent in &team.agents {
            if !team.roles.contains_key(&agent.role) && !roles.contains(&agent.role) {
                refusals.push(TreeRefusal {
                    path: TEAM_YML.to_owned(),
                    reason: format!(
                        "agent {:?} has role {:?}, which the manifest does not define",
                        agent.name, agent.role
                    ),
                });
            }
        }
    }
    roles.sort();
    roles.dedup();

    let mut composed = Vec::new();
    // A broken manifest already refused every composition; composing again
    // would repeat the same sentence once per role.
    if team.is_some() || !root.join(TEAM_YML).exists() {
        for role in &roles {
            let source = RoleSource::Flat {
                root: root.to_path_buf(),
                role: role.clone(),
            };
            match compose_role(
                &source,
                catalog,
                &ComposeOptions::local(root.display().to_string()),
            ) {
                Ok(result) => {
                    warnings.extend(
                        result
                            .provenance
                            .warnings
                            .iter()
                            .map(|w| format!("{role}: {w}")),
                    );
                    composed.push(role.clone());
                }
                Err(error) => refusals.push(TreeRefusal {
                    path: format!("roles/{role}.md"),
                    reason: error.to_string(),
                }),
            }
        }
    }

    let mut skills = Vec::new();
    for dir in skill_dirs_under(&root.join("skills")) {
        check_skill(root, &dir, &mut skills, &mut refusals);
    }
    for role in &roles {
        for dir in skill_dirs_under(&root.join("roles").join(role).join("skills")) {
            check_skill(root, &dir, &mut skills, &mut refusals);
        }
    }
    skills.sort();

    let actions_path = root.join(ACTIONS_YML);
    let actions_state = if !actions_path.is_file() {
        ActionsCheck::Absent
    } else {
        match actions {
            None => ActionsCheck::NotChecked(
                "this committer has no actions parser; `bee agents-repo commit` checks it"
                    .to_owned(),
            ),
            Some(parser) => match std::fs::read_to_string(&actions_path) {
                Err(error) => {
                    refusals.push(TreeRefusal {
                        path: ACTIONS_YML.to_owned(),
                        reason: format!("could not read: {error}"),
                    });
                    ActionsCheck::NotChecked("unreadable".to_owned())
                }
                Ok(text) => match parser(&text) {
                    Ok(count) => ActionsCheck::Checked(count),
                    Err(reason) => {
                        refusals.push(TreeRefusal {
                            path: ACTIONS_YML.to_owned(),
                            reason,
                        });
                        ActionsCheck::NotChecked("refused".to_owned())
                    }
                },
            },
        }
    };

    if refusals.is_empty() {
        Ok(TreeReport {
            roles: composed,
            archived,
            skills,
            actions: actions_state,
            warnings,
        })
    } else {
        refusals.sort_by(|a, b| a.path.cmp(&b.path).then(a.reason.cmp(&b.reason)));
        Err(refusals)
    }
}

/// `roles/<slug>.md` files directly under `roles/`, by stem.
fn live_role_slugs(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("roles")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()?
                .strip_suffix(".md")
                .map(str::to_owned)
        })
        .collect()
}

fn skill_dirs_under(dir: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .collect();
    dirs.sort();
    dirs
}

fn check_skill(root: &Path, dir: &Path, skills: &mut Vec<String>, refusals: &mut Vec<TreeRefusal>) {
    let rel = dir
        .strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| dir.display().to_string());
    match read_skill_meta(dir) {
        Ok(_) => skills.push(rel),
        Err(error) => refusals.push(TreeRefusal {
            path: format!("{rel}/SKILL.md"),
            reason: error.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::write_agents_repo_seed;

    fn shipped_templates() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../personas/templates")
    }

    fn seeded() -> (tempfile::TempDir, TemplateCatalog) {
        let catalog = TemplateCatalog::load(&shipped_templates(), "test").expect("catalog");
        let dir = tempfile::tempdir().expect("tempdir");
        write_agents_repo_seed(dir.path(), &catalog, "tank-loop").expect("seed");
        (dir, catalog)
    }

    #[test]
    fn the_seeded_tree_validates_clean() {
        let (dir, catalog) = seeded();
        let mut parser = |text: &str| -> Result<usize, String> {
            if text.contains("buzz-project-actions/v1") {
                Ok(0)
            } else {
                Err("no schema".into())
            }
        };
        let report = validate_root(dir.path(), &catalog, Some(&mut parser)).expect("clean");
        assert!(report.roles.contains(&"lead".to_owned()));
        assert_eq!(report.roles.len(), 8);
        assert!(report.archived.is_empty());
        assert_eq!(report.actions, ActionsCheck::Checked(0));
        let report = validate_root(dir.path(), &catalog, None).expect("clean");
        assert!(matches!(report.actions, ActionsCheck::NotChecked(_)));
    }

    #[test]
    fn a_broken_include_refuses_naming_the_role_file() {
        let (dir, catalog) = seeded();
        std::fs::write(
            dir.path().join("roles/lead.md"),
            "---\ndescription: lead\n---\n![[beekeeper/no-such-template@^1.0.0]]\n",
        )
        .expect("write");
        let refusals = validate_root(dir.path(), &catalog, None).expect_err("refuses");
        assert_eq!(refusals.len(), 1);
        assert_eq!(refusals[0].path, "roles/lead.md");
    }

    #[test]
    fn an_archived_role_named_by_the_manifest_refuses_at_team_yml() {
        let (dir, catalog) = seeded();
        std::fs::rename(
            dir.path().join("roles/poker.md"),
            dir.path().join("roles/archive/poker.md"),
        )
        .expect("archive");
        let refusals = validate_root(dir.path(), &catalog, None).expect_err("refuses");
        assert!(
            refusals
                .iter()
                .any(|r| r.path == "team.yml" && r.reason.contains("poker")),
            "{refusals:?}"
        );

        // Dropping it from the manifest makes the tree clean and lists it archived.
        let team = std::fs::read_to_string(dir.path().join("team.yml")).expect("team");
        let team: serde_yaml::Value = serde_yaml::from_str(&team).expect("yaml");
        let mut team = team;
        team["roles"]
            .as_mapping_mut()
            .expect("roles")
            .remove("poker");
        let agents = team["agents"].as_sequence_mut().expect("agents");
        agents.retain(|a| a["role"].as_str() != Some("poker"));
        std::fs::write(
            dir.path().join("team.yml"),
            serde_yaml::to_string(&team).expect("yaml"),
        )
        .expect("write");
        let report = validate_root(dir.path(), &catalog, None).expect("clean");
        assert_eq!(report.archived, vec!["poker".to_owned()]);
        assert!(!report.roles.contains(&"poker".to_owned()));
    }

    #[test]
    fn a_skill_without_frontmatter_and_a_bad_manifest_are_both_named() {
        let (dir, catalog) = seeded();
        std::fs::create_dir_all(dir.path().join("skills/marker")).expect("mkdir");
        std::fs::write(
            dir.path().join("skills/marker/SKILL.md"),
            "no frontmatter\n",
        )
        .expect("write");
        std::fs::write(dir.path().join("team.yml"), "schema: nope\n").expect("write");
        let refusals = validate_root(dir.path(), &catalog, None).expect_err("refuses");
        let paths: Vec<&str> = refusals.iter().map(|r| r.path.as_str()).collect();
        assert!(paths.contains(&"skills/marker/SKILL.md"), "{paths:?}");
        assert!(paths.contains(&"team.yml"), "{paths:?}");
    }

    #[test]
    fn a_refused_actions_file_is_named() {
        let (dir, catalog) = seeded();
        let mut parser = |_: &str| -> Result<usize, String> { Err("actions: bad".into()) };
        let refusals = validate_root(dir.path(), &catalog, Some(&mut parser)).expect_err("refuses");
        assert_eq!(refusals.len(), 1);
        assert_eq!(refusals[0].path, "actions.yml");
        assert_eq!(refusals[0].reason, "actions: bad");
    }
}
