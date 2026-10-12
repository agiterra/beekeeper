//! Restricted executor materialization, independent from public30177 projection.
use super::local::*;
use super::*;

pub(crate) fn stage_configuration(
    home: &Path,
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
    let current =
        beekeeper_host_core::ownership::for_pair(home, config.instance, actor, &config.relay_url)
            .map_err(fail)?;
    let mut native = args.native_context.clone();
    let mut native_required = args.adopt_agent_pubkey.is_some();
    if native.is_none()
        && current.as_ref().is_some_and(|held| {
            held.phase == beekeeper_host_core::ownership::Phase::HostStopped
                && held.target.is_none()
        })
    {
        let enrollment: Value = match std::fs::read(dir.join("enrollment.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(fail)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
            Err(error) => return Err(fail(error)),
        };
        native_required = enrollment
            .get("nativeContextRequired")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if native_required {
            let held = current
                .as_ref()
                .ok_or_else(|| fail("paused import ownership missing"))?;
            let source = config
                .provider_state_dir
                .join(format!("native-import-{}.json", held.operation_id));
            let metadata = std::fs::symlink_metadata(&source).map_err(|_| {
                CliError::Refused(
                    "paused import has no recoverable native context; repair required".into(),
                )
            })?;
            if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
                return Err(CliError::Refused(
                    "paused native import custody invalid".into(),
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err(CliError::Refused(
                        "paused native import custody permissions unsafe".into(),
                    ));
                }
            }
            native =
                Some(serde_json::from_slice(&std::fs::read(source).map_err(fail)?).map_err(fail)?);
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
        let record = preserve_migration_source(&dir, &private.record)?;
        let snapshot = json!({"schema":"beekeeper-private-agent-config/v1","actorPubkey":actor,"relayUrl":config.relay_url,"definition":definition,"record":record,"channelId":args.channel,"cwd":cwd,"desiredRunning":args.activate});
        save(&dir.join("configuration.json"), &snapshot)?;
    }
    let source_metadata = read_private_source(&dir)?.or_else(|| {
        args.private_config
            .as_ref()
            .and_then(|private| private.record.get("migration_backfill_since_unix").cloned())
    });
    save(
        &dir.join("enrollment.json"),
        &json!({"actorPubkey":actor,"channelId":args.channel,"cwd":cwd,"providerInstance":args.provider_instance.as_deref().or_else(||config.runtimes.first().filter(|_|config.runtimes.len()==1).map(|runtime|runtime.instance_ref.as_str())),"projectRef":args.project,"desiredRunning":args.activate,"definition":definition,"replayFloorUnix":args.replay_floor_unix,"migrationBackfillSinceUnix":source_metadata.and_then(|value|value.as_u64()),"nativeContextRequired":native_required,"nativeImportOperation":native.as_ref().map(|_|args.operation_id.to_string())}),
    )?;
    if let Some(native) = &native {
        if args.activate && args.replay_floor_unix.is_none() {
            return Err(CliError::Refused("native import requires a verified replay boundary; prior request outcomes are not established".into()));
        }
        if native.schema != "beekeeper-legacy-native-context/v1"
            || native.actor_pubkey != *actor
            || record::canonical_relay_key(&native.relay_url)
                != record::canonical_relay_key(&config.relay_url)
        {
            return Err(CliError::Auth("native import scope mismatch".into()));
        }
        let transfer = beekeeper_host_core::ownership::for_pair(
            home,
            config.instance,
            actor,
            &config.relay_url,
        )
        .map_err(fail)?;
        if !transfer.as_ref().is_some_and(|transfer| {
            transfer.legacy_nonce == native.start_nonce
                && ((transfer.operation_id == args.operation_id.to_string()
                    && transfer.phase == beekeeper_host_core::ownership::Phase::Quiesced)
                    || (transfer.phase == beekeeper_host_core::ownership::Phase::HostStopped
                        && transfer.target.is_none()))
        }) {
            return Err(CliError::Refused(
                "native import lacks exact quiesced generation".into(),
            ));
        }
        let sessions: Vec<_> = native
            .sessions
            .iter()
            .filter(|session| session.channel_id == args.channel.to_string())
            .collect();
        if sessions.len() != 1 {
            return Err(CliError::Refused(
                "native context missing or ambiguous; no fresh fallback".into(),
            ));
        }
        let session = sessions[0];
        if session.resume_cursor.trim().is_empty()
            || session.runtime_command.is_none()
            || session.source_home.is_none()
            || session.cwd.canonicalize().map_err(fail)? != cwd
        {
            return Err(CliError::Refused(
                "native runtime/account/workspace unavailable; no fresh fallback".into(),
            ));
        }
        save(
            &config
                .provider_state_dir
                .join(format!("native-import-{}.json", args.operation_id)),
            native,
        )?;
    }
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
    if record::canonical_relay_key(&config.relay_url)
        != record::canonical_relay_key(client.relay_url())
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
    preserve_migration_source(
        &config.provider_state_dir.join("actors").join(actor),
        &private.record,
    )?;
    Ok((config, enrollment))
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
    let record = preserve_migration_source(&dir, &private.record)?;
    let snapshot = json!({"schema":"beekeeper-private-agent-config/v1","actorPubkey":actor,"relayUrl":config.relay_url,"definition":definition,"record":record,"channelId":enrollment.get("channelId"),"cwd":enrollment.get("cwd"),"desiredRunning":enrollment.get("desiredRunning")});
    save(&dir.join("configuration.json"), &snapshot)?;
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

/// Selected source query metadata is private immutable migration evidence, not user configuration.
fn read_private_source(dir: &Path) -> Result<Option<Value>, CliError> {
    let path = dir.join("configuration.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(fail(error)),
    };
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err(CliError::Refused(
            "private migration metadata custody invalid".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Refused(
                "private migration metadata permissions unsafe".into(),
            ));
        }
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path).map_err(fail)?).map_err(fail)?;
    let source = value
        .get("record")
        .and_then(|record| record.get("migration_backfill_since_unix"))
        .filter(|value| !value.is_null())
        .cloned();
    if source
        .as_ref()
        .is_some_and(|value| value.as_u64().is_none())
    {
        return Err(CliError::Refused(
            "private migration source timestamp invalid".into(),
        ));
    }
    Ok(source)
}
fn preserve_migration_source(dir: &Path, requested: &Value) -> Result<Value, CliError> {
    let mut record = requested.clone();
    let supplied = record
        .get("migration_backfill_since_unix")
        .filter(|value| !value.is_null());
    if supplied.is_some_and(|value| value.as_u64().is_none()) {
        return Err(CliError::Usage(
            "migration source timestamp must be unsigned seconds".into(),
        ));
    }
    if let Some(source) = read_private_source(dir)? {
        if supplied.is_some_and(|value| value != &source) {
            return Err(CliError::Conflict(
                "configuration cannot change immutable selected migration source timestamp".into(),
            ));
        }
        record
            .as_object_mut()
            .ok_or_else(|| CliError::Usage("private configuration must be object".into()))?
            .insert("migration_backfill_since_unix".into(), source);
    }
    Ok(record)
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    #[test]
    fn selected_migration_source_survives_private_configuration_and_cannot_be_changed() {
        let root = tempfile::tempdir().expect("restricted custody");
        save(
            &root.path().join("configuration.json"),
            &json!({"record":{"migration_backfill_since_unix":100,"model":"old"}}),
        )
        .expect("private snapshot");
        let updated = preserve_migration_source(root.path(), &json!({"model":"new"}))
            .expect("preserved immutable metadata");
        assert_eq!(updated["migration_backfill_since_unix"], 100);
        assert_eq!(updated["model"], "new");
        assert!(matches!(
            preserve_migration_source(root.path(), &json!({"migration_backfill_since_unix":101})),
            Err(CliError::Conflict(_))
        ));
        assert!(matches!(
            preserve_migration_source(
                root.path(),
                &json!({"migration_backfill_since_unix":"unknown"})
            ),
            Err(CliError::Usage(_))
        ));
        assert_eq!(
            preserve_migration_source(root.path(), &json!({"migration_backfill_since_unix":null}))
                .expect("clear cannot discard source")["migration_backfill_since_unix"],
            100
        );
    }
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
