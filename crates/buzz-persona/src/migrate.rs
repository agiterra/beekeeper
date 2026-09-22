//! Converting a pack-layout role source into an agents repository (spec
//! § 4.11).
//!
//! A project created before 2026-09-18 points its kind:30624 at a packs
//! repository laid out one directory per role —
//! `<path>/<role>/.plugin/plugin.json`, `personas/<name>.persona.md`,
//! `skills/<skill>/SKILL.md`. The agents repository is flat:
//! `roles/<role>.md`, `roles/<role>/skills/<skill>/SKILL.md`, plus the
//! manifest, the actions file and `plans/`. This module turns the first
//! into the second so a legacy project can be migrated without rewriting
//! what its roles say.
//!
//! **What is preserved, exactly.** The persona's body is copied verbatim,
//! byte for byte, and so is every `SKILL.md` beside it. Two things cannot
//! survive the move and are reported rather than hidden:
//!
//! - The frontmatter's `skills:` list is dropped. In the flat layout a
//!   claimed skill path resolves against the repository root, not the role,
//!   and the composer also auto-claims everything under
//!   `roles/<role>/skills/` — keeping the list would either point at the
//!   shared directory or collide with the auto-claim. Every skill the pack
//!   carried is still copied and still reaches the seat; a skill the pack
//!   left unclaimed becomes claimed, and the composed `skills:` list comes
//!   out in directory order rather than the author's.
//! - The pack's identity (`pack_id`, `pack_version`) belongs to the pack
//!   manifest. A flat role takes `project:<name>` and `team.yml`'s version,
//!   so the composed provenance digest necessarily differs.
//!
//! Nothing is read from the relay here and nothing is written to the source:
//! this is a pure directory-to-directory conversion the caller commits.

use std::path::{Path, PathBuf};

use crate::compose::{FLAT_ROLES_DIR, FLAT_SKILLS_DIR};
use crate::persona::is_valid_role_slug;
use crate::seed::{
    agent_display_name, readme, refuse_non_empty, seeded_actions_yml, yaml_string, ACTIONS_YML,
    DEFAULT_LEAD, MODEL_REGISTRY_YML, PLANS_DIR, README_MD, SEEDED_MODEL_REGISTRY,
    SEED_TEAM_VERSION,
};
use crate::team::{AgentsRepoAccess, ARCHIVE_DIR, TEAM_SCHEMA, TEAM_YML};

/// The manifest a pack directory must carry to be one.
const PACK_MANIFEST: &str = ".plugin/plugin.json";

/// Where a pack keeps its personas.
const PACK_PERSONAS_DIR: &str = "personas";

/// The suffix a persona file carries.
const PERSONA_SUFFIX: &str = ".persona.md";

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("{path} is not a directory holding one pack per role")]
    NotADirectory { path: PathBuf },

    #[error(
        "no role packs under {path}: a pack is a directory holding {PACK_MANIFEST} and a persona"
    )]
    NoRoles { path: PathBuf },

    #[error(
        "pack directory {name:?} is not a role slug (1-64 bytes of [a-z0-9-], not {ARCHIVE_DIR:?})"
    )]
    NotARoleSlug { name: String },

    #[error("pack {role} has no persona under {PACK_PERSONAS_DIR}/")]
    NoPersona { role: String },

    #[error("persona {path} has no frontmatter; a role file needs at least a description")]
    NoFrontmatter { path: PathBuf },

    #[error("persona {path} has frontmatter this converter cannot read: {reason}")]
    BadFrontmatter { path: PathBuf, reason: String },

    #[error("persona {path} declares role {declared:?}, but its pack directory is {role:?}")]
    RoleMismatch {
        path: PathBuf,
        declared: String,
        role: String,
    },

    #[error(transparent)]
    Seed(#[from] crate::seed::SeedError),

    #[error("failed to {operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// One converted role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedRole {
    /// The role slug, which is the pack directory's name.
    pub role: String,
    /// The persona file the body came from, relative to the source root.
    pub persona: String,
    /// Skills copied under `roles/<role>/skills/`, ascending.
    pub skills: Vec<String>,
    /// Frontmatter keys the conversion dropped, ascending. Today only
    /// `skills`; listed rather than assumed so a reader sees it.
    pub dropped_keys: Vec<String>,
}

/// What a conversion wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionReport {
    /// Roles converted, ascending by slug.
    pub roles: Vec<ConvertedRole>,
    /// The lead named in `team.yml`.
    pub lead: String,
    /// Every file written, relative to the destination, in write order.
    pub files: Vec<String>,
}

impl ConversionReport {
    /// The role slugs, ascending.
    pub fn role_slugs(&self) -> Vec<String> {
        self.roles.iter().map(|r| r.role.clone()).collect()
    }
}

/// Read the pack directories under `src` without writing anything: the same
/// enumeration [`convert_pack_tree`] performs, so a caller can show what a
/// migration would convert before it asks for one.
pub fn preview_pack_tree(src: &Path) -> Result<Vec<String>, MigrateError> {
    Ok(pack_dirs(src)?.into_iter().map(|(role, _)| role).collect())
}

/// Convert the pack tree at `src` into an agents repository at `dest`.
///
/// `dest` must be empty but for a `.git` directory — a conversion is a first
/// commit, as a seed is. `name` becomes `team.yml`'s `name`, normally the
/// project slug.
///
/// Every role gets `workspace.agents_repo: read` and the lead `write`: a
/// migrated project's plans are in the repository the roles now carry, so a
/// seat that cannot read it cannot do the job it could do before.
///
/// # Errors
/// Every variant of [`MigrateError`]; nothing is written once one is
/// returned except the files listed in a report that never came back.
pub fn convert_pack_tree(
    src: &Path,
    dest: &Path,
    name: &str,
) -> Result<ConversionReport, MigrateError> {
    refuse_non_empty(dest)?;
    let packs = pack_dirs(src)?;

    // Read and validate every persona before writing anything, so a tree
    // that cannot convert leaves no half-written repository behind.
    let mut prepared = Vec::with_capacity(packs.len());
    for (role, dir) in packs {
        prepared.push(prepare_role(&role, &dir, src)?);
    }

    let lead = prepared
        .iter()
        .find(|r| r.role == DEFAULT_LEAD)
        .map(|r| r.role.clone())
        .unwrap_or_else(|| prepared[0].role.clone());

    let mut files = Vec::new();

    for (rel, bytes) in [
        (README_MD.to_owned(), readme(name)),
        (TEAM_YML.to_owned(), team_yml(name, &lead, &prepared)),
        (ACTIONS_YML.to_owned(), seeded_actions_yml()),
        (
            MODEL_REGISTRY_YML.to_owned(),
            SEEDED_MODEL_REGISTRY.to_owned(),
        ),
    ] {
        write_file(dest, &rel, bytes.as_bytes())?;
        files.push(rel);
    }

    let mut roles = Vec::with_capacity(prepared.len());
    for role in &prepared {
        let role_file = format!("{FLAT_ROLES_DIR}/{}.md", role.role);
        write_file(dest, &role_file, role.text.as_bytes())?;
        files.push(role_file);
        let mut skills = Vec::new();
        for skill in &role.skills {
            let from = role.dir.join(FLAT_SKILLS_DIR).join(skill);
            let into = format!("{FLAT_ROLES_DIR}/{}/{FLAT_SKILLS_DIR}/{skill}", role.role);
            for rel in copy_tree(&from, &dest.join(&into))? {
                files.push(format!("{into}/{rel}"));
            }
            skills.push(skill.clone());
        }
        roles.push(ConvertedRole {
            role: role.role.clone(),
            persona: role.persona_rel.clone(),
            skills,
            dropped_keys: role.dropped_keys.clone(),
        });
    }

    for dir in [
        format!("{FLAT_ROLES_DIR}/{ARCHIVE_DIR}"),
        FLAT_SKILLS_DIR.to_owned(),
        PLANS_DIR.to_owned(),
        format!("{PLANS_DIR}/{ARCHIVE_DIR}"),
    ] {
        let keep = format!("{dir}/.gitkeep");
        write_file(dest, &keep, b"")?;
        files.push(keep);
    }

    Ok(ConversionReport { roles, lead, files })
}

/// A role read and checked, before anything is written.
struct PreparedRole {
    role: String,
    dir: PathBuf,
    persona_rel: String,
    /// The `roles/<role>.md` text: rewritten frontmatter, verbatim body.
    text: String,
    /// Skill directory names under the pack's `skills/`, ascending.
    skills: Vec<String>,
    dropped_keys: Vec<String>,
}

/// The pack directories under `src`, ascending by role slug.
fn pack_dirs(src: &Path) -> Result<Vec<(String, PathBuf)>, MigrateError> {
    if !src.is_dir() {
        return Err(MigrateError::NotADirectory {
            path: src.to_path_buf(),
        });
    }
    let mut out = Vec::new();
    for entry in read_dir(src)? {
        let path = entry.path();
        if !path.is_dir() || !path.join(PACK_MANIFEST).is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if !is_valid_role_slug(&name) || name == ARCHIVE_DIR {
            return Err(MigrateError::NotARoleSlug { name });
        }
        out.push((name, path));
    }
    if out.is_empty() {
        return Err(MigrateError::NoRoles {
            path: src.to_path_buf(),
        });
    }
    out.sort();
    Ok(out)
}

/// Read one pack's persona and skills and build its role file's text.
fn prepare_role(role: &str, dir: &Path, src: &Path) -> Result<PreparedRole, MigrateError> {
    let persona_path = persona_file(role, dir)?;
    let content = read_to_string(&persona_path)?;
    let (frontmatter, body) =
        split_frontmatter(&content).ok_or_else(|| MigrateError::NoFrontmatter {
            path: persona_path.clone(),
        })?;
    let mut map: serde_yaml::Mapping =
        serde_yaml::from_str(frontmatter).map_err(|error| MigrateError::BadFrontmatter {
            path: persona_path.clone(),
            reason: error.to_string(),
        })?;
    if let Some(declared) = map.get("role").and_then(|v| v.as_str()) {
        if declared != role {
            return Err(MigrateError::RoleMismatch {
                path: persona_path.clone(),
                declared: declared.to_owned(),
                role: role.to_owned(),
            });
        }
    }
    let mut dropped_keys = Vec::new();
    if map.remove("skills").is_some() {
        dropped_keys.push("skills".to_owned());
    }
    // `name` in a pack persona is the persona's name inside the pack; in a
    // role file the stem is the role, and a differing `name` would only
    // rename the composed persona. Keep it when it equals the role, drop it
    // otherwise rather than carrying a name that means something else here.
    if map
        .get("name")
        .and_then(|v| v.as_str())
        .is_some_and(|value| value != role)
    {
        map.remove("name");
        dropped_keys.push("name".to_owned());
    }
    dropped_keys.sort();

    let rendered = serde_yaml::to_string(&map).map_err(|error| MigrateError::BadFrontmatter {
        path: persona_path.clone(),
        reason: error.to_string(),
    })?;
    let text = format!("---\n{}---\n{body}", rendered.trim_start_matches("---\n"));

    let mut skills = Vec::new();
    let skills_dir = dir.join(FLAT_SKILLS_DIR);
    if skills_dir.is_dir() {
        for entry in read_dir(&skills_dir)? {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !path.is_dir() || !path.join("SKILL.md").is_file() {
                continue;
            }
            skills.push(name);
        }
    }
    skills.sort();

    Ok(PreparedRole {
        role: role.to_owned(),
        dir: dir.to_path_buf(),
        persona_rel: relative(src, &persona_path),
        text,
        skills,
        dropped_keys,
    })
}

/// The persona to convert: the one whose `role` is this role, else whose
/// file stem is, else the only one there is.
fn persona_file(role: &str, dir: &Path) -> Result<PathBuf, MigrateError> {
    let personas_dir = dir.join(PACK_PERSONAS_DIR);
    let mut candidates = Vec::new();
    if personas_dir.is_dir() {
        for entry in read_dir(&personas_dir)? {
            let path = entry.path();
            if path.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(PERSONA_SUFFIX)
            {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    let declares_role = candidates.iter().find(|path| {
        read_to_string(path)
            .ok()
            .and_then(|content| {
                let (frontmatter, _) = split_frontmatter(&content)?;
                let map: serde_yaml::Mapping = serde_yaml::from_str(frontmatter).ok()?;
                map.get("role")
                    .and_then(|v| v.as_str())
                    .map(|v| v == role)
                    .or(Some(false))
            })
            .unwrap_or(false)
    });
    if let Some(path) = declares_role {
        return Ok(path.clone());
    }
    let named = candidates.iter().find(|path| {
        path.file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(PERSONA_SUFFIX))
            == Some(role)
    });
    match named.or_else(|| candidates.first()) {
        Some(path) => Ok(path.clone()),
        None => Err(MigrateError::NoPersona {
            role: role.to_owned(),
        }),
    }
}

/// `team.yml` for a converted tree: every role granted `read`, the lead
/// `write`, one default agent each.
fn team_yml(name: &str, lead: &str, roles: &[PreparedRole]) -> String {
    let mut out = format!(
        "# The project's team (spec § 4.2), converted from a pack-layout role\n\
         # source. Roles are files under roles/; a role listed here gets advisory\n\
         # runtime/model hints and an agents-repository grant\n\
         # (workspace.agents_repo: none | read | write). Retired roles live under\n\
         # roles/archive/ and may not be named here.\n\
         schema: {TEAM_SCHEMA}\nname: {}\nversion: {SEED_TEAM_VERSION}\nlead: {lead}\nroles:\n",
        yaml_string(name)
    );
    for role in roles {
        let access = if role.role == lead {
            AgentsRepoAccess::Write
        } else {
            AgentsRepoAccess::Read
        };
        let access = match access {
            AgentsRepoAccess::None => "none",
            AgentsRepoAccess::Read => "read",
            AgentsRepoAccess::Write => "write",
        };
        out.push_str(&format!(
            "  {}: {{ workspace: {{ agents_repo: {access} }} }}\n",
            role.role
        ));
    }
    out.push_str("agents:\n");
    for role in roles {
        let lifetime = if role.role == lead {
            "persistent"
        } else {
            "ephemeral"
        };
        out.push_str(&format!(
            "  - {{ name: {}, role: {}, lifetime: {lifetime} }}\n",
            yaml_string(&agent_display_name(&role.role)),
            role.role
        ));
    }
    out
}

/// Split `---\n…\n---\n` frontmatter from the body, returning both. `None`
/// when the text does not start with a frontmatter block.
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some((&rest[..end + 1], &rest[end + 5..]))
}

/// Copy `from` into `into` recursively, returning the files written
/// relative to `into`, ascending.
fn copy_tree(from: &Path, into: &Path) -> Result<Vec<String>, MigrateError> {
    let mut written = Vec::new();
    let mut stack = vec![(from.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for entry in read_dir(&dir)? {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if path.is_dir() {
                stack.push((path, rel));
            } else if path.is_file() {
                let bytes = std::fs::read(&path).map_err(|source| MigrateError::Io {
                    operation: "read",
                    path: path.clone(),
                    source,
                })?;
                write_file(into, &rel, &bytes)?;
                written.push(rel);
            }
        }
    }
    written.sort();
    Ok(written)
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) -> Result<(), MigrateError> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| MigrateError::Io {
            operation: "create",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&path, bytes).map_err(|source| MigrateError::Io {
        operation: "write",
        path,
        source,
    })
}

fn read_dir(dir: &Path) -> Result<Vec<std::fs::DirEntry>, MigrateError> {
    let entries = std::fs::read_dir(dir).map_err(|source| MigrateError::Io {
        operation: "read",
        path: dir.to_path_buf(),
        source,
    })?;
    let mut out = Vec::new();
    for entry in entries {
        out.push(entry.map_err(|source| MigrateError::Io {
            operation: "read",
            path: dir.to_path_buf(),
            source,
        })?);
    }
    out.sort_by_key(std::fs::DirEntry::file_name);
    Ok(out)
}

fn read_to_string(path: &Path) -> Result<String, MigrateError> {
    std::fs::read_to_string(path).map_err(|source| MigrateError::Io {
        operation: "read",
        path: path.to_path_buf(),
        source,
    })
}

/// `path` relative to `base` when it is under it, else the whole path.
fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
