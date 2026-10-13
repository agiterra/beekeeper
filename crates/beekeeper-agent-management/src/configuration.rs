//! Signed public configuration revision and project authorization.
use super::*;

/// Verify current project writer/visibility authority before private enrollment.
pub async fn authorize_project(
    client: &impl RelayTransport,
    project: &str,
    signer: &str,
) -> Result<bool, CliError> {
    let mut parts = project.splitn(3, ':');
    parts.next();
    let creator = parts
        .next()
        .ok_or_else(|| fail("project creator missing"))?;
    let slug = parts.next().ok_or_else(|| fail("project slug missing"))?;
    let rows = client
        .query_all(json!({"kinds":[30621],"authors":[creator],"#d":[slug],"limit":1}))
        .await?;
    let head = rows
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .find(|event| {
            event.pubkey.to_hex() == creator && beekeeper_core::verify_event(event).is_ok()
        })
        .ok_or_else(|| {
            CliError::Refused(
                "project authority/visibility is unreadable; nothing was enrolled".into(),
            )
        })?;
    if signer != creator {
        let roster = crate::project_roster(client, project).await?;
        if !roster.iter().any(|(pubkey, role)| {
            pubkey == signer && matches!(role.as_str(), "owner" | "collaborator")
        }) {
            return Err(CliError::Auth("signer has no project writer grant".into()));
        }
    }
    Ok(!crate::head_is_private(
        &serde_json::to_value(head).map_err(fail)?,
    ))
}

pub(crate) async fn current_head(
    client: &impl RelayTransport,
    actor: &str,
) -> Result<Event, CliError> {
    let owner = owner(client)?;
    let rows = client
        .query_all(json!({"kinds":[30177],"authors":[owner],"#d":[actor]}))
        .await?;
    let mut events: Vec<Event> = rows
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .filter(|event| {
            event.pubkey == client.keys().public_key()
                && event.kind.as_u16() == 30177
                && event.tags.iter().any(|tag| {
                    tag.as_slice().first().is_some_and(|v| v == "d")
                        && tag.as_slice().get(1).is_some_and(|v| v == actor)
                })
                && beekeeper_core::verify_event(event).is_ok()
        })
        .collect();
    events.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    events
        .into_iter()
        .next()
        .ok_or_else(|| CliError::NotFound("no managed-agent head signed by this owner".into()))
}

/// Publish a conditional owner revision; relay storage is distinct from host application.
pub async fn update(
    args: &UpdateRequest,
    client: &impl RelayTransport,
    context: &Context,
) -> Result<Value, CliError> {
    update_guarded(args, client, context, None).await
}

/// Conditionally update local private settings against exact prior protected bytes.
/// The expected hash is bound into the same immutable operation journal. A retry
/// may reconcile only its original snapshot or exact already-committed outcome;
/// it cannot overwrite intervening private changes. Public-only update is unchanged.
pub async fn update_if_private_unchanged(
    args: &UpdateRequest,
    client: &impl RelayTransport,
    context: &Context,
    expected_snapshot_hash: &str,
) -> Result<Value, CliError> {
    crate::validate_hex64(expected_snapshot_hash)?;
    if args.private_config.is_none() {
        return Err(CliError::Usage(
            "conditional private update requires private settings".into(),
        ));
    }
    update_guarded(args, client, context, Some(expected_snapshot_hash)).await
}

/// Read existing restricted actor configuration with signed owner/community scope.
/// This never creates/defaults a snapshot or exports identity keys.
pub fn private_configuration_snapshot(
    context: &Context,
    client: &impl RelayTransport,
    actor: &str,
) -> Result<crate::PrivateConfigurationSnapshot, CliError> {
    let (_, bytes) = super::private_config::snapshot_bytes(context, client, actor)?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| CliError::Refused("private snapshot malformed".into()))?;
    let definition: Event = serde_json::from_value(
        value
            .get("definition")
            .cloned()
            .ok_or_else(|| CliError::Refused("private snapshot definition missing".into()))?,
    )
    .map_err(|_| CliError::Refused("private snapshot definition malformed".into()))?;
    if value["schema"] != "beekeeper-private-agent-config/v1"
        || value["actorPubkey"] != actor
        || value["relayUrl"]
            .as_str()
            .map(|relay| record::canonical_relay_key(&crate::to_ws_url(relay)))
            != Some(record::canonical_relay_key(&crate::to_ws_url(
                client.relay_url(),
            )))
        || definition.pubkey != client.keys().public_key()
        || definition.kind.as_u16() != 30177
        || definition.verify().is_err()
        || definition
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("d"))
            .count()
            != 1
        || !definition
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["d", actor])
    {
        return Err(CliError::Auth(
            "private snapshot definition/scope authority mismatch".into(),
        ));
    }
    let private = crate::PrivateConfiguration {
        record: value
            .get("record")
            .cloned()
            .ok_or_else(|| CliError::Refused("private snapshot record missing".into()))?,
    };
    super::private_config::reject_owner_secret(&private, client.keys())?;
    // Reuse the same canonical enrollment/custody validation as materialization.
    let (_, enrollment) = super::private_config::prepare_update(context, client, actor, &private)?;
    super::private_config::validate_enrollment_definition(client, actor, &enrollment)?;
    let enrollment_definition: Event =
        serde_json::from_value(enrollment["definition"].clone()).map_err(fail)?;
    Ok(crate::PrivateConfigurationSnapshot {
        snapshot_hash: bytes_hash(&bytes),
        definition,
        enrollment_definition,
        private,
    })
}
fn bytes_hash(bytes: &[u8]) -> String {
    use nostr::hashes::{sha256, Hash};
    sha256::Hash::hash(bytes).to_string()
}
async fn update_guarded(
    args: &UpdateRequest,
    client: &impl RelayTransport,
    context: &Context,
    expected_snapshot_hash: Option<&str>,
) -> Result<Value, CliError> {
    crate::validate_hex64(&args.agent_pubkey)?;
    crate::validate_hex64(&args.expected_revision)?;
    let signer = owner(client)?;
    if let Some(private) = &args.private_config {
        super::private_config::reject_owner_secret(private, client.keys())?;
    }
    if args.home.as_ref().is_some_and(|home| home != &context.home) {
        return Err(CliError::Usage(
            "request home conflicts with explicit context".into(),
        ));
    }
    let _lock = management_lock(context)?;
    // Provider enrollment writes use the same pair lock. Conditional refresh
    // holds it after the management lock through raw snapshot readback/commit.
    let _pair = expected_snapshot_hash
        .map(|_| {
            beekeeper_host_core::ownership::admission_lock(
                &context.home,
                context.instance,
                &args.agent_pubkey,
                &crate::to_ws_url(client.relay_url()),
            )
            .map_err(fail)
        })
        .transpose()?;
    let prompt = args.system_prompt.clone();
    let mut request = json!({"type":"update","owner":signer,"relay":client.relay_url(),"agent":args.agent_pubkey,
        "expectedRevision":args.expected_revision,"name":args.display_name,"prompt":prompt,"model":args.model,"publicDefinition":args.public_definition,"privateConfig":args.private_config});
    if let Some(hash) = expected_snapshot_hash {
        request["expectedPrivateSnapshotHash"] = json!(hash);
    }
    let path = operation_path(context, args.operation_id)?;
    let operation = match read_operation(&path, &request)? {
        Some(operation) => operation,
        None => {
            if let Some(hash) = expected_snapshot_hash {
                if private_configuration_snapshot(context, client, &args.agent_pubkey)?
                    .snapshot_hash
                    != hash
                {
                    return Err(CliError::Conflict("private configuration changed before refresh; original snapshot/operation preserved".into()));
                }
            }
            let head = current_head(client, &args.agent_pubkey).await?;
            if head.id.to_hex() != args.expected_revision {
                return Err(CliError::Conflict(format!(
                    "stale revision; effective revision is {}",
                    head.id
                )));
            }
            let mut content: ManagedAgentEventContent = match &args.public_definition {
                Some(content) => content.clone(),
                None => serde_json::from_str(&head.content).map_err(fail)?,
            };
            if let Some(name) = &args.display_name {
                content.name = name.clone();
            }
            if let Some(prompt) = prompt {
                content.system_prompt = Some(prompt);
            }
            if let Some(model) = &args.model {
                content.model = Some(model.clone());
            }
            if content.persona_id.is_some()
                && (args.system_prompt.is_some() || args.model.is_some())
            {
                return Err(CliError::Usage(
                    "linked definitions own prompt/model; update that definition instead".into(),
                ));
            }
            let event = client.sign_event_unchecked(
                beekeeper_sdk::managed_agent::build_managed_agent_event(
                    &args.agent_pubkey,
                    &content,
                )
                .map_err(fail)?
                .custom_created_at(Timestamp::from(std::cmp::max(
                    chrono::Utc::now().timestamp() as u64,
                    head.created_at.as_secs().saturating_add(1),
                ))),
            )?;
            let operation = Operation {
                request: request.clone(),
                actor_pubkey: args.agent_pubkey.clone(),
                events: vec![event],
                target: None,
                receipt_event_id: None,
                predecessor: None,
                custody_handoff: None,
            };
            save(&path, &operation)?;
            operation
        }
    };
    let event = operation
        .events
        .first()
        .ok_or_else(|| fail("operation has no configuration event"))?;
    let before = current_head(client, &args.agent_pubkey).await?;
    if before.id != event.id && before.id.to_hex() != args.expected_revision {
        return Err(CliError::Conflict(format!(
            "stale revision; effective revision is {}",
            before.id
        )));
    }
    // Private runtime settings require existing executor custody. Validate
    // before publication; public-only remote updates require no executor files.
    let private_boundary = if let Some(private) = &args.private_config {
        Some(super::private_config::prepare_update(
            context,
            client,
            &args.agent_pubkey,
            private,
        )?)
    } else {
        None
    };
    if let Some(hash) = expected_snapshot_hash {
        let snapshot = private_configuration_snapshot(context, client, &args.agent_pubkey)?;
        if snapshot.snapshot_hash != hash {
            let (config, enrollment) = private_boundary
                .as_ref()
                .ok_or_else(|| fail("conditional private boundary missing"))?;
            let desired = super::private_config::update_snapshot(
                config,
                enrollment,
                event,
                args.private_config
                    .as_ref()
                    .ok_or_else(|| fail("conditional private settings missing"))?,
            )?;
            let desired_bytes = serde_json::to_vec_pretty(&desired).map_err(fail)?;
            if snapshot.snapshot_hash != bytes_hash(&desired_bytes) {
                return Err(CliError::Conflict("private configuration changed during retained operation; retry cannot override it".into()));
            }
        }
    }
    submit(client, event).await?;
    let effective = current_head(client, &args.agent_pubkey).await?;
    if effective.id != event.id {
        return Err(CliError::Conflict(format!(
            "configuration superseded; effective revision is {}",
            effective.id
        )));
    }
    if let Some((config, enrollment)) = private_boundary {
        super::private_config::commit_update(
            &config,
            &enrollment,
            event,
            args.private_config
                .as_ref()
                .ok_or_else(|| fail("private configuration missing"))?,
        )?;
    }
    Ok(
        json!({"operation_id":args.operation_id,"agent_pubkey":args.agent_pubkey,
        "requestedRevision":event.id.to_hex(),"effectiveRevision":effective.id.to_hex(),
        "conflict":false,"relay_accepted":true,"host_application":"unconfirmed",
        "private_materialization":if args.private_config.is_some() {"accepted_local_snapshot_next_runtime_boundary"}else{"not_requested"},
        "host_autostart_policy":"not_confirmed"}),
    )
}

pub(crate) async fn require_private_channel(
    client: &impl RelayTransport,
    channel: Uuid,
) -> Result<(), CliError> {
    let relay = client.relay_identity().await?;
    let rows = client
        .query_all(json!({"kinds":[39000],"authors":[relay],"#d":[channel]}))
        .await?;
    let head = rows
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .filter(|event| {
            event.kind.as_u16() == 39000
                && event.pubkey.to_hex() == relay
                && event.verify().is_ok()
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["d", &channel.to_string()])
        })
        .max_by_key(|event| (event.created_at, std::cmp::Reverse(event.id)))
        .ok_or_else(|| {
            CliError::Unconfirmed("private project work channel visibility unavailable".into())
        })?;
    if !head.tags.iter().any(|tag| tag.as_slice() == ["private"]) {
        return Err(CliError::Refused(
            "private project metadata cannot be published into an open work channel".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "configuration_cas_tests.rs"]
mod cas_tests;
