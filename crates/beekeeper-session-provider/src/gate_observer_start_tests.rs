//! SV-41: when the observer says a gate is running, and when it says it ended.
//!
//! Pure, like the rest of the observer's tests: published transcript items and
//! a clock, nothing else. Nothing here asks the seat for anything.

use beekeeper_core::coding_session_observation::{
    validate_coding_session_observation_envelope, CodingSessionObservationBody,
    CodingSessionObservationSource,
};
use beekeeper_sdk::coding_session_observation::build_coding_session_observation;
use serde_json::json;

use super::start::{gate_start_payload, gate_start_semantic_key};
use super::*;

const T: i64 = 1_759_572_120_000;
const SESSION_REF: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const GENESIS_REF: &str = "ce5d87ed1b9b4416bb0aa37ea0fb451f211289c54099882917f4ad538d51519b";
const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";

fn call(tool_id: &str, command: &str) -> Value {
    json!({
        "kind": "tool_call",
        "tool": {
            "toolName": "Bash",
            "toolId": tool_id,
            "toolKind": "execute",
            "input": { "command": command },
        },
    })
}

/// Finding 69's bare call: no arguments on the frame at all.
fn bare_call(tool_id: &str) -> Value {
    json!({
        "kind": "tool_call",
        "tool": { "toolName": "Terminal", "toolId": tool_id, "toolKind": "execute", "input": {} },
    })
}

fn result(tool_id: &str, is_error: Option<bool>) -> Value {
    let mut value = json!({
        "kind": "tool_result",
        "toolId": tool_id,
        "toolName": "Bash",
        "toolKind": "execute",
        "content": "test result: ok. 3 passed",
    });
    if let Some(is_error) = is_error {
        value["isError"] = json!(is_error);
    }
    value
}

fn start(gate: &'static str, started_at_ms: i64) -> ObservedGateStart {
    ObservedGateStart {
        gate,
        started_at_ms,
        ended_at_ms: None,
        measured: false,
    }
}

/// The close a result produces: the one end the provider saw happen.
fn closed(gate: &'static str, started_at_ms: i64, ended_at_ms: i64) -> ObservedGateStart {
    ObservedGateStart {
        gate,
        started_at_ms,
        ended_at_ms: Some(ended_at_ms),
        measured: true,
    }
}

/// The close the provider signs when it stops watching (eviction, turn end,
/// exit): an end time, and no span claimed.
fn unwatched(gate: &'static str, started_at_ms: i64, ended_at_ms: i64) -> ObservedGateStart {
    ObservedGateStart {
        measured: false,
        ..closed(gate, started_at_ms, ended_at_ms)
    }
}

fn duration_of(close: &ObservedGateStart) -> Option<u64> {
    let payload = gate_start_payload(SESSION_REF, GENESIS_REF, close).expect("a close builds");
    let CodingSessionObservationBody::Phase(body) = payload.body else {
        panic!("phase");
    };
    body.duration_ms
}

#[test]
fn a_gate_left_pending_for_five_seconds_starts_once() {
    let mut observer = GateObserver::default();
    observer.on_item(&call("t1", "cargo test -p beekeeper-core"), T);
    assert!(observer
        .due_starts(T + GATE_START_PUBLISH_DELAY_MS - 1)
        .is_empty());
    assert_eq!(
        observer.due_starts(T + GATE_START_PUBLISH_DELAY_MS),
        vec![start("cargo test", T)]
    );
    assert!(
        observer.due_starts(T + 60_000).is_empty(),
        "each start is handed out once"
    );
}

#[test]
fn a_result_inside_five_seconds_gives_no_start_and_no_close() {
    let mut observer = GateObserver::default();
    observer.on_item(&call("t1", "cargo test"), T);
    let rows = observer.on_item(&result("t1", Some(false)), T + 4_000);
    assert_eq!(rows.len(), 1, "the gate row is unchanged");
    assert!(observer.due_starts(T + 10_000).is_empty());
    assert!(observer.take_start_closes().is_empty());
}

#[test]
fn a_result_after_the_start_closes_it_with_the_same_started_at() {
    let mut observer = GateObserver::default();
    observer.on_item(&call("t1", "cargo test"), T);
    assert_eq!(observer.due_starts(T + 6_000).len(), 1);
    let rows = observer.on_item(&result("t1", Some(true)), T + 180_000);
    assert_eq!(rows.len(), 1);
    let closes = observer.take_start_closes();
    assert_eq!(closes, vec![closed("cargo test", T, T + 180_000)]);
    assert_eq!(
        duration_of(&closes[0]),
        Some(180_000),
        "the result is the end the provider saw: its span is measured"
    );
    assert!(observer.take_start_closes().is_empty(), "closed once");
}

#[test]
fn a_result_with_no_exit_evidence_still_closes_the_start() {
    let mut observer = GateObserver::default();
    observer.on_item(&call("t1", "cargo test"), T);
    observer.due_starts(T + 6_000);
    let rows = observer.on_item(&result("t1", None), T + 9_000);
    assert!(rows.is_empty(), "no evidence, no row");
    assert_eq!(
        observer.take_start_closes(),
        vec![closed("cargo test", T, T + 9_000)]
    );
}

#[test]
fn a_cd_chain_a_semicolon_line_and_a_non_gate_start_nothing() {
    for command in [
        "cd crates && cargo test",
        "echo hi; cargo test",
        "cargo test | tee out.txt",
        "ls -la",
        "grep -rn 'cargo test' docs/",
    ] {
        let mut observer = GateObserver::default();
        observer.on_item(&call("t1", command), T);
        assert!(observer.due_starts(T + 60_000).is_empty(), "{command}");
    }
}

#[test]
fn a_bare_call_starts_nothing_and_its_result_still_closes_into_a_row() {
    let mut observer = GateObserver::default();
    observer.on_item(&bare_call("t1"), T);
    assert!(observer.due_starts(T + 60_000).is_empty());
    let mut answered = result("t1", Some(false));
    answered["input"] = json!({ "command": "cargo test" });
    let rows = observer.on_item(&answered, T + 61_000);
    assert_eq!(rows.len(), 1);
    assert!(observer.take_start_closes().is_empty());
}

#[test]
fn a_call_announced_again_with_its_command_keeps_its_clock() {
    let mut observer = GateObserver::default();
    observer.on_item(&bare_call("t1"), T);
    observer.on_item(&call("t1", "cargo test"), T + 2_000);
    assert_eq!(observer.due_starts(T + 5_000), vec![start("cargo test", T)]);
    assert_eq!(observer.pending.len(), 1, "one call, one slot");
}

#[test]
fn a_composed_line_gives_two_starts_and_two_closes() {
    let mut observer = GateObserver::default();
    observer.on_item(&call("t1", "cargo fmt --check && cargo test"), T);
    assert_eq!(
        observer.due_starts(T + 5_000),
        vec![start("cargo fmt", T), start("cargo test", T)]
    );
    observer.on_item(&result("t1", Some(false)), T + 90_000);
    assert_eq!(
        observer.take_start_closes(),
        vec![
            closed("cargo fmt", T, T + 90_000),
            closed("cargo test", T, T + 90_000)
        ]
    );
}

#[test]
fn eviction_from_the_window_closes_a_published_start() {
    let mut observer = GateObserver::default();
    observer.on_item(&call("gate", "cargo test"), T);
    observer.due_starts(T + 5_000);
    for index in 0..MAX_PENDING_GATE_CALLS {
        observer.on_item(&call(&format!("ls-{index}"), "ls"), T + 6_000);
    }
    let closes = observer.take_start_closes();
    assert_eq!(closes, vec![unwatched("cargo test", T, T + 6_000)]);
    assert_eq!(
        duration_of(&closes[0]),
        None,
        "the command may still be running: the provider stopped watching, it measured nothing"
    );
}

#[test]
fn turn_end_closes_published_starts_and_suppresses_the_rest() {
    let mut observer = GateObserver::default();
    observer.on_item(&call("published", "cargo test"), T);
    observer.due_starts(T + 5_000);
    observer.on_item(&call("young", "cargo clippy"), T + 4_000);
    let closes = observer.end_turn(T + 7_000);
    assert_eq!(closes, vec![unwatched("cargo test", T, T + 7_000)]);
    assert_eq!(duration_of(&closes[0]), None, "no result, no measured span");
    assert!(
        observer.due_starts(T + 60_000).is_empty(),
        "a call whose turn ended is never started"
    );
    // A late result is still the row it always was, and owes no second close.
    let rows = observer.on_item(&result("published", Some(false)), T + 70_000);
    assert_eq!(rows.len(), 1);
    assert!(observer.take_start_closes().is_empty());
}

#[test]
fn exit_closes_every_published_start_and_forgets_the_calls() {
    let mut observer = GateObserver::default();
    observer.on_item(&call("t1", "cargo test"), T);
    observer.on_item(&call("t2", "pnpm test"), T + 1_000);
    observer.due_starts(T + 6_000);
    observer.on_item(&call("t3", "just ci"), T + 5_500);
    let closes = observer.close_all(T + 8_000);
    assert_eq!(
        closes,
        vec![
            unwatched("cargo test", T, T + 8_000),
            unwatched("pnpm test", T + 1_000, T + 8_000)
        ]
    );
    assert!(closes.iter().all(|close| duration_of(close).is_none()));
    assert!(observer.pending.is_empty());
    assert!(observer.due_starts(T + 60_000).is_empty());
}

#[test]
fn the_start_row_is_an_observed_phase_with_nothing_but_the_gate_and_the_clock() {
    let payload = gate_start_payload(SESSION_REF, GENESIS_REF, &start("cargo test", T))
        .expect("a start builds");
    assert_eq!(payload.source, CodingSessionObservationSource::Observed);
    assert_eq!(payload.assignment_ref, None);
    let CodingSessionObservationBody::Phase(body) = &payload.body else {
        panic!("a start is a phase row, never a gate row");
    };
    assert_eq!(body.phase, "gate:cargo test");
    assert_eq!(body.started_at_ms, T as u64);
    assert_eq!(body.ended_at_ms, None);
    assert_eq!(body.duration_ms, None);
    let event = build_coding_session_observation(CHANNEL, payload)
        .expect("the strict builder accepts it")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign");
    validate_coding_session_observation_envelope(&event).expect("the relay accepts it");
    assert!(!event.content.contains("-p beekeeper-core"));
}

#[test]
fn the_close_measures_its_span_and_a_backwards_clock_measures_none() {
    let payload = gate_start_payload(
        SESSION_REF,
        GENESIS_REF,
        &closed("cargo test", T, T + 3_000),
    )
    .expect("a close builds");
    let CodingSessionObservationBody::Phase(body) = payload.body else {
        panic!("phase");
    };
    assert_eq!(body.ended_at_ms, Some(T as u64 + 3_000));
    assert_eq!(body.duration_ms, Some(3_000));

    let payload = gate_start_payload(SESSION_REF, GENESIS_REF, &closed("cargo test", T, T - 50))
        .expect("a backwards close still builds");
    let CodingSessionObservationBody::Phase(body) = payload.body else {
        panic!("phase");
    };
    assert_eq!(body.ended_at_ms, Some(T as u64));
    assert_eq!(body.duration_ms, None, "an unmeasured span is not zero");

    let payload = gate_start_payload(
        SESSION_REF,
        GENESIS_REF,
        &unwatched("cargo test", T, T + 3_000),
    )
    .expect("a close the provider did not watch end still builds");
    let CodingSessionObservationBody::Phase(body) = &payload.body else {
        panic!("phase");
    };
    assert_eq!(body.ended_at_ms, Some(T as u64 + 3_000));
    assert_eq!(body.duration_ms, None, "no span is claimed for it");
    let event = build_coding_session_observation(CHANNEL, payload)
        .expect("the strict builder accepts it")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign");
    validate_coding_session_observation_envelope(&event).expect("the relay accepts it");

    assert!(gate_start_payload(SESSION_REF, GENESIS_REF, &start("cargo test", -1)).is_none());
}

#[test]
fn the_semantic_keys_fence_one_start_and_one_close() {
    assert_eq!(
        gate_start_semantic_key("s1", &start("cargo test", T)),
        format!("coding-session-gate-start:s1:cargo test:{T}")
    );
    assert_eq!(
        gate_start_semantic_key("s1", &closed("cargo test", T, T + 1)),
        format!("coding-session-gate-start-close:s1:cargo test:{T}")
    );
    assert_eq!(
        gate_start_semantic_key("s1", &unwatched("cargo test", T, T + 1)),
        format!("coding-session-gate-start-close:s1:cargo test:{T}"),
        "one close per start, however it ended"
    );
}
