//! The rule a verdict-gated ref enforces: which mission ruling admits a commit.
//!
//! Live run 3 (2026-09-02) ended with a verifier's FAIL on the wire and the
//! branch on `main` anyway: nothing in the push path had ever read a verdict,
//! and the seat that pushed held the repo owner's authority by inheritance.
//! This module is the half of the fix that has no I/O — the exact question
//! *"does a mission ruling name this commit, and may this key land it?"* —
//! so the relay's pre-receive hook and `bee git check --ref` can answer it
//! with one function instead of two that drift.
//!
//! # The rule
//!
//! **Revised 2026-09-03 (Brian): humans never gate a landing.** The gate
//! exists to replace review with proof, not to put a person in the loop. An
//! update `(ref, old, new)` on a ref carrying the `require-verdict` protection
//! rule ([`crate::git_perms::ProtectionRule::require_verdict`]) is admitted
//! when **any** of the three arms holds.
//!
//! ## (A) The pusher is a founder
//!
//! A founder of the repository ([`crate::repository_founders::RepositoryFounders`]:
//! the announcement's signer, its NIP-34 `maintainers`, and every
//! project-roster Owner) lands a gated ref with **no verdict at all** — no
//! mission is read, no report is resolved, and a repository whose missions are
//! unreadable does not stop them. Founders are trusted humans; the gate is not
//! for them. The push record (30618) is what makes the landing observable
//! afterwards, and revocation stays a founder's power at every moment. The
//! evidence says so out loud — `policy_not_evaluated: founder_exception` —
//! so a founder's landing is never mistaken for a verifier-approved one.
//!
//! ## (C) A verifier's verdict names the commit, over gates observed green
//!
//! Otherwise the push needs a machine-checkable ruling by a key that is not
//! the one being ruled on. All of:
//!
//! 1. some kind 44244 `verdict` of subtype `disposition` whose
//!    [`CodingSessionTeamDispositionDecision::is_approval`] holds is
//!    **canonical in the session's fold**
//!    ([`fold_coding_session_team_transactions`], never raw events), and its
//!    `reportRef` resolves to a canonical `report` whose **`headSha`** equals
//!    `new` — compared whole and case-folded, so a report naming a 64-hex id
//!    names a different object and does not admit;
//! 2. that session's genesis is on the channel the repository is bound to and
//!    its founder is a founder of the repository — the caller resolves both
//!    and supplies only candidates that pass — **and the mission is bound to
//!    the repository being pushed** (finding 91): its
//!    [`VerdictAdmissionCandidate::bound_repositories`] names
//!    [`VerdictAdmissionQuery::repository`]. Two repositories of one founder
//!    are two repositories; a seat and green rows on one prove nothing about
//!    the other, and the rule refuses before either arm reads the mission;
//! 3. a **second, independent record clears the same report**: a canonical
//!    `verdict` of subtype **`refutation`** whose `decision` is `not-refuted`,
//!    whose `reportRef` is that report, signed by a key holding an active
//!    **`verifier`** seat of that mission and **not** the report's own author.
//!
//!    The verb is `refutation`, not `disposition`, because that is the only
//!    verifier-authored verdict the governance fold authorises: a disposition
//!    is `may_lead` (founder, active `lead`, steer grantee) at
//!    `coding_session_team_transaction_fold.rs:712`, while a refutation is
//!    `is_active_role(author, "verifier")` at `:709`. A verifier-signed
//!    *disposition* is excluded `Unauthorized` and is not evidence of
//!    anything. So the shape arm (C) requires is the one the model already
//!    has: the lead **settles** the assignment, and the verifier
//!    **independently fails to break it** — two records, two keys, neither
//!    prose.
//!
//!    Note what this deliberately does **not** accept, though
//!    [`crate::coding_session_completion_verification`] does (its case (b),
//!    `coding_session_completion_verification.rs:118`): a report whose own
//!    author holds the verifier seat. A completion is a claim about work; a
//!    push is the work. A seat standing as the verifier of its own report
//!    reproduces exactly the live-run-3 failure this gate exists to prevent;
//! 3b. the approving report's `branch`, when it names one, is the branch being
//!    pushed — an approval of a commit *for `main`* does not admit that commit
//!    onto `release`;
//! 4. the pusher is an **active seat of that mission**. Any seat may land what
//!    a verifier cleared; a stranger holding the same patch may not;
//! 5. **every required gate was observed green on this very commit** — arm
//!    (B)'s whole rule ([`observed::gates_observed_green`]) minus its first
//!    clause, the one that closes that arm when the policy requires a
//!    verifier. Added by the 2026-09-03 **follow-up** ruling (L22 §6.3, built
//!    in L27): proof is scaled to risk, and scaling means the riskier class
//!    gets strictly *more* proof, not different proof.
//!
//!    Until this clause existed a founder who set
//!    `gates.verifierRequired: true` made their own mission the **weaker** of
//!    the two arms — (B) demanded three green gates on the pushed commit and
//!    (C) demanded none — so tightening the policy loosened the landing. The
//!    rows cost nothing to require: the provider signs them whether or not
//!    anyone reads them.
//!
//!    Its refusal, [`VerdictAdmissionRefusal::VerifiedButGatesNotGreen`],
//!    names both halves — the verifier who *did* clear it and the gate that
//!    did not pass — because either half alone sends a pusher looking for the
//!    wrong thing.
//!
//! **The rule only ever subtracts.** A ref without the protection rule is
//! evaluated exactly as before; nothing here can admit a push the ordinary
//! role check already denied.
//!
//! ## (B) Every required gate was observed green on this commit
//!
//! The velocity arm, built 2026-09-03 (L22) once the wire could carry the
//! fact it needs. It is arm (C)'s clause 5 without a verifier rather than a
//! separate rule: what (B) drops is the second seat, never the gates. A
//! seat's push lands with **no second seat** when the
//! mission's newest founder-signed kind 44245 policy does not set
//! `gates.verifierRequired` — resolved, not guessed: a withdrawal or an
//! absent policy applies the defaults and the evidence says which, and a
//! newest record this build cannot read refuses rather than reading an older
//! one (finding 89) — and every gate the policy requires — or
//! [`DEFAULT_REQUIRED_GATES`] when it names none — has a folded kind 44246
//! row that is `source: observed`, names **this commit** in `headSha`, was
//! measured over a clean worktree, and says `passed`. `observed` means the
//! signer is a provider the mission's **lifecycle** proves (finding 90:
//! [`mission_provider_pubkeys_from_lifecycle`]), never merely a key that
//! published metadata. The pusher must hold an active seat, exactly as in (C).
//!
//! `headSha` is what makes it safe. Binding a mission's gate rows to a push by
//! anything weaker — "this mission has green rows *somewhere*" — would let a
//! green row from an earlier commit admit a later one, the same class of
//! defect as the `landedShas` claim finding 27 caught being false. The
//! provider resolves `git rev-parse HEAD` in the seat's own workdir when it
//! pairs a gate `tool_call` with its `tool_result`
//! (`buzz_session_provider::Provider::spawn_gate_head_probe`); the seat is
//! never asked, and cannot sign the row.
//!
//! The rule itself is in [`observed`]. Three things it will not do: read an
//! **absent** `headSha` as "the commit being pushed" (a row that predates the
//! key names no commit and admits nothing); count a **declared** row, however
//! exactly it names the commit, since that is its own subject speaking; or
//! substitute a clock for the binding, because the observation fold reads
//! none.
//!
//! Two records that look like they should admit and never do:
//!
//! - a report's **`branch`** — a name is not a commit, and two pushes to one
//!   branch are two commits of which one was ruled on;
//! - a completion's **`landedShas`** — that record is the lead's own claim
//!   that it landed something, which is exactly the claim finding 27 caught
//!   being false.

use nostr::Event;

// Arm (B) lives in a sibling file so no file here passes 1,000 lines.
#[path = "coding_session_verdict_admission_observed.rs"]
mod observed;

// What an admission stood on, in a file of its own — same ceiling.
#[path = "coding_session_verdict_admission_evidence.rs"]
mod evidence;
pub use evidence::VerdictAdmissionEvidence;

// Who provides a mission (finding 90 + B1), in a third file, same reason.
#[path = "coding_session_verdict_admission_lifecycle.rs"]
mod lifecycle;

// Finding 56's candidate source, split out for the same reason — and, since
// L35 pushed this file past the ceiling again, the prediction-grade seat
// projection that reads the same kind 44228 page.
#[path = "coding_session_verdict_admission_source.rs"]
mod source;
pub use lifecycle::{
    mission_provider_pubkeys_from_lifecycle, VERDICT_ADMISSION_MAX_LIFECYCLE_RECORDS,
};
#[allow(deprecated)]
pub use observed::metadata_signers;
use observed::{
    evaluate_observed_gates, gate_rows_status, gates_observed_green, policy_unreadable,
    ObservedGateVerdict,
};
pub use observed::{
    mission_observations, resolve_mission_gate_policy, GatePolicyResolution,
    VerdictAdmissionGatePolicy, VerdictAdmissionPolicyEvidence, VerdictAdmissionPolicyNotEvaluated,
    VerdictAdmissionPolicyResolution, DEFAULT_REQUIRED_GATES, VERDICT_ADMISSION_MAX_OBSERVATIONS,
    VERDICT_ADMISSION_MAX_POLICIES, VERDICT_ADMISSION_MAX_PROVIDER_METADATA,
};
pub use source::{
    active_seats_from_authority_transitions, VerdictAdmissionCandidateSource,
    VERDICT_ADMISSION_BOUND_CHANNEL, VERDICT_ADMISSION_MAX_PROJECT_CHANNELS,
    VERDICT_ADMISSION_MAX_PUSHER_SEATS,
};

use crate::coding_session_observation::CodingSessionObservationGateEntry;
use crate::coding_session_team_transaction::{
    fold_coding_session_team_transactions, validate_coding_session_team_transaction_envelope,
    CodingSessionTeamActiveSeat, CodingSessionTeamFold, CodingSessionTeamFoldContext,
    CodingSessionTeamRefutationDecision, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionPayload, CodingSessionTeamVerdict,
};

/// Newest genesis events one verdict-gated ref update may search.
///
/// A SHA→session projection would make this O(1) and is deliberately absent:
/// it costs a migration whose numbering collides with `vanilla/main`. The
/// refusal copy discloses the cap instead of hiding it.
pub const VERDICT_ADMISSION_MAX_SESSIONS: usize = 16;

/// Newest kind 44244 transactions one verdict-gated ref update may read.
///
/// Read as one page across the bound channel rather than per session: kind
/// 44244 is not addressable, so its umbrella label is not a queryable column
/// and the split into missions happens after the read.
pub const VERDICT_ADMISSION_MAX_TRANSACTIONS: usize = 512;

/// Newest kind 44228 authority transitions one push or prediction may read.
///
/// Two readers, one bound. The **seat list** of a mission is still the relay's
/// own accepted projection; only the CLI folds the chain off the wire for
/// that, and past this bound a prediction can only get *fewer* seats and so
/// predict a refusal it might not get.
///
/// Since finding 56 both sides also read this page to answer a different
/// question — *which missions seat the pusher* — because that is the
/// authoritative record of a seat and no coding-session kind carries the
/// grantee in an indexed `p` tag. Past the bound a seat is simply not found,
/// and the search falls back to the project or the bound channel: fewer
/// candidates, never more.
pub const VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS: usize = 512;

/// The exact update being judged.
#[derive(Debug, Clone, Copy)]
pub struct VerdictAdmissionQuery<'a> {
    /// Full ref name, e.g. `refs/heads/main`.
    pub ref_name: &'a str,
    /// The object id the push would leave on that ref.
    pub new_oid: &'a str,
    /// Hex pubkey the push authenticated as.
    pub pusher_pubkey: &'a str,
    /// Every founder of the repository, lower-hex.
    ///
    /// Finding 33: this was a single `repo_owner_pubkey`, the kind:30617
    /// signer, which made a two-human repository unlandable by one of them.
    /// The caller resolves the set with
    /// [`crate::repository_founders::RepositoryFounders`] — signer ∪ NIP-34
    /// `maintainers` ∪ project-roster Owners — and passes
    /// [`crate::repository_founders::RepositoryFounders::pubkeys`] here;
    /// `buzz-core` cannot query the roster, so it never resolves it itself.
    ///
    /// An empty slice admits nothing: no candidate's founder is in it, and no
    /// pusher is either.
    pub repo_founders: &'a [String],
    /// Which lookup produced the candidates (finding 56).
    ///
    /// Borrowed so this type stays `Copy`. It changes no decision — the rule
    /// judges the candidates it was handed either way — and exists so the one
    /// refusal that reports a *count* also reports what was counted.
    pub candidate_source: &'a VerdictAdmissionCandidateSource,
    /// The kind:30617 coordinate (`30617:<owner-hex>:<d>`) of the repository
    /// being pushed (finding 91).
    ///
    /// A candidate whose [`VerdictAdmissionCandidate::bound_repositories`]
    /// does not name it is refused before arms (B) and (C) look at it, with
    /// [`VerdictAdmissionRefusal::MissionNotBoundToRepository`]. Founder
    /// overlap is no longer a binding.
    pub repository: &'a str,
}

/// One canonical transaction, decoded once for the search.
#[derive(Debug, Clone)]
pub struct VerdictAdmissionRecord {
    /// Event id.
    pub event_id: String,
    /// Hex pubkey that signed it.
    pub author_pubkey: String,
    /// Its decoded, validated payload.
    pub payload: CodingSessionTeamTransactionPayload,
}

/// One mission whose founder owns the repository, already folded.
#[derive(Debug, Clone)]
pub struct VerdictAdmissionCandidate {
    /// Umbrella session UUID.
    pub session_ref: String,
    /// Genesis event id.
    pub genesis_ref: String,
    /// Genesis signer — equal to the repository owner, checked again here.
    pub founder_pubkey: String,
    /// The fold's canonical records, in included order
    /// ([`canonical_records`]).
    pub canonical: Vec<VerdictAdmissionRecord>,
    /// Active seats of this umbrella, **with their roles**.
    ///
    /// The role is load-bearing since the 2026-09-03 ruling: arm (C) asks not
    /// only *"is this key seated"* but *"does it hold `verifier`"*, and a
    /// bare pubkey list — what this field was until then — cannot answer the
    /// second. The relay and the desktop already hold the role next to the
    /// pubkey; only this type was throwing it away.
    pub active_seats: Vec<CodingSessionTeamActiveSeat>,
    /// This mission's folded kind 44246 gate rows — arm (B)'s only evidence.
    ///
    /// **Folded, never raw**, and folded with the mission's provider set
    /// supplied: `fold_coding_session_observations` downgrades an `observed`
    /// claim from a signer no provider backs to `declared`, so a seat cannot
    /// reach this arm by writing the word about itself. A caller that folded
    /// without the provider set hands over rows whose provenance nothing
    /// checked, and every one of them is `declared` to this rule unless the
    /// fold said otherwise.
    ///
    /// Empty is the honest default for a caller that read no observations: the
    /// arm is then silent, and the push is judged by arm (C) exactly as
    /// before.
    pub observed_gates: Vec<CodingSessionObservationGateEntry>,
    /// What this mission's newest kind 44245 record resolved to (finding 89).
    ///
    /// A **resolution**, never an `Option`: `Absent` and `Withdrawn` both
    /// apply the defaults and say which of the two happened, `Present`
    /// carries the record's gate half, and `Unreadable` refuses on every arm
    /// — the one direction the old `None` could not express, and the one
    /// that opened arm (B) on a `verifierRequired: true` the page missed.
    /// Callers resolve it with [`resolve_mission_gate_policy`].
    pub gate_policy: GatePolicyResolution,
    /// The kind:30617 coordinates this mission may prove commits for
    /// (finding 91): the repository its genesis or metadata `repoRef` names,
    /// and every repository of its project.
    ///
    /// Empty binds the mission to nothing, and nothing it holds admits a
    /// push anywhere. The caller populates it; the rule only requires it.
    pub bound_repositories: Vec<String>,
    /// Kind 44245 records naming this mission that the signer rule **excluded**
    /// (2026-09-05 refuter, S2). Disclosure, never a decision: the rule reads
    /// the newest *authorized* record either way, and the silence this
    /// replaces is what a flood against the per-mission read is made of. A
    /// caller that cannot count them says `0` rather than guessing.
    pub excluded_unauthorized_policies: u32,
}

/// Arm (B)'s status on one commit, carried inside an arm-(C) refusal.
///
/// Finding 75: every arm-(C) refusal that follows an arm-(B) evaluation
/// reports what arm (B) found, so a pusher is never told to find a verifier
/// over a mission whose policy never asked for one. `Green` is "every
/// required gate is observed green on this commit over a clean worktree";
/// `Short` is arm (B)'s own sentence about what is not, carried whole rather
/// than paraphrased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictAdmissionGateRows {
    /// Every required gate was observed green on this commit, by the
    /// mission's own provider, over a clean worktree.
    Green,
    /// Something is short, and this is the sentence arm (B) would have
    /// refused with — boxed because a refusal that contains a refusal is
    /// otherwise infinitely sized.
    Short(Box<VerdictAdmissionRefusal>),
}

/// The refusal type and its frozen copy live in a sibling file so no file
/// here passes 1,000 lines; the enum is re-exported under its old path.
#[path = "coding_session_verdict_admission_reasons.rs"]
mod reasons;
pub use reasons::VerdictAdmissionRefusal;

/// The answer, for one ref update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictAdmission {
    /// A canonical ruling names this commit and this key may land it.
    Admitted(VerdictAdmissionEvidence),
    /// It does not, or this key may not.
    Refused(VerdictAdmissionRefusal),
}

impl VerdictAdmission {
    /// Whether the update is admitted.
    pub fn is_admitted(&self) -> bool {
        matches!(self, Self::Admitted(_))
    }
}

/// Decode a fold's canonical events, in included order.
///
/// The filter lives here rather than in each caller so "canonical" cannot
/// come to mean two things: an event outside
/// [`CodingSessionTeamFold::included_event_ids`] is excluded, superseded or
/// unauthorized, and is never evidence. Events that fail to decode are
/// skipped — the fold already validated every included one, so this cannot
/// silently drop a canonical record.
pub fn canonical_records(
    events: &[Event],
    fold: &CodingSessionTeamFold,
) -> Vec<VerdictAdmissionRecord> {
    fold.included_event_ids
        .iter()
        .filter_map(|id| {
            let event = events.iter().find(|event| &event.id.to_hex() == id)?;
            let payload = validate_coding_session_team_transaction_envelope(event).ok()?;
            Some(VerdictAdmissionRecord {
                event_id: id.clone(),
                author_pubkey: event.pubkey.to_hex(),
                payload,
            })
        })
        .collect()
}

/// The fold context both push-path callers judge with.
///
/// `active_grants` is deliberately empty. The relay's authority projection
/// (`coding_session_authority_acl`) records live grants without the event id
/// the fold context requires, and inventing one would be a fabricated
/// reference in an authorization input. The consequence is disclosed and
/// fail-closed: a record published by a *steering operator* rather than the
/// founder or an active seat does not fold here, so a mission run that way
/// cannot admit a push — it is refused, never wrongly admitted.
pub fn verdict_admission_fold_context(
    channel_ref: impl Into<String>,
    session_ref: impl Into<String>,
    genesis_ref: impl Into<String>,
    founder_pubkey: impl Into<String>,
    active_seats: Vec<CodingSessionTeamActiveSeat>,
) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: channel_ref.into(),
        session_ref: session_ref.into(),
        genesis_ref: genesis_ref.into(),
        founder_pubkey: founder_pubkey.into(),
        active_seats,
        active_grants: Vec::new(),
        // `CodingSessionTeamFoldContext::verifier_required` says a caller
        // that has not read the policy must pass `false`: the fold then
        // behaves exactly as it did before the flag existed. It stays `false`
        // here even now that the gate reads the policy, because the flag it
        // reads governs **arm (B)** — whether green gate rows may land a push
        // — and never which 44244 records fold. Turning it on here would make
        // an unverified report non-canonical and so change what arm (C) can
        // see, which is a different rule the founder did not ask for.
        verifier_required: false,
    }
}

/// The transactions belonging to one mission, out of a page read for a whole
/// channel.
///
/// A record belongs to a mission only when **both** envelope tags agree: the
/// umbrella label `d` and the immutable `cstx-genesis`. The label alone is
/// re-usable; the genesis is not. Shared by the relay's gate and
/// `bee git check --ref` so "which records are this mission's" cannot come to
/// mean two things.
pub fn mission_transactions<'a>(
    session_ref: &str,
    genesis_ref: &str,
    transactions: &'a [Event],
) -> Vec<&'a Event> {
    transactions
        .iter()
        .filter(|event| {
            has_exact_tag(event, "d", session_ref)
                && has_exact_tag(event, "cstx-genesis", genesis_ref)
        })
        .collect()
}

/// Whether an event carries exactly this two-value tag.
pub(super) fn has_exact_tag(event: &Event, name: &str, value: &str) -> bool {
    event.tags.iter().any(|tag| match tag.as_slice() {
        [tag_name, tag_value] => tag_name == name && tag_value == value,
        _ => false,
    })
}

/// Fold one candidate's events and decode its canonical records.
///
/// A fold that errors (the caller supplied a cross-context event, or a cycle)
/// yields no records rather than failing the whole push: one malformed
/// session must not deny every ref update, and a candidate with no records
/// admits nothing.
pub fn fold_candidate_records(
    events: &[Event],
    context: &CodingSessionTeamFoldContext,
) -> Result<Vec<VerdictAdmissionRecord>, String> {
    let fold = fold_coding_session_team_transactions(events, context)?;
    Ok(canonical_records(events, &fold))
}

/// Judge one ref update against the two arms of the 2026-09-03 ruling.
///
/// Arm (A) is answered first and reads no mission at all: a founder's push
/// must not depend on a mission being readable, foldable, or even present.
/// Only when the pusher is not a founder does arm (C) search the candidates,
/// newest first, stopping at the first mission that admits. `candidates` is
/// the bounded set the caller resolved (at most
/// [`VERDICT_ADMISSION_MAX_SESSIONS`]); its length is what the refusal
/// discloses as searched.
pub fn evaluate_verdict_admission(
    candidates: &[VerdictAdmissionCandidate],
    query: &VerdictAdmissionQuery<'_>,
) -> VerdictAdmission {
    // ── Arm (A) ──────────────────────────────────────────────────────────
    if is_founder(query.repo_founders, query.pusher_pubkey) {
        return VerdictAdmission::Admitted(VerdictAdmissionEvidence::FounderPush {
            pusher_pubkey: query.pusher_pubkey.to_ascii_lowercase(),
            policy_not_evaluated: VerdictAdmissionPolicyNotEvaluated::FounderException,
        });
    }
    // The two guards every candidate passes before either arm reads it
    // (findings 91 and 89), and the nearest miss each produced.
    let mut unbound_mission: Option<VerdictAdmissionRefusal> = None;
    let mut unreadable_policy: Option<VerdictAdmissionRefusal> = None;

    // ── Arm (C) ──────────────────────────────────────────────────────────
    let mut approved_branch_only = false;
    let mut approved_for_other_ref: Option<String> = None;
    // Near-misses, kept so the refusal can name the nearest missing fact
    // rather than the most generic one. `NoApprovingVerdict` is the answer
    // only when nothing at all named the commit.
    let mut approvals_naming_the_commit: usize = 0;
    // Finding 75: the facts `ApprovedButNotVerified` composes with — the
    // policy's verifier flag, arm (B)'s row status on this commit, and the
    // pusher's seat — taken from the first mission whose approval nobody
    // cleared. Recorded where that near-miss happens, so the refusal's
    // sentence is about the mission the approval came from.
    let mut approved_but_not_verified: Option<(bool, VerdictAdmissionGateRows, bool)> = None;
    let mut self_approving_verifier: Option<String> = None;
    let mut verified_but_unseated: Option<(String, usize)> = None;
    // The 2026-09-03 follow-up ruling's own near-miss: everything arm (C)
    // wanted except the gate rows. Kept rather than returned so a *second*
    // candidate that satisfies both halves still admits.
    let mut verified_but_gates_not_green: Option<(String, VerdictAdmissionRefusal)> = None;
    // ── Arm (B) ──────────────────────────────────────────────────────────
    // Answered before arm (C) searches, because it is the cheap arm and the
    // one a mission with no verifier is actually running. Its refusal is kept
    // rather than returned, so a mission that *also* has a verdict is not
    // handed a gate-row sentence about a route it is not taking.
    let mut observed_refusal: Option<VerdictAdmissionRefusal> = None;
    for candidate in candidates {
        if !candidate_stands(
            candidate,
            query,
            &mut unbound_mission,
            &mut unreadable_policy,
        ) {
            continue;
        }
        match evaluate_observed_gates(candidate, query.new_oid, query.pusher_pubkey) {
            ObservedGateVerdict::Admits(evidence) => return VerdictAdmission::Admitted(evidence),
            ObservedGateVerdict::Refuses(refusal) => {
                observed_refusal.get_or_insert(refusal);
            }
            ObservedGateVerdict::Silent => {}
        }
    }

    for candidate in candidates {
        if !candidate_stands(
            candidate,
            query,
            &mut unbound_mission,
            &mut unreadable_policy,
        ) {
            continue;
        }
        for record in &candidate.canonical {
            let CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
                report_ref,
                decision,
                ..
            }) = &record.payload.body
            else {
                continue;
            };
            if !decision.is_approval() {
                continue;
            }
            let Some(report_record) = canonical_report_record(candidate, report_ref) else {
                continue;
            };
            let CodingSessionTeamTransactionBody::Report(report) = &report_record.payload.body
            else {
                continue;
            };
            match report.head_sha.as_deref() {
                Some(head_sha) if eq_hex(head_sha, query.new_oid) => {
                    approvals_naming_the_commit += 1;
                    // Condition 3b: an approval is scoped to the branch its
                    // report named. Approving a commit *for `main`* is not
                    // approval to put it on `release`, nor to roll `main` back
                    // to it from somewhere else. A report naming no branch
                    // scopes nothing — it says only "this commit is good".
                    if let Some(branch) = report.branch.as_deref() {
                        if !ref_names_branch(query.ref_name, branch) {
                            approved_for_other_ref.get_or_insert_with(|| branch.to_string());
                            continue;
                        }
                    }
                    // Condition 3: a verifier seat must independently have
                    // failed to refute this same report.
                    let cleared = clearing_refutation(candidate, report_ref);
                    let Some(cleared) = cleared else {
                        if let Some(author) = self_cleared_by(candidate, report_ref, report_record)
                        {
                            self_approving_verifier.get_or_insert(author);
                        }
                        approved_but_not_verified.get_or_insert_with(|| {
                            (
                                candidate.gate_policy.requires_a_verifier(),
                                gate_rows_status(candidate, query.new_oid),
                                is_active_seat(candidate, query.pusher_pubkey),
                            )
                        });
                        continue;
                    };
                    // Condition 4: any active seat of that mission may land it.
                    if !is_active_seat(candidate, query.pusher_pubkey) {
                        verified_but_unseated.get_or_insert_with(|| {
                            (candidate.session_ref.clone(), candidate.active_seats.len())
                        });
                        continue;
                    }
                    // Condition 5, added by the 2026-09-03 follow-up ruling:
                    // arm (B)'s evidence, minus its "the policy must not
                    // require a verifier" clause — which this mission has
                    // already satisfied by producing one. A verifier-required
                    // mission was otherwise the *weaker* of the two arms.
                    let green =
                        match gates_observed_green(candidate, query.new_oid, query.pusher_pubkey) {
                            Ok(green) => green,
                            Err(refusal) => {
                                verified_but_gates_not_green.get_or_insert_with(|| {
                                    (cleared.author_pubkey.to_ascii_lowercase(), refusal)
                                });
                                continue;
                            }
                        };
                    return VerdictAdmission::Admitted(VerdictAdmissionEvidence::VerifierVerdict {
                        session_ref: candidate.session_ref.clone(),
                        disposition_event_id: record.event_id.clone(),
                        refutation_event_id: cleared.event_id.clone(),
                        report_event_id: report_ref.to_string(),
                        head_sha: head_sha.to_string(),
                        verifier_pubkey: cleared.author_pubkey.to_ascii_lowercase(),
                        gates: green.gates,
                        row_event_ids: green.row_event_ids,
                        // `candidate_stands` refused an unreadable policy
                        // above; `Absent` is the honest fallback for a
                        // resolution that somehow carries no evidence.
                        policy: candidate
                            .gate_policy
                            .evidence(candidate.excluded_unauthorized_policies)
                            .unwrap_or(VerdictAdmissionPolicyEvidence {
                                event_id: None,
                                resolution: VerdictAdmissionPolicyResolution::Absent,
                                excluded_unauthorized: candidate.excluded_unauthorized_policies,
                            }),
                    });
                }
                Some(_) => {}
                // `branch` never admits: a name is not a commit.
                None if report.branch.is_some() => approved_branch_only = true,
                None => {}
            }
        }
    }

    // A policy nobody could read outranks every other refusal: it is not a
    // fact about this push but a condition on the mission that someone must
    // fix before any push can be judged, and a sentence about rows or seats
    // would send the pusher to work on the wrong thing (finding 89).
    if let Some(refusal) = unreadable_policy {
        return VerdictAdmission::Refused(refusal);
    }
    // Nearest missing fact first, and this is the nearest of all: a commit a
    // verifier cleared, pushed by a seat of that very mission, short only of
    // the gate rows. Every other refusal below is about a fact further from
    // the pusher's own situation.
    if let Some((verifier, gates)) = verified_but_gates_not_green {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::VerifiedButGatesNotGreen {
            new_oid: query.new_oid.to_ascii_lowercase(),
            verifier,
            gates: Box::new(gates),
        });
    }
    // A verified commit blocked only on the pusher's seat is a different
    // problem from one nobody verified.
    if let Some((session_ref, seats)) = verified_but_unseated {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::PushNotSeated {
            new_oid: query.new_oid.to_ascii_lowercase(),
            session_ref,
            seats,
        });
    }
    if let Some(verifier) = self_approving_verifier {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::VerifierIsTheReportAuthor {
            new_oid: query.new_oid.to_ascii_lowercase(),
            verifier,
        });
    }
    if let Some(approved_branch) = approved_for_other_ref {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedForAnotherRef {
            new_oid: query.new_oid.to_ascii_lowercase(),
            approved_branch,
        });
    }
    // Every approval naming the commit that was not returned above as a
    // nearer miss reached the "nobody cleared it" branch, so the facts are
    // always recorded when the count is positive; the `if let` is the honest
    // shape rather than a count with nothing behind it.
    if let Some((verifier_required, rows, seated)) = approved_but_not_verified {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedButNotVerified {
            new_oid: query.new_oid.to_ascii_lowercase(),
            approvals: approvals_naming_the_commit,
            verifier_required,
            rows,
            seated,
        });
    }
    // Arm (B)'s near-miss ranks below every arm (C) one that named this exact
    // commit, and above the generic "nothing named it": a red gate row on the
    // pushed SHA is a far more actionable sentence than a bound disclosure.
    if let Some(refusal) = observed_refusal {
        return VerdictAdmission::Refused(refusal);
    }
    if approved_branch_only {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedReportNamesBranchOnly);
    }
    // A mission the pusher holds that is bound elsewhere (finding 91) ranks
    // below every sentence a bound mission produced about this commit, and
    // above "nothing named it": the pusher has proof, on the wrong repository.
    if let Some(refusal) = unbound_mission {
        return VerdictAdmission::Refused(refusal);
    }
    VerdictAdmission::Refused(VerdictAdmissionRefusal::NoApprovingVerdict {
        new_oid: query.new_oid.to_ascii_lowercase(),
        searched_sessions: candidates.len(),
        source: query.candidate_source.clone(),
    })
}

/// Whether a candidate may be read by either arm, and if not, which miss it
/// records.
///
/// Three questions in order. The founder check is the one from finding 33 —
/// a mission counts only when its founder founded the repository — and is
/// silent, because the relay's genesis query is scoped to the same set and
/// the desktop and CLI assemble a single candidate that already passed it.
/// The binding check (finding 91) refuses a mission bound to some other
/// repository, founder overlap notwithstanding. The policy check (finding
/// 89) refuses a mission whose newest policy nobody could read. Both misses
/// are kept, not returned, so a *second* candidate that stands may still
/// admit.
fn candidate_stands(
    candidate: &VerdictAdmissionCandidate,
    query: &VerdictAdmissionQuery<'_>,
    unbound_mission: &mut Option<VerdictAdmissionRefusal>,
    unreadable_policy: &mut Option<VerdictAdmissionRefusal>,
) -> bool {
    if !is_founder(query.repo_founders, &candidate.founder_pubkey) {
        return false;
    }
    if !candidate
        .bound_repositories
        .iter()
        .any(|bound| same_repository(bound, query.repository))
    {
        unbound_mission.get_or_insert_with(|| {
            VerdictAdmissionRefusal::MissionNotBoundToRepository {
                session_ref: candidate.session_ref.clone(),
                repository: query.repository.to_owned(),
            }
        });
        return false;
    }
    if let Some(refusal) = policy_unreadable(candidate) {
        unreadable_policy.get_or_insert(refusal);
        return false;
    }
    true
}

/// Whether two `30617:<owner-hex>:<d>` coordinates name one repository.
///
/// The kind and the owner's hex are compared case-folded; the `d`
/// identifier is compared exactly, because it is the repository's own name
/// and `Repo` and `repo` are two names. A string that is not a three-part
/// coordinate is compared whole, so a caller passing something else gets
/// exact matching rather than a guess.
fn same_repository(left: &str, right: &str) -> bool {
    let parts = |coordinate: &str| -> Option<(String, String, String)> {
        let mut split = coordinate.splitn(3, ':');
        let kind = split.next()?;
        let owner = split.next()?;
        let d = split.next()?;
        Some((
            kind.to_ascii_lowercase(),
            owner.to_ascii_lowercase(),
            d.to_owned(),
        ))
    };
    match (parts(left), parts(right)) {
        (Some(left), Some(right)) => left == right,
        _ => left == right,
    }
}

/// The canonical `not-refuted` refutation of `report_ref` signed by an active
/// verifier seat **other than** the report's own author, if one exists.
fn clearing_refutation<'a>(
    candidate: &'a VerdictAdmissionCandidate,
    report_ref: &str,
) -> Option<&'a VerdictAdmissionRecord> {
    let report_author = candidate
        .canonical
        .iter()
        .find(|record| record.event_id == report_ref)
        .map(|record| record.author_pubkey.as_str())?;
    candidate.canonical.iter().find(|record| {
        is_not_refuted_of(record, report_ref)
            && holds_verifier_seat(candidate, &record.author_pubkey)
            && !eq_hex(&record.author_pubkey, report_author)
    })
}

/// The verifier who cleared `report_ref` but *is* its author — the near-miss
/// worth naming, so the refusal says "you cannot verify your own work" rather
/// than the generic "nobody verified it".
fn self_cleared_by(
    candidate: &VerdictAdmissionCandidate,
    report_ref: &str,
    report_record: &VerdictAdmissionRecord,
) -> Option<String> {
    candidate
        .canonical
        .iter()
        .find(|record| {
            is_not_refuted_of(record, report_ref)
                && holds_verifier_seat(candidate, &record.author_pubkey)
                && eq_hex(&record.author_pubkey, &report_record.author_pubkey)
        })
        .map(|record| record.author_pubkey.to_ascii_lowercase())
}

/// Whether this record is a `not-refuted` refutation of exactly `report_ref`.
///
/// `confirmed` and `blocked` are rulings too, and both are rulings *against*:
/// a verifier who found the failure, or who could not conclude, has cleared
/// nothing.
fn is_not_refuted_of(record: &VerdictAdmissionRecord, report_ref: &str) -> bool {
    matches!(
        &record.payload.body,
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            report_ref: ruled,
            decision: CodingSessionTeamRefutationDecision::NotRefuted,
            ..
        }) if ruled == report_ref
    )
}

/// Whether `pubkey` holds an active `verifier` seat of this mission.
///
/// The role string is compared exactly, lowercase, as the authority
/// projection stores it. A seat holding `verifier-2` or `Verifier ` is not
/// this seat: a role is a closed token in the hire, not a description.
fn holds_verifier_seat(candidate: &VerdictAdmissionCandidate, pubkey: &str) -> bool {
    candidate
        .active_seats
        .iter()
        .any(|seat| seat.role == "verifier" && eq_hex(&seat.actor_pubkey, pubkey))
}

/// Whether `pubkey` holds any active seat of this mission.
fn is_active_seat(candidate: &VerdictAdmissionCandidate, pubkey: &str) -> bool {
    candidate
        .active_seats
        .iter()
        .any(|seat| eq_hex(&seat.actor_pubkey, pubkey))
}

/// Whether `pubkey` is one of the repository's founders. Case-folded, whole.
fn is_founder(founders: &[String], pubkey: &str) -> bool {
    founders.iter().any(|founder| eq_hex(founder, pubkey))
}

/// The canonical **record** of the report a disposition governs, or `None`
/// when the fold did not keep one under that id.
///
/// The whole record rather than the body since the 2026-09-03 ruling: arm (C)
/// compares the verifier's key against the *report author's*, so throwing the
/// envelope away here would leave the caller unable to tell a verifier from
/// the seat it is supposed to be checking.
fn canonical_report_record<'a>(
    candidate: &'a VerdictAdmissionCandidate,
    report_ref: &str,
) -> Option<&'a VerdictAdmissionRecord> {
    candidate.canonical.iter().find(|record| {
        record.event_id == report_ref
            && matches!(
                record.payload.body,
                CodingSessionTeamTransactionBody::Report(_)
            )
    })
}

/// Whether `ref_name` is the ref a report's `branch` names.
///
/// A report may name a bare branch (`main`) or a full ref
/// (`refs/heads/main`); both mean the same branch, and neither means any
/// other ref. Matched whole — `main` does not name `main-2`.
fn ref_names_branch(ref_name: &str, branch: &str) -> bool {
    ref_name == branch
        || ref_name
            .strip_prefix("refs/heads/")
            .is_some_and(|short| short == branch)
        || branch
            .strip_prefix("refs/heads/")
            .is_some_and(|short| ref_name.strip_prefix("refs/heads/") == Some(short))
}

/// Whole-string, case-folded hex comparison.
///
/// Whole is load-bearing: a 40-hex object id and a 64-hex event id that share
/// a prefix name different objects, and a prefix match would admit the wrong
/// commit.
fn eq_hex(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

/// The kind 44246 half every arm-(C) fixture needs since L27, shared so the
/// four test files cannot each grow their own — and one of them skip the
/// provenance fold.
#[cfg(test)]
#[path = "coding_session_verdict_admission_gate_fixture.rs"]
mod gate_fixture;

#[cfg(test)]
#[path = "coding_session_verdict_admission_tests.rs"]
mod tests;

/// The two arms of the 2026-09-03 ruling, in their own file: they need a
/// verifier seat every other fixture here lacks.
#[cfg(test)]
#[path = "coding_session_verdict_admission_arms_tests.rs"]
mod arms_tests;

/// Arm (B), in its own file: its fixtures are kind 44246 observations rather
/// than kind 44244 transactions, and its cases are ten.
#[cfg(test)]
#[path = "coding_session_verdict_admission_observed_tests.rs"]
mod observed_tests;

/// Arm (C) after the 2026-09-03 follow-up ruling, in its own file: its
/// fixtures need a transaction chain **and** folded observations at once,
/// which neither sibling carries.
#[cfg(test)]
#[path = "coding_session_verdict_admission_verified_tests.rs"]
mod verified_tests;

/// Findings 89, 90 and 91 (2026-09-05 admission audit), in their own file:
/// policy resolution, lifecycle-proven providers, and the repository binding.
#[cfg(test)]
#[path = "coding_session_verdict_admission_hardening_tests.rs"]
mod hardening_tests;

/// Who may commission an execution (2026-09-05 refuter, B1), in its own file:
/// the impostor-signed create, the hire a host answers, the operator grant.
#[cfg(test)]
#[path = "coding_session_verdict_admission_commission_tests.rs"]
mod commission_tests;
