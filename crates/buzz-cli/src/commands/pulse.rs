//! `buzz pulse` — Project Pulse: the explicit coordination entries a project's
//! workers publish (kind 44240) folded together with the coding-session facts
//! their providers already observe (44223/44227/44229/44230).
//!
//! Three rules shape this module, and each of them is a rule about honesty
//! rather than convenience:
//!
//! 1. **A read failure is never a quiet project.** Every failed source query
//!    lands in the digest's `errors[]`, flips `complete` to false, and exits
//!    non-zero. A partial fold must never print as a complete digest with an
//!    empty session list.
//! 2. **The absence of a closure is not evidence of life.** A session is
//!    Active work only on a positive freshness signal — see [`session_activity`].
//! 3. **A peer can never quietly retract your claim.** Supersession is honored
//!    only within one author, so nobody can push another author's `blocker` out
//!    of the set that drives the `wait | consult | proceed` advisory.
//!
//! The fold's single source of truth is `conformance/project-pulse-fold/`;
//! [`fold_pulse_digest`] is bound to those vectors by a test at the bottom of
//! this file, and the Desktop and (Slice 2) relay folds bind to the same ones.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use buzz_core::coding_session_closure::{
    decode_coding_session_closure, CodingSessionClosureAction,
};
use buzz_core::coding_session_command::coding_session_target_key;
use buzz_core::coding_session_payload::{
    decode_coding_session_metadata, SessionMetadata, SessionStatus,
};
use buzz_core::kind::{
    normalize_project_coordinate, KIND_CODING_SESSION_CLOSURE, KIND_CODING_SESSION_GOAL,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_NAME,
    KIND_NIP29_GROUP_METADATA, KIND_PROJECT, KIND_PROJECT_MEMBERS, KIND_PULSE_ENTRY,
};
use buzz_core::pulse::{
    pulse_entry_project_coordinate, validate_pulse_entry_envelope, PulseEntry, PulseEntryType,
    MAX_PULSE_TEXT_BYTES, PULSE_ENTRY_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;

// ── Wire constants ───────────────────────────────────────────────────────────

/// How recent the newest 44223 observation must be for a session to count as
/// Active work, in seconds. Pinned by
/// `conformance/project-pulse-fold/fixtures/fold-vectors.json`
/// (`activeWindowSeconds`); exactly `PULSE_ACTIVE_WINDOW` seconds old is still
/// active, one second more is not.
pub const PULSE_ACTIVE_WINDOW: i64 = 30 * 60;

/// Schema of the digest envelope — the kind-39011 content object of the plan's
/// §6, which Slice 1 already emits verbatim so Slice 2 can change only *who*
/// computes it.
const PULSE_DIGEST_SCHEMA: &str = "buzz-project-pulse-digest/v1";

/// `source` value for a digest this client folded itself. The relay-signed
/// 39011 of Slice 2 substitutes `relay-digest` and changes nothing else.
const PULSE_DIGEST_SOURCE: &str = "client-composed";

/// How far the session scan reaches. There is no queryable "sessions of this
/// project" relation, and a community-wide 44223 scan is both unbounded and a
/// leak, so sessions are reached through the project's channels only — a
/// session running in a channel outside that set is not discoverable in v1 and
/// no surface may present the Active-work list as exhaustive.
const PULSE_SESSIONS_SCOPE: &str = "project channels";

/// The three `commitConfirmation` strings. Fixed here and mirrored in Desktop
/// so the two surfaces cannot drift. Never render "relay reachable": the fact
/// is that the relay's advertised refs contained this exact commit at
/// `verifiedAt`, which says nothing about whether the session is connected.
const COMMIT_CONFIRMED: &str = "Commit confirmed on relay";
const COMMIT_NOT_FOUND: &str = "Commit not found on relay";
const COMMIT_NOT_CHECKED: &str = "Commit not checked";

/// The reserved `--branch` value selecting rows that carry no branch at all.
const NO_BRANCH: &str = "-";

/// Session facts fetched per project channel. 44224 is fetched with the rest
/// so one round trip carries the whole coding-session surface of a channel,
/// matching `sessions.rs`'s existing shape; the v1 fold reads 44223/44227/
/// 44229/44230 and ignores the receipts.
const SESSION_FACT_KINDS: [u32; 5] = [
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_GOAL,
    KIND_CODING_SESSION_NAME,
    KIND_CODING_SESSION_CLOSURE,
];

/// Upper bound on the channel-metadata scan used to resolve a project's
/// channel set, matching `channels list`'s own default page budget.
const CHANNEL_SCAN_LIMIT: u32 = 500;

/// Upper bound on the project heads a bare-dtag resolution will consider.
const PROJECT_SCAN_LIMIT: u32 = 500;

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

/// One folded coding session, keyed by its `cs-target` key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PulseDigestSession {
    /// `coding-session/v1|<len>:<driver>…` — the target key of one generation.
    pub target_key: String,
    /// The umbrella session UUID the goal/name/closure kinds join on.
    pub session_ref: Option<String>,
    /// Newest 44229, or `null`.
    pub name: Option<String>,
    /// Newest 44227, or `null`.
    pub goal: Option<String>,
    /// The winning 44223's status.
    pub status: String,
    /// That 44223's `created_at`.
    pub status_at: i64,
    /// Whether the newest 44230 closed the umbrella.
    pub closed: bool,
    /// `active` only on a positive freshness signal; see [`session_activity`].
    pub activity: String,
    /// Observed branch, or `null`. A null branch is its own group.
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
    /// `now − statusAt`. Never derived from `verifiedAt`.
    pub observed_age_seconds: i64,
    /// Every event folded into this row, ascending.
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

/// Every value of the named tag on a relay event.
fn json_tag_values<'a>(event: &'a Value, name: &str) -> Vec<&'a str> {
    let Some(tags) = event.get("tags").and_then(Value::as_array) else {
        return Vec::new();
    };
    tags.iter()
        .filter_map(Value::as_array)
        .filter(|parts| parts.first().and_then(Value::as_str) == Some(name))
        .filter_map(|parts| parts.get(1))
        .filter_map(Value::as_str)
        .collect()
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
/// `crates/buzz-core/src/coding_session_closure.rs:148-156`, so two events can
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
struct FactRow {
    event_id: String,
    created_at: i64,
    content: String,
}

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
fn fold_sessions(project: &str, now: i64, events: &[Value]) -> Vec<PulseDigestSession> {
    let mut metadata: HashMap<String, (FactRow, SessionMetadata)> = HashMap::new();
    let mut goals: HashMap<String, FactRow> = HashMap::new();
    let mut names: HashMap<String, FactRow> = HashMap::new();
    let mut closures: HashMap<String, FactRow> = HashMap::new();

    for event in events {
        let (Some(kind), Some(event_id), Some(created_at)) = (
            json_kind(event),
            json_str(event, "id"),
            json_created_at(event),
        ) else {
            continue;
        };
        let content = json_str(event, "content").unwrap_or_default();
        let fact = FactRow {
            event_id: event_id.to_owned(),
            created_at,
            content: content.to_owned(),
        };
        match kind {
            KIND_CODING_SESSION_METADATA => {
                let Ok(meta) = decode_coding_session_metadata(content) else {
                    continue;
                };
                let matches_project = meta
                    .project_ref
                    .as_deref()
                    .and_then(normalize_project_coordinate)
                    .is_some_and(|coordinate| coordinate == project);
                if !matches_project {
                    continue;
                }
                let key = coding_session_target_key(&meta.session);
                keep_newest_pair(&mut metadata, key, fact, meta);
            }
            KIND_CODING_SESSION_GOAL => keep_newest_by_d_tag(&mut goals, event, fact),
            KIND_CODING_SESSION_NAME => keep_newest_by_d_tag(&mut names, event, fact),
            KIND_CODING_SESSION_CLOSURE => keep_newest_by_d_tag(&mut closures, event, fact),
            _ => {}
        }
    }

    let mut sessions: Vec<PulseDigestSession> = metadata
        .into_iter()
        .map(|(target_key, (fact, meta))| {
            let mut source_event_ids = vec![fact.event_id.clone()];
            let session_ref = meta.session_ref.clone();
            let joined = |facts: &HashMap<String, FactRow>| -> Option<(String, String)> {
                let reference = session_ref.as_deref()?;
                let fact = facts.get(reference)?;
                Some((fact.event_id.clone(), fact.content.clone()))
            };
            let name = joined(&names);
            let goal = joined(&goals);
            let closure = joined(&closures);
            for (event_id, _) in [&name, &goal, &closure].into_iter().flatten() {
                source_event_ids.push(event_id.clone());
            }
            source_event_ids.sort();
            let closed = closure.as_ref().is_some_and(|(_, content)| {
                decode_coding_session_closure(content)
                    .is_ok_and(|payload| payload.action == CodingSessionClosureAction::Closed)
            });
            PulseDigestSession {
                target_key,
                session_ref: meta.session_ref.clone(),
                name: name.map(|(_, content)| content),
                goal: goal.map(|(_, content)| content),
                status: session_status_str(meta.status),
                status_at: fact.created_at,
                closed,
                activity: session_activity(meta.status, closed, now - fact.created_at).to_owned(),
                branch: meta.branch.clone(),
                observed_commit: meta.observed_commit.clone(),
                dirty: meta.dirty,
                relay_reachable: meta.relay_reachable,
                verified_at: meta.verified_at,
                commit_confirmation: commit_confirmation(meta.relay_reachable).to_owned(),
                observed_age_seconds: now - fact.created_at,
                source_event_ids,
            }
        })
        .collect();

    sessions.sort_by(|a, b| {
        b.status_at
            .cmp(&a.status_at)
            .then_with(|| a.target_key.cmp(&b.target_key))
    });
    sessions
}

/// Keep the newest `(created_at, event id)` fact per key, alongside its decoded
/// payload.
fn keep_newest_pair<T>(
    facts: &mut HashMap<String, (FactRow, T)>,
    key: String,
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
/// UUID that 44227/44229/44230 are keyed by.
fn keep_newest_by_d_tag(facts: &mut HashMap<String, FactRow>, event: &Value, fact: FactRow) {
    let Some(session_ref) = json_tag_value(event, "d") else {
        return;
    };
    match facts.get(session_ref) {
        Some(held)
            if !is_newer(
                fact.created_at,
                &fact.event_id,
                held.created_at,
                &held.event_id,
            ) => {}
        _ => {
            facts.insert(session_ref.to_owned(), fact);
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

/// Active work requires a **positive freshness signal**, not merely the absence
/// of a closure.
///
/// A machine that dies mid-turn leaves its last 44223 saying `running` forever,
/// and a session whose provider identity is gone can never publish a closure;
/// answering "who is actively working on this project?" with that ghost would
/// tell a new worker to wait on it indefinitely. All three must hold: the
/// umbrella is not closed, the newest status is one a live session reports, and
/// that status is no older than [`PULSE_ACTIVE_WINDOW`].
fn session_activity(status: SessionStatus, closed: bool, age_seconds: i64) -> &'static str {
    let live_status = matches!(
        status,
        SessionStatus::Starting
            | SessionStatus::Running
            | SessionStatus::Idle
            | SessionStatus::WaitingForInput
    );
    if !closed && live_status && age_seconds <= PULSE_ACTIVE_WINDOW {
        "active"
    } else {
        "stale"
    }
}

/// The fixed tri-state commit line. Unknown is never converted to false.
fn commit_confirmation(relay_reachable: Option<bool>) -> &'static str {
    match relay_reachable {
        Some(true) => COMMIT_CONFIRMED,
        Some(false) => COMMIT_NOT_FOUND,
        None => COMMIT_NOT_CHECKED,
    }
}

/// `--branch` semantics, identical for `list`, `digest`, and Desktop's chips:
/// a named filter matches byte-exactly and never returns null-branch rows, the
/// reserved `-` returns only null-branch rows, and no filter returns
/// everything.
fn branch_matches(filter: Option<&str>, branch: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(NO_BRANCH) => branch.is_none(),
        Some(name) => branch == Some(name),
    }
}

// ── Relay reads ──────────────────────────────────────────────────────────────

/// The Pulse-entry filter: kind 44240 alone, scoped to exactly one project
/// coordinate.
///
/// 44240 is always queried in its own filter. Mixing it with the session kinds
/// "for efficiency" satisfies the bridge's `#a` rule and still returns only
/// 44240 events — 44223 carries no `a` tag — so the caller gets an empty
/// session list with no error.
fn entries_filter(coordinate: &str, since: Option<u64>) -> Value {
    let mut filter = json!({ "kinds": [KIND_PULSE_ENTRY], "#a": [coordinate] });
    if let Some(since) = since {
        filter["since"] = json!(since);
    }
    filter
}

/// The per-channel session-fact filter. `kinds` is never omitted: an
/// open-ended filter trips the relay's p-gate and comes back 403.
fn session_facts_filter(channel_id: &str) -> Value {
    json!({ "kinds": SESSION_FACT_KINDS, "#h": [channel_id] })
}

/// Fetch a project's Pulse entries, reporting whether `--limit` truncated them.
async fn fetch_entry_events(
    client: &BuzzClient,
    coordinate: &str,
    since: Option<u64>,
    limit: Option<u32>,
) -> Result<(Vec<Value>, bool), CliError> {
    let filter = entries_filter(coordinate, since);
    match limit {
        Some(limit) => {
            let events = client.query_paginated(filter, limit).await?;
            let truncated = events.len() as u32 >= limit;
            Ok((events, truncated))
        }
        None => Ok((client.query_all(filter).await?, false)),
    }
}

/// Resolve the channels a project's coding sessions can be reached through:
/// the channels whose kind:39000 metadata carries this project's coordinate,
/// plus the `channel` references on the project head itself.
/// Resolve the project's channel set, plus whether the metadata scan hit
/// [`CHANNEL_SCAN_LIMIT`].
///
/// The truncation flag is not cosmetic: `query_paginated` stops as soon as it
/// has `limit` events, so in a community with 500+ kind:39000 events the scan
/// silently returns one page and every project channel outside it is never
/// queried for session facts. A digest that then printed `"complete": true`
/// with an empty `sessions` list would tell an agent the project is quiet
/// while a live session runs in a channel it never looked at.
async fn project_channel_ids(
    client: &BuzzClient,
    coordinate: &str,
) -> Result<(Vec<String>, bool), CliError> {
    let mut channels: HashSet<String> = HashSet::new();

    if let Some((owner, dtag)) = split_project_coordinate(coordinate) {
        let head = client
            .query(&json!({
                "kinds": [KIND_PROJECT],
                "authors": [owner],
                "#d": [dtag],
                "limit": 1,
            }))
            .await?;
        let heads: Vec<Value> = serde_json::from_str(&head)
            .map_err(|e| CliError::Other(format!("failed to parse relay response: {e}")))?;
        for head in &heads {
            for channel in json_tag_values(head, "channel") {
                channels.insert(channel.to_owned());
            }
            if let Some(channel) = json_tag_value(head, "buzz-channel") {
                channels.insert(channel.to_owned());
            }
        }
    }

    let metadata = client
        .query_paginated(
            json!({ "kinds": [KIND_NIP29_GROUP_METADATA] }),
            CHANNEL_SCAN_LIMIT,
        )
        .await?;
    for event in &metadata {
        // Normalize before comparing: the relay's `validate_project_ref_tag`
        // accepts any `is_ascii_hexdigit()` pubkey, so `channels.project_ref` —
        // and the 39000 `project` tag projected from it — can hold an
        // upper-case-hex coordinate. A byte-exact match would drop that
        // channel from the set, never query its session facts, and still print
        // `"complete": true` — the same quiet-project lie the truncation guard
        // above exists to prevent. Desktop already normalizes here.
        if json_tag_value(event, "project")
            .and_then(normalize_project_coordinate)
            .as_deref()
            == Some(coordinate)
        {
            if let Some(channel) = json_tag_value(event, "d") {
                channels.insert(channel.to_owned());
            }
        }
    }

    let truncated = metadata.len() as u32 >= CHANNEL_SCAN_LIMIT;

    let mut channels: Vec<String> = channels.into_iter().collect();
    channels.sort();
    Ok((channels, truncated))
}

/// The events and failures of one project's session scan.
struct SessionScan {
    events: Vec<Value>,
    errors: Vec<PulseDigestError>,
    /// The first real transport failure, kept so the caller can exit on the
    /// relay's own error rather than a manufactured one.
    failure: Option<CliError>,
}

/// Fetch every coding-session fact reachable through the project's channels.
///
/// A channel that fails is recorded and the scan continues: a partial read is
/// reported as partial, never silently narrowed to the channels that answered.
async fn scan_project_sessions(client: &BuzzClient, coordinate: &str) -> SessionScan {
    let mut scan = SessionScan {
        events: Vec::new(),
        errors: Vec::new(),
        failure: None,
    };
    let channels = match project_channel_ids(client, coordinate).await {
        Ok((channels, truncated)) => {
            if truncated {
                // A limit condition, not a relay failure: recorded in
                // `errors[]` (so `complete` goes false) while `scan.failure`
                // stays `None`, exactly as the entries read does.
                scan.errors.push(PulseDigestError {
                    scope: "channels".to_owned(),
                    message: format!(
                        "channel scan truncated at {CHANNEL_SCAN_LIMIT} events; some project channels were not queried"
                    ),
                });
            }
            channels
        }
        Err(error) => {
            scan.errors.push(PulseDigestError {
                scope: "channels".to_owned(),
                message: error.to_string(),
            });
            scan.failure = Some(error);
            return scan;
        }
    };
    for channel in channels {
        match client.query_all(session_facts_filter(&channel)).await {
            Ok(events) => scan.events.extend(events),
            Err(error) => {
                scan.errors.push(PulseDigestError {
                    scope: format!("sessions:{channel}"),
                    message: error.to_string(),
                });
                if scan.failure.is_none() {
                    scan.failure = Some(error);
                }
            }
        }
    }
    scan
}

/// Compose a full digest from the relay: entries by `#a`, sessions by `#h`.
///
/// Returns the digest plus the first transport failure, if any, so the caller
/// can print the (partial) digest *and* exit on the relay's own error.
async fn compose_digest(
    client: &BuzzClient,
    coordinate: &str,
    limit: Option<u32>,
) -> (PulseDigest, Option<CliError>) {
    let mut events: Vec<Value> = Vec::new();
    let mut errors: Vec<PulseDigestError> = Vec::new();
    let mut failure: Option<CliError> = None;

    match fetch_entry_events(client, coordinate, None, limit).await {
        Ok((entries, truncated)) => {
            events.extend(entries);
            if truncated {
                errors.push(PulseDigestError {
                    scope: "entries".to_owned(),
                    message: format!(
                        "entry read truncated at --limit {}; more entries may exist",
                        limit.unwrap_or_default()
                    ),
                });
            }
        }
        Err(error) => {
            errors.push(PulseDigestError {
                scope: "entries".to_owned(),
                message: error.to_string(),
            });
            failure = Some(error);
        }
    }

    let scan = scan_project_sessions(client, coordinate).await;
    events.extend(scan.events);
    errors.extend(scan.errors);
    if failure.is_none() {
        failure = scan.failure;
    }

    // `asOf` is the wall clock read after the last source query returned.
    let now = chrono::Utc::now().timestamp();
    (fold_pulse_digest(coordinate, now, errors, &events), failure)
}

/// The error a partial digest exits with.
///
/// A transport failure exits on the relay's own error (2, or 3 for an auth
/// status). A read the caller's own `--limit` truncated has no underlying
/// error, and is reported as the usage condition it is — raise `--limit` — so
/// the CLI never manufactures a relay status the relay never returned.
fn partial_digest_error(failure: Option<CliError>) -> CliError {
    failure.unwrap_or_else(|| {
        CliError::Usage(
            "digest is incomplete: the entry read was truncated by --limit; \
             raise --limit for a complete digest"
                .to_owned(),
        )
    })
}

// ── Project resolution ───────────────────────────────────────────────────────

/// Split a canonical coordinate into `(owner-hex, dtag)`.
fn split_project_coordinate(coordinate: &str) -> Option<(&str, &str)> {
    let mut parts = coordinate.splitn(3, ':');
    let kind = parts.next()?;
    let owner = parts.next()?;
    let dtag = parts.next()?;
    (kind == "30621").then_some((owner, dtag))
}

/// Resolve `--project` / `BUZZ_PULSE_PROJECT` into a canonical coordinate.
///
/// Precedence is the explicit flag, then the environment variable (clap
/// applies it), then an actionable usage error. A value containing `:` must
/// already be a full `30621:<owner-hex>:<dtag>` coordinate; a bare dtag
/// resolves only when exactly one **visible** project matches, because
/// guessing an owner would silently write another person's project.
async fn resolve_project(client: &BuzzClient, project: Option<&str>) -> Result<String, CliError> {
    if let Some(coordinate) = direct_project_coordinate(project)? {
        return Ok(coordinate);
    }
    // `direct_project_coordinate` returns `None` only for a bare dtag, and only
    // after proving the argument was supplied.
    let dtag = project.unwrap_or_default();
    let candidates = visible_project_coordinates(client, dtag).await?;
    resolve_bare_dtag(dtag, candidates)
}

/// The half of project resolution that needs no relay: a missing argument is an
/// actionable usage error, a value containing `:` must already be a canonical
/// coordinate, and a bare dtag returns `None` for the caller to look up.
fn direct_project_coordinate(project: Option<&str>) -> Result<Option<String>, CliError> {
    let Some(project) = project else {
        return Err(CliError::Usage(
            "--project is required: pass a `30621:<owner-hex>:<dtag>` coordinate or a project \
             dtag, or set BUZZ_PULSE_PROJECT"
                .to_owned(),
        ));
    };
    if !project.contains(':') {
        return Ok(None);
    }
    normalize_project_coordinate(project)
        .map(Some)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "--project must be a `30621:<owner-hex>:<dtag>` coordinate or a project dtag \
                 (got {project:?})"
            ))
        })
}

/// Every project coordinate visible to the caller whose dtag equals `dtag`.
///
/// The three queries are the whole visible set: projects the caller owns, the
/// roster projections naming the caller, and the heads those projections point
/// at. `projects list` cannot answer this — it always queries one explicit
/// author.
async fn visible_project_coordinates(
    client: &BuzzClient,
    dtag: &str,
) -> Result<Vec<String>, CliError> {
    let self_hex = client.keys().public_key().to_hex();
    let mut heads = client
        .query_paginated(
            json!({ "kinds": [KIND_PROJECT], "authors": [self_hex] }),
            PROJECT_SCAN_LIMIT,
        )
        .await?;

    let rosters = client
        .query_paginated(
            json!({ "kinds": [KIND_PROJECT_MEMBERS], "#p": [self_hex] }),
            PROJECT_SCAN_LIMIT,
        )
        .await?;
    let mut owners: Vec<String> = rosters
        .iter()
        .filter_map(|event| json_tag_value(event, "d"))
        .filter_map(normalize_project_coordinate)
        .filter_map(|coordinate| {
            split_project_coordinate(&coordinate).map(|(owner, _)| owner.to_owned())
        })
        .collect();
    owners.sort();
    owners.dedup();
    owners.retain(|owner| *owner != self_hex);
    if !owners.is_empty() {
        heads.extend(
            client
                .query_paginated(
                    json!({ "kinds": [KIND_PROJECT], "authors": owners }),
                    PROJECT_SCAN_LIMIT,
                )
                .await?,
        );
    }

    let mut coordinates: Vec<String> = heads
        .iter()
        .filter(|event| json_tag_value(event, "d") == Some(dtag))
        .filter_map(|event| {
            let author = json_str(event, "pubkey")?;
            normalize_project_coordinate(&format!("30621:{author}:{dtag}"))
        })
        .collect();
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

/// Pick the one visible project a bare dtag names, or say why it cannot.
fn resolve_bare_dtag(dtag: &str, candidates: Vec<String>) -> Result<String, CliError> {
    match candidates.len() {
        1 => Ok(candidates.into_iter().next().unwrap_or_default()),
        0 => Err(CliError::Usage(format!("no visible project named {dtag}"))),
        _ => Err(CliError::Usage(format!(
            "project dtag {dtag} is ambiguous — pass one of these coordinates to --project: {}",
            candidates.join(", ")
        ))),
    }
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// Build the entry payload from the command-line arguments.
///
/// Every rejection here happens **before** signing: an invalid code area, an
/// over-cap text, or a malformed `supersedes` is a usage error the caller can
/// fix, never a signed event the relay refuses.
fn build_entry(
    kind: crate::PulseKindArg,
    text: String,
    areas: Option<&str>,
    branch: Option<String>,
    supersedes: Option<String>,
) -> Result<PulseEntry, CliError> {
    if text.len() > MAX_PULSE_TEXT_BYTES {
        return Err(CliError::Usage(format!(
            "--content is {} bytes; the maximum is {MAX_PULSE_TEXT_BYTES}",
            text.len()
        )));
    }
    let entry_type = PulseEntryType::from_str(kind.as_str()).map_err(CliError::Usage)?;
    let code_areas: Vec<String> = match areas {
        // An empty `--areas` claims nothing; a stray comma yields an empty
        // path, which buzz-core rejects rather than silently dropping.
        Some(areas) if areas.trim().is_empty() => Vec::new(),
        Some(areas) => areas
            .split(',')
            .map(|area| area.trim().to_owned())
            .collect(),
        None => Vec::new(),
    };
    Ok(PulseEntry {
        schema: PULSE_ENTRY_SCHEMA.to_owned(),
        entry_type,
        text,
        code_areas,
        branch,
        supersedes,
    })
}

/// `buzz pulse update` — publish one entry.
#[allow(clippy::too_many_arguments)]
async fn cmd_update(
    client: &BuzzClient,
    project: Option<&str>,
    kind: crate::PulseKindArg,
    areas: Option<&str>,
    branch: Option<String>,
    session: Option<&str>,
    supersedes: Option<String>,
    content: &str,
) -> Result<(), CliError> {
    let coordinate = resolve_project(client, project).await?;
    let text = crate::validate::read_or_stdin(content)?;
    let entry = build_entry(kind, text, areas, branch, supersedes)?;
    let builder = buzz_sdk::builders::build_pulse_entry(&coordinate, &entry, None, session)
        .map_err(crate::validate::sdk_err)?;

    // Signed verbatim, not through `sign_event`: 44240's tag grammar is a
    // closed key set, so the NIP-OA `auth` tag that method injects would be
    // rejected at ingest. Membership delegation still travels with the request
    // — `submit_event` attaches the same tag as the `x-auth-tag` header.
    let event = client.sign_event_unchecked(builder)?;
    let event_id = event.id.to_hex();
    let created_at = event.created_at.as_secs();
    let raw = client.submit_event(event).await?;
    let response: Value = serde_json::from_str(&raw)
        .map_err(|e| CliError::Other(format!("relay response is not JSON: {e} ({raw})")))?;
    let accepted = response
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !accepted {
        let message = response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(CliError::Other(format!(
            "relay rejected pulse entry: {message}"
        )));
    }
    println!(
        "{}",
        json!({
            "event_id": response.get("event_id").and_then(Value::as_str).unwrap_or(&event_id),
            "accepted": accepted,
            "project": coordinate,
            "kind": kind.as_str(),
            "created_at": created_at,
        })
    );
    Ok(())
}

/// One row of `buzz pulse list` — an unfolded entry with its signature
/// stripped.
fn list_row(entry: &PulseDigestEntry, format: &crate::OutputFormat) -> Value {
    match format {
        crate::OutputFormat::Compact => json!({
            "eventId": entry.event_id,
            "createdAt": entry.created_at,
            "type": entry.entry_type,
            "text": entry.text,
            "branch": entry.branch,
        }),
        crate::OutputFormat::Json => json!({
            "eventId": entry.event_id,
            "pubkey": entry.pubkey,
            "createdAt": entry.created_at,
            "type": entry.entry_type,
            "text": entry.text,
            "claimedAreas": entry.claimed_areas,
            "branch": entry.branch,
            "sessionRef": entry.session_ref,
            "supersedes": entry.supersedes,
        }),
    }
}

/// `buzz pulse list` — unfolded entries, newest first.
async fn cmd_list(
    client: &BuzzClient,
    project: Option<&str>,
    since: Option<u64>,
    kind: Option<crate::PulseKindArg>,
    branch: Option<&str>,
    limit: Option<u32>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let coordinate = resolve_project(client, project).await?;
    let (events, _) = fetch_entry_events(client, &coordinate, since, limit).await?;
    let now = chrono::Utc::now().timestamp();
    let digest = fold_pulse_digest(&coordinate, now, Vec::new(), &events);
    let rows: Vec<Value> = digest
        .entries
        .iter()
        .filter(|entry| branch_matches(branch, entry.branch.as_deref()))
        .filter(|entry| kind.is_none_or(|kind| entry.entry_type == kind.as_str()))
        .map(|entry| list_row(entry, format))
        .collect();
    println!("{}", Value::Array(rows));
    Ok(())
}

/// One row of `buzz pulse sessions`.
fn session_row(session: &PulseDigestSession, format: &crate::OutputFormat) -> Value {
    match format {
        crate::OutputFormat::Compact => json!({
            "targetKey": session.target_key,
            "name": session.name,
            "status": session.status,
            "activity": session.activity,
            "branch": session.branch,
            "observedAgeSeconds": session.observed_age_seconds,
        }),
        crate::OutputFormat::Json => serde_json::to_value(session).unwrap_or(Value::Null),
    }
}

/// `buzz pulse sessions` — the coding sessions observed in the project's
/// channels.
///
/// The list is printed even when a channel failed, and the command still exits
/// non-zero in that case: the scope of a Pulse session list is "project
/// channels", and a channel that could not be read is disclosed rather than
/// quietly narrowing the answer.
async fn cmd_sessions(
    client: &BuzzClient,
    project: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let coordinate = resolve_project(client, project).await?;
    let scan = scan_project_sessions(client, &coordinate).await;
    let now = chrono::Utc::now().timestamp();
    let sessions = fold_sessions(&coordinate, now, &scan.events);
    let rows: Vec<Value> = sessions
        .iter()
        .map(|session| session_row(session, format))
        .collect();
    println!("{}", Value::Array(rows));
    if !scan.errors.is_empty() {
        return Err(partial_digest_error(scan.failure));
    }
    Ok(())
}

/// `buzz pulse digest` — the §6 envelope, printed verbatim.
async fn cmd_digest(
    client: &BuzzClient,
    project: Option<&str>,
    branch: Option<&str>,
    limit: Option<u32>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let coordinate = resolve_project(client, project).await?;
    let (mut digest, failure) = compose_digest(client, &coordinate, limit).await;
    digest
        .entries
        .retain(|entry| branch_matches(branch, entry.branch.as_deref()));
    digest
        .sessions
        .retain(|session| branch_matches(branch, session.branch.as_deref()));

    let rendered = match format {
        // Both formats print `source`; compact drops the two constants a
        // reader already knows and the per-row detail, never a fact.
        crate::OutputFormat::Compact => compact_digest(&digest),
        crate::OutputFormat::Json => serde_json::to_value(&digest).unwrap_or(Value::Null),
    };
    println!("{rendered}");

    if digest.complete {
        Ok(())
    } else {
        Err(partial_digest_error(failure))
    }
}

/// The `--format compact` digest: the same facts, fewer per-row fields.
fn compact_digest(digest: &PulseDigest) -> Value {
    json!({
        "source": digest.source,
        "project": digest.project,
        "asOf": digest.as_of,
        "complete": digest.complete,
        "sessionsScope": digest.sessions_scope,
        "sessions": digest.sessions.iter().map(|session| json!({
            "targetKey": session.target_key,
            "name": session.name,
            "status": session.status,
            "activity": session.activity,
            "branch": session.branch,
            "commitConfirmation": session.commit_confirmation,
            "observedAgeSeconds": session.observed_age_seconds,
        })).collect::<Vec<Value>>(),
        "entries": digest.entries.iter().map(|entry| json!({
            "eventId": entry.event_id,
            "pubkey": entry.pubkey,
            "createdAt": entry.created_at,
            "type": entry.entry_type,
            "text": entry.text,
            "branch": entry.branch,
            "active": entry.active,
        })).collect::<Vec<Value>>(),
        "errors": digest.errors,
    })
}

/// Route a `buzz pulse` subcommand.
pub async fn dispatch(
    cmd: crate::PulseCmd,
    client: &BuzzClient,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    use crate::PulseCmd;
    match cmd {
        PulseCmd::Update {
            project,
            kind,
            areas,
            branch,
            session,
            supersedes,
            content,
        } => {
            cmd_update(
                client,
                project.as_deref(),
                kind,
                areas.as_deref(),
                branch,
                session.as_deref(),
                supersedes,
                &content,
            )
            .await
        }
        PulseCmd::List {
            project,
            since,
            kind,
            branch,
            limit,
        } => {
            cmd_list(
                client,
                project.as_deref(),
                since,
                kind,
                branch.as_deref(),
                limit,
                format,
            )
            .await
        }
        PulseCmd::Sessions { project } => cmd_sessions(client, project.as_deref(), format).await,
        PulseCmd::Digest {
            project,
            branch,
            limit,
        } => cmd_digest(client, project.as_deref(), branch.as_deref(), limit, format).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::exit_code;

    /// The conformance corpus, loaded from the tree so the Rust fold and the
    /// Desktop fold cannot drift apart silently.
    const FOLD_VECTORS: &str =
        include_str!("../../../../conformance/project-pulse-fold/fixtures/fold-vectors.json");

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FoldVectorFile {
        active_window_seconds: i64,
        digest_schema: String,
        entry_schema: String,
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

    fn vectors() -> FoldVectorFile {
        serde_json::from_str(FOLD_VECTORS).expect("fold vectors parse")
    }

    // ---- Conformance binding ----

    /// The corpus pins the constants; the constants live in code.
    #[test]
    fn corpus_pins_the_active_window_and_schemas() {
        let file = vectors();
        assert_eq!(file.active_window_seconds, PULSE_ACTIVE_WINDOW);
        assert_eq!(file.digest_schema, PULSE_DIGEST_SCHEMA);
        assert_eq!(file.entry_schema, PULSE_ENTRY_SCHEMA);
    }

    /// Every vector, folded by this implementation, must serialize to the same
    /// bytes as the corpus's expected digest.
    #[test]
    fn fold_matches_every_conformance_vector() {
        let file = vectors();
        assert!(!file.vectors.is_empty(), "corpus must not be empty");
        for vector in &file.vectors {
            let actual = fold_pulse_digest(
                &vector.input.project,
                vector.input.now,
                vector.input.source_errors.clone(),
                &vector.input.events,
            );
            let actual_value = serde_json::to_value(&actual).expect("serialize digest");
            assert_eq!(
                actual_value, vector.expected,
                "vector {} produced a different digest",
                vector.name
            );

            // Byte identity, not just semantic equality: the expected object
            // round-trips through the same strict struct, so a missing or
            // extra key fails here as well.
            let expected: PulseDigest =
                serde_json::from_value(vector.expected.clone()).expect("expected digest decodes");
            assert_eq!(
                serde_json::to_string(&actual).expect("serialize actual"),
                serde_json::to_string(&expected).expect("serialize expected"),
                "vector {} is not byte-identical",
                vector.name
            );
        }
    }

    /// The envelope's key order is part of the contract both languages bind to.
    #[test]
    fn digest_key_order_is_the_envelope_order() {
        let digest = fold_pulse_digest("30621:aa:demo", 1, Vec::new(), &[]);
        let rendered = serde_json::to_string(&digest).expect("serialize");
        let keys: Vec<&str> = rendered
            .trim_start_matches('{')
            .split(",\"")
            .filter_map(|chunk| chunk.trim_start_matches('"').split('"').next())
            .collect();
        assert_eq!(
            keys,
            vec![
                "schema",
                "source",
                "project",
                "asOf",
                "complete",
                "sessionsScope",
                "sessions",
                "entries",
                "errors",
            ]
        );
    }

    // ---- Confirmed empty vs partial read ----

    fn source_error() -> PulseDigestError {
        PulseDigestError {
            scope: "sessions:05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2".to_owned(),
            message: "relay unavailable".to_owned(),
        }
    }

    #[test]
    fn confirmed_empty_and_partial_read_are_different_outcomes() {
        let empty = fold_pulse_digest("30621:aa:demo", 100, Vec::new(), &[]);
        assert!(empty.complete);
        assert!(empty.errors.is_empty());
        assert!(empty.sessions.is_empty() && empty.entries.is_empty());

        let partial = fold_pulse_digest("30621:aa:demo", 100, vec![source_error()], &[]);
        assert!(!partial.complete);
        assert_eq!(partial.errors.len(), 1);
    }

    /// A partial read exits 2 on the relay's own error; a `--limit` truncation
    /// exits on the usage condition it actually is, rather than a manufactured
    /// relay status.
    #[test]
    fn partial_digest_exit_codes_follow_the_error_table() {
        let relay_failure = CliError::Relay {
            status: 503,
            body: String::new(),
        };
        assert_eq!(exit_code(&partial_digest_error(Some(relay_failure))), 2);
        assert_eq!(exit_code(&partial_digest_error(None)), 1);
    }

    // ---- Query shapes ----

    /// The digest issues two distinct filters: 44240 by `#a`, session kinds by
    /// `#h`. Combining them would return only the 44240 rows, because 44223
    /// carries no `a` tag — an empty session list with no error.
    #[test]
    fn digest_issues_two_distinct_filters() {
        let entries = entries_filter("30621:aa:demo", Some(42));
        assert_eq!(entries["kinds"], json!([44240]));
        assert_eq!(entries["#a"], json!(["30621:aa:demo"]));
        assert_eq!(entries["since"], json!(42));
        assert!(entries.get("#h").is_none());

        let sessions = session_facts_filter("05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2");
        assert_eq!(
            sessions["kinds"],
            json!([44223, 44224, 44227, 44229, 44230])
        );
        assert_eq!(
            sessions["#h"],
            json!(["05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2"])
        );
        assert!(sessions.get("#a").is_none());
    }

    // ---- Project resolution ----

    #[test]
    fn bare_dtag_resolves_only_when_exactly_one_project_matches() {
        let one = vec![format!("30621:{}:demo", "a".repeat(64))];
        assert_eq!(
            resolve_bare_dtag("demo", one.clone()).expect("resolves"),
            one[0]
        );

        let none = resolve_bare_dtag("demo", Vec::new()).expect_err("must not resolve");
        assert_eq!(exit_code(&none), 1);
        assert!(none.to_string().contains("no visible project named demo"));

        let two = vec![
            format!("30621:{}:demo", "a".repeat(64)),
            format!("30621:{}:demo", "b".repeat(64)),
        ];
        let ambiguous = resolve_bare_dtag("demo", two.clone()).expect_err("must not resolve");
        assert_eq!(exit_code(&ambiguous), 1);
        for candidate in &two {
            assert!(
                ambiguous.to_string().contains(candidate),
                "the error must print every candidate coordinate"
            );
        }
    }

    /// The precedence, end to end: an explicit coordinate is normalized, a
    /// bare dtag defers to the visible-project lookup, a malformed value is a
    /// usage error, and no value at all names the environment variable that
    /// would have supplied it.
    #[test]
    fn project_precedence_resolves_without_a_relay_where_it_can() {
        let canonical = format!("30621:{}:demo", "a".repeat(64));
        // A case-variant owner normalizes; the dtag is byte-exact and is not
        // touched.
        assert_eq!(
            direct_project_coordinate(Some(&format!("30621:{}:demo", "A".repeat(64))))
                .expect("normalizes"),
            Some(canonical.clone())
        );
        assert_eq!(
            direct_project_coordinate(Some(&canonical)).expect("normalizes"),
            Some(canonical)
        );
        assert_eq!(direct_project_coordinate(Some("demo")).expect("bare"), None);

        let malformed = direct_project_coordinate(Some("30617:abc:repo")).expect_err("rejects");
        assert_eq!(exit_code(&malformed), 1);

        let missing = direct_project_coordinate(None).expect_err("rejects");
        assert_eq!(exit_code(&missing), 1);
        assert!(
            missing.to_string().contains("BUZZ_PULSE_PROJECT"),
            "the error must name the environment variable that supplies it"
        );
    }

    #[test]
    fn coordinate_splitting_tolerates_a_dtag_with_colons() {
        let coordinate = format!("30621:{}:team:demo", "a".repeat(64));
        let (owner, dtag) = split_project_coordinate(&coordinate).expect("splits");
        assert_eq!(owner, "a".repeat(64));
        assert_eq!(dtag, "team:demo");
        assert!(split_project_coordinate("30617:abc:repo").is_none());
    }

    // ---- Pre-sign validation ----

    #[test]
    fn invalid_areas_fail_before_signing() {
        for area in [
            "/etc/passwd",
            "crates/../etc",
            "~/notes",
            "crates\\acp\\pool.rs",
            "crates//pool.rs",
            "crates/",
            // A stray comma yields an empty path rather than a silent drop.
            "crates/buzz-cli/src/lib.rs,,",
        ] {
            let entry = build_entry(
                crate::PulseKindArg::Plan,
                "Working here.".to_owned(),
                Some(area),
                None,
                None,
            )
            .expect("payload assembles");
            // The payload itself is only assembled here; buzz-core rejects it
            // through the builder, before any signing happens.
            let built = buzz_sdk::builders::build_pulse_entry(
                &format!("30621:{}:demo", "a".repeat(64)),
                &entry,
                None,
                None,
            );
            let error = crate::validate::sdk_err(
                built
                    .err()
                    .unwrap_or_else(|| panic!("code area {area:?} must be rejected")),
            );
            assert_eq!(exit_code(&error), 1, "code area {area:?} is a usage error");
        }
    }

    /// The one row of the exit-code table the read path can never produce: a
    /// write refused by the project gate is exit 3, not 1. Reads never 403 by
    /// design — an inadmissible private project returns an empty 200 — so
    /// `list`, `sessions`, and `digest` cannot reach this row.
    ///
    /// The relay end of this row is the assertion in
    /// `crates/buzz-test-client/tests/e2e_pulse.rs` that a refused
    /// `POST /events` answers **403** — this unit test only pins the mapping
    /// once that status arrives, and would pass vacuously on its own.
    #[test]
    fn a_refused_write_exits_three() {
        assert_eq!(
            exit_code(&CliError::Relay {
                status: 403,
                body: "restricted: project write access required".to_owned(),
            }),
            3
        );
    }

    #[test]
    fn over_cap_content_is_a_usage_error_before_signing() {
        let error = build_entry(
            crate::PulseKindArg::Plan,
            "a".repeat(MAX_PULSE_TEXT_BYTES + 1),
            None,
            None,
            None,
        )
        .expect_err("must reject");
        assert_eq!(exit_code(&error), 1);
        assert!(build_entry(
            crate::PulseKindArg::Plan,
            "a".repeat(MAX_PULSE_TEXT_BYTES),
            None,
            None,
            None,
        )
        .is_ok());
    }

    #[test]
    fn areas_are_split_on_commas_and_trimmed() {
        let entry = build_entry(
            crate::PulseKindArg::Blocker,
            "Do not touch these.".to_owned(),
            Some("crates/buzz-acp/src/pool.rs, crates/buzz-cli/src/lib.rs"),
            None,
            None,
        )
        .expect("payload assembles");
        assert_eq!(
            entry.code_areas,
            vec![
                "crates/buzz-acp/src/pool.rs".to_owned(),
                "crates/buzz-cli/src/lib.rs".to_owned()
            ]
        );
        assert_eq!(entry.entry_type.as_str(), "blocker");

        let empty = build_entry(
            crate::PulseKindArg::Note,
            "Nothing claimed.".to_owned(),
            Some("  "),
            None,
            None,
        )
        .expect("payload assembles");
        assert!(empty.code_areas.is_empty());
    }

    // ---- Output shapes ----

    fn demo_digest() -> PulseDigest {
        let file = vectors();
        let vector = file
            .vectors
            .iter()
            .find(|vector| vector.name == "session-null-observations-preserved")
            .expect("vector present");
        fold_pulse_digest(
            &vector.input.project,
            vector.input.now,
            vector.input.source_errors.clone(),
            &vector.input.events,
        )
    }

    #[test]
    fn both_formats_carry_source_and_preserve_nullable_facts() {
        let digest = demo_digest();
        let standard = serde_json::to_value(&digest).expect("serialize");
        let compact = compact_digest(&digest);
        assert_eq!(standard["source"], json!("client-composed"));
        assert_eq!(compact["source"], json!("client-composed"));

        let session = &standard["sessions"][0];
        for field in ["observedCommit", "dirty", "relayReachable", "verifiedAt"] {
            assert_eq!(
                session[field],
                Value::Null,
                "{field} must stay null — unknown is not false"
            );
        }
        assert_eq!(session["commitConfirmation"], json!("Commit not checked"));
        // The age comes from the 44223 `created_at`, never from `verifiedAt`.
        assert_eq!(session["observedAgeSeconds"], json!(600));
        assert_eq!(
            compact["sessions"][0]["commitConfirmation"],
            json!("Commit not checked")
        );
    }

    #[test]
    fn list_rows_are_stable_in_both_formats() {
        let file = vectors();
        let vector = file
            .vectors
            .iter()
            .find(|vector| vector.name == "no-branch-rows-preserved")
            .expect("vector present");
        let digest = fold_pulse_digest(
            &vector.input.project,
            vector.input.now,
            Vec::new(),
            &vector.input.events,
        );
        let entry = &digest.entries[1];
        assert_eq!(
            list_row(entry, &crate::OutputFormat::Compact),
            json!({
                "eventId": entry.event_id,
                "createdAt": entry.created_at,
                "type": "plan",
                "text": "Branch-scoped claim.",
                "branch": "wip/project-pulse",
            })
        );
        assert_eq!(
            list_row(entry, &crate::OutputFormat::Json),
            json!({
                "eventId": entry.event_id,
                "pubkey": entry.pubkey,
                "createdAt": entry.created_at,
                "type": "plan",
                "text": "Branch-scoped claim.",
                "claimedAreas": [],
                "branch": "wip/project-pulse",
                "sessionRef": Value::Null,
                "supersedes": Value::Null,
            })
        );
    }

    // ---- Supersession and attribution ----

    fn folded_vector(name: &str) -> PulseDigest {
        let file = vectors();
        let vector = file
            .vectors
            .iter()
            .find(|vector| vector.name == name)
            .unwrap_or_else(|| panic!("vector {name} present"));
        fold_pulse_digest(
            &vector.input.project,
            vector.input.now,
            vector.input.source_errors.clone(),
            &vector.input.events,
        )
    }

    /// A peer cannot quietly retract another author's claim: the target stays
    /// active and the claim is rendered as unhonored on the claimant.
    #[test]
    fn cross_author_supersession_leaves_the_original_active() {
        let digest = folded_vector("cross-author-supersession-not-honored");
        assert!(
            digest.entries.iter().all(|entry| entry.active),
            "no entry may lose its active flag to a cross-author claim"
        );
        let claimant = &digest.entries[0];
        assert_eq!(claimant.superseded_by.len(), 1);
        assert!(!claimant.superseded_by[0].honored);
        assert_eq!(
            claimant.superseded_by[0].reason.as_deref(),
            Some("cross-author")
        );
        // The claim is a fold observation, not a read failure.
        assert!(digest.complete);
    }

    /// Entry-to-session attribution is a consumer law with no representation in
    /// the v1 envelope: the CLI echoes `pu-session` verbatim and never nests an
    /// entry inside a session row, so it cannot attribute one author's entry to
    /// another team's card.
    #[test]
    fn entries_are_never_nested_inside_a_session_row() {
        let digest = folded_vector("session-null-observations-preserved");
        let session = serde_json::to_value(&digest.sessions[0]).expect("serialize");
        let object = session.as_object().expect("object");
        assert!(!object.contains_key("entries"));
        assert!(!object.contains_key("founder"));
    }

    // ---- Branch semantics ----

    #[test]
    fn branch_filter_never_mixes_named_and_null_rows() {
        assert!(branch_matches(None, Some("main")));
        assert!(branch_matches(None, None));
        assert!(branch_matches(Some("main"), Some("main")));
        assert!(!branch_matches(Some("main"), Some("Main")));
        assert!(!branch_matches(Some("main"), None));
        assert!(branch_matches(Some("-"), None));
        assert!(!branch_matches(Some("-"), Some("main")));
    }

    // ---- Active work ----

    #[test]
    fn active_work_requires_a_positive_freshness_signal() {
        assert_eq!(
            session_activity(SessionStatus::Running, false, PULSE_ACTIVE_WINDOW),
            "active"
        );
        assert_eq!(
            session_activity(SessionStatus::Running, false, PULSE_ACTIVE_WINDOW + 1),
            "stale"
        );
        assert_eq!(session_activity(SessionStatus::Running, true, 0), "stale");
        for status in [
            SessionStatus::Disconnected,
            SessionStatus::Failed,
            SessionStatus::Completed,
            SessionStatus::Stopped,
            SessionStatus::Interrupted,
            SessionStatus::Unknown,
        ] {
            assert_eq!(
                session_activity(status, false, 0),
                "stale",
                "{status:?} is never Active work"
            );
        }
        for status in [
            SessionStatus::Starting,
            SessionStatus::Idle,
            SessionStatus::WaitingForInput,
        ] {
            assert_eq!(session_activity(status, false, 0), "active");
        }
    }

    #[test]
    fn commit_confirmation_is_a_fixed_tri_state() {
        assert_eq!(commit_confirmation(Some(true)), "Commit confirmed on relay");
        assert_eq!(
            commit_confirmation(Some(false)),
            "Commit not found on relay"
        );
        assert_eq!(commit_confirmation(None), "Commit not checked");
    }
}
