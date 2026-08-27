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
use buzz_core::coding_session_payload::{
    Capabilities, LifecycleReceipt, ReceiptError, ReceiptStatus, SessionMetadata, SessionStatus,
    TranscriptEnvelope, LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA, NO_LIVE_EXECUTION,
    NO_TURN_IN_FLIGHT, QUEUE_FULL, STALE_GENERATION,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_LEASE, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
};

use super::crew::*;
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
        runtime: Some("claude".into()),
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
        provider: Some("claude-primary".into()),
        runtime: Some("claude".into()),
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
            let turns = diagnose_turns(&records, &stages);
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
/// naming the commandId `--readdress` takes.
#[test]
fn doctor_still_names_readdress_for_a_recoverable_refusal() {
    let addressed = target("s-1", 1);
    for (status, code) in [
        (ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION),
        (ReceiptStatus::TurnRefused, STALE_GENERATION),
    ] {
        let events = vec![receipt_event(
            "r-1",
            1_005,
            "cmd-1",
            status,
            &addressed,
            Some((code, "did not run")),
            None,
        )];
        let (receipts, _) = decode_receipts(&events);
        let turns = diagnose_turns(&[], &newest_turn_stages(&receipts));
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
