//! `bee sessions handover continue` — pick another participant's work up.
//!
//! Five steps, in the order `docs/HANDOVER_IMPL.md` §4 sets them out, and two
//! outcomes that are never conflated:
//!
//! * **`native-resume`** — the original execution is still reachable, so the
//!   checkpoint's next action is sent to it as an ordinary turn and the
//!   original agent's own context carries on.
//! * **`reconstructed`** — it is not, so the artifacts are fetched into a
//!   checkout and a **new** execution joins the same umbrella from the
//!   checkpoint's words. Its native context did not travel; the label and the
//!   brief both say so.
//!
//! # Reachability is a live lease, not a guess
//!
//! "Reachable" means a live kind-24223 lease answers for the candidate
//! execution's **current generation**. A `quiet` execution — one whose
//! provider stopped renewing — is exactly the absent participant this feature
//! exists for, so it is not reachable, and `--native` against it is refused by
//! name rather than sent into a machine that cannot answer.
//!
//! # The base comes first
//!
//! A patch artifact is a diff against a **commit**. Artifact order in the
//! record is the author's, so the reconstruction checks the wip ref out
//! before applying anything and, when that checkout fails, applies nothing on
//! top of it — a three-way merge onto whatever the directory happened to hold
//! would report "recovered" over a tree that is not this work (REVIEW N9).
//!
//! # Idempotent rerun
//!
//! Step 0 is a rerun check: if this caller already holds the claim in force
//! and an authorized continuation already names it, this prints that
//! continuation and exits 0. A second run after an interrupted first one
//! therefore never creates a second execution (§6 case 9).

use std::time::Duration;

use buzz_core::coding_session_authority_claim::ClaimState;
use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionDelivery, CodingSessionTarget, CODING_SESSION_COMMAND_SCHEMA,
};
use buzz_core::coding_session_handover::{
    CodingSessionHandoverBody, CodingSessionHandoverCheckpoint, CodingSessionHandoverContinuation,
    CodingSessionHandoverMode, MAX_HANDOVER_MISSING, MAX_HANDOVER_RECOVERED,
};
use buzz_core::coding_session_payload::{decode_coding_session_lifecycle_receipt, ReceiptStatus};
use buzz_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;
use nostr::Event;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::HandoverContinueArgs;

use super::crew::short_pubkey;
use super::handover::{
    build_handover_event, classify_own_write, fetch_executions, is_reachable, load_handover_state,
    ExecutionRow, HandoverState,
};
use super::handover_claim::{
    bounded_wait, claim_session, refuse_retired, refuse_unverifiable_genesis, resolve_body,
};
use super::handover_reconstruct::{
    create_execution, recover_checkout, resolve_projects_file, verify_execution_workdir,
};
use super::handover_render::{VerificationNotes, WHOLE_SESSION_DISCLOSURE};

/// How often a receipt wait re-asks the relay.
const RECEIPT_POLL: Duration = Duration::from_millis(500);

/// What the run decided to do, before it did it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContinuationPlan {
    /// Steer the original execution on its own provider.
    Native,
    /// Build a new execution from the checkpoint's artifacts.
    Reconstruct,
}

/// Decide native or reconstruct, and say why in one sentence.
///
/// Pure so the decision is testable without a relay. The default is the one
/// §4 sets: native when the original is reachable **and** the caller already
/// holds a grant on the umbrella — that existing grant is the resource consent
/// this increment reuses, which is a stated limit (§9) — else reconstruct.
pub(super) fn decide_plan(
    force_native: bool,
    force_reconstruct: bool,
    reachable: bool,
    caller_has_grant: bool,
) -> Result<(ContinuationPlan, String), CliError> {
    match (force_native, force_reconstruct) {
        (true, true) => Err(CliError::Usage(
            "--native and --reconstruct name opposite outcomes; give at most one".into(),
        )),
        (true, false) => {
            if reachable {
                Ok((
                    ContinuationPlan::Native,
                    "--native was given and a live 24223 lease answers for the original \
                     execution's current generation"
                        .to_owned(),
                ))
            } else {
                Err(CliError::Usage(
                    "--native requires the original execution to be reachable, and no live \
                     24223 lease answers for its current generation. Its provider is not \
                     running, so a turn sent there would go nowhere. Use --reconstruct."
                        .into(),
                ))
            }
        }
        (false, true) => Ok((
            ContinuationPlan::Reconstruct,
            "--reconstruct was given, so reachability was not consulted".to_owned(),
        )),
        (false, false) => {
            if reachable && caller_has_grant {
                Ok((
                    ContinuationPlan::Native,
                    "a live 24223 lease answers for the original execution and this caller \
                     already holds a grant on the umbrella, so the work continues in its own \
                     context"
                        .to_owned(),
                ))
            } else if reachable {
                Ok((
                    ContinuationPlan::Reconstruct,
                    "the original execution is reachable, but this caller holds no grant on the \
                     umbrella — the existing grant is the resource consent a native resume \
                     reuses — so the work is reconstructed instead"
                        .to_owned(),
                ))
            } else {
                Ok((
                    ContinuationPlan::Reconstruct,
                    "no live 24223 lease answers for the original execution's current \
                     generation, so its provider cannot be steered and the work is reconstructed"
                        .to_owned(),
                ))
            }
        }
    }
}

/// `bee sessions handover continue`.
#[allow(clippy::too_many_lines)]
pub(super) async fn cmd_continue(
    client: &BuzzClient,
    args: HandoverContinueArgs,
) -> Result<(), CliError> {
    let wait_secs = bounded_wait(args.wait_secs)?;
    let state = load_handover_state(
        client,
        &args.channel,
        &args.session_ref,
        args.genesis.as_deref(),
    )
    .await?;
    let mut notes = VerificationNotes::default();

    // ── 1. standing ──────────────────────────────────────────────────────
    refuse_retired(&state)?;
    refuse_unverifiable_genesis(&state)?;
    let caller = client.keys().public_key().to_hex();
    if !state.has_standing(&caller) {
        return Err(CliError::Usage(format!(
            "{} is neither the founder of this session nor a live operator on it, so it cannot \
             continue this work. §1 reserves a claim for those two; ask the founder for \
             `grant-operator`.",
            short_pubkey(&caller)
        )));
    }
    if let Some(existing) = already_continued(&state, &caller) {
        println!("{existing}");
        return Ok(());
    }

    let checkpoint = state.latest_authorized_checkpoint().cloned();
    let checkpoint = match (checkpoint, args.allow_no_checkpoint) {
        (Some(entry), _) => Some(entry),
        (None, true) => {
            notes.not_verified(
                "no authorized checkpoint exists, and --allow-no-checkpoint was given: this \
                 continuation is seeded from nothing but the session's coordinates, so the new \
                 execution is told only that it is picking up work whose statement was never \
                 written"
                    .to_owned(),
            );
            None
        }
        (None, false) => {
            return Err(CliError::NotFound(format!(
                "this session has no authorized checkpoint to continue from ({} record(s) \
                 listed, all excluded — run `bee sessions handover status` to see why). Write \
                 one with `bee sessions handover checkpoint`, or pass --allow-no-checkpoint to \
                 continue without one and have that labelled on the record.",
                state.fold.checkpoints.len()
            )))
        }
    };
    if let Some(entry) = &checkpoint {
        notes.verified(format!(
            "checkpoint {} by {} held standing when it was written, so it is what this \
             continuation is seeded from",
            entry.event_id,
            short_pubkey(&entry.author)
        ));
    }

    // ── 2. reachability ──────────────────────────────────────────────────
    let executions = fetch_executions(client, &state.channel, &state.session_ref).await?;
    let candidate = select_candidate(
        &executions,
        checkpoint.as_ref().map(|entry| entry.author.as_str()),
    );
    let reachable = candidate.is_some_and(is_reachable);
    match candidate {
        Some(row) => notes.verified(format!(
            "the candidate original execution is {} and its liveness reads {} (from the relay's \
             kind-24223 lease snapshot)",
            row.execution.target_key,
            row.execution.liveness.render()
        )),
        None => notes.not_verified(
            "no execution of this umbrella was found on the wire, so there is nothing to resume \
             natively"
                .to_owned(),
        ),
    }
    let caller_has_grant = state
        .authority
        .as_ref()
        .is_some_and(|authority| authority.grants.iter().any(|(key, _)| key == &caller))
        || caller == state.founder;
    let (plan, why) = decide_plan(args.native, args.reconstruct, reachable, caller_has_grant)?;
    notes.verified(format!("chose {} because {why}", plan_word(plan)));

    // Resolved **before** the claim. A claim moves the fence for everybody, and
    // taking a session over only to find the new execution cannot be pointed
    // at the recovered work is a worse place to stop than not starting. Only a
    // reconstruction into a checkout needs it: with no --cwd nothing was
    // recovered locally, and the continuation says so rather than binding a
    // directory that holds nothing.
    let projects_file = match (plan, args.cwd.as_deref()) {
        (ContinuationPlan::Reconstruct, Some(cwd)) => {
            Some(resolve_projects_file(args.projects_file.as_deref(), cwd)?)
        }
        _ => None,
    };

    // ── 3. claim ─────────────────────────────────────────────────────────
    let body_pubkey = match plan {
        ContinuationPlan::Native => match args.body.as_deref() {
            Some(body) => resolve_body(client, &state, Some(body), false).await?,
            None => candidate
                .map(|row| row.execution.signer.clone())
                .ok_or_else(|| {
                    CliError::Usage(
                        "a native resume must claim the original execution's own provider as the \
                         body, and no execution was found to read it from; pass --body"
                            .into(),
                    )
                })?,
        },
        ContinuationPlan::Reconstruct => {
            resolve_body(client, &state, args.body.as_deref(), args.body_self).await?
        }
    };
    let claim = match state.claim() {
        ClaimState::Active(existing)
            if existing.claimant == caller && existing.body_pubkey == body_pubkey =>
        {
            notes.verified(format!(
                "this caller already holds the claim in force ({}) on the same body, so no \
                 second takeover was published",
                existing.accepted_event_id
            ));
            super::handover_claim::ClaimOutcome {
                accepted_event_id: existing.accepted_event_id.clone(),
                seq: existing.seq,
                claimant: existing.claimant.clone(),
                body_pubkey: existing.body_pubkey.clone(),
                receipt_event_id: None,
            }
        }
        _ => claim_session(client, &state, &body_pubkey, wait_secs, &mut notes).await?,
    };

    // ── 4/5. act ─────────────────────────────────────────────────────────
    let (mode, target, recovered, missing) = match plan {
        ContinuationPlan::Native => {
            let row = candidate.ok_or_else(|| {
                CliError::Other("a native resume lost its candidate execution".to_owned())
            })?;
            let text = next_action_of(checkpoint.as_ref().map(|entry| &entry.body));
            let outcome = send_native_turn(
                client,
                &state.channel,
                &row.execution.target,
                &text,
                wait_secs,
            )
            .await?;
            match &outcome.stage {
                Some(stage) if stage.0 == ReceiptStatus::TurnStarted => notes.verified(format!(
                    "the original execution answered turn_started for command {}",
                    outcome.command_id
                )),
                Some(stage) => notes.not_verified(format!(
                    "the original execution answered {} for command {}{}: the turn did not start",
                    receipt_word(stage.0),
                    outcome.command_id,
                    stage
                        .1
                        .as_deref()
                        .map(|code| format!(" ({code})"))
                        .unwrap_or_default()
                )),
                None => notes.not_verified(format!(
                    "no turn receipt answered command {} within {wait_secs}s; the relay stored \
                     the command, and whether the provider ran it is unknown",
                    outcome.command_id
                )),
            }
            (
                CodingSessionHandoverMode::NativeResume,
                row.execution.target.clone(),
                vec![format!(
                    "native resume on {} (the original provider's own context)",
                    row.execution.target_key
                )],
                outcome.missing,
            )
        }
        ContinuationPlan::Reconstruct => {
            let body = checkpoint.as_ref().map(|entry| &entry.body);
            // Taken before the create so metadata published in the same second
            // cannot fall outside the proof's window.
            let created_since = chrono::Utc::now().timestamp() - 1;
            let recovery = recover_checkout(
                client,
                args.cwd.as_deref(),
                &state.session_ref.chars().take(8).collect::<String>(),
                body,
                args.remote.as_deref(),
                args.allow_no_artifact,
                &mut notes,
            )
            .await?;
            let created = create_execution(
                client,
                &state,
                &body_pubkey,
                args.provider_instance.as_deref(),
                projects_file.as_deref(),
                args.cwd.as_deref(),
                body,
                checkpoint.as_ref().map(|entry| entry.event_id.as_str()),
                checkpoint
                    .as_ref()
                    .map_or(state.founder.as_str(), |entry| entry.author.as_str()),
                wait_secs,
                &mut notes,
            )
            .await?;

            // The create's receipt says an execution exists; it does not say
            // **where**. The provider's own first metadata carries the
            // worktree it probed, and that is the only evidence this command
            // can offer that the binding took. The patch is applied
            // uncommitted, so HEAD is still the checkpoint's headSha.
            let mut missing = recovery.missing;
            if args.cwd.is_some() {
                let expected_branch = format!(
                    "handover/{}",
                    state.session_ref.chars().take(8).collect::<String>()
                );
                let expected_head = body.and_then(|body| body.revision.head_sha.as_deref());
                if let Some(line) = verify_execution_workdir(
                    client,
                    &state.channel,
                    &created,
                    &expected_branch,
                    expected_head,
                    created_since,
                    wait_secs,
                )
                .await
                {
                    notes.not_verified(line.clone());
                    missing.push(line);
                } else {
                    notes.verified(format!(
                        "the new execution's first metadata reports branch {expected_branch} at \
                         the checkpoint's head, so it is running in the recovered checkout"
                    ));
                }
            }
            (
                CodingSessionHandoverMode::Reconstructed,
                created,
                recovery.recovered,
                missing,
            )
        }
    };

    let continuation = CodingSessionHandoverContinuation {
        claim_ref: claim.accepted_event_id.clone(),
        mode,
        checkpoint_ref: checkpoint.as_ref().map(|entry| entry.event_id.clone()),
        target,
        recovered: bound_lines(recovered, MAX_HANDOVER_RECOVERED),
        missing: bound_lines(missing, MAX_HANDOVER_MISSING),
        note: args.note.clone(),
    };
    let event = build_handover_event(
        client,
        &state.channel,
        &state.session_ref,
        &state.genesis_ref,
        CodingSessionHandoverBody::Continuation(continuation.clone()),
    )?;
    let event_id = event.id.to_hex();
    let raw = client.submit_event(event).await?;
    // Same rule as the checkpoint: a rerun that produces byte-identical
    // content produces the same id, and the relay already holding it is the
    // idempotency this command promises rather than a failure.
    classify_own_write(&raw)?;
    notes.verified(format!("published continuation {event_id}"));

    report(&event_id, &continuation, &notes, args.json);
    Ok(())
}

/// The word a plan reads as on stdout.
const fn plan_word(plan: ContinuationPlan) -> &'static str {
    match plan {
        ContinuationPlan::Native => "native-resume",
        ContinuationPlan::Reconstruct => "reconstructed",
    }
}

/// The word a receipt status reads as in a sentence.
fn receipt_word(status: ReceiptStatus) -> String {
    format!("{status:?}")
}

/// The rerun answer, when this caller has already continued this claim.
///
/// Printed and returned as success: a rerun after an interrupted first run
/// must not create a second execution, and telling the caller "already done,
/// here it is" is the whole of that guarantee.
pub(super) fn already_continued(state: &HandoverState, caller: &str) -> Option<String> {
    let claim = state.claim();
    let active = claim.active()?;
    if active.claimant != caller {
        return None;
    }
    let id = state.fold.active_continuation.as_deref()?;
    let entry = state
        .fold
        .continuations
        .iter()
        .find(|entry| entry.event_id == id)?;
    Some(
        json!({
            "eventId": entry.event_id,
            "accepted": true,
            "type": "continuation",
            "mode": entry.mode.as_str(),
            "claimRef": entry.claim_ref,
            "target": coding_session_target_key(&entry.body.target),
            "rerun": true,
            "message": "this caller already holds the claim in force and a continuation of it \
                        already exists; nothing was published and no second execution was \
                        created",
            "scope": WHOLE_SESSION_DISCLOSURE,
        })
        .to_string(),
    )
}

/// Pick the execution a native resume would steer.
///
/// The checkpoint's author first — the seat that wrote it is the one whose
/// work this is — and otherwise the umbrella's most recently active
/// execution. A `None` here is a real answer: an umbrella with no execution on
/// the wire can only be reconstructed.
pub(super) fn select_candidate<'rows>(
    rows: &'rows [ExecutionRow],
    author: Option<&str>,
) -> Option<&'rows ExecutionRow> {
    if let Some(author) = author {
        if let Some(row) = rows
            .iter()
            .find(|row| row.execution.actor.as_deref() == Some(author))
        {
            return Some(row);
        }
    }
    rows.iter().max_by_key(|row| {
        (
            row.execution.last_signed_at.unwrap_or(i64::MIN),
            row.execution.target.generation,
        )
    })
}

/// The next action a continuation carries into the original execution.
fn next_action_of(checkpoint: Option<&CodingSessionHandoverCheckpoint>) -> String {
    match checkpoint {
        Some(body) if !body.next_action.trim().is_empty() => body.next_action.clone(),
        Some(_) => "Continue this session's accepted work. The checkpoint stated no next action."
            .to_owned(),
        None => "Continue this session's accepted work. No authorized checkpoint existed, so \
                 there is no stated next action; report what you find before changing anything."
            .to_owned(),
    }
}

/// What a native send produced.
struct NativeOutcome {
    command_id: String,
    /// The receipt stage and its error code, when one answered inside the wait.
    stage: Option<(ReceiptStatus, Option<String>)>,
    missing: Vec<String>,
}

/// Send the checkpoint's next action to the original execution as a turn.
async fn send_native_turn(
    client: &BuzzClient,
    channel: &str,
    target: &CodingSessionTarget,
    text: &str,
    wait_secs: u64,
) -> Result<NativeOutcome, CliError> {
    let channel_uuid = Uuid::parse_str(channel)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    let command_id = Uuid::new_v4().to_string();
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        target: target.clone(),
        action: CodingSessionAction::ThreadTurnStart {
            text: text.to_owned(),
            attachments: Vec::new(),
            // The default class: hold until any running turn settles. A
            // handover's next action is not an interrupt, and `steer` is a
            // request a runtime may not honour.
            deliver: CodingSessionDelivery::Boundary,
        },
    };
    let builder = super::crew_cmds::build_turn_command(channel_uuid, &payload)?;
    let event = client.sign_event_unchecked(builder)?;
    let since = chrono::Utc::now().timestamp() - 1;
    let raw = client.submit_event(event).await?;
    crate::commands::parse_write_response(&raw, "turn command already accepted")?;

    let stage = await_turn_stage(client, channel, &command_id, since, wait_secs).await;
    let missing = match &stage {
        Some((ReceiptStatus::TurnStarted, _)) => Vec::new(),
        Some((status, code)) => vec![format!(
            "the next action was not started on the original execution: its provider answered \
             {status:?}{}",
            code.as_deref()
                .map(|code| format!(" ({code})"))
                .unwrap_or_default()
        )],
        None => vec![format!(
            "no turn receipt answered command {command_id} within {wait_secs}s, so whether the \
             original execution began the next action is unknown"
        )],
    };
    Ok(NativeOutcome {
        command_id,
        stage,
        missing,
    })
}

/// Wait, bounded, for the first turn receipt answering `command_id`.
async fn await_turn_stage(
    client: &BuzzClient,
    channel: &str,
    command_id: &str,
    since: i64,
    wait_secs: u64,
) -> Option<(ReceiptStatus, Option<String>)> {
    let deadline = std::time::Instant::now() + Duration::from_secs(wait_secs);
    let filter = json!({
        "kinds": [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
        "#h": [channel],
        "#csl-command": [command_id],
        "since": since,
    });
    loop {
        if let Ok(events) = client.query_all(filter.clone()).await {
            if let Some(stage) = newest_stage_for(&events, command_id) {
                return Some(stage);
            }
        }
        if std::time::Instant::now() + RECEIPT_POLL >= deadline {
            return None;
        }
        tokio::time::sleep(RECEIPT_POLL).await;
    }
}

/// The furthest stage a receipt reports for `command_id`.
///
/// `turn_started` wins over `turn_queued` published in the same second, which
/// second-granularity timestamps cannot separate.
pub(super) fn newest_stage_for(
    events: &[Value],
    command_id: &str,
) -> Option<(ReceiptStatus, Option<String>)> {
    let mut best: Option<(u8, ReceiptStatus, Option<String>)> = None;
    for value in events {
        let Ok(event) = serde_json::from_value::<Event>(value.clone()) else {
            continue;
        };
        let Ok(receipt) = decode_coding_session_lifecycle_receipt(&event.content) else {
            continue;
        };
        if receipt.command_id != command_id {
            continue;
        }
        let rank = match receipt.status {
            ReceiptStatus::TurnQueued => 1,
            ReceiptStatus::TurnDegraded => 2,
            ReceiptStatus::TurnStarted => 3,
            // A boundary resume never asks for a steer, so neither of these
            // is expected here; ranked anyway so an unexpected one is read as
            // the terminal answer it is rather than as "no stage".
            ReceiptStatus::TurnInjected => 4,
            ReceiptStatus::TurnDropped
            | ReceiptStatus::TurnRefused
            | ReceiptStatus::TurnDeliveryUnknown => 5,
            _ => 0,
        };
        let code = receipt.error.as_ref().map(|error| error.code.clone());
        if best.as_ref().is_none_or(|(held, _, _)| rank > *held) {
            best = Some((rank, receipt.status, code));
        }
    }
    best.map(|(_, status, code)| (status, code))
}

/// Keep a line list inside the record's own bound without hiding the overflow.
pub(super) fn bound_lines(lines: Vec<String>, max: usize) -> Vec<String> {
    if lines.len() <= max {
        return lines;
    }
    let mut bounded: Vec<String> = lines.iter().take(max - 1).cloned().collect();
    bounded.push(format!(
        "and {} more line(s), beyond this record's {max}-line bound",
        lines.len() - (max - 1)
    ));
    bounded
}

/// Print the continuation.
fn report(
    event_id: &str,
    continuation: &CodingSessionHandoverContinuation,
    notes: &VerificationNotes,
    as_json: bool,
) {
    if as_json {
        println!(
            "{}",
            json!({
                "eventId": event_id,
                "accepted": true,
                "type": "continuation",
                "continuation": continuation,
                "scope": WHOLE_SESSION_DISCLOSURE,
                "notes": notes.to_json(),
            })
        );
        return;
    }
    println!("continuation {event_id} ({})", continuation.mode.as_str());
    println!("scope: {WHOLE_SESSION_DISCLOSURE}");
    println!(
        "target: {}",
        coding_session_target_key(&continuation.target)
    );
    for line in &continuation.recovered {
        println!("recovered: {line}");
    }
    for line in &continuation.missing {
        println!("missing: {line}");
    }
    if continuation.missing.is_empty() {
        println!("missing: nothing");
    }
    print!("{}", notes.render());
}
