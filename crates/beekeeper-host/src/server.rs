//! The control socket: `~/.local/state/buzz[-dev]/host/host.sock`.
//!
//! One request line, one response line, one connection — the shape the session
//! broker already uses, for the same reasons.
//!
//! # The peer check the broker deliberately does without
//!
//! `session_broker/server.rs` records its own trust note: the caller's identity
//! is self-declared and "the socket permission, not the pubkey, is the actual
//! security boundary." That is the right bar for reading a terminal's
//! scrollback. It is a weaker bar than is right for ops that start and stop a
//! process holding a signing key, so this socket also checks the peer's uid
//! against its own and refuses anything else.
//!
//! The policy is [`peer_is_owner`], extracted so it is unit-testable; only the
//! syscall that fetches the uid is untested, and it is three lines.

use std::sync::Arc;

pub use crate::transport::bind;
#[cfg(unix)]
pub use crate::transport::peer_is_owner;
use crate::transport::{self, Listener, Stream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::control::HostControl;
use crate::protocol::{Hello, Logs, Request, Response, Status, Warning, MAX_LOG_TAIL_BYTES};
use crate::state::RelayConnectionState;

/// Serve an already-bound socket until the task is cancelled.
pub async fn serve(
    control: Arc<HostControl>,
    listener: Listener,
    path: std::path::PathBuf,
) -> Result<(), String> {
    let available_at = host_available_at();
    tracing::info!(
        "control socket listening on {} (available at {available_at})",
        path.display()
    );

    loop {
        match transport::accept(&listener).await {
            Ok(stream) => {
                let control = Arc::clone(&control);
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(stream, control).await {
                        tracing::warn!("control connection error: {error}");
                    }
                });
            }
            Err(error) => {
                tracing::error!("control socket accept failed: {error}");
                return Err(format!("accept failed: {error}"));
            }
        }
    }
}

async fn handle_connection(stream: Stream, control: Arc<HostControl>) -> Result<(), String> {
    let authorized = transport::authorized(&stream);
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .await
        .map_err(|error| format!("read: {error}"))?
        == 0
    {
        return Ok(());
    }

    let response = if !authorized {
        // Named rather than silent: a client that cannot tell "refused" from
        // "no host" will show the wrong thing.
        tracing::warn!("refused a control connection from another uid");
        Response::err(
            "this control socket only answers the user who owns it (peer uid mismatch)".to_string(),
        )
    } else {
        match serde_json::from_str::<Request>(line.trim_end()) {
            Ok(request) => dispatch(request, &control).await,
            Err(error) => Response::err(format!("malformed request: {error}")),
        }
    };

    let mut out = serde_json::to_string(&response).map_err(|error| format!("encode: {error}"))?;
    out.push('\n');
    write_half
        .write_all(out.as_bytes())
        .await
        .map_err(|error| format!("write: {error}"))?;
    Ok(())
}

async fn dispatch(request: Request, control: &Arc<HostControl>) -> Response {
    match request {
        Request::Hello => encode(&Hello::current()),
        Request::Status => encode(&status(control)),
        Request::Inventory => encode(&inventory(control)),
        Request::Logs { bytes } => match read_log_tail(&control.log_path(), bytes) {
            Ok(logs) => encode(&logs),
            Err(error) => Response::err(error),
        },
        // `stop` does carry the state, because it awaited the child's death
        // before answering — this one is observed, not guessed.
        Request::Stop => {
            let was_running = control.stop().await;
            Response::ok(serde_json::json!({
                "stopped": was_running,
                "provider": control.child_state(),
            }))
        }
        // `start` and `restart` deliberately return no provider state. They
        // are requests *accepted*, not states *observed*: the loop has not
        // spawned a child by the time this answers, so any state here would be
        // the previous one — and "notSupervised" in the reply to a successful
        // start is precisely the comfortable guess this protocol refuses to
        // make. The client polls `status` for what actually happened.
        Request::Start => match control.start() {
            Ok(started) => Response::ok(serde_json::json!({ "started": started })),
            Err(error) => Response::err(error),
        },
        Request::Restart => match control.restart().await {
            Ok(()) => Response::ok(serde_json::json!({ "restarted": true })),
            Err(error) => Response::err(error),
        },
        // Both re-read the files the app wrote. Separate ops because the two
        // mean different things in a log, and because a future version may
        // want to answer them differently.
        // Accepted and acknowledged with what the host now holds, so a caller
        // can tell a push that landed from one that was refused.
        Request::PushActivity { rows } => {
            let count = rows.len();
            control.app_activity().push(rows);
            Response::ok(serde_json::json!({ "accepted": count }))
        }
        Request::Bind | Request::AdoptIdentity => match control.recommission().await {
            Ok(()) => Response::ok(serde_json::json!({
                "relayUrl": control.config().relay_url,
                "providerPubkey": control.config().provider_pubkey,
            })),
            Err(error) => Response::err(error),
        },
    }
}

fn encode<T: serde::Serialize>(value: &T) -> Response {
    match serde_json::to_value(value) {
        Ok(value) => Response::ok(value),
        Err(error) => Response::err(format!("failed to encode the response: {error}")),
    }
}

/// When this host began answering, stamped once.
///
/// [`serve`] stamps it before its first `accept`. The `get_or_init` here is
/// only a floor for a caller that never served — it used to be the *only*
/// stamp, which made the field mean "when somebody first asked".
fn host_available_at() -> &'static str {
    static AVAILABLE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    AVAILABLE.get_or_init(beekeeper_host_core::logs::now_iso)
}

fn status(control: &Arc<HostControl>) -> Status {
    let config = control.config();
    let sessions = crate::sessions::read_sessions(&config.provider_state_dir);
    let mut warnings = Vec::new();
    if let Some(rows) = pending_seat_requests(&config.provider_state_dir) {
        warnings.push(Warning::seat_custody_pending(rows));
    }
    Status {
        protocol_version: crate::protocol::PROTOCOL_VERSION,
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        host_pid: std::process::id(),
        host_available_at: host_available_at().to_string(),
        relay_url: config.relay_url.clone(),
        provider_pubkey: config.provider_pubkey.clone(),
        provider_state_dir: config.provider_state_dir.clone(),
        provider: control.child_state(),
        provider_settings_in_force: control.settings_in_force(),
        relay_connection: RelayConnectionState::unknowable(),
        turns_in_flight: sessions.turns_in_flight(),
        sessions,
        app_activity: control.app_activity().current(),
        app_activity_leased: control.app_activity().has_live_lease(),
        warnings,
    }
}

fn inventory(control: &Arc<HostControl>) -> crate::inventory::Inventory {
    let config = control.config();
    let sessions = crate::sessions::read_sessions(&config.provider_state_dir);
    crate::inventory::read(crate::inventory::InventoryInput {
        home: control.home(),
        instance: control.instance(),
        config: &config,
        provider: control.child_state(),
        host_pid: std::process::id(),
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        sessions: &sessions,
    })
}

/// The worker's unresolved recovery-custody requests, when any remain.
/// The provider excludes modern requests already served by its protected
/// routed-actor custody adapter. Unknown/role-seat rows remain visible here;
/// the host does not read signing secrets or infer desktop dependency.
fn pending_seat_requests(state_dir: &std::path::Path) -> Option<usize> {
    #[derive(serde::Deserialize)]
    struct SeatRequests {
        #[serde(default)]
        requests: Vec<serde_json::Value>,
    }
    let content = std::fs::read_to_string(state_dir.join("seat-requests.json")).ok()?;
    let file: SeatRequests = serde_json::from_str(&content).ok()?;
    (!file.requests.is_empty()).then_some(file.requests.len())
}

/// Read the last `bytes` of `path`, capped at [`MAX_LOG_TAIL_BYTES`].
fn read_log_tail(path: &std::path::Path, bytes: Option<u64>) -> Result<Logs, String> {
    use std::io::{Read, Seek, SeekFrom};

    let want = bytes.unwrap_or(MAX_LOG_TAIL_BYTES).min(MAX_LOG_TAIL_BYTES);
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?
        .len();
    let start = len.saturating_sub(want);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("failed to seek {}: {error}", path.display()))?;
    let mut buffer = Vec::with_capacity(want.min(len) as usize);
    file.read_to_end(&mut buffer)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    Ok(Logs {
        path: path.to_path_buf(),
        truncated: start > 0,
        // Lossy on purpose: a tail starts at an arbitrary byte, so it can begin
        // mid-character. Refusing to answer over that would make the log
        // unreadable exactly when somebody needs it.
        text: String::from_utf8_lossy(&buffer).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::transport::check_socket_path;

    /// The rule, stated as a test: same uid, and nothing else. An unknown peer
    /// is refused — failing open on an op that can stop a signing process is
    /// not a trade worth making.
    #[cfg(unix)]
    #[test]
    fn only_the_owning_uid_may_drive_the_host() {
        assert!(peer_is_owner(Some(501), 501));
        assert!(!peer_is_owner(Some(502), 501));
        assert!(!peer_is_owner(Some(0), 501), "not even root");
        assert!(
            !peer_is_owner(None, 501),
            "an unknown peer must be refused, not waved through"
        );
    }

    /// A path the kernel will reject must be refused with the reason and the
    /// override, not with a bare `EINVAL` an operator cannot act on.
    #[cfg(unix)]
    #[test]
    fn an_over_long_socket_path_is_refused_by_name() {
        assert!(check_socket_path(std::path::Path::new("/tmp/host.sock")).is_ok());
        let long = std::path::PathBuf::from(format!("/tmp/{}/host.sock", "x".repeat(200)));
        let error = check_socket_path(&long).expect_err("must be refused");
        assert!(error.contains("longer than"), "{error}");
        assert!(
            error.contains(beekeeper_host_core::layout::SOCKET_VAR),
            "the message must name the way out: {error}"
        );
    }

    /// A socket in a directory somebody else owns must bind without changing
    /// that directory. Chmod-ing an operator's `/tmp` to 0700 because a socket
    /// lives there is not the host's business — and as root it would succeed.
    #[cfg(unix)]
    #[test]
    fn binding_into_an_existing_directory_leaves_its_permissions_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let shared = dir.path().join("shared");
        std::fs::create_dir(&shared).expect("mkdir");
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let socket = shared.join("h.sock");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let listener = runtime.block_on(async { bind(&socket) }).expect("bind");
        drop(listener);

        let mode = std::fs::metadata(&shared)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "the existing directory must be untouched");
        let socket_mode = std::fs::metadata(&socket)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(socket_mode, 0o600, "the socket itself is owner-only");
    }

    #[test]
    fn a_log_tail_is_capped_and_says_when_it_dropped_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("provider.log");
        std::fs::write(&path, "abcdefghij").expect("write");

        let whole = read_log_tail(&path, None).expect("tail");
        assert_eq!(whole.text, "abcdefghij");
        assert!(!whole.truncated, "a whole file is not truncated");
        assert_eq!(whole.path, path);

        let tail = read_log_tail(&path, Some(4)).expect("tail");
        assert_eq!(tail.text, "ghij");
        assert!(tail.truncated, "a partial tail must say so");

        // A request larger than the cap is capped, not honoured.
        std::fs::write(&path, vec![b'x'; (MAX_LOG_TAIL_BYTES + 100) as usize]).expect("write");
        let capped = read_log_tail(&path, Some(u64::MAX)).expect("tail");
        assert_eq!(capped.text.len() as u64, MAX_LOG_TAIL_BYTES);
        assert!(capped.truncated);
    }

    #[test]
    fn a_missing_log_is_an_error_that_names_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = read_log_tail(&dir.path().join("absent.log"), None).expect_err("must fail");
        assert!(error.contains("absent.log"), "{error}");
    }

    #[test]
    fn a_tail_that_begins_mid_character_still_answers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("provider.log");
        // "é" is two bytes; asking for one of them must not fail the call.
        std::fs::write(&path, "aé").expect("write");
        let tail = read_log_tail(&path, Some(1)).expect("a partial character must not fail");
        assert!(!tail.text.is_empty());
    }

    #[test]
    fn seat_requests_are_only_disclosed_when_there_are_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(pending_seat_requests(dir.path()), None, "no file");
        std::fs::write(
            dir.path().join("seat-requests.json"),
            r#"{"version":1,"providerPid":42,"requests":[]}"#,
        )
        .expect("write");
        assert_eq!(
            pending_seat_requests(dir.path()),
            None,
            "an empty list is nothing to disclose"
        );
        std::fs::write(
            dir.path().join("seat-requests.json"),
            r#"{"version":1,"providerPid":42,"requests":[{"a":1},{"b":2}]}"#,
        )
        .expect("write");
        assert_eq!(pending_seat_requests(dir.path()), Some(2));
    }
}
