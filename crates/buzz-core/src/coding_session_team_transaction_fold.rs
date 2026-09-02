//! Deterministic, storage-independent fold for NIP-CSTX event sets.
//!
//! The caller supplies a verified session/authority projection. This module
//! performs no I/O and never infers authority or terminal state from silence.

use std::collections::{HashMap, HashSet};

use nostr::Event;

use super::{
    validate_coding_session_team_transaction_envelope,
    validate_coding_session_team_transaction_supersession, validate_event_id, validate_role,
    CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CodingSessionTeamTransactionType, CodingSessionTeamVerdict,
};

/// One currently active role seat supplied by the authority/session reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamActiveSeat {
    /// Canonical lowercase-hex actor pubkey.
    pub actor_pubkey: String,
    /// Canonical `[a-z0-9-]+` role slug.
    pub role: String,
}

/// One active, signed authority grant supplied by the authority-chain reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamActiveGrant {
    /// Canonical lowercase-hex grantee pubkey.
    pub actor_pubkey: String,
    /// Event id of the active signed grant from which this projection came.
    pub grant_event_ref: String,
    /// Whether the active grant gives the operator steering standing.
    pub may_steer: bool,
}

/// Complete context required to authorize and fold one session's transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldContext {
    /// Canonical channel UUID expected in every event's `h` tag.
    pub channel_ref: String,
    /// Canonical umbrella UUID expected in every event and `d` tag.
    pub session_ref: String,
    /// Immutable genesis event id expected in every event.
    pub genesis_ref: String,
    /// Pubkey of the genesis signer.
    pub founder_pubkey: String,
    /// Current signed seat projection; only exact `lead` and `verifier` roles
    /// affect the v1 authority matrix.
    pub active_seats: Vec<CodingSessionTeamActiveSeat>,
    /// Current signed grant projection. Only grants with `may_steer=true`
    /// qualify an operator for founder/lead operations.
    pub active_grants: Vec<CodingSessionTeamActiveGrant>,
}

impl CodingSessionTeamFoldContext {
    fn validate(&self) -> Result<(), String> {
        super::validate_canonical_uuid("context.channelRef", &self.channel_ref)?;
        super::validate_canonical_uuid("context.sessionRef", &self.session_ref)?;
        validate_event_id("context.genesisRef", &self.genesis_ref)?;
        validate_event_id("context.founderPubkey", &self.founder_pubkey)?;
        for seat in &self.active_seats {
            validate_event_id("context.activeSeats.actorPubkey", &seat.actor_pubkey)?;
            validate_role(&seat.role)?;
        }
        for grant in &self.active_grants {
            validate_event_id("context.activeGrants.actorPubkey", &grant.actor_pubkey)?;
            validate_event_id("context.activeGrants.grantEventRef", &grant.grant_event_ref)?;
        }
        Ok(())
    }

    fn is_active_role(&self, author: &str, role: &str) -> bool {
        self.active_seats
            .iter()
            .any(|seat| seat.actor_pubkey == author && seat.role == role)
    }

    /// Whether `author` may act for the mission as a whole: the founder, an
    /// active `lead` seat, or an actor holding a steer grant.
    ///
    /// Public since batch 2 lane B2 so `bee sessions policy set|clear` can
    /// pre-check standing against the same context the fold judges with,
    /// rather than growing a second, drifting copy of this rule in the CLI.
    /// It remains a **courtesy to the author**: the fold, not the CLI, decides
    /// whether a published record counts.
    pub fn may_lead(&self, author: &str) -> bool {
        author == self.founder_pubkey
            || self.is_active_role(author, "lead")
            || self
                .active_grants
                .iter()
                .any(|grant| grant.actor_pubkey == author && grant.may_steer)
    }
}

/// Why a structurally valid event is absent from the canonical projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingSessionTeamFoldExclusionCode {
    /// The signer lacks the operation's required standing.
    Unauthorized,
    /// A causal or `supersedes` reference names an event that is absent from
    /// the supplied set, so this one record cannot be placed in the graph.
    ///
    /// This is a defect of the single record that carries the reference, not
    /// of the supplied set: one seat publishing a malformed pointer must not
    /// deny every other seat the governance projection. Contrast the failures
    /// that remain hard errors in [`fold_coding_session_team_transactions`] —
    /// only cross-context and cycle inputs are the *caller's* filter being
    /// wrong, and have no safe partial meaning.
    DanglingReference,
    /// A reference resolves to a supplied record that cannot stand where this
    /// record puts it: the wrong operation type (a report's `assignmentRef`
    /// naming a verdict), the wrong verdict subtype (a disposition's
    /// `refutationRef` naming another disposition), or a pointer that
    /// contradicts the record it names (a verdict whose `assignmentRef` is not
    /// the one its report answers).
    ///
    /// One record's own defect, like [`Self::DanglingReference`] and
    /// [`Self::InvalidCorrection`]. The precise diagnostic is carried verbatim
    /// in [`CodingSessionTeamFoldExclusion::reason`]; this code is the class.
    WrongTypeReference,
    /// This record claims to correct another and the correction is invalid:
    /// it changes the logical subject, the signed author, the operation type
    /// or the verdict subtype, or its envelope disagrees with the record it
    /// names.
    ///
    /// Like [`Self::DanglingReference`] this is one record's own defect. The
    /// record it names is left exactly as the projection already found it —
    /// a rejected correction never revives, displaces or alters its target.
    InvalidCorrection,
    /// A required canonical parent was individually unauthorized.
    DependentOnUnauthorized,
    /// A required parent lost correction projection or was superseded.
    DependentOnSuperseded,
    /// A required parent was excluded for another stable reason.
    DependentOnExcluded,
    /// A valid correction replaces this event.
    Superseded,
    /// Another head of the same correction fork won deterministic ordering.
    CorrectionConflict,
    /// A completion did not prove every referenced assignment's approval chain.
    CompletionNotApproved,
    /// A completion named an assignment that a canonical, still-unanswered
    /// `decision.request` declares itself blocking.
    ///
    /// The mission asked a named party for a ruling and then declared itself
    /// finished without it. Both answers are signed and they contradict each
    /// other, so the completion is excluded and
    /// [`CodingSessionTeamFold::waiting_on_decision`] keeps meaning exactly one
    /// thing: work that cannot finish until a person rules. Answer the request
    /// (or publish one that blocks nothing) and the completion folds.
    CompletionBlockedByOpenDecision,
    /// Another authorized terminal event is newer.
    TerminalConflict,
}

/// One excluded event and the stable reason for exclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldExclusion {
    /// Excluded event id.
    pub event_id: String,
    /// Machine-readable exclusion class.
    pub code: CodingSessionTeamFoldExclusionCode,
    /// Bounded diagnostic suitable for logs and conformance tests.
    pub reason: String,
}

/// A deterministic choice among competing semantic heads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFoldConflict {
    /// Stable logical subject of the conflict.
    pub subject: String,
    /// Winner selected by `(created_at, event id)`.
    pub winner_event_id: String,
    /// All competing heads, ordered by `(created_at, event id)`.
    pub contender_event_ids: Vec<String>,
}

/// Canonical approval state for one active assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamAssignmentSettlement {
    /// Active assignment event id.
    pub assignment_event_id: String,
    /// Explicitly governed report, when an approval chain is complete.
    pub governed_report_event_id: Option<String>,
    /// Approving disposition acknowledged by the assigned actor.
    pub disposition_event_id: Option<String>,
    /// Assigned actor's acknowledgement of that disposition.
    pub acknowledgement_event_id: Option<String>,
    /// True only for disposition `approve` or `approve-with-notes` plus the
    /// assigned actor's explicit acknowledgement.
    pub settled: bool,
}

/// One included report whose author holds no active seat for the role its
/// assignment named.
///
/// Inclusion is assignee equality, deliberately: an assignment names a
/// *target*, not an authorship claim, so a report the assigned actor signed is
/// canonical whether or not a `grant-seat` for that role was ever accepted.
/// The missing seat is a separate fact and a real one — an ungranted actor
/// cannot hold `verifier` authority, and nothing else in the projection
/// notices — so it is disclosed here rather than folded into exclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamUnseatedReport {
    /// Event id of the included report.
    pub event_id: String,
    /// Canonical lowercase-hex pubkey that signed the report.
    pub author_pubkey: String,
    /// Event id of the assignment the report answers.
    pub assignment_ref: String,
    /// Role slug that assignment named for its assignee.
    pub assignee_role: String,
}

/// Canonical newest authorized terminal transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamCanonicalTerminal {
    /// Terminal event id.
    pub event_id: String,
    /// Exactly `mission.completed` or `mission.blocked`.
    pub transaction_type: CodingSessionTeamTransactionType,
}

/// Deterministic projection of a supplied NIP-CSTX event set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTeamFold {
    /// Canonical active facts after authority and correction projection.
    pub included_event_ids: Vec<String>,
    /// Rejected or displaced events with explicit reasons.
    pub excluded: Vec<CodingSessionTeamFoldExclusion>,
    /// Correction, governance, and terminal conflicts with deterministic winners.
    pub conflicts: Vec<CodingSessionTeamFoldConflict>,
    /// Approval state for every active assignment.
    pub assignments: Vec<CodingSessionTeamAssignmentSettlement>,
    /// Included reports whose author holds no active seat for the assignment's
    /// `assigneeRole`, in included order. Disclosure, never exclusion.
    ///
    /// Measured against [`CodingSessionTeamFoldContext::active_seats`], which
    /// the caller is required to supply *complete*. An empty seat projection
    /// therefore reads as "nobody is seated", not as "seats unknown", and
    /// would list every included report. Both current callers fail closed
    /// before that can happen — the CLI errors out of
    /// `fetch_projected_authority`, and the Tauri adapter refuses an active
    /// projection with no authority-head provenance — but a caller that ever
    /// passes a partial projection must render this as unknown rather than as
    /// a roster of unseated authors.
    pub unseated_reports: Vec<CodingSessionTeamUnseatedReport>,
    /// Canonical notes in included order. Listing, never state: a note can
    /// neither settle an assignment nor end a mission.
    ///
    /// Bounded by the supplied event set exactly as every other collection here
    /// is, and each entry is fixed width — two event ids plus at most
    /// [`crate::coding_session_team_transaction::MAX_TEAM_TRANSACTION_NOTE_REFS`]
    /// pointers. No entry is ever dropped: a note queue that silently hid notes
    /// would be the same lie the verb exists to fix.
    pub notes: Vec<CodingSessionTeamFoldNote>,
    /// Canonical decision requests with their answers, in included order.
    pub decisions: Vec<CodingSessionTeamFoldDecision>,
    /// The oldest canonical `decision.request` nobody has answered, when there
    /// is one.
    ///
    /// A question held on a party with no answer on the wire, whatever its
    /// `blocks` names (finding 16). Which assignments that question holds up is
    /// a different fact, kept in
    /// [`CodingSessionTeamFoldDecision::blocks`] and read only by
    /// [`CodingSessionTeamFoldExclusionCode::CompletionBlockedByOpenDecision`].
    pub waiting_on_decision: Option<CodingSessionTeamFoldWaitingOnDecision>,
    /// Newest authorized valid terminal event, never inferred from silence.
    pub canonical_terminal: Option<CodingSessionTeamCanonicalTerminal>,
}

struct Record<'a> {
    event: &'a Event,
    id: String,
    author: String,
    payload: CodingSessionTeamTransactionPayload,
}

#[derive(Clone, Copy)]
enum ProjectionStage {
    Assignment,
    Report,
    Refutation,
    Disposition,
    Acknowledgement,
    Note,
    DecisionRequest,
    DecisionAnswer,
    /// Both terminals, projected together.
    ///
    /// One stage rather than two since batch 2 lane B2. A correction group is
    /// built inside a stage, so two stages made a `mission.completed` that
    /// supersedes a `mission.blocked` a correction of a record its own stage
    /// could not see: the completion was excluded as a dependant of an
    /// inactive parent and the blocked stayed canonical — the opposite of what
    /// finding 14 asked for. Uncorrected terminals are unaffected: they form
    /// separate correction groups, both stay active, and `project_terminal`
    /// records the `terminal` conflict exactly as before.
    Terminal,
}

/// Validate and fold signed transactions against a supplied authority context.
///
/// **Exactly two classes of *record* input are hard errors**, because only they
/// mean the *caller* filtered wrongly and have no safe partial meaning:
/// **cross-context** events (wrong session, genesis or channel) and reference
/// **cycles**. Nothing a single seat can put in a single well-formed record can
/// fail this fold.
///
/// Three further whole-set failures exist that are not judgements about a
/// governance record: a malformed caller `context`, the same event supplied
/// twice, and an event whose signature or envelope does not validate. The last
/// is record-shaped in principle; the relay validates both at ingest, so no
/// stored event reaches it.
///
/// Everything a single seat can get wrong in a single record is that record's
/// own defect and excludes only that record, because one malformed publication
/// must never deny every seat the governance projection:
///
/// - a **dangling** reference (an id nobody supplied) →
///   [`CodingSessionTeamFoldExclusionCode::DanglingReference`];
/// - a **wrong-type** reference (a resolvable id that cannot stand where the
///   record puts it) →
///   [`CodingSessionTeamFoldExclusionCode::WrongTypeReference`];
/// - an **invalid correction** (changed subject, author, operation type or
///   verdict subtype, or a disagreeing envelope) →
///   [`CodingSessionTeamFoldExclusionCode::InvalidCorrection`], leaving the
///   record it names exactly as the projection already found it.
///
/// Dependants of an excluded record fall out as
/// [`CodingSessionTeamFoldExclusionCode::DependentOnExcluded`]. Valid but
/// unauthorized or deterministically displaced events are disclosed in
/// [`CodingSessionTeamFold::excluded`] as before.
pub fn fold_coding_session_team_transactions(
    events: &[Event],
    context: &CodingSessionTeamFoldContext,
) -> Result<CodingSessionTeamFold, String> {
    context.validate()?;
    let mut records = Vec::with_capacity(events.len());
    let mut by_id = HashMap::with_capacity(events.len());
    for event in events {
        crate::verify_event(event)
            .map_err(|error| format!("invalid team-transaction signature: {error}"))?;
        let payload = validate_coding_session_team_transaction_envelope(event)?;
        let id = event.id.to_hex();
        if by_id.insert(id.clone(), records.len()).is_some() {
            return Err(format!("duplicate supplied team transaction {id}"));
        }
        if payload.session_ref != context.session_ref {
            return Err(format!(
                "team transaction {id} crosses the supplied session"
            ));
        }
        if payload.genesis_ref != context.genesis_ref {
            return Err(format!(
                "team transaction {id} crosses the supplied genesis"
            ));
        }
        let channel = event.tags.as_slice()[0].as_slice()[1].as_str();
        if channel != context.channel_ref {
            return Err(format!(
                "team transaction {id} crosses the supplied channel"
            ));
        }
        records.push(Record {
            event,
            id,
            author: event.pubkey.to_hex(),
            payload,
        });
    }

    let defects = collect_record_defects(&records, &by_id);
    reject_cycles(&records, &by_id)?;

    let mut excluded = Vec::new();
    let mut authorized = HashSet::new();
    for (index, record) in records.iter().enumerate() {
        if let Some(defect) = defects.get(&index) {
            excluded.push(CodingSessionTeamFoldExclusion {
                event_id: record.id.clone(),
                code: defect.code,
                reason: defect.reason.clone(),
            });
            continue;
        }
        match is_authorized(record, &records, &by_id, context) {
            Ok(true) => {
                authorized.insert(index);
            }
            Ok(false) => excluded.push(CodingSessionTeamFoldExclusion {
                event_id: record.id.clone(),
                code: CodingSessionTeamFoldExclusionCode::Unauthorized,
                reason: "signer lacks the operation's required active authority".into(),
            }),
            // `is_authorized` walks pointers two hops out. Every such hop is
            // typed by `collect_record_defects` first, so this arm should be
            // unreachable — but an unreachability argument is exactly what
            // REVIEW-B1b F5 falsified, so a shape defect found here excludes
            // this record *by construction* and can never fail the whole set.
            Err(detail) => excluded.push(CodingSessionTeamFoldExclusion {
                event_id: record.id.clone(),
                code: CodingSessionTeamFoldExclusionCode::WrongTypeReference,
                reason: detail,
            }),
        }
    }

    let mut active = Vec::new();
    let mut conflicts = Vec::new();
    for stage in [
        ProjectionStage::Assignment,
        ProjectionStage::Report,
        ProjectionStage::Refutation,
        ProjectionStage::Disposition,
        ProjectionStage::Acknowledgement,
        ProjectionStage::Note,
        ProjectionStage::DecisionRequest,
        ProjectionStage::DecisionAnswer,
    ] {
        project_stage(
            &records,
            &by_id,
            &authorized,
            &mut active,
            &mut excluded,
            &mut conflicts,
            stage,
        );
    }

    let mut assignments = settle_assignments(&records, &active, &mut conflicts);
    let settled: HashMap<&str, bool> = assignments
        .iter()
        .map(|state| (state.assignment_event_id.as_str(), state.settled))
        .collect();

    let open_blocks = open_request_blocks(&records, &active);
    let mut terminal_authorized = authorized.clone();
    for &index in &authorized {
        let CodingSessionTeamTransactionBody::MissionCompleted(body) = &records[index].payload.body
        else {
            continue;
        };
        let all_assignments_active = body.assignment_refs.iter().all(|reference| {
            by_id
                .get(reference)
                .is_some_and(|assignment| active.contains(assignment))
        });
        if all_assignments_active
            && !body
                .assignment_refs
                .iter()
                .all(|reference| settled.get(reference.as_str()) == Some(&true))
        {
            terminal_authorized.remove(&index);
            excluded.push(CodingSessionTeamFoldExclusion {
                event_id: records[index].id.clone(),
                code: CodingSessionTeamFoldExclusionCode::CompletionNotApproved,
                reason: "mission.completed requires every named active assignment to have an acknowledged approving disposition".into(),
            });
            continue;
        }
        if let Some(reason) = completion_blocked_by_open_decision(body, &open_blocks) {
            terminal_authorized.remove(&index);
            excluded.push(CodingSessionTeamFoldExclusion {
                event_id: records[index].id.clone(),
                code: CodingSessionTeamFoldExclusionCode::CompletionBlockedByOpenDecision,
                reason,
            });
        }
    }
    // One stage, not two: see [`ProjectionStage::Terminal`].
    project_stage(
        &records,
        &by_id,
        &terminal_authorized,
        &mut active,
        &mut excluded,
        &mut conflicts,
        ProjectionStage::Terminal,
    );

    let canonical_terminal = project_terminal(&records, &mut active, &mut excluded, &mut conflicts);
    sort_indices(&mut active, &records);
    assignments.sort_by(|left, right| left.assignment_event_id.cmp(&right.assignment_event_id));
    excluded.sort_by(|left, right| left.event_id.cmp(&right.event_id));

    let unseated_reports = disclose_unseated_reports(&records, &by_id, &active, context);
    let notes = list_notes(&records, &active);
    let decisions = project_decisions(&records, &active, &mut conflicts);
    let waiting_on_decision = waiting_on_decision(&decisions);
    conflicts.sort_by(|left, right| {
        left.subject
            .cmp(&right.subject)
            .then_with(|| left.winner_event_id.cmp(&right.winner_event_id))
    });

    Ok(CodingSessionTeamFold {
        included_event_ids: active
            .into_iter()
            .map(|index| records[index].id.clone())
            .collect(),
        excluded,
        conflicts,
        assignments,
        unseated_reports,
        notes,
        decisions,
        waiting_on_decision,
        canonical_terminal,
    })
}

/// List every included report whose author holds no active seat for the role
/// its assignment named, in the projection's own included order.
///
/// Reads only `active`, so a report excluded for any other reason is never
/// listed: this says "this canonical report carries no seat authority", which
/// is a different sentence from "this report is not canonical".
fn disclose_unseated_reports(
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
    active: &[usize],
    context: &CodingSessionTeamFoldContext,
) -> Vec<CodingSessionTeamUnseatedReport> {
    let mut disclosed = Vec::new();
    for &index in active {
        let CodingSessionTeamTransactionBody::Report(body) = &records[index].payload.body else {
            continue;
        };
        let Some(assignment) = by_id
            .get(&body.assignment_ref)
            .map(|reference| &records[*reference])
        else {
            continue;
        };
        let CodingSessionTeamTransactionBody::Assignment(assignment_body) =
            &assignment.payload.body
        else {
            continue;
        };
        let author = &records[index].author;
        if context.is_active_role(author, &assignment_body.assignee_role) {
            continue;
        }
        disclosed.push(CodingSessionTeamUnseatedReport {
            event_id: records[index].id.clone(),
            author_pubkey: author.clone(),
            assignment_ref: body.assignment_ref.clone(),
            assignee_role: assignment_body.assignee_role.clone(),
        });
    }
    disclosed
}

// The `note` and `decision.*` projections live in a sibling file for the same
// reason, and are children of this module for the same access.
#[path = "coding_session_team_transaction_fold_decisions.rs"]
mod decisions;

use decisions::{
    completion_blocked_by_open_decision, list_notes, open_request_blocks, project_decisions,
    waiting_on_decision,
};
pub use decisions::{
    CodingSessionTeamFoldDecision, CodingSessionTeamFoldNote,
    CodingSessionTeamFoldWaitingOnDecision,
};

// Stage projection and correction-group resolution live in a sibling file for
// the same reason (FINAL-B §7 asked B2 to split this file).
#[path = "coding_session_team_transaction_fold_projection.rs"]
mod projection;
use projection::project_stage;

// Record-local defect detection lives in a sibling file so no file here passes
// 1,000 lines (REVIEW-B1b R6). It is a child module, so it reads this module's
// private `Record`, `verdict_subtype` and `logical_subject` unchanged.
#[path = "coding_session_team_transaction_fold_defects.rs"]
mod defects;

use defects::collect_record_defects;

fn reject_cycles(records: &[Record<'_>], by_id: &HashMap<String, usize>) -> Result<(), String> {
    fn visit(
        index: usize,
        records: &[Record<'_>],
        by_id: &HashMap<String, usize>,
        state: &mut [u8],
    ) -> Result<(), String> {
        if state[index] == 1 {
            return Err("team transaction reference graph contains a cycle".into());
        }
        if state[index] == 2 {
            return Ok(());
        }
        state[index] = 1;
        let mut references = records[index].payload.causal_references();
        if let Some(reference) = &records[index].payload.supersedes {
            references.push(reference);
        }
        for reference in references {
            // An absent parent is no edge at all; the record that names it is
            // excluded as `DanglingReference` and cannot close a cycle.
            let Some(&parent) = by_id.get(reference) else {
                continue;
            };
            visit(parent, records, by_id, state)?;
        }
        state[index] = 2;
        Ok(())
    }

    let mut state = vec![0; records.len()];
    for index in 0..records.len() {
        visit(index, records, by_id, &mut state)?;
    }
    Ok(())
}

fn is_authorized(
    record: &Record<'_>,
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
    context: &CodingSessionTeamFoldContext,
) -> Result<bool, String> {
    Ok(match &record.payload.body {
        CodingSessionTeamTransactionBody::Assignment(_)
        | CodingSessionTeamTransactionBody::MissionCompleted(_)
        | CodingSessionTeamTransactionBody::MissionBlocked(_) => context.may_lead(&record.author),
        CodingSessionTeamTransactionBody::Report(body) => {
            // Indexing is safe here, and at every other `by_id[..]` in this
            // file, because `collect_record_defects` runs first and gives any
            // record with an unresolvable pointer a `DanglingReference`; the
            // authorization loop `continue`s past every such record before
            // this line is reached (REVIEW-B1b F3).
            assignment_actor(&records[by_id[&body.assignment_ref]])? == record.author
        }
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            ..
        }) => context.is_active_role(&record.author, "verifier"),
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
            ..
        }) => context.may_lead(&record.author),
        CodingSessionTeamTransactionBody::Acknowledgement(body) => {
            // Safe by the same invariant: an acknowledgement naming an absent
            // disposition is already excluded `DanglingReference` and never
            // reaches `is_authorized`.
            let disposition = &records[by_id[&body.acknowledged_event_ref]];
            let CodingSessionTeamTransactionBody::Verdict(verdict) = &disposition.payload.body
            else {
                return Err("acknowledgement reference must name a verdict body".into());
            };
            // The disposition resolves, but *its* assignment may be the absent
            // one. Standing is then unknowable, so this record is passed to
            // stage projection rather than judged here: the dangling
            // disposition is already excluded, so the acknowledgement falls out
            // as `DependentOnExcluded` — the true reason — instead of being
            // mislabelled `Unauthorized`.
            match by_id.get(verdict.assignment_ref()) {
                Some(&assignment) => assignment_actor(&records[assignment])? == record.author,
                None => true,
            }
        }
        // Saying something and asking for a ruling need participation, not
        // standing over anyone else's work.
        CodingSessionTeamTransactionBody::Note(_)
        | CodingSessionTeamTransactionBody::DecisionRequest(_) => context.may_speak(&record.author),
        CodingSessionTeamTransactionBody::DecisionAnswer(body) => {
            // Neither branch below is ever a hard error: an answer that points
            // at nothing, or at something that is not a request, is that one
            // record's own defect and is already excluded as
            // `DanglingReference` or `WrongTypeReference` before this runs.
            // `false` keeps both arms fail-closed anyway.
            match by_id.get(&body.request_ref) {
                Some(&request) => match &records[request].payload.body {
                    CodingSessionTeamTransactionBody::DecisionRequest(request) => {
                        // The founder may always rule; otherwise only the exact
                        // actor the request named holds the answer.
                        record.author == context.founder_pubkey || request.held_on == record.author
                    }
                    _ => false,
                },
                None => false,
            }
        }
    })
}

fn assignment_actor<'a>(record: &'a Record<'_>) -> Result<&'a str, String> {
    let CodingSessionTeamTransactionBody::Assignment(body) = &record.payload.body else {
        return Err("expected assignment record".into());
    };
    Ok(&body.assignee_actor)
}

fn verdict_subtype(payload: &CodingSessionTeamTransactionPayload) -> Option<&'static str> {
    match &payload.body {
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            ..
        }) => Some("refutation"),
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
            ..
        }) => Some("disposition"),
        _ => None,
    }
}

/// The subject a correction must preserve.
///
/// Two records belong to the same correction group when this string matches.
/// `supersedes` changes an operation's *wording*, never what it is about: a
/// report that names the wrong assignment is replaced by a **new** report, not
/// corrected into one about a different assignment (live run TeamRolesV1,
/// 23:15 — the runner's `46b03d08`).
///
/// Public since batch 2 lane B2 so the CLI can refuse that shape **before
/// signing** rather than leaving the fold to exclude it afterwards
/// (`crates/buzz-cli/src/commands/sessions/operations_precheck.rs`). One
/// implementation, read by the writer and by the reader; two would drift.
pub fn logical_subject(payload: &CodingSessionTeamTransactionPayload) -> String {
    match &payload.body {
        CodingSessionTeamTransactionBody::Assignment(body) => {
            format!("assignment:{}:{}", body.assignee_actor, body.assignee_role)
        }
        CodingSessionTeamTransactionBody::Report(body) => {
            format!("report:{}", body.assignment_ref)
        }
        CodingSessionTeamTransactionBody::Verdict(verdict) => format!(
            "verdict:{}:{}:{}",
            verdict_subtype(payload).unwrap_or("unknown"),
            verdict.assignment_ref(),
            verdict.report_ref()
        ),
        CodingSessionTeamTransactionBody::Acknowledgement(body) => {
            format!("acknowledgement:{}", body.acknowledged_event_ref)
        }
        // One subject for both terminals, so a `mission.completed` may correct
        // the `mission.blocked` it supersedes (finding 14). The *direction* is
        // the type rule's job — `validate_coding_session_team_transaction_supersession`
        // allows blocked → completed and refuses completed → blocked — and this
        // string only says the two records are about the same thing: how this
        // mission ended.
        CodingSessionTeamTransactionBody::MissionCompleted(_)
        | CodingSessionTeamTransactionBody::MissionBlocked(_) => "mission.terminal".into(),
        // A note can never carry `supersedes`, so no two notes ever share a
        // correction group and this constant is never used to compare subjects.
        CodingSessionTeamTransactionBody::Note(_) => "note".into(),
        // The holder is part of the subject, so a correction may not move a
        // ruling from the founder onto its own asker (REVIEW-B1c F2): that
        // correction fails `logical_subject` equality and is excluded
        // `InvalidCorrection`. Re-asking a different party stays possible — you
        // publish a new request, which is the honest shape.
        CodingSessionTeamTransactionBody::DecisionRequest(body) => {
            format!("decision.request:{}", body.held_on)
        }
        CodingSessionTeamTransactionBody::DecisionAnswer(body) => {
            format!("decision.answer:{}", body.request_ref)
        }
    }
}

fn settle_assignments(
    records: &[Record<'_>],
    active: &[usize],
    conflicts: &mut Vec<CodingSessionTeamFoldConflict>,
) -> Vec<CodingSessionTeamAssignmentSettlement> {
    let active_set: HashSet<usize> = active.iter().copied().collect();
    let mut result = Vec::new();
    for &assignment_index in active {
        let CodingSessionTeamTransactionBody::Assignment(_) =
            records[assignment_index].payload.body
        else {
            continue;
        };
        let assignment_id = &records[assignment_index].id;
        let mut chains = Vec::new();
        for &disposition_index in active {
            let CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
                assignment_ref,
                report_ref,
                decision,
                ..
            }) = &records[disposition_index].payload.body
            else {
                continue;
            };
            if assignment_ref != assignment_id || !decision.is_approval() {
                continue;
            }
            let Some(report_index) = records.iter().position(|record| &record.id == report_ref)
            else {
                continue;
            };
            if !active_set.contains(&report_index) {
                continue;
            }
            let mut acknowledgements: Vec<usize> = active
                .iter()
                .copied()
                .filter(|index| {
                    matches!(
                        &records[*index].payload.body,
                        CodingSessionTeamTransactionBody::Acknowledgement(body)
                            if body.acknowledged_event_ref == records[disposition_index].id
                    )
                })
                .collect();
            sort_indices(&mut acknowledgements, records);
            if let Some(acknowledgement) = acknowledgements.last() {
                if acknowledgements.len() > 1 {
                    conflicts.push(CodingSessionTeamFoldConflict {
                        subject: format!("acknowledgement:{}", records[disposition_index].id),
                        winner_event_id: records[*acknowledgement].id.clone(),
                        contender_event_ids: acknowledgements
                            .iter()
                            .map(|index| records[*index].id.clone())
                            .collect(),
                    });
                }
                chains.push((disposition_index, report_index, *acknowledgement));
            }
        }
        chains.sort_by(|left, right| compare_records(left.0, right.0, records));
        let winner = chains.last().copied();
        if chains.len() > 1 {
            if let Some((winner_index, _, _)) = winner {
                conflicts.push(CodingSessionTeamFoldConflict {
                    subject: format!("governance:{assignment_id}"),
                    winner_event_id: records[winner_index].id.clone(),
                    contender_event_ids: chains
                        .iter()
                        .map(|chain| records[chain.0].id.clone())
                        .collect(),
                });
            }
        }
        result.push(CodingSessionTeamAssignmentSettlement {
            assignment_event_id: assignment_id.clone(),
            governed_report_event_id: winner.map(|chain| records[chain.1].id.clone()),
            disposition_event_id: winner.map(|chain| records[chain.0].id.clone()),
            acknowledgement_event_id: winner.map(|chain| records[chain.2].id.clone()),
            settled: winner.is_some(),
        });
    }
    result
}

fn project_terminal(
    records: &[Record<'_>],
    active: &mut Vec<usize>,
    excluded: &mut Vec<CodingSessionTeamFoldExclusion>,
    conflicts: &mut Vec<CodingSessionTeamFoldConflict>,
) -> Option<CodingSessionTeamCanonicalTerminal> {
    let mut terminals: Vec<usize> = active
        .iter()
        .copied()
        .filter(|index| {
            matches!(
                records[*index].payload.transaction_type,
                CodingSessionTeamTransactionType::MissionCompleted
                    | CodingSessionTeamTransactionType::MissionBlocked
            )
        })
        .collect();
    sort_indices(&mut terminals, records);
    let winner = *terminals.last()?;
    if terminals.len() > 1 {
        conflicts.push(CodingSessionTeamFoldConflict {
            subject: "terminal".into(),
            winner_event_id: records[winner].id.clone(),
            contender_event_ids: terminals
                .iter()
                .map(|index| records[*index].id.clone())
                .collect(),
        });
    }
    for loser in terminals.into_iter().filter(|index| *index != winner) {
        active.retain(|index| *index != loser);
        excluded.push(CodingSessionTeamFoldExclusion {
            event_id: records[loser].id.clone(),
            code: CodingSessionTeamFoldExclusionCode::TerminalConflict,
            reason: "a newer authorized terminal transaction is canonical".into(),
        });
    }
    Some(CodingSessionTeamCanonicalTerminal {
        event_id: records[winner].id.clone(),
        transaction_type: records[winner].payload.transaction_type,
    })
}

fn compare_records(left: usize, right: usize, records: &[Record<'_>]) -> std::cmp::Ordering {
    records[left]
        .event
        .created_at
        .as_secs()
        .cmp(&records[right].event.created_at.as_secs())
        .then_with(|| records[left].id.cmp(&records[right].id))
}

fn sort_indices(indices: &mut [usize], records: &[Record<'_>]) {
    indices.sort_by(|left, right| compare_records(*left, *right, records));
}

#[cfg(test)]
#[path = "coding_session_team_transaction_fold_tests.rs"]
mod tests;
