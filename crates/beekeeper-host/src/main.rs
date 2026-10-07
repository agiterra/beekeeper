//! `beekeeper-host` — the headless host for Beekeeper's background agents.
//!
//! The command itself is [`beekeeper_host::cli`], which is Unix-only along with the
//! rest of the crate. This file is the one place that needs a platform branch,
//! and it says so out loud rather than failing to compile: a Windows build
//! that produced no binary at all would make a missing sidecar look like a
//! build-list mistake.

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    beekeeper_core::env_compat::adopt_legacy_env("beekeeper-host");
    beekeeper_host::cli::main()
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "beekeeper-host is only supported on Unix platforms: its control socket is an AF_UNIX \
         socket and its login registrations are launchd and systemd."
    );
    std::process::ExitCode::FAILURE
}
