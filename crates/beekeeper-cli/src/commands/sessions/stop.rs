//! `bee sessions stop` — the CLI form of the kind 44221 `session.stop`
//! lifecycle action the desktop sends.
//!
//! An unattended controller needs to stop an execution at a deadline and
//! confirm it ended. This publishes exactly the command the desktop's
//! `buildCodingSessionStopEvent` publishes — same schema, a fresh `commandId`,
//! the exact current generation's `cs-target` — through the same builder,
//! signer and write path `sessions create` uses.
//!
//! Every refusal happens before anything is signed: a session this channel
//! has no record of, one already stopped, or an authority that is not the
//! provider signing the execution's records. The provider would refuse the
//! last two itself, or ignore the command as not addressed to it, and a
//! command nobody will answer is a wait that can only end `unconfirmed`.

use serde_json::{json, Value};
use uuid::Uuid;

use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, ReceiptError, ReceiptStatus,
};
use beekeeper_sdk::builders::build_coding_session_lifecycle_command;
use beekeeper_sdk::kind::{KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA};

use super::crew::stopped_status_word;
use super::crew_cmds::{
    await_command_receipt, submit_with, verified_command_receipts, ReceiptWait,
    CREATE_WAIT_DEFAULT_SECONDS, CREATE_WAIT_MAX_SECONDS,
};
use super::{
    content_of, decode_metadata, decode_receipts, fetch_channel_events, resolve_sessions,
    ReceiptRecord, SessionRow,
};
use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{sdk_err, validate_lower_hex64};

/// Resolve the exact current generation `--session` names in this channel,
/// or refuse with the reason nothing may be published.
///
/// `rows` and `receipts` are what `sessions list` resolves from the channel.
/// Several generations of one execution are not an ambiguity: the newest is
/// the one a stop must name, because a provider refuses any other as
/// `STALE_GENERATION`. Several distinct executions sharing the id are.
pub(super) fn plan_stop<'a>(
    rows: &'a [SessionRow],
    receipts: &[ReceiptRecord],
    channel_id: &str,
    session_id: &str,
    provider_authority: &str,
) -> Result<&'a SessionRow, CliError> {
    let mut current: Vec<&SessionRow> = Vec::new();
    for row in rows
        .iter()
        .filter(|row| row.target.session_id == session_id)
    {
        let same = |held: &&SessionRow| {
            held.signer == row.signer
                && held.target.driver == row.target.driver
                && held.target.instance_id == row.target.instance_id
        };
        match current.iter_mut().find(|held| same(held)) {
            Some(held) if row.target.generation > held.target.generation => *held = row,
            Some(_) => {}
            None => current.push(row),
        }
    }
    let row = match current.as_slice() {
        [] => {
            return Err(CliError::NotFound(format!(
                "no execution with sessionId '{session_id}' in channel {channel_id}; nothing was \
                 published. `bee sessions list --channel {channel_id}` shows the sessionId \
                 inside each row's target"
            )))
        }
        [one] => *one,
        many => {
            return Err(CliError::Usage(format!(
                "sessionId '{session_id}' names {} different executions in this channel; nothing \
                 was published: {}",
                many.len(),
                many.iter()
                    .map(|row| format!("{} (signed by {})", row.target_key, row.signer))
                    .collect::<Vec<_>>()
                    .join(", ")
            )))
        }
    };
    if !row.signer.eq_ignore_ascii_case(provider_authority) {
        return Err(CliError::Refused(format!(
            "--provider-authority {provider_authority} is not the provider of {}, which is \
             signed by {}; nothing was published (that provider would ignore the stop as not \
             addressed to it)",
            row.target_key, row.signer
        )));
    }
    let stopped_receipt = receipts.iter().any(|record| {
        record.signer == row.signer
            && record.target_key.as_deref() == Some(row.target_key.as_str())
            && content_of(&record.raw)
                .and_then(|content| decode_coding_session_lifecycle_receipt(content).ok())
                .is_some_and(|receipt| receipt.status == ReceiptStatus::Stopped)
    });
    if row.status == stopped_status_word() || stopped_receipt {
        return Err(CliError::Refused(format!(
            "{} is already stopped; nothing was published",
            row.target_key
        )));
    }
    Ok(row)
}

/// What the provider answered to one stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StopAnswer {
    /// A signed `stopped` receipt naming the exact target.
    Stopped { receipt_event_id: String },
    /// A signed `failed` receipt: the provider refused the stop.
    Refused {
        receipt_event_id: String,
        error: ReceiptError,
    },
}

/// Select the provider's signed answer to one stop from the receipt query.
///
/// Receipts are admitted by [`verified_command_receipts`], the same gate the
/// create wait uses. A `stopped` receipt naming a different target, or two
/// answers that disagree, is a contradiction reported as such.
pub(super) fn classify_stop_receipts(
    events: &[Value],
    channel_id: &str,
    command_id: &str,
    provider_authority: &str,
    target: &CodingSessionTarget,
) -> Result<Option<StopAnswer>, String> {
    let mut answer: Option<StopAnswer> = None;
    for (receipt_event_id, receipt) in
        verified_command_receipts(events, channel_id, command_id, provider_authority)
    {
        let candidate = match receipt.status {
            ReceiptStatus::Stopped => {
                if receipt.session.as_ref() != Some(target) {
                    return Err(format!(
                        "provider receipt {receipt_event_id} for stop {command_id} says stopped \
                         but names a different target than {}",
                        coding_session_target_key(target)
                    ));
                }
                StopAnswer::Stopped { receipt_event_id }
            }
            ReceiptStatus::Failed => {
                let Some(error) = receipt.error else {
                    continue;
                };
                StopAnswer::Refused {
                    receipt_event_id,
                    error,
                }
            }
            _ => continue,
        };
        let same = |left: &StopAnswer, right: &StopAnswer| match (left, right) {
            (StopAnswer::Stopped { .. }, StopAnswer::Stopped { .. }) => true,
            (StopAnswer::Refused { error: left, .. }, StopAnswer::Refused { error: right, .. }) => {
                left == right
            }
            _ => false,
        };
        match &answer {
            Some(existing) if !same(existing, &candidate) => {
                return Err(format!(
                    "provider published conflicting lifecycle receipts for stop {command_id}"
                ));
            }
            Some(_) => {}
            None => answer = Some(candidate),
        }
    }
    Ok(answer)
}

/// `bee sessions stop`: publish one `session.stop` for the current generation
/// of `session_id`, and with `wait` report the provider's signed answer.
///
/// Exit codes: 0 published (and, with `--wait`, confirmed `stopped`); 1 a
/// refusal before publishing, or the provider's signed refusal; 5 the relay
/// accepted the command and no receipt arrived in time (`unconfirmed`).
pub async fn cmd_stop(
    client: &BuzzClient,
    channel_id: &str,
    session_id: &str,
    provider_authority: &str,
    wait: bool,
    timeout_secs: Option<u64>,
) -> Result<(), CliError> {
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    validate_lower_hex64("--provider-authority", provider_authority)?;
    if session_id.trim().is_empty() {
        return Err(CliError::Usage("--session must not be empty".to_owned()));
    }
    let timeout_secs = match (wait, timeout_secs) {
        (true, timeout) => {
            let timeout = timeout.unwrap_or(CREATE_WAIT_DEFAULT_SECONDS);
            if !(1..=CREATE_WAIT_MAX_SECONDS).contains(&timeout) {
                return Err(CliError::Usage(format!(
                    "--timeout-secs must be between 1 and {CREATE_WAIT_MAX_SECONDS} seconds"
                )));
            }
            timeout
        }
        (false, Some(_)) => {
            return Err(CliError::Usage("--timeout-secs requires --wait".to_owned()))
        }
        (false, None) => 0,
    };
    // The signed event carries the canonical spelling; read and wait on it.
    let canonical_channel = channel.to_string();
    let channel_id = canonical_channel.as_str();

    let events = fetch_channel_events(
        client,
        channel_id,
        &[
            KIND_CODING_SESSION_METADATA,
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        ],
    )
    .await?;
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let rows = resolve_sessions(&metadata, &receipts, &[]);
    let row = plan_stop(&rows, &receipts, channel_id, session_id, provider_authority)?;
    let target = row.target.clone();
    let target_key = row.target_key.clone();

    let command_id = Uuid::new_v4().to_string();
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        action: CodingSessionLifecycleAction::SessionStop {
            session: target.clone(),
            provider_authority_pubkey: provider_authority.to_owned(),
        },
    };
    let builder = build_coding_session_lifecycle_command(channel, &payload).map_err(sdk_err)?;
    let event = client.sign_event_unchecked(builder)?;
    let mut merged = submit_with(
        client,
        event,
        "lifecycle command already accepted",
        json!({ "commandId": command_id, "target": target_key }),
    )
    .await?;
    if !wait {
        println!("{merged}");
        return Ok(());
    }

    let outcome = await_command_receipt(client, channel_id, &command_id, timeout_secs, |events| {
        classify_stop_receipts(events, channel_id, &command_id, provider_authority, &target)
    })
    .await;
    let (outcome_word, receipt, result) = match outcome {
        ReceiptWait::Answered(StopAnswer::Stopped { receipt_event_id }) => (
            "stopped",
            json!({ "eventId": receipt_event_id, "status": "stopped", "error": null }),
            Ok(()),
        ),
        ReceiptWait::Answered(StopAnswer::Refused {
            receipt_event_id,
            error,
        }) => {
            let detail = format!(
                "provider refused the stop of {target_key}: {} ({})",
                error.message, error.code
            );
            (
                "refused",
                json!({ "eventId": receipt_event_id, "status": "failed", "error": error }),
                Err(CliError::Refused(detail)),
            )
        }
        ReceiptWait::Conflicted(detail) => {
            ("conflicted", Value::Null, Err(CliError::Refused(detail)))
        }
        ReceiptWait::Unconfirmed { query_failed } => (
            "unconfirmed",
            Value::Null,
            Err(CliError::Unconfirmed(format!(
                "relay accepted stop {command_id} for {target_key}, but {} within \
                 {timeout_secs}s; the execution may or may not have stopped, and no second stop \
                 was sent — query receipts for this commandId before acting",
                if query_failed {
                    "receipt queries failed or did not complete"
                } else {
                    "no signed receipt arrived"
                }
            ))),
        ),
    };
    if let Some(object) = merged.as_object_mut() {
        object.insert("waited".into(), json!(true));
        object.insert("outcome".into(), json!(outcome_word));
        object.insert("receipt".into(), receipt);
    }
    println!("{merged}");
    result
}
