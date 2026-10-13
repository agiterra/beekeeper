//! Local custody and journal materialization; no process supervisor.

use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Operation {
    pub(crate) request: Value,
    pub(crate) actor_pubkey: String,
    pub(crate) events: Vec<Event>,
    #[serde(default)]
    pub(crate) target: Option<CodingSessionTarget>,
    #[serde(default)]
    pub(crate) receipt_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) predecessor: Option<beekeeper_host_core::ownership::Transfer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) custody_handoff: Option<ReservedCustody>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReservedCustody {
    pub(crate) key: String,
    pub(crate) auth_tag: String,
}

/// Finish only the still-unpublished scoped identity reservation. The handoff
/// is removed durably before any relay publication, not retained as a keystore.
pub(crate) fn finish_reserved_custody(
    state: &Path,
    path: &Path,
    operation: &mut Operation,
    owner: &str,
) -> Result<(String, String), CliError> {
    if let Some(handoff) = operation.custody_handoff.as_ref() {
        if operation.target.is_some() || operation.receipt_event_id.is_some() {
            return Err(CliError::Refused(
                "completed operation cannot restore deleted custody".into(),
            ));
        }
        let keys = Keys::parse(&handoff.key).map_err(|_| fail("reserved actor key is invalid"))?;
        if keys.public_key().to_hex() != operation.actor_pubkey
            || keys.public_key().to_hex() == owner
            || operation.request.get("owner").and_then(Value::as_str) != Some(owner)
        {
            return Err(CliError::Auth(
                "reserved scoped identity does not match operation owner/actor".into(),
            ));
        }
        let attester =
            beekeeper_sdk::nip_oa::verify_auth_tag(&handoff.auth_tag, &keys.public_key())
                .map_err(|_| CliError::Auth("reserved scoped attestation invalid".into()))?;
        if attester.to_hex() != owner {
            return Err(CliError::Auth(
                "reserved identity is attested by another owner".into(),
            ));
        }
        let auth: Vec<String> = serde_json::from_str(&handoff.auth_tag)
            .map_err(|_| fail("reserved attestation malformed"))?;
        if !operation.events.iter().any(|event| {
            event.kind.as_u16() == 30177
                && event.pubkey.to_hex() == owner
                && event.verify().is_ok()
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["d", operation.actor_pubkey.as_str()])
        }) || !operation.events.iter().any(|event| {
            event.kind.as_u16() == 0
                && event.pubkey == keys.public_key()
                && event.verify().is_ok()
                && event.tags.iter().any(|tag| tag.as_slice() == auth)
        }) {
            return Err(CliError::Auth(
                "reserved custody does not match signed operation identity".into(),
            ));
        }
        let dir = state.join("actors").join(&operation.actor_pubkey);
        for file in [dir.join("key"), dir.join("auth-tag.json")] {
            match std::fs::symlink_metadata(&file) {
                Ok(metadata) => {
                    if !metadata.file_type().is_file() || metadata.len() > 4096 {
                        return Err(CliError::Refused("existing actor custody is not a bounded regular file; original retained".into()));
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if metadata.permissions().mode() & 0o077 != 0 {
                            return Err(CliError::Refused("existing actor custody permissions are too broad; original retained".into()));
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    return Err(CliError::Refused(
                        "existing actor custody unreadable; original retained".into(),
                    ))
                }
            }
        }
        if let Ok(existing) = std::fs::read_to_string(dir.join("key")) {
            let existing =
                Keys::parse(existing.trim()).map_err(|_| fail("existing actor key invalid"))?;
            if existing.public_key() != keys.public_key() {
                return Err(CliError::Refused(
                    "existing actor custody conflicts; original retained".into(),
                ));
            }
        } else if dir.join("key").exists() {
            return Err(CliError::Refused(
                "existing actor key unreadable; original retained".into(),
            ));
        }
        if let Ok(existing) = std::fs::read_to_string(dir.join("auth-tag.json")) {
            if existing.trim() != handoff.auth_tag.trim() {
                return Err(CliError::Refused(
                    "existing actor attestation conflicts; original retained".into(),
                ));
            }
        } else if dir.join("auth-tag.json").exists() {
            return Err(CliError::Refused(
                "existing actor attestation unreadable; original retained".into(),
            ));
        }
        actor_custody(
            state,
            &operation.actor_pubkey,
            Some((&handoff.key, &handoff.auth_tag)),
        )?;
        operation.custody_handoff = None;
        save(path, operation)?;
    }
    let held = actor_custody(state, &operation.actor_pubkey, None)?;
    let keys = Keys::parse(&held.0).map_err(|_| fail("actor custody invalid"))?;
    if beekeeper_sdk::nip_oa::verify_auth_tag(&held.1, &keys.public_key())
        .map_err(|_| fail("actor attestation invalid"))?
        .to_hex()
        != owner
    {
        return Err(CliError::Auth(
            "actor custody belongs to another owner".into(),
        ));
    }
    Ok(held)
}

pub(crate) fn fail(error: impl std::fmt::Display) -> CliError {
    CliError::Other(error.to_string())
}

pub(crate) fn validate_checkout(
    cwd: &Path,
    home: &Path,
    state: &Path,
    native_home: Option<&Path>,
) -> Result<(), CliError> {
    let canonical_cwd = std::fs::canonicalize(cwd).map_err(fail)?;
    let cwd = canonical_cwd.as_path();
    if !cwd.is_dir() {
        return Err(CliError::Usage(
            "workspace must be an existing directory".into(),
        ));
    }
    let native_home = native_home.and_then(|value| std::fs::canonicalize(value).ok());
    for custody in [home, state] {
        let custody = std::fs::canonicalize(custody).map_err(fail)?;
        if custody.starts_with(cwd)
            || (native_home.as_deref() != Some(custody.as_path()) && cwd.starts_with(&custody))
        {
            return Err(CliError::Usage(
                "--cwd must be outside the commissioned custody home and state directory".into(),
            ));
        }
    }
    if native_home.as_deref() == Some(cwd) {
        return Err(CliError::Usage(
            "--cwd cannot be the operating-system home directory".into(),
        ));
    }
    Ok(())
}

pub(crate) fn owner(client: &impl RelayTransport) -> Result<String, CliError> {
    if client.verified_auth_tag_owner_hex()?.is_some() {
        return Err(CliError::Auth(
            "commissioning requires an owner signer, not delegated agent credentials".into(),
        ));
    }
    Ok(client.keys().public_key().to_hex())
}

pub(crate) fn management_lock(context: &Context) -> Result<File, CliError> {
    beekeeper_host_core::ownership::management_lock(&context.home, context.instance).map_err(fail)
}

pub(crate) fn operation_path(context: &Context, id: Uuid) -> Result<PathBuf, CliError> {
    let dir = layout::host_dir(&context.home, context.instance).join("management/operations");
    atomic_write::create_dir_all_restricted(&dir).map_err(fail)?;
    Ok(dir.join(format!("{id}.json")))
}

pub(crate) fn read_operation(path: &Path, request: &Value) -> Result<Option<Operation>, CliError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(fail(error)),
    };
    if !metadata.file_type().is_file() || metadata.len() > 8 * 1024 * 1024 {
        return Err(CliError::Refused(
            "operation journal is not a bounded regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Refused(
                "operation journal custody permissions are too broad".into(),
            ));
        }
    }
    let bytes = std::fs::read(path).map_err(fail)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(CliError::Refused(
            "operation journal grew beyond its bound".into(),
        ));
    }
    let operation: Operation =
        serde_json::from_slice(&bytes).map_err(|_| fail("operation journal is malformed"))?;
    if operation.request != *request {
        return Err(CliError::Conflict("operation UUID was already used with a different request; original operation preserved".into()));
    }
    Ok(Some(operation))
}

pub(crate) fn save(path: &Path, value: &impl Serialize) -> Result<(), CliError> {
    atomic_write::atomic_write_json_restricted(
        path,
        &serde_json::to_vec_pretty(value).map_err(fail)?,
    )
    .map_err(fail)
}

pub(crate) fn actor_custody(
    state: &Path,
    actor: &str,
    initial: Option<(&str, &str)>,
) -> Result<(String, String), CliError> {
    crate::validate_hex64(actor)?;
    let dir = state.join("actors").join(actor);
    if let Some((key, auth)) = initial {
        atomic_write::create_dir_all_restricted(&dir).map_err(fail)?;
        atomic_write::atomic_write_json_restricted(&dir.join("key"), key.as_bytes())
            .map_err(fail)?;
        atomic_write::atomic_write_json_restricted(&dir.join("auth-tag.json"), auth.as_bytes())
            .map_err(fail)?;
    }
    let key = std::fs::read_to_string(dir.join("key"))
        .map_err(|_| CliError::Refused("operation identity is reserved but actor custody is unavailable; refusing to mint a replacement identity".into()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(dir.join("key"))
            .map_err(fail)?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(CliError::Refused(
                "actor key custody permissions are too broad".into(),
            ));
        }
    }
    let keys = Keys::parse(key.trim()).map_err(|_| fail("actor custody key is invalid"))?;
    if keys.public_key().to_hex() != actor {
        return Err(CliError::Refused("actor custody identity mismatch".into()));
    }
    let auth = std::fs::read_to_string(dir.join("auth-tag.json"))
        .map_err(|_| fail("actor attestation custody is unavailable"))?;
    beekeeper_sdk::nip_oa::verify_auth_tag(&auth, &keys.public_key())
        .map_err(|_| fail("actor custody attestation is invalid"))?;
    Ok((key, auth))
}

pub(crate) fn stage_actor(
    config: &HostConfig,
    command: &str,
    actor: &str,
    name: &str,
    credentials: (&str, &str),
    cwd: &Path,
    project: Option<&str>,
) -> Result<(), CliError> {
    let (key, auth) = credentials;
    let path = config.provider_state_dir.join("actor-seats.json");
    let mut seats: Value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
            fail("existing actor seat custody is malformed; refusing to replace it")
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            json!({"version":1,"pending":{}})
        }
        Err(error) => return Err(fail(error)),
    };
    let pending = seats
        .get_mut("pending")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| fail("existing actor seats pending map is malformed"))?;
    pending.insert(
        command.to_string(),
        json!({"pubkey":actor,"nsec":key,"authTag":auth,
        "relayUrl":config.relay_url,"displayName":name}),
    );
    save(&path, &seats)?;
    crate::bind_pending_directory(
        &config.provider_state_dir.join(PROJECTS_FILE_NAME),
        command,
        cwd,
        project,
    )?;
    Ok(())
}

pub(crate) fn install_route(
    state: &Path,
    actor: &str,
    channel: Uuid,
    target: &CodingSessionTarget,
    replay_floor_unix: Option<u64>,
) -> Result<(), CliError> {
    let path = state.join("conversation-routes.json");
    let mut value: Value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| fail("conversation routes are malformed; refusing to replace them"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({"routes":[]}),
        Err(error) => return Err(fail(error)),
    };
    let routes = value
        .get_mut("routes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| fail("conversation routes map is malformed"))?;
    let mut route = json!({"actorPubkey":actor,"channelId":channel,"target":target});
    if let Some(floor) = replay_floor_unix {
        route["replayFloorUnix"] = json!(floor);
    }
    if routes.iter().any(|held| {
        held["actorPubkey"] == actor
            && held["channelId"] == channel.to_string()
            && held["target"] == route["target"]
            && held != &route
    }) {
        return Err(CliError::Conflict(
            "accepted route replay boundary conflicts with preserved operation".into(),
        ));
    }
    if !routes.contains(&route) {
        routes.push(route);
    }
    save(&path, &value)
}

#[cfg(test)]
#[path = "reserved_custody_tests.rs"]
mod custody_tests;

// Completion is tied to the exact Created-receipt target retained by the caller.
// Prefer the worker's already-installed route boundary, never the latest head time.
pub(crate) fn completed_route_floor(
    state: &Path,
    actor: &str,
    channel: Uuid,
    target: &CodingSessionTarget,
    explicit: Option<u64>,
    staged: u64,
) -> Result<Option<u64>, CliError> {
    let path = state.join("conversation-routes.json");
    let value: Value = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(fail)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Some(staged)),
        Err(error) => return Err(fail(error)),
    };
    let rows = value["routes"]
        .as_array()
        .ok_or_else(|| fail("conversation routes malformed"))?;
    let exact = serde_json::to_value(target).map_err(fail)?;
    let matches: Vec<_> = rows
        .iter()
        .filter(|row| {
            row["actorPubkey"] == actor
                && row["channelId"] == channel.to_string()
                && row["target"] == exact
        })
        .collect();
    if matches.len() > 1 {
        return Err(CliError::Conflict(
            "accepted route replay boundary is ambiguous".into(),
        ));
    }
    let Some(route) = matches.first() else {
        return Ok(Some(staged));
    };
    let floor = route.get("replayFloorUnix").and_then(Value::as_u64);
    if explicit.is_some() && explicit != floor {
        return Err(CliError::Conflict(
            "explicit replay boundary conflicts with accepted worker route".into(),
        ));
    }
    Ok(floor)
}
