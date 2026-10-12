//! Argument and output adapters for the shared headless management service.
use crate::{client::BeekeeperClient, error::CliError, validate::read_or_stdin, RespondToArg};
use clap::Args;
use std::path::PathBuf;
use uuid::Uuid;
#[path = "agent_management_import_evidence.rs"]
mod import_evidence;
pub use import_evidence::{supply_migration_definition, SupplyMigrationDefinitionArgs};
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
    /// Configure custody and identity without admitting execution.
    #[arg(long)]
    pub no_start: bool,
    /// Verified replay boundary; earlier/same-second uncertain legacy requests are disclosed.
    #[arg(long)]
    pub replay_floor_unix: Option<u64>,
    /// Complete safe public definition JSON; supported optional fields may be cleared.
    #[arg(long)]
    pub definition_file: Option<PathBuf>,
    /// Owner-only executor-private settings JSON; never sent publicly.
    #[arg(long)]
    pub private_config_file: Option<PathBuf>,
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
    /// Complete safe public definition JSON; supported optional fields may be cleared.
    #[arg(long)]
    pub definition_file: Option<PathBuf>,
    /// Owner-only local executor-private configuration JSON.
    #[arg(long)]
    pub private_config_file: Option<PathBuf>,
}

fn context(home: &Option<PathBuf>) -> Result<beekeeper_agent_management::Context, CliError> {
    Ok(beekeeper_agent_management::Context {
        home: home
            .clone()
            .map(Ok)
            .unwrap_or_else(beekeeper_host_core::layout::home_dir)
            .map_err(CliError::Other)?,
        instance: beekeeper_host_core::layout::Instance::from_env().map_err(CliError::Usage)?,
        native_home: std::env::var_os("HOME").map(PathBuf::from),
    })
}
fn create_request(
    args: &CreateArgs,
) -> Result<beekeeper_agent_management::CreateRequest, CliError> {
    Ok(beekeeper_agent_management::CreateRequest {
        operation_id: args.operation_id,
        channel: args.channel,
        display_name: args.display_name.clone(),
        system_prompt: read_or_stdin(&args.system_prompt)?,
        home: None,
        cwd: args.cwd.clone(),
        project: args.project.clone(),
        model: args.model.clone(),
        provider_instance: args.provider_instance.clone(),
        agent_pubkey: args.agent_pubkey.clone(),
        adopt_agent_pubkey: args.adopt_agent_pubkey.clone(),
        key_file: args.key_file.clone(),
        auth_tag_file: args.auth_tag_file.clone(),
        wait_secs: args.wait_secs,
        activate: !args.no_start,
        replay_floor_unix: args.replay_floor_unix,
        public_definition: read_json(args.definition_file.as_deref(), false)?,
        private_config: read_json(args.private_config_file.as_deref(), true)?
            .map(|record| beekeeper_agent_management::PrivateConfiguration { record }),
        ..Default::default()
    })
}
pub async fn create(args: &CreateArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    let request = create_request(args)?;
    let outcome =
        beekeeper_agent_management::create(&request, client, &context(&args.home)?).await?;
    println!("{outcome}");
    Ok(())
}
pub async fn update(args: &UpdateArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    let request = beekeeper_agent_management::UpdateRequest {
        operation_id: args.operation_id,
        agent_pubkey: args.agent_pubkey.clone(),
        expected_revision: args.expected_revision.clone(),
        display_name: args.display_name.clone(),
        system_prompt: args
            .system_prompt
            .as_ref()
            .map(|v| read_or_stdin(v))
            .transpose()?,
        model: args.model.clone(),
        home: None,
        public_definition: read_json(args.definition_file.as_deref(), false)?,
        private_config: read_json(args.private_config_file.as_deref(), true)?
            .map(|record| beekeeper_agent_management::PrivateConfiguration { record }),
    };
    let outcome =
        beekeeper_agent_management::update(&request, client, &context(&args.home)?).await?;
    println!("{outcome}");
    Ok(())
}
pub(super) mod local {
    pub(crate) fn validate_checkout(
        cwd: &std::path::Path,
        home: &std::path::Path,
        state: &std::path::Path,
    ) -> Result<(), crate::error::CliError> {
        let native = std::env::var_os("HOME").map(std::path::PathBuf::from);
        beekeeper_agent_management::validate_workspace(cwd, home, state, native.as_deref())
            .map_err(Into::into)
    }
}

pub(crate) use beekeeper_agent_management::authorize_project;

fn read_json<T: serde::de::DeserializeOwned>(
    path: Option<&std::path::Path>,
    private: bool,
) -> Result<Option<T>, CliError> {
    let Some(path) = path else { return Ok(None) };
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| CliError::Usage("configuration file unavailable".into()))?;
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err(CliError::Usage(
            "configuration must be a bounded regular JSON file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if private && metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Usage(
                "private configuration file must be owner-only".into(),
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = private;
    serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|_| CliError::Usage("configuration file unreadable".into()))?,
    )
    .map(Some)
    .map_err(|_| CliError::Usage("configuration JSON malformed".into()))
}
/// Actor-wide signed lifecycle request with a caller-local immutable journal.
#[derive(Args, Debug)]
pub struct LifecycleArgs {
    /// Stable operation identity; retry unchanged.
    #[arg(long)]
    pub operation_id: Uuid,
    /// Managed actor public key.
    #[arg(long)]
    pub agent_pubkey: String,
    /// Known actor channel; discovery supplements these hints.
    #[arg(long)]
    pub channel: Vec<Uuid>,
    /// Caller-local journal home, not executor access.
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Signed receipt observation bound.
    #[arg(long, default_value_t=60, value_parser=clap::value_parser!(u64).range(1..=300))]
    pub wait_secs: u64,
}
impl LifecycleArgs {
    fn request(&self) -> beekeeper_agent_management::LifecycleRequest {
        beekeeper_agent_management::LifecycleRequest {
            operation_id: self.operation_id,
            agent_pubkey: self.agent_pubkey.clone(),
            channels: self.channel.clone(),
            wait_secs: self.wait_secs,
        }
    }
}
/// Owner deletion bound to the exact public definition; missing terminal proof stays pending.
#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Actor-wide lifecycle scope.
    #[command(flatten)]
    pub lifecycle: LifecycleArgs,
    /// Exact current owner30177 event ID.
    #[arg(long)]
    pub expected_revision: String,
    /// Info/deploy-only remote provider cannot acknowledge termination.
    #[arg(long)]
    pub remote_deployment: bool,
}
pub async fn status(args: &LifecycleArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    println!(
        "{}",
        beekeeper_agent_management::status(&args.request(), client).await?
    );
    Ok(())
}
pub async fn stop(args: &LifecycleArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    println!(
        "{}",
        beekeeper_agent_management::stop(&args.request(), client, &context(&args.home)?).await?
    );
    Ok(())
}
pub async fn start(args: &CreateArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    println!(
        "{}",
        beekeeper_agent_management::start(&create_request(args)?, client, &context(&args.home)?)
            .await?
    );
    Ok(())
}
pub async fn delete(args: &DeleteArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    let scope = &args.lifecycle;
    let request = beekeeper_agent_management::DeleteRequest {
        operation_id: scope.operation_id,
        agent_pubkey: scope.agent_pubkey.clone(),
        channels: scope.channel.clone(),
        wait_secs: scope.wait_secs,
        expected_revision: args.expected_revision.clone(),
        remote_deployment: args.remote_deployment,
    };
    println!(
        "{}",
        beekeeper_agent_management::delete(&request, client, &context(&scope.home)?).await?
    );
    Ok(())
}

/// Stop and successor admission retain two distinct stable operation identities.
#[derive(Args, Debug)]
pub struct RestartArgs {
    /// Successor definition/context; requires --agent-pubkey.
    #[command(flatten)]
    pub create: CreateArgs,
    /// Stable UUID for actor-wide stop, distinct from successor --operation-id.
    #[arg(long)]
    pub stop_operation_id: Uuid,
}
pub async fn restart(args: &RestartArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    let actor = args
        .create
        .agent_pubkey
        .clone()
        .ok_or_else(|| CliError::Usage("restart requires --agent-pubkey".into()))?;
    if args.stop_operation_id == args.create.operation_id {
        return Err(CliError::Usage(
            "stop and successor operations must differ".into(),
        ));
    }
    let stop = beekeeper_agent_management::LifecycleRequest {
        operation_id: args.stop_operation_id,
        agent_pubkey: actor,
        channels: vec![args.create.channel],
        wait_secs: args.create.wait_secs,
    };
    println!(
        "{}",
        beekeeper_agent_management::restart(
            &stop,
            &create_request(&args.create)?,
            client,
            &context(&args.create.home)?
        )
        .await?
    );
    Ok(())
}

/// Public owner start with no executor filesystem/socket.
#[derive(Args, Debug)]
pub struct RemoteStartArgs {
    /// Stable caller operation UUID.
    #[arg(long)]
    pub operation_id: Uuid,
    /// Existing owner-attested actor.
    #[arg(long)]
    pub agent_pubkey: String,
    /// Exact current public definition revision.
    #[arg(long)]
    pub expected_revision: String,
    /// Enrolled private or public transport channel.
    #[arg(long)]
    pub channel: Uuid,
    /// Existing owner-attested worker public key.
    #[arg(long)]
    pub provider_authority: String,
    /// Commissioned runtime alias.
    #[arg(long)]
    pub provider_instance: String,
    /// Existing authorized project association.
    #[arg(long)]
    pub project: Option<String>,
    /// Caller journal home; no executor filesystem is needed.
    #[arg(long)]
    pub home: Option<PathBuf>,
    #[arg(long, default_value_t=60, value_parser=clap::value_parser!(u64).range(1..=300))]
    pub wait_secs: u64,
}
#[derive(Args, Debug)]
pub struct RemoteRestartArgs {
    /// Relay-only successor request.
    #[command(flatten)]
    pub start: RemoteStartArgs,
    /// Distinct actor-wide stop operation.
    #[arg(long)]
    pub stop_operation_id: Uuid,
}
fn remote_request(args: &RemoteStartArgs) -> beekeeper_agent_management::RemoteStartRequest {
    beekeeper_agent_management::RemoteStartRequest {
        operation_id: args.operation_id,
        agent_pubkey: args.agent_pubkey.clone(),
        expected_revision: args.expected_revision.clone(),
        channel: args.channel,
        provider_authority: args.provider_authority.clone(),
        provider_instance: args.provider_instance.clone(),
        project: args.project.clone(),
        wait_secs: args.wait_secs,
    }
}
pub async fn start_remote(
    args: &RemoteStartArgs,
    client: &BeekeeperClient,
) -> Result<(), CliError> {
    println!(
        "{}",
        beekeeper_agent_management::start_remote(
            &remote_request(args),
            client,
            &context(&args.home)?
        )
        .await?
    );
    Ok(())
}
pub async fn restart_remote(
    args: &RemoteRestartArgs,
    client: &BeekeeperClient,
) -> Result<(), CliError> {
    let stop = beekeeper_agent_management::LifecycleRequest {
        operation_id: args.stop_operation_id,
        agent_pubkey: args.start.agent_pubkey.clone(),
        channels: vec![args.start.channel],
        wait_secs: args.start.wait_secs,
    };
    println!(
        "{}",
        beekeeper_agent_management::restart_remote(
            &stop,
            &remote_request(&args.start),
            client,
            &context(&args.start.home)?
        )
        .await?
    );
    Ok(())
}

#[derive(clap::Subcommand)]
pub enum AgentsCmd {
    /// Commission an identity and its attributed headless work context.
    Create(CreateArgs),
    /// Publish an authorized revision of an existing managed identity.
    #[command(visible_alias = "configure")]
    Update(UpdateArgs),
    /// Observe exact signed responsibilities and current runtime lease evidence.
    Status(LifecycleArgs),
    /// Start through shared host custody; already adopted execution is reused.
    Start(CreateArgs),
    /// Start an enrolled actor using relay-only shared management.
    StartRemote(RemoteStartArgs),
    /// Stop all observed responsibilities, retaining custody until confirmed.
    Stop(LifecycleArgs),
    /// Await actor-wide terminal stop, then admit a new responsibility.
    Restart(RestartArgs),
    /// Stop all responsibilities then start using relay-only shared management.
    RestartRemote(RemoteRestartArgs),
    /// Retire and tombstone only with truthful actor-wide terminal proof.
    Delete(DeleteArgs),
    /// Retain an original signed definition as migration evidence; never starts or repairs execution.
    SupplyMigrationDefinition(SupplyMigrationDefinitionArgs),
    /// Open a prefilled create-agent form in the owner's Beekeeper Desktop
    DraftCreate {
        /// Current channel UUID; the new agent is added here after save
        #[arg(long)]
        channel: String,
        /// Proposed agent name
        #[arg(long)]
        display_name: String,
        /// Proposed instructions; use '-' to read from stdin
        #[arg(long)]
        system_prompt: String,
    },
    /// Open a prefilled edit-agent form in the owner's Beekeeper Desktop
    DraftUpdate {
        /// Current channel UUID
        #[arg(long)]
        channel: String,
        /// Current name of the personal agent to update
        #[arg(long)]
        agent_name: String,
        #[arg(long)]
        display_name: Option<String>,
        /// Replacement instructions; use '-' to read from stdin
        #[arg(long)]
        system_prompt: Option<String>,
        #[arg(long)]
        runtime: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_enum)]
        respond_to: Option<RespondToArg>,
    },
    /// Submit a NIP-IA archive request for an identity (kind 9035)
    #[command(
        after_help = "Auth flow: when target != signer, the CLI fetches the target's kind:0 and \
attaches its owner-auth tag. On extraction failure it retries once (common cause: profile \
republish in progress). If the retry also fails, the command exits with an error — use \
--admin to bypass this guard when your key is a relay admin.\n\n\
Suggested --reason codes (unknown values are allowed): rotated, retired, \
bot-rebuilt, left-organization, spam\n\n\
Archiving a third-party identity is a human owner/admin action: an agent \
running under BEEKEEPER_AUTH_TAG signs as itself, so it can only ever satisfy \
the self path (target == signer) — not the owner-of-agent path for another \
identity.\n\n\
Examples:\n  \
bee agents archive <PUBKEY> --reason retired\n  \
bee agents archive <PUBKEY> --reason bot-rebuilt --replaced-by <NEW_PUBKEY>"
    )]
    Archive {
        /// Target identity pubkey (hex)
        target_pubkey: String,
        /// Machine-readable reason code, max 64 UTF-8 bytes
        #[arg(long)]
        reason: Option<String>,
        /// Rotation pointer pubkey (hex); must differ from the target
        #[arg(long)]
        replaced_by: Option<String>,
        /// Optional human-readable note (not parsed for authorization)
        #[arg(long, default_value = "")]
        content: String,
        /// Allow sending without owner-auth attestation after extraction fails
        /// (relay-admin path). Use only when your key is a relay admin; ordinary
        /// owners do not need this flag. Without it, auth-extraction failure after
        /// one automatic retry is a hard error rather than a silent bare send.
        #[arg(long, default_value_t = false)]
        admin: bool,
    },
    /// Submit a NIP-IA unarchive request for an identity (kind 9036)
    #[command(
        after_help = "Auth flow: same as `archive` — retries kind:0 fetch once on \
extraction failure, then exits with an error if still unresolvable. Use --admin to bypass \
for relay-admin callers.\n\n\
Examples:\n  \
bee agents unarchive <PUBKEY> --reason returned"
    )]
    Unarchive {
        /// Target identity pubkey (hex)
        target_pubkey: String,
        /// Machine-readable reason code, max 64 UTF-8 bytes
        #[arg(long)]
        reason: Option<String>,
        /// Optional human-readable note (not parsed for authorization)
        #[arg(long, default_value = "")]
        content: String,
        /// Allow sending without owner-auth attestation after extraction fails
        /// (relay-admin path). Use only when your key is a relay admin; ordinary
        /// owners do not need this flag. Without it, auth-extraction failure after
        /// one automatic retry is a hard error rather than a silent bare send.
        #[arg(long, default_value_t = false)]
        admin: bool,
    },
    /// Read the relay's current NIP-IA archive snapshot (kind 13535)
    #[command(
        after_help = "Verifies the snapshot's NIP-11 `self` authorship, event id, signature, \
and NIP-70 `-` protection tag before trusting it. Any trust failure is a \
nonzero-exit error, never a false-empty success — this command's whole \
purpose is verification.\n\n\
Examples:\n  \
bee agents archived"
    )]
    Archived,
}
