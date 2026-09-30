//! The host control protocol: newline-delimited JSON over an owner-only
//! Unix socket, one request line answered by one response line.
//!
//! The same shape as the session broker's protocol, for the same reasons: it
//! needs no framing library, it is readable with `nc`, and a connection that
//! dies mid-request cannot leave a half-applied operation.
//!
//! # What this channel is, and is not
//!
//! It carries **launcher-local facts only** — is the host installed, is it
//! running, its child's pid, its logs, start and stop. Agent turns, steering
//! and shutdown stay on the relay, where they are authenticated by keypair.
//!
//! That distinction is what keeps `VISION_REMOTE_AGENTS.md`'s axiom intact:
//! "after deploy, the desktop retains no substrate control channel." This is
//! not a substrate channel. It is a layer-1 launcher facility in the sense of
//! `docs/remote-agents.md` § Launchers — same machine, owner-only socket,
//! carrying only the facts the relay cannot carry.
//!
//! # Versioning by rejection, not negotiation
//!
//! [`PROTOCOL_VERSION`] is answered by `hello` and an unknown version gets a
//! named error rather than a best-effort attempt. An old host and a new client
//! must disagree *loudly*: the alternative is a client rendering a status it
//! half-understands, which is worse than a client that says it cannot talk to
//! this host.

use serde::{Deserialize, Serialize};

use crate::sessions::SessionSnapshot;
use crate::state::{ProviderChildState, RelayConnectionState, RunSettings};

/// The wire version. Bumped on any change a v1 client could misread.
pub const PROTOCOL_VERSION: u32 = 1;

/// Bound on a `logs` tail, in bytes. A control socket must not become a way to
/// stream an unbounded file into a menu bar's memory.
pub const MAX_LOG_TAIL_BYTES: u64 = 256 * 1024;

/// What a client asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum Request {
    /// Protocol and build identity. A client that cannot read the version
    /// should ask nothing else.
    Hello,
    /// Everything the host knows, in one round trip.
    Status,
    /// A bounded tail of the supervised provider's log.
    Logs {
        /// Bytes from the end, capped at [`MAX_LOG_TAIL_BYTES`].
        #[serde(default)]
        bytes: Option<u64>,
    },
    /// Stop the provider child. The host keeps running.
    Stop,
    /// Start the provider child if it is not running.
    Start,
    /// Stop and start the provider child.
    Restart,
    /// Re-read `host.json` and the identity, then swap the supervisor onto
    /// the result. This is the community switch.
    ///
    /// The app writes the file and then asks; the host does not take a relay
    /// URL over the wire. That keeps one writer for `host.json` and makes the
    /// file, not a message, the thing a headless install edits.
    Bind,
    /// Re-read the identity after the app has written the key file.
    ///
    /// **The secret never crosses this socket.** The app writes the `0600`
    /// file and this only says "look again". A socket capture therefore
    /// reveals nothing signable, and a future hardening of peer
    /// authentication does not change the secret's blast radius.
    ///
    /// The same operation as `bind`; a separate op because the two mean
    /// different things to whoever reads a log of them.
    AdoptIdentity,
    /// The app contributes its managed-agent rows, under a lease.
    ///
    /// The host does not know about managed agents — they are the app's
    /// children and still die with it — so the app says what they are and the
    /// menu bar shows a complete picture. The rows expire if the app stops
    /// saying so, which is how they vanish when it quits: see
    /// [`crate::activity`].
    PushActivity {
        #[serde(default)]
        rows: Vec<crate::activity::PushedActivity>,
    },
}

/// What the host answers. One shape for every op, so a client's read path does
/// not branch before it knows whether the call worked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(result: serde_json::Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(message.into()),
        }
    }
}

/// The answer to `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    /// [`PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// The host crate's version.
    pub host_version: String,
    /// The host's pid, so a client can tell one host from its replacement
    /// across a restart without waiting for a socket error.
    pub host_pid: u32,
}

impl Hello {
    pub fn current() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            host_pid: std::process::id(),
        }
    }
}

/// The answer to `status` — enough to tell the four situations apart.
///
/// Situations 1 and 2 (*not installed*, *installed but not running*) are
/// answered by the client, from the socket's absence plus the presence of a
/// login registration. This structure answers 3 and 4, and every field that
/// could be a comfortable guess is instead a named state with a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub protocol_version: u32,
    pub host_version: String,
    pub host_pid: u32,
    /// RFC 3339 stamp of when this host started.
    pub host_started_at: String,
    /// The relay this host was commissioned to serve.
    pub relay_url: String,
    /// The provisioned provider's pubkey.
    pub provider_pubkey: String,
    /// Where the provider's state directory is, so a client can find the log
    /// and the seat-request file without deriving the path itself.
    pub provider_state_dir: std::path::PathBuf,
    /// The supervised child, with its reason.
    pub provider: ProviderChildState,
    /// What the **running** provider was started with, or `None` when nothing
    /// is running.
    ///
    /// Distinct from the stored settings, which the app holds: the child reads
    /// its limits from the environment at startup, so a change takes effect at
    /// the next start and never mid-flight. A surface showing only the stored
    /// value would name a ceiling nothing is enforcing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_settings_in_force: Option<RunSettings>,
    /// Always `unknown`, with a `why`. See [`RelayConnectionState`].
    pub relay_connection: RelayConnectionState,
    /// Every session the provider is holding, read from its own
    /// atomically-replaced snapshot, stamped with the read time.
    ///
    /// Most of them are not doing anything: the provider keeps a record per
    /// session it has attached to. Use [`turns_in_flight`](Self::turns_in_flight)
    /// for "how many agents are working" — `sessions.sessions.len()` is a
    /// different number and reading it as that one is a mistake this field's
    /// old wording invited.
    pub sessions: SessionSnapshot,
    /// How many of those sessions have a turn open.
    ///
    /// On the wire so that reading `bee host status` answers the question
    /// without knowing the rule. Derived from `sessions` at the moment the
    /// host answers, in one place, so it cannot drift from the rows beside it.
    pub turns_in_flight: usize,
    /// Managed-agent rows the app most recently pushed, still under lease.
    ///
    /// Empty when no app is running — which is *correct*, because those
    /// agents died with it. `appActivityLeased` says which empty this is, so a
    /// menu can distinguish "the app says none are running" from "no app is
    /// running to ask".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_activity: Vec<crate::activity::PushedActivity>,
    /// Whether an app is currently holding an activity lease.
    pub app_activity_leased: bool,
    /// Facts a client must disclose rather than absorb.
    ///
    /// Ordinary, expected degradations belong here, not in `error`: a call
    /// that worked and has something to disclose is not a failed call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
}

/// A named degradation the client is expected to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    /// Stable machine-readable code, `SCREAMING_SNAKE_CASE`.
    pub code: String,
    /// One line for a person.
    pub message: String,
}

impl Warning {
    /// Seats are re-staged by the desktop app, because re-staging reads the
    /// relay and the managed-agent store. With no desktop running, the rows in
    /// `seat-requests.json` wait — which is acceptable, and must be visible.
    pub fn seat_restage_requires_desktop(rows: usize) -> Self {
        Self {
            code: "SEAT_RESTAGE_REQUIRES_DESKTOP".to_string(),
            message: format!(
                "{rows} agent seat(s) are waiting to be re-staged, which only Beekeeper can do — \
                 open the app to finish it"
            ),
        }
    }
}

/// The answer to `logs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logs {
    /// The file the tail came from, so an operator can open it themselves.
    pub path: std::path::PathBuf,
    /// Whether bytes were dropped from the front of the tail.
    pub truncated: bool,
    /// The tail, lossily decoded — a log may hold a partial UTF-8 sequence at
    /// whichever byte the tail started on, and refusing to answer over that
    /// would make the log unreadable exactly when it is needed.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_kebab_case_ops_and_round_trip() {
        for (request, wire) in [
            (Request::Hello, r#"{"op":"hello"}"#),
            (Request::Status, r#"{"op":"status"}"#),
            (Request::Stop, r#"{"op":"stop"}"#),
            (Request::Start, r#"{"op":"start"}"#),
            (Request::Restart, r#"{"op":"restart"}"#),
            (
                Request::PushActivity { rows: Vec::new() },
                r#"{"op":"push-activity","rows":[]}"#,
            ),
            (Request::Bind, r#"{"op":"bind"}"#),
            (Request::AdoptIdentity, r#"{"op":"adopt-identity"}"#),
        ] {
            assert_eq!(serde_json::to_string(&request).expect("encode"), wire);
            assert_eq!(
                serde_json::from_str::<Request>(wire).expect("decode"),
                request
            );
        }
        // `rows` is optional too, so an app with nothing to say can send the
        // bare op and still hold its lease.
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"push-activity"}"#).expect("decode"),
            Request::PushActivity { rows: Vec::new() }
        );
        // `bytes` is optional, so the simplest possible logs call works.
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"logs"}"#).expect("decode"),
            Request::Logs { bytes: None }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"logs","bytes":1024}"#).expect("decode"),
            Request::Logs { bytes: Some(1024) }
        );
    }

    /// An op this host does not know must be refused, not guessed at.
    #[test]
    fn an_unknown_op_fails_to_decode_rather_than_matching_something_close() {
        assert!(serde_json::from_str::<Request>(r#"{"op":"shutdown"}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"op":"Status"}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{}"#).is_err());
    }

    #[test]
    fn a_failed_call_carries_an_error_and_no_result() {
        let response = Response::err("nope");
        let json = serde_json::to_string(&response).expect("encode");
        assert_eq!(json, r#"{"ok":false,"error":"nope"}"#);
        let response = Response::ok(serde_json::json!({"a": 1}));
        assert_eq!(
            serde_json::to_string(&response).expect("encode"),
            r#"{"ok":true,"result":{"a":1}}"#
        );
    }

    /// A degradation is a disclosure on a successful call, not an error: a
    /// client that treated it as a failure would show nothing at all.
    #[test]
    fn a_warning_has_a_stable_code_and_a_human_line() {
        let warning = Warning::seat_restage_requires_desktop(3);
        assert_eq!(warning.code, "SEAT_RESTAGE_REQUIRES_DESKTOP");
        assert!(warning.message.contains("open the app"), "{warning:?}");
        assert_eq!(
            warning.code,
            warning.code.to_uppercase(),
            "codes are SCREAMING_SNAKE_CASE so a client can switch on them"
        );
    }

    #[test]
    fn hello_names_this_build_and_this_process() {
        let hello = Hello::current();
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
        assert_eq!(hello.host_pid, std::process::id());
        assert!(!hello.host_version.is_empty());
    }
}
