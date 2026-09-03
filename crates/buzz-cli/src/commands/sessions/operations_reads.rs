//! Reading one coding session off the relay, before anything is written.
//!
//! Every 44244 verb answers the same two questions before it acts: *what has
//! this session already published*, and *who may publish into it*. Both are
//! relay reads with a context check on every row, and both are shared by the
//! writers in [`super::operations`], the pre-publish check in
//! [`super::operations_precheck`] and `bee sessions policy`
//! ([`super::policy`]), so the reader and the writers can never disagree about
//! which events belong to a session.

use buzz_core::coding_session_genesis::{
    decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
};
use buzz_core::coding_session_policy::CodingSessionPolicyGrant;
use buzz_core::coding_session_team_transaction::CodingSessionTeamFoldContext;
use buzz_core::kind::{KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_TEAM_TRANSACTION};
use buzz_sdk::coding_session_team_transaction::parse_coding_session_team_transaction;
use nostr::Event;
use serde_json::{json, Value};

use super::operations_authority::fetch_projected_authority;
use crate::client::BuzzClient;
use crate::error::CliError;

pub(crate) async fn fetch_transactions(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<Vec<Event>, CliError> {
    let filter = transaction_query_filter(channel, session_ref, genesis);
    let values = client.query_all(filter).await?;
    let events = values
        .into_iter()
        .filter(|value| transaction_value_matches_context(value, channel, session_ref, genesis))
        .map(|value| {
            serde_json::from_value(value).map_err(|error| {
                CliError::Other(format!("relay returned malformed event: {error}"))
            })
        })
        .collect::<Result<Vec<Event>, _>>()?;
    Ok(events
        .into_iter()
        .filter(|event| transaction_matches_context(event, channel, session_ref, genesis))
        .collect())
}

pub(super) fn transaction_value_matches_context(
    value: &Value,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> bool {
    let has_tag = |name: &str, expected: &str| {
        value
            .get("tags")
            .and_then(Value::as_array)
            .is_some_and(|tags| {
                tags.iter().any(|tag| {
                    tag.as_array().is_some_and(|parts| {
                        parts.len() == 2
                            && parts[0].as_str() == Some(name)
                            && parts[1].as_str() == Some(expected)
                    })
                })
            })
    };
    let payload: Value = match value
        .get("content")
        .and_then(Value::as_str)
        .and_then(|content| serde_json::from_str(content).ok())
    {
        Some(payload) => payload,
        None => return false,
    };
    value.get("kind").and_then(Value::as_u64)
        == Some(u64::from(KIND_CODING_SESSION_TEAM_TRANSACTION))
        && has_tag("h", channel)
        && has_tag("d", session_ref)
        && has_tag("cstx-genesis", genesis)
        && payload.get("sessionRef").and_then(Value::as_str) == Some(session_ref)
        && payload.get("genesisRef").and_then(Value::as_str) == Some(genesis)
}

pub(super) fn transaction_query_filter(channel: &str, session_ref: &str, genesis: &str) -> Value {
    json!({
        "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
        "#h": [channel],
        "#d": [session_ref],
        "#cstx-genesis": [genesis],
    })
}

pub(super) fn transaction_matches_context(
    event: &Event,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> bool {
    parse_coding_session_team_transaction(event).is_ok_and(|payload| {
        payload.session_ref == session_ref
            && payload.genesis_ref == genesis
            && event
                .tags
                .iter()
                .any(|tag| tag.as_slice() == ["h", channel])
    })
}

pub(crate) async fn fetch_session_authority(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<SessionAuthority, CliError> {
    let rows = client
        .query_all(json!({
            "ids": [genesis],
            "kinds": [KIND_CODING_SESSION_GENESIS],
            "#h": [channel]
        }))
        .await?;
    if rows.len() != 1 {
        return Err(CliError::NotFound(format!(
            "expected exactly one genesis {genesis} in channel {channel}, found {}",
            rows.len()
        )));
    }
    let event: Event = serde_json::from_value(rows[0].clone())
        .map_err(|error| CliError::Other(format!("relay returned malformed genesis: {error}")))?;
    buzz_core::verify_event(&event)
        .map_err(|error| CliError::Other(format!("invalid genesis signature: {error}")))?;
    let payload = decode_coding_session_genesis(&event.content)
        .map_err(|error| CliError::Other(format!("invalid genesis content: {error}")))?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    let valid_envelope = tags.len() == 3
        && tags.iter().all(|tag| tag.len() == 2)
        && tags[0] == ["h", channel]
        && tags[1] == ["csg-v", CODING_SESSION_GENESIS_TAG_VERSION]
        && tags[2] == ["csg-session", payload.session_ref.as_str()];
    if !valid_envelope {
        return Err(CliError::Other("invalid genesis tag envelope".into()));
    }
    if payload.session_ref != session_ref {
        return Err(CliError::Usage(
            "--session-ref does not match the referenced genesis".into(),
        ));
    }
    let authority =
        fetch_projected_authority(client, channel, genesis, &event.pubkey.to_hex()).await?;
    Ok(SessionAuthority {
        context: CodingSessionTeamFoldContext {
            channel_ref: channel.to_owned(),
            session_ref: session_ref.to_owned(),
            genesis_ref: genesis.to_owned(),
            founder_pubkey: event.pubkey.to_hex(),
            active_seats: authority.seats,
            active_grants: authority.grants,
            // Not a claim about the session: this reader does not read kind
            // 44245 at all. `super::operations_verifier_gate` folds the policy
            // and overwrites it before the fold that enforces it runs.
            verifier_required: false,
        },
        policy_grants: authority.policy_grants,
    })
}

/// A session's verified genesis and its accepted authority chain, in the two
/// shapes the CLI's readers need.
pub(crate) struct SessionAuthority {
    /// What the kind-44244 fold judges against.
    pub(crate) context: CodingSessionTeamFoldContext,
    /// The grant/revoke history with acceptance times, which kind 44245 needs
    /// because a policy is judged at the moment it was published.
    pub(crate) policy_grants: Vec<CodingSessionPolicyGrant>,
}

/// The kind-44244 fold context alone, for the callers that need nothing else.
pub(super) async fn fetch_founder_context(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<CodingSessionTeamFoldContext, CliError> {
    Ok(
        fetch_session_authority(client, channel, session_ref, genesis)
            .await?
            .context,
    )
}
