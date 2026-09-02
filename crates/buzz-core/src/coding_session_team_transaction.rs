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

use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;

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
/// Exact wire token naming the founder as the party holding a decision.
pub const CODING_SESSION_TEAM_DECISION_FOUNDER: &str = "founder";
/// Exact refusal for a `mission.blocked` correction that names no blocker.
pub const TERMINAL_CANNOT_CLEAR_ITSELF: &str =
    "use a note or a decision.answer to clear a blocker; a terminal cannot clear itself";
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

/// Strictly decode and validate signed kind 44244 content.
pub fn decode_coding_session_team_transaction(
    content: &str,
) -> Result<CodingSessionTeamTransactionPayload, String> {
    if content.len() > MAX_TEAM_TRANSACTION_CONTENT_BYTES {
        return Err(format!(
            "coding-session team-transaction content exceeds {MAX_TEAM_TRANSACTION_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session team-transaction payload".to_owned())?;
    validate_exact_keys(
        value.as_object().ok_or_else(|| {
            "coding-session team-transaction payload must be an object".to_owned()
        })?,
        &[
            "schema",
            "sessionRef",
            "genesisRef",
            "type",
            "supersedes",
            "deliveryCommandId",
            "body",
        ],
        "team-transaction payload",
    )?;
    let transaction_type: CodingSessionTeamTransactionType = serde_json::from_value(
        value
            .get("type")
            .cloned()
            .ok_or_else(|| "team-transaction type is missing".to_owned())?,
    )
    .map_err(|_| "unsupported coding-session team-transaction type".to_owned())?;
    let body = value
        .get("body")
        .and_then(Value::as_object)
        .ok_or_else(|| "team-transaction body must be an object".to_owned())?;
    let expected_keys = if transaction_type == CodingSessionTeamTransactionType::Verdict {
        match body.get("subtype").and_then(Value::as_str) {
            Some("refutation") => &[
                "subtype",
                "assignmentRef",
                "reportRef",
                "decision",
                "summary",
                "findings",
                "requiredAction",
            ][..],
            Some("disposition") => &[
                "subtype",
                "assignmentRef",
                "reportRef",
                "refutationRef",
                "decision",
                "summary",
                "findings",
                "requiredAction",
            ][..],
            _ => return Err("unsupported team-transaction verdict subtype".to_owned()),
        }
    } else {
        expected_body_keys(transaction_type)
    };
    validate_exact_keys(body, expected_keys, "team-transaction body")?;
    if transaction_type == CodingSessionTeamTransactionType::Report {
        let tests = body
            .get("tests")
            .and_then(Value::as_array)
            .ok_or_else(|| "report tests must be an array".to_owned())?;
        for test in tests {
            validate_exact_keys(
                test.as_object()
                    .ok_or_else(|| "report test must be an object".to_owned())?,
                &["name", "command", "outcome", "evidence"],
                "report test",
            )?;
        }
    }

    // A second strict decode preserves serde's duplicate-field detection,
    // which the Value map above cannot represent.
    let payload: CodingSessionTeamTransactionPayload = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session team-transaction payload".to_owned())?;
    payload.validate()?;
    Ok(payload)
}

/// Validate the exact ordered event envelope and return its decoded payload.
pub fn validate_coding_session_team_transaction_envelope(
    event: &Event,
) -> Result<CodingSessionTeamTransactionPayload, String> {
    if event.kind.as_u16() as u32 != KIND_CODING_SESSION_TEAM_TRANSACTION {
        return Err("coding-session team transaction has the wrong event kind".into());
    }
    let payload = decode_coding_session_team_transaction(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 5 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session team transaction requires exactly five two-field tags".into());
    }
    if tags[0][0] != "h" {
        return Err("team-transaction first tag must be h=channel UUID".into());
    }
    validate_canonical_uuid("h", &tags[0][1])?;
    if tags[1][0] != "d" || tags[1][1] != payload.session_ref {
        return Err("team-transaction d tag does not match payload sessionRef".into());
    }
    if tags[2][0] != "cstx-v" || tags[2][1] != CODING_SESSION_TEAM_TRANSACTION_SCHEMA {
        return Err("unsupported coding-session team-transaction tag version".into());
    }
    if tags[3][0] != "cstx-genesis" || tags[3][1] != payload.genesis_ref {
        return Err("team-transaction genesis tag does not match payload genesisRef".into());
    }
    if tags[4][0] != "cstx-type" || tags[4][1] != payload.transaction_type.as_str() {
        return Err("team-transaction type tag does not match payload type".into());
    }
    let event_id = event.id.to_hex();
    if payload.supersedes.as_deref() == Some(event_id.as_str())
        || payload
            .causal_references()
            .into_iter()
            .any(|reference| reference == event_id)
    {
        return Err("team transaction cannot reference its own event id".into());
    }
    Ok(payload)
}

/// Validate a correction link when the superseded event is available.
///
/// Corrections must stay within one author, channel, session, genesis, and
/// operation type. Causal workflow links live in operation bodies and are not
/// accepted as substitutes for `supersedes`.
pub fn validate_coding_session_team_transaction_supersession(
    current: &Event,
    previous: &Event,
) -> Result<(), String> {
    let current_payload = validate_coding_session_team_transaction_envelope(current)?;
    let previous_payload = validate_coding_session_team_transaction_envelope(previous)?;
    if current_payload.supersedes.as_deref() != Some(previous.id.to_hex().as_str()) {
        return Err("supersedes does not name the supplied previous event".into());
    }
    if current.pubkey != previous.pubkey {
        return Err("a correction must have the same signed author".into());
    }
    if current_payload.session_ref != previous_payload.session_ref
        || current_payload.genesis_ref != previous_payload.genesis_ref
    {
        return Err("a correction cannot cross session or genesis".into());
    }
    if current_payload.transaction_type != previous_payload.transaction_type {
        return Err("a correction must preserve the operation type".into());
    }
    if current.tags.as_slice()[0].as_slice() != previous.tags.as_slice()[0].as_slice() {
        return Err("a correction cannot cross channels".into());
    }
    // A terminal that only rewrites its own prose is the shape the lead reached
    // for four times on 2026-09-01. Editing the sentence is now a `note`, and
    // clearing the blocker is a `decision.answer`; a correction of a blocked
    // terminal has to actually change what is blocking.
    if let (
        CodingSessionTeamTransactionBody::MissionBlocked(current_body),
        CodingSessionTeamTransactionBody::MissionBlocked(previous_body),
    ) = (&current_payload.body, &previous_payload.body)
    {
        if same_blocker_set(&current_body.blockers, &previous_body.blockers) {
            return Err(TERMINAL_PROSE_EDIT_NEEDS_A_NOTE.into());
        }
    }
    Ok(())
}

/// Whether two blocker lists name the same set, ignoring order.
///
/// Order is prose: reordering the same blockers says nothing new about what is
/// holding the mission up.
fn same_blocker_set(current: &[String], previous: &[String]) -> bool {
    if current.len() != previous.len() {
        return false;
    }
    let mut current: Vec<&str> = current.iter().map(String::as_str).collect();
    let mut previous: Vec<&str> = previous.iter().map(String::as_str).collect();
    current.sort_unstable();
    previous.sort_unstable();
    current == previous
}

fn expected_body_keys(
    transaction_type: CodingSessionTeamTransactionType,
) -> &'static [&'static str] {
    match transaction_type {
        CodingSessionTeamTransactionType::Assignment => &[
            "assigneeActor",
            "assigneeRole",
            "objective",
            "brief",
            "branch",
            "baseSha",
            "fileOwnership",
            "acceptanceSteps",
        ],
        CodingSessionTeamTransactionType::Report => &[
            "assignmentRef",
            "summary",
            "branch",
            "baseSha",
            "headSha",
            "files",
            "tests",
            "redBeforeGreen",
            "deviations",
            "residuals",
            "anomalies",
        ],
        // The decoder selects subtype-specific verdict keys before calling
        // this helper. An empty set remains fail-closed if a future caller
        // reaches this arm without doing so.
        CodingSessionTeamTransactionType::Verdict => &[],
        CodingSessionTeamTransactionType::Acknowledgement => {
            &["acknowledgedEventRef", "status", "note"]
        }
        CodingSessionTeamTransactionType::MissionCompleted => {
            &["assignmentRefs", "landedShas", "summary", "followUps"]
        }
        CodingSessionTeamTransactionType::MissionBlocked => &[
            "assignmentRefs",
            "summary",
            "blockers",
            "heldOn",
            "requiredAction",
        ],
        CodingSessionTeamTransactionType::Note => &["text", "refs"],
        CodingSessionTeamTransactionType::DecisionRequest => {
            &["question", "options", "heldOn", "blocks", "recommendation"]
        }
        CodingSessionTeamTransactionType::DecisionAnswer => &["requestRef", "choice", "note"],
    }
}

fn validate_exact_keys(
    object: &serde_json::Map<String, Value>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    if object.len() != expected.len()
        || !expected.iter().all(|key| object.contains_key(*key))
        || !object.keys().all(|key| expected.contains(&key.as_str()))
    {
        return Err(format!("{label} has missing or unsupported fields"));
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

fn validate_event_ids(
    field: &str,
    values: &[String],
    require_nonempty: bool,
) -> Result<(), String> {
    validate_collection_size(field, values.len(), require_nonempty)?;
    for value in values {
        validate_event_id(field, value)?;
    }
    validate_unique(field, values)
}

/// Validate a bounded, unique list of event ids with a per-field cap tighter
/// than [`MAX_TEAM_TRANSACTION_ITEMS`].
fn validate_bounded_event_ids(field: &str, values: &[String], max: usize) -> Result<(), String> {
    if values.len() > max {
        return Err(format!("{field} exceeds {max} entries"));
    }
    for value in values {
        validate_event_id(field, value)?;
    }
    validate_unique(field, values)
}

fn validate_git_sha(field: &str, value: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{field} must be a lowercase 40- or 64-hex git object id"
        ));
    }
    Ok(())
}

fn validate_optional_git_sha(field: &str, value: Option<&str>) -> Result<(), String> {
    if let Some(value) = value {
        validate_git_sha(field, value)?;
    }
    Ok(())
}

fn validate_git_shas(field: &str, values: &[String]) -> Result<(), String> {
    validate_collection_size(field, values.len(), false)?;
    for value in values {
        validate_git_sha(field, value)?;
    }
    validate_unique(field, values)
}

fn validate_role(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("assigneeRole must be a lowercase [a-z0-9-] slug of at most 64 bytes".into());
    }
    Ok(())
}

fn validate_delivery_command_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("deliveryCommandId must not be blank".into());
    }
    if value.len() > MAX_TEAM_TRANSACTION_DELIVERY_COMMAND_ID_BYTES {
        return Err(format!(
            "deliveryCommandId exceeds {MAX_TEAM_TRANSACTION_DELIVERY_COMMAND_ID_BYTES} bytes"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err("deliveryCommandId must not contain control characters".into());
    }
    Ok(())
}

fn validate_text(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if value.len() > max {
        return Err(format!("{field} exceeds {max} bytes"));
    }
    if value.contains('\0') {
        return Err(format!("{field} must not contain NUL"));
    }
    Ok(())
}

fn validate_optional_text(field: &str, value: Option<&str>, max: usize) -> Result<(), String> {
    if let Some(value) = value {
        validate_text(field, value, max)?;
    }
    Ok(())
}

fn validate_texts(field: &str, values: &[String], require_nonempty: bool) -> Result<(), String> {
    validate_collection_size(field, values.len(), require_nonempty)?;
    for value in values {
        validate_text(field, value, MAX_TEAM_TRANSACTION_TEXT_BYTES)?;
    }
    Ok(())
}

fn validate_paths(field: &str, values: &[String]) -> Result<(), String> {
    validate_collection_size(field, values.len(), false)?;
    for value in values {
        validate_text(field, value, MAX_TEAM_TRANSACTION_PATH_BYTES)?;
    }
    validate_unique(field, values)
}

fn validate_collection_size(field: &str, len: usize, require_nonempty: bool) -> Result<(), String> {
    if require_nonempty && len == 0 {
        return Err(format!("{field} must not be empty"));
    }
    if len > MAX_TEAM_TRANSACTION_ITEMS {
        return Err(format!(
            "{field} exceeds {MAX_TEAM_TRANSACTION_ITEMS} entries"
        ));
    }
    Ok(())
}

fn validate_unique(field: &str, values: &[String]) -> Result<(), String> {
    for (index, value) in values.iter().enumerate() {
        if values[..index].contains(value) {
            return Err(format!("{field} must not contain duplicates"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "coding_session_team_transaction_tests.rs"]
mod tests;
