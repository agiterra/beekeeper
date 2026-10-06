//! The requester travels with the hire; the create names the hire it answers.
//!
//! Both keys are additive and optional under the item-102 rule: absent is not
//! null, and a strict reader rejects the null. The shared fixture these tests
//! read is the same file the TypeScript decoder is pinned to.

use serde_json::{json, Value};

use super::*;

const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "12".repeat(32)
}

fn requester() -> String {
    "9f".repeat(32)
}

fn hire_event_id() -> String {
    "7a".repeat(32)
}

/// The shared fixture, verbatim from disk. B3's TypeScript decoder reads the
/// same file, so the two sides agree by construction.
fn fixture() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/coding_session_hire_requester/vectors.json"
    )))
    .expect("hire-requester fixture decodes")
}

fn hire_action(extra: &[(&str, Value)]) -> Value {
    let mut action = json!({
        "type": "session.hire",
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "role": "builder",
        "providerInstanceRef": "claude-primary",
        "model": Value::Null,
        "brief": "Rebase the lane and run the gate.",
    });
    let object = action.as_object_mut().expect("hire action object");
    for (key, value) in extra {
        object.insert((*key).to_owned(), value.clone());
    }
    action
}

fn create_action(extra: &[(&str, Value)]) -> Value {
    let mut action = json!({
        "type": "session.create",
        "projectRef": Value::Null,
        "repoRef": Value::Null,
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "providerInstanceRef": "claude-primary",
        "providerAuthorityPubkey": "ab".repeat(32),
        "model": Value::Null,
        "title": Value::Null,
        "initialTurn": "[From the lead] Rebase the lane and run the gate.",
        "actor": "cd".repeat(32),
        "role": "builder",
    });
    let object = action.as_object_mut().expect("create action object");
    for (key, value) in extra {
        object.insert((*key).to_owned(), value.clone());
    }
    action
}

fn payload(command_id: &str, action: Value) -> String {
    json!({
        "schema": CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
        "commandId": command_id,
        "action": action,
    })
    .to_string()
}

fn routing_request() -> Value {
    json!({
        "class": "builder",
        "risk": {"impact": 3, "uncertainty": 3, "irreversibility": 2},
    })
}

/// The requesting seat rides on the hire, and the decoder exposes it next to
/// the signer so a consumer can compare the two itself.
#[test]
fn a_hire_carries_the_requesting_seat() {
    let content = payload(
        "hire-1",
        hire_action(&[("requestedBy", json!(requester()))]),
    );
    let decoded = decode_coding_session_lifecycle_command(&content).expect("a requested hire");
    let CodingSessionLifecycleAction::SessionHire { requested_by, .. } = &decoded.action else {
        panic!("expected a hire action")
    };
    assert_eq!(requested_by.as_deref(), Some(requester().as_str()));
    assert_eq!(decoded.hire_requested_by(), Some(requester().as_str()));
    assert_eq!(
        decoded.hire_requester_matches_signer(&requester()),
        Some(true)
    );
    assert_eq!(
        decoded.hire_requester_matches_signer(&"ee".repeat(32)),
        Some(false)
    );
}

/// A hire that claims no requester answers `None`, never `Some(false)`:
/// unknown is not the same as mismatched.
#[test]
fn an_unclaimed_requester_is_unknown_not_mismatched() {
    let decoded = decode_coding_session_lifecycle_command(&payload("hire-1", hire_action(&[])))
        .expect("an unrequested hire still decodes");
    assert_eq!(decoded.hire_requested_by(), None);
    assert_eq!(decoded.hire_requester_matches_signer(&requester()), None);
}

/// Every other action type answers `None` rather than pretending to know.
#[test]
fn only_a_hire_answers_the_requester_questions() {
    let decoded = decode_coding_session_lifecycle_command(&payload(
        "create-1",
        create_action(&[("hireRef", json!(hire_event_id()))]),
    ))
    .expect("a create decodes");
    assert_eq!(decoded.hire_requested_by(), None);
    assert_eq!(decoded.hire_requester_matches_signer(&requester()), None);
    assert_eq!(decoded.create_hire_ref(), Some(hire_event_id().as_str()));
}

/// The create names the hire it answers, and a create that names none still
/// decodes exactly as it did before the key existed.
#[test]
fn a_create_names_the_hire_it_answers() {
    let decoded = decode_coding_session_lifecycle_command(&payload(
        "create-1",
        create_action(&[("hireRef", json!(hire_event_id()))]),
    ))
    .expect("a create that answers a hire");
    let CodingSessionLifecycleAction::SessionCreate { hire_ref, .. } = &decoded.action else {
        panic!("expected a create action")
    };
    assert_eq!(hire_ref.as_deref(), Some(hire_event_id().as_str()));

    let unattributed =
        decode_coding_session_lifecycle_command(&payload("create-1", create_action(&[])))
            .expect("an unattributed create still decodes");
    assert_eq!(unattributed.create_hire_ref(), None);
}

/// Absent is not null. A producer with nothing to say omits the key, and a
/// reader that accepted the null would put a decision on the wire nobody made.
#[test]
fn an_explicit_null_is_refused_by_name_on_both_keys() {
    let hire_error = decode_coding_session_lifecycle_command(&payload(
        "hire-1",
        hire_action(&[("requestedBy", Value::Null)]),
    ))
    .expect_err("an explicit null requester is refused");
    assert!(
        hire_error.contains("requestedBy"),
        "the refusal must name the key: {hire_error}"
    );

    let create_error = decode_coding_session_lifecycle_command(&payload(
        "create-1",
        create_action(&[("hireRef", Value::Null)]),
    ))
    .expect_err("an explicit null hire reference is refused");
    assert!(
        create_error.contains("hireRef"),
        "the refusal must name the key: {create_error}"
    );
}

/// Both values are compared byte-for-byte against relay-signed facts, so an
/// uppercase or truncated copy is rejected rather than coerced.
#[test]
fn both_references_must_be_lowercase_64_hex() {
    for bad in [
        requester().to_uppercase(),
        requester()[..63].to_owned(),
        format!("{}0", requester()),
        "not-a-pubkey".to_owned(),
    ] {
        assert!(
            decode_coding_session_lifecycle_command(&payload(
                "hire-1",
                hire_action(&[("requestedBy", json!(bad))]),
            ))
            .is_err(),
            "requestedBy {bad:?} must be refused"
        );
    }
    for bad in [
        hire_event_id().to_uppercase(),
        hire_event_id()[..63].to_owned(),
        "not-an-event-id".to_owned(),
    ] {
        assert!(
            decode_coding_session_lifecycle_command(&payload(
                "create-1",
                create_action(&[("hireRef", json!(bad))]),
            ))
            .is_err(),
            "hireRef {bad:?} must be refused"
        );
    }
}

/// The two keys never cross, and the refusal names the key **and** says where
/// it belongs.
///
/// `is_err()` alone is what let the shipped claim "refused, by name" go
/// unchecked while the decoder actually answered "action has missing or
/// unsupported fields" (REVIEW-B1 F1). Every assertion here is on the text.
#[test]
fn the_two_keys_never_cross_and_the_refusal_names_the_key() {
    for (command, action, key, belongs_on, this_action, remedy_fragment) in [
        (
            "hire-1",
            hire_action(&[("hireRef", json!(hire_event_id()))]),
            "hireRef",
            "create",
            "hire",
            "a hire cannot answer itself",
        ),
        (
            "create-1",
            create_action(&[("requestedBy", json!(requester()))]),
            "requestedBy",
            "hire",
            "create",
            "a create names the hire it answers with hireRef",
        ),
    ] {
        let error = decode_coding_session_lifecycle_command(&payload(command, action))
            .expect_err("a foreign additive key is refused");
        assert!(error.contains(key), "must name the key: {error}");
        assert!(
            error.contains(&format!("is a {belongs_on} field")),
            "must say where it belongs: {error}"
        );
        assert!(
            error.contains(&format!("does not belong on a {this_action}")),
            "must say where it does not belong: {error}"
        );
        assert!(
            error.contains(remedy_fragment),
            "must carry the way out: {error}"
        );
        assert!(
            !error.contains("missing or unsupported fields"),
            "the unactionable shape sentence must not be what a lead reads: {error}"
        );
    }

    // A foreign key written as an explicit null is refused the same way, and
    // still by name — not swallowed by the shape check.
    for (command, action, key) in [
        (
            "hire-1",
            hire_action(&[("hireRef", Value::Null)]),
            "hireRef",
        ),
        (
            "create-1",
            create_action(&[("requestedBy", Value::Null)]),
            "requestedBy",
        ),
    ] {
        let error = decode_coding_session_lifecycle_command(&payload(command, action))
            .expect_err("a null foreign key is refused");
        assert!(error.contains(key), "must name the key: {error}");
        assert!(!error.contains("missing or unsupported fields"), "{error}");
    }
}

/// `requestedBy` and `routing` are independent axes, so a hire has exactly
/// four accepted shapes — and nothing between or beyond them.
#[test]
fn all_four_hire_shapes_decode_and_nothing_beyond_them() {
    let shapes: [Vec<(&str, Value)>; 4] = [
        vec![],
        vec![("routing", routing_request())],
        vec![("requestedBy", json!(requester()))],
        vec![
            ("requestedBy", json!(requester())),
            ("routing", routing_request()),
        ],
    ];
    for shape in &shapes {
        let content = payload("hire-1", hire_action(shape));
        decode_coding_session_lifecycle_command(&content)
            .unwrap_or_else(|error| panic!("shape {shape:?} must decode: {error}"));
    }
    assert!(decode_coding_session_lifecycle_command(&payload(
        "hire-1",
        hire_action(&[
            ("requestedBy", json!(requester())),
            ("routing", routing_request()),
            ("somethingElse", json!(true)),
        ]),
    ))
    .is_err());
}

/// `hireRef` is a fourth independent additive axis on the create, so the
/// accepted shape count doubles from twelve to twenty-four. Every one of the
/// twenty-four decodes; a subset that omits `role` beside `actor` does not.
#[test]
fn every_one_of_the_twenty_four_create_shapes_decodes() {
    let mut accepted = 0_usize;
    for base in 0..3_usize {
        for seated in [false, true] {
            for routed in [false, true] {
                for attributed in [false, true] {
                    let mut action = json!({
                        "type": "session.create",
                        "projectRef": Value::Null,
                        "repoRef": Value::Null,
                        "providerInstanceRef": "claude-primary",
                        "providerAuthorityPubkey": "ab".repeat(32),
                        "model": Value::Null,
                        "title": Value::Null,
                        "initialTurn": Value::Null,
                    });
                    let object = action.as_object_mut().expect("create action object");
                    if base >= 1 {
                        object.insert("sessionRef".to_owned(), json!(SESSION));
                    }
                    if base >= 2 {
                        object.insert("genesisRef".to_owned(), json!(genesis()));
                    }
                    if seated {
                        object.insert("actor".to_owned(), json!("cd".repeat(32)));
                        object.insert("role".to_owned(), json!("builder"));
                    }
                    if attributed {
                        object.insert("hireRef".to_owned(), json!(hire_event_id()));
                    }
                    if routed {
                        object.insert("routing".to_owned(), full_routing_record());
                    }
                    decode_coding_session_lifecycle_command(&payload("create-1", action))
                        .unwrap_or_else(|error| {
                            panic!("base {base} seated {seated} routed {routed} attributed {attributed}: {error}")
                        });
                    accepted += 1;
                }
            }
        }
    }
    assert_eq!(accepted, 24);
}

/// The complete routing record a create carries; a create is the answer, so
/// the record must be complete.
fn full_routing_record() -> Value {
    json!({
        "class": "builder",
        "tier": "standard",
        "risk": {"impact": 3, "uncertainty": 3, "irreversibility": 2, "score": 18},
        "profile": Value::Null,
        "chosen": {"provider": "claude-primary", "model": "sonnet", "effort": "medium"},
        "runnerUp": Value::Null,
        "reason": "claude-primary/sonnet cleared the builder gate",
        "reviewRequired": false,
        "reviewReasons": [],
        "challengerSample": false,
        "override": Value::Null,
        "registryVersion": 1,
        "catalogRevision": 7,
    })
}

/// Wire bytes of the shapes that already exist are unchanged: a hire with no
/// requester re-serializes without the key, and one with a requester emits it
/// in exactly one place.
#[test]
fn the_key_is_emitted_only_when_it_is_set() {
    let plain = decode_coding_session_lifecycle_command(&payload("hire-1", hire_action(&[])))
        .expect("a plain hire");
    let reserialized = serde_json::to_string(&plain).expect("reserialize");
    assert!(
        !reserialized.contains("requestedBy"),
        "an unrequested hire must not write the key: {reserialized}"
    );

    let attributed = decode_coding_session_lifecycle_command(&payload(
        "hire-1",
        hire_action(&[("requestedBy", json!(requester()))]),
    ))
    .expect("a requested hire");
    let reserialized = serde_json::to_string(&attributed).expect("reserialize");
    assert_eq!(reserialized.matches("requestedBy").count(), 1);

    let plain_create =
        decode_coding_session_lifecycle_command(&payload("create-1", create_action(&[])))
            .expect("a plain create");
    assert!(!serde_json::to_string(&plain_create)
        .expect("reserialize")
        .contains("hireRef"));
}

/// Every vector in the shared fixture decodes exactly as it is labelled. This
/// is the file B3's TypeScript decoder is pinned to, so a divergence here is a
/// divergence between the two readers of the same signed bytes.
#[test]
fn the_shared_fixture_decodes_exactly_as_labelled() {
    let fixture = fixture();
    let vectors = fixture["vectors"].as_array().expect("vectors");
    assert_eq!(vectors.len(), 13, "the fixture names thirteen vectors");
    for vector in vectors {
        let name = vector["name"].as_str().expect("vector name");
        let expected = vector["valid"].as_bool().expect("vector validity");
        let content = vector["content"].to_string();
        assert_eq!(
            decode_coding_session_lifecycle_command(&content).is_ok(),
            expected,
            "vector {name}"
        );
    }
    assert_eq!(fixture["requestedBy"].as_str(), Some(requester().as_str()));
    assert_eq!(fixture["hireRef"].as_str(), Some(hire_event_id().as_str()));
}

/// **REVIEW-B1 F1.** The fixture's `note` is a cross-lane contract — B3's
/// TypeScript decoder is pinned to this file — so the sentences it quotes must
/// be the sentences the decoder actually produces. It previously claimed the
/// cross-key cases were "refused, by name" when they were not.
#[test]
fn the_fixture_note_quotes_what_the_decoder_actually_says() {
    let fixture = fixture();
    let note = fixture["note"].as_str().expect("fixture note");

    for (command, action) in [
        (
            "create-1",
            create_action(&[("requestedBy", json!(requester()))]),
        ),
        (
            "hire-1",
            hire_action(&[("hireRef", json!(hire_event_id()))]),
        ),
    ] {
        let error = decode_coding_session_lifecycle_command(&payload(command, action))
            .expect_err("a foreign additive key is refused");
        assert!(
            note.contains(&error),
            "the fixture note must quote the decoder verbatim.\n  decoder: {error}\n  note does not contain it"
        );
    }

    // And the note must not carry the unactionable sentence as if it were the
    // answer a consumer should expect.
    assert!(
        note.contains("REVIEW-B1 F1"),
        "the note records why the wording changed"
    );
    assert!(
        note.contains("THE RELAY DOES NOT CHECK THIS CLAIM"),
        "the note must disclose that requestedBy is unverified (REVIEW-B1 F8)"
    );
}
