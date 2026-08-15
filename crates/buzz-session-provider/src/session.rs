//! Per-session actors, each owning one `claude-agent-acp` subprocess.
//!
//! One actor owns one session and one process. That one-to-one shape is forced,
//! not chosen: [`AcpClient`] permits a single in-flight `session/prompt` per
//! process, so sharing a process across sessions would serialize unrelated
//! operators behind each other. It also buys per-session working directories and
//! crash isolation for free.
//!
//! The actor never publishes. It reports [`SessionEvent`]s to the provider loop,
//! which owns durable state and the outbox — one writer, so a sequence counter
//! can never be handed out twice.
//!
//! # Cancelling an in-flight turn
//!
//! `session_prompt_*` borrows the client for the whole turn, so the interrupt
//! path drops the prompt future to release that borrow before calling
//! `cancel_with_cleanup_grace`. This mirrors the harness's own control path
//! (`pool.rs`, the `control_rx` arm): dropping the future leaves the client's
//! `last_prompt_id` set, which is exactly what the cleanup drain needs.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use tokio::sync::broadcast;

use buzz_acp::acp::{AcpClient, AcpError, ModelSwitchMethod, StopReason};
use buzz_acp::observer::{context_for, ObserverEvent, ObserverHandle};
use buzz_acp::TurnUsage;
use buzz_core::coding_session_command::CodingSessionTarget;

use crate::payload::{PROVIDER_AUTH_REQUIRED, PROVIDER_UNAVAILABLE};
use crate::transcript::TranscriptTranslator;

/// Mailbox depth for one session. Turns beyond this are refused rather than
/// buffered without bound — an operator who cannot see a queue cannot reason
/// about one.
pub const SESSION_MAILBOX_DEPTH: usize = 8;
/// Turns held while another is running. Overflow becomes a visible dropped-turn
/// item rather than silent backlog.
pub const SESSION_QUEUE_DEPTH: usize = 8;
/// Ceiling on `spawn` + `initialize` + `session/new`. A create that has not
/// answered by now is an unavailable provider, not a slow one.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a cancelled agent has to acknowledge before the drain gives up.
pub const CANCEL_GRACE: Duration = Duration::from_secs(30);

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
    /// Previously persisted ACP session id to reattach, or `None` for a fresh
    /// provider session. This value is host-private.
    pub resume_cursor: Option<String>,
    /// ACP adapter binary to spawn.
    pub agent_command: String,
    /// Adapter argv after the command (e.g. `["acp"]` for goose).
    pub agent_args: Vec<String>,
    /// Extra environment for the adapter spawn (e.g. `CLAUDE_CODE_EXECUTABLE`).
    pub agent_env: Vec<(String, String)>,
    /// Per-turn silence budget.
    pub idle_timeout: Duration,
    /// Per-turn wall-clock ceiling.
    pub max_turn_duration: Duration,
    /// Idle window before the subprocess is reclaimed.
    pub idle_shutdown: Duration,
    /// Whether `agent_thought_chunk` updates become `reasoning` items.
    pub include_thoughts: bool,
}

/// Why a session could not be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateFailure {
    /// Receipt error code to publish.
    pub code: &'static str,
    /// Operator-facing detail.
    pub message: String,
}

/// What a successful create produced.
#[derive(Clone)]
pub struct SessionStartup {
    /// The adapter's own session id. Internal — never leaves this process.
    pub acp_session_id: String,
    /// Wire tap carrying every JSON-RPC frame this session's agent emits.
    pub observer: ObserverHandle,
    /// Effective model, when one could be established.
    pub model: Option<String>,
    /// Whether the adapter recovered its prior context.
    pub continuity: SessionContinuity,
}

/// How an ACP session was opened for this Buzz execution generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionContinuity {
    /// A brand-new Buzz execution opened a brand-new ACP session.
    Fresh,
    /// `session/resume` reattached without replaying history.
    Resumed,
    /// `session/load` reattached; replay frames were intentionally not ingested.
    Loaded,
    /// Reattachment was unavailable or rejected, so the generation has fresh
    /// provider context and must say so honestly.
    RestartedWithoutContext {
        /// Stable, non-sensitive explanation suitable for a transcript status.
        reason: &'static str,
    },
}

// Hand-written because `ObserverHandle` is a broadcast handle with no `Debug`,
// and the field carries no information worth printing anyway.
impl std::fmt::Debug for SessionStartup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionStartup")
            .field("model", &self.model)
            .field("continuity", &self.continuity)
            .finish_non_exhaustive()
    }
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

/// How a turn ended.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnOutcome {
    /// The agent returned a stop reason.
    Completed {
        /// The reason the agent gave.
        stop_reason: StopReason,
    },
    /// The operator interrupted it.
    Cancelled,
    /// The turn could not be completed.
    Failed {
        /// Operator-facing detail.
        message: String,
        /// Whether the agent process is gone, so the session cannot continue.
        agent_gone: bool,
    },
}

/// Why a session actor stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitReason {
    /// Nothing arrived within the idle-shutdown window.
    Idle,
    /// The provider asked it to stop.
    Requested,
    /// The agent process went away.
    AgentGone(String),
}

/// Something the provider loop must fold into state and publish.
///
/// Mostly reports from a session actor. [`SessionEvent::WorktreeObserved`] is
/// the exception: it comes from a bounded observation task the provider itself
/// spawned, and rides this queue rather than a parallel one so the loop keeps a
/// single ordered inbox with a single shutdown rule.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// A turn began.
    TurnStarted {
        /// Which session.
        session_id: String,
        /// Producer-minted turn id carried on every item of this turn.
        turn_id: String,
        /// The command that opened it.
        command_id: String,
        /// The prompt text, so the provider can record it as a `user_prompt`.
        text: String,
    },
    /// A turn ended.
    TurnFinished {
        /// Which session.
        session_id: String,
        /// The turn that ended.
        turn_id: String,
        /// How it ended.
        outcome: TurnOutcome,
        /// Wall-clock duration.
        duration_ms: u64,
        /// Usage, when the adapter reported any.
        usage: Option<Box<TurnUsage>>,
    },
    /// Projected transcript items, in the order they were produced.
    ///
    /// Sent from the actor's own task, interleaved with `TurnStarted` and
    /// `TurnFinished`, so the provider sees the turn's items in narrative order
    /// rather than in whatever order two tasks happened to race.
    TranscriptItems {
        /// Which session.
        session_id: String,
        /// The turn these items belong to.
        turn_id: String,
        /// The items.
        items: Vec<serde_json::Value>,
    },
    /// A turn was refused because the session's queue was full.
    TurnDropped {
        /// Which session.
        session_id: String,
        /// The command that was refused.
        command_id: String,
    },
    /// The actor stopped and the subprocess is gone.
    Exited {
        /// Which session.
        session_id: String,
        /// Why.
        reason: ExitReason,
    },
    /// A bounded look at a session's working directory finished.
    ///
    /// Produced by a task the provider spawned, never by an actor: the probe
    /// runs `git` subprocesses, and awaiting them on the loop would delay every
    /// *other* session's transcript delivery and the outbox flush.
    WorktreeObserved {
        /// Which session was observed.
        session_id: String,
        /// Monotonically increasing per-session sequence number assigned when
        /// this probe was launched (see [`crate::Provider::spawn_git_probe`]).
        /// Lets the provider fence out a result from a probe that a later probe
        /// for the same session has already superseded, regardless of which
        /// one's `git` subprocess happens to finish first.
        generation: u64,
        /// What git reported — every field optional, nothing fatal.
        observed: crate::git_probe::GitProbe,
    },
}

/// Handle to one live session actor.
#[derive(Debug)]
pub struct SessionHandle {
    session_id: String,
    tx: mpsc::Sender<SessionCommand>,
    shutdown: watch::Sender<bool>,
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

    /// Signal durable retirement on a control path that cannot be blocked by
    /// the bounded turn mailbox.
    fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

/// Registry of live session actors.
pub struct SessionManager {
    live: HashMap<String, SessionHandle>,
    events: mpsc::Sender<SessionEvent>,
}

impl SessionManager {
    /// A registry that reports actor events to `events`.
    pub fn new(events: mpsc::Sender<SessionEvent>) -> Self {
        Self {
            live: HashMap::new(),
            events,
        }
    }

    /// Spawn the adapter, open an ACP session, and start an actor for it.
    ///
    /// The whole startup is awaited so the caller can answer the create command
    /// with a receipt that reflects reality rather than an optimistic guess.
    pub async fn create(
        &mut self,
        request: CreateRequest,
    ) -> Result<SessionStartup, CreateFailure> {
        let observer = ObserverHandle::in_process();
        let started = tokio::time::timeout(STARTUP_TIMEOUT, start_agent(&request, &observer)).await;
        let (client, startup) = match started {
            Ok(Ok(started)) => started,
            Ok(Err(failure)) => return Err(failure),
            Err(_) => {
                return Err(CreateFailure {
                    code: PROVIDER_UNAVAILABLE,
                    message: format!(
                        "{} did not open a session within {}s",
                        request.agent_command,
                        STARTUP_TIMEOUT.as_secs()
                    ),
                })
            }
        };

        let session_id = request.target.session_id.clone();
        let (tx, rx) = mpsc::channel(SESSION_MAILBOX_DEPTH);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let actor = SessionActor {
            client,
            acp_session_id: startup.acp_session_id.clone(),
            session_id: session_id.clone(),
            idle_timeout: request.idle_timeout,
            max_turn_duration: request.max_turn_duration,
            idle_shutdown: request.idle_shutdown,
            events: self.events.clone(),
            observer,
            translator: TranscriptTranslator::new(request.include_thoughts),
        };
        tokio::spawn(actor.run(rx, shutdown_rx));
        self.live.insert(
            session_id.clone(),
            SessionHandle {
                session_id,
                tx,
                shutdown,
            },
        );
        Ok(startup)
    }

    /// Handle for a live session, if it is still running.
    pub fn handle(&self, session_id: &str) -> Option<&SessionHandle> {
        self.live.get(session_id)
    }

    /// Ask a session to retire and forget it.
    pub fn shutdown(&mut self, session_id: &str) {
        if let Some(handle) = self.live.remove(session_id) {
            handle.shutdown();
        }
    }

    /// Forget a session whose actor has already stopped.
    pub fn forget(&mut self, session_id: &str) {
        self.live.remove(session_id);
    }

    /// Number of live actors.
    pub fn live_count(&self) -> usize {
        self.live.len()
    }
}

/// Spawn the adapter and open one ACP session in `request.cwd`.
async fn start_agent(
    request: &CreateRequest,
    observer: &ObserverHandle,
) -> Result<(AcpClient, SessionStartup), CreateFailure> {
    // The adapter inherits this process's environment plus the descriptor's
    // per-runtime `agent_env` — that is how a runtime-specific CLI override
    // (e.g. `CLAUDE_CODE_EXECUTABLE`) reaches the adapter: the desktop host
    // resolves it once and every session gets the same answer.
    //
    // Minus the fence: the provider's signing key and the secrets it inherited
    // from the launching shell are removed first. See `crate::agent_fence`.
    let mut client = AcpClient::spawn_with_env_fence(
        &request.agent_command,
        &request.agent_args,
        &request.agent_env,
        false,
        &crate::agent_fence::FENCE,
    )
    .await
    .map_err(|error| classify_startup_error(&error, "spawn the agent"))?;

    client.set_observer(Some(observer.clone()), 0);
    client.set_observer_context(context_for(Some(request.channel_id), None, None));

    if let Err(failure) = client
        .initialize()
        .await
        .map_err(|error| classify_startup_error(&error, "initialize the agent"))
    {
        client.shutdown().await;
        return Err(failure);
    }

    let cwd = request.cwd.to_string_lossy().to_string();
    let opened = open_agent_session(&mut client, request, &cwd).await;
    let (response, continuity) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            let failure = classify_startup_error(&error, "open an agent session");
            client.shutdown().await;
            return Err(failure);
        }
    };

    let model = apply_model(&mut client, &response, request.model.as_deref()).await;
    Ok((
        client,
        SessionStartup {
            acp_session_id: response.session_id,
            observer: observer.clone(),
            model,
            continuity,
        },
    ))
}

async fn open_agent_session(
    client: &mut AcpClient,
    request: &CreateRequest,
    cwd: &str,
) -> Result<(buzz_acp::acp::SessionNewResponse, SessionContinuity), AcpError> {
    let Some(cursor) = request.resume_cursor.as_deref() else {
        let response = client
            .session_new_full(cwd, Vec::new(), None, request.title.as_deref())
            .await?;
        return Ok((response, SessionContinuity::Fresh));
    };

    let mut fallback_reason = "adapter does not advertise session resume or load";
    if client.session_resume_supported() {
        match client.session_resume_full(cursor, cwd, Vec::new()).await {
            Ok(response) => return Ok((response, SessionContinuity::Resumed)),
            Err(_) => {
                // Adapter errors are untrusted and may echo the opaque cursor.
                // Keep the durable resume identifier out of provider logs.
                tracing::warn!(target: "csp::session", "ACP session/resume rejected");
                fallback_reason = "adapter rejected session resume";
            }
        }
    }
    if client.session_load_supported() {
        match client.session_load_full(cursor, cwd, Vec::new()).await {
            Ok(response) => return Ok((response, SessionContinuity::Loaded)),
            Err(_) => {
                // See the resume branch above: an adapter error is not safe to log.
                tracing::warn!(target: "csp::session", "ACP session/load rejected");
                fallback_reason = if client.session_resume_supported() {
                    "adapter rejected session resume and load"
                } else {
                    "adapter rejected session load"
                };
            }
        }
    }

    let response = client
        .session_new_full(cwd, Vec::new(), None, request.title.as_deref())
        .await?;
    Ok((
        response,
        SessionContinuity::RestartedWithoutContext {
            reason: fallback_reason,
        },
    ))
}

/// Ask the adapter to use `desired`, best effort.
///
/// A model the adapter does not offer is reported as "not applied" rather than
/// failing the create: the session is perfectly usable on the adapter's own
/// default, and metadata says which model actually took effect.
async fn apply_model(
    client: &mut AcpClient,
    response: &buzz_acp::acp::SessionNewResponse,
    desired: Option<&str>,
) -> Option<String> {
    let desired = desired?;
    let method = buzz_acp::acp::resolve_model_switch_method(&response.raw, desired);
    let outcome = match method {
        Some(ModelSwitchMethod::ConfigOption {
            config_id,
            option_value,
        }) => {
            client
                .session_set_config_option(&response.session_id, &config_id, &option_value)
                .await
        }
        Some(ModelSwitchMethod::SetModel { model_id }) => {
            client
                .session_set_model(&response.session_id, &model_id)
                .await
        }
        None => {
            tracing::warn!(
                target: "csp::session",
                "agent does not offer model {desired} — using its default"
            );
            return None;
        }
    };
    match outcome {
        Ok(_) => Some(desired.to_owned()),
        Err(error) => {
            tracing::warn!(target: "csp::session", "model switch to {desired} failed: {error}");
            None
        }
    }
}

/// Map an ACP startup failure onto a receipt error code.
///
/// The auth split is what an operator acts on: `PROVIDER_AUTH_REQUIRED` means
/// "go log in", everything else means "the adapter is broken or absent". ACP
/// carries no dedicated auth error code, so the classification reads the
/// adapter's message. It is deliberately generous — misreading a genuine auth
/// failure as a generic outage sends the operator hunting a phantom bug, while
/// the reverse only shows a slightly wrong hint.
pub fn classify_startup_error(error: &AcpError, what: &str) -> CreateFailure {
    let message = error.to_string();
    let looks_like_auth = match error {
        AcpError::AgentError { message, .. } => mentions_auth(message),
        _ => false,
    };
    CreateFailure {
        code: if looks_like_auth {
            PROVIDER_AUTH_REQUIRED
        } else {
            PROVIDER_UNAVAILABLE
        },
        message: format!("could not {what}: {message}"),
    }
}

fn mentions_auth(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    [
        "auth",
        "login",
        "log in",
        "sign in",
        "credential",
        "api key",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

struct SessionActor {
    client: AcpClient,
    acp_session_id: String,
    session_id: String,
    idle_timeout: Duration,
    max_turn_duration: Duration,
    idle_shutdown: Duration,
    events: mpsc::Sender<SessionEvent>,
    observer: ObserverHandle,
    translator: TranscriptTranslator,
}

/// How the select loop around an in-flight prompt ended.
enum PromptInterruption {
    Completed(Result<StopReason, AcpError>),
    Interrupted,
    Shutdown,
}

impl SessionActor {
    async fn run(
        mut self,
        mut rx: mpsc::Receiver<SessionCommand>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        tracing::info!(
            target: "csp::session",
            session_id = %self.session_id,
            "session actor started"
        );
        let mut queued: VecDeque<(String, String)> = VecDeque::new();
        let mut reason = ExitReason::Requested;

        'actor: loop {
            if *shutdown.borrow() {
                break 'actor;
            }
            let next = match queued.pop_front() {
                Some(turn) => Some(SessionCommand::Turn {
                    command_id: turn.0,
                    text: turn.1,
                }),
                None => {
                    let idle = tokio::time::sleep(self.idle_shutdown);
                    tokio::pin!(idle);
                    tokio::select! {
                        biased;
                        changed = shutdown.changed() => {
                            let _ = changed;
                            break 'actor;
                        }
                        command = rx.recv() => command,
                        _ = &mut idle => {
                        reason = ExitReason::Idle;
                        break 'actor;
                        }
                    }
                }
            };
            match next {
                None | Some(SessionCommand::Shutdown) => break 'actor,
                Some(SessionCommand::Interrupt { command_id }) => {
                    tracing::debug!(
                        target: "csp::session",
                        session_id = %self.session_id,
                        %command_id,
                        "interrupt with no turn in flight — nothing to cancel"
                    );
                }
                Some(SessionCommand::Turn { command_id, text }) => {
                    if let Some(exit_reason) = self
                        .run_turn(&mut rx, &mut shutdown, &mut queued, command_id, text)
                        .await
                    {
                        reason = exit_reason;
                        break 'actor;
                    }
                }
            }
        }

        self.client.shutdown().await;
        let _ = self
            .events
            .send(SessionEvent::Exited {
                session_id: self.session_id.clone(),
                reason: reason.clone(),
            })
            .await;
        tracing::info!(
            target: "csp::session",
            session_id = %self.session_id,
            "session actor stopped: {reason:?}"
        );
    }

    /// Run one turn. Returns an exit reason when the actor must retire.
    async fn run_turn(
        &mut self,
        rx: &mut mpsc::Receiver<SessionCommand>,
        shutdown: &mut watch::Receiver<bool>,
        queued: &mut VecDeque<(String, String)>,
        command_id: String,
        text: String,
    ) -> Option<ExitReason> {
        let turn_id = Uuid::new_v4().to_string();
        let started = Instant::now();
        self.client.set_observer_context(context_for(
            None,
            Some(self.acp_session_id.clone()),
            Some(turn_id.clone()),
        ));
        let _ = self
            .events
            .send(SessionEvent::TurnStarted {
                session_id: self.session_id.clone(),
                turn_id: turn_id.clone(),
                command_id,
                text: text.clone(),
            })
            .await;

        // Subscribe before the prompt is written: a broadcast receiver only sees
        // what is sent after it exists, so subscribing afterwards would lose the
        // opening chunks of every turn.
        let mut frames = self.observer.subscribe();
        let opening = self.translator.begin_turn(&text);
        emit_items(&self.events, &self.session_id, &turn_id, opening).await;

        // The prompt future holds `&mut self.client` for the whole turn; it is
        // boxed so the interrupt path can drop it and get the client back.
        let mut prompt = Box::pin(self.client.session_prompt_with_idle_timeout(
            &self.acp_session_id,
            &text,
            self.idle_timeout,
            self.max_turn_duration,
        ));
        let interruption = loop {
            tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    let _ = changed;
                    break PromptInterruption::Shutdown
                },
                result = prompt.as_mut() => break PromptInterruption::Completed(result),
                frame = frames.recv() => {
                    let items = translate_frame(
                        &mut self.translator,
                        &self.acp_session_id,
                        frame,
                    );
                    emit_items(&self.events, &self.session_id, &turn_id, items).await;
                }
                command = rx.recv() => match command {
                    None | Some(SessionCommand::Shutdown) => break PromptInterruption::Shutdown,
                    Some(SessionCommand::Interrupt { .. }) => {
                        break PromptInterruption::Interrupted
                    }
                    Some(SessionCommand::Turn { command_id, text }) => {
                        if queued.len() >= SESSION_QUEUE_DEPTH {
                            let _ = self
                                .events
                                .send(SessionEvent::TurnDropped {
                                    session_id: self.session_id.clone(),
                                    command_id,
                                })
                                .await;
                        } else {
                            queued.push_back((command_id, text));
                        }
                    }
                },
            }
        };
        drop(prompt);

        // The prompt arm is polled first, so the agent's closing chunks can
        // still be sitting in the broadcast buffer when it resolves.
        loop {
            match frames.try_recv() {
                Ok(frame) => {
                    let items =
                        translate_frame(&mut self.translator, &self.acp_session_id, Ok(frame));
                    emit_items(&self.events, &self.session_id, &turn_id, items).await;
                }
                Err(broadcast::error::TryRecvError::Lagged(dropped)) => {
                    let items = vec![crate::payload::status_item(&format!(
                        "transcript_frames_dropped:{dropped}"
                    ))];
                    emit_items(&self.events, &self.session_id, &turn_id, items).await;
                }
                Err(_) => break,
            }
        }

        let requested_shutdown = matches!(interruption, PromptInterruption::Shutdown);
        let (outcome, agent_gone) = match interruption {
            PromptInterruption::Completed(Ok(stop_reason)) => {
                (TurnOutcome::Completed { stop_reason }, None)
            }
            PromptInterruption::Completed(Err(error)) => self.recover_from_turn_error(error).await,
            PromptInterruption::Interrupted | PromptInterruption::Shutdown => {
                match self
                    .client
                    .cancel_with_cleanup_grace(&self.acp_session_id, CANCEL_GRACE)
                    .await
                {
                    Ok(_) => (TurnOutcome::Cancelled, None),
                    Err(AcpError::AgentExited) => (
                        TurnOutcome::Failed {
                            message: "agent exited during cancellation".into(),
                            agent_gone: true,
                        },
                        Some("agent exited during cancellation".to_owned()),
                    ),
                    // The agent ignored the cancel but the operator's intent
                    // stands: report it cancelled, and let the drained process
                    // be reclaimed by the caller.
                    Err(error) => {
                        tracing::warn!(
                            target: "csp::session",
                            session_id = %self.session_id,
                            "cancel drain did not complete: {error}"
                        );
                        (TurnOutcome::Cancelled, None)
                    }
                }
            }
        };

        // Flush before the terminal item so the turn's prose and its final usage
        // snapshot are on the record ahead of the `result` the provider appends.
        let tail = self.translator.close_turn();
        emit_items(&self.events, &self.session_id, &turn_id, tail).await;

        let usage = self.client.take_turn_usage().map(Box::new);
        let _ = self
            .events
            .send(SessionEvent::TurnFinished {
                session_id: self.session_id.clone(),
                turn_id,
                outcome,
                duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                usage,
            })
            .await;
        self.client.set_observer_context(context_for(
            None,
            Some(self.acp_session_id.clone()),
            None,
        ));
        if requested_shutdown {
            Some(ExitReason::Requested)
        } else {
            agent_gone.map(ExitReason::AgentGone)
        }
    }

    /// Turn a failed prompt into an outcome, cancelling first when the agent is
    /// merely slow rather than dead.
    async fn recover_from_turn_error(&mut self, error: AcpError) -> (TurnOutcome, Option<String>) {
        let message = error.to_string();
        match error {
            AcpError::AgentExited | AcpError::Io(_) => (
                TurnOutcome::Failed {
                    message: message.clone(),
                    agent_gone: true,
                },
                Some(message),
            ),
            AcpError::IdleTimeout(_) | AcpError::HardTimeout { .. } => {
                // The turn is over as far as the operator is concerned, but the
                // agent may still be working; drain it so the next turn starts
                // from a quiet process.
                let _ = self
                    .client
                    .cancel_with_cleanup_grace(&self.acp_session_id, CANCEL_GRACE)
                    .await;
                (
                    TurnOutcome::Failed {
                        message,
                        agent_gone: false,
                    },
                    None,
                )
            }
            _ => (
                TurnOutcome::Failed {
                    message,
                    agent_gone: false,
                },
                None,
            ),
        }
    }
}

/// Forward translated items, skipping the send when there is nothing to say.
async fn emit_items(
    events: &mpsc::Sender<SessionEvent>,
    session_id: &str,
    turn_id: &str,
    items: Vec<serde_json::Value>,
) {
    if items.is_empty() {
        return;
    }
    let _ = events
        .send(SessionEvent::TranscriptItems {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            items,
        })
        .await;
}

/// Turn one observer frame into transcript items.
///
/// Only `acp_read` frames carrying a `session/update` notification matter; the
/// rest of the wire (requests, responses, writes) is machinery, not transcript.
/// `Closed` is unreachable while the actor lives — it owns the [`ObserverHandle`]
/// the frames are sent through — so it simply yields nothing.
fn translate_frame(
    translator: &mut TranscriptTranslator,
    acp_session_id: &str,
    frame: Result<ObserverEvent, broadcast::error::RecvError>,
) -> Vec<serde_json::Value> {
    let event = match frame {
        Ok(event) => event,
        Err(broadcast::error::RecvError::Lagged(dropped)) => {
            // Never silently: a gap in the record has to be visible in it.
            return vec![crate::payload::status_item(&format!(
                "transcript_frames_dropped:{dropped}"
            ))];
        }
        Err(broadcast::error::RecvError::Closed) => return Vec::new(),
    };
    if event.kind != "acp_read" {
        return Vec::new();
    }
    if event.payload.get("method").and_then(|m| m.as_str()) != Some("session/update") {
        return Vec::new();
    }
    let Some(params) = event.payload.get("params") else {
        return Vec::new();
    };
    // One process serves exactly one session, so this can only ever match — but
    // an adapter that multiplexed would otherwise cross two sessions' records.
    if let Some(session_id) = params.get("sessionId").and_then(|id| id.as_str()) {
        if session_id != acp_session_id {
            return Vec::new();
        }
    }
    match params.get("update") {
        Some(update) => translator.on_update(update),
        None => Vec::new(),
    }
}

/// Scripted stand-in agents, shared by this module's tests and the provider's.
///
/// The technique is buzz-acp's own (`acp.rs` spawns shell scripts that emit
/// NDJSON): a real subprocess speaking real JSON-RPC over real pipes, so nothing
/// about the transport is mocked away — only the model behind it.
#[cfg(test)]
pub(crate) mod testing {
    use std::io::Write;
    use std::path::Path;

    /// Write an executable shell script and return its path.
    pub(crate) fn fake_agent(dir: &Path, name: &str, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).expect("create script");
        write!(file, "#!/bin/bash\n{body}").expect("write script");
        drop(file);
        let mut perms = std::fs::metadata(&path).expect("stat").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
        path.to_string_lossy().into_owned()
    }

    /// A cooperative agent that first writes its own environment to
    /// `dump_path`, so a test can assert on what the child actually inherited
    /// rather than on what the spawn code appears to do.
    ///
    /// The path is baked into the script because the only other way to hand it
    /// to the child would be an environment variable — the very channel under
    /// test.
    pub(crate) fn env_dumping_agent(dump_path: &str) -> String {
        format!(
            r#"
env > "{dump_path}"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"acp-session-1"}}}}\n' "$id" ;;
  esac
done
"#
        )
    }

    /// A cooperative agent: answers `initialize` and `session/new`, streams a
    /// message chunk per prompt, then completes the turn.
    pub(crate) const GOOD_AGENT: &str = r#"
LAST_PROMPT=""
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acp-session-1"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      LAST_PROMPT="$id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp-session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"working"}}}}\n'
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
    *'"method":"session/cancel"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$LAST_PROMPT" ;;
  esac
done
"#;

    /// Advertises and accepts ACP `session/resume` for a saved cursor.
    pub(crate) const RESUMABLE_AGENT: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}}\n' "$id" ;;
    *'"method":"session/resume"'*'"sessionId":"saved-acp-session"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"saved-acp-session"}}\n' "$id" ;;
  esac
done
"#;

    /// Never answers the prompt, so the operator's interrupt is the only way out.
    pub(crate) const STALLING_AGENT: &str = r#"
LAST_PROMPT=""
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acp-session-1"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      LAST_PROMPT="$id" ;;
    *'"method":"session/cancel"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$LAST_PROMPT" ;;
  esac
done
"#;

    /// Opens a session, then dies the moment a turn starts.
    pub(crate) const DYING_AGENT: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acp-session-1"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      exit 1 ;;
  esac
done
"#;

    /// Refuses `session/new` with an authentication error.
    pub(crate) const UNAUTHENTICATED_AGENT: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2,"authMethods":[{"id":"claude-login"}]}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"Authentication required: run claude login"}}\n' "$id" ;;
  esac
done
"#;
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    fn request(command: String, cwd: &std::path::Path) -> CreateRequest {
        CreateRequest {
            target: CodingSessionTarget {
                driver: "claude-agent-acp".into(),
                instance_id: "instance-1".into(),
                session_id: "s1".into(),
                generation: 1,
            },
            channel_id: Uuid::nil(),
            cwd: cwd.to_path_buf(),
            title: Some("Ship it".into()),
            model: None,
            resume_cursor: None,
            agent_command: command,
            agent_args: Vec::new(),
            agent_env: Vec::new(),
            idle_timeout: Duration::from_secs(5),
            max_turn_duration: Duration::from_secs(10),
            idle_shutdown: Duration::from_secs(30),
            include_thoughts: true,
        }
    }

    async fn next_event(rx: &mut mpsc::Receiver<SessionEvent>) -> SessionEvent {
        tokio::time::timeout(Duration::from_secs(15), rx.recv())
            .await
            .expect("event within timeout")
            .expect("channel open")
    }

    /// The next lifecycle report, skipping the transcript items that stream
    /// alongside it — those are the translator's business, tested separately.
    async fn next_lifecycle_event(rx: &mut mpsc::Receiver<SessionEvent>) -> SessionEvent {
        loop {
            match next_event(rx).await {
                SessionEvent::TranscriptItems { .. } => continue,
                other => return other,
            }
        }
    }

    async fn collect_items(rx: &mut mpsc::Receiver<SessionEvent>) -> Vec<serde_json::Value> {
        let mut items = Vec::new();
        loop {
            match next_event(rx).await {
                SessionEvent::TranscriptItems { items: batch, .. } => items.extend(batch),
                SessionEvent::TurnFinished { .. } => return items,
                _ => {}
            }
        }
    }

    /// End-to-end proof of the credential fence, through the real create path
    /// and a real subprocess: the adapter writes its own environment to a file
    /// and the test reads what it actually got.
    ///
    /// The canaries are delivered through `agent_env` rather than by mutating
    /// this process's environment. `std::env::set_var` races every other
    /// test's `fork`/`exec` in a threaded runner, and it is not needed to
    /// prove the property — the fence is applied after all injection and
    /// removes unconditionally, so a key it drops here is a key it drops
    /// whatever the source. That the removal also reaches *inherited* values
    /// is asserted on the `Command` itself in `buzz-acp`.
    #[tokio::test]
    async fn the_adapter_never_receives_the_providers_credentials() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dump = dir.path().join("child-env");
        let agent = fake_agent(
            dir.path(),
            "env-dumping-agent",
            &env_dumping_agent(&dump.to_string_lossy()),
        );
        let (tx, _rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);

        let mut create = request(agent, dir.path());
        create.agent_env = vec![
            ("BUZZ_PRIVATE_KEY".into(), "nsec1canary".into()),
            ("BUZZ_AUTH_TAG".into(), "[\"canary\"]".into()),
            ("BUZZ_S3_SECRET_KEY".into(), "canary".into()),
            ("TYPESENSE_API_KEY".into(), "canary".into()),
            ("CLAUDE_CODE_EXECUTABLE".into(), "/opt/claude".into()),
        ];
        manager.create(create).await.expect("create");

        let dumped = std::fs::read_to_string(&dump).expect("the agent dumped its environment");
        for key in [
            "BUZZ_PRIVATE_KEY",
            "BUZZ_AUTH_TAG",
            "BUZZ_S3_SECRET_KEY",
            "TYPESENSE_API_KEY",
        ] {
            assert!(!dumped.contains(key), "{key} reached the agent:\n{dumped}");
        }
        assert!(
            dumped.contains("CLAUDE_CODE_EXECUTABLE"),
            "the fence took the per-runtime CLI override with it:\n{dumped}"
        );
        assert!(
            dumped.contains("PATH="),
            "the fence emptied the agent's environment:\n{dumped}"
        );
        manager.shutdown("s1");
    }

    #[tokio::test]
    async fn a_turn_runs_to_completion_and_reports_its_stop_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = fake_agent(dir.path(), "good-agent", GOOD_AGENT);
        let (tx, mut rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);

        let startup = manager
            .create(request(agent, dir.path()))
            .await
            .expect("create");
        assert_eq!(startup.acp_session_id, "acp-session-1");

        manager
            .handle("s1")
            .expect("handle")
            .deliver(SessionCommand::Turn {
                command_id: "turn-1".into(),
                text: "go".into(),
            })
            .expect("deliver");

        assert!(matches!(
            next_lifecycle_event(&mut rx).await,
            SessionEvent::TurnStarted { ref command_id, .. } if command_id == "turn-1"
        ));
        match next_lifecycle_event(&mut rx).await {
            SessionEvent::TurnFinished { outcome, .. } => assert_eq!(
                outcome,
                TurnOutcome::Completed {
                    stop_reason: StopReason::EndTurn
                }
            ),
            other => panic!("expected a finished turn, got {other:?}"),
        }
        manager.shutdown("s1");
    }

    #[tokio::test]
    async fn a_saved_cursor_uses_advertised_acp_resume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = fake_agent(dir.path(), "resumable-agent", RESUMABLE_AGENT);
        let (tx, _rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        let mut reattach = request(agent, dir.path());
        reattach.target.generation = 2;
        reattach.resume_cursor = Some("saved-acp-session".into());

        let startup = manager.create(reattach).await.expect("reattach");
        assert_eq!(startup.acp_session_id, "saved-acp-session");
        assert_eq!(startup.continuity, SessionContinuity::Resumed);
        manager.shutdown("s1");
    }

    #[tokio::test]
    async fn an_adapter_without_resume_starts_fresh_and_reports_discontinuity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = fake_agent(dir.path(), "good-agent", GOOD_AGENT);
        let (tx, _rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        let mut reattach = request(agent, dir.path());
        reattach.target.generation = 2;
        reattach.resume_cursor = Some("saved-acp-session".into());

        let startup = manager.create(reattach).await.expect("reattach");
        assert_eq!(startup.acp_session_id, "acp-session-1");
        assert!(matches!(
            startup.continuity,
            SessionContinuity::RestartedWithoutContext { .. }
        ));
        manager.shutdown("s1");
    }

    /// The translator is driven from the actor's own task, so the items of a
    /// turn arrive in narrative order rather than racing the turn's lifecycle
    /// reports. This is the end-to-end proof of that wiring: a real subprocess
    /// emits a real `session/update`, and it comes back as a projected item.
    #[tokio::test]
    async fn a_turns_updates_are_translated_into_ordered_transcript_items() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = fake_agent(dir.path(), "good-agent", GOOD_AGENT);
        let (tx, mut rx) = mpsc::channel(64);
        let mut manager = SessionManager::new(tx);
        manager
            .create(request(agent, dir.path()))
            .await
            .expect("create");
        manager
            .handle("s1")
            .expect("handle")
            .deliver(SessionCommand::Turn {
                command_id: "turn-1".into(),
                text: "go".into(),
            })
            .expect("deliver");

        let items = collect_items(&mut rx).await;
        let kinds: Vec<&str> = items
            .iter()
            .filter_map(|item| item["kind"].as_str())
            .collect();
        assert_eq!(kinds, vec!["user_prompt", "assistant_text"]);
        assert_eq!(items[0]["content"], "go");
        assert_eq!(items[1]["text"], "working");
        manager.shutdown("s1");
    }

    #[tokio::test]
    async fn an_interrupt_cancels_an_in_flight_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = fake_agent(dir.path(), "stalling-agent", STALLING_AGENT);
        let (tx, mut rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        manager
            .create(request(agent, dir.path()))
            .await
            .expect("create");

        let handle = manager.handle("s1").expect("handle");
        handle
            .deliver(SessionCommand::Turn {
                command_id: "turn-1".into(),
                text: "go".into(),
            })
            .expect("deliver");
        assert!(matches!(
            next_lifecycle_event(&mut rx).await,
            SessionEvent::TurnStarted { .. }
        ));
        handle
            .deliver(SessionCommand::Interrupt {
                command_id: "int-1".into(),
            })
            .expect("deliver interrupt");

        match next_lifecycle_event(&mut rx).await {
            SessionEvent::TurnFinished { outcome, .. } => {
                assert_eq!(outcome, TurnOutcome::Cancelled)
            }
            other => panic!("expected a cancelled turn, got {other:?}"),
        }
        manager.shutdown("s1");
    }

    /// An agent that dies mid-turn must produce a terminal outcome and take the
    /// session down with it — a session whose process is gone can serve no
    /// further turn, and pretending otherwise strands the operator.
    #[tokio::test]
    async fn an_agent_that_dies_mid_turn_ends_the_turn_and_the_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = fake_agent(dir.path(), "dying-agent", DYING_AGENT);
        let (tx, mut rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        manager
            .create(request(agent, dir.path()))
            .await
            .expect("create");
        manager
            .handle("s1")
            .expect("handle")
            .deliver(SessionCommand::Turn {
                command_id: "turn-1".into(),
                text: "go".into(),
            })
            .expect("deliver");

        assert!(matches!(
            next_lifecycle_event(&mut rx).await,
            SessionEvent::TurnStarted { .. }
        ));
        match next_lifecycle_event(&mut rx).await {
            SessionEvent::TurnFinished { outcome, .. } => assert!(
                matches!(
                    outcome,
                    TurnOutcome::Failed {
                        agent_gone: true,
                        ..
                    }
                ),
                "expected a fatal turn failure, got {outcome:?}"
            ),
            other => panic!("expected a finished turn, got {other:?}"),
        }
        match next_lifecycle_event(&mut rx).await {
            SessionEvent::Exited { reason, .. } => {
                assert!(matches!(reason, ExitReason::AgentGone(_)))
            }
            other => panic!("expected an exit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unauthenticated_agent_fails_the_create_with_a_recoverable_code() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = fake_agent(dir.path(), "unauth-agent", UNAUTHENTICATED_AGENT);
        let (tx, _rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        let failure = manager
            .create(request(agent, dir.path()))
            .await
            .expect_err("create should fail");
        assert_eq!(failure.code, PROVIDER_AUTH_REQUIRED);
        assert_eq!(manager.live_count(), 0);
    }

    #[tokio::test]
    async fn a_missing_agent_binary_fails_the_create_as_unavailable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        let failure = manager
            .create(request(
                dir.path()
                    .join("no-such-agent")
                    .to_string_lossy()
                    .into_owned(),
                dir.path(),
            ))
            .await
            .expect_err("create should fail");
        assert_eq!(failure.code, PROVIDER_UNAVAILABLE);
    }

    #[tokio::test]
    async fn an_idle_session_reclaims_its_subprocess() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent = fake_agent(dir.path(), "good-agent", GOOD_AGENT);
        let (tx, mut rx) = mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        let mut req = request(agent, dir.path());
        req.idle_shutdown = Duration::from_millis(150);
        manager.create(req).await.expect("create");

        match next_lifecycle_event(&mut rx).await {
            SessionEvent::Exited { reason, .. } => assert_eq!(reason, ExitReason::Idle),
            other => panic!("expected an idle exit, got {other:?}"),
        }
    }

    #[test]
    fn startup_errors_split_into_go_log_in_and_it_is_broken() {
        let auth = classify_startup_error(
            &AcpError::AgentError {
                code: -32000,
                message: "Authentication required".into(),
            },
            "open an agent session",
        );
        assert_eq!(auth.code, PROVIDER_AUTH_REQUIRED);

        let broken = classify_startup_error(
            &AcpError::AgentError {
                code: -32000,
                message: "disk full".into(),
            },
            "open an agent session",
        );
        assert_eq!(broken.code, PROVIDER_UNAVAILABLE);

        assert_eq!(
            classify_startup_error(&AcpError::AgentExited, "spawn the agent").code,
            PROVIDER_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn a_full_mailbox_is_reported_rather_than_awaited() {
        let (tx, _rx) = mpsc::channel(1);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        let handle = SessionHandle {
            session_id: "s1".into(),
            tx,
            shutdown,
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

    #[test]
    fn durable_shutdown_bypasses_a_full_turn_mailbox() {
        let (tx, _rx) = mpsc::channel(1);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let handle = SessionHandle {
            session_id: "s1".into(),
            tx,
            shutdown,
        };
        handle
            .deliver(SessionCommand::Turn {
                command_id: "queued".into(),
                text: "work".into(),
            })
            .expect("mailbox entry");

        handle.shutdown();

        assert!(*shutdown_rx.borrow());
    }

    #[tokio::test]
    async fn delivering_to_a_dead_actor_reports_rather_than_hangs() {
        let (tx, rx) = mpsc::channel(1);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        drop(rx);
        let handle = SessionHandle {
            session_id: "s1".into(),
            tx,
            shutdown,
        };
        assert_eq!(
            handle.deliver(SessionCommand::Shutdown),
            Err(DeliverError::Gone)
        );
        assert!(!handle.is_live());
    }
}
