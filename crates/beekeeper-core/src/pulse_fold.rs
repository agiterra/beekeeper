//! Pure Project Pulse v2 digest model and fold.
//!
//! This module is the single authority for entry supersession, lifecycle
//! authority, per-generation lease reachability, and umbrella session state.
//! Network adapters supply bounded event rows and explicit source errors; the
//! fold performs no I/O.

use std::collections::{HashMap, HashSet};

use crate::coding_session_closure::decode_coding_session_closure;
use crate::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use crate::coding_session_lease::{decode_coding_session_lease, CodingSessionLeaseState};
use crate::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
    CodingSessionLifecycleCommandPayload,
};
use crate::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, decode_coding_session_metadata, LifecycleReceipt,
    ReceiptStatus, SessionMetadata, SessionStatus,
};
use crate::coding_session_title::{SessionDisplayNameOrigin, SessionExecutionAuthority};
use crate::kind::{
    normalize_project_coordinate, KIND_CODING_SESSION_CLOSURE, KIND_CODING_SESSION_GENERATED_TITLE,
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_GOAL, KIND_CODING_SESSION_LEASE,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_NAME, KIND_PULSE_ENTRY,
};
use crate::pulse::{pulse_entry_project_coordinate, validate_pulse_entry_envelope, PulseEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[path = "pulse_fold_names.rs"]
mod names;
pub use names::{session_name_record_from_json, winning_name_event_id};

/// Conservative client-composed lease lifetime from signed issue time.
///
/// Redis retains an accepted lease for 180 seconds, but a cold client knows only
/// the signed event time. Subtracting 30 seconds prevents clock/transport delay
/// from extending a client claim beyond the relay's authoritative snapshot.
pub const PULSE_LEASE_TTL: i64 = 150;

/// Schema of the digest envelope — the kind-39011 content object of the plan's
/// §6, which Slice 1 already emits verbatim so Slice 2 can change only *who*
/// computes it.
pub const PULSE_DIGEST_SCHEMA: &str = "buzz-project-pulse-digest/v2";

/// `source` value for a digest this client folded itself. The relay-signed
/// 39011 of Slice 2 substitutes `relay-digest` and changes nothing else.
const PULSE_DIGEST_SOURCE: &str = "client-composed";

/// How far the session scan reaches. There is no queryable "sessions of this
/// project" relation, and a community-wide 44223 scan is both unbounded and a
/// leak, so sessions are reached through the project's channels only — a
/// session running in a channel outside that set is not discoverable and
/// no surface may present the Active-work list as exhaustive.
const PULSE_SESSIONS_SCOPE: &str = "project channels";

/// The three `commitConfirmation` strings. Fixed here and mirrored in Desktop
/// so the two surfaces cannot drift. Never render "relay reachable": the fact
/// is that the relay's advertised refs contained this exact commit at
/// `verifiedAt`, which says nothing about whether the session is connected.
const COMMIT_CONFIRMED: &str = "Commit confirmed on relay";
const COMMIT_NOT_FOUND: &str = "Commit not found on relay";
const COMMIT_NOT_CHECKED: &str = "Commit not checked";
// ── Digest model ─────────────────────────────────────────────────────────────

/// One `{scope, message}` row: a source query that failed or was truncated, or
/// a fold observation that must not disappear (an excluded invalid entry, a
/// dangling `supersedes`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PulseDigestError {
    /// What the failure was about — `entries`, `channels`, or
    /// `sessions:<channel-id>` for a read; `invalid-entry` /
    /// `unresolved-supersedes` for a fold observation.
    pub scope: String,
    /// Human-readable detail.
    pub message: String,
}

/// One supersession claim relating two entries.
///
/// An **honored** claim is recorded on the entry it retires, naming the
/// claimant. An **unhonored** claim is recorded on the claimant instead,
/// naming the entry it failed to retire — so a refused claim is visible on the
/// entry that made it rather than silently vanishing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseSupersessionClaim {
    /// The other entry in the relation.
    pub event_id: String,
    /// That entry's author, or `null` when it is not in the visible result set.
    pub pubkey: Option<String>,
    /// Whether the fold honored the claim.
    pub honored: bool,
    /// `null` when honored, else `cross-author`, `unresolved`, or
    /// `out-of-order`.
    pub reason: Option<String>,
}

/// One folded Pulse entry. `claimedAreas` are claims, never observed facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseDigestEntry {
    /// The 44240 event id.
    pub event_id: String,
    /// The author.
    pub pubkey: String,
    /// The event's `created_at`, Unix seconds.
    pub created_at: i64,
    /// `plan | milestone | note | handoff | blocker`.
    #[serde(rename = "type")]
    pub entry_type: String,
    /// The author's prose, verbatim.
    pub text: String,
    /// Repository-relative paths the author *claims* to be working in.
    pub claimed_areas: Vec<String>,
    /// The branch the claim applies to, or `null`.
    pub branch: Option<String>,
    /// The `pu-session` tag, echoed verbatim. Author-controlled and unverified
    /// at ingest — a surface must resolve 44226/44228 itself before placing an
    /// entry inside a session's card.
    pub session_ref: Option<String>,
    /// The entry this one claims to revise, echoed verbatim.
    pub supersedes: Option<String>,
    /// Every claim relating this entry to another, `eventId` ascending.
    pub superseded_by: Vec<PulseSupersessionClaim>,
    /// False exactly when an honored claim names this entry. Superseded
    /// entries are never dropped.
    pub active: bool,
}

/// Reachability of one authority-proven generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PulseGenerationReachability {
    /// A current live lease matches the lifecycle-bound provider authority.
    ProviderReachable,
    /// No current valid live lease exists.
    Unverified,
    /// Durable metadata reports stopped or disconnected.
    Terminal,
}

/// One authority-proven exact coding-session generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseDigestGeneration {
    /// Exact generation target key.
    pub target_key: String,
    /// Stable provider execution key with generation omitted.
    pub execution_key: String,
    /// Provider authority established by accepted lifecycle command and receipt.
    pub provider_authority_pubkey: String,
    /// Whether this is the highest accepted generation of its execution.
    pub current: bool,
    /// Reachability independent from the umbrella lifecycle.
    pub reachability: PulseGenerationReachability,
    /// Newest authority-signed metadata status, or `null`.
    pub status: Option<String>,
    /// Metadata observation time, or `null`.
    pub status_at: Option<i64>,
    /// Observed branch, or `null`.
    pub branch: Option<String>,
    /// Observed HEAD commit, or `null`.
    pub observed_commit: Option<String>,
    /// Observed worktree dirtiness. Unknown is not false.
    pub dirty: Option<bool>,
    /// Whether the relay's advertised refs contained the observed commit at
    /// `verifiedAt`. Null exactly when `verifiedAt` is null.
    pub relay_reachable: Option<bool>,
    /// When the commit check completed, or `null`.
    pub verified_at: Option<i64>,
    /// One of the three fixed [`COMMIT_CONFIRMED`] / [`COMMIT_NOT_FOUND`] /
    /// [`COMMIT_NOT_CHECKED`] strings. Surfaces append the `verifiedAt` age to
    /// the first two at paint time; the age is never folded in.
    pub commit_confirmation: String,
    /// Signed lease state at the highest unambiguous sequence.
    pub lease_state: Option<String>,
    /// Provider-signed issue time.
    pub lease_issued_at: Option<i64>,
    /// Relay receipt time; always null for client-composed digests.
    pub lease_accepted_at: Option<i64>,
    /// Conservative client expiry derived from signed issue time.
    pub lease_expires_at: Option<i64>,
    /// Provider signer of the selected lease.
    pub lease_signer: Option<String>,
    /// Selected signed lease event id.
    pub lease_source_event_id: Option<String>,
    /// Highest valid lease sequence, including an ambiguous equal-sequence conflict.
    pub lease_sequence: Option<u64>,
    /// Durable lifecycle command event that selected provider authority.
    pub lifecycle_command_event_id: String,
    /// Successful provider-signed lifecycle receipt event.
    pub lifecycle_receipt_event_id: String,
    /// Every event folded into this generation, ascending.
    pub source_event_ids: Vec<String>,
}

/// One durable coding-session umbrella with nested execution generations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseDigestSession {
    /// Verified sessionRef, or `implicit:<executionKey>` for a legacy singleton.
    pub session_key: String,
    /// Canonical umbrella UUID, or null for an implicit singleton.
    pub session_ref: Option<String>,
    /// The umbrella's display name from the shared resolver
    /// (`crate::coding_session_title::resolve_session_display_name`): the
    /// newest founder-signed 44229, else the earliest standing provider-signed
    /// 44252, else null. Never a non-founder 44229, and never a fallback.
    pub name: Option<String>,
    /// `person` or `generated` — which record supplied `name`; `None` exactly
    /// when `name` is null.
    ///
    /// Not serialized, and neither are the two fields below: the session
    /// object's bytes are pinned by `conformance/project-pulse-fold`, which
    /// the Desktop fold also binds to and which carries its own origins beside
    /// the session (`nameOriginsBySession`). Every reader that prints `name`
    /// prints these beside it — `bee pulse` as `nameOrigin`/`nameModel`/
    /// `nameSigner` and a digest-level `nameOrigins` map, the ACP prompt as
    /// `[auto-named]` — so a generated title is never shown as a person's.
    #[serde(skip)]
    pub name_origin: Option<String>,
    /// The model that generated `name`; `generated` only. Not serialized.
    #[serde(skip)]
    pub name_model: Option<String>,
    /// The provider pubkey that signed `name`; `generated` only. Not
    /// serialized.
    #[serde(skip)]
    pub name_signer: Option<String>,
    /// Newest 44227, or null.
    pub goal: Option<String>,
    /// `open` or `closed`, independent from generation reachability.
    pub lifecycle: String,
    /// `provider_reachable`, `open_unverified`, or `closed`.
    pub coordination_state: String,
    /// Newest durable metadata observation across generations.
    pub latest_observation_at: Option<i64>,
    /// `now - latestObservationAt`, or null without metadata.
    pub observed_age_seconds: Option<i64>,
    /// Authority-proven generations, current ones first.
    pub generations: Vec<PulseDigestGeneration>,
    /// Every event folded into the umbrella, ascending.
    pub source_event_ids: Vec<String>,
}

/// The digest envelope — the kind-39011 content object plus `source`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseDigest {
    /// Always [`PULSE_DIGEST_SCHEMA`].
    pub schema: String,
    /// Who folded it.
    pub source: String,
    /// The project coordinate the digest was asked for.
    pub project: String,
    /// The wall-clock second the last source query returned.
    pub as_of: i64,
    /// False whenever any source query failed or was truncated.
    pub complete: bool,
    /// Always [`PULSE_SESSIONS_SCOPE`].
    pub sessions_scope: String,
    /// Sessions, `statusAt` descending, ties on `targetKey` ascending.
    pub sessions: Vec<PulseDigestSession>,
    /// Session keys whose current generation has authority-valid lease evidence.
    pub provider_reachable_sessions: Vec<String>,
    /// Open session keys whose current liveness cannot be verified.
    pub open_unverified_sessions: Vec<String>,
    /// Durably closed session keys retained as history.
    pub closed_sessions: Vec<String>,
    /// Entries, `createdAt` descending, ties on the greater event id first.
    pub entries: Vec<PulseDigestEntry>,
    /// Failures and fold observations, `scope` then `message` ascending.
    pub errors: Vec<PulseDigestError>,
}

// ── Event access helpers ─────────────────────────────────────────────────────

/// A string field of a signature-stripped relay event.
fn json_str<'a>(event: &'a Value, field: &str) -> Option<&'a str> {
    event.get(field).and_then(Value::as_str)
}

/// The `created_at` of a relay event.
fn json_created_at(event: &Value) -> Option<i64> {
    event.get("created_at").and_then(Value::as_i64)
}

/// The kind of a relay event.
fn json_kind(event: &Value) -> Option<u32> {
    event
        .get("kind")
        .and_then(Value::as_u64)
        .and_then(|kind| u32::try_from(kind).ok())
}

/// The first value of the named tag on a relay event.
fn json_tag_value<'a>(event: &'a Value, name: &str) -> Option<&'a str> {
    event
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(Value::as_array)
        .find(|parts| parts.first().and_then(Value::as_str) == Some(name))
        .and_then(|parts| parts.get(1))
        .and_then(Value::as_str)
}

fn has_exact_ordered_two_field_tags(event: &Value, expected: &[(&str, Option<&str>)]) -> bool {
    let Some(tags) = event.get("tags").and_then(Value::as_array) else {
        return false;
    };
    if tags.len() != expected.len() {
        return false;
    }
    tags.iter().zip(expected).all(|(tag, (key, required))| {
        let Some(parts) = tag.as_array().filter(|parts| parts.len() == 2) else {
            return false;
        };
        let (Some(actual_key), Some(value)) = (
            parts.first().and_then(Value::as_str),
            parts.get(1).and_then(Value::as_str),
        ) else {
            return false;
        };
        actual_key == *key && required.map_or(!value.is_empty(), |required| value == required)
    })
}

/// Rebuild a `nostr::Event` from a relay row so buzz-core's validators can run
/// against it.
///
/// The signature is a placeholder: nothing in the Pulse fold verifies it (the
/// relay did that at ingest), while the *id* is load-bearing — the
/// self-supersession rule compares against it — so a row without a well-formed
/// id, author, kind, or tag set is not decodable at all and is reported rather
/// than guessed at.
fn nostr_event_from_json(event: &Value) -> Option<nostr::Event> {
    let id = nostr::EventId::from_hex(json_str(event, "id")?).ok()?;
    let pubkey = nostr::PublicKey::from_hex(json_str(event, "pubkey")?).ok()?;
    let created_at = u64::try_from(json_created_at(event)?).ok()?;
    let kind = u16::try_from(json_kind(event)?).ok()?;
    let mut tags: Vec<nostr::Tag> = Vec::new();
    for parts in event.get("tags")?.as_array()? {
        let parts: Vec<&str> = parts
            .as_array()?
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<&str>>>()?;
        tags.push(nostr::Tag::parse(parts).ok()?);
    }
    // 64 zero bytes is always a well-formed Schnorr signature value, so this
    // arm never fires; it is mapped rather than unwrapped.
    let signature = nostr::secp256k1::schnorr::Signature::from_slice(&[0u8; 64]).ok()?;
    Some(nostr::Event::new(
        id,
        pubkey,
        nostr::Timestamp::from_secs(created_at),
        nostr::Kind::Custom(kind),
        tags,
        json_str(event, "content")?,
        signature,
    ))
}

/// The total order the fold uses everywhere "newer" appears: `created_at`,
/// ties broken by the greater event id. Matching
/// `crates/beekeeper-core/src/coding_session_closure.rs:148-156`, so two events can
/// never each be newer than the other.
fn is_newer(a_created_at: i64, a_id: &str, b_created_at: i64, b_id: &str) -> bool {
    (a_created_at, a_id) > (b_created_at, b_id)
}

// ── The fold ─────────────────────────────────────────────────────────────────

/// One decoded, project-scoped Pulse entry, before its claims are resolved.
struct EntryRow {
    event_id: String,
    pubkey: String,
    created_at: i64,
    entry: PulseEntry,
    session_ref: Option<String>,
    branch: Option<String>,
    superseded_by: Vec<PulseSupersessionClaim>,
    active: bool,
}

/// One signed fact folded into a session row.
#[derive(Clone)]
struct FactRow {
    event_id: String,
    created_at: i64,
    content: String,
    channel_id: String,
}

#[derive(Clone)]
struct LifecycleCommandRow {
    fact: FactRow,
    payload: CodingSessionLifecycleCommandPayload,
}

#[derive(Clone)]
struct LifecycleReceiptRow {
    fact: FactRow,
    pubkey: String,
    payload: LifecycleReceipt,
}

#[derive(Clone)]
struct AcceptedGenerationRow {
    command: LifecycleCommandRow,
    receipt: LifecycleReceiptRow,
    target: CodingSessionTarget,
    target_key: String,
    execution_key: String,
    session_ref: Option<String>,
    provider_authority_pubkey: String,
    channel_id: String,
}

#[derive(Clone)]
struct LeaseRow {
    fact: FactRow,
    pubkey: String,
    state: CodingSessionLeaseState,
    sequence: u64,
}

type GroupedSessionGenerations =
    HashMap<String, (Option<String>, HashSet<String>, Vec<PulseDigestGeneration>)>;

/// Fold Pulse entries and coding-session facts into the digest envelope.
///
/// `events` are signature-stripped relay rows of any kind; rows that belong to
/// another project, or to no project, are ignored. `now` is a Unix-seconds
/// clock read **after** the last source query returned, and `source_errors`
/// carries one row per query that failed or was truncated — the only thing that
/// can set `complete: false`.
///
/// Every rule this implements is stated in
/// `conformance/project-pulse-fold/CONTRACT.md` and pinned by the vectors
/// beside it.
pub fn fold_pulse_digest(
    project: &str,
    now: i64,
    source_errors: Vec<PulseDigestError>,
    events: &[Value],
) -> PulseDigest {
    let complete = source_errors.is_empty();
    let mut errors = source_errors;
    let mut rows = Vec::new();

    for event in events {
        if json_kind(event) != Some(KIND_PULSE_ENTRY) {
            continue;
        }
        let raw_id = json_str(event, "id").unwrap_or_default().to_owned();
        let Some(decoded) = nostr_event_from_json(event) else {
            errors.push(invalid_entry_error(&raw_id));
            continue;
        };
        match pulse_entry_project_coordinate(&decoded) {
            // Another project's entry: not this digest's business, and not an
            // error — a mixed result set is a caller's shape, not a defect.
            Some(coordinate) if coordinate != project => continue,
            Some(_) => {}
            None => {
                errors.push(invalid_entry_error(&raw_id));
                continue;
            }
        }
        match validate_pulse_entry_envelope(&decoded) {
            Ok(entry) => rows.push(EntryRow {
                event_id: decoded.id.to_hex(),
                pubkey: decoded.pubkey.to_hex(),
                created_at: json_created_at(event).unwrap_or_default(),
                session_ref: json_tag_value(event, "pu-session").map(str::to_owned),
                // The content field is the claim; the tag exists so a relay
                // filter can see it, and the validator already proved the two
                // agree whenever both are present.
                branch: entry
                    .branch
                    .clone()
                    .or_else(|| json_tag_value(event, "branch").map(str::to_owned)),
                entry,
                superseded_by: Vec::new(),
                active: true,
            }),
            Err(_) => errors.push(invalid_entry_error(&raw_id)),
        }
    }

    resolve_supersession(&mut rows, &mut errors);

    rows.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.event_id.cmp(&a.event_id))
    });
    let entries: Vec<PulseDigestEntry> = rows
        .into_iter()
        .map(|row| PulseDigestEntry {
            event_id: row.event_id,
            pubkey: row.pubkey,
            created_at: row.created_at,
            entry_type: row.entry.entry_type.as_str().to_owned(),
            text: row.entry.text,
            claimed_areas: row.entry.code_areas,
            branch: row.branch,
            session_ref: row.session_ref,
            supersedes: row.entry.supersedes,
            superseded_by: row.superseded_by,
            active: row.active,
        })
        .collect();

    let sessions = fold_sessions(project, now, events);
    let provider_reachable_sessions = sessions
        .iter()
        .filter(|session| session.coordination_state == "provider_reachable")
        .map(|session| session.session_key.clone())
        .collect();
    let open_unverified_sessions = sessions
        .iter()
        .filter(|session| session.coordination_state == "open_unverified")
        .map(|session| session.session_key.clone())
        .collect();
    let closed_sessions = sessions
        .iter()
        .filter(|session| session.coordination_state == "closed")
        .map(|session| session.session_key.clone())
        .collect();

    errors.sort_by(|a, b| {
        a.scope
            .cmp(&b.scope)
            .then_with(|| a.message.cmp(&b.message))
    });

    PulseDigest {
        schema: PULSE_DIGEST_SCHEMA.to_owned(),
        source: PULSE_DIGEST_SOURCE.to_owned(),
        project: project.to_owned(),
        as_of: now,
        complete,
        sessions_scope: PULSE_SESSIONS_SCOPE.to_owned(),
        sessions,
        provider_reachable_sessions,
        open_unverified_sessions,
        closed_sessions,
        entries,
        errors,
    }
}

/// The `errors[]` row for an entry excluded by validation. An invalid entry is
/// never silently dropped and never counted as a valid claim, and it is not a
/// failed read — `complete` stays true.
fn invalid_entry_error(event_id: &str) -> PulseDigestError {
    PulseDigestError {
        scope: "invalid-entry".to_owned(),
        message: format!("entry {event_id} failed validation and was excluded"),
    }
}

/// Apply the supersession fold law: a **single-pass marking, never a
/// traversal**, so cycles are structurally impossible and no traversal can hang
/// or blank the active set.
///
/// Entry `E` is marked superseded only by an `S` with `S.supersedes == E.id`,
/// `S.pubkey == E.pubkey`, and `S` newer than `E` under [`is_newer`]. A
/// cross-author, out-of-order, or dangling claim is recorded on the claimant as
/// unhonored and changes nothing about its target — otherwise any project
/// writer could publish a one-line entry superseding a peer's `blocker` and
/// silently push it out of the set that drives `wait | consult | proceed`.
fn resolve_supersession(rows: &mut [EntryRow], errors: &mut Vec<PulseDigestError>) {
    let index: HashMap<String, usize> = rows
        .iter()
        .enumerate()
        .map(|(position, row)| (row.event_id.clone(), position))
        .collect();

    for position in 0..rows.len() {
        let Some(target_id) = rows[position].entry.supersedes.clone() else {
            continue;
        };
        let Some(&target) = index.get(&target_id) else {
            rows[position].superseded_by.push(PulseSupersessionClaim {
                event_id: target_id.clone(),
                pubkey: None,
                honored: false,
                reason: Some("unresolved".to_owned()),
            });
            errors.push(PulseDigestError {
                scope: "unresolved-supersedes".to_owned(),
                message: format!(
                    "entry {} supersedes {target_id}, which is not in the visible result set",
                    rows[position].event_id
                ),
            });
            continue;
        };
        // A self-reference cannot reach here: buzz-core rejects it, so such an
        // entry was already excluded above.
        if target == position {
            continue;
        }
        let unhonored = if rows[target].pubkey != rows[position].pubkey {
            Some("cross-author")
        } else if !is_newer(
            rows[position].created_at,
            &rows[position].event_id,
            rows[target].created_at,
            &rows[target].event_id,
        ) {
            Some("out-of-order")
        } else {
            None
        };
        match unhonored {
            Some(reason) => {
                let claim = PulseSupersessionClaim {
                    event_id: rows[target].event_id.clone(),
                    pubkey: Some(rows[target].pubkey.clone()),
                    honored: false,
                    reason: Some(reason.to_owned()),
                };
                rows[position].superseded_by.push(claim);
            }
            None => {
                let claim = PulseSupersessionClaim {
                    event_id: rows[position].event_id.clone(),
                    pubkey: Some(rows[position].pubkey.clone()),
                    honored: true,
                    reason: None,
                };
                rows[target].superseded_by.push(claim);
                rows[target].active = false;
            }
        }
    }

    for row in rows.iter_mut() {
        row.superseded_by
            .sort_by(|a, b| a.event_id.cmp(&b.event_id));
    }
}

/// Fold the coding-session facts of one project into session rows.
///
/// Only 44223 rows whose content `projectRef` normalizes to this project
/// participate; goal, name, and closure join on the umbrella `sessionRef`.
fn coding_execution_key(target: &CodingSessionTarget) -> String {
    let fields = [
        target.driver.as_str(),
        target.instance_id.as_str(),
        target.session_id.as_str(),
    ];
    let mut key = String::from("coding-execution/v1|");
    for field in fields {
        key.push_str(&field.len().to_string());
        key.push(':');
        key.push_str(field);
    }
    key
}

fn lifecycle_status_succeeded(
    action: &CodingSessionLifecycleAction,
    status: ReceiptStatus,
) -> bool {
    match action {
        CodingSessionLifecycleAction::SessionCreate { .. } => matches!(
            status,
            ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
        ),
        // A restart is a resume that detached first; the provider answers
        // both with the resume receipts.
        CodingSessionLifecycleAction::SessionResume { .. }
        | CodingSessionLifecycleAction::SessionRestart { .. } => matches!(
            status,
            ReceiptStatus::Resumed | ReceiptStatus::ResumedWithoutContext
        ),
        CodingSessionLifecycleAction::SessionStop { .. } => false,
        // A hire produces no receipt of its own: the host answers it by
        // publishing a seated create, and *that* create's receipts are the
        // hire's. Nothing here can ever confirm a generation.
        CodingSessionLifecycleAction::SessionHire { .. } => false,
    }
}

fn lifecycle_authority(action: &CodingSessionLifecycleAction) -> &str {
    match action {
        CodingSessionLifecycleAction::SessionCreate {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionResume {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionRestart {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionStop {
            provider_authority_pubkey,
            ..
        } => provider_authority_pubkey,
        // A hire names no provider authority — it asks a *host* to choose one.
        // The empty string matches no signer, which is the honest answer: no
        // receipt can be attributed to a hire.
        CodingSessionLifecycleAction::SessionHire { .. } => "",
    }
}

fn accept_only_unique_proofs(
    accepted: &mut HashMap<(String, String), AcceptedGenerationRow>,
    candidates_by_target: HashMap<(String, String), Vec<AcceptedGenerationRow>>,
) {
    for (key, mut candidates) in candidates_by_target {
        candidates.sort_by(|left, right| {
            (&left.command.fact.event_id, &left.receipt.fact.event_id)
                .cmp(&(&right.command.fact.event_id, &right.receipt.fact.event_id))
        });
        candidates.dedup_by(|left, right| {
            left.command.fact.event_id == right.command.fact.event_id
                && left.receipt.fact.event_id == right.receipt.fact.event_id
        });
        if candidates.len() == 1 && !accepted.contains_key(&key) {
            if let Some(candidate) = candidates.pop() {
                accepted.insert(key, candidate);
            }
        }
    }
}

fn fold_sessions(project: &str, now: i64, events: &[Value]) -> Vec<PulseDigestSession> {
    let mut commands: HashMap<(String, String), Vec<LifecycleCommandRow>> = HashMap::new();
    let mut receipts: HashMap<(String, String), Vec<LifecycleReceiptRow>> = HashMap::new();
    for event in events {
        let (Some(kind), Some(event_id), Some(created_at), Some(content), Some(channel_id)) = (
            json_kind(event),
            json_str(event, "id"),
            json_created_at(event),
            json_str(event, "content"),
            json_tag_value(event, "h").filter(|channel_id| !channel_id.is_empty()),
        ) else {
            continue;
        };
        let fact = FactRow {
            event_id: event_id.to_owned(),
            created_at,
            content: content.to_owned(),
            channel_id: channel_id.to_owned(),
        };
        if kind == KIND_CODING_SESSION_LIFECYCLE_COMMAND {
            let Ok(payload) = decode_coding_session_lifecycle_command(content) else {
                continue;
            };
            if !has_exact_ordered_two_field_tags(
                event,
                &[
                    ("h", None),
                    ("csl-v", Some("csl1-1")),
                    ("csl-command", Some(payload.command_id.as_str())),
                ],
            ) {
                continue;
            }
            let rows = commands
                .entry((channel_id.to_owned(), payload.command_id.clone()))
                .or_default();
            if !rows.iter().any(|row| row.fact.event_id == fact.event_id) {
                rows.push(LifecycleCommandRow { fact, payload });
            }
        } else if kind == KIND_CODING_SESSION_LIFECYCLE_RECEIPT {
            let Ok(payload) = decode_coding_session_lifecycle_receipt(content) else {
                continue;
            };
            let (Some(pubkey), Some(_target)) =
                (json_str(event, "pubkey"), payload.session.as_ref())
            else {
                continue;
            };
            let receipt_key = format!(
                "coding-session-lifecycle-receipt/v1|{}:{}",
                payload.command_id.len(),
                payload.command_id
            );
            if !has_exact_ordered_two_field_tags(
                event,
                &[
                    ("h", None),
                    ("cslr-v", Some("cslr1-1")),
                    ("csl-command", Some(payload.command_id.as_str())),
                    ("csl-key", Some(receipt_key.as_str())),
                ],
            ) {
                continue;
            }
            let rows = receipts
                .entry((channel_id.to_owned(), payload.command_id.clone()))
                .or_default();
            if !rows.iter().any(|row| row.fact.event_id == fact.event_id) {
                rows.push(LifecycleReceiptRow {
                    fact,
                    pubkey: pubkey.to_owned(),
                    payload,
                });
            }
        }
    }

    let mut pairs = Vec::new();
    for (channel_command_key, candidates) in &commands {
        let Some(answers) = receipts.get(channel_command_key) else {
            continue;
        };
        if candidates.len() != 1 || answers.len() != 1 {
            continue;
        }
        let command = candidates[0].clone();
        let receipt = answers[0].clone();
        if receipt.pubkey != lifecycle_authority(&command.payload.action)
            || !lifecycle_status_succeeded(&command.payload.action, receipt.payload.status)
        {
            continue;
        }
        pairs.push((command, receipt));
    }

    let mut accepted: HashMap<(String, String), AcceptedGenerationRow> = HashMap::new();
    let mut create_candidates: HashMap<(String, String), Vec<AcceptedGenerationRow>> =
        HashMap::new();
    for (command, receipt) in &pairs {
        let CodingSessionLifecycleAction::SessionCreate {
            project_ref,
            session_ref,
            provider_authority_pubkey,
            ..
        } = &command.payload.action
        else {
            continue;
        };
        let matches_project = project_ref
            .as_deref()
            .and_then(normalize_project_coordinate)
            .is_some_and(|coordinate| coordinate == project);
        let Some(target) = receipt
            .payload
            .session
            .clone()
            .filter(|target| matches_project && target.generation == 1)
        else {
            continue;
        };
        let target_key = coding_session_target_key(&target);
        create_candidates
            .entry((command.fact.channel_id.clone(), target_key.clone()))
            .or_default()
            .push(AcceptedGenerationRow {
                command: command.clone(),
                receipt: receipt.clone(),
                execution_key: coding_execution_key(&target),
                target,
                target_key,
                session_ref: session_ref.clone(),
                provider_authority_pubkey: provider_authority_pubkey.clone(),
                channel_id: command.fact.channel_id.clone(),
            });
    }
    accept_only_unique_proofs(&mut accepted, create_candidates);

    let mut changed = true;
    while changed {
        let mut resume_candidates: HashMap<(String, String), Vec<AcceptedGenerationRow>> =
            HashMap::new();
        for (command, receipt) in &pairs {
            // A restart mints the next generation exactly as a resume does.
            let (CodingSessionLifecycleAction::SessionResume {
                session,
                provider_authority_pubkey,
            }
            | CodingSessionLifecycleAction::SessionRestart {
                session,
                provider_authority_pubkey,
            }) = &command.payload.action
            else {
                continue;
            };
            let previous_key = (
                command.fact.channel_id.clone(),
                coding_session_target_key(session),
            );
            let Some(previous) = accepted.get(&previous_key).cloned() else {
                continue;
            };
            let Some(target) = receipt.payload.session.clone() else {
                continue;
            };
            let execution_key = coding_execution_key(&target);
            if execution_key != previous.execution_key
                || target.generation != session.generation.saturating_add(1)
            {
                continue;
            }
            let target_key = coding_session_target_key(&target);
            let accepted_key = (command.fact.channel_id.clone(), target_key.clone());
            resume_candidates
                .entry(accepted_key)
                .or_default()
                .push(AcceptedGenerationRow {
                    command: command.clone(),
                    receipt: receipt.clone(),
                    target,
                    target_key,
                    execution_key,
                    session_ref: previous.session_ref,
                    provider_authority_pubkey: provider_authority_pubkey.clone(),
                    channel_id: command.fact.channel_id.clone(),
                });
        }
        let before = accepted.len();
        accept_only_unique_proofs(&mut accepted, resume_candidates);
        changed = accepted.len() != before;
    }

    let mut metadata: HashMap<(String, String), (FactRow, SessionMetadata)> = HashMap::new();
    let mut leases: HashMap<(String, String), Vec<LeaseRow>> = HashMap::new();
    let mut goals: HashMap<(String, String), FactRow> = HashMap::new();
    let mut names = names::NameInputs::default();
    let mut closures: HashMap<(String, String), FactRow> = HashMap::new();
    for event in events {
        let (Some(kind), Some(event_id), Some(created_at), Some(content), Some(channel_id)) = (
            json_kind(event),
            json_str(event, "id"),
            json_created_at(event),
            json_str(event, "content"),
            json_tag_value(event, "h").filter(|channel_id| !channel_id.is_empty()),
        ) else {
            continue;
        };
        let fact = FactRow {
            event_id: event_id.to_owned(),
            created_at,
            content: content.to_owned(),
            channel_id: channel_id.to_owned(),
        };
        match kind {
            KIND_CODING_SESSION_METADATA => {
                let Ok(meta) = decode_coding_session_metadata(content) else {
                    continue;
                };
                let key = coding_session_target_key(&meta.session);
                let channel_target_key = (channel_id.to_owned(), key);
                let Some(authority) = accepted.get(&channel_target_key) else {
                    continue;
                };
                let matches_project = meta
                    .project_ref
                    .as_deref()
                    .and_then(normalize_project_coordinate)
                    .is_some_and(|coordinate| coordinate == project);
                if !matches_project
                    || json_str(event, "pubkey")
                        != Some(authority.provider_authority_pubkey.as_str())
                {
                    continue;
                }
                keep_newest_pair(&mut metadata, channel_target_key, fact, meta);
            }
            KIND_CODING_SESSION_LEASE => {
                let Ok(lease) = decode_coding_session_lease(content) else {
                    continue;
                };
                let target_key = coding_session_target_key(&lease.target);
                let channel_target_key = (channel_id.to_owned(), target_key.clone());
                let Some(authority) = accepted.get(&channel_target_key) else {
                    continue;
                };
                let Some(pubkey) = json_str(event, "pubkey") else {
                    continue;
                };
                let command_id = json_tag_value(event, "csl-command");
                let lease_sequence = lease.lease_sequence.to_string();
                if pubkey != authority.provider_authority_pubkey
                    || command_id != Some(authority.command.payload.command_id.as_str())
                    || !has_exact_ordered_two_field_tags(
                        event,
                        &[
                            ("h", None),
                            ("cslease-v", Some("cslease1-1")),
                            ("cs-target", Some(target_key.as_str())),
                            ("csl-command", command_id),
                            ("cslease-seq", Some(lease_sequence.as_str())),
                        ],
                    )
                {
                    continue;
                }
                leases
                    .entry(channel_target_key)
                    .or_default()
                    .push(LeaseRow {
                        fact,
                        pubkey: pubkey.to_owned(),
                        state: lease.state,
                        sequence: lease.lease_sequence,
                    });
            }
            KIND_CODING_SESSION_GOAL => keep_newest_by_d_tag(&mut goals, event, fact),
            KIND_CODING_SESSION_NAME
            | KIND_CODING_SESSION_GENERATED_TITLE
            | KIND_CODING_SESSION_GENESIS => names.observe(kind, channel_id, event),
            KIND_CODING_SESSION_CLOSURE => keep_newest_by_d_tag(&mut closures, event, fact),
            _ => {}
        }
    }

    let mut current_by_execution: HashMap<(String, String), u64> = HashMap::new();
    for generation in accepted.values() {
        current_by_execution
            .entry((
                generation.channel_id.clone(),
                generation.execution_key.clone(),
            ))
            .and_modify(|held| *held = (*held).max(generation.target.generation))
            .or_insert(generation.target.generation);
    }

    // The founder each umbrella's accepted creates prove, through the genesis
    // they name by event id. A resume carries no genesis reference.
    let mut founders: HashMap<String, HashSet<String>> = HashMap::new();
    for accepted_generation in accepted.values() {
        let (
            Some(session_ref),
            CodingSessionLifecycleAction::SessionCreate {
                genesis_ref: Some(genesis_ref),
                ..
            },
        ) = (
            accepted_generation.session_ref.as_deref(),
            &accepted_generation.command.payload.action,
        )
        else {
            continue;
        };
        if let Some(founder) =
            names.founder_of(&accepted_generation.channel_id, session_ref, genesis_ref)
        {
            founders
                .entry(session_ref.to_owned())
                .or_default()
                .insert(founder);
        }
    }

    let mut grouped: GroupedSessionGenerations = HashMap::new();
    for accepted_generation in accepted.values() {
        let channel_target_key = (
            accepted_generation.channel_id.clone(),
            accepted_generation.target_key.clone(),
        );
        let observed = metadata.get(&channel_target_key);
        let candidates = leases.get(&channel_target_key).cloned().unwrap_or_default();
        let highest_sequence = candidates.iter().map(|lease| lease.sequence).max();
        let mut highest: Vec<LeaseRow> = candidates
            .into_iter()
            .filter(|lease| Some(lease.sequence) == highest_sequence)
            .collect();
        highest.sort_by(|a, b| a.fact.event_id.cmp(&b.fact.event_id));
        highest.dedup_by(|a, b| a.fact.event_id == b.fact.event_id);
        let winning_lease = (highest.len() == 1).then(|| highest[0].clone());
        let current = current_by_execution.get(&(
            accepted_generation.channel_id.clone(),
            accepted_generation.execution_key.clone(),
        )) == Some(&accepted_generation.target.generation);
        let terminal = observed.is_some_and(|(_, meta)| {
            matches!(
                meta.status,
                SessionStatus::Stopped | SessionStatus::Disconnected
            )
        });
        let lease_expires_at = winning_lease
            .as_ref()
            .map(|lease| lease.fact.created_at.saturating_add(PULSE_LEASE_TTL));
        let lease_is_live = winning_lease.as_ref().is_some_and(|lease| {
            lease.state == CodingSessionLeaseState::Live
                && lease_expires_at.is_some_and(|expires_at| now < expires_at)
        });
        let reachability = if terminal {
            PulseGenerationReachability::Terminal
        } else if current && lease_is_live {
            PulseGenerationReachability::ProviderReachable
        } else {
            PulseGenerationReachability::Unverified
        };
        let mut source_event_ids = vec![
            accepted_generation.command.fact.event_id.clone(),
            accepted_generation.receipt.fact.event_id.clone(),
        ];
        if let Some((fact, _)) = observed {
            source_event_ids.push(fact.event_id.clone());
        }
        source_event_ids.extend(highest.iter().map(|lease| lease.fact.event_id.clone()));
        source_event_ids.sort();
        source_event_ids.dedup();
        let relay_reachable = observed.and_then(|(_, meta)| meta.relay_reachable);
        let generation = PulseDigestGeneration {
            target_key: accepted_generation.target_key.clone(),
            execution_key: accepted_generation.execution_key.clone(),
            provider_authority_pubkey: accepted_generation.provider_authority_pubkey.clone(),
            current,
            reachability,
            status: observed.map(|(_, meta)| session_status_str(meta.status)),
            status_at: observed.map(|(fact, _)| fact.created_at),
            branch: observed.and_then(|(_, meta)| meta.branch.clone()),
            observed_commit: observed.and_then(|(_, meta)| meta.observed_commit.clone()),
            dirty: observed.and_then(|(_, meta)| meta.dirty),
            relay_reachable,
            verified_at: observed.and_then(|(_, meta)| meta.verified_at),
            commit_confirmation: commit_confirmation(relay_reachable).to_owned(),
            lease_state: winning_lease.as_ref().map(|lease| match lease.state {
                CodingSessionLeaseState::Live => "live".to_owned(),
                CodingSessionLeaseState::Released => "released".to_owned(),
            }),
            lease_issued_at: winning_lease.as_ref().map(|lease| lease.fact.created_at),
            lease_accepted_at: None,
            lease_expires_at,
            lease_signer: winning_lease.as_ref().map(|lease| lease.pubkey.clone()),
            lease_source_event_id: winning_lease
                .as_ref()
                .map(|lease| lease.fact.event_id.clone()),
            lease_sequence: highest_sequence,
            lifecycle_command_event_id: accepted_generation.command.fact.event_id.clone(),
            lifecycle_receipt_event_id: accepted_generation.receipt.fact.event_id.clone(),
            source_event_ids,
        };
        let session_key = accepted_generation
            .session_ref
            .clone()
            .unwrap_or_else(|| format!("implicit:{}", accepted_generation.execution_key));
        let session_group = grouped.entry(session_key).or_insert_with(|| {
            (
                accepted_generation.session_ref.clone(),
                HashSet::new(),
                Vec::new(),
            )
        });
        session_group
            .1
            .insert(accepted_generation.channel_id.clone());
        session_group.2.push(generation);
    }

    let mut sessions = Vec::new();
    for (session_key, (session_ref, channels, mut generations)) in grouped {
        generations.sort_by(|a, b| {
            b.current
                .cmp(&a.current)
                .then_with(|| a.execution_key.cmp(&b.execution_key))
                .then_with(|| a.target_key.cmp(&b.target_key))
        });
        let joined = |facts: &HashMap<(String, String), FactRow>| -> Option<(String, String)> {
            if channels.len() != 1 {
                return None;
            }
            let reference = session_ref.as_deref()?;
            let channel_id = channels.iter().next()?;
            let fact = facts.get(&(channel_id.clone(), reference.to_owned()))?;
            Some((fact.event_id.clone(), fact.content.clone()))
        };
        // The same one-channel join as goal and closure, then the shared
        // resolver over that umbrella's executions.
        let name = match (channels.len(), session_ref.as_deref()) {
            (1, Some(reference)) => channels.iter().next().and_then(|channel_id| {
                let executions = generations
                    .iter()
                    .map(|generation| SessionExecutionAuthority {
                        target_key: generation.target_key.clone(),
                        provider_authority_pubkey: generation.provider_authority_pubkey.clone(),
                    })
                    .collect();
                names.resolve(
                    channel_id,
                    reference,
                    founders.get(reference).unwrap_or(&HashSet::new()),
                    executions,
                )
            }),
            _ => None,
        };
        let goal = joined(&goals);
        let closure = joined(&closures);
        let closed = closure.as_ref().is_some_and(|(_, content)| {
            decode_coding_session_closure(content).is_ok_and(|payload| payload.action.is_closed())
        });
        let coordination_state = if closed {
            "closed"
        } else if generations.iter().any(|generation| {
            generation.current
                && generation.reachability == PulseGenerationReachability::ProviderReachable
        }) {
            "provider_reachable"
        } else {
            "open_unverified"
        };
        let latest_observation_at = generations
            .iter()
            .filter_map(|generation| generation.status_at)
            .max();
        let mut source_event_ids: Vec<String> = generations
            .iter()
            .flat_map(|generation| generation.source_event_ids.clone())
            .collect();
        for (event_id, _) in [&goal, &closure].into_iter().flatten() {
            source_event_ids.push(event_id.clone());
        }
        if let Some(event_id) = name.as_ref().and_then(|(_, event_id)| event_id.clone()) {
            source_event_ids.push(event_id);
        }
        source_event_ids.sort();
        source_event_ids.dedup();
        sessions.push(PulseDigestSession {
            session_key,
            session_ref,
            name: name.as_ref().map(|(resolved, _)| resolved.name.clone()),
            name_origin: name.as_ref().map(|(resolved, _)| {
                match resolved.origin {
                    SessionDisplayNameOrigin::Person => "person",
                    SessionDisplayNameOrigin::Generated => "generated",
                    SessionDisplayNameOrigin::Fallback => "fallback",
                }
                .to_owned()
            }),
            name_model: name
                .as_ref()
                .and_then(|(resolved, _)| resolved.model.clone()),
            name_signer: name
                .as_ref()
                .and_then(|(resolved, _)| resolved.signer_pubkey.clone()),
            goal: goal.map(|(_, content)| content),
            lifecycle: if closed { "closed" } else { "open" }.to_owned(),
            coordination_state: coordination_state.to_owned(),
            latest_observation_at,
            observed_age_seconds: latest_observation_at.map(|observed| now - observed),
            generations,
            source_event_ids,
        });
    }
    sessions.sort_by(|a, b| {
        b.latest_observation_at
            .cmp(&a.latest_observation_at)
            .then_with(|| a.session_key.cmp(&b.session_key))
    });
    sessions
}

/// Keep the newest `(created_at, event id)` fact per key, alongside its decoded
/// payload.
fn keep_newest_pair<T>(
    facts: &mut HashMap<(String, String), (FactRow, T)>,
    key: (String, String),
    fact: FactRow,
    payload: T,
) {
    match facts.get(&key) {
        Some((held, _))
            if !is_newer(
                fact.created_at,
                &fact.event_id,
                held.created_at,
                &held.event_id,
            ) => {}
        _ => {
            facts.insert(key, (fact, payload));
        }
    }
}

/// Keep the newest `(created_at, event id)` fact per `d` tag — the session
/// UUID that 44227/44230 are keyed by. A 44229 name is not: it is resolved
/// with its signer's standing, in [`names`].
fn keep_newest_by_d_tag(
    facts: &mut HashMap<(String, String), FactRow>,
    event: &Value,
    fact: FactRow,
) {
    let Some(session_ref) = json_tag_value(event, "d") else {
        return;
    };
    let key = (fact.channel_id.clone(), session_ref.to_owned());
    match facts.get(&key) {
        Some(held)
            if !is_newer(
                fact.created_at,
                &fact.event_id,
                held.created_at,
                &held.event_id,
            ) => {}
        _ => {
            facts.insert(key, fact);
        }
    }
}

/// The wire spelling of a session status.
fn session_status_str(status: SessionStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The fixed tri-state commit line. Unknown is never converted to false.
/// Return the fixed honest display text for a tri-state commit observation.
pub fn commit_confirmation(relay_reachable: Option<bool>) -> &'static str {
    match relay_reachable {
        Some(true) => COMMIT_CONFIRMED,
        Some(false) => COMMIT_NOT_FOUND,
        None => COMMIT_NOT_CHECKED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOLD_VECTORS: &str =
        include_str!("../../../conformance/project-pulse-fold/fixtures/fold-vectors.json");

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FoldVectorFile {
        digest_schema: String,
        vectors: Vec<FoldVector>,
    }

    #[derive(Deserialize)]
    struct FoldVector {
        name: String,
        input: FoldVectorInput,
        expected: Value,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FoldVectorInput {
        project: String,
        now: i64,
        source_errors: Vec<PulseDigestError>,
        events: Vec<Value>,
    }

    #[test]
    fn fold_matches_every_shared_conformance_vector_byte_for_byte() {
        let file: FoldVectorFile = serde_json::from_str(FOLD_VECTORS).expect("vectors parse");
        assert_eq!(file.digest_schema, PULSE_DIGEST_SCHEMA);
        assert!(!file.vectors.is_empty());
        for vector in file.vectors {
            let actual = fold_pulse_digest(
                &vector.input.project,
                vector.input.now,
                vector.input.source_errors,
                &vector.input.events,
            );
            let expected: PulseDigest =
                serde_json::from_value(vector.expected).expect("expected digest decodes");
            assert_eq!(
                serde_json::to_string(&actual).expect("actual serializes"),
                serde_json::to_string(&expected).expect("expected serializes"),
                "vector {}",
                vector.name
            );
        }
    }
}
