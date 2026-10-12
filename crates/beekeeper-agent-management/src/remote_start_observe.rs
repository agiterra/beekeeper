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
        let matches = metadata
            .iter()
            .filter_map(|raw| serde_json::from_value::<Event>(raw.clone()).ok())
            .find(|event| event.id.to_hex() == row.metadata_event_id && event.verify().is_ok());
        let Some(event) = matches else {
            return Ok(None);
        };
        let content =
            beekeeper_core::coding_session_payload::decode_coding_session_metadata(&event.content)
                .map_err(local::fail)?;
        if content.provider.as_ref().map(|alias| alias.as_str())
            != Some(args.provider_instance.as_str())
            || content.project_ref != args.project
            || content.agent_ref.as_deref() != Some(args.agent_pubkey.as_str())
        {
            return Err(Error::Conflict(
                "prior responsibility runtime or project scope changed".into(),
            ));
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
