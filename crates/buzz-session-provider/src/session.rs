//! Per-session actors.
//!
//! One actor owns one session, and in the ACP binding one actor owns exactly one
//! `claude-agent-acp` subprocess. That one-to-one shape is forced, not chosen:
//! [`buzz_acp::acp::AcpClient`] permits a single in-flight `session/prompt` per
//! process, so sharing a process across sessions would serialize unrelated
//! operators behind each other. It also buys per-session working directories and
//! crash isolation for free.
//!
//! This commit lands the actor's shape, mailbox, and lifecycle so the command
//! loop is complete and testable; the subprocess itself arrives with the ACP
//! binding.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;
use uuid::Uuid;

use buzz_core::coding_session_command::CodingSessionTarget;

/// Mailbox depth for one session. Turns beyond this are refused rather than
/// buffered without bound — an operator who cannot see a queue cannot reason
/// about one.
pub const SESSION_MAILBOX_DEPTH: usize = 8;

/// Everything needed to bring one session up.
#[derive(Debug, Clone)]
pub struct CreateRequest {
    /// The generation being created.
    pub target: CodingSessionTarget,
    /// Channel this session publishes into.
    pub channel_id: Uuid,
    /// Host-local working directory for the agent.
    pub cwd: PathBuf,
    /// Operator-facing title, forwarded as `_meta.sessionTitle`.
    pub title: Option<String>,
    /// Requested model, or `None` to let the adapter decide.
    pub model: Option<String>,
    /// ACP adapter binary to spawn.
    pub agent_command: String,
    /// Per-turn silence budget.
    pub idle_timeout: Duration,
    /// Per-turn wall-clock ceiling.
    pub max_turn_duration: Duration,
}

/// Why a session could not be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateFailure {
    /// Receipt error code to publish.
    pub code: &'static str,
    /// Operator-facing detail.
    pub message: String,
}

/// Work delivered to a live session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommand {
    /// Run a turn.
    Turn {
        /// The command that requested it.
        command_id: String,
        /// Prompt text.
        text: String,
    },
    /// Cancel the in-flight turn.
    Interrupt {
        /// The command that requested it.
        command_id: String,
    },
    /// Retire the session and release its resources.
    Shutdown,
}

/// Why a command could not be delivered to a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliverError {
    /// The actor's mailbox is full.
    QueueFull,
    /// The actor is gone.
    Gone,
}

/// Handle to one live session actor.
#[derive(Debug)]
pub struct SessionHandle {
    session_id: String,
    tx: mpsc::Sender<SessionCommand>,
}

impl SessionHandle {
    /// The producer-minted session id this handle addresses.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Deliver work without blocking. A full mailbox is reported, never awaited:
    /// blocking here would stall the relay read loop behind one busy session.
    pub fn deliver(&self, command: SessionCommand) -> Result<(), DeliverError> {
        self.tx.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => DeliverError::QueueFull,
            mpsc::error::TrySendError::Closed(_) => DeliverError::Gone,
        })
    }

    /// Whether the actor is still running.
    pub fn is_live(&self) -> bool {
        !self.tx.is_closed()
    }
}

/// Registry of live session actors.
#[derive(Debug, Default)]
pub struct SessionManager {
    live: HashMap<String, SessionHandle>,
}

impl SessionManager {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bring up a session actor for `request`.
    pub async fn create(&mut self, request: CreateRequest) -> Result<(), CreateFailure> {
        let session_id = request.target.session_id.clone();
        let (tx, rx) = mpsc::channel(SESSION_MAILBOX_DEPTH);
        tokio::spawn(run_session(request, rx));
        self.live
            .insert(session_id.clone(), SessionHandle { session_id, tx });
        Ok(())
    }

    /// Handle for a live session, if it is still running.
    pub fn handle(&self, session_id: &str) -> Option<&SessionHandle> {
        self.live.get(session_id)
    }

    /// Ask a session to retire and forget it.
    pub fn shutdown(&mut self, session_id: &str) {
        if let Some(handle) = self.live.remove(session_id) {
            let _ = handle.deliver(SessionCommand::Shutdown);
        }
    }

    /// Number of live actors.
    pub fn live_count(&self) -> usize {
        self.live.len()
    }

    /// Drop handles whose actors have exited.
    pub fn reap(&mut self) -> Vec<String> {
        let dead: Vec<String> = self
            .live
            .iter()
            .filter(|(_, handle)| !handle.is_live())
            .map(|(session_id, _)| session_id.clone())
            .collect();
        for session_id in &dead {
            self.live.remove(session_id);
        }
        dead
    }
}

/// The session actor loop.
///
/// Placeholder body: it drains its mailbox and exits on shutdown, which is
/// exactly the lifecycle the ACP binding will keep once it owns a subprocess.
async fn run_session(request: CreateRequest, mut rx: mpsc::Receiver<SessionCommand>) {
    tracing::info!(
        target: "csp::session",
        session_id = %request.target.session_id,
        cwd = %request.cwd.display(),
        "session actor started"
    );
    while let Some(command) = rx.recv().await {
        match command {
            SessionCommand::Shutdown => break,
            other => tracing::debug!(
                target: "csp::session",
                session_id = %request.target.session_id,
                "session actor received {other:?}"
            ),
        }
    }
    tracing::info!(
        target: "csp::session",
        session_id = %request.target.session_id,
        "session actor stopped"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(session_id: &str) -> CreateRequest {
        CreateRequest {
            target: CodingSessionTarget {
                driver: "claude-agent-acp".into(),
                instance_id: "instance-1".into(),
                session_id: session_id.into(),
                generation: 1,
            },
            channel_id: Uuid::nil(),
            cwd: PathBuf::from("/tmp"),
            title: None,
            model: None,
            agent_command: "claude-agent-acp".into(),
            idle_timeout: Duration::from_secs(900),
            max_turn_duration: Duration::from_secs(7200),
        }
    }

    #[tokio::test]
    async fn a_created_session_accepts_work_until_it_is_shut_down() {
        let mut manager = SessionManager::new();
        manager.create(request("s1")).await.expect("create");
        assert_eq!(manager.live_count(), 1);

        let handle = manager.handle("s1").expect("handle");
        handle
            .deliver(SessionCommand::Turn {
                command_id: "turn-1".into(),
                text: "go".into(),
            })
            .expect("deliver");

        manager.shutdown("s1");
        assert_eq!(manager.live_count(), 0);
        assert!(manager.handle("s1").is_none());
    }

    /// Blocking the relay read loop behind one busy session would stall every
    /// other session on the box, so an overfull mailbox is reported instead.
    #[tokio::test]
    async fn a_full_mailbox_is_reported_rather_than_awaited() {
        let (tx, _rx) = mpsc::channel(1);
        let handle = SessionHandle {
            session_id: "s1".into(),
            tx,
        };
        handle
            .deliver(SessionCommand::Interrupt {
                command_id: "a".into(),
            })
            .expect("first fits");
        assert_eq!(
            handle.deliver(SessionCommand::Interrupt {
                command_id: "b".into()
            }),
            Err(DeliverError::QueueFull)
        );
    }

    #[tokio::test]
    async fn delivering_to_a_dead_actor_reports_rather_than_hangs() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let handle = SessionHandle {
            session_id: "s1".into(),
            tx,
        };
        assert_eq!(
            handle.deliver(SessionCommand::Shutdown),
            Err(DeliverError::Gone)
        );
        assert!(!handle.is_live());
    }
}
