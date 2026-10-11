//! Local custody and journal materialization; no process supervisor.

use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Operation {
    pub(super) request: Value,
    pub(super) actor_pubkey: String,
    pub(super) events: Vec<Event>,
    #[serde(default)]
    pub(super) target: Option<CodingSessionTarget>,
    #[serde(default)]
    pub(super) receipt_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) predecessor: Option<beekeeper_host_core::ownership::Transfer>,
}

pub(super) fn fail(error: impl std::fmt::Display) -> CliError {
    CliError::Other(error.to_string())
}

pub(crate) fn validate_checkout(cwd: &Path, home: &Path, state: &Path) -> Result<(), CliError> {
    let canonical_cwd = std::fs::canonicalize(cwd).map_err(fail)?;
    let cwd = canonical_cwd.as_path();
    if !cwd.is_dir() {
        return Err(CliError::Usage(
            "workspace must be an existing directory".into(),
        ));
    }
    let native_home = std::env::var_os("HOME").and_then(|value| std::fs::canonicalize(value).ok());
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
    if std::env::var_os("HOME")
        .and_then(|value| std::fs::canonicalize(value).ok())
        .as_deref()
        == Some(cwd)
    {
        return Err(CliError::Usage(
            "--cwd cannot be the operating-system home directory".into(),
        ));
    }
    Ok(())
}

pub(super) fn home(value: &Option<PathBuf>) -> Result<PathBuf, CliError> {
    let home = value
        .clone()
        .map(Ok)
        .unwrap_or_else(layout::home_dir)
        .map_err(CliError::Usage)?;
    if !home.is_absolute() {
        return Err(CliError::Usage("--home must be absolute".into()));
    }
    Ok(home)
}

pub(super) fn owner(client: &BeekeeperClient) -> Result<String, CliError> {
    if client.verified_auth_tag_owner_hex()?.is_some() {
        return Err(CliError::Auth(
            "commissioning requires an owner signer, not delegated agent credentials".into(),
        ));
    }
    Ok(client.keys().public_key().to_hex())
}

pub(super) fn management_lock(home: &Path) -> Result<File, CliError> {
    beekeeper_host_core::ownership::management_lock(
        home,
        layout::Instance::from_env().map_err(CliError::Usage)?,
    )
    .map_err(fail)
}

pub(super) fn operation_path(home: &Path, id: Uuid) -> Result<PathBuf, CliError> {
    let dir = layout::host_dir(home, layout::Instance::from_env().map_err(CliError::Usage)?)
        .join("management/operations");
    atomic_write::create_dir_all_restricted(&dir).map_err(fail)?;
    Ok(dir.join(format!("{id}.json")))
}

pub(super) fn read_operation(path: &Path, request: &Value) -> Result<Option<Operation>, CliError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(fail(error)),
    };
    let operation: Operation =
        serde_json::from_slice(&bytes).map_err(|_| fail("operation journal is malformed"))?;
    if operation.request != *request {
        return Err(CliError::Conflict("operation UUID was already used with a different request; original operation preserved".into()));
    }
    Ok(Some(operation))
}

pub(super) fn save(path: &Path, value: &impl Serialize) -> Result<(), CliError> {
    atomic_write::atomic_write_json_restricted(
        path,
        &serde_json::to_vec_pretty(value).map_err(fail)?,
    )
    .map_err(fail)
}

pub(super) fn actor_custody(
    state: &Path,
    actor: &str,
    initial: Option<(&str, &str)>,
) -> Result<(String, String), CliError> {
    crate::validate::validate_hex64(actor)?;
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

pub(super) fn stage_actor(
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
    crate::commands::sessions::handover_reconstruct::bind_pending_directory(
        &config.provider_state_dir.join(PROJECTS_FILE_NAME),
        command,
        cwd,
        project,
    )?;
    Ok(())
}

pub(super) fn restricted_input(path: &Path) -> Result<String, CliError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| CliError::Usage("staged custody input is unavailable".into()))?;
    if !metadata.file_type().is_file() || metadata.len() > 4096 {
        return Err(CliError::Usage(
            "staged custody input must be a bounded regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Usage(
                "staged custody input must be owner-only".into(),
            ));
        }
    }
    std::fs::read_to_string(path).map_err(|_| fail("staged custody input could not be read"))
}

pub(super) fn existing_identity(
    args: &CreateArgs,
    owner: &str,
    actor: &str,
) -> Result<provision::MintedAgentIdentity, CliError> {
    let key_path = args
        .key_file
        .as_deref()
        .ok_or_else(|| CliError::Usage("adoption requires --key-file".into()))?;
    let auth_path = args
        .auth_tag_file
        .as_deref()
        .ok_or_else(|| CliError::Usage("adoption requires --auth-tag-file".into()))?;
    let nsec = restricted_input(key_path)?;
    let keys = Keys::parse(nsec.trim())
        .map_err(|_| CliError::Usage("staged actor key is invalid".into()))?;
    if keys.public_key().to_hex() != actor {
        return Err(CliError::Auth(
            "staged key does not match the identity being adopted".into(),
        ));
    }
    let auth = restricted_input(auth_path)?;
    let attesting_owner =
        beekeeper_sdk::nip_oa::verify_auth_tag(auth.trim(), &keys.public_key())
            .map_err(|_| CliError::Auth("staged actor attestation is invalid".into()))?;
    if attesting_owner.to_hex() != owner {
        return Err(CliError::Auth(
            "signer does not own the existing actor".into(),
        ));
    }
    Ok(provision::MintedAgentIdentity {
        keys,
        pubkey: actor.into(),
        private_key_nsec: nsec.trim().into(),
        auth_tag: auth.trim().into(),
    })
}

pub(super) fn install_route(
    state: &Path,
    actor: &str,
    channel: Uuid,
    target: &CodingSessionTarget,
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
    let route = json!({"actorPubkey":actor,"channelId":channel,"target":target});
    if !routes.contains(&route) {
        routes.push(route);
    }
    save(&path, &value)
}
