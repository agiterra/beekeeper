//! Open `decision.request` records, admitted to the inbox of the party they
//! are held on (ledger 249(A)).
//!
//! A kind:44244 cannot carry a `p` tag — its envelope is exactly five tags,
//! and `validate_coding_session_team_transaction_envelope` refuses a sixth —
//! so the `#p = me` filters that admit mentions and 46010 approvals never
//! matched one. Both control runs of 2026-09-22 lost 49 and 42 minutes to a
//! founder-held request nobody saw. This reader admits a request by what it
//! says, not by a tag it cannot have:
//!
//! - `heldOn: founder` → the signer of the session's genesis (the founder is
//!   whoever founded *that* session, never this computer's user by default);
//! - `heldOn: <pubkey>` → that pubkey, or the NIP-OA owner of that agent.
//!
//! A request already answered or superseded is not admitted: an inbox item
//! with nothing left to decide would be a badge pointing at nothing.

use std::collections::{HashMap, HashSet};

use beekeeper_core_pkg::coding_session_team_transaction::{
    validate_coding_session_team_transaction_envelope, CodingSessionTeamTransactionBody,
    CODING_SESSION_TEAM_DECISION_FOUNDER,
};
use beekeeper_core_pkg::kind::{KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_TEAM_TRANSACTION};

use crate::app_state::AppState;

/// How far back an unanswered request is still looked for, when the caller
/// passed no `since`. A ruling older than this is not an inbox item.
const DECISION_LOOKBACK_SECS: i64 = 14 * 24 * 60 * 60;
/// Upper bound on team records read per feed refresh.
const DECISION_QUERY_LIMIT: u32 = 500;

/// One request that decoded, with the party it is held on unresolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OpenDecisionRequest {
    pub event_id: String,
    pub genesis_ref: String,
    pub held_on: String,
}

/// Decode every 44244 and return the requests nothing has answered or
/// superseded. Records that fail the strict envelope are skipped, exactly as
/// every other reader skips them.
pub(super) fn open_decision_requests(events: &[nostr::Event]) -> Vec<OpenDecisionRequest> {
    let mut closed = HashSet::new();
    let mut requests = Vec::new();
    for event in events {
        let Ok(payload) = validate_coding_session_team_transaction_envelope(event) else {
            continue;
        };
        if let Some(superseded) = payload.supersedes.as_ref() {
            closed.insert(superseded.clone());
        }
        match payload.body {
            CodingSessionTeamTransactionBody::DecisionAnswer(answer) => {
                closed.insert(answer.request_ref);
            }
            CodingSessionTeamTransactionBody::DecisionRequest(request) => {
                requests.push(OpenDecisionRequest {
                    event_id: event.id.to_hex(),
                    genesis_ref: payload.genesis_ref,
                    held_on: request.held_on,
                });
            }
            _ => {}
        }
    }
    requests.retain(|request| !closed.contains(&request.event_id));
    requests
}

/// The pubkey whose inbox a request belongs in, if it can be named.
///
/// `founder` resolves through the genesis signer; an unresolvable genesis
/// yields `None` rather than a guess at who founded the session.
pub(super) fn responsible_party(
    request: &OpenDecisionRequest,
    genesis_signers: &HashMap<String, String>,
) -> Option<String> {
    if request.held_on == CODING_SESSION_TEAM_DECISION_FOUNDER {
        genesis_signers.get(&request.genesis_ref).cloned()
    } else {
        Some(request.held_on.clone())
    }
}

/// Whether `me` is the responsible party, or owns the agent that is.
pub(super) fn admitted_for(
    request: &OpenDecisionRequest,
    me: &str,
    genesis_signers: &HashMap<String, String>,
    agent_owners: &HashMap<String, String>,
) -> bool {
    match responsible_party(request, genesis_signers) {
        Some(party) if party == me => true,
        Some(party) => agent_owners.get(&party).is_some_and(|owner| owner == me),
        None => false,
    }
}

/// The Inbox's needs-action events: the `#p = me` approvals, then the open
/// decision requests held on `me`, which no `p` tag can address.
pub(super) async fn needs_action_events(
    state: &AppState,
    approval_filter: serde_json::Value,
    me: &str,
    since: Option<i64>,
) -> Vec<nostr::Event> {
    let mut events = super::query_relay(state, &[approval_filter])
        .await
        .unwrap_or_default();
    events.extend(decision_requests_for(state, me, since).await);
    events
}

/// Read the relay and return the request events held on `me`.
pub(super) async fn decision_requests_for(
    state: &AppState,
    me: &str,
    since: Option<i64>,
) -> Vec<nostr::Event> {
    let since = since.unwrap_or_else(|| chrono::Utc::now().timestamp() - DECISION_LOOKBACK_SECS);
    let events = super::query_relay(
        state,
        &[serde_json::json!({
            "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
            "since": since,
            "limit": DECISION_QUERY_LIMIT,
        })],
    )
    .await
    .unwrap_or_default();
    let open = open_decision_requests(&events);
    if open.is_empty() {
        return Vec::new();
    }
    let genesis_ids: Vec<String> = open
        .iter()
        .filter(|request| request.held_on == CODING_SESSION_TEAM_DECISION_FOUNDER)
        .map(|request| request.genesis_ref.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let genesis_signers: HashMap<String, String> = if genesis_ids.is_empty() {
        HashMap::new()
    } else {
        super::query_relay(
            state,
            &[serde_json::json!({ "kinds": [KIND_CODING_SESSION_GENESIS], "ids": genesis_ids })],
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|genesis| (genesis.id.to_hex(), genesis.pubkey.to_hex()))
        .collect()
    };
    let held_pubkeys: Vec<String> = open
        .iter()
        .filter(|request| request.held_on != CODING_SESSION_TEAM_DECISION_FOUNDER)
        .filter(|request| request.held_on != me)
        .map(|request| request.held_on.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let agent_owners: HashMap<String, String> = if held_pubkeys.is_empty() {
        HashMap::new()
    } else {
        super::query_relay(
            state,
            &[serde_json::json!({ "kinds": [0], "authors": held_pubkeys })],
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|profile| {
            crate::nostr_convert::profile_valid_oa_owner_pubkey(&profile)
                .map(|owner| (profile.pubkey.to_hex(), owner))
        })
        .collect()
    };
    let admitted: HashSet<String> = open
        .iter()
        .filter(|request| admitted_for(request, me, &genesis_signers, &agent_owners))
        .map(|request| request.event_id.clone())
        .collect();
    events
        .into_iter()
        .filter(|event| admitted.contains(&event.id.to_hex()))
        .collect()
}

#[cfg(test)]
#[path = "decision_inbox_tests.rs"]
mod tests;
