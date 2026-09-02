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

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{json, Value};
use uuid::Uuid;

use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionDelivery, TurnAttachment, ALLOWED_ATTACHMENT_MIMES, CODING_SESSION_COMMAND_SCHEMA,
    CODING_SESSION_COMMAND_TAG_VERSION, MAX_TURN_ATTACHMENTS,
};
use buzz_core::coding_session_lifecycle_command::{
    validate_event_id_hex, validate_role_slug, validate_session_ref, CodingSessionLifecycleAction,
    CodingSessionLifecycleCommandPayload, CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::{
    context_window_usage, ReceiptStatus, TurnUsageReport, ACTOR_ROLE_PAIR,
};
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
    build_executions, build_founder_index, build_inbox, caller_umbrella,
    create_receipts_for_command, decode_leases, decode_resumes, decode_turn_commands,
    find_hire_refusal, find_hired_seat, fold_delivery, fold_hire, format_age,
    founder_seated_creates_for_actor, hire_exit_code, hire_payload, hire_report,
    hire_unsupported_by_relay, newest_turn_stages, plan_readdress, resolve_send_target,
    resolve_umbrella_genesis, seat_repair_exit_code, short_pubkey, turn_load, CrewExecution,
    FounderIndex, HireOutcome, HireWait, HireWaitOutcome, ReaddressPlan, SeatRepairOutcome,
    TurnCommand, TurnStage, DELIVERY_WAIT_SECONDS, HIRE_WAIT_SECONDS,
};
use super::{decode_metadata, decode_receipts, decode_transcripts, fetch_channel_events, rfc3339};
use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{read_file_or_stdin, read_or_stdin, sdk_err, validate_uuid};

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

/// Publish one signed coding-session event and return the write response,
/// merged with the crew fields the caller needs to follow it up.
///
/// Separate from [`publish_with`] because `bee sessions send` has one more
/// fact to gather — what the provider did with the turn — *after* the relay
/// has answered and *before* anything is printed. Printing twice would make a
/// scripted reader parse two JSON documents for one command.
async fn submit_with(
    client: &BuzzClient,
    event: nostr::Event,
    conflict: &str,
    extra: Value,
) -> Result<Value, CliError> {
    let raw = client.submit_event(event).await?;
    let response = crate::commands::parse_write_response(&raw, conflict)?;
    let mut merged: Value = serde_json::from_str(&response)
        .map_err(|error| CliError::Other(format!("relay response is not JSON: {error}")))?;
    if let (Some(object), Some(fields)) = (merged.as_object_mut(), extra.as_object()) {
        for (key, value) in fields {
            object.insert(key.clone(), value.clone());
        }
    }
    Ok(merged)
}

/// The namespace prefix every CLI-minted team-operation wake command id
/// carries.
///
/// Deliberately distinct from Desktop's `team-wake-v1:` and from the
/// provider's own derivation, so a reader of a 44220 can tell which producer
/// minted the command without consulting anything else.
pub(super) const CLI_TEAM_WAKE_COMMAND_ID_PREFIX: &str = "cli-wake-v1";

/// Hex characters of the target-key digest carried in a derived wake id.
const CLI_TEAM_WAKE_TARGET_DIGEST_HEX: usize = 12;

/// The deterministic 44220 command id for one stored operation against one
/// exact delivery target.
///
/// Derived — never inherited. Reusing the command id that already delivered an
/// assignment makes the lead runner fence the wake as `AlreadyConsumed`, which
/// is what silently swallowed every CLI report wake in the 2026-09-01 live run
/// (`docs/SESSION_STATE.md` item 103, finding 4). Deriving from
/// `(operation, target)` keeps one operation naming exactly one wake per
/// target forever, so a retry cannot double-spend a turn either.
pub(super) fn team_operation_wake_command_id(operation_id: &str, target_key: &str) -> String {
    let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(
        target_key.as_bytes(),
    ));
    let short = digest
        .get(..CLI_TEAM_WAKE_TARGET_DIGEST_HEX)
        .unwrap_or(digest.as_str());
    format!("{CLI_TEAM_WAKE_COMMAND_ID_PREFIX}:{operation_id}:{short}")
}

/// Publish the provider wake paired with an already-stored team transaction.
///
/// The 44220 text is deliberately a tiny routing pointer. The provider must
/// fetch and verify the signed 44244 record rather than trust command prose.
///
/// `shared_command_id` is `Some` only for the one operation class whose stored
/// record can name its own delivery before it is signed — an assignment, whose
/// `deliveryCommandId` the provider reads back as the binding proving a turn
/// was opened by that assignment (`crates/buzz-session-provider/src/team_wake.rs`).
/// Every other class derives its command id from the operation that is now
/// stored, via [`team_operation_wake_command_id`].
#[allow(clippy::too_many_arguments)]
pub(super) async fn send_team_operation_wake(
    client: &BuzzClient,
    channel_id: &str,
    to: &str,
    session_ref: &str,
    shared_command_id: Option<&str>,
    operation_id: &str,
    operation_type: &str,
) -> Result<Value, CliError> {
    validate_uuid(channel_id)?;
    validate_session_ref(session_ref).map_err(CliError::Usage)?;
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    let facts = fetch_crew_facts(client, channel_id).await?;
    let execution = resolve_send_target(&facts.executions, to, Some(session_ref))?;
    let (command_id, command_id_source) = match shared_command_id {
        Some(shared) => (shared.to_owned(), "shared"),
        None => (
            team_operation_wake_command_id(operation_id, &execution.target_key),
            "derived",
        ),
    };
    let command_id = command_id.as_str();
    let text = team_operation_wake_text(operation_id, operation_type)?;
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        target: execution.target.clone(),
        action: CodingSessionAction::ThreadTurnStart {
            text,
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        },
    };
    let event = client.sign_event_unchecked(build_turn_command(channel, &payload)?)?;
    submit_with(
        client,
        event,
        "team-operation wake already accepted",
        json!({
            "commandId": command_id,
            // Which producer minted the id above, so a reader tracing a fenced
            // wake can tell an inherited id from a derived one.
            "commandIdSource": command_id_source,
            "operationId": operation_id,
            "operationType": operation_type,
            "target": execution.target_key,
            "seat": execution.seat_label(),
            "status": "unconfirmed",
        }),
    )
    .await
}

fn team_operation_wake_text(operation_id: &str, operation_type: &str) -> Result<String, CliError> {
    serde_json::to_string(&json!({
        "operationId": operation_id,
        "type": operation_type,
    }))
    .map_err(|error| CliError::Other(format!("operation wake serialization failed: {error}")))
}

/// Publish one signed coding-session event and print the write response,
/// merged with the crew fields the caller needs to follow it up.
async fn publish_with(
    client: &BuzzClient,
    event: nostr::Event,
    conflict: &str,
    extra: Value,
) -> Result<(), CliError> {
    let merged = submit_with(client, event, conflict, extra).await?;
    println!("{merged}");
    Ok(())
}

/// How often the delivery wait re-asks the relay for a receipt.
///
/// Short enough that the common case (a provider that answers immediately)
/// costs one poll, long enough that a ten-second wait is twenty queries and
/// not two hundred.
const DELIVERY_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// Wait for the first turn receipt answering `command_id`.
///
/// Returns `None` when [`DELIVERY_WAIT_SECONDS`] elapse with no answer — which
/// is a real answer and is reported as such, never smoothed into a success.
/// Query failures inside the window are retried rather than raised: the write
/// already landed, and turning a transient read error into a command failure
/// would tell the sender its turn was not sent when it was.
async fn await_delivery(
    client: &BuzzClient,
    channel_id: &str,
    command_id: &str,
    since: i64,
) -> Option<TurnStage> {
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(DELIVERY_WAIT_SECONDS);
    // `since` bounds the read to receipts that could possibly answer this
    // command, so the wait does not re-read the channel's whole receipt
    // history on every poll.
    let filter = json!({
        "kinds": [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
        "#h": [channel_id],
        "since": since,
    });
    loop {
        if let Ok(events) = client.query_all(filter.clone()).await {
            let (receipts, _) = decode_receipts(&events);
            if let Some(stage) = newest_turn_stages(&receipts).remove(command_id) {
                return Some(stage);
            }
        }
        if std::time::Instant::now() + DELIVERY_POLL >= deadline {
            return None;
        }
        tokio::time::sleep(DELIVERY_POLL).await;
    }
}

/// `bee sessions send` — publish one 44220 to a named seat.
///
/// Prints one JSON object carrying two different facts, deliberately not
/// collapsed: `accepted` is the relay's — it stored the command — and
/// `delivered`/`deliveryStatus`/`delivery` are the provider's, read from the
/// first turn receipt that answers this `commandId`. Ledger 80 (c): a `steer`
/// the runtime cannot honour is answered `turn_degraded` and delivered at the
/// next boundary, and printing only `accepted:true` told the sender its words
/// had gone in mid-turn when they had not.
///
/// `no_wait` skips the receipt wait entirely; the command then reports the
/// relay's fact alone and says the delivery is unconfirmed, which is what it
/// is.
#[allow(clippy::too_many_arguments)]
/// Upload each `--image` to the relay's Blossom store and describe it for the
/// turn payload.
///
/// The blob is addressed by hash, never by URL: the consuming provider derives
/// the fetch URL from the relay it is already connected to.
async fn upload_turn_images(
    client: &BuzzClient,
    images: &[String],
) -> Result<Vec<TurnAttachment>, CliError> {
    if images.is_empty() {
        return Ok(Vec::new());
    }
    if images.len() > MAX_TURN_ATTACHMENTS {
        return Err(CliError::Usage(format!(
            "--image may be given at most {MAX_TURN_ATTACHMENTS} times"
        )));
    }
    let mut attachments = Vec::with_capacity(images.len());
    for path in images {
        let descriptor = client.upload_file(path).await?;
        if !ALLOWED_ATTACHMENT_MIMES.contains(&descriptor.mime_type.as_str()) {
            return Err(CliError::Usage(format!(
                "--image {path} is {}, but a turn attachment must be one of {}",
                descriptor.mime_type,
                ALLOWED_ATTACHMENT_MIMES.join(", ")
            )));
        }
        attachments.push(TurnAttachment {
            sha256: descriptor.sha256.clone(),
            mime: descriptor.mime_type.clone(),
            size: descriptor.size,
            dim: descriptor.dim.clone(),
            filename: std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
        });
    }
    Ok(attachments)
}

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
    images: &[String],
    no_wait: bool,
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

    // Uploaded only once the turn is fully resolved: a send that is going to
    // fail on an unresolvable target should not leave orphan blobs behind.
    let attachments = upload_turn_images(client, images).await?;

    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        target,
        action: CodingSessionAction::ThreadTurnStart {
            text,
            attachments,
            deliver: delivery,
        },
    };
    let event = client.sign_event_unchecked(build_turn_command(channel, &payload)?)?;

    if let Some(object) = extra.as_object_mut() {
        object.insert("commandId".into(), json!(command_id));
        object.insert("target".into(), json!(target_key));
        object.insert("deliver".into(), json!(delivery.as_str()));
    }
    // Taken before the write so a receipt published in the same second as the
    // command cannot fall outside the window, with a second of slack for
    // clock skew between this host and the provider's.
    let since = chrono::Utc::now().timestamp() - 1;
    let mut merged = submit_with(client, event, "turn command already accepted", extra).await?;

    let stage = if no_wait {
        None
    } else {
        await_delivery(client, channel_id, &command_id, since).await
    };
    let report = fold_delivery(delivery, stage.as_ref(), !no_wait);
    if let Some(object) = merged.as_object_mut() {
        object.insert(
            "delivered".into(),
            match report.delivered {
                Some(delivered) => json!(delivered),
                None => Value::Null,
            },
        );
        object.insert("deliveryStatus".into(), json!(report.status));
        object.insert("delivery".into(), json!(report.detail));
    }
    println!("{merged}");
    Ok(())
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
            // `bee sessions create` names its own model, so there is no
            // routing decision to record. A record here would claim a choice
            // nobody made.
            routing: None,
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

/// How often the hire wait re-asks the relay whether the host has answered.
///
/// Longer than the turn poll: a hire's answer is a human-scale sequence of
/// host-local work, not a queue push, and half-second polls across
/// [`HIRE_WAIT_SECONDS`]' two-minute window would be four hundred and eighty
/// queries for one seat. At two seconds it is sixty.
const HIRE_POLL: std::time::Duration = std::time::Duration::from_millis(2_000);

/// Every kind the hire wait reads: the host's seated create, the provider's
/// receipt for it, the metadata that says which executions are in this
/// umbrella, and the turn a refusal arrives as.
const HIRE_ANSWER_KINDS: &[u32] = &[
    KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_COMMAND,
];

/// One read of the channel for a hire answer, plus the evidence it discarded.
///
/// The discards are carried rather than dropped: a receipt that names the
/// create's commandId but is not bound to it decides nothing, and a reader who
/// is never told it existed cannot tell a quiet wait from a contested one.
pub(super) struct HireAnswer {
    /// What the channel says became of the hire.
    pub(super) outcome: HireOutcome,
    /// Receipts naming the create's commandId that were not cryptographically
    /// bound to it.
    pub(super) unbound_receipts: usize,
    /// Why a bound success receipt failed verification, when one did.
    pub(super) rejection: Option<String>,
}

/// Read the channel once and fold whatever answers the hire so far.
///
/// The authority chain is read only once a seat exists — a hire with no seat
/// has nothing to be granted — and it is a separate query because the chain
/// lives in kind:40099 acceptance receipts rather than in the lifecycle
/// stream the rest of this reads.
async fn read_hire_answer(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: &str,
    genesis_ref: &str,
    role: &str,
    since: i64,
) -> Result<HireAnswer, CliError> {
    let events = fetch_channel_events(client, channel_id, HIRE_ANSWER_KINDS).await?;
    let (receipts, _) = decode_receipts(&events);
    let mut unbound_receipts = 0;
    let mut rejection = None;
    // Evidence first, exactly as `seat-repair` chooses: the receipt is picked
    // by its cryptographic binding to the seated create and then verified, and
    // the author's own `created_at` decides nothing. Selecting by time and
    // checking the binding afterwards let anyone who could publish a 44224
    // carrying the commandId deny the hire.
    let (seat, receipt) = match find_hired_seat(&events, session_ref, role, since) {
        Some(seat) => {
            let assessed = assess_candidate(
                &events,
                channel_id,
                session_ref,
                genesis_ref,
                &receipts,
                seat,
            );
            unbound_receipts = assessed.unbound;
            rejection = assessed.rejection;
            // A bound failure-class receipt is the provider's answer, not a
            // missing one: the hire failed and says so.
            let receipt = assessed.verified.or(assessed.failed);
            (Some(assessed.seat), receipt)
        }
        None => (None, None),
    };
    let refusal = if seat.is_some() {
        None
    } else {
        let (metadata, _) = decode_metadata(&events);
        let (transcripts, _) = decode_transcripts(&events);
        let executions = build_executions(
            &metadata,
            &receipts,
            &transcripts,
            &HashMap::new(),
            chrono::Utc::now().timestamp(),
        );
        let (commands, _) = decode_turn_commands(&events);
        find_hire_refusal(&commands, &executions, session_ref, since)
    };
    Ok(HireAnswer {
        outcome: fold_hire(seat, receipt, refusal, false),
        unbound_receipts,
        rejection,
    })
}

/// `bee sessions hire` — ask an umbrella's host to seat a role (plan D14).
///
/// Two facts are printed, deliberately not collapsed. `accepted` is the
/// relay's: it stored the request. `outcome` is the *host's*: it seated
/// somebody, it refused, or it never answered. A hire's receipts are the
/// seat's own create receipts — the request carries no id the answer echoes —
/// so the wait recognizes the answer by what it is: a seated create for this
/// role, in this umbrella, published after the request went out.
///
/// A relay that predates `session.hire` refuses the payload as malformed;
/// that is reported as an old relay, never as a bad request.
/// What `bee sessions hire` was asked to route, if anything.
///
/// All-or-nothing on purpose: a class with no risk is a capability nobody
/// priced, and clap already refuses the half pair.
#[derive(Debug, Clone, Default)]
pub struct HireRouting {
    /// The capability class to route for.
    pub class: Option<String>,
    /// `impact,uncertainty,irreversibility`.
    pub risk: Option<String>,
    /// Extra trait minimums, as JSON.
    pub profile: Option<String>,
    /// Spec §6 triggers, comma-separated.
    pub review_flags: Option<String>,
    /// Deliberately sample a challenger for this class.
    pub challenger_sample: bool,
    /// The catalog id a human is overriding the router with.
    pub override_model: Option<String>,
    /// Why the human is overriding the router.
    pub because: Option<String>,
}

/// One hire's routing question, as it will ride on the wire.
struct HireRoutingPlan {
    /// The `providerInstanceRef` the hire's top level names, or `None`.
    provider_instance: Option<String>,
    /// The `model` the hire's top level names, or `None`.
    model: Option<String>,
    /// The routing request itself, or `None` when nothing routed.
    request: Option<buzz_core::coding_session_routing::HireRoutingRequest>,
    /// What the local router said, when it was asked and answered. Reported to
    /// the caller as `proposedUnavailable` when it could not — never hidden.
    proposal_unavailable: Option<String>,
}

/// Build the routing REQUEST a hire carries, or answer that it was not routed.
///
/// > "The lead chooses the capability required. The router chooses the
/// > execution target."
///
/// This command is the lead's side of that sentence: it emits a class and a
/// risk triple, and the founder's host decides. It still runs the router
/// locally and attaches the answer as `proposed` — informational, so the host
/// can disclose a disagreement and a lead can see the two side by side — but
/// the local answer never becomes the hire's instruction.
///
/// The hire's top-level `model` and `providerInstanceRef` are written **only**
/// for an override: they are how a hire dictates a target, and a routed hire
/// that filled them in with the requester's own pick would be dictating one
/// while claiming to ask a question. That is precisely what shipped on
/// 2026-08-30 and was dropped in silence (ledger draft 97).
///
/// A local router that cannot answer is not fatal. The host's catalog is the
/// one that decides, it may legitimately differ from this machine's, and a
/// hire refused here for a target *this* host cannot see would be a refusal
/// nobody asked for. The reason is reported instead, under
/// `proposedUnavailable`.
async fn resolve_hire_routing(
    client: &BuzzClient,
    channel_id: &str,
    routing: &HireRouting,
    provider_instance: Option<&str>,
    model: Option<&str>,
) -> Result<HireRoutingPlan, CliError> {
    let override_model = routing.override_model.as_deref().or(model);
    let Some(class) = routing.class.as_deref() else {
        if routing.because.is_some() {
            return Err(CliError::Usage(
                "--because explains an override of the router, so it needs --class and --risk; without them there is no routing decision to override"
                    .to_owned(),
            ));
        }
        if routing.override_model.is_some() {
            return Err(CliError::Usage(
                "--override-model overrides the router, so it needs --class and --risk; to hire a model without routing at all, pass --model"
                    .to_owned(),
            ));
        }
        return Ok(HireRoutingPlan {
            provider_instance: provider_instance.map(str::to_owned),
            model: model.map(str::to_owned),
            request: None,
            proposal_unavailable: None,
        });
    };
    let risk = routing.risk.as_deref().ok_or_else(|| {
        CliError::Usage("--class needs --risk impact,uncertainty,irreversibility".to_owned())
    })?;
    if override_model.is_some() && routing.because.is_none() {
        return Err(CliError::Usage(
            "--override-model overrides the router, so --because is required: an unexplained override is indistinguishable from a bug"
                .to_owned(),
        ));
    }

    let mut request = buzz_core::coding_session_routing::HireRoutingRequest {
        class: class.to_owned(),
        risk: super::route::parse_risk(risk)?,
        profile: match routing.profile.as_deref() {
            Some(profile) => Some(super::route::parse_profile(profile)?),
            None => None,
        },
        r#override: None,
        challenger_sample: routing.challenger_sample,
        review_flags: match routing.review_flags.as_deref() {
            Some(flags) => super::route::parse_review_flag_names(flags)?,
            None => Vec::new(),
        },
        proposed: None,
    };

    // The local router, run for disclosure rather than for instruction. A
    // failure here is reported, not fatal: this machine's catalog is not the
    // one that decides.
    let mut proposal_unavailable = None;
    match local_proposal(client, channel_id, &request).await {
        Ok(proposed) => request.proposed = proposed,
        Err(detail) => proposal_unavailable = Some(detail),
    }

    let Some(override_model) = override_model else {
        request.validate().map_err(CliError::Usage)?;
        // No override: the hire names no target at all. The host routes.
        return Ok(HireRoutingPlan {
            provider_instance: None,
            model: None,
            request: Some(request),
            proposal_unavailable,
        });
    };

    // An override still has to name something on offer. The catalog is the
    // only list of models this product has; an id it does not carry is named
    // and refused, never mapped onto a neighbour.
    let snapshot = super::catalog::load_catalogs(client, channel_id).await?;
    let offered = super::route::offers(&snapshot);
    let offer = offered
        .iter()
        .find(|offer| {
            offer.model == override_model
                && provider_instance.is_none_or(|wanted| offer.provider == wanted)
        })
        .ok_or_else(|| {
            CliError::Usage(format!(
                "--override-model {override_model:?} is not offered by this channel's catalog{}. Read the offer with `bee sessions catalog --channel {channel_id}`.",
                provider_instance.map_or(String::new(), |provider| format!(" on {provider}"))
            ))
        })?;
    request.r#override = Some(buzz_core::coding_session_routing::RoutingOverride {
        model: offer.model.clone(),
        // The tier's effort still applies; an override of the model is not an
        // override of the effort policy.
        effort: None,
        because: routing.because.clone().unwrap_or_default(),
    });
    request.validate().map_err(CliError::Usage)?;
    Ok(HireRoutingPlan {
        // The one case that writes the top level, and it writes the override's
        // own target — never the router's pick.
        provider_instance: Some(offer.provider.clone()),
        model: Some(offer.model.clone()),
        request: Some(request),
        proposal_unavailable,
    })
}

/// Run the router on this machine and return what it chose, for disclosure.
///
/// # Errors
///
/// One sentence saying why there is no local proposal — an unreadable
/// registry, a catalog this machine cannot fetch, or a class nothing here
/// clears. The caller reports it; it never blocks the hire, because the
/// founder's host routes against a catalog this machine may not share.
async fn local_proposal(
    client: &BuzzClient,
    channel_id: &str,
    request: &buzz_core::coding_session_routing::HireRoutingRequest,
) -> Result<Option<buzz_core::coding_session_routing::ProposedRouting>, String> {
    let (_, registry) = super::registry::load_registry(None).map_err(|error| error.to_string())?;
    let snapshot = super::catalog::load_catalogs(client, channel_id)
        .await
        .map_err(|error| error.to_string())?;
    let offered = super::route::offers(&snapshot);
    let route_request =
        buzz_core::coding_session_routing::RouteRequest::from_hire_routing(request)?;
    let decision = buzz_core::coding_session_routing::route(
        &registry,
        &offered,
        &route_request,
        super::registry::catalog_revision(&snapshot),
    )
    .map_err(|error| error.to_string())?;
    Ok(decision.record.as_proposed())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_hire(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: &str,
    genesis: Option<&str>,
    role: &str,
    provider_instance: Option<&str>,
    model: Option<&str>,
    brief: Option<&str>,
    content: Option<&str>,
    no_wait: bool,
    check: bool,
    routing: &HireRouting,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    validate_session_ref(session_ref).map_err(CliError::Usage)?;
    validate_role_slug(role).map_err(CliError::Usage)?;
    if let Some(genesis) = genesis {
        validate_event_id_hex("--genesis", genesis).map_err(CliError::Usage)?;
    }
    let brief_text = match (brief, content) {
        (Some(path), None) => read_file_or_stdin(path)?,
        (None, Some(text)) => read_or_stdin(text)?,
        (Some(_), Some(_)) => {
            return Err(CliError::Usage(
                "--brief names a file and --content carries the text; pass one, not both"
                    .to_owned(),
            ))
        }
        (None, None) => {
            return Err(CliError::Usage(
                "one of --brief <file> or --content <text> is required: a hired seat's first \
                 turn is the brief, so a hire with no brief would seat an agent with nothing \
                 to do"
                    .to_owned(),
            ))
        }
    };
    if brief_text.trim().is_empty() {
        return Err(CliError::Usage(
            "the brief is empty — a hired seat's first turn is the brief".to_owned(),
        ));
    }

    let genesis_ref = match genesis {
        Some(genesis) => genesis.to_owned(),
        None => {
            let events =
                fetch_channel_events(client, channel_id, &[KIND_CODING_SESSION_GENESIS]).await?;
            resolve_umbrella_genesis(&events, session_ref)?
        }
    };

    // The routing REQUEST is built before the hire is signed, and the local
    // router runs for disclosure only. The host routes: it is the only party
    // that can see its own live catalog.
    let plan = resolve_hire_routing(client, channel_id, routing, provider_instance, model).await?;

    let command_id = Uuid::new_v4().to_string();
    let payload = hire_payload(
        &command_id,
        session_ref,
        &genesis_ref,
        role,
        plan.provider_instance.as_deref(),
        plan.model.as_deref(),
        &brief_text,
        plan.request.clone(),
    );
    // The facts are measured off the exact payload that would be signed, and
    // they are measured *before* the builder runs: a report printed only on
    // the paths the builder accepts can never say `briefWithinCap: false`, so
    // the field described a warning that did not exist (REVIEW-A1 F11). A
    // refused check prints the facts and then the refusal.
    let check_report = check.then(|| {
        super::crew::hire_check_report(&super::crew::HireCheckRequest {
            channel: channel_id,
            session_ref,
            genesis_ref: &genesis_ref,
            role,
            brief: &brief_text,
            provider_instance: plan.provider_instance.as_deref(),
            model: plan.model.as_deref(),
            routing: plan.request.as_ref(),
            proposal_unavailable: plan.proposal_unavailable.as_deref(),
        })
    });
    // Validation is the SDK builder's, exactly as it would be for a real hire
    // — so `--check` can only pass on a payload the relay would accept.
    let builder = match build_coding_session_lifecycle_command(channel, &payload) {
        Ok(builder) => builder,
        Err(error) => {
            if let Some(report) = &check_report {
                println!("{report}");
            }
            return Err(sdk_err(error));
        }
    };
    if let Some(report) = check_report {
        println!("{report}");
        return Ok(());
    }
    let event = client.sign_event_unchecked(builder)?;

    // Taken before the write so a seated create published in the same second
    // as the request cannot fall outside the window, with a second of slack
    // for clock skew between this host and the founder's.
    let since = chrono::Utc::now().timestamp() - 1;
    let extra = json!({
        "commandId": command_id,
        "sessionRef": session_ref,
        "genesisRef": genesis_ref,
        "role": role,
        // The question this hire asked, exactly as it went on the wire.
        // `null` when nothing routed — the honest answer to "why this model"
        // for a hire that named one by hand.
        "routing": plan
            .request
            .as_ref()
            .and_then(|request| serde_json::to_value(request).ok()),
        // Why there is no `proposed` inside it, when there is not. A local
        // router that could not answer is said out loud rather than looking
        // like a lead that chose not to run one.
        "proposedUnavailable": plan.proposal_unavailable,
    });
    let mut merged = match submit_with(client, event, "hire request already accepted", extra).await
    {
        Ok(merged) => merged,
        Err(CliError::Other(message)) => {
            return match hire_unsupported_by_relay(&message) {
                // Deliberately a relay error, not a usage error: the request
                // was well formed and this relay is old.
                Some(named) => Err(CliError::Relay {
                    status: 400,
                    body: named,
                }),
                None => Err(CliError::Other(message)),
            };
        }
        Err(error) => return Err(error),
    };

    let HireWaitOutcome {
        mut outcome,
        last_evidence_error,
        unbound_receipts,
    } = if no_wait {
        HireWaitOutcome {
            outcome: HireOutcome::Unconfirmed,
            last_evidence_error: None,
            unbound_receipts: 0,
        }
    } else {
        wait_for_hire(client, channel_id, session_ref, &genesis_ref, role, since).await?
    };
    let mut seat_grant = None;
    let mut seat_grant_error = None;
    if let HireOutcome::Created {
        seat,
        receipt,
        granted,
    } = &mut outcome
    {
        let grant = async {
            let evidence = fetch_channel_events(client, channel_id, HIRE_ANSWER_KINDS).await?;
            super::hire_evidence::verify_hire_evidence(
                &evidence,
                &super::hire_evidence::HireEvidenceRequest {
                    channel: channel_id,
                    session_ref,
                    genesis: &genesis_ref,
                    role,
                    provider_instance: plan.provider_instance.as_deref(),
                },
                seat,
                receipt,
            )?;
            super::seat_authority::ensure_hired_seat_grant(
                client,
                channel_id,
                session_ref,
                &genesis_ref,
                &seat.actor,
                &seat.role,
            )
            .await
        }
        .await;
        match grant {
            Ok(result) => {
                *granted = true;
                seat_grant = Some(result);
            }
            Err(error) => seat_grant_error = Some(error.to_string()),
        }
    }
    let mut report = hire_report(&outcome, !no_wait, channel_id);
    if let Some(error) = &seat_grant_error {
        let actor = match &outcome {
            HireOutcome::Created { seat, .. } => seat.actor.clone(),
            _ => String::new(),
        };
        report.status = "created_ungranted";
        report.detail = format!(
            "the provider created the seat, but its role-seat authority was not accepted: \
             {error}. The seat is live, and the typed team fold still INCLUDES its report by \
             assignee identity, disclosed under `unseatedReports` as carrying no seat \
             authority; what the seat cannot do is hold verifier authority, so it can never \
             refute. {}",
            super::crew::seat_repair_remedy(channel_id, session_ref, &actor)
        );
    }
    // A wait that ended holding something because verification kept refusing
    // the host's answer must say so in the same sentence as the outcome — and
    // a wait a later poll answered must say the same fact in the past tense,
    // rather than reading as a failure on a hire that succeeded. Receipts that
    // named the create's commandId without being bound to it decided nothing
    // here; saying how many there were is the difference between a quiet wait
    // and a contested one.
    let disclosure = super::crew::evidence_disclosure(
        matches!(outcome, HireOutcome::Created { .. }),
        last_evidence_error.as_deref(),
        unbound_receipts,
    );
    if !disclosure.is_empty() {
        report.detail = format!("{} {disclosure}", report.detail);
    }
    if let Some(object) = merged.as_object_mut() {
        object.insert(
            "lastEvidenceError".into(),
            last_evidence_error
                .as_ref()
                .map_or(Value::Null, |error| json!(error)),
        );
        object.insert("unboundReceipts".into(), json!(unbound_receipts));
        object.insert("outcome".into(), json!(report.status));
        object.insert("detail".into(), json!(report.detail));
        object.insert(
            "seat".into(),
            match &outcome {
                HireOutcome::Created { seat, receipt, .. }
                | HireOutcome::Failed { seat, receipt } => {
                    json!({
                        "commandId": seat.command_id,
                        "createEventId": seat.event_id,
                        "receiptEventId": receipt.event_id,
                        "actor": seat.actor,
                        "seat": format!("{}\u{b7}{}", short_pubkey(&seat.actor), seat.role),
                        "role": seat.role,
                        "providerInstanceRef": seat.provider_instance_ref,
                        "model": seat.model,
                        "target": receipt.target_key,
                        "status": receipt.status.as_str(),
                    })
                }
                HireOutcome::Seating { seat } => json!({
                    "commandId": seat.command_id,
                    "createEventId": seat.event_id,
                    "receiptEventId": Value::Null,
                    "actor": seat.actor,
                    "seat": format!("{}\u{b7}{}", short_pubkey(&seat.actor), seat.role),
                    "role": seat.role,
                    "providerInstanceRef": seat.provider_instance_ref,
                    "model": seat.model,
                    "target": Value::Null,
                    "status": Value::Null,
                }),
                _ => Value::Null,
            },
        );
        let (code, reason) = match &outcome {
            HireOutcome::Refused(refusal) => (json!(refusal.code), json!(refusal.reason)),
            HireOutcome::Failed { receipt, .. } => (
                receipt
                    .error_code
                    .as_ref()
                    .map_or(Value::Null, |code| json!(code)),
                receipt
                    .error_message
                    .as_ref()
                    .map_or(Value::Null, |message| json!(message)),
            ),
            _ => (Value::Null, Value::Null),
        };
        object.insert("code".into(), code);
        object.insert("reason".into(), reason);
        // Null when no seat was created at all: `false` there would read as
        // "a seat exists and holds nothing", which is a different fact.
        object.insert(
            "granted".into(),
            match &outcome {
                HireOutcome::Created { granted, .. } => json!(granted),
                HireOutcome::Failed { .. } | HireOutcome::Seating { .. } => json!(false),
                HireOutcome::Refused(_) | HireOutcome::Unconfirmed => Value::Null,
            },
        );
        object.insert(
            "seatGrantEventId".into(),
            seat_grant
                .as_ref()
                .map_or(Value::Null, |grant| json!(grant.event_id)),
        );
        object.insert(
            "seatGrantAccepted".into(),
            match &outcome {
                HireOutcome::Created { .. } => json!(seat_grant.is_some()),
                _ => Value::Null,
            },
        );
        object.insert(
            "seatGrantAlreadyActive".into(),
            seat_grant
                .as_ref()
                .map_or(Value::Null, |grant| json!(grant.already_active)),
        );
        object.insert(
            "seatGrantError".into(),
            seat_grant_error
                .as_ref()
                .map_or(Value::Null, |error| json!(error)),
        );
    }
    println!("{merged}");

    match if seat_grant_error.is_some() {
        1
    } else {
        hire_exit_code(&outcome)
    } {
        0 => Ok(()),
        1 => Err(CliError::Refused(report.detail)),
        _ => Err(CliError::Unconfirmed(report.detail)),
    }
}

/// Poll the channel until the host answers the hire, or the wait runs out.
///
/// Query failures inside the window are retried rather than raised: the
/// request already landed, and turning a transient read error into a command
/// failure would tell a lead its hire was never sent when it was.
///
/// Returns the outcome and the **last evidence verification error** seen, so a
/// `seating` outcome caused by a binding defect can never again read as a slow
/// host. The state machine itself is [`HireWait`], which is pure and tested.
async fn wait_for_hire(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: &str,
    genesis_ref: &str,
    role: &str,
    since: i64,
) -> Result<HireWaitOutcome, CliError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(HIRE_WAIT_SECONDS);
    let mut wait = HireWait::new();
    loop {
        if let Ok(answer) =
            read_hire_answer(client, channel_id, session_ref, genesis_ref, role, since).await
        {
            wait.note(answer.rejection, answer.unbound_receipts);
            // `read_hire_answer` already ran the full evidence check on the
            // same snapshot it chose the receipt from, so a `Created` here is
            // verified by construction. Re-reading the channel to check it
            // again would ask a second, later snapshot a question the first
            // one already answered.
            let verified = matches!(answer.outcome, HireOutcome::Created { .. }).then(|| Ok(()));
            if let Some(answered) = wait.observe(answer.outcome, verified) {
                return Ok(wait.finish(answered));
            }
        }
        if std::time::Instant::now() + HIRE_POLL >= deadline {
            let held = wait.held();
            return Ok(wait.finish(held));
        }
        tokio::time::sleep(HIRE_POLL).await;
    }
}

/// `bee sessions seat-repair` — grant the role seat a hire created and lost.
///
/// The recovery path for the one hire failure nothing else can undo: the host
/// seated the role, the provider answered `created`, and the accepted 44228
/// chain never learned about it — because the receipt reached the relay after
/// `hire`'s window closed (cleantest, 2026-09-01) or because the grant write
/// itself failed. Re-hiring cannot recover it: a fresh hire takes a `since`
/// cutoff before its own request, which excludes the create that already
/// exists, and would seat a *second* agent. So this command builds no
/// kind:44221 at all, and the only event it can ever submit is one kind:44228
/// `grant-seat`.
///
/// One read of the channel, no wait and no poll — every fact it needs is
/// already on the relay by definition, since the premise is that the create is
/// older than the window that missed it. The evidence bar is exactly `hire`'s:
/// [`super::hire_evidence::verify_hire_evidence`] re-checks the signed
/// genesis, the seated create, the provider receipt bound to that create's own
/// commandId, and the provider's own metadata for the target, before
/// [`super::seat_authority::ensure_hired_seat_grant`] is allowed to write.
/// The role comes from the create, never from a flag: a repair that took the
/// role from its caller could grant a role no host ever seated.
pub async fn cmd_seat_repair(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: &str,
    genesis: Option<&str>,
    actor: &str,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    validate_session_ref(session_ref).map_err(CliError::Usage)?;
    crate::validate::validate_lower_hex64("--actor", actor)?;
    if let Some(genesis) = genesis {
        validate_event_id_hex("--genesis", genesis).map_err(CliError::Usage)?;
    }

    // Resolved exactly as `cmd_hire` resolves it when `--genesis` is absent.
    let genesis_ref = match genesis {
        Some(genesis) => genesis.to_owned(),
        None => {
            let events =
                fetch_channel_events(client, channel_id, &[KIND_CODING_SESSION_GENESIS]).await?;
            resolve_umbrella_genesis(&events, session_ref)?
        }
    };

    let events = fetch_channel_events(client, channel_id, HIRE_ANSWER_KINDS).await?;

    // The founder is established BEFORE any create is chosen: a create signed
    // by anyone else is not a candidate at all, so a stranger's 44221 cannot
    // occupy a slot in the candidate set and shadow the live seat.
    let founder = match super::hire_evidence::verified_genesis(
        &events,
        channel_id,
        session_ref,
        &genesis_ref,
    ) {
        Ok(genesis) => genesis.pubkey.to_hex(),
        Err(error) => {
            return finish_seat_repair(
                SeatRepairOutcome::Refused,
                actor,
                None,
                None,
                None,
                None,
                format!(
                    "the umbrella's own genesis {genesis_ref} could not be verified, so there \
                     is no founder to measure a seated create against: {error}. Nothing was \
                     written."
                ),
                format,
            );
        }
    };

    let candidates = founder_seated_creates_for_actor(&events, session_ref, actor, &founder);
    if candidates.is_empty() {
        return finish_seat_repair(
            SeatRepairOutcome::Refused,
            actor,
            None,
            None,
            None,
            None,
            format!(
                "no founder-signed seated create in channel {channel_id} names actor {actor} \
                 in umbrella {session_ref} — there is no seat to repair. Nothing was written. \
                 Read `bee sessions status --channel {channel_id}`; do not hire again to \
                 create one you did not mean to."
            ),
            format,
        );
    }

    let (receipts, _) = decode_receipts(&events);
    let assessed: Vec<AssessedCandidate> = candidates
        .into_iter()
        .map(|seat| {
            assess_candidate(
                &events,
                channel_id,
                session_ref,
                &genesis_ref,
                &receipts,
                seat,
            )
        })
        .collect();

    // `ambiguous` is DISAGREEMENT, never a count. A grant writes exactly one
    // thing — `(actor, role)` (`seat_authority::ensure_hired_seat_grant`) — and
    // every candidate here is already filtered to one actor, so two verifying
    // creates can only genuinely disagree about the ROLE. Candidates that imply
    // the identical write are the same answer arriving twice, and refusing them
    // broke the idempotent second run on any umbrella that had ever hired the
    // same actor twice.
    let verifying: Vec<(&AssessedCandidate, &super::crew::SeatReceipt)> = assessed
        .iter()
        .filter_map(|candidate| {
            candidate
                .verified
                .as_ref()
                .map(|receipt| (candidate, receipt))
        })
        .collect();

    let mut by_write: BTreeMap<(&str, &str), Vec<&AssessedCandidate>> = BTreeMap::new();
    for (candidate, _) in &verifying {
        by_write
            .entry((candidate.seat.actor.as_str(), candidate.seat.role.as_str()))
            .or_default()
            .push(candidate);
    }

    let chosen = if by_write.len() <= 1 {
        // Zero or one distinct write. One group of any size proceeds: every
        // member implies byte-identical authority, so there is nothing to
        // choose between and the first in event-id order represents them all.
        verifying.first().copied()
    } else {
        // Real disagreement about the role. One thing can still settle it
        // without guessing: a role this actor ALREADY holds on the accepted
        // 44228 chain is not the repair's opinion, it is the founder's
        // recorded decision. Reading it here is what makes the remedy below
        // converge — the founder grants the role they mean, re-runs, and gets
        // `already_granted` instead of the same refusal for ever.
        let seated_role = super::operations::fetch_projected_authority(
            client,
            channel_id,
            &genesis_ref,
            &founder,
        )
        .await
        .ok()
        .and_then(|authority| {
            authority
                .seats
                .iter()
                .find(|seat| seat.actor_pubkey == actor)
                .map(|seat| seat.role.clone())
        });
        match seated_role {
            Some(role) => verifying
                .iter()
                .find(|(candidate, _)| candidate.seat.role == role)
                .copied(),
            None => None,
        }
    };

    let Some((candidate, receipt)) = chosen else {
        if by_write.len() > 1 {
            let roles = describe_disagreement(&by_write);
            return finish_seat_repair(
                SeatRepairOutcome::Ambiguous,
                actor,
                None,
                None,
                None,
                None,
                format!(
                    "actor {actor} has founder-signed seated creates in umbrella {session_ref} \
                     for {} different roles, each with provider-signed proof of a running \
                     execution, so which role this actor holds is a decision only the founder \
                     can make. Nothing was written. The disagreement: {roles}. Re-running \
                     alone will not clear it — both creates are signed history and neither can \
                     be withdrawn. Settle it on the umbrella's accepted authority chain by \
                     granting the role you mean: `bee sessions grant-seat --channel \
                     {channel_id} --genesis {genesis_ref} --pubkey {actor} --role <slug>`. If \
                     this actor already holds the other role there, withdraw it first with \
                     `bee sessions \
                     revoke-seat --channel {channel_id} --genesis {genesis_ref} --pubkey \
                     {actor} --role <held-slug>` — a seated actor is never silently \
                     re-roled, so a grant alone will not converge while the disputed role \
                     stands. Once the seat you mean is accepted, re-run this and it reports \
                     `already_granted`.",
                    by_write.len()
                ),
                format,
            );
        }

        // Nothing verified. Two different facts, and they must not be collapsed:
        // a provider that REFUSED a create, and a provider that has not answered.
        if let Some((failed, receipt)) = assessed.iter().find_map(|candidate| {
            candidate
                .failed
                .as_ref()
                .map(|receipt| (candidate, receipt))
        }) {
            return finish_seat_repair(
                SeatRepairOutcome::Refused,
                actor,
                Some(&failed.seat.role),
                Some(&failed.seat.event_id),
                Some(&receipt.event_id),
                None,
                format!(
                    "the provider refused the seated create {}: {}{}. There is no execution to \
                     grant authority for, and no event was written. Candidates: {}.",
                    failed.seat.command_id,
                    receipt
                        .error_code
                        .as_deref()
                        .unwrap_or(receipt.status.as_str()),
                    match receipt.error_message.as_deref() {
                        Some(message) => format!(" — {message}"),
                        None => String::new(),
                    },
                    describe_candidates(&assessed)
                ),
                format,
            );
        }
        if let Some(rejected) = assessed
            .iter()
            .find_map(|candidate| candidate.rejection.as_deref())
        {
            return finish_seat_repair(
                SeatRepairOutcome::Refused,
                actor,
                None,
                None,
                None,
                None,
                format!(
                    "no seated create for actor {actor} in umbrella {session_ref} has signed \
                     provider evidence that verifies, so no seat authority was written. The \
                     closest candidate failed with: {rejected}. Candidates: {}.",
                    describe_candidates(&assessed)
                ),
                format,
            );
        }
        return finish_seat_repair(
            SeatRepairOutcome::NoReceiptYet,
            actor,
            None,
            None,
            None,
            None,
            format!(
                "no seated create for actor {actor} in umbrella {session_ref} has a bound \
                 provider lifecycle receipt yet, so nothing proves an execution exists to \
                 grant authority for. Nothing was written. Candidates: {}. Read `bee sessions \
                 status --channel {channel_id}` and run this again once the provider has \
                 answered.",
                describe_candidates(&assessed)
            ),
            format,
        );
    };

    let seat = &candidate.seat;

    match super::seat_authority::ensure_hired_seat_grant(
        client,
        channel_id,
        session_ref,
        &genesis_ref,
        &seat.actor,
        &seat.role,
    )
    .await
    {
        Ok(grant) if grant.already_active => finish_seat_repair(
            SeatRepairOutcome::AlreadyGranted,
            actor,
            Some(&seat.role),
            Some(&seat.event_id),
            Some(&receipt.event_id),
            Some(grant.event_id.as_str()),
            format!(
                "actor {actor} already holds an accepted {} seat in umbrella {session_ref}; \
                 nothing was written.",
                seat.role
            ),
            format,
        ),
        Ok(grant) => finish_seat_repair(
            SeatRepairOutcome::Granted,
            actor,
            Some(&seat.role),
            Some(&seat.event_id),
            Some(&receipt.event_id),
            Some(grant.event_id.as_str()),
            format!(
                "appended an accepted grant-seat naming actor {actor} as {} in umbrella \
                 {session_ref}, on the evidence of seated create {} and provider receipt {}.",
                seat.role, seat.command_id, receipt.event_id
            ),
            format,
        ),
        Err(error) => finish_seat_repair(
            SeatRepairOutcome::Refused,
            actor,
            Some(&seat.role),
            Some(&seat.event_id),
            Some(&receipt.event_id),
            None,
            format!("the role-seat grant was not accepted: {error}"),
            format,
        ),
    }
}

/// One candidate seated create, with what the signed evidence says about it.
pub(super) struct AssessedCandidate {
    pub(super) seat: super::crew::HiredSeat,
    /// A bound success receipt for which the whole hire chain verifies.
    pub(super) verified: Option<super::crew::SeatReceipt>,
    /// A bound failure-class receipt: the provider answered, and said no.
    pub(super) failed: Option<super::crew::SeatReceipt>,
    /// Why the closest bound success receipt did not verify, when one existed.
    pub(super) rejection: Option<String>,
    /// How many receipts named this commandId but were not bound to it.
    pub(super) unbound: usize,
}

/// Judge one candidate on signed evidence alone.
///
/// Receipts are filtered by [`super::hire_evidence::create_receipt_binding`]
/// *before* any of them is considered, so an unbound receipt — a forgery
/// carrying the commandId, whatever `created_at` it claims — is invisible here
/// rather than fatal. It is still counted, and reported, because a repair that
/// silently ignored competing evidence would be its own honesty bug.
pub(super) fn assess_candidate(
    events: &[Value],
    channel_id: &str,
    session_ref: &str,
    genesis_ref: &str,
    receipts: &[super::ReceiptRecord],
    seat: super::crew::HiredSeat,
) -> AssessedCandidate {
    let mut verified = None;
    let mut failed = None;
    let mut rejection = None;
    let mut unbound = 0;
    for receipt in create_receipts_for_command(receipts, &seat.command_id) {
        if super::hire_evidence::create_receipt_binding(channel_id, &seat, &receipt).is_err() {
            unbound += 1;
            continue;
        }
        if !matches!(
            receipt.status,
            ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
        ) {
            if failed.is_none() {
                failed = Some(receipt);
            }
            continue;
        }
        if verified.is_some() {
            continue;
        }
        // `provider_instance` is None because a repair has no request to
        // compare against: the create's own instance is the only claim, and it
        // is already bound to the receipt's target and the provider's metadata.
        match super::hire_evidence::verify_hire_evidence(
            events,
            &super::hire_evidence::HireEvidenceRequest {
                channel: channel_id,
                session_ref,
                genesis: genesis_ref,
                role: &seat.role,
                provider_instance: None,
            },
            &seat,
            &receipt,
        ) {
            Ok(()) => verified = Some(receipt),
            Err(error) => {
                if rejection.is_none() {
                    rejection = Some(error.to_string());
                }
            }
        }
    }
    AssessedCandidate {
        seat,
        verified,
        failed,
        rejection,
        unbound,
    }
}

/// The most candidates any one sentence will name before it truncates.
const MAX_LISTED_CANDIDATES: usize = 8;

/// Name each disputed role and the commandIds claiming it, bounded.
///
/// The `ambiguous` outcome's whole job: an operator cannot settle a
/// disagreement they cannot see, and "two creates verify" is not something
/// anyone can act on. Roles are listed in sorted order so two runs over the
/// same channel read identically.
fn describe_disagreement(by_write: &BTreeMap<(&str, &str), Vec<&AssessedCandidate>>) -> String {
    by_write
        .iter()
        .map(|((_, role), candidates)| {
            let mut ids: Vec<&str> = candidates
                .iter()
                .take(MAX_LISTED_CANDIDATES)
                .map(|candidate| candidate.seat.command_id.as_str())
                .collect();
            if candidates.len() > MAX_LISTED_CANDIDATES {
                return format!(
                    "{role} ({}, +{} more not listed)",
                    ids.join(", "),
                    candidates.len() - MAX_LISTED_CANDIDATES
                );
            }
            ids.dedup();
            format!("{role} ({})", ids.join(", "))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Name every candidate create and what its evidence says, bounded.
///
/// Every outcome that does not grant prints this, because the failure the
/// reviewer found was not a wrong write — it was a true sentence about the
/// wrong create, with no way for the operator to see that a second one existed.
fn describe_candidates(assessed: &[AssessedCandidate]) -> String {
    let mut parts: Vec<String> = assessed
        .iter()
        .take(MAX_LISTED_CANDIDATES)
        .map(|candidate| {
            let state = if candidate.verified.is_some() {
                "verified".to_owned()
            } else if let Some(receipt) = &candidate.failed {
                format!(
                    "provider refused ({})",
                    receipt
                        .error_code
                        .as_deref()
                        .unwrap_or(receipt.status.as_str())
                )
            } else if candidate.rejection.is_some() {
                "receipt did not verify".to_owned()
            } else {
                "no bound receipt".to_owned()
            };
            let unbound = match candidate.unbound {
                0 => String::new(),
                count => format!(", {count} unbound receipt(s) ignored"),
            };
            format!("{} [{state}{unbound}]", candidate.seat.command_id)
        })
        .collect();
    if assessed.len() > MAX_LISTED_CANDIDATES {
        parts.push(format!(
            "+{} more not listed",
            assessed.len() - MAX_LISTED_CANDIDATES
        ));
    }
    parts.join("; ")
}

/// The document one `seat-repair` outcome prints, for one format.
///
/// Every outcome prints the same key set, with `null` where a fact is absent:
/// a script reading `receiptEventId` must be able to tell "there was no
/// receipt" from "this command does not report receipts". That now includes
/// the outcomes with no create at all — an actor nothing seated, an
/// unverifiable genesis — which used to print nothing on stdout and leave a
/// script parsing stderr. `--format compact` keeps the four facts a script
/// branches on and drops the two provenance ids and the sentence, which is
/// what the sibling readers do (`cmd_inbox` rows, `status_row`).
///
/// Pure, so the shape is asserted without a relay.
#[allow(clippy::too_many_arguments)]
pub(super) fn seat_repair_document(
    outcome: SeatRepairOutcome,
    actor: &str,
    role: Option<&str>,
    create_event_id: Option<&str>,
    receipt_event_id: Option<&str>,
    seat_grant_event_id: Option<&str>,
    detail: &str,
    format: &crate::OutputFormat,
) -> Value {
    match format {
        crate::OutputFormat::Compact => json!({
            "outcome": outcome.as_str(),
            "actor": actor,
            "role": role,
            "seatGrantEventId": seat_grant_event_id,
        }),
        crate::OutputFormat::Json => json!({
            "outcome": outcome.as_str(),
            "actor": actor,
            "role": role,
            "createEventId": create_event_id,
            "receiptEventId": receipt_event_id,
            "seatGrantEventId": seat_grant_event_id,
            "detail": detail,
        }),
    }
}

/// Print one `seat-repair` result and return the exit code it earns.
#[allow(clippy::too_many_arguments)]
fn finish_seat_repair(
    outcome: SeatRepairOutcome,
    actor: &str,
    role: Option<&str>,
    create_event_id: Option<&str>,
    receipt_event_id: Option<&str>,
    seat_grant_event_id: Option<&str>,
    detail: String,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let document = seat_repair_document(
        outcome,
        actor,
        role,
        create_event_id,
        receipt_event_id,
        seat_grant_event_id,
        &detail,
        format,
    );
    println!("{document}");
    match seat_repair_exit_code(outcome) {
        0 => Ok(()),
        1 => Err(CliError::Refused(detail)),
        _ => Err(CliError::Unconfirmed(detail)),
    }
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

/// How much of its model's context an execution is holding, and out of what.
///
/// Read off the wire, never estimated. Two sources answer, in this order:
///
/// 1. The driver's own `context_window_updated` item (`used` / `size`). This
///    is *occupancy*: the driver measured the prompt it was about to send, so
///    it can never exceed the window. `claude-agent-acp` emits it.
/// 2. Failing that, the terminal `result` item's `usage` block, summed by
///    [`TurnUsageReport::used_tokens`]. That is the turn's prompt-side
///    *consumption*, which on a turn that made several model calls is larger
///    than the context the model actually held. It is the honest second-best
///    and it is labelled as such in `--help`.
///
/// `None` means no execution of either kind reached the wire — reported as
/// nothing, never as zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextLoad {
    used_tokens: u64,
    context_window: Option<u64>,
}

impl ContextLoad {
    /// Percent of the window in use, rounded half-up to a whole percent
    /// (137 498 of 1 000 000 reads `14%`). `None` when no window is known — a
    /// percentage against a guessed denominator reads exactly like a measured
    /// one, and there is no honest denominator to invent.
    fn pct(&self) -> Option<u64> {
        let window = self.context_window.filter(|window| *window > 0)?;
        Some((self.used_tokens.saturating_mul(200) / window).div_ceil(2))
    }

    /// The cell a person reads.
    fn render(&self) -> String {
        match (self.context_window, self.pct()) {
            (Some(window), Some(pct)) => {
                format!("{}/{window} ({pct}%)", self.used_tokens)
            }
            _ => format!("{} (window unknown)", self.used_tokens),
        }
    }

    /// The machine-readable shape: three keys, each `null` when unknown.
    fn to_json(self) -> Value {
        json!({
            "usedTokens": self.used_tokens,
            "contextWindow": self.context_window,
            "contextPct": self.pct(),
        })
    }
}

/// The `--format compact` cell printed when nothing on the wire has said how
/// full a seat's context is — a real em dash (U+2014), not two hyphens.
///
/// `--format json` prints `null` for the same state. Pinned as a const, and
/// checked against `bee sessions status --help` by
/// `status_help_prints_the_same_unknown_cell_the_rows_do`: item 89's help text
/// described this cell as `'--'`, which is not a string the command has ever
/// printed, so a reader grepping for it found nothing.
pub const CONTEXT_UNKNOWN_CELL: &str = "\u{2014}";

/// Fold this execution's transcript into its current context load.
///
/// Newest item wins within each source, ordered by `(seq, created_at)` — the
/// same ordering `build_executions` uses to pick an execution's last signed
/// item, so the two can never disagree about which item is newest.
fn context_load(
    execution: &CrewExecution,
    transcripts: &[super::TranscriptRecord],
) -> Option<ContextLoad> {
    let mut mine: Vec<&super::TranscriptRecord> = transcripts
        .iter()
        .filter(|record| {
            record.signer == execution.signer && record.target_key == execution.target_key
        })
        .collect();
    mine.sort_by_key(|record| (record.seq, record.created_at));

    let occupancy = mine
        .iter()
        .rev()
        .find_map(|record| context_window_usage(&record.envelope.item));
    // The turn block is read either way: it carries the window the provider
    // named, which an occupancy item without a `size` does not.
    let reported = mine.iter().rev().find_map(|record| {
        let item = &record.envelope.item;
        if item.get("kind").and_then(Value::as_str) != Some("result") {
            return None;
        }
        serde_json::from_value::<TurnUsageReport>(item.get("usage")?.clone()).ok()
    });

    match occupancy {
        Some(occupancy) => Some(ContextLoad {
            used_tokens: occupancy.used_tokens,
            context_window: occupancy
                .context_window
                .or_else(|| reported.and_then(|usage| usage.context_window)),
        }),
        None => {
            let reported = reported?;
            Some(ContextLoad {
                used_tokens: reported.used_tokens()?,
                context_window: reported.context_window,
            })
        }
    }
}

/// One `bee sessions status` row, for one execution, in the shape `format`
/// asks for.
///
/// Pure over the facts handed in, so the row shape is testable without a
/// relay: [`turn_load`] and [`FounderIndex::of`] are both reads, and the only
/// other input is the wall clock behind `runningFor`.
pub fn status_row(
    execution: &CrewExecution,
    commands: &[TurnCommand],
    stages: &HashMap<String, TurnStage>,
    transcripts: &[super::TranscriptRecord],
    founders: &FounderIndex,
    format: &crate::OutputFormat,
) -> Value {
    let load = turn_load(execution, commands, stages, transcripts);
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
    let founding = founders.of(&execution.target);
    // How full this seat's context is, from the wire only. `None` renders as
    // [`CONTEXT_UNKNOWN_CELL`] and as JSON `null`: nothing has said, which is
    // not zero.
    let context = context_load(execution, transcripts);
    match format {
        crate::OutputFormat::Compact => json!({
            "target": execution.target_key,
            "seat": execution.seat_label(),
            "founder": founding.founder.as_deref().map(short_pubkey),
            "live": execution.liveness.render(),
            "openTurn": load.open_command_id,
            "queued": load.queued,
            "turnBudget": turn_budget_line,
            "context": context
                .map(|context| Value::String(context.render()))
                .unwrap_or_else(|| Value::String(CONTEXT_UNKNOWN_CELL.to_owned())),
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
            "context": context.map(ContextLoad::to_json),
        }),
    }
}

/// Decide whether `bee sessions status` prints NDJSON, from the four facts
/// that get a vote — and nothing else.
///
/// The precedence, highest first:
///
/// 1. `--json-lines` was asked for → NDJSON, terminal or not.
/// 2. `--no-json-lines` was asked for → the single document, pipe or not.
///    (clap refuses both flags together, so 1 and 2 cannot both hold.)
/// 3. `--format` was named explicitly → that format's document. Naming a
///    format is a request for it, and the tooling that reads `bee --format
///    json sessions status` down a pipe indexes into the envelope.
/// 4. Nothing was asked for → stdout decides: a pipe or a file gets NDJSON,
///    a terminal gets the document a person reads.
///
/// `stdout_is_tty` is a parameter rather than a call to [`std::io::IsTerminal`]
/// so that this is a pure function of its inputs. A test that read the real
/// stdout would pass under `cargo test` and fail under `cargo test | cat`.
pub fn resolve_json_lines(
    flag: bool,
    no_flag: bool,
    format_explicit: bool,
    stdout_is_tty: bool,
) -> bool {
    if flag {
        return true;
    }
    if no_flag || format_explicit {
        return false;
    }
    !stdout_is_tty
}

/// The `--json-lines` rendering: one serialized [`crate::OutputFormat::Json`]
/// row per execution, in `executions` order, each one a complete JSON document
/// on its own.
///
/// The JSON shape is not a choice the caller gets to make — the flag's
/// contract is "the same fields as a `--format json` row", so a compact
/// request still gets these. No envelope, no header, and no rows at all when
/// there are no executions.
pub fn status_json_lines(
    executions: &[CrewExecution],
    commands: &[TurnCommand],
    stages: &HashMap<String, TurnStage>,
    transcripts: &[super::TranscriptRecord],
    founders: &FounderIndex,
) -> Vec<String> {
    executions
        .iter()
        .map(|execution| {
            let row = status_row(
                execution,
                commands,
                stages,
                transcripts,
                founders,
                &crate::OutputFormat::Json,
            );
            // `Value` serialization is infallible for the shapes above; the
            // fallback keeps this off the no-`unwrap` list without inventing
            // a row.
            serde_json::to_string(&row).unwrap_or_else(|_| row.to_string())
        })
        .collect()
}

/// `bee sessions status` — one row per execution: who is seated, whether an
/// actor is behind it, and what it owes.
pub async fn cmd_status(
    client: &BuzzClient,
    channel_id: &str,
    json_lines: bool,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let facts = fetch_crew_facts(client, channel_id).await?;

    // `--json-lines` overrides `--format`, and drops the envelope: a consumer
    // reading a line at a time cannot be handed `founders` or
    // `leaseSnapshotRecords`, which describe the whole call rather than any
    // one row. Read them from `--format json` when you need them.
    if json_lines {
        for line in status_json_lines(
            &facts.executions,
            &facts.commands,
            &facts.stages,
            &facts.transcripts,
            &facts.founders,
        ) {
            println!("{line}");
        }
        return Ok(());
    }

    let rows: Vec<Value> = facts
        .executions
        .iter()
        .map(|execution| {
            status_row(
                execution,
                &facts.commands,
                &facts.stages,
                &facts.transcripts,
                &facts.founders,
                format,
            )
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

#[cfg(test)]
mod tests {
    use super::{team_operation_wake_text, CONTEXT_UNKNOWN_CELL};
    use clap::CommandFactory;

    /// The help and the rows must name the same string for "nothing has said".
    ///
    /// Item 89's `after_help` said `'--' means nothing on the wire has said`,
    /// while `status_row` printed an em dash. A person who reads the help and
    /// then greps their output for `--` finds nothing, and a person who sees
    /// the em dash has nothing in the help to look it up under. This asserts
    /// the two agree, in the only direction that can be checked: the help
    /// quotes the literal the code emits.
    #[test]
    fn status_help_prints_the_same_unknown_cell_the_rows_do() {
        let cli = crate::Cli::command();
        let sessions = cli
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "sessions")
            .expect("sessions command");
        let status = sessions
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "status")
            .expect("sessions status command");
        let help = status.clone().render_long_help().to_string();
        assert!(
            help.contains(&format!("'{CONTEXT_UNKNOWN_CELL}'")),
            "the help must quote the cell the rows print ({CONTEXT_UNKNOWN_CELL}):\n{help}"
        );
        assert!(
            !help.contains("'--'"),
            "'--' is not a cell this command prints:\n{help}"
        );
    }

    #[test]
    fn team_operation_wake_contains_only_the_signed_record_pointer() {
        let operation_id = "ab".repeat(32);
        let text = team_operation_wake_text(&operation_id, "assignment").expect("wake JSON");
        let value: serde_json::Value = serde_json::from_str(&text).expect("decode wake");
        assert_eq!(
            value,
            serde_json::json!({
                "operationId": operation_id,
                "type": "assignment",
            })
        );
        assert_eq!(value.as_object().expect("object").len(), 2);
    }
}

#[cfg(test)]
#[path = "seat_repair_tests.rs"]
mod seat_repair_tests;

// ── Role seats: grant and revoke ────────────────────────────────────────────

/// The umbrella one verified genesis founds.
///
/// `--session-ref` is optional on the seat verbs because the genesis already
/// names its umbrella and is signature-verified before it is read; asking a
/// caller to restate a fact the signed event carries is a way to be told a
/// different one. The value is handed straight back to
/// [`super::operations::fetch_founder_context`], which re-verifies the whole
/// envelope including this pairing.
async fn session_ref_of_genesis(
    client: &BuzzClient,
    channel_id: &str,
    genesis: &str,
) -> Result<String, CliError> {
    let rows = client
        .query_all(json!({
            "ids": [genesis],
            "kinds": [KIND_CODING_SESSION_GENESIS],
            "#h": [channel_id],
        }))
        .await?;
    if rows.len() != 1 {
        return Err(CliError::NotFound(format!(
            "expected exactly one genesis {genesis} in channel {channel_id}, found {}",
            rows.len()
        )));
    }
    let event: nostr::Event = serde_json::from_value(rows[0].clone())
        .map_err(|error| CliError::Other(format!("relay returned malformed genesis: {error}")))?;
    buzz_core::verify_event(&event)
        .map_err(|error| CliError::Other(format!("invalid genesis signature: {error}")))?;
    let payload = buzz_core::coding_session_genesis::decode_coding_session_genesis(&event.content)
        .map_err(|error| CliError::Other(format!("invalid genesis content: {error}")))?;
    Ok(payload.session_ref)
}

/// Resolve the umbrella a seat verb operates on.
async fn resolve_seat_session_ref(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: Option<&str>,
    genesis: &str,
) -> Result<String, CliError> {
    match session_ref {
        Some(session_ref) => {
            validate_session_ref(session_ref).map_err(CliError::Usage)?;
            Ok(session_ref.to_owned())
        }
        None => session_ref_of_genesis(client, channel_id, genesis).await,
    }
}

/// `bee sessions grant-seat --role <slug>` — the role-seat verb.
///
/// The collaborator and viewer tiers are *operator* grants: they say what a
/// human may do to a session. A role seat says who an actor **is** inside one
/// umbrella, and it is the fact the typed team fold reads for verifier
/// standing. Until this existed the only writer of `grant-seat` was the hire
/// path itself, so a seat the hire failed to grant could only be repaired
/// (`bee sessions seat-repair`) and a seat nobody hired could not be created
/// at all — which is why `seat-repair`'s own `ambiguous` remedy had to point
/// at the founder's Desktop app.
///
/// Standing is decided by exactly the same rule the hire path uses
/// ([`super::seat_authority::ensure_hired_seat_grant`]): founder, active
/// steering operator, or active lead; a lead may not grant `lead`; an actor
/// may not nominate itself; an actor already seated in a different role is
/// refused rather than overwritten. Idempotent — a seat that already holds
/// this exact role is reported `already_granted` with no write.
pub async fn cmd_grant_seat(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: Option<&str>,
    genesis: &str,
    actor: &str,
    role: &str,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    validate_event_id_hex("--genesis", genesis).map_err(CliError::Usage)?;
    crate::validate::validate_lower_hex64("--pubkey", actor)?;
    validate_role_slug(role).map_err(CliError::Usage)?;
    let session_ref = resolve_seat_session_ref(client, channel_id, session_ref, genesis).await?;

    let grant = super::seat_authority::ensure_hired_seat_grant(
        client,
        channel_id,
        &session_ref,
        genesis,
        actor,
        role,
    )
    .await?;
    println!(
        "{}",
        json!({
            "outcome": if grant.already_active { "already_granted" } else { "granted" },
            "transition": "grant-seat",
            "eventId": grant.event_id,
            "actor": actor,
            "seat": format!("{}\u{b7}{role}", short_pubkey(actor)),
            "role": role,
            "sessionRef": session_ref,
            "genesisRef": genesis,
        })
    );
    Ok(())
}

/// What one `bee sessions revoke-seat` may do, decided on the accepted chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SeatRevokeDecision {
    /// Append a `revoke-seat` for this exact actor and role.
    Append,
}

/// Decide whether a revoke may be written, from the projected authority alone.
///
/// Pure so the refusals are testable without a relay. The relay's own
/// transition matrix remains the gate — this exists so the CLI refuses locally
/// with a sentence naming what the actor actually holds, instead of forwarding
/// a write the relay will reject with a shape error.
pub(super) fn decide_seat_revoke(
    authority: &super::operations::ProjectedAuthority,
    actor: &str,
    role: &str,
) -> Result<SeatRevokeDecision, CliError> {
    let Some(seat) = authority
        .seats
        .iter()
        .find(|seat| seat.actor_pubkey == actor)
    else {
        return Err(CliError::Refused(format!(
            "actor {actor} holds no active role seat on this umbrella's accepted authority \
             chain; there is nothing to revoke"
        )));
    };
    if seat.role != role {
        return Err(CliError::Refused(format!(
            "actor {actor} holds role {} on this umbrella, not {role}; revoke the role it \
             actually holds",
            seat.role
        )));
    }
    Ok(SeatRevokeDecision::Append)
}

/// `bee sessions revoke-seat` — withdraw one actor's exact role seat.
///
/// The counterpart `grant-seat` never had. It is the write that makes
/// `seat-repair`'s `ambiguous` outcome converge when the founder wants the
/// *other* role: two signed creates are history and neither can be withdrawn,
/// so the only way to settle which role an actor holds is on the accepted
/// kind:44228 chain.
///
/// Refused locally unless the pubkey holds that exact role — the relay's
/// transition matrix is still the gate, this just refuses with a sentence
/// naming what the actor really holds. Acceptance is re-read from the trusted
/// projection before it is reported: a submitted transition nothing accepted is
/// `unconfirmed`, never a success.
pub async fn cmd_revoke_seat(
    client: &BuzzClient,
    channel_id: &str,
    session_ref: Option<&str>,
    genesis: &str,
    actor: &str,
    role: &str,
) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    validate_event_id_hex("--genesis", genesis).map_err(CliError::Usage)?;
    crate::validate::validate_lower_hex64("--pubkey", actor)?;
    validate_role_slug(role).map_err(CliError::Usage)?;
    let session_ref = resolve_seat_session_ref(client, channel_id, session_ref, genesis).await?;
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;

    let context =
        super::operations::fetch_founder_context(client, channel_id, &session_ref, genesis).await?;
    let founder = context.founder_pubkey.clone();

    for attempt in 0..2 {
        let authority =
            super::operations::fetch_projected_authority(client, channel_id, genesis, &founder)
                .await?;
        decide_seat_revoke(&authority, actor, role)?;
        let seq = authority
            .head_seq
            .checked_add(1)
            .ok_or_else(|| CliError::Other("authority chain seq overflow".into()))?;
        let payload =
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_revoke_seat(
                genesis,
                authority.head_event_id,
                seq,
                actor,
                role,
            );
        let builder =
            buzz_sdk::builders::build_coding_session_authority_transition(channel, &payload)
                .map_err(|error| CliError::Other(error.to_string()))?;
        let event = client.sign_event_unchecked(builder)?;
        let event_id = event.id.to_hex();
        let outcome = client.submit_event(event).await.and_then(|raw| {
            crate::commands::parse_write_response(&raw, "seat revocation already accepted")
        });
        let response = match outcome {
            Ok(response) => response,
            // Another accepted transition advanced the head after this attempt
            // read it. Rebuild against a fresh projection exactly once; never
            // replay the stale event.
            Err(error) if attempt == 0 && super::is_chain_head_conflict(&error) => continue,
            Err(error) => return Err(error),
        };
        let accepted = serde_json::from_str::<Value>(&response)
            .ok()
            .and_then(|value| value.get("accepted").and_then(Value::as_bool));
        if accepted != Some(true) {
            return Err(CliError::Refused(
                "relay response did not prove acceptance of the seat revocation".into(),
            ));
        }
        let projected =
            super::operations::fetch_projected_authority(client, channel_id, genesis, &founder)
                .await?;
        if projected
            .seats
            .iter()
            .any(|seat| seat.actor_pubkey == actor && seat.role == role)
        {
            return Err(CliError::Unconfirmed(format!(
                "seat revocation {event_id} was submitted but the projected authority chain \
                 still seats {actor} as {role}; inspect the chain before retrying"
            )));
        }
        println!(
            "{}",
            json!({
                "outcome": "revoked",
                "transition": "revoke-seat",
                "eventId": event_id,
                "actor": actor,
                "seat": format!("{}\u{b7}{role}", short_pubkey(actor)),
                "role": role,
                "sessionRef": session_ref,
                "genesisRef": genesis,
            })
        );
        return Ok(());
    }
    Err(CliError::Other(
        "seat revocation exhausted its single head-conflict retry".into(),
    ))
}
