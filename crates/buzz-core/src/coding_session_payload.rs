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

use crate::coding_session_command::CodingSessionTarget;

/// Schema string on every lifecycle receipt.
pub const LIFECYCLE_RECEIPT_SCHEMA: &str = "buzz-coding-session-lifecycle-receipt/v1";
/// Schema string on every metadata event.
pub const METADATA_SCHEMA: &str = "buzz-coding-session-metadata/v1";
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

/// Lifecycle outcome for exactly one create command (kind 44224).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleReceipt {
    /// Always [`LIFECYCLE_RECEIPT_SCHEMA`].
    pub schema: String,
    /// The `commandId` of the create this answers.
    pub command_id: String,
    /// `created`, `created_with_failed_initial_turn`, or `failed`.
    pub status: ReceiptStatus,
    /// The minted target, or `null` when the create failed outright.
    pub session: Option<CodingSessionTarget>,
    /// Failure detail, or `null` for a clean `created`.
    pub error: Option<ReceiptError>,
}

/// The three receipt outcomes the consumer recognizes.
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
}

/// Machine-readable code plus an operator-facing message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
/// Field order is the consumer's declared key order; `agentRef` and `branch` are
/// structurally required and always `null` here — this provider is not a managed
/// agent and does not claim to know the checkout's branch.
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
    /// Checked-out branch. Always `null` — not observed over ACP.
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

/// Build a bounded `status` item — the projector renders it as a lifecycle row.
pub fn status_item(status: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "status", "status": status })
}

/// Build the `user_prompt` item that opens a turn.
pub fn user_prompt_item(content: &str, steered: bool) -> serde_json::Value {
    serde_json::json!({ "kind": "user_prompt", "content": content, "steered": steered })
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

    #[test]
    fn session_status_strings_match_the_donor_allowlist() {
        let allowed = [
            (SessionStatus::Starting, "starting"),
            (SessionStatus::Idle, "idle"),
            (SessionStatus::Running, "running"),
            (SessionStatus::WaitingForInput, "waiting_for_input"),
            (SessionStatus::Completed, "completed"),
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

    #[test]
    fn target_serializes_with_the_four_keys_the_consumer_decodes() {
        let value = serde_json::to_value(target()).expect("serialize");
        assert_eq!(
            keys(&value),
            sorted(&["driver", "instanceId", "sessionId", "generation"])
        );
    }
}
