//! Whether a wake still delivers new responsibility (ledger 266).
//!
//! # The defect this exists for
//!
//! Control run 8 (2026-09-25): after `mission.completed` the lead took five
//! turns that did nothing. Its own two dispositions woke it; a builder alert
//! was stale; a report wake and a verify-result wake arrived 73–93 s late,
//! after the facts they announced had already been used. Every one of those
//! wakes was a valid signed event, and every one was billable.
//!
//! # The rule
//!
//! A wake must deliver *new responsibility*, not announce a record. So the
//! question is asked twice: at **enqueue** — when the sending provider has
//! the verified snapshot and is about to mint the 44220 — and again at
//! **dequeue**, immediately before the receiving actor sends the prompt to
//! the adapter, because a queued wake is already billable and a valid fact can
//! become irrelevant while it waits. [`still_owed`] is the one predicate both
//! call, and it answers with one of five stable drop reasons:
//!
//! - [`WakeDropReason::SelfAuthored`] — the recipient signed the fact. An
//!   author's own report, verdict, disposition or completion never wakes that
//!   author.
//! - [`WakeDropReason::Duplicate`] — the same fact id was already delivered
//!   to this recipient (a verified `turn_started` receipt on one of its seats,
//!   [`delivered_to`]).
//! - [`WakeDropReason::AlreadyConsumed`] — a record the recipient signed
//!   already cites the fact id, so it has been used.
//! - [`WakeDropReason::ObligationDelivered`] — another fact about the same
//!   obligation ([`obligation_of`]: the assignment a report answers, or the
//!   verifier's own assignment a verdict reviews under) was delivered to the
//!   recipient in a turn that ended after this fact was signed, and this fact
//!   reopens no work. Run 11's verdict-then-settlement-report pair (ledger
//!   272(d)). The recipient's owed ruling stays visible in the fold's
//!   `awaiting` set; only the extra turn is not spent.
//! - [`WakeDropReason::PostTerminal`] — the umbrella has a canonical terminal
//!   and the fact either predates it or does not contest it. After a
//!   terminal the only legitimate model work is an explicit new request or
//!   genuinely new evidence that calls completion into question.
//!
//! A drop is never silent: the enqueue side logs it under
//! `csp::team_wake`/`team_wake_dropped`, the dequeue side answers the command
//! with a `turn_dropped` receipt coded [`WAKE_NOT_OWED`] naming the reason and
//! the fact. Anything this module cannot prove is delivered: an unknown fact,
//! an unreachable relay and an unverifiable authority chain all admit.

use std::collections::HashSet;

use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, validate_coding_session_team_transaction_envelope,
    CodingSessionTeamFoldContext, CodingSessionTeamRefutationDecision,
    CodingSessionTeamTransactionBody, CodingSessionTeamVerdict,
};
use nostr::Event;
use serde::{Deserialize, Serialize};

use crate::team_wake::{WakeScope, WakeSource};

/// Receipt error code of a wake dropped at dequeue because it was no longer
/// owed. The message names the [`WakeDropReason`] and the fact id.
pub const WAKE_NOT_OWED: &str = "WAKE_NOT_OWED";

/// Why a wake was dropped instead of spending a model turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeDropReason {
    /// A later record from the recipient already cites the fact.
    AlreadyConsumed,
    /// The recipient signed the fact itself.
    SelfAuthored,
    /// The umbrella is terminal and the fact predates or does not contest it.
    PostTerminal,
    /// The same fact id was already delivered to this recipient.
    Duplicate,
    /// Another fact about the same obligation was already delivered to this
    /// recipient in a turn that ended after this fact was signed.
    ObligationDelivered,
}

impl WakeDropReason {
    /// The stable wire and log slug.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyConsumed => "already_consumed",
            Self::SelfAuthored => "self_authored",
            Self::PostTerminal => "post_terminal",
            Self::Duplicate => "duplicate",
            Self::ObligationDelivered => "obligation_delivered",
        }
    }
}

/// The provider's answer to an actor asking whether a queued wake may start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeAdmission {
    /// Not decided yet; the actor keeps waiting.
    Pending,
    /// Start the turn.
    Admit,
    /// Do not start it; the actor reports the drop and moves on.
    Drop {
        /// Which of the rules held.
        reason: WakeDropReason,
        /// The fact the wake was about.
        fact_id: String,
    },
}

/// The canonical terminal of an umbrella and the second it was signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalMark {
    /// The terminal record's event id.
    pub event_id: String,
    /// Its signed `created_at`, seconds.
    pub created_at: u64,
}

/// The signed fact one wake is about, as far as it is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeFact {
    /// The fact's event id.
    pub fact_id: String,
    /// Its signer, when known.
    pub author: Option<String>,
    /// Its signed second, when known.
    pub created_at: Option<u64>,
    /// Whether the fact is a new request or evidence contesting a completion
    /// — the only kinds of fact a terminal umbrella still owes a turn.
    pub reopens_work: bool,
    /// The obligation the fact is about ([`obligation_of`]), when it names one.
    pub obligation: Option<String>,
}

/// One fact already delivered to the recipient in a turn that has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveredObligation {
    /// The obligation the delivered fact was about.
    pub obligation: String,
    /// The delivered fact.
    pub fact_id: String,
    /// The second the turn that delivered it ended (its `result`).
    pub turn_ended_at: u64,
}

/// What the recipient already knows, folded from signed records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelevanceView {
    /// The actor the wake would start a turn for.
    pub recipient: String,
    /// The umbrella's canonical terminal, when it has one.
    pub terminal: Option<TerminalMark>,
    /// Every fact id a record signed by the recipient cites.
    pub cited_by_recipient: HashSet<String>,
    /// Fact ids already delivered to the recipient.
    pub delivered: HashSet<String>,
    /// Obligations already delivered to the recipient, with when the
    /// delivering turn ended.
    pub delivered_obligations: Vec<DeliveredObligation>,
}

/// Whether `fact` still delivers new responsibility to `view.recipient`.
pub fn still_owed(fact: &WakeFact, view: &RelevanceView) -> Result<(), WakeDropReason> {
    if fact.author.as_deref() == Some(view.recipient.as_str()) {
        return Err(WakeDropReason::SelfAuthored);
    }
    if view.delivered.contains(&fact.fact_id) {
        return Err(WakeDropReason::Duplicate);
    }
    if view.cited_by_recipient.contains(&fact.fact_id) {
        return Err(WakeDropReason::AlreadyConsumed);
    }
    if obligation_already_delivered(fact, view) {
        return Err(WakeDropReason::ObligationDelivered);
    }
    if let Some(terminal) = &view.terminal {
        // Same-second or unknown ordering is not "predates": a contesting
        // fact whose order cannot be proven is delivered.
        let predates = fact
            .created_at
            .is_some_and(|created_at| created_at < terminal.created_at);
        if !fact.reopens_work || predates {
            return Err(WakeDropReason::PostTerminal);
        }
    }
    Ok(())
}

/// Whether another fact about `fact`'s obligation reached the recipient in a
/// turn that ended after `fact` was signed (ledger 272(d)).
///
/// Only bookkeeping is folded this way: a fact that reopens work (a new
/// request, a confirmed refutation, a non-approving disposition) is never
/// assumed seen because an earlier fact about the same obligation was. An
/// unknown signing second or an unended turn proves nothing and admits.
fn obligation_already_delivered(fact: &WakeFact, view: &RelevanceView) -> bool {
    let (Some(obligation), Some(created_at)) = (fact.obligation.as_deref(), fact.created_at) else {
        return false;
    };
    !fact.reopens_work
        && view.delivered_obligations.iter().any(|delivered| {
            delivered.obligation == obligation
                && delivered.fact_id != fact.fact_id
                && created_at < delivered.turn_ended_at
        })
}

/// The canonical terminal of a verified kind-44244 set, with its second.
pub fn terminal_mark(
    events: &[Event],
    context: &CodingSessionTeamFoldContext,
) -> Result<Option<TerminalMark>, String> {
    let fold = fold_coding_session_team_transactions(events, context)?;
    Ok(fold.canonical_terminal.and_then(|terminal| {
        events
            .iter()
            .find(|event| event.id.to_hex() == terminal.event_id)
            .map(|event| TerminalMark {
                event_id: terminal.event_id.clone(),
                created_at: event.created_at.as_secs(),
            })
    }))
}

/// Every event id cited by a record `author` signed.
///
/// Any 64-hex string anywhere in the record's content counts: an assignment
/// ref, a report ref, an evidence ref, a completion's refs. Citing a fact is
/// the author's own signed statement that it has seen it, and it can only
/// ever suppress that author's own wake.
pub fn cited_by(events: &[Event], author: &str) -> HashSet<String> {
    let mut cited = HashSet::new();
    for event in events
        .iter()
        .filter(|event| event.pubkey.to_hex() == author)
    {
        let own_id = event.id.to_hex();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&event.content) {
            collect_event_ids(&value, &mut cited);
        }
        cited.remove(&own_id);
    }
    cited
}

fn collect_event_ids(value: &serde_json::Value, into: &mut HashSet<String>) {
    match value {
        serde_json::Value::String(text) if is_event_id(text) => {
            into.insert(text.to_ascii_lowercase());
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_event_ids(item, into);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_event_ids(item, into);
            }
        }
        _ => {}
    }
}

fn is_event_id(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Whether a team record's body is a new request or contests a completion.
fn body_reopens_work(body: &CodingSessionTeamTransactionBody) -> bool {
    match body {
        CodingSessionTeamTransactionBody::Assignment(_)
        | CodingSessionTeamTransactionBody::DecisionRequest(_)
        | CodingSessionTeamTransactionBody::DecisionAnswer(_) => true,
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
            decision,
            ..
        }) => !decision.is_approval(),
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            decision,
            ..
        }) => *decision == CodingSessionTeamRefutationDecision::Confirmed,
        CodingSessionTeamTransactionBody::Report(_)
        | CodingSessionTeamTransactionBody::Acknowledgement(_)
        | CodingSessionTeamTransactionBody::MissionCompleted(_)
        | CodingSessionTeamTransactionBody::MissionBlocked(_)
        | CodingSessionTeamTransactionBody::Note(_) => false,
    }
}

/// The fact behind one team record id, read from the verified set.
///
/// An id the set does not hold is an unknown fact: no author, no second, and
/// `reopens_work` so the terminal rule cannot drop what it cannot see.
fn team_fact(fact_id: &str, team_events: &[Event]) -> WakeFact {
    let Some(event) = team_events
        .iter()
        .find(|event| event.id.to_hex() == fact_id)
    else {
        return WakeFact {
            fact_id: fact_id.to_owned(),
            author: None,
            created_at: None,
            reopens_work: true,
            obligation: None,
        };
    };
    let reopens_work = validate_coding_session_team_transaction_envelope(event)
        .map(|payload| body_reopens_work(&payload.body))
        .unwrap_or(true);
    WakeFact {
        fact_id: fact_id.to_owned(),
        author: Some(event.pubkey.to_hex()),
        created_at: Some(event.created_at.as_secs()),
        reopens_work,
        obligation: obligation_of(fact_id, team_events),
    }
}

/// The fact a provider-owned wake intent is about (enqueue side).
pub(crate) fn fact_of_source(source: &WakeSource, team_events: &[Event]) -> WakeFact {
    match source {
        WakeSource::Report { operation_id, .. } | WakeSource::Disposition { operation_id, .. } => {
            let mut fact = team_fact(operation_id, team_events);
            // The intent carries the verified signer even when the relay
            // query has not caught up with the record yet.
            if fact.author.is_none() {
                fact.author = source.author_pubkey().map(str::to_owned);
            }
            fact
        }
        // A missing-report diagnostic is bookkeeping about a seat's turn:
        // it never reopens a terminal umbrella.
        WakeSource::Terminal {
            terminal_event_id,
            actor_pubkey,
            terminal_at_ms,
            ..
        } => WakeFact {
            fact_id: terminal_event_id.clone(),
            author: Some(actor_pubkey.clone()),
            created_at: u64::try_from(*terminal_at_ms / 1_000).ok(),
            reopens_work: false,
            obligation: None,
        },
    }
}

/// The fact a delivered wake pointer is about (dequeue side), or `None` for
/// text that is not one of the provider's pointer shapes.
pub fn fact_of_pointer(text: &str, team_events: &[Event]) -> Option<WakeFact> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let fact_id = crate::team_wake::pointer_fact_id(&value)?;
    let object = value.as_object()?;
    if object.contains_key("operationId") {
        return Some(team_fact(&fact_id, team_events));
    }
    if object.contains_key("terminalEventId") {
        return Some(WakeFact {
            fact_id,
            author: None,
            created_at: None,
            reopens_work: false,
            obligation: None,
        });
    }
    // A host result: a failure or refusal is evidence that can contest a
    // completion; a clean exit is confirmation, never a reason to reopen.
    let clean_exit = object
        .get("disposition")
        .and_then(serde_json::Value::as_str)
        == Some("exited")
        && object.get("exitCode").and_then(serde_json::Value::as_i64) == Some(0);
    Some(WakeFact {
        fact_id,
        author: None,
        created_at: None,
        reopens_work: !clean_exit,
        obligation: None,
    })
}

/// The obligation a team fact is about: the assignment whose settlement it
/// moves (ledger 272(d)).
///
/// - A **report** is about the assignment it answers (`assignmentRef`).
/// - A **verdict** is about its author's own open assignment whose `baseSha`
///   is the reviewed report's `headSha` — a verifier reviewing a builder's
///   report does so under its own assignment, and its settlement report on
///   that assignment is the same obligation. When no such assignment is in
///   the set, the verdict's own `assignmentRef` (the reviewed report's).
/// - Anything else, or an id the set does not hold: `None`.
pub fn obligation_of(fact_id: &str, team_events: &[Event]) -> Option<String> {
    let body = |event: &Event| {
        validate_coding_session_team_transaction_envelope(event)
            .ok()
            .map(|payload| payload.body)
    };
    let event = team_events
        .iter()
        .find(|event| event.id.to_hex() == fact_id)?;
    let (assignment_ref, report_ref) = match body(event)? {
        CodingSessionTeamTransactionBody::Report(report) => return Some(report.assignment_ref),
        CodingSessionTeamTransactionBody::Verdict(
            CodingSessionTeamVerdict::Refutation {
                assignment_ref,
                report_ref,
                ..
            }
            | CodingSessionTeamVerdict::Disposition {
                assignment_ref,
                report_ref,
                ..
            },
        ) => (assignment_ref, report_ref),
        _ => return None,
    };
    let reviewed_head = team_events
        .iter()
        .find(|candidate| candidate.id.to_hex() == report_ref)
        .and_then(body)
        .and_then(|reviewed| match reviewed {
            CodingSessionTeamTransactionBody::Report(report) => report.head_sha,
            _ => None,
        });
    let author = event.pubkey.to_hex();
    let superseded: HashSet<String> = team_events
        .iter()
        .filter_map(|candidate| {
            validate_coding_session_team_transaction_envelope(candidate)
                .ok()?
                .supersedes
        })
        .collect();
    let own_open_assignment = reviewed_head.and_then(|head| {
        team_events
            .iter()
            .filter(|candidate| !superseded.contains(&candidate.id.to_hex()))
            .filter(|candidate| match body(candidate) {
                Some(CodingSessionTeamTransactionBody::Assignment(assignment)) => {
                    assignment.assignee_actor.eq_ignore_ascii_case(&author)
                        && assignment
                            .base_sha
                            .as_deref()
                            .is_some_and(|base| base.eq_ignore_ascii_case(&head))
                }
                _ => false,
            })
            .max_by_key(|candidate| candidate.created_at.as_secs())
            .map(|candidate| candidate.id.to_hex())
    });
    Some(own_open_assignment.unwrap_or(assignment_ref))
}

/// Command-id prefixes that embed the fact a wake is about:
/// `cli-wake-v1:<fact>:…` (`bee`) and `team-wake-v1:<fact>:…` (Desktop).
const FACT_NAMING_WAKE_PREFIXES: &[&str] = &["cli-wake-v1:", "team-wake-v1:"];

/// The fact a delivered wake command was about: from its command id when the
/// minter embeds it, else from its pointer text.
fn delivered_fact(command_id: &str, content: &str) -> Option<String> {
    FACT_NAMING_WAKE_PREFIXES
        .iter()
        .find_map(|prefix| command_id.strip_prefix(prefix))
        .and_then(|rest| rest.split(':').next())
        .filter(|fact| is_event_id(fact))
        .map(str::to_ascii_lowercase)
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(content)
                .ok()
                .and_then(|value| crate::team_wake::pointer_fact_id(&value))
        })
}

/// What the recipient's own turns already delivered, from the verified
/// context package (ledger 272(d)).
///
/// A command addressed to one of the recipient's seats whose verified
/// receipt says `turn_started` delivered its fact. The fact comes from the
/// command id (`cli-wake-v1:<fact>:…`, `team-wake-v1:<fact>:…`) or the
/// pointer it carried. Returns every such fact id (the `Duplicate` rule), and
/// — for each one whose turn has a signed `result` — its obligation and the
/// second that turn ended. A turn with no `result` in the package has no
/// provable end and delivers no obligation.
pub fn delivered_to(
    package: &buzz_core::coding_session_context::CodingSessionContextPackage,
    recipient: &str,
    team_events: &[Event],
) -> (HashSet<String>, Vec<DeliveredObligation>) {
    use buzz_core::coding_session_payload::ReceiptStatus;
    let seats: Vec<_> = package
        .roster
        .iter()
        .filter(|entry| entry.actor.as_deref() == Some(recipient))
        .map(|entry| &entry.target)
        .collect();
    let mut delivered = HashSet::new();
    let mut obligations = Vec::new();
    for item in package.inbox.iter().filter(|item| {
        seats.contains(&&item.target) && item.stage == Some(ReceiptStatus::TurnStarted)
    }) {
        let Some(fact_id) = delivered_fact(&item.command_id, &item.content) else {
            continue;
        };
        delivered.insert(fact_id.clone());
        let turn_id = package.history.iter().find_map(|echo| {
            (echo.target == item.target
                && echo.item_kind == "user_prompt"
                && echo
                    .content
                    .get("commandId")
                    .and_then(serde_json::Value::as_str)
                    == Some(item.command_id.as_str()))
            .then_some(echo.turn_id.as_deref())
            .flatten()
        });
        let ended_at = turn_id.and_then(|turn_id| {
            package
                .history
                .iter()
                .filter(|result| {
                    result.target == item.target
                        && result.item_kind == "result"
                        && result.turn_id.as_deref() == Some(turn_id)
                })
                .map(|result| result.created_at)
                .max()
        });
        if let (Some(turn_ended_at), Some(obligation)) =
            (ended_at, obligation_of(&fact_id, team_events))
        {
            obligations.push(DeliveredObligation {
                obligation,
                fact_id,
                turn_ended_at,
            });
        }
    }
    (delivered, obligations)
}

/// Build the view one routing decision checks a wake against.
pub fn view_for(
    recipient: &str,
    terminal: Option<TerminalMark>,
    cited_sources: &[&[Event]],
) -> RelevanceView {
    let mut cited_by_recipient = HashSet::new();
    for events in cited_sources {
        cited_by_recipient.extend(cited_by(events, recipient));
    }
    RelevanceView {
        recipient: recipient.to_owned(),
        terminal,
        cited_by_recipient,
        delivered: HashSet::new(),
        delivered_obligations: Vec::new(),
    }
}

/// The dequeue-side check, from fresh relay facts: is the wake `text`,
/// about to start a turn for `recipient` in `scope`, still owed?
///
/// `Ok(None)` admits; `Ok(Some(_))` names the drop; `Err` means the facts
/// could not be proven, which the caller must treat as an admit.
pub(crate) async fn evaluate_at_dequeue(
    rest: &buzz_acp::relay::RestClient,
    relay_self_pubkey: &str,
    scope: &WakeScope,
    recipient: &str,
    text: &str,
) -> Result<Option<(WakeDropReason, String)>, String> {
    let facts = crate::team_wake::fetch_verified_team_facts(rest, relay_self_pubkey, scope)
        .await
        .map_err(|error| error.to_string())?;
    let Some(fact) = fact_of_pointer(text, &facts.team_events) else {
        return Ok(None);
    };
    let context = crate::team_wake::fold_context(scope, &facts.founder_pubkey, &facts.authority);
    let terminal = terminal_mark(&facts.team_events, &context)?;
    // Work records (`work.evidence_bound`) cite host results; a relay that
    // cannot list them costs this check that one source, never the wake.
    let work_records = crate::context_projector::query_complete_kind_partition(
        rest,
        scope.channel_ref,
        buzz_core::kind::KIND_PROJECT_WORK_RECORD,
    )
    .await
    .unwrap_or_default();
    let mut view = view_for(recipient, terminal, &[&facts.team_events, &work_records]);
    if let Err(reason) = still_owed(&fact, &view) {
        return Ok(Some((reason, fact.fact_id)));
    }
    // Only a bookkeeping fact about a named obligation can be one the
    // recipient already had (ledger 272(d)); only then is the package — the
    // recipient's delivered turns — worth projecting. A projection that
    // fails costs this check that one rule, never the wake.
    if fact.obligation.is_none() || fact.reopens_work {
        return Ok(None);
    }
    let Ok(package) = crate::team_wake::fetch_scope_package(rest, relay_self_pubkey, scope).await
    else {
        return Ok(None);
    };
    let (delivered, obligations) = delivered_to(&package, recipient, &facts.team_events);
    view.delivered = delivered;
    view.delivered_obligations = obligations;
    Ok(still_owed(&fact, &view)
        .err()
        .map(|reason| (reason, fact.fact_id)))
}

#[cfg(test)]
#[path = "wake_relevance_tests.rs"]
mod tests;
