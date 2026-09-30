//! `buzz-host` — the headless host for Beekeeper's background agents.
//!
//! Runs in the **foreground by default**. That deliberately inverts
//! `buzz-shell-host`, which double-forks unconditionally: a service binary
//! must run in the foreground under systemd's `Type=simple` and in a
//! container, where the runtime's termination signal has to reach it directly.
//!
//! Commissioning happens in the app, not here. This binary reads what the app
//! wrote (`host.json`, and a key on one of three routes) and keeps the
//! provider alive. When it cannot, it says which of those it was missing
//! rather than minting a replacement — a new key would strand every event the
//! real one signed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use buzz_session_host_core::atomic_write::create_dir_all_restricted;
use buzz_session_host_core::config::HostConfig;
use buzz_session_host_core::layout::{self, Instance};
use buzz_session_host_core::record::load_provider_store_from;
use clap::{Parser, Subcommand};

use buzz_host::identity;
use buzz_host::state::ProviderChildState;
use buzz_host::supervisor::{PublishedState, Supervisor};

#[derive(Parser)]
#[command(
    name = "buzz-host",
    about = "Supervise Beekeeper's background agents on this machine",
    long_about = None,
    version
)]
struct Cli {
    /// Which Beekeeper instance to serve. Defaults to `BUZZ_HOST_INSTANCE`,
    /// then production.
    #[arg(long, global = true, value_parser = parse_instance)]
    instance: Option<Instance>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Supervise the provider in the foreground (the default).
    Run,
    /// Print what this host would do, and exit. Reads no secrets aloud.
    Check,
}

fn parse_instance(value: &str) -> Result<Instance, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "production" | "prod" => Ok(Instance::Production),
        "dev" | "development" => Ok(Instance::Dev),
        other => Err(format!("expected \"production\" or \"dev\", not {other:?}")),
    }
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    init_tracing();

    let instance = match cli.instance {
        Some(instance) => instance,
        None => match Instance::from_env() {
            Ok(instance) => instance,
            Err(error) => {
                eprintln!("buzz-host: {error}");
                return std::process::ExitCode::from(2);
            }
        },
    };

    match run(instance, cli.command.unwrap_or(Command::Run)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            // stderr only, not stderr *and* the tracing subscriber: both go to
            // the same place here, and under systemd both land in the journal,
            // so logging it twice just makes the operator read it twice.
            eprintln!("buzz-host: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// `RUST_LOG` if set, else the host's own lifecycle at info and nothing else.
///
/// Quiet on purpose: the provider's log is where a person looks for what the
/// agents did, and this one should carry only what the *host* decided.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,buzz_host=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Everything resolved and checked, before anything is spawned.
struct Commissioned {
    config: HostConfig,
    record: buzz_session_host_core::record::CodingSessionProviderRecord,
    key: identity::ResolvedKey,
}

/// Read the config and the identity, or say exactly what is missing.
///
/// Every failure here is a *disclosed* one: a client asking `status` must be
/// able to tell "no provider is commissioned" from "your key file is
/// unreadable", because they need different words and different actions.
fn commission(home: &std::path::Path, instance: Instance) -> Result<Commissioned, String> {
    let config_path = layout::host_config_path(home, instance);
    let config = HostConfig::load(&config_path)?.ok_or_else(|| {
        format!(
            "this host has not been commissioned: {} does not exist. Open Beekeeper and finish \
             setting up coding sessions, or write the file yourself for a headless install.",
            config_path.display()
        )
    })?;

    let store = load_provider_store_from(&config.record_store_path())?;
    let record = store.get(&config.relay_url).cloned().ok_or_else(|| {
        format!(
            "{} names {} but {} holds no record for that relay",
            config_path.display(),
            config.relay_url,
            config.record_store_path().display()
        )
    })?;
    if record.provider_pubkey != config.provider_pubkey {
        return Err(format!(
            "the record for {} is for a different provider than host.json names — re-commission \
             this host from Beekeeper",
            config.relay_url
        ));
    }

    let key = identity::resolve_key(home, instance, &record).map_err(|error| error.message())?;
    identity::matches_record(&key, &record).map_err(|error| error.message())?;
    Ok(Commissioned {
        config,
        record,
        key,
    })
}

fn run(instance: Instance, command: Command) -> Result<(), String> {
    let home = layout::home_dir()?;
    let host_dir = layout::host_dir(&home, instance);
    create_dir_all_restricted(&host_dir)?;

    let commissioned = commission(&home, instance)?;
    let log_path = commissioned
        .config
        .session_provider_base_dir
        .join("logs")
        .join(format!("{}.log", commissioned.config.provider_pubkey));

    if matches!(command, Command::Check) {
        // Deliberately prints where the key came from, never the key.
        println!("instance:      {}", instance.namespace());
        println!("relay:         {}", commissioned.config.relay_url);
        println!("provider:      {}", commissioned.config.provider_pubkey);
        println!(
            "state dir:     {}",
            commissioned.config.provider_state_dir.display()
        );
        println!("provider log:  {}", log_path.display());
        println!(
            "socket:        {}",
            layout::host_socket_path(&home, instance).display()
        );
        println!("key from:      {:?}", commissioned.key.source);
        println!(
            "provider bin:  {}",
            buzz_host::spawn::resolve_provider_binary(&commissioned.config)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("<unresolved: {error}>"))
        );
        return Ok(());
    }

    if let Some(parent) = log_path.parent() {
        create_dir_all_restricted(parent)?;
    }
    create_dir_all_restricted(&commissioned.config.provider_state_dir)?;

    let published = Arc::new(PublishedState::default());
    let stop = Arc::new(AtomicBool::new(false));
    let socket_path = layout::host_socket_path(&home, instance);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to start the async runtime: {error}"))?;

    runtime.block_on(async {
        let supervisor = Supervisor::new(
            commissioned.config,
            commissioned.record,
            commissioned.key.nsec,
            log_path,
            socket_path,
            Arc::clone(&published),
            Arc::clone(&stop),
        );
        let supervising = tokio::spawn(supervisor.run());

        // SIGTERM as well as Ctrl-C: systemd and launchd both stop a service
        // with SIGTERM, and a host that only handled Ctrl-C would be killed
        // outright — taking the provider's outbox flush with it.
        wait_for_shutdown_signal().await;
        tracing::info!("shutdown requested; stopping the provider");
        stop.store(true, Ordering::Release);
        let _ = supervising.await;
    });

    // Say what state it ended in rather than exiting silently.
    match published.child() {
        ProviderChildState::NotSupervised => tracing::info!("stopped"),
        other => tracing::info!("stopped: {}", other.message()),
    }
    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!("cannot listen for SIGTERM ({error}); Ctrl-C only");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
