//! Actor-wide management through existing signed session lifecycle events.
use crate::management::{self, local};
use crate::{Context, Error, RelayTransport};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_payload::ReceiptStatus;
use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Actor-wide lifecycle operation; channel hints never restrict the discovered actor inventory.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LifecycleRequest {
    /// Stable operation UUID.
    pub operation_id: Uuid,
    /// Scoped managed identity.
    pub agent_pubkey: String,
    /// Known channels, supplemented by current relay membership discovery.
    pub channels: Vec<Uuid>,
    /// Bound receipt wait in seconds.
    pub wait_secs: u64,
}
/// Owner-authorized retirement bound to the exact intended current definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeleteRequest {
    /// Stable immutable operation UUID.
    pub operation_id: Uuid,
    /// Scoped identity being retired.
    pub agent_pubkey: String,
    /// Exact owner30177 revision intended for deletion.
    pub expected_revision: String,
    /// Known channels supplemented by current relay membership discovery.
    pub channels: Vec<Uuid>,
    /// Bounded receipt wait in seconds.
    pub wait_secs: u64,
    /// Remote info/deploy-only substrate cannot certify termination.
    pub remote_deployment: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Responsibility {
    pub(crate) channel_id: Uuid,
    pub(crate) target: CodingSessionTarget,
    pub(crate) provider_authority: String,
    pub(crate) metadata_event_id: String,
    pub(crate) status: beekeeper_core::coding_session_payload::SessionStatus,
}
#[derive(Serialize, Deserialize)]
struct LifecycleOperation {
    request: Value,
    events: Vec<Event>,
    #[serde(default)]
    inventory: Vec<Responsibility>,
}

pub(crate) async fn inventory(
    args: &LifecycleRequest,
    client: &impl RelayTransport,
) -> Result<Vec<Responsibility>, Error> {
    crate::support::validate_hex64(&args.agent_pubkey)?;
    let relay = client.relay_identity().await?;
    let groups = client
        .query_all(json!({"kinds":[39002],"#p":[args.agent_pubkey]}))
        .await?;
    let mut channels = args.channels.clone();
    for group in groups {
        let Ok(event) = serde_json::from_value::<Event>(group) else {
            continue;
        };
        if event.kind.as_u16() != 39002 || event.pubkey.to_hex() != relay || event.verify().is_err()
        {
            continue;
        }
        if !event.tags.iter().any(|t| {
            let v = t.as_slice();
            v.first().is_some_and(|v| v == "p") && v.get(1) == Some(&args.agent_pubkey)
        }) {
            continue;
        }
        for tag in event.tags.iter() {
            let t = tag.as_slice();
            if t.first().is_some_and(|v| v == "d") {
                if let Some(channel) = t.get(1).and_then(|v| Uuid::parse_str(v).ok()) {
                    if !channels.contains(&channel) {
                        channels.push(channel)
                    }
                }
            }
        }
    }
    if channels.is_empty() {
        return Err(Error::Unconfirmed(
            "actor channel inventory unavailable; zero work cannot be certified".into(),
        ));
    }
    let mut records = BTreeMap::new();
    let mut uncertain = Vec::new();
    for channel in channels {
        uncertain.extend(
            crate::lifecycle_uncertain::inventory(client, &args.agent_pubkey, channel).await?,
        );
        let rows = client
            .query_all(json!({"kinds":[44223],"#h":[channel]}))
            .await?;
        for raw in rows {
            let Ok(event) = serde_json::from_value::<Event>(raw) else {
                continue;
            };
            if event.kind.as_u16() != 44223
                || event.verify().is_err()
                || !event
                    .tags
                    .iter()
                    .any(|t| t.as_slice() == ["h", &channel.to_string()])
            {
                continue;
            }
            let Ok(content) =
                beekeeper_core::coding_session_payload::decode_coding_session_metadata(
                    &event.content,
                )
            else {
                continue;
            };
            if content.agent_ref.as_deref() != Some(&args.agent_pubkey) {
                continue;
            }
            let target = content.session.clone();
            let channel_text = channel.to_string();
            let target_key =
                beekeeper_core::coding_session_command::coding_session_target_key(&target);
            let semantic_key =
                beekeeper_sdk::coding_session::coding_session_metadata_semantic_key(&target);
            let expected = [
                ["h", channel_text.as_str()],
                [
                    "csm-v",
                    beekeeper_sdk::coding_session::CODING_SESSION_METADATA_TAG_VERSION,
                ],
                ["cs-target", target_key.as_str()],
                ["csm-key", semantic_key.as_str()],
            ];
            if event.tags.len() != expected.len()
                || event
                    .tags
                    .iter()
                    .zip(expected)
                    .any(|(got, want)| got.as_slice() != want)
            {
                continue;
            }
            provider_authorized(client, &args.agent_pubkey, &event.pubkey.to_hex()).await?;
            let key = (
                channel,
                target.driver.clone(),
                target.instance_id.clone(),
                target.session_id.clone(),
            );
            let replace = records
                .get(&key)
                .is_none_or(|(held, _): &(Event, Responsibility)| {
                    event.created_at > held.created_at
                        || (event.created_at == held.created_at && event.id < held.id)
                });
            if replace {
                let row = Responsibility {
                    channel_id: channel,
                    target,
                    provider_authority: event.pubkey.to_hex(),
                    metadata_event_id: event.id.to_hex(),
                    status: content.status,
                };
                records.insert(key, (event, row));
            }
        }
    }
    let mut rows: Vec<_> = records.into_values().map(|(_, record)| record).collect();
    merge_inventory(&mut rows, &uncertain);
    Ok(rows)
}

pub(crate) async fn provider_authorized(
    client: &impl RelayTransport,
    actor: &str,
    provider: &str,
) -> Result<(), Error> {
    let actor_rows = client
        .query_all(json!({"kinds":[0],"authors":[actor],"limit":1}))
        .await?;
    let actor_profile = actor_rows
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Event>(v).ok())
        .find(|event| {
            event.kind.as_u16() == 0 && event.pubkey.to_hex() == actor && event.verify().is_ok()
        })
        .ok_or_else(|| Error::Unconfirmed("actor owner profile unavailable".into()))?;
    let actor_auth = actor_profile
        .tags
        .iter()
        .find(|tag| tag.as_slice().first().is_some_and(|v| v == "auth"))
        .ok_or_else(|| Error::Auth("actor owner attestation unavailable".into()))?;
    let actor_auth_json = serde_json::to_string(actor_auth.as_slice()).map_err(local::fail)?;
    let owner = beekeeper_sdk::nip_oa::verify_auth_tag(&actor_auth_json, &actor_profile.pubkey)
        .map_err(|_| Error::Auth("actor owner attestation invalid".into()))?;
    let rows = client
        .query_all(json!({"kinds":[0],"authors":[provider],"limit":1}))
        .await?;
    let profile = rows
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Event>(v).ok())
        .find(|event| {
            event.kind.as_u16() == 0 && event.pubkey.to_hex() == provider && event.verify().is_ok()
        })
        .ok_or_else(|| Error::Unconfirmed("provider authority profile unavailable".into()))?;
    let auth = profile
        .tags
        .iter()
        .find(|tag| tag.as_slice().first().is_some_and(|v| v == "auth"))
        .ok_or_else(|| Error::Auth("provider has no owner attestation".into()))?;
    let json = serde_json::to_string(auth.as_slice()).map_err(local::fail)?;
    let attester = beekeeper_sdk::nip_oa::verify_auth_tag(&json, &profile.pubkey)
        .map_err(|_| Error::Auth("provider owner attestation invalid".into()))?;
    if attester != owner {
        return Err(Error::Auth(
            "provider does not belong to intended actor owner".into(),
        ));
    }
    Ok(())
}
fn merge_inventory(held: &mut Vec<Responsibility>, fresh: &[Responsibility]) {
    for row in fresh {
        if !held.iter().any(|old| {
            old.channel_id == row.channel_id
                && old.provider_authority == row.provider_authority
                && old.target == row.target
        }) {
            held.push(row.clone())
        }
    }
}
pub(crate) async fn terminal(
    row: &Responsibility,
    client: &impl RelayTransport,
) -> Result<bool, Error> {
    let rows = client
        .query_all(
            json!({"kinds":[44224],"authors":[row.provider_authority],"#h":[row.channel_id]}),
        )
        .await?;
    for raw in &rows {
        let Some(content) = raw.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Ok(candidate) =
            beekeeper_core::coding_session_payload::decode_coding_session_lifecycle_receipt(
                content,
            )
        else {
            continue;
        };
        if crate::receipts::verified_command_receipts(
            &rows,
            &row.channel_id.to_string(),
            &candidate.command_id,
            &row.provider_authority,
        )
        .iter()
        .any(|(_, r)| r.status == ReceiptStatus::Stopped && r.session.as_ref() == Some(&row.target))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
async fn runtime_lease(row: &Responsibility, client: &impl RelayTransport) -> Result<bool, Error> {
    use beekeeper_core::coding_session_lease::{
        validate_coding_session_lease_envelope, CodingSessionLeaseState,
    };
    let rows = client
        .query_all(
            json!({"kinds":[24223],"authors":[row.provider_authority],"#h":[row.channel_id]}),
        )
        .await?;
    let now = nostr::Timestamp::now().as_secs();
    let mut winning = None;
    for raw in rows {
        let Ok(event) = serde_json::from_value::<Event>(raw) else {
            continue;
        };
        if event.pubkey.to_hex() != row.provider_authority || event.verify().is_err() {
            continue;
        }
        let Ok(lease) = validate_coding_session_lease_envelope(&event, now) else {
            continue;
        };
        if lease.channel_id != row.channel_id || lease.payload.target != row.target {
            continue;
        }
        let order = (lease.payload.lease_sequence, event.created_at, event.id);
        if winning.as_ref().is_none_or(|(held, _)| order > *held) {
            winning = Some((order, lease.payload.state));
        }
    }
    Ok(winning.is_some_and(|(_, state)| state == CodingSessionLeaseState::Live))
}
/// Inspect signed current responsibilities without executor files/socket.
pub async fn status(args: &LifecycleRequest, client: &impl RelayTransport) -> Result<Value, Error> {
    let rows = inventory(args, client).await?;
    let mut observations = Vec::new();
    for row in rows {
        let stopped = terminal(&row, client).await?;
        let live = runtime_lease(&row, client).await?;
        observations.push(json!({"responsibility":row,"terminalStopped":stopped,"runtimeStatus":row.status,"runtime_available":live && !stopped,"configuration_application":"unconfirmed","availability":if live && !stopped {"verified_unexpired_provider_lease"} else {"no_current_live_lease"}}));
    }
    Ok(
        json!({"agent_pubkey":args.agent_pubkey,"responsibilities":observations,"inventory":"relay_observed","absence_is_not_revocation":true,"configuration_application":"unconfirmed"}),
    )
}
/// Stop every observed responsibility and verify exact signed terminal receipts.
pub async fn stop(
    args: &LifecycleRequest,
    client: &impl RelayTransport,
    context: &Context,
) -> Result<Value, Error> {
    let _lock = local::management_lock(context)?;
    stop_inner(args, client, context, &[]).await
}
async fn stop_inner(
    args: &LifecycleRequest,
    client: &impl RelayTransport,
    context: &Context,
    seed: &[Responsibility],
) -> Result<Value, Error> {
    if !(1..=300).contains(&args.wait_secs) {
        return Err(Error::Usage("wait_secs must be1..300".into()));
    }
    let request = json!({"type":"stop","actor":args.agent_pubkey,"owner":client.keys().public_key().to_hex(),"relay":client.relay_url(),"channels":args.channels});
    let path = local::operation_path(context, args.operation_id)?;
    let mut operation = match std::fs::read(&path) {
        Ok(bytes) => {
            let value: LifecycleOperation = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Other("lifecycle journal malformed".into()))?;
            if value.request != request {
                return Err(Error::Conflict("operation UUID request changed".into()));
            }
            value
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LifecycleOperation {
            request,
            events: vec![],
            inventory: vec![],
        },
        Err(_) => return Err(Error::Other("lifecycle journal unavailable".into())),
    };
    let observed = inventory(args, client).await?;
    merge_inventory(&mut operation.inventory, seed);
    merge_inventory(&mut operation.inventory, &observed);
    local::save(&path, &operation)?;
    let mut rows = operation.inventory.clone();
    let mut renew_stop_delivery = true;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(args.wait_secs);
    loop {
        let mut pending = Vec::new();
        for row in &rows {
            if terminal(row, client).await? {
                continue;
            }
            pending.push(row.clone());
            let id = crate::lifecycle_retry::command_id(
                args.operation_id,
                row,
                &operation.events,
                client,
                renew_stop_delivery,
            )
            .await?;
            if !operation
                .events
                .iter()
                .any(|event| event.content.contains(&id))
            {
                let payload = CodingSessionLifecycleCommandPayload {
                    schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
                    command_id: id.clone(),
                    action: CodingSessionLifecycleAction::SessionStop {
                        session: row.target.clone(),
                        provider_authority_pubkey: row.provider_authority.clone(),
                    },
                };
                let builder =
                    beekeeper_sdk::build_coding_session_lifecycle_command(row.channel_id, &payload)
                        .map_err(local::fail)?;
                operation.events.push(client.sign_event_unchecked(builder)?);
                local::save(&path, &operation)?;
            }
        }
        renew_stop_delivery = false;
        for event in &operation.events {
            management::submit(client, event).await?;
        }
        if pending.is_empty() {
            // Client management is already locked. Serialize final discovery
            // and the canonical stopped CAS with worker admission in that order.
            let _admission = beekeeper_host_core::ownership::admission_lock(
                &context.home,
                context.instance,
                &args.agent_pubkey,
                &crate::support::to_ws_url(client.relay_url()),
            )
            .map_err(local::fail)?;
            let fresh = inventory(args, client).await?;
            let before = operation.inventory.len();
            merge_inventory(&mut operation.inventory, &fresh);
            local::save(&path, &operation)?;
            if operation.inventory.len() == before {
                if let Some(held) = beekeeper_host_core::ownership::for_pair(
                    &context.home,
                    context.instance,
                    &args.agent_pubkey,
                    &crate::support::to_ws_url(client.relay_url()),
                )
                .map_err(local::fail)?
                {
                    if held.phase == beekeeper_host_core::ownership::Phase::Adopted {
                        if let Some(target) = &held.target {
                            beekeeper_host_core::ownership::host_stopped(
                                &context.home,
                                context.instance,
                                &args.agent_pubkey,
                                &held.relay_url,
                                target,
                            )
                            .map_err(local::fail)?;
                        }
                    }
                }
                return Ok(
                    json!({"operation_id":args.operation_id,"agent_pubkey":args.agent_pubkey,"terminal":true,"responsibilities":operation.inventory,"custody":"retained"}),
                );
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(
                json!({"operation_id":args.operation_id,"agent_pubkey":args.agent_pubkey,"terminal":false,"pending":pending,"custody":"retained"}),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let fresh = inventory(args, client).await?;
        merge_inventory(&mut operation.inventory, &fresh);
        local::save(&path, &operation)?;
        rows = operation.inventory.clone();
    }
}
/// Retire through existing9035/NIP09 semantics; preserve custody and disclose partial outcomes.
pub async fn delete(
    args: &DeleteRequest,
    client: &impl RelayTransport,
    context: &Context,
) -> Result<Value, Error> {
    let owner = local::owner(client)?;
    crate::support::validate_hex64(&args.expected_revision)?;
    let _lock = local::management_lock(context)?;
    let request = json!({"type":"delete","owner":owner,"actor":args.agent_pubkey,"relay":client.relay_url(),"expectedRevision":args.expected_revision,"channels":args.channels,"remote":args.remote_deployment});
    let path = local::operation_path(context, args.operation_id)?;
    let operation = match std::fs::read(&path) {
        Ok(bytes) => {
            let value: LifecycleOperation = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Other("retirement journal malformed".into()))?;
            if value.request != request {
                return Err(Error::Conflict("delete operation request changed".into()));
            }
            value
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let head = management::configuration::current_head(client, &args.agent_pubkey).await?;
            if head.id.to_hex() != args.expected_revision {
                return Err(Error::Conflict(
                    "delete revision changed before retirement".into(),
                ));
            }
            let key = nostr::PublicKey::from_hex(&args.agent_pubkey).map_err(local::fail)?;
            let auth = beekeeper_sdk::nip_oa::compute_auth_tag(client.keys(), &key, "")
                .map_err(local::fail)?;
            let values: Vec<String> = serde_json::from_str(&auth).map_err(local::fail)?;
            let auth: [String; 4] = values
                .try_into()
                .map_err(|_| Error::Auth("owner attestation malformed".into()))?;
            let archive = beekeeper_sdk::build_archive_identity_request(
                &args.agent_pubkey,
                "",
                Some("retired"),
                None,
                Some(&auth),
            )
            .map_err(local::fail)?;
            let tombstone = nostr::EventBuilder::delete(
                nostr::nips::nip09::EventDeletionRequest::new().id(head.id),
            )
            .tags([nostr::Tag::parse([
                "a",
                &format!("30177:{}:{}", owner, args.agent_pubkey),
            ])
            .map_err(local::fail)?]);
            let captured = inventory(
                &LifecycleRequest {
                    operation_id: args.operation_id,
                    agent_pubkey: args.agent_pubkey.clone(),
                    channels: args.channels.clone(),
                    wait_secs: args.wait_secs,
                },
                client,
            )
            .await?;
            let value = LifecycleOperation {
                request,
                events: vec![
                    client.sign_event_unchecked(archive)?,
                    client.sign_event_unchecked(tombstone)?,
                ],
                inventory: captured,
            };
            local::save(&path, &value)?;
            value
        }
        Err(_) => return Err(Error::Other("retirement journal unavailable".into())),
    };
    // A reserved operation does not authorize retiring a newer owner head.
    // On a completed replay, only our exact accepted tombstone proves absence.
    match management::configuration::current_head(client, &args.agent_pubkey).await {
        Ok(head) if head.id.to_hex() == args.expected_revision => {}
        Ok(_) => {
            return Err(Error::Conflict(
                "delete revision changed before retirement; custody retained".into(),
            ))
        }
        Err(Error::NotFound(_)) => {
            if !accepted_exact(client, &operation.events[1]).await? {
                return Err(Error::Unconfirmed(
                    "definition absence lacks exact accepted owner tombstone; custody retained"
                        .into(),
                ));
            }
        }
        Err(error) => return Err(error),
    }
    // Retirement is a durable admission boundary, not a claim of process exit.
    let retirement = &operation.events[0];
    if !retirement_confirmed(client, &args.agent_pubkey, retirement).await? {
        management::submit(client, retirement).await?;
        if !retirement_confirmed(client, &args.agent_pubkey, retirement).await? {
            return Ok(
                json!({"operation_id":args.operation_id,"agent_pubkey":args.agent_pubkey,"retirement_requested":true,"retirement_confirmed":false,"terminal":false,"pending":"exact relay retirement delta unavailable","custody":"retained"}),
            );
        }
    }
    if args.remote_deployment {
        return Ok(
            json!({"operation_id":args.operation_id,"agent_pubkey":args.agent_pubkey,"retirement_requested":true,"terminal":false,"pending":"remote provider info/deploy protocol has no stop acknowledgement","custody":"retained"}),
        );
    }
    let mut channels = args.channels.clone();
    for row in &operation.inventory {
        if !channels.contains(&row.channel_id) {
            channels.push(row.channel_id)
        }
    }
    let lifecycle = LifecycleRequest {
        operation_id: Uuid::new_v5(&args.operation_id, b"all-context-stop"),
        agent_pubkey: args.agent_pubkey.clone(),
        channels,
        wait_secs: args.wait_secs,
    };
    let stopped = stop_inner(&lifecycle, client, context, &operation.inventory).await?;
    if stopped.get("terminal").and_then(Value::as_bool) != Some(true) {
        return Ok(
            json!({"operation_id":args.operation_id,"retirement_requested":true,"terminal":false,"stop":stopped,"custody":"retained"}),
        );
    }
    // A newly changed definition cannot be removed by a stale delete request.
    match management::configuration::current_head(client, &args.agent_pubkey).await {
        Ok(head) if head.id.to_hex() == args.expected_revision => {}
        Ok(_) => {
            return Err(Error::Conflict(
                "definition changed while deletion quiesced; custody retained".into(),
            ))
        }
        Err(Error::NotFound(_)) => {
            let tombstone = &operation.events[1];
            let rows = client
                .query_all(json!({"kinds":[5],"authors":[owner],"ids":[tombstone.id.to_hex()]}))
                .await?;
            if !rows
                .into_iter()
                .filter_map(|v| serde_json::from_value::<Event>(v).ok())
                .any(|v| v == *tombstone && v.verify().is_ok())
            {
                return Err(Error::Unconfirmed(
                    "definition absence lacks exact accepted owner tombstone; custody retained"
                        .into(),
                ));
            }
        }
        Err(error) => return Err(error),
    }
    management::submit(client, &operation.events[1]).await?;
    Ok(
        json!({"operation_id":args.operation_id,"agent_pubkey":args.agent_pubkey,"retirement_requested":true,"terminal":true,"tombstone_event_id":operation.events[1].id.to_hex(),"custody":"retained_for_explicit_recovery","stop":stopped}),
    )
}
async fn retirement_confirmed(
    client: &impl RelayTransport,
    actor: &str,
    request: &Event,
) -> Result<bool, Error> {
    let relay = client.relay_identity().await?;
    let events = client
        .query_all(
            json!({"kinds":[8002],"authors":[relay],"#p":[actor],"#e":[request.id.to_hex()]}),
        )
        .await?;
    Ok(events
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Event>(v).ok())
        .any(|event| {
            event.kind.as_u16() == 8002
                && event.pubkey.to_hex() == relay
                && event.verify().is_ok()
                && event.tags.iter().any(|tag| tag.as_slice() == ["p", actor])
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["e", &request.id.to_hex()])
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["reason", "retired"])
                && event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["consent", "owner", &request.pubkey.to_hex()])
        }))
}
async fn accepted_exact(client: &impl RelayTransport, event: &Event) -> Result<bool, Error> {
    let rows = client.query_all(json!({"kinds":[event.kind.as_u16()], "authors":[event.pubkey.to_hex()], "ids":[event.id.to_hex()]})).await?;
    Ok(rows
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Event>(v).ok())
        .any(|held| held == *event && held.verify().is_ok()))
}
/// Start a new context for an existing host-custodied identity through canonical create.
pub async fn start(
    args: &crate::CreateRequest,
    client: &impl RelayTransport,
    context: &Context,
) -> Result<Value, Error> {
    if !args.activate {
        return Err(Error::Usage("start requires activate=true".into()));
    }
    if let Some(actor) = args.agent_pubkey.as_deref() {
        let _lock = local::management_lock(context)?;
        let relay = crate::support::to_ws_url(client.relay_url());
        if let Some(held) =
            beekeeper_host_core::ownership::for_pair(&context.home, context.instance, actor, &relay)
                .map_err(local::fail)?
        {
            if held.phase == beekeeper_host_core::ownership::Phase::Adopted {
                let head = management::configuration::current_head(client, actor).await?;
                let content: beekeeper_core::managed_agent::ManagedAgentEventContent =
                    serde_json::from_str(&head.content).map_err(local::fail)?;
                if content.name != args.display_name
                    || content.system_prompt.as_deref().unwrap_or("") != args.system_prompt
                    || args
                        .model
                        .as_ref()
                        .is_some_and(|model| content.model.as_ref() != Some(model))
                    || args
                        .public_definition
                        .as_ref()
                        .is_some_and(|requested| requested != &content)
                {
                    return Err(Error::Conflict(
                        "start cannot silently change the current definition; configure first"
                            .into(),
                    ));
                }
                let observation = status(
                    &LifecycleRequest {
                        agent_pubkey: actor.into(),
                        channels: vec![args.channel],
                        ..Default::default()
                    },
                    client,
                )
                .await?;
                let held_target = serde_json::to_value(&held.target).map_err(local::fail)?;
                let runtime_available = observation
                    .get("responsibilities")
                    .and_then(Value::as_array)
                    .is_some_and(|rows| {
                        rows.iter().any(|row| {
                            row["responsibility"]["target"] == held_target
                                && row["runtime_available"] == true
                        })
                    });
                return Ok(
                    json!({"agent_pubkey":actor,"target":held.target,"revision":head.id.to_hex(),"operation_id":held.operation_id,"already_adopted":true,"execution_created":false,"runtime_available":runtime_available,"configuration_application":"unconfirmed","pending":!runtime_available,"observation":observation}),
                );
            }
        }
    }
    let mut result = management::create(args, client, context).await?;
    result
        .as_object_mut()
        .ok_or_else(|| Error::Other("start result is not an object".into()))?
        .insert("configuration_application".into(), json!("unconfirmed"));
    Ok(result)
}
/// Restart awaits the actor-wide terminal boundary before canonical create.
pub async fn restart(
    stop_request: &LifecycleRequest,
    create_request: &crate::CreateRequest,
    client: &impl RelayTransport,
    context: &Context,
) -> Result<Value, Error> {
    let stopped = stop(stop_request, client, context).await?;
    if stopped.get("terminal").and_then(Value::as_bool) != Some(true) {
        return Ok(json!({"terminal":false,"stop":stopped,"start":"not_admitted"}));
    }
    start(create_request, client, context).await
}

/// Prove configured-only enrollment has no prior nonterminal responsibility.
pub(crate) async fn require_configured_stops(
    actor: &str,
    channel: Uuid,
    accepted: Option<&CodingSessionTarget>,
    client: &impl RelayTransport,
) -> Result<(), Error> {
    let request = LifecycleRequest {
        agent_pubkey: actor.into(),
        channels: vec![channel],
        ..Default::default()
    };
    for row in inventory(&request, client).await? {
        if accepted == Some(&row.target) {
            continue;
        }
        if !terminal(&row, client).await? {
            return Err(Error::Refused(
                "configured enrollment has a responsibility without a verified stopped receipt"
                    .into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
