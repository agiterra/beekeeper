//! Project Pulse entries (kind 44240): explicit, human- or agent-authored
//! coordination claims scoped to one NIP-MP project.
//!
//! A Pulse entry says what its author *intends* — a plan, a milestone, a note,
//! a handoff, or a blocker. It never asserts an observed fact about a worktree;
//! observed facts stay in the coding-session kinds (44223 and friends) and are
//! joined to entries by the fold, not by this module.
//!
//! Entries are regular append-only events. Revision is expressed by
//! `supersedes`, which this module validates **syntactically only**: a signed
//! event's meaning must never depend on what one relay's local database happens
//! to contain (the rule already settled in
//! [`crate::coding_session_genesis`]), and a database probe here would turn
//! `POST /events` into an existence oracle for arbitrary event ids. The
//! supersession *fold* — same author, `(created_at, event id)` ordering,
//! single-pass marking — lives in the consumers and is pinned by
//! `conformance/project-pulse-fold/`.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coding_session_goal::validate_coding_session_goal_session_ref;
use crate::kind::{event_kind_u32, normalize_project_coordinate, KIND_PULSE_ENTRY};

/// Exact `schema` value carried by kind 44240 content.
pub const PULSE_ENTRY_SCHEMA: &str = "buzz-pulse-entry/v1";

/// Exact version carried by the `pu-v` tag.
pub const PULSE_ENTRY_TAG_VERSION: &str = "pu1-1";

/// Maximum UTF-8 byte length of a complete Pulse entry payload.
pub const MAX_PULSE_ENTRY_CONTENT_BYTES: usize = 16 * 1024;

/// Maximum UTF-8 byte length of an entry's prose.
pub const MAX_PULSE_TEXT_BYTES: usize = 4 * 1024;

/// Maximum number of claimed code areas on one entry.
pub const MAX_PULSE_CODE_AREAS: usize = 32;

/// Maximum UTF-8 byte length of one claimed code area.
pub const MAX_PULSE_CODE_AREA_BYTES: usize = 256;

/// Maximum UTF-8 byte length of a branch shortname, in either the `branch` tag
/// or the content field.
pub const MAX_PULSE_BRANCH_BYTES: usize = 256;

/// Maximum number of seats one entry's `cost` may account for.
pub const MAX_PULSE_COST_SEATS: usize = 64;

/// Maximum UTF-8 byte length of a cost seat's `role` or `model` label.
pub const MAX_PULSE_COST_LABEL_BYTES: usize = 256;

/// The closed content key set. Mirrors [`PulseEntry`]'s serde names; the
/// first decode pass checks against it so an unknown key is reported as such
/// rather than as a generic parse failure.
const PULSE_ENTRY_FIELDS: &[&str] = &[
    "schema",
    "type",
    "text",
    "codeAreas",
    "branch",
    "supersedes",
    "cost",
];

/// What an entry claims about its author's work.
///
/// The value is carried twice — in the `pu-type` tag (so the relay and any
/// index can read it without parsing content) and in the content `type` field
/// — and [`validate_pulse_entry_envelope`] requires the two to agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PulseEntryType {
    /// What the author intends to do next.
    Plan,
    /// Something the author finished.
    Milestone,
    /// Context a peer should know, with no claim on the work.
    Note,
    /// The author is handing work to someone else.
    Handoff,
    /// Something is stopping the author from proceeding.
    Blocker,
}

impl PulseEntryType {
    /// The wire spelling, identical in the `pu-type` tag and in content.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Milestone => "milestone",
            Self::Note => "note",
            Self::Handoff => "handoff",
            Self::Blocker => "blocker",
        }
    }
}

impl fmt::Display for PulseEntryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PulseEntryType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "plan" => Ok(Self::Plan),
            "milestone" => Ok(Self::Milestone),
            "note" => Ok(Self::Note),
            "handoff" => Ok(Self::Handoff),
            "blocker" => Ok(Self::Blocker),
            other => Err(format!(
                "pulse entry type must be one of plan|milestone|note|handoff|blocker (got {other:?})"
            )),
        }
    }
}

/// What one seat spent producing the work an entry claims.
///
/// Every field is optional and every unreported one is **omitted**, never
/// serialized as `null` or `0` — the same rule
/// [`crate::coding_session_payload::TurnUsageReport`] follows on the wire it
/// is folded from, and for the same reason: "the driver did not report it" and
/// "the driver measured zero" are different facts.
///
/// The four token counts are the provider's own per-turn numbers summed across
/// the seat's turns. `inputTokens`, `cacheReadTokens` and `cacheWriteTokens`
/// are disjoint prompt-side partitions (the `TurnUsageReport` convention), so
/// adding all four to `outputTokens` double-counts nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseCostSeat {
    /// The seat's agent pubkey, lowercase 64-hex, when the work ran as a seat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// The role slug the seat held, when one was published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The effective model, when the provider named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Fresh (uncached) prompt tokens, summed over the seat's turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Tokens the model produced, summed over the seat's turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Prompt tokens served from the provider's cache, summed over the turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    /// Prompt tokens written into the provider's cache, summed over the turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    /// Tool calls the seat opened, summed over the turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<u64>,
    /// Turns that carried a usage block. Never a count of turns the seat took
    /// — a turn whose provider reported nothing is not counted here, because
    /// this number exists to say how much of the cost was measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<u64>,
}

impl PulseCostSeat {
    /// Whether this seat reports nothing at all — neither an identity nor a
    /// number. Such a seat is a rejection, not a silent drop.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The seat's four token counts added together, or `None` when the seat
    /// reported none of them.
    ///
    /// Saturating on the (practically impossible) overflow of four `u64`
    /// counts: a clamped total is closer to the truth than a wrapped one.
    pub fn token_sum(&self) -> Option<u64> {
        let parts = [
            self.input_tokens,
            self.output_tokens,
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

/// What the work an entry claims cost, per seat.
///
/// The whole object is optional on [`PulseEntry`] and is **omitted** when
/// nothing on the wire measured the work. It is never published as an empty
/// object or as a set of zeros: a zero here would assert that a lane cost
/// nothing, which is a different claim from "nobody measured it".
///
/// `totalTokens` is defined as the sum of every listed seat's
/// [`PulseCostSeat::token_sum`]. When `seats` is non-empty the two must agree,
/// so a reader can add the seats up and get the headline back; a `totalTokens`
/// beside seats that do not add up to it is rejected rather than rendered.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseCost {
    /// One row per seat that contributed, in the publisher's chosen order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seats: Vec<PulseCostSeat>,
    /// Every listed seat's tokens added together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

impl PulseCost {
    /// Whether this cost reports nothing at all.
    pub fn is_empty(&self) -> bool {
        self.seats.is_empty() && self.total_tokens.is_none()
    }

    /// The sum of every listed seat's tokens, or `None` when no seat reported
    /// any token count.
    pub fn seat_token_sum(&self) -> Option<u64> {
        let sums: Vec<u64> = self
            .seats
            .iter()
            .filter_map(PulseCostSeat::token_sum)
            .collect();
        (!sums.is_empty()).then(|| {
            sums.into_iter()
                .fold(0u64, |total, part| total.saturating_add(part))
        })
    }
}

/// Strict public JSON carried by a Pulse entry (kind 44240).
///
/// `codeAreas`, `branch`, and `supersedes` are the three nullable optionals;
/// the canonical builder always emits those six keys (an empty array and
/// explicit `null`s), and a payload that omits one decodes to the same value
/// as one that spells it out. `cost` is the seventh key and is *omitted*
/// rather than nulled when absent. Unknown keys are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseEntry {
    /// Always [`PULSE_ENTRY_SCHEMA`].
    pub schema: String,
    /// The claim this entry makes; must equal the `pu-type` tag.
    #[serde(rename = "type")]
    pub entry_type: PulseEntryType,
    /// Author-written prose. Non-empty after trimming.
    pub text: String,
    /// Repository-relative paths the author claims to be working in.
    ///
    /// These are *claims*, never observed facts, and consumers must render
    /// them as such. Each path is normalized by stripping one leading `./`;
    /// duplicates (after that strip) are a rejection, not a silent merge.
    #[serde(default)]
    pub code_areas: Vec<String>,
    /// Branch the claim applies to, or `null`. Must match the `branch` tag
    /// when both are present.
    #[serde(default)]
    pub branch: Option<String>,
    /// Event id of an earlier entry this one revises, or `null`.
    ///
    /// Validated syntactically only — 64 lowercase hex characters that are not
    /// this event's own id. Whether the supersession is *honored* is a fold
    /// question (same author, `(created_at, event id)` ordering), answered by
    /// consumers against `conformance/project-pulse-fold/`.
    #[serde(default)]
    pub supersedes: Option<String>,
    /// What the work this entry claims cost, per seat, or `None`.
    ///
    /// The seventh key and the only one the canonical builder omits entirely
    /// when it is absent, so a costless entry is byte-identical to the shape
    /// that shipped before this field existed. Numbers here come from usage
    /// blocks the providers signed; an entry whose session published no usage
    /// omits `cost` rather than publishing zeros.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<PulseCost>,
}

/// Strictly decode and validate Pulse entry content.
///
/// The whole-content cap is checked **before** `serde_json::from_str`, and the
/// decode runs in two passes: the first reports an unknown key against the
/// closed field set, the second picks up serde's own duplicate-key detection
/// (the pattern used by every other buzz-core payload module — see
/// `coding_session_payload::decode_coding_session_metadata`).
///
/// The returned entry's `codeAreas` are normalized: a single leading `./` is
/// stripped from each path.
pub fn decode_pulse_entry(content: &str) -> Result<PulseEntry, String> {
    if content.len() > MAX_PULSE_ENTRY_CONTENT_BYTES {
        return Err(format!(
            "pulse entry content exceeds {MAX_PULSE_ENTRY_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value =
        serde_json::from_str(content).map_err(|_| "malformed pulse entry payload".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "pulse entry payload must be an object".to_owned())?;
    if let Some(unknown) = object
        .keys()
        .find(|key| !PULSE_ENTRY_FIELDS.contains(&key.as_str()))
    {
        return Err(format!(
            "pulse entry payload has unsupported field {unknown:?}"
        ));
    }
    let mut entry: PulseEntry = serde_json::from_str(content)
        .map_err(|err| format!("malformed pulse entry payload: {err}"))?;
    normalize_and_validate(&mut entry)?;
    Ok(entry)
}

/// Validate one claimed code area.
///
/// Code areas are repository-relative paths, never host paths. A single
/// leading `./` is stripped before the checks. Rejected: an empty path, a
/// leading `/` or `~`, a Windows drive prefix, a `\` separator, any `//`, any
/// `..` **substring** (the same substring rule the git manifest guard uses at
/// `crates/buzz-relay/src/api/git/manifest.rs:146` — a segment-only check is
/// not the same rule), a trailing `/`, any control character, and anything
/// over [`MAX_PULSE_CODE_AREA_BYTES`].
pub fn validate_code_area(path: &str) -> Result<(), String> {
    let path = strip_leading_dot_slash(path);
    if path.is_empty() {
        return Err("pulse code area must not be empty".to_owned());
    }
    if path.len() > MAX_PULSE_CODE_AREA_BYTES {
        return Err(format!(
            "pulse code area exceeds {MAX_PULSE_CODE_AREA_BYTES} bytes"
        ));
    }
    if path.starts_with('/') || path.starts_with('~') {
        return Err(format!(
            "pulse code area must be repository-relative (got {path:?})"
        ));
    }
    if has_drive_prefix(path) {
        return Err(format!(
            "pulse code area must not carry a drive prefix (got {path:?})"
        ));
    }
    if path.contains('\\') {
        return Err(format!(
            "pulse code area must use / separators (got {path:?})"
        ));
    }
    if path.contains("..") {
        return Err(format!(
            "pulse code area must not contain .. (got {path:?})"
        ));
    }
    if path.contains("//") || path.ends_with('/') {
        return Err(format!(
            "pulse code area must not contain an empty path segment (got {path:?})"
        ));
    }
    if path.chars().any(char::is_control) {
        return Err("pulse code area must not contain control characters".to_owned());
    }
    Ok(())
}

/// Validate a signed Pulse entry end to end: kind, tag grammar, canonical
/// project coordinate, and content envelope.
///
/// This is the single validator. The relay calls it and defines no local copy;
/// the buzz-sdk builder delegates to it rather than repeating the rules.
///
/// **Tag grammar** — position-independent, multiplicity-constrained, closed
/// key set. Nostr imposes no relative tag ordering and no relay enforces one,
/// so a positional validator would reject well-formed events from any client
/// that builds tags in a different order. Exactly one `a`, one `pu-v`, and one
/// `pu-type`; at most one each of `h`, `branch`, and `pu-session`; every tag
/// exactly two fields; any other key is a rejection.
pub fn validate_pulse_entry_envelope(event: &nostr::Event) -> Result<PulseEntry, String> {
    if event_kind_u32(event) != KIND_PULSE_ENTRY {
        return Err("event is not a pulse entry (kind 44240)".to_owned());
    }
    let entry = decode_pulse_entry(&event.content)?;

    let mut coordinate: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut tag_type: Option<&str> = None;
    let mut channel: Option<&str> = None;
    let mut tag_branch: Option<&str> = None;
    let mut session_ref: Option<&str> = None;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() != 2 {
            return Err("pulse entry tags must have exactly two fields".to_owned());
        }
        let key = parts[0].as_str();
        let slot = match key {
            "a" => &mut coordinate,
            "pu-v" => &mut version,
            "pu-type" => &mut tag_type,
            "h" => &mut channel,
            "branch" => &mut tag_branch,
            "pu-session" => &mut session_ref,
            other => return Err(format!("pulse entry has unsupported tag key {other:?}")),
        };
        if slot.is_some() {
            return Err(format!("pulse entry has more than one {key} tag"));
        }
        *slot = Some(parts[1].as_str());
    }

    let coordinate = coordinate.ok_or_else(|| "pulse entry requires one a tag".to_owned())?;
    if normalize_project_coordinate(coordinate).as_deref() != Some(coordinate) {
        return Err(
            "44240 `a` tag must be a canonical 30621:<lowercase-hex>:<dtag> coordinate".to_owned(),
        );
    }
    match version {
        Some(PULSE_ENTRY_TAG_VERSION) => {}
        _ => return Err("unsupported pulse entry tag version".to_owned()),
    }
    let tag_type = tag_type.ok_or_else(|| "pulse entry requires one pu-type tag".to_owned())?;
    if PulseEntryType::from_str(tag_type)? != entry.entry_type {
        return Err(format!(
            "pulse entry pu-type tag {tag_type:?} does not match content type {:?}",
            entry.entry_type.as_str()
        ));
    }
    if let Some(channel) = channel {
        // Canonical lowercase hyphenated form only, the same rule `pu-session`
        // rides two checks below. `Uuid::parse_str` also accepts uppercase,
        // 32-hex simple and braced forms; those are unreachable by any `#h`
        // relay filter (the SQL containment probe matches exact bytes) and the
        // TypeScript twin rejects them outright, so accepting them here would
        // fold one way in `bee pulse digest` and another in Desktop.
        match uuid::Uuid::parse_str(channel) {
            Ok(parsed) if parsed.to_string() == channel => {}
            _ => {
                return Err(
                    "pulse entry h tag must be a lowercase canonical channel UUID".to_owned(),
                )
            }
        }
    }
    if let Some(branch) = tag_branch {
        validate_branch(branch)?;
        if entry.branch.as_deref().is_some_and(|value| value != branch) {
            return Err("pulse entry branch tag does not match content branch".to_owned());
        }
    }
    if let Some(session_ref) = session_ref {
        validate_coding_session_goal_session_ref(session_ref)
            .map_err(|_| "pulse entry pu-session must be a lowercase canonical UUID".to_owned())?;
    }
    if entry
        .supersedes
        .as_deref()
        .is_some_and(|id| id == event.id.to_hex())
    {
        return Err("pulse entry must not supersede itself".to_owned());
    }
    Ok(entry)
}

/// The project coordinate a Pulse entry is scoped to, normalized to
/// `30621:<lowercase-hex>:<dtag>`.
///
/// Parsing is tolerant of hex case (like [`crate::kind::git_event_repo_names`])
/// so a smuggled case-variant coordinate cannot dodge the per-event read gate,
/// even though ingest requires the tag to already be canonical and the SQL
/// containment probe matches exact bytes. Returns `None` when the event does
/// not carry exactly one well-formed `a` tag — the gate closes, it does not
/// open.
pub fn pulse_entry_project_coordinate(event: &nostr::Event) -> Option<String> {
    let a = nostr::SingleLetterTag::lowercase(nostr::Alphabet::A);
    let mut values = event
        .tags
        .filter(nostr::TagKind::SingleLetter(a))
        .filter_map(|tag| tag.content());
    let first = values.next()?;
    if values.next().is_some() {
        return None;
    }
    normalize_project_coordinate(first)
}

/// Strip a single leading `./` — the one normalization a code area gets.
fn strip_leading_dot_slash(path: &str) -> &str {
    path.strip_prefix("./").unwrap_or(path)
}

/// `true` for a Windows drive prefix such as `C:\src` or `c:/src`.
fn has_drive_prefix(path: &str) -> bool {
    let mut chars = path.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}

/// Validate a branch shortname, in either the tag or the content field.
fn validate_branch(branch: &str) -> Result<(), String> {
    if branch.is_empty() {
        return Err("pulse entry branch must not be empty".to_owned());
    }
    if branch.len() > MAX_PULSE_BRANCH_BYTES {
        return Err(format!(
            "pulse entry branch exceeds {MAX_PULSE_BRANCH_BYTES} bytes"
        ));
    }
    if branch.chars().any(char::is_control) {
        return Err("pulse entry branch must not contain control characters".to_owned());
    }
    Ok(())
}

/// Validate every content field and normalize `codeAreas` in place.
fn normalize_and_validate(entry: &mut PulseEntry) -> Result<(), String> {
    if entry.schema != PULSE_ENTRY_SCHEMA {
        return Err(format!("unsupported pulse entry schema {:?}", entry.schema));
    }
    if entry.text.trim().is_empty() {
        return Err("pulse entry text must contain prose".to_owned());
    }
    if entry.text.len() > MAX_PULSE_TEXT_BYTES {
        return Err(format!(
            "pulse entry text exceeds {MAX_PULSE_TEXT_BYTES} bytes"
        ));
    }
    if entry.code_areas.len() > MAX_PULSE_CODE_AREAS {
        return Err(format!(
            "pulse entry claims more than {MAX_PULSE_CODE_AREAS} code areas"
        ));
    }
    let mut seen: HashSet<&str> = HashSet::with_capacity(entry.code_areas.len());
    let mut normalized = Vec::with_capacity(entry.code_areas.len());
    for area in &entry.code_areas {
        validate_code_area(area)?;
        normalized.push(strip_leading_dot_slash(area).to_owned());
    }
    for area in &normalized {
        if !seen.insert(area.as_str()) {
            return Err(format!("pulse entry repeats code area {area:?}"));
        }
    }
    entry.code_areas = normalized;
    if let Some(branch) = entry.branch.as_deref() {
        validate_branch(branch)?;
    }
    if let Some(supersedes) = entry.supersedes.as_deref() {
        if !is_lowercase_hex64(supersedes) {
            return Err(
                "pulse entry supersedes must be a 64-character lowercase hex event id".to_owned(),
            );
        }
    }
    if let Some(cost) = entry.cost.as_ref() {
        validate_cost(cost)?;
    }
    Ok(())
}

/// `true` for exactly 64 lowercase hex characters.
fn is_lowercase_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Validate one cost seat label (`role` or `model`).
fn validate_cost_label(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("pulse cost seat {field} must not be blank"));
    }
    if value.len() > MAX_PULSE_COST_LABEL_BYTES {
        return Err(format!(
            "pulse cost seat {field} exceeds {MAX_PULSE_COST_LABEL_BYTES} bytes"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "pulse cost seat {field} must not contain control characters"
        ));
    }
    Ok(())
}

/// Validate an entry's `cost`.
///
/// Four rules, all of them honesty rules:
///
/// 1. A `cost` that reports nothing is a rejection — a costless entry omits
///    the key rather than publishing an empty object.
/// 2. A seat that reports nothing is a rejection, for the same reason.
/// 3. A seat is named once. The same `actor` twice would double-count the
///    lane, and silently merging the rows would hide that it happened.
/// 4. `totalTokens` must equal the seats it sits beside (see [`PulseCost`]),
///    including the case where no seat reported a token count and there is
///    therefore no total to state.
fn validate_cost(cost: &PulseCost) -> Result<(), String> {
    if cost.is_empty() {
        return Err("pulse entry cost must report something".to_owned());
    }
    if cost.seats.len() > MAX_PULSE_COST_SEATS {
        return Err(format!(
            "pulse entry cost lists more than {MAX_PULSE_COST_SEATS} seats"
        ));
    }
    let mut seen: HashSet<&str> = HashSet::with_capacity(cost.seats.len());
    for seat in &cost.seats {
        if seat.is_empty() {
            return Err("pulse entry cost seat must report something".to_owned());
        }
        if let Some(actor) = seat.actor.as_deref() {
            if !is_lowercase_hex64(actor) {
                return Err(
                    "pulse cost seat actor must be a 64-character lowercase hex pubkey".to_owned(),
                );
            }
            if !seen.insert(actor) {
                return Err(format!("pulse entry cost repeats cost seat {actor:?}"));
            }
        }
        if let Some(role) = seat.role.as_deref() {
            validate_cost_label("role", role)?;
        }
        if let Some(model) = seat.model.as_deref() {
            validate_cost_label("model", model)?;
        }
    }
    if !cost.seats.is_empty() {
        let summed = cost.seat_token_sum();
        if cost.total_tokens.is_some() && cost.total_tokens != summed {
            let total = cost.total_tokens.unwrap_or_default();
            return Err(match summed {
                Some(summed) => format!(
                    "pulse entry cost totalTokens {total} does not equal the {summed} its seats report"
                ),
                None => format!(
                    "pulse entry cost totalTokens {total} does not equal the seats it lists, which report no tokens at all"
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const COORD: &str =
        "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:platform";
    const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
    const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    const OTHER_ID: &str = "bb00000000000000000000000000000000000000000000000000000000000011";

    fn content_with(fields: &str) -> String {
        format!(
            r#"{{"schema":"{PULSE_ENTRY_SCHEMA}","type":"plan","text":"refactoring pool.rs"{fields}}}"#
        )
    }

    fn valid_content() -> String {
        content_with(
            r#","codeAreas":["crates/buzz-acp/src/pool.rs"],"branch":null,"supersedes":null"#,
        )
    }

    fn event_with(content: &str, tags: &[&[&str]]) -> nostr::Event {
        let keys = Keys::generate();
        let tag_vec: Vec<Tag> = tags
            .iter()
            .map(|parts| Tag::parse(parts.iter().copied()).expect("test tag parses"))
            .collect();
        EventBuilder::new(Kind::Custom(KIND_PULSE_ENTRY as u16), content.to_owned())
            .tags(tag_vec)
            .sign_with_keys(&keys)
            .expect("test event signs")
    }

    fn valid_event() -> nostr::Event {
        event_with(
            &valid_content(),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
            ],
        )
    }

    // ── content ──────────────────────────────────────────────────────────

    #[test]
    fn accepts_every_entry_type() {
        for (wire, expected) in [
            ("plan", PulseEntryType::Plan),
            ("milestone", PulseEntryType::Milestone),
            ("note", PulseEntryType::Note),
            ("handoff", PulseEntryType::Handoff),
            ("blocker", PulseEntryType::Blocker),
        ] {
            let content = format!(
                r#"{{"schema":"{PULSE_ENTRY_SCHEMA}","type":"{wire}","text":"t","codeAreas":[],"branch":null,"supersedes":null}}"#
            );
            let entry = decode_pulse_entry(&content).expect("valid type decodes");
            assert_eq!(entry.entry_type, expected);
            assert_eq!(expected.as_str(), wire);
            assert_eq!(PulseEntryType::from_str(wire), Ok(expected));
        }
        assert!(PulseEntryType::from_str("PLAN").is_err());
    }

    #[test]
    fn optional_fields_may_be_omitted_but_unknown_fields_fail() {
        let entry = decode_pulse_entry(&content_with("")).expect("optionals may be omitted");
        assert!(entry.code_areas.is_empty());
        assert_eq!(entry.branch, None);
        assert_eq!(entry.supersedes, None);

        let unknown = content_with(r#","priority":"high""#);
        assert!(decode_pulse_entry(&unknown)
            .unwrap_err()
            .contains("unsupported field"));
    }

    #[test]
    fn rejects_wrong_schema_and_non_object_and_duplicate_keys() {
        let wrong = valid_content().replace(PULSE_ENTRY_SCHEMA, "buzz-pulse-entry/v2");
        assert!(decode_pulse_entry(&wrong)
            .unwrap_err()
            .contains("unsupported pulse entry schema"));
        assert!(decode_pulse_entry("[]").is_err());
        assert!(decode_pulse_entry("not json").is_err());

        let duplicated =
            format!(r#"{{"schema":"{PULSE_ENTRY_SCHEMA}","type":"plan","text":"a","text":"b"}}"#);
        assert!(decode_pulse_entry(&duplicated)
            .unwrap_err()
            .contains("malformed"));
    }

    #[test]
    fn rejects_blank_text() {
        let blank = content_with("").replace("refactoring pool.rs", "   ");
        assert!(decode_pulse_entry(&blank)
            .unwrap_err()
            .contains("must contain prose"));
    }

    // ── code areas ───────────────────────────────────────────────────────

    #[test]
    fn rejects_every_unsafe_code_area_shape() {
        for path in [
            "",
            "/etc/passwd",
            "~/secrets",
            "../../etc/passwd",
            "crates/../../etc",
            "crates/..",
            "C:\\src\\main.rs",
            "c:/src/main.rs",
            "crates\\buzz-core\\src",
            "crates//buzz-core",
            "crates/buzz-core/",
            "crates/buzz\0core",
            "crates/buzz\ncore",
            "./",
        ] {
            assert!(
                validate_code_area(path).is_err(),
                "expected {path:?} to be rejected"
            );
        }
    }

    #[test]
    fn strips_one_leading_dot_slash() {
        assert!(validate_code_area("./crates/buzz-core/src/pulse.rs").is_ok());
        let content = content_with(r#","codeAreas":["./crates/buzz-core/src/pulse.rs"]"#);
        let entry = decode_pulse_entry(&content).expect("leading ./ is stripped, not rejected");
        assert_eq!(entry.code_areas, vec!["crates/buzz-core/src/pulse.rs"]);
        // Only one `./` is stripped; the second leaves a `..`-free but
        // still-relative path, so `.//x` fails on the empty segment.
        assert!(validate_code_area(".//x").is_err());
    }

    #[test]
    fn rejects_duplicate_code_areas_including_after_normalization() {
        let content = content_with(r#","codeAreas":["a/b.rs","a/b.rs"]"#);
        assert!(decode_pulse_entry(&content)
            .unwrap_err()
            .contains("repeats code area"));
        let normalized = content_with(r#","codeAreas":["./a/b.rs","a/b.rs"]"#);
        assert!(decode_pulse_entry(&normalized)
            .unwrap_err()
            .contains("repeats code area"));
    }

    // ── caps, each at its exact boundary ─────────────────────────────────

    #[test]
    fn content_cap_boundary() {
        let overhead = content_with(r#","codeAreas":[],"branch":null,"supersedes":null"#)
            .replace("refactoring pool.rs", "");
        let room = MAX_PULSE_ENTRY_CONTENT_BYTES - overhead.len();
        let at = content_with(r#","codeAreas":[],"branch":null,"supersedes":null"#)
            .replace("refactoring pool.rs", &"x".repeat(room));
        assert_eq!(at.len(), MAX_PULSE_ENTRY_CONTENT_BYTES);
        // At the cap the content is accepted by the size gate; it fails only
        // on the text cap, which is the smaller of the two.
        assert!(!decode_pulse_entry(&at)
            .unwrap_err()
            .contains("content exceeds"));
        let over = at.replace("\"text\":\"", "\"text\":\"y");
        assert_eq!(over.len(), MAX_PULSE_ENTRY_CONTENT_BYTES + 1);
        assert!(decode_pulse_entry(&over)
            .unwrap_err()
            .contains("content exceeds"));
    }

    #[test]
    fn text_cap_boundary() {
        let at = content_with("").replace("refactoring pool.rs", &"x".repeat(MAX_PULSE_TEXT_BYTES));
        assert!(decode_pulse_entry(&at).is_ok());
        let over =
            content_with("").replace("refactoring pool.rs", &"x".repeat(MAX_PULSE_TEXT_BYTES + 1));
        assert!(decode_pulse_entry(&over)
            .unwrap_err()
            .contains("text exceeds"));
    }

    #[test]
    fn code_area_count_and_size_boundaries() {
        let areas = |count: usize| {
            let list: Vec<String> = (0..count).map(|i| format!("\"a/{i}.rs\"")).collect();
            content_with(&format!(r#","codeAreas":[{}]"#, list.join(",")))
        };
        assert!(decode_pulse_entry(&areas(MAX_PULSE_CODE_AREAS)).is_ok());
        assert!(decode_pulse_entry(&areas(MAX_PULSE_CODE_AREAS + 1))
            .unwrap_err()
            .contains("more than"));

        assert!(validate_code_area(&"x".repeat(MAX_PULSE_CODE_AREA_BYTES)).is_ok());
        assert!(
            validate_code_area(&"x".repeat(MAX_PULSE_CODE_AREA_BYTES + 1))
                .unwrap_err()
                .contains("code area exceeds")
        );
    }

    #[test]
    fn branch_cap_boundary() {
        let at = content_with(&format!(
            r#","branch":"{}""#,
            "b".repeat(MAX_PULSE_BRANCH_BYTES)
        ));
        assert!(decode_pulse_entry(&at).is_ok());
        let over = content_with(&format!(
            r#","branch":"{}""#,
            "b".repeat(MAX_PULSE_BRANCH_BYTES + 1)
        ));
        assert!(decode_pulse_entry(&over)
            .unwrap_err()
            .contains("branch exceeds"));
    }

    // ── envelope ─────────────────────────────────────────────────────────

    #[test]
    fn accepts_a_well_formed_entry_in_any_tag_order() {
        let entry = validate_pulse_entry_envelope(&valid_event()).expect("valid entry");
        assert_eq!(entry.entry_type, PulseEntryType::Plan);

        let shuffled = event_with(
            &valid_content(),
            &[
                &["pu-type", "plan"],
                &["h", CHANNEL],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-session", SESSION_REF],
                &["a", COORD],
            ],
        );
        assert!(validate_pulse_entry_envelope(&shuffled).is_ok());
    }

    #[test]
    fn rejects_missing_duplicate_and_unknown_tags() {
        let missing_a = event_with(
            &valid_content(),
            &[&["pu-v", PULSE_ENTRY_TAG_VERSION], &["pu-type", "plan"]],
        );
        assert!(validate_pulse_entry_envelope(&missing_a)
            .unwrap_err()
            .contains("requires one a tag"));

        let duplicate_a = event_with(
            &valid_content(),
            &[
                &["a", COORD],
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
            ],
        );
        assert!(validate_pulse_entry_envelope(&duplicate_a)
            .unwrap_err()
            .contains("more than one a tag"));

        let duplicate_optional = event_with(
            &valid_content(),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
                &["h", CHANNEL],
                &["h", CHANNEL],
            ],
        );
        assert!(validate_pulse_entry_envelope(&duplicate_optional)
            .unwrap_err()
            .contains("more than one h tag"));

        let unknown = event_with(
            &valid_content(),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
                &["e", OTHER_ID],
            ],
        );
        assert!(validate_pulse_entry_envelope(&unknown)
            .unwrap_err()
            .contains("unsupported tag key"));

        let three_field = event_with(
            &valid_content(),
            &[
                &["a", COORD, "wss://relay"],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
            ],
        );
        assert!(validate_pulse_entry_envelope(&three_field)
            .unwrap_err()
            .contains("exactly two fields"));
    }

    #[test]
    fn rejects_non_canonical_coordinate() {
        let upper = format!("30621:{}:platform", OWNER.to_ascii_uppercase());
        let event = event_with(
            &valid_content(),
            &[
                &["a", &upper],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
            ],
        );
        assert!(validate_pulse_entry_envelope(&event)
            .unwrap_err()
            .contains("canonical"));
        // ...and the tolerant reader still resolves it, so a smuggled event
        // cannot dodge the read gate.
        assert_eq!(
            pulse_entry_project_coordinate(&event).as_deref(),
            Some(COORD)
        );
    }

    #[test]
    fn rejects_version_type_and_branch_mismatches() {
        let bad_version = event_with(
            &valid_content(),
            &[&["a", COORD], &["pu-v", "pu1-0"], &["pu-type", "plan"]],
        );
        assert!(validate_pulse_entry_envelope(&bad_version)
            .unwrap_err()
            .contains("unsupported pulse entry tag version"));

        let type_mismatch = event_with(
            &valid_content(),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "blocker"],
            ],
        );
        assert!(validate_pulse_entry_envelope(&type_mismatch)
            .unwrap_err()
            .contains("does not match content type"));

        let branch_mismatch = event_with(
            &content_with(r#","branch":"wip/a""#),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
                &["branch", "wip/b"],
            ],
        );
        assert!(validate_pulse_entry_envelope(&branch_mismatch)
            .unwrap_err()
            .contains("does not match content branch"));

        let branch_agrees = event_with(
            &content_with(r#","branch":"wip/a""#),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
                &["branch", "wip/a"],
            ],
        );
        assert!(validate_pulse_entry_envelope(&branch_agrees).is_ok());
    }

    #[test]
    fn rejects_malformed_channel_and_session_refs() {
        let bad_channel = event_with(
            &valid_content(),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
                &["h", "not-a-uuid"],
            ],
        );
        assert!(validate_pulse_entry_envelope(&bad_channel)
            .unwrap_err()
            .contains("channel UUID"));

        // Non-canonical UUID spellings `Uuid::parse_str` accepts but no `#h`
        // filter can ever match, and the TypeScript twin rejects.
        for non_canonical in [
            CHANNEL.to_ascii_uppercase(),
            CHANNEL.replace('-', ""),
            format!("{{{CHANNEL}}}"),
        ] {
            let ev = event_with(
                &valid_content(),
                &[
                    &["a", COORD],
                    &["pu-v", PULSE_ENTRY_TAG_VERSION],
                    &["pu-type", "plan"],
                    &["h", &non_canonical],
                ],
            );
            assert!(
                validate_pulse_entry_envelope(&ev)
                    .unwrap_err()
                    .contains("lowercase canonical channel UUID"),
                "expected {non_canonical} to be rejected"
            );
        }

        let bad_session = event_with(
            &valid_content(),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
                &["pu-session", &SESSION_REF.to_ascii_uppercase()],
            ],
        );
        assert!(validate_pulse_entry_envelope(&bad_session)
            .unwrap_err()
            .contains("lowercase canonical UUID"));
    }

    #[test]
    fn supersedes_is_syntax_only_but_never_self() {
        let unknown_target = event_with(
            &content_with(&format!(r#","supersedes":"{OTHER_ID}""#)),
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
            ],
        );
        // No database lookup: an id this relay has never seen is accepted.
        assert_eq!(
            validate_pulse_entry_envelope(&unknown_target)
                .expect("unknown target accepted")
                .supersedes
                .as_deref(),
            Some(OTHER_ID)
        );

        for bad in [
            OTHER_ID.to_ascii_uppercase(),
            OTHER_ID[..63].to_owned(),
            format!("{OTHER_ID}0"),
            "not hex".to_owned(),
        ] {
            let content = content_with(&format!(r#","supersedes":"{bad}""#));
            assert!(decode_pulse_entry(&content)
                .unwrap_err()
                .contains("lowercase hex event id"));
        }

        // A self-referencing entry cannot be honestly constructed (the id is a
        // hash of the content), so rewrite the content of a signed event —
        // exactly the smuggled shape the check exists to refuse.
        let mut raw = serde_json::to_value(&unknown_target).expect("event serializes");
        raw["content"] = Value::String(content_with(&format!(
            r#","supersedes":"{}""#,
            unknown_target.id.to_hex()
        )));
        let self_ref: nostr::Event =
            serde_json::from_value(raw).expect("tampered event deserializes");
        assert!(validate_pulse_entry_envelope(&self_ref)
            .unwrap_err()
            .contains("must not supersede itself"));
    }

    #[test]
    fn other_kinds_are_not_pulse_entries() {
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(1), valid_content())
            .sign_with_keys(&keys)
            .expect("test event signs");
        assert!(validate_pulse_entry_envelope(&event)
            .unwrap_err()
            .contains("not a pulse entry"));
    }

    #[test]
    fn project_coordinate_requires_exactly_one_a_tag() {
        assert_eq!(
            pulse_entry_project_coordinate(&valid_event()).as_deref(),
            Some(COORD)
        );
        let two = event_with(
            &valid_content(),
            &[
                &["a", COORD],
                &["a", "30621:bb:other"],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
            ],
        );
        assert_eq!(pulse_entry_project_coordinate(&two), None);
        let none = event_with(
            &valid_content(),
            &[&["pu-v", PULSE_ENTRY_TAG_VERSION], &["pu-type", "plan"]],
        );
        assert_eq!(pulse_entry_project_coordinate(&none), None);
    }

    // ── cost ─────────────────────────────────────────────────────────────

    const SEAT: &str = "cc00000000000000000000000000000000000000000000000000000000000022";
    const SEAT_TWO: &str = "dd00000000000000000000000000000000000000000000000000000000000033";

    fn cost_content(cost: &str) -> String {
        content_with(&format!(
            r#","codeAreas":[],"branch":null,"supersedes":null,"cost":{cost}"#
        ))
    }

    #[test]
    fn a_costless_entry_is_byte_identical_to_the_pre_cost_shape() {
        let entry = decode_pulse_entry(&valid_content()).expect("valid entry decodes");
        assert_eq!(entry.cost, None);
        assert_eq!(
            serde_json::to_string(&entry).expect("entry serializes"),
            valid_content()
        );
    }

    #[test]
    fn accepts_a_cost_with_seats_and_a_matching_total() {
        let content = cost_content(&format!(
            r#"{{"seats":[{{"actor":"{SEAT}","role":"builder","model":"opus-5[1m]","inputTokens":10,"outputTokens":5,"cacheReadTokens":100,"cacheWriteTokens":20,"toolCalls":9,"turns":3}}],"totalTokens":135}}"#
        ));
        let entry = decode_pulse_entry(&content).expect("cost decodes");
        let cost = entry.cost.expect("cost present");
        assert_eq!(cost.seats.len(), 1);
        assert_eq!(cost.seats[0].actor.as_deref(), Some(SEAT));
        assert_eq!(cost.seats[0].role.as_deref(), Some("builder"));
        assert_eq!(cost.seats[0].turns, Some(3));
        assert_eq!(cost.total_tokens, Some(135));
        assert_eq!(cost.seat_token_sum(), Some(135));
    }

    #[test]
    fn accepts_a_seat_that_reports_only_some_of_the_counts() {
        let content = cost_content(&format!(
            r#"{{"seats":[{{"actor":"{SEAT}","outputTokens":7}}]}}"#
        ));
        let entry = decode_pulse_entry(&content).expect("partial seat decodes");
        let cost = entry.cost.expect("cost present");
        assert_eq!(cost.seats[0].input_tokens, None);
        assert_eq!(cost.seats[0].output_tokens, Some(7));
        assert_eq!(cost.total_tokens, None);
        assert_eq!(cost.seat_token_sum(), Some(7));
    }

    #[test]
    fn rejects_an_empty_cost_object_and_an_empty_seat() {
        assert!(decode_pulse_entry(&cost_content("{}"))
            .unwrap_err()
            .contains("cost must report something"));
        assert!(decode_pulse_entry(&cost_content(r#"{"seats":[{}]}"#))
            .unwrap_err()
            .contains("cost seat must report something"));
    }

    #[test]
    fn rejects_a_total_that_does_not_equal_the_seats_it_lists() {
        let content = cost_content(&format!(
            r#"{{"seats":[{{"actor":"{SEAT}","inputTokens":10,"outputTokens":5}}],"totalTokens":900}}"#
        ));
        assert!(decode_pulse_entry(&content)
            .unwrap_err()
            .contains("totalTokens 900 does not equal"));
    }

    #[test]
    fn rejects_a_repeated_seat_and_a_malformed_actor() {
        let repeated = cost_content(&format!(
            r#"{{"seats":[{{"actor":"{SEAT}","turns":1}},{{"actor":"{SEAT}","turns":1}}]}}"#
        ));
        assert!(decode_pulse_entry(&repeated)
            .unwrap_err()
            .contains("repeats cost seat"));

        let upper = cost_content(&format!(
            r#"{{"seats":[{{"actor":"{}","turns":1}}]}}"#,
            SEAT.to_uppercase()
        ));
        assert!(decode_pulse_entry(&upper)
            .unwrap_err()
            .contains("64-character lowercase hex"));
    }

    #[test]
    fn rejects_too_many_seats_and_unknown_cost_keys() {
        let seats: Vec<String> = (0..=MAX_PULSE_COST_SEATS)
            .map(|index| format!(r#"{{"role":"r{index}","turns":1}}"#))
            .collect();
        let many = cost_content(&format!(r#"{{"seats":[{}]}}"#, seats.join(",")));
        assert!(decode_pulse_entry(&many).unwrap_err().contains("more than"));

        assert!(decode_pulse_entry(&cost_content(r#"{"costUsd":1.5}"#)).is_err());
        assert!(
            decode_pulse_entry(&cost_content(r#"{"seats":[{"actor":null,"spend":1}]}"#)).is_err()
        );
    }

    #[test]
    fn rejects_a_blank_role_or_model_on_a_seat() {
        assert!(
            decode_pulse_entry(&cost_content(r#"{"seats":[{"role":"  ","turns":1}]}"#))
                .unwrap_err()
                .contains("role")
        );
        assert!(
            decode_pulse_entry(&cost_content(r#"{"seats":[{"model":"","turns":1}]}"#))
                .unwrap_err()
                .contains("model")
        );
    }

    #[test]
    fn a_cost_survives_the_signed_envelope_validator() {
        let content = cost_content(&format!(
            r#"{{"seats":[{{"actor":"{SEAT}","turns":2}},{{"actor":"{SEAT_TWO}","turns":1}}]}}"#
        ));
        let event = event_with(
            &content,
            &[
                &["a", COORD],
                &["pu-v", PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
            ],
        );
        let entry = validate_pulse_entry_envelope(&event).expect("cost-bearing entry validates");
        assert_eq!(entry.cost.expect("cost").seats.len(), 2);
    }
}
