//! Protected replay provenance and a private signed definition pin; never public events.
use super::*;

pub(super) fn protected_json(path: &Path) -> Result<Option<Value>, CliError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(fail(error)),
    };
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err(CliError::Refused(
            "private materialization is not bounded regular custody".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Refused(
                "private materialization custody permissions are too broad".into(),
            ));
        }
    }
    serde_json::from_slice(&std::fs::read(path).map_err(fail)?)
        .map(Some)
        .map_err(fail)
}

pub(super) fn replay_floor(
    existing: Option<&Value>,
    definition: &Event,
    actor: &str,
    explicit: Option<u64>,
    channel: Uuid,
    stopped_successor: bool,
) -> Result<u64, CliError> {
    let Some(existing) = existing else {
        return Ok(explicit.unwrap_or(definition.created_at.as_secs()));
    };
    let bound: Event = serde_json::from_value(existing["definition"].clone()).map_err(fail)?;
    if definition.verify().is_err()
        || definition.kind.as_u16() != 30177
        || !definition
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["d", actor])
        || existing["actorPubkey"] != actor
        || bound.pubkey != definition.pubkey
        || bound.kind.as_u16() != 30177
        || bound.verify().is_err()
        || !bound.tags.iter().any(|tag| tag.as_slice() == ["d", actor])
    {
        return Err(CliError::Auth(
            "existing enrollment replay provenance mismatch".into(),
        ));
    }
    // The retained primary boundary is not a global floor for every later
    // responsibility. A new channel or verified stopped-target successor may
    // explicitly commission its own replay contract.
    let new_responsibility =
        existing["channelId"].as_str() != Some(channel.to_string().as_str()) || stopped_successor;
    if new_responsibility {
        if let Some(explicit) = explicit {
            return Ok(explicit);
        }
    }
    let floor = existing.get("replayFloorUnix").and_then(Value::as_u64).ok_or_else(|| CliError::Refused("existing enrollment has no proven captured replay boundary; retained custody was not changed".into()))?;
    if !new_responsibility && explicit.is_some_and(|requested| requested != floor) {
        return Err(CliError::Conflict(
            "explicit replay boundary differs from retained enrollment".into(),
        ));
    }
    Ok(floor)
}

pub(super) fn bind_private_definition(
    snapshot: &mut Value,
    config: &HostConfig,
    actor: &str,
    keys: &Keys,
) -> Result<(), CliError> {
    let Some(pin) = snapshot["record"].get("private_persona") else {
        return Ok(());
    };
    let bound: Event = serde_json::from_value(snapshot["definition"].clone()).map_err(fail)?;
    let source: Event = serde_json::from_value(pin["event"].clone()).map_err(fail)?;
    let source_json =
        beekeeper_core::private_managed_agent::parse_strict_json(source.content.as_bytes())
            .map_err(fail)?;
    let content: beekeeper_core::persona::PersonaEventContent =
        serde_json::from_value(source_json).map_err(fail)?;
    use nostr::hashes::{sha256, Hash};
    let hash = sha256::Hash::hash(&serde_json::to_vec(&content).map_err(fail)?).to_string();
    let mut public =
        beekeeper_core::private_managed_agent::parse_strict_json(bound.content.as_bytes())
            .map_err(fail)?;
    let persona = public["persona_id"]
        .as_str()
        .ok_or_else(|| CliError::Auth("private pin requires linked definition".into()))?;
    let coordinate = pin
        .get("personaCoordinate")
        .and_then(Value::as_str)
        .unwrap_or(persona);
    if pin["schema"] != "beekeeper-private-persona/v1"
        || pin["actorPubkey"] != actor
        || pin["personaId"] != persona
        || pin["sourceHash"] != hash
        || pin["relayUrl"]
            .as_str()
            .map(|relay| record::canonical_relay_key(&crate::to_ws_url(relay)))
            != Some(record::canonical_relay_key(&crate::to_ws_url(
                &config.relay_url,
            )))
        || bound.pubkey != keys.public_key()
        || source.pubkey != keys.public_key()
        || bound.kind.as_u16() != 30177
        || source.kind.as_u16() != 30175
        || bound.verify().is_err()
        || source.verify().is_err()
        || !bound.tags.iter().any(|tag| tag.as_slice() == ["d", actor])
        || source
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().is_some_and(|name| name == "d"))
            .count()
            != 1
        || !source
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["d", coordinate])
        || source
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["shared", "true"])
    {
        return Err(CliError::Auth(
            "protected private definition pin authority mismatch".into(),
        ));
    }
    if let Some(version) = public.get("persona_source_version").and_then(Value::as_str) {
        if version != hash {
            return Err(CliError::Conflict(
                "private definition source hash differs from public pin".into(),
            ));
        }
        return Ok(());
    }
    public
        .as_object_mut()
        .ok_or_else(|| fail("public definition is not an object"))?
        .insert("persona_source_version".into(), json!(hash));
    let expected = serde_json::to_string(&public).map_err(fail)?;
    let existing = protected_json(
        &config
            .provider_state_dir
            .join("actors")
            .join(actor)
            .join("configuration.json"),
    )?;
    if let Some(cached) = existing
        .as_ref()
        .and_then(|value| value.get("privateDefinition"))
    {
        let cached: Event = serde_json::from_value(cached.clone()).map_err(fail)?;
        if cached.content == expected
            && cached.tags == bound.tags
            && cached.created_at == bound.created_at
        {
            if cached.pubkey != keys.public_key()
                || cached.kind != bound.kind
                || cached.verify().is_err()
            {
                return Err(CliError::Auth(
                    "cached private definition authority mismatch".into(),
                ));
            }
            snapshot["privateDefinition"] = serde_json::to_value(cached).map_err(fail)?;
            return Ok(());
        }
    }
    let private = nostr::EventBuilder::new(bound.kind, expected)
        .tags(bound.tags.iter().cloned())
        .custom_created_at(bound.created_at)
        .sign_with_keys(keys)
        .map_err(fail)?;
    snapshot["privateDefinition"] = serde_json::to_value(private).map_err(fail)?;
    Ok(())
}
