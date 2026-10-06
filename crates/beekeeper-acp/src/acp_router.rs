//! The adapter's stdout, read for the whole life of the process (SV-77).
//!
//! Before this module the client read stdout only from inside its own
//! requests. An adapter that does work nobody prompted — claude-agent-acp
//! forwards a task-notification cycle live (`dist/acp-agent.js` `runConsumer`)
//! — wrote into a pipe nobody was reading: the work was invisible until the
//! next prompt, then filed under it, and a `session/request_permission` it
//! raised meanwhile waited for that prompt to be answered (ledger 336).
//!
//! One task now owns the reader. Per frame, under one lock:
//!
//! 1. A response whose id a [`AcpClient::send_request`] registered goes to
//!    that request's oneshot, whatever else is happening.
//! 2. While a prompt is in flight (the client is *attached*), everything else
//!    goes to the client's inbox, which the prompt, cancel and steer-drain
//!    loops read exactly as they used to read the pipe.
//! 3. Otherwise an agent-initiated request is answered at once, with the same
//!    policy a prompted turn applies, by a small answerer task.
//! 4. Otherwise a notification goes upward as an [`Unsolicited`] frame — once
//!    the owner has asked for them with [`AcpClient::next_unsolicited`] — and
//!    into a backlog the client applies to its own state (usage, run id) at
//!    its next call. A client nobody asks keeps today's behaviour: the
//!    notification waits in the inbox for the next read loop.
//! 5. Anything else (a response nobody waits for) goes to the inbox, where the
//!    next read loop correlates it with an unresolved steer or skips it.
//!
//! When the stream ends every waiter fails with the reason, the inbox closes
//! (so a read loop sees EOF), and the owner is told [`Unsolicited::Exited`].

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{mpsc, oneshot};

use super::*;

/// Notifications held for the client's own state while nobody calls it. Only
/// usage and run-id bookkeeping read them, and only the newest of either
/// matters, so the oldest are dropped past this bound rather than growing
/// without limit on a seat that idles for days.
const BACKLOG_CAP: usize = 4_096;

/// Bound on one write from the answerer, as on every other write.
const ANSWER_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// One thing the reader hands a read loop.
#[derive(Debug)]
pub(super) enum Inbound {
    /// A parsed JSON-RPC message and the byte length of its line.
    Frame { msg: serde_json::Value, len: usize },
    /// The codec failed; the stream ends after this.
    Error(LinesCodecError),
}

/// What the adapter did while no prompt was in flight.
#[derive(Debug, Clone)]
pub enum Unsolicited {
    /// One JSON-RPC message, verbatim: a `session/update`, or an agent request
    /// that was already answered by the time this is read.
    Frame(serde_json::Value),
    /// claude-agent-acp reported the end of an autonomous cycle: a raw SDK
    /// `result` whose origin is one of the adapter's own autonomous kinds,
    /// carrying at least one model turn (a `num_turns: 0` placeholder is not
    /// the end — the shared followup is still ahead of it).
    CycleEnded {
        /// `origin.kind`, e.g. `task-notification`.
        origin: String,
    },
    /// The adapter's stdout closed.
    Exited,
}

impl Unsolicited {
    /// Whether this is the agent doing something — the start of a turn
    /// nobody prompted — rather than bookkeeping that trails every turn
    /// (usage, advertised commands, mode or run-id updates).
    pub fn opens_turn(&self) -> bool {
        let Self::Frame(msg) = self else {
            return false;
        };
        match msg.get("method").and_then(serde_json::Value::as_str) {
            Some("session/request_permission") => true,
            Some("session/update") => matches!(
                msg.pointer("/params/update/sessionUpdate")
                    .and_then(serde_json::Value::as_str),
                Some(
                    "agent_message_chunk"
                        | "agent_thought_chunk"
                        | "user_message_chunk"
                        | "tool_call"
                        | "tool_call_update"
                        | "plan"
                )
            ),
            _ => false,
        }
    }
}

/// Why the stream ended, kept so a request registered afterwards fails the
/// same way the ones in flight did.
#[derive(Debug, Clone)]
enum Closed {
    Eof,
    LineTooLong,
    Io(String),
}

impl Closed {
    fn to_error(&self) -> AcpError {
        match self {
            Closed::Eof => AcpError::AgentExited,
            Closed::LineTooLong => {
                AcpError::Protocol("agent stdout line exceeded 10MB limit".into())
            }
            Closed::Io(message) => AcpError::Io(std::io::Error::other(message.clone())),
        }
    }
}

type Waiter = oneshot::Sender<Result<serde_json::Value, AcpError>>;

/// Everything the reader decides with, behind one lock so a routing decision
/// and an attach or detach can never interleave.
pub(super) struct RouterState {
    waiters: HashMap<u64, Waiter>,
    inbox_tx: Option<mpsc::UnboundedSender<Inbound>>,
    answer_tx: Option<mpsc::UnboundedSender<serde_json::Value>>,
    unsolicited_tx: Option<mpsc::UnboundedSender<Unsolicited>>,
    attached: bool,
    closed: Option<Closed>,
    backlog: VecDeque<serde_json::Value>,
    observer: Option<ObserverHandle>,
    observer_agent_index: Option<usize>,
    observer_context: ObserverContext,
}

/// The shared half of the reader. Cloned into the reader and answerer tasks.
#[derive(Clone)]
pub(super) struct Router(Arc<Mutex<RouterState>>);

impl Router {
    pub(super) fn lock(&self) -> MutexGuard<'_, RouterState> {
        // A panic while routing must not wedge the adapter for good: the
        // state is plain bookkeeping and stays usable.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Start the reader and answerer for a freshly spawned child. Returns the
    /// router and the inbox the client's read loops consume.
    pub(super) fn start(
        stdout: ChildStdout,
        stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    ) -> (Self, mpsc::UnboundedReceiver<Inbound>) {
        let (inbox_tx, inbox_rx) = mpsc::unbounded_channel();
        let (answer_tx, answer_rx) = mpsc::unbounded_channel();
        let router = Router(Arc::new(Mutex::new(RouterState {
            waiters: HashMap::new(),
            inbox_tx: Some(inbox_tx),
            answer_tx: Some(answer_tx),
            unsolicited_tx: None,
            attached: false,
            closed: None,
            backlog: VecDeque::new(),
            observer: None,
            observer_agent_index: None,
            observer_context: ObserverContext::default(),
        })));
        tokio::spawn(read_stdout(stdout, router.clone()));
        tokio::spawn(answer_requests(answer_rx, stdin, router.clone()));
        (router, inbox_rx)
    }

    /// Register interest in the response to `id`. Fails at once when the
    /// stream has already ended, so a request to a dead adapter does not wait
    /// out its timeout.
    pub(super) fn register(
        &self,
        id: u64,
    ) -> Result<oneshot::Receiver<Result<serde_json::Value, AcpError>>, AcpError> {
        let mut state = self.lock();
        if let Some(closed) = &state.closed {
            return Err(closed.to_error());
        }
        let (tx, rx) = oneshot::channel();
        state.waiters.insert(id, tx);
        Ok(rx)
    }

    /// Forget a waiter whose request gave up.
    pub(super) fn unregister(&self, id: u64) {
        self.lock().waiters.remove(&id);
    }

    /// Ask for frames that arrive with no prompt in flight.
    pub(super) fn enable_unsolicited(&self) -> mpsc::UnboundedReceiver<Unsolicited> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut state = self.lock();
        if state.closed.is_some() {
            let _ = tx.send(Unsolicited::Exited);
        } else {
            state.unsolicited_tx = Some(tx);
        }
        rx
    }

    pub(super) fn set_observer(&self, observer: Option<ObserverHandle>, agent_index: usize) {
        let mut state = self.lock();
        state.observer = observer;
        state.observer_agent_index = Some(agent_index);
    }

    pub(super) fn set_observer_context(&self, context: ObserverContext) {
        self.lock().observer_context = context;
    }

    /// Notifications that arrived while nobody was reading, oldest first.
    pub(super) fn take_backlog(&self) -> Vec<serde_json::Value> {
        self.lock().backlog.drain(..).collect()
    }
}

impl RouterState {
    fn observe(&self, kind: &str, payload: serde_json::Value) {
        if let Some(observer) = &self.observer {
            observer.emit(
                kind,
                self.observer_agent_index,
                &self.observer_context,
                payload,
            );
        }
    }

    /// Route one notification nobody is attached to read. `false` when the
    /// owner never asked for unsolicited frames: the caller keeps it for the
    /// next read loop instead, as before.
    fn route_detached_notification(&mut self, msg: serde_json::Value) -> bool {
        let Some(tx) = self.unsolicited_tx.clone() else {
            return false;
        };
        // A distinct kind: a turn-scoped observer consumer translating
        // `acp_read` must never file between-turn output under a user turn.
        self.observe("acp_read_between_turns", msg.clone());
        if self.backlog.len() >= BACKLOG_CAP {
            self.backlog.pop_front();
        }
        self.backlog.push_back(msg.clone());
        let upward = match autonomous_cycle_end(&msg) {
            Some(origin) => Some(Unsolicited::CycleEnded { origin }),
            None if msg.get("method").and_then(serde_json::Value::as_str)
                == Some(RAW_SDK_FRAME_METHOD) =>
            {
                None
            }
            None => Some(Unsolicited::Frame(msg)),
        };
        if let Some(item) = upward {
            if tx.send(item).is_err() {
                self.unsolicited_tx = None;
            }
        }
        true
    }

    /// Hand an agent request to the answerer, and tell the owner it happened.
    fn route_detached_request(&mut self, msg: serde_json::Value) {
        self.observe("acp_read", msg.clone());
        if let Some(tx) = &self.unsolicited_tx {
            if tx.send(Unsolicited::Frame(msg.clone())).is_err() {
                self.unsolicited_tx = None;
            }
        }
        match &self.answer_tx {
            Some(tx) => {
                let _ = tx.send(msg);
            }
            None => tracing::warn!(
                target: "acp::wire",
                "agent request arrived after the answerer stopped; it goes unanswered"
            ),
        }
    }

    fn send_inbox(&mut self, item: Inbound) {
        if let Some(tx) = &self.inbox_tx {
            let _ = tx.send(item);
        }
    }

    fn close(&mut self, closed: Closed) {
        for (_, waiter) in self.waiters.drain() {
            let _ = waiter.send(Err(closed.to_error()));
        }
        self.inbox_tx = None;
        self.answer_tx = None;
        if let Some(tx) = self.unsolicited_tx.take() {
            let _ = tx.send(Unsolicited::Exited);
        }
        self.closed.get_or_insert(closed);
    }
}

/// `Some(origin)` when `msg` is claude-agent-acp's raw SDK `result` closing an
/// autonomous cycle — see [`Unsolicited::CycleEnded`].
fn autonomous_cycle_end(msg: &serde_json::Value) -> Option<String> {
    if msg.get("method").and_then(serde_json::Value::as_str) != Some(RAW_SDK_FRAME_METHOD) {
        return None;
    }
    let message = msg.pointer("/params/message")?;
    if message.get("type").and_then(serde_json::Value::as_str) != Some("result") {
        return None;
    }
    let origin = message.pointer("/origin/kind")?.as_str()?;
    if !AUTONOMOUS_RESULT_ORIGINS.contains(&origin) {
        return None;
    }
    let num_turns = message
        .get("num_turns")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    (num_turns > 0).then(|| origin.to_owned())
}

/// Route one parsed frame. See the module docs for the order.
fn route(router: &Router, msg: serde_json::Value, len: usize) {
    let mut state = router.lock();
    let has_method = msg.get("method").is_some();
    let has_id = msg.get("id").is_some();
    if !has_method && has_id {
        if let Some(waiter) = msg
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .and_then(|id| state.waiters.remove(&id))
        {
            state.observe("acp_read", msg.clone());
            let outcome = match msg.get("error") {
                Some(error) => Err(agent_error_from_json(error)),
                None => Ok(msg["result"].clone()),
            };
            let _ = waiter.send(outcome);
            return;
        }
    }
    if state.attached {
        state.send_inbox(Inbound::Frame { msg, len });
        return;
    }
    if has_method && has_id {
        state.route_detached_request(msg);
        return;
    }
    if has_method && state.route_detached_notification(msg.clone()) {
        return;
    }
    state.send_inbox(Inbound::Frame { msg, len });
}

/// The one reader of the adapter's stdout.
async fn read_stdout(stdout: ChildStdout, router: Router) {
    // LinesCodec::new_with_max_length enforces MAX_LINE_SIZE at the read
    // level — the buffer never grows beyond the limit, so a rogue agent
    // writing endless bytes without a newline cannot exhaust memory.
    let mut reader = FramedRead::new(stdout, LinesCodec::new_with_max_length(MAX_LINE_SIZE));
    let mut failed: Option<Closed> = None;
    loop {
        match reader.next().await {
            None => {
                router.lock().close(failed.unwrap_or(Closed::Eof));
                return;
            }
            Some(Err(error)) => {
                // The framed stream ends after a codec error. Whoever is
                // reading hears the error itself; the close that follows
                // fails everyone else with the same reason.
                failed = Some(match &error {
                    LinesCodecError::MaxLineLengthExceeded => Closed::LineTooLong,
                    other => Closed::Io(other.to_string()),
                });
                let mut state = router.lock();
                state.send_inbox(Inbound::Error(error));
            }
            Some(Ok(line)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                tracing::debug!(target: "acp::wire", "← {trimmed}");
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(msg) => route(&router, msg, trimmed.len()),
                    Err(error) => {
                        router.lock().observe(
                            "acp_parse_error",
                            serde_json::json!({
                                "line": trimmed,
                                "error": error.to_string(),
                            }),
                        );
                        tracing::warn!(
                            target: "acp::wire",
                            "failed to parse line as JSON: {error} — skipping"
                        );
                    }
                }
            }
        }
    }
}

/// The reply a prompted turn gives an agent request, for any request.
///
/// `session/request_permission` gets [`permission_reply`]'s answer; a
/// permission request with no usable option is answered `cancelled` rather
/// than left hanging, because between turns there is no turn to fail in its
/// place. Any other request is answered -32601, as every read loop does.
fn reply_to(msg: &serde_json::Value) -> serde_json::Value {
    let id = msg.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = msg
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if method == "session/request_permission" {
        return match permission_reply(msg) {
            Ok(reply) => reply,
            Err(error) => {
                tracing::warn!(
                    target: "acp::permission",
                    "between-turn permission request id={id} has no usable option ({error}); \
                     answering cancelled"
                );
                permission_response_cancelled(&id)
            }
        };
    }
    tracing::debug!(target: "acp::wire", "ignoring unknown method: {method}");
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": -32601, "message": format!("Method not found: {method}")}
    })
}

/// Answer agent requests that arrive with no prompt in flight, in order.
async fn answer_requests(
    mut requests: mpsc::UnboundedReceiver<serde_json::Value>,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    router: Router,
) {
    while let Some(msg) = requests.recv().await {
        let reply = reply_to(&msg);
        let Ok(line) = serde_json::to_string(&reply) else {
            continue;
        };
        let written = tokio::time::timeout(ANSWER_WRITE_TIMEOUT, async {
            let mut stdin = stdin.lock().await;
            stdin.write_all(line.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        })
        .await;
        match written {
            Ok(Ok(())) => router.lock().observe("acp_write", reply),
            Ok(Err(error)) => {
                tracing::warn!(target: "acp::wire", "between-turn reply failed: {error}");
            }
            Err(_) => tracing::warn!(
                target: "acp::wire",
                "between-turn reply timed out after {ANSWER_WRITE_TIMEOUT:?}: the agent stopped reading"
            ),
        }
    }
}

impl AcpClient {
    /// Route everything that is not a registered response to the read loops
    /// from now on. Called before a prompt is written and by every loop that
    /// reads the inbox.
    pub(super) fn attach(&mut self) {
        self.router.lock().attached = true;
        self.absorb_backlog();
    }

    /// Stop routing to the read loops when no prompt is in flight any more.
    ///
    /// Anything the loops left unread is re-routed as if it had arrived now,
    /// under the same lock, so it reaches the owner ahead of anything newer:
    /// a request is answered, a notification goes upward, a response is
    /// correlated with an unresolved steer or dropped as stray (which is what
    /// the next loop would have done with it). A client whose owner never
    /// asked for unsolicited frames keeps them for its next loop, as before.
    pub(super) fn detach_if_idle(&mut self) {
        if self.last_prompt_id.is_some() {
            return;
        }
        let mut leftovers = Vec::new();
        {
            let mut state = self.router.lock();
            state.attached = false;
            if state.unsolicited_tx.is_none() {
                return;
            }
            while let Ok(item) = self.inbox.try_recv() {
                let Inbound::Frame { msg, .. } = item else {
                    continue;
                };
                let has_method = msg.get("method").is_some();
                let has_id = msg.get("id").is_some();
                if has_method && has_id {
                    state.route_detached_request(msg);
                } else if has_method {
                    state.route_detached_notification(msg);
                } else {
                    leftovers.push(msg);
                }
            }
        }
        for msg in leftovers {
            if !self.route_late_steer_ack(&msg) {
                tracing::debug!(target: "acp::wire", "dropping stray response left after the prompt");
            }
        }
    }

    /// Apply notifications that arrived while nobody was reading to this
    /// client's own state, in arrival order.
    pub(super) fn absorb_backlog(&mut self) {
        for msg in self.router.take_backlog() {
            self.apply_notification(&msg);
        }
    }

    /// The state effects of one notification, shared by every path that
    /// reads one. Returns whether it opened a tool call (the prompt loop
    /// resets its idle clock on that).
    pub(super) fn apply_notification(&mut self, msg: &serde_json::Value) -> bool {
        match msg.get("method").and_then(serde_json::Value::as_str) {
            Some("session/update") => self.handle_session_update(msg),
            Some("_goose/unstable/session/update") => {
                self.handle_goose_usage_update(msg);
                false
            }
            Some(RAW_SDK_FRAME_METHOD) => {
                self.handle_raw_sdk_frame(msg);
                false
            }
            _ => false,
        }
    }

    /// After a plain request is answered: apply what arrived around it.
    ///
    /// The old per-request loop handled every notification that preceded the
    /// response before returning; these now sit in the inbox (or the
    /// backlog), so they are applied here, before the caller acts on the
    /// response. Skipped while a prompt is in flight — its inbox belongs to
    /// the cancel drain.
    pub(super) fn settle_after_request(&mut self) {
        self.absorb_backlog();
        if self.last_prompt_id.is_some() {
            return;
        }
        while let Ok(item) = self.inbox.try_recv() {
            let Inbound::Frame { msg, .. } = item else {
                continue;
            };
            self.observe("acp_read", msg.clone());
            if msg.get("method").is_some() {
                self.apply_notification(&msg);
            } else if !self.route_late_steer_ack(&msg) {
                tracing::debug!(target: "acp::wire", "skipping stray response");
            }
        }
    }

    /// The next thing the adapter did with no prompt in flight.
    ///
    /// The first call turns delivery on; before it, such frames keep waiting
    /// for the next read loop. Cancel-safe. Once the adapter has exited and
    /// [`Unsolicited::Exited`] has been returned, never resolves again.
    pub async fn next_unsolicited(&mut self) -> Unsolicited {
        self.enable_unsolicited();
        match self.unsolicited.as_mut() {
            Some(rx) => match rx.recv().await {
                Some(item) => item,
                None => {
                    self.unsolicited = None;
                    self.unsolicited_done = true;
                    std::future::pending().await
                }
            },
            None => std::future::pending().await,
        }
    }

    /// [`next_unsolicited`](Self::next_unsolicited) without waiting: `None`
    /// when nothing is queued. Turns delivery on, like `next_unsolicited`.
    pub fn try_next_unsolicited(&mut self) -> Option<Unsolicited> {
        self.enable_unsolicited();
        self.unsolicited.as_mut()?.try_recv().ok()
    }

    fn enable_unsolicited(&mut self) {
        if self.unsolicited.is_none() && !self.unsolicited_done {
            self.unsolicited = Some(self.router.enable_unsolicited());
        }
    }
}

/// The policy answer to a `session/request_permission`: `allow_once` found by
/// kind, else `reject_once` — never a hard-coded option id.
pub(super) fn permission_reply(msg: &serde_json::Value) -> Result<serde_json::Value, AcpError> {
    let id = msg
        .get("id")
        .cloned()
        .ok_or_else(|| AcpError::Protocol("permission request missing id".into()))?;
    let options = msg["params"]["options"]
        .as_array()
        .ok_or_else(|| AcpError::Protocol("permission request missing options".into()))?;
    tracing::debug!(
        target: "acp::permission",
        "session/request_permission id={id}, {} options",
        options.len()
    );
    let by_kind = |kind: &str| {
        options
            .iter()
            .find(|opt| opt.get("kind").and_then(|k| k.as_str()) == Some(kind))
    };
    if let Some(opt) = by_kind("allow_once") {
        let option_id = opt["optionId"]
            .as_str()
            .ok_or_else(|| AcpError::Protocol("allow_once option missing optionId".into()))?;
        tracing::info!(
            target: "acp::permission",
            "auto-approving permission id={id} with allow_once optionId={option_id:?}"
        );
        return Ok(permission_response_selected(&id, option_id));
    }
    tracing::warn!(
        target: "acp::permission",
        "no allow_once option found in permission request id={id}, falling back to reject_once"
    );
    match by_kind("reject_once") {
        Some(opt) => {
            let option_id = opt["optionId"].as_str().unwrap_or("reject");
            Ok(permission_response_selected(&id, option_id))
        }
        None => Err(AcpError::Protocol(
            "no suitable permission option found (neither allow_once nor reject_once)".into(),
        )),
    }
}

#[cfg(test)]
#[path = "acp_router_tests.rs"]
mod tests;
