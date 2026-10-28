//! Operator resume through the existing NIP-CSL lifecycle contract.
//! The provider owns admission and context recovery; this client never replays a turn.

use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_payload::{LifecycleReceipt, ReceiptStatus};
use beekeeper_core::coding_session_title::parse_coding_session_target_key;
use beekeeper_sdk::builders::build_coding_session_lifecycle_command;
use beekeeper_sdk::kind::{KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA};
use serde_json::{json, Value};
use uuid::Uuid;

use super::crew::{build_executions, resolve_send_target, CrewExecution};
use super::crew_cmds::{
    await_command_receipt, submit_with, verified_command_receipts, ReceiptWait,
    CREATE_WAIT_DEFAULT_SECONDS, CREATE_WAIT_MAX_SECONDS,
};
use super::{decode_metadata, decode_receipts, fetch_channel_events};
use crate::{client::BeekeeperClient, error::CliError, validate::sdk_err, SessionsResumeArgs};

pub(super) fn parse_target(to: &str) -> Result<CodingSessionTarget, CliError> {
    parse_coding_session_target_key(to)
        .map_err(|error| CliError::Usage(format!("--to requires an exact cs-target key: {error}")))
}

pub(super) fn resolve_exact<'a>(
    executions: &'a [CrewExecution],
    to: &str,
) -> Result<&'a CrewExecution, CliError> {
    let execution = resolve_send_target(executions, to, None)?;
    if execution.target_key != to {
        return Err(CliError::NotFound(format!(
            "no execution has exact cs-target {to}; nothing was published"
        )));
    }
    Ok(execution)
}

pub(super) fn resume_payload(
    command_id: &str,
    target: &CodingSessionTarget,
    provider: &str,
) -> CodingSessionLifecycleCommandPayload {
    CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        action: CodingSessionLifecycleAction::SessionResume {
            session: target.clone(),
            provider_authority_pubkey: provider.to_owned(),
        },
    }
}

/// Preserve the entire signed answer, including runtime-context disclosure in error.message.
#[derive(Debug, Clone)]
pub(super) struct ResumeAnswer {
    pub event_id: String,
    pub receipt: LifecycleReceipt,
}

pub(super) fn classify_resume_receipts(
    events: &[Value],
    channel: &str,
    command: &str,
    provider: &str,
    target: &CodingSessionTarget,
) -> Result<Option<ResumeAnswer>, String> {
    let mut answer: Option<ResumeAnswer> = None;
    for (event_id, receipt) in verified_command_receipts(events, channel, command, provider) {
        match receipt.status {
            ReceiptStatus::Resumed | ReceiptStatus::ResumedWithoutContext => {
                let next = CodingSessionTarget {
                    generation: target.generation + 1,
                    ..target.clone()
                };
                if receipt.session.as_ref() != Some(&next) || receipt.rewind.is_some() {
                    return Err(format!(
                        "provider receipt {event_id} for resume {command} does not confirm the next generation {} of this execution",
                        coding_session_target_key(&next)
                    ));
                }
            }
            ReceiptStatus::Failed => {}
            _ => continue,
        }
        if let Some(existing) = &answer {
            if existing.receipt != receipt {
                return Err(format!(
                    "provider published conflicting lifecycle receipts for resume {command}"
                ));
            }
        } else {
            answer = Some(ResumeAnswer { event_id, receipt });
        }
    }
    Ok(answer)
}

pub(super) fn resume_report(
    wait: ReceiptWait<ResumeAnswer>,
    command: &str,
    timeout_secs: u64,
) -> (Value, Result<(), CliError>) {
    match wait {
        ReceiptWait::Answered(answer) => {
            let receipt = answer.receipt;
            let refused = receipt.status == ReceiptStatus::Failed;
            let outcome = if refused {
                "refused"
            } else {
                receipt.status.as_str()
            };
            let summary = if refused {
                match &receipt.error {
                    Some(error) => format!(
                        "provider refused resume: {} ({})",
                        error.message, error.code
                    ),
                    None => "provider refused resume without a reason".into(),
                }
            } else {
                // `resumed` includes rehydration, and `resumed_without_context` can
                // retain runtime-owned context. Neither proves native restoration.
                format!("provider reports {outcome}; no prior turn was replayed by this command. Context details are preserved in the receipt; this is not proof of native context restoration or recovery of an old runtime run")
            };
            let session = (!refused)
                .then(|| receipt.session.as_ref().map(coding_session_target_key))
                .flatten();
            (
                json!({
                    "waited": true, "outcome": outcome, "session": session,
                    "summary": summary, "receiptEventId": answer.event_id, "receipt": receipt,
                }),
                if refused {
                    Err(CliError::Refused(summary))
                } else {
                    Ok(())
                },
            )
        }
        ReceiptWait::Conflicted(detail) => (
            json!({ "waited": true, "outcome": "conflicted", "session": null,
                "receiptEventId": null, "receipt": null, "summary": detail }),
            Err(CliError::Refused(detail)),
        ),
        ReceiptWait::Unconfirmed { query_failed } => {
            let detail = format!(
                "relay accepted resume {command}, but {} within {timeout_secs}s; resume is unconfirmed. No second resume was sent; query receipts for this commandId before acting",
                if query_failed { "receipt queries failed or did not complete" } else { "no signed provider receipt arrived" }
            );
            (
                json!({ "waited": true, "outcome": "unconfirmed", "session": null,
                "receiptEventId": null, "receipt": null, "summary": detail }),
                Err(CliError::Unconfirmed(detail)),
            )
        }
    }
}

// Keep the standard structured stderr category, exit code and retryable:false.
// As with plugins::with_result, attach reconciliation data to the error message.
pub(super) fn submission_error(
    error: CliError,
    command: &str,
    target: &str,
    event_id: &str,
) -> CliError {
    match error {
        CliError::DeliveryUnknown(detail) => {
            let result = json!({
                "commandId": command, "target": target, "event_id": event_id,
                "outcome": "delivery_unknown", "accepted": null,
            });
            CliError::DeliveryUnknown(format!(
                "resume acceptance is unknown; query receipts for this commandId before acting; do not retry with a new commandId. {detail}; result: {result}"
            ))
        }
        other => other,
    }
}

/// Publish one operator resume for the exact target and await its provider's answer.
pub async fn cmd_resume(
    client: &BeekeeperClient,
    args: &SessionsResumeArgs,
) -> Result<(), CliError> {
    let target = parse_target(&args.to)?;
    let channel = Uuid::parse_str(&args.channel)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    let channel_id = channel.to_string();
    let timeout = args.timeout_secs.unwrap_or(CREATE_WAIT_DEFAULT_SECONDS);
    if !(1..=CREATE_WAIT_MAX_SECONDS).contains(&timeout) {
        return Err(CliError::Usage(format!(
            "--timeout-secs must be between 1 and {CREATE_WAIT_MAX_SECONDS} seconds"
        )));
    }
    let events = fetch_channel_events(
        client,
        &channel_id,
        &[
            KIND_CODING_SESSION_METADATA,
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        ],
    )
    .await?;
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let executions = build_executions(
        &metadata,
        &receipts,
        &[],
        &Default::default(),
        chrono::Utc::now().timestamp(),
    );
    // Exact addressing deliberately does not upgrade stale generations or infer
    // liveness from metadata. Live/stopped/stale admission belongs to the provider.
    let execution = resolve_exact(&executions, &args.to)?;
    let provider = &execution.signer;
    let command = format!("resume-{}", Uuid::new_v4());
    let payload = resume_payload(&command, &target, provider);
    let builder = build_coding_session_lifecycle_command(channel, &payload).map_err(sdk_err)?;
    let event = client.sign_event_unchecked(builder)?;
    let event_id = event.id.to_hex();
    let mut merged = submit_with(
        client,
        event,
        "lifecycle command already accepted",
        json!({
            "commandId": command, "target": args.to, "providerAuthorityPubkey": provider,
        }),
    )
    .await
    .map_err(|error| submission_error(error, &command, &args.to, &event_id))?;
    let wait = await_command_receipt(client, &channel_id, &command, timeout, |events| {
        classify_resume_receipts(events, &channel_id, &command, provider, &target)
    })
    .await;
    let (report, result) = resume_report(wait, &command, timeout);
    if let (Some(output), Some(report)) = (merged.as_object_mut(), report.as_object()) {
        output.extend(report.clone());
    }
    println!("{merged}");
    result
}
