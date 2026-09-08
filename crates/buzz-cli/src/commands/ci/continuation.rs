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
//! its inputs (channel, CI identity, target, absolute expiry, continuation
//! text). An exact retry uses the printed `--expires-at` value and names the
//! same registration rather than minting a second one.

use std::time::Duration;

use nostr::PublicKey;
use serde_json::{json, Value};

use buzz_core::ci_result::{correlation_id, validate_identity, CiResultIdentity};
use buzz_core::coding_session_command::{
    ci_continuation_command_id, coding_session_target_key, CodingSessionTarget,
};
use buzz_core::coding_session_payload::{decode_coding_session_lifecycle_receipt, ReceiptStatus};
use buzz_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;
use buzz_sdk::builders::{
    build_coding_session_ci_continuation, coding_session_turn_receipt_semantic_key,
};
use buzz_sdk::coding_session::CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION;

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
const DEFAULT_EXPIRES_IN_SECONDS: u64 = 86_400;

/// Parse and canonicalize the provider signing key named by the caller.
pub(crate) fn normalize_provider(provider: &str) -> Result<String, CliError> {
    PublicKey::from_hex(provider)
        .map(|key| key.to_hex())
        .map_err(|error| CliError::Usage(format!("--provider is not a valid pubkey: {error}")))
}

/// Resolve the wire's absolute expiry from a first-attempt duration or a
/// retry's already printed timestamp.
pub(crate) fn resolve_expires_at(
    now: u64,
    expires_in: Option<u64>,
    expires_at: Option<u64>,
) -> Result<u64, CliError> {
    match (expires_in, expires_at) {
        (Some(_), Some(_)) => Err(CliError::Usage(
            "--expires-in and --expires-at are mutually exclusive".into(),
        )),
        (_, Some(expires_at)) => Ok(expires_at),
        (expires_in, None) => now
            .checked_add(expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECONDS))
            .ok_or_else(|| CliError::Usage("--expires-in is too large".into())),
    }
}

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
#[derive(Debug)]
pub(crate) struct RegistrationAck {
    pub(crate) status: ReceiptStatus,
    pub(crate) receipt_event_id: String,
    pub(crate) refusal_code: Option<String>,
    pub(crate) refusal_message: Option<String>,
}

struct TrustedReceipt {
    created_at: u64,
    event_id: String,
    receipt: buzz_core::coding_session_payload::LifecycleReceipt,
}

fn trusted_receipt(
    raw: &Value,
    channel_id: &str,
    command_id: &str,
    provider: &str,
) -> Option<TrustedReceipt> {
    let event = serde_json::from_value::<nostr::Event>(raw.clone()).ok()?;
    if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_LIFECYCLE_RECEIPT
        || event.pubkey.to_hex() != provider
        || buzz_core::verify_event(&event).is_err()
    {
        return None;
    }
    let receipt = decode_coding_session_lifecycle_receipt(&event.content).ok()?;
    if receipt.command_id != command_id || !is_continuation_status(receipt.status) {
        return None;
    }
    let semantic_key = coding_session_turn_receipt_semantic_key(command_id, receipt.status);
    let expected = [
        ["h", channel_id],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", command_id],
        ["csl-key", semantic_key.as_str()],
    ];
    if event.tags.len() != expected.len()
        || event.tags.iter().zip(expected).any(|(got, want)| {
            got.as_slice().len() != 2
                || got.as_slice()[0] != want[0]
                || got.as_slice()[1] != want[1]
        })
    {
        return None;
    }
    Some(TrustedReceipt {
        created_at: event.created_at.as_secs(),
        event_id: event.id.to_hex(),
        receipt,
    })
}

/// Scan one page of raw relay events for the registration/refusal receipt
/// that answers `command_id` for exactly `target`. An otherwise trusted
/// receipt naming a different target contradicts the registration and is
/// reported rather than ignored.
pub(crate) fn find_registration_ack(
    events: &[Value],
    channel_id: &str,
    command_id: &str,
    target: &CodingSessionTarget,
    provider: &str,
) -> Result<Option<RegistrationAck>, String> {
    let mut best: Option<(u64, RegistrationAck)> = None;
    let mut refusal: Option<buzz_core::coding_session_payload::LifecycleReceipt> = None;
    for raw in events {
        let Some(trusted) = trusted_receipt(raw, channel_id, command_id, provider) else {
            continue;
        };
        let receipt = trusted.receipt;
        if !matches!(
            receipt.status,
            ReceiptStatus::ContinuationRegistered | ReceiptStatus::TurnRefused
        ) {
            continue;
        }
        if receipt.session.as_ref() != Some(target) {
            return Err(format!(
                "provider {provider} published a lifecycle receipt for command {command_id} with a conflicting target"
            ));
        }
        if receipt.status == ReceiptStatus::TurnRefused {
            if refusal
                .as_ref()
                .is_some_and(|existing| existing != &receipt)
            {
                return Err(format!(
                    "provider {provider} published conflicting refusal receipts for command {command_id}"
                ));
            }
            refusal = Some(receipt.clone());
        }
        let ack = RegistrationAck {
            status: receipt.status,
            receipt_event_id: trusted.event_id,
            refusal_code: receipt.error.as_ref().map(|error| error.code.clone()),
            refusal_message: receipt.error.as_ref().map(|error| error.message.clone()),
        };
        let take = match &best {
            Some((best_at, best_ack)) => {
                (
                    trusted.created_at,
                    stage_rank(ack.status),
                    &ack.receipt_event_id,
                ) < (
                    *best_at,
                    stage_rank(best_ack.status),
                    &best_ack.receipt_event_id,
                )
            }
            None => true,
        };
        if take {
            best = Some((trusted.created_at, ack));
        }
    }
    Ok(best.map(|(_, ack)| ack))
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
    provider: &str,
    ack_timeout_secs: u64,
) -> Result<Option<RegistrationAck>, CliError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(ack_timeout_secs);
    let filter = registration_receipt_filter(channel_id, command_id, provider);
    loop {
        if let Ok(events) = client.query_all(filter.clone()).await {
            if let Some(ack) =
                find_registration_ack(&events, channel_id, command_id, target, provider)
                    .map_err(CliError::Other)?
            {
                return Ok(Some(ack));
            }
        }
        if std::time::Instant::now() + ACK_POLL >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(ACK_POLL).await;
    }
}

pub(crate) fn registration_receipt_filter(
    channel_id: &str,
    command_id: &str,
    provider: &str,
) -> Value {
    json!({
        "kinds": [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
        "authors": [provider],
        "#h": [channel_id],
        "#csl-command": [command_id],
    })
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
    expires_in: Option<u64>,
    expires_at: Option<u64>,
    ack_timeout: u64,
    provider: &str,
) -> Result<(), CliError> {
    let channel = uuid::Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    let channel_id = channel.to_string();
    let target = resolve_target(target_key, driver, instance_id, session_id, generation)?;
    let provider = normalize_provider(provider)?;

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
    let expires_at = resolve_expires_at(now, expires_in, expires_at)?;

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

    let raw = client.submit_event(event).await?;
    parse_write_response(&raw, "CI-continuation registration already accepted")?;

    let ack = await_registration_ack(
        client,
        &channel_id,
        &command_id,
        &target,
        &provider,
        ack_timeout,
    )
    .await?;

    match ack {
        None => {
            print_output(
                format,
                json!({
                    "commandId": command_id,
                    "target": target_key,
                    "provider": provider,
                    "expiresAt": expires_at,
                }),
                &[],
            );
            Err(CliError::Unconfirmed(format!(
                "no trusted registration or refusal receipt for CI-continuation {command_id} \
                 from provider {provider} within the ack timeout; retry with --expires-at \
                 {expires_at}, without --expires-in, and with the same remaining inputs to \
                 reproduce this commandId"
            )))
        }
        Some(ack) => match ack.status {
            ReceiptStatus::ContinuationRegistered => {
                print_output(
                    format,
                    json!({
                        "commandId": command_id,
                        "target": target_key,
                        "provider": provider,
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
                        "provider": provider,
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
#[derive(Debug)]
pub(crate) struct ContinuationStatus {
    pub(crate) stage: &'static str,
    pub(crate) receipt_event_ids: Vec<String>,
    pub(crate) refusal_code: Option<String>,
}

/// Fold a page of raw relay events into the [`ContinuationStatus`] for one
/// `command_id`. Pure and synchronous so it is testable without a mock relay;
/// [`cmd_continuation_status`] is the thin async wrapper that fetches the
/// page and prints this. Same-second receipts are ordered by stage before
/// event id, while conflicting terminal facts are rejected rather than
/// selected by their unrelated content hashes.
pub(crate) fn resolve_continuation_status(
    events: &[Value],
    channel_id: &str,
    command_id: &str,
    target: &CodingSessionTarget,
    provider: &str,
) -> Result<ContinuationStatus, String> {
    let mut matches: Vec<(u64, String, ReceiptStatus, Option<String>)> = Vec::new();
    let mut terminal: Option<buzz_core::coding_session_payload::LifecycleReceipt> = None;
    for raw in events {
        let Some(trusted) = trusted_receipt(raw, channel_id, command_id, provider) else {
            continue;
        };
        let receipt = trusted.receipt;
        if receipt.session.as_ref() != Some(target) {
            return Err(format!(
                "provider {provider} published a lifecycle receipt for command {command_id} with a conflicting target"
            ));
        }
        if matches!(
            receipt.status,
            ReceiptStatus::TurnStarted | ReceiptStatus::TurnRefused | ReceiptStatus::TurnDropped
        ) {
            if terminal
                .as_ref()
                .is_some_and(|existing| existing != &receipt)
            {
                return Err(format!(
                    "provider {provider} published conflicting terminal receipts for command {command_id}"
                ));
            }
            terminal = Some(receipt.clone());
        }
        matches.push((
            trusted.created_at,
            trusted.event_id,
            receipt.status,
            receipt.error.map(|error| error.code),
        ));
    }
    // Stage progression is monotonic, including when receipts share a second;
    // created_at orders duplicates within one stage and the content hash is
    // only the last resort. Distinct terminal facts were rejected above.
    matches.sort_by(|a, b| {
        stage_rank(a.2)
            .cmp(&stage_rank(b.2))
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });

    let stage = matches
        .last()
        .map_or("none", |(_, _, status, _)| stage_str(*status));
    let refusal_code = matches.last().and_then(|(_, _, _, code)| code.clone());
    let receipt_event_ids = matches.into_iter().map(|(_, id, _, _)| id).collect();

    Ok(ContinuationStatus {
        stage,
        receipt_event_ids,
        refusal_code,
    })
}

/// `bee ci continuation status` — read the latest receipt stage for one
/// registration `commandId`. Read-only: stored replay only, no wait, no
/// write.
pub async fn cmd_continuation_status(
    client: &BuzzClient,
    format: &OutputFormat,
    channel_id: &str,
    command_id: &str,
    target_key: &str,
    provider: &str,
) -> Result<(), CliError> {
    let channel_id = uuid::Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    let channel_id = channel_id.to_string();
    let target = parse_target_key(target_key)?;
    let provider = normalize_provider(provider)?;

    let filter = registration_receipt_filter(&channel_id, command_id, &provider);
    let events = client.query_all(filter).await?;
    let status = resolve_continuation_status(&events, &channel_id, command_id, &target, &provider)
        .map_err(CliError::Other)?;

    print_output(
        format,
        json!({
            "commandId": command_id,
            "target": target_key,
            "provider": provider,
            "stage": status.stage,
            "receiptEventIds": status.receipt_event_ids,
            "refusalCode": status.refusal_code,
        }),
        &["receiptEventIds"],
    );
    Ok(())
}
