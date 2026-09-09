//! Wire tests for kind 44247: the exact shapes, and every refusal by name.
//!
//! Split into its own file so no module here passes 1,000 lines.

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::json;

use super::*;

const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";

fn genesis() -> String {
    "ce".repeat(32)
}

fn checkpoint_body() -> Value {
    json!({
        "prevCheckpointRef": null,
        "task": "Land the handover fold",
        "assignmentRefs": ["ab".repeat(32)],
        "decisions": [{ "eventId": "cd".repeat(32), "summary": "Claim is umbrella-wide" }],
        "revision": {
            "repoRef": "30617:aa/beekeeper",
            "baseSha": "1a".repeat(20),
            "headSha": "2b".repeat(20),
            "branch": "work/handover",
            "dirty": true,
            "preserved": "partial"
        },
        "artifacts": [
            {
                "kind": "wip-ref",
                "repoRef": "30617:aa/beekeeper",
                "ref": "refs/heads/wip/builder/1f2e3d4c",
                "sha": "2b".repeat(20)
            },
            {
                "kind": "patch",
                "repoRef": "30617:aa/beekeeper",
                "eventId": "ef".repeat(32),
                "baseSha": "2b".repeat(20),
                "bytes": 4096
            }
        ],
        "tests": [{ "name": "core", "command": "cargo test -p buzz-core", "outcome": "passed" }],
        "unresolved": ["Does a voided claim fence sibling executions?"],
        "nextAction": "Wire the relay envelope validator",
        "missing": ["target/ artefacts on the author's disk"]
    })
}

fn continuation_body() -> Value {
    json!({
        "claimRef": "11".repeat(32),
        "mode": "reconstructed",
        "checkpointRef": "22".repeat(32),
        "target": {
            "driver": "claude-agent-acp",
            "instanceId": "provider-b",
            "sessionId": "sess-1",
            "generation": 1
        },
        "recovered": ["wip-ref refs/heads/wip/builder/1f2e3d4c at 2b2b"],
        "missing": ["uncommitted changes on A's machine"],
        "note": null
    })
}

fn payload_json(record_type: &str, body: Value) -> String {
    json!({
        "schema": CODING_SESSION_HANDOVER_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "type": record_type,
        "body": body,
    })
    .to_string()
}

fn signed(record_type: &str, body: Value, tags: Vec<Tag>) -> nostr::Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_HANDOVER as u16),
        payload_json(record_type, body),
    )
    .tags(tags)
    .sign_with_keys(&Keys::generate())
    .expect("sign")
}

fn envelope_tags(record_type: &str) -> Vec<Tag> {
    vec![
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", SESSION]).expect("d"),
        Tag::parse(["csh-v", CODING_SESSION_HANDOVER_TAG_VERSION]).expect("csh-v"),
        Tag::parse(["csh-genesis", &genesis()]).expect("csh-genesis"),
        Tag::parse(["csh-type", record_type]).expect("csh-type"),
    ]
}

#[test]
fn a_checkpoint_decodes_and_round_trips() {
    let content = payload_json("checkpoint", checkpoint_body());
    let payload = decode_coding_session_handover(&content).expect("a valid checkpoint");
    assert_eq!(payload.session_ref, SESSION);
    assert_eq!(payload.handover_type, CodingSessionHandoverType::Checkpoint);
    let CodingSessionHandoverBody::Checkpoint(body) = &payload.body else {
        panic!("expected a checkpoint body");
    };
    assert_eq!(body.artifacts.len(), 2);
    assert_eq!(
        body.revision.preserved,
        CodingSessionHandoverPreserved::Partial
    );
    // Re-serializing produces bytes the decoder accepts again: the struct is
    // the wire, not a lossy projection of it.
    let round_tripped = serde_json::to_string(&payload).expect("serialize");
    assert_eq!(
        decode_coding_session_handover(&round_tripped).expect("re-decode"),
        payload
    );
}

#[test]
fn a_continuation_decodes_and_keeps_its_target() {
    let content = payload_json("continuation", continuation_body());
    let payload = decode_coding_session_handover(&content).expect("a valid continuation");
    let CodingSessionHandoverBody::Continuation(body) = &payload.body else {
        panic!("expected a continuation body");
    };
    assert_eq!(body.mode, CodingSessionHandoverMode::Reconstructed);
    assert_eq!(body.mode.as_str(), "reconstructed");
    assert_eq!(body.target.instance_id, "provider-b");
    assert!(body.note.is_none());
}

#[test]
fn the_schema_type_and_body_must_agree() {
    let mut wrong_schema: Value =
        serde_json::from_str(&payload_json("checkpoint", checkpoint_body())).expect("json");
    wrong_schema["schema"] = json!("buzz-coding-session-observation/v1");
    assert!(decode_coding_session_handover(&wrong_schema.to_string()).is_err());

    // A checkpoint body filed as a continuation is refused by the exact-key
    // check before the type/body parity rule ever sees it — either way it
    // never decodes.
    let mismatched = payload_json("continuation", checkpoint_body());
    assert!(decode_coding_session_handover(&mismatched).is_err());
}

#[test]
fn unknown_and_missing_keys_are_refused_by_name() {
    let mut smuggled: Value =
        serde_json::from_str(&payload_json("checkpoint", checkpoint_body())).expect("json");
    smuggled["note"] = json!("trust me");
    let error = decode_coding_session_handover(&smuggled.to_string()).expect_err("refused");
    assert!(error.contains("\"note\""), "{error}");

    for key in ["schema", "sessionRef", "genesisRef", "type", "body"] {
        let mut missing: Value =
            serde_json::from_str(&payload_json("checkpoint", checkpoint_body())).expect("json");
        missing.as_object_mut().expect("object").remove(key);
        let error = decode_coding_session_handover(&missing.to_string()).expect_err("refused");
        assert!(error.contains(key), "{key}: {error}");
    }

    // Inside the body, too — an absent `missing` list is not an empty one.
    for key in [
        "prevCheckpointRef",
        "task",
        "assignmentRefs",
        "decisions",
        "revision",
        "artifacts",
        "tests",
        "unresolved",
        "nextAction",
        "missing",
    ] {
        let mut body = checkpoint_body();
        body.as_object_mut().expect("object").remove(key);
        assert!(
            decode_coding_session_handover(&payload_json("checkpoint", body)).is_err(),
            "checkpoint body missing {key} must be refused"
        );
    }
}

#[test]
fn a_required_null_is_refused_and_a_nullable_one_is_not() {
    let mut null_task = checkpoint_body();
    null_task["task"] = Value::Null;
    let error = decode_coding_session_handover(&payload_json("checkpoint", null_task))
        .expect_err("refused");
    assert!(error.contains("null is not a value here"), "{error}");

    // The revision's four coordinates are genuinely nullable: work outside a
    // repository has no branch and no shas.
    let mut bare = checkpoint_body();
    bare["revision"] = json!({
        "repoRef": null,
        "baseSha": null,
        "headSha": null,
        "branch": null,
        "dirty": false,
        "preserved": "all"
    });
    bare["artifacts"] = json!([]);
    bare["missing"] = json!([]);
    assert!(decode_coding_session_handover(&payload_json("checkpoint", bare)).is_ok());
}

#[test]
fn every_closed_vocabulary_names_its_whole_set_when_it_refuses() {
    let mut bad_preserved = checkpoint_body();
    bad_preserved["revision"]["preserved"] = json!("some");
    let error = decode_coding_session_handover(&payload_json("checkpoint", bad_preserved))
        .expect_err("refused");
    assert!(error.contains("\"all\", \"partial\", \"none\""), "{error}");

    let mut bad_artifact = checkpoint_body();
    bad_artifact["artifacts"][0]["kind"] = json!("tarball");
    let error = decode_coding_session_handover(&payload_json("checkpoint", bad_artifact))
        .expect_err("refused");
    assert!(
        error.contains("\"wip-ref\", \"patch\", \"blob\""),
        "{error}"
    );

    let mut bad_outcome = checkpoint_body();
    bad_outcome["tests"][0]["outcome"] = json!("green");
    let error = decode_coding_session_handover(&payload_json("checkpoint", bad_outcome))
        .expect_err("refused");
    assert!(
        error.contains("\"passed\", \"failed\", \"not-run\""),
        "{error}"
    );

    let mut bad_mode = continuation_body();
    bad_mode["mode"] = json!("resumed");
    let error = decode_coding_session_handover(&payload_json("continuation", bad_mode))
        .expect_err("refused");
    assert!(
        error.contains("\"native-resume\", \"reconstructed\""),
        "{error}"
    );

    let bad_type = payload_json("takeover", checkpoint_body());
    let error = decode_coding_session_handover(&bad_type).expect_err("refused");
    assert!(
        error.contains("\"checkpoint\", \"continuation\""),
        "{error}"
    );
}

/// Each artifact kind carries exactly its own fields — a `patch` with a `ref`
/// is a producer inventing a shape, and a `blob` with no hash names nothing.
#[test]
fn an_artifact_carries_exactly_its_kinds_fields() {
    let blob = json!({
        "kind": "blob",
        "repoRef": "30617:aa/beekeeper",
        "hash": "3c".repeat(32),
        "baseSha": "2b".repeat(20),
        "bytes": 2_000_000
    });
    let mut with_blob = checkpoint_body();
    with_blob["artifacts"] = json!([blob.clone()]);
    assert!(decode_coding_session_handover(&payload_json("checkpoint", with_blob)).is_ok());

    for (mutate, expect) in [
        (json!({ "ref": "refs/heads/wip/x" }), "must not carry"),
        (json!({ "sha": "2b".repeat(20) }), "must not carry"),
        (json!({ "eventId": "ef".repeat(32) }), "must not carry"),
    ] {
        let mut artifact = blob.clone();
        for (key, value) in mutate.as_object().expect("object") {
            artifact[key] = value.clone();
        }
        let mut body = checkpoint_body();
        body["artifacts"] = json!([artifact]);
        let error =
            decode_coding_session_handover(&payload_json("checkpoint", body)).expect_err("refused");
        assert!(error.contains(expect), "{error}");
    }

    let mut headless = blob;
    headless.as_object_mut().expect("object").remove("hash");
    let mut body = checkpoint_body();
    body["artifacts"] = json!([headless]);
    let error =
        decode_coding_session_handover(&payload_json("checkpoint", body)).expect_err("refused");
    assert!(error.contains("requires \"hash\""), "{error}");

    let mut wip_with_bytes = checkpoint_body();
    wip_with_bytes["artifacts"][0]["bytes"] = json!(12);
    assert!(decode_coding_session_handover(&payload_json("checkpoint", wip_with_bytes)).is_err());
}

/// The two honesty rules a single checkpoint can enforce on itself.
#[test]
fn preserved_must_agree_with_the_artifacts_and_the_missing_list() {
    let mut contradiction = checkpoint_body();
    contradiction["revision"]["preserved"] = json!("none");
    let error = decode_coding_session_handover(&payload_json("checkpoint", contradiction))
        .expect_err("refused");
    assert!(error.contains("preserved is \"none\""), "{error}");

    let mut silent = checkpoint_body();
    silent["missing"] = json!([]);
    let error =
        decode_coding_session_handover(&payload_json("checkpoint", silent)).expect_err("refused");
    assert!(error.contains("enumerated under missing"), "{error}");

    // "all" with nothing missing is the clean case, and is accepted.
    let mut clean = checkpoint_body();
    clean["revision"]["preserved"] = json!("all");
    clean["missing"] = json!([]);
    assert!(decode_coding_session_handover(&payload_json("checkpoint", clean)).is_ok());
}

#[test]
fn every_bound_is_enforced() {
    let mut long_task = checkpoint_body();
    long_task["task"] = json!("x".repeat(MAX_HANDOVER_TASK_BYTES + 1));
    assert!(decode_coding_session_handover(&payload_json("checkpoint", long_task)).is_err());

    let mut many_decisions = checkpoint_body();
    many_decisions["decisions"] = Value::Array(
        (0..=MAX_HANDOVER_DECISIONS)
            .map(|_| json!({ "eventId": "cd".repeat(32), "summary": "one" }))
            .collect(),
    );
    assert!(decode_coding_session_handover(&payload_json("checkpoint", many_decisions)).is_err());

    let mut many_artifacts = checkpoint_body();
    many_artifacts["artifacts"] = Value::Array(
        (0..=MAX_HANDOVER_ARTIFACTS)
            .map(|_| {
                json!({
                    "kind": "wip-ref",
                    "repoRef": "30617:aa/beekeeper",
                    "ref": "refs/heads/wip/builder/1f2e3d4c",
                    "sha": "2b".repeat(20)
                })
            })
            .collect(),
    );
    assert!(decode_coding_session_handover(&payload_json("checkpoint", many_artifacts)).is_err());

    let mut long_line = checkpoint_body();
    long_line["unresolved"] = json!(["x".repeat(MAX_HANDOVER_LINE_BYTES + 1)]);
    assert!(decode_coding_session_handover(&payload_json("checkpoint", long_line)).is_err());

    let oversize = " ".repeat(MAX_CODING_SESSION_HANDOVER_CONTENT_BYTES + 1);
    assert!(decode_coding_session_handover(&oversize).is_err());
}

#[test]
fn references_must_be_the_widths_they_claim() {
    let mut bad_assignment = checkpoint_body();
    bad_assignment["assignmentRefs"] = json!(["ab".repeat(31)]);
    assert!(decode_coding_session_handover(&payload_json("checkpoint", bad_assignment)).is_err());

    let mut bad_head = checkpoint_body();
    bad_head["revision"]["headSha"] = json!("2B".repeat(20));
    assert!(decode_coding_session_handover(&payload_json("checkpoint", bad_head)).is_err());

    let mut bad_claim = continuation_body();
    bad_claim["claimRef"] = json!("not-hex");
    assert!(decode_coding_session_handover(&payload_json("continuation", bad_claim)).is_err());

    let mut bad_target = continuation_body();
    bad_target["target"]["generation"] = json!(0);
    assert!(decode_coding_session_handover(&payload_json("continuation", bad_target)).is_err());

    let mut bad_session: Value =
        serde_json::from_str(&payload_json("checkpoint", checkpoint_body())).expect("json");
    bad_session["sessionRef"] = json!("DC580CFB-6C80-4FC2-8F4E-DFC328ACF222");
    assert!(decode_coding_session_handover(&bad_session.to_string()).is_err());
}

#[test]
fn the_envelope_is_five_ordered_tags_that_agree_with_the_content() {
    let event = signed("checkpoint", checkpoint_body(), envelope_tags("checkpoint"));
    let payload = validate_coding_session_handover_envelope(&event).expect("a valid envelope");
    assert_eq!(payload.handover_type.as_str(), "checkpoint");

    // Wrong count.
    let short = signed(
        "checkpoint",
        checkpoint_body(),
        envelope_tags("checkpoint")[..4].to_vec(),
    );
    assert!(validate_coding_session_handover_envelope(&short).is_err());

    // Wrong order.
    let mut swapped = envelope_tags("checkpoint");
    swapped.swap(1, 2);
    let reordered = signed("checkpoint", checkpoint_body(), swapped);
    assert!(validate_coding_session_handover_envelope(&reordered).is_err());

    // A `d` that files this record under another umbrella.
    let mut wrong_d = envelope_tags("checkpoint");
    wrong_d[1] = Tag::parse(["d", "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0"]).expect("d");
    let misfiled = signed("checkpoint", checkpoint_body(), wrong_d);
    let error = validate_coding_session_handover_envelope(&misfiled).expect_err("refused");
    assert!(error.contains("d tag does not match"), "{error}");

    // A genesis tag that disagrees with the content.
    let mut wrong_genesis = envelope_tags("checkpoint");
    wrong_genesis[3] = Tag::parse(["csh-genesis", &"99".repeat(32)]).expect("genesis");
    let mismatched = signed("checkpoint", checkpoint_body(), wrong_genesis);
    assert!(validate_coding_session_handover_envelope(&mismatched).is_err());

    // A type tag that indexes a continuation as a checkpoint.
    let mut wrong_type = envelope_tags("checkpoint");
    wrong_type[4] = Tag::parse(["csh-type", "continuation"]).expect("type");
    let mislabelled = signed("checkpoint", checkpoint_body(), wrong_type);
    let error = validate_coding_session_handover_envelope(&mislabelled).expect_err("refused");
    assert!(error.contains("type tag does not match"), "{error}");

    // An unsupported tag version.
    let mut old_version = envelope_tags("checkpoint");
    old_version[2] = Tag::parse(["csh-v", "csh0"]).expect("version");
    let stale = signed("checkpoint", checkpoint_body(), old_version);
    assert!(validate_coding_session_handover_envelope(&stale).is_err());

    // The wrong kind entirely.
    let foreign = EventBuilder::new(
        Kind::Custom(crate::kind::KIND_CODING_SESSION_OBSERVATION as u16),
        payload_json("checkpoint", checkpoint_body()),
    )
    .tags(envelope_tags("checkpoint"))
    .sign_with_keys(&Keys::generate())
    .expect("sign");
    assert!(validate_coding_session_handover_envelope(&foreign).is_err());
}
