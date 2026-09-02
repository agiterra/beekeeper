//! Tests for the native NIP-CSP policy boundary.
//!
//! Two things are being proved here. First, that this adapter adds no rules of
//! its own: every refusal a caller can provoke comes back in `buzz-core`'s own
//! words, naming the offending key. Second, that the TypeScript decoder is
//! pinned to **this adapter's real output** rather than to a hand-written
//! sample — the failure mode REVIEW-B1c B1 found, where a decoder stayed green
//! against a fixture nobody generated while it would have thrown for every
//! real record.

use nostr::{Keys, Timestamp};
use serde_json::json;

use super::*;

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "12".repeat(32)
}

/// Deterministic keys, so the generated fixture is byte-stable across runs.
fn fixed_keys(byte: u8) -> Keys {
    Keys::parse(&format!("{byte:02x}").repeat(32)).expect("fixed secret key")
}

/// A policy that sets something in every optional group.
fn complete_policy() -> serde_json::Value {
    json!({
        "schema": "buzz-coding-session-policy/v1",
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "posture": "overnight",
        "budget": {
            "turns": 240,
            "tokensPerSeat": 4_000_000u64,
            "tokensPerSession": 40_000_000u64,
            "costUsdPerSession": 120.5,
            "contextTier": "long"
        },
        "attention": "decisions",
        "gates": {
            "redFirst": true,
            "reviewEveryLane": true,
            "requiredGates": ["just ci", "just test"],
            "verifierRequired": false
        },
        "bench": {
            "identities": ["ab".repeat(32), "cd".repeat(32)],
            "providers": ["claude-primary", "codex-primary"],
            "challengerSampleRate": 0.25
        },
        "irreversible": ["push", "deploy", "external-message"],
        "stop": {
            "timeBoxSecs": 28_800u64,
            "onMilestone": "the lane lands and CI is green"
        }
    })
}

fn build_request(policy: serde_json::Value) -> CodingSessionPolicyBuildRequest {
    CodingSessionPolicyBuildRequest {
        schema: CODING_SESSION_POLICY_BUILD_REQUEST_SCHEMA.to_owned(),
        channel_ref: CHANNEL.to_owned(),
        policy,
    }
}

#[test]
fn a_complete_policy_builds_the_exact_envelope_and_flattens_every_key() {
    let response = build_adapter(build_request(complete_policy())).expect("build");

    assert_eq!(response.schema, CODING_SESSION_POLICY_ADAPTER_SCHEMA);
    assert_eq!(response.implementation, "buzz-core");
    // The kind comes from `buzz-core`'s allocation, never from a caller.
    assert_eq!(response.kind, 44245);
    assert_eq!(response.tags.len(), 4);
    assert_eq!(response.tags[0], vec!["h".to_owned(), CHANNEL.to_owned()]);
    assert_eq!(response.tags[1], vec!["d".to_owned(), SESSION.to_owned()]);
    assert_eq!(
        response.tags[2],
        vec![
            "csp-v".to_owned(),
            "buzz-coding-session-policy/v1".to_owned()
        ]
    );
    assert_eq!(response.tags[3], vec!["csp-genesis".to_owned(), genesis()]);
    // Rust's serialization, not the caller's: an unset key is absent from the
    // signed bytes even though the flattened record spells it as null.
    assert!(!response.content.contains("null"));

    let record = &response.record;
    assert_eq!(record.session_ref, SESSION);
    assert_eq!(record.posture.as_deref(), Some("overnight"));
    assert_eq!(record.attention.as_deref(), Some("decisions"));
    assert!(record.sets_any_policy);
    let budget = record.budget.as_ref().expect("budget");
    assert_eq!(budget.turns, Some(240));
    assert_eq!(budget.context_tier.as_deref(), Some("long"));
    let gates = record.gates.as_ref().expect("gates");
    assert_eq!(gates.red_first, Some(true));
    assert_eq!(
        gates.required_gates.as_deref(),
        Some(["just ci".to_owned(), "just test".to_owned()].as_slice())
    );
    let bench = record.bench.as_ref().expect("bench");
    assert_eq!(bench.challenger_sample_rate, Some(0.25));
    assert_eq!(
        record.irreversible.as_deref(),
        Some(
            [
                "push".to_owned(),
                "deploy".to_owned(),
                "external-message".to_owned()
            ]
            .as_slice()
        )
    );
    assert_eq!(
        record.stop.as_ref().expect("stop").on_milestone.as_deref(),
        Some("the lane lands and CI is green")
    );
}

#[test]
fn an_unset_key_is_present_and_null_never_absent() {
    // "Unknown ≠ empty" on the read side: a record that names no budget must
    // be distinguishable from an adapter that does not disclose budgets, and
    // the only way to do that is to always write the key.
    let response = build_adapter(build_request(json!({
        "schema": "buzz-coding-session-policy/v1",
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "posture": "ship"
    })))
    .expect("build");
    let wire = serde_json::to_value(&response.record).expect("serialize record");
    for key in [
        "budget",
        "attention",
        "gates",
        "bench",
        "irreversible",
        "stop",
    ] {
        assert!(
            wire.get(key).is_some(),
            "the record must carry {key} as null, never omit it"
        );
        assert!(wire[key].is_null(), "{key} must be null when unset");
    }
    assert!(response.record.sets_any_policy);
}

#[test]
fn the_withdrawal_record_builds_and_says_it_sets_nothing() {
    // Under newest-wins this is the only way to take a policy back, so it is a
    // legal record. A surface must be able to render it as "no policy" rather
    // than as "policy unknown", which is why the flag exists at all.
    let response = build_adapter(build_request(json!({
        "schema": "buzz-coding-session-policy/v1",
        "sessionRef": SESSION,
        "genesisRef": genesis()
    })))
    .expect("build");
    assert!(!response.record.sets_any_policy);
    assert_eq!(response.record.posture, None);
}

#[test]
fn every_refusal_is_the_core_decoders_own_sentence_naming_the_key() {
    let cases: [(serde_json::Value, &str); 4] = [
        (
            json!({"schema": "buzz-coding-session-policy/v1", "sessionRef": SESSION,
                   "genesisRef": genesis(), "noPushWithoutReview": true}),
            "noPushWithoutReview",
        ),
        (
            json!({"schema": "buzz-coding-session-policy/v1", "sessionRef": SESSION,
                   "genesisRef": genesis(), "posture": null}),
            "posture",
        ),
        (
            json!({"schema": "buzz-coding-session-policy/v1", "sessionRef": SESSION,
                   "genesisRef": genesis(), "budget": {"turns": 0}}),
            "turns",
        ),
        (
            json!({"schema": "buzz-coding-session-policy/v1", "sessionRef": SESSION,
                   "genesisRef": genesis(), "bench": {"identities": []}}),
            "identities",
        ),
    ];
    for (policy, named) in cases {
        let error = build_adapter(build_request(policy)).expect_err("must refuse");
        assert!(
            error.contains(named),
            "the refusal must name {named}: {error}"
        );
    }
}

/// A closed-vocabulary miss is refused, and — today — names nothing.
///
/// Recorded rather than hidden. `decode_coding_session_policy`
/// (`crates/buzz-core/src/coding_session_policy.rs:589-590`) discards serde's
/// own message, which does name the field and list the accepted variants, and
/// substitutes `malformed coding-session policy payload`. Every *other*
/// refusal on this path names its key. The launch form is the surface that
/// meets this one, so it never renders a bare adapter error for a word it
/// offered — but a `bee sessions policy set --posture sprint` would, and that
/// is the fix this test exists to hold a place for. Not changed here:
/// `coding_session_policy.rs` is outside lane B3's row.
#[test]
fn an_undefined_vocabulary_word_is_refused_but_the_sentence_names_nothing() {
    let error = build_adapter(build_request(json!({
        "schema": "buzz-coding-session-policy/v1",
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "posture": "sprint"
    })))
    .expect_err("must refuse");
    assert_eq!(error, "malformed coding-session policy payload");
    assert!(!error.contains("posture"));
}

#[test]
fn a_non_uuid_channel_never_reaches_a_signer() {
    let mut request = build_request(complete_policy());
    request.channel_ref = "not-a-uuid".into();
    let error = build_adapter(request).expect_err("must refuse");
    assert!(error.to_lowercase().contains("uuid"), "{error}");
}

#[test]
fn both_boundaries_refuse_a_schema_they_do_not_know() {
    let mut request = build_request(complete_policy());
    request.schema = "buzz-coding-session-policy-build-request/v2".into();
    assert!(build_adapter(request)
        .expect_err("must refuse")
        .contains(CODING_SESSION_POLICY_BUILD_REQUEST_SCHEMA));

    let error = read_adapter(CodingSessionPolicyReadRequest {
        schema: "buzz-coding-session-policy-read-request/v2".into(),
        event: json!({}),
    })
    .expect_err("must refuse");
    assert!(error.contains(CODING_SESSION_POLICY_READ_REQUEST_SCHEMA));
}

/// Sign a real kind-44245 event at a fixed timestamp, so the fixture is stable.
fn signed_policy(keys: &Keys, policy: serde_json::Value) -> nostr::Event {
    let payload =
        decode_coding_session_policy(&serde_json::to_string(&policy).expect("serialize policy"))
            .expect("decode policy");
    build_coding_session_policy(CHANNEL, payload)
        .expect("builder")
        .custom_created_at(Timestamp::from_secs(1_800_000_000))
        .sign_with_keys(keys)
        .expect("sign")
}

#[test]
fn a_forged_signature_is_refused_before_anything_is_read() {
    // A record read off an unverified event is a claim about who set a policy,
    // made by nobody.
    let founder = fixed_keys(0x11);
    let event = signed_policy(&founder, complete_policy());
    let mut wire = serde_json::to_value(&event).expect("serialize event");
    wire["sig"] = json!("00".repeat(64));
    let error = read_adapter(CodingSessionPolicyReadRequest {
        schema: CODING_SESSION_POLICY_READ_REQUEST_SCHEMA.to_owned(),
        event: wire,
    })
    .expect_err("must refuse");
    assert!(error.to_lowercase().contains("signature"), "{error}");
}

#[test]
fn a_read_returns_the_signer_and_the_same_flattened_record() {
    let founder = fixed_keys(0x11);
    let event = signed_policy(&founder, complete_policy());
    let response = read_adapter(CodingSessionPolicyReadRequest {
        schema: CODING_SESSION_POLICY_READ_REQUEST_SCHEMA.to_owned(),
        event: serde_json::to_value(&event).expect("serialize event"),
    })
    .expect("read");
    assert_eq!(response.author_pubkey, founder.public_key().to_hex());
    assert_eq!(response.event_id, event.id.to_hex());
    assert_eq!(response.created_at, 1_800_000_000);
    // Byte-for-byte the same record the build boundary produced: one flattener,
    // so a draft and the signed thing it became can never read differently.
    let built = build_adapter(build_request(complete_policy())).expect("build");
    assert_eq!(response.record, built.record);
}

/// Path of the fixture the Desktop decoder test reads, relative to this crate.
const TS_DECODER_FIXTURE: &str =
    "../src/features/coding-sessions/lib/codingSessionPolicyAdapterResponse.fixture.json";

#[test]
fn the_typescript_decoder_fixture_is_this_adapter_s_real_output() {
    // Regenerate with
    // `BUZZ_UPDATE_FIXTURES=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml coding_session_policy`.
    let founder = fixed_keys(0x11);
    let event = signed_policy(&founder, complete_policy());
    let generated = serde_json::to_string_pretty(&json!({
        "note": "Generated by `the_typescript_decoder_fixture_is_this_adapter_s_real_output` in \
desktop/src-tauri/src/commands/coding_session_policy_tests.rs. Do not hand-edit: a hand-written \
fixture is what let a decoder stay green while it would have thrown for every real record.",
        "build": build_adapter(build_request(complete_policy())).expect("build"),
        "withdrawal": build_adapter(build_request(json!({
            "schema": "buzz-coding-session-policy/v1",
            "sessionRef": SESSION,
            "genesisRef": genesis(),
        })))
        .expect("build withdrawal"),
        "read": read_adapter(CodingSessionPolicyReadRequest {
            schema: CODING_SESSION_POLICY_READ_REQUEST_SCHEMA.to_owned(),
            event: serde_json::to_value(&event).expect("serialize event"),
        })
        .expect("read"),
    }))
    .expect("serialize fixture")
        + "\n";

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TS_DECODER_FIXTURE);
    if std::env::var("BUZZ_UPDATE_FIXTURES").is_ok() {
        std::fs::write(&path, &generated).expect("write fixture");
    }
    let stored = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    // Compared as JSON rather than as bytes, and the reason is worth stating:
    // `biome` formats every `.json` under `desktop/src`, and it disagrees with
    // `serde_json::to_string_pretty` about when an array fits on one line.
    // Both are right about their own rules and neither owns this file, so a
    // byte comparison would make `pnpm check --write` and `cargo test` undo
    // each other forever. Value equality still fails the moment a field,
    // a value or a key set changes — which is the whole point of the pin.
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored).expect("stored fixture is JSON");
    let generated_value: serde_json::Value =
        serde_json::from_str(&generated).expect("generated fixture is JSON");
    assert_eq!(
        stored_value, generated_value,
        "the Desktop policy fixture is stale; regenerate it with BUZZ_UPDATE_FIXTURES=1"
    );

    // The fixture must exercise the shapes the decoder is pinned to, or it
    // proves nothing about the boundary it exists to hold.
    let wire: serde_json::Value = serde_json::from_str(&generated).expect("fixture JSON");
    assert_eq!(wire["build"]["kind"], json!(44245));
    assert_eq!(wire["build"]["record"]["setsAnyPolicy"], json!(true));
    assert_eq!(wire["withdrawal"]["record"]["setsAnyPolicy"], json!(false));
    assert!(wire["withdrawal"]["record"]["budget"].is_null());
    assert!(wire["read"]["authorPubkey"].is_string());
}
