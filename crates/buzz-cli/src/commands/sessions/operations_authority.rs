//! Receipt-backed projection of a session's accepted kind-44228 authority
//! chain.
//!
//! A child of `operations`, split out only to keep that file under 1,000 lines
//! (batch 2 lane B2). No behaviour change: these are the same items, and
//! `use super::*` gives them the parent's imports and private helpers exactly
//! as before.

use std::collections::BTreeMap;

use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionPayload,
    CodingSessionAuthorityTransitionType, CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use buzz_core::coding_session_policy::CodingSessionPolicyGrant;
use buzz_core::coding_session_team_transaction::{
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat,
};
use buzz_core::kind::{KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_SYSTEM_MESSAGE};
use nostr::Event;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;

pub(super) const AUTHORITY_ACCEPTANCE_RECEIPT_TYPE: &str =
    "coding_session_authority_transition_accepted";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthorityAcceptanceReceipt {
    #[serde(rename = "type")]
    receipt_type: String,
    genesis_ref: String,
    accepted_event_id: String,
    seq: u32,
    transition_type: CodingSessionAuthorityTransitionType,
    grantee_pubkey: String,
    #[serde(default)]
    role: Option<String>,
}

async fn fetch_trusted_relay_self(client: &BuzzClient) -> Result<String, CliError> {
    let raw = client
        .get_public("/")
        .await
        .map_err(|error| CliError::Other(format!("failed to fetch relay info: {error}")))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|error| CliError::Other(format!("relay info is not valid JSON: {error}")))?;
    let relay_self = value
        .get("self")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::Other("relay info is missing its trusted self pubkey".into()))?;
    if relay_self.len() != 64 || !relay_self.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Other(
            "relay info self is not a valid 64-hex pubkey".into(),
        ));
    }
    Ok(relay_self.to_ascii_lowercase())
}

/// Read the relay-receipt-backed accepted authority chain for one genesis.
pub(super) async fn fetch_projected_authority(
    client: &BuzzClient,
    channel: &str,
    genesis: &str,
    founder: &str,
) -> Result<ProjectedAuthority, CliError> {
    let relay_self = fetch_trusted_relay_self(client).await?;
    let transitions = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
            "#h": [channel],
            "#csat-genesis": [genesis]
        }))
        .await?
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<Event>, _>>()
        .map_err(|error| {
            CliError::Other(format!("relay returned malformed authority event: {error}"))
        })?;
    let receipts = client
        .query_all(json!({
            "kinds": [KIND_SYSTEM_MESSAGE],
            "#h": [channel],
            "authors": [relay_self]
        }))
        .await?
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<Event>, _>>()
        .map_err(|error| {
            CliError::Other(format!(
                "relay returned malformed authority receipt: {error}"
            ))
        })?;
    project_receipt_backed_authority_chain(
        &transitions,
        &receipts,
        channel,
        genesis,
        founder,
        &relay_self,
    )
    .map_err(|error| CliError::Other(format!("invalid accepted authority chain: {error}")))
}

pub(super) struct ProjectedAuthority {
    pub(super) grants: Vec<CodingSessionTeamActiveGrant>,
    pub(super) seats: Vec<CodingSessionTeamActiveSeat>,
    pub(super) seat_grant_refs: BTreeMap<String, String>,
    pub(super) head_event_id: Option<String>,
    pub(super) head_seq: u32,
    /// The grant/revoke history in accepted order, with the relay acceptance
    /// receipt's `created_at` on each.
    ///
    /// `grants` above is the projection's *current* answer, which cannot say
    /// whether a grant stood at some earlier moment. Kind 44245 needs exactly
    /// that: a policy is judged by whether its signer could steer **when it was
    /// published**, so a later revoke must not retroactively invalidate a
    /// record, and a later grant must not retroactively bless one
    /// (REVIEW-B2 F1).
    pub(super) policy_grants: Vec<CodingSessionPolicyGrant>,
}

fn validate_accepted_authority_transition(
    event: &Event,
    channel: &str,
    genesis: &str,
) -> Result<(String, CodingSessionAuthorityTransitionPayload), String> {
    buzz_core::verify_event(event)
        .map_err(|error| format!("invalid accepted authority-transition signature: {error}"))?;
    if event.kind.as_u16() as u32 != KIND_CODING_SESSION_AUTHORITY_TRANSITION {
        return Err("accepted authority transition has the wrong kind".into());
    }
    let payload = decode_coding_session_authority_transition(&event.content)?;
    if payload.genesis_ref != genesis {
        return Err("accepted authority transition crosses the supplied genesis".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 3
        || tags.iter().any(|tag| tag.len() != 2)
        || tags[0] != ["h", channel]
        || tags[1] != ["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION]
        || tags[2] != ["csat-genesis", genesis]
    {
        return Err("accepted authority transition has an invalid envelope".into());
    }
    Ok((event.pubkey.to_hex(), payload))
}

pub(super) fn project_receipt_backed_authority_chain(
    transition_events: &[Event],
    receipt_events: &[Event],
    channel: &str,
    genesis: &str,
    founder: &str,
    relay_self: &str,
) -> Result<ProjectedAuthority, String> {
    let mut transitions = BTreeMap::new();
    for event in transition_events {
        let event_id = event.id.to_hex();
        transitions.entry(event_id).or_insert(event);
    }

    let mut receipt_ids = std::collections::BTreeSet::new();
    let mut accepted_by_seq = BTreeMap::new();
    let mut accepted_ids = std::collections::BTreeSet::new();
    for event in receipt_events {
        let value: Value = match serde_json::from_str(&event.content) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("type").and_then(Value::as_str) != Some(AUTHORITY_ACCEPTANCE_RECEIPT_TYPE) {
            continue;
        }
        if value.get("genesisRef").and_then(Value::as_str) != Some(genesis) {
            continue;
        }
        if !receipt_ids.insert(event.id.to_hex()) {
            continue;
        }
        if u32::from(event.kind.as_u16()) != KIND_SYSTEM_MESSAGE {
            return Err("authority acceptance receipt has the wrong kind".into());
        }
        buzz_core::verify_event(event)
            .map_err(|error| format!("invalid authority-receipt signature: {error}"))?;
        if event.pubkey.to_hex() != relay_self {
            return Err("authority acceptance receipt is not signed by the trusted relay".into());
        }
        if !event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["h", channel])
        {
            return Err("authority acceptance receipt crosses the requested channel".into());
        }
        let role_key_present = value
            .as_object()
            .is_some_and(|object| object.contains_key("role"));
        let receipt: AuthorityAcceptanceReceipt = serde_json::from_value(value)
            .map_err(|error| format!("malformed authority acceptance receipt: {error}"))?;
        if receipt.receipt_type != AUTHORITY_ACCEPTANCE_RECEIPT_TYPE {
            return Err("authority receipt type mismatch".into());
        }
        let is_seat_receipt = matches!(
            receipt.transition_type,
            CodingSessionAuthorityTransitionType::GrantSeat
                | CodingSessionAuthorityTransitionType::RevokeSeat
        );
        if is_seat_receipt != role_key_present || is_seat_receipt != receipt.role.is_some() {
            return Err(
                "authority receipt role presence does not match its transition type".into(),
            );
        }
        let transition = transitions
            .get(&receipt.accepted_event_id)
            .ok_or_else(|| "authority receipt references a missing transition".to_owned())?;
        let (signer, payload) =
            validate_accepted_authority_transition(transition, channel, genesis)?;
        if payload.genesis_ref != receipt.genesis_ref
            || payload.seq != receipt.seq
            || payload.transition_type != receipt.transition_type
            || payload.grantee_pubkey != receipt.grantee_pubkey
            || payload.role != receipt.role
        {
            return Err("authority receipt facts do not match the accepted transition".into());
        }
        if !accepted_ids.insert(receipt.accepted_event_id.clone()) {
            return Err("duplicate authority receipts name the same accepted transition".into());
        }
        let accepted = (
            receipt.accepted_event_id.clone(),
            signer,
            payload,
            event.created_at.as_secs(),
        );
        if accepted_by_seq.insert(receipt.seq, accepted).is_some() {
            return Err("conflicting authority receipts claim the same sequence".into());
        }
    }

    let links: Vec<_> = accepted_by_seq.into_values().collect();
    let mut expected_prev: Option<&str> = None;
    let mut grants = BTreeMap::new();
    let mut seats = BTreeMap::new();
    let mut seat_grant_refs = BTreeMap::new();
    let mut policy_grants = Vec::with_capacity(links.len());
    for (offset, (event_id, signer, payload, accepted_at)) in links.iter().enumerate() {
        let expected_seq = u32::try_from(offset + 1)
            .map_err(|_| "authority chain exceeds u32 sequence space".to_owned())?;
        if payload.seq != expected_seq || payload.prev_accepted.as_deref() != expected_prev {
            return Err(format!(
                "authority transition {} does not extend the canonical chain",
                event_id
            ));
        }
        let signer_is_founder = signer == founder;
        let signer_is_operator = grants
            .get(signer)
            .is_some_and(|grant: &CodingSessionTeamActiveGrant| grant.may_steer);
        let signer_is_lead = seats
            .get(signer)
            .is_some_and(|seat: &CodingSessionTeamActiveSeat| seat.role == "lead");
        let is_seat_transition = matches!(
            payload.transition_type,
            CodingSessionAuthorityTransitionType::GrantSeat
                | CodingSessionAuthorityTransitionType::RevokeSeat
        );
        let signer_is_authorized = if is_seat_transition {
            signer_is_founder || signer_is_operator || signer_is_lead
        } else {
            signer_is_founder
        };
        if !signer_is_authorized {
            return Err(format!(
                "authority transition {event_id} has an unauthorized signer"
            ));
        }
        if payload.transition_type == CodingSessionAuthorityTransitionType::GrantSeat
            && payload.grantee_pubkey == *signer
        {
            return Err("seat grant cannot nominate its own signer".into());
        }
        if signer_is_lead
            && !signer_is_founder
            && !signer_is_operator
            && payload.role.as_deref() == Some("lead")
        {
            return Err("active lead cannot grant or revoke lead authority".into());
        }
        policy_grants.push(CodingSessionPolicyGrant {
            grantee: payload.grantee_pubkey.clone(),
            accepted_at: *accepted_at,
            transition_type: payload.transition_type,
        });
        match payload.transition_type {
            CodingSessionAuthorityTransitionType::GrantOperator => {
                grants.insert(
                    payload.grantee_pubkey.clone(),
                    CodingSessionTeamActiveGrant {
                        actor_pubkey: payload.grantee_pubkey.clone(),
                        grant_event_ref: event_id.clone(),
                        may_steer: true,
                    },
                );
            }
            CodingSessionAuthorityTransitionType::GrantViewer => {
                grants.insert(
                    payload.grantee_pubkey.clone(),
                    CodingSessionTeamActiveGrant {
                        actor_pubkey: payload.grantee_pubkey.clone(),
                        grant_event_ref: event_id.clone(),
                        may_steer: false,
                    },
                );
            }
            CodingSessionAuthorityTransitionType::Revoke => {
                if grants.remove(&payload.grantee_pubkey).is_none() {
                    return Err("revoke names a pubkey with no active grant".into());
                }
            }
            CodingSessionAuthorityTransitionType::GrantSeat => {
                let role = payload
                    .role
                    .clone()
                    .ok_or_else(|| "grant-seat requires role".to_owned())?;
                seats.insert(
                    payload.grantee_pubkey.clone(),
                    CodingSessionTeamActiveSeat {
                        actor_pubkey: payload.grantee_pubkey.clone(),
                        role,
                    },
                );
                seat_grant_refs.insert(payload.grantee_pubkey.clone(), event_id.clone());
            }
            CodingSessionAuthorityTransitionType::RevokeSeat => {
                let expected_role = payload
                    .role
                    .as_deref()
                    .ok_or_else(|| "revoke-seat requires role".to_owned())?;
                match seats.get(&payload.grantee_pubkey) {
                    Some(seat) if seat.role == expected_role => {
                        seats.remove(&payload.grantee_pubkey);
                        seat_grant_refs.remove(&payload.grantee_pubkey);
                    }
                    Some(_) => return Err("revoke-seat role does not match active seat".into()),
                    None => return Err("revoke-seat names no active seat".into()),
                }
            }
        }
        expected_prev = Some(event_id);
    }
    Ok(ProjectedAuthority {
        grants: grants.into_values().collect(),
        seats: seats.into_values().collect(),
        seat_grant_refs,
        head_event_id: expected_prev.map(str::to_owned),
        head_seq: u32::try_from(links.len())
            .map_err(|_| "authority chain exceeds u32 sequence space".to_owned())?,
        policy_grants,
    })
}
