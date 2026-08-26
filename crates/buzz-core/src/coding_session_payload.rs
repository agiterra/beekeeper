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
/// A new generation started, but the provider could not recover prior context.
pub const CONTEXT_NOT_RECOVERED: &str = "CONTEXT_NOT_RECOVERED";
/// A create named a genesis event that could not be resolved and verified.
pub const GENESIS_NOT_FOUND: &str = "GENESIS_NOT_FOUND";
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
/// The command is *not* consumed when this is published: the execution can be
/// resumed, and the turn is still owed. A drop is the honest answer to "where
/// did my turn go", and it replaces the log line that used to be the only
/// record of it.
pub const NO_LIVE_EXECUTION: &str = "NO_LIVE_EXECUTION";
/// A `steer` delivery was requested of a runtime that never advertised native
/// mid-turn steering, so the turn was delivered at the next boundary instead.
///
/// The only code a `turn_degraded` receipt carries today. Degraded is not
/// refused: the turn still runs, just later than the sender asked.
pub const STEER_UNSUPPORTED: &str = "STEER_UNSUPPORTED";
/// An interrupt addressed a live execution that had no turn in flight, so
/// there was nothing to cancel.
pub const NO_TURN_IN_FLIGHT: &str = "NO_TURN_IN_FLIGHT";

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
    /// The turn could not be accepted because the queue is full.
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
    /// A `thread.turn.interrupt` reached a live turn and its cancel was
    /// issued. The turn's own `result` item reports how it actually ended.
    #[serde(rename = "interrupt_delivered")]
    InterruptDelivered,
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
            Self::InterruptDelivered => "interrupt_delivered",
        }
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
                | Self::InterruptDelivered
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
    /// Today that is only [`STEER_UNSUPPORTED`]: a `steer` addressed to a
    /// runtime that never advertised native mid-turn steering. The turn is not
    /// refused and not lost — a `turn_queued` follows and it runs at the next
    /// boundary. Saying so is the whole point: a silent downgrade would let an
    /// operator believe the agent was steered mid-thought.
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
    if carries_turn_id != (receipt.status == ReceiptStatus::TurnStarted) {
        return Err("receipt turnId key is present exactly when status is turn_started".into());
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
    let expects_turn_id = receipt.status == ReceiptStatus::TurnStarted;
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
            return Err("receipt turnId is present exactly when status is turn_started".into());
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
        ReceiptStatus::TurnQueued | ReceiptStatus::TurnStarted => {
            receipt.session.is_some() && receipt.error.is_none()
        }
        // Open codes, deliberately. Pinning a list here meant a provider that
        // learned a new way to lose or refuse a turn could not report it
        // without a coordinated release of every reader — and the desktop
        // already renders an unknown code verbatim. What is still enforced is
        // that the code is well formed.
        ReceiptStatus::TurnDropped | ReceiptStatus::TurnRefused | ReceiptStatus::TurnDegraded => {
            receipt.session.is_some()
                && receipt
                    .error
                    .as_ref()
                    .is_some_and(|error| is_receipt_error_code(&error.code))
        }
        ReceiptStatus::InterruptDelivered => receipt.session.is_some() && receipt.error.is_none(),
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
    /// Managed-agent reference. Always `null`: this provider is not one.
    pub agent_ref: Option<String>,
    /// Advertised provider instance reference.
    pub provider: Option<String>,
    /// Runtime slug behind the driver.
    pub runtime: Option<String>,
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
}

/// Expected JSON key sets for [`SessionMetadata`], oldest first.
///
/// Two independent additive amendments have landed on this struct at
/// different times — the `sessionRef` echo, then B1's four coordinate-fact
/// keys — so there are four valid shapes, not two: base, base+sessionRef,
/// base+facts, and base+sessionRef+facts. Mirrors the exact-fields
/// discipline in `coding_session_lifecycle_command.rs`
/// (`rejects_action_shapes_between_and_beyond_the_two_forms`): every shape
/// in between or beyond these four — a partial subset of the four fact
/// keys, or any field this struct does not know — is rejected, not
/// tolerated.
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
const METADATA_FACT_FIELDS: &[&str] = &["observedCommit", "dirty", "relayReachable", "verifiedAt"];

/// Strictly decode and validate signed metadata content (kind 44223).
///
/// Accepts exactly the four field-set shapes documented above
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
    if has_all_facts {
        expected.extend_from_slice(METADATA_FACT_FIELDS);
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
        ("projectRef", &metadata.project_ref),
        ("repoRef", &metadata.repo_ref),
        ("title", &metadata.title),
        ("agentRef", &metadata.agent_ref),
        ("provider", &metadata.provider),
        ("runtime", &metadata.runtime),
        ("model", &metadata.model),
        ("branch", &metadata.branch),
        ("observedCommit", &metadata.observed_commit),
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

/// Build the terminal `result` item that closes a turn.
///
/// `costUsd` and the token counts are omitted rather than sent as `null`: the
/// consumer's projector renders `costUsd` only when it is a number, and an
/// explicit `null` would claim the provider measured zero.
pub fn result_item(
    subtype: ResultSubtype,
    duration_ms: u64,
    result: &str,
    cost: TurnCost,
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
/// Both keys are **additive and optional**: `operatorPubkey` is emitted only
/// for a well-formed 64-character lowercase-hex pubkey, `commandId` only for a
/// nonblank, control-free identifier within
/// [`MAX_IDENTIFIER_BYTES`](crate::coding_session_command::MAX_IDENTIFIER_BYTES).
/// Anything else is omitted entirely rather than sent as `null`, which would
/// claim the provider observed an absence. Items published before either field
/// existed stay valid everywhere.
pub fn user_prompt_item(
    content: &str,
    steered: bool,
    operator_pubkey: Option<&str>,
    command_id: Option<&str>,
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
            provider: Some("claude-primary".into()),
            runtime: Some("claude".into()),
            model: Some("claude-sonnet-4-6".into()),
            status: SessionStatus::Idle,
            branch: None,
            capabilities: Capabilities::v1_claude(),
            session_ref: None,
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
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
            provider: Some("codex-primary".into()),
            runtime: Some("codex".into()),
            model: None,
            status: SessionStatus::Running,
            branch: None,
            capabilities: Capabilities::v1_baseline(),
            session_ref: None,
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
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
        let unattributed = user_prompt_item("go", false, None, None);
        assert_eq!(keys(&unattributed), sorted(&["kind", "content", "steered"]));

        let operator = "a".repeat(64);
        let attributed = user_prompt_item("go", true, Some(&operator), None);
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
            let item = user_prompt_item("go", false, Some(bad), None);
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
        let joined = user_prompt_item("go", false, None, Some("turn-1"));
        assert_eq!(
            keys(&joined),
            sorted(&["kind", "content", "steered", "commandId"])
        );
        assert_eq!(joined["commandId"], "turn-1");

        let operator = "a".repeat(64);
        let both = user_prompt_item("go", true, Some(&operator), Some("turn-2"));
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
            keys(&user_prompt_item("go", false, None, None)),
            sorted(&["kind", "content", "steered"])
        );
        for bad in [
            "",
            "   ",
            "with\u{1}control",
            &"c".repeat(crate::coding_session_command::MAX_IDENTIFIER_BYTES + 1),
        ] {
            let item = user_prompt_item("go", false, None, Some(bad));
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

        for status in ["turn_queued", "turn_dropped", "turn_refused", "created"] {
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
        for status in ["created", "turn_queued", "turn_dropped", "turn_refused"] {
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
            ReceiptStatus::InterruptDelivered,
        ] {
            assert!(status.is_turn_stage(), "{status:?}");
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
        assert_eq!(ReceiptStatus::Created.as_str(), "created");
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
}
