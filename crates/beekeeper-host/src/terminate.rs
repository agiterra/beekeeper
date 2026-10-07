//! Stopping the provider without losing what it already produced.
//!
//! SIGINT — not SIGTERM — is the provider's documented graceful-shutdown
//! signal: its handler drains the publish outbox, so a stop never loses
//! already-produced transcript events. It goes to the process leader alone so
//! the provider can tear down its own per-session `claude-agent-acp` children
//! in order. Escalation then targets the whole process group, which is what
//! catches adapters the provider failed to reap.
//!
//! No `unsafe`: `nix`'s safe wrappers, as every other root-workspace crate
//! that sends a signal uses (`beekeeper-session-provider/src/session.rs`,
//! `beekeeper-acp`).

use std::time::Duration;

/// How long a SIGINT is given to flush the outbox before escalation begins.
pub const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
/// How long SIGTERM is given after SIGINT, before SIGKILL.
pub const ESCALATION_TIMEOUT: Duration = Duration::from_secs(2);

/// The three escalation steps, named so the call sites read as policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escalation {
    Interrupt,
    Terminate,
    Kill,
}

#[cfg(unix)]
fn nix_signal(escalation: Escalation) -> nix::sys::signal::Signal {
    use nix::sys::signal::Signal;
    match escalation {
        Escalation::Interrupt => Signal::SIGINT,
        Escalation::Terminate => Signal::SIGTERM,
        Escalation::Kill => Signal::SIGKILL,
    }
}

#[cfg(unix)]
fn as_pid(pid: u32) -> nix::unistd::Pid {
    nix::unistd::Pid::from_raw(i32::try_from(pid).unwrap_or(i32::MAX))
}

/// Signal the process leader only.
#[cfg(unix)]
pub fn signal_leader(pid: u32, escalation: Escalation) {
    let _ = nix::sys::signal::kill(as_pid(pid), nix_signal(escalation));
}

/// Signal the child's whole process group.
#[cfg(unix)]
pub fn signal_group(pid: u32, escalation: Escalation) {
    // The child is spawned into its own group via `process_group(0)`, so its
    // pid is its pgid.
    let _ = nix::sys::signal::killpg(as_pid(pid), nix_signal(escalation));
}

/// Whether a process with `pid` is still alive.
///
/// Signal `None` performs the permission/existence check only, and **`EPERM`
/// counts as alive**: it means the process exists but this uid may not signal
/// it. Reading `EPERM` as "gone" is how a takeover check decides a live owner
/// is dead and starts a second provider against the same state directory.
/// Only `ESRCH` — no such process — is absence.
#[cfg(unix)]
pub fn pid_is_running(pid: u32) -> bool {
    match nix::sys::signal::kill(as_pid(pid), None) {
        Ok(()) => true,
        Err(nix::errno::Errno::EPERM) => true,
        Err(_) => false,
    }
}

/// Windows has no way to deliver SIGINT to a console-less child, so the tree
/// is terminated directly on the first escalation step. The provider's outbox
/// is crash-safe (append-only JSONL), so this costs an orderly flush rather
/// than events.
#[cfg(not(unix))]
pub fn signal_leader(pid: u32, _escalation: Escalation) {
    taskkill_tree(pid);
}

#[cfg(not(unix))]
pub fn signal_group(pid: u32, _escalation: Escalation) {
    taskkill_tree(pid);
}

#[cfg(not(unix))]
pub fn pid_is_running(_pid: u32) -> bool {
    false
}

#[cfg(not(unix))]
fn taskkill_tree(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// SIGINT, then escalate — without blocking the async executor.
pub async fn terminate_gracefully_async(child: &mut std::process::Child) {
    let pid = child.id();
    signal_leader(pid, Escalation::Interrupt);
    if await_child_exit(child, GRACEFUL_SHUTDOWN_TIMEOUT).await {
        return;
    }
    signal_group(pid, Escalation::Terminate);
    if await_child_exit(child, ESCALATION_TIMEOUT).await {
        return;
    }
    signal_group(pid, Escalation::Kill);
    let _ = child.wait();
}

async fn await_child_exit(child: &mut std::process::Child, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Blocking variant for a pid this process does not own as a child — the
/// stale-provider takeover, and the host's own shutdown path.
pub fn terminate_gracefully_blocking(pid: u32) {
    signal_leader(pid, Escalation::Interrupt);
    if await_pid_exit(pid, GRACEFUL_SHUTDOWN_TIMEOUT) {
        return;
    }
    signal_group(pid, Escalation::Terminate);
    if await_pid_exit(pid, ESCALATION_TIMEOUT) {
        return;
    }
    signal_group(pid, Escalation::Kill);
}

fn await_pid_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !pid_is_running(pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The escalation order is the contract: a SIGKILL that arrives first
    /// costs the outbox flush, which is transcript events a person already
    /// saw being produced.
    #[test]
    fn the_graceful_signal_comes_first_and_gets_the_longer_budget() {
        assert!(GRACEFUL_SHUTDOWN_TIMEOUT > ESCALATION_TIMEOUT);
    }

    /// `EPERM` is the case that matters: pid 1 exists and an ordinary user may
    /// not signal it. A liveness check that read that as "gone" would let a
    /// takeover start a second provider against a live one's state directory.
    #[cfg(unix)]
    #[test]
    fn a_process_we_may_not_signal_still_reads_as_running() {
        assert!(pid_is_running(std::process::id()));
        assert!(pid_is_running(1), "pid 1 is alive, EPERM or not");
        assert!(!pid_is_running(u32::MAX - 1), "ESRCH is absence");
    }
}
