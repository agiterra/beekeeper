//! `bee sessions send | create | inbox | status` — the wire side.
//!
//! Two envelope rules govern everything here, and both come from the relay's
//! own validators rather than from taste:
//!
//! - **Exactly three tags.** `validate_coding_session_command_envelope`
//!   (`crates/buzz-relay/src/handlers/ingest.rs`) rejects any 44220 tag that is
//!   not `h`, `cs-v`, or `cs-target`, and the 44221 validator is the same shape.
//!   So these two kinds are signed with
//!   [`BuzzClient::sign_event_unchecked`], never with `sign_event`, whose
//!   NIP-OA `auth` tag injection would make the event invalid. Membership
//!   delegation still reaches the relay: `submit_event` sends the same tag in
//!   the `x-auth-tag` header, which is where `POST /events` reads it
//!   (`crates/buzz-relay/src/api/bridge.rs`).
//! - **`deliver` is omitted at its default.** See [`build_turn_command`].

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use uuid::Uuid;

use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionDelivery, CODING_SESSION_COMMAND_SCHEMA, CODING_SESSION_COMMAND_TAG_VERSION,
};
use buzz_core::coding_session_lifecycle_command::{
    validate_event_id_hex, validate_session_ref, CodingSessionLifecycleAction,
    CodingSessionLifecycleCommandPayload, CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::ACTOR_ROLE_PAIR;
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_LEASE,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
};
use buzz_sdk::builders::{build_coding_session_command, build_coding_session_lifecycle_command};
use buzz_sdk::kind::{
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_TRANSCRIPT,
};

use super::crew::{
    build_executions, build_founder_index, build_inbox, caller_umbrella, decode_leases,
    decode_resumes, decode_turn_commands, format_age, newest_turn_stages, plan_readdress,
    resolve_send_target, short_pubkey, turn_load, CrewExecution, FounderIndex, ReaddressPlan,
    TurnCommand, TurnStage,
};
use super::{decode_metadata, decode_receipts, decode_transcripts, fetch_channel_events, rfc3339};
use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{read_or_stdin, sdk_err, validate_uuid};

/// The exact substring serde writes for a default delivery class.
const BOUNDARY_DELIVER_KEY: &str = r#","deliver":"boundary""#;

/// Translate the CLI flag into the wire enum.
fn delivery_of(arg: crate::DeliveryArg) -> CodingSessionDelivery {
    match arg {
        crate::DeliveryArg::Boundary => CodingSessionDelivery::Boundary,
        crate::DeliveryArg::Steer => CodingSessionDelivery::Steer,
        crate::DeliveryArg::Interrupt => CodingSessionDelivery::Interrupt,
    }
}

/// Serialize a 44220 payload the way the relay in front of us will accept it.
///
/// `deliver` is a closed enum with a serde default, which means serde *writes*
/// it even when it is `boundary` — and a relay built before the field existed
/// refuses any payload carrying it, because the payload is
/// `deny_unknown_fields`. The plan's general rule is that a new optional field
/// on a relay-validated kind is omitted at its default value until the relay
/// carrying it is deployed, and the desktop sender already does exactly this
/// (`desktop/src/features/coding-sessions/lib/codingSessionCommand.ts`).
///
/// The removal is a single-occurrence string edit rather than a re-encode, so
/// the remaining bytes are byte-for-byte the SDK's own serialization. It is
/// unambiguous: every `"` inside `text` is escaped as `\"`, so the needle
/// cannot occur inside a string value. If it is ever absent — serde learns to
/// skip the default — the payload is passed through untouched.
pub(super) fn boundary_free_content(
    payload: &CodingSessionCommandPayload,
) -> Result<String, CliError> {
    let content = serde_json::to_string(payload).map_err(|error| {
        CliError::Other(format!("coding-session command serialization: {error}"))
    })?;
    let occurrences = content.matches(BOUNDARY_DELIVER_KEY).count();
    if occurrences != 1 {
        return Ok(content);
    }
    Ok(content.replacen(BOUNDARY_DELIVER_KEY, "", 1))
}

/// Build the signed envelope for one 44220 turn command.
///
/// Validation and the canonical tag set come from
/// [`build_coding_session_command`]; only a `boundary` payload takes the
/// re-serialized path described on [`boundary_free_content`], and its tags are
/// rebuilt from the same public inputs the SDK uses.
pub(super) fn build_turn_command(
    channel_id: Uuid,
    payload: &CodingSessionCommandPayload,
) -> Result<nostr::EventBuilder, CliError> {
    let canonical = build_coding_session_command(channel_id, payload).map_err(sdk_err)?;
    let is_boundary = matches!(
        payload.action,
        CodingSessionAction::ThreadTurnStart {
            deliver: CodingSessionDelivery::Boundary,
            ..
        }
    );
    if !is_boundary {
        return Ok(canonical);
    }
    let content = boundary_free_content(payload)?;
    let target_key = coding_session_target_key(&payload.target);
    let tags = [
        ["h", channel_id.to_string().as_str()],
        ["cs-v", CODING_SESSION_COMMAND_TAG_VERSION],
        ["cs-target", target_key.as_str()],
    ]
    .iter()
    .map(|parts| {
        nostr::Tag::parse(parts.iter().copied())
            .map_err(|error| CliError::Other(format!("tag construction failed: {error}")))
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok(nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CODING_SESSION_COMMAND as u16),
        content,
    )
    .tags(tags))
}

/// Every durable kind the crew surface reads, in one filter.
const CREW_FACT_KINDS: &[u32] = &[
    KIND_CODING_SESSION_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_TRANSCRIPT,
    // 44226 is the only event that names a founder. Without it `founder`
    // would be structurally unknowable and would print `null` for every
    // session in the channel — a column that is always null is worse than no
    // column, because it reads as "nobody founded this".
    KIND_CODING_SESSION_GENESIS,
];

/// Everything one channel's crew state is folded from.
struct CrewFacts {
    executions: Vec<CrewExecution>,
    commands: Vec<TurnCommand>,
    stages: HashMap<String, TurnStage>,
    transcripts: Vec<super::TranscriptRecord>,
    resumes: Vec<super::crew::ResumeRecord>,
    founders: FounderIndex,
    lease_records: usize,
}

/// Fetch and fold one channel's crew state.
///
/// The lease snapshot is fetched separately and never paged: kind 24223 is
/// ephemeral, served from Redis rather than from stored events, so a second
/// page would join lease states from two different instants.
async fn fetch_crew_facts(client: &BuzzClient, channel_id: &str) -> Result<CrewFacts, CliError> {
    let events = fetch_channel_events(client, channel_id, CREW_FACT_KINDS).await?;
    let lease_events = client
        .query_all(json!({ "kinds": [KIND_CODING_SESSION_LEASE], "#h": [channel_id] }))
        .await
        .unwrap_or_default();
    let leases = decode_leases(&lease_events);
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let (transcripts, _) = decode_transcripts(&events);
    let (commands, _) = decode_turn_commands(&events);
    let now = chrono::Utc::now().timestamp();
    Ok(CrewFacts {
        executions: build_executions(&metadata, &receipts, &transcripts, &leases, now),
        commands,
        stages: newest_turn_stages(&receipts),
        transcripts,
        resumes: decode_resumes(&events),
        founders: build_founder_index(&events, &receipts),
        lease_records: leases.len(),
    })
}

/// Publish one signed coding-session event and print the write response,
/// merged with the crew fields the caller needs to follow it up.
async fn publish_with(
    client: &BuzzClient,
    event: nostr::Event,
    conflict: &str,
    extra: Value,
) -> Result<(), CliError> {
    let raw = client.submit_event(event).await?;
    let response = crate::commands::parse_write_response(&raw, conflict)?;
    let mut merged: Value = serde_json::from_str(&response)
        .map_err(|error| CliError::Other(format!("relay response is not JSON: {error}")))?;
    if let (Some(object), Some(fields)) = (merged.as_object_mut(), extra.as_object()) {
        for (key, value) in fields {
            object.insert(key.clone(), value.clone());
        }
    }
    println!("{merged}");
    Ok(())
}

/// `bee sessions send` — publish one 44220 to a named seat.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_send(
    client: &BuzzClient,
    channel_id: &str,
    to: Option<&str>,
    session_ref: Option<&str>,
    deliver: crate::DeliveryArg,
    content: Option<&str>,
    readdress: Option<&str>,
    reply_to: Option<&str>,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    if reply_to.is_some() {
        return Err(CliError::Usage(
            "--reply-to is refused: kind 44220 carries no reply reference. Its envelope is \
             exactly three tags (h, cs-v, cs-target) and its payload is deny_unknown_fields, \
             so a reply id cannot be added without a contract change the relay must also \
             learn. Quote the event id inside --content instead."
                .to_owned(),
        ));
    }
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    if let Some(session_ref) = session_ref {
        validate_session_ref(session_ref).map_err(CliError::Usage)?;
    }

    let facts = fetch_crew_facts(client, channel_id).await?;
    let command_id = Uuid::new_v4().to_string();

    let (target, target_key, text, delivery, mut extra) = match readdress {
        Some(source_command_id) => {
            let plan = plan_readdress(
                &facts.commands,
                &facts.stages,
                &facts.executions,
                &facts.resumes,
                source_command_id,
            )?;
            let extra = readdress_report(&plan);
            (
                plan.target.clone(),
                plan.target_key.clone(),
                plan.text.clone(),
                plan.deliver,
                extra,
            )
        }
        None => {
            let to = to.ok_or_else(|| {
                CliError::Usage("one of --to or --readdress is required".to_owned())
            })?;
            let content = content
                .ok_or_else(|| CliError::Usage("--content is required with --to".to_owned()))?;
            let text = read_or_stdin(content)?;
            let caller = client.keys().public_key().to_hex();
            let umbrella = match session_ref {
                Some(session_ref) => Some(session_ref.to_owned()),
                None => caller_umbrella(&facts.executions, &caller)?,
            };
            let execution = resolve_send_target(&facts.executions, to, umbrella.as_deref())?;
            (
                execution.target.clone(),
                execution.target_key.clone(),
                text,
                delivery_of(deliver),
                json!({
                    "seat": execution.seat_label(),
                    "role": execution.role,
                    "sessionRef": execution.session_ref,
                    "liveness": execution.liveness.render(),
                }),
            )
        }
    };

    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        target,
        action: CodingSessionAction::ThreadTurnStart {
            text,
            deliver: delivery,
        },
    };
    let event = client.sign_event_unchecked(build_turn_command(channel, &payload)?)?;

    if let Some(object) = extra.as_object_mut() {
        object.insert("commandId".into(), json!(command_id));
        object.insert("target".into(), json!(target_key));
        object.insert("deliver".into(), json!(delivery.as_str()));
    }
    publish_with(client, event, "turn command already accepted", extra).await
}

/// The re-addressing facts a sender needs on stdout: what did not run, where
/// the words went instead, and who put that generation there.
fn readdress_report(plan: &ReaddressPlan) -> Value {
    json!({
        "readdressOf": plan.source_command_id,
        "readdressReason": format!(
            "{}/{}",
            plan.refused_stage.as_str(),
            plan.refused_code
        ),
        "readdressedFromGeneration": plan.refused_target.generation,
        "resumedBy": plan.resumed_by,
    })
}

/// `bee sessions create` — publish one 44221 `session.create`.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_create(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: Option<&str>,
    genesis: Option<&str>,
    provider_instance: &str,
    provider_authority: &str,
    model: Option<&str>,
    title: Option<&str>,
    project: Option<&str>,
    repo: Option<&str>,
    brief: Option<&str>,
    actor: Option<&str>,
    role: Option<&str>,
    driver: Option<&str>,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    refuse_unsupported_create_flags(actor, role, driver)?;
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    if let Some(session_ref) = session_ref {
        validate_session_ref(session_ref).map_err(CliError::Usage)?;
    }
    if let Some(genesis) = genesis {
        validate_event_id_hex("--genesis", genesis).map_err(CliError::Usage)?;
        if session_ref.is_none() {
            return Err(CliError::Usage(
                "--genesis names the immutable genesis of an umbrella, so it requires \
                 --session-ref naming that umbrella"
                    .to_owned(),
            ));
        }
    }
    let initial_turn = brief.map(read_or_stdin).transpose()?;

    let command_id = Uuid::new_v4().to_string();
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: project.map(str::to_owned),
            repo_ref: repo.map(str::to_owned),
            session_ref: session_ref.map(str::to_owned),
            genesis_ref: genesis.map(str::to_owned),
            provider_instance_ref: provider_instance.to_owned(),
            provider_authority_pubkey: provider_authority.to_owned(),
            model: model.map(str::to_owned),
            title: title.map(str::to_owned),
            initial_turn,
            actor: None,
            role: None,
        },
    };
    let builder = build_coding_session_lifecycle_command(channel, &payload).map_err(sdk_err)?;
    let event = client.sign_event_unchecked(builder)?;
    publish_with(
        client,
        event,
        "lifecycle command already accepted",
        json!({ "commandId": command_id, "seated": false }),
    )
    .await
}

/// Refuse the three create flags this surface cannot honestly publish.
///
/// Each refusal names the mechanism rather than the policy, because each is a
/// fact about the contract and not a preference:
///
/// - `--actor` names a seat whose key material is host-local custody (plan D6).
///   A provider that cannot resolve it answers `ACTOR_UNAVAILABLE`, so a create
///   signed here would name a seat nothing can act as.
/// - `--role` is the other half of the actor/role pair — `validate` rejects
///   either alone with [`ACTOR_ROLE_PAIR`] — so with `--actor` refused, a role
///   has nothing to attach to.
/// - `--driver` is minted by the provider into the target it returns; a create
///   names a provider *instance* and *authority*, never a driver.
pub fn refuse_unsupported_create_flags(
    actor: Option<&str>,
    role: Option<&str>,
    driver: Option<&str>,
) -> Result<(), CliError> {
    if actor.is_some() {
        return Err(CliError::Usage(
            "--actor is refused by `bee sessions create`: an agent seat's key material is \
             host-local custody the CLI does not hold, so a seat created here would be \
             answered ACTOR_UNAVAILABLE by the provider. Create seated executions from the \
             desktop, then address them with `bee sessions send --to <role>`."
                .to_owned(),
        ));
    }
    if role.is_some() {
        return Err(CliError::Usage(format!(
            "--role is refused by `bee sessions create`: {ACTOR_ROLE_PAIR} — a role and an \
             actor are a pair and neither is valid alone, and --actor is refused here. \
             Create seated executions from the desktop, which holds the seat's key."
        )));
    }
    if driver.is_some() {
        return Err(CliError::Usage(
            "--driver is refused by `bee sessions create`: a create names a provider instance \
             (--provider-instance) and its catalog authority (--provider-authority); the \
             driver slug is minted by the provider into the target it returns."
                .to_owned(),
        ));
    }
    Ok(())
}

/// `bee sessions inbox` — turns addressed to seats this identity holds.
pub async fn cmd_inbox(
    client: &BuzzClient,
    channel_id: &str,
    since: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let caller = client.keys().public_key().to_hex();
    let facts = fetch_crew_facts(client, channel_id).await?;
    let mine: HashSet<String> = facts
        .executions
        .iter()
        .filter(|execution| execution.actor.as_deref() == Some(caller.as_str()))
        .map(|execution| execution.target_key.clone())
        .collect();
    let rows = build_inbox(&facts.commands, &facts.stages, &mine, since)?;

    let seats: HashMap<&str, &CrewExecution> = facts
        .executions
        .iter()
        .map(|execution| (execution.target_key.as_str(), execution))
        .collect();
    let output: Vec<Value> = rows
        .iter()
        .map(|row| {
            let stage = row.stage.as_ref();
            match format {
                crate::OutputFormat::Compact => json!({
                    "eventId": row.event_id,
                    "from": row.from,
                    "role": seats.get(row.target_key.as_str()).and_then(|seat| seat.role.clone()),
                    "deliver": row.deliver.as_str(),
                    "stage": stage.map(|stage| stage.status.as_str()),
                    "text": row.text,
                }),
                crate::OutputFormat::Json => json!({
                    "eventId": row.event_id,
                    "createdAt": rfc3339(row.created_at),
                    "from": row.from,
                    "commandId": row.command_id,
                    "target": row.target_key,
                    "role": seats.get(row.target_key.as_str()).and_then(|seat| seat.role.clone()),
                    "deliver": row.deliver.as_str(),
                    "text": row.text,
                    "stage": stage.map(|stage| stage.status.as_str()),
                    "stageAt": stage.map(|stage| rfc3339(stage.at)),
                    "errorCode": stage.and_then(|stage| stage.error_code.clone()),
                    "errorMessage": stage.and_then(|stage| stage.error_message.clone()),
                    "turnId": stage.and_then(|stage| stage.turn_id.clone()),
                }),
            }
        })
        .collect();

    if matches!(format, crate::OutputFormat::Compact) {
        println!("{}", Value::Array(output));
        return Ok(());
    }
    println!(
        "{}",
        json!({
            "channel": channel_id,
            "identity": caller,
            "seats": mine.len(),
            "turns": output,
        })
    );
    Ok(())
}

/// `bee sessions status` — one row per execution: who is seated, whether an
/// actor is behind it, and what it owes.
pub async fn cmd_status(
    client: &BuzzClient,
    channel_id: &str,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let facts = fetch_crew_facts(client, channel_id).await?;
    let rows: Vec<Value> = facts
        .executions
        .iter()
        .map(|execution| {
            let load = turn_load(
                execution,
                &facts.commands,
                &facts.stages,
                &facts.transcripts,
            );
            let open_turn = load.open_command_id.as_ref().map(|command_id| {
                json!({
                    "commandId": command_id,
                    "turnId": load.open_turn_id,
                    "startedAt": load.open_since.map(rfc3339),
                    "runningFor": load.open_since.map(|at| {
                        format_age(chrono::Utc::now().timestamp().saturating_sub(at))
                    }),
                })
            });
            // D9 / contract B: `None` means no budget key has ever been seen
            // on this execution's metadata — either this provider predates
            // the umbrella turn budget, or the umbrella has none configured.
            // Never rendered as `0/0` or any other guessed number.
            let turn_budget_line = execution
                .turn_budget
                .map(|budget| format!("{}/{}", budget.used, budget.limit));
            // Who asked for this execution, and who founded the umbrella it
            // belongs to. `null` means this channel does not contain the
            // record that would say — never the provider's key, which signs
            // every execution here and would make every founder identical.
            let founding = facts.founders.of(&execution.target);
            match format {
                crate::OutputFormat::Compact => json!({
                    "target": execution.target_key,
                    "seat": execution.seat_label(),
                    "founder": founding.founder.as_deref().map(short_pubkey),
                    "live": execution.liveness.render(),
                    "openTurn": load.open_command_id,
                    "queued": load.queued,
                    "turnBudget": turn_budget_line,
                }),
                crate::OutputFormat::Json => json!({
                    "target": execution.target_key,
                    "sessionId": execution.target.session_id,
                    "generation": execution.target.generation,
                    "signer": execution.signer,
                    "actor": execution.actor,
                    "role": execution.role,
                    "sessionRef": execution.session_ref,
                    "seat": execution.seat_label(),
                    "founder": founding.founder,
                    "createSigner": founding.create_signer,
                    "runtime": execution.runtime,
                    "model": execution.model,
                    "status": execution.status,
                    "live": execution.liveness.render(),
                    "liveness": execution.liveness.word(),
                    "lastSignedSeq": execution.last_signed_seq,
                    "lastSignedAt": execution.last_signed_at.map(rfc3339),
                    "openTurn": open_turn,
                    "queuedTurns": load.queued,
                    "turnBudget": execution.turn_budget.map(|budget| json!({
                        "used": budget.used,
                        "limit": budget.limit,
                        "exhausted": budget.exhausted(),
                    })),
                }),
            }
        })
        .collect();

    if matches!(format, crate::OutputFormat::Compact) {
        println!("{}", Value::Array(rows));
        return Ok(());
    }
    println!(
        "{}",
        json!({
            "channel": channel_id,
            "executions": rows,
            // Every distinct founder named by a receipt-joined create in this
            // channel. Empty means no execution here could be joined back to
            // a genesis — not that the sessions have no founders.
            "founders": facts.founders.founders(),
            // Disclosed, not assumed: kind 24223 is ephemeral and is served
            // from the relay's Redis snapshot, so `live` is only ever as fresh
            // as this call and `quiet` means "no lease answered", never "the
            // provider is gone".
            "leaseSnapshotRecords": facts.lease_records,
        })
    );
    Ok(())
}
