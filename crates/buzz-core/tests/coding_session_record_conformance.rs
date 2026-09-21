//! The shared closed-record vectors, run against this crate's strict decoders.
//!
//! One file per record under `conformance/coding-session-records/`, loaded by
//! every strict reader in every language (`conformance/README.md`). A lane that
//! adds a key to one of these records adds a vector first and watches every
//! reader's test fail; that is the rule ledger 204 cost us
//! (`docs/UNIFIED_WORK_PLAN.md` § 8 A3.3).
//!
//! Each vector states what *each* reader does with it, so a disagreement
//! between two readers is pinned in the fixture rather than discovered live.
//! This test asserts only the `rust` expectation; the desktop and mobile tests
//! assert theirs. Where they differ the fixture says so and the divergence is
//! visible to anyone who opens the file.

use serde_json::Value;

/// Load one record's vectors, checking the envelope it must carry.
fn fixture(record_dir: &str, kind: u64) -> Value {
    let path = format!(
        "{}/../../conformance/coding-session-records/{record_dir}/fixtures/vectors.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("conformance fixture {path} must be readable: {error}");
    });
    let fixture: Value = serde_json::from_str(&raw).expect("fixture parses");
    assert_eq!(
        fixture["schema"].as_str(),
        Some("buzz-coding-session-record-conformance/v1"),
        "{record_dir} carries the conformance envelope"
    );
    assert_eq!(fixture["kind"].as_u64(), Some(kind));
    assert!(
        fixture["readers"]["rust"].is_string(),
        "{record_dir} names this crate's reader"
    );
    fixture
}

/// Run every vector in `record_dir` through `decode`, comparing against the
/// vector's own `accepts.rust`.
///
/// A vector whose `accepts.rust` is `null` names a variant this reader does
/// not read at all; there is nothing for it to agree or disagree with.
fn run(record_dir: &str, kind: u64, decode: impl Fn(&str) -> Result<(), String>) {
    let fixture = fixture(record_dir, kind);
    let vectors = fixture["vectors"].as_array().expect("vectors is an array");
    assert!(
        vectors.len() >= 10,
        "{record_dir} carries a real vector set, not a token one"
    );
    let mut checked = 0usize;
    for vector in vectors {
        let name = vector["name"].as_str().expect("every vector is named");
        assert!(
            vector["why"].as_str().is_some_and(|why| !why.is_empty()),
            "vector {name:?} says why it exists"
        );
        let Some(expected) = vector["accepts"]["rust"].as_bool() else {
            assert!(
                vector["accepts"]["rust"].is_null(),
                "vector {name:?} states this reader's verdict as a bool or null"
            );
            continue;
        };
        if !expected {
            assert!(
                vector["reason"].as_str().is_some(),
                "refused vector {name:?} names its reason class"
            );
        }
        let content = vector["content"].to_string();
        let actual = decode(&content);
        assert_eq!(
            actual.is_ok(),
            expected,
            "vector {name:?} of {record_dir}: this crate's decoder disagreed with the \
             fixture. Decoder said {actual:?}. Either the fixture is wrong, or a reader \
             changed without its vectors — see conformance/README.md"
        );
        checked += 1;
    }
    assert!(checked > 0, "{record_dir} exercised this reader");
}

/// Every refusal class a vector may name, so a typo cannot silently become a
/// new category nobody greps for.
const REASON_CLASSES: &[&str] = &["unknown-key", "missing-required", "wrong-type"];

#[test]
fn every_record_declares_a_known_refusal_class_and_names_every_reader() {
    for (dir, kind) in [
        ("44221-lifecycle-command", 44221),
        ("44223-metadata", 44223),
        ("44224-lifecycle-receipt", 44224),
        ("44226-genesis", 44226),
        ("44230-closure", 44230),
    ] {
        let fixture = fixture(dir, kind);
        let readers = fixture["readers"]
            .as_object()
            .expect("readers is an object");
        assert!(readers.len() >= 2, "{dir} names more than one reader");
        for vector in fixture["vectors"].as_array().expect("vectors") {
            let name = vector["name"].as_str().expect("named");
            let accepts = vector["accepts"].as_object().expect("accepts is an object");
            for reader in readers.keys() {
                assert!(
                    accepts.contains_key(reader),
                    "vector {name:?} of {dir} says nothing about reader {reader:?}: a reader \
                     with no stated verdict is a reader nobody checked"
                );
            }
            if let Some(reason) = vector["reason"].as_str() {
                assert!(
                    REASON_CLASSES.contains(&reason),
                    "vector {name:?} of {dir} names refusal class {reason:?}, which is not one \
                     of {REASON_CLASSES:?}"
                );
            }
        }
    }
}

#[test]
fn lifecycle_command_vectors_match_this_decoder() {
    run("44221-lifecycle-command", 44221, |content| {
        buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            content,
        )
        .map(|_| ())
    });
}

#[test]
fn metadata_vectors_match_this_decoder() {
    run("44223-metadata", 44223, |content| {
        buzz_core::coding_session_payload::decode_coding_session_metadata(content).map(|_| ())
    });
}

#[test]
fn lifecycle_receipt_vectors_match_this_decoder() {
    run("44224-lifecycle-receipt", 44224, |content| {
        buzz_core::coding_session_payload::decode_coding_session_lifecycle_receipt(content)
            .map(|_| ())
    });
}

#[test]
fn genesis_vectors_match_this_decoder() {
    run("44226-genesis", 44226, |content| {
        buzz_core::coding_session_genesis::decode_coding_session_genesis(content).map(|_| ())
    });
}

#[test]
fn closure_vectors_match_this_decoder() {
    run("44230-closure", 44230, |content| {
        buzz_core::coding_session_closure::decode_coding_session_closure(content).map(|_| ())
    });
}
