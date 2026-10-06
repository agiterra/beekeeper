use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag};
use serde_json::{json, Value};

use super::*;

fn signed(content: &str, keys: &Keys) -> Value {
    let event = EventBuilder::new(Kind::Custom(44_201), content)
        .tags([Tag::parse(["h", "36411e44-0e2d-4cfe-bd6e-567eb169db9f"]).expect("tag")])
        .sign_with_keys(keys)
        .expect("sign");
    serde_json::from_str(&event.as_json()).expect("json")
}

#[test]
fn a_signed_event_is_valid() {
    let keys = Keys::generate();
    assert_eq!(
        verify_event_value(signed("hello", &keys)),
        EventSignatureVerdict::Valid
    );
}

#[test]
fn escapes_hash_the_same_as_json_stringify() {
    // Control characters, quotes, backslashes, non-ASCII and the characters
    // JSON.stringify leaves alone (`/`, U+2028, DEL) all have to hash to the
    // id nostr-tools computes, or a real event would read as unchecked.
    let keys = Keys::generate();
    let content = "q\"b\\s/\u{0001}\u{001f}\u{007f}\n\t\r\u{0008}\u{000c} é 🐝 \u{2028}";
    assert_eq!(
        verify_event_value(signed(content, &keys)),
        EventSignatureVerdict::Valid
    );
}

#[test]
fn tampered_content_is_unchecked_not_a_verdict() {
    let keys = Keys::generate();
    let mut event = signed("original", &keys);
    event["content"] = json!("tampered");
    assert_eq!(verify_event_value(event), EventSignatureVerdict::Unchecked);
}

#[test]
fn tampered_tags_are_unchecked() {
    let keys = Keys::generate();
    let mut event = signed("original", &keys);
    event["tags"] = json!([["p", "33".repeat(32)]]);
    assert_eq!(verify_event_value(event), EventSignatureVerdict::Unchecked);
}

#[test]
fn tampered_signature_is_invalid() {
    let keys = Keys::generate();
    let mut event = signed("original", &keys);
    event["sig"] = json!("11".repeat(64));
    assert_eq!(
        verify_event_value(event),
        EventSignatureVerdict::InvalidSignature
    );
}

#[test]
fn another_signers_signature_is_invalid() {
    let keys = Keys::generate();
    let other = signed("original", &Keys::generate());
    let mut event = signed("original", &keys);
    event["sig"] = other["sig"].clone();
    assert_eq!(
        verify_event_value(event),
        EventSignatureVerdict::InvalidSignature
    );
}

#[test]
fn an_upper_case_id_is_not_the_canonical_id() {
    let keys = Keys::generate();
    let mut event = signed("original", &keys);
    let upper = event["id"].as_str().expect("id").to_uppercase();
    event["id"] = json!(upper);
    assert_eq!(verify_event_value(event), EventSignatureVerdict::Unchecked);
}

#[test]
fn malformed_events_are_unchecked() {
    for value in [
        json!(null),
        json!({}),
        json!({ "id": "x", "pubkey": "y", "created_at": -1, "kind": 1, "tags": [], "content": "", "sig": "z" }),
        json!({ "id": "x", "pubkey": "y", "created_at": 1, "kind": 1.5, "tags": [], "content": "", "sig": "z" }),
        json!({ "id": "x", "pubkey": "y", "created_at": 1, "kind": 1, "tags": [[1]], "content": "", "sig": "z" }),
    ] {
        assert_eq!(verify_event_value(value), EventSignatureVerdict::Unchecked);
    }
}

#[test]
fn a_garbage_pubkey_under_a_matching_id_is_invalid() {
    // The id is honestly the hash of these fields, so the verdict is about
    // (id, sig): the signature cannot verify under a key that does not parse.
    let fields = json!([0, "zz", 1, 1, [], ""]);
    let id = hex::encode(Sha256::digest(fields.to_string().as_bytes()));
    let event = json!({
        "id": id, "pubkey": "zz", "created_at": 1, "kind": 1,
        "tags": [], "content": "", "sig": "11".repeat(64),
    });
    assert_eq!(
        verify_event_value(event),
        EventSignatureVerdict::InvalidSignature
    );
}

#[test]
fn batch_order_is_preserved_across_workers() {
    let keys = Keys::generate();
    let mut events = Vec::new();
    let mut expected = Vec::new();
    for i in 0..(PARALLEL_THRESHOLD * 5 + 3) {
        let mut event = signed(&format!("event {i}"), &keys);
        let verdict = match i % 3 {
            0 => EventSignatureVerdict::Valid,
            1 => {
                event["content"] = json!("forged");
                EventSignatureVerdict::Unchecked
            }
            _ => {
                event["sig"] = json!("22".repeat(64));
                EventSignatureVerdict::InvalidSignature
            }
        };
        events.push(event);
        expected.push(verdict);
    }
    assert_eq!(verify_event_values(events), expected);
}

#[test]
fn verdicts_serialize_as_kebab_case() {
    assert_eq!(
        serde_json::to_string(&[
            EventSignatureVerdict::Valid,
            EventSignatureVerdict::InvalidSignature,
            EventSignatureVerdict::Unchecked,
        ])
        .expect("json"),
        r#"["valid","invalid-signature","unchecked"]"#
    );
}

#[test]
fn an_event_signed_by_nostr_tools_verifies_natively() {
    // Signed by nostr-tools `finalizeEvent` (the webview's own library) with
    // every escape class JSON.stringify treats specially; proves the native id
    // recomputation is byte-identical to the JavaScript one.
    let event = json!({
        "kind": 44201,
        "created_at": 1_790_909_152u64,
        "tags": [["h", "c"], ["e", "x", "", "root"]],
        "content": "q\"b\\s/\u{1}\u{1f}\u{7f}\n\t\r\u{8}\u{c} é 🐝 \u{2028} <>&'",
        "pubkey": "989c0b76cb563971fdc9bef31ec06c3560f3249d6ee9e5d83c57625596e05f6f",
        "id": "d0864c6f7218d69291db705a4db2682cad3259fed5ae2268d98225c3a983cf46",
        "sig": "3fa578cecb084d9e9e49a79a54385346ab1635cdd5ad1fbaa7aafc89f0fabba6f1f227d369a2203e8185bff48e29da5f46dd935dcf3dca935ae12b097440aabd",
    });
    assert_eq!(verify_event_value(event), EventSignatureVerdict::Valid);
}
