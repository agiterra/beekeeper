//! SV-41: what a gate start is, what closes it, and what it never becomes.

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::{json, Value};

use super::*;

const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const GENESIS: &str = "ce5d87ed1b9b4416bb0aa37ea0fb451f211289c54099882917f4ad538d51519b";
const T: u64 = 1_759_572_120_000;

fn context(provider: &Keys) -> CodingSessionObservationFoldContext {
    CodingSessionObservationFoldContext {
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        known_assignment_refs: Vec::new(),
        provider_pubkeys: Some(vec![provider.public_key().to_hex()]),
    }
}

fn signed(keys: &Keys, source: &str, observation_type: &str, body: Value) -> Event {
    let content = json!({
        "schema": CODING_SESSION_OBSERVATION_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": GENESIS,
        "type": observation_type,
        "source": source,
        "assignmentRef": Value::Null,
        "body": body,
    })
    .to_string();
    EventBuilder::new(
        Kind::Custom(crate::kind::KIND_CODING_SESSION_OBSERVATION as u16),
        content,
    )
    .tags(vec![
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", SESSION]).expect("d"),
        Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("csob-v"),
        Tag::parse(["csob-genesis", GENESIS]).expect("csob-genesis"),
        Tag::parse(["csob-type", observation_type]).expect("csob-type"),
    ])
    .sign_with_keys(keys)
    .expect("sign")
}

fn start(keys: &Keys, gate: &str, started_at_ms: u64) -> Event {
    signed(
        keys,
        "observed",
        "phase",
        json!({
            "phase": format!("gate:{gate}"),
            "startedAtMs": started_at_ms,
            "endedAtMs": Value::Null,
            "durationMs": Value::Null,
        }),
    )
}

fn close(keys: &Keys, gate: &str, started_at_ms: u64, ended_at_ms: u64) -> Event {
    signed(
        keys,
        "observed",
        "phase",
        json!({
            "phase": format!("gate:{gate}"),
            "startedAtMs": started_at_ms,
            "endedAtMs": ended_at_ms,
            "durationMs": ended_at_ms - started_at_ms,
        }),
    )
}

fn gate_row(keys: &Keys, gate: &str, outcome: &str) -> Event {
    signed(
        keys,
        "observed",
        "gate",
        json!({ "rows": [{
            "gate": gate,
            "outcome": outcome,
            "command": gate,
            "summary": Value::Null,
            "durationMs": 180_000,
        }] }),
    )
}

#[test]
fn the_phase_name_is_the_prefix_and_the_table_name() {
    assert_eq!(
        gate_start_phase_name("cargo test").unwrap(),
        "gate:cargo test"
    );
    assert_eq!(gate_of_start_phase("gate:cargo test"), Some("cargo test"));
    assert_eq!(gate_of_start_phase("gate:"), None);
    assert_eq!(gate_of_start_phase("red"), None);
    assert!(gate_start_phase_name("").is_err());
    assert!(gate_start_phase_name(" cargo test").is_err());
    assert!(gate_start_phase_name("cargo\ttest").is_err());
    assert!(gate_start_phase_name(&"x".repeat(60)).is_err());
}

#[test]
fn a_start_is_neither_a_gate_row_nor_a_phase() {
    let provider = Keys::generate();
    let fold =
        fold_coding_session_observations(&[start(&provider, "cargo test", T)], &context(&provider));
    assert!(fold.gates.is_empty());
    assert!(fold.phases.is_empty());
    assert!(fold.ignored.is_empty(), "{:?}", fold.ignored);
    assert_eq!(fold.gate_starts.len(), 1);
    let entry = &fold.gate_starts[0];
    assert_eq!(entry.gate, "cargo test");
    assert_eq!(entry.started_at_ms, T);
    assert_eq!(entry.author_pubkey, provider.public_key().to_hex());
    assert!(entry.close.is_none());
    assert!(!fold.truncated.any());
}

#[test]
fn the_close_ends_the_start_in_either_order() {
    let provider = Keys::generate();
    let opened = start(&provider, "cargo test", T);
    let closed = close(&provider, "cargo test", T, T + 180_000);
    let closed_id = closed.id.to_hex();
    for events in [
        vec![opened.clone(), closed.clone()],
        vec![closed.clone(), opened.clone()],
    ] {
        let fold = fold_coding_session_observations(&events, &context(&provider));
        assert_eq!(fold.gate_starts.len(), 1);
        let close = fold.gate_starts[0].close.as_ref().expect("closed");
        assert_eq!(close.event_id, closed_id);
        assert_eq!(close.ended_at_ms, T + 180_000);
        assert_eq!(close.duration_ms, Some(180_000));
        assert!(fold.phases.is_empty());
        assert_eq!(fold.truncated.gate_start_closes_unmatched, 0);
    }
    // A relay page, newest first, goes through the page fold to the same place.
    let page = fold_coding_session_observation_page(&[closed, opened], &context(&provider));
    assert_eq!(page.gate_starts.len(), 1);
    assert!(page.gate_starts[0].close.is_some());
}

/// Decision 2, pinned: a gate row names no start, so it ends none.
#[test]
fn the_finished_gate_row_alone_does_not_close_a_start() {
    let provider = Keys::generate();
    let fold = fold_coding_session_observations(
        &[
            start(&provider, "cargo test", T),
            gate_row(&provider, "cargo test", "passed"),
        ],
        &context(&provider),
    );
    assert_eq!(fold.gates.len(), 1);
    assert_eq!(fold.gate_starts.len(), 1);
    assert!(fold.gate_starts[0].close.is_none());
}

#[test]
fn a_stale_start() {
    assert!(gate_start_is_stale(T, T + GATE_START_STALE_AFTER_MS));
    assert!(!gate_start_is_stale(T, T + GATE_START_STALE_AFTER_MS - 1));
    assert!(
        !gate_start_is_stale(T, T - 1),
        "a future start is not stale"
    );
    assert!(!gate_start_is_stale(u64::MAX, 0));
    assert_eq!(GATE_START_STALE_AFTER_MS, 30 * 60 * 1_000);
}

#[test]
fn two_seats_same_gate_one_closed() {
    let provider = Keys::generate();
    let fold = fold_coding_session_observations(
        &[
            start(&provider, "cargo test", T),
            start(&provider, "cargo test", T + 4_000),
            close(&provider, "cargo test", T + 4_000, T + 60_000),
        ],
        &context(&provider),
    );
    assert_eq!(fold.gate_starts.len(), 2);
    assert!(
        fold.gate_starts[0].close.is_none(),
        "the other seat still runs"
    );
    assert!(fold.gate_starts[1].close.is_some());
}

#[test]
fn a_repeated_start_is_one_statement() {
    let provider = Keys::generate();
    let first = start(&provider, "cargo test", T);
    let first_id = first.id.to_hex();
    // Same key, different event (a re-signed replay).
    let again = start(&provider, "cargo test", T);
    let fold = fold_coding_session_observations(&[first, again], &context(&provider));
    assert_eq!(fold.gate_starts.len(), 1);
    assert_eq!(fold.gate_starts[0].event_id, first_id);
}

#[test]
fn a_close_from_another_author_closes_nothing() {
    let provider = Keys::generate();
    let other = Keys::generate();
    let fold = fold_coding_session_observations(
        &[
            start(&provider, "cargo test", T),
            close(&other, "cargo test", T, T + 1_000),
        ],
        &CodingSessionObservationFoldContext {
            provider_pubkeys: Some(vec![
                provider.public_key().to_hex(),
                other.public_key().to_hex(),
            ]),
            ..context(&provider)
        },
    );
    assert!(fold.gate_starts[0].close.is_none());
    assert_eq!(fold.truncated.gate_start_closes_unmatched, 1);
}

#[test]
fn a_declared_or_misclaimed_gate_phase_stays_a_phase() {
    let provider = Keys::generate();
    let seat = Keys::generate();
    let declared = signed(
        &seat,
        "declared",
        "phase",
        json!({
            "phase": "gate:cargo test",
            "startedAtMs": T,
            "endedAtMs": Value::Null,
            "durationMs": Value::Null,
        }),
    );
    // The seat claiming to be the watcher: folded down to declared.
    let misclaimed = start(&seat, "cargo clippy", T);
    let fold = fold_coding_session_observations(&[declared, misclaimed], &context(&provider));
    assert!(fold.gate_starts.is_empty());
    assert_eq!(fold.phases.len(), 2);
    assert_eq!(fold.misclaimed_observed.len(), 1);
}

#[test]
fn an_unchecked_provider_set_still_routes_an_observed_start() {
    let provider = Keys::generate();
    let fold = fold_coding_session_observations(
        &[start(&provider, "cargo test", T)],
        &CodingSessionObservationFoldContext {
            provider_pubkeys: None,
            ..context(&provider)
        },
    );
    assert_eq!(fold.gate_starts.len(), 1);
    assert!(!fold.provenance_checked);
}

#[test]
fn starts_are_bounded_newest_first() {
    let provider = Keys::generate();
    let events: Vec<Event> = (0..=MAX_OBSERVATION_GATE_STARTS as u64)
        .map(|offset| start(&provider, "cargo test", T + offset))
        .collect();
    let fold = fold_coding_session_observations(&events, &context(&provider));
    assert_eq!(fold.gate_starts.len(), MAX_OBSERVATION_GATE_STARTS);
    assert_eq!(fold.truncated.gate_starts, 1);
    assert!(fold.truncated.any());
    assert_eq!(
        fold.gate_starts[0].started_at_ms,
        T + 1,
        "the oldest fell off"
    );
    assert_eq!(
        fold.gate_starts.last().map(|entry| entry.started_at_ms),
        Some(T + MAX_OBSERVATION_GATE_STARTS as u64)
    );
}

#[test]
fn a_close_without_its_start_is_counted() {
    let provider = Keys::generate();
    let fold = fold_coding_session_observations(
        &[close(&provider, "cargo test", T, T + 5_000)],
        &context(&provider),
    );
    assert!(fold.gate_starts.is_empty());
    assert!(fold.phases.is_empty());
    assert_eq!(fold.truncated.gate_start_closes_unmatched, 1);
    assert!(
        !fold.truncated.any(),
        "an unmatched close is a disclosure, not a drop"
    );
}

/// The relay's ingest check and the strict decoder share this validator, so a
/// malformed start is refused at the door and never folded.
#[test]
fn a_malformed_start_is_refused_by_the_shared_validator() {
    let provider = Keys::generate();
    let malformed = [
        json!({"phase": "gate:", "startedAtMs": T, "endedAtMs": Value::Null, "durationMs": Value::Null}),
        json!({"phase": "gate: cargo test", "startedAtMs": T, "endedAtMs": Value::Null, "durationMs": Value::Null}),
        json!({"phase": "gate:cargo test", "startedAtMs": T, "endedAtMs": Value::Null, "durationMs": 5}),
        json!({"phase": "gate:cargo test", "startedAtMs": T, "endedAtMs": T - 1, "durationMs": Value::Null}),
    ];
    for body in malformed {
        let event = signed(&provider, "observed", "phase", body.clone());
        assert!(
            validate_coding_session_observation_envelope(&event).is_err(),
            "{body}"
        );
    }
    for event in [
        start(&provider, "cargo test", T),
        close(&provider, "cargo test", T, T),
    ] {
        validate_coding_session_observation_envelope(&event).expect("a well-formed start");
    }
}

/// The gate-start rule binds only a row that claims `observed`. A declared
/// `gate:` phase is its author's own words: one a seat could legally sign
/// before SV-41 (padded, or with a `durationMs` while open) still validates
/// and still folds as an ordinary phase, never `ignored`.
#[test]
fn a_declared_gate_phase_keeps_the_ordinary_phase_rules() {
    let provider = Keys::generate();
    let seat = Keys::generate();
    let bodies = [
        json!({"phase": "gate: x", "startedAtMs": T, "endedAtMs": Value::Null, "durationMs": Value::Null}),
        json!({"phase": "gate:foo", "startedAtMs": T, "endedAtMs": Value::Null, "durationMs": 5}),
        json!({"phase": "gate:", "startedAtMs": T, "endedAtMs": Value::Null, "durationMs": Value::Null}),
    ];
    let events: Vec<Event> = bodies
        .iter()
        .map(|body| signed(&seat, "declared", "phase", body.clone()))
        .collect();
    for event in &events {
        validate_coding_session_observation_envelope(event).expect("a declared gate: phase");
    }
    let fold = fold_coding_session_observations(&events, &context(&provider));
    assert!(fold.ignored.is_empty(), "{:?}", fold.ignored);
    assert!(fold.gate_starts.is_empty());
    assert_eq!(fold.phases.len(), bodies.len());
    // The ordinary phase rules still hold for a declared row.
    let backwards = signed(
        &seat,
        "declared",
        "phase",
        json!({"phase": "gate:x", "startedAtMs": T, "endedAtMs": T - 1, "durationMs": Value::Null}),
    );
    assert!(validate_coding_session_observation_envelope(&backwards).is_err());
}
