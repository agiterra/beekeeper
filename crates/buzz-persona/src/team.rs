//! `beekeeper/team.yml`: the project's team manifest (spec § 4.2).
//!
//! The manifest names the project's roles and the advisory shape of the
//! agents that fill them. It is a *manifest*, not a gate: a role file under
//! `roles/` composes whether or not the manifest lists it, and nothing here
//! grants access, mints an identity or seats anyone — the host does those
//! (D11). What the manifest does decide:
//!
//! - the synthesized pack's `id` and `version` (`name`, `version`);
//! - which role is hired first (`lead`, D14);
//! - per role, an alternative `file`, advisory `runtime` and `model` that
//!   fill in when the role file's own frontmatter is silent, and
//!   `workspace.roles_visible` — whether a seat in that role may see the
//!   `beekeeper/` directory in its worktree (spec § 4.10);
//! - advisory agent names and lifetimes (`agents`), which `actions.yml`
//!   refers to by name (Part C).
//!
//! Unknown keys are refused, so a typo cannot silently mean the default.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::persona::is_valid_role_slug;

/// The manifest file name under the flat root.
pub const TEAM_YML: &str = "team.yml";

/// The one schema this parser reads.
pub const TEAM_SCHEMA: &str = "beekeeper-team/v1";

/// Largest manifest this parser reads, in bytes.
pub const MAX_TEAM_YML_BYTES: u64 = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum TeamError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid team manifest {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },
}

/// An agent's advisory lifetime (spec § 5.7). The host applies the
/// behaviour; the truth stays the execution fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentLifetime {
    /// One open umbrella per (agent, project) the host keeps alive.
    Persistent,
    /// A hire whose report lets the lead or host close its seat.
    Ephemeral,
}

/// Per-role workspace facts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TeamWorkspace {
    /// Whether a seat in this role may see the `beekeeper/` directory in
    /// its worktree. Default `false`: every seat's tree omits it (spec
    /// § 4.10). A role whose job is to author roles opts in.
    #[serde(default)]
    pub roles_visible: bool,
}

/// One role's manifest entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TeamRole {
    /// The role file, relative to the flat root. Defaults to
    /// `roles/<role>.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Advisory runtime (`claude`, `codex`, …); fills in when the role
    /// file's frontmatter names none. The router decides (D17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// Advisory `provider:model-id`; fills in when the frontmatter names
    /// none. The router decides (D17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub workspace: TeamWorkspace,
}

/// One advisory agent entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TeamAgent {
    /// The name `actions.yml` refers to; a suggestion for the identity the
    /// host mints, never the identity itself.
    pub name: String,
    /// The role this agent fills; must be one of `roles`.
    pub role: String,
    pub lifetime: AgentLifetime,
}

/// The parsed manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TeamManifest {
    pub schema: String,
    /// The synthesized pack id's tail: `project:<name>`. Defaults to the
    /// flat root's directory name when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Becomes the synthesized pack version.
    pub version: String,
    /// The role hired first (D14).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    #[serde(default)]
    pub roles: BTreeMap<String, TeamRole>,
    #[serde(default)]
    pub agents: Vec<TeamAgent>,
}

impl TeamManifest {
    /// The manifest's entry for `role`, or the default when it lists none.
    pub fn role(&self, role: &str) -> TeamRole {
        self.roles.get(role).cloned().unwrap_or_default()
    }

    /// The role file for `role`, relative to the flat root.
    pub fn role_file(&self, role: &str) -> String {
        self.role(role)
            .file
            .unwrap_or_else(|| format!("roles/{role}.md"))
    }

    /// The agents that fill `role`, in manifest order.
    pub fn agents_for(&self, role: &str) -> Vec<&TeamAgent> {
        self.agents.iter().filter(|a| a.role == role).collect()
    }
}

/// Parse manifest text.
///
/// # Errors
/// [`TeamError::Invalid`] naming the key or entry at fault: a schema this
/// parser does not read, an empty version, a role key or agent role that is
/// not a slug, an agent naming a role the manifest does not declare, a lead
/// the manifest does not declare, a `file` that is absolute or climbs out of
/// the root, or an unknown key anywhere.
pub fn parse_team_yml(content: &str, path: &Path) -> Result<TeamManifest, TeamError> {
    let invalid = |reason: String| TeamError::Invalid {
        path: path.to_path_buf(),
        reason,
    };
    let manifest: TeamManifest =
        serde_yaml::from_str(content).map_err(|error| invalid(error.to_string()))?;
    if manifest.schema != TEAM_SCHEMA {
        return Err(invalid(format!(
            "schema must be {TEAM_SCHEMA:?}, got {:?}",
            manifest.schema
        )));
    }
    if manifest.version.trim().is_empty() {
        return Err(invalid("version is required".to_string()));
    }
    if let Some(name) = manifest.name.as_deref() {
        if name.trim().is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        {
            return Err(invalid(format!(
                "name must be [A-Za-z0-9._-] and not empty, got {name:?}"
            )));
        }
    }
    for (role, entry) in &manifest.roles {
        if !is_valid_role_slug(role) {
            return Err(invalid(format!(
                "roles.{role}: a role is 1-64 bytes of [a-z0-9-]"
            )));
        }
        if let Some(file) = entry.file.as_deref() {
            let rel = Path::new(file);
            if file.trim().is_empty()
                || rel
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
            {
                return Err(invalid(format!(
                    "roles.{role}.file must be a relative path inside the team root, got {file:?}"
                )));
            }
        }
    }
    if let Some(lead) = manifest.lead.as_deref() {
        if !manifest.roles.contains_key(lead) {
            return Err(invalid(format!(
                "lead {lead:?} is not one of roles: {}",
                manifest
                    .roles
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }
    let mut names = std::collections::BTreeSet::new();
    for agent in &manifest.agents {
        if agent.name.trim().is_empty() {
            return Err(invalid("agents[].name is required".to_string()));
        }
        if !names.insert(agent.name.as_str()) {
            return Err(invalid(format!(
                "agents: the name {:?} appears twice",
                agent.name
            )));
        }
        if !manifest.roles.contains_key(&agent.role) {
            return Err(invalid(format!(
                "agents[{}]: role {:?} is not one of roles",
                agent.name, agent.role
            )));
        }
    }
    Ok(manifest)
}

/// Read `<root>/team.yml` when it exists.
///
/// `Ok(None)` when the root has no manifest — a flat root without one is
/// still a flat root, with defaults. `Err` when a manifest is there and
/// cannot be read or does not parse: a manifest that is present and wrong
/// is refused, never skipped.
pub fn load_team(root: &Path) -> Result<Option<TeamManifest>, TeamError> {
    let path = root.join(TEAM_YML);
    let size = match std::fs::metadata(&path) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(TeamError::Io { path, source }),
    };
    if size > MAX_TEAM_YML_BYTES {
        return Err(TeamError::Invalid {
            path,
            reason: format!("file too large: {size} bytes (max {MAX_TEAM_YML_BYTES})"),
        });
    }
    let content = std::fs::read_to_string(&path).map_err(|source| TeamError::Io {
        path: path.clone(),
        source,
    })?;
    parse_team_yml(&content, &path).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
schema: beekeeper-team/v1
name: tank-loop
version: 0.3.0
lead: project-manager
roles:
  project-manager:
    file: roles/project-manager.md
    runtime: claude
    model: anthropic:claude-sonnet-5
    workspace: { roles_visible: true }
  builder: {}
agents:
  - { name: Keystone, role: project-manager, lifetime: persistent }
  - { name: Levain, role: builder, lifetime: ephemeral }
"#;

    fn parse(text: &str) -> Result<TeamManifest, TeamError> {
        parse_team_yml(text, Path::new("beekeeper/team.yml"))
    }

    #[test]
    fn a_good_manifest_parses_with_defaults_where_it_is_silent() {
        let team = parse(GOOD).unwrap();
        assert_eq!(team.name.as_deref(), Some("tank-loop"));
        assert_eq!(team.version, "0.3.0");
        assert_eq!(team.lead.as_deref(), Some("project-manager"));
        assert!(team.role("project-manager").workspace.roles_visible);
        assert!(!team.role("builder").workspace.roles_visible);
        assert_eq!(team.role_file("builder"), "roles/builder.md");
        assert_eq!(
            team.role_file("project-manager"),
            "roles/project-manager.md"
        );
        assert_eq!(team.role("builder").runtime, None);
        assert_eq!(
            team.agents_for("project-manager")
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Keystone"]
        );
        assert_eq!(team.agents[1].lifetime, AgentLifetime::Ephemeral);
        // A role the manifest does not list is the default entry, not an error.
        assert_eq!(team.role("verifier"), TeamRole::default());
    }

    #[test]
    fn schema_version_lead_and_agent_roles_are_checked() {
        let wrong_schema = GOOD.replace("beekeeper-team/v1", "beekeeper-team/v9");
        assert!(parse(&wrong_schema)
            .unwrap_err()
            .to_string()
            .contains("schema"));

        let no_version = GOOD.replace("version: 0.3.0", "version: \"\"");
        assert!(parse(&no_version)
            .unwrap_err()
            .to_string()
            .contains("version"));

        let bad_lead = GOOD.replace("lead: project-manager", "lead: poker");
        assert!(parse(&bad_lead).unwrap_err().to_string().contains("lead"));

        let bad_agent_role = GOOD.replace(
            "role: builder, lifetime: ephemeral",
            "role: poker, lifetime: ephemeral",
        );
        let error = parse(&bad_agent_role).unwrap_err().to_string();
        assert!(
            error.contains("Levain") && error.contains("poker"),
            "{error}"
        );

        let dup = GOOD.replace("name: Levain", "name: Keystone");
        assert!(parse(&dup).unwrap_err().to_string().contains("twice"));
    }

    #[test]
    fn unknown_keys_bad_slugs_and_escaping_files_are_refused() {
        let unknown = GOOD.replace("builder: {}", "builder: { colour: blue }");
        assert!(parse(&unknown).is_err());
        let bad_slug = GOOD.replace("  builder: {}", "  Builder: {}");
        assert!(parse(&bad_slug)
            .unwrap_err()
            .to_string()
            .contains("[a-z0-9-]"));
        let escaping = GOOD.replace("file: roles/project-manager.md", "file: ../secrets.md");
        assert!(parse(&escaping)
            .unwrap_err()
            .to_string()
            .contains("inside the team root"));
        let absolute = GOOD.replace("file: roles/project-manager.md", "file: /etc/passwd");
        assert!(parse(&absolute).is_err());
        let bad_lifetime = GOOD.replace("lifetime: ephemeral", "lifetime: forever");
        assert!(parse(&bad_lifetime).is_err());
    }

    #[test]
    fn a_missing_manifest_is_none_and_a_broken_one_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_team(dir.path()).unwrap(), None);
        std::fs::write(dir.path().join(TEAM_YML), "schema: nope\n").unwrap();
        assert!(load_team(dir.path()).is_err());
        std::fs::write(dir.path().join(TEAM_YML), GOOD).unwrap();
        assert_eq!(load_team(dir.path()).unwrap().unwrap().version, "0.3.0");
    }
}
