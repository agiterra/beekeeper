//! Read-only exact responsibility observation across owner settings revisions.
use super::*;

/// Observe a previously confirmed responsibility without replaying its Create.
/// Settings may have a newer owner-signed revision; this read never changes the
/// original request, target, journal, or configuration application claim.
pub async fn observe_remote_start(
    args: &RemoteStartRequest,
    prior: &Value,
    client: &impl RelayTransport,
) -> Result<Option<Value>, Error> {
    authorize_scope(args, client).await?;
    let mut targets = Vec::new();
    if let Some(target) = prior.get("target") {
        targets.push(
            serde_json::from_value::<beekeeper_core::coding_session_command::CodingSessionTarget>(
                target.clone(),
            )
            .map_err(local::fail)?,
        );
    } else if let Some(rows) = prior.get("responsibilities").and_then(Value::as_array) {
        for row in rows {
            let row: crate::lifecycle::Responsibility =
                serde_json::from_value(row.clone()).map_err(local::fail)?;
            if row.channel_id != args.channel || row.provider_authority != args.provider_authority {
                return Err(Error::Conflict(
                    "prior result belongs to another execution scope".into(),
                ));
            }
            targets.push(row.target);
        }
    }
    if targets.is_empty() {
        return Ok(None);
    }
    let scope = LifecycleRequest {
        agent_pubkey: args.agent_pubkey.clone(),
        channels: vec![args.channel],
        ..Default::default()
    };
    let rows = crate::lifecycle::inventory(&scope, client).await?;
    let metadata = client
        .query_all(json!({"kinds":[44223],"authors":[args.provider_authority],"#h":[args.channel]}))
        .await?;
    for target in &targets {
        let Some(row) = rows.iter().find(|row| {
            row.channel_id == args.channel
                && row.provider_authority == args.provider_authority
                && row.target == *target
        }) else {
            return Ok(None);
        };
        if crate::lifecycle::terminal(row, client).await? {
            return Ok(None);
        }
        if !matching_metadata(args, row, &metadata)? {
            return Ok(None);
        }
    }
    let observation = crate::lifecycle::status(&scope, client).await?;
    let available = targets.iter().all(|target| {
        observation["responsibilities"]
            .as_array()
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry["responsibility"]["target"] == json!(target)
                        && entry["responsibility"]["channelId"] == args.channel.to_string()
                        && entry["responsibility"]["providerAuthority"] == args.provider_authority
                        && entry["runtime_available"] == true
                })
            })
    });
    Ok(Some(
        json!({"agent_pubkey":args.agent_pubkey,"execution_created":false,"already_adopted":true,
        "runtime_available":available,"pending":!available,"configuration_application":"unconfirmed",
        "availability":if available {"verified_unexpired_provider_lease"} else {"no_current_live_lease"},
        "observation":observation,"responsibilities":rows.into_iter().filter(|row|targets.contains(&row.target)&&row.channel_id==args.channel).collect::<Vec<_>>()}),
    ))
}

/// Resolve an accepted exact target's actual conversation channel without
/// changing its immutable enrollment or pending Start request. Both active and
/// stopped targets require current signed metadata and retained authorization;
/// absence never falls back to the configured primary channel.
pub async fn resolve_remote_start_channel(
    args: &RemoteStartRequest,
    target: &beekeeper_core::coding_session_command::CodingSessionTarget,
    client: &impl RelayTransport,
) -> Result<Uuid, Error> {
    management::configuration::current_head(client, &args.agent_pubkey).await?;
    crate::lifecycle::provider_authorized(client, &args.agent_pubkey, &args.provider_authority)
        .await?;
    let scope = LifecycleRequest {
        agent_pubkey: args.agent_pubkey.clone(),
        channels: vec![args.channel],
        ..Default::default()
    };
    let rows = crate::lifecycle::inventory(&scope, client).await?;
    let matches: Vec<_> = rows.iter().filter(|row| row.target == *target).collect();
    if matches.is_empty() {
        return Err(Error::Unconfirmed("accepted target has no current signed channel metadata; configured channel was not substituted".into()));
    }
    if matches.len() != 1 || matches[0].provider_authority != args.provider_authority {
        return Err(Error::Conflict(
            "accepted target channel or execution owner is ambiguous".into(),
        ));
    }
    let row = matches[0];
    let mut resolved = args.clone();
    resolved.channel = row.channel_id;
    authorize_scope(&resolved, client).await?;
    let metadata = client
        .query_all(
            json!({"kinds":[44223],"authors":[args.provider_authority],"#h":[row.channel_id]}),
        )
        .await?;
    if !matching_metadata(&resolved, row, &metadata)? {
        return Err(Error::Unconfirmed("accepted target has no exact current runtime/project metadata; configured channel was not substituted".into()));
    }
    Ok(row.channel_id)
}

async fn authorize_scope(
    args: &RemoteStartRequest,
    client: &impl RelayTransport,
) -> Result<(), Error> {
    management::configuration::current_head(client, &args.agent_pubkey).await?;
    crate::lifecycle::provider_authorized(client, &args.agent_pubkey, &args.provider_authority)
        .await?;
    if let Some(project) = args.project.as_deref() {
        if !management::authorize_project(client, project, &client.keys().public_key().to_hex())
            .await?
        {
            management::configuration::require_private_channel(client, args.channel).await?;
        }
    }
    let members = channel_members(client, args.channel).await?;
    if ![
        client.keys().public_key().to_hex(),
        args.agent_pubkey.clone(),
        args.provider_authority.clone(),
    ]
    .iter()
    .all(|key| members.contains(key))
    {
        return Err(Error::Refused(
            "caller, actor and provider must retain requested channel membership".into(),
        ));
    }
    Ok(())
}

fn matching_metadata(
    args: &RemoteStartRequest,
    row: &crate::lifecycle::Responsibility,
    metadata: &[Value],
) -> Result<bool, Error> {
    let event = metadata
        .iter()
        .filter_map(|raw| serde_json::from_value::<Event>(raw.clone()).ok())
        .find(|event| {
            event.id.to_hex() == row.metadata_event_id
                && event.verify().is_ok()
                && event.pubkey.to_hex() == args.provider_authority
                && event.kind.as_u16() == 44223
        });
    let Some(event) = event else {
        return Ok(false);
    };
    let content =
        beekeeper_core::coding_session_payload::decode_coding_session_metadata(&event.content)
            .map_err(|_| Error::Unconfirmed("accepted target metadata is malformed".into()))?;
    if content.provider.as_ref().map(|alias| alias.as_str())
        != Some(args.provider_instance.as_str())
        || content.project_ref != args.project
        || content.agent_ref.as_deref() != Some(args.agent_pubkey.as_str())
        || content.session != row.target
    {
        return Err(Error::Conflict(
            "prior responsibility runtime or project scope changed".into(),
        ));
    }
    Ok(true)
}
