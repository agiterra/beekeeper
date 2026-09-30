//! `bee host` — the agent host on this machine.
//!
//! Local-only: this command does **not** talk to the relay. It calls
//! `beekeeper-host`'s owner-only control socket, or writes the login registration
//! that starts it. Both are launcher-local facts — is the host installed, is
//! it running, what is its child doing, what did it log — and none of them can
//! travel over the relay, which authenticates keypairs rather than machines.
//!
//! This is how a headless server is managed. `beekeeper-host` has the same
//! subcommands and is the same code underneath; `bee` carries them because
//! `bee` is the binary that is already on an agent's `PATH`.

use clap::Subcommand;

use crate::error::CliError;

#[derive(Subcommand)]
pub enum HostCmd {
    /// Ask the running host for its status, as JSON.
    ///
    /// Distinguishes a host that is not installed from one that is installed
    /// and stopped from one that is running with no provider — which are three
    /// different problems with three different fixes.
    Status,
    /// Print a bounded tail of the supervised provider's log.
    Logs {
        /// Bytes from the end of the file.
        #[arg(long)]
        bytes: Option<u64>,
    },
    /// Start the provider if it is not running.
    Start,
    /// Stop the provider. The host keeps running and keeps answering.
    Stop,
    /// Stop and start the provider, with a clean restart ladder.
    Restart,
    /// Re-read `host.json` and the identity, then restart onto the result.
    Bind,
    /// Register the host to start at login (macOS) or as a systemd user
    /// service (Linux).
    Install {
        /// The host binary to register. Defaults to the `beekeeper-host` beside
        /// this `bee`.
        #[arg(long)]
        program: Option<std::path::PathBuf>,
    },
    /// Remove the login registration and stop the service.
    Uninstall,
    /// Print whether the host is registered to start at login.
    Installed,
}

/// Which instance to act on: `--dev`, else `BEEKEEPER_HOST_INSTANCE`, else
/// production.
fn instance(dev: bool) -> Result<Instance, CliError> {
    if dev {
        return Ok(Instance::Dev);
    }
    Instance::from_env().map_err(CliError::Usage)
}

use beekeeper_host_core::layout::{self, Instance};

pub async fn dispatch(command: &HostCmd, dev: bool) -> Result<(), CliError> {
    let instance = instance(dev)?;
    let home = layout::home_dir().map_err(CliError::Other)?;

    match command {
        HostCmd::Install { program } => {
            let program = resolve_host_binary(program.as_deref())?;
            let registration = beekeeper_host::install::install(&home, instance, &program)
                .map_err(CliError::Other)?;
            print_json(&registration)?;
            for warning in &registration.warnings {
                eprintln!("bee: {warning}");
            }
            Ok(())
        }
        HostCmd::Uninstall => {
            beekeeper_host::install::uninstall(&home, instance).map_err(CliError::Other)?;
            eprintln!(
                "bee: removed the login registration for {}",
                instance.namespace_value()
            );
            Ok(())
        }
        HostCmd::Installed => {
            let registration = beekeeper_host::install::status(&home, instance);
            print_json(&registration)?;
            for warning in &registration.warnings {
                eprintln!("bee: {warning}");
            }
            Ok(())
        }
        HostCmd::Logs { bytes } => {
            let socket = layout::host_socket_path(&home, instance);
            let logs = beekeeper_host::client::logs(&socket, *bytes)
                .await
                .map_err(|error| host_error(error, &home, instance))?;
            if logs.truncated {
                eprintln!("bee: showing the tail of {}", logs.path.display());
            }
            print!("{}", logs.text);
            Ok(())
        }
        other => {
            let request = match other {
                HostCmd::Status => beekeeper_host::protocol::Request::Status,
                HostCmd::Start => beekeeper_host::protocol::Request::Start,
                HostCmd::Stop => beekeeper_host::protocol::Request::Stop,
                HostCmd::Restart => beekeeper_host::protocol::Request::Restart,
                HostCmd::Bind => beekeeper_host::protocol::Request::Bind,
                HostCmd::Logs { .. }
                | HostCmd::Install { .. }
                | HostCmd::Uninstall
                | HostCmd::Installed => unreachable!("handled above"),
            };
            let socket = layout::host_socket_path(&home, instance);
            let timeout = match other {
                HostCmd::Status => beekeeper_host::client::READ_TIMEOUT,
                _ => beekeeper_host::client::LIFECYCLE_READ_TIMEOUT,
            };
            let value = beekeeper_host::client::call(&socket, &request, timeout)
                .await
                .map_err(|error| host_error(error, &home, instance))?;
            print_json(&value)
        }
    }
}

/// Turn a client error into one an operator can act on.
///
/// "Nothing is listening" alone is not enough: with a login registration
/// present it means *installed but not running*, and without one it means *not
/// installed*. Those need different next steps, so the registration is
/// consulted before the message is written. Telling somebody to start
/// something they have not installed is worse than saying nothing.
fn host_error(
    error: beekeeper_host::client::ClientError,
    home: &std::path::Path,
    instance: Instance,
) -> CliError {
    let message = error.message();
    if !matches!(
        error,
        beekeeper_host::client::ClientError::NotRunning { .. }
    ) {
        return CliError::Other(message);
    }
    let registration = beekeeper_host::install::status(home, instance);
    CliError::Other(if registration.installed {
        format!(
            "{message} — it is registered to start at login ({}), so start it with \
             `bee host start` after logging in, or run `beekeeper-host run` in a terminal to see why \
             it exits",
            registration.path.display()
        )
    } else {
        format!("{message} — and it is not registered to start at login: `bee host install`")
    })
}

/// The `beekeeper-host` this `bee` should register.
///
/// Beside this executable first, because that is the one shipped with this
/// build; then `PATH`. Never a bare name in the registration itself — launchd
/// and systemd run with a minimal `PATH` and neither looks a program up.
fn resolve_host_binary(explicit: Option<&std::path::Path>) -> Result<std::path::PathBuf, CliError> {
    if let Some(explicit) = explicit {
        return std::fs::canonicalize(explicit).map_err(|error| {
            CliError::Usage(format!("cannot resolve {}: {error}", explicit.display()))
        });
    }
    let name = format!("beekeeper-host{}", std::env::consts::EXE_SUFFIX);
    if let Some(beside) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|candidate| candidate.is_file())
    {
        return std::fs::canonicalize(&beside).map_err(|error| {
            CliError::Other(format!("cannot resolve {}: {error}", beside.display()))
        });
    }
    let on_path = std::env::var_os("PATH")
        .map(|value| {
            std::env::split_paths(&value)
                .map(|dir| dir.join(&name))
                .find(|candidate| candidate.is_file())
        })
        .unwrap_or_default();
    match on_path {
        Some(found) => std::fs::canonicalize(&found).map_err(|error| {
            CliError::Other(format!("cannot resolve {}: {error}", found.display()))
        }),
        None => Err(CliError::Usage(format!(
            "no {name} beside this bee or on PATH — build it with \
             `cargo build --release -p beekeeper-host`, or name one with `--program`"
        ))),
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<(), CliError> {
    println!(
        "{}",
        serde_json::to_string_pretty(value)
            .map_err(|error| CliError::Other(format!("failed to render the answer: {error}")))?
    );
    Ok(())
}
