//! Spawning and supervising the single provider child process.
//!
//! One provider runs per desktop instance, for the active relay. It is started
//! at app launch when a record exists, restarted with capped exponential
//! backoff if it exits unexpectedly, and stopped with SIGINT — never SIGKILL
//! first — so its durable outbox flushes before the process dies.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::managed_agents::{
    append_log_marker, known_acp_runtime_exact, open_log_file, resolve_command,
    should_skip_claude_executable,
};
use crate::session_provider::env::{
    build_provider_env, ProviderEnvInputs, INHERITED_KEYS_TO_CLEAR,
};
use crate::session_provider::store::{load_provider_store, CodingSessionProviderRecord};
use crate::session_provider::{provider_log_path, provider_state_dir, trust};
use crate::util::now_iso;

/// Binary name of the provider. Resolved through the same discovery order every
/// other Buzz-spawned binary uses (`resolve_command`), which covers a dev build
/// in `target/{debug,release}` and a bundled sidecar next to the app executable.
pub(crate) const PROVIDER_BINARY: &str = "buzz-session-provider";
/// ACP adapter spawned by the provider for each coding session.
const PROVIDER_AGENT_BINARY: &str = "claude-agent-acp";

/// First restart delay. Doubles per consecutive failure.
pub(crate) const BASE_RESTART_DELAY: Duration = Duration::from_secs(2);
/// Ceiling on a single restart delay.
pub(crate) const MAX_RESTART_DELAY: Duration = Duration::from_secs(60);
/// Restarts allowed inside one [`RESTART_WINDOW`] before the supervisor gives up.
pub(crate) const MAX_RESTARTS_PER_WINDOW: u32 = 5;
/// Sliding window over which restarts are counted.
pub(crate) const RESTART_WINDOW: Duration = Duration::from_secs(600);
/// How long a SIGINT is given to flush the outbox before escalation begins.
pub(crate) const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
/// How long SIGTERM is given after SIGINT, before SIGKILL.
pub(crate) const ESCALATION_TIMEOUT: Duration = Duration::from_secs(2);
/// Poll interval for child exit while supervising.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// What the supervisor should do after the child exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartDecision {
    /// Sleep `delay`, then respawn. `failures` is the new in-window count.
    Retry {
        /// How long to wait before respawning.
        delay: Duration,
        /// Failure count after recording this exit.
        failures: u32,
    },
    /// The child has failed too often, too fast. Stop supervising and leave the
    /// provider down until something explicitly asks for it again.
    GiveUp,
}

/// Decide whether to restart after an unexpected exit.
///
/// Pure — the caller supplies the current in-window failure count and how long
/// ago the window opened, so the policy is testable without clocks or
/// processes.
///
/// A window that has aged out resets the count to one rather than zero: the
/// exit that just happened is itself the first failure of the new window.
pub(crate) fn plan_restart(failures_in_window: u32, window_elapsed: Duration) -> RestartDecision {
    let failures = if window_elapsed >= RESTART_WINDOW {
        1
    } else {
        failures_in_window.saturating_add(1)
    };
    if failures > MAX_RESTARTS_PER_WINDOW {
        return RestartDecision::GiveUp;
    }
    // `failures` is bounded by MAX_RESTARTS_PER_WINDOW, but the saturating
    // shift keeps this correct if that constant ever grows past 32.
    let multiplier = 1u32.checked_shl(failures - 1).unwrap_or(u32::MAX);
    let delay = BASE_RESTART_DELAY
        .checked_mul(multiplier)
        .unwrap_or(MAX_RESTART_DELAY)
        .min(MAX_RESTART_DELAY);
    RestartDecision::Retry { delay, failures }
}

/// Tauri-managed handle to the running supervisor, if any.
#[derive(Default)]
pub struct CodingSessionProviderState {
    /// Serializes start/stop decisions around the separately locked handle.
    /// Without this gate, two concurrent `ensure_running` calls can both see
    /// an empty handle, spawn a child, and then overwrite each other's slot.
    lifecycle: Mutex<()>,
    inner: Mutex<Option<SupervisorHandle>>,
    next_id: AtomicU64,
}

struct SupervisorHandle {
    id: u64,
    provider_pubkey: String,
    stop: Arc<AtomicBool>,
    child_pid: Arc<AtomicU32>,
    /// The ceiling this supervisor's children were started with. Kept so a
    /// settings surface can say what is *in force*, which is not necessarily
    /// what is stored: the child reads its ceiling from the environment once,
    /// at startup.
    max_sessions: Option<usize>,
    /// The silence budget this supervisor's children were started with, for
    /// the same reason as `max_sessions`: stored is not in force.
    turn_idle_timeout_secs: Option<u64>,
}

impl CodingSessionProviderState {
    /// The session ceiling the running provider was started with.
    ///
    /// `None` when nothing is supervised, or when the child was started with
    /// no explicit ceiling and is therefore on the provider's own default.
    pub(crate) fn running_max_sessions(&self) -> Option<usize> {
        self.inner
            .lock()
            .ok()
            .and_then(|handle| handle.as_ref().and_then(|handle| handle.max_sessions))
    }

    /// The per-turn silence budget the running provider was started with.
    pub(crate) fn running_turn_idle_timeout_secs(&self) -> Option<u64> {
        self.inner.lock().ok().and_then(|handle| {
            handle
                .as_ref()
                .and_then(|handle| handle.turn_idle_timeout_secs)
        })
    }

    /// Pubkey of the provider currently being supervised.
    ///
    /// `Some` means the desktop is actively keeping a provider alive — the
    /// child may be momentarily down inside a backoff window, which is a
    /// transient state the supervisor owns, not something a caller should act
    /// on.
    fn supervised_pubkey(&self) -> Option<String> {
        let guard = self.inner.lock().ok()?;
        let handle = guard.as_ref()?;
        if handle.stop.load(Ordering::Acquire) {
            return None;
        }
        Some(handle.provider_pubkey.clone())
    }

    fn install(&self, handle: SupervisorHandle) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some(handle);
        }
    }

    /// Clear the slot, but only if it still holds supervisor `id` — a task that
    /// was superseded must not evict its replacement.
    fn clear_if(&self, id: u64) {
        if let Ok(mut guard) = self.inner.lock() {
            if guard.as_ref().is_some_and(|handle| handle.id == id) {
                *guard = None;
            }
        }
    }

    /// Signal the current supervisor to stop and return its live child pid.
    fn request_stop(&self) -> Option<u32> {
        let guard = self.inner.lock().ok()?;
        let handle = guard.as_ref()?;
        handle.stop.store(true, Ordering::Release);
        let pid = handle.child_pid.load(Ordering::Acquire);
        (pid != 0).then_some(pid)
    }
}

/// Status of the provider host, as reported to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionProviderStatus {
    /// A provider identity exists for this relay.
    pub provisioned: bool,
    /// The desktop is supervising a provider process right now.
    pub running: bool,
    /// Present only when provisioned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_pubkey: Option<String>,
    /// Present only when provisioned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

/// Read the current status for `relay_url`.
pub(crate) fn provider_status(
    app: &AppHandle,
    state: &CodingSessionProviderState,
    relay_url: &str,
) -> Result<CodingSessionProviderStatus, String> {
    let store = load_provider_store(app)?;
    let record = store.get(relay_url);
    let supervised = state.supervised_pubkey();
    let running = match (record, supervised.as_deref()) {
        (Some(record), Some(pubkey)) => record.provider_pubkey == pubkey,
        _ => false,
    };
    Ok(CodingSessionProviderStatus {
        provisioned: record.is_some(),
        running,
        provider_pubkey: record.map(|record| record.provider_pubkey.clone()),
        instance_id: record.map(|record| record.instance_id.clone()),
    })
}

/// Start the provider for `relay_url` if one is provisioned and none is running.
///
/// Idempotent: a second call while the same identity is already supervised is a
/// no-op. Returns `false` when nothing is provisioned — the un-provisioned
/// state is normal, not an error.
pub(crate) fn ensure_running(
    app: &AppHandle,
    state: &CodingSessionProviderState,
    relay_url: &str,
) -> Result<bool, String> {
    let _lifecycle_guard = state
        .lifecycle
        .lock()
        .map_err(|_| "coding-session provider lifecycle lock is poisoned".to_string())?;
    let store = load_provider_store(app)?;
    let Some(record) = store.get(relay_url) else {
        return Ok(false);
    };
    if state.supervised_pubkey().as_deref() == Some(record.provider_pubkey.as_str()) {
        return Ok(true);
    }
    if record.private_key_nsec.is_empty() {
        return Err(format!(
            "coding-session provider {} has no private key available — the OS keyring may be \
             unreachable. Refusing to start without an identity.",
            record.provider_pubkey
        ));
    }
    // Re-assert trust before every start: see the `trust` module docs.
    if let Err(error) = trust::seed_provider_trust(app, &record.provider_pubkey) {
        eprintln!("buzz-desktop: session-provider: failed to seed bridge trust: {error}");
    }
    stop_provider_locked(state);
    start_supervisor(app, state, record.clone(), relay_url.to_string())?;
    Ok(true)
}

/// Spawn the supervision task. Returns once the first child has been spawned so
/// that a failure to launch surfaces to the caller instead of vanishing into a
/// background retry loop.
fn start_supervisor(
    app: &AppHandle,
    state: &CodingSessionProviderState,
    record: CodingSessionProviderRecord,
    relay_url: String,
) -> Result<(), String> {
    let state_dir = provider_state_dir(app, &record.provider_pubkey)?;
    let log_path = provider_log_path(app, &record.provider_pubkey)?;
    // Read here, not per respawn: a supervised child that crashes comes back
    // with the ceiling its supervisor started under, so the number a person
    // sees as "in force" stays true until they restart the provider.
    let stored_settings = load_provider_store(app).ok();
    let max_sessions = stored_settings
        .as_ref()
        .and_then(|store| store.max_sessions);
    let turn_idle_timeout_secs = stored_settings
        .as_ref()
        .and_then(|store| store.turn_idle_timeout_secs);
    let binary = resolve_command(PROVIDER_BINARY).ok_or_else(|| {
        format!(
            "{PROVIDER_BINARY} was not found — build it with \
             `cargo build -p buzz-session-provider`"
        )
    })?;

    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let child_pid = Arc::new(AtomicU32::new(0));

    // The provider holds an exclusive lock on its state directory (one live
    // instance per directory — the same at-most-one-live-instance invariant
    // the managed-agents tier enforces). An orphan from a previous desktop
    // run would make the child we are about to spawn fail that lock, so any
    // stale owner is stopped first. "Kill stale, then start" is the takeover
    // this architecture supports: the supervisor owns the provider strictly
    // as a child process and has no channel to adopt a foreign one.
    take_over_stale_provider(&state_dir, &log_path);

    let mut child = spawn_provider_child(
        &binary,
        &record,
        &relay_url,
        &state_dir,
        &log_path,
        max_sessions,
        turn_idle_timeout_secs,
    )?;
    child_pid.store(child.id(), Ordering::Release);

    state.install(SupervisorHandle {
        id,
        provider_pubkey: record.provider_pubkey.clone(),
        stop: Arc::clone(&stop),
        child_pid: Arc::clone(&child_pid),
        max_sessions,
        turn_idle_timeout_secs,
    });

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut failures = 0u32;
        let mut window_start = std::time::Instant::now();
        loop {
            let status = wait_for_exit(&mut child, &stop, &log_path).await;
            child_pid.store(0, Ordering::Release);
            if stop.load(Ordering::Acquire) {
                break;
            }
            let _ = append_log_marker(
                &log_path,
                &format!(
                    "=== coding-session provider exited ({}) at {} ===",
                    status
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".to_string()),
                    now_iso()
                ),
            );
            let RestartDecision::Retry { delay, failures: n } =
                plan_restart(failures, window_start.elapsed())
            else {
                let _ = append_log_marker(
                    &log_path,
                    "=== coding-session provider restarted too often; supervision stopped ===",
                );
                break;
            };
            if n == 1 {
                window_start = std::time::Instant::now();
            }
            failures = n;
            tokio::time::sleep(delay).await;
            if stop.load(Ordering::Acquire) {
                break;
            }
            match spawn_provider_child(
                &binary,
                &record,
                &relay_url,
                &state_dir,
                &log_path,
                max_sessions,
                turn_idle_timeout_secs,
            ) {
                Ok(next) => {
                    child_pid.store(next.id(), Ordering::Release);
                    child = next;
                }
                Err(error) => {
                    eprintln!("buzz-desktop: session-provider: respawn failed: {error}");
                    break;
                }
            }
        }
        stop.store(true, Ordering::Release);
        if let Some(state) = app.try_state::<CodingSessionProviderState>() {
            state.clear_if(id);
        }
    });
    Ok(())
}

/// Poll the child until it exits or a stop is requested.
///
/// Returns the exit code when the child exited on its own. On a stop request
/// the child is signalled here and reaped, and the caller sees the stop flag
/// rather than treating the exit as a crash.
async fn wait_for_exit(
    child: &mut std::process::Child,
    stop: &AtomicBool,
    log_path: &Path,
) -> Option<i32> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) => {}
            Err(error) => {
                eprintln!("buzz-desktop: session-provider: failed to poll child: {error}");
                return None;
            }
        }
        if stop.load(Ordering::Acquire) {
            let _ = append_log_marker(
                log_path,
                &format!("=== stopping coding-session provider at {} ===", now_iso()),
            );
            terminate_gracefully_async(child).await;
            return None;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// SIGINT, then escalate — without blocking the async executor.
///
/// SIGINT (not SIGTERM) is the provider's documented graceful-shutdown signal:
/// its handler drains the publish outbox, so a stop never loses already-produced
/// transcript events. It goes to the process leader alone so the provider can
/// tear down its own per-session `claude-agent-acp` children in order.
/// Escalation then targets the whole process group, which is what catches
/// adapters the provider failed to reap.
async fn terminate_gracefully_async(child: &mut std::process::Child) {
    let pid = child.id();
    signal_leader(pid, Signal::Interrupt);
    if await_child_exit(child, GRACEFUL_SHUTDOWN_TIMEOUT).await {
        return;
    }
    signal_group(pid, Signal::Terminate);
    if await_child_exit(child, ESCALATION_TIMEOUT).await {
        return;
    }
    signal_group(pid, Signal::Kill);
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

/// Blocking variant for app shutdown, which runs on the main thread outside any
/// async context and must not return before the child is gone.
fn terminate_gracefully_blocking(pid: u32) {
    signal_leader(pid, Signal::Interrupt);
    if await_pid_exit(pid, GRACEFUL_SHUTDOWN_TIMEOUT) {
        return;
    }
    signal_group(pid, Signal::Terminate);
    if await_pid_exit(pid, ESCALATION_TIMEOUT) {
        return;
    }
    signal_group(pid, Signal::Kill);
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

/// The three escalation steps, named so the call sites read as policy.
#[derive(Debug, Clone, Copy)]
enum Signal {
    Interrupt,
    Terminate,
    Kill,
}

#[cfg(unix)]
fn signal_leader(pid: u32, signal: Signal) {
    // SAFETY: `kill` is a plain syscall with no memory effects; an invalid pid
    // returns ESRCH rather than misbehaving.
    unsafe {
        libc::kill(pid as i32, unix_signal(signal));
    }
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: Signal) {
    // SAFETY: see `signal_leader`. A negative pid addresses the process group,
    // which the child was spawned into via `process_group(0)`.
    unsafe {
        libc::kill(-(pid as i32), unix_signal(signal));
    }
}

#[cfg(unix)]
fn unix_signal(signal: Signal) -> i32 {
    match signal {
        Signal::Interrupt => libc::SIGINT,
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    }
}

#[cfg(unix)]
fn pid_is_running(pid: u32) -> bool {
    // SAFETY: signal 0 performs the permission/existence check only.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Windows has no way to deliver SIGINT to a console-less child, so the tree is
/// terminated directly on the first escalation step. The provider's outbox is
/// crash-safe (append-only JSONL), so this costs an orderly flush rather than
/// events.
#[cfg(not(unix))]
fn signal_leader(pid: u32, _signal: Signal) {
    let _ = crate::managed_agents::taskkill_tree(pid);
}

#[cfg(not(unix))]
fn signal_group(pid: u32, _signal: Signal) {
    let _ = crate::managed_agents::taskkill_tree(pid);
}

#[cfg(not(unix))]
fn pid_is_running(_pid: u32) -> bool {
    false
}

/// Lock file the provider holds inside its state directory (see
/// `buzz-session-provider`'s `state::acquire_state_dir_lock`). Its contents
/// are the owner's pid, written while holding the lock.
const PROVIDER_LOCK_FILE: &str = "provider.lock";
/// How long a stale owner's lock is given to clear after it was signalled.
const TAKEOVER_LOCK_WAIT: Duration = Duration::from_secs(5);

/// Stop an orphaned provider that still owns `state_dir`, if there is one.
///
/// A desktop that exited uncleanly leaves its provider running (reparented to
/// launchd) and holding the state-dir lock. Best effort by design: when
/// nothing holds the lock this is a no-op, and when takeover fails the child
/// spawned next fails its own lock with a clear log line and the restart
/// ladder gives up rather than corrupting shared state. The graceful SIGINT
/// first lets the orphan flush its durable outbox before it dies.
fn take_over_stale_provider(state_dir: &Path, log_path: &Path) {
    let lock_path = state_dir.join(PROVIDER_LOCK_FILE);
    let Ok(file) = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&lock_path)
    else {
        return; // No lock file: no provider has ever owned this directory.
    };
    if file.try_lock().is_ok() {
        // Nothing holds it. Release before spawning so the child can lock it.
        let _ = file.unlock();
        return;
    }
    let owner_pid = std::fs::read_to_string(&lock_path)
        .ok()
        .and_then(|contents| parse_lock_owner_pid(&contents));
    let Some(pid) = owner_pid else {
        let _ = append_log_marker(
            log_path,
            "=== a stale provider holds the state-dir lock but recorded no readable pid; \
             the new child will refuse to start until it exits ===",
        );
        return;
    };
    if pid == std::process::id() || !pid_is_running(pid) {
        return;
    }
    let _ = append_log_marker(
        log_path,
        &format!(
            "=== taking over the provider state dir from stale pid {pid} at {} ===",
            now_iso()
        ),
    );
    terminate_gracefully_blocking(pid);
    // The OS drops the lock when the owner dies; wait for that to be visible
    // so the child we spawn next does not lose a takeover race it just won.
    let deadline = std::time::Instant::now() + TAKEOVER_LOCK_WAIT;
    while std::time::Instant::now() < deadline {
        if file.try_lock().is_ok() {
            let _ = file.unlock();
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = append_log_marker(
        log_path,
        &format!("=== stale pid {pid} did not release the state-dir lock in time ==="),
    );
}

/// The owning pid recorded in a provider lock file, if the contents are one.
pub(crate) fn parse_lock_owner_pid(contents: &str) -> Option<u32> {
    let pid = contents.trim().parse::<u32>().ok()?;
    (pid > 1).then_some(pid)
}

/// Build and launch one provider process.
fn spawn_provider_child(
    binary: &Path,
    record: &CodingSessionProviderRecord,
    relay_url: &str,
    state_dir: &Path,
    log_path: &Path,
    max_sessions: Option<usize>,
    turn_idle_timeout_secs: Option<u64>,
) -> Result<std::process::Child, String> {
    let _ = append_log_marker(
        log_path,
        &format!(
            "=== starting coding-session provider {} at {} ===",
            record.provider_pubkey,
            now_iso()
        ),
    );
    let stdout = open_log_file(log_path)?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("failed to clone provider log handle: {error}"))?;

    let mut command = std::process::Command::new(binary);
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::from(stdout));
    command.stderr(std::process::Stdio::from(stderr));
    if let Some(workdir) = crate::managed_agents::default_agent_workdir() {
        command.current_dir(workdir);
    }
    for key in INHERITED_KEYS_TO_CLEAR {
        command.env_remove(key);
    }
    let env = build_provider_env(&ProviderEnvInputs {
        record,
        relay_url,
        state_dir,
        agent_command: resolve_command(PROVIDER_AGENT_BINARY),
        context_mcp_command: resolve_command("buzz-dev-mcp"),
        claude_code_executable: resolve_claude_code_executable(),
        // Read once per spawn from the person's stored preference: the child
        // reads its ceiling from the environment at startup, so a change takes
        // effect the next time the provider starts and never mid-flight.
        max_sessions,
        turn_idle_timeout_secs,
        // Computed per spawn: installing an adapter takes effect on the next
        // provider (re)start, matching the rest of the discovery surface.
        runtimes: crate::session_provider::runtimes::build_runtime_descriptors(),
        // The adapters the provider spawns are `#!/usr/bin/env node` shims;
        // a Finder-launched desktop's GUI PATH has no `node`, so hand down
        // the same augmented PATH managed-agent launches use.
        augmented_path: crate::managed_agents::readiness::cli_probe::augmented_path_with_inherited(
        ),
    });
    for (key, value) in env {
        command.env(key, value);
    }
    // Own process group so escalation can reach per-session adapter children.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .spawn()
        .map_err(|error| format!("failed to spawn {PROVIDER_BINARY}: {error}"))
}

/// Resolve the Claude Code CLI exactly as the managed-agent claude runtime does.
///
/// The provider inherits `CLAUDE_CODE_EXECUTABLE` down into every
/// `claude-agent-acp` it spawns, so resolving it here keeps coding sessions and
/// managed agents on the same binary instead of whatever the adapter's own PATH
/// lookup happens to find.
pub(crate) fn resolve_claude_code_executable() -> Option<PathBuf> {
    let cli = known_acp_runtime_exact("claude")?.underlying_cli?;
    let path = resolve_command(cli)?;
    if should_skip_claude_executable(&path, cfg!(windows)) {
        return None;
    }
    Some(path)
}

/// Stop the supervisor and its child. Safe to call when nothing is running.
pub(crate) fn stop_provider(state: &CodingSessionProviderState) {
    let _lifecycle_guard = state
        .lifecycle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    stop_provider_locked(state);
}

/// Stop while the caller owns `CodingSessionProviderState::lifecycle`.
fn stop_provider_locked(state: &CodingSessionProviderState) {
    if let Some(pid) = state.request_stop() {
        terminate_gracefully_blocking(pid);
    }
}

/// Bind the supervised provider to `relay_url`: start the identity provisioned
/// for it, or stop whatever is running if that relay has none.
///
/// Called once the workspace relay and identity are installed, which is also
/// every community switch. The stop half matters there — a provider left
/// running against the previous community's relay would keep answering commands
/// the user has navigated away from.
///
/// Nothing runs on a desktop that never provisioned a provider; provisioning is
/// the single enablement gesture.
pub(crate) fn start_provider_if_provisioned(app: &AppHandle, relay_url: &str) {
    if relay_url.trim().is_empty() {
        return;
    }
    let Some(state) = app.try_state::<CodingSessionProviderState>() else {
        return;
    };
    match ensure_running(app, &state, relay_url) {
        Ok(true) => {}
        Ok(false) => stop_provider(&state),
        Err(error) => eprintln!("buzz-desktop: session-provider: failed to start: {error}"),
    }
}

/// Synchronous teardown for app shutdown.
///
/// Called from `shutdown::shut_down_app`, which runs on the main thread outside
/// any async context, so this blocks on the signal escalation rather than
/// scheduling it.
pub(crate) fn shutdown_coding_session_provider(app: &AppHandle) {
    if let Some(state) = app.try_state::<CodingSessionProviderState>() {
        stop_provider(&state);
    }
}
