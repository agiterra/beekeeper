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

use beekeeper_host::install::{launchd_daemon, Domain, Service};
use beekeeper_host_core::layout::{self, Instance};
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
    /// Register the agent host to start at login (macOS) or as a systemd
    /// user service (Linux) — or, with `--system`, as a macOS LaunchDaemon
    /// that starts at boot with nobody logged in.
    ///
    /// Only the host. The menu bar app is registered by Beekeeper itself,
    /// because it is nested inside that bundle and is meaningless without a
    /// logged-in session — which a server does not have.
    Install {
        /// The host binary to register. Defaults to the `beekeeper-host` beside
        /// this `bee`.
        #[arg(long)]
        program: Option<std::path::PathBuf>,
        /// Install a system LaunchDaemon instead of a login LaunchAgent, for a
        /// Mac nobody logs in to. Needs root and `--user`. macOS only.
        #[arg(long, requires = "user")]
        system: bool,
        /// The account the daemon runs as: the persona whose host this is.
        /// Read from the password database; never defaulted.
        #[arg(long, requires = "system")]
        user: Option<String>,
        /// Print the daemon's plist and exit, writing nothing. Needs no root.
        #[arg(long, requires = "system")]
        print: bool,
        /// The dev instance rather than `BEEKEEPER_HOST_INSTANCE`.
        #[arg(long)]
        dev: bool,
    },
    /// Remove the login registrations and stop the services.
    ///
    /// Both the host and the menu bar app, so an uninstall cannot leave
    /// launchd retrying a binary that is about to be deleted.
    ///
    /// Records that this machine's operator does not want them, so Beekeeper
    /// does not offer to put them back at the next launch — an uninstall that
    /// gets quietly undone reads as the app ignoring you.
    Uninstall {
        /// Remove them without recording a refusal, leaving the machine as if
        /// it had never been asked.
        ///
        /// The difference is "I do not want this" versus "take it off, and
        /// forget I said anything". After `--forget`, Beekeeper's next status
        /// poll finds nothing registered and no refusal, and asks — which is
        /// also the only way to see the first-run prompt again on a machine
        /// that has already answered, without deleting files by hand.
        ///
        /// It does not touch `host.json`, the key file, or the provider's
        /// state directory: the identity and its durable outbox survive, and
        /// so does every session the provider remembers.
        #[arg(long, conflicts_with = "system")]
        forget: bool,
        /// Remove a system LaunchDaemon instead. Needs root and `--user`.
        /// Records no refusal: nobody was asked the login question.
        #[arg(long, requires = "user")]
        system: bool,
        /// The account whose daemon to remove.
        #[arg(long, requires = "system")]
        user: Option<String>,
        /// The dev instance rather than `BEEKEEPER_HOST_INSTANCE`.
        #[arg(long)]
        dev: bool,
    },
    /// Print whether the host and the menu bar app are registered to start at
    /// login.
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

pub async fn dispatch(command: &HostCmd, dev: bool) -> Result<(), CliError> {
    let dev = dev
        || matches!(
            command,
            HostCmd::Install { dev: true, .. } | HostCmd::Uninstall { dev: true, .. }
        );
    let instance = instance(dev)?;

    // System mode first, and before `$HOME` is read: under `sudo` it is root's
    // or the operator's, never the persona's. The daemon reads the persona's
    // home from the password database.
    match command {
        HostCmd::Install {
            program,
            system: true,
            user: Some(user),
            print,
            ..
        } => {
            let program = resolve_host_binary(program.as_deref())?;
            if *print {
                let plist =
                    launchd_daemon::render(user, instance, &program).map_err(CliError::Other)?;
                print!("{plist}");
                return Ok(());
            }
            let registration =
                launchd_daemon::install(user, instance, &program, &launchd_daemon::rerun_command())
                    .map_err(CliError::Other)?;
            print_json(&registration)?;
            for warning in &registration.warnings {
                eprintln!("bee: {warning}");
            }
            eprintln!(
                "bee: installed {} — it starts at boot as {user}. Manage it as {user} with `bee                  host status|start|stop`; restart the process with `sudo launchctl kickstart -k                  system/{}`.",
                registration.path.display(),
                launchd_daemon::label(user, instance)
            );
            return Ok(());
        }
        HostCmd::Uninstall {
            system: true,
            user: Some(user),
            ..
        } => {
            launchd_daemon::uninstall(user, instance, &launchd_daemon::rerun_command())
                .map_err(CliError::Other)?;
            eprintln!(
                "bee: removed the system daemon for {user} ({}). Its identity, key file and                  provider state are untouched.",
                instance.namespace_value()
            );
            return Ok(());
        }
        _ => {}
    }

    let home = layout::home_dir().map_err(CliError::Other)?;

    match command {
        HostCmd::Install { program, .. } => {
            let program = resolve_host_binary(program.as_deref())?;
            let registration =
                beekeeper_host::install::install(Service::AgentHost, &home, instance, &program)
                    .map_err(CliError::Other)?;
            print_json(&registration)?;
            for warning in &registration.warnings {
                eprintln!("bee: {warning}");
            }
            Ok(())
        }
        HostCmd::Uninstall { forget, .. } => {
            // Both, and the menu bar app's failure does not stop the host's.
            let menubar = beekeeper_host::install::uninstall(Service::MenuBar, &home, instance);
            beekeeper_host::install::uninstall(Service::AgentHost, &home, instance)
                .map_err(CliError::Other)?;
            menubar.map_err(CliError::Other)?;
            // `uninstall` records the refusal, which is right for a person
            // saying no. `--forget` drops it again afterwards rather than
            // taking a different route through the removal, so the two paths
            // cannot come apart.
            if *forget {
                beekeeper_host::install::allow_login(&home, instance).map_err(CliError::Other)?;
                eprintln!(
                    "bee: removed the login registrations for {} and forgot the refusal — \
                     Beekeeper will ask about them again. The identity, the key file and the \
                     provider's state directory are untouched.",
                    instance.namespace_value()
                );
            } else {
                eprintln!(
                    "bee: removed the login registrations for {} — recorded, so Beekeeper will \
                     not offer to reinstall them. `bee host install`, `bee host uninstall \
                     --forget` to be asked again, or Settings \u{2192} Coding sessions.",
                    instance.namespace_value()
                );
            }
            Ok(())
        }
        HostCmd::Installed => {
            // Both services, because "is this machine set up" is a question
            // about both and reporting only one would answer it wrongly on a
            // desktop.
            let host = beekeeper_host::install::status(Service::AgentHost, &home, instance);
            let menubar = beekeeper_host::install::status(Service::MenuBar, &home, instance);
            // `installed: false` on its own does not say whether the app will
            // offer to fix it, and that is the question somebody runs this to
            // answer. `declined` and `shouldAsk` look identical without it —
            // and the first means the app will stay quiet on purpose.
            //
            // `provisioned: true` is assumed here rather than read: this
            // command is about the *registration*, and whether a relay has an
            // identity is `bee host status`'s question. So a machine with no
            // identity reads `shouldAsk`, not `notApplicable`.
            let refused = beekeeper_host::install::login_refused(&home, instance);
            let login = beekeeper_host::install::login_autostart(true, &host, refused);
            // And whether the service manager actually has them, because
            // `installed` is "the file exists" and the two come apart: a
            // `bootout` by hand, or replacing the app bundle under a loaded
            // job, leaves both plists on disk with nothing running. Reading
            // `installed: true, login: granted` off this command in that state
            // is how a person concludes their agents will come back when they
            // will not. `null` means the question could not be put.
            let host_loaded = beekeeper_host::install::service_loaded(Service::AgentHost, instance);
            let menubar_loaded =
                beekeeper_host::install::service_loaded(Service::MenuBar, instance);
            print_json(&serde_json::json!({
                "agentHost": host,
                "agentHostLoaded": host_loaded,
                "menuBar": menubar,
                "menuBarLoaded": menubar_loaded,
                "login": login,
            }))?;
            if host_loaded == Some(false) && host.installed {
                match host.domain {
                    Domain::User => eprintln!(
                        "bee: the agent host is registered but not loaded — open Beekeeper, \
                         which repairs this, or run `launchctl bootstrap gui/$UID {}`",
                        host.path.display()
                    ),
                    Domain::System => eprintln!(
                        "bee: the agent host is registered as a system daemon but launchd does \
                         not have it loaded — run `sudo launchctl bootstrap system {}`",
                        host.path.display()
                    ),
                }
            }
            for warning in host.warnings.iter().chain(&menubar.warnings) {
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
                | HostCmd::Uninstall { .. }
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
    let registration = beekeeper_host::install::status(Service::AgentHost, home, instance);
    let label = registration.label().unwrap_or_default();
    // Not `bee host start`: that is a request *to* the host, and nothing is
    // listening to receive it. The service manager is what starts the process.
    CliError::Other(match (registration.installed, registration.domain) {
        (true, Domain::User) => format!(
            "{message} — it is registered to start at login ({}). Start it with `launchctl \
             kickstart gui/$UID/{label}` (or `launchctl bootstrap gui/$UID {}` if it is not \
             loaded), or run `beekeeper-host run` in a terminal to see why it exits",
            registration.path.display(),
            registration.path.display()
        ),
        (true, Domain::System) => format!(
            "{message} — it is registered as a system daemon ({}), so launchd should be running \
             it. Restart it with `sudo launchctl kickstart -k system/{label}`, and read {} for why \
             it exits",
            registration.path.display(),
            layout::host_log_path(home, instance).display()
        ),
        (false, _) => format!(
            "{message} — and it is not registered to start: `bee host install`, or on a Mac \
             nobody logs in to, `sudo bee host install --system --user $USER`"
        ),
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
