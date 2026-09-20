//! NIP-PW: the closed kind:44249 work-record envelope (`buzz-project-work/v1`).
//!
//! One kind carries all three records — `work.declared`,
//! `work.assignment_bound` and `work.evidence_bound`. They share an envelope,
//! an authority rule and a fold, and 44244 stays closed and unchanged.
//!
//! This module validates only facts that are **self-contained in one event**:
//! the exact JSON and tag shape, the six ordered tags, tag-to-content parity,
//! every bound, and reference *syntax*. Whether a referenced event exists,
//! whether the plan blob resolves, and whether the signer held `may_lead` are
//! the consuming fold's questions — see [`crate::project_work_fold`] — exactly
//! as they are for kinds 44244 through 44247.
//!
//! Nullable keys are **present and written as JSON `null`**, never absent. An
//! absent key is a refusal, which is what makes a complete `--example`
//! possible for every body.
//!
//! The normative contract is `conformance/project-work/README.md` § (b),
//! restated in `docs/nips/NIP-PW.md`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::kind::KIND_PROJECT_WORK_RECORD;

/// Exact v1 schema identifier carried in content and the `pwk-v` tag.
pub const PROJECT_WORK_SCHEMA: &str = "buzz-project-work/v1";
/// Maximum UTF-8 byte length of a complete work-record payload.
///
/// 16 KiB, not 44244's 128 KiB: every field here is a pointer or a slug. A
/// record that needs more prose is carrying something that belongs in the
/// plan blob or in a 44244 report.
pub const MAX_PROJECT_WORK_CONTENT_BYTES: usize = 16 * 1024;
/// Maximum number of criterion ids one binding may name.
pub const MAX_PROJECT_WORK_CRITERION_IDS: usize = 64;
/// Maximum number of evidence references one binding may carry.
pub const MAX_PROJECT_WORK_EVIDENCE_REFS: usize = 32;
/// Maximum number of declarations one declaration may supersede.
pub const MAX_PROJECT_WORK_SUPERSEDES: usize = 8;
/// Maximum byte length of `planRef.repository`.
pub const MAX_PROJECT_WORK_REPOSITORY_BYTES: usize = 128;
/// Maximum byte length of `projectRef`.
pub const MAX_PROJECT_WORK_PROJECT_REF_BYTES: usize = 256;
/// Exact number of ordered two-field tags a work record carries.
pub const PROJECT_WORK_TAG_COUNT: usize = 6;

/// The closed record vocabulary for schema v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectWorkRecordType {
    /// A lead adopts `plans/<slug>.md` at an exact agents commit.
    #[serde(rename = "work.declared")]
    Declared,
    /// Which criteria an assignment owes.
    #[serde(rename = "work.assignment_bound")]
    AssignmentBound,
    /// Which evidence answers which criteria, at which artifact commit.
    #[serde(rename = "work.evidence_bound")]
    EvidenceBound,
}

impl ProjectWorkRecordType {
    /// The exact wire token used by `content.type` and the `pwk-type` tag.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Declared => "work.declared",
            Self::AssignmentBound => "work.assignment_bound",
            Self::EvidenceBound => "work.evidence_bound",
        }
    }

    /// Resolve a wire token, or `None` when the vocabulary does not know it.
    #[must_use]
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "work.declared" => Some(Self::Declared),
            "work.assignment_bound" => Some(Self::AssignmentBound),
            "work.evidence_bound" => Some(Self::EvidenceBound),
            _ => None,
        }
    }
}

/// Where a plan blob is read from: a repository, a commit and a path.
///
/// `repository` is the **full** kind:30617 coordinate, the same shape the
/// seat manifest's `packRef.repo` uses. A bare repo id is refused: a name is
/// community-scoped and would have to be re-resolved by every later reader,
/// which is how two readers end up pinning two repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectWorkPlanRef {
    /// `30617:<64-hex owner>:<repo id>`.
    pub repository: String,
    /// Full immutable agents commit, 40 or 64 lowercase hex.
    pub commit: String,
    /// Relative path under `plans/`, at most 256 bytes.
    pub path: String,
}

impl ProjectWorkPlanRef {
    /// The key this plan blob is looked up under: `<coordinate>@<commit>:<path>`.
    ///
    /// One composition, used by the fold and by every caller that supplies
    /// blobs, so a producer and a consumer cannot key the same blob two ways.
    #[must_use]
    pub fn blob_key(&self) -> String {
        format!("{}@{}:{}", self.repository, self.commit, self.path)
    }
}

/// `work.declared` — a lead adopts a committed plan as this work's contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectWorkDeclared {
    /// Canonical lowercase uuid, **stable across amendments**.
    pub work_id: String,
    /// The session's kind:44227 goal event this work serves.
    ///
    /// Only the goal. A decision that bounded the scope is a separate
    /// pointer, not this one.
    pub goal_ref: String,
    /// The actor who owes the outcome; a target, never an authorship claim.
    pub responsible_actor: String,
    /// Where the plan blob is read from.
    pub plan_ref: ProjectWorkPlanRef,
    /// Declaration event ids this one replaces; `[]` on first adoption.
    pub supersedes: Vec<String>,
}

/// `work.assignment_bound` — which criteria a 44244 assignment owes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectWorkAssignmentBound {
    /// The `work.declared` event this binds to.
    pub declaration_ref: String,
    /// Criterion slugs, 1..=64, unique.
    pub criterion_ids: Vec<String>,
    /// A kind:44244 `assignment` event id.
    pub assignment_ref: String,
    /// The earlier binding this supersedes; present, `null` when there is none.
    pub replaces_binding: Option<String>,
}

/// The closed evidence vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectWorkEvidenceKind {
    /// A kind:44244 `report`.
    #[serde(rename = "report")]
    Report,
    /// A kind:44244 `verdict`.
    #[serde(rename = "verdict")]
    Verdict,
    /// A recorded action result.
    #[serde(rename = "action_result")]
    ActionResult,
    /// A **relay-signed kind:30618 ref state** for the plan's `code_repository`.
    #[serde(rename = "ref_observation")]
    RefObservation,
}

impl ProjectWorkEvidenceKind {
    /// The exact wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::Verdict => "verdict",
            Self::ActionResult => "action_result",
            Self::RefObservation => "ref_observation",
        }
    }

    /// Resolve a wire token, or `None` when the vocabulary does not know it.
    #[must_use]
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "report" => Some(Self::Report),
            "verdict" => Some(Self::Verdict),
            "action_result" => Some(Self::ActionResult),
            "ref_observation" => Some(Self::RefObservation),
            _ => None,
        }
    }
}

/// One pointer to the event that carries a piece of evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectWorkEvidenceRef {
    /// Which kind of evidence the event is.
    pub kind: ProjectWorkEvidenceKind,
    /// The 64-hex event id.
    pub event_id: String,
}

/// `work.evidence_bound` — which evidence answers which criteria.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectWorkEvidenceBound {
    /// The `work.declared` event this binds to.
    pub declaration_ref: String,
    /// Criterion slugs, 1..=64, unique.
    pub criterion_ids: Vec<String>,
    /// The **code** commit the evidence is about, 40 or 64 lowercase hex.
    pub artifact_commit: String,
    /// 1..=32 evidence pointers.
    pub evidence_refs: Vec<ProjectWorkEvidenceRef>,
    /// The kind:44244 `mission.completed` this was computed for; present,
    /// `null` when none.
    pub completion_ref: Option<String>,
}

/// The type-specific body of one work record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectWorkBody {
    /// A `work.declared` body.
    Declared(ProjectWorkDeclared),
    /// A `work.assignment_bound` body.
    AssignmentBound(ProjectWorkAssignmentBound),
    /// A `work.evidence_bound` body.
    EvidenceBound(ProjectWorkEvidenceBound),
}

impl ProjectWorkBody {
    /// Which record type this body belongs to.
    #[must_use]
    pub const fn record_type(&self) -> ProjectWorkRecordType {
        match self {
            Self::Declared(_) => ProjectWorkRecordType::Declared,
            Self::AssignmentBound(_) => ProjectWorkRecordType::AssignmentBound,
            Self::EvidenceBound(_) => ProjectWorkRecordType::EvidenceBound,
        }
    }
}

impl Serialize for ProjectWorkBody {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Declared(body) => body.serialize(serializer),
            Self::AssignmentBound(body) => body.serialize(serializer),
            Self::EvidenceBound(body) => body.serialize(serializer),
        }
    }
}

/// A complete kind:44249 payload — the closed envelope plus one body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkPayload {
    /// Always [`PROJECT_WORK_SCHEMA`].
    pub schema: String,
    /// The session this work belongs to; equals the `d` tag.
    pub session_ref: String,
    /// The session genesis event id; equals the `pwk-genesis` tag.
    pub genesis_ref: String,
    /// `30621:<64-hex>:<d>`; equals the `a` tag.
    pub project_ref: String,
    /// The record type; equals the `pwk-type` tag.
    #[serde(rename = "type")]
    pub record_type: ProjectWorkRecordType,
    /// The type-specific body.
    pub body: ProjectWorkBody,
}

impl ProjectWorkPayload {
    /// The canonical content bytes for this payload.
    ///
    /// Key order is the contract's declaration order, and the form is compact
    /// JSON — exactly what a valid record fixture carries, byte for byte.
    ///
    /// # Errors
    ///
    /// Returns a refusal when the payload cannot be serialized, which in
    /// practice means a body a caller built by hand is not representable.
    pub fn canonical_content(&self) -> Result<String, ProjectWorkRefusal> {
        serde_json::to_string(self).map_err(|error| {
            ProjectWorkRefusal::new(
                ProjectWorkRefusalCode::Malformed,
                "content",
                format!("payload serialization failed: {error}"),
            )
        })
    }

    /// The six ordered two-field tags this payload requires, given its channel.
    ///
    /// The builder in `buzz-sdk` and the tag-parity check in this module read
    /// the same list, so an envelope cannot be written one way and judged
    /// another.
    #[must_use]
    pub fn canonical_tags(&self, channel_ref: &str) -> [[String; 2]; PROJECT_WORK_TAG_COUNT] {
        [
            ["h".to_owned(), channel_ref.to_owned()],
            ["d".to_owned(), self.session_ref.clone()],
            ["a".to_owned(), self.project_ref.clone()],
            ["pwk-v".to_owned(), PROJECT_WORK_SCHEMA.to_owned()],
            ["pwk-genesis".to_owned(), self.genesis_ref.clone()],
            ["pwk-type".to_owned(), self.record_type.as_str().to_owned()],
        ]
    }
}

/// One stored event as the work vocabulary sees it.
///
/// The subset of a Nostr event these rules read. A relay-stored event
/// converts losslessly through [`From`], and the conformance fixtures are
/// written in this shape directly — one decoder for the relay, the fold and
/// the tests rather than three that can disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorkEvent {
    /// Event id, 64 hex.
    pub id: String,
    /// Author, 64 hex.
    pub pubkey: String,
    /// Seconds since the epoch.
    pub created_at: u64,
    /// Event kind.
    pub kind: u32,
    /// Ordered tags.
    pub tags: Vec<Vec<String>>,
    /// Content JSON, as a string, exactly as on the wire.
    pub content: String,
}

impl ProjectWorkEvent {
    /// The value of the first tag named `name`, if any.
    #[must_use]
    pub fn tag_value(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|tag| tag.first().map(String::as_str) == Some(name))
            .and_then(|tag| tag.get(1))
            .map(String::as_str)
    }

    /// The sort key that makes every fold over these events order-independent.
    #[must_use]
    pub fn order_key(&self) -> (u64, &str) {
        (self.created_at, self.id.as_str())
    }
}

impl From<&nostr::Event> for ProjectWorkEvent {
    fn from(event: &nostr::Event) -> Self {
        Self {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: crate::kind::event_kind_u32(event),
            tags: event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
        }
    }
}

/// Why a work record was refused, as a stable code.
///
/// The strings are the contract's: every invalid record fixture names one of
/// them before the colon in its `refusal` field, and
/// `project_work_tests.rs` binds each fixture to the code it declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectWorkRefusalCode {
    /// The event is not kind 44249.
    WrongKind,
    /// `content` is not valid JSON, or is not an object.
    Malformed,
    /// `content` exceeds [`MAX_PROJECT_WORK_CONTENT_BYTES`].
    TooLarge,
    /// `schema` is absent or is not [`PROJECT_WORK_SCHEMA`].
    Schema,
    /// A tag count, order or arity that is not the six the contract fixes.
    TagCount,
    /// A tag whose value disagrees with the content it restates.
    TagParity,
    /// A `pwk-type` or `content.type` outside the closed vocabulary.
    RecordType,
    /// A key outside a closed key set, at any level.
    UnknownKey,
    /// A required key — including a nullable one — is absent.
    AbsentKey,
    /// A value has the wrong JSON type.
    WrongType,
    /// A hex reference of the wrong length or case.
    Hex,
    /// A uuid that is not canonical lowercase.
    Uuid,
    /// An `a` tag or `projectRef` that is not a canonical 30621 coordinate.
    ProjectRef,
    /// A `planRef.repository` that is not the full 30617 coordinate.
    Repository,
    /// A `planRef.path` that is not a relative path under `plans/`.
    Path,
    /// A value that is not slug grammar.
    Slug,
    /// A list that must not be empty, and is.
    EmptyList,
    /// A list longer than its ceiling.
    TooManyItems,
    /// A repeated entry in a list whose entries must be unique.
    Duplicate,
    /// A value outside a closed enumeration.
    ClosedEnum,
    /// A record that references its own event id.
    SelfReference,
}

impl ProjectWorkRefusalCode {
    /// The stable string form, as the contract writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrongKind => "wrong-kind",
            Self::Malformed => "malformed",
            Self::TooLarge => "too-large",
            Self::Schema => "schema",
            Self::TagCount => "tag-count",
            Self::TagParity => "tag-parity",
            Self::RecordType => "record-type",
            Self::UnknownKey => "unknown-key",
            Self::AbsentKey => "absent-key",
            Self::WrongType => "wrong-type",
            Self::Hex => "hex",
            Self::Uuid => "uuid",
            Self::ProjectRef => "project-ref",
            Self::Repository => "repository",
            Self::Path => "path",
            Self::Slug => "slug",
            Self::EmptyList => "empty-list",
            Self::TooManyItems => "too-many-items",
            Self::Duplicate => "duplicate",
            Self::ClosedEnum => "closed-enum",
            Self::SelfReference => "self-reference",
        }
    }
}

impl std::fmt::Display for ProjectWorkRefusalCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One refusal: the code, where the defect is, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWorkRefusal {
    /// The stable code.
    pub code: ProjectWorkRefusalCode,
    /// Where the defect is, e.g. `body.criterionIds[1]`.
    pub path: String,
    /// One sentence a person can act on.
    pub message: String,
}

impl ProjectWorkRefusal {
    fn new(
        code: ProjectWorkRefusalCode,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProjectWorkRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} ({})", self.code, self.message, self.path)
    }
}

impl std::error::Error for ProjectWorkRefusal {}

// Decoding, envelope validation and the field validators live in a sibling
// file for the same reason kind 44244's do: no file in this crate passes
// 1,000 lines. A child module, so it reads this module's types, constants and
// private helpers unchanged.
#[path = "project_work_decode.rs"]
mod decode;
pub use decode::{
    decode_project_work_content, is_canonical_project_coordinate,
    is_canonical_repository_coordinate, validate_project_work_envelope,
    validate_project_work_payload,
};

#[cfg(test)]
#[path = "project_work_tests.rs"]
mod tests;
