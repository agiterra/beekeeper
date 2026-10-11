//! Signed public configuration revision and project authorization.
use super::*;

pub(crate) async fn authorize_project(
    client: &BeekeeperClient,
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
        let roster = crate::commands::projects::project_roster(client, project).await?;
        if !roster.iter().any(|(pubkey, role)| {
            pubkey == signer && matches!(role.as_str(), "owner" | "collaborator")
        }) {
            return Err(CliError::Auth("signer has no project writer grant".into()));
        }
    }
    Ok(!crate::commands::project_agents::head_is_private(
        &serde_json::to_value(head).map_err(fail)?,
    ))
}

pub(super) async fn current_head(client: &BeekeeperClient, actor: &str) -> Result<Event, CliError> {
    let owner = owner(client)?;
    let rows = client
        .query_all(json!({"kinds":[30177],"authors":[owner],"#d":[actor]}))
        .await?;
    let mut events: Vec<Event> = rows
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .filter(|event| beekeeper_core::verify_event(event).is_ok())
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
pub async fn update(args: &UpdateArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    crate::validate::validate_hex64(&args.agent_pubkey)?;
    crate::validate::validate_hex64(&args.expected_revision)?;
    let signer = owner(client)?;
    let home = home(&args.home)?;
    let _lock = management_lock(&home)?;
    let prompt = args
        .system_prompt
        .as_ref()
        .map(|value| read_or_stdin(value))
        .transpose()?;
    let request = json!({"type":"update","owner":signer,"relay":client.relay_url(),"agent":args.agent_pubkey,
        "expectedRevision":args.expected_revision,"name":args.display_name,"prompt":prompt,"model":args.model});
    let path = operation_path(&home, args.operation_id)?;
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
            let mut content: ManagedAgentEventContent =
                serde_json::from_str(&head.content).map_err(fail)?;
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
    submit(client, event).await?;
    let effective = current_head(client, &args.agent_pubkey).await?;
    println!(
        "{}",
        json!({"operation_id":args.operation_id,"agent_pubkey":args.agent_pubkey,
        "requestedRevision":event.id.to_hex(),"effectiveRevision":effective.id.to_hex(),
        "conflict":effective.id != event.id,"relay_accepted":true,"host_application":"unconfirmed",
        "host_application_reason":"configuration publication is not an execution receipt; expected revision is an advisory NIP33 head check"})
    );
    if effective.id != event.id {
        return Err(CliError::Conflict(format!(
            "configuration superseded; effective revision is {}",
            effective.id
        )));
    }
    Ok(())
}
