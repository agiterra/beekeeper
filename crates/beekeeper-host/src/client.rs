//! Talking to a running host over its control socket.
//!
//! Shared by `bee host …`, the menu bar app and the desktop app, so that all
//! three agree about what "the host is not running" means. The distinction
//! this type exists to preserve:
//!
//! - [`ClientError::NotRunning`] — the socket is absent or refusing. Combined
//!   with whether a login registration exists, a client can tell *not
//!   installed* from *installed but not running*.
//! - [`ClientError::NotResponding`] — the socket accepted a connection and
//!   then went quiet. A hung host must render as hung, never as stopped, and
//!   never by hanging the caller.
//! - [`ClientError::Refused`] — the host answered and said no, with its own
//!   words.
//!
//! Every call is one short-lived connection: connect, write a line, read a
//! line, close. No pooling, no reconnect state, nothing to go stale — and a
//! caller polling every few seconds costs one socket per poll, which is
//! cheaper than any of the bookkeeping the alternative needs.

use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::protocol::{Request, Response};

/// How long to wait for the socket to accept a connection.
///
/// Short: on the same machine an absent host fails immediately and a present
/// one accepts immediately. Anything longer is a menu bar that stalls.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
/// How long to wait for the answer once connected.
pub const READ_TIMEOUT: Duration = Duration::from_secs(1);
/// The whole budget a *patient* status read gets — first attempt plus one
/// retry. The desktop's readiness gate and status poll use it; the menu bar
/// and `bee host status` keep the fast single [`READ_TIMEOUT`] attempt.
///
/// Why a retry at all: under load the host has been seen answering after the
/// 1s read window had closed (its log showed ~40 "control connection error:
/// write: Broken pipe" — our side had already hung up) while direct probes a
/// minute later answered in 4–17ms. A launch gate that hard-blocks on one
/// missed second reads a busy computer as a missing host.
pub const PATIENT_READ_BUDGET: Duration = Duration::from_secs(5);
/// A longer budget for the ops that stop a process. `stop` waits for the
/// child's SIGINT window (10s) plus escalation (2s) before it answers.
pub const LIFECYCLE_READ_TIMEOUT: Duration = Duration::from_secs(20);

/// Why a call did not produce an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// No host is listening. With a login registration present, this is
    /// "installed but not running"; without one, "not installed".
    NotRunning { socket: std::path::PathBuf },
    /// The host accepted a connection and then did not answer in time.
    NotResponding { after: Duration },
    /// The host answered with a refusal.
    Refused { message: String },
    /// Something below the protocol went wrong.
    Transport { message: String },
}

impl ClientError {
    /// One line for a person, and the words are the point: none of these may
    /// read as "no agents are running".
    pub fn message(&self) -> String {
        match self {
            Self::NotRunning { socket } => format!(
                "the agent host is not running (nothing is listening on {})",
                socket.display()
            ),
            Self::NotResponding { after } => format!(
                "the agent host did not answer within {} — the computer may be busy",
                budget_words(*after)
            ),
            Self::Refused { message } => message.clone(),
            Self::Transport { message } => format!("could not talk to the agent host: {message}"),
        }
    }
}

/// `5s` for whole seconds, `250ms` otherwise — the budget as a person reads it.
fn budget_words(after: Duration) -> String {
    let millis = after.as_millis();
    if millis >= 1000 && millis.is_multiple_of(1000) {
        format!("{}s", millis / 1000)
    } else {
        format!("{millis}ms")
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for ClientError {}

/// Send one request and return the `result` payload the host answered with.
pub async fn call(
    socket: &Path,
    request: &Request,
    read_timeout: Duration,
) -> Result<serde_json::Value, ClientError> {
    let stream = match tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(socket)).await {
        Ok(Ok(stream)) => stream,
        // A refused or absent socket is "not running", which is a fact a
        // client renders — not an error it reports as a malfunction.
        Ok(Err(_)) => {
            return Err(ClientError::NotRunning {
                socket: socket.to_path_buf(),
            })
        }
        Err(_) => {
            return Err(ClientError::NotResponding {
                after: CONNECT_TIMEOUT,
            })
        }
    };

    let (read_half, mut write_half) = stream.into_split();
    let mut line = serde_json::to_string(request).map_err(|error| ClientError::Transport {
        message: format!("encode: {error}"),
    })?;
    line.push('\n');
    write_half
        .write_all(line.as_bytes())
        .await
        .map_err(|error| ClientError::Transport {
            message: format!("write: {error}"),
        })?;

    let mut reader = BufReader::new(read_half);
    let mut answer = String::new();
    match tokio::time::timeout(read_timeout, reader.read_line(&mut answer)).await {
        Ok(Ok(0)) => {
            return Err(ClientError::Transport {
                message: "the host closed the connection without answering".to_string(),
            })
        }
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            return Err(ClientError::Transport {
                message: format!("read: {error}"),
            })
        }
        Err(_) => {
            return Err(ClientError::NotResponding {
                after: read_timeout,
            })
        }
    }

    let response: Response =
        serde_json::from_str(answer.trim_end()).map_err(|error| ClientError::Transport {
            message: format!("the host's answer was not understood: {error}"),
        })?;
    if !response.ok {
        return Err(ClientError::Refused {
            message: response
                .error
                .unwrap_or_else(|| "the host refused without saying why".to_string()),
        });
    }
    response.result.ok_or(ClientError::Transport {
        message: "the host reported success with no result".to_string(),
    })
}

/// Typed `hello`.
pub async fn hello(socket: &Path) -> Result<crate::protocol::Hello, ClientError> {
    decode(call(socket, &Request::Hello, READ_TIMEOUT).await?)
}

/// Typed `status`.
pub async fn status(socket: &Path) -> Result<crate::protocol::Status, ClientError> {
    decode(call(socket, &Request::Status, READ_TIMEOUT).await?)
}

/// Send one request; if the host is silent, try once more before giving up.
///
/// The first attempt gets `first`; the retry gets whatever of `total` is left
/// (never less than `first`). Only silence is retried — a closed connection or
/// a read timeout. `NotRunning` and `Refused` are answers, and retrying them
/// would only delay the truth. When both attempts are silent the error names
/// the whole time waited, so the sentence a person reads is the real budget.
pub async fn call_patiently(
    socket: &Path,
    request: &Request,
    first: Duration,
    total: Duration,
) -> Result<serde_json::Value, ClientError> {
    let started = std::time::Instant::now();
    match call(socket, request, first).await {
        Err(ClientError::NotResponding { .. } | ClientError::Transport { .. }) => {
            let remaining = total.saturating_sub(started.elapsed()).max(first);
            match call(socket, request, remaining).await {
                Err(ClientError::NotResponding { .. }) => Err(ClientError::NotResponding {
                    after: started.elapsed().max(total),
                }),
                other => other,
            }
        }
        other => other,
    }
}

/// Typed `status`, patiently: [`READ_TIMEOUT`] first, then one retry inside
/// [`PATIENT_READ_BUDGET`]. For gates that must not read a busy host as gone.
pub async fn status_patiently(socket: &Path) -> Result<crate::protocol::Status, ClientError> {
    decode(call_patiently(socket, &Request::Status, READ_TIMEOUT, PATIENT_READ_BUDGET).await?)
}

/// Typed `logs`.
pub async fn logs(socket: &Path, bytes: Option<u64>) -> Result<crate::protocol::Logs, ClientError> {
    decode(call(socket, &Request::Logs { bytes }, READ_TIMEOUT).await?)
}

fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, ClientError> {
    serde_json::from_value(value).map_err(|error| ClientError::Transport {
        message: format!("the host's answer was not understood: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not running, not responding and refused must read as three different
    /// things — a client that collapsed them would show "stopped" over a host
    /// that is wedged, or over one that said no.
    #[test]
    fn the_three_failures_are_three_different_sentences() {
        let messages = [
            ClientError::NotRunning {
                socket: std::path::PathBuf::from("/tmp/host.sock"),
            }
            .message(),
            ClientError::NotResponding {
                after: Duration::from_secs(1),
            }
            .message(),
            ClientError::Refused {
                message: "the provider has no identity".to_string(),
            }
            .message(),
        ];
        assert!(messages[0].contains("not running"));
        assert!(messages[1].contains("did not answer within 1s"));
        assert!(messages[1].contains("the computer may be busy"));
        assert_eq!(messages[2], "the provider has no identity");
        for message in &messages {
            assert!(
                !message.to_lowercase().contains("no agents are running"),
                "{message}"
            );
        }
    }

    #[tokio::test]
    async fn an_absent_socket_is_not_running_rather_than_a_transport_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("host.sock");
        let error = call(&socket, &Request::Hello, READ_TIMEOUT)
            .await
            .expect_err("nothing is listening");
        assert_eq!(error, ClientError::NotRunning { socket });
    }

    /// A socket that accepts and never answers must time out, not hang the
    /// caller. This is the one that would freeze a menu bar.
    #[tokio::test]
    async fn a_silent_host_times_out_and_reads_as_not_responding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("host.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        let accepting = tokio::spawn(async move {
            // Accept and hold the connection open, answering nothing.
            let (stream, _) = listener.accept().await.expect("accept");
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(stream);
        });
        let error = call(&socket, &Request::Hello, Duration::from_millis(150))
            .await
            .expect_err("no answer");
        assert!(
            matches!(error, ClientError::NotResponding { .. }),
            "{error:?}"
        );
        accepting.abort();
    }

    /// A host that answers `ok:false` is refused with *its* words, not ours.
    #[tokio::test]
    async fn a_refusal_carries_the_hosts_own_message() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("host.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        let serving = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;
            let _ = write_half
                .write_all(b"{\"ok\":false,\"error\":\"peer uid mismatch\"}\n")
                .await;
        });
        let error = call(&socket, &Request::Status, READ_TIMEOUT)
            .await
            .expect_err("refused");
        assert_eq!(
            error,
            ClientError::Refused {
                message: "peer uid mismatch".to_string()
            }
        );
        let _ = serving.await;
    }

    #[test]
    fn a_silence_names_its_budget_in_words_a_person_reads() {
        let words = |after| ClientError::NotResponding { after }.message();
        assert_eq!(
            words(Duration::from_secs(5)),
            "the agent host did not answer within 5s — the computer may be busy"
        );
        assert!(words(Duration::from_millis(250)).contains("within 250ms"));
        // One clause, no parenthetical: callers append it after a sentence of
        // their own, and the old "(no answer in 1000ms)" ran into it.
        assert!(!words(Duration::from_secs(1)).contains('('));
    }

    /// A host that misses the first window but answers the second is
    /// reachable: the retry is what keeps a busy machine from blocking launch.
    #[tokio::test]
    async fn a_patient_call_retries_once_and_takes_the_late_answer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("host.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        let serving = tokio::spawn(async move {
            // First connection: accept and say nothing.
            let (silent, _) = listener.accept().await.expect("accept");
            // Second connection: answer.
            let (stream, _) = listener.accept().await.expect("accept");
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;
            let _ = write_half
                .write_all(b"{\"ok\":true,\"result\":{\"late\":true}}\n")
                .await;
            drop(silent);
        });
        let value = call_patiently(
            &socket,
            &Request::Hello,
            Duration::from_millis(100),
            Duration::from_secs(2),
        )
        .await
        .expect("the retry answers");
        assert_eq!(value, serde_json::json!({ "late": true }));
        let _ = serving.await;
    }

    /// Silent twice: unreachable, naming the whole budget rather than the
    /// first window.
    #[tokio::test]
    async fn a_patient_call_silent_twice_names_the_whole_budget() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("host.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        let holding = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        let total = Duration::from_millis(400);
        let error = call_patiently(&socket, &Request::Hello, Duration::from_millis(100), total)
            .await
            .expect_err("silent twice");
        match error {
            ClientError::NotResponding { after } => assert!(after >= total, "{after:?}"),
            other => panic!("expected NotResponding, got {other:?}"),
        }
        holding.abort();
    }

    /// Absent is an answer: no retry, no waiting out the budget.
    #[tokio::test]
    async fn a_patient_call_does_not_retry_an_absent_host() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("host.sock");
        let started = std::time::Instant::now();
        let error = call_patiently(&socket, &Request::Hello, READ_TIMEOUT, PATIENT_READ_BUDGET)
            .await
            .expect_err("nothing is listening");
        assert_eq!(error, ClientError::NotRunning { socket });
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
