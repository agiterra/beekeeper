//! Tests for `bee sessions audit`.
//!
//! The fixtures are built from the shapes the 2026-09-01 live run actually put
//! on the wire (`docs/SESSION_STATE.md` item 103): Keystone's 64-tool turn with
//! a complete `usage` block, and Bob's 88-tool turn — the two turns whose
//! numbers had to be read by hand because nothing rendered them.

use serde_json::{json, Value};

use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::coding_session_payload::{
    result_item, ResultSubtype, TranscriptEnvelope, TurnCost, TurnUsageReport,
};

use super::audit::{audit_report, compact_lines, MAX_AUDIT_ITEMS_PER_EXECUTION};
use super::crew::{CrewExecution, Liveness};
use super::TranscriptRecord;

const KEYSTONE: &str = "11";
const BOB: &str = "22";

fn pk(seed: &str) -> String {
    seed.repeat(32)
}

fn target(session_id: &str) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "1958c6c448e05eed".into(),
        session_id: session_id.into(),
        generation: 1,
    }
}

fn execution(session_id: &str, actor: &str, role: &str) -> CrewExecution {
    let target = target(session_id);
    CrewExecution {
        target_key: coding_session_target_key(&target),
        target,
        signer: pk("aa"),
        actor: Some(pk(actor)),
        role: Some(role.to_owned()),
        session_ref: Some("6a8f1b2c-0000-4000-8000-000000000001".into()),
        status: "idle".into(),
        model: Some("claude-opus".into()),
        runtime: Some("claude".try_into().expect("runtime")),
        last_signed_seq: Some(1),
        last_signed_at: Some(1_000),
        liveness: Liveness::Quiet { age_secs: 10 },
        turn_budget: None,
    }
}

/// One 44225 item on the exact envelope the provider signs.
fn item(session_id: &str, seq: u64, turn_id: Option<&str>, item: Value) -> TranscriptRecord {
    let target = target(session_id);
    TranscriptRecord {
        id: format!("{seq:064}"),
        signer: pk("aa"),
        created_at: 1_700_000_000 + seq as i64,
        target_key: coding_session_target_key(&target),
        seq,
        envelope: TranscriptEnvelope::new(
            &target,
            seq,
            (1_700_000_000 + seq as i64) * 1_000,
            turn_id,
            item,
        ),
        raw: json!({}),
    }
}

fn tool_call(id: &str, name: &str, input: Value) -> Value {
    json!({
        "kind": "tool_call",
        "tool": { "toolName": name, "toolId": id, "input": input },
    })
}

fn tool_result(id: &str, content: &str) -> Value {
    json!({
        "kind": "tool_result",
        "toolId": id,
        "toolName": "shell",
        "content": content,
        "isError": false,
    })
}

/// Keystone's terminal item: a complete usage block, as the provider publishes
/// it when the driver reports everything.
///
/// Built with the producer's own `result_item`, not by hand. A hand-written
/// fixture is a statement about what this command *expects*; only the
/// producer's builder is a statement about what is on the wire, and the
/// difference between them is exactly how a `pricingIdentity` key nothing
/// publishes came to gate every cost this command reported (REVIEW-A1 F2).
fn full_result() -> Value {
    result_item(
        ResultSubtype::Success,
        1_284_233,
        "done",
        TurnCost {
            cost_usd: Some(4.25),
            input_tokens: Some(1_223_324),
            output_tokens: Some(9_113),
            total_tokens: Some(1_232_437),
        },
        TurnUsageReport {
            input_tokens: Some(18_442),
            output_tokens: Some(9_113),
            cache_read_tokens: Some(1_204_882),
            cache_write_tokens: Some(66_301),
            tool_calls: Some(64),
            context_window: Some(200_000),
        },
    )
}

fn turns(report: &Value) -> Vec<Value> {
    report
        .get("turns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn rows(report: &Value, table: &str) -> Vec<Value> {
    report
        .get(table)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

// ── Per-turn accounting ─────────────────────────────────────────────────────

#[test]
fn a_turn_row_carries_every_number_the_driver_reported() {
    let execution = execution("keystone", KEYSTONE, "builder");
    let items = vec![
        item(
            "keystone",
            1,
            Some("turn-1"),
            tool_call("t1", "shell", json!({ "command": "just ci" })),
        ),
        item("keystone", 2, Some("turn-1"), tool_result("t1", "ok")),
        item("keystone", 3, Some("turn-1"), full_result()),
    ];

    let report = audit_report(&[execution], &items);
    let rows = turns(&report);
    assert_eq!(rows.len(), 1, "one turn, one row: {rows:?}");
    let row = &rows[0];

    assert_eq!(row["turnId"], json!("turn-1"));
    assert_eq!(
        row["seat"],
        json!(format!("{}·builder", &pk(KEYSTONE)[..8]))
    );
    assert_eq!(row["durationMs"], json!(1_284_233u64));
    assert_eq!(row["toolCalls"], json!(64u64));
    assert_eq!(row["inputTokens"], json!(18_442u64));
    assert_eq!(row["outputTokens"], json!(9_113u64));
    assert_eq!(row["cacheReadTokens"], json!(1_204_882u64));
    assert_eq!(row["cacheWriteTokens"], json!(66_301u64));
    assert_eq!(row["contextWindow"], json!(200_000u64));
    assert_eq!(row["costUsd"], json!(4.25));
    assert!(
        row["startedAt"].as_str().is_some_and(|at| at.contains('T')),
        "startedAt is an instant: {row}"
    );
    assert_eq!(
        row["executionKey"],
        json!(coding_session_target_key(&target("keystone")))
    );
}

/// The rule the whole command exists to keep: absent is `null`, never `0`.
#[test]
fn a_number_the_driver_never_reported_is_null_not_zero() {
    let execution = execution("bob", BOB, "runner");
    let items = vec![
        item(
            "bob",
            1,
            Some("turn-9"),
            tool_call("t1", "shell", json!({ "command": "cargo test" })),
        ),
        item("bob", 2, Some("turn-9"), tool_result("t1", "ok")),
        item(
            "bob",
            3,
            Some("turn-9"),
            json!({
                "kind": "result",
                "subtype": "success",
                "isError": false,
                "durationMs": 900u64,
                "result": "done",
            }),
        ),
    ];

    let report = audit_report(&[execution], &items);
    let row = turns(&report).remove(0);
    for field in [
        "inputTokens",
        "outputTokens",
        "cacheReadTokens",
        "cacheWriteTokens",
        "contextWindow",
        "costUsd",
    ] {
        assert_eq!(
            row[field],
            Value::Null,
            "{field} must be null when the driver reported nothing, got {}",
            row[field]
        );
    }
    // Tool calls are still a measurement: this turn published exactly one.
    assert_eq!(row["toolCalls"], json!(1u64));

    let totals = &report["totals"]["session"];
    assert_eq!(totals["inputTokens"], Value::Null);
    assert_eq!(totals["durationMs"], json!(900u64));
}

/// The cost the producer publishes is the cost the audit reports.
///
/// The lane gated `costUsd` on a `pricingIdentity` key inside `usage`. Nothing
/// writes one: `result_item` serializes [`TurnUsageReport`], which has six
/// fields and no pricing identity, so every turn on the wire reported `null`
/// while the item beside it carried a number (REVIEW-A1 F2). The gate is gone;
/// the number is the producer's own.
#[test]
fn a_cost_the_producer_published_is_reported() {
    let execution = execution("keystone", KEYSTONE, "builder");
    let priced = result_item(
        ResultSubtype::Success,
        10,
        "done",
        TurnCost {
            cost_usd: Some(9.99),
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
        },
        TurnUsageReport {
            input_tokens: Some(5),
            ..TurnUsageReport::default()
        },
    );
    assert!(
        priced
            .get("usage")
            .and_then(|usage| usage.get("pricingIdentity"))
            .is_none(),
        "no producer stamps a pricing identity; a gate on one is a gate on nothing: {priced}"
    );

    let report = audit_report(&[execution], &[item("keystone", 1, Some("turn-1"), priced)]);
    let row = turns(&report).remove(0);
    assert_eq!(row["costUsd"], json!(9.99));
    assert_eq!(report["totals"]["session"]["costUsd"], json!(9.99));
}

/// A turn whose producer published no cost still reports none.
#[test]
fn a_turn_with_no_cost_reports_null_not_zero() {
    let execution = execution("bob", BOB, "runner");
    let free = result_item(
        ResultSubtype::Success,
        10,
        "done",
        TurnCost::default(),
        TurnUsageReport {
            input_tokens: Some(5),
            ..TurnUsageReport::default()
        },
    );
    let report = audit_report(&[execution], &[item("bob", 1, Some("turn-1"), free)]);
    assert_eq!(turns(&report).remove(0)["costUsd"], Value::Null);
    assert_eq!(report["totals"]["session"]["costUsd"], Value::Null);
}

#[test]
fn the_drivers_own_occupancy_item_supplies_a_missing_window() {
    let execution = execution("bob", BOB, "runner");
    let items = vec![
        item(
            "bob",
            1,
            Some("turn-1"),
            json!({ "kind": "context_window_updated", "usage": { "used": 40, "size": 128_000 } }),
        ),
        item(
            "bob",
            2,
            Some("turn-1"),
            json!({
                "kind": "result",
                "subtype": "success",
                "isError": false,
                "durationMs": 5u64,
                "result": "done",
                "usage": { "inputTokens": 3u64 },
            }),
        ),
    ];
    let report = audit_report(&[execution], &items);
    assert_eq!(turns(&report)[0]["contextWindow"], json!(128_000u64));
}

#[test]
fn items_outside_a_turn_do_not_invent_a_row() {
    let execution = execution("bob", BOB, "runner");
    let items = vec![
        item(
            "bob",
            1,
            None,
            json!({ "kind": "status", "status": "idle" }),
        ),
        item("bob", 2, None, json!({ "kind": "session_init" })),
    ];
    assert!(turns(&audit_report(&[execution], &items)).is_empty());
}

// ── Waste tables ────────────────────────────────────────────────────────────

#[test]
fn a_file_handed_twice_is_named_with_its_published_bytes() {
    let execution = execution("keystone", KEYSTONE, "builder");
    let clipped = format!("{}…[elided 4096 bytes, sha256:abc]", "x".repeat(64));
    let items = vec![
        item(
            "keystone",
            1,
            Some("turn-1"),
            tool_call(
                "t1",
                "read",
                json!({ "path": "/repo/crates/buzz-cli/src/lib.rs" }),
            ),
        ),
        item("keystone", 2, Some("turn-1"), tool_result("t1", "one")),
        item(
            "keystone",
            3,
            Some("turn-1"),
            tool_call(
                "t2",
                "read",
                json!({ "path": "/repo/crates/buzz-cli/src/lib.rs" }),
            ),
        ),
        item("keystone", 4, Some("turn-1"), tool_result("t2", &clipped)),
        item(
            "keystone",
            5,
            Some("turn-1"),
            tool_call("t3", "read", json!({ "path": "/repo/README.md" })),
        ),
        item("keystone", 6, Some("turn-1"), tool_result("t3", "once")),
    ];

    let report = audit_report(&[execution], &items);
    let handed = rows(&report, "handedTwice");
    assert_eq!(
        handed.len(),
        1,
        "only the repeated path is waste: {handed:?}"
    );
    assert_eq!(handed[0]["what"], json!("path"));
    assert_eq!(handed[0]["key"], json!("/repo/crates/buzz-cli/src/lib.rs"));
    assert_eq!(handed[0]["count"], json!(2u64));
    assert_eq!(
        handed[0]["bytes"],
        json!(("one".len() + clipped.len()) as u64)
    );
    assert_eq!(
        handed[0]["bytesClipped"],
        json!(true),
        "the provider clipped one of those results and the row must say so"
    );
}

#[test]
fn room_downloads_count_the_reads_a_seat_pulled_into_its_own_context() {
    let execution = execution("bob", BOB, "runner");
    let commands = [
        "bee --format compact sessions status --channel c",
        "bee sessions inbox --channel c",
        "/usr/local/bin/bee sessions status --channel c",
        "cargo test -p buzz-cli",
        "bee sessions audit --channel c",
    ];
    let items: Vec<TranscriptRecord> = commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            item(
                "bob",
                index as u64 + 1,
                Some("turn-1"),
                tool_call(&format!("t{index}"), "shell", json!({ "command": command })),
            )
        })
        .collect();

    let report = audit_report(&[execution], &items);
    let downloads = rows(&report, "roomDownloads");
    let named: Vec<(String, u64)> = downloads
        .iter()
        .map(|row| {
            (
                row["command"].as_str().unwrap_or_default().to_owned(),
                row["count"].as_u64().unwrap_or_default(),
            )
        })
        .collect();
    assert!(
        named.contains(&("sessions status".to_owned(), 2)),
        "two status reads, one behind a global flag and one behind a path: {named:?}"
    );
    assert!(
        named.contains(&("sessions inbox".to_owned(), 1)),
        "{named:?}"
    );
    assert!(
        !named.iter().any(|(command, _)| command.contains("audit")),
        "`sessions audit` is not one of the four room reads: {named:?}"
    );
    assert_eq!(named.len(), 2, "nothing else is a room download: {named:?}");
}

/// A command line that *mentions* `bee` did not run it. The scan now reads the
/// executable in command position only (REVIEW-A1 F9).
#[test]
fn a_command_that_only_mentions_bee_is_not_a_room_download() {
    let execution = execution("bob", BOB, "runner");
    let commands = [
        "echo bee sessions status --channel c",
        "grep -rn 'bee sessions inbox' docs",
        "BUZZ_RELAY_URL=http://localhost:3000 bee sessions status --channel c",
        "just ci && bee sessions inbox --channel c",
    ];
    let items: Vec<TranscriptRecord> = commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            item(
                "bob",
                index as u64 + 1,
                Some("turn-1"),
                tool_call(&format!("t{index}"), "shell", json!({ "command": command })),
            )
        })
        .collect();

    let downloads = rows(&audit_report(&[execution], &items), "roomDownloads");
    let named: Vec<(String, u64)> = downloads
        .iter()
        .map(|row| {
            (
                row["command"].as_str().unwrap_or_default().to_owned(),
                row["count"].as_u64().unwrap_or_default(),
            )
        })
        .collect();
    assert!(
        named.contains(&("sessions status".to_owned(), 1)),
        "the one real status read, behind an environment assignment: {named:?}"
    );
    assert!(
        named.contains(&("sessions inbox".to_owned(), 1)),
        "the one real inbox read, after `&&`: {named:?}"
    );
    assert_eq!(
        named.len(),
        2,
        "an `echo` and a `grep` ran no bee at all: {named:?}"
    );
}

/// `bytes` counts published results, and two calls can share one. Saying so is
/// the difference between "these two reads cost 3 bytes" and "one of these two
/// reads cost 3 bytes and the other was never answered" (REVIEW-A1 F10).
#[test]
fn handed_twice_says_how_many_of_those_calls_were_answered() {
    let execution = execution("keystone", KEYSTONE, "builder");
    let items = vec![
        item(
            "keystone",
            1,
            Some("turn-1"),
            tool_call("t1", "read", json!({ "path": "/repo/README.md" })),
        ),
        item("keystone", 2, Some("turn-1"), tool_result("t1", "one")),
        item(
            "keystone",
            3,
            Some("turn-1"),
            tool_call("t2", "read", json!({ "path": "/repo/README.md" })),
        ),
    ];

    let handed = rows(&audit_report(&[execution], &items), "handedTwice");
    assert_eq!(handed.len(), 1, "{handed:?}");
    assert_eq!(handed[0]["count"], json!(2u64));
    assert_eq!(handed[0]["resultsSeen"], json!(1u64), "{}", handed[0]);
    assert_eq!(handed[0]["bytes"], json!(3u64));
}

/// Two calls, no results at all: there is no byte count to give.
#[test]
fn bytes_are_null_when_nothing_came_back() {
    let execution = execution("keystone", KEYSTONE, "builder");
    let items = vec![
        item(
            "keystone",
            1,
            Some("turn-1"),
            tool_call("t1", "read", json!({ "path": "/repo/README.md" })),
        ),
        item(
            "keystone",
            2,
            Some("turn-1"),
            tool_call("t2", "read", json!({ "path": "/repo/README.md" })),
        ),
    ];

    let handed = rows(&audit_report(&[execution], &items), "handedTwice");
    assert_eq!(handed.len(), 1, "{handed:?}");
    assert_eq!(handed[0]["resultsSeen"], json!(0u64));
    assert_eq!(
        handed[0]["bytes"],
        Value::Null,
        "no result published, no byte count: {}",
        handed[0]
    );
}

#[test]
fn a_retry_loop_needs_three_consecutive_identical_results() {
    let execution = execution("bob", BOB, "runner");
    let mut items = Vec::new();
    let mut seq = 0u64;
    let run = |items: &mut Vec<TranscriptRecord>, seq: &mut u64, command: &str, result: &str| {
        *seq += 1;
        let id = format!("t{seq}");
        items.push(item(
            "bob",
            *seq,
            Some("turn-1"),
            tool_call(&id, "shell", json!({ "command": command })),
        ));
        *seq += 1;
        items.push(item("bob", *seq, Some("turn-1"), tool_result(&id, result)));
    };
    for _ in 0..3 {
        run(&mut items, &mut seq, "just ci", "FAILED");
    }
    // Twice is a retry, not a loop.
    for _ in 0..2 {
        run(&mut items, &mut seq, "cargo fmt", "ok");
    }

    let report = audit_report(&[execution], &items);
    let loops = rows(&report, "retryLoops");
    assert_eq!(loops.len(), 1, "only the three-run is a loop: {loops:?}");
    assert_eq!(loops[0]["command"], json!("just ci"));
    assert_eq!(loops[0]["count"], json!(3));
    assert_eq!(loops[0]["identicalResults"], json!(true));
}

/// Three runs of one command whose results were never published is three runs
/// of one command. Whether they agreed is unknown, and unknown is not `true`
/// (§0.8, I9; REVIEW-A1 F4).
#[test]
fn a_run_whose_results_were_never_published_claims_no_agreement() {
    let execution = execution("bob", BOB, "runner");
    let items: Vec<TranscriptRecord> = (1..=3)
        .map(|seq| {
            item(
                "bob",
                seq,
                Some("turn-1"),
                tool_call(
                    &format!("t{seq}"),
                    "shell",
                    json!({ "command": "cargo test -p buzz-cli" }),
                ),
            )
        })
        .collect();

    let loops = rows(&audit_report(&[execution], &items), "retryLoops");
    assert_eq!(
        loops.len(),
        1,
        "three consecutive runs is still a run: {loops:?}"
    );
    assert_eq!(loops[0]["count"], json!(3));
    assert_eq!(
        loops[0]["identicalResults"],
        Value::Null,
        "results nobody published cannot be identical: {}",
        loops[0]
    );
}

#[test]
fn three_identical_commands_with_different_results_are_work_not_a_loop() {
    let execution = execution("bob", BOB, "runner");
    let mut items = Vec::new();
    for (index, result) in ["1 failed", "2 failed", "ok"].iter().enumerate() {
        let seq = index as u64 * 2 + 1;
        let id = format!("t{seq}");
        items.push(item(
            "bob",
            seq,
            Some("turn-1"),
            tool_call(&id, "shell", json!({ "command": "just ci" })),
        ));
        items.push(item(
            "bob",
            seq + 1,
            Some("turn-1"),
            tool_result(&id, result),
        ));
    }
    assert!(rows(&audit_report(&[execution], &items), "retryLoops").is_empty());
}

// ── Totals and bounds ───────────────────────────────────────────────────────

#[test]
fn totals_are_reported_per_seat_and_per_session() {
    let items_keystone = vec![item("keystone", 1, Some("k1"), full_result())];
    let items_bob = vec![item("bob", 1, Some("b1"), full_result())];
    let mut items = items_keystone;
    items.extend(items_bob);

    let report = audit_report(
        &[
            execution("keystone", KEYSTONE, "builder"),
            execution("bob", BOB, "runner"),
        ],
        &items,
    );
    let by_seat = rows(&report, "totals");
    assert!(by_seat.is_empty(), "totals is an object, not an array");
    let seats = report["totals"]["bySeat"]
        .as_array()
        .expect("bySeat is an array")
        .clone();
    assert_eq!(seats.len(), 2, "one row per seat: {seats:?}");
    for seat in &seats {
        assert_eq!(seat["turns"], json!(1));
        assert_eq!(seat["inputTokens"], json!(18_442u64));
    }
    assert_eq!(report["totals"]["session"]["turns"], json!(2));
    assert_eq!(report["totals"]["session"]["inputTokens"], json!(36_884u64));
    assert_eq!(report["totals"]["session"]["costUsd"], json!(8.5));
}

#[test]
fn the_fold_is_bounded_per_execution_and_discloses_where_it_stopped() {
    let execution = execution("bob", BOB, "runner");
    let published = MAX_AUDIT_ITEMS_PER_EXECUTION + 7;
    let items: Vec<TranscriptRecord> = (1..=published)
        .map(|seq| {
            item(
                "bob",
                seq as u64,
                Some("turn-1"),
                json!({ "kind": "assistant_text", "text": "…" }),
            )
        })
        .collect();

    let report = audit_report(&[execution], &items);
    let bounds = rows(&report, "bounds");
    assert_eq!(bounds.len(), 1);
    assert_eq!(bounds[0]["itemsPublished"], json!(published));
    assert_eq!(bounds[0]["itemsRead"], json!(MAX_AUDIT_ITEMS_PER_EXECUTION));
    assert_eq!(
        bounds[0]["itemsBound"],
        json!(MAX_AUDIT_ITEMS_PER_EXECUTION)
    );
    assert_eq!(
        bounds[0]["itemsTruncated"],
        json!(true),
        "a bound that is not disclosed is a silent lie"
    );
}

/// The fold stops at the bound, and the terminal `result` item — the one
/// carrying `usage` — is exactly what a long execution loses. Counting the
/// surviving `tool_call` items then reports a number that is neither the
/// driver's nor the turn's, and the row said nothing about it (REVIEW-A1 F3).
#[test]
fn a_counted_tool_call_number_from_a_clipped_execution_says_it_is_partial() {
    let execution = execution("bob", BOB, "runner");
    let published = MAX_AUDIT_ITEMS_PER_EXECUTION + 100;
    let items: Vec<TranscriptRecord> = (1..=published)
        .map(|seq| {
            item(
                "bob",
                seq as u64,
                Some("turn-1"),
                tool_call(&format!("t{seq}"), "shell", json!({ "command": "ls" })),
            )
        })
        .collect();

    let report = audit_report(&[execution], &items);
    let row = turns(&report).remove(0);
    assert_eq!(
        row["toolCallsTruncated"],
        json!(true),
        "a count taken from a clipped stream must say so: {row}"
    );
    assert_eq!(
        report["totals"]["session"]["toolCallsTruncated"],
        json!(true),
        "and the total that summed it must say so too: {}",
        report["totals"]["session"]
    );
}

/// A row whose count is the driver's own is complete even inside a clipped
/// execution — the driver counted the whole turn, the fold did not.
#[test]
fn a_driver_reported_count_is_not_marked_partial() {
    let execution = execution("keystone", KEYSTONE, "builder");
    let report = audit_report(
        &[execution],
        &[item("keystone", 1, Some("turn-1"), full_result())],
    );
    let row = turns(&report).remove(0);
    assert_eq!(row["toolCalls"], json!(64u64));
    assert_eq!(row["toolCallsTruncated"], json!(false), "{row}");
}

/// `--format compact` is what a lead skimming a night's work reads. A bound
/// disclosed only in a sibling table the compact form never prints is not
/// disclosed to that reader (REVIEW-A1 F3).
#[test]
fn compact_output_prints_the_bound_as_well_as_the_turns() {
    let execution = execution("bob", BOB, "runner");
    let published = MAX_AUDIT_ITEMS_PER_EXECUTION + 3;
    let items: Vec<TranscriptRecord> = (1..=published)
        .map(|seq| {
            item(
                "bob",
                seq as u64,
                Some("turn-1"),
                json!({ "kind": "assistant_text", "text": "…" }),
            )
        })
        .collect();

    let lines = compact_lines(&audit_report(&[execution], &items));
    let parsed: Vec<Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("every compact line is one JSON object"))
        .collect();
    assert!(
        parsed.iter().any(|line| line.get("turnId").is_some()),
        "the turn rows are still printed: {lines:?}"
    );
    assert!(
        parsed
            .iter()
            .any(|line| line["itemsTruncated"] == json!(true)),
        "the bound must reach the compact reader: {lines:?}"
    );
}

#[test]
fn a_short_execution_is_not_reported_as_truncated() {
    let execution = execution("bob", BOB, "runner");
    let report = audit_report(&[execution], &[item("bob", 1, Some("t"), full_result())]);
    assert_eq!(rows(&report, "bounds")[0]["itemsTruncated"], json!(false));
}
