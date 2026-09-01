//! The runner's admission-time operation fence (2026-09-01).
//!
//! The lead runner used to fence turns by `commandId` alone, so two 44220s
//! carrying byte-identical identifier-only wake pointers for the same target
//! were two lead turns. Both producers — the provider's wake sender and the
//! founder's Desktop fallback — deliberately byte-match that pointer, so a
//! producer bug on either side spends a second turn of the lead's context.
//!
//! These tests pin the fix at the only place that spends model context:
//! admission. The process-local half lives in
//! [`crate::InFlightTurn::operation_key`]; the durable half is
//! `operations.jsonl` beside `commands.jsonl`.

use super::*;

/// A report pointer with the exact key set [`crate::team_wake::wake_text`]
/// mints for [`crate::team_wake::WakeSource::Report`].
fn report_pointer(operation_id: &str) -> String {
    serde_json::json!({"operationId": operation_id, "type": "report"}).to_string()
}

/// The fence key computed from the *specification* rather than from the
/// implementation under test: the target key, a NUL, and the pointer JSON
/// re-serialised through `serde_json::Value` (BTreeMap key order).
fn expected_fence_key(target: &CodingSessionTarget, pointer: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(pointer).expect("pointer json");
    format!(
        "{}\u{0}{}",
        buzz_core::coding_session_command::coding_session_target_key(target),
        serde_json::to_string(&value).expect("canonical pointer"),
    )
}

/// Every record in one of the provider's append-only ledgers.
fn ledger(state_dir: &Path, file: &str) -> Vec<serde_json::Value> {
    let Ok(body) = std::fs::read_to_string(state_dir.join(file)) else {
        return Vec::new();
    };
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("ledger line is json"))
        .collect()
}

/// The one receipt published for `command_id`, or a panic naming what was.
fn receipt_of(sink: &CollectingSink, command_id: &str) -> serde_json::Value {
    sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == command_id)
        .unwrap_or_else(|| panic!("{command_id} must be answered"))
}

fn assert_duplicate_refusal(sink: &CollectingSink, command_id: &str, owner: &str) {
    assert_eq!(
        receipt_stages(sink, command_id),
        vec!["turn_refused".to_owned()],
        "a duplicate operation is refused once and never queued"
    );
    let receipt = receipt_of(sink, command_id);
    assert_eq!(receipt["error"]["code"], "DUPLICATE_OPERATION");
    let message = receipt["error"]["message"]
        .as_str()
        .expect("refusal message")
        .to_owned();
    assert!(
        message.contains(owner),
        "the refusal must name the owning command; got {message}"
    );
    payload::decode_coding_session_lifecycle_receipt(&receipt.to_string())
        .expect("strictly decodable duplicate refusal");
}

/// P-T1 / acceptance #9 — the duplicate-producer attack.
///
/// One live execution, three 44220s with different command ids and one
/// byte-identical report pointer. Exactly one turn runs; the other two are
/// refused `DUPLICATE_OPERATION` naming the owner, spend no turn, and are
/// answered once each. The second is fenced while the owner is still only
/// *accepted* (process-local); the third after it started (durable).
#[tokio::test]
async fn a_duplicate_team_wake_pointer_is_refused_and_spends_no_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let mut provider = provider(&state_dir, Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    let target = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .target("instance-1");

    let pointer = report_pointer(&"79".repeat(32));
    let base = now_secs();

    // The owner: accepted into the mailbox, not yet started.
    provider
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "wake-owner", &target, &pointer, base),
        )
        .await
        .expect("handle");
    // A second producer, same pointer, different id, while the owner is still
    // only accepted. The process-local half of the fence answers this one.
    provider
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "wake-inflight-dup", &target, &pointer, base + 1),
        )
        .await
        .expect("handle");

    pump_until_turn_finished(&mut provider).await;

    // A third copy after the owner started. Only the durable ledger can
    // answer this one — the in-flight entry is gone.
    provider
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "wake-durable-dup", &target, &pointer, base + 2),
        )
        .await
        .expect("handle");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");

    assert_eq!(
        receipt_stages(&sink, "wake-owner"),
        vec!["turn_queued".to_owned(), "turn_started".to_owned()],
        "the owner is queued at admission and started once"
    );
    assert_duplicate_refusal(&sink, "wake-inflight-dup", "wake-owner");
    assert_duplicate_refusal(&sink, "wake-durable-dup", "wake-owner");

    let started: Vec<(String, String)> = turn_receipts_in_order(&sink)
        .into_iter()
        .filter(|(_, status)| status == "turn_started")
        .collect();
    assert_eq!(
        started,
        vec![("wake-owner".to_owned(), "turn_started".to_owned())],
        "exactly one lead turn may be spent on one operation"
    );

    let consumed: Vec<String> = ledger(&state_dir, "commands.jsonl")
        .into_iter()
        .filter_map(|record| record["commandId"].as_str().map(str::to_owned))
        .filter(|id| id.starts_with("wake-"))
        .collect();
    assert_eq!(consumed, vec!["wake-owner".to_owned()]);

    let refused: Vec<String> = ledger(&state_dir, "refusals.jsonl")
        .into_iter()
        .filter_map(|record| record["commandId"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        refused,
        vec![
            "wake-inflight-dup".to_owned(),
            "wake-durable-dup".to_owned()
        ],
        "both duplicates are durably answered so a redelivery cannot stutter"
    );

    let operations = ledger(&state_dir, "operations.jsonl");
    assert_eq!(operations.len(), 1, "{operations:?}");
    assert_eq!(operations[0]["key"], expected_fence_key(&target, &pointer));
    assert_eq!(operations[0]["commandId"], "wake-owner");
}

/// P-T2 — the durable half survives a restart.
///
/// The owner started and the process died. A duplicate delivered by the
/// relay's replay is answered from `operations.jsonl` alone, and the owner's
/// own redelivery stays silent because it is already consumed.
#[tokio::test]
async fn a_duplicate_replayed_after_a_restart_is_refused_from_the_durable_ledger() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");

    let pointer = report_pointer(&"5c".repeat(32));
    let base = now_secs();

    let (target, owner) = {
        let mut provider = provider(&state_dir, Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        let owner = turn_event_at(channel_id, "wake-owner", &target, &pointer, base);
        provider
            .handle_command_event(channel_id, &owner)
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;
        assert!(provider.state().is_command_consumed("wake-owner"));
        (target, owner)
    };

    let mut restarted = provider(&state_dir, Some(&projects));
    restarted.recover().expect("recover");
    assert_eq!(
        restarted
            .state()
            .operation_owner(&expected_fence_key(&target, &pointer)),
        Some("wake-owner"),
        "the operation ledger is loaded on restart"
    );

    // The relay replays both: the owner (already consumed, silent) and the
    // duplicate (never seen by this process).
    restarted
        .handle_command_event(channel_id, &owner)
        .await
        .expect("replay owner");
    restarted
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "wake-replayed-dup", &target, &pointer, base + 1),
        )
        .await
        .expect("replay duplicate");

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    assert!(
        receipt_stages(&sink, "wake-owner").is_empty(),
        "a redelivered consumed command is silent, not answered twice"
    );
    assert_duplicate_refusal(&sink, "wake-replayed-dup", "wake-owner");
    assert!(
        !restarted.state().is_command_consumed("wake-replayed-dup"),
        "a refused duplicate never ran, so nothing may claim it did"
    );
    let operations = ledger(&state_dir, "operations.jsonl");
    assert_eq!(operations.len(), 1, "{operations:?}");
}

/// P-T3 — a crash while the owner is still queued.
///
/// Nothing started, so nothing is durably custodied: the fence lives and dies
/// with the process that held it. What must not survive the crash is a second
/// *turn* — the duplicate was answered terminally before the crash and stays
/// answered, and the owner, whose execution died with the process, is dropped
/// out loud rather than run twice.
#[tokio::test]
async fn a_crash_while_queued_releases_the_fence_without_leaking_a_second_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");

    let pointer = report_pointer(&"3e".repeat(32));
    let base = now_secs();

    let (target, owner, duplicate) = {
        let mut provider = stalling_provider(&state_dir, Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");

        // Occupy the execution so nothing behind it can start.
        provider
            .handle_command_event(
                channel_id,
                &turn_event_at(channel_id, "blocker", &target, "hold the slot", base),
            )
            .await
            .expect("handle");
        pump_until_turn_started(&mut provider).await;

        let owner = turn_event_at(channel_id, "wake-owner", &target, &pointer, base + 1);
        let duplicate = turn_event_at(channel_id, "wake-dup", &target, &pointer, base + 2);
        provider
            .handle_command_event(channel_id, &owner)
            .await
            .expect("handle");
        provider
            .handle_command_event(channel_id, &duplicate)
            .await
            .expect("handle");

        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            receipt_stages(&sink, "wake-owner"),
            vec!["turn_queued".to_owned()],
            "the owner is accepted and has not started"
        );
        assert_duplicate_refusal(&sink, "wake-dup", "wake-owner");
        assert!(
            ledger(&state_dir, "operations.jsonl").is_empty(),
            "nothing started, so nothing is durably custodied"
        );
        // SIGKILL.
        (target, owner, duplicate)
    };

    let mut restarted = stalling_provider(&state_dir, Some(&projects));
    restarted.recover().expect("recover");
    assert_eq!(
        restarted
            .state()
            .operation_owner(&expected_fence_key(&target, &pointer)),
        None,
        "an owner that never started leaves no durable claim behind it"
    );

    restarted.open_replay_window(channel_id);
    for event in [&duplicate, &owner] {
        restarted
            .handle_command_event(channel_id, event)
            .await
            .expect("replay");
    }
    restarted.flush_replays_now().await.expect("deliver replay");

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let answers: Vec<(String, String)> = turn_receipts_in_order(&sink)
        .into_iter()
        .filter(|(command_id, _)| command_id.starts_with("wake-"))
        .collect();
    assert_eq!(
        answers,
        vec![("wake-owner".to_owned(), "turn_dropped".to_owned())],
        "the duplicate stays durably answered, and the owner's dead execution \
         is reported once"
    );
    let consumed: Vec<String> = ledger(&state_dir, "commands.jsonl")
        .into_iter()
        .filter_map(|record| record["commandId"].as_str().map(str::to_owned))
        .filter(|id| id.starts_with("wake-"))
        .collect();
    assert!(
        consumed.is_empty(),
        "no lead turn was spent on this operation across the crash: {consumed:?}"
    );
}

/// P-T4 — a dropped owner releases the fence.
///
/// The queue-full drop removes the in-flight entry and writes no durable
/// record, so a re-armed command with a *new* id and the same pointer is
/// admitted. A fence that outlived its owner's failure would lose the
/// operation permanently.
#[tokio::test]
async fn a_dropped_owner_releases_the_fence_for_a_rearmed_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let mut provider = stalling_provider(&state_dir, Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    let record = provider.state().sessions().next().expect("session").clone();
    let target = record.target("instance-1");

    let pointer = report_pointer(&"c1".repeat(32));
    let base = now_secs();
    provider
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "blocker", &target, "hold the slot", base),
        )
        .await
        .expect("handle");
    pump_until_turn_started(&mut provider).await;

    provider
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "wake-owner", &target, &pointer, base + 1),
        )
        .await
        .expect("handle");
    provider
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "wake-dup", &target, &pointer, base + 2),
        )
        .await
        .expect("handle");

    // The owner's mailbox overflowed: terminal, and it never ran.
    provider
        .handle_session_event(SessionEvent::TurnDropped {
            session_id: record.session_id.clone(),
            command_id: "wake-owner".into(),
        })
        .expect("record the drop");
    assert!(
        ledger(&state_dir, "operations.jsonl").is_empty(),
        "a turn that never started never claimed the operation"
    );

    // The re-arm: new id, same pointer, same target.
    provider
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "wake-rearm", &target, &pointer, base + 3),
        )
        .await
        .expect("handle");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_duplicate_refusal(&sink, "wake-dup", "wake-owner");
    assert_eq!(
        receipt_stages(&sink, "wake-owner"),
        vec!["turn_queued".to_owned(), "turn_dropped".to_owned()],
    );
    assert_eq!(
        receipt_stages(&sink, "wake-rearm"),
        vec!["turn_queued".to_owned()],
        "the released fence admits the re-armed command"
    );
}

/// P-T5 — the target generation is part of the key.
///
/// The same pointer addressed to generation 1 and, after a resume, to
/// generation 2 are two operations: the second execution never saw the first
/// one's prompt, so fencing it would silently drop the wake.
#[tokio::test]
async fn the_same_pointer_to_a_later_generation_is_a_different_operation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let keys = Keys::generate();
    let agent = fake_agent(dir.path(), "resumable-agent", RESUMABLE_AGENT);

    let pointer = report_pointer(&"a4".repeat(32));
    let base = now_secs();

    let session_id = {
        let mut first = Provider::new(config_of(
            keys.clone(),
            &state_dir,
            Some(&projects),
            agent.clone(),
        ))
        .expect("provider");
        first
            .handle_command_event(channel_id, &create_event(&first, channel_id, "create-1"))
            .await
            .expect("create");
        let target = first
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        first
            .handle_command_event(
                channel_id,
                &turn_event_at(channel_id, "wake-gen-1", &target, &pointer, base),
            )
            .await
            .expect("handle");
        pump_until_turn_started(&mut first).await;
        assert_eq!(
            first
                .state()
                .operation_owner(&expected_fence_key(&target, &pointer)),
            Some("wake-gen-1"),
        );
        let session_id = first
            .state()
            .sessions()
            .next()
            .expect("session")
            .session_id
            .clone();
        session_id
    };

    let mut restarted =
        Provider::new(config_of(keys, &state_dir, Some(&projects), agent)).expect("provider");
    restarted.recover().expect("recover");
    let previous = restarted
        .state()
        .session(&session_id)
        .expect("session")
        .target(&restarted.config.instance_id);
    let resume = lifecycle_target_event(
        &restarted,
        channel_id,
        "resume-1",
        "session.resume",
        &previous,
    );
    restarted
        .handle_command_event(channel_id, &resume)
        .await
        .expect("resume");
    let current = restarted
        .state()
        .session(&session_id)
        .expect("session")
        .target(&restarted.config.instance_id);
    assert_eq!(current.generation, 2);

    restarted
        .handle_command_event(
            channel_id,
            &turn_event_at(channel_id, "wake-gen-2", &current, &pointer, base + 1),
        )
        .await
        .expect("handle");
    pump_until_turn_started(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_stages(&sink, "wake-gen-2"),
        vec!["turn_queued".to_owned(), "turn_started".to_owned()],
        "a new generation is a new operation, not a duplicate"
    );
    let keys_recorded: Vec<String> = ledger(&state_dir, "operations.jsonl")
        .into_iter()
        .filter_map(|record| record["key"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        keys_recorded,
        vec![
            expected_fence_key(&previous, &pointer),
            expected_fence_key(&current, &pointer),
        ],
        "the two generations own two distinct operations"
    );
}

/// P-T6 — prose, and JSON that is not a wake pointer, are never fenced.
///
/// Only the two shapes `wake_text` mints are identifier-only pointers to one
/// governed operation. Fencing anything else would silently swallow an
/// operator who sent the same sentence twice on purpose.
#[tokio::test]
async fn identical_prose_and_foreign_json_are_never_fenced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = stalling_provider(&dir.path().join("state"), Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    let target = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .target("instance-1");

    let base = now_secs();
    let foreign = serde_json::json!({
        "operationId": "79".repeat(32),
        "type": "report",
        "note": "an extra key is not the pointer shape",
    })
    .to_string();
    let almost = serde_json::json!({"operationId": "not-a-hex-id", "type": "report"}).to_string();
    for (index, (first, second, text)) in [
        ("prose-1", "prose-2", "please summarise the report"),
        ("foreign-1", "foreign-2", foreign.as_str()),
        ("almost-1", "almost-2", almost.as_str()),
    ]
    .into_iter()
    .enumerate()
    {
        let at = base + (index as u64) * 10;
        provider
            .handle_command_event(
                channel_id,
                &turn_event_at(channel_id, first, &target, text, at),
            )
            .await
            .expect("handle");
        provider
            .handle_command_event(
                channel_id,
                &turn_event_at(channel_id, second, &target, text, at + 1),
            )
            .await
            .expect("handle");
    }

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    for command_id in [
        "prose-1",
        "prose-2",
        "foreign-1",
        "foreign-2",
        "almost-1",
        "almost-2",
    ] {
        assert!(
            receipt_stages(&sink, command_id)
                .first()
                .is_some_and(|stage| stage == "turn_queued"),
            "{command_id} must be admitted: {:?}",
            receipt_stages(&sink, command_id)
        );
    }
    assert!(
        ledger(&dir.path().join("state"), "operations.jsonl").is_empty(),
        "nothing here names a governed operation"
    );
}

/// P1 — the receipt code is one string, defined in `buzz-core` and re-exported
/// through the provider's `payload` module like every other code, so the
/// runner, the CLI, and Desktop cannot drift apart on its spelling.
#[test]
fn the_duplicate_operation_code_is_shared_with_every_other_reader() {
    assert_eq!(crate::payload::DUPLICATE_OPERATION, "DUPLICATE_OPERATION");
    assert_eq!(
        buzz_core::coding_session_payload::DUPLICATE_OPERATION,
        crate::payload::DUPLICATE_OPERATION,
    );
    // A code has to be a code: nonblank, control-free, and short enough for a
    // badge — the same shape every turn-stage receipt enforces.
    assert!(
        !crate::payload::DUPLICATE_OPERATION.is_empty()
            && crate::payload::DUPLICATE_OPERATION.len()
                <= buzz_core::coding_session_payload::MAX_RECEIPT_ERROR_CODE_BYTES
    );
}

fn fence_target(generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-code-acp".into(),
        instance_id: "instance-1".into(),
        session_id: "lead-session".into(),
        generation,
    }
}

/// P2 — both shapes `wake_text` mints fence, and only those two.
#[test]
fn only_the_two_wake_pointer_shapes_fence_an_operation() {
    let target = fence_target(1);
    let report = team_wake::wake_text(&team_wake::WakeSource::Report {
        operation_id: "79".repeat(32),
        operation_type: "report".into(),
        author_pubkey: "aa".repeat(32),
        created_at: 10,
    })
    .expect("report pointer");
    let terminal = team_wake::wake_text(&team_wake::WakeSource::Terminal {
        terminal_event_id: "bb".repeat(32),
        actor_pubkey: "cc".repeat(32),
        role: "builder".into(),
        caused_by_command_id: "csl-1".into(),
        source_target: fence_target(3),
        prompt_at_ms: Some(1),
        terminal_at_ms: 2,
    })
    .expect("terminal pointer");

    for pointer in [&report, &terminal] {
        let key = team_wake::operation_fence_key(&target, pointer)
            .unwrap_or_else(|| panic!("{pointer} is a wake pointer and must fence"));
        assert_eq!(key, expected_fence_key(&target, pointer));
        assert!(
            key.contains('\u{0}'),
            "the target key and the pointer are separated by a NUL, never concatenated"
        );
    }
    assert_ne!(
        team_wake::operation_fence_key(&target, &report),
        team_wake::operation_fence_key(&target, &terminal),
    );
}

/// P2 — the exact target generation is inside the key.
#[test]
fn the_fence_key_changes_with_the_target_generation() {
    let pointer = report_pointer(&"79".repeat(32));
    let one = team_wake::operation_fence_key(&fence_target(1), &pointer).expect("gen 1");
    let two = team_wake::operation_fence_key(&fence_target(2), &pointer).expect("gen 2");
    assert_ne!(one, two);
    let mut elsewhere = fence_target(1);
    elsewhere.session_id = "other-session".into();
    assert_ne!(
        one,
        team_wake::operation_fence_key(&elsewhere, &pointer).expect("other session")
    );
}

/// P2 — canonicalisation: whitespace and key order are not identity.
///
/// Two producers serialising the same pointer with different key order must
/// land on one key, or the fence is trivially defeated by a formatter.
#[test]
fn whitespace_and_key_order_variants_share_one_fence_key() {
    let target = fence_target(1);
    let id = "79".repeat(32);
    let canonical = report_pointer(&id);
    let reordered = format!("{{\"type\":\"report\",\"operationId\":\"{id}\"}}");
    let spaced = format!("{{ \"operationId\" : \"{id}\" ,\n  \"type\": \"report\" }}");
    let expected = team_wake::operation_fence_key(&target, &canonical).expect("canonical");
    for variant in [&reordered, &spaced] {
        assert_eq!(
            team_wake::operation_fence_key(&target, variant).as_deref(),
            Some(expected.as_str()),
            "{variant}"
        );
    }
}

/// P2 — everything that is not one of the two pointer shapes is unfenced.
#[test]
fn prose_and_near_miss_json_never_fence() {
    let target = fence_target(1);
    let id = "79".repeat(32);
    for text in [
        "please summarise the builder's report".to_owned(),
        String::new(),
        "[]".to_owned(),
        "\"a bare string\"".to_owned(),
        "42".to_owned(),
        // An extra key.
        serde_json::json!({"operationId": id, "type": "report", "note": "x"}).to_string(),
        // A missing key.
        serde_json::json!({"operationId": id}).to_string(),
        // A blank type.
        serde_json::json!({"operationId": id, "type": ""}).to_string(),
        // Not an event id.
        serde_json::json!({"operationId": "nope", "type": "report"}).to_string(),
        serde_json::json!({"operationId": "7", "type": "report"}).to_string(),
        // A non-string type.
        serde_json::json!({"operationId": id, "type": 1}).to_string(),
        // The terminal shape with a foreign schema.
        serde_json::json!({
            "schema": "someone-elses/v1",
            "type": "turn_ended_without_required_operation",
            "terminalEventId": id,
            "seatRole": "builder",
            "causedByCommandId": "csl-1",
        })
        .to_string(),
        // The terminal key set with a non-string member.
        serde_json::json!({
            "schema": "buzz-team-wake/v1",
            "type": "turn_ended_without_required_operation",
            "terminalEventId": id,
            "seatRole": "builder",
            "causedByCommandId": 7,
        })
        .to_string(),
    ] {
        assert_eq!(
            team_wake::operation_fence_key(&target, &text),
            None,
            "{text} must never fence"
        );
    }
}

/// P-T3 / acceptance #9, the reachable form: two same-pointer commands held
/// undecided, then both replayed into a **live** execution.
///
/// No crash is needed. The socket drops, both producers publish their wake
/// during the outage, the resubscribe opens a replay window
/// (`reopen_replay_windows_after_reconnect`), and both 44220s sit in
/// `replay.held` — neither decided — until `deliver_held_commands` feeds them
/// one at a time into an execution that never died. This is the shape the
/// crash matrix is really about, and the one that produces the assertion the
/// substitute property test below cannot: exactly one `turn_started`.
#[tokio::test]
async fn two_same_pointer_commands_replayed_into_a_live_execution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let mut provider = provider(&state_dir, Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    let target = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .target("instance-1");
    let pointer = report_pointer(&"66".repeat(32));
    let base = now_secs();

    provider.open_replay_window(channel_id);
    let provider_mint = turn_event_at(channel_id, "wake-provider", &target, &pointer, base);
    let desktop_mint = turn_event_at(
        channel_id,
        "team-wake-v1:desktop",
        &target,
        &pointer,
        base + 1,
    );
    // Served newest-first, as the relay serves stored events.
    for event in [&desktop_mint, &provider_mint] {
        provider
            .handle_command_event(channel_id, event)
            .await
            .expect("hold");
    }
    let sink_before = CollectingSink::new();
    provider.flush(&sink_before).await.expect("flush");
    let early: Vec<(String, String)> = turn_receipts_in_order(&sink_before)
        .into_iter()
        .filter(|(id, _)| id.contains("wake"))
        .collect();
    assert!(
        early.is_empty(),
        "both are held, so neither is decided yet: {early:?}"
    );

    provider.flush_replays_now().await.expect("deliver replay");
    pump_until_turn_finished(&mut provider).await;
    // Drain whatever else the actor had to say. Without this the fence could
    // be absent and the second turn merely not folded yet, which would make
    // the "exactly one `turn_started`" assertion below vacuous.
    pump_available(&mut provider).await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let started: Vec<String> = turn_receipts_in_order(&sink)
        .into_iter()
        .filter(|(_, status)| status == "turn_started")
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        started,
        vec!["wake-provider".to_owned()],
        "exactly one lead turn, and it is the one the operator sent first"
    );
    assert_duplicate_refusal(&sink, "team-wake-v1:desktop", "wake-provider");
    let operations = ledger(&state_dir, "operations.jsonl");
    assert_eq!(operations.len(), 1, "{operations:?}");
    assert_eq!(operations[0]["commandId"], "wake-provider");
}

/// P5 — the owner is *always* admitted, and this is the case that rule exists
/// for.
///
/// `operations.jsonl` is written before `commands.jsonl` (`lib.rs` `TurnStarted`
/// arm), so a crash between the two leaves exactly this state: the operation
/// ledger names the owner and the command ledger does not. The replay then
/// re-delivers the owner. Refusing it as its own duplicate would lose the wake
/// permanently — which is the one failure mode here worse than spending an
/// extra turn.
#[tokio::test]
async fn the_owner_is_admitted_when_only_the_operation_ledger_recorded_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let pointer = report_pointer(&"9d".repeat(32));
    let base = now_secs();

    let (target, owner) = {
        let mut provider = provider(&state_dir, Some(&projects));
        provider
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
            .await
            .expect("handle");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        let owner = turn_event_at(channel_id, "wake-owner", &target, &pointer, base);
        provider
            .handle_command_event(channel_id, &owner)
            .await
            .expect("handle");
        pump_until_turn_finished(&mut provider).await;
        assert!(provider.state().is_command_consumed("wake-owner"));
        (target, owner)
    };

    // Reproduce the crash window: the operation write landed, the command
    // write did not.
    let commands_path = state_dir.join("commands.jsonl");
    let body = std::fs::read_to_string(&commands_path).expect("read command ledger");
    let kept: String = body
        .lines()
        .filter(|line| !line.contains("wake-owner"))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&commands_path, kept).expect("write command ledger");

    let mut restarted = provider(&state_dir, Some(&projects));
    restarted.recover().expect("recover");
    assert!(
        !restarted.state().is_command_consumed("wake-owner"),
        "the crash window is only interesting while the command is unconsumed"
    );
    assert_eq!(
        restarted
            .state()
            .operation_owner(&expected_fence_key(&target, &pointer)),
        Some("wake-owner"),
        "the operation ledger survived the window"
    );

    restarted
        .handle_command_event(channel_id, &owner)
        .await
        .expect("replay owner");
    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let stages = receipt_stages(&sink, "wake-owner");
    assert!(
        !stages.contains(&"turn_refused".to_owned()),
        "the recorded owner must be admitted, not refused as its own duplicate: {stages:?}"
    );
}
