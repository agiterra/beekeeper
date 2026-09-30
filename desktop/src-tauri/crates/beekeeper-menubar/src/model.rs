//! What the menu says, derived from what the host answered.
//!
//! Pure on purpose. Every honesty rule this app has to keep is a rule about
//! *text* — which words appear over which state — and text derived by a pure
//! function can be asserted. The alternative is a rule that lives in a comment
//! above an `NSMenu` call and is checked by looking at a menu bar.
//!
//! # The rules
//!
//! 1. **Never "No agents are running" over a host that did not answer.** The
//!    desktop app's tray said that whenever its list was empty, which was fine
//!    when the app *was* the agent manager. It is now a claim this process
//!    usually cannot make.
//! 2. **"Not installed" and "not running" are different sentences.** The
//!    socket's absence cannot tell them apart; the login registration can.
//! 3. **Elapsed times are computed here from an absolute start.** The host
//!    sends `startedAtMs`, never a formatted duration — a formatted duration
//!    is true only at the instant it was written, and it is the whole reason
//!    a ticking clock would otherwise need a poll per second.
//! 4. **The host's own words win.** When it refuses to start a provider it
//!    says why, in a sentence written for a person; this app shows that rather
//!    than a category it invented.

use beekeeper_host::protocol::Status;
use beekeeper_host::state::ProviderChildState;
use beekeeper_tray::{format_elapsed, TrayAgentActivity};

/// What one poll learned.
#[derive(Debug, Clone)]
pub enum HostView {
    /// The host answered.
    Reachable(Box<Status>),
    /// It did not. `installed` is the login registration, which is the only
    /// thing that distinguishes *not installed* from *installed, not running*.
    Unreachable {
        installed: bool,
        /// The client's own sentence — "not running" vs "not responding".
        reason: String,
    },
}

/// Everything the menu needs, and nothing about how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// The disabled row at the top. Always a true sentence about this machine.
    pub header: String,
    pub running: Vec<TrayAgentActivity>,
    pub recent: Vec<TrayAgentActivity>,
    /// Whether the host can be asked to do things. False when it did not
    /// answer — a control that cannot work must not be offered.
    pub host_reachable: bool,
    /// Whether a provider is live, so "Stop" is offered and "Start" is not.
    pub provider_live: bool,
    /// Facts to show as their own disabled rows, below the sections. The
    /// host's `warnings`, plus anything wrong with the login registration.
    pub notices: Vec<String>,
}

/// Format a turn that began at `started_at_ms`, as of `now_ms`.
///
/// A start in the future — two machines' clocks, or a snapshot read across a
/// second boundary — reads as `0s` rather than panicking or showing a negative
/// duration.
fn elapsed_since(started_at_ms: i64, now_ms: i64) -> String {
    let millis = now_ms.saturating_sub(started_at_ms).max(0);
    format_elapsed(std::time::Duration::from_millis(millis as u64))
}

/// The display name for a coding session.
///
/// A seat's role when it has one, else its runtime, else the word `session`.
/// **Not a channel name**: the host reads its sessions out of the provider's
/// state file, which records a channel *id* (a UUID) and no name — names live
/// in kind:39000 metadata on the relay, and this process deliberately holds no
/// relay connection. So the row says what is knowable here and clicking it
/// opens Beekeeper, which can resolve the name.
fn session_label(session: &beekeeper_host::sessions::LiveSession) -> String {
    session
        .role
        .as_deref()
        .filter(|role| !role.trim().is_empty())
        .or(session
            .runtime
            .as_deref()
            .filter(|runtime| !runtime.trim().is_empty()))
        .unwrap_or("session")
        .to_string()
}

/// A short, stable stand-in for a channel whose name this process cannot know.
fn short_channel(channel_id: &str) -> String {
    let head: String = channel_id.chars().take(8).collect();
    if head.is_empty() {
        "session".to_string()
    } else {
        head
    }
}

/// Build the menu's content.
pub fn model(view: &HostView, now_ms: i64) -> MenuModel {
    match view {
        HostView::Unreachable { installed, reason } => MenuModel {
            // Rule 1 and 2: the words say which silence this is, and never
            // claim anything about agents.
            header: if *installed {
                "Agent host: installed, not running".to_string()
            } else {
                "Agent host: not installed".to_string()
            },
            running: Vec::new(),
            recent: Vec::new(),
            host_reachable: false,
            provider_live: false,
            notices: vec![reason.clone()],
        },
        HostView::Reachable(status) => {
            let mut running = Vec::new();
            let mut recent = Vec::new();

            // Coding sessions: the host's own knowledge. Only turns that are
            // actually open become rows — a session sitting idle is not work
            // in flight, and showing it with a ticking clock would be a lie
            // about what the machine is doing.
            for session in &status.sessions.sessions {
                let Some(started) = session.turn_started_at_ms else {
                    continue;
                };
                running.push(TrayAgentActivity {
                    activity_id: format!("session:{}", session.session_id),
                    agent_name: session_label(session),
                    agent_pubkey: session.actor.clone().unwrap_or_default(),
                    channel_id: session.channel_id.clone().unwrap_or_default(),
                    channel_name: session
                        .channel_id
                        .as_deref()
                        .map(short_channel)
                        .unwrap_or_else(|| "session".to_string()),
                    elapsed: elapsed_since(started, now_ms),
                });
            }

            // Managed agents: the app's contribution, under lease. Absent when
            // no app is running, which is correct — those agents died with it.
            for row in &status.app_activity {
                let activity = TrayAgentActivity {
                    activity_id: row.activity_id.clone(),
                    agent_name: row.agent_name.clone(),
                    agent_pubkey: row.agent_pubkey.clone(),
                    channel_id: row.channel_id.clone(),
                    channel_name: row.channel_name.clone(),
                    elapsed: elapsed_since(row.started_at_ms, now_ms),
                };
                if row.recent {
                    recent.push(activity);
                } else {
                    running.push(activity);
                }
            }

            let provider_live = status.provider.live_pid().is_some();
            let mut notices: Vec<String> = status
                .warnings
                .iter()
                .map(|warning| warning.message.clone())
                .collect();
            if let Some(unavailable) = &status.sessions.unavailable {
                notices.push(unavailable.clone());
            }
            if !status.app_activity_leased {
                // Said out loud, because the absence is otherwise unreadable:
                // a machine with agents running under a closed app looks
                // identical to one with none.
                notices.push(
                    "Beekeeper is not running, so agents it manages are not listed here"
                        .to_string(),
                );
            }

            MenuModel {
                header: header_for(&status.provider, running.len()),
                running,
                recent,
                host_reachable: true,
                provider_live,
                notices,
            }
        }
    }
}

/// The header sentence for a reachable host.
///
/// Rule 4: for every state the host refuses in, its own message is the header.
/// Those sentences name a cause and a fix — a missing key file, another host
/// holding the lock — and a category invented here ("stopped") would lose both.
fn header_for(provider: &ProviderChildState, running_rows: usize) -> String {
    match provider {
        ProviderChildState::Live { .. } => match running_rows {
            0 => "Agent host: running · idle".to_string(),
            1 => "Agent host: running · 1 agent".to_string(),
            n => format!("Agent host: running · {n} agents"),
        },
        ProviderChildState::Backoff { failures, .. } => {
            format!("Agent host: restarting (attempt {failures})")
        }
        ProviderChildState::NotSupervised => "Agent host: running · no provider".to_string(),
        other => format!("Agent host: {}", other.message()),
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
