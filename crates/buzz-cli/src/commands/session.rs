//! `buzz session` — agent-facing access to interactive sessions.
//!
//! Local-only: this command does **not** talk to the relay. It calls the desktop
//! app's owner-only session broker (a Unix socket), which is the single
//! authority — it enforces per-session **agent** consent. So an agent can
//! "check on the build session" (`read`) or "advance it" (`send`) by name, but
//! only for sessions the owner has allowed agents to drive.
//!
//! The `session` name and this protocol are deliberately backend-agnostic:
//! built-in shells fulfill requests today; another session backend could be
//! added without changing this surface.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::Subcommand;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::CliError;

#[derive(Subcommand)]
pub enum SessionCmd {
    /// List the interactive sessions running on this machine. Each entry
    /// includes `agentsEnabled` (whether writes are permitted) and, for
    /// built-in shells, the current `inputLine`.
    List,
    /// Print a session's on-screen contents. Defaults to the plain
    /// ANSI-stripped stream (real line breaks, parseable). Use `--rendered`
    /// for the terminal-emulated screen, or `--since <cursor>` for only the
    /// output after a prior read's cursor.
    Read {
        /// Session name (title), directory basename, or id.
        target: String,
        /// Include scrollback (history above the visible screen).
        #[arg(long)]
        scrollback: bool,
        /// Return the terminal-emulated screen instead of the plain stream.
        #[arg(long)]
        rendered: bool,
        /// Return only output produced after this cursor (built-in shell).
        #[arg(long)]
        since: Option<u64>,
        /// Print the full read object (text + cursor + inputLine) as JSON,
        /// not just the text. Required to capture the cursor for `--since`.
        #[arg(long)]
        json: bool,
    },
    /// Type a line of text into a session, then Enter. Requires the session's
    /// "Agents" consent to be enabled by its owner.
    Send {
        /// Session name (title), directory basename, or id.
        target: String,
        /// The text to type.
        text: String,
    },
    /// Send a single named key (`enter`, `escape`, `ctrl+c`, …) into a session.
    /// Requires the session's "Agents" consent.
    SendKey {
        /// Session name (title), directory basename, or id.
        target: String,
        /// The named key.
        key: String,
    },
    /// Run a command in a built-in shell session: type it, wait for the output
    /// to settle, and print just the new output as JSON (`output`, `cursor`,
    /// `timedOut`). Collapses the send/poll/read loop. Requires "Agents" consent.
    Exec {
        /// Session name (title), directory basename, or id.
        target: String,
        /// The command to run.
        command: String,
        /// Return once output has been quiet this many ms (default 600).
        #[arg(long)]
        quiet_ms: Option<u64>,
        /// Give up waiting after this many ms (default 15000).
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Ask the session's owner, in their chat app, for access when "Agents"
    /// consent is off: run a single `--command` once, or let them enable full
    /// control. Blocks until the owner answers. Prints the decision as JSON.
    RequestAccess {
        /// Session name (title), directory basename, or id.
        target: String,
        /// A single command to request permission to run once.
        #[arg(long)]
        command: Option<String>,
        /// Why you need access — shown to the owner in the prompt.
        #[arg(long)]
        reason: Option<String>,
    },
}

/// Env override for the broker socket path (kept in lockstep with the desktop's
/// `session_broker::server`). The default targets the **production** app's
/// broker; a dev-build app binds `…/buzz-dev/session-broker.sock` instead —
/// set the env override to reach it.
const SOCKET_PATH_ENV: &str = "BUZZ_SESSION_BROKER_SOCK";
const DEFAULT_SOCKET_REL: &str = ".local/state/buzz/session-broker.sock";

pub async fn dispatch(cmd: &SessionCmd, caller: Option<String>) -> Result<(), CliError> {
    let caller = caller.as_deref();
    match cmd {
        SessionCmd::List => {
            // Print the broker's full session objects verbatim (all fields),
            // not the resolution subset.
            let result = broker_call(json!({ "op": "list" }), caller)?;
            let json = serde_json::to_string_pretty(&result)
                .map_err(|e| CliError::Other(format!("failed to encode sessions: {e}")))?;
            println!("{json}");
            Ok(())
        }
        SessionCmd::Read {
            target,
            scrollback,
            rendered,
            since,
            json: as_json,
        } => {
            let id = resolve_session(target, caller)?;
            let result = broker_call(
                json!({
                    "op": "read",
                    "workspace_id": id,
                    "scrollback": scrollback,
                    "rendered": rendered,
                    "since": since,
                }),
                caller,
            )?;
            if *as_json {
                let json = serde_json::to_string_pretty(&result)
                    .map_err(|e| CliError::Other(format!("failed to encode read: {e}")))?;
                println!("{json}");
            } else {
                let text = result
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                println!("{text}");
            }
            Ok(())
        }
        SessionCmd::Send { target, text } => {
            let id = resolve_session(target, caller)?;
            broker_call(
                json!({ "op": "send", "workspace_id": id, "text": text }),
                caller,
            )?;
            print_delivered(&id, "send");
            Ok(())
        }
        SessionCmd::SendKey { target, key } => {
            let id = resolve_session(target, caller)?;
            broker_call(
                json!({ "op": "send_key", "workspace_id": id, "key": key }),
                caller,
            )?;
            print_delivered(&id, "send-key");
            Ok(())
        }
        SessionCmd::Exec {
            target,
            command,
            quiet_ms,
            timeout_ms,
        } => {
            let id = resolve_session(target, caller)?;
            // The broker waits up to timeout_ms for output to settle, and — if
            // the session lacks agent consent — additionally blocks on the
            // owner's approval prompt (capped at 180s). The socket read must
            // outlast both, so it doesn't fire before the broker answers.
            let socket_timeout =
                Duration::from_millis(timeout_ms.unwrap_or(15_000)) + Duration::from_secs(185);
            let result = broker_call_with_timeout(
                json!({
                    "op": "exec",
                    "workspace_id": id,
                    "command": command,
                    "quiet_ms": quiet_ms,
                    "timeout_ms": timeout_ms,
                }),
                caller,
                socket_timeout,
            )?;
            let json = serde_json::to_string_pretty(&result)
                .map_err(|e| CliError::Other(format!("failed to encode exec result: {e}")))?;
            println!("{json}");
            Ok(())
        }
        SessionCmd::RequestAccess {
            target,
            command,
            reason,
        } => {
            let id = resolve_session(target, caller)?;
            // The owner may take a while to answer; the broker caps the wait at
            // 180s, so the socket read must outlast that.
            let result = broker_call_with_timeout(
                json!({
                    "op": "request_access",
                    "workspace_id": id,
                    "command": command,
                    "reason": reason,
                }),
                caller,
                Duration::from_secs(185),
            )?;
            let json = serde_json::to_string_pretty(&result)
                .map_err(|e| CliError::Other(format!("failed to encode access result: {e}")))?;
            println!("{json}");
            Ok(())
        }
    }
}

fn print_delivered(workspace_id: &str, action: &str) {
    // Machine-readable status to stdout, human note to stderr (write convention).
    println!(
        "{}",
        json!({ "delivered": true, "session": workspace_id, "action": action })
    );
    eprintln!("Delivered {action} to session {workspace_id}.");
}

/// A session as the broker reports it (subset of the backend `CmuxSession`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummary {
    workspace_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    current_directory: Option<String>,
}

fn broker_list(caller: Option<&str>) -> Result<Vec<SessionSummary>, CliError> {
    let result = broker_call(json!({ "op": "list" }), caller)?;
    serde_json::from_value(result)
        .map_err(|e| CliError::Other(format!("failed to parse session list: {e}")))
}

/// Resolve a name/basename/id to a session id, disambiguating like
/// `messages::resolve_author`: exact id → exact title → fuzzy (title/basename
/// substring). 0 → usage error; 1 → that session; N → list the candidates.
fn resolve_session(target: &str, caller: Option<&str>) -> Result<String, CliError> {
    let sessions = broker_list(caller)?;
    if sessions.iter().any(|s| s.workspace_id == target) {
        return Ok(target.to_string());
    }
    let needle = target.to_lowercase();

    let exact: Vec<&SessionSummary> = sessions
        .iter()
        .filter(|s| {
            s.title
                .as_deref()
                .map(|t| t.to_lowercase() == needle)
                .unwrap_or(false)
        })
        .collect();
    match exact.len() {
        1 => return Ok(exact[0].workspace_id.clone()),
        n if n > 1 => return Err(ambiguous(target, &exact)),
        _ => {}
    }

    let fuzzy: Vec<&SessionSummary> = sessions
        .iter()
        .filter(|s| {
            let title_hit = s
                .title
                .as_deref()
                .map(|t| t.to_lowercase().contains(&needle))
                .unwrap_or(false);
            let dir_hit = s
                .current_directory
                .as_deref()
                .map(|c| basename(c).to_lowercase().contains(&needle))
                .unwrap_or(false);
            title_hit || dir_hit
        })
        .collect();
    match fuzzy.len() {
        0 => Err(CliError::Usage(format!(
            "no session matches '{target}'. Run `buzz session list` to see running sessions."
        ))),
        1 => Ok(fuzzy[0].workspace_id.clone()),
        _ => Err(ambiguous(target, &fuzzy)),
    }
}

fn ambiguous(target: &str, candidates: &[&SessionSummary]) -> CliError {
    let shown: Vec<String> = candidates
        .iter()
        .take(5)
        .map(|s| {
            format!(
                "{} — {} ({})",
                s.title.as_deref().unwrap_or("(untitled)"),
                s.current_directory.as_deref().unwrap_or("?"),
                s.workspace_id
            )
        })
        .collect();
    let more = if candidates.len() > 5 {
        format!(" … and {} more", candidates.len() - 5)
    } else {
        String::new()
    };
    CliError::Usage(format!(
        "'{target}' matches {} sessions; disambiguate by id:\n  {}{more}",
        candidates.len(),
        shown.join("\n  ")
    ))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn broker_socket_path() -> Result<PathBuf, CliError> {
    if let Ok(explicit) = std::env::var(SOCKET_PATH_ENV) {
        if !explicit.is_empty() {
            return Ok(PathBuf::from(explicit));
        }
    }
    let home = std::env::var("HOME").map_err(|_| CliError::Other("HOME is not set".to_string()))?;
    Ok(PathBuf::from(home).join(DEFAULT_SOCKET_REL))
}

/// One request/response round trip with the broker (default 8s timeout).
fn broker_call(request: Value, caller: Option<&str>) -> Result<Value, CliError> {
    broker_call_with_timeout(request, caller, Duration::from_secs(8))
}

/// One request/response round trip with the broker, with an explicit socket
/// timeout (longer for exec/request-access, which the broker holds open while
/// it waits for output to settle or the owner to answer). Maps a broker error
/// frame to a `CliError` so exit codes follow convention.
#[cfg(unix)]
fn broker_call_with_timeout(
    request: Value,
    caller: Option<&str>,
    timeout: Duration,
) -> Result<Value, CliError> {
    use std::os::unix::net::UnixStream;

    let path = broker_socket_path()?;
    let mut stream = UnixStream::connect(&path).map_err(|e| {
        CliError::Other(format!(
            "cannot reach the buzz desktop app (session broker at {}): {e}. \
             Is the desktop app running?",
            path.display()
        ))
    })?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|_| stream.set_write_timeout(Some(Duration::from_secs(8))))
        .map_err(|e| CliError::Other(format!("socket setup failed: {e}")))?;

    let envelope = json!({ "caller": caller, "request": request });
    let mut line = serde_json::to_string(&envelope)
        .map_err(|e| CliError::Other(format!("encode request: {e}")))?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| CliError::Other(format!("send request: {e}")))?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(|e| CliError::Other(format!("read response: {e}")))?;

    let response: BrokerResponse = serde_json::from_str(response_line.trim_end())
        .map_err(|e| CliError::Other(format!("malformed broker response: {e}")))?;
    if response.ok {
        Ok(response.result.unwrap_or(Value::Null))
    } else {
        Err(CliError::Usage(
            response
                .error
                .unwrap_or_else(|| "unknown broker error".to_string()),
        ))
    }
}

#[cfg(not(unix))]
fn broker_call_with_timeout(
    _request: Value,
    _caller: Option<&str>,
    _timeout: Duration,
) -> Result<Value, CliError> {
    Err(CliError::Other(
        "session commands require a Unix platform".to_string(),
    ))
}

#[derive(Debug, Deserialize)]
struct BrokerResponse {
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(id: &str, title: Option<&str>, cwd: Option<&str>) -> SessionSummary {
        SessionSummary {
            workspace_id: id.to_string(),
            title: title.map(str::to_string),
            current_directory: cwd.map(str::to_string),
        }
    }

    // resolve_session hits the broker, so test the pure matching directly.
    fn resolve(sessions: &[SessionSummary], target: &str) -> Result<String, String> {
        if sessions.iter().any(|s| s.workspace_id == target) {
            return Ok(target.to_string());
        }
        let needle = target.to_lowercase();
        let exact: Vec<&SessionSummary> = sessions
            .iter()
            .filter(|s| {
                s.title
                    .as_deref()
                    .map(|t| t.to_lowercase() == needle)
                    .unwrap_or(false)
            })
            .collect();
        match exact.len() {
            1 => return Ok(exact[0].workspace_id.clone()),
            n if n > 1 => return Err(format!("ambiguous:{n}")),
            _ => {}
        }
        let fuzzy: Vec<&SessionSummary> = sessions
            .iter()
            .filter(|s| {
                s.title
                    .as_deref()
                    .map(|t| t.to_lowercase().contains(&needle))
                    .unwrap_or(false)
                    || s.current_directory
                        .as_deref()
                        .map(|c| basename(c).to_lowercase().contains(&needle))
                        .unwrap_or(false)
            })
            .collect();
        match fuzzy.len() {
            0 => Err("none".to_string()),
            1 => Ok(fuzzy[0].workspace_id.clone()),
            n => Err(format!("ambiguous:{n}")),
        }
    }

    #[test]
    fn resolves_exact_id_and_title() {
        let s = vec![
            sess("WS1", Some("Rousseau"), Some("/Users/andy/Code/innovo")),
            sess("WS2", Some("TankLoop"), Some("/Users/andy/Code/tankloop")),
        ];
        assert_eq!(resolve(&s, "WS1").unwrap(), "WS1");
        assert_eq!(resolve(&s, "rousseau").unwrap(), "WS1");
        assert_eq!(resolve(&s, "TankLoop").unwrap(), "WS2");
    }

    #[test]
    fn resolves_by_directory_basename_and_reports_missing() {
        let s = vec![sess(
            "WS2",
            Some("TankLoop"),
            Some("/Users/andy/Code/tankloop"),
        )];
        assert_eq!(resolve(&s, "tankloop").unwrap(), "WS2");
        assert_eq!(resolve(&s, "nope").unwrap_err(), "none");
    }

    #[test]
    fn ambiguous_title_substring_is_rejected() {
        let s = vec![
            sess("WS1", Some("Book Tool"), Some("/a/book-tool")),
            sess("WS2", Some("Book Club"), Some("/a/book-club")),
        ];
        assert_eq!(resolve(&s, "book").unwrap_err(), "ambiguous:2");
    }
}
