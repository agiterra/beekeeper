//! A turn nobody prompted (SV-77, ledger 336).
//!
//! An agent can start work on its own between prompts: Claude Code wakes on a
//! background task's notification and runs a whole cycle — prose, tool calls,
//! permission requests — and claude-agent-acp forwards it live. The transport
//! hands that output up through [`AcpClient::next_unsolicited`]; this module
//! publishes it as the session's **own** turn: a fresh turn id, the same
//! transcript items and the same `working → idle` status a prompted turn gets,
//! never glued onto the user's next turn.
//!
//! The turn ends when the adapter says so (an autonomous `result`,
//! [`Unsolicited::CycleEnded`]); when the adapter goes quiet for
//! [`AUTONOMOUS_QUIET`] with no tool call open (said in the record, since that
//! end is inferred); when the operator stops it; or when the adapter exits.
//!
//! A prompt that arrives while an autonomous turn runs waits in the actor's
//! queue and is sent when the turn ends. Claude Code would queue it behind the
//! cycle anyway; sending it early would only interleave the two turns' output
//! on one stream that carries no turn tag, which is exactly how the old bug
//! filed autonomous work under the user's turn.

use super::*;
use beekeeper_acp::acp::Unsolicited;

/// How long an autonomous turn may go without a frame, with no tool call
/// open, before it is closed as finished without the adapter's end signal.
pub(super) const AUTONOMOUS_QUIET: Duration = Duration::from_secs(30);

/// How an autonomous turn ended.
enum AutonomousEnd {
    Finished(TurnOutcome),
    Shutdown,
    AgentGone,
}

/// What woke the autonomous turn's select loop.
enum Wake {
    Shutdown,
    Item(Unsolicited),
    Late(Option<LateSteerAck>),
    Command(Option<SessionCommand>),
    Quiet,
}

/// Tool-call ids an autonomous turn has seen open and not yet seen finish.
fn track_tools(open: &mut HashSet<String>, msg: &serde_json::Value) {
    if msg.get("method").and_then(serde_json::Value::as_str) != Some("session/update") {
        return;
    }
    let update = &msg["params"]["update"];
    let kind = update
        .get("sessionUpdate")
        .and_then(serde_json::Value::as_str);
    if !matches!(kind, Some("tool_call" | "tool_call_update")) {
        return;
    }
    let Some(id) = update.get("toolCallId").and_then(serde_json::Value::as_str) else {
        return;
    };
    match update.get("status").and_then(serde_json::Value::as_str) {
        Some("completed" | "failed") => {
            open.remove(id);
        }
        // An update without a status changes nothing about whether it runs.
        None if kind == Some("tool_call_update") => {}
        _ => {
            open.insert(id.to_owned());
        }
    }
}

/// Translate one unsolicited `session/update`, exactly as `translate_frame`
/// does for a prompted turn's observer frames.
fn translate_unsolicited(
    translator: &mut TranscriptTranslator,
    acp_session_id: &str,
    msg: &serde_json::Value,
) -> Vec<serde_json::Value> {
    if msg.get("method").and_then(serde_json::Value::as_str) != Some("session/update") {
        return Vec::new();
    }
    let Some(params) = msg.get("params") else {
        return Vec::new();
    };
    if let Some(session_id) = params.get("sessionId").and_then(serde_json::Value::as_str) {
        if session_id != acp_session_id {
            return Vec::new();
        }
    }
    match params.get("update") {
        Some(update) => translator.on_update(update),
        None => Vec::new(),
    }
}

impl SessionActor {
    /// Run every autonomous turn already waiting before `run_turn` writes a
    /// prompt: work the agent began before this prompt reached it is its own
    /// turn, and the prompt waits behind it. Bookkeeping frames are dropped —
    /// the client already applied them to its own state.
    pub(super) async fn run_waiting_autonomous_turns(
        &mut self,
        rx: &mut mpsc::Receiver<SessionCommand>,
        shutdown: &mut watch::Receiver<bool>,
        queued: &mut VecDeque<SessionCommand>,
    ) -> Option<ExitReason> {
        while let Some(item) = self.client.try_next_unsolicited() {
            if item.opens_turn() {
                if let Some(exit) = self.run_autonomous_turn(item, rx, shutdown, queued).await {
                    return Some(exit);
                }
            }
        }
        None
    }

    /// Publish one autonomous turn, starting from the frame that opened it.
    /// Returns an exit reason when the actor must retire.
    pub(super) async fn run_autonomous_turn(
        &mut self,
        first: Unsolicited,
        rx: &mut mpsc::Receiver<SessionCommand>,
        shutdown: &mut watch::Receiver<bool>,
        queued: &mut VecDeque<SessionCommand>,
    ) -> Option<ExitReason> {
        let turn_id = Uuid::new_v4().to_string();
        let started = Instant::now();
        let hard_deadline = started + self.max_turn_duration;
        tracing::info!(
            target: "csp::session",
            session_id = %self.session_id,
            %turn_id,
            "the agent started a turn nobody prompted"
        );
        let _ = self
            .events
            .send(SessionEvent::AutonomousTurnStarted {
                session_id: self.session_id.clone(),
                turn_id: turn_id.clone(),
            })
            .await;
        // A fresh turn for the translator. There is no prompt to echo, so the
        // `user_prompt` item `begin_turn` returns is discarded: publishing it
        // would claim somebody asked.
        let _ = self.translator.begin_turn("", None, None, None, 0);
        // Said first, so a reader knows at once that the agent woke on its
        // own: claude-agent-acp forwards the reply but not the
        // `<task-notification>` that caused it (verified live, 0.84.0), and
        // without this row an earlier turn's background task would keep
        // reading "running" for the whole wake (SV-78).
        emit_items(
            &self.events,
            &self.session_id,
            &turn_id,
            vec![crate::payload::status_item(
                "autonomous_turn_started: the agent began a turn nobody prompted",
            )],
        )
        .await;

        let mut open_tools: HashSet<String> = HashSet::new();
        let mut last_activity = Instant::now();
        let mut next = Some(first);
        let end = loop {
            if let Some(item) = next.take() {
                match item {
                    Unsolicited::Frame(msg) => {
                        last_activity = Instant::now();
                        track_tools(&mut open_tools, &msg);
                        let mut items =
                            translate_unsolicited(&mut self.translator, &self.acp_session_id, &msg);
                        let checks = self.translator.take_stream_checks();
                        crate::native_output::reconcile(
                            &mut items,
                            checks,
                            self.native_output.as_ref(),
                        )
                        .await;
                        emit_items(&self.events, &self.session_id, &turn_id, items).await;
                    }
                    Unsolicited::CycleEnded { origin } => {
                        // The agent's buffered prose goes first: emitted
                        // ahead of it, this row split the final answer in two
                        // (SV-93, audit seq 51/52/53).
                        let mut items = self.translator.flush_all();
                        items.push(crate::payload::status_item(&format!(
                            "autonomous_turn: the agent woke on {origin}"
                        )));
                        emit_items(&self.events, &self.session_id, &turn_id, items).await;
                        break AutonomousEnd::Finished(TurnOutcome::Completed {
                            stop_reason: StopReason::EndTurn,
                        });
                    }
                    Unsolicited::Exited => break AutonomousEnd::AgentGone,
                }
            }
            let deadline = if open_tools.is_empty() {
                (last_activity + AUTONOMOUS_QUIET).min(hard_deadline)
            } else {
                hard_deadline
            };
            let wake = tokio::select! {
                biased;
                _ = shutdown.changed() => Wake::Shutdown,
                item = self.client.next_unsolicited() => Wake::Item(item),
                late = self.late_steers.recv(), if !self.late_sink_closed => Wake::Late(late),
                command = rx.recv() => Wake::Command(command),
                _ = tokio::time::sleep_until(deadline.into()) => Wake::Quiet,
            };
            match wake {
                Wake::Shutdown => break AutonomousEnd::Shutdown,
                Wake::Item(item) => next = Some(item),
                Wake::Late(Some(ack)) => {
                    forward_late_ack(
                        &self.events,
                        &self.session_id,
                        &mut self.steer_commands,
                        ack,
                    )
                    .await;
                }
                Wake::Late(None) => self.late_sink_closed = true,
                Wake::Command(command) => {
                    if let Some(end) = self.autonomous_command(command, queued).await {
                        break end;
                    }
                }
                Wake::Quiet if open_tools.is_empty() => {
                    let mut items = self.translator.flush_all();
                    items.push(crate::payload::status_item(&format!(
                        "autonomous_turn_quiet: no activity for {}s and no tool call open; \
                         closed without the adapter's end-of-cycle signal",
                        AUTONOMOUS_QUIET.as_secs()
                    )));
                    emit_items(&self.events, &self.session_id, &turn_id, items).await;
                    break AutonomousEnd::Finished(TurnOutcome::Completed {
                        stop_reason: StopReason::EndTurn,
                    });
                }
                Wake::Quiet => {
                    break AutonomousEnd::Finished(TurnOutcome::Failed {
                        message: format!(
                            "the agent's unprompted turn still had {} tool call(s) open after {}s; \
                             Beekeeper stopped following it",
                            open_tools.len(),
                            self.max_turn_duration.as_secs()
                        ),
                        agent_gone: false,
                    });
                }
            }
        };

        let (outcome, exit) = match end {
            AutonomousEnd::Finished(outcome) => (outcome, None),
            AutonomousEnd::Shutdown => (TurnOutcome::Cancelled, Some(ExitReason::Requested)),
            AutonomousEnd::AgentGone => {
                let message = "agent exited during an unprompted turn".to_owned();
                (
                    TurnOutcome::Failed {
                        message: message.clone(),
                        agent_gone: true,
                    },
                    Some(ExitReason::AgentGone(message)),
                )
            }
        };
        let tool_calls = self.translator.tool_calls();
        let tail = self.translator.close_turn();
        emit_items(&self.events, &self.session_id, &turn_id, tail).await;
        let _ = self
            .events
            .send(SessionEvent::TurnFinished {
                session_id: self.session_id.clone(),
                turn_id,
                outcome,
                duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                // The adapter reports usage per prompt; an unprompted cycle's
                // cost reaches the client's trackers as a baseline advance and
                // is not attributed to any turn.
                usage: None,
                tool_calls,
            })
            .await;
        exit
    }

    /// A command that arrived while an autonomous turn runs.
    async fn autonomous_command(
        &mut self,
        command: Option<SessionCommand>,
        queued: &mut VecDeque<SessionCommand>,
    ) -> Option<AutonomousEnd> {
        let (command, guarded_ci) = unwrap_ci_turn(command);
        match command {
            Some(SessionCommand::GuardedCiTurn { .. }) => None,
            None | Some(SessionCommand::Shutdown) => Some(AutonomousEnd::Shutdown),
            // A switch waits for this turn's boundary like any queued command.
            Some(switch @ SessionCommand::SetModel { .. }) => {
                super::model_switch::queue_model_switch(
                    &self.events,
                    &self.session_id,
                    queued,
                    switch,
                )
                .await;
                None
            }
            Some(SessionCommand::Interrupt { command_id }) => {
                // Stop is honoured on a turn nobody prompted too. There is no
                // prompt to drain, so the cancel is the whole of it; anything
                // the adapter still emits opens a turn of its own.
                if let Err(error) = self.client.session_cancel(&self.acp_session_id).await {
                    tracing::warn!(
                        target: "csp::session",
                        session_id = %self.session_id,
                        %command_id,
                        "could not cancel the unprompted turn: {error}"
                    );
                }
                Some(AutonomousEnd::Finished(TurnOutcome::Cancelled))
            }
            Some(SessionCommand::Steer {
                command_id,
                attempt_id,
                ..
            }) => {
                // No prompt is in flight to steer into: answered exactly as
                // the idle actor answers it, so the provider delivers the
                // words as a turn at the next boundary.
                if dequeue_or_drop(&self.fenced, &command_id) {
                    return None;
                }
                release_dequeue(&self.fenced, &command_id);
                let _ = self
                    .events
                    .send(SessionEvent::SteerResolved {
                        session_id: self.session_id.clone(),
                        turn_id: None,
                        command_id,
                        attempt_id,
                        resolution: SteerDispatch::Idle,
                    })
                    .await;
                None
            }
            Some(turn @ SessionCommand::Turn { .. }) => {
                if queued.len() >= SESSION_QUEUE_DEPTH {
                    if let SessionCommand::Turn { command_id, .. } = turn {
                        let _ = self
                            .events
                            .send(SessionEvent::TurnDropped {
                                session_id: self.session_id.clone(),
                                command_id,
                                reason: TurnDropReason::QueueFull,
                            })
                            .await;
                    }
                } else {
                    queued.push_back(if guarded_ci {
                        SessionCommand::GuardedCiTurn {
                            turn: Box::new(turn),
                        }
                    } else {
                        turn
                    });
                }
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "session_autonomous_tests.rs"]
mod tests;
