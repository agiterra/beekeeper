//! Just-in-time access requests for sessions.
//!
//! An agent that lacks standing "Agents" consent for a session can ask the
//! owner, from chat, to either run a single command or enable full control.
//! The broker registers the request here, the desktop UI shows it, and the
//! owner's decision wakes the waiting broker call. This is the in-the-loop
//! counterpart to the pre-granted, default-off agent consent
//! (`session_broker::consent`): consent answers "may agents drive this?" ahead
//! of time; an access request answers "may this agent do this, right now?".
//!
//! Module-owned static store (same rationale as `manager`): the broker reaches
//! it without a Tauri `State` handle. Requests are ephemeral — a pending one
//! is dropped if the app exits.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

/// Frontend event fired when an agent opens an access request.
const REQUEST_EVENT: &str = "shell-access-request";
/// Frontend event fired when a request is resolved or withdrawn (so any open
/// prompt for it can dismiss).
const RESOLVED_EVENT: &str = "shell-access-request-resolved";

/// The owner's answer to an access request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Run the requested command this once; do not change standing consent.
    Once,
    /// Enable agent consent for the session (persisted), then honor the request.
    Full,
    /// Refuse.
    Deny,
}

impl Decision {
    pub fn parse(s: &str) -> Option<Decision> {
        match s {
            "once" => Some(Decision::Once),
            "full" => Some(Decision::Full),
            "deny" => Some(Decision::Deny),
            _ => None,
        }
    }
}

/// A pending request, as shown to the owner.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessRequest {
    pub id: String,
    pub workspace_id: String,
    /// Human label for the session (title), for the prompt.
    pub session_title: Option<String>,
    /// The single command the agent wants to run, if it named one.
    pub command: Option<String>,
    /// The agent's stated reason, if any.
    pub reason: Option<String>,
    /// The requesting agent's npub, for display/audit.
    pub caller: Option<String>,
}

struct Pending {
    request: AccessRequest,
    responder: oneshot::Sender<Decision>,
}

static REQUESTS: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, Pending>> {
    REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a request and return its id + a receiver the broker awaits. Emits
/// the request to the frontend. The id is derived from the workspace + caller +
/// a monotonic-ish salt so it is stable within a call but unique across calls.
pub fn register(
    app: &AppHandle,
    request: AccessRequest,
) -> Result<oneshot::Receiver<Decision>, String> {
    let (tx, rx) = oneshot::channel();
    {
        let mut map = store()
            .lock()
            .map_err(|_| "access-request store lock poisoned".to_string())?;
        map.insert(
            request.id.clone(),
            Pending {
                request: request.clone(),
                responder: tx,
            },
        );
    }
    let _ = app.emit(REQUEST_EVENT, &request);
    Ok(rx)
}

/// Resolve a pending request with the owner's decision. Returns an error if no
/// such request is pending (already answered or timed out).
pub fn resolve(app: &AppHandle, id: &str, decision: Decision) -> Result<(), String> {
    let pending = {
        let mut map = store()
            .lock()
            .map_err(|_| "access-request store lock poisoned".to_string())?;
        map.remove(id)
    };
    let Some(pending) = pending else {
        return Err(format!("no pending access request {id}"));
    };
    let _ = app.emit(RESOLVED_EVENT, serde_json::json!({ "id": id }));
    pending
        .responder
        .send(decision)
        .map_err(|_| "the agent stopped waiting for this request".to_string())
}

/// Drop a pending request without answering (e.g. the broker call timed out).
pub fn cancel(app: &AppHandle, id: &str) {
    if let Ok(mut map) = store().lock() {
        map.remove(id);
    }
    let _ = app.emit(RESOLVED_EVENT, serde_json::json!({ "id": id }));
}

/// The currently-pending requests, for a UI that mounts after one arrived.
pub fn list() -> Vec<AccessRequest> {
    store()
        .lock()
        .map(|map| map.values().map(|p| p.request.clone()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_parse_round_trips_known_values() {
        assert_eq!(Decision::parse("once"), Some(Decision::Once));
        assert_eq!(Decision::parse("full"), Some(Decision::Full));
        assert_eq!(Decision::parse("deny"), Some(Decision::Deny));
        assert_eq!(Decision::parse("maybe"), None);
    }
}
