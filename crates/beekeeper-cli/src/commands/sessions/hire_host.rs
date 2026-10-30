//! Thin owner-authenticated adapter for one explicitly isolated hire scope.
use crate::{client::BeekeeperClient, error::CliError};
use beekeeper_agent_management::{
    hire::{self, Scope},
    hire_host::{LocalHost, Policy},
    role_install, Context,
};
use clap::{Args, Subcommand};
use serde_json::json;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Canonical role installation and explicitly scoped headless hire answering.
#[derive(Subcommand)]
pub enum Command {
    /// Install one resolved role through canonical managed identity creation; do not start it.
    InstallRole(Install),
    /// Answer hires as the owner, using the existing host for all execution.
    Run(Run),
}
/// Explicit role installation inputs.
#[derive(Args)]
pub struct Install {
    #[arg(long)]
    operation_id: Uuid,
    #[arg(long)]
    home: PathBuf,
    #[arg(long)]
    project: String,
    #[arg(long)]
    channel: Uuid,
    #[arg(long)]
    role: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    provider_instance: String,
    #[arg(long)]
    model: String,
    /// Concurrent responsibilities, including the primary context; choose at least two to hire while started.
    #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u32).range(1..=32))]
    parallelism: u32,
    /// Installed shipped template catalog, explicitly selected.
    #[arg(long)]
    templates: PathBuf,
}
/// One owner/project/host, with an explicit legacy-responder cutover boundary.
#[derive(Args)]
pub struct Run {
    #[arg(long)]
    home: PathBuf,
    #[arg(long)]
    project: String,
    #[arg(long)]
    channel: Uuid,
    #[arg(long)]
    genesis: String,
    #[arg(long)]
    actor: String,
    #[arg(long)]
    provider_instance: String,
    #[arg(long)]
    model: String,
    /// Protected JSON with Desktop policy fields. Defaults are enabled/all installed/10 seats/all commissioned.
    #[arg(long)]
    policy_file: Option<PathBuf>,
    /// Required: this identity/project is excluded from legacy Desktop responders.
    /// This local listener does not claim distributed or mixed-client fencing.
    #[arg(long, required = true)]
    isolated_cutover: bool,
    /// Reconcile one complete history pass, then exit (for deterministic supervision).
    #[arg(long)]
    once: bool,
    #[arg(long,default_value_t=5,value_parser=clap::value_parser!(u64).range(1..=300))]
    wait_secs: u64,
}
fn context(home: &Path) -> Result<Context, CliError> {
    if !home.is_absolute() {
        return Err(CliError::Usage(
            "--home must be an explicit absolute isolated service home".into(),
        ));
    }
    Ok(Context {
        home: home.into(),
        instance: beekeeper_host_core::layout::Instance::from_env().map_err(CliError::Usage)?,
        native_home: std::env::var_os("HOME").map(PathBuf::from),
    })
}
pub(super) async fn dispatch(command: Command, client: &BeekeeperClient) -> Result<(), CliError> {
    match command {
        Command::InstallRole(a) => {
            let result = role_install::install(
                &context(&a.home)?,
                client,
                &role_install::RoleInstallRequest {
                    operation_id: a.operation_id,
                    channel: a.channel,
                    project: a.project,
                    role: a.role,
                    name: a.name,
                    provider_instance: a.provider_instance,
                    model: a.model,
                    parallelism: a.parallelism,
                    templates: a.templates,
                },
            )
            .await?;
            println!("{result}");
            Ok(())
        }
        Command::Run(a) => run(a, client).await,
    }
}
async fn run(args: Run, client: &BeekeeperClient) -> Result<(), CliError> {
    if !args.isolated_cutover {
        return Err(CliError::Usage(
            "explicit isolated cutover is required".into(),
        ));
    }
    if client.verified_auth_tag_owner_hex()?.is_some() {
        return Err(CliError::Auth(
            "hire-host requires the direct owner signer".into(),
        ));
    }
    let context = context(&args.home)?;
    let events=client.query_all(json!({"kinds":[beekeeper_core::kind::KIND_CODING_SESSION_GENESIS],"#h":[args.channel.to_string()],"ids":[args.genesis]})).await?;
    let genesis = events
        .into_iter()
        .filter_map(|v| serde_json::from_value::<nostr::Event>(v).ok())
        .find(|e| {
            e.id.to_hex() == args.genesis
                && e.pubkey == client.keys().public_key()
                && e.verify().is_ok()
        })
        .ok_or_else(|| CliError::Auth("exact owner-signed genesis unavailable".into()))?;
    let scope = Scope {
        project: args.project,
        channel: args.channel,
        genesis,
    };
    let policy = match args.policy_file {
        None => Policy::default(),
        Some(path) => {
            let meta =
                std::fs::symlink_metadata(&path).map_err(|e| CliError::Other(e.to_string()))?;
            if !meta.file_type().is_file() || meta.len() > 65536 {
                return Err(CliError::Usage(
                    "policy must be a bounded regular file".into(),
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o077 != 0 {
                    return Err(CliError::Auth(
                        "policy must be owner-only (chmod 600)".into(),
                    ));
                }
            }
            serde_json::from_slice(
                &std::fs::read(path).map_err(|e| CliError::Other(e.to_string()))?,
            )
            .map_err(|e| CliError::Usage(e.to_string()))?
        }
    };
    let adapter = LocalHost {
        context: &context,
        client,
        actor: args.actor,
        provider_instance: args.provider_instance,
        model: args.model,
        policy,
    };
    let mut settled = std::collections::BTreeSet::new();
    loop {
        // Durable pending requests lead discovery. Full signed history follows;
        // there is no cursor that could outrun a journaled operation.
        let mut requests = hire::pending_requests(&context, client, &scope)?;
        for raw in client.query_all(json!({"kinds":[beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND],"#h":[scope.channel.to_string()]})).await? {
            let Ok(event)=serde_json::from_value::<nostr::Event>(raw) else{continue};
            if event.verify().is_err(){continue}
            let Ok(p)=beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(&event.content) else{continue};
            if matches!(p.action,beekeeper_core::coding_session_lifecycle_command::CodingSessionLifecycleAction::SessionHire{genesis_ref,..} if genesis_ref==scope.genesis.id.to_hex()) && !requests.iter().any(|e|e.id==event.id){requests.push(event)}
        }
        for request in requests {
            if settled.contains(&request.id) {
                continue;
            }
            let result =
                hire::answer(&context, client, &adapter, &scope, &request, args.wait_secs).await;
            match result {
                Ok(disposition) => {
                    if matches!(
                        disposition,
                        hire::Disposition::Ready { .. }
                            | hire::Disposition::Refused { .. }
                            | hire::Disposition::PolicyRefused {
                                disclosure_confirmed: true,
                                ..
                            }
                    ) {
                        settled.insert(request.id);
                    }
                    println!(
                        "{}",
                        json!({"request_event_id":request.id.to_hex(),"disposition":disposition})
                    );
                }
                Err(error) => println!(
                    "{}",
                    json!({"request_event_id":request.id.to_hex(),"state":"unanswered","reason":error.to_string()})
                ),
            }
        }
        if args.once {
            return Ok(());
        }
        tokio::select! {
            result=tokio::signal::ctrl_c()=>return result.map_err(|e|CliError::Other(e.to_string())),
            _=tokio::time::sleep(std::time::Duration::from_secs(2))=>{}
        }
    }
}
