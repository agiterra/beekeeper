//! What this adapter answers with, and the fixture the Desktop decoder pins.
//!
//! The fixture is **generated from this adapter** and read verbatim by
//! `codingSessionObservationWire.test.mjs`. B1c's own defect is why: the
//! Desktop decoder's fixtures were hand-written, so `pnpm test` stayed green
//! while the decoder would have thrown for every real session
//! (`coding_session_team_fold_tests.rs`, `the_typescript_decoder_fixture_is_this_adapter_s_real_output`).
//! A field this adapter renames and the decoder does not is now a failing Rust
//! test, not a silently empty card.

use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};

use super::*;

use beekeeper_core_pkg::coding_session_observation::CODING_SESSION_OBSERVATION_SCHEMA;
use beekeeper_core_pkg::kind::KIND_CODING_SESSION_OBSERVATION;

const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const GENESIS: &str = "ce5d87ed1b9b4416bb0aa37ea0fb451f211289c54099882917f4ad538d51519b";

/// Deterministic keys, so the generated fixture is byte-stable across runs.
fn fixed_keys(byte: u8) -> Keys {
    Keys::parse(&format!("{byte:02x}").repeat(32)).expect("fixed secret key")
}

/// A fixed second, so the signed event ids are stable across runs.
///
/// Without it `EventBuilder` stamps `created_at` with the wall clock and every
/// run mints new ids — which the generated fixture would then differ by, and
/// the fixture test would fail for a reason that has nothing to do with the
/// adapter.
const FIXED_CREATED_AT: u64 = 1_800_000_000;

fn observation(
    keys: &Keys,
    observation_type: &str,
    source: &str,
    assignment_ref: Option<&str>,
    body: Value,
) -> Event {
    let content = json!({
        "schema": CODING_SESSION_OBSERVATION_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": GENESIS,
        "type": observation_type,
        "source": source,
        "assignmentRef": assignment_ref.map_or(Value::Null, |value| json!(value)),
        "body": body,
    })
    .to_string();
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        content,
    )
    .tags(vec![
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", SESSION]).expect("d"),
        Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("csob-v"),
        Tag::parse(["csob-genesis", GENESIS]).expect("csob-genesis"),
        Tag::parse(["csob-type", observation_type]).expect("csob-type"),
    ])
    .custom_created_at(Timestamp::from_secs(FIXED_CREATED_AT))
    .sign_with_keys(keys)
    .expect("sign")
}

fn request(events: &[Event], known: Vec<String>) -> CodingSessionObservationFoldRequest {
    CodingSessionObservationFoldRequest {
        schema: CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        known_assignment_refs: known,
        // The fixture verifies the provider's own row, so the generated
        // fixture carries `provenanceChecked: true` and an honoured
        // `observed` row — the shape Desktop actually sends (REVIEW-L5 F2).
        provider_pubkeys: Some(vec![fixed_keys(0x66).public_key().to_hex()]),
        events: events
            .iter()
            .map(|event| serde_json::to_value(event).expect("event json"))
            .collect(),
    }
}

/// One gate row. `head` is the commit it ran against and whether the tree was
/// dirty — `None` for a row that names no commit, which is what every row
/// signed before 2026-09-03 and every declared row without `--head-sha`
/// carries.
fn gate_body(
    gate: &str,
    outcome: &str,
    command: &str,
    summary: Option<&str>,
    head: Option<(&str, bool)>,
) -> Value {
    json!({ "rows": [{
        "gate": gate,
        "outcome": outcome,
        "command": command,
        "summary": summary.map_or(Value::Null, |value| json!(value)),
        "durationMs": 41_000,
        "headSha": head.map_or(Value::Null, |(sha, _)| json!(sha)),
        "dirty": head.map_or(Value::Null, |(_, dirty)| json!(dirty)),
    }] })
}

fn checkpoint_body(phase: &str) -> Value {
    json!({
        "phase": phase,
        "testsWritten": 6,
        "testsRed": 6,
        "testsGreen": 4,
        "lastCommand": "cargo test -p beekeeper-core coding_session_observation",
        "lastSummary": "26 passed; 0 failed",
        "note": Value::Null,
    })
}

fn finding_body(finding_id: &str, disposition: &str) -> Value {
    json!({
        "findingId": finding_id,
        "title": "the Structured tests card described the wire instead of reading it",
        "disposition": disposition,
        "detail": Value::Null,
        "refs": [GENESIS],
        "decisionRef": Value::Null,
    })
}

fn phase_body(phase: &str) -> Value {
    json!({
        "phase": phase,
        "startedAtMs": 1_756_800_000_000u64,
        "endedAtMs": 1_756_800_180_000u64,
        "durationMs": 180_000,
    })
}

#[test]
fn the_adapter_returns_the_cores_own_fold_with_every_collection_present() {
    let seat = fixed_keys(0x11);
    let events = vec![observation(
        &seat,
        "gate",
        "declared",
        None,
        gate_body(
            "just ci",
            "passed",
            "just ci",
            Some("All tests passed!"),
            None,
        ),
    )];
    let response = fold_adapter(request(&events, Vec::new())).expect("fold");
    let wire = serde_json::to_value(&response).expect("serialize");

    assert_eq!(wire["gates"].as_array().map(Vec::len), Some(1));
    assert_eq!(wire["gates"][0]["source"], json!("declared"));
    assert_eq!(wire["gates"][0]["outcome"], json!("passed"));
    assert_eq!(wire["gates"][0]["command"], json!("just ci"));
    // Present and empty, never absent: a reader has to be able to tell
    // "nothing was said" from "this adapter does not disclose it".
    for key in ["checkpoints", "findings", "phases", "unresolved", "ignored"] {
        assert_eq!(wire[key], json!([]), "{key} must be present and empty");
    }
    assert_eq!(wire["truncated"]["gates"], json!(0));
    assert_eq!(wire["disclosure"], json!(OBSERVATION_DISCLOSURE));
}

#[test]
fn an_observed_row_and_a_declared_row_reach_the_screen_as_two_rows() {
    // The addendum's rule at the adapter: the provider's measurement and the
    // seat's claim about the same gate are never merged, and each says which
    // it is. The observed row is signed by the **provider**, because since
    // REVIEW-L5 F2 that is the only signer whose `observed` claim is honoured.
    let seat = fixed_keys(0x22);
    let provider = fixed_keys(0x66);
    let events = vec![
        observation(
            &seat,
            "gate",
            "declared",
            None,
            gate_body(
                "cargo test",
                "passed",
                "cargo test -p beekeeper-cli",
                Some("13 passed"),
                // A declared row may still name a commit; it is never evidence
                // for a landing, and the surface says which word it carries.
                Some(("7f".repeat(20).as_str(), false)),
            ),
        ),
        observation(
            &provider,
            "gate",
            "observed",
            None,
            gate_body(
                "cargo test",
                "failed",
                "cargo test -p beekeeper-cli",
                Some("test result: FAILED. 0 passed; 2 failed"),
                None,
            ),
        ),
    ];
    let response = fold_adapter(request(&events, Vec::new())).expect("fold");
    assert_eq!(response.gates.len(), 2);
    let observed = response
        .gates
        .iter()
        .find(|row| row.source == "observed")
        .expect("the observed row");
    assert_eq!(observed.outcome, "failed");
}

#[test]
fn a_dangling_assignment_pointer_is_disclosed_and_excludes_nothing() {
    let seat = fixed_keys(0x33);
    let dangling = "ab".repeat(32);
    let events = vec![observation(
        &seat,
        "checkpoint",
        "declared",
        Some(&dangling),
        checkpoint_body("green"),
    )];
    let response = fold_adapter(request(&events, Vec::new())).expect("fold");
    assert_eq!(
        response.checkpoints.len(),
        1,
        "an observation cannot deny anything, its own pointer included"
    );
    assert_eq!(response.unresolved.len(), 1);
    assert_eq!(response.unresolved[0].assignment_ref, dangling);
}

#[test]
fn an_unreadable_event_is_listed_rather_than_failing_the_whole_fold() {
    let seat = fixed_keys(0x44);
    let good = observation(
        &seat,
        "gate",
        "declared",
        None,
        gate_body("just ci", "passed", "just ci", None, None),
    );
    let malformed = EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        "not json".to_owned(),
    )
    .sign_with_keys(&seat)
    .expect("sign");
    let response = fold_adapter(request(&[malformed, good], Vec::new())).expect("fold");
    assert_eq!(response.gates.len(), 1);
    assert_eq!(response.ignored.len(), 1);
}

#[test]
fn a_request_with_the_wrong_schema_is_refused_by_name() {
    let mut bad = request(&[], Vec::new());
    bad.schema = "buzz-coding-session-team-fold-request/v1".to_owned();
    let error = fold_adapter(bad).expect_err("a foreign request schema is refused");
    assert!(
        error.contains(CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA),
        "{error}"
    );
}

#[test]
fn the_disclosure_sentence_is_the_clis_own() {
    // `bee sessions observations` prints this string as its `disclosure`
    // field. Desktop does not depend on the CLI crate, so the two are held
    // together here rather than by an import: change one and this test names
    // the other.
    assert_eq!(
        OBSERVATION_DISCLOSURE,
        "an observation is something its author saw, not a decision: it settles nothing, \
         authorizes nothing and excludes nothing, and every duration in it is the author's own \
         measurement"
    );
}

/// Path of the fixture the Desktop decoder test reads, relative to this crate.
const TS_DECODER_FIXTURE: &str =
    "../src/features/coding-sessions/lib/codingSessionObservationFoldAdapterResponse.fixture.json";

#[test]
fn the_typescript_decoder_fixture_is_this_adapter_s_real_output() {
    // Regenerate with
    // `BEEKEEPER_UPDATE_FIXTURES=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml the_typescript_decoder_fixture_is_this_adapter_s_real_output`.
    let seat = fixed_keys(0x55);
    let provider = fixed_keys(0x66);
    let assignment = "cd".repeat(32);
    let dangling = "ef".repeat(32);
    let events = vec![
        observation(
            &seat,
            "checkpoint",
            "declared",
            Some(&assignment),
            checkpoint_body("gates"),
        ),
        observation(
            &seat,
            "gate",
            "declared",
            Some(&assignment),
            gate_body(
                "cargo test",
                "passed",
                "cargo test -p beekeeper-cli",
                Some("13 passed"),
                // A declared row may still name a commit; it is never evidence
                // for a landing, and the surface says which word it carries.
                Some(("7f".repeat(20).as_str(), false)),
            ),
        ),
        observation(
            &provider,
            "gate",
            "observed",
            None,
            gate_body(
                "cargo test",
                "failed",
                "cargo test -p beekeeper-cli",
                Some("test result: FAILED. 0 passed; 2 failed; 0 ignored"),
                Some(("3a".repeat(20).as_str(), true)),
            ),
        ),
        observation(
            &seat,
            "finding",
            "declared",
            Some(&assignment),
            finding_body("A3", "fixed"),
        ),
        observation(
            &seat,
            "phase",
            "declared",
            Some(&dangling),
            phase_body("green"),
        ),
        // SV-41: one start the provider closed, and one still open.
        observation(
            &provider,
            "phase",
            "observed",
            None,
            gate_start_body("cargo test", 1_756_800_000_000, None),
        ),
        observation(
            &provider,
            "phase",
            "observed",
            None,
            gate_start_body("cargo test", 1_756_800_000_000, Some(1_756_800_180_000)),
        ),
        observation(
            &provider,
            "phase",
            "observed",
            None,
            gate_start_body("cargo clippy", 1_756_800_200_000, None),
        ),
    ];
    // Written oldest-first above so it reads as a session unfolding; handed
    // over newest-first, the relay's page order, which is what the adapter
    // takes (finding 79). The fold reverses back to the order above before
    // it reads anything, so every folded collection in the stored fixture is
    // unchanged; only `inputEventIds`, the request echo, follows this order.
    let events: Vec<Event> = events.into_iter().rev().collect();
    let response =
        fold_adapter(request(&events, vec![assignment.clone()])).expect("canonical fold");
    let generated = serde_json::to_string_pretty(&response).expect("serialize response") + "\n";

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TS_DECODER_FIXTURE);
    if std::env::var("BEEKEEPER_UPDATE_FIXTURES").is_ok() {
        std::fs::write(&path, &generated).expect("write fixture");
    }
    let stored = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("read {}: {error}", path.display());
    });
    assert_eq!(
        stored, generated,
        "the Desktop decoder fixture is stale; regenerate it with BEEKEEPER_UPDATE_FIXTURES=1"
    );

    // The fixture must actually exercise every collection the decoder reads,
    // or it proves nothing about the shape it exists to pin.
    let wire: Value = serde_json::from_str(&generated).expect("fixture JSON");
    assert_eq!(wire["checkpoints"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        wire["gates"].as_array().map(Vec::len),
        Some(2),
        "one observed and one declared"
    );
    assert_eq!(wire["findings"].as_array().map(Vec::len), Some(1));
    assert_eq!(wire["phases"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        wire["gateStarts"].as_array().map(Vec::len),
        Some(2),
        "one closed start and one open"
    );
    assert_eq!(wire["unresolved"].as_array().map(Vec::len), Some(1));
    assert_eq!(wire["ignored"], json!([]));
}

/// The body of a provider's gate start (`ended: None`) or its close.
fn gate_start_body(gate: &str, started: u64, ended: Option<u64>) -> Value {
    json!({
        "phase": format!("gate:{gate}"),
        "startedAtMs": started,
        "endedAtMs": ended.map_or(Value::Null, |end| json!(end)),
        "durationMs": ended.map_or(Value::Null, |end| json!(end - started)),
    })
}

/// SV-41 at the adapter: an observed `gate:` phase leaves `phases`, arrives in
/// `gateStarts` paired with its close, and the stale rule's number rides along
/// so TypeScript never copies it.
#[test]
fn a_gate_start_reaches_the_screen_paired_and_never_as_a_phase() {
    let provider = fixed_keys(0x66);
    let open = observation(
        &provider,
        "phase",
        "observed",
        None,
        gate_start_body("cargo clippy", 1_756_800_200_000, None),
    );
    let started = observation(
        &provider,
        "phase",
        "observed",
        None,
        gate_start_body("cargo test", 1_756_800_000_000, None),
    );
    let closed = observation(
        &provider,
        "phase",
        "observed",
        None,
        gate_start_body("cargo test", 1_756_800_000_000, Some(1_756_800_180_000)),
    );
    let close_id = closed.id.to_hex();
    // Newest first, as the relay pages: the close ahead of its own start.
    let response = fold_adapter(request(&[open, closed, started], Vec::new())).expect("fold");
    let wire = serde_json::to_value(&response).expect("serialize");
    assert_eq!(
        wire["phases"],
        json!([]),
        "a start is not a phase anybody timed"
    );
    assert_eq!(wire["gates"], json!([]), "a start is never a gate outcome");
    assert_eq!(wire["gateStartStaleAfterMs"], json!(30 * 60 * 1_000));
    let starts = wire["gateStarts"].as_array().expect("gateStarts");
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0]["gate"], json!("cargo test"));
    assert_eq!(starts[0]["closeEventId"], json!(close_id));
    assert_eq!(starts[0]["endedAtMs"], json!(1_756_800_180_000u64));
    assert_eq!(starts[0]["durationMs"], json!(180_000));
    assert_eq!(starts[1]["gate"], json!("cargo clippy"));
    assert_eq!(starts[1]["closeEventId"], Value::Null);
    assert_eq!(starts[1]["endedAtMs"], Value::Null);
    assert_eq!(starts[1]["durationMs"], Value::Null);
    assert_eq!(wire["truncated"]["gateStarts"], json!(0));
    assert_eq!(wire["truncated"]["gateStartClosesUnmatched"], json!(0));
}

/// REVIEW-L5 **F2** at the adapter: a seat-signed `observed` row arrives as
/// `declared`, and the claim it made is disclosed rather than repeated.
#[test]
fn a_seat_claiming_observed_is_folded_as_declared_and_listed() {
    let seat = fixed_keys(0x77);
    let events = vec![observation(
        &seat,
        "gate",
        "observed",
        None,
        gate_body(
            "cargo test",
            "passed",
            "cargo test -p beekeeper-cli",
            None,
            None,
        ),
    )];
    let response = fold_adapter(request(&events, Vec::new())).expect("fold");
    assert_eq!(response.gates.len(), 1);
    assert_eq!(response.gates[0].source, "declared");
    assert_eq!(response.misclaimed_observed.len(), 1);
    assert_eq!(
        response.misclaimed_observed[0].author_pubkey,
        seat.public_key().to_hex()
    );
    assert!(response.provenance_checked);
}

/// A caller that resolved no provider set says so, and downgrades nothing.
#[test]
fn an_unresolved_provider_set_checks_nothing_and_says_so() {
    let seat = fixed_keys(0x77);
    let events = vec![observation(
        &seat,
        "gate",
        "observed",
        None,
        gate_body(
            "cargo test",
            "passed",
            "cargo test -p beekeeper-cli",
            None,
            None,
        ),
    )];
    let mut unchecked = request(&events, Vec::new());
    unchecked.provider_pubkeys = None;
    let response = fold_adapter(unchecked).expect("fold");
    assert_eq!(response.gates[0].source, "observed");
    assert!(response.misclaimed_observed.is_empty());
    assert!(!response.provenance_checked);
}

/// REVIEW-L5 **F1**, second half, at the adapter: a replaced row is counted.
#[test]
fn a_replaced_gate_row_is_counted_in_the_adapters_truncation() {
    let provider = fixed_keys(0x66);
    // The relay's page order: newest first. The adapter folds it as a page
    // (finding 79), so the `passed` row listed first here is the newer one.
    let events = vec![
        observation(
            &provider,
            "gate",
            "observed",
            None,
            gate_body(
                "cargo test",
                "passed",
                "cargo test -p beekeeper-cli",
                None,
                None,
            ),
        ),
        observation(
            &provider,
            "gate",
            "observed",
            None,
            gate_body(
                "cargo test",
                "failed",
                "cargo test -p beekeeper-cli",
                None,
                None,
            ),
        ),
    ];
    let response = fold_adapter(request(&events, Vec::new())).expect("fold");
    assert_eq!(response.gates.len(), 1);
    assert_eq!(response.gates[0].outcome, "passed");
    assert_eq!(
        response.gates[0].event_ids,
        vec![events[1].id.to_hex(), events[0].id.to_hex()],
        "both ids listed, oldest first"
    );
    assert_eq!(response.truncated.displaced_gates, 1);
}

/// **Finding 79** at the adapter. A relay page arrives newest-first; folded
/// as read, the oldest row per `(author, gate)` was the one Desktop showed.
/// Handing the same two events over in the other order — as a caller that
/// sorted them oldest-first would — proves the contract is the relay's order
/// and nothing else: the answer flips.
#[test]
fn the_adapter_takes_the_relay_page_newest_first_and_shows_the_newest_row() {
    let provider = fixed_keys(0x66);
    let red_at_base = observation(
        &provider,
        "gate",
        "observed",
        None,
        gate_body(
            "cargo fmt",
            "failed",
            "cargo fmt --all --check",
            Some("exit 127"),
            Some(("1b".repeat(20).as_str(), true)),
        ),
    );
    let green_on_head = observation(
        &provider,
        "gate",
        "observed",
        None,
        gate_body(
            "cargo fmt",
            "passed",
            "cargo fmt --all --check",
            None,
            Some(("2f".repeat(20).as_str(), false)),
        ),
    );

    let as_the_relay_returns_it = vec![green_on_head.clone(), red_at_base.clone()];
    let response = fold_adapter(request(&as_the_relay_returns_it, Vec::new())).expect("fold");
    assert_eq!(response.gates.len(), 1);
    assert_eq!(response.gates[0].outcome, "passed", "the newest row wins");

    let sorted_oldest_first = vec![red_at_base, green_on_head];
    let response = fold_adapter(request(&sorted_oldest_first, Vec::new())).expect("fold");
    assert_eq!(
        response.gates[0].outcome, "failed",
        "a caller that reorders the page gets the other answer — the order is the contract"
    );
}
