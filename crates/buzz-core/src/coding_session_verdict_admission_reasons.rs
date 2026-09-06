//! The exact sentences a refused `git push` prints, in one file.
//!
//! Split out of [`super`] on 2026-09-03 (L27) when the rule file passed the
//! repository's 1,000-line ceiling, and given the refusal enum itself on
//! 2026-09-05 (findings 89–91) when it passed it again. The split is along
//! the seam the module already had: everything here is **frozen copy** that
//! reaches a person through `git push`'s own stderr, plus the type that names
//! which sentence — and nothing here decides anything.
//!
//! Two properties every sentence holds, asserted by
//! [`super::arms_tests`] and its siblings:
//!
//! * it never repeats the ref name — the renderer prefixes `{ref}: `;
//! * it never invents a remedy the rule cannot deliver.

// The mission/transaction caps this file used to name itself now reach the
// sentence through `VerdictAdmissionCandidateSource::searched_clause`, which
// words them per lookup (L28).
use super::{
    VerdictAdmissionCandidateSource, VerdictAdmissionGateRows,
    VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS,
};

/// Why a verdict-gated update is refused.
///
/// The strings [`VerdictAdmissionRefusal::reason`] returns are frozen copy:
/// they reach a person through `git push`'s own stderr, so they never repeat
/// the ref (the renderer prefixes it) and never invent a remedy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictAdmissionRefusal {
    /// Nothing canonical names this commit.
    NoApprovingVerdict {
        /// The object id that was searched for.
        new_oid: String,
        /// How many missions were searched — the bound, disclosed.
        searched_sessions: usize,
        /// Which lookup found them (finding 56), so the count means something.
        source: VerdictAdmissionCandidateSource,
    },
    /// An approved report exists for this work but names only a branch.
    ApprovedReportNamesBranchOnly,
    /// Someone approved a report naming this commit, but nobody who could
    /// stand as its verifier did.
    ///
    /// Since finding 75 (live run 6, 2026-09-04) this carries **arm (B)'s
    /// status on the same commit** and whether the policy closes that arm,
    /// so the sentence can lead with the route the mission is actually
    /// running: a mission whose founder never asked for a verifier is owed
    /// "which gate row is missing", not "find a verifier".
    ApprovedButNotVerified {
        /// The approved object id.
        new_oid: String,
        /// How many approving dispositions named it — disclosed so a reader
        /// can tell "nobody ruled" from "the wrong people ruled".
        approvals: usize,
        /// Whether the mission's newest founder-signed kind 44245 policy sets
        /// `gates.verifierRequired`. `false` covers both "set to false" and
        /// "no policy read" — the two are one fact to arm (B), which is open
        /// in either case.
        verifier_required: bool,
        /// What arm (B) found on this commit, seat aside.
        rows: VerdictAdmissionGateRows,
        /// Whether the pusher holds an active seat of the mission. Both
        /// routes require one, and a pusher told to run the gates without
        /// being told this would run them for nothing.
        seated: bool,
    },
    /// The only approving verifier is the key that wrote the report.
    VerifierIsTheReportAuthor {
        /// The approved object id.
        new_oid: String,
        /// The key holding both the `verifier` seat and the report.
        verifier: String,
    },
    /// A verifier cleared this commit and the pusher is not of that mission.
    PushNotSeated {
        /// The verified object id.
        new_oid: String,
        /// The umbrella whose verifier cleared it.
        session_ref: String,
        /// How many active seats that mission has — disclosed so a seat whose
        /// grant was revoked can tell that from "this mission seats nobody".
        seats: usize,
    },
    /// The commit is approved, but for a different branch than this ref.
    ApprovedForAnotherRef {
        /// The approved object id.
        new_oid: String,
        /// The branch the approved report named.
        approved_branch: String,
    },
    /// Rows name this commit, and every one of them is its subject's own
    /// claim.
    ObservedRowsAreDeclared {
        /// The pushed object id.
        new_oid: String,
        /// How many rows named it — disclosed so a reader can tell "one seat
        /// said so" from "nobody said anything".
        rows: usize,
    },
    /// A gate was observed red on this very commit.
    ObservedGateRed {
        /// The gate's own name, as the row carries it.
        gate: String,
        /// The pushed object id.
        new_oid: String,
    },
    /// The gates were observed over a worktree the commit does not name.
    ObservedDirty {
        /// The pushed object id.
        new_oid: String,
    },
    /// A required gate has no observed green row on this commit.
    RequiredGateNotObserved {
        /// The first required gate with no such row.
        gate: String,
        /// The pushed object id.
        new_oid: String,
        /// Every gate this mission requires, so the refusal names the whole
        /// list rather than one item of it.
        required: Vec<String>,
    },
    /// A verifier cleared this commit and its gates were not observed green
    /// on it.
    ///
    /// The refusal that carries the 2026-09-03 follow-up ruling. It names
    /// **both** halves — the one that is satisfied and the one that is not —
    /// because a pusher told only "the gates are not green" would go looking
    /// for a verifier they already have, and one told only "cleared" would not
    /// understand why the push stopped.
    VerifiedButGatesNotGreen {
        /// The cleared object id.
        new_oid: String,
        /// The verifier seat that cleared it.
        verifier: String,
        /// Arm (B)'s own sentence about the rows, boxed because a refusal that
        /// contains a refusal is otherwise infinitely sized. Carried whole
        /// rather than paraphrased: the words a pusher reads here are the same
        /// words the gate-row route would have given them.
        gates: Box<VerdictAdmissionRefusal>,
    },
    /// The rule is set on a repository bound to no channel at all.
    RepositoryUnbound,
    /// The mission's newest policy record exists and this build cannot read
    /// it (finding 89).
    ///
    /// Both arms read the policy — (B) for its verifier flag and gate list,
    /// (C) for the gate list — so a record nobody can read closes both. An
    /// older record is never read in its place: that is the resurrection
    /// finding 89 caught.
    PolicyUnreadable {
        /// The mission whose policy it is.
        session_ref: String,
        /// The record that could not be read.
        event_id: String,
        /// The validator's own sentence about why.
        reason: String,
    },
    /// The mission is not bound to the repository being pushed (finding 91).
    ///
    /// Sharing a founder with the repository is not a binding: two
    /// repositories of one founder are two repositories, and a mission's
    /// rows and rulings prove commits for the ones its genesis names.
    MissionNotBoundToRepository {
        /// The mission whose proof was not admitted here.
        session_ref: String,
        /// The kind:30617 coordinate of the repository being pushed.
        repository: String,
    },
}

/// The gate-row route's own remedy, word for word the same wherever the rows
/// are what a push is short of — `NoApprovingVerdict` said it first, and a
/// pusher who has read one refusal should recognise the next.
const GATE_ROW_REMEDY: &str = "That route wants every required gate published green on this \
                               exact commit, by the mission's own provider, over a clean \
                               worktree. Run each gate as its own command so the host can \
                               record it.";

/// The verifier route, named the same way in every sentence that mentions it.
const VERIFIER_WANTS: &str = "a `refutation` verdict of `not-refuted` on that report, signed by \
                              a verifier seat of that mission";

/// The two-key point of arm (C), kept as one clause so it cannot drift.
const SETTLEMENT_NOT_CHECK: &str = "a lead's approval is the settlement, not the check";

/// The `ApprovedButNotVerified` sentence, composed from arm (B)'s status.
///
/// Finding 75 (live run 6, 2026-09-04). The mission's policy had
/// `gates.verifierRequired: false`, the lead had approved a report naming the
/// commit, no gate row named it, and the refusal said only "find a verifier"
/// and then offered a founder's push — a route Brian ruled out on 2026-09-03
/// ("humans never gate a landing"). Four shapes, each leading with the route
/// the policy actually runs:
///
/// * policy does not require a verifier, rows short — **lead with the rows**,
///   and mention the verifier only as the alternative;
/// * policy does not require a verifier, rows green — only reachable by a
///   pusher outside the mission (a seated one would have landed on arm (B)),
///   so the seat clause is the whole story;
/// * policy requires a verifier, rows short — the verifier is the missing
///   check, and the rows are owed as well, with the same remedy;
/// * policy requires a verifier, rows green — the verifier is the one thing
///   missing, and the sentence says so and nothing else.
///
/// No shape offers a human. The substring "no active verifier seat has
/// cleared the report it approves" survives in every one of them because two
/// callers outside this crate assert on it.
fn approved_but_not_verified(
    new_oid: &str,
    approvals: usize,
    verifier_required: bool,
    rows: &VerdictAdmissionGateRows,
    seated: bool,
) -> String {
    let mut reason = match (verifier_required, rows) {
        (false, VerdictAdmissionGateRows::Short(short)) => format!(
            "commit {new_oid} is approved ({approvals} approving disposition(s)), and no \
             founder-signed policy of this mission requires a verifier, so the gate-row route \
             is the one that applies — and it is not open for this commit: {} \
             {GATE_ROW_REMEDY} Or, if this mission seats a verifier, {VERIFIER_WANTS}, lands it \
             over those same rows; no active verifier seat has cleared the report it approves, \
             and {SETTLEMENT_NOT_CHECK}.",
            short.reason()
        ),
        (false, VerdictAdmissionGateRows::Green) => format!(
            "commit {new_oid} is approved ({approvals} approving disposition(s)), and every \
             required gate is observed green on it, by the mission's own provider, over a \
             clean worktree. No founder-signed policy of this mission requires a verifier, so \
             the gate-row route is the one that applies, and those rows satisfy it; no active \
             verifier seat has cleared the report it approves either, and \
             {SETTLEMENT_NOT_CHECK}."
        ),
        (true, VerdictAdmissionGateRows::Short(short)) => format!(
            "commit {new_oid} is approved ({approvals} approving disposition(s)) and no active \
             verifier seat has cleared the report it approves. This mission's founder-signed \
             policy requires a verifier, so green gate rows alone cannot land it: the gate \
             wants {VERIFIER_WANTS} — {SETTLEMENT_NOT_CHECK} — and every required gate observed \
             green on this commit besides. The rows are short too: {} The verifier route wants \
             the same rows the gate-row route does. {GATE_ROW_REMEDY}",
            short.reason()
        ),
        (true, VerdictAdmissionGateRows::Green) => format!(
            "commit {new_oid} is approved ({approvals} approving disposition(s)) and no active \
             verifier seat has cleared the report it approves. Every required gate is observed \
             green on it, by the mission's own provider, over a clean worktree, and this \
             mission's founder-signed policy requires a verifier, so those rows alone cannot \
             land it: the gate wants {VERIFIER_WANTS} — {SETTLEMENT_NOT_CHECK}."
        ),
    };
    if !seated {
        reason.push_str(
            " And this key holds no active seat of that mission, which both routes require of \
             the pusher.",
        );
    }
    reason
}

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
                source,
            } => format!(
                "require-verdict is set and no mission verdict names this commit: no approved \
                 report names {new_oid}. {} An older ruling can fall outside both. No observed \
                 gate row names {new_oid} either, so the gate-row route is not open for it: \
                 that route wants every required gate published green on this exact commit, by \
                 the mission's own provider, over a clean worktree. Run each gate as its own \
                 command so the host can record it.",
                source.searched_clause(*searched_sessions)
            ),
            Self::ApprovedReportNamesBranchOnly => "require-verdict is set and no mission verdict \
                 names this commit: the approved report for this work names a branch and no \
                 headSha, and a branch name is not a commit."
                .to_string(),
            Self::ApprovedButNotVerified {
                new_oid,
                approvals,
                verifier_required,
                rows,
                seated,
            } => approved_but_not_verified(new_oid, *approvals, *verifier_required, rows, *seated),
            Self::VerifierIsTheReportAuthor { new_oid, verifier } => format!(
                "commit {new_oid} is cleared only by {verifier}, which is the key that wrote the \
                 report being cleared. A seat cannot stand as the verifier of its own work."
            ),
            Self::PushNotSeated {
                new_oid,
                session_ref,
                seats,
            } => format!(
                "commit {new_oid} is cleared on mission {session_ref}, and this key is not an \
                 active seat of it ({seats} seat(s)). Both routes require the pusher to hold an \
                 active seat of that mission: resume the session so the host re-stages the seat, \
                 then push again."
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
            Self::RepositoryUnbound => format!(
                "require-verdict is set and there is nowhere to look for a mission verdict: \
                 this key holds no seat in the newest \
                 {VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS} authority transitions this relay \
                 could read, this repository names no project, and it is bound to no channel. \
                 Remove the rule, put the repository in a project, or bind it to the mission's \
                 channel."
            ),
            Self::PolicyUnreadable {
                session_ref,
                event_id,
                reason,
            } => format!(
                "mission {session_ref}'s newest policy record {event_id} could not be read by \
                 this build ({reason}), and a policy nobody can read closes both routes: the gate \
                 cannot tell whether it requires a verifier or which gates it names, and an older \
                 record must not stand in for the newest one. Publish a policy this relay can \
                 read, or withdraw it, then push again."
            ),
            Self::MissionNotBoundToRepository {
                session_ref,
                repository,
            } => format!(
                "mission {session_ref} is not bound to repository {repository}, so nothing it \
                 holds — gate rows or verdicts — can admit a commit here: a mission proves \
                 commits for the repositories its genesis and project name, and sharing a \
                 founder with this repository is not a binding. Push through a mission whose \
                 repoRef or project names {repository}."
            ),
        }
    }
}
