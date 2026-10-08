//! `bee sessions rewind` — the CLI form of the kind 44221 `session.rewind`
//! lifecycle action (SV-29, **Edit from here**).
//!
//! A rewind detaches the execution's current generation N and opens N+1 whose
//! context is cut at the start of one recorded turn: the kind 44231 `turn`
//! checkpoint `--checkpoint` names is the first turn forgotten. With
//! `--files restore` the provider also returns the working tree to that
//! checkpoint's `baseTree`; with `--files keep` the files stay as they are.
//!
//! This is destructive, so the command never reports success on its own
//! authority. It publishes once, waits for the provider's signed 44224
//! receipt for that `commandId`, and reports what the receipt — and only the
//! receipt — says:
//!
//! - `outcome: rewound` — a `resumed` (or `resumed_without_context`) receipt
//!   naming generation N+1 and carrying the `rewind{}` facts. `restarted` is
//!   `restarted`, or `restarted_without_context` when N+1 opened with no
//!   memory of the session before the cut.
//! - `outcome: not_restarted` — `failed` / `REWIND_NOT_RESTARTED`: the
//!   checks passed, `rewind.files` says what became of the files
//!   (`kept`, `restored`, `restore_failed`), and N+1 never opened.
//! - `outcome: refused` — any other signed `failed`: the provider refused
//!   before touching anything, so `files` is `untouched`.
//! - `outcome: unconfirmed` — no receipt inside the wait. Nothing is known;
//!   no second rewind is sent.
//!
//! Exit codes follow `bee sessions stop --wait`: 0 rewound; 1 a refusal before
//! publishing, a signed refusal or `not_restarted`, or receipts that
//! contradict each other; 5 unconfirmed.

use serde_json::{json, Value};
use uuid::Uuid;

use beekeeper_core::coding_session_checkpoint::CodingSessionCheckpointReason;
use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_lifecycle_command::{
    validate_event_id_hex, CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    RewindFiles, CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use beekeeper_core::coding_session_payload::{ReceiptError, ReceiptStatus, REWIND_NOT_RESTARTED};
use beekeeper_core::coding_session_rewind::{ReceiptRewind, RewindFilesOutcome};
use beekeeper_core::kind::{
    KIND_CODING_SESSION_CHECKPOINT, KIND_CODING_SESSION_LEASE, KIND_CODING_SESSION_TRANSCRIPT,
};
use beekeeper_sdk::builders::build_coding_session_lifecycle_command;
use beekeeper_sdk::kind::{KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA};

use super::checkpoints::{admit_checkpoints, decode_checkpoint_events, transcript_signers};
use super::crew::{
    build_executions, caller_umbrella, decode_leases, resolve_send_target, stopped_status_word,
    CrewExecution,
};
use super::crew_cmds::{
    await_command_receipt, submit_with, verified_command_receipts, ReceiptWait,
    CREATE_WAIT_DEFAULT_SECONDS, CREATE_WAIT_MAX_SECONDS,
};
use super::{decode_metadata, decode_receipts, decode_transcripts, fetch_channel_events};
use crate::client::BeekeeperClient;
use crate::error::CliError;
use crate::validate::sdk_err;

/// Parse `--files`.
pub(super) fn parse_files(value: &str) -> Result<RewindFiles, CliError> {
    match value {
        "keep" => Ok(RewindFiles::Keep),
        "restore" => Ok(RewindFiles::Restore),
        other => Err(CliError::Usage(format!(
            "--files must be `keep` or `restore`, not '{other}'"
        ))),
    }
}

fn files_word(files: RewindFiles) -> &'static str {
    match files {
        RewindFiles::Keep => "keep",
        RewindFiles::Restore => "restore",
    }
}

fn outcome_word(files: RewindFilesOutcome) -> &'static str {
    match files {
        RewindFilesOutcome::Kept => "kept",
        RewindFilesOutcome::Restored => "restored",
        RewindFilesOutcome::RestoreFailed => "restore_failed",
    }
}

/// Refuse, before anything is signed, a rewind the signed record already
/// shows cannot work: a stopped execution, or a checkpoint that is not on
/// the relay, does not verify, is not this execution's, is not a restorable
/// `turn` checkpoint, or (with `restore`) carries no trees to restore.
///
/// Everything else — the turn being open, a sibling busy in the same tree,
/// lineage, whether the git objects exist — is the provider's to judge, and
/// its signed refusal is reported as it arrives.
pub(super) fn plan_rewind(
    execution: &CrewExecution,
    events: &[Value],
    checkpoint: &str,
    files: RewindFiles,
) -> Result<(), CliError> {
    if execution.status == stopped_status_word() {
        return Err(CliError::Refused(format!(
            "{} is stopped and cannot be rewound; nothing was published",
            execution.target_key
        )));
    }
    let (decoded, mut refused) = decode_checkpoint_events(events);
    let (admitted, signer_refused) = admit_checkpoints(decoded, &transcript_signers(events));
    refused.extend(signer_refused);
    if let Some(refusal) = refused
        .iter()
        .find(|refusal| refusal.event_id.as_deref() == Some(checkpoint))
    {
        return Err(CliError::Refused(format!(
            "checkpoint {checkpoint} is not usable: {}; nothing was published",
            refusal.reason
        )));
    }
    let Some(record) = admitted.iter().find(|record| record.event_id == checkpoint) else {
        return Err(CliError::NotFound(format!(
            "no kind 44231 checkpoint {checkpoint} in this channel; nothing was published. \
             `bee sessions checkpoints --channel <uuid>` lists them"
        )));
    };
    let target = &execution.target;
    let session = &record.payload.session;
    if !record.signer.eq_ignore_ascii_case(&execution.signer)
        || session.session_id != target.session_id
        || session.driver != target.driver
        || session.instance_id != target.instance_id
        || session.generation > target.generation
    {
        return Err(CliError::Refused(format!(
            "checkpoint {checkpoint} belongs to {} (signed by {}), not to {}; nothing was \
             published",
            record.target_key, record.signer, execution.target_key
        )));
    }
    if record.payload.reason != CodingSessionCheckpointReason::Turn {
        return Err(CliError::Refused(format!(
            "checkpoint {checkpoint} is a {} capture, not a turn; only a turn can be rewound \
             to; nothing was published",
            record.payload.reason.as_str()
        )));
    }
    if !record.payload.restorable {
        return Err(CliError::Refused(format!(
            "checkpoint {checkpoint} says restorable: false — the provider that captured it \
             cannot rewind to it; nothing was published"
        )));
    }
    if files == RewindFiles::Restore
        && record
            .payload
            .git
            .as_ref()
            .and_then(|git| git.base_tree.as_ref())
            .is_none()
    {
        return Err(CliError::Refused(format!(
            "checkpoint {checkpoint} recorded no tree from before its turn, so --files restore \
             has nothing to restore; nothing was published (--files keep rewinds the \
             conversation only)"
        )));
    }
    Ok(())
}

/// What the provider answered to one rewind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RewindAnswer {
    /// N+1 opened: `resumed` or `resumed_without_context`.
    Restarted {
        receipt_event_id: String,
        status: ReceiptStatus,
        session: CodingSessionTarget,
        rewind: ReceiptRewind,
    },
    /// A signed `failed`, with the `rewind` facts when the checks passed.
    Failed {
        receipt_event_id: String,
        error: ReceiptError,
        rewind: Option<ReceiptRewind>,
    },
}

/// Select the provider's signed answer to one rewind.
///
/// Receipts pass [`verified_command_receipts`] (signature, exact tags, strict
/// core decode). A success that names any generation but N+1 of this
/// execution, carries no `rewind`, or names another checkpoint, and two
/// answers that disagree, are contradictions reported as such.
pub(super) fn classify_rewind_receipts(
    events: &[Value],
    channel_id: &str,
    command_id: &str,
    provider: &str,
    target: &CodingSessionTarget,
    checkpoint: &str,
) -> Result<Option<RewindAnswer>, String> {
    let mut answer: Option<RewindAnswer> = None;
    for (receipt_event_id, receipt) in
        verified_command_receipts(events, channel_id, command_id, provider)
    {
        if let Some(rewind) = &receipt.rewind {
            if rewind.checkpoint != checkpoint {
                return Err(format!(
                    "provider receipt {receipt_event_id} for rewind {command_id} names checkpoint \
                     {}, not {checkpoint}",
                    rewind.checkpoint
                ));
            }
        }
        let candidate = match receipt.status {
            ReceiptStatus::Resumed | ReceiptStatus::ResumedWithoutContext => {
                let next = CodingSessionTarget {
                    generation: target.generation.saturating_add(1),
                    ..target.clone()
                };
                if receipt.session.as_ref() != Some(&next) {
                    return Err(format!(
                        "provider receipt {receipt_event_id} for rewind {command_id} says {} but \
                         does not name {}, the generation after {}",
                        receipt.status.as_str(),
                        coding_session_target_key(&next),
                        coding_session_target_key(target)
                    ));
                }
                let Some(rewind) = receipt.rewind else {
                    return Err(format!(
                        "provider receipt {receipt_event_id} for rewind {command_id} says {} but \
                         carries no rewind facts, so what was cut and what became of the files \
                         is unknown",
                        receipt.status.as_str()
                    ));
                };
                RewindAnswer::Restarted {
                    receipt_event_id,
                    status: receipt.status,
                    session: next,
                    rewind,
                }
            }
            ReceiptStatus::Failed => {
                let Some(error) = receipt.error else {
                    continue;
                };
                RewindAnswer::Failed {
                    receipt_event_id,
                    error,
                    rewind: receipt.rewind,
                }
            }
            _ => continue,
        };
        let same = |left: &RewindAnswer, right: &RewindAnswer| match (left, right) {
            (
                RewindAnswer::Restarted {
                    status: a,
                    rewind: x,
                    ..
                },
                RewindAnswer::Restarted {
                    status: b,
                    rewind: y,
                    ..
                },
            ) => a == b && x == y,
            (
                RewindAnswer::Failed {
                    error: a,
                    rewind: x,
                    ..
                },
                RewindAnswer::Failed {
                    error: b,
                    rewind: y,
                    ..
                },
            ) => a == b && x == y,
            _ => false,
        };
        match &answer {
            Some(existing) if !same(existing, &candidate) => {
                return Err(format!(
                    "provider published conflicting lifecycle receipts for rewind {command_id}"
                ));
            }
            Some(_) => {}
            None => answer = Some(candidate),
        }
    }
    Ok(answer)
}

/// The fields `bee sessions rewind` adds to the relay's write response.
#[derive(Debug)]
pub(super) struct RewindReport {
    /// `rewound`, `not_restarted`, `refused`, `conflicted` or `unconfirmed`.
    pub outcome: &'static str,
    /// `restarted`, `restarted_without_context`, `not_restarted`, or `null`
    /// when unknown.
    pub restarted: Option<&'static str>,
    /// `kept`, `restored`, `restore_failed`, `untouched` (refused before any
    /// write), or `null` when unknown.
    pub files: Option<&'static str>,
    /// The generation N+1 opened, when it did.
    pub session: Option<String>,
    /// One sentence naming what happened.
    pub summary: String,
    /// The answering receipt, or `Null`.
    pub receipt: Value,
    /// The receipt's `rewind{}` exactly as signed, or `Null`.
    pub rewind: Value,
    /// Exit status: `None` for success.
    pub error: Option<CliError>,
}

/// Fold the receipt wait into the printed report. Pure, so each unpleasant
/// answer's wording is testable without a relay.
pub(super) fn fold_rewind_report(
    wait: &ReceiptWait<RewindAnswer>,
    target_key: &str,
    command_id: &str,
    timeout_secs: u64,
) -> RewindReport {
    let rewind_json = |rewind: &ReceiptRewind| serde_json::to_value(rewind).unwrap_or(Value::Null);
    match wait {
        ReceiptWait::Answered(RewindAnswer::Restarted {
            receipt_event_id,
            status,
            session,
            rewind,
        }) => {
            let without_context = *status == ReceiptStatus::ResumedWithoutContext;
            let session_key = coding_session_target_key(session);
            let files = outcome_word(rewind.files);
            let summary = format!(
                "rewound: {target_key} was detached (not truncated) and {session_key} opened \
                 with the record cut after seq {} of generation {}; files {files}{}",
                rewind.cut_after_seq,
                rewind.cut_generation,
                if without_context {
                    ". The new generation opened without the session's earlier context: it \
                     remembers nothing from before the cut"
                } else {
                    ""
                }
            );
            RewindReport {
                outcome: "rewound",
                restarted: Some(if without_context {
                    "restarted_without_context"
                } else {
                    "restarted"
                }),
                files: Some(files),
                session: Some(session_key),
                summary,
                receipt: json!({ "eventId": receipt_event_id, "status": status.as_str(), "error": null }),
                rewind: rewind_json(rewind),
                error: None,
            }
        }
        ReceiptWait::Answered(RewindAnswer::Failed {
            receipt_event_id,
            error,
            rewind,
        }) => {
            let receipt =
                json!({ "eventId": receipt_event_id, "status": "failed", "error": error });
            match rewind {
                Some(rewind) => {
                    let files = outcome_word(rewind.files);
                    let summary = format!(
                        "not restarted: the rewind of {target_key} passed its checks but the new \
                         generation did not open; files {files}. {} ({})",
                        error.message, error.code
                    );
                    RewindReport {
                        outcome: "not_restarted",
                        restarted: Some("not_restarted"),
                        files: Some(files),
                        session: None,
                        summary: summary.clone(),
                        receipt,
                        rewind: rewind_json(rewind),
                        error: Some(CliError::Refused(summary)),
                    }
                }
                None => {
                    // Without rewind facts a REWIND_NOT_RESTARTED says nothing
                    // about the files; any other refusal came before a write.
                    let files = (error.code != REWIND_NOT_RESTARTED).then_some("untouched");
                    let summary = format!(
                        "refused: the provider refused the rewind of {target_key}: {} ({}){}",
                        error.message,
                        error.code,
                        if files.is_some() {
                            "; nothing was changed"
                        } else {
                            "; the receipt carries no rewind facts, so what became of the files \
                             is unknown"
                        }
                    );
                    RewindReport {
                        outcome: "refused",
                        restarted: Some("not_restarted"),
                        files,
                        session: None,
                        summary: summary.clone(),
                        receipt,
                        rewind: Value::Null,
                        error: Some(CliError::Refused(summary)),
                    }
                }
            }
        }
        ReceiptWait::Conflicted(detail) => RewindReport {
            outcome: "conflicted",
            restarted: None,
            files: None,
            session: None,
            summary: detail.clone(),
            receipt: Value::Null,
            rewind: Value::Null,
            error: Some(CliError::Refused(detail.clone())),
        },
        ReceiptWait::Unconfirmed { query_failed } => {
            let summary = format!(
                "relay accepted rewind {command_id} for {target_key}, but {} within \
                 {timeout_secs}s; whether the execution was rewound, and what became of the \
                 files, is unknown. No second rewind was sent — query receipts for this \
                 commandId before acting",
                if *query_failed {
                    "receipt queries failed or did not complete"
                } else {
                    "no signed receipt arrived"
                }
            );
            RewindReport {
                outcome: "unconfirmed",
                restarted: None,
                files: None,
                session: None,
                summary: summary.clone(),
                receipt: Value::Null,
                rewind: Value::Null,
                error: Some(CliError::Unconfirmed(summary)),
            }
        }
    }
}

/// The exact 44221 payload one rewind publishes.
pub(super) fn rewind_payload(
    command_id: &str,
    target: &CodingSessionTarget,
    provider: &str,
    checkpoint: &str,
    files: RewindFiles,
) -> CodingSessionLifecycleCommandPayload {
    CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        action: CodingSessionLifecycleAction::SessionRewind {
            session: target.clone(),
            provider_authority_pubkey: provider.to_owned(),
            checkpoint: checkpoint.to_owned(),
            files,
        },
    }
}

/// `bee sessions rewind`: publish one `session.rewind` for the current
/// generation `--to` names, wait for the provider's signed receipt, and
/// report what it says.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_rewind(
    client: &BeekeeperClient,
    channel_id: &str,
    to: &str,
    session_ref: Option<&str>,
    checkpoint: &str,
    files: &str,
    timeout_secs: Option<u64>,
) -> Result<(), CliError> {
    let channel = Uuid::parse_str(channel_id)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    if let Some(session_ref) = session_ref {
        beekeeper_core::coding_session_lifecycle_command::validate_session_ref(session_ref)
            .map_err(CliError::Usage)?;
    }
    validate_event_id_hex("--checkpoint", checkpoint).map_err(CliError::Usage)?;
    let files = parse_files(files)?;
    let timeout_secs = timeout_secs.unwrap_or(CREATE_WAIT_DEFAULT_SECONDS);
    if !(1..=CREATE_WAIT_MAX_SECONDS).contains(&timeout_secs) {
        return Err(CliError::Usage(format!(
            "--timeout-secs must be between 1 and {CREATE_WAIT_MAX_SECONDS} seconds"
        )));
    }
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
            KIND_CODING_SESSION_CHECKPOINT,
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
    let executions = build_executions(
        &metadata,
        &receipts,
        &transcripts,
        &decode_leases(&lease_events),
        chrono::Utc::now().timestamp(),
    );
    let caller = client.keys().public_key().to_hex();
    let umbrella = match session_ref {
        Some(session_ref) => Some(session_ref.to_owned()),
        None => caller_umbrella(&executions, &caller)?,
    };
    let execution = resolve_send_target(&executions, to, umbrella.as_deref())?;
    plan_rewind(execution, &events, checkpoint, files)?;
    let target = execution.target.clone();
    let target_key = execution.target_key.clone();
    let provider = execution.signer.clone();

    let command_id = format!("rewind-{}", Uuid::new_v4());
    let payload = rewind_payload(&command_id, &target, &provider, checkpoint, files);
    let builder = build_coding_session_lifecycle_command(channel, &payload).map_err(sdk_err)?;
    let event = client.sign_event_unchecked(builder)?;
    let mut merged = submit_with(
        client,
        event,
        "lifecycle command already accepted",
        json!({
            "commandId": command_id,
            "target": target_key,
            "checkpoint": checkpoint,
            "filesRequested": files_word(files),
        }),
    )
    .await?;

    let wait = await_command_receipt(client, channel_id, &command_id, timeout_secs, |events| {
        classify_rewind_receipts(
            events,
            channel_id,
            &command_id,
            &provider,
            &target,
            checkpoint,
        )
    })
    .await;
    let report = fold_rewind_report(&wait, &target_key, &command_id, timeout_secs);
    if let Some(object) = merged.as_object_mut() {
        object.insert("waited".into(), json!(true));
        object.insert("outcome".into(), json!(report.outcome));
        object.insert("restarted".into(), json!(report.restarted));
        object.insert("files".into(), json!(report.files));
        object.insert("session".into(), json!(report.session));
        object.insert("summary".into(), json!(report.summary));
        object.insert("receipt".into(), report.receipt);
        object.insert("rewind".into(), report.rewind);
    }
    println!("{merged}");
    match report.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
