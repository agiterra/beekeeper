//! Which published kind-44245 record is *the* policy for an umbrella, and why
//! every other one is not.
//!
//! The relay stores a structurally valid 44245 from **any** channel member, by
//! design: NIP-CSP's validation boundary says the relay checks shape and the
//! *consumer* adjudicates standing. This module is that adjudication, written
//! once so every consumer gives the same answer.
//!
//! It exists because they did not. Batch 2 lane B2 shipped two readers of the
//! same records — the session provider's context projection and
//! `bee sessions policy get` — and only one of them folded authority. A
//! stranger could publish `budget.turns: 9999` into a channel it belonged to
//! and the CLI would print it, with an author and an event id, as "the newest
//! accepted policy", while the provider correctly ignored it. A ceiling shown
//! to a person that nothing is counting is the same defect as a status that
//! reads Idle over a disconnected provider (REVIEW-B2 F1).
//!
//! # Rejected records are listed, never dropped
//!
//! A record that fails the check is returned in
//! [`CodingSessionPolicyFold::excluded`] with the reason. Silence would let a
//! stranger's record — or a malformed one — be indistinguishable from "nobody
//! set a policy", and those are different facts.

use nostr::Event;
use serde::{Deserialize, Serialize};

use crate::coding_session_authority_transition::CodingSessionAuthorityTransitionType;
use crate::coding_session_context::CodingSessionContextPolicy;
use crate::coding_session_policy::validate_coding_session_policy_envelope;

/// One accepted authority transition, reduced to what the policy fold needs.
///
/// `accepted_at` is the **relay acceptance receipt's** `created_at`, not the
/// transition's own, because acceptance is what put the grant into the
/// canonical chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionPolicyGrant {
    /// Pubkey the transition grants or revokes.
    pub grantee: String,
    /// Unix seconds at which the relay accepted the transition.
    pub accepted_at: u64,
    /// What the transition did.
    pub transition_type: CodingSessionAuthorityTransitionType,
}

/// Whether `signer` could steer the umbrella at `created_at`.
///
/// The founder always could. Anybody else needs a `grant-operator` that the
/// relay had already accepted by then and that nothing has since revoked or
/// downgraded — **evaluated at the record's own time**, so a policy signed
/// while a grant stood is not retroactively invalidated by a later revoke, and
/// a policy signed before a grant existed is not retroactively blessed by it.
///
/// A `lead` role slug is deliberately not enough, and is not even an input: a
/// provider can prove a grant from the accepted NIP-CSAT chain and cannot
/// prove a role slug it did not mint.
pub fn signer_may_steer_at(
    signer: &str,
    created_at: u64,
    founder: &str,
    grants: &[CodingSessionPolicyGrant],
) -> bool {
    if signer == founder {
        return true;
    }
    let mut active = false;
    for grant in grants
        .iter()
        .filter(|grant| grant.grantee == signer && grant.accepted_at <= created_at)
    {
        match grant.transition_type {
            CodingSessionAuthorityTransitionType::GrantOperator => active = true,
            CodingSessionAuthorityTransitionType::GrantViewer
            | CodingSessionAuthorityTransitionType::Revoke => active = false,
            // A seat is not a steering grant, and neither is a claim: a
            // `takeover` says who is *carrying* the work, which the fence
            // answers, and letting it grant steering here would make a claim a
            // back door into policy authorship.
            // A project-action delegation is narrower still: it says the
            // grantee may publish and trigger one project's actions, and
            // reading it as steering authority here would turn the narrowest
            // grant in the chain into the broadest.
            CodingSessionAuthorityTransitionType::GrantSeat
            | CodingSessionAuthorityTransitionType::RevokeSeat
            | CodingSessionAuthorityTransitionType::Takeover
            | CodingSessionAuthorityTransitionType::Transfer
            | CodingSessionAuthorityTransitionType::GrantProjectActions
            | CodingSessionAuthorityTransitionType::RevokeProjectActions => {}
        }
    }
    active
}

/// Why a published 44245 is not this umbrella's policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodingSessionPolicyExclusionCode {
    /// Signed by an identity that could not steer the umbrella at the time.
    Unauthorized,
    /// The envelope or content did not decode.
    Undecodable,
    /// It decoded, and names a different umbrella than the tag it was filed
    /// under.
    WrongUmbrella,
}

impl CodingSessionPolicyExclusionCode {
    /// The exact wire token for this code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Undecodable => "undecodable",
            Self::WrongUmbrella => "wrongUmbrella",
        }
    }
}

/// One published record this fold refused, with the reason a reader needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionPolicyExclusion {
    /// Signed source event id.
    pub event_id: String,
    /// Pubkey that signed it.
    pub author: String,
    /// Signed source creation time, Unix seconds.
    pub created_at: u64,
    /// Why it is not the policy.
    pub code: CodingSessionPolicyExclusionCode,
    /// One sentence naming the rule it failed.
    pub reason: String,
}

/// The newest-accepted-wins answer, plus everything it refused.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CodingSessionPolicyFold {
    /// The policy in force, or `None` when nobody with standing set one.
    pub selected: Option<CodingSessionContextPolicy>,
    /// Every record this fold **refused**, ordered newest first.
    ///
    /// Not every record that is not [`Self::selected`]: an authorized,
    /// decodable, correctly-addressed policy that is merely older than the
    /// winner is neither selected nor excluded, because newest-accepted-wins
    /// simply passes over it. Only the unauthorized, the undecodable and the
    /// wrongly-addressed appear here.
    pub excluded: Vec<CodingSessionPolicyExclusion>,
}

/// Fold every published 44245 for one umbrella into the record in force.
///
/// Newest `created_at` wins; ties break on the event id descending, so two
/// revisions sharing a second fold the same way on every host rather than by
/// relay page order.
///
/// `may_set_policy` receives the author's pubkey and the record's `created_at`
/// in Unix seconds. Two callers answer it two ways — the context projection
/// from the verified NIP-CSAT chain at publication time
/// ([`signer_may_steer_at`]), a live provider from the grants it has already
/// folded onto the session record — but there is **one** fold, so they cannot
/// disagree about which record won.
pub fn fold_coding_session_policies(
    records: &[Event],
    session_ref: &str,
    genesis_ref: &str,
    founder: &str,
    may_set_policy: &dyn Fn(&str, u64) -> bool,
) -> CodingSessionPolicyFold {
    let mut candidates: Vec<&Event> = records.iter().collect();
    candidates.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });

    let mut fold = CodingSessionPolicyFold::default();
    for event in candidates {
        let created_at = event.created_at.as_secs();
        let author = event.pubkey.to_hex();
        // The winner short-circuits the *selection* but not the listing: a
        // reader must still be able to see that a stranger published a
        // competing ceiling, even when a good record outranks it.
        if !may_set_policy(&author, created_at) {
            fold.excluded.push(CodingSessionPolicyExclusion {
                event_id: event.id.to_hex(),
                author,
                created_at,
                code: CodingSessionPolicyExclusionCode::Unauthorized,
                reason: "signed by an identity that could not steer this umbrella when it was \
                         published: only the founder, or a seat holding an operator grant \
                         accepted by then, may set policy"
                    .to_owned(),
            });
            continue;
        }
        let record = match validate_coding_session_policy_envelope(event) {
            Ok(record) => record,
            Err(error) => {
                fold.excluded.push(CodingSessionPolicyExclusion {
                    event_id: event.id.to_hex(),
                    author,
                    created_at,
                    code: CodingSessionPolicyExclusionCode::Undecodable,
                    reason: error,
                });
                continue;
            }
        };
        if record.session_ref != session_ref || record.genesis_ref != genesis_ref {
            fold.excluded.push(CodingSessionPolicyExclusion {
                event_id: event.id.to_hex(),
                author,
                created_at,
                code: CodingSessionPolicyExclusionCode::WrongUmbrella,
                reason: "names a different umbrella than the tag it was filed under".to_owned(),
            });
            continue;
        }
        if fold.selected.is_none() {
            fold.selected = Some(CodingSessionContextPolicy {
                event_id: event.id.to_hex(),
                created_at,
                author_is_founder: author == founder,
                author,
                record,
            });
        }
    }
    fold
}
