//! NIP-CSTX: signed team transactions inside a coding session (kind 44244).
//!
//! This module defines the closed v1 vocabulary and validates only facts that
//! are self-contained in one event: exact JSON and tag shape, bounds, tag to
//! content parity, and reference syntax. Whether a referenced event exists,
//! belongs to the same session, or is authorized is storage-backed policy and
//! must be checked by the relay/fold that has those events available.
//!
//! The payload deliberately has no author or actor-of-record field. The Nostr
//! event signature is the sole author identity. An assignment's
//! `assigneeActor` is a target, never an authorship claim.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;

// Decoding, envelope validation and the correction rules live in a sibling
// file for the same reason.
#[path = "coding_session_team_transaction_decode.rs"]
mod decode;
pub use decode::*;

// Field validators live in a sibling file so no file here passes 1,000 lines
// (FINAL-B §7). A child module, so it reads this module's private constants
// unchanged.
#[path = "coding_session_team_transaction_validators.rs"]
mod validators;
use validators::*;

#[path = "coding_session_team_transaction_fold.rs"]
mod fold;
pub use fold::*;

/// Exact v1 schema identifier carried in content and the `cstx-v` tag.
pub const CODING_SESSION_TEAM_TRANSACTION_SCHEMA: &str = "buzz-coding-session-team-transaction/v1";
/// Maximum UTF-8 byte length of a complete transaction payload.
pub const MAX_TEAM_TRANSACTION_CONTENT_BYTES: usize = 128 * 1024;
/// Maximum byte length of long prose such as a brief.
pub const MAX_TEAM_TRANSACTION_LONG_TEXT_BYTES: usize = 32 * 1024;
/// Maximum byte length of ordinary prose such as a summary or finding.
pub const MAX_TEAM_TRANSACTION_TEXT_BYTES: usize = 8 * 1024;
/// Maximum number of entries in a general evidence collection.
pub const MAX_TEAM_TRANSACTION_ITEMS: usize = 256;
/// Maximum number of test records in one report.
pub const MAX_TEAM_TRANSACTION_TESTS: usize = 128;
/// Maximum byte length of one file path.
pub const MAX_TEAM_TRANSACTION_PATH_BYTES: usize = 1024;
/// Maximum byte length of short prose such as a recommendation or a free-text
/// decision choice.
pub const MAX_TEAM_TRANSACTION_SHORT_TEXT_BYTES: usize = 2 * 1024;
/// Maximum number of pointers one `note` may carry.
pub const MAX_TEAM_TRANSACTION_NOTE_REFS: usize = 16;
/// Maximum number of options one `decision.request` may offer.
pub const MAX_TEAM_TRANSACTION_DECISION_OPTIONS: usize = 8;
/// Maximum byte length of one `decision.request` option.
pub const MAX_TEAM_TRANSACTION_DECISION_OPTION_BYTES: usize = 512;
/// Maximum number of assignments one `decision.request` may block.
pub const MAX_TEAM_TRANSACTION_DECISION_BLOCKS: usize = 16;
/// Maximum byte length of a `decision.answer` `condition`.
///
/// The same bound one `decision.request` option carries, because a condition
/// is written in the same breath as the options it generalises. It is a
/// sentence a person reads, not a program: see
/// [`CodingSessionTeamDecisionAnswer::condition`].
pub const MAX_TEAM_TRANSACTION_DECISION_CONDITION_BYTES: usize = 512;
/// Exact wire token naming the founder as the party holding a decision.
pub const CODING_SESSION_TEAM_DECISION_FOUNDER: &str = "founder";
/// Exact refusal for a `mission.blocked` correction that names no blocker.
pub const TERMINAL_CANNOT_CLEAR_ITSELF: &str =
    "use a note or a decision.answer to clear a blocker; a terminal cannot clear itself";
/// Exact refusal for a `mission.completed` corrected by a `mission.blocked`.
///
/// The one terminal crossing that is legal runs the other way — see
/// [`validate_coding_session_team_transaction_supersession`].
pub const TERMINAL_COMPLETION_IS_NOT_REOPENED: &str =
    "a completion may correct a blocked, never the reverse: publish a new mission.blocked, \
     or a note, rather than correcting a completion";
/// Exact refusal for a `mission.blocked` correction that leaves the blocker set
/// untouched — a prose-only edit of a terminal, which now has its own verb.
pub const TERMINAL_PROSE_EDIT_NEEDS_A_NOTE: &str =
    "a mission.blocked correction must change its blockers; use a note to add context";
/// Exact maximum inherited from kind 44220 `commandId`.
pub const MAX_TEAM_TRANSACTION_DELIVERY_COMMAND_ID_BYTES: usize =
    crate::coding_session_command::MAX_IDENTIFIER_BYTES;

/// The closed operation vocabulary for schema v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodingSessionTeamTransactionType {
    /// Assigns bounded work to one actor and role.
    #[serde(rename = "assignment")]
    Assignment,
    /// Reports evidence against one assignment.
    #[serde(rename = "report")]
    Report,
    /// Rules on one report for one assignment.
    #[serde(rename = "verdict")]
    Verdict,
    /// Records receipt of another transaction.
    #[serde(rename = "acknowledgement")]
    Acknowledgement,
    /// Settles the mission as completed.
    #[serde(rename = "mission.completed")]
    MissionCompleted,
    /// Settles the mission as blocked.
    #[serde(rename = "mission.blocked")]
    MissionBlocked,
    /// Says something without changing any fold state.
    #[serde(rename = "note")]
    Note,
    /// Asks one named party for a ruling the mission needs.
    #[serde(rename = "decision.request")]
    DecisionRequest,
    /// Answers one `decision.request` with the standing to do so.
    #[serde(rename = "decision.answer")]
    DecisionAnswer,
}

impl CodingSessionTeamTransactionType {
    /// Return the exact wire token used by content and `cstx-type`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assignment => "assignment",
            Self::Report => "report",
            Self::Verdict => "verdict",
            Self::Acknowledgement => "acknowledgement",
            Self::MissionCompleted => "mission.completed",
            Self::MissionBlocked => "mission.blocked",
            Self::Note => "note",
            Self::DecisionRequest => "decision.request",
            Self::DecisionAnswer => "decision.answer",
        }
    }
}

/// One structured test result in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamTransactionTest {
    /// Human-readable test name.
    pub name: String,
    /// Exact command or action that produced the outcome.
    pub command: String,
    /// Closed outcome vocabulary.
    pub outcome: CodingSessionTeamTransactionTestOutcome,
    /// Bounded evidence such as an exit code and count; nullable but present.
    pub evidence: Option<String>,
}

/// Closed outcome vocabulary for structured report tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionTeamTransactionTestOutcome {
    /// The test ran and passed.
    Passed,
    /// The test ran and failed.
    Failed,
    /// The test was intentionally not run.
    NotRun,
}

/// Assignment-specific content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamAssignment {
    /// Lowercase 64-hex pubkey of the actor receiving the assignment.
    pub assignee_actor: String,
    /// Bounded role token under which the actor is assigned.
    pub assignee_role: String,
    /// Concise outcome this assignment owns.
    pub objective: String,
    /// Complete bounded brief.
    pub brief: String,
    /// Optional topic branch; the key is still present as JSON null.
    pub branch: Option<String>,
    /// Optional 40- or 64-hex git base object id.
    pub base_sha: Option<String>,
    /// Exclusive paths or path prefixes owned by this assignment.
    pub file_ownership: Vec<String>,
    /// Ordered acceptance commands or observable checks.
    pub acceptance_steps: Vec<String>,
}

/// Report-specific content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamReport {
    /// Event id of the assignment this report answers.
    pub assignment_ref: String,
    /// Concise result summary.
    pub summary: String,
    /// Optional topic branch; the key is still present as JSON null.
    pub branch: Option<String>,
    /// Optional 40- or 64-hex base object id.
    pub base_sha: Option<String>,
    /// Optional 40- or 64-hex reported head object id.
    pub head_sha: Option<String>,
    /// Changed files claimed by the report.
    pub files: Vec<String>,
    /// Structured test evidence. Prose elsewhere never populates this list.
    pub tests: Vec<CodingSessionTeamTransactionTest>,
    /// Whether the author observed a failing test before the green result.
    pub red_before_green: Option<bool>,
    /// Disclosed departures from the assignment.
    pub deviations: Vec<String>,
    /// Known remaining work or risk.
    pub residuals: Vec<String>,
    /// Surprising observations worth preserving.
    pub anomalies: Vec<String>,
}

/// Closed verdict subtypes for schema v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionTeamVerdictSubtype {
    /// An active verifier attempts to refute one report.
    Refutation,
    /// The founder or an active lead governs one report.
    Disposition,
}

/// Closed verifier decisions for a `refutation` verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionTeamRefutationDecision {
    /// A verifier found the named failure.
    Confirmed,
    /// A verifier did not find a refutation.
    NotRefuted,
    /// Missing input prevents a verifier conclusion.
    Blocked,
}

/// Closed lead decisions for a `disposition` verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionTeamDispositionDecision {
    /// Accepts the governed report and assignment.
    Approve,
    /// Accepts with non-blocking notes.
    ApproveWithNotes,
    /// Requires another report before acceptance.
    ChangesRequested,
    /// Rejects the governed report.
    Reject,
    /// Missing input prevents a disposition.
    Blocked,
}

impl CodingSessionTeamDispositionDecision {
    /// Whether this decision can settle an assignment after acknowledgement.
    pub const fn is_approval(self) -> bool {
        matches!(self, Self::Approve | Self::ApproveWithNotes)
    }
}

/// Verdict-specific content with a subtype-specific closed decision space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum CodingSessionTeamVerdict {
    /// An active verifier's attempt to refute a report.
    #[serde(rename = "refutation")]
    Refutation {
        /// Event id of the assignment under review.
        assignment_ref: String,
        /// Event id of the report under review.
        report_ref: String,
        /// Closed verifier conclusion.
        decision: CodingSessionTeamRefutationDecision,
        /// Concise conclusion summary.
        summary: String,
        /// Structured findings supporting the conclusion.
        findings: Vec<String>,
        /// Required next action, or null when none remains.
        required_action: Option<String>,
    },
    /// A founder/lead ruling that governs one report.
    #[serde(rename = "disposition")]
    Disposition {
        /// Event id of the assignment under review.
        assignment_ref: String,
        /// Event id of the report governed by this disposition.
        report_ref: String,
        /// Optional prior refutation of this exact assignment/report pair.
        refutation_ref: Option<String>,
        /// Closed founder/lead decision.
        decision: CodingSessionTeamDispositionDecision,
        /// Concise ruling summary.
        summary: String,
        /// Structured findings supporting the ruling.
        findings: Vec<String>,
        /// Required next action, or null when none remains.
        required_action: Option<String>,
    },
}

/// The only acknowledgement status in schema v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodingSessionTeamAcknowledgementStatus {
    /// The signed author received the referenced record.
    Received,
}

/// Acknowledgement-specific content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamAcknowledgement {
    /// Event id whose receipt is being acknowledged.
    pub acknowledged_event_ref: String,
    /// Exactly `received` in schema v1.
    pub status: CodingSessionTeamAcknowledgementStatus,
    /// Optional bounded note; the key is still present as JSON null.
    pub note: Option<String>,
}

/// Completed-mission-specific content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamMissionCompleted {
    /// Assignment event ids included in the disposition.
    pub assignment_refs: Vec<String>,
    /// Landed 40- or 64-hex git object ids; may be empty for non-code work.
    pub landed_shas: Vec<String>,
    /// Concise terminal summary.
    pub summary: String,
    /// Explicit non-blocking follow-up work.
    pub follow_ups: Vec<String>,
}

/// Blocked-mission-specific content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamMissionBlocked {
    /// Assignment event ids affected by the blocker; may be empty pre-dispatch.
    pub assignment_refs: Vec<String>,
    /// Concise terminal summary.
    pub summary: String,
    /// One or more concrete blockers.
    pub blockers: Vec<String>,
    /// Actor, role, or system currently holding progress, or null if unknown.
    pub held_on: Option<String>,
    /// The one bounded action that can move the mission again.
    pub required_action: String,
}

/// Note-specific content.
///
/// A note is the vocabulary's only way to say something without changing
/// state. It is never a phase, never a terminal, never supersedes another
/// record and can never be superseded, so `refs` are pointers for a reader and
/// are deliberately **not** causal references: a note whose pointer names an
/// event outside the supplied set is still a canonical note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamNote {
    /// The complete bounded text of the note.
    pub text: String,
    /// Bounded event ids this note points at; never causal, never required to
    /// resolve.
    pub refs: Vec<String>,
}

/// Decision-request-specific content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamDecisionRequest {
    /// The exact question the mission needs answered.
    pub question: String,
    /// Bounded closed options; may be empty for an open question.
    pub options: Vec<String>,
    /// Exactly `founder`, or the lowercase 64-hex actor holding the decision.
    pub held_on: String,
    /// Assignment event ids this unanswered question blocks; may be empty.
    pub blocks: Vec<String>,
    /// Optional bounded recommendation; the key is still present as JSON null.
    pub recommendation: Option<String>,
}

impl CodingSessionTeamDecisionRequest {
    /// Whether the founder, rather than a named actor, holds this decision.
    pub fn is_held_on_founder(&self) -> bool {
        self.held_on == CODING_SESSION_TEAM_DECISION_FOUNDER
    }
}

/// The two exact shapes a `decision.answer` choice may take.
///
/// An index selects one of the request's declared options; free text answers a
/// question whose options did not contain the answer. The variant order is
/// load-bearing for the untagged decode: a JSON number can only be an index and
/// a JSON string can only be text, so the two never collide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CodingSessionTeamDecisionChoice {
    /// Zero-based index into the request's `options`.
    Index(u32),
    /// Bounded free-text answer.
    Text(String),
}

/// Decision-answer-specific content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamDecisionAnswer {
    /// Event id of the `decision.request` being answered.
    pub request_ref: String,
    /// Chosen option index or bounded free text.
    pub choice: CodingSessionTeamDecisionChoice,
    /// Optional bounded reasoning; the key is still present as JSON null.
    pub note: Option<String>,
    /// Optional bounded statement of the **class** this ruling covers; the key
    /// is still present as JSON null.
    ///
    /// **Text, not a predicate.** Nothing evaluates it, the fold neither reads
    /// nor enforces it, and no surface may parse it into state. It exists so a
    /// ruling can say what it covers in one place a reader and a seat both
    /// find, instead of the same question being asked once per commit.
    ///
    /// Live run 2, 11:33 (finding 21): a builder asked the founder the same
    /// question twice because the first answer had been given about one SHA
    /// and a second SHA needed the identical ruling. The honest fix is not a
    /// machine-checked condition — the fold cannot evaluate "any SHA whose
    /// buzz-acp diff against main is empty" and must never pretend to — but a
    /// place to write it down where the next asker reads it first.
    ///
    /// **Required on write, optional on read.** Serialization always emits the
    /// key (`null` when unset), so every record this repository writes carries
    /// it; `serde(default)` means a body signed before 2026-09-02 still
    /// decodes, with an absent key reading as `None` exactly as `null` does.
    /// The alternative — an exact seven-key row — was implemented first and
    /// measured: it made live run 2's three signed answers undecodable and
    /// erased that session's terminal from the fold. A reader must never lose
    /// history.
    #[serde(default)]
    pub condition: Option<String>,
}

/// Operation-specific transaction body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CodingSessionTeamTransactionBody {
    /// Assignment body.
    Assignment(CodingSessionTeamAssignment),
    /// Report body.
    Report(CodingSessionTeamReport),
    /// Verdict body.
    Verdict(CodingSessionTeamVerdict),
    /// Acknowledgement body.
    Acknowledgement(CodingSessionTeamAcknowledgement),
    /// Completed mission body.
    MissionCompleted(CodingSessionTeamMissionCompleted),
    /// Blocked mission body.
    MissionBlocked(CodingSessionTeamMissionBlocked),
    /// Note body.
    Note(CodingSessionTeamNote),
    /// Decision-request body.
    DecisionRequest(CodingSessionTeamDecisionRequest),
    /// Decision-answer body.
    DecisionAnswer(CodingSessionTeamDecisionAnswer),
}

impl CodingSessionTeamTransactionBody {
    /// Return the operation type implied by this body variant.
    pub const fn transaction_type(&self) -> CodingSessionTeamTransactionType {
        match self {
            Self::Assignment(_) => CodingSessionTeamTransactionType::Assignment,
            Self::Report(_) => CodingSessionTeamTransactionType::Report,
            Self::Verdict(_) => CodingSessionTeamTransactionType::Verdict,
            Self::Acknowledgement(_) => CodingSessionTeamTransactionType::Acknowledgement,
            Self::MissionCompleted(_) => CodingSessionTeamTransactionType::MissionCompleted,
            Self::MissionBlocked(_) => CodingSessionTeamTransactionType::MissionBlocked,
            Self::Note(_) => CodingSessionTeamTransactionType::Note,
            Self::DecisionRequest(_) => CodingSessionTeamTransactionType::DecisionRequest,
            Self::DecisionAnswer(_) => CodingSessionTeamTransactionType::DecisionAnswer,
        }
    }
}

/// Strict public JSON carried by a kind 44244 event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTeamTransactionPayload {
    /// Exact schema identifier.
    pub schema: String,
    /// Canonical lowercase UUID of the umbrella session.
    pub session_ref: String,
    /// Lowercase 64-hex event id of the session genesis.
    pub genesis_ref: String,
    /// Closed operation token, repeated in `cstx-type`.
    #[serde(rename = "type")]
    pub transaction_type: CodingSessionTeamTransactionType,
    /// Optional corrected transaction event id. This is not a causal link.
    pub supersedes: Option<String>,
    /// Optional kind 44220 `commandId` whose wake/delivery this record correlates.
    pub delivery_command_id: Option<String>,
    /// Operation-specific structured body.
    pub body: CodingSessionTeamTransactionBody,
}

impl CodingSessionTeamTransactionPayload {
    /// Validate field bounds, type/body parity, and reference syntax.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_TEAM_TRANSACTION_SCHEMA {
            return Err("unsupported coding-session team-transaction schema".into());
        }
        validate_canonical_uuid("sessionRef", &self.session_ref)?;
        validate_event_id("genesisRef", &self.genesis_ref)?;
        if self.transaction_type != self.body.transaction_type() {
            return Err("team-transaction type does not match body shape".into());
        }
        if let Some(reference) = &self.supersedes {
            validate_event_id("supersedes", reference)?;
            // A note is not a phase and cannot correct one: it never changes
            // fold state, so there is no state for a correction to move.
            if self.transaction_type == CodingSessionTeamTransactionType::Note {
                return Err("a note never supersedes another record".into());
            }
            // A correction preserves its operation type, so a `mission.blocked`
            // carrying `supersedes` is by construction correcting another
            // `mission.blocked`. Naming zero blockers there is the shape the
            // lead reached for when it wanted to say "nothing is blocked any
            // more" — and a terminal saying that about itself is the honesty
            // bug the two new verbs exist to fix.
            if let CodingSessionTeamTransactionBody::MissionBlocked(body) = &self.body {
                if body.blockers.is_empty() {
                    return Err(TERMINAL_CANNOT_CLEAR_ITSELF.into());
                }
            }
        }
        if let Some(command_id) = &self.delivery_command_id {
            validate_delivery_command_id(command_id)?;
        }
        self.body.validate()
    }

    /// Return all explicit causal event references in the operation body.
    ///
    /// `supersedes` is deliberately excluded: it links a same-operation
    /// correction, not a workflow predecessor.
    pub fn causal_references(&self) -> Vec<&str> {
        match &self.body {
            CodingSessionTeamTransactionBody::Assignment(_) => Vec::new(),
            CodingSessionTeamTransactionBody::Report(body) => vec![&body.assignment_ref],
            CodingSessionTeamTransactionBody::Verdict(body) => body.causal_references(),
            CodingSessionTeamTransactionBody::Acknowledgement(body) => {
                vec![&body.acknowledged_event_ref]
            }
            CodingSessionTeamTransactionBody::MissionCompleted(body) => {
                body.assignment_refs.iter().map(String::as_str).collect()
            }
            CodingSessionTeamTransactionBody::MissionBlocked(body) => {
                body.assignment_refs.iter().map(String::as_str).collect()
            }
            // A note's `refs` are pointers a reader follows, not workflow
            // predecessors: a note must stay canonical whether or not the
            // events it mentions were supplied.
            CodingSessionTeamTransactionBody::Note(_) => Vec::new(),
            // `blocks` are pointers, not causal references (REVIEW-B1c F3).
            // A question about a piece of work must outlive a correction to
            // that work: the fold re-resolves `blocks` against the current
            // chain head every time rather than binding the request to one
            // superseded event id.
            CodingSessionTeamTransactionBody::DecisionRequest(_) => Vec::new(),
            CodingSessionTeamTransactionBody::DecisionAnswer(body) => vec![&body.request_ref],
        }
    }
}

impl CodingSessionTeamTransactionBody {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Assignment(body) => body.validate(),
            Self::Report(body) => body.validate(),
            Self::Verdict(body) => body.validate(),
            Self::Acknowledgement(body) => body.validate(),
            Self::MissionCompleted(body) => body.validate(),
            Self::MissionBlocked(body) => body.validate(),
            Self::Note(body) => body.validate(),
            Self::DecisionRequest(body) => body.validate(),
            Self::DecisionAnswer(body) => body.validate(),
        }
    }
}

impl CodingSessionTeamNote {
    fn validate(&self) -> Result<(), String> {
        validate_text("text", &self.text, MAX_TEAM_TRANSACTION_TEXT_BYTES)?;
        validate_bounded_event_ids("refs", &self.refs, MAX_TEAM_TRANSACTION_NOTE_REFS)
    }
}

impl CodingSessionTeamDecisionRequest {
    fn validate(&self) -> Result<(), String> {
        validate_text("question", &self.question, MAX_TEAM_TRANSACTION_TEXT_BYTES)?;
        if self.options.len() > MAX_TEAM_TRANSACTION_DECISION_OPTIONS {
            return Err(format!(
                "options exceeds {MAX_TEAM_TRANSACTION_DECISION_OPTIONS} entries"
            ));
        }
        for option in &self.options {
            validate_text(
                "options",
                option,
                MAX_TEAM_TRANSACTION_DECISION_OPTION_BYTES,
            )?;
        }
        validate_unique("options", &self.options)?;
        if !self.is_held_on_founder() {
            validate_event_id("heldOn", &self.held_on)?;
        }
        validate_bounded_event_ids("blocks", &self.blocks, MAX_TEAM_TRANSACTION_DECISION_BLOCKS)?;
        validate_optional_text(
            "recommendation",
            self.recommendation.as_deref(),
            MAX_TEAM_TRANSACTION_SHORT_TEXT_BYTES,
        )
    }
}

impl CodingSessionTeamDecisionAnswer {
    fn validate(&self) -> Result<(), String> {
        validate_event_id("requestRef", &self.request_ref)?;
        match &self.choice {
            CodingSessionTeamDecisionChoice::Index(index) => {
                // An index can only ever name an option the request could
                // carry, so an out-of-range index is refused at the schema
                // rather than silently pointing at nothing.
                if usize::try_from(*index)
                    .map_or(true, |index| index >= MAX_TEAM_TRANSACTION_DECISION_OPTIONS)
                {
                    return Err(format!(
                        "choice index must be below {MAX_TEAM_TRANSACTION_DECISION_OPTIONS}"
                    ));
                }
            }
            CodingSessionTeamDecisionChoice::Text(text) => {
                validate_text("choice", text, MAX_TEAM_TRANSACTION_SHORT_TEXT_BYTES)?;
            }
        }
        validate_optional_text(
            "note",
            self.note.as_deref(),
            MAX_TEAM_TRANSACTION_TEXT_BYTES,
        )?;
        // Bounded and non-blank exactly like `note`, and for the same reason:
        // a blank condition claims a class and names none.
        validate_optional_text(
            "condition",
            self.condition.as_deref(),
            MAX_TEAM_TRANSACTION_DECISION_CONDITION_BYTES,
        )
    }
}

impl CodingSessionTeamAssignment {
    fn validate(&self) -> Result<(), String> {
        validate_event_id("assigneeActor", &self.assignee_actor)?;
        validate_role(&self.assignee_role)?;
        validate_text(
            "objective",
            &self.objective,
            MAX_TEAM_TRANSACTION_TEXT_BYTES,
        )?;
        validate_text("brief", &self.brief, MAX_TEAM_TRANSACTION_LONG_TEXT_BYTES)?;
        validate_optional_text("branch", self.branch.as_deref(), 255)?;
        validate_optional_git_sha("baseSha", self.base_sha.as_deref())?;
        validate_paths("fileOwnership", &self.file_ownership)?;
        validate_texts("acceptanceSteps", &self.acceptance_steps, true)?;
        Ok(())
    }
}

impl CodingSessionTeamReport {
    fn validate(&self) -> Result<(), String> {
        validate_event_id("assignmentRef", &self.assignment_ref)?;
        validate_text("summary", &self.summary, MAX_TEAM_TRANSACTION_TEXT_BYTES)?;
        validate_optional_text("branch", self.branch.as_deref(), 255)?;
        validate_optional_git_sha("baseSha", self.base_sha.as_deref())?;
        validate_optional_git_sha("headSha", self.head_sha.as_deref())?;
        validate_paths("files", &self.files)?;
        if self.tests.len() > MAX_TEAM_TRANSACTION_TESTS {
            return Err(format!(
                "tests exceeds {MAX_TEAM_TRANSACTION_TESTS} entries"
            ));
        }
        for test in &self.tests {
            test.validate()?;
        }
        validate_texts("deviations", &self.deviations, false)?;
        validate_texts("residuals", &self.residuals, false)?;
        validate_texts("anomalies", &self.anomalies, false)
    }
}

impl CodingSessionTeamTransactionTest {
    fn validate(&self) -> Result<(), String> {
        validate_text("test.name", &self.name, 512)?;
        validate_text(
            "test.command",
            &self.command,
            MAX_TEAM_TRANSACTION_TEXT_BYTES,
        )?;
        validate_optional_text(
            "test.evidence",
            self.evidence.as_deref(),
            MAX_TEAM_TRANSACTION_TEXT_BYTES,
        )
    }
}

impl CodingSessionTeamVerdict {
    fn validate(&self) -> Result<(), String> {
        let (assignment_ref, report_ref, summary, findings, required_action) = match self {
            Self::Refutation {
                assignment_ref,
                report_ref,
                summary,
                findings,
                required_action,
                ..
            }
            | Self::Disposition {
                assignment_ref,
                report_ref,
                summary,
                findings,
                required_action,
                ..
            } => (
                assignment_ref,
                report_ref,
                summary,
                findings,
                required_action,
            ),
        };
        validate_event_id("assignmentRef", assignment_ref)?;
        validate_event_id("reportRef", report_ref)?;
        if assignment_ref == report_ref {
            return Err("assignmentRef and reportRef must name different events".into());
        }
        if let Self::Disposition {
            refutation_ref: Some(reference),
            ..
        } = self
        {
            validate_event_id("refutationRef", reference)?;
            if reference == assignment_ref || reference == report_ref {
                return Err("refutationRef must name a different event".into());
            }
        }
        validate_text("summary", summary, MAX_TEAM_TRANSACTION_TEXT_BYTES)?;
        validate_texts("findings", findings, false)?;
        validate_optional_text(
            "requiredAction",
            required_action.as_deref(),
            MAX_TEAM_TRANSACTION_TEXT_BYTES,
        )
    }

    /// Return this verdict's closed subtype.
    pub const fn subtype(&self) -> CodingSessionTeamVerdictSubtype {
        match self {
            Self::Refutation { .. } => CodingSessionTeamVerdictSubtype::Refutation,
            Self::Disposition { .. } => CodingSessionTeamVerdictSubtype::Disposition,
        }
    }

    /// Event id of the assignment under review.
    pub fn assignment_ref(&self) -> &str {
        match self {
            Self::Refutation { assignment_ref, .. } | Self::Disposition { assignment_ref, .. } => {
                assignment_ref
            }
        }
    }

    /// Event id of the report under review.
    pub fn report_ref(&self) -> &str {
        match self {
            Self::Refutation { report_ref, .. } | Self::Disposition { report_ref, .. } => {
                report_ref
            }
        }
    }

    /// Explicit causal references carried by this verdict.
    pub fn causal_references(&self) -> Vec<&str> {
        let mut references = vec![self.assignment_ref(), self.report_ref()];
        if let Self::Disposition {
            refutation_ref: Some(reference),
            ..
        } = self
        {
            references.push(reference);
        }
        references
    }
}

impl CodingSessionTeamAcknowledgement {
    fn validate(&self) -> Result<(), String> {
        validate_event_id("acknowledgedEventRef", &self.acknowledged_event_ref)?;
        validate_optional_text(
            "note",
            self.note.as_deref(),
            MAX_TEAM_TRANSACTION_TEXT_BYTES,
        )
    }
}

impl CodingSessionTeamMissionCompleted {
    fn validate(&self) -> Result<(), String> {
        validate_event_ids("assignmentRefs", &self.assignment_refs, true)?;
        validate_git_shas("landedShas", &self.landed_shas)?;
        validate_text("summary", &self.summary, MAX_TEAM_TRANSACTION_TEXT_BYTES)?;
        validate_texts("followUps", &self.follow_ups, false)
    }
}

impl CodingSessionTeamMissionBlocked {
    fn validate(&self) -> Result<(), String> {
        validate_event_ids("assignmentRefs", &self.assignment_refs, false)?;
        validate_text("summary", &self.summary, MAX_TEAM_TRANSACTION_TEXT_BYTES)?;
        validate_texts("blockers", &self.blockers, true)?;
        validate_optional_text(
            "heldOn",
            self.held_on.as_deref(),
            MAX_TEAM_TRANSACTION_TEXT_BYTES,
        )?;
        validate_text(
            "requiredAction",
            &self.required_action,
            MAX_TEAM_TRANSACTION_TEXT_BYTES,
        )
    }
}

#[cfg(test)]
#[path = "coding_session_team_transaction_tests.rs"]
mod tests;
