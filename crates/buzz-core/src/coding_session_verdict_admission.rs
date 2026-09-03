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
//! afterwards, and revocation stays a founder's power at every moment.
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
//!    and supplies only candidates that pass;
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
//! `gates.verifierRequired`, and every gate the policy requires — or
//! [`DEFAULT_REQUIRED_GATES`] when it names none — has a folded kind 44246
//! row that is `source: observed`, names **this commit** in `headSha`, was
//! measured over a clean worktree, and says `passed`. The pusher must hold an
//! active seat, exactly as in (C).
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
use observed::{evaluate_observed_gates, gates_observed_green, ObservedGateVerdict};
pub use observed::{
    mission_gate_policy, mission_observations, mission_provider_pubkeys,
    VerdictAdmissionGatePolicy, DEFAULT_REQUIRED_GATES, VERDICT_ADMISSION_MAX_OBSERVATIONS,
    VERDICT_ADMISSION_MAX_POLICIES, VERDICT_ADMISSION_MAX_PROVIDER_METADATA,
};

use crate::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
};
use crate::coding_session_observation::CodingSessionObservationGateEntry;
use crate::coding_session_team_transaction::{
    fold_coding_session_team_transactions, validate_coding_session_team_transaction_envelope,
    CodingSessionTeamActiveSeat, CodingSessionTeamFold, CodingSessionTeamFoldContext,
    CodingSessionTeamRefutationDecision, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionPayload, CodingSessionTeamVerdict,
};
use crate::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION;

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

/// Newest kind 44228 authority transitions one prediction may read.
///
/// The relay never reads these — it uses its own accepted projection. This
/// bound exists so the CLI's prediction has no unbounded read in a rule whose
/// whole thesis is disclosed bounds; past it, a prediction can only get
/// *fewer* seats and so predict a refusal it might not get.
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
    /// The gate half of this mission's newest founder-signed kind 44245
    /// policy, when the caller read one.
    ///
    /// `None` is "the caller read no policy", which this rule treats the same
    /// as a policy setting no flag: arm (B) is available and the default gate
    /// list applies. That is deliberate and it is the direction that only ever
    /// *subtracts* nothing — it never turns a verifier requirement off, since
    /// a `Some(true)` is the only thing that could have been read.
    pub gate_policy: Option<VerdictAdmissionGatePolicy>,
}

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
    },
}

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
    },
    /// An approved report exists for this work but names only a branch.
    ApprovedReportNamesBranchOnly,
    /// Someone approved a report naming this commit, but nobody who could
    /// stand as its verifier did.
    ApprovedButNotVerified {
        /// The approved object id.
        new_oid: String,
        /// How many approving dispositions named it — disclosed so a reader
        /// can tell "nobody ruled" from "the wrong people ruled".
        approvals: usize,
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
}

/// The frozen refusal copy lives in a sibling file so no file here passes
/// 1,000 lines; it is `impl VerdictAdmissionRefusal` and adds no new name.
#[path = "coding_session_verdict_admission_reasons.rs"]
mod reasons;

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

/// Project active role seats from a session's stored kind 44228 transitions.
///
/// **Prediction-grade, and only for a caller that has no better source.** The
/// relay enforces with its own accepted projection
/// (`buzz_db::coding_session_acl::session_authority_for_hire`), which is
/// authoritative because the relay refuses to store a transition it did not
/// accept. A client reading events off the wire has no such guarantee, so
/// `bee git check --ref` uses this and says "prediction" rather than
/// "promise". Transitions that fail signature verification, name another
/// genesis, or arrive out of sequence are skipped rather than trusted.
pub fn active_seats_from_authority_transitions(
    events: &[Event],
    genesis_ref: &str,
) -> Vec<CodingSessionTeamActiveSeat> {
    let mut links: Vec<(
        u32,
        CodingSessionAuthorityTransitionType,
        String,
        Option<String>,
    )> = Vec::new();
    for event in events {
        if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_AUTHORITY_TRANSITION {
            continue;
        }
        if crate::verify_event(event).is_err() {
            continue;
        }
        let Ok(payload) = decode_coding_session_authority_transition(&event.content) else {
            continue;
        };
        if payload.genesis_ref != genesis_ref {
            continue;
        }
        links.push((
            payload.seq,
            payload.transition_type,
            payload.grantee_pubkey,
            payload.role,
        ));
    }
    links.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));

    let mut seats: Vec<CodingSessionTeamActiveSeat> = Vec::new();
    for (_, transition_type, grantee, role) in links {
        match transition_type {
            CodingSessionAuthorityTransitionType::GrantSeat => {
                let Some(role) = role else { continue };
                seats.retain(|seat| seat.actor_pubkey != grantee);
                seats.push(CodingSessionTeamActiveSeat {
                    actor_pubkey: grantee,
                    role,
                });
            }
            CodingSessionAuthorityTransitionType::RevokeSeat => {
                seats.retain(|seat| {
                    seat.actor_pubkey != grantee || Some(&seat.role) != role.as_ref()
                });
            }
            _ => {}
        }
    }
    seats
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
        });
    }

    // ── Arm (C) ──────────────────────────────────────────────────────────
    let mut approved_branch_only = false;
    let mut approved_for_other_ref: Option<String> = None;
    // Near-misses, kept so the refusal can name the nearest missing fact
    // rather than the most generic one. `NoApprovingVerdict` is the answer
    // only when nothing at all named the commit.
    let mut approvals_naming_the_commit: usize = 0;
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
        if !is_founder(query.repo_founders, &candidate.founder_pubkey) {
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
        // A mission counts only when its founder founded the repository.
        // Since finding 33 that is a **set** — signer, maintainers, project
        // owners — not the announcement's signer alone. The relay's genesis
        // query is scoped to the same set, so this cannot fail for it; it is
        // the whole check for the desktop and CLI callers, which assemble a
        // single candidate themselves.
        if !is_founder(query.repo_founders, &candidate.founder_pubkey) {
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
                    });
                }
                Some(_) => {}
                // `branch` never admits: a name is not a commit.
                None if report.branch.is_some() => approved_branch_only = true,
                None => {}
            }
        }
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
    if approvals_naming_the_commit > 0 {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedButNotVerified {
            new_oid: query.new_oid.to_ascii_lowercase(),
            approvals: approvals_naming_the_commit,
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
    VerdictAdmission::Refused(VerdictAdmissionRefusal::NoApprovingVerdict {
        new_oid: query.new_oid.to_ascii_lowercase(),
        searched_sessions: candidates.len(),
    })
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
