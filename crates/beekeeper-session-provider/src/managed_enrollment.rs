//! Worker-owned restaging for authorized lifecycle creates of enrolled identities.
use crate::{
    config::Config, managed_configuration::protected, session::CreateFailure, state::StateStore,
};
use beekeeper_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
};
use beekeeper_host_core::{
    atomic_write,
    ownership::{self, Phase},
};
use nostr::{Event, Keys};
use serde_json::{json, Value};
use uuid::Uuid;

fn refused(message: &str) -> CreateFailure {
    crate::execution_scope::refuse("AGENT_ENROLLMENT_UNAVAILABLE", message)
}
fn write(path: &std::path::Path, value: &Value) -> Result<(), CreateFailure> {
    atomic_write::atomic_write_json_restricted(
        path,
        &serde_json::to_vec(value).map_err(|_| refused("enrollment encoding failed"))?,
    )
    .map_err(|_| refused("enrollment persistence failed"))
}
fn enrollment(config: &Config, actor: &str) -> Result<Option<Value>, CreateFailure> {
    protected(
        &config
            .state_dir
            .join("actors")
            .join(actor)
            .join("enrollment.json"),
    )
}
fn definition(value: &Value, actor: &str, owner: &str) -> Result<Event, CreateFailure> {
    let event: Event = serde_json::from_value(
        value
            .get("definition")
            .cloned()
            .ok_or_else(|| refused("enrollment definition missing"))?,
    )
    .map_err(|_| refused("enrollment definition malformed"))?;
    if event.kind.as_u16() != 30177
        || event.pubkey.to_hex() != owner
        || event.verify().is_err()
        || !event.tags.iter().any(|tag| tag.as_slice() == ["d", actor])
    {
        return Err(refused("enrollment definition authority mismatch"));
    }
    Ok(event)
}
/// Existing command admission still checks transport, project grants, membership and runtime.
/// This helper supplies only owner-authorized private execution facts already in worker custody.
pub(crate) async fn stage(
    config: &Config,
    state: &StateStore,
    rest: Option<&beekeeper_acp::relay::RestClient>,
    channel: Uuid,
    operator: &str,
    content: &str,
) -> Result<(), CreateFailure> {
    let Ok(payload) = decode_coding_session_lifecycle_command(content) else {
        return Ok(());
    };
    if state.is_command_consumed(&payload.command_id) {
        return Ok(());
    }
    let CodingSessionLifecycleAction::SessionCreate {
        actor: Some(actor),
        role,
        hire_ref,
        provider_authority_pubkey,
        provider_instance_ref,
        initial_turn,
        ..
    } = &payload.action
    else {
        return Ok(());
    };
    if provider_authority_pubkey != &config.keys.public_key().to_hex() {
        return Ok(());
    }
    let Some(mut value) = enrollment(config, actor)? else {
        return Ok(());
    };
    if role.is_some() || hire_ref.is_some() || initial_turn.is_some() {
        return Err(refused(
            "managed enrollment start requires an attributed Solo create without a seeded turn",
        ));
    }
    if value.get("actorPubkey").and_then(Value::as_str) != Some(actor)
        || value.get("channelId").and_then(Value::as_str) != Some(channel.to_string().as_str())
        || value.get("providerInstance").and_then(Value::as_str)
            != Some(provider_instance_ref.as_str())
    {
        return Err(refused("enrollment channel or runtime scope mismatch"));
    }
    let dir = config.state_dir.join("actors").join(actor);
    let key = protected_secret(&dir.join("key"))?;
    let keys = Keys::parse(key.trim()).map_err(|_| refused("actor custody invalid"))?;
    if keys.public_key().to_hex() != *actor {
        return Err(refused("actor custody identity mismatch"));
    }
    let auth = protected_secret(&dir.join("auth-tag.json"))?;
    let owner = beekeeper_sdk::nip_oa::verify_auth_tag(auth.trim(), &keys.public_key())
        .map_err(|_| refused("actor attestation invalid"))?
        .to_hex();
    if owner != operator {
        return Err(refused(
            "only the attested owner may start this managed identity",
        ));
    }
    definition(&value, actor, &owner)?;
    let rest = rest.ok_or_else(|| refused("current managed definition transport unavailable"))?;
    let owner_key =
        nostr::PublicKey::from_hex(&owner).map_err(|_| refused("actor owner invalid"))?;
    let rows = rest
        .query(&[nostr::Filter::new()
            .kind(nostr::Kind::Custom(30177))
            .author(owner_key)
            .identifier(actor)])
        .await
        .map_err(|_| refused("current managed definition unavailable"))?;
    let current = rows
        .as_array()
        .ok_or_else(|| refused("current managed definition query malformed"))?
        .iter()
        .filter_map(|raw| serde_json::from_value::<Event>(raw.clone()).ok())
        .filter(|event| {
            event.kind.as_u16() == 30177
                && event.pubkey == owner_key
                && event.verify().is_ok()
                && event.tags.iter().any(|tag| tag.as_slice() == ["d", actor])
        })
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| b.id.cmp(&a.id))
        })
        .ok_or_else(|| refused("managed identity current definition absent or retired"))?;
    let requested_project = match &payload.action {
        CodingSessionLifecycleAction::SessionCreate { project_ref, .. } => project_ref.as_deref(),
        _ => None,
    };
    if value.get("projectRef").and_then(Value::as_str) != requested_project {
        return Err(refused(
            "start must preserve the protected project association",
        ));
    }
    if state.sessions().any(|record| {
        record.actor.as_deref() == Some(actor)
            && config
                .state_dir
                .join(format!("stop-quiescence-{}.json", record.session_id))
                .exists()
    }) {
        return Err(refused(
            "a responsibility has unconfirmed process quiescence; explicit recovery is required",
        ));
    }
    let ownership_context = crate::host_ownership::context()?;
    let _admission_lock = ownership_context
        .as_ref()
        .map(|(home, instance)| {
            ownership::try_admission_lock(home, *instance, actor, &config.relay_url)
                .map_err(|_| refused("another ownership transition is in progress"))
        })
        .transpose()?;
    let revision = current.id.to_hex();
    value["definition"] =
        serde_json::to_value(current).map_err(|_| refused("current definition encoding failed"))?;
    write(&dir.join("enrollment.json"), &value)?;
    let mut import_required = false;
    if let Some((home, instance)) = ownership_context {
        if let Some(held) = ownership::for_pair(&home, instance, actor, &config.relay_url)
            .map_err(|_| refused("ownership unavailable"))?
        {
            import_required = held.phase == Phase::HostStopped
                && held.target.is_none()
                && value.get("nativeContextRequired").and_then(Value::as_bool) == Some(true);
            if held.phase == Phase::Adopted && held.operation_id != payload.command_id {
                // An explicitly requested additional Solo responsibility remains
                // host-owned. Only the all-terminal boundary changes the primary journal.
                let active = state.sessions().any(|record| {
                    record.actor.as_deref() == Some(actor) && !record.closed && !record.is_retired()
                });
                if state.sessions().any(|record| {
                    record.actor.as_deref() == Some(actor)
                        && config
                            .state_dir
                            .join(format!("stop-quiescence-{}.json", record.session_id))
                            .exists()
                }) {
                    return Err(refused("a responsibility has unconfirmed process quiescence; explicit recovery is required"));
                }
                if let Some(target) = held.target.as_ref().filter(|_| !active) {
                    require_primary_terminal(config, state, actor, target)?;
                    ownership::host_stopped(&home, instance, actor, &config.relay_url, target)
                        .map_err(|_| refused("stopped ownership could not be recorded"))?;
                }
            } else if held.phase == Phase::HostStopped
                && held.target.is_none()
                && held.legacy_nonce == format!("fresh-identity:{}", held.operation_id)
            {
                if held.definition_event_id != revision {
                    ownership::revise_fresh_configured_enrollment(
                        &home,
                        instance,
                        actor,
                        &config.relay_url,
                        &held.operation_id,
                        &held.definition_event_id,
                        &revision,
                    )
                    .map_err(|_| {
                        refused("configured activation revision could not be reconciled")
                    })?;
                }
                value["activationOperation"] = json!(payload.command_id);
                value["enrolledOperation"] = json!(held.operation_id);
                write(&dir.join("enrollment.json"), &value)?;
            }
        }
    }
    if import_required {
        if value
            .get("replayFloorUnix")
            .and_then(Value::as_u64)
            .is_none()
        {
            return Err(refused(
                "paused import replay boundary missing; no fresh fallback",
            ));
        }
        let source = value
            .get("nativeImportOperation")
            .and_then(Value::as_str)
            .ok_or_else(|| refused("paused native import provenance missing"))?;
        Uuid::parse_str(source).map_err(|_| refused("paused native import operation invalid"))?;
        let native = protected(
            &config
                .state_dir
                .join(format!("native-import-{source}.json")),
        )?
        .ok_or_else(|| refused("paused native context missing; no fresh fallback"))?;
        write(
            &config
                .state_dir
                .join(format!("native-import-{}.json", payload.command_id)),
            &native,
        )?;
    }
    let cwd = value
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| refused("enrolled workspace missing"))?;
    let cwd = cwd
        .canonicalize()
        .map_err(|_| refused("enrolled workspace unavailable"))?;
    let custody = config
        .state_dir
        .canonicalize()
        .map_err(|_| refused("worker custody unavailable"))?;
    if !cwd.is_dir() || cwd.starts_with(&custody) || custody.starts_with(&cwd) {
        return Err(refused("enrolled workspace overlaps worker custody"));
    }
    let seats_path = config
        .actor_seats_file
        .as_deref()
        .ok_or_else(|| refused("actor seat custody path unavailable"))?;
    let mut seats = protected(seats_path)?.unwrap_or_else(|| json!({"version":1,"pending":{}}));
    let pending = seats
        .get_mut("pending")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| refused("actor seat custody malformed"))?;
    pending.insert(
        payload.command_id.clone(),
        json!({"pubkey":actor,"nsec":key.trim(),"authTag":auth.trim(),"relayUrl":config.relay_url}),
    );
    write(seats_path, &seats)?;
    let projects = config
        .projects_file
        .as_deref()
        .ok_or_else(|| refused("workspace hint path unavailable"))?;
    let hints = projects
        .parent()
        .ok_or_else(|| refused("workspace hint parent missing"))?
        .join(beekeeper_host_core::project::PENDING_HINTS_DIR);
    atomic_write::create_dir_all_restricted(&hints)
        .map_err(|_| refused("workspace hint custody unavailable"))?;
    let project = match &payload.action {
        CodingSessionLifecycleAction::SessionCreate { project_ref, .. } => project_ref,
        _ => &None,
    };
    write(
        &hints.join(format!("{}.json", payload.command_id)),
        &json!({"commandId":payload.command_id,"path":cwd,"projectRef":project,"writtenAt":chrono::Utc::now().timestamp()}),
    )?;
    Ok(())
}
fn protected_secret(path: &std::path::Path) -> Result<String, CreateFailure> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| refused("actor custody missing"))?;
    if !metadata.file_type().is_file() || metadata.len() > 4096 {
        return Err(refused("actor custody not a bounded regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(refused("actor custody permissions unsafe"));
        }
    }
    std::fs::read_to_string(path).map_err(|_| refused("actor custody unreadable"))
}
/// Settle ownership and the canonical route before publishing the created receipt.
pub(crate) fn complete(
    config: &Config,
    state: &StateStore,
    plan: &crate::commands::CreatePlan,
    target: &beekeeper_core::coding_session_command::CodingSessionTarget,
) -> Result<(), CreateFailure> {
    let Some(actor) = plan.actor.as_deref() else {
        return Ok(());
    };
    let Some(value) = enrollment(config, actor)? else {
        return Ok(());
    };
    let event = definition(&value, actor, &plan.created_by)?;
    if let Some((home, instance)) = crate::host_ownership::context()? {
        if let Some(mut held) = ownership::for_pair(&home, instance, actor, &config.relay_url)
            .map_err(|_| refused("ownership unavailable"))?
        {
            match held.phase {
                Phase::Quiesced if held.operation_id == plan.command_id => {
                    held.phase = Phase::Adopted;
                    held.target = Some(target.clone());
                    ownership::advance(&home, instance, held)
                        .map_err(|_| refused("import ownership could not be recorded"))?;
                }
                Phase::HostStopped
                    if held.target.is_none()
                        && held.operation_id == plan.command_id
                        && held.legacy_nonce == format!("fresh-identity:{}", held.operation_id) =>
                {
                    ownership::record_host_enrollment(
                        &home,
                        instance,
                        actor,
                        &config.relay_url,
                        &held.operation_id,
                        &event.id.to_hex(),
                        Some(target.clone()),
                    )
                    .map_err(|_| refused("fresh ownership could not be recorded"))?;
                }
                Phase::HostStopped => {
                    if let Some(prior) = held.target.as_ref() {
                        require_primary_terminal(config, state, actor, prior)?;
                    }
                    if state.sessions().any(|record| {
                        record.actor.as_deref() == Some(actor)
                            && record.command_id != plan.command_id
                            && !record.closed
                            && !record.is_retired()
                    }) {
                        return Err(refused(
                            "another responsibility has not reached its terminal boundary",
                        ));
                    }
                    ownership::successor(
                        &home,
                        instance,
                        actor,
                        &config.relay_url,
                        &plan.command_id,
                        &event.id.to_hex(),
                        target.clone(),
                    )
                    .map_err(|_| refused("successor ownership could not be recorded"))?;
                }
                Phase::Adopted => {}
                _ => return Err(refused("ownership transition is not admitted")),
            }
        }
    }
    let path = config.state_dir.join("conversation-routes.json");
    let mut routes: crate::conversation::ConversationRoutes = protected(&path)?
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| refused("conversation routes malformed"))?
        .unwrap_or_default();
    if !routes.routes.iter().any(|route| {
        route.actor_pubkey == actor
            && route.channel_id == plan.channel_id
            && route.target == *target
    }) {
        routes.routes.push(crate::conversation::ConversationRoute {
            actor_pubkey: actor.into(),
            channel_id: plan.channel_id,
            target: target.clone(),
            replay_floor_unix: value.get("replayFloorUnix").and_then(Value::as_u64),
        });
    }
    write(
        &path,
        &serde_json::to_value(routes).map_err(|_| refused("conversation route encoding failed"))?,
    )
}

/// Record the actor-wide stopped boundary only after all worker contexts are terminal.
pub(crate) fn stopped(
    config: &Config,
    state: &StateStore,
    session: &str,
) -> Result<(), CreateFailure> {
    let Some(record) = state.session(session) else {
        return Err(refused("stopped responsibility missing"));
    };
    let Some(actor) = record.actor.as_deref() else {
        return Ok(());
    };
    if state.sessions().any(|row| {
        row.actor.as_deref() == Some(actor)
            && (!row.closed
                || config
                    .state_dir
                    .join(format!("stop-quiescence-{}.json", row.session_id))
                    .exists()
                || row
                    .process_witness
                    .as_ref()
                    .is_none_or(|witness| !witness.proves_absent(&row.target(&config.instance_id))))
    }) {
        return Ok(());
    }
    let Some((home, instance)) = crate::host_ownership::context()? else {
        return Ok(());
    };
    let _guard = ownership::try_admission_lock(&home, instance, actor, &config.relay_url)
        .map_err(|_| refused("ownership transition busy after terminal stop"))?;
    if let Some(held) = ownership::for_pair(&home, instance, actor, &config.relay_url)
        .map_err(|_| refused("stopped ownership unavailable"))?
    {
        if held.phase == Phase::Adopted {
            if let Some(target) = held.target {
                require_primary_terminal(config, state, actor, &target)?;
                ownership::host_stopped(&home, instance, actor, &config.relay_url, &target)
                    .map_err(|_| refused("stopped ownership persistence failed"))?;
            }
        }
    }
    if let Some(mut value) = enrollment(config, actor)? {
        value["desiredRunning"] = json!(false);
        write(
            &config
                .state_dir
                .join("actors")
                .join(actor)
                .join("enrollment.json"),
            &value,
        )?;
    }
    Ok(())
}

fn require_primary_terminal(
    config: &Config,
    state: &StateStore,
    actor: &str,
    target: &beekeeper_core::coding_session_command::CodingSessionTarget,
) -> Result<(), CreateFailure> {
    let record=state.sessions().find(|record|record.actor.as_deref()==Some(actor)&&record.target(&config.instance_id)==*target).ok_or_else(||refused("the exact primary responsibility is absent; reconcile its terminal evidence before restarting"))?;
    if !record.closed
        || config
            .state_dir
            .join(format!("stop-quiescence-{}.json", record.session_id))
            .exists()
        || record
            .process_witness
            .as_ref()
            .is_none_or(|witness| !witness.proves_absent(&record.target(&config.instance_id)))
    {
        return Err(refused(
            "the exact primary responsibility has not established terminal process quiescence",
        ));
    }
    Ok(())
}
/// Follow the primary accepted responsibility's exact generation without changing custody.
pub(crate) fn follow_generation(
    config: &Config,
    actor: Option<&str>,
    previous: &beekeeper_core::coding_session_command::CodingSessionTarget,
    next: &beekeeper_core::coding_session_command::CodingSessionTarget,
) -> Result<(), CreateFailure> {
    let Some(actor) = actor else { return Ok(()) };
    let Some((home, instance)) = crate::host_ownership::context()? else {
        return Ok(());
    };
    let Some(held) = ownership::for_pair(&home, instance, actor, &config.relay_url)
        .map_err(|_| refused("generation ownership unavailable"))?
    else {
        return Ok(());
    };
    // Additional responsibilities keep the canonical primary unchanged.
    if held.target.as_ref() != Some(previous) && held.target.as_ref() != Some(next) {
        return Ok(());
    }
    ownership::follow_generation(&home, instance, actor, &config.relay_url, previous, next)
        .map_err(|_| refused("accepted generation ownership CAS failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn missing_or_pending_primary_cannot_turn_absence_into_terminal_ownership() {
        let root = tempfile::tempdir().expect("fixture");
        let cwd = root.path().join("work");
        std::fs::create_dir(&cwd).expect("cwd");
        let channel = Uuid::new_v4();
        let projects = crate::tests::write_projects(root.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&root.path().join("state"), Some(&projects));
        let event = crate::tests::create_event(&provider, channel, "primary-boundary");
        provider
            .handle_command_event(channel, &event)
            .await
            .expect("actual runtime");
        let target = provider
            .state
            .sessions()
            .next()
            .expect("record")
            .target(&provider.config.instance_id);
        let actor = nostr::Keys::generate().public_key().to_hex();
        assert!(
            require_primary_terminal(&provider.config, &provider.state, &actor, &target).is_err(),
            "missing identity cannot prove stop"
        );
        provider
            .state
            .update_session(&target.session_id, |record| {
                record.actor = Some(actor.clone())
            })
            .expect("actor scope");
        assert!(
            require_primary_terminal(&provider.config, &provider.state, &actor, &target).is_err(),
            "live primary cannot prove stop"
        );
        provider
            .sessions
            .shutdown_and_join(&target.session_id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
        provider
            .state
            .update_session(&target.session_id, |record| record.closed = true)
            .expect("terminal record");
        let pending = provider
            .config
            .state_dir
            .join(format!("stop-quiescence-{}.json", target.session_id));
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(&pending, b"{}")
            .expect("interrupted intent");
        assert!(
            require_primary_terminal(&provider.config, &provider.state, &actor, &target).is_err()
        );
        std::fs::remove_file(pending).expect("verified drain");
        require_primary_terminal(&provider.config, &provider.state, &actor, &target)
            .expect("exact drained primary");
        let mut stale = target.clone();
        stale.generation += 1;
        assert!(
            require_primary_terminal(&provider.config, &provider.state, &actor, &stale).is_err()
        );
    }
}
