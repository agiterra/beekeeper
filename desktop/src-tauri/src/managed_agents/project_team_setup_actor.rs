//! Standalone setup identity and exact shipped bootstrap custody. No relay writes.

use super::{tree, ProjectTeamSetupDraft, SetupError};
use crate::app_state::AppState;
use crate::managed_agents::{actor_seats, packs_cache, storage};
use nostr::Keys;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[path = "project_team_setup_actor_identity.rs"]
mod identity;
#[path = "project_team_setup_actor_pack.rs"]
mod pack;
#[path = "project_team_setup_actor_restage.rs"]
pub(crate) mod restage;
#[cfg(test)]
#[path = "project_team_setup_actor_tests.rs"]
mod tests;

/// Exact actor/bootstrap binding to persist in the authoring launch journal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedSetupActor {
    pub actor_pubkey: String,
    pub pack_ref: packs_cache::PackRef,
    pub pack_digest: String,
    pub authoring_directory: String,
}

fn invalid(message: impl Into<String>) -> SetupError {
    SetupError::new("invalid_setup_actor", message)
}

fn storage_root(
    app: &AppHandle,
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    owner: &Keys,
) -> Result<PathBuf, SetupError> {
    let (root, scope) = super::context(app, state, &draft.project_ref, &draft.relay_url)?;
    let actual = super::read_draft(&root, &scope)?
        .ok_or_else(|| invalid("Prepare the setup draft first."))?;
    if owner.public_key().to_hex() != draft.owner_pubkey
        || actual.setup_id != draft.setup_id
        || actual.draft_directory != draft.draft_directory
        || actual.project_directory != draft.project_directory
    {
        return Err(invalid(
            "The setup draft or signing owner changed before actor preparation.",
        ));
    }
    let storage = scope.directory(&root);
    tree::ensure_contained_directory(&root, &storage)?;
    Ok(storage)
}

fn lock(storage: &Path) -> Result<std::fs::File, SetupError> {
    let path = storage.join("setup-actor.lock");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => tree::check_regular_file(&path, 0)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(path)?;
    lock.lock()?;
    Ok(lock)
}

fn verified_pack(storage: &Path, receipt: &identity::Receipt) -> Result<PathBuf, SetupError> {
    let path = storage.join("bootstrap").join(&receipt.directory_id);
    tree::ensure_contained_directory(storage, &path)?;
    if pack::digest(&path)? != receipt.pack_digest {
        return Err(invalid("The preserved setup bootstrap bytes changed."));
    }
    pack::validate(&path)?;
    if pack::digest(&path)? != receipt.pack_digest {
        return Err(invalid("The setup bootstrap changed during validation."));
    }
    Ok(path)
}

fn ensure_brief(directory: &Path, draft: &ProjectTeamSetupDraft) -> Result<(), SetupError> {
    let path = directory.join("PROJECT_TEAM_SETUP.md");
    let expected = super::authoring::authoring_prompt(draft)?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            pack::write_new(&path, expected.as_bytes())?;
            pack::sync(directory)?;
        }
        Err(e) => return Err(e.into()),
        Ok(_) => {
            if pack::read(&path, 32 * 1024)? != expected.as_bytes() {
                return Err(invalid(
                    "The preserved setup authoring brief changed; it was not overwritten.",
                ));
            }
        }
    }
    Ok(())
}

fn prepare_receipt(
    storage: &Path,
    draft: &ProjectTeamSetupDraft,
    owner: &Keys,
    shipped: Option<&Path>,
    version: &str,
    agents: &[crate::managed_agents::ManagedAgentRecord],
) -> Result<identity::Receipt, SetupError> {
    let path = storage.join("setup-actor.enc");
    if let Some(receipt) = identity::read(&path, draft, owner)? {
        verified_pack(storage, &receipt)?;
        return Ok(receipt);
    }
    if agents
        .iter()
        .any(|agent| agent.persona_id.as_deref() == Some(&identity::definition_id(draft)))
    {
        return Err(invalid("A setup identity exists without its bootstrap recovery receipt; refusing to replace it."));
    }
    let shipped =
        shipped.ok_or_else(|| invalid("This build has no shipped project-setup pack."))?;
    let source = shipped.join(pack::ROLE);
    let bootstrap = storage.join("bootstrap");
    tree::create_private_directory(&bootstrap)?;
    tree::ensure_contained_directory(storage, &bootstrap)?;
    let directory_id = uuid::Uuid::new_v4().to_string();
    let destination = bootstrap.join(&directory_id);
    pack::copy(&source, &destination)?;
    let persona = pack::validate(&destination)?;
    let digest = pack::digest(&destination)?;
    let pack_ref =
        packs_cache::shipped_pack_ref_for_dir(Some(shipped), &destination, pack::ROLE, version)
            .ok_or_else(|| {
                invalid("The captured bootstrap is not the exact shipped setup pack.")
            })?;
    if pack::digest(&destination)? != digest {
        return Err(invalid(
            "The captured setup bootstrap changed during provenance verification.",
        ));
    }
    pack::sync(&bootstrap)?;
    let receipt = identity::mint(draft, owner, directory_id, pack_ref, digest, persona)?;
    receipt.validate(draft, owner)?;
    identity::persist(&path, &receipt, owner)?;
    Ok(receipt)
}

/// Prepare or reconcile one standalone identity with its pinned shipped bootstrap.
/// Persists owner-encrypted recovery before touching the managed-agent/keyring store.
pub(crate) fn prepare_actor(
    app: &AppHandle,
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    owner: &Keys,
) -> Result<PreparedSetupActor, SetupError> {
    let storage_dir = storage_root(app, state, draft, owner)?;
    let _lock = lock(&storage_dir)?;
    let _store = state
        .managed_agents_store_lock
        .lock()
        .map_err(|_| invalid("Managed-agent storage lock is unavailable."))?;
    let mut agents = storage::load_managed_agents(app).map_err(invalid)?;
    let receipt = prepare_receipt(
        &storage_dir,
        draft,
        owner,
        packs_cache::shipped_packs_dir(app).as_deref(),
        &packs_cache::shipped_packs_version(app),
        &agents,
    )?;
    let directory = verified_pack(&storage_dir, &receipt)?;
    receipt.reconcile(&mut agents, &directory)?;
    let mut definitions = storage::load_agent_definitions(app).map_err(invalid)?;
    if let Some(existing) = definitions
        .iter()
        .find(|record| record.slug.as_deref() == Some(&receipt.definition.id))
    {
        let expected = receipt.definition.clone().into_agent_record();
        if existing.system_prompt != expected.system_prompt
            || existing.source_team.is_some()
            || !existing.is_active
        {
            return Err(invalid(
                "The setup definition was changed outside this assignment.",
            ));
        }
    } else {
        definitions.push(receipt.definition.clone().into_agent_record());
        storage::save_agent_definitions(app, &definitions).map_err(invalid)?;
    }
    storage::save_managed_agents(app, &agents).map_err(invalid)?;
    let prepared = receipt.prepared(&storage_dir);
    tree::create_private_directory(Path::new(&prepared.authoring_directory))?;
    tree::ensure_contained_directory(&storage_dir, Path::new(&prepared.authoring_directory))?;
    ensure_brief(Path::new(&prepared.authoring_directory), draft)?;
    pack::sync(&storage_dir)?;
    Ok(prepared)
}

fn stage_at(
    path: &Path,
    command_id: &str,
    entry: actor_seats::ActorSeatEntry,
) -> Result<(), SetupError> {
    if !command_id
        .strip_prefix("csl-")
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id))
    {
        return Err(invalid(
            "Setup staging requires the reserved csl-UUID command ID.",
        ));
    }
    actor_seats::mutate_actor_seats_file(path, |file| {
        if file
            .pending
            .get(command_id)
            .is_some_and(|existing| existing != &entry)
        {
            return Err("Another actor binding is already staged for this command.".into());
        }
        actor_seats::stage_actor_seat(file, command_id, entry)
    })
    .map_err(invalid)
}

/// Stage only the preserved bootstrap, never the project's evolving pack source.
/// Caller must authenticate command/session/provider binding before invoking this helper.
pub(crate) fn stage_actor(
    app: &AppHandle,
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    owner: &Keys,
    command_id: &str,
    expected: &PreparedSetupActor,
) -> Result<actor_seats::StagedActorSeat, SetupError> {
    let storage_dir = storage_root(app, state, draft, owner)?;
    let _lock = lock(&storage_dir)?;
    let receipt = identity::read(&storage_dir.join("setup-actor.enc"), draft, owner)?
        .ok_or_else(|| invalid("The setup actor has not been prepared."))?;
    if receipt.prepared(&storage_dir) != *expected {
        return Err(invalid("The setup actor or bootstrap binding changed."));
    }
    tree::ensure_contained_directory(&storage_dir, Path::new(&expected.authoring_directory))?;
    ensure_brief(Path::new(&expected.authoring_directory), draft)?;
    let directory = verified_pack(&storage_dir, &receipt)?;
    let _store = state
        .managed_agents_store_lock
        .lock()
        .map_err(|_| invalid("Managed-agent storage lock is unavailable."))?;
    let mut agents = storage::load_managed_agents(app).map_err(invalid)?;
    // Validate against existing custody; staging must not silently reinstall a deleted identity.
    if !agents.iter().any(|record| record.pubkey == receipt.pubkey) {
        return Err(invalid("The setup identity is no longer installed."));
    }
    receipt.reconcile(&mut agents, &directory)?;
    let record = agents
        .iter()
        .find(|record| record.pubkey == receipt.pubkey)
        .ok_or_else(|| invalid("The setup identity disappeared."))?;
    let entry = actor_seats::build_actor_seat_entry(
        &record.pubkey,
        &record.private_key_nsec,
        record.auth_tag.as_deref(),
        &draft.relay_url,
        Some(&record.name),
        Some((directory, pack::ROLE.into())),
        Some(receipt.pack_ref.clone()),
    )
    .map_err(invalid)?;
    let providers = crate::session_provider::store::load_provider_store(app).map_err(invalid)?;
    let provider = providers
        .get(&draft.relay_url)
        .ok_or_else(|| invalid("No local session provider is provisioned for this community."))?;
    let state_dir = crate::session_provider::provider_state_dir(app, &provider.provider_pubkey)
        .map_err(invalid)?;
    stage_at(
        &actor_seats::actor_seats_path(&state_dir),
        command_id,
        entry,
    )?;
    Ok(actor_seats::StagedActorSeat {
        pack_staged: true,
        pack_ref: Some(receipt.pack_ref),
    })
}
