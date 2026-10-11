//! Headless enrollment and public configuration writes using existing contracts.
//!
//! A restricted operation journal precedes publication. It retains exact signed
//! events and accepted execution targets, so a lost reply never changes identity
//! or creates another logical responsibility on retry. Runtime execution remains
//! the session provider's; this module does not supervise processes.

use std::fs::File;
use std::path::{Path, PathBuf};

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_identity::ProviderInstanceAlias;
use beekeeper_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::managed_agent::{
    validate_agent_definition_text, ManagedAgentEventContent, RespondTo,
};
use beekeeper_host_core::{
    atomic_write, config::HostConfig, env::PROJECTS_FILE_NAME, layout, provision, record,
};
use clap::Args;
use nostr::{Event, Keys, Tag, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::sessions::crew_cmds::{await_create_receipt, CreateWaitOutcome};

#[path = "agent_management/local.rs"]
pub(super) mod local;
use crate::{client::BeekeeperClient, error::CliError, validate::read_or_stdin};
use local::*;

/// Commission one attributed ordinary participant on an existing headless host.
#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Stable operation UUID; reuse unchanged when retrying a lost reply.
    #[arg(long)]
    pub operation_id: Uuid,
    /// Conversation channel whose owner authorizes enrollment.
    #[arg(long)]
    pub channel: Uuid,
    /// Public participant name.
    #[arg(long)]
    pub display_name: String,
    /// Executable instructions, or '-' to read stdin.
    #[arg(long)]
    pub system_prompt: String,
    /// Explicit commissioned service home.
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Existing checkout for this responsibility; never transmitted publicly.
    #[arg(long)]
    pub cwd: PathBuf,
    /// Authorized project coordinate; private coordinates remain executor-local.
    #[arg(long)]
    pub project: Option<String>,
    /// Advertised model choice; defaults to the commissioned runtime's choice.
    #[arg(long)]
    pub model: Option<String>,
    /// Commissioned runtime instance alias; required if more than one exists.
    #[arg(long)]
    pub provider_instance: Option<String>,
    /// Add another accepted responsibility for an existing host-custodied identity.
    #[arg(long, conflicts_with = "adopt_agent_pubkey")]
    pub agent_pubkey: Option<String>,
    /// Adopt an existing identity after its legacy generation is quiesced.
    #[arg(long, requires_all = ["key_file", "auth_tag_file"])]
    pub adopt_agent_pubkey: Option<String>,
    /// Owner-only staged existing scoped key; never an owner's signing key.
    #[arg(long, requires = "adopt_agent_pubkey")]
    pub key_file: Option<PathBuf>,
    /// Owner-only staged existing NIP-OA attestation.
    #[arg(long, requires = "adopt_agent_pubkey")]
    pub auth_tag_file: Option<PathBuf>,
    /// Maximum seconds to wait for the provider's signed create receipt.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=300))]
    pub wait_secs: u64,
}

/// Publish a new public configuration revision as its authorized owner.
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Stable operation UUID; reuse unchanged when retrying.
    #[arg(long)]
    pub operation_id: Uuid,
    /// Managed identity's lowercase hexadecimal public key.
    #[arg(long)]
    pub agent_pubkey: String,
    /// Exact kind30177 head event ID observed before editing.
    #[arg(long)]
    pub expected_revision: String,
    /// Optional new public name.
    #[arg(long)]
    pub display_name: Option<String>,
    /// Optional new instructions, or '-' to read stdin.
    #[arg(long)]
    pub system_prompt: Option<String>,
    /// Optional new model choice.
    #[arg(long)]
    pub model: Option<String>,
    /// Requester's journal home; no host files or socket are required for update.
    #[arg(long)]
    pub home: Option<PathBuf>,
}

async fn submit(client: &BeekeeperClient, event: &Event) -> Result<(), CliError> {
    if event.pubkey != client.keys().public_key() {
        return Err(CliError::Auth(
            "event signer does not match scoped transport identity".into(),
        ));
    }
    beekeeper_core::verify_event(event)
        .map_err(|_| CliError::Auth("journal event signature or identity is invalid".into()))?;
    // Persisted requests may predate the relay's publication clock window.
    // Reuse only their exact verified stored event, through the same authorized
    // signer transport. Never refresh a journal event's timestamp or signature.
    if exact_event_stored(&client.query_all(exact_event_filter(event)).await?, event) {
        return Ok(());
    }
    let raw = client.submit_event(event.clone()).await?;
    match super::parse_write_response(&raw, "operation event already accepted") {
        Ok(_) => Ok(()),
        Err(CliError::Conflict(_)) => {
            let rows = client.query_all(exact_event_filter(event)).await?;
            if exact_event_stored(&rows, event) {
                Ok(())
            } else {
                Err(CliError::Conflict(format!("relay reported a duplicate or dominated publication, but exact signed event {} is not stored; original operation preserved", event.id)))
            }
        }
        Err(error) => Err(error),
    }
}

fn exact_event_filter(event: &Event) -> Value {
    let mut filter = json!({"kinds":[event.kind.as_u16()],"ids":[event.id.to_hex()],"authors":[event.pubkey.to_hex()]});
    if let Some(channel) = event.tags.iter().find_map(|tag| {
        let fields = tag.as_slice();
        (fields.first().is_some_and(|value| value == "h"))
            .then(|| fields.get(1))
            .flatten()
    }) {
        filter["#h"] = json!([channel]);
    }
    filter
}

fn exact_event_stored(rows: &[Value], expected: &Event) -> bool {
    rows.iter()
        .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
        .any(|held| {
            held.id == expected.id
                && held.sig == expected.sig
                && beekeeper_core::verify_event(&held).is_ok()
        })
}

/// Create identity and attributed Solo execution, admitting its route only after a verified receipt.
pub async fn create(args: &CreateArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    let owner = owner(client)?;
    let home = home(&args.home)?;
    let _lock = management_lock(&home)?;
    let config = HostConfig::load(&layout::host_config_path(
        &home,
        layout::Instance::from_env().map_err(CliError::Usage)?,
    ))
    .map_err(fail)?
    .ok_or_else(|| {
        CliError::Usage("host has not been commissioned; run bee host commission first".into())
    })?;
    if record::canonical_relay_key(&config.relay_url)
        != record::canonical_relay_key(&crate::client::to_ws_url(client.relay_url()))
    {
        return Err(CliError::Auth(
            "active relay does not match commissioned host".into(),
        ));
    }
    let store = record::load_provider_store_from(&config.record_store_path()).map_err(fail)?;
    let provider = store
        .get(&config.relay_url)
        .ok_or_else(|| fail("commissioned provider record is missing"))?;
    if provider.provider_pubkey != config.provider_pubkey {
        return Err(CliError::Refused(
            "commissioned provider record/config identities disagree".into(),
        ));
    }
    let provider_key = nostr::PublicKey::from_hex(&provider.provider_pubkey).map_err(fail)?;
    let provider_owner = beekeeper_sdk::nip_oa::verify_auth_tag(
        provider
            .auth_tag
            .as_deref()
            .ok_or_else(|| fail("provider owner attestation is missing"))?,
        &provider_key,
    )
    .map_err(fail)?;
    if provider_owner.to_hex() != owner {
        return Err(CliError::Auth(
            "signer does not own this commissioned provider".into(),
        ));
    }
    let runtime = match args.provider_instance.as_deref() {
        Some(alias) => config
            .runtimes
            .iter()
            .find(|runtime| runtime.instance_ref == alias),
        None if config.runtimes.len() == 1 => config.runtimes.first(),
        None => {
            return Err(CliError::Usage(
                "--provider-instance is required when the host offers multiple runtimes".into(),
            ))
        }
    }
    .ok_or_else(|| CliError::Usage("provider runtime instance is not commissioned".into()))?;
    let prompt = read_or_stdin(&args.system_prompt)?;
    validate_agent_definition_text(&args.display_name, &prompt).map_err(CliError::Usage)?;
    let cwd = std::fs::canonicalize(&args.cwd)
        .map_err(|_| CliError::Usage("--cwd must name an existing checkout".into()))?;
    validate_checkout(&cwd, &home, &config.provider_state_dir)?;
    if !cwd.is_dir() {
        return Err(CliError::Usage("--cwd must be a directory".into()));
    }
    let model = args
        .model
        .clone()
        .unwrap_or_else(|| runtime.default_model.clone());
    let project = args
        .project
        .as_deref()
        .map(|value| {
            beekeeper_core::kind::normalize_project_coordinate(value)
                .ok_or_else(|| CliError::Usage("--project must be 30621:<creator>:<slug>".into()))
        })
        .transpose()?;
    let project_public = if let Some(project) = project.as_deref() {
        Some(authorize_project(client, project, &owner).await?)
    } else {
        None
    };
    let transfer = if let Some(actor) = &args.adopt_agent_pubkey {
        let transfer = beekeeper_host_core::ownership::for_pair(
            &home,
            config.instance,
            actor,
            &config.relay_url,
        )
        .map_err(fail)?
        .ok_or_else(|| {
            CliError::Refused("identity adoption requires a staged ownership transfer".into())
        })?;
        if transfer.operation_id != args.operation_id.to_string()
            || !matches!(
                transfer.phase,
                beekeeper_host_core::ownership::Phase::Quiesced
                    | beekeeper_host_core::ownership::Phase::Adopted
            )
        {
            return Err(CliError::Refused("adoption requires the matching operation and a verified quiesced legacy generation".into()));
        }
        Some(transfer)
    } else {
        None
    };
    let request = json!({"type":"create","owner":owner,"relay":config.relay_url,"channel":args.channel,
        "name":args.display_name,"prompt":prompt,"cwd":cwd,"model":model,"runtime":runtime.instance_ref,
        "existingActor":args.agent_pubkey,"adoptActor":args.adopt_agent_pubkey,"adoptRevision":transfer.as_ref().map(|value| &value.definition_event_id),
        "project":project});
    let successor_boundary = successor::prepare(
        &home,
        &config,
        args.agent_pubkey.as_deref(),
        args.operation_id,
    )?;
    if let Some(boundary) = &successor_boundary {
        successor::require_prior_stops(
            &config,
            provider,
            boundary,
            args.operation_id,
            None,
            client,
        )
        .await?;
    }
    let path = operation_path(&home, args.operation_id)?;
    let mut minted = None;
    let mut operation = match read_operation(&path, &request)? {
        Some(operation) => operation,
        None => {
            let actor = match transfer.as_ref() {
                Some(transfer) => existing_identity(args, &owner, &transfer.actor_pubkey)?,
                None => match args.agent_pubkey.as_deref() {
                    Some(actor) => {
                        let (key, auth_tag) =
                            actor_custody(&config.provider_state_dir, actor, None)?;
                        let keys = Keys::parse(&key)
                            .map_err(|_| fail("existing actor custody key is invalid"))?;
                        let attester =
                            beekeeper_sdk::nip_oa::verify_auth_tag(&auth_tag, &keys.public_key())
                                .map_err(fail)?;
                        if attester.to_hex() != owner {
                            return Err(CliError::Auth(
                                "existing actor is attested by another owner".into(),
                            ));
                        }
                        provision::MintedAgentIdentity {
                            keys,
                            pubkey: actor.into(),
                            private_key_nsec: key,
                            auth_tag,
                        }
                    }
                    None => provision::mint_agent_identity(client.keys()).map_err(fail)?,
                },
            };
            let content = ManagedAgentEventContent {
                name: args.display_name.clone(),
                persona_id: None,
                system_prompt: Some(prompt.clone()),
                model: Some(model.clone()),
                provider: None,
                persona_source_version: None,
                parallelism: 1,
                respond_to: RespondTo::OwnerOnly,
                respond_to_allowlist: vec![],
                home_role: None,
                project_digest: if project_public == Some(true) {
                    project
                        .as_deref()
                        .and_then(beekeeper_core::project_agent_association::project_agent_digest)
                } else {
                    None
                },
                project_withdrawn: project_public == Some(false),
            };
            let (public, profile) = if transfer.is_some() || args.agent_pubkey.is_some() {
                let public = current_head(client, &actor.pubkey).await?;
                if transfer
                    .as_ref()
                    .is_some_and(|value| public.id.to_hex() != value.definition_event_id)
                {
                    return Err(CliError::Conflict(format!(
                        "adoption definition changed; effective revision is {}",
                        public.id
                    )));
                }
                let existing: ManagedAgentEventContent =
                    serde_json::from_str(&public.content).map_err(fail)?;
                if existing.name != args.display_name
                    || (existing.persona_id.is_none()
                        && (existing.system_prompt.as_deref().unwrap_or_default() != prompt
                            || existing
                                .model
                                .as_ref()
                                .is_some_and(|selected| selected != &model)))
                {
                    return Err(CliError::Conflict(
                        "adoption must preserve the existing name, instructions and model".into(),
                    ));
                }
                let profiles = client
                    .query_all(json!({"kinds":[0],"authors":[actor.pubkey],"limit":1}))
                    .await?;
                let profile = profiles.into_iter().filter_map(|value| serde_json::from_value::<Event>(value).ok())
                    .find(|event| event.pubkey == actor.keys.public_key() && beekeeper_core::verify_event(event).is_ok())
                    .ok_or_else(|| CliError::Refused("existing signed agent profile is unavailable; refusing to replace it during adoption".into()))?;
                (public, profile)
            } else {
                let public = client.sign_event_unchecked(
                    beekeeper_sdk::managed_agent::build_managed_agent_event(
                        &actor.pubkey,
                        &content,
                    )
                    .map_err(fail)?,
                )?;
                let auth_values: Vec<String> =
                    serde_json::from_str(&actor.auth_tag).map_err(fail)?;
                let profile = beekeeper_sdk::build_profile(
                    Some(&args.display_name),
                    Some(&args.display_name),
                    None,
                    None,
                    None,
                )
                .map_err(fail)?
                .tags([Tag::parse(auth_values).map_err(fail)?])
                .sign_with_keys(&actor.keys)
                .map_err(fail)?;
                (public, profile)
            };
            let payload = CodingSessionLifecycleCommandPayload {
                schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
                command_id: args.operation_id.to_string(),
                action: CodingSessionLifecycleAction::SessionCreate {
                    project_ref: project.clone(),
                    repo_ref: None,
                    session_ref: None,
                    genesis_ref: None,
                    provider_instance_ref: ProviderInstanceAlias::from_wire(&runtime.instance_ref)
                        .map_err(fail)?,
                    provider_authority_pubkey: config.provider_pubkey.clone(),
                    model: Some(model),
                    title: Some(args.display_name.clone()),
                    initial_turn: None,
                    actor: Some(actor.pubkey.clone()),
                    role: None,
                    hire_ref: None,
                    routing: None,
                },
            };
            let create = client.sign_event_unchecked(
                beekeeper_sdk::build_coding_session_lifecycle_command(args.channel, &payload)
                    .map_err(fail)?,
            )?;
            let member = client.sign_event_unchecked(actor_membership(
                args.channel,
                &actor.pubkey,
                args.agent_pubkey.is_some() || args.adopt_agent_pubkey.is_some(),
            )?)?;
            let provider_member = client.sign_event_unchecked(
                beekeeper_sdk::build_add_member(args.channel, &config.provider_pubkey, None)
                    .map_err(fail)?,
            )?;
            let mut events = vec![public, profile, member, provider_member];
            if let Some(project) = project.as_deref() {
                let roster = super::projects::project_roster(client, project).await?;
                for participant in [&actor.pubkey, &config.provider_pubkey] {
                    if !roster.iter().any(|(key, role)| {
                        key == participant && matches!(role.as_str(), "owner" | "collaborator")
                    }) {
                        events.push(
                            client.sign_event_unchecked(
                                beekeeper_sdk::managed_agent::build_project_member_admission(
                                    project,
                                    participant,
                                    "collaborator",
                                )
                                .map_err(fail)?,
                            )?,
                        );
                    }
                }
            }
            events.push(create);
            let operation = Operation {
                request: request.clone(),
                actor_pubkey: actor.pubkey.clone(),
                events,
                target: None,
                receipt_event_id: None,
                predecessor: successor_boundary.clone(),
            };
            // Reserve the identity before custody/publication. A crash in the
            // following custody handoff is explicitly blocked on retry, never
            // repaired by silently minting a different participant.
            save(&path, &operation)?;
            minted = Some(actor);
            operation
        }
    };
    if let Some(boundary) = &successor_boundary {
        if operation
            .predecessor
            .as_ref()
            .is_some_and(|held| held != boundary)
        {
            return Err(CliError::Conflict("successor operation was prepared against a different stopped owner; original operation preserved".into()));
        }
        if operation.predecessor.is_none() {
            operation.predecessor = Some(boundary.clone());
            save(&path, &operation)?;
        }
    }
    let initial = minted
        .as_ref()
        .map(|actor| (actor.private_key_nsec.as_str(), actor.auth_tag.as_str()));
    let (key, auth) = actor_custody(&config.provider_state_dir, &operation.actor_pubkey, initial)?;
    let actor_keys = Keys::parse(&key).map_err(|_| fail("actor custody key is invalid"))?;
    let auth_parts: Vec<String> =
        serde_json::from_str(&auth).map_err(|_| fail("actor attestation is malformed"))?;
    let actor_client = BeekeeperClient::new(
        client.relay_url().into(),
        actor_keys,
        Some(Tag::parse(auth_parts).map_err(|_| fail("actor attestation is malformed"))?),
        Some(auth.clone()),
    )?;
    if operation.target.is_none() {
        stage_actor(
            &config,
            &args.operation_id.to_string(),
            &operation.actor_pubkey,
            &args.display_name,
            (&key, &auth),
            &cwd,
            project.as_deref(),
        )?;
        for event in &operation.events {
            let transport = event_transport(event, client, &actor_client)?;
            submit(transport, event).await?;
        }
        match await_create_receipt(client, &args.channel.to_string(), &args.operation_id.to_string(), &config.provider_pubkey, args.wait_secs).await {
            CreateWaitOutcome::Confirmed {target,receipt_event_id,..} => {
                operation.target=Some(target); operation.receipt_event_id=Some(receipt_event_id); save(&path,&operation)?;
            }
            CreateWaitOutcome::Refused {error,..} => return Err(CliError::Refused(format!("provider refused operation {}: {}",args.operation_id,serde_json::to_string(&error).map_err(fail)?))),
            CreateWaitOutcome::Conflicted {detail} => return Err(CliError::Conflict(detail)),
            CreateWaitOutcome::Unconfirmed {query_failed} => return Err(CliError::Unconfirmed(format!("operation {} remains unconfirmed (receipt_query_failed={query_failed}); retry the same operation and unchanged arguments",args.operation_id))),
        }
    }
    let target = operation
        .target
        .as_ref()
        .ok_or_else(|| fail("confirmed operation has no target"))?;
    install_route(
        &config.provider_state_dir,
        &operation.actor_pubkey,
        args.channel,
        target,
    )?;
    if let Some(mut transfer) = transfer {
        transfer.phase = beekeeper_host_core::ownership::Phase::Adopted;
        transfer.target = Some(target.clone());
        beekeeper_host_core::ownership::advance(&home, config.instance, transfer).map_err(fail)?;
    }
    if args.agent_pubkey.is_some() {
        successor::reconcile(
            &home,
            &config,
            provider,
            &operation,
            args.operation_id,
            operation.predecessor.as_ref(),
            client,
        )
        .await?;
    }
    println!(
        "{}",
        json!({"operation_id":args.operation_id,"agent_pubkey":operation.actor_pubkey,
        "revision":operation.events.first().ok_or_else(|| fail("operation has no configuration event"))?.id.to_hex(),"host_admitted":true,"receipt_event_id":operation.receipt_event_id,
        "target":target,"conversation_route":"installed","runtime_account":"unconfirmed"})
    );
    Ok(())
}

fn actor_membership(
    channel: Uuid,
    actor: &str,
    existing_identity: bool,
) -> Result<nostr::EventBuilder, CliError> {
    // Native directory discovery uses the channel's automated designation.
    // This is independent of SessionCreate's optional team role. A bare add
    // preserves an existing identity's channel role, including elevated roles.
    let role = (!existing_identity).then_some(beekeeper_sdk::MemberRole::Bot);
    beekeeper_sdk::build_add_member(channel, actor, role).map_err(fail)
}

fn event_transport<'a>(
    event: &Event,
    owner: &'a BeekeeperClient,
    actor: &'a BeekeeperClient,
) -> Result<&'a BeekeeperClient, CliError> {
    if event.pubkey == owner.keys().public_key() {
        Ok(owner)
    } else if event.pubkey == actor.keys().public_key() && event.kind == nostr::Kind::Metadata {
        Ok(actor)
    } else {
        Err(CliError::Auth(
            "journal event has no authorized scoped transport".into(),
        ))
    }
}

#[path = "agent_management/successor.rs"]
mod successor;

#[path = "agent_management/configuration.rs"]
mod configuration;
pub(super) use configuration::authorize_project;
use configuration::current_head;
pub use configuration::update;

#[cfg(test)]
#[path = "agent_management/tests.rs"]
mod tests;
