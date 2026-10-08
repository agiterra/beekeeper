//! Tests for `bee sessions rewind`: the refusals made before anything is
//! signed, the exact 44221 it publishes, how the provider's signed receipts
//! are read, and the wording of every outcome.

use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

use beekeeper_core::coding_session_checkpoint::{
    CodingSessionCheckpointCoverage, CodingSessionCheckpointGit, CodingSessionCheckpointPayload,
    CodingSessionCheckpointReason, CODING_SESSION_CHECKPOINT_SCHEMA,
};
use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, RewindFiles,
};
use beekeeper_core::coding_session_payload::{
    LifecycleReceipt, TranscriptEnvelope, REWIND_NOT_RESTARTED, TREE_BUSY,
};
use beekeeper_core::coding_session_rewind::{ReceiptRewind, RewindFilesOutcome};
use beekeeper_sdk::builders::{
    build_coding_session_lifecycle_command, build_coding_session_lifecycle_receipt,
};
use beekeeper_sdk::coding_session_checkpoint::build_coding_session_checkpoint;
use beekeeper_sdk::kind::KIND_CODING_SESSION_TRANSCRIPT;

use super::crew::{CrewExecution, Liveness};
use super::crew_cmds::ReceiptWait;
use super::rewind::{
    classify_rewind_receipts, fold_rewind_report, parse_files, plan_rewind, rewind_payload,
    RewindAnswer,
};
use crate::error::CliError;

const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const COMMAND: &str = "rewind-0b7c1f9e-5a8d-4b2e-9c3f-1d2e3f4a5b6c";

fn target(generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".to_owned(),
        instance_id: "claude-primary".to_owned(),
        session_id: "s1".to_owned(),
        generation,
    }
}

fn execution(provider: &Keys, status: &str) -> CrewExecution {
    let target = target(2);
    CrewExecution {
        target_key: coding_session_target_key(&target),
        target,
        signer: provider.public_key().to_hex(),
        actor: None,
        role: None,
        session_ref: None,
        status: status.to_owned(),
        model: None,
        runtime: None,
        last_signed_seq: Some(9),
        last_signed_at: Some(1_000),
        liveness: Liveness::Quiet { age_secs: 60 },
        turn_budget: None,
    }
}

fn checkpoint_payload(restorable: bool, base_tree: Option<&str>) -> CodingSessionCheckpointPayload {
    CodingSessionCheckpointPayload {
        schema: CODING_SESSION_CHECKPOINT_SCHEMA.to_owned(),
        session: target(2),
        turn_id: Some("turn-3".to_owned()),
        reason: CodingSessionCheckpointReason::Turn,
        coverage: CodingSessionCheckpointCoverage {
            from_seq: 5,
            through_seq: 8,
        },
        git: Some(CodingSessionCheckpointGit {
            head: Some("a".repeat(40)),
            branch: Some("main".to_owned()),
            base_tree: base_tree.map(str::to_owned),
            tree: "b".repeat(40),
            commit: "c".repeat(40),
            outside_turn: Some(false),
            complete: true,
            omitted: vec![],
            omitted_not_listed: 0,
        }),
        files: vec![],
        files_not_listed: 0,
        restorable,
        unavailable: None,
        summary: None,
    }
}

fn signed_checkpoint(keys: &Keys, payload: &CodingSessionCheckpointPayload) -> (String, Value) {
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");
    let event = build_coding_session_checkpoint(channel, payload)
        .expect("builder")
        .sign_with_keys(keys)
        .expect("sign");
    (
        event.id.to_hex(),
        serde_json::to_value(event).expect("json"),
    )
}

/// One transcript item, so the checkpoint reader knows the generation's signer.
fn transcript(keys: &Keys) -> Value {
    transcript_for(keys, &target(2))
}

fn transcript_for(keys: &Keys, target: &CodingSessionTarget) -> Value {
    let envelope = TranscriptEnvelope::new(
        target,
        1,
        1_000,
        Some("turn-1"),
        json!({ "kind": "user_prompt", "text": "hi" }),
    );
    json!({
        "id": format!("{:064x}", 1),
        "pubkey": keys.public_key().to_hex(),
        "kind": KIND_CODING_SESSION_TRANSCRIPT,
        "created_at": 1,
        "sig": "0".repeat(128),
        "tags": [
            ["h", CHANNEL],
            ["cst-v", "cst1-1"],
            ["cs-target", coding_session_target_key(target)],
            ["cst-seq", "1"],
        ],
        "content": serde_json::to_string(&envelope).expect("envelope"),
    })
}

fn facts(checkpoint: &str, files: RewindFilesOutcome) -> ReceiptRewind {
    ReceiptRewind {
        checkpoint: checkpoint.to_owned(),
        cut_generation: 2,
        cut_after_seq: 4,
        previous_generation: 2,
        files,
        pre_rewind_checkpoint: Some("d".repeat(64)),
        head: Some("a".repeat(40)),
    }
}

fn signed_receipt(keys: &Keys, receipt: &LifecycleReceipt) -> Value {
    let event = build_coding_session_lifecycle_receipt(
        Uuid::parse_str(CHANNEL).expect("channel"),
        &receipt.command_id,
        &serde_json::to_string(receipt).expect("receipt"),
    )
    .expect("receipt builder")
    .sign_with_keys(keys)
    .expect("sign receipt");
    serde_json::to_value(event).expect("event json")
}

fn refusal_text(result: Result<(), CliError>) -> String {
    match result {
        Err(CliError::Refused(text)) | Err(CliError::NotFound(text)) => text,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ── Flags ────────────────────────────────────────────────────────────────────

#[test]
fn files_flag_is_a_closed_pair() {
    assert_eq!(parse_files("keep").expect("keep"), RewindFiles::Keep);
    assert_eq!(
        parse_files("restore").expect("restore"),
        RewindFiles::Restore
    );
    assert!(matches!(parse_files("all"), Err(CliError::Usage(_))));
}

// ── Refusals before anything is signed ──────────────────────────────────────

#[test]
fn a_restorable_turn_checkpoint_of_this_execution_is_planned() {
    let provider = Keys::generate();
    let (id, event) =
        signed_checkpoint(&provider, &checkpoint_payload(true, Some(&"e".repeat(40))));
    let events = vec![transcript(&provider), event];
    let execution = execution(&provider, "idle");
    plan_rewind(&execution, &events, &id, RewindFiles::Restore).expect("restore planned");
    plan_rewind(&execution, &events, &id, RewindFiles::Keep).expect("keep planned");
}

#[test]
fn rewind_refuses_what_the_record_already_rules_out() {
    let provider = Keys::generate();
    let execution = execution(&provider, "idle");

    // Not on the relay at all.
    let text = refusal_text(plan_rewind(
        &execution,
        &[transcript(&provider)],
        &"f".repeat(64),
        RewindFiles::Keep,
    ));
    assert!(text.contains("no kind 44231 checkpoint"), "{text}");

    // Captured by a provider that cannot rewind to it.
    let (id, event) =
        signed_checkpoint(&provider, &checkpoint_payload(false, Some(&"e".repeat(40))));
    let text = refusal_text(plan_rewind(
        &execution,
        &[transcript(&provider), event],
        &id,
        RewindFiles::Keep,
    ));
    assert!(text.contains("restorable: false"), "{text}");

    // Restore with no pre-turn tree; keep is still fine.
    let (id, event) = signed_checkpoint(&provider, &checkpoint_payload(true, None));
    let events = vec![transcript(&provider), event];
    let text = refusal_text(plan_rewind(&execution, &events, &id, RewindFiles::Restore));
    assert!(text.contains("--files keep"), "{text}");
    plan_rewind(&execution, &events, &id, RewindFiles::Keep).expect("keep needs no tree");

    // Another execution's checkpoint.
    let mut other = checkpoint_payload(true, Some(&"e".repeat(40)));
    other.session.session_id = "s2".to_owned();
    let (id, event) = signed_checkpoint(&provider, &other);
    let other_transcript = transcript_for(&provider, &other.session);
    let text = refusal_text(plan_rewind(
        &execution,
        &[transcript(&provider), other_transcript, event],
        &id,
        RewindFiles::Keep,
    ));
    assert!(text.contains("not to"), "{text}");

    // Signed by a key that does not sign the transcript.
    let stranger = Keys::generate();
    let (id, event) =
        signed_checkpoint(&stranger, &checkpoint_payload(true, Some(&"e".repeat(40))));
    let text = refusal_text(plan_rewind(
        &execution,
        &[transcript(&provider), event],
        &id,
        RewindFiles::Keep,
    ));
    assert!(text.contains("not usable"), "{text}");

    // A stopped execution.
    let (id, event) =
        signed_checkpoint(&provider, &checkpoint_payload(true, Some(&"e".repeat(40))));
    let stopped = super::crew::stopped_status_word();
    let text = refusal_text(plan_rewind(
        &self::execution(&provider, &stopped),
        &[transcript(&provider), event],
        &id,
        RewindFiles::Keep,
    ));
    assert!(text.contains("stopped"), "{text}");
}

// ── The published command ───────────────────────────────────────────────────

#[test]
fn the_published_command_is_the_five_key_session_rewind() {
    let provider = Keys::generate().public_key().to_hex();
    let checkpoint = "ab".repeat(32);
    let payload = rewind_payload(
        COMMAND,
        &target(2),
        &provider,
        &checkpoint,
        RewindFiles::Restore,
    );
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");
    let event = build_coding_session_lifecycle_command(channel, &payload)
        .expect("builder validates")
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    let content: Value = serde_json::from_str(&event.content).expect("json");
    assert_eq!(
        content["action"],
        json!({
            "type": "session.rewind",
            "session": serde_json::to_value(target(2)).expect("target"),
            "providerAuthorityPubkey": provider,
            "checkpoint": checkpoint,
            "files": "restore",
        })
    );
    assert_eq!(
        decode_coding_session_lifecycle_command(&event.content).expect("decodes"),
        payload
    );
}

// ── Receipts ────────────────────────────────────────────────────────────────

fn classify(
    provider: &Keys,
    receipts: &[LifecycleReceipt],
    checkpoint: &str,
) -> Result<Option<RewindAnswer>, String> {
    let events: Vec<Value> = receipts
        .iter()
        .map(|receipt| signed_receipt(provider, receipt))
        .collect();
    classify_rewind_receipts(
        &events,
        CHANNEL,
        COMMAND,
        &provider.public_key().to_hex(),
        &target(2),
        checkpoint,
    )
}

#[test]
fn a_resumed_receipt_with_rewind_facts_is_a_rewind() {
    let provider = Keys::generate();
    let checkpoint = "ab".repeat(32);
    let mut receipt = LifecycleReceipt::resumed(COMMAND, &target(3));
    receipt.rewind = Some(facts(&checkpoint, RewindFilesOutcome::Restored));
    let answer = classify(&provider, &[receipt], &checkpoint)
        .expect("consistent")
        .expect("answered");
    let report = fold_rewind_report(&ReceiptWait::Answered(answer), "old", COMMAND, 30);
    assert_eq!(report.outcome, "rewound");
    assert_eq!(report.restarted, Some("restarted"));
    assert_eq!(report.files, Some("restored"));
    assert_eq!(report.session, Some(coding_session_target_key(&target(3))));
    assert_eq!(report.rewind["cutAfterSeq"], json!(4));
    assert_eq!(report.rewind["preRewindCheckpoint"], json!("d".repeat(64)));
    assert!(report.error.is_none());
}

#[test]
fn a_receipt_from_another_key_is_not_an_answer() {
    let provider = Keys::generate();
    let checkpoint = "ab".repeat(32);
    let mut receipt = LifecycleReceipt::resumed(COMMAND, &target(3));
    receipt.rewind = Some(facts(&checkpoint, RewindFilesOutcome::Kept));
    let events = vec![signed_receipt(&Keys::generate(), &receipt)];
    let answer = classify_rewind_receipts(
        &events,
        CHANNEL,
        COMMAND,
        &provider.public_key().to_hex(),
        &target(2),
        &checkpoint,
    )
    .expect("consistent");
    assert!(answer.is_none());
}

#[test]
fn contradictory_success_receipts_are_reported_not_believed() {
    let provider = Keys::generate();
    let checkpoint = "ab".repeat(32);
    // Another checkpoint than the one asked for.
    let mut other = LifecycleReceipt::resumed(COMMAND, &target(3));
    other.rewind = Some(facts(&"cd".repeat(32), RewindFilesOutcome::Kept));
    let error = classify(&provider, &[other], &checkpoint).expect_err("other checkpoint");
    assert!(error.contains("names checkpoint"), "{error}");
    // A success with no rewind facts at all.
    let bare = LifecycleReceipt::resumed(COMMAND, &target(3));
    let error = classify(&provider, &[bare], &checkpoint).expect_err("no facts");
    assert!(error.contains("carries no rewind facts"), "{error}");
    // Two answers that disagree.
    let mut ok = LifecycleReceipt::resumed(COMMAND, &target(3));
    ok.rewind = Some(facts(&checkpoint, RewindFilesOutcome::Kept));
    let refused = LifecycleReceipt::failed(COMMAND, TREE_BUSY, "seat x is mid-turn");
    let error = classify(&provider, &[ok, refused], &checkpoint).expect_err("conflict");
    assert!(error.contains("conflicting"), "{error}");
}

#[test]
fn without_context_says_the_new_generation_remembers_nothing() {
    let provider = Keys::generate();
    let checkpoint = "ab".repeat(32);
    let mut receipt = LifecycleReceipt::resumed_without_context(COMMAND, &target(3), "lost");
    receipt.rewind = Some(facts(&checkpoint, RewindFilesOutcome::Kept));
    let answer = classify(&provider, &[receipt], &checkpoint)
        .expect("consistent")
        .expect("answered");
    let report = fold_rewind_report(&ReceiptWait::Answered(answer), "old", COMMAND, 30);
    assert_eq!(report.outcome, "rewound");
    assert_eq!(report.restarted, Some("restarted_without_context"));
    assert!(
        report.summary.contains("remembers nothing"),
        "{}",
        report.summary
    );
}

#[test]
fn not_restarted_reports_the_files_as_they_ended_and_fails() {
    let provider = Keys::generate();
    let checkpoint = "ab".repeat(32);
    let mut receipt = LifecycleReceipt::failed(
        COMMAND,
        REWIND_NOT_RESTARTED,
        "the session still remembers turns 3–5",
    );
    receipt.rewind = Some(facts(&checkpoint, RewindFilesOutcome::RestoreFailed));
    let answer = classify(&provider, &[receipt], &checkpoint)
        .expect("consistent")
        .expect("answered");
    let report = fold_rewind_report(&ReceiptWait::Answered(answer), "old", COMMAND, 30);
    assert_eq!(report.outcome, "not_restarted");
    assert_eq!(report.restarted, Some("not_restarted"));
    assert_eq!(report.files, Some("restore_failed"));
    assert!(
        report.summary.contains("still remembers"),
        "{}",
        report.summary
    );
    assert!(matches!(report.error, Some(CliError::Refused(_))));
}

#[test]
fn a_refusal_before_the_checks_passed_touched_nothing() {
    let provider = Keys::generate();
    let checkpoint = "ab".repeat(32);
    let receipt = LifecycleReceipt::failed(COMMAND, TREE_BUSY, "seat x is mid-turn");
    let answer = classify(&provider, &[receipt], &checkpoint)
        .expect("consistent")
        .expect("answered");
    let report = fold_rewind_report(&ReceiptWait::Answered(answer), "old", COMMAND, 30);
    assert_eq!(report.outcome, "refused");
    assert_eq!(report.files, Some("untouched"));
    assert!(report.summary.contains("TREE_BUSY"), "{}", report.summary);
    assert!(report.rewind.is_null());
    assert!(matches!(report.error, Some(CliError::Refused(_))));
}

#[test]
fn silence_is_unconfirmed_and_claims_nothing() {
    let report = fold_rewind_report(
        &ReceiptWait::Unconfirmed {
            query_failed: false,
        },
        "old",
        COMMAND,
        30,
    );
    assert_eq!(report.outcome, "unconfirmed");
    assert_eq!(report.restarted, None);
    assert_eq!(report.files, None);
    assert!(report.summary.contains("unknown"), "{}", report.summary);
    assert!(matches!(report.error, Some(CliError::Unconfirmed(_))));
}
