//! Unit tests for [`super`] and the Rust binding to
//! `conformance/session-display-name/fixtures/vectors.json`.

use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::Value;

use super::*;
use crate::coding_session_command::coding_session_target_key;

const VECTORS: &str =
    include_str!("../../../conformance/session-display-name/fixtures/vectors.json");
const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn target(generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "1958c6c448e05eed".into(),
        session_id: "sess-a".into(),
        generation,
    }
}

fn payload_json(title: &str) -> String {
    serde_json::json!({
        "schema": CODING_SESSION_TITLE_SCHEMA,
        "title": title,
        "model": "claude-haiku-4-5",
        "basis": "first-message",
        "sourceCommand": null,
        "createEventId": "ca".repeat(32),
    })
    .to_string()
}

fn signed_title(keys: &Keys, title: &str, created_at: u64) -> nostr::Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_GENERATED_TITLE as u16),
        payload_json(title),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).unwrap(),
        Tag::parse(["d", SESSION_REF]).unwrap(),
        Tag::parse(["cstl-v", CODING_SESSION_TITLE_TAG_VERSION]).unwrap(),
        Tag::parse(["cs-target", &coding_session_target_key(&target(1))]).unwrap(),
    ])
    .custom_created_at(Timestamp::from(created_at))
    .sign_with_keys(keys)
    .unwrap()
}

fn signed_name(keys: &Keys, content: &str, created_at: u64) -> nostr::Event {
    EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_NAME as u16), content)
        .tags([
            Tag::parse(["h", CHANNEL]).unwrap(),
            Tag::parse(["d", SESSION_REF]).unwrap(),
            Tag::parse(["csnm-v", "csnm1-1"]).unwrap(),
        ])
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .unwrap()
}

fn scope_for(founder: &Keys, provider: &Keys) -> SessionDisplayNameScope {
    SessionDisplayNameScope {
        channel_id: CHANNEL.into(),
        session_ref: SESSION_REF.into(),
        founder_pubkey: Some(founder.public_key().to_hex()),
        founding_execution_title: Some("Founding title".into()),
        executions: vec![SessionExecutionAuthority {
            target_key: coding_session_target_key(&target(1)),
            provider_authority_pubkey: provider.public_key().to_hex(),
        }],
    }
}

#[test]
fn signed_title_passes_the_envelope_and_decodes() {
    let event = signed_title(&Keys::generate(), "Login redirect fix", 10);
    let envelope = validate_coding_session_title_envelope(&event).unwrap();
    assert_eq!(envelope.channel_id, CHANNEL);
    assert_eq!(envelope.session_ref, SESSION_REF);
    assert_eq!(envelope.target_key, coding_session_target_key(&target(1)));
    assert_eq!(envelope.payload.title, "Login redirect fix");
    assert_eq!(
        envelope.payload.basis,
        CodingSessionTitleBasis::FirstMessage
    );
    assert_eq!(envelope.payload.source_command, None);
}

#[test]
fn source_command_key_is_required_even_when_null() {
    let mut value: Value = serde_json::from_str(&payload_json("Title")).unwrap();
    value.as_object_mut().unwrap().remove("sourceCommand");
    assert!(parse_coding_session_title_content(&value.to_string()).is_err());
    value["sourceCommand"] = Value::String("cd".repeat(32));
    assert_eq!(
        parse_coding_session_title_content(&value.to_string())
            .unwrap()
            .source_command,
        Some("cd".repeat(32))
    );
}

#[test]
fn target_key_round_trips_and_rejects_every_other_shape() {
    let key = coding_session_target_key(&target(7));
    assert_eq!(parse_coding_session_target_key(&key).unwrap(), target(7));
    for bad in [
        "",
        "coding-session/v1|",
        "coding-session/v2|16:claude-agent-acp16:1958c6c448e05eed6:sess-a1:1",
        "coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed6:sess-a1:0",
        "coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed6:sess-a",
        "coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed6:sess-a1:1extra",
        "coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed6:sess-a2:01",
        "coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed6:sess-a1:11:x",
        "coding-session/v1|+1:x1:y1:z1:1",
        "coding-session/v1|1: 1:y1:z1:1",
        "coding-session/v1|99:short",
    ] {
        assert!(
            parse_coding_session_target_key(bad).is_err(),
            "accepted {bad:?}"
        );
    }
}

#[test]
fn older_person_name_beats_newer_title() {
    let founder = Keys::generate();
    let provider = Keys::generate();
    let records: Vec<SessionNameRecord> = [
        signed_name(&founder, "Person name", 100),
        signed_title(&provider, "Generated title", 200),
    ]
    .iter()
    .map(SessionNameRecord::from)
    .collect();
    let resolved = resolve_session_display_name(&scope_for(&founder, &provider), &records);
    assert_eq!(resolved.name, "Person name");
    assert_eq!(resolved.origin, SessionDisplayNameOrigin::Person);
    assert_eq!(resolved.model, None);
    assert_eq!(resolved.signer_pubkey, None);
}

#[test]
fn earliest_title_wins_and_carries_model_and_signer() {
    let founder = Keys::generate();
    let provider = Keys::generate();
    let records: Vec<SessionNameRecord> = [
        signed_title(&provider, "Later", 300),
        signed_title(&provider, "Earliest", 200),
    ]
    .iter()
    .map(SessionNameRecord::from)
    .collect();
    let resolved = resolve_session_display_name(&scope_for(&founder, &provider), &records);
    assert_eq!(resolved.name, "Earliest");
    assert_eq!(resolved.origin, SessionDisplayNameOrigin::Generated);
    assert_eq!(resolved.model.as_deref(), Some("claude-haiku-4-5"));
    assert_eq!(resolved.signer_pubkey, Some(provider.public_key().to_hex()));
}

#[test]
fn foreign_signer_is_ignored_and_counted() {
    let founder = Keys::generate();
    let provider = Keys::generate();
    let stranger = Keys::generate();
    let records: Vec<SessionNameRecord> = [
        signed_title(&stranger, "Forged", 100),
        signed_name(&stranger, "Forged name", 100),
    ]
    .iter()
    .map(SessionNameRecord::from)
    .collect();
    let resolved = resolve_session_display_name(&scope_for(&founder, &provider), &records);
    assert_eq!(resolved.name, "Founding title");
    assert_eq!(resolved.origin, SessionDisplayNameOrigin::Fallback);
    assert_eq!(resolved.diagnostics.foreign_titles, 1);
    assert_eq!(resolved.diagnostics.foreign_names, 1);
}

#[test]
fn clean_generated_name_strips_decoration_and_refuses_placeholders() {
    assert_eq!(
        clean_generated_name("  Title: \"Fix login redirect.\"\nbecause").as_deref(),
        Some("Fix login redirect")
    );
    assert_eq!(
        clean_generated_name("**Auth rework**").as_deref(),
        Some("Auth rework")
    );
    assert_eq!(clean_generated_name(" \n \"\" "), None);
    assert_eq!(clean_generated_name("Untitled session."), None);
    assert_eq!(clean_generated_name("new thread"), None);
    let long = clean_generated_name(&"é".repeat(200)).unwrap();
    assert_eq!(long.chars().count(), MAX_GENERATED_NAME_CHARS);
    assert!(validate_coding_session_name_content(&long).is_ok());
}

#[test]
fn naming_prompt_is_the_desktop_text() {
    assert!(NAMING_SYSTEM_PROMPT.starts_with("You name coding sessions."));
    assert!(NAMING_SYSTEM_PROMPT.contains("one to four words"));
}

// ---- Conformance vectors ----------------------------------------------------

fn vectors() -> Value {
    let file: Value = serde_json::from_str(VECTORS).unwrap();
    assert_eq!(file["schema"], "buzz.conformance/session-display-name@1");
    file
}

fn record_parts(record: &SessionNameRecord) -> Vec<&[String]> {
    record.tags.iter().map(Vec::as_slice).collect()
}

#[test]
fn conformance_constants_match_this_build() {
    let constants = &vectors()["constants"];
    assert_eq!(constants["kindName"], KIND_CODING_SESSION_NAME);
    assert_eq!(
        constants["kindGeneratedTitle"],
        KIND_CODING_SESSION_GENERATED_TITLE
    );
    assert_eq!(constants["tagVersion"], CODING_SESSION_TITLE_TAG_VERSION);
    assert_eq!(constants["payloadSchema"], CODING_SESSION_TITLE_SCHEMA);
    assert_eq!(
        constants["maxTitleBytes"],
        crate::coding_session_name::MAX_CODING_SESSION_NAME_CONTENT_BYTES
    );
    assert_eq!(
        constants["maxContentBytes"],
        MAX_CODING_SESSION_TITLE_CONTENT_BYTES
    );
    assert_eq!(
        constants["maxModelBytes"],
        MAX_CODING_SESSION_TITLE_MODEL_BYTES
    );
    assert_eq!(constants["untitled"], UNTITLED_SESSION_NAME);
}

#[test]
fn conformance_envelopes() {
    let file = vectors();
    let envelopes = file["envelopes"].as_array().unwrap();
    assert!(envelopes.iter().any(|case| case["valid"] == true));
    assert!(envelopes.iter().any(|case| case["valid"] == false));
    for case in envelopes {
        let name = case["name"].as_str().unwrap();
        let record: SessionNameRecord = serde_json::from_value(case["event"].clone()).unwrap();
        assert_eq!(record.kind, KIND_CODING_SESSION_GENERATED_TITLE, "{name}");
        let verdict = validate_coding_session_title_parts(&record_parts(&record), &record.content);
        assert_eq!(
            verdict.is_ok(),
            case["valid"].as_bool().unwrap(),
            "envelope vector {name}: {verdict:?}"
        );
    }
}

#[test]
fn conformance_resolver_vectors() {
    let file = vectors();
    let vectors = file["vectors"].as_array().unwrap();
    assert!(vectors.len() >= 10);
    for case in vectors {
        let name = case["name"].as_str().unwrap();
        let scope: SessionDisplayNameScope = serde_json::from_value(case["scope"].clone())
            .unwrap_or_else(|error| panic!("{name}: scope: {error}"));
        let records: Vec<SessionNameRecord> = serde_json::from_value(case["events"].clone())
            .unwrap_or_else(|error| panic!("{name}: events: {error}"));
        let expected: SessionDisplayName = serde_json::from_value(case["expected"].clone())
            .unwrap_or_else(|error| panic!("{name}: expected: {error}"));
        assert_eq!(
            resolve_session_display_name(&scope, &records),
            expected,
            "resolver vector {name}"
        );
        // Order-independence: every reader may receive events in any order.
        let reversed: Vec<SessionNameRecord> = records.iter().rev().cloned().collect();
        assert_eq!(
            resolve_session_display_name(&scope, &reversed),
            expected,
            "resolver vector {name}, reversed"
        );
    }
}

#[test]
fn conformance_vectors_cover_the_brief() {
    let file = vectors();
    let names: Vec<&str> = file["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["name"].as_str().unwrap())
        .collect();
    for required in [
        "older-person-name-beats-newer-title",
        "earliest-title-wins",
        "foreign-signer-ignored",
        "signer-without-execution-ignored",
        "fallback-founding-execution-title",
        "fallback-untitled",
        "malformed-events-rejected",
    ] {
        assert!(names.contains(&required), "missing vector {required}");
    }
}
