//! The local session broker: an owner-only Unix socket the `bee session` CLI
//! calls to act on sessions on behalf of an agent. It is the single enforcement
//! point for agent access — every write is gated on the session's invite
//! roster (a collaborator entry for the calling agent's pubkey) — so an agent
//! can neither reach a session's PTY directly nor bypass the roster.

use std::path::PathBuf;

use serde_json::json;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::shell_sessions::{self, keys, manager as shell_manager};

use super::model::{BrokerSession, BrokerTerminal, SessionActivity};
use super::protocol::{BrokerEnvelope, BrokerRequest, BrokerResponse};

/// Env override for the broker socket path.
const SOCKET_PATH_ENV: &str = "BUZZ_SESSION_BROKER_SOCK";

/// Resolve the broker socket path: `$BUZZ_SESSION_BROKER_SOCK`, else
/// `<state_dir>/session-broker.sock` (`~/.local/state/buzz` in production,
/// `…/buzz-dev` for dev builds — so two instances on one machine don't steal
/// each other's bind). The `bee session` CLI defaults to the production
/// path (`crates/buzz-cli/src/commands/session.rs` — keep in lockstep);
/// reaching a dev instance requires the env override.
pub fn socket_path() -> Result<PathBuf, String> {
    if let Ok(explicit) = std::env::var(SOCKET_PATH_ENV) {
        if !explicit.is_empty() {
            return Ok(PathBuf::from(explicit));
        }
    }
    Ok(crate::shell_sessions::state_dir()?.join("session-broker.sock"))
}

/// Bind the broker socket and serve requests until the process exits. The
/// `AppHandle` lets request handlers surface access prompts to the UI and
/// persist consent decisions.
pub async fn spawn_session_broker(app: tauri::AppHandle) {
    let path = match socket_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("session-broker: {e}");
            return;
        }
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("session-broker: failed to create {}: {e}", parent.display());
            return;
        }
    }
    // A stale socket from a previous run blocks bind; it's owner-only and
    // single-purpose, so removing it is safe.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("session-broker: failed to bind {}: {e}", path.display());
            return;
        }
    };
    restrict_socket_permissions(&path);
    eprintln!("session-broker: listening on {}", path.display());

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let app = app.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, app).await {
                        eprintln!("session-broker: connection error: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("session-broker: accept failed: {e}");
                break;
            }
        }
    }
}

#[cfg(unix)]
fn restrict_socket_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_socket_permissions(_path: &std::path::Path) {}

/// One request per connection: read a line, dispatch, write a response line.
async fn handle_connection(stream: UnixStream, app: tauri::AppHandle) -> Result<(), String> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .await
        .map_err(|e| format!("read: {e}"))?;
    if read == 0 {
        return Ok(());
    }

    let response = match serde_json::from_str::<BrokerEnvelope>(line.trim_end()) {
        Ok(envelope) => dispatch(envelope, &app).await,
        Err(e) => BrokerResponse::err(format!("malformed request: {e}")),
    };

    let mut out = serde_json::to_string(&response).map_err(|e| format!("encode: {e}"))?;
    out.push('\n');
    write_half
        .write_all(out.as_bytes())
        .await
        .map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Whether the calling agent may drive (write into) this session: its pubkey
/// holds a **collaborator** entry on the session's invite roster, or it IS
/// this app's own identity (every built-in shell session belongs to that
/// identity, so the owner always has interact rights). Anything else — no
/// caller, a viewer-role entry, an unparseable pubkey, missing signing keys —
/// fails closed.
///
/// V1 TRUST NOTE: `envelope.caller` is a *self-declared* pubkey on an
/// owner-only (0600) local Unix socket — the socket permission, not the
/// pubkey, is the actual security boundary, so any local process running as
/// the owner can claim any identity. That is the same trust level the old
/// consent toggle had. A later version can bind the claim cryptographically
/// (e.g. a signed envelope); the roster check here is about *which* sessions
/// an honest agent may drive, not about authenticating the socket peer.
fn agent_may_drive(app: &tauri::AppHandle, workspace_id: &str, caller: Option<&str>) -> bool {
    let Some(session_id) = shell_sessions::session_id_from_workspace(workspace_id) else {
        return false;
    };
    let Some(info) = shell_manager::info(session_id) else {
        return false;
    };
    let Some(caller) = caller else {
        return false;
    };
    let caller = caller.trim().to_ascii_lowercase();
    if caller.len() != 64 || !caller.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    if info.roster.iter().any(|entry| {
        entry.pubkey == caller && entry.role == buzz_core_pkg::kind::SHELL_ROLE_COLLABORATOR
    }) {
        return true;
    }
    match app.state::<crate::app_state::AppState>().signing_keys() {
        Ok(keys) => keys.public_key().to_hex() == caller,
        Err(_) => false,
    }
}

async fn dispatch(envelope: BrokerEnvelope, app: &tauri::AppHandle) -> BrokerResponse {
    let caller_id = envelope.caller.as_deref();
    let caller = caller_id.unwrap_or("unknown");
    match envelope.request {
        BrokerRequest::List => {
            eprintln!("session-broker: caller={caller} op=list");
            let sessions = shell_sessions_as_broker_sessions();
            match serde_json::to_value(&sessions) {
                Ok(value) => BrokerResponse::ok(value),
                Err(e) => BrokerResponse::err(format!("encode sessions: {e}")),
            }
        }
        BrokerRequest::Read {
            workspace_id,
            scrollback,
            rendered,
            since,
        } => {
            eprintln!("session-broker: caller={caller} op=read workspace={workspace_id}");
            let Some(session_id) = shell_sessions::session_id_from_workspace(&workspace_id) else {
                return only_shell_sessions();
            };
            shell_read(session_id, rendered, scrollback, since)
        }
        BrokerRequest::Send { workspace_id, text } => {
            eprintln!("session-broker: caller={caller} op=send workspace={workspace_id}");
            if !agent_may_drive(app, &workspace_id, caller_id) {
                return not_permitted(&workspace_id);
            }
            let Some(session_id) = shell_sessions::session_id_from_workspace(&workspace_id) else {
                return only_shell_sessions();
            };
            shell_send_text(session_id, &text)
        }
        BrokerRequest::SendKey { workspace_id, key } => {
            eprintln!(
                "session-broker: caller={caller} op=send_key workspace={workspace_id} key={key}"
            );
            if !agent_may_drive(app, &workspace_id, caller_id) {
                return not_permitted(&workspace_id);
            }
            let Some(session_id) = shell_sessions::session_id_from_workspace(&workspace_id) else {
                return only_shell_sessions();
            };
            shell_send_key(session_id, &key)
        }
        BrokerRequest::Exec {
            workspace_id,
            command,
            quiet_ms,
            timeout_ms,
        } => {
            eprintln!("session-broker: caller={caller} op=exec workspace={workspace_id}");
            let Some(session_id) = shell_sessions::session_id_from_workspace(&workspace_id) else {
                return BrokerResponse::err(
                    "exec is only supported for built-in shell sessions".to_string(),
                );
            };
            if agent_may_drive(app, &workspace_id, caller_id) {
                shell_exec(session_id, &command, quiet_ms, timeout_ms).await
            } else {
                // Not on the roster: prompt the owner for *this* command. They
                // can allow it once (runs it, roster unchanged, so the next
                // command prompts again) or enable full control (adds the
                // agent to the session roster as collaborator and runs it).
                // This makes exec self-prompting rather than failing.
                request_access(
                    app,
                    &workspace_id,
                    Some(command),
                    None,
                    envelope.caller.clone(),
                )
                .await
            }
        }
        BrokerRequest::RequestAccess {
            workspace_id,
            command,
            reason,
        } => {
            eprintln!("session-broker: caller={caller} op=request_access workspace={workspace_id}");
            request_access(app, &workspace_id, command, reason, envelope.caller.clone()).await
        }
    }
}

/// Built-in shell sessions projected into the session shape agents consume
/// from `list` — `shell:`-prefixed workspace ids.
fn shell_sessions_as_broker_sessions() -> Vec<BrokerSession> {
    shell_manager::list()
        .into_iter()
        .map(|info| {
            let workspace_id = shell_sessions::workspace_id(&info.session_id);
            // "Agents enabled" now projects the invite roster: any collaborator
            // entry means some identity has standing write access. Which
            // *specific* caller may drive is decided per-request in
            // `agent_may_drive`; this flag is the up-front hint `list` gives
            // callers so they don't discover access by a refused send.
            let agents_enabled = info
                .roster
                .iter()
                .any(|entry| entry.role == buzz_core_pkg::kind::SHELL_ROLE_COLLABORATOR);
            BrokerSession {
                workspace_id: workspace_id.clone(),
                window_id: None,
                title: Some(info.title.clone()),
                current_directory: Some(info.current_directory.clone()),
                status_line: (!info.running).then(|| "Shell exited".to_string()),
                status_line_at: None,
                activity: SessionActivity::Active,
                is_selected: false,
                has_unread: false,
                is_pinned: false,
                last_activity_at: Some(info.created_at as f64),
                terminals: vec![BrokerTerminal {
                    surface_id: workspace_id,
                    title: Some(info.title),
                    current_directory: Some(info.current_directory),
                    is_focused: true,
                    is_ready: info.running,
                }],
                agents_enabled,
                input_line: shell_manager::input_line(&info.session_id),
            }
        })
        .collect()
}

/// Fulfill a broker read against a built-in shell session.
fn shell_read(
    session_id: &str,
    rendered: bool,
    scrollback: bool,
    since: Option<u64>,
) -> BrokerResponse {
    match shell_manager::read(session_id, rendered, scrollback, since) {
        Ok(read) => match serde_json::to_value(&read) {
            Ok(value) => BrokerResponse::ok(value),
            Err(e) => BrokerResponse::err(format!("encode read: {e}")),
        },
        Err(e) => BrokerResponse::err(e),
    }
}

/// Type text into a shell session then press Enter.
fn shell_send_text(session_id: &str, text: &str) -> BrokerResponse {
    let result = shell_manager::write(session_id, text.as_bytes())
        .and_then(|()| shell_manager::write(session_id, b"\r"));
    delivered_or_err(result)
}

/// Send a single named key into a shell session.
fn shell_send_key(session_id: &str, key: &str) -> BrokerResponse {
    let result = match keys::key_to_bytes(key) {
        Some(bytes) => shell_manager::write(session_id, &bytes),
        None => Err(format!("unsupported key: {key}")),
    };
    delivered_or_err(result)
}

fn delivered_or_err(result: Result<(), String>) -> BrokerResponse {
    match result {
        Ok(()) => BrokerResponse::ok(json!({ "delivered": true })),
        Err(e) => BrokerResponse::err(e),
    }
}

/// Run a command in a shell session and return the new output once it settles.
/// Types the command + Enter, then polls until output has been quiet for
/// `quiet_ms` (default 600) or `timeout_ms` elapses (default 15000), and
/// returns everything printed since — the collapsed send/poll/read round-trip.
async fn shell_exec(
    session_id: &str,
    command: &str,
    quiet_ms: Option<u64>,
    timeout_ms: Option<u64>,
) -> BrokerResponse {
    let quiet = std::time::Duration::from_millis(quiet_ms.unwrap_or(600).max(50));
    let timeout =
        std::time::Duration::from_millis(timeout_ms.unwrap_or(15_000).max(quiet_ms.unwrap_or(600)));

    let start = match shell_manager::cursor(session_id) {
        Ok(c) => c,
        Err(e) => return BrokerResponse::err(e),
    };
    if let Err(e) = shell_manager::write(session_id, command.as_bytes())
        .and_then(|()| shell_manager::write(session_id, b"\r"))
    {
        return BrokerResponse::err(e);
    }

    let deadline = std::time::Instant::now() + timeout;
    let mut timed_out = true;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        match (
            shell_manager::idle_for(session_id),
            shell_manager::cursor(session_id),
        ) {
            (Ok(Some(idle)), Ok(cur)) if idle >= quiet && cur > start => {
                timed_out = false;
                break;
            }
            (Err(e), _) | (_, Err(e)) => return BrokerResponse::err(e),
            _ => {}
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
    }

    match shell_manager::read(session_id, false, false, Some(start)) {
        Ok(read) => BrokerResponse::ok(json!({
            "output": read.text,
            "cursor": read.cursor,
            "truncated": read.truncated,
            "inputLine": read.input_line,
            "timedOut": timed_out,
        })),
        Err(e) => BrokerResponse::err(e),
    }
}

/// Ask the owner, in chat, for access to a session the caller is not a
/// collaborator on. Blocks until the owner answers (or a fixed timeout). On
/// "once" the named command runs a single time; on "full" the calling
/// agent's pubkey is added to the session's invite roster as collaborator
/// (persisted + announced) and the command, if any, runs; "deny" returns
/// `approved: false`.
async fn request_access(
    app: &tauri::AppHandle,
    workspace_id: &str,
    command: Option<String>,
    reason: Option<String>,
    caller: Option<String>,
) -> BrokerResponse {
    // Access requests only make sense for the built-in shell today (it owns
    // the exec path the approval unblocks).
    let Some(session_id) = shell_sessions::session_id_from_workspace(workspace_id) else {
        return BrokerResponse::err(
            "access requests are only supported for built-in shell sessions".to_string(),
        );
    };
    // Verify the session exists before prompting, so a stale/bogus id can't pop
    // a phantom approval dialog and block for the timeout.
    let Some(session_info) = shell_manager::info(session_id) else {
        return BrokerResponse::err(format!("shell session {session_id} not found"));
    };
    let session_title = Some(session_info.title);

    // Derive a request id that is unique across concurrent asks without needing
    // a clock: workspace + caller + the current output cursor.
    let salt = shell_manager::cursor(session_id).unwrap_or(0);
    let id = format!("{workspace_id}:{}:{salt}", caller.as_deref().unwrap_or("?"));
    // Keep the caller pubkey around: Decision::Full below grants it a
    // collaborator roster entry.
    let request_caller = caller.clone();
    let request = shell_sessions::access::AccessRequest {
        id: id.clone(),
        workspace_id: workspace_id.to_string(),
        session_title,
        command: command.clone(),
        reason,
        caller,
    };
    let rx = match shell_sessions::access::register(app, request) {
        Ok(rx) => rx,
        Err(e) => return BrokerResponse::err(e),
    };

    let decision = match tokio::time::timeout(std::time::Duration::from_secs(180), rx).await {
        Ok(Ok(decision)) => decision,
        // Sender dropped or timed out: withdraw and report.
        _ => {
            shell_sessions::access::cancel(app, &id);
            return BrokerResponse::err(
                "the owner did not respond to the access request in time".to_string(),
            );
        }
    };

    use shell_sessions::access::Decision;
    match decision {
        Decision::Deny => BrokerResponse::ok(json!({ "approved": false })),
        Decision::Full => {
            // Full control = a persisted collaborator entry on the session's
            // invite roster, which requires the agent to have identified
            // itself with a pubkey (the CLI sends one when BUZZ_PRIVATE_KEY
            // is set). Without one there is nothing durable to grant.
            let Some(agent_pubkey) = request_caller
                .as_deref()
                .map(str::trim)
                .filter(|c| c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                return BrokerResponse::err(
                    "cannot enable full control: the agent did not identify itself with a \
                     pubkey (run the CLI with BUZZ_PRIVATE_KEY set), so it cannot be added \
                     to the session's roster"
                        .to_string(),
                );
            };
            if let Err(e) = shell_manager::add_roster_entry(
                app,
                session_id,
                agent_pubkey,
                buzz_core_pkg::kind::SHELL_ROLE_COLLABORATOR,
            ) {
                return BrokerResponse::err(e);
            }
            let output = match &command {
                Some(cmd) => match shell_exec(session_id, cmd, None, None).await.result {
                    Some(v) => v.get("output").and_then(|o| o.as_str()).map(str::to_string),
                    None => None,
                },
                None => None,
            };
            BrokerResponse::ok(json!({ "approved": true, "mode": "full", "output": output }))
        }
        Decision::Once => {
            let Some(cmd) = command else {
                return BrokerResponse::ok(json!({ "approved": true, "mode": "once" }));
            };
            match shell_exec(session_id, &cmd, None, None).await.result {
                Some(v) => BrokerResponse::ok(json!({
                    "approved": true,
                    "mode": "once",
                    "output": v.get("output").and_then(|o| o.as_str()).unwrap_or_default(),
                })),
                None => BrokerResponse::ok(json!({ "approved": true, "mode": "once" })),
            }
        }
    }
}

fn not_permitted(workspace_id: &str) -> BrokerResponse {
    BrokerResponse::err(format!(
        "not permitted: you are not a collaborator on session {workspace_id}. \
         Ask the owner with `bee session request-access {workspace_id}` \
         (optionally --command to run one command), or have them invite your \
         pubkey as a collaborator from the session's screen."
    ))
}

fn only_shell_sessions() -> BrokerResponse {
    BrokerResponse::err("only built-in shell sessions (shell:<id>) are supported".to_string())
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    async fn call(sock: &str, request: serde_json::Value) -> serde_json::Value {
        let stream = UnixStream::connect(sock).await.expect("connect broker");
        let (read_half, mut write_half) = stream.into_split();
        let mut line =
            serde_json::to_string(&json!({ "caller": "test", "request": request })).unwrap();
        line.push('\n');
        write_half.write_all(line.as_bytes()).await.unwrap();
        let mut reader = BufReader::new(read_half);
        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        serde_json::from_str(response.trim_end()).unwrap()
    }

    /// End-to-end broker check against a **live** desktop app with at least
    /// one built-in shell session open. Ignored by default (CI has no app);
    /// run with the app running:
    /// `cargo test --manifest-path desktop/src-tauri/Cargo.toml
    /// session_broker::server::live -- --ignored --nocapture`. Read-only: it
    /// lists sessions and asserts a write is refused by default — it never
    /// types into a session (the test caller is on no session's roster).
    #[tokio::test]
    #[ignore]
    async fn live_broker_lists_and_gates_writes() {
        // Target the running desktop app's broker socket rather than spawning
        // one here — spawn_session_broker requires an AppHandle.
        let sock = &super::socket_path()
            .expect("broker socket path")
            .to_string_lossy()
            .into_owned();
        for _ in 0..60 {
            if std::path::Path::new(sock).exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        let list = call(sock, json!({ "op": "list" })).await;
        assert_eq!(list["ok"], true, "list failed: {list}");
        let sessions = list["result"].as_array().expect("sessions array");
        assert!(!sessions.is_empty(), "expected live shell sessions");
        eprintln!("broker listed {} sessions", sessions.len());
        let ws = sessions[0]["workspaceId"]
            .as_str()
            .expect("workspaceId")
            .to_string();

        // Default-off agent consent: a write must be refused.
        let send = call(
            sock,
            json!({ "op": "send", "workspace_id": ws, "text": "noop" }),
        )
        .await;
        assert_eq!(
            send["ok"], false,
            "send should be refused by default: {send}"
        );
        assert!(
            send["error"]
                .as_str()
                .unwrap_or_default()
                .contains("not permitted"),
            "unexpected error: {send}"
        );
        eprintln!("consent gate correctly refused a non-consented send");
    }
}
