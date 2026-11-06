//! `beekeeper-host` — the headless host for Beekeeper's background agents.
//!
//! Foreground Host entry point on Unix and native Windows.

#[cfg(any(unix, windows))]
fn main() -> std::process::ExitCode {
    beekeeper_core::env_compat::adopt_legacy_env("beekeeper-host");
    beekeeper_host::cli::main()
}

#[cfg(not(any(unix, windows)))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "beekeeper-host is only supported on Unix platforms: its control socket is an AF_UNIX \
         socket and its login registrations are launchd and systemd."
    );
    std::process::ExitCode::FAILURE
}
