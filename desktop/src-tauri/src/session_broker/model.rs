//! The broker's session wire shapes, as `bee session list` serializes them.
//!
//! Extracted from the cmux integration this broker originally multiplexed
//! (buzz-old `cmux/model.rs`); built-in shells are the only backend here, but
//! the **camelCase JSON is a wire contract** — the CLI's `SessionSummary` and
//! agents parsing `bee session list` depend on these exact field names, so
//! keep them stable even though the "workspace" vocabulary is historical.

use serde::Serialize;

/// Coarse, honest read of a session's activity. Distinguishes only the case a
/// caller most needs — a session **blocked waiting on a human** — from
/// everything else. Kept deliberately conservative: unknown state reads as
/// `active`, never as blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionActivity {
    /// The session posted a status indicating it is waiting on input.
    #[allow(dead_code)]
    NeedsInput,
    /// Running or idle with no blocked-on-input signal.
    Active,
}

/// A session surfaced to `bee session list`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerSession {
    /// The stable session handle for later read/send/exec verbs. Built-in
    /// shells use `shell:<sessionId>`.
    pub workspace_id: String,
    pub window_id: Option<String>,
    pub title: Option<String>,
    pub current_directory: Option<String>,
    /// The session's live status line verbatim, when present.
    pub status_line: Option<String>,
    /// Unix time (seconds) the status line was posted, for staleness display.
    pub status_line_at: Option<f64>,
    pub activity: SessionActivity,
    pub is_selected: bool,
    pub has_unread: bool,
    pub is_pinned: bool,
    pub last_activity_at: Option<f64>,
    pub terminals: Vec<BrokerTerminal>,
    /// Whether agents are currently allowed to drive this session (the
    /// "Agents" consent). Lets a caller tell up front whether a write will be
    /// permitted instead of discovering it by a refused send. Set by the
    /// session broker's list handler, which knows the consent store.
    pub agents_enabled: bool,
    /// The session's current input line (pending typed text under the
    /// cursor), when the backend can surface it (built-in shell).
    pub input_line: Option<String>,
}

/// A terminal (surface) inside a session.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerTerminal {
    pub surface_id: String,
    pub title: Option<String>,
    pub current_directory: Option<String>,
    pub is_focused: bool,
    pub is_ready: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_session_serializes_camel_case_wire_shape() {
        let session = BrokerSession {
            workspace_id: "shell:abc".into(),
            window_id: None,
            title: Some("zsh".into()),
            current_directory: Some("/tmp".into()),
            status_line: None,
            status_line_at: None,
            activity: SessionActivity::Active,
            is_selected: false,
            has_unread: false,
            is_pinned: false,
            last_activity_at: Some(1.0),
            terminals: vec![BrokerTerminal {
                surface_id: "shell:abc".into(),
                title: Some("zsh".into()),
                current_directory: Some("/tmp".into()),
                is_focused: true,
                is_ready: true,
            }],
            agents_enabled: true,
            input_line: Some("ls".into()),
        };
        let json = serde_json::to_string(&session).expect("serialize");
        for field in [
            "\"workspaceId\":\"shell:abc\"",
            "\"currentDirectory\":\"/tmp\"",
            "\"activity\":\"active\"",
            "\"agentsEnabled\":true",
            "\"inputLine\":\"ls\"",
            "\"surfaceId\":\"shell:abc\"",
            "\"lastActivityAt\":1.0",
        ] {
            assert!(json.contains(field), "missing {field} in {json}");
        }
    }
}
