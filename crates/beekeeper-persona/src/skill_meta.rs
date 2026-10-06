//! What a skill says about itself: the `name` and `description` in its
//! `SKILL.md` frontmatter.
//!
//! A pack's `skills/<name>/SKILL.md` is craft a persona's prompt only refers
//! to; the frontmatter at the top of that file is where the skill names and
//! describes itself. This module reads exactly that and nothing else — it
//! never infers a description from a heading, a directory name, or a
//! persona's prompt — so a screen listing a role's skills lists only skills
//! whose `SKILL.md` was actually opened and parsed.
//!
//! `shared` follows [`crate::pack::resolve_skills`]: a skill claimed by no
//! persona in the pack goes to every persona, and that is the fact the flag
//! carries. It is computed from the pack's own persona files, never from the
//! resolved skill list alone, because a resolved list has already merged the
//! two and cannot say which was which.

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

use crate::pack::{self, PackError};
use crate::persona::{split_frontmatter, MAX_BODY_BYTES, MAX_FRONTMATTER_BYTES};
use crate::resolve::ResolvedPersona;

/// Largest `SKILL.md` this module will read, in bytes: the same ceiling a
/// persona file gets, because the file has the same two halves.
pub const MAX_SKILL_MD_BYTES: u64 = (MAX_FRONTMATTER_BYTES + MAX_BODY_BYTES) as u64;

/// What one skill's `SKILL.md` frontmatter declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMeta {
    /// The frontmatter `name`, or the skill directory's name when the
    /// frontmatter declares none. The directory name is not a guess: it is
    /// the name the persona claims the skill by and the name it is
    /// materialized under (`.agents/skills/<name>`).
    pub name: String,
    /// The frontmatter `description`, verbatim after trimming, or `""` when
    /// the frontmatter declares none. Never synthesized from the body.
    pub description: String,
    /// `true` when no persona in the pack claims this skill, so every persona
    /// receives it — the same rule [`crate::pack::resolve_skills`] applies.
    pub shared: bool,
}

/// The two frontmatter keys this module reads. Other keys are ignored rather
/// than refused: a `SKILL.md` may carry fields for tools this crate does not
/// know, and a description is still a description beside them.
#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// Read `skill_dir/SKILL.md` and return what its frontmatter declares.
///
/// `shared` is always `false` here — one directory cannot know whether a
/// persona claims it; [`list_skill_meta`] sets it against the pack.
///
/// # Errors
/// [`PackError::Io`] when the file cannot be read, [`PackError::FileParse`]
/// when it is over [`MAX_SKILL_MD_BYTES`], has no `---` frontmatter, or the
/// frontmatter is not YAML this module can read. A file with no frontmatter
/// is an error rather than a skill with an empty description, because "this
/// skill describes itself as nothing" and "this file was not read" must stay
/// distinct.
pub fn read_skill_meta(skill_dir: &Path) -> Result<SkillMeta, PackError> {
    let path = skill_dir.join("SKILL.md");
    let size = std::fs::metadata(&path)
        .map_err(|source| PackError::Io {
            path: path.clone(),
            source,
        })?
        .len();
    if size > MAX_SKILL_MD_BYTES {
        return Err(PackError::FileParse {
            path,
            reason: format!("file too large: {size} bytes (max {MAX_SKILL_MD_BYTES})"),
        });
    }
    let content = std::fs::read_to_string(&path).map_err(|source| PackError::Io {
        path: path.clone(),
        source,
    })?;
    let (frontmatter, _body) =
        split_frontmatter(&content).map_err(|error| PackError::FileParse {
            path: path.clone(),
            reason: error.to_string(),
        })?;
    let parsed: SkillFrontmatter = if frontmatter.trim().is_empty() {
        SkillFrontmatter::default()
    } else {
        serde_yaml::from_str(frontmatter).map_err(|error| PackError::FileParse {
            path: path.clone(),
            reason: format!("failed to parse YAML frontmatter: {error}"),
        })?
    };
    let directory_name = skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    let name = parsed
        .name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or(directory_name);
    let description = parsed
        .description
        .map(|description| description.trim().to_owned())
        .unwrap_or_default();
    Ok(SkillMeta {
        name,
        description,
        shared: false,
    })
}

/// Every skill of `persona` whose `SKILL.md` could be read, with `shared` set
/// against the pack's own persona files.
///
/// Walks [`ResolvedPersona::skills`] — the effective list, claimed plus
/// shared, already normalized to bare directory names — and reads each
/// `<skills dir>/<name>/SKILL.md`. A skill whose file cannot be read is
/// **omitted**, not invented: a row for it would be a claim about a file no
/// one opened. The order is the persona's own skill order.
///
/// # Errors
/// [`PackError`] when the pack itself cannot be loaded, because `shared`
/// cannot be decided without the pack's persona files and guessing it would
/// be worse than saying so.
pub fn list_skill_meta(
    pack_dir: &Path,
    persona: &ResolvedPersona,
) -> Result<Vec<SkillMeta>, PackError> {
    let loaded = pack::load_pack(pack_dir)?;
    let claimed: HashSet<String> = loaded
        .personas
        .iter()
        .flat_map(|persona| persona.skills.iter())
        .map(|path| bare_skill_name(path))
        .collect();
    let skills_dir = persona
        .skills_dir
        .clone()
        .unwrap_or_else(|| pack_dir.join("skills"));
    Ok(persona
        .skills
        .iter()
        .filter_map(|name| {
            read_skill_meta(&skills_dir.join(name))
                .ok()
                .map(|meta| SkillMeta {
                    shared: !claimed.contains(name),
                    ..meta
                })
        })
        .collect())
}

/// A persona's claimed skill path (`./skills/search/`, `skills/search`,
/// `search`) reduced to the bare directory name — the same normalization
/// [`crate::pack::resolve_skills`] applies before comparing claims to
/// directories.
fn bare_skill_name(path: &str) -> String {
    Path::new(path.trim_end_matches('/'))
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// A pack with one persona that claims `claimed` and a `skills/`
    /// directory holding `skills`, each with the given `SKILL.md` body.
    fn make_pack(dir: &TempDir, claimed: &[&str], skills: &[(&str, &str)]) -> std::path::PathBuf {
        let root = dir.path().to_path_buf();
        fs::create_dir_all(root.join(".plugin")).unwrap();
        fs::write(
            root.join(".plugin/plugin.json"),
            r#"{"id":"test-pack","name":"Test Pack","version":"0.3.0","personas":["personas/lead.persona.md"]}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("personas")).unwrap();
        let claims = claimed
            .iter()
            .map(|name| format!("  - skills/{name}"))
            .collect::<Vec<_>>()
            .join("\n");
        let skills_block = if claimed.is_empty() {
            String::new()
        } else {
            format!("skills:\n{claims}\n")
        };
        fs::write(
            root.join("personas/lead.persona.md"),
            format!(
                "---\nname: lead\ndisplay_name: Lead\ndescription: Leads.\nrole: lead\n{skills_block}---\nYou lead.\n"
            ),
        )
        .unwrap();
        for (name, body) in skills {
            let skill_dir = root.join("skills").join(name);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(skill_dir.join("SKILL.md"), body).unwrap();
        }
        root
    }

    #[test]
    fn reads_name_and_description_from_frontmatter() {
        let dir = TempDir::new().unwrap();
        let skill = dir.path().join("write-brief");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: write-brief\ndescription: \"How to write a locked brief.\"\n---\n# Write a brief\n",
        )
        .unwrap();
        let meta = read_skill_meta(&skill).unwrap();
        assert_eq!(meta.name, "write-brief");
        assert_eq!(meta.description, "How to write a locked brief.");
        assert!(!meta.shared);
    }

    #[test]
    fn a_missing_description_reads_as_empty() {
        let dir = TempDir::new().unwrap();
        let skill = dir.path().join("hire");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "---\nname: hire\n---\nBody.\n").unwrap();
        let meta = read_skill_meta(&skill).unwrap();
        assert_eq!(meta.name, "hire");
        assert_eq!(meta.description, "");
    }

    #[test]
    fn a_missing_name_falls_back_to_the_directory_name() {
        let dir = TempDir::new().unwrap();
        let skill = dir.path().join("triage-report");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\ndescription: Triage a report.\n---\nBody.\n",
        )
        .unwrap();
        let meta = read_skill_meta(&skill).unwrap();
        assert_eq!(meta.name, "triage-report");
        assert_eq!(meta.description, "Triage a report.");
    }

    #[test]
    fn a_file_with_no_frontmatter_is_an_error() {
        let dir = TempDir::new().unwrap();
        let skill = dir.path().join("bare");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# Bare\n\nJust markdown.\n").unwrap();
        let err = read_skill_meta(&skill).unwrap_err();
        assert!(matches!(err, PackError::FileParse { .. }), "got: {err}");
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let dir = TempDir::new().unwrap();
        let skill = dir.path().join("ghost");
        fs::create_dir_all(&skill).unwrap();
        let err = read_skill_meta(&skill).unwrap_err();
        assert!(matches!(err, PackError::Io { .. }), "got: {err}");
    }

    #[test]
    fn unknown_frontmatter_keys_are_ignored() {
        let dir = TempDir::new().unwrap();
        let skill = dir.path().join("choose-model");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: choose-model\ndescription: Pick a model.\nallowed-tools: [Bash]\n---\n",
        )
        .unwrap();
        let meta = read_skill_meta(&skill).unwrap();
        assert_eq!(meta.description, "Pick a model.");
    }

    #[test]
    fn claimed_and_shared_skills_are_told_apart() {
        let dir = TempDir::new().unwrap();
        let root = make_pack(
            &dir,
            &["write-brief"],
            &[
                (
                    "write-brief",
                    "---\nname: write-brief\ndescription: Claimed by the lead.\n---\n",
                ),
                (
                    "beekeeper-project",
                    "---\nname: beekeeper-project\ndescription: Shared with everyone.\n---\n",
                ),
            ],
        );
        let persona = crate::resolve::resolve_persona_by_name(&root, "lead").unwrap();
        let skills = list_skill_meta(&root, &persona).unwrap();
        assert_eq!(skills.len(), 2, "{skills:?}");
        let claimed = skills.iter().find(|s| s.name == "write-brief").unwrap();
        assert!(!claimed.shared);
        assert_eq!(claimed.description, "Claimed by the lead.");
        let shared = skills
            .iter()
            .find(|s| s.name == "beekeeper-project")
            .unwrap();
        assert!(shared.shared);
        assert_eq!(shared.description, "Shared with everyone.");
    }

    #[test]
    fn a_skill_whose_file_cannot_be_read_is_omitted_not_invented() {
        let dir = TempDir::new().unwrap();
        // A claimed skill with no SKILL.md at all: resolve keeps the claim
        // (it is the persona's problem), and this list must not describe it.
        let root = make_pack(
            &dir,
            &["missing"],
            &[("present", "---\nname: present\ndescription: Here.\n---\n")],
        );
        let persona = crate::resolve::resolve_persona_by_name(&root, "lead").unwrap();
        assert!(persona.skills.contains(&"missing".to_owned()));
        let skills = list_skill_meta(&root, &persona).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "present");
        assert!(skills[0].shared);
    }

    #[test]
    fn a_pack_with_no_skills_lists_none() {
        let dir = TempDir::new().unwrap();
        let root = make_pack(&dir, &[], &[]);
        let persona = crate::resolve::resolve_persona_by_name(&root, "lead").unwrap();
        assert!(list_skill_meta(&root, &persona).unwrap().is_empty());
    }

    #[test]
    fn a_pack_that_cannot_be_loaded_is_an_error_not_a_guess() {
        let dir = TempDir::new().unwrap();
        let root = make_pack(&dir, &[], &[]);
        let persona = crate::resolve::resolve_persona_by_name(&root, "lead").unwrap();
        let elsewhere = TempDir::new().unwrap();
        let err = list_skill_meta(elsewhere.path(), &persona).unwrap_err();
        assert!(matches!(err, PackError::ManifestNotFound(_)), "got: {err}");
    }
}
