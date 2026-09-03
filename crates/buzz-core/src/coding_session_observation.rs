//! NIP-CSOB: signed coding-session observations (kind 44246).
//!
//! Four facts a person watching an agent team needs and cannot get from the
//! governance record: where a seat is in its own loop, what its gates said,
//! what it found and did about it, and how long a phase took. They are
//! **observations**, not transactions: nothing here settles anything, nothing
//! here authorizes anything, and nothing here can deny anyone else's record.
//!
//! # Why this is its own kind and not four more 44244 subtypes
//!
//! Three consequences of 44244's own fold decided it.
//!
//! 1. **A 44244 envelope error fails the whole set.**
//!    `fold_coding_session_team_transactions` returns `Err` when
//!    `validate_coding_session_team_transaction_envelope` refuses one event
//!    (`coding_session_team_transaction_fold.rs`) — one of the *three further*
//!    whole-set failures that kind documents beside its two record-class hard
//!    errors, cross-context and cycles — and the operation vocabulary is
//!    a closed serde enum with no `other` arm. So any build that predates a new
//!    token reads a session carrying one gate row as a **broken mission** —
//!    the finding-13 cliff batch 2 has just paid for, this time on the stream
//!    every seat writes many times an hour. A build that never heard of 44246
//!    simply never queries it.
//! 2. **The governance fold is bounded for a handful of assignments.** Hundreds
//!    of observations in the same fold would evict the records mission state
//!    depends on.
//! 3. **Observations carry no authority, supersession or causal reference.** The
//!    correction validator and the twelve exclusion codes buy nothing here, and
//!    they would let one observation's defect become a governance disclosure.
//!
//! # What this module validates, and what it does not
//!
//! Only facts self-contained in one event: exact JSON and tag shape, closed
//! vocabularies, bounds, and tag-to-content parity. The event signature is the
//! sole author; there is no author field. Whether the signer held a seat is the
//! consumer's question against the accepted NIP-CSAT chain — **any active seat
//! or the founder may observe, and the relay checks structure only**, the same
//! division NIP-CSP draws.
//!
//! # Rules v1 commits to
//!
//! - Unknown top-level or body keys are **rejected, not ignored**.
//! - **Absent is not null.** Every key is always present; an unset optional is
//!   JSON `null`, and a `null` where a value is required is refused by name.
//! - There is **no `supersedes` key**. A later observation with the same
//!   `(author, findingId)` or `(author, gate)` is simply the newer statement,
//!   and the older one stays on the wire where a reader can find it.
//! - Every duration and `startedAtMs` is the **author's own measurement**. It
//!   is disclosed as such and never used for ordering, discovery or dedupe.
//! - Every observation says **how it was produced**. `source` is `observed`
//!   when a mechanism that was not the subject wrote the record — the provider
//!   deriving a gate row from a seat's own tool calls, a git hook writing a
//!   checkpoint — and `declared` when the subject said it about itself. The
//!   two are never merged: a reader prefers the observed row and shows the
//!   word, because "the tests passed" is a different fact depending on who
//!   counted.

use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::kind::KIND_CODING_SESSION_OBSERVATION;

// The bounded fold lives in a sibling file so no file here passes 1,000 lines.
#[path = "coding_session_observation_fold.rs"]
mod fold;
pub use fold::*;

/// Exact v1 schema identifier, carried in content and in the `csob-v` tag.
pub const CODING_SESSION_OBSERVATION_SCHEMA: &str = "buzz-coding-session-observation/v1";

/// Maximum UTF-8 byte length of a complete signed observation payload.
///
/// Sized to the worst legal record rather than guessed: thirty-two gate rows of
/// a 64-byte name, a 512-byte command and a 2 KiB summary come to roughly
/// 84 KiB, so the ceiling is set above that and below the 128 KiB kind 44244
/// allows for briefs.
pub const MAX_CODING_SESSION_OBSERVATION_CONTENT_BYTES: usize = 96 * 1024;

/// Maximum UTF-8 byte length of a recorded command line.
pub const MAX_OBSERVATION_COMMAND_BYTES: usize = 512;
/// Maximum UTF-8 byte length of a one-line summary.
pub const MAX_OBSERVATION_SUMMARY_BYTES: usize = 2 * 1024;
/// Maximum UTF-8 byte length of free prose — a checkpoint note or a finding's
/// detail.
pub const MAX_OBSERVATION_PROSE_BYTES: usize = 8 * 1024;
/// Maximum UTF-8 byte length of a gate name, a finding id, or a phase name.
pub const MAX_OBSERVATION_NAME_BYTES: usize = 64;
/// Maximum UTF-8 byte length of a finding title.
pub const MAX_OBSERVATION_TITLE_BYTES: usize = 512;
/// Maximum number of gate rows one observation may carry.
pub const MAX_OBSERVATION_GATE_ROWS: usize = 32;
/// Maximum number of pointers one finding may carry.
pub const MAX_OBSERVATION_FINDING_REFS: usize = 16;

/// The closed observation vocabulary for schema v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionObservationType {
    /// Where the author is in its own loop, and what it has written so far.
    Checkpoint,
    /// What one or more named gates said, and what command said it.
    Gate,
    /// One finding and what the author did about it.
    Finding,
    /// One phase's own measured wall-clock span.
    Phase,
}

impl CodingSessionObservationType {
    /// The exact wire token used by content and by the `csob-type` tag.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Checkpoint => "checkpoint",
            Self::Gate => "gate",
            Self::Finding => "finding",
            Self::Phase => "phase",
        }
    }
}

/// How an observation came to exist.
///
/// The distinction Brian's 2026-09-02 ruling turns on: an agent's account of
/// its own work is a claim, and a record produced without the subject's
/// cooperation is evidence. Live runs 2 and 3 produced both halves of the
/// proof — a seat's prose "cargo test -p buzz-cli green" against a verifier's
/// reproduced red on the same patch (LIVE-RUN finding 26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionObservationSource {
    /// Written by a mechanism watching the subject — the session provider
    /// reading the seat's own tool calls, or a hire host's git hook. The
    /// signer is that mechanism, never the subject.
    Observed,
    /// Written by the subject about itself. A claim, and rendered as one.
    Declared,
    /// Produced by a bench run: a mechanical scorer executing a fixed task set
    /// and recording what it got.
    ///
    /// Widened here (2026-09-02) for the registry bench. It is honestly a
    /// *second axis* squeezed into one key — `observed`/`declared` says who saw
    /// it, `measured` says why it ran — and that is a named residual, not a
    /// design. A reader that only cares whether the subject vouched for itself
    /// should treat `measured` the way it treats `observed`: nobody's word for
    /// their own work.
    Measured,
}

impl Default for CodingSessionObservationSource {
    /// `Declared` — what a record that does not say must be read as.
    ///
    /// Only the exact word `observed`, written by a mechanism, buys the
    /// stronger claim; absence never does.
    fn default() -> Self {
        Self::Declared
    }
}

impl CodingSessionObservationSource {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Declared => "declared",
            Self::Measured => "measured",
        }
    }
}

/// Where a seat is in the red-first loop its brief names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionObservationPhase {
    /// Reading the spec and deciding what to build.
    Planning,
    /// Writing the tests that must fail first.
    Red,
    /// Making them pass.
    Green,
    /// Running the gates the brief names.
    Gates,
    /// Writing the report.
    Reporting,
}

/// What one gate said.
///
/// Deliberately the same three words a 44244 `report.tests[].outcome` uses —
/// one word per outcome across the whole wire, so a reader never has to learn
/// two vocabularies for the same fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionObservationGateOutcome {
    /// The gate ran and passed.
    Passed,
    /// The gate ran and failed.
    Failed,
    /// The gate was not run.
    NotRun,
}

/// What the author did about one finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionObservationDisposition {
    /// Found, and nothing has been decided about it yet.
    Found,
    /// Fixed in this lane.
    Fixed,
    /// Belongs to somebody else's files; handed over rather than reached into.
    CrossLane,
    /// A person has to rule before it can move.
    NeedsRuling,
    /// Deliberately not fixed, and the record says so.
    WontFix,
}

/// Checkpoint body — where the author is and what it has written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionObservationCheckpoint {
    /// The loop phase the author is in.
    pub phase: CodingSessionObservationPhase,
    /// Tests written so far in this lane.
    pub tests_written: u32,
    /// Of those, how many the author has observed failing first.
    pub tests_red: u32,
    /// Of those, how many now pass.
    pub tests_green: u32,
    /// The last command run; the key is still present as JSON null.
    pub last_command: Option<String>,
    /// A one-line summary of the last result; present as JSON null when unset.
    pub last_summary: Option<String>,
    /// Free prose; present as JSON null when unset.
    pub note: Option<String>,
}

/// One gate row: a named gate, what it said, and the command that said it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionObservationGateRow {
    /// The gate's name, as the brief names it.
    pub gate: String,
    /// What it said.
    pub outcome: CodingSessionObservationGateOutcome,
    /// The exact command that produced the outcome.
    pub command: String,
    /// The command's own summary line; present as JSON null when unset.
    pub summary: Option<String>,
    /// The author's own measurement of how long the gate took, in
    /// milliseconds. Disclosed as the author's claim and never used for
    /// ordering, discovery or dedupe.
    pub duration_ms: Option<u64>,
}

/// Gate body — one to [`MAX_OBSERVATION_GATE_ROWS`] rows, unique by gate name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionObservationGate {
    /// The rows, in the author's own order.
    pub rows: Vec<CodingSessionObservationGateRow>,
}

/// Finding body — one finding and its disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionObservationFinding {
    /// The author's own stable id for this finding, unique within its author.
    pub finding_id: String,
    /// One line naming the finding.
    pub title: String,
    /// What the author did about it.
    pub disposition: CodingSessionObservationDisposition,
    /// Free prose; present as JSON null when unset.
    pub detail: Option<String>,
    /// Event ids a reader can follow. Pointers, never causal references.
    pub refs: Vec<String>,
    /// The `decision.request` this finding is waiting on, when it is waiting on
    /// one; present as JSON null otherwise.
    pub decision_ref: Option<String>,
}

/// Phase body — one phase's own measured span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionObservationPhaseTiming {
    /// The phase's name, in the author's own words.
    pub phase: String,
    /// When the author says it started, in milliseconds since the Unix epoch.
    ///
    /// The author's own measurement. It is displayed as such and never used
    /// for ordering, discovery or dedupe.
    pub started_at_ms: u64,
    /// When the author says it ended; present as JSON null while it runs.
    pub ended_at_ms: Option<u64>,
    /// The author's own measured duration; present as JSON null when unset.
    pub duration_ms: Option<u64>,
}

/// Type-specific observation body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CodingSessionObservationBody {
    /// Checkpoint body.
    Checkpoint(CodingSessionObservationCheckpoint),
    /// Gate body.
    Gate(CodingSessionObservationGate),
    /// Finding body.
    Finding(CodingSessionObservationFinding),
    /// Phase body.
    Phase(CodingSessionObservationPhaseTiming),
}

impl CodingSessionObservationBody {
    /// The observation type this body variant implies.
    pub const fn observation_type(&self) -> CodingSessionObservationType {
        match self {
            Self::Checkpoint(_) => CodingSessionObservationType::Checkpoint,
            Self::Gate(_) => CodingSessionObservationType::Gate,
            Self::Finding(_) => CodingSessionObservationType::Finding,
            Self::Phase(_) => CodingSessionObservationType::Phase,
        }
    }
}

/// Strict public JSON carried by a kind 44246 event.
///
/// Exactly seven top-level keys, every one always present. (Six until
/// 2026-09-02, when `source` was added.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionObservationPayload {
    /// Exact schema identifier.
    pub schema: String,
    /// Canonical lowercase UUID of the umbrella session.
    pub session_ref: String,
    /// Lowercase 64-hex event id of the session genesis.
    pub genesis_ref: String,
    /// Closed observation token, repeated in `csob-type`.
    #[serde(rename = "type")]
    pub observation_type: CodingSessionObservationType,
    /// How this record was produced: watched, or claimed by its subject.
    ///
    /// Not carried in a tag. The five-tag envelope is NIP-CSOB's frozen shape
    /// and a sixth tag would refuse every event a build that predates this
    /// field signed; a reader that wants only observed rows filters the folded
    /// collection rather than the relay query.
    ///
    /// **Required on write, optional on read** — the same rule kind 44244's
    /// `condition` is under, and for the same measured reason (finding 28).
    /// Serialization always emits the key, so every record this repository
    /// writes carries it; `serde(default)` plus the read-side exemption below
    /// means a six-key body signed before 2026-09-02 still decodes, reading as
    /// `declared`. That is the honest default: a row nothing watched is a
    /// claim. An explicit `null` is still refused — a key that says nothing is
    /// not the same as a key that was never written. Without this every 44246
    /// event on the wire today becomes unreadable, which is a reader losing
    /// history.
    #[serde(default)]
    pub source: CodingSessionObservationSource,
    /// The assignment this observation is about, when it is about one.
    ///
    /// A **pointer**, never a causal reference: an observation that names an
    /// assignment nobody supplied is still a real statement its author made,
    /// so a dangling id is disclosed as
    /// [`CodingSessionObservationFold::unresolved`] and excludes nothing. An
    /// observation cannot deny anything.
    pub assignment_ref: Option<String>,
    /// Type-specific structured body.
    pub body: CodingSessionObservationBody,
}

impl CodingSessionObservationPayload {
    /// Validate field bounds, type/body parity, and reference syntax.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_OBSERVATION_SCHEMA {
            return Err("unsupported coding-session observation schema".into());
        }
        validate_canonical_uuid("sessionRef", &self.session_ref)?;
        validate_event_id("genesisRef", &self.genesis_ref)?;
        if self.observation_type != self.body.observation_type() {
            return Err("coding-session observation type does not match body shape".into());
        }
        if let Some(reference) = &self.assignment_ref {
            validate_event_id("assignmentRef", reference)?;
        }
        match &self.body {
            CodingSessionObservationBody::Checkpoint(body) => body.validate(),
            CodingSessionObservationBody::Gate(body) => body.validate(),
            CodingSessionObservationBody::Finding(body) => body.validate(),
            CodingSessionObservationBody::Phase(body) => body.validate(),
        }
    }
}

impl CodingSessionObservationCheckpoint {
    fn validate(&self) -> Result<(), String> {
        if self.tests_red > self.tests_written {
            return Err("checkpoint testsRed must not exceed testsWritten".into());
        }
        if self.tests_green > self.tests_written {
            return Err("checkpoint testsGreen must not exceed testsWritten".into());
        }
        validate_optional_text(
            "checkpoint lastCommand",
            self.last_command.as_deref(),
            MAX_OBSERVATION_COMMAND_BYTES,
        )?;
        validate_optional_text(
            "checkpoint lastSummary",
            self.last_summary.as_deref(),
            MAX_OBSERVATION_SUMMARY_BYTES,
        )?;
        validate_optional_prose(
            "checkpoint note",
            self.note.as_deref(),
            MAX_OBSERVATION_PROSE_BYTES,
        )
    }
}

impl CodingSessionObservationGate {
    fn validate(&self) -> Result<(), String> {
        if self.rows.is_empty() {
            return Err(
                "gate rows must not be empty: publish a gate observation only when there is a \
                 gate to report"
                    .into(),
            );
        }
        if self.rows.len() > MAX_OBSERVATION_GATE_ROWS {
            return Err(format!(
                "gate rows exceed {MAX_OBSERVATION_GATE_ROWS} entries (got {})",
                self.rows.len()
            ));
        }
        for (index, row) in self.rows.iter().enumerate() {
            validate_text("gate row gate", &row.gate, MAX_OBSERVATION_NAME_BYTES)?;
            validate_text(
                "gate row command",
                &row.command,
                MAX_OBSERVATION_COMMAND_BYTES,
            )?;
            validate_optional_text(
                "gate row summary",
                row.summary.as_deref(),
                MAX_OBSERVATION_SUMMARY_BYTES,
            )?;
            if self.rows[..index].iter().any(|held| held.gate == row.gate) {
                return Err(format!(
                    "gate row names {:?} twice: one observation states each gate once, and a \
                     later observation is the newer statement",
                    row.gate
                ));
            }
        }
        Ok(())
    }
}

impl CodingSessionObservationFinding {
    fn validate(&self) -> Result<(), String> {
        validate_text(
            "finding findingId",
            &self.finding_id,
            MAX_OBSERVATION_NAME_BYTES,
        )?;
        validate_text("finding title", &self.title, MAX_OBSERVATION_TITLE_BYTES)?;
        validate_optional_prose(
            "finding detail",
            self.detail.as_deref(),
            MAX_OBSERVATION_PROSE_BYTES,
        )?;
        if self.refs.len() > MAX_OBSERVATION_FINDING_REFS {
            return Err(format!(
                "finding refs exceed {MAX_OBSERVATION_FINDING_REFS} entries (got {})",
                self.refs.len()
            ));
        }
        for (index, reference) in self.refs.iter().enumerate() {
            validate_event_id("finding refs entry", reference)?;
            if self.refs[..index].contains(reference) {
                return Err("finding refs must not contain duplicates".into());
            }
        }
        if let Some(reference) = &self.decision_ref {
            validate_event_id("finding decisionRef", reference)?;
        }
        Ok(())
    }
}

impl CodingSessionObservationPhaseTiming {
    fn validate(&self) -> Result<(), String> {
        validate_text("phase phase", &self.phase, MAX_OBSERVATION_NAME_BYTES)?;
        if let Some(ended) = self.ended_at_ms {
            if ended < self.started_at_ms {
                return Err("phase endedAtMs must not precede startedAtMs".into());
            }
        }
        Ok(())
    }
}

/// The exact seven top-level keys of an observation payload.
const OBSERVATION_KEYS: &[&str] = &[
    "schema",
    "sessionRef",
    "genesisRef",
    "type",
    "source",
    "assignmentRef",
    "body",
];

/// The one top-level key an observation may write as JSON `null`.
const OBSERVATION_NULLABLE_KEYS: &[&str] = &["assignmentRef"];

/// Top-level keys a **reader** accepts as absent, though a writer always emits
/// them.
///
/// `source` joined the payload on 2026-09-02. Requiring it on read would make
/// every 44246 event signed before that day undecodable — the exact failure
/// kind 44244's `condition` was measured into and ruled on (finding 28): a wire
/// widening never loses history. Absent reads as `declared`; an explicit `null`
/// is still refused by `reject_required_nulls`.
const OBSERVATION_READ_OPTIONAL_KEYS: &[&str] = &["source"];

/// The exact body keys, and which of them may be `null`, per observation type.
const fn body_keys(
    observation_type: CodingSessionObservationType,
) -> (&'static [&'static str], &'static [&'static str]) {
    match observation_type {
        CodingSessionObservationType::Checkpoint => (
            &[
                "phase",
                "testsWritten",
                "testsRed",
                "testsGreen",
                "lastCommand",
                "lastSummary",
                "note",
            ],
            &["lastCommand", "lastSummary", "note"],
        ),
        CodingSessionObservationType::Gate => (&["rows"], &[]),
        CodingSessionObservationType::Finding => (
            &[
                "findingId",
                "title",
                "disposition",
                "detail",
                "refs",
                "decisionRef",
            ],
            &["detail", "decisionRef"],
        ),
        CodingSessionObservationType::Phase => (
            &["phase", "startedAtMs", "endedAtMs", "durationMs"],
            &["endedAtMs", "durationMs"],
        ),
    }
}

/// The closed checkpoint phase tokens, as they appear on the wire.
const CHECKPOINT_PHASES: &[&str] = &["planning", "red", "green", "gates", "reporting"];
/// The closed gate outcome tokens. The same three words a 44244
/// `report.tests[].outcome` uses — one word per outcome across the wire.
const GATE_OUTCOMES: &[&str] = &["passed", "failed", "not-run"];
/// The closed finding disposition tokens.
const FINDING_DISPOSITIONS: &[&str] = &["found", "fixed", "cross-lane", "needs-ruling", "wont-fix"];
/// The closed provenance tokens.
const OBSERVATION_SOURCES: &[&str] = &["observed", "declared", "measured"];

/// The exact keys of one gate row, and which of them may be `null`.
const GATE_ROW_KEYS: &[&str] = &["gate", "outcome", "command", "summary", "durationMs"];
/// The gate-row keys an author may write as JSON `null`.
const GATE_ROW_NULLABLE_KEYS: &[&str] = &["summary", "durationMs"];

/// Strictly decode and validate signed kind 44246 content.
///
/// Rejects unknown keys rather than ignoring them, refuses a missing key and an
/// explicit `null` in a required position by name, and never accepts a token
/// outside the closed vocabularies.
pub fn decode_coding_session_observation(
    content: &str,
) -> Result<CodingSessionObservationPayload, String> {
    if content.len() > MAX_CODING_SESSION_OBSERVATION_CONTENT_BYTES {
        return Err(format!(
            "coding-session observation content exceeds \
             {MAX_CODING_SESSION_OBSERVATION_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session observation payload: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "coding-session observation payload must be an object".to_owned())?;
    validate_exact_keys(
        object,
        OBSERVATION_KEYS,
        OBSERVATION_READ_OPTIONAL_KEYS,
        "observation payload",
    )?;
    reject_required_nulls(
        object,
        OBSERVATION_KEYS,
        OBSERVATION_NULLABLE_KEYS,
        "observation payload",
    )?;
    if object.contains_key("source") {
        validate_closed_token(object, "source", OBSERVATION_SOURCES, "observation payload")?;
    }

    let observation_type: CodingSessionObservationType = serde_json::from_value(
        object
            .get("type")
            .cloned()
            .ok_or_else(|| "coding-session observation type is missing".to_owned())?,
    )
    .map_err(|_| {
        format!(
            "unsupported coding-session observation type: v1 knows exactly {}",
            joined_tokens(&["checkpoint", "gate", "finding", "phase"])
        )
    })?;

    let body = object
        .get("body")
        .and_then(Value::as_object)
        .ok_or_else(|| "coding-session observation body must be an object".to_owned())?;
    let (keys, nullable) = body_keys(observation_type);
    let field = format!("observation {} body", observation_type.as_str());
    validate_exact_keys(body, keys, &[], &field)?;
    reject_required_nulls(body, keys, nullable, &field)?;
    // Closed sub-vocabularies are checked here rather than left to serde: an
    // untagged body enum answers an unknown token with "data did not match any
    // variant", which names neither the field nor the set, and a refusal a
    // seat cannot act on is barely a refusal.
    match observation_type {
        CodingSessionObservationType::Checkpoint => {
            validate_closed_token(body, "phase", CHECKPOINT_PHASES, &field)?;
        }
        CodingSessionObservationType::Finding => {
            validate_closed_token(body, "disposition", FINDING_DISPOSITIONS, &field)?;
        }
        CodingSessionObservationType::Phase => {}
        CodingSessionObservationType::Gate => {
            let rows = body
                .get("rows")
                .and_then(Value::as_array)
                .ok_or_else(|| "gate rows must be an array".to_owned())?;
            for row in rows {
                let row = row
                    .as_object()
                    .ok_or_else(|| "gate row must be an object".to_owned())?;
                validate_exact_keys(row, GATE_ROW_KEYS, &[], "gate row")?;
                reject_required_nulls(row, GATE_ROW_KEYS, GATE_ROW_NULLABLE_KEYS, "gate row")?;
                validate_closed_token(row, "outcome", GATE_OUTCOMES, "gate row")?;
            }
        }
    }

    // A second strict decode preserves serde's duplicate-field detection, which
    // the `Value` map above cannot represent — the same reason kind 44244
    // decodes twice.
    let payload: CodingSessionObservationPayload = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session observation payload: {error}"))?;
    payload.validate()?;
    Ok(payload)
}

/// Validate the exact ordered event envelope and return its decoded payload.
///
/// Five two-field tags, in order: `h` (channel UUID), `d` (the umbrella),
/// `csob-v` (the schema), `csob-genesis` (the founding event), `csob-type` (the
/// observation token). `d`, `csob-genesis` and `csob-type` must agree with the
/// content, so an observation cannot be filed under one umbrella while claiming
/// another, or be indexed as one type while carrying another's body.
pub fn validate_coding_session_observation_envelope(
    event: &Event,
) -> Result<CodingSessionObservationPayload, String> {
    if event.kind.as_u16() as u32 != KIND_CODING_SESSION_OBSERVATION {
        return Err("coding-session observation has the wrong event kind".into());
    }
    let payload = decode_coding_session_observation(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 5 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session observation requires exactly five two-field tags".into());
    }
    if tags[0][0] != "h" {
        return Err("coding-session observation first tag must be h=channel UUID".into());
    }
    validate_canonical_uuid("h", &tags[0][1])?;
    if tags[1][0] != "d" || tags[1][1] != payload.session_ref {
        return Err("coding-session observation d tag does not match payload sessionRef".into());
    }
    if tags[2][0] != "csob-v" || tags[2][1] != CODING_SESSION_OBSERVATION_SCHEMA {
        return Err("unsupported coding-session observation tag version".into());
    }
    if tags[3][0] != "csob-genesis" || tags[3][1] != payload.genesis_ref {
        return Err(
            "coding-session observation genesis tag does not match payload genesisRef".into(),
        );
    }
    if tags[4][0] != "csob-type" || tags[4][1] != payload.observation_type.as_str() {
        return Err("coding-session observation type tag does not match payload type".into());
    }
    Ok(payload)
}

/// Refuse a token outside a closed set, naming the field and the whole set.
fn validate_closed_token(
    object: &serde_json::Map<String, Value>,
    key: &str,
    allowed: &[&str],
    field: &str,
) -> Result<(), String> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("coding-session {field} field {key:?} must be a string"))?;
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(format!(
        "coding-session {field} field {key:?} carries unsupported token {value:?}: v1 knows \
         exactly {}, and refuses anything else rather than ignoring it",
        joined_tokens(allowed)
    ))
}

fn joined_tokens(tokens: &[&str]) -> String {
    tokens
        .iter()
        .map(|token| format!("{token:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn validate_exact_keys(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
    read_optional: &[&str],
    field: &str,
) -> Result<(), String> {
    for key in keys {
        if read_optional.contains(key) {
            continue;
        }
        if !object.contains_key(*key) {
            return Err(format!(
                "coding-session {field} is missing {key:?}: every key is always present, and an \
                 unset optional is written as JSON null rather than omitted"
            ));
        }
    }
    for key in object.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(format!(
                "coding-session {field} carries unsupported field {key:?}: v1 rejects unknown \
                 fields rather than ignoring them, because a reader that ignores a key disagrees \
                 with its peer about the same signed bytes"
            ));
        }
    }
    Ok(())
}

fn reject_required_nulls(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
    nullable: &[&str],
    field: &str,
) -> Result<(), String> {
    for key in keys {
        if nullable.contains(key) {
            continue;
        }
        if object.get(*key).is_some_and(Value::is_null) {
            return Err(format!(
                "coding-session {field} field {key:?} requires a value: null is not a value here, \
                 and absent is not null"
            ));
        }
    }
    Ok(())
}

fn validate_canonical_uuid(field: &str, value: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(value).map_err(|_| format!("{field} must be a UUID"))?;
    if parsed.to_string() != value {
        return Err(format!("{field} must be a lowercase canonical UUID"));
    }
    Ok(())
}

fn validate_event_id(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be a lowercase 64-hex event id"));
    }
    Ok(())
}

/// A single-line value: bounded, non-blank, and free of control characters.
fn validate_text(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if value.len() > max {
        return Err(format!("{field} exceeds {max} bytes (got {})", value.len()));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

fn validate_optional_text(field: &str, value: Option<&str>, max: usize) -> Result<(), String> {
    match value {
        Some(value) => validate_text(field, value, max),
        None => Ok(()),
    }
}

/// Free prose: bounded and non-blank, but newlines are the point of it.
fn validate_prose(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if value.len() > max {
        return Err(format!("{field} exceeds {max} bytes (got {})", value.len()));
    }
    if value.chars().any(|character| {
        character.is_control() && character != '\n' && character != '\r' && character != '\t'
    }) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

fn validate_optional_prose(field: &str, value: Option<&str>, max: usize) -> Result<(), String> {
    match value {
        Some(value) => validate_prose(field, value, max),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "coding_session_observation_tests.rs"]
mod tests;
