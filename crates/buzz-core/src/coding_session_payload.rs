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

/// Lifecycle outcome for exactly one create, resume, or stop command (kind 44224).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleReceipt {
    /// Always [`LIFECYCLE_RECEIPT_SCHEMA`].
    pub schema: String,
    /// The `commandId` of the lifecycle command this answers.
    pub command_id: String,
    /// Exact lifecycle outcome.
    pub status: ReceiptStatus,
    /// The minted target, or `null` when the create failed outright.
    pub session: Option<CodingSessionTarget>,
    /// Failure detail, or `null` for a clean `created`.
    pub error: Option<ReceiptError>,
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
        }
    }
}

/// Strictly decode and validate one immutable lifecycle receipt.
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
    if object.len() != FIELDS.len()
        || FIELDS.iter().any(|field| !object.contains_key(*field))
        || object.keys().any(|field| !FIELDS.contains(&field.as_str()))
    {
        return Err("coding-session lifecycle receipt has missing or unsupported fields".into());
    }
    let receipt: LifecycleReceipt = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session lifecycle receipt: {error}"))?;
    validate_lifecycle_receipt(&receipt)?;
    Ok(receipt)
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
    /// Mid-turn steering without cancelling. Not offered in v1.
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
/// The key is **additive and optional**: it is emitted only for a well-formed
/// 64-character lowercase-hex pubkey, and omitted entirely otherwise rather
/// than sent as `null`, which would claim the provider observed "no operator".
/// Items published before this field existed stay valid everywhere.
pub fn user_prompt_item(
    content: &str,
    steered: bool,
    operator_pubkey: Option<&str>,
) -> serde_json::Value {
    let mut item =
        serde_json::json!({ "kind": "user_prompt", "content": content, "steered": steered });
    // The literal above is an object, so this always matches; written as a
    // pattern rather than an `expect` so a future edit degrades into an
    // unattributed prompt rather than a panic at the head of a turn.
    if let (Some(object), Some(pubkey)) = (item.as_object_mut(), operator_pubkey) {
        if is_operator_pubkey(pubkey) {
            object.insert("operatorPubkey".into(), serde_json::json!(pubkey));
        }
    }
    item
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
        let unattributed = user_prompt_item("go", false, None);
        assert_eq!(keys(&unattributed), sorted(&["kind", "content", "steered"]));

        let operator = "a".repeat(64);
        let attributed = user_prompt_item("go", true, Some(&operator));
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
            let item = user_prompt_item("go", false, Some(bad));
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
}
