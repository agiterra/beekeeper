//! Tests for the crew addressing rules.
//!
//! Every case here is a pure function over synthesized events or rows: the
//! question "does a message reach the seat the sender named" must be
//! answerable without a relay, because the wrong answer is silent.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionDelivery, CodingSessionTarget, CODING_SESSION_COMMAND_SCHEMA,
};
use buzz_core::coding_session_lease::{
    CodingSessionLease, CodingSessionLeaseState, CODING_SESSION_LEASE_TAG_VERSION,
};
use buzz_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::{
    Capabilities, LifecycleReceipt, ReceiptError, ReceiptStatus, SessionMetadata, SessionStatus,
    TranscriptEnvelope, LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA, NO_LIVE_EXECUTION,
    NO_TURN_IN_FLIGHT, QUEUE_FULL, STALE_GENERATION,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_LEASE, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
};

use super::crew::*;
use super::crew_cmds::{resolve_json_lines, status_json_lines, status_row};
use super::{decode_metadata, decode_receipts, decode_transcripts, diagnose_turns};
use crate::error::CliError;

const CHANNEL: &str = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const PROVIDER: &str = "aa";
const ALICE: &str = "11";
const BOB: &str = "22";

fn pk(seed: &str) -> String {
    seed.repeat(32)
}

fn target(session_id: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "instance-1".into(),
        session_id: session_id.into(),
        generation,
    }
}

// ── Row builders ─────────────────────────────────────────────────────────────

fn execution(
    session_id: &str,
    generation: u64,
    actor: Option<&str>,
    role: Option<&str>,
    session_ref: Option<&str>,
) -> CrewExecution {
    let target = target(session_id, generation);
    CrewExecution {
        target_key: coding_session_target_key(&target),
        target,
        signer: pk(PROVIDER),
        actor: actor.map(str::to_owned),
        role: role.map(str::to_owned),
        session_ref: session_ref.map(str::to_owned),
        status: "idle".into(),
        model: Some("claude-opus".into()),
        runtime: Some("claude".try_into().expect("runtime")),
        last_signed_seq: Some(4),
        last_signed_at: Some(1_000),
        liveness: Liveness::Quiet { age_secs: 60 },
        turn_budget: None,
    }
}

// ── Event builders ───────────────────────────────────────────────────────────

fn metadata_event(
    id: &str,
    created_at: i64,
    target: &CodingSessionTarget,
    status: SessionStatus,
    actor: Option<&str>,
    role: Option<&str>,
    session_ref: Option<&str>,
) -> Value {
    let payload = SessionMetadata {
        schema: METADATA_SCHEMA.to_owned(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: actor.map(str::to_owned),
        role: role.map(str::to_owned),
        provider: Some("claude-primary".try_into().expect("alias")),
        runtime: Some("claude".try_into().expect("runtime")),
        model: Some("claude-opus".into()),
        status,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: session_ref.map(str::to_owned),
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        handover: None,
    };
    json!({
        "id": id,
        "pubkey": pk(PROVIDER),
        "kind": buzz_sdk::kind::KIND_CODING_SESSION_METADATA,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["csm-v", "csm1-1"],
                 ["cs-target", coding_session_target_key(target)]],
        "content": serde_json::to_string(&payload).expect("serialize"),
    })
}

/// Inject a `turnBudget` key into a metadata event's content, exactly as a
/// provider that has started publishing contract-B budgets would.
///
/// `SessionMetadata` does carry the typed `turn_budget` field, but
/// `metadata_event` above takes a fixed parameter list and cannot express a
/// *malformed* budget at all — the typed field would refuse to serialize one.
/// Editing the already-serialized content JSON is therefore the only way to
/// synthesize both the well-formed and the half-written shapes this section
/// tests.
fn with_turn_budget(mut event: Value, budget: Value) -> Value {
    let content = event["content"].as_str().expect("content is a string");
    let mut decoded: Value = serde_json::from_str(content).expect("valid JSON content");
    decoded["turnBudget"] = budget;
    event["content"] = json!(serde_json::to_string(&decoded).expect("serialize"));
    event
}

fn command_event(
    id: &str,
    signer: &str,
    created_at: i64,
    command_id: &str,
    target: &CodingSessionTarget,
    action: CodingSessionAction,
) -> Value {
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        target: target.clone(),
        action,
    };
    json!({
        "id": id,
        "pubkey": pk(signer),
        "kind": KIND_CODING_SESSION_COMMAND,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["cs-v", "csc1-1"],
                 ["cs-target", coding_session_target_key(target)]],
        "content": serde_json::to_string(&payload).expect("serialize"),
    })
}

fn turn_event(
    id: &str,
    signer: &str,
    at: i64,
    command_id: &str,
    target: &CodingSessionTarget,
    text: &str,
) -> Value {
    command_event(
        id,
        signer,
        at,
        command_id,
        target,
        CodingSessionAction::ThreadTurnStart {
            text: text.to_owned(),
            attachments: Vec::new(),
            deliver: CodingSessionDelivery::Boundary,
        },
    )
}

fn receipt_event(
    id: &str,
    created_at: i64,
    command_id: &str,
    status: ReceiptStatus,
    target: &CodingSessionTarget,
    error: Option<(&str, &str)>,
    turn_id: Option<&str>,
) -> Value {
    let receipt = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        status,
        session: Some(target.clone()),
        error: error.map(|(code, message)| ReceiptError {
            code: code.to_owned(),
            message: message.to_owned(),
        }),
        turn_id: turn_id.map(str::to_owned),
    };
    json!({
        "id": id,
        "pubkey": pk(PROVIDER),
        "kind": KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["cslr-v", "cslr1-1"]],
        "content": serde_json::to_string(&receipt).expect("serialize"),
    })
}

fn transcript_event(
    id: &str,
    created_at: i64,
    target: &CodingSessionTarget,
    seq: u64,
    turn_id: Option<&str>,
    item: Value,
) -> Value {
    let envelope = TranscriptEnvelope::new(target, seq, created_at * 1000, turn_id, item);
    json!({
        "id": id,
        "pubkey": pk(PROVIDER),
        "kind": buzz_sdk::kind::KIND_CODING_SESSION_TRANSCRIPT,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["cst-v", "cst1-1"],
                 ["cs-target", coding_session_target_key(target)],
                 ["cst-seq", seq.to_string()]],
        "content": serde_json::to_string(&envelope).expect("serialize"),
    })
}

fn lease_event(target: &CodingSessionTarget, state: CodingSessionLeaseState) -> Value {
    let lease = CodingSessionLease::new(target.clone(), state, 1).expect("lease");
    json!({
        "id": "lease-1",
        "pubkey": pk(PROVIDER),
        "kind": KIND_CODING_SESSION_LEASE,
        "created_at": 2_000,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["cslease-v", CODING_SESSION_LEASE_TAG_VERSION],
                 ["cs-target", coding_session_target_key(target)],
                 ["csl-command", "create-1"]],
        "content": serde_json::to_string(&lease).expect("serialize"),
    })
}

fn usage_message(error: CliError) -> String {
    match error {
        CliError::Usage(message) | CliError::NotFound(message) => message,
        other => panic!("expected a usage/not-found error, got {other:?}"),
    }
}

// ── --to resolution ──────────────────────────────────────────────────────────

#[test]
fn an_exact_target_key_resolves_to_that_generation() {
    let rows = vec![
        execution("s-1", 1, None, None, None),
        execution("s-1", 2, None, None, None),
    ];
    let wanted = rows[0].target_key.clone();
    let resolved = resolve_send_target(&rows, &wanted, None).expect("resolves");
    assert_eq!(resolved.target.generation, 1);
}

#[test]
fn a_session_id_resolves_to_its_newest_generation() {
    let rows = vec![
        execution("s-1", 1, None, None, None),
        execution("s-1", 3, None, None, None),
        execution("s-1", 2, None, None, None),
    ];
    let resolved = resolve_send_target(&rows, "s-1", None).expect("resolves");
    assert_eq!(resolved.target.generation, 3);
}

#[test]
fn a_role_resolves_inside_the_named_umbrella() {
    let rows = vec![
        execution("s-1", 1, Some(&pk(ALICE)), Some("builder"), Some("u-1")),
        execution("s-2", 1, Some(&pk(BOB)), Some("verifier"), Some("u-1")),
    ];
    let resolved = resolve_send_target(&rows, "builder", Some("u-1")).expect("resolves");
    assert_eq!(resolved.target.session_id, "s-1");
}

/// Rule 2: a role slug is only unique inside one umbrella, so an unscoped
/// lookup is refused rather than widened. Without this, `--to builder` in a
/// channel running two crews reaches whichever seat sorted first.
#[test]
fn a_role_without_an_umbrella_is_refused_and_names_the_umbrellas() {
    let rows = vec![
        execution("s-1", 1, Some(&pk(ALICE)), Some("builder"), Some("u-1")),
        execution("s-2", 1, Some(&pk(BOB)), Some("builder"), Some("u-2")),
    ];
    let message = usage_message(resolve_send_target(&rows, "builder", None).unwrap_err());
    assert!(message.contains("--session-ref"), "got {message}");
    assert!(
        message.contains("u-1") && message.contains("u-2"),
        "got {message}"
    );
}

/// The same rule from the other side: naming umbrella `u-1` must never fall
/// through to the `builder` sitting in `u-2`.
#[test]
fn a_role_never_resolves_across_umbrellas() {
    let rows = vec![execution(
        "s-2",
        1,
        Some(&pk(BOB)),
        Some("builder"),
        Some("u-2"),
    )];
    let error = resolve_send_target(&rows, "builder", Some("u-1")).unwrap_err();
    assert!(matches!(error, CliError::NotFound(_)), "got {error:?}");
    let message = usage_message(error);
    assert!(
        message.contains("u-1") && message.contains("u-2"),
        "got {message}"
    );
}

#[test]
fn two_seats_holding_one_role_in_one_umbrella_are_an_error_listing_both() {
    let rows = vec![
        execution("s-1", 1, Some(&pk(ALICE)), Some("builder"), Some("u-1")),
        execution("s-2", 1, Some(&pk(BOB)), Some("builder"), Some("u-1")),
    ];
    let message = usage_message(resolve_send_target(&rows, "builder", Some("u-1")).unwrap_err());
    assert!(message.contains("matches 2 executions"), "got {message}");
    assert!(
        message.contains("3:s-1") && message.contains("3:s-2"),
        "got {message}"
    );
}

/// Several generations of one execution are not an ambiguity; several
/// executions are. This pins the difference.
#[test]
fn several_generations_of_one_seat_collapse_to_the_newest() {
    let rows = vec![
        execution("s-1", 1, Some(&pk(ALICE)), Some("builder"), Some("u-1")),
        execution("s-1", 4, Some(&pk(ALICE)), Some("builder"), Some("u-1")),
    ];
    let resolved = resolve_send_target(&rows, "builder", Some("u-1")).expect("resolves");
    assert_eq!(resolved.target.generation, 4);
}

#[test]
fn a_name_that_is_both_a_session_id_and_a_role_is_an_error_naming_both() {
    let rows = vec![
        execution("builder", 1, None, None, None),
        execution("s-2", 1, Some(&pk(BOB)), Some("builder"), Some("u-1")),
    ];
    let message = usage_message(resolve_send_target(&rows, "builder", Some("u-1")).unwrap_err());
    assert!(message.contains("both a session id"), "got {message}");
}

#[test]
fn an_unknown_name_is_not_found_and_says_all_three_readings() {
    let rows = vec![execution("s-1", 1, None, None, None)];
    let error = resolve_send_target(&rows, "nobody", None).unwrap_err();
    assert!(matches!(error, CliError::NotFound(_)), "got {error:?}");
    let message = usage_message(error);
    assert!(message.contains("cs-target key"), "got {message}");
    assert!(message.contains("role slug"), "got {message}");
}

// ── caller umbrella ──────────────────────────────────────────────────────────

#[test]
fn an_unseated_caller_has_no_umbrella() {
    let rows = vec![execution(
        "s-1",
        1,
        Some(&pk(ALICE)),
        Some("builder"),
        Some("u-1"),
    )];
    assert_eq!(caller_umbrella(&rows, &pk(BOB)).expect("ok"), None);
}

#[test]
fn a_seated_caller_scopes_role_lookup_to_its_own_umbrella() {
    let rows = vec![
        execution("s-1", 1, Some(&pk(ALICE)), Some("lead"), Some("u-1")),
        execution("s-2", 1, Some(&pk(BOB)), Some("builder"), Some("u-1")),
    ];
    assert_eq!(
        caller_umbrella(&rows, &pk(ALICE)).expect("ok"),
        Some("u-1".to_owned())
    );
}

#[test]
fn a_caller_seated_in_two_umbrellas_must_say_which() {
    let rows = vec![
        execution("s-1", 1, Some(&pk(ALICE)), Some("lead"), Some("u-1")),
        execution("s-2", 1, Some(&pk(ALICE)), Some("lead"), Some("u-2")),
    ];
    let message = usage_message(caller_umbrella(&rows, &pk(ALICE)).unwrap_err());
    assert!(message.contains("--session-ref"), "got {message}");
}

// ── liveness ─────────────────────────────────────────────────────────────────

#[test]
fn a_live_lease_beats_every_other_witness() {
    assert_eq!(
        derive_liveness(Some(CodingSessionLeaseState::Live), "idle", Some(0), 10_000),
        Liveness::Live
    );
}

#[test]
fn a_released_lease_reads_released() {
    assert_eq!(
        derive_liveness(Some(CodingSessionLeaseState::Released), "idle", Some(0), 10),
        Liveness::Released
    );
}

/// A stopped execution has no future, so reporting it as `quiet 3d` would
/// imply it might come back.
#[test]
fn a_stopped_execution_reads_released_even_with_no_lease() {
    assert_eq!(
        derive_liveness(None, "stopped", Some(0), 10),
        Liveness::Released
    );
}

#[test]
fn no_lease_reads_quiet_with_the_age_of_the_last_signed_item() {
    assert_eq!(
        derive_liveness(None, "idle", Some(9_820), 10_000),
        Liveness::Quiet { age_secs: 180 }
    );
    assert_eq!(
        derive_liveness(None, "idle", Some(9_820), 10_000).render(),
        "quiet 3m"
    );
}

#[test]
fn an_execution_that_never_signed_anything_reads_unknown() {
    assert_eq!(
        derive_liveness(None, "idle", None, 10_000),
        Liveness::Unknown
    );
}

#[test]
fn age_renders_one_unit() {
    assert_eq!(format_age(-5), "0s");
    assert_eq!(format_age(59), "59s");
    assert_eq!(format_age(180), "3m");
    assert_eq!(format_age(7_200), "2h");
    assert_eq!(format_age(172_800), "2d");
}

#[test]
fn a_seat_label_prefers_the_actor_and_role_over_the_runtime() {
    let seated = execution("s-1", 1, Some(&pk(ALICE)), Some("builder"), Some("u-1"));
    assert_eq!(seated.seat_label(), "11111111·builder");
    let human = execution("s-2", 1, None, None, None);
    assert_eq!(human.seat_label(), "claude·claude-opus");
}

#[test]
fn the_lease_snapshot_folds_by_signer_and_target() {
    let target = target("s-1", 2);
    let leases = decode_leases(&[lease_event(&target, CodingSessionLeaseState::Live)]);
    assert_eq!(
        leases.get(&(pk(PROVIDER), coding_session_target_key(&target))),
        Some(&CodingSessionLeaseState::Live)
    );
}

#[test]
fn build_executions_carries_the_seat_the_umbrella_and_the_lease() {
    let target = target("s-1", 2);
    let events = vec![
        metadata_event(
            "m-1",
            1_000,
            &target,
            SessionStatus::Idle,
            Some(&pk(ALICE)),
            Some("builder"),
            Some("u-1"),
        ),
        transcript_event(
            "t-1",
            1_100,
            &target,
            7,
            Some("turn-1"),
            json!({"kind": "assistant_text"}),
        ),
    ];
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let (transcripts, _) = decode_transcripts(&events);
    let leases = decode_leases(&[lease_event(&target, CodingSessionLeaseState::Live)]);
    let rows = build_executions(&metadata, &receipts, &transcripts, &leases, 2_000);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].actor, Some(pk(ALICE)));
    assert_eq!(rows[0].role, Some("builder".to_owned()));
    assert_eq!(rows[0].session_ref, Some("u-1".to_owned()));
    assert_eq!(rows[0].last_signed_seq, Some(7));
    assert_eq!(rows[0].liveness, Liveness::Live);
}

// ── turn budget (plan D9 / contract B) ──────────────────────────────────────

/// `bee sessions status` reads the budget off the newest metadata's typed
/// `turnBudget` field, beside the `sessionRef` a provider only ever publishes
/// it with.
#[test]
fn build_executions_reads_the_turn_budget_off_the_newest_metadata() {
    let target = target("s-1", 1);
    let event = with_turn_budget(
        metadata_event(
            "m-1",
            1_000,
            &target,
            SessionStatus::Idle,
            None,
            None,
            Some("u-1"),
        ),
        json!({"used": 3, "limit": 10}),
    );
    let (metadata, _) = decode_metadata(&[event]);
    let rows = build_executions(&metadata, &[], &[], &HashMap::new(), 2_000);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].turn_budget, Some(TurnBudget { used: 3, limit: 10 }));
}

/// No `turnBudget` key at all — either this provider predates the budget, or
/// the umbrella has none configured — reads as `None`, never a guessed zero.
#[test]
fn build_executions_reads_no_budget_when_the_key_is_absent() {
    let target = target("s-1", 1);
    let event = metadata_event(
        "m-1",
        1_000,
        &target,
        SessionStatus::Idle,
        None,
        None,
        Some("u-1"),
    );
    let (metadata, _) = decode_metadata(&[event]);
    let rows = build_executions(&metadata, &[], &[], &HashMap::new(), 2_000);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].turn_budget, None);
}

/// A `turnBudget` missing a required half (here, `limit`) drops the whole
/// metadata record and is counted malformed — it does not read as a row with
/// an absent budget.
///
/// This is the strict reading, and it is the only one that can be true here:
/// `turnBudget` is a typed `SessionMetadata` field, so a half-written one
/// fails the same decode any other malformed known field fails, and
/// `buzz-core`'s `decode_coding_session_metadata` rejects the identical bytes
/// on the relay side. Reporting the rest of a record whose signer wrote a
/// number this command cannot read would be guessing about the very fact the
/// row exists to disclose.
#[test]
fn decode_metadata_drops_a_record_whose_turn_budget_is_missing_a_key() {
    let target = target("s-1", 1);
    let event = with_turn_budget(
        metadata_event(
            "m-1",
            1_000,
            &target,
            SessionStatus::Idle,
            None,
            None,
            Some("u-1"),
        ),
        json!({"used": 3}),
    );
    let (metadata, stats) = decode_metadata(&[event]);
    assert!(metadata.is_empty());
    assert_eq!(stats.malformed, 1);
    let rows = build_executions(&metadata, &[], &[], &HashMap::new(), 2_000);
    assert!(rows.is_empty());
}

/// The newest metadata wins, exactly as it does for every other fact this
/// command folds — an older row's budget must never resurface over a newer
/// one that has since advanced (or cleared) it.
#[test]
fn build_executions_takes_the_turn_budget_from_the_newest_metadata_row() {
    let target = target("s-1", 1);
    let older = with_turn_budget(
        metadata_event(
            "m-1",
            1_000,
            &target,
            SessionStatus::Idle,
            None,
            None,
            Some("u-1"),
        ),
        json!({"used": 1, "limit": 10}),
    );
    let newer = with_turn_budget(
        metadata_event(
            "m-2",
            2_000,
            &target,
            SessionStatus::Idle,
            None,
            None,
            Some("u-1"),
        ),
        json!({"used": 9, "limit": 10}),
    );
    let (metadata, _) = decode_metadata(&[older, newer]);
    let rows = build_executions(&metadata, &[], &[], &HashMap::new(), 3_000);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].turn_budget, Some(TurnBudget { used: 9, limit: 10 }));
}

/// The budget belongs to the *umbrella*, not to whichever seat last echoed
/// it: every row sharing a `sessionRef` reports the highest `used` any of
/// them has published.
///
/// The provider publishes `turnBudget` on the acting execution's own metadata
/// only, so an idle sibling's newest row is frozen at the count it saw when it
/// last spoke. Printing that stale number under copy that calls it the crew
/// session's budget would tell an operator there is room at the very moment
/// the next agent turn is refused.
#[test]
fn build_executions_reports_the_umbrella_budget_on_every_seat_sharing_it() {
    let busy = target("s-1", 1);
    let idle = target("s-2", 1);
    let events = vec![
        with_turn_budget(
            metadata_event(
                "m-1",
                1_000,
                &busy,
                SessionStatus::Idle,
                None,
                None,
                Some("u-1"),
            ),
            json!({"used": 9, "limit": 20}),
        ),
        with_turn_budget(
            metadata_event(
                "m-2",
                1_100,
                &idle,
                SessionStatus::Idle,
                None,
                None,
                Some("u-1"),
            ),
            json!({"used": 2, "limit": 20}),
        ),
    ];
    let (metadata, _) = decode_metadata(&events);
    let rows = build_executions(&metadata, &[], &[], &HashMap::new(), 2_000);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(
            row.turn_budget,
            Some(TurnBudget { used: 9, limit: 20 }),
            "row {} reported a stale umbrella budget",
            row.target_key
        );
    }
}

/// A seat that has never echoed a budget still belongs to the umbrella, so it
/// reports the umbrella's — silence about a ceiling that exists is the same
/// lie as a stale count.
#[test]
fn build_executions_lends_the_umbrella_budget_to_a_seat_that_never_echoed_one() {
    let busy = target("s-1", 1);
    let quiet = target("s-2", 1);
    let events = vec![
        with_turn_budget(
            metadata_event(
                "m-1",
                1_000,
                &busy,
                SessionStatus::Idle,
                None,
                None,
                Some("u-1"),
            ),
            json!({"used": 4, "limit": 20}),
        ),
        metadata_event(
            "m-2",
            1_100,
            &quiet,
            SessionStatus::Idle,
            None,
            None,
            Some("u-1"),
        ),
    ];
    let (metadata, _) = decode_metadata(&events);
    let rows = build_executions(&metadata, &[], &[], &HashMap::new(), 2_000);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row.turn_budget, Some(TurnBudget { used: 4, limit: 20 }));
    }
}

/// Two umbrellas never pool their counts, and an execution that claimed no
/// umbrella keeps only what its own metadata said.
#[test]
fn build_executions_never_pools_budgets_across_umbrellas() {
    let mine = target("s-1", 1);
    let theirs = target("s-2", 1);
    let loose = target("s-3", 1);
    let events = vec![
        with_turn_budget(
            metadata_event(
                "m-1",
                1_000,
                &mine,
                SessionStatus::Idle,
                None,
                None,
                Some("u-1"),
            ),
            json!({"used": 4, "limit": 20}),
        ),
        with_turn_budget(
            metadata_event(
                "m-2",
                1_000,
                &theirs,
                SessionStatus::Idle,
                None,
                None,
                Some("u-2"),
            ),
            json!({"used": 17, "limit": 20}),
        ),
        metadata_event("m-3", 1_000, &loose, SessionStatus::Idle, None, None, None),
    ];
    let (metadata, _) = decode_metadata(&events);
    let rows = build_executions(&metadata, &[], &[], &HashMap::new(), 2_000);
    assert_eq!(rows.len(), 3);
    let budget_of = |session_id: &str| {
        rows.iter()
            .find(|row| row.target.session_id == session_id)
            .expect("row")
            .turn_budget
    };
    assert_eq!(budget_of("s-1"), Some(TurnBudget { used: 4, limit: 20 }));
    assert_eq!(
        budget_of("s-2"),
        Some(TurnBudget {
            used: 17,
            limit: 20
        })
    );
    assert_eq!(budget_of("s-3"), None);
}

#[test]
fn turn_budget_exhausted_is_used_at_or_past_limit() {
    assert!(!TurnBudget { used: 9, limit: 10 }.exhausted());
    assert!(TurnBudget {
        used: 10,
        limit: 10
    }
    .exhausted());
    assert!(TurnBudget {
        used: 11,
        limit: 10
    }
    .exhausted());
}

// ── receipt stages ───────────────────────────────────────────────────────────

/// `created_at` is second-granularity and a provider legitimately publishes
/// `turn_queued` and `turn_started` inside one second; picking by event id
/// would report a running turn as merely queued about half the time.
#[test]
fn a_queued_and_started_receipt_in_one_second_resolve_to_started() {
    let target = target("s-1", 1);
    let events = vec![
        receipt_event(
            "r-z",
            1_000,
            "cmd-1",
            ReceiptStatus::TurnStarted,
            &target,
            None,
            Some("turn-9"),
        ),
        receipt_event(
            "r-a",
            1_000,
            "cmd-1",
            ReceiptStatus::TurnQueued,
            &target,
            None,
            None,
        ),
    ];
    let (receipts, _) = decode_receipts(&events);
    let stages = newest_turn_stages(&receipts);
    assert_eq!(stages["cmd-1"].status, ReceiptStatus::TurnStarted);
    assert_eq!(stages["cmd-1"].turn_id.as_deref(), Some("turn-9"));
}

#[test]
fn a_lifecycle_receipt_is_not_a_turn_stage() {
    let target = target("s-1", 1);
    let events = vec![receipt_event(
        "r-1",
        1_000,
        "create-1",
        ReceiptStatus::Created,
        &target,
        None,
        None,
    )];
    let (receipts, _) = decode_receipts(&events);
    assert!(newest_turn_stages(&receipts).is_empty());
}

// ── inbox ────────────────────────────────────────────────────────────────────

fn inbox_fixture() -> (
    Vec<TurnCommand>,
    HashMap<String, TurnStage>,
    HashSet<String>,
) {
    let mine = target("s-1", 1);
    let theirs = target("s-2", 1);
    let events = vec![
        turn_event("e-2", BOB, 2_000, "cmd-2", &mine, "second"),
        turn_event("e-1", ALICE, 1_000, "cmd-1", &mine, "first"),
        turn_event("e-3", ALICE, 3_000, "cmd-3", &theirs, "not mine"),
        receipt_event(
            "r-1",
            1_001,
            "cmd-1",
            ReceiptStatus::TurnQueued,
            &mine,
            None,
            None,
        ),
        receipt_event(
            "r-2",
            1_002,
            "cmd-1",
            ReceiptStatus::TurnStarted,
            &mine,
            None,
            Some("turn-1"),
        ),
    ];
    let (commands, _) = decode_turn_commands(&events);
    let (receipts, _) = decode_receipts(&events);
    let stages = newest_turn_stages(&receipts);
    let mut owned = HashSet::new();
    owned.insert(coding_session_target_key(&mine));
    (commands, stages, owned)
}

#[test]
fn the_inbox_is_oldest_first_and_only_carries_turns_addressed_to_my_seats() {
    let (commands, stages, mine) = inbox_fixture();
    let rows = build_inbox(&commands, &stages, &mine, None).expect("inbox");
    assert_eq!(
        rows.iter()
            .map(|row| row.command_id.as_str())
            .collect::<Vec<_>>(),
        vec!["cmd-1", "cmd-2"]
    );
    assert_eq!(
        rows[0].stage.as_ref().map(|stage| stage.status),
        Some(ReceiptStatus::TurnStarted)
    );
    assert_eq!(rows[1].stage, None);
}

#[test]
fn the_since_cursor_is_exclusive() {
    let (commands, stages, mine) = inbox_fixture();
    let rows = build_inbox(&commands, &stages, &mine, Some("e-1")).expect("inbox");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].command_id, "cmd-2");
}

/// A cursor naming a turn addressed to somebody else must not silently
/// restart the inbox from the beginning.
#[test]
fn a_since_cursor_that_names_no_row_of_mine_is_refused() {
    let (commands, stages, mine) = inbox_fixture();
    let error = build_inbox(&commands, &stages, &mine, Some("e-3")).unwrap_err();
    assert!(matches!(error, CliError::NotFound(_)), "got {error:?}");
}

// ── turn load ────────────────────────────────────────────────────────────────

#[test]
fn a_started_turn_with_a_result_item_is_not_open() {
    let target = target("s-1", 1);
    let events = vec![
        turn_event("e-1", ALICE, 1_000, "cmd-1", &target, "go"),
        receipt_event(
            "r-1",
            1_001,
            "cmd-1",
            ReceiptStatus::TurnStarted,
            &target,
            None,
            Some("turn-1"),
        ),
        transcript_event(
            "t-1",
            1_050,
            &target,
            1,
            Some("turn-1"),
            json!({"kind": "result", "subtype": "success"}),
        ),
    ];
    let (commands, _) = decode_turn_commands(&events);
    let (receipts, _) = decode_receipts(&events);
    let (transcripts, _) = decode_transcripts(&events);
    let row = execution("s-1", 1, None, None, None);
    let load = turn_load(
        &row,
        &commands,
        &newest_turn_stages(&receipts),
        &transcripts,
    );
    assert_eq!(load.open_command_id, None);
    assert_eq!(load.queued, 0);
}

#[test]
fn a_started_turn_with_no_terminal_item_is_the_open_turn_and_queued_turns_are_counted() {
    let target = target("s-1", 1);
    let events = vec![
        turn_event("e-1", ALICE, 1_000, "cmd-1", &target, "go"),
        turn_event("e-2", BOB, 1_010, "cmd-2", &target, "then this"),
        receipt_event(
            "r-1",
            1_001,
            "cmd-1",
            ReceiptStatus::TurnStarted,
            &target,
            None,
            Some("turn-1"),
        ),
        receipt_event(
            "r-2",
            1_011,
            "cmd-2",
            ReceiptStatus::TurnQueued,
            &target,
            None,
            None,
        ),
        transcript_event(
            "t-1",
            1_020,
            &target,
            1,
            Some("turn-1"),
            json!({"kind": "assistant_text"}),
        ),
    ];
    let (commands, _) = decode_turn_commands(&events);
    let (receipts, _) = decode_receipts(&events);
    let (transcripts, _) = decode_transcripts(&events);
    let row = execution("s-1", 1, None, None, None);
    let load = turn_load(
        &row,
        &commands,
        &newest_turn_stages(&receipts),
        &transcripts,
    );
    assert_eq!(load.open_command_id.as_deref(), Some("cmd-1"));
    assert_eq!(load.open_turn_id.as_deref(), Some("turn-1"));
    assert_eq!(load.queued, 1);
}

// ── re-addressing (ruling R1) ────────────────────────────────────────────────

struct ReaddressFixture {
    commands: Vec<TurnCommand>,
    stages: HashMap<String, TurnStage>,
    resumes: Vec<ResumeRecord>,
}

fn owed_turn(status: ReceiptStatus, code: &str, generation: u64) -> ReaddressFixture {
    let addressed = target("s-1", generation);
    let events = vec![
        turn_event(
            "e-1",
            ALICE,
            1_000,
            "cmd-1",
            &addressed,
            "rebase and re-run the gate",
        ),
        receipt_event(
            "r-1",
            1_005,
            "cmd-1",
            status,
            &addressed,
            Some((code, "did not run")),
            None,
        ),
    ];
    let (commands, _) = decode_turn_commands(&events);
    let (receipts, _) = decode_receipts(&events);
    ReaddressFixture {
        commands,
        stages: newest_turn_stages(&receipts),
        resumes: vec![ResumeRecord {
            signer: pk(BOB),
            created_at: 1_500,
            previous: addressed,
        }],
    }
}

#[test]
fn a_dropped_turn_re_addresses_to_the_resumed_generation_and_names_who_resumed_it() {
    let fixture = owed_turn(ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION, 1);
    let mut resumed = execution("s-1", 2, Some(&pk(ALICE)), Some("builder"), Some("u-1"));
    resumed.liveness = Liveness::Live;
    let rows = vec![execution("s-1", 1, None, None, None), resumed];
    let plan = plan_readdress(
        &fixture.commands,
        &fixture.stages,
        &rows,
        &fixture.resumes,
        "cmd-1",
    )
    .expect("plan");
    assert_eq!(plan.target.generation, 2);
    assert_eq!(plan.text, "rebase and re-run the gate");
    assert_eq!(plan.refused_code, NO_LIVE_EXECUTION);
    assert_eq!(plan.resumed_by, Some(pk(BOB)));
}

#[test]
fn a_stale_generation_refusal_re_addresses_to_the_newest_generation() {
    let fixture = owed_turn(ReceiptStatus::TurnRefused, STALE_GENERATION, 1);
    let mut newest = execution("s-1", 3, None, None, None);
    newest.liveness = Liveness::Live;
    let rows = vec![execution("s-1", 1, None, None, None), newest];
    let plan =
        plan_readdress(&fixture.commands, &fixture.stages, &rows, &[], "cmd-1").expect("plan");
    assert_eq!(plan.target.generation, 3);
    assert_eq!(plan.resumed_by, None);
}

/// The honest answer to "which generation": when nobody resumed the
/// execution, re-sending into the same dead generation only earns a second
/// `NO_LIVE_EXECUTION`, so the caller is told to resume first.
#[test]
fn re_addressing_into_the_same_dead_generation_is_refused() {
    let fixture = owed_turn(ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION, 1);
    let mut same = execution("s-1", 1, None, None, None);
    same.liveness = Liveness::Quiet { age_secs: 400 };
    let message = usage_message(
        plan_readdress(&fixture.commands, &fixture.stages, &[same], &[], "cmd-1").unwrap_err(),
    );
    assert!(message.contains("resume it"), "got {message}");
    assert!(message.contains(NO_LIVE_EXECUTION), "got {message}");
}

/// …but a provider that came back on the *same* generation can be addressed
/// again, because its lease says an actor is behind it.
#[test]
fn re_addressing_the_same_generation_is_allowed_once_a_live_lease_answers() {
    let fixture = owed_turn(ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION, 1);
    let mut same = execution("s-1", 1, None, None, None);
    same.liveness = Liveness::Live;
    let plan =
        plan_readdress(&fixture.commands, &fixture.stages, &[same], &[], "cmd-1").expect("plan");
    assert_eq!(plan.target.generation, 1);
}

/// And if the session is closed: refused outright, naming the status.
#[test]
fn re_addressing_a_stopped_execution_is_refused() {
    let fixture = owed_turn(ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION, 1);
    let mut stopped = execution("s-1", 2, None, None, None);
    stopped.status = "stopped".into();
    stopped.liveness = Liveness::Released;
    let message = usage_message(
        plan_readdress(&fixture.commands, &fixture.stages, &[stopped], &[], "cmd-1").unwrap_err(),
    );
    assert!(message.contains("durably stopped"), "got {message}");
}

#[test]
fn a_turn_that_actually_started_cannot_be_re_addressed() {
    let addressed = target("s-1", 1);
    let events = vec![
        turn_event("e-1", ALICE, 1_000, "cmd-1", &addressed, "go"),
        receipt_event(
            "r-1",
            1_005,
            "cmd-1",
            ReceiptStatus::TurnStarted,
            &addressed,
            None,
            Some("turn-1"),
        ),
    ];
    let (commands, _) = decode_turn_commands(&events);
    let (receipts, _) = decode_receipts(&events);
    let message = usage_message(
        plan_readdress(
            &commands,
            &newest_turn_stages(&receipts),
            &[execution("s-1", 1, None, None, None)],
            &[],
            "cmd-1",
        )
        .unwrap_err(),
    );
    assert!(message.contains("turn_started"), "got {message}");
}

#[test]
fn re_addressing_an_unknown_command_id_is_not_found() {
    let fixture = owed_turn(ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION, 1);
    let error = plan_readdress(
        &fixture.commands,
        &fixture.stages,
        &[execution("s-1", 2, None, None, None)],
        &[],
        "cmd-missing",
    )
    .unwrap_err();
    assert!(matches!(error, CliError::NotFound(_)), "got {error:?}");
}

/// `doctor`'s recovery advice and `--readdress`'s own ruling are one rule.
///
/// [`readdressable_reason`] names the only two answers a re-send recovers — a
/// `turn_dropped`/`NO_LIVE_EXECUTION` or a `turn_refused`/`STALE_GENERATION`.
/// Every other terminal answer is final: `QUEUE_FULL` means the queue was full
/// then and says nothing about now, and `NO_TURN_IN_FLIGHT` answers a cancel
/// that had nothing to cancel. A `doctor` that prints "re-address it" over
/// those sends the reader straight into `plan_readdress`'s refusal — the
/// command telling them to run something it will not run. So the advice is
/// asserted here against the verb that has to honour it, on one fixture.
#[test]
fn doctor_only_offers_readdress_for_the_answers_readdress_accepts() {
    let addressed = target("s-1", 1);
    for (status, code) in [
        (ReceiptStatus::TurnDropped, QUEUE_FULL),
        (ReceiptStatus::TurnRefused, NO_TURN_IN_FLIGHT),
    ] {
        let events = vec![
            turn_event("e-1", ALICE, 1_000, "cmd-1", &addressed, "go"),
            receipt_event(
                "r-1",
                1_005,
                "cmd-1",
                status,
                &addressed,
                Some((code, "answered")),
                None,
            ),
        ];
        let (commands, _) = decode_turn_commands(&events);
        let (receipts, _) = decode_receipts(&events);
        let stages = newest_turn_stages(&receipts);

        // Both readings of the same refusal: the command that reached a turn
        // (its prompt echo is on the transcript) and the one that never did.
        let echoed = vec![transcript_event(
            "t-1",
            1_001,
            &addressed,
            1,
            Some("turn-1"),
            json!({ "kind": "user_prompt", "content": "go", "commandId": "cmd-1" }),
        )];
        let (with_turn, _) = decode_transcripts(&echoed);
        for records in [with_turn, Vec::new()] {
            let turns = diagnose_turns(&records, &commands, &stages);
            assert_eq!(turns.len(), 1, "{code}: {turns:?}");
            let turn = &turns[0];
            // The answer and its code are still reported — silence would be
            // its own lie.
            assert_eq!(turn.answered_stage.as_deref(), Some(status.as_str()));
            assert_eq!(turn.answered_code.as_deref(), Some(code));
            assert!(
                !turn
                    .findings
                    .iter()
                    .any(|finding| finding.contains("readdress")),
                "{code} was advertised as re-addressable: {:?}",
                turn.findings
            );
        }

        // …and the verb `doctor` would have named refuses this exact command.
        let mut live = execution("s-1", 1, None, None, None);
        live.liveness = Liveness::Live;
        let message =
            usage_message(plan_readdress(&commands, &stages, &[live], &[], "cmd-1").unwrap_err());
        assert!(message.contains(code), "got {message}");
    }
}

/// The pinned other side: the two codes that *are* recoverable keep the advice,
/// naming the commandId `--readdress` takes — on a `thread.turn.start`, the
/// only action carrying text to re-send.
#[test]
fn doctor_still_names_readdress_for_a_recoverable_refusal() {
    let addressed = target("s-1", 1);
    for (status, code) in [
        (ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION),
        (ReceiptStatus::TurnRefused, STALE_GENERATION),
    ] {
        let events = vec![
            turn_event("e-1", ALICE, 1_000, "cmd-1", &addressed, "go"),
            receipt_event(
                "r-1",
                1_005,
                "cmd-1",
                status,
                &addressed,
                Some((code, "did not run")),
                None,
            ),
        ];
        let (commands, _) = decode_turn_commands(&events);
        let (receipts, _) = decode_receipts(&events);
        let turns = diagnose_turns(&[], &commands, &newest_turn_stages(&receipts));
        assert_eq!(turns.len(), 1, "{code}");
        assert!(
            turns[0]
                .findings
                .iter()
                .any(|finding| finding.contains("--readdress cmd-1")),
            "{code} lost the verb that recovers it: {:?}",
            turns[0].findings
        );
    }
}

/// A refused *interrupt* is answered exactly like a refused turn, and only the
/// 44220 tells them apart.
///
/// The provider refuses every turn command aimed at a superseded generation
/// with `turn_refused`/`STALE_GENERATION`, whatever its action — so the receipt
/// alone cannot say whether the sender asked for a turn or asked to cancel one.
/// A `thread.turn.interrupt` carries no text, and `plan_readdress` refuses it
/// for exactly that reason. `doctor` therefore has to read the command before
/// it names the verb: the answer and its code are still reported, the recovery
/// advice is not.
#[test]
fn doctor_does_not_offer_readdress_for_a_refused_interrupt() {
    let addressed = target("s-1", 1);
    let events = vec![
        command_event(
            "e-1",
            ALICE,
            1_000,
            "cmd-1",
            &addressed,
            CodingSessionAction::ThreadTurnInterrupt,
        ),
        receipt_event(
            "r-1",
            1_005,
            "cmd-1",
            ReceiptStatus::TurnRefused,
            &addressed,
            Some((STALE_GENERATION, "generation 1 is superseded")),
            None,
        ),
    ];
    let (commands, _) = decode_turn_commands(&events);
    let (receipts, _) = decode_receipts(&events);
    let stages = newest_turn_stages(&receipts);

    let turns = diagnose_turns(&[], &commands, &stages);
    assert_eq!(turns.len(), 1, "{turns:?}");
    let turn = &turns[0];
    // Silence would be its own lie: the refusal is still on the report.
    assert_eq!(
        turn.answered_stage.as_deref(),
        Some(ReceiptStatus::TurnRefused.as_str())
    );
    assert_eq!(turn.answered_code.as_deref(), Some(STALE_GENERATION));
    assert!(
        !turn
            .findings
            .iter()
            .any(|finding| finding.contains("readdress")),
        "a refused interrupt was advertised as re-addressable: {:?}",
        turn.findings
    );

    // …and the verb `doctor` would have named refuses this exact command.
    let mut live = execution("s-1", 2, None, None, None);
    live.liveness = Liveness::Live;
    let message =
        usage_message(plan_readdress(&commands, &stages, &[live], &[], "cmd-1").unwrap_err());
    assert!(
        message.contains("carries no text to re-address"),
        "got {message}"
    );
}

/// The same rule for a command that is not in view at all.
///
/// A `commandId` with a refusal receipt but no 44220 in the channel is a
/// command `doctor` has never read — it cannot say the action was a turn start,
/// and `plan_readdress` will answer `no coding-session command with commandId`.
/// Guessing "re-address it" over that is the same lie as guessing over an
/// interrupt.
#[test]
fn doctor_does_not_offer_readdress_for_a_command_it_never_read() {
    let addressed = target("s-1", 1);
    let events = vec![receipt_event(
        "r-1",
        1_005,
        "cmd-1",
        ReceiptStatus::TurnDropped,
        &addressed,
        Some((NO_LIVE_EXECUTION, "did not run")),
        None,
    )];
    let (receipts, _) = decode_receipts(&events);
    let turns = diagnose_turns(&[], &[], &newest_turn_stages(&receipts));
    assert_eq!(turns.len(), 1, "{turns:?}");
    assert_eq!(turns[0].answered_code.as_deref(), Some(NO_LIVE_EXECUTION));
    assert!(
        !turns[0]
            .findings
            .iter()
            .any(|finding| finding.contains("readdress")),
        "an unread command was advertised as re-addressable: {:?}",
        turns[0].findings
    );
}

// ── Founders ─────────────────────────────────────────────────────────────────

/// The genesis event that founds an umbrella. Its **id** is the canonical
/// reference; the `csg-session` tag exists for the relay's uniqueness probe
/// and is never a founder lookup (NIP-CSG).
fn genesis_event(id: &str, signer: &str, session_ref: &str) -> Value {
    let payload = buzz_core::coding_session_genesis::CodingSessionGenesisPayload::new(session_ref);
    json!({
        "id": id,
        "pubkey": pk(signer),
        "kind": buzz_core::kind::KIND_CODING_SESSION_GENESIS,
        "created_at": 500,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["csg-v", "csg1-1"], ["csg-session", session_ref]],
        "content": serde_json::to_string(&payload).expect("serialize"),
    })
}

#[allow(clippy::too_many_arguments)]
fn create_event(
    id: &str,
    signer: &str,
    created_at: i64,
    command_id: &str,
    session_ref: Option<&str>,
    genesis_ref: Option<&str>,
    provider_authority: &str,
) -> Value {
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: session_ref.map(str::to_owned),
            genesis_ref: genesis_ref.map(str::to_owned),
            provider_instance_ref: "instance-1".try_into().expect("alias"),
            provider_authority_pubkey: pk(provider_authority),
            model: Some("claude-opus".into()),
            title: Some("a session".into()),
            initial_turn: None,
            actor: None,
            role: None,
            hire_ref: None,
            routing: None,
        },
    };
    json!({
        "id": id,
        "pubkey": pk(signer),
        "kind": KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["csl-v", "csl1-1"]],
        "content": serde_json::to_string(&payload).expect("serialize"),
    })
}

fn resume_event(
    id: &str,
    signer: &str,
    created_at: i64,
    command_id: &str,
    previous: &CodingSessionTarget,
    provider_authority: &str,
) -> Value {
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        action: CodingSessionLifecycleAction::SessionResume {
            session: previous.clone(),
            provider_authority_pubkey: pk(provider_authority),
        },
    };
    json!({
        "id": id,
        "pubkey": pk(signer),
        "kind": KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["csl-v", "csl1-1"]],
        "content": serde_json::to_string(&payload).expect("serialize"),
    })
}

/// A lifecycle receipt signed by a named provider, so the self-fence — only
/// the provider the create *named* may answer it — can be exercised.
fn receipt_event_signed_by(
    id: &str,
    signer: &str,
    created_at: i64,
    command_id: &str,
    status: ReceiptStatus,
    target: Option<&CodingSessionTarget>,
) -> Value {
    let receipt = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        status,
        session: target.cloned(),
        error: None,
        turn_id: None,
    };
    json!({
        "id": id,
        "pubkey": pk(signer),
        "kind": KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["cslr-v", "cslr1-1"]],
        "content": serde_json::to_string(&receipt).expect("serialize"),
    })
}

fn index_of(events: &[Value]) -> FounderIndex {
    let (receipts, _) = decode_receipts(events);
    build_founder_index(events, &receipts)
}

const UMBRELLA_ONE: &str = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
const UMBRELLA_TWO: &str = "6ba7b811-9dad-11d1-80b4-00c04fd430c8";
const GENESIS_ONE: &str = "1a";
const GENESIS_TWO: &str = "2a";

/// The provider signs every execution, so a founder read off the provider's
/// key would make every session look like it belonged to the same person.
/// The founder is the genesis signer, reached through the receipt-joined
/// create's explicit `genesisRef`.
#[test]
fn founder_is_the_genesis_signer_not_the_provider() {
    let target = target("s-1", 1);
    let events = vec![
        genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_ONE),
        create_event(
            "c1",
            ALICE,
            600,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        receipt_event_signed_by(
            "r1",
            PROVIDER,
            601,
            "cmd-1",
            ReceiptStatus::Created,
            Some(&target),
        ),
    ];
    let founding = index_of(&events).of(&target);
    assert_eq!(founding.founder.as_deref(), Some(pk(ALICE).as_str()));
    assert_eq!(founding.create_signer.as_deref(), Some(pk(ALICE).as_str()));
    assert_ne!(founding.founder.as_deref(), Some(pk(PROVIDER).as_str()));
}

/// A create's `genesisRef` names an event id. When that event is not in the
/// channel, the founder is `null` — never the create signer standing in for
/// it, and never the provider.
#[test]
fn founder_is_null_when_the_genesis_event_is_absent_from_the_channel() {
    let target = target("s-1", 1);
    let events = vec![
        create_event(
            "c1",
            ALICE,
            600,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        receipt_event_signed_by(
            "r1",
            PROVIDER,
            601,
            "cmd-1",
            ReceiptStatus::Created,
            Some(&target),
        ),
    ];
    let founding = index_of(&events).of(&target);
    assert_eq!(founding.founder, None);
    assert_eq!(founding.create_signer.as_deref(), Some(pk(ALICE).as_str()));
    assert!(index_of(&events).founders().is_empty());
}

/// A resume mints a new generation but founds nothing. Whoever signed it must
/// never appear as the founder or the create signer of the execution they
/// resumed — the founding create's signer carries forward instead.
#[test]
fn a_resume_signer_is_never_reported_as_a_founder() {
    let first = target("s-1", 1);
    let second = target("s-1", 2);
    let events = vec![
        genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_ONE),
        create_event(
            "c1",
            ALICE,
            600,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        receipt_event_signed_by(
            "r1",
            PROVIDER,
            601,
            "cmd-1",
            ReceiptStatus::Created,
            Some(&first),
        ),
        resume_event("c2", BOB, 700, "cmd-2", &first, PROVIDER),
        receipt_event_signed_by(
            "r2",
            PROVIDER,
            701,
            "cmd-2",
            ReceiptStatus::Resumed,
            Some(&second),
        ),
    ];
    let index = index_of(&events);
    for generation in [&first, &second] {
        let founding = index.of(generation);
        assert_eq!(founding.founder.as_deref(), Some(pk(ALICE).as_str()));
        assert_eq!(founding.create_signer.as_deref(), Some(pk(ALICE).as_str()));
        assert_ne!(founding.founder.as_deref(), Some(pk(BOB).as_str()));
        assert_ne!(founding.create_signer.as_deref(), Some(pk(BOB).as_str()));
    }
    assert_eq!(index.founders(), vec![pk(ALICE)]);
}

/// A create nobody's provider acted on mints no execution, so it is not
/// evidence of founding one: the join runs through the receipt's `commandId`
/// and the target that receipt named.
#[test]
fn create_signer_joins_through_the_receipt_command_id() {
    let target = target("s-1", 1);
    let unjoined = create_event(
        "c9",
        BOB,
        550,
        "cmd-never-answered",
        Some(UMBRELLA_TWO),
        Some(&pk(GENESIS_TWO)),
        PROVIDER,
    );
    let joined = create_event(
        "c1",
        ALICE,
        600,
        "cmd-1",
        Some(UMBRELLA_ONE),
        Some(&pk(GENESIS_ONE)),
        PROVIDER,
    );
    let receipt = receipt_event_signed_by(
        "r1",
        PROVIDER,
        601,
        "cmd-1",
        ReceiptStatus::Created,
        Some(&target),
    );
    let events = vec![
        genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_ONE),
        genesis_event(&pk(GENESIS_TWO), BOB, UMBRELLA_TWO),
        unjoined,
        joined,
        receipt,
    ];
    let index = index_of(&events);
    assert_eq!(
        index.of(&target).create_signer.as_deref(),
        Some(pk(ALICE).as_str())
    );
    // The unanswered create founds nothing at all — not this execution, and
    // not the channel's founder list.
    assert_eq!(index.founders(), vec![pk(ALICE)]);
}

/// Only the provider the create *named* may answer it. A receipt from any
/// other signer joins nothing, so a stranger cannot mint a foreign founder.
#[test]
fn a_receipt_from_an_unnamed_provider_joins_nothing() {
    let target = target("s-1", 1);
    let events = vec![
        genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_ONE),
        create_event(
            "c1",
            ALICE,
            600,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        receipt_event_signed_by(
            "r1",
            BOB,
            601,
            "cmd-1",
            ReceiptStatus::Created,
            Some(&target),
        ),
    ];
    let founding = index_of(&events).of(&target);
    assert_eq!(founding, Founding::default());
}

/// A genesis founding a different umbrella than the create claims is not this
/// create's genesis, even when the create points straight at its id.
#[test]
fn a_genesis_for_another_umbrella_is_not_this_creates_founder() {
    let target = target("s-1", 1);
    let events = vec![
        genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_TWO),
        create_event(
            "c1",
            ALICE,
            600,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        receipt_event_signed_by(
            "r1",
            PROVIDER,
            601,
            "cmd-1",
            ReceiptStatus::Created,
            Some(&target),
        ),
    ];
    let founding = index_of(&events).of(&target);
    assert_eq!(founding.founder, None);
    assert_eq!(founding.create_signer.as_deref(), Some(pk(ALICE).as_str()));
}

/// One channel can hold two crews. Each execution reports the founder of the
/// umbrella its own create named, and the channel-level list holds both.
#[test]
fn two_umbrellas_in_one_channel_keep_their_own_founders() {
    let alice_target = target("s-1", 1);
    let bob_target = target("s-2", 1);
    let events = vec![
        genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_ONE),
        genesis_event(&pk(GENESIS_TWO), BOB, UMBRELLA_TWO),
        create_event(
            "c1",
            ALICE,
            600,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        create_event(
            "c2",
            BOB,
            610,
            "cmd-2",
            Some(UMBRELLA_TWO),
            Some(&pk(GENESIS_TWO)),
            PROVIDER,
        ),
        receipt_event_signed_by(
            "r1",
            PROVIDER,
            601,
            "cmd-1",
            ReceiptStatus::Created,
            Some(&alice_target),
        ),
        receipt_event_signed_by(
            "r2",
            PROVIDER,
            611,
            "cmd-2",
            ReceiptStatus::Created,
            Some(&bob_target),
        ),
    ];
    let index = index_of(&events);
    assert_eq!(
        index.of(&alice_target).founder.as_deref(),
        Some(pk(ALICE).as_str())
    );
    assert_eq!(
        index.of(&bob_target).founder.as_deref(),
        Some(pk(BOB).as_str())
    );
    let mut founders = index.founders();
    founders.sort();
    let mut expected = vec![pk(ALICE), pk(BOB)];
    expected.sort();
    assert_eq!(founders, expected);
}

/// Two creates sharing one `commandId` but disagreeing about who signed them
/// is a disputed claim; the discipline is to resolve nothing rather than pick
/// the earliest.
#[test]
fn a_disputed_command_id_founds_nothing() {
    let target = target("s-1", 1);
    let events = vec![
        genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_ONE),
        create_event(
            "c1",
            ALICE,
            600,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        create_event(
            "c2",
            BOB,
            601,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        receipt_event_signed_by(
            "r1",
            PROVIDER,
            602,
            "cmd-1",
            ReceiptStatus::Created,
            Some(&target),
        ),
    ];
    assert_eq!(index_of(&events).of(&target), Founding::default());
}

/// A turn receipt names the execution its 44220 addressed, but it never
/// creates one — so it can never join a create to a target.
#[test]
fn a_turn_receipt_never_joins_a_create() {
    let target = target("s-1", 1);
    let events = vec![
        genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_ONE),
        create_event(
            "c1",
            ALICE,
            600,
            "cmd-1",
            Some(UMBRELLA_ONE),
            Some(&pk(GENESIS_ONE)),
            PROVIDER,
        ),
        receipt_event_signed_by(
            "r1",
            PROVIDER,
            601,
            "cmd-1",
            ReceiptStatus::TurnQueued,
            Some(&target),
        ),
    ];
    assert_eq!(index_of(&events).of(&target), Founding::default());
}

// ── Delivery reporting ───────────────────────────────────────────────────────

fn stage(status: ReceiptStatus, error: Option<(&str, &str)>) -> TurnStage {
    TurnStage {
        status,
        error_code: error.map(|(code, _)| code.to_string()),
        error_message: error.map(|(_, message)| message.to_string()),
        turn_id: (status == ReceiptStatus::TurnStarted).then(|| "turn-7".to_string()),
        at: 1_000,
    }
}

/// Ledger 80 (c): `--deliver steer` printed `accepted:true` over a provider
/// that had degraded the steer to a boundary delivery, so the sender believed
/// a mid-turn injection had happened that had not. The relay's `accepted` is
/// about storage; this is about delivery, and it must say the unwelcome half.
#[test]
fn a_degraded_steer_reports_the_degradation_not_acceptance() {
    let report = fold_delivery(
        CodingSessionDelivery::Steer,
        Some(&stage(ReceiptStatus::TurnDegraded, None)),
        true,
    );
    assert_eq!(report.status, "turn_degraded");
    assert_eq!(report.delivered, Some(true), "the words are not lost");
    assert_eq!(
        report.detail,
        "steer requested, provider degraded to boundary"
    );
}

/// The three shapes a sender must be able to tell apart: it ran, it never
/// will, and nobody answered. `unknown` is its own answer.
#[test]
fn delivery_separates_ran_never_and_unanswered() {
    let started = fold_delivery(
        CodingSessionDelivery::Boundary,
        Some(&stage(ReceiptStatus::TurnStarted, None)),
        true,
    );
    assert_eq!(started.delivered, Some(true));
    assert_eq!(started.status, "turn_started");
    assert!(started.detail.contains("turn-7"), "{}", started.detail);

    let dropped = fold_delivery(
        CodingSessionDelivery::Boundary,
        Some(&stage(
            ReceiptStatus::TurnDropped,
            Some((NO_LIVE_EXECUTION, "no live execution")),
        )),
        true,
    );
    assert_eq!(dropped.delivered, Some(false));
    assert!(
        dropped.detail.contains("not delivered"),
        "{}",
        dropped.detail
    );
    assert!(
        dropped.detail.contains(NO_LIVE_EXECUTION),
        "{}",
        dropped.detail
    );

    let silent = fold_delivery(CodingSessionDelivery::Interrupt, None, true);
    assert_eq!(silent.delivered, None, "silence is never a delivery");
    assert_eq!(silent.status, DELIVERY_UNCONFIRMED);
    assert!(silent.detail.contains("unknown"), "{}", silent.detail);
    assert!(
        silent.detail.contains(&DELIVERY_WAIT_SECONDS.to_string()),
        "{}",
        silent.detail
    );
}

/// An interrupt that landed says the running turn was cancelled — the fact the
/// sender asked for, not merely that the command was stored.
#[test]
fn a_delivered_interrupt_says_the_turn_was_cancelled() {
    let report = fold_delivery(
        CodingSessionDelivery::Interrupt,
        Some(&stage(ReceiptStatus::InterruptDelivered, None)),
        true,
    );
    assert_eq!(report.delivered, Some(true));
    assert_eq!(report.status, "interrupt_delivered");
    assert!(report.detail.contains("cancelled"), "{}", report.detail);
}

/// `--no-wait` never says "nothing answered" — nothing was asked. Both are
/// `unknown`; only one of them is a fact about the provider.
#[test]
fn no_wait_says_it_did_not_look_rather_than_that_nobody_answered() {
    let skipped = fold_delivery(CodingSessionDelivery::Steer, None, false);
    assert_eq!(skipped.delivered, None);
    assert_eq!(skipped.status, DELIVERY_UNCONFIRMED);
    assert!(skipped.detail.contains("--no-wait"), "{}", skipped.detail);
    assert!(
        !skipped.detail.contains("no provider receipt arrived"),
        "{}",
        skipped.detail
    );
}

// ── Hire (plan D14) ──────────────────────────────────────────────────────────

const UMBRELLA_HIRE: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

/// The seated create a host publishes in answer to a hire.
#[allow(clippy::too_many_arguments)]
fn seated_create_event(
    id: &str,
    signer: &str,
    created_at: i64,
    command_id: &str,
    session_ref: &str,
    role: &str,
    actor: &str,
    model: Option<&str>,
) -> Value {
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(session_ref.to_owned()),
            genesis_ref: Some("12".repeat(32)),
            provider_instance_ref: "claude-primary".try_into().expect("alias"),
            provider_authority_pubkey: pk(PROVIDER),
            model: model.map(str::to_owned),
            title: Some("a session".into()),
            initial_turn: Some("[From the lead] Rebase the lane.".into()),
            actor: Some(pk(actor)),
            role: Some(role.to_owned()),
            hire_ref: None,
            routing: None,
        },
    };
    json!({
        "id": id,
        "pubkey": pk(signer),
        "kind": KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["csl-v", "csl1-1"], ["csl-command", command_id]],
        "content": serde_json::to_string(&payload).expect("serialize"),
    })
}

/// A host's seated create for this role in this umbrella, published after the
/// hire went out, is the hire's answer — and one published before it, or for
/// another role or umbrella, is not.
#[test]
fn a_hire_is_answered_by_the_seated_create_for_its_role_and_umbrella() {
    let events = vec![
        // Published before the hire: somebody else's seat.
        seated_create_event(
            "old",
            ALICE,
            900,
            "create-old",
            UMBRELLA_HIRE,
            "builder",
            BOB,
            None,
        ),
        // Another umbrella.
        seated_create_event(
            "other-umbrella",
            ALICE,
            1_100,
            "create-x",
            "11111111-2222-3333-4444-555555555555",
            "builder",
            BOB,
            None,
        ),
        // Another role.
        seated_create_event(
            "other-role",
            ALICE,
            1_100,
            "create-y",
            UMBRELLA_HIRE,
            "verifier",
            BOB,
            None,
        ),
        // The answer.
        seated_create_event(
            "the-seat",
            ALICE,
            1_100,
            "create-hired",
            UMBRELLA_HIRE,
            "builder",
            BOB,
            Some("claude-opus"),
        ),
        // An unseated create for the same umbrella is not a hire's answer.
        create_event(
            "unseated",
            ALICE,
            1_100,
            "create-z",
            Some(UMBRELLA_HIRE),
            None,
            PROVIDER,
        ),
    ];

    let seat = sole_hired_seat(&events, UMBRELLA_HIRE, "builder", 1_000).expect("finds the seat");
    assert_eq!(seat.command_id, "create-hired");
    assert_eq!(seat.event_id, "the-seat");
    assert_eq!(seat.actor, pk(BOB));
    assert_eq!(seat.role, "builder");
    assert_eq!(seat.model.as_deref(), Some("claude-opus"));
    assert_eq!(seat.provider_instance_ref.as_str(), "claude-primary");

    assert_eq!(
        sole_hired_seat(&events, UMBRELLA_HIRE, "runner", 1_000),
        None
    );
    // Nothing after the cutoff: the old seat is not re-reported.
    assert_eq!(
        sole_hired_seat(&events, UMBRELLA_HIRE, "builder", 2_000),
        None
    );
}

/// The seat's own create receipt is the hire's receipt: `created` is a hire
/// that worked, `failed` is one that did not, and the provider's code is
/// carried through rather than summarized.
#[test]
fn a_hires_outcome_is_the_seated_creates_receipt() {
    let seat_target = target("s-hired", 1);
    let seat = |id: &str| {
        sole_hired_seat(
            &[seated_create_event(
                id,
                ALICE,
                1_100,
                "create-hired",
                UMBRELLA_HIRE,
                "builder",
                BOB,
                None,
            )],
            UMBRELLA_HIRE,
            "builder",
            1_000,
        )
        .expect("seat")
    };

    let created = vec![receipt_event(
        "r-1",
        1_200,
        "create-hired",
        ReceiptStatus::Created,
        &seat_target,
        None,
        None,
    )];
    let (created_receipts, _) = decode_receipts(&created);
    let outcome = fold_hire(
        Some(seat("a")),
        create_receipts_for_command(&created_receipts, "create-hired")
            .into_iter()
            .next(),
        None,
        true,
    );
    let HireOutcome::Created {
        seat: row, receipt, ..
    } = &outcome
    else {
        panic!("expected a created hire, got {outcome:?}")
    };
    assert_eq!(row.command_id, "create-hired");
    assert_eq!(
        receipt.target_key.as_deref(),
        Some(coding_session_target_key(&seat_target).as_str())
    );

    let failed = vec![receipt_event(
        "r-2",
        1_200,
        "create-hired",
        ReceiptStatus::Failed,
        &seat_target,
        Some(("ACTOR_UNAVAILABLE", "no key for that actor")),
        None,
    )];
    let (failed_receipts, _) = decode_receipts(&failed);
    let outcome = fold_hire(
        Some(seat("b")),
        create_receipts_for_command(&failed_receipts, "create-hired")
            .into_iter()
            .next(),
        None,
        false,
    );
    let HireOutcome::Failed { receipt, .. } = &outcome else {
        panic!("expected a failed hire, got {outcome:?}")
    };
    assert_eq!(receipt.error_code.as_deref(), Some("ACTOR_UNAVAILABLE"));

    // A create published but not yet answered is neither created nor failed.
    assert!(matches!(
        fold_hire(Some(seat("c")), None, None, false),
        HireOutcome::Seating { .. }
    ));
    // Nothing at all is unconfirmed — never a guess in either direction.
    assert_eq!(fold_hire(None, None, None, false), HireOutcome::Unconfirmed);
}

/// A seat the host created but never granted is still `created`, and says in
/// the same breath that it cannot report.
///
/// Live break, 2026-08-28 (item 83): the hire host seated a builder and
/// published no kind:44228 for it, so the relay refused the builder's report
/// with "only a session founder or a granted operator may steer". The command
/// printed `created` and nothing else, and the lead had no way to learn the
/// seat it was waiting on was mute.
#[test]
fn a_seated_but_ungranted_hire_says_it_cannot_report_yet() {
    let seat_target = target("s-hired", 1);
    let seat = sole_hired_seat(
        &[seated_create_event(
            "a",
            ALICE,
            1_100,
            "create-hired",
            UMBRELLA_HIRE,
            "builder",
            BOB,
            None,
        )],
        UMBRELLA_HIRE,
        "builder",
        1_000,
    )
    .expect("seat");
    let events = vec![receipt_event(
        "r-1",
        1_200,
        "create-hired",
        ReceiptStatus::Created,
        &seat_target,
        None,
        None,
    )];
    let (records, _) = decode_receipts(&events);
    let receipt = create_receipts_for_command(&records, "create-hired")
        .into_iter()
        .next();

    let ungranted = fold_hire(Some(seat.clone()), receipt.clone(), None, false);
    assert!(
        matches!(ungranted, HireOutcome::Created { granted: false, .. }),
        "{ungranted:?}"
    );
    // The seat exists, but automation must not read it as a usable hire.
    let report = hire_report(&ungranted, true, CHANNEL);
    assert_eq!(report.status, "created");
    assert_eq!(hire_exit_code(&ungranted), 1);
    assert!(
        report
            .detail
            .contains("exact actor-role grant is not accepted"),
        "got {}",
        report.detail
    );
    // What it may NOT say: the fold does not drop the report, and never did.
    assert!(
        report.detail.contains("still INCLUDES its report"),
        "got {}",
        report.detail
    );
    assert!(
        report.detail.contains("bee sessions seat-repair --channel"),
        "got {}",
        report.detail
    );

    let granted = fold_hire(Some(seat), receipt, None, true);
    let report = hire_report(&granted, true, CHANNEL);
    assert_eq!(report.status, "created");
    assert!(
        report.detail.contains("can report back"),
        "got {}",
        report.detail
    );
    assert!(
        !report.detail.contains("not granted"),
        "got {}",
        report.detail
    );
}

/// The host's refusal is a turn whose text carries a machine-readable code,
/// and only a turn inside this umbrella, after the request, counts.
#[test]
fn a_refusal_turn_is_parsed_into_its_code_and_reason() {
    for (text, code, reason) in [
        (
            "hire refused: HIRE_OFF — hiring is switched off on this computer",
            "HIRE_OFF",
            "hiring is switched off on this computer",
        ),
        (
            "hire refused: HIRE_NO_IDENTITY - install team roles",
            "HIRE_NO_IDENTITY",
            "install team roles",
        ),
    ] {
        let parsed = parse_hire_refusal(text).expect("parses");
        assert_eq!(parsed, (code.to_owned(), reason.to_owned()));
    }
    for text in [
        "hiring is off",
        "hire refused: not allowed",         // no uppercase code
        "hire refused: HIRE_OFF",            // no reason
        "please hire refused: HIRE_OFF — x", // prefix must start the text
    ] {
        assert_eq!(parse_hire_refusal(text), None, "text {text:?} parsed");
    }
}

#[test]
fn a_refusal_is_scoped_to_the_umbrella_and_to_this_request() {
    let mine = target("s-lead", 1);
    let theirs = target("s-elsewhere", 1);
    let rows = vec![
        CrewExecution {
            session_ref: Some(UMBRELLA_HIRE.to_owned()),
            ..execution(
                "s-lead",
                1,
                Some(&pk(ALICE)),
                Some("lead"),
                Some(UMBRELLA_HIRE),
            )
        },
        execution(
            "s-elsewhere",
            1,
            Some(&pk(BOB)),
            Some("lead"),
            Some("other-umbrella"),
        ),
    ];
    let events = vec![
        // Before the request.
        turn_event(
            "t-old",
            ALICE,
            900,
            "c-old",
            &mine,
            "hire refused: HIRE_OFF — hiring is switched off",
        ),
        // Another umbrella's refusal.
        turn_event(
            "t-other",
            BOB,
            1_100,
            "c-other",
            &theirs,
            "hire refused: HIRE_LIMIT — too many seats",
        ),
        // Ordinary prose that merely mentions hiring.
        turn_event(
            "t-prose",
            ALICE,
            1_100,
            "c-prose",
            &mine,
            "I would hire refused: nothing",
        ),
        // Ours.
        turn_event(
            "t-ours",
            ALICE,
            1_150,
            "c-ours",
            &mine,
            "hire refused: HIRE_LIMIT — this session already runs 4 seats",
        ),
    ];
    let (commands, _) = decode_turn_commands(&events);
    let refusal = find_hire_refusal(&commands, &rows, UMBRELLA_HIRE, 1_000).expect("finds it");
    assert_eq!(refusal.event_id, "t-ours");
    assert_eq!(refusal.code, "HIRE_LIMIT");
    assert_eq!(refusal.reason, "this session already runs 4 seats");

    assert_eq!(
        find_hire_refusal(&commands, &rows, UMBRELLA_HIRE, 2_000),
        None
    );

    let outcome = fold_hire(None, None, Some(refusal), false);
    assert!(
        matches!(outcome, HireOutcome::Refused(_)),
        "got {outcome:?}"
    );
}

/// The four outcomes map onto the documented exit codes, and each says in one
/// sentence what happened — an unconfirmed hire is never printed as a success.
#[test]
fn every_hire_outcome_has_its_own_exit_code_and_sentence() {
    let seat_target = target("s-hired", 1);
    let seat = sole_hired_seat(
        &[seated_create_event(
            "the-seat",
            ALICE,
            1_100,
            "create-hired",
            UMBRELLA_HIRE,
            "builder",
            BOB,
            None,
        )],
        UMBRELLA_HIRE,
        "builder",
        1_000,
    )
    .expect("seat");
    let receipt = |status, error| {
        let events = vec![receipt_event(
            "r",
            1_200,
            "create-hired",
            status,
            &seat_target,
            error,
            None,
        )];
        let (records, _) = decode_receipts(&events);
        create_receipts_for_command(&records, "create-hired")
            .into_iter()
            .next()
    };

    let cases = [
        (
            fold_hire(
                Some(seat.clone()),
                receipt(ReceiptStatus::Created, None),
                None,
                true,
            ),
            "created",
            0,
        ),
        (
            fold_hire(
                Some(seat.clone()),
                receipt(ReceiptStatus::Failed, Some(("ACTOR_UNAVAILABLE", "no key"))),
                None,
                false,
            ),
            "failed",
            1,
        ),
        (
            fold_hire(
                None,
                None,
                Some(HireRefusal {
                    event_id: "t".into(),
                    at: 1_100,
                    code: "HIRE_OFF".into(),
                    reason: "hiring is switched off".into(),
                }),
                false,
            ),
            "refused",
            1,
        ),
        (fold_hire(Some(seat), None, None, false), "seating", 5),
        (fold_hire(None, None, None, false), "unconfirmed", 5),
    ];
    for (outcome, word, code) in cases {
        let report = hire_report(&outcome, true, CHANNEL);
        assert_eq!(report.status, word, "{outcome:?}");
        assert_eq!(hire_exit_code(&outcome), code, "{outcome:?}");
        assert!(!report.detail.is_empty(), "{outcome:?} said nothing");
    }

    // Not waiting is its own sentence: nobody was asked, rather than nobody
    // answered.
    let unwaited = hire_report(&HireOutcome::Unconfirmed, false, CHANNEL);
    assert!(
        unwaited.detail.contains("--no-wait"),
        "got {}",
        unwaited.detail
    );
}

/// A relay that predates `session.hire` refuses the payload as malformed. The
/// CLI must say so in those words rather than pass the relay's shape error
/// through as if the request were wrong.
#[test]
fn a_relay_that_does_not_know_hire_is_named_as_such() {
    for message in [
        "relay rejected event: invalid: coding-session lifecycle command action type is unsupported",
        "relay rejected event: invalid: coding-session lifecycle command action has missing or unsupported fields",
        "relay rejected event: invalid: malformed coding-session lifecycle command payload",
    ] {
        let named = hire_unsupported_by_relay(message).expect("recognized");
        assert!(
            named.starts_with("this relay does not accept hire requests yet"),
            "got {named}"
        );
        assert!(named.contains(message), "the relay's own words are dropped: {named}");
    }
    for message in [
        "relay rejected event: restricted: only the session founder or a granted operator may hire",
        "relay rejected event: restricted: coding-session events require channel membership",
    ] {
        assert_eq!(hire_unsupported_by_relay(message), None, "{message}");
    }
}

/// The umbrella's genesis is resolved from the channel, so a lead does not
/// have to carry a 64-hex id; two geneses claiming one label is an error that
/// lists both rather than a coin flip.
#[test]
fn the_umbrella_genesis_is_resolved_or_refused_by_name() {
    let one = genesis_event(&pk(GENESIS_ONE), ALICE, UMBRELLA_HIRE);
    let two = genesis_event(&pk(GENESIS_TWO), BOB, UMBRELLA_HIRE);

    assert_eq!(
        resolve_umbrella_genesis(std::slice::from_ref(&one), UMBRELLA_HIRE).expect("resolves"),
        pk(GENESIS_ONE)
    );

    let missing = usage_message(
        resolve_umbrella_genesis(&[], UMBRELLA_HIRE).expect_err("no genesis is an error"),
    );
    assert!(missing.contains(UMBRELLA_HIRE), "got {missing}");

    let ambiguous = usage_message(
        resolve_umbrella_genesis(&[one, two], UMBRELLA_HIRE).expect_err("two geneses is an error"),
    );
    assert!(
        ambiguous.contains(&pk(GENESIS_ONE)) && ambiguous.contains(&pk(GENESIS_TWO)),
        "got {ambiguous}"
    );
}

/// The signed bytes, exactly. The relay validates 44221 with
/// `deny_unknown_fields` and a closed action list, so the seven keys, their
/// order, and the explicit nulls are the contract — not a formatting
/// preference.
#[test]
fn a_hire_publishes_the_seven_key_action_byte_for_byte() {
    let genesis = "12".repeat(32);
    let payload = hire_payload(
        "hire-1",
        UMBRELLA_HIRE,
        &genesis,
        "builder",
        Some("claude-primary".try_into().expect("alias")),
        Some("claude-opus"),
        "Rebase the lane.",
        None,
        None,
    );
    assert_eq!(
        serde_json::to_string(&payload).expect("serialize"),
        format!(
            concat!(
                r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"hire-1","#,
                r#""action":{{"type":"session.hire","sessionRef":"{umbrella}","#,
                r#""genesisRef":"{genesis}","role":"builder","#,
                r#""providerInstanceRef":"claude-primary","model":"claude-opus","#,
                r#""brief":"Rebase the lane."}}}}"#
            ),
            umbrella = UMBRELLA_HIRE,
            genesis = genesis,
        )
    );

    // The optional pair is written as explicit nulls, never skipped: a relay
    // that expects seven keys refuses five.
    let defaults = hire_payload(
        "hire-2",
        UMBRELLA_HIRE,
        &genesis,
        "builder",
        None,
        None,
        "Rebase the lane.",
        None,
        None,
    );
    let content = serde_json::to_string(&defaults).expect("serialize");
    assert!(
        content.contains(r#""providerInstanceRef":null,"model":null"#),
        "got {content}"
    );
    // And it survives the relay's own strict decoder.
    assert_eq!(
        buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &content
        )
        .expect("decodes"),
        defaults
    );
}

/// A routed hire is the same seven keys plus `routing`, in that order, and
/// `routing` is the REQUEST — never the record. Brian's ruling of 2026-08-30,
/// as corrected by the drop of 2026-08-30 09:52 (ledger draft 97).
#[test]
fn a_routed_hire_appends_the_routing_request_as_an_eighth_key() {
    use buzz_core::coding_session_routing::{HireRoutingRequest, ProposedRouting, RoutingTarget};

    let genesis = "12".repeat(32);
    let request = HireRoutingRequest {
        class: "builder".to_owned(),
        risk: buzz_core::coding_session_routing::Risk {
            impact: 3,
            uncertainty: 3,
            irreversibility: 2,
        },
        profile: None,
        r#override: None,
        challenger_sample: false,
        review_flags: Vec::new(),
        proposed: Some(ProposedRouting {
            chosen: RoutingTarget {
                provider: "claude-primary".to_owned(),
                model: "sonnet".to_owned(),
                effort: "medium".to_owned(),
            },
            runner_up: None,
            reason: "claude-primary/sonnet cleared the builder gate".to_owned(),
            registry_version: 1,
            catalog_revision: Some(7),
        }),
    };
    // No override, so the hire names no target at all: the host routes.
    let payload = hire_payload(
        "hire-3",
        UMBRELLA_HIRE,
        &genesis,
        "builder",
        None,
        None,
        "Rebase the lane.",
        None,
        Some(request),
    );
    let content = serde_json::to_string(&payload).expect("serialize");
    assert!(
        content.contains(r#""brief":"Rebase the lane.","routing":{"class":"builder""#),
        "routing must be the eighth key, after the brief: {content}"
    );
    // The request's risk has three factors and no `score`: the product is the
    // host's arithmetic, and a score a requester sets can disagree with its
    // own factors.
    assert!(
        content.contains(r#""risk":{"impact":3,"uncertainty":3,"irreversibility":2}"#),
        "{content}"
    );
    // None of the record's keys are at the top of a hire's `routing` — that
    // exact set is what the desktop host's parser refused in silence for
    // fifteen minutes. (They are legal *inside* `proposed`, which is why this
    // reads the object rather than grepping the bytes.)
    let emitted: serde_json::Value = serde_json::from_str(&content).expect("json");
    let routing = emitted["action"]["routing"].as_object().expect("routing");
    for key in [
        "tier",
        "chosen",
        "runnerUp",
        "reason",
        "reviewRequired",
        "reviewReasons",
        "registryVersion",
        "catalogRevision",
        "proposedDisagreement",
    ] {
        assert!(
            !routing.contains_key(key),
            "a hire must not carry the record's {key:?}: {content}"
        );
    }
    // The requester's own decision rides as `proposed`, clearly nested.
    assert!(
        content.contains(r#""proposed":{"chosen":{"provider":"claude-primary""#),
        "{content}"
    );
    // And the relay's own strict decoder accepts it.
    assert_eq!(
        buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &content
        )
        .expect("decodes"),
        payload
    );
}

/// Every request in the shared fixture is one this CLI emits byte-for-byte.
///
/// `testdata/routing/hire-request-fixture.json` pins three implementations:
/// `buzz-core`'s validator, this emitter and the desktop's parser. Reading it
/// here is what keeps them one contract rather than three that agree today.
#[test]
fn the_cli_emits_every_shared_fixture_request_byte_for_byte() {
    use buzz_core::coding_session_routing::HireRoutingRequest;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("testdata/routing/hire-request-fixture.json"))
            .expect("read the shared hire-request fixture"),
    )
    .expect("json");
    let cases = fixture["requests"].as_array().expect("requests");
    assert_eq!(cases.len(), 3, "the contract names three requests");
    let genesis = "12".repeat(32);
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let request: HireRoutingRequest = serde_json::from_value(case["routing"].clone())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        // An override is the one thing that writes the hire's top level, and
        // it writes the override's own model — never the router's pick.
        let model = request.r#override.as_ref().map(|over| over.model.clone());
        let payload = hire_payload(
            "hire-fixture",
            UMBRELLA_HIRE,
            &genesis,
            "builder",
            model
                .as_ref()
                .map(|_| "codex-primary".try_into().expect("alias")),
            model.as_deref(),
            "Rebase the lane.",
            None,
            Some(request.clone()),
        );
        let content = serde_json::to_string(&payload).expect("serialize");
        let decoded =
            buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
                &content,
            )
            .unwrap_or_else(|error| panic!("{name}: the relay refuses this hire: {error}"));
        assert_eq!(decoded, payload, "{name}");
        // The emitted request is the fixture's, key for key.
        let emitted: serde_json::Value = serde_json::from_str(&content).expect("json");
        assert_eq!(emitted["action"]["routing"], case["routing"], "{name}");
        // And an unrouted hire's top level stays null unless an override
        // named a target.
        assert_eq!(
            emitted["action"]["model"],
            match &model {
                Some(model) => serde_json::json!(model),
                None => serde_json::Value::Null,
            },
            "{name}: the top level names a model only for an override"
        );
    }
}

/// The exit-code table `bee sessions hire --help` prints is the one the
/// process actually returns.
#[test]
fn hire_errors_carry_the_documented_exit_codes() {
    use crate::error::exit_code;
    assert_eq!(
        exit_code(&CliError::Refused("hire refused: HIRE_OFF — x".into())),
        1
    );
    assert_eq!(
        exit_code(&CliError::Unconfirmed("nothing answered".into())),
        5
    );
    assert_eq!(
        exit_code(&CliError::Relay {
            status: 400,
            body: "this relay does not accept hire requests yet".into(),
        }),
        2
    );
}

/// Every refusal code the contract defines is one this CLI can act on.
///
/// A lead reads the refusal through `bee sessions hire`, so a code the CLI has
/// never heard of prints as a bare token with no way forward. The remedy table
/// is the CLI's list of known codes, and it must cover the contract's — which
/// is what keeps a new code from shipping in `buzz-core` alone.
#[test]
fn every_contract_refusal_code_has_a_remedy_this_cli_can_print() {
    for code in buzz_core::coding_session_lifecycle_command::HIRE_REFUSAL_CODES {
        let remedy = hire_refusal_remedy(code)
            .unwrap_or_else(|| panic!("no remedy for refusal code {code}"));
        assert!(!remedy.trim().is_empty(), "empty remedy for {code}");
    }
    // The two codes this CLI learned with the host's model check and its
    // staleness window; named literally so a rename cannot pass silently.
    assert!(hire_refusal_remedy("HIRE_MODEL_NOT_OFFERED").is_some());
    assert!(hire_refusal_remedy("HIRE_STALE").is_some());
    // The router's own refusal. Its remedy must not send a lead to the next
    // model down — "the smartest available" and "the cheapest that fits" are
    // both wrong answers to a requirement nothing meets.
    let no_route = hire_refusal_remedy("HIRE_NO_ROUTE").expect("HIRE_NO_ROUTE has a remedy");
    assert!(no_route.contains("--because"), "{no_route}");
    assert!(
        !no_route.to_lowercase().contains("next model"),
        "{no_route}"
    );
    // The refusal that exists because a hire used to vanish. Its remedy has
    // to carry two things: the placeholder for the key the host names, and
    // the command that produces a request shape that parses. Without both, a
    // lead reads "malformed" and re-sends the identical bytes.
    let malformed = hire_refusal_remedy("HIRE_MALFORMED").expect("HIRE_MALFORMED has a remedy");
    assert!(malformed.contains("<key>"), "{malformed}");
    assert!(malformed.contains("bee sessions route"), "{malformed}");
    assert!(
        malformed.contains("bee sessions hire --help"),
        "{malformed}"
    );
    // An invented code is not known, and the report says nothing rather than
    // guessing a remedy for it.
    assert_eq!(hire_refusal_remedy("HIRE_NOT_A_REAL_CODE"), None);
}

/// The two identity refusals carry two remedies, because they are two facts.
///
/// `HIRE_ROLE_BUSY` means this computer holds the role and every one of its
/// identities is already sitting in this umbrella — the lead's way forward is
/// the seat that already exists, not an install. `HIRE_NO_IDENTITY` means the
/// role is not on this computer at all, which only the operator can fix. Live
/// on 2026-08-28 a lead was told to install a role it already held (item
/// 88(h)).
#[test]
fn a_busy_role_and_an_absent_one_carry_different_remedies() {
    let busy = hire_refusal_remedy("HIRE_ROLE_BUSY").expect("HIRE_ROLE_BUSY has a remedy");
    assert!(
        busy.contains("bee sessions send --to <role>"),
        "busy remedy {busy:?}"
    );
    assert!(
        !busy.to_lowercase().contains("install"),
        "the busy remedy must not tell a lead to install a role it holds: {busy:?}"
    );
    let absent = hire_refusal_remedy("HIRE_NO_IDENTITY").expect("HIRE_NO_IDENTITY has a remedy");
    assert!(
        absent.to_lowercase().contains("install team roles"),
        "absent remedy {absent:?}"
    );
    assert_ne!(busy, absent);
}

/// A refused hire prints the host's own sentence *and* what to do next.
#[test]
fn a_refused_hire_report_carries_the_remedy_for_its_code() {
    let report = hire_report(
        &HireOutcome::Refused(HireRefusal {
            event_id: "e1".into(),
            at: 10,
            code: "HIRE_MODEL_NOT_OFFERED".into(),
            reason: "this computer's claude-primary runtime does not offer the model \
                     claude-sonnet-5. It offers default, haiku, opus, sonnet."
                .into(),
        }),
        true,
        CHANNEL,
    );
    assert_eq!(report.status, "refused");
    assert!(
        report
            .detail
            .contains("hire refused: HIRE_MODEL_NOT_OFFERED"),
        "detail {:?}",
        report.detail
    );
    assert!(
        report
            .detail
            .contains(hire_refusal_remedy("HIRE_MODEL_NOT_OFFERED").expect("remedy")),
        "detail {:?}",
        report.detail
    );
}

// ── `bee sessions status --json-lines` ───────────────────────────────────────
//
// NDJSON is for a consumer that reads the status stream a line at a time —
// `grep`, `jq -c`, a `while read` loop. The single-document outputs make that
// consumer buffer the whole array and index into it, so the guarantees worth
// pinning here are structural: one line per execution, in order; the same
// fields a `--format json` row carries; nothing at all when there is nothing
// to say.

/// Three executions in, three rows out, in the order `facts.executions` holds
/// them. A line format whose lines do not correspond one-to-one with the
/// executions is not a line format.
#[test]
fn json_lines_emits_one_row_per_execution() {
    let executions = vec![
        execution("s-1", 1, Some(&pk(ALICE)), Some("lead"), Some("u-1")),
        execution("s-2", 1, Some(&pk(BOB)), Some("builder"), Some("u-1")),
        execution("s-3", 2, None, None, None),
    ];
    let rows = status_json_lines(&executions, &[], &HashMap::new(), &[], &index_of(&[]));
    assert_eq!(rows.len(), 3, "one row per execution");
    let ids: Vec<String> = rows
        .iter()
        .map(|line| {
            let row: Value = serde_json::from_str(line).expect("each line is a JSON document");
            row["sessionId"].as_str().expect("sessionId").to_owned()
        })
        .collect();
    assert_eq!(
        ids,
        vec!["s-1", "s-2", "s-3"],
        "rows keep their input order"
    );
    // NDJSON, not a pretty-printed array: no line may carry an embedded newline.
    for line in &rows {
        assert!(!line.contains('\n'), "line carries a newline: {line:?}");
    }
}

/// Field parity with `--format json`, asserted rather than assumed: the same
/// key set and the same values. `--json-lines` is a framing change, not a
/// second row shape to keep in step by hand.
#[test]
fn json_lines_rows_carry_the_same_fields_as_the_json_format_rows() {
    let executions = vec![execution(
        "s-1",
        2,
        Some(&pk(ALICE)),
        Some("builder"),
        Some("u-1"),
    )];
    let founders = index_of(&[]);
    let line = status_json_lines(&executions, &[], &HashMap::new(), &[], &founders)
        .pop()
        .expect("one row");
    let from_line: Value = serde_json::from_str(&line).expect("the line is a JSON document");
    let from_json = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &[],
        &founders,
        &crate::OutputFormat::Json,
    );
    let line_keys: Vec<&String> = from_line.as_object().expect("object").keys().collect();
    let json_keys: Vec<&String> = from_json.as_object().expect("object").keys().collect();
    assert_eq!(line_keys, json_keys, "identical key set");
    assert_eq!(from_line, from_json, "identical values");
}

/// Zero executions is zero bytes on stdout — not `[]`, not an envelope, not a
/// header. A consumer counting lines must be able to read "none" as none.
#[test]
fn json_lines_emits_nothing_when_there_are_no_executions() {
    let rows = status_json_lines(&[], &[], &HashMap::new(), &[], &index_of(&[]));
    assert!(rows.is_empty(), "no executions means no rows: {rows:?}");
}

/// `--json-lines` overrides the global `--format`. Asking for compact and for
/// lines gets the JSON row's fields, because "same fields as a `--format json`
/// row" is the flag's whole contract — a compact seven-key row would silently
/// drop the `model` that item 82's tooling reads.
#[test]
fn json_lines_overrides_format_compact() {
    let executions = vec![execution(
        "s-1",
        1,
        Some(&pk(ALICE)),
        Some("builder"),
        Some("u-1"),
    )];
    let line = status_json_lines(&executions, &[], &HashMap::new(), &[], &index_of(&[]))
        .pop()
        .expect("one row");
    let row: Value = serde_json::from_str(&line).expect("the line is a JSON document");
    for key in ["actor", "runtime", "model", "lastSignedSeq"] {
        assert!(
            row.get(key).is_some(),
            "JSON-only key {key} missing from {row:?}"
        );
    }
    // And the compact row genuinely lacks them, so the assertion above is not
    // vacuous.
    let compact = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &[],
        &index_of(&[]),
        &crate::OutputFormat::Compact,
    );
    assert!(compact.get("model").is_none(), "compact row {compact:?}");
}

/// The flag exists on `sessions status`, defaults to `false`, and is a
/// subcommand flag rather than a global one.
#[test]
fn sessions_status_parses_the_json_lines_flag() {
    use clap::Parser;

    let with_flag = crate::Cli::try_parse_from([
        "bee",
        "sessions",
        "status",
        "--channel",
        CHANNEL,
        "--json-lines",
    ])
    .expect("--json-lines parses");
    match with_flag.command {
        crate::Cmd::Sessions(crate::SessionsCmd::Status { json_lines, .. }) => {
            assert!(json_lines, "--json-lines sets the flag");
        }
        _ => panic!("expected sessions status"),
    }

    let without = crate::Cli::try_parse_from(["bee", "sessions", "status", "--channel", CHANNEL])
        .expect("the bare command parses");
    match without.command {
        crate::Cmd::Sessions(crate::SessionsCmd::Status { json_lines, .. }) => {
            assert!(!json_lines, "the flag defaults to false");
        }
        _ => panic!("expected sessions status"),
    }
}

// ── `bee sessions status`: NDJSON automatically down a pipe ──────────────────
//
// The five rows of the lane's precedence table, each driven straight through
// `resolve_json_lines`. That function takes the terminal's state as an
// argument precisely so these cases never consult the test harness's own
// stdout — a test that passes under `cargo test` and fails under
// `cargo test | cat` would be testing the runner, not the rule.

/// Row 1. An explicit `--json-lines` is obeyed on a terminal, where the
/// automatic rule would have chosen the document.
#[test]
fn json_lines_auto_explicit_json_lines_beats_a_tty() {
    assert!(resolve_json_lines(true, false, false, true));
    // …and still wins when a format was named too.
    assert!(resolve_json_lines(true, false, true, true));
}

/// Row 2. An explicit `--no-json-lines` is obeyed down a pipe, where the
/// automatic rule would have chosen NDJSON. This is the escape hatch, so it
/// has to hold in exactly the case the new behavior applies to.
#[test]
fn json_lines_auto_explicit_no_json_lines_beats_a_pipe() {
    assert!(!resolve_json_lines(false, true, false, false));
}

/// Row 3. Naming `--format` is itself a request for that format's document.
/// Item 82's tooling reads `bee --format json sessions status …` down a pipe
/// and must keep getting the envelope it indexes into.
#[test]
fn json_lines_auto_an_explicit_format_keeps_the_document() {
    assert!(!resolve_json_lines(false, false, true, false));
    assert!(!resolve_json_lines(false, false, true, true));
}

/// Row 4, the only new behavior in the lane: nothing asked for, stdout is not
/// a terminal, so the output is NDJSON.
#[test]
fn json_lines_auto_a_bare_pipe_gets_ndjson() {
    assert!(resolve_json_lines(false, false, false, false));
}

/// Row 5. Nothing asked for and a terminal on the other end: unchanged, the
/// document a person reads.
#[test]
fn json_lines_auto_a_bare_tty_keeps_the_document() {
    assert!(!resolve_json_lines(false, false, false, true));
}

/// The two flags contradict each other, so clap refuses the invocation rather
/// than letting either win silently. An error, not a panic.
#[test]
fn json_lines_and_no_json_lines_together_are_a_usage_error() {
    use clap::Parser;

    // `Cli` is not `Debug`, so this cannot be `expect_err`.
    let error = match crate::Cli::try_parse_from([
        "bee",
        "sessions",
        "status",
        "--channel",
        CHANNEL,
        "--json-lines",
        "--no-json-lines",
    ]) {
        Ok(_) => panic!("the two flags together must be refused"),
        Err(error) => error,
    };
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::ArgumentConflict,
        "expected a conflict, got {:?}: {error}",
        error.kind()
    );
}

/// The escape hatch exists on the command, and is absent by default.
#[test]
fn sessions_status_parses_the_no_json_lines_flag() {
    use clap::Parser;

    let with_flag = crate::Cli::try_parse_from([
        "bee",
        "sessions",
        "status",
        "--channel",
        CHANNEL,
        "--no-json-lines",
    ])
    .expect("--no-json-lines parses");
    match with_flag.command {
        crate::Cmd::Sessions(crate::SessionsCmd::Status { no_json_lines, .. }) => {
            assert!(no_json_lines, "--no-json-lines sets the flag");
        }
        _ => panic!("expected sessions status"),
    }

    let without = crate::Cli::try_parse_from(["bee", "sessions", "status", "--channel", CHANNEL])
        .expect("the bare command parses");
    match without.command {
        crate::Cmd::Sessions(crate::SessionsCmd::Status { no_json_lines, .. }) => {
            assert!(!no_json_lines, "the flag defaults to false");
        }
        _ => panic!("expected sessions status"),
    }
}

/// The mechanism row 3 stands on: after parsing, `--format json` asked for
/// explicitly must still be distinguishable from the `json` that fell through
/// as the default. `Cli.format` alone cannot say — both read `Json` — so the
/// answer comes from the `ArgMatches` the entry point now keeps.
#[test]
fn an_explicit_format_is_distinguishable_from_the_default() {
    use clap::CommandFactory;

    let explicit = crate::Cli::command()
        .try_get_matches_from([
            "bee",
            "--format",
            "json",
            "sessions",
            "status",
            "--channel",
            CHANNEL,
        ])
        .expect("an explicit --format parses");
    assert_eq!(
        explicit.value_source("format"),
        Some(clap::parser::ValueSource::CommandLine),
        "an explicit --format must read as CommandLine"
    );

    let fell_through = crate::Cli::command()
        .try_get_matches_from(["bee", "sessions", "status", "--channel", CHANNEL])
        .expect("the bare command parses");
    assert_eq!(
        fell_through.value_source("format"),
        Some(clap::parser::ValueSource::DefaultValue),
        "an unstated --format must read as DefaultValue"
    );
}

// ── Context consumption on the wire ──────────────────────────────────────────

/// The driver's own occupancy item is the best fact available, so it wins:
/// `used`/`size` came from the thing holding the context.
#[test]
fn the_context_column_prefers_the_drivers_own_occupancy_item() {
    let target = target("s-1", 2);
    let events = vec![transcript_event(
        "t-1",
        1_000,
        &target,
        1,
        Some("turn-1"),
        json!({ "kind": "context_window_updated", "usage": { "size": 1_000_000, "used": 137_498 } }),
    )];
    let (transcripts, _) = decode_transcripts(&events);
    let executions = [execution("s-1", 2, None, None, None)];
    let row = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &transcripts,
        &index_of(&[]),
        &crate::OutputFormat::Json,
    );
    assert_eq!(
        row["context"],
        json!({ "usedTokens": 137_498, "contextWindow": 1_000_000, "contextPct": 14 })
    );
}

/// With no occupancy item, the turn's own usage block answers: the prompt-side
/// total, against the window the provider named.
#[test]
fn the_context_column_falls_back_to_the_turns_usage_block() {
    let target = target("s-1", 2);
    let events = vec![transcript_event(
        "t-1",
        1_000,
        &target,
        1,
        Some("turn-1"),
        json!({
            "kind": "result",
            "subtype": "success",
            "usage": {
                "inputTokens": 1_200,
                "cacheReadTokens": 96_000,
                "cacheWriteTokens": 4_000,
                "contextWindow": 200_000,
            },
        }),
    )];
    let (transcripts, _) = decode_transcripts(&events);
    let executions = [execution("s-1", 2, None, None, None)];
    let row = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &transcripts,
        &index_of(&[]),
        &crate::OutputFormat::Json,
    );
    assert_eq!(
        row["context"],
        json!({ "usedTokens": 101_200, "contextWindow": 200_000, "contextPct": 51 })
    );
}

/// Tokens without a window is a real answer and a partial one. It must not be
/// rendered as a percentage of a guess, and it must not be dropped.
#[test]
fn tokens_without_a_window_report_the_tokens_and_say_the_window_is_unknown() {
    let target = target("s-1", 2);
    let events = vec![transcript_event(
        "t-1",
        1_000,
        &target,
        1,
        Some("turn-1"),
        json!({ "kind": "result", "subtype": "success", "usage": { "inputTokens": 4_096 } }),
    )];
    let (transcripts, _) = decode_transcripts(&events);
    let executions = [execution("s-1", 2, None, None, None)];
    let json_row = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &transcripts,
        &index_of(&[]),
        &crate::OutputFormat::Json,
    );
    assert_eq!(
        json_row["context"],
        json!({ "usedTokens": 4_096, "contextWindow": Value::Null, "contextPct": Value::Null })
    );
    let compact = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &transcripts,
        &index_of(&[]),
        &crate::OutputFormat::Compact,
    );
    assert_eq!(compact["context"], json!("4096 (window unknown)"));
}

/// No usage anywhere on the wire is `null` in JSON and an em dash in the
/// compact row — never `0`, never `0%`.
#[test]
fn an_execution_with_no_usage_on_the_wire_reports_none_rather_than_zero() {
    let executions = [execution("s-1", 2, None, None, None)];
    let json_row = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &[],
        &index_of(&[]),
        &crate::OutputFormat::Json,
    );
    assert_eq!(json_row["context"], Value::Null);
    let compact = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &[],
        &index_of(&[]),
        &crate::OutputFormat::Compact,
    );
    assert_eq!(compact["context"], json!("\u{2014}"));
}

/// The newest occupancy item wins; an older one is history, not the answer.
#[test]
fn the_newest_occupancy_item_is_the_one_reported() {
    let target = target("s-1", 2);
    let events = vec![
        transcript_event(
            "t-1",
            1_000,
            &target,
            1,
            Some("turn-1"),
            json!({ "kind": "context_window_updated", "usage": { "size": 1_000_000, "used": 10 } }),
        ),
        transcript_event(
            "t-2",
            2_000,
            &target,
            2,
            Some("turn-2"),
            json!({ "kind": "context_window_updated", "usage": { "size": 1_000_000, "used": 500_000 } }),
        ),
    ];
    let (transcripts, _) = decode_transcripts(&events);
    let executions = [execution("s-1", 2, None, None, None)];
    let row = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &transcripts,
        &index_of(&[]),
        &crate::OutputFormat::Json,
    );
    assert_eq!(row["context"]["usedTokens"], json!(500_000));
    assert_eq!(row["context"]["contextPct"], json!(50));
}

/// The compact row a person reads renders the ratio and the percentage.
#[test]
fn the_compact_context_cell_renders_used_over_window_and_a_percentage() {
    let target = target("s-1", 2);
    let events = vec![transcript_event(
        "t-1",
        1_000,
        &target,
        1,
        Some("turn-1"),
        json!({ "kind": "context_window_updated", "usage": { "size": 1_000_000, "used": 137_498 } }),
    )];
    let (transcripts, _) = decode_transcripts(&events);
    let executions = [execution("s-1", 2, None, None, None)];
    let compact = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &transcripts,
        &index_of(&[]),
        &crate::OutputFormat::Compact,
    );
    assert_eq!(compact["context"], json!("137498/1000000 (14%)"));
}

/// Another execution's transcript is not this execution's context.
#[test]
fn context_is_scoped_to_the_execution_that_produced_it() {
    let other = target("s-2", 1);
    let events = vec![transcript_event(
        "t-1",
        1_000,
        &other,
        1,
        Some("turn-1"),
        json!({ "kind": "context_window_updated", "usage": { "size": 1_000_000, "used": 900_000 } }),
    )];
    let (transcripts, _) = decode_transcripts(&events);
    let executions = [execution("s-1", 2, None, None, None)];
    let row = status_row(
        &executions[0],
        &[],
        &HashMap::new(),
        &transcripts,
        &index_of(&[]),
        &crate::OutputFormat::Json,
    );
    assert_eq!(row["context"], Value::Null);
}

// ── Hire window and seat-repair remediation (batch 2026-09-01, C1/C2/C6) ─────

/// The window a hire waits, and the sentence an ungranted seat ends with.
///
/// Live break, cleantest 2026-09-01: the builder's `created` receipt was
/// signed at 19:11:28, three seconds after the 19:11:25 hire — and the CLI's
/// sixty-second window closed without seeing it, because the receipt reached
/// the relay later than it was signed. Desktop's own receipt wait has been
/// 120 s (`CODING_SESSION_CREW_RECEIPT_TIMEOUT_MS`) since it was written.
#[test]
fn the_hire_window_matches_desktop_and_ungranted_copy_names_seat_repair() {
    assert_eq!(HIRE_WAIT_SECONDS, 120);

    let seat_target = target("s-hired", 1);
    let seat = sole_hired_seat(
        &[seated_create_event(
            "a",
            ALICE,
            1_100,
            "create-hired",
            UMBRELLA_HIRE,
            "builder",
            BOB,
            None,
        )],
        UMBRELLA_HIRE,
        "builder",
        1_000,
    )
    .expect("seat");
    let events = vec![receipt_event(
        "r-1",
        1_200,
        "create-hired",
        ReceiptStatus::Created,
        &seat_target,
        None,
        None,
    )];
    let (records, _) = decode_receipts(&events);
    let receipt = create_receipts_for_command(&records, "create-hired")
        .into_iter()
        .next();
    let expected = format!(
        "If the seat runs anyway, repair its authority with: bee sessions seat-repair \
         --channel {CHANNEL} --session-ref {UMBRELLA_HIRE} --actor {}. Never hire again.",
        seat.actor
    );

    let ungranted = fold_hire(Some(seat.clone()), receipt, None, false);
    let report = hire_report(&ungranted, true, CHANNEL);
    assert!(report.detail.ends_with(&expected), "got {}", report.detail);
    // The old sentence was false: the fold includes the report by assignee
    // identity and discloses the missing seat instead of excluding it.
    assert!(
        !report.detail.contains("excludes an unauthoritative report"),
        "got {}",
        report.detail
    );
    assert!(
        report.detail.contains("unseatedReports"),
        "got {}",
        report.detail
    );

    let seating = hire_report(&HireOutcome::Seating { seat }, true, CHANNEL);
    assert!(
        seating.detail.contains("within 120s"),
        "got {}",
        seating.detail
    );
    assert!(
        seating.detail.ends_with(&expected),
        "got {}",
        seating.detail
    );

    let unconfirmed = hire_report(&HireOutcome::Unconfirmed, true, CHANNEL);
    assert!(
        unconfirmed.detail.contains("within 120s"),
        "got {}",
        unconfirmed.detail
    );
}

/// No sentence anywhere in the hire path may still claim the typed fold
/// excludes an ungranted seat's report. It does not, and did not.
#[test]
fn no_hire_copy_claims_the_typed_fold_excludes_an_ungranted_report() {
    for source in [include_str!("crew.rs"), include_str!("crew_cmds.rs")] {
        for needle in [
            "excludes an unauthoritative report",
            "cannot sign its typed team report",
        ] {
            assert!(
                !source.contains(needle),
                "a hire source still claims {needle:?}"
            );
        }
    }
}

// ── Derived team-operation wake command ids (batch 2, A1.1) ──────────────────

use super::crew_cmds::{team_operation_wake_command_id, CLI_TEAM_WAKE_COMMAND_ID_PREFIX};
use super::operations::{resolve_delivery_command_id, wake_shares_delivery_command_id};
use buzz_core::coding_session_team_transaction::CodingSessionTeamTransactionType;

/// The five operation classes whose wake must mint its own command id.
const DERIVING_OPERATIONS: &[CodingSessionTeamTransactionType] = &[
    CodingSessionTeamTransactionType::Report,
    CodingSessionTeamTransactionType::Verdict,
    CodingSessionTeamTransactionType::Acknowledgement,
    CodingSessionTeamTransactionType::MissionCompleted,
    CodingSessionTeamTransactionType::MissionBlocked,
];

#[test]
fn a_wake_command_id_is_deterministic_per_operation_and_target() {
    let operation = "f".repeat(64);
    let lead = coding_session_target_key(&target("lead-session", 3));

    let first = team_operation_wake_command_id(&operation, &lead);
    let second = team_operation_wake_command_id(&operation, &lead);

    assert_eq!(
        first, second,
        "the same operation and target must derive one id"
    );
    let (namespace, rest) = first
        .split_once(':')
        .unwrap_or_else(|| panic!("no namespace in {first}"));
    assert_eq!(namespace, CLI_TEAM_WAKE_COMMAND_ID_PREFIX);
    let (source, digest) = rest
        .split_once(':')
        .unwrap_or_else(|| panic!("no digest in {first}"));
    assert_eq!(
        source, operation,
        "the id must name the operation it wakes for"
    );
    assert_eq!(
        digest.len(),
        12,
        "digest is 12 hex characters, got {digest:?}"
    );
    assert!(
        digest
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "digest is lowercase hex, got {digest:?}"
    );
}

#[test]
fn a_wake_command_id_differs_across_targets_and_operations() {
    let operation = "a".repeat(64);
    let other_operation = "b".repeat(64);
    let lead = coding_session_target_key(&target("lead-session", 3));
    let other_generation = coding_session_target_key(&target("lead-session", 4));
    let other_seat = coding_session_target_key(&target("builder-session", 3));

    let base = team_operation_wake_command_id(&operation, &lead);
    assert_ne!(
        base,
        team_operation_wake_command_id(&operation, &other_generation)
    );
    assert_ne!(
        base,
        team_operation_wake_command_id(&operation, &other_seat)
    );
    assert_ne!(
        base,
        team_operation_wake_command_id(&other_operation, &lead)
    );
}

/// The exact live-run shape from `docs/SESSION_STATE.md` item 103, finding 4:
/// Bob's report carried the delivery command id of the assignment that had
/// already been consumed on his own target, so the lead runner fenced the wake
/// and published no 44224. The derived id must not be that id, and must not be
/// anything else the CLI was handed.
#[test]
fn a_derived_wake_id_is_never_a_delivery_command_id_the_cli_received() {
    // Prefixes are the live ids recorded in the ledger; the tails are
    // synthetic because the ledger records only the prefixes.
    let bob_report = format!("fc2769b3{}", "0".repeat(56));
    let assignment_delivery = "f40a8195-4c2b-4a1d-9f83-1b2c3d4e5f60";
    let received: Vec<String> = vec![
        assignment_delivery.to_owned(),
        "1f2e3d4c-5b6a-4798-8877-665544332211".to_owned(),
        format!("team-wake-v1:{}:{}", "9".repeat(64), "a".repeat(24)),
    ];

    let derived =
        team_operation_wake_command_id(&bob_report, &coding_session_target_key(&target("lead", 1)));

    for id in &received {
        assert_ne!(&derived, id, "the derived wake id reused a received id");
    }
    assert!(
        derived.starts_with(&format!("{CLI_TEAM_WAKE_COMMAND_ID_PREFIX}:")),
        "got {derived}"
    );
}

#[test]
fn only_an_assignment_shares_its_delivery_command_id_with_its_wake() {
    assert!(wake_shares_delivery_command_id(
        CodingSessionTeamTransactionType::Assignment
    ));
    for operation in DERIVING_OPERATIONS {
        assert!(
            !wake_shares_delivery_command_id(*operation),
            "{} must derive its wake command id",
            operation.as_str()
        );
    }
}

#[test]
fn a_waking_report_refuses_a_caller_supplied_delivery_command_id() {
    for operation in DERIVING_OPERATIONS {
        let error = resolve_delivery_command_id(
            *operation,
            Some("lead"),
            Some("f40a8195-4c2b-4a1d-9f83-1b2c3d4e5f60".to_owned()),
        )
        .expect_err(&format!(
            "{} accepted an inherited delivery command id",
            operation.as_str()
        ));
        assert!(
            matches!(error, CliError::Usage(_)),
            "{} refused with {error:?}, wanted a usage error",
            operation.as_str()
        );
        assert!(
            error.to_string().contains("AlreadyConsumed"),
            "{} refusal must say why: {error}",
            operation.as_str()
        );

        assert_eq!(
            resolve_delivery_command_id(*operation, Some("lead"), None).expect("no id is fine"),
            None,
            "{} must record no deliveryCommandId it did not use",
            operation.as_str()
        );
    }
}

#[test]
fn an_assignment_keeps_the_shared_delivery_command_id_its_binding_needs() {
    let named = resolve_delivery_command_id(
        CodingSessionTeamTransactionType::Assignment,
        Some("builder"),
        Some("cmd-77".to_owned()),
    )
    .expect("an assignment may name its own delivery command");
    assert_eq!(named.as_deref(), Some("cmd-77"));

    let minted = resolve_delivery_command_id(
        CodingSessionTeamTransactionType::Assignment,
        Some("builder"),
        None,
    )
    .expect("an assignment mints one when the caller named none");
    assert!(minted.is_some(), "an assignment wake needs a shared id");
}

/// Recording a correlation id without waking anybody is unchanged: nothing is
/// delivered, so nothing can be double-spent.
#[test]
fn a_recorded_operation_without_a_wake_keeps_its_correlation_id() {
    for operation in DERIVING_OPERATIONS {
        assert_eq!(
            resolve_delivery_command_id(*operation, None, Some("cmd-9".to_owned()))
                .expect("no wake, no refusal"),
            Some("cmd-9".to_owned())
        );
    }
}

// ── The hire wait carries its last verification error (batch 2, A1.4) ────────

fn wait_seat() -> HiredSeat {
    HiredSeat {
        event_id: "e".repeat(64),
        create_signer: pk("cc"),
        command_id: "create-hired".into(),
        actor: pk(BOB),
        role: "builder".into(),
        session_ref: "6a8f1b2c-0000-4000-8000-000000000001".into(),
        genesis_ref: "d".repeat(64),
        provider_authority_pubkey: pk(PROVIDER),
        provider_instance_ref: "claude-primary".try_into().expect("alias"),
        model: None,
        at: 1_000,
        raw: json!({}),
    }
}

fn wait_receipt() -> SeatReceipt {
    SeatReceipt {
        event_id: "f".repeat(64),
        signer: pk(PROVIDER),
        status: ReceiptStatus::Created,
        target_key: Some(coding_session_target_key(&target("session-1", 1))),
        error_code: None,
        error_message: None,
        at: 1_002,
        raw: json!({}),
    }
}

/// The exact 2026-09-01 shape: a seated create with no receipt yet, then a
/// receipt two seconds later whose binding check refuses it. The wait must end
/// on `seating` **and** name the refusal, never on a bare timeout.
#[test]
fn a_binding_failure_leaves_seating_carrying_the_reason() {
    let mut wait = HireWait::new();

    assert_eq!(
        wait.observe(HireOutcome::Seating { seat: wait_seat() }, None),
        None,
        "a seat with no receipt is progress, not an answer"
    );
    assert_eq!(
        wait.observe(
            HireOutcome::Created {
                seat: wait_seat(),
                receipt: wait_receipt(),
                granted: false,
            },
            Some(Err(CliError::Other(
                "receipt target claude-primary does not match create target 1958c6c448e05eed"
                    .into()
            ))),
        ),
        None,
        "a create whose evidence does not bind is not a hire answer"
    );

    assert!(
        matches!(wait.held(), HireOutcome::Seating { .. }),
        "the live seat must survive the failed verification"
    );
    let reason = wait
        .last_evidence_error()
        .expect("the wait must carry the verification failure out");
    assert!(
        reason.contains("does not match create target"),
        "got {reason}"
    );
}

#[test]
fn a_verified_create_answers_immediately_and_carries_no_error() {
    let mut wait = HireWait::new();
    let answered = wait
        .observe(
            HireOutcome::Created {
                seat: wait_seat(),
                receipt: wait_receipt(),
                granted: false,
            },
            Some(Ok(())),
        )
        .expect("a verified create is the answer");
    assert!(matches!(answered, HireOutcome::Created { .. }));
    assert_eq!(wait.last_evidence_error(), None);
}

/// A create whose provider metadata is merely late is held, as before — but
/// the reason is still recorded, so a wait that ends there says why.
#[test]
fn a_late_metadata_create_is_held_with_its_reason() {
    let mut wait = HireWait::new();
    assert_eq!(
        wait.observe(
            HireOutcome::Created {
                seat: wait_seat(),
                receipt: wait_receipt(),
                granted: false,
            },
            Some(Err(CliError::Unconfirmed(
                "no provider metadata for the receipt's target yet".into()
            ))),
        ),
        None
    );
    assert!(matches!(wait.held(), HireOutcome::Created { .. }));
    assert!(wait
        .last_evidence_error()
        .is_some_and(|error| error.contains("no provider metadata")));
}

#[test]
fn a_wait_that_saw_nothing_reports_unconfirmed_with_no_invented_error() {
    let mut wait = HireWait::new();
    assert_eq!(wait.observe(HireOutcome::Unconfirmed, None), None);
    assert_eq!(wait.held(), HireOutcome::Unconfirmed);
    assert_eq!(wait.last_evidence_error(), None);
}

// ── `bee sessions hire --check` publishes nothing (batch 2, A1.3) ────────────

/// Every path a `--check` run touched, recorded by a real HTTP server.
///
/// The claim is about the wire — "this run published nothing" — so it is
/// tested on the wire and not against a stubbed method.
#[derive(Default)]
struct CheckRecorder {
    paths: std::sync::Mutex<Vec<String>>,
}

async fn serve_recorder(recorder: std::sync::Arc<CheckRecorder>) -> String {
    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{Response, StatusCode};

    let app = axum::Router::new()
        .route(
            "/query",
            axum::routing::post(
                move |State(recorder): State<std::sync::Arc<CheckRecorder>>, _body: String| async move {
                    recorder
                        .paths
                        .lock()
                        .expect("path lock")
                        .push("/query".to_owned());
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from("[]"))
                        .expect("query response")
                },
            ),
        )
        .route(
            "/events",
            axum::routing::post(
                move |State(recorder): State<std::sync::Arc<CheckRecorder>>, _body: String| async move {
                    recorder
                        .paths
                        .lock()
                        .expect("path lock")
                        .push("/events".to_owned());
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(json!({"accepted": true, "id": "x"}).to_string()))
                        .expect("write response")
                },
            ),
        )
        .with_state(recorder.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    format!("http://{addr}")
}

#[allow(clippy::too_many_arguments)]
async fn run_hire_check(
    client: &crate::client::BuzzClient,
    genesis: &str,
    brief: &str,
    check: bool,
) -> Result<(), CliError> {
    super::crew_cmds::cmd_hire(
        client,
        CHANNEL,
        "6a8f1b2c-0000-4000-8000-000000000001",
        Some(genesis),
        "builder",
        Some("claude-primary"),
        Some("sonnet"),
        None,
        Some(brief),
        // `no_wait` is never read on the check path — it is consulted only
        // after the publish — so it carries no meaning for the green run. It
        // is `true` so the red run (the same test with the early return
        // removed) fails on the recorded POST instead of waiting out the
        // whole hire window.
        true,
        check,
        &super::crew_cmds::HireRouting {
            class: None,
            risk: None,
            profile: None,
            review_flags: None,
            challenger_sample: false,
            override_model: None,
            because: None,
        },
    )
    .await
}

#[tokio::test]
async fn hire_check_validates_and_publishes_nothing() {
    let recorder = std::sync::Arc::new(CheckRecorder::default());
    let url = serve_recorder(recorder.clone()).await;
    let client =
        crate::client::BuzzClient::new(url, nostr::Keys::generate(), None, None).expect("client");

    let outcome = run_hire_check(
        &client,
        &"a".repeat(64),
        "Rebase the lane and run the gate.",
        true,
    )
    .await;

    // The no-publish claim is checked BEFORE the exit code, so a run that
    // published and then succeeded still fails here.
    let paths = recorder.paths.lock().expect("path lock").clone();
    assert!(
        !paths.iter().any(|path| path == "/events"),
        "--check must publish nothing; it posted to {paths:?}"
    );
    outcome.expect("a well-formed hire checks clean");
}

#[tokio::test]
async fn hire_check_refuses_a_payload_the_relay_would_refuse() {
    let recorder = std::sync::Arc::new(CheckRecorder::default());
    let url = serve_recorder(recorder.clone()).await;
    let client =
        crate::client::BuzzClient::new(url, nostr::Keys::generate(), None, None).expect("client");

    let error = run_hire_check(&client, &"a".repeat(64), "   ", true)
        .await
        .expect_err("an empty brief is not a hire");
    assert!(matches!(error, CliError::Usage(_)), "got {error:?}");

    let paths = recorder.paths.lock().expect("path lock").clone();
    assert!(
        !paths.iter().any(|path| path == "/events"),
        "a refused check must publish nothing; it posted to {paths:?}"
    );
}

/// The printed facts are measured off the exact brief that would be signed.
#[test]
fn a_hire_check_report_measures_the_brief_against_the_relays_own_cap() {
    let brief = "Rebase the lane and run the gate.";
    let report = hire_check_report(&HireCheckRequest {
        channel: CHANNEL,
        session_ref: "6a8f1b2c-0000-4000-8000-000000000001",
        genesis_ref: &"a".repeat(64),
        role: "builder",
        brief,
        provider_instance: Some("claude-primary"),
        model: Some("sonnet"),
        routing: None,
        proposal_unavailable: None,
    });

    assert_eq!(report["published"], json!(false));
    assert_eq!(report["check"], json!(true));
    assert_eq!(report["briefBytes"], json!(brief.len()));
    assert_eq!(report["briefCapBytes"], json!(12 * 1024 - 16));
    assert_eq!(report["briefWithinCap"], json!(true));
    assert_eq!(report["role"], json!("builder"));
    assert_eq!(report["routed"], json!(false));
    assert_eq!(report["routing"], Value::Null);
}

/// The one field that measures rather than restates: a brief over the relay's
/// cap prints `briefWithinCap: false` beside its size, and the check then
/// refuses. Before, the report was built only after the builder had already
/// refused the payload, so the field could never print `false` (REVIEW-A1 F11).
#[test]
fn an_oversized_brief_is_reported_as_over_the_cap() {
    let brief = "x".repeat(12 * 1024 + 1);
    let report = hire_check_report(&HireCheckRequest {
        channel: CHANNEL,
        session_ref: "6a8f1b2c-0000-4000-8000-000000000001",
        genesis_ref: &"a".repeat(64),
        role: "builder",
        brief: &brief,
        provider_instance: None,
        model: None,
        routing: None,
        proposal_unavailable: None,
    });
    assert_eq!(report["briefBytes"], json!(brief.len()));
    assert_eq!(report["briefCapBytes"], json!(12 * 1024 - 16));
    assert_eq!(report["briefWithinCap"], json!(false));
    assert_eq!(report["published"], json!(false));
}

// ── Evidence-first hire receipt selection (batch 2, A1.6) ────────────────────

/// One complete, signed hire: genesis, seated create, provider receipt, and
/// the provider's own metadata — everything `verify_hire_evidence` demands.
struct SignedHire {
    events: Vec<Value>,
    channel: String,
    session: String,
    genesis: String,
    seat: HiredSeat,
    /// The host that signs seated creates — the genesis signer, so a rival
    /// create built with it is as founder-signed as the first.
    host: nostr::Keys,
    /// The provider that signs receipts and metadata for this umbrella.
    provider: nostr::Keys,
    /// The actor every create in this fixture seats.
    actor: String,
}

fn signed_hire(command_id: &str) -> SignedHire {
    signed_hire_with_status(command_id, ReceiptStatus::Created)
}

/// The same complete hire, with the provider answering in `status`.
///
/// Parameterized because a failure-class answer is a different fact from a
/// missing one, and the only way to test that difference is to build the
/// receipt the provider would actually sign for it.
fn signed_hire_with_status(command_id: &str, status: ReceiptStatus) -> SignedHire {
    use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
    use buzz_core::coding_session_payload::{
        Capabilities, LifecycleReceipt, SessionMetadata, SessionStatus, LIFECYCLE_RECEIPT_SCHEMA,
        METADATA_SCHEMA,
    };
    use buzz_sdk::builders::{
        build_coding_session_genesis, build_coding_session_lifecycle_command,
        build_coding_session_lifecycle_receipt, build_coding_session_metadata,
    };
    use nostr::Keys;
    use uuid::Uuid;

    let channel = Uuid::new_v4();
    let session = Uuid::new_v4().to_string();
    let host = Keys::generate();
    let provider = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let genesis_event =
        build_coding_session_genesis(channel, &CodingSessionGenesisPayload::new(session.clone()))
            .expect("genesis builder")
            .sign_with_keys(&host)
            .expect("sign genesis");
    let genesis = genesis_event.id.to_hex();
    // The receipt's instanceId is the provider's cryptographic short id; the
    // create's providerInstanceRef is the human alias. Different namespaces,
    // exactly as on the wire.
    let seat_target = CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "1958c6c448e05eed".into(),
        session_id: "session-1".into(),
        generation: 1,
    };
    let create_payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: command_id.into(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(session.clone()),
            genesis_ref: Some(genesis.clone()),
            provider_instance_ref: "claude-primary".try_into().expect("alias"),
            provider_authority_pubkey: provider.public_key().to_hex(),
            model: None,
            title: None,
            initial_turn: Some("build it".into()),
            actor: Some(actor.clone()),
            role: Some("builder".into()),
            hire_ref: None,
            routing: None,
        },
    };
    let create = build_coding_session_lifecycle_command(channel, &create_payload)
        .expect("create builder")
        .sign_with_keys(&host)
        .expect("sign create");
    let created = matches!(
        status,
        ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
    );
    let receipt_payload = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
        command_id: command_id.into(),
        status,
        // A provider that created nothing names no session, and says why.
        session: created.then(|| seat_target.clone()),
        error: (!created).then(|| buzz_core::coding_session_payload::ReceiptError {
            code: "ACTOR_UNAVAILABLE".into(),
            message: "no key for that actor".into(),
        }),
        turn_id: None,
    };
    let receipt = build_coding_session_lifecycle_receipt(
        channel,
        command_id,
        &serde_json::to_string(&receipt_payload).expect("serialize receipt"),
    )
    .expect("receipt builder")
    .sign_with_keys(&provider)
    .expect("sign receipt");
    let metadata_payload = SessionMetadata {
        schema: METADATA_SCHEMA.into(),
        session: seat_target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: Some(actor.clone()),
        role: Some("builder".into()),
        provider: Some("claude-primary".try_into().expect("alias")),
        runtime: Some("claude".try_into().expect("runtime")),
        model: None,
        status: SessionStatus::Idle,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: Some(session.clone()),
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        handover: None,
    };
    let metadata = build_coding_session_metadata(
        channel,
        &seat_target,
        &serde_json::to_string(&metadata_payload).expect("serialize metadata"),
    )
    .expect("metadata builder")
    .sign_with_keys(&provider)
    .expect("sign metadata");

    let seat = HiredSeat {
        event_id: create.id.to_hex(),
        create_signer: host.public_key().to_hex(),
        command_id: command_id.into(),
        actor: actor.clone(),
        role: "builder".into(),
        session_ref: session.clone(),
        genesis_ref: genesis.clone(),
        provider_authority_pubkey: provider.public_key().to_hex(),
        provider_instance_ref: "claude-primary".try_into().expect("alias"),
        model: None,
        at: create.created_at.as_secs() as i64,
        raw: serde_json::to_value(&create).expect("create JSON"),
    };
    SignedHire {
        events: vec![
            serde_json::to_value(genesis_event).expect("genesis JSON"),
            serde_json::to_value(create).expect("create JSON"),
            serde_json::to_value(receipt).expect("receipt JSON"),
            serde_json::to_value(metadata).expect("metadata JSON"),
        ],
        channel: channel.to_string(),
        session,
        genesis,
        seat,
        host,
        provider,
        actor,
    }
}

/// A second founder-signed seated create for the same umbrella and role.
///
/// `answered` decides whether the provider also signs a bound `created`
/// receipt and the metadata that makes it verify. Its `created_at` is pushed
/// `age_offset` seconds into the past so a rule that reads the author's clock
/// picks it — which is the whole point of the T2.5 red.
fn rival_seated_create(
    hire: &SignedHire,
    command_id: &str,
    session_id: &str,
    answered: bool,
    age_offset: u64,
) -> (Vec<Value>, HiredSeat) {
    use buzz_core::coding_session_payload::{
        Capabilities, LifecycleReceipt, SessionMetadata, SessionStatus, LIFECYCLE_RECEIPT_SCHEMA,
        METADATA_SCHEMA,
    };
    use buzz_sdk::builders::{
        build_coding_session_lifecycle_command, build_coding_session_lifecycle_receipt,
        build_coding_session_metadata,
    };
    use uuid::Uuid;

    let channel = Uuid::parse_str(&hire.channel).expect("channel UUID");
    let target = CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "1958c6c448e05eed".into(),
        session_id: session_id.into(),
        generation: 1,
    };
    let create_payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: command_id.into(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(hire.session.clone()),
            genesis_ref: Some(hire.genesis.clone()),
            provider_instance_ref: "claude-primary".try_into().expect("alias"),
            provider_authority_pubkey: hire.provider.public_key().to_hex(),
            model: None,
            title: None,
            initial_turn: Some("build it".into()),
            actor: Some(hire.actor.clone()),
            role: Some("builder".into()),
            hire_ref: None,
            routing: None,
        },
    };
    let create = build_coding_session_lifecycle_command(channel, &create_payload)
        .expect("create builder")
        .custom_created_at(nostr::Timestamp::now() - age_offset)
        .sign_with_keys(&hire.host)
        .expect("sign create");
    let seat = HiredSeat {
        event_id: create.id.to_hex(),
        create_signer: hire.host.public_key().to_hex(),
        command_id: command_id.into(),
        actor: hire.actor.clone(),
        role: "builder".into(),
        session_ref: hire.session.clone(),
        genesis_ref: hire.genesis.clone(),
        provider_authority_pubkey: hire.provider.public_key().to_hex(),
        provider_instance_ref: "claude-primary".try_into().expect("alias"),
        model: None,
        at: create.created_at.as_secs() as i64,
        raw: serde_json::to_value(&create).expect("create JSON"),
    };
    let mut events = vec![serde_json::to_value(create).expect("create JSON")];
    if answered {
        let receipt_payload = LifecycleReceipt {
            schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
            command_id: command_id.into(),
            status: ReceiptStatus::Created,
            session: Some(target.clone()),
            error: None,
            turn_id: None,
        };
        let receipt = build_coding_session_lifecycle_receipt(
            channel,
            command_id,
            &serde_json::to_string(&receipt_payload).expect("serialize receipt"),
        )
        .expect("receipt builder")
        .sign_with_keys(&hire.provider)
        .expect("sign receipt");
        let metadata_payload = SessionMetadata {
            schema: METADATA_SCHEMA.into(),
            session: target.clone(),
            project_ref: None,
            repo_ref: None,
            title: None,
            agent_ref: Some(hire.actor.clone()),
            role: Some("builder".into()),
            provider: Some("claude-primary".try_into().expect("alias")),
            runtime: Some("claude".try_into().expect("runtime")),
            model: None,
            status: SessionStatus::Idle,
            branch: None,
            capabilities: Capabilities::v1_claude(),
            session_ref: Some(hire.session.clone()),
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
            turn_budget: None,
            routing: None,
            bee_stamp: None,
            pack_ref: None,
            handover: None,
        };
        let metadata = build_coding_session_metadata(
            channel,
            &target,
            &serde_json::to_string(&metadata_payload).expect("serialize metadata"),
        )
        .expect("metadata builder")
        .sign_with_keys(&hire.provider)
        .expect("sign metadata");
        events.push(serde_json::to_value(receipt).expect("receipt JSON"));
        events.push(serde_json::to_value(metadata).expect("metadata JSON"));
    }
    (events, seat)
}

/// Assess every candidate for the fixture's role, in the CLI's own order.
fn assess_all(hire: &SignedHire, events: &[Value]) -> Vec<super::crew_cmds::AssessedCandidate> {
    let (receipts, _) = decode_receipts(events);
    super::crew::hired_seat_candidates(events, &hire.session, "builder", 0)
        .into_iter()
        .map(|seat| {
            super::crew_cmds::assess_candidate(
                events,
                &hire.channel,
                &hire.session,
                &hire.genesis,
                &receipts,
                seat,
            )
        })
        .collect()
}

/// The single seated create a fixture with exactly one candidate produces.
///
/// [`super::crew::hired_seat_candidates`] deliberately returns every
/// candidate; these fixtures build one, and the assertion keeps that true so a
/// fixture that quietly grew a second create cannot pass by accident.
fn sole_hired_seat(events: &[Value], umbrella: &str, role: &str, since: i64) -> Option<HiredSeat> {
    let mut candidates = super::crew::hired_seat_candidates(events, umbrella, role, since);
    assert!(
        candidates.len() <= 1,
        "this fixture is meant to hold one candidate, found {}",
        candidates.len()
    );
    candidates.pop()
}

/// **T2.5.** The CLI must not pick a hire's answer by the author's own clock.
///
/// `find_hired_seat` kept the minimum `(created_at, event_id)` and handed only
/// that create to the evidence check, so an earlier create that no provider
/// ever answered shadowed the seat that was actually running — and `created_at`
/// is a claim nothing signs into agreement with the wire, so anyone able to
/// publish a 44221 for the role could park the hire there permanently.
#[test]
fn a_hire_takes_the_create_whose_receipt_verifies_not_the_oldest() {
    let hire = signed_hire("create-hired");
    let (rival_events, rival) =
        rival_seated_create(&hire, "create-silent", "session-rival", false, 600);
    let mut events = hire.events.clone();
    events.extend(rival_events);

    let candidates = super::crew::hired_seat_candidates(&events, &hire.session, "builder", 0);
    assert_eq!(
        candidates.len(),
        2,
        "both seated creates are candidates; neither is filtered out by time"
    );
    assert!(
        rival.at < hire.seat.at,
        "the silent create is the older one, so a time rule would pick it"
    );

    let assessed = assess_all(&hire, &events);
    let chosen = match super::crew_cmds::select_hire_answer(&assessed) {
        super::crew_cmds::HireSelection::Answer(chosen) => chosen.expect("an answer"),
        super::crew_cmds::HireSelection::Ambiguous(_) => {
            panic!("only one create is answered, so nothing is ambiguous")
        }
    };
    assert_eq!(
        chosen.seat.command_id, "create-hired",
        "the create whose receipt verifies is the hire's answer, not the older silent one"
    );
    let outcome = fold_hire(
        Some(chosen.seat.clone()),
        chosen.verified.clone().or_else(|| chosen.failed.clone()),
        None,
        true,
    );
    assert!(
        matches!(outcome, HireOutcome::Created { .. }),
        "the running seat is reported, not a seating outcome: {outcome:?}"
    );
}

/// Two verified creates are two signed facts that disagree, and the CLI
/// refuses to break the tie rather than reading somebody's clock.
#[test]
fn two_verifying_creates_refuse_to_be_chosen_between() {
    let hire = signed_hire("create-hired");
    let (rival_events, _) = rival_seated_create(&hire, "create-second", "session-rival", true, 600);
    let mut events = hire.events.clone();
    events.extend(rival_events);

    let assessed = assess_all(&hire, &events);
    let verifying = match super::crew_cmds::select_hire_answer(&assessed) {
        super::crew_cmds::HireSelection::Ambiguous(verifying) => verifying,
        super::crew_cmds::HireSelection::Answer(chosen) => panic!(
            "two verified creates must not be resolved into one: {:?}",
            chosen.map(|candidate| candidate.seat.command_id.clone())
        ),
    };
    assert_eq!(verifying.len(), 2);

    let seats: Vec<&HiredSeat> = verifying.iter().map(|candidate| &candidate.seat).collect();
    let message = super::crew::ambiguous_hire_refusal(&hire.channel, &hire.session, &seats);
    assert!(
        message.starts_with("two seated creates for this role verify against their own receipts ("),
        "{message}"
    );
    assert!(
        message.contains("the CLI will not choose between them — repair the seat you meant with "),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "bee sessions seat-repair --channel {} --session-ref {}",
            hire.channel, hire.session
        )),
        "{message}"
    );
    for seat in &seats {
        assert!(message.contains(&seat.event_id), "{message}");
    }
    assert!(
        message.contains(&format!("--actor {}", hire.actor)),
        "both creates seat the same actor, so the remedy names it: {message}"
    );

    let outcome = HireOutcome::Ambiguous {
        message: message.clone(),
    };
    assert_eq!(super::crew::hire_exit_code(&outcome), 1);
    let report = super::crew::hire_report(&outcome, true, &hire.channel);
    assert_eq!(report.status, "ambiguous");
    assert_eq!(report.detail, message);
    // An ambiguity ends the wait: another poll can only find more of them.
    let mut wait = super::crew::HireWait::new();
    assert!(wait.observe(outcome, None).is_some());
}

/// A 44224 that names the create's commandId and is signed by somebody else —
/// the shape that could deny a hire the moment selection trusted `created_at`.
fn unbound_receipt(channel: &str, command_id: &str, created_at_offset: u64) -> Value {
    use buzz_core::coding_session_payload::{LifecycleReceipt, LIFECYCLE_RECEIPT_SCHEMA};
    use buzz_sdk::builders::build_coding_session_lifecycle_receipt;
    use nostr::Keys;
    use uuid::Uuid;

    let stranger = Keys::generate();
    let payload = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
        command_id: command_id.into(),
        status: ReceiptStatus::Failed,
        session: None,
        error: Some(buzz_core::coding_session_payload::ReceiptError {
            code: "ACTOR_UNAVAILABLE".into(),
            message: "no key for that actor".into(),
        }),
        turn_id: None,
    };
    let channel = Uuid::parse_str(channel).expect("channel UUID");
    let mut event = serde_json::to_value(
        build_coding_session_lifecycle_receipt(
            channel,
            command_id,
            &serde_json::to_string(&payload).expect("serialize receipt"),
        )
        .expect("receipt builder")
        .custom_created_at(nostr::Timestamp::now() + created_at_offset)
        .sign_with_keys(&stranger)
        .expect("sign receipt"),
    )
    .expect("receipt JSON");
    // The forgery does not have to be well signed to be *selected* by a rule
    // that sorts on time; it only has to be newest.
    if let Some(object) = event.as_object_mut() {
        object.insert(
            "created_at".into(),
            json!(nostr::Timestamp::now().as_secs() + created_at_offset),
        );
    }
    event
}

/// The rule A1.6 installs: the newest receipt does not win — the bound one
/// does. Before this, `newest_create_receipt` took the newest by the author's
/// own `created_at` and only then checked the binding, so any party able to
/// publish a 44224 carrying the commandId could deny the hire.
#[test]
fn a_newer_unbound_receipt_never_displaces_the_bound_one() {
    let hire = signed_hire("create-hired");
    let mut events = hire.events.clone();
    events.push(unbound_receipt(&hire.channel, "create-hired", 600));
    let (receipts, _) = decode_receipts(&events);

    let assessed = super::crew_cmds::assess_candidate(
        &events,
        &hire.channel,
        &hire.session,
        &hire.genesis,
        &receipts,
        hire.seat.clone(),
    );

    let verified = assessed
        .verified
        .as_ref()
        .expect("the bound created receipt is the answer");
    assert_eq!(verified.status, ReceiptStatus::Created);
    assert_eq!(
        assessed.unbound, 1,
        "the stranger's receipt is ignored and counted, not hidden"
    );
    assert!(
        assessed.failed.is_none(),
        "an unbound failure is not a refusal: {:?}",
        assessed.failed
    );

    let outcome = fold_hire(
        Some(assessed.seat),
        assessed.verified.or(assessed.failed),
        None,
        true,
    );
    assert!(
        matches!(outcome, HireOutcome::Created { .. }),
        "the hire is created, not failed: {outcome:?}"
    );
}

/// A bound failure-class receipt is the provider's own answer, and it settles
/// the hire as failed rather than leaving it seating.
///
/// The test this replaces built a `created` receipt and asserted only that a
/// receipt was found, so the sentence it was named for — "a bound Failed-class
/// receipt is a refusal" — was never exercised (REVIEW-A1 F6). This builds the
/// receipt a provider signs when it refuses, and follows it all the way to the
/// exit code.
#[test]
fn a_bound_failure_receipt_is_a_refusal_not_a_missing_answer() {
    let hire = signed_hire_with_status("create-refused", ReceiptStatus::Failed);
    let (receipts, _) = decode_receipts(&hire.events);
    let assessed = super::crew_cmds::assess_candidate(
        &hire.events,
        &hire.channel,
        &hire.session,
        &hire.genesis,
        &receipts,
        hire.seat.clone(),
    );

    assert!(
        assessed.verified.is_none(),
        "a refusal is not a verified create: {:?}",
        assessed.verified
    );
    let failed = assessed
        .failed
        .as_ref()
        .expect("the provider's own refusal is bound and is the answer");
    assert_eq!(failed.status, ReceiptStatus::Failed);
    assert_eq!(
        assessed.unbound, 0,
        "the provider signed it; nothing here is unbound"
    );

    let outcome = fold_hire(
        Some(assessed.seat),
        assessed.verified.or(assessed.failed),
        None,
        true,
    );
    assert!(
        matches!(outcome, HireOutcome::Failed { .. }),
        "the hire failed and says so, rather than waiting out its window: {outcome:?}"
    );
    assert_eq!(super::crew::hire_exit_code(&outcome), 1, "{outcome:?}");
}

// ── What a hire says about the evidence it refused (REVIEW-A1 F7) ───────────

/// A hire that ended on the host's own verified answer still discloses the
/// candidate it refused first — in the past tense, because on a `created` hire
/// the present tense reads as a failure that did not happen.
#[test]
fn an_overtaken_refusal_is_disclosed_as_history_on_a_successful_hire() {
    let sentence = evidence_disclosure(
        true,
        Some("provider receipt is not bound to the signed seated create"),
        1,
    );
    assert!(
        sentence.contains("was refused") || sentence.contains("were refused"),
        "past tense on a hire that then verified: {sentence}"
    );
    assert!(
        !sentence.contains("The last hire evidence check refused the host's answer"),
        "the present-tense sentence belongs to a hire that is still holding: {sentence}"
    );
    assert!(
        sentence.contains("provider receipt is not bound to the signed seated create"),
        "the reason itself is never dropped: {sentence}"
    );
    assert!(
        sentence.contains("1 receipt(s)"),
        "and the unbound count is still named: {sentence}"
    );
}

/// A hire still holding `seating` says why it is holding, in the present
/// tense: this is the sentence whose absence read as a slow host.
#[test]
fn a_hire_left_holding_says_why_in_the_present_tense() {
    let sentence = evidence_disclosure(false, Some("no provider metadata for the seat"), 0);
    assert!(
        sentence.contains(
            "The last hire evidence check refused the host's answer: no provider \
             metadata for the seat"
        ),
        "{sentence}"
    );
}

/// Nothing refused, nothing unbound, nothing to say.
#[test]
fn a_quiet_wait_adds_no_sentence() {
    assert!(evidence_disclosure(true, None, 0).is_empty());
    assert!(evidence_disclosure(false, None, 0).is_empty());
}

// ── Role-seat revocation (batch 2, A1.7) ────────────────────────────────────

fn projected(seats: &[(&str, &str)]) -> super::operations::ProjectedAuthority {
    super::operations::ProjectedAuthority {
        grants: Vec::new(),
        seats: seats
            .iter()
            .map(|(actor, role)| {
                buzz_core::coding_session_team_transaction::CodingSessionTeamActiveSeat {
                    actor_pubkey: (*actor).to_owned(),
                    role: (*role).to_owned(),
                }
            })
            .collect(),
        seat_grant_refs: std::collections::BTreeMap::new(),
        policy_grants: Vec::new(),
        head_event_id: Some("d".repeat(64)),
        head_seq: 4,
        // No handover has happened on this fixture's session, which is a
        // different answer from "a claim was made and voided" (Lane C).
        claim: buzz_core::coding_session_authority_claim::ClaimState::NoClaim,
        claim_since: None,
        grant_accepted_at: std::collections::BTreeMap::new(),
        seat_accepted_at: std::collections::BTreeMap::new(),
    }
}

#[test]
fn revoking_a_seat_nobody_holds_is_refused_not_written() {
    let error = super::crew_cmds::decide_seat_revoke(&projected(&[]), &pk(BOB), "builder")
        .expect_err("there is nothing to revoke");
    assert_eq!(crate::error::exit_code(&error), 1, "{error}");
    assert!(
        error.to_string().contains("holds no active role seat"),
        "{error}"
    );
}

/// A revoke that named the wrong role would otherwise be forwarded to the
/// relay and come back as a shape error. Refuse first, naming what is held.
#[test]
fn revoking_the_wrong_role_names_the_one_the_actor_actually_holds() {
    let error = super::crew_cmds::decide_seat_revoke(
        &projected(&[(&pk(BOB), "verifier")]),
        &pk(BOB),
        "builder",
    )
    .expect_err("the actor holds a different role");
    assert_eq!(crate::error::exit_code(&error), 1, "{error}");
    let text = error.to_string();
    assert!(text.contains("holds role verifier"), "{text}");
    assert!(text.contains("not builder"), "{text}");
}

#[test]
fn revoking_the_exact_role_is_allowed() {
    assert_eq!(
        super::crew_cmds::decide_seat_revoke(
            &projected(&[(&pk(BOB), "builder"), (&pk(ALICE), "lead")]),
            &pk(BOB),
            "builder",
        )
        .expect("the actor holds exactly this role"),
        super::crew_cmds::SeatRevokeDecision::Append
    );
}

/// `seat-repair`'s `ambiguous` remedy may only name commands that exist and
/// that converge. Before A1.7 it pointed at the founder's Desktop app because
/// no CLI verb could write a role seat.
#[test]
fn the_ambiguous_remedy_names_the_seat_verbs_that_now_exist() {
    let source = include_str!("crew_cmds.rs");
    assert!(
        source.contains("revoke-seat --channel {channel_id} --genesis {genesis_ref}"),
        "the ambiguous remedy must name the revoke that makes it converge"
    );
    assert!(
        !source.contains("`bee sessions grant` covers only the"),
        "the remedy still claims `grant` cannot write a role seat"
    );
    assert!(
        source.contains("a grant alone will not converge while the disputed role"),
        "the remedy must stay honest about needing the revoke first"
    );
}

// ── B2 (batch 2): the requester travels, and the four identity words are typed ─

/// COMMS-MAP §3, fact 1: a hired seat's brief arrived **unattributed** —
/// nothing on the wire recorded which lead asked for the seat, and the only
/// trace was a 16-byte `"[From the lead] "` prefix added in TypeScript. B2.2
/// makes `bee sessions hire` always write `requestedBy`, and it is always the
/// key that is about to sign: the one place that cannot be mistaken about the
/// answer.
#[test]
fn a_hire_writes_the_requester_as_the_eighth_key_after_the_brief() {
    let genesis = "12".repeat(32);
    let requester = "ab".repeat(32);
    let payload = hire_payload(
        "hire-8",
        UMBRELLA_HIRE,
        &genesis,
        "builder",
        None,
        None,
        "Rebase the lane.",
        Some(requester.as_str()),
        None,
    );
    let content = serde_json::to_string(&payload).expect("serialize");
    assert!(
        content.contains(&format!(
            r#""brief":"Rebase the lane.","requestedBy":"{requester}""#
        )),
        "requestedBy is the eighth key, immediately after the brief: {content}"
    );
    // And the relay's own strict decoder accepts it.
    let decoded =
        buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &content,
        )
        .expect("decodes");
    assert_eq!(decoded.hire_requested_by(), Some(requester.as_str()));
    assert_eq!(
        decoded.hire_requester_matches_signer(&requester),
        Some(true)
    );
    // The claim is a claim: a different signer is DISPUTED, not silently
    // dropped and not silently believed (POLICY.md §5).
    assert_eq!(
        decoded.hire_requester_matches_signer(&"cd".repeat(32)),
        Some(false)
    );
}

/// A hire that names no requester is unchanged: the key is **omitted**, never
/// written as an explicit `null`, so every hire signed before B2 stays
/// byte-identical.
#[test]
fn a_hire_with_no_requester_is_byte_identical_to_the_seven_key_form() {
    let genesis = "12".repeat(32);
    let payload = hire_payload(
        "hire-7",
        UMBRELLA_HIRE,
        &genesis,
        "builder",
        None,
        None,
        "Rebase the lane.",
        None,
        None,
    );
    let content = serde_json::to_string(&payload).expect("serialize");
    assert!(!content.contains("requestedBy"), "got {content}");
}

/// B2.1: the four identity words are the field's own type now, not an accessor
/// over a `String`. The newtypes are `#[serde(transparent)]`, so **the wire did
/// not change** — that is the whole claim, and the test that proves it is a
/// round trip of a value no constructor would accept.
#[test]
fn typing_the_identity_fields_did_not_tighten_the_wire() {
    let genesis = "12".repeat(32);
    // `claude\tprimary` is a signed shape that decoded yesterday. It must still
    // decode: a newtype that refused it would have made B2's field flip a
    // wire-breaking change disguised as a refactor.
    let content = format!(
        concat!(
            r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"hire-tab","#,
            r#""action":{{"type":"session.hire","sessionRef":"{umbrella}","#,
            r#""genesisRef":"{genesis}","role":"builder","#,
            r#""providerInstanceRef":"claude\tprimary","model":null,"#,
            r#""brief":"Rebase the lane."}}}}"#
        ),
        umbrella = UMBRELLA_HIRE,
        genesis = genesis,
    );
    let decoded =
        buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &content,
        )
        .expect("a tab in the alias still decodes");
    assert_eq!(
        serde_json::to_string(&decoded).expect("serialize"),
        content,
        "the newtype re-serializes the exact wire bytes"
    );
    // And the odd shape is *reported* rather than refused.
    let alias = decoded
        .provider_instance_alias()
        .expect("alias reads")
        .expect("alias present");
    assert!(!alias.is_canonical());
}

// ── Fix round 1: F3, the async seam the aggregation actually lives on ───────

/// A relay that answers `/query` from a fixed event list, filtering on `kinds`
/// exactly as the real one does for these reads.
///
/// The lane tested `select_hire_answer` — the pure seam it built — but the two
/// reductions T2.5 introduced (`unbound_receipts` summed across candidates,
/// `rejection` picked from among them) live only in `read_hire_answer`, which
/// is `async`. That is precisely where the behaviour drifted (REVIEW-L1 F3), so
/// it is tested here on the real function.
async fn serve_hire_events(events: Vec<Value>) -> String {
    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{Response, StatusCode};
    use axum::Router;
    use std::sync::Arc;

    let events = Arc::new(events);
    let app = Router::new()
        .route(
            "/",
            axum::routing::get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(json!({}).to_string()))
                    .expect("info response")
            }),
        )
        .route(
            "/query",
            axum::routing::post(
                |State(events): State<Arc<Vec<Value>>>, body: String| async move {
                    let filter: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    let wanted: Vec<u64> = filter
                        .get("kinds")
                        .or_else(|| filter.get(0).and_then(|first| first.get("kinds")))
                        .and_then(Value::as_array)
                        .map(|kinds| kinds.iter().filter_map(Value::as_u64).collect())
                        .unwrap_or_default();
                    let matched: Vec<Value> = events
                        .iter()
                        .filter(|event| {
                            wanted.is_empty()
                                || event
                                    .get("kind")
                                    .and_then(Value::as_u64)
                                    .is_some_and(|kind| wanted.contains(&kind))
                        })
                        .cloned()
                        .collect();
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(Value::Array(matched).to_string()))
                        .expect("query response")
                },
            ),
        )
        .with_state(events);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    format!("http://{addr}")
}

/// **REVIEW-L1 F3.** Two seated creates for one role, only one of them
/// verifiable, and an unbound receipt naming the *other* create's commandId.
///
/// Three things this pins, all of them only reachable through the async seam:
/// the hire takes the verifying create; the unbound count aggregates across
/// candidates and the sentence that renders it no longer claims the receipts
/// named "this create's" commandId; and **a successful hire reports
/// `lastEvidenceError: null`** — a rejection belonging to a create this hire
/// did not take is not this hire's evidence error.
#[tokio::test]
async fn the_hire_seam_aggregates_across_candidates_without_misattributing_them() {
    let hire = signed_hire("create-hired");
    // The rival gets a bound `created` receipt but NO metadata: its receipt
    // binds, so it is assessed, and `verify_hire_evidence` then refuses it —
    // a rejection that belongs to a create this hire does not take.
    let (rival_events, rival) =
        rival_seated_create(&hire, "create-silent", "session-rival", true, 600);
    let mut events = hire.events.clone();
    events.extend(rival_events.into_iter().filter(|event| {
        !(event.get("kind").and_then(Value::as_u64)
            == Some(u64::from(buzz_core::kind::KIND_CODING_SESSION_METADATA))
            && event
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("session-rival")))
    }));
    // A forged receipt naming the RIVAL create's commandId. It is bound to
    // nothing, so it decides nothing — but it is real evidence that somebody
    // published it, and the count must say so without claiming it named the
    // create the hire actually took.
    events.push(unbound_receipt(&hire.channel, "create-silent", 900));

    let url = serve_hire_events(events).await;
    let client =
        crate::client::BuzzClient::new(url, nostr::Keys::generate(), None, None).expect("client");
    let answer = super::crew_cmds::read_hire_answer(
        &client,
        &hire.channel,
        &hire.session,
        &hire.genesis,
        "builder",
        0,
    )
    .await
    .expect("the seam reads the channel");

    let HireOutcome::Created { seat, .. } = &answer.outcome else {
        panic!("the verifying create is the answer: {:?}", answer.outcome);
    };
    assert_eq!(seat.command_id, "create-hired");
    assert_ne!(rival.command_id, seat.command_id);
    assert_eq!(
        answer.unbound_receipts, 1,
        "the unbound receipt on the other candidate is counted, not hidden"
    );
    assert_eq!(
        answer.rejection, None,
        "the rival's create was refused, not this one: a hire that verified reports \
         lastEvidenceError null rather than somebody else's failure — got {:?}",
        answer.rejection
    );

    // The sentence the caller prints must not claim the receipt named the
    // create this hire took — it named a different one.
    let sentence = evidence_disclosure(true, answer.rejection.as_deref(), answer.unbound_receipts);
    assert!(
        !sentence.contains("this create's commandId"),
        "with two candidates the count spans both, so the sentence cannot speak for one: \
         {sentence}"
    );
    assert!(
        sentence.contains("seated create") && sentence.contains("for this role"),
        "the sentence says what the count is about: {sentence}"
    );
}

/// The single-candidate case — every ordinary hire — still reports exactly what
/// it did before, including a `lastEvidenceError` that IS about its own create.
#[tokio::test]
async fn one_candidate_still_reports_its_own_evidence_error() {
    let hire = signed_hire("create-hired");
    let mut events = hire.events.clone();
    // A receipt bound to this create's commandId whose target does not match
    // the provider's metadata: it binds, so it is assessed, and it fails
    // verification — a rejection that genuinely belongs to this hire.
    events.retain(|event| {
        event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(buzz_core::kind::KIND_CODING_SESSION_METADATA))
    });

    let url = serve_hire_events(events).await;
    let client =
        crate::client::BuzzClient::new(url, nostr::Keys::generate(), None, None).expect("client");
    let answer = super::crew_cmds::read_hire_answer(
        &client,
        &hire.channel,
        &hire.session,
        &hire.genesis,
        "builder",
        0,
    )
    .await
    .expect("the seam reads the channel");

    assert!(
        matches!(answer.outcome, HireOutcome::Seating { .. }),
        "no metadata means the evidence check refuses, so the hire is still seating: {:?}",
        answer.outcome
    );
    let rejection = answer
        .rejection
        .as_deref()
        .expect("the chosen create's own refusal is reported");
    assert!(
        rejection.contains("create-hired"),
        "the rejection names the create it is about: {rejection}"
    );
    assert_eq!(answer.unbound_receipts, 0);

    let sentence = evidence_disclosure(false, Some(rejection), 0);
    assert!(
        sentence.contains("The last hire evidence check refused the host's answer"),
        "{sentence}"
    );
}
