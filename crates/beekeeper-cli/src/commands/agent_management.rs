//! Argument and output adapters for the shared headless management service.
use crate::{client::BeekeeperClient, error::CliError, validate::read_or_stdin, RespondToArg};
use clap::Args;
use std::path::PathBuf;
use uuid::Uuid;
/// Create an agent on this machine's host and start it in a channel.
#[derive(Args, Debug)]
pub struct CreateArgs {
    /// A UUID you mint for this request (`uuidgen`). Reuse the same one to
    /// retry safely after a lost reply.
    #[arg(long)]
    pub operation_id: Uuid,
    /// Channel UUID the agent joins (from `bee channels list`). You must be
    /// its owner.
    #[arg(long)]
    pub channel: Uuid,
    /// The agent's name as channel members see it.
    #[arg(long)]
    pub display_name: String,
    /// Instructions the agent runs with, or '-' to read them from stdin.
    #[arg(long)]
    pub system_prompt: String,
    /// Host state directory, if not BEEKEEPER_HOST_HOME or the OS home: the
    /// same value given to `bee host commission --home`.
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Directory on this machine the agent works in (an existing checkout).
    /// Stays local; never published.
    #[arg(long)]
    pub cwd: PathBuf,
    /// Project coordinate to work under; the project must already be trusted
    /// on this host (`bee host trust-project`).
    #[arg(long)]
    pub project: Option<String>,
    /// Model to advertise. Defaults to the runtime's model from `bee host
    /// commission --model`.
    #[arg(long)]
    pub model: Option<String>,
    /// Which commissioned runtime runs it: `<runtime>-primary`, as minted by
    /// `bee host commission --runtime <runtime>`. Required only when the host
    /// has more than one.
    #[arg(long)]
    pub provider_instance: Option<String>,
    /// Add this channel to an existing agent whose key this host holds,
    /// instead of creating a new identity.
    #[arg(long)]
    pub agent_pubkey: Option<String>,
    /// Seconds to wait for the host's signed receipt before giving up.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=300))]
    pub wait_secs: u64,
    /// Create the identity and its files, but do not start it; `bee agents
    /// start` later with the same flags.
    #[arg(long)]
    pub no_start: bool,
    /// Unix time; the agent reads no channel history older than this when it
    /// first joins.
    #[arg(long)]
    pub replay_floor_unix: Option<u64>,
    /// Full public definition as a JSON file, instead of the flags above.
    /// Optional fields absent from the file are cleared.
    #[arg(long)]
    pub definition_file: Option<PathBuf>,
    /// Settings private to this host as an owner-only (chmod 600) JSON file.
    /// Never sent to the relay.
    #[arg(long)]
    pub private_config_file: Option<PathBuf>,
}

/// Publish a new revision of an agent's public definition.
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// A UUID you mint for this request (`uuidgen`). Reuse it to retry.
    #[arg(long)]
    pub operation_id: Uuid,
    /// The agent's public key (64-char lowercase hex), from `bee agents
    /// create` output or `bee agents status`.
    #[arg(long)]
    pub agent_pubkey: String,
    /// Event ID of the definition revision you are editing (kind 30177),
    /// from `bee agents status`. Refused if someone published a newer one.
    #[arg(long)]
    pub expected_revision: String,
    /// New name, if changing it.
    #[arg(long)]
    pub display_name: Option<String>,
    /// New instructions, if changing them; '-' reads stdin.
    #[arg(long)]
    pub system_prompt: Option<String>,
    /// New model, if changing it.
    #[arg(long)]
    pub model: Option<String>,
    /// Where to journal this request. No host files or socket are needed to
    /// update: it is a relay write.
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Full public definition as a JSON file, instead of the flags above.
    /// Optional fields absent from the file are cleared.
    #[arg(long)]
    pub definition_file: Option<PathBuf>,
    /// Settings private to the running host as an owner-only (chmod 600)
    /// JSON file. Never sent to the relay.
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
    let flag = if private {
        "--private-config-file"
    } else {
        "--definition-file"
    };
    let shown = path.display();
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| CliError::Usage(format!("{flag} {shown}: no such file")))?;
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err(CliError::Usage(format!(
            "{flag} {shown} must be a regular JSON file under 1 MiB (not a symlink or directory)"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if private && metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Usage(format!(
                "{flag} {shown} is readable by others; run `chmod 600 {shown}`"
            )));
        }
    }
    serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|_| CliError::Usage(format!("{flag} {shown}: cannot be read")))?,
    )
    .map(Some)
    .map_err(|error| CliError::Usage(format!("{flag} {shown} is not valid JSON: {error}")))
}
/// Which agent, and how long to wait for the host's signed receipt.
#[derive(Args, Debug)]
pub struct LifecycleArgs {
    /// A UUID you mint for this request (`uuidgen`). Reuse it to retry.
    #[arg(long)]
    pub operation_id: Uuid,
    /// The agent's public key (64-char lowercase hex), from `bee agents
    /// create` output.
    #[arg(long)]
    pub agent_pubkey: String,
    /// A channel the agent is in (repeatable). Optional: a hint that speeds
    /// up finding where it runs.
    #[arg(long)]
    pub channel: Vec<Uuid>,
    /// Where to journal this request; no host files or socket are needed.
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Seconds to wait for the host's signed receipt before giving up.
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
/// Retire an agent for good, bound to the definition revision you last saw.
#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Which agent, and how long to wait.
    #[command(flatten)]
    pub lifecycle: LifecycleArgs,
    /// Event ID of the agent's current definition (kind 30177), from `bee
    /// agents status`. Refused if someone published a newer one.
    #[arg(long)]
    pub expected_revision: String,
    /// The agent runs on a remote host that only reports and cannot confirm a
    /// stop. Without this, delete waits for every host's signed stop receipt.
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

/// Stop an agent, wait for it to confirm, then start it again.
#[derive(Args, Debug)]
pub struct RestartArgs {
    /// The definition to start again with; --agent-pubkey is required.
    #[command(flatten)]
    pub create: CreateArgs,
    /// A second UUID you mint for the stop half; must differ from
    /// --operation-id, which names the start half.
    #[arg(long)]
    pub stop_operation_id: Uuid,
}
pub async fn restart(args: &RestartArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    let actor = args.create.agent_pubkey.clone().ok_or_else(|| {
        CliError::Usage(
            "restart requires --agent-pubkey <hex> naming the agent to restart (from `bee \
                 agents create` output)"
                .into(),
        )
    })?;
    if args.stop_operation_id == args.create.operation_id {
        return Err(CliError::Usage(
            "--stop-operation-id and --operation-id must be two different UUIDs (mint one with \
             `uuidgen` for each)"
                .into(),
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

/// Start an agent on another machine's host, over the relay only.
#[derive(Args, Debug)]
pub struct RemoteStartArgs {
    /// A UUID you mint for this request (`uuidgen`). Reuse it to retry.
    #[arg(long)]
    pub operation_id: Uuid,
    /// The agent's public key (64-char lowercase hex).
    #[arg(long)]
    pub agent_pubkey: String,
    /// Event ID of the agent's current definition (kind 30177), from `bee
    /// agents status`.
    #[arg(long)]
    pub expected_revision: String,
    /// Channel UUID the agent is in.
    #[arg(long)]
    pub channel: Uuid,
    /// Public key of the host (provider) that should run it, as printed by
    /// `bee host commission` on that machine (`provider_pubkey`).
    #[arg(long)]
    pub provider_authority: String,
    /// Which of that host's commissioned runtimes runs it:
    /// `<runtime>-primary`.
    #[arg(long)]
    pub provider_instance: String,
    /// Project coordinate already trusted on that host.
    #[arg(long)]
    pub project: Option<String>,
    /// Where to journal this request; no host files or socket are needed.
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Seconds to wait for the host's signed receipt before giving up.
    #[arg(long, default_value_t=60, value_parser=clap::value_parser!(u64).range(1..=300))]
    pub wait_secs: u64,
}
/// Stop an agent on another machine's host, then start it again there.
#[derive(Args, Debug)]
pub struct RemoteRestartArgs {
    /// The start half.
    #[command(flatten)]
    pub start: RemoteStartArgs,
    /// A second UUID you mint for the stop half; must differ from
    /// --operation-id.
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
    /// Create an agent on this machine's host and start it in a channel
    #[command(
        after_help = "Needs a commissioned, running host on this machine: `bee host commission`, \
then `bee host install` (or `beekeeper-host run`), then `bee host status`.\n\n\
Example:\n  \
bee agents create --operation-id \"$(uuidgen)\" --channel <UUID> \\\n      \
--display-name helper --system-prompt - --cwd /srv/work/repo \\\n      \
--provider-instance acme-primary < prompt.md"
    )]
    Create(CreateArgs),
    /// Publish a new revision of an agent's definition (name, instructions, model)
    #[command(visible_alias = "configure")]
    Update(UpdateArgs),
    /// Show an agent's signed definition and whether a host is running it right now
    Status(LifecycleArgs),
    /// Start an agent created with --no-start or stopped; reuses it if already running
    Start(CreateArgs),
    /// Start an agent on another machine's host, over the relay only (no local host)
    StartRemote(RemoteStartArgs),
    /// Stop an agent everywhere it runs; its identity and files stay until `delete`
    Stop(LifecycleArgs),
    /// Stop an agent, wait for it to confirm, then start it again with a new definition
    Restart(RestartArgs),
    /// Stop an agent on another machine's host, then start it again there
    RestartRemote(RemoteRestartArgs),
    /// Retire an agent for good; refused until every host confirms it has stopped
    Delete(DeleteArgs),
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
