//! Bounded, metadata-only inspection of the managed-agent store.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use tauri::{AppHandle, Manager};

const MAX_STORE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECORDS: usize = 4096;

/// Public metadata needed by team readiness. Secret fields are intentionally
/// absent, so deserialization cannot hydrate or retain them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedAgentReadinessMetadata {
    pub pubkey: String,
    pub name: String,
    pub home_role: Option<String>,
    pub persona_team_dir: Option<PathBuf>,
    pub persona_name_in_team: Option<String>,
    pub persona_source_version: Option<String>,
    pub auth_tag_present: bool,
    pub auth_tag_owner: Option<String>,
    pub auth_tag_invalid: bool,
    pub auth_tag_owner_mismatch: bool,
}

#[derive(Deserialize)]
struct ManagedAgentReadinessWireMetadata {
    pubkey: String,
    name: String,
    #[serde(default)]
    home_role: Option<String>,
    #[serde(default)]
    persona_team_dir: Option<PathBuf>,
    #[serde(default)]
    persona_name_in_team: Option<String>,
    #[serde(default)]
    persona_source_version: Option<String>,
    #[serde(default)]
    auth_tag: Option<String>,
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn safe_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate(row: &ManagedAgentReadinessMetadata) -> Result<(), String> {
    if !lowercase_hex(&row.pubkey, 64) {
        return Err("managed-agent pubkey must be 64-character lowercase hex".into());
    }
    if row.name.trim().is_empty() || row.name.len() > 256 {
        return Err("managed-agent name is invalid".into());
    }
    if let Some(role) = &row.home_role {
        if !safe_slug(role) {
            return Err("managed-agent homeRole is invalid".into());
        }
    }
    if let Some(name) = &row.persona_name_in_team {
        if name.trim().is_empty() || name.len() > 256 {
            return Err("managed-agent personaNameInTeam is invalid".into());
        }
    }
    if let Some(path) = &row.persona_team_dir {
        if !path.is_absolute() {
            return Err("managed-agent personaTeamDir must be absolute".into());
        }
    }
    if row.persona_team_dir.is_some() != row.persona_name_in_team.is_some() {
        return Err("managed-agent persona source coordinates are incomplete".into());
    }
    if let Some(source) = &row.persona_source_version {
        if !lowercase_hex(source, 64) {
            return Err("managed-agent personaSourceVersion is invalid".into());
        }
    }
    Ok(())
}

pub(crate) fn managed_agents_store_path_readonly(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?
        .join("agents")
        .join("managed-agents.json"))
}

pub(crate) fn load_managed_agent_readiness_metadata_from(
    path: &Path,
    expected_owner: Option<&str>,
) -> Result<Vec<ManagedAgentReadinessMetadata>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("failed to read agent store: {error}")),
    };
    let length = file
        .metadata()
        .map_err(|error| format!("failed to inspect agent store: {error}"))?
        .len();
    if length > MAX_STORE_BYTES {
        return Err("managed-agent metadata store exceeds readiness limit".into());
    }
    let wire: Vec<ManagedAgentReadinessWireMetadata> = serde_json::from_reader(file)
        .map_err(|error| format!("failed to parse agent store: {error}"))?;
    if wire.len() > MAX_RECORDS {
        return Err("managed-agent metadata store has too many records".into());
    }
    let mut rows = Vec::with_capacity(wire.len());
    for row in wire {
        let auth = crate::readiness_auth::inspect_auth_tag(
            row.auth_tag.as_deref(),
            &row.pubkey,
            expected_owner,
        );
        let row = ManagedAgentReadinessMetadata {
            pubkey: row.pubkey,
            name: row.name,
            home_role: row.home_role,
            persona_team_dir: row.persona_team_dir,
            persona_name_in_team: row.persona_name_in_team,
            persona_source_version: row.persona_source_version,
            auth_tag_present: auth.present,
            auth_tag_owner: auth.verified_owner,
            auth_tag_invalid: auth.invalid,
            auth_tag_owner_mismatch: auth.owner_mismatch,
        };
        validate(&row)?;
        rows.push(row);
    }
    Ok(rows)
}

/// Inspect public metadata without directory creation, backup, migration,
/// locking, keychain access, or secret hydration.
pub(crate) fn load_managed_agent_readiness_metadata(
    app: &AppHandle,
    expected_owner: Option<&str>,
) -> Result<Vec<ManagedAgentReadinessMetadata>, String> {
    load_managed_agent_readiness_metadata_from(
        &managed_agents_store_path_readonly(app)?,
        expected_owner,
    )
}
