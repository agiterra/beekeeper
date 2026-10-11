//! Explicit, authorized headless project checkout and agents-source materialization.

use crate::{client::BeekeeperClient, error::CliError};
use beekeeper_core::project_pack_source::decode_project_pack_source;
use beekeeper_host_core::{
    atomic_write,
    config::HostConfig,
    env::PROJECTS_FILE_NAME,
    layout,
    project::{AgentsRepoRecord, ProjectsFile},
};
use clap::Args;
use nostr::Event;
use serde_json::json;
use std::path::PathBuf;

/// Bind this executor's folders to the project's signed source without a desktop.
#[derive(Args, Debug)]
pub struct ProjectArgs {
    /// Existing project coordinate.
    #[arg(long)]
    pub project: String,
    /// Existing workspace directory, kept private on this executor.
    #[arg(long)]
    pub checkout: PathBuf,
    /// Existing git clone of the project's owner-published agents repository.
    #[arg(long)]
    pub agents_repo: PathBuf,
    /// Explicit commissioned service home.
    #[arg(long)]
    pub home: Option<PathBuf>,
}

fn fail(error: impl std::fmt::Display) -> CliError {
    CliError::Other(error.to_string())
}

/// Verify relay authority, fetch the selected immutable input, then record local paths.
pub async fn trust_project(args: &ProjectArgs, client: &BeekeeperClient) -> Result<(), CliError> {
    if client.verified_auth_tag_owner_hex()?.is_some() {
        return Err(CliError::Auth(
            "project materialization requires the commissioning owner".into(),
        ));
    }
    let project = beekeeper_core::kind::normalize_project_coordinate(&args.project)
        .ok_or_else(|| CliError::Usage("invalid project coordinate".into()))?;
    let home = args
        .home
        .clone()
        .map(Ok)
        .unwrap_or_else(layout::home_dir)
        .map_err(CliError::Usage)?;
    let instance = layout::Instance::from_env().map_err(CliError::Usage)?;
    let _lock = beekeeper_host_core::ownership::management_lock(&home, instance).map_err(fail)?;
    let config = HostConfig::load(&layout::host_config_path(&home, instance))
        .map_err(fail)?
        .ok_or_else(|| CliError::Usage("host is not commissioned".into()))?;
    let relay = crate::client::to_ws_url(client.relay_url());
    if beekeeper_host_core::record::canonical_relay_key(&relay)
        != beekeeper_host_core::record::canonical_relay_key(&config.relay_url)
    {
        return Err(CliError::Auth(
            "active relay differs from commissioned host".into(),
        ));
    }
    let records =
        beekeeper_host_core::record::load_provider_store_from(&config.record_store_path())
            .map_err(fail)?;
    let provider = records
        .get(&config.relay_url)
        .ok_or_else(|| fail("provider record missing"))?;
    let owner = beekeeper_sdk::nip_oa::verify_auth_tag(
        provider
            .auth_tag
            .as_deref()
            .ok_or_else(|| fail("provider attestation missing"))?,
        &nostr::PublicKey::from_hex(&config.provider_pubkey).map_err(fail)?,
    )
    .map_err(fail)?;
    if owner != client.keys().public_key() {
        return Err(CliError::Auth(
            "signer does not own commissioned provider".into(),
        ));
    }
    super::agent_management::authorize_project(client, &project, &owner.to_hex()).await?;
    let creator = project
        .split(':')
        .nth(1)
        .ok_or_else(|| fail("project creator missing"))?;
    let rows = client
        .query_all(json!({"kinds":[30624],"authors":[creator],"#d":[project]}))
        .await?;
    let mut sources = rows
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .filter(|event| {
            event.pubkey.to_hex() == creator && beekeeper_core::verify_event(event).is_ok()
        })
        .filter_map(|event| {
            decode_project_pack_source(&event)
                .ok()
                .filter(|source| source.project() == project)
                .map(|source| (event, source))
        })
        .collect::<Vec<_>>();
    sources.sort_by(|(a, _), (b, _)| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let (source_event, source) = sources.first().ok_or_else(|| CliError::Refused("no verified creator-authored project agents source (30624); publish it with bee packs set-source".into()))?;
    if !matches!(source.path(), "." | "") {
        return Err(CliError::Refused(
            "this host hook path supports agents definitions at repository root only".into(),
        ));
    }
    let checkout = std::fs::canonicalize(&args.checkout).map_err(fail)?;
    let agents = std::fs::canonicalize(&args.agents_repo).map_err(fail)?;
    super::agent_management::local::validate_checkout(
        &checkout,
        &home,
        &config.provider_state_dir,
    )?;
    super::agent_management::local::validate_checkout(&agents, &home, &config.provider_state_dir)?;
    let mut parts = source.repo().splitn(3, ':');
    parts.next();
    let repo_owner = parts
        .next()
        .ok_or_else(|| fail("repository owner missing"))?;
    let repo_id = parts.next().ok_or_else(|| fail("repository id missing"))?;
    let url = format!(
        "{}/git/{repo_owner}/{repo_id}",
        client.relay_url().trim_end_matches('/')
    );
    let remote = tokio::process::Command::new("git")
        .args(["remote", "get-url", "--all", "origin"])
        .current_dir(&agents)
        .output()
        .await
        .map_err(fail)?;
    if !remote.status.success()
        || !String::from_utf8_lossy(&remote.stdout)
            .lines()
            .all(|line| line.trim_end_matches('/') == url)
        || remote.stdout.is_empty()
    {
        return Err(CliError::Refused(
            "agents checkout origin differs from the authorized source on this relay".into(),
        ));
    }
    let record = AgentsRepoRecord {
        path: agents,
        ref_name: source.pin().value().into(),
        url: Some(url),
    };
    let helper = std::env::current_exe()
        .map_err(fail)?
        .parent()
        .ok_or_else(|| fail("CLI executable has no directory"))?
        .join(if cfg!(windows) {
            "git-credential-nostr.exe"
        } else {
            "git-credential-nostr"
        });
    let auth = beekeeper_session_provider::agents_checkout::ScopedGitAuth::new(
        client.keys().clone(),
        client
            .auth_tag()
            .map(|tag| serde_json::to_string(tag.as_slice()))
            .transpose()
            .map_err(fail)?,
        helper,
    )
    .map_err(fail)?;
    let files = beekeeper_session_provider::agents_checkout::read_agents_file_scoped(
        &record,
        "actions.yml",
        Some(&auth),
    )
    .await
    .map_err(fail)?;
    if files.stale {
        return Err(CliError::Refused(format!(
            "authorized agents source fetch failed; refusing materialization: {}",
            files.provenance()
        )));
    }
    let path = config.provider_state_dir.join(PROJECTS_FILE_NAME);
    let mut state: ProjectsFile = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| fail("existing project materialization is malformed; preserved"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProjectsFile::default(),
        Err(error) => return Err(fail(error)),
    };
    state.projects.insert(project.clone(), checkout);
    state.agents_repos.insert(project.clone(), record);
    atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec_pretty(&state).map_err(fail)?,
    )
    .map_err(fail)?;
    println!(
        "{}",
        json!({"project":project,"sourceEventId":source_event.id.to_hex(),"agentsCommit":files.sha,"materialized":true,"execution_readiness":"unconfirmed"})
    );
    Ok(())
}
