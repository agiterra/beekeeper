//! Team records and their command request shapes.
//!
//! Split out of `types.rs` when the crew block (plan D8) arrived: the parent
//! module sat one line under the repository's 1000-line ceiling, and the
//! answer to that ceiling is a split, never a higher limit.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use crate::managed_agents::team_events::TeamCrew;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Runtime-layered instructions shared by every member deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub persona_ids: Vec<String>,
    /// Crew composition (plan D8): the ordered seats a crew launch creates and
    /// the one it addresses first. `None` is an ordinary team, not a crew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crew: Option<TeamCrew>,
    #[serde(default)]
    pub is_builtin: bool,
    /// Absolute path to the team's backing directory (if directory-backed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_dir: Option<PathBuf>,
    /// Whether `source_dir` is a symlink to an external directory.
    #[serde(default)]
    pub is_symlink: bool,
    /// Resolved symlink target path (for display). Only set when `is_symlink` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
    /// Version from the team's `plugin.json` manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTeamRequest {
    pub name: String,
    pub description: Option<String>,
    pub instructions: Option<String>,
    #[serde(default)]
    pub persona_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTeamRequest {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub instructions: Option<String>,
    #[serde(default)]
    pub persona_ids: Vec<String>,
}
