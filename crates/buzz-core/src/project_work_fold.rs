//! NIP-PW: the pure coverage fold — what remains, who owes it, what proves it.
//!
//! A pure function of (the kind:44249 event set, the resolved plan blobs, the
//! signer authority projection, the relay's ref state). **No clock, no
//! network, no ordering assumption**: the same events in any arrival order
//! fold to the same output, duplicates are harmless, and nothing here reads a
//! file or a socket.
//!
//! What the fold does and does not verify. It verifies *references and
//! scope*: that a binding names a declaration in this set, that the signer
//! holds `may_lead`, that a criterion exists in the plan the declaration
//! pinned, that the bound ref state is the newest the relay signed. It does
//! **not** re-verify signatures — the relay checked those at ingest and the
//! caller established the facts about referenced kind:44244 reports, verdicts
//! and action results before calling.
//!
//! The output carries **no mission-terminal field**, deliberately. Whether a
//! 44244 `mission.completed` folded to terminal is a different question with
//! a different fold, and a reader that shows both shows two rows. The two
//! disagreeing is a disclosure, not a reconciliation.
//!
//! The normative contract is `conformance/project-work/README.md` § (c),
//! restated in `docs/nips/NIP-PW.md`, and pinned by the five sequences under
//! `conformance/project-work/fixtures/sequences/`.
//!
//! **Where a contract amendment slots in.** The three seams an amendment to
//! § (c) would touch are deliberately separate functions rather than one
//! pass: [`resolve_states`] answers which declaration is the head and where a
//! fork is; `project_work_fold_project.rs`'s `evaluate_proof` is the single
//! per-criterion predicate, one arm per proof form, each returning a status
//! **and a named reason**; and `coverage` is where a whole-declaration
//! judgement (an artifact-commit candidate across criteria, say) belongs.
//! Two inputs are likewise named rather than assumed: `resolved_evidence_ids`
//! is the caller's established-facts set, and `may_lead` is the projection
//! the 44244 authority fold already computes — this fold consumes it and
//! never re-derives it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::kind::KIND_PROJECT_WORK_RECORD;
use crate::project_plan::{parse_plan, Plan, PlanProof};
use crate::project_work::{
    validate_project_work_envelope, ProjectWorkAssignmentBound, ProjectWorkBody,
    ProjectWorkDeclared, ProjectWorkEvent, ProjectWorkEvidenceBound, ProjectWorkEvidenceKind,
    ProjectWorkEvidenceRef, ProjectWorkPlanRef,
};

/// Exact `schema` value a coverage projection carries.
pub const PROJECT_WORK_COVERAGE_SCHEMA: &str = "buzz-project-work-coverage/v1";
/// The kind of a relay-signed repository ref-state event.
pub const KIND_REF_STATE: u32 = 30618;

/// Everything the fold is given. Nothing else is read.
#[derive(Debug, Clone, Default)]
pub struct WorkFoldInputs {
    /// The kind:44249 events. Any other kind is ignored, not excluded: an old
    /// reader never queries this kind, and a mixed stream must fold the same.
    pub events: Vec<ProjectWorkEvent>,
    /// The `may_lead` set the 44244 authority projection already computed,
    /// scoped to this project and session.
    pub may_lead: Vec<String>,
    /// The relay's NIP-11 `self` key. `None` means ref state cannot be
    /// judged, and a `git-ref` criterion reads `unknown` rather than `open`.
    pub relay_self_key: Option<String>,
    /// The session's current kind:44227 goal. `None` means the current goal
    /// was not read, and no declaration is marked stale on that account.
    pub current_goal_ref: Option<String>,
    /// Plan blobs keyed by [`ProjectWorkPlanRef::blob_key`].
    pub plan_blobs: BTreeMap<String, String>,
    /// Relay-signed kind:30618 ref-state events, newest or not: the fold
    /// picks the newest per repository itself.
    pub ref_states: Vec<ProjectWorkEvent>,
    /// Which referenced 44244 evidence events the caller established exist.
    ///
    /// `None` means the caller did not establish the question, and the fold
    /// treats every non-`ref_observation` reference as resolved — the honest
    /// reading, because "we did not look" must not render as "it is missing".
    /// `Some` set makes an unlisted reference `unknown` with a named reason.
    pub resolved_evidence_ids: Option<BTreeSet<String>>,
    /// The session this projection is about; derived from the records when
    /// absent.
    pub session_ref: Option<String>,
    /// The project this projection is about; derived from the records when
    /// absent.
    pub project_ref: Option<String>,
}

/// Where a declaration stands among its siblings for the same `workId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkDeclarationState {
    /// The current declaration: nothing supersedes it, no conflict.
    Head,
    /// Another declaration names it in `supersedes`.
    Superseded,
    /// Still the head, but adopted against a goal that is no longer current.
    Stale,
    /// Two or more unsuperseded successors of the same predecessor exist.
    Conflict,
}

impl WorkDeclarationState {
    /// The order declarations are reported in: current contracts first.
    const fn rank(self) -> u8 {
        match self {
            Self::Head => 0,
            Self::Stale => 1,
            Self::Conflict => 2,
            Self::Superseded => 3,
        }
    }

    /// Whether this state *is* a current contract, and so projects criteria.
    const fn projects_criteria(self) -> bool {
        matches!(self, Self::Head | Self::Stale)
    }
}

/// Where one criterion stands under the head declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkCriterionStatus {
    /// No evidence binding under the head declaration names it.
    Open,
    /// Evidence resolved and the required proof form was satisfied.
    Covered,
    /// Its evidence is bound to a declaration that is not the head, or its
    /// ref observation has been superseded by a newer, different ref state.
    Stale,
    /// The fold could not read an input it needs. A result, not an error.
    Unknown,
}

impl WorkCriterionStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Covered => "covered",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }
}

/// One criterion's standing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkCriterionProjection {
    /// The criterion's slug id.
    pub criterion_id: String,
    /// The proof form the plan requires; `null` when the plan is unresolved.
    pub proof: Option<PlanProof>,
    /// Where it stands.
    pub status: WorkCriterionStatus,
    /// The 44244 assignments bound to it under this declaration.
    pub assignment_refs: Vec<String>,
    /// The evidence bound to it under this declaration.
    pub evidence: Vec<ProjectWorkEvidenceRef>,
    /// The code commit the evidence is about.
    pub artifact_commit: Option<String>,
    /// Why, for every status but `open` and `covered`.
    pub reason: Option<String>,
}

/// One declaration's standing and coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkDeclarationProjection {
    /// The work this declaration is a revision of; stable across amendments.
    pub work_id: String,
    /// This declaration's event id.
    pub declaration_ref: String,
    /// Where its plan blob is read from.
    pub plan_ref: ProjectWorkPlanRef,
    /// Where it stands.
    pub state: WorkDeclarationState,
    /// What it supersedes.
    pub supersedes: Vec<String>,
    /// What supersedes it.
    pub superseded_by: Vec<String>,
    /// Why, when the state is not plain `head`.
    pub state_reason: Option<String>,
    /// Whether the plan blob at `planRef.commit` was supplied and parsed.
    pub plan_resolved: bool,
    /// Its criteria — projected for `head` and `stale` only.
    pub criteria: Vec<WorkCriterionProjection>,
    /// Whether every criterion in the plan is covered under this declaration.
    pub coverage_complete: bool,
    /// Which clause of `coverageComplete` failed, naming criteria.
    pub coverage_reason: Option<String>,
}

/// A record the projection leaves out, with the reason it was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkExclusion {
    /// The excluded event's id.
    pub event_id: String,
    /// The stable code.
    pub code: String,
    /// One sentence a person can act on.
    pub message: String,
}

/// Two or more competing heads for one `workId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkConflict {
    /// The work that forked.
    pub work_id: String,
    /// The competing declaration ids, ascending.
    pub heads: Vec<String>,
    /// What resolving it requires.
    pub message: String,
}

/// The whole answer to "what remains".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkProjection {
    /// Always [`PROJECT_WORK_COVERAGE_SCHEMA`].
    pub schema: String,
    /// The session this projection is about.
    pub session_ref: String,
    /// The project this projection is about.
    pub project_ref: String,
    /// Every declaration, current contracts first.
    pub declarations: Vec<WorkDeclarationProjection>,
    /// Records left out, ascending by event id.
    pub excluded: Vec<WorkExclusion>,
    /// Forks that must be resolved before their work can complete.
    pub conflicts: Vec<WorkConflict>,
}

/// Abbreviate an event id or pubkey for a message: eight characters and `…`.
fn short(id: &str) -> String {
    format!("{}…", &id[..id.len().min(8)])
}

/// Abbreviate a commit for a message: twelve characters and `…`.
fn short_commit(commit: &str) -> String {
    format!("{}…", &commit[..commit.len().min(12)])
}

/// Spell a small count, because "two heads compete" reads and "2" does not.
fn count_word(n: usize) -> String {
    const WORDS: [&str; 13] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];
    WORDS
        .get(n)
        .map_or_else(|| n.to_string(), |w| (*w).to_owned())
}

/// Join ids the way a sentence does: `a`, `a and b`, `a, b and c`.
fn join_ids(ids: &[String]) -> String {
    match ids {
        [] => String::new(),
        [one] => one.clone(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// One decoded record, with the envelope facts the fold sorts and scopes by.
struct Record {
    id: String,
    pubkey: String,
    created_at: u64,
    body: ProjectWorkBody,
    session_ref: String,
    project_ref: String,
}

impl Record {
    fn key(&self) -> (u64, &str) {
        (self.created_at, self.id.as_str())
    }
}

/// Fold a session's work records into its coverage projection.
///
/// Pure and total: every input set produces one projection, and permuting the
/// events or repeating them changes nothing. A record the fold cannot use is
/// named in `excluded` with a reason; it is never a fold-wide error and it
/// never silently becomes coverage.
#[must_use]
pub fn fold_work(inputs: &WorkFoldInputs) -> WorkProjection {
    let mut excluded: Vec<WorkExclusion> = Vec::new();
    let mut records: Vec<Record> = Vec::new();
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();

    for event in &inputs.events {
        // A kind this fold does not own is not its business. Silently
        // ignoring it is what makes a mixed stream fold identically.
        if event.kind != KIND_PROJECT_WORK_RECORD {
            continue;
        }
        if !seen_ids.insert(event.id.clone()) {
            continue; // A duplicate delivery is harmless.
        }
        match validate_project_work_envelope(event) {
            Ok(payload) => records.push(Record {
                id: event.id.clone(),
                pubkey: event.pubkey.clone(),
                created_at: event.created_at,
                body: payload.body,
                session_ref: payload.session_ref,
                project_ref: payload.project_ref,
            }),
            Err(refusal) => excluded.push(WorkExclusion {
                event_id: event.id.clone(),
                code: refusal.code.as_str().to_owned(),
                message: refusal.message,
            }),
        }
    }
    records.sort_by(|a, b| a.key().cmp(&b.key()));

    let session_ref = inputs.session_ref.clone().unwrap_or_else(|| {
        records
            .first()
            .map(|record| record.session_ref.clone())
            .unwrap_or_default()
    });
    let project_ref = inputs.project_ref.clone().unwrap_or_else(|| {
        records
            .first()
            .map(|record| record.project_ref.clone())
            .unwrap_or_default()
    });

    let may_lead: BTreeSet<&str> = inputs.may_lead.iter().map(String::as_str).collect();
    let mut in_scope: Vec<Record> = Vec::with_capacity(records.len());
    for record in records {
        if record.session_ref != session_ref {
            excluded.push(WorkExclusion {
                event_id: record.id.clone(),
                code: "session-scope".to_owned(),
                message: format!("{} names another session", short(&record.id)),
            });
            continue;
        }
        if record.project_ref != project_ref {
            excluded.push(WorkExclusion {
                event_id: record.id.clone(),
                code: "project-scope".to_owned(),
                message: format!("{} names another project", short(&record.id)),
            });
            continue;
        }
        if !may_lead.contains(record.pubkey.as_str()) {
            excluded.push(WorkExclusion {
                event_id: record.id.clone(),
                code: "signer-not-may-lead".to_owned(),
                message: format!(
                    "{} does not hold may_lead in this project and session",
                    short(&record.pubkey)
                ),
            });
            continue;
        }
        in_scope.push(record);
    }

    let declarations: Vec<(&Record, &ProjectWorkDeclared)> = in_scope
        .iter()
        .filter_map(|record| match &record.body {
            ProjectWorkBody::Declared(body) => Some((record, body)),
            _ => None,
        })
        .collect();
    let declaration_ids: BTreeSet<&str> = declarations
        .iter()
        .map(|(record, _)| record.id.as_str())
        .collect();

    let mut assignments: Vec<(&Record, &ProjectWorkAssignmentBound)> = Vec::new();
    let mut evidence: Vec<(&Record, &ProjectWorkEvidenceBound)> = Vec::new();
    for record in &in_scope {
        let declaration_ref = match &record.body {
            ProjectWorkBody::Declared(_) => continue,
            ProjectWorkBody::AssignmentBound(body) => &body.declaration_ref,
            ProjectWorkBody::EvidenceBound(body) => &body.declaration_ref,
        };
        if !declaration_ids.contains(declaration_ref.as_str()) {
            excluded.push(WorkExclusion {
                event_id: record.id.clone(),
                code: "unknown-declaration".to_owned(),
                message: format!(
                    "binds to declaration {}, which is not in the supplied set",
                    short(declaration_ref)
                ),
            });
            continue;
        }
        match &record.body {
            ProjectWorkBody::AssignmentBound(body) => assignments.push((record, body)),
            ProjectWorkBody::EvidenceBound(body) => evidence.push((record, body)),
            ProjectWorkBody::Declared(_) => unreachable!("declarations were filtered above"),
        }
    }

    let states = resolve_states(&declarations, inputs.current_goal_ref.as_deref());
    let mut conflicts: Vec<WorkConflict> = Vec::new();
    for (work_id, heads) in conflicting_heads(&declarations, &states) {
        conflicts.push(WorkConflict {
            work_id,
            message: conflict_message(&declarations, &heads),
            heads,
        });
    }
    conflicts.sort_by(|a, b| (&a.work_id, &a.heads).cmp(&(&b.work_id, &b.heads)));

    let mut projected: Vec<WorkDeclarationProjection> = declarations
        .iter()
        .map(|(record, body)| {
            project_declaration(
                record,
                body,
                &states,
                &declarations,
                &assignments,
                &evidence,
                inputs,
            )
        })
        .collect();
    projected.sort_by(|a, b| {
        (&a.work_id, a.state.rank(), &a.declaration_ref).cmp(&(
            &b.work_id,
            b.state.rank(),
            &b.declaration_ref,
        ))
    });

    excluded.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    WorkProjection {
        schema: PROJECT_WORK_COVERAGE_SCHEMA.to_owned(),
        session_ref,
        project_ref,
        declarations: projected,
        excluded,
        conflicts,
    }
}

type Declarations<'a> = [(&'a Record, &'a ProjectWorkDeclared)];

/// Which declarations supersede which, and what state each is therefore in.
fn resolve_states(
    declarations: &Declarations<'_>,
    current_goal_ref: Option<&str>,
) -> BTreeMap<String, (WorkDeclarationState, Vec<String>)> {
    let mut superseded_by: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (record, body) in declarations {
        for target in &body.supersedes {
            // Only a sibling revision of the *same* work can supersede: a
            // declaration naming an id outside this set is simply a pointer
            // the fold cannot place, and the target keeps its own state.
            if declarations
                .iter()
                .any(|(other, _)| other.id == *target && other.id != record.id)
            {
                superseded_by
                    .entry(target.as_str())
                    .or_default()
                    .push(record.id.clone());
            }
        }
    }
    for successors in superseded_by.values_mut() {
        successors.sort();
        successors.dedup();
    }

    let mut heads_per_work: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (record, body) in declarations {
        if !superseded_by.contains_key(record.id.as_str()) {
            heads_per_work
                .entry(body.work_id.as_str())
                .or_default()
                .push(record.id.as_str());
        }
    }

    let mut states = BTreeMap::new();
    for (record, body) in declarations {
        let successors = superseded_by
            .get(record.id.as_str())
            .cloned()
            .unwrap_or_default();
        let state = if successors.is_empty() {
            let competing = heads_per_work
                .get(body.work_id.as_str())
                .map_or(1, Vec::len);
            if competing > 1 {
                WorkDeclarationState::Conflict
            } else if current_goal_ref.is_some_and(|goal| goal != body.goal_ref) {
                WorkDeclarationState::Stale
            } else {
                WorkDeclarationState::Head
            }
        } else {
            // Structural states take precedence over `stale`: a superseded
            // declaration adopted against an older goal reads `superseded`.
            WorkDeclarationState::Superseded
        };
        states.insert(record.id.clone(), (state, successors));
    }
    states
}

/// The competing heads, per `workId`, that make a conflict.
fn conflicting_heads(
    declarations: &Declarations<'_>,
    states: &BTreeMap<String, (WorkDeclarationState, Vec<String>)>,
) -> Vec<(String, Vec<String>)> {
    let mut per_work: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (record, body) in declarations {
        if states
            .get(&record.id)
            .is_some_and(|(state, _)| *state == WorkDeclarationState::Conflict)
        {
            per_work
                .entry(body.work_id.clone())
                .or_default()
                .push(record.id.clone());
        }
    }
    per_work
        .into_iter()
        .map(|(work_id, mut heads)| {
            heads.sort();
            (work_id, heads)
        })
        .collect()
}

/// The one sentence a fork gets, in the conflict entry and in every head's
/// `stateReason`, so a reader never has to correlate two phrasings.
fn conflict_message(declarations: &Declarations<'_>, heads: &[String]) -> String {
    let predecessors: Vec<&String> = heads
        .iter()
        .filter_map(|head| {
            declarations
                .iter()
                .find(|(record, _)| record.id == *head)
                .map(|(_, body)| &body.supersedes)
        })
        .fold(None::<Vec<&String>>, |accumulated, supersedes| {
            Some(match accumulated {
                None => supersedes.iter().collect(),
                Some(common) => common
                    .into_iter()
                    .filter(|id| supersedes.contains(*id))
                    .collect(),
            })
        })
        .unwrap_or_default();
    let naming = if heads.len() == 2 {
        "naming both heads"
    } else {
        "naming all heads"
    };
    match predecessors.as_slice() {
        [one] => format!(
            "{} unsuperseded successors of {}; an authorized declaration {naming} is required before this work can complete",
            count_word(heads.len()),
            short(one)
        ),
        _ => format!(
            "{} unsuperseded declarations for this work; an authorized declaration {naming} is required before this work can complete",
            count_word(heads.len())
        ),
    }
}

// The per-declaration projection lives in a sibling file so no file here
// passes 1,000 lines. A child module, so it reads this module's types and
// private helpers unchanged.
#[path = "project_work_fold_project.rs"]
mod project;
use project::project_declaration;

#[cfg(test)]
#[path = "project_work_fold_tests.rs"]
mod tests;
