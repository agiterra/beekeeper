//! The `buzz-host` command line.
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

use std::sync::Arc;

use buzz_session_host_core::atomic_write::create_dir_all_restricted;
use buzz_session_host_core::layout::{self, Instance};
use clap::{Parser, Subcommand};

use crate::commission::commission;
use crate::control::HostControl;
use crate::protocol::Request;

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
    /// Ask the running host for its status, as JSON.
    Status,
    /// Print a bounded tail of the supervised provider's log.
    Logs {
        /// Bytes from the end of the file.
        #[arg(long)]
        bytes: Option<u64>,
    },
    /// Stop the provider. The host keeps running and keeps answering.
    Stop,
    /// Start the provider if it is not running.
    Start,
    /// Stop and start the provider, with a clean restart ladder.
    Restart,
    /// Re-read `host.json` and the identity, then restart onto the result.
    Bind,
    /// Register this host to start at login (macOS) or as a systemd user
    /// service (Linux).
    Install {
        /// The host binary to register. Defaults to this executable.
        #[arg(long)]
        program: Option<std::path::PathBuf>,
    },
    /// Remove the login registration and stop the service.
    Uninstall,
    /// Print whether this host is registered to start at login.
    ///
    /// Distinct from `status`, which asks a *running* host: the two together
    /// are what tell "not installed" from "installed but not running".
    Installed,
}

fn parse_instance(value: &str) -> Result<Instance, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "production" | "prod" => Ok(Instance::Production),
        "dev" | "development" => Ok(Instance::Dev),
        other => Err(format!("expected \"production\" or \"dev\", not {other:?}")),
    }
}

pub fn main() -> std::process::ExitCode {
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

fn run(instance: Instance, command: Command) -> Result<(), String> {
    let home = layout::home_dir()?;
    let socket_path = layout::host_socket_path(&home, instance);

    // The client-side commands need no config and no identity: they ask the
    // running host. Keeping them ahead of `commission` is what makes
    // `buzz-host status` answer "the agent host is not running" instead of
    // "this host has not been commissioned" — two different facts, and the
    // second would be a lie on a machine that is commissioned and simply not
    // running.
    if let Some(request) = client_request(&command) {
        return talk_to_host(&socket_path, request);
    }

    // Registration commands need no config and no identity either: installing
    // the service is what a person does *before* commissioning on a server.
    match &command {
        Command::Install { program } => {
            let program = match program {
                Some(program) => program.clone(),
                None => std::env::current_exe()
                    .map_err(|error| format!("cannot resolve this executable: {error}"))?,
            };
            let program = std::fs::canonicalize(&program)
                .map_err(|error| format!("cannot resolve {}: {error}", program.display()))?;
            let registration = crate::install::install(&home, instance, &program)?;
            print_registration(&registration);
            return Ok(());
        }
        Command::Uninstall => {
            crate::install::uninstall(&home, instance)?;
            println!(
                "removed the login registration for {}",
                instance.namespace()
            );
            return Ok(());
        }
        Command::Installed => {
            print_registration(&crate::install::status(&home, instance));
            return Ok(());
        }
        _ => {}
    }

    let host_dir = layout::host_dir(&home, instance);
    create_dir_all_restricted(&host_dir)?;

    let commissioned = commission(&home, instance)?;
    let log_path = commissioned.log_path();

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
        println!("socket:        {}", socket_path.display());
        println!("key from:      {:?}", commissioned.key.source);
        println!(
            "provider bin:  {}",
            crate::spawn::resolve_provider_binary(&commissioned.config)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("<unresolved: {error}>"))
        );
        return Ok(());
    }

    let provider_state_dir = commissioned.config.provider_state_dir.clone();
    if let Some(parent) = log_path.parent() {
        create_dir_all_restricted(parent)?;
    }
    create_dir_all_restricted(&provider_state_dir)?;

    let control = Arc::new(HostControl::new(
        home,
        instance,
        commissioned,
        socket_path.clone(),
    ));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to start the async runtime: {error}"))?;

    // Bind before supervising, and fail the process if it cannot bind. A host
    // with no control socket is indistinguishable from no host at all —
    // nothing could ask it for its status and nothing could stop it — so
    // starting agents behind an unbindable socket would produce exactly the
    // dishonest state this whole design exists to prevent. Under launchd or
    // systemd the exit is visible; a silent carry-on would not be.
    let listener = runtime.block_on(async { crate::server::bind(&socket_path) })?;

    runtime.block_on(async {
        control.start()?;

        // Served for as long as the host runs, including after the provider
        // has stopped or given up: those are states a client must be able to
        // read and act on.
        let serving = tokio::spawn(crate::server::serve(
            Arc::clone(&control),
            listener,
            socket_path.clone(),
        ));

        wait_for_shutdown_signal().await;
        tracing::info!("shutdown requested; stopping the provider");
        control.stop().await;
        serving.abort();
        let _ = std::fs::remove_file(&socket_path);
        Ok::<(), String>(())
    })?;

    tracing::info!("stopped: {}", control.child_state().message());
    Ok(())
}

/// Print a registration, with its warnings on stderr so a pipe gets only the
/// facts and a person still sees the problems.
fn print_registration(registration: &crate::install::Registration) {
    println!(
        "{}",
        serde_json::to_string_pretty(registration)
            .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"))
    );
    for warning in &registration.warnings {
        eprintln!("buzz-host: {warning}");
    }
}

/// The request a client-side subcommand sends, or `None` for the server ones.
fn client_request(command: &Command) -> Option<Request> {
    match command {
        Command::Status => Some(Request::Status),
        Command::Logs { bytes } => Some(Request::Logs { bytes: *bytes }),
        Command::Stop => Some(Request::Stop),
        Command::Start => Some(Request::Start),
        Command::Restart => Some(Request::Restart),
        Command::Bind => Some(Request::Bind),
        Command::Run
        | Command::Check
        | Command::Install { .. }
        | Command::Uninstall
        | Command::Installed => None,
    }
}

/// Send one request to a running host and print what it said.
fn talk_to_host(socket: &std::path::Path, request: Request) -> Result<(), String> {
    let lifecycle = matches!(request, Request::Stop | Request::Start | Request::Restart);
    let timeout = if lifecycle {
        crate::client::LIFECYCLE_READ_TIMEOUT
    } else {
        crate::client::READ_TIMEOUT
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to start the async runtime: {error}"))?;
    let result = runtime.block_on(crate::client::call(socket, &request, timeout));
    match (result, &request) {
        // A log tail is text a person reads, so it is printed as text. Every
        // other answer is JSON, so it can be piped.
        (Ok(value), Request::Logs { .. }) => {
            let logs: crate::protocol::Logs = serde_json::from_value(value)
                .map_err(|error| format!("the host's answer was not understood: {error}"))?;
            if logs.truncated {
                eprintln!("(showing the tail of {})", logs.path.display());
            }
            print!("{}", logs.text);
            Ok(())
        }
        (Ok(value), _) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value)
                    .map_err(|error| format!("failed to render the answer: {error}"))?
            );
            Ok(())
        }
        (Err(error), _) => Err(error.message()),
    }
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
