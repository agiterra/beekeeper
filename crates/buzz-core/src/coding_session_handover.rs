//! NIP-CSH: coding-session handover records (kind 44247).
//!
//! Two records, one kind. A **checkpoint** is what the work *is*, written
//! durably enough that another authorized participant can pick it up: the
//! accepted task, the decisions taken, the revision it sits on, the artifacts
//! that carry the bytes, what the tests said, what is unresolved, the next
//! useful action, and — the part every other tool omits — what could **not**
//! be preserved. A **continuation** is the record of somebody having done
//! that: which claim they acted on, whether they resumed the original
//! execution natively or reconstructed the work somewhere else, and where it
//! now runs.
//!
//! # What this module validates, and what it does not
//!
//! Only facts self-contained in one event: exact JSON shape, closed
//! vocabularies, bounds, hex widths, and tag-to-content parity. The signature
//! is verified by the fold ([`crate::coding_session_handover_fold`]), and
//! **standing is decided there too** — whether the author was the founder, a
//! live operator or a seated actor at the time, and whether a continuation's
//! `claimRef` is the claim in force. The relay runs exactly the checks in this
//! module and nothing more (`docs/HANDOVER_IMPL.md` §2), for the reason kinds
//! 44244, 44245 and 44246 give: a relay that adjudicated standing would be
//! asserting authority it cannot verify.
//!
//! # Why a checkpoint names its predecessor
//!
//! `prevCheckpointRef` exists because a clock could not answer "which of these
//! is the newest statement". Three checkpoints published inside one second
//! ordered by `(created_at, id)`, and the id is a hash, so the "latest" one was
//! chosen at random and a reconstruction ran from a stale record
//! (`handover-composition-5.log`, finding 3). An author says what its
//! checkpoint replaces; the fold reads that and nothing else within one
//! author.
//!
//! # Why "preserved" is a word and not an inference
//!
//! `revision.dirty` says uncommitted changes existed; it does not say whether
//! they survived. A reader that inferred "dirty plus a patch artifact means
//! everything is here" would be guessing, and the guess fails exactly when it
//! matters — a file too large for the patch bound, a failed push, a binary the
//! diff could not carry. So the author states
//! [`CodingSessionHandoverPreserved`] in words, and every byte it could not
//! keep is enumerated by path under `missing`.

use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionTarget,
    CODING_SESSION_COMMAND_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_HANDOVER;

/// Exact v1 schema identifier, carried in the payload's `schema` field.
pub const CODING_SESSION_HANDOVER_SCHEMA: &str = "buzz-coding-session-handover/v1";

/// The version token carried in the `csh-v` tag.
pub const CODING_SESSION_HANDOVER_TAG_VERSION: &str = "csh1";

/// Maximum UTF-8 byte length of a complete signed handover payload.
///
/// Sized above the worst legal record and well below the relay's event ceiling:
/// a 4 KiB task, 32 × 512 B decision summaries, 32 unresolved lines, 16
/// artifacts, 32 test rows and a 2 KiB next action come to roughly 30 KiB.
pub const MAX_CODING_SESSION_HANDOVER_CONTENT_BYTES: usize = 32 * 1024;

/// Maximum UTF-8 byte length of a checkpoint's accepted task.
pub const MAX_HANDOVER_TASK_BYTES: usize = 4 * 1024;
/// Maximum UTF-8 byte length of a next action, or a continuation note.
pub const MAX_HANDOVER_PROSE_BYTES: usize = 2 * 1024;
/// Maximum UTF-8 byte length of one line: a decision summary, an unresolved
/// question, a recovered/missing line, a branch, a repository coordinate, or a
/// test name or command.
pub const MAX_HANDOVER_LINE_BYTES: usize = 512;
/// Maximum number of assignment references one checkpoint may name.
pub const MAX_HANDOVER_ASSIGNMENT_REFS: usize = 16;
/// Maximum number of decisions one checkpoint may carry.
pub const MAX_HANDOVER_DECISIONS: usize = 32;
/// Maximum number of artifacts one checkpoint may carry.
pub const MAX_HANDOVER_ARTIFACTS: usize = 16;
/// Maximum number of test rows one checkpoint may carry.
pub const MAX_HANDOVER_TESTS: usize = 32;
/// Maximum number of unresolved questions one checkpoint may carry.
pub const MAX_HANDOVER_UNRESOLVED: usize = 32;
/// Maximum number of `missing` lines one record may carry.
pub const MAX_HANDOVER_MISSING: usize = 16;
/// Maximum number of `recovered` lines one continuation may carry.
pub const MAX_HANDOVER_RECOVERED: usize = 32;

/// The closed handover vocabulary for schema v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionHandoverType {
    /// What the work is, durably enough to be picked up.
    Checkpoint,
    /// What a claimant did with it.
    Continuation,
}

impl CodingSessionHandoverType {
    /// The exact wire token used by content and by the `csh-type` tag.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Checkpoint => "checkpoint",
            Self::Continuation => "continuation",
        }
    }
}

/// How much of the working tree's uncommitted state the artifacts actually
/// hold.
///
/// Stated by the author, never inferred by a reader (see this module's
/// header). `Partial` is the honest and common answer, and it is what makes
/// `missing` worth reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionHandoverPreserved {
    /// Every uncommitted byte is carried by an artifact of this checkpoint.
    /// With `dirty: false` this is the ordinary clean-tree answer.
    All,
    /// Some uncommitted bytes are carried and some are not; every one that is
    /// not is enumerated under `missing`.
    Partial,
    /// No uncommitted bytes were preserved at all.
    None,
}

/// The closed artifact vocabulary: the three ways bytes travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionHandoverArtifactKind {
    /// A git ref the author pushed, resolvable from the relay's own git
    /// storage.
    WipRef,
    /// A NIP-34 patch event carrying a diff against `baseSha`.
    Patch,
    /// A Blossom blob, for a patch above the relay's event limit.
    Blob,
}

impl CodingSessionHandoverArtifactKind {
    /// The exact wire token for this artifact kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WipRef => "wip-ref",
            Self::Patch => "patch",
            Self::Blob => "blob",
        }
    }
}

/// What a test said when the checkpoint was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionHandoverTestOutcome {
    /// The command ran and passed.
    Passed,
    /// The command ran and failed.
    Failed,
    /// The command was not run. Never a stand-in for "passed".
    NotRun,
}

/// How the work continued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionHandoverMode {
    /// The original execution was resumed on its own provider.
    NativeResume,
    /// A new execution was created from the checkpoint's artifacts. The native
    /// context did not travel — the cursor never leaves the original disk —
    /// and the label says so.
    Reconstructed,
}

impl CodingSessionHandoverMode {
    /// The exact wire token for this mode.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NativeResume => "native-resume",
            Self::Reconstructed => "reconstructed",
        }
    }
}

/// One decision the checkpoint's author took, and where it is recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionHandoverDecision {
    /// Event id (lowercase 64-hex) of the record the decision was made in.
    pub event_id: String,
    /// One line saying what was decided.
    pub summary: String,
}

/// The revision the work sits on, and how much of it survived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionHandoverRevision {
    /// Repository coordinate, or `null` when the work is not in a repository.
    pub repo_ref: Option<String>,
    /// The commit the work is based on, or `null`.
    pub base_sha: Option<String>,
    /// The commit `HEAD` pointed at, or `null`.
    pub head_sha: Option<String>,
    /// The checked-out branch, or `null`.
    pub branch: Option<String>,
    /// Whether uncommitted changes existed when this was written.
    pub dirty: bool,
    /// Of those uncommitted bytes, how much the artifacts hold.
    pub preserved: CodingSessionHandoverPreserved,
}

/// One artifact carrying the work's bytes.
///
/// Deliberately one struct with a closed `kind` and nullable per-kind fields
/// rather than an untagged enum: an untagged enum answers a malformed artifact
/// with "data did not match any variant", which names neither the field nor
/// what it wanted, and this is the object a person chases when a
/// reconstruction comes up short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionHandoverArtifact {
    /// Which of the three carriers this is.
    pub kind: CodingSessionHandoverArtifactKind,
    /// Repository coordinate the artifact belongs to.
    pub repo_ref: String,
    /// `wip-ref` only: the pushed ref name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    /// `wip-ref` only: the commit that ref points at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    /// `patch` only: the NIP-34 patch event id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// `blob` only: the Blossom sha256 hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// `patch`/`blob` only: the commit the diff applies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    /// `patch`/`blob` only: the artifact's size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// One test the checkpoint's author ran, or deliberately did not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionHandoverTest {
    /// What the test is called.
    pub name: String,
    /// The exact command that produced the outcome.
    pub command: String,
    /// What it said.
    pub outcome: CodingSessionHandoverTestOutcome,
}

/// A checkpoint body: what the work is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionHandoverCheckpoint {
    /// The event id of this author's previous checkpoint of the same
    /// umbrella, or `None` for their first.
    ///
    /// **A key that is always present and whose value may be null**, exactly
    /// like `prevAccepted` on a 44228 link and `supersedes` on a 44244 record.
    /// Absent would be ambiguous between "my first checkpoint" and "a producer
    /// that forgot the field", and the decoder refuses that ambiguity rather
    /// than resolving it.
    ///
    /// # Why a reference and not a timestamp
    ///
    /// Composition run `handover-composition-5.log` (finding 3): three
    /// checkpoints published inside the same second sort by `(created_at, id)`,
    /// so the id — a hash — decided which was "latest", and B reconstructed
    /// from an older statement whose `missing` list was empty. A deterministic
    /// order is not a recent one. **This field is the author's own statement of
    /// what its checkpoint replaces**, and it is the only ordering the fold
    /// trusts within one author; the clock is left to do what it can across
    /// authors, where nobody can state a link anyway.
    pub prev_checkpoint_ref: Option<String>,
    /// The accepted task, in the author's words.
    pub task: String,
    /// Kind 44244 assignment ids this work answers.
    pub assignment_refs: Vec<String>,
    /// Decisions taken, each pointing at where it is recorded.
    pub decisions: Vec<CodingSessionHandoverDecision>,
    /// The revision the work sits on.
    pub revision: CodingSessionHandoverRevision,
    /// The artifacts carrying the bytes.
    pub artifacts: Vec<CodingSessionHandoverArtifact>,
    /// What the tests said.
    pub tests: Vec<CodingSessionHandoverTest>,
    /// Open questions the next participant inherits.
    pub unresolved: Vec<String>,
    /// The next useful action.
    pub next_action: String,
    /// Local-only bytes the author could not preserve, enumerated.
    pub missing: Vec<String>,
}

/// A continuation body: what the claimant did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionHandoverContinuation {
    /// Event id of the accepted `takeover`/`transfer` this acts on.
    pub claim_ref: String,
    /// Native resume, or reconstruction. Never conflated.
    pub mode: CodingSessionHandoverMode,
    /// The 44247 checkpoint the continuation was seeded from, or `null` when
    /// none existed and the caller proceeded anyway (and said so).
    pub checkpoint_ref: Option<String>,
    /// The execution now carrying the work.
    pub target: CodingSessionTarget,
    /// What was recovered, in lines a person can check.
    pub recovered: Vec<String>,
    /// What was not.
    pub missing: Vec<String>,
    /// Anything else the claimant wants on the record, or `null`.
    pub note: Option<String>,
}

/// Type-specific handover body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CodingSessionHandoverBody {
    /// A checkpoint body.
    Checkpoint(CodingSessionHandoverCheckpoint),
    /// A continuation body.
    Continuation(CodingSessionHandoverContinuation),
}

impl CodingSessionHandoverBody {
    /// The record type this body variant implies.
    pub const fn handover_type(&self) -> CodingSessionHandoverType {
        match self {
            Self::Checkpoint(_) => CodingSessionHandoverType::Checkpoint,
            Self::Continuation(_) => CodingSessionHandoverType::Continuation,
        }
    }
}

/// Strict public JSON carried by a kind 44247 event: exactly five top-level
/// keys, every one always present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionHandoverPayload {
    /// Always [`CODING_SESSION_HANDOVER_SCHEMA`].
    pub schema: String,
    /// Canonical lowercase UUID of the umbrella session.
    pub session_ref: String,
    /// Lowercase 64-hex event id of the session genesis.
    pub genesis_ref: String,
    /// Closed record token, repeated in `csh-type`.
    #[serde(rename = "type")]
    pub handover_type: CodingSessionHandoverType,
    /// Type-specific structured body.
    pub body: CodingSessionHandoverBody,
}

impl CodingSessionHandoverPayload {
    /// Validate the schema, references, type/body parity and every bound.
    ///
    /// # Errors
    /// A sentence naming the field that is wrong and what was wanted.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_HANDOVER_SCHEMA {
            return Err("unsupported coding-session handover schema".into());
        }
        validate_canonical_uuid("sessionRef", &self.session_ref)?;
        validate_event_id("genesisRef", &self.genesis_ref)?;
        if self.handover_type != self.body.handover_type() {
            return Err("coding-session handover type does not match body shape".into());
        }
        match &self.body {
            CodingSessionHandoverBody::Checkpoint(body) => body.validate(),
            CodingSessionHandoverBody::Continuation(body) => body.validate(),
        }
    }
}

impl CodingSessionHandoverCheckpoint {
    fn validate(&self) -> Result<(), String> {
        if let Some(reference) = &self.prev_checkpoint_ref {
            validate_event_id("checkpoint prevCheckpointRef", reference)?;
        }
        validate_prose("checkpoint task", &self.task, MAX_HANDOVER_TASK_BYTES)?;
        validate_prose(
            "checkpoint nextAction",
            &self.next_action,
            MAX_HANDOVER_PROSE_BYTES,
        )?;
        bound(
            "checkpoint assignmentRefs",
            self.assignment_refs.len(),
            MAX_HANDOVER_ASSIGNMENT_REFS,
        )?;
        for reference in &self.assignment_refs {
            validate_event_id("checkpoint assignmentRefs entry", reference)?;
        }
        bound(
            "checkpoint decisions",
            self.decisions.len(),
            MAX_HANDOVER_DECISIONS,
        )?;
        for decision in &self.decisions {
            validate_event_id("checkpoint decision eventId", &decision.event_id)?;
            validate_text(
                "checkpoint decision summary",
                &decision.summary,
                MAX_HANDOVER_LINE_BYTES,
            )?;
        }
        self.revision.validate()?;
        bound(
            "checkpoint artifacts",
            self.artifacts.len(),
            MAX_HANDOVER_ARTIFACTS,
        )?;
        for artifact in &self.artifacts {
            artifact.validate()?;
        }
        bound("checkpoint tests", self.tests.len(), MAX_HANDOVER_TESTS)?;
        for test in &self.tests {
            validate_text("checkpoint test name", &test.name, MAX_HANDOVER_LINE_BYTES)?;
            validate_text(
                "checkpoint test command",
                &test.command,
                MAX_HANDOVER_LINE_BYTES,
            )?;
        }
        bound(
            "checkpoint unresolved",
            self.unresolved.len(),
            MAX_HANDOVER_UNRESOLVED,
        )?;
        for line in &self.unresolved {
            validate_text("checkpoint unresolved entry", line, MAX_HANDOVER_LINE_BYTES)?;
        }
        bound(
            "checkpoint missing",
            self.missing.len(),
            MAX_HANDOVER_MISSING,
        )?;
        for line in &self.missing {
            validate_text("checkpoint missing entry", line, MAX_HANDOVER_LINE_BYTES)?;
        }
        // The one cross-field honesty rule this record can enforce on its own:
        // "nothing was preserved" and "here are the artifacts holding it" is a
        // contradiction, and a reader deciding which half to believe would be
        // guessing at the author's meaning.
        if self.revision.preserved == CodingSessionHandoverPreserved::None
            && self
                .artifacts
                .iter()
                .any(|artifact| artifact.kind != CodingSessionHandoverArtifactKind::WipRef)
        {
            return Err(
                "checkpoint revision.preserved is \"none\" but a patch or blob artifact is \
                 present: say \"partial\" or \"all\", or drop the artifact"
                    .into(),
            );
        }
        if self.revision.preserved != CodingSessionHandoverPreserved::All && self.missing.is_empty()
        {
            return Err(
                "checkpoint revision.preserved is not \"all\", so every byte that was not \
                 preserved must be enumerated under missing"
                    .into(),
            );
        }
        Ok(())
    }
}

impl CodingSessionHandoverRevision {
    fn validate(&self) -> Result<(), String> {
        validate_optional_text(
            "checkpoint revision repoRef",
            self.repo_ref.as_deref(),
            MAX_HANDOVER_LINE_BYTES,
        )?;
        validate_optional_text(
            "checkpoint revision branch",
            self.branch.as_deref(),
            MAX_HANDOVER_LINE_BYTES,
        )?;
        for (field, value) in [
            ("checkpoint revision baseSha", self.base_sha.as_deref()),
            ("checkpoint revision headSha", self.head_sha.as_deref()),
        ] {
            if let Some(value) = value {
                validate_git_object_id(field, value)?;
            }
        }
        Ok(())
    }
}

impl CodingSessionHandoverArtifact {
    fn validate(&self) -> Result<(), String> {
        validate_text("artifact repoRef", &self.repo_ref, MAX_HANDOVER_LINE_BYTES)?;
        // Per-kind exactness in both directions: every field the kind needs is
        // present, and every field it does not is absent. A `patch` carrying a
        // `ref` is a producer that made up a shape.
        let (required, forbidden): (&[&str], &[&str]) = match self.kind {
            CodingSessionHandoverArtifactKind::WipRef => {
                (&["ref", "sha"], &["eventId", "hash", "baseSha", "bytes"])
            }
            CodingSessionHandoverArtifactKind::Patch => {
                (&["eventId", "baseSha", "bytes"], &["ref", "sha", "hash"])
            }
            CodingSessionHandoverArtifactKind::Blob => {
                (&["hash", "baseSha", "bytes"], &["ref", "sha", "eventId"])
            }
        };
        let present = |field: &str| -> bool {
            match field {
                "ref" => self.r#ref.is_some(),
                "sha" => self.sha.is_some(),
                "eventId" => self.event_id.is_some(),
                "hash" => self.hash.is_some(),
                "baseSha" => self.base_sha.is_some(),
                "bytes" => self.bytes.is_some(),
                _ => false,
            }
        };
        for field in required {
            if !present(field) {
                return Err(format!(
                    "{:?} artifact requires {field:?}",
                    self.kind.as_str()
                ));
            }
        }
        for field in forbidden {
            if present(field) {
                return Err(format!(
                    "{:?} artifact must not carry {field:?}",
                    self.kind.as_str()
                ));
            }
        }
        if let Some(name) = &self.r#ref {
            validate_text("artifact ref", name, MAX_HANDOVER_LINE_BYTES)?;
        }
        if let Some(sha) = &self.sha {
            validate_git_object_id("artifact sha", sha)?;
        }
        if let Some(event_id) = &self.event_id {
            validate_event_id("artifact eventId", event_id)?;
        }
        if let Some(hash) = &self.hash {
            validate_event_id("artifact hash", hash)?;
        }
        if let Some(base_sha) = &self.base_sha {
            validate_git_object_id("artifact baseSha", base_sha)?;
        }
        Ok(())
    }
}

impl CodingSessionHandoverContinuation {
    fn validate(&self) -> Result<(), String> {
        validate_event_id("continuation claimRef", &self.claim_ref)?;
        if let Some(reference) = &self.checkpoint_ref {
            validate_event_id("continuation checkpointRef", reference)?;
        }
        // The target is the execution now carrying the work, so it is held to
        // exactly the shape a command could have addressed — a continuation
        // naming an unaddressable execution would point at nothing. Validated
        // through the command payload that owns those rules rather than by a
        // second copy of them, exactly as `validate_session_metadata` does.
        CodingSessionCommandPayload {
            schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
            command_id: "handover-continuation-validation".to_owned(),
            target: self.target.clone(),
            action: CodingSessionAction::ThreadTurnInterrupt,
        }
        .validate()
        .map_err(|error| format!("continuation {error}"))?;
        bound(
            "continuation recovered",
            self.recovered.len(),
            MAX_HANDOVER_RECOVERED,
        )?;
        for line in &self.recovered {
            validate_text(
                "continuation recovered entry",
                line,
                MAX_HANDOVER_LINE_BYTES,
            )?;
        }
        bound(
            "continuation missing",
            self.missing.len(),
            MAX_HANDOVER_MISSING,
        )?;
        for line in &self.missing {
            validate_text("continuation missing entry", line, MAX_HANDOVER_LINE_BYTES)?;
        }
        validate_optional_prose(
            "continuation note",
            self.note.as_deref(),
            MAX_HANDOVER_PROSE_BYTES,
        )
    }
}

/// Top-level keys, in the order a writer emits them.
const HANDOVER_KEYS: &[&str] = &["schema", "sessionRef", "genesisRef", "type", "body"];
const CHECKPOINT_KEYS: &[&str] = &[
    "prevCheckpointRef",
    "task",
    "assignmentRefs",
    "decisions",
    "revision",
    "artifacts",
    "tests",
    "unresolved",
    "nextAction",
    "missing",
];
const CONTINUATION_KEYS: &[&str] = &[
    "claimRef",
    "mode",
    "checkpointRef",
    "target",
    "recovered",
    "missing",
    "note",
];
/// Checkpoint keys whose value may be JSON `null`. Only the supersession
/// reference: an author's first checkpoint has nothing to replace, and says so
/// with a null rather than by omitting the key.
const CHECKPOINT_NULLABLE_KEYS: &[&str] = &["prevCheckpointRef"];
/// Continuation keys whose value may be JSON `null`; every other key requires
/// a value, and an absent key is refused by name.
const CONTINUATION_NULLABLE_KEYS: &[&str] = &["checkpointRef", "note"];
const REVISION_KEYS: &[&str] = &[
    "repoRef",
    "baseSha",
    "headSha",
    "branch",
    "dirty",
    "preserved",
];
const REVISION_NULLABLE_KEYS: &[&str] = &["repoRef", "baseSha", "headSha", "branch"];
const PRESERVED_TOKENS: &[&str] = &["all", "partial", "none"];
const ARTIFACT_KIND_TOKENS: &[&str] = &["wip-ref", "patch", "blob"];
const TEST_OUTCOME_TOKENS: &[&str] = &["passed", "failed", "not-run"];
const MODE_TOKENS: &[&str] = &["native-resume", "reconstructed"];
const HANDOVER_TYPE_TOKENS: &[&str] = &["checkpoint", "continuation"];

/// Strictly decode and validate signed kind 44247 content.
///
/// Refuses unknown keys rather than ignoring them, refuses a missing key and
/// an explicit `null` in a required position **by name**, and never accepts a
/// token outside the closed vocabularies. The closed sets are checked here
/// rather than left to serde for the reason kind 44246 gives: an untagged body
/// enum answers an unknown token with "data did not match any variant", which
/// names neither the field nor the set, and a refusal a publisher cannot act
/// on is barely a refusal.
///
/// # Errors
/// A sentence naming the field and, for a closed vocabulary, the whole set.
pub fn decode_coding_session_handover(
    content: &str,
) -> Result<CodingSessionHandoverPayload, String> {
    if content.len() > MAX_CODING_SESSION_HANDOVER_CONTENT_BYTES {
        return Err(format!(
            "coding-session handover content exceeds \
             {MAX_CODING_SESSION_HANDOVER_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session handover payload: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "coding-session handover payload must be an object".to_owned())?;
    validate_exact_keys(object, HANDOVER_KEYS, "handover payload")?;
    reject_required_nulls(object, HANDOVER_KEYS, &[], "handover payload")?;
    validate_closed_token(object, "type", HANDOVER_TYPE_TOKENS, "handover payload")?;

    let handover_type: CodingSessionHandoverType = serde_json::from_value(
        object
            .get("type")
            .cloned()
            .ok_or_else(|| "coding-session handover type is missing".to_owned())?,
    )
    .map_err(|_| {
        format!(
            "unsupported coding-session handover type: v1 knows exactly {}",
            joined_tokens(HANDOVER_TYPE_TOKENS)
        )
    })?;

    let body = object
        .get("body")
        .and_then(Value::as_object)
        .ok_or_else(|| "coding-session handover body must be an object".to_owned())?;
    match handover_type {
        CodingSessionHandoverType::Checkpoint => {
            validate_exact_keys(body, CHECKPOINT_KEYS, "handover checkpoint body")?;
            reject_required_nulls(
                body,
                CHECKPOINT_KEYS,
                CHECKPOINT_NULLABLE_KEYS,
                "handover checkpoint body",
            )?;
            let revision = body
                .get("revision")
                .and_then(Value::as_object)
                .ok_or_else(|| "handover checkpoint revision must be an object".to_owned())?;
            validate_exact_keys(revision, REVISION_KEYS, "handover checkpoint revision")?;
            reject_required_nulls(
                revision,
                REVISION_KEYS,
                REVISION_NULLABLE_KEYS,
                "handover checkpoint revision",
            )?;
            validate_closed_token(
                revision,
                "preserved",
                PRESERVED_TOKENS,
                "handover checkpoint revision",
            )?;
            for artifact in array_of_objects(body, "artifacts", "handover checkpoint artifact")? {
                validate_closed_token(
                    artifact,
                    "kind",
                    ARTIFACT_KIND_TOKENS,
                    "handover checkpoint artifact",
                )?;
            }
            for test in array_of_objects(body, "tests", "handover checkpoint test")? {
                validate_closed_token(
                    test,
                    "outcome",
                    TEST_OUTCOME_TOKENS,
                    "handover checkpoint test",
                )?;
            }
        }
        CodingSessionHandoverType::Continuation => {
            validate_exact_keys(body, CONTINUATION_KEYS, "handover continuation body")?;
            reject_required_nulls(
                body,
                CONTINUATION_KEYS,
                CONTINUATION_NULLABLE_KEYS,
                "handover continuation body",
            )?;
            validate_closed_token(body, "mode", MODE_TOKENS, "handover continuation body")?;
        }
    }

    // A second strict decode preserves serde's duplicate-field detection,
    // which the `Value` map above cannot represent — the same reason kinds
    // 44244 and 44246 decode twice.
    let payload: CodingSessionHandoverPayload = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session handover payload: {error}"))?;
    payload.validate()?;
    Ok(payload)
}

/// Validate the exact ordered event envelope and return its decoded payload.
///
/// Five two-field tags, in order: `h` (channel UUID), `d` (the umbrella),
/// `csh-v` (the tag version), `csh-genesis` (the founding event), `csh-type`
/// (the record token). `d`, `csh-genesis` and `csh-type` must agree with the
/// content, so a handover record cannot be filed under one umbrella while
/// claiming another, or be indexed as a checkpoint while carrying a
/// continuation.
///
/// # Errors
/// A sentence naming the tag or field that disagrees.
pub fn validate_coding_session_handover_envelope(
    event: &Event,
) -> Result<CodingSessionHandoverPayload, String> {
    if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_HANDOVER {
        return Err("coding-session handover has the wrong event kind".into());
    }
    let payload = decode_coding_session_handover(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 5 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session handover requires exactly five two-field tags".into());
    }
    if tags[0][0] != "h" {
        return Err("coding-session handover first tag must be h=channel UUID".into());
    }
    validate_canonical_uuid("h", &tags[0][1])?;
    if tags[1][0] != "d" || tags[1][1] != payload.session_ref {
        return Err("coding-session handover d tag does not match payload sessionRef".into());
    }
    if tags[2][0] != "csh-v" || tags[2][1] != CODING_SESSION_HANDOVER_TAG_VERSION {
        return Err("unsupported coding-session handover tag version".into());
    }
    if tags[3][0] != "csh-genesis" || tags[3][1] != payload.genesis_ref {
        return Err("coding-session handover genesis tag does not match payload genesisRef".into());
    }
    if tags[4][0] != "csh-type" || tags[4][1] != payload.handover_type.as_str() {
        return Err("coding-session handover type tag does not match payload type".into());
    }
    Ok(payload)
}

fn array_of_objects<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    field: &str,
) -> Result<Vec<&'a serde_json::Map<String, Value>>, String> {
    let array = object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("coding-session {field} list must be an array"))?;
    array
        .iter()
        .map(|entry| {
            entry
                .as_object()
                .ok_or_else(|| format!("coding-session {field} must be an object"))
        })
        .collect()
}

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
    field: &str,
) -> Result<(), String> {
    for key in keys {
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

fn bound(field: &str, got: usize, max: usize) -> Result<(), String> {
    if got > max {
        return Err(format!(
            "{field} holds {got} entries, more than the {max} allowed"
        ));
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

/// A git object id as kinds 44244 and 44246 write one: lowercase hex, 40
/// characters under SHA-1 and 64 under SHA-256.
fn validate_git_object_id(field: &str, value: &str) -> Result<(), String> {
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
#[path = "coding_session_handover_tests.rs"]
mod tests;
