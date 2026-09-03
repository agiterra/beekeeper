//! Conformance suite for kind 44246 content, bounds and envelope.
//!
//! Every closed token is accepted by name and one neighbour is refused by
//! name, because a vocabulary nobody tested the edges of is a vocabulary two
//! builds will disagree about.

use nostr::{EventBuilder, Keys, Kind, Tag};

use crate::kind::KIND_CODING_SESSION_POLICY;
use serde_json::{json, Value};

use super::*;

const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const GENESIS: &str = "ce5d87ed1b9b4416bb0aa37ea0fb451f211289c54099882917f4ad538d51519b";

/// A complete, legal payload of each type, as JSON.
fn payload_json(observation_type: &str, body: Value) -> Value {
    json!({
        "schema": CODING_SESSION_OBSERVATION_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": GENESIS,
        "type": observation_type,
        "source": "declared",
        "assignmentRef": Value::Null,
        "body": body,
    })
}

fn checkpoint_body() -> Value {
    json!({
        "phase": "red",
        "testsWritten": 4,
        "testsRed": 4,
        "testsGreen": 0,
        "lastCommand": "cargo test -p buzz-core coding_session_observation",
        "lastSummary": "4 failed",
        "note": Value::Null,
    })
}

fn gate_body() -> Value {
    json!({
        "rows": [{
            "gate": "cargo fmt --check",
            "outcome": "passed",
            "command": "cargo fmt --check",
            "summary": Value::Null,
            "durationMs": 1_200,
        }],
    })
}

fn finding_body() -> Value {
    json!({
        "findingId": "16",
        "title": "a founder-held request with no blocks sets no waiting state",
        "disposition": "fixed",
        "detail": Value::Null,
        "refs": [GENESIS],
        "decisionRef": Value::Null,
    })
}

fn phase_body() -> Value {
    json!({
        "phase": "red",
        "startedAtMs": 1_756_800_000_000u64,
        "endedAtMs": Value::Null,
        "durationMs": Value::Null,
    })
}

fn decode(value: &Value) -> Result<CodingSessionObservationPayload, String> {
    decode_coding_session_observation(&value.to_string())
}

/// Replace one key inside `body` and decode the result.
fn with_body_key(
    observation_type: &str,
    mut body: Value,
    key: &str,
    value: Value,
) -> Result<CodingSessionObservationPayload, String> {
    body.as_object_mut()
        .expect("body object")
        .insert(key.into(), value);
    decode(&payload_json(observation_type, body))
}

#[test]
fn every_closed_token_is_accepted_and_its_neighbours_are_refused_by_name() {
    for phase in ["planning", "red", "green", "gates", "reporting"] {
        let mut body = checkpoint_body();
        body.as_object_mut()
            .expect("body")
            .insert("phase".into(), json!(phase));
        decode(&payload_json("checkpoint", body))
            .unwrap_or_else(|error| panic!("checkpoint phase {phase:?}: {error}"));
    }
    // The neighbours a reader would guess: 44244's own loop words, and the
    // gate outcome vocabulary, which is a different closed set in a different
    // position.
    for wrong in ["planning ", "Red", "review", "passed", "refactor"] {
        let error = with_body_key("checkpoint", checkpoint_body(), "phase", json!(wrong))
            .expect_err("an unknown checkpoint phase is refused");
        assert!(error.contains("phase"), "{error}");
    }

    for outcome in ["passed", "failed", "not-run"] {
        let mut body = gate_body();
        body["rows"][0]["outcome"] = json!(outcome);
        decode(&payload_json("gate", body))
            .unwrap_or_else(|error| panic!("gate outcome {outcome:?}: {error}"));
    }
    for wrong in ["notRun", "not_run", "skipped", "red", "pass"] {
        let mut body = gate_body();
        body["rows"][0]["outcome"] = json!(wrong);
        let error = decode(&payload_json("gate", body)).expect_err("unknown gate outcome");
        assert!(
            error.contains("outcome") || error.contains("unknown variant"),
            "{error}"
        );
    }

    for disposition in ["found", "fixed", "cross-lane", "needs-ruling", "wont-fix"] {
        let mut body = finding_body();
        body.as_object_mut()
            .expect("body")
            .insert("disposition".into(), json!(disposition));
        decode(&payload_json("finding", body))
            .unwrap_or_else(|error| panic!("disposition {disposition:?}: {error}"));
    }
    for wrong in ["crossLane", "cross_lane", "wontfix", "won't-fix", "open"] {
        let error = with_body_key("finding", finding_body(), "disposition", json!(wrong))
            .expect_err("an unknown disposition is refused");
        assert!(
            error.contains("disposition") || error.contains("unknown variant"),
            "{error}"
        );
    }

    for observation_type in ["checkpoint", "gate", "finding", "phase"] {
        let body = match observation_type {
            "checkpoint" => checkpoint_body(),
            "gate" => gate_body(),
            "finding" => finding_body(),
            _ => phase_body(),
        };
        decode(&payload_json(observation_type, body))
            .unwrap_or_else(|error| panic!("type {observation_type:?}: {error}"));
    }
    for wrong in [
        "checkpoint.report",
        "gates",
        "note",
        "decision.request",
        "observation",
    ] {
        let error = decode(&payload_json(wrong, checkpoint_body()))
            .expect_err("an unknown observation type is refused");
        assert!(
            error.contains("unsupported coding-session observation type"),
            "{error}"
        );
        assert!(
            error.contains("\"checkpoint\""),
            "the refusal names the set: {error}"
        );
    }
}

#[test]
fn an_unknown_key_is_refused_by_name_at_every_level() {
    let mut top = payload_json("checkpoint", checkpoint_body());
    top.as_object_mut()
        .expect("payload")
        .insert("supersedes".into(), json!(GENESIS));
    let error = decode(&top).expect_err("there is no supersedes key on an observation");
    assert!(error.contains("\"supersedes\""), "{error}");
    assert!(
        error.contains("rejects unknown fields rather than ignoring them"),
        "{error}"
    );

    let error = with_body_key("checkpoint", checkpoint_body(), "testsSkipped", json!(1))
        .expect_err("an unknown body key is refused");
    assert!(error.contains("\"testsSkipped\""), "{error}");

    let mut body = gate_body();
    body["rows"][0]["exitCode"] = json!(0);
    let error = decode(&payload_json("gate", body)).expect_err("an unknown row key is refused");
    assert!(error.contains("\"exitCode\""), "{error}");
}

#[test]
fn absent_is_not_null_and_a_required_null_is_refused_by_name() {
    // Absent: every key is always present, so a missing optional is refused
    // rather than defaulted.
    let mut body = checkpoint_body();
    body.as_object_mut().expect("body").remove("note");
    let error = decode(&payload_json("checkpoint", body)).expect_err("a missing key is refused");
    assert!(error.contains("is missing \"note\""), "{error}");
    assert!(
        error.contains("written as JSON null rather than omitted"),
        "{error}"
    );

    // Null where a value is required: refused, and the refusal names the key.
    let error = with_body_key("checkpoint", checkpoint_body(), "testsWritten", Value::Null)
        .expect_err("null is not a test count");
    assert!(
        error.contains("\"testsWritten\" requires a value"),
        "{error}"
    );
    assert!(error.contains("absent is not null"), "{error}");

    let error = with_body_key("phase", phase_body(), "startedAtMs", Value::Null)
        .expect_err("null is not a measurement");
    assert!(
        error.contains("\"startedAtMs\" requires a value"),
        "{error}"
    );

    let mut top = payload_json("checkpoint", checkpoint_body());
    top.as_object_mut()
        .expect("payload")
        .insert("sessionRef".into(), Value::Null);
    let error = decode(&top).expect_err("null is not a session");
    assert!(error.contains("\"sessionRef\" requires a value"), "{error}");

    // And the one key that IS nullable stays nullable.
    assert_eq!(
        decode(&payload_json("checkpoint", checkpoint_body()))
            .expect("assignmentRef null is legal")
            .assignment_ref,
        None
    );
}

#[test]
fn a_gate_observation_with_no_rows_is_refused() {
    // `validate_non_empty_collection`'s precedent, one kind over: an empty
    // array is a record that claims to state something and states nothing.
    let error = decode(&payload_json("gate", json!({ "rows": [] })))
        .expect_err("an empty gate observation is refused");
    assert!(error.contains("gate rows must not be empty"), "{error}");
}

#[test]
fn a_gate_observation_bounds_and_deduplicates_its_own_rows() {
    let row = |name: &str| {
        json!({
            "gate": name,
            "outcome": "passed",
            "command": "cargo test",
            "summary": Value::Null,
            "durationMs": Value::Null,
        })
    };
    let rows: Vec<Value> = (0..MAX_OBSERVATION_GATE_ROWS)
        .map(|index| row(&format!("gate-{index}")))
        .collect();
    decode(&payload_json("gate", json!({ "rows": rows.clone() })))
        .expect("exactly 32 rows is legal");

    let mut too_many = rows;
    too_many.push(row("gate-32"));
    let error = decode(&payload_json("gate", json!({ "rows": too_many })))
        .expect_err("a 33rd row is refused");
    assert!(
        error.contains("gate rows exceed 32 entries (got 33)"),
        "{error}"
    );

    let error = decode(&payload_json(
        "gate",
        json!({ "rows": [row("just ci"), row("just ci")] }),
    ))
    .expect_err("one observation states each gate once");
    assert!(error.contains("names \"just ci\" twice"), "{error}");
}

#[test]
fn a_finding_bounds_and_deduplicates_its_pointers() {
    let reference = |byte: u8| format!("{byte:02x}").repeat(32);
    let refs: Vec<Value> = (0..MAX_OBSERVATION_FINDING_REFS)
        .map(|index| json!(reference(index as u8)))
        .collect();
    with_body_key("finding", finding_body(), "refs", json!(refs.clone()))
        .expect("exactly 16 pointers is legal");

    let mut too_many = refs;
    too_many.push(json!(reference(16)));
    let error = with_body_key("finding", finding_body(), "refs", json!(too_many))
        .expect_err("a 17th pointer is refused");
    assert!(
        error.contains("finding refs exceed 16 entries (got 17)"),
        "{error}"
    );

    let error = with_body_key(
        "finding",
        finding_body(),
        "refs",
        json!([reference(1), reference(1)]),
    )
    .expect_err("a duplicated pointer is refused");
    assert!(error.contains("must not contain duplicates"), "{error}");

    let error = with_body_key("finding", finding_body(), "refs", json!(["not-hex"]))
        .expect_err("a pointer that is not an event id is refused");
    assert!(error.contains("lowercase 64-hex event id"), "{error}");
}

#[test]
fn the_envelope_is_five_ordered_tags_that_agree_with_the_content() {
    let keys = Keys::generate();
    let payload = payload_json("gate", gate_body()).to_string();
    let tags = |version: &str, session: &str, genesis: &str, observation_type: &str| {
        vec![
            Tag::parse(["h", CHANNEL]).expect("h"),
            Tag::parse(["d", session]).expect("d"),
            Tag::parse(["csob-v", version]).expect("csob-v"),
            Tag::parse(["csob-genesis", genesis]).expect("csob-genesis"),
            Tag::parse(["csob-type", observation_type]).expect("csob-type"),
        ]
    };
    let sign = |tags: Vec<Tag>| {
        EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
            payload.clone(),
        )
        .tags(tags)
        .sign_with_keys(&keys)
        .expect("sign")
    };

    let good = sign(tags(
        CODING_SESSION_OBSERVATION_SCHEMA,
        SESSION,
        GENESIS,
        "gate",
    ));
    let decoded = validate_coding_session_observation_envelope(&good).expect("the exact envelope");
    assert_eq!(decoded.observation_type, CodingSessionObservationType::Gate);

    // Each of the four disagreements a wrong tag can express.
    for (tags, expected) in [
        (
            tags(CODING_SESSION_OBSERVATION_SCHEMA, CHANNEL, GENESIS, "gate"),
            "d tag does not match payload sessionRef",
        ),
        (
            tags(
                "buzz-coding-session-observation/v2",
                SESSION,
                GENESIS,
                "gate",
            ),
            "unsupported coding-session observation tag version",
        ),
        (
            tags(
                CODING_SESSION_OBSERVATION_SCHEMA,
                SESSION,
                &"ab".repeat(32),
                "gate",
            ),
            "genesis tag does not match payload genesisRef",
        ),
        (
            tags(
                CODING_SESSION_OBSERVATION_SCHEMA,
                SESSION,
                GENESIS,
                "checkpoint",
            ),
            "type tag does not match payload type",
        ),
    ] {
        let error = validate_coding_session_observation_envelope(&sign(tags))
            .expect_err("a disagreeing tag is refused");
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error}"
        );
    }

    // A sixth tag, and a five-tag envelope in the wrong order.
    let mut extra = tags(CODING_SESSION_OBSERVATION_SCHEMA, SESSION, GENESIS, "gate");
    extra.push(Tag::parse(["e", GENESIS]).expect("e"));
    let error = validate_coding_session_observation_envelope(&sign(extra))
        .expect_err("a sixth tag is refused");
    assert!(error.contains("exactly five two-field tags"), "{error}");

    let mut reordered = tags(CODING_SESSION_OBSERVATION_SCHEMA, SESSION, GENESIS, "gate");
    reordered.swap(0, 1);
    let error = validate_coding_session_observation_envelope(&sign(reordered))
        .expect_err("reordered tags are refused");
    assert!(
        error.contains("first tag must be h=channel UUID"),
        "{error}"
    );

    // And the wrong kind entirely.
    let wrong_kind = EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_POLICY as u16), payload)
        .tags(tags(
            CODING_SESSION_OBSERVATION_SCHEMA,
            SESSION,
            GENESIS,
            "gate",
        ))
        .sign_with_keys(&keys)
        .expect("sign");
    let error = validate_coding_session_observation_envelope(&wrong_kind)
        .expect_err("kind 44245 is not an observation");
    assert!(error.contains("wrong event kind"), "{error}");
}

#[test]
fn a_payload_round_trips_through_serde_with_every_key_present() {
    for (observation_type, body) in [
        ("checkpoint", checkpoint_body()),
        ("gate", gate_body()),
        ("finding", finding_body()),
        ("phase", phase_body()),
    ] {
        let payload = decode(&payload_json(observation_type, body)).expect("decode");
        let written = serde_json::to_string(&payload).expect("serialize");
        assert_eq!(
            decode_coding_session_observation(&written).expect("re-decode"),
            payload
        );
        let object: Value = serde_json::from_str(&written).expect("json");
        let object = object.as_object().expect("object");
        assert_eq!(object.len(), 7, "exactly seven top-level keys: {written}");
        for key in [
            "schema",
            "sessionRef",
            "genesisRef",
            "type",
            "source",
            "assignmentRef",
            "body",
        ] {
            assert!(object.contains_key(key), "{key} is missing from {written}");
        }
    }
}

#[test]
fn the_bounds_each_field_carries_are_the_ones_the_vocabulary_names() {
    let long = |bytes: usize| "x".repeat(bytes);

    assert!(with_body_key(
        "checkpoint",
        checkpoint_body(),
        "lastCommand",
        json!(long(MAX_OBSERVATION_COMMAND_BYTES))
    )
    .is_ok());
    let error = with_body_key(
        "checkpoint",
        checkpoint_body(),
        "lastCommand",
        json!(long(MAX_OBSERVATION_COMMAND_BYTES + 1)),
    )
    .expect_err("a 513-byte command is refused");
    assert!(error.contains("exceeds 512 bytes"), "{error}");

    let error = with_body_key(
        "checkpoint",
        checkpoint_body(),
        "lastSummary",
        json!(long(MAX_OBSERVATION_SUMMARY_BYTES + 1)),
    )
    .expect_err("a 2049-byte summary is refused");
    assert!(error.contains("exceeds 2048 bytes"), "{error}");

    let error = with_body_key(
        "checkpoint",
        checkpoint_body(),
        "note",
        json!(long(MAX_OBSERVATION_PROSE_BYTES + 1)),
    )
    .expect_err("an 8193-byte note is refused");
    assert!(error.contains("exceeds 8192 bytes"), "{error}");

    let error = with_body_key(
        "finding",
        finding_body(),
        "findingId",
        json!(long(MAX_OBSERVATION_NAME_BYTES + 1)),
    )
    .expect_err("a 65-byte findingId is refused");
    assert!(error.contains("exceeds 64 bytes"), "{error}");

    let error = with_body_key(
        "finding",
        finding_body(),
        "title",
        json!(long(MAX_OBSERVATION_TITLE_BYTES + 1)),
    )
    .expect_err("a 513-byte title is refused");
    assert!(error.contains("exceeds 512 bytes"), "{error}");

    // A note keeps its newlines; a summary does not get to smuggle one.
    assert!(with_body_key("checkpoint", checkpoint_body(), "note", json!("two\nlines")).is_ok());
    let error = with_body_key(
        "checkpoint",
        checkpoint_body(),
        "lastSummary",
        json!("two\nlines"),
    )
    .expect_err("a summary is one line");
    assert!(error.contains("control characters"), "{error}");
}

#[test]
fn a_checkpoint_cannot_claim_more_red_or_green_than_it_wrote() {
    let error = with_body_key("checkpoint", checkpoint_body(), "testsRed", json!(9))
        .expect_err("more red than written is refused");
    assert!(
        error.contains("testsRed must not exceed testsWritten"),
        "{error}"
    );
    let error = with_body_key("checkpoint", checkpoint_body(), "testsGreen", json!(9))
        .expect_err("more green than written is refused");
    assert!(
        error.contains("testsGreen must not exceed testsWritten"),
        "{error}"
    );
}

#[test]
fn a_phase_cannot_end_before_it_started() {
    let mut body = phase_body();
    body.as_object_mut()
        .expect("body")
        .insert("endedAtMs".into(), json!(1u64));
    let error = decode(&payload_json("phase", body)).expect_err("a phase that ends first");
    assert!(
        error.contains("endedAtMs must not precede startedAtMs"),
        "{error}"
    );
}

#[test]
fn the_content_ceiling_is_enforced_before_anything_is_parsed() {
    let oversize = "x".repeat(MAX_CODING_SESSION_OBSERVATION_CONTENT_BYTES + 1);
    let error = decode_coding_session_observation(&oversize).expect_err("oversize content");
    assert!(error.contains("content exceeds"), "{error}");
}
