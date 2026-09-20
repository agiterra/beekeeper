//! Signed team-transaction transport for `bee sessions`.

use std::fs;
use std::future::Future;
use std::io::{self, Read};

use buzz_core::coding_session_team_transaction::{
    fold_coding_session_team_transactions, CodingSessionTeamAcknowledgement,
    CodingSessionTeamActiveSeat, CodingSessionTeamAssignment, CodingSessionTeamDecisionAnswer,
    CodingSessionTeamDecisionChoice, CodingSessionTeamDecisionRequest, CodingSessionTeamFold,
    CodingSessionTeamMissionBlocked, CodingSessionTeamMissionCompleted, CodingSessionTeamNote,
    CodingSessionTeamReport, CodingSessionTeamTransactionBody, CodingSessionTeamTransactionType,
    CodingSessionTeamVerdict, CODING_SESSION_TEAM_DECISION_FOUNDER,
};
use buzz_core::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use buzz_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
    parse_coding_session_team_transaction,
};
use nostr::Event;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use uuid::Uuid;

pub(super) use super::operations_authority::{fetch_projected_authority, ProjectedAuthority};
use super::operations_precheck::{precheck_operation, PrecheckRequest, PrecheckedOperation};
use super::operations_reads::{fetch_founder_context, fetch_transactions};
use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_uuid};
use crate::{TeamDecisionCmd, TeamNoteArgs, TeamOperationCmd, TeamTransactionWriteArgs};

/// Whether this operation class hands its stored `deliveryCommandId` straight
/// to its 44220 wake.
///
/// Only an assignment does. Its record names the command that will carry it to
/// the assignee, and the provider reads exactly that pairing back as the
/// binding that makes a finished turn owe a report
/// (`crates/buzz-session-provider/src/team_wake.rs`, `payload.delivery_command_id
/// == Some(command_id)`). Every other class is *answering* an assignment, so
/// the only id a caller could hand it is one already spent on the assignee's
/// own target; the lead runner then fences the wake as `AlreadyConsumed` and
/// the lead is never woken (`docs/SESSION_STATE.md` item 103, finding 4).
pub(super) const fn wake_shares_delivery_command_id(
    transaction_type: CodingSessionTeamTransactionType,
) -> bool {
    matches!(
        transaction_type,
        CodingSessionTeamTransactionType::Assignment
    )
}

/// The `deliveryCommandId` this operation's stored 44244 record carries.
///
/// An assignment with `--wake-to` mints one when the caller named none, so the
/// record and its wake share the id the provider's binding check compares. For
/// every other class the wake's id is derived from the operation *after* it is
/// signed, so no pre-sign value can name it: a caller-supplied id there would
/// be recorded as this operation's delivery while a different command actually
/// delivered it. That is refused rather than silently ignored.
pub(super) fn resolve_delivery_command_id(
    transaction_type: CodingSessionTeamTransactionType,
    wake_to: Option<&str>,
    requested: Option<String>,
) -> Result<Option<String>, CliError> {
    match (wake_to, requested) {
        (None, requested) => Ok(requested),
        (Some(_), requested) if wake_shares_delivery_command_id(transaction_type) => Ok(Some(
            requested.unwrap_or_else(|| Uuid::new_v4().to_string()),
        )),
        (Some(_), Some(_)) => Err(CliError::Usage(format!(
            "--delivery-command-id cannot be combined with --wake-to on a {}: the wake's \
             command id is derived from this operation and its exact target, and reusing the \
             command that already delivered an assignment makes the lead runner fence the wake \
             as AlreadyConsumed. Drop --delivery-command-id.",
            transaction_type.as_str()
        ))),
        (Some(_), None) => Ok(None),
    }
}

/// Publish one operation after strict local structural validation.
pub async fn cmd_write(
    client: &BuzzClient,
    args: TeamTransactionWriteArgs,
    transaction_type: CodingSessionTeamTransactionType,
) -> Result<(), CliError> {
    let body_value = read_json_argument(&args.body)?;
    let body = decode_body(transaction_type, body_value)?;
    publish_operation(
        client,
        PublishOperation {
            channel: args.channel,
            session_ref: args.session_ref,
            genesis: args.genesis,
            supersedes: args.supersedes,
            delivery_command_id: args.delivery_command_id,
            // A generic write says nothing about a wake it never sought; only
            // the verbs that resolve a target for themselves can (REVIEW-L1 F4).
            wake_omission: args.wake_to.is_none().then_some(WakeOmission::NotRequested),
            wake_to: args.wake_to,
            body,
        },
    )
    .await
}

/// One typed operation ready to sign, publish, and optionally wake a seat for.
///
/// Every `bee sessions` write verb funnels through this shape so that record
/// publication, the completion pre-check, wake correlation, and the JSON
/// answer are defined exactly once.
struct PublishOperation {
    channel: String,
    session_ref: String,
    genesis: String,
    supersedes: Option<String>,
    delivery_command_id: Option<String>,
    wake_to: Option<String>,
    /// Why no wake was published, when none was. Reported under `delivery`,
    /// never left as an absent key (REVIEW-L1 F4).
    wake_omission: Option<WakeOmission>,
    body: CodingSessionTeamTransactionBody,
}

/// Why a record that could have woken somebody did not.
///
/// Three facts that used to be one absent key, and only one of them is a
/// problem (REVIEW-L1 F4). "Empty means unknown" is exactly the shape §0.8 and
/// I9 forbid: a reader could not tell a ruling held on a person from a ruling
/// nobody will hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WakeOmission {
    /// The caller asked for no wake, and none was implied.
    NotRequested,
    /// The ruling is held on the founder, who is a person and not an
    /// execution. Nothing to wake, and the Mission rail is how they find out.
    FounderHeld,
    /// The ruling is held on an actor holding no active seat in this session.
    /// **This is the one that is a problem**: the party the mission is waiting
    /// on will not hear about it until somebody looks.
    NoSeat {
        /// The pubkey the request named, in full, so the reader can act on it.
        held_on: String,
    },
}

/// The `delivery` value a record with no wake reports.
///
/// Always the same three keys plus `heldOn`, so a reader parses one field
/// whether a wake happened or not, and every sentence says nobody was woken.
pub(super) fn wake_omission_delivery(omission: &WakeOmission) -> Value {
    let (status, held_on, message) = match omission {
        WakeOmission::NotRequested => (
            "not-requested",
            Value::Null,
            "no wake was asked for, so nobody was woken".to_owned(),
        ),
        WakeOmission::FounderHeld => (
            "founder-held",
            Value::Null,
            "this ruling is held on the founder, who is a person rather than an execution, so \
             nobody was woken — the Mission rail's waiting state is how they find out"
                .to_owned(),
        ),
        WakeOmission::NoSeat { held_on } => (
            "no-seat",
            json!(held_on),
            format!(
                "{} holds no active seat in this session, so nobody was woken: grant it a seat \
                 (`bee sessions grant-seat`) or answer the ruling yourself",
                super::crew::short_pubkey(held_on)
            ),
        ),
    };
    json!({
        "published": false,
        "status": status,
        "heldOn": held_on,
        "message": message,
    })
}

async fn publish_operation(
    client: &BuzzClient,
    operation: PublishOperation,
) -> Result<(), CliError> {
    validate_coordinates(
        &operation.channel,
        &operation.session_ref,
        &operation.genesis,
    )?;
    let transaction_type = operation.body.transaction_type();
    let delivery_command_id = resolve_delivery_command_id(
        transaction_type,
        operation.wake_to.as_deref(),
        operation.delivery_command_id,
    )?;
    // Refuse before signing (B2.5). The fold learned to exclude a bad record
    // rather than fail the session closed; nothing stopped a seat writing one,
    // and every record that forced B1b's four rounds would still be published
    // today (REVIEW-B1b F2). The candidate payload is built first so the check
    // reads the same `causalReferences` and `supersedes` the wire would carry.
    let signer_pubkey = client.keys().public_key().to_hex();
    let candidate = coding_session_team_transaction_payload(
        operation.session_ref.clone(),
        operation.genesis.clone(),
        operation.supersedes.clone(),
        delivery_command_id.clone(),
        operation.body.clone(),
    );
    let prechecked = precheck_operation(
        client,
        PrecheckRequest {
            channel: &operation.channel,
            session_ref: &operation.session_ref,
            genesis: &operation.genesis,
            signer_pubkey: &signer_pubkey,
            payload: &candidate,
        },
    )
    .await?;
    let payload = coding_session_team_transaction_payload(
        operation.session_ref.clone(),
        operation.genesis.clone(),
        prechecked.supersedes.clone(),
        delivery_command_id.clone(),
        operation.body,
    );
    let builder = build_coding_session_team_transaction(&operation.channel, payload)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    // The NIP-CSTX envelope is exactly five tags. NIP-OA remains on the HTTP
    // request header; injecting it into the signed event would invalidate the
    // public protocol record.
    let event = client.sign_event_unchecked(builder)?;

    // A completion whose prerequisites are missing is published, not refused:
    // it is early, the fold makes it terminal when they arrive, and refusing
    // it is what made a lead wake six seats for receipts (ledger 179(a)).
    // Every other refusal still happens before the record reaches the relay.
    let completion_outcome = match transaction_type {
        CodingSessionTeamTransactionType::MissionCompleted => Some(
            completion::classify_completion_before_submit(&prechecked, &event)?,
        ),
        _ => None,
    };

    let operation_id = event.id.to_hex();
    // Only an assignment hands its stored `deliveryCommandId` to the wake; see
    // [`wake_shares_delivery_command_id`]. Everything else lets
    // `send_team_operation_wake` derive one from this operation and the
    // *resolved* `execution.target_key` — not from the `--wake-to` string the
    // caller typed, which names the same seat by a different word and would
    // mint a different id for the same operation (REVIEW-B1c F6).
    let shared_command_id = wake_shares_delivery_command_id(transaction_type)
        .then(|| delivery_command_id.clone())
        .flatten();
    let wake = operation.wake_to.as_deref().map(|wake_to| {
        || {
            super::crew_cmds::send_team_operation_wake(
                client,
                &operation.channel,
                wake_to,
                &operation.session_ref,
                shared_command_id.as_deref(),
                &operation_id,
                transaction_type.as_str(),
            )
        }
    });
    let mut output = submit_record_then_wake(
        &operation_id,
        || client.submit_event(event),
        wake,
        operation.wake_omission,
    )
    .await?;
    disclose_adopted_correction(
        &mut output,
        transaction_type,
        operation.supersedes.as_deref(),
        prechecked.supersedes.as_deref(),
    );
    if let Some(outcome) = &completion_outcome {
        completion::disclose_completion_outcome(&mut output, outcome);
    }
    println!("{output}");
    Ok(())
}

/// Say, in the answer, that a completion corrected a terminal the caller did
/// not name.
///
/// `bee sessions complete` adopts the lead's own canonical `mission.blocked` as
/// its `supersedes` when the caller passed none (B2.7, live finding 14). That
/// is the right record to sign — but the author asked to "publish a completion"
/// and published "a correction of my own terminal", a materially different
/// signed record, and the only output they see said nothing about it
/// (REVIEW-B2 F4).
///
/// `supersedes` is **present and null** when a completion corrected nothing, so
/// "corrected nothing" and "did not say" stay different answers. It is absent
/// for every other verb, whose `--supersedes` is exactly what the caller typed.
pub(super) fn disclose_adopted_correction(
    output: &mut Value,
    transaction_type: CodingSessionTeamTransactionType,
    requested: Option<&str>,
    effective: Option<&str>,
) {
    if transaction_type != CodingSessionTeamTransactionType::MissionCompleted {
        return;
    }
    let Some(object) = output.as_object_mut() else {
        return;
    };
    object.insert(
        "supersedes".into(),
        effective.map_or(Value::Null, |id| Value::String(id.to_owned())),
    );
    if requested.is_none() {
        if let Some(adopted) = effective {
            object.insert(
                "correctedTerminal".into(),
                Value::String(format!(
                    "this completion corrects your mission.blocked {adopted}, which was this \
                     session's canonical terminal, so the fold sees one corrected terminal \
                     rather than a conflict"
                )),
            );
        }
    }
}

/// Publish a `note`: something said, with nothing changed.
///
/// A note carries no `--supersedes` and no `--wake-to` by construction. It can
/// never correct another record, and waking a seat for a record that changes
/// no state would spend a turn to deliver an interruption.
pub async fn cmd_note(client: &BuzzClient, args: TeamNoteArgs) -> Result<(), CliError> {
    for reference in &args.refs {
        validate_lower_hex64("--ref", reference)?;
    }
    publish_operation(
        client,
        PublishOperation {
            channel: args.channel,
            session_ref: args.session_ref,
            genesis: args.genesis,
            supersedes: None,
            delivery_command_id: None,
            wake_to: None,
            wake_omission: Some(WakeOmission::NotRequested),
            body: CodingSessionTeamTransactionBody::Note(CodingSessionTeamNote {
                text: args.text,
                refs: args.refs,
            }),
        },
    )
    .await
}

/// Publish a `decision.request` or a `decision.answer`.
pub async fn cmd_decide(client: &BuzzClient, cmd: TeamDecisionCmd) -> Result<(), CliError> {
    match cmd {
        TeamDecisionCmd::Request {
            channel,
            session_ref,
            genesis,
            question,
            options,
            held_on,
            blocks,
            recommendation,
            supersedes,
            wake_to,
        } => {
            if held_on != CODING_SESSION_TEAM_DECISION_FOUNDER {
                validate_lower_hex64("--held-on", &held_on)?;
            }
            for reference in &blocks {
                validate_lower_hex64("--blocks", reference)?;
            }
            // Finding 19 (live run 2): Bob's request, held on Keystone's
            // pubkey, published NO wake, so the party the mission was waiting
            // on learned nothing until a person looked. `decide answer` has
            // defaulted its wake to the asker's role since REVIEW-B1c F4;
            // `decide request` defaulted nothing, and `--wake-to` speaks
            // cs-target keys, session ids and role slugs — never pubkeys —
            // while `heldOn` is exactly a pubkey, so no caller had a name for
            // the held-on party either. Resolve it here, through the same
            // receipt-backed 44228 projection the fold uses. A founder-held
            // request still wakes nobody and does not even query: the founder
            // is a person, not an execution.
            let (wake_to, wake_omission) = match wake_to {
                Some(explicit) => (Some(explicit), None),
                None if held_on == CODING_SESSION_TEAM_DECISION_FOUNDER => {
                    (None, Some(WakeOmission::FounderHeld))
                }
                None => {
                    let context =
                        fetch_founder_context(client, &channel, &session_ref, &genesis).await?;
                    match held_on_wake_role(&held_on, &context.active_seats) {
                        Some(role) => (Some(role), None),
                        None => (
                            None,
                            Some(WakeOmission::NoSeat {
                                held_on: held_on.clone(),
                            }),
                        ),
                    }
                }
            };
            publish_operation(
                client,
                PublishOperation {
                    channel,
                    session_ref,
                    genesis,
                    supersedes,
                    // Derived from the stored request and its target, never
                    // inherited (REVIEW-B1c F6). The flag is gone from the
                    // surface, so there is nothing to inherit.
                    delivery_command_id: None,
                    wake_to,
                    wake_omission,
                    body: CodingSessionTeamTransactionBody::DecisionRequest(
                        CodingSessionTeamDecisionRequest {
                            question,
                            options,
                            held_on,
                            blocks,
                            recommendation,
                        },
                    ),
                },
            )
            .await
        }
        TeamDecisionCmd::Answer {
            channel,
            session_ref,
            genesis,
            request,
            choice_index,
            choice,
            note,
            condition,
            supersedes,
            wake_to,
        } => {
            validate_lower_hex64("--request", &request)?;
            let choice = match (choice_index, choice) {
                (Some(index), None) => CodingSessionTeamDecisionChoice::Index(index),
                (None, Some(text)) => CodingSessionTeamDecisionChoice::Text(text),
                _ => {
                    return Err(CliError::Usage(
                        "pass exactly one of --choice-index or --choice".into(),
                    ));
                }
            };
            // An answer nobody is told about is an answer that never lands,
            // and the batch forbids polling for it (REVIEW-B1c F4). Wake the
            // seat that asked, unless the caller named someone else.
            let (wake_to, wake_omission) = match wake_to {
                Some(explicit) => (Some(explicit), None),
                None => {
                    let (asker, role) =
                        resolve_asker_role(client, &channel, &session_ref, &genesis, &request)
                            .await?;
                    match role {
                        Some(role) => (Some(role), None),
                        // The asker holds no seat, so there is no execution to
                        // tell. Same disclosure as a request held on an
                        // unseated actor (REVIEW-L1 F4).
                        None => (None, Some(WakeOmission::NoSeat { held_on: asker })),
                    }
                }
            };
            publish_operation(
                client,
                PublishOperation {
                    channel,
                    session_ref,
                    genesis,
                    supersedes,
                    delivery_command_id: None,
                    wake_to,
                    wake_omission,
                    body: CodingSessionTeamTransactionBody::DecisionAnswer(
                        CodingSessionTeamDecisionAnswer {
                            request_ref: request,
                            choice,
                            note,
                            // Verbatim into the signed content. Nothing here
                            // trims, normalises or parses it: it is the class
                            // the ruling covers, in the answerer's own words.
                            condition,
                        },
                    ),
                },
            )
            .await
        }
    }
}

/// The role of the seated execution a request's `heldOn` names, if any.
///
/// `--wake-to` speaks cs-target keys, provider session ids and role slugs, and
/// never pubkeys, while `heldOn` is exactly a pubkey or the literal `founder`.
/// This is the one translation between them, and it reads the same
/// receipt-backed 44228 projection the fold does.
///
/// `None` twice over, and both mean *wake nobody*, honestly:
///
/// - **`founder`** is a person, not an execution. There is nothing to wake, and
///   the Mission rail saying "waiting on the founder" is how a person finds out.
/// - **An actor holding no active seat** has no execution either. Inventing one
///   would be a guess, so the caller reports [`WakeOmission::NoSeat`] under
///   `delivery` instead — the one case here that is actually a problem, and the
///   one the absent key used to hide (REVIEW-L1 F4).
pub(super) fn held_on_wake_role(
    held_on: &str,
    active_seats: &[CodingSessionTeamActiveSeat],
) -> Option<String> {
    if held_on == CODING_SESSION_TEAM_DECISION_FOUNDER {
        return None;
    }
    active_seats
        .iter()
        .find(|seat| seat.actor_pubkey == held_on)
        .map(|seat| seat.role.clone())
}

/// Resolve the seat role of the actor that published one `decision.request`.
///
/// The answer's default wake target. `--wake-to` speaks cs-target keys, session
/// ids and role slugs — never pubkeys — so the asker's *role*, read from the
/// same receipt-backed 44228 chain the fold uses, is the one name for that
/// actor the resolver understands.
///
/// Returns the asker's pubkey and its role. The role is `None` when the asker
/// holds no active seat: there is then no execution to wake, and inventing one
/// would be a guess. The answer is still published, and the omission is
/// reported explicitly under `delivery` as
/// [`WakeOmission::NoSeat`] — never as an absent key (REVIEW-L1 F4).
async fn resolve_asker_role(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
    request_id: &str,
) -> Result<(String, Option<String>), CliError> {
    let values = client
        .query_all(operation_pointer_query_filter(request_id))
        .await?;
    let [value] = values.as_slice() else {
        return Err(CliError::NotFound(format!(
            "expected exactly one signed decision.request {request_id}, found {}",
            values.len()
        )));
    };
    let event: Event = serde_json::from_value(value.clone())
        .map_err(|error| CliError::Other(format!("relay returned malformed request: {error}")))?;
    if event.id.to_hex() != request_id {
        return Err(CliError::Other(
            "relay returned an operation other than the requested event id".into(),
        ));
    }
    buzz_core::verify_event(&event)
        .map_err(|error| CliError::Other(format!("invalid request signature: {error}")))?;
    let payload = parse_coding_session_team_transaction(&event)
        .map_err(|error| CliError::Other(format!("invalid team operation: {error}")))?;
    if payload.transaction_type != CodingSessionTeamTransactionType::DecisionRequest {
        return Err(CliError::Usage(format!(
            "--request must name a decision.request; {request_id} is a {}",
            payload.transaction_type.as_str()
        )));
    }
    let context = fetch_founder_context(client, channel, session_ref, genesis).await?;
    let asker = event.pubkey.to_hex();
    let role = context
        .active_seats
        .iter()
        .find(|seat| seat.actor_pubkey == asker)
        .map(|seat| seat.role.clone());
    Ok((asker, role))
}

async fn submit_record_then_wake<Submit, SubmitFuture, Wake, WakeFuture>(
    operation_id: &str,
    submit: Submit,
    wake: Option<Wake>,
    omission: Option<WakeOmission>,
) -> Result<Value, CliError>
where
    Submit: FnOnce() -> SubmitFuture,
    SubmitFuture: Future<Output = Result<String, CliError>>,
    Wake: FnOnce() -> WakeFuture,
    WakeFuture: Future<Output = Result<Value, CliError>>,
{
    let raw = submit().await?;
    let response = crate::commands::parse_write_response(&raw, "team transaction already stored")?;
    let mut output: Value = serde_json::from_str(&response)
        .map_err(|error| CliError::Other(format!("relay response is not JSON: {error}")))?;
    if let Some(wake) = wake {
        let delivery = match wake().await {
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
    } else if let Some(omission) = omission {
        // Present and explicit. The absent key used to carry three different
        // meanings, one of them a real failure (REVIEW-L1 F4).
        if let Some(object) = output.as_object_mut() {
            object.insert("delivery".into(), wake_omission_delivery(&omission));
        }
    }
    Ok(output)
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
    let context = super::operations_verifier_gate::fetch_context_with_verifier_gate(
        client,
        &channel,
        &session_ref,
        &genesis,
    )
    .await?;
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
        CodingSessionTeamTransactionType::Note => {
            CodingSessionTeamTransactionBody::Note(typed::<CodingSessionTeamNote>(value, "note")?)
        }
        CodingSessionTeamTransactionType::DecisionRequest => {
            CodingSessionTeamTransactionBody::DecisionRequest(typed::<
                CodingSessionTeamDecisionRequest,
            >(
                value, "decision.request"
            )?)
        }
        CodingSessionTeamTransactionType::DecisionAnswer => {
            CodingSessionTeamTransactionBody::DecisionAnswer(typed::<
                CodingSessionTeamDecisionAnswer,
            >(
                value, "decision.answer"
            )?)
        }
    })
}

/// What a refused completion says when the fold attached no reason to this
/// event id.
///
/// The reachable path quotes the fold's own reason, which names the exact
/// assignment and report. This is the fallback for a completion the fold
/// simply did not make terminal without excluding it — and it is printed to a
/// person, so it is a constant with a test on it rather than a literal buried
/// in an `unwrap_or_else` (REVIEW-L7 F3, which found eighteen spaces in the
/// middle of this sentence).
pub(super) const COMPLETION_REFUSED_FALLBACK: &str =
    "every referenced assignment must have an active report, an approving disposition, and \
     the assigned actor's acknowledgement";

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
            "code": buzz_core::team_vocabulary::fold_exclusion_wire_code(item.code),
            "reason": item.reason,
        })),
    }))
}

// The completion outcome — terminal, waiting, or refused — lives in a sibling
// file for the same reason (ledger 183).
#[path = "operations_completion.rs"]
mod completion;

// The fold's JSON rendering lives in a sibling file so this one stays under
// the repository's 1,000-line ceiling (split, never bump).
#[path = "operations_fold_json.rs"]
mod fold_render;
use fold_render::fold_json;

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "operations_receipt_tests.rs"]
mod receipt_tests;
