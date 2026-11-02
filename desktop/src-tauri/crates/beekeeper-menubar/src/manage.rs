//! The Manage Host window: the only part of this app that opens one.
//!
//! The window loads files embedded in this binary and reaches this module over
//! Tauri's IPC, so nothing listens on a port and there is no origin to defend.
//! It reads through the Host's owner-only socket. It writes by spawning this
//! app's own `bee`, one bounded process group at a time, so the Host stays
//! key-free and this process stays free of signing.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tokio::io::AsyncWriteExt;

use crate::manage_actions::{resolve_bee, Action, Outcome, ACTION_TIMEOUT_SECS};

const WINDOW_LABEL: &str = "manage-host";

/// The process group of the running action, if any. One at a time: two at once
/// would race for the same setup lock and the second would only be refused.
#[derive(Default)]
pub struct ManageState {
    running: Mutex<Option<u32>>,
}

/// Show the window, creating it the first time.
pub fn open<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        WebviewUrl::App(PathBuf::from("manage-host.html")),
    )
    .title("Manage Host")
    .inner_size(880.0, 780.0)
    .min_inner_size(620.0, 480.0)
    .build();
    match built {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(error) => eprintln!("beekeeper-menubar: could not open Manage Host: {error}"),
    }
}

async fn read_inventory(socket: &std::path::Path) -> Outcome {
    match beekeeper_host::client::inventory(socket).await {
        Ok(inventory) => Outcome {
            ok: true,
            value: serde_json::to_value(&inventory).ok(),
            code: None,
            message: None,
        },
        Err(error) => Outcome::from_output(
            false,
            "",
            &beekeeper_host::inventory::describe_client_error(&error),
        ),
    }
}

/// The Host's inventory, or the sentence for why there is none.
#[tauri::command]
pub async fn manage_inventory(socket: tauri::State<'_, HostSocket>) -> Result<Outcome, String> {
    eprintln!("beekeeper-menubar: manage-host read the inventory");
    Ok(read_inventory(&socket.0).await)
}

/// Answer one request on stdout, with no window, through the same code the
/// window's commands use: `--manage-inventory`, or `--manage-action <json>`.
/// Returns whether the arguments asked for that.
pub fn answer_from_arguments(socket: &std::path::Path) -> bool {
    let arguments: Vec<String> = std::env::args().collect();
    let action = arguments
        .iter()
        .position(|argument| argument == "--manage-action")
        .map(|at| arguments.get(at + 1).cloned().unwrap_or_default());
    let inventory = arguments
        .iter()
        .any(|argument| argument == "--manage-inventory");
    if action.is_none() && !inventory {
        return false;
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        eprintln!("beekeeper-menubar: could not start a runtime");
        return true;
    };
    let outcome = runtime.block_on(async {
        match action {
            None => read_inventory(socket).await,
            Some(json) => match serde_json::from_str::<Action>(&json) {
                Ok(action) => run_action(action, &ManageState::default()).await,
                Err(error) => Outcome::refused("INVALID_REQUEST", error.to_string()),
            },
        }
    });
    match serde_json::to_string(&outcome) {
        Ok(text) => println!("{text}"),
        Err(error) => eprintln!("beekeeper-menubar: {error}"),
    }
    true
}

/// The socket path, as Tauri-managed state.
pub struct HostSocket(pub PathBuf);

fn bee_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exists = |path: &std::path::Path| path.is_file();
    resolve_bee(
        &exe,
        std::env::var_os("BEEKEEPER_BEE").map(PathBuf::from),
        exists,
    )
}

/// Run one action as `bee`, in its own process group, under a hard time limit.
#[tauri::command]
pub async fn manage_run(
    action: Action,
    state: tauri::State<'_, ManageState>,
) -> Result<Outcome, String> {
    eprintln!("beekeeper-menubar: manage-host running an action");
    Ok(run_action(action, &state).await)
}

async fn run_action(action: Action, state: &ManageState) -> Outcome {
    let Some(bee) = bee_path() else {
        return Outcome::refused(
            "BEE_NOT_FOUND",
            "This app's own `bee` was not found beside it, so nothing can be changed from here.",
        );
    };
    let invocation = match action.invocation() {
        Ok(invocation) => invocation,
        Err(error) => return Outcome::refused("INVALID_REQUEST", error),
    };
    {
        let Ok(mut running) = state.running.lock() else {
            return Outcome::refused("BEE_REFUSED", "this window's action state is unusable");
        };
        if running.is_some() {
            return Outcome::refused(
                "SETUP_BUSY",
                "Another action from this window is still running.",
            );
        }
        // Reserved before the spawn, replaced by the pid once there is one.
        *running = Some(0);
    }
    let outcome = run_bee(&bee, invocation, state).await;
    if let Ok(mut running) = state.running.lock() {
        *running = None;
    }
    outcome
}

async fn run_bee(
    bee: &std::path::Path,
    invocation: crate::manage_actions::Invocation,
    state: &ManageState,
) -> Outcome {
    let mut command = tokio::process::Command::new(bee);
    command
        .args(&invocation.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    // Never a credential or a pointer to one from this process's environment: the
    // window's key path is the only route, and it is visible to the person.
    for name in crate::manage_actions::scrubbed_env() {
        command.env_remove(name);
    }
    // A login item inherits launchd's PATH, whose git is too old to authenticate
    // to the relay. The same resolution the Host uses for its provider applies.
    // It may run the login shell the first time, so not on this async thread.
    if let Ok(Some(path)) =
        tokio::task::spawn_blocking(beekeeper_host::discovery::augmented_path).await
    {
        command.env("PATH", path);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Outcome::refused("BEE_NOT_FOUND", format!("`bee` did not start: {error}"))
        }
    };
    let pid = child.id();
    if let Ok(mut running) = state.running.lock() {
        *running = pid;
    }
    if let Some(mut stdin) = child.stdin.take() {
        if let Some(text) = invocation.stdin {
            if let Err(error) = stdin.write_all(text.as_bytes()).await {
                return Outcome::refused(
                    "BEE_REFUSED",
                    format!("`bee` did not take its input: {error}"),
                );
            }
        }
    }
    match tokio::time::timeout(
        Duration::from_secs(ACTION_TIMEOUT_SECS),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(output)) => Outcome::from_output(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        ),
        Ok(Err(error)) => {
            Outcome::refused("BEE_REFUSED", format!("`bee` could not be read: {error}"))
        }
        // Dropping the future kills the child; the group goes too.
        Err(_) => {
            if let Some(pid) = pid {
                terminate_group(pid);
            }
            Outcome::refused(
                "ACTION_TIMED_OUT",
                format!("The action did not finish in {ACTION_TIMEOUT_SECS} s and was stopped. Its journal is safe to retry."),
            )
        }
    }
}

/// Stop the running action's process group, leaving its journal to say where it
/// got to. Its effects are not undone; a retry reuses them.
#[tauri::command]
pub fn manage_interrupt(state: tauri::State<'_, ManageState>) -> Result<bool, String> {
    let running = state.running.lock().map_err(|e| e.to_string())?;
    Ok(match *running {
        Some(pid) if pid != 0 => {
            terminate_group(pid);
            true
        }
        _ => false,
    })
}

fn terminate_group(pid: u32) {
    // `kill(1)` rather than a libc dependency for one call. Only ever this app's
    // own child's group, which `process_group(0)` made.
    let _ = std::process::Command::new("/bin/kill")
        .args(["-TERM", &format!("-{pid}")])
        .status();
}
