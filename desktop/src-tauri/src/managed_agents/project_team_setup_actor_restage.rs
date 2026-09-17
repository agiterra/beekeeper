//! Setup-specific restart resolution before any project-source lookup.

use super::{identity, invalid, verified_pack, PreparedSetupActor, SetupError};
use crate::app_state::AppState;
use crate::managed_agents::{
    actor_seats::{SeatPackOrigin, SeatPackPreview},
    packs_cache::PackRef,
    ManagedAgentRecord,
};
use std::path::Path;
use tauri::AppHandle;

/// Reserved setup definitions require their original scoped bootstrap.
pub(crate) fn is_setup_actor(record: &ManagedAgentRecord) -> bool {
    record
        .persona_id
        .as_deref()
        .is_some_and(|id| id.starts_with("project-team-setup:"))
}

/// Generic staging has no authenticated setup/project/session binding.
pub(crate) const SCOPED_STAGE_REQUIRED: &str = "This setup session needs its preserved bootstrap. Open project team setup to check the saved authoring request; starting a replacement would lose its context.";

pub(super) fn preview(
    storage: &Path,
    receipt: &identity::Receipt,
    record: &ManagedAgentRecord,
    role: &str,
    original: Option<&PackRef>,
) -> Result<SeatPackPreview, SetupError> {
    if role != super::pack::ROLE || original != Some(&receipt.pack_ref) {
        return Err(invalid(
            "The setup recovery request does not name its original bootstrap role and revision.",
        ));
    }
    let path = verified_pack(storage, receipt)?;
    if record.pubkey != receipt.pubkey {
        return Err(invalid(
            "The setup recovery request names another identity.",
        ));
    }
    receipt.reconcile(&mut vec![record.clone()], &path)?;
    Ok(SeatPackPreview {
        pack_staged: true,
        origin: SeatPackOrigin::Shipped,
        role: Some(role.into()),
        pack_dir: Some(path.to_string_lossy().into_owned()),
        persona_id: Some(super::pack::ROLE.into()),
        pack_ref: Some(receipt.pack_ref.clone()),
        refusal: None,
        reason: None,
        warnings: Vec::new(),
        compose_digest: None,
    })
}

/// Resolve only an authenticated preserved setup bootstrap, without minting or fetching packs.
pub(crate) fn restage_plan(
    app: &AppHandle,
    state: &AppState,
    record: &ManagedAgentRecord,
    project_ref: Option<&str>,
    role: &str,
    original: Option<&PackRef>,
) -> Result<SeatPackPreview, String> {
    let project_ref = project_ref.ok_or_else(|| SCOPED_STAGE_REQUIRED.to_string())?;
    let owner = state.signing_keys()?;
    let relay = crate::relay::relay_ws_url_with_override(state);
    let (root, scope) =
        super::super::context(app, state, project_ref, &relay).map_err(|e| e.message)?;
    let draft = super::super::read_draft(&root, &scope)
        .map_err(|e| e.message)?
        .ok_or_else(|| SCOPED_STAGE_REQUIRED.to_string())?;
    if record.persona_id.as_deref() != Some(identity::definition_id(&draft).as_str()) {
        return Err("The requested setup identity does not belong to this project draft.".into());
    }
    let storage = super::storage_root(app, state, &draft, &owner).map_err(|e| e.message)?;
    let _lock = super::lock(&storage).map_err(|e| e.message)?;
    let receipt = identity::read(&storage.join("setup-actor.enc"), &draft, &owner)
        .map_err(|e| e.message)?
        .ok_or_else(|| SCOPED_STAGE_REQUIRED.to_string())?;
    let PreparedSetupActor {
        authoring_directory,
        ..
    } = receipt.prepared(&storage);
    super::tree::ensure_contained_directory(&storage, Path::new(&authoring_directory))
        .map_err(|e| e.message)?;
    super::ensure_brief(Path::new(&authoring_directory), &draft).map_err(|e| e.message)?;
    preview(&storage, &receipt, record, role, original).map_err(|e| e.message)
}
