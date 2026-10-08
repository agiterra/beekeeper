//! Wire protocol for the local session broker (desktop ↔ `bee session` CLI).
//!
//! Newline-delimited JSON, one request line answered by one response line, over
//! an owner-only Unix socket. The unit is a **session** (workspaceId); the
//! broker resolves the concrete terminal surface internally. Deliberately
//! backend-agnostic: built-in shells fulfill it today; another session backend
//! could be added without changing this protocol or the CLI.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request envelope. `caller` is the requesting agent's pubkey (hex),
/// self-declared over the owner-only local socket (local-socket-trusted in
/// v1 — see `server::agent_may_drive`); writes require it to hold a
/// collaborator entry on the session's invite roster.
#[derive(Debug, Deserialize)]
pub struct BrokerEnvelope {
    #[serde(default)]
    pub caller: Option<String>,
    pub request: BrokerRequest,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BrokerRequest {
    /// Enumerate sessions (read-only).
    List,
    /// Read a session's on-screen text (read-only). `rendered` returns the
    /// terminal-emulated screen; otherwise the plain ANSI-stripped byte stream.
    /// `since` returns only output after that cursor (built-in shell only).
    Read {
        workspace_id: String,
        #[serde(default)]
        scrollback: bool,
        #[serde(default)]
        rendered: bool,
        #[serde(default)]
        since: Option<u64>,
    },
    /// Type a line of text into a session then press Enter (write — gated).
    Send { workspace_id: String, text: String },
    /// Send a single named key into a session (write — gated).
    SendKey { workspace_id: String, key: String },
    /// Run a command: type it, press Enter, wait for the output to settle (or
    /// `timeout_ms`), and return just the new output plus the cursor (write —
    /// gated). Built-in shell only; the collapsed send/poll/read round-trip.
    Exec {
        workspace_id: String,
        command: String,
        /// Return once output has been quiet this long (default 600ms).
        #[serde(default)]
        quiet_ms: Option<u64>,
        /// Give up waiting after this long (default 15000ms).
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    /// Ask the owner, in chat, for access to a session that lacks standing
    /// agent consent: run a single `command` once, or enable full control.
    /// Blocks until the owner answers or the request times out.
    RequestAccess {
        workspace_id: String,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        reason: Option<String>,
    },
    /// Drive this session's local Browser preview (`bee preview`). Authorized
    /// by the per-session preview grant, not the roster; the session comes
    /// only from the grant. `action` stays raw JSON here so a malformed one is
    /// answered `preview_bad_request` by `session_preview::broker` rather
    /// than as an unparseable envelope.
    Preview {
        #[serde(default)]
        grant: Option<String>,
        action: Value,
    },
}

#[derive(Debug, Serialize)]
pub struct BrokerResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Stable machine code for a refusal (`preview_*` ops set it; other ops
    /// leave it out, so their responses are unchanged).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl BrokerResponse {
    pub fn ok(result: Value) -> Self {
        BrokerResponse {
            ok: true,
            result: Some(result),
            error: None,
            code: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        BrokerResponse {
            ok: false,
            result: None,
            error: Some(message.into()),
            code: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_and_send() {
        let list: BrokerEnvelope =
            serde_json::from_str(r#"{"request":{"op":"list"}}"#).expect("list");
        assert!(matches!(list.request, BrokerRequest::List));
        assert!(list.caller.is_none());

        let send: BrokerEnvelope = serde_json::from_str(
            r#"{"caller":"npub1x","request":{"op":"send","workspace_id":"WS","text":"hi"}}"#,
        )
        .expect("send");
        assert_eq!(send.caller.as_deref(), Some("npub1x"));
        match send.request {
            BrokerRequest::Send { workspace_id, text } => {
                assert_eq!(workspace_id, "WS");
                assert_eq!(text, "hi");
            }
            _ => panic!("expected send"),
        }
    }

    #[test]
    fn parses_preview_with_raw_action() {
        let preview: BrokerEnvelope = serde_json::from_str(
            r#"{"request":{"op":"preview","grant":"bkpg1.x.y","action":{"verb":"click","target":{"role":"button","name":"Save"}}}}"#,
        )
        .expect("preview");
        match preview.request {
            BrokerRequest::Preview { grant, action } => {
                assert_eq!(grant.as_deref(), Some("bkpg1.x.y"));
                assert_eq!(action["verb"], "click");
                assert_eq!(action["target"]["name"], "Save");
            }
            _ => panic!("expected preview"),
        }
        // No grant still parses: the broker answers `preview_no_grant`.
        let bare: BrokerEnvelope =
            serde_json::from_str(r#"{"request":{"op":"preview","action":{"verb":"status"}}}"#)
                .expect("bare preview");
        assert!(matches!(
            bare.request,
            BrokerRequest::Preview { grant: None, .. }
        ));
    }

    #[test]
    fn response_code_is_serialized_only_when_set() {
        let mut refused = BrokerResponse::err("No preview is open.");
        refused.code = Some("preview_not_open".into());
        let text = serde_json::to_string(&refused).expect("ser");
        assert!(text.contains("\"code\":\"preview_not_open\""));
        assert!(!serde_json::to_string(&BrokerResponse::err("x"))
            .expect("ser")
            .contains("code"));
    }

    #[test]
    fn response_omits_none_fields() {
        let ok = serde_json::to_string(&BrokerResponse::ok(serde_json::json!({"text": "x"})))
            .expect("ser");
        assert!(ok.contains("\"ok\":true"));
        assert!(!ok.contains("error"));
        let err = serde_json::to_string(&BrokerResponse::err("nope")).expect("ser");
        assert!(err.contains("\"ok\":false"));
        assert!(!err.contains("result"));
    }
}
