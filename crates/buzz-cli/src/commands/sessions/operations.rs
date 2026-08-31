//! Signed team-transaction transport for `bee sessions`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};

use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionPayload,
    CodingSessionAuthorityTransitionType, CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use buzz_core::coding_session_genesis::{
    decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
};
use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, CodingSessionTeamAcknowledgement,
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat, CodingSessionTeamAssignment,
    CodingSessionTeamFold, CodingSessionTeamFoldContext, CodingSessionTeamMissionBlocked,
    CodingSessionTeamMissionCompleted, CodingSessionTeamReport, CodingSessionTeamTransactionBody,
    CodingSessionTeamTransactionType, CodingSessionTeamVerdict,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_TEAM_TRANSACTION, KIND_SYSTEM_MESSAGE,
};
use buzz_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
    parse_coding_session_team_transaction,
};
use nostr::Event;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_uuid};
use crate::{TeamOperationCmd, TeamTransactionWriteArgs};

/// Publish one operation after strict local structural validation.
pub async fn cmd_write(
    client: &BuzzClient,
    args: TeamTransactionWriteArgs,
    transaction_type: CodingSessionTeamTransactionType,
) -> Result<(), CliError> {
    validate_coordinates(&args.channel, &args.session_ref, &args.genesis)?;
    let body_value = read_json_argument(&args.body)?;
    let body = decode_body(transaction_type, body_value)?;
    let delivery_command_id = match (&args.wake_to, args.delivery_command_id) {
        (Some(_), Some(command_id)) => Some(command_id),
        (Some(_), None) => Some(Uuid::new_v4().to_string()),
        (None, command_id) => command_id,
    };
    let payload = coding_session_team_transaction_payload(
        args.session_ref.clone(),
        args.genesis.clone(),
        args.supersedes,
        delivery_command_id.clone(),
        body,
    );
    let builder = build_coding_session_team_transaction(&args.channel, payload)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    // The NIP-CSTX envelope is exactly five tags. NIP-OA remains on the HTTP
    // request header; injecting it into the signed event would invalidate the
    // public protocol record.
    let event = client.sign_event_unchecked(builder)?;

    if transaction_type == CodingSessionTeamTransactionType::MissionCompleted {
        verify_completion_before_submit(
            client,
            &args.channel,
            &args.session_ref,
            &args.genesis,
            &event,
        )
        .await?;
    }

    let operation_id = event.id.to_hex();
    let raw = client.submit_event(event).await?;
    let response = crate::commands::parse_write_response(&raw, "team transaction already stored")?;
    let mut output: Value = serde_json::from_str(&response)
        .map_err(|error| CliError::Other(format!("relay response is not JSON: {error}")))?;
    if let (Some(wake_to), Some(command_id)) =
        (args.wake_to.as_deref(), delivery_command_id.as_deref())
    {
        let wake = super::crew_cmds::send_team_operation_wake(
            client,
            &args.channel,
            wake_to,
            &args.session_ref,
            command_id,
            &operation_id,
            transaction_type.as_str(),
        )
        .await;
        let delivery = match wake {
            Ok(value) => value,
            Err(error) => json!({
                "accepted": null,
                "status": "unconfirmed",
                "error": error.to_string(),
                "recordedOperationId": operation_id,
            }),
        };
        if let Some(object) = output.as_object_mut() {
            object.insert("delivery".into(), delivery);
        }
    }
    println!("{output}");
    Ok(())
}

/// Read one or all operations with signed provenance and fold disclosure.
pub async fn cmd_read(client: &BuzzClient, cmd: TeamOperationCmd) -> Result<(), CliError> {
    let (channel, session_ref, genesis, wanted) = match cmd {
        TeamOperationCmd::Get {
            channel,
            session_ref,
            genesis,
            id,
        } => {
            validate_lower_hex64("--id", &id)?;
            let coordinates = match (channel, session_ref, genesis) {
                (Some(channel), Some(session_ref), Some(genesis)) => {
                    validate_coordinates(&channel, &session_ref, &genesis)?;
                    OperationCoordinates {
                        channel,
                        session_ref,
                        genesis,
                    }
                }
                (None, None, None) => resolve_operation_coordinates(client, &id).await?,
                _ => {
                    return Err(CliError::Usage(
                        "pass --channel, --session-ref, and --genesis together, or omit all three and resolve scope from the signed operation"
                            .into(),
                    ));
                }
            };
            (
                coordinates.channel,
                coordinates.session_ref,
                coordinates.genesis,
                Some(id),
            )
        }
        TeamOperationCmd::List {
            channel,
            session_ref,
            genesis,
        } => (channel, session_ref, genesis, None),
    };
    validate_coordinates(&channel, &session_ref, &genesis)?;

    let events = fetch_transactions(client, &channel, &session_ref, &genesis).await?;
    let context = fetch_founder_context(client, &channel, &session_ref, &genesis).await?;
    let fold = fold_coding_session_team_transactions(&events, &context)
        .map_err(|error| CliError::Other(format!("team-operation fold failed: {error}")))?;
    let rows: Vec<Value> = events
        .iter()
        .filter(|event| wanted.as_deref().is_none_or(|id| event.id.to_hex() == id))
        .map(|event| operation_json(event, &fold))
        .collect::<Result<_, _>>()?;
    if wanted.is_some() && rows.is_empty() {
        return Err(CliError::NotFound(
            "team operation not found in this session".into(),
        ));
    }

    println!(
        "{}",
        json!({
            "operations": rows,
            "fold": fold_json(&fold),
            "authorityContext": {
                "founder": context.founder_pubkey,
            "activeSeats": context.active_seats.iter().map(|seat| json!({
                "actorPubkey": seat.actor_pubkey,
                "role": seat.role,
            })).collect::<Vec<_>>(),
            "activeGrants": context.active_grants.iter().map(|grant| json!({
                "actorPubkey": grant.actor_pubkey,
                "grantEventRef": grant.grant_event_ref,
                "maySteer": grant.may_steer,
            })).collect::<Vec<_>>(),
            "source": "relay-receipt-backed accepted kind 44228 authority chain"
            }
        })
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperationCoordinates {
    channel: String,
    session_ref: String,
    genesis: String,
}

/// Resolve the context of an operation pointer from the exact signed record.
///
/// Managed seats receive relay credentials, not unsigned session coordinates.
/// A kind-44220 wake therefore needs only the operation event id: this exact-id
/// query verifies the record before its signed `h`, `d`, and `cstx-genesis`
/// values are allowed to scope the canonical fold.
async fn resolve_operation_coordinates(
    client: &BuzzClient,
    operation_id: &str,
) -> Result<OperationCoordinates, CliError> {
    let values = client
        .query_all(operation_pointer_query_filter(operation_id))
        .await?;
    if values.len() != 1 {
        return Err(CliError::NotFound(format!(
            "expected exactly one signed team operation {operation_id}, found {}",
            values.len()
        )));
    }
    operation_coordinates_from_value(&values[0], operation_id)
}

fn operation_pointer_query_filter(operation_id: &str) -> Value {
    json!({
        "ids": [operation_id],
        "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
    })
}

fn operation_coordinates_from_value(
    value: &Value,
    operation_id: &str,
) -> Result<OperationCoordinates, CliError> {
    let event: Event = serde_json::from_value(value.clone())
        .map_err(|error| CliError::Other(format!("relay returned malformed operation: {error}")))?;
    if event.id.to_hex() != operation_id {
        return Err(CliError::Other(
            "relay returned an operation other than the requested event id".into(),
        ));
    }
    buzz_core::verify_event(&event)
        .map_err(|error| CliError::Other(format!("invalid operation signature: {error}")))?;
    let payload = parse_coding_session_team_transaction(&event)
        .map_err(|error| CliError::Other(format!("invalid team operation: {error}")))?;
    let channel = event
        .tags
        .iter()
        .next()
        .and_then(|tag| tag.as_slice().get(1))
        .cloned()
        .ok_or_else(|| CliError::Other("verified operation has no channel tag".into()))?;
    let coordinates = OperationCoordinates {
        channel,
        session_ref: payload.session_ref,
        genesis: payload.genesis_ref,
    };
    validate_coordinates(
        &coordinates.channel,
        &coordinates.session_ref,
        &coordinates.genesis,
    )?;
    Ok(coordinates)
}

fn validate_coordinates(channel: &str, session_ref: &str, genesis: &str) -> Result<(), CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;
    validate_lower_hex64("--genesis", genesis)
}

fn read_json_argument(input: &str) -> Result<Value, CliError> {
    let raw = if input == "-" {
        let mut raw = String::new();
        io::stdin()
            .read_to_string(&mut raw)
            .map_err(|error| CliError::Other(format!("failed to read stdin: {error}")))?;
        raw
    } else if let Some(path) = input.strip_prefix('@') {
        fs::read_to_string(path)
            .map_err(|error| CliError::Usage(format!("failed to read body file {path}: {error}")))?
    } else {
        input.to_owned()
    };
    serde_json::from_str(&raw)
        .map_err(|error| CliError::Usage(format!("invalid --body JSON: {error}")))
}

fn typed<T: DeserializeOwned>(value: Value, label: &str) -> Result<T, CliError> {
    serde_json::from_value(value)
        .map_err(|error| CliError::Usage(format!("invalid {label} body: {error}")))
}

fn decode_body(
    transaction_type: CodingSessionTeamTransactionType,
    value: Value,
) -> Result<CodingSessionTeamTransactionBody, CliError> {
    Ok(match transaction_type {
        CodingSessionTeamTransactionType::Assignment => {
            CodingSessionTeamTransactionBody::Assignment(typed::<CodingSessionTeamAssignment>(
                value,
                "assignment",
            )?)
        }
        CodingSessionTeamTransactionType::Report => CodingSessionTeamTransactionBody::Report(
            typed::<CodingSessionTeamReport>(value, "report")?,
        ),
        CodingSessionTeamTransactionType::Verdict => CodingSessionTeamTransactionBody::Verdict(
            typed::<CodingSessionTeamVerdict>(value, "verdict")?,
        ),
        CodingSessionTeamTransactionType::Acknowledgement => {
            CodingSessionTeamTransactionBody::Acknowledgement(typed::<
                CodingSessionTeamAcknowledgement,
            >(
                value, "acknowledgement"
            )?)
        }
        CodingSessionTeamTransactionType::MissionCompleted => {
            CodingSessionTeamTransactionBody::MissionCompleted(typed::<
                CodingSessionTeamMissionCompleted,
            >(
                value, "mission.completed"
            )?)
        }
        CodingSessionTeamTransactionType::MissionBlocked => {
            CodingSessionTeamTransactionBody::MissionBlocked(typed::<
                CodingSessionTeamMissionBlocked,
            >(
                value, "mission.blocked"
            )?)
        }
    })
}

async fn fetch_transactions(
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

fn transaction_value_matches_context(
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

fn transaction_query_filter(channel: &str, session_ref: &str, genesis: &str) -> Value {
    json!({
        "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
        "#h": [channel],
        "#d": [session_ref],
        "#cstx-genesis": [genesis],
    })
}

fn transaction_matches_context(
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

pub(super) async fn fetch_founder_context(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<CodingSessionTeamFoldContext, CliError> {
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
    Ok(CodingSessionTeamFoldContext {
        channel_ref: channel.to_owned(),
        session_ref: session_ref.to_owned(),
        genesis_ref: genesis.to_owned(),
        founder_pubkey: event.pubkey.to_hex(),
        active_seats: authority.seats,
        active_grants: authority.grants,
    })
}

const AUTHORITY_ACCEPTANCE_RECEIPT_TYPE: &str = "coding_session_authority_transition_accepted";

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

fn project_receipt_backed_authority_chain(
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
        let accepted = (receipt.accepted_event_id.clone(), signer, payload);
        if accepted_by_seq.insert(receipt.seq, accepted).is_some() {
            return Err("conflicting authority receipts claim the same sequence".into());
        }
    }

    let links: Vec<_> = accepted_by_seq.into_values().collect();
    let mut expected_prev: Option<&str> = None;
    let mut grants = BTreeMap::new();
    let mut seats = BTreeMap::new();
    let mut seat_grant_refs = BTreeMap::new();
    for (offset, (event_id, signer, payload)) in links.iter().enumerate() {
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
    })
}

async fn verify_completion_before_submit(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
    candidate: &Event,
) -> Result<(), CliError> {
    let mut events = fetch_transactions(client, channel, session_ref, genesis).await?;
    events.push(candidate.clone());
    let context = fetch_founder_context(client, channel, session_ref, genesis).await?;
    let fold = fold_coding_session_team_transactions(&events, &context)
        .map_err(|error| CliError::Usage(format!("completion verification failed: {error}")))?;
    let candidate_id = candidate.id.to_hex();
    let is_terminal = fold
        .canonical_terminal
        .as_ref()
        .is_some_and(|terminal| terminal.event_id == candidate_id);
    if !is_terminal {
        return Err(CliError::Usage(
            "completion refused: every referenced assignment must have an active report, an approving disposition, and the assigned actor's acknowledgement"
                .into(),
        ));
    }
    Ok(())
}

fn operation_json(event: &Event, fold: &CodingSessionTeamFold) -> Result<Value, CliError> {
    let payload = parse_coding_session_team_transaction(event)
        .map_err(|error| CliError::Other(error.to_string()))?;
    let id = event.id.to_hex();
    let exclusion = fold.excluded.iter().find(|item| item.event_id == id);
    Ok(json!({
        "id": id,
        "pubkey": event.pubkey.to_hex(),
        "createdAt": event.created_at.as_secs(),
        "sig": event.sig.to_string(),
        "kind": KIND_CODING_SESSION_TEAM_TRANSACTION,
        "tags": event.tags,
        "payload": payload,
        "canonical": fold.included_event_ids.contains(&event.id.to_hex()),
        "exclusion": exclusion.map(|item| json!({
            "code": format!("{:?}", item.code),
            "reason": item.reason,
        })),
    }))
}

fn fold_json(fold: &CodingSessionTeamFold) -> Value {
    json!({
        "includedEventIds": fold.included_event_ids,
        "excluded": fold.excluded.iter().map(|item| json!({
            "eventId": item.event_id,
            "code": format!("{:?}", item.code),
            "reason": item.reason,
        })).collect::<Vec<_>>(),
        "conflicts": fold.conflicts.iter().map(|item| json!({
            "subject": item.subject,
            "winnerEventId": item.winner_event_id,
            "contenderEventIds": item.contender_event_ids,
        })).collect::<Vec<_>>(),
        "assignments": fold.assignments.iter().map(|item| json!({
            "assignmentEventId": item.assignment_event_id,
            "governedReportEventId": item.governed_report_event_id,
            "dispositionEventId": item.disposition_event_id,
            "acknowledgementEventId": item.acknowledgement_event_id,
            "settled": item.settled,
        })).collect::<Vec<_>>(),
        "canonicalTerminal": fold.canonical_terminal.as_ref().map(|item| json!({
            "eventId": item.event_id,
            "type": item.transaction_type.as_str(),
        })),
    })
}

#[cfg(test)]
mod tests {
    use buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    use super::*;

    const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
    const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

    fn seat_event(
        signer: &Keys,
        transition_type: CodingSessionAuthorityTransitionType,
        grantee: &str,
        role: &str,
        seq: u32,
        previous: Option<&Event>,
    ) -> Event {
        let payload = match transition_type {
            CodingSessionAuthorityTransitionType::GrantSeat => {
                CodingSessionAuthorityTransitionPayload::new_grant_seat(
                    GENESIS,
                    previous.map(|event| event.id.to_hex()),
                    seq,
                    grantee,
                    role,
                )
            }
            CodingSessionAuthorityTransitionType::RevokeSeat => {
                CodingSessionAuthorityTransitionPayload::new_revoke_seat(
                    GENESIS,
                    previous.map(|event| event.id.to_hex()),
                    seq,
                    grantee,
                    role,
                )
            }
            _ => panic!("seat helper requires a seat transition"),
        };
        let tags = [
            Tag::parse(["h", CHANNEL]).expect("h"),
            Tag::parse(["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION])
                .expect("version"),
            Tag::parse(["csat-genesis", GENESIS]).expect("genesis"),
        ];
        EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
            serde_json::to_string(&payload).expect("payload"),
        )
        .tags(tags)
        .sign_with_keys(signer)
        .expect("sign")
    }

    fn authority_receipt(transition: &Event, relay: &Keys) -> Event {
        let payload = decode_coding_session_authority_transition(&transition.content)
            .expect("transition payload");
        let mut content = json!({
            "type": AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
            "genesisRef": payload.genesis_ref,
            "acceptedEventId": transition.id.to_hex(),
            "seq": payload.seq,
            "transitionType": payload.transition_type,
            "granteePubkey": payload.grantee_pubkey,
        });
        if let (Some(object), Some(role)) = (content.as_object_mut(), payload.role) {
            object.insert("role".into(), Value::String(role));
        }
        EventBuilder::new(
            Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
            content.to_string(),
        )
        .tags([Tag::parse(["h", CHANNEL]).expect("h")])
        .sign_with_keys(relay)
        .expect("receipt")
    }

    fn project_authority_chain(
        events: &[Event],
        channel: &str,
        genesis: &str,
        founder: &str,
    ) -> Result<ProjectedAuthority, String> {
        let relay = Keys::generate();
        let receipts: Vec<Event> = events
            .iter()
            .map(|event| authority_receipt(event, &relay))
            .collect();
        project_receipt_backed_authority_chain(
            events,
            &receipts,
            channel,
            genesis,
            founder,
            &relay.public_key().to_hex(),
        )
    }

    #[test]
    fn operation_command_rejects_a_body_for_the_wrong_operation() {
        let report = json!({
            "assignmentRef": "11".repeat(32),
            "summary": "done",
            "branch": null,
            "baseSha": null,
            "headSha": null,
            "files": [],
            "tests": [],
            "redBeforeGreen": null,
            "deviations": [],
            "residuals": [],
            "anomalies": []
        });
        assert!(decode_body(CodingSessionTeamTransactionType::Assignment, report).is_err());
    }

    #[test]
    fn body_file_syntax_does_not_guess_plain_strings_are_paths() {
        let body = read_json_argument(r#"{"status":"received"}"#).expect("inline JSON");
        assert_eq!(body["status"], "received");
    }

    #[test]
    fn authority_projection_folds_lead_verifier_role_change_and_revoke() {
        let founder = Keys::generate();
        let lead = Keys::generate();
        let worker = Keys::generate();
        let lead_grant = seat_event(
            &founder,
            CodingSessionAuthorityTransitionType::GrantSeat,
            &lead.public_key().to_hex(),
            "lead",
            1,
            None,
        );
        let verifier_grant = seat_event(
            &lead,
            CodingSessionAuthorityTransitionType::GrantSeat,
            &worker.public_key().to_hex(),
            "verifier",
            2,
            Some(&lead_grant),
        );
        let role_change = seat_event(
            &founder,
            CodingSessionAuthorityTransitionType::GrantSeat,
            &worker.public_key().to_hex(),
            "builder",
            3,
            Some(&verifier_grant),
        );
        let authority = project_authority_chain(
            &[
                lead_grant.clone(),
                verifier_grant.clone(),
                role_change.clone(),
            ],
            CHANNEL,
            GENESIS,
            &founder.public_key().to_hex(),
        )
        .expect("seat projection");
        assert!(authority.seats.iter().any(|seat| {
            seat.actor_pubkey == lead.public_key().to_hex() && seat.role == "lead"
        }));
        assert!(authority.seats.iter().any(|seat| {
            seat.actor_pubkey == worker.public_key().to_hex() && seat.role == "builder"
        }));

        let revoke = seat_event(
            &founder,
            CodingSessionAuthorityTransitionType::RevokeSeat,
            &worker.public_key().to_hex(),
            "builder",
            4,
            Some(&role_change),
        );
        let authority = project_authority_chain(
            &[lead_grant, verifier_grant, role_change, revoke],
            CHANNEL,
            GENESIS,
            &founder.public_key().to_hex(),
        )
        .expect("revoke projection");
        assert!(!authority
            .seats
            .iter()
            .any(|seat| seat.actor_pubkey == worker.public_key().to_hex()));
    }
}

#[cfg(test)]
#[path = "operations_receipt_tests.rs"]
mod receipt_tests;
