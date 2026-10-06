//! What `bee sessions observe` refuses before signing, and what
//! `bee sessions observations` prints.
//!
//! Everything here is pure: the row parser, the fold's wire shape, and the
//! compact rendering. Nothing in this file needs a relay, which is the point —
//! a refusal a seat gets only after a round trip is a refusal it pays for.

use beekeeper_core::coding_session_observation::{
    fold_coding_session_observation_page, CodingSessionObservationFoldContext,
    CodingSessionObservationGateOutcome, CODING_SESSION_OBSERVATION_SCHEMA,
};
use beekeeper_core::kind::KIND_CODING_SESSION_OBSERVATION;
use nostr::{Event, EventBuilder, Keys, Kind, Tag};

use super::*;

const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const GENESIS: &str = "ce5d87ed1b9b4416bb0aa37ea0fb451f211289c54099882917f4ad538d51519b";

fn gate_args(
    gates: Vec<&str>,
    summaries: Vec<&str>,
    durations: Vec<u64>,
) -> SessionObserveGateArgs {
    SessionObserveGateArgs {
        channel: CHANNEL.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis: Some(GENESIS.to_owned()),
        assignment: None,
        gate: gates.into_iter().map(str::to_owned).collect(),
        summary: summaries.into_iter().map(str::to_owned).collect(),
        duration_ms: durations,
        head_sha: Vec::new(),
    }
}

#[test]
fn a_gate_row_is_parsed_on_its_first_two_colons_so_a_command_may_carry_more() {
    let rows = gate_rows(&gate_args(
        vec!["cargo clippy:failed:cargo clippy --all-targets -- -D warnings"],
        vec!["3 warnings emitted"],
        vec![41_000],
    ))
    .expect("one row");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].gate, "cargo clippy");
    assert_eq!(rows[0].outcome, CodingSessionObservationGateOutcome::Failed);
    assert_eq!(
        rows[0].command, "cargo clippy --all-targets -- -D warnings",
        "everything after the second colon is the command, colons and all"
    );
    assert_eq!(rows[0].summary.as_deref(), Some("3 warnings emitted"));
    assert_eq!(rows[0].duration_ms, Some(41_000));
}

#[test]
fn a_gate_observation_with_nothing_to_say_is_refused_before_signing() {
    let error =
        gate_rows(&gate_args(Vec::new(), Vec::new(), Vec::new())).expect_err("no rows is refused");
    assert!(
        format!("{error}").contains("needs at least one --gate"),
        "{error}"
    );
}

#[test]
fn a_malformed_row_and_an_unknown_outcome_are_both_refused_by_name() {
    let error = gate_rows(&gate_args(vec!["just ci"], Vec::new(), Vec::new()))
        .expect_err("a row with no outcome");
    assert!(
        format!("{error}").contains("NAME:OUTCOME:COMMAND"),
        "{error}"
    );

    let error = gate_rows(&gate_args(
        vec!["just ci:green:just ci"],
        Vec::new(),
        Vec::new(),
    ))
    .expect_err("an outcome outside the closed set");
    assert!(
        format!("{error}").contains("must be one of: passed, failed, not-run"),
        "{error}"
    );
}

/// A row with no summary and no duration keeps both keys, as JSON null.
#[test]
fn an_unset_row_value_is_null_and_never_absent() {
    let rows = gate_rows(&gate_args(
        vec!["just ci:passed:just ci"],
        Vec::new(),
        Vec::new(),
    ))
    .expect("one row");
    assert_eq!(rows[0].summary, None);
    assert_eq!(rows[0].duration_ms, None);
}

fn observation(keys: &Keys, observation_type: &str, body: Value) -> Event {
    observation_from(keys, observation_type, "declared", body)
}

fn observation_from(keys: &Keys, observation_type: &str, source: &str, body: Value) -> Event {
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
    .sign_with_keys(keys)
    .expect("sign")
}

/// Fold `events` the way `cmd_observations` does: as a relay page, newest
/// first (finding 79). Fixtures below list events in that order.
fn folded(page_newest_first: &[Event]) -> CodingSessionObservationFold {
    fold_coding_session_observation_page(
        page_newest_first,
        &CodingSessionObservationFoldContext {
            session_ref: SESSION.to_owned(),
            genesis_ref: GENESIS.to_owned(),
            known_assignment_refs: Vec::new(),
            // This reader verifies no provenance claim, and the fold's own
            // `provenanceChecked: false` is what says so (REVIEW-L5 F2).
            provider_pubkeys: None,
        },
    )
}

#[test]
fn the_fold_prints_every_collection_even_when_it_is_empty() {
    let wire = fold_json(&folded(&[]));
    for key in [
        "checkpoints",
        "gates",
        "findings",
        "phases",
        "unresolved",
        "ignored",
        "truncated",
        "disclosure",
    ] {
        assert!(wire.get(key).is_some(), "{key} is missing from {wire}");
    }
    for key in [
        "checkpoints",
        "gates",
        "findings",
        "phases",
        "unresolved",
        "ignored",
    ] {
        assert_eq!(
            wire[key].as_array().map(Vec::len),
            Some(0),
            "{key} prints as an empty array, never as null: {wire}"
        );
    }
    for key in [
        "checkpoints",
        "gates",
        "findings",
        "phases",
        "unresolved",
        "ignored",
        "entryEventIds",
    ] {
        assert_eq!(
            wire["truncated"][key],
            json!(0),
            "every truncation count prints as a number, never as an absence: {wire}"
        );
    }
    assert_eq!(wire["disclosure"], json!(OBSERVATION_DISCLOSURE));
}

#[test]
fn the_wire_shape_names_both_event_ids_a_replaced_statement_leaves_behind() {
    let seat = Keys::generate();
    let first = observation(
        &seat,
        "finding",
        json!({
            "findingId": "16",
            "title": "waiting state does not fire",
            "disposition": "found",
            "detail": Value::Null,
            "refs": [],
            "decisionRef": Value::Null,
        }),
    );
    let second = observation(
        &seat,
        "finding",
        json!({
            "findingId": "16",
            "title": "waiting state does not fire",
            "disposition": "fixed",
            "detail": Value::Null,
            "refs": [],
            "decisionRef": Value::Null,
        }),
    );
    let ids = vec![first.id.to_hex(), second.id.to_hex()];
    // The relay page: newest first. The ids come back oldest first.
    let wire = fold_json(&folded(&[second, first]));
    assert_eq!(wire["findings"].as_array().map(Vec::len), Some(1));
    assert_eq!(wire["findings"][0]["disposition"], json!("fixed"));
    assert_eq!(wire["findings"][0]["eventIds"], json!(ids));
    assert_eq!(
        wire["findings"][0]["droppedEventIds"],
        json!(0),
        "an entry inside the bound says zero dropped, never nothing: {wire}"
    );
}

#[test]
fn compact_prints_one_row_per_fact_and_says_whose_measurement_a_time_is() {
    let seat = Keys::generate();
    let events = vec![
        observation(
            &seat,
            "checkpoint",
            json!({
                "phase": "gates",
                "testsWritten": 3,
                "testsRed": 3,
                "testsGreen": 3,
                "lastCommand": Value::Null,
                "lastSummary": Value::Null,
                "note": Value::Null,
            }),
        ),
        observation(
            &seat,
            "gate",
            json!({ "rows": [{
                "gate": "just ci",
                "outcome": "passed",
                "command": "just ci",
                "summary": Value::Null,
                "durationMs": Value::Null,
            }] }),
        ),
        observation(
            &seat,
            "phase",
            json!({
                "phase": "gates",
                "startedAtMs": 1_756_800_000_000u64,
                "endedAtMs": Value::Null,
                "durationMs": Value::Null,
            }),
        ),
    ];
    let rows = compact_rows(&folded(&events));
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows[0].starts_with("checkpoint "), "{rows:?}");
    assert!(
        rows[1].contains("gate ") && rows[1].contains("just ci passed"),
        "{rows:?}"
    );
    // Every row names how it was produced. A reader must never have to infer
    // provenance from the author key.
    assert!(
        rows.iter().all(|row| row.contains(" declared ")),
        "{rows:?}"
    );
    assert!(
        rows[2].contains("(author's own measurement)"),
        "a timestamp on the wire is a claim, and the row says so: {rows:?}"
    );
}

#[test]
fn an_unreadable_event_is_listed_rather_than_failing_the_whole_read() {
    let seat = Keys::generate();
    let good = observation(
        &seat,
        "gate",
        json!({ "rows": [{
            "gate": "just ci",
            "outcome": "passed",
            "command": "just ci",
            "summary": Value::Null,
            "durationMs": Value::Null,
        }] }),
    );
    let malformed = EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        "not json".to_owned(),
    )
    .sign_with_keys(&seat)
    .expect("sign");
    let wire = fold_json(&folded(&[malformed, good]));
    assert_eq!(wire["gates"].as_array().map(Vec::len), Some(1));
    assert_eq!(wire["ignored"].as_array().map(Vec::len), Some(1));
}

// ── Fix round 1 ────────────────────────────────────────────────────────────

/// **REVIEW-L1 F5.** The compact checkpoint row printed `tests_written` twice,
/// so a 5-written / 3-red / 2-green checkpoint rendered `… 5/5 red 3 green 2` —
/// an `n/n` that reads as a ratio and is always 100%. The only test used
/// 3/3/3 and asserted `starts_with("checkpoint ")`, so nothing caught it.
///
/// Three distinct numbers, three distinct positions, and the row says which is
/// which rather than leaving a reader to infer a denominator.
#[test]
fn the_compact_checkpoint_row_names_each_of_its_three_counts() {
    let seat = Keys::generate();
    let event = observation(
        &seat,
        "checkpoint",
        json!({
            "phase": "green",
            "testsWritten": 5,
            "testsRed": 3,
            "testsGreen": 2,
            "lastCommand": Value::Null,
            "lastSummary": Value::Null,
            "note": Value::Null,
        }),
    );
    let rows = compact_rows(&folded(&[event]));
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert!(
        row.contains("written 5") && row.contains("red 3") && row.contains("green 2"),
        "each count is labelled and distinct: {row}"
    );
    assert!(!row.contains("5/5"), "no ratio that is always 100%: {row}");
    assert!(row.contains("green"), "{row}");
    assert!(row.starts_with("checkpoint "), "{row}");
}

/// A checkpoint at the start of a lane — nothing written yet — reads as zeros
/// rather than as a blank, because zero written and unknown are different.
#[test]
fn a_checkpoint_with_no_tests_yet_prints_zeros() {
    let seat = Keys::generate();
    let event = observation(
        &seat,
        "checkpoint",
        json!({
            "phase": "planning",
            "testsWritten": 0,
            "testsRed": 0,
            "testsGreen": 0,
            "lastCommand": Value::Null,
            "lastSummary": Value::Null,
            "note": Value::Null,
        }),
    );
    let rows = compact_rows(&folded(&[event]));
    assert!(
        rows[0].contains("written 0") && rows[0].contains("red 0") && rows[0].contains("green 0"),
        "{}",
        rows[0]
    );
}

/// The provider's watched row and the seat's own claim are two rows, and the
/// reader is told which is which. Brian's 2026-09-02 ruling, at the CLI.
#[test]
fn an_observed_gate_row_and_a_declared_one_are_two_rows_that_name_their_source() {
    let seat = Keys::generate();
    let rows_body = |outcome: &str| {
        json!({ "rows": [{
            "gate": "cargo test -p beekeeper-cli",
            "outcome": outcome,
            "command": "cargo test -p beekeeper-cli",
            "summary": Value::Null,
            "durationMs": Value::Null,
        }] })
    };
    let declared = observation_from(&seat, "gate", "declared", rows_body("passed"));
    let observed = observation_from(&seat, "gate", "observed", rows_body("failed"));
    let wire = fold_json(&folded(&[declared, observed]));
    let gates = wire["gates"].as_array().expect("gates").clone();
    assert_eq!(gates.len(), 2, "{wire}");
    let by_source: Vec<&str> = gates
        .iter()
        .map(|row| row["source"].as_str().unwrap_or_default())
        .collect();
    assert!(by_source.contains(&"observed"), "{wire}");
    assert!(by_source.contains(&"declared"), "{wire}");
    let observed_row = gates
        .iter()
        .find(|row| row["source"] == json!("observed"))
        .expect("observed row");
    assert_eq!(observed_row["outcome"], json!("failed"), "{wire}");
}

/// REVIEW-L5 F2: the reader states that it verified nothing, rather than
/// letting a self-asserted `observed` pass for a watched measurement.
#[test]
fn the_reader_says_it_checked_no_provenance() {
    let seat = Keys::generate();
    let claimed = observation_from(
        &seat,
        "gate",
        "observed",
        json!({ "rows": [{
            "gate": "cargo test",
            "outcome": "passed",
            "command": "cargo test -p beekeeper-cli",
            "summary": Value::Null,
            "durationMs": Value::Null,
        }] }),
    );
    let wire = fold_json(&folded(&[claimed]));
    assert_eq!(wire["provenanceChecked"], json!(false));
    assert_eq!(wire["misclaimedObserved"], json!([]));
}

// ── Finding 79: the page is newest-first, and the reader must reverse ──────

/// **Finding 79.** `bee sessions observations` folded the relay's page as
/// read. The relay hands it over newest-first, and the fold's "newest" is
/// last in the order supplied, so the command rendered a seat's *first* row
/// per gate as its current one — the same wrong winner the relay's push gate
/// crowned. Andy's run: `cargo fmt` red and dirty at the base commit, four
/// later green rows on the pushed commit, and the screen showed the red one.
///
/// Both renderings, JSON and compact, now show the newest row, and the JSON
/// still lists both ids, oldest first.
#[test]
fn the_reader_renders_the_newest_row_when_the_page_arrives_newest_first() {
    let provider = Keys::generate();
    let row = |outcome: &str, head_sha: &str, dirty: bool| {
        json!({ "rows": [{
            "gate": "cargo fmt",
            "outcome": outcome,
            "command": "cargo fmt --all --check",
            "summary": Value::Null,
            "durationMs": Value::Null,
            "headSha": head_sha,
            "dirty": dirty,
        }] })
    };
    let red_at_base = observation_from(
        &provider,
        "gate",
        "observed",
        row("failed", "1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b", true),
    );
    let green_on_head = observation_from(
        &provider,
        "gate",
        "observed",
        row("passed", "2f73f558f2f73f558f2f73f558f2f73f558f2f73", false),
    );
    let ids = vec![red_at_base.id.to_hex(), green_on_head.id.to_hex()];
    // The relay's page, newest first — exactly what `query_all` returns.
    let page = vec![green_on_head, red_at_base];

    let wire = fold_json(&folded(&page));
    assert_eq!(wire["gates"].as_array().map(Vec::len), Some(1), "{wire}");
    assert_eq!(
        wire["gates"][0]["outcome"],
        json!("passed"),
        "the newest row is the one rendered: {wire}"
    );
    assert_eq!(
        wire["gates"][0]["headSha"],
        json!("2f73f558f2f73f558f2f73f558f2f73f558f2f73"),
        "{wire}"
    );
    assert_eq!(
        wire["gates"][0]["eventIds"],
        json!(ids),
        "both ids listed, oldest first: {wire}"
    );
    assert_eq!(wire["truncated"]["displacedGates"], json!(1), "{wire}");

    // One gate row, plus the `truncated …` line that says a row was displaced
    // — the displacement is never silent (REVIEW-L5 F1).
    let rows = compact_rows(&folded(&page));
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows[0].contains("cargo fmt passed"),
        "compact shows the newest row: {rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("cargo fmt failed")),
        "and not the base-commit one: {rows:?}"
    );
    assert!(
        rows[1].starts_with("truncated ") && rows[1].contains("displacedGates 1"),
        "the displaced red row is counted on screen: {rows:?}"
    );
}

// ── SV-41: gate starts ───────────────────────────────────────────────────────

const STARTED: u64 = 1_759_572_120_000;

fn gate_start(keys: &Keys, gate: &str, started: u64, ended: Option<u64>) -> Event {
    observation_from(
        keys,
        "phase",
        "observed",
        json!({
            "phase": format!("gate:{gate}"),
            "startedAtMs": started,
            "endedAtMs": ended.map_or(Value::Null, |end| json!(end)),
            "durationMs": ended.map_or(Value::Null, |end| json!(end - started)),
        }),
    )
}

#[test]
fn the_reader_prints_a_gate_start_in_all_three_states() {
    let provider = Keys::generate();
    let page = vec![
        // Newest first: the close, then both starts.
        gate_start(&provider, "cargo test", STARTED, Some(STARTED + 180_000)),
        gate_start(&provider, "cargo clippy", STARTED + 1_000, None),
        gate_start(&provider, "cargo test", STARTED, None),
    ];
    let fold = folded(&page);
    assert!(fold.phases.is_empty(), "a start is not a phase line");
    let author = short(&provider.public_key().to_hex());

    let soon = compact_rows_at(&fold, STARTED + 60_000);
    assert!(
        soon.contains(&format!(
            "gate-start {author} observed cargo test started {STARTED} (author's own measurement) \
             ended {}",
            STARTED + 180_000
        )),
        "{soon:?}"
    );
    assert!(
        soon.contains(&format!(
            "gate-start {author} observed cargo clippy started {} (author's own measurement) \
             running",
            STARTED + 1_000
        )),
        "{soon:?}"
    );

    let late = compact_rows_at(&fold, STARTED + 1_000 + GATE_START_STALE_AFTER_MS);
    assert!(
        late.iter()
            .any(|row| row.contains("cargo clippy") && row.ends_with("no result observed")),
        "past the stale rule an unclosed start never reads running: {late:?}"
    );
    assert!(!late.iter().any(|row| row.ends_with("running")), "{late:?}");

    let wire = fold_json_at(&fold, STARTED + 60_000);
    assert_eq!(wire["gateStarts"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        wire["gateStarts"][0]["state"],
        json!(format!("ended {}", STARTED + 180_000))
    );
    assert_eq!(wire["gateStarts"][0]["durationMs"], json!(180_000));
    assert_eq!(wire["gateStarts"][1]["state"], json!("running"));
    assert_eq!(wire["gateStarts"][1]["closeEventId"], Value::Null);
    assert_eq!(
        wire["gateStartStaleAfterMs"],
        json!(GATE_START_STALE_AFTER_MS)
    );
    assert_eq!(wire["truncated"]["gateStarts"], json!(0));
    assert_eq!(wire["truncated"]["gateStartClosesUnmatched"], json!(0));
}

fn unmeasured_close(keys: &Keys, gate: &str, started: u64, ended: u64) -> Event {
    observation_from(
        keys,
        "phase",
        "observed",
        json!({
            "phase": format!("gate:{gate}"),
            "startedAtMs": started,
            "endedAtMs": ended,
            "durationMs": Value::Null,
        }),
    )
}

#[test]
fn an_unmeasured_close_reads_as_stopped_watching_never_ended() {
    let provider = Keys::generate();
    let page = vec![
        // Evicted / turn end / exit: the provider stopped watching.
        unmeasured_close(&provider, "cargo test", STARTED, STARTED + 180_000),
        gate_start(&provider, "cargo test", STARTED, None),
        // A clock that ran backwards: endedAtMs = startedAtMs, no span.
        unmeasured_close(&provider, "cargo clippy", STARTED + 1_000, STARTED + 1_000),
        gate_start(&provider, "cargo clippy", STARTED + 1_000, None),
    ];
    let fold = folded(&page);
    let rows = compact_rows_at(&fold, STARTED + 200_000);
    assert!(
        rows.iter().any(|row| row.contains("cargo test")
            && row.ends_with(&format!(
                "stopped watching {} (end not observed)",
                STARTED + 180_000
            ))),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("cargo clippy")
            && row.ends_with(&format!("closed {} (no measured span)", STARTED + 1_000))),
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains(" ended ")),
        "no unmeasured close says the gate ended: {rows:?}"
    );

    let wire = fold_json_at(&fold, STARTED + 200_000);
    let starts = wire["gateStarts"].as_array().cloned().unwrap_or_default();
    let test = starts
        .iter()
        .find(|entry| entry["gate"] == json!("cargo test"))
        .expect("cargo test start");
    assert_eq!(
        test["state"],
        json!(format!(
            "stopped watching {} (end not observed)",
            STARTED + 180_000
        ))
    );
    assert_eq!(test["durationMs"], Value::Null);
    assert_eq!(test["endedAtMs"], json!(STARTED + 180_000));
}

#[test]
fn a_seat_may_not_write_a_gate_phase() {
    let error = refuse_reserved_gate_phase("gate:cargo test").expect_err("reserved");
    assert!(
        error
            .to_string()
            .contains("the gate: prefix is the provider's"),
        "{error}"
    );
    refuse_reserved_gate_phase("red").expect("an ordinary phase is the seat's own");
    refuse_reserved_gate_phase("gates").expect("the checkpoint word is not the prefix");
}
