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
            provider_instance_ref: "instance-1".into(),
            provider_authority_pubkey: pk(provider_authority),
            model: Some("claude-opus".into()),
            title: Some("a session".into()),
            initial_turn: None,
            actor: None,
            role: None,
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
            provider_instance_ref: "claude-primary".into(),
            provider_authority_pubkey: pk(PROVIDER),
            model: model.map(str::to_owned),
            title: Some("a session".into()),
            initial_turn: Some("[From the lead] Rebase the lane.".into()),
            actor: Some(pk(actor)),
            role: Some(role.to_owned()),
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

    let seat = find_hired_seat(&events, UMBRELLA_HIRE, "builder", 1_000).expect("finds the seat");
    assert_eq!(seat.command_id, "create-hired");
    assert_eq!(seat.event_id, "the-seat");
    assert_eq!(seat.actor, pk(BOB));
    assert_eq!(seat.role, "builder");
    assert_eq!(seat.model.as_deref(), Some("claude-opus"));
    assert_eq!(seat.provider_instance_ref, "claude-primary");

    assert_eq!(
        find_hired_seat(&events, UMBRELLA_HIRE, "runner", 1_000),
        None
    );
    // Nothing after the cutoff: the old seat is not re-reported.
    assert_eq!(
        find_hired_seat(&events, UMBRELLA_HIRE, "builder", 2_000),
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
        find_hired_seat(
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
        newest_create_receipt(&created_receipts, "create-hired"),
        None,
    );
    let HireOutcome::Created { seat: row, receipt } = &outcome else {
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
        newest_create_receipt(&failed_receipts, "create-hired"),
        None,
    );
    let HireOutcome::Failed { receipt, .. } = &outcome else {
        panic!("expected a failed hire, got {outcome:?}")
    };
    assert_eq!(receipt.error_code.as_deref(), Some("ACTOR_UNAVAILABLE"));

    // A create published but not yet answered is neither created nor failed.
    assert!(matches!(
        fold_hire(Some(seat("c")), None, None),
        HireOutcome::Seating { .. }
    ));
    // Nothing at all is unconfirmed — never a guess in either direction.
    assert_eq!(fold_hire(None, None, None), HireOutcome::Unconfirmed);
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

    let outcome = fold_hire(None, None, Some(refusal));
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
    let seat = find_hired_seat(
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
        newest_create_receipt(&records, "create-hired")
    };

    let cases = [
        (
            fold_hire(
                Some(seat.clone()),
                receipt(ReceiptStatus::Created, None),
                None,
            ),
            "created",
            0,
        ),
        (
            fold_hire(
                Some(seat.clone()),
                receipt(ReceiptStatus::Failed, Some(("ACTOR_UNAVAILABLE", "no key"))),
                None,
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
            ),
            "refused",
            1,
        ),
        (fold_hire(Some(seat), None, None), "seating", 5),
        (fold_hire(None, None, None), "unconfirmed", 5),
    ];
    for (outcome, word, code) in cases {
        let report = hire_report(&outcome, true);
        assert_eq!(report.status, word, "{outcome:?}");
        assert_eq!(hire_exit_code(&outcome), code, "{outcome:?}");
        assert!(!report.detail.is_empty(), "{outcome:?} said nothing");
    }

    // Not waiting is its own sentence: nobody was asked, rather than nobody
    // answered.
    let unwaited = hire_report(&HireOutcome::Unconfirmed, false);
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
        Some("claude-primary"),
        Some("claude-opus"),
        "Rebase the lane.",
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
