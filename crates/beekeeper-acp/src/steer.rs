//! Public mid-turn steering transport.
//!
//! The narrow surface an out-of-tree caller (today `beekeeper-session-provider`)
//! uses to hand an input to the turn already running on an [`AcpClient`] and
//! learn, truthfully, what became of it. The legacy channel harness keeps its
//! private `pool` types and adapts them onto this module; nothing here knows
//! about mentions, queues or cancel-and-merge.
//!
//! The contract is `docs/NATIVE_STEERING_IMPL.md` §3.1. The one rule that
//! shapes every variant: an outcome names what the wire established, never
//! what would be convenient. A written request whose acknowledgement never
//! arrived is [`SteerResolution::Unknown`], not a failure and not a success,
//! and the caller decides what to publish about it.
//!
//! [`AcpClient`]: crate::acp::AcpClient

use std::time::Duration;

/// How long the read loop keeps reading for a pending steer acknowledgement
/// after the prompt it was written into has already answered.
///
/// Bounded so a late adapter cannot hold the actor past its turn; long enough
/// that claude-agent-acp's synchronous `steer` handler, which answers in the
/// same tick it pushes the message, is never cut off.
pub const STEER_ACK_DRAIN: Duration = Duration::from_millis(1500);

/// A caller's revocable admission, checked immediately before a native write.
///
/// Queueing an input is not a dispatch. Implementations atomically check any
/// revocation and record dispatch under the same lock that revokes admission.
/// A denied input is positively not delivered and must not be auto-requeued.
pub trait SteerWriteGuard: std::fmt::Debug + Send + Sync {
    /// Claim this input's write boundary, or refuse it without writing bytes.
    fn begin_write(&self) -> Result<(), SteerWriteRefusal>;
}

/// Why the caller prevented an input before any runtime write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerWriteRefusal {
    /// A recorded authority fence revoked this queued input.
    Fenced,
    /// The sender lost its previously verified operator grant.
    OperatorRevoked,
    /// The session authority chain is known to require verification.
    AuthorityUnverified,
    /// The caller could not read its authority guard reliably.
    Unavailable,
}

/// One mid-turn input the caller wants delivered into the running turn.
#[derive(Debug)]
pub struct SteerInput {
    /// Caller-minted attempt identity, echoed on every outcome so a late
    /// acknowledgement is correlated to this attempt and never to whichever
    /// input happens to be newest.
    pub attempt_id: String,
    /// Prompt body; each entry becomes one ACP `text` content block.
    pub prompt_blocks: Vec<String>,
    /// What the adapter is asked to do when no turn is running.
    pub idle_guard: IdleGuard,
    /// Optional caller authority fence, rechecked after all earlier ACKs.
    pub write_guard: Option<std::sync::Arc<dyn SteerWriteGuard>>,
    /// Answered exactly once by the read loop. Dropped unanswered only when
    /// the input never left the channel, which the caller reads as
    /// [`NotDeliveredReason::PromptEndedBeforeWrite`].
    pub outcome_tx: tokio::sync::oneshot::Sender<SteerResolution>,
}

/// What a `_session/steering` request asks of an adapter that finds no
/// running turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleGuard {
    /// Send `_meta.steering.idleBehavior: "promptRequired"`: an adapter that
    /// honours it answers `promptRequired` and starts nothing.
    PromptRequired,
    /// Send no idle behaviour; the adapter's own default applies (for
    /// claude-agent-acp and codex-acp, a detached new turn).
    AdapterDefault,
}

/// Which wire method carried the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerWire {
    /// The cross-adapter `_session/steering` extension.
    AcpExtension,
    /// goose's `_goose/unstable/session/steer`, which names a run id.
    Goose,
}

/// What the transport can truthfully say about one attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerResolution {
    /// Written and positively acknowledged as joined into the running turn.
    Injected {
        /// The method that carried it.
        wire: SteerWire,
        /// The adapter's run id the request named, when the wire has one.
        native_run_id: Option<String>,
    },
    /// Written and positively acknowledged as delivered into a **new** native
    /// turn the caller is not awaiting.
    StartedNewTurn {
        /// The method that carried it.
        wire: SteerWire,
    },
    /// Nothing reached the runtime; the input is still the caller's to
    /// deliver another way.
    NotDelivered {
        /// Why nothing was written, or why the write is known to have
        /// delivered nothing.
        reason: NotDeliveredReason,
    },
    /// Bytes may have reached the runtime and nothing establishes whether the
    /// input arrived. The caller must not replay it automatically.
    Unknown {
        /// What is known about how the attempt lost its answer.
        reason: UnknownReason,
        /// The JSON-RPC id the request was written under, when it was written,
        /// so a late answer can still be correlated.
        wire_request_id: Option<u64>,
    },
}

/// Why an input was positively not delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotDeliveredReason {
    /// At write time neither steer wire was available: no goose run id and no
    /// advertised `_session/steering`. Nothing was written.
    Unsupported,
    /// The adapter answered `promptRequired`: no turn was running and the
    /// content was left with the caller.
    PromptRequired,
    /// JSON-RPC `-32601`: the adapter does not implement the method.
    MethodNotFound {
        /// The adapter's error text.
        message: String,
    },
    /// Any other JSON-RPC error: the adapter refused the request and injected
    /// nothing.
    Rejected {
        /// JSON-RPC error code.
        code: i64,
        /// The adapter's error text.
        message: String,
    },
    /// The prompt ended before the read loop took this input from its
    /// channel; nothing was written.
    PromptEndedBeforeWrite,
    /// The caller's authority fence prevented this queued input's write.
    DispatchPrevented {
        /// Observed revocation or unavailable authority verification.
        reason: SteerWriteRefusal,
    },
}

/// Why delivery could not be established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownReason {
    /// The write returned an error; some bytes may already have gone out.
    WriteFailed {
        /// The I/O error text.
        message: String,
    },
    /// The request was written and the prompt answered, errored or timed out
    /// before the acknowledgement arrived.
    PromptEndedBeforeAck,
    /// The request was written and the bounded post-prompt drain
    /// ([`STEER_ACK_DRAIN`]) expired without its acknowledgement.
    AckTimeout,
    /// A JSON-RPC success whose `outcome` is absent or unrecognized — a bare
    /// `{}` proves nothing either way.
    UnrecognizedAck {
        /// What the adapter actually reported, for the record.
        outcome: String,
    },
    /// A JSON-RPC success carrying the adapter's own `failed`: it says the
    /// input could not be applied, without proving where the bytes went.
    AdapterReportedFailure {
        /// The reported outcome string.
        outcome: String,
    },
    /// The runtime's stdout closed after the request was written.
    RuntimeExited,
}

/// An acknowledgement that arrived after its attempt had already been
/// resolved [`SteerResolution::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateSteerAck {
    /// The attempt the answer belongs to.
    pub attempt_id: String,
    /// What the late answer established.
    pub resolution: SteerResolution,
}
