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
    let prompt = args.system_prompt.clone();
    let request = json!({"type":"update","owner":signer,"relay":client.relay_url(),"agent":args.agent_pubkey,
        "expectedRevision":args.expected_revision,"name":args.display_name,"prompt":prompt,"model":args.model,"publicDefinition":args.public_definition,"privateConfig":args.private_config});
    let path = operation_path(context, args.operation_id)?;
    let operation = match read_operation(&path, &request)? {
        Some(operation) => operation,
        None => {
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
