//! `bee ci continue` and `bee ci continuation status` — CI-managed turn
//! continuation over the coding-session command/receipt wire.
//! `docs/CI_MANAGED_CONTINUATION_IMPL.md` §2 is the implementation contract.
//!
//! **Private read (§3f).** The provider reads the recorded CI result with its
//! own relay key. A public project's result is visible to it; a private
//! project's result is visible only if that project explicitly admits the
//! provider's identity. Otherwise a real, hidden result is indistinguishable
//! from "not finished yet" until the registration expires, and the eventual
//! refusal is `CI_RESULT_UNAVAILABLE_OR_HIDDEN`. Neither `bee` command can
//! detect or work around this from the caller's side.
//!
//! **Once-admission guarantee (§0).** For one exact target (driver,
//! instanceId, sessionId, generation) and one CI correlation digest, the
//! provider admits at most one continuation turn, durably, across restarts,
//! duplicate result events, reconnect replays, and any number of
//! registration command ids. `bee ci continue`'s `commandId` is derived from
//! its inputs (channel, CI identity, target, expiry, continuation text), so
//! an exact retry of the same command names the same registration rather
//! than minting a second one.

use std::time::Duration;

use serde_json::{json, Value};

use buzz_core::ci_result::{correlation_id, validate_identity, CiResultIdentity};
use buzz_core::coding_session_command::{
    ci_continuation_command_id, coding_session_target_key, CodingSessionTarget,
};
use buzz_core::coding_session_payload::{decode_coding_session_lifecycle_receipt, ReceiptStatus};
use buzz_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;
use buzz_sdk::builders::build_coding_session_ci_continuation;

use crate::client::BuzzClient;
use crate::commands::parse_write_response;
use crate::error::CliError;
use crate::validate::{read_file_or_stdin, sdk_err};
use crate::{CiPhaseArg, OutputFormat};

/// How often the ack wait re-asks the relay for a registration/refusal
/// receipt. Mirrors `DELIVERY_POLL` in `commands/sessions/crew_cmds.rs`: fast
/// enough that the common case (a provider that answers immediately) costs
/// one poll, slow enough that a long `--ack-timeout` is not hundreds of
/// queries.
const ACK_POLL: Duration = Duration::from_millis(500);

/// Decode a `--continuation` value: `@path` reads a file (`@-` reads
/// stdin, matching [`read_file_or_stdin`]'s convention); anything else is
/// used as literal text.
pub(crate) fn read_continuation(value: &str) -> Result<String, CliError> {
    match value.strip_prefix('@') {
        Some(path) => read_file_or_stdin(path),
        None => Ok(value.to_owned()),
    }
}

/// Parse a `cs-target` key back into its four parts.
///
/// The inverse of [`coding_session_target_key`], which is the single source
/// of truth for the wire encoding (`"coding-session/v1|" + length-prefixed
/// fields`). This reimplements that exact shape rather than approximating it
/// with a delimiter split, then re-encodes the parsed target and requires it
/// to reproduce the input byte-for-byte before accepting it.
pub(crate) fn parse_target_key(key: &str) -> Result<CodingSessionTarget, CliError> {
    const PREFIX: &str = "coding-session/v1|";
    let bad = || CliError::Usage(format!("--target is not a valid cs-target key: {key}"));

    let mut rest = key.strip_prefix(PREFIX).ok_or_else(bad)?;
    let mut fields: Vec<String> = Vec::with_capacity(4);
    for _ in 0..4 {
        let colon = rest.find(':').ok_or_else(bad)?;
        let (len_str, tail) = rest.split_at(colon);
        let len: usize = len_str.parse().map_err(|_| bad())?;
        let tail = tail.get(1..).ok_or_else(bad)?; // skip the ':' itself
        if tail.len() < len || !tail.is_char_boundary(len) {
            return Err(bad());
        }
        let (field, remainder) = tail.split_at(len);
        fields.push(field.to_owned());
        rest = remainder;
    }
    if !rest.is_empty() {
        return Err(bad());
    }
    let generation: u64 = fields[3].parse().map_err(|_| bad())?;
    let target = CodingSessionTarget {
        driver: fields[0].clone(),
        instance_id: fields[1].clone(),
        session_id: fields[2].clone(),
        generation,
    };
    if coding_session_target_key(&target) != key {
        return Err(bad());
    }
    Ok(target)
}

/// Resolve the addressed target from `--target` or the four separate flags.
/// Clap's `conflicts_with_all` on `--target` already refuses mixing the two
/// forms; this only has to require all four parts when `--target` is absent.
pub(crate) fn resolve_target(
    target_key: Option<&str>,
    driver: Option<&str>,
    instance_id: Option<&str>,
    session_id: Option<&str>,
    generation: Option<u64>,
) -> Result<CodingSessionTarget, CliError> {
    if let Some(key) = target_key {
        return parse_target_key(key);
    }
    match (driver, instance_id, session_id, generation) {
        (Some(driver), Some(instance_id), Some(session_id), Some(generation)) => {
            Ok(CodingSessionTarget {
                driver: driver.to_owned(),
                instance_id: instance_id.to_owned(),
                session_id: session_id.to_owned(),
                generation,
            })
        }
        _ => Err(CliError::Usage(
            "the target requires either --target <cs-target key> or all four of --driver, \
             --instance-id, --session-id, and --generation"
                .into(),
        )),
    }
}

/// The first receipt this registration earned: durable admission or an
/// early refusal. Never a later turn stage — those arrive only after the
/// named CI result is recorded, long after any reasonable `--ack-timeout`.
pub(crate) struct RegistrationAck {
    pub(crate) status: ReceiptStatus,
    pub(crate) receipt_event_id: String,
    pub(crate) refusal_code: Option<String>,
    pub(crate) refusal_message: Option<String>,
}

/// Scan one page of raw relay events for the registration/refusal receipt
/// that answers `command_id` for exactly `target`. A receipt naming the same
/// `command_id` but a different target is not this registration's answer —
/// it is ignored, not treated as a match.
pub(crate) fn find_registration_ack(
    events: &[Value],
    command_id: &str,
    target: &CodingSessionTarget,
) -> Option<RegistrationAck> {
    let mut best: Option<(i64, RegistrationAck)> = None;
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_LIFECYCLE_RECEIPT))
        {
            continue;
        }
        let Some(content) = event.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Ok(receipt) = decode_coding_session_lifecycle_receipt(content) else {
            continue;
        };
        if receipt.command_id != command_id {
            continue;
        }
        if !matches!(
            receipt.status,
            ReceiptStatus::ContinuationRegistered | ReceiptStatus::TurnRefused
        ) {
            continue;
        }
        if receipt.session.as_ref() != Some(target) {
            continue;
        }
        let (Some(created_at), Some(event_id)) = (
            event.get("created_at").and_then(Value::as_i64),
            event.get("id").and_then(Value::as_str),
        ) else {
            continue;
        };
        let ack = RegistrationAck {
            status: receipt.status,
            receipt_event_id: event_id.to_owned(),
            refusal_code: receipt.error.as_ref().map(|error| error.code.clone()),
            refusal_message: receipt.error.as_ref().map(|error| error.message.clone()),
        };
        let take = match &best {
            Some((best_at, _)) => created_at < *best_at,
            None => true,
        };
        if take {
            best = Some((created_at, ack));
        }
    }
    best.map(|(_, ack)| ack)
}

/// Poll the relay for the registration/refusal receipt until it appears or
/// `ack_timeout_secs` elapses. `None` means the timeout elapsed with no
/// answer — a real, reportable outcome (`unconfirmed`), never smoothed into
/// success or failure. Query failures inside the window are retried rather
/// than raised: the write already landed, and a transient read error must
/// not be reported as if the registration itself failed.
async fn await_registration_ack(
    client: &BuzzClient,
    channel_id: &str,
    command_id: &str,
    target: &CodingSessionTarget,
    since: i64,
    ack_timeout_secs: u64,
) -> Option<RegistrationAck> {
    let deadline = std::time::Instant::now() + Duration::from_secs(ack_timeout_secs);
    let filter = json!({
        "kinds": [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
        "#h": [channel_id],
        "since": since,
    });
    loop {
        if let Ok(events) = client.query_all(filter.clone()).await {
            if let Some(ack) = find_registration_ack(&events, command_id, target) {
                return Some(ack);
            }
        }
        if std::time::Instant::now() + ACK_POLL >= deadline {
            return None;
        }
        tokio::time::sleep(ACK_POLL).await;
    }
}

fn print_output(format: &OutputFormat, mut value: Value, compact_drop: &[&str]) {
    if matches!(format, OutputFormat::Compact) {
        if let Some(object) = value.as_object_mut() {
            for field in compact_drop {
                object.remove(*field);
            }
        }
    }
    println!("{value}");
}

/// `bee ci continue` — register a turn to run when one exact CI result is
/// recorded, then wait (bounded) for the relay's registration or refusal
/// receipt.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_continue(
    client: &BuzzClient,
    format: &OutputFormat,
    channel_id: &str,
    target_key: Option<&str>,
    driver: Option<&str>,
    instance_id: Option<&str>,
    session_id: Option<&str>,
    generation: Option<u64>,
    project: String,
    repository: String,
    commit: String,
    check: String,
    run: String,
    attempt: u32,
    workflow: String,
    phase: CiPhaseArg,
    continuation: &str,
    expires_in: u64,
    ack_timeout: u64,
) -> Result<(), CliError> {
    let channel = uuid::Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    let channel_id = channel.to_string();
    let target = resolve_target(target_key, driver, instance_id, session_id, generation)?;

    let identity = CiResultIdentity {
        project,
        repository,
        commit,
        check,
        run,
        attempt,
        workflow,
        phase: phase.into(),
    };
    validate_identity(&identity).map_err(CliError::Usage)?;
    let operation_id = correlation_id(&identity).map_err(CliError::Usage)?;

    let continuation_text = read_continuation(continuation)?;
    if continuation_text.trim().is_empty() {
        return Err(CliError::Usage("--continuation must not be empty".into()));
    }

    let now = chrono::Utc::now().timestamp();
    let now = u64::try_from(now)
        .map_err(|_| CliError::Other("system clock is before the Unix epoch".into()))?;
    let expires_at = now
        .checked_add(expires_in)
        .ok_or_else(|| CliError::Usage("--expires-in is too large".into()))?;

    let target_key = coding_session_target_key(&target);
    let command_id = ci_continuation_command_id(
        &channel_id,
        &operation_id,
        &target_key,
        expires_at,
        &continuation_text,
    );

    let builder = build_coding_session_ci_continuation(
        channel,
        &command_id,
        &target,
        &identity,
        &continuation_text,
        expires_at,
    )
    .map_err(sdk_err)?;
    // Exactly the envelope `validate_coding_session_command_envelope`
    // accepts (h, cs-v, cs-target — nothing else): `sign_event` would inject
    // a fourth NIP-OA `auth` tag and make the event invalid. Same reasoning
    // as `build_turn_command` in `commands/sessions/crew_cmds.rs`.
    let event = client.sign_event_unchecked(builder)?;
    let registered_event_id = event.id.to_hex();

    // Taken before the write so a receipt published in the same second as
    // the command cannot fall outside the window, with a second of slack for
    // clock skew between this host and the relay's.
    let since = chrono::Utc::now().timestamp() - 1;
    let raw = client.submit_event(event).await?;
    parse_write_response(&raw, "CI-continuation registration already accepted")?;

    let ack = await_registration_ack(
        client,
        &channel_id,
        &command_id,
        &target,
        since,
        ack_timeout,
    )
    .await;

    match ack {
        None => {
            print_output(
                format,
                json!({
                    "commandId": command_id,
                    "target": target_key,
                }),
                &[],
            );
            Err(CliError::Unconfirmed(format!(
                "no registration or refusal receipt for CI-continuation {command_id} within the \
                 ack timeout; re-running this exact command reproduces the same commandId and \
                 names the same registration rather than minting a second one"
            )))
        }
        Some(ack) => match ack.status {
            ReceiptStatus::ContinuationRegistered => {
                print_output(
                    format,
                    json!({
                        "commandId": command_id,
                        "target": target_key,
                        "operationId": operation_id,
                        "expiresAt": expires_at,
                        "registeredEventId": registered_event_id,
                        "receiptEventId": ack.receipt_event_id,
                    }),
                    &["operationId"],
                );
                Ok(())
            }
            ReceiptStatus::TurnRefused => {
                let code = ack.refusal_code.unwrap_or_default();
                let message = ack.refusal_message.unwrap_or_default();
                print_output(
                    format,
                    json!({
                        "commandId": command_id,
                        "target": target_key,
                        "code": code,
                        "message": message,
                        "receiptEventId": ack.receipt_event_id,
                    }),
                    &[],
                );
                Err(CliError::Refused(format!(
                    "CI-continuation registration {command_id} was refused: {code}: {message}"
                )))
            }
            // `find_registration_ack` only ever returns these two statuses.
            _ => Err(CliError::Other(format!(
                "unexpected receipt status for CI-continuation registration {command_id}"
            ))),
        },
    }
}

/// The stages `bee ci continuation status` reports, in the vocabulary
/// [`docs/CI_MANAGED_CONTINUATION_IMPL.md`] §2 defines: the wire
/// [`ReceiptStatus`] values a CI-continuation command can ever produce,
/// spelled without the `turn_`/`continuation_` prefixes redundant once the
/// caller already knows they asked about one.
/// How far a continuation has progressed, for ordering receipts that share a
/// second: registered, then queued, then started; a refusal or drop is the
/// end of the road.
fn stage_rank(status: ReceiptStatus) -> u8 {
    match status {
        ReceiptStatus::ContinuationRegistered => 0,
        ReceiptStatus::TurnQueued => 1,
        ReceiptStatus::TurnStarted => 2,
        _ => 3,
    }
}

pub(crate) fn stage_str(status: ReceiptStatus) -> &'static str {
    match status {
        ReceiptStatus::ContinuationRegistered => "registered",
        ReceiptStatus::TurnQueued => "queued",
        ReceiptStatus::TurnStarted => "started",
        ReceiptStatus::TurnRefused => "refused",
        ReceiptStatus::TurnDropped => "dropped",
        _ => "none",
    }
}

/// Whether a decoded receipt status is one a CI-continuation command can
/// produce, at registration or at eventual delivery.
pub(crate) fn is_continuation_status(status: ReceiptStatus) -> bool {
    matches!(
        status,
        ReceiptStatus::ContinuationRegistered
            | ReceiptStatus::TurnQueued
            | ReceiptStatus::TurnStarted
            | ReceiptStatus::TurnRefused
            | ReceiptStatus::TurnDropped
    )
}

/// The resolved answer to `bee ci continuation status`: the latest stage
/// among the receipts naming one `commandId`, every receipt event id that
/// contributed, and the refusal code when the latest stage is `refused`.
pub(crate) struct ContinuationStatus {
    pub(crate) stage: &'static str,
    pub(crate) receipt_event_ids: Vec<String>,
    pub(crate) refusal_code: Option<String>,
}

/// Fold a page of raw relay events into the [`ContinuationStatus`] for one
/// `command_id`. Pure and synchronous so it is testable without a mock relay;
/// [`cmd_continuation_status`] is the thin async wrapper that fetches the
/// page and prints this. "Latest" breaks ties on `(created_at, event id)` —
/// the same ordering `resolve_sessions` in `commands/sessions.rs` uses for a
/// same-second burst, since a provider can legitimately publish more than one
/// receipt inside one wall-clock second.
pub(crate) fn resolve_continuation_status(
    events: &[Value],
    command_id: &str,
) -> ContinuationStatus {
    let mut matches: Vec<(i64, String, ReceiptStatus, Option<String>)> = Vec::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_LIFECYCLE_RECEIPT))
        {
            continue;
        }
        let Some(content) = event.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Ok(receipt) = decode_coding_session_lifecycle_receipt(content) else {
            continue;
        };
        if receipt.command_id != command_id || !is_continuation_status(receipt.status) {
            continue;
        }
        let (Some(created_at), Some(event_id)) = (
            event.get("created_at").and_then(Value::as_i64),
            event.get("id").and_then(Value::as_str),
        ) else {
            continue;
        };
        matches.push((
            created_at,
            event_id.to_owned(),
            receipt.status,
            receipt.error.map(|error| error.code),
        ));
    }
    // Receipts for one command can share a second. Their event ids are content
    // hashes, unrelated to which came first, so ties are broken by how far the
    // stage has progressed: a `turn_started` in the same second as its
    // `turn_queued` is the later fact. The id is only the last-resort order.
    matches.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| stage_rank(a.2).cmp(&stage_rank(b.2)))
            .then_with(|| a.1.cmp(&b.1))
    });

    let stage = matches
        .last()
        .map_or("none", |(_, _, status, _)| stage_str(*status));
    let refusal_code = matches.last().and_then(|(_, _, _, code)| code.clone());
    let receipt_event_ids = matches.into_iter().map(|(_, id, _, _)| id).collect();

    ContinuationStatus {
        stage,
        receipt_event_ids,
        refusal_code,
    }
}

/// `bee ci continuation status` — read the latest receipt stage for one
/// registration `commandId`. Read-only: stored replay only, no wait, no
/// write.
pub async fn cmd_continuation_status(
    client: &BuzzClient,
    format: &OutputFormat,
    channel_id: &str,
    command_id: &str,
) -> Result<(), CliError> {
    uuid::Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;

    let filter = json!({
        "kinds": [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
        "#h": [channel_id],
    });
    let events = client.query_all(filter).await?;
    let status = resolve_continuation_status(&events, command_id);

    print_output(
        format,
        json!({
            "commandId": command_id,
            "stage": status.stage,
            "receiptEventIds": status.receipt_event_ids,
            "refusalCode": status.refusal_code,
        }),
        &["receiptEventIds"],
    );
    Ok(())
}
