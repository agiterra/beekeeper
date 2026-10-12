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
fn retain_replay_boundary(value: &mut Value, enrolled: &Event) {
    if value
        .get("replayFloorUnix")
        .and_then(Value::as_u64)
        .is_none()
    {
        value["replayFloorUnix"] = json!(enrolled.created_at.as_secs());
    }
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
    stage_with_context(
        config,
        state,
        rest,
        channel,
        operator,
        content,
        crate::host_ownership::context()?,
    )
    .await
}
async fn stage_with_context(
    config: &Config,
    state: &StateStore,
    rest: Option<&beekeeper_acp::relay::RestClient>,
    channel: Uuid,
    operator: &str,
    content: &str,
    ownership_context: Option<(std::path::PathBuf, beekeeper_host_core::layout::Instance)>,
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
    let enrolled_definition = definition(&value, actor, &owner)?;
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
    if value.get("nativeContextRequired").and_then(Value::as_bool) == Some(true)
        || value
            .get("nativeImportOperation")
            .is_some_and(|v| !v.is_null())
    {
        return Err(refused(
            "historical imported enrollment is retired; recreate through fresh headless setup",
        ));
    }
    let _admission_lock = ownership_context
        .as_ref()
        .map(|(home, instance)| {
            ownership::try_admission_lock(home, *instance, actor, &config.relay_url)
                .map_err(|_| refused("another ownership transition is in progress"))
        })
        .transpose()?;
    if let Some((home, instance)) = &ownership_context {
        if let Some(held) = ownership::for_pair(home, *instance, actor, &config.relay_url)
            .map_err(|_| refused("ownership unavailable"))?
        {
            if !ownership::is_modern_enrollment(&held)
                || !matches!(held.phase, Phase::Adopted | Phase::HostStopped)
            {
                return Err(refused("historical setup is retired; recreate through fresh headless setup, retained custody was not changed"));
            }
        }
    }
    if state.unresolved_actor_start(Some(actor)) {
        return Err(refused("a managed actor startup outcome is unconfirmed"));
    }
    // The actor-wide enrollment remembers the latest responsibility. A remote
    // owner can also restart a previously accepted, exactly scoped and proved
    // stopped ordinary responsibility without access to this worker's files.
    let cwd =
        if value.get("channelId").and_then(Value::as_str) == Some(channel.to_string().as_str()) {
            value
                .get("cwd")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from)
                .ok_or_else(|| refused("enrolled workspace missing"))?
        } else {
            accepted_workspace(
                config,
                state,
                actor,
                channel,
                provider_instance_ref.as_str(),
                requested_project,
                &owner,
            )?
        };
    let revision = current.id.to_hex();
    // The original protected signed enrollment predates queued inputs. Never
    // substitute the latest revision or start time for this durable boundary.
    retain_replay_boundary(&mut value, &enrolled_definition);
    value["definition"] =
        serde_json::to_value(current).map_err(|_| refused("current definition encoding failed"))?;
    write(&dir.join("enrollment.json"), &value)?;
    if let Some((home, instance)) = ownership_context {
        if let Some(held) = ownership::for_pair(&home, instance, actor, &config.relay_url)
            .map_err(|_| refused("ownership unavailable"))?
        {
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
fn accepted_workspace(
    config: &Config,
    state: &StateStore,
    actor: &str,
    channel: Uuid,
    runtime: &str,
    project: Option<&str>,
    owner: &str,
) -> Result<std::path::PathBuf, CreateFailure> {
    if state.unresolved_actor_start(Some(actor)) {
        return Err(refused("a managed actor startup outcome is unconfirmed"));
    }
    let routes: crate::conversation::ConversationRoutes =
        protected(&config.state_dir.join("conversation-routes.json"))?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| refused("accepted responsibility routes malformed"))?
            .ok_or_else(|| refused("accepted responsibility routes unavailable"))?;
    let mut selected = None;
    for record in state.sessions().filter(|record| {
        record.actor.as_deref() == Some(actor)
            && record.channel_id == channel
            && record.provider_instance_ref == runtime
            && record.project_ref.as_deref() == project
    }) {
        let target = record.target(&config.instance_id);
        if record.created_by.as_deref() != Some(owner)
            || record.role.is_some()
            || record.pack_ref.is_some()
            || record.authority_withdrawn.is_some()
            || record.is_retired()
            || !record.closed
            || state.unresolved_start_target(&target)
            || config
                .state_dir
                .join(format!("stop-quiescence-{}.json", record.session_id))
                .exists()
            || record
                .process_witness
                .as_ref()
                .is_none_or(|witness| !witness.proves_absent(&target))
            || !routes.routes.iter().any(|route| {
                route.actor_pubkey == actor && route.channel_id == channel && route.target == target
            })
        {
            return Err(refused(
                "prior responsibility scope or terminal evidence is unconfirmed",
            ));
        }
        if selected.as_ref().is_some_and(|cwd| cwd != &record.cwd) {
            return Err(refused("prior responsibility workspace is ambiguous"));
        }
        selected = Some(record.cwd.clone());
    }
    selected
        .ok_or_else(|| refused("no accepted responsibility matches requested channel and runtime"))
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
        if let Some(held) = ownership::for_pair(&home, instance, actor, &config.relay_url)
            .map_err(|_| refused("ownership unavailable"))?
        {
            match held.phase {
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

    #[test]
    fn replay_boundary_retains_original_signed_enrollment_across_revision_and_retry() {
        let keys = crate::tests::test_operator_keys();
        let old = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "original")
            .custom_created_at(nostr::Timestamp::from(123))
            .sign_with_keys(keys)
            .expect("signed original");
        let newer = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "updated")
            .custom_created_at(nostr::Timestamp::from(456))
            .sign_with_keys(keys)
            .expect("signed update");
        let mut value = json!({"replayFloorUnix":null});
        retain_replay_boundary(&mut value, &old);
        assert_eq!(value["replayFloorUnix"], 123);
        retain_replay_boundary(&mut value, &newer);
        assert_eq!(value["replayFloorUnix"], 123);
        let mut explicit = json!({"replayFloorUnix":100});
        retain_replay_boundary(&mut explicit, &old);
        assert_eq!(explicit["replayFloorUnix"], 100);
    }

    #[tokio::test]
    async fn historical_custody_without_import_flags_is_refused_before_any_restaging_write() {
        use beekeeper_host_core::{layout::Instance, ownership::Transfer};
        let root = tempfile::tempdir().expect("isolated fixture");
        let home = root.path().join("home");
        atomic_write::create_dir_all_restricted(&beekeeper_host_core::layout::host_dir(
            &home,
            Instance::Dev,
        ))
        .expect("ownership directory");
        let channel = Uuid::new_v4();
        let cwd = root.path().join("work");
        std::fs::create_dir(&cwd).expect("workspace");
        let projects = crate::tests::write_projects(root.path(), channel, &cwd);
        let mut provider = crate::tests::provider(&root.path().join("state"), Some(&projects));
        let owner = crate::tests::test_operator_keys();
        let actor =
            beekeeper_host_core::provision::mint_agent_identity(owner).expect("scoped identity");
        let actor_dir = provider.config.state_dir.join("actors").join(&actor.pubkey);
        atomic_write::create_dir_all_restricted(&actor_dir).expect("actor directory");
        atomic_write::atomic_write_json_restricted(
            &actor_dir.join("key"),
            actor.private_key_nsec.as_bytes(),
        )
        .expect("scoped key");
        atomic_write::atomic_write_json_restricted(
            &actor_dir.join("auth-tag.json"),
            actor.auth_tag.as_bytes(),
        )
        .expect("attestation");
        let old = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "old retained definition")
            .tags([nostr::Tag::parse(["d", &actor.pubkey]).expect("actor")])
            .sign_with_keys(owner)
            .expect("old owner head");
        let current =
            nostr::EventBuilder::new(nostr::Kind::Custom(30177), "new current owner definition")
                .tags(old.tags.iter().cloned())
                .custom_created_at(nostr::Timestamp::from(old.created_at.as_secs() + 1))
                .sign_with_keys(owner)
                .expect("current owner head");
        let enrollment_path = actor_dir.join("enrollment.json");
        write(&enrollment_path,&json!({"actorPubkey":actor.pubkey,"channelId":channel,"providerInstance":"claude-primary","definition":old,"cwd":cwd,"projectRef":null})).expect("historical enrollment without import flags");
        let original_enrollment = std::fs::read(&enrollment_path).expect("enrollment bytes");
        let seats_path = root.path().join("actor-seats.json");
        write(
            &seats_path,
            &json!({"version":1,"pending":{},"retained":"unchanged"}),
        )
        .expect("retained seats");
        provider.config.actor_seats_file = Some(seats_path.clone());
        let original_seats = std::fs::read(&seats_path).expect("seats bytes");
        let command = Uuid::new_v4().to_string();
        let event = crate::tests::create_event(&provider, channel, &command);
        let mut payload: Value = serde_json::from_str(&event.content).expect("existing wire");
        payload["action"]["actor"] = json!(actor.pubkey);
        let content = serde_json::to_string(&payload).expect("actor-only Solo create");
        assert!(decode_coding_session_lifecycle_command(&content).is_ok());
        let hints = projects
            .parent()
            .expect("project parent")
            .join(beekeeper_host_core::project::PENDING_HINTS_DIR);
        atomic_write::create_dir_all_restricted(&hints).expect("hints");
        let hint_path = hints.join(format!("{command}.json"));
        write(&hint_path, &json!({"retained":"unchanged"})).expect("retained hint");
        let original_hint = std::fs::read(&hint_path).expect("hint bytes");
        let (relay, _, server) =
            crate::tests::spawn_test_relay_with_events(&provider.config.keys, vec![current]).await;
        let rest = relay.rest_client();
        for phase in [
            Phase::Prepared,
            Phase::Quiesced,
            Phase::Adopted,
            Phase::HostStopped,
        ] {
            let held = Transfer {
                operation_id: command.clone(),
                actor_pubkey: actor.pubkey.clone(),
                relay_url: provider.config.relay_url.clone(),
                legacy_nonce: "retained-old-owner".into(),
                definition_event_id: old.id.to_hex(),
                target: None,
                phase,
            };
            write(
                &beekeeper_host_core::layout::host_dir(&home, Instance::Dev)
                    .join("managed-ownership.json"),
                &json!({"transfers":[held],"history":[]}),
            )
            .expect("retained historical journal");
            assert!(stage_with_context(
                &provider.config,
                &provider.state,
                Some(&rest),
                channel,
                &owner.public_key().to_hex(),
                &content,
                Some((home.clone(), Instance::Dev))
            )
            .await
            .is_err());
            assert_eq!(
                std::fs::read(&enrollment_path).expect("unchanged enrollment"),
                original_enrollment
            );
            assert_eq!(
                std::fs::read(&seats_path).expect("unchanged seats"),
                original_seats
            );
            assert_eq!(
                std::fs::read(&hint_path).expect("unchanged hint"),
                original_hint
            );
            assert!(!provider.state.is_command_consumed(&command));
            assert_eq!(provider.state.sessions().count(), 0);
        }
        server.abort();
    }
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

#[cfg(test)]
#[path = "managed_enrollment_scope_tests.rs"]
mod scope_tests;
