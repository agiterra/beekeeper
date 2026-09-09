//! Declared work in Project Pulse: what one umbrella **assigned**, and the
//! evidence its own canonical fold established about each assignment.
//!
//! Contract: `docs/DECLARED_WORK_PULSE_IMPL.md` §2 and §3. Product authority:
//! `docs/WORK_COORDINATION_VISIBILITY_SPEC.md`.
//!
//! # This module re-implements no fold rule
//!
//! Inclusion, supersession, settlement and the canonical terminal are all
//! decided by
//! [`fold_coding_session_team_transactions`](crate::coding_session_team_transaction::fold_coding_session_team_transactions).
//! This module joins that decision to the *payloads* the fold does not carry —
//! assignee, role, objective, brief, branch, base, declared paths, acceptance
//! steps — by decoding the included events through their own envelope
//! validator. Where the fold and a sentence here disagree, the fold wins and
//! this is the bug.
//!
//! # Three honesty rules the spec puts on this projection
//!
//! - **An excluded 44244 event is not a declaration.** Its content is never
//!   rendered; the count of excluded events is disclosed as a number
//!   ([`PulseDeclaredWorkSession::excluded_count`]).
//! - **Closing a session settles nothing.** [`PulseDeclaredWorkLifecycle`] is
//!   carried beside the assignments and is never read into
//!   [`PulseDeclaredAssignmentStatus`]; an unsettled assignment in a closed
//!   umbrella stays `unresolved` and stays visible.
//! - **A report is evidence of a report.** `status` reads `reported` from one,
//!   never `settled`: settlement is the fold's existing rule (an approving
//!   disposition plus the assignee's acknowledgement) and nothing here adds an
//!   approval requirement.
//!
//! Nothing in this module compares a declared path to any other path, infers a
//! repository, or renders `verifier_required` — see §2 of the contract.

use std::collections::{HashMap, HashSet};

use nostr::Event;
use serde::{Deserialize, Serialize};

use crate::coding_session_team_transaction::{
    fold_coding_session_team_transactions, validate_coding_session_team_transaction_envelope,
    CodingSessionTeamDispositionDecision, CodingSessionTeamFold, CodingSessionTeamFoldContext,
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CodingSessionTeamVerdict,
};

/// Exact schema identifier of the declared-work response.
pub const PULSE_DECLARED_WORK_SCHEMA: &str = "buzz-pulse-declared-work/v1";

/// Exact schema identifier of the declared-work request.
pub const PULSE_DECLARED_WORK_REQUEST_SCHEMA: &str = "buzz-pulse-declared-work-request/v1";

/// How many umbrellas one declared-work request may carry.
///
/// A bound on the *read*, not on the project: a caller with more sessions pages
/// them, and the page it has not read yet is disclosed by the surface rather
/// than dropped.
pub const MAX_PULSE_DECLARED_WORK_SESSIONS: usize = 8;

// ── Vocabularies ─────────────────────────────────────────────────────────────

/// Durable lifecycle of an umbrella, exactly as the digest proved it.
///
/// Carried for display and for the "Session closed" evidence line. **Never**
/// an input to settlement: closing an execution settles nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PulseDeclaredWorkLifecycle {
    /// The umbrella has no durable close on the wire.
    Open,
    /// The umbrella was durably closed.
    Closed,
}

impl PulseDeclaredWorkLifecycle {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

/// The one word this projection derives per assignment.
///
/// Derived in Rust, once, so a CLI and Desktop reading the same records say the
/// same word. See [`project_declared_work`] for the derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PulseDeclaredAssignmentStatus {
    /// No included report names this assignment, and it is not settled.
    Unresolved,
    /// At least one included report names it. Evidence of a report, no more.
    Reported,
    /// The fold's own settlement: an approving disposition the assignee
    /// acknowledged.
    Settled,
}

impl PulseDeclaredAssignmentStatus {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::Reported => "reported",
            Self::Settled => "settled",
        }
    }
}

// ── Response shapes ──────────────────────────────────────────────────────────

/// One read that failed or was bounded, named rather than folded away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseDeclaredWorkError {
    /// What the failure was about — `declared:<sessionKey>` for a session.
    pub scope: String,
    /// The failure's own sentence, verbatim.
    pub message: String,
}

/// One canonically included report answering one assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseDeclaredReport {
    /// Event id of the report.
    pub event_id: String,
    /// Pubkey that signed it.
    pub author_pubkey: String,
    /// The report's own `created_at`, Unix seconds.
    pub created_at: i64,
    /// The report's summary, verbatim.
    pub summary: String,
    /// Reported branch, or null when the report named none.
    pub branch: Option<String>,
    /// Reported base object id, or null.
    pub base_sha: Option<String>,
    /// Reported head object id, or null.
    pub head_sha: Option<String>,
    /// Files the report claimed it changed — a claim, never an observation.
    pub files: Vec<String>,
    /// How many structured test records the report carried.
    ///
    /// A count, deliberately: the surface must not read a passing count as an
    /// independent verification, and the records themselves live in the source
    /// event a reader can open.
    pub test_count: usize,
    /// Departures from the assignment the report disclosed.
    pub deviations: Vec<String>,
    /// Remaining work or risk the report disclosed.
    pub residuals: Vec<String>,
    /// Whether the fold listed this report's author as holding no active seat
    /// for the role its assignment named.
    pub author_unseated: bool,
}

/// One canonically included disposition governing a report of one assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseDeclaredDisposition {
    /// Event id of the disposition.
    pub event_id: String,
    /// Pubkey that signed it.
    pub author_pubkey: String,
    /// The disposition's own `created_at`, Unix seconds.
    pub created_at: i64,
    /// The closed decision, exactly as kind 44244 spells it.
    pub decision: CodingSessionTeamDispositionDecision,
    /// Event id of the report this disposition governs.
    pub report_ref: String,
}

/// The canonical fold's settlement row for one assignment, verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseDeclaredSettlement {
    /// The fold's own settled flag. The only source of `settled`.
    pub settled: bool,
    /// Explicitly governed report, when an approval chain is complete.
    pub governed_report_event_id: Option<String>,
    /// The approving disposition, when there is one.
    pub disposition_event_id: Option<String>,
    /// The assignee's acknowledgement of that disposition, when there is one.
    pub acknowledgement_event_id: Option<String>,
}

/// One canonically included assignment with its verified source fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseDeclaredAssignment {
    /// Event id of the assignment — the source a reader opens.
    pub source_event_id: String,
    /// The assignment's own `created_at`, Unix seconds.
    pub created_at: i64,
    /// Pubkey that **signed** the assignment. The assigning author stays
    /// inspectable; it is never the responsible participant.
    pub assigner_pubkey: String,
    /// Pubkey the assignment targets. The responsible participant.
    pub assignee_actor: String,
    /// Role the assignment named for its assignee.
    pub assignee_role: String,
    /// The concise outcome this assignment owns.
    pub objective: String,
    /// The complete brief, verbatim.
    pub brief: String,
    /// Declared topic branch, or null when none was declared.
    pub branch: Option<String>,
    /// Declared base object id, or null.
    pub base_sha: Option<String>,
    /// Declared paths, verbatim. Compared with nothing (§2).
    pub file_ownership: Vec<String>,
    /// Declared acceptance commands or observable checks, in order.
    pub acceptance_steps: Vec<String>,
    /// The assignment this one corrects, when it corrects one.
    pub supersedes: Option<String>,
    /// Included reports naming this assignment, in the fold's included order.
    pub reports: Vec<PulseDeclaredReport>,
    /// Included dispositions naming this assignment, in included order.
    pub dispositions: Vec<PulseDeclaredDisposition>,
    /// The fold's settlement row.
    pub settlement: PulseDeclaredSettlement,
    /// The derived word — see [`PulseDeclaredAssignmentStatus`].
    pub status: PulseDeclaredAssignmentStatus,
}

/// The canonical newest authorized terminal, when the fold established one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseDeclaredTerminal {
    /// Event id of the terminal.
    pub event_id: String,
    /// Exactly `mission.completed` or `mission.blocked`, the record's own token.
    #[serde(rename = "type")]
    pub terminal_type: String,
    /// The terminal event's own `created_at`, Unix seconds.
    pub at: i64,
}

/// One umbrella's declared work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseDeclaredWorkSession {
    /// Umbrella key, as the Pulse digest keys sessions.
    pub session_key: String,
    /// Canonical umbrella UUID.
    pub session_ref: String,
    /// Immutable genesis event id of the umbrella.
    ///
    /// Carried beside `session_ref` because the two identify different things:
    /// the umbrella's coordination key, and the signed record every 44244 in
    /// it names. A details block that showed only one would leave a reader
    /// unable to check that the records they are reading belong to the session
    /// the row claims (contract §6).
    pub genesis_ref: String,
    /// Canonical channel UUID the records were read from.
    pub channel_id: String,
    /// The session's reported name, or null.
    pub name: Option<String>,
    /// Durable lifecycle. Evidence, never settlement.
    pub lifecycle: PulseDeclaredWorkLifecycle,
    /// Newest durable observation time — display only.
    pub latest_observation_at: Option<i64>,
    /// Pubkey of the genesis signer.
    pub founder_pubkey: String,
    /// The canonical terminal, or null when the fold established none.
    pub terminal: Option<PulseDeclaredTerminal>,
    /// The fold's own error sentence when this session's 44244 set could not
    /// be read. A session carrying it also carries `assignments: []`, and the
    /// surface renders "records unreadable" — never "no work".
    pub unreadable: Option<String>,
    /// How many 44244 events the fold excluded. A count, never their content.
    pub excluded_count: usize,
    /// Canonically included assignments, ascending by `(createdAt, sourceEventId)`.
    pub assignments: Vec<PulseDeclaredAssignment>,
}

/// One page of declared work, as the native projection wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseDeclaredWork {
    /// Must equal [`PULSE_DECLARED_WORK_SCHEMA`].
    pub schema: String,
    /// The reader's own pubkey, or null when this surface has no identity.
    pub viewer_pubkey: Option<String>,
    /// The sessions this page read, in request order.
    pub sessions: Vec<PulseDeclaredWorkSession>,
    /// Reads that failed or were bounded, each named by scope.
    pub errors: Vec<PulseDeclaredWorkError>,
}

// ── Inputs ───────────────────────────────────────────────────────────────────

/// Everything one umbrella's declared work is projected from.
///
/// Whole **signed** events, never stripped rows: the 44244 fold verifies
/// signatures as its first act, so anything else would hand it nothing to
/// verify.
pub struct PulseDeclaredWorkSources<'a> {
    /// Umbrella key, as the digest keys sessions.
    pub session_key: &'a str,
    /// Canonical channel UUID the records were read from.
    pub channel_id: &'a str,
    /// Canonical umbrella UUID.
    pub session_ref: &'a str,
    /// The session's reported name, when one is on the wire.
    pub name: Option<&'a str>,
    /// Durable lifecycle, from the digest's own session coordination fold.
    pub lifecycle: PulseDeclaredWorkLifecycle,
    /// Newest durable observation time — display only.
    pub latest_observation_at: Option<i64>,
    /// Context the 44244 fold judges with, straight from the signed chain.
    pub context: &'a CodingSessionTeamFoldContext,
    /// Signed kind 44244 events, ascending by `(created_at, id)`.
    pub team_events: &'a [Event],
}

// ── The projection ───────────────────────────────────────────────────────────

/// Project one umbrella's signed 44244 set into its declared work.
///
/// A fold `Err` is **this session's** failure and never the page's: the session
/// comes back with [`PulseDeclaredWorkSession::unreadable`] set and
/// `assignments: []`, and every other session on the page is untouched.
///
/// `status` is derived here, once: `settled` iff the fold settled it; else
/// `reported` iff at least one included report names it; else `unresolved`.
/// The lifecycle and the canonical terminal are carried beside that word and
/// never fold into it — closing an execution settles nothing.
pub fn project_declared_work(sources: &PulseDeclaredWorkSources<'_>) -> PulseDeclaredWorkSession {
    let mut session = PulseDeclaredWorkSession {
        session_key: sources.session_key.to_owned(),
        session_ref: sources.session_ref.to_owned(),
        genesis_ref: sources.context.genesis_ref.clone(),
        channel_id: sources.channel_id.to_owned(),
        name: sources.name.map(str::to_owned),
        lifecycle: sources.lifecycle,
        latest_observation_at: sources.latest_observation_at,
        founder_pubkey: sources.context.founder_pubkey.clone(),
        terminal: None,
        unreadable: None,
        excluded_count: 0,
        assignments: Vec::new(),
    };

    let fold = match fold_coding_session_team_transactions(sources.team_events, sources.context) {
        Ok(fold) => fold,
        Err(error) => {
            session.unreadable = Some(error);
            return session;
        }
    };

    session.excluded_count = fold.excluded.len();
    // One index of the supplied events for the whole projection: the terminal,
    // every assignment and every piece of evidence read from the same map.
    let by_id = index_by_id(sources.team_events);
    session.terminal = canonical_terminal(&fold, &by_id);
    session.assignments = assignments(&fold, &by_id);
    session
}

/// The fold's canonical terminal, with the terminal event's own time.
fn canonical_terminal(
    fold: &CodingSessionTeamFold,
    by_id: &HashMap<String, &Event>,
) -> Option<PulseDeclaredTerminal> {
    let terminal = fold.canonical_terminal.as_ref()?;
    let event = by_id.get(terminal.event_id.as_str())?;
    Some(PulseDeclaredTerminal {
        event_id: terminal.event_id.clone(),
        terminal_type: terminal.transaction_type.as_str().to_owned(),
        at: created_at(event),
    })
}

/// Every supplied event by its own id, hashed once.
///
/// The projection reaches for events by id from three places — the terminal,
/// every assignment, and every piece of evidence — and one page carries up to
/// eight umbrellas' full 44244 sets. A linear scan per lookup, each recomputing
/// `id.to_hex()`, made that quadratic in the page; the hex is computed once per
/// event here and never again.
fn index_by_id(events: &[Event]) -> HashMap<String, &Event> {
    events
        .iter()
        .map(|event| (event.id.to_hex(), event))
        .collect()
}

/// Every canonically included record, decoded exactly once.
///
/// Through the record's own envelope validator, exactly as before — a second
/// reader of the same bytes is a second reader that can disagree. What changed
/// is only *how often*: the previous shape re-decoded every included event once
/// per assignment, so a mission with twelve assignments decoded its whole set
/// twenty-five times over.
///
/// Keys borrow from the fold's own id list rather than cloning it: the map
/// lives inside one projection, and the list outlives it.
fn included_payloads<'a>(
    fold: &'a CodingSessionTeamFold,
    by_id: &HashMap<String, &Event>,
) -> HashMap<&'a str, CodingSessionTeamTransactionPayload> {
    let mut payloads = HashMap::with_capacity(fold.included_event_ids.len());
    for id in &fold.included_event_ids {
        let Some(event) = by_id.get(id.as_str()) else {
            continue;
        };
        let Some(payload) = transaction_payload(event) else {
            continue;
        };
        payloads.insert(id.as_str(), payload);
    }
    payloads
}

/// Included reports and dispositions, grouped by the assignment they name.
///
/// Built in **one** ordered pass over the fold's included ids, so each list
/// keeps the fold's own included order — the order the previous per-assignment
/// filter produced, and the order the surface renders.
struct DeclaredEvidence {
    /// Reports whose `assignmentRef` names each assignment.
    reports: HashMap<String, Vec<PulseDeclaredReport>>,
    /// Dispositions whose `assignmentRef` names each assignment.
    dispositions: HashMap<String, Vec<PulseDeclaredDisposition>>,
}

fn group_evidence(
    fold: &CodingSessionTeamFold,
    by_id: &HashMap<String, &Event>,
    payloads: &HashMap<&str, CodingSessionTeamTransactionPayload>,
) -> DeclaredEvidence {
    // The fold's unseated disclosure, hashed once: it is a subset of the
    // included reports, and scanning it per report was the same nested scan in
    // miniature.
    let unseated: HashSet<&str> = fold
        .unseated_reports
        .iter()
        .map(|report| report.event_id.as_str())
        .collect();
    let mut evidence = DeclaredEvidence {
        reports: HashMap::new(),
        dispositions: HashMap::new(),
    };
    for id in &fold.included_event_ids {
        let Some(event) = by_id.get(id.as_str()) else {
            continue;
        };
        let Some(payload) = payloads.get(id.as_str()) else {
            continue;
        };
        match &payload.body {
            CodingSessionTeamTransactionBody::Report(body) => {
                evidence
                    .reports
                    .entry(body.assignment_ref.clone())
                    .or_default()
                    .push(PulseDeclaredReport {
                        event_id: id.clone(),
                        author_pubkey: event.pubkey.to_hex(),
                        created_at: created_at(event),
                        summary: body.summary.clone(),
                        branch: body.branch.clone(),
                        base_sha: body.base_sha.clone(),
                        head_sha: body.head_sha.clone(),
                        files: body.files.clone(),
                        test_count: body.tests.len(),
                        deviations: body.deviations.clone(),
                        residuals: body.residuals.clone(),
                        // Disclosure the fold makes, carried rather than
                        // recomputed: an author with no active seat for the
                        // role their assignment named.
                        author_unseated: unseated.contains(id.as_str()),
                    });
            }
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
                assignment_ref,
                report_ref,
                decision,
                ..
            }) => {
                evidence
                    .dispositions
                    .entry(assignment_ref.clone())
                    .or_default()
                    .push(PulseDeclaredDisposition {
                        event_id: id.clone(),
                        author_pubkey: event.pubkey.to_hex(),
                        created_at: created_at(event),
                        decision: *decision,
                        report_ref: report_ref.clone(),
                    });
            }
            // A refutation, acknowledgement, note, decision or terminal is not
            // evidence *of an assignment* on this surface. Each is still a
            // signed record a reader can open from the source id.
            _ => {}
        }
    }
    evidence
}

/// Every included assignment, joined to its payload and its evidence.
fn assignments(
    fold: &CodingSessionTeamFold,
    by_id: &HashMap<String, &Event>,
) -> Vec<PulseDeclaredAssignment> {
    let payloads = included_payloads(fold, by_id);
    let mut evidence = group_evidence(fold, by_id, &payloads);
    let mut projected: Vec<PulseDeclaredAssignment> = Vec::with_capacity(fold.assignments.len());

    for settlement in &fold.assignments {
        let id = settlement.assignment_event_id.as_str();
        // The fold only names ids it was supplied, and only assignment bodies:
        // a miss here is impossible rather than tolerable, and skipping keeps
        // one impossible record from erasing every other assignment.
        let Some(event) = by_id.get(id) else {
            continue;
        };
        let Some(payload) = payloads.get(id) else {
            continue;
        };
        let CodingSessionTeamTransactionBody::Assignment(body) = &payload.body else {
            continue;
        };
        // `remove` rather than `get`: the fold emits one settlement per active
        // assignment, so no two iterations claim the same id, and the evidence
        // moves instead of being cloned.
        let reports = evidence.reports.remove(id).unwrap_or_default();
        let dispositions = evidence.dispositions.remove(id).unwrap_or_default();
        // The derivation, in one place: the fold settles, a report is evidence
        // of a report, and everything else is unresolved.
        let status = if settlement.settled {
            PulseDeclaredAssignmentStatus::Settled
        } else if reports.is_empty() {
            PulseDeclaredAssignmentStatus::Unresolved
        } else {
            PulseDeclaredAssignmentStatus::Reported
        };
        projected.push(PulseDeclaredAssignment {
            source_event_id: id.to_owned(),
            created_at: created_at(event),
            assigner_pubkey: event.pubkey.to_hex(),
            assignee_actor: body.assignee_actor.clone(),
            assignee_role: body.assignee_role.clone(),
            objective: body.objective.clone(),
            brief: body.brief.clone(),
            branch: body.branch.clone(),
            base_sha: body.base_sha.clone(),
            file_ownership: body.file_ownership.clone(),
            acceptance_steps: body.acceptance_steps.clone(),
            supersedes: payload.supersedes.clone(),
            reports,
            dispositions,
            settlement: PulseDeclaredSettlement {
                settled: settlement.settled,
                governed_report_event_id: settlement.governed_report_event_id.clone(),
                disposition_event_id: settlement.disposition_event_id.clone(),
                acknowledgement_event_id: settlement.acknowledgement_event_id.clone(),
            },
            status,
        });
    }

    projected.sort_by(|left, right| {
        (left.created_at, left.source_event_id.as_str())
            .cmp(&(right.created_at, right.source_event_id.as_str()))
    });
    projected
}

/// One event's `created_at` in Unix seconds.
fn created_at(event: &Event) -> i64 {
    event.created_at.as_secs() as i64
}

/// The decoded 44244 payload of one signed event.
///
/// Through the record's own validator, never an ad-hoc JSON reach: a second
/// reader of the same bytes is a second reader that can disagree.
fn transaction_payload(event: &Event) -> Option<CodingSessionTeamTransactionPayload> {
    validate_coding_session_team_transaction_envelope(event).ok()
}

#[cfg(test)]
#[path = "pulse_declared_work_tests.rs"]
mod tests;
