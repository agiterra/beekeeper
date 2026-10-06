//! What an admitted push stood on — the evidence half of the rule.
//!
//! Split out of [`super`] so no file here passes 1,000 lines, the same reason
//! [`super::reasons`] holds the refusals. A refusal and an admission are two
//! halves of one disclosure: the first names the rule it applied, the second
//! names the records it consulted.

use super::VerdictAdmissionPolicyEvidence;
use super::VerdictAdmissionPolicyNotEvaluated;

/// What admitted a commit — **which arm**, and the facts it stood on.
///
/// An enum rather than a struct because the two arms stand on different
/// evidence and a struct would have to carry empty strings for the half that
/// does not apply. A founder push has no disposition and no report; saying so
/// with `None`s would invite a renderer to print "approved by" over nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictAdmissionEvidence {
    /// Arm (A): a founder of the repository pushed. No mission was read.
    FounderPush {
        /// The founder's hex pubkey, as the push authenticated.
        pusher_pubkey: String,
        /// Why no policy was consulted — so a founder's landing is recorded
        /// as the exception it is and never reads as verifier-approved.
        policy_not_evaluated: VerdictAdmissionPolicyNotEvaluated,
    },
    /// Arm (B): every required gate was observed green on this commit.
    ObservedGates {
        /// Umbrella whose observations admitted it.
        session_ref: String,
        /// The commit every one of those rows named.
        head_sha: String,
        /// The gates that had to be green, in the order they were required.
        gates: Vec<String>,
        /// The newest observation event id behind each of those gates, in the
        /// same order — so a reader can go and read the rows themselves.
        row_event_ids: Vec<String>,
        /// The policy record this arm read, and how it resolved.
        policy: VerdictAdmissionPolicyEvidence,
    },
    /// Arm (C): a verifier seat cleared a report naming this commit, **and**
    /// every required gate was observed green on it.
    VerifierVerdict {
        /// Umbrella whose fold admitted it.
        session_ref: String,
        /// The approving disposition that settled the assignment.
        disposition_event_id: String,
        /// The verifier's `not-refuted` refutation of the same report.
        refutation_event_id: String,
        /// The report both records govern.
        report_event_id: String,
        /// That report's `headSha`, as published.
        head_sha: String,
        /// The verifier seat that signed the refutation.
        verifier_pubkey: String,
        /// The gates that also had to be green on this commit, in the order
        /// they were required.
        ///
        /// Present since the 2026-09-03 follow-up ruling, and never empty: a
        /// surface that says "a verifier cleared it" over an arm that also
        /// checked three gates is telling half the truth, and this project
        /// treats a comfortable half-truth as a defect.
        gates: Vec<String>,
        /// The newest observation event id behind each of those gates, in the
        /// same order — so a reader can go and read the rows themselves.
        row_event_ids: Vec<String>,
        /// The policy record whose gate list this arm read, and how it
        /// resolved.
        policy: VerdictAdmissionPolicyEvidence,
    },
}
