//! Oracle tests for `bee sessions measure`.
//!
//! The oracle is not a golden file this lane wrote: it is the two hand-made
//! audits of 2026-09-20, whose every number a model derived by reading relay
//! events one at a time. If these tests pass, the command reproduces the
//! audits. Where a number here differs from an audit, the difference is named
//! in the test and in ledger item 197 — the code was not bent to match.
//!
//! ## The fixtures, and what was done to them
//!
//! `tests/fixtures/measure/` holds two read-only captures from
//! `wss://hive.agiterra.org`, taken 2026-09-20 with Brian's key:
//!
//! - `kettle.json` — channel `85b8db75…`, umbrella `e8338b95…`, the coding
//!   session kinds from 2026-09-20T11:27:00Z to 17:05:00Z, the umbrella
//!   identity kinds with no lower bound, the action chain in the same window,
//!   and the `pivot-test` ref states.
//! - `rpg-part1.json` / `rpg-part2.json` — channel `5d1e59bc…`, umbrella
//!   `377f3623…`, 2026-09-19T16:38:00Z to 20:55:00Z, split only for file size.
//!
//! **Projection.** Both files are sig-stripped and carry `id`, `pubkey`,
//! `kind`, `created_at`, `tags` and `content`. The raw capture is 2.0 MB and
//! 5.4 MB; transcripts dominate, so for kind 44225 the item is projected to
//! the fields these functions read:
//!
//! | item kind | kept |
//! | --- | --- |
//! | `assistant_text`, `reasoning` | `kind` only — never read |
//! | `tool_result` | `kind`, `toolId`, `toolName`, `toolKind`, `isError`, `content` clipped to 8192 chars |
//! | `tool_call` | `kind`, `toolId`, `toolName`, `toolKind`, `input` clipped to 2048 chars |
//! | `user_prompt` | `kind`, `commandId`, `senderRole`, `operatorPubkey`, `steered`, `hostAnswer`, `content` clipped to 4096 chars |
//! | everything else, including every `result` row | verbatim |
//!
//! The clip ceilings are above the furthest offset at which any detector
//! substring occurs in the raw capture (8567), and the provider itself clips a
//! tool result at 8 KiB. The projection was verified lossless for every metric
//! asserted below by recomputing each one on the unprojected capture first.

use super::*;

/// Kettle: channel, umbrella, and the audit's inclusive cutoff second.
const KETTLE_SESSION: &str = "e8338b95-b2bb-4ed8-84eb-57600a6e118e";
/// 2026-09-20T11:49:46Z — the kettle audit's frozen cutoff.
const KETTLE_CUTOFF: i64 = 1_789_904_986;
/// 2026-09-20T11:27:00Z — the window the kettle audit measured from.
const KETTLE_SINCE: i64 = 1_789_903_620;
/// Andy's NES RPG umbrella.
const RPG_SESSION: &str = "377f3623-a247-4deb-a3b6-2ba27a86c1e2";

fn load(parts: &[&str]) -> Vec<Value> {
    let mut events = Vec::new();
    for part in parts {
        let path = format!(
            "{}/tests/fixtures/measure/{part}",
            env!("CARGO_MANIFEST_DIR")
        );
        let body = std::fs::read_to_string(&path).expect("fixture readable");
        let page: Vec<Value> = serde_json::from_str(&body).expect("fixture is a JSON array");
        events.extend(page);
    }
    events
}

fn kettle() -> Vec<Value> {
    load(&["kettle.json"])
}

fn rpg() -> Vec<Value> {
    load(&["rpg-part1.json", "rpg-part2.json"])
}

fn seat<'a>(report: &'a Value, role: &str) -> &'a Value {
    report
        .get("seats")
        .and_then(Value::as_array)
        .expect("seats")
        .iter()
        .find(|seat| seat.get("role").and_then(Value::as_str) == Some(role))
        .unwrap_or_else(|| panic!("no seat with role {role}"))
}

fn i64_at(value: &Value, pointer: &str) -> i64 {
    value
        .pointer(pointer)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("{pointer} is not an integer: {value}"))
}

fn f64_at(value: &Value, pointer: &str) -> f64 {
    value
        .pointer(pointer)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("{pointer} is not a number: {value}"))
}

// ── Kettle, at the audit's cutoff ───────────────────────────────────────────

/// The five exact result rows of the kettle audit's "Exact result rows" table,
/// and the completed-result subtotal it draws from them.
#[test]
fn kettle_reproduces_the_audits_completed_result_subtotal() {
    let report = measure_report(
        &kettle(),
        Some(KETTLE_SESSION),
        Some(KETTLE_SINCE),
        Some(KETTLE_CUTOFF),
    );

    assert_eq!(i64_at(&report, "/totals/completed_turns"), 5);
    assert_eq!(i64_at(&report, "/totals/open_turns"), 2);
    assert_eq!(i64_at(&report, "/totals/input_tokens"), 9_250_152);
    assert_eq!(i64_at(&report, "/totals/output_tokens"), 106_661);
    assert_eq!(i64_at(&report, "/totals/tool_calls"), 155);

    // $2.297416 of reported cost over 2 priced results out of 5.
    assert_eq!(f64_at(&report, "/totals/cost/reported_usd"), 2.297_416);
    assert_eq!(i64_at(&report, "/totals/cost/results_priced"), 2);
    assert_eq!(i64_at(&report, "/totals/cost/results_total"), 5);
}

/// Per-seat turns, tokens, active minutes and the cache split of the kettle
/// audit's § 2 tables.
#[test]
fn kettle_reproduces_the_per_seat_table() {
    let report = measure_report(
        &kettle(),
        Some(KETTLE_SESSION),
        Some(KETTLE_SINCE),
        Some(KETTLE_CUTOFF),
    );

    let lead = seat(&report, "lead");
    assert_eq!(i64_at(lead, "/turns/completed"), 3);
    assert_eq!(i64_at(lead, "/turns/open"), 1);
    assert_eq!(
        i64_at(lead, "/tokens/input_total_cache_inclusive"),
        4_302_237
    );
    assert_eq!(i64_at(lead, "/tokens/output"), 39_489);
    assert_eq!(i64_at(lead, "/tokens/fresh_input"), 112);
    assert_eq!(i64_at(lead, "/tokens/cache_read"), 4_195_840);
    assert_eq!(i64_at(lead, "/tokens/cache_write"), 106_285);
    assert_eq!(f64_at(lead, "/active_minutes"), 9.21);
    assert_eq!(f64_at(lead, "/no_open_turn_wall_minutes"), 10.22);

    let builder = seat(&report, "builder");
    assert_eq!(i64_at(builder, "/turns/completed"), 1);
    assert_eq!(
        i64_at(builder, "/tokens/input_total_cache_inclusive"),
        2_404_645
    );
    assert_eq!(i64_at(builder, "/tokens/output"), 28_750);
    assert_eq!(i64_at(builder, "/tokens/fresh_input"), 78);
    assert_eq!(i64_at(builder, "/tokens/cache_read"), 2_332_655);
    assert_eq!(i64_at(builder, "/tokens/cache_write"), 71_912);
    assert_eq!(f64_at(builder, "/active_minutes"), 5.85);
    assert_eq!(f64_at(builder, "/no_open_turn_wall_minutes"), 12.83);

    let verifier = seat(&report, "verifier");
    assert_eq!(i64_at(verifier, "/turns/completed"), 1);
    assert_eq!(i64_at(verifier, "/turns/open"), 1);
    assert_eq!(
        i64_at(verifier, "/tokens/input_total_cache_inclusive"),
        2_543_270
    );
    assert_eq!(i64_at(verifier, "/tokens/output"), 38_422);
    assert_eq!(i64_at(verifier, "/tokens/fresh_input"), 70);
    assert_eq!(i64_at(verifier, "/tokens/cache_read"), 2_449_322);
    assert_eq!(i64_at(verifier, "/tokens/cache_write"), 93_878);
    assert_eq!(f64_at(verifier, "/active_minutes"), 9.78);
    assert_eq!(f64_at(verifier, "/no_open_turn_wall_minutes"), 0.87);
}

/// "L's queue/start differences are 0,43,0,0 seconds; V's second turn is 0."
#[test]
fn kettle_reproduces_the_queue_to_start_samples() {
    let report = measure_report(
        &kettle(),
        Some(KETTLE_SESSION),
        Some(KETTLE_SINCE),
        Some(KETTLE_CUTOFF),
    );

    let lead = seat(&report, "lead");
    let samples: Vec<i64> = lead
        .pointer("/queue_to_start_seconds/samples")
        .and_then(Value::as_array)
        .expect("samples")
        .iter()
        .filter_map(Value::as_i64)
        .collect();
    assert_eq!(samples, vec![0, 0, 0, 43]);
    assert_eq!(i64_at(lead, "/queue_to_start_seconds/max"), 43);
    assert_eq!(
        i64_at(lead, "/queue_to_start_seconds/resolution_limited_zeros"),
        3
    );

    let verifier = seat(&report, "verifier");
    let samples: Vec<i64> = verifier
        .pointer("/queue_to_start_seconds/samples")
        .and_then(Value::as_array)
        .expect("samples")
        .iter()
        .filter_map(Value::as_i64)
        .collect();
    assert_eq!(samples, vec![0]);
    // "B/V initial hires emit `created` and `turn_started`, but no
    // `turn_queued`" — an unpaired start is disclosed, not counted as zero.
    assert_eq!(
        i64_at(
            verifier,
            "/queue_to_start_seconds/starts_without_a_queue_receipt"
        ),
        1
    );
    let builder = seat(&report, "builder");
    assert!(builder
        .pointer("/queue_to_start_seconds/p50")
        .is_some_and(Value::is_null));
}

/// "There are 15 explicit CLI `user_error` JSON records in L, 11 in B and 26
/// in V through cutoff." The audit counted occurrences, not tool results; both
/// are reported, and the occurrence count is the one that matches.
#[test]
fn kettle_reproduces_the_schema_discovery_counts() {
    let report = measure_report(
        &kettle(),
        Some(KETTLE_SESSION),
        Some(KETTLE_SINCE),
        Some(KETTLE_CUTOFF),
    );

    for (role, records, rows) in [("lead", 15, 7), ("builder", 11, 9), ("verifier", 26, 13)] {
        let seat = seat(&report, role);
        assert_eq!(
            i64_at(seat, "/refusals/schema_discovery_user_error_records"),
            records,
            "{role} user_error occurrences"
        );
        assert_eq!(
            i64_at(seat, "/refusals/schema_discovery_tool_results"),
            rows,
            "{role} tool results carrying one"
        );
    }
}

/// Finding A: `HIRE_NO_ROUTE`, once, on the lead.
#[test]
fn kettle_names_the_hire_refusal_by_its_code() {
    let report = measure_report(
        &kettle(),
        Some(KETTLE_SESSION),
        Some(KETTLE_SINCE),
        Some(KETTLE_CUTOFF),
    );
    let lead = seat(&report, "lead");
    let codes = lead
        .pointer("/refusals/hire_by_code")
        .and_then(Value::as_object)
        .expect("hire codes");
    assert!(
        codes.contains_key("HIRE_NO_ROUTE"),
        "expected HIRE_NO_ROUTE among {codes:?}"
    );
}

/// Identity is read from 44223, not guessed: role, actor, runtime and model.
#[test]
fn kettle_keys_each_seat_by_execution_with_its_identity() {
    let report = measure_report(
        &kettle(),
        Some(KETTLE_SESSION),
        Some(KETTLE_SINCE),
        Some(KETTLE_CUTOFF),
    );
    assert_eq!(i64_at(&report, "/channel_scope/executions_selected"), 3);
    let lead = seat(&report, "lead");
    assert_eq!(
        lead.get("session_id").and_then(Value::as_str),
        Some("74495ca8-2aeb-4f25-9f40-3124be1c476f")
    );
    assert_eq!(lead.get("runtime").and_then(Value::as_str), Some("claude"));
    assert_eq!(lead.get("model").and_then(Value::as_str), Some("opus[1m]"));
    assert_eq!(
        lead.get("provider").and_then(Value::as_str),
        Some("claude-primary")
    );
    assert!(lead
        .get("actor")
        .and_then(Value::as_str)
        .is_some_and(|actor| actor.starts_with("93710ade")));
}

/// Nothing partial is ever presented as a total: with two turns open and three
/// of five results unpriced, both facts are in the honesty block.
#[test]
fn kettle_discloses_open_turns_and_partial_pricing() {
    let report = measure_report(
        &kettle(),
        Some(KETTLE_SESSION),
        Some(KETTLE_SINCE),
        Some(KETTLE_CUTOFF),
    );
    let honesty = report
        .get("honesty")
        .and_then(Value::as_array)
        .expect("honesty");
    let metrics: Vec<&str> = honesty
        .iter()
        .filter_map(|row| row.get("metric").and_then(Value::as_str))
        .collect();
    assert!(
        metrics.contains(&"tokens and cost of open turns"),
        "{metrics:?}"
    );
    assert!(metrics.contains(&"dollar cost"), "{metrics:?}");
    assert!(
        metrics.contains(&"prompts that are host notices"),
        "the 2026-09-20 builds carry no hostAnswer flag: {metrics:?}"
    );
}

// ── Kettle, the whole run ───────────────────────────────────────────────────

/// The full run reaches a terminal record, and the timeline carries the whole
/// action chain plus the relay-signed 30618 that observed the landed sha.
#[test]
fn kettle_full_run_times_the_action_chain_and_the_terminal_record() {
    let report = measure_report(&kettle(), Some(KETTLE_SESSION), None, None);
    let timeline = report
        .get("timeline")
        .and_then(Value::as_array)
        .expect("timeline");
    let events: Vec<&str> = timeline
        .iter()
        .filter_map(|row| row.get("event").and_then(Value::as_str))
        .collect();
    for expected in [
        "goal",
        "first_turn_start",
        "assignment",
        "report",
        "verdict",
        "acknowledgement",
        "decision.request",
        "action.approval_requested",
        "action.host_requested",
        "action.claimed",
        "action.result",
        "observed_delivery",
        "mission.completed",
    ] {
        assert!(
            events.contains(&expected),
            "{expected} missing from {events:?}"
        );
    }

    // The runbook's "goal to terminal record: 5 h 31 m" measured from the
    // recorded intent, not from the goal event. Both ends are named, so both
    // numbers are readable and neither can be mistaken for the other.
    let goal_to_terminal = i64_at(&report, "/gross/goal_to_terminal_seconds");
    assert_eq!(
        goal_to_terminal, 20_042,
        "goal 11:24:49Z → completed 16:58:51Z"
    );
    let first_turn_to_terminal = i64_at(&report, "/gross/first_turn_to_terminal_seconds");
    assert!(
        (19_800..19_950).contains(&first_turn_to_terminal),
        "first turn to terminal was {first_turn_to_terminal}s, not the runbook's ~5h31m"
    );

    // A 30618 naming the landed sha of the completion.
    let delivery = timeline
        .iter()
        .find(|row| row.get("event").and_then(Value::as_str) == Some("observed_delivery"))
        .expect("observed delivery");
    assert_eq!(
        delivery.pointer("/detail/repo").and_then(Value::as_str),
        Some("pivot-test")
    );
}

/// Ledger 206 C: the founder-signed commands split into what a person did
/// and what this computer did under the person's key.
#[test]
fn kettle_full_run_separates_the_persons_acts_from_the_hosts() {
    let report = measure_report(&kettle(), Some(KETTLE_SESSION), None, None);
    let owner = report
        .pointer("/coordination/founder_key")
        .and_then(Value::as_str)
        .expect("founder key");
    assert!(owner.starts_with("3d3b7169"), "{owner}");

    // Seven founder-signed commands in this umbrella, as before — but two of
    // them are seats the desktop created to answer the lead's `session.hire`,
    // so the person's own count is five: one seat opened with no initial
    // turn, and four turns typed. The kettle channel also holds an earlier
    // hiring-verification umbrella from 2026-09-19 whose founder-signed
    // commands must not be counted here.
    let person = report
        .pointer("/coordination/person_actions")
        .and_then(Value::as_array)
        .expect("person actions");
    let host = report
        .pointer("/coordination/host_actions_under_founder_key")
        .and_then(Value::as_array)
        .expect("host actions");
    assert_eq!(person.len(), 5);
    assert_eq!(i64_at(&report, "/coordination/person_action_count"), 5);
    assert_eq!(host.len(), 2);
    assert_eq!(
        i64_at(&report, "/coordination/host_action_under_founder_key_count"),
        2
    );
    assert_eq!(
        i64_at(&report, "/coordination/unattributed_founder_action_count"),
        0,
        "every founder-signed act in this run is attributable"
    );

    // The person's five: one create, four turns.
    let kinds: Vec<i64> = person
        .iter()
        .filter_map(|action| action.get("kind").and_then(Value::as_i64))
        .collect();
    assert_eq!(kinds, vec![44221, 44220, 44220, 44220, 44220]);

    // Each host row names the hire request it answered, and says which rule
    // placed it there — never a bare classification.
    for action in host {
        assert_eq!(
            action.get("action").and_then(Value::as_str),
            Some("session.create")
        );
        let hire = action
            .get("answers_hire_request")
            .and_then(Value::as_str)
            .expect("the hire request this create answered");
        assert_eq!(hire.len(), 64, "a hire request event id");
        assert!(action
            .get("rule")
            .and_then(Value::as_str)
            .is_some_and(|rule| rule.contains("session.hire")));
    }
    for action in person {
        assert!(action.get("rule").and_then(Value::as_str).is_some());
        assert!(action
            .get("answers_hire_request")
            .is_some_and(Value::is_null));
    }

    for action in person.iter().chain(host) {
        let at = action.get("at").and_then(Value::as_str).unwrap_or_default();
        assert!(
            at.starts_with("2026-09-20"),
            "{at} belongs to an earlier umbrella in the same channel"
        );
    }

    // No 46030/46031 is readable for this run, and the report says so rather
    // than reporting zero approvals as a fact.
    let honesty = report
        .get("honesty")
        .and_then(Value::as_array)
        .expect("honesty");
    assert!(
        honesty.iter().any(|row| {
            row.get("metric").and_then(Value::as_str) == Some("approvals the person signed")
                && row.get("value").and_then(Value::as_str) == Some("unknown")
        }),
        "{honesty:#?}"
    );

    // Every action-chain row is this session's too.
    for row in report
        .get("timeline")
        .and_then(Value::as_array)
        .expect("timeline")
        .iter()
        .filter(|row| {
            row.get("event")
                .and_then(Value::as_str)
                .is_some_and(|event| event.starts_with("action."))
        })
    {
        let at = row.get("at").and_then(Value::as_str).unwrap_or_default();
        assert!(at.starts_with("2026-09-20"), "{at} predates the genesis");
    }
    // The ruling was opened and never answered on the wire.
    assert_eq!(i64_at(&report, "/coordination/rulings_opened"), 1);
    assert_eq!(i64_at(&report, "/coordination/rulings_answered"), 0);
}

// ── Andy's NES RPG ──────────────────────────────────────────────────────────

/// The RPG audit's cost-and-coordination ledger, row by row.
#[test]
fn rpg_reproduces_the_cost_and_coordination_ledger() {
    let report = measure_report(&rpg(), Some(RPG_SESSION), None, None);

    assert_eq!(i64_at(&report, "/totals/completed_turns"), 32);
    assert_eq!(i64_at(&report, "/totals/input_tokens"), 66_492_119);
    assert_eq!(i64_at(&report, "/totals/output_tokens"), 714_115);
    assert_eq!(f64_at(&report, "/totals/cost/reported_usd"), 38.540_707);
    assert_eq!(i64_at(&report, "/totals/cache_read_tokens"), 63_744_991);
    assert_eq!(i64_at(&report, "/totals/cache_write_tokens"), 2_738_740);
    assert_eq!(i64_at(&report, "/totals/fresh_input_tokens"), 8_388);
    assert_eq!(i64_at(&report, "/totals/tool_calls"), 632);
    assert_eq!(f64_at(&report, "/totals/active_minutes"), 171.44);

    // "Every seat's first result omits costUsd" — eight unpriced of 32.
    assert_eq!(i64_at(&report, "/totals/cost/results_priced"), 24);
    assert_eq!(i64_at(&report, "/totals/cost/results_total"), 32);

    for (role, turns, input, output, cost, idle) in [
        ("lead", 10, 16_970_755, 90_205, 13.905_748, 232.78),
        ("designer", 3, 2_122_072, 75_907, 2.781_290, 234.87),
        ("architect", 3, 3_693_951, 60_957, 2.895_462, 237.97),
        ("project-setup", 2, 2_164_048, 32_215, 0.435_316, 244.04),
        ("builder", 5, 23_531_885, 221_947, 13.487_909, 94.16),
        ("runner", 3, 5_829_266, 78_484, 0.555_213, 16.10),
        ("poker", 3, 8_788_554, 118_617, 0.608_015, 15.82),
        ("verifier", 3, 3_391_588, 35_783, 3.871_755, 93.33),
    ] {
        let seat = seat(&report, role);
        assert_eq!(i64_at(seat, "/turns/completed"), turns, "{role} turns");
        assert_eq!(
            i64_at(seat, "/tokens/input_total_cache_inclusive"),
            input,
            "{role} input"
        );
        assert_eq!(i64_at(seat, "/tokens/output"), output, "{role} output");
        assert_eq!(f64_at(seat, "/cost/reported_usd"), cost, "{role} cost");
        assert_eq!(
            f64_at(seat, "/no_open_turn_wall_minutes"),
            idle,
            "{role} no-open-turn wall minutes"
        );
    }
}

/// "CLI error responses": 20 / 2 / 5 / 9 / 2 / 1 / 0 / 1, totalling 40.
#[test]
fn rpg_reproduces_the_cli_error_response_counts() {
    let report = measure_report(&rpg(), Some(RPG_SESSION), None, None);
    let mut total = 0;
    for (role, expected) in [
        ("lead", 20),
        ("designer", 2),
        ("architect", 5),
        ("project-setup", 9),
        ("builder", 2),
        ("runner", 1),
        ("poker", 0),
        ("verifier", 1),
    ] {
        let seat = seat(&report, role);
        let count = i64_at(seat, "/refusals/schema_discovery_user_error_records");
        assert_eq!(count, expected, "{role} CLI error responses");
        total += count;
    }
    assert_eq!(total, 40);
}

/// "**14/32 turns (43.75%) are worker disposition/acknowledgement handling**."
///
/// The audit reached 14 by reading sequences. This reaches it mechanically:
/// the union of turns opened by a `cli-wake-v1:` command naming a 44244
/// `verdict` (8) and turns whose only relay write is an acknowledgement (7).
/// The union is 14, not 15, because project setup's single ACK landed inside
/// the same turn its verdict wake opened. Reported separately as well, because
/// the audit's own narrative "six repair turns" is the ack-only count minus
/// that overlapping seat — a subset, not a different measurement.
#[test]
fn rpg_reproduces_the_disposition_and_acknowledgement_turn_count() {
    let report = measure_report(&rpg(), Some(RPG_SESSION), None, None);
    assert_eq!(i64_at(&report, "/coordination/turns_total"), 32);
    assert_eq!(i64_at(&report, "/coordination/verdict_woken_turns"), 8);
    assert_eq!(
        i64_at(&report, "/coordination/acknowledgement_only_turns"),
        7
    );
    assert_eq!(
        i64_at(
            &report,
            "/coordination/disposition_or_acknowledgement_turns"
        ),
        14
    );
    assert_eq!(
        f64_at(
            &report,
            "/coordination/disposition_or_acknowledgement_share"
        ),
        43.75
    );
}

/// Ledger 206 C on Andy's run: nine founder-signed commands, of which seven
/// are seats his desktop created to answer a lead's hire. Two are his own —
/// the first seat, opened with no initial turn, and the turn he typed into it.
#[test]
fn rpg_separates_the_persons_acts_from_the_hosts() {
    let report = measure_report(&rpg(), Some(RPG_SESSION), None, None);
    assert_eq!(i64_at(&report, "/coordination/person_action_count"), 2);
    assert_eq!(
        i64_at(&report, "/coordination/host_action_under_founder_key_count"),
        7
    );
    assert_eq!(
        i64_at(&report, "/coordination/unattributed_founder_action_count"),
        0
    );
    let host = report
        .pointer("/coordination/host_actions_under_founder_key")
        .and_then(Value::as_array)
        .expect("host actions");
    // Every one of the seven names a distinct hire request: a create matched
    // to the same hire twice would inflate the host count and deflate his.
    let mut hires: Vec<&str> = host
        .iter()
        .filter_map(|action| action.get("answers_hire_request").and_then(Value::as_str))
        .collect();
    hires.sort_unstable();
    hires.dedup();
    assert_eq!(hires.len(), 7);
}

/// "No whole turn consisting of repeated 'is it done yet?' polling was
/// identified" — in both runs, and in both the tool agrees.
#[test]
fn neither_audited_run_contains_a_polling_turn() {
    for report in [
        measure_report(
            &kettle(),
            Some(KETTLE_SESSION),
            Some(KETTLE_SINCE),
            Some(KETTLE_CUTOFF),
        ),
        measure_report(&rpg(), Some(RPG_SESSION), None, None),
    ] {
        for seat in report
            .get("seats")
            .and_then(Value::as_array)
            .expect("seats")
        {
            assert_eq!(
                i64_at(seat, "/polling_turns"),
                0,
                "{} polled",
                seat.get("role").and_then(Value::as_str).unwrap_or("?")
            );
        }
    }
}

/// The RPG run has no mission.completed and no action chain; both are printed
/// as `unknown` with the reason, never as an empty success.
#[test]
fn rpg_discloses_the_missing_terminal_record_and_action_chain() {
    let report = measure_report(&rpg(), Some(RPG_SESSION), None, None);
    assert!(report
        .pointer("/gross/terminal_at")
        .is_some_and(Value::is_null));
    let metrics: Vec<&str> = report
        .get("honesty")
        .and_then(Value::as_array)
        .expect("honesty")
        .iter()
        .filter_map(|row| row.get("metric").and_then(Value::as_str))
        .collect();
    assert!(
        metrics.contains(&"action trigger → result chain"),
        "{metrics:?}"
    );
}

/// The goal is fetched without a lower bound, so the RPG goal published
/// 13m36s before the first prompt is still on the timeline.
#[test]
fn rpg_timeline_carries_the_goal_that_precedes_the_window() {
    let report = measure_report(&rpg(), Some(RPG_SESSION), None, None);
    let goal = report
        .get("timeline")
        .and_then(Value::as_array)
        .expect("timeline")
        .iter()
        .find(|row| row.get("event").and_then(Value::as_str) == Some("goal"))
        .expect("goal on the timeline");
    assert_eq!(
        goal.get("event_id").and_then(Value::as_str),
        Some("955b27d0f30d338252f61d2311253ca55263f63b1b08e09a2bb1864d57254067")
    );
}

// ── Scoping, windows and shape ──────────────────────────────────────────────

/// Without `--session-ref` the kettle channel folds a second, unrelated
/// umbrella's executions too; the scope block says how many were selected.
#[test]
fn an_unscoped_read_folds_every_execution_in_the_channel() {
    let scoped = measure_report(&kettle(), Some(KETTLE_SESSION), None, None);
    let unscoped = measure_report(&kettle(), None, None, None);
    assert!(
        i64_at(&unscoped, "/channel_scope/executions_selected")
            >= i64_at(&scoped, "/channel_scope/executions_selected"),
        "an unscoped read can only widen the cast"
    );
    assert_eq!(
        scoped
            .pointer("/channel_scope/session_ref")
            .and_then(Value::as_str),
        Some(KETTLE_SESSION)
    );
}

/// The window bounds are reported verbatim, and the cutoff every waiting
/// number is measured against is named beside them.
#[test]
fn the_window_and_its_cutoff_are_printed_not_implied() {
    let report = measure_report(
        &kettle(),
        Some(KETTLE_SESSION),
        Some(KETTLE_SINCE),
        Some(KETTLE_CUTOFF),
    );
    assert!(report
        .pointer("/channel_scope/since")
        .and_then(Value::as_str)
        .is_some_and(|since| since.starts_with("2026-09-20T11:27:00")));
    assert!(report
        .pointer("/channel_scope/cutoff")
        .and_then(Value::as_str)
        .is_some_and(|cutoff| cutoff.starts_with("2026-09-20T11:49:46")));
}

/// An empty input is a report with no seats and an honest scope, not a panic.
#[test]
fn no_events_produces_an_empty_report_rather_than_a_failure() {
    let report = measure_report(&[], None, None, None);
    assert_eq!(i64_at(&report, "/channel_scope/events_read"), 0);
    assert_eq!(i64_at(&report, "/channel_scope/executions_selected"), 0);
    assert!(report
        .get("seats")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty));
}

/// The compact rendering prints every section and never panics on a report
/// whose optional fields are null.
#[test]
fn the_table_rendering_covers_every_section() {
    let report = measure_report(&rpg(), Some(RPG_SESSION), None, None);
    let lines = table_lines(&report);
    for heading in ["TIMELINE", "SEATS", "COORDINATION", "HONESTY"] {
        assert!(
            lines.iter().any(|line| line == heading),
            "{heading} missing"
        );
    }
    assert!(table_lines(&measure_report(&[], None, None, None)).len() >= 4);
}

/// Every `HIRE_*` code in a body is named, and nothing else is.
#[test]
fn hire_codes_are_read_off_the_text_without_inventing_one() {
    assert_eq!(
        hire_refusal_codes("{\"code\":\"HIRE_NO_ROUTE\",\"x\":1} HIRE_CHECKOUT_NOT_RECORDED."),
        vec!["HIRE_NO_ROUTE", "HIRE_CHECKOUT_NOT_RECORDED"]
    );
    assert!(hire_refusal_codes("hired a builder").is_empty());
}
