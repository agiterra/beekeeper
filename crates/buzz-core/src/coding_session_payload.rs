//! Provider-authored wire payloads: receipts (44224), metadata (44223), and
//! transcript envelopes (44225).
//!
//! These structs are shaped by the consumer, not by convenience. The donor's
//! ingress parser
//! (`desktop/src/features/agents/ui/buzzCodingSessionTrustedIngress.ts` and
//! `buzzCodingSessionTranscriptPresentation.ts`) checks *exact* key sets — an
//! extra field, a missing nullable, or an empty string where `null` was meant
//! all classify the event as `malformed` and it is silently dropped. So every
//! field below is emitted unconditionally, `Option` serializes to explicit
//! `null`, and empty strings are normalized to `null` on the way in.
//!
//! Nothing here may carry host-local execution state. The working directory a
//! session runs in is resolved from the projects file and stays in the
//! provider's own state module; a test there asserts it never appears in signed
//! bytes.
//!
//! These live in `buzz-core` rather than in the provider that authors them
//! because they are also what every *reader* has to agree with — `buzz
//! sessions` parses all three, and the provider crate it would otherwise have
//! to depend on drags the whole ACP stack behind it. One definition, two
//! directions.

use serde::{Deserialize, Serialize};

use crate::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionTarget,
    CODING_SESSION_COMMAND_SCHEMA, MAX_SAFE_GENERATION,
};
use crate::coding_session_lifecycle_command::validate_session_ref;

/// Maximum signed content bytes for a lifecycle receipt (kind 44224).
pub const MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES: usize = 16 * 1024;

/// Schema string on every lifecycle receipt.
pub const LIFECYCLE_RECEIPT_SCHEMA: &str = "buzz-coding-session-lifecycle-receipt/v1";
/// Schema string on every metadata event.
pub const METADATA_SCHEMA: &str = "buzz-coding-session-metadata/v1";
/// Maximum signed content bytes for coding-session metadata.
pub const MAX_METADATA_CONTENT_BYTES: usize = 32 * 1024;
/// Maximum bytes for nullable metadata reference and label fields.
pub const MAX_METADATA_REFERENCE_BYTES: usize = 2 * 1024;
/// Schema string on every transcript item.
pub const TRANSCRIPT_SCHEMA: &str = "buzz-coding-session-transcript/v1";

/// The receipt error code the consumer pins for a create that succeeded but
/// whose initial turn did not.
pub const INITIAL_TURN_FAILED: &str = "INITIAL_TURN_FAILED";
/// No working directory could be resolved for the create command.
pub const PROJECT_CWD_UNRESOLVED: &str = "PROJECT_CWD_UNRESOLVED";
/// The provider is already running as many sessions as it will run.
pub const SESSION_LIMIT: &str = "SESSION_LIMIT";
/// The agent adapter needs interactive authentication before it can serve.
pub const PROVIDER_AUTH_REQUIRED: &str = "PROVIDER_AUTH_REQUIRED";
/// The agent adapter could not be started or did not complete `session/new`.
pub const PROVIDER_UNAVAILABLE: &str = "PROVIDER_UNAVAILABLE";
/// A resume was requested while the execution still had a live actor.
pub const SESSION_ALREADY_ATTACHED: &str = "SESSION_ALREADY_ATTACHED";
/// A `session.restart` arrived while a turn was open on the execution: the
/// provider will not kill work in flight. Wait for the turn to end, or
/// interrupt it, then restart.
pub const SESSION_BUSY: &str = "SESSION_BUSY";
/// A new generation started, but the provider could not recover prior context.
pub const CONTEXT_NOT_RECOVERED: &str = "CONTEXT_NOT_RECOVERED";
/// A create named a genesis event that could not be resolved and verified.
pub const GENESIS_NOT_FOUND: &str = "GENESIS_NOT_FOUND";
/// A create named an `actor` without a `role`, or a `role` without an `actor`.
///
/// The two are a pair: an agent seat is a pubkey *and* the role it holds, and
/// half of one describes nothing a consumer can render or a provider can seat.
/// Structural, not semantic — the payload is refused before it is stored, so
/// this code appears in a relay rejection rather than in a receipt.
pub const ACTOR_ROLE_PAIR: &str = "ACTOR_ROLE_PAIR";
/// A create seated an `actor` whose key material this host does not hold.
///
/// Custody is host-local by design (plan D6): the seat's key never crosses the
/// wire, so a provider that cannot resolve it locally refuses the create rather
/// than starting an execution that is labelled as an agent and cannot act as
/// one.
pub const ACTOR_UNAVAILABLE: &str = "ACTOR_UNAVAILABLE";
/// The command signer is not authorized to operate the addressed session.
pub const UNAUTHORIZED_OPERATOR: &str = "UNAUTHORIZED_OPERATOR";
/// The addressed provider has no execution matching the requested target.
pub const UNKNOWN_TARGET: &str = "UNKNOWN_TARGET";
/// The command addressed a superseded generation of an existing execution.
pub const STALE_GENERATION: &str = "STALE_GENERATION";
/// The addressed execution was already durably stopped.
pub const SESSION_CLOSED: &str = "SESSION_CLOSED";
/// A turn could not be accepted because the execution's queue is full.
pub const QUEUE_FULL: &str = "QUEUE_FULL";
/// A turn addressed a persisted execution that has no live process behind it.
///
/// **Terminal: the turn did not run and will not run.** The provider records a
/// durable refusal beside this receipt (`record_refusal` in
/// `report_no_live_execution`, `crates/buzz-session-provider/src/lib.rs`), so a
/// replayed copy of the same 44220 is answered `AlreadyRefused` instead of
/// being run, and a `session.resume` mints generation N+1 that the replayed
/// command no longer addresses anyway. The receipt message says so in words —
/// "will not be retried; resume the execution and send it again" — and
/// re-addressing an owed turn to the successor generation is the **sender's**
/// job, not the provider's. A drop is the honest answer to "where did my turn
/// go", and it replaces the log line that used to be the only record of it.
pub const NO_LIVE_EXECUTION: &str = "NO_LIVE_EXECUTION";
/// A `steer` delivery was requested of a runtime that never advertised native
/// mid-turn steering, so the turn was delivered at the next boundary instead.
///
/// The only code a `turn_degraded` receipt carries today. Degraded is not
/// refused: the turn still runs, just later than the sender asked.
pub const STEER_UNSUPPORTED: &str = "STEER_UNSUPPORTED";
/// A `steer` reached the provider after the running turn had already ended
/// (or before one existed), so nothing was injected and the turn was
/// delivered at the next boundary. Includes the adapter's own
/// `promptRequired` answer under the idle guard.
pub const STEER_TURN_ENDED: &str = "STEER_TURN_ENDED";
/// The execution's native-steer admission is full, so this input was not
/// delivered; nothing was written to the runtime. A `turn_dropped` code —
/// terminal like [`QUEUE_FULL`] — the sender sends it again once it drains.
pub const STEER_SATURATED: &str = "STEER_SATURATED";
/// The runtime answered the steer request with an explicit JSON-RPC error
/// that proves nothing was injected; delivered at the next boundary.
pub const STEER_REJECTED: &str = "STEER_REJECTED";
/// A `steer` carried image attachments, which the native injection path does
/// not take; delivered at the next boundary with its images.
pub const STEER_ATTACHMENTS_UNSUPPORTED: &str = "STEER_ATTACHMENTS_UNSUPPORTED";
/// A late runtime answer proved an input whose delivery had been reported
/// unknown never reached the turn. Terminal: the sender sends it again.
pub const STEER_NOT_DELIVERED: &str = "STEER_NOT_DELIVERED";
/// The steer request's write to the runtime failed part way; bytes may have
/// reached it. Delivery unknown, never replayed automatically.
pub const STEER_WRITE_FAILED: &str = "STEER_WRITE_FAILED";
/// The steer request was written and the prompt ended, or the runtime
/// exited, before its acknowledgement arrived. Delivery unknown.
pub const STEER_ACK_LOST: &str = "STEER_ACK_LOST";
/// The steer request was written and the bounded wait for its
/// acknowledgement expired. Delivery unknown.
pub const STEER_ACK_TIMEOUT: &str = "STEER_ACK_TIMEOUT";
/// The runtime acknowledged the steer request with a result that names no
/// recognized outcome (a bare `{}`, or its own `failed`). Delivery unknown.
pub const STEER_ACK_UNRECOGNIZED: &str = "STEER_ACK_UNRECOGNIZED";
/// The provider restarted with a native-steer intent that no acknowledgement
/// ever resolved. Delivery unknown.
pub const STEER_UNRESOLVED_AT_RESTART: &str = "STEER_UNRESOLVED_AT_RESTART";
/// The runtime reports it started a separate turn with this input, one this
/// provider does not observe. The input was delivered and must not be
/// resent; its output may be absent from the published transcript.
pub const STEER_UNOBSERVED_NEW_TURN: &str = "STEER_UNOBSERVED_NEW_TURN";
/// A turn carried image attachments, but the runtime behind this execution
/// never advertised image prompts, so the turn was delivered as text only.
///
/// Like [`STEER_UNSUPPORTED`], degraded is not refused: the words still reach
/// the agent. Saying so is the point — an image that silently never arrived is
/// indistinguishable from an agent that looked at it and said nothing.
pub const IMAGE_UNSUPPORTED: &str = "IMAGE_UNSUPPORTED";
/// An interrupt addressed a live execution that had no turn in flight, so
/// there was nothing to cancel.
pub const NO_TURN_IN_FLIGHT: &str = "NO_TURN_IN_FLIGHT";
/// A second turn command carried an identifier-only team-wake pointer that is
/// byte-equal — after JSON canonicalisation — to one already custodied or
/// consumed for the *same exact target* (driver, instance, session,
/// generation).
///
/// **Refused, and it spent zero turns.** The runner fences an operation, not
/// just a `commandId`: the provider's wake sender and the founder's Desktop
/// fallback deliberately mint the same pointer text under different command
/// ids, so without this code a producer bug on either side spends a second
/// turn of the lead's context on a fact it already has. The receipt's
/// `message` names the command that owns the operation, so the refused sender
/// can join its own intent to the delivery that actually happened.
///
/// **Both producers treat this refusal of their own command as settlement of
/// the operation, never as a failure.** Something is delivering the wake — it
/// is simply not this command. Counting it as a delivery failure would re-arm
/// a fallback against a turn that is already queued.
pub const DUPLICATE_OPERATION: &str = "DUPLICATE_OPERATION";
/// A turn was refused because its umbrella has spent its turn budget (D9).
///
/// **Terminal for this command, and about the umbrella rather than the
/// sender's authority.** The signer may steer the execution — that was checked
/// first — but the umbrella named by the create's `sessionRef` has already
/// started as many turns as the host allowed it, so the provider refuses
/// rather than letting a crew run without a floor under it. The receipt
/// message carries the two numbers (`used` of `limit`) so nobody has to guess
/// how far past the line they are, and the same pair is republished in every
/// 44223 as [`TurnBudget`].
///
/// The founder is exempt by construction: a budget is a bound on delegated
/// work, and a human who wants one more turn on their own session is the
/// person the budget was protecting. Nothing here bounds *interrupts* — a
/// cancel spends nothing and refusing one would leave a runaway turn running
/// with no way to stop it short of stopping the execution.
pub const BUDGET_EXHAUSTED: &str = "BUDGET_EXHAUSTED";
/// A CI continuation reached its `expiresAt` with no result recorded for the
/// exact run attempt it named.
///
/// **Terminal for the registration, and it says nothing about the run.** The
/// check may still be queued, still running, or may have been recorded under a
/// different attempt; what the provider knows is only that it waited the
/// window the sender chose and nothing arrived. Re-registering with a later
/// horizon is the sender's call, and mints a new registration rather than
/// resurrecting this one.
pub const CI_CONTINUATION_EXPIRED: &str = "CI_CONTINUATION_EXPIRED";
/// Two or more *different* relay-signed results exist for one CI correlation
/// digest, so there is no single fact to deliver.
///
/// A result is meant to be immutable per identity: one run attempt, one
/// terminal conclusion. When the relay's store answers with more than one
/// canonical result for the same digest, the provider refuses rather than
/// picking a winner — delivering "the first one" would let whichever producer
/// wrote first decide what the agent believes about a build.
pub const CI_RESULT_CONFLICT: &str = "CI_RESULT_CONFLICT";
/// The provider's window closed without ever being able to *see* results for
/// the named project, so "not finished" and "not permitted to read" are
/// indistinguishable.
///
/// Results are private-project gated for the reader, and the listener reads
/// with the provider's own key — it borrows no credential from the registering
/// signer. When every check in the window returned no rows for a project the
/// provider may not read, this code says exactly that instead of implying the
/// check never completed.
pub const CI_RESULT_UNAVAILABLE_OR_HIDDEN: &str = "CI_RESULT_UNAVAILABLE_OR_HIDDEN";
/// A second registration reused a `commandId` already durably held for a
/// *different* payload.
///
/// The id of a CI continuation is derived from everything that changes what
/// would be delivered, so an exact retry is idempotent and collides with
/// nothing. A collision therefore means two different intents named one
/// registration; the first durable record wins and the second is refused,
/// because silently overwriting it would change what a pending turn will say
/// after its sender was told it was registered.
pub const COMMAND_ID_CONFLICT: &str = "COMMAND_ID_CONFLICT";
/// The provider's durable continuation store is at its bound, so the
/// registration was refused before it was acknowledged.
///
/// Refused, not queued: a bounded store that accepted one more registration by
/// evicting another would silently drop a turn somebody was already told would
/// run. The bound exists because each pending record costs a live
/// subscription filter and a slot in the file the provider must rewrite
/// atomically.
pub const CI_CONTINUATION_STORE_FULL: &str = "CI_CONTINUATION_STORE_FULL";
/// The umbrella this execution belongs to has been handed over, and this
/// command is not the claimant acting on the claimed body.
///
/// Answered by the **provider**, from the claim it folded out of the accepted
/// authority chain
/// ([`crate::coding_session_authority_claim::ClaimState`]) — never by the
/// relay, which stores the claim link and adjudicates nothing about
/// executions. Three situations produce it, and a surface should say which:
/// this provider is not the claimed body, the sender is not the claimant, or
/// the claim was voided and nobody holds the session until a fresh accepted
/// `takeover`/`transfer` is published.
///
/// It exists because the alternative is worse than a refusal: a machine that
/// comes back online and silently resumes work somebody else has taken over
/// produces two divergent executions of one task, and nothing on the wire
/// says which one is real.
pub const HANDOVER_FENCED: &str = "HANDOVER_FENCED";
/// The umbrella has been deleted, so nothing under it runs again.
///
/// Answered by the **provider**, and only from an accepted deletion it
/// verified: a relay-signed deletion receipt naming this genesis, or a kind 5
/// signed by the record's own founder together with an authenticated read
/// showing the relay applied it (`docs/HANDOVER_IMPL.md` §3.2). A missing or
/// failed read is never retirement authority — "I could not check" and "it was
/// deleted" are different answers, and only one of them stops a session.
///
/// Terminal: a retired record publishes no metadata, is skipped by seat
/// requests and restaging, and answers this to every command. Nothing is
/// republished or reconstructed to make a deleted session resumable.
pub const SESSION_RETIRED: &str = "SESSION_RETIRED";

/// Ceiling on a receipt error code, in UTF-8 bytes.
///
/// Turn-stage receipts no longer pin a closed list of codes — a provider that
/// learns a new way to refuse a turn must be able to say so, and every current
/// consumer already renders an unknown code verbatim. What stays enforced is
/// that a code is a *code*: nonblank, free of control characters, and short
/// enough to sit in a badge.
pub const MAX_RECEIPT_ERROR_CODE_BYTES: usize = 64;

/// Outcome of exactly one coding-session command (kind 44224).
///
/// Two vocabularies share this event. The **lifecycle** statuses answer one
/// 44221 create, resume, or stop with exactly one terminal outcome. The
/// **turn** statuses ([`ReceiptStatus::is_turn_stage`]) answer one 44220
/// `thread.turn.start` and are *per stage*: a single turn command can produce
/// `turn_queued` and then `turn_started`, or a single `turn_dropped` /
/// `turn_refused`. A consumer therefore keys a turn receipt by
/// `(commandId, status)`, never by `commandId` alone, and a fold that decides
/// what happened to a *generation* ignores the turn statuses entirely — a turn
/// receipt never creates, confirms, or ends a generation.
///
/// ## Accepted JSON shapes
///
/// There are exactly two, and the strict decoder accepts nothing between or
/// beyond them:
///
/// 1. The five keys `{schema, commandId, status, session, error}` — every
///    lifecycle status, plus `turn_queued`, `turn_dropped`, `turn_refused`.
/// 2. Those five plus `turnId` — `turn_started` and nothing else.
///
/// `turnId` is present *exactly* when the status is `turn_started`: a started
/// receipt without one names no turn, and any other status carrying one claims
/// a turn that has not begun. The key is additive and omitted rather than
/// serialized as `null`, so shape 1 is byte-identical to the shape shipped
/// before the turn vocabulary existed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleReceipt {
    /// Always [`LIFECYCLE_RECEIPT_SCHEMA`].
    pub schema: String,
    /// The `commandId` of the lifecycle or turn command this answers.
    pub command_id: String,
    /// Exact outcome, or — for the turn vocabulary — exact stage.
    pub status: ReceiptStatus,
    /// The addressed target, or `null` when a create failed outright. Never
    /// `null` for a turn status: a turn receipt always names the execution the
    /// 44220 addressed.
    pub session: Option<CodingSessionTarget>,
    /// Failure detail, or `null` for a clean outcome.
    pub error: Option<ReceiptError>,
    /// The provider's turn id, present exactly when `status` is
    /// `turn_started`. Omitted entirely otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// Receipt outcomes recognized by current consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiptStatus {
    /// Session exists and, if a first turn was requested, it was delivered.
    #[serde(rename = "created")]
    Created,
    /// Session exists but its requested first turn failed.
    #[serde(rename = "created_with_failed_initial_turn")]
    CreatedWithFailedInitialTurn,
    /// No session exists.
    #[serde(rename = "failed")]
    Failed,
    /// A disconnected execution reattached with its prior provider context.
    #[serde(rename = "resumed")]
    Resumed,
    /// A new generation attached with fresh provider context.
    #[serde(rename = "resumed_without_context")]
    ResumedWithoutContext,
    /// The execution was durably stopped.
    #[serde(rename = "stopped")]
    Stopped,
    /// A turn command was accepted into the execution's mailbox.
    #[serde(rename = "turn_queued")]
    TurnQueued,
    /// The accepted turn began running; carries the provider's `turnId`.
    #[serde(rename = "turn_started")]
    TurnStarted,
    /// The turn was not accepted and will not run: the mailbox was full
    /// ([`QUEUE_FULL`], or `QUEUE_FULL_TURN_KEPT` for an interrupt that needed
    /// two slots), or the addressed execution had no live process
    /// ([`NO_LIVE_EXECUTION`]). Terminal in every case; `error.code` says
    /// which, and nothing in the status itself marks it terminal.
    #[serde(rename = "turn_dropped")]
    TurnDropped,
    /// The turn was refused: the signer lacked authority, or the target was
    /// unknown, superseded, or closed.
    #[serde(rename = "turn_refused")]
    TurnRefused,
    /// The turn was accepted, but not in the class the sender asked for — a
    /// `steer` this runtime cannot honour, delivered at the next boundary
    /// instead. A `turn_queued` follows; the turn is not lost.
    #[serde(rename = "turn_degraded")]
    TurnDegraded,
    /// A `steer` was injected into the turn already running; carries that
    /// turn's `turnId`. The original turn keeps its ownership, streaming
    /// state and accounting; a `user_prompt{steered:true}` echo precedes it.
    #[serde(rename = "turn_injected")]
    TurnInjected,
    /// A `steer` was written to the runtime and its delivery could not be
    /// established. Terminal for the provider — never replayed automatically;
    /// `error.code` says why. A later receipt under the same `commandId` may
    /// reconcile it.
    #[serde(rename = "turn_delivery_unknown")]
    TurnDeliveryUnknown,
    /// A `thread.turn.interrupt` reached a live turn and its cancel was
    /// issued. The turn's own `result` item reports how it actually ended.
    #[serde(rename = "interrupt_delivered")]
    InterruptDelivered,
    /// A `thread.turn.continue_on_ci` was validated and its registration
    /// durably stored.
    ///
    /// **Not a mailbox stage and not terminal.** Nothing is queued, no budget
    /// is spent, and no turn exists yet: the provider has only promised to
    /// watch for one exact CI result until the registration's `expiresAt`. If
    /// the result arrives and the signer may still steer the target then, the
    /// ordinary `turn_queued`/`turn_started` stages follow under the same
    /// `commandId`; otherwise a `turn_refused` names why. A consumer that
    /// treats this as delivery would report a turn that has not been accepted
    /// by anything.
    #[serde(rename = "continuation_registered")]
    ContinuationRegistered,
}

impl ReceiptStatus {
    /// The exact wire string this status serializes as.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::CreatedWithFailedInitialTurn => "created_with_failed_initial_turn",
            Self::Failed => "failed",
            Self::Resumed => "resumed",
            Self::ResumedWithoutContext => "resumed_without_context",
            Self::Stopped => "stopped",
            Self::TurnQueued => "turn_queued",
            Self::TurnStarted => "turn_started",
            Self::TurnDropped => "turn_dropped",
            Self::TurnRefused => "turn_refused",
            Self::TurnDegraded => "turn_degraded",
            Self::TurnInjected => "turn_injected",
            Self::TurnDeliveryUnknown => "turn_delivery_unknown",
            Self::InterruptDelivered => "interrupt_delivered",
            Self::ContinuationRegistered => "continuation_registered",
        }
    }

    /// Whether this status carries the sixth `turnId` key: `turn_started`
    /// names the turn that began, `turn_injected` the turn the input joined.
    pub const fn carries_turn_id(self) -> bool {
        matches!(self, Self::TurnStarted | Self::TurnInjected)
    }

    /// Whether this status reports a *stage* of one 44220 turn command rather
    /// than the terminal outcome of one 44221 lifecycle command.
    ///
    /// A fold that decides the status of a session generation must skip these:
    /// one turn command produces up to three of them, none of them creates,
    /// confirms, or ends a generation.
    pub const fn is_turn_stage(self) -> bool {
        matches!(
            self,
            Self::TurnQueued
                | Self::TurnStarted
                | Self::TurnDropped
                | Self::TurnRefused
                | Self::TurnDegraded
                | Self::TurnInjected
                | Self::TurnDeliveryUnknown
                | Self::InterruptDelivered
                | Self::ContinuationRegistered
        )
    }
}

/// Machine-readable code plus an operator-facing message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptError {
    /// Stable code; the consumer branches on `INITIAL_TURN_FAILED` specifically.
    pub code: String,
    /// Human-readable detail. Never empty — the consumer rejects a blank message.
    pub message: String,
}

impl LifecycleReceipt {
    /// A session was created and any requested first turn was delivered.
    pub fn created(command_id: &str, target: &CodingSessionTarget) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::Created,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        }
    }

    /// A session was created but its requested first turn failed.
    pub fn created_with_failed_initial_turn(
        command_id: &str,
        target: &CodingSessionTarget,
        message: &str,
    ) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::CreatedWithFailedInitialTurn,
            session: Some(target.clone()),
            error: Some(ReceiptError {
                code: INITIAL_TURN_FAILED.to_owned(),
                message: bounded_message(message),
            }),
            turn_id: None,
        }
    }

    /// No session was created.
    pub fn failed(command_id: &str, code: &str, message: &str) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::Failed,
            session: None,
            error: Some(ReceiptError {
                code: code.to_owned(),
                message: bounded_message(message),
            }),
            turn_id: None,
        }
    }

    /// A disconnected execution reattached with its prior provider context.
    pub fn resumed(command_id: &str, target: &CodingSessionTarget) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::Resumed,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        }
    }

    /// A new generation attached, but the adapter could not recover context.
    pub fn resumed_without_context(
        command_id: &str,
        target: &CodingSessionTarget,
        message: &str,
    ) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::ResumedWithoutContext,
            session: Some(target.clone()),
            error: Some(ReceiptError {
                code: CONTEXT_NOT_RECOVERED.to_owned(),
                message: bounded_message(message),
            }),
            turn_id: None,
        }
    }

    /// An execution was durably stopped.
    pub fn stopped(command_id: &str, target: &CodingSessionTarget) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::Stopped,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        }
    }

    /// A turn command was accepted into the execution's mailbox.
    ///
    /// Says the provider took custody of the turn, not that it ran — the turn
    /// may still be waiting behind another. `turn_started` is the stage that
    /// says it began.
    pub fn turn_queued(command_id: &str, target: &CodingSessionTarget) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::TurnQueued,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        }
    }

    /// The accepted turn began running under `turn_id`.
    ///
    /// `turn_id` is the provider's minted id, the same one every transcript
    /// item of this turn carries, so a consumer joins the receipt to the
    /// transcript without matching text.
    pub fn turn_started(command_id: &str, target: &CodingSessionTarget, turn_id: &str) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::TurnStarted,
            session: Some(target.clone()),
            error: None,
            turn_id: Some(turn_id.to_owned()),
        }
    }

    /// A `steer` was injected into the turn already running under `turn_id`.
    ///
    /// Says the runtime positively acknowledged the input as joined into that
    /// turn. It is not a new turn: no second `turn_started`, no new
    /// accounting, and the `user_prompt{steered:true}` echo that precedes
    /// this receipt is what settles the sender's pending row.
    pub fn turn_injected(command_id: &str, target: &CodingSessionTarget, turn_id: &str) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::TurnInjected,
            session: Some(target.clone()),
            error: None,
            turn_id: Some(turn_id.to_owned()),
        }
    }

    /// A `steer` was written to the runtime and nothing establishes whether
    /// it arrived.
    ///
    /// Terminal from the provider's side: the command is answered and never
    /// replayed automatically, because a replay of an input the runtime may
    /// already hold is the double delivery the classes exist to prevent. The
    /// sender decides whether to send again. `code` is one of the
    /// `STEER_*` unknown codes documented in NIP-CSL.
    pub fn turn_delivery_unknown(
        command_id: &str,
        target: &CodingSessionTarget,
        code: &str,
        message: &str,
    ) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::TurnDeliveryUnknown,
            session: Some(target.clone()),
            error: Some(ReceiptError {
                code: code.to_owned(),
                message: bounded_message(message),
            }),
            turn_id: None,
        }
    }

    /// The turn was accepted by the provider and then not run: the queue was
    /// full ([`QUEUE_FULL`]), or nothing live was there to run it
    /// ([`NO_LIVE_EXECUTION`]).
    ///
    /// The code is open rather than pinned. A provider that learns a new way
    /// to lose a turn must be able to name it, and a code the consumer does
    /// not recognize renders verbatim — which is strictly better than the turn
    /// vanishing into a log line.
    pub fn turn_dropped(
        command_id: &str,
        target: &CodingSessionTarget,
        code: &str,
        message: &str,
    ) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::TurnDropped,
            session: Some(target.clone()),
            error: Some(ReceiptError {
                code: code.to_owned(),
                message: bounded_message(message),
            }),
            turn_id: None,
        }
    }

    /// The turn was accepted, but downgraded out of the class the sender
    /// asked for.
    ///
    /// Two codes reach here. [`STEER_UNSUPPORTED`]: a `steer` addressed to a
    /// runtime that never advertised native mid-turn steering. The turn is not
    /// refused and not lost — a `turn_queued` follows and it runs at the next
    /// boundary. Saying so is the whole point: a silent downgrade would let an
    /// operator believe the agent was steered mid-thought.
    /// [`IMAGE_UNSUPPORTED`]: the turn's attachments were dropped because the
    /// runtime does not take image prompts, for the same reason — an operator
    /// must not be left believing the agent saw a picture it never received.
    pub fn turn_degraded(
        command_id: &str,
        target: &CodingSessionTarget,
        code: &str,
        message: &str,
    ) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::TurnDegraded,
            session: Some(target.clone()),
            error: Some(ReceiptError {
                code: code.to_owned(),
                message: bounded_message(message),
            }),
            turn_id: None,
        }
    }

    /// A `thread.turn.interrupt` reached a live turn and its cancel was
    /// issued.
    ///
    /// Says the cancel was delivered, not that the agent has stopped — the
    /// turn's own `result` item reports how it actually ended. An interrupt
    /// that reached no live turn is a `turn_refused`, never this.
    pub fn interrupt_delivered(command_id: &str, target: &CodingSessionTarget) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::InterruptDelivered,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        }
    }

    /// A `thread.turn.continue_on_ci` was validated and durably stored.
    ///
    /// Says the provider took custody of the *registration*, not of a turn.
    /// The turn, if the named result is recorded in time and the signer may
    /// still steer the target then, is answered by the ordinary turn stages
    /// under this same `commandId`.
    pub fn continuation_registered(command_id: &str, target: &CodingSessionTarget) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::ContinuationRegistered,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        }
    }

    /// The turn was refused before it reached the execution.
    ///
    /// `code` is documented in NIP-CSL — [`UNAUTHORIZED_OPERATOR`],
    /// [`UNKNOWN_TARGET`], [`STALE_GENERATION`], [`SESSION_CLOSED`],
    /// [`NO_TURN_IN_FLIGHT`] — but the field is open: the decoder requires a
    /// well-formed code, not a member of a list this build happens to know.
    /// `target` is the target the 44220 addressed, which is a fact even when
    /// no execution answers to it.
    pub fn turn_refused(
        command_id: &str,
        target: &CodingSessionTarget,
        code: &str,
        message: &str,
    ) -> Self {
        Self {
            schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
            command_id: command_id.to_owned(),
            status: ReceiptStatus::TurnRefused,
            session: Some(target.clone()),
            error: Some(ReceiptError {
                code: code.to_owned(),
                message: bounded_message(message),
            }),
            turn_id: None,
        }
    }
}

/// Strictly decode and validate one immutable coding-session receipt.
///
/// Accepts exactly the two key-set shapes documented on [`LifecycleReceipt`]:
/// the five base keys, or those five plus `turnId` when — and only when — the
/// status is `turn_started`. Anything else, including a status/`turnId`
/// mismatch in either direction, is a hard rejection rather than a tolerated
/// half-truth.
pub fn decode_coding_session_lifecycle_receipt(content: &str) -> Result<LifecycleReceipt, String> {
    if content.len() > MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES {
        return Err(format!(
            "coding-session lifecycle receipt exceeds {MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES} bytes"
        ));
    }
    let value: serde_json::Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session lifecycle receipt".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "coding-session lifecycle receipt must be an object".to_owned())?;
    const FIELDS: [&str; 5] = ["schema", "commandId", "status", "session", "error"];
    const TURN_ID_FIELD: &str = "turnId";
    let carries_turn_id = object.contains_key(TURN_ID_FIELD);
    let expected_len = FIELDS.len() + usize::from(carries_turn_id);
    if object.len() != expected_len
        || FIELDS.iter().any(|field| !object.contains_key(*field))
        || object
            .keys()
            .any(|field| !FIELDS.contains(&field.as_str()) && field != TURN_ID_FIELD)
    {
        return Err("coding-session lifecycle receipt has missing or unsupported fields".into());
    }
    let receipt: LifecycleReceipt = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session lifecycle receipt: {error}"))?;
    // Keyed on the *key's* presence, not the parsed value: an explicit
    // `"turnId": null` is a six-key object claiming the provider observed "no
    // turn", which is a different claim from the five-key shape that carries
    // no such key at all. Only the strict decoder can tell them apart —
    // `Option<String>` has collapsed both to `None` by the time
    // `validate_lifecycle_receipt` sees the struct.
    if carries_turn_id != receipt.status.carries_turn_id() {
        return Err(
            "receipt turnId key is present exactly when status is turn_started or turn_injected"
                .into(),
        );
    }
    validate_lifecycle_receipt(&receipt)?;
    Ok(receipt)
}

/// Whether a turn-stage receipt error code is well formed: nonblank, free of
/// control characters, and within [`MAX_RECEIPT_ERROR_CODE_BYTES`].
///
/// This replaced the closed per-status code lists. The known codes stay
/// documented in NIP-CSL; the wire check is a shape check.
fn is_receipt_error_code(code: &str) -> bool {
    !code.trim().is_empty()
        && code.len() <= MAX_RECEIPT_ERROR_CODE_BYTES
        && !code.chars().any(char::is_control)
}

fn validate_lifecycle_receipt(receipt: &LifecycleReceipt) -> Result<(), String> {
    use crate::coding_session_command::{
        CodingSessionAction, CodingSessionCommandPayload, CODING_SESSION_COMMAND_SCHEMA,
        MAX_IDENTIFIER_BYTES,
    };

    if receipt.schema != LIFECYCLE_RECEIPT_SCHEMA {
        return Err("unsupported coding-session lifecycle receipt schema".into());
    }
    if receipt.command_id.trim().is_empty() || receipt.command_id.len() > MAX_IDENTIFIER_BYTES {
        return Err("receipt commandId must be a nonempty bounded identifier".into());
    }
    if let Some(target) = &receipt.session {
        CodingSessionCommandPayload {
            schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
            command_id: receipt.command_id.clone(),
            target: target.clone(),
            action: CodingSessionAction::ThreadTurnInterrupt,
        }
        .validate()?;
    }
    if let Some(error) = &receipt.error {
        if error.code.trim().is_empty() || error.code.len() > MAX_IDENTIFIER_BYTES {
            return Err("receipt error code must be a nonempty bounded identifier".into());
        }
        if error.message.trim().is_empty() || error.message.len() > 1024 + '…'.len_utf8() {
            return Err("receipt error message must be nonempty and bounded".into());
        }
    }
    // `turnId` is present exactly when the turn started: a `turn_started`
    // without one names no turn, and any other status carrying one claims a
    // turn that has not begun.
    let expects_turn_id = receipt.status.carries_turn_id();
    match (&receipt.turn_id, expects_turn_id) {
        (Some(turn_id), true) => {
            if turn_id.trim().is_empty()
                || turn_id.len() > MAX_IDENTIFIER_BYTES
                || turn_id.chars().any(char::is_control)
            {
                return Err("receipt turnId must be a nonempty bounded identifier".into());
            }
        }
        (None, false) => {}
        _ => {
            return Err(
                "receipt turnId is present exactly when status is turn_started or turn_injected"
                    .into(),
            );
        }
    }
    let valid_shape = match receipt.status {
        ReceiptStatus::Created | ReceiptStatus::Resumed | ReceiptStatus::Stopped => {
            receipt.session.is_some() && receipt.error.is_none()
        }
        ReceiptStatus::CreatedWithFailedInitialTurn => {
            receipt.session.is_some()
                && receipt
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == INITIAL_TURN_FAILED)
        }
        ReceiptStatus::ResumedWithoutContext => {
            receipt.session.is_some()
                && receipt
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == CONTEXT_NOT_RECOVERED)
        }
        ReceiptStatus::Failed => receipt.session.is_none() && receipt.error.is_some(),
        // Every turn status names the execution the 44220 addressed, even the
        // refusals: "which session did this refer to" is the first thing an
        // operator asks.
        ReceiptStatus::TurnQueued | ReceiptStatus::TurnStarted | ReceiptStatus::TurnInjected => {
            receipt.session.is_some() && receipt.error.is_none()
        }
        // Open codes, deliberately. Pinning a list here meant a provider that
        // learned a new way to lose or refuse a turn could not report it
        // without a coordinated release of every reader — and the desktop
        // already renders an unknown code verbatim. What is still enforced is
        // that the code is well formed.
        ReceiptStatus::TurnDropped
        | ReceiptStatus::TurnRefused
        | ReceiptStatus::TurnDegraded
        | ReceiptStatus::TurnDeliveryUnknown => {
            receipt.session.is_some()
                && receipt
                    .error
                    .as_ref()
                    .is_some_and(|error| is_receipt_error_code(&error.code))
        }
        ReceiptStatus::InterruptDelivered => receipt.session.is_some() && receipt.error.is_none(),
        // A registration names the execution its eventual turn would address
        // and carries no failure: the refusals a continuation can earn all
        // arrive later, as turn stages of the same command.
        ReceiptStatus::ContinuationRegistered => {
            receipt.session.is_some() && receipt.error.is_none()
        }
    };
    if !valid_shape {
        return Err("lifecycle receipt status/session/error shape is inconsistent".into());
    }
    Ok(())
}

/// Lifecycle status of one session generation, as the consumer models it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Subprocess is starting; no turn has run yet.
    Starting,
    /// Live and awaiting a turn.
    Idle,
    /// A turn is in flight.
    Running,
    /// The agent asked for operator input.
    WaitingForInput,
    /// The session finished normally and will accept no more turns.
    Completed,
    /// The operator durably stopped the execution.
    Stopped,
    /// The session ended in an error.
    Failed,
    /// A turn was interrupted.
    Interrupted,
    /// The provider is no longer attached to this generation.
    Disconnected,
    /// Status could not be determined.
    Unknown,
}

/// What this provider can be asked to do with a session.
///
/// Advertised identically in the catalog (44222) and in per-generation metadata
/// (44223), so an operator's picker and the live session header can never
/// disagree about which controls are real.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// `thread.turn.start` is honored.
    pub thread_turn_start: bool,
    /// An in-flight turn can be interrupted.
    pub thread_turn_interrupt: bool,
    /// Mid-turn steering without cancelling.
    ///
    /// **Per-execution truth, not a product-level promise.** The provider
    /// learns this at `initialize` from the runtime actually behind *this*
    /// generation and republishes it in that generation's metadata (44223).
    /// Two executions of the same driver can legitimately disagree — one
    /// adapter build advertises `_meta.steering.supported` and an older one on
    /// the same host does not. A consumer that offers a "Steer" control reads
    /// it from the execution's metadata, never from the catalog's static
    /// vector.
    pub thread_steer: bool,
    /// Context-window summaries are published. Not offered in v1.
    pub context: bool,
    /// Diff summaries are published. Not offered in v1.
    pub diff: bool,
    /// Plan updates are published.
    pub plan: bool,
    /// The runtime accepts `image` content blocks in `session/prompt`.
    ///
    /// **Per-execution truth, exactly like [`thread_steer`](Self::thread_steer).**
    /// It is what the process behind *this* generation advertised at
    /// `initialize` (`agentCapabilities.promptCapabilities.image`), not a
    /// property of the driver: `claude-agent-acp` and an in-house `buzz-agent`
    /// build can legitimately disagree. A consumer that offers an "attach
    /// image" control reads it from the execution's metadata (44223).
    ///
    /// Defaulted so that metadata published before this field existed still
    /// decodes — as `false`, which is the honest reading of a provider that
    /// never claimed image support.
    #[serde(default)]
    pub prompt_image: bool,
}

impl Capabilities {
    /// The v1 vector for the named runtime slug; unknown slugs get the
    /// conservative baseline.
    pub fn v1_for_runtime(runtime: &str) -> Self {
        match runtime {
            "claude" => Self::v1_claude(),
            _ => Self::v1_baseline(),
        }
    }

    /// The exact capability set the Claude runtime offers in v1.
    pub const fn v1_claude() -> Self {
        Self {
            thread_turn_start: true,
            thread_turn_interrupt: true,
            thread_steer: false,
            context: false,
            diff: false,
            plan: true,
            prompt_image: false,
        }
    }

    /// The conservative v1 baseline: like [`Capabilities::v1_claude`] but with
    /// `plan: false` — a capability turns on only once its transcript path is
    /// proven end to end for that runtime.
    pub const fn v1_baseline() -> Self {
        Self {
            thread_turn_start: true,
            thread_turn_interrupt: true,
            thread_steer: false,
            context: false,
            diff: false,
            plan: false,
            prompt_image: false,
        }
    }

    /// The same vector with `threadSteer` set to what *this* execution's
    /// runtime advertised at `initialize`.
    ///
    /// The static vectors above are what a driver offers in general; this is
    /// what the process behind one generation actually answered. Metadata for
    /// a live generation must publish the latter — a `true` an operator's
    /// Steer button relies on has to have been witnessed, and a `false` on a
    /// runtime that does steer needlessly hides a working control.
    pub const fn with_thread_steer(self, thread_steer: bool) -> Self {
        Self {
            thread_steer,
            ..self
        }
    }

    /// The same vector with `promptImage` set to what *this* execution's
    /// runtime advertised at `initialize`.
    ///
    /// Same reasoning as [`with_thread_steer`](Self::with_thread_steer): an
    /// attach control that publishes an image the runtime will refuse is worse
    /// than no control, and hiding the control on a runtime that does take
    /// images costs the operator a capability they paid for.
    pub const fn with_prompt_image(self, prompt_image: bool) -> Self {
        Self {
            prompt_image,
            ..self
        }
    }

    /// The exact capability set this provider offers in v1.
    #[deprecated(note = "renamed to v1_claude")]
    pub const fn claude_agent_acp() -> Self {
        Self::v1_claude()
    }
}

/// Immutable facts about one session generation (kind 44223).
///
/// Field order is the consumer's declared key order; `agentRef` is
/// structurally required and always `null` here — this provider is not a
/// managed agent.
///
/// # B1: coordinate facts (D4a)
///
/// `observedCommit`, `dirty`, `relayReachable`, and `verifiedAt` land here
/// rather than on [`LifecycleReceipt`] because their cadence matches this
/// event's, not the receipt's: [`crate::coding_session_payload`]'s consumer
/// contract already republishes metadata at generation start and at every
/// turn end (`Provider::spawn_git_probe`'s call sites), which is exactly the
/// cadence D4a specifies for these facts. A lifecycle receipt answers one
/// `commandId` once — create, resume, or stop — and is never republished as
/// the working tree changes underneath a long-running generation, so it
/// would go stale as a home for a per-turn fact the moment the second turn
/// started. `repoRef` is not repeated in that list: it is the existing field
/// above, reused rather than re-derived, per instructions.
///
/// Five separate facts, never collapsed into one "recoverable" claim:
/// `observedCommit` is what `git` reported the local `HEAD` to be;
/// `repoRef` is the repository coordinate already carried by the session;
/// `dirty` is whether the worktree had uncommitted changes at that same
/// observation (honesty rule: dirty is recorded as dirty, never inferred
/// away — a probe that could not run leaves this `null` rather than
/// guessing `false`); `relayReachable` is whether the relay's git storage
/// was confirmed to already hold `observedCommit`; `verifiedAt` is when
/// that confirmation happened. "Recoverable" is a UI-side word for
/// `relayReachable == true`, never asserted by the provider itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    /// Always [`METADATA_SCHEMA`].
    pub schema: String,
    /// The generation these facts describe.
    pub session: CodingSessionTarget,
    /// NIP-MP project coordinate, or `null` for a standalone session.
    pub project_ref: Option<String>,
    /// Repository coordinate within the project, or `null`.
    pub repo_ref: Option<String>,
    /// Operator-facing title, or `null`.
    pub title: Option<String>,
    /// The agent seat this execution runs as, lowercase 64-hex, or `null`.
    ///
    /// `null` is still the answer for every human-created execution: that work
    /// is supervised by the provider, not performed by a Buzz participant
    /// acting as itself. A non-null value is the seat named by the create's
    /// `actor` (plan D1) and is the *only* thing about that seat that is
    /// published — its key material is resolved host-locally and never
    /// crosses the wire.
    pub agent_ref: Option<String>,
    /// The role slug this seat holds within its umbrella, or `null`.
    ///
    /// The one additive key of this amendment: emitted only alongside a
    /// non-null `agentRef`, never as an explicit `null`, so metadata for an
    /// unseated execution keeps the exact key sets pre-amendment consumers
    /// require. Validated as
    /// [`crate::coding_session_lifecycle_command::validate_role_slug`] does,
    /// because it is the same value echoed from the create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Advertised provider instance **alias** (`claude-primary`), or `null`.
    ///
    /// Typed since batch 2 lane B2: an alias is not an instance id, and
    /// `session.instanceId` on the same struct is one. The newtype is
    /// `#[serde(transparent)]`, so no signed metadata changed shape.
    ///
    /// **The key is always written, `null` included** — deliberately no
    /// `skip_serializing_if`. Kind 44223 is an exact-key contract on both
    /// sides: `provider` and `runtime` are in the *required* list of the
    /// Desktop decoder's `hasRequiredAndOptionalKeys` check
    /// (`desktop/src/features/coding-sessions/lib/codingSessionIngressPayloads.ts`,
    /// `parseBuzzCodingSessionMetadata`), so omitting either would make **every**
    /// metadata event from an updated provider fail to decode on Desktop, not
    /// just the ones with nothing to say. Checked, not assumed (REVIEW-B2 F8).
    pub provider: Option<crate::coding_session_identity::ProviderInstanceAlias>,
    /// **Runtime word** behind the driver (`claude`, `codex`), or `null`.
    ///
    /// Typed since batch 2 lane B2 so it can never be compared with
    /// `session.driver`, which is a driver slug (`claude-agent-acp`).
    ///
    /// Always written, `null` included, for the same reason as
    /// [`provider`](Self::provider).
    pub runtime: Option<crate::coding_session_identity::RuntimeWord>,
    /// Effective model, or `null` when the adapter chose its own.
    pub model: Option<String>,
    /// Current lifecycle status.
    pub status: SessionStatus,
    /// Checked-out branch, from the same bounded worktree probe as
    /// `observedCommit`/`dirty`, or `null` when not observed (no
    /// repository, a detached `HEAD`, or the probe has not completed yet).
    pub branch: Option<String>,
    /// Capabilities in force for this generation.
    pub capabilities: Capabilities,
    /// Umbrella session reference echoed from the create, when one was claimed.
    ///
    /// This is the one *optional* key in an otherwise exact-key contract:
    /// emitted only when the create carried a non-null `sessionRef`, never as
    /// an explicit `null`. Pre-amendment consumers' exact-key check therefore
    /// keeps accepting every session that never claimed an umbrella, and the
    /// echo is a projection convenience only — the operator-signed create
    /// remains the authoritative membership claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    /// The local `HEAD` commit the provider most recently observed, as a
    /// lowercase hex object id, or `null` when never observed (no
    /// repository, no commits yet, or the probe failed or has not completed).
    ///
    /// B1 additive field: see the struct-level doc for why it lands here.
    #[serde(default)]
    pub observed_commit: Option<String>,
    /// Whether the worktree had uncommitted changes at the same observation
    /// as `observedCommit`, or `null` when not observed. Honesty rule: dirty
    /// is recorded as dirty, never inferred away.
    #[serde(default)]
    pub dirty: Option<bool>,
    /// Whether `observedCommit` was confirmed present in the relay's git
    /// storage. Tri-state, represented as a nullable bool: `null` means "not
    /// checked" — no repository coordinate, no observed commit, or the check
    /// itself could not complete (network/auth failure, malformed response).
    /// `Some(false)` is a positive claim that the check ran and the relay's
    /// advertised refs did not include the commit, never a stand-in for a
    /// failed check. `null` is never read as "confirmed not reachable".
    #[serde(default)]
    pub relay_reachable: Option<bool>,
    /// Unix seconds when the check that produced `relayReachable` ran.
    ///
    /// `null` exactly when `relayReachable` is `null` — there is no "checked
    /// but the outcome is unknown" state; a check either lands a confirmed
    /// `true`/`false` with its timestamp, or it did not happen and both
    /// fields stay `null` together.
    #[serde(default)]
    pub verified_at: Option<i64>,
    /// The umbrella turn budget in force for this execution, when one is (D9).
    ///
    /// The fourth independent additive key, and emitted under exactly two
    /// conditions: the execution claimed an umbrella (`sessionRef`), and the
    /// host set a finite budget. An unbudgeted or unclaimed execution omits
    /// the key entirely rather than publishing a null, so consumers written
    /// before this amendment keep accepting every shape they already knew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_budget: Option<TurnBudget>,
    /// Why this seat runs the model it runs — the routing record echoed from
    /// the create that seated it (Brian's ruling of 2026-08-30).
    ///
    /// The fifth independent additive key, emitted only when the create
    /// carried one, never as an explicit `null`. It exists so the question
    /// "why is this seat on that model" is answerable from the seat's own row
    /// rather than by hunting for the create that made it — and so an answer
    /// that cannot be explained from the wire is visibly absent instead of
    /// quietly assumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<crate::coding_session_routing::RoutingRecord>,
    /// Which `bee` binary this seat was started with, as the host observed it.
    ///
    /// The sixth independent additive key, emitted only when the host
    /// resolved one, never as an explicit `null`. On 2026-09-01 a seat ran
    /// the desktop app's bundled sidecar — three fixes behind — while the
    /// orchestrator ran the checkout's own debug build, and neither said so
    /// (`docs/SESSION_STATE.md` item 103, finding 1). This key is how a run
    /// says which binary answered.
    ///
    /// It is an **observed** fact: the host runs `$BEE --version` itself and
    /// parses the answer. Nothing is asked of the agent, so no seat can
    /// mis-state it by omission or by prose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bee_stamp: Option<BeeStamp>,
    /// Which persona pack this seat was staged from, as the host resolved it.
    ///
    /// The seventh independent additive key, emitted only when the host staged
    /// a pack named by a kind:30624 project pack source
    /// ([`crate::project_pack_source`]), never as an explicit `null`. A seat
    /// running the checkout's own `personas/roles/<role>/` — today's
    /// behaviour, and what happens when a project has published no pack
    /// source — omits the key, and the surfaces read *no pack staged* rather
    /// than inventing one.
    ///
    /// Read-optional, per finding 31: every decoder accepts its absence, and
    /// `decode_metadata_reads_a_44223_signed_before_pack_ref_existed` is the
    /// regression that keeps it so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_ref: Option<PackRef>,
    /// Who holds this session, and on which execution body — present exactly
    /// when a handover claim stands over this execution's umbrella.
    ///
    /// The eighth independent additive key. It exists so a returning provider
    /// **discloses the fence in the same breath as the status**: without it,
    /// `status: disconnected` over an execution somebody else has taken over
    /// reads as an ordinary outage, and the desktop would have to make a
    /// second query to find out otherwise. Omitted rather than written as an
    /// explicit `null` when no claim stands, so every pre-amendment consumer's
    /// exact-key check keeps accepting every session that was never handed
    /// over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handover: Option<SessionMetadataHandover>,
    /// How the staged pack named by `packRef` was composed: the app version
    /// whose template catalog resolved its includes, and the digest of the
    /// bytes that ran (spec § 4.6). Absent when the host staged nothing, or
    /// staged before this key existed. Never present without `packRef`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compose_ref: Option<ComposeRef>,
}

/// Whether the claim this metadata discloses still stands.
///
/// The word a reader needs and cannot infer. A provider publishing a fenced
/// execution has two very different things to say — "B holds this session on
/// that body" and "B held it, lost standing, and nobody holds it now" — and
/// both are published from the same three fields, because
/// [`crate::coding_session_authority_claim::ClaimState::last`] answers a
/// voided session with the claim as it stood. Without this word the second
/// case reads exactly like the first, and a surface would tell a person to go
/// ask a claimant who no longer holds anything (review finding N7).
///
/// There is deliberately no third token for "no claim": that state is the
/// **absence** of the `handover` key, and inventing a `"none"` variant would
/// give two encodings of one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionMetadataHandoverState {
    /// A claim is in force: `claimant` holds this umbrella on `bodyPubkey`,
    /// and only that pair may steer it.
    Active,
    /// The claim was voided when its claimant lost standing. The three fields
    /// describe the claim **as it stood**, for disclosure only — nobody holds
    /// the session, and the fence stays up for everybody until a fresh
    /// accepted `takeover`/`transfer`.
    Voided,
}

impl SessionMetadataHandoverState {
    /// The exact wire token for this state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Voided => "voided",
        }
    }
}

/// The claim this execution's umbrella is under, as its provider advertises
/// it.
///
/// Four fields, all required and all non-null — the same exactness
/// [`PackRef`] is held to, and for the same reason: a partial answer here
/// ("somebody took over" with no body, or a body with no claimant) would send
/// a person looking for something the record cannot name. The seq is
/// deliberately absent: this is a disclosure for a reader, and the chain is
/// the place to ask ordering questions.
///
/// `state` is what keeps a **voided** claim from reading as a live one; see
/// [`SessionMetadataHandoverState`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionMetadataHandover {
    /// Whether this claim still stands.
    pub state: SessionMetadataHandoverState,
    /// Pubkey (lowercase 64-hex) holding the session — or, when `state` is
    /// `voided`, the pubkey that held it.
    pub claimant: String,
    /// Provider authority pubkey of the execution body that claimant uses (or
    /// used).
    pub body_pubkey: String,
    /// Event id of the accepted `takeover`/`transfer` that set the claim.
    pub accepted_event_id: String,
}

impl SessionMetadataHandover {
    /// Validate the four fields as the wire requires them.
    ///
    /// # Errors
    /// A sentence naming the field that is wrong.
    pub fn validate(&self) -> Result<(), String> {
        for (field, value) in [
            ("claimant", &self.claimant),
            ("bodyPubkey", &self.body_pubkey),
            ("acceptedEventId", &self.accepted_event_id),
        ] {
            if value.len() != 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(format!(
                    "metadata handover.{field} must be a lowercase 64-hex id (got {value:?})"
                ));
            }
        }
        Ok(())
    }
}

/// The persona pack a seat was staged from, named exactly.
///
/// Four fields, all required and all non-null: this record exists so that
/// "which prompt did that agent actually run" has one answer a person can
/// check out. A partial answer — a repository with no commit, a commit with
/// no path — would be a worse lie than an absent key, which is why the shape
/// is exact and the whole object is omitted when the host staged nothing.
///
/// `sha` is always the **resolved** commit, even when the pack source pinned
/// a ref: the host records what it fetched, not what it asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackRef {
    /// The packs repository coordinate `30617:<owner-hex>:<id>`, or
    /// [`crate::project_pack_source::PACK_REF_SHIPPED_REPO`] (`app:shipped`)
    /// when the host staged the packs its own build bundles.
    pub repo: String,
    /// The 40-hex commit the host actually staged from — or, for
    /// `app:shipped`, the **app version** that bundled the packs.
    pub sha: String,
    /// The role slug whose pack was staged — the **seat's** role, never the
    /// actor's home role.
    pub role: String,
    /// The directory inside that commit the pack was read from, e.g.
    /// `personas/roles/builder`.
    pub path: String,
}

/// The composition of the staged pack `packRef` names (spec § 4.6).
///
/// `packRef` names the source bytes; template resolution is a function of
/// those bytes **and** the app version whose catalog resolved
/// `![[beekeeper/<template>@<range>]]` includes. This record carries that
/// version and the digest of the composed result, so "which prompt actually
/// ran" has one answer even after the app updates its templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposeRef {
    /// The app version whose template catalog composed the pack.
    pub app_version: String,
    /// `sha256:<64 lowercase hex>` over the staged persona and skill files —
    /// the `digest` in the staged pack's `compose.json`.
    pub digest: String,
}

/// Maximum UTF-8 byte length of `composeRef.appVersion`.
pub const MAX_COMPOSE_REF_APP_VERSION_BYTES: usize = 64;

impl ComposeRef {
    /// Check the wire shape: a bounded, non-blank app version and a
    /// `sha256:` digest of exactly sixty-four lowercase hex characters.
    pub fn validate(&self) -> Result<(), String> {
        let app_version = self.app_version.trim();
        if app_version.is_empty() || app_version.len() > MAX_COMPOSE_REF_APP_VERSION_BYTES {
            return Err(format!(
                "metadata composeRef.appVersion must be 1 to {MAX_COMPOSE_REF_APP_VERSION_BYTES} bytes"
            ));
        }
        let Some(hex) = self.digest.strip_prefix("sha256:") else {
            return Err("metadata composeRef.digest must start with sha256:".to_string());
        };
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(
                "metadata composeRef.digest must be sha256: followed by 64 lowercase hex characters"
                    .to_string(),
            );
        }
        Ok(())
    }
}

/// Where a seat's staged packs came from, as a reader must name it.
///
/// Two arms because the wire has two `repo` forms, and a surface that showed a
/// build's bundled packs as though they were a repository would be telling a
/// person to go look for something that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackRefSource {
    /// A git repository named by the project's kind:30624 pack source.
    Repository,
    /// The packs the running app's own build bundles.
    ShippedDefaults,
}

impl PackRefSource {
    /// The word a surface prints for this source.
    pub fn word(self) -> &'static str {
        match self {
            Self::Repository => crate::project_pack_source::PACK_SOURCE_WORD_REPOSITORY,
            Self::ShippedDefaults => crate::project_pack_source::PACK_SOURCE_WORD_SHIPPED,
        }
    }
}

impl PackRef {
    /// Which rung of the staging ladder this reference names.
    pub fn source(&self) -> PackRefSource {
        if self.repo == crate::project_pack_source::PACK_REF_SHIPPED_REPO {
            PackRefSource::ShippedDefaults
        } else {
            PackRefSource::Repository
        }
    }

    /// Validate the four fields as the wire requires them.
    ///
    /// # Errors
    /// A sentence naming the field that is wrong.
    pub fn validate(&self) -> Result<(), String> {
        match self.source() {
            PackRefSource::ShippedDefaults => {
                // `sha` is an app version here, not a commit. It is bounded and
                // printable and nothing more: inventing a hex shape for a
                // version string would make a reader believe it could be
                // checked out.
                if self.sha.trim().is_empty()
                    || self.sha.len() > MAX_METADATA_REFERENCE_BYTES
                    || self.sha.chars().any(char::is_control)
                {
                    return Err(format!(
                        "metadata packRef.sha must be the app version that bundled the \
                         shipped packs, nonempty and bounded (got {:?})",
                        self.sha
                    ));
                }
            }
            PackRefSource::Repository => {
                if crate::project_pack_source::normalize_repository_coordinate(&self.repo)
                    .as_deref()
                    != Some(self.repo.as_str())
                {
                    return Err(format!(
                        "metadata packRef.repo must be a canonical repository coordinate \
                         30617:<64-hex>:<id>, or {:?} for the app's shipped packs (got {:?})",
                        crate::project_pack_source::PACK_REF_SHIPPED_REPO,
                        self.repo
                    ));
                }
                if self.sha.len() != 40
                    || !self
                        .sha
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(format!(
                        "metadata packRef.sha must be 40 lowercase hex characters (got {:?})",
                        self.sha
                    ));
                }
            }
        }
        crate::coding_session_lifecycle_command::validate_role_slug(&self.role)
            .map_err(|error| error.replace("action.role", "metadata packRef.role"))?;
        if self.path.trim().is_empty() || self.path.len() > MAX_METADATA_REFERENCE_BYTES {
            return Err("metadata packRef.path must be nonempty and bounded".to_string());
        }
        if !self.path.ends_with(&format!("/{}", self.role)) {
            return Err(format!(
                "metadata packRef.path must end in the role it staged (path {:?}, role {:?})",
                self.path, self.role
            ));
        }
        Ok(())
    }
}

/// How the host came to run this particular `bee`.
///
/// Two words, because the resolution has exactly two outcomes and a third
/// would be a guess. `Bundled` is the sidecar shipped beside the running host
/// executable — what this build produced. `Path` is the first `bee` on the
/// inherited `PATH`, which is whatever the machine happened to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BeeStampSource {
    /// The sidecar beside the running host executable.
    Bundled,
    /// The first `bee` found on the inherited `PATH`.
    Path,
}

impl BeeStampSource {
    /// The wire word, so a renderer and a producer cannot drift.
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::Path => "path",
        }
    }
}

/// What `bee --version` said about the binary a seat was started with.
///
/// Exact fields, all five always present: an optional value is JSON `null`,
/// never absent, so a reader can tell "the host looked and could not parse an
/// answer" from "this key predates the amendment" (§0.8 — unknown is not
/// empty). `version`, `sha` and `dirty` are all `null` together when
/// `--version` was unparseable or exited non-zero; the surfaces then read
/// **unknown** rather than blank.
///
/// `sha` never carries the `-dirty` suffix `bee --version` prints; the suffix
/// is the separate `dirty` field, so a consumer never has to string-strip a
/// commit name to compare it with one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeeStamp {
    /// Absolute path to the binary the host chose.
    pub path: String,
    /// How the host came to choose it.
    pub source: BeeStampSource,
    /// The crate version `bee --version` printed, or `null` when unparsed.
    pub version: Option<String>,
    /// The short commit the binary was built from, lowercase hex, or `null`
    /// when the build could not name one (`unknown`) or the output did not
    /// parse.
    pub sha: Option<String>,
    /// Whether that build carried uncommitted changes to tracked files, or
    /// `null` when there is no commit for the flag to qualify.
    pub dirty: Option<bool>,
}

impl SessionMetadata {
    /// This generation's provider instance **alias**, typed so it can never be
    /// compared with a [`crate::coding_session_identity::ProviderInstanceId`].
    ///
    /// `Ok(None)` means the provider published no alias for this generation.
    /// `Err` means the value on the wire is not an alias — blank, oversized,
    /// or carrying control characters.
    ///
    /// This is the read that ledger item 102 got wrong in the other direction:
    /// Desktop's hire path compared a receipt's cryptographic
    /// `cs-target.instanceId` against the human-facing `providerInstanceRef`,
    /// and because both are `String` nothing objected. Reading through this
    /// accessor makes that comparison a compile error.
    pub fn provider_alias(
        &self,
    ) -> Result<Option<crate::coding_session_identity::ProviderInstanceAlias>, String> {
        self.provider
            .as_ref()
            .map(crate::coding_session_identity::ProviderInstanceAlias::as_str)
            .map(crate::coding_session_identity::ProviderInstanceAlias::from_wire)
            .transpose()
            .map_err(|error| error.replace("providerInstanceRef", "metadata provider"))
    }

    /// This generation's **runtime word** (`claude`, `codex`), typed so it can
    /// never be compared with the target's
    /// [`crate::coding_session_identity::DriverSlug`].
    ///
    /// The runtime word names the agent product; the driver slug names the ACP
    /// adapter that drives it (`claude` vs `claude-agent-acp`). A command is
    /// routed by driver and never by runtime, so the two are different
    /// questions with different answers.
    pub fn runtime_word(
        &self,
    ) -> Result<Option<crate::coding_session_identity::RuntimeWord>, String> {
        self.runtime
            .as_ref()
            .map(crate::coding_session_identity::RuntimeWord::as_str)
            .map(crate::coding_session_identity::RuntimeWord::from_wire)
            .transpose()
            .map_err(|error| error.replace("runtime", "metadata runtime"))
    }
}

/// How much of an umbrella's turn budget has been spent (D9).
///
/// Both numbers are facts the publishing provider witnessed itself: `used`
/// counts turns it actually *started* under this `sessionRef` (durably, so it
/// survives a restart), and `limit` is the ceiling its host configured. `used`
/// can exceed `limit` — the founder is never refused — and that is reported as
/// it happened rather than clamped, because a clamped count would hide who
/// spent what.
///
/// The object is exact: `deny_unknown_fields` here is what keeps this side
/// reading the same bytes the desktop's decoder reads, which rejects an extra
/// key inside `turnBudget` and drops the whole 44223 for it. A nested shape
/// that is strict on one side only is the divergence the ingress rules
/// forbid — the same signed event would show a session to one reader and
/// nothing to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnBudget {
    /// Turns started under this umbrella by the provider publishing this.
    pub used: u64,
    /// The ceiling non-founder turns are refused at.
    pub limit: u64,
}

/// Expected JSON key sets for [`SessionMetadata`], oldest first.
///
/// Eight independent additive amendments have landed on this struct at
/// different times — the `sessionRef` echo, B1's four coordinate-fact keys,
/// the agent seat's `role`, D9's `turnBudget`, the `routing` echo, the
/// `beeStamp` observation, L23's `packRef`, then the handover claim's
/// `handover` — and each one is present or absent on its own, so the base key
/// set has **two hundred and fifty-six** valid shapes, not four: base, and base
/// plus any combination of `sessionRef`, `role`, `turnBudget`, `routing`,
/// `beeStamp`, `packRef`, `handover`, and the four fact keys taken together. Mirrors the
/// exact-fields discipline in `coding_session_lifecycle_command.rs`
/// (`rejects_action_shapes_between_and_beyond_the_two_forms`): every shape in
/// between or beyond those two hundred and fifty-six — a partial subset of
/// the four fact keys, or any field this struct does not know — is rejected,
/// not tolerated.
const METADATA_BASE_FIELDS: &[&str] = &[
    "schema",
    "session",
    "projectRef",
    "repoRef",
    "title",
    "agentRef",
    "provider",
    "runtime",
    "model",
    "status",
    "branch",
    "capabilities",
];
const METADATA_SESSION_REF_FIELD: &str = "sessionRef";
/// The agent-seat amendment's one additive key. Independent of both earlier
/// amendments, so it doubles the accepted shape count from four to eight.
const METADATA_ROLE_FIELD: &str = "role";
const METADATA_FACT_FIELDS: &[&str] = &["observedCommit", "dirty", "relayReachable", "verifiedAt"];
/// The budget amendment's one additive key (D9). Independent of the other
/// three, so it doubles the accepted shape count from eight to sixteen.
const METADATA_TURN_BUDGET_FIELD: &str = "turnBudget";
/// The routing amendment's one additive key. The producer omits an absent
/// record, so an explicit null is not one of the accepted wire shapes.
const METADATA_ROUTING_FIELD: &str = "routing";
/// The bee-stamp amendment's one additive key. Independent of the other five,
/// so it doubles the accepted shape count from thirty-two to sixty-four.
/// Exactly like `routing`, the producer omits an absent stamp rather than
/// writing a null, so an explicit null is refused **naming the key** — a
/// consumer that sees `"beeStamp": null` is reading a producer that made up a
/// shape, not a host that had nothing to say.
const METADATA_BEE_STAMP_FIELD: &str = "beeStamp";
/// The pack-source amendment's one additive key. Independent of the other
/// six, so it doubles the accepted shape count from sixty-four to
/// one hundred and twenty-eight. Like `routing` and `beeStamp`, an explicit
/// null is refused **naming the key**: a host with nothing to stage omits it.
const METADATA_PACK_REF_FIELD: &str = "packRef";
/// The handover amendment's one additive key. Independent of the other seven,
/// so it doubles the accepted shape count from one hundred and twenty-eight to
/// two hundred and fifty-six. Like `routing`, `beeStamp` and `packRef`, an
/// explicit null is refused **naming the key**: a provider with no claim to
/// disclose omits it, and `"handover": null` is a producer inventing a shape
/// rather than a session nobody took over.
const METADATA_HANDOVER_FIELD: &str = "handover";
/// The compose-provenance amendment's one additive key (spec § 4.6).
/// Independent of the other eight on the wire, so it doubles the accepted
/// shape count from two hundred and fifty-six to five hundred and twelve —
/// but `validate_session_metadata` refuses it without `packRef`, since a
/// composition of nothing is not a fact. An explicit null is refused
/// **naming the key**, like every other amendment.
const METADATA_COMPOSE_REF_FIELD: &str = "composeRef";

/// Strictly decode and validate signed metadata content (kind 44223).
///
/// Accepts exactly the field-set shapes documented above
/// `METADATA_BASE_FIELDS`; anything else — an unknown key, or a B1 fact
/// key present without its three siblings — is a hard rejection. A second
/// pass through `serde_json` (after the shape check) picks up serde's own
/// duplicate-key detection, matching the two-pass pattern used for lifecycle
/// commands.
pub fn decode_coding_session_metadata(content: &str) -> Result<SessionMetadata, String> {
    if content.len() > MAX_METADATA_CONTENT_BYTES {
        return Err(format!(
            "coding-session metadata exceeds {MAX_METADATA_CONTENT_BYTES} bytes"
        ));
    }
    let value: serde_json::Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session metadata".to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "coding-session metadata must be an object".to_string())?;

    let has_session_ref = object.contains_key(METADATA_SESSION_REF_FIELD);
    let has_role = object.contains_key(METADATA_ROLE_FIELD);
    let has_turn_budget = object.contains_key(METADATA_TURN_BUDGET_FIELD);
    let has_routing = object.contains_key(METADATA_ROUTING_FIELD);
    if has_routing
        && object
            .get(METADATA_ROUTING_FIELD)
            .is_some_and(serde_json::Value::is_null)
    {
        return Err("coding-session metadata routing must not be null".to_string());
    }
    let has_bee_stamp = object.contains_key(METADATA_BEE_STAMP_FIELD);
    if has_bee_stamp
        && object
            .get(METADATA_BEE_STAMP_FIELD)
            .is_some_and(serde_json::Value::is_null)
    {
        return Err("coding-session metadata beeStamp must not be null".to_string());
    }
    let has_pack_ref = object.contains_key(METADATA_PACK_REF_FIELD);
    if has_pack_ref
        && object
            .get(METADATA_PACK_REF_FIELD)
            .is_some_and(serde_json::Value::is_null)
    {
        return Err("coding-session metadata packRef must not be null".to_string());
    }
    let has_handover = object.contains_key(METADATA_HANDOVER_FIELD);
    if has_handover
        && object
            .get(METADATA_HANDOVER_FIELD)
            .is_some_and(serde_json::Value::is_null)
    {
        return Err("coding-session metadata handover must not be null".to_string());
    }
    let has_compose_ref = object.contains_key(METADATA_COMPOSE_REF_FIELD);
    if has_compose_ref
        && object
            .get(METADATA_COMPOSE_REF_FIELD)
            .is_some_and(serde_json::Value::is_null)
    {
        return Err("coding-session metadata composeRef must not be null".to_string());
    }
    let has_all_facts = METADATA_FACT_FIELDS
        .iter()
        .all(|key| object.contains_key(*key));
    let has_any_fact = METADATA_FACT_FIELDS
        .iter()
        .any(|key| object.contains_key(*key));
    if has_any_fact && !has_all_facts {
        return Err("coding-session metadata has some but not all B1 fact fields".to_string());
    }

    let mut expected: Vec<&str> = METADATA_BASE_FIELDS.to_vec();
    if has_session_ref {
        expected.push(METADATA_SESSION_REF_FIELD);
    }
    if has_role {
        expected.push(METADATA_ROLE_FIELD);
    }
    if has_all_facts {
        expected.extend_from_slice(METADATA_FACT_FIELDS);
    }
    if has_turn_budget {
        expected.push(METADATA_TURN_BUDGET_FIELD);
    }
    if has_routing {
        expected.push(METADATA_ROUTING_FIELD);
    }
    if has_bee_stamp {
        expected.push(METADATA_BEE_STAMP_FIELD);
    }
    if has_pack_ref {
        expected.push(METADATA_PACK_REF_FIELD);
    }
    if has_handover {
        expected.push(METADATA_HANDOVER_FIELD);
    }
    if has_compose_ref {
        expected.push(METADATA_COMPOSE_REF_FIELD);
    }
    let recognized = object.keys().all(|key| expected.contains(&key.as_str()));
    let complete = expected.iter().all(|key| object.contains_key(*key));
    if !recognized || !complete || object.len() != expected.len() {
        return Err("coding-session metadata has missing or unsupported fields".to_string());
    }

    let metadata: SessionMetadata = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session metadata".to_string())?;
    validate_session_metadata(&metadata)?;
    Ok(metadata)
}

fn validate_session_metadata(metadata: &SessionMetadata) -> Result<(), String> {
    if metadata.schema != METADATA_SCHEMA {
        return Err("unsupported coding-session metadata schema".to_owned());
    }
    CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: "metadata-validation".to_owned(),
        target: metadata.session.clone(),
        action: CodingSessionAction::ThreadTurnInterrupt,
    }
    .validate()?;
    for (field, value) in [
        ("projectRef", metadata.project_ref.as_deref()),
        ("repoRef", metadata.repo_ref.as_deref()),
        ("title", metadata.title.as_deref()),
        ("agentRef", metadata.agent_ref.as_deref()),
        // `provider` and `runtime` are identity newtypes since B2; they are
        // read through `as_str` rather than `as_deref` on purpose, because
        // neither type derefs to `str` — that is the hole they exist to close.
        (
            "provider",
            metadata
                .provider
                .as_ref()
                .map(crate::coding_session_identity::ProviderInstanceAlias::as_str),
        ),
        (
            "runtime",
            metadata
                .runtime
                .as_ref()
                .map(crate::coding_session_identity::RuntimeWord::as_str),
        ),
        ("model", metadata.model.as_deref()),
        ("branch", metadata.branch.as_deref()),
        ("observedCommit", metadata.observed_commit.as_deref()),
    ] {
        if let Some(value) = value {
            if value.trim().is_empty() || value.len() > MAX_METADATA_REFERENCE_BYTES {
                return Err(format!("metadata {field} must be nonempty and bounded"));
            }
        }
    }
    if let Some(session_ref) = &metadata.session_ref {
        validate_session_ref(session_ref)?;
    }
    if let Some(agent_ref) = &metadata.agent_ref {
        crate::coding_session_lifecycle_command::validate_actor_pubkey(agent_ref)
            .map_err(|error| error.replace("action.actor", "metadata agentRef"))?;
    }
    match (&metadata.agent_ref, &metadata.role) {
        (Some(_), Some(role)) => {
            crate::coding_session_lifecycle_command::validate_role_slug(role)
                .map_err(|error| error.replace("action.role", "metadata role"))?;
        }
        (_, None) => {}
        (None, Some(_)) => {
            return Err(format!(
                "{ACTOR_ROLE_PAIR}: metadata role describes a seat, so it requires a non-null agentRef"
            ));
        }
    }
    if let Some(budget) = &metadata.turn_budget {
        // A budget is a fact about an umbrella, so it cannot describe an
        // execution that never claimed one — and a limit of zero would be a
        // ceiling nothing could ever pass, which is not what "unbudgeted"
        // means. Unbudgeted omits the key.
        if metadata.session_ref.is_none() {
            return Err(
                "metadata turnBudget describes an umbrella, so it requires a sessionRef".to_owned(),
            );
        }
        if budget.limit == 0 {
            return Err(
                "metadata turnBudget limit must be positive; omit the key when unbudgeted"
                    .to_owned(),
            );
        }
    }
    if metadata.relay_reachable.is_none() != metadata.verified_at.is_none() {
        return Err(
            "metadata relayReachable and verifiedAt must both be null or present".to_owned(),
        );
    }
    if metadata
        .verified_at
        .is_some_and(|value| value.unsigned_abs() > MAX_SAFE_GENERATION)
    {
        return Err("metadata verifiedAt must be a safe integer".to_owned());
    }
    if let Some(handover) = &metadata.handover {
        handover.validate()?;
    }
    if let Some(compose_ref) = &metadata.compose_ref {
        if metadata.pack_ref.is_none() {
            return Err(
                "metadata composeRef requires a packRef: it describes how that pack was composed"
                    .to_string(),
            );
        }
        compose_ref.validate()?;
    }
    if let Some(pack_ref) = &metadata.pack_ref {
        // The seat's role decides the pack, so a `packRef` naming a role the
        // metadata does not claim is a staging bug published as a fact. Refuse
        // it here rather than let a surface print a pack for the wrong role.
        pack_ref.validate()?;
        if metadata
            .role
            .as_deref()
            .is_some_and(|role| role != pack_ref.role)
        {
            return Err(format!(
                "metadata packRef.role must be this seat's role (role {:?}, packRef.role {:?})",
                metadata.role.as_deref().unwrap_or_default(),
                pack_ref.role
            ));
        }
    }
    Ok(())
}

/// One transcript item's signed envelope (kind 44225).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEnvelope {
    /// Always [`TRANSCRIPT_SCHEMA`].
    pub schema: String,
    /// The generation this item belongs to.
    pub session: CodingSessionTarget,
    /// Per-(session, generation) monotonic sequence, starting at 1.
    pub event_seq: u64,
    /// Milliseconds since the Unix epoch.
    pub timestamp: i64,
    /// Producer-minted turn id, or `null` for items outside any turn.
    pub turn_id: Option<String>,
    /// The projected item; a `kind`-discriminated open union.
    pub item: serde_json::Value,
}

impl TranscriptEnvelope {
    /// Wrap one projected item for a generation.
    pub fn new(
        target: &CodingSessionTarget,
        event_seq: u64,
        timestamp_ms: i64,
        turn_id: Option<&str>,
        item: serde_json::Value,
    ) -> Self {
        Self {
            schema: TRANSCRIPT_SCHEMA.to_owned(),
            session: target.clone(),
            event_seq,
            timestamp: timestamp_ms,
            turn_id: turn_id.map(str::to_owned),
            item,
        }
    }
}

/// How a turn ended, as the consumer's `result` item spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultSubtype {
    /// The turn finished normally.
    Success,
    /// The turn failed.
    Error,
    /// The turn was interrupted.
    Cancelled,
}

impl ResultSubtype {
    /// The exact string the consumer's projector reads.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Per-turn accounting, when the adapter reported any.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TurnCost {
    /// Estimated USD cost of the turn.
    pub cost_usd: Option<f64>,
    /// Input tokens consumed.
    pub input_tokens: Option<u64>,
    /// Output tokens produced.
    pub output_tokens: Option<u64>,
    /// Total tokens, when the adapter reports a genuine total.
    pub total_tokens: Option<u64>,
}

impl TurnCost {
    /// Whether anything at all is known.
    pub fn is_empty(&self) -> bool {
        self.cost_usd.is_none()
            && self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.total_tokens.is_none()
    }
}

/// Per-turn token accounting, in the shape a reader can compute context
/// occupancy from.
///
/// This rides the terminal `result` item as an optional `usage` object. Every
/// field is optional and every unknown one is *omitted*, never serialized as
/// `null` or `0` — "the driver did not report it" and "the driver measured
/// zero" are different facts and the archive keeps them apart.
///
/// # The three prompt-side fields are disjoint
///
/// [`Self::input_tokens`], [`Self::cache_read_tokens`] and
/// [`Self::cache_write_tokens`] partition the tokens the provider sent *to*
/// the model this turn, so [`Self::used_tokens`] is their sum. That is
/// deliberately not the same convention as the item's own top-level
/// `inputTokens` key, which is cache-*inclusive* — it shipped that way before
/// this block existed and is left alone so old readers keep reading it.
///
/// # What `usedTokens` is, and what it is not
///
/// It is the prompt-side token total for **one turn**. A turn that makes
/// several model calls sends a prompt on each one, so on those turns the total
/// is larger than the context the model actually held — it is consumption, not
/// occupancy. A driver that states occupancy directly does so in its own
/// `context_window_updated` item; read that with [`context_window_usage`] and
/// prefer it when it is present.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnUsageReport {
    /// Fresh (uncached) prompt tokens sent this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Tokens the model produced this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Prompt tokens served from the provider's cache this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    /// Prompt tokens written into the provider's cache this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    /// Tool calls the agent opened during this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<u64>,
    /// The model's context window in tokens, when the provider knows it.
    /// Omitted for an unrecognized model — a guessed window makes every
    /// percentage downstream a fiction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
}

impl TurnUsageReport {
    /// Whether the driver reported nothing at all.
    ///
    /// An empty block is omitted from the item rather than published as `{}`.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Prompt-side tokens for this turn: fresh input plus both cache subsets.
    ///
    /// `None` when the driver reported none of the three — there is no honest
    /// number to give, and `0` would claim a measurement nobody made.
    /// Saturating on the (impossible in practice) overflow of three `u64`
    /// token counts, because a clamped total is still closer to the truth than
    /// a wrapped one.
    pub fn used_tokens(&self) -> Option<u64> {
        let parts = [
            self.input_tokens,
            self.cache_read_tokens,
            self.cache_write_tokens,
        ];
        parts.iter().any(Option::is_some).then(|| {
            parts
                .into_iter()
                .flatten()
                .fold(0u64, |total, part| total.saturating_add(part))
        })
    }
}

/// A driver's own statement of how full the model's context window is.
///
/// Unlike [`TurnUsageReport::used_tokens`], this *is* occupancy: the driver
/// measured the prompt it is about to send, so it never exceeds the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextWindowUsage {
    /// Tokens currently occupying the window.
    pub used_tokens: u64,
    /// The window itself, when the driver named it. `None` rather than a
    /// guess: a percentage against an invented denominator is a lie.
    pub context_window: Option<u64>,
}

/// Read occupancy off a `context_window_updated` transcript item.
///
/// Returns `None` for any other item kind and for an item that names no
/// occupancy. Both spellings drivers use for each number are accepted —
/// `used`/`usedTokens` and `size`/`contextWindow` — because the ACP schema
/// pins neither and the provider forwards whatever the driver sent.
pub fn context_window_usage(item: &serde_json::Value) -> Option<ContextWindowUsage> {
    if item.get("kind").and_then(serde_json::Value::as_str) != Some("context_window_updated") {
        return None;
    }
    let usage = item.get("usage")?;
    let number = |keys: &[&str]| -> Option<u64> {
        keys.iter()
            .find_map(|key| usage.get(*key).and_then(serde_json::Value::as_u64))
    };
    Some(ContextWindowUsage {
        used_tokens: number(&["used", "usedTokens"])?,
        context_window: number(&["size", "contextWindow", "contextLimit"]),
    })
}

/// Build the terminal `result` item that closes a turn.
///
/// `costUsd` and the token counts are omitted rather than sent as `null`: the
/// consumer's projector renders `costUsd` only when it is a number, and an
/// explicit `null` would claim the provider measured zero. `usage` follows the
/// same rule and the whole object is omitted when it is empty, so a result
/// item from a driver that reports nothing is byte-identical to the shape that
/// shipped before [`TurnUsageReport`] existed.
pub fn result_item(
    subtype: ResultSubtype,
    duration_ms: u64,
    result: &str,
    cost: TurnCost,
    usage: TurnUsageReport,
) -> serde_json::Value {
    let mut item = serde_json::json!({
        "kind": "result",
        "subtype": subtype.as_str(),
        "isError": subtype == ResultSubtype::Error,
        "durationMs": duration_ms,
        "result": result,
    });
    // The literal above is an object, so this always matches; written as a
    // pattern rather than an `expect` so a future edit to the literal degrades
    // into a result item without accounting rather than a panic mid-turn.
    if let Some(object) = item.as_object_mut() {
        if let Some(cost_usd) = cost.cost_usd {
            object.insert("costUsd".into(), serde_json::json!(cost_usd));
        }
        for (key, value) in [
            ("inputTokens", cost.input_tokens),
            ("outputTokens", cost.output_tokens),
            ("totalTokens", cost.total_tokens),
        ] {
            if let Some(value) = value {
                object.insert(key.into(), serde_json::json!(value));
            }
        }
        // Serialization of a struct of `Option`s with `skip_serializing_if`
        // cannot fail; written as an `if let` rather than an `expect` so a
        // future edit degrades into a result item without the block instead of
        // panicking mid-turn.
        if !usage.is_empty() {
            if let Ok(value) = serde_json::to_value(usage) {
                object.insert("usage".into(), value);
            }
        }
    }
    item
}

/// Closed set of reasons a create or resume lost its verified prior context.
///
/// The set is closed on purpose. Projector failures interpolate event and
/// command ids, and a storage failure formats a host path; publishing either
/// into a signed durable event would leak host-private material. A reason is a
/// *class*, never the underlying message — the class goes on the wire, the
/// detail stays in the provider's log.
///
/// `no_prior_execution` and `no_umbrella_context` name the *same* missing
/// input — the umbrella `sessionRef`/`genesisRef` pair the projector is keyed
/// on — but they are two different facts about the execution reading them, and
/// only the create path can honestly claim the first. On a create, no umbrella
/// refs means this really is the session's first execution. On a resume, an
/// earlier generation of this execution demonstrably ran, so the same missing
/// pair means only that it never ran under an umbrella; publishing
/// `no_prior_execution` there would print "this is the session's first
/// execution" directly above that execution's own earlier turns.
pub const CONTEXT_UNAVAILABLE_REASONS: &[&str] = &[
    "no_prior_execution",
    "no_umbrella_context",
    "context_fact_conflict",
    "relay_unavailable",
    "relay_query_failed",
    "unverifiable_source_fact",
    "source_exceeds_projection_bound",
    "context_sidecar_unavailable",
    "context_sidecar_path_invalid",
    "brief_encode_failed",
    "package_write_failed",
];

/// Build a bounded `status` item — the projector renders it as a lifecycle row.
pub fn status_item(status: &str) -> serde_json::Value {
    status_item_with_reason(status, None)
}

/// Build a status item that may name why continuity was lost.
///
/// `reason` is emitted only when it is a member of
/// [`CONTEXT_UNAVAILABLE_REASONS`]; anything else is dropped rather than
/// published, because this value enters a signed durable event.
///
/// The key is **additive and optional**: when there is no recognized reason it
/// is omitted entirely rather than sent as `null`, so the output is
/// byte-identical to the shape shipped before this field existed. A `null` on a
/// durable record would claim the provider observed "no reason", which is a
/// different fact from "this item carries no reason".
pub fn status_item_with_reason(status: &str, reason: Option<&str>) -> serde_json::Value {
    let mut item = serde_json::json!({ "kind": "status", "status": status });
    // The literal above is an object, so this always matches; written as a
    // pattern rather than an `expect` so a future edit degrades into a status
    // row without a reason rather than a panic mid-turn.
    if let (Some(object), Some(reason)) = (item.as_object_mut(), reason) {
        if CONTEXT_UNAVAILABLE_REASONS.contains(&reason) {
            object.insert("reason".into(), serde_json::json!(reason));
        }
    }
    item
}

/// Build the `user_prompt` item that opens a turn.
///
/// `operator_pubkey` is the signer of the 44220 the provider *verified* before
/// running the turn — a locally witnessed fact, not a relayed claim. Sessions
/// are multi-operator (founder plus granted operators), so without it the
/// transcript cannot say who drove a turn and every reader has to guess it was
/// itself.
///
/// `command_id` is the `commandId` of the 44220 `thread.turn.start` that
/// started this turn — or, for the first turn embedded in a 44221 create, that
/// create's own `commandId`, so *every* operator-originated prompt is joinable.
/// Without it a consumer has to settle a pending turn by matching prompt text,
/// which cannot tell two identical prompts apart.
///
/// `sender_role` is the crew role slug the signer held on its own seat in this
/// umbrella at the moment the turn was delivered — `null` for the founder and
/// for any operator who holds no seat. It is what lets a reader say *lead
/// asked for this* without joining the prompt back to a roster it may no
/// longer be able to fetch. It describes the **sender**, never the execution
/// running the turn: that seat's role is in its own 44223 metadata.
///
/// All three keys are **additive and optional**: `operatorPubkey` is emitted
/// only for a well-formed 64-character lowercase-hex pubkey, `commandId` only
/// for a nonblank, control-free identifier within
/// [`MAX_IDENTIFIER_BYTES`](crate::coding_session_command::MAX_IDENTIFIER_BYTES),
/// and `senderRole` only for a valid role slug. Anything else is omitted
/// entirely rather than sent as `null`, which would claim the provider
/// observed an absence. Items published before any of these fields existed
/// stay valid everywhere.
pub fn user_prompt_item(
    content: &str,
    steered: bool,
    operator_pubkey: Option<&str>,
    command_id: Option<&str>,
    sender_role: Option<&str>,
    attachment_count: usize,
) -> serde_json::Value {
    let mut item =
        serde_json::json!({ "kind": "user_prompt", "content": content, "steered": steered });
    // The literal above is an object, so this always matches; written as a
    // pattern rather than an `expect` so a future edit degrades into an
    // unattributed prompt rather than a panic at the head of a turn.
    let Some(object) = item.as_object_mut() else {
        return item;
    };
    if let Some(pubkey) = operator_pubkey {
        if is_operator_pubkey(pubkey) {
            object.insert("operatorPubkey".into(), serde_json::json!(pubkey));
        }
    }
    if let Some(command_id) = command_id {
        if is_wire_identifier(command_id) {
            object.insert("commandId".into(), serde_json::json!(command_id));
        }
    }
    if let Some(role) = sender_role {
        if crate::coding_session_lifecycle_command::validate_role_slug(role).is_ok() {
            object.insert("senderRole".into(), serde_json::json!(role));
        }
    }
    // Additive and optional like the rest: a turn with no images is
    // byte-identical to the shape published before attachments existed. This
    // counts what was actually *delivered* to the agent, so a turn whose
    // images were dropped for an execution that cannot take them does not
    // leave a transcript claiming the agent saw them.
    if attachment_count > 0 {
        object.insert(
            "attachmentCount".into(),
            serde_json::json!(attachment_count),
        );
    }
    item
}

/// Whether `value` could have travelled as a `commandId` on the wire: nonblank,
/// bounded, and free of control characters — the same rule
/// `coding_session_command`'s validator applies to the command itself.
fn is_wire_identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= crate::coding_session_command::MAX_IDENTIFIER_BYTES
        && !value.chars().any(char::is_control)
}

/// Whether `pubkey` is the canonical 64-character lowercase-hex form.
fn is_operator_pubkey(pubkey: &str) -> bool {
    pubkey.len() == 64
        && pubkey
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Normalize an operator-facing string to the consumer's nullable contract.
///
/// The consumer's `boundedNullable` accepts `null` or a *non-blank* string and
/// rejects everything else, so a whitespace-only title has to become `null`
/// here rather than travelling as `""` and failing the whole event.
pub fn nullable(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Cap on the total serialized size of one tool item's `edit` payload.
///
/// 16 KiB, half the 32 KiB CST envelope cap, so an edit payload can never be
/// the reason an item has to be elided whole: the surrounding item —
/// `toolName`, `toolId`, `input`, the result's own `content` — still has room.
/// The cap is documented rather than implicit because a reader who sees
/// `truncated: true` is owed the number it was measured against.
pub const MAX_TOOL_EDIT_PAYLOAD_BYTES: usize = 16 * 1024;

/// One file change an ACP adapter reported for an `edit`-kind tool call.
///
/// This is ACP's own `ToolCallContent::Diff` block, not something the provider
/// derived: `path` is the adapter's path, `old_text` and `new_text` are the
/// adapter's texts. A missing `old_text` means the adapter reported a new file,
/// which is different from an empty one, so it stays `None` rather than `""`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolEditChange {
    /// The file the adapter named for this change, already redacted/relativized
    /// by the caller.
    pub path: Option<String>,
    /// The text before the edit, when the adapter sent one.
    pub old_text: Option<String>,
    /// The text after the edit, when the adapter sent one.
    pub new_text: Option<String>,
}

/// Build the `edit` payload published beside an `edit`-kind tool item.
///
/// The payload answers the one question the Observed-changes surface asks —
/// *which files did this call touch, and what did it do to them* — from the two
/// ACP fields that carry it: `ToolCall.locations` and the `ToolCallContent`
/// diff blocks. Before this existed the provider published only `rawInput`,
/// which claude-agent-acp leaves empty on an edit while the tool's arguments
/// are still streaming, so every edit reached the wire as `"input": {}` and the
/// consumer rendered presence as absence.
///
/// Shape:
///
/// ```json
/// {
///   "paths": ["desktop/src/App.tsx"],
///   "changes": [
///     { "path": "desktop/src/App.tsx", "oldText": "…", "newText": "…", "truncated": true }
///   ],
///   "truncated": true
/// }
/// ```
///
/// Every field is optional and additive; a consumer that does not know this
/// payload ignores it. `paths` is deduplicated and order-preserving.
///
/// **Truncation is never silent.** The payload is bounded by
/// [`MAX_TOOL_EDIT_PAYLOAD_BYTES`]: change texts are shortened first — each
/// shortened change carries its own `"truncated": true` — and whole changes are
/// dropped only after that, which sets the payload-level `"truncated": true`.
/// `paths` is never dropped, because naming the file is the payload's point;
/// a path list that alone exceeds the cap is itself truncated and flagged.
///
/// Returns `None` when the adapter reported neither a path nor any change text,
/// so an empty object never travels claiming an observation nobody made.
pub fn tool_edit_payload(
    paths: &[String],
    changes: &[ToolEditChange],
) -> Option<serde_json::Value> {
    /// Room for `{"paths":[],"changes":[],"truncated":true}` and its commas.
    const WRAPPER_BYTES: usize = 64;
    /// Shortest text worth keeping; below this a change says nothing anyway.
    const MIN_KEPT_TEXT_BYTES: usize = 64;
    /// Room for one change's keys, its path, and the escaping of its texts.
    const PER_CHANGE_OVERHEAD_BYTES: usize = 512;

    let mut unique_paths: Vec<String> = Vec::new();
    let mut budget = MAX_TOOL_EDIT_PAYLOAD_BYTES.saturating_sub(WRAPPER_BYTES);
    let mut truncated = false;
    for path in paths {
        let path = path.trim();
        if path.is_empty() || unique_paths.iter().any(|kept| kept == path) {
            continue;
        }
        // A path list that alone would blow the cap is truncated too — flagged,
        // never silently short.
        let cost = path.len() + 3;
        if cost > budget {
            truncated = true;
            break;
        }
        budget -= cost;
        unique_paths.push(path.to_owned());
    }

    // Every change gets an equal share of what is left, so one enormous file
    // cannot starve the rest. Sizes are computed on the *truncated* texts, so
    // this never serializes a multi-megabyte diff to find out it is too big.
    let share = if changes.is_empty() {
        0
    } else {
        // Two texts per change, plus room for the keys, the path and the JSON
        // escaping of what is kept.
        ((budget / changes.len()).saturating_sub(PER_CHANGE_OVERHEAD_BYTES) / 2)
            .max(MIN_KEPT_TEXT_BYTES)
    };
    let mut encoded: Vec<serde_json::Value> = Vec::new();
    for change in changes {
        let mut object = serde_json::Map::new();
        if let Some(path) = change.path.as_deref().map(str::trim) {
            if !path.is_empty() {
                object.insert("path".into(), serde_json::json!(path));
            }
        }
        let mut cut = false;
        for (key, text) in [("oldText", &change.old_text), ("newText", &change.new_text)] {
            let Some(text) = text.as_deref() else {
                continue;
            };
            let kept = clamp_to_char_boundary(text, share);
            cut |= kept.len() < text.len();
            object.insert(key.into(), serde_json::json!(kept));
        }
        if object.is_empty() {
            continue;
        }
        if cut {
            object.insert("truncated".into(), serde_json::json!(true));
        }
        let encoded_change = serde_json::Value::Object(object);
        let cost = encoded_change.to_string().len() + 1;
        if cost > budget {
            // No room for this change or any after it: say so rather than
            // letting a reader mistake a dropped change for one that never
            // happened.
            truncated = true;
            break;
        }
        budget -= cost;
        encoded.push(encoded_change);
    }

    let mut payload = serde_json::Map::new();
    if !unique_paths.is_empty() {
        payload.insert("paths".into(), serde_json::json!(unique_paths));
    }
    if !encoded.is_empty() {
        payload.insert("changes".into(), serde_json::Value::Array(encoded));
    }
    if payload.is_empty() {
        return None;
    }
    if truncated {
        payload.insert("truncated".into(), serde_json::json!(true));
    }
    Some(serde_json::Value::Object(payload))
}

/// Keep at most `max_bytes` of `text`, never splitting a multibyte character.
fn clamp_to_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Clamp an error message to something the 16 KiB receipt cap can always hold.
fn bounded_message(message: &str) -> String {
    const MAX_MESSAGE_BYTES: usize = 1024;
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return "unspecified provider error".to_owned();
    }
    if trimmed.len() <= MAX_MESSAGE_BYTES {
        return trimmed.to_owned();
    }
    let mut end = MAX_MESSAGE_BYTES;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &trimmed[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "11111111-2222-3333-4444-555555555555".into(),
            generation: 1,
        }
    }

    /// The consumer checks key *sets*, not order (`hasExactKeys`), and
    /// `serde_json::Value` sorts its map anyway — so compare sorted sets and
    /// pin the on-the-wire ordering separately where it matters.
    fn keys(value: &serde_json::Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        keys
    }

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        names.sort();
        names
    }

    /// The consumer's `hasExactKeys` check means these key lists are the
    /// contract, not an implementation detail.
    #[test]
    fn receipt_always_carries_the_five_keys_the_consumer_requires() {
        for receipt in [
            LifecycleReceipt::created("create-1", &target()),
            LifecycleReceipt::created_with_failed_initial_turn("create-1", &target(), "boom"),
            LifecycleReceipt::failed("create-1", PROJECT_CWD_UNRESOLVED, "no cwd"),
            LifecycleReceipt::resumed("resume-1", &target()),
            LifecycleReceipt::resumed_without_context("resume-2", &target(), "cursor rejected"),
            LifecycleReceipt::stopped("stop-1", &target()),
        ] {
            let value = serde_json::to_value(&receipt).expect("serialize");
            assert_eq!(
                keys(&value),
                sorted(&["schema", "commandId", "status", "session", "error"])
            );
            assert_eq!(value["schema"], LIFECYCLE_RECEIPT_SCHEMA);
        }
    }

    #[test]
    fn receipt_status_strings_match_the_donor_parser() {
        let created = serde_json::to_value(LifecycleReceipt::created("c", &target())).unwrap();
        assert_eq!(created["status"], "created");
        assert!(created["error"].is_null());

        let partial = serde_json::to_value(LifecycleReceipt::created_with_failed_initial_turn(
            "c",
            &target(),
            "prompt failed",
        ))
        .unwrap();
        assert_eq!(partial["status"], "created_with_failed_initial_turn");
        assert_eq!(partial["error"]["code"], INITIAL_TURN_FAILED);
        assert_eq!(partial["error"]["message"], "prompt failed");

        let failed =
            serde_json::to_value(LifecycleReceipt::failed("c", SESSION_LIMIT, "at cap")).unwrap();
        assert_eq!(failed["status"], "failed");
        assert!(failed["session"].is_null());
        assert_eq!(keys(&failed["error"]), sorted(&["code", "message"]));

        let resumed = serde_json::to_value(LifecycleReceipt::resumed("c", &target())).unwrap();
        assert_eq!(resumed["status"], "resumed");
        assert!(resumed["error"].is_null());

        let discontinuity = serde_json::to_value(LifecycleReceipt::resumed_without_context(
            "c",
            &target(),
            "cursor rejected",
        ))
        .unwrap();
        assert_eq!(discontinuity["status"], "resumed_without_context");
        assert_eq!(discontinuity["error"]["code"], CONTEXT_NOT_RECOVERED);

        let stopped = serde_json::to_value(LifecycleReceipt::stopped("c", &target())).unwrap();
        assert_eq!(stopped["status"], "stopped");
        assert!(stopped["error"].is_null());
    }

    /// A blank message would make the whole receipt malformed at the consumer's
    /// `boundedNonempty` check, losing the failure entirely.
    #[test]
    fn receipt_messages_are_never_blank_and_never_unbounded() {
        let blank = LifecycleReceipt::failed("c", SESSION_LIMIT, "   ");
        assert_eq!(
            blank.error.expect("error").message,
            "unspecified provider error"
        );

        let long = LifecycleReceipt::failed("c", SESSION_LIMIT, &"é".repeat(4096));
        let message = long.error.expect("error").message;
        assert!(
            message.len() <= 1024 + '…'.len_utf8(),
            "got {} bytes",
            message.len()
        );
        assert!(message.ends_with('…'));
    }

    #[test]
    fn strict_receipt_decoder_accepts_successes_and_rejects_ambiguous_shapes() {
        for receipt in [
            LifecycleReceipt::created("create-1", &target()),
            LifecycleReceipt::created_with_failed_initial_turn("create-1", &target(), "boom"),
            LifecycleReceipt::resumed("resume-1", &target()),
            LifecycleReceipt::resumed_without_context("resume-2", &target(), "lost"),
            LifecycleReceipt::stopped("stop-1", &target()),
            LifecycleReceipt::failed("bad-1", SESSION_LIMIT, "full"),
        ] {
            let json = serde_json::to_string(&receipt).unwrap();
            assert_eq!(
                decode_coding_session_lifecycle_receipt(&json).unwrap(),
                receipt
            );
        }

        let mut wrong = serde_json::to_value(LifecycleReceipt::created("c", &target())).unwrap();
        wrong["session"] = serde_json::Value::Null;
        assert!(decode_coding_session_lifecycle_receipt(&wrong.to_string()).is_err());
        wrong = serde_json::to_value(LifecycleReceipt::failed("c", SESSION_LIMIT, "x")).unwrap();
        wrong["session"] = serde_json::to_value(target()).unwrap();
        assert!(decode_coding_session_lifecycle_receipt(&wrong.to_string()).is_err());
    }

    #[test]
    fn strict_receipt_decoder_rejects_unknown_missing_and_duplicate_fields() {
        let valid = serde_json::to_value(LifecycleReceipt::created("c", &target())).unwrap();
        let mut unknown = valid.clone();
        unknown["trusted"] = serde_json::json!(true);
        assert!(decode_coding_session_lifecycle_receipt(&unknown.to_string()).is_err());
        let mut missing = valid;
        missing.as_object_mut().unwrap().remove("error");
        assert!(decode_coding_session_lifecycle_receipt(&missing.to_string()).is_err());
        let duplicate = format!(
            r#"{{"schema":"{s}","schema":"{s}","commandId":"c","status":"created","session":{{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"11111111-2222-3333-4444-555555555555","generation":1}},"error":null}}"#,
            s = LIFECYCLE_RECEIPT_SCHEMA
        );
        assert!(decode_coding_session_lifecycle_receipt(&duplicate).is_err());
    }

    #[test]
    fn metadata_carries_every_required_key_and_no_others() {
        let metadata = SessionMetadata {
            schema: METADATA_SCHEMA.to_owned(),
            session: target(),
            project_ref: None,
            repo_ref: None,
            title: nullable(Some("  ")),
            agent_ref: None,
            role: None,
            provider: Some("claude-primary".try_into().expect("alias")),
            runtime: Some("claude".try_into().expect("runtime")),
            model: Some("claude-sonnet-4-6".into()),
            status: SessionStatus::Idle,
            branch: None,
            capabilities: Capabilities::v1_claude(),
            session_ref: None,
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
            turn_budget: None,
            routing: None,
            bee_stamp: None,
            pack_ref: None,
            handover: None,
            compose_ref: None,
        };
        let value = serde_json::to_value(&metadata).expect("serialize");
        assert_eq!(
            keys(&value),
            sorted(&[
                "schema",
                "session",
                "projectRef",
                "repoRef",
                "title",
                "agentRef",
                "provider",
                "runtime",
                "model",
                "status",
                "branch",
                "capabilities",
                "observedCommit",
                "dirty",
                "relayReachable",
                "verifiedAt",
            ])
        );
        // Signed bytes come from `to_string`, which keeps declaration order.
        assert!(serde_json::to_string(&metadata)
            .expect("serialize")
            .starts_with(r#"{"schema":"buzz-coding-session-metadata/v1","session":"#));
        // A whitespace title must arrive as null, not "".
        assert!(value["title"].is_null());
        assert_eq!(value["status"], "idle");
        assert_eq!(
            keys(&value["capabilities"]),
            sorted(&[
                "threadTurnStart",
                "threadTurnInterrupt",
                "threadSteer",
                "context",
                "diff",
                "plan",
                "promptImage",
            ])
        );
    }

    /// `sessionRef` is optional-key, not explicit-null: present exactly when
    /// an umbrella was claimed, absent otherwise. An explicit `null` would
    /// fail *every* old client's exact-key metadata check; absence fails it
    /// only for sessions that actually claimed an umbrella.
    #[test]
    fn metadata_emits_session_ref_only_when_an_umbrella_was_claimed() {
        let mut metadata = SessionMetadata {
            schema: METADATA_SCHEMA.to_owned(),
            session: target(),
            project_ref: None,
            repo_ref: None,
            title: None,
            agent_ref: None,
            role: None,
            provider: Some("codex-primary".try_into().expect("alias")),
            runtime: Some("codex".try_into().expect("runtime")),
            model: None,
            status: SessionStatus::Running,
            branch: None,
            capabilities: Capabilities::v1_baseline(),
            session_ref: None,
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
            turn_budget: None,
            routing: None,
            bee_stamp: None,
            pack_ref: None,
            handover: None,
            compose_ref: None,
        };
        let unclaimed = serde_json::to_value(&metadata).expect("serialize");
        assert!(
            !unclaimed
                .as_object()
                .expect("object")
                .contains_key("sessionRef"),
            "no umbrella claimed: the key must be absent, not null"
        );

        metadata.session_ref = Some("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into());
        let claimed = serde_json::to_value(&metadata).expect("serialize");
        assert_eq!(
            claimed["sessionRef"],
            "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"
        );

        // Both shapes round-trip: readers of old 12-key events and new 13-key
        // events decode through the same struct.
        let reloaded: SessionMetadata =
            serde_json::from_value(unclaimed).expect("deserialize 12-key form");
        assert!(reloaded.session_ref.is_none());
        let reloaded: SessionMetadata =
            serde_json::from_value(claimed).expect("deserialize 13-key form");
        assert_eq!(
            reloaded.session_ref.as_deref(),
            Some("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10")
        );
    }

    /// A pre-B1 event — 12 keys, or 13 with the `sessionRef` echo — carries
    /// none of the four coordinate-fact keys. `decode_coding_session_metadata`
    /// must still accept it, with every new fact field landing as `None`.
    #[test]
    fn decode_metadata_accepts_historical_forms_without_the_facts() {
        let base = serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": null,
            "provider": "claude-primary",
            "runtime": "claude",
            "model": null,
            "status": "idle",
            "branch": null,
            "capabilities": Capabilities::v1_claude(),
        });
        let decoded = decode_coding_session_metadata(&base.to_string()).expect("12-key form");
        assert!(decoded.observed_commit.is_none());
        assert!(decoded.dirty.is_none());
        assert!(decoded.relay_reachable.is_none());
        assert!(decoded.verified_at.is_none());
        assert!(decoded.session_ref.is_none());

        let mut with_session_ref = base.clone();
        with_session_ref["sessionRef"] = serde_json::json!("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");
        let decoded =
            decode_coding_session_metadata(&with_session_ref.to_string()).expect("13-key form");
        assert_eq!(
            decoded.session_ref.as_deref(),
            Some("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10")
        );
        assert!(decoded.observed_commit.is_none());
    }

    #[test]
    fn decode_metadata_accepts_the_writer_routing_shape_and_rejects_null() {
        let mut metadata = serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": "ab".repeat(32),
            "provider": "claude-primary",
            "runtime": "claude",
            "model": "sonnet",
            "status": "idle",
            "branch": null,
            "capabilities": Capabilities::v1_claude(),
            "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
            "role": "builder",
            "routing": {
                "class": "builder",
                "tier": "standard",
                "risk": {"impact": 3, "uncertainty": 3, "irreversibility": 2, "score": 18},
                "profile": null,
                "chosen": {"provider": "claude-primary", "model": "sonnet", "effort": "medium"},
                "runnerUp": null,
                "reason": "cleared the builder gate",
                "reviewRequired": false,
                "reviewReasons": [],
                "challengerSample": false,
                "override": null,
                "registryVersion": 1,
                "catalogRevision": 7
            }
        });
        let decoded =
            decode_coding_session_metadata(&metadata.to_string()).expect("writer routing form");
        assert!(decoded.routing.is_some());

        metadata["routing"] = serde_json::Value::Null;
        assert!(decode_coding_session_metadata(&metadata.to_string()).is_err());
    }

    /// The `beeStamp` amendment, held to exactly the rules `routing` set.
    ///
    /// Four assertions, one per rule: the key decodes and round-trips, an
    /// explicit `null` is refused **naming the key**, a 44223 without the key
    /// is byte-identical to what a pre-amendment producer wrote, and every
    /// optional value survives as `null` rather than being dropped.
    #[test]
    fn decode_metadata_accepts_the_bee_stamp_shape_and_refuses_a_null() {
        let without = serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": "ab".repeat(32),
            "provider": "claude-primary",
            "runtime": "claude",
            "model": "sonnet",
            "status": "idle",
            "branch": null,
            "capabilities": Capabilities::v1_claude(),
            "role": "builder"
        });
        let decoded_without =
            decode_coding_session_metadata(&without.to_string()).expect("pre-amendment form");
        assert!(decoded_without.bee_stamp.is_none());
        let encoded_without = serde_json::to_value(&decoded_without).expect("re-encode");
        assert!(
            !encoded_without
                .as_object()
                .expect("an object")
                .contains_key("beeStamp"),
            "an absent beeStamp must not become a null on the way back out"
        );

        let mut with = without.clone();
        with["beeStamp"] = serde_json::json!({
            "path": "/Applications/Beekeeper.app/Contents/MacOS/bee",
            "source": "bundled",
            "version": "0.1.0",
            "sha": "23728227b",
            "dirty": false
        });
        let decoded = decode_coding_session_metadata(&with.to_string()).expect("stamped form");
        let stamp = decoded.bee_stamp.clone().expect("a stamp");
        assert_eq!(stamp.source, BeeStampSource::Bundled);
        assert_eq!(stamp.sha.as_deref(), Some("23728227b"));
        assert_eq!(stamp.dirty, Some(false));
        let mut encoded = serde_json::to_value(&decoded).expect("re-encode");
        assert_eq!(
            encoded.get("beeStamp"),
            with.get("beeStamp"),
            "the stamp must round-trip byte-identically"
        );
        // The byte-identity assertion the spec asks for, stated as an
        // assertion rather than a sentence: strip the one new key and the
        // remaining 44223 is byte-for-byte what a pre-amendment producer
        // wrote. Nothing else about the shape moved.
        encoded
            .as_object_mut()
            .expect("an object")
            .remove("beeStamp");
        assert_eq!(
            encoded, encoded_without,
            "publishing a beeStamp must change nothing else on the event"
        );

        let mut unknown = without.clone();
        unknown["beeStamp"] = serde_json::json!({
            "path": "/usr/local/bin/bee",
            "source": "path",
            "version": null,
            "sha": null,
            "dirty": null
        });
        let decoded_unknown =
            decode_coding_session_metadata(&unknown.to_string()).expect("unknown-build form");
        let stamp = decoded_unknown.bee_stamp.clone().expect("a stamp");
        assert_eq!(stamp.source, BeeStampSource::Path);
        assert!(stamp.sha.is_none() && stamp.dirty.is_none() && stamp.version.is_none());
        assert_eq!(
            serde_json::to_value(&decoded_unknown)
                .expect("re-encode")
                .get("beeStamp"),
            unknown.get("beeStamp"),
            "an unparsed stamp keeps its three explicit nulls"
        );

        let mut null_stamp = without;
        null_stamp["beeStamp"] = serde_json::Value::Null;
        let error = decode_coding_session_metadata(&null_stamp.to_string())
            .expect_err("an explicit null is refused");
        assert!(
            error.contains("beeStamp"),
            "the refusal must name the key it refused: {error}"
        );
    }

    /// The `packRef` amendment, held to exactly the rules `beeStamp` set.
    ///
    /// Round-trip, byte-identity with the pre-amendment form, an explicit null
    /// refused by name, and every part of the object validated.
    #[test]
    fn decode_metadata_accepts_the_pack_ref_shape_and_refuses_a_null() {
        let without = pack_ref_base();
        let decoded_without =
            decode_coding_session_metadata(&without.to_string()).expect("pre-amendment form");
        assert!(decoded_without.pack_ref.is_none());
        let encoded_without = serde_json::to_value(&decoded_without).expect("re-encode");
        assert!(
            !encoded_without
                .as_object()
                .expect("an object")
                .contains_key("packRef"),
            "an absent packRef must not become a null on the way back out"
        );

        let mut with = without.clone();
        with["packRef"] = valid_pack_ref();
        let decoded = decode_coding_session_metadata(&with.to_string()).expect("staged form");
        let pack = decoded.pack_ref.clone().expect("a packRef");
        assert_eq!(pack.role, "builder");
        assert_eq!(pack.path, "personas/roles/builder");
        assert_eq!(pack.sha, "a".repeat(40));
        let mut encoded = serde_json::to_value(&decoded).expect("re-encode");
        assert_eq!(
            encoded.get("packRef"),
            with.get("packRef"),
            "the packRef must round-trip byte-identically"
        );
        encoded
            .as_object_mut()
            .expect("an object")
            .remove("packRef");
        assert_eq!(
            encoded, encoded_without,
            "publishing a packRef must change nothing else on the event"
        );

        let mut null_pack = without;
        null_pack["packRef"] = serde_json::Value::Null;
        let error = decode_coding_session_metadata(&null_pack.to_string())
            .expect_err("an explicit null is refused");
        assert!(
            error.contains("packRef"),
            "the refusal must name the key it refused: {error}"
        );
    }

    /// The compose-provenance amendment (spec § 4.6): absent on every 44223
    /// signed before it existed and on every packless seat, round-trips
    /// exactly beside a `packRef`, refuses an explicit null by name, and is
    /// refused without the `packRef` it describes.
    #[test]
    fn decode_metadata_accepts_the_compose_ref_shape_only_beside_a_pack_ref() {
        let without = pack_ref_base();
        let decoded_without =
            decode_coding_session_metadata(&without.to_string()).expect("pre-amendment form");
        assert!(decoded_without.compose_ref.is_none());
        let encoded_without = serde_json::to_value(&decoded_without).expect("re-encode");
        assert!(!encoded_without
            .as_object()
            .expect("an object")
            .contains_key("composeRef"));

        let compose_ref = serde_json::json!({
            "appVersion": "0.4.2",
            "digest": format!("sha256:{}", "b".repeat(64))
        });
        let mut with = without.clone();
        with["packRef"] = valid_pack_ref();
        with["composeRef"] = compose_ref.clone();
        let decoded = decode_coding_session_metadata(&with.to_string()).expect("composed form");
        let composed = decoded.compose_ref.clone().expect("a composeRef");
        assert_eq!(composed.app_version, "0.4.2");
        let encoded = serde_json::to_value(&decoded).expect("re-encode");
        assert_eq!(encoded.get("composeRef"), Some(&compose_ref));

        let mut orphan = without.clone();
        orphan["composeRef"] = compose_ref.clone();
        let error = decode_coding_session_metadata(&orphan.to_string())
            .expect_err("a composeRef without a packRef is refused");
        assert!(error.contains("requires a packRef"), "{error}");

        let mut null_compose = without.clone();
        null_compose["packRef"] = valid_pack_ref();
        null_compose["composeRef"] = serde_json::Value::Null;
        let error = decode_coding_session_metadata(&null_compose.to_string())
            .expect_err("an explicit null is refused");
        assert!(error.contains("composeRef"), "{error}");

        let mut bad_digest = with.clone();
        bad_digest["composeRef"]["digest"] = serde_json::Value::String("abc".into());
        assert!(decode_coding_session_metadata(&bad_digest.to_string())
            .expect_err("a malformed digest is refused")
            .contains("digest"));
        let mut extra = with.clone();
        extra["composeRef"]["command"] = serde_json::Value::String("rm".into());
        assert!(decode_coding_session_metadata(&extra.to_string()).is_err());
    }

    /// The handover amendment: the key is absent on every unclaimed session,
    /// round-trips exactly when present, and refuses an explicit null by name.
    #[test]
    fn decode_metadata_accepts_the_handover_shape_and_refuses_a_null() {
        let without = pack_ref_base();
        let decoded_without =
            decode_coding_session_metadata(&without.to_string()).expect("pre-amendment form");
        assert!(decoded_without.handover.is_none());
        let encoded_without = serde_json::to_value(&decoded_without).expect("re-encode");
        assert!(
            !encoded_without
                .as_object()
                .expect("an object")
                .contains_key("handover"),
            "an absent handover must not become a null on the way back out"
        );

        let mut with = without.clone();
        with["handover"] = valid_handover("active");
        let decoded = decode_coding_session_metadata(&with.to_string()).expect("claimed form");
        let handover = decoded.handover.clone().expect("a handover");
        assert_eq!(handover.state, SessionMetadataHandoverState::Active);
        assert_eq!(handover.claimant, "bb".repeat(32));
        assert_eq!(handover.body_pubkey, "dd".repeat(32));
        assert_eq!(handover.accepted_event_id, "ee".repeat(32));
        let mut encoded = serde_json::to_value(&decoded).expect("re-encode");
        assert_eq!(
            encoded.get("handover"),
            with.get("handover"),
            "the handover must round-trip byte-identically"
        );
        encoded
            .as_object_mut()
            .expect("an object")
            .remove("handover");
        assert_eq!(
            encoded, encoded_without,
            "publishing a handover must change nothing else on the event"
        );

        let mut null_handover = without;
        null_handover["handover"] = serde_json::Value::Null;
        let error = decode_coding_session_metadata(&null_handover.to_string())
            .expect_err("an explicit null is refused");
        assert!(
            error.contains("handover"),
            "the refusal must name the key it refused: {error}"
        );
    }

    /// Review finding N7: a voided claim must not read as a live one.
    ///
    /// Both states carry the same three references — the provider publishes
    /// the claim as it stood — so the only thing telling them apart is the
    /// word, and it is required rather than defaulted: a producer that omits
    /// it is refused instead of being read as `active`.
    #[test]
    fn a_voided_claim_says_so_and_the_state_word_is_required() {
        let mut voided = pack_ref_base();
        voided["handover"] = valid_handover("voided");
        let decoded = decode_coding_session_metadata(&voided.to_string()).expect("voided form");
        let handover = decoded.handover.expect("a handover");
        assert_eq!(handover.state, SessionMetadataHandoverState::Voided);
        assert_eq!(handover.state.as_str(), "voided");
        // Same three references as an active claim: only the word differs, so
        // a reader that ignored it would show a fenced session as steerable.
        assert_eq!(handover.claimant, "bb".repeat(32));

        let mut missing_state = pack_ref_base();
        missing_state["handover"] = valid_handover("active");
        missing_state["handover"]
            .as_object_mut()
            .expect("an object")
            .remove("state");
        assert!(
            decode_coding_session_metadata(&missing_state.to_string()).is_err(),
            "an absent state must be refused, never defaulted to active"
        );

        for bad in ["none", "Active", "no-claim", ""] {
            let mut wrong = pack_ref_base();
            wrong["handover"] = valid_handover(bad);
            assert!(
                decode_coding_session_metadata(&wrong.to_string()).is_err(),
                "state {bad:?} must be refused"
            );
        }
    }

    /// Every reference in a handover is a lowercase 64-hex id, and a
    /// malformed one is refused naming the field.
    #[test]
    fn a_handover_with_a_malformed_reference_is_refused_by_name() {
        for (field, value) in [
            ("claimant", "not-hex"),
            ("bodyPubkey", "DD"),
            ("acceptedEventId", ""),
        ] {
            let mut wrong = pack_ref_base();
            wrong["handover"] = valid_handover("active");
            wrong["handover"][field] = serde_json::json!(value);
            let error = decode_coding_session_metadata(&wrong.to_string())
                .expect_err("a malformed reference is refused");
            assert!(error.contains(field), "{error}");
        }

        let mut smuggled = pack_ref_base();
        smuggled["handover"] = valid_handover("active");
        smuggled["handover"]["seq"] = serde_json::json!(2);
        assert!(
            decode_coding_session_metadata(&smuggled.to_string()).is_err(),
            "the handover object rejects unknown keys rather than ignoring them"
        );
    }

    /// Finding 31, stated as a test: a 44223 signed before `packRef` existed
    /// must keep decoding, and every other pre-amendment shape with it.
    #[test]
    fn decode_metadata_reads_a_44223_signed_before_pack_ref_existed() {
        // Byte-for-byte what a producer wrote on 2026-09-02, before this key.
        let signed_before = r#"{"schema":"buzz-coding-session-metadata/v1","session":{"driver":"claude-agent-acp","instanceId":"instance-1","sessionId":"11111111-2222-3333-4444-555555555555","generation":1},"projectRef":null,"repoRef":null,"title":null,"agentRef":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","provider":"claude-primary","runtime":"claude","model":"sonnet","status":"idle","branch":null,"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":true,"promptImage":false},"role":"builder"}"#;
        let decoded =
            decode_coding_session_metadata(signed_before).expect("a pre-packRef 44223 still reads");
        assert!(
            decoded.pack_ref.is_none(),
            "absence must read as absence, never as a default pack"
        );
    }

    /// Every part of a `packRef` is checked, and a partial object is refused
    /// outright: a repository with no commit names no bytes.
    #[test]
    fn a_pack_ref_is_exact_and_every_field_is_validated() {
        let base = pack_ref_base();
        let mut cases: Vec<(serde_json::Value, &str)> = Vec::new();

        let mut short_sha = valid_pack_ref();
        short_sha["sha"] = serde_json::json!("a".repeat(39));
        cases.push((short_sha, "packRef.sha"));

        let mut upper_sha = valid_pack_ref();
        upper_sha["sha"] = serde_json::json!("A".repeat(40));
        cases.push((upper_sha, "packRef.sha"));

        let mut bad_repo = valid_pack_ref();
        bad_repo["repo"] = serde_json::json!(format!("30621:{}:agiterra", "ab".repeat(32)));
        cases.push((bad_repo, "packRef.repo"));

        let mut bad_role = valid_pack_ref();
        bad_role["role"] = serde_json::json!("Builder");
        cases.push((bad_role, "packRef.role"));

        let mut wrong_path = valid_pack_ref();
        wrong_path["path"] = serde_json::json!("personas/roles/lead");
        cases.push((wrong_path, "must end in the role"));

        for (pack, needle) in cases {
            let mut event = base.clone();
            event["packRef"] = pack;
            let error = decode_coding_session_metadata(&event.to_string())
                .expect_err("an invalid packRef is refused");
            assert!(
                error.contains(needle),
                "expected {needle:?} in the refusal, got {error:?}"
            );
        }

        // Missing and unknown keys inside the object are both hard rejections.
        let mut missing = base.clone();
        let mut partial = valid_pack_ref();
        partial.as_object_mut().expect("an object").remove("path");
        missing["packRef"] = partial;
        assert!(decode_coding_session_metadata(&missing.to_string()).is_err());

        let mut extra = base.clone();
        let mut widened = valid_pack_ref();
        widened["branch"] = serde_json::json!("main");
        extra["packRef"] = widened;
        assert!(decode_coding_session_metadata(&extra.to_string()).is_err());

        // The staging rule: the seat's role picks the pack, so a packRef for a
        // different role than the seat holds is a published staging bug.
        let mut mismatched = base;
        let mut other_role = valid_pack_ref();
        other_role["role"] = serde_json::json!("lead");
        other_role["path"] = serde_json::json!("personas/roles/lead");
        mismatched["packRef"] = other_role;
        let error = decode_coding_session_metadata(&mismatched.to_string())
            .expect_err("a packRef for another role is refused");
        assert!(error.contains("this seat's role"), "{error}");
    }

    /// The addendum's third rung: a seat staged from the app's own bundled
    /// packs names them, and is never dressed up as a repository.
    #[test]
    fn a_shipped_defaults_pack_ref_carries_the_app_version_and_says_so() {
        let mut event = pack_ref_base();
        event["packRef"] = serde_json::json!({
            "repo": buzz_shipped_repo(),
            "sha": "0.5.16",
            "role": "builder",
            "path": "personas/roles/builder"
        });
        let decoded = decode_coding_session_metadata(&event.to_string()).expect("shipped form");
        let pack = decoded.pack_ref.clone().expect("a packRef");
        assert_eq!(pack.source(), PackRefSource::ShippedDefaults);
        assert_eq!(pack.source().word(), "shipped defaults");
        assert_eq!(
            pack.sha, "0.5.16",
            "the app version is the sha here, not a commit"
        );

        // A blank version is refused: "shipped, from a build that will not say
        // which" is exactly the unknown-as-empty this key exists to prevent.
        let mut blank = pack_ref_base();
        blank["packRef"] = serde_json::json!({
            "repo": buzz_shipped_repo(),
            "sha": "   ",
            "role": "builder",
            "path": "personas/roles/builder"
        });
        let error = decode_coding_session_metadata(&blank.to_string()).expect_err("refused");
        assert!(error.contains("app version"), "{error}");

        // And a repository-shaped reference still needs a real commit, so the
        // shipped arm cannot be used to smuggle a short sha past the check.
        let mut repo_form = pack_ref_base();
        repo_form["packRef"] = serde_json::json!({
            "repo": format!("30617:{}:agiterra-packs", "6c".repeat(32)),
            "sha": "0.5.16",
            "role": "builder",
            "path": "personas/roles/builder"
        });
        let error = decode_coding_session_metadata(&repo_form.to_string()).expect_err("refused");
        assert!(error.contains("40 lowercase hex"), "{error}");
    }

    /// The one place the shipped sentinel is spelled, read back.
    fn buzz_shipped_repo() -> &'static str {
        crate::project_pack_source::PACK_REF_SHIPPED_REPO
    }

    /// The shared conformance vectors' 44223 half, decoded by this crate.
    #[test]
    fn shared_pack_source_conformance_metadata_vectors_match_this_decoder() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../conformance/project-pack-source/fixtures/pack-source-vectors.json"
        ))
        .expect("fixture parses");
        let vectors = fixture["metadataVectors"]
            .as_array()
            .expect("metadataVectors is an array");
        assert!(vectors.len() >= 5);
        for vector in vectors {
            let name = vector["name"].as_str().expect("a name");
            let expected_valid = vector["valid"].as_bool().expect("a verdict");
            let decoded = decode_coding_session_metadata(&vector["content"].to_string());
            assert_eq!(
                decoded.is_ok(),
                expected_valid,
                "vector {name:?} disagreed with this decoder: {decoded:?}"
            );
        }
    }

    /// The pre-amendment 44223 every `packRef` case starts from.
    fn pack_ref_base() -> serde_json::Value {
        serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": "cd".repeat(32),
            "provider": "claude-primary",
            "runtime": "claude",
            "model": "sonnet",
            "status": "idle",
            "branch": null,
            "capabilities": Capabilities::v1_claude(),
            "role": "builder"
        })
    }

    /// A `handover` in the given state, every reference well-formed.
    fn valid_handover(state: &str) -> serde_json::Value {
        serde_json::json!({
            "state": state,
            "claimant": "bb".repeat(32),
            "bodyPubkey": "dd".repeat(32),
            "acceptedEventId": "ee".repeat(32)
        })
    }

    /// A `packRef` every field of which is what the wire requires.
    fn valid_pack_ref() -> serde_json::Value {
        serde_json::json!({
            "repo": format!("30617:{}:agiterra-packs", "6c".repeat(32)),
            "sha": "a".repeat(40),
            "role": "builder",
            "path": "personas/roles/builder"
        })
    }

    /// The two current B1 forms — facts alone, and facts plus `sessionRef` —
    /// decode, and every fact is independently readable including the
    /// `Some(false)` "confirmed not reachable" case.
    #[test]
    fn decode_metadata_accepts_the_current_fact_bearing_forms() {
        let base = serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": "30617:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:repo",
            "title": null,
            "agentRef": null,
            "provider": "claude-primary",
            "runtime": "claude",
            "model": null,
            "status": "idle",
            "branch": "main",
            "capabilities": Capabilities::v1_claude(),
            "observedCommit": "a".repeat(40),
            "dirty": false,
            "relayReachable": false,
            "verifiedAt": 1_700_000_000i64,
        });
        let decoded = decode_coding_session_metadata(&base.to_string()).expect("16-key form");
        assert_eq!(
            decoded.observed_commit.as_deref(),
            Some("a".repeat(40).as_str())
        );
        assert_eq!(decoded.dirty, Some(false));
        assert_eq!(decoded.relay_reachable, Some(false));
        assert_eq!(decoded.verified_at, Some(1_700_000_000));

        let mut with_session_ref = base.clone();
        with_session_ref["sessionRef"] = serde_json::json!("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");
        let decoded =
            decode_coding_session_metadata(&with_session_ref.to_string()).expect("17-key form");
        assert_eq!(decoded.relay_reachable, Some(false));
        assert_eq!(
            decoded.session_ref.as_deref(),
            Some("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10")
        );
    }

    /// D9's `turnBudget` is the fourth independent additive key: present only
    /// for a budgeted umbrella, absent (never null, never a zero limit) for
    /// everything else, and refused when it describes an execution that
    /// claimed no umbrella to bound.
    #[test]
    fn decode_metadata_accepts_a_turn_budget_only_beside_an_umbrella() {
        let base = serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": null,
            "provider": "claude-primary",
            "runtime": "claude",
            "model": null,
            "status": "running",
            "branch": null,
            "capabilities": Capabilities::v1_claude(),
            "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
            "turnBudget": { "used": 12, "limit": 200 },
        });
        let decoded = decode_coding_session_metadata(&base.to_string()).expect("14-key form");
        assert_eq!(
            decoded.turn_budget,
            Some(TurnBudget {
                used: 12,
                limit: 200
            })
        );

        // Used past limit is reported as it happened: the founder is never
        // refused, so a crew can legitimately end up over its allowance.
        let mut overspent = base.clone();
        overspent["turnBudget"] = serde_json::json!({ "used": 201, "limit": 200 });
        assert_eq!(
            decode_coding_session_metadata(&overspent.to_string())
                .expect("overspent decodes")
                .turn_budget,
            Some(TurnBudget {
                used: 201,
                limit: 200
            })
        );

        // A budget without an umbrella describes nothing, and a limit of zero
        // would read as "no turns allowed" rather than "unbudgeted".
        let mut orphaned = base.clone();
        orphaned
            .as_object_mut()
            .expect("object")
            .remove("sessionRef");
        assert!(decode_coding_session_metadata(&orphaned.to_string()).is_err());
        let mut zero = base.clone();
        zero["turnBudget"] = serde_json::json!({ "used": 0, "limit": 0 });
        assert!(decode_coding_session_metadata(&zero.to_string()).is_err());

        // An unbudgeted session omits the key entirely, and still decodes.
        let mut unbudgeted = base.clone();
        unbudgeted
            .as_object_mut()
            .expect("object")
            .remove("turnBudget");
        assert!(decode_coding_session_metadata(&unbudgeted.to_string())
            .expect("13-key form")
            .turn_budget
            .is_none());

        // The nested object is exact too. The desktop's decoder rejects an
        // extra key inside `turnBudget` and drops the whole 44223 for it, so a
        // shape this side tolerated would be a divergence: two readers of the
        // same signed bytes, one showing the session and one not.
        let mut smuggled = base.clone();
        smuggled["turnBudget"] = serde_json::json!({ "used": 1, "limit": 200, "remaining": 199 });
        assert!(decode_coding_session_metadata(&smuggled.to_string()).is_err());
        let mut halved = base.clone();
        halved["turnBudget"] = serde_json::json!({ "used": 1 });
        assert!(decode_coding_session_metadata(&halved.to_string()).is_err());

        // Serialization is the mirror image: absent, not null.
        let mut metadata = decode_coding_session_metadata(&base.to_string()).expect("14-key form");
        metadata.turn_budget = None;
        assert!(!serde_json::to_value(&metadata)
            .expect("serialize")
            .as_object()
            .expect("object")
            .contains_key("turnBudget"));
    }

    /// Canonical smuggle-rejection test, mirroring
    /// `rejects_action_shapes_between_and_beyond_the_two_forms` in
    /// `coding_session_lifecycle_command.rs`: a shape with some but not all
    /// of the four fact keys, and a shape with an unrecognized key, are both
    /// rejected — never silently accepted with the missing keys defaulted.
    #[test]
    fn decode_metadata_rejects_shapes_between_and_beyond_the_known_forms() {
        let base = serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": null,
            "provider": "claude-primary",
            "runtime": "claude",
            "model": null,
            "status": "idle",
            "branch": null,
            "capabilities": Capabilities::v1_claude(),
        });

        // Between: only two of the four fact keys present.
        let mut partial = base.clone();
        partial["observedCommit"] = serde_json::json!(null);
        partial["dirty"] = serde_json::json!(null);
        let error =
            decode_coding_session_metadata(&partial.to_string()).expect_err("partial facts");
        assert!(error.contains("some but not all"));

        // Beyond: a smuggled key no known form recognizes.
        let mut smuggled = base.clone();
        smuggled["observedCommit"] = serde_json::json!(null);
        smuggled["dirty"] = serde_json::json!(null);
        smuggled["relayReachable"] = serde_json::json!(null);
        smuggled["verifiedAt"] = serde_json::json!(null);
        smuggled["hostPath"] = serde_json::json!("/etc/passwd");
        let error =
            decode_coding_session_metadata(&smuggled.to_string()).expect_err("smuggled field");
        assert!(error.contains("missing or unsupported"));

        // Beyond: sessionRef present together with only a partial fact set.
        let mut mixed = base.clone();
        mixed["sessionRef"] = serde_json::json!("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");
        mixed["observedCommit"] = serde_json::json!(null);
        let error = decode_coding_session_metadata(&mixed.to_string()).expect_err("mixed shape");
        assert!(error.contains("some but not all"));
    }

    /// D1: an agent seat's metadata names the seat (`agentRef`) and the role
    /// it holds, and the `role` key is additive — present only for a seat.
    #[test]
    fn decode_metadata_accepts_the_agent_seat_forms() {
        let base = serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": "cd".repeat(32),
            "role": "lead",
            "provider": "claude-primary",
            "runtime": "claude",
            "model": null,
            "status": "idle",
            "branch": null,
            "capabilities": Capabilities::v1_claude(),
        });
        let decoded = decode_coding_session_metadata(&base.to_string()).expect("seated 13-key");
        assert_eq!(decoded.agent_ref.as_deref(), Some("cd".repeat(32).as_str()));
        assert_eq!(decoded.role.as_deref(), Some("lead"));

        // The seat key composes with both earlier amendments.
        let mut full = base.clone();
        full["sessionRef"] = serde_json::json!("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");
        full["observedCommit"] = serde_json::json!(null);
        full["dirty"] = serde_json::json!(null);
        full["relayReachable"] = serde_json::json!(null);
        full["verifiedAt"] = serde_json::json!(null);
        let decoded = decode_coding_session_metadata(&full.to_string()).expect("seated 18-key");
        assert_eq!(decoded.role.as_deref(), Some("lead"));
        assert_eq!(
            decoded.session_ref.as_deref(),
            Some("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10")
        );
    }

    /// A `role` describes a seat, so it can never appear without one, and it
    /// obeys the same slug rule the create's `role` does.
    #[test]
    fn metadata_role_requires_an_agent_ref_and_a_valid_slug() {
        let base = serde_json::json!({
            "schema": METADATA_SCHEMA,
            "session": target(),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": null,
            "provider": "claude-primary",
            "runtime": "claude",
            "model": null,
            "status": "idle",
            "branch": null,
            "capabilities": Capabilities::v1_claude(),
        });

        let mut orphan = base.clone();
        orphan["role"] = serde_json::json!("lead");
        let error = decode_coding_session_metadata(&orphan.to_string())
            .expect_err("a role with no seat must be refused");
        assert!(error.contains(ACTOR_ROLE_PAIR), "got {error:?}");

        for role in ["", "Lead", "lead builder"] {
            let mut bad = base.clone();
            bad["agentRef"] = serde_json::json!("cd".repeat(32));
            bad["role"] = serde_json::json!(role);
            assert!(
                decode_coding_session_metadata(&bad.to_string()).is_err(),
                "role {role:?} was accepted"
            );
        }
    }

    /// The byte-for-byte guarantee: metadata for an execution with no seat is
    /// exactly what it was before this amendment.
    #[test]
    fn unseated_metadata_never_writes_the_role_key() {
        let metadata = SessionMetadata {
            schema: METADATA_SCHEMA.to_owned(),
            session: target(),
            project_ref: None,
            repo_ref: None,
            title: None,
            agent_ref: None,
            role: None,
            provider: Some("claude-primary".try_into().expect("alias")),
            runtime: Some("claude".try_into().expect("runtime")),
            model: None,
            status: SessionStatus::Idle,
            branch: None,
            capabilities: Capabilities::v1_claude(),
            session_ref: None,
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
            turn_budget: None,
            routing: None,
            bee_stamp: None,
            pack_ref: None,
            handover: None,
            compose_ref: None,
        };
        let content = serde_json::to_string(&metadata).expect("serialize");
        assert!(
            !content.contains("\"role\""),
            "an unseated execution wrote a role key: {content}"
        );
        assert_eq!(
            serde_json::to_value(&metadata)
                .expect("value")
                .as_object()
                .expect("object")
                .len(),
            16,
            "unseated metadata is still the exact base + B1-facts shape"
        );
        assert!(decode_coding_session_metadata(&content).is_ok());
    }

    #[test]
    fn session_status_strings_match_the_fork_allowlist() {
        let allowed = [
            (SessionStatus::Starting, "starting"),
            (SessionStatus::Idle, "idle"),
            (SessionStatus::Running, "running"),
            (SessionStatus::WaitingForInput, "waiting_for_input"),
            (SessionStatus::Completed, "completed"),
            (SessionStatus::Stopped, "stopped"),
            (SessionStatus::Failed, "failed"),
            (SessionStatus::Interrupted, "interrupted"),
            (SessionStatus::Disconnected, "disconnected"),
            (SessionStatus::Unknown, "unknown"),
        ];
        for (status, wire) in allowed {
            assert_eq!(serde_json::to_value(status).unwrap(), wire);
        }
    }

    #[test]
    fn transcript_envelope_carries_the_six_keys_the_consumer_requires() {
        let envelope = TranscriptEnvelope::new(
            &target(),
            7,
            1_700_000_000_000,
            Some("turn-1"),
            serde_json::json!({ "kind": "assistant_text", "text": "hello" }),
        );
        let value = serde_json::to_value(&envelope).expect("serialize");
        assert_eq!(
            keys(&value),
            sorted(&[
                "schema",
                "session",
                "eventSeq",
                "timestamp",
                "turnId",
                "item"
            ])
        );
        assert_eq!(value["schema"], TRANSCRIPT_SCHEMA);
        assert_eq!(value["eventSeq"], 7);

        let untimed = TranscriptEnvelope::new(&target(), 1, 0, None, serde_json::json!({}));
        assert!(serde_json::to_value(&untimed).unwrap()["turnId"].is_null());
    }

    /// The attribution key is additive: an unattributed prompt keeps exactly
    /// the three keys every existing consumer already reads.
    #[test]
    fn user_prompt_carries_the_operator_only_when_one_was_witnessed() {
        let unattributed = user_prompt_item("go", false, None, None, None, 0);
        assert_eq!(keys(&unattributed), sorted(&["kind", "content", "steered"]));

        let operator = "a".repeat(64);
        let attributed = user_prompt_item("go", true, Some(&operator), None, None, 0);
        assert_eq!(
            keys(&attributed),
            sorted(&["kind", "content", "steered", "operatorPubkey"])
        );
        assert_eq!(attributed["operatorPubkey"], operator);
        assert_eq!(attributed["steered"], true);
    }

    /// A malformed pubkey is dropped rather than published: the field exists
    /// to be an authority-grade fact, so a half-true one is worse than none.
    #[test]
    fn user_prompt_drops_an_operator_that_is_not_canonical_hex() {
        for bad in [
            "",
            "not-hex",
            &"a".repeat(63),
            &"a".repeat(65),
            &"A".repeat(64),
            &format!("{}{}", "z", "a".repeat(63)),
        ] {
            let item = user_prompt_item("go", false, Some(bad), None, None, 0);
            assert!(
                item.get("operatorPubkey").is_none(),
                "{bad:?} must not be published as an operator"
            );
        }
    }

    #[test]
    fn target_serializes_with_the_four_keys_the_consumer_decodes() {
        let value = serde_json::to_value(target()).expect("serialize");
        assert_eq!(
            keys(&value),
            sorted(&["driver", "instanceId", "sessionId", "generation"])
        );
    }

    /// Every existing reader of a status item was written against this exact
    /// shape, so the no-reason path has to stay byte-for-byte what it was.
    #[test]
    fn a_status_item_without_a_reason_is_byte_identical_to_the_shipped_shape() {
        let shipped = serde_json::json!({ "kind": "status", "status": "session_fresh" });
        assert_eq!(status_item("session_fresh"), shipped);
        assert_eq!(status_item_with_reason("session_fresh", None), shipped);
        assert_eq!(
            serde_json::to_string(&status_item("session_fresh")).expect("serialize"),
            serde_json::to_string(&shipped).expect("serialize")
        );
        assert_eq!(
            keys(&status_item("session_fresh")),
            sorted(&["kind", "status"])
        );
    }

    #[test]
    fn status_item_with_reason_emits_only_enumerated_reasons() {
        assert_eq!(CONTEXT_UNAVAILABLE_REASONS.len(), 11);
        for reason in CONTEXT_UNAVAILABLE_REASONS {
            let item = status_item_with_reason("session_fresh", Some(reason));
            assert_eq!(
                keys(&item),
                sorted(&["kind", "status", "reason"]),
                "{reason} must be published"
            );
            assert_eq!(item["reason"], serde_json::json!(reason));
            assert_eq!(item["status"], "session_fresh");
            assert_eq!(item["kind"], "status");
        }
        // No slug may leak an id, a path, or free text into a signed event.
        for reason in CONTEXT_UNAVAILABLE_REASONS {
            assert!(
                reason
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "{reason} must be a bare lowercase slug"
            );
        }
    }

    /// An unrecognized reason is dropped, not published as `null`: a `null` on
    /// a durable record claims the provider observed an absence.
    #[test]
    fn an_unrecognized_reason_is_omitted_not_published_as_null() {
        for reason in [
            "",
            "Fresh",
            "relay_query_failed ",
            "command 0123abcd has more than one provider receipt",
            "/Users/alice/state/context-packages",
            "no_prior_execution_v2",
        ] {
            let item = status_item_with_reason("session_restarted_without_context", Some(reason));
            assert!(
                item.get("reason").is_none(),
                "{reason:?} must not be published"
            );
            assert_eq!(keys(&item), sorted(&["kind", "status"]));
        }
    }

    /// Contract A: the echo of an operator prompt names the command that
    /// started the turn, so a consumer joins the transcript to the receipt by
    /// id instead of by matching prompt text.
    #[test]
    fn user_prompt_carries_the_command_that_started_the_turn() {
        let joined = user_prompt_item("go", false, None, Some("turn-1"), None, 0);
        assert_eq!(
            keys(&joined),
            sorted(&["kind", "content", "steered", "commandId"])
        );
        assert_eq!(joined["commandId"], "turn-1");

        let operator = "a".repeat(64);
        let both = user_prompt_item("go", true, Some(&operator), Some("turn-2"), None, 0);
        assert_eq!(
            keys(&both),
            sorted(&["kind", "content", "steered", "operatorPubkey", "commandId"])
        );
    }

    /// The key is additive: an echo with no command keeps exactly the shape
    /// every shipped consumer already reads, and a command id that could not
    /// have come off the wire is dropped rather than published.
    #[test]
    fn user_prompt_omits_a_command_that_is_absent_or_unbounded() {
        assert_eq!(
            keys(&user_prompt_item("go", false, None, None, None, 0)),
            sorted(&["kind", "content", "steered"])
        );
        for bad in [
            "",
            "   ",
            "with\u{1}control",
            &"c".repeat(crate::coding_session_command::MAX_IDENTIFIER_BYTES + 1),
        ] {
            let item = user_prompt_item("go", false, None, Some(bad), None, 0);
            assert!(
                item.get("commandId").is_none(),
                "{bad:?} must not be published as a commandId"
            );
        }
    }

    /// Contract B: `turn_started` is the only status that carries `turnId`,
    /// and it carries it always. Every other turn status keeps the five keys
    /// the shipped consumers require.
    #[test]
    fn turn_receipts_carry_exactly_the_locked_key_sets() {
        let started =
            serde_json::to_value(LifecycleReceipt::turn_started("t-1", &target(), "turn-abc"))
                .expect("serialize");
        assert_eq!(
            keys(&started),
            sorted(&[
                "schema",
                "commandId",
                "status",
                "session",
                "error",
                "turnId"
            ])
        );
        assert_eq!(started["status"], "turn_started");
        assert_eq!(started["turnId"], "turn-abc");
        assert!(started["error"].is_null());

        for (receipt, status, code) in [
            (
                LifecycleReceipt::turn_queued("t-1", &target()),
                "turn_queued",
                None,
            ),
            (
                LifecycleReceipt::turn_dropped("t-1", &target(), QUEUE_FULL, "queue full"),
                "turn_dropped",
                Some(QUEUE_FULL),
            ),
            (
                LifecycleReceipt::turn_refused("t-1", &target(), STALE_GENERATION, "stale"),
                "turn_refused",
                Some(STALE_GENERATION),
            ),
        ] {
            let value = serde_json::to_value(&receipt).expect("serialize");
            assert_eq!(
                keys(&value),
                sorted(&["schema", "commandId", "status", "session", "error"]),
                "{status} must not carry turnId"
            );
            assert_eq!(value["status"], status);
            assert_eq!(value["session"], serde_json::to_value(target()).unwrap());
            match code {
                None => assert!(value["error"].is_null()),
                Some(code) => assert_eq!(value["error"]["code"], code),
            }
            let json = serde_json::to_string(&receipt).unwrap();
            assert_eq!(
                decode_coding_session_lifecycle_receipt(&json).unwrap(),
                receipt
            );
        }

        let json = serde_json::to_string(&LifecycleReceipt::turn_started(
            "t-1",
            &target(),
            "turn-abc",
        ))
        .unwrap();
        assert_eq!(
            decode_coding_session_lifecycle_receipt(&json).unwrap(),
            LifecycleReceipt::turn_started("t-1", &target(), "turn-abc")
        );
    }

    /// `turnId` is present exactly when the status is `turn_started`: a
    /// started receipt without one names no turn, and a queued receipt with
    /// one claims a turn that has not begun.
    #[test]
    fn strict_receipt_decoder_enforces_turn_id_presence_by_status() {
        let mut started =
            serde_json::to_value(LifecycleReceipt::turn_started("t-1", &target(), "turn-abc"))
                .unwrap();
        started.as_object_mut().unwrap().remove("turnId");
        assert!(decode_coding_session_lifecycle_receipt(&started.to_string()).is_err());

        for status in [
            "turn_queued",
            "turn_dropped",
            "turn_refused",
            "turn_degraded",
            "turn_delivery_unknown",
            "created",
        ] {
            let mut wrong =
                serde_json::to_value(LifecycleReceipt::turn_queued("t-1", &target())).unwrap();
            wrong["status"] = serde_json::json!(status);
            wrong["turnId"] = serde_json::json!("turn-abc");
            assert!(
                decode_coding_session_lifecycle_receipt(&wrong.to_string()).is_err(),
                "{status} must not accept a turnId"
            );
        }

        let mut blank =
            serde_json::to_value(LifecycleReceipt::turn_started("t-1", &target(), "turn-abc"))
                .unwrap();
        blank["turnId"] = serde_json::json!("   ");
        assert!(decode_coding_session_lifecycle_receipt(&blank.to_string()).is_err());

        // An explicit `null` is a six-key object claiming an observed absence,
        // not the five-key shape. Both directions are rejected.
        for status in [
            "created",
            "turn_queued",
            "turn_dropped",
            "turn_refused",
            "turn_delivery_unknown",
        ] {
            let mut explicit_null =
                serde_json::to_value(LifecycleReceipt::turn_queued("t-1", &target())).unwrap();
            explicit_null["status"] = serde_json::json!(status);
            explicit_null["turnId"] = serde_json::Value::Null;
            assert!(
                decode_coding_session_lifecycle_receipt(&explicit_null.to_string()).is_err(),
                "{status} must not accept an explicit null turnId"
            );
        }
        let mut started_null =
            serde_json::to_value(LifecycleReceipt::turn_started("t-1", &target(), "turn-abc"))
                .unwrap();
        started_null["turnId"] = serde_json::Value::Null;
        assert!(decode_coding_session_lifecycle_receipt(&started_null.to_string()).is_err());
    }

    /// A turn receipt is not a lifecycle outcome: the shapes that a create,
    /// resume, or stop receipt is allowed to take must stay closed against
    /// the turn vocabulary, and vice versa.
    #[test]
    fn turn_statuses_are_distinguishable_from_lifecycle_statuses() {
        for status in [
            ReceiptStatus::TurnQueued,
            ReceiptStatus::TurnStarted,
            ReceiptStatus::TurnDropped,
            ReceiptStatus::TurnRefused,
            ReceiptStatus::TurnDegraded,
            ReceiptStatus::TurnInjected,
            ReceiptStatus::TurnDeliveryUnknown,
            ReceiptStatus::InterruptDelivered,
        ] {
            assert!(status.is_turn_stage(), "{status:?}");
            assert_eq!(
                status.carries_turn_id(),
                matches!(
                    status,
                    ReceiptStatus::TurnStarted | ReceiptStatus::TurnInjected
                ),
                "{status:?}"
            );
        }
        for status in [
            ReceiptStatus::Created,
            ReceiptStatus::CreatedWithFailedInitialTurn,
            ReceiptStatus::Failed,
            ReceiptStatus::Resumed,
            ReceiptStatus::ResumedWithoutContext,
            ReceiptStatus::Stopped,
        ] {
            assert!(!status.is_turn_stage(), "{status:?}");
        }
        assert_eq!(ReceiptStatus::TurnStarted.as_str(), "turn_started");
        assert_eq!(ReceiptStatus::TurnInjected.as_str(), "turn_injected");
        assert_eq!(
            ReceiptStatus::TurnDeliveryUnknown.as_str(),
            "turn_delivery_unknown"
        );
        assert_eq!(ReceiptStatus::Created.as_str(), "created");
    }

    /// `turn_injected` is the second six-key receipt: it names the turn the
    /// input joined, exactly as `turn_started` names the turn that began.
    /// The consumer's `hasExactKeys` makes the key set the contract.
    #[test]
    fn turn_injected_is_a_six_key_receipt_that_round_trips_strictly() {
        let receipt = LifecycleReceipt::turn_injected("steer-1", &target(), "turn-7");
        let value = serde_json::to_value(&receipt).expect("serialize");
        assert_eq!(
            keys(&value),
            sorted(&[
                "schema",
                "commandId",
                "status",
                "session",
                "error",
                "turnId"
            ])
        );
        assert_eq!(value["status"], "turn_injected");
        assert!(value["error"].is_null());
        assert_eq!(value["turnId"], "turn-7");
        assert_eq!(value["session"], serde_json::to_value(target()).unwrap());
        assert_eq!(
            decode_coding_session_lifecycle_receipt(&value.to_string()).expect("decode"),
            receipt
        );

        // Without the turn it joined it names nothing; a blank or explicit
        // null is the same absence dressed up as a key.
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("turnId");
        assert!(decode_coding_session_lifecycle_receipt(&missing.to_string()).is_err());
        let mut blank = value.clone();
        blank["turnId"] = serde_json::json!("  ");
        assert!(decode_coding_session_lifecycle_receipt(&blank.to_string()).is_err());
        let mut null = value.clone();
        null["turnId"] = serde_json::Value::Null;
        assert!(decode_coding_session_lifecycle_receipt(&null.to_string()).is_err());
        // An injected input was delivered: it carries no error.
        let mut errored = value;
        errored["error"] = serde_json::json!({ "code": STEER_ACK_LOST, "message": "x" });
        assert!(decode_coding_session_lifecycle_receipt(&errored.to_string()).is_err());
    }

    /// `turn_delivery_unknown` is a five-key terminal answer that says *why*
    /// delivery could not be established, so it needs a well-formed code and
    /// must not claim a turn it cannot name.
    #[test]
    fn turn_delivery_unknown_requires_a_well_formed_code_and_no_turn_id() {
        for code in [
            STEER_WRITE_FAILED,
            STEER_ACK_LOST,
            STEER_ACK_TIMEOUT,
            STEER_ACK_UNRECOGNIZED,
            STEER_UNRESOLVED_AT_RESTART,
            STEER_UNOBSERVED_NEW_TURN,
        ] {
            let receipt = LifecycleReceipt::turn_delivery_unknown(
                "steer-1",
                &target(),
                code,
                "the prompt ended before the acknowledgement arrived",
            );
            let value = serde_json::to_value(&receipt).expect("serialize");
            assert_eq!(
                keys(&value),
                sorted(&["schema", "commandId", "status", "session", "error"]),
                "{code}"
            );
            assert_eq!(value["status"], "turn_delivery_unknown");
            assert_eq!(value["error"]["code"], code);
            assert_eq!(
                decode_coding_session_lifecycle_receipt(&value.to_string()).expect("decode"),
                receipt,
                "{code}"
            );
            assert!(receipt.error.expect("error").code.len() <= MAX_RECEIPT_ERROR_CODE_BYTES);
        }

        let value = serde_json::to_value(LifecycleReceipt::turn_delivery_unknown(
            "steer-1",
            &target(),
            STEER_ACK_LOST,
            "lost",
        ))
        .unwrap();
        // The code is open but still has to be a code.
        for bad in [
            "",
            "   ",
            "STEER\u{7}ACK",
            &"c".repeat(MAX_RECEIPT_ERROR_CODE_BYTES + 1),
        ] {
            let mut wrong = value.clone();
            wrong["error"]["code"] = serde_json::json!(bad);
            assert!(
                decode_coding_session_lifecycle_receipt(&wrong.to_string()).is_err(),
                "code {bad:?} must be refused"
            );
        }
        // No error at all is not "unknown", it is a claim of delivery.
        let mut no_error = value.clone();
        no_error["error"] = serde_json::Value::Null;
        assert!(decode_coding_session_lifecycle_receipt(&no_error.to_string()).is_err());
        // It is not a six-key receipt: a turnId would claim the input joined
        // a turn, which is exactly what this status cannot say.
        let mut with_turn = value;
        with_turn["turnId"] = serde_json::json!("turn-7");
        assert!(decode_coding_session_lifecycle_receipt(&with_turn.to_string()).is_err());

        // A blank message is bounded and never blank on the wire.
        let blank = LifecycleReceipt::turn_delivery_unknown("s", &target(), STEER_ACK_LOST, " ");
        assert_eq!(
            blank.error.expect("error").message,
            "unspecified provider error"
        );
    }

    /// A turn-stage refusal, drop, or downgrade carries an *open* code: the
    /// decoder checks that it is a well-formed code, not that this build has
    /// heard of it.
    ///
    /// The closed list this replaced meant a provider that learned a new way
    /// to lose a turn could not say so without a coordinated release of every
    /// reader — and the alternative to saying so is a turn that vanishes. What
    /// stays enforced is shape: nonblank, control-free, bounded.
    #[test]
    fn turn_stage_codes_are_open_but_still_have_to_be_codes() {
        for (status, code) in [
            (ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION),
            (
                ReceiptStatus::TurnDropped,
                "SOMETHING_THIS_BUILD_NEVER_HEARD_OF",
            ),
            (ReceiptStatus::TurnRefused, NO_TURN_IN_FLIGHT),
            (ReceiptStatus::TurnDegraded, STEER_UNSUPPORTED),
        ] {
            let mut receipt = serde_json::to_value(LifecycleReceipt::turn_dropped(
                "t-1",
                &target(),
                code,
                "why",
            ))
            .unwrap_or_default();
            receipt["status"] = serde_json::json!(status.as_str());
            decode_coding_session_lifecycle_receipt(&receipt.to_string())
                .unwrap_or_else(|error| panic!("{status:?}/{code} rejected: {error}"));
        }

        for rejected in [
            "",
            "   ",
            "HAS\nNEWLINE",
            &"A".repeat(MAX_RECEIPT_ERROR_CODE_BYTES + 1),
        ] {
            let mut receipt = serde_json::to_value(LifecycleReceipt::turn_dropped(
                "t-1",
                &target(),
                QUEUE_FULL,
                "full",
            ))
            .unwrap_or_default();
            receipt["error"] = serde_json::json!({ "code": rejected, "message": "full" });
            assert!(
                decode_coding_session_lifecycle_receipt(&receipt.to_string()).is_err(),
                "accepted code {rejected:?}"
            );
        }

        // Opening the code list changed nothing else about the shape.
        let mut queued = serde_json::to_value(LifecycleReceipt::turn_queued("t-1", &target()))
            .unwrap_or_default();
        queued["error"] = serde_json::json!({ "code": QUEUE_FULL, "message": "full" });
        assert!(decode_coding_session_lifecycle_receipt(&queued.to_string()).is_err());

        let mut headless = serde_json::to_value(LifecycleReceipt::turn_queued("t-1", &target()))
            .unwrap_or_default();
        headless["session"] = serde_json::Value::Null;
        assert!(decode_coding_session_lifecycle_receipt(&headless.to_string()).is_err());
    }

    /// The two receipts this slice adds are exactly five keys each, and each
    /// says one thing: `turn_degraded` always names why it was downgraded,
    /// `interrupt_delivered` never carries an error because a delivered
    /// cancel is not a failure.
    #[test]
    fn degraded_and_interrupt_delivered_hold_their_exact_shapes() {
        let degraded = LifecycleReceipt::turn_degraded(
            "t-1",
            &target(),
            STEER_UNSUPPORTED,
            "cannot steer; queued for the next boundary",
        );
        let encoded = serde_json::to_value(&degraded).unwrap_or_default();
        assert_eq!(
            keys(&encoded),
            vec!["commandId", "error", "schema", "session", "status"]
        );
        assert_eq!(encoded["status"], "turn_degraded");
        assert_eq!(encoded["error"]["code"], STEER_UNSUPPORTED);
        assert_eq!(
            decode_coding_session_lifecycle_receipt(&encoded.to_string())
                .unwrap_or_else(|error| { panic!("turn_degraded rejected: {error}") }),
            degraded
        );

        let delivered = LifecycleReceipt::interrupt_delivered("t-2", &target());
        let encoded = serde_json::to_value(&delivered).unwrap_or_default();
        assert_eq!(
            keys(&encoded),
            vec!["commandId", "error", "schema", "session", "status"]
        );
        assert_eq!(encoded["status"], "interrupt_delivered");
        assert!(encoded["error"].is_null());
        assert_eq!(
            encoded["session"],
            serde_json::to_value(target()).unwrap_or_default()
        );
        decode_coding_session_lifecycle_receipt(&encoded.to_string())
            .unwrap_or_else(|error| panic!("interrupt_delivered rejected: {error}"));

        // An interrupt that says it was delivered *and* failed is incoherent.
        let mut contradictory = serde_json::to_value(&delivered).unwrap_or_default();
        contradictory["error"] = serde_json::json!({ "code": QUEUE_FULL, "message": "full" });
        assert!(decode_coding_session_lifecycle_receipt(&contradictory.to_string()).is_err());
    }

    /// `continuation_registered` is a stage of one 44220 and carries no
    /// error: everything a CI continuation can be refused for happens later,
    /// as a turn stage of the same command.
    #[test]
    fn continuation_registered_holds_its_exact_shape_and_is_a_turn_stage() {
        let registered =
            LifecycleReceipt::continuation_registered("cic-0123456789abcdef", &target());
        let encoded = serde_json::to_value(&registered).unwrap_or_default();
        assert_eq!(
            keys(&encoded),
            vec!["commandId", "error", "schema", "session", "status"],
            "a registration must not carry turnId: no turn has begun"
        );
        assert_eq!(encoded["status"], "continuation_registered");
        assert!(encoded["error"].is_null());
        assert_eq!(
            encoded["session"],
            serde_json::to_value(target()).unwrap_or_default()
        );
        assert_eq!(
            decode_coding_session_lifecycle_receipt(&encoded.to_string())
                .unwrap_or_else(|error| panic!("continuation_registered rejected: {error}")),
            registered
        );
        assert_eq!(
            ReceiptStatus::ContinuationRegistered.as_str(),
            "continuation_registered"
        );
        // It answers a 44220, so a fold deciding what happened to a
        // *generation* must skip it exactly as it skips the other turn
        // stages — a registration creates, confirms, and ends nothing.
        assert!(ReceiptStatus::ContinuationRegistered.is_turn_stage());

        // A registration that also names a turn, or a failure, is incoherent:
        // nothing was queued and nothing has been refused yet.
        let mut with_turn_id = encoded.clone();
        with_turn_id["turnId"] = serde_json::json!("turn-abc");
        assert!(decode_coding_session_lifecycle_receipt(&with_turn_id.to_string()).is_err());
        let mut with_error = encoded.clone();
        with_error["error"] =
            serde_json::json!({ "code": CI_CONTINUATION_EXPIRED, "message": "expired" });
        assert!(decode_coding_session_lifecycle_receipt(&with_error.to_string()).is_err());

        // The refusals a continuation earns ride the existing turn vocabulary
        // with the new codes, and those codes are well formed.
        for code in [
            CI_CONTINUATION_EXPIRED,
            CI_RESULT_CONFLICT,
            CI_RESULT_UNAVAILABLE_OR_HIDDEN,
            COMMAND_ID_CONFLICT,
            CI_CONTINUATION_STORE_FULL,
        ] {
            assert!(is_receipt_error_code(code), "malformed code {code}");
            let refused =
                LifecycleReceipt::turn_refused("cic-0123456789abcdef", &target(), code, "no");
            let encoded = serde_json::to_value(&refused).unwrap_or_default();
            assert_eq!(
                decode_coding_session_lifecycle_receipt(&encoded.to_string())
                    .unwrap_or_else(|error| panic!("{code} rejected: {error}")),
                refused
            );
        }
    }

    /// `with_thread_steer` changes exactly one capability and nothing else.
    ///
    /// Scope, stated because the previous comment here did not: this pins the
    /// override, not the generation fold. Whether a consumer ignores turn
    /// stages when folding a generation's status is that consumer's test —
    /// `buzz-cli`'s `a_turn_receipt_does_not_confirm_or_change_the_status_of_a_
    /// known_target`, which is where that guarantee was in fact broken while
    /// this comment claimed to cover it.
    #[test]
    fn per_execution_thread_steer_overrides_only_that_capability() {
        let base = Capabilities::v1_claude();
        let steering = base.with_thread_steer(true);
        assert!(steering.thread_steer);
        assert_eq!(
            Capabilities {
                thread_steer: false,
                ..steering
            },
            base
        );
        assert!(!base.with_thread_steer(false).thread_steer);
    }

    /// `with_prompt_image` changes exactly one capability and nothing else,
    /// and metadata published before the field existed still decodes — as
    /// `false`, the honest reading of a provider that never claimed images.
    #[test]
    fn per_execution_prompt_image_overrides_only_that_capability() {
        let base = Capabilities::v1_claude();
        assert!(
            !base.prompt_image,
            "a static driver vector must not claim image support it has not witnessed"
        );
        let imaging = base.with_prompt_image(true);
        assert!(imaging.prompt_image);
        assert_eq!(
            Capabilities {
                prompt_image: false,
                ..imaging
            },
            base
        );

        let legacy: Capabilities = serde_json::from_str(
            r#"{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":true}"#,
        )
        .expect("metadata predating promptImage must still decode");
        assert!(!legacy.prompt_image);
    }

    /// The `usage` block is optional and additive: a result item built without
    /// one is byte-identical to the shape that shipped before it existed.
    #[test]
    fn a_result_item_without_usage_carries_no_usage_key() {
        let item = result_item(
            ResultSubtype::Success,
            1_000,
            "completed",
            TurnCost::default(),
            TurnUsageReport::default(),
        );
        assert!(item.get("usage").is_none(), "{item}");
    }

    /// Every field the provider knows reaches the wire under its camelCase
    /// name, and nothing it does not know is serialized as `null`.
    #[test]
    fn a_result_item_with_usage_carries_exactly_the_known_fields() {
        let item = result_item(
            ResultSubtype::Success,
            1_000,
            "completed",
            TurnCost::default(),
            TurnUsageReport {
                input_tokens: Some(1_200),
                output_tokens: Some(340),
                cache_read_tokens: Some(96_000),
                cache_write_tokens: Some(4_000),
                tool_calls: Some(7),
                context_window: Some(1_000_000),
            },
        );
        assert_eq!(
            item["usage"],
            serde_json::json!({
                "inputTokens": 1_200,
                "outputTokens": 340,
                "cacheReadTokens": 96_000,
                "cacheWriteTokens": 4_000,
                "toolCalls": 7,
                "contextWindow": 1_000_000,
            })
        );
    }

    /// A partly-known block omits the keys it does not know rather than
    /// claiming a measured zero.
    #[test]
    fn an_unknown_usage_field_is_omitted_not_zeroed() {
        let item = result_item(
            ResultSubtype::Success,
            1,
            "completed",
            TurnCost::default(),
            TurnUsageReport {
                output_tokens: Some(5),
                ..TurnUsageReport::default()
            },
        );
        assert_eq!(item["usage"], serde_json::json!({ "outputTokens": 5 }));
    }

    /// `usedTokens` is the prompt-side total: fresh input plus both cache
    /// subsets. The three fields are disjoint by contract, so this is a sum.
    #[test]
    fn used_tokens_sums_the_three_prompt_side_fields() {
        let usage = TurnUsageReport {
            input_tokens: Some(1_200),
            cache_read_tokens: Some(96_000),
            cache_write_tokens: Some(4_000),
            output_tokens: Some(999),
            ..TurnUsageReport::default()
        };
        assert_eq!(usage.used_tokens(), Some(101_200));
    }

    /// A block that reports none of the three prompt-side fields knows no
    /// prompt size — it must say so rather than report zero.
    #[test]
    fn used_tokens_is_unknown_when_no_prompt_side_field_was_reported() {
        let usage = TurnUsageReport {
            output_tokens: Some(999),
            tool_calls: Some(2),
            ..TurnUsageReport::default()
        };
        assert_eq!(usage.used_tokens(), None);
    }

    /// A `context_window_updated` item is the driver's own statement of how
    /// full the window is; both names it uses are read.
    #[test]
    fn context_window_usage_reads_the_drivers_own_occupancy_item() {
        let item = serde_json::json!({
            "kind": "context_window_updated",
            "usage": { "size": 1_000_000, "used": 137_498 },
        });
        assert_eq!(
            context_window_usage(&item),
            Some(ContextWindowUsage {
                used_tokens: 137_498,
                context_window: Some(1_000_000),
            })
        );
    }

    /// A driver that reports occupancy without a window still gets read; the
    /// window stays unknown rather than being guessed.
    #[test]
    fn context_window_usage_without_a_size_leaves_the_window_unknown() {
        let item = serde_json::json!({
            "kind": "context_window_updated",
            "usage": { "usedTokens": 42 },
        });
        assert_eq!(
            context_window_usage(&item),
            Some(ContextWindowUsage {
                used_tokens: 42,
                context_window: None,
            })
        );
    }

    /// Any other item kind is not an occupancy statement, whatever it carries.
    #[test]
    fn context_window_usage_ignores_other_item_kinds() {
        let item = serde_json::json!({ "kind": "result", "usage": { "used": 9 } });
        assert_eq!(context_window_usage(&item), None);
    }

    fn change(path: &str, old_text: Option<&str>, new_text: Option<&str>) -> ToolEditChange {
        ToolEditChange {
            path: Some(path.to_owned()),
            old_text: old_text.map(str::to_owned),
            new_text: new_text.map(str::to_owned),
        }
    }

    /// The payload names the files and carries the adapter's own texts.
    #[test]
    fn an_edit_payload_names_its_files_and_keeps_the_adapters_texts() {
        let payload = tool_edit_payload(
            &["a.rs".to_owned(), "a.rs".to_owned(), " b.rs ".to_owned()],
            &[change("a.rs", Some("one"), Some("two"))],
        )
        .expect("payload");
        assert_eq!(payload["paths"], serde_json::json!(["a.rs", "b.rs"]));
        assert_eq!(payload["changes"][0]["path"], "a.rs");
        assert_eq!(payload["changes"][0]["oldText"], "one");
        assert_eq!(payload["changes"][0]["newText"], "two");
        assert!(payload.get("truncated").is_none());
    }

    /// A new file has no previous text — that is different from an empty one,
    /// so the key is absent rather than `""`.
    #[test]
    fn a_new_file_reports_no_old_text_rather_than_an_empty_one() {
        let payload =
            tool_edit_payload(&[], &[change("a.rs", None, Some("hello"))]).expect("payload");
        assert!(payload["changes"][0].get("oldText").is_none());
        assert_eq!(payload["changes"][0]["newText"], "hello");
    }

    /// Nothing observed publishes nothing — an empty object would claim an
    /// observation nobody made.
    #[test]
    fn an_edit_payload_with_nothing_in_it_is_none() {
        assert!(tool_edit_payload(&[], &[]).is_none());
        assert!(tool_edit_payload(
            &["   ".to_owned()],
            &[ToolEditChange {
                path: Some("  ".to_owned()),
                old_text: None,
                new_text: None,
            }],
        )
        .is_none());
    }

    /// Over the cap, texts are shortened first and each shortened change says
    /// so; the file names survive, because naming the file is the point.
    #[test]
    fn an_oversized_edit_payload_shrinks_texts_before_dropping_changes() {
        let huge = "x".repeat(MAX_TOOL_EDIT_PAYLOAD_BYTES);
        let payload = tool_edit_payload(
            &["big.rs".to_owned()],
            &[change("big.rs", Some(&huge), Some(&huge))],
        )
        .expect("payload");
        assert!(payload.to_string().len() <= MAX_TOOL_EDIT_PAYLOAD_BYTES);
        assert_eq!(payload["paths"], serde_json::json!(["big.rs"]));
        assert_eq!(payload["changes"][0]["truncated"], true);
    }

    /// When shortening is not enough, whole changes are dropped — and the
    /// payload itself is flagged, so a reader never mistakes a dropped change
    /// for a change that never happened.
    #[test]
    fn dropping_a_whole_change_is_flagged_on_the_payload() {
        let huge = "x".repeat(MAX_TOOL_EDIT_PAYLOAD_BYTES);
        let changes: Vec<ToolEditChange> = (0..512)
            .map(|index| change(&format!("f{index}.rs"), Some(&huge), Some(&huge)))
            .collect();
        let payload = tool_edit_payload(&[], &changes).expect("payload");
        assert!(payload.to_string().len() <= MAX_TOOL_EDIT_PAYLOAD_BYTES);
        assert_eq!(payload["truncated"], true);
        let kept = payload["changes"].as_array().expect("changes").len();
        assert!(kept < changes.len(), "kept {kept} of {}", changes.len());
    }
}
