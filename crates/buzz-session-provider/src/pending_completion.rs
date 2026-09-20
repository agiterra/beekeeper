//! Software re-evaluation of a pending `mission.completed` (ledger 183).
//!
//! # What is being re-evaluated, and by whom
//!
//! A lead that publishes a completion before every prerequisite is on the wire
//! publishes a **durable** record: the fold re-derives its prerequisites from
//! the supplied event set on every read, so the same record becomes the
//! session's canonical terminal the moment the last one arrives
//! ([`buzz_core::coding_session_team_transaction::CodingSessionTeamPendingCompletion`]).
//! There is therefore no second record to produce and nothing to deliver — and
//! that is the whole point. `VISION_COLLABORATION.md` § "Deterministic
//! operations first": no model turn is spent asking whether a known process
//! finished.
//!
//! What *was* missing is that nothing looked. This provider treats an arriving
//! kind-44244 report as a fact worth acting on and drops every other operation
//! class on the floor (`Provider::team_report_candidate`), so the three
//! records that can finish a held-back completion — an acknowledgement, a
//! decision answer, a verifier's refutation — arrived and caused nothing at
//! all. [`settlement_fact`] names exactly those three, and the caller asks the
//! ordinary complete-discovery pass to read the channel again. The
//! re-evaluation is a fold over signed facts in the provider's own pass; no
//! seat is woken, and no wake intent is minted for a settlement fact.
//!
//! # Why this is not a CI-style continuation
//!
//! [`crate::ci_continuation`] exists to deliver **one model turn, exactly
//! once**, when an external system reports a result: it registers a durable
//! promise against an exact target, re-checks authority at delivery, and
//! spends an operation-ledger slot so two registrations cannot both run. A
//! pending completion needs the opposite of all three. It has no turn to
//! deliver, it carries no target, and the record that would be "delivered"
//! already exists and is already signed. Registering it in that machinery
//! would add a durable promise whose only action is to publish nothing, and a
//! second copy of a fact the fold already derives — two states that can
//! disagree. Ledger item 183 records the choice.

use buzz_core::coding_session_team_transaction::{
    validate_coding_session_team_transaction_envelope, CodingSessionTeamTransactionBody,
    CodingSessionTeamVerdict,
};
use nostr::Event;

/// A signed record whose arrival can turn a held-back completion terminal.
///
/// Exactly the three classes the fold's three pending exclusion codes wait on
/// ([`buzz_core::coding_session_team_transaction::PENDING_COMPLETION_CODES`]),
/// and no others. An assignment, a report or a lead's disposition can only
/// ever *add* a prerequisite or leave one unmet; a note changes no state; a
/// terminal is the thing being waited for, not a thing it waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementFact {
    /// An assignee's acknowledgement — the last link of an approval chain, and
    /// the one ledger 179(a)'s lead woke six seats to collect.
    Acknowledgement,
    /// An answer to a `decision.request` that a completion's assignment was
    /// blocked on (`CompletionBlockedByOpenDecision`).
    DecisionAnswer,
    /// A verifier's refutation verdict, which is what
    /// `gates.verifierRequired` counts as a ruling
    /// (`CompletionNotVerified`).
    VerifierRefutation,
}

impl SettlementFact {
    /// The stable lowercase word this fact is logged under.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Acknowledgement => "acknowledgement",
            Self::DecisionAnswer => "decision_answer",
            Self::VerifierRefutation => "verifier_refutation",
        }
    }
}

/// Classify an arriving kind-44244 event as a settlement fact, or not.
///
/// Decided from the **validated envelope**, never from the event's own
/// self-description: this is the same decode
/// `Provider::team_report_candidate` performs, and an event whose envelope
/// does not validate is not a fact about anything. Authorization is
/// deliberately *not* checked here — nothing is admitted on the strength of
/// this answer, the only consequence is that the provider reads the channel's
/// complete signed set again and folds it, which is where standing is judged.
pub fn settlement_fact(event: &Event) -> Option<SettlementFact> {
    let payload = validate_coding_session_team_transaction_envelope(event).ok()?;
    match payload.body {
        CodingSessionTeamTransactionBody::Acknowledgement(_) => {
            Some(SettlementFact::Acknowledgement)
        }
        CodingSessionTeamTransactionBody::DecisionAnswer(_) => Some(SettlementFact::DecisionAnswer),
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            ..
        }) => Some(SettlementFact::VerifierRefutation),
        _ => None,
    }
}

#[cfg(test)]
#[path = "pending_completion_tests.rs"]
mod tests;
