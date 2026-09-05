//! What Project Pulse knows about a mission **without asking anyone to report**.
//!
//! Brian's ruling, 2026-09-02: *"we can achieve this without asking the agents
//! to do it — which I find to always be the weak link."* Every fact this module
//! turns into a sentence is produced by a mechanism rather than by cooperation:
//!
//! - the **hire host**, through the git hooks it installs in the seat's own
//!   worktree ([`crate::seat_git_hooks`]) — wip refs and kind 44246 checkpoints
//!   land under the seat's own key because it ran `git commit`, not because it
//!   remembered to post;
//! - the **provider**, through gate rows derived from tool calls it already
//!   parses (`source: "observed"`, another lane's field — see
//!   [`PulseGateSource`]);
//! - the **relay**, through kind 30618 ref state, which it signs after a push.
//!
//! The agent's own words are left where judgment genuinely lives — a report, a
//! verdict, a note — and are never parsed. A commit subject is shown as the
//! seat's words and read by nothing.
//!
//! # One model, two consumers
//!
//! [`fold_pulse_mission_row`] produces the facts and
//! [`render_pulse_mission_lines`] composes every human sentence **in Rust**, so
//! `bee pulse digest` and Desktop print the same bytes. Desktop renders the
//! strings into elements and re-words nothing; there is no second copy of this
//! copy in TypeScript.
//!
//! # No fold rule is restated here
//!
//! This module **calls**
//! [`fold_coding_session_team_transactions`](crate::coding_session_team_transaction_fold::fold_coding_session_team_transactions),
//! [`fold_coding_session_policies`](crate::coding_session_policy_fold::fold_coding_session_policies)
//! and
//! [`fold_coding_session_observations`](crate::coding_session_observation_fold::fold_coding_session_observations).
//! It re-implements none of them. Where those folds disagree with a sentence
//! here, they win and this is the bug.

use std::collections::BTreeMap;

use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coding_session_observation::{
    fold_coding_session_observations, CodingSessionObservationFoldContext,
    CodingSessionObservationGateOutcome,
};
use crate::coding_session_policy::{fold_coding_session_policies, CodingSessionPolicyGrant};
use crate::coding_session_team_transaction::{
    fold_coding_session_team_transactions, validate_coding_session_team_transaction_envelope,
    CodingSessionTeamFold, CodingSessionTeamFoldContext, CodingSessionTeamFoldExclusionCode,
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CodingSessionTeamTransactionType, CodingSessionTeamVerdict,
};

/// Schema identifier of the mission-row sibling object.
pub const PULSE_MISSION_ROWS_SCHEMA: &str = "buzz-pulse-mission-rows/v1";

/// How many open sessions one digest reads mission rows for.
///
/// Every further open session costs six queries plus a receipt read, so the cap
/// is on the *rows*, not on the sentence: session nine is disclosed by name in
/// [`PulseMissionRows::mission_errors`] rather than quietly dropped.
pub const MAX_PULSE_MISSION_ROWS: usize = 8;

/// How many gate lines one seat shows before the rest are counted.
pub const MAX_PULSE_MISSION_GATE_LINES: usize = 4;

/// How many `moved` rows one mission shows.
pub const MAX_PULSE_MISSION_MOVED_ROWS: usize = 8;

/// How many phase-timing lines one mission shows.
pub const MAX_PULSE_MISSION_TIMING_LINES: usize = 6;

/// The one namespace a wip-share hook is allowed to push to.
///
/// Defined here rather than beside the hook text because it is the boundary two
/// unrelated things agree on: the hook refuses to push outside it, and Pulse
/// refuses to call anything outside it a shared local commit.
pub const WIP_REF_PREFIX: &str = "refs/heads/wip/";

/// How long a wip ref lives before `bee git prune-wip` deletes it.
///
/// Disclosed wherever wip refs are shown: a ref that vanishes on a schedule
/// nobody stated would read as a commit that never happened.
pub const WIP_REF_RETENTION_DAYS: u64 = 30;

/// Whether a ref name is in the wip namespace.
///
/// Prefix equality on the full ref name, never a substring or a suffix match: a
/// branch called `feature/wip/x` is not a wip ref and must not be pruned,
/// force-pushed, or rendered as one.
///
/// A name containing `..` is refused outright. `git check-ref-format` forbids
/// it, so such a name never came from git — and a prune planner that accepted
/// it would be handed a traversal by whoever wrote the event (sub-lane H's
/// finding).
pub fn is_wip_ref(name: &str) -> bool {
    name.starts_with(WIP_REF_PREFIX)
        && name.len() > WIP_REF_PREFIX.len()
        && !name.contains("..")
        && !name.contains(char::is_whitespace)
}

// ── Gate provenance, the one field another lane owns ─────────────────────────

/// Whether a gate row was **observed** by a mechanism or **declared** by its
/// author.
///
/// The token arrives on the wire as kind 44246's `gate.source`, which is Lane
/// L5's field to land. This lane consumes it strictly as an interface: read it
/// with [`pulse_gate_source_token`], turn it into this enum with
/// [`PulseGateSource::from_wire_token`], and treat its absence as
/// [`PulseGateSource::Declared`] — the honest reading, since a row nothing
/// observed is a claim.
///
/// Nothing here writes the field, and nothing here fails when it is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PulseGateSource {
    /// Stated by the row's author about its own work. A claim.
    ///
    /// Ordered first so `Declared < Observed` and the precedence rule below is
    /// a plain comparison.
    Declared,
    /// Derived from a tool call somebody's machine actually ran.
    Observed,
}

impl PulseGateSource {
    /// The exact wire token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Declared => "declared",
        }
    }

    /// Read a `gate.source` token, absent or unknown reading as `declared`.
    ///
    /// An unknown token is **not** promoted to `observed`: only the exact word
    /// `observed` buys the stronger claim.
    pub fn from_wire_token(token: Option<&str>) -> Self {
        match token {
            Some("observed") => Self::Observed,
            _ => Self::Declared,
        }
    }
}

/// How a caller supplies a gate row's `source` token.
///
/// Named rather than inlined so the seam this lane consumes Lane L5's field
/// through has one spelling everywhere.
pub type PulseGateSourceLookup = dyn Fn(&Event, &str) -> Option<String>;

/// The `gate.source` token a signed 44246 event carries for one gate name.
///
/// Reads the event's own content JSON rather than the decoded struct, because
/// the decoded struct is another lane's type and does not carry the field yet.
/// Until it does this returns `None` for every row — the strict decoder refuses
/// unknown keys, so a row carrying `source` does not reach the fold at all.
/// The moment L5 lands the field, both halves work with no change here.
pub fn pulse_gate_source_token(event: &Event, gate: &str) -> Option<String> {
    let content: Value = serde_json::from_str(&event.content).ok()?;
    let rows = content.get("body")?.get("rows")?.as_array()?;
    rows.iter()
        .find(|row| row.get("gate").and_then(Value::as_str) == Some(gate))
        .and_then(|row| row.get("source"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

// Facts and rendering live in sibling files so no file here passes 1,000 lines
// (the same split `coding_session_team_transaction.rs` uses). Child modules,
// re-exported, so every caller's path is unchanged.
#[path = "pulse_mission_facts.rs"]
mod facts;
pub use facts::*;

// ── Inputs ───────────────────────────────────────────────────────────────────

/// Everything one mission row is folded from.
///
/// Whole **signed** events, never stripped `Value` rows: 44244 verifies
/// signatures as its first act, so passing anything else would hand it nothing
/// to verify.
pub struct PulseMissionSources<'a> {
    /// Umbrella key, as the digest keys sessions.
    pub session_key: &'a str,
    /// Channel the records were read from.
    pub channel_id: &'a str,
    /// The session's name, when one is on the wire.
    pub name: Option<&'a str>,
    /// Newest durable observation time — display only.
    pub latest_observation_at: Option<i64>,
    /// Context the 44244 fold judges with, straight from the signed chain.
    pub context: &'a CodingSessionTeamFoldContext,
    /// Grants the policy fold judges standing with.
    pub policy_grants: &'a [CodingSessionPolicyGrant],
    /// Signed kind 44244 events, ascending by the fold's own order.
    pub team_events: &'a [Event],
    /// Signed kind 44245 events.
    pub policy_events: &'a [Event],
    /// Signed kind 44246 events, ascending — the observation fold's
    /// newest-wins is **last in the slice**.
    ///
    /// A relay page arrives newest-first, so the caller reverses it before
    /// building this (the CLI does; Desktop's read sorts ascending). Fed as
    /// read, the digest showed a seat's first row per gate as its current one
    /// (finding 79).
    pub observation_events: &'a [Event],
    /// Relay-signed ref state for the repo this umbrella works in.
    pub ref_state: &'a [PulseRefState],
    /// Seats a caller claims; each is kept only when `context.active_seats`
    /// supports it.
    pub claimed_seats: &'a [String],
    /// How a gate row's `source` is read.
    ///
    /// The seam this lane consumes Lane L5's `gate.source` through. `None`
    /// means [`pulse_gate_source_token`], which reads the signed event's own
    /// content and today finds nothing because the key has not landed. A test —
    /// or the day L5 lands it, the wire itself — supplies the token here and
    /// every precedence rule below runs unchanged.
    pub gate_source: Option<&'a PulseGateSourceLookup>,
}

impl PulseMissionSources<'_> {
    /// The `gate.source` token for one row, through whichever adapter is set.
    fn gate_source_token(&self, event: &Event, gate: &str) -> Option<String> {
        match self.gate_source {
            Some(lookup) => lookup(event, gate),
            None => pulse_gate_source_token(event, gate),
        }
    }
}

/// Fold one umbrella's signed events into the facts Pulse renders.
///
/// A 44244 `Err` is **one row's** failure and never the digest's: the row reads
/// `unreadable` and every other session is untouched.
pub fn fold_pulse_mission_row(
    sources: &PulseMissionSources<'_>,
    now_unix: i64,
) -> PulseMissionFacts {
    let session_ref = Some(sources.context.session_ref.clone());
    let mut facts = PulseMissionFacts {
        session_key: sources.session_key.to_owned(),
        session_ref,
        channel_id: sources.channel_id.to_owned(),
        name: sources.name.map(str::to_owned),
        latest_observation_at: sources.latest_observation_at,
        state: PulseMissionState::Running,
        unreadable: None,
        waiting: None,
        terminal_event_id: None,
        verdict: None,
        excluded_completion: None,
        policy: PulseMissionPolicy::default(),
        seats: Vec::new(),
        moved: Vec::new(),
        timing: Vec::new(),
        seat_claims_refused: Vec::new(),
        ref_state_present: !sources.ref_state.is_empty(),
    };

    let team = match fold_coding_session_team_transactions(sources.team_events, sources.context) {
        Ok(fold) => fold,
        Err(error) => {
            facts.state = PulseMissionState::Unreadable;
            facts.unreadable = Some(error);
            return facts;
        }
    };

    facts.seat_claims_refused = refused_seat_claims(sources.claimed_seats, sources.context);
    apply_team_facts(&mut facts, &team, sources.team_events);
    apply_policy_facts(&mut facts, sources);
    apply_observation_facts(&mut facts, &team, sources);
    apply_ref_state(&mut facts, sources, now_unix);
    facts
}

/// Claimed seats no accepted 44228 transition supports (L5.5's rule).
fn refused_seat_claims(claimed: &[String], context: &CodingSessionTeamFoldContext) -> Vec<String> {
    claimed
        .iter()
        .filter(|claim| {
            !context
                .active_seats
                .iter()
                .any(|seat| &&seat.actor_pubkey == claim)
        })
        .cloned()
        .collect()
}

/// State, waiting, verdict, refused completion, and what each seat owes.
fn apply_team_facts(facts: &mut PulseMissionFacts, team: &CodingSessionTeamFold, events: &[Event]) {
    let owned_by_id: BTreeMap<String, &Event> = events
        .iter()
        .map(|event| (event.id.to_hex(), event))
        .collect();

    if let Some(terminal) = &team.canonical_terminal {
        facts.terminal_event_id = Some(terminal.event_id.clone());
        facts.state = match terminal.transaction_type {
            CodingSessionTeamTransactionType::MissionCompleted => PulseMissionState::Completed,
            _ => PulseMissionState::Blocked,
        };
    }

    if let Some(waiting) = &team.waiting_on_decision {
        let event = owned_by_id.get(&waiting.request_event_id);
        facts.waiting = Some(PulseMissionRuling {
            session_key: facts.session_key.clone(),
            request_id: waiting.request_event_id.clone(),
            held_on: waiting.held_on.clone(),
            asked_by: event.map_or_else(String::new, |event| event.pubkey.to_hex()),
            asked_at: event.map(|event| event.created_at.as_secs() as i64),
            question: event.and_then(|event| decision_question(event)),
        });
    }

    facts.verdict = newest_verdict(team, &owned_by_id);
    // A completion the fold refused is not a completion. No 44244 record says
    // that in one field, so the row says it in a sentence — and never `completed`.
    facts.excluded_completion = team
        .excluded
        .iter()
        .find(|exclusion| {
            owned_by_id.get(&exclusion.event_id).is_some_and(|event| {
                transaction_payload(event).is_some_and(|payload| {
                    payload.transaction_type == CodingSessionTeamTransactionType::MissionCompleted
                })
            })
        })
        .map(|exclusion| PulseMissionExcludedCompletion {
            event_id: exclusion.event_id.clone(),
            code: exclusion_code_token(exclusion.code).to_owned(),
        });
}

/// The decoded 44244 payload of one signed event.
///
/// Through the record's own validator, never an ad-hoc JSON reach: a second
/// reader of the same bytes is a second reader that can disagree.
fn transaction_payload(event: &Event) -> Option<CodingSessionTeamTransactionPayload> {
    validate_coding_session_team_transaction_envelope(event).ok()
}

/// The `question` of a `decision.request`.
fn decision_question(event: &Event) -> Option<String> {
    match transaction_payload(event)?.body {
        CodingSessionTeamTransactionBody::DecisionRequest(request) => Some(request.question),
        _ => None,
    }
}

/// The exclusion code's own wire token.
///
/// An exhaustive match rather than a `Debug` string: `{:?}` would rename every
/// row the day somebody renames a variant, and a code a reader has learned is
/// part of the contract.
const fn exclusion_code_token(code: CodingSessionTeamFoldExclusionCode) -> &'static str {
    match code {
        CodingSessionTeamFoldExclusionCode::Unauthorized => "unauthorized",
        CodingSessionTeamFoldExclusionCode::DanglingReference => "danglingReference",
        CodingSessionTeamFoldExclusionCode::WrongTypeReference => "wrongTypeReference",
        CodingSessionTeamFoldExclusionCode::InvalidCorrection => "invalidCorrection",
        CodingSessionTeamFoldExclusionCode::DependentOnUnauthorized => "dependentOnUnauthorized",
        CodingSessionTeamFoldExclusionCode::DependentOnSuperseded => "dependentOnSuperseded",
        CodingSessionTeamFoldExclusionCode::DependentOnExcluded => "dependentOnExcluded",
        CodingSessionTeamFoldExclusionCode::Superseded => "superseded",
        CodingSessionTeamFoldExclusionCode::CorrectionConflict => "correctionConflict",
        CodingSessionTeamFoldExclusionCode::CompletionNotApproved => "completionNotApproved",
        CodingSessionTeamFoldExclusionCode::CompletionBlockedByOpenDecision => {
            "completionBlockedByOpenDecision"
        }
        CodingSessionTeamFoldExclusionCode::CompletionNotVerified => "completionNotVerified",
        CodingSessionTeamFoldExclusionCode::TerminalConflict => "terminalConflict",
    }
}

/// The newest included `disposition` or `refutation`, with its own token.
fn newest_verdict(
    team: &CodingSessionTeamFold,
    by_id: &BTreeMap<String, &Event>,
) -> Option<PulseMissionVerdict> {
    let mut newest: Option<(i64, String, PulseMissionVerdict)> = None;
    for id in &team.included_event_ids {
        let Some(event) = by_id.get(id) else { continue };
        let Some(payload) = transaction_payload(event) else {
            continue;
        };
        if payload.transaction_type != CodingSessionTeamTransactionType::Verdict {
            continue;
        }
        let Some(token) = verdict_token(&payload) else {
            continue;
        };
        let created_at = event.created_at.as_secs() as i64;
        let candidate = PulseMissionVerdict {
            event_id: id.clone(),
            author: event.pubkey.to_hex(),
            token,
        };
        let replace = newest
            .as_ref()
            .is_none_or(|(at, other, _)| (created_at, id) > (*at, other));
        if replace {
            newest = Some((created_at, id.clone(), candidate));
        }
    }
    newest.map(|(_, _, verdict)| verdict)
}

/// A verdict's own decision token, verbatim.
///
/// Serialised through the record's own serde rather than re-spelled here, so a
/// token this row shows is exactly the token the wire carries — `refutation`
/// and `disposition` are the two signed subtypes of a `verdict`.
fn verdict_token(payload: &CodingSessionTeamTransactionPayload) -> Option<String> {
    let CodingSessionTeamTransactionBody::Verdict(verdict) = &payload.body else {
        return None;
    };
    let decision = match verdict {
        CodingSessionTeamVerdict::Refutation { decision, .. } => serde_json::to_value(decision),
        CodingSessionTeamVerdict::Disposition { decision, .. } => serde_json::to_value(decision),
    };
    decision.ok()?.as_str().map(str::to_owned)
}

/// The three policy fields Pulse renders, or the withdrawal, or nothing.
fn apply_policy_facts(facts: &mut PulseMissionFacts, sources: &PulseMissionSources<'_>) {
    let grants = sources.policy_grants;
    let founder = sources.context.founder_pubkey.clone();
    let may_set_policy = |author: &str, created_at: u64| {
        crate::coding_session_policy::signer_may_steer_at(author, created_at, &founder, grants)
    };
    let fold = fold_coding_session_policies(
        sources.policy_events,
        &sources.context.session_ref,
        &sources.context.genesis_ref,
        &sources.context.founder_pubkey,
        &may_set_policy,
    );
    let Some(selected) = fold.selected else {
        return;
    };
    facts.policy.author = Some(selected.author.clone());
    if !selected.record.sets_any_policy() {
        facts.policy.withdrawn = true;
        return;
    }
    // Serialised through the record's own serde so the word Pulse prints is the
    // word the wire carries.
    facts.policy.posture = selected.record.posture.and_then(|posture| {
        serde_json::to_value(posture)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
    });
    facts.policy.budget_turns = selected
        .record
        .budget
        .as_ref()
        .and_then(|budget| budget.turns);
    facts.policy.irreversible = selected
        .record
        .irreversible
        .clone()
        .unwrap_or_default()
        .iter()
        .map(|act| act.as_str().to_owned())
        .collect();
}

/// Checkpoints, gate rows with their provenance, phase timings, and what is owed.
fn apply_observation_facts(
    facts: &mut PulseMissionFacts,
    team: &CodingSessionTeamFold,
    sources: &PulseMissionSources<'_>,
) {
    let context = CodingSessionObservationFoldContext {
        session_ref: sources.context.session_ref.clone(),
        genesis_ref: sources.context.genesis_ref.clone(),
        known_assignment_refs: team
            .assignments
            .iter()
            .map(|settlement| settlement.assignment_event_id.clone())
            .collect(),
        // `None`, and it is not an empty set: Pulse is handed signed events and
        // a fold context, never the provider instances that ran the session's
        // executions, so it cannot check who signed an `observed` claim. The
        // fold therefore leaves every claim standing and reports
        // `provenance_checked: false`. **A Pulse gate row saying `observed` is
        // the row's own word for itself, not a checked measurement** — the one
        // place in this wave where L5's rule does not run. Threading the
        // executions' signer pubkeys into `PulseMissionSources` is the fix and
        // is owed.
        provider_pubkeys: None,
    };
    let fold = fold_coding_session_observations(sources.observation_events, &context);
    let by_id: BTreeMap<String, &Event> = sources
        .observation_events
        .iter()
        .map(|event| (event.id.to_hex(), event))
        .collect();

    let mut seats: BTreeMap<String, PulseMissionSeat> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let ensure =
        |seats: &mut BTreeMap<String, PulseMissionSeat>, order: &mut Vec<String>, pubkey: &str| {
            if !seats.contains_key(pubkey) {
                order.push(pubkey.to_owned());
                let role = sources
                    .context
                    .active_seats
                    .iter()
                    .find(|seat| seat.actor_pubkey == pubkey)
                    .map(|seat| seat.role.clone());
                seats.insert(
                    pubkey.to_owned(),
                    PulseMissionSeat {
                        pubkey: pubkey.to_owned(),
                        role,
                        checkpoint: None,
                        gates: Vec::new(),
                        gates_truncated: 0,
                        owed: Vec::new(),
                        wip: None,
                    },
                );
            }
        };

    // Every seat the signed chain knows about gets a row, so a seat with no
    // gate row is visible as exactly that rather than absent.
    for seat in &sources.context.active_seats {
        ensure(&mut seats, &mut order, &seat.actor_pubkey);
    }

    for entry in &fold.checkpoints {
        ensure(&mut seats, &mut order, &entry.author_pubkey);
        let at = by_id
            .get(&entry.event_id)
            .map(|event| event.created_at.as_secs() as i64);
        let candidate = PulseMissionCheckpoint {
            phase: entry.body.phase,
            tests_written: entry.body.tests_written,
            tests_red: entry.body.tests_red,
            tests_green: entry.body.tests_green,
            at,
        };
        if let Some(seat) = seats.get_mut(&entry.author_pubkey) {
            // Checkpoints are never deduped by the fold: each is a moment. The
            // newest of them by supplied order is the one Pulse shows.
            seat.checkpoint = Some(candidate);
        }
    }

    // Provenance: an observed row is attributed to the seat whose assignment it
    // names, because the provider signs it under its own key.
    let assignee: BTreeMap<String, String> = team
        .assignments
        .iter()
        .filter_map(|settlement| {
            let event = sources
                .team_events
                .iter()
                .find(|event| event.id.to_hex() == settlement.assignment_event_id)?;
            let assignee = assignment_assignee(event)?;
            Some((settlement.assignment_event_id.clone(), assignee))
        })
        .collect();

    let mut resolved: BTreeMap<(String, String), PulseMissionGate> = BTreeMap::new();
    for entry in &fold.gates {
        let event_id = entry.event_ids.last().cloned().unwrap_or_default();
        let source = by_id
            .get(&event_id)
            .and_then(|event| sources.gate_source_token(event, &entry.row.gate));
        let source = PulseGateSource::from_wire_token(source.as_deref());
        let author = match (source, entry.assignment_ref.as_ref()) {
            (PulseGateSource::Observed, Some(reference)) => assignee
                .get(reference)
                .cloned()
                .unwrap_or_else(|| entry.author_pubkey.clone()),
            _ => entry.author_pubkey.clone(),
        };
        let key = (author.clone(), entry.row.gate.clone());
        let candidate = PulseMissionGate {
            gate: entry.row.gate.clone(),
            outcome: entry.row.outcome,
            command: entry.row.command.clone(),
            source,
            over_declared: false,
            event_id,
        };
        match resolved.get_mut(&key) {
            // An observed row beats a declared row for the same (author, gate),
            // and says so — never silently.
            Some(existing) if existing.source < candidate.source => {
                let over_declared = existing.source == PulseGateSource::Declared;
                *existing = candidate;
                existing.over_declared = over_declared;
            }
            // A newer row of the same provenance replaces the older one, but the
            // disclosure is **sticky**: once an observed row has displaced a
            // declared one, a later observed row for the same `(author, gate)`
            // is still standing over a claim somebody made, and dropping the
            // clause would quietly retire the only evidence that it did
            // (REVIEW-L9 F11).
            Some(existing) if existing.source == candidate.source => {
                let over_declared = existing.over_declared;
                *existing = candidate;
                existing.over_declared = over_declared;
            }
            Some(_) => {}
            None => {
                resolved.insert(key, candidate);
            }
        }
    }
    for ((author, _), gate) in resolved {
        ensure(&mut seats, &mut order, &author);
        if let Some(seat) = seats.get_mut(&author) {
            seat.gates.push(gate);
        }
    }

    for settlement in &team.assignments {
        if settlement.settled {
            continue;
        }
        let Some(pubkey) = assignee.get(&settlement.assignment_event_id) else {
            continue;
        };
        ensure(&mut seats, &mut order, pubkey);
        let assigned_at = sources
            .team_events
            .iter()
            .find(|event| event.id.to_hex() == settlement.assignment_event_id)
            .map(|event| event.created_at.as_secs() as i64);
        if let Some(seat) = seats.get_mut(pubkey) {
            seat.owed.push(PulseMissionOwed {
                assignment_id: settlement.assignment_event_id.clone(),
                assigned_at,
            });
        }
    }

    for seat in seats.values_mut() {
        sort_and_bound_gates(seat);
    }

    facts.seats = order
        .into_iter()
        .filter_map(|pubkey| seats.remove(&pubkey))
        .collect();

    facts.timing = fold
        .phases
        .iter()
        .take(MAX_PULSE_MISSION_TIMING_LINES)
        .map(|entry| PulseMissionPhase {
            phase: entry.body.phase.clone(),
            duration_ms: entry.body.duration_ms.or_else(|| {
                entry
                    .body
                    .ended_at_ms
                    .and_then(|end| end.checked_sub(entry.body.started_at_ms))
            }),
        })
        .collect();
}

/// Order `failed`, `not-run`, `passed`, then by name; bound with a visible count.
fn sort_and_bound_gates(seat: &mut PulseMissionSeat) {
    seat.gates.sort_by(|left, right| {
        outcome_rank(left.outcome)
            .cmp(&outcome_rank(right.outcome))
            .then_with(|| left.gate.cmp(&right.gate))
    });
    if seat.gates.len() > MAX_PULSE_MISSION_GATE_LINES {
        seat.gates_truncated = seat.gates.len() - MAX_PULSE_MISSION_GATE_LINES;
        seat.gates.truncate(MAX_PULSE_MISSION_GATE_LINES);
    }
}

/// Failures first: a reader scanning one line must not miss a red gate.
const fn outcome_rank(outcome: CodingSessionObservationGateOutcome) -> u8 {
    match outcome {
        CodingSessionObservationGateOutcome::Failed => 0,
        CodingSessionObservationGateOutcome::NotRun => 1,
        CodingSessionObservationGateOutcome::Passed => 2,
    }
}

/// The actor an `assignment` names.
fn assignment_assignee(event: &Event) -> Option<String> {
    match transaction_payload(event)?.body {
        CodingSessionTeamTransactionBody::Assignment(assignment) => Some(assignment.assignee_actor),
        _ => None,
    }
}

/// What moved, from the relay's own ref state — never from a push history.
fn apply_ref_state(
    facts: &mut PulseMissionFacts,
    sources: &PulseMissionSources<'_>,
    now_unix: i64,
) {
    let subjects: BTreeMap<String, String> = sources
        .observation_events
        .iter()
        .filter_map(checkpoint_sha_subject)
        .collect();

    // 30618 is parameterized-replaceable: one ref has one current state. Two
    // supplied versions of the same ref collapse to the newer one rather than
    // becoming two rows, because the relay never held a push history to show.
    let mut newest_by_ref: BTreeMap<&str, &PulseRefState> = BTreeMap::new();
    for state in sources.ref_state {
        let entry = newest_by_ref
            .entry(state.ref_name.as_str())
            .or_insert(state);
        if (state.as_of, &state.sha) > (entry.as_of, &entry.sha) {
            *entry = state;
        }
    }

    let mut moved: Vec<PulseMissionMoved> = newest_by_ref
        .values()
        .map(|state| {
            let kind = if is_wip_ref(&state.ref_name) {
                PulseMovedKind::Wip
            } else {
                PulseMovedKind::Landing
            };
            PulseMissionMoved {
                kind,
                sha: state.sha.clone(),
                ref_name: state.ref_name.clone(),
                author_pubkey: state.pusher_pubkey.clone(),
                subject: subjects.get(&state.sha).cloned(),
                age_seconds: state
                    .as_of
                    .and_then(|as_of| now_unix.checked_sub(as_of))
                    .filter(|seconds| *seconds >= 0),
                verdict: match kind {
                    PulseMovedKind::Landing => facts.verdict.clone(),
                    PulseMovedKind::Wip => None,
                },
            }
        })
        .collect();
    moved.sort_by(|left, right| left.ref_name.cmp(&right.ref_name));
    moved.truncate(MAX_PULSE_MISSION_MOVED_ROWS);

    for state in newest_by_ref.values() {
        if !is_wip_ref(&state.ref_name) {
            continue;
        }
        if let Some(seat) = facts
            .seats
            .iter_mut()
            .find(|seat| seat.pubkey == state.pusher_pubkey)
        {
            seat.wip = Some(PulseMissionWip {
                ref_name: state.ref_name.clone(),
                sha: state.sha.clone(),
                as_of: state.as_of,
            });
        }
    }
    facts.moved = moved;
}

/// The `(sha, subject)` a checkpoint recorded, if it recorded one.
///
/// The subject is the seat's own words, carried verbatim and **parsed by
/// nothing** (I5).
fn checkpoint_sha_subject(event: &Event) -> Option<(String, String)> {
    let content: Value = serde_json::from_str(&event.content).ok()?;
    let body = content.get("body")?;
    let sha = body.get("sha")?.as_str()?.to_owned();
    let subject = body.get("subject")?.as_str()?.to_owned();
    Some((sha, subject))
}

#[path = "pulse_mission_render.rs"]
mod render;
pub use render::*;

#[cfg(test)]
#[path = "pulse_mission_tests.rs"]
mod tests;
