//! `buzz-shell-host` — a detached PTY host for one Buzz built-in shell session.
//!
//! Spawned by the desktop app (never run by hand); it detaches from the app's
//! session so it keeps the shell alive across app restarts/updates, and serves
//! a Unix socket the app attaches to. See `host::run`.

#[cfg(not(unix))]
fn main() {
    eprintln!("buzz-shell-host is only supported on Unix platforms");
    std::process::exit(1);
}

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use clap::Parser;

#[cfg(unix)]
use buzz_shell_host::host::{self, HostOptions};

#[cfg(unix)]
#[derive(Parser)]
#[command(
    name = "buzz-shell-host",
    about = "Detached PTY host for a Buzz shell session"
)]
struct Cli {
    /// Stable session id (shared with the app and the `shell:<id>` workspace).
    #[arg(long)]
    id: String,
    /// Path to bind the per-session control socket.
    #[arg(long)]
    socket: PathBuf,
    /// Directory for the reattach receipt (`<dir>/<id>.json`).
    #[arg(long)]
    hosts_dir: PathBuf,
    /// Directory for reboot-fallback history (`<dir>/<id>.{json,log}`).
    #[arg(long)]
    persist_dir: PathBuf,
    /// Working directory to launch the shell in.
    #[arg(long)]
    cwd: String,
    /// Shell to run.
    #[arg(long)]
    shell: String,
    /// Display title.
    #[arg(long)]
    title: String,
    /// Initial terminal rows.
    #[arg(long, default_value_t = 24)]
    rows: u16,
    /// Initial terminal columns.
    #[arg(long, default_value_t = 80)]
    cols: u16,
    /// Unix seconds the session was created (preserved across restarts).
    #[arg(long)]
    created_at: u64,
}

#[cfg(unix)]
fn main() {
    let cli = Cli::parse();
    let code = host::run(HostOptions {
        id: cli.id,
        socket_path: cli.socket,
        hosts_dir: cli.hosts_dir,
        persist_dir: cli.persist_dir,
        cwd: cli.cwd,
        shell: cli.shell,
        title: cli.title,
        rows: cli.rows,
        cols: cli.cols,
        created_at: cli.created_at,
    });
    std::process::exit(code);
}
