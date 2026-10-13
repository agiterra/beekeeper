//! Restricted executor materialization, independent from public30177 projection.
use super::local::*;
use super::*;

pub(crate) fn stage_configuration(
    _home: &Path,
    config: &HostConfig,
    operation: &Operation,
    args: &CreateRequest,
    cwd: &Path,
) -> Result<(), CliError> {
    let actor = &operation.actor_pubkey;
    let definition = operation
        .events
        .first()
        .ok_or_else(|| fail("configuration missing"))?;
    let dir = config.provider_state_dir.join("actors").join(actor);
    let historical = dir.join("enrollment.json");
    if historical.exists() {
        let value: Value =
            serde_json::from_slice(&std::fs::read(&historical).map_err(fail)?).map_err(fail)?;
        if value.get("nativeContextRequired").and_then(Value::as_bool) == Some(true)
            || value
                .get("nativeImportOperation")
                .is_some_and(|value| !value.is_null())
        {
            return Err(CliError::Refused("historical imported enrollment is retired; recreate through fresh headless setup, retained custody was not changed".into()));
        }
    }
    atomic_write::create_dir_all_restricted(&dir).map_err(fail)?;
    if let Some(private) = &args.private_config {
        validate_record(private, actor)?;
        let record = private
            .record
            .as_object()
            .ok_or_else(|| CliError::Usage("private configuration must be record object".into()))?;
        for field in ["privateKeyNsec", "private_key_nsec", "authTag", "auth_tag"] {
            if record
                .get(field)
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
            {
                return Err(CliError::Usage(
                    "scoped key/attestation must use separate custody adapter".into(),
                ));
            }
        }
        if record
            .get("pubkey")
            .and_then(Value::as_str)
            .is_some_and(|value| value != actor)
        {
            return Err(CliError::Auth(
                "private configuration actor mismatch".into(),
            ));
        }
        let record = private.record.clone();
        let snapshot = json!({"schema":"beekeeper-private-agent-config/v1","actorPubkey":actor,"relayUrl":config.relay_url,"definition":definition,"record":record,"channelId":args.channel,"cwd":cwd,"desiredRunning":args.activate});
        save(&dir.join("configuration.json"), &snapshot)?;
    }
    save(
        &dir.join("enrollment.json"),
        &json!({"actorPubkey":actor,"channelId":args.channel,"cwd":cwd,"providerInstance":args.provider_instance.as_deref().or_else(||config.runtimes.first().filter(|_|config.runtimes.len()==1).map(|runtime|runtime.instance_ref.as_str())),"projectRef":args.project,"desiredRunning":args.activate,"definition":definition,"replayFloorUnix":args.replay_floor_unix.unwrap_or(definition.created_at.as_secs())}),
    )?;
    Ok(())
}

pub(super) fn validate_record(
    private: &crate::PrivateConfiguration,
    actor: &str,
) -> Result<(), CliError> {
    let record = private
        .record
        .as_object()
        .ok_or_else(|| CliError::Usage("private configuration must be record object".into()))?;
    if record
        .get("effective_mcp_command")
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err(CliError::Usage(
            "effective MCP executable must be a string".into(),
        ));
    }
    for field in ["privateKeyNsec", "private_key_nsec", "authTag", "auth_tag"] {
        if record
            .get(field)
            .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
        {
            return Err(CliError::Usage(
                "scoped key/attestation must use separate custody adapter".into(),
            ));
        }
    }
    if record
        .get("pubkey")
        .and_then(Value::as_str)
        .is_some_and(|key| key != actor)
    {
        return Err(CliError::Auth(
            "private configuration actor mismatch".into(),
        ));
    }
    if let Some(command) = record
        .get("acp_command")
        .and_then(Value::as_str)
        .filter(|command| !command.trim().is_empty())
    {
        if Path::new(command)
            .file_name()
            .and_then(|name| name.to_str())
            != Some("beekeeper-acp")
        {
            return Err(CliError::Refused("custom ACP bridge replacement is unsupported by host execution; preserve source custody and configure the agent adapter command instead".into()));
        }
    }
    for name in ["effective_env", "env_vars"] {
        if let Some(values) = record.get(name) {
            let env: std::collections::BTreeMap<String, String> =
                serde_json::from_value(values.clone()).map_err(|_| {
                    CliError::Usage("private runtime environment must contain string values".into())
                })?;
            if env.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "BEEKEEPER_PRIVATE_KEY"
                        | "NOSTR_PRIVATE_KEY"
                        | "BEEKEEPER_AUTH_TAG"
                        | "BEEKEEPER_CSP_PRIVATE_KEY"
                )
            }) {
                return Err(CliError::Usage(
                    "private settings cannot replace identity custody".into(),
                ));
            }
        }
    }
    Ok(())
}
pub(super) fn prepare_update(
    context: &Context,
    client: &impl RelayTransport,
    actor: &str,
    private: &crate::PrivateConfiguration,
) -> Result<(HostConfig, Value), CliError> {
    validate_record(private, actor)?;
    let config = HostConfig::load(&layout::host_config_path(&context.home, context.instance)).map_err(fail)?.ok_or_else(|| CliError::Refused("private configuration needs executor custody; remote public configuration is available separately".into()))?;
    if record::canonical_relay_key(&crate::to_ws_url(&config.relay_url))
        != record::canonical_relay_key(&crate::to_ws_url(client.relay_url()))
    {
        return Err(CliError::Auth(
            "private configuration host relay mismatch".into(),
        ));
    }
    let (_, auth) = actor_custody(&config.provider_state_dir, actor, None)?;
    let actor_key = nostr::PublicKey::from_hex(actor).map_err(fail)?;
    let attester = beekeeper_sdk::nip_oa::verify_auth_tag(&auth, &actor_key).map_err(fail)?;
    if attester.to_hex() != owner(client)? {
        return Err(CliError::Auth(
            "signer does not own private actor custody".into(),
        ));
    }
    let path = config
        .provider_state_dir
        .join("actors")
        .join(actor)
        .join("enrollment.json");
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| CliError::Refused("private enrollment unavailable".into()))?;
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err(CliError::Refused(
            "private enrollment is not bounded regular custody".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Refused(
                "private enrollment permissions unsafe".into(),
            ));
        }
    }
    let enrollment: Value =
        serde_json::from_slice(&std::fs::read(path).map_err(fail)?).map_err(fail)?;
    if enrollment.get("actorPubkey").and_then(Value::as_str) != Some(actor) {
        return Err(CliError::Auth("private enrollment actor mismatch".into()));
    }
    Ok((config, enrollment))
}
pub(super) fn validate_enrollment_definition(
    client: &impl RelayTransport,
    actor: &str,
    enrollment: &Value,
) -> Result<(), CliError> {
    let accepted: Event = serde_json::from_value(
        enrollment
            .get("definition")
            .cloned()
            .ok_or_else(|| CliError::Refused("private enrollment definition missing".into()))?,
    )
    .map_err(|_| CliError::Refused("private enrollment definition malformed".into()))?;
    if accepted.pubkey != client.keys().public_key()
        || accepted.kind.as_u16() != 30177
        || accepted.verify().is_err()
        || accepted
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("d"))
            .count()
            != 1
        || !accepted
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["d", actor])
    {
        return Err(CliError::Auth(
            "private enrollment definition authority invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn snapshot_bytes(
    context: &Context,
    client: &impl RelayTransport,
    actor: &str,
) -> Result<(HostConfig, Vec<u8>), CliError> {
    crate::validate_hex64(actor)?;
    let config = HostConfig::load(&layout::host_config_path(&context.home, context.instance))
        .map_err(fail)?
        .ok_or_else(|| {
            CliError::Refused("private snapshot requires commissioned custody".into())
        })?;
    if config.instance != context.instance
        || record::canonical_relay_key(&crate::to_ws_url(&config.relay_url))
            != record::canonical_relay_key(&crate::to_ws_url(client.relay_url()))
    {
        return Err(CliError::Auth(
            "private snapshot host/community scope mismatch".into(),
        ));
    }
    let path = config
        .provider_state_dir
        .join("actors")
        .join(actor)
        .join("configuration.json");
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| CliError::Refused("protected private snapshot unavailable".into()))?;
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err(CliError::Refused(
            "private snapshot must be bounded regular custody".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Refused(
                "private snapshot permissions unsafe".into(),
            ));
        }
    }
    let (_, auth) = actor_custody(&config.provider_state_dir, actor, None)?;
    let actor_key = nostr::PublicKey::from_hex(actor).map_err(fail)?;
    if beekeeper_sdk::nip_oa::verify_auth_tag(&auth, &actor_key)
        .map_err(fail)?
        .to_hex()
        != owner(client)?
    {
        return Err(CliError::Auth(
            "signer does not own private actor snapshot".into(),
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|_| CliError::Refused("private snapshot unreadable".into()))?;
    Ok((config, bytes))
}

pub(super) fn update_snapshot(
    config: &HostConfig,
    enrollment: &Value,
    definition: &Event,
    private: &crate::PrivateConfiguration,
) -> Result<Value, CliError> {
    let actor = enrollment
        .get("actorPubkey")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("private enrollment actor missing"))?;
    let record = private.record.clone();
    Ok(
        json!({"schema":"beekeeper-private-agent-config/v1","actorPubkey":actor,"relayUrl":config.relay_url,"definition":definition,"record":record,"channelId":enrollment.get("channelId"),"cwd":enrollment.get("cwd"),"desiredRunning":enrollment.get("desiredRunning")}),
    )
}

pub(super) fn commit_update(
    config: &HostConfig,
    enrollment: &Value,
    definition: &Event,
    private: &crate::PrivateConfiguration,
) -> Result<(), CliError> {
    let actor = enrollment
        .get("actorPubkey")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("private enrollment actor missing"))?;
    let dir = config.provider_state_dir.join("actors").join(actor);
    let snapshot = update_snapshot(config, enrollment, definition, private)?;
    save(&dir.join("configuration.json"), &snapshot)?;
    #[cfg(test)]
    {
        let mut cuts = ENROLLMENT_COMMIT_CUTS.lock().expect("fixture cuts");
        if let Some(index) = cuts.iter().position(|path| path == &dir) {
            cuts.remove(index);
            return Err(CliError::Other(
                "fixture crash after configuration before enrollment commit".into(),
            ));
        }
    }
    let mut enrollment = enrollment.clone();
    enrollment["definition"] = serde_json::to_value(definition).map_err(fail)?;
    save(&dir.join("enrollment.json"), &enrollment)
}

pub(super) fn reject_owner_secret(
    private: &crate::PrivateConfiguration,
    owner: &Keys,
) -> Result<(), CliError> {
    use nostr::ToBech32;
    let serialized = serde_json::to_string(&private.record).map_err(fail)?;
    let key = owner.secret_key();
    let hex = key.to_secret_hex();
    let nsec = key.to_bech32().map_err(fail)?;
    if serialized.contains(&hex) || serialized.contains(&nsec) {
        return Err(CliError::Usage(
            "owner signing key cannot enter private managed settings or journal".into(),
        ));
    }
    // Validate custody-reserved environment fields before immutable journaling.
    let actor = private
        .record
        .get("pubkey")
        .and_then(Value::as_str)
        .unwrap_or("");
    validate_record(private, actor)
}

#[cfg(test)]
static ENROLLMENT_COMMIT_CUTS: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());
#[cfg(test)]
pub(super) fn fail_next_enrollment_commit(dir: &Path) {
    ENROLLMENT_COMMIT_CUTS
        .lock()
        .expect("fixture cuts")
        .push(dir.into());
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    #[test]
    fn internal_bridge_retires_only_known_default_and_refuses_custom_replacement() {
        let actor = "ab".repeat(32);
        for command in ["beekeeper-acp", "/bundle/bin/beekeeper-acp", ""] {
            assert!(validate_record(
                &crate::PrivateConfiguration {
                    record: json!({"acp_command":command})
                },
                &actor
            )
            .is_ok());
        }
        assert!(matches!(
            validate_record(
                &crate::PrivateConfiguration {
                    record: json!({"acp_command":"/private/custom-bridge","effective_command":"/private/custom-agent"})
                },
                &actor
            ),
            Err(CliError::Refused(_))
        ));
        assert!(validate_record(&crate::PrivateConfiguration{record:json!({"effective_command":"/private/custom-agent","effective_args":["--acp"],"effective_env":{"CUSTOM_ADAPTER_OPTION":"preserved"}})},&actor).is_ok());
    }
}
