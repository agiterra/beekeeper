//! The exact sentences a refused `git push` prints, in one file.
//!
//! Split out of [`super`] on 2026-09-03 (L27) when the rule file passed the
//! repository's 1,000-line ceiling. The split is along the seam the module
//! already had: everything here is **frozen copy** that reaches a person
//! through `git push`'s own stderr, and nothing here decides anything.
//!
//! Two properties every sentence holds, asserted by
//! [`super::arms_tests`] and its siblings:
//!
//! * it never repeats the ref name — the renderer prefixes `{ref}: `;
//! * it never invents a remedy the rule cannot deliver.

use super::{
    VerdictAdmissionRefusal, VERDICT_ADMISSION_MAX_SESSIONS, VERDICT_ADMISSION_MAX_TRANSACTIONS,
};

impl VerdictAdmissionRefusal {
    /// The exact sentence a pusher sees, without the ref name.
    ///
    /// Provisional copy (batch 3 §1j): frozen in code so both callers say the
    /// same thing, pending Brian's word on the wording itself.
    pub fn reason(&self) -> String {
        match self {
            Self::NoApprovingVerdict {
                new_oid,
                searched_sessions,
            } => format!(
                "require-verdict is set and no mission verdict names this commit: no approved \
                 report names {new_oid}. Searched {searched_sessions} mission(s) — the newest \
                 {VERDICT_ADMISSION_MAX_SESSIONS} on this channel whose founder is a founder of \
                 this repository — over one shared page of the newest \
                 {VERDICT_ADMISSION_MAX_TRANSACTIONS} team transactions on that channel. An \
                 older ruling can fall outside both. No observed gate row names {new_oid} \
                 either, so the gate-row route is not open for it: that route wants every \
                 required gate published green on this exact commit, by the mission's own \
                 provider, over a clean worktree. Run each gate as its own command so the host \
                 can record it."
            ),
            Self::ApprovedReportNamesBranchOnly => "require-verdict is set and no mission verdict \
                 names this commit: the approved report for this work names a branch and no \
                 headSha, and a branch name is not a commit."
                .to_string(),
            Self::ApprovedButNotVerified { new_oid, approvals } => format!(
                "commit {new_oid} is approved ({approvals} approving disposition(s)) and no \
                 active verifier seat has cleared the report it approves. The gate wants a \
                 `refutation` verdict of `not-refuted` on that report, signed by a verifier \
                 seat of that mission — a lead's approval is the settlement, not the check. A \
                 founder may land this commit by pushing it themselves."
            ),
            Self::VerifierIsTheReportAuthor { new_oid, verifier } => format!(
                "commit {new_oid} is cleared only by {verifier}, which is the key that wrote the \
                 report being cleared. A seat cannot stand as the verifier of its own work."
            ),
            Self::PushNotSeated {
                new_oid,
                session_ref,
                seats,
            } => format!(
                "commit {new_oid} carries a verifier's verdict on mission {session_ref}, and \
                 this key is not an active seat of it ({seats} seat(s)). A founder of this \
                 repository may land it, or a seat of that mission may."
            ),
            Self::ApprovedForAnotherRef {
                new_oid,
                approved_branch,
            } => format!(
                "commit {new_oid} is approved for {approved_branch}, and this is a different \
                 ref. A verdict admits a commit to the branch its report named."
            ),
            Self::ObservedRowsAreDeclared { new_oid, rows } => format!(
                "commit {new_oid} is named by {rows} gate row(s), and every one of them is \
                 `declared` — its own subject saying so about itself. A landing needs rows a \
                 mechanism watched: the provider publishes those under its own key while the \
                 gate runs, and `bee sessions observe gate` cannot mint one."
            ),
            Self::ObservedGateRed { gate, new_oid } => format!(
                "gate `{gate}` was observed red on {new_oid}. Fix it and run it again; the next \
                 observed row names the commit it ran at."
            ),
            Self::ObservedDirty { new_oid } => format!(
                "{new_oid} was observed dirty: the gates ran over a worktree with uncommitted \
                 changes in it, so they measured something no commit names. Commit the tree and \
                 run them again."
            ),
            Self::RequiredGateNotObserved {
                gate,
                new_oid,
                required,
            } => format!(
                "gate `{gate}` has no observed green row on {new_oid}. This mission requires {}, \
                 each observed green on the commit being pushed and over a clean worktree.",
                required
                    .iter()
                    .map(|gate| format!("`{gate}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::VerifiedButGatesNotGreen {
                new_oid,
                verifier,
                gates,
            } => format!(
                "verifier {verifier} cleared {new_oid}, and a landing also needs every required \
                 gate observed green on it: {} Both halves are required on a verifier-gated \
                 mission — the stricter policy asks for more proof than the gate-row route, \
                 never for different proof.",
                gates.reason()
            ),
            Self::RepositoryUnbound => "require-verdict is set and this repository is bound to no \
                 channel, so no mission verdict can be read here. Remove the rule, or bind the \
                 repository to the mission's channel."
                .to_string(),
        }
    }
}
