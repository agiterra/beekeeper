//! A log file for the desktop app.
//!
//! Until this existed the app installed no `tracing` subscriber at all, so its
//! own instrumentation — the provider supervisor, deep links, managed-agent
//! discovery — went nowhere.
//!
//! **Scope, because this was initially claimed too broadly.** The ACP and
//! session-provider stacks do *not* log through here: `buzz-session-provider`
//! runs as a supervised child process, installs its own subscriber
//! (`buzz-session-provider/src/lib.rs:147`), and has its stdout and stderr
//! redirected by `supervisor.rs` to
//! `<app data>/session-provider/logs/<pubkey>.log`. That file is where
//! `acp::stall`, `acp::stderr`, `acp::sdk_frame` and every `csp::` line land,
//! and it already existed. This module covers the desktop process only.
//!
//! What is written here can still be host-private, so it stays local: not
//! uploaded, not attached to a crash report, not folded into anything that
//! leaves the machine.

use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

/// Keeps the non-blocking writer's worker thread alive.
///
/// `tracing_appender`'s guard flushes and joins on drop; dropping it at the end
/// of `install` would silently stop the file from being written after the first
/// buffered batch.
static GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Default verbosity when `RUST_LOG` says nothing.
///
/// The `acp::`/`csp::` targets are named even though those crates log in the
/// provider child rather than here: the desktop links them for types and
/// constants, an in-process user could appear, and a filter that silently
/// dropped them would be a trap. `acp::wire` is deliberately absent — it logs
/// every line in both directions and would bury everything else.
const DEFAULT_FILTER: &str = "info,csp=debug,acp::stall=debug,acp::stderr=debug";

/// Where the log file goes, per platform.
///
/// Returns `None` rather than guessing when the home directory cannot be
/// resolved — a log written somewhere unexpected is worse than none, because
/// nobody looks for it and it still accumulates.
pub fn log_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(dirs::home_dir()?.join("Library/Logs/io.agiterra.beekeeper.app"))
    }
    #[cfg(target_os = "windows")]
    {
        Some(dirs::data_local_dir()?.join("io.agiterra.beekeeper.app/logs"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Some(
            dirs::state_dir()
                .unwrap_or(dirs::home_dir()?.join(".local/state"))
                .join("beekeeper/logs"),
        )
    }
}

/// Open the rolling file the subscriber writes into.
///
/// Daily rotation with a bounded backlog: unbounded log files on a machine
/// nobody is watching is how a debugging aid becomes a disk-space incident.
///
/// Split out from [`install`] so the file-writing half can be tested without
/// claiming the process-wide subscriber, which only one test in a binary could
/// ever do.
fn open_appender(
    dir: &std::path::Path,
) -> Result<tracing_appender::rolling::RollingFileAppender, String> {
    tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("beekeeper")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .map_err(|error| error.to_string())
}

/// Install the process-wide subscriber. Idempotent and never fatal.
///
/// A failure here must not stop the app from starting: the log is a
/// convenience for diagnosing a stalled session, and refusing to launch
/// because a directory was not writable would trade a debugging aid for an
/// outage. On failure the app runs exactly as it did before this module
/// existed, and says so on stderr.
pub fn install() {
    if GUARD.get().is_some() {
        return;
    }
    let Some(dir) = log_dir() else {
        eprintln!("beekeeper: no home directory; logging to file is disabled");
        return;
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("beekeeper: cannot create {}: {error}", dir.display());
        return;
    }

    let appender = match open_appender(&dir) {
        Ok(appender) => appender,
        Err(error) => {
            eprintln!(
                "beekeeper: cannot open a log file in {}: {error}",
                dir.display()
            );
            return;
        }
    };

    let (writer, guard) = tracing_appender::non_blocking(appender);
    if GUARD.set(guard).is_err() {
        // Another thread won the race and its subscriber is already installed.
        return;
    }

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    let installed = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_target(true),
        )
        .with(filter)
        .try_init();

    // An error means a subscriber was already set — in tests, or by an
    // embedder. Not a failure, and not ours to override.
    if installed.is_ok() {
        tracing::info!(
            target: "desktop",
            directory = %dir.display(),
            "log file opened"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The directory has to be somewhere a person would think to look, and
    /// somewhere per-user rather than shared.
    #[test]
    fn the_log_directory_is_under_the_users_own_home() {
        let dir = log_dir().expect("a home directory in test");
        let home = dirs::home_dir().expect("home");
        #[cfg(target_os = "macos")]
        assert!(
            dir.starts_with(home.join("Library/Logs")),
            "macOS logs belong in ~/Library/Logs, got {}",
            dir.display()
        );
        #[cfg(not(target_os = "macos"))]
        let _ = home;
        assert!(
            dir.to_string_lossy().contains("beekeeper"),
            "the directory must name the app: {}",
            dir.display()
        );
    }

    /// `acp::wire` logs every line in both directions. Leaving it on by
    /// default would bury the handful of lines that actually diagnose a stall
    /// and turn a debugging aid into a disk-space problem.
    #[test]
    fn the_default_filter_keeps_the_diagnosis_and_drops_the_firehose() {
        assert!(DEFAULT_FILTER.contains("acp::stall"));
        assert!(DEFAULT_FILTER.contains("acp::stderr"));
        assert!(
            !DEFAULT_FILTER.contains("acp::wire"),
            "the per-line wire log must stay opt-in via RUST_LOG"
        );
    }

    /// Installing twice must not panic or replace a live subscriber — `run()`
    /// calls it, and a test binary or embedder may have got there first.
    #[test]
    fn installing_twice_is_harmless() {
        install();
        install();
    }

    /// The point of the whole module is that a line emitted by the embedded
    /// stacks ends up in a file somebody can read. Asserting the subscriber
    /// was constructed would not have caught a writer that silently discards,
    /// which is the bug this module exists to fix.
    #[test]
    fn a_logged_line_reaches_a_file_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let appender = open_appender(dir.path()).expect("appender");
        let (writer, guard) = tracing_appender::non_blocking(appender);

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_target(true),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(target: "acp::stall", "answer stall — last agent_message_chunk at +52.1s");
        });
        // Flushes and joins the worker thread, so the read below is not a race.
        drop(guard);

        let written = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(Result::ok)
            .map(|entry| std::fs::read_to_string(entry.path()).unwrap_or_default())
            .collect::<String>();
        assert!(
            written.contains("last agent_message_chunk at +52.1s"),
            "the diagnosis must reach the file; got {written:?}"
        );
        assert!(
            written.contains("acp::stall"),
            "the target must be recorded so a reader can filter: {written:?}"
        );
    }
}
