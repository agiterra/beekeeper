//! Protected import evidence and affirmative existing admission provenance.
use crate::{Context, Error};
use beekeeper_host_core::{
    atomic_write, config::HostConfig, layout, ownership::Transfer, record::canonical_relay_key,
};
use nostr::{Event, PublicKey};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[path = "import_evidence_publication.rs"]
mod publication;

fn read(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::Refused("import evidence unavailable".into())),
    };
    if metadata.len() > 4 * 1024 * 1024 {
        return Err(Error::Refused(
            "import evidence exceeds protected capture limit".into(),
        ));
    }
    if !metadata.file_type().is_file() {
        return Err(Error::Refused(
            "import evidence must be a regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(Error::Refused(
                "import evidence must be owner restricted".into(),
            ));
        }
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|_| Error::Refused("import evidence unreadable".into()))
}
fn evidence_path(context: &Context, operation: Uuid) -> PathBuf {
    layout::host_dir(&context.home, context.instance)
        .join("management/import-definition-evidence")
        .join(format!("{operation}.json"))
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    schema: String,
    operation_id: String,
    actor_pubkey: String,
    relay_url: String,
    legacy_nonce: String,
    definition_event_id: String,
    definition: Event,
}
fn validate(expected: &Transfer, owner: &PublicKey, event: &Event) -> Result<(), Error> {
    Uuid::parse_str(&expected.operation_id)
        .map_err(|_| Error::Usage("invalid import operation".into()))?;
    crate::validate_hex64(&expected.actor_pubkey)?;
    if expected.legacy_nonce.is_empty()
        || expected.legacy_nonce.starts_with("fresh-identity:")
        || event.kind.as_u16() != 30177
        || event.pubkey != *owner
        || event.verify().is_err()
        || event.id.to_hex() != expected.definition_event_id
        || !event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["d", expected.actor_pubkey.as_str()])
    {
        return Err(Error::Refused(
            "original definition evidence does not match signed import scope".into(),
        ));
    }
    Ok(())
}
/// Retain exact owner-signed original import definition in exclusive restricted custody.
/// This writes evidence only: it cannot revise ownership, clear an admission intent,
/// signal a process, or establish a completed import. Prepared and quiesced capture
/// are supported; identical existing evidence remains readable across a later CAS.
pub fn supply_import_definition_evidence(
    context: &Context,
    expected: &Transfer,
    owner: &PublicKey,
    event: &Event,
) -> Result<(), Error> {
    validate(expected, owner, event)?;
    let operation = Uuid::parse_str(&expected.operation_id)
        .map_err(|_| Error::Usage("invalid import operation".into()))?;
    let value = Evidence {
        schema: "beekeeper-import-definition-evidence/v1".into(),
        operation_id: expected.operation_id.clone(),
        actor_pubkey: expected.actor_pubkey.clone(),
        relay_url: canonical_relay_key(&expected.relay_url),
        legacy_nonce: expected.legacy_nonce.clone(),
        definition_event_id: expected.definition_event_id.clone(),
        definition: event.clone(),
    };
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| Error::Other("cannot encode import evidence".into()))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(Error::Usage(
            "signed import evidence exceeds protected capture limit".into(),
        ));
    }
    let path = evidence_path(context, operation);
    if let Some(old) = read(&path)? {
        if old == bytes {
            return publication::sync_parent(&path);
        }
        return Err(Error::Conflict(
            "original import definition evidence is immutable".into(),
        ));
    }
    let held = beekeeper_host_core::ownership::for_pair(
        &context.home,
        context.instance,
        &expected.actor_pubkey,
        &expected.relay_url,
    )
    .map_err(Error::Refused)?
    .ok_or_else(|| Error::Refused("staged import ownership is unavailable".into()))?;
    if !matches!(
        held.phase,
        beekeeper_host_core::ownership::Phase::Prepared
            | beekeeper_host_core::ownership::Phase::Quiesced
    ) || held.target.is_some()
        || held.operation_id != expected.operation_id
        || held.legacy_nonce != expected.legacy_nonce
        || held.definition_event_id != expected.definition_event_id
    {
        return Err(Error::Refused(
            "original evidence must match the preserved unadmitted import".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other("import evidence directory missing".into()))?;
    atomic_write::create_dir_all_restricted(parent).map_err(Error::Other)?;
    publication::publish(&path, &bytes)?;
    Ok(())
}
/// Read and verify the immutable original signed definition for this import.
/// The original scope remains valid after exact B5 CAS while canonical revision
/// is newer; this reader grants no permission to bypass the admission guard/CAS.
pub fn read_import_definition_evidence(
    context: &Context,
    expected: &Transfer,
    owner: &PublicKey,
) -> Result<Option<Event>, Error> {
    let operation = Uuid::parse_str(&expected.operation_id)
        .map_err(|_| Error::Usage("invalid import operation".into()))?;
    let Some(bytes) = read(&evidence_path(context, operation))? else {
        return Ok(None);
    };
    let value: Evidence = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Refused("original import evidence malformed".into()))?;
    if value.schema != "beekeeper-import-definition-evidence/v1"
        || value.operation_id != expected.operation_id
        || value.actor_pubkey != expected.actor_pubkey
        || value.relay_url != canonical_relay_key(&expected.relay_url)
        || value.legacy_nonce != expected.legacy_nonce
        || value.definition_event_id != expected.definition_event_id
    {
        return Err(Error::Conflict(
            "original import evidence scope conflicts".into(),
        ));
    }
    validate(expected, owner, &value.definition)?;
    Ok(Some(value.definition))
}
/// Establish no durable create/admission provenance before a definition CAS.
/// Caller must hold management then the same actor/community admission lock
/// throughout this readback, owner-head/B5 proof and the ownership CAS. It refuses
/// any retained same-operation journal, actor create request, protected provider
/// lifecycle reservation (including BeforeEffects), consumed/answered operation,
/// or actor enrollment. Target absence alone never satisfies this guard.
pub fn require_unadmitted_import(
    context: &Context,
    operation: Uuid,
    actor: &str,
    relay: &str,
) -> Result<(), Error> {
    let actor_key = PublicKey::from_hex(actor)
        .map_err(|_| Error::Usage("invalid import actor".into()))?
        .to_hex();
    let actor = actor_key.as_str();
    let config = HostConfig::load(&layout::host_config_path(&context.home, context.instance))
        .map_err(Error::Refused)?
        .ok_or_else(|| {
            Error::Refused("commissioned provider state required for import CAS".into())
        })?;
    if config.instance != context.instance
        || canonical_relay_key(&config.relay_url) != canonical_relay_key(relay)
    {
        return Err(Error::Refused(
            "import commissioned provider scope differs".into(),
        ));
    }
    let dir = layout::host_dir(&context.home, context.instance).join("management/operations");
    let rows = match std::fs::read_dir(&dir) {
        Ok(rows) => Some(rows),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return Err(Error::Refused(
                "client operation journal unavailable".into(),
            ))
        }
    };
    if let Some(rows) = rows {
        for entry in rows {
            let path = entry
                .map_err(|_| Error::Refused("client operation observation failed".into()))?
                .path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| Error::Refused("client operation name malformed".into()))?;
            if name == operation.to_string() || name == format!("remote-start-{operation}") {
                return Err(Error::Refused(
                    "import operation already has a retained client request".into(),
                ));
            }
            let bytes = read(&path)?.ok_or_else(|| {
                Error::Refused("client operation changed during observation".into())
            })?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Refused("client operation journal malformed".into()))?;
            let request = &value["request"];
            if (value["actorPubkey"].as_str() == Some(actor)
                && request["type"].as_str() == Some("create")
                && request["relay"]
                    .as_str()
                    .map(canonical_relay_key)
                    .as_deref()
                    == Some(canonical_relay_key(relay).as_str()))
                || (request["agent_pubkey"].as_str() == Some(actor)
                    && request["provider_authority"].as_str()
                        == Some(config.provider_pubkey.as_str()))
            {
                return Err(Error::Refused(
                    "actor has a retained client admission request".into(),
                ));
            }
        }
    }
    beekeeper_session_provider::state::StateStore::require_unadmitted_import(
        &config.provider_state_dir,
        actor,
        &operation.to_string(),
    )
    .map_err(Error::Refused)
}

#[cfg(test)]
#[path = "import_boundary_tests.rs"]
mod tests;
