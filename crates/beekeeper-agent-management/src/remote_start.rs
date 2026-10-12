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
        let rows = crate::lifecycle::inventory(&scope, client).await?;
        let mut active = Vec::new();
        for row in rows {
            if !crate::lifecycle::terminal(&row, client).await? {
                active.push(row)
            }
        }
        if !active.is_empty() {
            let observation = crate::lifecycle::status(&scope, client).await?;
            let runtime_available =
                observation["responsibilities"]
                    .as_array()
                    .is_some_and(|observed| {
                        observed.iter().any(|entry| {
                            active.iter().any(|row| {
                                row.provider_authority == args.provider_authority
                                    && row.channel_id == args.channel
                                    && entry["responsibility"]["target"]
                                        == serde_json::json!(row.target)
                                    && entry["responsibility"]["channelId"]
                                        == row.channel_id.to_string()
                                    && entry["responsibility"]["providerAuthority"]
                                        == row.provider_authority
                                    && entry["runtime_available"] == true
                            })
                        })
                    });
            return Ok(
                json!({"agent_pubkey":args.agent_pubkey,"execution_created":false,"already_adopted":true,"runtime_available":runtime_available,"pending":!runtime_available,"availability":if runtime_available {"verified_unexpired_provider_lease"} else {"no_current_live_lease"},"configuration_application":"unconfirmed","observation":observation,"responsibilities":active}),
            );
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
        let held = Operation {
            request,
            event,
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
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use std::sync::Mutex;
    struct Relay {
        owner: Keys,
        provider: Keys,
        actor: Keys,
        definition: Event,
        events: Mutex<Vec<Event>>,
    }
    fn profile(owner: &Keys, key: &Keys) -> Event {
        let auth =
            beekeeper_sdk::nip_oa::compute_auth_tag(owner, &key.public_key(), "").expect("auth");
        EventBuilder::new(Kind::Metadata, "{}")
            .tags([
                Tag::parse(serde_json::from_str::<Vec<String>>(&auth).expect("auth values"))
                    .expect("tag"),
            ])
            .sign_with_keys(key)
            .expect("profile")
    }
    impl RelayTransport for Relay {
        fn keys(&self) -> &Keys {
            &self.owner
        }
        fn relay_url(&self) -> &str {
            "ws://127.0.0.1:31337"
        }
        fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
            Ok(None)
        }
        fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
            Err(Error::Other("no actor transport required".into()))
        }
        async fn relay_identity(&self) -> Result<String, Error> {
            Ok(self.provider.public_key().to_hex())
        }
        async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
            let rows = match filter["kinds"][0].as_u64().expect("explicit kind") {
                30177 => vec![self.definition.clone()],
                0 => vec![profile(
                    &self.owner,
                    if filter["authors"][0] == self.actor.public_key().to_hex() {
                        &self.actor
                    } else {
                        &self.provider
                    },
                )],
                44221 | 44224 => self
                    .events
                    .lock()
                    .expect("events")
                    .iter()
                    .filter(|event| {
                        event.kind.as_u16() as u64 == filter["kinds"][0].as_u64().expect("kind")
                    })
                    .cloned()
                    .collect(),
                _ => vec![],
            };
            Ok(rows
                .into_iter()
                .map(|event| serde_json::to_value(event).expect("event"))
                .collect())
        }
        async fn submit_event(&self, event: Event) -> Result<Value, Error> {
            assert_eq!(event.kind.as_u16(), 44221);
            assert_eq!(event.pubkey, self.owner.public_key());
            let payload=beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(&event.content).expect("payload");
            let target = beekeeper_core::coding_session_command::CodingSessionTarget {
                driver: "codex-acp".into(),
                instance_id: "fixture".into(),
                session_id: Uuid::new_v4().to_string(),
                generation: 1,
            };
            let key = beekeeper_sdk::coding_session::coding_session_lifecycle_receipt_semantic_key(
                &payload.command_id,
            );
            let receipt = EventBuilder::new(
                Kind::Custom(44224),
                serde_json::to_string(
                    &beekeeper_core::coding_session_payload::LifecycleReceipt::created(
                        &payload.command_id,
                        &target,
                    ),
                )
                .expect("receipt"),
            )
            .tags([
                Tag::parse([
                    "h",
                    event
                        .tags
                        .iter()
                        .find(|tag| tag.as_slice()[0] == "h")
                        .expect("h")
                        .as_slice()[1]
                        .as_str(),
                ])
                .expect("h"),
                Tag::parse([
                    "cslr-v",
                    beekeeper_sdk::coding_session::CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
                ])
                .expect("version"),
                Tag::parse(["csl-command", &payload.command_id]).expect("command"),
                Tag::parse(["csl-key", &key]).expect("key"),
            ])
            .sign_with_keys(&self.provider)
            .expect("signed receipt");
            self.events.lock().expect("events").extend([event, receipt]);
            Ok(json!({"accepted":true,"message":"stored"}))
        }
    }
    #[tokio::test]
    async fn owner_remote_start_uses_only_caller_journal_and_exact_signed_retry() {
        let owner = Keys::generate();
        let actor = Keys::generate();
        let provider = Keys::generate();
        let content = beekeeper_core::managed_agent::ManagedAgentEventContent {
            name: "Remote".into(),
            system_prompt: Some("ordinary work".into()),
            model: Some("default".into()),
            persona_id: None,
            provider: None,
            persona_source_version: None,
            parallelism: 1,
            respond_to: beekeeper_core::managed_agent::RespondTo::OwnerOnly,
            respond_to_allowlist: vec![],
            home_role: None,
            project_digest: None,
            project_withdrawn: false,
        };
        let definition = beekeeper_sdk::managed_agent::build_managed_agent_event(
            &actor.public_key().to_hex(),
            &content,
        )
        .expect("definition")
        .sign_with_keys(&owner)
        .expect("signed head");
        let args = RemoteStartRequest {
            operation_id: Uuid::new_v4(),
            agent_pubkey: actor.public_key().to_hex(),
            expected_revision: definition.id.to_hex(),
            channel: Uuid::new_v4(),
            provider_authority: provider.public_key().to_hex(),
            provider_instance: "codex-primary".into(),
            project: None,
            wait_secs: 1,
        };
        let client = Relay {
            owner,
            provider,
            actor,
            definition,
            events: Mutex::new(vec![]),
        };
        let home = tempfile::tempdir().expect("caller home");
        let context = Context {
            home: home.path().into(),
            instance: beekeeper_host_core::layout::Instance::Dev,
            native_home: None,
        };
        let first = start_remote(&args, &client, &context)
            .await
            .expect("remote accepted");
        assert_eq!(first["host_admitted"], true);
        assert!(!home.path().join("host/config.json").exists());
        assert!(!home.path().join("actors").exists());
        assert_eq!(
            start_remote(&args, &client, &context).await.expect("retry"),
            first
        );
        assert_eq!(
            client
                .events
                .lock()
                .expect("events")
                .iter()
                .filter(|event| event.kind.as_u16() == 44221)
                .count(),
            1
        );
        let mut changed = args.clone();
        changed.provider_instance = "different".into();
        assert!(matches!(
            start_remote(&changed, &client, &context).await,
            Err(Error::Conflict(_))
        ));
    }
}
