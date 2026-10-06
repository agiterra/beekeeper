//! NIP-PW: which `work.evidence_bound` records follow mechanically from facts
//! the relay has already signed.
//!
//! # Why this exists
//!
//! Control run 5 (ledger 257(d)) spent 190 s between a green host verify and
//! `mission.completed`, most of it a lead turn doing clerical work: read the
//! kind:46023, run `bind evidence` for the action criterion, run `bind ref`
//! for the git-ref criterion, with four CLI usage errors on the way. Every one
//! of those binds is a pure function of three relay-signed facts:
//!
//! 1. the relay's kind:46014 echo of a host result (exit, dirty, `headSha`);
//! 2. the newest **relay-signed** kind:30618 for the plan's repository, which
//!    names the delivery ref's commit;
//! 3. the adopted plan, read at the declaration's pinned commit.
//!
//! So the host that ran the step can bind them itself. This module is the
//! decision, with no I/O: the provider gathers the facts and signs, and
//! `bee sessions work bind ref` uses the same ref-observation lookup.
//!
//! # What it binds, and what it refuses
//!
//! It binds only when **all** of these hold, and otherwise binds nothing:
//!
//! - the result exited `0`, the checkout was not dirty, and it names a
//!   `headSha`;
//! - the plan has at least one `{kind: action}` criterion whose action and
//!   step are the ones this run executed;
//! - the relay's own newest ref state names the plan's `delivery_ref` at
//!   exactly that `headSha` — a green run of an undelivered commit proves
//!   nothing about what landed.
//!
//! Then it produces the same bodies the CLI would: one `action_result`
//! binding for the matching action criteria and, when the plan has any, one
//! `ref_observation` binding for its `git-ref` criteria. A binding already on
//! the wire for the same declaration, commit, criteria and evidence kind —
//! whoever signed it — is reported as existing, never republished.
//!
//! No new record type and no new field: the records are exactly the records
//! the CLI publishes today (ruling of 2026-09-22, "no new record types without
//! need"), and the fold judges them by the same rules whoever signed them.

use crate::kind::{KIND_GIT_REPO_STATE, KIND_PROJECT_WORK_RECORD};
use crate::project_plan::{Plan, PlanProof};
use crate::project_work::{
    decode_project_work_content, ProjectWorkBody, ProjectWorkDeclared, ProjectWorkEvent,
    ProjectWorkEvidenceBound, ProjectWorkEvidenceKind, ProjectWorkEvidenceRef,
};

/// The relay's own observation of one delivery ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefObservation {
    /// The kind:30618 event id — the `ref_observation` evidence pointer.
    pub event_id: String,
    /// The commit it names at the delivery ref, lowercase.
    pub commit: String,
}

/// Why no relay observation of the delivery ref was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefObservationMissing {
    /// The relay has signed no ref state for the repository.
    NoState,
    /// The newest relay-signed ref state does not name the delivery ref.
    NoDeliveryRef {
        /// That newest ref state's event id.
        state_id: String,
    },
}

/// The newest relay-signed kind:30618 for `repository`, and the commit it
/// names at `delivery_ref`.
///
/// Rows are re-checked here whatever filter produced them: an owner-signed
/// kind:30618 is a claim about one's own branch, never an observation.
///
/// # Errors
/// [`RefObservationMissing`] when nothing the relay signed names the ref.
pub fn newest_ref_observation(
    states: &[ProjectWorkEvent],
    relay_self: &str,
    repository: &str,
    delivery_ref: &str,
) -> Result<RefObservation, RefObservationMissing> {
    let newest = states
        .iter()
        .filter(|state| {
            state.kind == KIND_GIT_REPO_STATE
                && state.pubkey.eq_ignore_ascii_case(relay_self)
                && state.tag_value("d") == Some(repository)
        })
        .max_by(|a, b| a.order_key().cmp(&b.order_key()))
        .ok_or(RefObservationMissing::NoState)?;
    let commit =
        newest
            .tag_value(delivery_ref)
            .ok_or_else(|| RefObservationMissing::NoDeliveryRef {
                state_id: newest.id.clone(),
            })?;
    Ok(RefObservation {
        event_id: newest.id.clone(),
        commit: commit.to_ascii_lowercase(),
    })
}

/// The facts of one relay-echoed host result this module reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostResultFacts {
    /// The kind:46023 event id — the `action_result` evidence pointer.
    pub result_event_id: String,
    /// The action (workflow) name the run executed.
    pub action_name: String,
    /// The step the result is for.
    pub step_id: String,
    /// Whether the disposition is `exited` (not refused, timed out or lost).
    pub exited: bool,
    /// The step's exit code, when it had one.
    pub exit_code: Option<i32>,
    /// Whether the checkout was dirty; absent means not recorded.
    pub dirty: Option<bool>,
    /// The commit the step ran against, sampled after it ran.
    pub head_sha: Option<String>,
}

/// Why a host result produced no binding. Each is a sentence for the log and
/// the wake, never a failure of the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoBindSkip {
    /// The result is not a clean green run at a named commit.
    NotGreen(String),
    /// No active plan criterion is proved by this action and step.
    NoActionCriterion {
        /// The action the run executed.
        action: String,
        /// The step the result is for.
        step: String,
    },
    /// The relay's ref state does not name the delivery ref at this commit.
    NotDelivered {
        /// The commit the result ran at.
        head_sha: String,
        /// The commit the relay's newest ref state names there.
        delivered: String,
        /// That ref state's event id.
        observation: String,
    },
}

impl std::fmt::Display for AutoBindSkip {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotGreen(why) => write!(formatter, "not a green result: {why}"),
            Self::NoActionCriterion { action, step } => write!(
                formatter,
                "no active plan criterion is proved by action {action:?} step {step:?}"
            ),
            Self::NotDelivered {
                head_sha,
                delivered,
                observation,
            } => write!(
                formatter,
                "the result ran at {}, but the relay's newest ref state ({}) names the delivery \
                 ref at {}",
                short(head_sha),
                short(observation),
                short(delivered)
            ),
        }
    }
}

fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

/// The commit a clean green result ran at, lowercase.
///
/// # Errors
/// [`AutoBindSkip::NotGreen`] naming the first fact that fails.
pub fn green_head(result: &HostResultFacts) -> Result<String, AutoBindSkip> {
    if !result.exited {
        return Err(AutoBindSkip::NotGreen("the step did not exit".into()));
    }
    if result.exit_code != Some(0) {
        return Err(AutoBindSkip::NotGreen(match result.exit_code {
            Some(code) => format!("exit code {code}"),
            None => "no exit code".into(),
        }));
    }
    if result.dirty != Some(false) {
        return Err(AutoBindSkip::NotGreen(match result.dirty {
            Some(_) => "the checkout was dirty".into(),
            None => "the result does not say whether the checkout was clean".into(),
        }));
    }
    let Some(head) = result.head_sha.as_deref() else {
        return Err(AutoBindSkip::NotGreen("the result names no headSha".into()));
    };
    let head = head.to_ascii_lowercase();
    if !(head.len() == 40 || head.len() == 64) || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AutoBindSkip::NotGreen(format!(
            "headSha {head:?} is not a full commit"
        )));
    }
    Ok(head)
}

/// The session's one current `work.declared`: the declaration no other
/// declaration in the set supersedes.
///
/// # Errors
/// A sentence when there is none, or more than one — an ambiguous contract is
/// left to the lead, never guessed at.
pub fn current_declaration(
    records: &[ProjectWorkEvent],
) -> Result<(String, ProjectWorkDeclared), String> {
    let declared: Vec<(String, ProjectWorkDeclared)> = records
        .iter()
        .filter(|record| record.kind == KIND_PROJECT_WORK_RECORD)
        .filter_map(|record| {
            let payload = decode_project_work_content(&record.content).ok()?;
            match payload.body {
                ProjectWorkBody::Declared(body) => Some((record.id.clone(), body)),
                _ => None,
            }
        })
        .collect();
    let mut heads = declared.iter().filter(|(id, _)| {
        !declared
            .iter()
            .any(|(_, other)| other.supersedes.iter().any(|old| old == id))
    });
    match (heads.next(), heads.next()) {
        (Some(head), None) => Ok(head.clone()),
        (None, _) => Err("this session has adopted no plan".into()),
        (Some(_), Some(_)) => Err("this session has more than one current work declaration".into()),
    }
}

/// One binding this result supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoBinding {
    /// The body to publish, exactly as the CLI would build it.
    pub body: ProjectWorkEvidenceBound,
    /// A record already on the wire that says the same thing, if any.
    pub existing: Option<String>,
}

/// Every binding a clean green result at the delivered commit supports.
///
/// # Errors
/// [`AutoBindSkip`] — nothing is bound, and the reason says why.
pub fn auto_bindings(
    plan: &Plan,
    declaration_ref: &str,
    records: &[ProjectWorkEvent],
    result: &HostResultFacts,
    observation: &RefObservation,
) -> Result<Vec<AutoBinding>, AutoBindSkip> {
    let head = green_head(result)?;
    let action_criteria = action_criteria(plan, &result.action_name, &result.step_id);
    if action_criteria.is_empty() {
        return Err(AutoBindSkip::NoActionCriterion {
            action: result.action_name.clone(),
            step: result.step_id.clone(),
        });
    }
    if observation.commit != head {
        return Err(AutoBindSkip::NotDelivered {
            head_sha: head,
            delivered: observation.commit.clone(),
            observation: observation.event_id.clone(),
        });
    }
    let mut bodies = vec![ProjectWorkEvidenceBound {
        declaration_ref: declaration_ref.to_owned(),
        criterion_ids: action_criteria,
        artifact_commit: head.clone(),
        evidence_refs: vec![ProjectWorkEvidenceRef {
            kind: ProjectWorkEvidenceKind::ActionResult,
            event_id: result.result_event_id.to_ascii_lowercase(),
        }],
        completion_ref: None,
    }];
    let git_ref_criteria: Vec<String> = plan
        .criteria
        .iter()
        .filter(|criterion| matches!(criterion.proof, PlanProof::GitRef))
        .map(|criterion| criterion.id.clone())
        .collect();
    if !git_ref_criteria.is_empty() {
        bodies.push(ProjectWorkEvidenceBound {
            declaration_ref: declaration_ref.to_owned(),
            criterion_ids: git_ref_criteria,
            artifact_commit: head,
            evidence_refs: vec![ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::RefObservation,
                event_id: observation.event_id.clone(),
            }],
            completion_ref: None,
        });
    }
    Ok(bodies
        .into_iter()
        .map(|body| AutoBinding {
            existing: already_bound(records, &body),
            body,
        })
        .collect())
}

/// Active criteria proved by exactly this action and step.
#[must_use]
pub fn action_criteria(plan: &Plan, action: &str, step: &str) -> Vec<String> {
    plan.criteria
        .iter()
        .filter(|criterion| {
            matches!(&criterion.proof, PlanProof::Action { name, step: proof_step }
                if name == action && proof_step == step)
        })
        .map(|criterion| criterion.id.clone())
        .collect()
}

/// A binding already on the wire that says what `body` says: the same
/// declaration and commit, every one of its criteria, and evidence of the
/// same kind. The same predicate `bind ref` applies before it signs.
#[must_use]
pub fn already_bound(
    records: &[ProjectWorkEvent],
    body: &ProjectWorkEvidenceBound,
) -> Option<String> {
    let kinds: Vec<ProjectWorkEvidenceKind> = body
        .evidence_refs
        .iter()
        .map(|reference| reference.kind)
        .collect();
    records.iter().find_map(|record| {
        let payload = decode_project_work_content(&record.content).ok()?;
        let ProjectWorkBody::EvidenceBound(existing) = payload.body else {
            return None;
        };
        (existing.declaration_ref == body.declaration_ref
            && existing.artifact_commit == body.artifact_commit
            && body
                .criterion_ids
                .iter()
                .all(|id| existing.criterion_ids.contains(id))
            && kinds.iter().all(|kind| {
                existing
                    .evidence_refs
                    .iter()
                    .any(|reference| reference.kind == *kind)
            }))
        .then(|| record.id.clone())
    })
}

#[cfg(test)]
#[path = "project_work_autobind_tests.rs"]
mod tests;
