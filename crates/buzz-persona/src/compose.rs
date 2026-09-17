//! Role composition: expand a role's include directives into one persona
//! and stage it as an ordinary pack directory.
//!
//! Contract: `docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md` § 4.4–4.6. The
//! composer is pure with respect to hosts and relays — it reads the role
//! source and a [`TemplateCatalog`] and produces a [`ComposedRole`]; only
//! [`write_staged_pack`] touches the filesystem, and only under the directory
//! it is handed. The session provider never learns any of this: it receives
//! the staged directory and reads it through `resolve_persona_by_name`
//! exactly as it reads a hand-written pack today.
//!
//! Directives are whole lines, nothing else:
//!
//! | line | meaning |
//! | --- | --- |
//! | `![[beekeeper/<template>@<range>]]` | a shipped template; the range is required |
//! | `![[./<path>]]` | a file under the source root, inserted verbatim |
//! | `![[roles/<role>]]` | another role's *resolved body* from the same source |
//!
//! A `![[` anywhere else is prose, and the composer says so as a warning
//! rather than guessing. Refusals name the chain, the cap or the collision
//! they stand on; nothing is silently dropped or overwritten.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::merge::{HooksData, TriggersData};
use crate::pack::{self, LoadedPack, LoadedPersona, PackError};
use crate::persona::{self, is_valid_role_slug, PersonaConfig, PersonaError, MAX_BODY_BYTES};
use crate::template::{TemplateCatalog, TemplateError, TemplateRange, TEMPLATE_INCLUDE_PREFIX};

/// Schema of `compose.json`, the provenance record beside a staged pack.
pub const COMPOSE_PROVENANCE_SCHEMA: &str = "buzz-composed-role/v1";

/// Filename of the provenance record inside a staged pack directory.
pub const COMPOSE_JSON: &str = "compose.json";

/// Deepest chain of `roles/<role>` includes the composer will follow.
pub const MAX_INCLUDE_DEPTH: usize = 8;

/// Directory holding flat role files under a flat source root.
pub const FLAT_ROLES_DIR: &str = "roles";

/// Directory holding skills every flat role receives.
pub const FLAT_SKILLS_DIR: &str = "skills";

#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    #[error(transparent)]
    Pack(#[from] PackError),

    #[error("invalid role file {path}: {source}")]
    Persona {
        path: PathBuf,
        #[source]
        source: PersonaError,
    },

    #[error(transparent)]
    Template(#[from] TemplateError),

    #[error("failed to {operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("role {role:?} not found in {looked_in}")]
    RoleNotFound { role: String, looked_in: String },

    #[error("role slug {role:?} must be 1-64 bytes of [a-z0-9-]")]
    InvalidRoleSlug { role: String },

    #[error("include cycle: {chain}")]
    Cycle { chain: String },

    #[error("include chain deeper than {max}: {chain}")]
    TooDeep { max: usize, chain: String },

    #[error("composed body of {role} is {bytes} bytes; a persona body may be at most {max}")]
    BodyTooLarge {
        role: String,
        bytes: usize,
        max: usize,
    },

    #[error("skill {name:?} is provided twice, by {first} and {second}; rename one")]
    SkillCollision {
        name: String,
        first: String,
        second: String,
    },

    #[error("skill {name:?} at {path} has no SKILL.md")]
    MissingSkill { name: String, path: PathBuf },

    #[error("{path}: include {directive:?} names a file that does not exist: {target}")]
    IncludeNotFound {
        path: PathBuf,
        directive: String,
        target: PathBuf,
    },

    #[error("{path}: include {directive:?} escapes the source root")]
    IncludeEscapes { path: PathBuf, directive: String },

    #[error("{path}: include {directive:?} is not a form this composer knows: {reason}")]
    InvalidDirective {
        path: PathBuf,
        directive: String,
        reason: String,
    },

    #[error("role {role} has no description: give it one in frontmatter or start the body with a line of prose")]
    NoDescription { role: String },
}

/// Where a role's text comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleSource {
    /// A pack directory (`.plugin/plugin.json`, `personas/*.persona.md`,
    /// `skills/`): the shipped and sibling-repo layout. The persona whose
    /// `role` equals `role` (or, failing that, whose `name` does) is the role.
    Pack { dir: PathBuf, role: String },
    /// A flat project layout: `<root>/roles/<role>.md`, optional
    /// `<root>/roles/<role>/skills/*`, shared `<root>/skills/*`.
    Flat { root: PathBuf, role: String },
}

impl RoleSource {
    pub fn role(&self) -> &str {
        match self {
            Self::Pack { role, .. } | Self::Flat { role, .. } => role,
        }
    }

    fn root(&self) -> &Path {
        match self {
            Self::Pack { dir, .. } => dir,
            Self::Flat { root, .. } => root,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Pack { dir, .. } => format!("pack {}", dir.display()),
            Self::Flat { root, .. } => format!("{}", root.join(FLAT_ROLES_DIR).display()),
        }
    }

    fn with_role(&self, role: &str) -> Self {
        match self {
            Self::Pack { dir, .. } => Self::Pack {
                dir: dir.clone(),
                role: role.to_owned(),
            },
            Self::Flat { root, .. } => Self::Flat {
                root: root.clone(),
                role: role.to_owned(),
            },
        }
    }
}

/// What the caller knows about where the source bytes came from. The
/// composer records it verbatim; it cannot verify a repository coordinate
/// or a commit and does not try.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceProvenance {
    /// `repository`, `branch-override`, `shipped`, or `local`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    /// Source-relative path of the role: `personas/roles/<role>` or
    /// `beekeeper/roles/<role>`.
    pub path: String,
}

impl SourceProvenance {
    /// A source read from a local directory nobody has named on the wire.
    pub fn local(path: impl Into<String>) -> Self {
        Self {
            kind: "local".to_owned(),
            repo: None,
            sha: None,
            path: path.into(),
        }
    }
}

/// One include, as resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludeRecord {
    /// The directive text between `![[` and `]]`.
    #[serde(rename = "ref")]
    pub reference: String,
    /// The template version that answered, for `beekeeper/…` includes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    /// The deprecation reason, when the resolved template carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// Size of an inserted project file, for `./…` includes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// The `compose.json` beside a staged pack: everything a reader needs to
/// say which bytes ran and where they came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeProvenance {
    pub schema: String,
    pub role: String,
    pub source: SourceProvenance,
    pub app_version: String,
    /// `sha256:<hex>` over the staged persona file and every skill file.
    pub digest: String,
    pub includes: Vec<IncludeRecord>,
    pub warnings: Vec<String>,
}

/// A skill the staged pack must carry, and where its directory is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSource {
    /// Bare directory name; the name the seat loads it by.
    pub name: String,
    /// Absolute directory holding `SKILL.md`.
    pub dir: PathBuf,
    /// Where it came from, for the collision message.
    pub origin: String,
}

/// A composed role, ready to stage.
#[derive(Debug, Clone)]
pub struct ComposedRole {
    /// The synthesized manifest id and version.
    pub pack_id: String,
    pub pack_version: String,
    /// The persona as it will be written: frontmatter fields with the body
    /// fully expanded and `skills` rewritten to the staged layout.
    pub persona: PersonaConfig,
    /// Skills from every source, name-unique.
    pub skills: Vec<SkillSource>,
    pub provenance: ComposeProvenance,
}

impl ComposedRole {
    /// The role slug (equal to the persona name in a staged pack).
    pub fn role(&self) -> &str {
        &self.provenance.role
    }

    /// The `personas/<role>.persona.md` bytes a stage writes.
    pub fn persona_markdown(&self) -> String {
        render_persona_markdown(&self.persona)
    }

    /// The `.plugin/plugin.json` bytes a stage writes.
    pub fn manifest_json(&self) -> String {
        let manifest = serde_json::json!({
            "$schema": "https://open-plugin-spec.org/schema/v1/plugin.json",
            "id": self.pack_id,
            "name": self.persona.display_name,
            "version": self.pack_version,
            "description": self.persona.description,
            "personas": [format!("personas/{}.persona.md", self.role())],
        });
        serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n"
    }

    /// The `compose.json` bytes a stage writes.
    pub fn provenance_json(&self) -> String {
        serde_json::to_string_pretty(&self.provenance).unwrap_or_default() + "\n"
    }
}

/// Options that a flat source cannot read from its own files until
/// `team.yml` exists (spec slice A3).
#[derive(Debug, Clone)]
pub struct ComposeOptions {
    /// Manifest id for a flat source; a pack source keeps its own.
    pub pack_id: Option<String>,
    /// Manifest version for a flat source; a pack source keeps its own.
    pub pack_version: Option<String>,
    /// Recorded verbatim in `compose.json`.
    pub source: SourceProvenance,
}

impl ComposeOptions {
    pub fn local(source_path: impl Into<String>) -> Self {
        Self {
            pack_id: None,
            pack_version: None,
            source: SourceProvenance::local(source_path),
        }
    }
}

/// Compose one role from `source` against `catalog`.
pub fn compose_role(
    source: &RoleSource,
    catalog: &TemplateCatalog,
    options: &ComposeOptions,
) -> Result<ComposedRole, ComposeError> {
    if !is_valid_role_slug(source.role()) {
        return Err(ComposeError::InvalidRoleSlug {
            role: source.role().to_owned(),
        });
    }
    let mut state = ComposeState {
        catalog,
        includes: Vec::new(),
        warnings: Vec::new(),
        template_skills: Vec::new(),
    };
    let draft = load_draft(source)?;
    // The role's own first line of prose is its description by default; a
    // template's opening line would describe the template, not the role.
    let own_prose = first_prose_line(&draft.body);
    let mut stack = vec![source.role().to_owned()];
    let body = state.expand(source, &draft.body, &draft.path, &mut stack)?;
    if body.len() > MAX_BODY_BYTES {
        return Err(ComposeError::BodyTooLarge {
            role: source.role().to_owned(),
            bytes: body.len(),
            max: MAX_BODY_BYTES,
        });
    }

    let mut persona = draft.persona;
    persona.prompt = body;
    if persona.description.trim().is_empty() {
        persona.description = own_prose
            .or_else(|| first_prose_line(&persona.prompt))
            .ok_or_else(|| ComposeError::NoDescription {
                role: source.role().to_owned(),
            })?;
    }

    // Skills: the role's own first, then every template's, name-unique.
    let mut skills: Vec<SkillSource> = Vec::new();
    for skill in draft
        .skills
        .into_iter()
        .chain(state.template_skills.drain(..))
    {
        if let Some(existing) = skills.iter().find(|s| s.name == skill.name) {
            return Err(ComposeError::SkillCollision {
                name: skill.name,
                first: existing.origin.clone(),
                second: skill.origin,
            });
        }
        if !skill.dir.join("SKILL.md").is_file() {
            return Err(ComposeError::MissingSkill {
                name: skill.name,
                path: skill.dir,
            });
        }
        skills.push(skill);
    }
    persona.skills = skills
        .iter()
        .map(|s| format!("./skills/{}/", s.name))
        .collect();

    let pack_id = match source {
        RoleSource::Pack { .. } => draft.pack_id,
        RoleSource::Flat { .. } => options
            .pack_id
            .clone()
            .unwrap_or_else(|| format!("project:{}", dir_name(source.root()))),
    };
    let pack_version = match source {
        RoleSource::Pack { .. } => draft.pack_version,
        RoleSource::Flat { .. } => options
            .pack_version
            .clone()
            .unwrap_or_else(|| "0.0.0".to_owned()),
    };

    let mut composed = ComposedRole {
        pack_id,
        pack_version,
        persona,
        skills,
        provenance: ComposeProvenance {
            schema: COMPOSE_PROVENANCE_SCHEMA.to_owned(),
            role: source.role().to_owned(),
            source: options.source.clone(),
            app_version: catalog.app_version.clone(),
            digest: String::new(),
            includes: state.includes,
            warnings: state.warnings,
        },
    };
    composed.provenance.digest = digest_of(&composed)?;
    Ok(composed)
}

/// Write `composed` as a pack directory under `dest`:
///
/// ```text
/// <dest>/.plugin/plugin.json
/// <dest>/personas/<role>.persona.md
/// <dest>/skills/<name>/…
/// <dest>/compose.json
/// ```
///
/// `dest` is created if absent and its files overwritten if present. A
/// caller keying `dest` by [`ComposeProvenance::digest`] therefore gets an
/// immutable directory per distinct composition.
pub fn write_staged_pack(composed: &ComposedRole, dest: &Path) -> Result<PathBuf, ComposeError> {
    let role = composed.role();
    write_file(
        &dest.join(".plugin").join("plugin.json"),
        composed.manifest_json().as_bytes(),
    )?;
    write_file(
        &dest.join("personas").join(format!("{role}.persona.md")),
        composed.persona_markdown().as_bytes(),
    )?;
    let skills_root = dest.join("skills");
    for skill in &composed.skills {
        copy_tree(&skill.dir, &skills_root.join(&skill.name))?;
    }
    write_file(
        &dest.join(COMPOSE_JSON),
        composed.provenance_json().as_bytes(),
    )?;
    Ok(dest.to_path_buf())
}

/// True when `line` is exactly an include directive (after trimming).
pub fn is_include_line(line: &str) -> bool {
    parse_directive(line).is_some()
}

/// The text between `![[` and `]]` when `line` is exactly a directive.
fn parse_directive(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix("![[")?.strip_suffix("]]")?;
    let inner = inner.trim();
    if inner.is_empty() || inner.contains("]]") {
        return None;
    }
    Some(inner)
}

// ── internals ────────────────────────────────────────────────────────────────

struct ComposeState<'a> {
    catalog: &'a TemplateCatalog,
    includes: Vec<IncludeRecord>,
    warnings: Vec<String>,
    template_skills: Vec<SkillSource>,
}

/// A role as read from its source, before expansion.
struct RoleDraft {
    persona: PersonaConfig,
    body: String,
    path: PathBuf,
    skills: Vec<SkillSource>,
    pack_id: String,
    pack_version: String,
}

fn load_draft(source: &RoleSource) -> Result<RoleDraft, ComposeError> {
    match source {
        RoleSource::Pack { dir, role } => {
            let loaded = pack::load_pack(dir)?;
            let persona =
                find_pack_persona(&loaded, role).ok_or_else(|| ComposeError::RoleNotFound {
                    role: role.clone(),
                    looked_in: source.describe(),
                })?;
            let effective = pack::resolve_skills(&loaded.root, &loaded.personas);
            let names = effective.get(&persona.name).cloned().unwrap_or_default();
            let skills = names
                .into_iter()
                .map(|name| SkillSource {
                    dir: loaded.root.join("skills").join(&name),
                    origin: format!("pack {}", dir.display()),
                    name,
                })
                .collect();
            Ok(RoleDraft {
                persona: persona_from_loaded(persona, role),
                body: persona.prompt.clone(),
                path: persona.source_path.clone(),
                skills,
                pack_id: loaded.manifest.id.clone(),
                pack_version: loaded.manifest.version.clone(),
            })
        }
        RoleSource::Flat { root, role } => {
            let path = root.join(FLAT_ROLES_DIR).join(format!("{role}.md"));
            if !path.is_file() {
                return Err(ComposeError::RoleNotFound {
                    role: role.clone(),
                    looked_in: source.describe(),
                });
            }
            let content = read_bounded(&path)?;
            let persona =
                persona::parse_role_md(&content, role).map_err(|source| ComposeError::Persona {
                    path: path.clone(),
                    source,
                })?;
            let mut skills = Vec::new();
            // Frontmatter-claimed, relative to the root.
            for claimed in &persona.skills {
                let dir = safe_join(root, claimed).ok_or_else(|| ComposeError::IncludeEscapes {
                    path: path.clone(),
                    directive: claimed.clone(),
                })?;
                skills.push(SkillSource {
                    name: dir_name(&dir),
                    dir,
                    origin: format!("{} frontmatter", path.display()),
                });
            }
            // Role-private, auto-claimed.
            for dir in skill_dirs(&root.join(FLAT_ROLES_DIR).join(role).join(FLAT_SKILLS_DIR))? {
                skills.push(SkillSource {
                    name: dir_name(&dir),
                    origin: format!("{}", dir.parent().unwrap_or(&dir).display()),
                    dir,
                });
            }
            // Shared: every role receives them.
            for dir in skill_dirs(&root.join(FLAT_SKILLS_DIR))? {
                let name = dir_name(&dir);
                if skills.iter().any(|s| s.name == name && s.dir == dir) {
                    continue;
                }
                skills.push(SkillSource {
                    name,
                    origin: format!("{}", root.join(FLAT_SKILLS_DIR).display()),
                    dir,
                });
            }
            let body = persona.prompt.clone();
            Ok(RoleDraft {
                persona,
                body,
                path,
                skills,
                pack_id: String::new(),
                pack_version: String::new(),
            })
        }
    }
}

fn find_pack_persona<'a>(loaded: &'a LoadedPack, role: &str) -> Option<&'a LoadedPersona> {
    loaded
        .personas
        .iter()
        .find(|p| p.role.as_deref() == Some(role))
        .or_else(|| loaded.personas.iter().find(|p| p.name == role))
}

/// A `PersonaConfig` carrying the *merged* values of a loaded persona, so the
/// staged pack needs no defaults block of its own.
fn persona_from_loaded(lp: &LoadedPersona, role: &str) -> PersonaConfig {
    PersonaConfig {
        name: role.to_owned(),
        display_name: lp.display_name.clone(),
        avatar: lp.avatar.clone(),
        description: lp.description.clone(),
        role: Some(role.to_owned()),
        version: None,
        author: None,
        skills: Vec::new(),
        mcp_servers: lp
            .mcp_servers
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect(),
        subscribe: Some(lp.subscribe.clone()),
        triggers: lp
            .triggers
            .as_ref()
            .map(|t: &TriggersData| persona::RespondTo {
                mentions: Some(t.mentions),
                keywords: t.keywords.clone(),
                all_messages: Some(t.all_messages),
            }),
        model: lp.model.clone(),
        runtime: lp.runtime.clone(),
        temperature: lp.temperature,
        max_context_tokens: lp.max_context_tokens,
        thread_replies: Some(lp.thread_replies),
        broadcast_replies: Some(lp.broadcast_replies),
        hooks: lp.hooks.as_ref().map(|h: &HooksData| persona::Hooks {
            on_start: h.on_start.clone(),
            on_stop: h.on_stop.clone(),
            on_message: h.on_message.clone(),
        }),
        prompt: String::new(),
    }
}

impl ComposeState<'_> {
    fn expand(
        &mut self,
        source: &RoleSource,
        body: &str,
        path: &Path,
        stack: &mut Vec<String>,
    ) -> Result<String, ComposeError> {
        let mut out = String::with_capacity(body.len());
        for line in body.split_inclusive('\n') {
            let bare = line.strip_suffix('\n').unwrap_or(line);
            let bare = bare.strip_suffix('\r').unwrap_or(bare);
            match parse_directive(bare) {
                Some(directive) => {
                    let expanded = self.resolve_directive(source, directive, path, stack)?;
                    out.push_str(&expanded);
                    if !expanded.ends_with('\n') && line.ends_with('\n') {
                        out.push('\n');
                    }
                }
                None => {
                    if bare.contains("![[") {
                        self.warnings.push(format!(
                            "{}: line looks like an include but is not on its own line, so it is prose: {}",
                            path.display(),
                            bare.trim()
                        ));
                    }
                    out.push_str(line);
                }
            }
        }
        Ok(out)
    }

    fn resolve_directive(
        &mut self,
        source: &RoleSource,
        directive: &str,
        path: &Path,
        stack: &mut Vec<String>,
    ) -> Result<String, ComposeError> {
        if let Some(rest) = directive.strip_prefix(TEMPLATE_INCLUDE_PREFIX) {
            let (name, range) =
                rest.split_once('@')
                    .ok_or_else(|| ComposeError::InvalidDirective {
                        path: path.to_path_buf(),
                        directive: directive.to_owned(),
                        reason:
                            "a shipped template needs a version: @latest, @1.2.0, @^1.0.0 or @~1.1"
                                .to_owned(),
                    })?;
            let name = name.trim();
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            {
                return Err(ComposeError::InvalidDirective {
                    path: path.to_path_buf(),
                    directive: directive.to_owned(),
                    reason: "template names are [a-z0-9-]".to_owned(),
                });
            }
            let range = TemplateRange::parse(name, range)?;
            let resolved = self.catalog.resolve(name, &range)?;
            if let Some(warning) = resolved.warning {
                self.warnings.push(warning);
            }
            let template = resolved.template;
            for rel in &template.skills {
                let dir =
                    safe_join(&template.dir, rel).ok_or_else(|| ComposeError::IncludeEscapes {
                        path: template.dir.join(crate::template::TEMPLATE_MD),
                        directive: rel.clone(),
                    })?;
                self.template_skills.push(SkillSource {
                    name: dir_name(&dir),
                    origin: format!("template {}@{}", template.name, template.version),
                    dir,
                });
            }
            self.includes.push(IncludeRecord {
                reference: directive.to_owned(),
                resolved: Some(template.version.to_string()),
                deprecated: template.deprecated.clone(),
                bytes: None,
            });
            return Ok(template.body);
        }

        if let Some(rel) = directive.strip_prefix("./") {
            let target =
                safe_join(source.root(), rel).ok_or_else(|| ComposeError::IncludeEscapes {
                    path: path.to_path_buf(),
                    directive: directive.to_owned(),
                })?;
            if !target.is_file() {
                return Err(ComposeError::IncludeNotFound {
                    path: path.to_path_buf(),
                    directive: directive.to_owned(),
                    target,
                });
            }
            let content = read_bounded(&target)?;
            self.includes.push(IncludeRecord {
                reference: directive.to_owned(),
                resolved: None,
                deprecated: None,
                bytes: Some(content.len() as u64),
            });
            return Ok(content);
        }

        if let Some(role) = directive.strip_prefix("roles/") {
            let role = role.trim().trim_end_matches(".md");
            if !is_valid_role_slug(role) {
                return Err(ComposeError::InvalidDirective {
                    path: path.to_path_buf(),
                    directive: directive.to_owned(),
                    reason: "role slugs are 1-64 bytes of [a-z0-9-]".to_owned(),
                });
            }
            if stack.iter().any(|seen| seen == role) {
                let mut chain = stack.clone();
                chain.push(role.to_owned());
                return Err(ComposeError::Cycle {
                    chain: chain.join(" → "),
                });
            }
            if stack.len() >= MAX_INCLUDE_DEPTH {
                let mut chain = stack.clone();
                chain.push(role.to_owned());
                return Err(ComposeError::TooDeep {
                    max: MAX_INCLUDE_DEPTH,
                    chain: chain.join(" → "),
                });
            }
            let other = source.with_role(role);
            let draft = load_draft(&other)?;
            stack.push(role.to_owned());
            let expanded = self.expand(&other, &draft.body, &draft.path, stack);
            stack.pop();
            let expanded = expanded?;
            self.includes.push(IncludeRecord {
                reference: directive.to_owned(),
                resolved: None,
                deprecated: None,
                bytes: Some(expanded.len() as u64),
            });
            return Ok(expanded);
        }

        Err(ComposeError::InvalidDirective {
            path: path.to_path_buf(),
            directive: directive.to_owned(),
            reason: "expected beekeeper/<template>@<range>, ./<path> or roles/<role>".to_owned(),
        })
    }
}

/// `root/rel` when `rel` is relative, has no `..`, and (if it exists) does not
/// resolve outside `root`.
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim().trim_start_matches("./");
    if rel.is_empty() || rel.starts_with('/') {
        return None;
    }
    let rel_path = Path::new(rel.trim_end_matches('/'));
    if rel_path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    let joined = root.join(rel_path);
    if joined.exists() {
        let canonical_root = root.canonicalize().ok()?;
        let canonical = joined.canonicalize().ok()?;
        if !canonical.starts_with(&canonical_root) {
            return None;
        }
        return Some(canonical);
    }
    Some(joined)
}

/// Child directories of `dir` holding a `SKILL.md`, sorted. Absent dir → none.
fn skill_dirs(dir: &Path) -> Result<Vec<PathBuf>, ComposeError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(dir).map_err(|source| ComposeError::Io {
        operation: "read",
        path: dir.to_path_buf(),
        source,
    })?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ComposeError::Io {
            operation: "read",
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') || !path.is_dir() {
            continue;
        }
        if path.join("SKILL.md").is_file() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn first_prose_line(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("![["))
        .map(str::to_owned)
}

fn read_bounded(path: &Path) -> Result<String, ComposeError> {
    let max = (persona::MAX_FRONTMATTER_BYTES + MAX_BODY_BYTES) as u64;
    let size = std::fs::metadata(path)
        .map_err(|source| ComposeError::Io {
            operation: "stat",
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if size > max {
        return Err(ComposeError::Io {
            operation: "read",
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("file too large: {size} bytes (max {max})"),
            ),
        });
    }
    std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
        operation: "read",
        path: path.to_path_buf(),
        source,
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ComposeError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ComposeError::Io {
            operation: "create",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(path, bytes).map_err(|source| ComposeError::Io {
        operation: "write",
        path: path.to_path_buf(),
        source,
    })
}

/// Copy a skill directory. Symlinks are refused rather than followed, for
/// the reason `skills::materialize_skill_bundle` refuses them: whatever they
/// point at would land in a seat's instructions.
fn copy_tree(source: &Path, target: &Path) -> Result<(), ComposeError> {
    std::fs::create_dir_all(target).map_err(|io| ComposeError::Io {
        operation: "create",
        path: target.to_path_buf(),
        source: io,
    })?;
    let entries = std::fs::read_dir(source).map_err(|io| ComposeError::Io {
        operation: "read",
        path: source.to_path_buf(),
        source: io,
    })?;
    for entry in entries {
        let entry = entry.map_err(|io| ComposeError::Io {
            operation: "read",
            path: source.to_path_buf(),
            source: io,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|io| ComposeError::Io {
            operation: "read",
            path: path.clone(),
            source: io,
        })?;
        if file_type.is_symlink() {
            return Err(ComposeError::IncludeEscapes {
                path,
                directive: "symlink inside a skill directory".to_owned(),
            });
        }
        let dest = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_tree(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest).map_err(|io| ComposeError::Io {
                operation: "copy",
                path: path.clone(),
                source: io,
            })?;
        }
    }
    Ok(())
}

/// Every regular file under `dir`, as (relative path, bytes), sorted by path.
fn tree_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, ComposeError> {
    fn walk(
        base: &Path,
        dir: &Path,
        out: &mut BTreeMap<String, Vec<u8>>,
    ) -> Result<(), ComposeError> {
        let entries = std::fs::read_dir(dir).map_err(|io| ComposeError::Io {
            operation: "read",
            path: dir.to_path_buf(),
            source: io,
        })?;
        for entry in entries {
            let entry = entry.map_err(|io| ComposeError::Io {
                operation: "read",
                path: dir.to_path_buf(),
                source: io,
            })?;
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let bytes = std::fs::read(&path).map_err(|io| ComposeError::Io {
                    operation: "read",
                    path: path.clone(),
                    source: io,
                })?;
                out.insert(rel, bytes);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// `sha256:<hex>` over the staged persona markdown and every skill file,
/// each prefixed by its staged relative path — the same bytes
/// [`write_staged_pack`] writes, so two stages with equal digests are equal
/// directories (the manifest and `compose.json` derive from these).
fn digest_of(composed: &ComposedRole) -> Result<String, ComposeError> {
    let mut hasher = Sha256::new();
    let persona_rel = format!("personas/{}.persona.md", composed.role());
    hasher.update(persona_rel.as_bytes());
    hasher.update([0]);
    hasher.update(composed.persona_markdown().as_bytes());
    hasher.update([0]);
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for skill in &composed.skills {
        names.insert(&skill.name);
    }
    for name in names {
        let Some(skill) = composed.skills.iter().find(|s| s.name == name) else {
            continue;
        };
        for (rel, bytes) in tree_files(&skill.dir)? {
            hasher.update(format!("skills/{name}/{rel}").as_bytes());
            hasher.update([0]);
            hasher.update(&bytes);
            hasher.update([0]);
        }
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Render frontmatter and body. Only keys the `.persona.md` parser knows are
/// written, in a fixed order, so the output re-parses under
/// `deny_unknown_fields` and two equal configs render to equal bytes.
fn render_persona_markdown(persona: &PersonaConfig) -> String {
    use serde_yaml::{Mapping, Value};
    fn s(text: &str) -> Value {
        Value::String(text.to_owned())
    }
    fn list(items: &[String]) -> Value {
        Value::Sequence(items.iter().map(|i| s(i)).collect())
    }
    let mut map = Mapping::new();
    map.insert(s("name"), s(&persona.name));
    map.insert(s("display_name"), s(&persona.display_name));
    map.insert(s("description"), s(&persona.description));
    if let Some(role) = &persona.role {
        map.insert(s("role"), s(role));
    }
    if let Some(version) = &persona.version {
        map.insert(s("version"), s(version));
    }
    if let Some(author) = &persona.author {
        map.insert(s("author"), s(author));
    }
    if let Some(avatar) = &persona.avatar {
        map.insert(s("avatar"), s(avatar));
    }
    if !persona.skills.is_empty() {
        map.insert(s("skills"), list(&persona.skills));
    }
    if let Some(model) = &persona.model {
        map.insert(s("model"), s(model));
    }
    if let Some(runtime) = &persona.runtime {
        map.insert(s("runtime"), s(runtime));
    }
    if let Some(temperature) = persona.temperature {
        map.insert(s("temperature"), Value::Number(temperature.into()));
    }
    if let Some(tokens) = persona.max_context_tokens {
        map.insert(s("max_context_tokens"), Value::Number(tokens.into()));
    }
    if let Some(subscribe) = &persona.subscribe {
        map.insert(s("subscribe"), list(subscribe));
    }
    if let Some(triggers) = &persona.triggers {
        let mut t = Mapping::new();
        if let Some(mentions) = triggers.mentions {
            t.insert(s("mentions"), Value::Bool(mentions));
        }
        if !triggers.keywords.is_empty() {
            t.insert(s("keywords"), list(&triggers.keywords));
        }
        if let Some(all) = triggers.all_messages {
            t.insert(s("all_messages"), Value::Bool(all));
        }
        map.insert(s("triggers"), Value::Mapping(t));
    }
    if let Some(v) = persona.thread_replies {
        map.insert(s("thread_replies"), Value::Bool(v));
    }
    if let Some(v) = persona.broadcast_replies {
        map.insert(s("broadcast_replies"), Value::Bool(v));
    }
    if !persona.mcp_servers.is_empty() {
        if let Ok(value) = serde_yaml::to_value(&persona.mcp_servers) {
            map.insert(s("mcp_servers"), value);
        }
    }
    if let Some(hooks) = &persona.hooks {
        if let Ok(value) = serde_yaml::to_value(hooks) {
            map.insert(s("hooks"), value);
        }
    }
    let yaml = serde_yaml::to_string(&Value::Mapping(map)).unwrap_or_default();
    format!("---\n{yaml}---\n{}", persona.prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::TEMPLATE_MD;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn catalog(dir: &Path) -> TemplateCatalog {
        write(
            &dir.join("working-contract").join("1.0.0").join(TEMPLATE_MD),
            "---\nname: working-contract\nversion: 1.0.0\ndescription: wc\n---\n## Working contract\n\nFollow the project.\n",
        );
        write(
            &dir.join("memory").join("1.0.0").join(TEMPLATE_MD),
            "---\nname: memory\nversion: 1.0.0\ndescription: m\nskills:\n  - ./skills/recall/\n---\nRecall first.\n",
        );
        write(
            &dir.join("memory")
                .join("1.0.0")
                .join("skills")
                .join("recall")
                .join("SKILL.md"),
            "---\nname: recall\ndescription: r\n---\nrecall body\n",
        );
        write(
            &dir.join("memory").join("1.2.0").join(TEMPLATE_MD),
            "---\nname: memory\nversion: 1.2.0\ndescription: m\ndeprecated: \"use 1.0.0\"\n---\nRecall later.\n",
        );
        TemplateCatalog::load(dir, "0.4.2").unwrap()
    }

    fn flat_root(dir: &Path) {
        write(&dir.join("rules.md"), "Rule one.\nRule two.\n");
        write(
            &dir.join("roles").join("project-manager.md"),
            "---\ndescription: \"Keeps the plan.\"\nruntime: claude\n---\n![[beekeeper/working-contract@^1.0.0]]\n![[beekeeper/memory@1.0.0]]\n![[./rules.md]]\n\nAlways re-check pulse.\n",
        );
        write(
            &dir.join("roles").join("builder.md"),
            "# Builder\n\nBuild things well.\nInline ![[not-an-include]] stays.\n",
        );
        write(
            &dir.join("skills")
                .join("verify-before-claiming")
                .join("SKILL.md"),
            "---\nname: verify-before-claiming\ndescription: v\n---\nverify\n",
        );
        write(
            &dir.join("roles")
                .join("builder")
                .join("skills")
                .join("run-ci")
                .join("SKILL.md"),
            "---\nname: run-ci\ndescription: c\n---\nci\n",
        );
    }

    fn compose_flat(
        root: &Path,
        role: &str,
        catalog: &TemplateCatalog,
    ) -> Result<ComposedRole, ComposeError> {
        compose_role(
            &RoleSource::Flat {
                root: root.to_path_buf(),
                role: role.to_owned(),
            },
            catalog,
            &ComposeOptions::local(format!("beekeeper/roles/{role}")),
        )
    }

    #[test]
    fn a_flat_role_expands_templates_and_project_files_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        flat_root(&root);

        let composed = compose_flat(&root, "project-manager", &catalog).unwrap();
        assert_eq!(
            composed.persona.prompt,
            "## Working contract\n\nFollow the project.\nRecall first.\nRule one.\nRule two.\n\nAlways re-check pulse.\n"
        );
        assert_eq!(composed.persona.name, "project-manager");
        assert_eq!(composed.persona.role.as_deref(), Some("project-manager"));
        assert_eq!(composed.persona.display_name, "project-manager");
        assert_eq!(composed.persona.description, "Keeps the plan.");
        assert_eq!(composed.persona.runtime.as_deref(), Some("claude"));
        let skill_names: Vec<&str> = composed.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(skill_names, vec!["verify-before-claiming", "recall"]);
        assert_eq!(
            composed.persona.skills,
            vec!["./skills/verify-before-claiming/", "./skills/recall/"]
        );
        let refs: Vec<&str> = composed
            .provenance
            .includes
            .iter()
            .map(|i| i.reference.as_str())
            .collect();
        assert_eq!(
            refs,
            vec![
                "beekeeper/working-contract@^1.0.0",
                "beekeeper/memory@1.0.0",
                "./rules.md"
            ]
        );
        assert_eq!(
            composed.provenance.includes[1].resolved.as_deref(),
            Some("1.0.0")
        );
        assert_eq!(composed.provenance.includes[2].bytes, Some(20));
        assert!(composed.provenance.digest.starts_with("sha256:"));
        assert_eq!(composed.provenance.app_version, "0.4.2");
        assert_eq!(composed.provenance.source.kind, "local");
        assert!(
            composed.provenance.warnings.is_empty(),
            "{:?}",
            composed.provenance.warnings
        );
    }

    #[test]
    fn a_mid_line_include_is_prose_with_a_warning_and_description_defaults_to_first_prose_line() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        flat_root(&root);

        let composed = compose_flat(&root, "builder", &catalog).unwrap();
        assert!(composed
            .persona
            .prompt
            .contains("Inline ![[not-an-include]] stays."));
        assert_eq!(composed.persona.description, "Build things well.");
        assert_eq!(composed.provenance.warnings.len(), 1);
        assert!(composed.provenance.warnings[0].contains("not on its own line"));
        let skill_names: Vec<&str> = composed.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(skill_names, vec!["run-ci", "verify-before-claiming"]);
    }

    #[test]
    fn a_deprecated_template_resolves_with_a_warning_not_a_refusal() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        write(
            &root.join("roles").join("lead.md"),
            "![[beekeeper/memory@1.2.0]]\n\nLead.\n",
        );
        let composed = compose_flat(&root, "lead", &catalog).unwrap();
        assert_eq!(composed.persona.prompt, "Recall later.\n\nLead.\n");
        assert_eq!(
            composed.provenance.includes[0].deprecated.as_deref(),
            Some("use 1.0.0")
        );
        assert!(composed.provenance.warnings[0].contains("deprecated"));
    }

    #[test]
    fn a_missing_template_version_refuses_naming_the_shipped_set() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        write(
            &root.join("roles").join("lead.md"),
            "![[beekeeper/memory@^3.0.0]]\n",
        );
        let error = compose_flat(&root, "lead", &catalog)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("this build ships memory 1.0.0, 1.2.0 (deprecated)"),
            "{error}"
        );
    }

    #[test]
    fn a_template_include_without_a_version_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        write(
            &root.join("roles").join("lead.md"),
            "![[beekeeper/memory]]\n",
        );
        let error = compose_flat(&root, "lead", &catalog).unwrap_err();
        assert!(
            matches!(error, ComposeError::InvalidDirective { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("needs a version"));
    }

    #[test]
    fn a_cycle_refuses_naming_the_chain() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = TemplateCatalog::empty("x");
        let root = tmp.path().join("beekeeper");
        write(&root.join("roles").join("a.md"), "A.\n![[roles/b]]\n");
        write(&root.join("roles").join("b.md"), "B.\n![[roles/c]]\n");
        write(&root.join("roles").join("c.md"), "C.\n![[roles/a]]\n");
        let error = compose_flat(&root, "a", &catalog).unwrap_err();
        match error {
            ComposeError::Cycle { chain } => assert_eq!(chain, "a → b → c → a"),
            other => panic!("expected a cycle, got {other}"),
        }
    }

    #[test]
    fn a_role_include_inserts_the_other_roles_resolved_body_only() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        write(
            &root.join("roles").join("base.md"),
            "---\ndescription: base\nmodel: x:y\n---\n![[beekeeper/working-contract@latest]]\nBase line.\n",
        );
        write(
            &root.join("roles").join("lead.md"),
            "![[roles/base]]\nLead line.\n",
        );
        let composed = compose_flat(&root, "lead", &catalog).unwrap();
        assert_eq!(
            composed.persona.prompt,
            "## Working contract\n\nFollow the project.\nBase line.\nLead line.\n"
        );
        assert!(
            composed.persona.model.is_none(),
            "frontmatter is not inherited"
        );
        assert_eq!(
            composed.persona.description, "Lead line.",
            "the role's own prose, not the included one"
        );
    }

    #[test]
    fn a_chain_deeper_than_the_cap_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = TemplateCatalog::empty("x");
        let root = tmp.path().join("beekeeper");
        for i in 0..=MAX_INCLUDE_DEPTH {
            let next = if i == MAX_INCLUDE_DEPTH {
                String::new()
            } else {
                format!("![[roles/r{}]]\n", i + 1)
            };
            write(
                &root.join("roles").join(format!("r{i}.md")),
                &format!("R{i}.\n{next}"),
            );
        }
        let error = compose_flat(&root, "r0", &catalog).unwrap_err();
        assert!(matches!(error, ComposeError::TooDeep { .. }), "{error}");
    }

    #[test]
    fn an_expanded_body_over_the_persona_cap_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = TemplateCatalog::empty("x");
        let root = tmp.path().join("beekeeper");
        let big = "x".repeat(MAX_BODY_BYTES / 2 + 10) + "\n";
        write(&root.join("big.md"), &big);
        write(
            &root.join("roles").join("lead.md"),
            "Lead.\n![[./big.md]]\n![[./big.md]]\n",
        );
        let error = compose_flat(&root, "lead", &catalog).unwrap_err();
        assert!(
            matches!(error, ComposeError::BodyTooLarge { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_skill_name_provided_twice_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        write(
            &root.join("skills").join("recall").join("SKILL.md"),
            "---\nname: recall\ndescription: x\n---\nmine\n",
        );
        write(
            &root.join("roles").join("lead.md"),
            "![[beekeeper/memory@1.0.0]]\nLead.\n",
        );
        let error = compose_flat(&root, "lead", &catalog).unwrap_err();
        match error {
            ComposeError::SkillCollision { name, .. } => assert_eq!(name, "recall"),
            other => panic!("expected a collision, got {other}"),
        }
    }

    #[test]
    fn a_project_include_may_not_escape_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = TemplateCatalog::empty("x");
        let root = tmp.path().join("beekeeper");
        write(&tmp.path().join("secret.md"), "s\n");
        write(
            &root.join("roles").join("lead.md"),
            "![[./../secret.md]]\nLead.\n",
        );
        let error = compose_flat(&root, "lead", &catalog).unwrap_err();
        assert!(
            matches!(error, ComposeError::IncludeEscapes { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_missing_project_file_refuses_naming_it() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = TemplateCatalog::empty("x");
        let root = tmp.path().join("beekeeper");
        write(
            &root.join("roles").join("lead.md"),
            "![[./nope.md]]\nLead.\n",
        );
        let error = compose_flat(&root, "lead", &catalog).unwrap_err();
        assert!(
            matches!(error, ComposeError::IncludeNotFound { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_legacy_pack_with_no_includes_composes_byte_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let pack = tmp.path().join("lead");
        write(
            &pack.join(".plugin").join("plugin.json"),
            r#"{"id":"com.beekeeper.crew.lead","name":"Team Lead","version":"0.2.0","personas":["personas/lead.persona.md"],"defaults":{"model":"anthropic:claude"}}"#,
        );
        let body = "Own the outcome.\n\n## Working contract\n\nFollow the project.\n";
        write(
            &pack.join("personas").join("lead.persona.md"),
            &format!("---\nname: lead\nrole: lead\ndisplay_name: \"Lead\"\ndescription: \"Leads.\"\nskills:\n  - \"./skills/hire/\"\n---\n{body}"),
        );
        write(
            &pack.join("skills").join("hire").join("SKILL.md"),
            "---\nname: hire\ndescription: h\n---\nhire\n",
        );
        write(
            &pack.join("skills").join("shared").join("SKILL.md"),
            "---\nname: shared\ndescription: s\n---\nshared\n",
        );

        let composed = compose_role(
            &RoleSource::Pack {
                dir: pack.clone(),
                role: "lead".to_owned(),
            },
            &TemplateCatalog::empty("0.4.2"),
            &ComposeOptions::local("personas/roles/lead"),
        )
        .unwrap();
        assert_eq!(composed.persona.prompt, body);
        assert_eq!(composed.pack_id, "com.beekeeper.crew.lead");
        assert_eq!(composed.pack_version, "0.2.0");
        assert_eq!(
            composed.persona.model.as_deref(),
            Some("anthropic:claude"),
            "pack defaults are baked in"
        );
        let names: Vec<&str> = composed.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["hire", "shared"]);
        assert!(composed.provenance.includes.is_empty());

        // Staged, it is an ordinary pack the existing resolver reads, with
        // the same prompt and the same effective skills.
        let staged = tmp.path().join("staged");
        write_staged_pack(&composed, &staged).unwrap();
        let resolved = crate::resolve::resolve_persona_by_name(&staged, "lead").unwrap();
        assert_eq!(resolved.system_prompt, body);
        assert_eq!(resolved.role.as_deref(), Some("lead"));
        assert_eq!(resolved.model.as_deref(), Some("claude"));
        assert_eq!(resolved.skills, vec!["hire", "shared"]);
        assert!(staged.join(COMPOSE_JSON).is_file());
        let provenance: ComposeProvenance =
            serde_json::from_str(&std::fs::read_to_string(staged.join(COMPOSE_JSON)).unwrap())
                .unwrap();
        assert_eq!(provenance, composed.provenance);
        assert!(!crate::validate::validate_pack(&staged).has_errors());
    }

    #[test]
    fn the_same_inputs_stage_to_the_same_digest_and_a_changed_include_changes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        flat_root(&root);
        let first = compose_flat(&root, "project-manager", &catalog).unwrap();
        let again = compose_flat(&root, "project-manager", &catalog).unwrap();
        assert_eq!(first.provenance.digest, again.provenance.digest);
        write(&root.join("rules.md"), "Rule one, revised.\n");
        let changed = compose_flat(&root, "project-manager", &catalog).unwrap();
        assert_ne!(first.provenance.digest, changed.provenance.digest);
    }

    #[test]
    fn a_staged_flat_role_reparses_under_the_strict_persona_parser() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = catalog(&tmp.path().join("templates"));
        let root = tmp.path().join("beekeeper");
        flat_root(&root);
        let composed = compose_flat(&root, "project-manager", &catalog).unwrap();
        let markdown = composed.persona_markdown();
        let parsed = persona::parse_persona_md(&markdown).unwrap();
        assert_eq!(parsed.prompt, composed.persona.prompt);
        assert_eq!(parsed.skills, composed.persona.skills);
        assert_eq!(parsed.runtime.as_deref(), Some("claude"));
        let staged = tmp.path().join("staged");
        write_staged_pack(&composed, &staged).unwrap();
        assert!(staged
            .join("skills")
            .join("recall")
            .join("SKILL.md")
            .is_file());
        assert!(staged
            .join("skills")
            .join("verify-before-claiming")
            .join("SKILL.md")
            .is_file());
        let resolved = crate::resolve::resolve_persona_by_name(&staged, "project-manager").unwrap();
        assert_eq!(resolved.skills, vec!["verify-before-claiming", "recall"]);
    }

    #[test]
    fn an_unknown_role_and_a_bad_slug_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = TemplateCatalog::empty("x");
        let root = tmp.path().join("beekeeper");
        flat_root(&root);
        assert!(matches!(
            compose_flat(&root, "nobody", &catalog).unwrap_err(),
            ComposeError::RoleNotFound { .. }
        ));
        assert!(matches!(
            compose_flat(&root, "Not Valid", &catalog).unwrap_err(),
            ComposeError::InvalidRoleSlug { .. }
        ));
    }

    #[test]
    fn directive_parsing_is_whole_line_only() {
        assert!(is_include_line("![[beekeeper/memory@latest]]"));
        assert!(is_include_line("  ![[./rules.md]]  "));
        assert!(!is_include_line("see ![[./rules.md]]"));
        assert!(!is_include_line("![[./rules.md]] and more"));
        assert!(!is_include_line("![[]]"));
    }
}
