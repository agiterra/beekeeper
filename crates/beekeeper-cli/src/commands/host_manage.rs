//! `bee host manage` — the operations behind the menu bar's Manage Host window.
//!
//! Public on purpose: the window spawns exactly these commands, and a terminal
//! can run them too. Reads go to the Host's socket and need no key. Writes run
//! here, in a short-lived process, because the Host holds no owner key by rule
//! (`beekeeper_host::identity`) and a window must not. The key arrives as a file
//! path; its bytes go no further than this process's signing transport.
//!
//! The relay always comes from the commissioned `host.json`, never from the
//! environment: a setup signed for one relay and journaled against another is a
//! mistake this command cannot make.

use std::path::{Path, PathBuf};

use beekeeper_agent_management::project_agent_setup::{self as setup, ProviderProbe};
use beekeeper_agent_management::Context;
use beekeeper_host::client;
use beekeeper_host_core::agent_setup::{AgentSetup, AgentSetupSpec, SetupState};
use beekeeper_host_core::config::HostConfig;
use beekeeper_host_core::layout;
use clap::{Args, Subcommand};
use nostr::Keys;
use serde_json::json;
use uuid::Uuid;

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// The largest key file worth reading: an nsec is 63 bytes.
const MAX_KEY_FILE_BYTES: u64 = 512;

/// Where the owner's signature comes from.
#[derive(Args, Debug, Clone)]
pub struct OwnerArgs {
    /// Owner-only file holding the owner's key. Defaults to BEEKEEPER_PRIVATE_KEY
    /// when that is set, then ~/.nostr/key (the file git-credential-nostr reads).
    #[arg(long)]
    pub owner_key_file: Option<PathBuf>,
}

/// One setup, by id.
#[derive(Args, Debug)]
pub struct SetupArgs {
    /// The setup id `plan` printed.
    #[arg(long)]
    pub setup: Uuid,
    #[command(flatten)]
    pub owner: OwnerArgs,
}

/// A setup's choices.
#[derive(Args, Debug)]
pub struct SpecArgs {
    /// JSON file of the choices, or '-' to read them from stdin.
    #[arg(long)]
    pub spec_file: PathBuf,
    /// The shipped role templates. Defaults to BEEKEEPER_TEMPLATES_DIR, then the
    /// templates this `bee`'s install carries.
    #[arg(long)]
    pub templates: Option<PathBuf>,
    #[command(flatten)]
    pub owner: OwnerArgs,
}

/// Manage this machine's agents, without a desktop.
#[derive(Subcommand, Debug)]
pub enum ManageCmd {
    /// What this Host knows: runtimes, models on offer, projects, agents, setups,
    /// recovery requests. Needs no key.
    Inventory,
    /// Which identity a key would sign as, against this Host's owner. Prints no key.
    Signer(OwnerArgs),
    /// The roles in a trusted project's local agents clone. Needs no key; plan
    /// re-resolves the signed source.
    Roles {
        #[arg(long)]
        project: String,
    },
    /// Channels the owner can add an agent to.
    Channels(OwnerArgs),
    /// Create a private channel owned by the owner, for an agent to live in.
    CreateChannel {
        #[arg(long)]
        name: String,
        #[command(flatten)]
        owner: OwnerArgs,
    },
    /// Change an installed agent's model and effort, keeping its identity and conversation.
    /// A running execution takes it at its next boundary, and a refusal changes nothing
    SetModel {
        #[arg(long)]
        setup: Uuid,
        /// A model id from the running provider's catalog.
        #[arg(long)]
        model: String,
        /// Keeps the setup's current effort when absent.
        #[arg(long)]
        effort: Option<String>,
        #[arg(long)]
        templates: Option<PathBuf>,
        #[command(flatten)]
        owner: OwnerArgs,
    },
    /// Record a project's checkout and agents clone on this Host.
    Trust {
        #[arg(long)]
        project: String,
        #[arg(long)]
        checkout: PathBuf,
        #[arg(long)]
        agents_repo: PathBuf,
        #[command(flatten)]
        owner: OwnerArgs,
    },
    /// Bring an existing identity's key under this Host, once. Starts nothing and
    /// spends no turn. Reads this instance's desktop key store, one entry, only if
    /// this Host holds no key for it yet; macOS may ask. Prints no key.
    AdoptIdentity {
        /// The identity's public key.
        #[arg(long)]
        pubkey: String,
        #[command(flatten)]
        owner: OwnerArgs,
    },
    /// Check a setup's choices. Changes nothing.
    Preflight(SpecArgs),
    /// Check the choices and journal a setup. Publishes nothing.
    Plan(SpecArgs),
    /// Run or resume a setup. Safe to repeat: the same setup reuses the same
    /// operations and the same identity.
    Apply {
        #[command(flatten)]
        target: SetupArgs,
        /// Continue a setup that was cancelled.
        #[arg(long)]
        resume_cancelled: bool,
        #[arg(long)]
        templates: Option<PathBuf>,
    },
    /// Record that a setup should not continue, and print what already exists.
    Cancel {
        #[arg(long)]
        setup: Uuid,
    },
    /// Start a stopped agent.
    Start(SetupArgs),
    /// Stop an agent and wait for its signed receipt. Its identity stays.
    Stop(SetupArgs),
    /// A setup's journal beside a fresh signed reading.
    Inspect(SetupArgs),
}

fn fail(error: impl std::fmt::Display) -> CliError {
    CliError::Other(error.to_string())
}

fn print<T: serde::Serialize>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value).map_err(fail)?);
    Ok(())
}

struct Scope {
    context: Context,
    config: HostConfig,
    socket: PathBuf,
}

fn scope() -> Result<Scope, CliError> {
    let home = layout::home_dir().map_err(CliError::Other)?;
    let instance = layout::Instance::from_env().map_err(CliError::Usage)?;
    let config = HostConfig::load(&layout::host_config_path(&home, instance))
        .map_err(CliError::Other)?
        .ok_or_else(|| {
            CliError::Refused(
                "HOST_NOT_COMMISSIONED: this machine has no commissioned Host; run `bee host commission`"
                    .into(),
            )
        })?;
    Ok(Scope {
        socket: layout::host_socket_path(&home, instance),
        context: Context {
            home,
            instance,
            native_home: std::env::var_os("HOME").map(PathBuf::from),
        },
        config,
    })
}

/// Read a key file, refusing anything but a small owner-only regular file. The
/// error names the path and never the contents.
pub(super) fn read_key_file(path: &Path) -> Result<Keys, CliError> {
    let shown = path.display();
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| CliError::Key(format!("OWNER_KEY_UNAVAILABLE: no key file at {shown}")))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_KEY_FILE_BYTES {
        return Err(CliError::Key(format!(
            "OWNER_KEY_UNAVAILABLE: {shown} must be a small regular file, not a link or a directory"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Key(format!(
                "OWNER_KEY_UNAVAILABLE: {shown} is readable by others; run `chmod 600 {shown}`"
            )));
        }
    }
    let text = std::fs::read_to_string(path)
        .map_err(|_| CliError::Key(format!("OWNER_KEY_UNAVAILABLE: {shown} cannot be read")))?;
    Keys::parse(text.trim()).map_err(|_| {
        CliError::Key(format!(
            "OWNER_KEY_UNAVAILABLE: {shown} does not hold a key"
        ))
    })
}

/// The signer, and a phrase saying where it came from.
fn resolve_owner(
    args: &OwnerArgs,
    env_key: Option<&str>,
    delegated: bool,
    default_file: Option<PathBuf>,
) -> Result<(Keys, String), CliError> {
    if let Some(path) = &args.owner_key_file {
        return Ok((read_key_file(path)?, path.display().to_string()));
    }
    // An agent's environment must not reach owner operations by accident: it signs
    // as the agent, and the relay would say so late. An attestation, or a file
    // pointing at one, says this is that environment; neither is opened here.
    if delegated {
        return Err(CliError::Auth(
            "DELEGATED_SIGNER_REFUSED: BEEKEEPER_AUTH_TAG or BEEKEEPER_AUTH_TAG_FILE is set, so this environment signs as an agent; Manage Host needs the owner's own key, named with --owner-key-file".into(),
        ));
    }
    if let Some(key) = env_key.filter(|key| !key.is_empty()) {
        let keys = Keys::parse(key).map_err(|_| {
            CliError::Key("OWNER_KEY_UNAVAILABLE: BEEKEEPER_PRIVATE_KEY is not a key".into())
        })?;
        return Ok((keys, "BEEKEEPER_PRIVATE_KEY".to_string()));
    }
    let path = default_file.ok_or_else(|| {
        CliError::Key("OWNER_KEY_UNAVAILABLE: no key file, and no home directory to look in".into())
    })?;
    Ok((read_key_file(&path)?, path.display().to_string()))
}

fn owner_client(
    args: &OwnerArgs,
    env_key: Option<&str>,
    delegated: bool,
    config: &HostConfig,
) -> Result<BeekeeperClient, CliError> {
    let default_file = dirs::home_dir().map(|home| home.join(".nostr/key"));
    let (keys, _) = resolve_owner(args, env_key, delegated, default_file)?;
    BeekeeperClient::new(
        crate::client::normalize_relay_url(&config.relay_url),
        keys,
        None,
        None,
    )
}

async fn probe(socket: &Path) -> ProviderProbe {
    match client::status(socket).await {
        Ok(status) => ProviderProbe {
            live_pid: status.provider.live_pid(),
            state: format!("{:?}", status.provider),
        },
        Err(error) => ProviderProbe {
            live_pid: None,
            state: error.message(),
        },
    }
}

fn templates(explicit: Option<&Path>) -> Result<PathBuf, CliError> {
    super::pack::resolve_templates_dir(explicit).ok_or_else(|| {
        CliError::Refused(
            "ROLE_TEMPLATES_UNAVAILABLE: no shipped role templates were found; set BEEKEEPER_TEMPLATES_DIR or pass --templates".into(),
        )
    })
}

fn read_spec(path: &Path) -> Result<AgentSetupSpec, CliError> {
    let text = if path == Path::new("-") {
        std::io::read_to_string(std::io::stdin()).map_err(fail)?
    } else {
        std::fs::read_to_string(path)
            .map_err(|_| CliError::Usage(format!("{}: cannot be read", path.display())))?
    };
    serde_json::from_str(&text).map_err(|error| {
        CliError::Usage(format!(
            "INVALID_REQUEST: the setup choices are not valid: {error}"
        ))
    })
}

/// Print the journal; a state needing attention also exits non-zero, so a script
/// cannot read a refused setup as a finished one.
fn report(setup: &AgentSetup) -> Result<(), CliError> {
    print(setup)?;
    if setup.state == SetupState::NeedsAttention {
        let refusal = setup.last_refusal.as_ref();
        return Err(CliError::Refused(format!(
            "{}: {}",
            refusal.map_or("SETUP_NEEDS_ATTENTION", |r| r.code.as_str()),
            refusal.map_or("see the journal", |r| r.message.as_str()),
        )));
    }
    Ok(())
}

/// The Host's attested owner, for comparing a key against it before anything
/// else is enabled.
async fn signer(args: &OwnerArgs, env_key: Option<&str>, delegated: bool) -> Result<(), CliError> {
    let scope = scope()?;
    let default_file = dirs::home_dir().map(|home| home.join(".nostr/key"));
    let (keys, source) = resolve_owner(args, env_key, delegated, default_file)?;
    let inventory = client::inventory(&scope.socket).await.map_err(|error| {
        CliError::Other(beekeeper_host::inventory::describe_client_error(&error))
    })?;
    let signer = keys.public_key().to_hex();
    print(&json!({
        "signer": signer,
        "source": source,
        "hostOwner": inventory.host.owner_pubkey,
        "hostOwnerUnverified": inventory.host.owner_unverified,
        "matches": inventory.host.owner_pubkey.as_deref() == Some(signer.as_str()),
    }))
}

/// Run one `bee host manage` command. `env_key` and `delegated` come from the
/// parsed global flags, and nothing here reads them from anywhere else. A client
/// built here never carries an auth tag: owner operations sign as the owner.
pub async fn dispatch(
    command: &ManageCmd,
    env_key: Option<&str>,
    delegated: bool,
) -> Result<(), CliError> {
    match command {
        ManageCmd::Inventory => {
            let scope = scope()?;
            match client::inventory(&scope.socket).await {
                Ok(inventory) => print(&inventory),
                Err(error) => Err(CliError::Other(
                    beekeeper_host::inventory::describe_client_error(&error),
                )),
            }
        }
        ManageCmd::Signer(args) => signer(args, env_key, delegated).await,
        ManageCmd::AdoptIdentity { pubkey, owner } => {
            let scope = scope()?;
            let client = owner_client(owner, env_key, delegated, &scope.config)?;
            let source =
                super::host_manage_keystore::DesktopKeyStore::for_instance(scope.context.instance);
            let outcome = beekeeper_agent_management::adopt_identity::adopt(
                &scope.context,
                &client,
                pubkey,
                &source,
            )
            .await?;
            print(&outcome)
        }
        ManageCmd::Roles { project } => roles(project),
        ManageCmd::Channels(args) => {
            let scope = scope()?;
            let client = owner_client(args, env_key, delegated, &scope.config)?;
            print(&setup::manageable_channels(&client).await?)
        }
        ManageCmd::CreateChannel { name, owner } => {
            let scope = scope()?;
            let client = owner_client(owner, env_key, delegated, &scope.config)?;
            super::channels::cmd_create_channel(
                &client, name, "stream", "private", None, None, None,
            )
            .await
        }
        ManageCmd::Trust {
            project,
            checkout,
            agents_repo,
            owner,
        } => {
            let scope = scope()?;
            let client = owner_client(owner, env_key, delegated, &scope.config)?;
            super::host_project::trust_project(
                &super::host_project::ProjectArgs {
                    project: project.clone(),
                    checkout: checkout.clone(),
                    agents_repo: agents_repo.clone(),
                    home: None,
                },
                &client,
            )
            .await
        }
        ManageCmd::Preflight(args) => {
            let scope = scope()?;
            let client = owner_client(&args.owner, env_key, delegated, &scope.config)?;
            let spec = read_spec(&args.spec_file)?;
            let report = setup::preflight(
                &scope.context,
                &client,
                &probe(&scope.socket).await,
                &spec,
                &templates(args.templates.as_deref())?,
                None,
            )
            .await?;
            print(&report)
        }
        ManageCmd::Plan(args) => {
            let scope = scope()?;
            let client = owner_client(&args.owner, env_key, delegated, &scope.config)?;
            let planned = setup::plan(
                &scope.context,
                &client,
                &probe(&scope.socket).await,
                read_spec(&args.spec_file)?,
                &templates(args.templates.as_deref())?,
            )
            .await?;
            print(&planned)
        }
        ManageCmd::Apply {
            target,
            resume_cancelled,
            templates: explicit,
        } => {
            let scope = scope()?;
            let client = owner_client(&target.owner, env_key, delegated, &scope.config)?;
            let applied = setup::apply(
                &scope.context,
                &client,
                &probe(&scope.socket).await,
                target.setup,
                &templates(explicit.as_deref())?,
                *resume_cancelled,
            )
            .await?;
            report(&applied)
        }
        ManageCmd::Cancel { setup: id } => {
            let scope = scope()?;
            print(&setup::cancel(
                &scope.context,
                &scope.config,
                &probe(&scope.socket).await,
                *id,
            )?)
        }
        ManageCmd::Start(target) => {
            let scope = scope()?;
            let client = owner_client(&target.owner, env_key, delegated, &scope.config)?;
            report(&setup::start(&scope.context, &client, target.setup).await?)
        }
        ManageCmd::Stop(target) => {
            let scope = scope()?;
            let client = owner_client(&target.owner, env_key, delegated, &scope.config)?;
            report(&setup::stop(&scope.context, &client, target.setup).await?)
        }
        ManageCmd::SetModel {
            setup: setup_id,
            model,
            effort,
            templates: explicit,
            owner,
        } => {
            let scope = scope()?;
            let client = owner_client(owner, env_key, delegated, &scope.config)?;
            let plan = setup::plan_model_change(
                &scope.context,
                &client,
                &probe(&scope.socket).await,
                *setup_id,
                model,
                effort.as_deref(),
                &templates(explicit.as_deref())?,
            )
            .await?;
            let switch = match &plan.session {
                Some(session) => {
                    let report = super::sessions::model::switch_model(
                        &client,
                        &plan.channel.to_string(),
                        session,
                        Some(&plan.session_ref.to_string()),
                        &plan.selection,
                        None,
                    )
                    .await?;
                    if report["modelStatus"] != "confirmed"
                        || report["model"].as_str() != Some(plan.selection.as_str())
                    {
                        return Err(CliError::Refused(format!(
                            "MODEL_NOT_SWITCHED: the journal and definition were not changed. \
                             Provider status {}, provider-signed model {}. {} If the switch \
                             applied, or applies later at its boundary, the execution differs \
                             from the definition and mentions refuse with \
                             AGENT_MODEL_UPDATE_PENDING until they agree. Run this again to \
                             record it, or name the previous model to go back.",
                            report["deliveryStatus"],
                            report["model"],
                            report["delivery"]
                                .as_str()
                                .unwrap_or("no answer from the provider")
                        )));
                    }
                    report
                }
                None => serde_json::Value::Null,
            };
            let recorded = setup::record_model_change(
                &scope.context,
                &client,
                *setup_id,
                model,
                effort.as_deref(),
            )
            .await?;
            println!(
                "{}",
                json!({
                    "setupId": recorded.setup_id,
                    "agentPubkey": recorded.agent_pubkey,
                    "selection": recorded.spec.selection(),
                    "revision": recorded.revision,
                    "session": plan.session,
                    "switch": switch,
                })
            );
            Ok(())
        }
        ManageCmd::Inspect(target) => {
            let scope = scope()?;
            let client = owner_client(&target.owner, env_key, delegated, &scope.config)?;
            print(&setup::inspect(&scope.context, &client, target.setup).await?)
        }
    }
}

/// The roles as the local clone has them, at the ref the project pins. Advisory:
/// `plan` resolves the signed source and is what refuses.
fn roles(project: &str) -> Result<(), CliError> {
    let scope = scope()?;
    let projects: beekeeper_host_core::project::ProjectsFile = serde_json::from_slice(
        &std::fs::read(
            scope
                .config
                .provider_state_dir
                .join(beekeeper_host_core::env::PROJECTS_FILE_NAME),
        )
        .map_err(fail)?,
    )
    .map_err(fail)?;
    let record = projects.agents_repos.get(project).ok_or_else(|| {
        CliError::Refused(format!(
            "PROJECT_NOT_TRUSTED: this Host has no agents clone on record for {project}"
        ))
    })?;
    let git = |args: &[&str]| -> Result<Option<String>, CliError> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&record.path)
            .args(args)
            .output()
            .map_err(fail)?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
    };
    let commit = git(&["rev-parse", &record.ref_name])?.map(|text| text.trim().to_string());
    let team = git(&["show", &format!("{}:team.yml", record.ref_name)])?
        .map(|text| beekeeper_persona::team::parse_team_yml(&text, Path::new("team.yml")))
        .transpose()
        .map_err(|error| {
            CliError::Refused(format!(
                "ROLE_SOURCE_UNAVAILABLE: team.yml is invalid: {error}"
            ))
        })?;
    let mut slugs: std::collections::BTreeSet<String> = git(&[
        "ls-tree",
        "--name-only",
        &format!("{}:roles", record.ref_name),
    ])?
    .unwrap_or_default()
    .lines()
    .map(|name| name.trim_end_matches(".md").to_string())
    .collect();
    if let Some(team) = &team {
        slugs.extend(team.roles.keys().cloned());
    }
    let rows: Vec<_> = slugs
        .iter()
        .map(|slug| {
            let declared = team.as_ref().map(|team| team.role(slug));
            json!({
                "slug": slug,
                "declaredRuntime": declared.as_ref().and_then(|role| role.runtime.clone()),
                "declaredModel": declared.as_ref().and_then(|role| role.model.clone()),
            })
        })
        .collect();
    print(&json!({
        "project": project,
        "commit": commit,
        "lead": team.as_ref().map(|team| team.lead.clone().unwrap_or_else(|| "lead".to_string())),
        "roles": rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_key(dir: &Path, name: &str, mode: u32, bytes: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn message(result: Result<(Keys, String), CliError>) -> String {
        match result {
            Err(error) => error.to_string(),
            Ok(_) => panic!("expected a refusal"),
        }
    }

    #[test]
    fn a_key_file_must_be_owner_only_and_a_refusal_never_quotes_it() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::generate();
        let secret = keys.secret_key().to_secret_hex();
        let good = write_key(dir.path(), "good", 0o600, &format!("{secret}\n"));
        assert_eq!(
            read_key_file(&good).unwrap().public_key(),
            keys.public_key()
        );

        let open = write_key(dir.path(), "open", 0o644, &secret);
        let refused = read_key_file(&open).err().unwrap().to_string();
        assert!(refused.contains("readable by others"), "{refused}");
        assert!(!refused.contains(&secret));

        let garbage = write_key(dir.path(), "garbage", 0o600, "not-a-key-SENTINEL");
        let refused = read_key_file(&garbage).err().unwrap().to_string();
        assert!(refused.contains("does not hold a key"), "{refused}");
        assert!(!refused.contains("SENTINEL"), "{refused}");

        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&good, &link).unwrap();
        assert!(read_key_file(&link)
            .err()
            .unwrap()
            .to_string()
            .contains("regular file"));
        assert!(read_key_file(&dir.path().join("absent")).is_err());
    }

    #[test]
    fn an_explicit_file_wins_then_the_environment_then_the_default_file() {
        let dir = tempfile::tempdir().unwrap();
        let (file_keys, env_keys, default_keys) =
            (Keys::generate(), Keys::generate(), Keys::generate());
        let file = write_key(
            dir.path(),
            "file",
            0o600,
            &file_keys.secret_key().to_secret_hex(),
        );
        let default = write_key(
            dir.path(),
            "default",
            0o600,
            &default_keys.secret_key().to_secret_hex(),
        );
        let env = env_keys.secret_key().to_secret_hex();
        let explicit = OwnerArgs {
            owner_key_file: Some(file),
        };
        let none = OwnerArgs {
            owner_key_file: None,
        };
        let pubkey = |r: Result<(Keys, String), CliError>| r.unwrap().0.public_key();
        assert_eq!(
            pubkey(resolve_owner(
                &explicit,
                Some(&env),
                true,
                Some(default.clone())
            )),
            file_keys.public_key()
        );
        assert_eq!(
            pubkey(resolve_owner(
                &none,
                Some(&env),
                false,
                Some(default.clone())
            )),
            env_keys.public_key()
        );
        assert_eq!(
            pubkey(resolve_owner(&none, None, false, Some(default.clone()))),
            default_keys.public_key()
        );
    }

    /// A seat's environment carries BEEKEEPER_PRIVATE_KEY and an auth tag; that
    /// signs as the agent and must never be taken for the owner.
    #[test]
    fn a_delegated_environment_is_refused_as_the_owner() {
        let env = Keys::generate().secret_key().to_secret_hex();
        let none = OwnerArgs {
            owner_key_file: None,
        };
        let refused = message(resolve_owner(&none, Some(&env), true, None));
        assert!(refused.contains("DELEGATED_SIGNER_REFUSED"), "{refused}");
        let by_pointer_alone = message(resolve_owner(&none, None, true, None));
        assert!(
            by_pointer_alone.contains("DELEGATED_SIGNER_REFUSED"),
            "{by_pointer_alone}"
        );
        let empty_default = message(resolve_owner(&none, None, false, None));
        assert!(
            empty_default.contains("OWNER_KEY_UNAVAILABLE"),
            "{empty_default}"
        );
    }
}
