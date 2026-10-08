//! `bee sessions model` — the CLI form of the 44220 `thread.model.set` action
//! (SV-35, NIP-CSC): switch one execution's model (and effort) at its next
//! turn boundary.
//!
//! Three facts are reported separately and never collapsed into one, because
//! each is proved by a different signed record:
//!
//! - `accepted` — the relay stored the command.
//! - `deliveryStatus` — the provider's one terminal receipt for this
//!   `commandId`: `model_applied` (the adapter accepted the switch),
//!   `turn_refused` / `turn_dropped` (it did not, and the execution keeps its
//!   model), or `unconfirmed` when nothing answered inside the wait. A switch
//!   waits behind any running or queued turn, so `unconfirmed` usually means
//!   "still pending", never "failed" and never "applied".
//! - `model` — read **only** from the generation's next 44223 metadata, the
//!   provider's republish after the receipt. The receipt deliberately does not
//!   name a model; a command that echoed the requested selection here would be
//!   claiming the request took effect. With no such republish it is `null` and
//!   `modelStatus` is `unconfirmed`.
//!
//! Exit codes follow `bee sessions send`: once the relay accepted the command,
//! the command exits 0 and the JSON carries the provider's answer, refusal
//! included. Never 5 — that is a NIP-33 write conflict. A refusal or drop is
//! still stated plainly rather than left to `deliveryStatus` alone: `accepted`
//! is then `false`, `message` and stderr say "not switched: …", and
//! `relayAccepted: true` keeps the relay's own answer.

use std::collections::HashSet;

use serde_json::{json, Value};
use uuid::Uuid;

use beekeeper_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionTarget, CODING_SESSION_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, ReceiptError, ReceiptStatus,
};
use beekeeper_core::kind::{KIND_CODING_SESSION_LEASE, KIND_CODING_SESSION_TRANSCRIPT};
use beekeeper_sdk::builders::coding_session_turn_receipt_semantic_key;
use beekeeper_sdk::coding_session::CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION;
use beekeeper_sdk::kind::{KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA};

/// Default seconds `bee sessions model` waits for the provider's receipt.
///
/// Longer than `bee sessions send`'s wait: a switch applies only at a turn
/// boundary, so one sent during a running turn is answered when that turn
/// ends, and a short wait would report most switches `unconfirmed`.
pub const MODEL_WAIT_SECONDS: u64 = 30;

use super::crew::{build_executions, caller_umbrella, decode_leases, resolve_send_target};
use super::crew_cmds::{
    await_command_receipt, build_turn_command, submit_with, ReceiptWait, CREATE_WAIT_MAX_SECONDS,
};
use super::{decode_metadata, decode_receipts, decode_transcripts, fetch_channel_events};
use crate::client::BeekeeperClient;
use crate::error::CliError;

/// How long, after `model_applied`, the command keeps reading for the
/// generation's republished metadata. The provider publishes item → receipt
/// → 44223 from one loop step, so the metadata normally lands with the
/// receipt; this only covers relay and polling lag.
pub(super) const MODEL_METADATA_WAIT_SECONDS: u64 = 5;

/// The provider's one terminal answer to a `thread.model.set`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ModelAnswer {
    /// `model_applied` naming the exact target.
    Applied {
        receipt_event_id: String,
        created_at: i64,
    },
    /// `turn_refused` or `turn_dropped`: nothing was switched.
    NotApplied {
        receipt_event_id: String,
        status: ReceiptStatus,
        error: Option<ReceiptError>,
    },
}

impl ModelAnswer {
    fn status(&self) -> ReceiptStatus {
        match self {
            Self::Applied { .. } => ReceiptStatus::ModelApplied,
            Self::NotApplied { status, .. } => *status,
        }
    }
}

/// Every 44224 stage receipt in `events` that `provider` signed for exactly
/// `command_id` in `channel_id`: verified signature, the four tags the SDK's
/// turn-receipt builder writes (stage-keyed `csl-key`), and content that
/// decodes through `buzz-core` naming the same command.
fn verified_stage_receipts(
    events: &[Value],
    channel_id: &str,
    command_id: &str,
    provider: &str,
) -> Vec<(
    String,
    i64,
    beekeeper_core::coding_session_payload::LifecycleReceipt,
)> {
    let mut admitted = Vec::new();
    for raw in events {
        let Ok(event) = serde_json::from_value::<nostr::Event>(raw.clone()) else {
            continue;
        };
        if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_LIFECYCLE_RECEIPT
            || !event.pubkey.to_hex().eq_ignore_ascii_case(provider)
            || beekeeper_core::verify_event(&event).is_err()
        {
            continue;
        }
        let Ok(receipt) = decode_coding_session_lifecycle_receipt(&event.content) else {
            continue;
        };
        if receipt.command_id != command_id || !receipt.status.is_turn_stage() {
            continue;
        }
        let key = coding_session_turn_receipt_semantic_key(command_id, receipt.status);
        let expected = [
            ["h", channel_id],
            ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
            ["csl-command", command_id],
            ["csl-key", key.as_str()],
        ];
        if event.tags.len() != expected.len()
            || event.tags.iter().zip(expected).any(|(got, want)| {
                got.as_slice().len() != 2
                    || got.as_slice()[0] != want[0]
                    || got.as_slice()[1] != want[1]
            })
        {
            continue;
        }
        admitted.push((
            event.id.to_hex(),
            i64::try_from(event.created_at.as_secs()).unwrap_or(i64::MAX),
            receipt,
        ));
    }
    admitted
}

/// Select the provider's signed answer to one `thread.model.set`.
///
/// `model_applied` naming another target, or two answers that disagree, is a
/// contradiction reported as such — never resolved by picking the newer one.
/// Stages a switch never produces (`turn_queued`, `turn_started`, …) are
/// ignored rather than read as progress.
pub(super) fn classify_model_receipts(
    events: &[Value],
    channel_id: &str,
    command_id: &str,
    provider: &str,
    target: &CodingSessionTarget,
) -> Result<Option<ModelAnswer>, String> {
    let mut answer: Option<ModelAnswer> = None;
    for (receipt_event_id, created_at, receipt) in
        verified_stage_receipts(events, channel_id, command_id, provider)
    {
        let candidate = match receipt.status {
            ReceiptStatus::ModelApplied => {
                if receipt.session.as_ref() != Some(target) {
                    return Err(format!(
                        "provider receipt {receipt_event_id} for model switch {command_id} says \
                         model_applied but names a different target than {}",
                        coding_session_target_key(target)
                    ));
                }
                ModelAnswer::Applied {
                    receipt_event_id,
                    created_at,
                }
            }
            ReceiptStatus::TurnRefused | ReceiptStatus::TurnDropped => ModelAnswer::NotApplied {
                receipt_event_id,
                status: receipt.status,
                error: receipt.error,
            },
            _ => continue,
        };
        match &answer {
            Some(existing) if existing.status() != candidate.status() => {
                return Err(format!(
                    "provider published conflicting receipts for model switch {command_id}: {} \
                     and {}",
                    existing.status().as_str(),
                    candidate.status().as_str()
                ));
            }
            Some(_) => {}
            None => answer = Some(candidate),
        }
    }
    Ok(answer)
}

/// What the generation's metadata said after a `model_applied`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MetadataModel {
    /// One republished 44223 for the exact generation, signed by its provider.
    Confirmed {
        event_id: String,
        model: Option<String>,
    },
    /// Several republishes in the newest second disagree on `model`.
    Ambiguous(String),
    /// No qualifying republish.
    Absent,
}

/// Read the generation's metadata republished after the receipt.
///
/// A record qualifies when it is signed (and verifies) by `provider`, names
/// exactly `target` — the same generation the switch addressed — was not
/// already present before the command was published (`seen`), and is no
/// older than the receipt (`not_before`, Unix seconds). The newest second
/// wins; two records in that second that disagree are reported as ambiguous
/// rather than picked between.
pub(super) fn metadata_after_receipt(
    events: &[Value],
    provider: &str,
    target: &CodingSessionTarget,
    seen: &HashSet<String>,
    not_before: i64,
) -> MetadataModel {
    let (records, _) = decode_metadata(events);
    let mut qualifying: Vec<(i64, String, Option<String>)> = Vec::new();
    for record in records {
        if !record.signer.eq_ignore_ascii_case(provider)
            || record.metadata.session != *target
            || seen.contains(&record.id)
            || record.created_at < not_before
        {
            continue;
        }
        let verified = serde_json::from_value::<nostr::Event>(record.raw.clone())
            .ok()
            .is_some_and(|event| beekeeper_core::verify_event(&event).is_ok());
        if !verified {
            continue;
        }
        qualifying.push((record.created_at, record.id, record.metadata.model));
    }
    let Some(newest) = qualifying.iter().map(|(at, _, _)| *at).max() else {
        return MetadataModel::Absent;
    };
    let mut at_newest: Vec<&(i64, String, Option<String>)> = qualifying
        .iter()
        .filter(|(at, _, _)| *at == newest)
        .collect();
    at_newest.sort_by(|left, right| left.1.cmp(&right.1));
    let Some(first) = at_newest.first().copied() else {
        return MetadataModel::Absent;
    };
    if at_newest.iter().any(|(_, _, model)| *model != first.2) {
        return MetadataModel::Ambiguous(format!(
            "{} metadata records for {} published in the same second disagree on the model: {}",
            at_newest.len(),
            coding_session_target_key(target),
            at_newest
                .iter()
                .map(|(_, id, model)| format!("{id}={}", model.as_deref().unwrap_or("null")))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    MetadataModel::Confirmed {
        event_id: first.1.clone(),
        model: first.2.clone(),
    }
}

/// The report fields `bee sessions model` adds to the relay's write response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModelReport {
    /// The receipt's status word, `conflicted`, or `unconfirmed`.
    pub delivery_status: &'static str,
    /// One sentence naming what happened.
    pub delivery: String,
    /// The receipt that answered, or `Null`.
    pub receipt: Value,
    /// The model now in effect, from metadata only.
    pub model: Option<String>,
    /// `confirmed` (read from republished metadata), `unconfirmed`, or
    /// `not_switched` (the provider refused or dropped the switch).
    pub model_status: &'static str,
    /// The metadata event `model` was read from.
    pub metadata_event_id: Option<String>,
}

/// Fold the receipt wait and the metadata read into the printed report.
///
/// Pure, so the wording of each unpleasant answer is testable without a relay.
pub(super) fn fold_model_report(
    requested: &str,
    wait: Option<&ReceiptWait<ModelAnswer>>,
    metadata: &MetadataModel,
    timeout_secs: u64,
) -> ModelReport {
    let unconfirmed = |delivery: String| ModelReport {
        delivery_status: "unconfirmed",
        delivery,
        receipt: Value::Null,
        model: None,
        model_status: "unconfirmed",
        metadata_event_id: None,
    };
    let Some(wait) = wait else {
        return unconfirmed(
            "the relay stored the command; --no-wait skipped the receipt read, so whether the \
             switch was applied is unknown"
                .to_owned(),
        );
    };
    match wait {
        ReceiptWait::Unconfirmed { query_failed } => unconfirmed(format!(
            "the relay stored the command, but {} within {timeout_secs}s. A switch waits behind \
             any running or queued turn, so it may still apply at the next boundary; nothing was \
             re-sent",
            if *query_failed {
                "receipt queries failed or did not complete"
            } else {
                "no provider receipt arrived"
            }
        )),
        ReceiptWait::Conflicted(detail) => ModelReport {
            delivery_status: "conflicted",
            delivery: detail.clone(),
            receipt: Value::Null,
            model: None,
            model_status: "unconfirmed",
            metadata_event_id: None,
        },
        ReceiptWait::Answered(ModelAnswer::NotApplied {
            receipt_event_id,
            status,
            error,
        }) => {
            let reason = match error {
                Some(error) => format!("{} — {}", error.code, error.message),
                None => "no reason given".to_owned(),
            };
            let verb = if *status == ReceiptStatus::TurnDropped {
                "dropped"
            } else {
                "refused"
            };
            ModelReport {
                delivery_status: status.as_str(),
                delivery: format!(
                    "not switched: the provider {verb} the switch to {requested} ({reason}); \
                     the execution keeps its model"
                ),
                receipt: json!({
                    "eventId": receipt_event_id,
                    "status": status.as_str(),
                    "error": error,
                }),
                model: None,
                model_status: "not_switched",
                metadata_event_id: None,
            }
        }
        ReceiptWait::Answered(ModelAnswer::Applied {
            receipt_event_id, ..
        }) => {
            let receipt = json!({
                "eventId": receipt_event_id,
                "status": ReceiptStatus::ModelApplied.as_str(),
                "error": null,
            });
            let (model, model_status, metadata_event_id, delivery) = match metadata {
                MetadataModel::Confirmed { event_id, model } => {
                    let delivery = match model.as_deref() {
                        Some(model) if model == requested => {
                            format!("switched: the execution's metadata now reads {model}")
                        }
                        Some(model) => format!(
                            "switched, but not to exactly what was asked: asked for {requested}, \
                             the execution's metadata reads {model}"
                        ),
                        None => "the provider applied the switch, but its republished metadata \
                                 names no model"
                            .to_owned(),
                    };
                    (model.clone(), "confirmed", Some(event_id.clone()), delivery)
                }
                MetadataModel::Ambiguous(detail) => (
                    None,
                    "unconfirmed",
                    None,
                    format!(
                        "the provider applied the switch, but which model took effect is \
                         unknown: {detail}"
                    ),
                ),
                MetadataModel::Absent => (
                    None,
                    "unconfirmed",
                    None,
                    format!(
                        "the provider applied the switch, but no republished metadata for this \
                         generation arrived within {MODEL_METADATA_WAIT_SECONDS}s, so the model \
                         in effect is unconfirmed"
                    ),
                ),
            };
            ModelReport {
                delivery_status: ReceiptStatus::ModelApplied.as_str(),
                delivery,
                receipt,
                model,
                model_status,
                metadata_event_id,
            }
        }
    }
}

/// Poll the generation's metadata until a republish after the receipt
/// arrives or [`MODEL_METADATA_WAIT_SECONDS`] pass.
async fn await_metadata(
    client: &BeekeeperClient,
    channel_id: &str,
    provider: &str,
    target: &CodingSessionTarget,
    seen: &HashSet<String>,
    not_before: i64,
) -> MetadataModel {
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(MODEL_METADATA_WAIT_SECONDS);
    let filter = json!({
        "kinds": [KIND_CODING_SESSION_METADATA],
        "#h": [channel_id],
        "#cs-target": [coding_session_target_key(target)],
        "since": not_before,
    });
    loop {
        if let Ok(Ok(events)) =
            tokio::time::timeout_at(deadline, client.query_all(filter.clone())).await
        {
            let read = metadata_after_receipt(&events, provider, target, seen, not_before);
            if read != MetadataModel::Absent {
                return read;
            }
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return MetadataModel::Absent;
        }
        let next_poll = std::cmp::min(deadline, now + std::time::Duration::from_millis(500));
        tokio::time::sleep_until(next_poll).await;
    }
}

/// `bee sessions model` — publish one `thread.model.set` to the execution
/// `--to` names, wait for its receipt, then read the model now in effect from
/// the generation's republished metadata.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_model(
    client: &BeekeeperClient,
    channel_id: &str,
    to: &str,
    session_ref: Option<&str>,
    selection: &str,
    no_wait: bool,
    timeout_secs: Option<u64>,
) -> Result<(), CliError> {
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    if let Some(session_ref) = session_ref {
        beekeeper_core::coding_session_lifecycle_command::validate_session_ref(session_ref)
            .map_err(CliError::Usage)?;
    }
    let timeout_secs = match (no_wait, timeout_secs) {
        (true, Some(_)) => {
            return Err(CliError::Usage(
                "--timeout-secs cannot be combined with --no-wait".to_owned(),
            ))
        }
        (true, None) => 0,
        (false, timeout) => {
            let timeout = timeout.unwrap_or(MODEL_WAIT_SECONDS);
            if !(1..=CREATE_WAIT_MAX_SECONDS).contains(&timeout) {
                return Err(CliError::Usage(format!(
                    "--timeout-secs must be between 1 and {CREATE_WAIT_MAX_SECONDS} seconds"
                )));
            }
            timeout
        }
    };
    // Sent verbatim: the provider resolves the grammar against its catalog.
    let selection = selection.to_owned();
    // The signed event carries the canonical spelling; read and wait on it.
    let canonical_channel = channel.to_string();
    let channel_id = canonical_channel.as_str();

    let events = fetch_channel_events(
        client,
        channel_id,
        &[
            KIND_CODING_SESSION_METADATA,
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            KIND_CODING_SESSION_TRANSCRIPT,
        ],
    )
    .await?;
    let lease_events = client
        .query_all(json!({ "kinds": [KIND_CODING_SESSION_LEASE], "#h": [channel_id] }))
        .await
        .unwrap_or_default();
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let (transcripts, _) = decode_transcripts(&events);
    let leases = decode_leases(&lease_events);
    let executions = build_executions(
        &metadata,
        &receipts,
        &transcripts,
        &leases,
        chrono::Utc::now().timestamp(),
    );
    let caller = client.keys().public_key().to_hex();
    let umbrella = match session_ref {
        Some(session_ref) => Some(session_ref.to_owned()),
        None => caller_umbrella(&executions, &caller)?,
    };
    let execution = resolve_send_target(&executions, to, umbrella.as_deref())?;
    let target = execution.target.clone();
    let target_key = execution.target_key.clone();
    let provider = execution.signer.clone();
    // Metadata already on the relay cannot be the republish this switch causes.
    let seen: HashSet<String> = metadata.iter().map(|record| record.id.clone()).collect();

    let command_id = Uuid::new_v4().to_string();
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        target: target.clone(),
        action: CodingSessionAction::ThreadModelSet {
            selection: selection.clone(),
        },
    };
    // The core validator (non-blank, size cap, no control characters) runs
    // inside the builder, before anything is signed.
    let event = client.sign_event_unchecked(build_turn_command(channel, &payload)?)?;
    let mut merged = submit_with(
        client,
        event,
        "model command already accepted",
        json!({
            "commandId": command_id,
            "target": target_key,
            "seat": execution.seat_label(),
            "requested": selection,
        }),
    )
    .await?;

    let wait = if no_wait {
        None
    } else {
        Some(
            await_command_receipt(client, channel_id, &command_id, timeout_secs, |events| {
                classify_model_receipts(events, channel_id, &command_id, &provider, &target)
            })
            .await,
        )
    };
    let metadata_read = match &wait {
        Some(ReceiptWait::Answered(ModelAnswer::Applied { created_at, .. })) => {
            await_metadata(client, channel_id, &provider, &target, &seen, *created_at).await
        }
        _ => MetadataModel::Absent,
    };
    let report = fold_model_report(&selection, wait.as_ref(), &metadata_read, timeout_secs);
    if report.model_status == "not_switched" {
        eprintln!("bee sessions model: {}", report.delivery);
    }
    merge_model_report(&mut merged, report, !no_wait);
    println!("{merged}");
    Ok(())
}

/// Write `report` into the relay's write response.
///
/// A refusal or drop (`not_switched`) overwrites `accepted` with `false` and
/// `message` with the "not switched: …" sentence, keeping the relay's own
/// `accepted` as `relayAccepted`: the relay stored the command, the provider
/// refused it, and a reader who looks only at `accepted` must not read a
/// refusal as success.
pub(super) fn merge_model_report(merged: &mut Value, report: ModelReport, waited: bool) {
    let Some(object) = merged.as_object_mut() else {
        return;
    };
    if report.model_status == "not_switched" {
        let relay_accepted = object.get("accepted").cloned().unwrap_or(Value::Null);
        object.insert("relayAccepted".into(), relay_accepted);
        object.insert("accepted".into(), json!(false));
        object.insert("message".into(), json!(report.delivery));
    }
    object.insert("waited".into(), json!(waited));
    object.insert("deliveryStatus".into(), json!(report.delivery_status));
    object.insert("delivery".into(), json!(report.delivery));
    object.insert("receipt".into(), report.receipt);
    object.insert("model".into(), json!(report.model));
    object.insert("modelStatus".into(), json!(report.model_status));
    object.insert("metadataEventId".into(), json!(report.metadata_event_id));
}
