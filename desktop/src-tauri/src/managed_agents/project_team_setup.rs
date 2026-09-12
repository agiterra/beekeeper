//! Local project setup drafts. Preparing and validating never publishes,
//! installs an identity, or asserts that a project is configured.

use crate::app_state::AppState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

#[path = "project_team_setup_snapshot.rs"]
mod snapshot;
#[cfg(test)]
#[path = "project_team_setup_tests.rs"]
mod tests;
#[path = "project_team_setup_tree.rs"]
mod tree;
pub use snapshot::ProjectTeamSetupSnapshot;
#[path = "project_team_setup_authoring.rs"]
pub(crate) mod authoring;

static PREPARE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
const MAX_INTENT_BYTES: usize = 16 * 1024;

/// A machine-readable failure with copy suitable for the setup workbench.
#[derive(Debug, Serialize)]
pub struct SetupError {
    pub code: &'static str,
    pub message: String,
}

impl SetupError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<std::io::Error> for SetupError {
    fn from(error: std::io::Error) -> Self {
        Self::new("filesystem", error.to_string())
    }
}

/// Only a draft exists at this milestone; validation does not advance it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Draft,
}

/// Role identity fixed by the seed, independently of its editable procedures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetupRoleIdentity {
    pub role: String,
    pub persona_name: String,
    pub pack_id: String,
}

/// Host-local draft metadata. None of these paths are published to the relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamSetupDraft {
    pub setup_id: String,
    pub project_ref: String,
    pub project_directory: String,
    pub draft_directory: String,
    pub roles_directory: String,
    pub status: SetupStatus,
    pub intent: String,
    pub owner_pubkey: String,
    pub relay_url: String,
    pub roles: Vec<String>,
    pub expected_roles: Vec<SetupRoleIdentity>,
    pub created_at: String,
    /// Last saved candidate ID; bytes must be reverified before using it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_snapshot_id: Option<String>,
}

/// One validation diagnostic, with warnings distinct from rejection.
#[derive(Debug, Serialize)]
pub struct SetupDiagnostic {
    pub level: &'static str,
    pub message: String,
}

/// Validation of local bytes, not publication or execution adoption evidence.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamSetupValidation {
    pub setup_id: String,
    pub status: SetupStatus,
    pub valid: bool,
    pub roles: Vec<String>,
    pub diagnostics: Vec<SetupDiagnostic>,
}

struct SetupScope {
    project: String,
    owner: String,
    relay: String,
}

impl SetupScope {
    fn new(project: &str, owner: &str, relay: &str) -> Result<Self, SetupError> {
        let project = buzz_core_pkg::kind::normalize_project_coordinate(project.trim())
            .ok_or_else(|| {
                SetupError::new(
                    "invalid_input",
                    "Expected a project coordinate 30621:<owner>:<slug>",
                )
            })?;
        let owner = nostr::PublicKey::parse(owner)
            .map_err(|error| SetupError::new("invalid_input", error.to_string()))?
            .to_hex();
        Ok(Self {
            project,
            owner,
            relay: crate::session_provider::canonical_relay_key(relay),
        })
    }

    fn directory(&self, root: &Path) -> PathBuf {
        let key = format!("{}\n{}\n{}", self.relay, self.owner, self.project);
        root.join(hex::encode(Sha256::digest(key.as_bytes())))
    }
}

fn context(
    app: &AppHandle,
    state: &AppState,
    project: &str,
    expected: &str,
) -> Result<(PathBuf, SetupScope), SetupError> {
    let relay = crate::session_provider::commands::provider_command_relay_for_active(
        &crate::relay::relay_ws_url_with_override(state),
        Some(expected),
    )
    .map_err(|message| SetupError::new("scope_changed", message))?;
    let owner = state
        .signing_keys()
        .map_err(|message| SetupError::new("scope_changed", message))?
        .public_key()
        .to_hex();
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| SetupError::new("filesystem", error.to_string()))?
        .join("project-team-setup");
    Ok((root, SetupScope::new(project, &owner, &relay)?))
}

fn verify_context(state: &AppState, scope: &SetupScope) -> Result<(), SetupError> {
    let owner = state
        .signing_keys()
        .map_err(|message| SetupError::new("scope_changed", message))?
        .public_key()
        .to_hex();
    if owner != scope.owner
        || crate::session_provider::canonical_relay_key(&crate::relay::relay_ws_url_with_override(
            state,
        )) != scope.relay
    {
        return Err(SetupError::new(
            "scope_changed",
            "The active identity or community changed; reopen project setup.",
        ));
    }
    Ok(())
}

fn read_draft(
    root: &Path,
    scope: &SetupScope,
) -> Result<Option<ProjectTeamSetupDraft>, SetupError> {
    if !root.try_exists()? {
        return Ok(None);
    }
    tree::ensure_contained_directory(root, root)?;
    let root = std::fs::canonicalize(root)?;
    let directory = scope.directory(&root);
    if !directory.try_exists()? {
        return Ok(None);
    }
    tree::ensure_contained_directory(&root, &directory)?;
    let path = directory.join("record.json");
    tree::check_regular_file(&path, 64 * 1024)?;
    let mut record: ProjectTeamSetupDraft =
        serde_json::from_slice(&std::fs::read(path)?).map_err(|error| {
            SetupError::new(
                "invalid_draft",
                format!("Could not read the preserved draft record: {error}"),
            )
        })?;
    if record.project_ref != scope.project
        || record.owner_pubkey != scope.owner
        || record.relay_url != scope.relay
        || record.draft_directory != directory.join("draft").to_string_lossy()
        || record.roles_directory != directory.join("draft/personas/roles").to_string_lossy()
        || uuid::Uuid::parse_str(&record.setup_id).is_err()
        || record.roles
            != record
                .expected_roles
                .iter()
                .map(|r| r.role.clone())
                .collect::<Vec<_>>()
    {
        return Err(SetupError::new(
            "invalid_draft",
            "The saved draft does not match this project, identity and community.",
        ));
    }
    tree::ensure_contained_directory(&directory, Path::new(&record.draft_directory))?;
    record.latest_snapshot_id = snapshot::latest(&record)?;
    Ok(Some(record))
}

fn prepare(
    root: &Path,
    scope: &SetupScope,
    intent: &str,
    project_directory: &Path,
    seed: &Path,
) -> Result<ProjectTeamSetupDraft, SetupError> {
    let intent = intent.trim();
    if intent.is_empty() || intent.len() > MAX_INTENT_BYTES {
        return Err(SetupError::new(
            "invalid_input",
            "Describe the project in 1–16384 bytes.",
        ));
    }
    let project = tree::git_directory(project_directory)?;
    let _guard = PREPARE_LOCK
        .lock()
        .map_err(|error| SetupError::new("filesystem", error.to_string()))?;
    if let Some(existing) = read_draft(root, scope)? {
        if existing.intent != intent || Path::new(&existing.project_directory) != project {
            return Err(SetupError::new("existing_draft", "This project already has a setup draft with a different intent or folder. Its files were preserved; reopen that draft."));
        }
        return Ok(existing);
    }
    let expected_roles = tree::seed_identities(seed)?;
    tree::create_private_directory(root)?;
    let root = std::fs::canonicalize(root)?;
    let destination = scope.directory(&root);
    let setup_id = uuid::Uuid::new_v4().to_string();
    let temporary = root.join(format!(".preparing-{setup_id}"));
    tree::create_private_directory(&temporary)?;
    let roles = temporary.join("draft/personas/roles");
    tree::copy_tree(seed, &roles)?;
    let record = ProjectTeamSetupDraft {
        setup_id,
        project_ref: scope.project.clone(),
        project_directory: project.to_string_lossy().into_owned(),
        draft_directory: destination.join("draft").to_string_lossy().into_owned(),
        roles_directory: destination
            .join("draft/personas/roles")
            .to_string_lossy()
            .into_owned(),
        status: SetupStatus::Draft,
        intent: intent.to_string(),
        owner_pubkey: scope.owner.clone(),
        relay_url: scope.relay.clone(),
        roles: expected_roles.iter().map(|r| r.role.clone()).collect(),
        expected_roles,
        created_at: crate::util::now_iso(),
        latest_snapshot_id: None,
    };
    let bytes = serde_json::to_vec_pretty(&record)
        .map_err(|error| SetupError::new("filesystem", error.to_string()))?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.join("record.json"))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(temporary, destination)?;
    Ok(record)
}

/// Read this identity's project draft without creating files or starting work.
#[tauri::command]
pub async fn project_team_setup_get(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    expected_relay_url: String,
) -> Result<Option<ProjectTeamSetupDraft>, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    read_draft(&root, &scope)
}

/// Prepare a durable local copy of the neutral role packs for project adaptation.
#[tauri::command]
pub async fn project_team_setup_prepare(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    intent: String,
    project_directory: String,
    expected_relay_url: String,
) -> Result<ProjectTeamSetupDraft, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let seed = super::packs_cache::shipped_packs_dir(&app).ok_or_else(|| {
        SetupError::new(
            "filesystem",
            "This build does not contain the project role baseline.",
        )
    })?;
    let result = tokio::task::spawn_blocking(move || {
        prepare(&root, &scope, &intent, Path::new(&project_directory), &seed)
    })
    .await
    .map_err(|error| SetupError::new("filesystem", error.to_string()))??;
    let scope = SetupScope::new(&result.project_ref, &result.owner_pubkey, &result.relay_url)?;
    verify_context(&state, &scope)?;
    Ok(result)
}

/// Validate the exact saved draft's local tree, retaining its draft status.
#[tauri::command]
pub async fn project_team_setup_validate(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
) -> Result<ProjectTeamSetupValidation, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let record = read_draft(&root, &scope)?
        .ok_or_else(|| SetupError::new("invalid_draft", "Prepare a project draft first."))?;
    if setup_id != record.setup_id {
        return Err(SetupError::new(
            "invalid_draft",
            "The setup ID does not match this project's saved draft.",
        ));
    }
    let result = tokio::task::spawn_blocking(move || tree::validate(&record))
        .await
        .map_err(|error| SetupError::new("filesystem", error.to_string()))?;
    verify_context(&state, &scope)?;
    Ok(result)
}

/// Capture or reverify a content-addressed candidate without publishing it.
/// Stored files are not a security boundary; publication must reverify them.
#[tauri::command]
pub async fn project_team_setup_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
    snapshot_id: Option<String>,
) -> Result<ProjectTeamSetupSnapshot, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let expected = SetupScope::new(&scope.project, &scope.owner, &scope.relay)?;
    let result = tokio::task::spawn_blocking(move || {
        snapshot::run(&root, &scope, &setup_id, snapshot_id.as_deref())
    })
    .await
    .map_err(|error| SetupError::new("filesystem", error.to_string()))??;
    verify_context(&state, &expected)?;
    Ok(result)
}
