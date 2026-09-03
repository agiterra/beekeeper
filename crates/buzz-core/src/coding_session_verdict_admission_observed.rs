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
//!    founder-signed kind 44245 sets `gates.verifierRequired` false, or sets
//!    no policy at all. A founder who asked for a second seat gets one; green
//!    gates are not one.
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

use super::{VerdictAdmissionCandidate, VerdictAdmissionEvidence, VerdictAdmissionRefusal};

/// Newest kind 44246 observations one verdict-gated ref update may read.
///
/// Read as one page across the bound channel, split into missions in memory,
/// exactly as the team transactions are and for the same reason: 44246 is not
/// addressable, so its umbrella label is not a queryable column. Past this
/// bound a push can only get *fewer* rows and so be refused where it might
/// have been admitted — the direction a gate must fail in.
pub const VERDICT_ADMISSION_MAX_OBSERVATIONS: usize = 512;

/// Newest kind 44245 policies one verdict-gated ref update may read.
///
/// Scoped to the repository's founders, so this is "the newest 64 policies
/// the founders published on this channel". A mission whose policy falls
/// outside it is judged as though the founder set no flag — which can only
/// *open* arm (B), never close it, so the bound is disclosed in the refusal
/// rather than hidden: a founder who set `verifierRequired` and finds it
/// unread has a stale page, not a lost ruling.
pub const VERDICT_ADMISSION_MAX_POLICIES: usize = 64;

/// Newest kind 44223 session metadata events one verdict-gated ref update may
/// read, to learn which keys are this mission's providers.
///
/// The signer of an execution's metadata is the provider authority behind that
/// execution — the same set the desktop honours an `observed` claim from. A
/// provider whose metadata falls outside this page is not in the set, so its
/// rows fold down to `declared` and admit nothing: the failure direction is
/// again a refusal, never a wrong admission.
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

/// Judge one candidate under arm (B).
///
/// `Silent` rather than a refusal is the answer whenever this arm has nothing
/// to say, so a mission running arm (C) never has an arm-(B) sentence put in
/// front of its own.
pub(super) fn evaluate_observed_gates(
    candidate: &VerdictAdmissionCandidate,
    new_oid: &str,
    pusher_pubkey: &str,
) -> ObservedGateVerdict {
    if candidate
        .gate_policy
        .as_ref()
        .is_some_and(VerdictAdmissionGatePolicy::requires_a_verifier)
    {
        return ObservedGateVerdict::Silent;
    }
    let required = candidate
        .gate_policy
        .clone()
        .unwrap_or_default()
        .required_gates();

    // Rows this mission holds for this exact commit, whoever signed them.
    // Kept apart from the observed set so the refusal can tell "nobody
    // measured this commit" from "its subject said so about itself".
    let naming: Vec<&CodingSessionObservationGateEntry> = candidate
        .observed_gates
        .iter()
        .filter(|entry| {
            entry
                .row
                .head_sha
                .as_deref()
                .is_some_and(|sha| sha.eq_ignore_ascii_case(new_oid))
        })
        .collect();
    if naming.is_empty() {
        return ObservedGateVerdict::Silent;
    }
    let observed: Vec<&CodingSessionObservationGateEntry> = naming
        .iter()
        .copied()
        .filter(|entry| entry.source == CodingSessionObservationSource::Observed)
        .collect();
    if observed.is_empty() {
        return ObservedGateVerdict::Refuses(VerdictAdmissionRefusal::ObservedRowsAreDeclared {
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
        return ObservedGateVerdict::Refuses(VerdictAdmissionRefusal::ObservedGateRed {
            gate: red.row.gate.clone(),
            new_oid: new_oid.to_ascii_lowercase(),
        });
    }
    if observed.iter().any(|entry| entry.row.dirty == Some(true)) {
        return ObservedGateVerdict::Refuses(VerdictAdmissionRefusal::ObservedDirty {
            new_oid: new_oid.to_ascii_lowercase(),
        });
    }

    let mut event_ids: Vec<String> = Vec::with_capacity(required.len());
    for gate in &required {
        let Some(entry) = observed.iter().find(|entry| {
            &entry.row.gate == gate
                && entry.row.outcome == CodingSessionObservationGateOutcome::Passed
                && entry.row.dirty == Some(false)
        }) else {
            return ObservedGateVerdict::Refuses(
                VerdictAdmissionRefusal::RequiredGateNotObserved {
                    gate: gate.clone(),
                    new_oid: new_oid.to_ascii_lowercase(),
                    required: required.clone(),
                },
            );
        };
        // The newest observation naming this gate — the one the fold shows.
        if let Some(id) = entry.event_ids.last() {
            event_ids.push(id.clone());
        }
    }

    if !candidate
        .active_seats
        .iter()
        .any(|seat| seat.actor_pubkey.eq_ignore_ascii_case(pusher_pubkey))
    {
        return ObservedGateVerdict::Refuses(VerdictAdmissionRefusal::PushNotSeated {
            new_oid: new_oid.to_ascii_lowercase(),
            session_ref: candidate.session_ref.clone(),
            seats: candidate.active_seats.len(),
        });
    }

    ObservedGateVerdict::Admits(VerdictAdmissionEvidence::ObservedGates {
        session_ref: candidate.session_ref.clone(),
        head_sha: new_oid.to_ascii_lowercase(),
        gates: required,
        row_event_ids: event_ids,
    })
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

/// The gate half of the newest founder-signed kind 44245 policy for one
/// mission.
///
/// `policies` is the caller's page, **newest first**; the first event that
/// belongs to this mission and decodes is the answer, and one that does not
/// decode is skipped rather than treated as an empty policy — a policy this
/// build cannot read must not silently become "no verifier required".
///
/// `None` means no readable policy, which arm (B) treats as "the founder set
/// no flag". Callers that read no policies at all pass an empty slice and get
/// the same answer, which is why every refusal here names the gate list it
/// used rather than implying the founder chose it.
pub fn mission_gate_policy(
    session_ref: &str,
    genesis_ref: &str,
    policies: &[Event],
) -> Option<VerdictAdmissionGatePolicy> {
    policies
        .iter()
        .filter(|event| {
            super::has_exact_tag(event, "d", session_ref)
                && super::has_exact_tag(event, "csp-genesis", genesis_ref)
        })
        .find_map(|event| {
            let payload = validate_coding_session_policy_envelope(event).ok()?;
            let gates = payload.gates?;
            Some(VerdictAdmissionGatePolicy {
                verifier_required: gates.verifier_required,
                required_gates: gates.required_gates,
            })
        })
}

/// The provider identities behind one mission, from a page of kind 44223
/// session metadata.
///
/// The **signer** of an execution's metadata is that execution's provider
/// authority, which is exactly the set whose `observed` claim a mission
/// honours (REVIEW-L5 F2; the desktop resolves the same set from
/// `execution.signerPubkey`). A metadata event that does not decode, or that
/// claims no umbrella, contributes nothing rather than being guessed at.
///
/// The answer is deliberately a `Vec` and never an `Option`: an empty set is
/// "no provider of this mission published metadata on this page", and folding
/// with `Some(empty)` folds every `observed` claim down to `declared`. That is
/// the fail-closed direction — a push is refused, never wrongly admitted.
pub fn mission_provider_pubkeys(session_ref: &str, metadata: &[Event]) -> Vec<String> {
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
