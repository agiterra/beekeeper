//! The verified facts the coverage fold is handed, and the three proof
//! predicates it judges them with.
//!
//! A child of `project_work_fold`. Nothing here reads an event: the caller
//! established these facts from kind:44244 reports and verdicts, kind:46023
//! host results carried by the relay's kind:46014 echo, and the agents
//! repository's `actions.yml` compiled at the declaration's plan commit. The
//! fold compares; it never verifies a signature it was not told about and
//! never compiles an action definition itself — an evidence record that
//! nominated its own expected hash would prove nothing.
//!
//! **A signed binding is a claim to verify.** A criterion is `covered` only
//! when its bound evidence satisfies the predicate for its `proof` form.
//! Evidence that resolved and failed leaves the criterion `open` **with its
//! reason named**: the binding exists, and saying so is the difference
//! between "nobody has done this" and "somebody claimed it and the claim did
//! not hold".
//!
//! The normative contract is `conformance/project-work/README.md` § (c)
//! "The input", "The three proof predicates" and "Reason codes".

use serde::{Deserialize, Serialize};

use crate::coding_session_team_transaction::{
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat, CodingSessionTeamFoldContext,
};

/// The 44244 authority projection, in that fold's own context shape.
///
/// One predicate serves both folds: [`WorkAuthority::may_lead`] delegates to
/// [`CodingSessionTeamFoldContext::may_lead`] rather than re-enumerating
/// founder / active `lead` seat / active `may_steer` grantee. A second copy
/// of that rule is a copy that drifts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkAuthority {
    /// Pubkey of the session genesis signer.
    pub founder_pubkey: String,
    /// The current signed seat projection.
    pub active_seats: Vec<WorkActiveSeat>,
    /// The current signed grant projection.
    pub active_grants: Vec<WorkActiveGrant>,
}

/// One currently active role seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkActiveSeat {
    /// Canonical lowercase-hex actor pubkey.
    pub actor_pubkey: String,
    /// Canonical role slug; only an exact `lead` affects this predicate.
    pub role: String,
}

/// One active, signed authority grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkActiveGrant {
    /// Canonical lowercase-hex grantee pubkey.
    pub actor_pubkey: String,
    /// Event id of the active signed grant this projection came from.
    pub grant_event_ref: String,
    /// Whether the grant gives the operator steering standing.
    pub may_steer: bool,
}

/// Exact refusal for a signer the authority projection does not admit.
pub const SIGNER_NOT_MAY_LEAD_MESSAGE: &str =
    "is neither the founder, nor an active lead seat, nor the holder of an active may_steer grant";

impl WorkAuthority {
    /// Whether `actor` may act for this mission.
    ///
    /// Delegates to the 44244 fold's own `may_lead`, built from this
    /// projection. The session, channel and genesis the context also carries
    /// play no part in that predicate, so placeholder-free values are not
    /// needed here — but the caller has already scoped this authority to one
    /// project and session, which is what makes the answer meaningful.
    #[must_use]
    pub fn may_lead(&self, actor: &str) -> bool {
        self.as_team_context().may_lead(actor)
    }

    fn as_team_context(&self) -> CodingSessionTeamFoldContext {
        CodingSessionTeamFoldContext {
            channel_ref: String::new(),
            session_ref: String::new(),
            genesis_ref: String::new(),
            founder_pubkey: self.founder_pubkey.clone(),
            active_seats: self
                .active_seats
                .iter()
                .map(|seat| CodingSessionTeamActiveSeat {
                    actor_pubkey: seat.actor_pubkey.clone(),
                    role: seat.role.clone(),
                })
                .collect(),
            active_grants: self
                .active_grants
                .iter()
                .map(|grant| CodingSessionTeamActiveGrant {
                    actor_pubkey: grant.actor_pubkey.clone(),
                    grant_event_ref: grant.grant_event_ref.clone(),
                    may_steer: grant.may_steer,
                })
                .collect(),
            verifier_required: false,
        }
    }
}

/// The canonical kind:44244 projection this fold judges evidence against.
///
/// **Work coverage never admits what the team contract excludes** (A5
/// decision 23, review finding 6). The 44244 fold already requires a report's
/// signer to be its assignment's assignee and drops a ruling that a later one
/// replaced; relay ingest validates structure and cannot see either. So the
/// caller folds the session's team transactions with
/// `fold_coding_session_team_transactions` — the existing fold, never a second
/// implementation — and hands its answer here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTeamProjection {
    /// Every record that projection **includes**: not excluded, not
    /// superseded, not corrected away.
    pub included_event_ids: std::collections::BTreeSet<String>,
    /// Its projected assignments, by event id.
    pub assignments: std::collections::BTreeMap<String, WorkTeamAssignment>,
}

impl WorkTeamProjection {
    /// Whether the canonical projection includes this record.
    #[must_use]
    pub fn includes(&self, event_id: &str) -> bool {
        self.included_event_ids.contains(event_id)
    }

    /// The actor an assignment names, when the projection carries it.
    #[must_use]
    pub fn assignee(&self, assignment_ref: &str) -> Option<&str> {
        self.assignments
            .get(assignment_ref)
            .map(|assignment| assignment.assignee_actor.as_str())
    }
}

/// One projected assignment: who owes it, under which role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTeamAssignment {
    /// Canonical lowercase-hex actor the assignment names.
    pub assignee_actor: String,
    /// The role it names, as recorded.
    pub assignee_role: String,
}

/// One action definition the **caller** compiled from `actions.yml` at the
/// declaration's plan commit, with the publication compiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkActionDefinition {
    /// The compiled definition's hash, compared against a host result's.
    pub definition_hash: String,
    /// The step ids the definition declares.
    pub steps: Vec<String>,
}

/// Where a host result's command ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkCheckoutFact {
    /// How the checkout was established, verbatim from the result.
    pub mode: String,
    /// The commit the command ran on.
    pub sha: String,
    /// Whether the tree was already dirty before the command.
    pub dirty_before: bool,
}

/// One verified fact about a referenced evidence event.
///
/// Tagged by `kind` exactly as the fixture files write it, so the conformance
/// input deserializes into this type directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum WorkEvidenceFact {
    /// A kind:44244 `report`.
    Report {
        /// The report's event id.
        event_id: String,
        /// Who signed it.
        signer: String,
        /// The assignment it answers.
        assignment_ref: String,
        /// The revision it is about.
        head_sha: String,
    },
    /// A kind:44244 `verdict`.
    Verdict {
        /// The verdict's event id.
        event_id: String,
        /// Who signed it.
        signer: String,
        /// `refutation` or `disposition`.
        subtype: String,
        /// The closed decision token.
        decision: String,
        /// The assignment under review.
        assignment_ref: String,
        /// The report under review.
        report_ref: String,
    },
    /// A kind:46023 host result carried by the relay's kind:46014 echo.
    ActionResult {
        /// The 46023 event id.
        event_id: String,
        /// The 46014 echo's event id.
        exited_event_id: String,
        /// The host key that signed the result.
        result_signer: String,
        /// The key that signed the echo; must be the relay's `self` key.
        echo_signer: String,
        /// Which action ran.
        action_name: String,
        /// Which run it belonged to.
        run_id: String,
        /// Which step of the action ran.
        step_id: String,
        /// The hash of the definition that ran.
        definition_hash: String,
        /// How the run ended.
        disposition: String,
        /// The command's exit code.
        exit_code: i64,
        /// Where it ran.
        checkout: WorkCheckoutFact,
        /// Whether the tree was dirty after the command.
        dirty: bool,
    },
}

impl WorkEvidenceFact {
    /// The event id this fact is about.
    #[must_use]
    pub fn event_id(&self) -> &str {
        match self {
            Self::Report { event_id, .. }
            | Self::Verdict { event_id, .. }
            | Self::ActionResult { event_id, .. } => event_id,
        }
    }
}

/// Why a criterion is not `covered`, as a stable code the contract fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkReasonCode {
    /// A bound evidence id is in neither `evidence` nor `refStates`.
    EvidenceUnavailable,
    /// The plan blob at `planRef.commit` was not supplied.
    PlanUnreadable,
    /// The verdict's signer does not satisfy `may_lead`.
    WrongSigner,
    /// The disposition is not an approval.
    NotApproving,
    /// The report is about another revision.
    RevisionMismatch,
    /// The run executed another definition, or another step.
    WrongRunOrHash,
    /// The run exited non-zero.
    ActionFailed,
    /// The tree was dirty before or after the command.
    DirtyRevision,
    /// The team projection excludes the report, it was signed by somebody who
    /// is not the assignment's assignee, or it answers an assignment this
    /// criterion is not bound to.
    ReportNotCanonical,
    /// The team projection excludes the disposition — most often because a
    /// later ruling replaced it.
    DispositionNotCanonical,
    /// The late-green-for-P case.
    BoundToSupersededDeclaration,
    /// A newer ref state names another commit.
    RefObservationSuperseded,
}

impl WorkReasonCode {
    /// The stable string form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EvidenceUnavailable => "evidence_unavailable",
            Self::PlanUnreadable => "plan_unreadable",
            Self::WrongSigner => "wrong_signer",
            Self::NotApproving => "not_approving",
            Self::RevisionMismatch => "revision_mismatch",
            Self::WrongRunOrHash => "wrong_run_or_hash",
            Self::ActionFailed => "action_failed",
            Self::DirtyRevision => "dirty_revision",
            Self::ReportNotCanonical => "report_not_canonical",
            Self::DispositionNotCanonical => "disposition_not_canonical",
            Self::BoundToSupersededDeclaration => "bound_to_superseded_declaration",
            Self::RefObservationSuperseded => "ref_observation_superseded",
        }
    }
}

/// Why a declaration is not in plain `head` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStateReasonCode {
    /// Another declaration of this work supersedes it.
    Superseded,
    /// More than one maximal declaration exists for this work.
    Conflict,
    /// The session's current goal is not the one it was adopted against.
    GoalChanged,
}

impl WorkStateReasonCode {
    /// The stable string form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Superseded => "superseded",
            Self::Conflict => "conflict",
            Self::GoalChanged => "goal_changed",
        }
    }
}

/// Which clause of `coverageComplete` failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkCoverageReasonCode {
    /// At least one criterion is not `covered`.
    CriteriaNotCovered,
    /// Every criterion is covered, at more than one artifact commit.
    MixedArtifacts,
    /// Coverage is computed for the head declaration only.
    Superseded,
    /// Coverage is not computed while heads compete.
    Conflict,
    /// The plan blob was not supplied, so no criterion could be evaluated.
    ///
    /// A declaration-level answer on purpose: with no plan **and** no
    /// bindings there are no criterion rows at all, and an empty list must
    /// never read as "nothing outstanding" (A5 decision 25, finding 5).
    PlanUnavailable,
}

impl WorkCoverageReasonCode {
    /// The stable string form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CriteriaNotCovered => "criteria_not_covered",
            Self::MixedArtifacts => "mixed_artifacts",
            Self::Superseded => "superseded",
            Self::Conflict => "conflict",
            Self::PlanUnavailable => "plan_unavailable",
        }
    }
}
