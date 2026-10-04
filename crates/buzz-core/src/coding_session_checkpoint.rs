//! NIP-CSCK: coding-session turn checkpoints (kind 44231).
//!
//! One provider-signed fact per turn: what the working tree was when the turn
//! ended, which transcript range the turn covers, and which files it changed —
//! or, when the tree could not be read, the closed reason why. The wire
//! contract is `docs/nips/NIP-CSCK.md`.
//!
//! # What this module validates, and what it does not
//!
//! Only facts self-contained in one event: exact JSON and tag shape, closed
//! vocabularies, bounds, path shape, and tag-to-content parity. Whether the
//! signer is the generation's provider — the key that signs its 44225 items —
//! is the consumer's question; the relay checks structure only, as it does for
//! every provider-authored coding-session kind.
//!
//! # Rules v1 commits to
//!
//! - Unknown keys are **rejected, not ignored**, at every nesting level.
//! - **Absent is not null** (the NIP-CSOB rule). Every key is always present;
//!   an unset optional is JSON `null`, and a `null` where a value is required
//!   is refused by name.
//! - **Exactly one of `git` and `unavailable` is non-null.** A capture that
//!   failed is still published, so a reader can say why there is no diff
//!   instead of showing nothing.
//! - **Paths are repo-relative and nothing else.** An absolute path, a `..`
//!   segment, a NUL or other control character, a ref name and a redaction
//!   marker are refused when encoding and again when decoding, so a host path
//!   or a redacted secret can never ride a checkpoint onto the relay.
//! - `summary` is reserved for the continuity research's context use and must
//!   be `null` in v1.

use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use crate::kind::KIND_CODING_SESSION_CHECKPOINT;

/// Exact v1 schema identifier carried in content.
pub const CODING_SESSION_CHECKPOINT_SCHEMA: &str = "buzz-coding-session-checkpoint/v1";
/// Value of the `csck-v` tag on every v1 checkpoint.
pub const CODING_SESSION_CHECKPOINT_TAG_VERSION: &str = "csck1-1";
/// Domain of the `csck-key` structured semantic key.
pub const CODING_SESSION_CHECKPOINT_KEY_DOMAIN: &str = "coding-session-checkpoint/v1";

/// Maximum UTF-8 byte length of a complete signed checkpoint payload — the
/// same ceiling as a 44225 transcript item.
pub const MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES: usize = 32 * 1024;
/// Maximum UTF-8 byte length of a session identifier or a turn id, as in
/// NIP-CST.
pub const MAX_CHECKPOINT_IDENTIFIER_BYTES: usize = 512;
/// Maximum number of files one checkpoint lists; the rest are counted in
/// `filesNotListed`.
pub const MAX_CHECKPOINT_FILES: usize = 256;
/// Maximum number of omitted paths one checkpoint names.
pub const MAX_CHECKPOINT_OMITTED: usize = 32;
/// Maximum UTF-8 byte length of the `unavailable` sentence.
pub const MAX_CHECKPOINT_UNAVAILABLE_SENTENCE_BYTES: usize = 512;
/// Maximum UTF-8 byte length of one repo-relative path. A longer path is not
/// listed; the producer counts it in `filesNotListed`.
pub const MAX_CHECKPOINT_PATH_BYTES: usize = 1024;
/// Maximum UTF-8 byte length of a branch name.
pub const MAX_CHECKPOINT_BRANCH_BYTES: usize = 512;
/// The largest integer every JSON reader represents exactly (2^53 − 1).
pub const MAX_CHECKPOINT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Substrings that mark a value the redaction pipeline already rewrote. A path
/// carrying one is refused: publishing it would either leak around the
/// redaction or publish a marker as if it were a file.
const REDACTION_MARKERS: &[&str] = &["[elided private context:", "••••••••", "[redacted"];

/// Why a checkpoint was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingSessionCheckpointReason {
    /// The end of one turn. `turnId` is required.
    Turn,
    /// Captured immediately before a rewind touches anything, so every rewind
    /// is itself undoable. `turnId` may be `null`.
    PreRewind,
}

impl CodingSessionCheckpointReason {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::PreRewind => "pre_rewind",
        }
    }
}

/// The transcript range one checkpoint covers, in the generation's own 44225
/// `eventSeq` clock: the turn's `user_prompt` through its terminal result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionCheckpointCoverage {
    /// First covered transcript seq.
    pub from_seq: u64,
    /// Last covered transcript seq; also the `csck-seq` tag.
    pub through_seq: u64,
}

/// Why a path was left out of the captured tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingSessionCheckpointOmissionReason {
    /// Larger than the capture's per-file cap.
    TooLarge,
    /// The capture could not read it.
    Unreadable,
}

/// One path the capture omitted, named so the omission is visible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionCheckpointOmission {
    /// Repo-relative path.
    pub path: String,
    /// Why it is not in the tree.
    pub reason: CodingSessionCheckpointOmissionReason,
}

/// The git facts of one capture. Only object ids travel; the refs that keep
/// them alive stay on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionCheckpointGit {
    /// `HEAD` when the capture ran, or `null` on an unborn branch. This is the
    /// commit↔session link: a commit finds its session through it.
    pub head: Option<String>,
    /// Short branch name, or `null` when `HEAD` is detached.
    pub branch: Option<String>,
    /// Tree before the turn's prompt was sent, or `null` when the baseline
    /// capture did not finish in time.
    pub base_tree: Option<String>,
    /// Tree when the terminal result was written.
    pub tree: String,
    /// The checkpoint commit (this tree, parent `HEAD`), held by a host-local
    /// ref only.
    pub commit: String,
    /// Whether the previous checkpoint's tree differs from this `baseTree` —
    /// files changed between turns. `null` when there is nothing to compare.
    pub outside_turn: Option<bool>,
    /// Whether every path made it into the tree.
    pub complete: bool,
    /// The paths that did not, by name — at most
    /// [`MAX_CHECKPOINT_OMITTED`] of them.
    pub omitted: Vec<CodingSessionCheckpointOmission>,
    /// Omitted paths that are not named in `omitted`: those beyond the cap,
    /// and those whose path this NIP refuses to publish. `omitted.len()` plus
    /// this is the true number of paths left out of the tree.
    pub omitted_not_listed: u64,
}

/// How one file changed between `baseTree` and `tree`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodingSessionCheckpointFileStatus {
    /// New in `tree`.
    Added,
    /// Changed content or mode.
    Modified,
    /// Gone from `tree`.
    Deleted,
    /// Moved from `from`.
    Renamed,
}

impl CodingSessionCheckpointFileStatus {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
        }
    }
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionCheckpointFile {
    /// Repo-relative path in `tree` (for a deletion, in `baseTree`).
    pub path: String,
    /// What happened to it.
    pub status: CodingSessionCheckpointFileStatus,
    /// The previous path; non-null exactly when `status` is `renamed`.
    pub from: Option<String>,
    /// Added lines, or `null` when git could not count them (a binary file).
    pub additions: Option<u64>,
    /// Deleted lines, or `null` when git could not count them.
    pub deletions: Option<u64>,
}

/// The closed reasons a capture produced no git facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CodingSessionCheckpointUnavailableCode {
    /// The session's working directory is not inside a git repository.
    NotARepository,
    /// The host-Git boundary was not prepared for this execution.
    BoundaryUnprepared,
    /// The capture did not finish within its cap.
    TimedOut,
    /// A git command failed.
    GitFailed,
}

impl CodingSessionCheckpointUnavailableCode {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotARepository => "NOT_A_REPOSITORY",
            Self::BoundaryUnprepared => "BOUNDARY_UNPREPARED",
            Self::TimedOut => "TIMED_OUT",
            Self::GitFailed => "GIT_FAILED",
        }
    }
}

/// Why there are no git facts, in a code and one sentence a person can read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionCheckpointUnavailable {
    /// The closed reason.
    pub code: CodingSessionCheckpointUnavailableCode,
    /// One line, at most 512 bytes, naming no host path.
    pub sentence: String,
}

/// The exact v1 content of a kind 44231 event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionCheckpointPayload {
    /// Always [`CODING_SESSION_CHECKPOINT_SCHEMA`].
    pub schema: String,
    /// The exact generation this checkpoint belongs to.
    pub session: CodingSessionTarget,
    /// The turn, or `null` only for a `pre_rewind` capture.
    pub turn_id: Option<String>,
    /// Why it was captured.
    pub reason: CodingSessionCheckpointReason,
    /// The transcript range it covers.
    pub coverage: CodingSessionCheckpointCoverage,
    /// The git facts, or `null` exactly when `unavailable` is set.
    pub git: Option<CodingSessionCheckpointGit>,
    /// Changed files `baseTree`→`tree`, at most [`MAX_CHECKPOINT_FILES`].
    pub files: Vec<CodingSessionCheckpointFile>,
    /// Changed files not listed in `files`.
    pub files_not_listed: u64,
    /// Whether the provider that captured this can rewind to it. `false` until
    /// a provider implements `session.rewind`.
    pub restorable: bool,
    /// Why there are no git facts, or `null` exactly when `git` is set.
    pub unavailable: Option<CodingSessionCheckpointUnavailable>,
    /// Reserved; must be `null` in v1.
    pub summary: Option<String>,
}

impl CodingSessionCheckpointPayload {
    /// Validate every bound and cross-field rule the wire contract states.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_CHECKPOINT_SCHEMA {
            return Err("unsupported coding-session checkpoint schema".into());
        }
        validate_target(&self.session)?;
        match (&self.turn_id, self.reason) {
            (Some(turn_id), _) => validate_identifier("turnId", turn_id)?,
            (None, CodingSessionCheckpointReason::PreRewind) => {}
            (None, CodingSessionCheckpointReason::Turn) => {
                return Err(
                    "checkpoint turnId must be present when reason is turn: only a pre_rewind \
                     capture may name no turn"
                        .into(),
                )
            }
        }
        validate_coverage(&self.coverage)?;
        if self.files_not_listed > MAX_CHECKPOINT_SAFE_INTEGER {
            return Err("checkpoint filesNotListed must be a safe integer".into());
        }
        if self.summary.is_some() {
            return Err("checkpoint summary must be null in v1".into());
        }
        match (&self.git, &self.unavailable) {
            (Some(git), None) => {
                validate_git(git)?;
                validate_files(&self.files)?;
            }
            (None, Some(unavailable)) => {
                validate_single_line(
                    "checkpoint unavailable.sentence",
                    &unavailable.sentence,
                    MAX_CHECKPOINT_UNAVAILABLE_SENTENCE_BYTES,
                )?;
                if !self.files.is_empty() || self.files_not_listed != 0 {
                    return Err(
                        "checkpoint files must be empty and filesNotListed 0 when git is \
                         unavailable: nothing was measured"
                            .into(),
                    );
                }
            }
            (Some(_), Some(_)) => {
                return Err(
                    "checkpoint git and unavailable are both set: exactly one is non-null".into(),
                )
            }
            (None, None) => {
                return Err(
                    "checkpoint git and unavailable are both null: exactly one is non-null".into(),
                )
            }
        }
        Ok(())
    }
}

/// The `csck-key` tag value: the identity of one checkpoint, length-prefixed
/// exactly as `cst-key` is.
///
/// The reason is part of the identity. A `pre_rewind` capture's coverage ends
/// at the last seq the rewound generation wrote — normally the last turn's
/// terminal result, so the same `throughSeq` as that turn's checkpoint.
/// Without the reason the two would share a key, and every reader would drop
/// the `pre_rewind` as a duplicate, and with it the undo of the rewind.
///
/// The rewind command is not part of the key, so two `pre_rewind` captures of
/// one generation must not share a `throughSeq` (NIP-CSCK § `pre_rewind`
/// groundwork): a provider advances the transcript past a failed rewind before
/// it captures again. Their host-local ref leaves are command-keyed and never
/// collide either way.
pub fn coding_session_checkpoint_semantic_key(
    target: &CodingSessionTarget,
    reason: CodingSessionCheckpointReason,
    through_seq: u64,
) -> String {
    let generation = target.generation.to_string();
    let seq = through_seq.to_string();
    let fields = [
        target.driver.as_str(),
        target.instance_id.as_str(),
        target.session_id.as_str(),
        generation.as_str(),
        seq.as_str(),
        reason.as_str(),
    ];
    let mut key = String::from(CODING_SESSION_CHECKPOINT_KEY_DOMAIN);
    key.push('|');
    for field in fields {
        key.push_str(&field.len().to_string());
        key.push(':');
        key.push_str(field);
    }
    key
}

/// The exact ordered tag values of a checkpoint in `channel_id`: `h`,
/// `csck-v`, `cs-target`, `csck-seq`, `csck-key`.
pub fn coding_session_checkpoint_tags(
    channel_id: &Uuid,
    payload: &CodingSessionCheckpointPayload,
) -> [[String; 2]; 5] {
    [
        ["h".to_owned(), channel_id.to_string()],
        [
            "csck-v".to_owned(),
            CODING_SESSION_CHECKPOINT_TAG_VERSION.to_owned(),
        ],
        [
            "cs-target".to_owned(),
            coding_session_target_key(&payload.session),
        ],
        [
            "csck-seq".to_owned(),
            payload.coverage.through_seq.to_string(),
        ],
        [
            "csck-key".to_owned(),
            coding_session_checkpoint_semantic_key(
                &payload.session,
                payload.reason,
                payload.coverage.through_seq,
            ),
        ],
    ]
}

/// Validate and serialize a payload into signed-content bytes.
///
/// Everything the decoder refuses, this refuses first, so no provider ever
/// signs bytes the relay and every reader would reject.
pub fn encode_coding_session_checkpoint(
    payload: &CodingSessionCheckpointPayload,
) -> Result<String, String> {
    payload.validate()?;
    let content = serde_json::to_string(payload)
        .map_err(|error| format!("coding-session checkpoint serialization failed: {error}"))?;
    decode_coding_session_checkpoint(&content)?;
    Ok(content)
}

/// Strictly decode and validate kind 44231 content.
pub fn decode_coding_session_checkpoint(
    content: &str,
) -> Result<CodingSessionCheckpointPayload, String> {
    if content.len() > MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES {
        return Err(format!(
            "coding-session checkpoint content exceeds \
             {MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session checkpoint payload: {error}"))?;
    validate_shape(&value)?;
    // A second, typed decode keeps serde's duplicate-key detection, which the
    // `Value` map above cannot represent — the reason 44244 and 44246 decode
    // twice too.
    let payload: CodingSessionCheckpointPayload = serde_json::from_str(content)
        .map_err(|error| format!("malformed coding-session checkpoint payload: {error}"))?;
    payload.validate()?;
    Ok(payload)
}

/// Validate a kind 44231 event's exact envelope and return its payload.
///
/// Structure only, for the relay: kind, content, and five two-field tags in
/// order whose values are re-derived from the content. It does not check the
/// signer.
pub fn validate_coding_session_checkpoint_event(
    event: &Event,
) -> Result<CodingSessionCheckpointPayload, String> {
    if event.kind.as_u16() as u32 != KIND_CODING_SESSION_CHECKPOINT {
        return Err("coding-session checkpoint has the wrong event kind".into());
    }
    let payload = decode_coding_session_checkpoint(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 5 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session checkpoint requires exactly five two-field tags".into());
    }
    if tags[0][0] != "h" {
        return Err("coding-session checkpoint first tag must be h=channel UUID".into());
    }
    let channel = Uuid::parse_str(&tags[0][1])
        .map_err(|_| "coding-session checkpoint h tag must be a UUID".to_owned())?;
    if channel.to_string() != tags[0][1] {
        return Err("coding-session checkpoint h tag must be a lowercase canonical UUID".into());
    }
    let expected = coding_session_checkpoint_tags(&channel, &payload);
    for (index, (actual, expected)) in tags.iter().zip(expected.iter()).enumerate().skip(1) {
        if actual[0] != expected[0] {
            return Err(format!(
                "coding-session checkpoint tag {} must be {:?}",
                index + 1,
                expected[0]
            ));
        }
        if actual[1] != expected[1] {
            return Err(format!(
                "coding-session checkpoint {} tag does not match the payload",
                expected[0]
            ));
        }
    }
    Ok(payload)
}

/// Decode a kind 44231 event for a reader (the CLI, the provider): the same
/// checks as [`validate_coding_session_checkpoint_event`].
pub fn decode_coding_session_checkpoint_event(
    event: &Event,
) -> Result<CodingSessionCheckpointPayload, String> {
    validate_coding_session_checkpoint_event(event)
}

/// Whether `path` is a repo-relative path a checkpoint may carry. Producers
/// call this before listing a path; a path it refuses is counted in
/// `filesNotListed` instead.
pub fn is_publishable_checkpoint_path(path: &str) -> bool {
    validate_path("path", path).is_ok()
}

// ── Exact JSON shape ────────────────────────────────────────────────────────

const PAYLOAD_KEYS: &[&str] = &[
    "schema",
    "session",
    "turnId",
    "reason",
    "coverage",
    "git",
    "files",
    "filesNotListed",
    "restorable",
    "unavailable",
    "summary",
];
const PAYLOAD_NULLABLE: &[&str] = &["turnId", "git", "unavailable", "summary"];
const SESSION_KEYS: &[&str] = &["driver", "instanceId", "sessionId", "generation"];
const COVERAGE_KEYS: &[&str] = &["fromSeq", "throughSeq"];
const GIT_KEYS: &[&str] = &[
    "head",
    "branch",
    "baseTree",
    "tree",
    "commit",
    "outsideTurn",
    "complete",
    "omitted",
    "omittedNotListed",
];
const GIT_NULLABLE: &[&str] = &["head", "branch", "baseTree", "outsideTurn"];
const OMISSION_KEYS: &[&str] = &["path", "reason"];
const FILE_KEYS: &[&str] = &["path", "status", "from", "additions", "deletions"];
const FILE_NULLABLE: &[&str] = &["from", "additions", "deletions"];
const UNAVAILABLE_KEYS: &[&str] = &["code", "sentence"];

const REASONS: &[&str] = &["turn", "pre_rewind"];
const OMISSION_REASONS: &[&str] = &["too_large", "unreadable"];
const FILE_STATUSES: &[&str] = &["added", "modified", "deleted", "renamed"];
const UNAVAILABLE_CODES: &[&str] = &[
    "NOT_A_REPOSITORY",
    "BOUNDARY_UNPREPARED",
    "TIMED_OUT",
    "GIT_FAILED",
];

/// Walk the `Value` tree checking exact keys and required non-nulls at every
/// level, and closed tokens by name, before serde sees it.
fn validate_shape(value: &Value) -> Result<(), String> {
    let payload = as_object(value, "payload")?;
    exact(payload, PAYLOAD_KEYS, PAYLOAD_NULLABLE, "payload")?;
    if !payload.get("summary").is_some_and(Value::is_null) {
        return Err("coding-session checkpoint summary must be null in v1".into());
    }
    closed_token(payload, "reason", REASONS, "payload")?;
    let session = as_object(field(payload, "session")?, "session")?;
    exact(session, SESSION_KEYS, &[], "session")?;
    let coverage = as_object(field(payload, "coverage")?, "coverage")?;
    exact(coverage, COVERAGE_KEYS, &[], "coverage")?;

    if let Some(git) = payload.get("git").filter(|git| !git.is_null()) {
        let git = as_object(git, "git")?;
        exact(git, GIT_KEYS, GIT_NULLABLE, "git")?;
        let omitted = as_array(field(git, "omitted")?, "git.omitted")?;
        if omitted.len() > MAX_CHECKPOINT_OMITTED {
            return Err(too_many(
                "git.omitted",
                MAX_CHECKPOINT_OMITTED,
                omitted.len(),
            ));
        }
        for entry in omitted {
            let entry = as_object(entry, "git.omitted entry")?;
            exact(entry, OMISSION_KEYS, &[], "git.omitted entry")?;
            closed_token(entry, "reason", OMISSION_REASONS, "git.omitted entry")?;
        }
    }

    let files = as_array(field(payload, "files")?, "files")?;
    if files.len() > MAX_CHECKPOINT_FILES {
        return Err(too_many("files", MAX_CHECKPOINT_FILES, files.len()));
    }
    for entry in files {
        let entry = as_object(entry, "files entry")?;
        exact(entry, FILE_KEYS, FILE_NULLABLE, "files entry")?;
        closed_token(entry, "status", FILE_STATUSES, "files entry")?;
    }

    if let Some(unavailable) = payload.get("unavailable").filter(|value| !value.is_null()) {
        let unavailable = as_object(unavailable, "unavailable")?;
        exact(unavailable, UNAVAILABLE_KEYS, &[], "unavailable")?;
        closed_token(unavailable, "code", UNAVAILABLE_CODES, "unavailable")?;
    }
    Ok(())
}

fn as_object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("coding-session checkpoint {what} must be an object"))
}

fn as_array<'a>(value: &'a Value, what: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("coding-session checkpoint {what} must be an array"))
}

fn field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    object
        .get(key)
        .ok_or_else(|| format!("coding-session checkpoint is missing {key:?}"))
}

fn too_many(what: &str, max: usize, got: usize) -> String {
    format!("coding-session checkpoint {what} exceeds {max} entries (got {got})")
}

/// Every key present, no unknown key, and `null` only where it is allowed.
fn exact(
    object: &Map<String, Value>,
    keys: &[&str],
    nullable: &[&str],
    what: &str,
) -> Result<(), String> {
    for key in keys {
        match object.get(*key) {
            None => {
                return Err(format!(
                    "coding-session checkpoint {what} is missing {key:?}: every key is always \
                     present, and an unset optional is written as JSON null rather than omitted"
                ))
            }
            Some(Value::Null) if !nullable.contains(key) => {
                return Err(format!(
                    "coding-session checkpoint {what} field {key:?} requires a value: null is \
                     not a value here, and absent is not null"
                ))
            }
            Some(_) => {}
        }
    }
    for key in object.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(format!(
                "coding-session checkpoint {what} carries unsupported field {key:?}: v1 rejects \
                 unknown fields rather than ignoring them"
            ));
        }
    }
    Ok(())
}

fn closed_token(
    object: &Map<String, Value>,
    key: &str,
    allowed: &[&str],
    what: &str,
) -> Result<(), String> {
    let value = object.get(key).and_then(Value::as_str).ok_or_else(|| {
        format!("coding-session checkpoint {what} field {key:?} must be a string")
    })?;
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(format!(
        "coding-session checkpoint {what} field {key:?} carries unsupported token {value:?}: v1 \
         knows exactly {}",
        allowed
            .iter()
            .map(|token| format!("{token:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

// ── Typed rules ─────────────────────────────────────────────────────────────

fn validate_target(target: &CodingSessionTarget) -> Result<(), String> {
    validate_identifier("session.driver", &target.driver)?;
    validate_identifier("session.instanceId", &target.instance_id)?;
    validate_identifier("session.sessionId", &target.session_id)?;
    if target.generation == 0 || target.generation > MAX_CHECKPOINT_SAFE_INTEGER {
        return Err("checkpoint session.generation must be a positive safe integer".into());
    }
    Ok(())
}

fn validate_identifier(what: &str, value: &str) -> Result<(), String> {
    validate_single_line(
        &format!("checkpoint {what}"),
        value,
        MAX_CHECKPOINT_IDENTIFIER_BYTES,
    )
}

fn validate_coverage(coverage: &CodingSessionCheckpointCoverage) -> Result<(), String> {
    for (what, seq) in [
        ("fromSeq", coverage.from_seq),
        ("throughSeq", coverage.through_seq),
    ] {
        if seq == 0 || seq > MAX_CHECKPOINT_SAFE_INTEGER {
            return Err(format!(
                "checkpoint coverage.{what} must be a positive safe integer"
            ));
        }
    }
    if coverage.from_seq > coverage.through_seq {
        return Err("checkpoint coverage.fromSeq must not exceed throughSeq".into());
    }
    Ok(())
}

fn validate_git(git: &CodingSessionCheckpointGit) -> Result<(), String> {
    validate_oid("git.tree", &git.tree)?;
    validate_oid("git.commit", &git.commit)?;
    if let Some(head) = &git.head {
        validate_oid("git.head", head)?;
    }
    if let Some(base_tree) = &git.base_tree {
        validate_oid("git.baseTree", base_tree)?;
    }
    // One repository has one hash algorithm, so every object id it names has
    // one length; a mix means the ids came from two places.
    let width = git.tree.len();
    let mixed = [Some(&git.commit), git.head.as_ref(), git.base_tree.as_ref()]
        .into_iter()
        .flatten()
        .any(|oid| oid.len() != width);
    if mixed {
        return Err("checkpoint git object ids must all be SHA-1 or all SHA-256".into());
    }
    if let Some(branch) = &git.branch {
        validate_single_line("checkpoint git.branch", branch, MAX_CHECKPOINT_BRANCH_BYTES)?;
        if branch.starts_with("refs/") {
            return Err("checkpoint git.branch must be a short branch name, not a ref name".into());
        }
    }
    if git.omitted.len() > MAX_CHECKPOINT_OMITTED {
        return Err(too_many(
            "git.omitted",
            MAX_CHECKPOINT_OMITTED,
            git.omitted.len(),
        ));
    }
    for (index, omission) in git.omitted.iter().enumerate() {
        validate_path("git.omitted path", &omission.path)?;
        if git.omitted[..index]
            .iter()
            .any(|held| held.path == omission.path)
        {
            return Err("checkpoint git.omitted names one path twice".into());
        }
    }
    if git.omitted_not_listed > MAX_CHECKPOINT_SAFE_INTEGER {
        return Err("checkpoint git.omittedNotListed must be a safe integer".into());
    }
    if !git.omitted.is_empty() && git.complete {
        return Err("checkpoint git.complete must be false when git.omitted names a path".into());
    }
    if git.omitted_not_listed > 0 && git.complete {
        return Err(
            "checkpoint git.complete must be false when git.omittedNotListed counts a path".into(),
        );
    }
    Ok(())
}

fn validate_files(files: &[CodingSessionCheckpointFile]) -> Result<(), String> {
    if files.len() > MAX_CHECKPOINT_FILES {
        return Err(too_many("files", MAX_CHECKPOINT_FILES, files.len()));
    }
    let mut seen = std::collections::HashSet::with_capacity(files.len());
    for file in files {
        validate_path("files path", &file.path)?;
        if !seen.insert(file.path.as_str()) {
            return Err("checkpoint files names one path twice".into());
        }
        match (file.status, &file.from) {
            (CodingSessionCheckpointFileStatus::Renamed, Some(from)) => {
                validate_path("files from", from)?;
                if from == &file.path {
                    return Err("checkpoint files from must differ from path".into());
                }
            }
            (CodingSessionCheckpointFileStatus::Renamed, None) => {
                return Err("checkpoint files from must be present when status is renamed".into())
            }
            (_, Some(_)) => {
                return Err("checkpoint files from must be null unless status is renamed".into())
            }
            (_, None) => {}
        }
        for (what, count) in [("additions", file.additions), ("deletions", file.deletions)] {
            if count.is_some_and(|count| count > MAX_CHECKPOINT_SAFE_INTEGER) {
                return Err(format!("checkpoint files {what} must be a safe integer"));
            }
        }
    }
    Ok(())
}

/// A lowercase 40-hex (SHA-1) or 64-hex (SHA-256) git object id — the shape
/// kinds 44244 and 44246 already write.
fn validate_oid(what: &str, value: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "checkpoint {what} must be a lowercase 40- or 64-hex git object id"
        ));
    }
    Ok(())
}

/// A repo-relative path in canonical form: `/`-separated, no empty, `.` or
/// `..` segment, no leading `/`, `\` or drive letter, no control character
/// (NUL included), not a ref name, and no redaction marker.
fn validate_path(what: &str, path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err(format!("checkpoint {what} must not be empty"));
    }
    if path.len() > MAX_CHECKPOINT_PATH_BYTES {
        return Err(format!(
            "checkpoint {what} exceeds {MAX_CHECKPOINT_PATH_BYTES} bytes"
        ));
    }
    if path.chars().any(char::is_control) {
        return Err(format!(
            "checkpoint {what} must not contain a NUL or other control character"
        ));
    }
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.starts_with('/') || path.starts_with('\\') || drive {
        return Err(format!(
            "checkpoint {what} must be repo-relative, never an absolute path"
        ));
    }
    if path.starts_with("refs/") {
        return Err(format!(
            "checkpoint {what} must be a file path, never a ref name"
        ));
    }
    if REDACTION_MARKERS.iter().any(|marker| path.contains(marker)) {
        return Err(format!(
            "checkpoint {what} carries a redaction marker: a redacted path is not published"
        ));
    }
    for segment in path.split(['/', '\\']) {
        match segment {
            ".." => {
                return Err(format!(
                    "checkpoint {what} must not contain a \"..\" segment"
                ))
            }
            "" | "." => {
                return Err(format!(
                    "checkpoint {what} must be canonical: no empty or \".\" segment"
                ))
            }
            _ => {}
        }
    }
    Ok(())
}

/// Bounded, non-blank, single-line text.
fn validate_single_line(what: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{what} must not be blank"));
    }
    if value.len() > max {
        return Err(format!("{what} exceeds {max} bytes (got {})", value.len()));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{what} must not contain control characters"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "coding_session_checkpoint_tests.rs"]
mod tests;
