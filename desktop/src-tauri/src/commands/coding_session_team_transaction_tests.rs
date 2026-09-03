//! Red-first tests for the founder's 44244 build boundary.
//!
//! Every one of these fails on the base tree for the same reason: the command
//! and its module did not exist, so the file did not compile.

use super::*;
use serde_json::json;

const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";

fn genesis() -> String {
    "ab".repeat(32)
}

fn request_ref() -> String {
    "22".repeat(32)
}

fn answer(body: serde_json::Value) -> CodingSessionTeamTransactionBuildRequest {
    CodingSessionTeamTransactionBuildRequest {
        schema: CODING_SESSION_TEAM_TRANSACTION_BUILD_REQUEST_SCHEMA.to_owned(),
        channel_ref: CHANNEL.to_owned(),
        transaction: json!({
            "schema": CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
            "sessionRef": SESSION,
            "genesisRef": genesis(),
            "type": "decision.answer",
            "supersedes": serde_json::Value::Null,
            "deliveryCommandId": serde_json::Value::Null,
            "body": body,
        }),
    }
}

#[test]
fn an_index_answer_is_built_as_rusts_own_bytes_with_the_exact_envelope() {
    let built = build_adapter(answer(json!({
        "requestRef": request_ref(),
        "choice": 2,
        "note": serde_json::Value::Null,
    })))
    .expect("an index answer builds");

    assert_eq!(built.schema, CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA);
    assert_eq!(built.implementation, "buzz-core");
    assert_eq!(built.kind, 44244);
    assert_eq!(built.record.transaction_type, "decision.answer");
    assert_eq!(built.record.session_ref, SESSION);
    assert_eq!(built.record.genesis_ref, genesis());
    assert_eq!(built.record.body["requestRef"], json!(request_ref()));
    assert_eq!(built.record.body["choice"], json!(2));
    // The five-tag NIP-CSTX envelope, in order, produced by buzz-sdk.
    let tags: Vec<Vec<String>> = built.tags.clone();
    assert_eq!(tags.len(), 5);
    assert_eq!(tags[0], vec!["h".to_owned(), CHANNEL.to_owned()]);
    assert_eq!(tags[1], vec!["d".to_owned(), SESSION.to_owned()]);
    assert_eq!(
        tags[4],
        vec!["cstx-type".to_owned(), "decision.answer".to_owned()]
    );
    // What a keyring signs is what the decoder read.
    decode_coding_session_team_transaction(&built.content).expect("built content re-decodes");
}

#[test]
fn a_free_text_answer_carries_its_words_and_its_note() {
    let built = build_adapter(answer(json!({
        "requestRef": request_ref(),
        "choice": "neither — rebase onto main first",
        "note": "the second option would land an unverified commit",
    })))
    .expect("a free-text answer builds");

    assert_eq!(
        built.record.body["choice"],
        json!("neither — rebase onto main first")
    );
    assert_eq!(
        built.record.body["note"],
        json!("the second option would land an unverified commit")
    );
}

#[test]
fn an_over_long_choice_and_note_are_refused_in_cores_own_words_naming_the_key() {
    let long_choice = build_adapter(answer(json!({
        "requestRef": request_ref(),
        "choice": "x".repeat(CODING_SESSION_DECISION_CHOICE_MAX_BYTES + 1),
        "note": serde_json::Value::Null,
    })))
    .expect_err("an over-long choice is refused");
    assert!(
        long_choice.contains("choice"),
        "refusal must name the key: {long_choice}"
    );

    let long_note = build_adapter(answer(json!({
        "requestRef": request_ref(),
        "choice": 0,
        "note": "y".repeat(CODING_SESSION_DECISION_NOTE_MAX_BYTES + 1),
    })))
    .expect_err("an over-long note is refused");
    assert!(
        long_note.contains("note"),
        "refusal must name the key: {long_note}"
    );
}

#[test]
fn an_absent_key_and_an_unknown_key_are_both_refused() {
    // The decoder's own sentence. It does not name the key for an exact-key
    // failure — that is core's wording and this side does not improve on it.
    let missing = build_adapter(answer(json!({
        "requestRef": request_ref(),
        "choice": 0,
    })))
    .expect_err("absent is not null");
    assert_eq!(
        missing,
        "team-transaction body has missing or unsupported fields"
    );

    let unknown = build_adapter(answer(json!({
        "requestRef": request_ref(),
        "choice": 0,
        "note": serde_json::Value::Null,
        "rationale": "not a key on this body",
    })))
    .expect_err("an unknown key is refused");
    assert_eq!(
        unknown,
        "team-transaction body has missing or unsupported fields"
    );
}

#[test]
fn a_wrong_request_schema_is_refused_before_anything_is_built() {
    let mut request = answer(json!({
        "requestRef": request_ref(),
        "choice": 0,
        "note": serde_json::Value::Null,
    }));
    request.schema = "buzz-coding-session-policy-build-request/v1".to_owned();
    let error = build_adapter(request).expect_err("a foreign schema is refused");
    assert!(
        error.contains(CODING_SESSION_TEAM_TRANSACTION_BUILD_REQUEST_SCHEMA),
        "{error}"
    );
}

#[test]
fn the_condition_capability_is_measured_by_running_cores_own_decoder() {
    // The six-key body must always decode; the probe reports `false` only
    // because the seventh key is not on this build's core yet, and `true` the
    // day L7's `condition` lands — with no edit here.
    assert!(
        decode_coding_session_team_transaction(&condition_probe(false)).is_ok(),
        "the canonical six-key answer must decode on every build"
    );
    assert_eq!(
        decision_answer_condition_is_supported(),
        decode_coding_session_team_transaction(&condition_probe(true)).is_ok(),
        "the capability is the decoder's own answer, never a version number"
    );
}

#[test]
fn a_condition_is_built_when_core_accepts_it_and_refused_when_it_does_not() {
    let built = build_adapter(answer(json!({
        "requestRef": request_ref(),
        "choice": 0,
        "note": serde_json::Value::Null,
        "condition": "for every commit on this branch that keeps the gate green",
    })));
    if decision_answer_condition_is_supported() {
        let built = built.expect("core accepts condition, so the build must succeed");
        assert_eq!(
            built.record.body["condition"],
            json!("for every commit on this branch that keeps the gate green")
        );
    } else {
        let error = built.expect_err("core refuses condition, so the build must refuse");
        assert_eq!(
            error,
            "team-transaction body has missing or unsupported fields"
        );
    }
}

/// Path of the fixture the Desktop decoder test reads, relative to this crate.
const TS_DECODER_FIXTURE: &str =
    "../src/features/coding-sessions/lib/codingSessionTeamTransactionAdapterResponse.fixture.json";

#[test]
fn the_typescript_decoder_fixture_is_this_adapter_s_real_output() {
    // Regenerate with
    // `BUZZ_UPDATE_FIXTURES=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml coding_session_team_transaction`.
    let generated = serde_json::to_string_pretty(&json!({
        "note": "Generated by `the_typescript_decoder_fixture_is_this_adapter_s_real_output` in \
desktop/src-tauri/src/commands/coding_session_team_transaction_tests.rs. Do not hand-edit: a \
hand-written fixture is what let a decoder stay green while it would have thrown for every real \
record.",
        "build": build_adapter(answer(json!({
            "requestRef": request_ref(),
            "choice": 1,
            "note": "the second option would land an unverified commit",
        })))
        .expect("build"),
        "capabilities": CodingSessionTeamTransactionCapabilities {
            schema: CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA.to_owned(),
            implementation: "buzz-core".to_owned(),
            choice_max_bytes: CODING_SESSION_DECISION_CHOICE_MAX_BYTES,
            note_max_bytes: CODING_SESSION_DECISION_NOTE_MAX_BYTES,
            condition_max_bytes: CODING_SESSION_DECISION_CONDITION_MAX_BYTES,
            supports_decision_answer_condition: decision_answer_condition_is_supported(),
        },
    }))
    .expect("serialize fixture")
        + "\n";

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TS_DECODER_FIXTURE);
    if std::env::var("BUZZ_UPDATE_FIXTURES").is_ok() {
        std::fs::write(&path, &generated).expect("write fixture");
    }
    let stored = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored).expect("stored fixture is JSON");
    let generated_value: serde_json::Value =
        serde_json::from_str(&generated).expect("generated fixture is JSON");
    assert_eq!(
        stored_value, generated_value,
        "the Desktop team-transaction fixture is stale; regenerate it with BUZZ_UPDATE_FIXTURES=1"
    );
    assert_eq!(generated_value["build"]["kind"], json!(44244));
    assert_eq!(
        generated_value["build"]["record"]["type"],
        json!("decision.answer")
    );
}
