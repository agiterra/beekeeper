//! Shipped role templates: the versioned prompt fragments a role file
//! includes with `![[beekeeper/<name>@<range>]]`.
//!
//! Contract: `docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md` § 3. A catalog is one
//! directory laid out as `<name>/<semver>/TEMPLATE.md` with an optional
//! `<semver>/skills/<skill>/SKILL.md` beside it. Every supported version is
//! present at once, so resolving a range is a pure function of the range and
//! the catalog this build ships — never of the network, never of a cache.
//!
//! Rules this module enforces, each with the refusal it produces:
//!
//! - the directory name must equal the frontmatter `version`
//!   ([`TemplateError::VersionMismatch`]);
//! - the parent directory name must equal the frontmatter `name`
//!   ([`TemplateError::NameMismatch`]);
//! - `@latest` is the highest version whose `deprecated` is null; a range
//!   that matches only deprecated versions resolves to the highest of them
//!   with a warning; a range matching nothing refuses naming what this build
//!   ships ([`TemplateError::NoMatch`]);
//! - a template is a leaf: it may not contain an include directive of its
//!   own ([`TemplateError::IncludeInTemplate`]).
//!
//! Identical bytes under two versions is a validator warning, not a refusal:
//! [`validate_catalog`] reports it so the catalog does not grow by habit.
//!
//! A template is either a **fragment** (a paragraph roles share:
//! `working-contract`, `memory`) or a **role** (`kind: role`: a whole
//! shipped role's own paragraph and skills, which a project's seeded
//! `roles/<role>.md` includes rather than copies — spec § 4.11). The kind
//! changes nothing about resolution; it tells the seed writer which templates
//! are roles.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::persona::{split_frontmatter, MAX_BODY_BYTES, MAX_FRONTMATTER_BYTES};
use crate::validate::ValidationReport;

/// The file a template's text and frontmatter live in.
pub const TEMPLATE_MD: &str = "TEMPLATE.md";

/// The include prefix that names a shipped template: `![[beekeeper/…]]`.
pub const TEMPLATE_INCLUDE_PREFIX: &str = "beekeeper/";

/// Largest `TEMPLATE.md` this module reads: the same two halves a persona
/// file has, so a template cannot exceed what a persona may.
pub const MAX_TEMPLATE_MD_BYTES: u64 = (MAX_FRONTMATTER_BYTES + MAX_BODY_BYTES) as u64;

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid template {path}: {reason}")]
    Parse { path: PathBuf, reason: String },

    #[error("template {path}: directory says {found:?} but frontmatter says version {expected:?}")]
    VersionMismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },

    #[error("template {path}: directory says {found:?} but frontmatter says name {expected:?}")]
    NameMismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },

    #[error("template {name}@{version} contains an include directive; templates are leaves")]
    IncludeInTemplate { name: String, version: String },

    #[error("no template catalog: this build has no templates directory, so `beekeeper/{name}` cannot resolve")]
    NoCatalog { name: String },

    #[error("unknown template {name:?}; this build ships: {known}")]
    UnknownTemplate { name: String, known: String },

    #[error("this build ships {name} {shipped}; {range} matches none — update Beekeeper or widen the range")]
    NoMatch {
        name: String,
        range: String,
        shipped: String,
    },

    #[error("invalid version range {range:?} for template {name}: {reason}")]
    InvalidRange {
        name: String,
        range: String,
        reason: String,
    },
}

/// What a template is for: a paragraph roles share, or a whole shipped role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateKind {
    /// A paragraph a role includes beside its own text (the default).
    #[default]
    Fragment,
    /// A shipped role's own paragraph and skills; a seeded project role is
    /// one include of this plus the shared fragments.
    Role,
}

/// One version of one template, read from its `TEMPLATE.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Frontmatter `name`, equal to the parent directory's name.
    pub name: String,
    /// Frontmatter `kind`; `fragment` when absent.
    pub kind: TemplateKind,
    /// Frontmatter `version`, equal to this directory's name.
    pub version: semver::Version,
    /// Frontmatter `description`, trimmed.
    pub description: String,
    /// `None` when current; `Some(reason)` when the author flagged it.
    pub deprecated: Option<String>,
    /// Frontmatter `skills`, as declared (template-relative directories).
    pub skills: Vec<String>,
    /// The prompt fragment: everything after the frontmatter, verbatim.
    pub body: String,
    /// Absolute directory holding `TEMPLATE.md` and any `skills/`.
    pub dir: PathBuf,
}

/// The frontmatter a `TEMPLATE.md` may carry. Unknown keys are refused so a
/// typo (`deprecate:`) cannot silently mean "current".
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateFrontmatter {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    #[serde(default)]
    deprecated: Option<String>,
    #[serde(default)]
    kind: TemplateKind,
    #[serde(default)]
    skills: Vec<String>,
}

/// A version constraint as written after `@` in an include.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateRange {
    /// `@latest`: the highest non-deprecated version.
    Latest,
    /// `@1.2.0`: exactly that version. A bare version is exact here, not the
    /// caret the `semver` crate would read it as.
    Exact(semver::Version),
    /// `@^1.0.0`, `@~1.1`, `@>=1.0.0, <2`: a `semver` requirement.
    Req(semver::VersionReq),
}

impl TemplateRange {
    /// Parse the text after `@`.
    pub fn parse(name: &str, range: &str) -> Result<Self, TemplateError> {
        let range = range.trim();
        if range.is_empty() {
            return Err(TemplateError::InvalidRange {
                name: name.to_owned(),
                range: range.to_owned(),
                reason: "empty; write @latest, @1.2.0, @^1.0.0 or @~1.1".to_owned(),
            });
        }
        if range == "latest" {
            return Ok(Self::Latest);
        }
        if let Ok(exact) = semver::Version::parse(range) {
            return Ok(Self::Exact(exact));
        }
        semver::VersionReq::parse(range)
            .map(Self::Req)
            .map_err(|error| TemplateError::InvalidRange {
                name: name.to_owned(),
                range: range.to_owned(),
                reason: error.to_string(),
            })
    }

    fn matches(&self, version: &semver::Version) -> bool {
        match self {
            Self::Latest => true,
            Self::Exact(exact) => exact == version,
            Self::Req(req) => req.matches(version),
        }
    }
}

impl std::fmt::Display for TemplateRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Latest => f.write_str("latest"),
            Self::Exact(version) => write!(f, "{version}"),
            Self::Req(req) => write!(f, "{req}"),
        }
    }
}

/// What resolving one include produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTemplate {
    pub template: Template,
    /// Set when the resolution is legal but worth saying: the range matched
    /// only deprecated versions.
    pub warning: Option<String>,
}

/// Every template this build ships, keyed by name, versions ascending.
#[derive(Debug, Clone, Default)]
pub struct TemplateCatalog {
    /// The catalog directory, or `None` for a build with no templates.
    pub dir: Option<PathBuf>,
    /// The app version this catalog belongs to: the value the wire carries
    /// as the catalog's identity (`packRef.sha` for `app:shipped`).
    pub app_version: String,
    templates: BTreeMap<String, Vec<Template>>,
}

impl TemplateCatalog {
    /// A catalog with no templates. Any `beekeeper/…` include refuses with
    /// [`TemplateError::NoCatalog`].
    pub fn empty(app_version: &str) -> Self {
        Self {
            dir: None,
            app_version: app_version.to_owned(),
            templates: BTreeMap::new(),
        }
    }

    /// Read `<dir>/<name>/<version>/TEMPLATE.md` for every name and version.
    ///
    /// Entries that are not directories, or start with a dot, are skipped;
    /// a version directory without `TEMPLATE.md` is skipped too, because an
    /// empty directory is not a claim. Anything that *is* a `TEMPLATE.md`
    /// must parse, or the whole load refuses: a catalog with one broken
    /// version is not a catalog whose ranges can be trusted.
    pub fn load(dir: &Path, app_version: &str) -> Result<Self, TemplateError> {
        let mut templates: BTreeMap<String, Vec<Template>> = BTreeMap::new();
        for name_entry in read_dirs(dir)? {
            let name = dir_name(&name_entry);
            let mut versions = Vec::new();
            for version_entry in read_dirs(&name_entry)? {
                let md = version_entry.join(TEMPLATE_MD);
                if !md.is_file() {
                    continue;
                }
                versions.push(read_template(&md, &name, &dir_name(&version_entry))?);
            }
            if versions.is_empty() {
                continue;
            }
            versions.sort_by(|a, b| a.version.cmp(&b.version));
            templates.insert(name, versions);
        }
        Ok(Self {
            dir: Some(dir.to_path_buf()),
            app_version: app_version.to_owned(),
            templates,
        })
    }

    /// Names this catalog knows, ascending.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.templates.keys().map(String::as_str)
    }

    /// Every version of `name`, ascending, or an empty slice.
    pub fn versions(&self, name: &str) -> &[Template] {
        self.templates.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The newest non-deprecated version of every `kind: role` template,
    /// ascending by name: the roles a seeded project starts with. A role
    /// template whose every version is deprecated is not offered.
    pub fn role_templates(&self) -> Vec<&Template> {
        self.templates
            .values()
            .filter_map(|versions| {
                versions
                    .iter()
                    .rev()
                    .find(|t| t.kind == TemplateKind::Role && t.deprecated.is_none())
            })
            .collect()
    }

    /// Resolve `name@range` against this catalog.
    pub fn resolve(
        &self,
        name: &str,
        range: &TemplateRange,
    ) -> Result<ResolvedTemplate, TemplateError> {
        if self.dir.is_none() {
            return Err(TemplateError::NoCatalog {
                name: name.to_owned(),
            });
        }
        let versions = self
            .templates
            .get(name)
            .ok_or_else(|| TemplateError::UnknownTemplate {
                name: name.to_owned(),
                known: if self.templates.is_empty() {
                    "nothing".to_owned()
                } else {
                    self.names().collect::<Vec<_>>().join(", ")
                },
            })?;
        let matching: Vec<&Template> = versions
            .iter()
            .filter(|t| range.matches(&t.version))
            .collect();
        if matching.is_empty() {
            return Err(TemplateError::NoMatch {
                name: name.to_owned(),
                range: range.to_string(),
                shipped: shipped_list(versions),
            });
        }
        // `matching` is ascending, so the last current one is the answer.
        if let Some(current) = matching.iter().rev().find(|t| t.deprecated.is_none()) {
            return Ok(ResolvedTemplate {
                template: (*current).clone(),
                warning: None,
            });
        }
        let newest = matching[matching.len() - 1];
        let reason = newest.deprecated.as_deref().unwrap_or("no reason given");
        Ok(ResolvedTemplate {
            template: newest.clone(),
            warning: Some(format!(
                "beekeeper/{name}@{range} resolved to {}, which is deprecated: {reason}",
                newest.version
            )),
        })
    }
}

fn shipped_list(versions: &[Template]) -> String {
    versions
        .iter()
        .map(|t| {
            if t.deprecated.is_some() {
                format!("{} (deprecated)", t.version)
            } else {
                t.version.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned()
}

/// Child directories of `dir`, sorted by name, dotfiles skipped.
fn read_dirs(dir: &Path) -> Result<Vec<PathBuf>, TemplateError> {
    let entries = std::fs::read_dir(dir).map_err(|source| TemplateError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| TemplateError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let file_type = entry.file_type().map_err(|source| TemplateError::Io {
            path: entry.path(),
            source,
        })?;
        if file_type.is_dir() {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

/// Parse one `TEMPLATE.md`, checking it against the directory names that
/// claim it.
pub fn read_template(
    md: &Path,
    expected_name: &str,
    expected_version: &str,
) -> Result<Template, TemplateError> {
    let size = std::fs::metadata(md)
        .map_err(|source| TemplateError::Io {
            path: md.to_path_buf(),
            source,
        })?
        .len();
    if size > MAX_TEMPLATE_MD_BYTES {
        return Err(TemplateError::Parse {
            path: md.to_path_buf(),
            reason: format!("file too large: {size} bytes (max {MAX_TEMPLATE_MD_BYTES})"),
        });
    }
    let content = std::fs::read_to_string(md).map_err(|source| TemplateError::Io {
        path: md.to_path_buf(),
        source,
    })?;
    let (frontmatter, body) =
        split_frontmatter(&content).map_err(|error| TemplateError::Parse {
            path: md.to_path_buf(),
            reason: error.to_string(),
        })?;
    let parsed: TemplateFrontmatter =
        serde_yaml::from_str(frontmatter).map_err(|error| TemplateError::Parse {
            path: md.to_path_buf(),
            reason: format!("failed to parse YAML frontmatter: {error}"),
        })?;
    let name = parsed.name.map(|n| n.trim().to_owned()).unwrap_or_default();
    if name != expected_name {
        return Err(TemplateError::NameMismatch {
            path: md.to_path_buf(),
            expected: name,
            found: expected_name.to_owned(),
        });
    }
    let version_text = parsed
        .version
        .map(|v| v.trim().to_owned())
        .unwrap_or_default();
    if version_text != expected_version {
        return Err(TemplateError::VersionMismatch {
            path: md.to_path_buf(),
            expected: version_text,
            found: expected_version.to_owned(),
        });
    }
    let version = semver::Version::parse(&version_text).map_err(|error| TemplateError::Parse {
        path: md.to_path_buf(),
        reason: format!("version {version_text:?} is not semver: {error}"),
    })?;
    let description = parsed
        .description
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty())
        .ok_or_else(|| TemplateError::Parse {
            path: md.to_path_buf(),
            reason: "description is required".to_owned(),
        })?;
    if body.lines().any(crate::compose::is_include_line) {
        return Err(TemplateError::IncludeInTemplate {
            name,
            version: version_text,
        });
    }
    let deprecated = parsed
        .deprecated
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty());
    let dir = md
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(Template {
        name,
        kind: parsed.kind,
        version,
        description,
        deprecated,
        skills: parsed.skills,
        body: body.to_owned(),
        dir,
    })
}

/// Validate a catalog directory: every `TEMPLATE.md` loads, and no two
/// versions of one template carry identical bytes (a warning — the spec says a
/// new version exists only when the text changed).
pub fn validate_catalog(dir: &Path) -> ValidationReport {
    let mut report = ValidationReport::default();
    let catalog = match TemplateCatalog::load(dir, "validate") {
        Ok(catalog) => catalog,
        Err(error) => {
            report.error(error.to_string());
            return report;
        }
    };
    for name in catalog.names() {
        let versions = catalog.versions(name);
        for (index, later) in versions.iter().enumerate() {
            for earlier in &versions[..index] {
                if earlier.body == later.body && earlier.skills == later.skills {
                    report.warn(format!(
                        "template {name}: versions {} and {} carry identical text and skills; a new version exists only when the template changes",
                        earlier.version, later.version
                    ));
                }
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn template_md(name: &str, version: &str, deprecated: Option<&str>, body: &str) -> String {
        let deprecated = match deprecated {
            Some(reason) => format!("deprecated: {reason:?}\n"),
            None => String::new(),
        };
        format!("---\nname: {name}\nversion: {version}\ndescription: \"{name} at {version}\"\n{deprecated}---\n{body}")
    }

    fn catalog_with(
        versions: &[(&str, Option<&str>, &str)],
    ) -> (tempfile::TempDir, TemplateCatalog) {
        let dir = tempfile::tempdir().unwrap();
        for (version, deprecated, body) in versions {
            write(
                &dir.path().join("memory").join(version).join(TEMPLATE_MD),
                &template_md("memory", version, *deprecated, body),
            );
        }
        let catalog = TemplateCatalog::load(dir.path(), "0.4.2").unwrap();
        (dir, catalog)
    }

    fn resolve(catalog: &TemplateCatalog, range: &str) -> Result<ResolvedTemplate, TemplateError> {
        catalog.resolve("memory", &TemplateRange::parse("memory", range).unwrap())
    }

    #[test]
    fn exact_caret_tilde_and_latest_resolve_as_the_spec_says() {
        let (_dir, catalog) = catalog_with(&[
            ("1.0.0", None, "one\n"),
            ("1.2.0", None, "one-two\n"),
            ("1.3.0", None, "one-three\n"),
            ("2.0.0", None, "two\n"),
        ]);
        assert_eq!(
            resolve(&catalog, "1.2.0")
                .unwrap()
                .template
                .version
                .to_string(),
            "1.2.0"
        );
        assert_eq!(
            resolve(&catalog, "^1.0.0")
                .unwrap()
                .template
                .version
                .to_string(),
            "1.3.0"
        );
        assert_eq!(
            resolve(&catalog, "~1.2")
                .unwrap()
                .template
                .version
                .to_string(),
            "1.2.0"
        );
        assert_eq!(
            resolve(&catalog, "latest")
                .unwrap()
                .template
                .version
                .to_string(),
            "2.0.0"
        );
        assert!(resolve(&catalog, "latest").unwrap().warning.is_none());
    }

    #[test]
    fn a_bare_version_is_exact_not_caret() {
        let (_dir, catalog) = catalog_with(&[("1.0.0", None, "a\n"), ("1.5.0", None, "b\n")]);
        assert_eq!(
            resolve(&catalog, "1.0.0")
                .unwrap()
                .template
                .version
                .to_string(),
            "1.0.0"
        );
    }

    #[test]
    fn latest_skips_deprecated_versions() {
        let (_dir, catalog) = catalog_with(&[
            ("1.0.0", None, "a\n"),
            ("1.1.0", Some("renamed the tool"), "b\n"),
        ]);
        let resolved = resolve(&catalog, "latest").unwrap();
        assert_eq!(resolved.template.version.to_string(), "1.0.0");
        assert!(resolved.warning.is_none());
    }

    #[test]
    fn a_range_matching_only_deprecated_versions_resolves_with_a_warning() {
        let (_dir, catalog) = catalog_with(&[
            ("1.0.0", Some("superseded by 2.0.0"), "a\n"),
            ("2.0.0", None, "b\n"),
        ]);
        let resolved = resolve(&catalog, "^1.0.0").unwrap();
        assert_eq!(resolved.template.version.to_string(), "1.0.0");
        let warning = resolved.warning.unwrap();
        assert!(warning.contains("deprecated"), "{warning}");
        assert!(warning.contains("superseded by 2.0.0"), "{warning}");
    }

    #[test]
    fn a_range_matching_nothing_refuses_naming_the_shipped_versions() {
        let (_dir, catalog) = catalog_with(&[("1.0.0", None, "a\n"), ("1.2.0", None, "b\n")]);
        let error = resolve(&catalog, "^2.0.0").unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("this build ships memory 1.0.0, 1.2.0"),
            "{text}"
        );
        assert!(text.contains("^2.0.0 matches none"), "{text}");
    }

    #[test]
    fn an_unknown_template_names_what_is_known() {
        let (_dir, catalog) = catalog_with(&[("1.0.0", None, "a\n")]);
        let error = catalog
            .resolve("pulse", &TemplateRange::Latest)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown template \"pulse\""), "{error}");
        assert!(error.contains("memory"), "{error}");
    }

    #[test]
    fn an_empty_catalog_refuses_every_include() {
        let catalog = TemplateCatalog::empty("0.4.2");
        let error = catalog
            .resolve("memory", &TemplateRange::Latest)
            .unwrap_err();
        assert!(matches!(error, TemplateError::NoCatalog { .. }));
    }

    #[test]
    fn directory_and_frontmatter_must_agree_on_name_and_version() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("memory").join("1.0.0").join(TEMPLATE_MD),
            &template_md("memory", "1.0.1", None, "a\n"),
        );
        let error = TemplateCatalog::load(dir.path(), "x").unwrap_err();
        assert!(
            matches!(error, TemplateError::VersionMismatch { .. }),
            "{error}"
        );

        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("memory").join("1.0.0").join(TEMPLATE_MD),
            &template_md("recall", "1.0.0", None, "a\n"),
        );
        let error = TemplateCatalog::load(dir.path(), "x").unwrap_err();
        assert!(
            matches!(error, TemplateError::NameMismatch { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_template_may_not_include() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("memory").join("1.0.0").join(TEMPLATE_MD),
            &template_md("memory", "1.0.0", None, "![[beekeeper/other@latest]]\n"),
        );
        let error = TemplateCatalog::load(dir.path(), "x").unwrap_err();
        assert!(
            matches!(error, TemplateError::IncludeInTemplate { .. }),
            "{error}"
        );
    }

    #[test]
    fn unknown_frontmatter_keys_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("memory").join("1.0.0").join(TEMPLATE_MD),
            "---\nname: memory\nversion: 1.0.0\ndescription: d\ndeprecate: yes\n---\nbody\n",
        );
        let error = TemplateCatalog::load(dir.path(), "x").unwrap_err();
        assert!(matches!(error, TemplateError::Parse { .. }), "{error}");
    }

    #[test]
    fn a_template_is_a_fragment_unless_it_says_it_is_a_role() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("memory/1.0.0").join(TEMPLATE_MD),
            &template_md("memory", "1.0.0", None, "Remember.\n"),
        );
        write(
            &dir.path().join("lead/1.0.0").join(TEMPLATE_MD),
            "---\nname: lead\nversion: 1.0.0\ndescription: Leads.\nkind: role\n---\nLead.\n",
        );
        write(
            &dir.path().join("lead/2.0.0").join(TEMPLATE_MD),
            "---\nname: lead\nversion: 2.0.0\ndescription: Leads.\nkind: role\ndeprecated: no\n---\nLead again.\n",
        );
        write(
            &dir.path().join("runner/1.0.0").join(TEMPLATE_MD),
            "---\nname: runner\nversion: 1.0.0\ndescription: Runs.\nkind: role\ndeprecated: gone\n---\nRun.\n",
        );
        let catalog = TemplateCatalog::load(dir.path(), "0.4.2").unwrap();
        assert_eq!(catalog.versions("memory")[0].kind, TemplateKind::Fragment);
        assert_eq!(catalog.versions("lead")[0].kind, TemplateKind::Role);
        // The newest *current* role version is offered; a wholly deprecated
        // role is not.
        let roles: Vec<(&str, String)> = catalog
            .role_templates()
            .iter()
            .map(|t| (t.name.as_str(), t.version.to_string()))
            .collect();
        assert_eq!(roles, vec![("lead", "1.0.0".to_owned())]);

        write(
            &dir.path().join("odd/1.0.0").join(TEMPLATE_MD),
            "---\nname: odd\nversion: 1.0.0\ndescription: Odd.\nkind: overlay\n---\nOdd.\n",
        );
        assert!(TemplateCatalog::load(dir.path(), "0.4.2").is_err());
    }

    #[test]
    fn identical_bytes_under_two_versions_is_a_validator_warning() {
        let (dir, _) = catalog_with(&[("1.0.0", None, "same\n"), ("1.1.0", None, "same\n")]);
        let report = validate_catalog(dir.path());
        assert!(!report.has_errors());
        assert!(report.has_warnings());
        let text = format!("{:?}", report.diagnostics);
        assert!(text.contains("identical"), "{text}");
    }

    #[test]
    fn invalid_ranges_refuse() {
        assert!(TemplateRange::parse("memory", "").is_err());
        assert!(TemplateRange::parse("memory", "banana").is_err());
        assert!(matches!(
            TemplateRange::parse("memory", "^1.0.0").unwrap(),
            TemplateRange::Req(_)
        ));
    }
}
