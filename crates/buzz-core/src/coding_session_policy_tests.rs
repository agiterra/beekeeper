use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::{json, Value};

use super::*;

const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";

fn genesis() -> String {
    "12".repeat(32)
}

fn content(extra: &[(&str, Value)]) -> String {
    let mut payload = json!({
        "schema": CODING_SESSION_POLICY_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": genesis(),
    });
    let object = payload.as_object_mut().expect("policy object");
    for (key, value) in extra {
        object.insert((*key).to_owned(), value.clone());
    }
    payload.to_string()
}

/// Every field at once, so a decoder that drops one is caught.
fn full_policy() -> Vec<(&'static str, Value)> {
    vec![
        ("posture", json!("overnight")),
        (
            "budget",
            json!({
                "turns": 240,
                "tokensPerSeat": 4_000_000_u64,
                "tokensPerSession": 40_000_000_u64,
                "costUsdPerSession": 120.5,
                "contextTier": "long",
            }),
        ),
        ("attention", json!("decisions")),
        (
            "gates",
            json!({
                "redFirst": true,
                "reviewEveryLane": true,
                "requiredGates": ["just ci", "just test"],
                "verifierRequired": false,
            }),
        ),
        (
            "bench",
            json!({
                "identities": ["ab".repeat(32), "cd".repeat(32)],
                "providers": ["claude-primary", "codex-primary"],
                "challengerSampleRate": 0.25,
            }),
        ),
        (
            "irreversible",
            json!(["push", "deploy", "external-message"]),
        ),
        (
            "stop",
            json!({"timeBoxSecs": 28_800_u64, "onMilestone": "the lane lands and CI is green"}),
        ),
    ]
}

fn signed(content: &str, tags: Vec<[&str; 2]>) -> nostr::Event {
    EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_POLICY as u16), content)
        .tags(
            tags.into_iter()
                .map(|parts| Tag::parse(parts).expect("tag parses")),
        )
        .sign_with_keys(&Keys::generate())
        .expect("sign policy")
}

fn canonical_tags() -> Vec<[&'static str; 2]> {
    vec![
        ["h", CHANNEL],
        ["d", SESSION],
        ["csp-v", CODING_SESSION_POLICY_SCHEMA],
        [
            "csp-genesis",
            "1212121212121212121212121212121212121212121212121212121212121212",
        ],
    ]
}

/// 44245 is the lowest unused, unreserved kind in this fork and in vanilla:
/// 44231-44239 and 44240-44243 are documented reservations and 44244 is the
/// team transaction.
#[test]
fn the_policy_kind_is_the_next_free_number_and_is_not_replaceable() {
    assert_eq!(KIND_CODING_SESSION_POLICY, 44245);
    assert_eq!(
        KIND_CODING_SESSION_POLICY,
        crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION + 1
    );
    assert!(!crate::kind::is_replaceable(KIND_CODING_SESSION_POLICY));
    assert!(!crate::kind::is_parameterized_replaceable(
        KIND_CODING_SESSION_POLICY
    ));
    assert!(!crate::kind::is_ephemeral(KIND_CODING_SESSION_POLICY));
    assert!(crate::kind::ALL_KINDS.contains(&KIND_CODING_SESSION_POLICY));
}

#[test]
fn a_complete_policy_decodes_and_round_trips_key_for_key() {
    let raw = content(&full_policy());
    let decoded = decode_coding_session_policy(&raw).expect("a complete policy decodes");
    assert!(decoded.sets_any_policy());
    assert_eq!(decoded.posture, Some(CodingSessionPosture::Overnight));
    assert_eq!(decoded.attention, Some(CodingSessionAttention::Decisions));
    assert_eq!(
        decoded.irreversible.as_deref(),
        Some(
            [
                CodingSessionIrreversibleAct::Push,
                CodingSessionIrreversibleAct::Deploy,
                CodingSessionIrreversibleAct::ExternalMessage,
            ]
            .as_slice()
        )
    );
    let reserialized: Value =
        serde_json::from_str(&serde_json::to_string(&decoded).expect("reserialize"))
            .expect("reparse");
    assert_eq!(
        reserialized,
        serde_json::from_str::<Value>(&raw).expect("original")
    );
}

/// A record that sets nothing is the explicit withdrawal of a policy under a
/// newest-wins fold. Refusing it would leave a policy nobody can take back.
#[test]
fn a_policy_that_sets_nothing_is_the_withdrawal_and_is_accepted() {
    let decoded = decode_coding_session_policy(&content(&[])).expect("an empty policy decodes");
    assert!(!decoded.sets_any_policy());
    assert_eq!(
        decoded,
        CodingSessionPolicyPayload::empty(SESSION, genesis())
    );
    let reserialized = serde_json::to_string(&decoded).expect("reserialize");
    assert_eq!(reserialized.matches("null").count(), 0);
    for key in POLICY_OPTIONAL_KEYS {
        assert!(
            !reserialized.contains(key),
            "an unset key must not be written: {key}"
        );
    }
}

/// **REVIEW-B1 F4.** An empty array is an empty sub-object one level down.
/// `irreversible: []` was already refused by name; the three collections that
/// were not could pass the "must carry at least one field" guard, so a record
/// that set nothing answered `sets_any_policy() == true` and could impersonate
/// a withdrawal. Every collection now gets the same answer.
#[test]
fn an_empty_collection_cannot_impersonate_a_policy() {
    for (key, body, field, omission) in [
        (
            "gates",
            json!({"requiredGates": []}),
            "gates.requiredGates",
            "require no gate",
        ),
        (
            "bench",
            json!({"identities": []}),
            "bench.identities",
            "bench no identity",
        ),
        (
            "bench",
            json!({"providers": []}),
            "bench.providers",
            "bench no provider",
        ),
    ] {
        let error = decode_coding_session_policy(&content(&[(key, body)]))
            .expect_err("an empty collection must be refused");
        assert!(error.contains(field), "must name the field: {error}");
        assert!(
            error.contains(&format!("omit the key to {omission}")),
            "must carry the way out: {error}"
        );
    }

    // The record the reviewer built: every collection empty. It must not
    // decode, and the in-memory payload must not claim to set a policy.
    let reviewer_record = content(&[
        ("gates", json!({"requiredGates": []})),
        ("bench", json!({"identities": [], "providers": []})),
    ]);
    assert!(decode_coding_session_policy(&reviewer_record).is_err());

    let hollow = CodingSessionPolicyPayload {
        gates: Some(CodingSessionPolicyGates {
            red_first: None,
            review_every_lane: None,
            required_gates: Some(Vec::new()),
            verifier_required: None,
        }),
        bench: Some(CodingSessionPolicyBench {
            identities: Some(Vec::new()),
            providers: Some(Vec::new()),
            challenger_sample_rate: None,
        }),
        irreversible: Some(Vec::new()),
        ..CodingSessionPolicyPayload::empty(SESSION, genesis())
    };
    assert!(
        !hollow.sets_any_policy(),
        "a record whose every collection is empty sets no policy"
    );
    assert!(
        hollow.validate().is_err(),
        "and it does not validate either"
    );
}

/// Absent is not null, at the top level and inside every nested object.
#[test]
fn an_explicit_null_is_refused_by_name_everywhere() {
    for key in POLICY_OPTIONAL_KEYS {
        let error = decode_coding_session_policy(&content(&[(key, Value::Null)]))
            .expect_err("an explicit null is refused");
        assert!(error.contains(key), "{key}: {error}");
    }
    let error = decode_coding_session_policy(&content(&[(
        "budget",
        json!({"turns": 10, "contextTier": Value::Null}),
    )]))
    .expect_err("a nested explicit null is refused");
    assert!(error.contains("contextTier"), "{error}");
}

/// v1 rejects unknown fields rather than ignoring them, at every level.
#[test]
fn unknown_fields_are_rejected_at_every_level() {
    let top = decode_coding_session_policy(&content(&[("cadence", json!("hourly"))]))
        .expect_err("an unknown top-level key is refused");
    assert!(top.contains("cadence"), "{top}");

    let nested = decode_coding_session_policy(&content(&[(
        "gates",
        json!({"redFirst": true, "mustBeNice": true}),
    )]))
    .expect_err("an unknown nested key is refused");
    assert!(nested.contains("mustBeNice"), "{nested}");

    let inside_bench = decode_coding_session_policy(&content(&[(
        "bench",
        json!({"identities": ["ab".repeat(32)], "models": ["sonnet"]}),
    )]))
    .expect_err("an unknown bench key is refused");
    assert!(inside_bench.contains("models"), "{inside_bench}");
}

/// Every closed vocabulary refuses a word nobody defined.
#[test]
fn the_closed_vocabularies_refuse_an_undefined_word() {
    assert!(decode_coding_session_policy(&content(&[("posture", json!("vibes"))])).is_err());
    assert!(decode_coding_session_policy(&content(&[("attention", json!("some"))])).is_err());
    assert!(
        decode_coding_session_policy(&content(&[("irreversible", json!(["rm-rf"])),])).is_err()
    );
    assert!(decode_coding_session_policy(&content(&[(
        "budget",
        json!({"contextTier": "enormous"}),
    )]))
    .is_err());
    for posture in ["spike", "ship", "investigate", "overnight"] {
        decode_coding_session_policy(&content(&[("posture", json!(posture))]))
            .unwrap_or_else(|error| panic!("{posture}: {error}"));
    }
    for attention in ["decisions", "decisions-and-milestones", "everything"] {
        decode_coding_session_policy(&content(&[("attention", json!(attention))]))
            .unwrap_or_else(|error| panic!("{attention}: {error}"));
    }
    for act in ["push", "deploy", "delete", "external-message"] {
        decode_coding_session_policy(&content(&[("irreversible", json!([act]))]))
            .unwrap_or_else(|error| panic!("{act}: {error}"));
    }
}

/// A limit of zero and "no limit" would otherwise be the same record read two
/// ways, so zero is refused rather than stored.
#[test]
fn a_zero_limit_is_refused_rather_than_read_as_no_limit() {
    for (key, body) in [
        ("budget", json!({"turns": 0})),
        ("budget", json!({"tokensPerSeat": 0})),
        ("budget", json!({"tokensPerSession": 0})),
        ("budget", json!({"costUsdPerSession": 0.0})),
        ("stop", json!({"timeBoxSecs": 0})),
    ] {
        assert!(
            decode_coding_session_policy(&content(&[(key, body.clone())])).is_err(),
            "{key} {body} must be refused"
        );
    }
}

/// An empty sub-object claims nothing and is refused: omit the key instead.
#[test]
fn an_empty_sub_object_is_refused() {
    for key in ["budget", "gates", "bench", "stop"] {
        let error = decode_coding_session_policy(&content(&[(key, json!({}))]))
            .expect_err("an empty sub-object is refused");
        assert!(error.contains("at least one field"), "{key}: {error}");
    }
    let error = decode_coding_session_policy(&content(&[("irreversible", json!([]))]))
        .expect_err("an empty irreversible list is refused");
    assert!(error.contains("must not be empty"), "{error}");
}

/// A per-seat token ceiling above the whole session's cannot bind, so it is a
/// contradiction rather than a permissive setting.
#[test]
fn a_per_seat_ceiling_above_the_session_ceiling_is_refused() {
    let error = decode_coding_session_policy(&content(&[(
        "budget",
        json!({"tokensPerSeat": 10_u64, "tokensPerSession": 9_u64}),
    )]))
    .expect_err("a contradictory budget is refused");
    assert!(error.contains("tokensPerSeat"), "{error}");
}

#[test]
fn every_collection_is_bounded_and_free_of_duplicates() {
    let too_many_gates: Vec<String> = (0..=MAX_POLICY_REQUIRED_GATES)
        .map(|index| format!("gate-{index}"))
        .collect();
    assert!(decode_coding_session_policy(&content(&[(
        "gates",
        json!({"requiredGates": too_many_gates}),
    )]))
    .is_err());
    assert!(decode_coding_session_policy(&content(&[(
        "gates",
        json!({"requiredGates": ["just ci", "just ci"]}),
    )]))
    .is_err());
    assert!(decode_coding_session_policy(&content(&[(
        "gates",
        json!({"requiredGates": ["a".repeat(MAX_POLICY_REQUIRED_GATE_BYTES + 1)]}),
    )]))
    .is_err());

    let too_many_identities: Vec<String> = (0..=MAX_POLICY_BENCH_IDENTITIES)
        .map(|index| format!("{index:064x}"))
        .collect();
    assert!(decode_coding_session_policy(&content(&[(
        "bench",
        json!({"identities": too_many_identities}),
    )]))
    .is_err());
    assert!(decode_coding_session_policy(&content(&[(
        "bench",
        json!({"identities": ["AB".repeat(32)]}),
    )]))
    .is_err());
    let too_many_providers: Vec<String> = (0..=MAX_POLICY_BENCH_PROVIDERS)
        .map(|index| format!("provider-{index}"))
        .collect();
    assert!(decode_coding_session_policy(&content(&[(
        "bench",
        json!({"providers": too_many_providers}),
    )]))
    .is_err());
    assert!(decode_coding_session_policy(&content(&[(
        "bench",
        json!({"providers": ["claude-primary", "claude-primary"]}),
    )]))
    .is_err());

    for rate in [-0.1_f64, 1.1_f64] {
        assert!(decode_coding_session_policy(&content(&[(
            "bench",
            json!({"challengerSampleRate": rate}),
        )]))
        .is_err());
    }
    for rate in [0.0_f64, 0.5_f64, 1.0_f64] {
        decode_coding_session_policy(&content(&[(
            "bench",
            json!({"challengerSampleRate": rate}),
        )]))
        .unwrap_or_else(|error| panic!("{rate}: {error}"));
    }
}

#[test]
fn prose_is_bounded_and_carries_no_control_characters() {
    assert!(decode_coding_session_policy(&content(&[(
        "stop",
        json!({"onMilestone": "a".repeat(MAX_POLICY_MILESTONE_BYTES + 1)}),
    )]))
    .is_err());
    assert!(decode_coding_session_policy(&content(&[(
        "stop",
        json!({"onMilestone": "green\u{0}ci"}),
    )]))
    .is_err());
    assert!(
        decode_coding_session_policy(&content(&[("stop", json!({"onMilestone": "   "}),)]))
            .is_err()
    );
}

#[test]
fn the_content_cap_is_enforced_before_parsing() {
    let oversize = content(&[(
        "stop",
        json!({"onMilestone": "a".repeat(MAX_POLICY_MILESTONE_BYTES)}),
    )]);
    decode_coding_session_policy(&oversize).expect("a full-size milestone still fits");
    let padded = format!(
        "{{\"pad\":\"{}\",{}",
        "a".repeat(MAX_CODING_SESSION_POLICY_CONTENT_BYTES),
        &content(&[])[1..]
    );
    let error = decode_coding_session_policy(&padded).expect_err("an oversize payload is refused");
    assert!(error.contains("exceeds"), "{error}");
}

#[test]
fn the_session_and_genesis_references_are_canonical() {
    let uppercase_session = content(&[]).replace(SESSION, &SESSION.to_uppercase());
    assert!(decode_coding_session_policy(&uppercase_session).is_err());
    let bad_genesis = content(&[]).replace(&genesis(), "not-an-event-id");
    assert!(decode_coding_session_policy(&bad_genesis).is_err());
}

#[test]
fn the_envelope_is_exactly_four_ordered_two_field_tags() {
    let raw = content(&full_policy());
    let event = signed(&raw, canonical_tags());
    let payload = validate_coding_session_policy_envelope(&event).expect("a canonical envelope");
    assert_eq!(payload.session_ref, SESSION);

    // A fifth tag, a missing tag, a reordered pair, and a three-field tag are
    // all refused.
    let mut extra = canonical_tags();
    extra.push(["cstx-type", "assignment"]);
    assert!(validate_coding_session_policy_envelope(&signed(&raw, extra)).is_err());

    let mut short = canonical_tags();
    short.pop();
    assert!(validate_coding_session_policy_envelope(&signed(&raw, short)).is_err());

    let mut reordered = canonical_tags();
    reordered.swap(1, 2);
    assert!(validate_coding_session_policy_envelope(&signed(&raw, reordered)).is_err());

    let three_field =
        EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_POLICY as u16), raw.clone())
            .tags([
                Tag::parse(["h", CHANNEL, "extra"]).expect("tag"),
                Tag::parse(["d", SESSION]).expect("tag"),
                Tag::parse(["csp-v", CODING_SESSION_POLICY_SCHEMA]).expect("tag"),
                Tag::parse(["csp-genesis", genesis().as_str()]).expect("tag"),
            ])
            .sign_with_keys(&Keys::generate())
            .expect("sign");
    assert!(validate_coding_session_policy_envelope(&three_field).is_err());
}

/// A policy cannot be filed under one umbrella while claiming another.
#[test]
fn the_tags_must_agree_with_the_content() {
    let raw = content(&full_policy());
    let mut mismatched = canonical_tags();
    mismatched[1] = ["d", "aaaaaaaa-0000-4000-8000-000000000000"];
    let error = validate_coding_session_policy_envelope(&signed(&raw, mismatched))
        .expect_err("a mismatched d tag is refused");
    assert!(error.contains("sessionRef"), "{error}");

    let mut wrong_genesis = canonical_tags();
    wrong_genesis[3] = [
        "csp-genesis",
        "3434343434343434343434343434343434343434343434343434343434343434",
    ];
    assert!(validate_coding_session_policy_envelope(&signed(&raw, wrong_genesis)).is_err());

    let wrong_kind = EventBuilder::new(
        Kind::Custom(crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        raw,
    )
    .tags(
        canonical_tags()
            .into_iter()
            .map(|parts| Tag::parse(parts).expect("tag")),
    )
    .sign_with_keys(&Keys::generate())
    .expect("sign");
    assert!(validate_coding_session_policy_envelope(&wrong_kind).is_err());
}

/// Duplicate keys are serde's to catch; the `Value` map above cannot see them.
#[test]
fn a_duplicate_key_is_refused_by_the_second_pass() {
    let duplicated = format!(
        "{{\"schema\":\"{CODING_SESSION_POLICY_SCHEMA}\",\"sessionRef\":\"{SESSION}\",\
         \"sessionRef\":\"{SESSION}\",\"genesisRef\":\"{}\"}}",
        genesis()
    );
    assert!(decode_coding_session_policy(&duplicated).is_err());
}
