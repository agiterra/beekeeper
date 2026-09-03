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
//! An update `(ref, old, new)` on a ref carrying the `require-verdict`
//! protection rule ([`crate::git_perms::ProtectionRule::require_verdict`]) is
//! admitted when **all** of:
//!
//! 1. some kind 44244 `verdict` of subtype `disposition` whose
//!    [`CodingSessionTeamDispositionDecision::is_approval`] holds is
//!    **canonical in the session's fold**
//!    ([`fold_coding_session_team_transactions`], never raw events), and its
//!    `reportRef` resolves to a canonical `report` whose **`headSha`** equals
//!    `new` — compared whole and case-folded, so a report naming a 64-hex id
//!    names a different object and does not admit;
//! 2. that session's genesis is on the channel the repository is bound to and
//!    its founder is the repository owner (the caller resolves both and
//!    supplies only candidates that pass);
//! 3. the pusher is that founder — or, once
//!    [`VerdictAdmissionRules::seat_may_push`] is enabled, an active seat of
//!    the umbrella. **The reservation is the relay's rule, not the mission's:**
//!    no session policy is read, and the refusal says whose rule refused;
//! 3b. the approving report's `branch`, when it names one, is the branch being
//!    pushed — an approval of a commit *for `main`* does not admit that commit
//!    onto `release`;
//! 4. the ruling is founder-signed — or, once
//!    [`VerdictAdmissionRules::lead_disposition_admits`] is enabled, signed by
//!    anyone the fold accepted as a disposition author.
//!
//! **The rule only ever subtracts.** A ref without the protection rule is
//! evaluated exactly as before; nothing here can admit a push the ordinary
//! role check already denied.
//!
//! Two records that look like they should admit and never do:
//!
//! - a report's **`branch`** — a name is not a commit, and two pushes to one
//!   branch are two commits of which one was ruled on;
//! - a completion's **`landedShas`** — that record is the lead's own claim
//!   that it landed something, which is exactly the claim finding 27 caught
//!   being false.

use nostr::Event;

use crate::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
};
use crate::coding_session_team_transaction::{
    fold_coding_session_team_transactions, validate_coding_session_team_transaction_envelope,
    CodingSessionTeamActiveSeat, CodingSessionTeamFold, CodingSessionTeamFoldContext,
    CodingSessionTeamReport, CodingSessionTeamTransactionBody, CodingSessionTeamTransactionPayload,
    CodingSessionTeamVerdict,
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

/// Which relaxations of the founder-only default are switched on.
///
/// Both default to `false` — batch 3's ruling: while a verdict-gated push has
/// never been exercised live, only the founder pushes such a ref and only the
/// founder's ruling admits one. Both paths are implemented and tested; making
/// either live is a one-field change here plus the caller that constructs it,
/// not new code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerdictAdmissionRules {
    /// Whether a canonical disposition signed by someone other than the
    /// founder (an active `lead`, an operator with steering standing) admits.
    pub lead_disposition_admits: bool,
    /// Whether an active seat of the umbrella may land a verdict-gated ref.
    ///
    /// **This is the relay's rule, not the mission's.** An earlier draft
    /// carried a `policy_reserves_push` field meant to read the session
    /// policy's `irreversible` list; nothing ever populated it, so the
    /// refusal a person saw attributed the reservation to a record the relay
    /// never fetched. The field is gone. Letting a mission policy decide is a
    /// change here *and* in the copy, not a field nobody reads.
    pub seat_may_push: bool,
}

impl VerdictAdmissionRules {
    /// The shipped rule: a founder-signed ruling, landed by the founder.
    pub const FOUNDER_ONLY: Self = Self {
        lead_disposition_admits: false,
        seat_may_push: false,
    };
}

impl Default for VerdictAdmissionRules {
    fn default() -> Self {
        Self::FOUNDER_ONLY
    }
}

/// The exact update being judged.
#[derive(Debug, Clone, Copy)]
pub struct VerdictAdmissionQuery<'a> {
    /// Full ref name, e.g. `refs/heads/main`.
    pub ref_name: &'a str,
    /// The object id the push would leave on that ref.
    pub new_oid: &'a str,
    /// Hex pubkey the push authenticated as.
    pub pusher_pubkey: &'a str,
    /// Hex pubkey of the kind:30617 announcement's author.
    pub repo_owner_pubkey: &'a str,
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
    /// Active seat pubkeys of this umbrella. Read only when
    /// [`VerdictAdmissionRules::seat_may_push`] is on.
    pub active_seat_pubkeys: Vec<String>,
}

/// What admitted a commit, for the log line and the CLI's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictAdmissionEvidence {
    /// Umbrella whose fold admitted it.
    pub session_ref: String,
    /// The approving disposition.
    pub disposition_event_id: String,
    /// The report it governs.
    pub report_event_id: String,
    /// That report's `headSha`, as published.
    pub head_sha: String,
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
    /// The commit is approved and this key may not be the one to land it.
    ApprovedButPushReserved {
        /// The approved object id.
        new_oid: String,
    },
    /// The commit is approved, but for a different branch than this ref.
    ApprovedForAnotherRef {
        /// The approved object id.
        new_oid: String,
        /// The branch the approved report named.
        approved_branch: String,
    },
    /// The rule is set on a repository bound to no channel at all.
    RepositoryUnbound,
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
            } => format!(
                "require-verdict is set and no mission verdict names this commit: no approved \
                 report names {new_oid}. Searched {searched_sessions} mission(s) — the newest \
                 {VERDICT_ADMISSION_MAX_SESSIONS} on this channel whose founder owns this \
                 repository — over one shared page of the newest \
                 {VERDICT_ADMISSION_MAX_TRANSACTIONS} team transactions on that channel. An \
                 older ruling can fall outside both."
            ),
            Self::ApprovedReportNamesBranchOnly => "require-verdict is set and no mission verdict \
                 names this commit: the approved report for this work names a branch and no \
                 headSha, and a branch name is not a commit."
                .to_string(),
            Self::ApprovedButPushReserved { new_oid } => format!(
                "commit {new_oid} is approved, but the relay's require-verdict rule reserves a \
                 gated ref to the founder (founder-only pushes). Ask the founder to land it."
            ),
            Self::ApprovedForAnotherRef {
                new_oid,
                approved_branch,
            } => format!(
                "commit {new_oid} is approved for {approved_branch}, and this is a different \
                 ref. A verdict admits a commit to the branch its report named."
            ),
            Self::RepositoryUnbound => "require-verdict is set and this repository is bound to no \
                 channel, so no mission verdict can be read here. Remove the rule, or bind the \
                 repository to the mission's channel."
                .to_string(),
        }
    }
}

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
        // This caller has not read the session's policy set, and
        // `CodingSessionTeamFoldContext::verifier_required` says such a caller
        // must pass `false`: the fold then behaves exactly as it did before the
        // flag existed. It is honest about what this fold enforces — nothing
        // extra — not about what the session requires. The push gate does not
        // read `gates.verifierRequired`, and no refusal here may be read as
        // "no verifier is required".
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
fn has_exact_tag(event: &Event, name: &str, value: &str) -> bool {
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

/// Judge one ref update against every candidate mission, newest first.
///
/// Stops at the first mission that admits. `candidates` is the bounded set the
/// caller resolved (at most [`VERDICT_ADMISSION_MAX_SESSIONS`]); its length is
/// what the refusal discloses as searched.
pub fn evaluate_verdict_admission(
    candidates: &[VerdictAdmissionCandidate],
    query: &VerdictAdmissionQuery<'_>,
    rules: &VerdictAdmissionRules,
) -> VerdictAdmission {
    let mut approved_branch_only = false;
    let mut approved_for_other_ref: Option<String> = None;

    for candidate in candidates {
        // Belt and braces. Both shipped callers set `founder_pubkey` to the
        // repository owner they queried by, so this cannot fail for them —
        // the real enforcement is the author filter on the genesis query. It
        // stays for a future caller that assembles candidates differently.
        if !eq_hex(&candidate.founder_pubkey, query.repo_owner_pubkey) {
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
            if !rules.lead_disposition_admits
                && !eq_hex(&record.author_pubkey, &candidate.founder_pubkey)
            {
                continue;
            }
            let Some(report) = canonical_report(candidate, report_ref) else {
                continue;
            };
            match report.head_sha.as_deref() {
                Some(head_sha) if eq_hex(head_sha, query.new_oid) => {
                    // Condition 3b: an approval is scoped to the branch its
                    // report named. Approving a commit *for `main`* is not
                    // approval to put it on `release`, nor to roll `main` back
                    // to it from somewhere else. A report naming no branch
                    // scopes nothing — it says only "this commit is good".
                    match report.branch.as_deref() {
                        Some(branch) if !ref_names_branch(query.ref_name, branch) => {
                            approved_for_other_ref.get_or_insert_with(|| branch.to_string());
                        }
                        _ => {
                            return admit_or_reserve(
                                candidate, record, report_ref, head_sha, query, rules,
                            );
                        }
                    }
                }
                Some(_) => {}
                // `branch` never admits: a name is not a commit.
                None if report.branch.is_some() => approved_branch_only = true,
                None => {}
            }
        }
    }

    if let Some(approved_branch) = approved_for_other_ref {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedForAnotherRef {
            new_oid: query.new_oid.to_ascii_lowercase(),
            approved_branch,
        });
    }
    if approved_branch_only {
        return VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedReportNamesBranchOnly);
    }
    VerdictAdmission::Refused(VerdictAdmissionRefusal::NoApprovingVerdict {
        new_oid: query.new_oid.to_ascii_lowercase(),
        searched_sessions: candidates.len(),
    })
}

/// Condition 3: an approved commit still needs a key allowed to land it.
fn admit_or_reserve(
    candidate: &VerdictAdmissionCandidate,
    disposition: &VerdictAdmissionRecord,
    report_ref: &str,
    head_sha: &str,
    query: &VerdictAdmissionQuery<'_>,
    rules: &VerdictAdmissionRules,
) -> VerdictAdmission {
    let pusher_is_founder = eq_hex(query.pusher_pubkey, &candidate.founder_pubkey);
    let seat_may_land = rules.seat_may_push
        && candidate
            .active_seat_pubkeys
            .iter()
            .any(|seat| eq_hex(seat, query.pusher_pubkey));
    if pusher_is_founder || seat_may_land {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence {
            session_ref: candidate.session_ref.clone(),
            disposition_event_id: disposition.event_id.clone(),
            report_event_id: report_ref.to_string(),
            head_sha: head_sha.to_string(),
        })
    } else {
        VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedButPushReserved {
            new_oid: query.new_oid.to_ascii_lowercase(),
        })
    }
}

/// The canonical report a disposition governs, or `None` when the fold did not
/// keep one under that id.
fn canonical_report<'a>(
    candidate: &'a VerdictAdmissionCandidate,
    report_ref: &str,
) -> Option<&'a CodingSessionTeamReport> {
    candidate
        .canonical
        .iter()
        .find(|record| record.event_id == report_ref)
        .and_then(|record| match &record.payload.body {
            CodingSessionTeamTransactionBody::Report(report) => Some(report),
            _ => None,
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

#[cfg(test)]
#[path = "coding_session_verdict_admission_tests.rs"]
mod tests;
