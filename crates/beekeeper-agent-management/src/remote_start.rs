//! Public existing-actor lifecycle requests; executor private custody never leaves its worker.
use crate::{
    management::{self, local},
    Context, Error, LifecycleRequest, RelayTransport,
};
use beekeeper_core::{
    coding_session_identity::ProviderInstanceAlias,
    coding_session_lifecycle_command::{
        CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
        CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
    },
};
use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

/// An existing enrolled actor may be started by its owner without executor files or socket.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteStartRequest {
    /// Stable operation UUID, persisted before publication.
    pub operation_id: Uuid,
    /// Existing scoped actor identity.
    pub agent_pubkey: String,
    /// Exact current owner definition revision; no configuration changes are implicit.
    pub expected_revision: String,
    /// Enrolled conversation channel.
    pub channel: Uuid,
    /// Existing worker authority, verified against owner-attested profiles.
    pub provider_authority: String,
    /// Worker runtime alias already enrolled in protected custody.
    pub provider_instance: String,
    /// Existing authorized project association; private coordinates require a private transport.
    pub project: Option<String>,
    /// Maximum wait for a signed worker create result.
    pub wait_secs: u64,
}
#[derive(Serialize, Deserialize)]
struct Operation {
    request: Value,
    event: Event,
    #[serde(default)]
    memberships: Vec<Event>,
    #[serde(default)]
    result: Option<Value>,
}

/// Start an enrolled actor through existing44221 and verified44224 receipts.
/// Only the caller's operation journal is read; no HostConfig, actor key or host socket is needed.
pub async fn start_remote(
    args: &RemoteStartRequest,
    client: &impl RelayTransport,
    context: &Context,
) -> Result<Value, Error> {
    crate::support::validate_hex64(&args.agent_pubkey)?;
    crate::support::validate_hex64(&args.expected_revision)?;
    crate::support::validate_hex64(&args.provider_authority)?;
    if args.operation_id.is_nil() || args.wait_secs == 0 || args.wait_secs > 300 {
        return Err(Error::Usage(
            "remote start requires an operation UUID and wait 1..300 seconds".into(),
        ));
    }
    let _lock = local::management_lock(context)?;
    let head = management::configuration::current_head(client, &args.agent_pubkey).await?;
    if head.id.to_hex() != args.expected_revision {
        return Err(Error::Conflict(
            "remote start definition revision changed; no execution requested".into(),
        ));
    }
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
    if !members
        .iter()
        .any(|key| key == &client.keys().public_key().to_hex())
    {
        return Err(Error::Refused(
            "caller must join the requested conversation channel".into(),
        ));
    }
    let request = serde_json::to_value(args).map_err(local::fail)?;
    let path = local::operation_path(context, args.operation_id)?
        .with_file_name(format!("remote-start-{}.json", args.operation_id));
    let mut operation: Operation = if path.exists() {
        let held: Operation = serde_json::from_slice(&std::fs::read(&path).map_err(local::fail)?)
            .map_err(local::fail)?;
        if held.request != request {
            return Err(Error::Conflict(
                "remote operation payload changed; original request preserved".into(),
            ));
        }
        held
    } else {
        let scope = LifecycleRequest {
            agent_pubkey: args.agent_pubkey.clone(),
            channels: vec![args.channel],
            ..Default::default()
        };
        // One read per identical filter for this pre-publish inventory: an
        // actor with history otherwise repeats them per row (ledger 403).
        let reads = crate::read_cache::ReadCache::new(client);
        let rows = crate::lifecycle::inventory(&scope, &reads).await?;
        let mut active = Vec::new();
        for row in rows {
            // Inventory remains actor-wide for uncertainty. A healthy DM
            // responsibility does not fulfill an explicit stream responsibility.
            let terminal = crate::lifecycle::terminal(&row, &reads).await?;
            if !terminal && row.channel_id == args.channel {
                if row.provider_authority != args.provider_authority {
                    return Err(Error::Conflict(
                        "requested channel already has another execution owner".into(),
                    ));
                }
                active.push(row)
            } else if !terminal
                && !matches!(
                    row.status,
                    beekeeper_core::coding_session_payload::SessionStatus::Idle
                        | beekeeper_core::coding_session_payload::SessionStatus::Running
                        | beekeeper_core::coding_session_payload::SessionStatus::WaitingForInput
                        | beekeeper_core::coding_session_payload::SessionStatus::Interrupted
                )
            {
                return Err(Error::Unconfirmed("another responsibility has unconfirmed startup or quiescence; no additional execution requested".into()));
            }
        }
        if !active.is_empty() {
            return observe_remote_start(args, &json!({"responsibilities":active}), client).await?
                .ok_or_else(|| Error::Unconfirmed("existing requested-channel responsibility has no exact current runtime/project observation; no execution requested".into()));
        }
        let content: beekeeper_core::managed_agent::ManagedAgentEventContent =
            serde_json::from_str(&head.content).map_err(local::fail)?;
        let payload = CodingSessionLifecycleCommandPayload {
            schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: args.operation_id.to_string(),
            action: CodingSessionLifecycleAction::SessionCreate {
                project_ref: args.project.clone(),
                repo_ref: None,
                session_ref: None,
                genesis_ref: None,
                provider_instance_ref: ProviderInstanceAlias::from_wire(&args.provider_instance)
                    .map_err(local::fail)?,
                provider_authority_pubkey: args.provider_authority.clone(),
                model: content.model,
                title: Some(content.name),
                initial_turn: None,
                actor: Some(args.agent_pubkey.clone()),
                role: None,
                hire_ref: None,
                routing: None,
            },
        };
        let event = client.sign_event_unchecked(
            beekeeper_sdk::build_coding_session_lifecycle_command(args.channel, &payload)
                .map_err(local::fail)?,
        )?;
        let mut memberships = Vec::new();
        for participant in [&args.agent_pubkey, &args.provider_authority] {
            if !members.iter().any(|key| key == participant) {
                memberships.push(
                    client.sign_event_unchecked(
                        beekeeper_sdk::build_add_member(
                            args.channel,
                            participant,
                            Some(beekeeper_sdk::MemberRole::Bot),
                        )
                        .map_err(local::fail)?,
                    )?,
                );
            }
        }
        let held = Operation {
            request,
            event,
            memberships,
            result: None,
        };
        local::save(&path, &held)?;
        held
    };
    if let Some(result) = &operation.result {
        return crate::lifecycle::observe_created_start(
            result.clone(),
            args.channel,
            &args.provider_authority,
            client,
        )
        .await;
    }
    for membership in &operation.memberships {
        management::submit(client, membership).await?;
    }
    let members = channel_members(client, args.channel).await?;
    if ![&args.agent_pubkey, &args.provider_authority]
        .iter()
        .all(|participant| members.iter().any(|key| key == *participant))
    {
        return Err(Error::Unconfirmed(
            "requested channel membership remains pending; retry the same operation".into(),
        ));
    }
    management::submit(client, &operation.event).await?;
    let result=match crate::receipts::await_create_receipt(client,&args.channel.to_string(),&args.operation_id.to_string(),&args.provider_authority,args.wait_secs).await {
        crate::receipts::CreateWaitOutcome::Confirmed{target,receipt_event_id,..}=>json!({"operation_id":args.operation_id,"agent_pubkey":args.agent_pubkey,"revision":args.expected_revision,"target":target,"receipt_event_id":receipt_event_id,"host_admitted":true,"ownership":"worker_reconciled","runtime_account":"unconfirmed","configuration_application":"unconfirmed"}),
        crate::receipts::CreateWaitOutcome::Refused{receipt_event_id,error}=>return Err(Error::Refused(format!("remote start refused in receipt {receipt_event_id}: {}",serde_json::to_string(&error).map_err(local::fail)?))),
        crate::receipts::CreateWaitOutcome::Conflicted{detail}=>return Err(Error::Conflict(detail)),
        crate::receipts::CreateWaitOutcome::Unconfirmed{query_failed}=>return Err(Error::Unconfirmed(format!("remote start remains pending (receipt_query_failed={query_failed}); retry the same operation"))),
    };
    operation.result = Some(result.clone());
    local::save(&path, &operation)?;
    crate::lifecycle::observe_created_start(result, args.channel, &args.provider_authority, client)
        .await
}
#[path = "remote_start_observe.rs"]
mod observe;
pub use observe::{observe_remote_start, resolve_remote_start_channel};

/// Only relay-signed current membership can authorize channel-scoped work.
async fn channel_members(
    client: &impl RelayTransport,
    channel: Uuid,
) -> Result<Vec<String>, Error> {
    let relay = client.relay_identity().await?;
    let rows = client
        .query_all(json!({"kinds":[39002],"#d":[channel]}))
        .await?;
    let membership = rows
        .into_iter()
        .filter_map(|row| serde_json::from_value::<Event>(row).ok())
        .filter(|event| {
            event.kind.as_u16() == 39002
                && event.pubkey.to_hex() == relay
                && event.verify().is_ok()
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["d", &channel.to_string()])
        })
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| b.id.cmp(&a.id))
        })
        .ok_or_else(|| Error::Refused("requested channel membership unavailable".into()))?;
    Ok(membership
        .tags
        .iter()
        .filter_map(|tag| {
            (tag.as_slice().first().is_some_and(|kind| kind == "p"))
                .then(|| tag.as_slice().get(1).cloned())
                .flatten()
        })
        .collect())
}

/// Restart first proves every observed responsibility terminal using the shared actor-wide stop.
pub async fn restart_remote(
    stop: &LifecycleRequest,
    start: &RemoteStartRequest,
    client: &impl RelayTransport,
    context: &Context,
) -> Result<Value, Error> {
    if stop.agent_pubkey != start.agent_pubkey || stop.operation_id == start.operation_id {
        return Err(Error::Usage(
            "restart needs the same actor and distinct stop/start operations".into(),
        ));
    }
    let result = crate::lifecycle::stop(stop, client, context).await?;
    if result.get("terminal").and_then(Value::as_bool) != Some(true) {
        return Ok(json!({"terminal":false,"stop":result,"start":"not_admitted"}));
    }
    start_remote(start, client, context).await
}

#[cfg(test)]
#[path = "remote_start_tests.rs"]
mod tests;
