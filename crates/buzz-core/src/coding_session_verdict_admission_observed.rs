//! Arm **(B)** of the verdict-gated push rule: gate rows a mechanism watched,
//! green on the pushed commit, land a seat's work with no second seat.
//!
//! The velocity arm. Brian's ruling on 2026-09-03 named it first and it could
//! not be built: a kind 44246 gate row carried five keys and none of them was
//! a commit, so the strongest thing a reader could say was *"this mission has
//! green rows somewhere"* — which would let an earlier commit's green admit a
//! later one, the shape of finding 27. The row now carries `headSha` and
//! `dirty` (`crate::coding_session_observation::CodingSessionObservationGateRow`),
//! resolved in the provider process from the seat's own workdir at the moment
//! the gate closed, and this module is what reads them.
//!
//! # What it requires, all of it
//!
//! 1. The mission's policy does **not** require a verifier: the newest
//!    founder-signed kind 44245 sets `gates.verifierRequired` false, sets no
//!    gate half, was withdrawn, or was never published — each a **resolved**
//!    fact ([`GatePolicyResolution`]) the admission evidence discloses. A
//!    newest record this build cannot read closes the arm (finding 89): the
//!    rule refuses with [`VerdictAdmissionRefusal::PolicyUnreadable`] rather
//!    than reading an older record in its place. A founder who asked for a
//!    second seat gets one; green gates are not one.
//! 2. Every **required** gate — the policy's own `gates.requiredGates` when it
//!    names any, else [`DEFAULT_REQUIRED_GATES`] — has a folded row that:
//!    * is `source: observed`, which after
//!      [`crate::coding_session_observation::fold_coding_session_observations`]
//!      means signed by a provider identity of this mission (a seat claiming
//!      the word is folded down to `declared` and cannot reach here);
//!    * names **this commit** in `headSha`, compared whole and case-folded;
//!    * was measured over a **clean** worktree (`dirty: false`), because a run
//!      over a tree the commit does not name is not evidence about that
//!      commit;
//!    * says `passed`.
//! 3. The pusher holds an active seat of that mission — the same condition arm
//!    (C) ends on. Any seat may land what the gates cleared; a stranger
//!    holding the same patch may not.
//!
//! # The same rule, under arm (C)
//!
//! Since the 2026-09-03 follow-up ruling (L27) arm (C) requires this evidence
//! too, minus clause 1: a verifier-required mission needs the verifier's
//! clearance **and** the rows, because proof scaled to risk means the riskier
//! class gets strictly more proof, not different proof. Clause 1 is the only
//! thing dropped, and only because a mission reaching arm (C) has already
//! produced the second seat the flag asked for. [`gates_observed_green`] is
//! that shared half; [`evaluate_observed_gates`] is it plus clause 1 plus the
//! silence a mission with no rows for this commit is owed under arm (B).
//!
//! # What it deliberately will not do
//!
//! * **Absent is not "this commit".** A row signed before `headSha` existed
//!   names no commit and admits nothing. Reading absent as "whatever is being
//!   pushed" would land every commit on the strength of a row that predates
//!   the question — the finding-31 rule has two halves, and this is the
//!   second.
//! * **Declared never counts.** `bee sessions observe gate --head-sha` writes
//!   a real commit onto a real row, and that row is still a claim by its own
//!   subject. It is rendered, it is never evidence here.
//! * **No clock.** The fold reads none
//!   (`coding_session_observation_fold.rs`), so "the row is newer than the
//!   commit" is not available and is not approximated. `headSha` is the
//!   binding, and it is exact.

use nostr::Event;

use crate::coding_session_observation::{
    CodingSessionObservationGateEntry, CodingSessionObservationGateOutcome,
    CodingSessionObservationSource,
};
use crate::coding_session_policy::validate_coding_session_policy_envelope;

use super::{
    VerdictAdmissionCandidate, VerdictAdmissionEvidence, VerdictAdmissionGateRows,
    VerdictAdmissionRefusal,
};

/// Newest kind 44246 observations one verdict-gated ref update may read.
///
/// Read as one page across the bound channel, split into missions in memory,
/// exactly as the team transactions are and for the same reason: 44246 is not
/// addressable, so its umbrella label is not a queryable column. Past this
/// bound a push can only get *fewer* rows and so be refused where it might
/// have been admitted — the direction a gate must fail in.
pub const VERDICT_ADMISSION_MAX_OBSERVATIONS: usize = 512;

/// Newest kind 44245 policies one verdict-gated ref update may read **per
/// mission**.
///
/// Finding 89: this used to bound one page across a whole channel, and a
/// mission whose policy fell outside it was judged as though the founder set
/// no flag — a `verifierRequired: true` the page missed opened arm (B). The
/// page is now read per mission (filtered by `d` and `csp-genesis`), newest
/// first, so the authoritative record is always the first one and the bound
/// only limits how many superseded records travel with it.
/// [`resolve_mission_gate_policy`] classifies that newest record and never
/// skips past it.
pub const VERDICT_ADMISSION_MAX_POLICIES: usize = 64;

/// Newest kind 44223 session metadata events one reader may page through.
///
/// Kept for callers that still render metadata; **not an admission input**
/// since finding 90. A page of 44223 proves who published metadata, which is
/// not who provides the mission — see [`metadata_signers`].
pub const VERDICT_ADMISSION_MAX_PROVIDER_METADATA: usize = 256;

/// The gates a mission must have green when its policy names none.
///
/// The three every lane in this repository runs, under the names
/// `buzz_session_provider::gate_observer` gives them — the observer's table is
/// what mints an `observed` row, so a default naming a gate it cannot match
/// would be a rule nothing could ever satisfy.
pub const DEFAULT_REQUIRED_GATES: &[&str] = &["cargo fmt", "cargo clippy", "cargo test"];

/// The half of a mission's kind 44245 policy this rule reads.
///
/// Deliberately not the whole payload. The push gate has no business in a
/// mission's budget or bench, and a struct carrying them would invite a later
/// reader to gate a landing on one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerdictAdmissionGatePolicy {
    /// `gates.verifierRequired`, exactly as the newest founder-signed policy
    /// set it. `None` is "the founder set no such flag", which is not the same
    /// statement as `Some(false)` but has the same effect here.
    pub verifier_required: Option<bool>,
    /// `gates.requiredGates`, when the policy names any. An empty list is
    /// treated as naming none, so a policy cannot accidentally admit a push
    /// with no gate green at all.
    pub required_gates: Option<Vec<String>>,
}

impl VerdictAdmissionGatePolicy {
    /// Whether this policy forbids arm (B) outright.
    pub fn requires_a_verifier(&self) -> bool {
        self.verifier_required == Some(true)
    }

    /// The gates a push under this policy must have green, in order.
    pub fn required_gates(&self) -> Vec<String> {
        match self.required_gates.as_deref() {
            Some(gates) if !gates.is_empty() => gates.to_vec(),
            _ => DEFAULT_REQUIRED_GATES
                .iter()
                .map(|gate| (*gate).to_owned())
                .collect(),
        }
    }
}

/// What the mission's newest kind 44245 record resolved to (finding 89).
///
/// The rule reads **one** record — the newest that belongs to this mission by
/// both `d` and `csp-genesis` — and says what it is. It never skips to an
/// older one: under NIP-CSP's newest-wins fold the newest record *is* the
/// policy, so an older record standing in for it would be exactly the
/// resurrection finding 89 caught (a valid withdrawal read past to the
/// restrictive policy it withdrew, or an unreadable record read past to a
/// weaker one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatePolicyResolution {
    /// The newest record decodes and this is its gate half — possibly empty,
    /// when the record sets a budget or bench and no gate.
    Present {
        /// The gate half of that record.
        policy: VerdictAdmissionGatePolicy,
        /// The record it was read from.
        event_id: String,
    },
    /// The newest record is a valid withdrawal (`{schema, sessionRef,
    /// genesisRef}`, NIP-CSP rule 3): somebody took the policy back, and the
    /// defaults apply because they chose that.
    Withdrawn {
        /// The withdrawal record.
        event_id: String,
    },
    /// No record names this mission. The defaults apply because nothing was
    /// ever set — disclosed as such, since "nobody set a policy" and "the
    /// founder withdrew it" are different facts.
    Absent,
    /// The newest record exists and this build cannot read it. Nothing is
    /// known about the policy, and the rule refuses rather than guessing.
    Unreadable {
        /// The record that could not be read.
        event_id: String,
        /// The validator's own sentence about why.
        reason: String,
    },
}

impl GatePolicyResolution {
    /// Whether the resolved policy forbids arm (B) outright.
    ///
    /// `Unreadable` answers `false` here **and is never asked**: every caller
    /// checks [`GatePolicyResolution::unreadable`] first, because an
    /// unreadable policy closes both arms rather than opening one.
    pub fn requires_a_verifier(&self) -> bool {
        matches!(self, Self::Present { policy, .. } if policy.requires_a_verifier())
    }

    /// The gates a push under this resolution must have green, in order.
    ///
    /// Defaults for `Withdrawn` and `Absent`; the record's own list for
    /// `Present`. `Unreadable` also answers defaults, and again is never
    /// asked — see [`GatePolicyResolution::requires_a_verifier`].
    pub fn required_gates(&self) -> Vec<String> {
        match self {
            Self::Present { policy, .. } => policy.required_gates(),
            Self::Withdrawn { .. } | Self::Absent | Self::Unreadable { .. } => {
                VerdictAdmissionGatePolicy::default().required_gates()
            }
        }
    }

    /// The record id and reason when the newest record could not be read.
    pub fn unreadable(&self) -> Option<(&str, &str)> {
        match self {
            Self::Unreadable { event_id, reason } => Some((event_id, reason)),
            _ => None,
        }
    }

    /// What an admission discloses about the policy it consulted.
    ///
    /// `None` for `Unreadable`: nothing admits under a policy nobody read, so
    /// there is no admission to carry it.
    ///
    /// `excluded_unauthorized` is the caller's own count of records naming
    /// this mission that the signer rule dropped (S2). It is a parameter and
    /// not a field of the resolution because a resolution is what **one**
    /// record said, while what other records were discarded is a fact about
    /// the read that produced it.
    pub fn evidence(&self, excluded_unauthorized: u32) -> Option<VerdictAdmissionPolicyEvidence> {
        match self {
            Self::Present { event_id, .. } => Some(VerdictAdmissionPolicyEvidence {
                event_id: Some(event_id.clone()),
                resolution: VerdictAdmissionPolicyResolution::Present,
                excluded_unauthorized,
            }),
            Self::Withdrawn { event_id } => Some(VerdictAdmissionPolicyEvidence {
                event_id: Some(event_id.clone()),
                resolution: VerdictAdmissionPolicyResolution::Withdrawn,
                excluded_unauthorized,
            }),
            Self::Absent => Some(VerdictAdmissionPolicyEvidence {
                event_id: None,
                resolution: VerdictAdmissionPolicyResolution::Absent,
                excluded_unauthorized,
            }),
            Self::Unreadable { .. } => None,
        }
    }
}

/// The policy an admission stood on, as its evidence carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictAdmissionPolicyEvidence {
    /// The kind 44245 record that was read, or `None` when none named the
    /// mission.
    pub event_id: Option<String>,
    /// How that record resolved.
    pub resolution: VerdictAdmissionPolicyResolution,
    /// How many records naming this mission the signer rule excluded
    /// (`excludedUnauthorized` on the wire; 2026-09-05 refuter, S2).
    ///
    /// An admission that consulted a policy says how many *other* records
    /// claimed to be that policy and were not signed by anyone entitled to
    /// set it. Zero is the ordinary case; a non-zero count is worth a
    /// sentence, and the silence it replaces is what the audit's policy-signer
    /// parity test asked to end.
    pub excluded_unauthorized: u32,
}

/// The three resolutions an admission can stand on. `Unreadable` is absent by
/// construction: it admits nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictAdmissionPolicyResolution {
    /// The newest record set a policy.
    Present,
    /// The newest record withdrew the policy; defaults applied by choice.
    Withdrawn,
    /// No record named the mission; defaults applied by default.
    Absent,
}

impl VerdictAdmissionPolicyResolution {
    /// The wire token for this resolution.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Withdrawn => "withdrawn",
            Self::Absent => "absent",
        }
    }
}

/// Why arm (A)'s evidence carries no policy at all.
///
/// The founder exception is deliberate (Brian, 2026-09-03: humans never gate
/// a landing), and the audit's ask was that a founder's landing never read as
/// verifier-approved. This token is that disclosure: the policy was not
/// evaluated, and the evidence says which exception skipped it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictAdmissionPolicyNotEvaluated {
    /// The pusher is a founder of the repository; no mission was read.
    FounderException,
}

impl VerdictAdmissionPolicyNotEvaluated {
    /// The wire token for this exception.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FounderException => "founder_exception",
        }
    }
}

/// What arm (B) concluded for one candidate.
#[derive(Debug, Clone)]
pub(super) enum ObservedGateVerdict {
    /// Every required gate was observed green on this commit, over a clean
    /// tree, and the pusher is seated.
    Admits(VerdictAdmissionEvidence),
    /// Something specific was missing, and this names it.
    Refuses(VerdictAdmissionRefusal),
    /// This arm does not apply to this candidate at all — the policy requires
    /// a verifier, or no row here mentions this commit in any form.
    Silent,
}

/// What [`gates_observed_green`] found when it found it: the gates it required,
/// in order, and the newest observation event behind each.
pub(super) struct ObservedGreenGates {
    /// The gates that had to be green, in the order the policy required them.
    pub gates: Vec<String>,
    /// The newest observation event id behind each of those gates, in the same
    /// order, so a reader can go and read the rows themselves.
    pub row_event_ids: Vec<String>,
}

/// Judge one candidate under arm (B).
///
/// `Silent` rather than a refusal is the answer whenever this arm has nothing
/// to say, so a mission running arm (C) never has an arm-(B) sentence put in
/// front of its own. The two things that silence this arm are exactly the two
/// arm (C) does **not** inherit when it asks the same question
/// ([`gates_observed_green`]): a founder-signed `gates.verifierRequired`, and
/// a commit no row mentions at all.
pub(super) fn evaluate_observed_gates(
    candidate: &VerdictAdmissionCandidate,
    new_oid: &str,
    pusher_pubkey: &str,
) -> ObservedGateVerdict {
    // Finding 89: a policy this build cannot read is refused, never read
    // past. It is asked before anything else because every later question
    // — does it require a verifier, which gates does it name — is about the
    // record nobody could read.
    if let Some(refusal) = policy_unreadable(candidate) {
        return ObservedGateVerdict::Refuses(refusal);
    }
    if candidate.gate_policy.requires_a_verifier() {
        return ObservedGateVerdict::Silent;
    }
    if rows_naming(candidate, new_oid).is_empty() {
        return ObservedGateVerdict::Silent;
    }
    // Resolved above: an unreadable policy already returned.
    let Some(policy) = candidate
        .gate_policy
        .evidence(candidate.excluded_unauthorized_policies)
    else {
        return ObservedGateVerdict::Silent;
    };
    match gates_observed_green(candidate, new_oid, pusher_pubkey) {
        Ok(green) => ObservedGateVerdict::Admits(VerdictAdmissionEvidence::ObservedGates {
            session_ref: candidate.session_ref.clone(),
            head_sha: new_oid.to_ascii_lowercase(),
            gates: green.gates,
            row_event_ids: green.row_event_ids,
            policy,
        }),
        Err(refusal) => ObservedGateVerdict::Refuses(refusal),
    }
}

/// The refusal an unreadable policy earns, on any arm.
///
/// Both arms read the policy — (B) for its verifier flag and its gate list,
/// (C) for the gate list alone — so a record nobody can read closes both.
/// Reading an older record instead is what finding 89 caught.
pub(super) fn policy_unreadable(
    candidate: &VerdictAdmissionCandidate,
) -> Option<VerdictAdmissionRefusal> {
    candidate
        .gate_policy
        .unreadable()
        .map(
            |(event_id, reason)| VerdictAdmissionRefusal::PolicyUnreadable {
                session_ref: candidate.session_ref.clone(),
                event_id: event_id.to_owned(),
                reason: reason.to_owned(),
            },
        )
}

/// Arm (B)'s whole rule **minus the "policy must not require a verifier"
/// clause** — the half arm (C) requires on top of a verifier's clearance.
///
/// # Why arm (C) asks this at all
///
/// L22 §6.3 left it open and the 2026-09-03 follow-up ruling (L27) closed it:
/// **(C) = (B)'s rows + the verifier's clearance.** Proof is scaled to risk,
/// and scaling means the riskier class gets strictly *more* proof, not
/// different proof. Until this existed a founder who set
/// `gates.verifierRequired: true` made their mission the **weaker** of the two
/// arms — (B) wanted three green gates on the pushed commit and (C) wanted
/// none — which is precisely backwards. The rows cost nothing to require: the
/// provider signs them whether or not anyone reads them.
///
/// The verifier clause is the one thing dropped, and it is dropped because
/// under arm (C) it is already satisfied by construction: the caller reaches
/// this only after finding the verifier's `not-refuted` refutation.
///
/// Silence is not an answer here either. A commit **no row names** is
/// `RequiredGateNotObserved` naming the whole list, not a shrug: arm (C) has
/// already decided the gates are owed, so "nobody measured it" is the refusal
/// rather than a reason to stop asking.
pub(super) fn gates_observed_green(
    candidate: &VerdictAdmissionCandidate,
    new_oid: &str,
    pusher_pubkey: &str,
) -> Result<ObservedGreenGates, VerdictAdmissionRefusal> {
    let green = rows_observed_green(candidate, new_oid)?;
    if !candidate
        .active_seats
        .iter()
        .any(|seat| seat.actor_pubkey.eq_ignore_ascii_case(pusher_pubkey))
    {
        return Err(VerdictAdmissionRefusal::PushNotSeated {
            new_oid: new_oid.to_ascii_lowercase(),
            session_ref: candidate.session_ref.clone(),
            seats: candidate.active_seats.len(),
        });
    }
    Ok(green)
}

/// Arm (B)'s status on one commit, as an arm-(C) refusal carries it.
///
/// Finding 75 (live run 6): a refusal that says "no verifier cleared it" over
/// a mission whose policy never asked for one sends the pusher after the
/// wrong route. Every arm-(C) refusal that follows an arm-(B) evaluation now
/// carries what arm (B) found on the pushed commit, so the sentence can lead
/// with the route the policy actually runs. The seat is deliberately not part
/// of this: it is a fact about the key, not about the rows, and the composed
/// refusal names it separately.
pub(super) fn gate_rows_status(
    candidate: &VerdictAdmissionCandidate,
    new_oid: &str,
) -> VerdictAdmissionGateRows {
    match rows_observed_green(candidate, new_oid) {
        Ok(_) => VerdictAdmissionGateRows::Green,
        Err(refusal) => VerdictAdmissionGateRows::Short(Box::new(refusal)),
    }
}

/// The rows half of [`gates_observed_green`]: every required gate observed
/// green on `new_oid` over a clean worktree, whoever is pushing.
///
/// Split from the seat clause so a refusal on another arm can report the row
/// status truthfully for a pusher who is not seated at all — the seat is
/// asked separately, and a `PushNotSeated` here would read as "the rows are
/// fine" to a caller that never asked about the key.
fn rows_observed_green(
    candidate: &VerdictAdmissionCandidate,
    new_oid: &str,
) -> Result<ObservedGreenGates, VerdictAdmissionRefusal> {
    if let Some(refusal) = policy_unreadable(candidate) {
        return Err(refusal);
    }
    let required = candidate.gate_policy.required_gates();

    // Rows this mission holds for this exact commit, whoever signed them.
    // Kept apart from the observed set so the refusal can tell "nobody
    // measured this commit" from "its subject said so about itself".
    let naming = rows_naming(candidate, new_oid);
    let observed: Vec<&CodingSessionObservationGateEntry> = naming
        .iter()
        .copied()
        .filter(|entry| entry.source == CodingSessionObservationSource::Observed)
        .collect();
    if observed.is_empty() {
        // Nothing observed names it. Which sentence that earns depends on
        // whether anything named it at all — "its own subject said so" and
        // "nobody measured this commit" are different facts, and a `rows: 0`
        // declared-rows sentence would be the second dressed as the first.
        if naming.is_empty() {
            return Err(no_row_names_it(&required, new_oid));
        }
        return Err(VerdictAdmissionRefusal::ObservedRowsAreDeclared {
            new_oid: new_oid.to_ascii_lowercase(),
            rows: naming.len(),
        });
    }

    // A red row is named before a missing one: a gate that ran and failed is a
    // fact about this commit, and a gate that never ran is only an absence.
    if let Some(red) = observed
        .iter()
        .find(|entry| entry.row.outcome == CodingSessionObservationGateOutcome::Failed)
    {
        return Err(VerdictAdmissionRefusal::ObservedGateRed {
            gate: red.row.gate.clone(),
            new_oid: new_oid.to_ascii_lowercase(),
        });
    }
    if observed.iter().any(|entry| entry.row.dirty == Some(true)) {
        return Err(VerdictAdmissionRefusal::ObservedDirty {
            new_oid: new_oid.to_ascii_lowercase(),
        });
    }

    let mut row_event_ids: Vec<String> = Vec::with_capacity(required.len());
    for gate in &required {
        let Some(entry) = observed.iter().find(|entry| {
            &entry.row.gate == gate
                && entry.row.outcome == CodingSessionObservationGateOutcome::Passed
                && entry.row.dirty == Some(false)
        }) else {
            return Err(VerdictAdmissionRefusal::RequiredGateNotObserved {
                gate: gate.clone(),
                new_oid: new_oid.to_ascii_lowercase(),
                required: required.clone(),
            });
        };
        // The newest observation naming this gate — the one the fold shows.
        if let Some(id) = entry.event_ids.last() {
            row_event_ids.push(id.clone());
        }
    }

    Ok(ObservedGreenGates {
        gates: required,
        row_event_ids,
    })
}

/// Every folded row of this mission that names `new_oid`, whoever signed it.
fn rows_naming<'a>(
    candidate: &'a VerdictAdmissionCandidate,
    new_oid: &str,
) -> Vec<&'a CodingSessionObservationGateEntry> {
    candidate
        .observed_gates
        .iter()
        .filter(|entry| {
            entry
                .row
                .head_sha
                .as_deref()
                .is_some_and(|sha| sha.eq_ignore_ascii_case(new_oid))
        })
        .collect()
}

/// The refusal for a commit **no** gate row names, under a rule that requires
/// them.
///
/// The first required gate carries the sentence and the whole list is named
/// beside it, so a reader learns what is owed rather than only what is
/// missing. [`VerdictAdmissionGatePolicy::required_gates`] never returns an
/// empty list — an empty `requiredGates` falls back to
/// [`DEFAULT_REQUIRED_GATES`] — and the fallback here says so out loud rather
/// than printing an empty gate name if that ever stops being true.
fn no_row_names_it(required: &[String], new_oid: &str) -> VerdictAdmissionRefusal {
    let gate = required
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_REQUIRED_GATES[0].to_owned());
    VerdictAdmissionRefusal::RequiredGateNotObserved {
        gate,
        new_oid: new_oid.to_ascii_lowercase(),
        required: required.to_vec(),
    }
}

/// The kind 44246 observations belonging to one mission, out of a page read
/// for a whole channel.
///
/// The same both-tags rule [`super::mission_transactions`] applies, over kind
/// 44246's own genesis tag: the umbrella label `d` is re-usable and the
/// genesis is not, so a record belongs to a mission only when both agree.
pub fn mission_observations<'a>(
    session_ref: &str,
    genesis_ref: &str,
    observations: &'a [Event],
) -> Vec<&'a Event> {
    observations
        .iter()
        .filter(|event| {
            super::has_exact_tag(event, "d", session_ref)
                && super::has_exact_tag(event, "csob-genesis", genesis_ref)
        })
        .collect()
}

/// Resolve the newest kind 44245 record of one mission (finding 89).
///
/// `policies` is the caller's page in any order; the newest record that
/// belongs to this mission — by `d` **and** `csp-genesis`, the same both-tags
/// rule every other kind here applies — is chosen by `created_at`, ties by
/// the larger event id (the desktop fold's `isNewer`). That one record is
/// classified and returned; an older one is never consulted.
///
/// Who may sign a policy (the founder, or a seat holding an operator grant
/// at the record's `created_at` — NIP-CSP § Validation boundary) is the
/// caller's filter, applied before the page reaches here: this crate cannot
/// read the authority chain, and a rule that pretended to would be asserting
/// standing it cannot verify.
pub fn resolve_mission_gate_policy(
    session_ref: &str,
    genesis_ref: &str,
    policies: &[Event],
) -> GatePolicyResolution {
    let newest = policies
        .iter()
        .filter(|event| {
            super::has_exact_tag(event, "d", session_ref)
                && super::has_exact_tag(event, "csp-genesis", genesis_ref)
        })
        .max_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.to_hex().cmp(&right.id.to_hex()))
        });
    let Some(event) = newest else {
        return GatePolicyResolution::Absent;
    };
    let event_id = event.id.to_hex();
    match validate_coding_session_policy_envelope(event) {
        Err(reason) => GatePolicyResolution::Unreadable { event_id, reason },
        Ok(payload) => match payload.gates {
            Some(gates) => GatePolicyResolution::Present {
                policy: VerdictAdmissionGatePolicy {
                    verifier_required: gates.verifier_required,
                    required_gates: gates.required_gates,
                },
                event_id,
            },
            // A record that sets a budget or a bench and no gate half is a
            // policy whose gate half is unset — present, with the defaults.
            None if payload.sets_any_policy() => GatePolicyResolution::Present {
                policy: VerdictAdmissionGatePolicy::default(),
                event_id,
            },
            None => GatePolicyResolution::Withdrawn { event_id },
        },
    }
}

/// The keys that signed kind 44223 metadata naming one mission.
///
/// **Not authority, and nothing in this crate may feed it to admission**
/// (finding 90). This used to be `mission_provider_pubkeys`: the signer of a
/// decodable metadata event naming mission M joined M's trusted provider set,
/// so any channel member could publish a 44223 for M, sign `observed` green
/// rows, and have arm (B) consume them. Publishing metadata proves who spoke;
/// it does not prove the speaker was commissioned to run the mission. The
/// provider set is [`mission_provider_pubkeys_from_lifecycle`] now, proven by
/// the accepted lifecycle command and the receipt its named provider signed.
///
/// Kept, renamed and deprecated, so a reader who needs "who published
/// metadata" still has it and a reader who needs "who provides this mission"
/// cannot reach for it by the old name.
#[deprecated(
    since = "0.1.0",
    note = "metadata signers are not mission providers (finding 90); use \
            `mission_provider_pubkeys_from_lifecycle` for admission"
)]
pub fn metadata_signers(session_ref: &str, metadata: &[Event]) -> Vec<String> {
    let mut pubkeys: Vec<String> = Vec::new();
    for event in metadata {
        let Ok(payload) =
            crate::coding_session_payload::decode_coding_session_metadata(&event.content)
        else {
            continue;
        };
        if payload.session_ref.as_deref() != Some(session_ref) {
            continue;
        }
        let pubkey = event.pubkey.to_hex();
        if !pubkeys.iter().any(|held| held == &pubkey) {
            pubkeys.push(pubkey);
        }
    }
    pubkeys
}
